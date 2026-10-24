package dev.highbay.libmobile.service

import android.Manifest
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.lang.ref.WeakReference

/**
 * The foreground service's state and its calls from Rust
 * (`libmobile::android::service`).
 *
 * Rust calls the static methods from any thread, with the Application
 * (from `ndk-context`). Those that can fail return `null` on success or a
 * `"<kind>: <detail>"` message (`not-declared`, `start-not-allowed`,
 * `missing-permission`, `not-running`, `platform`), which Rust maps to
 * `ServiceError`. What the service does goes back to Rust through
 * [nativeServiceEvent].
 *
 * As an activity plugin it keeps a weak reference to the activity, which
 * Exit finishes, and delivers the taps on buttons that open the app, which
 * arrive as an activity intent.
 */
object ForegroundServicePlugin : MobileActivityPlugin {
    const val CHANNEL = "libmobile.foreground"
    const val NOTIFICATION_ID = 1
    const val WAKE_LOCK_TAG = "libmobile:foreground"

    const val ACTION_START = "dev.highbay.libmobile.service.START"
    const val ACTION_EXIT = "dev.highbay.libmobile.service.EXIT"
    const val ACTION_WAKE_LOCK = "dev.highbay.libmobile.service.WAKE_LOCK"
    const val ACTION_APP = "dev.highbay.libmobile.service.APP"
    const val ACTION_DELETED = "dev.highbay.libmobile.service.DELETED"
    const val EXTRA_ACTION_ID = "dev.highbay.libmobile.service.ACTION_ID"

    // Event codes: keep in step with `ServiceEvent::from_code`.
    const val EVENT_STARTED = 0
    const val EVENT_STOPPED = 1
    const val EVENT_FAILED = 2
    const val EVENT_TIMED_OUT = 3
    const val EVENT_NOTIFICATION_HIDDEN = 4
    const val EVENT_ACTION = 5
    const val EVENT_EXITED = 6
    const val EVENT_WAKE_LOCK_HELD = 7
    const val EVENT_WAKE_LOCK_RELEASED = 8
    const val EVENT_TASK_REMOVED = 9

    private const val REQUEST_OPEN = 0
    private const val REQUEST_EXIT = 1
    private const val REQUEST_WAKE_LOCK = 2
    private const val REQUEST_DELETED = 3
    private const val REQUEST_APP = 10

    private class Content(
        val title: String,
        val text: String,
        val ids: Array<String>,
        val labels: Array<String>,
        val opens: Int,
        val visibility: Int,
    )

    /** Exit, acquire, release, held suffix. */
    private class Controls(
        val types: Int,
        val ongoing: Boolean,
        val exit: Boolean,
        val wakeLockToggle: Boolean,
        val labels: Array<String>,
    )

    /** Guards everything below. */
    private val lock = Any()
    private var content = Content("", "", emptyArray(), emptyArray(), 0, Notification.VISIBILITY_PRIVATE)
    private var controls = Controls(0, false, false, false, arrayOf("", "", "", ""))
    private var wakeLock: PowerManager.WakeLock? = null

    /** Started and not stopped since. */
    @Volatile
    var running = false
        private set

    /** Counts starts, so a late `onDestroy` of an earlier instance changes nothing. */
    private var generation = 0

    private var activity: WeakReference<Activity>? = null

