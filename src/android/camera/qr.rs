//! The `libmobile-qr` thread (plan 5.1): it decodes the newest grey frame a
//! QR session hands it and posts the [`Scan`] back to the camera thread.
//! The camera thread puts each frame into a one-slot cell (an older one not
//! yet taken is dropped), so decoding never blocks the camera or the app,
//! and a slow decode only lowers the rate.

use std::sync::{Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use super::worker::{Msg, post_if_running};
use crate::camera::SessionId;
use crate::camera::qr::control::Controller;
use crate::camera::qr::scan::{GreyFrame, Rqrr, analyse};
use crate::camera::qr::{Dedup, QR_ANALYSIS_HZ, QR_DECODE_BUDGET_MS, QrOptions};

/// One frame to decode, tagged with the session and the streams it came
/// from (the camera thread drops a scan of older streams).
pub(super) struct Job {
    pub session: SessionId,
    pub epoch: u64,
    pub grey: GreyFrame,
}

static SLOT: Mutex<Option<Job>> = Mutex::new(None);
static READY: Condvar = Condvar::new();
static START: Once = Once::new();

/// Hand the decoder the newest frame, starting its thread on first use.
pub(super) fn submit(job: Job) {
    START.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("libmobile-qr".into())
            .spawn(run);
        if let Err(err) = spawned {
            log::error!("libmobile: the QR thread could not start: {err}");
        }
    });
    *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(job);
    READY.notify_one();
}

/// Decodes per median-time log line.
const TIMING_WINDOW: usize = 64;

fn run() {
    log::info!("libmobile: QR thread start");
    let mut decoder = Rqrr;
    let mut times: Vec<Duration> = Vec::with_capacity(TIMING_WINDOW);
    loop {
        let job = {
            let mut slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(job) = slot.take() {
                    break job;
                }
                slot = READY.wait(slot).unwrap_or_else(|e| e.into_inner());
            }
        };
        let started = Instant::now();
        let scan = analyse(&mut decoder, &job.grey);
        times.push(started.elapsed());
        if times.len() == TIMING_WINDOW {
            times.sort();
            let median = times[TIMING_WINDOW / 2];
            let (w, h) = (job.grey.width, job.grey.height);
            if median > Duration::from_millis(QR_DECODE_BUDGET_MS) {
                log::warn!(
                    "libmobile: QR decode median {} ms at {w} × {h}, over the {QR_DECODE_BUDGET_MS} ms budget",
                    median.as_millis()
                );
            } else {
                log::debug!(
                    "libmobile: QR decode median {} ms at {w} × {h}",
                    median.as_millis()
                );
            }
            times.clear();
        }
        post_if_running(Msg::QrScan {
            session: job.session,
            epoch: job.epoch,
            scan,
        });
    }
}

/// A QR session's state on the camera thread.
pub(super) struct QrState {
    pub options: QrOptions,
    /// Made when the camera opens (it needs the camera's limits), and made
    /// again on every reopen: auto zoom and the ladder start over.
    pub control: Option<Controller>,
    pub dedup: Dedup,
    /// When the last frame went to the decoder.
    pub last_submit: Option<Instant>,
}

impl QrState {
    pub(super) fn new(options: QrOptions) -> Self {
        Self {
            options,
            control: None,
            dedup: Dedup::default(),
            last_submit: None,
        }
    }

    /// Whether a frame at `now` should go to the decoder
    /// (`QR_ANALYSIS_HZ` at most); records it if so.
    pub(super) fn due(&mut self, now: Instant) -> bool {
        if self.control.is_none() {
            return false;
        }
        // A few ms of slack, so a 30 fps stream is decoded every second
        // frame rather than every third.
        let gap = Duration::from_secs(1) / QR_ANALYSIS_HZ;
        let slack = Duration::from_millis(5);
        if self
            .last_submit
            .is_some_and(|at| now.saturating_duration_since(at) + slack < gap)
        {
            return false;
        }
        self.last_submit = Some(now);
        true
    }
}
