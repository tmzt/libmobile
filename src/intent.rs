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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn zip_extras_keeps_common_prefix() {
        let extras = IntentData::zip_extras(vec!["a".into(), "b".into()], vec!["1".into()]);
        assert_eq!(extras, vec![("a".to_string(), "1".to_string())]);
    }
}
