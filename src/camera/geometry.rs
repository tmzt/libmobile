//! Camera geometry and choices made from the camera's characteristics:
//! quarter turns for upright frames, the zoom control on API 30+ (zoom
//! ratio) and API 29 (crop region), regions in sensor coordinates (plan
//! 6.4), stream sizes, and whether 3A has settled.

#![cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android camera thread.

use super::Facing;

/// Clockwise quarter turns that make a frame upright: `sensor` is
/// `ACAMERA_SENSOR_ORIENTATION`, `display` the display's rotation, both in
/// degrees. Back: `(s − d + 360) % 360`; front: `(s + d) % 360`; over 90.
pub(crate) fn quarter_turns(sensor: i32, display: i32, facing: Facing) -> u8 {
    let s = sensor.rem_euclid(360);
    let d = display.rem_euclid(360);
    let degrees = match facing {
        Facing::Back => (s - d + 360) % 360,
        Facing::Front => (s + d) % 360,
    };
    ((degrees + 45) / 90 % 4) as u8
}

/// A rectangle in sensor pixels: left, top, width, height (the layout of
/// `ACAMERA_SENSOR_INFO_ACTIVE_ARRAY_SIZE` and `ACAMERA_SCALER_CROP_REGION`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// How zoom is applied on this camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ZoomControl {
    /// API 30+: `ACAMERA_CONTROL_ZOOM_RATIO`, continuous, `min..=max` with
    /// `min` raised to 1.0 (a logical multi-camera never switches to the
    /// ultra-wide).
    Ratio { max: f32, active: Rect },
    /// API 29, or no ratio range: a centred `ACAMERA_SCALER_CROP_REGION`, up
    /// to `ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM`.
    Crop { max: f32, active: Rect },
}

impl ZoomControl {
    /// From the characteristics: the API level, `ACAMERA_CONTROL_ZOOM_RATIO_RANGE`,
    /// `ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM` and the active array.
    pub(crate) fn new(
        api: i32,
        ratio_range: Option<(f32, f32)>,
        max_digital: Option<f32>,
        active: Rect,
    ) -> Self {
        let sane = |x: f32| x.is_finite() && x >= 1.0;
        match ratio_range {
            Some((_, max)) if api >= 30 && sane(max) => ZoomControl::Ratio { max, active },
            _ => ZoomControl::Crop {
                max: max_digital.filter(|m| sane(*m)).unwrap_or(1.0),
                active,
            },
        }
    }

    /// `(1.0, max)`.
    pub(crate) fn range(&self) -> (f32, f32) {
        match *self {
            ZoomControl::Ratio { max, .. } | ZoomControl::Crop { max, .. } => (1.0, max),
        }
    }

    /// `ratio` within the range; a non-finite ratio is 1.0.
    pub(crate) fn clamp(&self, ratio: f32) -> f32 {
        let (min, max) = self.range();
        if ratio.is_finite() {
            ratio.clamp(min, max)
        } else {
            min
        }
    }

    #[allow(dead_code)] // For regions (`region`), which QR scanning uses.
    pub(crate) fn active(&self) -> Rect {
        match *self {
            ZoomControl::Ratio { active, .. } | ZoomControl::Crop { active, .. } => active,
        }
    }

    /// The field of view `ratio` shows, in sensor pixels: on the crop path
    /// the crop region; on the ratio path the whole active array (regions
    /// are given in the post-zoom field there).
    #[allow(dead_code)] // For regions (`region`), which QR scanning uses.
    pub(crate) fn field(&self, ratio: f32) -> Rect {
        match *self {
            ZoomControl::Ratio { active, .. } => Rect {
                x: 0,
                y: 0,
                ..active
            },
            ZoomControl::Crop { active, .. } => crop_region(active, self.clamp(ratio)),
        }
    }
}

/// The active array divided by `ratio`, centred (coordinates relative to the
/// array, as the crop region is).
pub(crate) fn crop_region(active: Rect, ratio: f32) -> Rect {
    let ratio = if ratio.is_finite() && ratio >= 1.0 {
        ratio
    } else {
        1.0
    };
    let w = ((active.w as f32 / ratio).round() as i32).clamp(1, active.w.max(1));
    let h = ((active.h as f32 / ratio).round() as i32).clamp(1, active.h.max(1));
    Rect {
        x: (active.w - w) / 2,
        y: (active.h - h) / 2,
        w,
        h,
    }
}

/// A point in an analysis frame (fractions 0..=1) in the camera's field of
/// view (fractions), plan 6.4. The frame shows the field cropped to the
/// stream's aspect: `stream` and `field` are width / height.
#[allow(dead_code)] // For regions (`region`), which QR scanning uses.
pub(crate) fn frame_to_field(x: f32, y: f32, stream: f32, field: f32) -> (f32, f32) {
    if stream >= field {
        // The stream is wider: the field's top and bottom are cut.
        (x, 0.5 + (y - 0.5) * field / stream)
    } else {
        (0.5 + (x - 0.5) * stream / field, y)
    }
}

