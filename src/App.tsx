import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import type { Settings, SpeakEvent } from "./types";

const DEFAULT_SETTINGS: Settings = {
  global_watch_enabled: false,
  editor_echo_enabled: true,
  speak_words: true,
  speak_sentences: true,
  rate: 1.0,
  volume: 1.0,
  poll_ms: 100,
};

export default function App() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [watcherAvailable, setWatcherAvailable] = useState<boolean>(false);
  const [lastEvent, setLastEvent] = useState<SpeakEvent | null>(null);
  const [lastEventTs, setLastEventTs] = useState<number | null>(null);
  const [testText, setTestText] = useState<string>("");
  const [showTest, setShowTest] = useState<boolean>(false);
  const tickTimer = useRef<number | null>(null);

  // Bootstrap: pull initial state from backend and wire up event listeners.
  useEffect(() => {
    let unlistenSpoke: UnlistenFn | null = null;
    let unlistenSettings: UnlistenFn | null = null;
    (async () => {
      try {
        const [s, avail] = await Promise.all([
          api.getSettings(),
          api.watcherAvailable(),
        ]);
        setSettings(s);
        setWatcherAvailable(avail);
      } catch (e) {
        console.error("bootstrap failed", e);
      }

      // Watcher fires one event per spoken word / sentence — we render
      // the most recent one so the user can verify the service is
      // actually catching their typing without needing to listen.
      unlistenSpoke = await listen<SpeakEvent>("loreglide:spoke", (e) => {
        setLastEvent(e.payload);
        setLastEventTs(Date.now());
      });
      // Tray menu's "Pause / resume" toggle updates settings from the
      // backend side — keep the UI in sync.
      unlistenSettings = await listen<Settings>(
        "loreglide:settings",
        (e) => setSettings(e.payload),
      );
    })();

    return () => {
      unlistenSpoke?.();
      unlistenSettings?.();
    };
  }, []);

  const patch = async (partial: Partial<Settings>) => {
    const next = { ...settings, ...partial };
    setSettings(next);
    try {
      const saved = await api.updateSettings(next);
      setSettings(saved);
    } catch (e) {
      console.error("update_settings failed", e);
    }
  };

  // --- Test box (lightweight) ------------------------------------------------
  const scheduleEditorTick = (value: string) => {
    if (tickTimer.current !== null) window.clearTimeout(tickTimer.current);
    tickTimer.current = window.setTimeout(async () => {
      try {
        const ev = await api.editorTick(value);
        if (ev) {
          setLastEvent(ev);
          setLastEventTs(Date.now());
        }
      } catch (e) {
        console.error("editor_tick failed", e);
      }
    }, 30);
  };

  const onTestChange = (v: string) => {
    setTestText(v);
    scheduleEditorTick(v);
  };

  const resetTest = async () => {
    await api.editorReset();
    setTestText("");
  };

  // --- Computed state --------------------------------------------------------
  const watching = settings.global_watch_enabled && watcherAvailable;
  const statusLabel = watching
    ? "LISTENING"
    : watcherAvailable
      ? "PAUSED"
      : "UNAVAILABLE";

  return (
    <div className="shell">
      <header className="bar">
        <div className="logo">LOREGLIDE</div>
        <div className="tag">TYPING ECHO · BACKGROUND SERVICE</div>
      </header>

      {/* ---- Hero: global watch state + toggle --------------------------- */}
      <section className={`hero state-${watching ? "on" : "off"}`}>
        <div className="hero-status-line">
          <span
            className={`dot ${watching ? "dot-on" : "dot-off"}`}
            aria-hidden
          />
          <span className="hero-status">{statusLabel}</span>
        </div>

        <p className="hero-copy">
          {watching
            ? "Loreglide is listening to whatever text field you focus — Cursor, Google Docs, Gmail, VS Code, Word, browser inputs. Each finished word speaks; each finished sentence reads back."
            : watcherAvailable
              ? "Service is paused. Flip it on to hear every word and sentence you type in any application."
              : "The OS accessibility API is unavailable on this system. Only the test box below will work."}
        </p>

        <button
          className={`hero-toggle ${watching ? "on" : "off"}`}
          onClick={() => patch({ global_watch_enabled: !settings.global_watch_enabled })}
          disabled={!watcherAvailable}
        >
          {watching ? "PAUSE" : "START LISTENING"}
        </button>

        <LastHeard event={lastEvent} ts={lastEventTs} />
      </section>

      {/* ---- Voice + trigger settings ------------------------------------ */}
      <section className="panel">
        <h2 className="panel-title">VOICE</h2>
        <Slider
          label={`rate · ${settings.rate.toFixed(2)}x`}
          min={0.5}
          max={2.0}
          step={0.05}
          value={settings.rate}
          onChange={(v) => patch({ rate: v })}
        />
        <Slider
          label={`volume · ${(settings.volume * 100).toFixed(0)}%`}
          min={0}
          max={1}
          step={0.05}
          value={settings.volume}
          onChange={(v) => patch({ volume: v })}
        />
        <div className="row row-compact">
          <button className="btn btn-ghost" onClick={() => api.stopSpeaking()}>
            Stop voice
          </button>
        </div>
      </section>

      <section className="panel">
        <h2 className="panel-title">TRIGGERS</h2>
        <Toggle
          label="Speak each finished word"
          checked={settings.speak_words}
          onChange={(v) => patch({ speak_words: v })}
        />
        <Toggle
          label="Speak each finished sentence"
          checked={settings.speak_sentences}
          onChange={(v) => patch({ speak_sentences: v })}
        />
        <Slider
          label={`poll · ${settings.poll_ms} ms`}
          min={50}
          max={500}
          step={10}
          value={settings.poll_ms}
          onChange={(v) => patch({ poll_ms: Math.round(v) })}
        />
      </section>

      {/* ---- Optional: expandable test box ------------------------------- */}
      <section className="panel">
        <button
          className="panel-title panel-title-btn"
          onClick={() => setShowTest((s) => !s)}
        >
          {showTest ? "▾ TEST BOX" : "▸ TEST BOX"}
        </button>
        {showTest && (
          <>
            <p className="panel-hint">
              Type here to verify the TTS engine is working without
              switching apps. Exact same pipeline as the global watcher.
            </p>
            <textarea
              className="editor editor-small"
              value={testText}
              onChange={(e) => onTestChange(e.target.value)}
              placeholder="Type here…"
              spellCheck
            />
            <div className="row row-compact">
              <button className="btn btn-ghost" onClick={resetTest}>
                Clear
              </button>
            </div>
          </>
        )}
      </section>

      <footer className="foot">
        Close the window — Loreglide stays in the tray. Right-click the tray icon to quit.
      </footer>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function LastHeard({
  event,
  ts,
}: {
  event: SpeakEvent | null;
  ts: number | null;
}) {
  const [age, setAge] = useState<string>("—");
  useEffect(() => {
    if (ts === null) return;
    const tick = () => {
      const delta = Math.max(0, Date.now() - ts);
      if (delta < 1500) setAge("just now");
      else if (delta < 60_000) setAge(`${Math.floor(delta / 1000)}s ago`);
      else setAge(`${Math.floor(delta / 60_000)}m ago`);
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [ts]);

  return (
    <div className="last-heard">
      <span className="last-heard-label">LAST HEARD</span>
      {event ? (
        <>
          <span className={`last-heard-kind kind-${event.kind}`}>
            {event.kind}
          </span>
          <span className="last-heard-text">"{event.text}"</span>
          <span className="last-heard-age">{age}</span>
        </>
      ) : (
        <span className="last-heard-idle">nothing yet</span>
      )}
    </div>
  );
}

function Toggle(props: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className={`toggle ${props.disabled ? "is-disabled" : ""}`}>
      <input
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onChange={(e) => props.onChange(e.target.checked)}
      />
      <span className="toggle-box" />
      <span className="toggle-label">{props.label}</span>
    </label>
  );
}

function Slider(props: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="slider">
      <span className="slider-label">{props.label}</span>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        onChange={(e) => props.onChange(parseFloat(e.target.value))}
      />
    </label>
  );
}
