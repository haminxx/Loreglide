//! Loreglide — library entrypoint.
//!
//! The Tauri app is split across two crates:
//!   * `loreglide_lib` (this file) — all the real logic, so tests and
//!     other tools can pull it in without dragging the Tauri runtime.
//!   * `loreglide` (see `main.rs`) — the thin binary that just calls
//!     `loreglide_lib::run()`.
//!
//! Runtime shape
//! -------------
//! Loreglide is a **background service** with a settings-panel window,
//! not a standalone editor. On boot we:
//!
//!   1. Start a native TTS worker (SAPI/WinRT on Windows, AVSpeech on mac).
//!   2. Spawn an OS accessibility watcher that polls the focused text
//!      field in *any* application, diffs the text, and speaks completed
//!      words and sentences.
//!   3. Install a system tray icon so closing the window just hides it —
//!      the service keeps running in the background until the user picks
//!      "Quit" from the tray menu.
//!   4. Expose a minimal Tauri command surface to the React settings UI.

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
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use crate::diffing::{evaluate, SpeakEvent, TypingState};
use crate::tts::TtsHandle;
use crate::watcher::WatcherHandle;

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub tts: TtsHandle,
    pub watcher: Mutex<Option<WatcherHandle>>,
    /// Diff state for the tiny in-app "test here" box (editor echo).
    /// Kept because it's useful for verifying the pipeline without
    /// focusing another app, but it is no longer the primary workflow.
    pub editor_state: Arc<Mutex<TypingState>>,
    pub settings: Arc<Mutex<Settings>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Master switch for the OS-wide watcher. When `false`, Loreglide is
    /// silent regardless of what you type in other applications.
    pub global_watch_enabled: bool,
    /// Separate toggle for the tiny in-window test field.
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
            // Start OFF — nothing speaks until the user opts in. This is
            // a privacy-first default for a tool that can see text from
            // any window.
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
    if let Some(w) = state.watcher.lock().as_ref() {
        w.set_enabled(new.global_watch_enabled);
        w.set_interval_ms(new.poll_ms);
    }
    *state.settings.lock() = new.clone();
    new
}

/// Called from the tiny in-window test box on every keystroke.
#[tauri::command]
fn editor_tick(text: String, state: State<'_, AppState>) -> Option<SpeakEvent> {
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

#[tauri::command]
fn editor_reset(state: State<'_, AppState>) {
    state.editor_state.lock().reset();
}

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
    state.watcher.lock().is_some()
}

/// Show the main window (tray icon / menu item calls this).
#[tauri::command]
fn show_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.unminimize();
    }
}

/// Hide the main window (stays running in the tray).
#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Quit the whole app (tray "Quit" menu).
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Tray icon
// ---------------------------------------------------------------------------

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray:show", "Show Loreglide", true, None::<&str>)?;
    let toggle = MenuItem::with_id(
        app,
        "tray:toggle",
        "Pause / resume watching",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "tray:quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &quit])?;

    let _tray = TrayIconBuilder::with_id("loreglide-tray")
        .tooltip("Loreglide — typing echo")
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("window icon is bundled"),
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray:show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.unminimize();
                }
            }
            "tray:toggle" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let mut s = state.settings.lock();
                    s.global_watch_enabled = !s.global_watch_enabled;
                    if let Some(w) = state.watcher.lock().as_ref() {
                        w.set_enabled(s.global_watch_enabled);
                    }
                    let _ = app.emit("loreglide:settings", s.clone());
                }
            }
            "tray:quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click toggles the window's visibility — the most
            // common expectation for a tray-resident background tool.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                        let _ = w.unminimize();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Platform watcher factories
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn make_probe_factory() -> impl FnOnce() -> Option<watcher_windows::WindowsProbe> + Send + 'static {
    watcher_windows::try_new
}

#[cfg(target_os = "macos")]
fn make_probe_factory() -> impl FnOnce() -> Option<watcher_macos::MacosProbe> + Send + 'static {
    watcher_macos::try_new
}

#[cfg(not(any(windows, target_os = "macos")))]
fn make_probe_factory() -> impl FnOnce() -> Option<watcher::NullProbe> + Send + 'static {
    || None
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,loreglide=debug"),
    )
    .init();

    info!("Loreglide booting…");

    let tts = TtsHandle::spawn().expect("TTS engine failed to start");
    let mut initial = Settings::default();
    // Dev/test helper: set `LOREGLIDE_AUTO_WATCH=1` to boot with the
    // OS watcher already enabled. In normal use the user opts in via
    // the UI; this env var lets us script end-to-end tests cleanly.
    if std::env::var("LOREGLIDE_AUTO_WATCH").as_deref() == Ok("1") {
        info!("LOREGLIDE_AUTO_WATCH=1 → global watch will start enabled");
        initial.global_watch_enabled = true;
    }
    let settings = Arc::new(Mutex::new(initial));

    // Apply initial TTS settings.
    {
        let s = settings.lock().clone();
        let _ = tts.set_rate(s.rate);
        let _ = tts.set_volume(s.volume);
    }

    let app_state = AppState {
        tts: tts.clone(),
        watcher: Mutex::new(None),
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
            show_window,
            hide_window,
            quit_app,
        ])
        .on_window_event(|window, event| {
            // Close-to-tray: when the user clicks the X, hide the window
            // instead of quitting the process. The tray menu's "Quit"
            // remains the only way to actually exit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            // Install tray icon now that we have an AppHandle.
            if let Err(e) = install_tray(&handle) {
                warn!("failed to install tray icon: {e}");
            }

            // Spawn the OS-wide watcher. We do it *inside* setup so the
            // emit-to-frontend callback can capture `handle`. The probe
            // itself is constructed on the watcher thread.
            let emit_handle = handle.clone();
            let watcher_handle = watcher::spawn(
                make_probe_factory(),
                tts.clone(),
                move |ev: &SpeakEvent| {
                    let _ = emit_handle.emit("loreglide:spoke", ev.clone());
                },
            );

            // Seed the watcher with the initial settings.
            {
                let state = handle.state::<AppState>();
                let s = state.settings.lock().clone();
                watcher_handle.set_interval_ms(s.poll_ms);
                watcher_handle.set_enabled(s.global_watch_enabled);
                *state.watcher.lock() = Some(watcher_handle);
            }

            let _ = app.emit("loreglide:ready", ());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Loreglide");
}
