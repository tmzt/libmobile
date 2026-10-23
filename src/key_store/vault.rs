//! libmobile's half of the key store: where sealed envelopes live.
//!
//! * `Vault::named(namespace)` (Android): `SecureStorePlugin`'s key space,
//!   sealed under Keystore wrapping keys.
//! * `Vault::memory()` (feature `test-vault`, for tests only): a map per
//!   instance, with a simulated device lock.
//!
//! The engine (`key_store.rs`) is the same file in libdevice. The checks
//! below name this crate, so they live here.
//!
//! A `SealToken` cannot be made outside the crate:
//!
//! ```compile_fail,E0423
//! let token = libmobile::key_store::SealToken(());
//! ```
//!
//! nor kept past the call that lends it:
//!
//! ```compile_fail,E0521
//! use libmobile::key_store::SealToken;
//! static mut KEPT: Option<&'static SealToken> = None;
//! fn keep(token: &SealToken) {
//!     unsafe { KEPT = Some(token) };
//! }
//! ```
//!
//! and `with_key` cannot hand the key out of its closure:
//!
//! ```compile_fail
//! use libmobile::key_store::{SecretKey, Vault, with_key};
//! fn leak<K: SecretKey>(vault: &Vault) -> &K {
//!     with_key::<K, _>(vault, "k", |key| key).unwrap()
//! }
//! ```
#![cfg_attr(
    not(feature = "test-vault"),
    doc = "Without `test-vault` there is no memory vault:\n\n```compile_fail,E0599\nlet vault = libmobile::key_store::Vault::memory();\n```"
)]
// A host build without `test-vault` has no vault at all.
#![cfg_attr(not(any(target_os = "android", feature = "test-vault")), allow(unused))]

#[cfg(feature = "test-vault")]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "test-vault")]
use std::sync::Mutex;
#[cfg(feature = "test-vault")]
use std::sync::atomic::{AtomicBool, Ordering};

use zeroize::Zeroizing;

use super::{FLAG_DEVICE_UNLOCK_REQUIRED, KeyStoreError};

/// A stored item: its flags and its sealed envelope.
pub(super) type Stored = (u8, Zeroizing<Vec<u8>>);

/// A thin handle over the platform's storage; not a security boundary.
/// Cheap to clone, `'static`, usable from any thread.
#[derive(Clone)]
pub struct Vault(Inner);

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_tuple("Vault");
        match self.0 {
            #[cfg(target_os = "android")]
            Inner::Named(ref namespace) => out.field(namespace),
            #[cfg(feature = "test-vault")]
            Inner::Memory(_) => out.field(&"memory"),
        };
        out.finish()
    }
}

#[derive(Clone)]
enum Inner {
    #[cfg(target_os = "android")]
    Named(Arc<str>),
    #[cfg(feature = "test-vault")]
    Memory(Arc<Memory>),
}

#[cfg(feature = "test-vault")]
#[derive(Default)]
struct Memory {
    items: Mutex<HashMap<String, Stored>>,
    locked: AtomicBool,
}

impl Vault {
    /// The namespace's vault: a directory of the key space, sealed under
    /// Keystore wrapping keys. Namespaces follow the key name rule.
    #[cfg(target_os = "android")]
    pub fn named(namespace: &str) -> Result<Vault, KeyStoreError> {
        super::check_name(namespace)?;
        Ok(Vault(Inner::Named(namespace.into())))
    }

    /// For tests: a map of its own, empty, gone when the last clone drops.
    #[cfg(feature = "test-vault")]
    pub fn memory() -> Vault {
        Vault(Inner::Memory(Arc::default()))
    }

    /// For tests: while set, keys with `FLAG_DEVICE_UNLOCK_REQUIRED` give
    /// `DeviceLocked`. No effect on a named vault.
    #[cfg(feature = "test-vault")]
    pub fn set_device_locked(&self, locked: bool) {
        #[allow(irrefutable_let_patterns)]
        if let Inner::Memory(memory) = &self.0 {
            memory.locked.store(locked, Ordering::Relaxed);
        }
    }

    /// Flags this vault enforces.
    pub(super) fn supported_flags(&self) -> u8 {
        match self.0 {
            #[cfg(target_os = "android")]
            Inner::Named(_) => FLAG_DEVICE_UNLOCK_REQUIRED,
            #[cfg(feature = "test-vault")]
            Inner::Memory(_) => FLAG_DEVICE_UNLOCK_REQUIRED,
        }
    }

