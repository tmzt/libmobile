//! JNI leg of the system events (`events/SystemEventsPlugin.kt`, feature
//! `system-events`).
//!
//! Rust -> Kotlin once: [`ensure_registered`] registers the callbacks on the
//! Application. Kotlin -> Rust from then on, on whatever thread runs each
//! callback: the events go straight to the subscribers
//! (`crate::system_events::publish`), never through the main loop.

use super::jni_util::with_application_class;
use crate::system_events::{NetworkInfo, SystemEvent, Transport, publish};
use jni::objects::JObject;
use jni::sys::jint;
use jni::{EnvUnowned, jni_sig, jni_str};
use std::sync::Mutex;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.events.SystemEventsPlugin";

/// Register the callbacks, once per process. A failure is logged, and the
/// next call tries again.
pub(crate) fn ensure_registered() {
    static REGISTERED: Mutex<bool> = Mutex::new(false);
    let mut registered = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    if *registered {
        return;
    }
    let result = with_application_class(PLUGIN_CLASS, |env, context, class| {
        let called = env.call_static_method(
            class,
            jni_str!("register"),
            jni_sig!("(Landroid/content/Context;)V"),
            &[context.into()],
        );
        match called {
            // Clears it, as `CaughtJavaException` with the message.
            Err(jni::errors::Error::JavaException) => env.exception_catch(),
            other => other.map(drop),
        }
    });
    match result {
        Ok(()) => *registered = true,
        Err(err) => log::error!("libmobile: registering system events failed: {err}"),
    }
}

/// Kotlin -> Rust: `SystemEventsPlugin.nativeState(int, boolean)`:
/// 0 visible, 1 focused, 2 Doze.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_events_SystemEventsPlugin_nativeState<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    kind: jint,
    value: bool,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let event = match kind {
            0 => SystemEvent::Visible(value),
            1 => SystemEvent::Focused(value),
            2 => SystemEvent::DeviceIdle(value),
            _ => {
                log::error!("libmobile: unknown system state {kind}");
                return Ok(());
            }
        };
        log::info!("libmobile: system {event:?}");
        publish(event);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Kotlin -> Rust: `SystemEventsPlugin.nativeNetwork(boolean, boolean,
/// boolean, int)`: whether there is a default network, then its details.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_events_SystemEventsPlugin_nativeNetwork<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    present: bool,
    validated: bool,
    metered: bool,
    transport: jint,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let info =
            present.then(|| NetworkInfo::new(validated, metered, Transport::from_code(transport)));
        let event = SystemEvent::Network(info);
        log::info!("libmobile: system {event:?}");
        publish(event);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
