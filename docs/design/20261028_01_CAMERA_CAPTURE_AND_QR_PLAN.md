---
title: Single-frame camera capture and QR scanning with auto zoom
date: 2026-10-28
status: In implementation
covers:
  - "Feature `camera-device`: an in-process camera on the NDK Camera2 API
    (`ACamera*`, `AImageReader`), opened only when the app uses it, with
    the CAMERA permission asked for at that moment and never at launch"
  - "Feature `camera-capture`: one still frame (`Gray8`, `Rgba8` or
    `Jpeg`), at a chosen resolution, upright, with a timeout and cancel,
    with or without a running preview"
  - "Feature `camera-qr`: QR decoding on device with rqrr, the decoded
    payload as an event, auto zoom toward a code that is too small, and
    exposure that steps down for codes shown on screens"
  - "Zoom the app drives (`set_zoom`, `zoom_by`, `zoom_range`) for a
    pinch made by the app, and how auto zoom fine-tunes around it"
  - "`MobileEvent::Camera`, lifecycle (pause, resume, eviction), threading"
  - "`android::open_app_settings`, for a permission the user refused for
    good"
  - "The manifest: `CAMERA` and optional `uses-feature`s only under these
    features; how the existing `camera` (system camera app) feature lives
    alongside them"
sources:
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"reaserch
    adding single frame camera capture to l8bmobile/libmorph in subagent
    as plan in that repo, use worktree in those repos, add qr capture mode
    with auto zoom, uee features for each subpart (such as qr capture) and
    related perm8ssions, only request camera access if caller uses
    camera\"."
  - "Owner (2026-10-28, through the lead, verbatim): \"it should also
    adjust exposure when reading screens and allow pinch zoom for gross
    adjustment (remote tv etc.)\". The lead added libcontrols' direction:
    the component responds to calls; it does not implement gestures."
  - "libmobile today: `Cargo.toml:33` (`camera = []`);
    `src/camera.rs:15` (`CaptureOutcome::{Jpeg, Failed}`);
    `src/android/camera.rs:26` (`capture_photo`, a JNI call to
    `CameraPlugin.capture`); `camera/CameraPlugin.kt:32-36`
    (`startActivityForResult(ACTION_IMAGE_CAPTURE)`, the result's
    `data` thumbnail bitmap re-encoded as JPEG 90); `CameraPlugin.kt:20-21`
    (if the manifest declares CAMERA, Android requires it granted before
    `ACTION_IMAGE_CAPTURE` launches); `src/codegen.rs:159-174` (the
    `camera` plugin contributes an `IMAGE_CAPTURE` `<queries>` entry and
    `android.hardware.camera.any` `required=false`, no permission;
    asserted at `src/codegen.rs:1338-1356`); `src/event.rs:164-165`
    (`MobileEvent::Capture`, the enum `#[non_exhaustive]` at 129);
    `src/android/mod.rs:191-194` and `230-232` (the mailbox drain and
    the stale-command discard)."
  - "libmobile's runtime permissions: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md`
    and its amendments (request ids, one request at a time,
    `should_show_rationale`); `src/android/permission.rs:36`
    (`request_permissions`), `bridge/PermissionPlugin.kt:35,44`."
  - "libmorph `docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md`
    (status Hold): a live `camera-view` on the NDK camera with the CAMERA
    permission, a rename of `camera` to `camera-capture`, and the owner's
    confirmed decision (its amendment of 2026-10-14 12:14 EDT, item 2)
    that Android's in-process camera is the NDK camera, not CameraX."
  - "ndk-sys 0.6.0: `ACameraManager_create`, `ACaptureRequest_setEntry_float`,
    `ACAMERA_CONTROL_ZOOM_RATIO`, `ACAMERA_CONTROL_ZOOM_RATIO_RANGE`,
    `ACAMERA_SCALER_CROP_REGION`, `ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM`,
    `ACAMERA_SENSOR_INFO_ACTIVE_ARRAY_SIZE`, `ACAMERA_CONTROL_AF_REGIONS`
    are declared, but libcamera2ndk is not linked (ndk-sys links only
    android, nativewindow, mediandk, jnigraphics, aaudio, sync). ndk 0.9
    `media::image_reader` (feature `media`): `ImageReader::new`,
    `set_image_listener`, `acquire_latest_image`, `Image::plane_data`,
    `plane_row_stride`, `plane_pixel_stride`, `timestamp`; formats
    `YUV_420_888` and `JPEG`."
  - "rqrr 0.11.0: licence `(MIT OR Apache-2.0) AND ISC`; one dependency
    (`g2p`) with `default-features = false`;
    `PreparedImage::prepare_from_greyscale(w, h, fill)`, `detect_grids()`,
    `Grid { grid, bounds: [Point; 4] }` (corners in image coordinates),
    `Grid::decode() -> (MetaData, String)`, `decode_to(writer)` for raw
    bytes, `BitGrid::size()` (modules per side)."
  - "Android: generated projects set `minSdk = 29` (`src/codegen.rs:519`);
    `CONTROL_ZOOM_RATIO` is API 30; declaring CAMERA implies
    `uses-feature android.hardware.camera` and `camera.autofocus` as
    required unless declared `required=false`."
related:
  - libmorph docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (wraps
    this plan)
  - libmorph docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold;
    overlaps, see section 12)
  - docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md (the permission
    path this uses)
living sections:
  - "11. Tunable constants"
---

# Single-frame camera capture and QR scanning with auto zoom

## 1. What exists, and what is missing

libmobile's only camera today is the `camera` feature: `capture_photo`
starts the **system camera app** with `MediaStore.ACTION_IMAGE_CAPTURE`
(`CameraPlugin.kt:32-36`). What comes back is the intent's `data`
extra, a thumbnail bitmap (typically a few hundred pixels wide),
re-encoded as JPEG and delivered as `MobileEvent::Capture(Jpeg | Failed)`
(`src/camera.rs:15`). It needs no permission and declares none.

