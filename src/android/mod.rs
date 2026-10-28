//! The NativeActivity side (target_os = "android" only).
//!
//! [`run`] is the main loop: an android-activity poll on the `android_main`
//! thread that never blocks on anything but the poll itself. Each iteration it
//!
//! 1. polls lifecycle events (delivering `WindowDestroyed` synchronously — see
//!    [`MobileEvent::WindowDestroyed`] — and everything else after the poll
//!    returns, because `AndroidApp::native_window()` takes the same internal
//!    lock the poll callback holds);
//! 2. drains the Kotlin bridges' mailboxes — insets, intents, IME text, and
//!    permission answers, memory-trim levels, service and alert events and the feature results (the camera's among them) — as [`MobileEvent`]s, *before* input, so a tap that
//!    lands in the same tick as a rotation is resolved against the new insets;
//! 3. pumps input events to [`MobileApp::input`];
//! 4. calls [`MobileApp::tick`], where the app drains its own
//!    [`Workers`](crate::worker::Workers) and redraws.
//!
//! The JNI bridges themselves live in the submodules; each JNI export is a
//! thin shim that converts arguments and pushes into a
//! [`Mailbox`](crate::mailbox::Mailbox), so no Java thread ever touches app
//! state.

mod appearance;
#[cfg(feature = "cpal")]
pub mod audio_capture;
#[cfg(feature = "google-auth")]
pub mod auth;
#[cfg(feature = "battery")]
pub mod battery;
mod build_info;
#[cfg(feature = "camera")]
pub mod camera;
#[cfg(feature = "camera-intent")]
pub mod camera_intent;
pub mod dirs;
mod insets;
mod intent;
mod jni_util;
pub mod link;
#[cfg(any(feature = "foreground-service", feature = "notifications"))]
mod notification;
#[cfg(feature = "notifications")]
pub mod notify;
mod permission;
pub mod secure_store;
#[cfg(feature = "foreground-service")]
pub mod service;
pub mod surface;
#[cfg(feature = "system-events")]
pub(crate) mod system_events;
pub mod text_input;
mod trim_memory;

// Re-exported whole so an app reaches input types, `ndk::native_window`, etc.
// (and, under `cpal`, cpal's AAudio backend) through the exact versions
// libmobile links (one `JavaVM`, one `NativeWindow`).
pub use android_activity;
pub use android_activity::input::InputEvent;
pub use android_activity::{AndroidApp, InputStatus};
pub use appearance::{night_mode, set_system_bar_icons};
#[cfg(feature = "cpal")]
pub use audio_capture::CaptureStream;
#[cfg(feature = "google-auth")]
pub use auth::start_google_login;
pub use build_info::{BuildInfo, build_info};
#[cfg(feature = "camera-intent")]
pub use camera_intent::capture_photo;
#[cfg(feature = "cpal")]
pub use cpal;
pub use link::open_app_settings;
pub use ndk;
pub use permission::{permission_granted, request_permissions, should_show_rationale};
pub use surface::SurfaceTarget;
pub use text_input::set_text_input;

use crate::MobileEvent;
use android_activity::{MainEvent, PollEvent};
use jni::{jni_sig, jni_str};
use std::time::Duration;

/// Poll timeout when the app does not ask for another.
pub const DEFAULT_POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// The app side of [`run`].
pub trait MobileApp {
    /// A lifecycle change or a bridge report.
    fn event(&mut self, app: &AndroidApp, event: MobileEvent);

    /// A touch/key event. Return [`InputStatus::Handled`] to consume it.
    fn input(&mut self, app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
        let _ = (app, event);
        InputStatus::Unhandled
    }

    /// Once per loop iteration, after events and input: drain workers, advance
    /// animations, draw.
    fn tick(&mut self, app: &AndroidApp) {
        let _ = app;
    }

    /// How long the next poll may wait for an event. `Some(Duration::ZERO)`
    /// spins (continuous rendering); `None` waits indefinitely, which is only
    /// right for an app with no background work to drain.
    fn poll_timeout(&self) -> Option<Duration> {
        Some(DEFAULT_POLL_TIMEOUT)
    }
}

/// Ask the activity to finish (`Activity.finish()`), as the system Back
/// action would. The normal teardown follows through [`run`]: `Pause`,
/// `Stop`, `WindowDestroyed`, then `Destroy`, after which the loop returns.
/// Callable from the `android_main` thread; a failure is logged.
pub fn finish(app: &AndroidApp) {
    let result = jni_util::with_activity(app, |env, activity| {
        env.call_method(activity, jni_str!("finish"), jni_sig!("()V"), &[])?;
        Ok(())
    });
    if let Err(err) = result {
        log::error!("libmobile: finishing the activity failed: {err}");
    }
}

