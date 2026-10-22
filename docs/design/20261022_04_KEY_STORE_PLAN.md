---
title: Key store for adapter-defined keys
date: 2026-10-22
status: Draft
covers:
  - "`SecretKey`, `SealToken`, `KeyHandle`, `KeyOptions` and `KeyStoreError`:
    platform-free types an adapter implements against, so its private key
    can be generated, kept and validated by the store without the app
    reading it"
  - "`android::KeyStore`: `generate`, `generate_with`, `load`, `delete`,
    with storage separate from the blob store"
  - A `space` argument for `store/SecureStorePlugin.kt`, so blobs and keys
    keep separate directories and separate Keystore wrapping keys
sources:
  - libmorph docs/design/20261022_04_OPAQUE_KEYS_PLAN.md, amendment "a key
    store of its own, built in libmobile and libdevice first". This plan is
    its step 1. It fixes the API that libdevice's plan (its step 2) repeats
    and that libmorph wraps.
  - "Owner (2026-10-22): the key management logic stays in the black box
    passed into the secure store; the generic aspect is a type (K) and (P)
    (public opaque key/handle); keys are a specific API, not a meaning
    given to blob names."
  - docs/design/20261022_03_SECURE_STORE_PLAN.md (the blob store, its
    plugin, its file layout and its StrongBox and unlock rules, reused
    here)
related:
  - docs/design/20261022_03_SECURE_STORE_PLAN.md
  - libdevice's key store plan (to be written; same API on the desktop)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md (its Noise adapter
    implements `SecretKey` through libmorph)
---

# Key store for adapter-defined keys

## 1. Goal

An adapter, a crate that knows a key's algorithm (handlr's Noise adapter,
for example), defines its private key type `K` and a public opaque type
`P`. The store generates `K`, keeps it sealed, loads and validates it, and
hands out a handle. Code outside the adapter gets `P`, and can pass `K`
around, but cannot read it:

- `K`'s fields are private to the adapter's crate.
- Turning `K` into bytes and back needs a `SealToken`, which only libmobile
  can make. The adapter writes `seal` and `unseal`, but only the store can
  call them.

This is not the blob store. Keys have their own API, error type and
storage, and a key and a blob may share a name without meeting. The store
has no algorithm, and libmobile gains no cryptography dependency.

## 2. Types (platform-free, `src/key_store.rs`)

Built on every host, so libmorph and libdevice can match them field for
field. Re-exported at the crate root.

```rust
/// Proof that the store is the caller. Only libmobile can make one.
pub struct SealToken(());

/// A private key type, implemented by its adapter. Keep its fields private:
/// whoever can see them can read the key.
pub trait SecretKey: Sized + Send + Sync + 'static {
    /// The public opaque value: a public key, or any handle the adapter
    /// chooses to expose.
    type Public: Clone + Send + Sync + 'static;
    /// The key type and its encoding, e.g. "handlr.noise.x25519.v1". 1 to
    /// 255 bytes of UTF-8. A key stored as one kind never loads as another.
    const KIND: &'static str;
    /// A new key, from the adapter's own randomness.
    fn generate(token: SealToken) -> Self;
    fn seal(&self, token: SealToken) -> Vec<u8>;
    /// Decode and validate. An error is reported as `KeyStoreError::Corrupt`.
    fn unseal(bytes: &[u8], token: SealToken) -> Result<Self, String>;
    fn public(&self) -> Self::Public;
}

/// A loaded key. Cheap to clone.
pub struct KeyHandle<K: SecretKey> { key: Arc<K>, public: K::Public }
impl<K: SecretKey> KeyHandle<K> {
    pub fn public(&self) -> &K::Public;
    /// For the adapter's own operations; opaque to everyone else.
    pub fn key(&self) -> &K;
}
impl<K: SecretKey> Clone for KeyHandle<K> { /* clones the Arc and the public value */ }

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct KeyOptions {
    /// Sealed under a wrapping key the platform refuses while the device is
    /// locked; `load` gives `Locked` until it is unlocked.
    pub unlocked_only: bool,
}
impl KeyOptions { pub fn unlocked_only(self, on: bool) -> Self; }

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KeyStoreError {
    /// Not 1 to 64 of [A-Za-z0-9._-], or starts with '.'. The blob rule.
    InvalidName,
    /// Stored as another kind; the stored kind is given.
    WrongKind(String),
    /// `unseal` refused the bytes, the sealed form is malformed, or it fails
    /// to decrypt.
    Corrupt(String),
    /// Stored with `unlocked_only` and the device is locked.
    Locked,
    /// The platform failed (Keystore, files, JNI).
    Platform(String),
}
```

`impl Display` and `std::error::Error` for `KeyStoreError`.

### 2.1 The sealed form

