//! **Asking the user for a photo, a video or a file** (feature
//! `media-request`): the platform-free types. The calls are in
//! `android::media`.
//!
//! One request opens Android's own chooser (`ACTION_GET_CONTENT` in
//! `Intent.createChooser`: the gallery, Files, Drive...) with an optional
//! camera entry that writes a full-size photo or video into the app's cache,
//! or the system Photo Picker when the app prefers it
//! ([`Picker::PhotoPicker`]). The answer, [`MediaResponse`], arrives as
//! `MobileEvent::Media`: each chosen item's bytes or a file, with its URI,
//! MIME type, display name and size, under a per-item size cap.
//!
//! Picking needs no permission. The camera entry needs CAMERA only when the
//! app's manifest declares it (the in-process `camera` feature does); it is
//! then asked for when the camera entry is requested, never at launch
//! ([`CameraPermission`]).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// The default per-item cap, [`MediaRequest::max_bytes`]: 32 MiB.
pub const MEDIA_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// The ceiling on [`MediaRequest::max_items`].
pub const MEDIA_MAX_ITEMS: u16 = 100;

/// The stream copy buffer (`MediaRequestPlugin.kt`'s `COPY_CHUNK`).
pub const MEDIA_COPY_CHUNK: usize = 64 * 1024;

/// Files in `cache_dir()/libmobile-media/` older than this are deleted at
/// the next request (`MediaRequestPlugin.kt`'s `CACHE_MAX_AGE_MS`).
pub const MEDIA_CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The directory under the app's cache that results and camera output use.
pub const MEDIA_DIR: &str = "libmobile-media";

/// One `request_media` call. Opaque; ids count from 1 per process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaRequestId(pub(crate) u32);

impl MediaRequestId {
    /// The id as a number, for logs and for wrappers with their own id type.
    pub fn id(self) -> u32 {
        self.0
    }
}

/// What the user may choose.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaKind {
    #[default]
    Images,
    Videos,
    ImagesAndVideos,
    /// Any openable file.
    Any,
    /// These MIME types (`"application/pdf"`, `"image/png"`, ...). An empty
    /// list is [`Any`](MediaKind::Any).
    Mime(Vec<String>),
}

/// The chooser's camera entries (one per camera app on the device).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraEntry {
    #[default]
    None,
    Photo,
    Video,
    PhotoAndVideo,
}

impl CameraEntry {
    fn photo(self) -> bool {
        matches!(self, CameraEntry::Photo | CameraEntry::PhotoAndVideo)
    }

    fn video(self) -> bool {
        matches!(self, CameraEntry::Video | CameraEntry::PhotoAndVideo)
    }
}

/// What to do about CAMERA for a camera entry, when the manifest declares
/// it and it is not granted. Without CAMERA in the manifest the camera entry
/// needs no permission, and this is not used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraPermission {
    /// Ask for CAMERA; a refusal fails the request with
    /// [`MediaError::PermissionDenied`] and no chooser opens.
    #[default]
    Ask,
    /// Never ask; leave the camera entry out instead
    /// ([`MediaReply::camera_left_out`]).
    IfGranted,
}

/// Which system screen to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Picker {
    /// `ACTION_GET_CONTENT` in the system chooser, with camera entries.
    #[default]
    Chooser,
    /// The Photo Picker (`ACTION_PICK_IMAGES`) when it can serve the
    /// request: no camera entry, images and/or videos, and a device that
    /// has it (API 33, or API 30+ with SDK extension R >= 2). Otherwise the
    /// chooser; [`MediaReply::picker`] says which was shown.
    PhotoPicker,
}

/// How each item reaches the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delivery {
    /// The bytes in memory; the cache file is deleted.
    #[default]
    Bytes,
    /// A file under `cache_dir()/libmobile-media/`, the app's to move or
    /// delete. Files left there are deleted after [`MEDIA_CACHE_MAX_AGE`].
    File,
}

