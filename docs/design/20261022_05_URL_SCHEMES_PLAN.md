---
title: Declaring URL schemes, and redacting the intent log
date: 2026-10-22
status: Draft
covers:
  - "`gradle::Project::url_scheme`: VIEW intent filters for custom URL
    schemes in the generated manifest"
  - "`codegen::url_scheme_filters`, the filters' XML"
  - "`intent::redact_url`, and an intent log line that never shows a URL's
    query or fragment"
sources:
  - libmorph docs/design/20261022_07_URL_SCHEMES_PLAN.md, approved by the
    owner on 2026-10-22. This plan is its section 2 and steps 1 to 3;
    libmorph wraps the result with `MorphApp::on_open_url`.
  - "handlr (owner-approved request, 2026-10-22): `handlr pair` prints
    `handlr://<tailscale host>:7347/pair#handlr1<base58>` and a QR code of
    it. The fragment is a single-use pairing secret."
  - "libmobile today: the core `bridge.IntentPlugin` forwards the launch
    intent (`onCreate`) and later ones (`onNewIntent`) as
    `MobileEvent::Intent(IntentData { action, data, extras, is_new })`,
    with `data` from `Intent.getDataString()`. The generated manifest sets
    `launchMode=\"singleTop\"` (`codegen.rs`). Nothing declares a scheme,
    and `src/android/intent.rs` logs every intent in full
    (`libmobile: intent {intent:?}`)."
related:
  - libmorph docs/design/20261022_07_URL_SCHEMES_PLAN.md
---

# Declaring URL schemes, and redacting the intent log

## 1. Goal

A generated project can declare custom URL schemes, so that a link with
one of them, tapped or scanned with the camera, opens the app. The link
reaches the app through the existing `MobileEvent::Intent`, at launch or
while running. libmobile never logs a URL past its path, because a query
or fragment may hold a secret.

## 2. Declaring schemes

```rust
impl gradle::Project {
    /// Open the app for links with this scheme (`handlr` for `handlr://…`).
    /// Lowercase letters, digits, `+`, `-` and `.`, starting with a letter
    /// (RFC 3986); panics otherwise. May be called more than once.
    pub fn url_scheme(self, scheme: &str) -> Self;
}
```

For each scheme, the generated manifest's activity gets one more filter,
next to the launcher's:

```xml
<intent-filter>
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />
    <data android:scheme="handlr" />
</intent-filter>
```

- No `android:autoVerify` and no App Links.
- `launchMode="singleTop"` is unchanged. Step 2 checks on the S23 that a
  link arriving while the app is in the background reaches the running
  activity through `onNewIntent`. If it creates a second activity instead,
  the choice of `singleTask` goes to the owner (section 6).
- `Project::generate` inserts the filters into the rendered manifest,
  before `</activity>`, using a new pure function
  `codegen::url_scheme_filters(&[String]) -> String`. `GradleProject`'s
  public fields do not change.
- Hand-written Gradle projects add their own filter; the README shows it.

## 3. Redacting the log

`src/android/intent.rs` logs the action, the data cut after its path, the
extra keys without their values, and `is_new`. The cut is a new pure
function:

```rust
/// The scheme, host, port and path of `url`, followed by `<redacted>` if
/// a query or fragment was cut off.
pub fn redact_url(url: &str) -> String;
```

Cases tested:

| Input | Output |
|---|---|
| `handlr://h.ts.net:7347/pair#handlr1abc` | `handlr://h.ts.net:7347/pair<redacted>` |
| `a://h/p?k=v` | `a://h/p<redacted>` |
| `a://h/p` | `a://h/p` |
| `` (empty) | `` |
| `not a url` | `not a url` |

`IntentData`'s `Debug` is unchanged; apps own what they log.

## 4. Example

The `wgpu` example declares `url_scheme("libmobile-wgpu")` and logs each
`MobileEvent::Intent`'s data through `redact_url`.

## 5. Steps

1. **Schemes and redaction.** `Project::url_scheme`,
   `codegen::url_scheme_filters`, `intent::redact_url` and the new log line,
   with host tests for the filter XML, scheme validation and the
   redaction cases.
   **Done when:** `cargo test --workspace` passes, and the `wgpu`
   example's generated project builds with `gradle assembleDebug` with the
   filter in its merged manifest.
2. **On the S23.**
   **Done when:** on the owner's Galaxy S23 Ultra, with the `wgpu` example:
   - `adb shell am start -a android.intent.action.VIEW -d
     'libmobile-wgpu://example.ts.net:7347/pair#secret'` cold-starts it,
     and logcat shows `…/pair<redacted>`, never `secret`;
   - the same command while it is in front, and again while it is in the
     background, reaches the running activity through `onNewIntent`
     (`is_new: true`), with no second activity in `dumpsys activity
     activities`.
3. **Release** 0.2.2, on the owner's word. It is additive: a new builder
   method and new pure functions; the log line is not API.
   **Done when:** the version commit is tagged `v0.2.2` and recorded in
   `VERSIONS.md`.

## 6. Open question for the owner

1. **`singleTop` or `singleTask`.** Only if step 2 shows a background link
   creating a second activity: should every generated project switch to
   `singleTask`, or only projects that declare a scheme?

## Amendment — 2026-10-22 23:30 EDT: approved

The owner approved this plan, with libmorph's, on 2026-10-22. It stays
`Draft` until the first commit that implements it. The open question in
section 6 is answered after step 2, if it arises.
