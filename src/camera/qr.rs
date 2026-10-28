//! **QR scanning** (feature `camera-qr`): the platform-free half of
//! `android::camera::start_qr`.
//!
//! A QR session is a camera session whose analysis frames also go to the
//! `libmobile-qr` thread, which reads codes with rqrr (newest frame only;
//! older ones are dropped). Codes arrive as
//! [`CameraEvent::Qr`](super::CameraEvent::Qr). While a code is found but
//! not read:
//!
//! * **auto zoom** zooms in toward modules of `QR_TARGET_MODULE_PX`, upward
//!   from the app's own zoom only, and pauses after each `set_zoom` /
//!   `zoom_by` (a pinch wins). On API 30+ it ends with fine steps of the
//!   continuous zoom ratio; on API 29 it keeps coarse steps;
//! * **exposure for screens** meters on the code and steps compensation down
//!   when the code looks like an overexposed screen ([`ScreenExposure`]);
//! * autofocus is triggered on the code.
//!
//! A payload is never logged past its length: it may be a pairing secret.

// The decoder, the controller and the constants are used by the Android
// camera thread; on other hosts only by the tests.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::time::{Duration, Instant};

use super::{Facing, PreviewOptions};

pub(crate) mod control;
pub(crate) mod scan;

// ── Tunable constants (plan section 11) ─────────────────────────────────────

/// Most decodes per second.
pub(crate) const QR_ANALYSIS_HZ: u32 = 15;
/// Median decode time (ms) above which the analysis stream should shrink;
/// logged, not acted on.
pub(crate) const QR_DECODE_BUDGET_MS: u64 = 40;
/// `Continuous`: the same payload is reported again after this long without it.
pub(crate) const QR_REPEAT_MS: u64 = 2000;
/// The module size auto zoom aims for, px.
pub(crate) const QR_TARGET_MODULE_PX: f32 = 4.0;
/// The largest zoom factor per window.
pub(crate) const QR_ZOOM_STEP_MAX: f32 = 1.6;
/// The smallest coarse zoom step (plan 6.3 step 2: "only if k >= 1.1").
pub(crate) const QR_ZOOM_MIN_STEP: f32 = 1.1;
/// Fine zoom (API 30+): the smallest step it makes.
pub(crate) const QR_FINE_ZOOM_MIN: f32 = 1.02;
/// Fine zoom aims this much above `QR_TARGET_MODULE_PX`, so a measurement a
/// little short still lands on a readable size.
pub(crate) const QR_FINE_AIM: f32 = 1.05;
/// The code must stay within this part of the frame.
pub(crate) const QR_FRAME_MARGIN: f32 = 0.9;
/// Auto zoom above the app's zoom, at most.
pub(crate) const QR_AUTO_MAX_FACTOR: f32 = 3.0;
/// Auto zoom never beyond this ratio.
pub(crate) const QR_AUTO_MAX_RATIO: f32 = 8.0;
/// Auto zoom paused after the app sets the zoom, ms.
pub(crate) const QR_USER_HOLD_MS: u64 = 1500;
/// Wait after a zoom change, ms.
pub(crate) const QR_ZOOM_SETTLE_MS: u64 = 300;
/// Least time between AF triggers, ms.
pub(crate) const QR_AF_RETRIGGER_MS: u64 = 1000;
/// Misses before stepping back (about 1.3 s at 15 Hz).
pub(crate) const QR_LOST_FRAMES: u32 = 20;
/// A luma sample at or above this counts as saturated.
pub(crate) const QR_SAT_LEVEL: u8 = 245;
/// The fraction of saturated samples that means overexposed.
pub(crate) const QR_SAT_FRACTION: f32 = 0.08;
/// The mean luma that means overexposed.
pub(crate) const QR_SAT_MEAN: f32 = 200.0;
/// One exposure rung, EV.
pub(crate) const QR_EV_STEP: f32 = 1.0;
/// The lowest compensation the ladder uses, EV.
pub(crate) const QR_EV_FLOOR: f32 = -2.0;
/// Wait after an exposure change, ms (less if AE converges).
pub(crate) const QR_AE_SETTLE_MS: u64 = 400;
/// Failing windows at the floor before bracketing.
pub(crate) const QR_BRACKET_AFTER: u32 = 3;
/// Windows of bracketing before giving up.
pub(crate) const QR_BRACKET_WINDOWS: u32 = 6;
/// Modules at least this big (px) that fail to decode get the 2 × 2 retry.
pub(crate) const QR_RETRY_MODULE_PX: f32 = 8.0;

/// When a QR session stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QrMode {
    /// Stop after the first code (`Closed { reason: None }` follows `Qr`).
    #[default]
    Once,
    /// Keep scanning; the same payload is reported again only after two
    /// seconds without it.
    Continuous,
}

/// Exposure for codes shown on screens (a TV, a monitor, a phone).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenExposure {
    /// Meter on the code and step exposure down when it looks like a bright
    /// screen; with nothing found at all, test the centre of the frame.
    #[default]
    Auto,
    /// Never adjust exposure.
    Off,
    /// Start 1 EV darker (half the −2 EV floor), metered on the centre of the
    /// frame: for apps that know the code is on a screen (a TV pairing code).
    Screen,
}

