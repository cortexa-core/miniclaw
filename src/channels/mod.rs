use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};
use crate::config::Config;

/// Sender type for communicating with the agent worker.
pub type AgentSender = mpsc::Sender<(Input, oneshot::Sender<Output>)>;

/// Every messaging channel implements this trait.
#[allow(dead_code)]
#[async_trait]
pub trait Channel: Send + Sync {
    /// Unique channel name (e.g., "telegram", "discord")
    fn name(&self) -> &str;

    /// Long-running loop: connect to platform, receive messages,
    /// send to agent via agent_tx, send responses back to platform.
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

    let _ = &agent_tx; // suppress unused warning when no channel features enabled
    let _ = config;
    let _ = &tasks;
}
