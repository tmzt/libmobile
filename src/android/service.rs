//! The foreground service (`service/MobileForegroundService.kt`, feature
//! `foreground-service`).
//!
//! A started foreground service keeps Android from caching and freezing the
//! process, so the app's own threads keep running with the screen off. It
//! does not keep the process from being killed (memory pressure, Force
//! stop), and it is never restarted after that (`START_NOT_STICKY`): the
//! app starts it again the next time it is visible.
//!
//! Declare it in the build script with
//! [`Project::foreground_service`](crate::gradle::Project::foreground_service).
//! Every call here is synchronous, may be made from any thread, and needs no
//! `AndroidApp`: it reaches the plugin through the Application that
//! android-activity registers with `ndk-context`.
//!
//! [`start`] works only while the app is visible; Android refuses a start
//! from the background ([`ServiceError::StartNotAllowed`]), and `Microphone`
//! also needs `RECORD_AUDIO` granted. Updating and stopping work at any
//! time. What the service does is reported as
//! [`MobileEvent::Service`](crate::MobileEvent::Service), and to system-event
//! subscribers under `system-events`.
//!
//! On Android 13+ the app asks for `POST_NOTIFICATIONS` itself
//! (`request_permissions`) before starting. Without it the service still
//! runs, but its notification shows only in the system's running-apps list,
//! reported as [`ServiceEvent::NotificationHidden`].

pub use crate::codegen::ServiceType;
pub use crate::service::{
    ControlLabels, Controls, EXIT_ACTION, LockScreenVisibility, MAX_ACTIONS, NotificationAction,
    NotificationContent, ServiceError, ServiceOptions, WAKE_LOCK_ACTION,
};
pub use crate::system_events::ServiceEvent;

use super::jni_util::{opt_string, with_application_class};
use crate::mailbox::Mailbox;
use crate::service::check_content;
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::sys::jint;
use jni::{Env, EnvUnowned, jni_sig, jni_str};
use std::sync::atomic::{AtomicUsize, Ordering};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.service.ForegroundServicePlugin";

/// Service events for the main loop.
static EVENTS: Mailbox<ServiceEvent> = Mailbox::new();

/// The buttons the running service's controls take, for [`update`]'s check.
static BUILTIN_BUTTONS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn take_events() -> Vec<ServiceEvent> {
    EVENTS.take()
}

/// Start the service, or restart it with new options if it runs. While the
/// app is visible only. A failure inside the service after this returns
/// arrives as [`ServiceEvent::Failed`].
pub fn start(options: &ServiceOptions) -> Result<(), ServiceError> {
    let builtin = options.controls.buttons();
    check_content(&options.content, builtin)?;
    let labels = &options.controls.labels;
    let control_labels = [
        labels.exit.as_str(),
        labels.acquire_wake_lock.as_str(),
        labels.release_wake_lock.as_str(),
        labels.wake_lock_held.as_str(),
    ];
    call_status(|env, context, class| {
        let content = ContentArgs::new(env, &options.content)?;
        let control_labels = string_array(env, &control_labels)?;
        env.call_static_method(
            class,
            jni_str!("start"),
            jni_sig!(
                "(Landroid/content/Context;ILjava/lang/String;Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;IIZZZ[Ljava/lang/String;Z)Ljava/lang/String;"
            ),
            &[
                context.into(),
                options.type_bits().into(),
                (&content.title).into(),
                (&content.text).into(),
                (&content.ids).into(),
                (&content.labels).into(),
                content.opens.into(),
                content.visibility.into(),
                options.controls.ongoing.into(),
                options.controls.exit.into(),
                options.controls.wake_lock_toggle.into(),
                (&control_labels).into(),
                options.wake_lock.into(),
            ],
        )?
        .l()
    })?;
    BUILTIN_BUTTONS.store(builtin, Ordering::Relaxed);
    Ok(())
}

/// Replace the notification's content. [`ServiceError::NotRunning`] while
/// stopped.
pub fn update(content: &NotificationContent) -> Result<(), ServiceError> {
    check_content(content, BUILTIN_BUTTONS.load(Ordering::Relaxed))?;
    call_status(|env, context, class| {
        let content = ContentArgs::new(env, content)?;
        env.call_static_method(
            class,
            jni_str!("update"),
            jni_sig!(
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;II)Ljava/lang/String;"
            ),
            &[
                context.into(),
                (&content.title).into(),
                (&content.text).into(),
                (&content.ids).into(),
                (&content.labels).into(),
                content.opens.into(),
                content.visibility.into(),
            ],
        )?
        .l()
    })
}

/// Replace only the notification's text. [`ServiceError::NotRunning`] while
/// stopped.
pub fn update_text(text: &str) -> Result<(), ServiceError> {
    call_status(|env, context, class| {
        let text = JString::from_str(env, text)?;
        env.call_static_method(
            class,
            jni_str!("updateText"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;"),
            &[context.into(), (&text).into()],
        )?
        .l()
    })
}

/// Stop the service, release the wake lock and remove the notification.
/// Nothing happens while stopped.
pub fn stop() -> Result<(), ServiceError> {
    call_status(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("stop"),
            jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
            &[context.into()],
        )?
        .l()
    })
}

