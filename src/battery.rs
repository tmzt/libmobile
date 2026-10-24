//! **Battery optimization and app sleeping** (feature `battery`): the
//! platform-free types. The calls are in `android::battery`.
//!
//! An app exempt from battery optimization is left out of some of Android's
//! background limits (App Standby, and on some devices Doze's network
//! limits). On Samsung phones, One UI also puts apps to sleep, which is set
//! on Samsung's own battery pages.

use std::fmt;

/// The app's battery settings, as `android::battery::state` reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatteryState {
    /// `PowerManager.isIgnoringBatteryOptimizations(package)`.
    pub ignoring_optimizations: bool,
    /// `ActivityManager.isBackgroundRestricted()`: the "Restricted" battery
    /// setting, which stops background work even with a service.
    pub background_restricted: bool,
    /// `Build.MANUFACTURER`, as reported ("samsung" on the S23).
    pub manufacturer: String,
}

impl BatteryState {
    /// From the plugin's answers.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn new(
        ignoring_optimizations: bool,
        background_restricted: bool,
        manufacturer: String,
    ) -> Self {
        Self {
            ignoring_optimizations,
            background_restricted,
            manufacturer,
        }
    }

    /// The manufacturer is Samsung, so app sleeping applies.
    pub fn is_samsung(&self) -> bool {
        self.manufacturer.trim().eq_ignore_ascii_case("samsung")
    }
}

/// Which page opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Page {
    /// `ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`: every app's setting.
    OptimizationList,
    /// `ACTION_APPLICATION_DETAILS_SETTINGS` for this app.
    AppDetails,
    /// The vendor's battery page (Samsung: app sleeping).
    Vendor,
}

/// Why a battery call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatteryError {
    /// `request_ignore_optimizations` without
    /// `gradle::Project::request_ignore_battery_optimizations`.
    NotDeclared,
    /// A page was asked for while the app was not visible.
    NotVisible,
    /// Anything else Android or JNI reported.
    Platform(String),
}

impl BatteryError {
    /// From the plugin's `"<kind>: <detail>"` message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let kind = message.split_once(':').map_or(message, |(kind, _)| kind);
        match kind.trim() {
            "not-declared" => BatteryError::NotDeclared,
            "not-visible" => BatteryError::NotVisible,
            _ => BatteryError::Platform(message.to_string()),
        }
    }
}

impl fmt::Display for BatteryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatteryError::NotDeclared => {
                f.write_str("REQUEST_IGNORE_BATTERY_OPTIMIZATIONS is not declared")
            }
            BatteryError::NotVisible => {
                f.write_str("a settings page may only be opened while the app is visible")
            }
            BatteryError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for BatteryError {}

/// The last exemption value read, for `SystemEvent::BatteryExemption`: an
/// event only when it changes, and always for the first value.
#[cfg(feature = "system-events")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
#[derive(Debug, Default)]
pub(crate) struct ExemptionTracker {
    last: Option<bool>,
}

#[cfg(feature = "system-events")]
impl ExemptionTracker {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) const fn new() -> Self {
        Self { last: None }
    }

    /// `Some(now)` when `now` differs from the last value read, or is the
    /// first.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn update(&mut self, now: bool) -> Option<bool> {
        (self.last.replace(now) != Some(now)).then_some(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samsung_by_manufacturer() {
        let state = |m: &str| BatteryState::new(false, false, m.into());
        assert!(state("samsung").is_samsung());
        assert!(state("Samsung").is_samsung());
        assert!(state(" SAMSUNG ").is_samsung());
        assert!(!state("Google").is_samsung());
        assert!(!state("").is_samsung());
        assert!(!state("samsungx").is_samsung());
    }

    #[test]
    fn errors_from_messages() {
        assert_eq!(
            BatteryError::from_message("not-declared: android.permission.X"),
            BatteryError::NotDeclared
        );
        assert_eq!(
            BatteryError::from_message("not-visible: the app is not visible"),
            BatteryError::NotVisible
        );
        assert_eq!(
            BatteryError::from_message("platform: boom"),
            BatteryError::Platform("platform: boom".into())
        );
        assert_eq!(
            BatteryError::from_message("odd"),
            BatteryError::Platform("odd".into())
        );
    }

    #[cfg(feature = "system-events")]
    #[test]
    fn exemption_reported_on_first_read_and_on_change() {
        let mut tracker = ExemptionTracker::new();
        assert_eq!(tracker.update(false), Some(false));
        assert_eq!(tracker.update(false), None);
        assert_eq!(tracker.update(true), Some(true));
        assert_eq!(tracker.update(true), None);
        assert_eq!(tracker.update(false), Some(false));

        let mut tracker = ExemptionTracker::default();
        assert_eq!(tracker.update(true), Some(true));
    }
}
