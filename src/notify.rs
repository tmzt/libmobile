//! **Alert notifications' channels, errors and events** (feature
//! `notifications`), platform-free so they build and test on any host. The
//! calls that use them are in `libmobile::android::notify`, which
//! re-exports everything here.
//!
//! ```
//! use libmobile::notify::{Channel, Importance, NotificationAction, NotificationContent};
//!
//! let channel = Channel::new("questions", "Questions", Importance::High);
//! let mut content = NotificationContent::new("agent 3", "needs an approval");
//! content.tag = Some("agent-3".into());
//! content.actions.push(NotificationAction::new("approve", "Approve"));
//! ```

pub use crate::notification::{
    LockScreenVisibility, MAX_ACTIONS, NotificationAction, NotificationContent,
};

use crate::notification::tag_too_long;
use std::fmt;

/// The foreground service's channel, which an alert may not use.
const SERVICE_CHANNEL: &str = "libmobile.foreground";

/// The longest alert or channel id.
const MAX_ID: usize = 64;

/// A notification channel of the app's own, created on its first post.
///
/// Android keeps a channel's importance and sound once it is created, and
/// the user may change them in the system's settings: changing
/// [`importance`](Self::importance) later has no effect on an existing
/// channel, so a different importance needs a new channel id.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Channel {
    /// Stable id, 1 to 64 of `[a-z0-9._-]`; not `libmobile.foreground`.
    pub id: String,
    /// Shown in the system's settings.
    pub name: String,
    pub importance: Importance,
}

impl Channel {
    pub fn new(id: impl Into<String>, name: impl Into<String>, importance: Importance) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            importance,
        }
    }
}

/// How an alert on the channel interrupts (`NotificationManager.IMPORTANCE_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Importance {
    /// No sound.
    Low,
    /// Sound, no heads-up.
    Default,
    /// Sound and a heads-up.
    High,
}

impl Importance {
    /// `NotificationManager.IMPORTANCE_*`.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn code(self) -> i32 {
        match self {
            Importance::Low => 2,
            Importance::Default => 3,
            Importance::High => 4,
        }
    }
}

/// Why an alert call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyError {
    /// An alert id that is not 1 to 64 of `[A-Za-z0-9._-]`, a channel id
    /// that is not 1 to 64 of `[a-z0-9._-]` or is the foreground service's,
    /// or an empty button id.
    BadId,
    /// Notifications are off for the app (on Android 13+,
    /// `POST_NOTIFICATIONS` not granted). The app asks for it itself.
    PermissionDenied,
    /// More than [`MAX_ACTIONS`] buttons.
    TooManyActions,
    /// Anything else Android or JNI reported, or a tag over 256 bytes.
    Platform(String),
}

impl NotifyError {
    /// From the plugin's `"<kind>: <detail>"` message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let (kind, _) = message.split_once(':').unwrap_or((message, ""));
        match kind.trim() {
            "bad-id" => NotifyError::BadId,
            "permission-denied" => NotifyError::PermissionDenied,
            "too-many-actions" => NotifyError::TooManyActions,
            _ => NotifyError::Platform(message.to_string()),
        }
    }
}

impl fmt::Display for NotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotifyError::BadId => f.write_str("an alert, channel or button id is not allowed"),
            NotifyError::PermissionDenied => f.write_str("notifications are off for the app"),
            NotifyError::TooManyActions => {
                write!(f, "more than {MAX_ACTIONS} notification buttons")
            }
            NotifyError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for NotifyError {}

/// A tap on an alert. Delivered as `MobileEvent::Notify` on the main loop
/// and, with `system-events`, as `SystemEvent::Notify` to subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyEvent {
    /// The alert's body: the app was opened, and the alert removed.
    Tapped { id: String, tag: Option<String> },
    /// One of the alert's buttons, by its id. The alert stays until the app
    /// cancels it.
    Action { id: String, action: String },
}

