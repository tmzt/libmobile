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

## 2026-10-22 20:58 EDT — Unlock-required keys and blobs on the S23

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` and libmorph
`docs/design/20261022_06_KEY_STORE_PLAN.md` (steps 1 and 4).

With the owner's S23 Ultra unlocked (`deviceLocked=0`), the `securestore`
example generated key `device-unlocked` (`FLAG_DEVICE_UNLOCK_REQUIRED`),
used it inside `with_key`, and wrote and read back the `unlocked_only`
blob. After the screen was turned off over adb (`deviceLocked=1`), the
worker thread's next reads of both gave `DeviceLocked`. Key `device`
(no flags) loaded with the same fingerprint as before.

## 2026-10-22 23:30 EDT — URL schemes and a redacted intent log (approved)

Plan: `docs/design/20261022_05_URL_SCHEMES_PLAN.md` (Draft, approved by
the owner with libmorph's `docs/design/20261022_07_URL_SCHEMES_PLAN.md`,
which wraps it). `gradle::Project::url_scheme` declares VIEW intent
filters, and the intent log stops showing URLs past their path, so a
pairing secret in a fragment never reaches logcat.

## 2026-10-22 23:33 EDT — URL schemes on the S23

Plan: `docs/design/20261022_05_URL_SCHEMES_PLAN.md` (steps 1 and 2).

Step 1 is `1d9efda` and `5b37af0`. The `wgpu` example's merged
manifest has the `libmobile-wgpu` VIEW filter, and `assembleDebug`
builds it.

On the owner's S23 Ultra, with `am start -a android.intent.action.VIEW`:
- `libmobile-wgpu://example.ts.net:7347/pair#secret` cold-started the
  app (`LaunchState: COLD`), and it logged
  `…/pair<redacted>` with `is_new=false`;
- a link sent while the app was in front, and one sent after Home, both
  reached the running activity through `onNewIntent` (`is_new=true`).
  `dumpsys activity activities` still showed one activity in one task
  (`t4968`).

`secret` appeared in logcat only in adbd's record of the shell command,
never from the app. `singleTop` is enough, so the plan's open question
does not arise.

## 2026-10-23 18:06 EDT — Foreground service, notification controls and system events (proposed)

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (Draft). Written
at the owner's request, relayed by the handlr session, as the libmobile
half of libmorph's `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`,
with that plan's amendments folded in. New from the owner: Termux-style
controls set at start (a notification that cannot be swiped away, an Exit
button, a wake-lock toggle). Three open questions: background capture in
the S23 check, swipe-away, and the wake lock at start.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 18:09 EDT — Foreground service approved

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (Draft, approved).
The owner approved it; section 7's recommendations stand (background
capture is checked, swipe-away stops the service by default, the wake
lock starts released). The release after it is 0.2.3.

## 2026-10-23 18:18 EDT — Foreground service: no background microphone

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`. On the
owner's instruction, an amendment drops background capture: the example
starts `specialUse` only, and step 4 has no capture check. Android stops
microphone input when the app is hidden. `ServiceType::Microphone` stays
in the API. libmorph's plan was approved at the same time and wraps this
one.

## 2026-10-23 18:30 EDT — Foreground service: departures in steps 1 to 3

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (In
implementation). Small departures needed to make the plan work, none of
them changing its types, calls or behaviour:

- Step 1. `NotificationAction` and `ServiceOptions` have no `Default`
  and are `#[non_exhaustive]`, so another crate could not build them at
  all, and `Controls { ongoing: true, .. }` does not compile outside the
  crate. Added `NotificationAction::new(id, label)`,
  `ServiceOptions::new(types, content)` and
  `NotificationContent::new(title, text)`; the fields stay public and are
  set after. `ControlLabels`, which the plan names but does not define, has
  `exit`, `acquire_wake_lock`, `release_wake_lock` and `wake_lock_held`,
  a `Default` with the plan's text, and is `#[non_exhaustive]` like the
  rest.
