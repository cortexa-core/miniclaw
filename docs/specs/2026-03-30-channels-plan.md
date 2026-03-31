# UniClaw Channel System — Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development to implement task-by-task.

**Goal:** Add extensible messaging channel system, starting with Telegram. Feature-flagged, compiled into binary.

**Architecture:** Channel trait with `run()` method → sends to existing agent_worker via mpsc. Each channel is a feature-flagged module. Telegram uses teloxide with long polling.

**Tech Stack:** Rust, teloxide (Telegram), tokio, feature flags.

---

## File Map

### New Files
- `src/channels/mod.rs` — Channel trait, AgentSender type, ChannelsConfig, spawn_channels()
- `src/channels/telegram.rs` — TelegramChannel implementation (behind `#[cfg(feature = "telegram")]`)

### Modified Files
- `Cargo.toml` — add teloxide dependency (optional), add `telegram` feature
- `src/config.rs` — add `ChannelsConfig` and `TelegramConfig`, add `channels` field to `Config`
- `src/main.rs` — add `mod channels`, call `spawn_channels()` in `run_serve()`
- `src/lib.rs` — add `pub mod channels`
- `config/default_config.toml` — add `[channels.telegram]` section (disabled by default)

---

## Task 1: Add Channel trait and module structure

**Files:**
- Create: `src/channels/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs` (add mod only, no spawn yet)

- [ ] **Step 1: Create src/channels/mod.rs**

```rust
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};
use crate::config::Config;

/// Sender type for communicating with the agent worker.
pub type AgentSender = mpsc::Sender<(Input, oneshot::Sender<Output>)>;

/// Every messaging channel implements this trait.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Unique channel name (e.g., "telegram", "discord")
    fn name(&self) -> &str;

    /// Long-running loop: connect to platform, receive messages,
    /// send to agent via agent_tx, send responses back to platform.
    /// Returns only on error or shutdown.
    async fn run(&self, agent_tx: AgentSender) -> Result<()>;
}

#[cfg(feature = "telegram")]
pub mod telegram;

/// Spawn all enabled channels as tokio tasks.
pub fn spawn_channels(
    config: &Config,
    agent_tx: AgentSender,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    #[cfg(feature = "telegram")]
    {
        if let Some(ref tg_config) = config.channels.telegram {
            if tg_config.enabled {
                match telegram::TelegramChannel::new(tg_config) {
                    Ok(channel) => {
                        let tx = agent_tx.clone();
                        tracing::info!("Starting Telegram channel");
                        tasks.push(tokio::spawn(async move {
                            if let Err(e) = channel.run(tx).await {
                                tracing::error!("Telegram channel error: {e}");
                            }
                        }));
                    }
                    Err(e) => {
                        tracing::error!("Failed to create Telegram channel: {e}");
                    }
                }
            }
        }
    }

    let _ = agent_tx; // suppress unused warning when no channel features enabled
}
```

- [ ] **Step 2: Add `pub mod channels` to src/lib.rs**

```rust
pub mod agent;
pub mod channels;
pub mod config;
pub mod llm;
pub mod mcp;
pub mod server;
pub mod tools;
```

- [ ] **Step 3: Add `mod channels` to src/main.rs** (after `mod tools;`)

```rust
mod channels;
```

- [ ] **Step 4: Build and test**

```bash
cargo build  # zero errors
cargo test   # all pass
```

- [ ] **Step 5: Commit**

```bash
git add src/channels/mod.rs src/lib.rs src/main.rs
git commit -m "feat: add Channel trait and module structure for messaging channels

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add channel configuration to Config

**Files:**
- Modify: `src/config.rs`
- Modify: `config/default_config.toml`

- [ ] **Step 1: Add ChannelsConfig and TelegramConfig to src/config.rs**

Add these structs after `LoggingConfig`:

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TelegramConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token_env: String,
    #[serde(default)]
    pub allowed_users: Vec<i64>,
    #[serde(default = "default_respond_in_groups")]
    pub respond_in_groups: String,
}

fn default_respond_in_groups() -> String { "mention".to_string() }
```

- [ ] **Step 2: Add `channels` field to Config struct**

```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub channels: ChannelsConfig,
    // ... rest of fields ...
}
```

Add it after `mcp_servers` and before `tools`.

- [ ] **Step 3: Add commented-out section to config/default_config.toml**

```toml
# Messaging channels
# [channels.telegram]
# enabled = true
# bot_token_env = "TELEGRAM_BOT_TOKEN"
# allowed_users = []
# respond_in_groups = "mention"
```

- [ ] **Step 4: Build and test**

```bash
cargo build  # verify existing config still parses (default = empty channels)
cargo test   # all pass
```

- [ ] **Step 5: Commit**

