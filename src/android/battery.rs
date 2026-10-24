//! Battery optimization and app sleeping (`battery/BatteryPlugin.kt`,
//! feature `battery`).
//!
//! An app exempt from battery optimization is left out of some of Android's
//! background limits, for example App Standby, and on some devices Doze's
//! network limits. On Samsung phones One UI also puts apps to sleep, which
//! is set on Samsung's battery page. What to tell the user is the app's
//! job; on Samsung, for example: "Battery → Unrestricted, and add <app> to
//! Never sleeping apps".
//!
//! Every call is synchronous and may be made from any thread; it reaches
//! the plugin through the Application that android-activity registers with
//! `ndk-context`. The calls that open a page need the app to be visible
//! ([`BatteryError::NotVisible`] otherwise).
//!
//! Android does not report the answer to the dialog, or a change made in
//! settings. Under `system-events`, libmobile reads the exemption again each
//! time the app becomes visible and sends `SystemEvent::BatteryExemption`
//! when it changed.
//!
//! **Google Play** allows [`request_ignore_optimizations`] only for some
//! kinds of app (see its policy on `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`).
//! A build for Play uses [`open_settings`] or [`open_app_details`] instead.
//! Sideloaded apps are not bound by it.

pub use crate::battery::{BatteryError, BatteryState, Page};

use super::jni_util::{opt_string, with_application_class};
use jni::objects::{JClass, JObject, JString};
use jni::{Env, jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.battery.BatteryPlugin";

/// The app's battery settings now.
pub fn state() -> Result<BatteryState, BatteryError> {
    let (flags, manufacturer) = call(|env, context, class| {
        let flags = state_flags(env, context, class)?;
        let manufacturer = env
            .call_static_method(
                class,
                jni_str!("manufacturer"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let manufacturer = env.cast_local::<JString>(manufacturer)?;
        Ok((flags, opt_string(&manufacturer).unwrap_or_default()))
    })?;
    Ok(BatteryState::new(
        flags & 1 != 0,
        flags & 2 != 0,
        manufacturer,
    ))
}

/// `PowerManager.isIgnoringBatteryOptimizations(package)`.
pub fn is_ignoring_optimizations() -> Result<bool, BatteryError> {
    call(|env, context, class| Ok(state_flags(env, context, class)? & 1 != 0))
}

/// Show the system's Allow/Deny dialog
/// (`ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`). Does nothing (`Ok`) if
/// the app is already exempt. [`BatteryError::NotDeclared`] without
/// [`Project::request_ignore_battery_optimizations`](crate::gradle::Project::request_ignore_battery_optimizations).
/// Not for a build for Google Play; see the module documentation.
pub fn request_ignore_optimizations() -> Result<(), BatteryError> {
    status(call_string(jni_str!("requestIgnoreOptimizations"))?)
}

/// Open the system's list of every app's battery optimization
/// (`ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`).
pub fn open_settings() -> Result<Page, BatteryError> {
    status(call_string(jni_str!("openSettings"))?).map(|()| Page::OptimizationList)
}

/// Open this app's settings page; its Battery entry is one tap away.
pub fn open_app_details() -> Result<Page, BatteryError> {
    status(call_string(jni_str!("openAppDetails"))?).map(|()| Page::AppDetails)
}

/// Open the vendor's battery page if one is known and resolves on this
/// device (Samsung's Device Care, where app sleeping is set), otherwise this
/// app's settings page. Returns which opened.
///
/// Samsung's page has no public intent: it is found by component name,
/// which has moved between One UI versions, so this is best-effort.
pub fn open_vendor_battery() -> Result<Page, BatteryError> {
    match call_string(jni_str!("openVendorBattery"))?.as_deref() {
        Some("vendor") => Ok(Page::Vendor),
        Some("app-details") => Ok(Page::AppDetails),
        Some(message) => Err(BatteryError::from_message(message)),
        None => Err(BatteryError::Platform("no answer".into())),
    }
}

/// `BatteryPlugin.stateFlags(Context)`: bit 0 ignoring optimizations, bit 1
/// background restricted.
fn state_flags(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    class: &JClass<'_>,
) -> jni::errors::Result<i32> {
    env.call_static_method(
        class,
        jni_str!("stateFlags"),
        jni_sig!("(Landroid/content/Context;)I"),
        &[context.into()],
    )?
    .i()
}

/// `null` is success; anything else is a `"<kind>: <detail>"` message.
fn status(message: Option<String>) -> Result<(), BatteryError> {
    match message {
        None => Ok(()),
        Some(message) => Err(BatteryError::from_message(&message)),
    }
}

/// `BatteryPlugin.<method>(Context) -> String?`.
fn call_string(method: &'static jni::strings::JNIStr) -> Result<Option<String>, BatteryError> {
    call(|env, context, class| {
        let value = env
            .call_static_method(
                class,
                method,
                jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
                &[context.into()],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let value = env.cast_local::<JString>(value)?;
        Ok(opt_string(&value))
    })
}

/// Run `f` against the plugin class with the Application, catching a thrown
/// exception as its message.
fn call<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> Result<R, BatteryError> {
    with_application_class(PLUGIN_CLASS, |env, context, class| {
        match f(env, context, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                Err(jni::errors::Error::JavaException)
            }
            other => other,
        }
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => BatteryError::Platform(msg),
        other => BatteryError::Platform(other.to_string()),
    })
}

/// Read the exemption again and publish `SystemEvent::BatteryExemption` if
/// it changed, or if it is the first read. Called when the app becomes
/// visible and at the first `subscribe()`.
#[cfg(feature = "system-events")]
pub(crate) fn recheck() {
    use crate::battery::ExemptionTracker;
    use crate::system_events::{SystemEvent, publish};
    use std::sync::Mutex;

    // Held across the read, so two threads cannot publish out of order.
    static LAST: Mutex<ExemptionTracker> = Mutex::new(ExemptionTracker::new());
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    match is_ignoring_optimizations() {
        Ok(now) => {
            if let Some(changed) = last.update(now) {
                let event = SystemEvent::BatteryExemption(changed);
                log::info!("libmobile: system {event:?}");
                publish(event);
            }
        }
        Err(err) => log::error!("libmobile: reading the battery exemption failed: {err}"),
    }
}
