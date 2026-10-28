//! Auto zoom and exposure for screens (plan 6.2, 6.3, 7, and the fine zoom
//! of the 2026-10-28 14:46 EDT amendment): a pure state machine. The camera
//! thread feeds it each analysed frame ([`Scan`]) and the AE state of each
//! capture result; it answers with at most one [`Change`] per adjustment
//! window, and the camera thread applies what [`Controller::auto`],
//! [`Controller::ev`], [`Controller::ae_region`] and
//! [`Controller::af_region`] then say.
//!
//! One kind of change per window, in the order zoom, exposure, focus, so the
//! effect of each is seen before the next. Nothing changes while the last
//! change settles.

use std::time::{Duration, Instant};

use super::ScreenExposure;
use super::scan::{CENTRE, FracBox, Luma, Measure, Scan, Target};
use super::{
    QR_AE_SETTLE_MS, QR_AF_RETRIGGER_MS, QR_AUTO_MAX_FACTOR, QR_AUTO_MAX_RATIO, QR_BRACKET_AFTER,
    QR_BRACKET_WINDOWS, QR_EV_FLOOR, QR_EV_STEP, QR_FINE_AIM, QR_FINE_ZOOM_MIN, QR_FRAME_MARGIN,
    QR_LOST_FRAMES, QR_TARGET_MODULE_PX, QR_USER_HOLD_MS, QR_ZOOM_MIN_STEP, QR_ZOOM_SETTLE_MS,
    QR_ZOOM_STEP_MAX,
};

/// What the open camera offers the controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Limits {
    /// The app's zoom ratio (`set_zoom` / `zoom_by`).
    pub user: f32,
    /// The device's largest zoom ratio.
    pub max_ratio: f32,
    /// Zoom is the continuous `CONTROL_ZOOM_RATIO` (API 30+): auto zoom ends
    /// with fine steps. On the crop region (API 29) it keeps coarse steps.
    pub fine: bool,
    /// `ACAMERA_CONTROL_MAX_REGIONS` allows an AE region.
    pub ae_regions: bool,
    /// `ACAMERA_CONTROL_MAX_REGIONS` allows an AF region.
    pub af_regions: bool,
    /// The camera has autofocus.
    pub has_af: bool,
    /// The lowest compensation the ladder uses, in EV (≤ 0): `QR_EV_FLOOR`,
    /// or the device's lowest if that is higher; 0 with no compensation.
    pub ev_floor: f32,
}

impl Limits {
    /// The largest auto factor: `QR_AUTO_MAX_FACTOR` above the app's zoom,
    /// and never beyond `QR_AUTO_MAX_RATIO` or the device's range.
    pub(crate) fn auto_max(&self) -> f32 {
        let cap = QR_AUTO_MAX_RATIO.min(self.max_ratio);
        (cap / self.user.max(1.0)).clamp(1.0, QR_AUTO_MAX_FACTOR)
    }
}

/// `max(QR_EV_FLOOR, the device's lowest)` from
/// `ACAMERA_CONTROL_AE_COMPENSATION_RANGE` (device steps) and
/// `ACAMERA_CONTROL_AE_COMPENSATION_STEP` (EV per step).
pub(crate) fn ev_floor(range: (i32, i32), step_ev: f32) -> f32 {
    if !(step_ev.is_finite() && step_ev > 0.0) || range.0 >= 0 {
        return 0.0;
    }
    QR_EV_FLOOR.max(range.0 as f32 * step_ev).min(0.0)
}

/// `ev` in device steps, rounded and clamped to the range.
pub(crate) fn ev_steps(ev: f32, range: (i32, i32), step_ev: f32) -> i32 {
    if !(step_ev.is_finite() && step_ev > 0.0) || range.0 > range.1 {
        return 0;
    }
    ((ev / step_ev).round() as i32).clamp(range.0, range.1)
}

/// The one change a window makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// [`Controller::auto`] changed.
    Zoom,
    /// [`Controller::ev`] or [`Controller::ae_region`] changed.
    Exposure,
    /// [`Controller::af_region`] changed; send this AF trigger once.
    Focus(AfTrigger),
}

/// `ACAMERA_CONTROL_AF_TRIGGER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfTrigger {
    /// Focus on the region now.
    Start,
    /// Back to continuous focus over the whole field.
    Cancel,
}

