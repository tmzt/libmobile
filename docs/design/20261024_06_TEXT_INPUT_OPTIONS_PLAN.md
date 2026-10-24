---
title: Soft keyboard options, the action key and dismissal
date: 2026-10-24
status: Draft
covers:
  - "`TextInputOptions` for the hidden field: single or multiple lines,
    the keyboard type, the action key, autocorrect"
  - "`MobileEvent::TextAction` (the Done, Send, Go or Search key) and
    `MobileEvent::TextInputDismissed` (the keyboard closed by the system
    or the user)"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\", from the handlr
    session): typed replies to agents, a typed pairing code, and search;
    a keyboard with a starting value, options (single or multiple lines,
    text, URL or number, Done or Send, autocorrect), the full text after
    each edit, the action key, and a dismissal event. For the owner's
    approval in the libmorph session; this is the Android half."
  - "libmobile today (`src/android/text_input.rs`,
    `bridge/TextInputPlugin.kt`): `set_text_input(app, Some(value) | None)`
    focuses an invisible 1x1 `EditText` that owns the `InputConnection`
    and shows or hides the keyboard; `MobileEvent::TextChanged(String)`
    carries the full text after each edit; `Insets::ime_bottom` gives the
    keyboard's height. There are no options, no action key and no
    dismissal event."
  - "Android: `EditText.setInputType` (`TYPE_CLASS_TEXT`,
    `TYPE_TEXT_VARIATION_URI`, `TYPE_TEXT_VARIATION_EMAIL_ADDRESS`,
    `TYPE_CLASS_NUMBER`, `TYPE_TEXT_FLAG_MULTI_LINE`,
    `TYPE_TEXT_FLAG_NO_SUGGESTIONS`, `TYPE_TEXT_VARIATION_PASSWORD`),
    `setImeOptions` (`IME_ACTION_DONE`, `SEND`, `GO`, `SEARCH`,
    `IME_FLAG_NO_ENTER_ACTION`), `OnEditorActionListener`; the IME inset
    (`WindowInsets.Type.ime()`), already measured by
    `WindowInsetsPlugin`."
related:
  - libmorph docs/design/20261024_06_TEXT_INPUT_PLAN.md (wraps it)
---

# Soft keyboard options, the action key and dismissal

## 1. API

Core, no feature, in `src/android/text_input.rs` with platform-free types
in `libmobile::text_input`:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextInputOptions {
    pub multiline: bool,
    pub kind: TextKind,          // default Text
    pub action: TextAction,      // default Done
    pub autocorrect: bool,       // default true (false also turns off suggestions)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextKind { #[default] Text, Url, Email, Number, Password }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextAction { #[default] Done, Send, Go, Search, None }

/// As set_text_input, with options. set_text_input(app, v) is this with
/// TextInputOptions::default().
pub fn set_text_input_with(app: &AndroidApp, value: Option<&str>, options: &TextInputOptions);

// MobileEvent (#[non_exhaustive]) gains:
TextAction(TextAction),   // the keyboard's action key; for multiline with
                          // TextAction::None, Enter is a newline instead
TextInputDismissed,       // the keyboard closed while the field was active
```

- **Options** map to the field's `inputType` and `imeOptions` before it
  is focused; changing them while active re-applies them
  (`InputMethodManager.restartInput`).
- **The action key** comes from `OnEditorActionListener`. Single-line
  fields always report it (Enter is the action). Multi-line fields report
  it only with an action other than `None`.
- **Dismissal**: while the field is active, the IME inset falling to zero
  (the user pressed Back or the keyboard's hide key) reports
  `TextInputDismissed` once, and the field stays focused, so
  `set_text_input_with(app, Some(..))` shows the keyboard again. Hiding
  it with `None` reports nothing.
- `MobileEvent::TextChanged` and `ime_bottom` are unchanged.

## 2. Steps

1. **The options and events**, with the `wgpu` example opening the
   keyboard on a long press and logging each event.
   **Done when:** clippy and tests pass on the host and Android; on the
   owner's S23, each `TextKind` shows the matching keyboard (URL with `/`
   and `.com`, number pad), Done and Send log `TextAction`, Back logs
   `TextInputDismissed`, multi-line text logs newlines in `TextChanged`,
   and autocorrect off shows no suggestions.
2. **Release** in the next libmobile release, on the owner's word.
   Additive.

## Amendment — 2026-10-24 16:18 EDT: approved

The owner approved the text input plans in libmobile, libdevice and
libmorph on 2026-10-24 ("yes"), with libmorph's recommended answers: on
the desktop, text is edited at its end only (no caret movement or
selection); paste through libdevice's `clipboard`; `unicode-segmentation`
for Backspace. They are built after the notification and dark mode work
and released after it (libmobile 0.2.7, the next libdevice release,
libmorph 0.4.4), so they do not hold it up. The status stays `Draft`
until the first commit that implements it.
