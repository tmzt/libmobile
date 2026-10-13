//! **The platform safe area, as plain numbers.**
//!
//! The activity runs edge-to-edge on every supported API level, so the surface
//! Rust draws into is the whole panel: the display cutout and the system bars
//! are drawn by the OS *over* the app's pixels. `WindowInsetsPlugin` measures
//! how much of each edge they take and reports it here; nothing downstream
//! learns it came from Android.

/// Edge reservations in physical pixels — the space the surface size and
/// touch coordinates arrive in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeArea {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl SafeArea {
    pub const NONE: SafeArea = SafeArea {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        SafeArea {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Negative or non-finite edges become zero: a bogus report must never
    /// grow the drawable area beyond the surface.
    pub fn clamped(self) -> Self {
        let edge = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        SafeArea::new(
            edge(self.left),
            edge(self.top),
            edge(self.right),
            edge(self.bottom),
        )
    }
}

/// One inset report.
///
/// `safe_area` is the union of the system bars and the display cutout —
/// chrome the app must never draw under. `ime_bottom` is kept apart on
/// purpose: the soft keyboard is a transient overlay a focused field scrolls
/// for, not a permanent reservation the whole layout shrinks into. It is the
/// keyboard's occlusion *above* `safe_area.bottom`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub safe_area: SafeArea,
    pub ime_bottom: f32,
}

impl Insets {
    /// Build from the raw integers the Kotlin bridge reports.
    pub fn from_px(left: i32, top: i32, right: i32, bottom: i32, ime_bottom: i32) -> Self {
        Insets {
            safe_area: SafeArea::new(left as f32, top as f32, right as f32, bottom as f32)
                .clamped(),
            ime_bottom: ime_bottom.max(0) as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamped_zeroes_bad_edges() {
        let s = SafeArea::new(-1.0, f32::NAN, f32::INFINITY, 12.0).clamped();
        assert_eq!(s, SafeArea::new(0.0, 0.0, 0.0, 12.0));
    }

    #[test]
    fn from_px_clamps_and_keeps_ime_apart() {
        let insets = Insets::from_px(0, 75, 0, 126, -4);
        assert_eq!(insets.safe_area, SafeArea::new(0.0, 75.0, 0.0, 126.0));
        assert_eq!(insets.ime_bottom, 0.0);
    }
}
