mod agent;
mod config;
mod llm;
mod tools;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use agent::{Agent, Input, Output};
use config::Config;

#[derive(Parser)]
#[command(name = "miniclaw", version, about = "Privacy-first AI agent for ARM Linux SBCs")]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "config/config.toml")]
    config: PathBuf,

    /// Path to data directory
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize data directories and default config
    Init,
    /// Start an interactive chat session
    Chat {
        /// Single message (non-interactive mode)
        #[arg(long, short)]
        message: Option<String>,
        /// Session ID (default: "cli")
        #[arg(long, default_value = "cli")]
        session: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => run_init(&cli.config, &cli.data_dir),
        Commands::Chat { message, session } => {
            run_chat(&cli.config, &cli.data_dir, message, &session).await
        }
    }
}

fn run_init(config_path: &PathBuf, data_dir: &PathBuf) -> Result<()> {
    println!("Initializing MiniClaw...");

    // Create directories
    let dirs = [
        data_dir.to_path_buf(),
        data_dir.join("memory"),
        data_dir.join("sessions"),
        data_dir.join("skills"),
        PathBuf::from("config"),
        PathBuf::from("logs"),
    ];
    for dir in &dirs {
        std::fs::create_dir_all(dir)?;
        println!("  Created {}/", dir.display());
    }

    // Write default SOUL.md if missing
    let soul_path = data_dir.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(&soul_path, agent::context::DEFAULT_SOUL)?;
        println!("  Written {}", soul_path.display());
    }

    // Write default config if missing
    if !config_path.exists() {
        let default_config = r#"[agent]
max_iterations = 10
max_tool_calls_per_iteration = 4
consolidation_threshold = 40
context_cache_ttl_secs = 60

[llm]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-6"
base_url = "https://api.anthropic.com"
max_tokens = 1024
temperature = 0.7
timeout_secs = 60

# Optional fallback provider (e.g., local Ollama)
# [llm.fallback]
# provider = "openai_compatible"
# base_url = "http://localhost:11434"
# model = "qwen3:0.6b"
# api_key_env = ""

[logging]
level = "info"
"#;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(config_path, default_config)?;
        println!("  Written {}", config_path.display());
    }

    // Write empty MEMORY.md
    let memory_path = data_dir.join("memory/MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "")?;
    }

    println!("\nPlease set your API key:");
    println!("  export ANTHROPIC_API_KEY=\"your-key-here\"");
    println!("\nThen run:");
    println!("  ./miniclaw chat");
    Ok(())
}

async fn run_chat(
    config_path: &PathBuf,
    data_dir: &PathBuf,
    message: Option<String>,
    session_id: &str,
) -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Load config
    let config = Config::load(config_path)?;

    // Ensure data dirs exist
    std::fs::create_dir_all(data_dir.join("memory"))?;
    std::fs::create_dir_all(data_dir.join("sessions"))?;
    std::fs::create_dir_all(data_dir.join("skills"))?;

    // Create LLM providers
    let primary = llm::create_provider(&config.llm)?;
    let fallback = config
        .llm
        .fallback
        .as_ref()
        .and_then(|f| llm::create_provider(f).ok());

    // Create tool registry
    let mut tool_registry = tools::registry::ToolRegistry::new();
    tools::register_default_tools(&mut tool_registry);

    // Create agent
    let mut agent = Agent::new(primary, fallback, tool_registry, &config, data_dir.clone());

    // Single-shot mode
    if let Some(msg) = message {
        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            content: msg,
        };
        let output = agent.process(&input).await?;
        println!("{}", output.content);
        return Ok(());
    }

    // REPL mode
    let is_tty = atty_check();
    if is_tty {
        println!(
            "MiniClaw v{} | {} | {}",
            env!("CARGO_PKG_VERSION"),
            config.llm.model,
            std::env::consts::ARCH
        );
        println!("Type 'exit' or Ctrl+C to quit.\n");
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        if is_tty {
            print!("You> ");
            io::stdout().flush()?;
        }

        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            if is_tty {
                println!("Goodbye!");
            }
            break;
        }

        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            content: trimmed.to_string(),
        };

        match agent.process(&input).await {
            Ok(output) => {
                if is_tty {
                    println!("MiniClaw> {}\n", output.content);
                } else {
                    println!("{}", output.content);
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }

    // Persist sessions on exit
    agent.session_store.persist_all()?;

    Ok(())
}

/// Check if stdin is a TTY (interactive terminal)
fn atty_check() -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc_isatty(io::stdin().as_raw_fd()) != 0 }
}

extern "C" {
    fn isatty(fd: i32) -> i32;
}

unsafe fn libc_isatty(fd: i32) -> i32 {
    unsafe { isatty(fd) }
}
