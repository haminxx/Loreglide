# Loreglide

A Grammarly-style typing companion that **speaks what you type in real time**. Every finished word is read out loud the moment you press space. Every finished sentence is read back when you hit `.`, `!`, or `?`. A "Read whole text" button lets you review long paragraphs.

Built for writers, copywriters, directors, novelists — anyone who needs to *hear* their prose to catch what the eye misses.

- **Frontend**: React + TypeScript + Vite, rendered in a Tauri 2 window.
- **Backend**: Rust (single process, `~15 MB` binary).
- **TTS**: Native SAPI5 (Windows) / AVSpeechSynthesizer (macOS). Offline, zero-latency, no API keys.
- **Accessibility**: UI Automation on Windows, AX API on macOS.

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

Two separately-toggleable modes:

1. **Global watch** — reads *any* text field in *any* app. Requires OS accessibility access (see permissions below).
2. **In-app editor echo** — works everywhere, always, with zero permissions. Useful for initial testing and as a guaranteed fallback.

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

| Setting              | Range       | Notes                                               |
| -------------------- | ----------- | --------------------------------------------------- |
| Global watch         | on / off    | Requires OS accessibility access                    |
| In-app editor echo   | on / off    | Works everywhere                                    |
| Speak words          | on / off    | Fires on whitespace                                 |
| Speak sentences      | on / off    | Fires on `.`, `!`, `?`, `。`, `！`, `？`             |
| Voice rate           | 0.5 – 2.0   | Mapped to engine-native rate range                  |
| Volume               | 0 – 1       | Clamped to engine capability                        |
| Global poll interval | 50 – 500 ms | 100 ms is a good default                            |

> Persistence to disk is not wired up yet — see the roadmap.

---

## Roadmap

- [ ] Persist settings to `AppData`/`~/Library/Application Support`
- [ ] System tray icon with quick-toggle for global watch
- [ ] Per-app allow/deny list for global watch
- [ ] Grammar review (pass completed sentences through an LLM for suggestions)
- [ ] Alternate TTS engines (ElevenLabs, local Piper) behind a plugin trait
- [ ] Global hotkey to pause / resume

---

## Icons

Before `npm run tauri:build` will produce a bundled installer you need icons in `src-tauri/icons/`. Generate them with:

```bash
npx @tauri-apps/cli icon path/to/logo.png
```

---

## License

MIT — do what you want with it.
