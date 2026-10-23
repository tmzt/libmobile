//! **Key store engine.** Keys whose algorithm an adapter defines
//! ([`SecretKey`]) are generated, sealed and validated here, and lent to the
//! adapter's code for one call ([`with_key`]). Only the public value leaves.
//!
//! This file is the same, byte for byte, in libmobile and libdevice
//! (libmorph's key store plan, `20261022_06_KEY_STORE_PLAN.md`); the
//! platform half is `key_store/vault.rs`. Change it in libmobile first, then
//! copy it.

mod vault;

pub use vault::Vault;

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use zeroize::{ZeroizeOnDrop, Zeroizing};

/// Proof that the store is the caller. Only this crate can make one, and it
/// is lent by reference, so it cannot outlive a call.
pub struct SealToken(());

/// The platform keeps the key usable only while the device is unlocked.
pub const FLAG_DEVICE_UNLOCK_REQUIRED: u8 = 1 << 0;
/// Reserved for keys the app must unlock before use (a later plan). Refused
/// on write and on read until then.
pub const FLAG_APP_UNLOCK_REQUIRED: u8 = 1 << 1;

/// Longest key or namespace name, in bytes.
pub const MAX_NAME_LEN: usize = 64;
/// Largest sealed envelope (kind and sealed bytes), in bytes.
pub const MAX_ENVELOPE_LEN: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KeyStoreError {
    /// Not 1 to 64 of `[A-Za-z0-9._-]`, or starts with `.`.
    InvalidName,
    NotFound,
    /// Stored as a kind the adapter does not accept; the stored kind.
    WrongKind(String),
    /// Malformed, failed to decrypt, or refused by `unseal`.
    Corrupt(String),
    /// The device is locked and the key needs it unlocked.
    DeviceLocked,
    /// Reserved: the key must be unlocked by the app first. Never returned
    /// yet.
    Locked,
    /// Flags this build does not enforce, or unknown bits; the flags byte.
    UnsupportedFlags(u8),
    /// The platform failed.
    Platform(String),
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "key store: names are 1 to {MAX_NAME_LEN} of [A-Za-z0-9._-], not starting with '.'"
            ),
            Self::NotFound => write!(f, "key store: no such key"),
            Self::WrongKind(kind) => write!(f, "key store: stored as {kind:?}"),
            Self::Corrupt(detail) => write!(f, "key store: corrupt: {detail}"),
            Self::DeviceLocked => write!(f, "key store: the device is locked"),
            Self::Locked => write!(f, "key store: the key is locked"),
            Self::UnsupportedFlags(flags) => {
                write!(f, "key store: unsupported flags {flags:#010b}")
            }
            Self::Platform(detail) => write!(f, "key store: {detail}"),
        }
    }
}

impl std::error::Error for KeyStoreError {}

/// A private key type, implemented by its adapter. Keep its fields private,
/// and its secret behind a heap pointer (`Box<[u8; 32]>`): Rust moves are
/// copies that leave the old bytes behind.
pub trait SecretKey: ZeroizeOnDrop + Sized + Send + Sync + 'static {
    type Public: Clone + Send + Sync + 'static;
    /// 1 to 255 bytes, e.g. "handlr.noise.x25519.v1".
    const KIND: &'static str;
    /// Kinds `unseal` can read: `KIND` and older encodings.
    fn accepts(kind: &str) -> bool {
        kind == Self::KIND
    }
    fn generate(token: &SealToken) -> Self;
    fn seal(&self, token: &SealToken) -> Zeroizing<Vec<u8>>;
    /// `kind` is the stored kind, which may be an older one `accepts` allows.
    fn unseal(kind: &str, bytes: &[u8], token: &SealToken) -> Result<Self, String>;
    fn public(&self) -> Self::Public;
}

/// Serializes vault calls and the migration write. It guards no data, so a
/// poisoned lock is simply taken. Never held while adapter code runs.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A new key called `name`, replacing any earlier one. Returns its public
/// value; the private key is dropped (and cleared) before return.
pub fn generate<K: SecretKey>(
    vault: &Vault,
    name: &str,
    flags: u8,
) -> Result<K::Public, KeyStoreError> {
    check_name(name)?;
    if flags & !vault.supported_flags() != 0 {
        return Err(KeyStoreError::UnsupportedFlags(flags));
    }
    let token = SealToken(());
    let key = K::generate(&token);
    let envelope = envelope::<K>(&key.seal(&token))?;
    {
        let _guard = lock();
        vault.put(name, flags, &envelope)?;
    }
    Ok(key.public())
}