```bash
git add src/config.rs config/default_config.toml
git commit -m "feat: add channel configuration (Telegram config struct)

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Implement Telegram channel

**Files:**
- Create: `src/channels/telegram.rs`
- Modify: `Cargo.toml` (add teloxide dependency + feature)

- [ ] **Step 1: Add teloxide to Cargo.toml**

In `[dependencies]`:
```toml
teloxide = { version = "0.13", features = ["macros"], optional = true }
```

In `[features]`:
```toml
[features]
default = []
telegram = ["teloxide"]
```

- [ ] **Step 2: Create src/channels/telegram.rs**

```rust
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ParseMode};
use tokio::sync::oneshot;

use super::{AgentSender, Channel};
use crate::agent::{Input, Output};
use crate::config::TelegramConfig;

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<i64>,
    respond_in_groups: String,
}

impl TelegramChannel {
    pub fn new(config: &TelegramConfig) -> Result<Self> {
        let bot_token = if config.bot_token_env.is_empty() {
            return Err(anyhow!("Telegram bot_token_env not configured"));
        } else {
            std::env::var(&config.bot_token_env)
                .map_err(|_| anyhow!("Environment variable {} is not set", config.bot_token_env))?
        };

        if bot_token.is_empty() {
            return Err(anyhow!("Telegram bot token is empty"));
        }

        Ok(Self {
            bot_token,
            allowed_users: config.allowed_users.clone(),
            respond_in_groups: config.respond_in_groups.clone(),
        })
    }

    fn is_allowed(&self, user_id: i64) -> bool {
        self.allowed_users.is_empty() || self.allowed_users.contains(&user_id)
    }

    fn should_respond_in_group(&self, message: &Message) -> bool {
        match self.respond_in_groups.as_str() {
            "always" => true,
            "never" => false,
            _ => {
                // "mention" — respond only if bot is mentioned
                if let Some(text) = message.text() {
                    if let Some(bot_name) = message.via_bot.as_ref().map(|b| &b.username) {
                        return text.contains(&format!("@{}", bot_name.as_deref().unwrap_or("")));
                    }
                    // Also respond to replies to the bot's messages
                    if let Some(reply) = &message.reply_to_message() {
                        if reply.from.as_ref().map(|u| u.is_bot).unwrap_or(false) {
                            return true;
                        }
                    }
                }
                false
            }
        }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn run(&self, agent_tx: AgentSender) -> Result<()> {
        let bot = Bot::new(&self.bot_token);

        // Verify bot token is valid
        let me = bot.get_me().await
            .map_err(|e| anyhow!("Failed to connect to Telegram: {e}"))?;
        tracing::info!(
            "Telegram bot connected: @{} ({})",
            me.username.as_deref().unwrap_or("unknown"),
            me.first_name
        );

        let allowed_users = self.allowed_users.clone();
        let respond_in_groups = self.respond_in_groups.clone();

        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let agent_tx = agent_tx.clone();
            let allowed_users = allowed_users.clone();
            let respond_in_groups = respond_in_groups.clone();

            async move {
                // Only handle text messages
                let text = match msg.text() {
                    Some(t) => t.to_string(),
                    None => return Ok(()),
                };

                // Check user authorization
                let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
                if !allowed_users.is_empty() && !allowed_users.contains(&user_id) {
                    return Ok(()); // Silently ignore unauthorized users
                }

                // Check group policy
                let is_group = msg.chat.is_group() || msg.chat.is_supergroup();
                if is_group {
                    let should_respond = match respond_in_groups.as_str() {
                        "always" => true,
                        "never" => return Ok(()),
                        _ => {
                            // "mention" — check if bot username is in the text
                            let bot_me = bot.get_me().await?;
                            let bot_username = bot_me.username.as_deref().unwrap_or("");
                            let mentioned = text.contains(&format!("@{bot_username}"));
                            // Also respond to replies to bot
                            let is_reply_to_bot = msg.reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| u.is_bot)
                                .unwrap_or(false);
                            mentioned || is_reply_to_bot
                        }
                    };
                    if !should_respond {
                        return Ok(());
                    }
                }

                // Send typing indicator
                bot.send_chat_action(msg.chat.id, ChatAction::Typing).await.ok();

                // Build session ID: "telegram:{user_id}"
                let session_id = format!("telegram:{user_id}");

                let input = Input {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id,
                    content: text,
                };

                let (reply_tx, reply_rx) = oneshot::channel::<Output>();

                // Send to agent worker
                if agent_tx.send((input, reply_tx)).await.is_err() {
                    bot.send_message(msg.chat.id, "Agent is unavailable. Please try again later.")
                        .await.ok();
                    return Ok(());
                }

                // Wait for response with timeout
                let response = match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    reply_rx,
                ).await {
                    Ok(Ok(output)) => output.content,
                    Ok(Err(_)) => "Something went wrong. Please try again.".to_string(),
                    Err(_) => "Request timed out. Please try again.".to_string(),
                };

                // Chunk and send response (Telegram limit: 4096 chars)
                let chunks = chunk_message(&response, 4096);
                for chunk in chunks {
                    // Try MarkdownV2 first, fall back to plain text
                    let result = bot.send_message(msg.chat.id, &chunk)
                        .parse_mode(ParseMode::MarkdownV2)
                        .await;

                    if result.is_err() {
                        // Markdown parse failed — send as plain text
                        bot.send_message(msg.chat.id, &chunk).await.ok();
                    }
                }

                Ok(())
            }
        })
        .await;

        Ok(())
    }
}

