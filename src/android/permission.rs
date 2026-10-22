//! Runtime permission requests (`bridge/PermissionPlugin.kt`).
//!
//! * [`request_permissions`] — Rust -> Kotlin: queue an
//!   `Activity.requestPermissions` call, identified by a [`PermissionRequest`].
//! * [`permission_granted`] — Rust -> Kotlin: `checkSelfPermission`.
//! * [`should_show_rationale`] — Rust -> Kotlin:
//!   `shouldShowRequestPermissionRationale`.
//! * [`Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult`]
//!   — Kotlin -> Rust from `onRequestPermissionsResult`, on the UI thread.

use std::sync::atomic::{AtomicU32, Ordering};

use super::jni_util::{opt_string, with_app_class};
use crate::event::{PermissionRequest, PermissionsOutcome};
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JBooleanArray, JObject, JObjectArray, JString};
use jni::sys::jint;
use jni::{Env, EnvUnowned, jni_sig, jni_str};

const PERMISSION_CLASS: &str = "dev.highbay.libmobile.bridge.PermissionPlugin";

static RESULTS: Mailbox<PermissionsOutcome> = Mailbox::new();

/// The next request's id; 0 is never handed out.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub(crate) fn take_results() -> Vec<PermissionsOutcome> {
    RESULTS.take()
}

/// Ask the user for `permissions` (full names, e.g.
/// `"android.permission.RECORD_AUDIO"`). The answer arrives as
/// [`MobileEvent::Permissions`](crate::MobileEvent::Permissions) carrying the
/// returned id. Requests are made one at a time, in order. Each permission
/// must be declared in the manifest, or it is denied without a dialog.
pub fn request_permissions(app: &AndroidApp, permissions: &[&str]) -> PermissionRequest {
    let request = PermissionRequest(NEXT_ID.fetch_add(1, Ordering::Relaxed));
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
            jni_sig!("(Landroid/app/Activity;I[Ljava/lang/String;)V"),
            &[activity.into(), (request.0 as jint).into(), (&array).into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: permission request could not be made: {err}");
        RESULTS.push(PermissionsOutcome::denied(request, permissions));
    }
    request
}

/// Whether `permission` is granted now. `false` if the JNI call fails.
pub fn permission_granted(app: &AndroidApp, permission: &str) -> bool {
    call_bool(app, jni_str!("granted"), permission).unwrap_or_else(|err| {
        log::error!("libmobile: permission check failed: {err}");
        false
    })
}

/// After a denial, whether asking again shows a dialog (`true`), or Android
/// will deny it without one (`false`: the user chose "Don't allow" twice, so
/// point them to Settings). Also `false` before the first request, and if
/// the JNI call fails.
pub fn should_show_rationale(app: &AndroidApp, permission: &str) -> bool {
    call_bool(app, jni_str!("shouldShowRationale"), permission).unwrap_or_else(|err| {
        log::error!("libmobile: permission rationale check failed: {err}");
        false
    })
}

/// `PermissionPlugin.<method>(Activity, String) -> boolean`.
fn call_bool(
    app: &AndroidApp,
    method: &'static jni::strings::JNIStr,
    permission: &str,
) -> jni::errors::Result<bool> {
    with_app_class(app, PERMISSION_CLASS, |env, activity, class| {
        let name = JString::from_str(env, permission)?;
        env.call_static_method(
            class,
            method,
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Z"),
            &[activity.into(), (&name).into()],
        )?
        .z()
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
/// `PermissionPlugin.nativePermissionsResult(int, String[], String[], boolean[])`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    id: jint,
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
        let request = PermissionRequest(id as u32);
        let outcome = PermissionsOutcome::from_result(request, requested, names, &flags);
        log::info!(
            "libmobile: permissions (request {}) {:?}",
            request.id(),
            outcome.permissions
        );
        RESULTS.push(outcome);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