/// The public value of the key called `name`.
pub fn public<K: SecretKey>(vault: &Vault, name: &str) -> Result<K::Public, KeyStoreError> {
    Ok(load::<K>(vault, name)?.public())
}

/// Lend the private key to `f` for this call only. `f` cannot return the
/// borrow. The lock is not held while `f` runs, so `f` may use the store.
pub fn with_key<K: SecretKey, R>(
    vault: &Vault,
    name: &str,
    f: impl FnOnce(&K) -> R,
) -> Result<R, KeyStoreError> {
    let key = load::<K>(vault, name)?;
    Ok(f(&key))
}

/// Remove the key called `name`. `true` if there was one.
pub fn delete(vault: &Vault, name: &str) -> Result<bool, KeyStoreError> {
    check_name(name)?;
    let _guard = lock();
    vault.delete(name)
}

/// Read, check and unseal; migrate an older accepted kind.
fn load<K: SecretKey>(vault: &Vault, name: &str) -> Result<K, KeyStoreError> {
    check_name(name)?;
    let stored = {
        let _guard = lock();
        vault.get(name)?
    };
    let (flags, envelope) = stored.ok_or(KeyStoreError::NotFound)?;
    // Software refuses on a flags byte; it never grants on one.
    if flags & !vault.supported_flags() != 0 {
        return Err(KeyStoreError::UnsupportedFlags(flags));
    }
    let (kind, bytes) = parse(&envelope)?;
    if !K::accepts(kind) {
        return Err(KeyStoreError::WrongKind(kind.to_owned()));
    }
    let token = SealToken(());
    let key = K::unseal(kind, bytes, &token).map_err(KeyStoreError::Corrupt)?;
    if kind != K::KIND
        && let Err(error) = migrate(vault, name, flags, &envelope, &key, &token)
    {
        log::warn!("key store: {name}: kept as {kind:?}, rewrite failed: {error}");
    }
    Ok(key)
}

/// Seal `key` again under `K::KIND`, unless the stored envelope changed
/// since it was read (a concurrent `generate` wins).
fn migrate<K: SecretKey>(
    vault: &Vault,
    name: &str,
    flags: u8,
    read: &[u8],
    key: &K,
    token: &SealToken,
) -> Result<(), KeyStoreError> {
    let envelope = envelope::<K>(&key.seal(token))?;
    let _guard = lock();
    match vault.get(name)? {
        Some((now_flags, now)) if now_flags == flags && now.as_slice() == read => {
            vault.put(name, flags, &envelope)
        }
        _ => Ok(()),
    }
}

/// `[kind length: u8][kind][sealed bytes]`.
fn envelope<K: SecretKey>(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, KeyStoreError> {
    let kind = K::KIND.as_bytes();
    if kind.is_empty() || kind.len() > usize::from(u8::MAX) {
        return Err(KeyStoreError::Platform(format!(
            "kind length {} is not 1 to 255",
            kind.len()
        )));
    }
    let len = 1 + kind.len() + sealed.len();
    if len > MAX_ENVELOPE_LEN {
        return Err(KeyStoreError::Platform(format!(
            "sealed key of {len} bytes is over {MAX_ENVELOPE_LEN}"
        )));
    }
    let mut out = Zeroizing::new(Vec::with_capacity(len));
    out.push(kind.len() as u8);
    out.extend_from_slice(kind);
    out.extend_from_slice(sealed);
    Ok(out)
}

fn parse(envelope: &[u8]) -> Result<(&str, &[u8]), KeyStoreError> {
    let (&len, rest) = envelope
        .split_first()
        .ok_or_else(|| KeyStoreError::Corrupt("empty envelope".into()))?;
    let len = usize::from(len);
    if len == 0 || rest.len() < len {
        return Err(KeyStoreError::Corrupt("bad kind length".into()));
    }
    let (kind, bytes) = rest.split_at(len);
    let kind = std::str::from_utf8(kind)
        .map_err(|_| KeyStoreError::Corrupt("kind is not UTF-8".into()))?;
    Ok((kind, bytes))
}

/// 1 to [`MAX_NAME_LEN`] of `[A-Za-z0-9._-]`, not starting with `.`. For key
/// names and namespaces.
pub(crate) fn check_name(name: &str) -> Result<(), KeyStoreError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(KeyStoreError::InvalidName)
    }
}

