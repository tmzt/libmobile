//! Owners of the NDK camera handles (`libcamera2ndk`, `libmediandk`), used
//! only on the `libmobile-camera` thread.
//!
//! Every NDK callback here gets the *generation* of the device or stream
//! set it belongs to as its context (an integer, never a pointer), and only
//! posts a message to the camera thread. Nothing it could outlive is
//! referenced, so a late callback after a close is harmless: the thread
//! drops messages of an old generation. The NDK copies every callback
//! struct it is given, so those live on the stack.

// The whole module is FFI: each `unsafe` block is a single NDK call on a
// handle this module owns (non-null, not yet freed), or a read of data the
// NDK returned for the duration of the call.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int, c_void};
use std::ptr::{self, NonNull};

use ndk_sys as ffi;

use super::worker::{NdkEvent, post_ndk};

/// A non-OK `camera_status_t` or `media_status_t`.
pub(super) type Status = i32;

fn check(status: ffi::camera_status_t) -> Result<(), Status> {
    if status.0 == ffi::camera_status_t::ACAMERA_OK.0 {
        Ok(())
    } else {
        Err(status.0)
    }
}

fn check_media(status: ffi::media_status_t) -> Result<(), Status> {
    if status.0 == ffi::media_status_t::AMEDIA_OK.0 {
        Ok(())
    } else {
        Err(status.0)
    }
}

/// `ACAMERA_ERROR_*` codes the thread maps to [`CameraError`](crate::camera::CameraError).
pub(super) mod status {
    pub const PERMISSION_DENIED: i32 = -10013;
    pub const CAMERA_DISABLED: i32 = -10012;
    pub const MAX_CAMERA_IN_USE: i32 = -10011;
    pub const CAMERA_IN_USE: i32 = -10010;
    pub const CAMERA_DISCONNECTED: i32 = -10002;
}

/// `ACameraDevice` `onError` codes.
pub(super) mod device_error {
    pub const CAMERA_IN_USE: i32 = 1;
    pub const MAX_CAMERAS_IN_USE: i32 = 2;
    pub const CAMERA_DISABLED: i32 = 3;
}

/// Metadata tags, as the `u32` the NDK calls take.
pub(super) mod tag {
    use ndk_sys::acamera_metadata_tag as T;
    pub const LENS_FACING: u32 = T::ACAMERA_LENS_FACING.0;
    pub const SENSOR_ORIENTATION: u32 = T::ACAMERA_SENSOR_ORIENTATION.0;
    pub const STREAM_CONFIGURATIONS: u32 = T::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS.0;
    pub const ZOOM_RATIO_RANGE: u32 = T::ACAMERA_CONTROL_ZOOM_RATIO_RANGE.0;
    pub const ZOOM_RATIO: u32 = T::ACAMERA_CONTROL_ZOOM_RATIO.0;
    pub const CROP_REGION: u32 = T::ACAMERA_SCALER_CROP_REGION.0;
    pub const MAX_DIGITAL_ZOOM: u32 = T::ACAMERA_SCALER_AVAILABLE_MAX_DIGITAL_ZOOM.0;
    pub const ACTIVE_ARRAY_SIZE: u32 = T::ACAMERA_SENSOR_INFO_ACTIVE_ARRAY_SIZE.0;
    pub const AF_AVAILABLE_MODES: u32 = T::ACAMERA_CONTROL_AF_AVAILABLE_MODES.0;
    pub const AF_MODE: u32 = T::ACAMERA_CONTROL_AF_MODE.0;
    pub const AE_MODE: u32 = T::ACAMERA_CONTROL_AE_MODE.0;
    pub const AE_ANTIBANDING_MODE: u32 = T::ACAMERA_CONTROL_AE_ANTIBANDING_MODE.0;
    pub const AE_STATE: u32 = T::ACAMERA_CONTROL_AE_STATE.0;
    pub const AF_STATE: u32 = T::ACAMERA_CONTROL_AF_STATE.0;
    #[cfg(feature = "camera-capture")]
    pub const JPEG_ORIENTATION: u32 = T::ACAMERA_JPEG_ORIENTATION.0;
    #[cfg(feature = "camera-capture")]
    pub const JPEG_QUALITY: u32 = T::ACAMERA_JPEG_QUALITY.0;
}

