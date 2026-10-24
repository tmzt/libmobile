---
title: The system's night mode
date: 2026-10-24
status: In implementation
covers:
  - "`android::night_mode(&AndroidApp) -> Option<bool>`, from the
    configuration's `uiMode` night bits"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\", from the handlr
    session): its Appearance setting (System, Always light, Always dark)
    needs Android's dark mode for System, and a change when it changes
    while the app runs. For the owner's approval in the libmorph session;
    this is the Android half."
  - "libmobile today: `MobileEvent::ConfigChanged` when the configuration
    changes; the generated manifest's `configChanges` includes `uiMode`,
    so a night-mode switch arrives as `ConfigChanged` without recreating
    the activity. android-activity 0.6's `AndroidApp::config()` has
    `ui_mode_night()` (ndk's `UiModeNight`: `Any`, `No`, `Yes`)."
  - "libmorph AGENTS.md, Platform Layering: libmorph does not call
    android-activity or the NDK for such a feature itself."
related:
  - libmorph docs/design/20261024_05_DARK_MODE_PLAN.md (wraps it)
---

# The system's night mode

## 1. API

Core, no feature, in `src/android/mod.rs` (or a small `config` module):

```rust
/// Whether the system is in night (dark) mode: `Configuration.uiMode`'s
/// night bits. `None` when undefined. Read again after
/// `MobileEvent::ConfigChanged` to see a change.
pub fn night_mode(app: &AndroidApp) -> Option<bool>;
```

`UiModeNight::Yes` is `Some(true)`, `No` is `Some(false)`, anything else
`None`. No new event: a change arrives as the existing `ConfigChanged`.

## 2. Steps

1. **The call**, with the `wgpu` example logging it at start and on each
   `ConfigChanged`.
   **Done when:** clippy and tests pass on the host and Android, and on
   the owner's S23 `adb shell cmd uimode night yes` and `night no` (the
   setting restored afterwards) log `Some(true)` and `Some(false)` on the
   following `ConfigChanged`.
2. **Release** in the next libmobile release, on the owner's word.
   Additive.

## Amendment — 2026-10-24 15:37 EDT: the system bars' icons

The owner's direction on scope, relayed by the handlr session
(2026-10-24): the system bars "should follow our theme background and
titlebar colors", in this plan rather than a later one. Proposed for the
owner's approval; it adds to section 1 and step 1.

`bridge.WindowInsetsPlugin` already makes every generated app draw
edge-to-edge on every API level (`setDecorFitsSystemWindows(false)`), so
the app draws behind the status and navigation bars. From Android 15 (the
generated projects' target SDK) Android ignores the bars' background
colours anyway. So libmobile does not paint the bars; libmorph does, in
the app's own frame (its plan's amendment). libmobile adds the part only
the platform can do:

```rust
/// Dark (true) or light icons on the status and navigation bars,
/// against what the app draws behind them. Any thread; applied on the UI
/// thread (WindowInsetsController.setSystemBarsAppearance, API 30, and
/// the older View flags on API 29).
pub fn set_system_bar_icons(status_dark: bool, navigation_dark: bool);
```

It also turns off the system's contrast scrims
(`isStatusBarContrastEnforced`, `isNavigationBarContrastEnforced`, API
29) and makes both bars' own colours transparent, so on any API level
only what the app draws shows behind the bars. The current icons are kept
across `WindowCreated`, since a new window resets them.

Step 1 also checks, on the S23, with 3-button navigation and with
gestures: `set_system_bar_icons(true, true)` gives dark icons and
`(false, false)` light ones (`dumpsys window` shows the appearance
flags, and a screenshot shows the icons), with no scrim over the
navigation bar.

## Amendment — 2026-10-24 15:38 EDT: approved, for libmobile 0.2.6 and libmorph 0.4.3

The owner approved the dark mode plans in libmobile and libmorph on
2026-10-24 ("approve, yes"), with their system bars amendments and the
recommended answer to libmorph's question 1 (a hook, not a system event).
They are built and released with the notification plans, as libmobile
0.2.6 and libmorph 0.4.3. The status stays `Draft` until the first
commit that implements it.

## Amendment — 2026-10-24 15:53 EDT: implementation has started; the document is now immutable

Step 1 (the call, and the system bars' icons) is the first implementing commit.

## Amendment — 2026-10-24 16:22 EDT: released in 0.2.6; step 1's device checks still open

Step 1 is `20f5083` and `45097f7`, with CHANGES `1a3e6c5`. The scrims
are turned off and the bars made transparent from the first
`set_system_bar_icons` call rather than at start, so apps that never
call it are unchanged (a reading of the amendment, kept unless the owner
says otherwise). Step 2 is libmobile 0.2.6 (`f7954ed`, tag `v0.2.6`),
released on the owner's word before step 1's S23 checks, which need the
phone idle or the owner. The status stays `In implementation` until they
pass.
