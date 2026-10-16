---
title: cpal on Android
date: 2026-10-16
status: Draft
covers:
  - An optional `cpal` feature that adds cpal as an Android-only dependency
  - Re-exporting it as `libmobile::android::cpal`, next to
    `android_activity` and `ndk`
sources:
  - Owner's decision (2026-10-16), in the libmorph session: "libmobile own
    cpal on android and libmorph owns it on desktop/web, feature passed
    down to libmobile so that cpal dependency remains optional"
  - libmorph docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md,
    amendment "libmobile owns cpal on Android": libmorph's `audio` feature
    turns on `libmobile?/cpal` and uses this re-export on Android.
  - cpal 0.18.2 on Android: AAudio through `ndk` 0.9 (features `audio`,
    `api-level-26`), `ndk-context` for the `JavaVM`, and `jni` 0.22.
    These are the `ndk` and `jni` versions libmobile already links.
related:
  - libmorph docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md
    (depends on this plan: its step 0)
---

# cpal on Android

## 1. Goal

libmobile owns the Android platform layer, including the NDK. cpal reaches
the NDK's AAudio through the `ndk` crate, so on Android cpal comes through
libmobile, the same way `android_activity` and `ndk` do. A wrapper crate
such as libmorph then uses the exact copy libmobile links and does not
declare cpal for Android itself.

The dependency is optional. Without the feature, libmobile does not depend
on cpal and nothing changes.

libmobile adds no audio API of its own: no stream, no mixer, no lifecycle
handling. The app or wrapper opens and pauses its stream through cpal.

## 2. Manifest

```toml
[features]
# cpal, re-exported as `libmobile::android::cpal` (AAudio output and
# input). No Kotlin plugin.
cpal = ["dep:cpal"]

[target.'cfg(target_os = "android")'.dependencies]
cpal = { version = "0.18.2", optional = true }
```

- cpal turns on `ndk`'s `audio` and `api-level-26` features. Feature
  unification applies them to libmobile's `ndk` too. The generated Gradle
  project's `minSdk` defaults to 29, so API level 26 is already met.
- cpal is not a Kotlin plugin feature, so it is not added to
  `codegen::PLUGINS` and not forwarded in `DEP_*_FEATURES`. The comment
  on the `known_features_match_cargo_manifest` test changes to say that
  it lists the plugin features only.

## 3. Re-export

In `src/android/mod.rs`, with the other re-exports:

```rust
#[cfg(feature = "cpal")]
pub use cpal;
```

## 4. Steps

1. **Feature and re-export.** Sections 2 and 3; the README's feature table
   gets a `cpal` row and its layout list is unchanged (no Kotlin
   directory).
   **Done when:** `cargo fmt --check`, `cargo clippy --all-targets`
   and `cargo test` pass on the host, and `cargo clippy --target
   aarch64-linux-android` passes with and without `--features cpal`,
   with one `ndk` and one `jni` in `Cargo.lock`.

## Amendment — 2026-10-16 09:10 EDT: Approval

The owner approved this plan (2026-10-16) and asked for it to be
implemented now.
