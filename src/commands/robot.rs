use anyhow::Result;
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::Config;
use crate::robot::bridge::mock::MockBridge;
use crate::robot::description::RobotDescription;
use crate::robot::runtime::RobotRuntime;

use super::{atty_check, create_agent, send_and_wait, setup_logging, spawn_agent_worker};

pub async fn run(
    config_path: &Path,
    data_dir: &Path,
    robot_config_path: &Path,
    message: Option<String>,
) -> Result<()> {
    setup_logging();
    let config = Config::load(config_path)?;
    let robot_desc = RobotDescription::load(robot_config_path)?;

    tracing::info!(
        "Robot: {} ({})",
        robot_desc.robot.name,
        robot_desc.robot.description
    );

    // Create bridge based on config
    let bridge: Box<dyn crate::robot::bridge::HardwareBridge> =
        match robot_desc.hardware.bridge.as_str() {
            "mock" => Box::new(MockBridge::new()),
            other => {
                tracing::warn!("Unknown bridge '{other}', using mock");
                Box::new(MockBridge::new())
            }
        };

    let mut runtime = RobotRuntime::new(robot_desc, bridge);

    // Create agent with robot context
    let mut agent = create_agent(&config, data_dir).await?;
    agent.set_robot_context(runtime.description().to_system_prompt(), runtime.world_rx());
    agent.action_tx = Some(runtime.action_tx());
    agent.world_rx = Some(runtime.world_rx());

    let inbound_tx = spawn_agent_worker(agent);

    // Start runtime tasks (sensor polling + action executor)
    let _runtime_tasks = runtime.start().await;

    // Single-shot mode
    if let Some(msg) = message {
        let output = send_and_wait(&inbound_tx, &msg, "robot").await?;
        println!("{}", output.content);
        return Ok(());
    }

    // REPL mode
    let is_tty = atty_check();
    let robot_name = runtime.description().robot.name.clone();
    if is_tty {
        println!(
            "UniClaw Robot v{} | {}",
            env!("CARGO_PKG_VERSION"),
            robot_name
        );
        println!("Type 'exit' to quit.\n");
    }

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        if is_tty {
            print!("You> ");
            std::io::stdout().flush()?;
        }

        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
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

        match send_and_wait(&inbound_tx, trimmed, "robot").await {
            Ok(output) => {
                if is_tty {
                    println!("{}> {}\n", robot_name, output.content);
                } else {
                    println!("{}", output.content);
                }
            }
            Err(e) => eprintln!("Error: {e}"),
        }
    }

    Ok(())
}
