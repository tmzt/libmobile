//! The `libmobile-camera` thread (plan 5.1): it owns the camera manager, the
//! device, the capture session, the image readers and every request, and
//! takes commands over one channel. The NDK's callbacks (device state,
//! capture results, image listeners) only post into the same channel, so
//! all camera state lives on this thread.
//!
//! After every message the thread *reconciles*: from the session the app
//! wants and the oldest capture, it works out the configuration the camera
//! should have (which camera, the analysis stream's size, a still reader or
//! none) and opens, reconfigures or closes the camera to match. Failures
//! end the session or capture that needed them.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use android_activity::{AndroidApp, AndroidAppWaker};
use jni::objects::JIntArray;
use jni::{jni_sig, jni_str};

use super::CAMERA_PERMISSION;
#[cfg(feature = "camera-capture")]
use super::ndk::FORMAT_JPEG;
use super::ndk::{
    self, Device, FORMAT_YUV, Manager, Metadata, Reader, ReaderKind, Session, Template,
    device_error, status, tag,
};
#[cfg(feature = "camera-qr")]
use super::qr::{self, QrState};
use crate::MobileEvent;
use crate::android::jni_util::with_app_class;
#[cfg(feature = "camera-capture")]
use crate::camera::PREVIEW_MAX;
use crate::camera::convert::{self, Planes};
use crate::camera::gate::{Admit, Answer, Gate};
use crate::camera::geometry::{self, Rect, StreamConfig, ZoomControl};
#[cfg(feature = "camera-qr")]
use crate::camera::qr::{
    QrCode, QrMode, QrOptions,
    control::{self, AfTrigger, Change, Controller, Limits},
    scan::{GreyFrame, Scan},
};
use crate::camera::{
    CameraError, CameraEvent, Facing, IMAGE_READER_MAX, PreviewFrame, PreviewOptions, SessionId,
    ZoomSource,
};
#[cfg(feature = "camera-capture")]
use crate::camera::{
    CaptureId, CaptureOptions, Frame, FrameFormat, Resolution,
    capture::{Captures, Phase},
};
use crate::event::{PermissionRequest, PermissionsOutcome};
use crate::mailbox::Mailbox;

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.camera.CameraPlugin";

/// How long the thread sleeps with no deadline pending.
const IDLE: Duration = Duration::from_secs(3600);

// ── Messages ─────────────────────────────────────────────────────────────────

/// A command for the camera thread.
pub(super) enum Msg {
    StartPreview {
        app: AndroidApp,
        session: SessionId,
        facing: Facing,
        preview: PreviewOptions,
    },
    #[cfg(feature = "camera-qr")]
    StartQr {
        app: AndroidApp,
        session: SessionId,
        options: QrOptions,
    },
    /// The QR thread's result for a frame of the streams of `epoch`.
    #[cfg(feature = "camera-qr")]
    QrScan {
        session: SessionId,
        epoch: u64,
        scan: Scan,
    },
    Stop(SessionId),
    Zoom {
        session: SessionId,
        change: ZoomChange,
    },
    #[cfg(feature = "camera-capture")]
    Capture {
        app: AndroidApp,
        capture: CaptureId,
        options: CaptureOptions,
    },
    #[cfg(feature = "camera-capture")]
    Cancel(CaptureId),
    #[cfg(feature = "camera-intent")]
    Intent {
        app: AndroidApp,
    },
    /// The gate's permission answer.
    Permission {
        request: u32,
        granted: bool,
    },
    Pause,
    Resume(AndroidApp),
    ConfigChanged(AndroidApp),
    Destroy,
    /// From an NDK callback, with the generation it was registered with.
    Ndk {
        epoch: u64,
        event: NdkEvent,
    },
}

/// `set_zoom` or `zoom_by`.
#[derive(Clone, Copy, Debug)]
pub(super) enum ZoomChange {
    Set(f32),
    By(f32),
}

/// What an NDK callback reports.
#[derive(Clone, Copy, Debug)]
pub(super) enum NdkEvent {
    Disconnected,
    Error(i32),
    AnalysisImage,
    StillImage,
    /// A repeating frame completed with these AE and AF states.
    Result {
        ae: Option<u8>,
        af: Option<u8>,
    },
    #[cfg(feature = "camera-capture")]
    StillFailed,
}

// ── Process-wide state shared with other threads ────────────────────────────

static TX: OnceLock<Sender<Msg>> = OnceLock::new();
static EVENTS: Mailbox<CameraEvent> = Mailbox::new();
static WAKER: Mutex<Option<AndroidAppWaker>> = Mutex::new(None);
/// The gate's request in flight, for routing its answer
/// ([`offer_permission`]).
static GATE_REQUEST: Mutex<Option<PermissionRequest>> = Mutex::new(None);
/// Whether the activity is resumed; kept even while the thread is not
/// running, so it starts knowing.
static RESUMED: AtomicBool = AtomicBool::new(false);
static SHARED: Mutex<Shared> = Mutex::new(Shared::EMPTY);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What `latest_preview`, `zoom` and `zoom_range` read from any thread.
pub(super) struct Shared {
    session: Option<SessionId>,
    range: Option<(f32, f32)>,
    zoom: Option<f32>,
    preview: Option<Arc<PreviewFrame>>,
}

impl Shared {
    const EMPTY: Shared = Shared {
        session: None,
        range: None,
        zoom: None,
        preview: None,
    };

    pub(super) fn preview_of(&self, session: SessionId) -> Option<Arc<PreviewFrame>> {
        self.preview
            .clone()
            .filter(|_| self.session == Some(session))
    }

    pub(super) fn zoom_range_of(&self, session: SessionId) -> Option<(f32, f32)> {
        self.range.filter(|_| self.session == Some(session))
    }

    pub(super) fn zoom_of(&self, session: SessionId) -> Option<f32> {
        self.zoom.filter(|_| self.session == Some(session))
    }
}

pub(super) fn shared<R>(f: impl FnOnce(&Shared) -> R) -> R {
    f(&lock(&SHARED))
}

fn reset_shared(session: Option<SessionId>) {
    *lock(&SHARED) = Shared {
        session,
        ..Shared::EMPTY
    };
}

/// Send a command, starting the thread on first use.
pub(super) fn post(msg: Msg) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("libmobile-camera".into())
            .spawn(move || run(rx));
        if let Err(err) = spawned {
            log::error!("libmobile: the camera thread could not start: {err}");
        }
        tx
    });
    let _ = tx.send(msg);
}

/// Send a command only if the thread runs (nothing to stop or tell otherwise).
pub(super) fn post_if_running(msg: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(msg);
    }
}

/// From an NDK callback thread.
pub(super) fn post_ndk(epoch: u64, event: NdkEvent) {
    post_if_running(Msg::Ndk { epoch, event });
}

