export type Settings = {
  global_watch_enabled: boolean;
  editor_echo_enabled: boolean;
  speak_words: boolean;
  speak_sentences: boolean;
  rate: number;
  volume: number;
  poll_ms: number;
};

export type SpeakEvent =
  | { kind: "word"; text: string }
  | { kind: "sentence"; text: string };
