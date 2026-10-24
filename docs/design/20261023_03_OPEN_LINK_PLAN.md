---
title: Opening a link in another app
date: 2026-10-23
status: Draft
covers:
  - "`open_link(url)`: ask Android to open an http or https URL in the
    app that handles it (normally the browser)"
  - What is checked before the intent is sent, and what is logged
sources:
  - "handlr (request, 2026-10-23, from the handlr session): the owner
    asked for repowiki links in the companion's agent view; plan paths in
    agent replies become `http://<mac>:8088/#/src--handlr/docs/design/<plan>`
    links, and a tap should open them in the browser. It asked for an
    outbound call, `Intent(ACTION_VIEW, uri)`, validating the scheme,
    not logging past the path, and an error when nothing handles it.
    For the owner's approval in the libmorph session."
  - "libmobile today: the inbound side (`MobileEvent::Intent`, URL
    schemes, plan 20261022_05); activities are started only by the camera
    plugin and, unreleased, the foreground service plugin's
    `startVisible`. `intent::redact_url` cuts a URL after its path."
  - "Android: `Intent(Intent.ACTION_VIEW, Uri.parse(url))`, with
    `FLAG_ACTIVITY_NEW_TASK` when started from the Application context;
    `ActivityNotFoundException` when nothing handles it; from Android 10
    an app cannot start an activity while it is not visible; from Android
    11 `resolveActivity` needs a `<queries>` entry to see other apps, so
    the call is tried and the exception caught instead."
related:
  - docs/design/20261022_05_URL_SCHEMES_PLAN.md (the inbound side)
  - libdevice docs/design/20261023_01_OPEN_LINK_PLAN.md (the same API on
    the Mac)
  - libmorph docs/design/20261023_02_OPEN_LINK_PLAN.md (wraps both)
---

# Opening a link in another app

## 1. Goal

The app asks Android to open a web link, and the phone's browser (or the
app registered for that link) opens it. One call, from any thread, with
an error when it cannot be done.

## 2. API

Core, no Cargo feature. Platform-free types in `libmobile::link`, the
call in `android::link`; the Kotlin is a static method on a new core
`bridge.LinkPlugin`, reached through the Application context like the
secure store.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenLinkError {
    /// Not an absolute http or https URL, or it holds whitespace or
    /// control characters.
    BadUrl,
    /// No app on the device handles it.
    NoHandler,
    /// The app is not visible, and Android does not let it start one.
    NotVisible,
    Platform(String),
}

/// Open `url` (http or https) in the app that handles it. Any thread.
pub fn open_link(url: &str) -> Result<(), OpenLinkError>;

/// The check open_link makes before sending anything. Platform-free.
pub fn check_link(url: &str) -> Result<(), OpenLinkError>;
```

- **Schemes.** `http` and `https` only, matched case-insensitively, with
  a non-empty host. Other schemes (`mailto:`, `tel:`, `geo:`, app
  schemes) are left to a later amendment, each when an app needs it;
  `file:`, `content:`, `intent:` and `javascript:` are never accepted
  (section 4, question 1).
- **Starting.** Through the current activity when there is one, on its UI
  thread; otherwise from the Application context with
  `FLAG_ACTIVITY_NEW_TASK`, where Android 10+ refuses it while the app is
  hidden (`NotVisible`). `ActivityNotFoundException` is `NoHandler`. The
  call returns once the intent is sent; it does not wait for the browser.
- **Logging.** libmobile logs `libmobile: open link <redacted url>` at
  debug, using `intent::redact_url`, so a query or fragment never
  appears. `IntentData` is not involved.

## 3. Steps

1. **The call.** `link::check_link` with host tests (http and https,
   upper-case schemes, a missing host, whitespace, control characters,
   `file:`, `intent:`, `javascript:`, an empty string), `LinkPlugin`, and
   `android::link::open_link`.
   **Done when:** `cargo test --workspace` passes, clippy is clean on the
   host and on Android, and a generated project builds.
2. **On the S23.** The `wgpu` example opens `https://example.com/a?k=secret`
   on a tap (or `--es cmd open-link`).
   **Done when:** on the owner's S23 Ultra, the browser opens the page;
   logcat shows `…/a<redacted>` and never `secret`; with the browser
   disabled (`pm disable-user` on it, restored after), a second link gives
   `NoHandler` if no other app takes it, or opens in the next handler,
   whichever happens is recorded; a call from a background thread while
   the app is hidden gives `NotVisible`.
3. **Release** in the next libmobile release, on the owner's word.
   Additive: a new module, type and functions.

## 4. Open questions for the owner

1. **Schemes.** http and https only for now, as section 2 says?
   Recommended: yes; handlr needs only http.
2. **No Cargo feature.** The call is small and needs no permission or
   manifest entry. Recommended: core, like the intent bridge.
