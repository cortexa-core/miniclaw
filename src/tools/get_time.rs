use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};

pub struct GetTimeTool;

#[async_trait]
impl Tool for GetTimeTool {
    fn name(&self) -> &str {
        "get_time"
    }

    fn description(&self) -> &str {
        "Get the current date, time, and timezone of the device."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "Optional timezone (e.g., 'America/New_York'). Defaults to device timezone."
                }
            }
        })
    }

    async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let now = chrono::Local::now();
        ToolResult::Success(format!(
            "Current time: {}\nTimezone: {}\nUnix timestamp: {}",
            now.format("%Y-%m-%d %H:%M:%S %Z"),
            now.format("%z"),
            now.timestamp()
        ))
    }
}
