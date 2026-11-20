//! Free space, the cache quota, and periodic readings
//! (`disk_space/DiskSpacePlugin.kt`, feature `disk-space`).
//!
//! [`space`] adds to `statvfs` what only Android knows:
//! `StorageManager.getAllocatableBytes` (the bytes the system could give
//! the app if it also cleared other apps' cached data) and
//! `getCacheQuotaBytes` (how much cached data this app may keep before its
//! cache is cleared first). Android sends nothing when storage runs low on
//! API 30 and later, so [`watch`] reads `statvfs` on a slow Kotlin timer
//! while the activity is resumed and posts every reading as
//! `MobileEvent::DiskSpace`. Nothing is graded here.
//!
//! **The reading never runs per tick or per frame.** It runs once when the
//! activity resumes and every `interval` after that, by a `Handler` on the
//! main looper, only while the activity is resumed: `onPause` removes the
//! callback, so a paused app has no timer. The only per-tick cost is
//! draining one more `mailbox::Latest` slot.

pub use crate::disk_space::{DiskSpace, DiskSpaceError, statvfs_space};

use super::jni_util::with_application_class;
use crate::mailbox::Latest;
use jni::objects::{JLongArray, JObject, JString};
use jni::{EnvUnowned, jni_sig, jni_str};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.disk_space.DiskSpacePlugin";

/// The newest reading of the watched volume, drained by `pump_bridges`.
static PENDING: Latest<DiskSpace> = Latest::new();

/// The watched path; `None` while nothing is watched.
static WATCHED: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(crate) fn take_pending() -> Option<DiskSpace> {
    PENDING.take()
}

/// [`statvfs_space`]`(path)` plus Android's allocatable bytes and this
/// app's cache quota on that volume. Any thread; the `StorageManager` part
/// may block for tens of milliseconds, so not on the loop thread every
/// frame. A failure of the `StorageManager` part leaves those two fields
/// `None` and is logged at debug; only `statvfs` failing is an error.
pub fn space(path: &Path) -> Result<DiskSpace, DiskSpaceError> {
    let mut space = statvfs_space(path)?;
    let started = Instant::now();
    let answer = storage_manager(path);
    let took = started.elapsed();
    match answer {
        Ok([allocatable, quota]) => {
            space.allocatable = u64::try_from(allocatable).ok();
            space.cache_quota = u64::try_from(quota).ok();
            if space.allocatable.is_none() || space.cache_quota.is_none() {
                log::debug!(
                    "libmobile: disk space: StorageManager could not say for {} \
                     (allocatable {allocatable}, cache quota {quota})",
                    path.display()
                );
            }
        }
        Err(err) => log::debug!("libmobile: disk space: StorageManager failed: {err}"),
    }
    log::debug!(
        "libmobile: disk space: StorageManager part took {} us",
        took.as_micros()
    );
    Ok(space)
}

/// Read the volume holding `path` (`files_dir` when `None`) on resume and
/// every `interval` while resumed, posting each reading as
/// `MobileEvent::DiskSpace`. Replaces an earlier watch. Any thread.
/// An interval below 1 s is taken as 1 s. If the activity is resumed, the
/// first reading is taken at once.
///
/// [`DiskSpaceError::Path`] if `path` cannot be read now (nothing is
/// watched then).
pub fn watch(path: Option<&Path>, interval: Duration) -> Result<(), DiskSpaceError> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => super::dirs::files_dir()
            .ok_or_else(|| DiskSpaceError::Platform("no files directory".into()))?,
    };
    statvfs_space(&path)?;
    *WATCHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    let ms = crate::disk_space::watch_interval_ms(interval);
    let result = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        env.call_static_method(
            class,
            jni_str!("setInterval"),
            jni_sig!("(J)V"),
            &[ms.into()],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        *WATCHED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Err(platform(err));
    }
    log::debug!("libmobile: disk watch every {ms} ms");
    Ok(())
}

/// Stop: the timer is removed, nothing more is posted, and a reading not
/// yet delivered is dropped.
pub fn unwatch() {
    *WATCHED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let result = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        env.call_static_method(class, jni_str!("stop"), jni_sig!("()V"), &[])?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: stopping the disk watch failed: {err}");
    }
    let _ = PENDING.take();
}

/// `DiskSpacePlugin.space(Context, String) -> long[2]`: `[allocatable,
/// cacheQuota]`, each `-1` when its call threw.
fn storage_manager(path: &Path) -> Result<[i64; 2], DiskSpaceError> {
    with_application_class(PLUGIN_CLASS, |env, context, class| {
        let path = JString::from_str(env, path.to_string_lossy())?;
        let called = env.call_static_method(
            class,
            jni_str!("space"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)[J"),
            &[context.into(), (&path).into()],
        );
        let value = match called {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                return Err(jni::errors::Error::JavaException);
            }
            other => other?.l()?,
        };
        let array = env.cast_local::<JLongArray>(value)?;
        let mut out = [-1i64; 2];
        if !array.is_null() && array.len(env)? >= 2 {
            array.get_region(env, 0, &mut out)?;
        }
        Ok(out)
    })
    .map_err(platform)
}

fn platform(err: jni::errors::Error) -> DiskSpaceError {
    match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => DiskSpaceError::Platform(msg),
        other => DiskSpaceError::Platform(other.to_string()),
    }
}

/// One timer reading: `statvfs` of the watched path into [`PENDING`].
fn check() {
    let Some(path) = WATCHED.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return;
    };
    match statvfs_space(&path) {
        Ok(space) => {
            log::debug!(
                "libmobile: disk check {}: free {} of {}",
                path.display(),
                space.free,
                space.total
            );
            PENDING.set(space);
        }
        Err(err) => log::warn!("libmobile: disk check failed: {err}"),
    }
}

/// Kotlin -> Rust: `DiskSpacePlugin.nativeDiskCheck()`, on the main looper.
/// An instance method of the Kotlin `object`. The package's `_` is `_1` in
/// the JNI symbol.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_disk_1space_DiskSpacePlugin_nativeDiskCheck<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
) {
    // `with_env` wraps the body in catch_unwind, so a panic can never unwind
    // across the FFI boundary into the JVM.
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        check();
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
