# MiniClaw — Phase 1 Implementation Plan

> **Goal**: Working ReAct agent on Mac + RPi. Text in, text out. CLI only.
>
> **Estimated**: 5-7 working days | **Binary**: ~3-5MB | **RAM**: ~15-20MB

---

## Implementation Order & Dependency Graph

```
Step 1: Scaffold
  Cargo.toml, directory structure, deploy.sh
  │
Step 2: Config
  config.rs (TOML loading, Config struct)
  │
Step 3: LLM Types
  llm/types.rs (canonical Message, ToolCall, ChatResponse)
  │
  ├── Step 4a: Anthropic Provider
  │   llm/anthropic.rs (serialize/deserialize Anthropic format)
  │
  ├── Step 4b: OpenAI Provider
  │   llm/openai.rs (serialize/deserialize OpenAI format)
  │
  └── Step 4c: Provider Trait + Failover
      llm/mod.rs (LlmProvider trait, provider construction, failover)
      │
Step 5: Tool System
  tools/registry.rs (Tool trait, ToolRegistry, JSON schema gen)
  │
  ├── Step 6a: get_time tool
  ├── Step 6b: read_file tool
  ├── Step 6c: write_file tool
  ├── Step 6d: list_dir tool
  └── Step 6e: system_info tool
      │
Step 7: Context Builder
  agent/context.rs (assemble system prompt from files, cache, budget)
  │
Step 8: Session Store
  agent/memory.rs (Session, SessionStore, JSONL persistence)
  │
Step 9: Agent Loop
  agent/loop.rs (ReAct loop, parallel tool exec, agent_worker)
  │
Step 10: CLI + Main
  main.rs (clap CLI, init command, REPL + single-shot, signal handling)
  │
Step 11: Integration Tests
  tests/ (MockLlmClient, end-to-end agent tests)
  │
Step 12: Cross-Compile & Deploy
  Verify on RPi 4, measure binary size + RAM
```

---

## Step 1: Scaffold

**Files created**: `Cargo.toml`, `src/main.rs` (stub), `deploy.sh`, `.gitignore`, directory structure

### Cargo.toml

```toml
[package]
name = "miniclaw"
version = "0.1.0"
edition = "2021"
description = "Privacy-first AI agent OS for ARM Linux SBCs"
license = "MIT"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
futures = "0.3"

# HTTP client (for LLM API)
reqwest = { version = "0.12", default-features = false, features = [
    "json", "stream", "rustls-tls"
] }

# HTTP server (Phase 3, but axum is used in Phase 1 for future-proofing imports)
# axum = "0.8"  # uncomment in Phase 3

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# CLI
clap = { version = "4", features = ["derive"] }

# Async utilities
async-trait = "0.1"
tokio-stream = "0.1"

# Error handling
anyhow = "1"
thiserror = "2"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Misc
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tempfile = "3"

[features]
default = []
# Future phases:
# privacy = ["ort"]
# safety = []
# voice = []
# rpi = []
# mcp = []

[profile.release]
lto = true
panic = "abort"
strip = true
codegen-units = 1
opt-level = "z"
```

### Directory structure

```
miniclaw/
├── Cargo.toml
├── deploy.sh
├── .gitignore
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── agent/
│   │   ├── mod.rs
│   │   ├── loop.rs
│   │   ├── context.rs
│   │   └── memory.rs
│   ├── llm/
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   ├── anthropic.rs
│   │   └── openai.rs
│   └── tools/
│       ├── mod.rs
│       ├── registry.rs
│       ├── get_time.rs
│       ├── file_ops.rs
│       └── system.rs
├── config/
│   └── config.toml
├── data/
│   ├── SOUL.md
│   └── memory/
├── tests/
│   └── agent_test.rs
└── .gitignore
```

### deploy.sh