/// How [`start_qr`](crate::android::camera::start_qr) scans.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct QrOptions {
    /// Default `Back`.
    pub facing: Facing,
    /// Default `Once`.
    pub mode: QrMode,
    /// Zoom in toward a code too small to read. Default `true`.
    pub auto_zoom: bool,
    /// Default `Auto`.
    pub exposure: ScreenExposure,
    /// Preview frames for a viewfinder (`latest_preview`); `None` for none.
    /// Default `Some(PreviewOptions::default())`.
    pub preview: Option<PreviewOptions>,
}

impl Default for QrOptions {
    fn default() -> Self {
        Self {
            facing: Facing::Back,
            mode: QrMode::Once,
            auto_zoom: true,
            exposure: ScreenExposure::Auto,
            preview: Some(PreviewOptions::default()),
        }
    }
}

/// A code that was read.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct QrCode {
    /// The payload bytes as decoded (byte, alphanumeric, numeric and Kanji
    /// segments; ECI is not interpreted).
    pub bytes: Vec<u8>,
    /// `bytes` as UTF-8, when they are.
    pub text: Option<String>,
    /// The code's top-left, top-right, bottom-right and bottom-left corners
    /// in the upright frame, as fractions 0.0..=1.0.
    pub corners: [(f32, f32); 4],
    /// QR version 1..=40.
    pub version: u8,
}

impl QrCode {
    /// From a decoder's result: corners in pixels of a `size` frame as the
    /// sensor delivers it, turned upright by `quarter_turns`.
    pub(crate) fn new(
        bytes: Vec<u8>,
        corners: [(f32, f32); 4],
        version: u8,
        size: (usize, usize),
        quarter_turns: u8,
    ) -> Self {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let corners = corners.map(|(x, y)| {
            let (x, y) = scan::turn_point((x / w, y / h), quarter_turns);
            (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
        });
        Self {
            text: String::from_utf8(bytes.clone()).ok(),
            bytes,
            corners,
            version,
        }
    }
}

/// Which payloads to report (plan 4.4): `Once` reports the first;
/// `Continuous` reports a payload again only after `QR_REPEAT_MS` without
/// it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dedup {
    seen: Vec<(Vec<u8>, Instant)>,
}

impl Dedup {
    /// Whether to report `bytes` seen at `now`.
    pub(crate) fn admit(&mut self, bytes: &[u8], now: Instant) -> bool {
        let repeat = Duration::from_millis(QR_REPEAT_MS);
        self.seen
            .retain(|(_, at)| now.saturating_duration_since(*at) < repeat);
        match self.seen.iter_mut().find(|(b, _)| b == bytes) {
            Some((_, at)) => {
                *at = now;
                false
            }
            None => {
                self.seen.push((bytes.to_vec(), now));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let o = QrOptions::default();
        assert_eq!(o.facing, Facing::Back);
        assert_eq!(o.mode, QrMode::Once);
        assert!(o.auto_zoom);
        assert_eq!(o.exposure, ScreenExposure::Auto);
        assert_eq!(o.preview, Some(PreviewOptions::default()));
    }

    #[test]
    fn continuous_reports_again_only_after_a_gap() {
        let mut d = Dedup::default();
        let t = Instant::now();
        let at = |ms| t + Duration::from_millis(ms);
        assert!(d.admit(b"a", at(0)));
        assert!(!d.admit(b"a", at(100)));
        assert!(d.admit(b"b", at(150)));
        // Seen again and again: never reported while it stays in view.
        for ms in (500..6000).step_by(500) {
            assert!(!d.admit(b"a", at(ms)));
        }
        // Gone for QR_REPEAT_MS: reported again.
        assert!(d.admit(b"a", at(5500 + QR_REPEAT_MS)));
        assert!(d.admit(b"b", at(5500 + QR_REPEAT_MS)));
    }

    #[test]
    fn codes_carry_upright_fractions_and_text() {
        let corners = [(320.0, 0.0), (640.0, 0.0), (640.0, 360.0), (320.0, 360.0)];
        let code = QrCode::new(b"hi".to_vec(), corners, 1, (640, 360), 1);
        assert_eq!(code.text.as_deref(), Some("hi"));
        // A quarter turn: the sensor's top-left (0.5, 0) lands at (1, 0.5).
        assert_eq!(code.corners[0], (1.0, 0.5));
        assert_eq!(code.corners[2], (0.0, 1.0));
        let binary = QrCode::new(vec![0xff, 0xfe], corners, 2, (640, 360), 0);
        assert_eq!(binary.text, None);
    }

    #[test]
    fn constants_hold_together() {
        // 20 misses at 15 Hz is about 1.3 s.
        let lost_ms = QR_LOST_FRAMES as u64 * 1000 / QR_ANALYSIS_HZ as u64;
        assert!((1200..=1400).contains(&lost_ms));
        const {
            assert!(QR_FINE_ZOOM_MIN < QR_ZOOM_MIN_STEP && QR_ZOOM_MIN_STEP < QR_ZOOM_STEP_MAX);
            assert!(1.0 <= QR_AUTO_MAX_FACTOR && QR_AUTO_MAX_FACTOR <= QR_AUTO_MAX_RATIO);
            assert!(QR_EV_FLOOR < 0.0 && QR_EV_STEP > 0.0);
        }
    }
}
