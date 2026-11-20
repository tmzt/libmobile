package dev.highbay.libmobile.media_scan

import android.content.Context
import android.media.MediaScannerConnection
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Telling MediaStore about changed files (`libmobile::android::media_scan`).
 *
 * Rust calls [scan] from any thread, with the Application. It returns once
 * the request is queued: null on success, `"platform: <message>"` if
 * queuing threw. The scan's result per path is only logged at debug.
 * No permission. An activity plugin with no hooks, listed only under the
 * `media-scan` feature.
 */
object MediaScanPlugin : MobileActivityPlugin {
    /** Called from Rust via JNI: static (`media_scan::scan_paths`). */
    @JvmStatic
    fun scan(context: Context, paths: Array<String>): String? = try {
        MediaScannerConnection.scanFile(context.applicationContext, paths, null) { path, uri ->
            Log.d(NativeLib.TAG, "media scan: $path -> $uri")
        }
        null
    } catch (e: Exception) {
        "platform: $e"
    }
}
