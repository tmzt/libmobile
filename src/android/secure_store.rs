//! Named byte blobs, encrypted at rest (`store/SecureStorePlugin.kt`).
//!
//! Each blob is encrypted with an AES-256-GCM key that Android Keystore keeps
//! and never exports: in StrongBox where the device has one (unless the app
//! enables `secure-store-no-strongbox`), else in the TEE. The ciphertext is
//! kept in the app's no-backup storage. There is no plaintext fallback: if
//! Keystore fails, the call fails.
//!
//! Every call is synchronous, Rust -> Kotlin, and may be made from any
//! thread (`with_app_class` attaches it). Each costs a Keystore operation,
//! milliseconds in the TEE and more in StrongBox, so don't call per frame.

use super::jni_util::with_app_class;
use crate::secure_store::{PutOptions, SecureStoreError, SecurityLevel, check_name, check_value};
use android_activity::AndroidApp;
use jni::objects::{JByteArray, JString};
use jni::{Env, jni_sig, jni_str};

const STORE_CLASS: &str = "dev.highbay.libmobile.store.SecureStorePlugin";

/// The blob called `name`, or `None` if there is none.
/// [`SecureStoreError::Locked`] for an `unlocked_only` blob while the device
/// is locked.
pub fn get(app: &AndroidApp, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
    check_name(name)?;
    call(app, |env, activity, class| {
        let jname = JString::from_str(env, name)?;
        let value = env
            .call_static_method(
                class,
                jni_str!("get"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)[B"),
                &[activity.into(), (&jname).into()],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let bytes = env.cast_local::<JByteArray>(value)?;
        Ok(Some(env.convert_byte_array(&bytes)?))
    })
}

/// Store `value` as the blob called `name`, replacing any earlier one.
/// Readable while the device is locked; see [`put_with`].
pub fn put(app: &AndroidApp, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    put_with(app, name, value, PutOptions::default())
}

/// [`put`] with options: [`PutOptions::unlocked_only`] wraps the blob with
/// a key Keystore refuses while the device is locked, so [`get`] gives
/// [`SecureStoreError::Locked`] until it is unlocked.
pub fn put_with(
    app: &AndroidApp,
    name: &str,
    value: &[u8],
    options: PutOptions,
) -> Result<(), SecureStoreError> {
    check_name(name)?;
    check_value(value)?;
    call(app, |env, activity, class| {
        let jname = JString::from_str(env, name)?;
        let bytes = env.byte_array_from_slice(value)?;
        env.call_static_method(
            class,
            jni_str!("put"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;[BI)V"),
            &[
                activity.into(),
                (&jname).into(),
                (&bytes).into(),
                options.flags().into(),
            ],
        )?;
        Ok(())
    })
}

/// Remove the blob called `name`. `true` if there was one.
pub fn delete(app: &AndroidApp, name: &str) -> Result<bool, SecureStoreError> {
    check_name(name)?;
    call(app, |env, activity, class| {
        let jname = JString::from_str(env, name)?;
        env.call_static_method(
            class,
            jni_str!("delete"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Z"),
            &[activity.into(), (&jname).into()],
        )?
        .z()
    })
}

/// Where the store's key lives. Creates the key if there is none yet.
pub fn security_level(app: &AndroidApp) -> Result<SecurityLevel, SecureStoreError> {
    let code = call(app, |env, activity, class| {
        env.call_static_method(
            class,
            jni_str!("securityLevel"),
            jni_sig!("(Landroid/app/Activity;)I"),
            &[activity.into()],
        )?
        .i()
    })?;
    SecurityLevel::from_code(code)
        .ok_or_else(|| SecureStoreError::Jni(format!("unknown security level {code}")))
}

/// Run `f` against the plugin class, turning a thrown `SecureStoreException`
/// into its [`SecureStoreError`] (and clearing it).
fn call<R>(
    app: &AndroidApp,
    f: impl FnOnce(
        &mut Env<'_>,
        &jni::objects::JObject<'_>,
        &jni::objects::JClass<'_>,
    ) -> jni::errors::Result<R>,
) -> Result<R, SecureStoreError> {
    with_app_class(app, STORE_CLASS, |env, activity, class| {
        match f(env, activity, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                Err(jni::errors::Error::JavaException)
            }
            other => other,
        }
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => SecureStoreError::from_message(&msg),
        other => SecureStoreError::Jni(other.to_string()),
    })
}
