---
title: Asking the user for a photo or video through Android's own chooser
date: 2026-10-28
status: Draft
covers:
  - "Feature `media-request`: one call that opens Android's chooser for
    media (`ACTION_GET_CONTENT` in `Intent.createChooser`), with an
    optional camera entry (`ACTION_IMAGE_CAPTURE`, `ACTION_VIDEO_CAPTURE`
    through `EXTRA_INITIAL_INTENTS`), or the system Photo Picker
    (`MediaStore.ACTION_PICK_IMAGES`) when the app prefers it"
  - "Results: each chosen item's bytes or a file in the app's cache,
    with its content URI, MIME type, display name and size, under a size
    cap; multiple selection; the camera entry's full-size photo or video;
    cancellation and errors; `MobileEvent::Media`"
  - "A content provider of libmobile's own (no AndroidX `FileProvider`)
    that lets the camera app write into the app's cache, declared in the
    generated manifest with the app's id filled in by the Gradle script"
  - "The CAMERA permission for the camera entry: never in the manifest
    because of this feature; asked for on use, through the same lazy gate
    as the in-process camera, only when the manifest declares CAMERA"
  - "Threading, pause and resume, the activity being recreated, and the
    process dying while the chooser or camera is up"
  - "How it sits beside `camera-intent` (the straight hand-off to the
    camera app) and the in-process `camera` base"
sources:
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"im thinking
    of media-request as open the media selector which has a camera
    option\"; then \"you can plan the media request in a new plan, we'll
    need it soon anyway\"."
  - "Owner's direction relayed with the request: no Jetpack/AndroidX if
    avoidable; permissions asked for only when used, never at launch; a
    refusal fails the call with the reason (`settings_only` when Android
    will not ask again); the app may then call `open_app_settings`
    (`docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md`, 3.3)."
  - "libmorph `docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md`
    (Hold), its amendment of 2026-10-28 13:35 EDT: libmobile's `camera`
    (the system camera app hand-off) is renamed `camera-intent`; `camera`
    becomes the in-process NDK camera base; item 3 names a later
    `media-request` as a separate idea. This plan is that idea."
  - "libmobile today: `Cargo.toml:33` (`camera = []`); `src/camera.rs:15`
    (`CaptureOutcome`); `src/android/camera.rs:18,26-40,45` (the mailbox,
    `capture_photo`, the JNI result export);
    `android/java/dev/highbay/libmobile/camera/CameraPlugin.kt:27`
    (`REQUEST_CODE = 0x4c4d`), `:32-43` (`startActivityForResult` with a
    bare `ACTION_IMAGE_CAPTURE`), `:45-70` (the `data` thumbnail
    re-encoded as JPEG), `:20-21` (CAMERA in the manifest makes the
    intent need it granted)."
  - "Activity results: `MobileNativeActivity.kt:66-71` (one
    `onActivityResult`, dispatched to the plugins until one returns
    true), `MobileActivityPlugin.kt:34-38` (the hook; each plugin owns a
    request code), `MobileNativeActivity.kt:31-37` (`NativeLib.ensureLoaded`
    in `onCreate`, before any plugin hook)."
  - "Permissions: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md`;
    `src/android/permission.rs:37,62,73` (`request_permissions`,
    `permission_granted`, `should_show_rationale`);
    `bridge/PermissionPlugin.kt:23-84` (one request at a time, queue
    cleared on destroy)."
  - "Events and the loop: `src/event.rs:128-130` (`MobileEvent`,
    `#[non_exhaustive]`), `:164-165` (`Capture` under `camera`), `:177`
    (`Permissions`); `src/android/mod.rs:121-153` (`run`), `:174-206`
    (`pump_bridges`), `:225-238` (`discard_stale_commands`);
    `src/mailbox.rs:21` (`Mailbox`)."
  - "Manifest generation: `src/codegen.rs:47-58` (`Contributes`, whose
    `application` list is for `<provider>` elements), `:92` (`PLUGINS`),
    `:160-175` (the camera plugin's `IMAGE_CAPTURE` `<queries>` and
    optional `camera.any`), `:306-323` (`merge`, exact-string dedup),
    `:380-425` (`LibmobileManifestTask`, which splices contributions into
    the already-merged manifest, after Gradle has replaced placeholders),
    `:519` (`minSdk = 29`)."
  - "Visibility: `bridge/LinkPlugin.kt:40-50` (an activity start from an
    app that is not visible is refused; checked with
    `ActivityManager.getMyMemoryState`), `src/link.rs:13-23`
    (`OpenLinkError::NotVisible`)."
  - "Android platform (framework only): `Intent.ACTION_GET_CONTENT`,
    `CATEGORY_OPENABLE`, `EXTRA_MIME_TYPES`, `EXTRA_ALLOW_MULTIPLE`,
    `Intent.createChooser`, `EXTRA_INITIAL_INTENTS`;
    `MediaStore.ACTION_IMAGE_CAPTURE`, `ACTION_VIDEO_CAPTURE`,
    `EXTRA_OUTPUT`; `MediaStore.ACTION_PICK_IMAGES`,
    `EXTRA_PICK_IMAGES_MAX`, `getPickImagesMaxLimit()` (API 33, or API 30+
    with SDK extension R >= 2); `Intent.ACTION_OPEN_DOCUMENT`;
    `ContentResolver.openInputStream`, `OpenableColumns`;
    `android.content.ContentProvider`; `android.media.ExifInterface`."
