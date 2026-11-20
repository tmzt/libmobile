//! **All files access** (feature `all-files-access`): the platform-free
//! types. The calls are in `android::all_files_access`.
//!
//! All files access (`MANAGE_EXTERNAL_STORAGE`, Android 11 and later) lets
//! an app read and write the phone's shared storage through POSIX paths. It
//! is not a runtime permission: it is declared in the manifest
//! (`gradle::Project::all_files_access`) and granted only by the user, on a
//! settings page.

use std::fmt;

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

impl AllFilesAccess {
    /// From the plugin's `state` answer: `Ok` for `"granted"` and
    /// `"missing"`, the error for anything else.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_answer(answer: &str) -> Result<Self, AllFilesAccessError> {
        match answer {
            "granted" => Ok(AllFilesAccess::Granted),
            "missing" => Ok(AllFilesAccess::Missing),
            other => Err(AllFilesAccessError::from_message(other)),
        }
    }
}

impl AccessPage {
    /// From the plugin's `openSettings` answer: `Ok` for `"app"` and
    /// `"list"`, the error for anything else.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_answer(answer: &str) -> Result<Self, AllFilesAccessError> {
        match answer {
            "app" => Ok(AccessPage::App),
            "list" => Ok(AccessPage::List),
            other => Err(AllFilesAccessError::from_message(other)),
        }
    }
}

impl AllFilesAccessError {
    /// From the plugin's `"<kind>: <detail>"` message (or a bare kind).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let kind = message.split_once(':').map_or(message, |(kind, _)| kind);
        match kind.trim() {
            "not-declared" => AllFilesAccessError::NotDeclared,
            "unsupported" => AllFilesAccessError::Unsupported,
            "not-visible" => AllFilesAccessError::NotVisible,
            "no-settings-page" => AllFilesAccessError::NoSettingsPage,
            _ => AllFilesAccessError::Platform(message.to_string()),
        }
    }
}

impl fmt::Display for AllFilesAccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AllFilesAccessError::NotDeclared => {
                f.write_str("MANAGE_EXTERNAL_STORAGE is not declared")
            }
            AllFilesAccessError::Unsupported => {
                f.write_str("All files access needs Android 11 (API 30) or later")
            }
            AllFilesAccessError::NotVisible => {
                f.write_str("a settings page may only be opened while the app is visible")
            }
            AllFilesAccessError::NoSettingsPage => {
                f.write_str("no All files access settings page resolves on this device")
            }
            AllFilesAccessError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for AllFilesAccessError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_from_messages() {
        use AllFilesAccessError as E;
        assert_eq!(
            E::from_message("not-declared: android.permission.X"),
            E::NotDeclared
        );
        assert_eq!(E::from_message("unsupported"), E::Unsupported);
        assert_eq!(
            E::from_message("not-visible: importance 400"),
            E::NotVisible
        );
        assert_eq!(E::from_message("no-settings-page"), E::NoSettingsPage);
        assert_eq!(
            E::from_message("platform: boom"),
            E::Platform("platform: boom".into())
        );
        assert_eq!(E::from_message("odd"), E::Platform("odd".into()));
    }

    #[test]
    fn state_from_answers() {
        assert_eq!(
            AllFilesAccess::from_answer("granted"),
            Ok(AllFilesAccess::Granted)
        );
        assert_eq!(
            AllFilesAccess::from_answer("missing"),
            Ok(AllFilesAccess::Missing)
        );
        assert_eq!(
            AllFilesAccess::from_answer("not-declared: x"),
            Err(AllFilesAccessError::NotDeclared)
        );
        assert_eq!(
            AllFilesAccess::from_answer("unsupported"),
            Err(AllFilesAccessError::Unsupported)
        );
    }

    #[test]
    fn page_from_answers() {
        assert_eq!(AccessPage::from_answer("app"), Ok(AccessPage::App));
        assert_eq!(AccessPage::from_answer("list"), Ok(AccessPage::List));
        assert_eq!(
            AccessPage::from_answer("no-settings-page"),
            Err(AllFilesAccessError::NoSettingsPage)
        );
        assert_eq!(
            AccessPage::from_answer("not-visible: importance 125"),
            Err(AllFilesAccessError::NotVisible)
        );
    }

    #[test]
    fn errors_display() {
        assert!(
            AllFilesAccessError::NotDeclared
                .to_string()
                .contains("MANAGE_EXTERNAL_STORAGE")
        );
        assert_eq!(
            AllFilesAccessError::Platform("x".into()).to_string(),
            "platform error: x"
        );
    }
}
