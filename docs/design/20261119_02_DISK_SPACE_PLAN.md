---
title: Free space, the cache quota, and disk pressure reported when it changes
date: 2026-11-19
status: Draft
covers:
  - "Feature `disk-space`: `disk_space::statvfs_space(path)` (POSIX, any host) and `android::disk_space::space(path)` adding Android's allocatable bytes and the app's cache quota from `StorageManager`"
  - "Why `getAllocatableBytes` and not the storage-low broadcast or `getFreeSpace` for API 30 and later"
  - "Disk pressure graded in Rust (`DiskPressure`: `None`, `Low`, `Critical`, with thresholds and hysteresis), checked by a Kotlin timer while the activity is resumed, and delivered as `MobileEvent::DiskPressure` only when the grade changes, through the insets bridge's `mailbox::Latest` pattern"
sources:
  - "ShotToss `docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md` (Draft) and its coordinator's request of 2026-11-19: disk pressure for trimming its two cache tiers (data_dir thumbnails 256 MiB LRU, cache_dir previews 128 MiB LRU)"
  - "Owner (2026-11-19, in the ShotToss session): disk and graded memory pressure \"might be valuable as libmorph/libmobile plans\"; and on delivery, \"callbacks in libmobile that call JNI exist for things like the cutout code, though this would probably need periodic checking\""
  - "libmobile's insets bridge (`src/android/insets.rs`, `bridge/WindowInsetsPlugin.kt`): a Kotlin plugin calls a native method, which fills a `mailbox::Latest` slot that `pump_bridges` drains each tick; and `android::battery::recheck`, which publishes only a changed value"
  - "libmobile today (0.5.2, `75fb7d9`): no disk-space call; `android::dirs` gives the app's directories; feature plugins call static Kotlin methods through `jni_util::with_application_class` (the `battery` pattern)"
  - "Android: `StorageManager.getUuidForPath(File)`, `getAllocatableBytes(UUID)` and `getCacheQuotaBytes(UUID)` (all API 26); `Intent.ACTION_DEVICE_STORAGE_LOW` (deprecated in API 26 and not delivered to manifest receivers); `StorageManager.ACTION_MANAGE_STORAGE` (API 25); POSIX `statvfs(3)`"
related:
  - libmorph docs/design/20261119_02_DISK_PRESSURE_PLAN.md (wraps it as `MorphApp::on_disk_pressure`)
  - docs/design/20261119_01_ALL_FILES_ACCESS_PLAN.md (the other ShotToss request of the same day; independent)
---

# Free space, the cache quota, and disk pressure reported when it changes

## 1. What it is

A small `disk-space` feature that tells an app how much room the volume
holding a path has, so it can keep its caches within it and stop writing
before the disk fills, and that tells it, on a slow timer and only when
it changes, how short of space that volume is (`DiskPressure`). ShotToss
trims its thumbnail and preview caches by it; libmorph wraps it as a hook
(its own plan).

## 2. Which Android numbers, and why

- **`statvfs` free and total.** `f_bavail * f_frsize` and
  `f_blocks * f_frsize`: the bytes an unprivileged process may write now,
  and the volume's size. POSIX, microseconds, any thread, any path the app
  can stat. The same call works on every Unix host, so it is in the
  platform-free module.
- **`StorageManager.getAllocatableBytes(uuid)`.** The bytes the system
  could give the app if it also cleared other apps' cached data. Android's
  documentation names it as the call to make before writing a large file
  (with `allocateBytes`) since API 26, and it is current through API 35.
  It is at least `statvfs` free, and can be far more on a full phone whose
  caches are large, so a gallery deciding whether to drop its own cache
  sees the space it can really have. It is a binder call into the storage
  service and can take tens of milliseconds, so it is never called per
  frame.
- **`StorageManager.getCacheQuotaBytes(uuid)`.** How much cached data
  Android lets this app keep on that volume before its cache is cleared
  first. An app above its quota loses its cache first when space runs low,
  so it is the natural ceiling for a cache tier in `cacheDir`.
- **Not used:** `ACTION_DEVICE_STORAGE_LOW` and `_OK` (deprecated in API 26
  and no longer delivered to manifest-declared receivers; a dynamic
  receiver sees them only while running, and the threshold behind them is
  hidden); `StorageManager.getStorageBytesUntilLow` (hidden API);
  `File.getFreeSpace`/`getUsableSpace` (the same number as `statvfs`,
  through JNI for nothing).

