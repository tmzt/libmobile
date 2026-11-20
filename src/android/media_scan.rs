//! Telling MediaStore about changed files (`media_scan/MediaScanPlugin.kt`,
//! feature `media-scan`), through `MediaScannerConnection.scanFile`. No
//! permission.

pub use crate::media_scan::{MAX_SCAN_PATHS, MediaScanError};

use super::jni_util::{opt_string, string_array, with_application_class};
use jni::objects::JString;
use jni::{jni_sig, jni_str};
use std::path::Path;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.media_scan.MediaScanPlugin";

/// Ask MediaStore to scan these paths: new and changed files are added
/// or updated, and paths that no longer exist are removed from its index.
/// Returns once the request is queued; the scan finishes later, on the
/// system's own thread, and its result is only logged at debug. Any thread.
/// An empty list does nothing.
///
/// Each path must be absolute and valid UTF-8, and there may be at most
/// [`MAX_SCAN_PATHS`]; both are checked before any JNI call.
pub fn scan_paths(paths: &[&Path]) -> Result<(), MediaScanError> {
    let paths = crate::media_scan::checked_paths(paths)?;
    if paths.is_empty() {
        return Ok(());
    }
    let answer = with_application_class(PLUGIN_CLASS, |env, context, class| {
        let array = string_array(env, &paths)?;
        let result = env.call_static_method(
            class,
            jni_str!("scan"),
            jni_sig!("(Landroid/content/Context;[Ljava/lang/String;)Ljava/lang/String;"),
            &[context.into(), (&array).into()],
        );
        let value = match result {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let value = env.cast_local::<JString>(value)?;
        Ok(opt_string(&value))
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => MediaScanError::Platform(msg),
        other => MediaScanError::Platform(other.to_string()),
    })?;
    match answer {
        None => {
            log::debug!("libmobile: media scan of {} path(s) queued", paths.len());
            Ok(())
        }
        Some(message) => Err(MediaScanError::Platform(message)),
    }
}
