//! Asking the user for media through Android's own chooser
//! (`media/MediaRequestPlugin.kt`, feature `media-request`).
//!
//! * [`request_media`] — Rust -> Kotlin: `ACTION_GET_CONTENT` in
//!   `Intent.createChooser`, with camera entries writing into the app's
//!   cache through `MediaFileProvider`, or the Photo Picker. Returns at once;
//!   the answer is `MobileEvent::Media`.
//! * [`photo_picker_available`] — whether the device has the Photo Picker.
//! * [`Java_dev_highbay_libmobile_media_MediaRequestPlugin_nativeMediaResult`]
//!   — Kotlin -> Rust, on the plugin's `libmobile-media` thread after the
//!   items are copied (or on the UI thread for an early error).
//!
//! The camera entry needs CAMERA only when the app's manifest declares it;
//! then it is asked for here, through the core permission gate, before the
//! chooser opens (plan 4.3). URIs, display names and bytes are never logged.

pub use crate::media::{
    CameraEntry, CameraPermission, Delivery, MEDIA_CACHE_MAX_AGE, MEDIA_COPY_CHUNK, MEDIA_DIR,
    MEDIA_MAX_BYTES, MEDIA_MAX_ITEMS, MediaData, MediaError, MediaItem, MediaKind, MediaReply,
    MediaRequest, MediaRequestId, MediaResponse, MediaSource, Picker,
};

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use super::jni_util::{opt_string, with_app_class};
use super::permission_gate;
use crate::event::PermissionRequest;
use crate::mailbox::Mailbox;
use crate::media::{self, RawItem, RawResult, Spec};
use android_activity::AndroidApp;
use jni::objects::{JIntArray, JLongArray, JObject, JObjectArray, JString};
use jni::sys::{jboolean, jint, jlong};
use jni::{Env, EnvUnowned, jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.media.MediaRequestPlugin";
const CAMERA: &str = "android.permission.CAMERA";

static RESULTS: Mailbox<MediaResponse> = Mailbox::new();

/// The next request's id; 0 is never handed out.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// `android::run` calls in this process (plan 6.2): an answer to a request
/// of an earlier run is delivered with `request: None`.
static RUN: AtomicU32 = AtomicU32::new(0);

/// The request in flight: one at a time, as Android shows one chooser.
static IN_FLIGHT: Mutex<Option<InFlight>> = Mutex::new(None);

#[derive(Clone, Copy, Debug)]
struct InFlight {
    id: MediaRequestId,
    run: u32,
    /// Waiting for CAMERA at the gate; the chooser is not open yet.
    gate: Option<PermissionRequest>,
}

fn in_flight() -> MutexGuard<'static, Option<InFlight>> {
    IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn take_results() -> Vec<MediaResponse> {
    RESULTS.take()
}

/// A new `android::run` starts.
pub(crate) fn begin_run() {
    RUN.fetch_add(1, Ordering::SeqCst);
}

/// The run ends. A request still waiting at the permission gate will get
/// no answer (the permission queue goes with the activity), so it is
/// dropped; one whose chooser is open stays in flight for the next run.
pub(crate) fn end_run() {
    let mut slot = in_flight();
    if let Some(gate) = slot.and_then(|f| f.gate) {
        permission_gate::withdraw(gate);
        *slot = None;
    }
}

/// Ask the user for media (plan 2, 4). Returns at once; the answer is
/// `MobileEvent::Media` with this id. Any thread except a JNI callback; the
/// app must be visible ([`MediaError::NotVisible`] otherwise).
///
/// One request at a time: another while one is in flight is answered with
/// [`MediaError::Busy`]. With a camera entry, CAMERA may be asked for first
/// (see [`CameraPermission`]); the chooser then opens after the answer.
pub fn request_media(app: &AndroidApp, request: &MediaRequest) -> MediaRequestId {
    let id = MediaRequestId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    {
        let mut slot = in_flight();
        if slot.is_some() {
            drop(slot);
            log::info!("libmobile: media request {}: busy", id.0);
            push(Some(id), Err(MediaError::Busy));
            return id;
        }
        *slot = Some(InFlight {
            id,
            run: RUN.load(Ordering::SeqCst),
            gate: None,
        });
    }

    let mut camera_left_out = false;
    if request.camera != CameraEntry::None
        && declares(app, CAMERA)
        && !super::permission_granted(app, CAMERA)
    {
        match request.camera_permission {
            CameraPermission::IfGranted => camera_left_out = true,
            _ => {
                ask_camera(app, id, request.clone());
                return id;
            }
        }
    }
    launch(app, id, request, camera_left_out);
    id
}

/// CAMERA through the gate; the chooser opens on a grant.
fn ask_camera(app: &AndroidApp, id: MediaRequestId, request: MediaRequest) {
    let mut slot = in_flight();
    let gate = permission_gate::request(
        app,
        &[CAMERA],
        Box::new(move |app, outcome| {
            let granted = outcome.granted(CAMERA);
            let rationale = !granted && super::should_show_rationale(app, CAMERA);
            log::info!(
                "libmobile: media request {}: CAMERA granted: {granted}",
                id.0
            );
            {
                let mut slot = in_flight();
                match *slot {
                    Some(ref mut f) if f.id == id => f.gate = None,
                    // Dropped at the end of its run.
                    _ => return,
                }
            }
            match media::camera_gate(granted, rationale) {
                Ok(()) => launch(app, id, &request, false),
                Err(error) => finish(id, Err(error)),
            }
        }),
    );
    if let Some(f) = slot.as_mut().filter(|f| f.id == id) {
        f.gate = Some(gate);
    }
    log::info!(
        "libmobile: media request {}: CAMERA asked (request {})",
        id.0,
        gate.id()
    );
}

/// Whether the Photo Picker is available on this device: API 33+, or API
/// 30-32 with SDK extension R >= 2 (a Play system update). Any thread.
pub fn photo_picker_available(app: &AndroidApp) -> bool {
    let answer = with_app_class(app, PLUGIN_CLASS, |env, _activity, class| {
        let called = env.call_static_method(
            class,
            jni_str!("photoPickerAvailable"),
            jni_sig!("()Z"),
            &[],
        );
        caught(env, called)?.z()
    });
    answer.unwrap_or_else(|err| {
        log::warn!("libmobile: media: photo picker check failed: {err}");
        false
    })
}

/// Whether the app's own manifest requests `permission`
/// (`PackageInfo.requestedPermissions`).
fn declares(app: &AndroidApp, permission: &str) -> bool {
    let answer = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        let name = JString::from_str(env, permission)?;
        let called = env.call_static_method(
            class,
            jni_str!("declares"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Z"),
            &[activity.into(), (&name).into()],
        );
        caught(env, called)?.z()
    });
    answer.unwrap_or_else(|err| {
        log::warn!("libmobile: media: manifest check failed: {err}");
        false
    })
}

