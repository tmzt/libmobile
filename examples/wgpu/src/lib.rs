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
//! `--es cmd clipboard-write` writes the fixed text `CLIPBOARD_CHECK`
//! (`android::clipboard::write_text`) and logs only the `Result`; a
//! `clipboard` after it reads the example's own clip, with no toast.
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
//!
//! `--es cmd files-access` logs `android::all_files_access::state` and
//! `external_storage_dir`; `files-access-open` opens the All files access
//! page and logs which (`open_settings`); `state` is logged again on each
//! `Resume`. The build script declares `MANAGE_EXTERNAL_STORAGE` unless
//! `LIBMOBILE_WGPU_NO_ALL_FILES=1` is set when it runs, which shows
//! `NotDeclared`. `--es cmd media-scan --es path <path>` asks MediaStore to
//! scan that path (`android::media_scan::scan_paths`) and logs the result.
//!
//! `--es cmd disk-space`, on a thread of its own, logs
//! `android::disk_space::space` of `files_dir` and of
//! `/storage/emulated/0`, with each call's time. `--es cmd disk-watch`
//! (optionally `--es interval <seconds>`, 60 by default) watches
//! `files_dir` (`android::disk_space::watch`) and logs each
//! `MobileEvent::DiskSpace`; `disk-unwatch` stops it. libmobile's own
//! `disk check` line is logged at debug.
//!
//! A long press (half a second, still) opens the soft keyboard
//! (`android::set_text_input_with`) with the next of the options in
//! `keyboards()`, logging their name; a long press while it is open moves to the
//! next. It logs each `TextChanged` (its length and newlines, never the
//! text), `TextAction` and `TextInputDismissed`. `--es cmd keyboard-close`
//! hides it.

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
    use libmobile::android::all_files_access;
    use libmobile::android::android_activity::input::{InputEvent, MotionAction};
    use libmobile::android::clipboard;
    use libmobile::android::dirs;
    use libmobile::android::disk_space;
    use libmobile::android::image_codec::{self, Encoding};
    use libmobile::android::link::open_link;
    use libmobile::android::media::{
        self, CameraEntry, Delivery, MediaData, MediaKind, MediaRequest, MediaResponse, Picker,
    };
    use libmobile::android::media_scan;
    use libmobile::android::{
        AndroidApp, InputStatus, MobileApp, build_info, night_mode, run, set_system_bar_icons,
        set_text_input_with,
    };
    use libmobile::text_input::{TextAction, TextInputOptions, TextKind};
    use std::time::{Duration, Instant};

    /// A press shorter than this, that moved less than [`TAP_SLOP`], is a tap.
    const TAP_TIME: Duration = Duration::from_millis(300);
    const TAP_SLOP: f32 = 24.0;
    /// A still press this long opens the keyboard.
    const LONG_PRESS: Duration = Duration::from_millis(500);

    /// The keyboards a long press cycles through, by name.
    fn keyboards() -> Vec<(&'static str, TextInputOptions)> {
        let with = |multiline, kind, action, autocorrect| {
            let mut o = TextInputOptions::default();
            o.multiline = multiline;
            o.kind = kind;
            o.action = action;
            o.autocorrect = autocorrect;
            o
        };
        vec![
            ("text, Done", TextInputOptions::default()),
            ("url, Go", with(false, TextKind::Url, TextAction::Go, false)),
            (
                "number, Done",
                with(false, TextKind::Number, TextAction::Done, false),
            ),
            (
                "email, Send",
                with(false, TextKind::Email, TextAction::Send, false),
            ),
            (
                "password, Done",
                with(false, TextKind::Password, TextAction::Done, false),
            ),
            (
                "text, Search, no autocorrect",
                with(false, TextKind::Text, TextAction::Search, false),
            ),
            (
                "one line, Enter, no autocorrect",
                with(false, TextKind::Text, TextAction::None, false),
            ),
            (
                "many lines, newline",
                with(true, TextKind::Text, TextAction::None, true),
            ),
            (
                "many lines, Send",
                with(true, TextKind::Text, TextAction::Send, true),
            ),
        ]
    }

    /// The fixed text `--es cmd clipboard-write` copies (29 bytes). It is
    /// never logged, so a check can look for it in the whole log.
    const CLIPBOARD_CHECK: &str = "libmobile-clipboard-check-29Q";

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

    /// `--es cmd disk-space`: `space` of the files directory and of the
    /// shared volume, timed. Not on the main loop's thread: it may block.
    fn log_disk_space() {
        let shared = std::path::PathBuf::from("/storage/emulated/0");
        for (name, path) in [("files_dir", dirs::files_dir()), ("shared", Some(shared))] {
            let Some(path) = path else {
                log::warn!("wgpu example: disk-space {name}: no path");
                continue;
            };
            let started = Instant::now();
            let space = disk_space::space(&path);
            let took = started.elapsed();
            let started = Instant::now();
            let statvfs = disk_space::statvfs_space(&path);
            log::info!(
                "wgpu example: disk-space {name} {}: space {space:?} in {} us; statvfs alone {} us",
                path.display(),
                took.as_micros(),
                started.elapsed().as_micros(),
            );
            if let (Ok(s), Ok(v)) = (&space, &statvfs) {
                log::info!(
                    "wgpu example: disk-space {name}: allocatable >= free: {:?}, cache_quota > 0: {:?}, statvfs free {}",
                    s.allocatable.map(|a| a >= s.free),
                    s.cache_quota.map(|q| q > 0),
                    v.free
                );
            }
        }
    }

    /// `--es cmd files-access`, and on each `Resume`.
    fn log_files_access() {
        log::info!(
            "wgpu example: files-access state {:?}, external_storage_dir {:?}",
            all_files_access::state(),
            all_files_access::external_storage_dir()
        );
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
        /// The next of `keyboards()` a long press opens.
        keyboard: usize,
    }

    impl WgpuApp {
        fn open_keyboard(&mut self, app: &AndroidApp) {
            let keyboards = keyboards();
            let (name, options) = &keyboards[self.keyboard % keyboards.len()];
            self.keyboard += 1;
            log::info!("wgpu example: keyboard: {name}");
            set_text_input_with(app, Some(""), options);
        }
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
                MobileEvent::Resume => {
                    self.resumed = true;
                    log_files_access();
                }
                MobileEvent::Pause => self.resumed = false,
                MobileEvent::Insets(insets) => self.uniforms.set_safe_area(insets.safe_area),
                MobileEvent::TrimMemory(level) => log::info!("wgpu example: TrimMemory({level:?})"),
                MobileEvent::Media(response) => log_media(response),
                MobileEvent::DiskSpace(space) => log::info!("wgpu example: DiskSpace({space:?})"),
                MobileEvent::TextChanged(text) => log::info!(
                    "wgpu example: TextChanged: {} chars, {} newlines",
                    text.chars().count(),
                    text.matches('\n').count()
                ),
                MobileEvent::TextAction(action) => {
                    log::info!("wgpu example: TextAction({action:?})")
                }
                MobileEvent::TextInputDismissed => log::info!("wgpu example: TextInputDismissed"),
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
                        Some("keyboard-close") => {
                            set_text_input_with(app, None, &TextInputOptions::default())
                        }
                        Some("clipboard-write") => log::info!(
                            "wgpu example: clipboard write_text {:?}",
                            clipboard::write_text(app, CLIPBOARD_CHECK)
                        ),
                        Some("disk-space") => {
                            std::thread::spawn(log_disk_space);
                        }
                        Some("disk-watch") => {
                            let seconds = intent
                                .extra("interval")
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            log::info!(
                                "wgpu example: disk-watch every {seconds} s: {:?}",
                                disk_space::watch(None, Duration::from_secs(seconds))
                            );
                        }
                        Some("disk-unwatch") => {
                            disk_space::unwatch();
                            log::info!("wgpu example: disk-unwatch");
                        }
                        Some("files-access") => log_files_access(),
                        Some("files-access-open") => log::info!(
                            "wgpu example: files-access-open {:?}",
                            all_files_access::open_settings()
                        ),
                        Some("media-scan") => match intent.extra("path") {
                            Some(path) => log::info!(
                                "wgpu example: media-scan {path}: {:?}",
                                media_scan::scan_paths(&[std::path::Path::new(path)])
                            ),
                            None => log::warn!("wgpu example: media-scan needs --es path <path>"),
                        },
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

        fn tick(&mut self, app: &AndroidApp) {
            if self.down
                && self
                    .press
                    .is_some_and(|(when, _)| when.elapsed() >= LONG_PRESS)
            {
                self.press = None;
                self.open_keyboard(app);
            }
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
                        .parse(
                            "info,libmobile::android::link=debug,libmobile::android::disk_space=debug",
                        )
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
                keyboard: 0,
            },
        );
    }
}
