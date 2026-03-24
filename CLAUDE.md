# MiniClaw

A privacy-first, voice-capable AI agent OS for resource-constrained ARM Linux SBCs.

## What This Is

MiniClaw is a lightweight OpenClaw-like AI agent that runs on Raspberry Pi 3+ (1GB+ RAM) as a single Rust binary. The agent loop runs locally on the device. Only LLM inference is remote (cloud API call). Tools, memory, privacy gate, and state all live on the device.

## Tech Stack

- **Language**: Rust (edition 2024, multi-threaded Tokio async runtime)
- **Target**: aarch64-unknown-linux-gnu (Raspberry Pi 3/4/5, RK3566, any ARM Linux SBC)
- **Dev Machine**: macOS (cross-compile via cargo-zigbuild)
- **Test Hardware**: Raspberry Pi 4 (4GB)
- **Binary Size**: ~3-5MB (LTO + strip + panic=abort + opt-level=z)
- **RAM**: ~15-20MB Phase 1, ~295MB with all features (TTS + privacy classifier)

## Key Design Decisions

1. **Agent loop runs LOCAL**, LLM inference is REMOTE (standard API call, not a cloud agent service)
2. **Single-owner concurrency**: `agent_worker` task is the sole owner of `Agent`. All inputs arrive via mpsc channel. Request-response sources (HTTP, CLI) get replies via oneshot channel. No `Arc<Mutex<Agent>>`, no shared mutable state.
3. **Multi-threaded Tokio** — I/O tasks (HTTP, MQTT, cron) never block each other
4. **Two LLM providers**: Anthropic native + OpenAI-compatible (~350 lines, hand-rolled, no rig-core)
5. **Provider failover**: primary → fallback → error
6. **Credential boundary injection** (from IronClaw): API keys never enter LLM context
7. **Parallel tool execution**: multiple independent tool calls run concurrently via `join_all`
8. **Privacy gate** (Phase 4): ~20MB encoder-only ONNX classifier for S1/S2/S3
9. **No local LLM for conversation** — cloud for thinking, local for everything else
10. **Offline mode**: rule-based command handler + local Piper TTS, not a bad local LLM
11. **Single Rust binary**, no Python/Node.js/JVM dependency, rustls (no OpenSSL)
12. **Feature flags** for optional modules: `privacy`, `safety`, `voice`, `rpi`, `mcp`
13. **In-memory session store** with persist-on-change (reduce SD card I/O)
14. **Message count threshold** for consolidation (no tokenizer dependency). Runs at start of next turn, not blocking current.
15. **Memory bounds**: MEMORY.md auto-reconsolidates when exceeding `memory_max_bytes`. Context budget enforced per component.
16. **Session cleanup**: old sessions pruned by age and count
17. **Graceful shutdown** with signal handler + session persistence

## Development Workflow

```bash
# Build for Mac (fast iteration)
cargo build
cargo test

# Build for RPi (cross-compile)
cargo zigbuild --target aarch64-unknown-linux-gnu --release

# Deploy to RPi
./deploy.sh
```

## Project Structure

```
src/
  main.rs           — entry point, CLI, signal handling, task spawning
  config.rs         — TOML config loading + validation
  agent/
    mod.rs
    loop.rs         — ReAct agent loop (THE core, ~500 lines)
    context.rs      — context builder with caching
    memory.rs       — session store + memory manager + consolidation
  llm/
    mod.rs
    client.rs       — LlmProvider trait
    types.rs        — canonical types (ChatRequest, ChatResponse, ToolCall)
    anthropic.rs    — Anthropic Messages API serialization (~200 lines)
    openai.rs       — OpenAI Chat Completions serialization (~150 lines)
  tools/
    mod.rs
    registry.rs     — Tool trait, registry, dispatch
    [tool files]    — individual tool implementations
  server/           — Phase 3
    mod.rs
    http.rs         — axum HTTP API
    mqtt.rs         — MQTT pub/sub
```

## Build Phases

- **Phase 1**: Agent loop + LLM + 5 tools + CLI (Week 1-2) — **START HERE**
- Phase 2: Memory persistence + consolidation (Week 3)
- Phase 3: HTTP API + MQTT + cron + heartbeat + systemd (Week 4-5)
- Phase 4: Privacy gate + safety module (Week 6-7)
- Phase 5: Voice pipeline (STT cloud + TTS local Piper) (Week 8-10)
- Phase 6: Hardware I/O + MCP (Week 11+)

## Important Files

- `DESIGN.md` — comprehensive end-to-end design document (v3, with self-review fixes)
- `config/config.toml` — runtime configuration
- `data/SOUL.md` — agent personality
- `data/USER.md` — user context
- `data/memory/MEMORY.md` — long-term memory
- `data/HEARTBEAT.md` — proactive task checklist

## Conventions

- `anyhow` for error handling in application code, `thiserror` for library-like modules
- All async, all Tokio — no std threads, multi-threaded runtime
- Feature flags gate optional modules: `#[cfg(feature = "rpi")]`
- API keys from env vars only, never in config files
- File operations restricted to data/ directory (sandboxed path validation)
- TOML for config, JSONL for sessions, Markdown for memory
- Credential boundary injection: secrets substituted in tool HTTP requests after LLM generates them, responses scanned for leaks
- Release profile: `lto = true, panic = "abort", strip = true, codegen-units = 1, opt-level = "z"`

## Learnings Incorporated From

- **IronClaw**: credential injection, release profile, safety module, provider failover, job state machine
- **MimiClaw**: context builder from files, heartbeat/cron, CLI fallback, OTA concept
- **NanoBot**: memory consolidation, MCP pattern, config-driven design
- **EdgeClaw**: S1/S2/S3 privacy classification, dual-track memory, PII detection
- **DuckyClaw**: proactive agent pattern (but avoid Tuya vendor lock-in)
- **Xiaozhi**: voice pipeline protocol, self-hosted STT/TTS architecture
