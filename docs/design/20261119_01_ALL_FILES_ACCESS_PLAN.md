---
title: All files access, the shared storage root, and telling MediaStore about changed files
date: 2026-11-19
status: Draft
covers:
  - "Feature `all-files-access`: `android::all_files_access::state`, `open_settings` and `external_storage_dir`, through `Environment` and the system's All files access page"
  - "Feature `media-scan`: `android::media_scan::scan_paths`, through `MediaScannerConnection.scanFile`"
  - "`gradle::Project::all_files_access()`, which declares `MANAGE_EXTERNAL_STORAGE`"
sources:
  - "ShotToss `docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md` (Draft), section 4.2: the access check, the settings intent with its fallback, and the media scan, written there as ShotToss's own JNI in `access.rs`, with open question 2 asking to move them into libmobile"
  - "Owner (2026-11-19, in the ShotToss session, verbatim): \"generic JNI like access checks should be added to libmorph/libmobile as a plan (or through a session if one is open)\""
  - "libmobile today (0.5.2, `75fb7d9`): no storage-access call. The pattern followed is the `battery` feature: a feature plugin with static Kotlin methods called through `jni_util::with_application_class`, `\"<kind>: <detail>\"` error strings, a visibility check before starting a settings page, and a `Project` method that declares a sensitive permission (`request_ignore_battery_optimizations`)"
  - "Android: `Environment.isExternalStorageManager()` and `Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION` (API 30); `Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION` (API 30); `Environment.getExternalStorageDirectory()` (deprecated in API 29, still the path of the primary shared volume); `MediaScannerConnection.scanFile(Context, String[], String[], OnScanCompletedListener)` (API 8)"
related:
  - libmorph docs/design/20261119_01_ALL_FILES_ACCESS_PLAN.md (wraps it on `MorphContext`)
  - ShotToss docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md (the first user)
  - docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md (the pattern)
---

# All files access, the shared storage root, and telling MediaStore about changed files

## 1. What it is

Two small features for an app that works on the phone's shared storage
directly, through POSIX paths, as ShotToss (`dev.tmzt.shottoss`), a
gallery and file manager, does:

- `all-files-access`: whether the app holds All files access
  (`MANAGE_EXTERNAL_STORAGE`), opening the system page where the user turns
  it on, and the path of the primary shared volume (`/storage/emulated/0`
  on most phones), so apps stop writing `/sdcard` by hand.
- `media-scan`: telling MediaStore that files were created, changed,
  moved or deleted, so other apps (Google Photos, the Files app) see the
  change without waiting for the next full scan.

They are two features because a media scan needs no permission and is
useful to any app that writes to shared storage (a camera app saving to
`DCIM`, say), while All files access is a permission Google Play restricts.

`MANAGE_EXTERNAL_STORAGE` is not a runtime permission: `request_permissions`
cannot ask for it, and `permission_granted` always answers false. It is
declared in the manifest and granted only by the user on a settings page.

### Not in this plan

- Android 10's `requestLegacyExternalStorage` and `READ_EXTERNAL_STORAGE`
  route. The generated projects' default `minSdk` is 29; on API 29 these
  calls answer `Unsupported` (2.3).
- Telling the app when the grant changes. Android sends nothing; the app
  reads `state()` again when it resumes (3). A `SystemEvent` for it, like
  `BatteryExemption`, can be a later amendment if an app needs one.
- Secondary volumes (SD cards, USB drives) through `StorageManager`.

## 2. API

### 2.1 Platform-free types

`src/all_files_access.rs` (`libmobile::all_files_access`), feature
`all-files-access`, built and tested on any host:

```rust
/// Whether the app holds All files access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AllFilesAccess {
    /// `Environment.isExternalStorageManager()` is true.
    Granted,
    /// It is false: the user has not turned it on, or turned it off.
    Missing,
}

/// Which settings page opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessPage {
    /// `ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION` for this app: one switch.
    App,
    /// `ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION`: the list of every app
    /// that declares the permission, where the user finds this one. Opened
    /// only when the app's own page does not resolve (some vendor builds).
    List,
}

/// Why an All files access call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AllFilesAccessError {
    /// The manifest does not declare `MANAGE_EXTERNAL_STORAGE`; the build
    /// script calls `gradle_project(id).all_files_access()`.
    NotDeclared,
    /// Below Android 11 (API 30), where the permission and its page do not exist.
    Unsupported,
    /// `open_settings` while the app is not visible.
    NotVisible,
    /// Neither page resolved on this device.
    NoSettingsPage,
    /// Anything else Android or JNI reported.
    Platform(String),
}
// Display and std::error::Error; from_message("<kind>: <detail>") as BatteryError has.
```

