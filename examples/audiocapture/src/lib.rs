//! **libmobile audiocapture example**: the `wgpu` example's plasma, made to
//! respond to the microphone through `android::CaptureStream`.
//!
//! * On the first window it asks for `RECORD_AUDIO` if it is not granted
//!   (`request_permissions`), logging the request id and the answer. After a
//!   denial it logs `should_show_rationale`, and a tap asks again.
//! * Once allowed, it records with AAudio's default preset. Each block's RMS
//!   level goes to the main loop through an atomic and, eased, drives the
//!   shader's `level`: rings from the centre and a brighter picture.
//! * Once recording, a tap cycles the input preset (`set_preset`), logging
//!   the one asked for and the one granted. The stream stops on `Pause` and
//!   starts again on `Resume`.
//!
//! The device, pipeline and shader are the `wgpu` example's own files,
//! included with `#[path]`.

#[cfg(target_os = "android")]
#[path = "../../wgpu/src/gpu.rs"]
mod gpu;
#[path = "../../wgpu/src/uniforms.rs"]
pub mod uniforms;

/// The microphone level, kept apart from Android so it is tested on the host.
pub mod level {
    /// Scales a block's RMS so ordinary speech near the phone reaches
    /// about 1.
    pub const GAIN: f32 = 12.0;

    /// Root mean square of `samples`; 0 for none.
    pub fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Move `current` toward `target`: quick to rise (about 30 ms), slower
    /// to fall (about 300 ms), so speech reads as pulses rather than flicker.
    pub fn ease(current: f32, target: f32, dt: f32) -> f32 {
        let time = if target > current { 0.03 } else { 0.3 };
        current + (target - current) * (1.0 - (-dt / time).exp())
    }

    #[cfg(test)]
    mod tests {
        use super::{ease, rms};

        #[test]
        fn rms_of_a_square_wave_is_its_amplitude() {
            assert_eq!(rms(&[]), 0.0);
            assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
        }

        #[test]
        fn rises_fast_and_falls_slowly() {
            let up = ease(0.0, 1.0, 0.03);
            assert!(up > 0.6, "{up}");
            let down = ease(1.0, 0.0, 0.03);
            assert!(down > 0.9, "{down}");
        }
    }
}

#[cfg(target_os = "android")]
mod app {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    use crate::gpu::{Gpu, window_changed};
    use crate::level::{GAIN, ease, rms};
    use crate::uniforms::Uniforms;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::{
        AndroidApp, CaptureStream, InputStatus, MobileApp, permission_granted, request_permissions,
        run, should_show_rationale,
    };
    use libmobile::{InputPreset, MobileEvent, PermissionRequest};

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

    /// What a tap cycles through once recording; `None` lets AAudio choose.
    const PRESETS: [Option<InputPreset>; 7] = [
        None,
        Some(InputPreset::Generic),
        Some(InputPreset::Camcorder),
        Some(InputPreset::VoiceRecognition),
        Some(InputPreset::VoiceCommunication),
        Some(InputPreset::Unprocessed),
        Some(InputPreset::VoicePerformance),
    ];

    struct CaptureApp {
        gpu: Option<Gpu>,
        resumed: bool,
        uniforms: Uniforms,
        start: Instant,
        last_tick: Instant,
        /// A finger is on the screen.
        down: bool,
        capture: Option<CaptureStream>,
        /// The last block's RMS, as `f32` bits, written on AAudio's thread.
        rms: Arc<AtomicU32>,
        /// Our `RECORD_AUDIO` request awaiting its answer.
        request: Option<PermissionRequest>,
        /// Whether the first request has been made.
        asked: bool,
        /// The place in `PRESETS` last asked for.
        preset: usize,
    }

    impl CaptureApp {
        /// Record if allowed; otherwise ask, unless a request is waiting.
        fn ensure_capture(&mut self, app: &AndroidApp) {
            if self.capture.is_some() || self.request.is_some() {
                return;
            }
            if permission_granted(app, RECORD_AUDIO) {
                self.open_capture();
                return;
            }
            let request = request_permissions(app, &[RECORD_AUDIO]);
            log::info!(
                "audiocapture: asked for the microphone (request {})",
                request.id()
            );
            self.request = Some(request);
            self.asked = true;
        }

