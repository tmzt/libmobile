//! Decoding and encoding images through Android's own codecs
//! (`image/ImageCodecPlugin.kt`, feature `image-codec`).
//!
//! [`decode_image`] goes through `ImageDecoder`: the EXIF orientation of a
//! JPEG or HEIF is applied, HEIC/HEIF, PNG, JPEG, WebP, GIF, BMP and DNG are
//! read, an animation gives its first frame, and the pixels come back as
//! sRGB [`Rgba`] with straight alpha. [`encode_image`] writes JPEG or PNG
//! through `Bitmap.compress`. [`probe_image`] reads the MIME type, the
//! stored size and the EXIF orientation from the headers only.
//!
//! **Threads.** All three block (a 12 MP HEIC decode takes some hundreds of
//! milliseconds) and may run on any thread **except** the main loop's: run
//! them through [`Workers`](crate::worker::Workers) or a thread of the
//! app's own. They need no `AndroidApp` (they reach the plugin through the
//! Application android-activity registers with `ndk-context`, as
//! [`cache_dir`](super::dirs::cache_dir) does) and may run concurrently.
//!
//! **Memory.** A decode peaks at about twice its output (the bitmap's
//! pixels and the `Rgba`); an encode at the bitmap plus the output. Neither
//! may be over [`IMAGE_MAX_PIXELS`].

pub use crate::image_codec::{Encoding, IMAGE_MAX_PIXELS, ImageError, ImageInfo, Rgba, fit_within};

use super::jni_util::{opt_string, with_application_class};
use crate::image_codec::{check_encode, pack_rows};
use jni::objects::{JByteArray, JClass, JIntArray, JObject, JObjectArray, JString};
use jni::signature::MethodSignature;
use jni::strings::JNIStr;
use jni::{Env, JValue, jni_sig, jni_str};

const PLUGIN_CLASS: &str = "dev.highbay.libmobile.image.ImageCodecPlugin";

/// `ImageCodecPlugin.encode`'s `format`; keep in step with the Kotlin.
const FORMAT_JPEG: i32 = 0;
const FORMAT_PNG: i32 = 1;

/// The MIME type, the size as stored (before the EXIF rotation) and the
/// EXIF orientation, from the headers alone: no pixels are decoded. Copies
/// `bytes` into the Java heap once. Any thread but the main loop's.
///
/// [`ImageError::Unsupported`] when Android does not recognise the format.
/// An app uses it to tell whether a photo needs converting (HEIC/HEIF, an
/// orientation other than 1, or a size) before decoding it.
pub fn probe_image(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
    let answer = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        let array = env.byte_array_from_slice(bytes)?;
        let result = call(
            env,
            class,
            jni_str!("probe"),
            jni_sig!("([B)[Ljava/lang/Object;"),
            &[(&array).into()],
        )?;
        let (value, text) = pair(env, result)?;
        let text = text.unwrap_or_default();
        if value.is_null() {
            return Ok(Err(ImageError::from_message(&text)));
        }
        let ints = env.cast_local::<JIntArray>(value)?;
        let mut parts = [0i32; 3];
        ints.get_region(env, 0, &mut parts)?;
        Ok(Ok(ImageInfo::from_parts(
            text, parts[0], parts[1], parts[2],
        )))
    });
    answer.map_err(platform)?
}