`seal`'s output is wrapped as `[kind length: u8][kind: UTF-8][sealed
bytes]`, and that whole buffer is what the platform encrypts, so the kind
is authenticated with the key. `load::<K>` decrypts, compares the kind with
`K::KIND` (`WrongKind` if they differ), then calls `K::unseal`. The store
clears its own copies of the buffer (`fill(0)`) after use. Clearing `K` on
drop is the adapter's job, for example with zeroize.

The sealed buffer is limited to 64 KiB, as a blob is.

## 3. Android (`src/android/key_store.rs`)

```rust
/// Cheap to clone, Send + Sync. Holds a clone of the AndroidApp.
#[derive(Clone)]
pub struct KeyStore { app: AndroidApp }

impl KeyStore {
    pub fn new(app: &AndroidApp) -> Self;
    /// A new key called `name`, replacing any earlier one.
    pub fn generate<K: SecretKey>(&self, name: &str) -> Result<KeyHandle<K>, KeyStoreError>;
    pub fn generate_with<K: SecretKey>(&self, name: &str, options: KeyOptions)
        -> Result<KeyHandle<K>, KeyStoreError>;
    /// `None` if there is no key called `name`.
    pub fn load<K: SecretKey>(&self, name: &str) -> Result<Option<KeyHandle<K>>, KeyStoreError>;
    /// `true` if there was one.
    pub fn delete(&self, name: &str) -> Result<bool, KeyStoreError>;
}
```

The calls are synchronous and can be made from any thread, as the blob
store's are.

### 3.1 Storage

`SecureStorePlugin` gains a `space: Int` first argument on `get`, `put`,
`delete` and `securityLevel`: `0` for blobs, `1` for keys. Each space has
its own directory and its own pair of Keystore wrapping keys:

| Space | Directory under `noBackupFilesDir` | Wrapping keys |
|---|---|---|
| 0 blobs | `libmobile-secure-store/` | `libmobile.secure-store.v1`, `libmobile.secure-store.unlocked.v1` |
| 1 keys | `libmobile-key-store/` | `libmobile.key-store.v1`, `libmobile.key-store.unlocked.v1` |

The file layout, the associated data (header and name), the atomic write,
the StrongBox rule with `secure-store-no-strongbox`, and the `locked`
handling are the blob store's, unchanged. The JNI signatures change before
anything is released, so nothing outside libmobile is affected. The blob
functions pass `0`.

Error mapping from the plugin's `"<kind>: <detail>"` messages: `keystore`
and `io` become `Platform`, `corrupt` becomes `Corrupt`, `locked` becomes
`Locked`.

## 4. Steps

1. **Types.** `src/key_store.rs` with the types of section 2, the sealed
   form of 2.1, and the name check shared with the blob store. Host tests:
   the sealed form round-trips; a different kind gives `WrongKind`; a
   truncated or empty buffer gives `Corrupt`; and a `compile_fail` doc test
   shows that `SealToken(())` cannot be built outside the crate.
   **Done when:** `cargo test --workspace` passes.
2. **Plugin spaces.** The `space` argument, the second directory and the
   second pair of wrapping keys, with the blob store passing `0`.
   **Done when:** a generated project builds with `gradle assembleDebug`,
   and the `securestore` example still counts launches on the S23 as
   before.
3. **Android `KeyStore`.** Section 3.
   **Done when:** Android clippy is clean with all features, and
   `cargo test --workspace` passes.
4. **Example on a device.** The `securestore` example gains a test adapter
   in the example crate, `TestKey`: 32 bytes from `/dev/urandom`, with
   `Public` a `u64` fingerprint of the bytes (std's `DefaultHasher`, enough
   for a test). It is "libmobile.example.test.v1". On first launch the
   example generates key `device` and logs its fingerprint; on later
   launches it loads it and logs it. It also writes a blob called `device`
   and checks that the two do not affect each other. Loading `device` as a
   second test type, `OtherKey`, is logged as `WrongKind`.
   **Done when:** on the owner's Galaxy S23 Ultra:
   - the fingerprint is the same across a force-stop;
   - the blob `device` and the key `device` both survive;
   - `OtherKey` logs `WrongKind("libmobile.example.test.v1")`;
   - a key made with `unlocked_only` gives `Locked` with the screen
     locked, and loads with it unlocked;
   - after `pm clear`, a new fingerprint is generated.

## 5. Release

Strictly additive (new types, a new module, new methods; the plugin's JNI
signatures are internal). Released with the blob store as libmobile 0.2.1.

## 6. Open question for the owner

1. **Randomness.** `generate` takes no RNG. The adapter uses its own (for
   Noise, snow's or `OsRng`). Should the store pass one in, so a scripted
   desktop run could make repeatable keys? This matters more to libdevice's
   memory backend than to Android.

## Amendment — 2026-10-22 19:11 EDT: no randomness from the store

Agreed with the owner (2026-10-22). This settles open question 1 of
section 6. `SecretKey::generate` takes no RNG: the adapter uses its own.
Only libmorph's scripted runs (headless, for tests and evidence) could
want repeatable keys, and nothing needs that. A test that needs a known
key uses a test adapter whose `generate` returns a fixed value.