/// The exposure ladder's rung (plan 7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ladder {
    /// Nothing done yet.
    Start,
    /// Metering on the code (rung 1).
    Metered,
    /// Stepped down, not yet at the floor (rung 2).
    Stepping,
    /// At the floor, counting failing windows (before rung 3).
    Floor { fails: u32 },
    /// Bracketing between the floor and half of it (rung 3).
    Bracket { windows: u32 },
    /// At the floor for good (rung 4), until the code is lost or decodes.
    GaveUp,
}

/// What started the ladder: a found code, or the central-area test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Code,
    Centre,
}

/// What a window waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Settle {
    /// A zoom change: `QR_ZOOM_SETTLE_MS`.
    Zoom(Instant),
    /// An exposure change: `QR_AE_SETTLE_MS`, or less once AE has left
    /// `CONVERGED`/`LOCKED` and come back.
    Exposure {
        until: Instant,
        left: bool,
        done: bool,
    },
    /// A window with no change (counting failures at the floor).
    Window(Instant),
}

/// One rung's outcome.
enum Rung {
    /// The exposure changed.
    Changed,
    /// No change, but the window is used up.
    Held,
    /// Nothing to do: the next kind of change may go.
    Pass,
}

/// The auto zoom and exposure state of one QR session.
#[derive(Clone, Debug)]
pub(crate) struct Controller {
    mode: ScreenExposure,
    auto_zoom: bool,
    /// The factor on top of the app's zoom, >= 1.
    auto: f32,
    /// Exposure compensation, EV.
    ev: f32,
    ae_region: Option<FracBox>,
    af_region: Option<FracBox>,
    ladder: Ladder,
    origin: Option<Origin>,
    /// The mode's starting exposure, which loss returns to.
    base: (f32, Option<FracBox>, Ladder),
    misses: u32,
    /// A code has been found since the session opened.
    seen: bool,
    settle: Option<Settle>,
    /// Auto zoom is paused until then (the app set the zoom).
    hold_until: Option<Instant>,
    last_af: Option<Instant>,
    af_locked: bool,
}

const fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

impl Controller {
    /// A controller for a session that has just opened.
    /// `ScreenExposure::Screen` starts at half the floor with the centre
    /// metered (rung 2, plan 7.3).
    pub(crate) fn new(mode: ScreenExposure, auto_zoom: bool, limits: &Limits) -> Self {
        let base = match mode {
            ScreenExposure::Screen => (
                limits.ev_floor / 2.0,
                limits.ae_regions.then_some(CENTRE),
                Ladder::Stepping,
            ),
            ScreenExposure::Auto | ScreenExposure::Off => (0.0, None, Ladder::Start),
        };
        Self {
            mode,
            auto_zoom,
            auto: 1.0,
            ev: base.0,
            ae_region: base.1,
            af_region: None,
            ladder: base.2,
            origin: None,
            base,
            misses: 0,
            seen: false,
            settle: None,
            hold_until: None,
            last_af: None,
            af_locked: false,
        }
    }

    /// The zoom factor on top of the app's zoom (>= 1).
    pub(crate) fn auto(&self) -> f32 {
        self.auto
    }

    /// Exposure compensation, EV.
    pub(crate) fn ev(&self) -> f32 {
        self.ev
    }

    /// The AE metering region, in analysis-frame fractions.
    pub(crate) fn ae_region(&self) -> Option<FracBox> {
        self.ae_region
    }

    /// The AF region, in analysis-frame fractions.
    pub(crate) fn af_region(&self) -> Option<FracBox> {
        self.af_region
    }

    #[cfg(test)]
    pub(crate) fn ladder(&self) -> Ladder {
        self.ladder
    }

    /// The app set the zoom (`set_zoom` / `zoom_by`): auto zoom drops back
    /// to the app's zoom and pauses for `QR_USER_HOLD_MS` after this call.
    /// Exposure is left as it is. Returns whether the auto factor changed.
    pub(crate) fn app_zoomed(&mut self, now: Instant) -> bool {
        self.hold_until = Some(now + ms(QR_USER_HOLD_MS));
        let changed = self.auto != 1.0;
        self.auto = 1.0;
        changed
    }

    fn held(&self, now: Instant) -> bool {
        self.hold_until.is_some_and(|until| now < until)
    }

