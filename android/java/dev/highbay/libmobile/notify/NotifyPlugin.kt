package dev.highbay.libmobile.notify

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.drawable.Icon
import android.os.Build
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * Alert notifications on the app's own channels (`libmobile::android::notify`).
 *
 * Rust calls the static methods from any thread, with the Application (from
 * `ndk-context`). Each returns `null` on success or a `"<kind>: <detail>"`
 * message (`bad-id`, `permission-denied`, `platform`), which Rust maps to
 * `NotifyError`.
 *
 * An alert is posted with the app's id as the notification's tag and
 * [NOTIFICATION_ID] as its number, apart from the foreground service's
 * notification (no tag). Its body's tap opens the launch activity with the
 * extras `bridge.IntentPlugin` turns into `NotifyEvent::Tapped` (keep in step
 * with `crate::intent::tap_extras`); a button that opens the app does the
 * same with the button's id, and any other button goes to
 * [NotifyActionReceiver], which reports it through [nativeNotifyAction].
 *
 * An activity plugin with no hooks, listed so the feature's sources are
 * staged like the others.
 */
object NotifyPlugin : MobileActivityPlugin {
    /** The number every alert is posted with; the tag tells them apart. */
    const val NOTIFICATION_ID = 0x6c6d

    const val EXTRA_ID = "dev.highbay.libmobile.notify.ID"
    const val EXTRA_ACTION = "dev.highbay.libmobile.notify.ACTION"

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun post(
        context: Context,
        id: String,
        channelId: String,
        channelName: String,
        importance: Int,
        title: String,
        text: String,
        ids: Array<String>,
        labels: Array<String>,
        opens: Int,
        visibility: Int,
        tag: String?,
    ): String? {
        val app = context.applicationContext
        val manager = app.getSystemService(NotificationManager::class.java)
        if (!shown(app, manager)) return "permission-denied"
        return try {
            if (manager.getNotificationChannel(channelId) == null) {
                manager.createNotificationChannel(NotificationChannel(channelId, channelName, importance))
            }
            val icon = app.applicationInfo.icon.takeIf { it != 0 } ?: android.R.drawable.stat_notify_chat
            val builder = Notification.Builder(app, channelId)
                .setSmallIcon(icon)
                .setContentTitle(title)
                .setContentText(text)
                .setAutoCancel(true)
                .setVisibility(visibility)
                .setContentIntent(openApp(app, id, tag, null))
            ids.forEachIndexed { i, action ->
                val intent = if (opens and (1 shl i) != 0) {
                    openApp(app, id, null, action)
                } else {
                    toReceiver(app, id, action)
                }
                builder.addAction(
                    Notification.Action.Builder(null as Icon?, labels.getOrElse(i) { action }, intent).build(),
                )
            }
            manager.notify(id, NOTIFICATION_ID, builder.build())
            null
        } catch (e: SecurityException) {
            "permission-denied: ${e.message}"
        } catch (e: Exception) {
            "platform: $e"
        }
    }

    /** Called from Rust via JNI: static. */
    @JvmStatic
    fun cancel(context: Context, id: String): String? = try {
        context.applicationContext.getSystemService(NotificationManager::class.java).cancel(id, NOTIFICATION_ID)
        null
    } catch (e: Exception) {
        "platform: $e"
    }

    /** Called from Rust via JNI: static. The app's alerts only, by their number. */
    @JvmStatic
    fun cancelAll(context: Context): String? = try {
        val manager = context.applicationContext.getSystemService(NotificationManager::class.java)
        for (shown in manager.activeNotifications) {
            if (shown.id == NOTIFICATION_ID && shown.tag != null) manager.cancel(shown.tag, NOTIFICATION_ID)
        }
        null
    } catch (e: Exception) {
        "platform: $e"
    }

    /** Notifications are on for the app: on Android 13+, `POST_NOTIFICATIONS` granted. */
    private fun shown(context: Context, manager: NotificationManager): Boolean {
        if (Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return false
        }
        return manager.areNotificationsEnabled()
    }

    /**
     * The launch activity, delivered to the running one, with the tap's
     * extras: the body's (with [tag]) or button [action]'s. The identifier
     * keeps each alert's and button's PendingIntent apart.
     */
    private fun openApp(context: Context, id: String, tag: String?, action: String?): PendingIntent {
        val intent = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?: Intent().setPackage(context.packageName)
        intent.addFlags(
            Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP,
        )
        intent.setIdentifier("libmobile.notify/$id/${action ?: ""}")
        intent.putExtra("libmobile.notification", id)
        intent.putExtra("libmobile.kind", "alert")
        if (tag != null) intent.putExtra("libmobile.tag", tag)
        if (action != null) intent.putExtra("libmobile.action", action)
        return PendingIntent.getActivity(
            context, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    private fun toReceiver(context: Context, id: String, action: String): PendingIntent {
        val intent = Intent(context, NotifyActionReceiver::class.java)
            .setIdentifier("libmobile.notify/$id/$action")
            .putExtra(EXTRA_ID, id)
            .putExtra(EXTRA_ACTION, action)
        return PendingIntent.getBroadcast(
            context, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    /** From [NotifyActionReceiver]: a button that does not open the app. */
    internal fun action(context: Context, id: String, action: String) {
        NativeLib.ensureLoaded(context)
        try {
            nativeNotifyAction(id, action)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "notify: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/notify.rs`) as
     * `Java_dev_highbay_libmobile_notify_NotifyPlugin_nativeNotifyAction`.
     */
    private external fun nativeNotifyAction(id: String, action: String)
}
