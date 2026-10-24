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
 *
 * A tap on a notification libmobile posted (the foreground service's, or
 * an alert) carries the extras below instead; it reaches Rust through
 * [nativeNotificationTap] as `ServiceEvent::Tapped` or a `NotifyEvent`, not
 * as an intent. Keep the names in step with `crate::intent::tap_extras`.
 */
object IntentPlugin : MobileActivityPlugin {
    private const val EXTRA_NOTIFICATION = "libmobile.notification"
    private const val EXTRA_KIND = "libmobile.kind"
    private const val EXTRA_TAG = "libmobile.tag"
    private const val EXTRA_ACTION = "libmobile.action"

    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        activity.intent?.let { deliver(it, isNew = false, restored = savedInstanceState != null) }
    }

    override fun onNewIntent(activity: Activity, intent: Intent) {
        deliver(intent, isNew = true, restored = false)
    }

    private fun deliver(intent: Intent, isNew: Boolean, restored: Boolean) {
        if (intent.hasExtra(EXTRA_NOTIFICATION)) {
            // A recreated activity gets the same intent again: the tap was
            // delivered the first time.
            if (!restored) deliverTap(intent)
            return
        }
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

    private fun deliverTap(intent: Intent) {
        val id = intent.getStringExtra(EXTRA_NOTIFICATION) ?: ""
        val kind = intent.getStringExtra(EXTRA_KIND)
        val tag = intent.getStringExtra(EXTRA_TAG)
        val action = intent.getStringExtra(EXTRA_ACTION)
        try {
            nativeNotificationTap(kind, id, tag, action)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "intent: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/intent.rs`) as
     * `Java_dev_highbay_libmobile_bridge_IntentPlugin_nativeNotificationTap`.
     */
    private external fun nativeNotificationTap(kind: String?, id: String, tag: String?, action: String?)

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
