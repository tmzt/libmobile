//! Finding and reading QR codes in a grey frame (plan 6.1 and 9): the
//! decoder behind a private trait, the measurements auto zoom needs, the
//! 2 × 2 retry for moiré, and the luma statistics of the screen test (7.1).
//!
//! Everything here runs on the `libmobile-qr` thread (or a host test); it
//! never touches the camera.

use super::{QR_RETRY_MODULE_PX, QR_SAT_FRACTION, QR_SAT_LEVEL, QR_SAT_MEAN};

/// A greyscale image, rows packed: the analysis stream's Y plane as the
/// sensor delivers it (not turned upright).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GreyFrame {
    pub width: usize,
    pub height: usize,
    /// `width × height` bytes, 0 black to 255 white.
    pub data: Vec<u8>,
}

impl GreyFrame {
    fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    /// The frame box-filtered 2 × 2 (half the size, each pixel the mean of
    /// four): a screen's pixel grid at the code's scale averages out.
    pub(crate) fn halved(&self) -> GreyFrame {
        let (w, h) = (self.width / 2, self.height / 2);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let sum = self.at(2 * x, 2 * y) as u32
                    + self.at(2 * x + 1, 2 * y) as u32
                    + self.at(2 * x, 2 * y + 1) as u32
                    + self.at(2 * x + 1, 2 * y + 1) as u32;
                data.push(((sum + 2) / 4) as u8);
            }
        }
        GreyFrame {
            width: w,
            height: h,
            data,
        }
    }
}

/// One code a decoder saw. Corners are in frame pixels: the code's top-left,
/// top-right, bottom-right and bottom-left.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Detection {
    /// Read.
    Decoded {
        bytes: Vec<u8>,
        corners: [(f32, f32); 4],
        /// QR version 1..=40.
        version: u8,
        /// Modules per side.
        modules: u32,
    },
    /// Its finder patterns were found, but it did not decode.
    Found {
        corners: [(f32, f32); 4],
        modules: u32,
    },
}

impl Detection {
    fn corners(&self) -> [(f32, f32); 4] {
        match self {
            Self::Decoded { corners, .. } | Self::Found { corners, .. } => *corners,
        }
    }

    fn modules(&self) -> u32 {
        match self {
            Self::Decoded { modules, .. } | Self::Found { modules, .. } => *modules,
        }
    }

    fn scaled(self, s: f32) -> Self {
        let scale = |c: [(f32, f32); 4]| c.map(|(x, y)| (x * s, y * s));
        match self {
            Self::Decoded {
                bytes,
                corners,
                version,
                modules,
            } => Self::Decoded {
                bytes,
                corners: scale(corners),
                version,
                modules,
            },
            Self::Found { corners, modules } => Self::Found {
                corners: scale(corners),
                modules,
            },
        }
    }
}

/// A QR decoder (plan 9): rqrr today; rxing could replace it under a later
/// feature.
pub(crate) trait Decoder: Send {
    fn scan(&mut self, grey: &GreyFrame) -> Vec<Detection>;
}

/// rqrr 0.11: quirc's detector and decoder in Rust.
#[derive(Default)]
pub(crate) struct Rqrr;

impl Decoder for Rqrr {
    fn scan(&mut self, grey: &GreyFrame) -> Vec<Detection> {
        if grey.width < 21 || grey.height < 21 || grey.data.len() < grey.width * grey.height {
            return Vec::new();
        }
        let mut prepared =
            rqrr::PreparedImage::prepare_from_greyscale(grey.width, grey.height, |x, y| {
                grey.at(x, y)
            });
        prepared
            .detect_grids()
            .into_iter()
            .map(|grid| {
                use rqrr::BitGrid;
                let corners = grid.bounds.map(|p| (p.x as f32, p.y as f32));
                let modules = grid.grid.size() as u32;
                let mut bytes = Vec::new();
                match grid.decode_to(&mut bytes) {
                    Ok(meta) => Detection::Decoded {
                        bytes,
                        corners,
                        version: meta.version.0.clamp(1, 40) as u8,
                        modules,
                    },
                    Err(_) => Detection::Found { corners, modules },
                }
            })
            .collect()
    }
}

/// A box in frame fractions: left, top, right, bottom.
pub(crate) type FracBox = (f32, f32, f32, f32);

/// The central 50 % of the frame.
pub(crate) const CENTRE: FracBox = (0.25, 0.25, 0.75, 0.75);

