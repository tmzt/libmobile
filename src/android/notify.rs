//! Alert notifications (`notify/NotifyPlugin.kt`, feature `notifications`).
//!
//! Alerts are dismissable notifications on the app's own channels, apart
//! from the foreground service's: for something the user should see while
//! the app is not on screen. Every call here is synchronous, may be made
//! from any thread, and needs no `AndroidApp`: it reaches the plugin through
//! the Application that android-activity registers with `ndk-context`.
//!
//! ```no_run
//! use libmobile::android::notify::{self, Channel, Importance, NotificationContent};
//!
//! let channel = Channel::new("questions", "Questions", Importance::High);
//! let mut content = NotificationContent::new("agent 3", "has a question");
//! content.tag = Some("agent-3".into());
//! notify::post("agent-3", &content, &channel)?;
//! # Ok::<(), notify::NotifyError>(())
//! ```
//!
//! The channel is created on its first post; Android then keeps its
//! importance and sound (see [`Channel`]). Posting the same id again
//! replaces the alert. The app asks for `POST_NOTIFICATIONS` itself
//! (`request_permissions`); without it [`post`] gives
//! [`NotifyError::PermissionDenied`].
//!
//! A tap on the body opens the app, removes the alert and delivers
//! [`NotifyEvent::Tapped`] with the content's tag; a button delivers
//! [`NotifyEvent::Action`], opening the app only if it
//! [`opens_app`](NotificationAction::opens_app). Both arrive as
//! [`MobileEvent::Notify`](crate::MobileEvent::Notify), and to system-event
//! subscribers under `system-events`. A tap that starts the app arrives
//! after it starts.

pub use crate::notify::{
    Channel, Importance, LockScreenVisibility, MAX_ACTIONS, NotificationAction,
    NotificationContent, NotifyError, NotifyEvent,
};

use super::jni_util::{opt_string, with_application_class};
use super::notification::ContentArgs;
use crate::mailbox::Mailbox;
use crate::notify::{alert_id_ok, check_post};
use jni::objects::{JClass, JObject, JString};
use jni::{Env, EnvUnowned, jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.notify.NotifyPlugin";

/// Alert events for the main loop.
static EVENTS: Mailbox<NotifyEvent> = Mailbox::new();

pub(crate) fn take_events() -> Vec<NotifyEvent> {
    EVENTS.take()
}

/// Post the alert `id` (1 to 64 of `[A-Za-z0-9._-]`) on `channel`, creating
/// the channel on its first use, or replace the alert already posted with
/// that id.
pub fn post(id: &str, content: &NotificationContent, channel: &Channel) -> Result<(), NotifyError> {
    check_post(id, content, channel)?;
    call_status(|env, context, class| {
        let id = JString::from_str(env, id)?;
        let channel_id = JString::from_str(env, &channel.id)?;
        let channel_name = JString::from_str(env, &channel.name)?;
        let content = ContentArgs::new(env, content)?;
        env.call_static_method(
            class,
            jni_str!("post"),
            jni_sig!(
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;IILjava/lang/String;)Ljava/lang/String;"
            ),
            &[
                context.into(),
                (&id).into(),
                (&channel_id).into(),
                (&channel_name).into(),
                channel.importance.code().into(),
                (&content.title).into(),
                (&content.text).into(),
                (&content.ids).into(),
                (&content.labels).into(),
                content.opens.into(),
                content.visibility.into(),
                (&content.tag).into(),
            ],
        )?
        .l()
    })
}

/// Remove the alert `id`. Nothing happens if it is not shown.
pub fn cancel(id: &str) -> Result<(), NotifyError> {
    if !alert_id_ok(id) {
        return Err(NotifyError::BadId);
    }
    call_status(|env, context, class| {
        let id = JString::from_str(env, id)?;
        env.call_static_method(
            class,
            jni_str!("cancel"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;"),
            &[context.into(), (&id).into()],
        )?
        .l()
    })
}

/// Remove every alert the app has posted. The foreground service's
/// notification stays.
pub fn cancel_all() -> Result<(), NotifyError> {
    call_status(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("cancelAll"),
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[context.into()],
        )?
        .l()
    })
}

/// Run `f` against the plugin class with the Application, for a plugin
/// method that returns `null` on success or a `"<kind>: <detail>"` message.
fn call_status(
    f: impl for<'a> FnOnce(&mut Env<'a>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<JObject<'a>>,
) -> Result<(), NotifyError> {
    let message = with_application_class(PLUGIN_CLASS, |env, context, class| {
        let status = match f(env, context, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?,
        };
        if status.is_null() {
            return Ok(None);
        }
        let status = env.cast_local::<JString>(status)?;
        Ok(opt_string(&status))
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => NotifyError::Platform(msg),
        other => NotifyError::Platform(other.to_string()),
    })?;
    match message {
        None => Ok(()),
        Some(message) => Err(NotifyError::from_message(&message)),
    }
}

/// Kotlin -> Rust: `NotifyPlugin.nativeNotifyAction(String, String)`, for
/// a button that does not open the app, on the receiver's thread.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_notify_NotifyPlugin_nativeNotifyAction<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    id: JString<'caller>,
    action: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        if let (Some(id), Some(action)) = (opt_string(&id), opt_string(&action)) {
            deliver(NotifyEvent::Action { id, action });
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// To the main loop (and, under `system-events`, to subscribers).
pub(crate) fn deliver(event: NotifyEvent) {
    // The tag is the app's own and may name something private: not logged.
    match &event {
        NotifyEvent::Tapped { id, tag } => {
            log::info!("libmobile: alert {id:?} tapped (tag: {})", tag.is_some())
        }
        NotifyEvent::Action { id, action } => {
            log::info!("libmobile: alert {id:?} action {action:?}")
        }
    }
    #[cfg(feature = "system-events")]
    crate::system_events::publish(crate::system_events::SystemEvent::Notify(event.clone()));
    EVENTS.push(event);
}