    /// A capture result's `ACAMERA_CONTROL_AE_STATE`: ends an exposure
    /// window early once AE has moved and settled again.
    pub(crate) fn ae_state(&mut self, ae: Option<u8>) {
        if let (Some(Settle::Exposure { left, done, .. }), Some(state)) = (&mut self.settle, ae) {
            // CONVERGED (2) or LOCKED (3).
            if matches!(state, 2 | 3) {
                *done |= *left;
            } else {
                *left = true;
            }
        }
    }

    fn settled(&self, now: Instant) -> bool {
        match self.settle {
            None => true,
            Some(Settle::Zoom(until) | Settle::Window(until)) => now >= until,
            Some(Settle::Exposure { until, done, .. }) => done || now >= until,
        }
    }

    fn exposure_changed(&mut self, now: Instant) -> Rung {
        self.settle = Some(Settle::Exposure {
            until: now + ms(QR_AE_SETTLE_MS),
            left: false,
            done: false,
        });
        Rung::Changed
    }

    /// The newest analysed frame. At most one change, after the last one has
    /// settled (plan 6.3).
    pub(crate) fn step(&mut self, scan: &Scan, now: Instant, limits: &Limits) -> Option<Change> {
        match scan.target {
            Target::Missed => self.misses = self.misses.saturating_add(1),
            Target::Decoded(_) | Target::Found(_) => {
                self.misses = 0;
                self.seen = true;
            }
        }
        if !self.settled(now) {
            return None;
        }
        self.settle = None;
        match scan.target {
            // Read: keep zoom and exposure while the code stays in view.
            Target::Decoded(_) => None,
            Target::Found(m) => self.found(&m, scan.luma, now, limits),
            Target::Missed => self.missed(scan.luma, now, limits),
        }
    }

    fn found(
        &mut self,
        m: &Measure,
        luma: Option<Luma>,
        now: Instant,
        limits: &Limits,
    ) -> Option<Change> {
        // 2. Too small: zoom in (6.3), coarse steps, then fine ones on API 30+.
        if self.auto_zoom
            && m.module_px < QR_TARGET_MODULE_PX
            && let Some(k) = self.zoom_factor(m, limits)
        {
            if self.held(now) {
                return None; // Never fight a pinch in progress.
            }
            self.auto = (self.auto * k).min(limits.auto_max());
            self.settle = Some(Settle::Zoom(now + ms(QR_ZOOM_SETTLE_MS)));
            return Some(Change::Zoom);
        }
        // 3. Big enough (or as big as it gets) and overexposed: the ladder.
        if self.mode != ScreenExposure::Off && luma.is_some_and(|l| l.overexposed()) {
            match self.ladder_step(m.bbox, now, limits) {
                Rung::Changed => {
                    self.origin = Some(Origin::Code);
                    return Some(Change::Exposure);
                }
                Rung::Held => return None,
                Rung::Pass => {}
            }
        }
        // 4. Not bright (or the ladder is done): focus on it.
        self.focus(m.bbox, now, limits)
    }

    /// The factor to zoom in by now, if any (plan 6.3 step 2 and the fine
    /// zoom of the amendment).
    fn zoom_factor(&self, m: &Measure, limits: &Limits) -> Option<f32> {
        let cap = limits.auto_max() / self.auto;
        // Zoom is centred: the code's farthest edge from the centre, per
        // axis, must stay within the margin.
        let reach = (m.offset.0 + m.half.0).max(m.offset.1 + m.half.1);
        let k_fit = if reach > 0.0 {
            QR_FRAME_MARGIN / reach
        } else {
            f32::INFINITY
        };
        let want = QR_TARGET_MODULE_PX / m.module_px.max(0.01);
        let coarse = want.min(QR_ZOOM_STEP_MAX).min(k_fit).min(cap);
        if coarse >= QR_ZOOM_MIN_STEP {
            return Some(coarse);
        }
        if limits.fine {
            // The continuous ratio: finish on the ratio that gives a readable
            // module size, aiming a little above the target.
            let fine = (want * QR_FINE_AIM)
                .min(QR_ZOOM_STEP_MAX)
                .min(k_fit)
                .min(cap);
            if fine >= QR_FINE_ZOOM_MIN {
                return Some(fine);
            }
        }
        None
    }

