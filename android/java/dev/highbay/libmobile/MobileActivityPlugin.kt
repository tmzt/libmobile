package dev.highbay.libmobile

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.Window

/**
 * A lifecycle participant of [MobileNativeActivity].
 *
 * Every hook has an empty default, so a plugin overrides only what it needs.
 * Plugins are Kotlin `object`s listed in [GeneratedPlugins.activePlugins],
 * which `build.rs` writes from the active Cargo features — there is no runtime
 * discovery. Hooks run on the UI thread, in list order; a plugin that reports
 * to Rust does so through its own `external fun`, and the Rust side only
 * queues the report for the `android_main` loop.
 */
interface MobileActivityPlugin {
    /** After `NativeActivity.onCreate` (the native thread is running) and after the native library is registered. */
    fun onCreate(activity: Activity, savedInstanceState: Bundle?) {}

    fun onResume(activity: Activity) {}

    fun onPause(activity: Activity) {}

    fun onDestroy(activity: Activity) {}

    /** After `setIntent(intent)`, so `activity.intent` is already the new one. */
    fun onNewIntent(activity: Activity, intent: Intent) {}

    /** The decor view is attached; the place to install view-level listeners. */
    fun onWindowAttached(activity: Activity, window: Window) {}

    /**
     * Return `true` if this plugin started the request and consumed the
     * result; dispatch stops at the first plugin that does.
     */
    fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?): Boolean = false
}
