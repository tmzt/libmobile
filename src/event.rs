//! **What the main loop hands the app.**
//!
//! One enum for the NativeActivity lifecycle (from android-activity) and for
//! everything the Kotlin plugins report (insets, intents, text, feature
//! results), so an app handles platform facts in one place. Platform-free:
//! window *handles* are not carried — read `AndroidApp::native_window()` on
//! [`MobileEvent::WindowCreated`].

use crate::insets::Insets;
use crate::intent::IntentData;

/// One `request_permissions` call, for matching its answer
/// ([`PermissionsOutcome::request`]). Ids count from 1 per process.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PermissionRequest(pub(crate) u32);

impl PermissionRequest {
    /// The id as a number, for logs and for wrappers with their own id type.
    pub fn id(self) -> u32 {
        self.0
    }
}

/// The answer to one `request_permissions` call, in request order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermissionsOutcome {
    /// The request this answers.
    pub request: PermissionRequest,
    /// Each permission's full name and whether it is granted.
    pub permissions: Vec<(String, bool)>,
}

impl PermissionsOutcome {
    /// From `onRequestPermissionsResult`: the names and grants it reported,
    /// or, when it reported none (an interrupted request), every `requested`
    /// name denied.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
    pub(crate) fn from_result(
        request: PermissionRequest,
        requested: Vec<String>,
        names: Vec<String>,
        granted: &[bool],
    ) -> Self {
        let permissions = if names.is_empty() {
            requested.into_iter().map(|name| (name, false)).collect()
        } else {
            names
                .into_iter()
                .enumerate()
                .map(|(i, name)| (name, granted.get(i).copied().unwrap_or(false)))
                .collect()
        };
        Self {
            request,
            permissions,
        }
    }

