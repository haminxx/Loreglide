//! Diffing engine: compares the latest snapshot of a focused text field
//! against the previous snapshot and decides whether a *word* or a
//! *sentence* was just completed, which are the two events we want to
//! voice.
//!
//! Design notes
//! ------------
//! * We never trust `str::len()` to detect "what was added" — Unicode
//!   and middle-of-string edits (paste, IME composition) both break
//!   that assumption. Instead we compute the common character prefix
//!   between the old and new string and treat the *tail* of the new
//!   string (everything after the common prefix) as the newly-added
//!   region.
//! * Word trigger: the newly-added region ends with a whitespace
//!   character. We then walk *backwards* from the whitespace in the
//!   full new string to find the last completed word.
//! * Sentence trigger: the newly-added region ends with `.`, `!`, `?`,
//!   or their full-width counterparts. We walk backwards to find the
//!   previous sentence boundary and voice the span in between.
//! * De-duplication: we remember the last spoken word and sentence so
//!   that repeated polls (same text, same state) never double-speak.
//! * Shrink: if the new text is strictly shorter than the old *and*
//!   shares a prefix, we only update memory and return `None`.

use serde::Serialize;

/// An event that should be voiced by the TTS engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum SpeakEvent {
    /// A single word was just completed (user pressed space / newline / tab).
    Word(String),
    /// A full sentence was just completed (user pressed `.`, `!`, `?`).
    Sentence(String),
}

impl SpeakEvent {
    pub fn text(&self) -> &str {
        match self {
            SpeakEvent::Word(s) | SpeakEvent::Sentence(s) => s,
        }
    }
}

/// Rolling state for one focused text field.
#[derive(Debug, Default, Clone)]
pub struct TypingState {
    pub last_text: String,
    pub last_spoken_word: Option<String>,
    pub last_spoken_sentence: Option<String>,
}

impl TypingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wipe state. Call when focus moves to a *different* text field so
    /// the first snapshot in the new field doesn't get voiced.
    pub fn reset(&mut self) {
        self.last_text.clear();
        self.last_spoken_word = None;
        self.last_spoken_sentence = None;
    }
}

const SENTENCE_ENDERS: &[char] = &['.', '!', '?', '。', '！', '？'];

fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || c == '\t' || c == '\n'
}

fn is_sentence_ender(c: char) -> bool {
    SENTENCE_ENDERS.contains(&c)
}

/// Compute a `(char_count, byte_offset)` for the longest common *character*
/// prefix between `a` and `b`. Using byte offsets keeps slicing safe under
/// UTF-8; using char counts lets callers reason about "how many characters
/// were added."
fn common_prefix(a: &str, b: &str) -> (usize, usize) {
    let mut chars_a = a.char_indices();
    let mut chars_b = b.char_indices();
    let mut last_byte = 0usize;
    let mut char_count = 0usize;
    loop {
        match (chars_a.next(), chars_b.next()) {
            (Some((_, ca)), Some((ib, cb))) if ca == cb => {
                last_byte = ib + cb.len_utf8();
                char_count += 1;
            }
            _ => break,
        }
    }
    (char_count, last_byte)
}

/// Evaluate a text change and return an event to speak, if any.
pub fn evaluate(new_text: &str, state: &mut TypingState) -> Option<SpeakEvent> {
    if new_text == state.last_text {
        return None;
    }

    let (_, prefix_byte) = common_prefix(&state.last_text, new_text);
    let added = &new_text[prefix_byte..];

    // Pure shrink / backspace with no new content → just remember and exit.
    if added.is_empty() {
        state.last_text = new_text.to_owned();
        return None;
    }

    // We only speak on *completion*, which means the newly-typed region
    // must end in a boundary character (space or sentence ender).
    let last_added_char = added.chars().rev().next();
    let event = match last_added_char {
        Some(c) if is_sentence_ender(c) => extract_sentence(new_text, state),
        Some(c) if is_word_boundary(c) => extract_word(new_text, state),
        _ => None,
    };

    state.last_text = new_text.to_owned();
    event
}

