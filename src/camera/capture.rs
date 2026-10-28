//! One still frame (feature `camera-capture`): the options and the frame,
//! and the capture state machine the camera thread drives (pure, so its
//! timeout, cancel and interruption rules are host-tested).
//!
//! Captures are served one at a time, oldest first. A capture's timeout
//! starts when the camera opens for it, not while the permission dialog is
//! up or while an older capture runs.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::{CAPTURE_3A_TIMEOUT_MS, CAPTURE_TIMEOUT_MS, CameraError, CaptureId, Facing};

/// The pixels a capture returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameFormat {
    /// The luma plane, one byte per pixel, rows packed.
    Gray8,
    /// RGBA, 4 bytes per pixel, rows packed, converted from YUV (BT.601
    /// full range).
    Rgba8,
    /// The camera's hardware JPEG.
    Jpeg,
}

/// How large a capture is.
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

/// What `capture_frame` takes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CaptureOptions {
    /// Default `Back`.
    pub facing: Facing,
    /// Default `Rgba8`.
    pub format: FrameFormat,
    /// Default `Preview`.
    pub resolution: Resolution,
    /// Rotate to the display's orientation (Gray8 and Rgba8 rotated on the
    /// camera thread; Jpeg through `JPEG_ORIENTATION`). Default `true`.
    pub upright: bool,
    /// From the camera opening to the frame. Default 5 s.
    pub timeout: Duration,
    /// 1..=100, Jpeg only. Default 90.
    pub jpeg_quality: u8,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            facing: Facing::Back,
            format: FrameFormat::Rgba8,
            resolution: Resolution::Preview,
            upright: true,
            timeout: Duration::from_millis(CAPTURE_TIMEOUT_MS),
            jpeg_quality: 90,
        }
    }
}

/// One captured frame.
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

impl Frame {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android camera thread.
    pub(crate) fn new(
        (width, height): (u32, u32),
        format: FrameFormat,
        data: Vec<u8>,
        quarter_turns: u8,
        facing: Facing,
        timestamp_ns: i64,
    ) -> Self {
        Self {
            width,
            height,
            format,
            data,
            quarter_turns,
            facing,
            timestamp_ns,
        }
    }
}

/// Where a capture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Waiting for the permission answer.
    Permission,
    /// Granted: waiting for its turn, the camera, or a resume.
    Queued,
    /// The camera is open for it: waiting for 3A (or the 3A timeout).
    Settling { since: Instant },
    /// The still request is sent, or the next analysis frame is awaited.
    Requested,
}

/// One capture in the queue.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub id: CaptureId,
    pub options: CaptureOptions,
    pub phase: Phase,
    /// Set when the camera first opens for it.
    pub deadline: Option<Instant>,
}

impl Pending {
    fn in_progress(&self) -> bool {
        matches!(self.phase, Phase::Settling { .. } | Phase::Requested)
    }
}