        fn open_capture(&mut self) {
            let rms_out = self.rms.clone();
            let opened = CaptureStream::open(None, move |samples, _time| {
                rms_out.store(rms(samples).to_bits(), Ordering::Relaxed);
            });
            match opened {
                Ok(stream) => {
                    log::info!(
                        "audiocapture: recording {:?}, preset {:?}",
                        stream.format(),
                        stream.preset()
                    );
                    if self.resumed
                        && let Err(error) = stream.start()
                    {
                        log::error!("audiocapture: start: {error}");
                    }
                    self.preset = 0;
                    self.capture = Some(stream);
                }
                Err(error) => log::error!("audiocapture: open: {error}"),
            }
        }

        fn next_preset(&mut self) {
            let Some(stream) = &self.capture else {
                return;
            };
            self.preset = (self.preset + 1) % PRESETS.len();
            let asked = PRESETS[self.preset];
            match stream.set_preset(asked) {
                Ok(granted) => log::info!("audiocapture: preset: asked {asked:?}, got {granted:?}"),
                Err(error) => log::error!("audiocapture: preset {asked:?}: {error}"),
            }
        }
    }

    impl MobileApp for CaptureApp {
        fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
            match event {
                MobileEvent::WindowCreated | MobileEvent::WindowResized => {
                    window_changed(&mut self.gpu, &mut self.uniforms, app);
                    if !self.asked {
                        self.ensure_capture(app);
                    }
                }
                MobileEvent::WindowDestroyed => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.swapchain = None;
                    }
                }
                MobileEvent::Resume => {
                    self.resumed = true;
                    if let Some(stream) = &self.capture
                        && let Err(error) = stream.start()
                    {
                        log::error!("audiocapture: start: {error}");
                    }
                }
                MobileEvent::Pause => {
                    self.resumed = false;
                    if let Some(stream) = &self.capture
                        && let Err(error) = stream.stop()
                    {
                        log::error!("audiocapture: stop: {error}");
                    }
                }
                MobileEvent::Insets(insets) => self.uniforms.set_safe_area(insets.safe_area),
                MobileEvent::Permissions(outcome) if Some(outcome.request) == self.request => {
                    self.request = None;
                    log::info!(
                        "audiocapture: permissions (request {}) {:?}",
                        outcome.request.id(),
                        outcome.permissions
                    );
                    if outcome.granted(RECORD_AUDIO) {
                        self.open_capture();
                    } else if should_show_rationale(app, RECORD_AUDIO) {
                        log::info!("audiocapture: denied; a tap asks again");
                    } else {
                        log::info!(
                            "audiocapture: denied for good; allow the microphone in Settings"
                        );
                    }
                }
                _ => {}
            }
        }

        fn input(&mut self, app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
            let InputEvent::MotionEvent(motion) = event else {
                return InputStatus::Unhandled;
            };
            let pointer = motion.pointer_at_index(motion.pointer_index());
            match motion.action() {
                MotionAction::Down => {
                    self.uniforms.touch = [pointer.x(), pointer.y()];
                    self.down = true;
                    if self.capture.is_some() {
                        self.next_preset();
                    } else {
                        self.ensure_capture(app);
                    }
                }
                MotionAction::Move | MotionAction::PointerDown => {
                    self.uniforms.touch = [pointer.x(), pointer.y()];
                    self.down = true;
                }
                MotionAction::Up | MotionAction::Cancel => self.down = false,
                _ => {}
            }
            InputStatus::Handled
        }

        fn tick(&mut self, _app: &AndroidApp) {
            if !self.resumed {
                return;
            }
            let now = Instant::now();
            let dt = (now - self.last_tick).as_secs_f32().min(0.1);
            self.last_tick = now;
            self.uniforms.time = (now - self.start).as_secs_f32();
            self.uniforms.ease_strength(self.down, dt);
            let target = (f32::from_bits(self.rms.load(Ordering::Relaxed)) * GAIN).min(1.0);
            self.uniforms.level = ease(self.uniforms.level, target, dt);
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.render(&self.uniforms);
            }
        }

        fn poll_timeout(&self) -> Option<Duration> {
            // Continuous animation: never wait; Fifo present paces to vsync.
            if self.resumed && self.gpu.is_some() {
                Some(Duration::ZERO)
            } else {
                Some(libmobile::android::DEFAULT_POLL_TIMEOUT)
            }
        }
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("libmobile-audiocapture"),
        );
        run(
            app,
            CaptureApp {
                gpu: None,
                resumed: false,
                uniforms: Uniforms::default(),
                start: Instant::now(),
                last_tick: Instant::now(),
                down: false,
                capture: None,
                rms: Arc::default(),
                request: None,
                asked: false,
                preset: 0,
            },
        );
    }
}
