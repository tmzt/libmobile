package dev.highbay.libmobile.disk_space

import android.app.Activity
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.storage.StorageManager
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.io.File

/**
 * Free space, the cache quota, and periodic readings
 * (`libmobile::android::disk_space`).
 *
 * [space] answers `[allocatable, cacheQuota]` from `StorageManager` for the
 * volume holding a path, each `-1` when its call threw; it catches every
 * exception itself. No permission and no manifest entry.
 *
 * The timer: after Rust's `watch` calls [setInterval], a `Handler` on the
 * main looper calls [nativeDiskCheck] (a `statvfs` reading in Rust) once
 * when the activity resumes and every interval after that, only while it
 * is resumed. `onPause` removes the callback, so a paused app has no
 * timer. Without `watch` the timer is never posted. All timer state is
 * touched on the main looper only.
 */
object DiskSpacePlugin : MobileActivityPlugin {
    private val handler = Handler(Looper.getMainLooper())

    /** Set by [setInterval], cleared by [stop]; main looper only. */
    private var intervalMs = 0L
    private var resumed = false

    private val tick = object : Runnable {
        override fun run() {
            if (!resumed || intervalMs <= 0L) return
            try {
                nativeDiskCheck()
            } catch (t: Throwable) {
                Log.e(NativeLib.TAG, "disk check failed", t)
            }
            handler.postDelayed(this, intervalMs)
        }
    }

    override fun onResume(activity: Activity) {
        resumed = true
        restart()
    }

    override fun onPause(activity: Activity) {
        resumed = false
        handler.removeCallbacks(tick)
    }

    /** Called from Rust via JNI: static (`disk_space::watch`); any thread. */
    @JvmStatic
    fun setInterval(ms: Long) {
        handler.post {
            intervalMs = ms
            restart()
        }
    }

    /** Called from Rust via JNI: static (`disk_space::unwatch`); any thread. */
    @JvmStatic
    fun stop() {
        handler.post {
            intervalMs = 0L
            handler.removeCallbacks(tick)
        }
    }

    /** Called from Rust via JNI: static (`disk_space::space`). */
    @JvmStatic
    fun space(context: Context, path: String): LongArray {
        val out = longArrayOf(-1L, -1L)
        try {
            val storage = context.applicationContext.getSystemService(StorageManager::class.java)
            val uuid = storage.getUuidForPath(File(path))
            out[0] = try { storage.getAllocatableBytes(uuid) } catch (e: Exception) { -1L }
            out[1] = try { storage.getCacheQuotaBytes(uuid) } catch (e: Exception) { -1L }
        } catch (e: Exception) {
            Log.d(NativeLib.TAG, "disk space: StorageManager for $path: $e")
        }
        return out
    }

    /** One reading now, then every interval, while resumed and watching. */
    private fun restart() {
        handler.removeCallbacks(tick)
        if (resumed && intervalMs > 0L) handler.post(tick)
    }

    private external fun nativeDiskCheck()
}
