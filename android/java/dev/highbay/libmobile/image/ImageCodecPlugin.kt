package dev.highbay.libmobile.image

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ColorSpace
import android.graphics.ImageDecoder
import android.media.ExifInterface
import dev.highbay.libmobile.MobileActivityPlugin
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer

/**
 * Image decoding and encoding through Android's own codecs
 * (`libmobile::android::image_codec`): [ImageDecoder] (API 28) to decode,
 * [Bitmap.compress] to encode, [BitmapFactory] bounds and [ExifInterface]
 * to probe. Framework only, no Jetpack.
 *
 * Rust calls the static methods from any thread but the main loop's; they
 * need no activity or context and may run concurrently. The pixel buffers
 * are direct `ByteBuffer`s over Rust memory, valid only for the call.
 *
 * Answers are `[value, null]` or `[null, "<kind>: <detail>"]`, which Rust
 * maps with `ImageError::from_message` (src/image_codec.rs). Kinds:
 * `unsupported`, `malformed`, `incomplete`, `too-large: <w>x<h>`,
 * `out-of-memory`, `platform: <exception class>`. An exception's message is
 * never passed on or logged.
 *
 * An activity plugin with no hooks, listed only under the `image-codec`
 * feature.
 */
object ImageCodecPlugin : MobileActivityPlugin {
    /** [encode]'s `format`: keep in step with `src/android/image_codec.rs`. */
    private const val FORMAT_JPEG = 0
    private const val FORMAT_PNG = 1

    private class TooLarge(val width: Int, val height: Int) : RuntimeException()

    private val SRGB: ColorSpace get() = ColorSpace.get(ColorSpace.Named.SRGB)

    /**
     * The size [decode] produces: the long side at most [maxSide], aspect
     * kept, rounded to the nearest, at least 1, never enlarged; 0 keeps the
     * size. Keep in step with `fit_within` (src/image_codec.rs).
     */
    private fun fitWithin(width: Int, height: Int, maxSide: Int): Pair<Int, Int> {
        val long = maxOf(width, height)
        if (maxSide <= 0 || long <= maxSide) return width to height
        fun scale(side: Int): Int =
            maxOf(1, ((side.toLong() * maxSide + long / 2) / long).toInt())
        return if (width >= height) maxSide to scale(height) else scale(width) to maxSide
    }

    private fun platform(e: Throwable): String = "platform: " + e.javaClass.name

