//! **Launcher icon rasters** (feature `icons`): the PNG mipmaps an icon
//! makes, as pure functions of the decoded sources. The file I/O and the
//! XML are elsewhere (`crate::gradle`, `crate::codegen::adaptive_icon_xml`).
//!
//! Kept out of `codegen.rs`, which `build.rs` also compiles and which has
//! no `image` dependency there.
//!
//! Scaling is Lanczos3 on the sRGB values, as Android's own tools do, with
//! the colour premultiplied by alpha so transparent pixels leave no fringe.
//! Layers are composed and masked premultiplied, and written as RGBA PNGs.

use crate::codegen::{ICON_DENSITIES, RES_DIR};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{ExtendedColorType, ImageEncoder, Rgba, Rgba32FImage, RgbaImage};

/// The adaptive canvas is 108 dp; the artwork goes in its centre 66 dp.
const CANVAS_DP: u32 = 108;
const SAFE_DP: u32 = 66;
/// The part of the canvas a launcher shows before masking, 72 dp: what a
/// legacy icon (48 dp) is made from.
const VIEWPORT_DP: u32 = 72;

/// An adaptive icon's background.
pub(crate) enum Background<'a> {
    Color([u8; 3]),
    /// Square; used at full bleed.
    Png(&'a RgbaImage),
}

/// The decoded sources of an icon. Every image is square.
pub(crate) struct Sources<'a> {
    /// The master, or PNGs at known sizes: an output whose artwork is `n`
    /// pixels uses the one `n` pixels wide, else the largest.
    pub artwork: &'a [RgbaImage],
    pub background: Background<'a>,
    /// Laid out like the foreground.
    pub monochrome: Option<&'a RgbaImage>,
    /// The artwork already fills the 108 dp canvas: use it unchanged.
    pub full_bleed: bool,
}

/// Every PNG the icon makes, as `(path relative to the project, bytes)`:
/// per density, `ic_launcher.png` and `ic_launcher_round.png` (legacy
/// sizes), `ic_launcher_foreground.png` and, when given,
/// `ic_launcher_background.png` and `ic_launcher_monochrome.png`
/// (adaptive sizes).
pub(crate) fn icon_images(sources: &Sources) -> Vec<(String, Vec<u8>)> {
    assert!(!sources.artwork.is_empty(), "an icon needs artwork");
    let mut files = Vec::new();
    for (density, legacy, adaptive) in ICON_DENSITIES {
        let path = |name: &str| format!("{RES_DIR}/mipmap-{density}/{name}.png");
        let (square, round) = legacy_icons(sources, legacy);
        files.push((path("ic_launcher"), encode(&square)));
        files.push((path("ic_launcher_round"), encode(&round)));
        let foreground = layer(sources.artwork, adaptive, sources.full_bleed);
        files.push((path("ic_launcher_foreground"), encode(&foreground)));
        if let Background::Png(png) = sources.background {
            files.push((
                path("ic_launcher_background"),
                encode(&scale(png, adaptive)),
            ));
        }
        if let Some(mono) = sources.monochrome {
            let mono = layer(std::slice::from_ref(mono), adaptive, sources.full_bleed);
            files.push((path("ic_launcher_monochrome"), encode(&mono)));
        }
    }
    files
}

/// The artwork's size on a canvas of `canvas` pixels: the centre 66/108,
/// rounded down so that it sits on whole pixels in the middle.
pub(crate) fn artwork_size(canvas: u32) -> u32 {
    let size = canvas * SAFE_DP / CANVAS_DP;
    if (canvas - size) % 2 == 1 {
        size - 1
    } else {
        size
    }
}

