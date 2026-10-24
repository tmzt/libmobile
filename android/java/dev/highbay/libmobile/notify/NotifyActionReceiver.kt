package dev.highbay.libmobile.notify

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * An alert's button that does not open the app (`NotifyPlugin`). Declared
 * in the manifest by the `notifications` feature, not exported: only the
 * app's own PendingIntents reach it.
 */
class NotifyActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra(NotifyPlugin.EXTRA_ID) ?: return
        val action = intent.getStringExtra(NotifyPlugin.EXTRA_ACTION) ?: return
        NotifyPlugin.action(context, id, action)
    }
}
