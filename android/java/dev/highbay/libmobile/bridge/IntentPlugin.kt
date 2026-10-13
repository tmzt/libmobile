package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Intent bridge: the launch intent (from [onCreate]) and every later one
 * (from [onNewIntent], which stock `NativeActivity` swallows) reach Rust as
 * `MobileEvent::Intent` (`src/android/intent.rs`). Rust decides what any of
 * it means — `adb shell am start -n <pkg>/<activity> --es cmd refresh`
 * arrives with extra `cmd = "refresh"`.
 */
object IntentPlugin : MobileActivityPlugin {
    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        activity.intent?.let { deliver(it, isNew = false) }
    }

    override fun onNewIntent(activity: Activity, intent: Intent) {
        deliver(intent, isNew = true)
    }

    private fun deliver(intent: Intent, isNew: Boolean) {
        val extras = intent.extras
        val keys = extras?.keySet()?.toTypedArray() ?: emptyArray()
        // Every extra stringified, so `--ei` and `--ez` arrive too.
        @Suppress("DEPRECATION")
        val values = Array(keys.size) { i -> extras?.get(keys[i])?.toString() ?: "" }
        try {
            nativeIntent(intent.action, intent.dataString, keys, values, isNew)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "intent: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/intent.rs`) as
     * `Java_dev_highbay_libmobile_bridge_IntentPlugin_nativeIntent`.
     */
    private external fun nativeIntent(
        action: String?,
        data: String?,
        keys: Array<String>,
        values: Array<String>,
        isNew: Boolean,
    )
}
