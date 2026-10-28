//! Pixel work on the camera thread: packing YUV_420_888 planes (dropping the
//! row and pixel strides), BT.601 full-range YUV to RGBA in fixed point,
//! scaling down by a whole factor, and turning by quarter turns.
//!
//! Plane lengths are not trusted: the last row of an `AImage` plane is often
//! shorter than its row stride (and, with a pixel stride of 2, than the
//! width), so every read is bounds-checked, with luma 0 and chroma 128 (no
//! colour) past the end.

#![cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android camera thread.

/// One YUV_420_888 image as borrowed planes and strides.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Planes<'a> {
    /// Luma size.
    pub width: usize,
    pub height: usize,
    pub y: &'a [u8],
    /// Bytes from one luma row to the next.
    pub y_row: usize,
    pub u: &'a [u8],
    pub v: &'a [u8],
    /// Bytes from one chroma row to the next (U and V share it on Android).
    pub uv_row: usize,
    /// Bytes from one chroma sample to the next: 1 (planar) or 2 (NV12 or
    /// NV21 seen through two overlapping planes).
    pub uv_pixel: usize,
}

impl Planes<'_> {
    fn luma(&self, x: usize, y: usize) -> u8 {
        self.y.get(y * self.y_row + x).copied().unwrap_or(0)
    }

    /// U and V at chroma coordinates.
    fn chroma(&self, cx: usize, cy: usize) -> (u8, u8) {
        let i = cy * self.uv_row + cx * self.uv_pixel;
        (
            self.u.get(i).copied().unwrap_or(128),
            self.v.get(i).copied().unwrap_or(128),
        )
    }
}

/// The luma plane, rows packed (`Gray8`).
#[cfg_attr(not(feature = "camera-capture"), allow(dead_code))] // Also for QR scanning.
pub(crate) fn pack_gray(p: &Planes<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.width * p.height);
    for row in 0..p.height {
        let start = row * p.y_row;
        match p.y.get(start..start + p.width) {
            Some(line) => out.extend_from_slice(line),
            None => (0..p.width).for_each(|x| out.push(p.luma(x, row))),
        }
    }
    out
}

/// 16.16 fixed-point BT.601 full-range coefficients.
const R_V: i32 = 91_881; // 1.402
const G_U: i32 = 22_554; // 0.344136
const G_V: i32 = 46_802; // 0.714136
const B_U: i32 = 116_130; // 1.772
const HALF: i32 = 1 << 15;

/// One pixel, BT.601 full range: `r = y + 1.402 (v−128)`,
/// `g = y − 0.344136 (u−128) − 0.714136 (v−128)`, `b = y + 1.772 (u−128)`.
pub(crate) fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let y = (y as i32) << 16;
    let u = u as i32 - 128;
    let v = v as i32 - 128;
    let clamp = |x: i32| ((x + HALF) >> 16).clamp(0, 255) as u8;
    [
        clamp(y + R_V * v),
        clamp(y - G_U * u - G_V * v),
        clamp(y + B_U * u),
    ]
}

