package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.ComponentCallbacks2
import android.content.res.Configuration
import android.os.Bundle
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Android's memory-trim levels (`MobileEvent::TrimMemory`).
 *
 * At the first activity's `onCreate` it registers [ComponentCallbacks2] on
 * the Application, once per process, and reports each `onTrimMemory` level
 * to Rust, which queues it for the main loop (and, under `system-events`,
 * hands it to the subscribers). Rust maps the number to a level.
 */
object TrimMemoryPlugin : MobileActivityPlugin {
    /** The callbacks are registered. Guarded by `this`. */
    private var registered = false

    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        synchronized(this) {
            if (registered) return
            registered = true
        }
        activity.application.registerComponentCallbacks(Callbacks)
    }

    private object Callbacks : ComponentCallbacks2 {
        override fun onTrimMemory(level: Int) {
            try {
                nativeTrimMemory(level)
            } catch (e: UnsatisfiedLinkError) {
                Log.e(NativeLib.TAG, "trim memory: native callback unavailable", e)
            }
        }

        override fun onConfigurationChanged(newConfig: Configuration) {}

        // `MobileEvent::LowMemory` comes from NativeActivity's own callback.
        @Deprecated("Deprecated in Java")
        override fun onLowMemory() {}
    }

    /** Implemented in Rust (`src/android/trim_memory.rs`). */
    private external fun nativeTrimMemory(level: Int)
}
