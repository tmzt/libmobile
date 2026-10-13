//! The IME bridge (`bridge/TextInputPlugin.kt`): an invisible `EditText` owns
//! the `InputConnection`, so the soft keyboard (autocorrect, composition,
//! voice input) works while the app still draws every pixel.
//!
//! * [`set_text_input`] — Rust -> Kotlin: focus the hidden field with a value
//!   and show the keyboard, or hide it.
//! * [`Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextChanged`] —
//!   Kotlin -> Rust: the field's full text after each IME edit.

use super::jni_util::with_app_class;
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JObject, JString};
use jni::{EnvUnowned, jni_sig, jni_str};

const TEXT_INPUT_CLASS: &str = "dev.highbay.libmobile.bridge.TextInputPlugin";

static PENDING: Mailbox<String> = Mailbox::new();

pub(crate) fn take_pending() -> Vec<String> {
    PENDING.take()
}

/// `Some(value)`: focus the hidden field holding `value` and show the
/// keyboard. `None`: hide it. Failures are logged; there is no result.
pub fn set_text_input(app: &AndroidApp, value: Option<&str>) {
    let result = with_app_class(app, TEXT_INPUT_CLASS, |env, activity, class| {
        let text = JString::from_str(env, value.unwrap_or_default())?;
        env.call_static_method(
            class,
            jni_str!("setActive"),
            jni_sig!("(Landroid/app/Activity;ZLjava/lang/String;)V"),
            &[activity.into(), value.is_some().into(), (&text).into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: text input activation failed: {err}");
    }
}

/// Kotlin -> Rust: `TextInputPlugin.nativeTextChanged(String)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextChanged<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    text: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        PENDING.push(super::jni_util::opt_string(&text).unwrap_or_default());
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
