package dev.highbay.libmobile.bridge

import android.app.Activity
import android.graphics.Rect
import android.os.Build
import android.util.Log
import android.view.View
import android.view.Window
import android.view.WindowInsets
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Platform safe-area bridge.
 *
 * Rust owns every pixel on the surface, and the surface is the WHOLE panel:
 * this plugin turns `decorFitsSystemWindows` off on every supported API level
 * (SDK 35 does it by default), so the display cutout and the system bars are
 * drawn by the OS ON TOP of the app's pixels and the app has to reserve those
 * edges itself. This plugin measures the reservation and hands it to Rust
 * (`src/android/insets.rs`); it makes no layout decision of its own.
 *
 * WHAT IS REPORTED: the union of `systemBars()` and `displayCutout()`, in
 * physical pixels (the space the native surface size arrives in), plus the
 * IME's occlusion above that union, kept separate: a soft keyboard is a
 * transient overlay a focused field scrolls for, not a permanent reservation.
 *
 * WHY A LISTENER: insets change while the process runs — rotation (which
 * `android:configChanges` may keep in the same activity), a gesture/3-button
 * navigation switch, the IME. A boot-time read would be wrong for the rest of
 * the session.
 */
object WindowInsetsPlugin : MobileActivityPlugin {
    override fun onWindowAttached(activity: Activity, window: Window) {
        // Edge-to-edge on EVERY supported API level, not only where the
        // platform does it by default. Otherwise the reservation would have to
        // be conditional too: with `decorFitsSystemWindows` on, the framework
        // already shrinks the content view, and applying these insets on top
        // would inset twice.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // Deprecated at API 36, where edge-to-edge is simply the behaviour;
            // still the only way to get it on API 30..34.
            @Suppress("DEPRECATION")
            window.setDecorFitsSystemWindows(false)
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                    View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                    View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
        }

        val decor = window.decorView
        decor.setOnApplyWindowInsetsListener { view, insets ->
            deliver(insets)
            // `setOnApplyWindowInsetsListener` TAKES OVER the view's inset
            // policy; handing the insets back to the view's own implementation
            // observes without changing DecorView's dispatch to its children.
            view.onApplyWindowInsets(insets)
        }
        // Ask for a pass now so the first frame Rust draws is not a frame late.
        decor.requestApplyInsets()
    }

    private fun deliver(insets: WindowInsets) {
        val reserved = union(insets)
        val imeBottom =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                insets.getInsets(WindowInsets.Type.ime()).bottom
            } else {
                @Suppress("DEPRECATION")
                insets.systemWindowInsetBottom
            }
        val imeOcclusion = maxOf(0, imeBottom - reserved.bottom)
        try {
            nativeWindowInsets(reserved.left, reserved.top, reserved.right, reserved.bottom, imeOcclusion)
        } catch (e: UnsatisfiedLinkError) {
            // Only if NativeLib could not register the library; log rather
            // than crash from a layout callback.
            Log.e(NativeLib.TAG, "window insets: native callback unavailable", e)
        }
    }

    /** System bars and the display cutout, unioned, in physical pixels. */
    private fun union(insets: WindowInsets): Rect {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val both = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            return Rect(both.left, both.top, both.right, both.bottom)
        }
        // API 29: no `WindowInsets.Type`; `systemWindowInsets` is the pre-R
        // spelling of the system bars, unioned with the cutout by hand.
        @Suppress("DEPRECATION")
        val reserved = Rect(
            insets.systemWindowInsetLeft,
            insets.systemWindowInsetTop,
            insets.systemWindowInsetRight,
            insets.systemWindowInsetBottom,
        )
        val cutout = insets.displayCutout
        if (cutout != null) {
            reserved.left = maxOf(reserved.left, cutout.safeInsetLeft)
            reserved.top = maxOf(reserved.top, cutout.safeInsetTop)
            reserved.right = maxOf(reserved.right, cutout.safeInsetRight)
            reserved.bottom = maxOf(reserved.bottom, cutout.safeInsetBottom)
        }
        return reserved
    }

    /**
     * Implemented in Rust (`src/android/insets.rs`) as
     * `Java_dev_highbay_libmobile_bridge_WindowInsetsPlugin_nativeWindowInsets`.
     * An instance method of this `object` (no `@JvmStatic`), so the JNI symbol
     * shape is unambiguous.
     */
    private external fun nativeWindowInsets(left: Int, top: Int, right: Int, bottom: Int, imeBottom: Int)
}