```bash
#!/bin/bash
set -e
RPI_HOST="${RPI_HOST:-rpi4.local}"
RPI_USER="${RPI_USER:-pi}"
RPI_DIR="${RPI_DIR:-/home/pi/miniclaw}"

echo "Building for aarch64..."
cargo zigbuild --target aarch64-unknown-linux-gnu --release

echo "Deploying to ${RPI_USER}@${RPI_HOST}..."
rsync -avz --progress \
  target/aarch64-unknown-linux-gnu/release/miniclaw \
  ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/

rsync -avz --progress \
  config/ ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/config/
rsync -avz --progress \
  data/ ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/data/

echo "Done. Run on RPi:"
echo "  ssh ${RPI_USER}@${RPI_HOST} 'cd ${RPI_DIR} && ./miniclaw chat'"
```

### .gitignore

```
/target
*.swp
*.swo
.env
logs/
```

### Validation

```bash
cargo build          # compiles
cargo run -- --help  # shows CLI help (stub)
```

---

## Step 2: Config

**File**: `src/config.rs`

### What it does

- Loads `config/config.toml` into a typed `Config` struct
- API keys read from environment variables (never stored in config)
- Validates required fields
- Provides defaults for optional fields

### Structs

```rust
#[derive(Debug, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub llm: LlmConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,               // 10
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls_per_iteration: usize,  // 4
    #[serde(default = "default_consolidation")]
    pub consolidation_threshold: usize,       // 40
    #[serde(default = "default_cache_ttl")]
    pub context_cache_ttl_secs: u64,          // 60
    #[serde(default = "default_memory_max")]
    pub memory_max_bytes: usize,              // 8192
    #[serde(default = "default_session_age")]
    pub session_max_age_days: u64,            // 30
    #[serde(default = "default_session_count")]
    pub session_max_count: usize,             // 100
}

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub provider: String,           // "anthropic" | "openai_compatible"
    #[serde(default)]
    pub api_key_env: String,        // env var name
    pub model: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,            // 1024
    #[serde(default = "default_temperature")]
    pub temperature: f32,           // 0.7
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,          // 60
    pub fallback: Option<LlmConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub shell_enabled: bool,
    #[serde(default)]
    pub shell_allowed_commands: Vec<String>,
    #[serde(default = "default_shell_timeout")]
    pub shell_timeout_secs: u64,
    #[serde(default = "default_true")]
    pub http_fetch_enabled: bool,
    #[serde(default = "default_http_timeout")]
    pub http_fetch_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,              // "info"
    pub file: Option<String>,
}
```

### Key methods

```rust
impl Config {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn data_dir(&self) -> PathBuf;   // resolve data directory
}

impl LlmConfig {
    pub fn api_key(&self) -> Result<String>;  // reads from env var
}
```

### Default config.toml

```toml
[agent]
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

[logging]
level = "info"
```

### Tests

```rust
#[test]
fn test_load_config() { /* parse valid TOML */ }

#[test]
fn test_config_defaults() { /* missing optional fields get defaults */ }

#[test]
fn test_api_key_from_env() { /* set env, read key */ }

#[test]
fn test_missing_api_key_error() { /* unset env, get error */ }
```

### Validation

```bash
cargo test config    # all config tests pass
cargo run            # loads config without crashing
```

---

## Step 3: LLM Types

**File**: `src/llm/types.rs`

### What it does

Defines the canonical types shared between the agent loop and all LLM providers. This is the internal API — providers convert to/from these types.

### Types

```rust
/// Context sent to the LLM
pub struct Context {
    pub system: String,                  // system prompt
    pub messages: Vec<Message>,          // conversation history
    pub tool_schemas: Vec<ToolSchema>,   // available tools
}

/// A single message in the conversation
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

pub enum Role { User, Assistant, Tool }

pub enum MessageContent {
    Text(String),
    ToolUse { text: Option<String>, tool_calls: Vec<ToolCall> },
    ToolResult { tool_use_id: String, content: String },
}

/// A tool call from the LLM
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool schema for LLM
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}

/// LLM response (parsed from provider-specific format)
pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

pub enum StopReason { EndTurn, ToolUse, MaxTokens }

pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
```

