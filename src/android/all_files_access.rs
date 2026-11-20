//! All files access and the shared storage root
//! (`all_files_access/AllFilesAccessPlugin.kt`, feature `all-files-access`).
//!
//! All files access (`MANAGE_EXTERNAL_STORAGE`) is not a runtime
//! permission: `request_permissions` cannot ask for it and
//! `permission_granted` always answers false for it. The app declares it
//! with [`Project::all_files_access`](crate::gradle::Project::all_files_access)
//! and the user turns it on in the page [`open_settings`] opens.
//!
//! Android does not tell the app when the user changes the switch: read
//! [`state`] again on `MobileEvent::Resume`. The page is an activity in the
//! Settings task, so going there pauses the app and Back resumes it.
//!
//! Every call is synchronous and may be made from any thread; it reaches
//! the plugin through the Application that android-activity registers with
//! `ndk-context`.
//!
//! **Google Play** allows `MANAGE_EXTERNAL_STORAGE` only for apps whose core
//! purpose needs it (file managers, backup and similar). Sideloaded apps
//! are not bound by that.

pub use crate::all_files_access::{AccessPage, AllFilesAccess, AllFilesAccessError};

use super::jni_util::{opt_string, with_application_class};
use jni::objects::{JClass, JObject, JString};
use jni::{Env, jni_sig, jni_str};
use std::path::PathBuf;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.all_files_access.AllFilesAccessPlugin";

/// Whether the app holds All files access now. Any thread; a short binder
/// call. [`AllFilesAccessError::NotDeclared`] before `Granted`/`Missing`,
/// so a build-script slip is not mistaken for the user's "no";
/// [`AllFilesAccessError::Unsupported`] below Android 11.
pub fn state() -> Result<AllFilesAccess, AllFilesAccessError> {
    let answer = call_context_string(jni_str!("state"))?;
    AllFilesAccess::from_answer(&answer)
}

/// Open this app's All files access page, or the list of apps if that
/// does not resolve. The app must be visible
/// ([`AllFilesAccessError::NotVisible`] otherwise). Android reports nothing
/// back; read [`state`] again when the app resumes. Opening it while the
/// access is already granted is allowed (the user may turn it off there).
pub fn open_settings() -> Result<AccessPage, AllFilesAccessError> {
    let answer = call_context_string(jni_str!("openSettings"))?;
    let page = AccessPage::from_answer(&answer);
    log::info!("libmobile: all files access page {page:?}");
    page
}

/// The primary shared volume's root (`Environment.getExternalStorageDirectory()`),
/// for example `/storage/emulated/0`. Any thread. Does not check access:
/// reading under it without the grant fails with the OS's own error.
pub fn external_storage_dir() -> Result<PathBuf, AllFilesAccessError> {
    let dir = call(|env, _context, class| {
        let value = env
            .call_static_method(
                class,
                jni_str!("externalStorageDir"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let value = env.cast_local::<JString>(value)?;
        Ok(opt_string(&value))
    })?;
    dir.map(PathBuf::from)
        .ok_or_else(|| AllFilesAccessError::Platform("no external storage".into()))
}

/// `AllFilesAccessPlugin.<method>(Context) -> String`.
fn call_context_string(
    method: &'static jni::strings::JNIStr,
) -> Result<String, AllFilesAccessError> {
    let answer = call(|env, context, class| {
        let value = env
            .call_static_method(
                class,
                method,
                jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
                &[context.into()],
            )?
            .l()?;
        let value = env.cast_local::<JString>(value)?;
        Ok(opt_string(&value))
    })?;
    answer.ok_or_else(|| AllFilesAccessError::Platform("no answer".into()))
}

/// Run `f` against the plugin class with the Application, catching a thrown
/// exception as its message.
fn call<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> Result<R, AllFilesAccessError> {
    with_application_class(PLUGIN_CLASS, |env, context, class| {
        match f(env, context, class) {
            Err(jni::errors::Error::JavaException) => {
                env.exception_catch()?;
                Err(jni::errors::Error::JavaException)
            }
            other => other,
        }
    })
    .map_err(|err| match err {
        jni::errors::Error::CaughtJavaException { msg, .. } => AllFilesAccessError::Platform(msg),
        other => AllFilesAccessError::Platform(other.to_string()),
    })
}
