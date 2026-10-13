//! JNI leg of the safe-area bridge (`bridge/WindowInsetsPlugin.kt`).
//!
//! Kotlin -> Rust only. The plugin installs an `OnApplyWindowInsetsListener`
//! on the decor view and calls [`Java_dev_highbay_libmobile_bridge_WindowInsetsPlugin_nativeWindowInsets`]
//! every time it fires — on the UI thread, so the callback only fills
//! [`PENDING`] and the main loop drains it each tick.
//!
//! **Insets are a value that changes, not a boot-time constant**: rotation, a
//! gesture/3-button navigation switch and the IME all move them while the
//! process runs. The slot keeps only the newest report.
//!
//! **Deliberately never reset when the loop exits**, unlike the command
//! mailboxes: the panel's cutout and bars did not change because the activity
//! was recreated, and the first report of a new activity can land before
//! `android_main` gets far enough to drain it — the next one might be a
//! rotation away.

use crate::insets::Insets;
use crate::mailbox::Latest;
use jni::EnvUnowned;
use jni::objects::JObject;
use jni::sys::jint;

static PENDING: Latest<Insets> = Latest::new();

pub(crate) fn take_pending() -> Option<Insets> {
    PENDING.take()
}

/// Kotlin -> Rust: `WindowInsetsPlugin.nativeWindowInsets(int x5)`, physical
/// pixels. An *instance* method of the Kotlin `object` (no `@JvmStatic`), so
/// the symbol shape is unambiguous: `(JNIEnv*, jobject, jint x5)`.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_bridge_WindowInsetsPlugin_nativeWindowInsets<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
    ime_bottom: jint,
) {
    // `with_env` wraps the body in catch_unwind, so a panic can never unwind
    // across the FFI boundary into the JVM.
    let outcome = unowned_env.with_env(|_env| -> Result<(), jni::errors::Error> {
        let insets = Insets::from_px(left, top, right, bottom, ime_bottom);
        if PENDING.set(insets) != Some(insets) {
            log::debug!("libmobile: insets {insets:?}");
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