Also: `Serialize`/`Deserialize` impls for session persistence, `Clone` where needed, helper constructors.

### Key helper methods

```rust
impl Context {
    /// Create a simple context for one-off queries (e.g., memory consolidation)
    pub fn simple_query(prompt: &str) -> Self;
}

impl Message {
    pub fn user(text: &str) -> Self;
    pub fn assistant(text: &str) -> Self;
    pub fn tool_result(id: &str, content: &str) -> Self;

    /// Extract text content for display/logging
    pub fn content_text(&self) -> &str;
}
```

### Tests

```rust
#[test]
fn test_message_serialization() { /* roundtrip Message through serde_json */ }

#[test]
fn test_context_simple_query() { /* verify helper creates valid context */ }
```

---

## Step 4a: Anthropic Provider

**File**: `src/llm/anthropic.rs`

### What it does

Converts between canonical types and Anthropic Messages API JSON format. Handles HTTP requests to `api.anthropic.com`.

### Key implementation details

**Request serialization** (canonical → Anthropic JSON):
- `Context.system` → `"system"` field (separate from messages)
- `Message::User` with `Text` → `{"role": "user", "content": "..."}`
- `Message::Assistant` with `ToolUse` → `{"role": "assistant", "content": [{"type": "text", ...}, {"type": "tool_use", "id": ..., "name": ..., "input": ...}]}`
- `Message::Tool` with `ToolResult` → `{"role": "user", "content": [{"type": "tool_result", "tool_use_id": ..., "content": "..."}]}`
- `ToolSchema` → `{"name": ..., "description": ..., "input_schema": ...}`

**Response deserialization** (Anthropic JSON → canonical):
- `content[].type == "text"` → `ChatResponse.text`
- `content[].type == "tool_use"` → `ChatResponse.tool_calls`
- `stop_reason: "end_turn"` → `StopReason::EndTurn`
- `stop_reason: "tool_use"` → `StopReason::ToolUse`
- `stop_reason: "max_tokens"` → `StopReason::MaxTokens`
- `usage.input_tokens`, `usage.output_tokens` → `Usage`

**HTTP call**:
```
POST {base_url}/v1/messages
Headers:
  x-api-key: {api_key}
  anthropic-version: 2023-06-01
  content-type: application/json
Body: serialized request
```

### Struct

```rust
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}
```

### Tests

```rust
#[test]
fn test_serialize_simple_request() {
    /* canonical Context → Anthropic JSON, verify structure */
}

#[test]
fn test_serialize_tool_call_request() {
    /* Context with tool schemas → correct Anthropic tools format */
}

#[test]
fn test_deserialize_text_response() {
    /* Anthropic JSON with text → ChatResponse */
}

#[test]
fn test_deserialize_tool_use_response() {
    /* Anthropic JSON with tool_use blocks → ChatResponse with tool_calls */
}

#[test]
fn test_deserialize_mixed_response() {
    /* Anthropic JSON with text + tool_use → both text and tool_calls populated */
}
```

---

## Step 4b: OpenAI Provider

**File**: `src/llm/openai.rs`

### What it does

Same as Anthropic but for the OpenAI Chat Completions format. Covers OpenAI, Ollama, Groq, DeepSeek, OpenRouter, vLLM, etc.

### Key differences from Anthropic

- System prompt → `{"role": "system", "content": "..."}` message (first in array)
- Tool calls → `assistant.tool_calls[].function.{name, arguments}`
- Tool results → `{"role": "tool", "tool_call_id": ..., "content": "..."}`
- Stop reason → `finish_reason: "stop"` / `"tool_calls"`
- Usage → `prompt_tokens` / `completion_tokens`
- Tool schema → `{"type": "function", "function": {"name": ..., "description": ..., "parameters": ...}}`

### Tests

Same pattern as Anthropic: serialize/deserialize request/response roundtrips.

---

## Step 4c: Provider Trait + Failover

**File**: `src/llm/mod.rs`

### What it does

- Defines `LlmProvider` trait
- Factory function: `create_provider(config: &LlmConfig) -> Box<dyn LlmProvider>`
- Re-exports types

### Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
    // chat_stream added in Phase 3 (for SSE endpoint)
}
```

### Factory

```rust
pub fn create_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    match config.provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(config)?)),
        "openai_compatible" | "openai" => Ok(Box::new(OpenAiProvider::new(config)?)),
        other => Err(anyhow!("Unknown LLM provider: {other}")),
    }
}
```

### Validation (manual)

```bash
# Test with real API (requires API key)
ANTHROPIC_API_KEY=sk-... cargo run -- chat --message "Say hello"
```

---

## Step 5: Tool System

**File**: `src/tools/registry.rs`

### What it does

- `Tool` trait definition
- `ToolRegistry`: register tools, generate JSON schemas, dispatch calls
- `ToolContext`: shared context passed to all tool executions
- `ToolResult`: Success/Error enum

### Key implementation

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, tool: impl Tool + 'static);
    pub fn schemas(&self) -> Vec<ToolSchema>;
    pub async fn execute(&self, name: &str, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}
```

### File: `src/tools/mod.rs`

```rust
pub fn register_default_tools(registry: &mut ToolRegistry) {
    registry.register(get_time::GetTimeTool);
    registry.register(file_ops::ReadFileTool);
    registry.register(file_ops::WriteFileTool);
    registry.register(file_ops::ListDirTool);
    registry.register(system::SystemInfoTool);
}
```

### Tests

```rust
#[test]
fn test_register_and_list_schemas() { /* register tools, verify schemas generated */ }

#[tokio::test]
async fn test_dispatch_known_tool() { /* execute get_time, verify success */ }

#[tokio::test]
async fn test_dispatch_unknown_tool() { /* execute "nonexistent", verify error */ }
```

---

## Step 6a-6e: Built-in Tools

### 6a: `src/tools/get_time.rs`

```rust
pub struct GetTimeTool;

// name: "get_time"
// description: "Get the current date, time, and timezone of the device."
// params: { timezone?: string }
// execute: returns formatted current time
```

### 6b-6c: `src/tools/file_ops.rs`

```rust
pub struct ReadFileTool { data_dir: PathBuf }
pub struct WriteFileTool { data_dir: PathBuf }
pub struct ListDirTool { data_dir: PathBuf }

// All paths validated against data_dir (no escape)
// ReadFileTool: reads file content, returns as string
// WriteFileTool: writes content to file, creates parent dirs
// ListDirTool: lists directory contents with file sizes
```

**Critical**: Path validation function:
```rust
fn validate_path(data_dir: &Path, requested: &str) -> Result<PathBuf> {
    let joined = data_dir.join(requested);
    let canonical = joined.canonicalize()
        .or_else(|_| {
            // File may not exist yet (write_file), canonicalize parent
            if let Some(parent) = joined.parent() {
                std::fs::create_dir_all(parent)?;
                Ok(joined.clone())
            } else {
                Err(anyhow!("Invalid path"))
            }
        })?;
    if !canonical.starts_with(data_dir) {
        return Err(anyhow!("Path escapes data directory"));
    }
    Ok(canonical)
}
```

### 6e: `src/tools/system.rs`

```rust
pub struct SystemInfoTool;

// name: "system_info"
// description: "Get device system information: OS, architecture, CPU, memory, uptime."
// params: {}
// execute: reads from /proc/meminfo, /proc/cpuinfo, /proc/uptime (Linux)
//          or sysctl on macOS (for dev)
```

### Tests for each tool

```rust
#[tokio::test]
async fn test_get_time_returns_timestamp() { ... }

#[tokio::test]
async fn test_read_file_success() { /* create temp file, read it */ }

#[tokio::test]
async fn test_read_file_path_escape() { /* "../../../etc/passwd" → error */ }

#[tokio::test]
async fn test_write_file_creates_parents() { /* write to nested dir */ }

#[tokio::test]
async fn test_list_dir() { /* list data dir, verify entries */ }

#[tokio::test]
async fn test_system_info() { /* verify returns non-empty info */ }
```