/// `AIMAGE_FORMAT_YUV_420_888`.
pub(super) const FORMAT_YUV: i32 = ffi::AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32;
/// `AIMAGE_FORMAT_JPEG`.
#[cfg(feature = "camera-capture")]
pub(super) const FORMAT_JPEG: i32 = ffi::AIMAGE_FORMATS::AIMAGE_FORMAT_JPEG.0 as i32;

/// Capture request templates.
#[derive(Clone, Copy, Debug)]
pub(super) enum Template {
    Preview,
    #[cfg(feature = "camera-capture")]
    Still,
}

fn context(epoch: u64) -> *mut c_void {
    epoch as usize as *mut c_void
}

fn generation_of(context: *mut c_void) -> u64 {
    context as usize as u64
}

/// `ACameraManager`.
pub(super) struct Manager(NonNull<ffi::ACameraManager>);

impl Manager {
    pub(super) fn new() -> Option<Self> {
        NonNull::new(unsafe { ffi::ACameraManager_create() }).map(Manager)
    }

    pub(super) fn camera_ids(&self) -> Result<Vec<CString>, Status> {
        let mut list: *mut ffi::ACameraIdList = ptr::null_mut();
        check(unsafe { ffi::ACameraManager_getCameraIdList(self.0.as_ptr(), &mut list) })?;
        if list.is_null() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        unsafe {
            let l = &*list;
            for i in 0..l.numCameras.max(0) as usize {
                let id = *l.cameraIds.add(i);
                if !id.is_null() {
                    ids.push(CStr::from_ptr(id).to_owned());
                }
            }
            ffi::ACameraManager_deleteCameraIdList(list);
        }
        Ok(ids)
    }

    pub(super) fn characteristics(&self, id: &CStr) -> Result<Metadata, Status> {
        let mut meta: *mut ffi::ACameraMetadata = ptr::null_mut();
        check(unsafe {
            ffi::ACameraManager_getCameraCharacteristics(self.0.as_ptr(), id.as_ptr(), &mut meta)
        })?;
        NonNull::new(meta).map(Metadata).ok_or(-1)
    }

    /// Open camera `id`; its disconnect and error callbacks carry `epoch`.
    pub(super) fn open(&self, id: &CStr, epoch: u64) -> Result<Device, Status> {
        let mut callbacks = ffi::ACameraDevice_StateCallbacks {
            context: context(epoch),
            onDisconnected: Some(on_disconnected),
            onError: Some(on_error),
        };
        let mut device: *mut ffi::ACameraDevice = ptr::null_mut();
        check(unsafe {
            ffi::ACameraManager_openCamera(
                self.0.as_ptr(),
                id.as_ptr(),
                &mut callbacks,
                &mut device,
            )
        })?;
        NonNull::new(device).map(Device).ok_or(-1)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        unsafe { ffi::ACameraManager_delete(self.0.as_ptr()) };
    }
}

/// Owned `ACameraMetadata` (camera characteristics).
pub(super) struct Metadata(NonNull<ffi::ACameraMetadata>);

impl Metadata {
    pub(super) fn u8s(&self, tag: u32) -> Vec<u8> {
        unsafe { entry(self.0.as_ptr(), tag) }
            .map(|e| unsafe { slice(e.data.u8_, e.count) })
            .unwrap_or_default()
    }

    pub(super) fn i32s(&self, tag: u32) -> Vec<i32> {
        unsafe { entry(self.0.as_ptr(), tag) }
            .map(|e| unsafe { slice(e.data.i32_, e.count) })
            .unwrap_or_default()
    }

    pub(super) fn f32s(&self, tag: u32) -> Vec<f32> {
        unsafe { entry(self.0.as_ptr(), tag) }
            .map(|e| unsafe { slice(e.data.f, e.count) })
            .unwrap_or_default()
    }
}

impl Drop for Metadata {
    fn drop(&mut self) {
        unsafe { ffi::ACameraMetadata_free(self.0.as_ptr()) };
    }
}

/// # Safety
/// `meta` is a live `ACameraMetadata`.
unsafe fn entry(
    meta: *const ffi::ACameraMetadata,
    tag: u32,
) -> Option<ffi::ACameraMetadata_const_entry> {
    let mut e = std::mem::MaybeUninit::<ffi::ACameraMetadata_const_entry>::zeroed();
    let status = unsafe { ffi::ACameraMetadata_getConstEntry(meta, tag, e.as_mut_ptr()) };
    check(status).ok()?;
    Some(unsafe { e.assume_init() })
}

