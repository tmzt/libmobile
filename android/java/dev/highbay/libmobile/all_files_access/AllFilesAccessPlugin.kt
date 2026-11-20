package dev.highbay.libmobile.all_files_access

import android.Manifest
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * All files access and the shared storage root
 * (`libmobile::android::all_files_access`).
 *
 * Rust calls the static methods from any thread, with the Application
 * (from `ndk-context`). [state] answers `"granted"` or `"missing"`,
 * [openSettings] `"app"` or `"list"`; otherwise both answer a
 * `"<kind>: <detail>"` message (`unsupported`, `not-declared`,
 * `not-visible`, `no-settings-page`, `platform`), which Rust maps to
 * `AllFilesAccessError`. Every exception is caught here, so none is left
 * pending when control returns to Rust.
 *
 * An activity plugin with no hooks, listed only under the
 * `all-files-access` feature.
 */
object AllFilesAccessPlugin : MobileActivityPlugin {
    // Answers: keep in step with `src/all_files_access.rs`.
    private const val GRANTED = "granted"
    private const val MISSING = "missing"
    private const val PAGE_APP = "app"
    private const val PAGE_LIST = "list"
    private const val UNSUPPORTED = "unsupported"
    private const val NO_SETTINGS_PAGE = "no-settings-page"

    /** Called from Rust via JNI: static (`all_files_access::state`). */
    @JvmStatic
    fun state(context: Context): String = try {
        val app = context.applicationContext
        // The SDK check is repeated here, where lint's NewApi check sees it.
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            UNSUPPORTED
        } else {
            unavailable(app) ?: if (Environment.isExternalStorageManager()) GRANTED else MISSING
        }
    } catch (e: Exception) {
        "platform: $e"
    }

    /** Called from Rust via JNI: static (`all_files_access::open_settings`). */
    @JvmStatic
    fun openSettings(context: Context): String = try {
        val app = context.applicationContext
        unavailable(app) ?: notVisible() ?: open(app)
    } catch (e: Exception) {
        "platform: $e"
    }

    /** Called from Rust via JNI: static (`all_files_access::external_storage_dir`). */
    @JvmStatic
    fun externalStorageDir(): String? = try {
        @Suppress("DEPRECATION")
        Environment.getExternalStorageDirectory()?.absolutePath
    } catch (e: Exception) {
        null
    }

    /** The app's own page, else the list of apps, else `no-settings-page`. */
    private fun open(app: Context): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return UNSUPPORTED
        val own = Intent(
            Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
            Uri.fromParts("package", app.packageName, null),
        ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            app.startActivity(own)
            return PAGE_APP
        } catch (e: ActivityNotFoundException) {
            // Some vendor builds have no per-app page: the list.
        }
        val list = Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        return try {
            app.startActivity(list)
            PAGE_LIST
        } catch (e: ActivityNotFoundException) {
            NO_SETTINGS_PAGE
        }
    }

    /** `unsupported` below API 30, `not-declared` without the permission, else null. */
    private fun unavailable(app: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return UNSUPPORTED
        val declared = try {
            @Suppress("DEPRECATION")
            app.packageManager.getPackageInfo(app.packageName, PackageManager.GET_PERMISSIONS)
                .requestedPermissions?.contains(Manifest.permission.MANAGE_EXTERNAL_STORAGE) == true
        } catch (e: PackageManager.NameNotFoundException) {
            false
        }
        return if (declared) null else "not-declared: ${Manifest.permission.MANAGE_EXTERNAL_STORAGE}"
    }

    /**
     * `not-visible` unless the app is in the foreground or visible behind a
     * dialog, as `BatteryPlugin` checks: a hidden app may not start activities.
     */
    private fun notVisible(): String? {
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND &&
            state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
        ) {
            return "not-visible: importance ${state.importance}"
        }
        return null
    }
}
