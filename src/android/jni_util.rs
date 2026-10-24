//! The two JNI shapes every Rust -> Kotlin call shares, in one place.

use android_activity::AndroidApp;
use jni::objects::{Global, JClass, JObject, JObjectArray, JString};
use jni::sys::jobject;
use jni::{Env, jni_sig, jni_str};

/// Attach this thread to the VM and run `f` with the activity and the
/// libmobile class `class_name` (dotted `ClassLoader.loadClass` form).
///
/// **The class is resolved through `Context.getClassLoader()`, not
/// `FindClass`.** `android_main` runs on a thread the native glue created and
/// attached, so JNI resolves names there against the *system* class loader,
/// which cannot see the APK's dex: a plain `FindClass` fails with
/// `ClassNotFoundException`. It must be the `Context` method (the app's
/// loader from the LoadedApk) and not `getClass().getClassLoader()`, which for
/// a framework-loaded activity class is the boot loader and fails the same way.
pub(crate) fn with_app_class<R>(
    app: &AndroidApp,
    class_name: &str,
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    with_activity(app, |env, activity| {
        let class = load_class(env, activity, class_name)?;
        f(env, activity, &class)
    })
}

/// [`with_app_class`] without an `AndroidApp`: the Application from
/// `ndk-context`, which android-activity registers once per process before
/// `android_main`. Valid from any thread and across `android_main` runs.
pub(crate) fn with_application_class<R>(
    class_name: &str,
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    with_application(|env, application| {
        let class = load_class(env, application, class_name)?;
        f(env, application, &class)
    })
}

/// `context.getClassLoader().loadClass(class_name)`; see [`with_app_class`]
/// for why it is the `Context` method.
fn load_class<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    class_name: &str,
) -> jni::errors::Result<JClass<'local>> {
    let loader = env
        .call_method(
            context,
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
    env.cast_local::<JClass>(loaded)
}

/// Attach this thread to the VM and run `f` with the Application.
#[allow(unsafe_code)] // Scoped: one documented `as_cast_raw` on a pointer android-activity owns.
pub(crate) fn with_application<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    let vm = jni::JavaVM::singleton()?;
    let raw_application: jobject = ndk_context::android_context().context().cast();
    vm.attach_current_thread(|env: &mut Env| {
        // SAFETY: `raw_application` is a JNI global reference android-activity
        // created for `ndk-context` and never deletes (it lives for the
        // process); `Cast` borrows it without taking ownership.
        let application = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_application)? };
        f(env, &application)
    })
}

/// Attach this thread to the VM and run `f` with the activity.
///
/// `activity_as_ptr` hands back an *unowned* JNI global reference to the
/// activity; `as_cast_raw` borrows it without taking ownership (so nothing
/// ever deletes it) and type-checks it at runtime.
#[allow(unsafe_code)] // Scoped: one documented `as_cast_raw` on a pointer android-activity owns.
pub(crate) fn with_activity<R>(
    app: &AndroidApp,
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    let vm = jni::JavaVM::singleton()?;
    let raw_activity: jobject = app.activity_as_ptr().cast();
    vm.attach_current_thread(|env: &mut Env| {
        // SAFETY: `raw_activity` is a live JNI global reference owned by
        // `AndroidApp`, which outlives this call; `Cast` borrows it without
        // taking ownership, so it is never deleted here.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        f(env, &activity)
    })
}

/// A nullable Java string argument as an `Option`.
pub(crate) fn opt_string(value: &JString<'_>) -> Option<String> {
    (!value.is_null()).then(|| value.to_string())
}

/// A `String[]` of `items`.
pub(crate) fn string_array<'local>(
    env: &mut Env<'local>,
    items: &[&str],
) -> jni::errors::Result<JObjectArray<'local, JString<'local>>> {
    let empty = JString::from_str(env, "")?;
    let array = JObjectArray::<JString>::new(env, items.len(), &empty)?;
    for (i, item) in items.iter().enumerate() {
        let item = JString::from_str(env, item)?;
        array.set_element(env, i, &item)?;
    }
    Ok(array)
}
