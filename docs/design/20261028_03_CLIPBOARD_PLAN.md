---
title: Reading the clipboard's text
date: 2026-10-28
status: In implementation
covers:
  - "Feature `clipboard`: `android::clipboard::read_text(&AndroidApp) ->
    Result<Option<String>, ClipboardError>` and `has_text(&AndroidApp) ->
    Result<bool, ClipboardError>`, through `ClipboardManager`"
  - "Android 10's rule that only the app with window focus reads the
    clipboard, and Android 12's \"pasted from your clipboard\" toast"
  - "Read only: nothing here writes to the clipboard"
sources:
  - "flankr `docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md` (Draft),
    decision 11.4: Paste text in the agent view's + menu and the terminal
    keys row needs the clipboard's text, \"a new libmobile `clipboard`
    feature, `android::clipboard::read_text(app: &AndroidApp) ->
    Option<String>` (`ClipboardManager.getPrimaryClip()`, item 0,
    `coerceToText`)\"; Paste text \"is disabled while the clipboard has no
    text\" (decision 8)."
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"plan those
    in appropriate repos with subagent, then impl\"."
  - "libmobile today (0.3.1, `488a698`): no clipboard call. Core bridges
    with a static Kotlin method called from Rust through
    `jni_util::with_app_class`: `bridge.LinkPlugin.openAppSettings`,
    `bridge.BuildInfoPlugin.info`; feature plugins in their own
    directories, listed in `codegen::PLUGINS` (`battery.BatteryPlugin`)."
  - "Android: `ClipboardManager.getPrimaryClip()`,
    `getPrimaryClipDescription()`, `ClipData.Item.getText()` and
    `coerceToText(Context)`; API 29 (Android 10) returns no clip to an app
    that is neither the default input method nor the app with window
    focus; API 31 (Android 12) shows a toast when an app reads a clip
    another app set through `getPrimaryClip()` (not through
    `getPrimaryClipDescription()`). No permission is needed."
related:
  - libmorph docs/design/20261028_03_CLIPBOARD_PLAN.md (wraps it as
    `MorphContext::clipboard_text`)
  - flankr docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md (the user)
  - libdevice docs/design/20261024_01_CLIPBOARD_PLAN.md (the desktop's
    `clipboard::read_text`, macOS)
---

# Reading the clipboard's text

## 1. What it is

A small `clipboard` feature that reads the plain text of the clipboard's
first item, for an app's own Paste button (flankr's **Paste text**). The
soft keyboard's own paste already works in the hidden text field; this is
for pasting outside it, on the user's tap.

It only reads. Writing to the clipboard (`setPrimaryClip`) is not in this
plan; it would be a later amendment when an app needs it.

## 2. API

Feature `clipboard`, off by default. No permission, no manifest entry.

The platform-free error in `src/clipboard.rs` (`libmobile::clipboard`),
which builds and is tested on any host:

```rust
/// Why the clipboard could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardError {
    /// The app's window does not have input focus. From Android 10 only
    /// the app with window focus (or the default keyboard) may read the
    /// clipboard; Android would answer as if it were empty. Read it on the
    /// user's tap, while the app is in front and no dialog is over it.
    NotFocused,
    /// Anything else Android or JNI reported.
    Platform(String),
}
// Display and std::error::Error.
```

The calls in `src/android/clipboard.rs` (`libmobile::android::clipboard`):

```rust
/// The clipboard's text: the first item of the primary clip, as text.
/// `Ok(None)` when the clipboard is empty, its first item has no text
/// (an image, say), or the text is empty. Any thread.
///
/// From Android 12 the system shows a toast ("<app> pasted from your
/// clipboard") when the clip came from another app, so call it only when
/// the user asked to paste, never to look.
pub fn read_text(app: &AndroidApp) -> Result<Option<String>, ClipboardError>;

