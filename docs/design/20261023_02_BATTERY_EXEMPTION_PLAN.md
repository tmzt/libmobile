---
title: Exempting the app from battery optimization and app sleeping
date: 2026-10-23
status: In implementation
covers:
  - "A feature `battery` of its own: `android::battery`, the
    battery-optimization dialog, the settings pages, and its state"
  - "Samsung's app sleeping: the app's own settings page, and Samsung's
    battery page by its internal name, best-effort with a fallback"
  - "`SystemEvent::BatteryExemption(bool)` when the exemption changes"
  - What the foreground service plan's section 5 becomes
sources:
  - "Owner (2026-10-23, relayed by the handlr session): \"is there an
    intent to allow the user to activate it?\", then \"yes, separate
    plan\". Plan only; the owner approves it in the libmorph session.
    libmorph's wrapper follows under its own plan."
  - "handlr's request (2026-10-23): the standard dialog with a check first
    so the app only asks while the exemption is off; the settings list as
    the alternative; Samsung's \"Never sleeping apps\", \"Put unused apps
    to sleep\" and \"Deep sleeping apps\", which have no public intent;
    an API the app calls when it decides to (handlr: on first pairing, and
    from the foreground service's notification), returning the state, and
    an event when it changes. handlr is sideloaded, so Play's policy on the
    dialog does not bind it."
  - "docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md, section 5 and
    commit a81a650: `android::battery` with `is_ignoring_optimizations`,
    `request_ignore_optimizations` and `open_settings`, returning
    `ServiceError`, under feature `foreground-service`, and
    `Project::request_ignore_battery_optimizations`. Not yet released."
  - "Its S23 results so far (CHANGES 1395d0f): with Samsung's default
    battery setting (\"Optimized\", not exempt), lines from the background
    example arrived every 30 to 31 s in the background, and 34 to 39 s
    apart with the screen off."
  - "Android: `Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` with
    `package:<package>` and the permission
    `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`;
    `Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`;
    `Settings.ACTION_APPLICATION_DETAILS_SETTINGS`;
    `PowerManager.isIgnoringBatteryOptimizations(package)` (API 23);
    `ActivityManager.isBackgroundRestricted()` (API 28, the \"Restricted\"
    battery setting); `PackageManager.resolveActivity`;
    `Build.MANUFACTURER`; starting an activity needs the app to be
    visible."
related:
  - docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md (the service keeps
    the process; the exemption can keep its network in Doze)
  - libmorph docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md (wraps
    both)
---

# Exempting the app from battery optimization and app sleeping

## 1. Goal

From inside the app, when the app decides to, the user can:
- allow the app to ignore battery optimization, through Android's own
  Allow/Deny dialog;
- reach the pages where the same setting, and on Samsung the app-sleeping
  lists, are changed.

The app can read the current state at any time and hears when it
changes. The foreground service keeps the process running; the exemption
can keep its network working through Doze, which is what step 3 measures.

## 2. Feature and existing code

A new feature `battery`, with no Kotlin of its own beyond a small plugin,
`battery.BatteryPlugin`. `foreground-service` no longer gates
`android::battery`, so an app without the service can use it.

The foreground service plan's section 5 is replaced by this plan, before
any release: the three functions already built (a81a650) move to
`battery`, return `BatteryError` instead of `ServiceError`, and their
Kotlin moves out of `ForegroundServicePlugin` into `BatteryPlugin`.
`Project::request_ignore_battery_optimizations` stays as it is, and is
reached from libmorph-build through libmobile's `Project`, like the other
builder methods. This needs an amendment to the foreground service plan,
written when this plan is approved (section 7, question 1).

## 3. API

`src/android/battery.rs`, platform-free types in `libmobile::battery`.
Every call is synchronous and works from any thread through the
Application context. Calls that open a page need the app to be visible.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatteryState {
    /// PowerManager.isIgnoringBatteryOptimizations(package).
    pub ignoring_optimizations: bool,
    /// ActivityManager.isBackgroundRestricted(): the "Restricted" battery
    /// setting, which stops background work even with a service.
    pub background_restricted: bool,
    /// Build.MANUFACTURER, as reported ("samsung" on the S23).
    pub manufacturer: String,
}

impl BatteryState {
    /// The manufacturer is Samsung, so app sleeping applies.
    pub fn is_samsung(&self) -> bool;
}

/// Which page opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Page {
    /// ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS: every app's setting.
    OptimizationList,
    /// ACTION_APPLICATION_DETAILS_SETTINGS for this app.
    AppDetails,
    /// The vendor's battery page (Samsung: app sleeping).
    Vendor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatteryError {
    /// request_ignore_optimizations without
    /// Project::request_ignore_battery_optimizations.
    NotDeclared,
    /// A page was asked for while the app was not visible.
    NotVisible,
    Platform(String),
}

pub fn state() -> Result<BatteryState, BatteryError>;
pub fn is_ignoring_optimizations() -> Result<bool, BatteryError>;
/// The Allow/Deny dialog. Does nothing (Ok) if the app is already exempt.
pub fn request_ignore_optimizations() -> Result<(), BatteryError>;
/// The list of every app's optimization setting.
pub fn open_settings() -> Result<Page, BatteryError>;
/// This app's settings page; its Battery entry is one tap away.
pub fn open_app_details() -> Result<Page, BatteryError>;
/// The vendor's battery page if one is known and resolves on this device,
/// otherwise this app's settings page. Returns which opened.
pub fn open_vendor_battery() -> Result<Page, BatteryError>;
```

- The answer to the dialog, and changes made in settings, are not
  reported by Android. libmobile reads `state()` again on each
  `Visible(true)` and sends `SystemEvent::BatteryExemption(bool)` when
  `ignoring_optimizations` differs from the last value it read (section
  4).
- **Samsung's page.** One UI has no public intent for app sleeping.
  `open_vendor_battery` tries, in order, components of Samsung's Device
  Care app (`com.samsung.android.lool`), checking each with
  `resolveActivity` and starting the first that resolves. The candidate
  list is a constant in `BatteryPlugin`, starting with
  `com.samsung.android.sm.battery.ui.BatteryActivity` and
  `com.samsung.android.sm.ui.battery.BatteryActivity`. These names are
  undocumented and have moved between One UI versions, so step 3 records
  which one opens on the S23 (One UI on Android 16), and the list is
  corrected then. If none resolves, or the manufacturer is not Samsung, it
  opens the app's settings page and returns `Page::AppDetails`.
- What to tell the user is the app's job. For example, on Samsung:
  "Battery → Unrestricted, and add <app> to Never sleeping apps".
- **Play.** Google Play allows the direct dialog only for some kinds of
  app. A Play build uses `open_settings` or `open_app_details`; the
  rustdoc says so. Sideloaded apps such as handlr are not bound by it.

## 4. Event

Under `system-events` (with `battery`):

```rust
SystemEvent::BatteryExemption(bool) // ignoring_optimizations changed
```

`SystemEvent` is `#[non_exhaustive]`, so this is additive. The first
`subscribe()` reports the value at that moment, as it does for Doze and
the network. `current()` includes it.

## 5. Example

The `background` example gains:
- on its first resume, after starting the service, `state()` is logged,
  and if the app is not exempt, `request_ignore_optimizations()`;
- `--es cmd battery-settings`, `app-details` and `vendor-battery` open
  the pages and log which `Page` opened;
- every `BatteryExemption` event is logged.

It declares `request_ignore_battery_optimizations()`.

## 6. Steps

1. **The feature and the API.** Feature `battery`, `BatteryPlugin`,
   `android::battery` as in section 3 (moving the three existing
   functions), `BatteryExemption` with the re-check on `Visible(true)`,
   and the foreground service's code no longer holding the battery calls.
   Host tests for what is platform-free (`is_samsung`, the event's
   change detection).
   **Done when:** `cargo test --workspace` passes, clippy is clean on the
   host and on Android with `battery`, `battery,system-events` and every
   feature, and the generated project of the `background` example builds.
2. **The example** (section 5).
   **Done when:** it builds and runs on the S23.
3. **On the S23.**
   **Done when:** on the owner's Galaxy S23 Ultra:
   - with the app not exempt, the dialog appears on the first resume;
     Allow makes `dumpsys deviceidle whitelist` list the package, and the
     example logs `BatteryExemption(true)` on return;
   - removing it (`dumpsys deviceidle whitelist -<package>`) and returning
     to the app logs `BatteryExemption(false)`; Deny leaves it false;
   - `battery-settings` and `app-details` open their pages, and
     `vendor-battery` opens Samsung's battery page, with the component
     that opened recorded (`dumpsys activity top`);
   - with the exemption on, the foreground service plan's step 4 Doze
     check (`dumpsys deviceidle force-idle`, then `unforce`) is repeated,
     and whether lines arrived in Doze, with and without it, is recorded.

   The results go in CHANGES.md.
4. **Release** with the foreground service in 0.2.3, on the owner's word
   (section 7, question 2). Additive: a new feature, module, types and an
   event variant; the moved functions were never released.
   **Done when:** 0.2.3 is tagged and its `VERSIONS.md` entry lists
   `battery`.

libmorph wraps it as `libmorph::battery` (the same on every host,
`Unsupported`-style errors off Android) under an amendment to its
foreground service plan or a plan of its own, once this one is approved.

## 7. Open questions for the owner

1. **Moving `android::battery`.** This plan takes the battery calls out
   of the foreground service plan's section 5 and its feature, as section
   2 says, and that plan gets an amendment saying so. Recommended: yes,
   since nothing is released.
2. **One release.** Ship this with the foreground service as 0.2.3, or
   release the service first as 0.2.3 and this as 0.2.4? Recommended: one
   release, since the service's background behaviour on Samsung depends
   on these settings.
3. **Samsung's internal page.** Best-effort by component name, falling
   back to the app's settings page, as section 3 says. Recommended: yes;
   the fallback is one tap away from the same setting.

## Amendment — 2026-10-23 22:02 EDT: approved

The owner approved this plan on 2026-10-23 ("approve plans"), without
separate answers to section 7, so its recommendations stand:
1. `android::battery` moves to the feature `battery`, out of the
   foreground service plan's section 5; that plan gets an amendment
   saying so.
2. It ships with the foreground service in libmobile 0.2.3.
3. Samsung's battery page is opened best-effort by component name,
   falling back to the app's settings page.

The status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-23 22:30 EDT: implementation has started; the document is now immutable

Implementation has started: step 1 (the feature and the API) starts with
this commit. The status is now `In implementation`, and this document is
immutable. Departures are recorded in CHANGES.md.

## Amendment — 2026-10-23 23:37 EDT: released in 0.2.3; step 3 still open

Steps 1 and 2 are `e76d8f7`, `a33edb5` and `6ea2f63` (departures
`9c2a646`, approved). Step 4 is libmobile 0.2.3 (`2c50e9d`, tag
`v0.2.3`), released on the owner's word before step 3 is complete.
Step 3's results are in CHANGES `55d96bb`: everything passed except the
Doze comparison, which needs the screen off and so the owner's hands. The
status stays `In implementation` until it passes.