/// Decode `bytes` (any format Android reads) to upright sRGB [`Rgba`], the
/// long side at most `max_side` ([`fit_within`]; 0 keeps the full size,
/// never enlarged). Any thread but the main loop's.
///
/// `bytes` is lent to Android as a direct `ByteBuffer`, not copied.
/// Errors: [`ImageError::Unsupported`] (not an image Android decodes),
/// [`Malformed`](ImageError::Malformed), [`Incomplete`](ImageError::Incomplete)
/// (a truncated file), [`TooLarge`](ImageError::TooLarge) (the output would
/// be over [`IMAGE_MAX_PIXELS`]; nothing is decoded),
/// [`OutOfMemory`](ImageError::OutOfMemory).
pub fn decode_image(bytes: &[u8], max_side: u32) -> Result<Rgba, ImageError> {
    let max_side = i32::try_from(max_side).unwrap_or(i32::MAX);
    let answer = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        #[allow(unsafe_code)] // Scoped: a direct ByteBuffer over `bytes`, read only.
        // SAFETY: the buffer covers exactly `bytes`, which outlives this
        // synchronous call; `ImageDecoder` reads it within `decode` and keeps
        // no reference once `decodeBitmap` returns. The plugin never writes
        // to it, so lending a shared slice as `*mut` is sound.
        let buffer = unsafe { env.new_direct_byte_buffer(bytes.as_ptr().cast_mut(), bytes.len())? };
        let result = call(
            env,
            class,
            jni_str!("decode"),
            jni_sig!("(Ljava/nio/ByteBuffer;IJ)[Ljava/lang/Object;"),
            &[
                (&buffer).into(),
                JValue::Int(max_side),
                JValue::Long(IMAGE_MAX_PIXELS as i64),
            ],
        )?;
        let (bitmap, error) = pair(env, result)?;
        if bitmap.is_null() {
            return Ok(Err(ImageError::from_message(&error.unwrap_or_default())));
        }
        copy_out(env, class, &bitmap)
    });
    answer.map_err(platform)?
}

/// The decoded bitmap's pixels into a new [`Rgba`]; `copyOut` recycles it.
fn copy_out(
    env: &mut Env<'_>,
    class: &JClass<'_>,
    bitmap: &JObject<'_>,
) -> jni::errors::Result<Result<Rgba, ImageError>> {
    let int = |env: &mut Env<'_>, name: &'static JNIStr| -> jni::errors::Result<i32> {
        env.call_method(bitmap, name, jni_sig!("()I"), &[])?.i()
    };
    let width = int(env, jni_str!("getWidth"))?;
    let height = int(env, jni_str!("getHeight"))?;
    let row_bytes = int(env, jni_str!("getRowBytes"))?;
    let (Ok(width), Ok(height), Ok(row_bytes)) = (
        usize::try_from(width),
        usize::try_from(height),
        usize::try_from(row_bytes),
    ) else {
        recycle(env, bitmap);
        return Ok(Err(ImageError::Platform("negative bitmap size".into())));
    };
    if row_bytes < width * 4 {
        recycle(env, bitmap);
        return Ok(Err(ImageError::Platform(
            "bitmap rows shorter than width * 4".into(),
        )));
    }
    let len = row_bytes * height;
    let mut pixels = Vec::new();
    if pixels.try_reserve_exact(len).is_err() {
        recycle(env, bitmap);
        return Ok(Err(ImageError::OutOfMemory));
    }
    pixels.resize(len, 0);
    #[allow(unsafe_code)] // Scoped: a direct ByteBuffer over `pixels`, written by `copyOut`.
    // SAFETY: the buffer covers exactly `pixels`, which is not touched from
    // Rust until `copyOut` returns; `copyPixelsToBuffer` writes at most the
    // buffer's capacity and keeps no reference.
    let buffer = unsafe { env.new_direct_byte_buffer(pixels.as_mut_ptr(), len)? };
    let error = call(
        env,
        class,
        jni_str!("copyOut"),
        jni_sig!("(Landroid/graphics/Bitmap;Ljava/nio/ByteBuffer;)Ljava/lang/String;"),
        &[bitmap.into(), (&buffer).into()],
    )?;
    let error = env.cast_local::<JString>(error)?;
    if let Some(error) = opt_string(&error) {
        return Ok(Err(ImageError::from_message(&error)));
    }
    pack_rows(&mut pixels, width, height, row_bytes);
    Ok(Ok(Rgba {
        width: width as u32,
        height: height as u32,
        pixels,
    }))
}

/// `Bitmap.recycle()` on an error path before `copyOut`; a failure is
/// cleared (the garbage collector frees the bitmap anyway).
fn recycle(env: &mut Env<'_>, bitmap: &JObject<'_>) {
    if env
        .call_method(bitmap, jni_str!("recycle"), jni_sig!("()V"), &[])
        .is_err()
    {
        env.exception_clear();
    }
}

