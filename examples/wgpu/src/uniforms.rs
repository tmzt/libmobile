//! The shader's uniform block, laid out by hand to match `shader.wgsl`'s
//! `Uniforms` (std140-compatible: every field 8/16-byte aligned as declared).

use libmobile::SafeArea;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Uniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub pressed: f32,
    /// left, top, right, bottom.
    pub safe: [f32; 4],
    pub touch: [f32; 2],
}

impl Uniforms {
    /// Byte size of the WGSL struct, padding included.
    pub const SIZE: usize = 48;

    pub fn set_safe_area(&mut self, safe: SafeArea) {
        self.safe = [safe.left, safe.top, safe.right, safe.bottom];
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let floats: [f32; Self::SIZE / 4] = [
            self.resolution[0],
            self.resolution[1],
            self.time,
            self.pressed,
            self.safe[0],
            self.safe[1],
            self.safe[2],
            self.safe[3],
            self.touch[0],
            self.touch[1],
            0.0,
            0.0,
        ];
        let mut out = [0u8; Self::SIZE];
        for (chunk, value) in out.chunks_exact_mut(4).zip(floats) {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_wgsl_struct() {
        let mut u = Uniforms {
            resolution: [1080.0, 2316.0],
            time: 1.5,
            pressed: 1.0,
            touch: [10.0, 20.0],
            ..Default::default()
        };
        u.set_safe_area(SafeArea::new(0.0, 75.0, 0.0, 126.0));
        let bytes = u.to_bytes();
        let at = |i: usize| f32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!((at(0), at(1), at(2), at(3)), (1080.0, 2316.0, 1.5, 1.0));
        // `safe` is a vec4: offset 16.
        assert_eq!((at(5), at(7)), (75.0, 126.0));
        // `touch` follows at offset 32.
        assert_eq!((at(8), at(9)), (10.0, 20.0));
    }
}
