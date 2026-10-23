# CHANGES

Planned work, rule changes, and departures from frozen plans, appended
oldest first under timestamped headings. Entries are never edited.

## 2026-10-16 09:15 EDT — Versions and tags

- Owner's instruction (2026-10-16): apply libcontrols' versioning rules to
  libmobile, recorded here rather than in a plan. AGENTS.md gains
  "Versions and tags": the version commit is tagged `v<version>`,
  `VERSIONS.md` lists the public API changes of each version, `0.x`
  bumps the minor version for anything not strictly additive and the patch
  version otherwise, and incompatible dependency bumps propagate up to
  libmorph and libmorph-build.
- `VERSIONS.md` starts with 0.1.0, recorded retroactively, and the tag
  `v0.1.0` is added on `21938c4`, the commit that set it.
- The `cpal` feature (`docs/design/20261016_01_CPAL_FEATURE_PLAN.md`,
  `ee0f098`) is strictly additive, so it is released as 0.1.1.

## 2026-10-22 07:05 EDT — Runtime permission requests

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` (Draft).

Requested by the owner after libmorph's audiocapture example ran on the
Galaxy S23 with `RECORD_AUDIO` granted through `adb`, since libmobile
cannot ask for a permission at runtime. A new core
`bridge.PermissionPlugin` and an `onRequestPermissionsResult` hook on
`MobileActivityPlugin` let Rust call
`android::request_permissions` and `permission_granted`, with the
answer delivered as `MobileEvent::Permissions`. libmorph's audio capture
plan depends on it. To be released as 0.1.2 (strictly additive).

## 2026-10-22 07:18 EDT — Runtime permissions on the S23, and a stale Kotlin build

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` (In
implementation), step 3.

On the owner's Galaxy S23 Ultra the `wgpu` example showed the system
dialog on a tap and logged `permissions [("android.permission.RECORD_AUDIO",
true)]` when allowed. libmorph's audiocapture example, on this branch, saw
both answers: an interrupted request (the app reinstalled while the dialog
was up) arrived as denied, and a later one as granted. An explicit "Don't
allow" is not yet checked.

