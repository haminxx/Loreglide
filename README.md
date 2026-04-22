# Loreglide

A **background typing-echo service**. Loreglide silently watches whichever text field is focused — in Cursor, Google Docs, Gmail, VS Code, Word, the browser, Slack, anywhere — and speaks every finished word the moment you press space, plus each finished sentence when you hit `.`, `!`, or `?`.

You don't type *into* Loreglide. You type wherever you normally type. Loreglide lives in the system tray; the window is only a control panel for toggling it on / off and tweaking the voice.

Built for writers, copywriters, directors, novelists — anyone who needs to *hear* their prose to catch what the eye misses.

- **Frontend**: React + TypeScript + Vite, rendered in a Tauri 2 window (settings panel only).
- **Backend**: Rust (single process, `~15 MB` binary, tray-resident).
- **TTS**: Native SAPI5 / WinRT (Windows) / AVSpeechSynthesizer (macOS). Offline, zero-latency, no API keys.
- **Accessibility**: UI Automation on Windows, AX API on macOS. Password fields are never read.

---

## Quick start

### Prerequisites

- **Node.js ≥ 18** and **npm** (or pnpm / yarn).
- **Rust toolchain** (`rustup` + `cargo`). Install from <https://rustup.rs>.
- **Tauri 2 system deps**:
  - **Windows**: Microsoft Edge WebView2 (usually pre-installed on Win10+). Install [Build Tools for Visual Studio](https://visualstudio.microsoft.com/downloads/) with the "Desktop development with C++" workload.
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`).
  - See the official checklist: <https://tauri.app/start/prerequisites/>.

### Install

```bash
npm install
```

### Run in dev mode

```bash
npm run tauri:dev
```

The first build is slow (pulling the full Rust crate graph, including `uiautomation`, `tts`, and the `windows` crate). Subsequent builds are incremental and fast.

### Build a production installer

```bash
npm run tauri:build
```

Output lands in `src-tauri/target/release/bundle/`:
- Windows: `.msi` and `.exe` installers
- macOS: `.dmg` and `.app`

### Run the Rust unit tests

```bash
cd src-tauri
cargo test
```

The diffing engine ships with a test suite that covers word completion, sentence completion, backspace, mid-string pastes, de-duplication, and Unicode.

---

## How it works

```
┌─────────────────────┐
│  OS Accessibility   │  UI Automation (Win) / AX (mac)
│  ─ focused element  │
│  ─ current value    │
└──────────┬──────────┘
           │ every 100 ms
           ▼
┌─────────────────────┐
│   Watcher thread    │  ← WatcherHandle (enable / interval)
└──────────┬──────────┘
           │ new snapshot
           ▼
┌─────────────────────┐
│   Diffing engine    │  common-prefix diff →
│   (TypingState)     │  Word / Sentence / nothing
└──────────┬──────────┘
           │ SpeakEvent
           ▼
┌─────────────────────┐
│    TTS worker       │  native SAPI / AVSpeechSynthesizer
│   (mpsc channel)    │  non-blocking, interruptible
└─────────────────────┘
```

### Runtime model

- On launch Loreglide installs a **tray icon** and shows its control panel.
- Clicking the window's close button **hides** the window; the service keeps running. The only way to actually quit is tray-menu → **Quit**.
- Left-clicking the tray icon toggles the window. Right-clicking opens the menu.
- Global watch defaults to **OFF** — nothing is read until you opt in.

### Two toggleable sources

1. **Global watch (primary)** — reads *any* text field in *any* app. Requires OS accessibility access (see permissions below). Password fields are skipped via UIA's `IsPassword` property.
2. **In-window test box (secondary)** — a tiny textarea inside the settings window. Useful for verifying the TTS engine without leaving the app.

### Dev helper

Set `LOREGLIDE_AUTO_WATCH=1` before launching in dev mode to boot with the global watcher already enabled. Lets you script end-to-end tests without clicking the toggle.

### macOS permission

The first time you flip **Global watch** on, macOS will prompt you to grant **Accessibility** permission:

> System Settings → Privacy & Security → Accessibility → enable "Loreglide"

If you deny it, the app silently drops back to editor-only mode and logs a warning.

### Windows

UI Automation just works out of the box for most Win32, UWP, and WPF applications. A few caveats:
- Some Electron apps don't expose their text fields unless their accessibility tree is built (e.g. VS Code does, Discord historically did not).
- Browsers: Chromium-based browsers expose text under UIA only when accessibility is enabled (which happens automatically when a screen reader / UIA client is running — *you* become that client).

---

## Project layout

```
Loreglide/
├── index.html                 Vite entry
├── package.json
├── vite.config.ts
├── tsconfig.json
├── src/                       React frontend
│   ├── main.tsx
│   ├── App.tsx                 UI (brutalist)
│   ├── App.css
│   ├── api.ts                  Tauri command wrappers
│   └── types.ts
└── src-tauri/                 Rust backend
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── capabilities/
    │   └── default.json
    └── src/
        ├── main.rs             Binary entrypoint
        ├── lib.rs              Tauri setup + commands
        ├── diffing.rs          Word/sentence detector + tests
        ├── tts.rs              Non-blocking TTS worker
        ├── watcher.rs          Cross-platform polling loop
        ├── watcher_windows.rs  UIA implementation
        └── watcher_macos.rs    AX implementation
```

---

## Settings (exposed in the UI and persisted in-memory)

| Setting              | Range       | Notes                                                |
| -------------------- | ----------- | ---------------------------------------------------- |
| Global watch         | on / off    | Default **off** · requires OS accessibility access   |
| Test-box echo        | on / off    | In-window textarea for smoke testing                 |
| Speak words          | on / off    | Fires on whitespace                                  |
| Speak sentences      | on / off    | Fires on `.`, `!`, `?`, `。`, `！`, `？`              |
| Voice rate           | 0.5 – 2.0   | Mapped to engine-native rate range                   |
| Volume               | 0 – 1       | Clamped to engine capability                         |
| Global poll interval | 50 – 500 ms | 100 ms is a good default                             |

> Persistence to disk is not wired up yet — see the roadmap.

---

## Roadmap

- [ ] Persist settings to `AppData` / `~/Library/Application Support`
- [ ] Per-app allow / deny list for global watch
- [ ] Grammar review — pass completed sentences through an LLM for suggestions
- [ ] Alternate TTS engines (ElevenLabs, local Piper) behind a plugin trait
- [ ] Global hotkey to pause / resume
- [ ] Auto-start on login (opt-in)
- [x] System tray icon with quick-toggle for global watch
- [x] Close-to-tray behavior instead of quit
- [x] Skip password fields in Windows UIA probe
- [x] Live "last heard" readout in the UI

---

## Icons

Placeholder icons (a black square with a white **L**) are generated by
`scripts/generate-icons.ps1` and checked in under `src-tauri/icons/` so
that `cargo build` and `tauri build` work out of the box.

To replace them with your own brand artwork, either:

```powershell
# Regenerate the built-in placeholders (useful after tweaking the PS1 script)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/generate-icons.ps1
```

or use the official Tauri CLI with a 1024×1024 source PNG:

```bash
npx @tauri-apps/cli icon path/to/logo.png
```

## Windows: MSVC toolchain notes

Rust on Windows compiles via the MSVC linker. During development we installed
the C++ Build Tools to `C:\BuildTools` (a non-default path used to work around
a partially-installed "canceled" shell that otherwise blocks reinstallation).
`vswhere.exe` discovers the toolchain automatically no matter where it lives,
so Cargo picks it up with no further configuration.

If you're bootstrapping on a new machine and the standard install path works,
the regular installer (`winget install Microsoft.VisualStudio.2022.BuildTools`
with the **Desktop development with C++** workload) is simpler. See the
prerequisites section above for details.

---

## License

MIT — do what you want with it.
