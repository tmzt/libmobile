//! **What a notification shows**, shared by the foreground service's
//! notification (feature `foreground-service`) and alerts (feature
//! `notifications`). Platform-free; re-exported from
//! [`service`](crate::service) and [`notify`](crate::notify).

/// Android shows at most this many buttons on a notification.
pub const MAX_ACTIONS: usize = 3;

/// The longest [`NotificationContent::tag`], in bytes.
pub(crate) const MAX_TAG_BYTES: usize = 256;

/// The `Platform` message for a tag over [`MAX_TAG_BYTES`].
pub(crate) const TAG_TOO_LONG: &str = "the notification's tag is longer than 256 bytes";

/// What a notification shows. Tapping its body opens the app and reports
/// [`tag`](Self::tag): `ServiceEvent::Tapped` for the foreground service's
/// notification, `NotifyEvent::Tapped` for an alert.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NotificationContent {
    pub title: String,
    pub text: String,
    /// The app's own buttons, at most [`MAX_ACTIONS`] (Android's limit),
    /// counting the foreground service's built-in controls.
    pub actions: Vec<NotificationAction>,
    /// On the lock screen. Default [`LockScreenVisibility::Private`].
    pub visibility: LockScreenVisibility,
    /// Set on the body's tap: the app gets it back, to know what was tapped
    /// (for example an agent's id). At most 256 bytes. Default `None`.
    pub tag: Option<String>,
}

impl NotificationContent {
    /// A title and text, with no buttons, the default visibility and no tag.
    pub fn new(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            ..Self::default()
        }
    }
}

/// One of the app's own notification buttons. A tap arrives as
/// `ServiceEvent::Action(id)` on the foreground service's notification, and
/// as `NotifyEvent::Action` on an alert.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NotificationAction {
    /// The app's own id, returned on a tap. Not empty; on the foreground
    /// service's notification, also not starting with `libmobile.`
    /// (reserved for the built-in controls).
    pub id: String,
    pub label: String,
    /// Also bring the activity forward on a tap.
    pub opens_app: bool,
}

impl NotificationAction {
    /// A button that does not open the app.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            opens_app: false,
        }
    }
}

/// How much of the notification shows on the lock screen
/// (`Notification.VISIBILITY_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockScreenVisibility {
    /// All of it.
    Public,
    /// Only that there is one, until the device is unlocked.
    #[default]
    Private,
    /// None of it.
    Secret,
}

impl LockScreenVisibility {
    /// `Notification.VISIBILITY_*`.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn code(self) -> i32 {
        match self {
            LockScreenVisibility::Public => 1,
            LockScreenVisibility::Private => 0,
            LockScreenVisibility::Secret => -1,
        }
    }
}

/// Whether `content`'s tag is over [`MAX_TAG_BYTES`].
pub(crate) fn tag_too_long(content: &NotificationContent) -> bool {
    content
        .tag
        .as_ref()
        .is_some_and(|t| t.len() > MAX_TAG_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_no_tag() {
        let content = NotificationContent::new("t", "x");
        assert_eq!(content.tag, None);
        assert!(!tag_too_long(&content));
    }

    #[test]
    fn tags_count_bytes() {
        let mut content = NotificationContent::new("t", "x");
        content.tag = Some("é".repeat(128)); // 256 bytes
        assert!(!tag_too_long(&content));
        content.tag = Some("é".repeat(129));
        assert!(tag_too_long(&content));
    }
}