pub(super) fn lifecycle(app: &AndroidApp, event: &MobileEvent) {
    match event {
        MobileEvent::Pause => {
            RESUMED.store(false, Ordering::SeqCst);
            post_if_running(Msg::Pause);
        }
        MobileEvent::Resume => {
            RESUMED.store(true, Ordering::SeqCst);
            post_if_running(Msg::Resume(app.clone()));
        }
        MobileEvent::ConfigChanged => post_if_running(Msg::ConfigChanged(app.clone())),
        MobileEvent::Destroy => {
            RESUMED.store(false, Ordering::SeqCst);
            post_if_running(Msg::Destroy);
        }
        _ => {}
    }
}

pub(super) fn offer_permission(outcome: PermissionsOutcome) -> Option<PermissionsOutcome> {
    let mut slot = lock(&GATE_REQUEST);
    if *slot != Some(outcome.request) {
        return Some(outcome);
    }
    *slot = None;
    let granted = outcome.granted(CAMERA_PERMISSION);
    log::info!("libmobile: camera permission granted: {granted}");
    post_if_running(Msg::Permission {
        request: outcome.request.id(),
        granted,
    });
    None
}

pub(super) fn take_events() -> Vec<CameraEvent> {
    EVENTS.take()
}

fn emit(event: CameraEvent) {
    EVENTS.push(event);
    wake();
}

fn wake() {
    if let Some(waker) = lock(&WAKER).as_ref() {
        waker.wake();
    }
}

// ── The thread ───────────────────────────────────────────────────────────────

fn run(rx: Receiver<Msg>) {
    log::info!("libmobile: camera thread start");
    let mut worker = Worker::new();
    loop {
        let wait = worker
            .next_wakeup()
            .map_or(IDLE, |at| at.saturating_duration_since(Instant::now()));
        match rx.recv_timeout(wait) {
            Ok(msg) => worker.handle(msg),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        worker.tick(Instant::now());
    }
}

/// One camera the NDK lists, from its characteristics.
#[derive(Clone, Debug)]
struct CameraInfo {
    id: CString,
    facing: Facing,
    /// `ACAMERA_SENSOR_ORIENTATION`, degrees.
    sensor: i32,
    configs: Vec<StreamConfig>,
    zoom: ZoomControl,
    /// The AF mode used: continuous picture, else continuous video, else
    /// auto; 0 (off) when the camera has no autofocus.
    af_mode: u8,
    /// `ACAMERA_CONTROL_AE_COMPENSATION_RANGE` (device steps) and
    /// `AE_COMPENSATION_STEP` (EV per step; 0 when there is none).
    #[cfg(feature = "camera-qr")]
    ev_range: (i32, i32),
    #[cfg(feature = "camera-qr")]
    ev_step: f32,
    /// `ACAMERA_CONTROL_MAX_REGIONS`: AE and AF regions allowed.
    #[cfg(feature = "camera-qr")]
    max_regions: (i32, i32),
}

impl CameraInfo {
    fn from_metadata(id: CString, meta: &Metadata, api: i32) -> Option<Self> {
        let facing = match meta.u8s(tag::LENS_FACING).first() {
            Some(0) => Facing::Front,
            Some(1) => Facing::Back,
            _ => return None, // External cameras are not chosen by facing.
        };
        let active = match meta.i32s(tag::ACTIVE_ARRAY_SIZE)[..] {
            [x, y, w, h, ..] => Rect { x, y, w, h },
            _ => Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        };
        let ratio = meta.f32s(tag::ZOOM_RATIO_RANGE);
        let ratio_range = match ratio[..] {
            [min, max, ..] => Some((min, max)),
            _ => None,
        };
        let max_digital = meta.f32s(tag::MAX_DIGITAL_ZOOM).first().copied();
        let modes = meta.u8s(tag::AF_AVAILABLE_MODES);
        // CONTINUOUS_PICTURE (4), CONTINUOUS_VIDEO (3), AUTO (1).
        let af_mode = [4u8, 3, 1]
            .into_iter()
            .find(|m| modes.contains(m))
            .unwrap_or(0);
        Some(Self {
            id,
            facing,
            sensor: meta
                .i32s(tag::SENSOR_ORIENTATION)
                .first()
                .copied()
                .unwrap_or(90),
            configs: geometry::output_configs(&meta.i32s(tag::STREAM_CONFIGURATIONS)),
            zoom: ZoomControl::new(api, ratio_range, max_digital, active),
            af_mode,
            #[cfg(feature = "camera-qr")]
            ev_range: match meta.i32s(tag::AE_COMPENSATION_RANGE)[..] {
                [min, max, ..] if min <= max => (min, max),
                _ => (0, 0),
            },
            #[cfg(feature = "camera-qr")]
            ev_step: match meta.rationals(tag::AE_COMPENSATION_STEP).first() {
                Some(&(n, d)) if d != 0 && n > 0 => n as f32 / d as f32,
                _ => 0.0,
            },
            #[cfg(feature = "camera-qr")]
            max_regions: match meta.i32s(tag::MAX_REGIONS)[..] {
                // AE, AWB, AF.
                [ae, _, af, ..] => (ae, af),
                _ => (0, 0),
            },
        })
    }

    /// What auto zoom and exposure may use on this camera, at the app's
    /// zoom `user`.
    #[cfg(feature = "camera-qr")]
    fn qr_limits(&self, user: f32) -> Limits {
        Limits {
            user,
            max_ratio: self.zoom.range().1,
            fine: matches!(self.zoom, ZoomControl::Ratio { .. }),
            ae_regions: self.max_regions.0 > 0,
            af_regions: self.max_regions.1 > 0 && self.af_mode != 0,
            has_af: self.af_mode != 0,
            ev_floor: control::ev_floor(self.ev_range, self.ev_step),
        }
    }

    #[cfg(feature = "camera-capture")]
    fn has_af(&self) -> bool {
        self.af_mode != 0
    }
}

/// A still reader's format and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StillSpec {
    format: i32,
    size: (u32, u32),
}

/// What the camera should be doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Config {
    /// Index into the listed cameras.
    camera: usize,
    analysis: (u32, u32),
    still: Option<StillSpec>,
}

/// The session and readers on an open device. Fields drop in order: the
/// session closes before the readers it streams to are deleted.
struct Streams {
    session: Session,
    #[cfg_attr(not(feature = "camera-capture"), allow(dead_code))] // Held for the session.
    still: Option<Reader>,
    analysis: Reader,
    epoch: u64,
    config: Config,
}

/// An open device. `streams` drops (closes) before the device.
struct Open {
    streams: Option<Streams>,
    device: Device,
    device_epoch: u64,
    camera: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionPhase {
    /// Waiting for the permission answer.
    Permission,
    /// Granted; not open yet.
    Ready,
    Open,
    /// Closed by a pause; reopens on resume.
    Suspended,
}

struct LiveSession {
    id: SessionId,
    facing: Facing,
    /// `None`: no preview frames (a QR session without a viewfinder).
    preview: Option<PreviewOptions>,
    /// The app's zoom ratio (`set_zoom` / `zoom_by`).
    user_zoom: f32,
    phase: SessionPhase,
    sequence: u64,
    /// A QR session's decoder hand-off, auto zoom and exposure.
    #[cfg(feature = "camera-qr")]
    qr: Option<QrState>,
}

impl LiveSession {
    /// Whether it wants the camera (granted, not stopped).
    fn wants_camera(&self) -> bool {
        self.phase != SessionPhase::Permission
    }

