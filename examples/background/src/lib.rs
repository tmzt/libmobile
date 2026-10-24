//! **libmobile background example**: a foreground service keeps a plain
//! thread's TCP connection alive while the app is in the background and the
//! screen is off.
//!
//! * On its first resume it asks for `POST_NOTIFICATIONS`, then, whatever
//!   the answer, starts the service (`specialUse`) with all three built-in
//!   controls (ongoing, Exit, wake-lock button) and one button of its own,
//!   "Ping".
//! * A plain thread connects to the host given by an intent extra and writes
//!   a line every 30 s, logging any reply and putting the count in the
//!   notification's text. On `Exited` it stops.
//! * After starting the service, it logs the battery state and, if the app
//!   is not exempt from battery optimization, shows the Allow/Deny dialog.
//! * Another thread logs every `SystemEvent`, `BatteryExemption` among them.
//! * Every 30 s the network thread replaces the ongoing notification
//!   (`service::update`) with a new title, text, button set (Ping, or an
//!   Open button that opens the app) and tag (`update-<n>`), also with the
//!   screen off. A tap on its body logs `ServiceEvent::Tapped`.
//! * `--es cmd alert` posts an alert (`android::notify`) on the `alerts`
//!   channel (high importance) with the tag `alert-tag-<n>` and two buttons,
//!   Ack (does not open the app) and Open (opens it); `--es id <id>` names
//!   it (default `example`), and posting the same id replaces it.
//!   `alert-later` posts it from a thread after `--es delay <seconds>`
//!   (default 8), so it can be posted while the app is in the background.
//!   `alert-cancel` and `alert-cancel-all` remove alerts. Taps log
//!   `MobileEvent::Notify`.
//!
//! ```sh
//! nc -lk 7999      # on a computer on the same network
//! adb shell am start -n dev.highbay.libmobile.example.background/dev.highbay.libmobile.MobileNativeActivity --es host 192.168.1.10:7999
//! adb logcat -s libmobile-background libmobile
//! ```
//!
//! More intents (`--es cmd <name>`): `start`, `stop`, `wake-on`,
//! `wake-off`, `state`, `battery` (ask for the battery exemption),
//! `battery-settings`, `app-details` and `vendor-battery` (open those
//! pages and log which opened), and `start-later` (stop now, then start again after
//! `--es delay <seconds>`, default 8, so that a start from the background
//! can be tried), `alert`, `alert-later`, `alert-cancel` and
//! `alert-cancel-all` (above).

/// What goes over the wire, kept apart from Android so it is tested on the
/// host.
pub mod wire {
    /// Line `n`, with the phone's clock in seconds since 1970.
    pub fn line(n: u64, unix_secs: u64) -> String {
        format!("libmobile-background line {n} at {unix_secs}\n")
    }

    /// The ongoing notification's title after `n` updates.
    pub fn update_title(n: u64) -> String {
        format!("libmobile background, update {n}")
    }

    /// The ongoing notification's tag after `n` updates.
    pub fn update_tag(n: u64) -> String {
        format!("update-{n}")
    }

    /// The notification's text after `n` lines.
    pub fn status(n: u64, host: &str) -> String {
        match n {
            1 => format!("1 line sent to {host}"),
            n => format!("{n} lines sent to {host}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lines_end_in_a_newline() {
            assert_eq!(line(3, 10), "libmobile-background line 3 at 10\n");
        }

        #[test]
        fn updates_are_numbered() {
            assert_eq!(update_title(2), "libmobile background, update 2");
            assert_eq!(update_tag(2), "update-2");
        }

        #[test]
        fn status_counts_lines() {
            assert_eq!(status(1, "h:1"), "1 line sent to h:1");
            assert_eq!(status(2, "h:1"), "2 lines sent to h:1");
        }
    }
}

#[cfg(target_os = "android")]
mod app {
    use crate::wire;
    use libmobile::android::ndk::hardware_buffer_format::HardwareBufferFormat;
    use libmobile::android::ndk::native_window::NativeWindow;
    use libmobile::android::notify::{self, Channel, Importance};
    use libmobile::android::service::{
        self, Controls, NotificationAction, NotificationContent, ServiceEvent, ServiceOptions,
        ServiceType,
    };
    use libmobile::android::{AndroidApp, MobileApp, battery, request_permissions, run};
    use libmobile::system_events::{self, SystemEvent};
    use libmobile::{MobileEvent, PermissionRequest};
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const TAG: &str = "libmobile-background";
    const POST_NOTIFICATIONS: &str = "android.permission.POST_NOTIFICATIONS";
    /// As declared in build.rs; only the kind matters at start.
    const SPECIAL_USE: ServiceType =
        ServiceType::SpecialUse("a persistent connection to the user's own server");
    const PERIOD: Duration = Duration::from_secs(30);

    /// `ip:port` from the `host` extra.
    static HOST: Mutex<Option<String>> = Mutex::new(None);
    /// Set on `Exited`: the network thread ends.
    static STOP: AtomicBool = AtomicBool::new(false);
    static NETWORK_RUNNING: AtomicBool = AtomicBool::new(false);
    static EVENTS_RUNNING: AtomicBool = AtomicBool::new(false);
    /// Alerts posted, for their text and tag.
    static ALERTS: AtomicU64 = AtomicU64::new(0);

