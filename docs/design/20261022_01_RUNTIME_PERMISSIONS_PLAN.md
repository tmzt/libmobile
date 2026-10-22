---
title: Runtime permission requests
date: 2026-10-22
status: In implementation
covers:
  - An `onRequestPermissionsResult` hook on `MobileActivityPlugin`,
    dispatched by `MobileNativeActivity`
  - A core `bridge.PermissionPlugin` that asks for dangerous permissions at
    runtime and reports the answers to Rust
  - `libmobile::android::request_permissions`, `permission_granted`, and
    `MobileEvent::Permissions`
sources:
  - Owner (2026-10-22), in the libmorph session, after libmorph's
    audiocapture example ran on the Galaxy S23 with `RECORD_AUDIO` granted
    through `adb shell pm grant`: "it didnt request permission, that m8ght
    require java plugin". Agreed with the outline proposed there ("that's
    fine").
  - libmobile today: `gradle::Project::permission(name)` and plugin
    `Contributes::permissions` only add `uses-permission` to the manifest.
    `MobileActivityPlugin` has `onActivityResult` but no permission result
    hook, and `MobileNativeActivity` does not override
    `onRequestPermissionsResult`.
  - The camera bridge as the pattern (`src/android/camera.rs`,
    `camera/CameraPlugin.kt`): Rust calls a static Kotlin method through
    `jni_util::with_app_class`, Kotlin works on the UI thread and answers
    through an `external fun` that pushes into a `Mailbox`, and the main
    loop turns mailbox entries into `MobileEvent`s.
  - Android: `Activity.requestPermissions(String[], int)` (API 23),
    `Activity.onRequestPermissionsResult(int, String[], int[])`,
    `Context.checkSelfPermission(String)`. A permission missing from the
    manifest, or one the user has refused twice, is answered "denied"
    without a dialog. An empty result means the request was interrupted.
related:
  - libmorph docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md (depends on
    this plan for asking for `RECORD_AUDIO`)
  - docs/design/20261016_01_CPAL_FEATURE_PLAN.md (cpal's AAudio input,
    which needs `RECORD_AUDIO`)
---

# Runtime permission requests

## 1. Goal

An app can ask the user for a dangerous permission (`RECORD_AUDIO`,
`CAMERA`, ...) and learn the answer, without its own Kotlin. The app still
declares the permission in the manifest (`Project::permission`, or a
plugin's `Contributes::permissions`); this plan only adds the runtime
request.

Nothing changes for apps that do not call it.

## 2. Kotlin

`MobileActivityPlugin` gains, with a default body:

```kotlin
/**
 * Return `true` if this plugin made the request and consumed the result;
 * dispatch stops at the first plugin that does.
 */
fun onRequestPermissionsResult(
    activity: Activity,
    requestCode: Int,
    permissions: Array<out String>,
    grantResults: IntArray,
): Boolean = false
```

`MobileNativeActivity` overrides `onRequestPermissionsResult`, calls
`super`, and dispatches with `firstOrNull`, as it does `onActivityResult`.

`bridge/PermissionPlugin.kt`, a core plugin (no feature) listed after the
other bridges in `PLUGINS`:

```kotlin
object PermissionPlugin : MobileActivityPlugin {
    private const val REQUEST_CODE = 0x504d // "PM"

    /** From Rust: static, `(Landroid/app/Activity;[Ljava/lang/String;)V`. */
    @JvmStatic
    fun request(activity: Activity, permissions: Array<String>)
    // runOnUiThread { activity.requestPermissions(permissions, REQUEST_CODE) }

    /** From Rust: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`. */
    @JvmStatic
    fun granted(activity: Activity, permission: String): Boolean
    // activity.checkSelfPermission(permission) == PERMISSION_GRANTED

    override fun onRequestPermissionsResult(...): Boolean
    // requestCode != REQUEST_CODE -> false; otherwise
    // nativePermissionsResult(permissions, grantedFlags) and true

    @JvmStatic
    private external fun nativePermissionsResult(
        permissions: Array<String>,
        granted: BooleanArray,
    )
}
```

An empty `permissions` array in the result (an interrupted request) is
answered with the requested names, all denied, so every request gets one
answer. `request` remembers the names of the request in flight for that.

## 3. Rust

In `src/android/permission.rs`, re-exported from `libmobile::android`:

```rust
/// Ask the user for `permissions` (full names, e.g.
/// "android.permission.RECORD_AUDIO"). The answer arrives as
/// `MobileEvent::Permissions`. Each one must be declared in the manifest,
/// or it is denied without a dialog.
pub fn request_permissions(app: &AndroidApp, permissions: &[&str]);

/// Whether `permission` is granted now (`checkSelfPermission`).
/// `false` if the JNI call fails.
pub fn permission_granted(app: &AndroidApp, permission: &str) -> bool;
```

`request_permissions` calls `PermissionPlugin.request` through
`with_app_class`. If the call fails, it logs the error and queues every
name as denied. The JNI export
`Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult`
pushes a `PermissionsOutcome` into a `Mailbox`, and the main loop drains it
next to the camera's results (and discards it in the same place the
camera's are discarded).

In `src/event.rs`, platform-free like `Insets`:

```rust
/// The answer to one `request_permissions` call, in request order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionsOutcome {
    pub permissions: Vec<(String, bool)>,
}