related:
  - docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (Draft; the
    in-process camera, its CAMERA gate and `open_app_settings`)
  - docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md (the permission
    path the gate uses)
  - docs/design/20261023_04_APP_DIRECTORIES_PLAN.md (`cache_dir`, where
    results land)
  - libmorph docs/design/20261028_02_MEDIA_REQUEST_PLAN.md (wraps this
    plan for every host)
  - libmorph docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md
    (Hold; its 13:35 EDT amendment names the camera features this plan
    sits beside)
living sections:
  - "10. Tunable constants"
---

# Asking the user for a photo or video through Android's own chooser

## 1. What it is

`media-request` is one call: "give me an image (or video, or file) the
user picks, or one they take now". It opens the system's own media
chooser, a sheet listing the apps that can hand over content (Photos or
Gallery, Files, Drive) and, when the app asks for it, the camera. The
user picks or shoots; the app gets the bytes.

libmobile has nothing like it today. Its only media path is the camera
hand-off (`camera`, being renamed `camera-intent`): straight to the
camera app, no gallery, and only the thumbnail comes back
(`CameraPlugin.kt:51-58`), a bitmap of a few hundred pixels.

The three camera-related features after the rename, and where this one
sits:

| Feature | What the user sees | Permission | Result |
|---|---|---|---|
| `camera-intent` (today's `camera`) | the camera app, directly | none of its own | thumbnail JPEG |
| `camera` (in-process base, plan `20261028_01`) | the app's own UI | CAMERA, asked on first use | frames, QR |
| **`media-request`** (this plan) | Android's chooser: gallery, files, and an optional camera entry | none to pick; CAMERA for the camera entry only when the manifest declares it | the chosen items' bytes or files, full size |

`media-request` does not depend on either camera feature, and neither
depends on it. Keeping `camera-intent` is deliberate: it is the one-tap
"take a photo" with no chooser in the way.

Naming note: the body of the camera plan `20261028_01` still calls the
in-process base `camera-device`; the owner's decision recorded in
libmorph's Hold plan (13:35 EDT) makes it `camera`. This plan does not
depend on the name: what matters is whether the final manifest declares
CAMERA, which is checked at run time (4.3).

## 2. Which system screen, and when

| | `ACTION_GET_CONTENT` in `createChooser` | Photo Picker (`ACTION_PICK_IMAGES`) | `ACTION_OPEN_DOCUMENT` |
|---|---|---|---|
| Shows | every app that hands over content of the type (Photos, Gallery, Files, Drive...), plus our initial intents | the system's photo and video grid | the system file browser (DocumentsUI) |
| Camera entry | yes, through `EXTRA_INITIAL_INTENTS` | no | no |
| Types | any MIME, several through `EXTRA_MIME_TYPES` | images and/or videos only | any MIME |
| Availability | every API level | API 33+, or API 30-32 with SDK extension R >= 2 (Play system update); not on API 29 | every API level |
| Access | a temporary read grant on the returned URIs | same | a grant that can be made persistable |
| Multiple | `EXTRA_ALLOW_MULTIPLE` (the target app may ignore it) | `EXTRA_PICK_IMAGES_MAX`, enforced | `EXTRA_ALLOW_MULTIPLE` |
| AndroidX needed | no | no (the Jetpack `PickVisualMedia` contract only wraps this action plus a Play services backport for old devices, which we skip) | no |

Used:

- **`Picker::Chooser` (the default)**: `ACTION_GET_CONTENT` with
  `CATEGORY_OPENABLE`, wrapped in `Intent.createChooser`, with the camera
  entries as `EXTRA_INITIAL_INTENTS`. This is the owner's "media selector
  which has a camera option", it is on every device the generated
  projects support (`minSdk = 29`), and it accepts any MIME type.
- **`Picker::PhotoPicker`**: `ACTION_PICK_IMAGES`, when the app asks for
  it **and** no camera entry is asked for **and** the kind is images
  and/or videos **and** the device has it (API 33+, or API 30+ with
  `SdkExtensions.getExtensionVersion(R) >= 2`). Otherwise the request
  quietly uses the chooser, and the reply says which was used
  (`MediaReply::picker`). The Photo Picker is the better screen for
  photos only (fast, private, enforced limits), but it has no camera.
- **`ACTION_OPEN_DOCUMENT`: not used.** Its only gain is a persistable
  grant to re-read the item later; this plan copies the item once. On
  many devices DocumentsUI is already one of the chooser's entries (as
  "Files").

