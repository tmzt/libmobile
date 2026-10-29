---
title: Writing text to the clipboard
date: 2026-10-29
status: In implementation
covers:
  - "Feature `clipboard` gains the write: `android::clipboard::write_text(app:
    &AndroidApp, text: &str) -> Result<Copied, ClipboardError>`, through
    `ClipboardManager.setPrimaryClip(ClipData.newPlainText(label, text))`"
  - "`clipboard::Copied { system_notice: bool }`: whether Android showed its
    own copy confirmation (Android 13, API 33, and later), so the app shows
    its own notice only on older versions"
  - "The Kotlin plugin's `writeText`, the wgpu example's `clipboard-write`
    command, and a scripted adb check on the owner's S23"
sources:
  - "Owner (2026-10-29, in the RoundTwo session, verbatim): \"also add a
    copy button (copy icon if available or Copy in 0.8 size) to the
    question and transcript in the review card\"."
  - "Owner (2026-10-29, in the RoundTwo session), asked how to proceed since
    no library writes the clipboard, chose: \"Plan it in the libraries:
    write a plan in libmobile for writing the clipboard (Android's
    ClipboardManager), then one in libmorph for ctx.set_clipboard_text
    (Android and macOS), each under its own rules and version. Then move
    RoundTwo's libmorph pin and add the buttons.\""
  - "`docs/design/20261028_03_CLIPBOARD_PLAN.md` (In implementation), 1:
    \"It only reads. Writing to the clipboard (`setPrimaryClip`) is not in
    this plan; it would be a later amendment when an app needs it.\" That
    plan is frozen, so the write is this new plan (AGENTS.md Rule 1.7)."
  - "libmobile today (0.3.4, `07fe2df`): `src/clipboard.rs`
    (`ClipboardError`, `from_kind`, `non_empty`), `src/android/clipboard.rs`
    (`read_text`, `has_text`, any thread, through `jni_util::with_app_class`),
    `android/java/dev/highbay/libmobile/clipboard/ClipboardPlugin.kt`
    (`readText`, `hasText`); the wgpu example's `--es cmd clipboard`."
  - "Android: `ClipboardManager.setPrimaryClip(ClipData)` and
    `ClipData.newPlainText(CharSequence label, CharSequence text)`; no
    permission and no window-focus rule for writing (Android 10's focus
    rule applies to reads only). From API 33 (Android 13) the system shows
    its own visual confirmation of a copy, and Android's guidance is that
    apps targeting it should not show their own as well (\"avoid duplicate
    notifications\"; show an in-app notice only on Android 12L, API 32, and
    lower). `ClipDescription.EXTRA_IS_SENSITIVE` (API 33, the string
    `android.content.extra.IS_SENSITIVE`) in the description's
    `PersistableBundle` extras hides the content from that confirmation's
    preview."
related:
  - docs/design/20261028_03_CLIPBOARD_PLAN.md (the read, which this
    extends in the same feature, plugin and error type)
  - libdevice docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md (the desktop
    write, macOS)
  - libdevice docs/design/20261024_01_CLIPBOARD_PLAN.md (the desktop read)
  - libmorph docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md (wraps both as
    `MorphContext::set_clipboard_text`)
  - libmorph docs/design/20261028_03_CLIPBOARD_PLAN.md (the read on every
    host)
  - RoundTwo docs/design/20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md (the
    first user: Copy on the results' question text and each answer's
    transcript)
---

# Writing text to the clipboard

## 1. What it is

One call that puts plain text on the clipboard, for an app's own Copy
button (RoundTwo's Copy on a question and on an answer's transcript). It
completes the `clipboard` feature, which until now only reads.

### Decision: the same feature, not a new `clipboard-write`

The write goes into the existing `clipboard` feature. The read plan's
"Read only" described its scope, not a promise to callers: the write adds
no permission, no manifest entry, no plugin hook and no dependency, so an
app that enables `clipboard` today builds and behaves exactly as before.
A separate feature would need either a second Kotlin plugin directory for
one method or `#[cfg]` inside one Kotlin object (which the Gradle source
sets cannot express), for no gain. libmorph's `mobile-clipboard`
passthrough then gives both directions with no new flag.

The comments that say "read only" (`Cargo.toml`, `src/clipboard.rs`,
`src/android/clipboard.rs`, `ClipboardPlugin.kt`, `src/codegen.rs`) are
updated to "the clipboard's text, read and written". The read plan is not
edited (Rule 1.7).

## 2. API

### The platform-free type, `src/clipboard.rs` (`libmobile::clipboard`)

```rust
/// A copy that Android accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Copied {
    /// Android showed its own confirmation of the copy: Android 13
    /// (API 33) and later. The app should then show none of its own; on
    /// older versions (`false`) it may show a short "Copied" notice.
    pub system_notice: bool,
}
```

Also re-exported as `libmobile::android::clipboard::Copied`.
`ClipboardError` is unchanged; a write never gives `NotFocused` (writing
has no focus rule), only `Platform(String)`.

