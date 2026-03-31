# UniClaw Channel System — Design Spec

## Overview

Add a messaging channel system that lets users chat with UniClaw through Telegram, Discord, and future platforms. Starting with Telegram. Channels are feature-flagged Rust modules compiled into the binary.

## Architecture

### Channel Trait

Every channel implements one trait — a long-running async loop that bridges a platform to the agent worker:

```rust
pub type AgentSender = mpsc::Sender<(Input, oneshot::Sender<Output>)>;

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, agent_tx: AgentSender) -> Result<()>;
}
```

The channel's `run()` method:
1. Connects to the platform (long polling for Telegram, WebSocket for Discord)
2. Receives a message
3. Checks authorization (`allowed_users`)
4. Normalizes to `Input { id, session_id, content }`
5. Creates a oneshot reply channel
6. Sends `(Input, reply_tx)` to agent worker via `agent_tx`
7. Sends typing indicator while waiting
8. Awaits response via oneshot
9. Chunks long responses for platform limits
10. Sends response back to platform
11. Loops back to step 2

This is the same pattern used by HTTP, MQTT, cron, heartbeat — just another sender to the same `agent_worker` mpsc channel.

### Session IDs

Each channel generates deterministic session IDs encoding the source:

```
telegram:{user_id}           — per-user persistent conversation
discord:{guild}:{channel}    — per-channel (future)
whatsapp:{phone}             — per-phone (future)
```

Same user gets continuous conversation memory. Different platforms don't mix.

### Feature Flags

```toml
[features]
default = []
telegram = ["teloxide"]
# discord = ["serenity", "poise"]    # future
```

Binary without channels: 3.5MB. With Telegram: ~5-6MB. Users only pay for what they enable. Prebuilt releases offer both "minimal" and "full" variants.

### Configuration

```toml
[channels.telegram]
enabled = true
bot_token_env = "TELEGRAM_BOT_TOKEN"
allowed_users = []              # empty = allow all
respond_in_groups = "mention"   # "mention" | "always" | "never"
```

### Config Struct

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token_env: String,
    #[serde(default)]
    pub allowed_users: Vec<i64>,
    #[serde(default = "default_mention")]
    pub respond_in_groups: String,
}
```

### File Structure

```
src/channels/
  mod.rs            — Channel trait, AgentSender type, spawn_channels()
  telegram.rs       — #[cfg(feature = "telegram")] Telegram via teloxide
```

### Integration with main.rs

In `run_serve()`, after spawning HTTP/MQTT/cron/heartbeat:

```rust
// Spawn messaging channels
channels::spawn_channels(&config, inbound_tx.clone(), &mut tasks);
```

`spawn_channels` reads channel configs, creates enabled channels, spawns each as a tokio task — same pattern as cron/heartbeat.

## Telegram Implementation

### Library

teloxide — the main Rust Telegram library. Async with Tokio, uses reqwest + rustls (same TLS stack we use). Supports Bot API v9.1.

### Connection: Long Polling

Long polling is ideal for edge devices:
- No public URL needed
- Works behind any NAT/firewall
- No reverse proxy or tunnel required
- teloxide handles the polling loop internally

### Message Handling

For each incoming message:
1. Extract user ID and chat ID
2. Check `allowed_users` (empty = allow all)
3. For group chats: check `respond_in_groups` policy
   - `"mention"`: only respond if bot is @mentioned
   - `"always"`: respond to all messages
   - `"never"`: ignore group messages
4. Send `ChatAction::Typing` while processing
5. Create Input with `session_id = format!("telegram:{user_id}")`
6. Send to agent, await response
7. Chunk response at 4096 chars (Telegram limit)
8. Send chunks with MarkdownV2 formatting, fallback to plain text on parse error

### Rate Limiting

- 1 message/second per chat (Telegram's practical limit)
- On 429 response, respect `retry_after` header
- teloxide handles basic rate limiting internally

### Error Handling

- Platform errors (network, 429): log, retry with backoff
- Agent errors: send user-friendly message "Sorry, something went wrong."
- Auth rejection: silently ignore (don't reveal bot exists to unauthorized users)
- Agent timeout: send "Request timed out, please try again."

### Bot Setup (User Flow)

1. Message @BotFather on Telegram → `/newbot` → get token
2. Set `TELEGRAM_BOT_TOKEN=your-token`
3. Add to config:
   ```toml
   [channels.telegram]
   enabled = true
   bot_token_env = "TELEGRAM_BOT_TOKEN"
   ```
4. `uniclaw serve` → bot starts accepting messages

## What This Does NOT Include (Future)

- Discord channel (serenity + poise, separate feature flag)
- WhatsApp channel (wa-rs or Business API, when crate matures)
- Media handling (images, voice) — text only for v1
- Inline keyboards / custom Telegram UI elements
- Multi-bot support
- Channel health monitoring / auto-restart (add when we have 3+ channels)