No Jetpack `ActivityResultContracts`: the plugin uses
`startActivityForResult` with its own request code, as `CameraPlugin`
does, routed by `MobileNativeActivity.onActivityResult`.

## 3. Features and manifest

```toml
[features]
# Ask the user for media through Android's chooser (gallery, files, and an
# optional camera entry) or the Photo Picker
# (android/java/dev/highbay/libmobile/media, src/android/media.rs). Adds a
# private content provider for camera output and `<queries>` for the
# camera apps; no permission of its own.
media-request = []
```

The plugin in `PLUGINS` (`src/codegen.rs`), after the camera plugin(s):

```rust
// Media requests (`media/`, `src/android/media.rs`). The provider gives
// the camera app a URI to write a full-size photo or video into the
// app's cache; `${applicationId}` is filled in by the Gradle script (3.1).
Plugin {
    feature: Some("media-request"),
    dir: "media",
    class: "media.MediaRequestPlugin",
    contributes: Contributes {
        manifest: &[
            // Package visibility (API 30+): the camera apps the chooser's
            // camera entries are built from (4.2). The first string is the
            // camera plugin's own, so `merge` emits it once with both.
            "<queries><intent><action android:name=\"android.media.action.IMAGE_CAPTURE\" /></intent></queries>",
            "<queries><intent><action android:name=\"android.media.action.VIDEO_CAPTURE\" /></intent></queries>",
        ],
        application: &[
            "<provider android:name=\"dev.highbay.libmobile.media.MediaFileProvider\" android:authorities=\"${applicationId}.libmobile.media\" android:exported=\"false\" android:grantUriPermissions=\"true\" />",
        ],
        ..Contributes::NOTHING
    },
},
```

No `<uses-permission>`: picking needs none (the chooser grants read
access to exactly the chosen items), and the camera entry needs none
unless something else declared CAMERA (4.3). No `READ_EXTERNAL_STORAGE`
or `READ_MEDIA_*`. No `uses-feature`: a device without a camera simply
gets no camera entry.

### 3.1 The app id in the provider's authority

`LibmobileManifestTask` (`src/codegen.rs:380-425`) splices contributions
into the **merged** manifest, after Gradle has already replaced
`${applicationId}`, so the placeholder would reach the APK unreplaced. The
task gains one input and one replacement:

```groovy
@Input abstract Property<String> getApplicationId()
// in splice(), before building `app`:
def appXml = applicationXml.get().collect { it.replace('${applicationId}', applicationId.get()) }
```

registered with `applicationId.set(variant.applicationId)`. Every other
contribution is unchanged (none uses the placeholder). In Kotlin the
authority is `context.packageName + ".libmobile.media"`; the package name
is the application id at run time.

### 3.2 `MediaFileProvider` instead of AndroidX `FileProvider`

AndroidX `FileProvider` would add `androidx.core` and an XML paths
resource. libmobile's provider is about 80 lines of framework Kotlin
(`android.content.ContentProvider`), serving exactly one directory:

- URIs are `content://<package>.libmobile.media/out/<name>`; `<name>`
  must match `^[A-Za-z0-9._-]{1,96}$` and is resolved to
  `<cacheDir>/libmobile-media/<name>`; the canonical path must stay in that
  directory. Anything else: `FileNotFoundException`.
- `openFile(uri, mode)`: `ParcelFileDescriptor.open(file,
  ParcelFileDescriptor.parseMode(mode))` (the camera app opens it `"w"`
  or `"rw"`).
- `query`: a `MatrixCursor` with `OpenableColumns.DISPLAY_NAME` and
  `SIZE` (some camera apps query before writing).
- `getType`: `image/jpeg` for `.jpg`, `video/mp4` for `.mp4`, else
  `application/octet-stream`.
- `insert`, `update`, `delete`: unsupported (`null` / 0).
- `exported="false"`, `grantUriPermissions="true"`: only apps holding a
  per-URI grant from us can open a file, and only that file.

## 4. How a request runs

### 4.1 From Rust to the chooser

1. `request_media(app, &request)` takes the next `MediaRequestId`
   (ids count from 1) and returns it at once. Any thread except a JNI
   callback.
