//! JNI leg of the memory-trim levels (`bridge/TrimMemoryPlugin.kt`, core).
//!
//! Kotlin -> Rust only: the plugin registers `ComponentCallbacks2` on the
//! Application and reports each `onTrimMemory` level, on the UI thread. The
//! level is queued for the main loop as [`MobileEvent::TrimMemory`](crate::MobileEvent::TrimMemory)
//! and, under `system-events`, handed to the subscribers at once.

use crate::event::TrimLevel;
use crate::mailbox::Mailbox;
use jni::EnvUnowned;
use jni::objects::JObject;
use jni::sys::jint;

static PENDING: Mailbox<TrimLevel> = Mailbox::new();

pub(crate) fn take_pending() -> Vec<TrimLevel> {
    PENDING.take()
}

/// Kotlin -> Rust: `TrimMemoryPlugin.nativeTrimMemory(int)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_TrimMemoryPlugin_nativeTrimMemory<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    level: jint,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let trim = TrimLevel::from_level(level);
        log::info!("libmobile: trim memory {level} ({trim:?})");
        PENDING.push(trim);
        #[cfg(feature = "system-events")]
        crate::system_events::publish(crate::system_events::SystemEvent::TrimMemory(trim));
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
