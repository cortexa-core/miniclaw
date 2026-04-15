//! Voice pipeline: VAD (energy-based), cloud STT (Whisper), cloud TTS.
//!
//! Pure-logic module with no audio device dependencies. Audio I/O traits
//! (`AudioInput`, `AudioOutput`) are provided for future `cpal` integration.

use anyhow::Result;

// ---------------------------------------------------------------------------
// Audio I/O traits (implement with cpal or other backend later)
// ---------------------------------------------------------------------------

/// Provides raw audio frames (e.g. from a microphone).
#[allow(dead_code)]
pub trait AudioInput: Send + Sync {
    /// Read one 20 ms frame (320 i16 samples at 16 kHz mono).
    fn read_frame(&mut self) -> Result<Vec<i16>>;
}

/// Plays raw audio (e.g. to a speaker).
#[allow(dead_code)]
pub trait AudioOutput: Send + Sync {
    /// Write PCM samples (16-bit mono 16 kHz) to the output device.
    fn write_samples(&mut self, samples: &[i16]) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Voice Activity Detector — energy-based, pure Rust
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum VadState {
    Idle,
    Speaking,
}

/// Energy-based Voice Activity Detector.
///
/// Feed 20 ms frames (320 samples at 16 kHz) via [`Vad::process_frame`].
/// Returns `Some(true)` when speech starts, `Some(false)` when speech ends.
#[allow(dead_code)]
pub struct Vad {
    threshold_multiplier: f32,
    noise_floor: f32,
    speech_start_ms: u32,
    silence_end_ms: u32,
    state: VadState,
    speech_frames: u32,
    silence_frames: u32,
}

#[allow(dead_code)]
impl Vad {
    /// Create a new VAD with sensible defaults.
    pub fn new() -> Self {
        Self {
            threshold_multiplier: 3.0,
            noise_floor: 0.01,
            speech_start_ms: 300,
            silence_end_ms: 500,
            state: VadState::Idle,
            speech_frames: 0,
            silence_frames: 0,
        }
    }

    /// Process a 20 ms audio frame (320 samples at 16 kHz).
    ///
    /// Returns:
    /// - `Some(true)`  — speech just started
    /// - `Some(false)` — speech just ended
    /// - `None`        — no transition
    pub fn process_frame(&mut self, samples: &[i16]) -> Option<bool> {
        let rms = Self::rms_energy(samples);
        const FRAME_MS: u32 = 20;

        let is_speech = rms > self.noise_floor * self.threshold_multiplier;

        // Adaptive noise floor (slow EMA, only during confirmed silence)
        if self.state == VadState::Idle && !is_speech {
            self.noise_floor = self.noise_floor * 0.95 + rms * 0.05;
        }

        match self.state {
            VadState::Idle => {
                if is_speech {
                    self.speech_frames += 1;
                    if self.speech_frames * FRAME_MS >= self.speech_start_ms {
                        self.state = VadState::Speaking;
                        self.silence_frames = 0;
                        return Some(true);
                    }
                } else {
                    self.speech_frames = 0;
                }
            }
            VadState::Speaking => {
                if !is_speech {
                    self.silence_frames += 1;
                    if self.silence_frames * FRAME_MS >= self.silence_end_ms {
                        self.state = VadState::Idle;
                        self.speech_frames = 0;
                        return Some(false);
                    }
                } else {
                    self.silence_frames = 0;
                }
            }
        }
        None
    }

    /// Compute RMS energy of samples, normalized to [0.0, 1.0].
    fn rms_energy(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        ((sum / samples.len() as f64).sqrt() / 32768.0) as f32
    }

    /// Reset the detector to idle state.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.state = VadState::Idle;
        self.speech_frames = 0;
        self.silence_frames = 0;
    }
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Cloud STT — Whisper API
// ---------------------------------------------------------------------------

/// Send WAV audio to the Whisper API and return the transcribed text.
#[allow(dead_code)]
pub async fn transcribe_audio(
    audio_wav: &[u8],
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<String> {
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(audio_wav.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")?,
        )
        .text("model", model.to_string());

    let url = format!("{base_url}/v1/audio/transcriptions");
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("STT failed ({status}): {body}"));
    }

    let json: serde_json::Value = resp.json().await?;
    Ok(json["text"].as_str().unwrap_or("").to_string())
}

// ---------------------------------------------------------------------------
// Cloud TTS — OpenAI TTS API
// ---------------------------------------------------------------------------

/// Generate speech audio (WAV bytes) from text via cloud TTS.
#[allow(dead_code)]
pub async fn synthesize_speech(
    text: &str,
    api_key: &str,
    base_url: &str,
    voice: &str,
) -> Result<Vec<u8>> {
    let client = reqwest::Client::new();
    let url = format!("{base_url}/v1/audio/speech");
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": voice,
            "response_format": "wav",
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("TTS failed ({status}): {body}"));
    }

    Ok(resp.bytes().await?.to_vec())
}

// ---------------------------------------------------------------------------
// WAV encoding helper
// ---------------------------------------------------------------------------

/// Encode raw PCM samples (16-bit mono 16 kHz) as a valid WAV byte buffer.
#[allow(dead_code)]
pub fn encode_wav(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let file_len = 36 + data_len;
    let mut buf = Vec::with_capacity(44 + data_len);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(file_len as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&16000u32.to_le_bytes()); // sample rate
    buf.extend_from_slice(&32000u32.to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    buf
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_detects_speech() {
        let mut vad = Vad::new();
        // Feed silence to establish noise floor
        for _ in 0..50 {
            vad.process_frame(&[10i16; 320]);
        }
        // Feed loud signal — should detect speech start after ~300 ms (15 frames)
        let mut started = false;
        for _ in 0..20 {
            if let Some(true) = vad.process_frame(&[10000i16; 320]) {
                started = true;
                break;
            }
        }
        assert!(started, "VAD should detect speech start");
    }

    #[test]
    fn test_vad_detects_silence() {
        let mut vad = Vad::new();
        // Initialize noise floor
        for _ in 0..50 {
            vad.process_frame(&[10i16; 320]);
        }
        // Start speech
        for _ in 0..20 {
            vad.process_frame(&[10000i16; 320]);
        }
        // End speech — feed silence
        let mut ended = false;
        for _ in 0..30 {
            if let Some(false) = vad.process_frame(&[10i16; 320]) {
                ended = true;
                break;
            }
        }
        assert!(ended, "VAD should detect speech end");
    }

    #[test]
    fn test_rms_energy() {
        assert!(Vad::rms_energy(&[0i16; 320]) < 0.001);
        assert!(Vad::rms_energy(&[16384i16; 320]) > 0.4);
    }

    #[test]
    fn test_rms_energy_empty() {
        assert!((Vad::rms_energy(&[]) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_encode_wav() {
        let samples = vec![0i16; 16000]; // 1 second of silence
        let wav = encode_wav(&samples);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 32000); // header + 16000 samples * 2 bytes
    }

    #[test]
    fn test_encode_wav_empty() {
        let wav = encode_wav(&[]);
        assert_eq!(wav.len(), 44);
        assert_eq!(&wav[0..4], b"RIFF");
    }

    #[test]
    fn test_vad_default() {
        let vad = Vad::default();
        assert_eq!(vad.state, VadState::Idle);
    }
}
