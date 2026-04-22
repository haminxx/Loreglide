//! Native text-to-speech wrapper.
//!
//! We use the `tts` crate, which on Windows routes through SAPI5 and on
//! macOS through `AVSpeechSynthesizer`/`NSSpeechSynthesizer`. Both are
//! offline, low-latency, and free of network dependencies.
//!
//! The synthesizer is *not* `Send` on every platform, so we keep it on a
//! dedicated worker thread and feed it through an `mpsc` channel. That
//! gives us a truly non-blocking `speak()` API that can be called from
//! any thread — including the Tokio runtime used by Tauri commands.

use log::{error, warn};
use std::sync::mpsc;
use std::thread;
use thiserror::Error;
use tts::{Features, Tts};

use crate::diffing::SpeakEvent;

#[derive(Debug, Error)]
pub enum TtsError {
    #[error("TTS engine could not be initialized: {0}")]
    Init(String),
    #[error("TTS worker thread has stopped")]
    WorkerGone,
}

#[derive(Debug, Clone)]
enum Command {
    Speak { text: String, interrupt: bool },
    SetRate(f32),
    SetVolume(f32),
    Stop,
}

/// Public handle. Cheap to clone — all clones route to the same worker.
#[derive(Clone)]
pub struct TtsHandle {
    tx: mpsc::Sender<Command>,
}

impl TtsHandle {
    /// Boot the TTS worker thread. Returns immediately.
    pub fn spawn() -> Result<Self, TtsError> {
        let (tx, rx) = mpsc::channel::<Command>();
        // We initialize the engine *inside* the thread to avoid crossing
        // the `Send` boundary on platforms where `Tts` is `!Send`.
        thread::Builder::new()
            .name("loreglide-tts".into())
            .spawn(move || worker_loop(rx))
            .map_err(|e| TtsError::Init(e.to_string()))?;
        Ok(Self { tx })
    }

    /// Speak a raw string. `interrupt=true` cancels any in-progress utterance.
    pub fn speak(&self, text: impl Into<String>, interrupt: bool) -> Result<(), TtsError> {
        self.tx
            .send(Command::Speak { text: text.into(), interrupt })
            .map_err(|_| TtsError::WorkerGone)
    }

    /// Convenience for the diff engine output.
    pub fn speak_event(&self, event: &SpeakEvent) -> Result<(), TtsError> {
        // Sentences interrupt any mid-flight per-word playback so the
        // reviewer gets a clean read-back. Words never interrupt each
        // other — that way rapid typing naturally queues.
        let interrupt = matches!(event, SpeakEvent::Sentence(_));
        self.speak(event.text().to_owned(), interrupt)
    }

    /// 0.5 (slow) … 2.0 (fast). Values outside are clamped by the engine.
    pub fn set_rate(&self, rate: f32) -> Result<(), TtsError> {
        self.tx
            .send(Command::SetRate(rate))
            .map_err(|_| TtsError::WorkerGone)
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), TtsError> {
        self.tx
            .send(Command::SetVolume(volume))
            .map_err(|_| TtsError::WorkerGone)
    }

    pub fn stop(&self) -> Result<(), TtsError> {
        self.tx.send(Command::Stop).map_err(|_| TtsError::WorkerGone)
    }
}

fn worker_loop(rx: mpsc::Receiver<Command>) {
    let mut engine = match Tts::default() {
        Ok(e) => e,
        Err(e) => {
            error!("TTS engine init failed: {e}");
            return;
        }
    };

    // Normalize the engine's native rate range to a `0.5..=2.0` scale.
    // Note: `tts` 0.26 returns plain `f32` (not `Result<f32>`).
    let min_rate = engine.min_rate();
    let max_rate = engine.max_rate();
    let normal_rate = engine.normal_rate();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Speak { text, interrupt } => {
                if text.trim().is_empty() {
                    continue;
                }
                if let Err(e) = engine.speak(text, interrupt) {
                    warn!("TTS speak error: {e}");
                }
            }
            Command::SetRate(scale) => {
                // `scale` is user-facing (0.5 = half, 2.0 = double).
                let clamped = scale.clamp(0.5, 2.0);
                // Map [0.5..=2.0] onto [min_rate..=max_rate] with 1.0 → normal_rate.
                let target = if clamped <= 1.0 {
                    let t = (clamped - 0.5) / 0.5; // 0..1
                    min_rate + (normal_rate - min_rate) * t
                } else {
                    let t = (clamped - 1.0) / 1.0; // 0..1
                    normal_rate + (max_rate - normal_rate) * t
                };
                if engine.supported_features().rate {
                    if let Err(e) = engine.set_rate(target) {
                        warn!("TTS set_rate error: {e}");
                    }
                }
            }
            Command::SetVolume(v) => {
                let v = v.clamp(0.0, 1.0);
                if engine.supported_features().volume {
                    if let Err(e) = engine.set_volume(v) {
                        warn!("TTS set_volume error: {e}");
                    }
                }
            }
            Command::Stop => {
                if engine.supported_features().stop {
                    let _ = engine.stop();
                }
            }
        }
    }
}

/// Convenience: report what the underlying engine actually supports.
pub fn report_features() -> Features {
    Tts::default()
        .map(|e| e.supported_features())
        .unwrap_or_default()
}