/// One request. Start from `MediaRequest::default()` and set the fields:
///
/// ```
/// use libmobile::media::{CameraEntry, MediaRequest};
/// let mut request = MediaRequest::default();
/// request.camera = CameraEntry::Photo;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaRequest {
    /// Default [`MediaKind::Images`].
    pub kind: MediaKind,
    /// Default [`CameraEntry::None`].
    pub camera: CameraEntry,
    /// Default [`CameraPermission::Ask`].
    pub camera_permission: CameraPermission,
    /// Default [`Picker::Chooser`].
    pub picker: Picker,
    /// 1 = single selection (the default). Clamped to
    /// `1..=MEDIA_MAX_ITEMS`.
    pub max_items: u16,
    /// Per item. Default [`MEDIA_MAX_BYTES`]; 0 is taken as 1.
    pub max_bytes: u64,
    /// Default [`Delivery::Bytes`].
    pub delivery: Delivery,
    /// The chooser's title; `None` (the default) for the system's.
    pub title: Option<String>,
}

impl Default for MediaRequest {
    fn default() -> Self {
        MediaRequest {
            kind: MediaKind::Images,
            camera: CameraEntry::None,
            camera_permission: CameraPermission::Ask,
            picker: Picker::Chooser,
            max_items: 1,
            max_bytes: MEDIA_MAX_BYTES,
            delivery: Delivery::Bytes,
            title: None,
        }
    }
}

/// Where an item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSource {
    /// Chosen in the chooser or the Photo Picker.
    Picked,
    /// Taken through the chooser's camera entry.
    Camera,
}

/// An item's content.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaData {
    Bytes(Vec<u8>),
    File(PathBuf),
    /// Over `max_bytes`; `size` when the provider reported it.
    TooLarge {
        size: Option<u64>,
    },
    /// The item could not be read (the exception's class, for logs).
    Unreadable(String),
}

/// One chosen item.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaItem {
    pub source: MediaSource,
    /// The `content://` URI as returned (ours for a camera item). Read
    /// access to it ends when the activity finishes; use `data`.
    pub uri: String,
    pub mime: Option<String>,
    pub display_name: Option<String>,
    pub size: Option<u64>,
    /// JPEG EXIF orientation 1..=8, 0 when absent or not a JPEG. The pixels
    /// are not rotated.
    pub exif_orientation: u8,
    pub data: MediaData,
}

/// A request's items. Never empty: no item is
/// [`MediaError::Failed`]`("no_item")`, and per-item failures
/// ([`MediaData::TooLarge`], [`MediaData::Unreadable`]) are items.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaReply {
    pub items: Vec<MediaItem>,
    /// Which screen was shown (`PhotoPicker` may fall back to `Chooser`).
    pub picker: Picker,
    /// A camera entry was asked for but not shown (no camera app, or
    /// `IfGranted` without CAMERA).
    pub camera_left_out: bool,
    /// The app returned more than `max_items`; the rest were dropped.
    pub truncated: bool,
}

/// Why a request has no items.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaError {
    /// The user backed out of the chooser, the picker or the camera.
    Cancelled,
    /// Another request is in flight.
    Busy,
    /// Nothing on the device handles the request.
    NoApp,
    /// The app was not visible, and Android does not let it start one.
    NotVisible,
    /// CAMERA refused for the camera entry. `settings_only`: asking again
    /// shows no dialog; send the user to `android::open_app_settings`.
    PermissionDenied { settings_only: bool },
    /// Anything else, for logs (`"no_item"`, `"lost"`, `"platform: ..."`).
    Failed(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Cancelled => f.write_str("cancelled"),
            MediaError::Busy => f.write_str("another media request is in flight"),
            MediaError::NoApp => f.write_str("no app handles the media request"),
            MediaError::NotVisible => f.write_str("the app is not visible"),
            MediaError::PermissionDenied { settings_only } => write!(
                f,
                "CAMERA denied{}",
                if *settings_only {
                    " (only Settings can grant it)"
                } else {
                    ""
                }
            ),
            MediaError::Failed(message) => write!(f, "media request failed: {message}"),
        }
    }
}