/// Run the main loop until the activity is destroyed. Call from the app's
/// `android_main`.
pub fn run<A: MobileApp>(app: AndroidApp, mut handler: A) {
    log::info!("libmobile: main loop start");
    loop {
        let mut deferred: Vec<MobileEvent> = Vec::new();
        let mut destroy = false;
        app.poll_events(handler.poll_timeout(), |event| {
            let PollEvent::Main(main) = event else {
                return;
            };
            match main {
                MainEvent::TerminateWindow { .. } => {
                    handler.event(&app, MobileEvent::WindowDestroyed)
                }
                MainEvent::Destroy => destroy = true,
                other => deferred.extend(map_main_event(&other)),
            }
        });

        for event in deferred {
            #[cfg(feature = "camera")]
            camera::lifecycle(&app, &event);
            handler.event(&app, event);
        }
        pump_bridges(&app, &mut handler);
        pump_input(&app, &mut handler);
        handler.tick(&app);

        if destroy {
            #[cfg(feature = "camera")]
            camera::lifecycle(&app, &MobileEvent::Destroy);
            handler.event(&app, MobileEvent::Destroy);
            break;
        }
    }
    discard_stale_commands();
    log::info!("libmobile: main loop exit");
}

/// The lifecycle events delivered after the poll returns.
fn map_main_event(event: &MainEvent<'_>) -> Option<MobileEvent> {
    Some(match event {
        MainEvent::Start => MobileEvent::Start,
        MainEvent::Resume { .. } => MobileEvent::Resume,
        MainEvent::Pause => MobileEvent::Pause,
        MainEvent::Stop => MobileEvent::Stop,
        MainEvent::InitWindow { .. } => MobileEvent::WindowCreated,
        MainEvent::WindowResized { .. } => MobileEvent::WindowResized,
        MainEvent::RedrawNeeded { .. } => MobileEvent::RedrawNeeded,
        MainEvent::GainedFocus => MobileEvent::FocusChanged(true),
        MainEvent::LostFocus => MobileEvent::FocusChanged(false),
        MainEvent::ConfigChanged { .. } => MobileEvent::ConfigChanged,
        MainEvent::LowMemory => MobileEvent::LowMemory,
        // InputAvailable: input is pumped every tick regardless.
        _ => return None,
    })
}

fn pump_bridges<A: MobileApp>(app: &AndroidApp, handler: &mut A) {
    if let Some(insets) = insets::take_pending() {
        handler.event(app, MobileEvent::Insets(insets));
    }
    for intent in intent::take_pending() {
        handler.event(app, MobileEvent::Intent(intent));
    }
    for text in text_input::take_pending() {
        handler.event(app, MobileEvent::TextChanged(text));
    }
    for level in trim_memory::take_pending() {
        handler.event(app, MobileEvent::TrimMemory(level));
    }
    #[cfg(feature = "google-auth")]
    for outcome in auth::take_results() {
        handler.event(app, MobileEvent::GoogleLogin(outcome));
    }
    #[cfg(feature = "camera-intent")]
    for outcome in camera_intent::take_results() {
        handler.event(app, MobileEvent::Capture(outcome));
    }
    for outcome in permission::take_results() {
        // The camera's own CAMERA request is answered to its gate, not to
        // the app.
        #[cfg(feature = "camera")]
        let Some(outcome) = camera::offer_permission(outcome) else {
            continue;
        };
        handler.event(app, MobileEvent::Permissions(outcome));
    }
    #[cfg(feature = "camera")]
    for event in camera::take_events() {
        handler.event(app, MobileEvent::Camera(event));
    }
    #[cfg(feature = "foreground-service")]
    for event in service::take_events() {
        handler.event(app, MobileEvent::Service(event));
    }
    #[cfg(feature = "notifications")]
    for event in notify::take_events() {
        handler.event(app, MobileEvent::Notify(event));
    }
}

/// Drain every pending input event. Called every iteration: android-activity
/// does not resend `InputAvailable` for events left in the queue.
fn pump_input<A: MobileApp>(app: &AndroidApp, handler: &mut A) {
    let mut iter = match app.input_events_iter() {
        Ok(iter) => iter,
        Err(err) => {
            log::error!("libmobile: no input iterator: {err:?}");
            return;
        }
    };
    while iter.next(|event| handler.input(app, event)) {}
}

/// `android_main` can run more than once per process (the activity is
/// destroyed and recreated). Queued *commands* from this run must not replay
/// into the next one, so they are dropped on the way out. The safe area is a
/// current value, not a command, and is kept (see `insets`).
fn discard_stale_commands() {
    let _ = intent::take_pending();
    let _ = text_input::take_pending();
    let _ = trim_memory::take_pending();
    #[cfg(feature = "google-auth")]
    let _ = auth::take_results();
    #[cfg(feature = "camera-intent")]
    let _ = camera_intent::take_results();
    #[cfg(feature = "camera")]
    let _ = camera::take_events();
    let _ = permission::take_results();
    #[cfg(feature = "foreground-service")]
    let _ = service::take_events();
    #[cfg(feature = "notifications")]
    let _ = notify::take_events();
}