2. If a request is already in flight: the reply is
   `Err(MediaError::Busy)` (pushed to the mailbox, delivered on the next
   tick). One at a time, as Android shows one chooser at a time.
3. If a camera entry is asked for: the CAMERA check of 4.3, which may
   wait for the permission dialog.
4. JNI `MediaRequestPlugin.request(activity, id, spec)` where `spec` is
   flattened to primitives and string arrays (MIME types, flags, limits).
   On the UI thread the plugin:
   - refuses with `not-visible` when the app is not foreground or visible
     (the `LinkPlugin.kt:44-50` check);
   - deletes leftovers in `<cacheDir>/libmobile-media/` older than
     `MEDIA_CACHE_MAX_AGE` (10);
   - builds the intent (4.2), writes the pending record (6.3), and calls
     `startActivityForResult(intent, REQUEST_CODE)` with
     `REQUEST_CODE = 0x4d52` ("MR"), distinct from `CameraPlugin`'s
     `0x4c4d` and `PermissionPlugin`'s `0x504d`;
   - maps `ActivityNotFoundException` to `no-app` and a
     `SecurityException` to `platform`.

### 4.2 The intent

Content (both pickers):

| `MediaKind` | `GET_CONTENT` type / `EXTRA_MIME_TYPES` | `PICK_IMAGES` type |
|---|---|---|
| `Images` (default) | `image/*` | (none: images only by default) |
| `Videos` | `video/*` | `video/*` |
| `ImagesAndVideos` | `*/*` + `["image/*", "video/*"]` | `*/*` (both) |
| `Any` | `*/*` | Photo Picker not used |
| `Mime(list)` | one entry: that type; several: `*/*` + the list | not used |

`GET_CONTENT` always adds `CATEGORY_OPENABLE` (only items
`openInputStream` can read). With `max_items > 1`:
`EXTRA_ALLOW_MULTIPLE = true` (a target app may still return one), or
`EXTRA_PICK_IMAGES_MAX = min(max_items, getPickImagesMaxLimit())`.

Camera entries (chooser only), for `CameraEntry::Photo`, `Video` or
`PhotoAndVideo`:

1. The plugin creates the output file(s), empty:
   `<cacheDir>/libmobile-media/cam-<id>.jpg` and/or `cam-<id>.mp4`, and
   their URIs from the provider.
2. For each activity `packageManager.queryIntentActivities` returns for
   `ACTION_IMAGE_CAPTURE` (and/or `ACTION_VIDEO_CAPTURE`): an intent with
   that action, `setComponent` to that activity, `EXTRA_OUTPUT` = the URI,
   `ClipData.newRawUri("", uri)` and
   `FLAG_GRANT_WRITE_URI_PERMISSION | FLAG_GRANT_READ_URI_PERMISSION`, and
   an explicit `grantUriPermission(package, uri, write|read)` for that
   package. The explicit grant is what makes it work when the chooser,
   not our activity, starts the camera; the flags cover the cases where
   the platform carries them through.
3. All of them go into `EXTRA_INITIAL_INTENTS` of
   `Intent.createChooser(content, title)`. One entry per camera app, so a
   device with two camera apps shows both.
4. No camera app found (`queryIntentActivities` empty): no camera entry,
   and the reply's `camera_left_out` is `true`.

The chooser title is `MediaRequest::title`, or `null` for the system's
default.

### 4.3 The camera entry and CAMERA

Android refuses to start `ACTION_IMAGE_CAPTURE` or
`ACTION_VIDEO_CAPTURE` for an app whose manifest declares CAMERA but
does not hold it (`CameraPlugin.kt:20-21`). In a chooser that refusal
happens when the user taps the camera, inside the system's sheet, where
the app cannot report it. So the check happens before the chooser opens:

- `MediaRequestPlugin.declares(activity, "android.permission.CAMERA")`:
  whether the app's own manifest requests it
  (`PackageInfo.requestedPermissions`, with
  `PackageManager.PackageInfoFlags.of(GET_PERMISSIONS)` on API 33+). This
  is true when the in-process `camera` feature is on, and also when the
  app or another library declared it.
- Not declared: no check, no dialog; the camera entry needs no permission.
  An app with `media-request` and no camera feature never sees a CAMERA
  dialog and has no CAMERA in its manifest.
