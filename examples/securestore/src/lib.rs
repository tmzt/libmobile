//! **libmobile securestore example**: the `wgpu` example's plasma, with a
//! launch counter kept in `android::secure_store`.
//!
//! * On the first window it logs the key's `security_level`, reads blob
//!   `counter` (a little-endian `u64`), logs it, and writes it back plus one.
//! * It writes blob `unlocked-only` with `PutOptions::unlocked_only` if
//!   there is none, then reads it on every `Resume` and, from a worker
//!   thread, every 5 seconds, logging the result: the text while the
//!   device is unlocked, `Locked` while it is locked.
//! * A tap deletes `counter`, so the next launch counts from 1 again.
//!
//! The device, pipeline and shader are the `wgpu` example's own files,
//! included with `#[path]`.

#[cfg(target_os = "android")]
#[path = "../../wgpu/src/gpu.rs"]
mod gpu;
#[path = "../../wgpu/src/uniforms.rs"]
pub mod uniforms;

/// The counter's encoding, kept apart from Android so it is tested on the
/// host.
pub mod counter {
    /// The stored count, or 0 for none. A blob of the wrong length is 0 too.
    pub fn decode(blob: Option<&[u8]>) -> u64 {
        blob.and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
            .unwrap_or(0)
    }

    pub fn encode(count: u64) -> [u8; 8] {
        count.to_le_bytes()
    }

    #[cfg(test)]
    mod tests {
        use super::{decode, encode};

        #[test]
        fn round_trip() {
            assert_eq!(decode(None), 0);
            assert_eq!(decode(Some(&[1, 2])), 0);
            assert_eq!(decode(Some(&encode(41))), 41);
        }
    }
}

#[cfg(target_os = "android")]
mod app {
    use std::time::{Duration, Instant};

    use crate::counter;
    use crate::gpu::{Gpu, window_changed};
    use crate::uniforms::Uniforms;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::{AndroidApp, InputStatus, MobileApp, run, secure_store};
    use libmobile::{MobileEvent, PutOptions};

    const COUNTER: &str = "counter";
    const UNLOCKED_ONLY: &str = "unlocked-only";

    struct StoreApp {
        gpu: Option<Gpu>,
        resumed: bool,
        uniforms: Uniforms,
        start: Instant,
        last_tick: Instant,
        down: bool,
        /// Whether the launch has been counted.
        counted: bool,
    }

    /// Count this launch and make sure the unlocked-only blob exists.
    fn on_launch(app: &AndroidApp) {
        match secure_store::security_level(app) {
            Ok(level) => log::info!("securestore: key in {level:?}"),
            Err(error) => log::error!("securestore: security level: {error}"),
        }
        match secure_store::get(app, COUNTER) {
            Ok(blob) => {
                let count = counter::decode(blob.as_deref()) + 1;
                log::info!("securestore: launch {count}");
                if let Err(error) = secure_store::put(app, COUNTER, &counter::encode(count)) {
                    log::error!("securestore: put {COUNTER}: {error}");
                }
            }
            Err(error) => log::error!("securestore: get {COUNTER}: {error}"),
        }
        match secure_store::get(app, UNLOCKED_ONLY) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let options = PutOptions::default().unlocked_only(true);
                match secure_store::put_with(app, UNLOCKED_ONLY, b"only while unlocked", options) {
                    Ok(()) => log::info!("securestore: wrote {UNLOCKED_ONLY}"),
                    Err(error) => log::error!("securestore: put {UNLOCKED_ONLY}: {error}"),
                }
            }
            Err(error) => log::error!("securestore: get {UNLOCKED_ONLY}: {error}"),
        }
    }

    fn read_unlocked_only(app: &AndroidApp, from: &str) {
        match secure_store::get(app, UNLOCKED_ONLY) {
            Ok(Some(text)) => log::info!(
                "securestore: {UNLOCKED_ONLY} ({from}): {:?}",
                String::from_utf8_lossy(&text)
            ),
            Ok(None) => log::info!("securestore: {UNLOCKED_ONLY} ({from}): none"),
            Err(error) => log::info!("securestore: {UNLOCKED_ONLY} ({from}): {error:?}"),
        }
    }

    impl MobileApp for StoreApp {
        fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
            match event {
                MobileEvent::WindowCreated | MobileEvent::WindowResized => {
                    window_changed(&mut self.gpu, &mut self.uniforms, app);
                    if !self.counted {
                        self.counted = true;
                        on_launch(app);
                    }
                }
                MobileEvent::WindowDestroyed => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.swapchain = None;
                    }
                }
                MobileEvent::Resume => {
                    self.resumed = true;
                    if self.counted {
                        read_unlocked_only(app, "resume");
                    }
                }
                MobileEvent::Pause => self.resumed = false,
                MobileEvent::Insets(insets) => self.uniforms.set_safe_area(insets.safe_area),
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
                    match secure_store::delete(app, COUNTER) {
                        Ok(removed) => log::info!("securestore: deleted {COUNTER}: {removed}"),
                        Err(error) => log::error!("securestore: delete {COUNTER}: {error}"),
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
                .with_tag("libmobile-securestore"),
        );
        // The store from another thread, every 5 seconds, also while the
        // activity is paused or the screen is locked.
        let worker = app.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));
                read_unlocked_only(&worker, "timer");
            }
        });
        run(
            app,
            StoreApp {
                gpu: None,
                resumed: false,
                uniforms: Uniforms::default(),
                start: Instant::now(),
                last_tick: Instant::now(),
                down: false,
                counted: false,
            },
        );
    }
}
