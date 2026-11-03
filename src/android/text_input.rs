//! The IME bridge (`bridge/TextInputPlugin.kt`): an invisible `EditText` owns
//! the `InputConnection`, so the soft keyboard (autocorrect, composition,
//! voice input) works while the app still draws every pixel.
//!
//! * [`set_text_input_with`] — Rust -> Kotlin: focus the hidden field with a
//!   value and [`TextInputOptions`] and show the keyboard, or hide it.
//!   [`set_text_input`] is it with the default options.
//! * [`Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextChanged`] —
//!   Kotlin -> Rust: the field's full text after each IME edit.
//! * [`Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextAction`] —
//!   Kotlin -> Rust: the action key, or Enter on one line.
//! * The dismissal is not a callback: the main loop watches the keyboard's
//!   inset while the field is active (`ime_inset`).

use super::jni_util::with_app_class;
use crate::mailbox::Mailbox;
use crate::text_input::DismissWatch;
pub use crate::text_input::{TextAction, TextInputOptions, TextKind};
use android_activity::AndroidApp;
use jni::objects::{JObject, JString};
use jni::sys::jint;
use jni::{EnvUnowned, jni_sig, jni_str};
use std::sync::Mutex;

const TEXT_INPUT_CLASS: &str = "dev.highbay.libmobile.bridge.TextInputPlugin";

/// What the field reported, in order: an edit then Enter stays an edit then
/// Enter.
pub(crate) enum Update {
    Changed(String),
    Action(TextAction),
}

static PENDING: Mailbox<Update> = Mailbox::new();

/// Only the main loop touches it (`set_text_input_with` and the inset pump).
static DISMISS: Mutex<DismissWatch> = Mutex::new(DismissWatch::new());

pub(crate) fn take_pending() -> Vec<Update> {
    PENDING.take()
}

/// An inset report reached the main loop: `true` when it is the keyboard
/// closing while the field is active.
pub(crate) fn ime_inset(ime_bottom: f32) -> bool {
    DISMISS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ime(ime_bottom)
}

/// A new run's field is a new one: nothing is active.
pub(crate) fn reset() {
    DISMISS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .deactivate();
}

/// `Some(value)`: focus the hidden field holding `value` and show the
/// keyboard. `None`: hide it. [`set_text_input_with`] with
/// [`TextInputOptions::default()`]. Failures are logged; there is no result.
pub fn set_text_input(app: &AndroidApp, value: Option<&str>) {
    set_text_input_with(app, value, &TextInputOptions::default());
}

/// As [`set_text_input`], with `options` applied to the field before it is
/// focused; new options while it is active are re-applied
/// (`InputMethodManager.restartInput`). With `None`, `options` are unused.
/// Failures are logged; there is no result.
pub fn set_text_input_with(app: &AndroidApp, value: Option<&str>, options: &TextInputOptions) {
    {
        let mut watch = DISMISS.lock().unwrap_or_else(|e| e.into_inner());
        if value.is_some() {
            watch.activate();
        } else {
            watch.deactivate();
        }
    }
    let result = with_app_class(app, TEXT_INPUT_CLASS, |env, activity, class| {
        let text = JString::from_str(env, value.unwrap_or_default())?;
        env.call_static_method(
            class,
            jni_str!("setActive"),
            jni_sig!("(Landroid/app/Activity;ZLjava/lang/String;IIZI)V"),
            &[
                activity.into(),
                value.is_some().into(),
                (&text).into(),
                options.input_type().into(),
                options.ime_options().into(),
                options.reports_action().into(),
                options.action.code().into(),
            ],
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
        PENDING.push(Update::Changed(
            super::jni_util::opt_string(&text).unwrap_or_default(),
        ));
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Kotlin -> Rust: `TextInputPlugin.nativeTextAction(int)`, the code
/// `set_text_input_with` handed it for the options' action.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextAction<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    code: jint,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        PENDING.push(Update::Action(TextAction::from_code(code)));
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
