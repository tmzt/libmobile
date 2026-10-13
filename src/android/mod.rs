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
//!    the feature results — as [`MobileEvent`]s, *before* input, so a tap that
//!    lands in the same tick as a rotation is resolved against the new insets;
//! 3. pumps input events to [`MobileApp::input`];
//! 4. calls [`MobileApp::tick`], where the app drains its own
//!    [`Workers`](crate::worker::Workers) and redraws.
//!
//! The JNI bridges themselves live in the submodules; each JNI export is a
//! thin shim that converts arguments and pushes into a
//! [`Mailbox`](crate::mailbox::Mailbox), so no Java thread ever touches app
//! state.

mod insets;
mod intent;
mod jni_util;
pub mod surface;

// Re-exported whole so an app reaches input types, `ndk::native_window`, etc.
// through the exact versions libmobile links (one `JavaVM`, one `NativeWindow`).
pub use android_activity;
pub use android_activity::input::InputEvent;
pub use android_activity::{AndroidApp, InputStatus};
pub use ndk;
pub use surface::SurfaceTarget;

use crate::MobileEvent;
use android_activity::{MainEvent, PollEvent};
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
            handler.event(&app, event);
        }
        pump_bridges(&app, &mut handler);
        pump_input(&app, &mut handler);
        handler.tick(&app);

        if destroy {
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
}
