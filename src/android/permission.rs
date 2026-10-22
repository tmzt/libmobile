//! Runtime permission requests (`bridge/PermissionPlugin.kt`).
//!
//! * [`request_permissions`] — Rust -> Kotlin: `Activity.requestPermissions`.
//! * [`permission_granted`] — Rust -> Kotlin: `checkSelfPermission`.
//! * [`Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult`]
//!   — Kotlin -> Rust from `onRequestPermissionsResult`, on the UI thread.

use super::jni_util::{opt_string, with_app_class};
use crate::event::PermissionsOutcome;
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JBooleanArray, JObject, JObjectArray, JString};
use jni::{Env, EnvUnowned, jni_sig, jni_str};

const PERMISSION_CLASS: &str = "dev.highbay.libmobile.bridge.PermissionPlugin";

static RESULTS: Mailbox<PermissionsOutcome> = Mailbox::new();

pub(crate) fn take_results() -> Vec<PermissionsOutcome> {
    RESULTS.take()
}

/// Ask the user for `permissions` (full names, e.g.
/// `"android.permission.RECORD_AUDIO"`). The answer arrives as
/// [`MobileEvent::Permissions`](crate::MobileEvent::Permissions). Each one
/// must be declared in the manifest, or it is denied without a dialog.
pub fn request_permissions(app: &AndroidApp, permissions: &[&str]) {
    let result = with_app_class(app, PERMISSION_CLASS, |env, activity, class| {
        let empty = JString::from_str(env, "")?;
        let array = JObjectArray::<JString>::new(env, permissions.len(), &empty)?;
        for (i, name) in permissions.iter().enumerate() {
            let name = JString::from_str(env, name)?;
            array.set_element(env, i, &name)?;
        }
        env.call_static_method(
            class,
            jni_str!("request"),
            jni_sig!("(Landroid/app/Activity;[Ljava/lang/String;)V"),
            &[activity.into(), (&array).into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: permission request could not be made: {err}");
        RESULTS.push(PermissionsOutcome::denied(permissions));
    }
}

/// Whether `permission` is granted now. `false` if the JNI call fails.
pub fn permission_granted(app: &AndroidApp, permission: &str) -> bool {
    let result = with_app_class(app, PERMISSION_CLASS, |env, activity, class| {
        let name = JString::from_str(env, permission)?;
        env.call_static_method(
            class,
            jni_str!("granted"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Z"),
            &[activity.into(), (&name).into()],
        )?
        .z()
    });
    result.unwrap_or_else(|err| {
        log::error!("libmobile: permission check failed: {err}");
        false
    })
}

fn strings(
    env: &mut Env<'_>,
    array: &JObjectArray<'_, JString<'_>>,
) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let len = array.len(env)?;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let item = array.get_element(env, i)?;
        out.push(opt_string(&item).unwrap_or_default());
    }
    Ok(out)
}

/// Kotlin -> Rust:
/// `PermissionPlugin.nativePermissionsResult(String[], String[], boolean[])`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    requested: JObjectArray<'caller, JString<'caller>>,
    permissions: JObjectArray<'caller, JString<'caller>>,
    granted: JBooleanArray<'caller>,
) {
    let outcome = unowned_env.with_env(|env| -> Result<(), jni::errors::Error> {
        let requested = strings(env, &requested)?;
        let names = strings(env, &permissions)?;
        let mut flags = Vec::new();
        if !granted.is_null() {
            flags.resize(granted.len(env)?, false);
            granted.get_region(env, 0, &mut flags)?;
        }
        let outcome = PermissionsOutcome::from_result(requested, names, &flags);
        log::info!("libmobile: permissions {:?}", outcome.permissions);
        RESULTS.push(outcome);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