/// # Safety
/// `data` points at `count` values the metadata owns.
unsafe fn slice<T: Copy>(data: *const T, count: u32) -> Vec<T> {
    if data.is_null() || count == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(data, count as usize) }.to_vec()
}

/// An open `ACameraDevice`; closed on drop.
pub(super) struct Device(NonNull<ffi::ACameraDevice>);

impl Device {
    pub(super) fn request(&self, template: Template) -> Result<Request, Status> {
        let template = match template {
            Template::Preview => ffi::ACameraDevice_request_template::TEMPLATE_PREVIEW,
            #[cfg(feature = "camera-capture")]
            Template::Still => ffi::ACameraDevice_request_template::TEMPLATE_STILL_CAPTURE,
        };
        let mut request: *mut ffi::ACaptureRequest = ptr::null_mut();
        check(unsafe {
            ffi::ACameraDevice_createCaptureRequest(self.0.as_ptr(), template, &mut request)
        })?;
        let ptr = NonNull::new(request).ok_or(-1)?;
        Ok(Request {
            ptr,
            targets: Vec::new(),
        })
    }

    /// A capture session over `readers`' windows.
    pub(super) fn session(&self, readers: &[&Reader]) -> Result<Session, Status> {
        let mut container: *mut ffi::ACaptureSessionOutputContainer = ptr::null_mut();
        check(unsafe { ffi::ACaptureSessionOutputContainer_create(&mut container) })?;
        let mut session = Session {
            ptr: None,
            container,
            outputs: Vec::new(),
        };
        for reader in readers {
            let mut output: *mut ffi::ACaptureSessionOutput = ptr::null_mut();
            check(unsafe { ffi::ACaptureSessionOutput_create(reader.window, &mut output) })?;
            session.outputs.push(output);
            check(unsafe { ffi::ACaptureSessionOutputContainer_add(container, output) })?;
        }
        let callbacks = ffi::ACameraCaptureSession_stateCallbacks {
            context: ptr::null_mut(),
            onClosed: Some(on_session_state),
            onReady: Some(on_session_state),
            onActive: Some(on_session_state),
        };
        let mut raw: *mut ffi::ACameraCaptureSession = ptr::null_mut();
        check(unsafe {
            ffi::ACameraDevice_createCaptureSession(
                self.0.as_ptr(),
                container,
                &callbacks,
                &mut raw,
            )
        })?;
        session.ptr = NonNull::new(raw);
        if session.ptr.is_none() {
            return Err(-1);
        }
        Ok(session)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { ffi::ACameraDevice_close(self.0.as_ptr()) };
    }
}

/// An `ACameraCaptureSession` and the outputs it was made from; closed on
/// drop, before the readers whose windows it streams to.
pub(super) struct Session {
    ptr: Option<NonNull<ffi::ACameraCaptureSession>>,
    container: *mut ffi::ACaptureSessionOutputContainer,
    outputs: Vec<*mut ffi::ACaptureSessionOutput>,
}

