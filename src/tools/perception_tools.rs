use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// DescribeSceneTool
// ---------------------------------------------------------------------------

/// Returns the latest scene description from world state (no new VLM call).
/// Fast — reads the cached description that the perception pipeline maintains.
pub struct DescribeSceneTool;

#[async_trait]
impl Tool for DescribeSceneTool {
    fn name(&self) -> &str {
        "describe_scene"
    }

    fn description(&self) -> &str {
        "Get the latest scene description from the robot's camera. Returns the most recent \
         cached description without triggering a new image capture."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(ref rx) = ctx.world_rx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        let state = rx.borrow().clone();
        match state.scene_description {
            Some(desc) => {
                let age = state
                    .scene_timestamp
                    .map(|t| format!(" ({}s ago)", t.elapsed().as_secs()))
                    .unwrap_or_default();
                ToolResult::Success(format!("{desc}{age}"))
            }
            None => ToolResult::Success(
                "No scene description available yet. The camera may not be active.".into(),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// TakePhotoTool
// ---------------------------------------------------------------------------

/// Requests a fresh scene description. Currently reads from the cached world
/// state (same as DescribeSceneTool). When on-demand capture is wired up,
/// this will trigger a new frame capture + VLM call.
pub struct TakePhotoTool;

#[async_trait]
impl Tool for TakePhotoTool {
    fn name(&self) -> &str {
        "take_photo"
    }

    fn description(&self) -> &str {
        "Take a photo with the robot's camera and describe what is seen. \
         Returns a fresh scene description from the vision model."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(ref rx) = ctx.world_rx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        // TODO: trigger on-demand capture via a channel to the perception pipeline
        // For now, return the latest cached scene with age info.
        let state = rx.borrow().clone();
        match state.scene_description {
            Some(desc) => {
                let age = state
                    .scene_timestamp
                    .map(|t| {
                        let secs = t.elapsed().as_secs();
                        if secs < 5 {
                            " (fresh)".to_string()
                        } else {
                            format!(" (captured {}s ago)", secs)
                        }
                    })
                    .unwrap_or_default();
                ToolResult::Success(format!("{desc}{age}"))
            }
            None => ToolResult::Success(
                "No scene description available. The perception pipeline may not be running."
                    .into(),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::robot::world_state::WorldState;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_ctx_with_world(
        world_tx: &tokio::sync::watch::Sender<WorldState>,
    ) -> (ToolContext, tokio::sync::watch::Receiver<WorldState>) {
        let world_rx = world_tx.subscribe();
        let ctx = ToolContext {
            data_dir: PathBuf::from("/tmp/uniclaw-test"),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str::<Config>("[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\"")
                    .unwrap(),
            ),
            action_tx: None,
            world_rx: Some(world_rx),
        };
        // We need a second receiver for assertions
        let assert_rx = world_tx.subscribe();
        (ctx, assert_rx)
    }

    fn test_ctx_no_robot() -> ToolContext {
        ToolContext {
            data_dir: PathBuf::from("/tmp/uniclaw-test"),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str::<Config>("[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\"")
                    .unwrap(),
            ),
            action_tx: None,
            world_rx: None,
        }
    }

    #[tokio::test]
    async fn describe_scene_returns_cached_description() {
        let (world_tx, _) = tokio::sync::watch::channel(WorldState::default());
        world_tx.send_modify(|state| {
            state.scene_description = Some("A desk with a laptop and coffee mug.".into());
            state.scene_timestamp = Some(std::time::Instant::now());
        });
        let (ctx, _rx) = test_ctx_with_world(&world_tx);
        let tool = DescribeSceneTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("desk"));
        assert!(result.content().contains("laptop"));
    }

    #[tokio::test]
    async fn describe_scene_no_description_yet() {
        let (world_tx, _) = tokio::sync::watch::channel(WorldState::default());
        let (ctx, _rx) = test_ctx_with_world(&world_tx);
        let tool = DescribeSceneTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("No scene description"));
    }

    #[tokio::test]
    async fn describe_scene_not_robot_mode() {
        let ctx = test_ctx_no_robot();
        let tool = DescribeSceneTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_error());
        assert!(result.content().contains("Not in robot mode"));
    }

    #[tokio::test]
    async fn take_photo_returns_cached_description() {
        let (world_tx, _) = tokio::sync::watch::channel(WorldState::default());
        world_tx.send_modify(|state| {
            state.scene_description = Some("A person standing near a window.".into());
            state.scene_timestamp = Some(std::time::Instant::now());
        });
        let (ctx, _rx) = test_ctx_with_world(&world_tx);
        let tool = TakePhotoTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("person"));
        assert!(result.content().contains("window"));
    }

    #[tokio::test]
    async fn take_photo_not_robot_mode() {
        let ctx = test_ctx_no_robot();
        let tool = TakePhotoTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_error());
        assert!(result.content().contains("Not in robot mode"));
    }
}
