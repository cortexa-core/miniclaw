use std::collections::HashMap;

use tokio::sync::{mpsc, watch};

use super::bridge::{HardwareCommand, SensorValue};
use super::description::SafetyRule;
use super::world_state::WorldState;

// ---------------------------------------------------------------------------
// Expression parser types
// ---------------------------------------------------------------------------

/// Comparison operators supported in safety rule conditions.
#[derive(Debug, Clone, PartialEq)]
pub enum CompareOp {
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
}

/// Action to take when a safety rule triggers.
#[derive(Debug, Clone, PartialEq)]
pub enum SafetyAction {
    StopAll,
    EmergencyStop,
    Speak(String),
}

/// A parsed safety rule ready for evaluation.
#[derive(Debug, Clone)]
pub struct ParsedRule {
    pub name: String,
    pub sensor_name: String,
    pub operator: CompareOp,
    pub threshold: f32,
    pub action: SafetyAction,
    pub priority: i32,
}

impl ParsedRule {
    /// Parse a declarative safety rule like `"front_distance < 10"`.
    pub fn parse(rule: &SafetyRule) -> anyhow::Result<Self> {
        let parts: Vec<&str> = rule.condition.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!(
                "Invalid safety rule '{}': expected '<sensor> <op> <threshold>'",
                rule.condition
            ));
        }
        let sensor_name = parts[0].to_string();
        let operator = match parts[1] {
            "<" => CompareOp::LessThan,
            ">" => CompareOp::GreaterThan,
            "<=" => CompareOp::LessEqual,
            ">=" => CompareOp::GreaterEqual,
            op => return Err(anyhow::anyhow!("Unknown operator: {op}")),
        };
        let threshold: f32 = parts[2]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid threshold: {}", parts[2]))?;

        let action = if let Some(msg) = rule.action.strip_prefix("speak:") {
            SafetyAction::Speak(msg.to_string())
        } else if rule.action == "emergency_stop" {
            SafetyAction::EmergencyStop
        } else {
            SafetyAction::StopAll
        };

        Ok(Self {
            name: rule.name.clone(),
            sensor_name,
            operator,
            threshold,
            action,
            priority: rule.priority.unwrap_or(0),
        })
    }

    /// Evaluate this rule against current sensor values. Returns `true` if triggered.
    pub fn evaluate(&self, sensors: &HashMap<String, SensorValue>) -> bool {
        let value = match sensors.get(&self.sensor_name) {
            Some(SensorValue::Distance(d)) => *d,
            Some(SensorValue::Temperature(t)) => *t,
            Some(SensorValue::Raw(r)) => *r as f32,
            Some(SensorValue::Boolean(b)) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            _ => return false,
        };
        match self.operator {
            CompareOp::LessThan => value < self.threshold,
            CompareOp::GreaterThan => value > self.threshold,
            CompareOp::LessEqual => value <= self.threshold,
            CompareOp::GreaterEqual => value >= self.threshold,
        }
    }
}

// ---------------------------------------------------------------------------
// Safety monitor
// ---------------------------------------------------------------------------

/// Background task that continuously evaluates safety rules against world state.
pub struct SafetyMonitor {
    rules: Vec<ParsedRule>,
    world_rx: watch::Receiver<WorldState>,
    action_tx: mpsc::Sender<HardwareCommand>,
}

impl SafetyMonitor {
    pub fn new(
        rules: Vec<ParsedRule>,
        world_rx: watch::Receiver<WorldState>,
        action_tx: mpsc::Sender<HardwareCommand>,
    ) -> Self {
        Self {
            rules,
            world_rx,
            action_tx,
        }
    }

    /// Number of rules loaded.
    pub fn rules_count(&self) -> usize {
        self.rules.len()
    }

