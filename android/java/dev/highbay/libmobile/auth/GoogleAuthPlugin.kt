package dev.highbay.libmobile.auth

import android.app.Activity
import android.util.Log
import androidx.credentials.CredentialManager
import androidx.credentials.CredentialManagerCallback
import androidx.credentials.CustomCredential
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetCredentialResponse
import androidx.credentials.exceptions.GetCredentialException
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.util.concurrent.Executors

/**
 * Native Sign in with Google over Android's Credential Manager (Cargo feature
 * `google-auth`, which also adds the androidx.credentials + googleid AARs to
 * the generated Gradle script).
 *
 * Draws nothing: the account-picker sheet is Google's own system UI.
 *
 * Flow:
 *  1. Rust (`libmobile::android::start_google_login`) calls [start].
 *  2. Credential Manager shows the sheet and answers on [executor].
 *  3. Either outcome goes straight back to Rust through [nativeLoginResult]
 *     and arrives as `MobileEvent::GoogleLogin`.
 *
 * A cancel is a *failure*, not a silent drop: Rust must always get an answer,
 * or an app waiting on the sheet would have no way out of that state.
 */
object GoogleAuthPlugin : MobileActivityPlugin {
    /** Credential Manager needs somewhere to call us back; one thread is plenty. */
    private val executor = Executors.newSingleThreadExecutor { r ->
        Thread(r, "libmobile-google-login").apply { isDaemon = true }
    }

    /** Guards against a second sheet if login is requested twice. */
    @Volatile
    private var inFlight = false

    override fun onDestroy(activity: Activity) {
        // The sheet dies with its activity; let the next one start a new flow.
        inFlight = false
    }

    /**
     * Start the account-picker flow. Returns immediately.
     *
     * `serverClientId` is the **web** OAuth client of the backend that will
     * verify the token (not the Android client — Google binds that one to the
     * package name + signing SHA-1 and never puts it in the token's `aud`).
     *
     * Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)V`.
     */
    @JvmStatic
    fun start(activity: Activity, serverClientId: String) {
        if (inFlight) {
            Log.i(NativeLib.TAG, "google login: already in flight, ignoring")
            return
        }
        if (serverClientId.isEmpty()) {
            // Fail fast rather than show a sheet that cannot succeed.
            deliver(null, "no_client_id: the server client ID was empty")
            return
        }

        inFlight = true
        val option = GetGoogleIdOption.Builder()
            .setServerClientId(serverClientId)
            // A first sign-in on this device has no previously-authorized
            // account to filter down to.
            .setFilterByAuthorizedAccounts(false)
            // Never sign in silently: the user picks the account.
            .setAutoSelectEnabled(false)
            .build()
        val request = GetCredentialRequest.Builder()
            .addCredentialOption(option)
            .build()

        try {
            CredentialManager.create(activity).getCredentialAsync(
                activity,
                request,
                null,
                executor,
                object : CredentialManagerCallback<GetCredentialResponse, GetCredentialException> {
                    override fun onResult(result: GetCredentialResponse) = onCredential(result)

                    override fun onError(e: GetCredentialException) {
                        Log.w(NativeLib.TAG, "google login: failed: ${e.type}: ${e.message}")
                        deliver(null, "${e.type}: ${e.message}")
                    }
                },
            )
        } catch (t: Throwable) {
            // e.g. no credential provider at all on a de-Googled device.
            Log.e(NativeLib.TAG, "google login: could not start", t)
            deliver(null, "start_failed: ${t.javaClass.simpleName}: ${t.message}")
        }
    }

    private fun onCredential(result: GetCredentialResponse) {
        val credential = result.credential
        if (credential is CustomCredential &&
            credential.type == GoogleIdTokenCredential.TYPE_GOOGLE_ID_TOKEN_CREDENTIAL
        ) {
            try {
                val google = GoogleIdTokenCredential.createFrom(credential.data)
                deliver(google.idToken, null)
            } catch (t: Throwable) {
                Log.e(NativeLib.TAG, "google login: malformed credential", t)
                deliver(null, "bad_google_credential: ${t.message}")
            }
            return
        }
        deliver(null, "unexpected_credential_type: ${credential.type}")
    }

    /** Exactly one of [idToken] / [error] is non-null. */
    private fun deliver(idToken: String?, error: String?) {
        inFlight = false
        try {
            nativeLoginResult(idToken, error)
        } catch (e: UnsatisfiedLinkError) {
            // Log rather than crash the app from a background executor thread.
            Log.e(NativeLib.TAG, "google login: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/auth.rs`) as
     * `Java_dev_highbay_libmobile_auth_GoogleAuthPlugin_nativeLoginResult`.
     */
    private external fun nativeLoginResult(idToken: String?, error: String?)
}
