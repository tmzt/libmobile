//! **Intents, as plain data.**
//!
//! Stock `android.app.NativeActivity` never overrides `onNewIntent`, so an
//! intent aimed at an already-running instance is swallowed before native code
//! sees it. `IntentPlugin` forwards both legs — the launch intent from
//! `onCreate` and every later one from `onNewIntent` — as an [`IntentData`].
//! What any of it *means* is the app's decision.
//!
//! `adb shell am start -n <pkg>/<activity> --es cmd refresh` arrives as an
//! `IntentData` whose `extra("cmd")` is `Some("refresh")`.

/// The parts of an `android.content.Intent` worth carrying across JNI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntentData {
    /// `Intent.getAction()`.
    pub action: Option<String>,
    /// `Intent.getDataString()` — the URI of a deep link.
    pub data: Option<String>,
    /// Every extra, stringified with `toString()` (so `--ei n 3` arrives as
    /// `"3"`). Bundle iteration order, which Android does not define.
    pub extras: Vec<(String, String)>,
    /// `false` for the intent the activity was created with, `true` for one
    /// delivered to an already-running instance through `onNewIntent`.
    pub is_new: bool,
}

impl IntentData {
    /// The value of extra `key`, if present.
    pub fn extra(&self, key: &str) -> Option<&str> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Pair up the parallel key/value arrays the Kotlin bridge sends. A
    /// length mismatch (impossible from `IntentPlugin`) keeps the common
    /// prefix rather than failing the whole intent.
    pub fn zip_extras(keys: Vec<String>, values: Vec<String>) -> Vec<(String, String)> {
        keys.into_iter().zip(values).collect()
    }
}

/// The extras a notification's tap intent carries (`bridge/IntentPlugin.kt`
/// and the plugins that post notifications): the notification's id, its
/// kind, the tag and, for an alert's button that opens the app, the button.
#[cfg_attr(not(test), allow(dead_code))] // Named in tests against the Kotlin sources.
pub(crate) mod tap_extras {
    pub const NOTIFICATION: &str = "libmobile.notification";
    pub const TAG: &str = "libmobile.tag";
    pub const KIND: &str = "libmobile.kind";
    pub const ACTION: &str = "libmobile.action";
    /// [`KIND`] of the foreground service's notification.
    pub const SERVICE: &str = "service";
    /// [`KIND`] of an alert.
    pub const ALERT: &str = "alert";
}

/// A tap on a notification, from its intent's extras.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NotificationTap {
    /// The foreground service's notification's body.
    Service { tag: Option<String> },
    /// An alert's body.
    Alert { id: String, tag: Option<String> },
    /// An alert's button that opens the app.
    AlertAction { id: String, action: String },
}

impl NotificationTap {
    /// From the extras' values; `None` for a kind libmobile did not post.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_extras(
        kind: Option<String>,
        id: String,
        tag: Option<String>,
        action: Option<String>,
    ) -> Option<Self> {
        match (kind.as_deref(), action) {
            (Some(tap_extras::SERVICE), _) => Some(NotificationTap::Service { tag }),
            (Some(tap_extras::ALERT), Some(action)) => {
                Some(NotificationTap::AlertAction { id, action })
            }
            (Some(tap_extras::ALERT), None) => Some(NotificationTap::Alert { id, tag }),
            _ => None,
        }
    }
}

/// The scheme, host, port and path of `url`, followed by `<redacted>` if
/// a query or fragment was cut off. A URL's query or fragment may hold a
/// secret (a pairing token), so this is what libmobile logs.
pub fn redact_url(url: &str) -> String {
    match url.find(['?', '#']) {
        Some(end) => format!("{}<redacted>", &url[..end]),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_cuts_query_and_fragment() {
        for (url, redacted) in [
            (
                "handlr://h.ts.net:7347/pair#handlr1abc",
                "handlr://h.ts.net:7347/pair<redacted>",
            ),
            ("a://h/p?k=v", "a://h/p<redacted>"),
            ("a://h/p?k=v#f", "a://h/p<redacted>"),
            ("a://h/p", "a://h/p"),
            ("", ""),
            ("not a url", "not a url"),
        ] {
            assert_eq!(redact_url(url), redacted, "{url}");
        }
    }

    #[test]
    fn extra_lookup() {
        let intent = IntentData {
            extras: IntentData::zip_extras(
                vec!["cmd".into(), "n".into()],
                vec!["refresh".into(), "3".into()],
            ),
            ..Default::default()
        };
        assert_eq!(intent.extra("cmd"), Some("refresh"));
        assert_eq!(intent.extra("n"), Some("3"));
        assert_eq!(intent.extra("missing"), None);
    }

    #[test]
    fn notification_taps_from_extras() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            NotificationTap::from_extras(s("service"), "libmobile.foreground".into(), s("t"), None),
            Some(NotificationTap::Service { tag: s("t") })
        );
        assert_eq!(
            NotificationTap::from_extras(s("alert"), "a".into(), None, None),
            Some(NotificationTap::Alert {
                id: "a".into(),
                tag: None
            })
        );
        assert_eq!(
            NotificationTap::from_extras(s("alert"), "a".into(), s("t"), s("open")),
            Some(NotificationTap::AlertAction {
                id: "a".into(),
                action: "open".into()
            })
        );
        assert_eq!(
            NotificationTap::from_extras(None, "a".into(), None, None),
            None
        );
        assert_eq!(
            NotificationTap::from_extras(s("other"), "a".into(), None, None),
            None
        );
    }

    #[test]
    fn tap_extras_match_the_kotlin_sources() {
        let root = env!("CARGO_MANIFEST_DIR");
        let read = |path: &str| {
            std::fs::read_to_string(format!("{root}/android/java/dev/highbay/libmobile/{path}"))
                .unwrap()
        };
        let quoted = |s: &str| format!("\"{s}\"");
        let intent = read("bridge/IntentPlugin.kt");
        for extra in [
            tap_extras::NOTIFICATION,
            tap_extras::TAG,
            tap_extras::KIND,
            tap_extras::ACTION,
        ] {
            assert!(intent.contains(&quoted(extra)), "IntentPlugin: {extra}");
        }
        let service = read("service/ForegroundServicePlugin.kt");
        let notify = read("notify/NotifyPlugin.kt");
        for (source, kind) in [
            (&service, tap_extras::SERVICE),
            (&notify, tap_extras::ALERT),
        ] {
            for extra in [tap_extras::NOTIFICATION, tap_extras::TAG, tap_extras::KIND] {
                assert!(source.contains(&quoted(extra)), "{kind}: {extra}");
            }
            assert!(source.contains(&quoted(kind)), "{kind}");
        }
        assert!(notify.contains(&quoted(tap_extras::ACTION)));
    }

    #[test]
    fn zip_extras_keeps_common_prefix() {
        let extras = IntentData::zip_extras(vec!["a".into(), "b".into()], vec!["1".into()]);
        assert_eq!(extras, vec![("a".to_string(), "1".to_string())]);
    }
}