---

## Step 7: Context Builder

**File**: `src/agent/context.rs`

### What it does

- Reads SOUL.md, USER.md, MEMORY.md, daily notes, skills from data directory
- Assembles system prompt with per-component budget enforcement
- Caches assembled prompt with TTL (avoids re-reading from disk every turn)
- Generates device context (version, platform, time)

### Key implementation

```rust
pub struct ContextBuilder {
    data_dir: PathBuf,
    cache: Option<CachedPrompt>,
    ttl: Duration,
    budgets: ContextBudgets,
}

struct CachedPrompt {
    system: String,
    loaded_at: Instant,
}

struct ContextBudgets {
    soul_max: usize,          // 4096
    user_max: usize,          // 2048
    memory_max: usize,        // 4096
    daily_notes_max: usize,   // 3072
    skills_max: usize,        // 2048
}

impl ContextBuilder {
    pub fn new(data_dir: PathBuf, ttl: Duration) -> Self;

    /// Build full context for an agent turn
    pub fn build(
        &mut self,
        session: &Session,
        tool_schemas: &[ToolSchema],
    ) -> Result<Context>;

    /// Reload files from disk and rebuild system prompt cache
    fn reload_system_prompt(&mut self) -> Result<()>;

    /// Read a file, truncate to max bytes at a paragraph boundary
    fn read_budgeted(path: &Path, max_bytes: usize) -> String;

    /// Generate device context (version, platform, time)
    fn device_context() -> String;
}
```

### Tests

```rust
#[test]
fn test_build_with_soul_only() { /* minimal: just SOUL.md exists */ }

#[test]
fn test_build_with_all_files() { /* SOUL + USER + MEMORY + daily notes */ }

#[test]
fn test_budget_truncation() { /* MEMORY.md larger than budget → truncated */ }

#[test]
fn test_cache_ttl() { /* build twice quickly → uses cache */ }

#[test]
fn test_missing_soul_uses_default() { /* no SOUL.md → default personality */ }
```

---

## Step 8: Session Store

**File**: `src/agent/memory.rs`

### What it does

- `Session`: in-memory conversation history (Vec<Message>)
- `SessionStore`: HashMap of sessions, load from / persist to JSONL files
- `MemoryManager`: read/write MEMORY.md, daily notes (used by memory_store tool)

### Key implementation

```rust
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub needs_consolidation: bool,
}

impl Session {
    pub fn new(id: &str) -> Self;
    pub fn add_message(&mut self, role: Role, content: &str);
    pub fn add_tool_use_message(&mut self, response: &ChatResponse);
    pub fn add_tool_result(&mut self, tool_use_id: &str, result: ToolResult);
    pub fn message_count(&self) -> usize;
    /// Convert to messages array for LLM context
    pub fn messages_for_context(&self) -> Vec<Message>;
}

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: PathBuf) -> Self;
    pub fn get_or_load(&mut self, id: &str) -> &mut Session;
    pub fn persist(&self, id: &str) -> Result<()>;
    pub fn persist_all(&self) -> Result<()>;
}

pub struct MemoryManager {
    data_dir: PathBuf,
}

impl MemoryManager {
    pub fn read_memory(&self) -> Result<String>;
    pub fn append_memory(&self, key: &str, value: &str) -> Result<()>;
    pub fn read_daily_note(&self, date: &str) -> Result<String>;
    pub fn append_daily_note(&self, content: &str) -> Result<()>;
}
```

### JSONL session format

```jsonl
{"role":"user","content":{"Text":"What time is it?"}}
{"role":"assistant","content":{"Text":"It's 3:42 PM."}}
{"role":"user","content":{"Text":"Remember my name is Jiekai"}}
{"role":"assistant","content":{"ToolUse":{"text":"I'll save that.","tool_calls":[{"id":"call_1","name":"memory_store","arguments":{"key":"name","value":"Jiekai"}}]}}}
{"role":"tool","content":{"ToolResult":{"tool_use_id":"call_1","content":"Stored: name = Jiekai"}}}
{"role":"assistant","content":{"Text":"I've saved that your name is Jiekai."}}
```

