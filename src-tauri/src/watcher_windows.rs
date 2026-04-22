//! Windows implementation of `FocusProbe` using UI Automation.
//!
//! We ask the `UIAutomation` COM interface for the currently-focused
//! element and then read its text in order of preference:
//!   1. `ValuePattern.Value` — edit controls (text boxes, address bars).
//!   2. `TextPattern.GetVisibleRanges` joined — rich text controls.
//!   3. `Name` — as a last-ditch fallback for labels.
//!
//! COM must be initialized on the watcher thread. The `uiautomation`
//! crate handles that automatically in `UIAutomation::new()`.

#![cfg(windows)]

use log::{trace, warn};
use uiautomation::patterns::{UITextPattern, UIValuePattern};
use uiautomation::UIAutomation;

use crate::watcher::{FocusId, FocusProbe, FocusSnapshot};

pub struct WindowsProbe {
    automation: UIAutomation,
}

impl WindowsProbe {
    pub fn new() -> Result<Self, String> {
        let automation = UIAutomation::new().map_err(|e| e.to_string())?;
        Ok(Self { automation })
    }
}

impl FocusProbe for WindowsProbe {
    fn snapshot(&mut self) -> Option<FocusSnapshot> {
        let element = match self.automation.get_focused_element() {
            Ok(e) => e,
            Err(e) => {
                trace!("no focused element: {e}");
                return None;
            }
        };

        // PRIVACY: never read password fields. UIA exposes the
        // `IsPassword` property; any control that claims it stores
        // secret text is skipped silently.
        if matches!(element.is_password(), Ok(true)) {
            trace!("skipping focused password field");
            return None;
        }

        // Build a stable identity for this element so we can detect
        // focus changes. Runtime IDs are the most reliable identifier
        // UIA provides; fall back to (process_id, control_type, name).
        let id = element
            .get_runtime_id()
            .ok()
            .map(|rid| {
                let mut s = String::with_capacity(rid.len() * 4);
                for n in rid {
                    s.push_str(&format!("{n:x}-"));
                }
                s
            })
            .or_else(|| {
                let pid = element.get_process_id().ok()?;
                let name = element.get_name().unwrap_or_default();
                let ct = element.get_control_type().ok().map(|c| c as i32).unwrap_or(0);
                Some(format!("pid{pid}-ct{ct}-{name}"))
            })
            .unwrap_or_default();

        // Prefer ValuePattern: that's what edit controls expose.
        let text = if let Ok(p) = element.get_pattern::<UIValuePattern>() {
            p.get_value().ok()
        } else {
            None
        }
        // Then TextPattern (rich text / documents).
        .or_else(|| {
            element.get_pattern::<UITextPattern>().ok().and_then(|p| {
                let range = p.get_document_range().ok()?;
                range.get_text(-1).ok()
            })
        })
        // Then Name (labels, some custom controls).
        .or_else(|| element.get_name().ok())
        .unwrap_or_default();

        if text.is_empty() {
            // Empty strings are legitimate (user cleared the field), but
            // we still want to detect focus changes, so emit the snapshot.
            trace!("focused element has no text");
        }

        Some(FocusSnapshot {
            id: FocusId(id),
            text,
        })
    }
}

/// Helper used by the library's `init` to create a probe, logging any
/// failure instead of panicking so the app can still launch in
/// "in-app editor only" mode.
pub fn try_new() -> Option<WindowsProbe> {
    match WindowsProbe::new() {
        Ok(p) => Some(p),
        Err(e) => {
            warn!("UIAutomation unavailable: {e}");
            None
        }
    }
}
