package dev.highbay.libmobile

import android.app.NativeActivity
import android.content.Intent
import android.os.Bundle

/**
 * A [NativeActivity] that dispatches its lifecycle to [plugins].
 *
 * Rust still owns the window, the event loop and every pixel: `android_main`
 * drives off the native `ANativeActivity`, not this class, so subclassing
 * changes nothing there. What this class adds is the callbacks stock
 * `NativeActivity` never forwards to native code — `onNewIntent` (an intent
 * aimed at a running instance is otherwise swallowed), window insets, IME
 * text, activity and permission results — each handled by a [MobileActivityPlugin].
 *
 * Use it directly as the manifest's activity, or subclass it to add plugins:
 * ```
 * class MyActivity : MobileNativeActivity() {
 *     override val plugins = GeneratedPlugins.activePlugins + MyPlugin
 * }
 * ```
 * Declare it `android:launchMode="singleTop"` so a later intent is redelivered
 * through [onNewIntent] instead of stacking a second instance.
 */
open class MobileNativeActivity : NativeActivity() {
    /** The plugins this activity dispatches to, in order. */
    protected open val plugins: List<MobileActivityPlugin>
        get() = GeneratedPlugins.activePlugins

    override fun onCreate(savedInstanceState: Bundle?) {
        // `super` first: `NativeActivity.onCreate` builds the window and
        // starts the native thread.
        super.onCreate(savedInstanceState)
        NativeLib.ensureLoaded(this)
        plugins.forEach { it.onCreate(this, savedInstanceState) }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        plugins.forEach { it.onWindowAttached(this, window) }
    }

    override fun onResume() {
        super.onResume()
        plugins.forEach { it.onResume(this) }
    }

    override fun onPause() {
        plugins.forEach { it.onPause(this) }
        super.onPause()
    }

    override fun onDestroy() {
        plugins.forEach { it.onDestroy(this) }
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // So a later `getIntent()` (and every plugin) sees the newest one.
        setIntent(intent)
        plugins.forEach { it.onNewIntent(this, intent) }
    }

    @Deprecated("Deprecated in Java")
    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        plugins.firstOrNull { it.onActivityResult(this, requestCode, resultCode, data) }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        plugins.firstOrNull { it.onRequestPermissionsResult(this, requestCode, permissions, grantResults) }
    }
}
