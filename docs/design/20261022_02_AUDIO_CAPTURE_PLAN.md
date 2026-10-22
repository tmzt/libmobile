---
title: Audio capture with input presets
date: 2026-10-22
status: In implementation
covers:
  - A capture stream on AAudio (`android::audio_capture::CaptureStream`),
    with the input preset cpal cannot set, changeable while it runs
  - A platform-free `InputPreset` for wrappers to map onto
  - An `audiocapture` example that shares the `wgpu` example's GPU code and
    makes its plasma respond to the microphone
sources:
  - Owner (2026-10-22), in the libmorph session, after libmorph's own
    capture opened AAudio streams through `libmobile::android::ndk`
    directly: "libmorph is supposed to be a wrapper for libmobile so the
    features we added should have been in libmobile first", agreeing to
    this outline ("yes, go ahead in that order").
  - Owner, on the example: "we could add audiocapture but reuse wgpu (shared
    module, like plasma) and a sound-responsive mode", after noting that the
    `wgpu` example asked for permissions it does not use.
  - docs/design/20261016_01_CPAL_FEATURE_PLAN.md, section 1: "libmobile adds
    no audio API of its own". cpal 0.18.2 cannot set an AAudio input preset,
    so this is functionality cpal does not expose, which libmobile provides
    by design.
  - "ndk 0.9 `audio` with `api-level-28`, enabled by the `cpal` feature
    (CHANGES.md 2026-10-22): `AudioStreamBuilder::input_preset`, the data
    callback `FnMut(&AudioStream, *mut c_void, i32) -> AudioCallbackResult`,
    and `request_stop` for input streams, which have no pause."
  - "libmorph's `src/audio.rs` `aaudio` module (its commit `7b140e4`), the
    code this plan moves down: open, fall back to the default preset,
    reopen with the same callback and format, stop and start."
related:
  - docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md (RECORD_AUDIO; its
    device checks move to this plan's example)
  - libmorph docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md (wraps this on
    Android)
---

# Audio capture with input presets

## 1. Goal

An Android app on libmobile can record from the microphone with a chosen
AAudio input preset (voice communication for echo cancellation,
unprocessed, camcorder, ...) and change it while recording, without
writing AAudio code. libmorph wraps it instead of driving AAudio itself.

cpal stays the way to play audio, and to record when the preset does not
matter.

## 2. Types

In `src/audio_capture.rs`, platform-free, under the `cpal` feature,
re-exported from the crate root:

```rust
/// AAudio's input presets (`AAUDIO_INPUT_PRESET_*`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum InputPreset {
    Generic,            // 1, the source dumpsys reports as MIC
    Camcorder,          // 5
    VoiceRecognition,   // 6, AAudio's default
    VoiceCommunication, // 7: echo cancellation, noise suppression, gain control
    Unprocessed,        // 9, where the device offers it
    VoicePerformance,   // 10, API 29
}

/// Why a capture stream did not open or start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureError(pub String);   // Display, Error

/// The stream's format, chosen by AAudio when it opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureFormat { pub sample_rate: u32, pub channels: u16 }
```

The raw values are kept in one table with `to_raw`/`from_raw`, unit-tested,
so `VoicePerformance` needs no further ndk API-level feature.

## 3. The stream

In `src/android/audio_capture.rs`, re-exported as
`libmobile::android::CaptureStream`:

```rust
pub struct CaptureStream { .. }

impl CaptureStream {
    /// Open the default input as f32 with `preset` (`None`: AAudio chooses),
    /// stopped. The callback gets interleaved f32 frames and when the first
    /// was captured, on a monotonic clock that starts at `open` and keeps
    /// running across `set_preset` and `stop`. It runs on AAudio's thread
    /// and must not block. A preset that fails to open falls back to
    /// `None`, with a warning.
    pub fn open(
        preset: Option<InputPreset>,
        callback: impl FnMut(&[f32], Duration) + Send + 'static,
    ) -> Result<Self, CaptureError>;

    pub fn format(&self) -> CaptureFormat;
    /// The preset AAudio reports (`VoiceRecognition` when `None` was asked).
    pub fn preset(&self) -> InputPreset;
    /// Close and reopen with `preset`, at the same format, with the same
    /// callback, started if it was started. Returns the preset granted. On
    /// failure the stream stays closed and `start` reports it.
    pub fn set_preset(&self, preset: Option<InputPreset>) -> Result<InputPreset, CaptureError>;

    pub fn start(&self) -> Result<(), CaptureError>;
    /// AAudio input streams have no pause: this stops, and `start` resumes.
    pub fn stop(&self) -> Result<(), CaptureError>;
}
```

Inside, the callback lives in an `Arc<Mutex<_>>` that only AAudio's
callback locks while a stream is open. Closing a stream waits for its
callback to return, so `set_preset` can hand the same callback to the next
one. The methods take `&self` (`RefCell` and `Cell` inside), so a wrapper
can hold the stream behind a shared reference.

Like the rest of `android`, it needs `RECORD_AUDIO` declared and granted;
without it `open` fails.

## 4. The examples

**Shared module.** The `wgpu` example's device, pipeline and swapchain code
moves from `examples/wgpu/src/lib.rs` into `examples/wgpu/src/gpu.rs`, next
to `uniforms.rs` and `shader.wgsl`. `Uniforms` gains `level: f32` in place
of half of `_pad` (same size and layout). The shader adds to its usual
picture in proportion to `level`, brightening and quickening the plasma, so
at 0 it draws as before.

**`wgpu`.** Back to how it is on `main`: no permissions and no requests.
It uses `gpu.rs` and leaves `level` at 0.

**`audiocapture`** (`examples/audiocapture`, crate
`libmobile-example-audiocapture`, application id
`dev.highbay.libmobile.example.audiocapture`, `RECORD_AUDIO` in its
manifest, libmobile's `cpal` feature):

- It includes `gpu.rs` and `uniforms.rs` from `../wgpu/src` with `#[path]`,
  so there is one copy of the plasma.
- On the first window it checks `permission_granted`, and asks with
  `request_permissions` if needed, logging the request id.
- When granted, it opens a `CaptureStream` with no preset and starts it on
  `Resume`, stopping it on `Pause`. The callback computes each block's RMS
  and stores it in an `AtomicU32`. Each tick eases `level` towards it
  (attack 30 ms, release 300 ms), scaled so ordinary speech reaches about 1.
- After a denial it logs `should_show_rationale`; a tap asks again.
- Once recording, a tap cycles the preset through `None` and the six
  presets, logging `preset: asked <preset>, got <preset>`.

`examples/build.sh audiocapture --install` builds it like the others.

## 5. Steps

1. **Types and stream.** Sections 2 and 3. Unit tests of the raw-value
   table.
   **Done when:** `cargo test --workspace` passes and Android clippy is
   clean with all features.
2. **Examples.** Section 4.
   **Done when:** both build for Android, and the `wgpu` example asks for
   nothing.
3. **On the S23.** **Done when:** on the owner's Galaxy S23, the
   `audiocapture` example asks for the microphone on first launch; once
   allowed, the plasma responds to speech; tapping through the presets
   shows each one's source in `dumpsys audio`; and after a denial a tap
   asks again.

## 6. Release

Additive (a feature-gated module, types and examples), released with the
runtime permissions as 0.1.2.

## Amendment — 2026-10-22 08:07 EDT: implementation has started; the document is now immutable

On the owner's instruction (2026-10-22), on the `runtime-permissions`
branch. Step 1 is in this commit.
