//! **Telling MediaStore about changed files** (feature `media-scan`): the
//! platform-free types and the path check. The call is
//! `android::media_scan::scan_paths`.
//!
//! A media scan needs no permission: any app that writes to shared storage
//! (a camera app saving to `DCIM`, a file manager) uses it so other apps see
//! the change without waiting for the next full scan.

use std::fmt;
use std::path::{Path, PathBuf};

/// The most paths one `scan_paths` call takes.
pub const MAX_SCAN_PATHS: usize = 1024;

/// Why a media scan was not requested.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaScanError {
    /// A path was not absolute or not valid UTF-8 (`scanFile` takes `String`s).
    BadPath(PathBuf),
    /// More than [`MAX_SCAN_PATHS`] (1 024) paths in one call.
    TooMany(usize),
    /// Anything Android or JNI reported.
    Platform(String),
}

impl fmt::Display for MediaScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaScanError::BadPath(path) => {
                write!(f, "not an absolute UTF-8 path: {}", path.display())
            }
            MediaScanError::TooMany(n) => {
                write!(f, "{n} paths in one scan; at most {MAX_SCAN_PATHS}")
            }
            MediaScanError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for MediaScanError {}

/// The paths as the strings `scanFile` takes, after the checks: at most
/// [`MAX_SCAN_PATHS`], each absolute and valid UTF-8.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn checked_paths<'a>(paths: &[&'a Path]) -> Result<Vec<&'a str>, MediaScanError> {
    if paths.len() > MAX_SCAN_PATHS {
        return Err(MediaScanError::TooMany(paths.len()));
    }
    paths
        .iter()
        .map(|path| match path.to_str() {
            Some(text) if path.is_absolute() => Ok(text),
            _ => Err(MediaScanError::BadPath(path.to_path_buf())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_utf8_paths_pass() {
        let a = Path::new("/storage/emulated/0/DCIM/a.jpg");
        let b = Path::new("/storage/emulated/0/Pictures/ü.png");
        assert_eq!(
            checked_paths(&[a, b]),
            Ok(vec![
                "/storage/emulated/0/DCIM/a.jpg",
                "/storage/emulated/0/Pictures/ü.png"
            ])
        );
        assert_eq!(checked_paths(&[]), Ok(vec![]));
    }

    #[test]
    fn relative_paths_fail() {
        let good = Path::new("/a");
        let bad = Path::new("DCIM/a.jpg");
        assert_eq!(
            checked_paths(&[good, bad]),
            Err(MediaScanError::BadPath(bad.to_path_buf()))
        );
        assert!(matches!(
            checked_paths(&[Path::new("")]),
            Err(MediaScanError::BadPath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_fail() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new(OsStr::from_bytes(b"/storage/\xff.jpg"));
        assert_eq!(
            checked_paths(&[bad]),
            Err(MediaScanError::BadPath(bad.to_path_buf()))
        );
    }

    #[test]
    fn at_most_max_paths() {
        let path = Path::new("/a");
        let max = vec![path; MAX_SCAN_PATHS];
        assert_eq!(checked_paths(&max).map(|p| p.len()), Ok(MAX_SCAN_PATHS));
        let over = vec![path; MAX_SCAN_PATHS + 1];
        assert_eq!(
            checked_paths(&over),
            Err(MediaScanError::TooMany(MAX_SCAN_PATHS + 1))
        );
    }
}
