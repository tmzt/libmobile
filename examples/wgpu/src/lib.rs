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

#[cfg(target_os = "android")]
mod gpu;
pub mod uniforms;

#[cfg(target_os = "android")]
mod app {
    use crate::gpu::{Gpu, window_changed};
    use crate::uniforms::Uniforms;
    use libmobile::MobileEvent;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::{AndroidApp, InputStatus, MobileApp, run};
    use std::time::{Duration, Instant};

    struct WgpuApp {
        gpu: Option<Gpu>,
        resumed: bool,
        uniforms: Uniforms,
        start: Instant,
        last_tick: Instant,
        /// A finger is on the screen.
        down: bool,
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
                _ => {}
            }
        }

        fn input(&mut self, _app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
            let InputEvent::MotionEvent(motion) = event else {
                return InputStatus::Unhandled;
            };
            let pointer = motion.pointer_at_index(motion.pointer_index());
            match motion.action() {
                MotionAction::Down | MotionAction::Move | MotionAction::PointerDown => {
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
                .with_tag("libmobile-wgpu"),
        );
        run(
            app,
            WgpuApp {
                gpu: None,
                resumed: false,
                uniforms: Uniforms::default(),
                start: Instant::now(),
                last_tick: Instant::now(),
                down: false,
            },
        );
    }
}