impl std::error::Error for MediaError {}

impl MediaError {
    /// From the plugin's `"<kind>: <detail>"` (`MediaRequestPlugin.kt`):
    /// `cancelled`, `busy`, `no-app`, `not-visible`; anything else is
    /// `Failed` with the whole message.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_message(message: &str) -> MediaError {
        match message.split(':').next().unwrap_or("").trim() {
            "cancelled" => MediaError::Cancelled,
            "busy" => MediaError::Busy,
            "no-app" => MediaError::NoApp,
            "not-visible" => MediaError::NotVisible,
            _ => MediaError::Failed(message.to_owned()),
        }
    }
}

/// The answer to one `request_media` call, as `MobileEvent::Media`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MediaResponse {
    /// The request this answers; `None` when it was made before the
    /// activity or the process was recreated.
    pub request: Option<MediaRequestId>,
    pub result: Result<MediaReply, MediaError>,
}

// ── Crate-private: the decisions, flattened for the plugin and back ─────────

/// The request as the plugin takes it (plan 4.2): every decision made here,
/// so the Kotlin side only applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) struct Spec {
    /// `ACTION_PICK_IMAGES` instead of the chooser.
    pub photo_picker: bool,
    /// `GET_CONTENT`'s type, or the Photo Picker's (`None`: its default,
    /// images only).
    pub mime_type: Option<String>,
    /// `GET_CONTENT`'s `EXTRA_MIME_TYPES`; empty for none.
    pub extra_mimes: Vec<String>,
    pub max_items: u16,
    pub max_bytes: u64,
    pub delivery: Delivery,
    pub camera: CameraEntry,
    pub camera_left_out: bool,
    pub title: Option<String>,
}

/// `max_items` clamped to `1..=MEDIA_MAX_ITEMS`.
pub(crate) fn clamp_items(max_items: u16) -> u16 {
    max_items.clamp(1, MEDIA_MAX_ITEMS)
}

/// The kind as `GET_CONTENT`'s type and `EXTRA_MIME_TYPES` (plan 4.2).
pub(crate) fn content_types(kind: &MediaKind) -> (String, Vec<String>) {
    let all = || ("*/*".to_owned(), Vec::new());
    match kind {
        MediaKind::Images => ("image/*".into(), Vec::new()),
        MediaKind::Videos => ("video/*".into(), Vec::new()),
        MediaKind::ImagesAndVideos => ("*/*".into(), vec!["image/*".into(), "video/*".into()]),
        MediaKind::Any => all(),
        MediaKind::Mime(list) => match list.as_slice() {
            [] => all(),
            [one] => (one.clone(), Vec::new()),
            many => ("*/*".into(), many.to_vec()),
        },
    }
}

/// The kind as the Photo Picker's type: `Some(None)` for its default
/// (images), `Some(Some(t))` for `t`, `None` when it cannot serve the kind.
pub(crate) fn picker_type(kind: &MediaKind) -> Option<Option<String>> {
    match kind {
        MediaKind::Images => Some(None),
        MediaKind::Videos => Some(Some("video/*".into())),
        MediaKind::ImagesAndVideos => Some(Some("*/*".into())),
        MediaKind::Any | MediaKind::Mime(_) => None,
    }
}

/// Whether the Photo Picker is shown (plan 2): asked for, no camera entry,
/// a kind it serves, and on the device.
pub(crate) fn use_photo_picker(
    picker: Picker,
    kind: &MediaKind,
    camera: CameraEntry,
    available: bool,
) -> bool {
    picker == Picker::PhotoPicker
        && camera == CameraEntry::None
        && picker_type(kind).is_some()
        && available
}

