---
title: The device's vendor, make and model
date: 2026-10-24
status: Implemented
covers:
  - "`android::build_info() -> BuildInfo { manufacturer, brand, model }`,
    from `android.os.Build`, and nothing that identifies the person or the
    unit"
sources:
  - "Owner (2026-10-24, relayed by the handlr session): \"needs libmorph
    support, does not expose private identifiers just vendor, make and
    model\". handlr uses the vendor for vendor-specific notes in its
    settings, such as Samsung's app sleeping. For the owner's approval in
    the libmorph session; this is the Android half."
  - "libmobile today: `BatteryPlugin.manufacturer()` reads
    `Build.MANUFACTURER` for `BatteryState::manufacturer` (feature
    `battery`). Nothing else reads `Build`."
  - "Android: `Build.MANUFACTURER` (\"samsung\"), `Build.BRAND`
    (\"samsung\"), `Build.MODEL` (\"SM-S918U\" on the owner's S23 Ultra) are
    public static fields that need no permission. `Build.getSerial()`,
    `Settings.Secure.ANDROID_ID`, the IMEI and the advertising ID identify
    the unit or the person."
related:
  - libmorph docs/design/20261024_02_DEVICE_INFO_PLAN.md (wraps it)
  - docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md (`BatteryState::manufacturer`)
---

# The device's vendor, make and model

## 1. API

Core, no feature. The platform-free type in `libmobile::build_info`, the
call in `android::build_info`, read through a static method on a core
`bridge.BuildInfoPlugin`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuildInfo {
    /// Build.MANUFACTURER ("samsung").
    pub manufacturer: String,
    /// Build.BRAND ("samsung").
    pub brand: String,
    /// Build.MODEL ("SM-S918U").
    pub model: String,
}

/// The device's manufacturer, brand and model. Any thread; read once and
/// kept. None only if the JNI call fails (logged at warn).
pub fn build_info() -> Option<BuildInfo>;
```

- **Only these three fields**, each a public `Build` constant that needs
  no permission. libmobile never reads the serial, `ANDROID_ID`, the IMEI,
  the advertising ID, the fingerprint, or anything else that identifies
  the unit or its owner, and the rustdoc says so. Adding a field is a
  plan amendment.
- A missing value is an empty string.
- `BatteryState::manufacturer` is unchanged; it reads the same constant.

## 2. Steps

1. **The call**, with the `wgpu` example logging it at start.
   **Done when:** clippy and tests pass on the host and Android, and on
   the owner's S23 the example logs `samsung`, `samsung` and the model
   (`getprop ro.product.model` gives the value to compare).
2. **Release** in the next libmobile release, on the owner's word.
   Additive.

## Amendment — 2026-10-24 13:06 EDT: approved, for 0.2.4

The owner approved this plan with libmorph's
`docs/design/20261024_02_DEVICE_INFO_PLAN.md` on 2026-10-24 ("approve,
plan a 0.4.0 with what we've planned"). It ships in libmobile 0.2.4, with
the trim-memory event (`docs/design/20261024_01_TRIM_MEMORY_PLAN.md`),
so libmorph 0.4.0 can wrap both. `build_info()` keeps its name. The
status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-24 13:08 EDT: implementation has started; the document is now immutable

Step 1 starts with this commit.

## Amendment — 2026-10-24 13:32 EDT: implemented

Every step is done:
1. `0586bec`, `2df80ad` (the `wgpu` example) and `ded93e9` (README),
   with `806cb05` (CHANGES: choices) and `8a2b041` (CHANGES: the S23,
   where it logged `samsung`, `samsung`, `SM-S918U`, matching
   `getprop`).
2. libmobile 0.2.4 (`27f3f74`, tag `v0.2.4`), on the owner's word.
