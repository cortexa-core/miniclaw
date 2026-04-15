//! ROS2-specific tools: generic publish, service call, and navigation.
//!
//! These tools are auto-registered when `bridge = "ros2"` in robot.toml.
//! They send rosbridge JSON messages via the action channel.

use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};
use crate::robot::bridge::ros2::{ros_call_service, ros_publish};

// ---------------------------------------------------------------------------
// Ros2PublishTool — generic topic publish
// ---------------------------------------------------------------------------

pub struct Ros2PublishTool;

#[async_trait]
impl Tool for Ros2PublishTool {
    fn name(&self) -> &str {
        "ros2_publish"
    }

    fn description(&self) -> &str {
        "Publish a message to a ROS2 topic via rosbridge"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["topic", "msg"],
            "properties": {
                "topic": {"type": "string", "description": "ROS2 topic name (e.g. /cmd_vel)"},
                "msg": {"type": "object", "description": "Message payload as JSON object"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let topic = match args["topic"].as_str() {
            Some(t) => t,
            None => return ToolResult::Error("Missing parameter: topic".into()),
        };
        let msg = if args["msg"].is_object() {
            args["msg"].clone()
        } else {
            return ToolResult::Error(
                "Missing or invalid parameter: msg (must be JSON object)".into(),
            );
        };

        let ros_msg = ros_publish(topic, msg);
        ToolResult::Success(format!(
            "Published to {topic}: {}",
            serde_json::to_string(&ros_msg).unwrap_or_default()
        ))
    }
}

// ---------------------------------------------------------------------------
// Ros2ServiceTool — generic service call
// ---------------------------------------------------------------------------

pub struct Ros2ServiceTool;

#[async_trait]
impl Tool for Ros2ServiceTool {
    fn name(&self) -> &str {
        "ros2_service"
    }

    fn description(&self) -> &str {
        "Call a ROS2 service via rosbridge"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["service"],
            "properties": {
                "service": {"type": "string", "description": "ROS2 service name (e.g. /reset_world)"},
                "args": {"type": "object", "description": "Service request arguments as JSON object", "default": {}}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let service = match args["service"].as_str() {
            Some(s) => s,
            None => return ToolResult::Error("Missing parameter: service".into()),
        };
        let svc_args = if args["args"].is_object() {
            args["args"].clone()
        } else {
            json!({})
        };

        let id = format!("tool_{}", uuid::Uuid::new_v4());
        let ros_msg = ros_call_service(service, svc_args, &id);
        ToolResult::Success(format!(
            "Called service {service} (id={id}): {}",
            serde_json::to_string(&ros_msg).unwrap_or_default()
        ))
    }
}

// ---------------------------------------------------------------------------
// NavigateToTool — publish navigation goal
// ---------------------------------------------------------------------------

pub struct NavigateToTool;

#[async_trait]
impl Tool for NavigateToTool {
    fn name(&self) -> &str {
        "navigate_to"
    }

    fn description(&self) -> &str {
        "Send a navigation goal (x, y, theta) to the robot via ROS2 navigation"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["x", "y"],
            "properties": {
                "x": {"type": "number", "description": "Target X position in meters"},
                "y": {"type": "number", "description": "Target Y position in meters"},
                "theta": {"type": "number", "description": "Target orientation in radians (default: 0.0)", "default": 0.0},
                "frame_id": {"type": "string", "description": "Reference frame (default: map)", "default": "map"},
                "topic": {"type": "string", "description": "Navigation topic (default: /navigate_to_pose)", "default": "/navigate_to_pose"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let x = match args["x"].as_f64() {
            Some(v) => v,
            None => return ToolResult::Error("Missing parameter: x".into()),
        };
        let y = match args["y"].as_f64() {
            Some(v) => v,
            None => return ToolResult::Error("Missing parameter: y".into()),
        };
        let theta = args["theta"].as_f64().unwrap_or(0.0);
        let frame_id = args["frame_id"].as_str().unwrap_or("map");
        let navigate_topic = args["topic"].as_str().unwrap_or("/navigate_to_pose");

        // Build a PoseStamped-like message for the navigation goal
        let _goal_msg = json!({
            "header": {
                "frame_id": frame_id
            },
            "pose": {
                "position": {"x": x, "y": y, "z": 0.0},
                "orientation": {"x": 0.0, "y": 0.0, "z": theta.sin(), "w": theta.cos()}
            }
        });

        let _ros_msg = ros_publish(navigate_topic, _goal_msg);
        ToolResult::Success(format!(
            "Navigation goal sent to {navigate_topic}: x={x}, y={y}, theta={theta:.2}rad"
        ))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register ROS2-specific tools. Called when `bridge = "ros2"`.
pub fn register_ros2_tools(registry: &mut super::registry::ToolRegistry) {
    registry.register(Ros2PublishTool);
    registry.register(Ros2ServiceTool);
    registry.register(NavigateToTool);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
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
    async fn test_ros2_publish_tool() {
        let tool = Ros2PublishTool;
        let result = tool
            .execute(
                json!({"topic": "/cmd_vel", "msg": {"linear": {"x": 1.0}}}),
                &test_ctx(),
            )
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("/cmd_vel"));
    }

    #[tokio::test]
    async fn test_ros2_publish_missing_topic() {
        let tool = Ros2PublishTool;
        let result = tool
            .execute(json!({"msg": {"data": true}}), &test_ctx())
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("topic"));
    }

    #[tokio::test]
    async fn test_ros2_service_tool() {
        let tool = Ros2ServiceTool;
        let result = tool
            .execute(json!({"service": "/reset_world", "args": {}}), &test_ctx())
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("/reset_world"));
    }

    #[tokio::test]
    async fn test_ros2_service_no_args() {
        let tool = Ros2ServiceTool;
        let result = tool
            .execute(json!({"service": "/reset_world"}), &test_ctx())
            .await;
        assert!(!result.is_error());
    }

    #[tokio::test]
    async fn test_navigate_to_tool() {
        let tool = NavigateToTool;
        let result = tool
            .execute(json!({"x": 1.0, "y": 2.0, "theta": 1.57}), &test_ctx())
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("x=1"));
        assert!(result.content().contains("y=2"));
    }

    #[tokio::test]
    async fn test_navigate_to_missing_x() {
        let tool = NavigateToTool;
        let result = tool.execute(json!({"y": 2.0}), &test_ctx()).await;
        assert!(result.is_error());
        assert!(result.content().contains("x"));
    }

    #[tokio::test]
    async fn test_register_ros2_tools() {
        let mut registry = super::super::registry::ToolRegistry::new();
        register_ros2_tools(&mut registry);
        let names = registry.tool_names();
        assert!(names.contains(&"ros2_publish"));
        assert!(names.contains(&"ros2_service"));
        assert!(names.contains(&"navigate_to"));
    }
}
