//! **The in-process camera** (feature `camera`): the platform-free half.
//!
//! The calls live in `android::camera` (the NDK Camera2 API: `ACamera*`
//! and `AImageReader`, no Jetpack); this module holds the types they take
//! and report, and the pure parts of the work (pixel conversion and
//! rotation, zoom and region geometry, size choice, the permission gate and
//! the capture state machine), so they build and test on any host.
//!
//! * A **session** ([`SessionId`]) is a running camera: `start_preview`
//!   opens one, `stop` ends it. Its frames are polled with `latest_preview`
//!   as [`PreviewFrame`]s, YUV 4:2:0 planes for a GPU viewfinder (libmorph
//!   converts them to RGB in a shader) or the CPU. One session runs at a
//!   time.
//! * A **capture** ([`CaptureId`], feature `camera-capture`) is one still
//!   frame, with or without a session.
//! * Results and state changes arrive as
//!   [`MobileEvent::Camera`](crate::MobileEvent::Camera) ([`CameraEvent`]).
//!
//! CAMERA is asked for on the first camera call, never at launch. A refusal
//! fails the call with [`CameraError::PermissionDenied`]; when
//! `settings_only` is set, Android will no longer show the dialog, and the
//! app may send the user to `android::open_app_settings`. libmobile never
//! opens Settings itself.
//!
//! Gestures stay with the app: libmobile delivers raw touches and does not
//! recognise a pinch. The app tracks two pointers and calls `zoom_by` with
//! the distance ratio since its last call, or `set_zoom` from its own slider.

use std::fmt;

#[cfg(feature = "camera-capture")]
pub(crate) mod capture;
pub(crate) mod convert;
pub(crate) mod gate;
pub(crate) mod geometry;

#[cfg(feature = "camera-capture")]
pub use capture::{CaptureOptions, Frame, FrameFormat, Resolution};

/// Largest analysis (preview) stream: the longer side, then the shorter.
pub(crate) const PREVIEW_MAX: (u32, u32) = (1280, 720);
/// Buffers per `AImageReader`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // The Android camera thread.
pub(crate) const IMAGE_READER_MAX: i32 = 3;
/// Default `CaptureOptions::timeout`, milliseconds.
#[cfg(feature = "camera-capture")]
pub(crate) const CAPTURE_TIMEOUT_MS: u64 = 5000;
/// Longest wait for AE/AF to settle before a still, milliseconds.
#[cfg(feature = "camera-capture")]
pub(crate) const CAPTURE_3A_TIMEOUT_MS: u64 = 1500;

/// Which way the camera faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Facing {
    /// The camera on the back of the phone.
    #[default]
    Back,
    /// The selfie camera.
    Front,
}

/// A running camera session (a preview). Opaque; ids count from 1 per
/// process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub(crate) u32);

impl SessionId {
    /// The id as a number, for logs and for wrappers with their own id type.
    pub fn id(self) -> u32 {
        self.0
    }
}

/// One `capture_frame` call. Opaque; ids count from 1 per process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureId(pub(crate) u32);

impl CaptureId {
    /// The id as a number, for logs and for wrappers with their own id type.
    pub fn id(self) -> u32 {
        self.0
    }
}

/// Why a session closed or a capture failed.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraError {
    /// The user refused CAMERA. `settings_only`: asking again shows no
    /// dialog (the user chose "Don't allow" twice, or the device policy
    /// forbids it); the app may offer `android::open_app_settings`.
    PermissionDenied { settings_only: bool },
    /// No camera with that facing (or the NDK lists none: LEGACY devices).
    NoCamera,
    /// Another app holds the camera (`ERROR_CAMERA_IN_USE`,
    /// `ERROR_MAX_CAMERAS_IN_USE`) or took it from us (`onDisconnected`), or
    /// a capture asked for the other facing while a session runs.
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

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied {
                settings_only: true,
            } => f.write_str("camera permission refused; only Settings can grant it now"),
            Self::PermissionDenied {
                settings_only: false,
            } => f.write_str("camera permission refused"),
            Self::NoCamera => f.write_str("no camera with that facing"),
            Self::InUse => f.write_str("the camera is in use"),
            Self::Disabled => f.write_str("the camera is disabled by policy"),
            Self::Timeout => f.write_str("the capture timed out"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Interrupted => f.write_str("interrupted by the activity pausing"),
            Self::Unsupported(what) => write!(f, "not supported by this camera: {what}"),
            Self::Failed(message) => write!(f, "camera failed: {message}"),
        }
    }
}

impl std::error::Error for CameraError {}

