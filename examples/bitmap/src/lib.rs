//! **libmobile bitmap example**: no GPU at all. Every frame is computed on the
//! CPU and written straight into the `ANativeWindow`'s locked buffer.
//!
//! Shows the libmobile plumbing in its plainest form:
//! * `WindowCreated`/`WindowDestroyed` → hold or drop the window;
//! * `Insets` → the hatched edges are the system bars and cutout, reported by
//!   `WindowInsetsPlugin`; the bouncing box stays inside them;
//! * touch input → white dots;
//! * `Intent` → `adb shell am start -n dev.highbay.libmobile.example.bitmap/dev.highbay.libmobile.MobileNativeActivity --es cmd clear`
//!   clears the dots on the running instance.

pub mod scene;

#[cfg(target_os = "android")]
mod app {
    use crate::scene::{BPP, Scene};
    use libmobile::MobileEvent;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::ndk::hardware_buffer_format::HardwareBufferFormat;
    use libmobile::android::ndk::native_window::NativeWindow;
    use libmobile::android::{AndroidApp, InputStatus, MobileApp, run};
    use std::time::{Duration, Instant};

    /// ~30 fps: a software renderer filling a phone-sized buffer has no
    /// business trying for more.
    const FRAME: Duration = Duration::from_millis(33);

    struct BitmapApp {
        window: Option<NativeWindow>,
        resumed: bool,
        scene: Scene,
        start: Instant,
    }

    impl BitmapApp {
        fn attach(&mut self, app: &AndroidApp) {
            let Some(window) = app.native_window() else {
                return;
            };
            // Ask for RGBA8888 at the window's own size, so the byte layout
            // below is known.
            if let Err(err) =
                window.set_buffers_geometry(0, 0, Some(HardwareBufferFormat::R8G8B8A8_UNORM))
            {
                log::error!("bitmap: set_buffers_geometry: {err}");
            }
            self.scene.width = window.width().max(0) as usize;
            self.scene.height = window.height().max(0) as usize;
            log::info!("bitmap: window {}x{}", self.scene.width, self.scene.height);
            self.window = Some(window);
        }

        fn draw(&mut self) {
            let Some(window) = &self.window else {
                return;
            };
            let mut buffer = match window.lock(None) {
                Ok(buffer) => buffer,
                Err(err) => {
                    log::warn!("bitmap: lock failed: {err}");
                    return;
                }
            };
            if buffer.format().bytes_per_pixel() != Some(BPP) {
                log::error!("bitmap: unexpected buffer format {:?}", buffer.format());
                return;
            }
            let t = self.start.elapsed().as_secs_f32();
            let box_origin = self.scene.box_origin(t);
            let Some(lines) = buffer.lines() else {
                return;
            };
            for (y, line) in lines.enumerate() {
                for (x, px) in line.chunks_exact_mut(BPP).enumerate() {
                    let rgba = self.scene.shade(x, y, t, box_origin);
                    for (dst, src) in px.iter_mut().zip(rgba) {
                        dst.write(src);
                    }
                }
            }
            // Dropping the guard unlocks and posts the buffer.
        }
    }

    impl MobileApp for BitmapApp {
        fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
            match event {
                MobileEvent::WindowCreated | MobileEvent::WindowResized => self.attach(app),
                MobileEvent::WindowDestroyed => self.window = None,
                MobileEvent::Resume => self.resumed = true,
                MobileEvent::Pause => self.resumed = false,
                MobileEvent::Insets(insets) => {
                    log::info!("bitmap: safe area {:?}", insets.safe_area);
                    self.scene.safe = insets.safe_area;
                }
                MobileEvent::Intent(intent) => {
                    if intent.extra("cmd") == Some("clear") {
                        self.scene.clear();
                    }
                }
                _ => {}
            }
        }

        fn input(&mut self, _app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
            let InputEvent::MotionEvent(motion) = event else {
                return InputStatus::Unhandled;
            };
            if matches!(motion.action(), MotionAction::Down | MotionAction::Move) {
                for pointer in motion.pointers() {
                    self.scene.touch(pointer.x(), pointer.y());
                }
            }
            InputStatus::Handled
        }

        fn tick(&mut self, _app: &AndroidApp) {
            if self.resumed {
                self.draw();
            }
        }

        fn poll_timeout(&self) -> Option<Duration> {
            Some(FRAME)
        }
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("libmobile-bitmap"),
        );
        run(
            app,
            BitmapApp {
                window: None,
                resumed: false,
                scene: Scene::default(),
                start: Instant::now(),
            },
        );
    }
}
