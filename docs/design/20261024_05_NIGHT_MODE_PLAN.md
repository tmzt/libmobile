---
title: The system's night mode
date: 2026-10-24
status: Draft
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
