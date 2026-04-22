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

use crate::diffing::{evaluate, SpeakEvent, TypingState};
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
///
/// The probe does NOT need to be `Send` — we always construct it
/// *inside* the watcher thread via a factory closure. On Windows this
/// matters because the UIAutomation COM interfaces hold `NonNull<c_void>`
/// and are per-thread; on macOS the AX APIs are similarly tied to the
/// thread that owns the accessibility client.
pub trait FocusProbe: 'static {
    fn snapshot(&mut self) -> Option<FocusSnapshot>;
}

#[derive(Clone)]
pub struct WatcherHandle {
    enabled: Arc<AtomicBool>,
    interval_ms: Arc<Mutex<u64>>,
}

/// A no-op probe for platforms we don't have accessibility integrations for.
/// `make_probe_factory` returns `|| None` on such platforms so the watcher
/// thread exits immediately and the app effectively runs in editor-only mode.
pub struct NullProbe;
impl FocusProbe for NullProbe {
    fn snapshot(&mut self) -> Option<FocusSnapshot> {
        None
    }
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
/// * `factory` — constructs the OS-specific probe *inside* the watcher
///   thread (COM / AX handles aren't `Send`). If the factory returns
///   `None` the watcher thread exits and the UI falls back to editor mode.
/// * `tts` — handle the watcher uses to speak completed words / sentences.
/// * `on_event` — side-channel callback fired after every successful
///   speak. lib.rs uses this to `AppHandle::emit` a frontend notification
///   so the settings window can show a live "last heard" indicator.
pub fn spawn<F, P, E>(factory: F, tts: TtsHandle, on_event: E) -> WatcherHandle
where
    F: FnOnce() -> Option<P> + Send + 'static,
    P: FocusProbe,
    E: Fn(&SpeakEvent) + Send + 'static,
{
    let enabled = Arc::new(AtomicBool::new(false));
    let interval = Arc::new(Mutex::new(100u64));

    let enabled_thread = enabled.clone();
    let interval_thread = interval.clone();

    thread::Builder::new()
        .name("loreglide-watcher".into())
        .spawn(move || {
            let Some(mut probe) = factory() else {
                warn!("probe factory returned None; watcher thread exiting");
                return;
            };
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
                    on_event(&event);
                }
            }
        })
        .expect("failed to spawn watcher thread");

    WatcherHandle {
        enabled,
        interval_ms: interval,
    }
}
