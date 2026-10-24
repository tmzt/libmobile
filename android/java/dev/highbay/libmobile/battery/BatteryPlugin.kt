package dev.highbay.libmobile.battery

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.lang.ref.WeakReference

/**
 * Battery optimization and app sleeping (`libmobile::android::battery`).
 *
 * Rust calls the static methods from any thread, with the Application
 * (from `ndk-context`). Those that can fail return `null` (or, for the
 * vendor page, which page opened) on success, or a `"<kind>: <detail>"`
 * message (`not-declared`, `not-visible`, `platform`), which Rust maps to
 * `BatteryError`.
 *
 * As an activity plugin it keeps a weak reference to the activity, so a
 * page opens on top of it rather than in a task of its own.
 */
object BatteryPlugin : MobileActivityPlugin {
    /**
     * Samsung's Device Care battery pages, tried in order. Undocumented and
     * moved between One UI versions; the first that resolves and is
     * exported opens. The package is listed in the manifest's `<queries>`
     * (codegen), or Android 11+ hides it.
     */
    private const val SAMSUNG_PACKAGE = "com.samsung.android.lool"
    private val SAMSUNG_PAGES = arrayOf(
        "com.samsung.android.sm.battery.ui.BatteryActivity",
        "com.samsung.android.sm.ui.battery.BatteryActivity",
    )

    // Results of openVendorBattery: keep in step with `src/android/battery.rs`.
    private const val PAGE_VENDOR = "vendor"
    private const val PAGE_APP_DETAILS = "app-details"

    private var activity: WeakReference<Activity>? = null

    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        this.activity = WeakReference(activity)
    }

    override fun onDestroy(activity: Activity) {
        if (this.activity?.get() === activity) this.activity = null
    }

    /** Called from Rust via JNI: static. Bit 0 ignoring optimizations, bit 1 background restricted. */
    @JvmStatic
    fun stateFlags(context: Context): Int {
        val app = context.applicationContext
        val power = app.getSystemService(PowerManager::class.java)
        val activities = app.getSystemService(ActivityManager::class.java)
        var flags = 0
        if (power.isIgnoringBatteryOptimizations(app.packageName)) flags = flags or 1
        if (activities.isBackgroundRestricted) flags = flags or 2
        return flags
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun manufacturer(): String = Build.MANUFACTURER ?: ""

    /** Called from Rust via JNI: static (`battery::request_ignore_optimizations`). */
    @JvmStatic
    fun requestIgnoreOptimizations(context: Context): String? {
        val app = context.applicationContext
        val declared = try {
            @Suppress("DEPRECATION")
            app.packageManager.getPackageInfo(app.packageName, PackageManager.GET_PERMISSIONS)
                .requestedPermissions?.contains(Manifest.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS) == true
        } catch (e: PackageManager.NameNotFoundException) {
            false
        }
        if (!declared) return "not-declared: ${Manifest.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS}"
        if (app.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(app.packageName)) {
            return null
        }
        @Suppress("BatteryLife")
        val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${app.packageName}"))
        return startVisible(app, intent)
    }

    /** Called from Rust via JNI: static (`battery::open_settings`). */
    @JvmStatic
    fun openSettings(context: Context): String? =
        startVisible(context.applicationContext, Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))

    /** Called from Rust via JNI: static (`battery::open_app_details`). */
    @JvmStatic
    fun openAppDetails(context: Context): String? {
        val app = context.applicationContext
        return startVisible(app, appDetails(app))
    }

    /** Called from Rust via JNI: static (`battery::open_vendor_battery`): the page that opened, or an error. */
    @JvmStatic
    fun openVendorBattery(context: Context): String {
        val app = context.applicationContext
        if (Build.MANUFACTURER.equals("samsung", ignoreCase = true)) {
            for (name in SAMSUNG_PAGES) {
                val intent = Intent().setComponent(ComponentName(SAMSUNG_PACKAGE, name))
                @Suppress("DEPRECATION")
                val info = app.packageManager.resolveActivity(intent, 0)?.activityInfo ?: continue
                if (!info.exported) continue
                return startVisible(app, intent) ?: PAGE_VENDOR
            }
        }
        return startVisible(app, appDetails(app)) ?: PAGE_APP_DETAILS
    }

    private fun appDetails(context: Context): Intent =
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))

    /**
     * Start a system page, while the app is visible only: in the foreground
     * or visible behind a dialog. A hidden app with a foreground service is
     * `IMPORTANCE_FOREGROUND_SERVICE`, which Android does not let start
     * activities, so it is refused too.
     */
    private fun startVisible(context: Context, intent: Intent): String? {
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND &&
            state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
        ) {
            return "not-visible: importance ${state.importance}"
        }
        return try {
            val current = activity?.get()
            if (current != null && !current.isFinishing) {
                current.runOnUiThread {
                    try {
                        current.startActivity(intent)
                    } catch (e: Exception) {
                        Log.e(NativeLib.TAG, "battery: starting $intent failed", e)
                    }
                }
            } else {
                context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
            null
        } catch (e: Exception) {
            "platform: $e"
        }
    }
}