pub enum MobileEvent {
    // ...
    /// The user answered a `request_permissions` call.
    Permissions(PermissionsOutcome),
}
```

`MobileEvent` is `#[non_exhaustive]`, so the variant is additive.

## 4. Steps

1. **Kotlin hook and plugin.** The `MobileActivityPlugin` hook, the
   `MobileNativeActivity` override, `bridge/PermissionPlugin.kt`, and its
   entry in `PLUGINS` (codegen tests updated for the new core plugin).
   **Done when:** `cargo test --workspace` passes and a generated project
   builds with `gradle assembleDebug`.
2. **Rust API and event.** `request_permissions`, `permission_granted`,
   the JNI export, the mailbox, `PermissionsOutcome` and
   `MobileEvent::Permissions`. Unit test for answering an interrupted
   request.
   **Done when:** `cargo clippy --target aarch64-linux-android
   --workspace --all-features` is clean and `cargo test --workspace`
   passes.
3. **On a device.** The `wgpu` example requests `RECORD_AUDIO` (added to
   its manifest) on a tap and logs the event.
   **Done when:** on the owner's Galaxy S23, after
   `adb shell pm revoke`, a tap shows the system dialog, and allowing and
   denying each log the matching `MobileEvent::Permissions`.

## 5. Release

Strictly additive (a defaulted Kotlin hook, new functions, a new type, a
variant of a `#[non_exhaustive]` enum), so it is released as libmobile
0.1.2.

## Amendment — 2026-10-22 07:10 EDT: implementation has started; the document is now immutable

The owner asked for this to be implemented (2026-10-22), on the
`runtime-permissions` branch in its own worktree. Steps 1 and 2 are
built in this commit, and step 3's example change is in it too. On the
owner's Galaxy S23, the `wgpu` example showed the system dialog
("Allow libmobile wgpu to record audio?") and logged
`permissions [("android.permission.RECORD_AUDIO", true)]` when allowed.
The denied answer is not yet checked.

## Amendment — 2026-10-22 07:53 EDT: request ids, one request at a time, and the rationale check

On the owner's explicit instruction (2026-10-22, "write it up then
implement"), after asking whether the camera and other permissions use
this path. Replaces the signatures in sections 2 and 3, and adds a step 4.

- **Request ids.** `request_permissions` returns a
  `PermissionRequest`, an opaque `Copy` id (`u32` inside, counting
  from 1 per process). `PermissionsOutcome` gains
  `pub request: PermissionRequest` and becomes `#[non_exhaustive]`. A
  failed JNI call still answers, with its id and every name denied.
- **One request at a time.** Android shows one permission dialog at a
  time, and a request made while another is waiting is answered as
  interrupted. `PermissionPlugin` therefore keeps a queue of
  `(id, names)`: `request(activity, id, names)` appends to it and starts
  the head if nothing is in flight. Each result is delivered with the
  head's id and names, then the next request starts. Every request still
  gets exactly one answer, in the order made. The Kotlin call becomes
  `request(Activity, int, String[])`, and the native result
  `nativePermissionsResult(int, String[], String[], boolean[])`.
- **Rationale.** `should_show_rationale(app, permission) -> bool` wraps
  `Activity.shouldShowRequestPermissionRationale`: `true` after the
  user has denied once, `false` before the first request and once the
  user has chosen "Don't allow" a second time. After a denial, `false`
  means a further request will be denied without a dialog, so the app
  should point to Settings. `false` if the JNI call fails.
- **Not in this plan.** Permissions granted from Settings rather than a
  dialog (drawing over other apps, all-files access, exact alarms), a
  Settings intent, and background location, which Android asks for
  separately.

4. **Ids, queue and rationale.** As above. The `wgpu` example logs each
   request's id and its answer's id, and logs the rationale after a
   denial.
   **Done when:** `cargo test --workspace` passes, Android clippy is
   clean, and on the S23 two requests made back to back are answered in
   order, with their own ids and names.

## Amendment — 2026-10-22 08:06 EDT: the device checks move to the audiocapture example

On the owner's instruction (2026-10-22). The owner noted that the `wgpu`
example asks for permissions it does not use ("it doesn't look like wgpu
uses either of those permissions"). Replaces the `wgpu` example in step 3
and in the amendment "request ids, one request at a time, and the
rationale check" (its step 4).

- The `wgpu` example goes back to how it is on `main`: it declares and
  asks for no permissions.
- The device checks are made with the `audiocapture` example of
  `docs/design/20261022_02_AUDIO_CAPTURE_PLAN.md`, which records from the
  microphone. It asks for `RECORD_AUDIO`, logs the request id and answer,
  logs `should_show_rationale` after a denial, and asks again on a tap.
- Step 4's two requests made back to back were seen answered in order on
  the S23 (`CHANGES.md`, "Permission ids and the queue on the S23"). No
  example keeps that check: an app asks for what it uses. A host unit test
  of the answer's id and names stays.
