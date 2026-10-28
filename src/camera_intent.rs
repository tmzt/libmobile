//! **Photo capture results** (feature `camera-intent`, named `camera` before
//! 0.3.0; `camera` is now the in-process camera, `crate::camera`).
//!
//! `android::camera_intent::capture_photo` hands off to the system camera app
//! (`MediaStore.ACTION_IMAGE_CAPTURE`) and `CameraIntentPlugin.onActivityResult`
//! returns the result as a
//! [`MobileEvent::Capture`](crate::MobileEvent::Capture). No camera permission
//! is needed: the photo is taken by another app. With the `camera` feature
//! also on, CAMERA is in the manifest and Android requires it granted before
//! the hand-off, so it is asked for first; a refusal is
//! `CaptureOutcome::Failed("permission_denied")`.
//!
//! What comes back is the intent's preview bitmap, JPEG-encoded by the plugin —
//! a thumbnail, not a full-resolution file. Like login, every path produces an
//! outcome, including a cancelled camera.

/// What the camera hand-off produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// JPEG bytes of the captured preview.
    Jpeg(Vec<u8>),
    /// Cancelled, no camera app, or an encode failure; the message is for logs.
    Failed(String),
}

impl CaptureOutcome {
    /// Map the Kotlin callback's `(jpeg?, error?)` pair.
    pub fn from_parts(jpeg: Option<Vec<u8>>, error: Option<String>) -> Self {
        match (jpeg, error) {
            (Some(bytes), _) => CaptureOutcome::Jpeg(bytes),
            (None, Some(error)) => CaptureOutcome::Failed(error),
            (None, None) => {
                CaptureOutcome::Failed("camera returned neither an image nor an error".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parts() {
        assert_eq!(
            CaptureOutcome::from_parts(Some(vec![0xff, 0xd8]), None),
            CaptureOutcome::Jpeg(vec![0xff, 0xd8])
        );
        assert_eq!(
            CaptureOutcome::from_parts(None, Some("cancelled".into())),
            CaptureOutcome::Failed("cancelled".into())
        );
    }
}
