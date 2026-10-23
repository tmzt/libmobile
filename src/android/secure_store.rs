//! Named byte blobs, encrypted at rest (`store/SecureStorePlugin.kt`).
//!
//! Each blob is encrypted with an AES-256-GCM key that Android Keystore keeps
//! and never exports: in StrongBox where the device has one (unless the app
//! enables `secure-store-no-strongbox`), else in the TEE. The ciphertext is
//! kept in the app's no-backup storage. There is no plaintext fallback: if
//! Keystore fails, the call fails.
//!
//! Every call is synchronous, Rust -> Kotlin, and may be made from any
//! thread. It reaches the plugin through the Application that
//! android-activity registers with `ndk-context`, so no `AndroidApp` is
//! needed. Each call costs a Keystore operation, milliseconds in the TEE and
//! more in StrongBox, so don't call per frame.

use super::jni_util::with_application_class;
use crate::secure_store::{PutOptions, SecureStoreError, SecurityLevel, check_name, check_value};
use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::{Env, jni_sig, jni_str};

const STORE_CLASS: &str = "dev.highbay.libmobile.store.SecureStorePlugin";

/// The blob store as a value, with the same methods as the functions below,
/// so it has the shape of libdevice's `SecureStore`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SecureStore;

impl SecureStore {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
        get(name)
    }

    pub fn put(&self, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
        put(name, value)
    }

    pub fn put_with(
        &self,
        name: &str,
        value: &[u8],
        options: PutOptions,
    ) -> Result<(), SecureStoreError> {
        put_with(name, value, options)
    }

    pub fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        delete(name)
    }

    pub fn security_level(&self) -> Result<SecurityLevel, SecureStoreError> {
        security_level()
    }
}

/// The blob called `name`, or `None` if there is none.
/// [`SecureStoreError::DeviceLocked`] for an `unlocked_only` blob while the
/// device is locked.
pub fn get(name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
    check_name(name)?;
    call(|env, context, class| {
        let jname = JString::from_str(env, name)?;
        let value = env
            .call_static_method(
                class,
                jni_str!("get"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;)[B"),
                &[context.into(), (&jname).into()],
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
pub fn put(name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    put_with(name, value, PutOptions::default())
}

/// [`put`] with options: [`PutOptions::unlocked_only`] wraps the blob with
/// a key Keystore refuses while the device is locked, so [`get`] gives
/// [`SecureStoreError::DeviceLocked`] until it is unlocked.
pub fn put_with(name: &str, value: &[u8], options: PutOptions) -> Result<(), SecureStoreError> {
    check_name(name)?;
    check_value(value)?;
    call(|env, context, class| {
        let jname = JString::from_str(env, name)?;
        let bytes = env.byte_array_from_slice(value)?;
        env.call_static_method(
            class,
            jni_str!("put"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;[BI)V"),
            &[
                context.into(),
                (&jname).into(),
                (&bytes).into(),
                options.flags().into(),
            ],
        )?;
        Ok(())
    })
}

/// Remove the blob called `name`. `true` if there was one.
pub fn delete(name: &str) -> Result<bool, SecureStoreError> {
    check_name(name)?;
    call(|env, context, class| {
        let jname = JString::from_str(env, name)?;
        env.call_static_method(
            class,
            jni_str!("delete"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Z"),
            &[context.into(), (&jname).into()],
        )?
        .z()
    })
}

/// Where the store's key lives. Creates the key if there is none yet.
pub fn security_level() -> Result<SecurityLevel, SecureStoreError> {
    let code = call(|env, context, class| {
        env.call_static_method(
            class,
            jni_str!("securityLevel"),
            jni_sig!("(Landroid/content/Context;)I"),
            &[context.into()],
        )?
        .i()
    })?;
    SecurityLevel::from_code(code)
        .ok_or_else(|| SecureStoreError::Platform(format!("unknown security level {code}")))
}

/// Run `f` against the plugin class with the Application, turning a thrown
/// `SecureStoreException` into its `"<kind>: <detail>"` message (and
/// clearing it).
pub(crate) fn call_plugin<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> Result<R, PluginError> {
    with_application_class(STORE_CLASS, |env, context, class| {
        match f(env, context, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                Err(jni::errors::Error::JavaException)
            }
            other => other,
        }
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => PluginError::Thrown(msg),
        other => PluginError::Jni(other.to_string()),
    })
}

/// How a plugin call failed: an exception's message, or JNI itself.
pub(crate) enum PluginError {
    Thrown(String),
    Jni(String),
}

fn call<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> Result<R, SecureStoreError> {
    call_plugin(f).map_err(|err| match err {
        PluginError::Thrown(msg) => SecureStoreError::from_message(&msg),
        PluginError::Jni(msg) => SecureStoreError::Platform(msg),
    })
}