    /// The next rung of the exposure ladder (plan 7.2), metering on `area`.
    fn ladder_step(&mut self, area: FracBox, now: Instant, limits: &Limits) -> Rung {
        let floor = limits.ev_floor;
        match self.ladder {
            Ladder::Start => {
                self.ladder = Ladder::Metered;
                if limits.ae_regions {
                    self.ae_region = Some(area);
                    return self.exposure_changed(now);
                }
                self.ladder_step(area, now, limits)
            }
            Ladder::Metered | Ladder::Stepping => {
                if floor >= 0.0 {
                    self.ladder = Ladder::GaveUp; // No compensation to step.
                    return Rung::Pass;
                }
                if limits.ae_regions {
                    self.ae_region = Some(area);
                }
                self.ev = (self.ev - QR_EV_STEP).max(floor);
                self.ladder = if self.ev <= floor {
                    Ladder::Floor { fails: 0 }
                } else {
                    Ladder::Stepping
                };
                self.exposure_changed(now)
            }
            Ladder::Floor { fails } if fails >= QR_BRACKET_AFTER => {
                self.ladder = Ladder::Bracket { windows: 1 };
                self.ev = floor / 2.0;
                self.exposure_changed(now)
            }
            Ladder::Floor { fails } => {
                self.ladder = Ladder::Floor { fails: fails + 1 };
                self.settle = Some(Settle::Window(now + ms(QR_AE_SETTLE_MS)));
                Rung::Held
            }
            Ladder::Bracket { windows } if windows >= QR_BRACKET_WINDOWS => {
                self.ladder = Ladder::GaveUp;
                if self.ev != floor {
                    self.ev = floor;
                    return self.exposure_changed(now);
                }
                Rung::Pass
            }
            Ladder::Bracket { windows } => {
                self.ladder = Ladder::Bracket {
                    windows: windows + 1,
                };
                self.ev = if self.ev == floor { floor / 2.0 } else { floor };
                self.exposure_changed(now)
            }
            Ladder::GaveUp => Rung::Pass,
        }
    }

    /// Plan 6.3 step 4: focus on the code, at most every
    /// `QR_AF_RETRIGGER_MS`.
    fn focus(&mut self, area: FracBox, now: Instant, limits: &Limits) -> Option<Change> {
        if !limits.has_af
            || self
                .last_af
                .is_some_and(|at| now < at + ms(QR_AF_RETRIGGER_MS))
        {
            return None;
        }
        self.last_af = Some(now);
        self.af_locked = true;
        if limits.af_regions {
            self.af_region = Some(area);
        }
        Some(Change::Focus(AfTrigger::Start))
    }

    /// Plan 6.3 step 5 and the central-area test of 7.1.
    fn missed(&mut self, luma: Option<Luma>, now: Instant, limits: &Limits) -> Option<Change> {
        if self.misses < QR_LOST_FRAMES {
            return None;
        }
        // Lost: back toward the app's zoom, one step per window.
        if self.auto > 1.0 {
            self.auto = (self.auto / QR_ZOOM_STEP_MAX).max(1.0);
            self.settle = Some(Settle::Zoom(now + ms(QR_ZOOM_SETTLE_MS)));
            return Some(Change::Zoom);
        }
        // Nothing found at all and the centre looks like a bright screen.
        if self.mode == ScreenExposure::Auto && !self.seen && luma.is_some_and(|l| l.overexposed())
        {
            match self.ladder_step(CENTRE, now, limits) {
                Rung::Changed => {
                    self.origin = Some(Origin::Centre);
                    return Some(Change::Exposure);
                }
                Rung::Held => return None,
                Rung::Pass => {}
            }
        }
        // Exposure back to the mode's start (0 EV, no region).
        if self.origin == Some(Origin::Code) {
            self.origin = None;
            self.ladder = self.base.2;
            if (self.ev, self.ae_region) != (self.base.0, self.base.1) {
                self.ev = self.base.0;
                self.ae_region = self.base.1;
                self.exposure_changed(now);
                return Some(Change::Exposure);
            }
        }
        // Focus regions cleared, back to continuous focus.
        if self.af_locked || self.af_region.is_some() {
            self.af_locked = false;
            self.af_region = None;
            self.last_af = None;
            return Some(Change::Focus(AfTrigger::Cancel));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const API30: Limits = Limits {
        user: 1.0,
        max_ratio: 10.0,
        fine: true,
        ae_regions: true,
        af_regions: true,
        has_af: true,
        ev_floor: -2.0,
    };

    const API29: Limits = Limits {
        fine: false,
        ..API30
    };

    /// A square code of `modules` modules at `module_px`, centred at
    /// `(cx, cy)` px in a 1280 × 720 frame.
    fn measure(module_px: f32, modules: u32, cx: f32, cy: f32) -> Measure {
        let half = module_px * modules as f32 / 2.0;
        let corners = [
            (cx - half, cy - half),
            (cx + half, cy - half),
            (cx + half, cy + half),
            (cx - half, cy + half),
        ];
        Measure::of(&corners, modules, 1280, 720)
    }

    fn luma(bright: bool) -> Option<Luma> {
        Some(if bright {
            Luma {
                mean: 230.0,
                p_sat: 0.4,
            }
        } else {
            Luma {
                mean: 120.0,
                p_sat: 0.0,
            }
        })
    }

    fn scan(target: Target, bright: bool) -> Scan {
        Scan {
            width: 1280,
            height: 720,
            codes: Vec::new(),
            target,
            luma: luma(bright),
        }
    }

    fn found(m: Measure, bright: bool) -> Scan {
        scan(Target::Found(m), bright)
    }

    fn missed(bright: bool) -> Scan {
        scan(Target::Missed, bright)
    }

    /// A clock that steps past every settle time.
    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Clock(Instant::now())
        }
        fn tick(&mut self) -> Instant {
            self.0 += ms(QR_AE_SETTLE_MS.max(QR_ZOOM_SETTLE_MS) + 1);
            self.0
        }
        fn frame(&mut self) -> Instant {
            self.0 += ms(66);
            self.0
        }
    }

