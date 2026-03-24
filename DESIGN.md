# MiniClaw — End-to-End Design Document

> A privacy-first, voice-capable AI agent OS for resource-constrained ARM Linux SBCs.
>
> **Target**: Raspberry Pi 3+ (1GB+ RAM) | **Language**: Rust | **Binary**: ~3-5MB | **RAM**: ~20MB (Phase 1)

---

## Table of Contents

1. [Design Philosophy](#1-design-philosophy)
2. [Landscape & Positioning](#2-landscape--positioning)
3. [System Architecture](#3-system-architecture)
4. [Agent Core](#4-agent-core)
5. [LLM Integration](#5-llm-integration)
6. [Tool System](#6-tool-system)
7. [Memory & State](#7-memory--state)
8. [Privacy Architecture](#8-privacy-architecture)
9. [Security Model](#9-security-model)
10. [Server & Communication](#10-server--communication)
11. [Voice Pipeline](#11-voice-pipeline)
12. [Hardware Abstraction](#12-hardware-abstraction)
13. [Configuration](#13-configuration)
14. [File System Layout](#14-file-system-layout)
15. [Error Handling & Recovery](#15-error-handling--recovery)
16. [Deployment & Operations](#16-deployment--operations)
17. [Testing Strategy](#17-testing-strategy)
18. [Build Phases](#18-build-phases)

---

## 1. Design Philosophy

### Core Principles

1. **The agent IS the device.** The agent loop runs locally in Rust. The cloud LLM is just an API the agent calls when it needs to think. Tools, memory, privacy gate, control flow, and state all live on the device. This is fundamentally different from a thin client calling a cloud OpenClaw service.

2. **Privacy by architecture.** Data cannot leave the device unless explicitly routed out through a local classifier. This is verifiable by network monitoring — not a policy promise. Inspired by EdgeClaw's S1/S2/S3 classification and Apple Intelligence's on-device routing.

3. **1GB is the floor.** Everything must work on a Raspberry Pi 3 with 1GB RAM. 4GB and 8GB enable optional features (local models, larger TTS voices), but the core agent runs in ~20MB.

4. **Cloud for thinking, local for everything else.** The LLM API call is the only mandatory cloud dependency. STT, TTS, tools, memory, cron, hardware I/O — all local.

5. **Graceful degradation, not broken fallback.** When cloud is unavailable, the device handles predefined commands via rule-based execution and local TTS. It does not pretend to be a conversational agent with a bad local LLM. (Lesson from evaluating 0.5-1B models on RPi — the quality cliff is real.)

6. **Single binary, zero runtime dependencies.** One Rust binary, statically linked with rustls (no OpenSSL). No Python, no Node.js, no JVM. Copy to any ARM Linux device and it runs. Inspired by IronClaw (3.4MB binary, <10ms startup, 7.8MB RAM) and PicoClaw (single Go binary).

7. **Start small, grow modular.** Phase 1 is a text-in/text-out agent with CLI. TTS, privacy classifiers, voice, hardware I/O are additive modules behind feature flags. They plug into well-defined pipeline points without changing the core.

### Non-Goals (Explicit)

- Not a cloud-hosted service (that's OpenClaw)
- Not an MCU firmware (that's MimiClaw)
- Not a Python wrapper (that's NanoBot)
- Not a security-hardened desktop agent with WASM sandboxing (that's IronClaw)
- Not locked to a vendor's cloud ecosystem (that's DuckyClaw/Tuya)
- Not a full desktop assistant with browser automation
- Not a multi-agent orchestration framework
- No local LLM inference for conversation (cloud-first; tiny local models only for classification/routing)

---

## 2. Landscape & Positioning

### Competitive Analysis

| Project | Language | RAM | Target | Agent Loop | Voice | Privacy | HW I/O | Offline |
|---------|---------|-----|--------|-----------|-------|---------|--------|---------|
| **OpenClaw** | TypeScript | 1.5 GB | Desktop/server | Local | No | No | No | No |
| **IronClaw** | Rust | 7.8 MB | Desktop/server | Local | No | No | No | No |
| **NanoBot** | Python | 150 MB | Desktop/server | Local | No | No | No | No |
| **MimiClaw** | C | 8 MB PSRAM | ESP32-S3 | Local | No | No | GPIO | No |
| **DuckyClaw** | C | MCU-scale | ESP32/RPi | Local | No | No | Tuya IoT | No |
| **PicoClaw** | Go | <10 MB | RISC-V/ARM SBC | Gateway | No | No | No | No |
| **EdgeClaw** | TypeScript | 2+ GB | Desktop/server | Plugin on OpenClaw | No | S1/S2/S3 | No | No |
| **MiniClaw** | **Rust** | **~20 MB** | **RPi 3+ (1GB+)** | **Local** | **Planned** | **Planned** | **Planned** | **Yes** |

### What We Learn From Each

| Project | Key Lesson | Applied In MiniClaw |
|---------|-----------|-------------------|
| **IronClaw** | Credential boundary injection — secrets never enter LLM context. Release profile: LTO+strip+panic=abort. Safety as separate concern. Provider failover chains. Job state machine. | Credential injection in tools, release profile, safety module, LLM fallback, request state tracking |
| **MimiClaw** | Agent loop IS simple (~500 lines C). Context builder from markdown files. Heartbeat (HEARTBEAT.md every 30 min). Cron (cron.json every 60s). OTA updates. Serial CLI fallback. | Context builder pattern, heartbeat service, cron service, CLI fallback |
| **NanoBot** | Memory consolidation (old messages → summary → MEMORY.md). MCP lazy-connect. Config-driven (single validated JSON). SpawnTool (background subagents). 25+ LLM providers via LiteLLM. | Memory consolidation, MCP pattern, config-driven design |
| **DuckyClaw** | MCP tool server works on device in C. Heartbeat/proactive agent. Single C codebase across MCU+Linux via HAL. | Proactive agent pattern. But avoid: Tuya vendor lock-in. |
| **EdgeClaw** | S1/S2/S3 privacy classification. Dual detection (regex + semantic LLM). Dual-track memory (clean + full). Cost-aware routing (60-80% savings). | Privacy tier model, PII detection, dual-track memory concept. But avoid: heavy Node.js base, 8B model requirement. |
| **Xiaozhi** | Best ESP32 voice firmware. WebSocket + OPUS audio streaming. MCP on both device and server. Dialogue interruption. Self-hosted server (FunASR + CoSyVoice + Ollama). | Voice pipeline protocol, self-hosted STT/TTS architecture |
| **Willow** | <500ms end-to-end latency. Audio front-end (AEC, BSS, 25ft range). Proves sub-$50 hardware beats commercial assistants. | Latency targets, audio processing quality bar |

### MiniClaw's Unique Position

The only project targeting the RPi 3+ / ARM Linux SBC tier with ALL of:
- Local agent loop (not a cloud proxy)
- Privacy-by-architecture (local classifier, not cloud plugin)
- Voice pipeline (STT + TTS) — planned
- Offline graceful degradation
- Physical hardware I/O — planned
- Single Rust binary, ~20MB footprint
- 1GB RAM minimum

---

## 3. System Architecture

### High-Level Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                        MiniClaw (Rust Binary)                     │
│                        ~3-5 MB, ~20 MB RAM (Phase 1)             │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                     EVENT BUS (tokio mpsc)                 │    │
│  │                                                            │    │
│  │   Inbound:  Input  { source, session_id, content, meta } │    │
│  │   Outbound: Output { target, content, metadata }          │    │
│  └──────┬───────────────────────────────────────┬────────────┘    │
│         │                                       │                  │
│  ┌──────▼──────────┐                   ┌───────▼─────────┐       │
│  │  INPUT SOURCES   │                   │  OUTPUT SINKS    │       │
│  │                  │                   │                  │       │
│  │  • CLI (stdin)   │ ← Phase 1        │  • CLI (stdout)  │       │
│  │  • HTTP API      │ ← Phase 3        │  • HTTP response │       │
│  │  • MQTT sub      │ ← Phase 3        │  • MQTT pub      │       │
│  │  • Cron tick     │ ← Phase 3        │  • TTS (Piper)   │ P5   │
│  │  • Heartbeat     │ ← Phase 3        │  • WebSocket     │       │
│  │  • WebSocket     │ ← Phase 5        │                  │       │
│  └──────────────────┘                   └──────────────────┘       │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                     AGENT CORE                             │    │
│  │                                                            │    │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │    │
│  │  │ Context  │  │  Agent   │  │   Tool   │  │  Memory  │ │    │
│  │  │ Builder  │─►│   Loop   │─►│ Executor │  │ Manager  │ │    │
│  │  │ (cached) │  │ (ReAct)  │  │          │  │(in-memory│ │    │
│  │  └──────────┘  └─────┬────┘  └──────────┘  │ +persist)│ │    │
│  │                      │                       └──────────┘ │    │
│  │                ┌─────▼────┐                               │    │
│  │                │   LLM    │  ◄── Only remote dependency   │    │
│  │                │  Client  │      Anthropic + OpenAI-compat│    │
│  │                │(failover)│      with provider failover   │    │
│  │                └──────────┘                               │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                 OPTIONAL MODULES (feature flags)           │    │
│  │                                                            │    │
│  │  [privacy]  Privacy gate — ONNX classifier + PII regex    │ P4 │
│  │  [safety]   Prompt injection defense, output sanitization │ P4 │
│  │  [voice]    Voice pipeline — WebSocket audio, Piper TTS   │ P5 │
│  │  [rpi]      Hardware I/O — GPIO, I2C, SPI via sysfs       │ P6 │
│  │  [mcp]      MCP server — expose device tools to network   │ P6 │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                    │
└──────────────────────────────────────────────────────────────────┘
```

### Process Model

Single process, multi-threaded Tokio runtime (default: workers = CPU core count).

Multi-threaded is the right choice because I/O tasks (HTTP server, MQTT listener, cron tick) should never block each other, even when the agent loop is waiting on a cloud LLM call. The cost is ~2MB extra RAM. The benefit is proper concurrency.

```
main()
 ├── task: agent_worker       (sole owner of Agent — consumes from inbound
 │                             queue, runs agent loop, routes output inline)
 │
 │  Phase 1:
 ├── task: cli_reader         (reads stdin, sends to inbound queue)
 │
 │  Phase 3:
 ├── task: http_server        (axum, port 3000)
 ├── task: mqtt_client        (rumqttc, subscribe to command topics)
 ├── task: cron_scheduler     (check cron.json every 60s)
 └── task: heartbeat_service  (check HEARTBEAT.md every 30 min)
```

**Single-owner concurrency model**: The `agent_worker` task is the ONLY task that owns and mutates the `Agent` struct. All other tasks send `Input` messages through the inbound mpsc channel. This eliminates all concurrency concerns — no `Arc<Mutex<Agent>>`, no shared mutable state, no data races. Output routing is done inline by `agent_worker` after each turn.

For request-response sources (HTTP, CLI), the sender embeds a `oneshot::Sender<Output>` in the `Input`. The agent worker sends the response back through it. For fire-and-forget sources (cron, heartbeat), output goes to a configured default sink (MQTT publish, log, etc.).

```
HTTP handler flow:
  1. Create oneshot channel (tx, rx)
  2. Send Input { source: Http { reply_tx: tx }, ... } to inbound queue
  3. await rx → blocks until agent_worker responds (with HTTP timeout)
  4. Return HTTP response

agent_worker flow (sole owner of Agent):
  loop {
      input = inbound_rx.recv().await
      output = agent.process(&input).await    // &mut self — safe, sole owner
      match input.source {
          Http { reply_tx } => reply_tx.send(output).ok(),
          Cli { reply_tx }  => reply_tx.send(output).ok(),
          Mqtt { topic }    => mqtt_tx.send((topic, output)).await.ok(),
          Cron { .. }       => log_or_publish(output).await,
          Heartbeat         => log_or_publish(output).await,
      }
  }
```

**Note on queuing**: Since agent_worker processes one request at a time, HTTP requests may wait behind cron/heartbeat jobs. This is acceptable for a personal device (typical queue depth: 0-1). HTTP handlers should have a timeout (default: 60s) to avoid indefinite blocking.

### Runtime Configuration

```rust
// Multi-threaded Tokio, workers = CPU cores
// 4 workers on RPi 3 (quad-core A53), 4 on RPi 4 (quad-core A72)
#[tokio::main]
async fn main() { ... }
```

big.LITTLE SOCs (e.g., RK3588: 4×A76 + 4×A55) are handled transparently by Linux's Energy Aware Scheduling. No core pinning needed — our workload is I/O-bound.

### Graceful Shutdown

```rust
// Handle SIGTERM/SIGINT for clean shutdown
async fn run(config: Config) -> Result<()> {
    let shutdown = tokio::signal::ctrl_c();

    tokio::select! {
        result = run_all_tasks(&config) => result,
        _ = shutdown => {
            tracing::info!("Shutting down...");
            // 1. Persist all in-memory sessions to disk
            session_store.persist_all().await?;
            // 2. Publish offline status via MQTT (Phase 3)
            #[cfg(feature = "server")]
            mqtt_client.publish_status(DeviceStatus::Offline).await.ok();
            // 3. Wait for in-flight agent loop to finish (with timeout)
            tokio::time::timeout(Duration::from_secs(10), agent_handle).await.ok();
            Ok(())
        }
    }
}
```

---

## 4. Agent Core

### 4.1 Agent Loop (ReAct Pattern)

The agent loop is the heart of MiniClaw. ~500 lines of Rust.

```
Input → [Privacy Gate*] → [Context Build] → [LLM Call] → [Parse Response]
                                                ▲                │
                                                │          ┌─────▼─────┐
                                                │          │ Tool Call? │
                                                │          └─────┬─────┘
                                                │           Yes  │  No
                                                │          ┌─────▼─────┐
                                                └──────────┤ Execute   │
                                                 tool result│ Tools    │
                                                           │(parallel)│
                                                           └───────────┘
                                                                 │ No
                                                           ┌─────▼─────┐
                                                           │ Final     │
                                                           │ Response  │
                                                           └───────────┘
                                          * Phase 4 feature flag
```

#### Single Entry Point

All inputs — CLI, HTTP, MQTT, cron, heartbeat — arrive through the inbound mpsc channel. The agent has one method: `process(&mut self, input: &Input) -> Result<Output>`. The `agent_worker` task is the sole owner of the `Agent` struct, so `&mut self` is safe without locks.

```rust
impl Agent {
    /// The single entry point. Called only by agent_worker task.
    pub async fn process(&mut self, input: &Input) -> Result<Output> {
        // Full ReAct loop — see implementation below
    }
}
```

#### Request State Machine

Borrowed from IronClaw's job state machine for better error handling and observability:

```
┌─────────┐     ┌────────────┐     ┌───────────┐
│ Pending  │────►│ InProgress │────►│ Completed │
└─────────┘     └─────┬──────┘     └───────────┘
                      │
                      ├────►┌────────┐
                      │     │ Failed │ (LLM error, tool error, timeout)
                      │     └────────┘
                      │
                      └────►┌────────┐     ┌────────────┐
                            │ Stuck  │────►│ InProgress │ (self-repair retry)
                            └────────┘     └────────────┘
```

```rust
pub enum RequestState {
    Pending,
    InProgress { started_at: Instant, iteration: u32 },
    Completed { output: Output, usage: Usage },
    Failed { error: String, retryable: bool },
    Stuck { reason: String },  // detected if no progress for timeout period
}
```

#### Agent Loop Implementation

```rust
pub struct Agent {
    llm: Box<dyn LlmProvider>,
    fallback_llm: Option<Box<dyn LlmProvider>>,
    tool_registry: ToolRegistry,
    memory: MemoryManager,
    session_store: SessionStore,
    context_cache: ContextCache,
    config: AgentConfig,
}

impl Agent {
    /// Single entry point. Called only by agent_worker task (sole owner).
    pub async fn process(&mut self, input: &Input) -> Result<Output> {
        // 1. Load or create session
        let session = self.session_store.get_or_load(&input.session_id)?;

        // 2. Add user message
        session.add_message(Role::User, &input.content);

        // 3. Agent reasoning loop
        for iteration in 0..self.config.max_iterations {
            // 3a. Build context (from cache if fresh)
            let context = self.context_cache.build(session, &self.tool_registry)?;

            // 3b. Call LLM (with failover)
            let response = self.call_llm_with_fallback(&context).await?;

            // 3c. Handle response
            match response.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens => {
                    if let Some(text) = &response.text {
                        session.add_message(Role::Assistant, text);
                    }
                    self.session_store.persist(&input.session_id)?;

                    // 4. Post-turn: background consolidation if needed
                    self.maybe_consolidate_background(&input.session_id);

                    return Ok(Output::text(response.text.unwrap_or_default()));
                }
                StopReason::ToolUse => {
                    session.add_tool_use_message(&response);

                    // Execute tools in parallel (independent calls benefit from concurrency)
                    let max_calls = self.config.max_tool_calls_per_iter
                        .min(response.tool_calls.len());
                    let tool_calls = &response.tool_calls[..max_calls];
                    let ctx = self.tool_context();

                    let results = futures::future::join_all(
                        tool_calls.iter().map(|tc| {
                            self.tool_registry.execute(&tc.name, &tc.arguments, &ctx)
                        })
                    ).await;

                    for (tool_call, result) in tool_calls.iter().zip(results) {
                        session.add_tool_result(&tool_call.id, result);
                    }
                    // Loop back — LLM will see tool results
                }
            }
        }

        // Safety: max iterations exceeded
        self.session_store.persist(&input.session_id)?;
        Ok(Output::text("I've reached my reasoning limit for this turn.".into()))
    }

    async fn call_llm_with_fallback(&self, context: &Context) -> Result<ChatResponse> {
        match self.llm.chat(context).await {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::warn!("Primary LLM failed: {e}");
                if let Some(fallback) = &self.fallback_llm {
                    fallback.chat(context).await
                        .map_err(|e2| anyhow!("All LLM providers failed. Primary: {e}, Fallback: {e2}"))
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Trigger background consolidation if session exceeds threshold.
    /// Does NOT block the current turn — runs after response is sent.
    fn maybe_consolidate_background(&self, session_id: &str) {
        let session = match self.session_store.sessions.get(session_id) {
            Some(s) => s,
            None => return,
        };
        if session.message_count() <= self.config.consolidation_threshold {
            return;
        }
        // Consolidation will happen at the START of the next turn for this session,
        // before building context. This is simpler than spawning a background task
        // that would need to synchronize with the session store.
        // The agent_worker checks the flag and consolidates before the next LLM call.
        tracing::info!("Session {session_id} exceeds consolidation threshold, will consolidate next turn");
    }
}

// In the agent_worker loop, before processing:
// if session.needs_consolidation {
//     memory.consolidate(session, llm).await?;
//     session.needs_consolidation = false;
// }
// This runs BEFORE the user's next message is processed, so it doesn't
// add latency to the current turn — only to the next turn's startup.
// Typical consolidation: 1-2 seconds (one cheap LLM call for summarization).
```

### 4.2 Context Builder (Cached)

Assembles the LLM prompt from markdown files. **Caches in memory** with TTL to avoid repeated SD card reads (lesson from reviewing RPi I/O characteristics).

```
System Prompt Assembly Order (with hard budget per component):
┌──────────────────────────────────────────────────────────────┐
│ Component            │ Max Size │ Truncation Strategy         │
├──────────────────────┼──────────┼─────────────────────────────┤
│ 1. SOUL.md           │   4 KB   │ Truncate tail, warn in log │
│ 2. USER.md           │   2 KB   │ Truncate tail              │
│ 3. Device context    │  0.5 KB  │ Generated, always fits     │
│ 4. MEMORY.md         │   4 KB   │ Keep most recent sections  │
│ 5. Recent daily notes│   3 KB   │ Most recent entries first  │
│ 6. Skills catalog    │   2 KB   │ Select most relevant       │
│ 7. Privacy rules*    │  0.5 KB  │ Fixed size                 │
├──────────────────────┼──────────┼─────────────────────────────┤
│ Total budget         │  16 KB   │ Hard cap enforced           │
└──────────────────────┴──────────┴─────────────────────────────┘
                                        * Phase 4 only

Each component is truncated independently before assembly.
If any file exceeds its budget, a tracing::warn is emitted.
This prevents MEMORY.md growth from silently ballooning costs.
Conversation:
┌────────────────────────────────────────────────────────┐
│ 8. Session history  — user/assistant/tool messages      │
│ 9. Current user message                                 │
└────────────────────────────────────────────────────────┘
```

```rust
pub struct ContextCache {
    system_prompt: String,
    tool_schemas: Vec<ToolSchema>,
    last_loaded: Instant,
    ttl: Duration,   // default: 60 seconds — reload files if stale
}

impl ContextCache {
    pub fn build(&mut self, session: &Session, tools: &ToolRegistry) -> Result<Context> {
        // Reload from disk if cache expired
        if self.last_loaded.elapsed() > self.ttl {
            self.reload_from_files()?;
            self.tool_schemas = tools.schemas();
            self.last_loaded = Instant::now();
        }

        let messages = self.assemble_messages(session);
        Ok(Context {
            system: self.system_prompt.clone(),
            messages,
            tool_schemas: self.tool_schemas.clone(),
        })
    }
}
```

### 4.3 Input/Output Types

```rust
/// All input sources normalize to this canonical type.
/// The source determines how the agent_worker routes the response.
pub struct Input {
    pub id: String,                  // uuid v4
    pub source: InputSource,
    pub session_id: String,
    pub content: String,
    pub metadata: InputMetadata,     // timestamp, user info
}

/// Input source — carries reply channel for request-response sources.
pub enum InputSource {
    Cli { reply_tx: oneshot::Sender<Output> },       // REPL or single-shot
    Http { reply_tx: oneshot::Sender<Output> },      // HTTP API
    Mqtt { reply_topic: String },                     // MQTT — reply via publish
    WebSocket { conn_id: String },                    // WebSocket client
    Cron { job_id: String },                          // Cron-triggered (fire & forget)
    Heartbeat,                                        // Heartbeat-triggered
}

/// Output — produced by the agent, routed by agent_worker based on InputSource.
pub struct Output {
    pub content: String,
    pub metadata: OutputMetadata,    // usage, latency, request state
}
```

Note: `Output` no longer carries a `target` — the `agent_worker` routes based on the `InputSource` of the corresponding `Input`. This eliminates the duplication between `InputSource` and `OutputTarget`.

#### Session ID Strategy

| Source | Default Session ID | Behavior |
|--------|-------------------|----------|
| CLI | `"cli"` | Single persistent session |
| HTTP | Client-provided or uuid per request | Stateless by default, persistent if client provides ID |
| MQTT | `"{source_device_id}"` | Persistent per device |
| Cron | `"cron-{job_id}"` | Persistent per job |
| Heartbeat | `"heartbeat"` | Single persistent session |

---

## 5. LLM Integration

### 5.1 Provider Architecture

Two native providers: **Anthropic** (native Messages API) + **OpenAI-compatible** (covers 90% of other providers). Hand-rolled, ~350 lines total. No rig-core or LiteLLM dependency.

Why hand-roll instead of using rig-core (IronClaw's approach):
- Only 2 serialization formats needed
- Full control over streaming, error handling, retry logic
- No dependency risk — we own every line
- The `LlmProvider` trait makes rig-core a drop-in replacement later if needed

```
┌─────────────────────────────────────────────┐
│           LLM Provider Architecture          │
│                                               │
│   Agent Loop                                  │
│       │                                       │
│       ▼                                       │
│   ChatRequest / ChatResponse (canonical)     │
│       │                                       │
│   ┌───▼──────────┐                           │
│   │ LlmProvider   │  (trait)                  │
│   └──────┬───────┘                           │
│          │                                    │
│   ┌──────┴──────────────────┐                │
│   ▼                         ▼                │
│  AnthropicProvider    OpenAiProvider          │
│  (~200 lines)         (~150 lines)           │
│                              │                │
│  api.anthropic.com    ┌──────┴──────┐        │
│                       ▼             ▼        │
│                  api.openai.com  localhost:   │
│                  api.groq.com    11434        │
│                  openrouter.ai   (Ollama)     │
│                  api.deepseek.   (vLLM)      │
│                  com             (anything)   │
│                  (any OpenAI-                 │
│                   compatible)                 │
└─────────────────────────────────────────────┘
```

Why support Anthropic natively instead of OpenAI-only:
1. Claude is the best agent model. Forcing users through OpenRouter is a bad UX tax.
2. Anthropic-specific features matter: extended thinking, cache control, content blocks.
3. It's only ~200 lines of serialization code.

### 5.2 Provider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;

    async fn chat_stream(&self, context: &Context)
        -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>>;
}
```

### 5.3 Canonical Types

```rust
pub struct Context {
    pub system: String,
    pub messages: Vec<Message>,
    pub tool_schemas: Vec<ToolSchema>,
}

pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

pub enum Role { System, User, Assistant, Tool }

pub enum MessageContent {
    Text(String),
    ToolUse { text: Option<String>, tool_calls: Vec<ToolCall> },  // text + tool calls can coexist
    ToolResult { tool_use_id: String, content: String },
}

pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

pub enum StopReason { EndTurn, ToolUse, MaxTokens }

pub struct Usage { pub input_tokens: u32, pub output_tokens: u32 }

pub enum StreamChunk {
    TextDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { arguments_chunk: String },
    Done(ChatResponse),
}
```

### 5.4 Format Conversion

The key differences between Anthropic and OpenAI are purely serialization:

| Aspect | Anthropic | OpenAI |
|--------|-----------|--------|
| System prompt | Separate `system` field | `role: "system"` message |
| Tool calls | `content` blocks with `type: "tool_use"` | `tool_calls` field on assistant message |
| Tool results | `content` block `type: "tool_result"` in user msg | Separate message with `role: "tool"` |
| Stop reason | `stop_reason: "tool_use"` / `"end_turn"` | `finish_reason: "tool_calls"` / `"stop"` |
| Token usage | `usage.input_tokens` | `usage.prompt_tokens` |
| Streaming | Custom SSE event types | Standard SSE `data: {...}` |

Each provider implementation handles JSON serialization/deserialization internally. The agent loop only sees canonical types.

### 5.5 Provider Failover

```rust
// config.toml
[llm]
provider = "anthropic"
model = "claude-sonnet-4-6"
api_key_env = "ANTHROPIC_API_KEY"

[llm.fallback]
provider = "openai_compatible"
base_url = "http://localhost:11434"  # Ollama
model = "qwen3:0.6b"
api_key_env = ""                     # no key for Ollama
```

Failover logic: Primary → Fallback → Error. Inspired by IronClaw's provider chain.

---

## 6. Tool System

### 6.1 Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

pub struct ToolContext {
    pub data_dir: PathBuf,
    pub session_id: String,
    pub config: Arc<Config>,
}

pub enum ToolResult {
    Success(String),
    Error(String),
}
```

### 6.2 Tool Registry

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| ToolSchema {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        }).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(args, ctx).await,
            None => ToolResult::Error(format!("Unknown tool: {name}")),
        }
    }
}
```

### 6.3 Built-in Tools by Phase

**Phase 1:**

| Tool | Name | Parameters |
|------|------|-----------|
| Get Time | `get_time` | `{ timezone?: string }` |
| Read File | `read_file` | `{ path: string }` |
| Write File | `write_file` | `{ path: string, content: string }` |
| List Directory | `list_dir` | `{ path?: string }` |
| System Info | `system_info` | `{}` |

**Phase 2:**

| Tool | Name | Parameters |
|------|------|-----------|
| Edit File | `edit_file` | `{ path: string, old_text: string, new_text: string }` |
| Memory Store | `memory_store` | `{ key: string, value: string }` |
| Memory Read | `memory_read` | `{ key?: string }` |

**Phase 3:**

| Tool | Name | Parameters |
|------|------|-----------|
| Shell Execute | `shell_exec` | `{ command: string }` |
| HTTP Fetch | `http_fetch` | `{ url: string, method?: string }` |
| Cron Add | `cron_add` | `{ schedule: string, action: string, name?: string }` |
| Cron List | `cron_list` | `{}` |
| Cron Remove | `cron_remove` | `{ id: string }` |

### 6.4 Tool Sandboxing

Path restriction + command whitelist + timeout. Lighter than IronClaw's WASM sandbox (which adds 15ms overhead per call and ~20MB binary size), but appropriate for a trusted personal device.

```rust
// File access: restricted to data_dir
fn validate_path(data_dir: &Path, requested: &str) -> Result<PathBuf> {
    let full = data_dir.join(requested).canonicalize()?;
    if !full.starts_with(data_dir) {
        return Err(anyhow!("Path escapes data directory"));
    }
    Ok(full)
}

// Shell: whitelist + timeout + working directory restriction
pub struct ShellExecTool {
    allowed_commands: HashSet<String>,
    workspace_dir: PathBuf,
    timeout: Duration,  // default: 10s
}
```

### 6.5 Credential Boundary Injection

Borrowed from IronClaw. API keys never enter the LLM context or tool arguments directly.

```rust
// HTTP Fetch tool: credentials injected AFTER LLM generates the request

impl HttpFetchTool {
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let url = args["url"].as_str().unwrap_or("");

        // LLM might generate: "fetch https://api.example.com with header X-API-Key: {MY_KEY}"
        // We substitute {MY_KEY} with the real value from config/env
        let url = self.substitute_credentials(url, &ctx.config);

        // After receiving response, scan for credential leaks
        let response = reqwest::get(&url).await?;
        let body = response.text().await?;
        let safe_body = self.redact_leaked_credentials(&body, &ctx.config);

        ToolResult::Success(safe_body)
    }

    fn redact_leaked_credentials(&self, text: &str, config: &Config) -> String {
        let mut result = text.to_string();
        for secret in config.known_secrets() {
            if result.contains(secret) {
                result = result.replace(secret, "[REDACTED]");
            }
        }
        result
    }
}
```

---

## 7. Memory & State

### 7.1 Memory Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      MEMORY LAYERS                           │
│                                                               │
│  Layer 1: System Identity (rarely changes)                   │
│    SOUL.md     — personality, identity, rules                │
│    USER.md     — user context, preferences                   │
│    skills/*.md — skill descriptions                          │
│                                                               │
│  Layer 2: Long-term Memory (grows over time)                 │
│    memory/MEMORY.md     — consolidated facts & knowledge     │
│    memory/YYYY-MM-DD.md — daily notes (auto-created)         │
│                                                               │
│  Layer 3: Short-term Memory (per conversation)               │
│    sessions/<id>.jsonl  — message history (in-memory cache)  │
│    Consolidated when exceeding message count threshold       │
│                                                               │
│  Layer 4: Operational State (ephemeral)                      │
│    cron.json       — scheduled jobs                          │
│    HEARTBEAT.md    — proactive task checklist                │
│    state.json      — runtime state                           │
└─────────────────────────────────────────────────────────────┘
```

### 7.2 Session Store (In-Memory + Persist-on-Change)

Sessions are kept in memory for fast access. Written to disk after each agent turn. Avoids unnecessary SD card I/O and wear.

```rust
pub struct SessionStore {
    sessions: HashMap<String, Session>,
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn get_or_load(&mut self, id: &str) -> &mut Session {
        self.sessions.entry(id.to_string()).or_insert_with(|| {
            Self::load_from_disk(&self.data_dir, id)
                .unwrap_or_else(|_| Session::new(id))
        })
    }

    pub fn persist(&self, id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get(id) {
            let path = self.data_dir.join(format!("sessions/{id}.jsonl"));
            let content: String = session.messages.iter()
                .map(|m| serde_json::to_string(m).unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, content)?;
        }
        Ok(())
    }

    pub fn persist_all(&self) -> Result<()> {
        for id in self.sessions.keys() {
            self.persist(id)?;
        }
        Ok(())
    }
}
```

### 7.3 Memory Consolidation

When session message count exceeds threshold, compress old messages into MEMORY.md. Uses **message count** (not token count) to avoid needing a tokenizer dependency.

Consolidation runs at the **start of the next turn** (not during the current turn) so it doesn't add latency to the user's active request. See Agent Loop section for details.

```rust
impl MemoryManager {
    pub async fn consolidate(&self, session: &mut Session, llm: &dyn LlmProvider) -> Result<()> {
        let split_point = session.messages.len() / 2;
        let old_messages = &session.messages[..split_point];

        // Summarize using cheap model
        let summary_prompt = format!(
            "Summarize the key facts and decisions from this conversation \
             in bullet points. Only include information worth remembering:\n\n{}",
            old_messages.iter()
                .map(|m| format!("{}: {}", m.role, m.content_text()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let summary = llm.chat(&Context::simple_query(&summary_prompt)).await?;

        // Append to MEMORY.md
        if let Some(text) = summary.text {
            let memory_path = self.data_dir.join("memory/MEMORY.md");
            let mut memory = std::fs::read_to_string(&memory_path).unwrap_or_default();
            let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
            memory.push_str(&format!("\n\n### Consolidated {date}\n\n{text}"));

            // Enforce MEMORY.md max size — re-consolidate if too large
            if memory.len() > self.config.memory_max_bytes {
                memory = self.reconsolidate_memory(&memory, llm).await?;
            }

            std::fs::write(&memory_path, memory)?;
        }

        // Keep only recent half
        session.messages = session.messages[split_point..].to_vec();
        Ok(())
    }

    /// When MEMORY.md exceeds max size, summarize the memory itself.
    /// Keeps it from growing unbounded over months of use.
    async fn reconsolidate_memory(&self, memory: &str, llm: &dyn LlmProvider) -> Result<String> {
        tracing::info!("MEMORY.md exceeds max size ({}B), reconsolidating", memory.len());
        let prompt = format!(
            "Condense these notes into the most important facts only. \
             Remove redundant or outdated information. Keep it under {} bytes:\n\n{}",
            self.config.memory_max_bytes / 2,
            memory
        );
        let response = llm.chat(&Context::simple_query(&prompt)).await?;
        Ok(response.text.unwrap_or_default())
    }
}
```

### 7.5 Session Cleanup

Old sessions accumulate on disk. Cleanup runs at startup and daily:

```rust
impl SessionStore {
    pub fn cleanup(&mut self, config: &AgentConfig) -> Result<()> {
        let sessions_dir = self.data_dir.join("sessions");
        let mut entries: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(&sessions_dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let modified = e.metadata().ok()?.modified().ok()?;
                Some((e.path(), modified))
            })
            .collect();

        // Sort by modification time (oldest first)
        entries.sort_by_key(|(_, t)| *t);

        let now = SystemTime::now();
        let max_age = Duration::from_secs(config.session_max_age_days * 86400);

        for (path, modified) in &entries {
            // Delete if older than max age
            if now.duration_since(*modified).unwrap_or_default() > max_age {
                std::fs::remove_file(path).ok();
            }
        }

        // Delete oldest if count exceeds max
        let remaining: Vec<_> = std::fs::read_dir(&sessions_dir)?
            .filter_map(|e| e.ok())
            .collect();
        if remaining.len() > config.session_max_count {
            let excess = remaining.len() - config.session_max_count;
            for (path, _) in entries.iter().take(excess) {
                std::fs::remove_file(path).ok();
                self.sessions.remove(path.file_stem().unwrap().to_str().unwrap());
            }
        }

        Ok(())
    }
}
```
```

### 7.4 Daily Notes

Auto-created when the agent writes date-specific observations:

```markdown
<!-- memory/2026-03-22.md -->
## 2026-03-22
- User asked about weather, prefers Celsius
- Cron job "morning-briefing" created at 07:00
- Device temperature reached 65°C, recommended adding heatsink
```

Context builder loads the 3 most recent daily notes into the system prompt.

---

## 8. Privacy Architecture (Phase 4)

### 8.1 Three-Tier Classification

Inspired by EdgeClaw, but using a ~20MB encoder-only ONNX model instead of EdgeClaw's 8B generative LLM. 300x smaller for a classification task that doesn't need generation.

```
Input Text
    │
    ▼
┌──────────────────────────┐
│  FAST PATH: Regex Rules   │  ~0ms
│  API key patterns  → S3   │
│  SSH key headers   → S3   │
│  Password mentions → S3   │
│  Email addresses   → S2   │
│  Phone numbers     → S2   │
│  No match          → next │
└────────────┬─────────────┘
             ▼
┌──────────────────────────┐
│  SLOW PATH: ONNX Model   │  ~50ms on RPi 3
│  (DistilBERT ~20MB)       │
│  Encoder-only classifier  │
│  Classifies: S1 / S2 / S3 │
└────────────┬─────────────┘
     ┌───────┼───────┐
     ▼       ▼       ▼
    S1      S2      S3
     │       │       │
     │       │       └──► LOCAL ONLY — never reaches cloud
     │       └──► PII STRIP → Cloud LLM → Rehydrate locally
     └──► PASS THROUGH → Cloud LLM
```

### 8.2 PII Detection & Stripping

```rust
pub struct PiiDetector {
    patterns: Vec<PiiPattern>,
}

impl PiiDetector {
    pub fn strip(&self, text: &str) -> (String, Vec<PiiMapping>) {
        let mut result = text.to_string();
        let mut mappings = Vec::new();
        for pattern in &self.patterns {
            for m in pattern.regex.find_iter(text) {
                let original = m.as_str().to_string();
                let id = format!("{}_{}", pattern.name, mappings.len());
                let placeholder = format!("[REDACTED:{}:{}]", pattern.name.to_uppercase(), id);
                result = result.replace(&original, &placeholder);
                mappings.push(PiiMapping { id, original, pii_type: pattern.name.to_string() });
            }
        }
        (result, mappings)
    }

    pub fn rehydrate(&self, text: &str, mappings: &[PiiMapping]) -> String {
        let mut result = text.to_string();
        for m in mappings {
            let placeholder = format!("[REDACTED:{}:{}]", m.pii_type.to_uppercase(), m.id);
            result = result.replace(&placeholder, &m.original);
        }
        result
    }
}
```

### 8.3 Dual-Track Memory (Phase 4)

Borrowed from EdgeClaw. When privacy gate is active, maintain two memory tracks:

- `memory/MEMORY.md` — PII-stripped version (safe for cloud LLM context)
- `memory/MEMORY-FULL.md` — complete version (local-only, never sent to cloud)

---

## 9. Security Model

### 9.1 Threat Model

| Threat | Mitigation |
|--------|-----------|
| **API key exposure in LLM context** | Credential boundary injection (IronClaw pattern). Keys in env vars → substituted in tool HTTP requests AFTER LLM generates them. Response scanning for leaks. |
| **Shell injection** | Command whitelist, workspace restriction, timeout |
| **File system escape** | Path validation: all file tools restricted to data directory |
| **Prompt injection via HTTP/MQTT** | Safety module: input sanitization (Phase 4) |
| **Network eavesdropping** | TLS for all outbound (rustls, no OpenSSL). MQTT with TLS option. |
| **Physical device theft** | Future: encrypted data partition |
| **Credential leakage in tool output** | Response leak scanning — redact any known secret patterns |
| **PII leakage to cloud** | Privacy gate (Phase 4) classifies S1/S2/S3 before sending |

### 9.2 Safety Module (Phase 4)

Separate from privacy — handles prompt injection defense and output sanitization. Inspired by IronClaw's `ironclaw_safety` crate.

```rust
pub struct SafetyGuard {
    injection_patterns: Vec<Regex>,
}

impl SafetyGuard {
    pub fn check_input(&self, text: &str) -> SafetyResult {
        for pattern in &self.injection_patterns {
            if pattern.is_match(text) {
                return SafetyResult::Blocked { reason: "Potential prompt injection detected".into() };
            }
        }
        SafetyResult::Safe
    }

    pub fn sanitize_output(&self, text: &str, secrets: &[String]) -> String {
        let mut result = text.to_string();
        for secret in secrets {
            if result.contains(secret) {
                result = result.replace(secret, "[REDACTED]");
            }
        }
        result
    }
}
```

---

## 10. Server & Communication

### 10.1 HTTP API (Phase 3)

```
POST /api/chat                 — send message, get response
POST /api/chat/stream          — send message, get SSE stream
GET  /api/status               — device status, version, uptime
GET  /api/sessions             — list sessions
GET  /api/memory               — read MEMORY.md and daily notes
POST /api/tools/{tool_name}    — direct tool invocation
```

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/chat/stream", post(chat_stream_handler))
        .route("/api/status", get(status_handler))
        .route("/api/sessions", get(sessions_handler))
        .route("/api/memory", get(memory_handler))
        .route("/api/tools/:name", post(tool_handler))
        .with_state(state)
}
```

### 10.2 MQTT (Phase 3)

```
Topic Structure:
  miniclaw/{device_id}/command    — inbound (subscribe)
  miniclaw/{device_id}/response   — outbound (publish)
  miniclaw/{device_id}/status     — device status (publish, retained)
  miniclaw/{device_id}/event      — events (publish)

Message Format (JSON):
  {
    "id": "uuid",
    "type": "chat" | "tool_call" | "status_request",
    "session_id": "string",
    "content": "string",
    "timestamp": "ISO8601"
  }
```

### 10.3 Cron Service (Phase 3)

```rust
#[derive(Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: CronSchedule,
    pub action: String,          // natural language for agent
    pub last_run: Option<DateTime<Utc>>,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize)]
pub enum CronSchedule {
    Every { seconds: u64 },
    Cron(String),                // "0 */2 * * *"
    Once { at: DateTime<Utc> },
}
```

Checks `cron.json` every 60 seconds. Max 16 jobs (MimiClaw-inspired constraint).

### 10.4 Heartbeat Service (Phase 3)

Polls `HEARTBEAT.md` every 30 minutes. If uncompleted items exist (`- [ ]`), injects a prompt into the agent queue. MimiClaw and DuckyClaw both validate this proactive agent pattern.

---

## 11. Voice Pipeline (Phase 5)

### 11.1 Architecture

Inspired by Xiaozhi's proven pipeline. STT is cloud/server-side. TTS is local (Piper) with cloud option for higher quality.

```
Microphone → [WebSocket Audio Stream]
                    │
                    ▼
             Cloud/Server STT
             (Whisper API, FunASR, Deepgram)
                    │
                    ▼
              Text input → Agent Loop → Text output
                                            │
                                      ┌─────┼──────┐
                                      ▼              ▼
                                 Local Piper     Cloud TTS
                                 TTS (~80MB)     (optional)
                                      │              │
                                      ▼              ▼
                                 [Audio output]
```

### 11.2 STT/TTS Provider Options

**STT** (all remote — ESP32/RPi can't do open-vocabulary STT locally):

| Provider | Type | Notes |
|----------|------|-------|
| Whisper API | Cloud | OpenAI, reliable |
| Deepgram | Cloud | Fast, affordable |
| FunASR (SenseVoice) | Self-hosted | 70ms/10s audio, CPU-only, 2GB model |
| SherpaASR | Self-hosted | ONNX-based, streaming |

**TTS**:

| Provider | Type | Size | Notes |
|----------|------|------|-------|
| Piper | Local | ~80MB | Real-time on RPi 4, offline capable |
| OpenAI TTS | Cloud | — | Higher quality |
| CoSyVoice | Self-hosted | — | Streaming, 150ms first chunk |

### 11.3 Self-Hosted Server Option

For users wanting full privacy, Xiaozhi proves a modest home server (4 cores, 8GB RAM) can run FunASR + CoSyVoice + Ollama entirely offline. MiniClaw's agent speaks the same protocol (WebSocket + audio) to work with such a server.

---

## 12. Hardware Abstraction (Phase 6)

### GPIO via sysfs (Linux-native, no external crate dependency)

```rust
// hw/gpio.rs — behind #[cfg(feature = "rpi")]

pub struct GpioPin { pin_number: u32 }

impl GpioPin {
    pub fn export(pin: u32) -> Result<Self> { /* /sys/class/gpio/export */ }
    pub fn set_direction(&self, dir: &str) -> Result<()> { /* .../direction */ }
    pub fn write(&self, value: bool) -> Result<()> { /* .../value */ }
    pub fn read(&self) -> Result<bool> { /* .../value */ }
}
```

Tools registered under `#[cfg(feature = "rpi")]`: `gpio_write`, `gpio_read`, `system_temp`.

---

## 13. Configuration

### 13.1 Config File (TOML)

```toml
# config/config.toml

[agent]
max_iterations = 10
max_tool_calls_per_iteration = 4
consolidation_threshold = 40       # message count (not tokens — no tokenizer needed)
context_cache_ttl_secs = 60
memory_max_bytes = 8192            # max MEMORY.md size before reconsolidation
session_max_age_days = 30          # delete sessions older than this
session_max_count = 100            # keep at most this many sessions

[llm]
provider = "anthropic"             # "anthropic" | "openai_compatible"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-6"
base_url = "https://api.anthropic.com"
max_tokens = 1024
temperature = 0.7
timeout_secs = 60

[llm.fallback]                     # optional
provider = "openai_compatible"
base_url = "http://localhost:11434"
model = "qwen3:0.6b"
api_key_env = ""

[server]                           # Phase 3
http_enabled = true
http_port = 3000
http_bind = "0.0.0.0"
mqtt_enabled = true
mqtt_broker = "localhost"
mqtt_port = 1883
mqtt_device_id = "miniclaw-01"

[cron]                             # Phase 3
enabled = true
check_interval_secs = 60

[heartbeat]                        # Phase 3
enabled = true
interval_secs = 1800

[privacy]                          # Phase 4
enabled = false
classifier_model = "models/privacy-classifier.onnx"
pii_regex_enabled = true

[safety]                           # Phase 4
enabled = false
prompt_injection_defense = true

[voice]                            # Phase 5
enabled = false
stt_provider = "whisper_api"
stt_api_key_env = "OPENAI_API_KEY"
tts_provider = "piper"
tts_model = "models/en_US-lessac-medium.onnx"

[tools]
shell_enabled = true
shell_allowed_commands = ["ls", "cat", "date", "df", "free", "uptime", "ping"]
shell_timeout_secs = 10
http_fetch_enabled = true
http_fetch_timeout_secs = 15

[logging]
level = "info"
file = "logs/miniclaw.log"
max_size_mb = 10
```

### 13.2 Environment Variables

API keys always from env vars, never in config:

```rust
impl LlmConfig {
    pub fn api_key(&self) -> Result<String> {
        if self.api_key_env.is_empty() { return Ok(String::new()); }
        std::env::var(&self.api_key_env)
            .map_err(|_| anyhow!("Set {} environment variable", self.api_key_env))
    }
}
```

---

## 14. File System Layout

```
~/miniclaw/
├── miniclaw                           # single Rust binary (~3-5 MB)
├── config/
│   └── config.toml
├── data/
│   ├── SOUL.md                        # personality definition
│   ├── USER.md                        # user context
│   ├── HEARTBEAT.md                   # proactive task checklist (Phase 3)
│   ├── memory/
│   │   ├── MEMORY.md                  # long-term facts
│   │   ├── MEMORY-FULL.md            # unredacted version (Phase 4)
│   │   ├── 2026-03-22.md             # daily notes
│   │   └── 2026-03-21.md
│   ├── sessions/
│   │   └── cli.jsonl                  # conversation sessions
│   ├── skills/
│   │   └── *.md                       # custom skill definitions
│   ├── cron.json                      # Phase 3
│   └── state.json                     # runtime state
├── models/                            # Phase 4-5
│   ├── privacy-classifier.onnx       # ~20MB
│   └── en_US-lessac-medium.onnx      # Piper TTS ~80MB
├── logs/
│   └── miniclaw.log
└── bin/                               # Phase 5
    └── piper
```

### Default SOUL.md

```markdown
# MiniClaw

You are MiniClaw, a helpful AI assistant running on a local device.

## Identity
- You are a local-first AI agent running on a Raspberry Pi
- You have direct access to the device's file system and network
- You value privacy — sensitive data stays on this device

## Behavior
- Be concise and direct
- When asked to do something, use your tools to actually do it
- If you learn something worth remembering, use the memory_store tool
- Check HEARTBEAT.md for pending tasks when reminded

## Capabilities
- File operations (read, write, edit, list)
- Shell commands (sandboxed)
- Web search and URL fetching
- Scheduled tasks (cron)
- Memory management
- System diagnostics
```

---

## 15. Error Handling & Recovery

### 15.1 Error Strategy

```rust
pub enum AgentError {
    // Recoverable — retry or failover
    LlmTimeout,
    LlmRateLimit { retry_after: Duration },
    NetworkError(reqwest::Error),
    MqttDisconnect,

    // Degraded — continue with reduced functionality
    LlmProviderDown,                // switch to fallback
    ToolExecutionFailed(String),    // report to LLM, let it handle

    // Fatal — log and restart
    ConfigInvalid(String),
    DataCorruption(String),
}
```

### 15.2 Systemd Watchdog

```ini
# /etc/systemd/system/miniclaw.service

[Unit]
Description=MiniClaw AI Agent
After=network-online.target mosquitto.service
Wants=network-online.target

[Service]
Type=notify
ExecStart=/home/pi/miniclaw/miniclaw --config /home/pi/miniclaw/config/config.toml
WorkingDirectory=/home/pi/miniclaw
User=pi
Restart=always
RestartSec=5
WatchdogSec=120
Environment="ANTHROPIC_API_KEY=sk-ant-..."
MemoryMax=512M
CPUQuota=80%

[Install]
WantedBy=multi-user.target
```

---

## 16. Deployment & Operations

### 16.1 Build & Cross-Compile

```bash
# On MacBook — cross-compile for RPi
cargo zigbuild --target aarch64-unknown-linux-gnu --release

# Deploy
rsync -avz target/aarch64-unknown-linux-gnu/release/miniclaw pi@rpi4.local:~/miniclaw/
```

### 16.2 Release Profile

Inspired by IronClaw's optimization. Minimizes binary size for faster SD card load and better instruction cache utilization.

```toml
[profile.release]
lto = true              # link-time optimization
panic = "abort"         # no unwinding overhead
strip = true            # remove debug symbols
codegen-units = 1       # maximize optimization
opt-level = "z"         # optimize for size (better than "3" for I/O-bound RPi workload)
```

### 16.3 CLI Subcommands

```bash
# First-time setup — creates directories, default SOUL.md, template config
$ ./miniclaw init
  Creating ~/miniclaw/data/
  Creating ~/miniclaw/data/memory/
  Creating ~/miniclaw/data/sessions/
  Creating ~/miniclaw/data/skills/
  Creating ~/miniclaw/config/
  Creating ~/miniclaw/logs/
  Writing default data/SOUL.md
  Writing template config/config.toml

  Please set your API key:
    export ANTHROPIC_API_KEY="your-key-here"
  Then run:
    ./miniclaw chat

# Interactive REPL mode (default for terminal)
$ ./miniclaw chat
  MiniClaw v0.1.0 on aarch64-linux
  MiniClaw> What time is it?
  It's 3:42 PM on Saturday, March 22, 2026.
  MiniClaw> _

# Single-shot mode (piped input)
$ echo "What time is it?" | ./miniclaw chat
  It's 3:42 PM.

# Single-shot mode (explicit flag)
$ ./miniclaw chat --message "What time is it?"
  It's 3:42 PM.

# Server mode (Phase 3 — HTTP + MQTT daemon)
$ ./miniclaw serve

# Version
$ ./miniclaw --version
  miniclaw 0.1.0

# Status (Phase 3)
$ ./miniclaw status
```

Auto-detection: if stdin is a TTY, use REPL mode. If piped, use single-shot mode.

If `data/SOUL.md` doesn't exist on startup, create it with default content silently (don't crash).

### 16.4 First-Time Setup on RPi

```bash
# Copy binary
scp target/aarch64-unknown-linux-gnu/release/miniclaw pi@rpi4.local:~/miniclaw/

# Init (creates directories + default files)
ssh pi@rpi4.local 'cd ~/miniclaw && ./miniclaw init'

# Set API key
ssh pi@rpi4.local 'echo "export ANTHROPIC_API_KEY=sk-ant-..." >> ~/.bashrc'

# Run
ssh pi@rpi4.local 'cd ~/miniclaw && ./miniclaw chat'
```

---

## 17. Testing Strategy

### 17.1 Test Layers

```
Unit Tests (cargo test, Mac):              ~80%
  Context builder, tool schemas, PII regex, config parsing,
  session serialization, cron evaluation

Integration Tests (cargo test, mock LLM):  ~15%
  Agent loop with MockLlmClient, tool dispatch, memory
  consolidation, multi-turn conversation, privacy gate

System Tests (on RPi, real API):            ~5%
  End-to-end chat, MQTT flow, cron execution, resource usage
```

### 17.2 Mock LLM Client

```rust
pub struct MockLlmClient {
    responses: Mutex<VecDeque<ChatResponse>>,
    recorded_contexts: Mutex<Vec<Context>>,
}

impl MockLlmClient {
    pub fn text(response: &str) -> Self { /* single text response */ }
    pub fn tool_then_text(tool: &str, args: Value, text: &str) -> Self { /* tool call then text */ }
    pub fn infinite_tool_calls(tool: &str, args: Value) -> Self { /* for max_iterations test */ }
}

#[async_trait]
impl LlmProvider for MockLlmClient {
    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        self.recorded_contexts.lock().await.push(context.clone());
        self.responses.lock().await.pop_front()
            .ok_or_else(|| anyhow!("No more mock responses"))
    }
}
```

### 17.3 Key Test Cases

```rust
#[tokio::test]
async fn test_simple_conversation() { /* text in → text out */ }

#[tokio::test]
async fn test_tool_call_flow() { /* triggers get_time, verifies tool called */ }

#[tokio::test]
async fn test_multi_tool_chain() { /* LLM calls 2 tools, then responds */ }

#[tokio::test]
async fn test_max_iterations_safety() { /* infinite tool calls → stops at max */ }

#[tokio::test]
async fn test_memory_persistence() { /* store fact, restart, recall */ }

#[tokio::test]
async fn test_session_isolation() { /* two sessions don't see each other */ }

#[tokio::test]
async fn test_llm_failover() { /* primary fails, fallback succeeds */ }

#[tokio::test]
async fn test_file_path_escape() { /* ../../../etc/passwd → rejected */ }

#[cfg(feature = "privacy")]
#[tokio::test]
async fn test_pii_stripping() { /* email stripped, rehydrated */ }
```

---

## 18. Build Phases

### Phase 1: "It thinks" (Week 1-2)

Working agent loop. Text in, text out. CLI only.

```
Deliverables:
  ✓ Project scaffold (Cargo.toml, directory structure)
  ✓ `miniclaw init` — create dirs + default SOUL.md + template config
  ✓ `miniclaw --version`
  ✓ Config loading (TOML → Config struct)
  ✓ LLM client (Anthropic native + OpenAI-compatible)
  ✓ Provider failover (primary → fallback)
  ✓ Tool trait + registry + JSON schema generation
  ✓ 5 tools: get_time, read_file, write_file, list_dir, system_info
  ✓ Context builder with cache + budget enforcement
  ✓ Agent loop (ReAct: think → parallel tool calls → loop)
  ✓ Single-owner agent_worker with inbound mpsc + oneshot reply
  ✓ CLI: REPL mode (TTY) + single-shot mode (pipe / --message)
  ✓ Release profile (LTO, strip, panic=abort, opt-level=z)
  ✓ Cross-compile setup (cargo-zigbuild + deploy.sh)

Binary: ~3-5 MB | RAM: ~15-20 MB

Test: ./miniclaw chat --message "What time is it?"
Test: ./miniclaw chat  (interactive REPL)
```

### Phase 2: "It remembers" (Week 3)

Memory persistence and multi-turn conversations.

```
Deliverables:
  ✓ Session store (in-memory + persist-on-change)
  ✓ Memory manager (MEMORY.md read/write)
  ✓ Daily notes (YYYY-MM-DD.md)
  ✓ Memory consolidation (next-turn, message count threshold → summarize → MEMORY.md)
  ✓ MEMORY.md bounds (reconsolidate when exceeding memory_max_bytes)
  ✓ Session cleanup (max age + max count, runs at startup + daily)
  ✓ 3 more tools: edit_file, memory_store, memory_read

Test: Remember a fact → restart → ask about it → remembers
```

### Phase 3: "It serves" (Week 4-5)

Daemon mode, accessible via HTTP and MQTT. Proactive agent.

```
Deliverables:
  ✓ HTTP API (axum): /api/chat, /api/status, /api/sessions
  ✓ MQTT client (rumqttc): subscribe/publish
  ✓ Cron service (cron.json, 60s tick)
  ✓ Heartbeat service (HEARTBEAT.md, 30 min tick)
  ✓ 3 more tools: shell_exec (sandboxed), http_fetch (with credential injection), cron tools
  ✓ Systemd service file
  ✓ Logging (tracing → file)
  ✓ Graceful shutdown (signal handler + persist)
  ✓ Response leak scanning (credential boundary injection)

Test: curl http://rpi4.local:3000/api/chat -d '{"message":"Hello"}'
```

### Phase 4: "It protects" (Week 6-7)

Privacy gate and safety module.

```
Deliverables:
  ✓ PII regex detector (email, phone, API keys, SSH keys)
  ✓ Privacy classifier (ONNX DistilBERT ~20MB, encoder-only)
  ✓ S1/S2/S3 routing in agent loop
  ✓ PII stripping and rehydration
  ✓ Dual-track memory (MEMORY.md clean + MEMORY-FULL.md)
  ✓ Safety module: prompt injection defense, output sanitization
  ✓ Local-only handler for S3 content

Test: "My API key is sk-abc123" → never reaches cloud LLM
```

### Phase 5: "It speaks" (Week 8-10)

Voice input and output.

```
Deliverables:
  ✓ Piper TTS integration (local, ~80MB model)
  ✓ STT integration (cloud Whisper API or self-hosted)
  ✓ WebSocket audio streaming endpoint
  ✓ Audio output via speaker

Test: Speak → STT → Agent → TTS → Hear response
```

### Phase 6: "It controls" (Week 11+)

Physical hardware I/O and MCP.

```
Deliverables:
  ✓ GPIO tools (sysfs)
  ✓ I2C device scanning
  ✓ System temperature monitoring
  ✓ MCP server (expose tools to network)

Test: "Turn on GPIO pin 17" → LED lights up
```

---

## Appendix A: RAM Budget (Realistic)

### RPi 3 (1GB) — Measured Estimates

```
Total:                              1024 MB
OS (headless RPi OS Lite):          ~150 MB
Mosquitto MQTT:                       ~2 MB
MiniClaw binary RSS:                ~8-12 MB
  Tokio runtime:                     ~2-3 MB
  reqwest + connections:             ~2-3 MB
  Session cache + context:           ~2-5 MB
  axum HTTP server:                  ~1-2 MB
                                    ────────
Phase 1-3 total:                    ~165 MB (860 MB free)

Phase 4 additions:
  ONNX Runtime + classifier:         ~50 MB
                                    ────────
Phase 4 total:                      ~215 MB (810 MB free)

Phase 5 additions:
  Piper TTS model + runtime:         ~80 MB
                                    ────────
Phase 5 total:                      ~295 MB (730 MB free)

Optional future:
  Qwen3 0.6B Q4 (local LLM):       ~400 MB → 330 MB free
  TinyLlama 1.1B Q4:               ~700 MB → 30 MB free (tight)
```

### RPi 4 (4GB) — Your Test Device

Phase 1-3: ~165 MB used, **3.8 GB free**. Massive headroom for debug tools, larger models, experimentation.

## Appendix B: Latency Budget

```
Input parsing:                          ~1 ms
Privacy gate (regex):                   ~1 ms   (Phase 4)
Privacy gate (ONNX classifier):       ~50 ms   (Phase 4)
Context building (from cache):          ~1 ms
LLM API call (network + inference): 1000-3000 ms  ← dominant
Response parsing:                       ~1 ms
Tool execution (local):             1-100 ms
Output dispatch:                        ~1 ms
TTS synthesis (Piper, Phase 5):    200-500 ms
                                    ──────────
Total without voice:                1-3 seconds
Total with voice I/O:              2-5 seconds
```

## Appendix C: Key Crate Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream", "rustls-tls"], default-features = false }
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rumqttc = "0.24"
tracing = "0.1"
tracing-subscriber = "0.3"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "2"
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }
cron = "0.15"
async-trait = "0.1"
tokio-stream = "0.1"
futures = "0.3"

[features]
default = []
privacy = ["ort"]       # ONNX Runtime for classifier
safety = []             # prompt injection defense (pure Rust, no extra deps)
voice = []              # TTS/STT integration
rpi = []                # GPIO, I2C, sensors
mcp = []                # MCP server

[profile.release]
lto = true
panic = "abort"
strip = true
codegen-units = 1
opt-level = "z"
```

## Appendix D: Wire Protocols

### HTTP API

```
Request:  Content-Type: application/json
Response: application/json (normal) | text/event-stream (SSE)

SSE format:
  event: text_delta
  data: {"text": "partial..."}

  event: tool_call
  data: {"name": "get_time", "arguments": {}}

  event: done
  data: {"usage": {"input_tokens": 150, "output_tokens": 80}}
```

### MQTT Topics

```
miniclaw/{device_id}/command     QoS 1, subscribe
miniclaw/{device_id}/response    QoS 1, publish
miniclaw/{device_id}/status      QoS 1, publish, retained
miniclaw/{device_id}/event       QoS 0, publish
```

---

*Last updated: 2026-03-23. Version 3 — self-review fixes: single-owner concurrency (no dual invocation), background consolidation, memory bounds, context budget enforcement, parallel tool execution, CLI subcommands, session cleanup, init command, mixed content type fix.*