Missing:

- No in-process camera: no Camera2 or CameraX, no frames, no preview, no
  zoom or exposure control. Nothing links `libcamera2ndk`.
- No full-resolution still, no format choice, no orientation, no timeout.
- No CAMERA permission anywhere: not in the manifest, not requested.
- No QR or barcode decoding.
- libmorph has only the `mobile-camera` passthrough (`Cargo.toml:28` there)
  and no camera API; its `src/runner/overlay.wgsl` draws display cutouts
  ("camera cutouts" of the emulated glass), not a camera preview. No
  desktop or web camera exists.

## 2. Features

```toml
[features]
# Unchanged: photo through the system camera app (no permission of its own).
camera = []
# New base: the in-process camera on the NDK Camera2 API. Adds the CAMERA
# permission and optional uses-features to the manifest, the
# `cameradevice/CameraDevicePlugin` Kotlin plugin, ndk's AImageReader and
# libcamera2ndk. Sessions, zoom, exposure, preview frames, events.
camera-device = ["ndk/media"]
# One still frame (`android::camera_device::capture_frame`).
camera-capture = ["camera-device"]
# QR scanning with auto zoom and screen exposure (adds rqrr).
camera-qr = ["camera-device", "dep:rqrr"]
```

```toml
[dependencies]
# QR detection and decoding (feature `camera-qr`). Without `img`, so no
# `image` dependency; it reads our grey plane directly.
rqrr = { version = "0.11", default-features = false, optional = true }
```

`build.rs` emits `cargo:rustc-link-lib=camera2ndk` when the target OS is
Android and `CARGO_FEATURE_CAMERA_DEVICE` is set.

Each feature carries only what it needs: an app with `camera-capture`
alone does not compile or link rqrr; an app with only `camera` keeps the
permission-free hand-off and an unchanged manifest.

**Naming.** The owner's sketch named the base `camera`. That name is taken
by the system-camera-app hand-off, and giving it the CAMERA permission
would change what an existing `camera` app does (with CAMERA in the
manifest, `ACTION_IMAGE_CAPTURE` fails until it is granted): a breaking
change. This plan therefore names the base `camera-device` and leaves
`camera` alone. Section 13, question 1, offers the rename instead.

## 3. Manifest and permissions

### 3.1 What each feature declares

A new plugin in `PLUGINS` (`src/codegen.rs`), after `camera.CameraPlugin`:

```rust
Plugin {
    feature: Some("camera-device"),
    dir: "cameradevice",
    class: "cameradevice.CameraDevicePlugin",
    contributes: Contributes {
        permissions: &["android.permission.CAMERA"],
        manifest: &[
            // CAMERA implies these two as required; declared optional so
            // Play does not filter out devices without a back camera or
            // autofocus. The camera is opened only when the app asks.
            "<uses-feature android:name=\"android.hardware.camera\" android:required=\"false\" />",
            "<uses-feature android:name=\"android.hardware.camera.autofocus\" android:required=\"false\" />",
            "<uses-feature android:name=\"android.hardware.camera.any\" android:required=\"false\" />",
        ],
        ..Contributes::NOTHING
    },
},
```

The `camera.any` line is the same string the `camera` plugin contributes,
so `merge` (which deduplicates exact strings) emits it once when both are
on. `camera-capture` and `camera-qr` add no plugin of their own: they are
Rust-only on top of `camera-device`.

`CameraDevicePlugin.kt` is small: `displayRotation(activity): Int`
(`activity.display.rotation`, for upright frames) and nothing else. The
permission goes through the core `bridge.PermissionPlugin`.

Without `camera-device`, the manifest has no CAMERA permission, exactly as
today (a codegen test asserts this for `[]` and `["camera"]`).

### 3.2 Asked for only when the app uses the camera

libmobile never asks for CAMERA at start, at `init`, or when the feature
is merely compiled in. The first call that needs the camera device
(`capture_frame`, `start_qr`, `start_preview`) runs a **permission gate**
in `src/android/camera_device/gate.rs`:

1. `permission_granted(app, CAMERA)` is true: open the camera.
2. Otherwise, if no camera request is in flight:
   `request_permissions(app, &[CAMERA])`, remembering its
   `PermissionRequest`. The call is queued (its session or capture is
   `Waiting`), and so is every later camera call until the answer.
3. In `pump_bridges`, each `PermissionsOutcome` is offered to the gate
   first. If its `request` is the gate's, the gate consumes it (it is
   **not** also delivered as `MobileEvent::Permissions`) and:
   - granted: every waiting call proceeds;
   - denied: every waiting call ends with
     `CameraError::PermissionDenied { settings_only }`, where
     `settings_only = !should_show_rationale(app, CAMERA)`. After a
     denial, `false` means Android would refuse without a dialog (the user
     chose "Don't allow" twice), so the app should send the user to
     Settings (3.3).
4. A later call after a denial asks again (step 2). If Android refuses
   without a dialog, the answer comes back at once, denied, with
   `settings_only: true`; libmobile does not keep its own
   "never ask again" flag, because the user may grant it in Settings at
   any time and step 1 then succeeds.

A capture's timeout (4.2) starts when the camera opens, not while the
permission dialog is up. `cancel_capture` and `stop` work on waiting calls.

### 3.3 `android::open_app_settings`

Core (no feature), in `bridge/LinkPlugin.kt` beside `open_link`:

```rust
/// Open this app's page in Settings (`ACTION_APPLICATION_DETAILS_SETTINGS`
/// with `package:<own package>`), where the user can grant a permission
/// they refused for good. Needs the app to be visible.
pub fn open_app_settings(app: &AndroidApp) -> Result<(), OpenLinkError>;
```

`OpenLinkError` is the existing error of `open_link` (`src/link.rs:13`):
`NotVisible` when the app is not in front, `Platform` for anything else.

### 3.4 `camera` and `camera-device` together

