//! Opening a link in another app, and this app's page in Settings
//! (`bridge/LinkPlugin.kt`, core).
//!
//! [`open_link`] asks Android to open an http or https URL in the app that
//! handles it, normally the browser. It is synchronous, may be called from
//! any thread, and returns once the intent is sent; it does not wait for the
//! browser. It reaches the plugin through the Application that
//! android-activity registers with `ndk-context`, so no `AndroidApp` is
//! needed. A call on the UI thread starts the link from the activity; one
//! from any other thread starts it from the Application, in a new task, so
//! the call never waits for the UI thread.
//!
//! Android lets an app start another app's activity only while it is
//! visible ([`OpenLinkError::NotVisible`] otherwise). libmobile logs
//! `libmobile: open link <url>` at debug level, cut after the path
//! ([`redact_url`](crate::intent::redact_url)), so a query or fragment
//! never appears in the log.

pub use crate::link::{OpenLinkError, check_link};

use super::jni_util::{opt_string, with_app_class, with_application_class};
use android_activity::AndroidApp;
use jni::JValueOwned;
use jni::objects::JString;
use jni::{Env, jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.bridge.LinkPlugin";

/// Open `url` (http or https) in the app that handles it. Any thread.
///
/// [`OpenLinkError::BadUrl`] when [`check_link`] fails, before anything is
/// sent; [`OpenLinkError::NoHandler`] when no app on the device takes it;
/// [`OpenLinkError::NotVisible`] while the app is hidden.
pub fn open_link(url: &str) -> Result<(), OpenLinkError> {
    check_link(url)?;
    log::debug!("libmobile: open link {}", crate::intent::redact_url(url));
    let message = with_application_class(PLUGIN_CLASS, |env, context, class| {
        let url = JString::from_str(env, url)?;
        let called = env.call_static_method(
            class,
            jni_str!("open"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;"),
            &[context.into(), (&url).into()],
        );
        status_message(env, called)
    });
    answer(message)
}

/// Open this app's page in Settings (`ACTION_APPLICATION_DETAILS_SETTINGS`
/// with `package:<own package>`), where the user can grant a permission they
/// refused for good: for example after
/// [`CameraError::PermissionDenied`](crate::camera::CameraError::PermissionDenied)
/// with `settings_only: true`, from a "Grant in Settings" button. libmobile
/// never opens Settings by itself. Any thread; needs the app to be visible
/// ([`OpenLinkError::NotVisible`] otherwise; [`OpenLinkError::Platform`] for
/// anything else).
pub fn open_app_settings(app: &AndroidApp) -> Result<(), OpenLinkError> {
    log::debug!("libmobile: open app settings");
    let message = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        let called = env.call_static_method(
            class,
            jni_str!("openAppSettings"),
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[activity.into()],
        );
        status_message(env, called)
    });
    answer(message)
}

/// The plugin's `String?` answer: `None` once the intent is sent.
fn status_message<'local>(
    env: &mut Env<'local>,
    called: jni::errors::Result<JValueOwned<'local>>,
) -> jni::errors::Result<Option<String>> {
    let status = match called {
        Err(jni::errors::Error::JavaException) => {
            env.exception_catch()?;
            return Err(jni::errors::Error::JavaException);
        }
        other => other?.l()?,
    };
    if status.is_null() {
        return Ok(None);
    }
    let status = env.cast_local::<JString>(status)?;
    Ok(opt_string(&status))
}

fn answer(message: jni::errors::Result<Option<String>>) -> Result<(), OpenLinkError> {
    let message = message.map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => OpenLinkError::Platform(msg),
        other => OpenLinkError::Platform(other.to_string()),
    })?;
    match message {
        None => Ok(()),
        Some(message) => Err(OpenLinkError::from_message(&message)),
    }
}