/// Flatten `request` for the plugin. `photo_picker_available` is asked
/// only when the Photo Picker is wanted; `camera_left_out` is set when the
/// gate left the camera entry out (`IfGranted`).
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn spec(
    request: &MediaRequest,
    photo_picker_available: impl FnOnce() -> bool,
    camera_left_out: bool,
) -> Spec {
    let camera = if camera_left_out {
        CameraEntry::None
    } else {
        request.camera
    };
    let photo_picker = request.picker == Picker::PhotoPicker
        && use_photo_picker(
            request.picker,
            &request.kind,
            camera,
            photo_picker_available(),
        );
    let (mime_type, extra_mimes) = if photo_picker {
        (picker_type(&request.kind).flatten(), Vec::new())
    } else {
        let (t, extra) = content_types(&request.kind);
        (Some(t), extra)
    };
    Spec {
        photo_picker,
        mime_type,
        extra_mimes,
        max_items: clamp_items(request.max_items),
        max_bytes: request.max_bytes.max(1),
        delivery: request.delivery,
        camera,
        camera_left_out,
        title: request.title.clone(),
    }
}

impl Spec {
    /// The camera entry as the plugin's int: bit 0 photo, bit 1 video.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn camera_bits(&self) -> i32 {
        (self.camera.photo() as i32) | ((self.camera.video() as i32) << 1)
    }
}

/// CAMERA's answer for a media request's camera entry (plan 4.3): go ahead,
/// or fail with `PermissionDenied`; `rationale` is
/// `should_show_rationale(CAMERA)` read after the denial.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn camera_gate(granted: bool, rationale: bool) -> Result<(), MediaError> {
    if granted {
        Ok(())
    } else {
        Err(MediaError::PermissionDenied {
            settings_only: !rationale,
        })
    }
}

