//! Photo capture via the system camera app (feature `camera`,
//! `camera/CameraPlugin.kt`).
//!
//! * [`capture_photo`] — Rust -> Kotlin: `startActivityForResult` with
//!   `MediaStore.ACTION_IMAGE_CAPTURE`.
//! * [`Java_dev_highbay_libmobile_camera_CameraPlugin_nativeCaptureResult`] —
//!   Kotlin -> Rust from `CameraPlugin.onActivityResult`, on the UI thread.

use super::jni_util::{opt_string, with_app_class};
use crate::camera::CaptureOutcome;
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JByteArray, JObject, JString};
use jni::{EnvUnowned, jni_sig, jni_str};

const CAMERA_CLASS: &str = "dev.highbay.libmobile.camera.CameraPlugin";

static RESULTS: Mailbox<CaptureOutcome> = Mailbox::new();

pub(crate) fn take_results() -> Vec<CaptureOutcome> {
    RESULTS.take()
}

/// Hand off to the camera app. The result arrives as
/// [`MobileEvent::Capture`](crate::MobileEvent::Capture).
pub fn capture_photo(app: &AndroidApp) {
    let result = with_app_class(app, CAMERA_CLASS, |env, activity, class| {
        env.call_static_method(
            class,
            jni_str!("capture"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[activity.into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: camera capture could not be started: {err}");
        RESULTS.push(CaptureOutcome::Failed(format!("jni: {err}")));
    }
}

/// Kotlin -> Rust: `CameraPlugin.nativeCaptureResult(byte[]?, String?)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_camera_CameraPlugin_nativeCaptureResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    jpeg: JByteArray<'caller>,
    error: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|env| -> Result<(), jni::errors::Error> {
        let bytes = if jpeg.is_null() {
            None
        } else {
            Some(env.convert_byte_array(&jpeg)?)
        };
        let outcome = CaptureOutcome::from_parts(bytes, opt_string(&error));
        match &outcome {
            CaptureOutcome::Jpeg(bytes) => {
                log::info!("libmobile: capture received ({} bytes)", bytes.len())
            }
            CaptureOutcome::Failed(message) => log::warn!("libmobile: capture failed: {message}"),
        }
        RESULTS.push(outcome);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