/// Whether the clipboard holds text, from the clip's description only,
/// so Android shows no toast. For enabling a Paste button. Any thread.
pub fn has_text(app: &AndroidApp) -> Result<bool, ClipboardError>;
```

`read_text` returns a `Result` where flankr's plan names an `Option`, so
that "the app is not in front" is not taken for "the clipboard is empty".
libmorph's wrapper gives the `Option`.

### How the text is read

`clipboard.ClipboardPlugin` (`android/java/dev/highbay/libmobile/clipboard/`),
an activity plugin with no hooks, has two static methods that Rust calls
with the activity:

- `readText(activity: Activity): Array<String?>` gives `[kind, text]`,
  `kind` one of `"text"`, `"none"`, `"not-focused"`:
  1. On API 29 and later, `activity.hasWindowFocus()` false gives
     `"not-focused"`, before the clipboard is touched.
  2. `getSystemService(ClipboardManager)`; `getPrimaryClip()` null or with
     no items gives `"none"`.
  3. Item 0's `getText()`, if not null. Otherwise, only when the clip's
     description has a `text/*` MIME type, `coerceToText(activity)` (a
     content URI whose provider serves text). An image's URI is never
     turned into its URI string.
  4. An empty string gives `"none"`.
- `hasText(activity: Activity): String` gives `"yes"`, `"no"` or
  `"not-focused"`: the same focus check, then
  `hasPrimaryClip()` and the description's `hasMimeType("text/*")`.

A thrown exception reaches Rust as `ClipboardError::Platform` with its
message. The Rust side maps the kinds (`ClipboardError::from_kind` for the
error ones, host-tested).

### Privacy

libmobile never logs the clipboard's text or its length. Only errors are
logged, at warn, without the text.

## 3. Android's rules

- **Focus (Android 10, API 29).** Android returns no clip, and logs
  "Denying clipboard access", to an app that is not the default input
  method and does not have window focus. libmobile checks
  `hasWindowFocus()` first and answers `NotFocused` instead of a false
  "empty". A NativeActivity in front has focus while the soft keyboard is
  up; it loses it while a system dialog (a permission prompt, the
  chooser) or another window in split screen has focus. The generated
  projects' `minSdk` is 29, so the rule always applies.
- **The toast (Android 12, API 31).** `getPrimaryClip()` of a clip another
  app set shows the system's "<app> pasted from your clipboard" toast;
  the app cannot hide it. `getPrimaryClipDescription()` and
  `hasPrimaryClip()` show none, which is why `has_text` exists.
- **Threads.** `ClipboardManager` may be called from any thread. Both
  calls are short binder calls; `coerceToText` of a content URI reads the
  provider's stream on the calling thread, so a large text file blocks
  for as long as it takes (only for URI clips, which typed or copied text
  is not).

## 4. Files

- `Cargo.toml`: `clipboard = []` with a comment, after `notifications`.
- `src/clipboard.rs`: `ClipboardError`, `from_kind`, tests.
- `src/lib.rs`: `#[cfg(feature = "clipboard")] pub mod clipboard;`.
- `src/android/clipboard.rs`: `read_text`, `has_text`.
- `src/android/mod.rs`: `#[cfg(feature = "clipboard")] pub mod clipboard;`.
- `android/java/dev/highbay/libmobile/clipboard/ClipboardPlugin.kt`.
- `src/codegen.rs`: a `Plugin { feature: Some("clipboard"), dir:
  "clipboard", class: "clipboard.ClipboardPlugin", contributes: NOTHING }`
  at the end of `PLUGINS`; `known_features` gains `"clipboard"` before
  the flag; a test that it is gated and contributes nothing.
- `examples/wgpu`: the `clipboard` feature, and `--es cmd clipboard`,
  which logs `has_text` and then `read_text` as `Ok(Some(<n> bytes))`,
  `Ok(None)` or the error (never the text).
- `README.md`: a short section under the features.

## 5. Versioning

Additive: a new feature, a new module and a new `#[non_exhaustive]` error
type. libmobile 0.3.1 to **0.3.2**, tagged `v0.3.2`, with a `VERSIONS.md`
entry. libmorph and libmorph-build take it without a minor bump of their
own.

## 6. Steps

1. **The feature**: the error type, the Kotlin plugin, the two calls, the
   registry entry, the tests and the wgpu example's command.
   **Done when:** `cargo fmt --all -- --check`; host `cargo clippy
   --workspace --all-targets` with no features and `--all-features`;
   `cargo clippy --target aarch64-linux-android --workspace
   --all-targets` with no features, `--features clipboard` and
   `--all-features`: no warnings; `cargo test --workspace` and `cargo
   test -p libmobile --all-features` pass; `examples/build.sh wgpu` builds
   (the Kotlin compiles, `dexdump` shows `clipboard/ClipboardPlugin`); on
   the owner's S23, after copying a line in another app, `--es cmd
   clipboard` logs `has_text Ok(true)` and the line's length in bytes, and
   Android shows its "pasted" toast once, for `read_text` only.
2. **Release** as libmobile 0.3.2 on the owner's word: the README
   section, `Cargo.toml`'s version, the `VERSIONS.md` entry, the tag.
   **Done when:** the commit is tagged `v0.3.2` and libmorph builds
   against it.

## 7. Risks

- **A focus race.** A tap that also dismisses a dialog may arrive before
  the window has focus back; the call answers `NotFocused` and the app
  can say so. Rare, since the tap itself lands on the focused window.
- **Large clips.** No size cap here: Android's binder limit (about 1 MB)
  bounds a clip in practice, and the app caps what it pastes (flankr: 8
  KB).

## Amendment — 2026-10-28 23:42 EDT: approved

The owner approved planning and implementing the clipboard calls that
flankr's bottom bar plan needs, on 2026-10-28, through the flankr lead
(verbatim): "plan those in appropriate repos with subagent, then impl".
This plan and libmorph's `docs/design/20261028_03_CLIPBOARD_PLAN.md` are
built together, libmobile first, for libmobile 0.3.2 and libmorph 0.5.1.
The status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-28 23:44 EDT: implementation has started; the document is now immutable

Step 1 (the feature) is the first implementing commit.
