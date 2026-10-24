//! **The foreground service's options and errors** (feature
//! `foreground-service`), platform-free so they build and test on any host.
//! The calls that use them are in `libmobile::android::service`, which
//! re-exports everything here.
//!
//! The structs are `#[non_exhaustive]`, so outside this crate they are
//! built with their `new` or `default` and then changed field by field:
//!
//! ```
//! use libmobile::gradle::ServiceType;
//! use libmobile::service::{Controls, NotificationAction, NotificationContent, ServiceOptions};
//!
//! let mut content = NotificationContent::new("handlr", "connected");
//! content.actions.push(NotificationAction::new("ping", "Ping"));
//! let mut options = ServiceOptions::new(vec![ServiceType::SpecialUse("a link")], content);
//! let mut controls = Controls::default();
//! controls.ongoing = true;
//! controls.exit = true;
//! controls.wake_lock_toggle = true;
//! options.controls = controls;
//! ```

pub use crate::notification::{
    LockScreenVisibility, MAX_ACTIONS, NotificationAction, NotificationContent,
};

use crate::codegen::ServiceType;
use crate::notification::{TAG_TOO_LONG, tag_too_long};
use std::fmt;

/// The id of the built-in Exit button ([`Controls::exit`]).
pub const EXIT_ACTION: &str = "libmobile.exit";
/// The id of the built-in wake-lock button ([`Controls::wake_lock_toggle`]).
pub const WAKE_LOCK_ACTION: &str = "libmobile.wake_lock";

/// How the service starts.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ServiceOptions {
    /// A subset of the types declared with
    /// `gradle::Project::foreground_service`. Only the kind matters here;
    /// a `SpecialUse` reason is read from the manifest.
    pub types: Vec<ServiceType>,
    pub content: NotificationContent,
    pub controls: Controls,
    /// Hold the wake lock from the start.
    pub wake_lock: bool,
}

impl ServiceOptions {
    /// `types` and `content`, with no built-in controls and no wake lock.
    pub fn new(types: Vec<ServiceType>, content: NotificationContent) -> Self {
        Self {
            types,
            content,
            controls: Controls::default(),
            wake_lock: false,
        }
    }

    /// The types as `ServiceInfo.FOREGROUND_SERVICE_TYPE_*` bits.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn type_bits(&self) -> i32 {
        self.types.iter().fold(0, |bits, ty| bits | ty.bits())
    }
}

/// The built-in notification controls, in the manner of Termux. All are off
/// by default, so an app gets only what it asks for.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Controls {
    /// The notification cannot be swiped away. From Android 14 the user can
    /// dismiss it anyway, so libmobile posts it again at once; leaving is
    /// then only through Exit or the app's own `stop()`.
    pub ongoing: bool,
    /// An Exit button: it releases the wake lock, stops the service,
    /// delivers `ServiceEvent::Exited` and finishes the activity's task.
    /// libmobile does not kill the process.
    pub exit: bool,
    /// A button that acquires or releases the wake lock, delivering
    /// `ServiceEvent::WakeLock`.
    pub wake_lock_toggle: bool,
    /// The controls' text.
    pub labels: ControlLabels,
}

impl Controls {
    /// How many buttons the controls add.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn buttons(&self) -> usize {
        usize::from(self.exit) + usize::from(self.wake_lock_toggle)
    }
}

/// The built-in controls' text.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ControlLabels {
    /// Default "Exit".
    pub exit: String,
    /// The wake-lock button while none is held. Default "Acquire wake lock".
    pub acquire_wake_lock: String,
    /// The wake-lock button while one is held. Default "Release wake lock".
    pub release_wake_lock: String,
    /// After the app's text while the wake lock is held. Default
    /// " (wake lock held)".
    pub wake_lock_held: String,
}

impl Default for ControlLabels {
    fn default() -> Self {
        Self {
            exit: "Exit".into(),
            acquire_wake_lock: "Acquire wake lock".into(),
            release_wake_lock: "Release wake lock".into(),
            wake_lock_held: " (wake lock held)".into(),
        }
    }
}

/// Why a service call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The build script did not declare the service, or these types.
    NotDeclared,
    /// Started while the app was not visible.
    StartNotAllowed,
    /// A permission the call needs, for example `RECORD_AUDIO` for
    /// `Microphone`.
    MissingPermission(String),
    /// `update`, `set_wake_lock` or an action while stopped.
    NotRunning,
    /// More than [`MAX_ACTIONS`] buttons with the built-in controls.
    TooManyActions,
    /// An action id that is empty or starts with `libmobile.`.
    BadActionId,
    /// Anything else Android or JNI reported.
    Platform(String),
}

