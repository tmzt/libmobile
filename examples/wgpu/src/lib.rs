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
//! and model (`android::build_info`). It logs each `MobileEvent::TrimMemory`
//! (try `adb shell am send-trim-memory <package> UI_HIDDEN`).
//!
//! It logs `android::night_mode` at start and on each `ConfigChanged` (try
//! `adb shell cmd uimode night yes`). `--es cmd bar-icons-dark` and
//! `bar-icons-light` set the system bars' icons
//! (`android::set_system_bar_icons`) from another thread.
//!
//! `--es cmd clipboard` logs `android::clipboard::has_text`, then
//! `read_text` as the text's length in bytes (never the text). Copy a line
//! in another app first; Android 12 and later show their "pasted" toast.
//!
//! `--es cmd image-codec`, on a thread of its own, probes and decodes every
//! file in `cache_dir()/codec-test/` (`android::image_codec`), logs each
//! `ImageInfo`, the decoded sizes and times (full size and `max_side` 2048)
//! and `is_opaque`, and writes each back beside it as `<name>.out.jpg` (JPEG
//! 90) and `<name>.out.png`, logging whether the PNG decodes back to the
//! same pixels. Put files there with `adb push <file> /data/local/tmp/` and
//! `adb shell run-as dev.highbay.libmobile.example.wgpu sh -c
//! 'mkdir -p cache/codec-test && cp /data/local/tmp/<file> cache/codec-test/'`.
//!
//! The `media-*` commands ask for media (`android::media::request_media`)
//! and log each `MobileEvent::Media` (counts, MIME types, sizes, the EXIF
//! orientation, never a URI or name), probing the first image's bytes with
//! `image_codec`: `media-pick` (one image, as bytes), `media-multi` (up to
//! five images and videos, as files), `media-camera` (the chooser with a
//! photo camera entry), `media-video` (with a video camera entry),
//! `media-photo-picker` (the Photo Picker, up to three images) and
//! `media-small` (one image under a 1 MiB cap). This example has no CAMERA
//! in its manifest, so the camera entry asks for no permission.

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
    use libmobile::android::clipboard;
    use libmobile::android::dirs;
    use libmobile::android::image_codec::{self, Encoding};
    use libmobile::android::link::open_link;
    use libmobile::android::media::{
        self, CameraEntry, Delivery, MediaData, MediaKind, MediaRequest, MediaResponse, Picker,
    };
    use libmobile::android::{
        AndroidApp, InputStatus, MobileApp, build_info, night_mode, run, set_system_bar_icons,
    };
    use std::time::{Duration, Instant};

    /// A press shorter than this, that moved less than [`TAP_SLOP`], is a tap.
    const TAP_TIME: Duration = Duration::from_millis(300);
    const TAP_SLOP: f32 = 24.0;

    /// Log the clipboard's state and its text's length, never the text.
    fn log_clipboard(app: &AndroidApp) {
        log::info!(
            "wgpu example: clipboard has_text {:?}",
            clipboard::has_text(app)
        );
        match clipboard::read_text(app) {
            Ok(Some(text)) => log::info!(
                "wgpu example: clipboard read_text Ok(Some({} bytes))",
                text.len()
            ),
            other => log::info!("wgpu example: clipboard read_text {other:?}"),
        }
    }

    /// `--es cmd image-codec`: every file in `cache_dir()/codec-test/`
    /// through `probe_image`, `decode_image` and `encode_image`. Not on the
    /// main loop's thread: the calls block.
    fn codec_test() {
        let Some(dir) = dirs::cache_dir().map(|dir| dir.join("codec-test")) else {
            log::warn!("wgpu example: image-codec: no cache directory");
            return;
        };
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                log::warn!("wgpu example: image-codec: {}: {err}", dir.display());
                return;
            }
        };
        let mut paths: Vec<_> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.is_file()
                    && !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains(".out."))
            })
            .collect();
        paths.sort();
        log::info!("wgpu example: image-codec: {} files", paths.len());
        for path in paths {
            codec_test_file(&path);
        }
    }

    fn codec_test_file(path: &std::path::Path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("wgpu example: image-codec {name}: read: {err}");
                return;
            }
        };
        log::info!(
            "wgpu example: image-codec {name}: {} bytes, probe {:?}",
            bytes.len(),
            image_codec::probe_image(&bytes)
        );
        let started = Instant::now();
        let scaled = image_codec::decode_image(&bytes, 2048);
        log::info!(
            "wgpu example: image-codec {name}: decode max_side 2048 in {} ms: {}",
            started.elapsed().as_millis(),
            scaled
                .as_ref()
                .map_or_else(|err| format!("{err:?}"), describe_image)
        );
        let started = Instant::now();
        let full = match image_codec::decode_image(&bytes, 0) {
            Ok(full) => full,
            Err(err) => {
                log::info!(
                    "wgpu example: image-codec {name}: decode full size in {} ms: {err:?}",
                    started.elapsed().as_millis()
                );
                return;
            }
        };
        log::info!(
            "wgpu example: image-codec {name}: decode full size in {} ms: {}",
            started.elapsed().as_millis(),
            describe_image(&full)
        );
        for (encoding, ext) in [
            (Encoding::Jpeg { quality: 90 }, "jpg"),
            (Encoding::Png, "png"),
        ] {
            let started = Instant::now();
            let out = path.with_file_name(format!("{name}.out.{ext}"));
            match image_codec::encode_image(&full, encoding) {
                Ok(encoded) => {
                    let written = std::fs::write(&out, &encoded);
                    log::info!(
                        "wgpu example: image-codec {name}: {encoding:?} in {} ms: {} bytes, write {written:?}",
                        started.elapsed().as_millis(),
                        encoded.len()
                    );
                    if encoding == Encoding::Png {
                        let back = image_codec::decode_image(&encoded, 0);
                        log::info!(
                            "wgpu example: image-codec {name}: PNG decodes back to the same pixels: {}",
                            back.as_ref().is_ok_and(|back| *back == full)
                        );
                    }
                }
                Err(err) => {
                    log::info!("wgpu example: image-codec {name}: {encoding:?}: {err:?}")
                }
            }
        }
    }

    fn describe_image(image: &image_codec::Rgba) -> String {
        format!(
            "{}x{}, is_opaque {}",
            image.width,
            image.height,
            image.is_opaque()
        )
    }

    /// `--es cmd media-*`: the request each command makes.
    fn media_request(cmd: &str) -> Option<MediaRequest> {
        let mut request = MediaRequest::default();
        match cmd {
            "media-pick" => {}
            "media-multi" => {
                request.kind = MediaKind::ImagesAndVideos;
                request.max_items = 5;
                request.delivery = Delivery::File;
            }
            "media-camera" => request.camera = CameraEntry::Photo,
            "media-video" => {
                request.kind = MediaKind::Videos;
                request.camera = CameraEntry::Video;
                request.delivery = Delivery::File;
            }
            "media-photo-picker" => {
                request.picker = Picker::PhotoPicker;
                request.max_items = 3;
            }
            "media-small" => request.max_bytes = 1024 * 1024,
            _ => return None,
        }
        Some(request)
    }

    /// Log a media answer without its URIs or names; probe the first image.
    fn log_media(response: MediaResponse) {
        let reply = match response.result {
            Ok(reply) => reply,
            Err(err) => {
                log::info!(
                    "wgpu example: media {:?}: {err:?}",
                    response.request.map(|r| r.id())
                );
                return;
            }
        };
        log::info!(
            "wgpu example: media {:?}: {} item(s), picker {:?}, camera_left_out {}, truncated {}",
            response.request.map(|r| r.id()),
            reply.items.len(),
            reply.picker,
            reply.camera_left_out,
            reply.truncated
        );
        for item in &reply.items {
            let data = match &item.data {
                MediaData::Bytes(bytes) => format!("{} bytes in memory", bytes.len()),
                MediaData::File(path) => format!(
                    "file of {:?} bytes",
                    std::fs::metadata(path).map(|m| m.len()).ok()
                ),
                other => format!("{other:?}"),
            };
            log::info!(
                "wgpu example: media item {:?} {:?} size {:?} exif {}: {data}",
                item.source,
                item.mime,
                item.size,
                item.exif_orientation
            );
        }
        let first = reply.items.into_iter().find_map(|item| match item.data {
            MediaData::Bytes(bytes) => Some(bytes),
            _ => None,
        });
        if let Some(bytes) = first {
            std::thread::spawn(move || {
                log::info!(
                    "wgpu example: media first item probe {:?}",
                    image_codec::probe_image(&bytes)
                );
            });
        }
    }

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
                MobileEvent::TrimMemory(level) => log::info!("wgpu example: TrimMemory({level:?})"),
                MobileEvent::Media(response) => log_media(response),
                MobileEvent::ConfigChanged => {
                    log::info!(
                        "wgpu example: ConfigChanged: night_mode {:?}",
                        night_mode(app)
                    )
                }
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
                        Some(cmd @ ("bar-icons-dark" | "bar-icons-light")) => {
                            let dark = cmd == "bar-icons-dark";
                            std::thread::spawn(move || {
                                set_system_bar_icons(dark, dark);
                                log::info!("wgpu example: set_system_bar_icons({dark}, {dark})");
                            });
                        }
                        Some("clipboard") => log_clipboard(app),
                        Some("image-codec") => {
                            std::thread::spawn(codec_test);
                        }
                        Some(cmd) if media_request(cmd).is_some() => {
                            let request = media_request(cmd).unwrap_or_default();
                            let id = media::request_media(app, &request);
                            log::info!("wgpu example: {cmd}: request {}", id.id());
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
        log::info!("wgpu example: night_mode {:?}", night_mode(&app));
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
