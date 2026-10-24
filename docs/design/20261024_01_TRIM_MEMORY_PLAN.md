---
title: Android's memory-trim levels
date: 2026-10-24
status: Draft
covers:
  - "`MobileEvent::TrimMemory(TrimLevel)`, from
    `ComponentCallbacks2.onTrimMemory`"
sources:
  - "libmorph docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md, stage 2
    (approved by the owner on 2026-10-24, \"the new hook in stages\"):
    handlr's companion app wants a graded memory-pressure event."
  - "libmobile today: `MobileEvent::LowMemory` from android-activity's
    `MainEvent::LowMemory`; nothing for `onTrimMemory`."
  - "Android: `Application.registerComponentCallbacks(ComponentCallbacks2)`;
    `onTrimMemory(int)` levels `RUNNING_MODERATE` (5), `RUNNING_LOW` (10),
    `RUNNING_CRITICAL` (15), `UI_HIDDEN` (20), `BACKGROUND` (40),
    `MODERATE` (60), `COMPLETE` (80); from API 34 only `UI_HIDDEN` and
    `BACKGROUND` are delivered. `adb shell am send-trim-memory <package>
    <level>` sends one."
related:
  - libmorph docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md
---

# Android's memory-trim levels

## 1. API

Core, no feature. A core `bridge.TrimMemoryPlugin` registers
`ComponentCallbacks2` on the Application when the activity is created and
queues each level into a mailbox, which the main loop drains like the
other bridges.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrimLevel {
    RunningModerate,
    RunningLow,
    RunningCritical,
    UiHidden,
    Background,
    Moderate,
    Complete,
}

// MobileEvent (#[non_exhaustive]) gains:
TrimMemory(TrimLevel),
```

- An unknown level maps to the nearest lower known one (for example 30
  to `UiHidden`), so a new Android level still arrives.
- `MobileEvent::LowMemory` is unchanged.
- With `system-events`, `SystemEvent::TrimMemory(TrimLevel)` also reaches
  subscribers on other threads.

## 2. Steps

1. **The event**, with host tests for the level mapping, and the `wgpu`
   example logging it.
   **Done when:** clippy and tests pass on the host and Android, and on
   the owner's S23 `am send-trim-memory <package> UI_HIDDEN` and
   `BACKGROUND` log those levels.
2. **Release** in the next libmobile release, on the owner's word.
   Additive.

## Amendment — 2026-10-24 12:17 EDT: approved

The owner approved this plan on 2026-10-24 ("approve them, the new hook
in stages"). Stage 1 (`on_low_memory`) is built first, in libmorph
only; stage 2 (trim levels) follows libmobile's trim-memory release. The
status stays `Draft` until the first commit that implements it.
