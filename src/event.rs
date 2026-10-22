//! **What the main loop hands the app.**
//!
//! One enum for the NativeActivity lifecycle (from android-activity) and for
//! everything the Kotlin plugins report (insets, intents, text, feature
//! results), so an app handles platform facts in one place. Platform-free:
//! window *handles* are not carried — read `AndroidApp::native_window()` on
//! [`MobileEvent::WindowCreated`].

use crate::insets::Insets;
use crate::intent::IntentData;

/// The answer to one `request_permissions` call, in request order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionsOutcome {
    /// Each permission's full name and whether it is granted.
    pub permissions: Vec<(String, bool)>,
}

impl PermissionsOutcome {
    /// From `onRequestPermissionsResult`: the names and grants it reported,
    /// or, when it reported none (an interrupted request), every `requested`
    /// name denied.
    pub fn from_result(requested: Vec<String>, names: Vec<String>, granted: &[bool]) -> Self {
        let permissions = if names.is_empty() {
            requested.into_iter().map(|name| (name, false)).collect()
        } else {
            names
                .into_iter()
                .enumerate()
                .map(|(i, name)| (name, granted.get(i).copied().unwrap_or(false)))
                .collect()
        };
        Self { permissions }
    }

    /// Every requested name denied: the request could not be made.
    pub fn denied<S: AsRef<str>>(requested: &[S]) -> Self {
        Self::from_result(
            requested.iter().map(|s| s.as_ref().to_owned()).collect(),
            Vec::new(),
            &[],
        )
    }

    /// Whether `permission` was granted in this answer.
    pub fn granted(&self, permission: &str) -> bool {
        self.permissions
            .iter()
            .any(|(name, granted)| name == permission && *granted)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MobileEvent {
    Start,
    Resume,
    Pause,
    Stop,
    /// A native window exists (`InitWindow`). Build or re-attach the surface.
    WindowCreated,
    /// The native window changed size.
    WindowResized,
    /// The native window is going away (`TerminateWindow`). Delivered
    /// **synchronously inside the poll callback**: android-activity returns the
    /// `ANativeWindow` to the platform the moment that callback returns, so a
    /// swapchain must be released while handling this event, not later.
    WindowDestroyed,
    RedrawNeeded,
    FocusChanged(bool),
    /// The configuration changed (density, night mode, orientation...); read
    /// it from `AndroidApp::config()`.
    ConfigChanged,
    LowMemory,
    /// System bars / cutout / IME moved. Only the newest report per tick.
    Insets(Insets),
    /// The launch intent, or a later one via `onNewIntent`.
    Intent(IntentData),
    /// The hidden `InputConnection` field's full text after an IME edit.
    TextChanged(String),
    #[cfg(feature = "google-auth")]
    GoogleLogin(crate::auth::LoginOutcome),
    #[cfg(feature = "camera")]
    Capture(crate::camera::CaptureOutcome),
    /// The user answered a
    /// [`request_permissions`](crate::android::request_permissions) call.
    Permissions(PermissionsOutcome),
    /// The activity is being destroyed; the loop returns after this.
    Destroy,
}

#[cfg(test)]
mod tests {
    use super::PermissionsOutcome;

    #[test]
    fn answers_pair_names_with_grants() {
        let outcome = PermissionsOutcome::from_result(
            vec!["a".into(), "b".into()],
            vec!["a".into(), "b".into()],
            &[true, false],
        );
        assert_eq!(
            outcome.permissions,
            [("a".to_owned(), true), ("b".to_owned(), false)]
        );
        assert!(outcome.granted("a"));
        assert!(!outcome.granted("b"));
        assert!(!outcome.granted("c"));
    }

    #[test]
    fn interrupted_request_denies_every_name() {
        let outcome =
            PermissionsOutcome::from_result(vec!["a".into(), "b".into()], Vec::new(), &[]);
        assert_eq!(
            outcome.permissions,
            [("a".to_owned(), false), ("b".to_owned(), false)]
        );
        assert_eq!(outcome, PermissionsOutcome::denied(&["a", "b"]));
    }

    #[test]
    fn missing_grants_are_denials() {
        let outcome = PermissionsOutcome::from_result(Vec::new(), vec!["a".into()], &[]);
        assert!(!outcome.granted("a"));
    }
}
