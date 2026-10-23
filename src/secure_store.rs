//! **Secure store types**, platform-free so wrappers can map onto them on
//! any host. The store itself is `android::secure_store`: named byte blobs,
//! each encrypted with an AES-256-GCM key kept in Android Keystore
//! (`store/SecureStorePlugin.kt`).

use std::fmt;

/// Longest blob name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// Largest blob, in bytes of plaintext.
pub const MAX_VALUE_LEN: usize = 64 * 1024;

/// Why a secure store call failed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SecureStoreError {
    /// Not 1 to 64 of `[A-Za-z0-9._-]`, or starts with `.`.
    InvalidName,
    /// Over [`MAX_VALUE_LEN`].
    TooLarge,
    /// Keystore refused to create or use the key.
    Keystore(String),
    /// The blob was stored with [`PutOptions::unlocked_only`] and the device
    /// is locked. Try again once it is unlocked.
    DeviceLocked,
    /// Reading or writing the blob's file failed.
    Io(String),
    /// The file is malformed, or decryption failed (a renamed or altered
    /// file, or a Keystore key that was lost or invalidated). Not treated as
    /// absent: the app decides whether to delete it and start over.
    Corrupt(String),
    /// The platform failed (JNI, or an unexpected exception).
    Platform(String),
}

impl fmt::Display for SecureStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "secure store: names are 1 to {MAX_NAME_LEN} of [A-Za-z0-9._-], not starting with '.'"
            ),
            Self::TooLarge => write!(f, "secure store: blobs are at most {MAX_VALUE_LEN} bytes"),
            Self::Keystore(detail) => write!(f, "secure store: keystore: {detail}"),
            Self::DeviceLocked => write!(f, "secure store: the device is locked"),
            Self::Io(detail) => write!(f, "secure store: io: {detail}"),
            Self::Corrupt(detail) => write!(f, "secure store: corrupt: {detail}"),
            Self::Platform(detail) => write!(f, "secure store: platform: {detail}"),
        }
    }
}

impl std::error::Error for SecureStoreError {}

impl SecureStoreError {
    /// The plugin's `"<kind>: <detail>"` exception message. Anything else is
    /// reported as [`SecureStoreError::Platform`].
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by `android::secure_store`.
    pub(crate) fn from_message(message: &str) -> Self {
        let (kind, detail) = message.split_once(": ").unwrap_or((message, ""));
        let detail = detail.to_string();
        match kind {
            "keystore" => Self::Keystore(detail),
            "io" => Self::Io(detail),
            "corrupt" => Self::Corrupt(detail),
            "device-locked" => Self::DeviceLocked,
            _ => Self::Platform(message.to_string()),
        }
    }
}

/// How a blob is stored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct PutOptions {
    /// Wrap the blob with a key the platform refuses while the device is
    /// locked. Off by default, so the blob can be read in the background.
    pub unlocked_only: bool,
}

impl PutOptions {
    pub fn unlocked_only(mut self, unlocked_only: bool) -> Self {
        self.unlocked_only = unlocked_only;
        self
    }

    /// The plugin's flags: bit 0 is `unlocked_only`.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by `android::secure_store`.
    pub(crate) fn flags(self) -> i32 {
        i32::from(self.unlocked_only)
    }
}

/// Where the store's key lives (`KeyInfo.getSecurityLevel`, or
/// `isInsideSecureHardware` before API 31).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SecurityLevel {
    /// Keystore in software: no secure hardware on this device.
    Software,
    /// The trusted execution environment.
    TrustedEnvironment,
    /// A separate secure element (feature `secure-store-strongbox`).
    StrongBox,
}

impl SecurityLevel {
    /// The plugin's code: 0 software, 1 TEE, 2 StrongBox.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by `android::secure_store`.
    pub(crate) fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Software),
            1 => Some(Self::TrustedEnvironment),
            2 => Some(Self::StrongBox),
            _ => None,
        }
    }
}

/// Check a blob name: 1 to [`MAX_NAME_LEN`] of `[A-Za-z0-9._-]`, not
/// starting with `.`. Names are file names in the store's directory.
pub fn check_name(name: &str) -> Result<(), SecureStoreError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(SecureStoreError::InvalidName)
    }
}

/// Check a blob's size against [`MAX_VALUE_LEN`].
pub fn check_value(value: &[u8]) -> Result<(), SecureStoreError> {
    if value.len() <= MAX_VALUE_LEN {
        Ok(())
    } else {
        Err(SecureStoreError::TooLarge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "noise.static", "server", "A-b_c.9", &"x".repeat(64)] {
            assert_eq!(check_name(ok), Ok(()), "{ok}");
        }
        for bad in [
            "",
            ".hidden",
            "..",
            "a/b",
            "a b",
            "ü",
            "a\0",
            &"x".repeat(65),
        ] {
            assert_eq!(
                check_name(bad),
                Err(SecureStoreError::InvalidName),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(check_value(&[]), Ok(()));
        assert_eq!(check_value(&vec![0; MAX_VALUE_LEN]), Ok(()));
        assert_eq!(
            check_value(&vec![0; MAX_VALUE_LEN + 1]),
            Err(SecureStoreError::TooLarge)
        );
    }

    #[test]
    fn messages() {
        assert_eq!(
            SecureStoreError::from_message("keystore: no provider"),
            SecureStoreError::Keystore("no provider".into())
        );
        assert_eq!(
            SecureStoreError::from_message("io: ENOSPC: full"),
            SecureStoreError::Io("ENOSPC: full".into())
        );
        assert_eq!(
            SecureStoreError::from_message("corrupt: tag mismatch"),
            SecureStoreError::Corrupt("tag mismatch".into())
        );
        assert_eq!(
            SecureStoreError::from_message("java.lang.NullPointerException"),
            SecureStoreError::Platform("java.lang.NullPointerException".into())
        );
    }

    #[test]
    fn options() {
        assert_eq!(PutOptions::default().flags(), 0);
        assert_eq!(PutOptions::default().unlocked_only(true).flags(), 1);
        assert_eq!(
            SecureStoreError::from_message("device-locked: the device is locked"),
            SecureStoreError::DeviceLocked
        );
    }

    #[test]
    fn levels() {
        assert_eq!(SecurityLevel::from_code(0), Some(SecurityLevel::Software));
        assert_eq!(
            SecurityLevel::from_code(1),
            Some(SecurityLevel::TrustedEnvironment)
        );
        assert_eq!(SecurityLevel::from_code(2), Some(SecurityLevel::StrongBox));
        assert_eq!(SecurityLevel::from_code(3), None);
    }
}