    /**
     * Called from Rust via JNI: static,
     * `(Ljava/nio/ByteBuffer;IJ)[Ljava/lang/Object;`: `[Bitmap, null]` or
     * `[null, error]`. The bitmap is `ARGB_8888`, sRGB, not premultiplied,
     * upright (EXIF orientation applied) and scaled by [fitWithin]; Rust
     * passes it to [copyOut], which recycles it.
     */
    @JvmStatic
    fun decode(buffer: ByteBuffer, maxSide: Int, maxPixels: Long): Array<Any?> {
        return try {
            val source = ImageDecoder.createSource(buffer)
            val decoded = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
                // The size as displayed, after the EXIF rotation.
                val (width, height) = fitWithin(info.size.width, info.size.height, maxSide)
                if (width.toLong() * height > maxPixels) throw TooLarge(width, height)
                if (width != info.size.width || height != info.size.height) {
                    decoder.setTargetSize(width, height)
                }
                // A hardware bitmap cannot be read back.
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                decoder.isUnpremultipliedRequired = true
                decoder.setTargetColorSpace(SRGB)
                // A truncated file fails instead of giving a partly grey image.
                decoder.setOnPartialImageListener { false }
            }
            // A 16-bit PNG or an HDR photo decodes to RGBA_F16.
            val bitmap = if (decoded.config == Bitmap.Config.ARGB_8888) {
                decoded
            } else {
                val copy = decoded.copy(Bitmap.Config.ARGB_8888, false)
                decoded.recycle()
                copy ?: return arrayOf(null, "platform: Bitmap.copy")
            }
            arrayOf(bitmap, null)
        } catch (e: TooLarge) {
            arrayOf(null, "too-large: ${e.width}x${e.height}")
        } catch (e: ImageDecoder.DecodeException) {
            arrayOf(null, decodeError(e))
        } catch (e: OutOfMemoryError) {
            arrayOf(null, "out-of-memory: ")
        } catch (e: Exception) {
            arrayOf(null, platform(e))
        }
    }

    private fun decodeError(e: ImageDecoder.DecodeException): String = when (e.error) {
        ImageDecoder.DecodeException.SOURCE_INCOMPLETE -> "incomplete: "
        // `DecodeException` has no code of its own for an unknown format:
        // Skia's `kUnimplemented` ("unimplemented") comes as malformed data.
        ImageDecoder.DecodeException.SOURCE_MALFORMED_DATA ->
            if (e.message?.contains("unimplemented") == true) "unsupported: " else "malformed: "
        else -> platform(e.cause ?: e)
    }

    /**
     * Called from Rust via JNI: static,
     * `(Landroid/graphics/Bitmap;Ljava/nio/ByteBuffer;)Ljava/lang/String;`:
     * `null` once the pixels are in [buffer] (`rowBytes * height` bytes,
     * rows as stored), or an error. Recycles [bitmap] either way.
     */
    @JvmStatic
    fun copyOut(bitmap: Bitmap, buffer: ByteBuffer): String? {
        return try {
            bitmap.copyPixelsToBuffer(buffer)
            null
        } catch (e: Exception) {
            platform(e)
        } finally {
            bitmap.recycle()
        }
    }

    /**
     * Called from Rust via JNI: static, `([B)[Ljava/lang/Object;`:
     * `[IntArray(width, height, exifOrientation), mime]` or `[null, error]`.
     * Headers only: the size as stored, before the EXIF rotation; an
     * orientation of 0 when there is none.
     */
    @JvmStatic
    fun probe(bytes: ByteArray): Array<Any?> {
        return try {
            val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
            val mime = options.outMimeType ?: return arrayOf(null, "unsupported: ")
            val orientation = try {
                ExifInterface(ByteArrayInputStream(bytes))
                    .getAttributeInt(ExifInterface.TAG_ORIENTATION, 0)
            } catch (e: Exception) {
                0
            }
            arrayOf(intArrayOf(options.outWidth, options.outHeight, orientation), mime)
        } catch (e: OutOfMemoryError) {
            arrayOf(null, "out-of-memory: ")
        } catch (e: Exception) {
            arrayOf(null, platform(e))
        }
    }

    /**
     * Called from Rust via JNI: static,
     * `(IILjava/nio/ByteBuffer;II)[Ljava/lang/Object;`: `[ByteArray, null]`
     * or `[null, error]`. [pixels] is `width * height * 4` bytes of R, G, B,
     * A, straight alpha, copied in as is. [format] is [FORMAT_JPEG] (which
     * ignores alpha) or [FORMAT_PNG] (which ignores [quality]). No EXIF.
     */
    @JvmStatic
    fun encode(width: Int, height: Int, pixels: ByteBuffer, format: Int, quality: Int): Array<Any?> {
        var bitmap: Bitmap? = null
        return try {
            val created = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888, true, SRGB)
            bitmap = created
            created.isPremultiplied = false
            created.copyPixelsFromBuffer(pixels)
            val compressFormat = when (format) {
                FORMAT_JPEG -> Bitmap.CompressFormat.JPEG
                FORMAT_PNG -> Bitmap.CompressFormat.PNG
                else -> return arrayOf(null, "platform: format $format")
            }
            val out = ByteArrayOutputStream()
            if (created.compress(compressFormat, quality, out)) {
                arrayOf(out.toByteArray(), null)
            } else {
                arrayOf(null, "platform: Bitmap.compress")
            }
        } catch (e: OutOfMemoryError) {
            arrayOf(null, "out-of-memory: ")
        } catch (e: Exception) {
            arrayOf(null, platform(e))
        } finally {
            bitmap?.recycle()
        }
    }
}
