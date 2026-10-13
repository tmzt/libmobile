//! A safe `raw-window-handle` target for the activity's window.
//!
//! `ndk`'s `NativeWindow` implements [`HasWindowHandle`] (its `rwh_06`
//! feature), and Android's display handle carries no data, so
//! [`DisplayHandle::android`] is a safe constructor. Pairing the two lets a
//! renderer use a *safe* surface constructor (e.g. wgpu's `create_surface`)
//! rather than a raw-handle one.
//!
//! The target owns its window reference (`NativeWindow` is refcounted), so a
//! surface built from it can be `'static`. Drop that surface on
//! [`MobileEvent::WindowDestroyed`](crate::MobileEvent::WindowDestroyed).

use android_activity::AndroidApp;
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

pub struct SurfaceTarget(pub NativeWindow);

impl SurfaceTarget {
    /// The current native window, if one exists (between `WindowCreated` and
    /// `WindowDestroyed`).
    pub fn current(app: &AndroidApp) -> Option<SurfaceTarget> {
        app.native_window().map(SurfaceTarget)
    }

    /// Size in physical pixels, never zero.
    pub fn size(&self) -> (u32, u32) {
        (self.0.width().max(1) as u32, self.0.height().max(1) as u32)
    }
}

impl HasWindowHandle for SurfaceTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for SurfaceTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}
