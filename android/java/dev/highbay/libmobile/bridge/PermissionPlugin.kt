package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.pm.PackageManager
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Runtime permission requests. Rust asks with [request]
 * (`libmobile::android::request_permissions`), checks with [granted]
 * (`permission_granted`) and [shouldShowRationale] (`should_show_rationale`);
 * the system dialog's answer goes back to Rust (`src/android/permission.rs`)
 * and arrives as `MobileEvent::Permissions`, carrying the request's id.
 *
 * Each permission must also be in the manifest, or Android denies it without
 * a dialog. Android shows one dialog at a time and answers a request made
 * while another waits as interrupted, so requests are queued here and made
 * one after another. Every request gets one answer, in the order made; an
 * interrupted one (an empty result) is reported by Rust as every requested
 * name denied.
 */
object PermissionPlugin : MobileActivityPlugin {
    /** Arbitrary; distinguishes our result from any other plugin's. */
    private const val REQUEST_CODE = 0x504d // "PM"

    private class Pending(val id: Int, val names: Array<String>)

    /** Requests not yet answered; the head is in flight when [inFlight]. UI thread only. */
    private val queue = ArrayDeque<Pending>()
    private var inFlight = false

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;I[Ljava/lang/String;)V`. */
    @JvmStatic
    fun request(activity: Activity, id: Int, permissions: Array<String>) {
        activity.runOnUiThread {
            queue.addLast(Pending(id, permissions))
            if (!inFlight) startNext(activity)
        }
    }

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`. */
    @JvmStatic
    fun granted(activity: Activity, permission: String): Boolean =
        activity.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`. */
    @JvmStatic
    fun shouldShowRationale(activity: Activity, permission: String): Boolean =
        activity.shouldShowRequestPermissionRationale(permission)

    private fun startNext(activity: Activity) {
        val next = queue.firstOrNull() ?: return
        inFlight = true
        activity.requestPermissions(next.names, REQUEST_CODE)
    }

    override fun onRequestPermissionsResult(
        activity: Activity,
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ): Boolean {
        if (requestCode != REQUEST_CODE) return false
        val head = queue.removeFirstOrNull()
        inFlight = false
        val names = Array(permissions.size) { permissions[it] }
        val granted = BooleanArray(grantResults.size) {
            grantResults[it] == PackageManager.PERMISSION_GRANTED
        }
        try {
            nativePermissionsResult(head?.id ?: 0, head?.names ?: emptyArray(), names, granted)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "permissions: native callback unavailable", e)
        }
        startNext(activity)
        return true
    }

    override fun onDestroy(activity: Activity) {
        // Requests belong to this run; the Rust side drops their answers too.
        queue.clear()
        inFlight = false
    }

    /**
     * Implemented in Rust (`src/android/permission.rs`) as
     * `Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult`.
     */
    private external fun nativePermissionsResult(
        id: Int,
        requested: Array<String>,
        permissions: Array<String>,
        granted: BooleanArray,
    )
}
