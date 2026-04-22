//! Cross-platform focused-text-field watcher.
//!
//! The watcher runs on its own thread (or async task) and periodically
//! asks the OS for (1) which window/control currently holds keyboard
//! focus, and (2) the plain-text value of that control. Whenever that
//! value changes it feeds the new string through the diffing engine and
//! forwards any resulting `SpeakEvent` to the TTS handle.

use log::{debug, info, warn};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::diffing::{evaluate, TypingState};
use crate::tts::TtsHandle;

/// Opaque identifier for a focused control. Used to detect "focus moved
/// to a different field" so we can reset diff state without voicing the
/// pre-existing contents of the new field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FocusId(pub String);

/// A single snapshot of whatever text field currently owns focus.
pub struct FocusSnapshot {
    pub id: FocusId,
    pub text: String,
}

/// Platform-specific probe implementation. Each platform builds its own
/// `FocusProbe` that knows how to take one synchronous snapshot.
pub trait FocusProbe: Send + 'static {
    fn snapshot(&mut self) -> Option<FocusSnapshot>;
}

#[derive(Clone)]
pub struct WatcherHandle {
    enabled: Arc<AtomicBool>,
    interval_ms: Arc<Mutex<u64>>,
}

impl WatcherHandle {
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_interval_ms(&self, ms: u64) {
        *self.interval_ms.lock() = ms.clamp(20, 2000);
    }

    pub fn interval_ms(&self) -> u64 {
        *self.interval_ms.lock()
    }
}

/// Spawn the watcher loop on a dedicated thread.
///
/// * `probe` is the OS-specific snapshotter.
/// * `tts` receives any word/sentence events emitted by the diffing engine.
/// * The returned `WatcherHandle` lets the UI toggle it on/off at runtime
///   and adjust polling cadence.
pub fn spawn<P: FocusProbe>(mut probe: P, tts: TtsHandle) -> WatcherHandle {
    let enabled = Arc::new(AtomicBool::new(false));
    let interval = Arc::new(Mutex::new(100u64));

    let enabled_thread = enabled.clone();
    let interval_thread = interval.clone();

    thread::Builder::new()
        .name("loreglide-watcher".into())
        .spawn(move || {
            let mut state = TypingState::new();
            let mut current_focus = FocusId::default();
            loop {
                let sleep_ms = *interval_thread.lock();
                thread::sleep(Duration::from_millis(sleep_ms));

                if !enabled_thread.load(Ordering::SeqCst) {
                    // Make sure we don't accidentally re-voice the
                    // buffer content when the user flips it back on.
                    state.reset();
                    current_focus = FocusId::default();
                    continue;
                }

                let Some(snap) = probe.snapshot() else {
                    continue;
                };

                if snap.id != current_focus {
                    debug!("focus moved → {:?}", snap.id);
                    current_focus = snap.id.clone();
                    state.reset();
                    // Seed with the new field's current value so we
                    // don't read back everything that was already there.
                    state.last_text = snap.text.clone();
                    continue;
                }

                if let Some(event) = evaluate(&snap.text, &mut state) {
                    info!("speak → {:?}", event);
                    if let Err(e) = tts.speak_event(&event) {
                        warn!("tts send failed: {e}");
                    }
                }
            }
        })
        .expect("failed to spawn watcher thread");

    WatcherHandle {
        enabled,
        interval_ms: interval,
    }
}
