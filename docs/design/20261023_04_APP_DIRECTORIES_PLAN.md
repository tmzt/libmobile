---
title: The app's files and cache directories
date: 2026-10-23
status: Draft
covers:
  - "`android::dirs::files_dir()` and `cache_dir()`, from any thread"
sources:
  - "Owner (2026-10-23, relayed by the handlr session): \"libmorph should
    provide a persistent storage location on all platforms, with
    mkdir_all\". Plan first, for the owner's approval in the libmorph
    session. This is the Android half (libmorph AGENTS.md, Platform
    Layering)."
  - "libmorph today: `MorphContext::data_dir()` is
    `AndroidApp::internal_data_path()` on Android, read on the main
    thread at bring-up. There is no cache directory."
  - "Android: `Context.getFilesDir()` and `Context.getCacheDir()`; both
    private to the app; the system may delete the cache directory's files
    when storage is low. libmobile reaches the Application context from
    any thread through `jni_util::with_application_class` (the secure
    store's pattern)."
related:
  - libdevice docs/design/20261023_02_APP_DIRECTORIES_PLAN.md (the same
    API on the desktop)
  - libmorph docs/design/20261023_03_APP_STORAGE_PLAN.md (wraps both)
---

# The app's files and cache directories

## 1. API

Core, no Cargo feature. `src/android/dirs.rs`, with a static method pair
on the core `bridge.DirsPlugin`:

```rust
/// Context.getFilesDir(): private, kept until the app is uninstalled or
/// its data cleared. The same directory as AndroidApp::internal_data_path.
pub fn files_dir() -> Option<PathBuf>;
/// Context.getCacheDir(): private, and Android may delete its files when
/// storage is low.
pub fn cache_dir() -> Option<PathBuf>;
```

- Any thread, no `AndroidApp`. `None` only if the JNI call fails, which
  is logged at warn.
- Both directories exist when Android returns them; libmobile does not
  create subdirectories.

## 2. Steps

1. **The calls.** `DirsPlugin`, `android::dirs`, and the `wgpu` example
   logging both paths at start.
   **Done when:** clippy is clean on the host and on Android, tests pass,
   and on the owner's S23 the example logs
   `/data/user/0/<package>/files` and `/data/user/0/<package>/cache`,
   with `files_dir()` equal to `internal_data_path()`.
2. **Release** in the next libmobile release, on the owner's word.
   Additive.
