//! **Free space and the cache quota** (feature `disk-space`): the
//! platform-free part, built and tested on any Unix host. The Android calls
//! (`space`, which adds Android's allocatable bytes and the app's cache
//! quota, and `watch`) are in `android::disk_space`.
//!
//! libmobile reports readings only. How short of space a volume is, and
//! when to tell the app, is the caller's (libmorph grades it).

use std::fmt;
use std::path::Path;
use std::time::Duration;

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

/// Why a disk space reading failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskSpaceError {
    /// The path does not exist or cannot be reached (`statvfs`'s errno,
    /// or `getUuidForPath`'s `IOException`).
    Path(String),
    /// Anything else the platform or JNI reported.
    Platform(String),
}

impl fmt::Display for DiskSpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiskSpaceError::Path(detail) => write!(f, "path: {detail}"),
            DiskSpaceError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for DiskSpaceError {}

impl DiskSpace {
    /// From `statvfs`'s block counts and fragment size, the products
    /// saturating at `u64::MAX`.
    pub(crate) fn from_blocks(available: u64, blocks: u64, fragment: u64) -> Self {
        Self {
            free: available.saturating_mul(fragment),
            total: blocks.saturating_mul(fragment),
            allocatable: None,
            cache_quota: None,
        }
    }
}

/// `statvfs(path)` alone: `free` and `total`, with `allocatable` and
/// `cache_quota` `None`. Any thread, any Unix host; microseconds.
pub fn statvfs_space(path: &Path) -> Result<DiskSpace, DiskSpaceError> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| DiskSpaceError::Path(format!("{}: contains NUL", path.display())))?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c_path` is a NUL-terminated string that outlives the call,
    // and `stat` points to writable memory of the right size; statvfs
    // fills it on success and leaves it unread on failure.
    #[allow(unsafe_code)]
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        return Err(DiskSpaceError::Path(format!("{}: {err}", path.display())));
    }
    // SAFETY: statvfs returned 0, so it initialised `stat`.
    #[allow(unsafe_code)]
    let stat = unsafe { stat.assume_init() };
    // The field types differ between platforms (u32 on some, u64 on others).
    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
    let space = DiskSpace::from_blocks(
        u64::from(stat.f_bavail),
        u64::from(stat.f_blocks),
        stat.f_frsize as u64,
    );
    Ok(space)
}

/// The shortest interval `android::disk_space::watch` uses: a zero or tiny
/// interval cannot spin the main looper.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) const WATCH_INTERVAL_FLOOR: Duration = Duration::from_secs(1);

/// The watch interval in whole milliseconds for the Kotlin timer: at least
/// [`WATCH_INTERVAL_FLOOR`], saturating at `i64::MAX`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn watch_interval_ms(interval: Duration) -> i64 {
    i64::try_from(interval.max(WATCH_INTERVAL_FLOOR).as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_has_space() {
        let space = statvfs_space(&std::env::temp_dir()).expect("statvfs of the temp dir");
        assert!(space.total > 0);
        assert!(space.free <= space.total);
        assert_eq!(space.allocatable, None);
        assert_eq!(space.cache_quota, None);
    }

    #[test]
    fn missing_path_is_a_path_error() {
        let missing = std::env::temp_dir().join("libmobile-disk-space-no-such-dir/x/y");
        assert!(matches!(
            statvfs_space(&missing),
            Err(DiskSpaceError::Path(_))
        ));
        assert!(matches!(
            statvfs_space(Path::new("/a\0b")),
            Err(DiskSpaceError::Path(_))
        ));
    }

    #[test]
    fn products_saturate() {
        let space = DiskSpace::from_blocks(3, 5, 4096);
        assert_eq!((space.free, space.total), (3 * 4096, 5 * 4096));
        let space = DiskSpace::from_blocks(u64::MAX / 2, u64::MAX, 4);
        assert_eq!((space.free, space.total), (u64::MAX, u64::MAX));
    }

    #[test]
    fn interval_has_a_floor_of_one_second() {
        assert_eq!(watch_interval_ms(Duration::ZERO), 1000);
        assert_eq!(watch_interval_ms(Duration::from_millis(999)), 1000);
        assert_eq!(watch_interval_ms(Duration::from_secs(1)), 1000);
        assert_eq!(watch_interval_ms(Duration::from_secs(60)), 60_000);
        assert_eq!(watch_interval_ms(Duration::from_secs(600)), 600_000);
        assert_eq!(watch_interval_ms(Duration::MAX), i64::MAX);
    }

    #[test]
    fn errors_display() {
        assert_eq!(DiskSpaceError::Path("x".into()).to_string(), "path: x");
        assert_eq!(
            DiskSpaceError::Platform("y".into()).to_string(),
            "platform error: y"
        );
    }
}