    /// The QR controller, while the session is open.
    #[cfg(feature = "camera-qr")]
    fn control(&self) -> Option<&Controller> {
        self.qr.as_ref().and_then(|q| q.control.as_ref())
    }

    /// Auto zoom's factor on top of the app's zoom (1 outside QR scanning).
    fn auto_zoom(&self) -> f32 {
        #[cfg(feature = "camera-qr")]
        if let Some(c) = self.control() {
            return c.auto();
        }
        1.0
    }
}

/// A call waiting at the permission gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Session(SessionId),
    #[cfg(feature = "camera-capture")]
    Capture(CaptureId),
    #[cfg(feature = "camera-intent")]
    Intent,
}

/// Who a failure ends: the active capture, or the session and the active
/// capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    #[cfg(feature = "camera-capture")]
    Capture,
    All,
}

struct Worker {
    app: Option<AndroidApp>,
    api: i32,
    manager: Option<Manager>,
    cameras: Vec<CameraInfo>,
    resumed: bool,
    /// Display rotation (degrees) and aspect (long / short).
    display: (i32, f32),
    gate: Gate<Call>,
    session: Option<LiveSession>,
    #[cfg(feature = "camera-capture")]
    captures: Captures,
    open: Option<Open>,
    next_epoch: u64,
    /// The newest AE and AF states of the current streams; `None` until the
    /// first result arrives.
    three_a: Option<(Option<u8>, Option<u8>)>,
}

impl Worker {
    fn new() -> Self {
        Self {
            app: None,
            api: AndroidApp::sdk_version(),
            manager: None,
            cameras: Vec::new(),
            resumed: RESUMED.load(Ordering::SeqCst),
            display: (0, 16.0 / 9.0),
            gate: Gate::default(),
            session: None,
            #[cfg(feature = "camera-capture")]
            captures: Captures::default(),
            open: None,
            next_epoch: 1,
            three_a: None,
        }
    }

    fn next_epoch(&mut self) -> u64 {
        self.next_epoch += 1;
        self.next_epoch
    }

    fn set_app(&mut self, app: AndroidApp) {
        *lock(&WAKER) = Some(app.create_waker());
        self.app = Some(app);
    }

    fn granted(&self) -> bool {
        self.app
            .as_ref()
            .is_some_and(|app| crate::android::permission_granted(app, CAMERA_PERMISSION))
    }

    fn next_wakeup(&self) -> Option<Instant> {
        #[cfg(feature = "camera-capture")]
        return self.captures.next_wakeup();
        #[cfg(not(feature = "camera-capture"))]
        None
    }

