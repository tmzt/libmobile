package dev.highbay.libmobile.bridge

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.text.Editable
import android.text.TextWatcher
import android.text.InputType
import android.util.Log
import android.view.KeyEvent
import android.view.ViewGroup
import android.view.Window
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib

/**
 * An invisible 1x1 [EditText] that owns the IME `InputConnection`, so the
 * soft keyboard (composition, autocorrect, voice input) works while Rust still
 * draws every pixel. Rust focuses it with [setActive]
 * (`libmobile::android::set_text_input_with`), with the input type and IME
 * options Rust computed from its `TextInputOptions`, and receives the field's
 * full text after every edit and the action key (`src/android/text_input.rs`).
 */
object TextInputPlugin : MobileActivityPlugin {
    private var editor: EditText? = null
    private var syncing = false

    // Set by setActive on the UI thread, read by the listener there.
    private var reportsAction = true
    private var actionCode = 0

    override fun onWindowAttached(activity: Activity, window: Window) {
        if (editor != null) return
        val field = EditText(activity).apply {
            setBackgroundColor(Color.TRANSPARENT)
            setTextColor(Color.TRANSPARENT)
            isCursorVisible = false
            inputType = InputType.TYPE_CLASS_TEXT
            imeOptions = EditorInfo.IME_ACTION_DONE or EditorInfo.IME_FLAG_NO_EXTRACT_UI
            alpha = 0f
            // The action key (performEditorAction: event null), or Enter on a
            // single line from a soft or hardware keyboard (IME_NULL with the
            // key's down and up). Handled, so the keyboard stays up; reported
            // once, on the down. On many lines without an action, not
            // handled: Enter is a newline.
            setOnEditorActionListener { _, _, event ->
                if (!reportsAction) return@setOnEditorActionListener false
                if (event == null || event.action == KeyEvent.ACTION_DOWN) {
                    deliverAction(actionCode)
                }
                true
            }
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

    /**
     * Called from Rust via JNI: static,
     * `(Landroid/app/Activity;ZLjava/lang/String;IIZI)V`. `inputType` and
     * `imeOptions` are applied before the field is focused, and re-applied
     * with `restartInput` when they change while it is active.
     */
    @JvmStatic
    fun setActive(
        activity: Activity,
        active: Boolean,
        value: String,
        inputType: Int,
        imeOptions: Int,
        reportsAction: Boolean,
        actionCode: Int,
    ) {
        activity.runOnUiThread {
            val field = editor ?: return@runOnUiThread
            val input = activity.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
            if (active) {
                this.reportsAction = reportsAction
                this.actionCode = actionCode
                val changed = field.inputType != inputType || field.imeOptions != imeOptions
                syncing = true
                if (field.inputType != inputType) field.inputType = inputType
                field.imeOptions = imeOptions
                field.setText(value)
                field.setSelection(value.length)
                syncing = false
                if (changed && field.hasFocus()) input.restartInput(field)
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

    private fun deliverAction(code: Int) {
        try {
            nativeTextAction(code)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "text input: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/text_input.rs`) as
     * `Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextChanged`.
     */
    private external fun nativeTextChanged(text: String)

    /**
     * Implemented in Rust (`src/android/text_input.rs`) as
     * `Java_dev_highbay_libmobile_bridge_TextInputPlugin_nativeTextAction`:
     * the code Rust handed [setActive] for the options' action.
     */
    private external fun nativeTextAction(code: Int)
}
