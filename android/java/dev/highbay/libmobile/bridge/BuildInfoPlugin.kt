package dev.highbay.libmobile.bridge

import android.os.Build
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * The device's manufacturer, brand and model (`libmobile::android::build_info`).
 *
 * Rust calls [info] from any thread. Only these three public `Build`
 * constants, which need no permission: never the serial, `ANDROID_ID`, the
 * IMEI, the advertising ID, the fingerprint, or anything else that
 * identifies the unit or its owner. Adding a value is a plan amendment.
 *
 * An activity plugin with no hooks, listed so it is in every build like the
 * other core bridges.
 */
object BuildInfoPlugin : MobileActivityPlugin {
    /** Called from Rust via JNI: static, `()[Ljava/lang/String;`: manufacturer, brand, model. */
    @JvmStatic
    fun info(): Array<String> = arrayOf(
        Build.MANUFACTURER ?: "",
        Build.BRAND ?: "",
        Build.MODEL ?: "",
    )
}