With both features on, CAMERA is in the manifest, so the system camera
app hand-off needs it granted. `capture_photo` then goes through the same
gate first; a denial ends it with `CaptureOutcome::Failed("permission_denied")`.
With `camera` alone nothing changes.

## 4. Rust API

Platform-free types live in `src/camera_device.rs` (built on every host
under `camera-device`, host-tested, like `event.rs`). The Android calls
live in `src/android/camera_device/` and are re-exported as
`libmobile::android::camera_device`.

### 4.1 Common types

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facing { #[default] Back, Front }

/// A running camera session (QR scan or preview). Opaque; ids count from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub(crate) u32);

/// One `capture_frame` call. Opaque; ids count from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureId(pub(crate) u32);

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraError {
    /// CAMERA refused. `settings_only`: asking again shows no dialog.
    PermissionDenied { settings_only: bool },
    /// No camera with that facing (or the NDK lists none: LEGACY devices).
    NoCamera,
    /// Another app holds the camera (`ERROR_CAMERA_IN_USE`,
    /// `ERROR_MAX_CAMERAS_IN_USE`) or took it from us (`onDisconnected`).
    InUse,
    /// Disabled by device policy (`ERROR_CAMERA_DISABLED`).
    Disabled,
    /// The capture's timeout passed.
    Timeout,
    /// `cancel_capture` or `stop`.
    Cancelled,
    /// The activity paused before the capture finished.
    Interrupted,
    /// The device cannot do what was asked (format, size).
    Unsupported(&'static str),
    /// Anything else, for logs.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraEvent {
    /// A session's camera is open and streaming (again, after a resume).
    Opened { session: SessionId, facing: Facing, size: (u32, u32),
             zoom_range: (f32, f32) },
    /// The session's camera closed: `reason` is `None` for `stop`.
    Closed { session: SessionId, reason: Option<CameraError> },
    /// A paused session will reopen on resume (no event until `Opened`).
    Suspended { session: SessionId },
    /// The zoom changed, by the app or by auto zoom.
    Zoom { session: SessionId, ratio: f32, source: ZoomSource },
    /// A capture finished.
    #[cfg(feature = "camera-capture")]
    Frame { capture: CaptureId, result: Result<Frame, CameraError> },
    /// A QR code was read.
    #[cfg(feature = "camera-qr")]
    Qr { session: SessionId, code: QrCode },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomSource { App, Auto }
```

In `src/event.rs`:

```rust
#[cfg(feature = "camera-device")]
Camera(crate::camera_device::CameraEvent),
```

`MobileEvent` is `#[non_exhaustive]`, so the variant is additive.

### 4.2 Single frame (`camera-capture`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameFormat {
    /// The luma plane, one byte per pixel, rows packed.
    Gray8,
    /// RGBA, 4 bytes per pixel, rows packed, converted from YUV (5.3).
    Rgba8,
    /// The camera's hardware JPEG.
    Jpeg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolution {
    /// The largest supported size within 1280 × 720 (fast, small).
    Preview,
    /// The largest size the device offers for the format.
    Max,
    /// The largest supported size within this box (either orientation).
    AtMost { width: u32, height: u32 },
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CaptureOptions {
    pub facing: Facing,             // Back
    pub format: FrameFormat,        // Rgba8
    pub resolution: Resolution,     // Preview
    /// Rotate to the display's orientation (Gray8 and Rgba8 rotated on the
    /// camera thread; Jpeg through `JPEG_ORIENTATION`, see 10).
    pub upright: bool,              // true
    /// From the camera opening to the frame. Default 5 s.
    pub timeout: Duration,
    /// 1..=100, Jpeg only. Default 90.
    pub jpeg_quality: u8,
}
impl Default for CaptureOptions { /* as commented */ }

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub data: Vec<u8>,
    /// Clockwise quarter turns still needed to show it upright: 0 when
    /// `upright` was honoured; the sensor's value when it was not.
    pub quarter_turns: u8,
    pub facing: Facing,
    /// `AImage` timestamp (sensor clock, ns).
    pub timestamp_ns: i64,
}

/// Take one still. The answer is `CameraEvent::Frame` with this id.
/// Asks for CAMERA first if needed (3.2). Any thread that is not a JNI
/// callback; the work happens on the camera thread.
pub fn capture_frame(app: &AndroidApp, options: &CaptureOptions) -> CaptureId;

/// End a capture early with `CameraError::Cancelled` (no-op once answered).
pub fn cancel_capture(capture: CaptureId);
```

Front-camera frames are not mirrored: they are the scene as the sensor
saw it. Mirroring a selfie preview is the app's display choice.

### 4.3 Sessions, preview and zoom (`camera-device`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreviewOptions {
    /// Longest side of the frames `latest_preview` returns. Default 640.
    pub max_side: u32,
    pub upright: bool,              // true
}

/// Open the camera for a live session without QR (a viewfinder only).
pub fn start_preview(app: &AndroidApp, facing: Facing, preview: PreviewOptions) -> SessionId;

/// Stop a session (preview or QR). `Closed { reason: None }` follows.
pub fn stop(session: SessionId);

/// The newest preview frame, Gray8 or Rgba8 per the session (a coalescing
/// `Latest` value: only the newest is kept). `None` before the first.
pub fn latest_preview(session: SessionId) -> Option<Frame>;

/// The session's zoom ratio range (`(1.0, max)`), once `Opened`.
pub fn zoom_range(session: SessionId) -> Option<(f32, f32)>;
/// The current zoom ratio.
pub fn zoom(session: SessionId) -> Option<f32>;
/// Set the zoom ratio, clamped to the range. `Zoom { source: App }`.
pub fn set_zoom(session: SessionId, ratio: f32);
/// Multiply the zoom ratio (a pinch's distance ratio since the last call).
pub fn zoom_by(session: SessionId, factor: f32);
```

**Gestures stay with the app.** libmobile delivers raw touches through
`MobileApp::input`; it does not recognise a pinch. As libcontrols puts it,
the component responds to calls and does not implement gestures: the app
tracks two pointers and calls `zoom_by(d_now / d_prev)` on each move, or
`set_zoom` from its own slider. libmorph's companion plan does the same
with its `Touch` events.

Zoom is applied on API 30+ with `ACAMERA_CONTROL_ZOOM_RATIO` (range from
`ACAMERA_CONTROL_ZOOM_RATIO_RANGE`, its lower end raised to 1.0 so a
logical multi-camera does not switch to the ultra-wide), which lets
Samsung and Pixel switch to the telephoto lens. On API 29 it is a centred
`ACAMERA_SCALER_CROP_REGION`: the active array
(`ACAMERA_SENSOR_INFO_ACTIVE_ARRAY_SIZE`) divided by the ratio, up to
`ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM`. The API level comes from
`AndroidApp::config().sdk_version()`.

### 4.4 QR (`camera-qr`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrMode {
    /// Stop after the first code (`Closed { reason: None }` follows `Qr`).
    Once,
    /// Keep scanning; the same payload is reported again only after
    /// `QR_REPEAT_MS` without it.
    Continuous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenExposure {
    /// Lower exposure when a code looks like it is on a bright screen (7).
    Auto,
    /// Never adjust.
    Off,
    /// Start at `QR_EV_FLOOR / 2` with metering on the centre: for apps
    /// that know the code is on a screen (a TV pairing code).
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct QrOptions {
    pub facing: Facing,                     // Back
    pub mode: QrMode,                       // Once
    pub auto_zoom: bool,                    // true
    pub exposure: ScreenExposure,           // Auto
    /// Preview frames for a viewfinder; `None` for none.
    pub preview: Option<PreviewOptions>,    // Some(default)
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct QrCode {
    /// The payload bytes as decoded (byte, alphanumeric, numeric and
    /// Kanji segments; ECI is not interpreted).
    pub bytes: Vec<u8>,
    /// `bytes` as UTF-8, when they are.
    pub text: Option<String>,
    /// Corners (top-left, top-right, bottom-right, bottom-left of the
    /// code) in the upright preview frame, as fractions 0.0..=1.0.
    pub corners: [(f32, f32); 4],
    /// QR version 1..=40.
    pub version: u8,
}

/// Start scanning. `Opened`, then `Qr` events. Asks for CAMERA first if
/// needed. `set_zoom`, `zoom_by`, `latest_preview` and `stop` take the id.
pub fn start_qr(app: &AndroidApp, options: &QrOptions) -> SessionId;
```

A payload is never logged past its length; it may be a pairing secret.

### 4.5 One camera at a time

Android phones open one camera per app in practice. One session runs at a
time: `start_qr` or `start_preview` while another session runs stops the
old one (`Closed { reason: None }`) first. `capture_frame` with a session
running on the same facing is served by that session (5.2), without
closing it; on the other facing, it fails with `CameraError::InUse`.

## 5. Inside

### 5.1 Threads

- **`libmobile-camera`**, spawned on the first camera call and kept for
  the process: owns `ACameraManager`, the device, the session, the image
  readers and every capture request. It takes commands (`Open`,
  `Capture`, `Cancel`, `Stop`, `SetZoom`, `Pause`, `Resume`, the gate's
  answer) over an `mpsc` channel. The NDK's device, session and
  capture-result callbacks, and the image listeners, run on NDK threads;
  they only send a message into the same channel, so all state lives on
  one thread with no locks beyond the channel.
- **`libmobile-qr`** (under `camera-qr`): decodes. The camera thread puts
  the newest grey frame into a one-slot cell (older ones are dropped) and
  the decoder thread takes it, runs rqrr, and sends a `Detection` back.
  Decoding never blocks the camera thread or the app.
- Results reach the app through a `Mailbox<CameraEvent>` drained in
  `pump_bridges` as `MobileEvent::Camera`, and discarded in
  `discard_stale_commands`, like the existing camera mailbox.
- Rust calls are non-blocking and may be made from any thread except a
  JNI callback (they send a command; `latest_preview`, `zoom` and
  `zoom_range` read a `Latest` value).

### 5.2 Streams

A session has one **analysis stream**: an `AImageReader`, `YUV_420_888`,
the largest size within 1280 × 720 with the display's aspect if offered
(else 4:3), `max_images = 3`, under a repeating `TEMPLATE_PREVIEW` request
with `CONTROL_AF_MODE_CONTINUOUS_PICTURE` (or `CONTINUOUS_VIDEO`, then
`AUTO`, by availability), `CONTROL_AE_MODE_ON`,
`CONTROL_AE_ANTIBANDING_MODE_AUTO`. Each image is copied out (the Y plane
for QR; Y and UV for an Rgba8 preview), scaled to the preview's
`max_side` where asked, and released at once.

A **capture** with no session opens the camera, starts the analysis stream
(at its smallest size, frames discarded), and adds a **still reader**
(the requested format and size from
`ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS`). It waits for 3A to
settle: `ACAMERA_CONTROL_AE_STATE` in `CONVERGED`, `FLASH_REQUIRED` or
`LOCKED`, and `ACAMERA_CONTROL_AF_STATE` in `PASSIVE_FOCUSED`,
`FOCUSED_LOCKED` or `NOT_FOCUSED_LOCKED` (or `INACTIVE` with no AF), or
`CAPTURE_3A_TIMEOUT_MS`, whichever comes first. Then it sends one
`TEMPLATE_STILL_CAPTURE` request to the still reader, delivers the image
and closes the camera.

With a session running, a capture at or below the analysis size in
`Gray8` or `Rgba8` takes the session's next analysis image. A larger one,
or `Jpeg`, reconfigures the session with a still reader for one request
(the preview stops for about the reconfigure time) and then returns to the
session's own outputs.

### 5.3 Conversion and orientation

- `Gray8`: the Y plane, copied row by row to drop the row stride.
- `Rgba8`: BT.601 full range (camera YUV_420_888 is JFIF full range),
  with U and V read through their pixel stride (1 or 2):
  `r = y + 1.402 (v−128)`, `g = y − 0.344136 (u−128) − 0.714136 (v−128)`,
  `b = y + 1.772 (u−128)`, clamped, in fixed point.
- Quarter turns: `d` = display rotation in degrees
  (`CameraDevicePlugin.displayRotation`, read when the camera opens and on
  `ConfigChanged`), `s` = `ACAMERA_SENSOR_ORIENTATION`. Back:
  `(s − d + 360) % 360`; front: `(s + d) % 360`; divided by 90.
  Gray8 and Rgba8 are rotated on the camera thread when `upright`.
- `Jpeg`: `ACAMERA_JPEG_ORIENTATION` is set to the same angle, and
  `ACAMERA_JPEG_QUALITY` to `jpeg_quality`. Some devices only write the
  EXIF tag instead of rotating; see section 10.

## 6. Auto zoom (`camera-qr`)

### 6.1 Measuring a detection

For each analysed frame (at most `QR_ANALYSIS_HZ`), the decoder thread
runs `PreparedImage::prepare_from_greyscale` on the grey frame and
`detect_grids`. For each grid:

- `decode_to` succeeds: a `Decoded { bytes, corners, version }`.
- It fails: a `Found { corners, modules }`, where `modules =
  grid.grid.size()`.

From the corners: `side` = mean of the four edge lengths (px);
`module_px = side / modules`; `centre` and the code's half-extent, both as
fractions of half the frame's short side. If no grid is found: `Missed`.

When `module_px >= 8` and decoding fails, the decoder also tries the frame
box-filtered 2 × 2, once: a screen's pixel grid at that scale causes moiré,
which halving the resolution averages out.

### 6.2 The zoom the app sets, and auto zoom on top of it

The session holds two numbers: `user` (the app's last `set_zoom` /
`zoom_by`, default 1.0) and `auto` (a factor >= 1.0). The ratio sent to
the camera is `user × auto`, clamped to the device range.

- **Auto zoom fine-tunes around the user's choice.** It only zooms in
  from `user`, never below it, by at most `QR_AUTO_MAX_FACTOR` (3×) and
  never above `QR_AUTO_MAX_RATIO` (8.0, where digital zoom stops helping).
- **A pinch wins.** Each `set_zoom` / `zoom_by` sets `user`, resets `auto`
  to 1.0, and suspends auto zoom for `QR_USER_HOLD_MS` (1.5 s) after the
  last call, so it never fights a pinch in progress. A user who frames a
  TV across a room at 4× gets auto zoom from 4× upward.
- When the code is lost, auto zoom returns to `user`, not to 1.0.

### 6.3 The rule, once per adjustment window

Nothing changes while the last change is settling (`QR_ZOOM_SETTLE_MS`
after a zoom change, `QR_AE_SETTLE_MS` after an exposure change, below).
Then, on the newest detection:

1. `Decoded`: report it; keep the zoom and exposure while the code stays
   in view. `misses = 0`.
2. `Found` and `module_px < QR_TARGET_MODULE_PX` (4 px): zoom in by
   `k = min(QR_TARGET_MODULE_PX / module_px, QR_ZOOM_STEP_MAX, k_fit)`,
   where `k_fit = QR_FRAME_MARGIN / (centre + half_extent)` keeps the
   whole code inside 90 % of the frame (zoom is centred, so an off-centre
   code limits it). Apply only if `k >= 1.1`; else go to 3.
3. `Found`, big enough, and the screen test of 7.1 holds: the exposure
   ladder (7.2).
4. `Found`, big enough, not bright: if no AF trigger in the last
   `QR_AF_RETRIGGER_MS`, set `ACAMERA_CONTROL_AF_REGIONS` to the code's box
   (where `ACAMERA_CONTROL_MAX_REGIONS` allows an AF region) and send
   `ACAMERA_CONTROL_AF_TRIGGER_START` once.
5. `Missed`: `misses += 1`. At `QR_LOST_FRAMES` misses, step back:
   `auto /= QR_ZOOM_STEP_MAX` (to 1.0 at least), exposure back to 0 EV
   and regions cleared, each as its own window.

One kind of change per window, in the order zoom, exposure, focus, so the
effect of each is seen before the next.

### 6.4 Mapping a box to sensor coordinates

The analysis frame shows the current field of view, cropped to the
stream's aspect. A point `(x, y)` in the frame (fractions) maps to the
field as `fx = x`, `fy = 0.5 + (y − 0.5) × a_s / a_f` when the stream is
wider than the field (`a` = width / height), or the same on `x` when it is
taller. Regions are then `fx × W`, `fy × H` with `W × H` the active array
on API 30+ with the zoom ratio (regions are in the post-zoom field there),
or `crop.x + fx × crop.w`, `crop.y + fy × crop.h` with the crop region on
API 29. Each region gets weight 1000 and is clamped to the array.

## 7. Exposure for codes on screens (`camera-qr`)

Codes on a TV, monitor or phone are emitted light. Auto-exposure metered
on a dim room overexposes the screen, washing out the modules, and the
screen's pixel grid adds moiré.

### 7.1 The screen test

Over the code's bounding box (or, when `Missed` for `QR_LOST_FRAMES` with
`ScreenExposure::Auto`, the central 50 % of the frame), sampling every
second pixel of the Y plane: `mean` and `p_sat`, the fraction of samples
>= 245. The code looks overexposed when `p_sat >= QR_SAT_FRACTION` (0.08)
or `mean >= QR_SAT_MEAN` (200). The central-area test only starts the
ladder when no code has been found at all; it never runs while a code
decodes.

### 7.2 The ladder

Each rung is one adjustment window; the next rung is taken only if the
code still fails and still looks overexposed.

1. **Meter on the code.** `ACAMERA_CONTROL_AE_REGIONS` = the code's box
   (6.4), if `ACAMERA_CONTROL_MAX_REGIONS` allows an AE region. Often
   enough on its own: AE then exposes for the screen.
2. **Step down.** `ACAMERA_CONTROL_AE_EXPOSURE_COMPENSATION` lowered by
   `QR_EV_STEP` (1 EV, converted to device steps:
   `round(1.0 / ACAMERA_CONTROL_AE_COMPENSATION_STEP)`), down to
   `max(QR_EV_FLOOR, range.min)` with `QR_EV_FLOOR` = −2 EV and the
   range from `ACAMERA_CONTROL_AE_COMPENSATION_RANGE`.
3. **Bracket.** At the floor, after `QR_BRACKET_AFTER` (3) failing
   windows: alternate between the floor and half of it, one window each,
   for `QR_BRACKET_WINDOWS` (6) windows. Alternating per frame does not
   work: AE takes several frames to reach a new target, so each setting
   is held for a window.
4. **Give up quietly.** Back to the floor and stay there until the code
   is lost (6.3 step 5 resets to 0 EV) or decodes.

`QR_AE_SETTLE_MS` (400 ms) ends early when the capture result's
`ACAMERA_CONTROL_AE_STATE` reports `CONVERGED` or `LOCKED`.

### 7.3 With auto zoom

Zoom first: a code too small to read is not helped by exposure, and
zooming in makes the screen fill more of the metered area, which lowers
exposure by itself. Exposure rungs are taken only when `module_px` is
already at the target. A pinch (`set_zoom`) leaves exposure as it is.
`ScreenExposure::Screen` starts at rung 2 with half the floor and the
centre 50 % metered, and the ladder continues from there.
`ScreenExposure::Off` skips 6.3 step 3 and the central-area test.

## 8. Lifecycle

- **Pause** (`MobileEvent::Pause` in the loop): the camera thread closes
  the session's device (Android requires a backgrounded app to release
  it). A running session sends `Suspended`; a capture in progress ends
  with `Interrupted`; a call still waiting for the permission answer keeps
  waiting (the dialog itself pauses the activity, so this is normal).
- **Resume**: a suspended session reopens with its facing, `user` zoom
  and exposure mode (auto zoom and the ladder start over), then `Opened`.
- **Eviction** (`onDisconnected`, or `onError` with
  `ERROR_CAMERA_IN_USE` / `MAX_CAMERAS_IN_USE` while resumed, such as a
  call app or split-screen): `Closed { reason: Some(InUse) }`. The app
  decides whether to start again.
- **Destroy**: every session and capture closes; the mailbox is
  discarded; `android_main` running again starts clean.
- **Rotation** (`ConfigChanged`): the display rotation is read again; it
  changes quarter turns for the next frames only.

## 9. Decoder choice

| | rqrr 0.11 (chosen) | rxing | ML Kit barcode scanning | ZXing core (Java) |
|---|---|---|---|---|
| Where it runs | Rust, any host | Rust, any host | Kotlin, Android with Play services or a bundled model | Kotlin, any JVM |
| Licence | (MIT OR Apache-2.0) AND ISC | Apache-2.0 | ML Kit terms (proprietary) | Apache-2.0 |
| Formats | QR only | QR, Data Matrix, Aztec, 1D, ... | QR and most 1D/2D | QR and most 1D/2D |
| Size | small; one dependency (`g2p`) | larger: many formats | bundled model about 2.4 MB per ABI (Google's figure), or a Play services download | about 0.5 MB jar |
| Offline | yes | yes | bundled yes; unbundled needs a first download | yes |
| Input here | our Y plane, no copy across JNI | same | frames must cross JNI, or Kotlin owns the camera (a second stack) | frames cross JNI |
| Robustness | quirc-grade: good on clean codes; weaker on damaged, curved or very low contrast | ZXing-grade | best | ZXing-grade |

rqrr is chosen: it is pure Rust, small, permissively licensed, works on
the Y plane the NDK camera already gives, and the same code can later
serve libmorph's desktop and web cameras. It also reports a code's
corners and module count when decoding fails, which is what auto zoom
needs. The decoder sits behind a private trait
(`fn scan(&mut self, grey: &GreyFrame) -> Vec<Detection>`), so rxing can
replace it under a later feature if rqrr proves too weak on screens.

## 10. Risks

- **LEGACY devices.** The NDK camera does not list cameras at the LEGACY
  hardware level; on such a phone every call ends in `NoCamera`. Old and
  low-end devices only; the S23 is FULL/LEVEL_3.
- **JPEG orientation.** Some HALs honour `JPEG_ORIENTATION` only through
  the EXIF tag. `Frame::quarter_turns` is then 0 while the pixels are not
  rotated. Step 3 checks the S23; if it only writes EXIF, `Jpeg` frames
  report the sensor's turns instead (a departure to record).
- **Decode time.** rqrr on a 1280 × 720 grey frame is not measured on the
  S23. If it exceeds `QR_DECODE_BUDGET_MS` (40 ms) at the 50th percentile,
  analysis drops to 960 × 540.
- **rqrr on screens.** Moiré and glare may defeat it more often than ML
  Kit. The 2 × 2 retry and the exposure ladder are the mitigation; the
  decoder trait is the way out.
- **Zoom ratio below 1.0.** Kept out: the lower end is raised to 1.0.
- **Play filtering.** If an app also declares `android.hardware.camera`
  as required in its own manifest, that wins; the README says so.
- **Privacy indicator.** Android shows the camera-in-use dot while a
  session runs; sessions are closed on pause, so it never shows in the
  background.

## 11. Tunable constants (living section)

| Name | Value | Meaning |
|---|---|---|
| `PREVIEW_MAX` | 1280 × 720 | largest analysis stream |
| `IMAGE_READER_MAX` | 3 | buffers per `AImageReader` |
| `CAPTURE_TIMEOUT` | 5 s | default `CaptureOptions::timeout` |
| `CAPTURE_3A_TIMEOUT_MS` | 1500 | longest wait for AE/AF before a still |
| `QR_ANALYSIS_HZ` | 15 | most decodes per second |
| `QR_DECODE_BUDGET_MS` | 40 | median decode time before analysis shrinks |
| `QR_REPEAT_MS` | 2000 | same payload reported again after this, `Continuous` |
| `QR_TARGET_MODULE_PX` | 4.0 | module size auto zoom aims for |
| `QR_ZOOM_STEP_MAX` | 1.6 | largest zoom factor per window |
| `QR_FRAME_MARGIN` | 0.9 | the code must stay within this part of the frame |
| `QR_AUTO_MAX_FACTOR` | 3.0 | auto zoom above the user's zoom, at most |
| `QR_AUTO_MAX_RATIO` | 8.0 | auto zoom never beyond this ratio |
| `QR_USER_HOLD_MS` | 1500 | auto zoom paused after the app sets the zoom |
| `QR_ZOOM_SETTLE_MS` | 300 | wait after a zoom change |
| `QR_AF_RETRIGGER_MS` | 1000 | least time between AF triggers |
| `QR_LOST_FRAMES` | 20 | misses before stepping back (about 1.3 s) |
| `QR_SAT_FRACTION` | 0.08 | fraction of samples >= 245 that means overexposed |
| `QR_SAT_MEAN` | 200 | mean luma that means overexposed |
| `QR_EV_STEP` | 1.0 EV | one exposure rung |
| `QR_EV_FLOOR` | −2.0 EV | lowest compensation the ladder uses |
| `QR_AE_SETTLE_MS` | 400 | wait after an exposure change (less if AE converges) |
| `QR_BRACKET_AFTER` | 3 | failing windows at the floor before bracketing |
| `QR_BRACKET_WINDOWS` | 6 | windows of bracketing before giving up |

## 12. Relation to libmorph's camera view plan (Hold)

libmorph's `20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md` is on Hold. This
plan does not change it, and does not start any of its steps. They
overlap:

- Its libmobile `camera-view` (6.3 there) is close to this plan's
  `camera-device`: NDK camera, CAMERA permission, `AImageReader`. This plan
  asks for the permission lazily through the core permission bridge
  (which that plan's 6.2 predates), and gives CPU preview frames instead
  of a GPU texture path.
- Its 6.1 renames `camera` to `camera-capture` for the system camera app.
  Here `camera-capture` is an in-process still. Both cannot hold.

If this plan is approved, the Hold plan's 6.1 and 6.3 would be replaced by
this one; how to record that is section 13, question 2.

## 13. Open questions

1. **Names.** `camera-device` as the base and `camera` left unchanged
   (additive, 0.2.7), or the owner's `camera` as the base with the system
   camera app moved to `camera-app` (breaking, 0.3.0; libmorph's
   `mobile-camera` follows)?
2. **The Hold plan.** Record the overlap as an amendment to libmorph's
   Hold plan once this is approved (returning it to Draft with 6.1 and 6.3
   replaced), or leave it on Hold for the GPU preview only?
3. **Decoder.** rqrr now, with rxing or ML Kit only if the S23 checks
   show it failing on screens? The ISC part of rqrr's licence (from quirc)
   needs its notice kept in apps' licence screens.
4. **The camera's own permission answer.** Consumed by the gate (as
   libmorph does for `RECORD_AUDIO`), or also delivered as
   `MobileEvent::Permissions`?
5. **Search zoom.** When nothing is detected at all at `user` zoom for a
   while, should auto zoom probe 2× for a few frames (a code too small
   even to find)? Not in this plan.
6. **More controls.** The torch (`FLASH_MODE_TORCH`) for dark scenes and
   an app-set exposure compensation are cheap on the same session; in
   this plan, or later?
7. **Defaults.** `QrMode::Once` and a preview on by default?

## 14. Steps

Each step is one commit to libmobile (with its tests), passing
`cargo fmt -- --check`, host clippy with no features and
`--all-features`, Android clippy (`--target aarch64-linux-android`) with
no features, `--features camera-capture`, `--features camera-qr` and
`--all-features`, and `cargo test --workspace`.

1. **`open_app_settings`** (3.3): the Kotlin call in `LinkPlugin.kt` and
   the Rust wrapper.
   **Done when:** the checks above pass and a generated project builds
   with `gradle assembleDebug`.
2. **`camera-device`: feature, manifest and gate** (2, 3.1, 3.2, 3.4):
   the features, `build.rs` link line, the plugin and
   `CameraDevicePlugin.kt`, `camera_device` types, the permission gate and
   the routing in `pump_bridges`, `MobileEvent::Camera`. Host tests: the
   manifest has CAMERA and the three optional `uses-feature`s only with
   `camera-device` (and `camera.any` once with both camera features); the
   gate answers waiting calls in order for grant, denial and
   `settings_only`.
   **Done when:** the checks pass and the generated manifest of a
   `camera`-only app is byte-identical to today's.
3. **`camera-device`: sessions, preview, zoom, lifecycle** (4.3, 4.5,
   5.1, 5.2, 8): the camera thread, `start_preview`, `stop`,
   `latest_preview`, `zoom_range`, `zoom`, `set_zoom`, `zoom_by`. Host
   tests: quarter turns for every sensor orientation, facing and display
   rotation; crop-region and region mapping (6.4); zoom clamping for
   both API paths.
   **Done when:** the checks pass, and on the S23 the example of step 6
   (built with this step) shows a live preview, pinch zoom reaches the
   telephoto range, and pause and resume reopen it.
4. **`camera-capture`** (4.2, 5.2, 5.3): `capture_frame`,
   `cancel_capture`, the 3A wait, the three formats, timeout and cancel.
   Host tests: YUV to RGBA against known pixels for pixel strides 1 and
   2; stride packing; rotation of a 3 × 2 image; the capture state machine
   (timeout, cancel, interrupted).
   **Done when:** the checks pass, and on the S23 a capture with no
   session returns an upright `Rgba8` frame at `Preview` and a `Jpeg` at
   `Max`, and a capture during a preview does not close it.
5. **`camera-qr`** (4.4, 6, 7): rqrr, the decoder thread, the auto zoom
   and exposure controllers as pure functions of
   `(state, detection, now) -> (state, action)`. Host tests, with codes
   rendered by the `qrcode` crate (a dev-dependency only): decoding at
   several module sizes; a 2 px-module code yields a zoom-in with `k`
   capped by `QR_ZOOM_STEP_MAX` and `k_fit`; auto zoom never goes below
   `user` and pauses for `QR_USER_HOLD_MS` after `set_zoom`; a
   saturated, found-but-undecoded code walks the ladder (region, −1, −2,
   bracket, give up) one rung per window; loss resets zoom and exposure;
   the 2 × 2 retry decodes a code with a synthetic pixel-grid pattern;
   `Continuous` dedup.
   **Done when:** the checks pass and the host tests above pass.
6. **Example `examples/camerascan`** (its own commit, a workspace
   member): CPU-drawn preview (as `examples/bitmap`), two-finger pinch
   driving `zoom_by`, a tap to capture, scanning in `Continuous` mode,
   each event logged (payload lengths only), and a "Settings" tap on
   `settings_only`.
   **Done when:** on the owner's S23 (the owner asked first; checks
   recorded in `CHANGES.md` and evidence under `out/camera/`): launch
   shows no permission dialog; the first scan shows it; "Don't allow"
   twice gives `settings_only: true` and Settings opens; a printed code
   at arm's length decodes at 1×; a small printed code at 2 m zooms in and
   decodes; a code on a TV across the room decodes after a pinch to about
   4× and auto zoom; a code on a monitor at full brightness decodes after
   the exposure ladder (logged rungs); pause and resume during a scan
   resume it.
7. **README** (its own commit): the features table rows for
   `camera-device`, `camera-capture` and `camera-qr`, the layout line for
   `cameradevice/`, and this plan's decision-table row updated.
   **Done when:** the README lists the three features and their manifest
   effects.

## 15. Release

All additive: three new features, new modules and types, a variant of
the `#[non_exhaustive]` `MobileEvent`, and `open_app_settings`. Apps
without the new features see no change. Released as libmobile 0.2.7
(after any release made in the meantime, the next patch). If question 1
is answered with the rename, 0.3.0 instead, with the rename marked
**Breaking**.

## Amendment — 2026-10-28 14:46 EDT: `camera` is the in-process base, the hand-off is `camera-intent`; a wgpu viewfinder; fine zoom; failing with the reason; approved

The owner (2026-10-28), in order: "what's CameraX and camera-app? we
shouldnt be bringing in jetpack or similar if it can be avoided using a
wgpu viewfinder. the autozoom should also adjust fine zoom on supported
devices to read qr code. what is open_app_settings and why would it be
rejecting camera permission? the camera init should fail if user rejects
with the reason given if we hit an Android limit that might be why we'd
open the permissions screen in settings"; on the hand-off's name, "that's
more like camera-intent"; on libmorph's Hold plan, "just change it to
camera-intent in the plan and record the change of the camera feature";
then "approve this and the camera plans". Where this disagrees with the
body, it wins.

1. **Features.** libmobile's `camera` becomes the base of the in-process
   NDK camera (what the body calls `camera-device`: `ACamera*`,
   `AImageReader`, CAMERA and `uses-feature` with `required=false`,
   `libcamera2ndk`); `camera-capture` and `camera-qr` build on it. Today's
   system-camera-app hand-off (`ACTION_IMAGE_CAPTURE`) is renamed
   **`camera-intent`**. This is breaking: libmobile **0.3.0**. libmorph's
   passthroughs are `mobile-camera` (the base), `mobile-camera-capture`,
   `mobile-camera-qr` and `mobile-camera-intent` (the hand-off), recorded
   in libmorph's camera view plan (Hold) by its 2026-10-28 amendment.
   Every `camera-device` in the body reads `camera`, and the body's
   `camera` for the hand-off reads `camera-intent`.
2. **No Jetpack.** No AndroidX, CameraX or other Jetpack library: the NDK
   camera only, and the small Kotlin plugin for display rotation.
3. **A wgpu viewfinder.** Preview frames reach the screen through wgpu:
   each frame's YUV planes are uploaded into textures and converted to RGB
   in a shader; libmorph provides this as its camera viewfinder (no
   Android view). CPU frames stay available for capture and QR decoding.
4. **Fine zoom.** On API 30+ (`CONTROL_ZOOM_RATIO`, a continuous value)
   auto zoom ends with fine adjustments to the ratio that gives the code a
   readable module size, rather than stopping at the coarse steps; on API
   29 (crop region) it keeps the steps.
5. **Failing with the reason.** Starting the camera fails with
   `PermissionDenied { settings_only }` when the user refuses;
   `settings_only` is true when Android will no longer show the dialog.
   The library never opens Settings itself; `open_app_settings` is a
   helper the app may call (for example from a "Grant in Settings" button)
   when `settings_only` is true.
6. **Open questions** not answered keep the body's proposals (rqrr; the
   camera's permission answer consumed by its gate; no 2× probe when
   nothing is detected; torch and app-set exposure later; scan once with
   the preview on by default; the `mobile-` names).
7. **Approved.** The plan is approved with this amendment; the status
   becomes `In implementation` with its first code commit.

## Amendment — 2026-10-28 15:13 EDT: implementation has started; the document is now immutable

Implementation has started with the camera base and single-frame capture
(steps 1-4, in one commit). The status is `In implementation`, and the
document is now immutable: it changes only on the owner's explicit
instruction.