/// Whether the service has been started and not stopped since. `false` if
/// the JNI call fails.
pub fn is_running() -> bool {
    call_bool(jni_str!("isRunning"))
}

/// Acquire or release the service's partial wake lock (tag
/// `libmobile:foreground`), as the wake-lock button does: the notification
/// is rebuilt, and [`ServiceEvent::WakeLock`] is delivered when it changes.
/// [`ServiceError::NotRunning`] while stopped.
pub fn set_wake_lock(held: bool) -> Result<(), ServiceError> {
    call_status(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("setWakeLock"),
            jni_sig!("(Landroid/content/Context;Z)Ljava/lang/String;"),
            &[context.into(), held.into()],
        )?
        .l()
    })
}

/// Whether the service holds its wake lock. `false` if the JNI call fails.
pub fn wake_lock_held() -> bool {
    call_bool(jni_str!("wakeLockHeld"))
}

/// The notification content as JNI arguments.
struct ContentArgs<'local> {
    title: JString<'local>,
    text: JString<'local>,
    ids: JObjectArray<'local, JString<'local>>,
    labels: JObjectArray<'local, JString<'local>>,
    /// Bit `i`: action `i` opens the app.
    opens: jint,
    visibility: jint,
}

impl<'local> ContentArgs<'local> {
    fn new(env: &mut Env<'local>, content: &NotificationContent) -> jni::errors::Result<Self> {
        let ids: Vec<&str> = content.actions.iter().map(|a| a.id.as_str()).collect();
        let labels: Vec<&str> = content.actions.iter().map(|a| a.label.as_str()).collect();
        let opens = content
            .actions
            .iter()
            .enumerate()
            .fold(0, |bits, (i, a)| bits | (jint::from(a.opens_app) << i));
        Ok(Self {
            title: JString::from_str(env, &content.title)?,
            text: JString::from_str(env, &content.text)?,
            ids: string_array(env, &ids)?,
            labels: string_array(env, &labels)?,
            opens,
            visibility: content.visibility.code(),
        })
    }
}

fn string_array<'local>(
    env: &mut Env<'local>,
    items: &[&str],
) -> jni::errors::Result<JObjectArray<'local, JString<'local>>> {
    let empty = JString::from_str(env, "")?;
    let array = JObjectArray::<JString>::new(env, items.len(), &empty)?;
    for (i, item) in items.iter().enumerate() {
        let item = JString::from_str(env, item)?;
        array.set_element(env, i, &item)?;
    }
    Ok(array)
}

/// Run `f` against the plugin class with the Application, catching a thrown
/// exception as its message.
pub(crate) fn call<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> Result<R, ServiceError> {
    with_application_class(PLUGIN_CLASS, |env, context, class| {
        match f(env, context, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                Err(jni::errors::Error::JavaException)
            }
            other => other,
        }
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => ServiceError::Platform(msg),
        other => ServiceError::Platform(other.to_string()),
    })
}

/// [`call`] for a plugin method that returns `null` on success or a
/// `"<kind>: <detail>"` message.
pub(crate) fn call_status(
    f: impl for<'a> FnOnce(&mut Env<'a>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<JObject<'a>>,
) -> Result<(), ServiceError> {
    let message = call(|env, context, class| {
        let status = f(env, context, class)?;
        if status.is_null() {
            return Ok(None);
        }
        let status = env.cast_local::<JString>(status)?;
        Ok(opt_string(&status))
    })?;
    match message {
        None => Ok(()),
        Some(message) => Err(ServiceError::from_message(&message)),
    }
}

/// `ForegroundServicePlugin.<method>(Context) -> boolean`, `false` on error.
fn call_bool(method: &'static jni::strings::JNIStr) -> bool {
    call(|env, context, class| {
        env.call_static_method(
            class,
            method,
            jni_sig!("(Landroid/content/Context;)Z"),
            &[context.into()],
        )?
        .z()
    })
    .unwrap_or_else(|err| {
        log::error!("libmobile: service query failed: {err}");
        false
    })
}

/// Kotlin -> Rust: `ForegroundServicePlugin.nativeServiceEvent(int, String?)`,
/// on the UI thread.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_service_ForegroundServicePlugin_nativeServiceEvent<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    code: jint,
    detail: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        match ServiceEvent::from_code(code, opt_string(&detail)) {
            Some(event) => {
                log::info!("libmobile: service {event:?}");
                if matches!(event, ServiceEvent::Stopped | ServiceEvent::Exited) {
                    BUILTIN_BUTTONS.store(0, Ordering::Relaxed);
                }
                deliver(event);
            }
            None => log::error!("libmobile: unknown service event {code}"),
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// To the main loop (and, under `system-events`, to subscribers).
fn deliver(event: ServiceEvent) {
    #[cfg(feature = "system-events")]
    crate::system_events::publish(crate::system_events::SystemEvent::Service(event.clone()));
    EVENTS.push(event);
}