/// The captures not yet answered, oldest first. The *active* one is the
/// oldest that is not waiting for the permission answer.
#[derive(Debug, Default)]
pub(crate) struct Captures {
    list: VecDeque<Pending>,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Driven by the Android camera thread.
impl Captures {
    pub(crate) fn push(&mut self, id: CaptureId, options: CaptureOptions, granted: bool) {
        let phase = if granted {
            Phase::Queued
        } else {
            Phase::Permission
        };
        self.list.push_back(Pending {
            id,
            options,
            phase,
            deadline: None,
        });
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// CAMERA granted: `ids` may run.
    pub(crate) fn grant(&mut self, ids: &[CaptureId]) {
        for p in self.list.iter_mut().filter(|p| ids.contains(&p.id)) {
            if p.phase == Phase::Permission {
                p.phase = Phase::Queued;
            }
        }
    }

    /// CAMERA refused: `ids` end with `error`.
    pub(crate) fn deny(
        &mut self,
        ids: &[CaptureId],
        error: &CameraError,
    ) -> Vec<(CaptureId, CameraError)> {
        let mut out = Vec::new();
        self.list.retain(|p| {
            if ids.contains(&p.id) && p.phase == Phase::Permission {
                out.push((p.id, error.clone()));
                false
            } else {
                true
            }
        });
        out
    }

    pub(crate) fn active(&self) -> Option<&Pending> {
        self.list.iter().find(|p| p.phase != Phase::Permission)
    }

    /// The camera is open for the active capture: 3A starts, and so does its
    /// timeout (the first time only; a reconfigure keeps the deadline).
    pub(crate) fn opened(&mut self, now: Instant) {
        if let Some(p) = self.list.iter_mut().find(|p| p.phase != Phase::Permission) {
            p.phase = Phase::Settling { since: now };
            p.deadline.get_or_insert(now + p.options.timeout);
        }
    }

    /// Whether the active capture should be taken now: settling, and 3A
    /// settled or `CAPTURE_3A_TIMEOUT_MS` passed.
    pub(crate) fn ready_to_shoot(&self, now: Instant, settled: bool) -> bool {
        match self.active().map(|p| p.phase) {
            Some(Phase::Settling { since }) => {
                settled || now >= since + Duration::from_millis(CAPTURE_3A_TIMEOUT_MS)
            }
            _ => false,
        }
    }

    /// The active capture's request is sent.
    pub(crate) fn requested(&mut self) {
        if let Some(p) = self.list.iter_mut().find(|p| p.phase != Phase::Permission) {
            p.phase = Phase::Requested;
        }
    }

    /// Remove the active capture (its frame or its error is delivered).
    pub(crate) fn finish(&mut self) -> Option<Pending> {
        let i = self
            .list
            .iter()
            .position(|p| p.phase != Phase::Permission)?;
        self.list.remove(i)
    }

    /// Remove `id` (cancelled). `Some(true)` if it was in progress, so the
    /// camera must be released or reconfigured; `None` if unknown.
    pub(crate) fn cancel(&mut self, id: CaptureId) -> Option<bool> {
        let i = self.list.iter().position(|p| p.id == id)?;
        self.list.remove(i).map(|p| p.in_progress())
    }

    /// Remove every capture whose deadline has passed.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<CaptureId> {
        let mut out = Vec::new();
        self.list.retain(|p| match p.deadline {
            Some(deadline) if now >= deadline => {
                out.push(p.id);
                false
            }
            _ => true,
        });
        out
    }

    /// The activity paused: captures in progress end; queued ones and those
    /// waiting for the permission answer stay (they start on resume).
    pub(crate) fn interrupt(&mut self) -> Vec<CaptureId> {
        let mut out = Vec::new();
        self.list.retain(|p| {
            if p.in_progress() {
                out.push(p.id);
                false
            } else {
                true
            }
        });
        out
    }

    /// The camera closed under the active capture (a reconfigure): back to
    /// queued, keeping its deadline.
    pub(crate) fn requeue(&mut self) {
        for p in self.list.iter_mut().filter(|p| p.in_progress()) {
            p.phase = Phase::Queued;
        }
    }

    /// Every id, for a teardown.
    pub(crate) fn drain(&mut self) -> Vec<CaptureId> {
        self.list.drain(..).map(|p| p.id).collect()
    }