impl ServiceError {
    /// From the plugin's `"<kind>: <detail>"` message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let (kind, detail) = message.split_once(':').unwrap_or((message, ""));
        let detail = detail.trim();
        match kind.trim() {
            "not-declared" => ServiceError::NotDeclared,
            "start-not-allowed" => ServiceError::StartNotAllowed,
            "missing-permission" => ServiceError::MissingPermission(detail.to_string()),
            "not-running" => ServiceError::NotRunning,
            "too-many-actions" => ServiceError::TooManyActions,
            "bad-action-id" => ServiceError::BadActionId,
            _ => ServiceError::Platform(message.to_string()),
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceError::NotDeclared => {
                f.write_str("the foreground service or this type is not declared")
            }
            ServiceError::StartNotAllowed => {
                f.write_str("the service may only be started while the app is visible")
            }
            ServiceError::MissingPermission(name) => write!(f, "permission {name} not granted"),
            ServiceError::NotRunning => f.write_str("the service is not running"),
            ServiceError::TooManyActions => {
                write!(f, "more than {MAX_ACTIONS} notification buttons")
            }
            ServiceError::BadActionId => {
                f.write_str("an action id is empty or starts with `libmobile.`")
            }
            ServiceError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// The checks Rust makes before asking Android: the button count with
/// `builtin` buttons from the controls, the action ids, and the tag's
/// length (a [`ServiceError::Platform`] naming it).
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn check_content(
    content: &NotificationContent,
    builtin: usize,
) -> Result<(), ServiceError> {
    if content
        .actions
        .iter()
        .any(|a| a.id.is_empty() || a.id.starts_with("libmobile."))
    {
        return Err(ServiceError::BadActionId);
    }
    if builtin + content.actions.len() > MAX_ACTIONS {
        return Err(ServiceError::TooManyActions);
    }
    if tag_too_long(content) {
        return Err(ServiceError::Platform(TAG_TOO_LONG.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(ids: &[&str]) -> NotificationContent {
        let mut content = NotificationContent::new("t", "x");
        content.actions = ids
            .iter()
            .map(|id| NotificationAction::new(*id, *id))
            .collect();
        content
    }

    #[test]
    fn at_most_three_buttons_in_all() {
        assert!(check_content(&content(&["a", "b", "c"]), 0).is_ok());
        assert!(check_content(&content(&["a"]), 2).is_ok());
        assert_eq!(
            check_content(&content(&["a", "b"]), 2),
            Err(ServiceError::TooManyActions)
        );
        assert_eq!(
            check_content(&content(&["a", "b", "c", "d"]), 0),
            Err(ServiceError::TooManyActions)
        );
    }

    #[test]
    fn action_ids_are_not_empty_or_reserved() {
        for bad in ["", "libmobile.exit", "libmobile.x"] {
            assert_eq!(
                check_content(&content(&[bad]), 0),
                Err(ServiceError::BadActionId)
            );
        }
        assert!(check_content(&content(&["libmobile"]), 0).is_ok());
    }

    #[test]
    fn tags_are_at_most_256_bytes() {
        let mut c = content(&[]);
        c.tag = Some("a".repeat(256));
        assert!(check_content(&c, 0).is_ok());
        c.tag = Some("a".repeat(257));
        assert_eq!(
            check_content(&c, 0),
            Err(ServiceError::Platform(TAG_TOO_LONG.into()))
        );
    }

    #[test]
    fn controls_are_off_by_default() {
        let controls = Controls::default();
        assert!(!controls.ongoing && !controls.exit && !controls.wake_lock_toggle);
        assert_eq!(controls.buttons(), 0);
        assert_eq!(controls.labels.exit, "Exit");
        assert_eq!(controls.labels.wake_lock_held, " (wake lock held)");
        let options = ServiceOptions::new(vec![], NotificationContent::default());
        assert!(!options.wake_lock);
        assert_eq!(options.content.visibility, LockScreenVisibility::Private);
    }

    #[test]
    fn types_become_bits() {
        let options = ServiceOptions::new(
            vec![ServiceType::SpecialUse("r"), ServiceType::Microphone],
            NotificationContent::default(),
        );
        assert_eq!(options.type_bits(), (1 << 30) | 128);
    }

    #[test]
    fn errors_from_plugin_messages() {
        assert_eq!(
            ServiceError::from_message("not-declared: types 128"),
            ServiceError::NotDeclared
        );
        assert_eq!(
            ServiceError::from_message("start-not-allowed: background"),
            ServiceError::StartNotAllowed
        );
        assert_eq!(
            ServiceError::from_message("missing-permission: android.permission.RECORD_AUDIO"),
            ServiceError::MissingPermission("android.permission.RECORD_AUDIO".into())
        );
        assert_eq!(
            ServiceError::from_message("not-running"),
            ServiceError::NotRunning
        );
        assert_eq!(
            ServiceError::from_message("platform: boom"),
            ServiceError::Platform("platform: boom".into())
        );
    }
}
