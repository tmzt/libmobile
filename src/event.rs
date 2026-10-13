//! **What the main loop hands the app.**
//!
//! One enum for the NativeActivity lifecycle (from android-activity) and for
//! everything the Kotlin plugins report (insets, intents, text, feature
//! results), so an app handles platform facts in one place. Platform-free:
//! window *handles* are not carried — read `AndroidApp::native_window()` on
//! [`MobileEvent::WindowCreated`].

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
    /// The activity is being destroyed; the loop returns after this.
    Destroy,
}