/// `MediaFileProvider.kt`'s name check, kept in step with it: 1 to 96 of
/// `A-Z a-z 0-9 . _ -`, and not `.` or `..`.
#[cfg_attr(not(test), allow(dead_code))] // The Kotlin provider's check; mirrored for the tests.
pub(crate) fn file_name_ok(name: &str) -> bool {
    (1..=96).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// One item as the plugin reports it, before the file is read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
pub(crate) struct RawItem {
    pub camera: bool,
    pub uri: String,
    pub mime: Option<String>,
    pub name: Option<String>,
    /// -1 when unknown.
    pub size: i64,
    pub path: Option<String>,
    /// `None`, `"too-large"` or `"unreadable: <class>"`.
    pub error: Option<String>,
    pub exif: i32,
}

/// The plugin's answer (`nativeMediaResult`), before the files are read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
pub(crate) struct RawResult {
    pub error: Option<String>,
    pub items: Vec<RawItem>,
    /// 0 chooser, 1 Photo Picker.
    pub picker: i32,
    /// 0 bytes, 1 file.
    pub delivery: i32,
    pub camera_left_out: bool,
    pub truncated: bool,
}

/// Build the reply from the plugin's answer. For `Bytes`, `read` reads (and
/// deletes) each item's file; a read failure makes that item `Unreadable`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
pub(crate) fn reply_from_raw(
    raw: RawResult,
    mut read: impl FnMut(&str) -> std::io::Result<Vec<u8>>,
) -> Result<MediaReply, MediaError> {
    if let Some(error) = raw.error {
        return Err(MediaError::from_message(&error));
    }
    if raw.items.is_empty() {
        return Err(MediaError::Failed("no_item".into()));
    }
    let bytes = raw.delivery == 0;
    let items = raw
        .items
        .into_iter()
        .map(|item| {
            let size = u64::try_from(item.size).ok();
            let data = match (item.error.as_deref(), item.path) {
                (Some(error), _) if error.starts_with("too-large") => MediaData::TooLarge { size },
                (Some(error), _) => MediaData::Unreadable(
                    error
                        .strip_prefix("unreadable:")
                        .unwrap_or(error)
                        .trim()
                        .to_owned(),
                ),
                (None, None) => MediaData::Unreadable("no file".into()),
                (None, Some(path)) if bytes => match read(&path) {
                    Ok(data) => MediaData::Bytes(data),
                    Err(err) => MediaData::Unreadable(format!("{:?}", err.kind())),
                },
                (None, Some(path)) => MediaData::File(PathBuf::from(path)),
            };
            MediaItem {
                source: if item.camera {
                    MediaSource::Camera
                } else {
                    MediaSource::Picked
                },
                uri: item.uri,
                mime: item.mime,
                display_name: item.name,
                size,
                exif_orientation: u8::try_from(item.exif)
                    .ok()
                    .filter(|o| *o <= 8)
                    .unwrap_or(0),
                data,
            }
        })
        .collect();
    Ok(MediaReply {
        items,
        picker: if raw.picker == 1 {
            Picker::PhotoPicker
        } else {
            Picker::Chooser
        },
        camera_left_out: raw.camera_left_out,
        truncated: raw.truncated,
    })
}

/// The pending record (`cache_dir()/libmobile-media/pending`, plan 6.3), as
/// `MediaRequestPlugin.kt` writes and reads it: `key=value` lines.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // The Kotlin parser's rules; mirrored for the tests.
pub(crate) struct Pending {
    pub id: u32,
    pub max_items: u16,
    pub max_bytes: u64,
    pub delivery: i32,
    pub picker: i32,
    pub camera_left_out: bool,
    pub photo: Option<String>,
    pub video: Option<String>,
    pub time: u64,
}

/// `MediaRequestPlugin.parsePending`, kept in step with it: `id` (> 0),
/// `max_items` (1..=MEDIA_MAX_ITEMS), `max_bytes` (> 0), `delivery` and
/// `picker` (0 or 1) are required; `camera_left_out` (0 or 1, default 0),
/// `photo` and `video` (file names passing [`file_name_ok`]) and `time` are
/// optional; unknown keys are ignored. Anything else: `None`.
#[cfg_attr(not(test), allow(dead_code))] // The Kotlin parser's rules; mirrored for the tests.
pub(crate) fn parse_pending(text: &str) -> Option<Pending> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once('=')?;
        map.insert(key.trim(), value.trim());
    }
    let int = |key: &str| -> Option<i64> { map.get(key)?.parse().ok() };
    let flag = |key: &str| -> Option<i32> {
        match map.get(key) {
            None => Some(0),
            Some(&"0") => Some(0),
            Some(&"1") => Some(1),
            Some(_) => None,
        }
    };
    let name = |key: &str| -> Option<Option<String>> {
        match map.get(key) {
            None => Some(None),
            Some(n) if file_name_ok(n) => Some(Some((*n).to_owned())),
            Some(_) => None,
        }
    };
    let id = u32::try_from(int("id")?).ok().filter(|id| *id > 0)?;
    let max_items = u16::try_from(int("max_items")?)
        .ok()
        .filter(|n| (1..=MEDIA_MAX_ITEMS).contains(n))?;
    let max_bytes = u64::try_from(int("max_bytes")?).ok().filter(|n| *n > 0)?;
    let delivery = map.get("delivery").and(flag("delivery"))?;
    let picker = map.get("picker").and(flag("picker"))?;
    Some(Pending {
        id,
        max_items,
        max_bytes,
        delivery,
        picker,
        camera_left_out: flag("camera_left_out")? == 1,
        photo: name("photo")?,
        video: name("video")?,
        time: match map.get("time") {
            None => 0,
            Some(t) => t.parse().ok()?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_plans() {
        let r = MediaRequest::default();
        assert_eq!(r.kind, MediaKind::Images);
        assert_eq!(r.camera, CameraEntry::None);
        assert_eq!(r.camera_permission, CameraPermission::Ask);
        assert_eq!(r.picker, Picker::Chooser);
        assert_eq!(r.max_items, 1);
        assert_eq!(r.max_bytes, MEDIA_MAX_BYTES);
        assert_eq!(r.delivery, Delivery::Bytes);
        assert_eq!(r.title, None);
        assert_eq!(MEDIA_MAX_BYTES, 33_554_432);
    }

    #[test]
    fn mime_table_is_plan_4_2() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            content_types(&MediaKind::Images),
            ("image/*".into(), s(&[]))
        );
        assert_eq!(
            content_types(&MediaKind::Videos),
            ("video/*".into(), s(&[]))
        );
        assert_eq!(
            content_types(&MediaKind::ImagesAndVideos),
            ("*/*".into(), s(&["image/*", "video/*"]))
        );
        assert_eq!(content_types(&MediaKind::Any), ("*/*".into(), s(&[])));
        assert_eq!(
            content_types(&MediaKind::Mime(s(&["application/pdf"]))),
            ("application/pdf".into(), s(&[]))
        );
        assert_eq!(
            content_types(&MediaKind::Mime(s(&["image/png", "application/pdf"]))),
            ("*/*".into(), s(&["image/png", "application/pdf"]))
        );
        assert_eq!(
            content_types(&MediaKind::Mime(vec![])),
            ("*/*".into(), s(&[]))
        );

        assert_eq!(picker_type(&MediaKind::Images), Some(None));
        assert_eq!(
            picker_type(&MediaKind::Videos),
            Some(Some("video/*".into()))
        );
        assert_eq!(
            picker_type(&MediaKind::ImagesAndVideos),
            Some(Some("*/*".into()))
        );
        assert_eq!(picker_type(&MediaKind::Any), None);
        assert_eq!(picker_type(&MediaKind::Mime(s(&["image/png"]))), None);
    }

    #[test]
    fn photo_picker_fallback_for_every_combination() {
        let kinds = [
            MediaKind::Images,
            MediaKind::Videos,
            MediaKind::ImagesAndVideos,
            MediaKind::Any,
            MediaKind::Mime(vec!["image/png".into()]),
        ];
        let cameras = [
            CameraEntry::None,
            CameraEntry::Photo,
            CameraEntry::Video,
            CameraEntry::PhotoAndVideo,
        ];
        for picker in [Picker::Chooser, Picker::PhotoPicker] {
            for kind in &kinds {
                for camera in cameras {
                    for available in [false, true] {
                        let expected = picker == Picker::PhotoPicker
                            && camera == CameraEntry::None
                            && matches!(
                                kind,
                                MediaKind::Images | MediaKind::Videos | MediaKind::ImagesAndVideos
                            )
                            && available;
                        assert_eq!(
                            use_photo_picker(picker, kind, camera, available),
                            expected,
                            "{picker:?} {kind:?} {camera:?} {available}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn spec_flattens_and_clamps() {
        let mut r = MediaRequest {
            max_items: 0,
            max_bytes: 0,
            ..MediaRequest::default()
        };
        let s = spec(&r, || panic!("not asked for the chooser"), false);
        assert!(!s.photo_picker);
        assert_eq!(s.mime_type.as_deref(), Some("image/*"));
        assert_eq!((s.max_items, s.max_bytes), (1, 1));
        assert_eq!(s.camera_bits(), 0);

        r.max_items = 500;
        r.picker = Picker::PhotoPicker;
        r.kind = MediaKind::Videos;
        let s = spec(&r, || true, false);
        assert!(s.photo_picker);
        assert_eq!(s.mime_type.as_deref(), Some("video/*"));
        assert_eq!(s.max_items, MEDIA_MAX_ITEMS);
        let s = spec(&r, || false, false);
        assert!(!s.photo_picker, "falls back to the chooser");
        assert_eq!(s.mime_type.as_deref(), Some("video/*"));

        r.kind = MediaKind::Images;
        let s = spec(&r, || true, false);
        assert_eq!(s.mime_type, None, "the Photo Picker's own default");

        r.camera = CameraEntry::PhotoAndVideo;
        let s = spec(&r, || true, false);
        assert!(!s.photo_picker, "a camera entry needs the chooser");
        assert_eq!(s.camera_bits(), 3);
        // The gate left the camera out (IfGranted): the Photo Picker may serve.
        let s = spec(&r, || true, true);
        assert!(s.photo_picker && s.camera_left_out);
        assert_eq!(s.camera_bits(), 0);
        r.picker = Picker::Chooser;
        r.camera = CameraEntry::Video;
        assert_eq!(spec(&r, || false, false).camera_bits(), 2);
        r.camera = CameraEntry::Photo;
        assert_eq!(spec(&r, || false, false).camera_bits(), 1);
    }

    #[test]
    fn camera_gate_grant_and_denial() {
        assert_eq!(camera_gate(true, false), Ok(()));
        assert_eq!(
            camera_gate(false, true),
            Err(MediaError::PermissionDenied {
                settings_only: false
            })
        );
        assert_eq!(
            camera_gate(false, false),
            Err(MediaError::PermissionDenied {
                settings_only: true
            })
        );
    }

    #[test]
    fn provider_names() {
        for ok in ["cam-7.jpg", "cam-7.mp4", "3-0.bin", "a", &"x".repeat(96)] {
            assert!(file_name_ok(ok), "{ok}");
        }
        for bad in [
            "",
            "../x",
            "a/b",
            "..",
            ".",
            "a b",
            "é",
            &"x".repeat(97),
            "a\\b",
        ] {
            assert!(!file_name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn every_error_string() {
        for (message, error) in [
            ("cancelled", MediaError::Cancelled),
            ("busy", MediaError::Busy),
            ("no-app", MediaError::NoApp),
            ("not-visible: importance 400", MediaError::NotVisible),
            ("no_item", MediaError::Failed("no_item".into())),
            ("lost", MediaError::Failed("lost".into())),
            (
                "platform: java.lang.SecurityException",
                MediaError::Failed("platform: java.lang.SecurityException".into()),
            ),
        ] {
            assert_eq!(MediaError::from_message(message), error, "{message}");
            let raw = RawResult {
                error: Some(message.into()),
                ..RawResult::default()
            };
            assert_eq!(reply_from_raw(raw, |_| unreachable!()), Err(error));
        }
        assert_eq!(
            reply_from_raw(RawResult::default(), |_| unreachable!()),
            Err(MediaError::Failed("no_item".into()))
        );
    }

    fn item(path: &str) -> RawItem {
        RawItem {
            uri: format!("content://p/{path}"),
            mime: Some("image/jpeg".into()),
            name: Some("IMG.jpg".into()),
            size: 3,
            path: Some(path.into()),
            exif: 6,
            ..RawItem::default()
        }
    }

    #[test]
    fn items_bytes_files_and_failures() {
        let raw = RawResult {
            items: vec![
                item("/c/1-0.jpg"),
                RawItem {
                    camera: true,
                    exif: 99,
                    ..item("/c/cam-1.jpg")
                },
                RawItem {
                    error: Some("too-large".into()),
                    path: None,
                    size: -1,
                    ..item("x")
                },
                RawItem {
                    error: Some("too-large".into()),
                    path: None,
                    size: 50_000_000,
                    ..item("y")
                },
                RawItem {
                    error: Some("unreadable: java.io.FileNotFoundException".into()),
                    path: None,
                    ..item("z")
                },
                item("/c/missing"),
            ],
            picker: 0,
            delivery: 0,
            camera_left_out: true,
            truncated: true,
            ..RawResult::default()
        };
        let mut read = Vec::new();
        let reply = reply_from_raw(raw.clone(), |path| {
            read.push(path.to_owned());
            if path == "/c/missing" {
                Err(std::io::ErrorKind::NotFound.into())
            } else {
                Ok(vec![1, 2, 3])
            }
        })
        .unwrap();
        assert_eq!(read, ["/c/1-0.jpg", "/c/cam-1.jpg", "/c/missing"]);
        assert_eq!(reply.picker, Picker::Chooser);
        assert!(reply.camera_left_out && reply.truncated);
        let data: Vec<_> = reply.items.iter().map(|i| i.data.clone()).collect();
        assert_eq!(
            data,
            [
                MediaData::Bytes(vec![1, 2, 3]),
                MediaData::Bytes(vec![1, 2, 3]),
                MediaData::TooLarge { size: None },
                MediaData::TooLarge {
                    size: Some(50_000_000)
                },
                MediaData::Unreadable("java.io.FileNotFoundException".into()),
                MediaData::Unreadable("NotFound".into()),
            ]
        );
        let first = &reply.items[0];
        assert_eq!(first.source, MediaSource::Picked);
        assert_eq!(first.uri, "content://p//c/1-0.jpg");
        assert_eq!(first.mime.as_deref(), Some("image/jpeg"));
        assert_eq!(first.display_name.as_deref(), Some("IMG.jpg"));
        assert_eq!(first.size, Some(3));
        assert_eq!(first.exif_orientation, 6);
        assert_eq!(reply.items[1].source, MediaSource::Camera);
        assert_eq!(reply.items[1].exif_orientation, 0, "out of range");

        let files = reply_from_raw(
            RawResult {
                delivery: 1,
                picker: 1,
                items: vec![item("/c/1-0.jpg")],
                ..RawResult::default()
            },
            |_| unreachable!("File does not read"),
        )
        .unwrap();
        assert_eq!(files.picker, Picker::PhotoPicker);
        assert_eq!(
            files.items[0].data,
            MediaData::File(PathBuf::from("/c/1-0.jpg"))
        );
    }

    #[test]
    fn pending_records() {
        let good = "id=7\nmax_items=5\nmax_bytes=1048576\ndelivery=1\npicker=0\n\
                    camera_left_out=0\nphoto=cam-7.jpg\nvideo=cam-7.mp4\ntime=1730000000000\n";
        assert_eq!(
            parse_pending(good),
            Some(Pending {
                id: 7,
                max_items: 5,
                max_bytes: 1_048_576,
                delivery: 1,
                picker: 0,
                camera_left_out: false,
                photo: Some("cam-7.jpg".into()),
                video: Some("cam-7.mp4".into()),
                time: 1_730_000_000_000,
            })
        );
        let minimal = "id=1\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=1\nextra=ignored";
        let p = parse_pending(minimal).unwrap();
        assert_eq!((p.photo, p.video, p.time, p.picker), (None, None, 0, 1));

        for bad in [
            "",
            "max_items=1\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=0\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=-3\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=x\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=1\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=1\nmax_items=101\nmax_bytes=1\ndelivery=0\npicker=0",
            "id=1\nmax_items=1\nmax_bytes=0\ndelivery=0\npicker=0",
            "id=1\nmax_items=1\nmax_bytes=1\npicker=0",
            "id=1\nmax_items=1\nmax_bytes=1\ndelivery=2\npicker=0",
            "id=1\nmax_items=1\nmax_bytes=1\ndelivery=0",
            "id=1\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0\nphoto=../x",
            "id=1\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0\ncamera_left_out=yes",
            "id=1\nmax_items=1\nmax_bytes=1\ndelivery=0\npicker=0\ngarbage",
        ] {
            assert_eq!(parse_pending(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn errors_display() {
        assert_eq!(
            MediaError::PermissionDenied {
                settings_only: true
            }
            .to_string(),
            "CAMERA denied (only Settings can grant it)"
        );
        assert_eq!(
            MediaError::Busy.to_string(),
            "another media request is in flight"
        );
    }
}