    /// Run the safety monitor loop. Checks rules every 200ms.
    pub async fn run(&mut self) {
        let interval = std::time::Duration::from_millis(200);
        loop {
            let state = self.world_rx.borrow().clone();
            for rule in &self.rules {
                if rule.evaluate(&state.sensors) {
                    tracing::warn!("Safety rule '{}' triggered", rule.name);
                    let cmd = match &rule.action {
                        SafetyAction::StopAll | SafetyAction::EmergencyStop => {
                            HardwareCommand::EmergencyStop
                        }
                        SafetyAction::Speak(_) => continue, // TODO: wire TTS
                    };
                    self.action_tx.send(cmd).await.ok();
                }
            }
            tokio::time::sleep(interval).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rule(name: &str, condition: &str, action: &str, priority: Option<i32>) -> SafetyRule {
        SafetyRule {
            name: name.to_string(),
            condition: condition.to_string(),
            action: action.to_string(),
            priority,
        }
    }

    #[test]
    fn test_parse_rule_less_than() {
        let rule = make_rule("obstacle_stop", "front_distance < 10", "stop_all", Some(10));
        let parsed = ParsedRule::parse(&rule).unwrap();
        assert_eq!(parsed.name, "obstacle_stop");
        assert_eq!(parsed.sensor_name, "front_distance");
        assert_eq!(parsed.operator, CompareOp::LessThan);
        assert!((parsed.threshold - 10.0).abs() < f32::EPSILON);
        assert_eq!(parsed.action, SafetyAction::StopAll);
        assert_eq!(parsed.priority, 10);
    }

    #[test]
    fn test_parse_rule_greater_equal() {
        let rule = make_rule("overheat", "temperature >= 80", "emergency_stop", None);
        let parsed = ParsedRule::parse(&rule).unwrap();
        assert_eq!(parsed.sensor_name, "temperature");
        assert_eq!(parsed.operator, CompareOp::GreaterEqual);
        assert!((parsed.threshold - 80.0).abs() < f32::EPSILON);
        assert_eq!(parsed.action, SafetyAction::EmergencyStop);
        assert_eq!(parsed.priority, 0); // default
    }

    #[test]
    fn test_parse_rule_speak_action() {
        let rule = make_rule("warn", "front_distance < 20", "speak:Too close!", None);
        let parsed = ParsedRule::parse(&rule).unwrap();
        assert_eq!(parsed.action, SafetyAction::Speak("Too close!".to_string()));
    }

    #[test]
    fn test_evaluate_rule_triggered() {
        let rule = make_rule("obstacle", "front_distance < 10", "stop_all", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let mut sensors = HashMap::new();
        sensors.insert("front_distance".to_string(), SensorValue::Distance(5.0));
        assert!(parsed.evaluate(&sensors));
    }

    #[test]
    fn test_evaluate_rule_not_triggered() {
        let rule = make_rule("obstacle", "front_distance < 10", "stop_all", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let mut sensors = HashMap::new();
        sensors.insert("front_distance".to_string(), SensorValue::Distance(50.0));
        assert!(!parsed.evaluate(&sensors));
    }

    #[test]
    fn test_evaluate_rule_sensor_missing() {
        let rule = make_rule("obstacle", "front_distance < 10", "stop_all", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let sensors = HashMap::new();
        assert!(!parsed.evaluate(&sensors));
    }

    #[test]
    fn test_evaluate_rule_temperature() {
        let rule = make_rule("overheat", "temperature > 60", "emergency_stop", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let mut sensors = HashMap::new();
        sensors.insert("temperature".to_string(), SensorValue::Temperature(75.0));
        assert!(parsed.evaluate(&sensors));

        sensors.insert("temperature".to_string(), SensorValue::Temperature(50.0));
        assert!(!parsed.evaluate(&sensors));
    }

    #[test]
    fn test_evaluate_rule_raw_sensor() {
        let rule = make_rule("low_battery", "battery_raw <= 20", "stop_all", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let mut sensors = HashMap::new();
        sensors.insert("battery_raw".to_string(), SensorValue::Raw(15));
        assert!(parsed.evaluate(&sensors));

        sensors.insert("battery_raw".to_string(), SensorValue::Raw(50));
        assert!(!parsed.evaluate(&sensors));
    }

    #[test]
    fn test_invalid_rule_format() {
        let rule = make_rule("bad", "toofew", "stop_all", None);
        assert!(ParsedRule::parse(&rule).is_err());

        let rule = make_rule("bad", "a b c d", "stop_all", None);
        assert!(ParsedRule::parse(&rule).is_err());
    }

    #[test]
    fn test_invalid_operator() {
        let rule = make_rule("bad", "sensor == 10", "stop_all", None);
        assert!(ParsedRule::parse(&rule).is_err());
    }

    #[test]
    fn test_invalid_threshold() {
        let rule = make_rule("bad", "sensor < abc", "stop_all", None);
        assert!(ParsedRule::parse(&rule).is_err());
    }

    #[tokio::test]
    async fn test_safety_monitor_sends_stop() {
        let rule = make_rule("obstacle", "front_distance < 10", "stop_all", None);
        let parsed = ParsedRule::parse(&rule).unwrap();

        let (action_tx, mut action_rx) = mpsc::channel(32);
        let (world_tx, world_rx) = watch::channel(WorldState::default());

        // Set sensor to trigger the rule
        world_tx.send_modify(|state| {
            state
                .sensors
                .insert("front_distance".to_string(), SensorValue::Distance(5.0));
        });

        let mut monitor = SafetyMonitor::new(vec![parsed], world_rx, action_tx);

        // Run monitor in background, check for command
        let handle = tokio::spawn(async move {
            monitor.run().await;
        });

        let cmd = tokio::time::timeout(std::time::Duration::from_secs(2), action_rx.recv())
            .await
            .expect("timed out waiting for safety command")
            .expect("channel closed");

        assert!(matches!(cmd, HardwareCommand::EmergencyStop));
        handle.abort();
    }
}
