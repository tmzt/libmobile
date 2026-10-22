---
title: Secure blob store backed by Android Keystore
date: 2026-10-22
status: Draft
covers:
  - A `secure-store` Cargo feature with a Kotlin plugin,
    `store/SecureStorePlugin.kt`
  - `libmobile::android::secure_store`: `get`, `put`, `delete` and
    `info` for small named byte blobs
  - Envelope encryption under a non-exportable AES-256-GCM key in Android
    Keystore (StrongBox when the device has it), with the ciphertext in
    the app's no-backup storage
sources:
  - handlr (owner-approved request, 2026-10-22, from the handlr session).
    The companion app in ~/src/handlr apps/companion pairs with the handlr
    daemon over Noise (`snow`). It needs a long-lived X25519 static private
    key (32 bytes) and a small server record: public key, addresses, name
    and session_id. Both must persist across launches, never leave the
    device, and be protected at rest. snow needs the raw private key bytes,
    so a Keystore key that cannot be exported cannot be the Noise key
    itself. The owner wants no file-based fallback on Android.
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md: its `KeyStore`
    trait (`secret`, `set_secret`, `server`, `set_server`) and
    `ServerRecord`. Its step 4, the Android app, waits on this plan.
  - Android: `KeyGenParameterSpec.Builder` with `KeyProperties.KEY_ALGORITHM_AES`,
    `BLOCK_MODE_GCM`, `ENCRYPTION_PADDING_NONE`, `setKeySize(256)`,
    `setIsStrongBoxBacked` (API 28, `StrongBoxUnavailableException`),
    `KeyInfo.getSecurityLevel` (API 31). `Context.getNoBackupFilesDir()`.
    `androidx.security:security-crypto` (EncryptedSharedPreferences) is
    deprecated, so it is not used; the platform APIs do the same with no
    AAR.
  - The owner's Galaxy S23 Ultra (API 36) reports
    `android.hardware.hardware_keystore=200` and
    `android.hardware.strongbox_keystore=4` (Knox Vault).
  - The camera and permission bridges as the pattern: Rust calls a static
    Kotlin method through `jni_util::with_app_class`.
related:
  - docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md (the bridge
    pattern; no permission is needed here)
  - libmorph docs/design/20261022_03_SECURE_STORE_PLAN.md (wraps this
    plan, and covers the desktop)
---

# Secure blob store backed by Android Keystore

## 1. Goal

An app can keep a few small secrets (keys, tokens, a pairing record) on the
device, encrypted at rest under a key that never leaves the secure
hardware, without writing its own Kotlin. The store holds opaque named
byte blobs. What they mean is the app's business. handlr builds its
`KeyStore` trait on top: one blob for the 32-byte Noise key, one for the
serialized `ServerRecord`.

There is no plaintext fallback. If Keystore cannot give the store a key,
every call fails with an error, and nothing is written.

Nothing changes for apps without the feature.

## 2. Design

### 2.1 Envelope encryption

- One AES-256-GCM key per app, in the `AndroidKeyStore` provider, alias
  `libmobile.secure-store.v1`. Purposes `ENCRYPT | DECRYPT`, block mode
  GCM, no padding, randomized encryption required (Keystore picks the
  12-byte IV). No user authentication is required, so the store works
  without a screen lock prompt. Requiring the device to be unlocked or the
  user to authenticate is left for a later amendment.
- The key is created on first use. StrongBox is tried first
  (`setIsStrongBoxBacked(true)`), and on `StrongBoxUnavailableException`
  the key is created again in the TEE. Software-only Keystore is not
  refused, since the platform does not offer a choice there, but `info`
  reports it (2.4).
- Each blob is encrypted on its own, with the blob's name as GCM
  associated data, so a file renamed to another name fails to decrypt
  rather than being read as the other blob.

### 2.2 Files

- Directory: `<noBackupFilesDir>/libmobile-secure-store/`. The no-backup
  directory keeps the ciphertext out of Auto Backup and device transfer.
  The key does not travel either, so a restored copy could never be
  decrypted anyway.
- One file per blob, named `<name>.blob`. Names are 1 to 64 characters
  from `[A-Za-z0-9._-]`, not starting with `.`, checked in Rust before any
  JNI call.
- File layout, all integers big-endian:

  | Offset | Size | Field |
  |---|---|---|
  | 0 | 4 | magic `LMSS` |
  | 4 | 1 | format version, `1` |
  | 5 | 1 | IV length *n* (12) |
  | 6 | *n* | IV |
  | 6 + *n* | rest | ciphertext with the 16-byte GCM tag at the end |

- Writes go to `<name>.blob.tmp`, are flushed with `FileOutputStream.getFD().sync()`,
  then renamed over `<name>.blob`, so a crash leaves the old or the new
  blob, never half of one.