The volume's UUID comes from `getUuidForPath(File(path))`, which throws
`IOException` for a path on no known volume.

## 3. API

Platform-free, `src/disk_space.rs` (`libmobile::disk_space`), feature
`disk-space`, built and tested on any Unix host:

```rust
/// A volume's space, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskSpace {
    /// `statvfs`: f_bavail * f_frsize.
    pub free: u64,
    /// `statvfs`: f_blocks * f_frsize.
    pub total: u64,
    /// Android: `getAllocatableBytes`; `None` elsewhere or when Android
    /// could not say.
    pub allocatable: Option<u64>,
    /// Android: `getCacheQuotaBytes` for this app on the volume; `None`
    /// elsewhere or when Android could not say.
    pub cache_quota: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskSpaceError {
    /// The path does not exist or cannot be reached (`statvfs`'s errno,
    /// or `getUuidForPath`'s `IOException`).
    Path(String),
    Platform(String),
}

/// `statvfs(path)` alone. Any thread, any Unix host.
pub fn statvfs_space(path: &Path) -> Result<DiskSpace, DiskSpaceError>;
```

`statvfs_space` uses `libc::statvfs` (a new dependency, `libc = "0.2"`,
already in the tree through `jni` and `ndk`), with the products saturating
at `u64::MAX`.

Android, `src/android/disk_space.rs` (`libmobile::android::disk_space`):

```rust
/// statvfs_space(path) plus Android's allocatable bytes and this app's
/// cache quota on that volume. Any thread; the StorageManager part may
/// block for tens of milliseconds, so not on the loop thread every frame.
/// A failure of the StorageManager part leaves those two fields `None`
/// and is logged at debug; only statvfs failing is an error.
pub fn space(path: &Path) -> Result<DiskSpace, DiskSpaceError>;
```

`disk_space.DiskSpacePlugin`
(`android/java/dev/highbay/libmobile/disk_space/`), with no hooks, has one
static method, `space(context: Context, path: String): LongArray`,
returning `[allocatable, cacheQuota]`, each `-1` when its call threw. It
catches every exception itself, so none is left pending. No permission and
no manifest entry.

## 4. Disk pressure, reported when it changes

Android sends nothing when storage runs low on API 30 and later (2), so
libmobile checks on a slow timer and tells the app only when the grade
changes.

### 4.1 The grade

Platform-free, in `src/disk_space.rs`, host-tested:

```rust
/// How short of space a volume is, in increasing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DiskPressure { None, Low, Critical }

/// Where the grades start. All in bytes except `low_fraction_permille`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskThresholds {
    /// Low below max(low_min, total * low_fraction_permille / 1000).
    pub low_min: u64,
    pub low_fraction_permille: u32,
    /// Critical below this.
    pub critical: u64,
    /// A grade clears only when free is this far above its threshold.
    pub hysteresis: u64,
}

impl DiskThresholds {
    /// ShotToss's proposal: Low under max(1 GiB, 5 %), Critical under
    /// 256 MiB, 256 MiB of hysteresis.
    pub const DEFAULT: Self = Self {
        low_min: 1 << 30,
        low_fraction_permille: 50,
        critical: 256 << 20,
        hysteresis: 256 << 20,
    };

    /// The grade for `free` of `total`, given the grade before (`prev`).
    /// Rising is immediate: free below a threshold gives that grade. Falling
    /// needs free at least threshold + hysteresis: from Critical to Low
    /// above critical + hysteresis, to None above low + hysteresis. So a
    /// volume hovering at a threshold gives one report, not one per check.
    pub fn grade(&self, prev: DiskPressure, free: u64, total: u64) -> DiskPressure;
}

/// A change of grade, with the numbers that caused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskPressureReport { pub pressure: DiskPressure, pub space: DiskSpace }
```

The grade is from `statvfs` free, not from `allocatable`: it is the space
that exists without asking Android to clear other apps' caches, and it
costs microseconds.

### 4.2 When it is checked

```rust
/// How often the Kotlin timer checks while the activity is resumed.
pub const DISK_CHECK_INTERVAL: Duration = Duration::from_secs(60);
/// The bounds an app's own interval is clamped to.
pub const DISK_CHECK_INTERVAL_MIN: Duration = Duration::from_secs(15);
pub const DISK_CHECK_INTERVAL_MAX: Duration = Duration::from_secs(600);
```