impl Session {
    fn raw(&self) -> *mut ffi::ACameraCaptureSession {
        self.ptr.map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    /// Repeat `request` (replacing any repeating request); each completed
    /// frame's AE and AF states are posted with `epoch`.
    pub(super) fn repeat(&self, request: &Request, epoch: u64) -> Result<(), Status> {
        let mut callbacks = capture_callbacks(epoch);
        callbacks.onCaptureCompleted = Some(on_repeat_completed);
        let mut requests = [request.ptr.as_ptr()];
        check(unsafe {
            ffi::ACameraCaptureSession_setRepeatingRequest(
                self.raw(),
                &mut callbacks,
                1,
                requests.as_mut_ptr(),
                ptr::null_mut(),
            )
        })
    }

    /// Send `request` once; a failure is posted with `epoch`.
    #[cfg(feature = "camera-capture")]
    pub(super) fn capture(&self, request: &Request, epoch: u64) -> Result<(), Status> {
        let mut callbacks = capture_callbacks(epoch);
        callbacks.onCaptureFailed = Some(on_still_failed);
        let mut requests = [request.ptr.as_ptr()];
        check(unsafe {
            ffi::ACameraCaptureSession_capture(
                self.raw(),
                &mut callbacks,
                1,
                requests.as_mut_ptr(),
                ptr::null_mut(),
            )
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if let Some(session) = self.ptr {
                ffi::ACameraCaptureSession_stopRepeating(session.as_ptr());
                ffi::ACameraCaptureSession_close(session.as_ptr());
            }
            for output in self.outputs.drain(..) {
                ffi::ACaptureSessionOutput_free(output);
            }
            if !self.container.is_null() {
                ffi::ACaptureSessionOutputContainer_free(self.container);
            }
        }
    }
}

fn capture_callbacks(epoch: u64) -> ffi::ACameraCaptureSession_captureCallbacks {
    ffi::ACameraCaptureSession_captureCallbacks {
        context: context(epoch),
        onCaptureStarted: None,
        onCaptureProgressed: None,
        onCaptureCompleted: None,
        onCaptureFailed: None,
        onCaptureSequenceCompleted: None,
        onCaptureSequenceAborted: None,
        onCaptureBufferLost: None,
    }
}

/// An `ACaptureRequest` and its output targets. The NDK copies a request
/// when it is sent, so it is freed as soon as it has been.
pub(super) struct Request {
    ptr: NonNull<ffi::ACaptureRequest>,
    targets: Vec<*mut ffi::ACameraOutputTarget>,
}

impl Request {
    pub(super) fn add_target(&mut self, reader: &Reader) -> Result<(), Status> {
        let mut target: *mut ffi::ACameraOutputTarget = ptr::null_mut();
        check(unsafe { ffi::ACameraOutputTarget_create(reader.window, &mut target) })?;
        self.targets.push(target);
        check(unsafe { ffi::ACaptureRequest_addTarget(self.ptr.as_ptr(), target) })
    }

    pub(super) fn set_u8(&mut self, tag: u32, values: &[u8]) -> Result<(), Status> {
        check(unsafe {
            ffi::ACaptureRequest_setEntry_u8(
                self.ptr.as_ptr(),
                tag,
                values.len() as u32,
                values.as_ptr(),
            )
        })
    }

    pub(super) fn set_i32(&mut self, tag: u32, values: &[i32]) -> Result<(), Status> {
        check(unsafe {
            ffi::ACaptureRequest_setEntry_i32(
                self.ptr.as_ptr(),
                tag,
                values.len() as u32,
                values.as_ptr(),
            )
        })
    }

    pub(super) fn set_f32(&mut self, tag: u32, values: &[f32]) -> Result<(), Status> {
        check(unsafe {
            ffi::ACaptureRequest_setEntry_float(
                self.ptr.as_ptr(),
                tag,
                values.len() as u32,
                values.as_ptr(),
            )
        })
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        unsafe {
            ffi::ACaptureRequest_free(self.ptr.as_ptr());
            for target in self.targets.drain(..) {
                ffi::ACameraOutputTarget_free(target);
            }
        }
    }
}

/// Which listener an image reader posts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReaderKind {
    Analysis,
    Still,
}

/// An `AImageReader` and its window (owned by the reader); deleted on drop.
pub(super) struct Reader {
    ptr: NonNull<ffi::AImageReader>,
    window: *mut ffi::ANativeWindow,
}

impl Reader {
    pub(super) fn new(
        size: (u32, u32),
        format: i32,
        max_images: i32,
        kind: ReaderKind,
        epoch: u64,
    ) -> Result<Self, Status> {
        let mut raw: *mut ffi::AImageReader = ptr::null_mut();
        check_media(unsafe {
            ffi::AImageReader_new(size.0 as i32, size.1 as i32, format, max_images, &mut raw)
        })?;
        let ptr = NonNull::new(raw).ok_or(-1)?;
        // Owned from here, so an error below deletes it.
        let mut reader = Reader {
            ptr,
            window: ptr::null_mut(),
        };
        check_media(unsafe { ffi::AImageReader_getWindow(ptr.as_ptr(), &mut reader.window) })?;
        let mut listener = ffi::AImageReader_ImageListener {
            context: context(epoch),
            onImageAvailable: Some(match kind {
                ReaderKind::Analysis => on_analysis_image,
                ReaderKind::Still => on_still_image,
            }),
        };
        check_media(unsafe { ffi::AImageReader_setImageListener(ptr.as_ptr(), &mut listener) })?;
        Ok(reader)
    }