`ClipboardError::from_kind` learns the write's two answer kinds,
`"copied"` and `"copied-notice"`, as kinds that are not errors (it maps
any unknown kind to `Platform`). A new `pub(crate) fn
copied_from_kind(kind: &str) -> Result<Copied, ClipboardError>` maps
`"copied"` to `Copied { system_notice: false }`, `"copied-notice"` to
`Copied { system_notice: true }`, and anything else through `from_kind`
(an unknown kind is `Platform("unexpected answer ...")`). Both are
host-tested.

### The call, `src/android/clipboard.rs` (`libmobile::android::clipboard`)

```rust
/// Put `text` on the clipboard as plain text (`ClipData.newPlainText`),
/// replacing what was there. Any thread, as `read_text`; no permission,
/// and the app's window need not have focus.
///
/// `Ok(Copied { system_notice: true })` on Android 13 and later, where the
/// system shows its own confirmation: the app shows none. On older
/// versions `system_notice` is `false` and the app may show its own.
/// An empty `text` is written as given (it clears the clipboard's text).
///
/// libmobile never logs the text or its length.
pub fn write_text(app: &AndroidApp, text: &str) -> Result<Copied, ClipboardError>;
```

It calls the plugin through `with_app_class`, as `read_text` does, with
the same `JavaException` handling (`exception_catch`, then
`CaughtJavaException { msg, .. }` to `Platform(msg)`, logged at warn by the
existing `platform` helper, which logs the error only).

### The label

`ClipData.newPlainText(label, text)` takes a label, a short description of
the clip that some clipboard managers show. libmobile passes the app's own
name (`activity.applicationInfo.loadLabel(activity.packageManager)`), so
callers need not choose one and nothing of the text leaks into it. A label
parameter can be added later, additively, as a `write_text_with`.

## 3. The Kotlin method

In `android/java/dev/highbay/libmobile/clipboard/ClipboardPlugin.kt`:

```kotlin
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
```

with `private const val COPIED = "copied"` and `COPIED_NOTICE =
"copied-notice"` beside the other kinds (kept in step with
`ClipboardError::from_kind`), and `import android.content.ClipData`.
`Build.VERSION_CODES.TIRAMISU` is 33; the generated projects compile
against an SDK that has it.

**Threads.** The read path calls `ClipboardManager` from whatever thread
Rust is on, and so does the write: `setPrimaryClip` is one binder call to
the system clipboard service and is safe from any thread. No hop to the UI
thread is needed. (The 2026-10-29 request said "on the UI thread as the
read path does"; the read path in fact runs on the calling thread, and the
write matches it. Open question 8.1.)

**Size.** A text over the binder transaction limit (about 1 MB with
everything else in flight) makes `setPrimaryClip` throw; it reaches Rust
as `Platform` with the exception's message. libmobile sets no cap of its
own; apps copy short texts.

## 4. Android's rules

- **No permission, no focus rule.** Any app may write the clipboard; the
  Android 10 focus rule (`NotFocused`) is for reads.
