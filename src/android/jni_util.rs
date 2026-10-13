//! The two JNI shapes every Rust -> Kotlin call shares, in one place.

use android_activity::AndroidApp;
use jni::objects::{Global, JClass, JObject, JString};
use jni::sys::jobject;
use jni::{Env, jni_sig, jni_str};

/// Attach this thread to the VM and run `f` with the activity and the
/// libmobile class `class_name` (dotted `ClassLoader.loadClass` form).
///
/// `activity_as_ptr` hands back an *unowned* JNI global reference to the
/// activity; `as_cast_raw` borrows it without taking ownership (so nothing
/// ever deletes it) and type-checks it at runtime.
///
/// **The class is resolved through `Context.getClassLoader()`, not
/// `FindClass`.** `android_main` runs on a thread the native glue created and
/// attached, so JNI resolves names there against the *system* class loader,
/// which cannot see the APK's dex: a plain `FindClass` fails with
/// `ClassNotFoundException`. It must be the `Context` method (the app's
/// loader from the LoadedApk) and not `getClass().getClassLoader()`, which for
/// a framework-loaded activity class is the boot loader and fails the same way.
#[allow(unsafe_code)] // Scoped: one documented `as_cast_raw` on a pointer android-activity owns.
pub(crate) fn with_app_class<R>(
    app: &AndroidApp,
    class_name: &str,
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    let vm = jni::JavaVM::singleton()?;
    let raw_activity: jobject = app.activity_as_ptr().cast();
    vm.attach_current_thread(|env: &mut Env| {
        // SAFETY: `raw_activity` is a live JNI global reference owned by
        // `AndroidApp`, which outlives this call; `Cast` borrows it without
        // taking ownership, so it is never deleted here.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        let loader = env
            .call_method(
                &*activity,
                jni_str!("getClassLoader"),
                jni_sig!("()Ljava/lang/ClassLoader;"),
                &[],
            )?
            .l()?;
        let name = JString::from_str(env, class_name)?;
        let loaded = env
            .call_method(
                &loader,
                jni_str!("loadClass"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                &[(&name).into()],
            )?
            .l()?;
        let class = env.cast_local::<JClass>(loaded)?;
        f(env, &activity, &class)
    })
}

/// A nullable Java string argument as an `Option`.
pub(crate) fn opt_string(value: &JString<'_>) -> Option<String> {
    (!value.is_null()).then(|| value.to_string())
}