    /// Post (or replace) alert `id` with a tag and two buttons.
    fn post_alert(id: &str) {
        let n = ALERTS.fetch_add(1, Ordering::Relaxed) + 1;
        let channel = Channel::new("alerts", "Alerts", Importance::High);
        let mut content =
            NotificationContent::new(format!("libmobile alert {id}"), format!("alert number {n}"));
        content.tag = Some(format!("alert-tag-{n}"));
        content.actions.push(NotificationAction::new("ack", "Ack"));
        let mut open = NotificationAction::new("open", "Open");
        open.opens_app = true;
        content.actions.push(open);
        log::info!(
            "background: alert {id:?} {n}: {:?}",
            notify::post(id, &content, &channel)
        );
    }

    /// The ongoing notification after `n` updates: title, text, buttons
    /// and tag all change.
    fn update_content(n: u64, text: &str) -> NotificationContent {
        let mut content = NotificationContent::new(wire::update_title(n), text);
        content.tag = Some(wire::update_tag(n));
        let action = if n.is_multiple_of(2) {
            NotificationAction::new("ping", "Ping")
        } else {
            let mut open = NotificationAction::new("open", "Open");
            open.opens_app = true;
            open
        };
        content.actions.push(action);
        content
    }

    fn start_service() {
        let mut content = NotificationContent::new("libmobile background", "starting");
        content
            .actions
            .push(NotificationAction::new("ping", "Ping"));
        let mut options = ServiceOptions::new(vec![SPECIAL_USE], content);
        let mut controls = Controls::default();
        controls.ongoing = true;
        controls.exit = true;
        controls.wake_lock_toggle = true;
        options.controls = controls;
        let result = service::start(&options);
        log::info!("background: start: {result:?}");
        if result.is_ok() {
            spawn_threads();
        }
    }