    // ── activity plugin ────────────────────────────────────────────────

    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        this.activity = WeakReference(activity)
        if (savedInstanceState == null) deliverActionFrom(activity.intent)
    }

    override fun onNewIntent(activity: Activity, intent: Intent) {
        deliverActionFrom(intent)
    }

    override fun onDestroy(activity: Activity) {
        if (this.activity?.get() === activity) this.activity = null
    }

    /** A button that opens the app arrives as an activity intent. */
    private fun deliverActionFrom(intent: Intent?) {
        val id = intent?.getStringExtra(EXTRA_ACTION_ID) ?: return
        intent.removeExtra(EXTRA_ACTION_ID)
        event(EVENT_ACTION, id)
    }

    // ── called from Rust ───────────────────────────────────────────────

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun start(
        context: Context,
        types: Int,
        title: String,
        text: String,
        ids: Array<String>,
        labels: Array<String>,
        opens: Int,
        visibility: Int,
        ongoing: Boolean,
        exit: Boolean,
        wakeLockToggle: Boolean,
        controlLabels: Array<String>,
        holdWakeLock: Boolean,
    ): String? {
        val app = context.applicationContext
        val info = try {
            @Suppress("DEPRECATION")
            app.packageManager.getServiceInfo(ComponentName(app, MobileForegroundService::class.java), 0)
        } catch (e: PackageManager.NameNotFoundException) {
            return "not-declared: no foreground service in the manifest"
        }
        if (types == 0) return "not-declared: no types"
        if (types and info.foregroundServiceType.inv() != 0) {
            return "not-declared: types $types, declared ${info.foregroundServiceType}"
        }
        if (types and ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE != 0 &&
            app.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED
        ) {
            return "missing-permission: ${Manifest.permission.RECORD_AUDIO}"
        }
        synchronized(lock) {
            content = Content(title, text, ids, labels, opens, visibility)
            controls = Controls(types, ongoing, exit, wakeLockToggle, controlLabels)
            ensureChannel(app)
            try {
                app.startForegroundService(Intent(app, MobileForegroundService::class.java).setAction(ACTION_START))
            } catch (e: IllegalStateException) {
                // ForegroundServiceStartNotAllowedException (API 31) is one.
                return "start-not-allowed: ${e.message}"
            } catch (e: SecurityException) {
                return "platform: ${e.message}"
            }
            generation += 1
            running = true
            setHeldLocked(app, holdWakeLock, notify = false)
        }
        return null
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun update(
        context: Context,
        title: String,
        text: String,
        ids: Array<String>,
        labels: Array<String>,
        opens: Int,
        visibility: Int,
    ): String? = synchronized(lock) {
        if (!running) return "not-running"
        content = Content(title, text, ids, labels, opens, visibility)
        post(context.applicationContext)
        null
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun updateText(context: Context, text: String): String? = synchronized(lock) {
        if (!running) return "not-running"
        val c = content
        content = Content(c.title, text, c.ids, c.labels, c.opens, c.visibility)
        post(context.applicationContext)
        null
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun stop(context: Context): String? {
        val app = context.applicationContext
        synchronized(lock) {
            if (!running) return null
            running = false
            setHeldLocked(app, false, notify = false)
        }
        app.stopService(Intent(app, MobileForegroundService::class.java))
        return null
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun isRunning(context: Context): Boolean = running

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun setWakeLock(context: Context, held: Boolean): String? = synchronized(lock) {
        if (!running) return "not-running"
        setHeldLocked(context.applicationContext, held, notify = true)
        null
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun wakeLockHeld(context: Context): Boolean = synchronized(lock) { wakeLock?.isHeld == true }

    // ── called by the service ──────────────────────────────────────────

    internal fun goForeground(service: MobileForegroundService) {
        val notification: Notification
        synchronized(lock) {
            if (!running) {
                service.stopSelf()
                return
            }
            service.generation = generation
            notification = build(service)
        }
        try {
            service.startForeground(NOTIFICATION_ID, notification, controls.types)
        } catch (e: Exception) {
            Log.e(NativeLib.TAG, "service: startForeground failed", e)
            synchronized(lock) {
                running = false
                setHeldLocked(service, false, notify = false)
            }
            event(EVENT_FAILED, e.toString())
            service.stopSelf()
            return
        }
        event(EVENT_STARTED, null)
        if (!notificationsShown(service)) event(EVENT_NOTIFICATION_HIDDEN, null)
    }

    internal fun exit(service: MobileForegroundService) {
        synchronized(lock) {
            if (!running) {
                service.stopSelf()
                return
            }
            running = false
            setHeldLocked(service, false, notify = false)
        }
        service.stopForeground(Service.STOP_FOREGROUND_REMOVE)
        service.stopSelf()
        event(EVENT_EXITED, null)
        activity?.get()?.let { it.runOnUiThread { it.finishAndRemoveTask() } }
    }

    internal fun toggleWakeLock(service: MobileForegroundService) {
        synchronized(lock) {
            if (!running) {
                service.stopSelf()
                return
            }
            setHeldLocked(service, wakeLock?.isHeld != true, notify = true)
        }
    }

    internal fun appAction(service: MobileForegroundService, id: String?) {
        if (!running) {
            service.stopSelf()
            return
        }
        if (id != null) event(EVENT_ACTION, id)
    }

    /** The user dismissed the notification; with `ongoing`, it comes back. */
    internal fun dismissed(service: MobileForegroundService) {
        synchronized(lock) {
            if (!running) {
                service.stopSelf()
                return
            }
            if (controls.ongoing) post(service)
        }
    }

    internal fun timedOut(service: MobileForegroundService) {
        synchronized(lock) {
            running = false
            setHeldLocked(service, false, notify = false)
        }
        event(EVENT_TIMED_OUT, null)
        service.stopSelf()
    }

    internal fun destroyed(service: MobileForegroundService) {
        synchronized(lock) {
            if (service.generation == generation) {
                running = false
                setHeldLocked(service, false, notify = false)
            }
        }
        event(EVENT_STOPPED, null)
    }

    // ── the notification and the wake lock ─────────────────────────────

    /** Acquire or release; reports a change, and with [notify] rebuilds the notification. */
    private fun setHeldLocked(context: Context, held: Boolean, notify: Boolean) {
        val was = wakeLock?.isHeld == true
        if (held == was) return
        if (held) {
            val lock = wakeLock ?: context.getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
                .also { it.setReferenceCounted(false) }
            wakeLock = lock
            @Suppress("WakelockTimeout")
            lock.acquire()
        } else {
            wakeLock?.release()
        }
        if (notify && running) post(context)
        event(if (held) EVENT_WAKE_LOCK_HELD else EVENT_WAKE_LOCK_RELEASED, null)
    }

    private fun post(context: Context) {
        context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, build(context))
    }

    private fun ensureChannel(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) != null) return
        val channel = NotificationChannel(CHANNEL, "Running in the background", NotificationManager.IMPORTANCE_LOW)
        channel.setShowBadge(false)
        manager.createNotificationChannel(channel)
    }

    private fun notificationsShown(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        val channel = manager.getNotificationChannel(CHANNEL)
        return manager.areNotificationsEnabled() &&
            (channel == null || channel.importance != NotificationManager.IMPORTANCE_NONE)
    }

    private fun build(context: Context): Notification {
        val c = content
        val k = controls
        val held = wakeLock?.isHeld == true
        val icon = context.applicationInfo.icon.takeIf { it != 0 } ?: android.R.drawable.stat_notify_sync
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(icon)
            .setContentTitle(c.title)
            .setContentText(if (held) c.text + k.labels[3] else c.text)
            .setOngoing(k.ongoing)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setVisibility(c.visibility)
            .setContentIntent(openApp(context, null, REQUEST_OPEN))
        if (Build.VERSION.SDK_INT >= 31) {
            builder.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        }
        if (k.ongoing) builder.setDeleteIntent(toService(context, ACTION_DELETED, REQUEST_DELETED, null))
        if (k.exit) builder.addAction(button(k.labels[0], toService(context, ACTION_EXIT, REQUEST_EXIT, null)))
        if (k.wakeLockToggle) {
            val label = if (held) k.labels[2] else k.labels[1]
            builder.addAction(button(label, toService(context, ACTION_WAKE_LOCK, REQUEST_WAKE_LOCK, null)))
        }
        c.ids.forEachIndexed { i, id ->
            val request = REQUEST_APP + i
            val intent = if (c.opens and (1 shl i) != 0) {
                openApp(context, id, request)
            } else {
                toService(context, ACTION_APP, request, id)
            }
            builder.addAction(button(c.labels.getOrElse(i) { id }, intent))
        }
        return builder.build()
    }

    private fun button(label: String, intent: PendingIntent): Notification.Action =
        Notification.Action.Builder(null as Icon?, label, intent).build()

    private fun toService(context: Context, action: String, request: Int, id: String?): PendingIntent {
        val intent = Intent(context, MobileForegroundService::class.java).setAction(action)
        if (id != null) intent.putExtra(EXTRA_ACTION_ID, id)
        return PendingIntent.getService(
            context, request, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    /** The app's launch activity; with [id], delivered to the running one as an action tap. */
    private fun openApp(context: Context, id: String?, request: Int): PendingIntent {
        val intent = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?: Intent().setPackage(context.packageName)
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (id != null) {
            intent.putExtra(EXTRA_ACTION_ID, id)
            intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        return PendingIntent.getActivity(
            context, request, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    // ── to Rust ────────────────────────────────────────────────────────

    internal fun event(code: Int, detail: String?) {
        try {
            nativeServiceEvent(code, detail)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "service: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/service.rs`) as
     * `Java_dev_highbay_libmobile_service_ForegroundServicePlugin_nativeServiceEvent`.
     */
    private external fun nativeServiceEvent(code: Int, detail: String?)
}