    #[test]
    fn a_two_px_code_zooms_in_capped_by_the_step() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let t = Instant::now();
        // 2 px modules want 2×; one window gives at most QR_ZOOM_STEP_MAX.
        let m = measure(2.0, 25, 640.0, 360.0);
        assert_eq!(c.step(&found(m, false), t, &API30), Some(Change::Zoom));
        assert_eq!(c.auto(), QR_ZOOM_STEP_MAX);
        // Nothing more while the zoom settles.
        assert_eq!(c.step(&found(m, false), t + ms(100), &API30), None);
    }

    #[test]
    fn a_rendered_two_px_code_that_does_not_read_zooms_in() {
        use crate::camera::qr::scan::{Rqrr, analyse, tests};
        let payload = "https://example.com/pair?k=0123456789";
        let n = tests::modules_of(payload);
        let module = 2;
        let at = (560, 300);
        let mut grey = tests::render(payload, module, (1280, 720), at);
        // Damage the data area past what error correction can mend; the
        // finder patterns stay.
        let (x0, y0) = (at.0 + (4 + 9) * module, at.1 + (4 + 9) * module);
        let side = (n - 18) * module;
        for y in y0..y0 + side {
            for x in x0..x0 + side {
                grey.data[y * 1280 + x] = if (x / 2 + y / 3) % 2 == 0 { 0 } else { 255 };
            }
        }
        let scan = analyse(&mut Rqrr, &grey);
        let Target::Found(m) = scan.target else {
            panic!("{:?}", scan.target);
        };
        assert!((m.module_px - 2.0).abs() < 0.3, "{}", m.module_px);
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        assert_eq!(c.step(&scan, Instant::now(), &API30), Some(Change::Zoom));
        assert_eq!(c.auto(), QR_ZOOM_STEP_MAX);
    }

    #[test]
    fn an_off_centre_code_is_capped_by_k_fit() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        // 2 px modules, 50 px wide, its right edge at x = 1100 of 1280:
        // reach = (1100 − 640) / 640 ≈ 0.719, so k_fit = 0.9 / 0.719 ≈ 1.252.
        let m = measure(2.0, 25, 1075.0, 360.0);
        let reach = m.offset.0 + m.half.0;
        assert!((reach - 460.0 / 640.0).abs() < 1e-4, "{reach}");
        assert_eq!(
            c.step(&found(m, false), Instant::now(), &API30),
            Some(Change::Zoom)
        );
        assert!((c.auto() - 0.9 / reach).abs() < 1e-4, "{}", c.auto());
        // At the edge of the margin: nothing more to fit (k < the fine step).
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let edge = measure(2.0, 25, 640.0 + 0.9 * 640.0 - 25.0, 360.0);
        assert_eq!(
            c.step(&found(edge, false), Instant::now(), &API30),
            Some(Change::Focus(AfTrigger::Start))
        );
        assert_eq!(c.auto(), 1.0);
    }

    /// A camera whose code shows `base` px modules at 1×, readable from
    /// `QR_TARGET_MODULE_PX`: run the controller until nothing changes.
    fn converge(base: f32, limits: &Limits) -> f32 {
        let mut c = Controller::new(ScreenExposure::Auto, true, limits);
        let mut clock = Clock::new();
        for _ in 0..20 {
            let px = base * limits.user * c.auto();
            let m = measure(px, 25, 640.0, 360.0);
            let target = if px >= QR_TARGET_MODULE_PX {
                Target::Decoded(m)
            } else {
                Target::Found(m)
            };
            match c.step(&scan(target, false), clock.tick(), limits) {
                Some(Change::Zoom) => {}
                _ => break,
            }
        }
        base * limits.user * c.auto()
    }

    #[test]
    fn fine_zoom_finishes_on_a_readable_module_size_on_api_30() {
        // 3.7 px wants only 1.08×: below the coarse step.
        let api30 = converge(3.7, &API30);
        assert!(
            (QR_TARGET_MODULE_PX..=QR_TARGET_MODULE_PX * QR_FINE_AIM + 1e-3).contains(&api30),
            "{api30}"
        );
        // API 29 (crop region) keeps the coarse steps: it stops short.
        assert_eq!(converge(3.7, &API29), 3.7);
        // From far below, both reach the target; API 30 is never short of it.
        assert!(converge(1.5, &API30) >= QR_TARGET_MODULE_PX);
        let api29 = converge(1.5, &API29);
        assert!(api29 >= QR_TARGET_MODULE_PX / QR_ZOOM_MIN_STEP, "{api29}");
    }

    #[test]
    fn auto_zoom_never_goes_below_the_app_zoom_nor_past_its_caps() {
        let at4 = Limits { user: 4.0, ..API30 };
        // From 4×: at most 8.0 / 4.0 = 2× on top.
        assert_eq!(at4.auto_max(), 2.0);
        assert_eq!(API30.auto_max(), QR_AUTO_MAX_FACTOR);
        let tele = Limits { user: 9.0, ..API30 };
        assert_eq!(tele.auto_max(), 1.0);
        let mut c = Controller::new(ScreenExposure::Auto, true, &at4);
        let mut clock = Clock::new();
        let tiny = measure(0.5, 25, 640.0, 360.0);
        for _ in 0..10 {
            c.step(&found(tiny, false), clock.tick(), &at4);
        }
        assert_eq!(c.auto(), 2.0);
        // Lost: back down step by step to the app's zoom, never below.
        for _ in 0..(QR_LOST_FRAMES + 20) {
            c.step(&missed(false), clock.tick(), &at4);
            assert!(c.auto() >= 1.0);
        }
        assert_eq!(c.auto(), 1.0);
    }

    #[test]
    fn auto_zoom_pauses_after_the_app_sets_the_zoom() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let t = Instant::now();
        let small = measure(2.0, 25, 640.0, 360.0);
        assert_eq!(c.step(&found(small, false), t, &API30), Some(Change::Zoom));
        // A pinch: auto drops back to 1 and waits QR_USER_HOLD_MS after the
        // last call.
        let pinch = t + ms(400);
        assert!(c.app_zoomed(pinch));
        assert_eq!(c.auto(), 1.0);
        assert!(!c.app_zoomed(pinch + ms(100)));
        let last = pinch + ms(100);
        for dt in [400, 800, 1200, QR_USER_HOLD_MS - 1] {
            assert_eq!(c.step(&found(small, false), last + ms(dt), &API30), None);
        }
        assert_eq!(c.auto(), 1.0);
        assert_eq!(
            c.step(&found(small, false), last + ms(QR_USER_HOLD_MS), &API30),
            Some(Change::Zoom)
        );
    }

    #[test]
    fn a_saturated_code_walks_the_ladder_one_rung_per_window() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let mut clock = Clock::new();
        let big = measure(6.0, 25, 640.0, 360.0);
        let mut step = |c: &mut Controller| c.step(&found(big, true), clock.tick(), &API30);
        // Rung 1: meter on the code.
        assert_eq!(step(&mut c), Some(Change::Exposure));
        assert_eq!(c.ae_region(), Some(big.bbox));
        assert_eq!((c.ev(), c.ladder()), (0.0, Ladder::Metered));
        // Rung 2: −1, then −2 (the floor).
        assert_eq!(step(&mut c), Some(Change::Exposure));
        assert_eq!(c.ev(), -1.0);
        assert_eq!(step(&mut c), Some(Change::Exposure));
        assert_eq!(c.ev(), -2.0);
        // QR_BRACKET_AFTER failing windows at the floor, with no change.
        for _ in 0..QR_BRACKET_AFTER {
            assert_eq!(step(&mut c), None);
            assert_eq!(c.ev(), -2.0);
        }
        // Rung 3: QR_BRACKET_WINDOWS windows alternating −1 and −2.
        let mut seen = Vec::new();
        for _ in 0..QR_BRACKET_WINDOWS {
            assert_eq!(step(&mut c), Some(Change::Exposure));
            seen.push(c.ev());
        }
        assert_eq!(seen, [-1.0, -2.0, -1.0, -2.0, -1.0, -2.0]);
        // Rung 4: give up quietly at the floor; focus is still tried.
        assert_eq!(step(&mut c), Some(Change::Focus(AfTrigger::Start)));
        assert_eq!((c.ev(), c.ladder()), (-2.0, Ladder::GaveUp));
        // Focus waits QR_AF_RETRIGGER_MS between triggers; exposure stays.
        assert_eq!(step(&mut c), None);
        assert_eq!(c.ev(), -2.0);
    }

    #[test]
    fn exposure_windows_end_early_when_ae_converges_again() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let t = Instant::now();
        let big = measure(6.0, 25, 640.0, 360.0);
        assert_eq!(c.step(&found(big, true), t, &API30), Some(Change::Exposure));
        // Still CONVERGED from before the change: not settled.
        c.ae_state(Some(2));
        assert_eq!(c.step(&found(big, true), t + ms(100), &API30), None);
        // SEARCHING, then CONVERGED: settled before QR_AE_SETTLE_MS.
        c.ae_state(Some(1));
        c.ae_state(Some(2));
        assert_eq!(
            c.step(&found(big, true), t + ms(150), &API30),
            Some(Change::Exposure)
        );
        assert_eq!(c.ev(), -1.0);
    }

    #[test]
    fn zoom_comes_before_exposure() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let mut clock = Clock::new();
        let small = measure(2.0, 25, 640.0, 360.0);
        assert_eq!(
            c.step(&found(small, true), clock.tick(), &API30),
            Some(Change::Zoom)
        );
        assert_eq!(c.ev(), 0.0);
        assert_eq!(c.ae_region(), None);
    }

    #[test]
    fn loss_resets_zoom_then_exposure_then_focus() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let mut clock = Clock::new();
        let small = measure(2.5, 25, 640.0, 360.0);
        assert_eq!(
            c.step(&found(small, false), clock.tick(), &API30),
            Some(Change::Zoom)
        );
        let big = measure(6.0, 25, 640.0, 360.0);
        for _ in 0..3 {
            assert_eq!(
                c.step(&found(big, true), clock.tick(), &API30),
                Some(Change::Exposure)
            );
        }
        assert_eq!(c.ev(), -2.0);
        // Not lost until QR_LOST_FRAMES misses.
        for _ in 0..QR_LOST_FRAMES - 1 {
            assert_eq!(c.step(&missed(false), clock.frame(), &API30), None);
        }
        let mut changes = Vec::new();
        for _ in 0..6 {
            if let Some(change) = c.step(&missed(false), clock.tick(), &API30) {
                changes.push(change);
            }
        }
        assert_eq!(changes, [Change::Zoom, Change::Exposure]);
        assert_eq!(c.auto(), 1.0);
        assert_eq!((c.ev(), c.ae_region()), (0.0, None));
        assert_eq!(c.ladder(), Ladder::Start);
        // With a focus trigger sent, loss also cancels it.
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        assert_eq!(
            c.step(&found(big, false), clock.tick(), &API30),
            Some(Change::Focus(AfTrigger::Start))
        );
        assert_eq!(c.af_region(), Some(big.bbox));
        let mut changes = Vec::new();
        for _ in 0..QR_LOST_FRAMES + 3 {
            changes.extend(c.step(&missed(false), clock.frame(), &API30));
        }
        assert_eq!(changes, [Change::Focus(AfTrigger::Cancel)]);
        assert_eq!(c.af_region(), None);
    }

    #[test]
    fn a_bright_centre_starts_the_ladder_only_before_any_code() {
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let mut clock = Clock::new();
        for _ in 0..QR_LOST_FRAMES - 1 {
            assert_eq!(c.step(&missed(true), clock.frame(), &API30), None);
        }
        assert_eq!(
            c.step(&missed(true), clock.tick(), &API30),
            Some(Change::Exposure)
        );
        assert_eq!(c.ae_region(), Some(CENTRE));
        assert_eq!(
            c.step(&missed(true), clock.tick(), &API30),
            Some(Change::Exposure)
        );
        assert_eq!(c.ev(), -1.0);
        // Once a code has been seen, a bright centre does nothing.
        let mut c = Controller::new(ScreenExposure::Auto, true, &API30);
        let big = measure(6.0, 25, 640.0, 360.0);
        assert_eq!(
            c.step(&scan(Target::Decoded(big), false), clock.tick(), &API30),
            None
        );
        for _ in 0..QR_LOST_FRAMES + 3 {
            assert_eq!(c.step(&missed(true), clock.tick(), &API30), None);
        }
        assert_eq!(c.ev(), 0.0);
    }

    #[test]
    fn screen_starts_at_half_the_floor_and_off_never_adjusts() {
        let screen = Controller::new(ScreenExposure::Screen, true, &API30);
        assert_eq!(screen.ev(), -1.0);
        assert_eq!(screen.ae_region(), Some(CENTRE));
        let mut screen = screen;
        let mut clock = Clock::new();
        let big = measure(6.0, 25, 640.0, 360.0);
        // The ladder continues from rung 2: next is the floor.
        assert_eq!(
            screen.step(&found(big, true), clock.tick(), &API30),
            Some(Change::Exposure)
        );
        assert_eq!(screen.ev(), -2.0);
        // Lost: back to the mode's start, not 0 EV.
        for _ in 0..QR_LOST_FRAMES + 3 {
            screen.step(&missed(false), clock.tick(), &API30);
        }
        assert_eq!((screen.ev(), screen.ae_region()), (-1.0, Some(CENTRE)));
        // A device whose compensation only goes to −1.5 EV.
        let shallow = Limits {
            ev_floor: ev_floor((-3, 3), 0.5),
            ..API30
        };
        assert_eq!(shallow.ev_floor, -1.5);
        assert_eq!(
            Controller::new(ScreenExposure::Screen, true, &shallow).ev(),
            -0.75
        );
        // Off: a bright code gets focus, never exposure.
        let mut off = Controller::new(ScreenExposure::Off, true, &API30);
        assert_eq!(
            off.step(&found(big, true), clock.tick(), &API30),
            Some(Change::Focus(AfTrigger::Start))
        );
        for _ in 0..QR_LOST_FRAMES + 3 {
            off.step(&missed(true), clock.tick(), &API30);
        }
        assert_eq!((off.ev(), off.ae_region()), (0.0, None));
    }

    #[test]
    fn without_auto_zoom_a_small_code_goes_to_exposure_and_focus() {
        let mut c = Controller::new(ScreenExposure::Auto, false, &API30);
        let small = measure(2.0, 25, 640.0, 360.0);
        assert_eq!(
            c.step(&found(small, false), Instant::now(), &API30),
            Some(Change::Focus(AfTrigger::Start))
        );
        assert_eq!(c.auto(), 1.0);
    }

    #[test]
    fn ev_conversions() {
        // 1/3 EV steps, −12..=12 (−4..=4 EV): the floor is −2 EV = −6 steps.
        assert_eq!(ev_floor((-12, 12), 1.0 / 3.0), -2.0);
        assert_eq!(ev_steps(-2.0, (-12, 12), 1.0 / 3.0), -6);
        assert_eq!(ev_steps(-1.0, (-12, 12), 1.0 / 3.0), -3);
        assert_eq!(ev_steps(-9.0, (-12, 12), 1.0 / 3.0), -12);
        // No compensation.
        assert_eq!(ev_floor((0, 0), 0.0), 0.0);
        assert_eq!(ev_steps(-2.0, (0, 0), 0.0), 0);
    }
}
