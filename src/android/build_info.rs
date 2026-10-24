//! The device's manufacturer, brand and model (`bridge/BuildInfoPlugin.kt`,
//! core).
//!
//! [`build_info`] may be called from any thread, with no `AndroidApp`: it
//! reaches the plugin through the Application that android-activity
//! registers with `ndk-context`. The values cannot change while the process
//! runs, so the first answer is kept.

pub use crate::build_info::BuildInfo;

use super::jni_util::{opt_string, with_application_class};
use jni::objects::{JObjectArray, JString};
use jni::{jni_sig, jni_str};
use std::sync::OnceLock;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.bridge.BuildInfoPlugin";

/// The device's manufacturer, brand and model: `Build.MANUFACTURER`,
/// `Build.BRAND` and `Build.MODEL`, public constants that need no
/// permission. Any thread; read once and kept.
///
/// Only those three. libmobile never reads the serial, `ANDROID_ID`, the
/// IMEI, the advertising ID, the build fingerprint, or anything else that
/// identifies the unit or its owner.
///
/// `None` only if the JNI call fails, which is logged at warn level (and
/// tried again at the next call).
pub fn build_info() -> Option<BuildInfo> {
    static INFO: OnceLock<BuildInfo> = OnceLock::new();
    if let Some(info) = INFO.get() {
        return Some(info.clone());
    }
    match read() {
        Ok(info) => Some(INFO.get_or_init(|| info).clone()),
        Err(err) => {
            log::warn!("libmobile: the build info: {err}");
            None
        }
    }
}

/// `BuildInfoPlugin.info()`: `[MANUFACTURER, BRAND, MODEL]`.
fn read() -> jni::errors::Result<BuildInfo> {
    with_application_class(PLUGIN_CLASS, |env, _context, class| {
        let called = env.call_static_method(
            class,
            jni_str!("info"),
            jni_sig!("()[Ljava/lang/String;"),
            &[],
        );
        let array = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let array = env.cast_local::<JObjectArray<JString>>(array)?;
        let manufacturer = opt_string(&array.get_element(env, 0)?);
        let brand = opt_string(&array.get_element(env, 1)?);
        let model = opt_string(&array.get_element(env, 2)?);
        Ok(BuildInfo::new(manufacturer, brand, model))
    })
}
