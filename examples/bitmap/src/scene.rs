//! Everything the bitmap example draws, as a pure function of the scene and a
//! pixel coordinate — no Android types, so it is tested on the host.

use libmobile::SafeArea;

/// Bytes per pixel of the `R8G8B8A8_UNORM` buffer the example asks for.
pub const BPP: usize = 4;

const TOUCH_RADIUS: f32 = 36.0;
const MAX_TOUCHES: usize = 64;
const BOX_SIZE: f32 = 120.0;
/// Box speed in px/s along each axis.
const BOX_SPEED: f32 = 420.0;

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub width: usize,
    pub height: usize,
    pub safe: SafeArea,
    touches: Vec<(f32, f32)>,
}

impl Scene {
    pub fn touch(&mut self, x: f32, y: f32) {
        if self.touches.len() == MAX_TOUCHES {
            self.touches.remove(0);
        }
        self.touches.push((x, y));
    }

    pub fn clear(&mut self) {
        self.touches.clear();
    }

    /// The bouncing box's top-left corner at time `t` seconds, kept inside the
    /// safe area.
    pub fn box_origin(&self, t: f32) -> (f32, f32) {
        let s = self.safe;
        let span_x = (self.width as f32 - s.left - s.right - BOX_SIZE).max(1.0);
        let span_y = (self.height as f32 - s.top - s.bottom - BOX_SIZE).max(1.0);
        (
            s.left + bounce(t * BOX_SPEED, span_x),
            s.top + bounce(t * BOX_SPEED * 0.73, span_y),
        )
    }

    /// The colour of pixel (`x`, `y`) at time `t`, as RGBA.
    pub fn shade(&self, x: usize, y: usize, t: f32, box_origin: (f32, f32)) -> [u8; 4] {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);

        for &(tx, ty) in &self.touches {
            let (dx, dy) = (fx - tx, fy - ty);
            if dx * dx + dy * dy <= TOUCH_RADIUS * TOUCH_RADIUS {
                return [255, 255, 255, 255];
            }
        }

        let (bx, by) = box_origin;
        if fx >= bx && fx < bx + BOX_SIZE && fy >= by && fy < by + BOX_SIZE {
            return [255, 196, 0, 255];
        }

        if !self.in_safe_area(fx, fy) {
            // Reserved edges: dark diagonal hatch, so the cutout and bars are
            // visibly not app content.
            return if ((x + y) / 12).is_multiple_of(2) {
                [24, 24, 32, 255]
            } else {
                [48, 48, 64, 255]
            };
        }

        // Content: a vertical gradient whose hue drifts with time.
        let v = fy / self.height.max(1) as f32;
        let phase = t * 0.3;
        let channel = |offset: f32| {
            let c = 0.5 + 0.5 * (std::f32::consts::TAU * (v * 0.5 + phase + offset)).cos();
            (40.0 + c * 160.0) as u8
        };
        [channel(0.0), channel(0.33), channel(0.67), 255]
    }

    fn in_safe_area(&self, x: f32, y: f32) -> bool {
        let s = self.safe;
        x >= s.left
            && y >= s.top
            && x < self.width as f32 - s.right
            && y < self.height as f32 - s.bottom
    }
}

/// Triangle wave over `[0, span]`.
fn bounce(distance: f32, span: f32) -> f32 {
    let m = distance.rem_euclid(2.0 * span);
    if m > span { 2.0 * span - m } else { m }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene {
            width: 400,
            height: 800,
            safe: SafeArea::new(0.0, 50.0, 0.0, 60.0),
            ..Default::default()
        }
    }

    #[test]
    fn reserved_edges_are_hatched() {
        let s = scene();
        let far = (1000.0, 1000.0);
        let top = s.shade(10, 10, 0.0, far);
        let bottom = s.shade(10, 790, 0.0, far);
        for px in [top, bottom] {
            assert!(px[0] <= 48 && px[0] == px[1], "{px:?}");
        }
    }

    #[test]
    fn touches_draw_white_dots_over_everything() {
        let mut s = scene();
        s.touch(200.0, 10.0);
        assert_eq!(
            s.shade(200, 10, 0.0, (1000.0, 1000.0)),
            [255, 255, 255, 255]
        );
        s.clear();
        assert_ne!(
            s.shade(200, 10, 0.0, (1000.0, 1000.0)),
            [255, 255, 255, 255]
        );
    }

    #[test]
    fn touches_are_capped() {
        let mut s = scene();
        for i in 0..100 {
            s.touch(i as f32, 0.0);
        }
        assert_eq!(s.touches.len(), MAX_TOUCHES);
        assert_eq!(s.touches[0], (36.0, 0.0));
    }

    #[test]
    fn box_stays_inside_safe_area() {
        let s = scene();
        for step in 0..500 {
            let (x, y) = s.box_origin(step as f32 * 0.037);
            assert!(x >= 0.0 && x + BOX_SIZE <= 400.0 + 1.0);
            assert!(y >= 50.0 && y + BOX_SIZE <= 740.0 + 1.0, "y={y}");
        }
    }

    #[test]
    fn bounce_is_a_triangle_wave() {
        assert_eq!(bounce(0.0, 10.0), 0.0);
        assert_eq!(bounce(10.0, 10.0), 10.0);
        assert_eq!(bounce(15.0, 10.0), 5.0);
        assert_eq!(bounce(20.0, 10.0), 0.0);
    }
}