/// What changed the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomSource {
    /// `set_zoom` or `zoom_by`.
    App,
    /// Automatic zoom (QR scanning).
    Auto,
}

/// What the camera reports, as [`MobileEvent::Camera`](crate::MobileEvent::Camera).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraEvent {
    /// A session's camera is open and streaming (again, after a resume).
    /// `size` is the size of its [`PreviewFrame`]s (scaled, and turned when
    /// upright); `zoom_range` as [`zoom_range`](crate::android::camera::zoom_range).
    Opened {
        session: SessionId,
        facing: Facing,
        size: (u32, u32),
        zoom_range: (f32, f32),
    },
    /// The session's camera closed: `reason` is `None` for `stop` (or a
    /// newer session replacing it).
    Closed {
        session: SessionId,
        reason: Option<CameraError>,
    },
    /// The activity paused: the session's camera is closed and will reopen on
    /// resume (no event until `Opened`).
    Suspended { session: SessionId },
    /// The zoom changed.
    Zoom {
        session: SessionId,
        ratio: f32,
        source: ZoomSource,
    },
    /// A capture finished.
    #[cfg(feature = "camera-capture")]
    Frame {
        capture: CaptureId,
        result: Result<Frame, CameraError>,
    },
}

/// How a session's preview frames are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreviewOptions {
    /// Longest side of the frames `latest_preview` returns; the camera's
    /// stream is scaled down by a whole factor to fit. Default 640.
    pub max_side: u32,
    /// Turn the planes upright on the CPU (`quarter_turns` is then 0).
    /// `false` leaves them as the sensor delivers them and reports the turns
    /// still needed, for a viewfinder that rotates in its shader. Default
    /// `true`.
    pub upright: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            max_side: 640,
            upright: true,
        }
    }
}

/// One preview frame: YUV 4:2:0 as three planes (I420), rows packed, BT.601
/// full range (as camera YUV_420_888 is). The layout a wgpu viewfinder
/// uploads as three `R8Unorm` textures and converts in its shader.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PreviewFrame {
    /// Luma size, after scaling (and turning, when upright).
    pub width: u32,
    pub height: u32,
    /// `width × height` bytes.
    pub y: Vec<u8>,
    /// `chroma_size()` bytes each.
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    /// Clockwise quarter turns still needed to show it upright: 0 when the
    /// session's `upright` was set.
    pub quarter_turns: u8,
    pub facing: Facing,
    /// `AImage` timestamp (sensor clock, ns).
    pub timestamp_ns: i64,
    /// Counts up per frame of a session, from 1: a viewfinder skips the
    /// upload when it has not changed.
    pub sequence: u64,
}

impl PreviewFrame {
    /// The size of the `u` and `v` planes: half the luma's, rounded up.
    pub fn chroma_size(&self) -> (u32, u32) {
        (self.width.div_ceil(2), self.height.div_ceil(2))
    }

    /// The luma plane: a greyscale image of the frame.
    pub fn gray(&self) -> &[u8] {
        &self.y
    }

    /// The frame as RGBA, 4 bytes per pixel, rows packed (the CPU path; a
    /// viewfinder converts on the GPU instead).
    pub fn to_rgba(&self) -> Vec<u8> {
        let (cw, _) = self.chroma_size();
        let planes = convert::Planes {
            width: self.width as usize,
            height: self.height as usize,
            y: &self.y,
            y_row: self.width as usize,
            u: &self.u,
            v: &self.v,
            uv_row: cw as usize,
            uv_pixel: 1,
        };
        convert::to_rgba(&planes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_defaults() {
        let p = PreviewOptions::default();
        assert_eq!((p.max_side, p.upright), (640, true));
        assert_eq!(Facing::default(), Facing::Back);
    }

    #[test]
    fn preview_frame_chroma_rounds_up_and_converts() {
        let frame = PreviewFrame {
            width: 3,
            height: 1,
            y: vec![0, 128, 255],
            u: vec![128, 128],
            v: vec![128, 128],
            quarter_turns: 0,
            facing: Facing::Back,
            timestamp_ns: 0,
            sequence: 1,
        };
        assert_eq!(frame.chroma_size(), (2, 1));
        assert_eq!(frame.gray(), [0, 128, 255]);
        assert_eq!(
            frame.to_rgba(),
            [0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn errors_display() {
        assert_eq!(
            CameraError::PermissionDenied {
                settings_only: true
            }
            .to_string(),
            "camera permission refused; only Settings can grant it now"
        );
        assert_eq!(
            CameraError::Unsupported("jpeg").to_string(),
            "not supported by this camera: jpeg"
        );
    }
}
