package dev.highbay.libmobile.camera

import android.app.Activity
import android.graphics.Point
import android.os.Build
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * The in-process camera (Cargo feature `camera`).
 *
 * The camera itself is the NDK's (`ACamera*`, `AImageReader`), driven from
 * Rust (`src/android/camera/`); no Jetpack, no Android view. This plugin
 * only answers what the NDK cannot: the display's rotation, for upright
 * frames, and its size, for the preview stream's aspect. CAMERA is asked for
 * through the core `bridge.PermissionPlugin`, on the first camera call.
 */
object CameraPlugin : MobileActivityPlugin {
    /**
     * Called from Rust via JNI (any thread): static,
     * `(Landroid/app/Activity;)[I`. `[rotation in degrees (0, 90, 180, 270),
     * width px, height px]` of the display the activity is on.
     */
    @JvmStatic
    fun displayInfo(activity: Activity): IntArray {
        val rotation: Int
        val width: Int
        val height: Int
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            rotation = activity.display?.rotation ?: 0
            val bounds = activity.windowManager.currentWindowMetrics.bounds
            width = bounds.width()
            height = bounds.height()
        } else {
            @Suppress("DEPRECATION")
            val display = activity.windowManager.defaultDisplay
            rotation = display.rotation
            val size = Point()
            @Suppress("DEPRECATION")
            display.getRealSize(size)
            width = size.x
            height = size.y
        }
        return intArrayOf(rotation * 90, width, height)
    }
}