/// Where a code sits and how big its modules are (plan 6.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measure {
    /// Mean edge length over modules per side, in frame pixels.
    pub module_px: f32,
    /// The code's centre from the frame's centre, per axis, as fractions of
    /// half the frame's width and height (0 centred, 1 at the edge).
    pub offset: (f32, f32),
    /// Half the code's bounding box, per axis, in the same units.
    pub half: (f32, f32),
    /// Its bounding box in frame fractions.
    pub bbox: FracBox,
}

impl Measure {
    pub(crate) fn of(corners: &[(f32, f32); 4], modules: u32, width: usize, height: usize) -> Self {
        let edge =
            |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        let side = (0..4)
            .map(|i| edge(corners[i], corners[(i + 1) % 4]))
            .sum::<f32>()
            / 4.0;
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let xs = corners.map(|c| c.0);
        let ys = corners.map(|c| c.1);
        let min = |v: [f32; 4]| v.into_iter().fold(f32::INFINITY, f32::min);
        let max = |v: [f32; 4]| v.into_iter().fold(f32::NEG_INFINITY, f32::max);
        let (x0, x1, y0, y1) = (min(xs), max(xs), min(ys), max(ys));
        let (hw, hh) = (w / 2.0, h / 2.0);
        Measure {
            module_px: side / modules.max(1) as f32,
            offset: (
                ((x0 + x1) / 2.0 - hw).abs() / hw,
                ((y0 + y1) / 2.0 - hh).abs() / hh,
            ),
            half: ((x1 - x0) / 2.0 / hw, (y1 - y0) / 2.0 / hh),
            bbox: (
                (x0 / w).clamp(0.0, 1.0),
                (y0 / h).clamp(0.0, 1.0),
                (x1 / w).clamp(0.0, 1.0),
                (y1 / h).clamp(0.0, 1.0),
            ),
        }
    }
}

/// The luma of an area (plan 7.1), sampling every second pixel each way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Luma {
    pub mean: f32,
    /// The fraction of samples at or above `QR_SAT_LEVEL`.
    pub p_sat: f32,
}

impl Luma {
    pub(crate) fn of(grey: &GreyFrame, area: FracBox) -> Option<Luma> {
        let (w, h) = (grey.width as f32, grey.height as f32);
        let px = |f: f32, n: f32| ((f * n).round().max(0.0) as usize).min(n as usize);
        let (x0, x1) = (px(area.0, w), px(area.2, w));
        let (y0, y1) = (px(area.1, h), px(area.3, h));
        let (mut sum, mut sat, mut n) = (0u64, 0u64, 0u64);
        for y in (y0..y1).step_by(2) {
            for x in (x0..x1).step_by(2) {
                let v = grey.at(x, y);
                sum += v as u64;
                sat += (v >= QR_SAT_LEVEL) as u64;
                n += 1;
            }
        }
        (n > 0).then(|| Luma {
            mean: sum as f32 / n as f32,
            p_sat: sat as f32 / n as f32,
        })
    }

    /// Whether this looks like an overexposed screen.
    pub(crate) fn overexposed(&self) -> bool {
        self.p_sat >= QR_SAT_FRACTION || self.mean >= QR_SAT_MEAN
    }
}

/// What the controller acts on for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    /// A code was read (the largest, if several).
    Decoded(Measure),
    /// A code was found but not read (the largest).
    Found(Measure),
    /// No code at all.
    Missed,
}

/// A code that was read: its payload, its corners in frame pixels and its
/// version.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Read {
    pub bytes: Vec<u8>,
    pub corners: [(f32, f32); 4],
    pub version: u8,
}

/// One analysed frame: the codes read, and what auto zoom and exposure
/// look at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scan {
    pub width: usize,
    pub height: usize,
    /// The codes read.
    pub codes: Vec<Read>,
    pub target: Target,
    /// Over the target's bounding box, or the central 50 % when missed.
    pub luma: Option<Luma>,
}

