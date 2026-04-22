//! Loreglide — library entrypoint.
//!
//! The Tauri app is split across two crates:
//!   * `loreglide_lib` (this file) — all the real logic, so tests and
//!     other tools can pull it in without dragging the Tauri runtime.
//!   * `loreglide` (see `main.rs`) — the thin binary that just calls
//!     `loreglide_lib::run()`.

pub mod diffing;
pub mod tts;
pub mod watcher;

#[cfg(windows)]
pub mod watcher_windows;
#[cfg(target_os = "macos")]
pub mod watcher_macos;

use std::sync::Arc;

use log::{info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::diffing::{evaluate, SpeakEvent, TypingState};
use crate::tts::TtsHandle;
use crate::watcher::WatcherHandle;

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub tts: TtsHandle,
    pub watcher: Option<WatcherHandle>,
    /// Diff state used by the *in-app* editor mode (always available,
    /// even on platforms or configurations where the OS-wide watcher
    /// isn't usable).
    pub editor_state: Arc<Mutex<TypingState>>,
    pub settings: Arc<Mutex<Settings>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub global_watch_enabled: bool,
    pub editor_echo_enabled: bool,
    pub speak_words: bool,
    pub speak_sentences: bool,
    pub rate: f32,
    pub volume: f32,
    pub poll_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            global_watch_enabled: false,
            editor_echo_enabled: true,
            speak_words: true,
            speak_sentences: true,
            rate: 1.0,
            volume: 1.0,
            poll_ms: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands exposed to the React frontend
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
fn update_settings(new: Settings, state: State<'_, AppState>) -> Settings {
    let _ = state.tts.set_rate(new.rate);
    let _ = state.tts.set_volume(new.volume);
    if let Some(w) = &state.watcher {
        w.set_enabled(new.global_watch_enabled);
        w.set_interval_ms(new.poll_ms);
    }
    *state.settings.lock() = new.clone();
    new
}

/// Called from the React editor on every keystroke. Returns the event
/// that should be spoken (if any). Speaking is also done here so the
/// user doesn't need a second round-trip.
#[tauri::command]
fn editor_tick(
    text: String,
    state: State<'_, AppState>,
) -> Option<SpeakEvent> {
    let settings = state.settings.lock().clone();
    if !settings.editor_echo_enabled {
        return None;
    }
    let mut diff = state.editor_state.lock();
    let event = evaluate(&text, &mut diff);
    if let Some(ev) = &event {
        let allowed = match ev {
            SpeakEvent::Word(_) => settings.speak_words,
            SpeakEvent::Sentence(_) => settings.speak_sentences,
        };
        if allowed {
            let _ = state.tts.speak_event(ev);
        }
    }
    event
}

/// Reset the editor's diff memory — for example when the user clicks
/// "clear" in the UI.
#[tauri::command]
fn editor_reset(state: State<'_, AppState>) {
    state.editor_state.lock().reset();
}

/// Speak an arbitrary string immediately (used by the "review" button
/// that reads the whole paragraph).
#[tauri::command]
fn speak_text(text: String, state: State<'_, AppState>) -> Result<(), String> {
    state.tts.speak(text, true).map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_speaking(state: State<'_, AppState>) -> Result<(), String> {
    state.tts.stop().map_err(|e| e.to_string())
}

#[tauri::command]
fn watcher_available(state: State<'_, AppState>) -> bool {
    state.watcher.is_some()
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

/// Spawn the correct platform watcher. The probe is constructed on the
/// watcher thread itself (COM / AX handles are per-thread), so here we
/// just return a handle. If the OS APIs ultimately turn out to be
/// unavailable the watcher thread logs and exits — the app keeps
/// running in editor-only mode.
fn spawn_platform_watcher(tts: &TtsHandle) -> Option<WatcherHandle> {
    #[cfg(windows)]
    {
        return Some(watcher::spawn(
            watcher_windows::try_new,
            tts.clone(),
        ));
    }
    #[cfg(target_os = "macos")]
    {
        return Some(watcher::spawn(watcher_macos::try_new, tts.clone()));
    }
    #[allow(unreachable_code)]
    {
        let _ = tts;
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,loreglide=debug"),
    )
    .init();

    info!("Loreglide booting…");

    let tts = TtsHandle::spawn().expect("TTS engine failed to start");
    let watcher = spawn_platform_watcher(&tts);
    if watcher.is_none() {
        warn!(
            "OS-wide watcher unavailable; running in in-app editor mode only. \
             On macOS, grant Accessibility permission and relaunch."
        );
    }

    let settings = Arc::new(Mutex::new(Settings::default()));
    // Apply initial TTS settings.
    {
        let s = settings.lock().clone();
        let _ = tts.set_rate(s.rate);
        let _ = tts.set_volume(s.volume);
        if let Some(w) = &watcher {
            w.set_interval_ms(s.poll_ms);
            w.set_enabled(s.global_watch_enabled);
        }
    }

    let app_state = AppState {
        tts,
        watcher,
        editor_state: Arc::new(Mutex::new(TypingState::new())),
        settings,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            editor_tick,
            editor_reset,
            speak_text,
            stop_speaking,
            watcher_available,
        ])
        .setup(|app| {
            // Emit a one-time "ready" event so the UI can query initial state.
            let _ = app.emit("loreglide:ready", ());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Loreglide");
}
