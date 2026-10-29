package dev.highbay.libmobile.clipboard

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * The clipboard's text, read and written (`libmobile::android::clipboard`).
 *
 * Rust calls the static methods from any thread, with the activity.
 * Answers are kinds that Rust maps (`ClipboardError::from_kind`):
 * `"text"`, `"none"`, `"yes"`, `"no"`, `"copied"`, `"copied-notice"`, or
 * `"not-focused"`. A thrown exception reaches Rust as
 * `ClipboardError::Platform`.
 *
 * Android 10 (API 29) gives the clipboard only to the app with window focus
 * (or the default keyboard), and tells any other app it is empty, so focus
 * is checked first. Android 12 (API 31) shows a "pasted" toast when
 * [readText] reads a clip another app copied; [hasText] reads only the
 * description, which shows none. Writing ([writeText]) needs no focus;
 * Android 13 (API 33) confirms a copy itself.
 *
 * Nothing here logs the text or its length. An activity plugin with no
 * hooks, listed only under the `clipboard` feature.
 */
object ClipboardPlugin : MobileActivityPlugin {
    // Kinds: keep in step with `ClipboardError::from_kind` (src/clipboard.rs).
    private const val TEXT = "text"
    private const val NONE = "none"
    private const val YES = "yes"
    private const val NO = "no"
    private const val NOT_FOCUSED = "not-focused"
    private const val COPIED = "copied"
    private const val COPIED_NOTICE = "copied-notice"

    /** Any text type: `text/plain`, `text/html`, `text/uri-list`. */
    private const val TEXT_ANY = "text/*"

    private fun focused(activity: Activity): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.Q || activity.hasWindowFocus()

    private fun manager(activity: Activity): ClipboardManager? =
        activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager

    /**
     * Called from Rust via JNI: static, `(Landroid/app/Activity;)[Ljava/lang/String;`:
     * `[kind, text]`. The first item's `getText()`, or for a clip with a text MIME type
     * without it (a content URI), `coerceToText`; never an image's URI.
     */
    @JvmStatic
    fun readText(activity: Activity): Array<String?> {
        if (!focused(activity)) return arrayOf(NOT_FOCUSED, null)
        val clip = manager(activity)?.primaryClip
        if (clip == null || clip.itemCount == 0) return arrayOf(NONE, null)
        val item = clip.getItemAt(0)
        val text = item.text
            ?: if (clip.description.hasMimeType(TEXT_ANY)) item.coerceToText(activity) else null
        val value = text?.toString()
        return if (value.isNullOrEmpty()) arrayOf(NONE, null) else arrayOf(TEXT, value)
    }

    /**
     * Called from Rust via JNI: static, `(Landroid/app/Activity;)Ljava/lang/String;`:
     * `"yes"` when the primary clip's description has a text MIME type ([TEXT_ANY]).
     */
    @JvmStatic
    fun hasText(activity: Activity): String {
        if (!focused(activity)) return NOT_FOCUSED
        val manager = manager(activity) ?: return NO
        if (!manager.hasPrimaryClip()) return NO
        val description = manager.primaryClipDescription ?: return NO
        return if (description.hasMimeType(TEXT_ANY)) YES else NO
    }

    /**
     * Called from Rust via JNI: static,
     * `(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;`.
     * Puts [text] on the clipboard as plain text, labelled with the app's
     * name. `"copied-notice"` on API 33 and later, where the system shows its
     * own confirmation; `"copied"` before it. Throws on failure (Rust maps it
     * to `ClipboardError::Platform`). Never logs the text.
     */
    @JvmStatic
    fun writeText(activity: Activity, text: String): String {
        val manager = manager(activity)
            ?: throw IllegalStateException("no clipboard service")
        val label = activity.applicationInfo.loadLabel(activity.packageManager)
        manager.setPrimaryClip(ClipData.newPlainText(label, text))
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) COPIED_NOTICE else COPIED
    }
}
