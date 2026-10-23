//! **libmobile securestore example**: the `wgpu` example's plasma, with a
//! launch counter kept in `android::secure_store`.
//!
//! * On the first window it logs the key's `security_level`, reads blob
//!   `counter` (a little-endian `u64`), logs it, and writes it back plus one.
//! * It writes blob `unlocked-only` with `PutOptions::unlocked_only` if
//!   there is none, then reads it on every `Resume` and, from a worker
//!   thread, every 5 seconds, logging the result: the text while the
//!   device is unlocked, `DeviceLocked` while it is locked.
//! * The worker thread also counts its ticks in blob `worker`, with no
//!   `AndroidApp` in reach (the store uses the Application from
//!   `ndk-context`), and the main thread logs that count on every `Resume`.
//! * A tap deletes `counter`, so the next launch counts from 1 again.
//! * The key store (`key_store`, namespace `libmobile.example`): key `device`
//!   is generated on first launch and its public fingerprint logged on later
//!   ones; key `device-unlocked` is made with `FLAG_DEVICE_UNLOCK_REQUIRED`
//!   and read with the blobs, giving `DeviceLocked` while the screen is
//!   locked. The adapter, `ExampleKey`, is 32 random bytes with a `u64`
//!   fingerprint as its public value (enough for a test).
//!
//! The device, pipeline and shader are the `wgpu` example's own files,
//! included with `#[path]`.

#[cfg(target_os = "android")]
#[path = "../../wgpu/src/gpu.rs"]
mod gpu;
#[path = "../../wgpu/src/uniforms.rs"]
pub mod uniforms;

/// The example's key adapter, kept apart from Android so it is tested on
/// the host.
pub mod example_key {
    use libmobile::key_store::{SealToken, SecretKey};
    use std::hash::{DefaultHasher, Hash, Hasher};
    use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

    /// 32 bytes behind a pointer, so moves copy only the pointer.
    #[derive(Zeroize, ZeroizeOnDrop)]
    pub struct ExampleKey(Box<[u8; 32]>);

    impl SecretKey for ExampleKey {
        type Public = u64;
        const KIND: &'static str = "libmobile.example.test.v1";

        fn generate(_: &SealToken) -> Self {
            let mut key = Self(Box::new([0; 32]));
            if let Err(error) = std::fs::File::open("/dev/urandom")
                .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut key.0[..]))
            {
                panic!("example key: no randomness: {error}");
            }
            key
        }

        fn seal(&self, _: &SealToken) -> Zeroizing<Vec<u8>> {
            Zeroizing::new(self.0.to_vec())
        }

        fn unseal(_: &str, bytes: &[u8], _: &SealToken) -> Result<Self, String> {
            let mut key = Self(Box::new([0; 32]));
            if bytes.len() != 32 {
                return Err(format!("{} bytes, not 32", bytes.len()));
            }
            key.0.copy_from_slice(bytes);
            Ok(key)
        }

        fn public(&self) -> u64 {
            let mut hasher = DefaultHasher::new();
            self.0.hash(&mut hasher);
            hasher.finish()
        }
    }
}

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
    use crate::example_key::ExampleKey;
    use crate::gpu::{Gpu, window_changed};
    use crate::uniforms::Uniforms;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::{AndroidApp, InputStatus, MobileApp, run, secure_store};
    use libmobile::key_store::{self, FLAG_DEVICE_UNLOCK_REQUIRED, KeyStoreError, Vault};
    use libmobile::{MobileEvent, PutOptions};

    const NAMESPACE: &str = "libmobile.example";
    const DEVICE_KEY: &str = "device";
    const UNLOCKED_KEY: &str = "device-unlocked";

    fn vault() -> Vault {
        Vault::named(NAMESPACE).expect("a valid namespace")
    }

    /// Load key `name`, or generate it with `flags` if there is none.
    fn key(name: &str, flags: u8) {
        let vault = vault();
        match key_store::public::<ExampleKey>(&vault, name) {
            Ok(public) => log::info!("securestore: key {name}: {public:016x} (loaded)"),
            Err(KeyStoreError::NotFound) => {
                match key_store::generate::<ExampleKey>(&vault, name, flags) {
                    Ok(public) => log::info!("securestore: key {name}: {public:016x} (generated)"),
                    Err(error) => log::info!("securestore: key {name}: generate: {error:?}"),
                }
            }
            Err(error) => log::info!("securestore: key {name}: {error:?}"),
        }
    }

    /// Use key `name` inside `with_key`, logging what the closure saw.
    fn use_key(name: &str, from: &str) {
        let seen = key_store::with_key::<ExampleKey, _>(&vault(), name, |key| {
            libmobile::key_store::SecretKey::public(key)
        });
        match seen {
            Ok(public) => log::info!("securestore: key {name} ({from}): used {public:016x}"),
            Err(error) => log::info!("securestore: key {name} ({from}): {error:?}"),
        }
    }

    const COUNTER: &str = "counter";
    const UNLOCKED_ONLY: &str = "unlocked-only";
    /// Written by the timer thread, read on the main thread at `Resume`.
    const WORKER: &str = "worker";

    fn read_worker() {
        match secure_store::get(WORKER) {
            Ok(blob) => log::info!(
                "securestore: {WORKER} (resume): {} ticks",
                counter::decode(blob.as_deref())
            ),
            Err(error) => log::error!("securestore: get {WORKER}: {error}"),
        }
    }

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
    fn on_launch() {
        key(DEVICE_KEY, 0);
        key(UNLOCKED_KEY, FLAG_DEVICE_UNLOCK_REQUIRED);
        match secure_store::security_level() {
            Ok(level) => log::info!("securestore: key in {level:?}"),
            Err(error) => log::error!("securestore: security level: {error}"),
        }
        match secure_store::get(COUNTER) {
            Ok(blob) => {
                let count = counter::decode(blob.as_deref()) + 1;
                log::info!("securestore: launch {count}");
                if let Err(error) = secure_store::put(COUNTER, &counter::encode(count)) {
                    log::error!("securestore: put {COUNTER}: {error}");
                }
            }
            Err(error) => log::error!("securestore: get {COUNTER}: {error}"),
        }
        match secure_store::get(UNLOCKED_ONLY) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let options = PutOptions::default().unlocked_only(true);
                match secure_store::put_with(UNLOCKED_ONLY, b"only while unlocked", options) {
                    Ok(()) => log::info!("securestore: wrote {UNLOCKED_ONLY}"),
                    Err(error) => log::error!("securestore: put {UNLOCKED_ONLY}: {error}"),
                }
            }
            Err(error) => log::error!("securestore: get {UNLOCKED_ONLY}: {error}"),
        }
    }

    fn read_unlocked_only(from: &str) {
        use_key(UNLOCKED_KEY, from);
        match secure_store::get(UNLOCKED_ONLY) {
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
                        on_launch();
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
                        read_unlocked_only("resume");
                        read_worker();
                    }
                }
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
                MotionAction::Down => {
                    self.uniforms.touch = [pointer.x(), pointer.y()];
                    self.down = true;
                    match secure_store::delete(COUNTER) {
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
        // No AndroidApp in reach of this thread: the store finds the
        // Application through ndk-context.
        std::thread::spawn(move || {
            let mut ticks = 0u64;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                read_unlocked_only("timer");
                ticks += 1;
                if let Err(error) = secure_store::put(WORKER, &counter::encode(ticks)) {
                    log::error!("securestore: put {WORKER} (timer): {error}");
                }
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
