//! Native Sign in with Google over Credential Manager (feature `google-auth`,
//! `auth/GoogleAuthPlugin.kt`).
//!
//! * [`start_google_login`] — Rust -> Kotlin: opens the account-picker sheet.
//! * [`Java_dev_highbay_libmobile_auth_GoogleAuthPlugin_nativeLoginResult`] —
//!   Kotlin -> Rust, on Credential Manager's executor thread.
//!
//! Failures to even start are reported through the same queue, so the app has
//! one place to handle every outcome.

use super::jni_util::{opt_string, with_app_class};
use crate::auth::LoginOutcome;
use crate::mailbox::Mailbox;
use android_activity::AndroidApp;
use jni::objects::{JObject, JString};
use jni::{EnvUnowned, jni_sig, jni_str};

const GOOGLE_AUTH_CLASS: &str = "dev.highbay.libmobile.auth.GoogleAuthPlugin";

static RESULTS: Mailbox<LoginOutcome> = Mailbox::new();

pub(crate) fn take_results() -> Vec<LoginOutcome> {
    RESULTS.take()
}

/// Open the Credential Manager account-picker sheet. `server_client_id` is the
/// **web** OAuth client of the backend that will verify the token (Google puts
/// it in the token's `aud`); an empty one fails fast with `no_client_id`.
/// The outcome arrives as [`MobileEvent::GoogleLogin`](crate::MobileEvent::GoogleLogin).
pub fn start_google_login(app: &AndroidApp, server_client_id: &str) {
    let result = with_app_class(app, GOOGLE_AUTH_CLASS, |env, activity, class| {
        let client_id = JString::from_str(env, server_client_id)?;
        env.call_static_method(
            class,
            jni_str!("start"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
            &[activity.into(), (&client_id).into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: google login could not be started: {err}");
        RESULTS.push(LoginOutcome::Failed(format!("jni: {err}")));
    }
}

/// Kotlin -> Rust: `GoogleAuthPlugin.nativeLoginResult(String?, String?)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_auth_GoogleAuthPlugin_nativeLoginResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    id_token: JString<'caller>,
    error: JString<'caller>,
) {
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let outcome = LoginOutcome::from_parts(opt_string(&id_token), opt_string(&error));
        match &outcome {
            LoginOutcome::IdToken(token) => {
                log::info!(
                    "libmobile: google id token received ({} chars)",
                    token.len()
                )
            }
            LoginOutcome::Failed(message) => {
                log::warn!("libmobile: google login failed: {message}")
            }
        }
        RESULTS.push(outcome);
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
