use std::sync::Arc;
use tokio::sync::watch;

use super::camera::{jpeg_to_base64, CameraCapture};
use super::world_state::WorldState;
use crate::llm::types::{Context, Message};
use crate::llm::LlmProvider;

/// How the perception pipeline decides when to call the cloud VLM.
pub enum TriggerMode {
    /// Call VLM every `periodic_secs` regardless of motion.
    Periodic,
    /// Call VLM only when motion is detected.
    Event,
    /// Never call VLM automatically; only via explicit `capture_and_describe`.
    OnDemand,
}

/// Continuous perception pipeline: captures camera frames, detects motion,
/// and optionally calls a cloud Vision-Language Model to describe scenes.
pub struct PerceptionPipeline {
    camera: Box<dyn CameraCapture>,
    llm: Arc<dyn LlmProvider>,
    world_tx: watch::Sender<WorldState>,
    vision_model: String,
    trigger_mode: TriggerMode,
    periodic_secs: u64,
    last_frame: Option<Vec<u8>>,
}

impl PerceptionPipeline {
    pub fn new(
        camera: Box<dyn CameraCapture>,
        llm: Arc<dyn LlmProvider>,
        world_tx: watch::Sender<WorldState>,
        vision_model: String,
        trigger: &str,
        periodic_secs: u64,
    ) -> Self {
        let trigger_mode = match trigger {
            "event" => TriggerMode::Event,
            "on_demand" | "ondemand" => TriggerMode::OnDemand,
            _ => TriggerMode::Periodic,
        };
        Self {
            camera,
            llm,
            world_tx,
            vision_model,
            trigger_mode,
            periodic_secs,
            last_frame: None,
        }
    }

    /// Run the perception loop as a long-lived async task.
    pub async fn run(&mut self) {
        let interval = std::time::Duration::from_secs(self.periodic_secs);
        tracing::info!(
            "Perception pipeline started (model={}, interval={}s)",
            self.vision_model,
            self.periodic_secs
        );

        loop {
            // Capture a frame
            let frame = match self.camera.capture_jpeg() {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Camera capture failed: {e}");
                    tokio::time::sleep(interval).await;
                    continue;
                }
            };

            // Simple motion detection (compare frame sizes as crude proxy)
            let motion = self.detect_motion(&frame);
            self.world_tx
                .send_modify(|state| state.motion_detected = motion);

            // Decide whether to invoke the cloud VLM
            let should_describe = match self.trigger_mode {
                TriggerMode::Periodic => true,
                TriggerMode::Event => motion,
                TriggerMode::OnDemand => false,
            };

            if should_describe {
                match self.describe_scene(&frame).await {
                    Ok(description) => {
                        self.world_tx.send_modify(|state| {
                            state.scene_description = Some(description);
                            state.scene_timestamp = Some(std::time::Instant::now());
                        });
                    }
                    Err(e) => tracing::warn!("VLM scene description failed: {e}"),
                }
            }

            self.last_frame = Some(frame);
            tokio::time::sleep(interval).await;
        }
    }

    /// Crude motion detection: compares JPEG byte-size of current vs previous frame.
    /// A real implementation would decode pixels and compute frame difference.
    fn detect_motion(&self, current: &[u8]) -> bool {
        match &self.last_frame {
            Some(prev) => {
                let diff = (current.len() as i64 - prev.len() as i64).unsigned_abs();
                // >10% size change ≈ motion
                diff > (prev.len() as u64 / 10)
            }
            None => false,
        }
    }

    /// Send a JPEG frame to the cloud VLM and return a scene description.
    async fn describe_scene(&self, jpeg: &[u8]) -> anyhow::Result<String> {
        let b64 = jpeg_to_base64(jpeg);
        let message = Message::user_with_image(
            "Briefly describe what you see in this image. Focus on people, objects, and activities.",
            b64,
            "image/jpeg",
        );
        let context = Context {
            system: "You are a robot's vision system. Describe scenes concisely.".into(),
            messages: vec![message],
            tool_schemas: vec![],
        };
        let response = self.llm.chat(&context).await?;
        Ok(response
            .text
            .unwrap_or_else(|| "No description available.".to_string()))
    }

    /// One-shot: capture a new frame, describe it via VLM, and update world state.
    /// Called by the `take_photo` tool.
    pub async fn capture_and_describe(&mut self) -> anyhow::Result<String> {
        let frame = self.camera.capture_jpeg()?;
        let description = self.describe_scene(&frame).await?;
        self.world_tx.send_modify(|state| {
            state.scene_description = Some(description.clone());
            state.scene_timestamp = Some(std::time::Instant::now());
        });
        Ok(description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{ChatResponse, StopReason, Usage};
    use crate::robot::camera::MockCamera;

    /// Fake LLM provider that returns a fixed string.
    struct FakeLlm;

    #[async_trait::async_trait]
    impl LlmProvider for FakeLlm {
        async fn chat(&self, _ctx: &Context) -> anyhow::Result<ChatResponse> {
            Ok(ChatResponse {
                text: Some("A white room with no objects.".to_string()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    #[test]
    fn detect_motion_no_previous_frame() {
        let (world_tx, _world_rx) = watch::channel(WorldState::default());
        let pipeline = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx,
            "test-model".into(),
            "periodic",
            30,
        );
        // No previous frame → no motion
        assert!(!pipeline.detect_motion(&[0xFF; 100]));
    }

    #[test]
    fn detect_motion_similar_frames() {
        let (world_tx, _world_rx) = watch::channel(WorldState::default());
        let mut pipeline = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx,
            "test-model".into(),
            "periodic",
            30,
        );
        pipeline.last_frame = Some(vec![0xFF; 1000]);
        // Same size → no motion
        assert!(!pipeline.detect_motion(&[0xFF; 1000]));
        // Small change → no motion
        assert!(!pipeline.detect_motion(&[0xFF; 1050]));
        // Large change (>10%) → motion
        assert!(pipeline.detect_motion(&[0xFF; 1200]));
    }

    #[tokio::test]
    async fn capture_and_describe_updates_world_state() {
        let (world_tx, world_rx) = watch::channel(WorldState::default());
        let mut pipeline = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx,
            "test-model".into(),
            "periodic",
            30,
        );
        let desc = pipeline.capture_and_describe().await.unwrap();
        assert_eq!(desc, "A white room with no objects.");

        let state = world_rx.borrow().clone();
        assert_eq!(state.scene_description.as_deref(), Some(desc.as_str()));
        assert!(state.scene_timestamp.is_some());
    }

    #[test]
    fn trigger_mode_parsing() {
        let (world_tx, _) = watch::channel(WorldState::default());
        let p = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx.clone(),
            "m".into(),
            "event",
            1,
        );
        assert!(matches!(p.trigger_mode, TriggerMode::Event));

        let p = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx.clone(),
            "m".into(),
            "on_demand",
            1,
        );
        assert!(matches!(p.trigger_mode, TriggerMode::OnDemand));

        let p = PerceptionPipeline::new(
            Box::new(MockCamera),
            Arc::new(FakeLlm),
            world_tx,
            "m".into(),
            "periodic",
            1,
        );
        assert!(matches!(p.trigger_mode, TriggerMode::Periodic));
    }
}