    // ── Commands ─────────────────────────────────────────────────────────────

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::StartPreview {
                app,
                session,
                facing,
                preview,
            } => self.start_session(
                app,
                session,
                facing,
                Some(preview),
                #[cfg(feature = "camera-qr")]
                None,
            ),
            #[cfg(feature = "camera-qr")]
            Msg::StartQr {
                app,
                session,
                options,
            } => self.start_session(
                app,
                session,
                options.facing,
                options.preview,
                Some(QrState::new(options)),
            ),
            #[cfg(feature = "camera-qr")]
            Msg::QrScan {
                session,
                epoch,
                scan,
            } => self.qr_scan(session, epoch, &scan),
            Msg::Stop(session) => {
                if self.session.as_ref().is_some_and(|s| s.id == session) {
                    self.end_session(None);
                }
            }
            Msg::Zoom { session, change } => self.zoom(session, change),
            #[cfg(feature = "camera-capture")]
            Msg::Capture {
                app,
                capture,
                options,
            } => {
                self.set_app(app);
                let granted = self.admit(Call::Capture(capture)).is_some();
                self.captures.push(capture, options, granted);
            }
            #[cfg(feature = "camera-capture")]
            Msg::Cancel(capture) => {
                if self.captures.cancel(capture).is_some() {
                    self.gate.withdraw(|c| *c != Call::Capture(capture));
                    emit(CameraEvent::Frame {
                        capture,
                        result: Err(CameraError::Cancelled),
                    });
                }
            }
            #[cfg(feature = "camera-intent")]
            Msg::Intent { app } => {
                self.set_app(app.clone());
                if self.admit(Call::Intent).is_some() {
                    crate::android::camera_intent::launch(&app);
                }
            }
            Msg::Permission { request, granted } => self.answer(request, granted),
            Msg::Pause => self.pause(),
            Msg::Resume(app) => {
                self.set_app(app);
                self.resumed = true;
                self.read_display();
            }
            Msg::ConfigChanged(app) => {
                self.set_app(app);
                self.read_display();
            }
            Msg::Destroy => self.destroy(),
            Msg::Ndk { epoch, event } => self.ndk(epoch, event),
        }
    }

    /// A new session (stopping the old one first), waiting for the
    /// permission answer if CAMERA is not granted yet.
    fn start_session(
        &mut self,
        app: AndroidApp,
        session: SessionId,
        facing: Facing,
        preview: Option<PreviewOptions>,
        #[cfg(feature = "camera-qr")] qr: Option<QrState>,
    ) {
        self.set_app(app);
        self.end_session(None);
        let phase = match self.admit(Call::Session(session)) {
            Some(_) => SessionPhase::Ready,
            None => SessionPhase::Permission,
        };
        self.session = Some(LiveSession {
            id: session,
            facing,
            preview,
            user_zoom: 1.0,
            phase,
            sequence: 0,
            #[cfg(feature = "camera-qr")]
            qr,
        });
        reset_shared(Some(session));
    }

    /// Pass `call` through the gate: `Some` if it may go ahead now; else it
    /// waits (the request is made here if none is in flight).
    fn admit(&mut self, call: Call) -> Option<Call> {
        let granted = self.granted();
        match self.gate.admit(granted, call) {
            Admit::Proceed(call) => Some(call),
            Admit::Ask => {
                self.ask();
                None
            }
            Admit::Wait => None,
        }
    }

    fn ask(&mut self) {
        let Some(app) = self.app.clone() else {
            return;
        };
        // Held across the request, so an answer the main loop drains at once
        // (a failed JNI call answers immediately) is still routed here.
        let mut slot = lock(&GATE_REQUEST);
        let request = crate::android::request_permissions(&app, &[CAMERA_PERMISSION]);
        *slot = Some(request);
        self.gate.asked(request.id());
        log::info!(
            "libmobile: camera permission asked (request {})",
            request.id()
        );
    }

    fn answer(&mut self, request: u32, granted: bool) {
        let settings_only = !granted
            && !self
                .app
                .as_ref()
                .is_some_and(|app| crate::android::should_show_rationale(app, CAMERA_PERMISSION));
        match self.gate.answer(request, granted, settings_only) {
            None => {}
            Some(Answer::Granted(calls)) => {
                for call in calls {
                    match call {
                        Call::Session(id) => {
                            if let Some(s) = self.session.as_mut().filter(|s| s.id == id) {
                                s.phase = SessionPhase::Ready;
                            }
                        }
                        #[cfg(feature = "camera-capture")]
                        Call::Capture(id) => self.captures.grant(&[id]),
                        #[cfg(feature = "camera-intent")]
                        Call::Intent => {
                            if let Some(app) = &self.app {
                                crate::android::camera_intent::launch(app);
                            }
                        }
                    }
                }
            }
            Some(Answer::Denied(calls, error)) => {
                log::warn!("libmobile: camera permission refused (settings only: {settings_only})");
                for call in calls {
                    match call {
                        Call::Session(id) => {
                            if self.session.as_ref().is_some_and(|s| s.id == id) {
                                self.end_session(Some(error.clone()));
                            }
                        }
                        #[cfg(feature = "camera-capture")]
                        Call::Capture(id) => {
                            for (capture, error) in self.captures.deny(&[id], &error) {
                                emit(CameraEvent::Frame {
                                    capture,
                                    result: Err(error),
                                });
                            }
                        }
                        #[cfg(feature = "camera-intent")]
                        Call::Intent => crate::android::camera_intent::permission_denied(),
                    }
                }
            }
        }
    }

    /// End the session, if any: `Closed { reason }`.
    fn end_session(&mut self, reason: Option<CameraError>) {
        if let Some(old) = self.session.take() {
            self.gate.withdraw(|c| *c != Call::Session(old.id));
            emit(CameraEvent::Closed {
                session: old.id,
                reason,
            });
            reset_shared(None);
        }
    }

    fn zoom(&mut self, session: SessionId, change: ZoomChange) {
        let control = self.open.as_ref().map(|o| self.cameras[o.camera].zoom);
        let Some(s) = self.session.as_mut().filter(|s| s.id == session) else {
            return;
        };
        let target = match change {
            ZoomChange::Set(ratio) => ratio,
            // From the ratio shown, auto zoom included, so a pinch during
            // auto zoom does not jump back to the app's last ratio first.
            ZoomChange::By(factor) => s.user_zoom * s.auto_zoom() * factor,
        };
        if !target.is_finite() || target <= 0.0 {
            return;
        }
        let ratio = match (s.phase, control) {
            (SessionPhase::Open, Some(control)) => control.clamp(target),
            _ => target.max(1.0),
        };
        // A pinch wins over auto zoom: back to the app's zoom, and auto zoom
        // pauses (plan 6.2).
        #[cfg(feature = "camera-qr")]
        let auto_dropped =
            s.qr.as_mut()
                .and_then(|q| q.control.as_mut())
                .is_some_and(|c| c.app_zoomed(Instant::now()));
        #[cfg(not(feature = "camera-qr"))]
        let auto_dropped = false;
        if ratio == s.user_zoom && !auto_dropped {
            return;
        }
        s.user_zoom = ratio;
        if s.phase != SessionPhase::Open {
            return; // Applied on opening.
        }
        if let Err(status) = self.repeat() {
            log::warn!("libmobile: camera zoom not applied: status {status}");
            return;
        }
        lock(&SHARED).zoom = Some(ratio);
        emit(CameraEvent::Zoom {
            session,
            ratio,
            source: ZoomSource::App,
        });
    }

    fn pause(&mut self) {
        self.resumed = false;
        #[cfg(feature = "camera-capture")]
        for capture in self.captures.interrupt() {
            emit(CameraEvent::Frame {
                capture,
                result: Err(CameraError::Interrupted),
            });
        }
        self.close();
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.phase == SessionPhase::Open)
        {
            s.phase = SessionPhase::Suspended;
            emit(CameraEvent::Suspended { session: s.id });
        }
    }

    fn destroy(&mut self) {
        self.close();
        self.resumed = false;
        self.session = None;
        #[cfg(feature = "camera-capture")]
        let _ = self.captures.drain();
        self.gate.clear();
        *lock(&GATE_REQUEST) = None;
        reset_shared(None);
        *lock(&WAKER) = None;
        self.app = None;
    }

    fn read_display(&mut self) {
        let Some(app) = &self.app else {
            return;
        };
        let info = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
            let array = env
                .call_static_method(
                    class,
                    jni_str!("displayInfo"),
                    jni_sig!("(Landroid/app/Activity;)[I"),
                    &[activity.into()],
                )?
                .l()?;
            let array = env.cast_local::<JIntArray>(array)?;
            let mut out = [0i32; 3];
            array.get_region(env, 0, &mut out)?;
            Ok(out)
        });
        match info {
            Ok([rotation, w, h]) => {
                let (long, short) = (w.max(h), w.min(h).max(1));
                self.display = (rotation, long as f32 / short as f32);
            }
            Err(err) => log::warn!("libmobile: display rotation unknown: {err}"),
        }
    }

    // ── NDK reports ──────────────────────────────────────────────────────────

    fn ndk(&mut self, epoch: u64, event: NdkEvent) {
        let Some(open) = &self.open else {
            return;
        };
        let streams_epoch = open.streams.as_ref().map(|s| s.epoch);
        match event {
            NdkEvent::Disconnected | NdkEvent::Error(_) if epoch == open.device_epoch => {
                let error = match event {
                    NdkEvent::Error(device_error::CAMERA_DISABLED) => CameraError::Disabled,
                    NdkEvent::Error(device_error::CAMERA_IN_USE)
                    | NdkEvent::Error(device_error::MAX_CAMERAS_IN_USE)
                    | NdkEvent::Disconnected => CameraError::InUse,
                    NdkEvent::Error(code) => {
                        CameraError::Failed(format!("camera device error {code}"))
                    }
                    _ => CameraError::Failed("camera device".into()),
                };
                log::warn!("libmobile: camera lost: {error}");
                self.close();
                self.fail(Who::All, error);
            }
            NdkEvent::AnalysisImage if Some(epoch) == streams_epoch => self.analysis_image(),
            #[cfg(feature = "camera-capture")]
            NdkEvent::StillImage if Some(epoch) == streams_epoch => self.still_image(),
            NdkEvent::Result { ae, af } if Some(epoch) == streams_epoch => {
                self.three_a = Some((ae, af));
                #[cfg(feature = "camera-qr")]
                if let Some(c) = self
                    .session
                    .as_mut()
                    .and_then(|s| s.qr.as_mut())
                    .and_then(|q| q.control.as_mut())
                {
                    c.ae_state(ae);
                }
            }
            #[cfg(feature = "camera-capture")]
            NdkEvent::StillFailed if Some(epoch) == streams_epoch => {
                self.fail(
                    Who::Capture,
                    CameraError::Failed("still capture failed".into()),
                );
            }
            _ => {}
        }
    }

    /// Clockwise quarter turns to show the open camera's frames upright.
    fn turns(&self, info: &CameraInfo) -> u8 {
        geometry::quarter_turns(info.sensor, self.display.0, info.facing)
    }

    fn analysis_image(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let Some(streams) = &open.streams else {
            return;
        };
        let info = &self.cameras[open.camera];
        let turns = self.turns(info);
        // Always acquired, even when unused, so the stream keeps flowing.
        let Some(image) = streams.analysis.latest() else {
            return;
        };
        let Some(planes) = yuv_planes(&image) else {
            return;
        };
        #[cfg(feature = "camera-qr")]
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.phase == SessionPhase::Open)
            && let Some(q) = s.qr.as_mut()
            && q.due(Instant::now())
        {
            qr::submit(qr::Job {
                session: s.id,
                epoch: streams.epoch,
                grey: GreyFrame {
                    width: planes.width,
                    height: planes.height,
                    data: convert::pack_gray(&planes),
                },
            });
        }
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.phase == SessionPhase::Open)
            && let Some(preview) = s.preview
        {
            let factor = convert::scale_factor(planes.width, planes.height, preview.max_side);
            let mut i420 = convert::to_i420(&planes, factor);
            let mut quarter_turns = turns;
            if preview.upright {
                i420 = i420.rotated(turns);
                quarter_turns = 0;
            }
            s.sequence += 1;
            let frame = PreviewFrame {
                width: i420.width as u32,
                height: i420.height as u32,
                y: i420.y,
                u: i420.u,
                v: i420.v,
                quarter_turns,
                facing: info.facing,
                timestamp_ns: image.timestamp_ns(),
                sequence: s.sequence,
            };
            {
                let mut shared = lock(&SHARED);
                if shared.session == Some(s.id) {
                    shared.preview = Some(Arc::new(frame));
                }
            }
            wake();
        }
        #[cfg(feature = "camera-capture")]
        if streams.config.still.is_none()
            && let Some(active) = self.captures.active()
            && active.phase == Phase::Requested
        {
            let options = active.options.clone();
            let bound = requested_size(info, &options)
                .unwrap_or((planes.width as u32, planes.height as u32));
            let frame = frame_from_planes(
                &planes,
                &options,
                bound,
                turns,
                info.facing,
                image.timestamp_ns(),
            );
            drop(image);
            self.deliver(Ok(frame));
        }
    }

    #[cfg(feature = "camera-capture")]
    fn still_image(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let Some(still) = open.streams.as_ref().and_then(|s| s.still.as_ref()) else {
            return;
        };
        let Some(image) = still.latest() else {
            return;
        };
        let Some(active) = self
            .captures
            .active()
            .filter(|p| p.phase == Phase::Requested)
        else {
            return; // Not asked for (a late frame): released.
        };
        let options = active.options.clone();
        let info = &self.cameras[open.camera];
        let turns = self.turns(info);
        let timestamp = image.timestamp_ns();
        let result = match options.format {
            FrameFormat::Jpeg => match image.plane(0) {
                // `JPEG_ORIENTATION` was set when upright (section 10's risk:
                // a HAL may only write the EXIF tag).
                Some((bytes, _, _)) => Ok(Frame::new(
                    image.size(),
                    FrameFormat::Jpeg,
                    bytes.to_vec(),
                    if options.upright { 0 } else { turns },
                    info.facing,
                    timestamp,
                )),
                None => Err(CameraError::Failed("empty JPEG".into())),
            },
            _ => match yuv_planes(&image) {
                Some(planes) => {
                    let bound = (planes.width as u32, planes.height as u32);
                    Ok(frame_from_planes(
                        &planes,
                        &options,
                        bound,
                        turns,
                        info.facing,
                        timestamp,
                    ))
                }
                None => Err(CameraError::Failed("no YUV planes".into())),
            },
        };
        drop(image);
        self.deliver(result);
    }

    /// Answer the active capture.
    #[cfg(feature = "camera-capture")]
    fn deliver(&mut self, result: Result<Frame, CameraError>) {
        if let Some(p) = self.captures.finish() {
            match &result {
                Ok(frame) => log::info!(
                    "libmobile: capture {} done ({} × {}, {:?})",
                    p.id.id(),
                    frame.width,
                    frame.height,
                    frame.format
                ),
                Err(error) => log::warn!("libmobile: capture {} failed: {error}", p.id.id()),
            }
            emit(CameraEvent::Frame {
                capture: p.id,
                result,
            });
        }
    }

    /// End who `who` names with `error`.
    fn fail(&mut self, who: Who, error: CameraError) {
        if who == Who::All && self.session.as_ref().is_some_and(LiveSession::wants_camera) {
            self.end_session(Some(error.clone()));
        }
        #[cfg(feature = "camera-capture")]
        if matches!(who, Who::Capture | Who::All) && self.captures.active().is_some() {
            self.deliver(Err(error));
        }
    }

    // ── Reconciling ──────────────────────────────────────────────────────────

    fn tick(&mut self, now: Instant) {
        #[cfg(feature = "camera-capture")]
        for capture in self.captures.expire(now) {
            log::warn!("libmobile: capture {} timed out", capture.id());
            emit(CameraEvent::Frame {
                capture,
                result: Err(CameraError::Timeout),
            });
        }
        self.reconcile(now);
        #[cfg(feature = "camera-capture")]
        if self.captures.ready_to_shoot(now, self.settled()) {
            self.shoot();
        }
    }

    /// Whether 3A has settled on the current streams.
    #[cfg(feature = "camera-capture")]
    fn settled(&self) -> bool {
        let has_af = self
            .open
            .as_ref()
            .is_some_and(|o| self.cameras[o.camera].has_af());
        self.three_a
            .is_some_and(|(ae, af)| geometry::three_a_settled(ae, af, has_af))
    }

    fn reconcile(&mut self, now: Instant) {
        if !self.resumed {
            self.close();
            return;
        }
        loop {
            match self.desired() {
                Err((who, error)) => self.fail(who, error),
                Ok(None) => {
                    self.close();
                    return;
                }
                Ok(Some(config)) => match self.ensure(config) {
                    Ok(()) => break,
                    Err((who, error)) => {
                        log::warn!("libmobile: camera could not open: {error}");
                        if who == Who::All {
                            self.close();
                        }
                        self.fail(who, error);
                    }
                },
            }
        }
        self.after_open(now);
    }

    /// The configuration the session and the oldest capture need.
    fn desired(&mut self) -> Result<Option<Config>, (Who, CameraError)> {
        let session = self
            .session
            .as_ref()
            .filter(|s| s.wants_camera())
            .map(|s| s.facing);
        #[cfg(feature = "camera-capture")]
        let capture = self.captures.active().map(|p| p.options.clone());
        #[cfg(not(feature = "camera-capture"))]
        let capture: Option<()> = None;
        #[cfg(feature = "camera-capture")]
        if let (Some(facing), Some(c)) = (session, &capture)
            && c.facing != facing
        {
            return Err((Who::Capture, CameraError::InUse));
        }
        #[cfg(feature = "camera-capture")]
        let facing = session.or(capture.as_ref().map(|c| c.facing));
        #[cfg(not(feature = "camera-capture"))]
        let facing = session;
        let Some(facing) = facing else {
            return Ok(None);
        };
        self.list_cameras().map_err(|e| (Who::All, e))?;
        let camera = self
            .cameras
            .iter()
            .position(|c| c.facing == facing)
            .ok_or((Who::All, CameraError::NoCamera))?;
        let info = &self.cameras[camera];
        let yuv = geometry::sizes_for(&info.configs, FORMAT_YUV);
        let analysis = if session.is_some() {
            geometry::analysis_size(&yuv, self.display.1)
        } else {
            geometry::smallest(&yuv)
        }
        .ok_or((Who::All, CameraError::Unsupported("YUV_420_888")))?;
        #[cfg(feature = "camera-capture")]
        let still = match &capture {
            None => None,
            Some(options) => still_spec(info, options, session.is_some().then_some(analysis))
                .map_err(|e| (Who::Capture, e))?,
        };
        #[cfg(not(feature = "camera-capture"))]
        let still = {
            let _ = capture;
            None
        };
        Ok(Some(Config {
            camera,
            analysis,
            still,
        }))
    }

    fn list_cameras(&mut self) -> Result<(), CameraError> {
        if !self.cameras.is_empty() {
            return Ok(());
        }
        if self.manager.is_none() {
            self.manager = Manager::new();
        }
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| CameraError::Failed("no camera manager".into()))?;
        let ids = manager
            .camera_ids()
            .map_err(|s| CameraError::Failed(format!("camera list: status {s}")))?;
        self.cameras = ids
            .into_iter()
            .filter_map(|id| {
                let meta = manager.characteristics(&id).ok()?;
                CameraInfo::from_metadata(id, &meta, self.api)
            })
            .collect();
        log::info!("libmobile: {} camera(s) by facing", self.cameras.len());
        Ok(())
    }

    /// Open, reconfigure or keep the camera so it matches `config`.
    fn ensure(&mut self, config: Config) -> Result<(), (Who, CameraError)> {
        if let Some(open) = &self.open {
            if open.camera != config.camera {
                self.close();
            } else if open.streams.as_ref().is_some_and(|s| s.config == config) {
                return Ok(());
            }
        }
        if self.open.is_none() {
            self.read_display();
            let manager = self
                .manager
                .as_ref()
                .ok_or((Who::All, CameraError::NoCamera))?;
            let epoch = self.next_epoch + 1;
            self.next_epoch = epoch;
            let device = manager
                .open(&self.cameras[config.camera].id, epoch)
                .map_err(|s| (Who::All, open_error(s)))?;
            log::info!(
                "libmobile: camera {:?} open",
                self.cameras[config.camera].facing
            );
            self.open = Some(Open {
                streams: None,
                device,
                device_epoch: epoch,
                camera: config.camera,
            });
        }
        // New streams: the old session closes before its readers go.
        if let Some(open) = self.open.as_mut() {
            open.streams = None;
        }
        #[cfg(feature = "camera-capture")]
        self.captures.requeue();
        self.three_a = None;
        let epoch = self.next_epoch();
        let who = if config.still.is_some() {
            #[cfg(feature = "camera-capture")]
            {
                Who::Capture
            }
            #[cfg(not(feature = "camera-capture"))]
            {
                Who::All
            }
        } else {
            Who::All
        };
        let failed =
            |what: &str, s: ndk::Status| CameraError::Failed(format!("{what}: status {s}"));
        let analysis = Reader::new(
            config.analysis,
            FORMAT_YUV,
            IMAGE_READER_MAX,
            ReaderKind::Analysis,
            epoch,
        )
        .map_err(|s| (Who::All, failed("analysis reader", s)))?;
        let still = match config.still {
            Some(spec) => Some(
                Reader::new(spec.size, spec.format, 2, ReaderKind::Still, epoch)
                    .map_err(|s| (who, failed("still reader", s)))?,
            ),
            None => None,
        };
        let open = self
            .open
            .as_mut()
            .ok_or((Who::All, CameraError::NoCamera))?;
        let mut readers = vec![&analysis];
        readers.extend(still.as_ref());
        let session = open
            .device
            .session(&readers)
            .map_err(|_| (who, CameraError::Unsupported("stream combination")))?;
        open.streams = Some(Streams {
            session,
            still,
            analysis,
            epoch,
            config,
        });
        #[cfg(feature = "camera-qr")]
        self.qr_prepare(config.camera);
        self.repeat()
            .map_err(|s| (who, failed("repeating request", s)))?;
        Ok(())
    }

    /// The zoom the camera should use now.
    fn zoom_ratio(&self, info: &CameraInfo) -> f32 {
        let ratio = self
            .session
            .as_ref()
            .filter(|s| s.wants_camera())
            .map_or(1.0, |s| s.user_zoom * s.auto_zoom());
        info.zoom.clamp(ratio)
    }

    /// The controls every request carries: AF, AE, antibanding and zoom.
    fn controls(&self, request: &mut ndk::Request, info: &CameraInfo) -> Result<(), ndk::Status> {
        request.set_u8(tag::AF_MODE, &[info.af_mode])?;
        request.set_u8(tag::AE_MODE, &[1])?; // ON
        request.set_u8(tag::AE_ANTIBANDING_MODE, &[3])?; // AUTO
        let ratio = self.zoom_ratio(info);
        match info.zoom {
            ZoomControl::Ratio { .. } => request.set_f32(tag::ZOOM_RATIO, &[ratio])?,
            ZoomControl::Crop { active, .. } => {
                let c = geometry::crop_region(active, ratio);
                request.set_i32(tag::CROP_REGION, &[c.x, c.y, c.w, c.h])?
            }
        }
        #[cfg(feature = "camera-qr")]
        self.qr_controls(request, info, ratio)?;
        Ok(())
    }

    /// A QR session's exposure compensation and metering and focus regions
    /// (plan 6.3, 6.4, 7.2). A region not set is left to the camera (the
    /// whole field).
    #[cfg(feature = "camera-qr")]
    fn qr_controls(
        &self,
        request: &mut ndk::Request,
        info: &CameraInfo,
        ratio: f32,
    ) -> Result<(), ndk::Status> {
        let Some(c) = self.session.as_ref().and_then(LiveSession::control) else {
            return Ok(());
        };
        let Some((w, h)) = self
            .open
            .as_ref()
            .and_then(|o| o.streams.as_ref())
            .map(|s| s.config.analysis)
        else {
            return Ok(());
        };
        let aspect = w as f32 / h.max(1) as f32;
        if info.ev_step > 0.0 {
            let steps = control::ev_steps(c.ev(), info.ev_range, info.ev_step);
            request.set_i32(tag::AE_EXPOSURE_COMPENSATION, &[steps])?;
        }
        if let Some(area) = c.ae_region().filter(|_| info.max_regions.0 > 0) {
            request.set_i32(
                tag::AE_REGIONS,
                &geometry::region(&info.zoom, ratio, area, aspect),
            )?;
        }
        if let Some(area) = c.af_region().filter(|_| info.max_regions.1 > 0) {
            request.set_i32(
                tag::AF_REGIONS,
                &geometry::region(&info.zoom, ratio, area, aspect),
            )?;
        }
        Ok(())
    }

    /// (Re)start the repeating preview request on the analysis stream.
    fn repeat(&self) -> Result<(), ndk::Status> {
        let Some(open) = &self.open else {
            return Ok(());
        };
        let Some(streams) = &open.streams else {
            return Ok(());
        };
        let info = &self.cameras[open.camera];
        let mut request = open.device.request(Template::Preview)?;
        request.add_target(&streams.analysis)?;
        self.controls(&mut request, info)?;
        streams.session.repeat(&request, streams.epoch)
    }

    /// After a (re)configuration: a session waiting to open reports
    /// `Opened`; a queued capture starts settling.
    fn after_open(&mut self, now: Instant) {
        let Some(open) = &self.open else {
            return;
        };
        let Some(streams) = &open.streams else {
            return;
        };
        let info = &self.cameras[open.camera];
        let turns = self.turns(info);
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| matches!(s.phase, SessionPhase::Ready | SessionPhase::Suspended))
        {
            s.phase = SessionPhase::Open;
            s.user_zoom = info.zoom.clamp(s.user_zoom);
            let range = info.zoom.range();
            let (w, h) = streams.config.analysis;
            // Without a preview: the analysis frame's size, upright.
            let preview = s.preview.unwrap_or(PreviewOptions {
                max_side: w.max(h),
                upright: true,
            });
            let factor = convert::scale_factor(w as usize, h as usize, preview.max_side) as u32;
            let (mut w, mut h) = (w / factor, h / factor);
            if preview.upright && turns % 2 == 1 {
                std::mem::swap(&mut w, &mut h);
            }
            {
                let mut shared = lock(&SHARED);
                if shared.session == Some(s.id) {
                    shared.range = Some(range);
                    shared.zoom = Some(s.user_zoom);
                }
            }
            log::info!("libmobile: camera session {} open ({w} × {h})", s.id.id());
            emit(CameraEvent::Opened {
                session: s.id,
                facing: s.facing,
                size: (w, h),
                zoom_range: range,
            });
            if s.user_zoom != 1.0 {
                emit(CameraEvent::Zoom {
                    session: s.id,
                    ratio: s.user_zoom,
                    source: ZoomSource::App,
                });
            }
        }
        #[cfg(feature = "camera-capture")]
        if self
            .captures
            .active()
            .is_some_and(|p| p.phase == Phase::Queued)
        {
            self.captures.opened(now);
        }
        #[cfg(not(feature = "camera-capture"))]
        let _ = now;
    }

    /// Take the active capture: a still request, or the next analysis frame.
    #[cfg(feature = "camera-capture")]
    fn shoot(&mut self) {
        let Some(options) = self.captures.active().map(|p| p.options.clone()) else {
            return;
        };
        let Some(open) = &self.open else {
            return;
        };
        let Some(streams) = &open.streams else {
            return;
        };
        let Some(still) = &streams.still else {
            self.captures.requested();
            return;
        };
        let sent = self.send_still(&options, still);
        match sent {
            Ok(()) => self.captures.requested(),
            Err(status) => self.fail(
                Who::Capture,
                CameraError::Failed(format!("still request: status {status}")),
            ),
        }
    }

    /// A still request for `options` to the still reader.
    #[cfg(feature = "camera-capture")]
    fn send_still(&self, options: &CaptureOptions, still: &Reader) -> Result<(), ndk::Status> {
        let Some(open) = &self.open else {
            return Ok(());
        };
        let Some(streams) = &open.streams else {
            return Ok(());
        };
        let info = &self.cameras[open.camera];
        let mut request = open.device.request(Template::Still)?;
        request.add_target(still)?;
        self.controls(&mut request, info)?;
        if options.format == FrameFormat::Jpeg {
            let degrees = if options.upright {
                self.turns(info) as i32 * 90
            } else {
                0
            };
            request.set_i32(tag::JPEG_ORIENTATION, &[degrees])?;
            request.set_u8(tag::JPEG_QUALITY, &[options.jpeg_quality.clamp(1, 100)])?;
        }
        streams.session.capture(&request, streams.epoch)
    }

    // ── QR scanning ──────────────────────────────────────────────────────────

    /// New streams for a QR session about to open (or reopen after a pause):
    /// its controller starts over with this camera's limits, so the first
    /// repeating request already carries `ScreenExposure::Screen`'s start.
    #[cfg(feature = "camera-qr")]
    fn qr_prepare(&mut self, camera: usize) {
        let info = &self.cameras[camera];
        let Some(s) = self
            .session
            .as_mut()
            .filter(|s| matches!(s.phase, SessionPhase::Ready | SessionPhase::Suspended))
        else {
            return;
        };
        let user = info.zoom.clamp(s.user_zoom);
        if let Some(q) = s.qr.as_mut() {
            let limits = info.qr_limits(user);
            q.control = Some(Controller::new(
                q.options.exposure,
                q.options.auto_zoom,
                &limits,
            ));
            q.dedup = Default::default();
            q.last_submit = None;
            log::info!(
                "libmobile: QR session {} ({:?}, auto zoom {}, {:?} exposure, EV floor {}, {} zoom)",
                s.id.id(),
                q.options.mode,
                q.options.auto_zoom,
                q.options.exposure,
                limits.ev_floor,
                if limits.fine { "fine" } else { "coarse" },
            );
        }
    }

    /// The QR thread's result: report the codes read, then let auto zoom and
    /// exposure make at most one change.
    #[cfg(feature = "camera-qr")]
    fn qr_scan(&mut self, session: SessionId, epoch: u64, scan: &Scan) {
        let now = Instant::now();
        let Some(open) = &self.open else {
            return;
        };
        if open.streams.as_ref().map(|s| s.epoch) != Some(epoch) {
            return; // A frame of older streams.
        }
        let info = &self.cameras[open.camera];
        let turns = geometry::quarter_turns(info.sensor, self.display.0, info.facing);
        let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.id == session && s.phase == SessionPhase::Open)
        else {
            return;
        };
        let limits = info.qr_limits(s.user_zoom);
        let Some(q) = s.qr.as_mut() else {
            return;
        };
        let mut reported = false;
        for read in &scan.codes {
            let bytes = &read.bytes;
            if !q.dedup.admit(bytes, now) {
                continue;
            }
            // Never the payload itself: it may be a pairing secret.
            log::info!(
                "libmobile: QR code on session {} ({} bytes, version {})",
                session.id(),
                bytes.len(),
                read.version
            );
            emit(CameraEvent::Qr {
                session,
                code: QrCode::new(
                    bytes.clone(),
                    read.corners,
                    read.version,
                    (scan.width, scan.height),
                    turns,
                ),
            });
            reported = true;
            if q.options.mode == QrMode::Once {
                break;
            }
        }
        if reported && q.options.mode == QrMode::Once {
            self.end_session(None);
            return;
        }
        let Some(ctl) = q.control.as_mut() else {
            return;
        };
        let Some(change) = ctl.step(scan, now, &limits) else {
            return;
        };
        let (auto, ev) = (ctl.auto(), ctl.ev());
        let applied = self.repeat();
        if let Err(status) = applied {
            log::warn!("libmobile: QR {change:?} not applied: status {status}");
            return;
        }
        match change {
            Change::Zoom => {
                let ratio = self.zoom_ratio(info);
                log::info!("libmobile: QR auto zoom ×{auto:.2}: ratio {ratio:.2}");
                let before = lock(&SHARED).zoom.replace(ratio);
                if before == Some(ratio) {
                    return; // At the device's limit: nothing changed.
                }
                emit(CameraEvent::Zoom {
                    session,
                    ratio,
                    source: ZoomSource::Auto,
                });
            }
            Change::Exposure => {
                let steps = control::ev_steps(ev, info.ev_range, info.ev_step);
                log::info!("libmobile: QR exposure {ev:+.1} EV ({steps} steps)");
            }
            Change::Focus(trigger) => {
                if let Err(status) = self.af_trigger(trigger) {
                    log::warn!("libmobile: QR AF trigger not sent: status {status}");
                }
            }
        }
    }

    /// Send `ACAMERA_CONTROL_AF_TRIGGER` once on the analysis stream. A start
    /// is preceded by a cancel: in the continuous AF modes a start locks
    /// focus, and only a cancel unlocks it for the next start.
    #[cfg(feature = "camera-qr")]
    fn af_trigger(&self, trigger: AfTrigger) -> Result<(), ndk::Status> {
        let Some(open) = &self.open else {
            return Ok(());
        };
        let Some(streams) = &open.streams else {
            return Ok(());
        };
        let info = &self.cameras[open.camera];
        let request = |value: u8| -> Result<ndk::Request, ndk::Status> {
            let mut request = open.device.request(Template::Preview)?;
            request.add_target(&streams.analysis)?;
            self.controls(&mut request, info)?;
            // CONTROL_AF_TRIGGER: START 1, CANCEL 2.
            request.set_u8(tag::AF_TRIGGER, &[value])?;
            Ok(request)
        };
        let cancel = request(2)?;
        match trigger {
            AfTrigger::Cancel => streams.session.trigger(&[&cancel]),
            AfTrigger::Start => streams.session.trigger(&[&cancel, &request(1)?]),
        }
    }

    fn close(&mut self) {
        if self.open.take().is_some() {
            log::info!("libmobile: camera closed");
        }
        self.three_a = None;
    }
}

