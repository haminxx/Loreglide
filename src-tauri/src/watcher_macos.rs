//! macOS implementation of `FocusProbe` using the Accessibility API.
//!
//! The flow is:
//!   1. Get the front-most application via `NSWorkspace.frontmostApplication`
//!      (exposed as a system-wide AX element).
//!   2. From the system-wide element read `kAXFocusedUIElementAttribute`.
//!   3. From the focused element read `kAXValueAttribute` (for text fields)
//!      or `kAXSelectedTextAttribute` / `kAXTitleAttribute` as fallbacks.
//!
//! The user must grant Accessibility permission in
//! `System Settings → Privacy & Security → Accessibility`.
//! We detect missing permission and log a clear message.

#![cfg(target_os = "macos")]

use log::{trace, warn};

use accessibility::{AXAttribute, AXUIElement};
use accessibility_sys::{
    kAXErrorAPIDisabled, kAXErrorNotImplemented, kAXFocusedUIElementAttribute,
    kAXTitleAttribute, kAXValueAttribute,
};
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;

use crate::watcher::{FocusId, FocusProbe, FocusSnapshot};

pub struct MacosProbe {
    system: AXUIElement,
}

impl MacosProbe {
    pub fn new() -> Result<Self, String> {
        // Probe permissions by attempting to read from the system-wide
        // element. If the OS returns `kAXErrorAPIDisabled` it means the
        // user has not granted Accessibility permission.
        let system = AXUIElement::system_wide();
        let attr: AXAttribute<CFType> =
            AXAttribute::new(&CFString::from_static_string(kAXFocusedUIElementAttribute));
        match system.attribute(&attr) {
            Ok(_) => Ok(Self { system }),
            Err(e) if e.0 == kAXErrorAPIDisabled => Err(
                "Accessibility permission not granted. Enable Loreglide in System \
                 Settings → Privacy & Security → Accessibility."
                    .into(),
            ),
            Err(e) if e.0 == kAXErrorNotImplemented => {
                // No focused element right now; permission is probably OK.
                Ok(Self { system })
            }
            Err(e) => Err(format!("AX init error: {e:?}")),
        }
    }
}

impl FocusProbe for MacosProbe {
    fn snapshot(&mut self) -> Option<FocusSnapshot> {
        let focused_attr: AXAttribute<CFType> =
            AXAttribute::new(&CFString::from_static_string(kAXFocusedUIElementAttribute));
        let focused_ref = match self.system.attribute(&focused_attr) {
            Ok(v) => v,
            Err(e) => {
                trace!("no focused AX element: {e:?}");
                return None;
            }
        };
        // Re-wrap the CFType as an AXUIElement.
        let focused = unsafe {
            AXUIElement::wrap_under_get_rule(focused_ref.as_concrete_TypeRef() as _)
        };

        // Identity: Apple doesn't expose a stable ID; use (pid, role, title).
        let pid = focused.pid().unwrap_or(0);
        let role = focused
            .role()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let title_attr: AXAttribute<CFString> =
            AXAttribute::new(&CFString::from_static_string(kAXTitleAttribute));
        let title = focused
            .attribute(&title_attr)
            .map(|s| s.to_string())
            .unwrap_or_default();
        let id = format!("pid{pid}-{role}-{title}");

        // Read the value.
        let value_attr: AXAttribute<CFType> =
            AXAttribute::new(&CFString::from_static_string(kAXValueAttribute));
        let text = focused
            .attribute(&value_attr)
            .ok()
            .and_then(|cf| cf.downcast_into::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_else(|| title.clone());

        Some(FocusSnapshot {
            id: FocusId(id),
            text,
        })
    }
}

pub fn try_new() -> Option<MacosProbe> {
    match MacosProbe::new() {
        Ok(p) => Some(p),
        Err(e) => {
            warn!("macOS AX probe unavailable: {e}");
            None
        }
    }
}