- Declared and granted: proceed.
- Declared and not granted: by `MediaRequest::camera_permission`:
  - **`CameraPermission::Ask` (default)**: the camera entry is the use of
    the camera, so this is where CAMERA is asked for, through the same
    lazy gate as the in-process camera (camera plan 3.2): one
    `request_permissions(&[CAMERA])`, its answer consumed by the gate and
    not delivered as `MobileEvent::Permissions`. Granted: the chooser
    opens with the camera entry. Refused: the request fails with
    `MediaError::PermissionDenied { settings_only }`,
    `settings_only = !should_show_rationale(CAMERA)`; the chooser does not
    open. The app may call `open_app_settings` (camera plan 3.3).
  - **`CameraPermission::IfGranted`**: never ask; open the chooser
    without the camera entry, `camera_left_out: true`.

The gate: the camera plan builds it in
`src/android/camera_device/gate.rs` under the camera feature. Both
plans need it with no dependency on each other, so it becomes a core
module, `src/android/permission_gate.rs` (no feature): a table of
`(PermissionRequest, waiter)` that `pump_bridges` offers each
`PermissionsOutcome` to before delivering it as
`MobileEvent::Permissions`. Whichever plan's step lands first builds it
there; the other uses it (a departure for the camera plan, to be recorded
in `CHANGES.md` if that plan is frozen by then).

### 4.4 The answer

`MediaRequestPlugin.onActivityResult(requestCode == 0x4d52)` returns
`true` at once and hands the work to a single background thread
(`libmobile-media`, a `java.util.concurrent` executor), because reading a
cloud item can block for seconds:

1. `resultCode != RESULT_OK`: `cancelled`.
2. Returned URIs: `data.clipData` items, else `data.data`. Our own output
   URI, if a camera app echoes it, is not a picked item.
3. No returned URI, and `cam-<id>.jpg` (or `.mp4`) is non-empty: the
   camera entry was used; that file is the item, `MediaSource::Camera`.
   A camera app that ignores `EXTRA_OUTPUT` for video and returns its own
   `content://` URI is read like a picked item, with
   `MediaSource::Camera` when the URI came back with no camera file
   written and a video camera entry was offered.
4. Neither: `Failed("no_item")`.
5. More items than `max_items` (a target app ignoring the limit): the
   first `max_items` are kept, and `MediaReply::truncated` is `true`.
6. Each picked URI: `contentResolver.getType`, a query of
   `DISPLAY_NAME` and `SIZE`, then a copy from `openInputStream` to
   `<cacheDir>/libmobile-media/<id>-<n>.<ext>` (extension from the MIME
   type, else `bin`) in 64 KiB chunks. When `SIZE` already exceeds
   `max_bytes`, or the stream passes `max_bytes`, the copy stops, the
   partial file is deleted and the item is `TooLarge { size }`. Any
   exception: `Unreadable(<exception class>)` (not its message, which
   may hold a path or name).
7. For `image/jpeg` items (picked or camera):
   `android.media.ExifInterface(file).getAttributeInt(TAG_ORIENTATION, 0)`
   into `exif_orientation` (0 when absent). The pixels are not rotated.
8. Grants we made for the camera URIs are revoked
   (`revokeUriPermission`); the empty camera file of an unused entry is
   deleted; the pending record is deleted.
9. One JNI call, `nativeMediaResult(id, restored, error, kinds[], uris[],
   mimes[], names[], sizes[], paths[], itemErrors[], exif[], picker,
   cameraLeftOut, truncated)`, on that background thread.

In Rust (still on that thread, never the main loop) the export builds the
`MediaResponse`. For `Delivery::Bytes` it reads each file with
`std::fs::read` and deletes it; for `Delivery::File` it hands over the
path. Large items therefore never cross JNI as Java arrays and never sit
in the Java heap: Kotlin streams to a file, Rust reads the file. The
response goes into a `Mailbox<MediaResponse>` drained in `pump_bridges` as
`MobileEvent::Media`.

The URI string, the display name and the bytes are never logged: only
counts, MIME types and sizes.

## 5. Rust API

Platform-free types in `src/media.rs` (every host, host-tested, under
`media-request`); the Android calls in `src/android/media.rs`,
re-exported as `libmobile::android::media`.