### Tests

```rust
#[test]
fn test_session_add_messages() { ... }

#[test]
fn test_session_roundtrip_jsonl() { /* create session, persist, load, compare */ }

#[test]
fn test_session_store_get_or_create() { /* new session created if not exists */ }

#[test]
fn test_memory_manager_append_and_read() { ... }
```

---

## Step 9: Agent Loop

**File**: `src/agent/loop.rs`

### What it does

The core ReAct loop. This is the most important file (~300-500 lines).

### Key implementation

```rust
pub struct Agent {
    llm: Box<dyn LlmProvider>,
    fallback_llm: Option<Box<dyn LlmProvider>>,
    tool_registry: ToolRegistry,
    memory: MemoryManager,
    session_store: SessionStore,
    context_builder: ContextBuilder,
    config: AgentConfig,
}

impl Agent {
    pub fn new(config: &Config, data_dir: PathBuf) -> Result<Self>;

    /// Process one input. Called only by agent_worker (sole owner).
    pub async fn process(&mut self, input: &Input) -> Result<Output>;

    /// LLM call with failover
    async fn call_llm(&self, context: &Context) -> Result<ChatResponse>;
}
```

### Process method pseudo-flow

```
1. Get or create session for input.session_id
2. If session.needs_consolidation, consolidate now (deferred from previous turn)
3. Add user message to session
4. For iteration in 0..max_iterations:
   a. Build context (from cache + session + tools)
   b. Call LLM (with failover)
   c. If StopReason::EndTurn or MaxTokens:
      - Add assistant message to session
      - Persist session
      - Flag consolidation if threshold exceeded
      - Return Output with text
   d. If StopReason::ToolUse:
      - Add tool_use message to session
      - Execute tools in parallel (join_all)
      - Add tool results to session
      - Continue loop
5. If max iterations exceeded:
   - Persist session
   - Return "reasoning limit" message
```

### Input/Output types (also in this file or `src/agent/mod.rs`)

```rust
pub struct Input {
    pub id: String,
    pub source: InputSource,
    pub session_id: String,
    pub content: String,
}

pub enum InputSource {
    Cli { reply_tx: oneshot::Sender<Output> },
    // Phase 3: Http, Mqtt, Cron, Heartbeat
}

pub struct Output {
    pub content: String,
    pub usage: Option<Usage>,
}
```

### Tests

These are the most important tests — they validate the entire agent behavior.

```rust
// In tests/agent_test.rs

#[tokio::test]
async fn test_simple_text_response() {
    // MockLlm returns "Hello!" → agent returns "Hello!"
}

#[tokio::test]
async fn test_single_tool_call() {
    // MockLlm returns tool_call("get_time") then "It's 3:42 PM"
    // Verify: tool was called, final response correct
}

#[tokio::test]
async fn test_multi_tool_parallel() {
    // MockLlm returns tool_call("get_time") + tool_call("system_info") simultaneously
    // Verify: both tools executed, both results in context
}

#[tokio::test]
async fn test_max_iterations() {
    // MockLlm always returns tool_use → agent stops at max_iterations
}

#[tokio::test]
async fn test_llm_failover() {
    // Primary LlmProvider returns error → fallback succeeds
}

#[tokio::test]
async fn test_session_persistence() {
    // Process message → verify JSONL file written
}

#[tokio::test]
async fn test_multi_turn() {
    // Process message A → process message B → verify both in session
}

#[tokio::test]
async fn test_context_includes_memory() {
    // Write to MEMORY.md → process message → verify memory in context sent to LLM
}
```

### MockLlmClient (in tests/)

