package dev.highbay.libmobile.cameraintent

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.provider.MediaStore
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.io.ByteArrayOutputStream

/**
 * Photo capture through the system camera app (Cargo feature
 * `camera-intent`; named `camera` before libmobile 0.3.0).
 *
 * Rust (`libmobile::android::capture_photo`) calls [capture]; the camera app
 * returns through [onActivityResult], and the preview bitmap goes back to
 * Rust JPEG-encoded, arriving as `MobileEvent::Capture`. No CAMERA permission
 * is needed — unless the app's manifest declares it (the in-process `camera`
 * feature does), in which case Android requires it to be granted before
 * `ACTION_IMAGE_CAPTURE` will launch; Rust asks for it first then.
 *
 * Every path produces an outcome, including a cancelled camera.
 */
object CameraIntentPlugin : MobileActivityPlugin {
    /** Arbitrary; distinguishes our result from any other plugin's. */
    private const val REQUEST_CODE = 0x4c4d // "LM"
    private const val JPEG_QUALITY = 90

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;)V`. */
    @JvmStatic
    fun capture(activity: Activity) {
        activity.runOnUiThread {
            try {
                @Suppress("DEPRECATION")
                activity.startActivityForResult(Intent(MediaStore.ACTION_IMAGE_CAPTURE), REQUEST_CODE)
            } catch (e: ActivityNotFoundException) {
                deliver(null, "no_camera_app")
            } catch (e: SecurityException) {
                deliver(null, "permission_denied: ${e.message}")
            }
        }
    }

    override fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CODE) return false
        if (resultCode != Activity.RESULT_OK) {
            deliver(null, "cancelled")
            return true
        }
        val bitmap = data?.extras?.let { extras ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                extras.getParcelable("data", Bitmap::class.java)
            } else {
                @Suppress("DEPRECATION")
                extras.getParcelable("data") as? Bitmap
            }
        }
        if (bitmap == null) {
            deliver(null, "no_image")
            return true
        }
        val out = ByteArrayOutputStream()
        if (bitmap.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, out)) {
            deliver(out.toByteArray(), null)
        } else {
            deliver(null, "encode_failed")
        }
        return true
    }

    /** Exactly one of [jpeg] / [error] is non-null. */
    private fun deliver(jpeg: ByteArray?, error: String?) {
        try {
            nativeCaptureResult(jpeg, error)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "camera intent: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/camera_intent.rs`) as
     * `Java_dev_highbay_libmobile_cameraintent_CameraIntentPlugin_nativeCaptureResult`.
     */
    private external fun nativeCaptureResult(jpeg: ByteArray?, error: String?)
}