fn open_error(status: ndk::Status) -> CameraError {
    match status {
        status::PERMISSION_DENIED => CameraError::PermissionDenied {
            settings_only: false,
        },
        status::CAMERA_IN_USE | status::MAX_CAMERA_IN_USE | status::CAMERA_DISCONNECTED => {
            CameraError::InUse
        }
        status::CAMERA_DISABLED => CameraError::Disabled,
        other => CameraError::Failed(format!("open camera: status {other}")),
    }
}

/// The three YUV_420_888 planes of `image`.
fn yuv_planes(image: &ndk::Image) -> Option<Planes<'_>> {
    let (w, h) = image.size();
    let (y, y_row, _) = image.plane(0)?;
    let (u, uv_row, uv_pixel) = image.plane(1)?;
    let (v, _, _) = image.plane(2)?;
    Some(Planes {
        width: w as usize,
        height: h as usize,
        y,
        y_row,
        u,
        v,
        uv_row,
        uv_pixel: uv_pixel.max(1),
    })
}

/// The size a capture asks for, from the sizes offered for its format.
#[cfg(feature = "camera-capture")]
fn requested_size(info: &CameraInfo, options: &CaptureOptions) -> Result<(u32, u32), CameraError> {
    let format = match options.format {
        FrameFormat::Jpeg => FORMAT_JPEG,
        _ => FORMAT_YUV,
    };
    let sizes = geometry::sizes_for(&info.configs, format);
    if sizes.is_empty() {
        return Err(CameraError::Unsupported("format"));
    }
    match options.resolution {
        Resolution::Preview => geometry::largest_within(&sizes, PREVIEW_MAX),
        Resolution::Max => geometry::largest(&sizes),
        Resolution::AtMost { width, height } => geometry::largest_within(&sizes, (width, height)),
    }
    .ok_or(CameraError::Unsupported("resolution"))
}