```rust
pub struct MockLlmClient {
    responses: std::sync::Mutex<VecDeque<ChatResponse>>,
    pub recorded_contexts: std::sync::Mutex<Vec<Context>>,
}

impl MockLlmClient {
    pub fn text(response: &str) -> Self;
    pub fn tool_then_text(tool_name: &str, args: Value, final_text: &str) -> Self;
    pub fn multi_tool_then_text(calls: Vec<(&str, Value)>, final_text: &str) -> Self;
    pub fn failing() -> Self;  // always returns error
}

#[async_trait]
impl LlmProvider for MockLlmClient { ... }
```

---

## Step 10: CLI + Main

**File**: `src/main.rs`

### What it does

- Clap CLI with subcommands: `init`, `chat`, `--version`
- `init`: create directory structure + default files
- `chat`: run agent in REPL or single-shot mode
- Sets up tracing (logging)
- Spawns `agent_worker` task
- Handles SIGINT/SIGTERM for graceful shutdown

### CLI definition

```rust
#[derive(Parser)]
#[command(name = "miniclaw", version, about = "Privacy-first AI agent for ARM Linux SBCs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to config file
    #[arg(long, default_value = "config/config.toml")]
    config: PathBuf,
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
    },
    // Phase 3: Serve { ... }
}
```

### Main flow

```
main()
  1. Parse CLI args
  2. Match command:
     Init → create_dirs(), write_defaults(), print instructions, exit
     Chat → run_chat()

run_chat()
  1. Load config
  2. Setup tracing
  3. Ensure data dir exists (auto-create SOUL.md if missing)
  4. Create Agent (providers, tools, context builder, session store)
  5. Create inbound mpsc channel
  6. Spawn agent_worker task (sole owner of Agent)
  7. If --message flag:
     a. Send single Input with oneshot reply
     b. Await reply, print, exit
  8. Else (REPL mode):
     a. Print welcome banner
     b. Loop: read line from stdin → send Input with oneshot → await reply → print
     c. Handle Ctrl+C for clean exit
  9. On shutdown: agent_worker persists sessions

agent_worker task:
  loop {
      input = inbound_rx.recv().await
      output = agent.process(&input).await
      match input.source {
          Cli { reply_tx } => reply_tx.send(output).ok(),
          // Phase 3: Http, Mqtt, etc.
      }
  }
```

### REPL UX

```
$ ./miniclaw chat
MiniClaw v0.1.0 | claude-sonnet-4-6 | RPi 4
Type 'exit' or Ctrl+C to quit.

You> What time is it?
MiniClaw> It's 3:42 PM on Saturday, March 22, 2026.

You> Read the file data/SOUL.md
MiniClaw> [calls read_file tool]
Here's the content of SOUL.md:
# MiniClaw
You are MiniClaw, a helpful AI assistant...

You> exit
Goodbye!
```

### Validation

```bash
# Init
cargo run -- init
ls data/   # verify structure created

# Single-shot
ANTHROPIC_API_KEY=sk-... cargo run -- chat --message "What time is it?"

# REPL
ANTHROPIC_API_KEY=sk-... cargo run -- chat

# Version
cargo run -- --version
```

---

## Step 11: Integration Tests

**File**: `tests/agent_test.rs`

All the test cases from Step 9, plus:

```rust
#[tokio::test]
async fn test_end_to_end_file_operations() {
    // "Write 'hello' to test.txt" → tool called → "Read test.txt" → returns "hello"
}

#[tokio::test]
async fn test_context_builder_integration() {
    // Write SOUL.md + MEMORY.md → build context → verify both appear in system prompt
}

#[tokio::test]
async fn test_agent_with_real_data_dir() {
    // Use tempdir, init default files, run agent with mock LLM
}
```

### Validation

```bash
cargo test                    # all tests pass
cargo test -- --nocapture     # see output for debugging
```

---

## Step 12: Cross-Compile & Deploy

### Build

```bash
# Install toolchain (one-time)
rustup target add aarch64-unknown-linux-gnu
brew install zig
cargo install cargo-zigbuild

# Build
cargo zigbuild --target aarch64-unknown-linux-gnu --release

# Check binary size
ls -lh target/aarch64-unknown-linux-gnu/release/miniclaw
# Target: < 5 MB
```