/// Encode `image` as JPEG or PNG through `Bitmap.compress`. Any thread but
/// the main loop's. The pixels are lent to Android as a direct `ByteBuffer`,
/// not copied; the output carries no EXIF.
///
/// [`ImageError::BadInput`] before anything reaches Android: a length that
/// is not `width * height * 4`, a side outside 1..=32768, more than
/// [`IMAGE_MAX_PIXELS`], a JPEG quality over 100. [`Encoding::Png`] ignores
/// quality. [`Encoding::Jpeg`] ignores alpha, so encode an image that is not
/// [`is_opaque`](Rgba::is_opaque) as PNG, or flatten it first.
pub fn encode_image(image: &Rgba, encoding: Encoding) -> Result<Vec<u8>, ImageError> {
    check_encode(image, encoding)?;
    let (format, quality) = match encoding {
        Encoding::Jpeg { quality } => (FORMAT_JPEG, i32::from(quality)),
        Encoding::Png => (FORMAT_PNG, 100),
    };
    let pixels = &image.pixels;
    let answer = with_application_class(PLUGIN_CLASS, |env, _context, class| {
        #[allow(unsafe_code)] // Scoped: a direct ByteBuffer over the pixels, read only.
        // SAFETY: the buffer covers exactly `pixels`, borrowed for this
        // synchronous call; `copyPixelsFromBuffer` only reads it and keeps no
        // reference, so lending a shared slice as `*mut` is sound.
        let buffer =
            unsafe { env.new_direct_byte_buffer(pixels.as_ptr().cast_mut(), pixels.len())? };
        let result = call(
            env,
            class,
            jni_str!("encode"),
            jni_sig!("(IILjava/nio/ByteBuffer;II)[Ljava/lang/Object;"),
            &[
                JValue::Int(image.width as i32),
                JValue::Int(image.height as i32),
                (&buffer).into(),
                JValue::Int(format),
                JValue::Int(quality),
            ],
        )?;
        let (bytes, error) = pair(env, result)?;
        if bytes.is_null() {
            return Ok(Err(ImageError::from_message(&error.unwrap_or_default())));
        }
        let bytes = env.cast_local::<JByteArray>(bytes)?;
        Ok(Ok(env.convert_byte_array(&bytes)?))
    });
    answer.map_err(platform)?
}

/// A static call on the plugin whose answer is an object; a thrown
/// exception is caught and cleared.
fn call<'local, 'sig, 'args>(
    env: &mut Env<'local>,
    class: &JClass<'_>,
    name: &'static JNIStr,
    sig: impl AsRef<MethodSignature<'sig, 'args>>,
    args: &[JValue<'_>],
) -> jni::errors::Result<JObject<'local>> {
    match env.call_static_method(class, name, sig, args) {
        Err(jni::errors::Error::JavaException) => {
            env.exception_catch()?;
            Err(jni::errors::Error::JavaException)
        }
        other => other?.l(),
    }
}

/// The plugin's `[value, text]` answer: the value (maybe null) and the
/// text (an error, or `probe`'s MIME type).
fn pair<'local>(
    env: &mut Env<'local>,
    answer: JObject<'local>,
) -> jni::errors::Result<(JObject<'local>, Option<String>)> {
    let array = env.cast_local::<JObjectArray<JObject>>(answer)?;
    let value = array.get_element(env, 0)?;
    let text = array.get_element(env, 1)?;
    let text = if text.is_null() {
        None
    } else {
        opt_string(&env.cast_local::<JString>(text)?)
    };
    Ok((value, text))
}

fn platform(err: jni::errors::Error) -> ImageError {
    let error = match err {
        jni::errors::Error::CaughtJavaException { name, .. } => ImageError::Platform(name),
        other => ImageError::Platform(other.to_string()),
    };
    log::warn!("libmobile: image codec: {error}");
    error
}