    /// The newest image, older ones released; `None` when none is queued.
    pub(super) fn latest(&self) -> Option<Image> {
        let mut image: *mut ffi::AImage = ptr::null_mut();
        check_media(unsafe { ffi::AImageReader_acquireLatestImage(self.ptr.as_ptr(), &mut image) })
            .ok()?;
        NonNull::new(image).map(Image)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        unsafe { ffi::AImageReader_delete(self.ptr.as_ptr()) };
    }
}

/// One acquired `AImage`; released on drop.
pub(super) struct Image(NonNull<ffi::AImage>);

impl Image {
    pub(super) fn size(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        unsafe {
            ffi::AImage_getWidth(self.0.as_ptr(), &mut w);
            ffi::AImage_getHeight(self.0.as_ptr(), &mut h);
        }
        (w.max(0) as u32, h.max(0) as u32)
    }

    pub(super) fn timestamp_ns(&self) -> i64 {
        let mut t = 0;
        unsafe { ffi::AImage_getTimestamp(self.0.as_ptr(), &mut t) };
        t
    }

    /// Plane `index`: its bytes, row stride and pixel stride. The bytes
    /// borrow the image.
    pub(super) fn plane(&self, index: i32) -> Option<(&[u8], usize, usize)> {
        let (mut data, mut len): (*mut u8, c_int) = (ptr::null_mut(), 0);
        let (mut row, mut pixel) = (0, 0);
        unsafe {
            check_media(ffi::AImage_getPlaneData(
                self.0.as_ptr(),
                index,
                &mut data,
                &mut len,
            ))
            .ok()?;
            // A JPEG plane has no strides; they stay 0.
            let _ = ffi::AImage_getPlaneRowStride(self.0.as_ptr(), index, &mut row);
            let _ = ffi::AImage_getPlanePixelStride(self.0.as_ptr(), index, &mut pixel);
        }
        if data.is_null() || len <= 0 {
            return None;
        }
        // SAFETY: the NDK keeps `len` bytes at `data` until the image is
        // deleted, which the borrow of `self` outlives.
        let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
        Some((bytes, row.max(0) as usize, pixel.max(0) as usize))
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { ffi::AImage_delete(self.0.as_ptr()) };
    }
}

// ── NDK callbacks: post and return ──────────────────────────────────────────

unsafe extern "C" fn on_disconnected(context: *mut c_void, _device: *mut ffi::ACameraDevice) {
    post_ndk(generation_of(context), NdkEvent::Disconnected);
}

unsafe extern "C" fn on_error(
    context: *mut c_void,
    _device: *mut ffi::ACameraDevice,
    error: c_int,
) {
    post_ndk(generation_of(context), NdkEvent::Error(error));
}

unsafe extern "C" fn on_session_state(
    _context: *mut c_void,
    _session: *mut ffi::ACameraCaptureSession,
) {
}

unsafe extern "C" fn on_analysis_image(context: *mut c_void, _reader: *mut ffi::AImageReader) {
    post_ndk(generation_of(context), NdkEvent::AnalysisImage);
}

unsafe extern "C" fn on_still_image(context: *mut c_void, _reader: *mut ffi::AImageReader) {
    post_ndk(generation_of(context), NdkEvent::StillImage);
}

unsafe extern "C" fn on_repeat_completed(
    context: *mut c_void,
    _session: *mut ffi::ACameraCaptureSession,
    _request: *mut ffi::ACaptureRequest,
    result: *const ffi::ACameraMetadata,
) {
    if result.is_null() {
        return;
    }
    // SAFETY: `result` is valid for the duration of the callback.
    let first = |tag| {
        unsafe { entry(result, tag) }
            .and_then(|e| unsafe { slice(e.data.u8_, e.count) }.first().copied())
    };
    post_ndk(
        generation_of(context),
        NdkEvent::Result {
            ae: first(tag::AE_STATE),
            af: first(tag::AF_STATE),
        },
    );
}

#[cfg(feature = "camera-capture")]
unsafe extern "C" fn on_still_failed(
    context: *mut c_void,
    _session: *mut ffi::ACameraCaptureSession,
    _request: *mut ffi::ACaptureRequest,
    _failure: *mut ffi::ACameraCaptureFailure,
) {
    post_ndk(generation_of(context), NdkEvent::StillFailed);
}