- **The confirmation (Android 13, API 33).** After any `setPrimaryClip`
  the system shows its own confirmation with a preview of the content.
  Android's guidance is that an app should not show its own notice as well
  on 13 and later, and should show one on 12L and earlier. `Copied::
  system_notice` carries that, decided from `Build.VERSION.SDK_INT >= 33`.
  It says what Android version the phone runs, not that the overlay was
  drawn: a vendor build may style or replace the overlay (step 3 checks
  what the owner's Samsung shows).
- **Our own clip read back.** Reading a clip the app itself set shows no
  "pasted" toast (Android 12's toast is for another app's clip), which is
  what step 3's check relies on.

## 5. Privacy

libmobile never logs the text or its length on the write either. The
example logs only the `Result`. Errors are logged at warn without the
text (an exception's message from `setPrimaryClip` does not include the
clip's text).

## 6. Files

- `src/clipboard.rs`: `Copied`, the two kinds in `from_kind`,
  `copied_from_kind`, tests; module docs say read and write.
- `src/android/clipboard.rs`: `write_text`; `pub use
  crate::clipboard::{ClipboardError, Copied};`; module docs.
- `android/java/dev/highbay/libmobile/clipboard/ClipboardPlugin.kt`:
  `writeText` and the two kinds; class docs.
- `Cargo.toml`: the `clipboard` feature's comment (no new feature).
- `src/codegen.rs`: the `clipboard` entry's comment only.
- `examples/wgpu/src/lib.rs`: `--es cmd clipboard-write` writes the fixed
  string `CLIPBOARD_CHECK = "libmobile-clipboard-check-29Q"` (29 bytes)
  and logs `wgpu example: clipboard write_text {result:?}` (the `Result`
  only); the module docs list it.
- `README.md`: the clipboard section gains the write (release step).

## 7. Versioning

Additive: a new function, a new `#[non_exhaustive]` struct, and new
non-error answer kinds inside a crate-private mapping. No permission or
manifest change. libmobile 0.3.4 to **0.3.5**, tagged `v0.3.5`, with a
`VERSIONS.md` entry; if another release (the share and view plan's
sharing features) takes 0.3.5 first, this lands as the next patch after
it. libmorph and libmorph-build take it without a minor bump.

## 8. Steps

1. **The Kotlin method and the Rust call**: `writeText`, `Copied`,
   `copied_from_kind`, `write_text`, the comment changes and the host
   tests (`from_kind` treats `"copied"` and `"copied-notice"` as no error;
   `copied_from_kind` maps both and rejects `"bogus"` and `"not-focused"`
   correctly).
   **Done when:** `cargo fmt --all -- --check`; host `cargo clippy
   --workspace --all-targets` with no features and `--all-features`;
   `cargo clippy --target aarch64-linux-android --workspace --all-targets`
   with no features, `--features clipboard` and `--all-features`: no
   warnings; `cargo test --workspace` and `cargo test -p libmobile
   --all-features` pass.
2. **The example command**: `clipboard-write` in `examples/wgpu`.
   **Done when:** `examples/build.sh wgpu` builds; `dexdump` of the APK
   shows `clipboard/ClipboardPlugin` with a public static `writeText` of
   signature `(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;`
   beside `readText` and `hasText`.
3. **On the owner's S23** (Android 15 or 16), with the example installed
   and in front, this script passes:

   ```sh
   APP=dev.highbay.libmobile.example.wgpu
   ACT="$APP/dev.highbay.libmobile.MobileNativeActivity"
   adb logcat -c
   adb shell am start -n "$ACT" --es cmd clipboard-write
   sleep 2
   adb shell am start -n "$ACT" --es cmd clipboard
   sleep 2
   LOG=$(adb logcat -d -s libmobile-wgpu libmobile)
   echo "$LOG" | grep -q 'clipboard write_text Ok(Copied { system_notice: true })'
   echo "$LOG" | grep -q 'clipboard has_text Ok(true)'
   echo "$LOG" | grep -q 'clipboard read_text Ok(Some(29 bytes))'
   ! adb logcat -d | grep -q 'libmobile-clipboard-check-29Q'
   ```

   (the tags are the ones `examples/build.sh` prints for the example's
   log; the last line checks the whole log, so the text never reaches any
   of it).
   **Done when:** the script exits 0; the owner sees Android's (or One
   UI's) own copy confirmation once after the write and no "pasted from
   your clipboard" toast for the read; pasting in another app (Messages,
   say) gives `libmobile-clipboard-check-29Q`. The result is recorded in
   `CHANGES.md`, including what the S23's confirmation looks like.
4. **Release** as libmobile 0.3.5 (or the next free patch) on the owner's
   word: the README's clipboard section, `Cargo.toml`'s version, the
   `VERSIONS.md` entry, the tag `v0.3.5`.
   **Done when:** the commit is tagged and libmorph builds against it with
   `mobile-clipboard` for `aarch64-linux-android`.

## 9. Open questions

1. **Threads.** Keep the write on the calling thread, as the read is
   (proposed), or post it to the UI thread with `runOnUiThread` and wait?
   Nothing in `ClipboardManager` requires the UI thread; posting would only
   add latency and a deadlock risk if called from the UI thread's own
   handler.
2. **Sensitive content.** Should the write take an option that sets
   `ClipDescription.EXTRA_IS_SENSITIVE` (API 33+; the literal
   `"android.content.extra.IS_SENSITIVE"` on older versions, which they
   ignore), so Android's confirmation hides the preview? Interview
   questions and transcripts are not secrets, so this plan leaves it out;
   it would be an additive `write_text_with(app, text, &WriteOptions {
   sensitive, label })` later, with libdevice's
   `org.nspasteboard.ConcealedType` as its macOS counterpart.
3. **Older versions.** `system_notice: false` tells the app to show its
   own notice on Android 10 to 12L (the generated `minSdk` is 29). Is a
   flag enough, or should libmobile show a `Toast` itself on those
   versions? Proposed: the flag; the app draws its notice in its own UI.
4. **A size cap.** Should libmobile refuse texts over, say, 512 KB with a
   new `ClipboardError::TooLarge` (additive, the enum is
   `#[non_exhaustive]`) instead of letting the binder throw? Proposed: no
   cap now.

## 10. Risks

- **Vendor overlays.** One UI may draw its own confirmation, or none, on
  top of AOSP's; `system_notice` follows the API level, not what was
  drawn. Step 3 records what the S23 shows.
- **A stale libmorph-build.** An APK whose Gradle sources come from an
  older libmobile would lack `writeText` and fail with
  `NoSuchMethodError` (reported as `Platform`). libmorph republishes the
  Gradle sources of its own libmobile dependency, so this holds only while
  an app's `Cargo.lock` resolves libmobile to 0.3.5 or later.

## Amendment — 2026-10-29 17:35 EDT: approved; implementation started

The owner approved the plan on 2026-10-29: "approve all four plans and
build them", and "push them when built". It is approved as written, and
its open questions take the body's defaults: (1) the write runs on the
calling thread, as the read does; (2) marking the text sensitive
(`EXTRA_IS_SENSITIVE`) is deferred to a later, additive
`write_text_with`; (3) below Android 13 the `system_notice` flag is
enough, and libmobile shows no toast of its own; (4) no size cap.

Step 1 is this commit, the first implementing one. Implementation has
started and the document is now immutable (Rule 1.5).
