package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.pm.PackageManager
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Runtime permission requests. Rust asks with [request]
 * (`libmobile::android::request_permissions`) and checks with [granted]
 * (`permission_granted`); the system dialog's answer goes back to Rust
 * (`src/android/permission.rs`) and arrives as `MobileEvent::Permissions`.
 *
 * Each permission must also be in the manifest, or Android denies it without
 * a dialog. Every request gets one answer, including an interrupted one
 * (an empty result), which Rust reports as every requested name denied.
 */
object PermissionPlugin : MobileActivityPlugin {
    /** Arbitrary; distinguishes our result from any other plugin's. */
    private const val REQUEST_CODE = 0x504d // "PM"

    /** The names of the request in flight, for an interrupted result. */
    private var requested: Array<String> = emptyArray()

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;[Ljava/lang/String;)V`. */
    @JvmStatic
    fun request(activity: Activity, permissions: Array<String>) {
        activity.runOnUiThread {
            requested = permissions
            activity.requestPermissions(permissions, REQUEST_CODE)
        }
    }

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`. */
    @JvmStatic
    fun granted(activity: Activity, permission: String): Boolean =
        activity.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    override fun onRequestPermissionsResult(
        activity: Activity,
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ): Boolean {
        if (requestCode != REQUEST_CODE) return false
        val names = Array(permissions.size) { permissions[it] }
        val granted = BooleanArray(grantResults.size) {
            grantResults[it] == PackageManager.PERMISSION_GRANTED
        }
        try {
            nativePermissionsResult(requested, names, granted)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "permissions: native callback unavailable", e)
        }
        requested = emptyArray()
        return true
    }

    /**
     * Implemented in Rust (`src/android/permission.rs`) as
     * `Java_dev_highbay_libmobile_bridge_PermissionPlugin_nativePermissionsResult`.
     */
    private external fun nativePermissionsResult(
        requested: Array<String>,
        permissions: Array<String>,
        granted: BooleanArray,
    )
}