/// 1 to 64 bytes, each one `allowed`.
fn id_ok(id: &str, allowed: impl Fn(u8) -> bool) -> bool {
    (1..=MAX_ID).contains(&id.len()) && id.bytes().all(allowed)
}

/// An alert id: 1 to 64 of `[A-Za-z0-9._-]`.
pub(crate) fn alert_id_ok(id: &str) -> bool {
    id_ok(id, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
    })
}

/// A channel id: 1 to 64 of `[a-z0-9._-]`, not the service's.
fn channel_id_ok(id: &str) -> bool {
    id != SERVICE_CHANNEL
        && id_ok(id, |b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// The checks Rust makes before asking Android.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn check_post(
    id: &str,
    content: &NotificationContent,
    channel: &Channel,
) -> Result<(), NotifyError> {
    if !alert_id_ok(id)
        || !channel_id_ok(&channel.id)
        || content.actions.iter().any(|a| a.id.is_empty())
    {
        return Err(NotifyError::BadId);
    }
    if content.actions.len() > MAX_ACTIONS {
        return Err(NotifyError::TooManyActions);
    }
    if tag_too_long(content) {
        return Err(NotifyError::Platform(
            crate::notification::TAG_TOO_LONG.into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: &str) -> Channel {
        Channel::new(id, "Name", Importance::High)
    }

    fn content(actions: &[&str]) -> NotificationContent {
        let mut content = NotificationContent::new("t", "x");
        content.actions = actions
            .iter()
            .map(|id| NotificationAction::new(*id, *id))
            .collect();
        content
    }

    #[test]
    fn alert_ids() {
        for good in ["a", "Agent-3", "a.b_c-D9", &"x".repeat(64)] {
            assert!(alert_id_ok(good), "{good}");
        }
        for bad in ["", "a b", "a/b", "é", &"x".repeat(65)] {
            assert!(!alert_id_ok(bad), "{bad}");
        }
    }

    #[test]
    fn channel_ids() {
        for good in ["questions", "a.b_c-9", &"x".repeat(64)] {
            assert!(channel_id_ok(good), "{good}");
        }
        for bad in [
            "",
            "Questions",
            "a b",
            "libmobile.foreground",
            &"x".repeat(65),
        ] {
            assert!(!channel_id_ok(bad), "{bad}");
        }
    }

    #[test]
    fn post_checks() {
        let ok = channel("questions");
        assert!(check_post("a", &content(&["x", "y", "z"]), &ok).is_ok());
        assert_eq!(
            check_post("a b", &content(&[]), &ok),
            Err(NotifyError::BadId)
        );
        assert_eq!(
            check_post("a", &content(&[]), &channel("libmobile.foreground")),
            Err(NotifyError::BadId)
        );
        assert_eq!(
            check_post("a", &content(&[""]), &ok),
            Err(NotifyError::BadId)
        );
        // Alerts have no built-in controls: `libmobile.` is allowed.
        assert!(check_post("a", &content(&["libmobile.x"]), &ok).is_ok());
        assert_eq!(
            check_post("a", &content(&["w", "x", "y", "z"]), &ok),
            Err(NotifyError::TooManyActions)
        );
        let mut tagged = content(&[]);
        tagged.tag = Some("t".repeat(257));
        assert!(matches!(
            check_post("a", &tagged, &ok),
            Err(NotifyError::Platform(_))
        ));
    }

    #[test]
    fn importance_codes() {
        assert_eq!(Importance::Low.code(), 2);
        assert_eq!(Importance::Default.code(), 3);
        assert_eq!(Importance::High.code(), 4);
    }

    #[test]
    fn errors_from_plugin_messages() {
        assert_eq!(NotifyError::from_message("bad-id: x"), NotifyError::BadId);
        assert_eq!(
            NotifyError::from_message("permission-denied"),
            NotifyError::PermissionDenied
        );
        assert_eq!(
            NotifyError::from_message("too-many-actions"),
            NotifyError::TooManyActions
        );
        assert_eq!(
            NotifyError::from_message("platform: boom"),
            NotifyError::Platform("platform: boom".into())
        );
    }
}
