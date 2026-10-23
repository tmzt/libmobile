//! The battery-optimization exemption (feature `foreground-service`).
//!
//! An app exempt from battery optimization is left out of some of Android's
//! background limits, for example App Standby, and on some devices Doze's
//! network limits. Whether a foreground service needs it varies by version
//! and vendor; check on the devices that matter.
//!
//! Every call is synchronous and may be made from any thread. The answer to
//! a request is not reported: read it back with [`is_ignoring_optimizations`]
//! when the app is visible again.

use super::service::{ServiceError, call, call_status};
use jni::{jni_sig, jni_str};

/// `PowerManager.isIgnoringBatteryOptimizations(package)`.
pub fn is_ignoring_optimizations() -> Result<bool, ServiceError> {
    call(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("isIgnoringOptimizations"),
            jni_sig!("(Landroid/content/Context;)Z"),
            &[context.into()],
        )?
        .z()
    })
}

/// Show the system's Allow/Deny dialog
/// (`ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`). While the app is visible
/// only ([`ServiceError::StartNotAllowed`] otherwise), and
/// [`ServiceError::NotDeclared`] without
/// [`Project::request_ignore_battery_optimizations`](crate::gradle::Project::request_ignore_battery_optimizations).
///
/// Google Play allows asking directly only for some kinds of app (see its
/// policy on `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`). A build for Play uses
/// [`open_settings`] instead.
pub fn request_ignore_optimizations() -> Result<(), ServiceError> {
    call_status(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("requestIgnoreOptimizations"),
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[context.into()],
        )?
        .l()
    })
}

/// Open the system's list of apps and their battery optimization
/// (`ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`), where the user changes
/// it. While the app is visible only.
pub fn open_settings() -> Result<(), ServiceError> {
    call_status(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("openBatterySettings"),
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[context.into()],
        )?
        .l()
    })
}