    /// When the thread must next wake for a deadline or the 3A timeout.
    pub(crate) fn next_wakeup(&self) -> Option<Instant> {
        let deadlines = self.list.iter().filter_map(|p| p.deadline);
        let settle = self.active().and_then(|p| match p.phase {
            Phase::Settling { since } => Some(since + Duration::from_millis(CAPTURE_3A_TIMEOUT_MS)),
            _ => None,
        });
        deadlines.chain(settle).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> CaptureId {
        CaptureId(n)
    }

    fn opts(timeout_ms: u64) -> CaptureOptions {
        CaptureOptions {
            timeout: Duration::from_millis(timeout_ms),
            ..CaptureOptions::default()
        }
    }

    #[test]
    fn defaults() {
        let o = CaptureOptions::default();
        assert_eq!(o.facing, Facing::Back);
        assert_eq!(o.format, FrameFormat::Rgba8);
        assert_eq!(o.resolution, Resolution::Preview);
        assert!(o.upright);
        assert_eq!(o.timeout, Duration::from_secs(5));
        assert_eq!(o.jpeg_quality, 90);
    }

    #[test]
    fn the_timeout_starts_when_the_camera_opens() {
        let t0 = Instant::now();
        let mut c = Captures::default();
        c.push(id(1), opts(100), false);
        // Waiting for the permission answer: no deadline yet.
        assert_eq!(c.next_wakeup(), None);
        assert!(c.expire(t0 + Duration::from_secs(60)).is_empty());
        assert!(c.active().is_none());
        c.grant(&[id(1)]);
        let open = t0 + Duration::from_secs(10);
        c.opened(open);
        assert!(c.expire(open + Duration::from_millis(99)).is_empty());
        assert_eq!(c.expire(open + Duration::from_millis(100)), [id(1)]);
        assert!(c.is_empty());
    }

    #[test]
    fn three_a_settles_or_times_out() {
        let t0 = Instant::now();
        let mut c = Captures::default();
        c.push(id(1), opts(5000), true);
        assert!(!c.ready_to_shoot(t0, true), "not open yet");
        c.opened(t0);
        assert!(!c.ready_to_shoot(t0, false));
        assert!(c.ready_to_shoot(t0, true));
        let late = t0 + Duration::from_millis(CAPTURE_3A_TIMEOUT_MS);
        assert!(c.ready_to_shoot(late, false));
        assert_eq!(c.next_wakeup(), Some(late));
        c.requested();
        assert!(!c.ready_to_shoot(late, true));
        assert_eq!(c.next_wakeup(), Some(t0 + Duration::from_secs(5)));
        assert_eq!(c.finish().map(|p| p.id), Some(id(1)));
        assert!(c.is_empty());
    }

    #[test]
    fn cancel_reports_whether_the_camera_was_busy() {
        let t0 = Instant::now();
        let mut c = Captures::default();
        c.push(id(1), opts(5000), true);
        c.push(id(2), opts(5000), true);
        c.push(id(3), opts(5000), false);
        c.opened(t0);
        assert_eq!(c.cancel(id(3)), Some(false));
        assert_eq!(c.cancel(id(2)), Some(false));
        assert_eq!(c.cancel(id(1)), Some(true));
        assert_eq!(c.cancel(id(1)), None, "answered once");
    }

    #[test]
    fn pause_interrupts_only_captures_in_progress() {
        let t0 = Instant::now();
        let mut c = Captures::default();
        c.push(id(1), opts(5000), true);
        c.push(id(2), opts(5000), true);
        c.push(id(3), opts(5000), false);
        c.opened(t0);
        assert_eq!(c.interrupt(), [id(1)]);
        // The next one becomes active on resume; the third still waits for
        // its permission answer.
        assert_eq!(c.active().map(|p| p.id), Some(id(2)));
        assert_eq!(c.active().map(|p| p.phase), Some(Phase::Queued));
        assert_eq!(c.drain(), [id(2), id(3)]);
    }

    #[test]
    fn denial_ends_only_the_waiting_captures() {
        let mut c = Captures::default();
        c.push(id(1), opts(5000), true);
        c.push(id(2), opts(5000), false);
        let err = CameraError::PermissionDenied {
            settings_only: true,
        };
        assert_eq!(c.deny(&[id(1), id(2)], &err), [(id(2), err.clone())]);
        assert_eq!(c.active().map(|p| p.id), Some(id(1)));
    }

    #[test]
    fn a_reconfigure_keeps_the_deadline() {
        let t0 = Instant::now();
        let mut c = Captures::default();
        c.push(id(1), opts(1000), true);
        c.opened(t0);
        c.requeue();
        assert_eq!(c.active().map(|p| p.phase), Some(Phase::Queued));
        c.opened(t0 + Duration::from_millis(600));
        assert_eq!(c.expire(t0 + Duration::from_millis(1000)), [id(1)]);
    }
}
