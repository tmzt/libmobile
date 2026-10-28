//! Photo capture via the system camera app (feature `camera-intent`,
//! `cameraintent/CameraIntentPlugin.kt`; named `camera` before 0.3.0).
//!
//! * [`capture_photo`] — Rust -> Kotlin: `startActivityForResult` with
//!   `MediaStore.ACTION_IMAGE_CAPTURE`.
//! * [`Java_dev_highbay_libmobile_cameraintent_CameraIntentPlugin_nativeCaptureResult`] —
//!   Kotlin -> Rust from `CameraIntentPlugin.onActivityResult`, on the UI
//!   thread.
//!
//! With the in-process `camera` feature also on, CAMERA is in the manifest,
//! and Android refuses `ACTION_IMAGE_CAPTURE` until it is granted. The
//! hand-off then goes through the camera's permission gate first; a refusal
//! ends it with `CaptureOutcome::Failed("permission_denied")`.

use super::jni_util::{opt_string, with_app_class};
use crate::camera_intent::CaptureOutcome;
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JByteArray, JObject, JString};
use jni::{EnvUnowned, jni_sig, jni_str};

const CAMERA_INTENT_CLASS: &str = "dev.highbay.libmobile.cameraintent.CameraIntentPlugin";

static RESULTS: Mailbox<CaptureOutcome> = Mailbox::new();

pub(crate) fn take_results() -> Vec<CaptureOutcome> {
    RESULTS.take()
}

/// Hand off to the camera app. The result arrives as
/// [`MobileEvent::Capture`](crate::MobileEvent::Capture).
pub fn capture_photo(app: &AndroidApp) {
    #[cfg(feature = "camera")]
    if !super::permission_granted(app, super::camera::CAMERA_PERMISSION) {
        super::camera::gate_intent(app);
        return;
    }
    launch(app);
}

/// A refusal of CAMERA by the gate (`camera` and `camera-intent` together).
#[cfg(feature = "camera")]
pub(crate) fn permission_denied() {
    RESULTS.push(CaptureOutcome::Failed("permission_denied".into()));
}

/// Start the hand-off; any thread.
pub(crate) fn launch(app: &AndroidApp) {
    let result = with_app_class(app, CAMERA_INTENT_CLASS, |env, activity, class| {
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

/// Kotlin -> Rust: `CameraIntentPlugin.nativeCaptureResult(byte[]?, String?)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_cameraintent_CameraIntentPlugin_nativeCaptureResult<
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