/// A pending Java exception is cleared, so the thread can go on calling.
fn caught<T>(env: &mut Env<'_>, called: jni::errors::Result<T>) -> jni::errors::Result<T> {
    if let Err(jni::errors::Error::JavaException) = called {
        env.exception_catch()?;
    }
    called
}

/// Hand the flattened request to the plugin (it opens the chooser on the UI
/// thread).
fn launch(app: &AndroidApp, id: MediaRequestId, request: &MediaRequest, camera_left_out: bool) {
    let spec = media::spec(request, || photo_picker_available(app), camera_left_out);
    log::info!(
        "libmobile: media request {}: {} (types {:?} + {:?}, max {} items, {} bytes each, camera {})",
        id.0,
        if spec.photo_picker {
            "Photo Picker"
        } else {
            "chooser"
        },
        spec.mime_type,
        spec.extra_mimes,
        spec.max_items,
        spec.max_bytes,
        spec.camera_bits()
    );
    let result = with_app_class(app, PLUGIN_CLASS, |env, activity, class| {
        call_request(env, activity, class, id, &spec)
    });
    if let Err(err) = result {
        log::error!("libmobile: media request {}: could not start: {err}", id.0);
        finish(id, Err(MediaError::Failed(format!("jni: {err}"))));
    }
}

fn call_request(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    class: &jni::objects::JClass<'_>,
    id: MediaRequestId,
    spec: &Spec,
) -> jni::errors::Result<()> {
    let ints = JIntArray::new(env, 5)?;
    ints.set_region(
        env,
        0,
        &[
            spec.photo_picker as jint,
            spec.max_items as jint,
            match spec.delivery {
                Delivery::File => 1,
                _ => 0,
            },
            spec.camera_bits(),
            spec.camera_left_out as jint,
        ],
    )?;
    let mime_type = match &spec.mime_type {
        Some(t) => JString::from_str(env, t)?,
        None => JString::null(),
    };
    let empty = JString::from_str(env, "")?;
    let mimes = JObjectArray::<JString>::new(env, spec.extra_mimes.len(), &empty)?;
    for (i, mime) in spec.extra_mimes.iter().enumerate() {
        let mime = JString::from_str(env, mime)?;
        mimes.set_element(env, i, &mime)?;
    }
    let title = match &spec.title {
        Some(t) => JString::from_str(env, t)?,
        None => JString::null(),
    };
    let max_bytes = jlong::try_from(spec.max_bytes).unwrap_or(jlong::MAX);
    let called = env.call_static_method(
        class,
        jni_str!("request"),
        jni_sig!(
            "(Landroid/app/Activity;I[IJLjava/lang/String;[Ljava/lang/String;Ljava/lang/String;)V"
        ),
        &[
            activity.into(),
            (id.0 as jint).into(),
            (&ints).into(),
            max_bytes.into(),
            (&mime_type).into(),
            (&mimes).into(),
            (&title).into(),
        ],
    );
    caught(env, called).map(|_| ())
}

/// The request `id` is answered: no longer in flight.
fn finish(id: MediaRequestId, result: Result<MediaReply, MediaError>) {
    let request = {
        let mut slot = in_flight();
        match *slot {
            Some(f) if f.id == id => {
                *slot = None;
                (f.run == RUN.load(Ordering::SeqCst)).then_some(id)
            }
            // Not in flight here (a restored process): not this run's.
            _ => None,
        }
    };
    push(request, result);
}