/// A box in an analysis frame (fractions: left, top, right, bottom) as a
/// metering region (`xmin, ymin, xmax, ymax, weight`) for
/// `ACAMERA_CONTROL_AF_REGIONS` / `AE_REGIONS`, weight 1000, clamped to the
/// active array. `stream_aspect` is the analysis stream's width / height.
#[allow(dead_code)] // AF and AE regions for QR scanning (feature `camera-qr`).
pub(crate) fn region(
    zoom: &ZoomControl,
    ratio: f32,
    frame_box: (f32, f32, f32, f32),
    stream_aspect: f32,
) -> [i32; 5] {
    let field = zoom.field(ratio);
    let field_aspect = field.w as f32 / field.h.max(1) as f32;
    let (x0, y0) = frame_to_field(frame_box.0, frame_box.1, stream_aspect, field_aspect);
    let (x1, y1) = frame_to_field(frame_box.2, frame_box.3, stream_aspect, field_aspect);
    let active = zoom.active();
    let fx = |f: f32| (field.x as f32 + f * field.w as f32).round() as i32;
    let fy = |f: f32| (field.y as f32 + f * field.h as f32).round() as i32;
    let cx = |v: i32| v.clamp(0, (active.w - 1).max(0));
    let cy = |v: i32| v.clamp(0, (active.h - 1).max(0));
    [
        cx(fx(x0.min(x1))),
        cy(fy(y0.min(y1))),
        cx(fx(x0.max(x1))),
        cy(fy(y0.max(y1))),
        1000,
    ]
}

/// One output stream configuration: format, width, height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamConfig {
    pub format: i32,
    pub width: u32,
    pub height: u32,
}

/// The output configurations in `ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS`
/// (quadruples `format, width, height, is_input`).
pub(crate) fn output_configs(raw: &[i32]) -> Vec<StreamConfig> {
    raw.chunks_exact(4)
        .filter(|c| c[3] == 0 && c[1] > 0 && c[2] > 0)
        .map(|c| StreamConfig {
            format: c[0],
            width: c[1] as u32,
            height: c[2] as u32,
        })
        .collect()
}

/// The sizes offered for `format`, deduplicated.
pub(crate) fn sizes_for(configs: &[StreamConfig], format: i32) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for c in configs.iter().filter(|c| c.format == format) {
        if !out.contains(&(c.width, c.height)) {
            out.push((c.width, c.height));
        }
    }
    out
}

/// Whether `size` fits in `bound` in either orientation.
pub(crate) fn fits(size: (u32, u32), bound: (u32, u32)) -> bool {
    let (long, short) = (size.0.max(size.1), size.0.min(size.1));
    let (b_long, b_short) = (bound.0.max(bound.1), bound.0.min(bound.1));
    long <= b_long && short <= b_short
}

fn area(size: (u32, u32)) -> u64 {
    size.0 as u64 * size.1 as u64
}

fn aspect(size: (u32, u32)) -> f32 {
    let (long, short) = (size.0.max(size.1), size.0.min(size.1).max(1));
    long as f32 / short as f32
}

/// The largest of `sizes` within `bound` (either orientation).
#[cfg_attr(not(feature = "camera-capture"), allow(dead_code))]
pub(crate) fn largest_within(sizes: &[(u32, u32)], bound: (u32, u32)) -> Option<(u32, u32)> {
    sizes
        .iter()
        .copied()
        .filter(|s| fits(*s, bound))
        .max_by_key(|s| area(*s))
}

/// The largest of `sizes`.
pub(crate) fn largest(sizes: &[(u32, u32)]) -> Option<(u32, u32)> {
    sizes.iter().copied().max_by_key(|s| area(*s))
}

/// The smallest of `sizes`.
pub(crate) fn smallest(sizes: &[(u32, u32)]) -> Option<(u32, u32)> {
    sizes.iter().copied().min_by_key(|s| area(*s))
}

/// The analysis stream for a session (plan 5.2): the largest size within
/// `PREVIEW_MAX` with the display's aspect (`display_aspect`, long / short)
/// if offered, else 4:3, else any; failing all, the smallest offered.
pub(crate) fn analysis_size(sizes: &[(u32, u32)], display_aspect: f32) -> Option<(u32, u32)> {
    let within: Vec<(u32, u32)> = sizes
        .iter()
        .copied()
        .filter(|s| fits(*s, super::PREVIEW_MAX))
        .collect();
    let with_aspect = |target: f32| {
        within
            .iter()
            .copied()
            .filter(|s| (aspect(*s) - target).abs() < 0.03)
            .max_by_key(|s| area(*s))
    };
    with_aspect(display_aspect)
        .or_else(|| with_aspect(4.0 / 3.0))
        .or_else(|| largest(&within))
        .or_else(|| smallest(sizes))
}