- Step 1. The platform-free types live in `libmobile::service` (as the
  secure store's do in `libmobile::secure_store`) and are re-exported from
  `libmobile::android::service`, so they build and test on the host.
  `ServiceType` is defined in `codegen`, which build.rs also compiles, and
  re-exported as `gradle::ServiceType` and `android::service::ServiceType`.
  `MAX_ACTIONS`, `EXIT_ACTION` and `WAKE_LOCK_ACTION` name the limit and
  the reserved ids.
- Step 1. `stop()` while stopped returns `Ok`; the plan lists
  `NotRunning` only for update, set_wake_lock and actions.
  `foreground_service(&[])` panics, as `url_scheme` does on a bad scheme:
  Android needs a type. Several `SpecialUse` reasons: the first is used.
- Step 1. `ServiceEvent` lives in `libmobile::system_events`, which is
  compiled under `foreground-service` or `system-events`; the subscriber
  calls are under `system-events` only, so `MobileEvent::Service` works
  without it.
- Step 2 (`88bfe52`). The activity lifecycle callbacks are registered at
  the first activity's `onCreate`, not at the first `subscribe()`, and
  report only once Rust has subscribed. Registered at subscribe time, they
  miss the current activity's `onStart`/`onResume`, and counting it from
  the process importance can be wrong. The network callback and the Doze
  receiver are still registered at the first `subscribe()`. The first
  `subscribe()` also reports the state at that moment (`Visible`,
  `Focused`, `DeviceIdle`, and the network through its callback).
- Step 3. Without the microphone, on the owner's change of scope (relayed
  by the coordinator; the plan's amendment is the coordinator's): the
  example declares and starts with `specialUse` only and opens no capture
  stream. `ServiceType::Microphone` stays in the API and the manifest code.
- Step 3. The example has no Kotlin, so there is no `am broadcast` hook.
  `--es cmd start-later [--es delay <s>]` stops the service, then starts it
  again from a thread after the delay, so a start from the background can
  be tried after Home.

## 2026-10-23 18:53 EDT — Foreground service on the S23 (step 4, partial)

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`, steps 3 and 4.
The `background` example (`a923b6f`, built with `88bfe52`) on the
owner's Galaxy S23 Ultra (Android 16, API 36), over adb on the tailnet.
The phone was on cellular with Wi-Fi off, so the listener was
`nc -lk 7999` on the Mac's tailnet address. Battery setting as found, not
changed: not on the Doze allowlist (`is_ignoring_optimizations` gave
`Ok(false)`), `RUN_ANY_IN_BACKGROUND` default (allow), that is Samsung's
default "Optimized".

Passed:
- Step 3: it builds, runs, asks for `POST_NOTIFICATIONS` (granted with
  `pm grant` first), and starts: `background: start: Ok(())`,
  `libmobile: service Started`, `main loop Started`.
- The notification as `dumpsys notification` shows it:
  `flags=ONGOING_EVENT|ONLY_ALERT_ONCE|NO_CLEAR|FOREGROUND_SERVICE`,
  `category=service actions=3 vis=PRIVATE`, buttons `[0] "Exit"`,
  `[1] "Acquire wake lock"`, `[2] "Ping"`, each a `startService`
  PendingIntent; the text follows `update_text`
  (`android.text=String (45 lines sent to 100.103.47.86:7999)`).
- In the background (Home, `Visible(false)`) from 18:29:16 to past
  18:52, the listener got a line every 30 s with no gap: 46 lines, intervals
  30 or 31 s with the screen on. The process was not frozen
  (`isFrozen=false`, `curProcState=5` during the run).
- With the screen off (18:40:01 to 18:43:53, power button, by the owner),
  lines kept coming, but late once the CPU slept: intervals of 39, 34, 37
  and 36 s, as section 2 expects without a wake lock.
- System events at the first `subscribe()`: `Visible(true)`,
  `Focused(true)`, `DeviceIdle(false)`,
  `Network(Some(NetworkInfo { validated: true, metered: true, transport: Vpn }))`;
  then `Focused(false)` and `Visible(false)` when the screen went off,
  and `Focused(true)` when it came on. A first version counted the
  activity from the process importance at subscribe time; `88bfe52`
  counts it from its `onCreate` (see the departures entry).

Not finished:
- The full 10-minute screen-off check. The owner was using the phone
  (Termux, handlr): the screen was on from 18:30:44 to 18:40:01 and from
  18:43:53 to 18:51:31, so neither window reached 10 minutes. A third
  window started at 18:51:31 and was still open when this was written
  (lines 45 and 46 at 30 s). Rerun with the phone left alone, on the
  default setting and on "Unrestricted".

Not checked, needing the owner's hands (the phone is locked with a secure
lock, so the shade shows no buttons and adb cannot unlock it): the
wake-lock button (labels, " (wake lock held)", `dumpsys power` listing
`libmobile:foreground`, `WakeLock(bool)`), Ping (`Action("ping")`),
Exit, swiping the notification, swiping the app from Recents, Home and
return. Not run, for lack of time in this session: `--es cmd wake-on/off`,
Wi-Fi on and off (`svc wifi`), `dumpsys deviceidle force-idle`/`unforce`,
`POST_NOTIFICATIONS` revoked (`NotificationHidden`), and
`--es cmd start-later` (`StartNotAllowed`). With and without the battery
exemption needs the owner to change the setting. Background capture is out
of scope (the owner's change).

The phone was left as found: Wi-Fi off, not in Doze, permissions as before
(`POST_NOTIFICATIONS` granted to the example only), the example
force-stopped.