`src/media_scan.rs` (`libmobile::media_scan`), feature `media-scan`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaScanError {
    /// A path was not absolute or not valid UTF-8 (`scanFile` takes `String`s).
    BadPath(PathBuf),
    /// More than `MAX_SCAN_PATHS` (1 024) paths in one call.
    TooMany(usize),
    Platform(String),
}
pub const MAX_SCAN_PATHS: usize = 1024;
```

### 2.2 The calls

`src/android/all_files_access.rs` (`libmobile::android::all_files_access`):

```rust
/// Whether the app holds All files access now. Any thread; a short binder
/// call. NotDeclared before Granted/Missing, so a build-script slip is not
/// mistaken for the user's "no".
pub fn state() -> Result<AllFilesAccess, AllFilesAccessError>;

/// Open this app's All files access page, or the list of apps if that
/// does not resolve. The app must be visible. Android reports nothing
/// back; read state() again when the app resumes. Opening it while the
/// access is already granted is allowed (the user may turn it off there).
pub fn open_settings() -> Result<AccessPage, AllFilesAccessError>;

/// The primary shared volume's root (`Environment.getExternalStorageDirectory()`),
/// for example `/storage/emulated/0`. Any thread. Does not check access:
/// reading under it without the grant fails with the OS's own error.
pub fn external_storage_dir() -> Result<PathBuf, AllFilesAccessError>;
```

`src/android/media_scan.rs` (`libmobile::android::media_scan`):

```rust
/// Ask MediaStore to scan these paths: new and changed files are added
/// or updated, and paths that no longer exist are removed from its index.
/// Returns once the request is queued; the scan finishes later, on the
/// system's own thread, and its result is only logged at debug. Any thread.
/// An empty list does nothing.
pub fn scan_paths(paths: &[&Path]) -> Result<(), MediaScanError>;
```

Paths are checked (absolute, UTF-8, at most `MAX_SCAN_PATHS`) in the
platform-free module, before JNI, so that check is host-tested.

### 2.3 How each call works

`all_files_access.AllFilesAccessPlugin`
(`android/java/dev/highbay/libmobile/all_files_access/`), an activity
plugin with no hooks, has static methods that Rust calls with the
Application context, as `BatteryPlugin` does:

- `state(context: Context): String`: below API 30 `"unsupported"`; then,
  from `PackageManager.getPackageInfo(packageName, GET_PERMISSIONS)`,
  `"not-declared"` when `requestedPermissions` lacks
  `android.permission.MANAGE_EXTERNAL_STORAGE`; then `"granted"` or
  `"missing"` from `Environment.isExternalStorageManager()`.
- `openSettings(context: Context): String`: the same `unsupported` and
  `not-declared` checks; the visibility check `BatteryPlugin` uses
  (`"not-visible: importance <n>"`); then `startActivity` of an `Intent`
  with action `ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION`, data
  `Uri.fromParts("package", packageName, null)` and
  `FLAG_ACTIVITY_NEW_TASK`, answering `"app"`. If that throws
  `ActivityNotFoundException`, the same with
  `ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION` and no data, answering
  `"list"`. If both throw, `"no-settings-page"`. Any other exception is
  caught in Kotlin and answered as `"platform: <message>"`, so no Java
  exception is ever left pending when control returns to Rust.
- `externalStorageDir(): String?`: `Environment.getExternalStorageDirectory().absolutePath`,
  or null if Android gives none (no primary volume mounted), which Rust
  answers as `Platform("no external storage")`.

`media_scan.MediaScanPlugin` (`android/java/dev/highbay/libmobile/media_scan/`):

- `scan(context: Context, paths: Array<String>): String?`:
  `MediaScannerConnection.scanFile(context, paths, null, listener)`, the
  listener logging each `(path, uri)` at debug under the `libmobile` tag;
  null on success, `"platform: <message>"` on an exception.

On the Rust side, a `jni::errors::Error::CaughtJavaException` (which the
`jni` 0.22 helpers already clear) becomes `Platform` with its message, as
in `android/battery.rs`.

### 2.4 Declaring the permission

`gradle.rs`:

```rust
/// Declare `MANAGE_EXTERNAL_STORAGE`, so the app can be granted All files
/// access (`android::all_files_access`, feature `all-files-access`).
/// Google Play allows it only for apps whose core purpose needs it (file
/// managers, backup, and similar); sideloaded apps are not bound by that.
pub fn all_files_access(mut self) -> Self;
```

It adds `android.permission.MANAGE_EXTERNAL_STORAGE` to the app's
permissions once, as `request_ignore_battery_optimizations` does, with
`tools:ignore="ScopedStorage"` on that `<uses-permission>` element so
Android lint's ScopedStorage check does not warn on the app's own,
intended choice. If the manifest writer cannot yet add a `tools:`
attribute to one element, step 1 adds that (and the `xmlns:tools`
declaration on `<manifest>`) as part of this method.

The feature does not add the permission by itself: an app that only
checks, and is built for Play without it, gets `NotDeclared` rather than a
manifest it did not ask for.

## 3. Re-checking on resume

Android does not tell the app when the user changes the switch. The app
reads `state()` again when it comes back to the foreground; in libmobile's
loop that is the `MobileEvent::Resumed` it already delivers, and in
libmorph `MorphApp::on_resume`. The page is a separate activity in the
Settings task, so going to it pauses the app and Back resumes it: one
read on resume always sees the switch's latest value. If the user turns
the access off from Settings while the app is in the background, Android
may also kill the app's process; either way the next `init` or resume
reads the new value. Step 1 records which the S23 does.

## 4. Files

- `Cargo.toml`: `all-files-access = []` and `media-scan = []`, each with a
  comment, after `clipboard`.
- `src/all_files_access.rs`, `src/media_scan.rs`: the types, `from_message`,
  path checks, tests.
- `src/lib.rs`: both modules behind their features.
- `src/android/all_files_access.rs`, `src/android/media_scan.rs`: the calls.
- `src/android/mod.rs`: both modules behind their features.
- `android/java/dev/highbay/libmobile/all_files_access/AllFilesAccessPlugin.kt`,
  `android/java/dev/highbay/libmobile/media_scan/MediaScanPlugin.kt`.
- `src/codegen.rs`: two `Plugin` entries (`contributes: NOTHING`), the two
  names in `known_features`, and a test that each is gated and contributes
  no permission.
- `src/gradle.rs`: `Project::all_files_access`, with a test that the
  generated manifest declares the permission once, with
  `tools:ignore="ScopedStorage"`, after two calls.
- `examples/wgpu`: both features, `.all_files_access()` in its build
  script, and `--es cmd files-access` (logs `state()` and
  `external_storage_dir()`), `--es cmd files-access-open` (logs the page
  opened), `--es cmd media-scan --es path <path>` (logs the result).
- `README.md`: a short section under the features.

## 5. Versioning

Additive: two features, four new modules, new `#[non_exhaustive]` types
and one new `Project` method. libmobile 0.5.2 to **0.5.3**, tagged
`v0.5.3`, with a `VERSIONS.md` entry. libmorph and libmorph-build take it
without a minor bump of their own.

