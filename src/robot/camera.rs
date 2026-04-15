//! Camera capture abstraction for the perception pipeline.
//!
//! Feature-gated behind `camera`. The trait allows swapping between
//! a real V4L2 camera (Linux) and a mock for testing/macOS dev.

/// Camera capture trait — implementations must be Send + Sync.
pub trait CameraCapture: Send + Sync {
    /// Capture a single frame and return it as JPEG bytes.
    fn capture_jpeg(&mut self) -> anyhow::Result<Vec<u8>>;
}

/// Mock camera for testing — returns a minimal valid JPEG (1x1 white pixel).
pub struct MockCamera;

impl CameraCapture for MockCamera {
    fn capture_jpeg(&mut self) -> anyhow::Result<Vec<u8>> {
        // Minimal valid JPEG: SOI + APP0 (JFIF) + EOI
        Ok(vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
        ])
    }
}

/// Encode JPEG bytes as a base64 string for embedding in LLM messages.
pub fn jpeg_to_base64(jpeg: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_camera_returns_valid_jpeg_bytes() {
        let mut cam = MockCamera;
        let data = cam.capture_jpeg().unwrap();
        // JPEG starts with SOI marker (0xFF 0xD8) and ends with EOI (0xFF 0xD9)
        assert!(data.len() >= 4);
        assert_eq!(data[0], 0xFF);
        assert_eq!(data[1], 0xD8);
        assert_eq!(data[data.len() - 2], 0xFF);
        assert_eq!(data[data.len() - 1], 0xD9);
    }

    #[test]
    fn jpeg_to_base64_encodes_correctly() {
        let input = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let encoded = jpeg_to_base64(&input);
        // Decode back and verify roundtrip
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn jpeg_to_base64_empty_input() {
        let encoded = jpeg_to_base64(&[]);
        assert_eq!(encoded, "");
    }
}
