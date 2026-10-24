//! **Opening a link in another app**: the platform-free half. The call,
//! `android::link::open_link`, asks Android to open an http or https URL in
//! the app that handles it, normally the browser.
//!
//! [`check_link`] accepts and rejects exactly what libdevice's does, so
//! libmorph's `open_link` behaves the same on every host.

use std::fmt;

/// Why a link was not opened.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenLinkError {
    /// Not an absolute http or https URL, or it holds whitespace or
    /// control characters.
    BadUrl,
    /// No app on the device handles it.
    NoHandler,
    /// The app is not visible, and Android does not let it start one.
    NotVisible,
    /// Anything else Android or JNI reported.
    Platform(String),
}

impl OpenLinkError {
    /// From the plugin's `"<kind>: <detail>"` message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let kind = message.split_once(':').map_or(message, |(kind, _)| kind);
        match kind.trim() {
            "no-handler" => OpenLinkError::NoHandler,
            "not-visible" => OpenLinkError::NotVisible,
            _ => OpenLinkError::Platform(message.to_string()),
        }
    }
}

impl fmt::Display for OpenLinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUrl => f.write_str("not an http or https link"),
            Self::NoHandler => f.write_str("no app opens this link"),
            Self::NotVisible => f.write_str("a link may only be opened while the app is visible"),
            Self::Platform(message) => write!(f, "opening the link failed: {message}"),
        }
    }
}

impl std::error::Error for OpenLinkError {}

/// The check `android::link::open_link` makes before sending anything:
/// the scheme is `http` or `https` (any case), followed by `://` and a
/// non-empty host, and the link holds no whitespace or control characters.
/// Every other scheme (`file:`, `content:`, `intent:`, `javascript:`,
/// `mailto:` and the rest) is [`OpenLinkError::BadUrl`]. Platform-free.
pub fn check_link(url: &str) -> Result<(), OpenLinkError> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(OpenLinkError::BadUrl);
    }
    let (scheme, rest) = url.split_once("://").ok_or(OpenLinkError::BadUrl)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(OpenLinkError::BadUrl);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_and_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_and_port.strip_prefix('[') {
        // An IPv6 literal: everything up to the closing bracket.
        Some(literal) => literal.split_once(']').ok_or(OpenLinkError::BadUrl)?.0,
        None => host_and_port.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() {
        return Err(OpenLinkError::BadUrl);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OpenLinkError, check_link};

    #[test]
    fn http_and_https_are_accepted() {
        for url in [
            "http://example.com",
            "https://example.com/",
            "https://example.com/a?k=v#f",
            "http://host:8088/#/src--handlr/docs/design/plan.md",
            "http://user:pass@host/",
            "http://[::1]:8080/",
            "https://h?q",
            "https://h#f",
        ] {
            assert_eq!(check_link(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn upper_case_schemes_are_accepted() {
        for url in [
            "HTTP://example.com/",
            "HTTPS://example.com/",
            "HtTpS://example.com/",
        ] {
            assert_eq!(check_link(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn a_missing_host_is_rejected() {
        for url in [
            "http://",
            "https:///path",
            "http://?q",
            "http://#f",
            "http://:8080/",
            "http://user@/",
            "http://[]/",
            "http://[::1/",
            "http:example.com",
            "https:/example.com",
            "example.com",
        ] {
            assert_eq!(check_link(url), Err(OpenLinkError::BadUrl), "{url}");
        }
    }

    #[test]
    fn whitespace_is_rejected() {
        for url in [
            "https://example.com/a b",
            " https://example.com/",
            "https://example.com/\t",
            "https://example.com/\n",
            "https://example.com/\u{a0}",
        ] {
            assert_eq!(check_link(url), Err(OpenLinkError::BadUrl), "{url:?}");
        }
    }

    #[test]
    fn control_characters_are_rejected() {
        for url in [
            "https://example.com/\0",
            "https://example.com/\u{7}",
            "https://example.com/\u{1b}",
            "https://example.com/\u{7f}",
            "https://exa\u{85}mple.com/",
        ] {
            assert_eq!(check_link(url), Err(OpenLinkError::BadUrl), "{url:?}");
        }
    }

    #[test]
    fn other_schemes_are_rejected() {
        for url in [
            "file:///etc/passwd",
            "file://host/etc/passwd",
            "content://com.example.provider/x",
            "intent://scan/#Intent;scheme=zxing;end",
            "javascript:alert(1)",
            "javascript://example.com/%0aalert(1)",
            "mailto:someone@example.com",
            "tel:+15555550100",
            "ftp://example.com/",
            "httpx://example.com/",
            "notascheme://x",
            "://example.com",
        ] {
            assert_eq!(check_link(url), Err(OpenLinkError::BadUrl), "{url}");
        }
    }

    #[test]
    fn an_empty_string_is_rejected() {
        assert_eq!(check_link(""), Err(OpenLinkError::BadUrl));
    }

    #[test]
    fn plugin_messages() {
        assert_eq!(
            OpenLinkError::from_message("no-handler: android.content.ActivityNotFoundException"),
            OpenLinkError::NoHandler
        );
        assert_eq!(
            OpenLinkError::from_message("not-visible: the app is not visible"),
            OpenLinkError::NotVisible
        );
        assert_eq!(
            OpenLinkError::from_message("platform: x"),
            OpenLinkError::Platform("platform: x".into())
        );
    }
}