/// The still reader a capture needs, or `None` when a running session's
/// analysis stream (`analysis`) serves it: Gray8 or Rgba8 at or below its
/// size.
#[cfg(feature = "camera-capture")]
fn still_spec(
    info: &CameraInfo,
    options: &CaptureOptions,
    analysis: Option<(u32, u32)>,
) -> Result<Option<StillSpec>, CameraError> {
    let size = requested_size(info, options)?;
    if options.format == FrameFormat::Jpeg {
        return Ok(Some(StillSpec {
            format: FORMAT_JPEG,
            size,
        }));
    }
    if analysis.is_some_and(|a| geometry::fits(size, a)) {
        return Ok(None);
    }
    Ok(Some(StillSpec {
        format: FORMAT_YUV,
        size,
    }))
}

/// A Gray8 or Rgba8 frame from YUV planes, scaled by a whole factor to fit
/// `bound` and turned upright when asked.
#[cfg(feature = "camera-capture")]
fn frame_from_planes(
    planes: &Planes<'_>,
    options: &CaptureOptions,
    bound: (u32, u32),
    turns: u8,
    facing: Facing,
    timestamp_ns: i64,
) -> Frame {
    let factor = planes
        .width
        .div_ceil(bound.0.max(1) as usize)
        .max(planes.height.div_ceil(bound.1.max(1) as usize))
        .max(1);
    let (width, height) = (planes.width / factor, planes.height / factor);
    let (data, bpp, format) = match (options.format, factor) {
        (FrameFormat::Gray8, 1) => (convert::pack_gray(planes), 1, FrameFormat::Gray8),
        (FrameFormat::Gray8, _) => (convert::to_i420(planes, factor).y, 1, FrameFormat::Gray8),
        (_, 1) => (convert::to_rgba(planes), 4, FrameFormat::Rgba8),
        _ => {
            let i420 = convert::to_i420(planes, factor);
            let rgba = convert::to_rgba(&Planes {
                width: i420.width,
                height: i420.height,
                y: &i420.y,
                y_row: i420.width,
                u: &i420.u,
                v: &i420.v,
                uv_row: i420.width.div_ceil(2),
                uv_pixel: 1,
            });
            (rgba, 4, FrameFormat::Rgba8)
        }
    };
    let (data, width, height, quarter_turns) = if options.upright {
        let (data, w, h) = convert::rotate(&data, width, height, bpp, turns);
        (data, w, h, 0)
    } else {
        (data, width, height, turns)
    };
    Frame::new(
        (width as u32, height as u32),
        format,
        data,
        quarter_turns,
        facing,
        timestamp_ns,
    )
}