    /// The stored flags and envelope, or `None`.
    pub(super) fn get(&self, name: &str) -> Result<Option<Stored>, KeyStoreError> {
        match self.0 {
            #[cfg(target_os = "android")]
            Inner::Named(ref namespace) => android::get(namespace, name),
            #[cfg(feature = "test-vault")]
            Inner::Memory(ref memory) => {
                let items = memory.items.lock().unwrap_or_else(|e| e.into_inner());
                match items.get(name) {
                    Some((flags, _))
                        if flags & FLAG_DEVICE_UNLOCK_REQUIRED != 0
                            && memory.locked.load(Ordering::Relaxed) =>
                    {
                        Err(KeyStoreError::DeviceLocked)
                    }
                    Some((flags, envelope)) => Ok(Some((*flags, envelope.clone()))),
                    None => Ok(None),
                }
            }
        }
    }

    pub(super) fn put(&self, name: &str, flags: u8, envelope: &[u8]) -> Result<(), KeyStoreError> {
        match self.0 {
            #[cfg(target_os = "android")]
            Inner::Named(ref namespace) => android::put(namespace, name, flags, envelope),
            #[cfg(feature = "test-vault")]
            Inner::Memory(ref memory) => {
                let mut items = memory.items.lock().unwrap_or_else(|e| e.into_inner());
                items.insert(name.to_owned(), (flags, Zeroizing::new(envelope.to_vec())));
                Ok(())
            }
        }
    }

    pub(super) fn delete(&self, name: &str) -> Result<bool, KeyStoreError> {
        match self.0 {
            #[cfg(target_os = "android")]
            Inner::Named(ref namespace) => android::delete(namespace, name),
            #[cfg(feature = "test-vault")]
            Inner::Memory(ref memory) => {
                let mut items = memory.items.lock().unwrap_or_else(|e| e.into_inner());
                Ok(items.remove(name).is_some())
            }
        }
    }
}

/// The key space of `SecureStorePlugin`, through the Application context.
#[cfg(target_os = "android")]
mod android {
    use super::super::KeyStoreError;
    use super::Stored;
    use crate::android::secure_store::{PluginError, call_plugin};
    use jni::objects::{JByteArray, JString};
    use jni::{jni_sig, jni_str};
    use zeroize::Zeroizing;

    pub(super) fn get(namespace: &str, name: &str) -> Result<Option<Stored>, KeyStoreError> {
        let read = call(|env, context, class| {
            let (jnamespace, jname) = (
                JString::from_str(env, namespace)?,
                JString::from_str(env, name)?,
            );
            let value = env
                .call_static_method(
                    class,
                    jni_str!("keyGet"),
                    jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)[B"),
                    &[context.into(), (&jnamespace).into(), (&jname).into()],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            let bytes = env.cast_local::<JByteArray>(value)?;
            Ok(Some(Zeroizing::new(env.convert_byte_array(&bytes)?)))
        })?;
        Ok(match read {
            None => None,
            Some(bytes) => {
                let (&flags, envelope) = bytes
                    .split_first()
                    .ok_or_else(|| KeyStoreError::Corrupt("empty item".into()))?;
                Some((flags, Zeroizing::new(envelope.to_vec())))
            }
        })
    }

    pub(super) fn put(
        namespace: &str,
        name: &str,
        flags: u8,
        envelope: &[u8],
    ) -> Result<(), KeyStoreError> {
        call(|env, context, class| {
            let (jnamespace, jname) = (
                JString::from_str(env, namespace)?,
                JString::from_str(env, name)?,
            );
            let bytes = env.byte_array_from_slice(envelope)?;
            env.call_static_method(
                class,
                jni_str!("keyPut"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;I[B)V"),
                &[
                    context.into(),
                    (&jnamespace).into(),
                    (&jname).into(),
                    i32::from(flags).into(),
                    (&bytes).into(),
                ],
            )?;
            Ok(())
        })
    }

    pub(super) fn delete(namespace: &str, name: &str) -> Result<bool, KeyStoreError> {
        call(|env, context, class| {
            let (jnamespace, jname) = (
                JString::from_str(env, namespace)?,
                JString::from_str(env, name)?,
            );
            env.call_static_method(
                class,
                jni_str!("keyDelete"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Z"),
                &[context.into(), (&jnamespace).into(), (&jname).into()],
            )?
            .z()
        })
    }

    /// The plugin's `"<kind>: <detail>"` exceptions as key store errors.
    fn call<R>(
        f: impl FnOnce(
            &mut jni::Env<'_>,
            &jni::objects::JObject<'_>,
            &jni::objects::JClass<'_>,
        ) -> jni::errors::Result<R>,
    ) -> Result<R, KeyStoreError> {
        call_plugin(f).map_err(|err| match err {
            PluginError::Thrown(msg) => match msg.split_once(": ") {
                Some(("device-locked", _)) => KeyStoreError::DeviceLocked,
                Some(("corrupt", detail)) => KeyStoreError::Corrupt(detail.to_owned()),
                _ => KeyStoreError::Platform(msg),
            },
            PluginError::Jni(msg) => KeyStoreError::Platform(msg),
        })
    }
}