/// Scan `grey` with `decoder`. When a code is found with modules of at
/// least `QR_RETRY_MODULE_PX` but nothing decodes, the frame is scanned
/// again box-filtered 2 × 2, once.
pub(crate) fn analyse(decoder: &mut dyn Decoder, grey: &GreyFrame) -> Scan {
    let measure = |d: &Detection| Measure::of(&d.corners(), d.modules(), grey.width, grey.height);
    let mut found = decoder.scan(grey);
    let decoded = |v: &[Detection]| v.iter().any(|d| matches!(d, Detection::Decoded { .. }));
    if !decoded(&found)
        && found
            .iter()
            .any(|d| measure(d).module_px >= QR_RETRY_MODULE_PX)
    {
        let half = grey.halved();
        let retry: Vec<Detection> = decoder
            .scan(&half)
            .into_iter()
            .map(|d| d.scaled(2.0))
            .collect();
        if decoded(&retry) {
            found = retry;
        }
    }
    let largest = |want_decoded: bool| {
        found
            .iter()
            .filter(|d| matches!(d, Detection::Decoded { .. }) == want_decoded)
            .map(&measure)
            .max_by(|a, b| {
                let area = |m: &Measure| (m.bbox.2 - m.bbox.0) * (m.bbox.3 - m.bbox.1);
                area(a).total_cmp(&area(b))
            })
    };
    let target = match (largest(true), largest(false)) {
        (Some(m), _) => Target::Decoded(m),
        (None, Some(m)) => Target::Found(m),
        (None, None) => Target::Missed,
    };
    let area = match target {
        Target::Decoded(m) | Target::Found(m) => m.bbox,
        Target::Missed => CENTRE,
    };
    let codes = found
        .into_iter()
        .filter_map(|d| match d {
            Detection::Decoded {
                bytes,
                corners,
                version,
                ..
            } => Some(Read {
                bytes,
                corners,
                version,
            }),
            Detection::Found { .. } => None,
        })
        .collect();
    Scan {
        width: grey.width,
        height: grey.height,
        codes,
        target,
        luma: Luma::of(grey, area),
    }
}

