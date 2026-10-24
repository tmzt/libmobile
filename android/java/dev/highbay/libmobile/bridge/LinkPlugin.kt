package dev.highbay.libmobile.bridge

import android.app.Activity
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Looper
import dev.highbay.libmobile.MobileActivityPlugin
import java.lang.ref.WeakReference

/**
 * Opens a link in the app that handles it (`libmobile::android::link`).
 *
 * Rust checks the URL first (http or https only), then calls [open] from
 * any thread with the Application (from `ndk-context`). It returns `null`
 * once the intent is sent, or a `"<kind>: <detail>"` message
 * (`no-handler`, `not-visible`, `platform`), which Rust maps to
 * `OpenLinkError`. It does not wait for the browser.
 *
 * As an activity plugin it keeps a weak reference to the activity, so a
 * call on the UI thread starts the link from it. A call from another thread
 * starts it from the Application, in a new task.
 */
object LinkPlugin : MobileActivityPlugin {
    private var activity: WeakReference<Activity>? = null

    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        this.activity = WeakReference(activity)
    }

    override fun onDestroy(activity: Activity) {
        if (this.activity?.get() === activity) this.activity = null
    }

    /** Called from Rust via JNI: static, `(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;`. */
    @JvmStatic
    fun open(context: Context, url: String): String? {
        // Android 10+ refuses, silently, an activity start from an app that
        // is not visible; a hidden app with a foreground service is
        // IMPORTANCE_FOREGROUND_SERVICE and is refused too.
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND &&
            state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
        ) {
            return "not-visible: importance ${state.importance}"
        }
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
        // From the activity on its UI thread; from any other thread, from
        // the Application in a new task. Not by waiting for the UI thread:
        // while the activity starts or changes state, the UI thread waits
        // for `android_main`, which may be the caller.
        val current = activity?.get()?.takeIf { !it.isFinishing }
        if (current != null && Looper.myLooper() == Looper.getMainLooper()) return start(current, intent)
        return start(context.applicationContext, intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun start(context: Context, intent: Intent): String? = try {
        context.startActivity(intent)
        null
    } catch (e: ActivityNotFoundException) {
        // Not the exception's message: it can hold the URL.
        "no-handler: nothing handles the link"
    } catch (e: Exception) {
        "platform: ${e.javaClass.name}"
    }
}