/// The legacy icon of `size` pixels, square with rounded corners (radius
/// 1/6 of the size) and round: the centre 72 dp of the adaptive icon,
/// foreground over background, transparent outside the shape.
fn legacy_icons(sources: &Sources, size: u32) -> (Rgba32FImage, Rgba32FImage) {
    let canvas = size * CANVAS_DP / VIEWPORT_DP;
    let mut composed = background(&sources.background, canvas);
    over(
        &mut composed,
        &layer(sources.artwork, canvas, sources.full_bleed),
    );
    let at = (canvas - size) / 2;
    let viewport = imageops::crop_imm(&composed, at, at, size, size).to_image();
    let mut square = viewport.clone();
    mask(&mut square, size as f32 / 6.0);
    let mut round = viewport;
    mask(&mut round, size as f32 / 2.0);
    (square, round)
}

/// One layer on a transparent canvas of `canvas` pixels, premultiplied.
fn layer(artwork: &[RgbaImage], canvas: u32, full_bleed: bool) -> Rgba32FImage {
    if full_bleed {
        return scale(pick(artwork, canvas), canvas);
    }
    let size = artwork_size(canvas);
    let mut out = Rgba32FImage::new(canvas, canvas);
    let at = (canvas - size) / 2;
    imageops::replace(
        &mut out,
        &scale(pick(artwork, size), size),
        at.into(),
        at.into(),
    );
    out
}

fn background(background: &Background, canvas: u32) -> Rgba32FImage {
    match background {
        Background::Color([r, g, b]) => Rgba32FImage::from_pixel(
            canvas,
            canvas,
            Rgba([*r as f32 / 255.0, *g as f32 / 255.0, *b as f32 / 255.0, 1.0]),
        ),
        Background::Png(png) => scale(png, canvas),
    }
}

/// The source `size` pixels wide, else the largest.
fn pick(artwork: &[RgbaImage], size: u32) -> &RgbaImage {
    artwork
        .iter()
        .find(|image| image.width() == size)
        .or_else(|| artwork.iter().max_by_key(|image| image.width()))
        .expect("an icon needs artwork")
}

/// `image` scaled to `size` square with Lanczos3, premultiplied.
fn scale(image: &RgbaImage, size: u32) -> Rgba32FImage {
    let mut premultiplied = Rgba32FImage::new(image.width(), image.height());
    for (to, from) in premultiplied.pixels_mut().zip(image.pixels()) {
        let a = from[3] as f32 / 255.0;
        *to = Rgba([
            from[0] as f32 / 255.0 * a,
            from[1] as f32 / 255.0 * a,
            from[2] as f32 / 255.0 * a,
            a,
        ]);
    }
    let mut scaled = imageops::resize(&premultiplied, size, size, FilterType::Lanczos3);
    // Lanczos rings: keep each pixel a valid premultiplied colour.
    for pixel in scaled.pixels_mut() {
        let a = pixel[3].clamp(0.0, 1.0);
        for c in 0..3 {
            pixel[c] = pixel[c].clamp(0.0, a);
        }
        pixel[3] = a;
    }
    scaled
}

/// `top` over `bottom`, both premultiplied and the same size.
fn over(bottom: &mut Rgba32FImage, top: &Rgba32FImage) {
    for (b, t) in bottom.pixels_mut().zip(top.pixels()) {
        let keep = 1.0 - t[3];
        for c in 0..4 {
            b[c] = t[c] + b[c] * keep;
        }
    }
}

/// Keep the rounded square of corner `radius` filling the image (a circle
/// at half the size), anti-aliased over one pixel; transparent outside.
fn mask(image: &mut Rgba32FImage, radius: f32) {
    let half = image.width() as f32 / 2.0;
    let inner = half - radius;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let qx = (x as f32 + 0.5 - half).abs() - inner;
        let qy = (y as f32 + 0.5 - half).abs() - inner;
        let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
        let coverage = (0.5 - outside).clamp(0.0, 1.0);
        for c in 0..4 {
            pixel[c] *= coverage;
        }
    }
}

