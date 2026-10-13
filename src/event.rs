//! **What the main loop hands the app.**
//!
//! One enum for the NativeActivity lifecycle (from android-activity) and for
//! everything the Kotlin plugins report (insets, intents, text, feature
//! results), so an app handles platform facts in one place. Platform-free:
//! window *handles* are not carried — read `AndroidApp::native_window()` on
//! [`MobileEvent::WindowCreated`].

use crate::insets::Insets;
use crate::intent::IntentData;

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MobileEvent {
    Start,
    Resume,
    Pause,
    Stop,
    /// A native window exists (`InitWindow`). Build or re-attach the surface.
    WindowCreated,
    /// The native window changed size.
    WindowResized,
    /// The native window is going away (`TerminateWindow`). Delivered
    /// **synchronously inside the poll callback**: android-activity returns the
    /// `ANativeWindow` to the platform the moment that callback returns, so a
    /// swapchain must be released while handling this event, not later.
    WindowDestroyed,
    RedrawNeeded,
    FocusChanged(bool),
    /// The configuration changed (density, night mode, orientation...); read
    /// it from `AndroidApp::config()`.
    ConfigChanged,
    LowMemory,
    /// System bars / cutout / IME moved. Only the newest report per tick.
    Insets(Insets),
    /// The launch intent, or a later one via `onNewIntent`.
    Intent(IntentData),
    /// The hidden `InputConnection` field's full text after an IME edit.
    TextChanged(String),
    #[cfg(feature = "google-auth")]
    GoogleLogin(crate::auth::LoginOutcome),
    /// The activity is being destroyed; the loop returns after this.
    Destroy,
}
