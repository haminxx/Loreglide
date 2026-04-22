import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { Settings, SpeakEvent } from "./types";

type Status = {
  lastEvent: SpeakEvent | null;
  watcherAvailable: boolean;
};

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
  const [status, setStatus] = useState<Status>({
    lastEvent: null,
    watcherAvailable: false,
  });
  const [text, setText] = useState<string>("");
  const tickTimer = useRef<number | null>(null);

  // Bootstrap: pull settings + watcher availability from backend.
  useEffect(() => {
    (async () => {
      try {
        const [s, avail] = await Promise.all([
          api.getSettings(),
          api.watcherAvailable(),
        ]);
        setSettings(s);
        setStatus((st) => ({ ...st, watcherAvailable: avail }));
      } catch (e) {
        console.error("bootstrap failed", e);
      }
    })();
  }, []);

  // Debounced per-keystroke tick. We don't need to ship every single
  // character — the diffing engine works fine with small batches, and a
  // 30ms debounce keeps us well below human perception while also
  // de-thrashing the IPC bridge.
  const scheduleTick = (value: string) => {
    if (tickTimer.current !== null) {
      window.clearTimeout(tickTimer.current);
    }
    tickTimer.current = window.setTimeout(async () => {
      try {
        const ev = await api.editorTick(value);
        if (ev) setStatus((st) => ({ ...st, lastEvent: ev }));
      } catch (e) {
        console.error("editor_tick failed", e);
      }
    }, 30);
  };

  const onTextChange = (v: string) => {
    setText(v);
    scheduleTick(v);
  };

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

  const speakAll = async () => {
    if (text.trim()) await api.speakText(text);
  };

  const stop = () => api.stopSpeaking().catch(console.error);

  const reset = async () => {
    await api.editorReset();
    setText("");
    setStatus((st) => ({ ...st, lastEvent: null }));
  };

  return (
    <div className="shell">
      <header className="bar">
        <div className="logo">LOREGLIDE</div>
        <div className="tag">TYPING ECHO // WRITE &amp; LISTEN</div>
      </header>

      <section className="panel">
        <h2 className="panel-title">01 / SETTINGS</h2>

        <Toggle
          label="Global watch (speaks whatever you type in ANY app)"
          disabled={!status.watcherAvailable}
          checked={settings.global_watch_enabled}
          onChange={(v) => patch({ global_watch_enabled: v })}
          hint={
            status.watcherAvailable
              ? undefined
              : "OS accessibility API unavailable — editor mode still works"
          }
        />
        <Toggle
          label="In-app editor echo"
          checked={settings.editor_echo_enabled}
          onChange={(v) => patch({ editor_echo_enabled: v })}
        />
        <Toggle
          label="Speak on word completion"
          checked={settings.speak_words}
          onChange={(v) => patch({ speak_words: v })}
        />
        <Toggle
          label="Speak on sentence completion"
          checked={settings.speak_sentences}
          onChange={(v) => patch({ speak_sentences: v })}
        />

        <Slider
          label={`Voice rate — ${settings.rate.toFixed(2)}x`}
          min={0.5}
          max={2.0}
          step={0.05}
          value={settings.rate}
          onChange={(v) => patch({ rate: v })}
        />
        <Slider
          label={`Volume — ${(settings.volume * 100).toFixed(0)}%`}
          min={0}
          max={1}
          step={0.05}
          value={settings.volume}
          onChange={(v) => patch({ volume: v })}
        />
        <Slider
          label={`Global poll interval — ${settings.poll_ms} ms`}
          min={50}
          max={500}
          step={10}
          value={settings.poll_ms}
          onChange={(v) => patch({ poll_ms: Math.round(v) })}
        />
      </section>

      <section className="panel">
        <h2 className="panel-title">02 / EDITOR</h2>
        <textarea
          className="editor"
          value={text}
          onChange={(e) => onTextChange(e.target.value)}
          placeholder="Type here. Each finished word is spoken. Each finished sentence is read back."
          spellCheck
        />
        <div className="row">
          <button className="btn" onClick={speakAll}>
            Read whole text
          </button>
          <button className="btn" onClick={stop}>
            Stop
          </button>
          <button className="btn btn-ghost" onClick={reset}>
            Clear
          </button>
        </div>
        <div className="status">
          {status.lastEvent ? (
            <>
              <span className="status-kind">
                {status.lastEvent.kind.toUpperCase()}
              </span>{" "}
              <span className="status-text">"{status.lastEvent.text}"</span>
            </>
          ) : (
            <span className="status-idle">Awaiting input…</span>
          )}
        </div>
      </section>

      <footer className="foot">
        v0.1 · press · listen · refine
      </footer>
    </div>
  );
}

function Toggle(props: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  hint?: string;
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
      {props.hint && <span className="toggle-hint">{props.hint}</span>}
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