Found while building libmorph against this branch: when a build switches
to another copy of libmobile, its `OUT_DIR` and so the staged Kotlin
source root (`libmobile.gradle`'s `java.srcDir`) move. Gradle's
incremental Kotlin compile in the generated project then fails with
"Unresolved reference 'WindowInsetsPlugin'" (and the other bridges) until
`gradle clean`. Not fixed here; a fix would belong in its own plan.

## 2026-10-22 07:24 EDT — cpal feature: ndk's AAudio API

Follows `docs/design/20261016_01_CPAL_FEATURE_PLAN.md` (Implemented),
agreed with the owner in the libmorph session.

The `cpal` feature now enables `ndk/audio` itself, so
`libmobile::android::ndk::audio` (AAudio) is available whenever cpal is.
Before, it was enabled only because cpal's own ndk dependency asks for
it. libmorph opens its Android capture stream through it, to set the
AAudio input preset that cpal cannot (libmorph
`docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`, capture modes). Released
with 0.1.2; strictly additive.

## 2026-10-22 07:28 EDT — cpal feature: ndk's API 28 calls too

Corrects the entry "cpal feature: ndk's AAudio API" above. ndk 0.9 gates
`AudioInputPreset` and `AudioStreamBuilder::input_preset` behind its
`api-level-28` feature, so `cpal` enables `ndk/api-level-28` as well as
`ndk/audio`. An app on this feature then needs API 28. The projects
`libmobile::gradle` generates target API 29.

## 2026-10-22 07:53 EDT — Runtime permissions: request ids, a queue, and the rationale check

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` (In
implementation), amended on the owner's instruction.

Before libmorph routes permission answers to apps, each request gets an
id that its answer carries. `PermissionPlugin` queues requests so a second
one no longer interrupts the first. `should_show_rationale` lets an app
tell a request that will show a dialog from one Android will deny
silently. Changes the unreleased `request_permissions` and
`PermissionsOutcome`; still released as 0.1.2.

## 2026-10-22 07:55 EDT — Permission ids and the queue on the S23

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md`, step 4.

On the owner's Galaxy S23 Ultra, with both permissions revoked, one tap in
the `wgpu` example made two requests back to back (`RECORD_AUDIO` as
request 1, `CAMERA` as request 2). The dialogs came one after the other,
and the answers arrived in order with their own ids and names. Both were
allowed; the owner was using the phone at the same time, so the denied
answer and `should_show_rationale` are not yet checked on a device.
`PermissionsOutcome::from_result` and `denied` became crate-private.

## 2026-10-22 08:06 EDT — Audio capture with input presets

Plan: `docs/design/20261022_02_AUDIO_CAPTURE_PLAN.md` (Draft), to be
implemented now on the owner's instruction, before libmorph switches to it.

libmorph had opened AAudio capture streams itself through
`libmobile::android::ndk` to set the input preset cpal cannot. The owner
pointed out that this belongs in libmobile, which libmorph wraps. libmobile
gains `android::CaptureStream` (open with a preset, change it while
recording, stop and start) and a platform-free `InputPreset`, under the
`cpal` feature. This revises the cpal plan's "no audio API of its own"
for the one thing cpal cannot do. A new `audiocapture` example shares the
`wgpu` example's GPU code and makes its plasma respond to the microphone.
The `wgpu` example stops asking for permissions it does not use, and the
permission checks move to the new example. Released with the runtime
permissions as 0.1.2.

## 2026-10-22 08:25 EDT — 0.2.0 released, not 0.1.2

Plans: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` and
`docs/design/20261022_02_AUDIO_CAPTURE_PLAN.md` (both In implementation).

The owner asked for the work to be merged with proper versioning. Both
plans and the entries above expected 0.1.2. But the `cpal` feature now
turns on `ndk/api-level-28`, so an app built with it needs Android 9, which
a caller can observe. Under "Versions and tags" rule 4 that is not strictly
additive, so this is 0.2.0 (tag `v0.2.0`). libmorph and libmorph-build
take it with a minor bump of their own (rule 5).

Not yet checked on a device, and left open in the plans: an explicit
"Don't allow" with the rationale and a second request, and the plasma
responding to sound.

## 2026-10-22 18:19 EDT — Secure blob store (proposed)

Plan: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (Draft). Requested
by the handlr session with the owner's approval: its libmorph companion
app needs a Noise private key and a server record kept on the device and
protected at rest, with no file-based fallback. The proposal is a
`secure-store` feature that does envelope encryption under an Android
Keystore AES-256-GCM key, using StrongBox where available, with the
ciphertext in no-backup storage. Proposed only, awaiting the owner.

## 2026-10-22 19:10 EDT — Key store for adapter-defined keys (proposed)

Plan: `docs/design/20261022_04_KEY_STORE_PLAN.md` (Draft), step 1 of
libmorph's `20261022_04_OPAQUE_KEYS_PLAN.md`. An adapter defines its
private key type `K` (implementing `SecretKey`) and its public opaque
type `P`. The store generates, seals, loads and validates `K`, and hands
out a `KeyHandle`. Sealing needs a `SealToken` only libmobile can make.
Keys have their own API and storage (a second space in
`SecureStorePlugin`), separate from the blob store. libdevice will
implement the same API on the desktop, and libmorph wraps both.

## 2026-10-22 20:32 EDT — Secure store: DeviceLocked and Platform

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (In implementation),
and libmorph `docs/design/20261022_06_KEY_STORE_PLAN.md`, step 1.

A departure from the secure-store plan, before any release:
- `SecureStoreError::Locked` is `DeviceLocked`, and the plugin's error
  kind `locked:` is `device-locked:`. Owner: "Locked and DeviceLocked are
  different state (like in Connectbot, where keys can be unlocked to use
  them)". `Locked` is left for the key store's app-unlock flag.
- `Jni` is `Platform`.

## 2026-10-22 20:36 EDT — Secure store: the Application context, no `&AndroidApp`

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (In implementation),
and libmorph `docs/design/20261022_06_KEY_STORE_PLAN.md`, step 2.

A departure from the secure-store plan, before any release. The key store
shares `SecureStorePlugin` and needs it to work without an activity, so
the plugin now takes a `Context`. Rust passes the Application that
android-activity registers with `ndk-context`, which is a new direct
dependency so libmobile shares android-activity's copy. The blob functions
drop `&AndroidApp`, as in `secure_store::get(name)`, and
`android::SecureStore` wraps them for libdevice's shape.

On the owner's S23 Ultra:
- the key is in StrongBox;
- the counter goes from 1 to 2 across a force-stop;
- a worker thread with no `AndroidApp` wrote a blob that the main thread
  read back (5 ticks);
- with the screen locked, an `unlocked_only` put reported
  `DeviceLocked`.

Not yet checked: reading an `unlocked_only` blob written while unlocked,
which needs the owner to unlock the phone.