- Blobs are limited to 64 KiB of plaintext. This is a store for secrets,
  not for data.

### 2.3 Kotlin

`android/java/dev/highbay/libmobile/store/SecureStorePlugin.kt`, staged
and listed in `PLUGINS` only with the `secure-store` feature. It has no
lifecycle hooks; it is an `object` so the codegen lists it like the
others.

```kotlin
object SecureStorePlugin : MobileActivityPlugin {
    /** `null` if the blob does not exist. Throws on any other failure. */
    @JvmStatic fun get(activity: Activity, name: String): ByteArray?
    @JvmStatic fun put(activity: Activity, name: String, value: ByteArray)
    /** `true` if a blob was removed. */
    @JvmStatic fun delete(activity: Activity, name: String): Boolean
    /** 0 software, 1 TEE, 2 StrongBox; creates the key if needed. */
    @JvmStatic fun securityLevel(activity: Activity): Int
}
```

Every method is synchronous and thread-safe (one lock around the key and
the files). Plaintext arrays the plugin creates are cleared with `fill(0)`
before they are released. Failures are thrown as exceptions with a message
of the form `"<kind>: <detail>"`, where *kind* is one of `keystore`,
`io`, `corrupt`, and Rust maps them (2.4).

### 2.4 Rust

`src/android/secure_store.rs`, re-exported as
`libmobile::android::secure_store`, and a platform-free
`src/secure_store.rs` for the types, so they build on any host:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecureStoreError {
    /// Not 1..=64 of [A-Za-z0-9._-], or starts with '.'.
    InvalidName,
    /// Over 64 KiB.
    TooLarge,
    /// Keystore refused to create or use the key.
    Keystore(String),
    /// Reading or writing the blob's file failed.
    Io(String),
    /// The file is malformed, or decryption failed (wrong name, tampering,
    /// or the Keystore key was lost or invalidated). Not treated as
    /// absent: the app decides whether to delete it and start over.
    Corrupt(String),
    /// The JNI call itself failed.
    Jni(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecurityLevel { Software, TrustedEnvironment, StrongBox }

// libmobile::android::secure_store
pub fn get(app: &AndroidApp, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError>;
pub fn put(app: &AndroidApp, name: &str, value: &[u8]) -> Result<(), SecureStoreError>;
pub fn delete(app: &AndroidApp, name: &str) -> Result<bool, SecureStoreError>;
pub fn security_level(app: &AndroidApp) -> Result<SecurityLevel, SecureStoreError>;
```

The calls are synchronous and can be made from any thread:
`with_app_class` attaches the calling thread to the VM. `AndroidApp` is
`Clone + Send`, so an app can move a clone into a worker or into its own
store type (handlr's `KeyStore: Send`). A call costs one Keystore
operation, a few milliseconds in the TEE and more in StrongBox, so an app
should not call it every frame. No zeroizing type is added: the caller owns
the returned `Vec`, and copies in the Java heap cannot be wiped reliably
beyond the plugin's `fill(0)`.

### 2.5 How handlr maps onto it

Not part of libmobile, shown to check the API fits. The 32-byte key is
blob `noise.static`. The `ServerRecord` is blob `server`, serialized by
handlr (TOML, as `FileKeyStore` writes it today). `secret()` returns
`get` with a length check, `set_secret` is `put`, and so on. Through
libmorph, handlr would use libmorph's wrapper rather than this module
(libmorph plan `20261022_03_SECURE_STORE_PLAN.md`).

## 3. Steps

1. **Types and name checks.** `src/secure_store.rs` with
   `SecureStoreError`, `SecurityLevel`, the name and size checks, and the
   mapping of `"<kind>: <detail>"` messages to errors. Host unit tests for
   names, sizes and the mapping.
   **Done when:** `cargo test --workspace` passes.
2. **Kotlin plugin and feature.** The `secure-store` feature,
   `SecureStorePlugin.kt` as in 2.1 to 2.3, staged and listed only under
   the feature (codegen tests updated).
   **Done when:** `cargo test --workspace` passes, and a generated
   project with the feature builds with `gradle assembleDebug`.
3. **Rust bridge.** `src/android/secure_store.rs` as in 2.4, turning
   thrown exceptions into `SecureStoreError` (clearing the pending
   exception).
   **Done when:** `cargo clippy --target aarch64-linux-android
   --workspace --all-features` is clean and `cargo test --workspace`
   passes.
4. **Example on a device.** A `securestore` example (shares the `wgpu`
   example's GPU code, like `audiocapture`): on start it reads blob
   `counter`, logs it, increments it and writes it back, and logs
   `security_level`. A tap deletes the blob.
   **Done when:** on the owner's Galaxy S23 Ultra, the count survives a
   force-stop and relaunch; `security_level` logs `StrongBox`;
   `run-as <package> xxd` on the blob file shows no plaintext; copying the
   blob to another name through `run-as` makes `get` of that name return
   `Corrupt`; and after `pm clear` the count starts again from nothing.

## 4. Release

Strictly additive: a new feature, a new module and new types (both enums
are `#[non_exhaustive]`). Released as libmobile 0.2.1.

## 5. Open questions for the owner

1. **Unlock and authentication.** Should the key require
   `setUnlockedDeviceRequired(true)` (API 28), so blobs cannot be read
   while the device is locked? It blocks a background reconnect while the
   phone is locked, so this plan leaves it off. A per-call
   option could come later.
2. **Feature or core.** This plan puts it behind `secure-store`, like
   `camera`. It has no AAR and no permission, so it could be core instead.

## Amendment — 2026-10-22 18:28 EDT: secure preferences in core, StrongBox as a default feature

Agreed with the owner (2026-10-22). Replaces the feature in `covers`,
the StrongBox paragraph of 2.1, the staging line of 2.3, step 2, step 4's
security level, section 4, and open questions 2 and 3.

- **Core, no feature for the store.** `SecureStorePlugin` is a core
  plugin, listed in `PLUGINS` after `PermissionPlugin` and always
  staged, and `libmobile::android::secure_store` is always built. Owner:
  "the default is secure preferences", meaning envelope encryption under a
  Keystore key, as `EncryptedSharedPreferences` does. Blobs stay in
  no-backup files (2.2), not in `SharedPreferences`, which Auto Backup
  copies by default. Restored blobs could never be decrypted, because
  the key does not travel.
- **StrongBox is a default feature, `secure-store-strongbox`.**
  `default = ["secure-store-strongbox"]`. With it, the key is created
  with `setIsStrongBoxBacked(true)`, and on
  `StrongBoxUnavailableException` it is created again in the TEE, as 2.1
  had it. Without it, the key is created in the TEE only. The feature
  reaches Kotlin as a `BuildConfig`-free constant in the generated
  `GeneratedPlugins.kt` (`SECURE_STORE_STRONGBOX: Boolean`), the
  generated file being the only place codegen already varies by feature.
  The feature applies when the key is first created. An existing key stays
  where it is, and `security_level` reports where it lives.
  StrongBox needs no permission, attestation or Play Integrity, only API 28
  and `android.hardware.strongbox_keystore`. It is a feature because it is
  slower and not on every device. The owner made it a default.
- **What it protects against.** The key cannot be exported, even with
  root. But on a rooted device, code running as the app can still ask
  Keystore to decrypt the blobs. Protection against that (user
  authentication on the key, or attestation checked by a server, as
  Play Integrity does) would be a separate opt-in feature with its own
  amendment, and none is planned.
- **Step 2** becomes: the core plugin and the `secure-store-strongbox`
  feature, with codegen tests for the plugin entry and both values of the
  constant. **Done when:** `cargo test --workspace` passes, and a
  generated project builds with `gradle assembleDebug` with and without
  default features.
- **Step 4**'s security level: `StrongBox` by default, and
  `TrustedEnvironment` when the example is built with
  `default-features = false` and the app's data cleared first.
- **Section 4:** still strictly additive, so still 0.2.1. A new default
  feature turns on only new behaviour, and the core plugin is a new
  Kotlin class that apps not using it never call.
- **Open question 1** (unlocked device) is settled: not required, so a
  background reconnect works while the phone is locked. It may come later
  as an opt-in.

## Amendment — 2026-10-22 18:32 EDT: StrongBox by default, turned off by a negative feature

Owner (2026-10-22): "we should use a negative feature to disable
strongbox if we don't want it enabled, so that leaving out audio (or
camera) doesn't disable a security feature". Replaces the
`secure-store-strongbox` default feature in the amendment "secure
preferences in core, StrongBox as a default feature", and the matching
part of its steps 2 and 4.

- **No default features.** libmobile's `default` stays `[]`. StrongBox
  is tried by default (with the TEE fallback of 2.1). The feature
  `secure-store-no-strongbox` turns that off, so the key is created in
  the TEE only. Turning off default features can therefore never turn off
  StrongBox.
- Cargo features only add. If any crate in the build enables
  `secure-store-no-strongbox`, it applies to the whole app, so only the
  app itself should enable it.
- **Generated constant.** `GeneratedPlugins.kt` gets
  `const val SECURE_STORE_STRONGBOX: Boolean`, which is `false` only under
  `secure-store-no-strongbox`.
- **Step 2:** codegen tests cover both values of the constant, and the
  project builds with and without the feature.
- **Step 4:** `TrustedEnvironment` is checked by building with
  `secure-store-no-strongbox` and clearing the app's data first.
