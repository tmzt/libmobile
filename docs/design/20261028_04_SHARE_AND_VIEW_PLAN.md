---
title: Viewing and sharing the app's files, receiving shares, and decoding images through Android
date: 2026-10-28
status: Draft
covers:
  - "Feature `share-out`: a read-only content provider of libmobile's own
    (no AndroidX `FileProvider`), authority
    `${applicationId}.libmobile.share`, serving files the app wrote under
    `cache_dir()/libmobile-share/`; `view_file` (`ACTION_VIEW` in the
    system chooser, \"Open in…\") and `share_file` (`ACTION_SEND` with
    `EXTRA_STREAM`, in the chooser); a read grant only; pruning old files"
  - "Feature `share-target`: the app as an `ACTION_SEND` and
    `ACTION_SEND_MULTIPLE` target for `image/*` and `text/plain`, through a
    small translucent receiver activity that copies the shared streams
    through the `ContentResolver` under a size cap while it still holds
    the read grant, then brings the app forward; the text and the items as
    `MobileEvent::Shared`"
  - "Feature `image-codec`: `probe_image`, `decode_image(bytes, max_side)
    -> Rgba` through `ImageDecoder` (EXIF orientation applied, HEIC/HEIF
    read, sRGB, straight alpha) and `encode_image(rgba, Jpeg { quality } |
    Png)` through `Bitmap.compress`, on any thread but the main loop, with
    errors that say why"
  - "Why `IntentData` stays as it is, and how the new event fixes, additively,
    that its stringified extras cannot carry a readable stream"
  - "The libmorph plan that surfaces these (later), and how flankr uses
    each"
  - "Not covered: the media chooser (`docs/design/20261028_02_MEDIA_REQUEST_PLAN.md`);
    sharing several files out at once; Direct Share shortcuts; writing to
    MediaStore"
sources:
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"plan the
    libmobile share-and-view plan\". A Draft only; no approval yet."
  - "flankr `docs/design/20261028_05_BINARY_FILES_PLAN.md` (read only):
    section 6.4 (the phone keeps received files in
    `cache_dir()/libmobile-share/fl-<hash16>.<ext>`, 64 MiB in all, least
    recently viewed deleted first); section 9.1 (Share → flankr for PNG,
    JPEG, WebP and GIF, a Send to… sheet); section 10.2 (the preview's
    share icon with Open in… and Share); 10.5 (Open in… and Share write
    nothing new: the bytes are already in `libmobile-share/`; a `git`
    output is shared as `<request>.diff`, `text/x-diff`); 10.6 (this plan:
    the provider with names `^[A-Za-z0-9._-]{1,96}$`, `exported=\"false\"`,
    `grantUriPermissions=\"true\"`, feature `share-out`; `view_file(path,
    mime)`, `share_file(path, mime, title)`; feature `share-target` with
    `MobileEvent::Shared { items }`; libmorph surfacing it)."
  - "The same plan's amendment of 2026-10-28 21:29 EDT, item 6: no
    `zune-jpeg`, `image-webp` or `gif` in the app; thumbnails, the
    preview's images and conversion decode through Android's
    `ImageDecoder` (EXIF orientation, HEIC/HEIF) and encode with
    `Bitmap.compress`, through this plan's `decode_image(bytes, max_side)
    -> Rgba` and `encode_image(rgba, Jpeg { quality } | Png) -> Vec<u8>`;
    item 7 (conversion: HEIC/HEIF, an EXIF orientation other than 1, or
    longer than 2048 px is decoded with rotation, scaled to 2048 px and
    encoded as JPEG 90, or PNG with transparency)."
  - "The same plan's amendment of 2026-10-28 23:43 EDT (the bottom bar
    plan's + replaces the paperclip; versions by release order) and of
    23:48 EDT (approved; the preview's share icon, the preview's images on
    Android and step 9's attaching wait for this plan)."
  - "flankr `docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md` (Draft):
    the + opens the media chooser (`MorphContext::media_request`, decision
    8 and the terminal keys row); it names neither Share → flankr nor
    decoding, which come through the binary files plan above."
  - "libmobile today (0.3.2, `db05ffb`; HEAD `4edeca4`): `src/intent.rs:12-25`
    (`IntentData`, every extra stringified with `toString()`);
    `bridge/IntentPlugin.kt:45-47` (the stringifying); `src/codegen.rs:47-58`
    (`Contributes`, its `application` list), `:97-264` (`PLUGINS`),
    `:418-464` (`LibmobileManifestTask`, which splices into the merged
    manifest after placeholders are replaced);
    `src/gradle.rs:205-221,458-468` (URL schemes, generated projects only);
    `src/android/dirs.rs` (`cache_dir`, any thread);
    `src/android/link.rs` and `bridge/LinkPlugin.kt:40-90` (the
    `not-visible` check, `\"<kind>: <detail>\"` answers);
    `src/android/camera_intent.rs` (a feature's Rust/Kotlin split);
    `src/android/mod.rs:183-238` (`pump_bridges`); `src/mailbox.rs`;
    `NativeLib.kt` (`ensureLoaded(context)` for a component that is not the
    activity); `MobileActivityPlugin.kt` (hooks)."
  - "libmobile `docs/design/20261028_02_MEDIA_REQUEST_PLAN.md` (Draft):
    3.1 (the `applicationId` input of `LibmobileManifestTask`), 3.2
    (`MediaFileProvider`, the name rule this plan reuses), 4.4 (copying
    through `openInputStream` on a background thread, Kotlin to a file and
    Rust reads the file), 5 (`MediaItem`, `MediaData`, `Delivery`)."
  - "Android platform (framework only, no Jetpack): `ContentProvider`,
    `ParcelFileDescriptor.open`, `OpenableColumns`, `MatrixCursor`;
    `Intent.ACTION_VIEW`, `ACTION_SEND`, `ACTION_SEND_MULTIPLE`,
    `EXTRA_STREAM`, `EXTRA_TEXT`, `EXTRA_SUBJECT`, `Intent.createChooser`
    (which moves the target's data or `EXTRA_STREAM` into `ClipData` with
    its grant flags), `EXTRA_EXCLUDE_COMPONENTS` (API 24),
    `FLAG_GRANT_READ_URI_PERMISSION`; a temporary URI grant given to an
    activity ends when that activity is destroyed;
    `android.graphics.ImageDecoder` (API 28: `createSource(ByteBuffer)`,
    `setTargetSize`, `setAllocator(ALLOCATOR_SOFTWARE)`,
    `setUnpremultipliedRequired`, `setTargetColorSpace`, EXIF orientation
    applied, HEIF read, the first frame of an animation),
    `BitmapFactory.Options.inJustDecodeBounds`, `ExifInterface(InputStream)`,
    `Bitmap.copyPixelsToBuffer`, `copyPixelsFromBuffer`, `setPremultiplied`,
    `Bitmap.compress`."
related:
  - docs/design/20261028_02_MEDIA_REQUEST_PLAN.md (Draft; the other way in
    for images, its provider beside this one, its item types)
  - docs/design/20261023_04_APP_DIRECTORIES_PLAN.md (`cache_dir`)
  - docs/design/20261023_03_OPEN_LINK_PLAN.md (the visibility rule and
    answer shape the calls reuse)
  - docs/design/20261028_03_CLIPBOARD_PLAN.md (0.3.2, the release before
    this one)
  - flankr docs/design/20261028_05_BINARY_FILES_PLAN.md (the user; waits on
    this plan)
  - flankr docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md
  - libmorph, a later plan of the same name (wraps these for every host)
living sections:
  - "9. Tunable constants"
---

# Viewing and sharing the app's files, receiving shares, and decoding images through Android

## 1. What it is

Three small features, each off by default, each usable alone:

| Feature | What it gives | Manifest | Permission |
|---|---|---|---|
| `share-out` | `view_file` ("Open in…") and `share_file` ("Share") for a file the app wrote under `cache_dir()/libmobile-share/`; `prune` | a read-only `<provider>`; a `<queries>` entry for `ACTION_VIEW` | none |
| `share-target` | the app in other apps' share sheets for `image/*` and `text/plain`; what was shared, as `MobileEvent::Shared` | a translucent `<activity>` with two `<intent-filter>`s | none |
| `image-codec` | `probe_image`, `decode_image`, `encode_image` through Android's own codecs | nothing | none |

libmobile avoids Jetpack: the provider is framework `ContentProvider`
code, as media request's `MediaFileProvider` is (that plan, 3.2), and the
codec is `android.graphics` only. Nothing asks for a permission: a
provider grant covers sharing out, the sender's grant covers sharing in,
and decoding touches no storage.

## 2. `share-out`: Open in… and Share

### 2.1 The directory and its names

- The directory is `cache_dir()/libmobile-share/`. The app writes files
  there itself (flankr already does, binary files plan 6.4), then calls
  `view_file` or `share_file` with the path. `share_dir()` returns the
  directory, creating it.
- A file is served only when it sits **directly** in that directory and
  its name matches `^[A-Za-z0-9._-]{1,96}$` and is neither `.` nor `..`
  (media request's rule). The Rust call checks this before anything is
  sent (`ShareError::NotInShareDir`, `BadName`), canonicalising both the
  directory and the file's parent so a symlink out is refused; the
  provider checks it again on every open.
- The **display name**, what the receiving app shows and saves the file
  as, is separate from the stored name, so flankr can keep
  `fl-<hash16>.md` on disk and share it as `README.md`. It is 1-128
  characters, holds no `/`, `\` or control character, and is not `.` or
  `..` (`ShareError::BadDisplayName`). It defaults to the stored name.

### 2.2 The provider

`dev.highbay.libmobile.shareout.ShareFileProvider`, about 90 lines of
framework Kotlin:

- URIs are `content://<package>.libmobile.share/f/<name>/<display>`,
  built with `Uri.Builder().appendPath(...)` (so the display name is
  percent-encoded). The last path segment is the display name, for
  receiving apps that take a name from the URI instead of querying.
- `openFile(uri, mode)`: only `"r"`; any other mode is a
  `SecurityException`. Segment 1 is checked by the name rule and resolved
  to `<cacheDir>/libmobile-share/<name>`; its canonical path must have
  that directory as its parent; it must be a regular file. Otherwise
  `FileNotFoundException`. `ParcelFileDescriptor.open(file,
  MODE_READ_ONLY)`.
- `query`: a `MatrixCursor` of the requested columns among
  `OpenableColumns.DISPLAY_NAME` (segment 2) and `SIZE` (the file's
  length); unknown columns are left out.
- `getType`: by the display name's extension: `md`, `markdown` →
  `text/markdown`; `diff`, `patch` → `text/x-diff`; `txt` → `text/plain`;
  `png`, `jpg`/`jpeg`, `webp`, `gif`, `heic`, `pdf`, `json` → their types;
  else `application/octet-stream`. (The intent also carries the app's
  MIME type, which is what receivers mostly use.)
- `insert`, `update`, `delete`: unsupported (`null`, `0`).
- Declared `exported="false"` and `grantUriPermissions="true"`: only an
  app holding a per-URI grant from us can open a file, only that file,
  only for reading.

### 2.3 The calls

Both run on any thread (they reach the activity through `with_app_class`),
send one intent and return; neither waits for the other app, and there is
no result event, since Android does not report what the user chose.

1. Rust checks the path and names (2.1) and the MIME type: `type/subtype`
   of RFC 6838 token characters, lowercase, with no `*`
   (`ShareError::BadMime`). The file must exist (`NotFound`).
2. Rust sets the file's modification time to now
   (`File::set_modified`), which marks it as recently shared for `prune`
   (2.4).
3. JNI `ShareOutPlugin.view(activity, name, display, mime, title)` or
   `.share(...)`, answering `null` once the intent is sent, or
   `"<kind>: <detail>"` as `LinkPlugin` does. On the plugin:
   - `not-visible` when the app is not visible (the `LinkPlugin` check);
   - **view:** `Intent(ACTION_VIEW).setDataAndType(uri, mime)`,
     `ClipData.newUri(contentResolver, display, uri)`,
     `FLAG_GRANT_READ_URI_PERMISSION`. Before the chooser,
     `packageManager.queryIntentActivities(intent, 0)` empty → `no-app`
     (the `<queries>` entry of 2.5 makes other apps' viewers visible).
     Then `Intent.createChooser(intent, title)`, which carries the grant
     to whichever app the user picks;
   - **share:** `Intent(ACTION_SEND).setType(mime)`, `EXTRA_STREAM` =
     the URI, the same `ClipData` and grant, `EXTRA_SUBJECT` and
     `EXTRA_TITLE` = the display name; `Intent.createChooser(intent,
     title)`; with `share-target` also on, `EXTRA_EXCLUDE_COMPONENTS` =
     this app's own receiver (3.2), so the app does not offer itself;
   - `startActivity(chooser)`; `ActivityNotFoundException` → `no-app`,
     anything else → `platform: <exception class>`.
   - Never `FLAG_GRANT_WRITE_URI_PERMISSION` or a persistable grant.

`title` is the chooser's title; `None` gives the system's own ("Open
with", "Share").

### 2.4 Cleanup

The app owns the files it writes there, and an app such as flankr keeps
its own cache in this directory, so libmobile deletes little on its own:

- **On the plugin's `onCreate`** (each app start), files in
  `libmobile-share/` not modified for `SHARE_MAX_AGE` (7 days) are
  deleted, on a background thread.
- **`prune(&Prune)`**, called by the app when it likes: deletes files
  older than `max_age`, then, while the directory holds more than
  `max_total_bytes`, the least recently modified file. It never deletes a
  file modified within `SHARE_GRACE` (1 hour), because a receiving app may
  read the stream long after the chooser closed (a mail app reads it on
  Send). Answers how many files and bytes it removed.

The pure part, `prune_dir(dir, &prune, now)`, is std-only and tested on
the host with a temporary directory.

### 2.5 Manifest

```rust
// Viewing and sharing files the app wrote (`shareout/`,
// `src/android/share_out.rs`). `${applicationId}` is filled in by the
// manifest task (2.6).
Plugin {
    feature: Some("share-out"),
    dir: "shareout",
    class: "shareout.ShareOutPlugin",
    contributes: Contributes {
        manifest: &[
            // Package visibility (API 30+): lets `view_file` see whether any
            // app views the type, to answer `NoApp` instead of an empty
            // chooser.
            "<queries><intent><action android:name=\"android.intent.action.VIEW\" /><data android:mimeType=\"*/*\" /></intent></queries>",
        ],
        application: &[
            "<provider android:name=\"dev.highbay.libmobile.shareout.ShareFileProvider\" android:authorities=\"${applicationId}.libmobile.share\" android:exported=\"false\" android:grantUriPermissions=\"true\" />",
        ],
        ..Contributes::NOTHING
    },
},
```

### 2.6 The app id in the authority

The `applicationId` input of `LibmobileManifestTask` that media request
3.1 specifies (the contributions are spliced after Gradle has replaced
placeholders, so `${applicationId}` would otherwise reach the APK as
written). Whichever of the two plans is built first adds it, exactly as
written there; the other uses it. In Kotlin the authority is
`context.packageName + ".libmobile.share"`.

## 3. `share-target`: Share → the app

### 3.1 Why `IntentData` is not extended

Today an intent's extras reach Rust stringified (`IntentPlugin.kt:45-47`), so
`EXTRA_STREAM` arrives as the text `content://…` (or, for
`SEND_MULTIPLE`, `[content://a, content://b]`). The text cannot be read:
reading a shared URI needs the read grant the sender attached, and that
grant belongs to the activity that received the intent and ends when that
activity is destroyed. Adding the URIs to `IntentData` would hand the app
strings it still could not open. So the fix is additive and lives
elsewhere: the activity that receives the share reads the streams itself
while it holds the grant, and the bytes reach the app as a new event,
`MobileEvent::Shared`. `IntentData` and `IntentPlugin` are unchanged, and
the main activity never sees `SEND` intents (it has no filter for them).

### 3.2 The receiver activity

`dev.highbay.libmobile.sharetarget.ShareReceiverActivity`, a plain
`android.app.Activity`, not `NativeActivity`:

- **Why a separate activity.** A share started from another app creates
  the receiving activity in the *sender's* task. A second
  `MobileNativeActivity` there would start a second native app in the
  same process. The receiver instead does its work invisibly and hands
  over to the app's own task.
- **Theme** `@android:style/Theme.Translucent.NoTitleBar` (not
  `NoDisplay`, which must finish before `onResume`: copying may take
  longer), `excludeFromRecents="true"`, the main activity's
  `configChanges` (so a rotation does not recreate it mid-copy).
- **`onCreate`:**
  1. `savedInstanceState != null` (recreated after the share was handled):
     `finish()`, nothing else.
  2. `NativeLib.ensureLoaded(this)` (the process may have been started
     for the share).
  3. Gather: the action; `intent.type`; `EXTRA_TEXT` as text
     (`getCharSequenceExtra(...).toString()`), cut at `max_text_bytes` of
     UTF-8 on a character boundary; `EXTRA_SUBJECT`; the stream URIs from
     `EXTRA_STREAM` (`getParcelableExtra(EXTRA_STREAM, Uri::class.java)`
     on API 33+, the deprecated form before; for `SEND_MULTIPLE`,
     `getParcelableArrayListExtra`) and from `clipData` items, without
     duplicates, in order. A value that is not a `Uri` is skipped.
  4. **Refused URIs** become `Unreadable` items without being opened: any
     scheme but `content` (`"scheme"`; a `file://` URI could name this
     app's own private files), and any authority equal to or starting with
     `packageName + "."` (`"own"`; this app's own providers, so a share
     cannot make the app re-read its own files as if they were sent).
  5. More than `max_items` URIs: the first `max_items` are kept and
     `truncated` is set.
  6. On a single background thread (`libmobile-share-in`), for each URI:
     `contentResolver.getType`; a query of `DISPLAY_NAME` and `SIZE`;
     then `openInputStream` copied in 64 KiB chunks to
     `<cacheDir>/libmobile-received/<seq>-<n>.<ext>` (extension from the
     MIME type, else `bin`). `SIZE` over `max_bytes`, or the stream
     passing it: the copy stops, the partial file is deleted, the item is
     `TooLarge { size }`. An exception: `Unreadable(<exception class>)`,
     never its message. The whole copy has `SHARE_COPY_TIMEOUT` (30 s);
     items not finished by then are `Unreadable("timeout")`.
  7. One JNI call, `nativeShared(seq, action, type, text, textCut,
     subject, truncated, mimes[], names[], sizes[], paths[], itemErrors[])`,
     on that thread. Rust builds `SharedContent` there (3.3), reading
     each file into memory and deleting it for `Delivery::Bytes`, and
     pushes it to a mailbox drained by `pump_bridges`.
  8. Back on the UI thread: `startActivity` of
     `packageManager.getLaunchIntentForPackage(packageName)` with
     `FLAG_ACTIVITY_NEW_TASK`, which brings the app's existing task
     forward (or starts it), then `finish()` with no animation. The share
     reaches the app as the event, not as an intent.
- **`ShareTargetPlugin`** (a `MobileActivityPlugin` object in the same
  directory) deletes files in `libmobile-received/` older than
  `RECEIVED_MAX_AGE` (24 h) in its `onCreate`, on a background thread.

`max_items`, `max_bytes`, `max_text_bytes` and the delivery come from
`ShareOptions` (3.3), which the receiver reads from Rust with one JNI call
(`nativeShareOptions(): LongArray`) before step 3; defaults apply until
the app calls `set_share_options`.

The URIs, display names and text are never logged: only the action,
counts, MIME types and sizes.

### 3.3 Manifest

```rust
// Receiving shares from other apps (`sharetarget/`,
// `src/android/share_target.rs`). The receiver copies the streams while it
// holds the sender's grant, then brings the app's own task forward.
Plugin {
    feature: Some("share-target"),
    dir: "sharetarget",
    class: "sharetarget.ShareTargetPlugin",
    contributes: Contributes {
        application: &[concat!(
            "<activity android:name=\"dev.highbay.libmobile.sharetarget.ShareReceiverActivity\"",
            " android:exported=\"true\" android:excludeFromRecents=\"true\"",
            " android:theme=\"@android:style/Theme.Translucent.NoTitleBar\"",
            " android:configChanges=\"orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|density|uiMode\">",
            "<intent-filter><action android:name=\"android.intent.action.SEND\" />",
            "<category android:name=\"android.intent.category.DEFAULT\" />",
            "<data android:mimeType=\"image/*\" /><data android:mimeType=\"text/plain\" /></intent-filter>",
            "<intent-filter><action android:name=\"android.intent.action.SEND_MULTIPLE\" />",
            "<category android:name=\"android.intent.category.DEFAULT\" />",
            "<data android:mimeType=\"image/*\" /><data android:mimeType=\"text/plain\" /></intent-filter>",
            "</activity>"
        )],
        ..Contributes::NOTHING
    },
},
```

A contribution, not a `gradle::Project` builder option as URL schemes
are, so a hand-written project gets it through the same splice. The
share sheet shows the app's own label and icon. The MIME list is fixed
(open question 2); an app filters what it takes from `SharedContent`.

## 4. `image-codec`: decoding and encoding through Android

### 4.1 Decoding

`decode_image(bytes, max_side)`:

1. Rust wraps `bytes` as a direct `ByteBuffer` (`NewDirectByteBuffer`,
   no copy into the Java heap; the slice outlives the synchronous call).
2. `ImageCodecPlugin.decode(buffer, maxSide, maxPixels): Array<Any?>`
   (`[bitmap, null]` or `[null, "<kind>: <detail>"]`):
   `ImageDecoder.createSource(buffer)`, then `decodeBitmap(source) {
   decoder, info, _ -> ... }` with, in the header listener:
   - the size as displayed (`info.size`, after the EXIF rotation) scaled
     by `fit_within(w, h, max_side)` (the long side at most `max_side`,
     aspect kept, never enlarged; `max_side == 0` keeps the full size);
     the target over `maxPixels` (`IMAGE_MAX_PIXELS`, 64 MP) is refused
     as `too-large: <w>x<h>` before any pixel is decoded;
   - `setTargetSize`, `setAllocator(ALLOCATOR_SOFTWARE)` (no hardware
     bitmap, which cannot be read), `setUnpremultipliedRequired(true)`
     (straight alpha), `setTargetColorSpace(SRGB)`;
   - `setOnPartialImageListener { false }`, so a truncated file fails.
   `ImageDecoder` applies the EXIF orientation of JPEG and HEIF itself,
   reads HEIC/HEIF, PNG, JPEG, WebP, GIF, BMP and the camera's DNG, and
   gives the first frame of an animated GIF or WebP.
3. A bitmap that is not `ARGB_8888` (a 16-bit PNG or an HDR photo decodes
   to `RGBA_F16`) is converted with `copy(ARGB_8888, false)`.
4. Rust reads `width`, `height` and `rowBytes`, allocates `width * height
   * 4` bytes, and calls `ImageCodecPlugin.copyOut(bitmap, buffer)`
   (`copyPixelsToBuffer` into a direct buffer over that `Vec`, then
   `recycle()`); padded rows (`rowBytes > width * 4`) are copied into a
   scratch buffer and packed. `ARGB_8888` is stored as R, G, B, A bytes,
   which is `Rgba`'s layout.

Errors from `DecodeException.error`: `SOURCE_MALFORMED_DATA` →
`Malformed`, `SOURCE_INCOMPLETE` → `Incomplete`; a format
`ImageDecoder` does not know → `Unsupported`; `OutOfMemoryError` →
`OutOfMemory`; anything else → `Platform(<class>)`.

### 4.2 Probing

`probe_image(bytes)`: `BitmapFactory.decodeByteArray` with
`inJustDecodeBounds` (MIME type and stored size, no pixels; `null`
`outMimeType` → `Unsupported`), and `ExifInterface(ByteArrayInputStream)`
for `TAG_ORIENTATION` (JPEG, HEIF, PNG and WebP; 0 when absent). It
copies `bytes` into the Java heap once (`ExifInterface` needs a stream);
it reads headers only and is cheap beside a decode. It tells an app
whether a photo needs converting (flankr's rules) without decoding it.

### 4.3 Encoding

`encode_image(&rgba, encoding)`:

1. Rust checks `pixels.len() == width * height * 4`, both sides 1..=32768,
   at most `IMAGE_MAX_PIXELS`, and `quality <= 100`
   (`ImageError::BadInput(<what>)`).
2. `ImageCodecPlugin.encode(width, height, buffer, format, quality):
   Array<Any?>` (`[byteArray, null]` or `[null, error]`):
   `Bitmap.createBitmap(width, height, ARGB_8888, true, SRGB)`,
   `setPremultiplied(false)`, `copyPixelsFromBuffer` from a direct buffer
   over the slice (as is, no premultiplying), `compress(JPEG | PNG,
   quality, ByteArrayOutputStream)`, `recycle()`.
3. Rust copies the `byte[]` out. `Png` ignores `quality`. `Jpeg` has no
   alpha: Android's encoder ignores it, so a transparent pixel's colour
   shows; an app with transparency encodes PNG (`Rgba::is_opaque` tells
   it) or flattens first. The output carries no EXIF.

### 4.4 Threads and memory

All three calls block (a 12 MP HEIC decode takes some hundreds of
milliseconds on an S23) and may run on any thread **except** the main
loop's: an app runs them through `Workers` or its own threads. They need
no `AndroidApp` (they reach the plugin through `ndk-context`, as
`cache_dir` does) and may run concurrently. Peak memory for a decode is
about twice the output (the bitmap's pixels and the `Rgba`); for an
encode, the bitmap plus the output.

### 4.5 Manifest

```rust
// Image decoding and encoding through ImageDecoder and Bitmap.compress
// (`image/`, `src/android/image_codec.rs`). Nothing in the manifest.
Plugin {
    feature: Some("image-codec"),
    dir: "image",
    class: "image.ImageCodecPlugin",
    contributes: Contributes::NOTHING,
},
```

## 5. Rust API

Platform-free types in `src/share_out.rs`, `src/share_target.rs` and
`src/image_codec.rs` (each under its feature, built and tested on every
host); the Android calls in `src/android/share_out.rs`,
`share_target.rs` and `image_codec.rs`, re-exported as
`libmobile::android::{share_out, share_target, image_codec}`.

```rust
// src/share_out.rs (share-out)
/// The directory's name under `cache_dir()`.
pub const SHARE_DIR: &str = "libmobile-share";

/// A file to view or share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareFile<'a> {
    /// A file directly in `share_dir()`, named by the rule of 2.1.
    pub path: &'a std::path::Path,
    /// Its MIME type (`text/markdown`, `image/png`); no wildcard.
    pub mime: &'a str,
    /// What the other app shows and saves it as; the file's name if `None`.
    pub display_name: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShareError {
    /// The file is not directly in the share directory.
    NotInShareDir,
    /// Its name breaks the rule (`^[A-Za-z0-9._-]{1,96}$`).
    BadName,
    BadDisplayName,
    BadMime,
    NotFound,
    /// No app on the device views this type.
    NoApp,
    /// The app is not visible, and Android does not let it start one.
    NotVisible,
    /// Anything else, for logs.
    Platform(String),
}
// Display, std::error::Error, from_message("<kind>: <detail>").

/// The checks of 2.1 and 2.3 step 1, given the share directory: the
/// stored name to send. Pure (it canonicalises paths), host-tested.
pub fn check_share(dir: &std::path::Path, file: &ShareFile<'_>) -> Result<String, ShareError>;
pub fn valid_name(name: &str) -> bool;
pub fn valid_display_name(name: &str) -> bool;
pub fn valid_mime(mime: &str) -> bool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prune {
    /// Delete files not modified for this long.
    pub max_age: Option<std::time::Duration>,
    /// Then delete the least recently modified while the total is over this.
    pub max_total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned { pub files: u32, pub bytes: u64 }

/// 2.4 on any directory; never deletes a file modified within
/// `SHARE_GRACE` of `now`. Host-tested.
pub fn prune_dir(dir: &std::path::Path, prune: &Prune, now: std::time::SystemTime)
    -> std::io::Result<Pruned>;
```

```rust
// src/android/share_out.rs
/// `cache_dir()/libmobile-share/`, created if missing. Any thread.
pub fn share_dir() -> Option<PathBuf>;
/// "Open in…": `ACTION_VIEW` in the chooser (2.3). Any thread; needs the
/// app visible.
pub fn view_file(app: &AndroidApp, file: &ShareFile<'_>, title: Option<&str>) -> Result<(), ShareError>;
/// "Share": `ACTION_SEND` with the file as `EXTRA_STREAM`, in the chooser.
pub fn share_file(app: &AndroidApp, file: &ShareFile<'_>, title: Option<&str>) -> Result<(), ShareError>;
/// `prune_dir` on `share_dir()`. Any thread.
pub fn prune(prune: &Prune) -> std::io::Result<Pruned>;
```

```rust
// src/share_target.rs (share-target)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delivery {
    /// The bytes in memory; the received file is deleted.
    #[default]
    Bytes,
    /// A file under `cache_dir()/libmobile-received/`, the app's to move or
    /// delete; files left there are deleted after `RECEIVED_MAX_AGE`.
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareOptions {
    pub max_items: u16,       // 16, clamped to 1..=SHARE_MAX_ITEMS
    pub max_bytes: u64,       // per item, 32 MiB
    pub max_text_bytes: u32,  // 1 MiB
    pub delivery: Delivery,   // Bytes
}
impl Default for ShareOptions { /* as commented */ }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShareAction { Send, SendMultiple }

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SharedData {
    Bytes(Vec<u8>),
    File(std::path::PathBuf),
    /// Over `max_bytes`; `size` when the sender reported it.
    TooLarge { size: Option<u64> },
    /// Not read: `"scheme"`, `"own"`, `"timeout"`, or the exception's class.
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SharedItem {
    pub mime: Option<String>,
    pub display_name: Option<String>,
    pub size: Option<u64>,
    pub data: SharedData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SharedContent {
    pub action: ShareAction,
    /// The intent's type (`image/*`, `text/plain`, `image/jpeg`).
    pub mime: Option<String>,
    /// `EXTRA_TEXT`, cut at `max_text_bytes`.
    pub text: Option<String>,
    pub text_truncated: bool,
    pub subject: Option<String>,
    pub items: Vec<SharedItem>,
    /// More streams than `max_items`; the rest were dropped.
    pub truncated: bool,
}

/// Build from the receiver's flattened parts (3.2 step 7). Host-tested.
pub(crate) fn from_parts(/* … */) -> SharedContent;
```

```rust
// src/android/share_target.rs
/// The limits and delivery for shares from now on. Any thread, any time;
/// the defaults apply until it is called.
pub fn set_share_options(options: &ShareOptions);
```

```rust
// src/event.rs
/// Another app shared text or files with this one (`share-target`).
#[cfg(feature = "share-target")]
Shared(crate::share_target::SharedContent),
```

A share with neither text nor any stream is still delivered (an empty
`SharedContent`), so the app can say nothing came.

```rust
// src/image_codec.rs (image-codec)
/// Pixels: rows top to bottom, 4 bytes R, G, B, A each, sRGB, straight
/// (not premultiplied) alpha; `pixels.len() == width * height * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba { pub width: u32, pub height: u32, pub pixels: Vec<u8> }
impl Rgba {
    /// Checks the length; `BadInput` otherwise.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError>;
    /// Every alpha is 255.
    pub fn is_opaque(&self) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    /// `image/jpeg`, `image/heif`, `image/png`, …
    pub mime: String,
    /// As stored, before the EXIF rotation.
    pub width: u32,
    pub height: u32,
    /// 1..=8, 0 when absent.
    pub exif_orientation: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Encoding { Jpeg { quality: u8 }, Png }

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageError {
    /// Not an image format Android decodes.
    Unsupported,
    Malformed,
    /// The data ends early.
    Incomplete,
    /// The decoded size (after `max_side`) would be over `IMAGE_MAX_PIXELS`.
    TooLarge { width: u32, height: u32 },
    /// Bad arguments: what was wrong.
    BadInput(&'static str),
    OutOfMemory,
    /// Anything else, for logs.
    Platform(String),
}
// Display, std::error::Error, from_message.

/// The size `decode_image` produces for a `width` × `height` image (as
/// displayed): the long side at most `max_side`, aspect kept, at least 1,
/// never enlarged; 0 keeps the size. Host-tested.
pub fn fit_within(width: u32, height: u32, max_side: u32) -> (u32, u32);
```

```rust
// src/android/image_codec.rs — any thread but the main loop's.
pub fn probe_image(bytes: &[u8]) -> Result<ImageInfo, ImageError>;
pub fn decode_image(bytes: &[u8], max_side: u32) -> Result<Rgba, ImageError>;
pub fn encode_image(image: &Rgba, encoding: Encoding) -> Result<Vec<u8>, ImageError>;
```

## 6. What uses it

### 6.1 libmorph (a later plan, not written now)

A libmorph plan of the same name, written after this one is approved,
surfaces the three features to every libmorph app: the features
forwarded by libmorph and libmorph-build; `MorphContext::view_file`,
`share_file`, `share_dir` and `prune_shared`; the `Shared` event given to
the app, held until the app object exists when a share starts the
process (as libmorph's media request plan holds a media answer);
`probe_image`, `decode_image` and `encode_image` with their types
re-exported. What each does on the desktop (libdevice's open, the `png`
crate, or `Unsupported`) is that plan's decision. It takes libmobile's
release additively.

### 6.2 flankr

- **`share-out`:** the preview's share icon (binary files plan 10.2) opens
  Open in… (`view_file`) and Share (`share_file`) for the file already in
  `share_dir()` as `fl-<hash16>.<ext>`, with its own name as the display
  name (`README.md`; a `git` output `<request>.diff` as `text/x-diff`,
  10.5). After keeping each received file, flankr calls
  `prune(&Prune { max_total_bytes: Some(64 MiB), max_age: None })`, its
  64 MiB cache of 6.4, ordered by modification time (flankr touches a
  file when it views it; libmobile touches it when it is shared). An
  attached image's chip writes its bytes there first, then shares.
- **`share-target`:** Share → flankr (9.1): `MobileEvent::Shared` opens
  the Send to… sheet with the image items; HEIC/HEIF are converted (the
  21:29 EDT amendment, item 7); more than 4 keep the first 4 with its
  note; other types show its toast. flankr sets `max_items: 4`,
  `max_bytes: 20 MiB`. Shared text is not used by the binary files plan;
  a later flankr amendment may place it in the message card.
- **`image-codec`:** thumbnails (`decode_image(bytes, 192)`, about 64 dp
  at the S23's density), the preview's images on Android
  (`decode_image(bytes, 4096)`, until now "needs a newer app"), and
  conversion before upload (`probe_image` for HEIC/HEIF, orientation and
  size; `decode_image(bytes, 2048)`; `encode_image(&rgba, Jpeg { quality:
  90 })`, or `Png` unless `is_opaque()`; "Send unmodified" converts
  HEIC/HEIF to PNG at full size, `max_side` 0).

## 7. Versioning

Additive: three new features, new modules and types, a variant of the
`#[non_exhaustive]` `MobileEvent`, and manifest changes only for apps that
turn `share-out` or `share-target` on. `IntentData` is unchanged. The
manifest task's `applicationId` input (2.6), if this plan adds it,
changes the generated `libmobile.gradle` text but not the resulting
manifest of any app without these features. A patch release under Rule
5.4, taking the next free patch number by release order when it is
committed: **0.3.3** as of writing (the clipboard plan took 0.3.2; if
media request is released first, it takes 0.3.3 and this 0.3.4). The
`VERSIONS.md` entry names the number it took. libmorph and libmorph-build
take it without a minor bump of their own.

## 8. Steps

Each step one commit to libmobile, with its tests, passing `cargo fmt
--all -- --check`; host `cargo clippy --workspace --all-targets` with no
features and `--all-features`; `cargo clippy --target
aarch64-linux-android --workspace --all-targets` with no features, with
the step's feature alone and `--all-features`: no warnings; `cargo test
--workspace` and `cargo test -p libmobile --all-features`. Checks on the
owner's S23 are made only after asking, recorded in `CHANGES.md`, with
evidence under `out/share/<YYYYMMDD_NN_name>/`.

1. **`image-codec`** (4, 5): the feature, `src/image_codec.rs`,
   `src/android/image_codec.rs`, `image/ImageCodecPlugin.kt`, the registry
   entry, and an `examples/wgpu` command `--es cmd image-codec` that
   probes and decodes every file in `cache_dir()/codec-test/` (put there
   with `adb push` and `run-as … cp`), logs each `ImageInfo` and decoded
   size, and writes each back as JPEG 90 and PNG beside it. Host tests:
   `fit_within` (landscape, portrait, square, 0, smaller than `max_side`,
   a 1 × 10000 strip staying at least 1 px), `Rgba::new` and `is_opaque`,
   `encode_image`'s argument checks (as a pure function),
   `ImageError::from_message` for every kind; the registry entry adds
   nothing to the manifest.
   **Done when:** the checks pass, `examples/build.sh wgpu` builds and
   `dexdump` shows `image/ImageCodecPlugin`'s static methods with the
   signatures Rust calls; on the S23, a camera HEIC decodes upright with
   `exif_orientation` 6 reported by `probe_image`, a JPEG with orientation
   6 decodes upright at the swapped size, a transparent PNG comes back
   with `is_opaque() == false` and round-trips through `Png` unchanged, a
   truncated JPEG is `Incomplete`, a text file is `Unsupported`, and a
   12 MP decode at `max_side` 2048 is timed in the log.
2. **The `share-out` provider** (2.1, 2.2, 2.5, 2.6): the feature, the
   registry entry, `ShareFileProvider.kt`, an empty `ShareOutPlugin.kt`,
   the `applicationId` input (unless media request's step 1 has landed),
   `valid_name`, `valid_display_name`, `valid_mime`. Host tests: the
   provider and the `<queries>` entry appear only with `share-out`; no
   `<uses-permission>`; the generated script replaces `${applicationId}`;
   the name rules accept `fl-1a2b3c4d5e6f7a8b.md` and `README.md` and
   refuse `../x`, `a/b`, `.`, `..`, empty, 97 characters, a control
   character; the Kotlin source holds the same pattern string.
   **Done when:** the checks pass, a generated project with `share-out`
   builds with `gradle assembleDebug`, its merged manifest holds the
   provider with the real application id, and the manifest of an app
   without it is byte-identical to today's.
3. **`view_file`, `share_file`, `prune`** (2.3, 2.4): the calls, the
   plugin's intents and `onCreate` cleanup, `check_share`, `prune_dir`,
   `ShareError::from_message`; `examples/wgpu` commands `--es cmd
   share-view` and `share-send`, which write `hello.md` into `share_dir()`
   and call the two with display name `Hello.md`. Host tests:
   `check_share` (a file directly in the directory, one in a
   subdirectory, a symlink out, a missing file, a bad MIME type);
   `prune_dir` with files of set modification times (age, total, the
   grace period kept, an empty directory).
   **Done when:** the checks pass; on the S23, `share-view` opens a
   chooser with the apps that view Markdown and one of them shows the
   text, `share-send` opens the share sheet, Gmail attaches `Hello.md`
   and sends it after a minute's wait, Drive saves it as `Hello.md`;
   `adb shell content read --uri
   content://<pkg>.libmobile.share/f/hello.md/Hello.md` from the shell is
   refused (no grant); `view_file` while the app is in the background is
   `NotVisible`.
4. **`share-target`** (3): the feature, the registry entry,
   `ShareReceiverActivity.kt`, `ShareTargetPlugin.kt`,
   `src/share_target.rs`, `src/android/share_target.rs`,
   `nativeShareOptions`, `nativeShared`, the mailbox, `pump_bridges`,
   `MobileEvent::Shared`, and `EXTRA_EXCLUDE_COMPONENTS` in `share_file`
   when both features are on; `examples/wgpu` logs each `Shared` event
   (action, counts, MIME types, sizes; never the text or names). Host
   tests: `from_parts` for every item error, `truncated`,
   `text_truncated`, `File` and `Bytes`; the activity element with both
   filters appears only with `share-target`; the Kotlin source refuses
   `file` and the app's own authorities (the strings checked in a test).
   **Done when:** the checks pass; on the S23, sharing one photo from
   Gallery to the example brings it forward with one `image/jpeg` item of
   the photo's full size; three photos give `SendMultiple` with three
   items; a HEIC from Gallery arrives as `image/heif` (or as Gallery
   converts it, recorded); a link shared from Chrome gives `text` and no
   items; a photo over a 1 MiB `max_bytes` is `TooLarge`; with the
   example not running, a share starts it and the event arrives; Recents
   shows no extra entry; sharing from the example's own `share-send` does
   not list the example.
5. **README and release** (its own commit): the features table rows, the
   layout lines for `shareout/`, `sharetarget/`, `image/`, a usage
   section for each feature, `Cargo.toml`'s version (the next free patch,
   section 7), the `VERSIONS.md` entry, this plan's decision-table row
   updated, the tag.
   **Done when:** the commit is tagged with that version and libmorph
   builds against it with the three features on.

## 9. Tunable constants (living section)

| Name | Value | Meaning |
|---|---|---|
| `SHARE_MAX_AGE` | 7 days | `libmobile-share/` files older than this are deleted at app start |
| `SHARE_GRACE` | 1 h | `prune` never deletes a file modified more recently |
| `SHARE_MAX_ITEMS` | 100 | ceiling on `ShareOptions::max_items` |
| `SHARE_COPY_TIMEOUT` | 30 s | the receiver's limit for copying every stream |
| `SHARE_COPY_CHUNK` | 64 KiB | stream copy buffer |
| `RECEIVED_MAX_AGE` | 24 h | `libmobile-received/` files older than this are deleted at app start |
| `IMAGE_MAX_PIXELS` | 64 M | largest decode or encode, in pixels (256 MiB of `Rgba`) |

## 10. Risks

- **Grants that end early.** A temporary grant belongs to the activity
  that received it. Copying in the receiver before it finishes is the
  reliable way; forwarding the URIs to the main activity with the grant
  flag is not relied on, since a grant to the app that already holds it
  may record no new owner and end with the receiver. A sender that revokes its
  grant at once (rare) gives `Unreadable(SecurityException)`.
- **The receiver over the sender.** While copying, the translucent
  receiver takes touches over the sending app: milliseconds for local
  photos, up to `SHARE_COPY_TIMEOUT` for a cloud item. A visible
  progress line would need a layout; not planned (open question 3).
- **`ImageDecoder` on images from the wild.** Malformed files are the
  platform decoder's to reject; it is the same decoder Android's own
  apps use and gets security updates with the system. Decoding runs in
  the app's process, as any decoder would.
- **Colour.** Wide-gamut and HDR photos are converted to sRGB; Ultra HDR
  JPEGs decode as their SDR base image. Fine for previews and for
  agents.
- **JPEG and alpha.** `encode_image` with `Jpeg` ignores alpha (4.3); an
  app that forgets `is_opaque` gets the hidden colours of transparent
  pixels.
- **Receivers that ignore `DISPLAY_NAME`.** The display name is also the
  URI's last segment, which covers most; an app that names files from
  something else shows its own name.
- **`<queries>` for `VIEW */*`.** It lets the app see which apps view any
  type. Play does not restrict an intent `<queries>` entry as it does
  `QUERY_ALL_PACKAGES`; if it ever did, `view_file` would drop the check
  and let the chooser show "No apps".

## 11. Open questions for the owner

1. **One item type with media request.** flankr's 10.6 asks for
   `MobileEvent::Shared` with media request's item type. That plan is a
   Draft, its `MediaSource` is closed and has no "shared" source, and
   changing it needs its own amendment. As written, `SharedItem` and
   `SharedData` are this plan's own, shaped like `MediaItem` and
   `MediaData` (minus `source`, `uri` and `exif_orientation`). Keep them
   separate, or amend media request so both use one type (a
   `MediaSource::Shared` and one `Delivery`)?
2. **The MIME list.** Fixed to `image/*` and `text/plain` for every app
   with `share-target`, so hand-written projects get it too; or chosen by
   the app with a `gradle::Project::share_types(&[...])` option, which
   would reach generated projects only (as URL schemes do)?
3. **The pause while copying.** Is an invisible receiver over the sender
   for up to 30 s acceptable, or should it bring the app forward first
   and copy with the app showing its own progress (which needs the grant
   held differently, for example by the receiver staying alive behind)?
4. **Releases.** One release with all three features after step 4, or
   `image-codec` released on its own after step 1 (it unblocks flankr's
   Android image preview and thumbnails soonest), each taking the next
   patch number?
5. **Cleanup at start.** Delete `libmobile-share/` files older than 7
   days at every start (2.4), or never delete there on libmobile's own,
   leaving it all to `prune`?
6. **`share_files`.** Sharing several files out at once
   (`ACTION_SEND_MULTIPLE`) is not planned; flankr shares one. Add it now
   or when needed?

## Amendment — 2026-10-29 00:07 EDT: approved; image-codec built now, sharing later

The owner (2026-10-29): "approve and build the share and view plan, just
clipboard on Mac leave out sharing for now", then, of the lead's readings,
"Build image-codec only; no sharing yet". The plan is approved. Only the
`image-codec` feature is built now (its step, with the README and the
release step for it): `decode_image`, `encode_image` and `probe_image`.
`share-out` and `share-target` are not built until the owner asks, so
open questions 1, 2, 3, 5 and 6 (all about sharing) stay open; question 4
is answered: `image-codec` ships on its own first. Its version is the next
free patch (0.3.3). The status stays `Draft` until the first implementing
commit (Rule 1.5).