/// `ACAMERA_CONTROL_AE_STATE` values that count as settled for a still:
/// `CONVERGED`, `LOCKED`, `FLASH_REQUIRED`.
#[cfg_attr(not(feature = "camera-capture"), allow(dead_code))]
const AE_SETTLED: [u8; 3] = [2, 3, 4];
/// `ACAMERA_CONTROL_AF_STATE` values that count as settled: `PASSIVE_FOCUSED`,
/// `FOCUSED_LOCKED`, `NOT_FOCUSED_LOCKED`.
#[cfg_attr(not(feature = "camera-capture"), allow(dead_code))]
const AF_SETTLED: [u8; 3] = [2, 4, 5];

/// Whether 3A has settled enough for a still (plan 5.2). A state the result
/// does not carry counts as settled; with no autofocus, AF `INACTIVE` does.
#[cfg_attr(not(feature = "camera-capture"), allow(dead_code))]
pub(crate) fn three_a_settled(ae: Option<u8>, af: Option<u8>, has_af: bool) -> bool {
    let ae_ok = ae.is_none_or(|s| AE_SETTLED.contains(&s));
    let af_ok = af.is_none_or(|s| AF_SETTLED.contains(&s) || (!has_af && s == 0));
    ae_ok && af_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_for_every_sensor_display_and_facing() {
        for sensor in [0, 90, 180, 270] {
            for display in [0, 90, 180, 270] {
                let back = quarter_turns(sensor, display, Facing::Back);
                let front = quarter_turns(sensor, display, Facing::Front);
                assert_eq!(back as i32 * 90, (sensor - display + 360) % 360);
                assert_eq!(front as i32 * 90, (sensor + display) % 360);
            }
        }
        // A phone held upright: back sensor at 90, front at 270.
        assert_eq!(quarter_turns(90, 0, Facing::Back), 1);
        assert_eq!(quarter_turns(270, 0, Facing::Front), 3);
        // Turned left (display rotation 90): the back camera is level.
        assert_eq!(quarter_turns(90, 90, Facing::Back), 0);
        assert_eq!(quarter_turns(270, 90, Facing::Front), 0);
        // Out-of-range input still lands on a quarter.
        assert_eq!(quarter_turns(450, -90, Facing::Back), 2);
    }

    const ACTIVE: Rect = Rect {
        x: 8,
        y: 8,
        w: 4000,
        h: 3000,
    };

    #[test]
    fn zoom_ratio_path_on_api_30() {
        let z = ZoomControl::new(30, Some((0.6, 10.0)), Some(8.0), ACTIVE);
        assert_eq!(
            z,
            ZoomControl::Ratio {
                max: 10.0,
                active: ACTIVE
            }
        );
        assert_eq!(z.range(), (1.0, 10.0));
        assert_eq!(z.clamp(0.6), 1.0);
        assert_eq!(z.clamp(4.0), 4.0);
        assert_eq!(z.clamp(30.0), 10.0);
        assert_eq!(z.clamp(f32::NAN), 1.0);
    }

    #[test]
    fn crop_path_on_api_29_or_without_a_ratio_range() {
        let api29 = ZoomControl::new(29, Some((0.6, 10.0)), Some(8.0), ACTIVE);
        assert_eq!(
            api29,
            ZoomControl::Crop {
                max: 8.0,
                active: ACTIVE
            }
        );
        assert_eq!(api29.clamp(12.0), 8.0);
        assert_eq!(api29.clamp(0.5), 1.0);
        let no_range = ZoomControl::new(33, None, Some(4.0), ACTIVE);
        assert_eq!(no_range.range(), (1.0, 4.0));
        let nothing = ZoomControl::new(33, None, None, ACTIVE);
        assert_eq!(nothing.range(), (1.0, 1.0));
        assert_eq!(nothing.clamp(3.0), 1.0);
    }

    #[test]
    fn crop_region_is_centred() {
        assert_eq!(
            crop_region(ACTIVE, 1.0),
            Rect {
                x: 0,
                y: 0,
                w: 4000,
                h: 3000
            }
        );
        assert_eq!(
            crop_region(ACTIVE, 2.0),
            Rect {
                x: 1000,
                y: 750,
                w: 2000,
                h: 1500
            }
        );
        assert_eq!(
            crop_region(ACTIVE, 4.0),
            Rect {
                x: 1500,
                y: 1125,
                w: 1000,
                h: 750
            }
        );
        // Below 1 or not a number: the whole array.
        assert_eq!(crop_region(ACTIVE, 0.5), crop_region(ACTIVE, 1.0));
        assert_eq!(crop_region(ACTIVE, f32::NAN), crop_region(ACTIVE, 1.0));
    }

    #[test]
    fn frame_points_map_into_the_field() {
        // A 16:9 stream of a 4:3 field: the field's top and bottom are cut.
        let (s, f) = (16.0 / 9.0, 4.0 / 3.0);
        assert_eq!(frame_to_field(0.5, 0.5, s, f), (0.5, 0.5));
        let (x, y) = frame_to_field(0.0, 0.0, s, f);
        assert_eq!(x, 0.0);
        assert!((y - 0.125).abs() < 1e-6, "{y}");
        // A 4:3 stream of a 16:9 field: the sides are cut.
        let (x, y) = frame_to_field(1.0, 1.0, f, s);
        assert!((x - 0.875).abs() < 1e-6, "{x}");
        assert_eq!(y, 1.0);
    }

    #[test]
    fn regions_on_both_zoom_paths() {
        let full = (0.0, 0.0, 1.0, 1.0);
        let centre = (0.25, 0.25, 0.75, 0.75);
        let ratio = ZoomControl::new(30, Some((1.0, 10.0)), None, ACTIVE);
        // Ratio path: the post-zoom field is the whole array.
        assert_eq!(
            region(&ratio, 4.0, full, 4.0 / 3.0),
            [0, 0, 3999, 2999, 1000]
        );
        assert_eq!(
            region(&ratio, 4.0, centre, 4.0 / 3.0),
            [1000, 750, 3000, 2250, 1000]
        );
        // Crop path at 2×: inside the crop region.
        let crop = ZoomControl::new(29, None, Some(8.0), ACTIVE);
        assert_eq!(
            region(&crop, 2.0, full, 4.0 / 3.0),
            [1000, 750, 3000, 2250, 1000]
        );
        assert_eq!(
            region(&crop, 2.0, centre, 4.0 / 3.0),
            [1500, 1125, 2500, 1875, 1000]
        );
        // A 16:9 stream at 1×: an eighth cut from the top and bottom.
        assert_eq!(
            region(&crop, 1.0, full, 16.0 / 9.0),
            [0, 375, 3999, 2625, 1000]
        );
    }

    const YUV: i32 = 35;
    const JPEG: i32 = 256;

    #[test]
    fn stream_configs_keep_outputs() {
        let raw = [
            YUV, 1920, 1080, 0, //
            YUV, 1280, 720, 0, //
            YUV, 640, 480, 1, // an input: dropped
            JPEG, 4000, 3000, 0, //
            YUV, 1280, 720, 0, // repeated
        ];
        let configs = output_configs(&raw);
        assert_eq!(configs.len(), 4);
        assert_eq!(sizes_for(&configs, YUV), [(1920, 1080), (1280, 720)]);
        assert_eq!(sizes_for(&configs, JPEG), [(4000, 3000)]);
    }

    #[test]
    fn size_choices() {
        let sizes = [
            (4000, 3000),
            (1920, 1080),
            (1280, 960),
            (1280, 720),
            (960, 720),
            (640, 480),
            (320, 240),
        ];
        assert_eq!(largest(&sizes), Some((4000, 3000)));
        assert_eq!(smallest(&sizes), Some((320, 240)));
        assert_eq!(largest_within(&sizes, (1280, 720)), Some((1280, 720)));
        assert_eq!(largest_within(&sizes, (720, 1280)), Some((1280, 720)));
        assert_eq!(largest_within(&sizes, (100, 100)), None);
        // A 16:9 display: 1280 × 720.
        assert_eq!(analysis_size(&sizes, 16.0 / 9.0), Some((1280, 720)));
        // A 19.5:9 phone: not offered, so 4:3 within the bound.
        assert_eq!(analysis_size(&sizes, 19.5 / 9.0), Some((960, 720)));
        // Nothing within the bound: the smallest offered.
        assert_eq!(
            analysis_size(&[(4000, 3000), (1920, 1440)], 2.0),
            Some((1920, 1440))
        );
    }

    #[test]
    fn three_a() {
        assert!(three_a_settled(Some(2), Some(2), true));
        assert!(three_a_settled(Some(4), Some(5), true));
        assert!(!three_a_settled(Some(1), Some(2), true));
        assert!(!three_a_settled(Some(2), Some(1), true));
        // No AF: INACTIVE is settled; with AF it is not.
        assert!(three_a_settled(Some(2), Some(0), false));
        assert!(!three_a_settled(Some(2), Some(0), true));
        // States the result does not carry.
        assert!(three_a_settled(None, None, true));
    }
}