/// RGBA, 4 bytes per pixel, rows packed, alpha 255 (`Rgba8`).
pub(crate) fn to_rgba(p: &Planes<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.width * p.height * 4);
    for row in 0..p.height {
        for x in 0..p.width {
            let (u, v) = p.chroma(x / 2, row / 2);
            let [r, g, b] = yuv_to_rgb(p.luma(x, row), u, v);
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    out
}

/// The factor a `width × height` image is divided by so that its longer side
/// fits in `max_side`: 1 when it already fits (or `max_side` is 0).
pub(crate) fn scale_factor(width: usize, height: usize, max_side: u32) -> usize {
    let longest = width.max(height);
    let max_side = max_side as usize;
    if max_side == 0 || longest <= max_side {
        1
    } else {
        longest.div_ceil(max_side)
    }
}

/// Three packed planes of a 4:2:0 image: luma `width × height`, chroma
/// `ceil(width / 2) × ceil(height / 2)` each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct I420 {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

/// The planes packed as I420, every `factor`-th pixel kept (point sampling;
/// `factor` 1 copies).
pub(crate) fn to_i420(p: &Planes<'_>, factor: usize) -> I420 {
    let f = factor.max(1);
    let width = p.width / f;
    let height = p.height / f;
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    let mut y = Vec::with_capacity(width * height);
    for row in 0..height {
        for x in 0..width {
            y.push(p.luma(x * f, row * f));
        }
    }
    let mut u = Vec::with_capacity(cw * ch);
    let mut v = Vec::with_capacity(cw * ch);
    for row in 0..ch {
        for x in 0..cw {
            let (cu, cv) = p.chroma(x * f, row * f);
            u.push(cu);
            v.push(cv);
        }
    }
    I420 {
        width,
        height,
        y,
        u,
        v,
    }
}

/// Turn a packed image of `bpp` bytes per pixel clockwise by `quarter_turns`
/// (taken mod 4). Returns the pixels and the new width and height.
pub(crate) fn rotate(
    data: &[u8],
    width: usize,
    height: usize,
    bpp: usize,
    quarter_turns: u8,
) -> (Vec<u8>, usize, usize) {
    let turns = quarter_turns % 4;
    if turns == 0 {
        return (data.to_vec(), width, height);
    }
    let (out_w, out_h) = if turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let mut out = vec![0u8; out_w * out_h * bpp];
    for sy in 0..height {
        for sx in 0..width {
            let (dx, dy) = match turns {
                1 => (height - 1 - sy, sx),
                2 => (width - 1 - sx, height - 1 - sy),
                _ => (sy, width - 1 - sx),
            };
            let src = (sy * width + sx) * bpp;
            let dst = (dy * out_w + dx) * bpp;
            if let Some(pixel) = data.get(src..src + bpp) {
                out[dst..dst + bpp].copy_from_slice(pixel);
            }
        }
    }
    (out, out_w, out_h)
}

impl I420 {
    /// Turn all three planes clockwise by `quarter_turns`.
    pub(crate) fn rotated(self, quarter_turns: u8) -> I420 {
        if quarter_turns.is_multiple_of(4) {
            return self;
        }
        let (cw, ch) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let (y, width, height) = rotate(&self.y, self.width, self.height, 1, quarter_turns);
        let (u, _, _) = rotate(&self.u, cw, ch, 1, quarter_turns);
        let (v, _, _) = rotate(&self.v, cw, ch, 1, quarter_turns);
        I420 {
            width,
            height,
            y,
            u,
            v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_drops_the_row_stride() {
        // 4 × 2, row stride 6; the last row is short, as on Android.
        let y = [10, 20, 30, 40, 0xee, 0xee, 50, 60, 70, 80];
        let p = Planes {
            width: 4,
            height: 2,
            y: &y,
            y_row: 6,
            u: &[128, 128],
            v: &[128, 128],
            uv_row: 2,
            uv_pixel: 1,
        };
        assert_eq!(pack_gray(&p), [10, 20, 30, 40, 50, 60, 70, 80]);
    }

    #[test]
    fn gray_reads_a_truncated_last_row_as_far_as_it_goes() {
        let y = [1, 2, 3, 4, 0, 0, 5, 6, 7];
        let p = Planes {
            width: 4,
            height: 2,
            y: &y,
            y_row: 6,
            u: &[],
            v: &[],
            uv_row: 2,
            uv_pixel: 1,
        };
        assert_eq!(pack_gray(&p), [1, 2, 3, 4, 5, 6, 7, 0]);
    }

    fn close(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    #[test]
    fn known_colours() {
        assert_eq!(yuv_to_rgb(128, 128, 128), [128, 128, 128]);
        assert_eq!(yuv_to_rgb(0, 128, 128), [0, 0, 0]);
        assert_eq!(yuv_to_rgb(255, 128, 128), [255, 255, 255]);
        // JFIF primaries: red (76, 85, 255), green (150, 44, 21), blue (29, 255, 107).
        assert!(
            close(yuv_to_rgb(76, 85, 255), [254, 0, 0]),
            "{:?}",
            yuv_to_rgb(76, 85, 255)
        );
        assert!(
            close(yuv_to_rgb(150, 44, 21), [0, 255, 1]),
            "{:?}",
            yuv_to_rgb(150, 44, 21)
        );
        assert!(
            close(yuv_to_rgb(29, 255, 107), [0, 0, 254]),
            "{:?}",
            yuv_to_rgb(29, 255, 107)
        );
    }

    #[test]
    fn rgba_with_planar_chroma() {
        // 2 × 2 luma, one chroma sample: red.
        let y = [76, 76, 76, 76];
        let p = Planes {
            width: 2,
            height: 2,
            y: &y,
            y_row: 2,
            u: &[85],
            v: &[255],
            uv_row: 1,
            uv_pixel: 1,
        };
        let rgba = to_rgba(&p);
        assert_eq!(rgba.len(), 16);
        for px in rgba.chunks(4) {
            assert!(close([px[0], px[1], px[2]], [254, 0, 0]));
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn rgba_with_interleaved_chroma() {
        // 4 × 2 luma: the left chroma sample grey, the right one blue. One
        // NV12-style buffer: U at even bytes, V at odd; Android hands it out
        // as two planes, V one byte after U, pixel stride 2.
        let y = [128, 128, 29, 29, 128, 128, 29, 29];
        let nv12 = [128u8, 128, 255, 107];
        let p = Planes {
            width: 4,
            height: 2,
            y: &y,
            y_row: 4,
            u: &nv12[0..3],
            v: &nv12[1..4],
            uv_row: 4,
            uv_pixel: 2,
        };
        let rgba = to_rgba(&p);
        for row in 0..2 {
            for x in 0..4 {
                let px = &rgba[(row * 4 + x) * 4..][..3];
                let want = if x < 2 { [128, 128, 128] } else { [0, 0, 254] };
                assert!(close([px[0], px[1], px[2]], want), "{x},{row}: {px:?}");
            }
        }
    }

    #[test]
    fn scale_factors() {
        assert_eq!(scale_factor(640, 480, 640), 1);
        assert_eq!(scale_factor(1280, 720, 640), 2);
        assert_eq!(scale_factor(1280, 720, 500), 3);
        assert_eq!(scale_factor(1280, 720, 0), 1);
        assert_eq!(scale_factor(720, 1280, 641), 2);
    }

    #[test]
    fn i420_packs_and_scales() {
        // 4 × 2, stride 5; NV12 chroma (2 samples).
        let y = [1, 2, 3, 4, 0, 5, 6, 7, 8, 0];
        let nv12 = [100u8, 200, 110, 210];
        let p = Planes {
            width: 4,
            height: 2,
            y: &y,
            y_row: 5,
            u: &nv12[0..3],
            v: &nv12[1..4],
            uv_row: 4,
            uv_pixel: 2,
        };
        let full = to_i420(&p, 1);
        assert_eq!((full.width, full.height), (4, 2));
        assert_eq!(full.y, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(full.u, [100, 110]);
        assert_eq!(full.v, [200, 210]);
        let half = to_i420(&p, 2);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.y, [1, 3]);
        assert_eq!(half.u, [100]);
        assert_eq!(half.v, [200]);
    }

    /// The 3 × 2 image
    /// ```text
    /// a b c
    /// d e f
    /// ```
    const IMG: [u8; 6] = [b'a', b'b', b'c', b'd', b'e', b'f'];

    #[test]
    fn rotate_3x2_each_quarter() {
        assert_eq!(rotate(&IMG, 3, 2, 1, 0), (IMG.to_vec(), 3, 2));
        // One turn clockwise: d a / e b / f c.
        assert_eq!(rotate(&IMG, 3, 2, 1, 1), (b"daebfc".to_vec(), 2, 3));
        // Two: f e d / c b a.
        assert_eq!(rotate(&IMG, 3, 2, 1, 2), (b"fedcba".to_vec(), 3, 2));
        // Three (one anticlockwise): c f / b e / a d.
        assert_eq!(rotate(&IMG, 3, 2, 1, 3), (b"cfbead".to_vec(), 2, 3));
        assert_eq!(rotate(&IMG, 3, 2, 1, 4), (IMG.to_vec(), 3, 2));
    }

    #[test]
    fn rotate_keeps_multi_byte_pixels_whole() {
        let rgba: Vec<u8> = (0..6).flat_map(|i| [i, i, i, 255]).collect();
        let (out, w, h) = rotate(&rgba, 3, 2, 4, 1);
        assert_eq!((w, h), (2, 3));
        let firsts: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        assert_eq!(firsts, [3, 0, 4, 1, 5, 2]);
        assert!(out.chunks(4).all(|p| p[1] == p[0] && p[3] == 255));
    }

    #[test]
    fn i420_rotates_every_plane() {
        let img = I420 {
            width: 4,
            height: 2,
            y: (0..8).collect(),
            u: vec![1, 2],
            v: vec![3, 4],
        };
        let r = img.rotated(1);
        assert_eq!((r.width, r.height), (2, 4));
        assert_eq!(r.y, [4, 0, 5, 1, 6, 2, 7, 3]);
        assert_eq!(r.u, [1, 2]);
        assert_eq!(r.v, [3, 4]);
    }
}