60 s because other apps fill a phone over minutes (downloads, photos, app
updates), the 256 MiB of hysteresis means a grade cannot flap between
checks, and the app's own large writes are covered by `check_now` before
them rather than by a faster timer. 15 s is the floor so no app turns
this into polling; 10 minutes the ceiling so a resumed app still hears
within one long reading session.

**The check never runs per tick or per frame.** It runs:

1. once when the activity resumes (`onResume`);
2. every `interval` after that, by a `Handler` on the main looper
   (`postDelayed`, re-posted after each run), only while the activity is
   resumed: `onPause` removes the callback, so a paused or backgrounded
   app has no timer at all;
3. when the app calls `check_now`, for example before a batch write.

The only per-tick cost is the main loop's existing check of one more
`mailbox::Latest` slot in `pump_bridges`, an atomic load when nothing was
posted, and a report is posted to that slot only when the grade changes.

### 4.3 The Android pieces

```rust
// src/android/disk_space.rs
/// Start watching the volume holding `path` (the app's files_dir when
/// `None`) with these thresholds and interval (clamped to the bounds).
/// Replaces an earlier watch. Any thread. The first check runs at once.
pub fn watch(path: Option<&Path>, thresholds: DiskThresholds, interval: Duration) -> Result<(), DiskSpaceError>;
/// Stop watching; the timer is removed and no more reports are posted.
pub fn unwatch();
/// Check now, on the calling thread; posts a report if the grade
/// changed, and returns the current grade and space either way.
pub fn check_now() -> Result<DiskPressureReport, DiskSpaceError>;
```

- `disk_space.DiskSpacePlugin`, an activity plugin, gains
  `onResume` (one check, then post the timer), `onPause` (remove it), and
  static `setInterval(ms: Long)` / `stop()` that `watch` and `unwatch`
  call. The timer's runnable calls `private external fun nativeDiskCheck()`.
- `nativeDiskCheck` (Rust, `Java_dev_highbay_libmobile_disk_space_DiskSpacePlugin_nativeDiskCheck`)
  runs `statvfs_space` on the watched path, grades it against the last
  grade held in a `Mutex<Option<Watch>>` (path, thresholds, last grade),
  and on a change pushes a `DiskPressureReport` into
  `static PENDING: Latest<DiskPressureReport>`. No `StorageManager` call
  is made on the timer: the report's `allocatable` and `cache_quota` are
  `None`, and an app that wants them calls `space`.
- `pump_bridges` takes the slot and delivers
  `MobileEvent::DiskPressure(DiskPressureReport)` (`MobileEvent` is
  `#[non_exhaustive]`, so this is additive). The first check after
  `watch` always reports, so the app learns its starting grade.
- Without `watch`, the timer is never posted; the feature alone costs
  nothing at run time.

## 5. Files

- `Cargo.toml`: `disk-space = ["dep:libc"]` with a comment, after
  `clipboard`; `libc = { version = "0.2", optional = true }`.
- `src/disk_space.rs`: the types, `statvfs_space`, `DiskThresholds::grade`,
  the interval constants and clamping, tests on the host (the temp dir's
  volume has `free <= total` and `total > 0`; a missing path is `Path`; the
  saturating products; each rise, each fall only past the hysteresis, no
  change at a threshold's edge, the 5 % term winning on a 64 GiB volume
  and the 1 GiB term on a 16 GiB one).
- `src/event.rs`: `MobileEvent::DiskPressure(DiskPressureReport)`.
- `src/android/mod.rs`'s `pump_bridges`: the slot's drain.
- `src/lib.rs`, `src/android/mod.rs`: the modules behind the feature.
- `src/android/disk_space.rs`, `android/java/dev/highbay/libmobile/disk_space/DiskSpacePlugin.kt`.
- `src/codegen.rs`: a `Plugin` entry (`contributes: NOTHING`), the name in
  `known_features`, a gating test.