/// Split a message into chunks at paragraph or line boundaries.
fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let search = &remaining[..max_len];

        // Find a good split point: paragraph > line > space
        let split_at = search.rfind("\n\n")
            .or_else(|| search.rfind('\n'))
            .or_else(|| search.rfind(' '))
            .unwrap_or(max_len);

        let split_at = if split_at == 0 { max_len } else { split_at };

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_short_message() {
        let chunks = chunk_message("Hello world", 4096);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_chunk_long_message() {
        let text = "a".repeat(5000);
        let chunks = chunk_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 4096);
    }

    #[test]
    fn test_chunk_at_paragraph() {
        let text = format!("{}\n\n{}", "a".repeat(2000), "b".repeat(3000));
        let chunks = chunk_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('a'));
        assert!(chunks[1].starts_with('b'));
    }
}
```

- [ ] **Step 3: Build with feature flag**

```bash
cargo build --features telegram   # verify compiles
cargo test --features telegram    # all tests pass
cargo build                       # verify still builds WITHOUT telegram
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/channels/telegram.rs
git commit -m "feat: implement Telegram channel via teloxide with long polling

Feature-flagged: cargo build --features telegram
- Long polling (no public URL needed, works behind NAT)
- Per-user sessions (telegram:{user_id})
- Group policy: mention/always/never
- Authorization via allowed_users list
- Typing indicator while agent processes
- Response chunking at 4096 chars
- MarkdownV2 with plain text fallback

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Wire channels into serve command

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add channels::spawn_channels call in run_serve()**

In `run_serve()`, after the heartbeat task spawn block (around line 325) and before the `if tasks.is_empty()` check, add:

```rust
    // Messaging channels (Telegram, Discord, etc.)
    channels::spawn_channels(&config, inbound_tx.clone(), &mut tasks);
```

- [ ] **Step 2: Build and test**

```bash
cargo build                     # without telegram — still works
cargo build --features telegram # with telegram — compiles
cargo test                      # all pass
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire channel spawning into serve command

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Integration test and documentation

**Files:**
- Modify: `README.md` (add Telegram section)
- Modify: `build-release.sh` (add telegram feature to release builds)

- [ ] **Step 1: Update build-release.sh**

Change the cargo zigbuild line to include the telegram feature:

```bash
cargo zigbuild --target "$TARGET" --release --features telegram 2>&1 | tail -1
```

- [ ] **Step 2: Add Telegram section to README.md**

After the "Skills" section, add:

```markdown
### Messaging Channels

Chat with UniClaw through messaging platforms. Currently supported: Telegram.

**Telegram setup:**

1. Message [@BotFather](https://t.me/botfather) on Telegram → `/newbot` → get your bot token
2. Set the token: `export TELEGRAM_BOT_TOKEN="your-token"`
3. Add to config:
   ```toml
   [channels.telegram]
   enabled = true
   bot_token_env = "TELEGRAM_BOT_TOKEN"
   ```
4. Start: `uniclaw serve`
5. Message your bot on Telegram

**Options:**
- `allowed_users = [123456]` — restrict to specific Telegram user IDs (empty = allow all)
- `respond_in_groups = "mention"` — in groups: `"mention"` (only when @mentioned), `"always"`, or `"never"`

Build with Telegram support: `cargo build --release --features telegram`
```

- [ ] **Step 3: Manual end-to-end test**

```bash
export TELEGRAM_BOT_TOKEN="your-token"
cargo run --features telegram -- serve

# On Telegram: message the bot
# Verify: agent responds, tools work, session persists
```

- [ ] **Step 4: Commit**

```bash
git add README.md build-release.sh
git commit -m "feat: add Telegram docs to README, include in release builds

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Summary

| Task | What | Files |
|------|------|-------|
| 1 | Channel trait + module structure | channels/mod.rs, lib.rs, main.rs |
| 2 | Channel config structs | config.rs, default_config.toml |
| 3 | Telegram implementation | channels/telegram.rs, Cargo.toml |
| 4 | Wire into serve command | main.rs |
| 5 | Docs + release build | README.md, build-release.sh |

5 tasks, ~400 lines of new Rust code, 3 new tests. Feature-flagged — base binary unchanged.
