package dev.highbay.libmobile.service

import android.app.Service
import android.content.Intent
import android.os.IBinder

/**
 * The foreground service (`libmobile::android::service`). It holds no state
 * of its own: [ForegroundServicePlugin] keeps the options, the notification
 * and the wake lock, and the service only carries out what they say.
 *
 * Declared by `gradle::Project::foreground_service`, not exported. Every
 * notification button arrives here as an intent action. It returns
 * `START_NOT_STICKY`, so Android never restarts it after the process dies.
 */
class MobileForegroundService : Service() {
    /** The [ForegroundServicePlugin.generation] this instance went foreground for. */
    internal var generation = 0

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val plugin = ForegroundServicePlugin
        when (intent?.action) {
            plugin.ACTION_START -> plugin.goForeground(this)
            plugin.ACTION_EXIT -> plugin.exit(this)
            plugin.ACTION_WAKE_LOCK -> plugin.toggleWakeLock(this)
            plugin.ACTION_APP -> plugin.appAction(this, intent.getStringExtra(plugin.EXTRA_ACTION_ID))
            plugin.ACTION_DELETED -> plugin.dismissed(this)
            // Not ours: nothing to start.
            else -> if (!plugin.running) stopSelf(startId)
        }
        return START_NOT_STICKY
    }

    /** API 35: a type's time limit (`dataSync`) was reached. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        ForegroundServicePlugin.timedOut(this)
    }

    /** Called only with `stopWithTask="false"` (`foreground_service_keeps_running`). */
    override fun onTaskRemoved(rootIntent: Intent?) {
        ForegroundServicePlugin.event(ForegroundServicePlugin.EVENT_TASK_REMOVED, null)
    }

    override fun onDestroy() {
        ForegroundServicePlugin.destroyed(this)
        super.onDestroy()
    }
}
