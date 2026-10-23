package dev.highbay.libmobile.events

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * System events for the app's own threads (`libmobile::system_events`).
 *
 * Rust calls [register] at the first `subscribe()`, with the Application.
 * It registers, for the life of the process, the default network callback
 * and a Doze receiver, and reports the state at that moment. Activity
 * lifecycle callbacks (visibility and focus) are registered earlier, at the
 * first activity's `onCreate`, so that they see its `onStart`; they report
 * only once Rust has subscribed. Each callback reports to Rust on the thread
 * it runs on; Rust hands the event to the subscribers at once.
 */
object SystemEventsPlugin : MobileActivityPlugin {
    // State kinds: keep in step with `src/android/system_events.rs`.
    private const val VISIBLE = 0
    private const val FOCUSED = 1
    private const val DEVICE_IDLE = 2

    // Transport codes: keep in step with `Transport::from_code`.
    private const val WIFI = 0
    private const val CELLULAR = 1
    private const val ETHERNET = 2
    private const val VPN = 3
    private const val OTHER = 4

    /** Rust has subscribed: events are reported. Guarded by `this`. */
    private var registered = false

    /** The lifecycle callbacks are registered. Guarded by `this`. */
    private var tracking = false

    /** Started and resumed activities, so several activities count as one app. Guarded by `this`. */
    private var started = 0
    private var resumed = 0

    /**
     * The lifecycle callbacks go on at the first activity's `onCreate`,
     * before its `onStart`, so the counts are right when Rust subscribes
     * later. Nothing is reported until then.
     */
    override fun onCreate(activity: Activity, savedInstanceState: Bundle?) {
        track(activity.application, seed = false)
    }

    private fun track(app: Application, seed: Boolean) = synchronized(this) {
        if (tracking) return@synchronized
        tracking = true
        if (seed) {
            // No activity has been created yet; count from the process importance.
            val state = ActivityManager.RunningAppProcessInfo()
            ActivityManager.getMyMemoryState(state)
            started = if (state.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE) 1 else 0
            resumed = if (state.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND) 1 else 0
        }
        app.registerActivityLifecycleCallbacks(Lifecycle)
    }

    /** Called from Rust via JNI: static, `(Landroid/content/Context;)V`. */
    @JvmStatic
    fun register(context: Context) {
        val app = context.applicationContext as Application
        track(app, seed = true)
        synchronized(this) {
            if (registered) return
            registered = true
            nativeStateSafe(VISIBLE, started > 0)
            nativeStateSafe(FOCUSED, resumed > 0)
        }
        registerDoze(app)
        registerNetwork(app)
    }

    private object Lifecycle : Application.ActivityLifecycleCallbacks {
        override fun onActivityStarted(activity: Activity) = count(VISIBLE, +1)
        override fun onActivityStopped(activity: Activity) = count(VISIBLE, -1)
        override fun onActivityResumed(activity: Activity) = count(FOCUSED, +1)
        override fun onActivityPaused(activity: Activity) = count(FOCUSED, -1)
        override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
        override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
        override fun onActivityDestroyed(activity: Activity) {}
    }

    /** Reports when the count goes from 0 to 1 or back. */
    private fun count(kind: Int, step: Int) = synchronized(this) {
        val before = if (kind == VISIBLE) started else resumed
        val after = maxOf(0, before + step)
        if (kind == VISIBLE) started = after else resumed = after
        if (registered && (before == 0) != (after == 0)) nativeStateSafe(kind, after > 0)
    }

    private fun registerDoze(app: Application) {
        val power = app.getSystemService(PowerManager::class.java)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                nativeStateSafe(DEVICE_IDLE, power.isDeviceIdleMode)
            }
        }
        val filter = IntentFilter(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED)
        if (Build.VERSION.SDK_INT >= 33) {
            app.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            app.registerReceiver(receiver, filter)
        }
        nativeStateSafe(DEVICE_IDLE, power.isDeviceIdleMode)
    }

    private fun registerNetwork(app: Application) {
        val connectivity = app.getSystemService(ConnectivityManager::class.java)
        connectivity.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            private var network: Network? = null
            private var reported: List<Any>? = null

            // Called after every onAvailable, and on each change. Reported
            // only when the network or what Rust sees of it changes.
            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                val validated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
                val metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                val transport = when {
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> VPN
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> WIFI
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> CELLULAR
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> ETHERNET
                    else -> OTHER
                }
                val now = listOf(validated, metered, transport)
                if (network == this.network && now == reported) return
                this.network = network
                reported = now
                nativeNetworkSafe(true, validated, metered, transport)
            }

            override fun onLost(network: Network) {
                if (network != this.network) return
                this.network = null
                reported = null
                nativeNetworkSafe(false, false, false, OTHER)
            }
        })
        // With no default network, no callback comes until one appears.
        if (connectivity.activeNetwork == null) nativeNetworkSafe(false, false, false, OTHER)
    }

    private fun nativeStateSafe(kind: Int, value: Boolean) {
        try {
            nativeState(kind, value)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "system events: native callback unavailable", e)
        }
    }

    private fun nativeNetworkSafe(present: Boolean, validated: Boolean, metered: Boolean, transport: Int) {
        try {
            nativeNetwork(present, validated, metered, transport)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "system events: native callback unavailable", e)
        }
    }

    /** Implemented in Rust (`src/android/system_events.rs`). */
    private external fun nativeState(kind: Int, value: Boolean)

    /** Implemented in Rust (`src/android/system_events.rs`). */
    private external fun nativeNetwork(present: Boolean, validated: Boolean, metered: Boolean, transport: Int)
}
