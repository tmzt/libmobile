package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.text.Editable
import android.text.TextWatcher
import android.util.Log
import android.view.ViewGroup
import android.view.Window
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * An invisible 1x1 [EditText] that owns the IME `InputConnection`, so the
 * soft keyboard (composition, autocorrect, voice input) works while Rust still
 * draws every pixel. Rust focuses it with [setActive]
 * (`libmobile::android::set_text_input`) and receives the field's full text
 * after every edit (`src/android/text_input.rs`).
 */
object TextInputPlugin : MobileActivityPlugin {
    private var editor: EditText? = null
    private var syncing = false

    override fun onWindowAttached(activity: Activity, window: Window) {
        if (editor != null) return
        val field = EditText(activity).apply {
            setBackgroundColor(Color.TRANSPARENT)
            setTextColor(Color.TRANSPARENT)
            isCursorVisible = false
            isSingleLine = true
            alpha = 0f
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                    if (!syncing) deliver(s?.toString() ?: "")
                }

                override fun afterTextChanged(s: Editable?) {}
            })
        }
        (window.decorView as ViewGroup).addView(field, ViewGroup.LayoutParams(1, 1))
        editor = field
    }

    override fun onDestroy(activity: Activity) {
        // The field belongs to this activity's window; a recreated activity
        // gets a fresh one.
        editor = null
    }

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;ZLjava/lang/String;)V`. */
    @JvmStatic
    fun setActive(activity: Activity, active: Boolean, value: String) {
        activity.runOnUiThread {
            val field = editor ?: return@runOnUiThread
            val input = activity.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
            if (active) {
                syncing = true
                field.setText(value)
                field.setSelection(value.length)
                syncing = false
                field.requestFocus()
                input.showSoftInput(field, InputMethodManager.SHOW_IMPLICIT)
            } else {
                input.hideSoftInputFromWindow(field.windowToken, 0)
                field.clearFocus()
            }
        }
    }

    private fun deliver(text: String) {
        try {
            nativeTextChanged(text)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "text input: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/text_input.rs`) as
     * `Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextChanged`.
     */
    private external fun nativeTextChanged(text: String)
}
