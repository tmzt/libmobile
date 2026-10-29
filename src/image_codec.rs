//! **Decoding and encoding images through Android's own codecs** (feature
//! `image-codec`): the platform-free types. The calls are in
//! `android::image_codec`.
//!
//! Decoding goes through `android.graphics.ImageDecoder`, which applies the
//! EXIF orientation, reads HEIC/HEIF, PNG, JPEG, WebP, GIF, BMP and DNG, and
//! gives the first frame of an animation. Encoding goes through
//! `Bitmap.compress` (JPEG or PNG). Pixels are always [`Rgba`]: sRGB,
//! straight alpha, rows packed.

use std::fmt;

/// The largest decode or encode, in pixels: 64 Mi, 256 MiB of [`Rgba`].
pub const IMAGE_MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// The longest side `android::image_codec::encode_image` takes.
const MAX_ENCODE_SIDE: u32 = 32768;

/// Pixels: rows top to bottom, 4 bytes R, G, B, A each, sRGB, straight
/// (not premultiplied) alpha; `pixels.len() == width * height * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    /// Checks the length; [`ImageError::BadInput`] otherwise.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        if !length_matches(width, height, pixels.len()) {
            return Err(ImageError::BadInput(BAD_LENGTH));
        }
        Ok(Rgba {
            width,
            height,
            pixels,
        })
    }

    /// Every alpha is 255. A JPEG has no alpha, so an image that is not
    /// opaque is encoded as PNG (or flattened first).
    pub fn is_opaque(&self) -> bool {
        self.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255)
    }
}

const BAD_LENGTH: &str = "pixels.len() is not width * height * 4";

fn length_matches(width: u32, height: u32, len: usize) -> bool {
    (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .is_some_and(|n| n == len as u64)
}

/// What `android::image_codec::probe_image` reads from an image's headers,
/// without decoding it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    /// `image/jpeg`, `image/heif`, `image/png`, …
    pub mime: String,
    /// As stored, before the EXIF rotation.
    pub width: u32,
    pub height: u32,
    /// 1..=8, 0 when absent.
    pub exif_orientation: u8,
}

impl ImageInfo {
    /// From the plugin's answer; an orientation outside 1..=8 is 0.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_parts(mime: String, width: i32, height: i32, orientation: i32) -> Self {
        ImageInfo {
            mime,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            exif_orientation: match orientation {
                1..=8 => orientation as u8,
                _ => 0,
            },
        }
    }
}

/// How `android::image_codec::encode_image` writes the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Encoding {
    /// JPEG at `quality` 0..=100. JPEG has no alpha: Android's encoder
    /// ignores it, so a transparent pixel's colour shows.
    Jpeg { quality: u8 },
    /// PNG, lossless, with alpha.
    Png,
}

/// Why an image could not be probed, decoded or encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageError {
    /// Not an image format Android decodes.
    Unsupported,
    Malformed,
    /// The data ends early.
    Incomplete,
    /// The decoded size (after `max_side`) would be over [`IMAGE_MAX_PIXELS`].
    TooLarge {
        width: u32,
        height: u32,
    },
    /// Bad arguments: what was wrong.
    BadInput(&'static str),
    OutOfMemory,
    /// Anything else, for logs.
    Platform(String),
}

impl ImageError {
    /// From the plugin's `"<kind>: <detail>"` message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> Self {
        let (kind, detail) = match message.split_once(':') {
            Some((kind, detail)) => (kind.trim(), detail.trim()),
            None => (message.trim(), ""),
        };
        match kind {
            "unsupported" => ImageError::Unsupported,
            "malformed" => ImageError::Malformed,
            "incomplete" => ImageError::Incomplete,
            "out-of-memory" => ImageError::OutOfMemory,
            "too-large" => match parse_size(detail) {
                Some((width, height)) => ImageError::TooLarge { width, height },
                None => ImageError::Platform(message.to_string()),
            },
            "platform" if !detail.is_empty() => ImageError::Platform(detail.to_string()),
            _ => ImageError::Platform(message.to_string()),
        }
    }
}

