//! The system's night mode and the system bars' icons (core).
//!
//! [`night_mode`] reads the configuration android-activity keeps; a change
//! arrives as [`MobileEvent::ConfigChanged`](crate::MobileEvent::ConfigChanged)
//! (the generated manifest's `configChanges` includes `uiMode`, so the
//! activity is not recreated). [`set_system_bar_icons`] goes to
//! `bridge/WindowInsetsPlugin.kt`, which draws the app edge-to-edge.

use super::jni_util::with_application_class;
use android_activity::AndroidApp;
use jni::{jni_sig, jni_str};
use ndk::configuration::UiModeNight;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.bridge.WindowInsetsPlugin";

/// Whether the system is in night (dark) mode: `Configuration.uiMode`'s
/// night bits. `None` when undefined. Read again after
/// `MobileEvent::ConfigChanged` to see a change.
pub fn night_mode(app: &AndroidApp) -> Option<bool> {
    match app.config().ui_mode_night() {
        UiModeNight::Yes => Some(true),
        UiModeNight::No => Some(false),
        _ => None,
    }
}

/// Dark (true) or light icons on the status and navigation bars,
/// against what the app draws behind them. Any thread; applied on the UI
/// thread (WindowInsetsController.setSystemBarsAppearance, API 30, and
/// the older View flags on API 29).
///
/// The first call also turns off the system's contrast scrims
/// (`isStatusBarContrastEnforced`, `isNavigationBarContrastEnforced`) and
/// makes both bars' own colours transparent, so only what the app draws
/// shows behind the bars. The latest icons are kept and applied again to
/// each new window, since a new window resets them. Until the first call,
/// libmobile leaves the bars as the system sets them. A failure is logged.
pub fn set_system_bar_icons(status_dark: bool, navigation_dark: bool) {
    let result = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        let called = env.call_static_method(
            class,
            jni_str!("setSystemBarIcons"),
            jni_sig!("(ZZ)V"),
            &[status_dark.into(), navigation_dark.into()],
        );
        match called {
            Err(jni::errors::Error::JavaException) => env.exception_catch(),
            other => other.map(drop),
        }
    });
    if let Err(err) = result {
        log::error!("libmobile: setting the system bars' icons failed: {err}");
    }
}