/// An RGBA PNG of the premultiplied `image`, with fixed settings so the
/// same sources give the same bytes.
fn encode(image: &Rgba32FImage) -> Vec<u8> {
    let mut rgba = RgbaImage::new(image.width(), image.height());
    for (to, from) in rgba.pixels_mut().zip(image.pixels()) {
        let a = from[3];
        let straight = |c: f32| if a > 0.0 { c / a } else { 0.0 };
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        *to = Rgba([
            byte(straight(from[0])),
            byte(straight(from[1])),
            byte(straight(from[2])),
            byte(a),
        ]);
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Best, PngFilter::Adaptive)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            ExtendedColorType::Rgba8,
        )
        .expect("encoding a PNG into memory");
    png
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A square of `size` pixels: `color` inside `[from, to)` on both axes,
    /// transparent elsewhere.
    pub(crate) fn square(size: u32, from: u32, to: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(size, size, |x, y| {
            if (from..to).contains(&x) && (from..to).contains(&y) {
                Rgba(color)
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    fn decode(png: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    }

    fn file(files: &[(String, Vec<u8>)], path: &str) -> RgbaImage {
        decode(&files.iter().find(|(p, _)| p == path).unwrap().1)
    }

    /// The bounds of the pixels with any alpha, `(min, max)` inclusive.
    fn bounds(image: &RgbaImage) -> (u32, u32, u32, u32) {
        let mut b = (u32::MAX, u32::MAX, 0, 0);
        for (x, y, p) in image.enumerate_pixels() {
            if p[3] > 0 {
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
        b
    }

    fn opaque_master() -> RgbaImage {
        square(1024, 0, 1024, [200, 30, 40, 255])
    }

    #[test]
    fn every_size_is_made() {
        let master = opaque_master();
        let mono = square(512, 0, 512, [255, 255, 255, 255]);
        let bg = square(600, 0, 600, [0, 0, 255, 255]);
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Png(&bg),
            monochrome: Some(&mono),
            full_bleed: false,
        });
        assert_eq!(files.len(), 5 * 5);
        for (density, legacy, adaptive) in ICON_DENSITIES {
            let dir = format!("app/src/main/res/mipmap-{density}");
            for (name, size) in [
                ("ic_launcher", legacy),
                ("ic_launcher_round", legacy),
                ("ic_launcher_foreground", adaptive),
                ("ic_launcher_background", adaptive),
                ("ic_launcher_monochrome", adaptive),
            ] {
                let image = file(&files, &format!("{dir}/{name}.png"));
                assert_eq!(image.dimensions(), (size, size), "{density} {name}");
            }
        }
    }

    #[test]
    fn only_given_layers_are_made() {
        let master = opaque_master();
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([255, 255, 255]),
            monochrome: None,
            full_bleed: false,
        });
        assert_eq!(files.len(), 5 * 3);
        assert!(
            files
                .iter()
                .all(|(p, _)| !p.contains("background") && !p.contains("monochrome"))
        );
    }

    #[test]
    fn artwork_sits_in_the_centre_66_dp() {
        let master = opaque_master();
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([255, 255, 255]),
            monochrome: Some(&master),
            full_bleed: false,
        });
        for (density, _, canvas) in ICON_DENSITIES {
            for name in ["ic_launcher_foreground", "ic_launcher_monochrome"] {
                let image = file(&files, &format!("{RES_DIR}/mipmap-{density}/{name}.png"));
                let (x0, y0, x1, y1) = bounds(&image);
                // Inside the 66 dp square, centred, and at least 65 dp wide.
                let safe = canvas as f32 * 66.0 / 108.0;
                let lo = (canvas as f32 - safe) / 2.0;
                assert!(
                    x0 as f32 >= lo.floor() && y0 as f32 >= lo.floor(),
                    "{density}"
                );
                assert!((x1 + 1) as f32 <= (lo + safe).ceil(), "{density}");
                assert_eq!((x0, y0), (canvas - 1 - x1, canvas - 1 - y1), "centred");
                assert!((x1 + 1 - x0) as f32 >= canvas as f32 * 65.0 / 108.0);
                assert_eq!(image.get_pixel(canvas / 2, canvas / 2)[3], 255);
            }
        }
        assert_eq!(artwork_size(432), 264);
        assert_eq!(artwork_size(162), 98);
    }

    #[test]
    fn full_bleed_uses_the_whole_canvas() {
        let master = opaque_master();
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([0, 0, 0]),
            monochrome: None,
            full_bleed: true,
        });
        let image = file(
            &files,
            "app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png",
        );
        assert_eq!(bounds(&image), (0, 0, 431, 431));
    }

    #[test]
    fn legacy_and_round_icons_have_transparent_corners() {
        let master = square(1024, 300, 724, [0, 200, 0, 255]);
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([10, 20, 30]),
            monochrome: None,
            full_bleed: false,
        });
        for (density, size, _) in ICON_DENSITIES {
            let legacy = file(
                &files,
                &format!("{RES_DIR}/mipmap-{density}/ic_launcher.png"),
            );
            let round = file(
                &files,
                &format!("{RES_DIR}/mipmap-{density}/ic_launcher_round.png"),
            );
            let last = size - 1;
            for (x, y) in [(0, 0), (last, 0), (0, last), (last, last)] {
                assert_eq!(legacy.get_pixel(x, y)[3], 0, "{density} legacy corner");
                assert_eq!(round.get_pixel(x, y)[3], 0, "{density} round corner");
            }
            // Inside the rounded square but outside the circle.
            let near = size / 8;
            assert_eq!(
                legacy.get_pixel(near, near).0,
                [10, 20, 30, 255],
                "{density}"
            );
            assert_eq!(round.get_pixel(near, near)[3], 0, "{density}");
            // The middle of each edge is inside both, on the background.
            for icon in [&legacy, &round] {
                assert_eq!(
                    icon.get_pixel(size / 2, 1).0,
                    [10, 20, 30, 255],
                    "{density}"
                );
                assert_eq!(icon.get_pixel(size / 2, size / 2).0, [0, 200, 0, 255]);
            }
        }
    }

    #[test]
    fn a_given_size_is_used_unscaled() {
        // At xxxhdpi the foreground's artwork is 264 px.
        let large = opaque_master();
        let exact = square(264, 0, 264, [1, 2, 3, 255]);
        let artwork = [large, exact];
        let files = icon_images(&Sources {
            artwork: &artwork,
            background: Background::Color([255, 255, 255]),
            monochrome: None,
            full_bleed: false,
        });
        let fg = file(
            &files,
            "app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png",
        );
        assert_eq!(fg.get_pixel(216, 216).0, [1, 2, 3, 255]);
        let other = file(
            &files,
            "app/src/main/res/mipmap-mdpi/ic_launcher_foreground.png",
        );
        assert_eq!(
            other.get_pixel(54, 54).0,
            [200, 30, 40, 255],
            "from the largest"
        );
    }

    #[test]
    fn transparent_edges_leave_no_dark_fringe() {
        // A white disc-ish square on transparent black: scaled, the edge
        // pixels are partly transparent white, never grey.
        let master = square(1024, 256, 768, [255, 255, 255, 255]);
        let files = icon_images(&Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([0, 0, 0]),
            monochrome: None,
            full_bleed: true,
        });
        let fg = file(
            &files,
            "app/src/main/res/mipmap-hdpi/ic_launcher_foreground.png",
        );
        for p in fg.pixels().filter(|p| p[3] > 8) {
            assert!(p[0] >= 250 && p[1] >= 250 && p[2] >= 250, "{p:?}");
        }
    }

    #[test]
    fn output_is_deterministic() {
        let master = square(500, 100, 400, [9, 99, 199, 255]);
        let sources = Sources {
            artwork: std::slice::from_ref(&master),
            background: Background::Color([1, 1, 1]),
            monochrome: None,
            full_bleed: false,
        };
        assert_eq!(icon_images(&sources), icon_images(&sources));
    }
}