/// `"<w>x<h>"`.
fn parse_size(detail: &str) -> Option<(u32, u32)> {
    let (width, height) = detail.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Unsupported => f.write_str("not an image format Android decodes"),
            ImageError::Malformed => f.write_str("the image data is malformed"),
            ImageError::Incomplete => f.write_str("the image data ends early"),
            ImageError::TooLarge { width, height } => write!(
                f,
                "a {width}x{height} image is over the limit of {IMAGE_MAX_PIXELS} pixels"
            ),
            ImageError::BadInput(what) => write!(f, "bad input: {what}"),
            ImageError::OutOfMemory => f.write_str("out of memory"),
            ImageError::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// The size `decode_image` produces for a `width` × `height` image (as
/// displayed): the long side at most `max_side`, aspect kept, at least 1,
/// never enlarged; 0 keeps the size.
///
/// The Kotlin side (`image/ImageCodecPlugin.kt`, `fitWithin`) computes the
/// same in its header listener; keep the two in step.
pub fn fit_within(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if max_side == 0 || long <= max_side {
        return (width, height);
    }
    let scale = |side: u32| -> u32 {
        // Rounded to the nearest: side * max_side / long, at least 1.
        let scaled = (side as u64 * max_side as u64 + long as u64 / 2) / long as u64;
        (scaled as u32).max(1)
    };
    if width >= height {
        (max_side, scale(height))
    } else {
        (scale(width), max_side)
    }
}

/// The argument checks `encode_image` makes before anything reaches
/// Android: the length, both sides 1..=32768, at most [`IMAGE_MAX_PIXELS`],
/// and a JPEG quality of at most 100.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn check_encode(image: &Rgba, encoding: Encoding) -> Result<(), ImageError> {
    if !length_matches(image.width, image.height, image.pixels.len()) {
        return Err(ImageError::BadInput(BAD_LENGTH));
    }
    let side = 1..=MAX_ENCODE_SIDE;
    if !side.contains(&image.width) || !side.contains(&image.height) {
        return Err(ImageError::BadInput("width or height outside 1..=32768"));
    }
    if image.width as u64 * image.height as u64 > IMAGE_MAX_PIXELS {
        return Err(ImageError::BadInput("more pixels than IMAGE_MAX_PIXELS"));
    }
    if let Encoding::Jpeg { quality } = encoding
        && quality > 100
    {
        return Err(ImageError::BadInput("JPEG quality over 100"));
    }
    Ok(())
}

