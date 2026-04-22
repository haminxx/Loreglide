import { invoke } from "@tauri-apps/api/core";
import type { Settings, SpeakEvent } from "./types";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (s: Settings) => invoke<Settings>("update_settings", { new: s }),
  editorTick: (text: string) => invoke<SpeakEvent | null>("editor_tick", { text }),
  editorReset: () => invoke<void>("editor_reset"),
  speakText: (text: string) => invoke<void>("speak_text", { text }),
  stopSpeaking: () => invoke<void>("stop_speaking"),
  watcherAvailable: () => invoke<boolean>("watcher_available"),
};
