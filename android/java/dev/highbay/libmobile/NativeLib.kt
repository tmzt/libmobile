package dev.highbay.libmobile

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.util.Log

/**
 * Registers the app's cdylib with ART so the plugins' `external fun`s resolve.
 *
 * REQUIRED, and not obviously so. `android.app.NativeActivity` brings the
 * library up with a raw classloader-namespace `dlopen` that never registers it
 * with ART's JNI library list, so the process has the `.so` mapped and running
 * `android_main` while every `Java_dev_highbay_libmobile_*` symbol still
 * resolves to "No implementation found". `System.loadLibrary` from app code
 * goes through `Runtime.nativeLoad`, which registers the library against the
 * app class loader. `dlopen` is idempotent, so nothing is loaded twice.
 *
 * The library name is the one `NativeActivity` itself loads: the activity's
 * `android.app.lib_name` meta-data, defaulting to `main` exactly as the
 * framework does — so the manifest stays the single place it is named.
 */
object NativeLib {
    const val TAG = "libmobile"
    private const val META_LIB_NAME = "android.app.lib_name"

    @Volatile
    private var loaded = false

    fun ensureLoaded(activity: Activity) {
        if (loaded) return
        load(libName(activity, activity.componentName))
    }

    /**
     * Without an activity, for a receiver in a process no activity has
     * started yet: the name comes from the launch activity's meta-data.
     */
    fun ensureLoaded(context: Context) {
        if (loaded) return
        val component = context.packageManager.getLaunchIntentForPackage(context.packageName)?.component
        load(if (component != null) libName(context, component) else "main")
    }

    @Synchronized
    private fun load(name: String) {
        if (loaded) return
        try {
            System.loadLibrary(name)
            loaded = true
        } catch (t: Throwable) {
            Log.e(TAG, "could not register lib$name.so with the app class loader", t)
        }
    }

    private fun libName(context: Context, component: ComponentName): String {
        return try {
            @Suppress("DEPRECATION")
            val info = context.packageManager.getActivityInfo(
                component,
                PackageManager.GET_META_DATA,
            )
            info.metaData?.getString(META_LIB_NAME) ?: "main"
        } catch (e: PackageManager.NameNotFoundException) {
            "main"
        }
    }
}
