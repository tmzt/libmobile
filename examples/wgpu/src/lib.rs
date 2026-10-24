//! **libmobile wgpu example**: a full-screen WGSL shader (`shader.wgsl`)
//! through wgpu, fed the surface size, the safe area libmobile reports, and
//! the last touch.
//!
//! The part worth copying is the surface lifecycle:
//! * the device and pipeline are built once and survive backgrounding;
//! * `WindowCreated` builds a swapchain against the new window
//!   (`SurfaceTarget`, so wgpu's *safe* `create_surface` works);
//! * `WindowDestroyed` drops it — synchronously, because libmobile delivers
//!   that event inside the poll callback, before Android takes the window back.
//!
//! A tap, or `--es cmd open-link`, opens [`LINK`] in the browser
//! (`android::link::open_link`); `--es cmd open-link-later` tries again from
//! a thread after 5 s, so that a call while the app is hidden can be tried.
//!
//! At start it logs the app's files and cache directories
//! (`android::dirs`), and whether the files directory is
//! `AndroidApp::internal_data_path()`, and the device's manufacturer, brand
//! and model (`android::build_info`).

/// The link a tap opens. Its query stands for a secret: libmobile's log cuts
/// it off.
pub const LINK: &str = "https://example.com/a?k=secret";

#[cfg(target_os = "android")]
mod gpu;
pub mod uniforms;

#[cfg(target_os = "android")]
mod app {
    use crate::gpu::{Gpu, window_changed};
    use crate::uniforms::Uniforms;
    use libmobile::MobileEvent;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::dirs;
    use libmobile::android::link::open_link;
    use libmobile::android::{AndroidApp, InputStatus, MobileApp, build_info, run};
    use std::time::{Duration, Instant};

    /// A press shorter than this, that moved less than [`TAP_SLOP`], is a tap.
    const TAP_TIME: Duration = Duration::from_millis(300);
    const TAP_SLOP: f32 = 24.0;

    /// Log `android::dirs` against `internal_data_path`, here and from
    /// another thread.
    fn log_dirs(app: &AndroidApp) {
        let files = dirs::files_dir();
        let internal = app.internal_data_path();
        log::info!("wgpu example: files_dir {files:?}");
        log::info!("wgpu example: cache_dir {:?}", dirs::cache_dir());
        log::info!("wgpu example: internal_data_path {internal:?}");
        log::info!(
            "wgpu example: files_dir == internal_data_path: {}",
            files.is_some() && files == internal
        );
        std::thread::spawn(|| {
            log::info!(
                "wgpu example: from a thread: files_dir {:?}, cache_dir {:?}",
                dirs::files_dir(),
                dirs::cache_dir()
            );
        });
    }

    fn open() {
        log::info!("wgpu example: open link: {:?}", open_link(crate::LINK));
    }

    struct WgpuApp {
        gpu: Option<Gpu>,
        resumed: bool,
        uniforms: Uniforms,
        start: Instant,
        last_tick: Instant,
        /// A finger is on the screen.
        down: bool,
        /// Where and when the press started, while it can still be a tap.
        press: Option<(Instant, [f32; 2])>,
    }

    impl MobileApp for WgpuApp {
        fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
            match event {
                MobileEvent::WindowCreated | MobileEvent::WindowResized => {
                    window_changed(&mut self.gpu, &mut self.uniforms, app);
                }
                MobileEvent::WindowDestroyed => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.swapchain = None;
                    }
                }
                MobileEvent::Resume => self.resumed = true,
                MobileEvent::Pause => self.resumed = false,
                MobileEvent::Insets(insets) => self.uniforms.set_safe_area(insets.safe_area),
                // A `libmobile-wgpu://` link (see build.rs). Logged redacted:
                // its query or fragment may be a secret.
                MobileEvent::Intent(intent) => {
                    if let Some(url) = intent.data.as_deref() {
                        log::info!(
                            "wgpu example: opened with {} (is_new: {})",
                            libmobile::intent::redact_url(url),
                            intent.is_new
                        );
                    }
                    match intent.extra("cmd") {
                        Some("open-link") => open(),
                        Some("open-link-later") => {
                            std::thread::spawn(|| {
                                std::thread::sleep(Duration::from_secs(5));
                                open();
                            });
                        }
                        Some(other) => log::warn!("wgpu example: unknown cmd {other:?}"),
                        None => {}
                    }
                }
                _ => {}
            }
        }

        fn input(&mut self, _app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
            let InputEvent::MotionEvent(motion) = event else {
                return InputStatus::Unhandled;
            };
            let pointer = motion.pointer_at_index(motion.pointer_index());
            let at = [pointer.x(), pointer.y()];
            match motion.action() {
                MotionAction::Down => {
                    self.press = Some((Instant::now(), at));
                    self.uniforms.touch = at;
                    self.down = true;
                }
                MotionAction::Move | MotionAction::PointerDown => {
                    let moved = self.press.is_some_and(|(_, from)| {
                        (at[0] - from[0]).hypot(at[1] - from[1]) > TAP_SLOP
                    });
                    if moved || motion.action() == MotionAction::PointerDown {
                        self.press = None;
                    }
                    self.uniforms.touch = at;
                    self.down = true;
                }
                MotionAction::Up => {
                    if self
                        .press
                        .take()
                        .is_some_and(|(when, _)| when.elapsed() < TAP_TIME)
                    {
                        open();
                    }
                    self.down = false;
                }
                MotionAction::Cancel => {
                    self.press = None;
                    self.down = false;
                }
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
                .with_max_level(log::LevelFilter::Debug)
                // libmobile's `open link` line is at debug level.
                .with_filter(
                    android_logger::FilterBuilder::new()
                        .parse("info,libmobile::android::link=debug")
                        .build(),
                )
                .with_tag("libmobile-wgpu"),
        );
        log_dirs(&app);
        log::info!("wgpu example: build_info {:?}", build_info());
        run(
            app,
            WgpuApp {
                gpu: None,
                resumed: false,
                uniforms: Uniforms::default(),
                start: Instant::now(),
                last_tick: Instant::now(),
                down: false,
                press: None,
            },
        );
    }
}
