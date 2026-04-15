//! ROS2 bridge via rosbridge WebSocket protocol.
//!
//! Protocol helpers (JSON message building/parsing) are always compiled.
//! The actual WebSocket bridge (`RosBridge`) is feature-gated behind `ros2`.

use serde_json::json;

use super::HardwareCommand;

// ---------------------------------------------------------------------------
// Protocol helpers (always compiled — useful for testing without ros2 feature)
// ---------------------------------------------------------------------------

/// Build a rosbridge "publish" message.
pub fn ros_publish(topic: &str, msg: serde_json::Value) -> serde_json::Value {
    json!({"op": "publish", "topic": topic, "msg": msg})
}

/// Build a rosbridge "subscribe" message.
pub fn ros_subscribe(topic: &str, msg_type: &str) -> serde_json::Value {
    json!({"op": "subscribe", "topic": topic, "type": msg_type})
}

/// Build a rosbridge "call_service" message.
pub fn ros_call_service(service: &str, args: serde_json::Value, id: &str) -> serde_json::Value {
    json!({"op": "call_service", "service": service, "args": args, "id": id})
}

/// Convert a `HardwareCommand` to a rosbridge publish message.
pub fn command_to_ros_msg(cmd: &HardwareCommand, topics: &RosTopics) -> Option<serde_json::Value> {
    match cmd {
        HardwareCommand::ServoSet {
            name,
            angle,
            speed_deg_s,
        } => Some(ros_publish(
            &topics.servo_cmd,
            json!({
                "name": name,
                "angle": angle,
                "speed": speed_deg_s,
            }),
        )),
        HardwareCommand::MotorSet {
            name: _,
            speed,
            duration_ms: _,
        } => topics.cmd_vel.as_ref().map(|topic| {
            ros_publish(
                topic,
                json!({
                    "linear": {"x": speed, "y": 0.0, "z": 0.0},
                    "angular": {"x": 0.0, "y": 0.0, "z": 0.0}
                }),
            )
        }),
        HardwareCommand::LedSet { name, r, g, b } => Some(ros_publish(
            &topics.led_cmd,
            json!({"name": name, "r": r, "g": g, "b": b}),
        )),
        HardwareCommand::EmergencyStop => Some(ros_publish(&topics.estop, json!({"data": true}))),
        HardwareCommand::Ping | HardwareCommand::LedPattern { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// ROS2 topic configuration
// ---------------------------------------------------------------------------

/// Topic names used by the ROS2 bridge.
#[derive(Debug, Clone)]
pub struct RosTopics {
    pub servo_cmd: String,
    pub led_cmd: String,
    pub estop: String,
    pub cmd_vel: Option<String>,
    pub odom: Option<String>,
    pub scan: Option<String>,
    pub camera: Option<String>,
    pub navigate_action: Option<String>,
}

impl RosTopics {
    /// Build topic configuration from a `Ros2Config` (loaded from robot.toml).
    pub fn from_config(ros2: &crate::robot::description::Ros2Config) -> Self {
        let ns = ros2.namespace.as_deref().unwrap_or("/uniclaw");
        Self {
            servo_cmd: format!("{ns}/servo_cmd"),
            led_cmd: format!("{ns}/led_cmd"),
            estop: format!("{ns}/estop"),
            cmd_vel: ros2.cmd_vel_topic.clone(),
            odom: ros2.odom_topic.clone(),
            scan: ros2.scan_topic.clone(),
            camera: ros2.camera_topic.clone(),
            navigate_action: ros2.navigate_action.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Feature-gated: actual WebSocket bridge
// ---------------------------------------------------------------------------

#[cfg(feature = "ros2")]
use super::{HardwareBridge, SensorValue};
#[cfg(feature = "ros2")]
use anyhow::{anyhow, Result};
#[cfg(feature = "ros2")]
use std::collections::HashMap;
#[cfg(feature = "ros2")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "ros2")]
pub struct RosBridge {
    ws_url: String,
    topics: RosTopics,
    sensors: std::sync::Mutex<HashMap<String, SensorValue>>,
    msg_id: AtomicU64,
}

#[cfg(feature = "ros2")]
impl RosBridge {
    /// Connect to a rosbridge WebSocket server and verify reachability.
    pub async fn connect(url: &str, topics: RosTopics) -> Result<Self> {
        // Verify connectivity
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| anyhow!("Failed to connect to rosbridge at {url}: {e}"))?;
        drop(ws);
        tracing::info!("ROS2 bridge connected to {url}");
        Ok(Self {
            ws_url: url.to_string(),
            topics,
            sensors: std::sync::Mutex::new(HashMap::new()),
            msg_id: AtomicU64::new(0),
        })
    }

    /// Get the configured topics.
    pub fn topics(&self) -> &RosTopics {
        &self.topics
    }

    fn next_id(&self) -> String {
        format!("uniclaw_{}", self.msg_id.fetch_add(1, Ordering::SeqCst))
    }

    async fn send_msg(&self, msg: serde_json::Value) -> Result<()> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let (mut ws, _) = tokio_tungstenite::connect_async(&self.ws_url)
            .await
            .map_err(|e| anyhow!("rosbridge connection failed: {e}"))?;
        ws.send(Message::Text(msg.to_string()))
            .await
            .map_err(|e| anyhow!("rosbridge send failed: {e}"))?;
        ws.close(None).await.ok();
        Ok(())
    }

    /// Send a raw rosbridge JSON message.
    pub async fn send_raw(&self, msg: serde_json::Value) -> Result<()> {
        self.send_msg(msg).await
    }

    /// Call a ROS2 service via rosbridge.
    pub async fn call_service(&self, service: &str, args: serde_json::Value) -> Result<()> {
        let id = self.next_id();
        let msg = ros_call_service(service, args, &id);
        self.send_msg(msg).await
    }
}

#[cfg(feature = "ros2")]
#[async_trait::async_trait]
impl HardwareBridge for RosBridge {
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()> {
        if let Some(msg) = command_to_ros_msg(&cmd, &self.topics) {
            self.send_msg(msg).await?;
        }
        Ok(())
    }

    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue> {
        self.sensors
            .lock()
            .unwrap()
            .get(sensor_id)
            .cloned()
            .ok_or_else(|| anyhow!("Sensor '{sensor_id}' not available via ROS2"))
    }

    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>> {
        Ok(self.sensors.lock().unwrap().clone())
    }

    async fn heartbeat(&self) -> Result<()> {
        Ok(()) // ROS2 doesn't need heartbeat
    }

    async fn emergency_stop(&self) -> Result<()> {
        self.send_command(HardwareCommand::EmergencyStop).await
    }

    fn name(&self) -> &str {
        "ros2"
    }
}

// ---------------------------------------------------------------------------
// Tests (not feature-gated — test protocol helpers only)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ros_publish_message_format() {
        let msg = ros_publish("/cmd_vel", json!({"linear": {"x": 1.0}}));
        assert_eq!(msg["op"], "publish");
        assert_eq!(msg["topic"], "/cmd_vel");
        assert_eq!(msg["msg"]["linear"]["x"], 1.0);
    }

    #[test]
    fn test_ros_subscribe_message_format() {
        let msg = ros_subscribe("/odom", "nav_msgs/Odometry");
        assert_eq!(msg["op"], "subscribe");
        assert_eq!(msg["topic"], "/odom");
        assert_eq!(msg["type"], "nav_msgs/Odometry");
    }

    #[test]
    fn test_ros_call_service_format() {
        let msg = ros_call_service("/reset", json!({}), "req_1");
        assert_eq!(msg["op"], "call_service");
        assert_eq!(msg["service"], "/reset");
        assert_eq!(msg["id"], "req_1");
    }

    #[test]
    fn test_command_to_ros_msg_servo() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: None,
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        let cmd = HardwareCommand::ServoSet {
            name: "arm".into(),
            angle: 90.0,
            speed_deg_s: Some(60.0),
        };
        let msg = command_to_ros_msg(&cmd, &topics).unwrap();
        assert_eq!(msg["op"], "publish");
        assert_eq!(msg["topic"], "/uniclaw/servo_cmd");
        assert_eq!(msg["msg"]["name"], "arm");
        assert_eq!(msg["msg"]["angle"], 90.0);
        assert_eq!(msg["msg"]["speed"], 60.0);
    }

    #[test]
    fn test_command_to_ros_msg_motor_no_cmd_vel() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: None,
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        let cmd = HardwareCommand::MotorSet {
            name: "left".into(),
            speed: 0.5,
            duration_ms: None,
        };
        assert!(command_to_ros_msg(&cmd, &topics).is_none());
    }

    #[test]
    fn test_command_to_ros_msg_motor_with_cmd_vel() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: Some("/cmd_vel".into()),
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        let cmd = HardwareCommand::MotorSet {
            name: "drive".into(),
            speed: 1.5,
            duration_ms: Some(1000),
        };
        let msg = command_to_ros_msg(&cmd, &topics).unwrap();
        assert_eq!(msg["topic"], "/cmd_vel");
        assert_eq!(msg["msg"]["linear"]["x"], 1.5);
    }

    #[test]
    fn test_command_to_ros_msg_estop() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: None,
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        let msg = command_to_ros_msg(&HardwareCommand::EmergencyStop, &topics).unwrap();
        assert_eq!(msg["topic"], "/uniclaw/estop");
        assert_eq!(msg["msg"]["data"], true);
    }

    #[test]
    fn test_command_to_ros_msg_ping_returns_none() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: None,
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        assert!(command_to_ros_msg(&HardwareCommand::Ping, &topics).is_none());
    }

    #[test]
    fn test_command_to_ros_msg_led() {
        let topics = RosTopics {
            servo_cmd: "/uniclaw/servo_cmd".into(),
            led_cmd: "/uniclaw/led_cmd".into(),
            estop: "/uniclaw/estop".into(),
            cmd_vel: None,
            odom: None,
            scan: None,
            camera: None,
            navigate_action: None,
        };
        let cmd = HardwareCommand::LedSet {
            name: "ring".into(),
            r: 255,
            g: 0,
            b: 128,
        };
        let msg = command_to_ros_msg(&cmd, &topics).unwrap();
        assert_eq!(msg["msg"]["r"], 255);
        assert_eq!(msg["msg"]["g"], 0);
        assert_eq!(msg["msg"]["b"], 128);
    }

    #[test]
    fn test_ros_topics_from_config() {
        let config = crate::robot::description::Ros2Config {
            namespace: Some("/mybot".into()),
            cmd_topic: Some("/cmd".into()),
            sensor_topic: Some("/sensors".into()),
            rosbridge_url: Some("ws://localhost:9090".into()),
            cmd_vel_topic: Some("/cmd_vel".into()),
            odom_topic: Some("/odom".into()),
            scan_topic: None,
            camera_topic: None,
            navigate_action: Some("/navigate_to_pose".into()),
        };
        let topics = RosTopics::from_config(&config);
        assert_eq!(topics.servo_cmd, "/mybot/servo_cmd");
        assert_eq!(topics.led_cmd, "/mybot/led_cmd");
        assert_eq!(topics.estop, "/mybot/estop");
        assert_eq!(topics.cmd_vel.as_deref(), Some("/cmd_vel"));
        assert_eq!(topics.odom.as_deref(), Some("/odom"));
        assert!(topics.scan.is_none());
        assert!(topics.camera.is_none());
        assert_eq!(topics.navigate_action.as_deref(), Some("/navigate_to_pose"));
    }

    #[test]
    fn test_ros_topics_default_namespace() {
        let config = crate::robot::description::Ros2Config {
            namespace: None,
            cmd_topic: None,
            sensor_topic: None,
            rosbridge_url: None,
            cmd_vel_topic: None,
            odom_topic: None,
            scan_topic: None,
            camera_topic: None,
            navigate_action: None,
        };
        let topics = RosTopics::from_config(&config);
        assert_eq!(topics.servo_cmd, "/uniclaw/servo_cmd");
        assert_eq!(topics.estop, "/uniclaw/estop");
    }
}