```rust
/// One `request_media` call. Opaque; ids count from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaRequestId(pub(crate) u32);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaKind {
    #[default]
    Images,
    Videos,
    ImagesAndVideos,
    /// Any openable file.
    Any,
    /// These MIME types (`"application/pdf"`, `"image/png"`, ...).
    Mime(Vec<String>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraEntry {
    #[default]
    None,
    Photo,
    Video,
    PhotoAndVideo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraPermission {
    /// Ask for CAMERA if the manifest declares it and it is not granted;
    /// a refusal fails the request (4.3).
    #[default]
    Ask,
    /// Never ask; leave the camera entry out instead.
    IfGranted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Picker {
    /// `ACTION_GET_CONTENT` in the system chooser, with camera entries.
    #[default]
    Chooser,
    /// The Photo Picker when it can serve the request (2), else `Chooser`.
    PhotoPicker,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delivery {
    /// The bytes in memory; the cache file is deleted.
    #[default]
    Bytes,
    /// A file under `cache_dir()/libmobile-media/`, the app's to move or
    /// delete. Files left there are deleted after `MEDIA_CACHE_MAX_AGE`.
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaRequest {
    pub kind: MediaKind,                    // Images
    pub camera: CameraEntry,                // None
    pub camera_permission: CameraPermission,// Ask
    pub picker: Picker,                     // Chooser
    /// 1 = single selection. Clamped to 1..=MEDIA_MAX_ITEMS.
    pub max_items: u16,                     // 1
    /// Per item. Default MEDIA_MAX_BYTES (32 MiB).
    pub max_bytes: u64,
    pub delivery: Delivery,                 // Bytes
    /// The chooser's title; `None` for the system's.
    pub title: Option<String>,
}
impl Default for MediaRequest { /* as commented */ }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSource { Picked, Camera }

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaData {
    Bytes(Vec<u8>),
    File(std::path::PathBuf),
    /// Over `max_bytes`; `size` when the provider reported it.
    TooLarge { size: Option<u64> },
    /// The item could not be read (the exception's class, for logs).
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaItem {
    pub source: MediaSource,
    /// The `content://` URI as returned (ours for a camera item). Read
    /// access to it ends when the activity finishes; use `data`.
    pub uri: String,
    pub mime: Option<String>,
    pub display_name: Option<String>,
    pub size: Option<u64>,
    /// JPEG EXIF orientation 1..=8, 0 when absent or not a JPEG.
    pub exif_orientation: u8,
    pub data: MediaData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaReply {
    pub items: Vec<MediaItem>,
    /// Which screen was shown (`PhotoPicker` may fall back to `Chooser`).
    pub picker: Picker,
    /// A camera entry was asked for but not shown (no camera app, or
    /// `IfGranted` without CAMERA).
    pub camera_left_out: bool,
    /// The app returned more than `max_items`; the rest were dropped.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaError {
    /// The user backed out of the chooser, the picker or the camera.
    Cancelled,
    /// Another request is in flight.
    Busy,
    /// Nothing on the device handles the request.
    NoApp,
    /// The app was not visible, and Android does not let it start one.
    NotVisible,
    /// CAMERA refused for the camera entry. `settings_only`: asking again
    /// shows no dialog; send the user to `open_app_settings`.
    PermissionDenied { settings_only: bool },
    /// Anything else, for logs.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaResponse {
    /// The request this answers; `None` when it was made before the
    /// activity or the process was recreated (6.2, 6.3).
    pub request: Option<MediaRequestId>,
    pub result: Result<MediaReply, MediaError>,
}
```

```rust
// src/android/media.rs
/// Ask the user for media (2, 4). Returns at once; the answer is
/// `MobileEvent::Media` with this id. Needs the app visible.
pub fn request_media(app: &AndroidApp, request: &MediaRequest) -> MediaRequestId;

