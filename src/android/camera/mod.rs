//! The in-process camera (feature `camera`; `camera/CameraPlugin.kt` for the
//! display's rotation): the NDK Camera2 API, `ACamera*` and `AImageReader`,
//! with no Jetpack and no Android view.
//!
//! The types are platform-free, in [`crate::camera`], and re-exported here.
//! Every call is non-blocking and may be made from any thread except a JNI
//! callback: it sends a command to the `libmobile-camera` thread, which owns
//! the camera, and results arrive as
//! [`MobileEvent::Camera`](crate::MobileEvent::Camera). The thread starts on
//! the first camera call; an app that never calls one has none, and CAMERA
//! is never asked for.
//!
//! * [`start_preview`] opens a session; [`latest_preview`] polls its newest
//!   [`PreviewFrame`] (YUV planes for a GPU viewfinder); [`stop`] ends it.
//!   One session at a time: a new one stops the old one first.
//! * [`set_zoom`], [`zoom_by`], [`zoom_range`], [`zoom`]: the app drives
//!   zoom (a pinch it recognises itself, or a slider).
//! * `capture_frame` / `cancel_capture` (feature `camera-capture`): one
//!   still, served by a running session on the same facing without closing
//!   it.
//!
//! The first call asks for CAMERA if it is not granted, and waits for the
//! answer. A refusal fails it with
//! [`CameraError::PermissionDenied`]`{ settings_only }`; libmobile never
//! opens Settings, but the app may call
//! [`open_app_settings`](super::open_app_settings) when `settings_only` is
//! set.
//!
//! Lifecycle: on `Pause` the camera closes (a session reports `Suspended`, a
//! capture in progress ends `Interrupted`); on `Resume` a suspended session
//! reopens (`Opened` again); on `Destroy` everything is dropped.

mod ndk;
mod worker;

pub use crate::camera::{
    CameraError, CameraEvent, CaptureId, Facing, PreviewFrame, PreviewOptions, SessionId,
    ZoomSource,
};
#[cfg(feature = "camera-capture")]
pub use crate::camera::{CaptureOptions, Frame, FrameFormat, Resolution};

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use android_activity::AndroidApp;

use crate::MobileEvent;
use crate::event::PermissionsOutcome;
use worker::{Msg, ZoomChange};

/// The permission the camera needs.
pub(crate) const CAMERA_PERMISSION: &str = "android.permission.CAMERA";

static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);
#[cfg(feature = "camera-capture")]
static NEXT_CAPTURE: AtomicU32 = AtomicU32::new(1);

/// Open the camera facing `facing` for a live session (a viewfinder).
/// `Opened` follows once it streams; poll its frames with
/// [`latest_preview`]. Asks for CAMERA first if needed. A session already
/// running is stopped first (`Closed { reason: None }`).
pub fn start_preview(app: &AndroidApp, facing: Facing, preview: PreviewOptions) -> SessionId {
    let session = SessionId(NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
    worker::post(Msg::StartPreview {
        app: app.clone(),
        session,
        facing,
        preview,
    });
    session
}

/// Stop a session. `Closed { reason: None }` follows (also when it was still
/// waiting for the permission answer).
pub fn stop(session: SessionId) {
    worker::post_if_running(Msg::Stop(session));
}

/// The session's newest preview frame, or `None` before the first (or once
/// it has stopped). The same frame is returned until a newer one arrives;
/// [`PreviewFrame::sequence`] tells them apart.
pub fn latest_preview(session: SessionId) -> Option<Arc<PreviewFrame>> {
    worker::shared(|s| s.preview_of(session))
}

/// The session's zoom ratio range, `(1.0, max)`, once `Opened`.
pub fn zoom_range(session: SessionId) -> Option<(f32, f32)> {
    worker::shared(|s| s.zoom_range_of(session))
}

/// The session's current zoom ratio, once `Opened`.
pub fn zoom(session: SessionId) -> Option<f32> {
    worker::shared(|s| s.zoom_of(session))
}

/// Set the zoom ratio, clamped to [`zoom_range`].
/// `Zoom { source: App }` follows when it changes. Before `Opened`, it is
/// kept and applied on opening.
pub fn set_zoom(session: SessionId, ratio: f32) {
    worker::post_if_running(Msg::Zoom {
        session,
        change: ZoomChange::Set(ratio),
    });
}

/// Multiply the zoom ratio by `factor`: for a pinch, the ratio of the two
/// pointers' distance now to their distance at the last call.
pub fn zoom_by(session: SessionId, factor: f32) {
    worker::post_if_running(Msg::Zoom {
        session,
        change: ZoomChange::By(factor),
    });
}

/// Take one still. The answer is `CameraEvent::Frame` with this id. Asks for
/// CAMERA first if needed; the timeout starts when the camera opens.
#[cfg(feature = "camera-capture")]
pub fn capture_frame(app: &AndroidApp, options: &CaptureOptions) -> CaptureId {
    let capture = CaptureId(NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed));
    worker::post(Msg::Capture {
        app: app.clone(),
        capture,
        options: options.clone(),
    });
    capture
}

/// End a capture early with `CameraError::Cancelled` (no-op once answered).
#[cfg(feature = "camera-capture")]
pub fn cancel_capture(capture: CaptureId) {
    worker::post_if_running(Msg::Cancel(capture));
}

/// The system camera app hand-off (`camera-intent`) waits for CAMERA here.
#[cfg(feature = "camera-intent")]
pub(crate) fn gate_intent(app: &AndroidApp) {
    worker::post(Msg::Intent { app: app.clone() });
}

/// The main loop's lifecycle events, for the camera thread (only if it
/// runs).
pub(crate) fn lifecycle(app: &AndroidApp, event: &MobileEvent) {
    worker::lifecycle(app, event);
}

/// Offer a permission answer to the gate: `None` if it was the gate's
/// (consumed), else the outcome back for `MobileEvent::Permissions`.
pub(crate) fn offer_permission(outcome: PermissionsOutcome) -> Option<PermissionsOutcome> {
    worker::offer_permission(outcome)
}

pub(crate) fn take_events() -> Vec<CameraEvent> {
    worker::take_events()
}