    /// Every requested name denied: the request could not be made.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
    pub(crate) fn denied<S: AsRef<str>>(request: PermissionRequest, requested: &[S]) -> Self {
        Self::from_result(
            request,
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

/// How much memory Android asks the app to give back
/// (`ComponentCallbacks2.onTrimMemory`), lowest pressure first, so levels
/// compare: `level >= TrimLevel::Background`.
///
/// From API 34 Android delivers only [`UiHidden`](TrimLevel::UiHidden) and
/// [`Background`](TrimLevel::Background).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrimLevel {
    /// `TRIM_MEMORY_RUNNING_MODERATE` (5): running, the device is beginning
    /// to run low on memory.
    RunningModerate,
    /// `TRIM_MEMORY_RUNNING_LOW` (10): running, the device is running much
    /// lower on memory.
    RunningLow,
    /// `TRIM_MEMORY_RUNNING_CRITICAL` (15): running, the device is very low
    /// on memory and background processes are being killed.
    RunningCritical,
    /// `TRIM_MEMORY_UI_HIDDEN` (20): the app's UI is no longer visible.
    UiHidden,
    /// `TRIM_MEMORY_BACKGROUND` (40): the process is on the list of
    /// background processes to kill.
    Background,
    /// `TRIM_MEMORY_MODERATE` (60): the process is near the middle of that
    /// list.
    Moderate,
    /// `TRIM_MEMORY_COMPLETE` (80): the process is among the first to be
    /// killed.
    Complete,
}

impl TrimLevel {
    /// From the `onTrimMemory` level. A level Android adds later maps to the
    /// nearest lower known one (30 is `UiHidden`), so it still arrives; below
    /// 5 there is none lower, and it is `RunningModerate`.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_level(level: i32) -> Self {
        match level {
            80.. => TrimLevel::Complete,
            60.. => TrimLevel::Moderate,
            40.. => TrimLevel::Background,
            20.. => TrimLevel::UiHidden,
            15.. => TrimLevel::RunningCritical,
            10.. => TrimLevel::RunningLow,
            _ => TrimLevel::RunningModerate,
        }
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
    /// Android asked the app to give back memory
    /// (`ComponentCallbacks2.onTrimMemory`). Also delivered to system-event
    /// subscribers under `system-events`.
    TrimMemory(TrimLevel),
    /// System bars / cutout / IME moved. Only the newest report per tick.
    Insets(Insets),
    /// The launch intent, or a later one via `onNewIntent`. A tap on a
    /// notification libmobile posted arrives as `Service` or `Notify`
    /// instead.
    Intent(IntentData),
    /// The hidden `InputConnection` field's full text after an IME edit.
    TextChanged(String),
    #[cfg(feature = "google-auth")]
    GoogleLogin(crate::auth::LoginOutcome),
    /// The system camera app's photo (`camera-intent`).
    #[cfg(feature = "camera-intent")]
    Capture(crate::camera_intent::CaptureOutcome),
    /// The in-process camera: sessions, zoom and captures (`camera`).
    #[cfg(feature = "camera")]
    Camera(crate::camera::CameraEvent),
    /// The foreground service did something, or its notification was
    /// tapped (`android::service`). Also delivered to system-event
    /// subscribers under `system-events`.
    #[cfg(feature = "foreground-service")]
    Service(crate::system_events::ServiceEvent),
    /// An alert was tapped (`android::notify`). Also delivered to
    /// system-event subscribers under `system-events`.
    #[cfg(feature = "notifications")]
    Notify(crate::notify::NotifyEvent),
    /// The user answered a
    /// [`request_permissions`](crate::android::request_permissions) call.
    Permissions(PermissionsOutcome),
    /// The activity is being destroyed; the loop returns after this.
    Destroy,
}

#[cfg(test)]
mod tests {
    use super::{PermissionRequest, PermissionsOutcome, TrimLevel};

    #[test]
    fn known_trim_levels_map_to_themselves() {
        for (level, expected) in [
            (5, TrimLevel::RunningModerate),
            (10, TrimLevel::RunningLow),
            (15, TrimLevel::RunningCritical),
            (20, TrimLevel::UiHidden),
            (40, TrimLevel::Background),
            (60, TrimLevel::Moderate),
            (80, TrimLevel::Complete),
        ] {
            assert_eq!(TrimLevel::from_level(level), expected, "level {level}");
        }
    }

    #[test]
    fn unknown_trim_levels_map_to_the_nearest_lower() {
        for (level, expected) in [
            (6, TrimLevel::RunningModerate),
            (9, TrimLevel::RunningModerate),
            (11, TrimLevel::RunningLow),
            (14, TrimLevel::RunningLow),
            (19, TrimLevel::RunningCritical),
            (30, TrimLevel::UiHidden),
            (39, TrimLevel::UiHidden),
            (41, TrimLevel::Background),
            (59, TrimLevel::Background),
            (79, TrimLevel::Moderate),
        ] {
            assert_eq!(TrimLevel::from_level(level), expected, "level {level}");
        }
    }

    #[test]
    fn trim_levels_below_5_are_running_moderate() {
        for level in [i32::MIN, -1, 0, 1, 4] {
            assert_eq!(TrimLevel::from_level(level), TrimLevel::RunningModerate);
        }
    }

    #[test]
    fn trim_levels_above_80_are_complete() {
        for level in [81, 100, i32::MAX] {
            assert_eq!(TrimLevel::from_level(level), TrimLevel::Complete);
        }
    }

    #[test]
    fn trim_levels_order_by_pressure() {
        assert!(TrimLevel::RunningCritical < TrimLevel::UiHidden);
        assert!(TrimLevel::Background < TrimLevel::Complete);
    }

    const REQUEST: PermissionRequest = PermissionRequest(7);

    #[test]
    fn answers_pair_names_with_grants() {
        let outcome = PermissionsOutcome::from_result(
            REQUEST,
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
        assert_eq!(outcome.request.id(), 7);
    }

    #[test]
    fn interrupted_request_denies_every_name() {
        let outcome =
            PermissionsOutcome::from_result(REQUEST, vec!["a".into(), "b".into()], Vec::new(), &[]);
        assert_eq!(
            outcome.permissions,
            [("a".to_owned(), false), ("b".to_owned(), false)]
        );
        assert_eq!(outcome, PermissionsOutcome::denied(REQUEST, &["a", "b"]));
    }

    #[test]
    fn missing_grants_are_denials() {
        let outcome = PermissionsOutcome::from_result(REQUEST, Vec::new(), vec!["a".into()], &[]);
        assert!(!outcome.granted("a"));
    }
}
