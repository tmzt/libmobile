//! **The clipboard's text** (feature `clipboard`), read and written: the
//! platform-free types. The calls are in `android::clipboard`.
//!
//! libmobile never logs the clipboard's text or its length, whether it
//! reads or writes it.
//!
//! Three Android rules shape the calls:
//!
//! * **Android 10 (API 29) and later** give the clipboard only to the app
//!   with window focus (or the default keyboard); any other app is told it
//!   is empty. libmobile checks focus first and answers
//!   [`ClipboardError::NotFocused`] instead.
//! * **Android 12 (API 31) and later** show a "<app> pasted from your
//!   clipboard" toast when an app reads a clip another app copied.
//!   `android::clipboard::read_text` triggers it; `has_text` reads only the
//!   clip's description and does not.
//! * **Android 13 (API 33) and later** show their own confirmation after
//!   an app writes the clipboard, so the app should show none of its own
//!   ([`Copied::system_notice`]). Writing needs no permission and no focus.

use std::fmt;

/// A copy that Android accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Copied {
    /// Android showed its own confirmation of the copy: Android 13
    /// (API 33) and later. The app should then show none of its own; on
    /// older versions (`false`) it may show a short "Copied" notice.
    pub system_notice: bool,
}

/// Why the clipboard could not be read or written. A write never gives
/// [`ClipboardError::NotFocused`], only [`ClipboardError::Platform`].
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
    /// errors (`"text"`, `"none"`, `"yes"`, `"no"`, `"copied"`,
    /// `"copied-notice"`).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "text" | "none" | "yes" | "no" | "copied" | "copied-notice" => None,
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

/// The write's answer kind as the API gives it: `"copied"` and
/// `"copied-notice"` are a [`Copied`]; anything else is an error through
/// [`ClipboardError::from_kind`].
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn copied_from_kind(kind: &str) -> Result<Copied, ClipboardError> {
    match kind {
        "copied" => Ok(Copied {
            system_notice: false,
        }),
        "copied-notice" => Ok(Copied {
            system_notice: true,
        }),
        other => Err(ClipboardError::from_kind(other)
            .unwrap_or_else(|| ClipboardError::Platform(format!("unexpected answer {other:?}")))),
    }
}

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
        assert_eq!(ClipboardError::from_kind("copied"), None);
        assert_eq!(ClipboardError::from_kind("copied-notice"), None);
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
    fn write_kinds_map_to_copied() {
        assert_eq!(
            copied_from_kind("copied"),
            Ok(Copied {
                system_notice: false
            })
        );
        assert_eq!(
            copied_from_kind("copied-notice"),
            Ok(Copied {
                system_notice: true
            })
        );
        assert!(matches!(
            copied_from_kind("bogus"),
            Err(ClipboardError::Platform(_))
        ));
        assert_eq!(
            copied_from_kind("not-focused"),
            Err(ClipboardError::NotFocused)
        );
        // A read's answer is not a copy.
        assert!(matches!(
            copied_from_kind("text"),
            Err(ClipboardError::Platform(_))
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
