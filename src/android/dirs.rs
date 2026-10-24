//! The app's private files and cache directories (`bridge/DirsPlugin.kt`,
//! core).
//!
//! [`files_dir`] and [`cache_dir`] may be called from any thread, with no
//! `AndroidApp`: they reach the plugin through the Application that
//! android-activity registers with `ndk-context`. Android creates both
//! directories before returning them; libmobile creates nothing inside them.

use super::jni_util::{opt_string, with_application_class};
use jni::objects::JString;
use jni::{jni_sig, jni_str};
use std::path::PathBuf;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.bridge.DirsPlugin";

/// `Context.getFilesDir()`: private to the app, and kept until the app is
/// uninstalled or its data is cleared. The same directory as
/// [`AndroidApp::internal_data_path`](android_activity::AndroidApp::internal_data_path),
/// but available on any thread.
///
/// `None` only if the JNI call fails, which is logged at warn level.
pub fn files_dir() -> Option<PathBuf> {
    dir(jni_str!("filesDir"), "files")
}

/// `Context.getCacheDir()`: private to the app. Android may delete its files
/// when storage is low, so keep only what can be made again there. Any
/// thread.
///
/// `None` only if the JNI call fails, which is logged at warn level.
pub fn cache_dir() -> Option<PathBuf> {
    dir(jni_str!("cacheDir"), "cache")
}

fn dir(method: &'static jni::strings::JNIStr, what: &str) -> Option<PathBuf> {
    let path = with_application_class(PLUGIN_CLASS, |env, context, class| {
        let called = env.call_static_method(
            class,
            method,
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[context.into()],
        );
        let path = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let path = env.cast_local::<JString>(path)?;
        Ok(opt_string(&path))
    });
    match path {
        Ok(Some(path)) => Some(PathBuf::from(path)),
        Ok(None) => {
            log::warn!("libmobile: the {what} directory: Android returned none");
            None
        }
        Err(err) => {
            log::warn!("libmobile: the {what} directory: {err}");
            None
        }
    }
}