    fn spawn_threads() {
        STOP.store(false, Ordering::Relaxed);
        if !NETWORK_RUNNING.swap(true, Ordering::Relaxed) {
            std::thread::spawn(network_loop);
        }
        if !EVENTS_RUNNING.swap(true, Ordering::Relaxed) {
            let events = system_events::subscribe();
            std::thread::spawn(move || {
                log::info!("background: events thread: {:?}", system_events::current());
                loop {
                    let event = events.next_blocking();
                    log::info!("background: event {event:?}");
                    if event == SystemEvent::Service(ServiceEvent::Exited) {
                        STOP.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
    }

    fn unix_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn connect(host: &str) -> Option<TcpStream> {
        let addr = match host.to_socket_addrs().map(|mut a| a.next()) {
            Ok(Some(addr)) => addr,
            other => {
                log::error!("background: cannot resolve {host}: {other:?}");
                return None;
            }
        };
        match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                log::info!("background: connected to {host}");
                Some(stream)
            }
            Err(err) => {
                log::error!("background: connect {host}: {err}");
                None
            }
        }
    }

    /// Writes a line every 30 s to the host while there is one, until
    /// `STOP`.
    fn network_loop() {
        log::info!("background: network thread start");
        let mut stream: Option<(String, TcpStream)> = None;
        let mut next = Instant::now();
        let mut sent = 0u64;
        let mut updates = 0u64;
        let mut next_update = Instant::now() + PERIOD;
        let mut text = String::from("starting");
        while !STOP.load(Ordering::Relaxed) {
            if Instant::now() >= next_update {
                next_update = Instant::now() + PERIOD;
                updates += 1;
                let result = service::update(&update_content(updates, &text));
                log::info!(
                    "background: update {updates} (tag {}): {result:?}",
                    wire::update_tag(updates)
                );
            }
            let host = HOST.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(host) = host
                && Instant::now() >= next
            {
                next = Instant::now() + PERIOD;
                if stream.as_ref().is_none_or(|(to, _)| *to != host) {
                    stream = connect(&host).map(|s| (host.clone(), s));
                }
                if let Some((_, s)) = stream.as_mut() {
                    let line = wire::line(sent + 1, unix_now());
                    match s.write_all(line.as_bytes()) {
                        Ok(()) => {
                            sent += 1;
                            log::info!("background: sent line {sent}");
                            let mut reply = [0u8; 512];
                            match s.read(&mut reply) {
                                Ok(0) => {
                                    log::info!("background: connection closed");
                                    stream = None;
                                }
                                Ok(n) => log::info!(
                                    "background: reply {:?}",
                                    String::from_utf8_lossy(&reply[..n]).trim_end()
                                ),
                                Err(err)
                                    if matches!(
                                        err.kind(),
                                        ErrorKind::WouldBlock | ErrorKind::TimedOut
                                    ) => {}
                                Err(err) => log::error!("background: read: {err}"),
                            }
                            text = wire::status(sent, &host);
                            if let Err(err) = service::update_text(&text) {
                                log::warn!("background: update_text: {err}");
                            }
                        }
                        Err(err) => {
                            log::error!("background: write: {err}");
                            stream = None;
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        log::info!("background: network thread exit after {sent} lines");
        NETWORK_RUNNING.store(false, Ordering::Relaxed);
    }

    fn command(cmd: &str, delay: Option<&str>, id: Option<&str>) {
        let id = id.unwrap_or("example").to_string();
        match cmd {
            "alert" => post_alert(&id),
            "alert-later" => {
                let secs = delay.and_then(|d| d.parse().ok()).unwrap_or(8);
                std::thread::spawn(move || {
                    log::info!("background: alert {id:?} in {secs} s");
                    std::thread::sleep(Duration::from_secs(secs));
                    post_alert(&id);
                });
            }
            "alert-cancel" => log::info!("background: cancel {id:?}: {:?}", notify::cancel(&id)),
            "alert-cancel-all" => log::info!("background: cancel all: {:?}", notify::cancel_all()),
            "start" => start_service(),
            "stop" => log::info!("background: stop: {:?}", service::stop()),
            "wake-on" => log::info!("background: wake-on: {:?}", service::set_wake_lock(true)),
            "wake-off" => log::info!("background: wake-off: {:?}", service::set_wake_lock(false)),
            "state" => log::info!(
                "background: running={} wake_lock={} ignoring_optimizations={:?} {:?}",
                service::is_running(),
                service::wake_lock_held(),
                battery::is_ignoring_optimizations(),
                system_events::current()
            ),
            "battery" => log::info!(
                "background: battery request: {:?}",
                battery::request_ignore_optimizations()
            ),
            "battery-settings" => {
                log::info!(
                    "background: battery settings: {:?}",
                    battery::open_settings()
                )
            }
            "app-details" => {
                log::info!("background: app details: {:?}", battery::open_app_details())
            }
            "vendor-battery" => log::info!(
                "background: vendor battery: {:?}",
                battery::open_vendor_battery()
            ),
            "start-later" => {
                let secs = delay.and_then(|d| d.parse().ok()).unwrap_or(8);
                log::info!("background: stop: {:?}", service::stop());
                std::thread::spawn(move || {
                    log::info!("background: starting again in {secs} s");
                    std::thread::sleep(Duration::from_secs(secs));
                    start_service();
                });
            }
            other => log::warn!("background: unknown cmd {other:?}"),
        }
    }

    #[derive(Default)]
    struct BackgroundApp {
        window: Option<NativeWindow>,
        /// This run has asked for POST_NOTIFICATIONS.
        asked: bool,
        request: Option<PermissionRequest>,
    }

    impl BackgroundApp {
        /// A plain dark fill: this example draws nothing else.
        fn fill(&mut self, app: &AndroidApp) {
            let Some(window) = app.native_window() else {
                return;
            };
            let _ = window.set_buffers_geometry(0, 0, Some(HardwareBufferFormat::R8G8B8A8_UNORM));
            if let Ok(mut buffer) = window.lock(None)
                && let Some(lines) = buffer.lines()
            {
                for line in lines {
                    for (i, byte) in line.iter_mut().enumerate() {
                        byte.write(if i % 4 == 3 { 255 } else { 24 });
                    }
                }
            }
            self.window = Some(window);
        }
    }

    impl MobileApp for BackgroundApp {
        fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
            match event {
                MobileEvent::WindowCreated
                | MobileEvent::WindowResized
                | MobileEvent::RedrawNeeded => self.fill(app),
                MobileEvent::WindowDestroyed => self.window = None,
                MobileEvent::Resume => {
                    log::info!(
                        "background: resumed; ignoring battery optimizations: {:?}",
                        battery::is_ignoring_optimizations()
                    );
                    if !self.asked {
                        self.asked = true;
                        self.request = Some(request_permissions(app, &[POST_NOTIFICATIONS]));
                    }
                }
                MobileEvent::Permissions(outcome) if Some(outcome.request) == self.request => {
                    self.request = None;
                    log::info!("background: permissions {:?}", outcome.permissions);
                    start_service();
                    let state = battery::state();
                    log::info!("background: battery {state:?}");
                    if matches!(&state, Ok(s) if !s.ignoring_optimizations) {
                        log::info!(
                            "background: battery request: {:?}",
                            battery::request_ignore_optimizations()
                        );
                    }
                }
                MobileEvent::Intent(intent) => {
                    if let Some(host) = intent.extra("host") {
                        log::info!("background: host {host}");
                        *HOST.lock().unwrap_or_else(|e| e.into_inner()) = Some(host.to_string());
                    }
                    if let Some(cmd) = intent.extra("cmd") {
                        command(cmd, intent.extra("delay"), intent.extra("id"));
                    }
                }
                MobileEvent::Service(event) => {
                    log::info!("background: main loop {event:?}");
                    if event == ServiceEvent::Exited {
                        STOP.store(true, Ordering::Relaxed);
                    }
                }
                MobileEvent::Notify(event) => log::info!("background: main loop {event:?}"),
                MobileEvent::Destroy => log::info!("background: destroy"),
                _ => {}
            }
        }
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag(TAG),
        );
        run(app, BackgroundApp::default());
    }
}
