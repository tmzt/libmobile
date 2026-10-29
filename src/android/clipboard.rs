//! The clipboard's text, read and written (`clipboard/ClipboardPlugin.kt`,
//! feature `clipboard`).
//!
//! [`read_text`] gives the plain text of the clipboard's first item, for an
//! app's own Paste button; [`has_text`] says whether there is any, for
//! enabling that button. Both may be called from any thread and take the
//! activity, whose window focus Android 10 and later require (see
//! [`ClipboardError::NotFocused`]). [`write_text`] puts plain text on the
//! clipboard, for an app's own Copy button, from any thread, with no
//! permission and no focus rule. libmobile never logs the text or its
//! length.
//!
//! From Android 12 the system shows a "<app> pasted from your clipboard"
//! toast when [`read_text`] reads a clip another app copied. [`has_text`]
//! reads only the clip's description, which shows no toast, so call
//! [`read_text`] only when the user asked to paste.

pub use crate::clipboard::{ClipboardError, Copied};

use super::jni_util::{opt_string, with_app_class};
use crate::clipboard::{copied_from_kind, non_empty};
use android_activity::AndroidApp;
use jni::objects::{JObjectArray, JString};
use jni::{jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.clipboard.ClipboardPlugin";

/// The clipboard's text: the first item of the primary clip, as text
/// (`ClipData.Item.getText()`, or `coerceToText` for a content URI whose
/// clip is `text/*`). `Ok(None)` when the clipboard is empty, its first item
/// has no text (an image, say), or the text is empty. Any thread.
///
/// [`ClipboardError::NotFocused`] while the app's window does not have
/// focus. On Android 12 and later the system shows its "pasted" toast for a
/// clip another app copied, so call it only when the user asked to paste,
/// never to look; use [`has_text`] for that.
pub fn read_text(app: &AndroidApp) -> Result<Option<String>, ClipboardError> {
    let answer = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        let called = env.call_static_method(
            class,
            jni_str!("readText"),
            jni_sig!("(Landroid/app/Activity;)[Ljava/lang/String;"),
            &[activity.into()],
        );
        let array = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let array = env.cast_local::<JObjectArray<JString>>(array)?;
        let kind = opt_string(&array.get_element(env, 0)?).unwrap_or_default();
        let text = opt_string(&array.get_element(env, 1)?);
        Ok((kind, text))
    });
    let (kind, text) = answer.map_err(platform)?;
    match ClipboardError::from_kind(&kind) {
        Some(error) => Err(error),
        None => Ok(non_empty(text)),
    }
}

/// Whether the clipboard holds text: a primary clip whose description has a
/// `text/*` MIME type. Reads the description only, so Android shows no
/// toast; for enabling a Paste button. Any thread.
///
/// [`ClipboardError::NotFocused`] while the app's window does not have
/// focus. A `true` may still give `Ok(None)` from [`read_text`] when the
/// text turns out to be empty.
pub fn has_text(app: &AndroidApp) -> Result<bool, ClipboardError> {
    let answer = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        let called = env.call_static_method(
            class,
            jni_str!("hasText"),
            jni_sig!("(Landroid/app/Activity;)Ljava/lang/String;"),
            &[activity.into()],
        );
        let kind = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let kind = env.cast_local::<JString>(kind)?;
        Ok(opt_string(&kind).unwrap_or_default())
    });
    let kind = answer.map_err(platform)?;
    match ClipboardError::from_kind(&kind) {
        Some(error) => Err(error),
        None => Ok(kind == "yes"),
    }
}

/// Put `text` on the clipboard as plain text (`ClipData.newPlainText`),
/// replacing what was there. Any thread, as [`read_text`]; no permission,
/// and the app's window need not have focus.
///
/// `Ok(Copied { system_notice: true })` on Android 13 and later, where the
/// system shows its own confirmation: the app shows none. On older
/// versions `system_notice` is `false` and the app may show its own.
/// An empty `text` is written as given (it clears the clipboard's text).
/// The clip is labelled with the app's name.
///
/// libmobile never logs the text or its length.
pub fn write_text(app: &AndroidApp, text: &str) -> Result<Copied, ClipboardError> {
    let answer = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        let text = JString::from_str(env, text)?;
        let called = env.call_static_method(
            class,
            jni_str!("writeText"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;"),
            &[activity.into(), (&text).into()],
        );
        let kind = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let kind = env.cast_local::<JString>(kind)?;
        Ok(opt_string(&kind).unwrap_or_default())
    });
    let kind = answer.map_err(platform)?;
    copied_from_kind(&kind)
}

fn platform(err: jni::errors::Error) -> ClipboardError {
    let error = match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => ClipboardError::Platform(msg),
        other => ClipboardError::Platform(other.to_string()),
    };
    log::warn!("libmobile: clipboard: {error}");
    error
}