- `examples/wgpu`: the feature; `--es cmd disk-space` (logs `space` of
  `files_dir` and of `/storage/emulated/0`, and each call's time);
  `--es cmd disk-watch --es critical <bytes>` (watches `files_dir` with a
  raised `critical`, so the S23's real free space crosses it, and logs
  each `MobileEvent::DiskPressure`).
- `README.md`: a short section under the features.

## 6. Versioning

Additive: a new feature, two modules, `#[non_exhaustive]` types and a
new variant of the `#[non_exhaustive]` `MobileEvent`.
A patch release: **0.5.4** if the all-files access plan's 0.5.3 is
released first, otherwise 0.5.3; tagged and entered in `VERSIONS.md`.

## 7. Steps

1. **The feature**: types, `statvfs_space`, the grade, the Kotlin plugin
   with its timer, `space`, `watch`, `unwatch`, `check_now`, the event,
   the registry entry, tests and the example's commands.
   **Done when:** `cargo fmt --all -- --check`; host `cargo clippy
   --workspace --all-targets` with no features and `--all-features`;
   `cargo clippy --target aarch64-linux-android --workspace --all-targets`
   with no features, `--features disk-space` and `--all-features`: no
   warnings; `cargo test --workspace` and `cargo test -p libmobile
   --all-features` pass; on the owner's S23, `disk-space` logs `free` and
   `total` matching `adb shell df /data` within one block, an
   `allocatable` at least `free`, a non-zero `cache_quota`, and the
   `StorageManager` part's time; `disk-watch` with `critical` set above
   the phone's free space logs one `Critical` report at once and no more
   while the app stays resumed; logcat with the timer's own debug line
   shows one check per 60 s while resumed and none while the app is in
   the background for 5 minutes; after `--es critical` is set back to
   the default, one `None` (or `Low`) report follows within 60 s.
2. **Release** on the owner's word: README section, version,
   `VERSIONS.md` entry, tag.
   **Done when:** the commit is tagged and libmorph builds against it.

## 8. Risks

- **Slow `getAllocatableBytes`.** If the S23 shows it above 50 ms, step 1
  records the time. Nothing changes: the timer already uses `statvfs_space`
  alone, and `space` runs only on the app's request.
- **A fast fill between checks.** A video recording can use 256 MiB in a
  few seconds; a 60 s timer will see it late. The app's own writes are
  covered by `check_now`; another app's are what the timer is for, and
  late by up to a minute is acceptable for trimming a cache.
- **Emulated shared storage.** `/storage/emulated/0` is a FUSE view of
  `/data/media`; `statvfs` there reports the backing `/data` volume, which
  is what ShotToss wants. The example logs both to confirm it.

## Amendment — 2026-11-19 18:32 EST: the platform-free part becomes an engine file shared with libdevice

The owner decided on 2026-11-19, through the ShotToss coordinator, that the
desktop half is built in libdevice and must "share API with libmorph"
(libdevice `docs/design/20261119_01_DISK_SPACE_PLAN.md`, Draft). That
answer is the agreement for this amendment. It replaces the following:

- **Sections 3 and 4.1, where the platform-free code lives.** Everything
  that section 3 put in `src/disk_space.rs` and section 4.1 added
  (`DiskSpace`, `DiskSpaceError`, `statvfs_space`, `DiskPressure`,
  `DiskThresholds` with `DEFAULT` and `grade`, `DiskPressureReport`), and
  section 4.2's `DISK_CHECK_INTERVAL`, `_MIN`, `_MAX`, goes into
  `src/disk_space/engine.rs`. That file is byte-identical with libdevice's,
  ends with `#[doc(hidden)] pub const ENGINE_SOURCE: &str =
  include_str!("engine.rs");`, and holds its own tests, as the key store
  engine does. `src/disk_space.rs` becomes `mod engine; pub use
  engine::*;`. A change to the engine is made here first and copied to
  libdevice in the same release cycle; libmorph's test compares the two
  `ENGINE_SOURCE`s.
- **The engine's API, as libdevice's plan section 3 states it.** Added to
  what this plan named: `DiskSpaceError::Unsupported` (non-Unix hosts;
  never on Android), `DiskThresholds::new`, `DiskThresholds::low_threshold`,
  `DiskPressureReport::new` (the structs are `#[non_exhaustive]`) and
  `clamp_interval`. `statvfs_space` is under `#[cfg(unix)]`, `Unsupported`
  elsewhere, and `libc` becomes a `cfg(unix)` optional dependency.
- **Section 5's test list** is replaced, for the engine, by libdevice's
  plan section 5 (space on a temp dir, the full grading table with
  hysteresis, the interval clamp), which this crate runs too because the
  file is the same.
- **Step 1's Done when** gains: the engine's tests pass here with
  `cargo test -p libmobile --features disk-space` on the host.

Nothing else changes: the Kotlin timer, `android::disk_space::{space,
watch, unwatch, check_now}`, `MobileEvent::DiskPressure` and the version.