#[doc(hidden)]
pub const ENGINE_SOURCE: &str = include_str!("key_store.rs");

#[cfg(all(test, feature = "test-vault"))]
// The thread_local below is `const`; the lint misfires on some targets.
#[allow(clippy::missing_const_for_thread_local)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU8, Ordering};
    use zeroize::{Zeroize, ZeroizeOnDrop};

    static NEXT: AtomicU8 = AtomicU8::new(1);

    /// A test adapter: 32 bytes, distinct for each key, public = the bytes.
    #[derive(Zeroize, ZeroizeOnDrop)]
    struct V1(Box<[u8; 32]>);

    impl SecretKey for V1 {
        type Public = [u8; 32];
        const KIND: &'static str = "test.key.v1";
        fn generate(_: &SealToken) -> Self {
            Self(Box::new([NEXT.fetch_add(1, Ordering::Relaxed); 32]))
        }
        fn seal(&self, _: &SealToken) -> Zeroizing<Vec<u8>> {
            Zeroizing::new(self.0.to_vec())
        }
        fn unseal(_: &str, bytes: &[u8], _: &SealToken) -> Result<Self, String> {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| "not 32 bytes".to_string())?;
            Ok(Self(Box::new(bytes)))
        }
        fn public(&self) -> [u8; 32] {
            *self.0
        }
    }

    // When set, `V2::unseal` of a v1 key generates a new v2 key at the same
    // place first: a concurrent `generate` during the migration.
    thread_local! {
        static RACE: RefCell<Option<(Vault, &'static str)>> = const { RefCell::new(None) };
    }

    /// The v2 encoding: the v1 bytes reversed. Reads v1.
    #[derive(Zeroize, ZeroizeOnDrop)]
    struct V2(Box<[u8; 32]>);

    impl SecretKey for V2 {
        type Public = [u8; 32];
        const KIND: &'static str = "test.key.v2";
        fn accepts(kind: &str) -> bool {
            kind == Self::KIND || kind == V1::KIND
        }
        fn generate(_: &SealToken) -> Self {
            Self(Box::new([NEXT.fetch_add(1, Ordering::Relaxed); 32]))
        }
        fn seal(&self, _: &SealToken) -> Zeroizing<Vec<u8>> {
            let mut out = self.0.to_vec();
            out.reverse();
            Zeroizing::new(out)
        }
        fn unseal(kind: &str, bytes: &[u8], _: &SealToken) -> Result<Self, String> {
            if kind == V1::KIND
                && let Some((vault, name)) = RACE.with(|race| race.borrow_mut().take())
            {
                generate::<V2>(&vault, name, 0).unwrap();
            }
            let mut bytes: [u8; 32] = bytes.try_into().map_err(|_| "not 32 bytes".to_string())?;
            if kind == Self::KIND {
                bytes.reverse();
            }
            Ok(Self(Box::new(bytes)))
        }
        fn public(&self) -> [u8; 32] {
            *self.0
        }
    }

    /// The stored kind of `name`.
    fn stored_kind(vault: &Vault, name: &str) -> String {
        let (_, envelope) = vault.get(name).unwrap().unwrap();
        parse(&envelope).unwrap().0.to_owned()
    }

    #[test]
    fn generate_public_and_replace() {
        let vault = Vault::memory();
        let first = generate::<V1>(&vault, "k", 0).unwrap();
        assert_eq!(public::<V1>(&vault, "k"), Ok(first));
        let second = generate::<V1>(&vault, "k", 0).unwrap();
        assert_ne!(first, second);
        assert_eq!(public::<V1>(&vault, "k"), Ok(second));
    }

    #[test]
    fn with_key_lends_and_may_reenter() {
        let vault = Vault::memory();
        let public = generate::<V1>(&vault, "k", 0).unwrap();
        let seen = with_key::<V1, _>(&vault, "k", |key| {
            // The store again, from inside the closure.
            generate::<V1>(&vault, "other", 0).unwrap();
            assert!(super::public::<V1>(&vault, "other").is_ok());
            key.public()
        });
        assert_eq!(seen, Ok(public));
    }

    #[test]
    fn kinds() {
        let vault = Vault::memory();
        generate::<V2>(&vault, "k", 0).unwrap();
        assert_eq!(
            public::<V1>(&vault, "k"),
            Err(KeyStoreError::WrongKind("test.key.v2".into()))
        );
    }

    #[test]
    fn migration_keeps_the_key_and_flags() {
        let vault = Vault::memory();
        let public = generate::<V1>(&vault, "k", FLAG_DEVICE_UNLOCK_REQUIRED).unwrap();
        assert_eq!(super::public::<V2>(&vault, "k"), Ok(public));
        assert_eq!(stored_kind(&vault, "k"), V2::KIND);
        assert_eq!(
            vault.get("k").unwrap().unwrap().0,
            FLAG_DEVICE_UNLOCK_REQUIRED
        );
        assert_eq!(super::public::<V2>(&vault, "k"), Ok(public));
    }

    #[test]
    fn migration_never_overwrites_a_concurrent_generate() {
        let vault = Vault::memory();
        let old = generate::<V1>(&vault, "k", 0).unwrap();
        RACE.with(|race| *race.borrow_mut() = Some((vault.clone(), "k")));
        // Reads the v1 key; its unseal generates a new v2 key before the
        // migration writes.
        assert_eq!(public::<V2>(&vault, "k"), Ok(old));
        let now = public::<V2>(&vault, "k").unwrap();
        assert_ne!(now, old, "the migration overwrote the newer key");
    }

    #[test]
    fn flags_are_refused_unless_enforced() {
        let vault = Vault::memory();
        assert_eq!(
            generate::<V1>(&vault, "k", FLAG_APP_UNLOCK_REQUIRED),
            Err(KeyStoreError::UnsupportedFlags(0b10))
        );
        assert_eq!(
            generate::<V1>(&vault, "k", 1 << 5),
            Err(KeyStoreError::UnsupportedFlags(1 << 5))
        );
        // Written by a build that enforces app unlock: never handed out here.
        let envelope = envelope::<V1>(&[7; 32]).unwrap();
        vault.put("k", FLAG_APP_UNLOCK_REQUIRED, &envelope).unwrap();
        assert_eq!(
            with_key::<V1, _>(&vault, "k", |_| ()),
            Err(KeyStoreError::UnsupportedFlags(0b10))
        );
        assert_eq!(
            public::<V1>(&vault, "k"),
            Err(KeyStoreError::UnsupportedFlags(0b10))
        );
    }

    #[test]
    fn device_lock() {
        let vault = Vault::memory();
        generate::<V1>(&vault, "locked", FLAG_DEVICE_UNLOCK_REQUIRED).unwrap();
        generate::<V1>(&vault, "open", 0).unwrap();
        vault.set_device_locked(true);
        assert_eq!(
            public::<V1>(&vault, "locked"),
            Err(KeyStoreError::DeviceLocked)
        );
        assert!(public::<V1>(&vault, "open").is_ok());
        vault.set_device_locked(false);
        assert!(public::<V1>(&vault, "locked").is_ok());
    }

    #[test]
    fn malformed_missing_and_bad_names() {
        let vault = Vault::memory();
        vault.put("empty", 0, &[]).unwrap();
        vault.put("short", 0, &[9, b'a']).unwrap();
        for name in ["empty", "short"] {
            assert!(matches!(
                public::<V1>(&vault, name),
                Err(KeyStoreError::Corrupt(_))
            ));
        }
        let envelope = envelope::<V1>(&[1, 2, 3]).unwrap();
        vault.put("truncated", 0, &envelope).unwrap();
        assert!(matches!(
            public::<V1>(&vault, "truncated"),
            Err(KeyStoreError::Corrupt(_))
        ));
        assert_eq!(public::<V1>(&vault, "absent"), Err(KeyStoreError::NotFound));
        assert_eq!(public::<V1>(&vault, ".x"), Err(KeyStoreError::InvalidName));
        assert_eq!(delete(&vault, "absent"), Ok(false));
        assert_eq!(delete(&vault, "empty"), Ok(true));
    }

    #[test]
    fn memory_vaults_are_separate() {
        let (a, b) = (Vault::memory(), Vault::memory());
        generate::<V1>(&a, "k", 0).unwrap();
        assert_eq!(public::<V1>(&b, "k"), Err(KeyStoreError::NotFound));
    }
}