/// A point in frame fractions, turned clockwise by `quarter_turns`: where it
/// lands in the upright frame.
pub(crate) fn turn_point((x, y): (f32, f32), quarter_turns: u8) -> (f32, f32) {
    match quarter_turns % 4 {
        0 => (x, y),
        1 => (1.0 - y, x),
        2 => (1.0 - x, 1.0 - y),
        _ => (y, 1.0 - x),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A frame `size × size` (white) with `payload` rendered as a QR code of
    /// `module` px per module at `(left, top)` (the quiet zone included in
    /// the code's area, 4 modules).
    pub(crate) fn render(
        payload: &str,
        module: usize,
        frame: (usize, usize),
        at: (usize, usize),
    ) -> GreyFrame {
        let code = qrcode::QrCode::new(payload.as_bytes()).unwrap();
        let n = code.width();
        let colors = code.to_colors();
        let mut data = vec![255u8; frame.0 * frame.1];
        let quiet = 4;
        for my in 0..n {
            for mx in 0..n {
                if colors[my * n + mx] != qrcode::Color::Dark {
                    continue;
                }
                for dy in 0..module {
                    for dx in 0..module {
                        let x = at.0 + (mx + quiet) * module + dx;
                        let y = at.1 + (my + quiet) * module + dy;
                        if x < frame.0 && y < frame.1 {
                            data[y * frame.0 + x] = 0;
                        }
                    }
                }
            }
        }
        GreyFrame {
            width: frame.0,
            height: frame.1,
            data,
        }
    }

    /// The number of modules per side of `payload`'s code.
    pub(crate) fn modules_of(payload: &str) -> usize {
        qrcode::QrCode::new(payload.as_bytes()).unwrap().width()
    }

    #[test]
    fn decodes_at_several_module_sizes() {
        let payload = "https://example.com/pair?k=0123456789";
        let n = modules_of(payload);
        for module in [3usize, 4, 6, 10] {
            let side = (n + 8) * module;
            let grey = render(payload, module, (side + 40, side + 20), (20, 10));
            let scan = analyse(&mut Rqrr, &grey);
            assert_eq!(scan.codes.len(), 1, "module {module}");
            assert_eq!(scan.codes[0].bytes, payload.as_bytes());
            let Target::Decoded(m) = scan.target else {
                panic!("module {module}: {:?}", scan.target);
            };
            assert!(
                (m.module_px - module as f32).abs() < 0.6,
                "module {module}: {}",
                m.module_px
            );
        }
    }

    #[test]
    fn nothing_in_a_blank_frame() {
        let grey = GreyFrame {
            width: 64,
            height: 48,
            data: vec![128; 64 * 48],
        };
        let scan = analyse(&mut Rqrr, &grey);
        assert_eq!(scan.target, Target::Missed);
        assert!(scan.codes.is_empty());
        assert_eq!(scan.luma.unwrap().mean, 128.0);
    }

    #[test]
    fn measure_from_corners() {
        // A 100 px code of 25 modules, centred at (300, 120) in 400 × 200.
        let c = [(250.0, 70.0), (350.0, 70.0), (350.0, 170.0), (250.0, 170.0)];
        let m = Measure::of(&c, 25, 400, 200);
        assert_eq!(m.module_px, 4.0);
        assert_eq!(m.offset, (0.5, 0.2));
        assert_eq!(m.half, (0.25, 0.5));
        assert_eq!(m.bbox, (0.625, 0.35, 0.875, 0.85));
    }

    #[test]
    fn luma_and_the_screen_test() {
        let mut grey = GreyFrame {
            width: 100,
            height: 100,
            data: vec![100; 100 * 100],
        };
        let luma = Luma::of(&grey, (0.0, 0.0, 1.0, 1.0)).unwrap();
        assert_eq!((luma.mean, luma.p_sat), (100.0, 0.0));
        assert!(!luma.overexposed());
        // A tenth of the samples saturated: overexposed.
        for y in 0..10 {
            for x in 0..100 {
                grey.data[y * 100 + x] = 250;
            }
        }
        let luma = Luma::of(&grey, (0.0, 0.0, 1.0, 1.0)).unwrap();
        assert!((luma.p_sat - 0.1).abs() < 1e-6, "{}", luma.p_sat);
        assert!(luma.overexposed());
        // A bright mean without saturation: overexposed too.
        let bright = GreyFrame {
            width: 10,
            height: 10,
            data: vec![210; 100],
        };
        assert!(Luma::of(&bright, CENTRE).unwrap().overexposed());
        // An empty area has no statistics.
        assert_eq!(Luma::of(&bright, (0.5, 0.5, 0.5, 0.5)), None);
    }

    #[test]
    fn halving_averages_two_by_two() {
        let grey = GreyFrame {
            width: 4,
            height: 2,
            data: vec![0, 255, 10, 10, 255, 0, 10, 10],
        };
        assert_eq!(grey.halved().data, [128, 10]);
    }

    /// A screen's pixel grid at a code's scale: a dark dot at every odd
    /// pixel of the light area, as the gaps between a display's pixels look
    /// close up. The finder patterns still show on the even rows, but light
    /// modules sampled at their centres read dark.
    fn with_pixel_grid(mut grey: GreyFrame) -> GreyFrame {
        for y in (1..grey.height).step_by(2) {
            for x in (1..grey.width).step_by(2) {
                let i = y * grey.width + x;
                if grey.data[i] > 128 {
                    grey.data[i] = 0;
                }
            }
        }
        grey
    }

    #[test]
    fn the_two_by_two_retry_reads_through_a_pixel_grid() {
        let payload = "TV-PAIRING-0042";
        let module = 10;
        let side = (modules_of(payload) + 8) * module;
        let clean = render(payload, module, (side + 40, side + 40), (20, 20));
        let grey = with_pixel_grid(clean);
        // Without the retry the grid defeats the decoder.
        let direct = Rqrr.scan(&grey);
        assert!(
            !direct
                .iter()
                .any(|d| matches!(d, Detection::Decoded { .. })),
            "{direct:?}"
        );
        // With it, the halved frame reads, and corners are in full pixels.
        let scan = analyse(&mut Rqrr, &grey);
        assert_eq!(scan.codes.len(), 1, "{:?}", scan.target);
        assert_eq!(scan.codes[0].bytes, payload.as_bytes());
        let Target::Decoded(m) = scan.target else {
            panic!("{:?}", scan.target);
        };
        assert!((m.module_px - module as f32).abs() < 1.0, "{}", m.module_px);
    }

    #[test]
    fn points_turn_with_the_frame() {
        assert_eq!(turn_point((0.0, 0.0), 0), (0.0, 0.0));
        // A clockwise quarter turn takes the top-left to the top-right.
        assert_eq!(turn_point((0.0, 0.0), 1), (1.0, 0.0));
        assert_eq!(turn_point((0.25, 0.0), 1), (1.0, 0.25));
        assert_eq!(turn_point((0.25, 0.0), 2), (0.75, 1.0));
        assert_eq!(turn_point((0.25, 0.0), 3), (0.0, 0.75));
        assert_eq!(turn_point((0.25, 0.0), 4), (0.25, 0.0));
    }
}