/// Packs `height` rows of `row_bytes` each, in place, to `width * 4` bytes
/// each, and truncates: `Bitmap.copyPixelsToBuffer` copies the padding too.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn pack_rows(pixels: &mut Vec<u8>, width: usize, height: usize, row_bytes: usize) {
    let packed = width * 4;
    if row_bytes > packed {
        for row in 1..height {
            pixels.copy_within(row * row_bytes..row * row_bytes + packed, row * packed);
        }
    }
    pixels.truncate(packed * height);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_scales_the_long_side() {
        assert_eq!(fit_within(4000, 3000, 2048), (2048, 1536));
        assert_eq!(fit_within(3000, 4000, 2048), (1536, 2048));
        assert_eq!(fit_within(5000, 5000, 192), (192, 192));
        // 0 keeps the size.
        assert_eq!(fit_within(4000, 3000, 0), (4000, 3000));
        // Never enlarged.
        assert_eq!(fit_within(100, 50, 2048), (100, 50));
        assert_eq!(fit_within(2048, 10, 2048), (2048, 10));
        // A strip keeps at least 1 px.
        assert_eq!(fit_within(1, 10000, 192), (1, 192));
        assert_eq!(fit_within(10000, 1, 192), (192, 1));
        // Rounded to the nearest.
        assert_eq!(fit_within(4032, 3024, 192), (192, 144));
        assert_eq!(fit_within(1000, 333, 100), (100, 33));
        assert_eq!(fit_within(1000, 335, 100), (100, 34));
    }

    #[test]
    fn rgba_new_checks_the_length() {
        assert!(Rgba::new(2, 3, vec![0; 24]).is_ok());
        assert_eq!(
            Rgba::new(2, 3, vec![0; 23]),
            Err(ImageError::BadInput(BAD_LENGTH))
        );
        assert!(Rgba::new(0, 0, Vec::new()).is_ok());
        assert!(Rgba::new(u32::MAX, u32::MAX, Vec::new()).is_err());
    }

    #[test]
    fn is_opaque_looks_at_every_alpha() {
        let mut image = Rgba::new(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
        assert!(image.is_opaque());
        image.pixels[7] = 254;
        assert!(!image.is_opaque());
        assert!(Rgba::new(0, 0, Vec::new()).unwrap().is_opaque());
    }

    #[test]
    fn encode_arguments_are_checked() {
        let ok = Rgba::new(2, 2, vec![0; 16]).unwrap();
        assert_eq!(check_encode(&ok, Encoding::Png), Ok(()));
        assert_eq!(check_encode(&ok, Encoding::Jpeg { quality: 100 }), Ok(()));
        assert_eq!(check_encode(&ok, Encoding::Jpeg { quality: 0 }), Ok(()));
        assert!(matches!(
            check_encode(&ok, Encoding::Jpeg { quality: 101 }),
            Err(ImageError::BadInput(_))
        ));
        // Png ignores quality, so there is nothing to check there.
        let short = Rgba {
            width: 2,
            height: 2,
            pixels: vec![0; 15],
        };
        assert_eq!(
            check_encode(&short, Encoding::Png),
            Err(ImageError::BadInput(BAD_LENGTH))
        );
        let empty = Rgba::new(0, 4, Vec::new()).unwrap();
        assert!(matches!(
            check_encode(&empty, Encoding::Png),
            Err(ImageError::BadInput(_))
        ));
        let wide = Rgba {
            width: 32769,
            height: 1,
            pixels: vec![0; 32769 * 4],
        };
        assert!(matches!(
            check_encode(&wide, Encoding::Png),
            Err(ImageError::BadInput(_))
        ));
    }

    #[test]
    fn the_pixel_limit_is_checked() {
        // 8193 x 8193 is just over 64 Mi pixels (256 MiB): allocated once here.
        let side = 8193u32;
        let image = Rgba::new(side, side, vec![0; (side * side * 4) as usize]).unwrap();
        assert_eq!(
            check_encode(&image, Encoding::Png),
            Err(ImageError::BadInput("more pixels than IMAGE_MAX_PIXELS"))
        );
    }

    #[test]
    fn messages_map_to_errors() {
        assert_eq!(
            ImageError::from_message("unsupported: "),
            ImageError::Unsupported
        );
        assert_eq!(
            ImageError::from_message("unsupported"),
            ImageError::Unsupported
        );
        assert_eq!(
            ImageError::from_message("malformed: x"),
            ImageError::Malformed
        );
        assert_eq!(
            ImageError::from_message("incomplete: "),
            ImageError::Incomplete
        );
        assert_eq!(
            ImageError::from_message("out-of-memory: "),
            ImageError::OutOfMemory
        );
        assert_eq!(
            ImageError::from_message("too-large: 9000x8000"),
            ImageError::TooLarge {
                width: 9000,
                height: 8000
            }
        );
        assert_eq!(
            ImageError::from_message("too-large: bogus"),
            ImageError::Platform("too-large: bogus".into())
        );
        assert_eq!(
            ImageError::from_message("platform: java.io.IOException"),
            ImageError::Platform("java.io.IOException".into())
        );
        assert_eq!(
            ImageError::from_message("something else"),
            ImageError::Platform("something else".into())
        );
    }

    #[test]
    fn info_orientation_is_clamped() {
        let info = ImageInfo::from_parts("image/jpeg".into(), 4032, 3024, 6);
        assert_eq!(
            (info.width, info.height, info.exif_orientation),
            (4032, 3024, 6)
        );
        assert_eq!(
            ImageInfo::from_parts(String::new(), 1, 1, 0).exif_orientation,
            0
        );
        assert_eq!(
            ImageInfo::from_parts(String::new(), 1, 1, 9).exif_orientation,
            0
        );
        assert_eq!(
            ImageInfo::from_parts(String::new(), 1, 1, -1).exif_orientation,
            0
        );
        assert_eq!(ImageInfo::from_parts(String::new(), -5, 1, 1).width, 0);
    }

    #[test]
    fn padded_rows_are_packed() {
        // 2 x 3 pixels, rows of 12 bytes (4 of padding).
        let mut pixels: Vec<u8> = (0..36).collect();
        pack_rows(&mut pixels, 2, 3, 12);
        let expected: Vec<u8> = [0..8, 12..20, 24..32].into_iter().flatten().collect();
        assert_eq!(pixels, expected);
        // Unpadded rows are left as they are.
        let mut pixels: Vec<u8> = (0..24).collect();
        pack_rows(&mut pixels, 2, 3, 8);
        assert_eq!(pixels, (0..24).collect::<Vec<u8>>());
    }

    #[test]
    fn errors_display() {
        assert!(ImageError::Unsupported.to_string().contains("format"));
        assert!(
            ImageError::TooLarge {
                width: 1,
                height: 2
            }
            .to_string()
            .contains("1x2")
        );
        assert_eq!(
            ImageError::Platform("x".into()).to_string(),
            "platform error: x"
        );
    }
}