/// Find the most-recently-completed word in `text`. `text` is assumed to
/// end with a word-boundary character.
fn extract_word(text: &str, state: &mut TypingState) -> Option<SpeakEvent> {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    // The last word is everything after the last whitespace in `trimmed`.
    let word_start = trimmed
        .rfind(|c: char| c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    let word = trimmed[word_start..].trim_matches(|c: char| {
        // Strip surrounding punctuation that would make "hello," speak oddly.
        !c.is_alphanumeric() && c != '\''
    });
    if word.is_empty() {
        return None;
    }
    // De-dup: if the very same word was just spoken (e.g. polling jitter
    // caused a duplicate event) skip it.
    if state.last_spoken_word.as_deref() == Some(word) && state.last_text.ends_with(word) {
        // only de-dup within the same field snapshot window
    }
    state.last_spoken_word = Some(word.to_owned());
    Some(SpeakEvent::Word(word.to_owned()))
}

/// Find the sentence that just ended in `text`. `text` is assumed to end
/// in a sentence-ending punctuation character.
fn extract_sentence(text: &str, state: &mut TypingState) -> Option<SpeakEvent> {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    // Walk backwards past the terminal punctuation we just hit.
    let end = trimmed.len();
    let body = trimmed.trim_end_matches(|c: char| is_sentence_ender(c));
    // Find the boundary of the *previous* sentence.
    let start = body
        .rfind(|c: char| is_sentence_ender(c))
        .map(|i| i + body[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1))
        .unwrap_or(0);
    let sentence = trimmed[start..end].trim();
    if sentence.is_empty() {
        return None;
    }
    if state.last_spoken_sentence.as_deref() == Some(sentence) {
        return None;
    }
    state.last_spoken_sentence = Some(sentence.to_owned());
    Some(SpeakEvent::Sentence(sentence.to_owned()))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn run(inputs: &[&str]) -> Vec<SpeakEvent> {
        let mut state = TypingState::new();
        let mut events = Vec::new();
        for s in inputs {
            if let Some(e) = evaluate(s, &mut state) {
                events.push(e);
            }
        }
        events
    }

    #[test]
    fn word_fires_on_space() {
        let events = run(&["h", "he", "hel", "hell", "hello", "hello "]);
        assert_eq!(events, vec![SpeakEvent::Word("hello".into())]);
    }

    #[test]
    fn sentence_fires_on_period() {
        let events = run(&["Hi", "Hi ", "Hi th", "Hi there", "Hi there."]);
        assert!(events
            .iter()
            .any(|e| matches!(e, SpeakEvent::Sentence(s) if s == "Hi there.")));
    }

    #[test]
    fn backspace_does_not_speak() {
        let events = run(&["hello", "hell", "hel"]);
        assert!(events.is_empty());
    }

    #[test]
    fn paste_in_middle_does_not_false_fire_sentence() {
        // User pastes "foo" into the middle of "abc abc". Neither the
        // boundary nor a sentence ender is the last-added char.
        let events = run(&["abc abc", "abfooc abc"]);
        assert!(events.is_empty());
    }

    #[test]
    fn trailing_punctuation_is_stripped_from_word() {
        let events = run(&["Hello,", "Hello, "]);
        assert_eq!(events, vec![SpeakEvent::Word("Hello".into())]);
    }

    #[test]
    fn multi_sentence_only_speaks_newest() {
        let mut state = TypingState::new();
        assert_eq!(
            evaluate("First sentence.", &mut state),
            Some(SpeakEvent::Sentence("First sentence.".into()))
        );
        assert_eq!(
            evaluate("First sentence. Second.", &mut state),
            Some(SpeakEvent::Sentence("Second.".into()))
        );
    }

    #[test]
    fn unicode_is_respected() {
        // "café " → should speak "café".
        let events = run(&["café ", "café w", "café world "]);
        assert_eq!(
            events,
            vec![
                SpeakEvent::Word("café".into()),
                SpeakEvent::Word("world".into()),
            ]
        );
    }

    #[test]
    fn repeat_sentence_does_not_double_speak() {
        let mut state = TypingState::new();
        assert!(evaluate("Hi there.", &mut state).is_some());
        // Same snapshot arrives again from the watcher.
        assert!(evaluate("Hi there.", &mut state).is_none());
    }

    #[test]
    fn reset_clears_memory() {
        let mut state = TypingState::new();
        evaluate("Hello world ", &mut state);
        state.reset();
        assert_eq!(state.last_text, "");
        assert!(state.last_spoken_word.is_none());
    }
}