/// Whether the Photo Picker is available on this device (2).
pub fn photo_picker_available(app: &AndroidApp) -> bool;
```

In `src/event.rs`:

```rust
/// The answer to a `request_media` call.
#[cfg(feature = "media-request")]
Media(crate::media::MediaResponse),
```

A reply with zero items never happens: no item is `Failed("no_item")`,
and per-item failures (`TooLarge`, `Unreadable`) are items.

## 6. Threads and lifecycle

### 6.1 Threads and pause

- `request_media` sends one JNI call and returns. The permission gate
  waits in the main loop (`pump_bridges`), not on a thread.
- The chooser, picker or camera pauses and stops the activity:
  `MobileEvent::Pause` and `Stop` arrive as for any other activity on
  top. Nothing in libmobile closes on pause for this feature; the request
  stays in flight. The answer arrives before `Resume` (Android delivers
  `onActivityResult` before `onResume`), is copied on `libmobile-media`,
  and reaches the app on a tick after it; with large items possibly after
  `Resume`.
- `MobileEvent::Media` is delivered whether or not the app is resumed.

### 6.2 The activity recreated, the process alive

With `configChanges` covering rotation (`src/codegen.rs:660`), a
recreation means the activity was destroyed while in the background (for
example "Don't keep activities", or the system reclaiming it). The
plugin is a Kotlin `object` and the Rust statics live on, so the answer
still arrives, now in a new `android_main` run whose app state knows
nothing of the id. libmobile counts `run` calls (a static run number);
each pending request records its run, and an answer for a request of an
earlier run is delivered with `request: None`. `discard_stale_commands`
already drops mailbox items left at the end of a run; an answer that
arrives later belongs to the new run as `None`.

### 6.3 The process killed while the chooser or camera is up

Common in practice: a camera app needs memory, and the app is in the
background. Android restarts the process, recreates the activity, and
delivers `onActivityResult` with the request code to the new instance
(after `onCreate`, where `NativeLib.ensureLoaded` has registered the
natives, `MobileNativeActivity.kt:31-37`). The camera output file in
`cacheDir` survives; memory does not.

- Before `startActivityForResult`, the plugin writes
  `<cacheDir>/libmobile-media/pending` (a few `key=value` lines: id,
  max_items, max_bytes, delivery, camera file names, time).
- `onActivityResult` with `0x4d52` and no request in memory: read that
  record, handle the answer as in 4.4 with its settings, and deliver it
  with `restored = true`, which Rust reports as `request: None`. Without
  a record: `request: None`, `Err(Failed("lost"))`.
- Rust has no gate state after a restart, and needs none: the
  permission was settled before the chooser opened.
- In libmorph and other wrappers the answer can arrive before the app
  object exists (before the window); the wrapper holds it until the app
  is created (libmorph's companion plan, 3).

### 6.4 Destroy

`MediaRequestPlugin.onDestroy` keeps the in-flight request (the answer
may come to the next instance, 6.2). Nothing is cancelled: the chooser
cannot be withdrawn once the user is in another app.

## 7. Versioning

Additive: a new feature, new modules and types, a variant of the
`#[non_exhaustive]` `MobileEvent`, and a manifest change only for apps
that turn the feature on. The manifest task's new input changes the
generated `libmobile.gradle` text for every app with manifest
contributions, but not the resulting manifest unless a contribution uses
`${applicationId}`. A patch release under Rule 5.4: the next libmobile
patch, or, if the camera rename (0.3.0) is released first, part of
whichever release comes next, listed as added.

## 8. Steps

Each step one commit to libmobile, with its tests, passing
`cargo fmt -- --check`, host clippy with no features and
`--all-features`, Android clippy (`--target aarch64-linux-android`) with
no features, `--features media-request` and `--all-features`, and
`cargo test --workspace`.

1. **Feature, types and manifest** (3, 3.1, 5): the `media-request`
   feature, `src/media.rs` with the types and their `Default`s, the
   plugin entry, the `applicationId` input of `LibmobileManifestTask`,
   and `MediaFileProvider.kt` with an empty `MediaRequestPlugin.kt`.
   Host tests: the provider and both `<queries>` appear only with
   `media-request`; `IMAGE_CAPTURE` appears once with `media-request` and
   the camera hand-off together; no `<uses-permission>` comes from
   `media-request`; the generated script replaces `${applicationId}`; the
   provider's name check accepts `cam-7.jpg` and refuses `../x`, `a/b`
   and an empty name (the check is a pure function, also in Rust for the
   test).
   **Done when:** the checks pass, a generated project with
   `media-request` builds with `gradle assembleDebug`, its merged
   manifest holds the provider with the real application id, and the
   generated manifest of an app without the feature is byte-identical to
   today's.
2. **The chooser without a camera** (4.1, 4.2 content part, 4.4, 5):
   `request_media`, the plugin's intent, `onActivityResult`, the copy
   thread, `nativeMediaResult`, `Delivery::Bytes` and `File`, the size
   cap, `max_items`, `MobileEvent::Media`, `Busy`, `NotVisible`. Host
   tests: building `MediaResponse` from the flattened JNI parts (every
   error string, per-item failures, `truncated`); the MIME table of 4.2.
   **Done when:** the checks pass.
3. **Camera entries and CAMERA** (4.2 camera part, 4.3): output files,
   per-app initial intents with grants and revokes, the run-time
   `declares` check, the core `permission_gate.rs` (or the camera plan's,
   if built first), `CameraPermission::Ask` and `IfGranted`,
   `camera_left_out`. Host tests: the gate's handling of grant, denial and
   `settings_only` for a media waiter; its answer is not delivered as
   `MobileEvent::Permissions`.
   **Done when:** the checks pass.
4. **Photo Picker** (2): `photo_picker_available`, `ACTION_PICK_IMAGES`
   with `EXTRA_PICK_IMAGES_MAX`, the fallback rules, `MediaReply::picker`.
   Host test: the fallback decision for every combination of kind, camera
   entry and availability (a pure function).
   **Done when:** the checks pass.
5. **Recreation and process death** (6.2, 6.3): the run number, the
   pending record, `request: None`, cache cleanup by age.
   **Done when:** the checks pass, and a host test parses and rejects
   pending records (missing keys, a bad id).