## 6. Steps

1. **The two features**: the types, the Kotlin plugins, the calls, the
   registry entries, `Project::all_files_access`, the tests and the wgpu
   example's commands.
   **Done when:** `cargo fmt --all -- --check`; host `cargo clippy
   --workspace --all-targets` with no features and `--all-features`;
   `cargo clippy --target aarch64-linux-android --workspace --all-targets`
   with no features, `--features all-files-access`, `--features
   media-scan` and `--all-features`: no warnings; `cargo test --workspace`
   and `cargo test -p libmobile --all-features` pass; `examples/build.sh
   wgpu` builds with lint passing and `dexdump` shows both plugin classes;
   on the owner's S23: `files-access` logs `Missing` and
   `/storage/emulated/0` on a fresh install; `files-access-open` opens the
   app's own page and logs `App`; after turning the switch on and pressing
   Back, `files-access` logs `Granted`; `media-scan` on a photo deleted
   with `adb shell rm` removes it from Google Photos after Photos
   refreshes; a build of the example without `.all_files_access()` logs
   `NotDeclared`.
2. **Release** as libmobile 0.5.3 on the owner's word: the README section,
   `Cargo.toml`'s version, the `VERSIONS.md` entry, the tag.
   **Done when:** the commit is tagged `v0.5.3` and libmorph builds
   against it.

## 7. Risks

- **Vendor pages.** Some builds have no per-app page; the list fallback
  covers them, and `NoSettingsPage` is the honest answer when neither
  resolves.
- **Lint.** If AGP's lint treats `MANAGE_EXTERNAL_STORAGE` as an error in
  a release build despite the `tools:ignore`, step 1 records the lint id
  it names and adds that to the same attribute (a recorded departure).
- **Play.** The permission's restriction is Play's policy, not Android's;
  the method's documentation says so, as `battery`'s does.
