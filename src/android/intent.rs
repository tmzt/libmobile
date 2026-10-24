//! JNI leg of the intent bridge (`bridge/IntentPlugin.kt`).
//!
//! Kotlin -> Rust only: `IntentPlugin` forwards the launch intent from
//! `onCreate` and every later one from `onNewIntent` (which stock
//! `NativeActivity` swallows). Runs on the UI thread, so it only queues.

use super::jni_util::opt_string;
use crate::intent::{IntentData, NotificationTap, redact_url};
use crate::mailbox::Mailbox;
use jni::objects::{JObject, JObjectArray, JString};
use jni::sys::jboolean;
use jni::{Env, EnvUnowned};

static PENDING: Mailbox<IntentData> = Mailbox::new();

pub(crate) fn take_pending() -> Vec<IntentData> {
    PENDING.take()
}

fn strings(
    env: &mut Env<'_>,
    array: &JObjectArray<'_, JString<'_>>,
) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let len = array.len(env)?;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let item = array.get_element(env, i)?;
        out.push(opt_string(&item).unwrap_or_default());
    }
    Ok(out)
}

/// Kotlin -> Rust: `IntentPlugin.nativeIntent(String?, String?, String[], String[], boolean)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_IntentPlugin_nativeIntent<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    action: JString<'caller>,
    data: JString<'caller>,
    keys: JObjectArray<'caller, JString<'caller>>,
    values: JObjectArray<'caller, JString<'caller>>,
    is_new: jboolean,
) {
    let outcome = unowned_env.with_env(|env| -> Result<(), jni::errors::Error> {
        let intent = IntentData {
            action: opt_string(&action),
            data: opt_string(&data),
            extras: IntentData::zip_extras(strings(env, &keys)?, strings(env, &values)?),
            is_new,
        };
        // Never the full URL or the extras' values: either may hold a secret.
        let keys: Vec<&str> = intent.extras.iter().map(|(k, _)| k.as_str()).collect();
        log::info!(
            "libmobile: intent action={:?} data={:?} extras={keys:?} is_new={}",
            intent.action,
            intent.data.as_deref().map(redact_url),
            intent.is_new,
        );
        PENDING.push(intent);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Kotlin -> Rust: `IntentPlugin.nativeNotificationTap(String?, String,
/// String?, String?)`: a tap on a notification libmobile posted, from its
/// intent's extras (`crate::intent::tap_extras`), delivered instead of
/// `MobileEvent::Intent`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_IntentPlugin_nativeNotificationTap<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    kind: JString<'caller>,
    id: JString<'caller>,
    tag: JString<'caller>,
    action: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let tap = NotificationTap::from_extras(
            opt_string(&kind),
            opt_string(&id).unwrap_or_default(),
            opt_string(&tag),
            opt_string(&action),
        );
        match tap {
            #[cfg(feature = "foreground-service")]
            Some(NotificationTap::Service { tag }) => {
                log::info!(
                    "libmobile: service notification tapped (tag: {})",
                    tag.is_some()
                );
                super::service::deliver(crate::system_events::ServiceEvent::Tapped(tag));
            }
            #[cfg(feature = "notifications")]
            Some(NotificationTap::Alert { id, tag }) => {
                super::notify::deliver(crate::notify::NotifyEvent::Tapped { id, tag })
            }
            #[cfg(feature = "notifications")]
            Some(NotificationTap::AlertAction { id, action }) => {
                super::notify::deliver(crate::notify::NotifyEvent::Action { id, action })
            }
            other => log::warn!("libmobile: a notification tap not delivered: {other:?}"),
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
