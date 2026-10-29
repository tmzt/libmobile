//! **The clipboard's text** (feature `clipboard`): the platform-free
//! types. The calls are in `android::clipboard`.
//!
//! Read only: libmobile does not write to the clipboard. It never logs the
//! clipboard's text or its length.
//!
//! Two Android rules shape the calls:
//!
//! * **Android 10 (API 29) and later** give the clipboard only to the app
//!   with window focus (or the default keyboard); any other app is told it
//!   is empty. libmobile checks focus first and answers
//!   [`ClipboardError::NotFocused`] instead.
//! * **Android 12 (API 31) and later** show a "<app> pasted from your
//!   clipboard" toast when an app reads a clip another app copied.
//!   `android::clipboard::read_text` triggers it; `has_text` reads only the
//!   clip's description and does not.

use std::fmt;

/// Why the clipboard could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardError {
    /// The app's window does not have input focus. From Android 10 only
    /// the app with window focus (or the default keyboard) may read the
    /// clipboard; Android would answer as if it were empty. Read it on the
    /// user's tap, while the app is in front and no dialog is over it.
    NotFocused,
    /// Anything else Android or JNI reported.
    Platform(String),
}

impl ClipboardError {
    /// From the plugin's answer kind; `None` for the kinds that are not
    /// errors (`"text"`, `"none"`, `"yes"`, `"no"`).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "text" | "none" | "yes" | "no" => None,
            "not-focused" => Some(ClipboardError::NotFocused),
            other => Some(ClipboardError::Platform(format!(
                "unexpected answer {other:?}"
            ))),
        }
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::NotFocused => {
                f.write_str("the clipboard may only be read while the app's window has focus")
            }
            ClipboardError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// The plugin's text as the API gives it: an empty or missing text is
/// `None`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_map_to_errors() {
        assert_eq!(ClipboardError::from_kind("text"), None);
        assert_eq!(ClipboardError::from_kind("none"), None);
        assert_eq!(ClipboardError::from_kind("yes"), None);
        assert_eq!(ClipboardError::from_kind("no"), None);
        assert_eq!(
            ClipboardError::from_kind("not-focused"),
            Some(ClipboardError::NotFocused)
        );
        assert!(matches!(
            ClipboardError::from_kind("bogus"),
            Some(ClipboardError::Platform(_))
        ));
    }

    #[test]
    fn empty_text_is_none() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(Some("a".into())), Some("a".into()));
        assert_eq!(non_empty(Some(" ".into())), Some(" ".into()));
    }

    #[test]
    fn errors_display() {
        assert!(ClipboardError::NotFocused.to_string().contains("focus"));
        assert_eq!(
            ClipboardError::Platform("x".into()).to_string(),
            "platform error: x"
        );
    }
}