6. **Example `examples/mediarequest`** (its own commit, a workspace
   member): a CPU-drawn screen (as `examples/bitmap`) with four taps:
   pick one image (`Bytes`), pick up to five images and videos (`File`),
   chooser with a photo entry, Photo Picker; the replies logged (counts,
   MIME types, sizes, `exif_orientation`), the first image decoded and
   shown; a "Settings" tap on `settings_only`.
   **Done when:** on the owner's S23 (the owner asked first; checks
   recorded in `CHANGES.md`, evidence under `out/media/`): launch shows no
   dialog; the chooser lists the gallery apps and Camera; a gallery pick
   returns full-size bytes; a camera shot returns a full-size JPEG (not a
   thumbnail) with its EXIF orientation; back from the chooser and back
   from the camera each give `Cancelled`; five items come back with
   `File`; the Photo Picker opens and respects the limit; an item over a
   1 MiB cap gives `TooLarge`; with the process killed while the camera
   is open (`adb shell am kill` after backgrounding), the shot still
   arrives with `request: None`; an APK built with `media-request` and
   the in-process `camera` asks for CAMERA when the camera chooser is
   requested, not at launch, and a double refusal gives
   `settings_only: true`.
7. **README** (its own commit): the features table row, the layout line
   for `media/`, a usage section, and this plan's decision-table row
   updated.
   **Done when:** the README lists the feature, its manifest effects and
   the call.

## 9. Risks

- **Grants through the chooser.** Whether the camera app can write to
  our URI when the chooser starts it is the fragile part on some OEMs;
  the explicit per-package `grantUriPermission` is the known remedy.
  Step 6 checks the S23's camera app; other camera apps (Open Camera,
  GCam ports) are untested.
- **What `GET_CONTENT` shows.** Recent Android versions route image and
  video `GET_CONTENT` to the Photo Picker or DocumentsUI rather than
  listing each gallery app, and One UI may differ again. The chooser
  still shows the camera entries beside whatever handles the content;
  step 6 records what the S23 shows.
- **Camera apps that ignore `EXTRA_OUTPUT`** (mostly for video) return
  their own URI or nothing; 4.4 step 3 reads the URI when there is one,
  else `Failed("no_item")`.
- **HEIC.** Samsung and Pixel galleries hold HEIC photos; `GET_CONTENT`
  with `image/*` may return them as is. libmobile returns the bytes
  unchanged; the app needs a decoder or asks for `Mime(["image/jpeg"])`
  (which some galleries honour by converting, and some do not).
- **Memory.** `Bytes` holds the whole item in the Rust heap; video
  should use `File`. The default cap (32 MiB) bounds it.
- **Process death without a record.** If the cache dir was cleared
  between launch and result, the answer is `Failed("lost")`.
- **Photo Picker on API 30-32** needs the SDK extension; without it the
  request falls back to the chooser, which is correct but looks
  different.

## 10. Tunable constants (living section)

| Name | Value | Meaning |
|---|---|---|
| `MEDIA_MAX_BYTES` | 32 MiB | default per-item cap |
| `MEDIA_MAX_ITEMS` | 100 | ceiling on `max_items` |
| `MEDIA_COPY_CHUNK` | 64 KiB | stream copy buffer |
| `MEDIA_CACHE_MAX_AGE` | 24 h | files in `libmobile-media/` older than this are deleted at the next request |
| `REQUEST_CODE` | `0x4d52` | the plugin's activity request code |

## 11. Open questions

1. **A refused CAMERA.** With `Ask`, a refusal fails the whole request,
   per the owner's rule. Should it instead open the chooser without the
   camera entry (the gallery still works) and report the refusal in the
   reply?
2. **Default picker.** The chooser (with its camera entry) by default,
   and the Photo Picker only on request; or the Photo Picker by default
   whenever no camera entry is asked for?
3. **`camera-intent` full size.** The provider here could also give
   `camera-intent` a full-size photo instead of the thumbnail (an option
   on `capture_photo`). In a later plan, or not at all?
4. **The shared gate.** Move the camera plan's gate to the core
   `src/android/permission_gate.rs` as 4.3 says (and record that in the
   camera plan once agreed)?
5. **Video options.** `EXTRA_DURATION_LIMIT` and `EXTRA_VIDEO_QUALITY`
   for the video camera entry, now or later?
6. **Cancel.** `finishActivity(REQUEST_CODE)` can withdraw the chooser
   itself, but not a camera app it started. Is a `cancel_media` worth
   having for that narrow case?
7. **URI only.** A `Delivery::Uri` that copies nothing, for apps that
   pass the URI straight to another intent (share, crop)? The read grant
   ends with the activity, so it is of limited use.
