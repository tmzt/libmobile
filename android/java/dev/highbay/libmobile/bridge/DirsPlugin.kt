package dev.highbay.libmobile.bridge

import android.content.Context
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * The app's private files and cache directories (`libmobile::android::dirs`).
 *
 * Rust calls [filesDir] and [cacheDir] from any thread with the Application
 * (from `ndk-context`), so no activity is needed. Each returns the absolute
 * path, or `null` if Android gives none. Android creates both directories
 * before returning them.
 *
 * An activity plugin with no hooks, listed so it is in every build like the
 * other core bridges.
 */
object DirsPlugin : MobileActivityPlugin {
    /** Called from Rust via JNI: static, `(Landroid/content/Context;)Ljava/lang/String;`. */
    @JvmStatic
    fun filesDir(context: Context): String? = context.filesDir?.absolutePath

    /** Called from Rust via JNI: static, `(Landroid/content/Context;)Ljava/lang/String;`. */
    @JvmStatic
    fun cacheDir(context: Context): String? = context.cacheDir?.absolutePath
}