fn push(request: Option<MediaRequestId>, result: Result<MediaReply, MediaError>) {
    match &result {
        Ok(reply) => log::info!(
            "libmobile: media {:?}: {} item(s) [{}], picker {:?}, camera_left_out {}, truncated {}",
            request.map(|r| r.0),
            reply.items.len(),
            reply
                .items
                .iter()
                .map(describe)
                .collect::<Vec<_>>()
                .join(", "),
            reply.picker,
            reply.camera_left_out,
            reply.truncated
        ),
        Err(error) => log::info!("libmobile: media {:?}: {error:?}", request.map(|r| r.0)),
    }
    RESULTS.push(MediaResponse { request, result });
}

/// An item for the log: source, MIME type, size and outcome, never its URI
/// or name.
fn describe(item: &MediaItem) -> String {
    let data = match &item.data {
        MediaData::Bytes(b) => format!("{} bytes", b.len()),
        MediaData::File(_) => "file".into(),
        MediaData::TooLarge { size } => format!("too large ({size:?})"),
        MediaData::Unreadable(why) => format!("unreadable ({why})"),
    };
    format!(
        "{:?} {} {:?} exif {} {data}",
        item.source,
        item.mime.as_deref().unwrap_or("?"),
        item.size,
        item.exif_orientation
    )
}

fn strings(
    env: &mut Env<'_>,
    array: &JObjectArray<'_, JString<'_>>,
) -> jni::errors::Result<Vec<Option<String>>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let len = array.len(env)?;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(opt_string(&array.get_element(env, i)?));
    }
    Ok(out)
}

fn ints(env: &mut Env<'_>, array: &JIntArray<'_>) -> jni::errors::Result<Vec<i32>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let mut out = vec![0; array.len(env)?];
    array.get_region(env, 0, &mut out)?;
    Ok(out)
}

fn longs(env: &mut Env<'_>, array: &JLongArray<'_>) -> jni::errors::Result<Vec<i64>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let mut out = vec![0; array.len(env)?];
    array.get_region(env, 0, &mut out)?;
    Ok(out)
}

/// Kotlin -> Rust: `MediaRequestPlugin.nativeMediaResult(int id, boolean
/// restored, String error, int picker, int delivery, boolean cameraLeftOut,
/// boolean truncated, int[] sources, String[] uris, String[] mimes,
/// String[] names, long[] sizes, String[] paths, String[] itemErrors, int[]
/// exif)`. For `Delivery::Bytes` each file is read here, on the caller's
/// thread, and deleted.
#[allow(unsafe_code)] // Scoped: the `#[unsafe(no_mangle)]` JNI export symbol.
#[allow(clippy::too_many_arguments)] // The JNI signature.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_highbay_libmobile_media_MediaRequestPlugin_nativeMediaResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    id: jint,
    restored: jboolean,
    error: JString<'caller>,
    picker: jint,
    delivery: jint,
    camera_left_out: jboolean,
    truncated: jboolean,
    sources: JIntArray<'caller>,
    uris: JObjectArray<'caller, JString<'caller>>,
    mimes: JObjectArray<'caller, JString<'caller>>,
    names: JObjectArray<'caller, JString<'caller>>,
    sizes: JLongArray<'caller>,
    paths: JObjectArray<'caller, JString<'caller>>,
    item_errors: JObjectArray<'caller, JString<'caller>>,
    exif: JIntArray<'caller>,
) {
    let outcome = unowned_env.with_env(|env| -> Result<(), jni::errors::Error> {
        let sources = ints(env, &sources)?;
        let uris = strings(env, &uris)?;
        let mimes = strings(env, &mimes)?;
        let names = strings(env, &names)?;
        let sizes = longs(env, &sizes)?;
        let paths = strings(env, &paths)?;
        let item_errors = strings(env, &item_errors)?;
        let exif = ints(env, &exif)?;
        let items = (0..sources.len())
            .map(|i| RawItem {
                camera: sources[i] == 1,
                uri: uris.get(i).cloned().flatten().unwrap_or_default(),
                mime: mimes.get(i).cloned().flatten(),
                name: names.get(i).cloned().flatten(),
                size: sizes.get(i).copied().unwrap_or(-1),
                path: paths.get(i).cloned().flatten(),
                error: item_errors.get(i).cloned().flatten(),
                exif: exif.get(i).copied().unwrap_or(0),
            })
            .collect();
        let raw = RawResult {
            error: opt_string(&error),
            items,
            picker,
            delivery,
            camera_left_out,
            truncated,
        };
        let result = media::reply_from_raw(raw, |path| {
            let bytes = std::fs::read(path)?;
            if let Err(err) = std::fs::remove_file(path) {
                log::warn!("libmobile: media: cache file not deleted: {:?}", err.kind());
            }
            Ok(bytes)
        });
        let id = MediaRequestId(id as u32);
        if restored {
            push(None, result);
        } else {
            finish(id, result);
        }
        Ok(())
    });
    outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