### Deploy & Test on RPi 4

```bash
# Deploy
./deploy.sh

# SSH to RPi
ssh pi@rpi4.local

# Init
cd ~/miniclaw
./miniclaw init

# Set API key
export ANTHROPIC_API_KEY="sk-ant-..."

# Test single-shot
./miniclaw chat --message "What time is it?"

# Test REPL
./miniclaw chat

# Check resource usage (in another terminal)
ssh pi@rpi4.local 'ps aux | grep miniclaw; free -h'
# Target: < 20 MB RSS
```

### Acceptance Criteria for Phase 1 Complete

```
✓ Binary compiles for both macOS and aarch64-linux
✓ Binary size < 5 MB (release, stripped)
✓ `miniclaw init` creates data directory structure + defaults
✓ `miniclaw --version` prints version
✓ `miniclaw chat --message "Hello"` returns LLM response
✓ `miniclaw chat` starts interactive REPL
✓ Agent calls tools when needed (get_time, read_file, etc.)
✓ Multi-tool chains work (LLM calls tool, sees result, calls another)
✓ Parallel tool execution for independent calls
✓ Session persists to JSONL between turns
✓ Context includes SOUL.md content
✓ Provider failover works (primary fails → fallback)
✓ File path escape prevented (../../../etc/passwd → error)
✓ All unit + integration tests pass
✓ Runs on RPi 4 with < 20 MB RSS
✓ All tests pass with MockLlmClient (no API key needed)
```

---

## Day-by-Day Schedule

| Day | Steps | Deliverable |
|-----|-------|-------------|
| **Day 1** | Step 1 (scaffold) + Step 2 (config) | Project compiles, config loads |
| **Day 2** | Step 3 (types) + Step 4a (Anthropic) | Can serialize/deserialize Anthropic format |
| **Day 3** | Step 4b (OpenAI) + Step 4c (provider trait) | Both providers work, can call real LLM API |
| **Day 4** | Step 5 (registry) + Step 6 (all 5 tools) | Tools registered, schemas generated, tools execute |
| **Day 5** | Step 7 (context) + Step 8 (sessions) | Context assembles from files, sessions persist |
| **Day 6** | Step 9 (agent loop) | **Core milestone**: agent loop works with mock LLM |
| **Day 7** | Step 10 (CLI/main) + Step 11 (tests) + Step 12 (deploy) | End-to-end on RPi |

---

## Files Summary (Phase 1)

| File | Lines (est.) | Purpose |
|------|-------------|---------|
| `src/main.rs` | ~200 | CLI, init, REPL, agent_worker spawn |
| `src/config.rs` | ~150 | Config loading, defaults, validation |
| `src/llm/mod.rs` | ~30 | LlmProvider trait, factory, re-exports |
| `src/llm/types.rs` | ~200 | Canonical types, helpers, serde impls |
| `src/llm/anthropic.rs` | ~250 | Anthropic serialize/deserialize + HTTP |
| `src/llm/openai.rs` | ~200 | OpenAI serialize/deserialize + HTTP |
| `src/tools/mod.rs` | ~30 | Tool registration entry point |
| `src/tools/registry.rs` | ~100 | Tool trait, ToolRegistry, dispatch |
| `src/tools/get_time.rs` | ~40 | get_time tool |
| `src/tools/file_ops.rs` | ~150 | read_file, write_file, list_dir + path validation |
| `src/tools/system.rs` | ~60 | system_info tool |
| `src/agent/mod.rs` | ~10 | Re-exports |
| `src/agent/loop.rs` | ~300 | Agent struct, ReAct loop, failover |
| `src/agent/context.rs` | ~200 | ContextBuilder, cache, budget |
| `src/agent/memory.rs` | ~250 | Session, SessionStore, MemoryManager |
| `tests/agent_test.rs` | ~300 | Integration tests with MockLlmClient |
| **Total** | **~2,470** | |

Under 2,500 lines for a working AI agent. Comparable to MimiClaw (~2,000 lines C) and NanoBot (~4,000 lines Python).
