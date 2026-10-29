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

## 2026-10-23 21:27 EDT — Battery exemption and app sleeping (proposed)

Plan: `docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md` (Draft). On the
owner's request, relayed by the handlr session: a separate plan to let the
user exempt the app from battery optimization, and reach Samsung's app
sleeping lists, from inside the app. It proposes a feature `battery` of
its own, taking the unreleased battery calls out of the foreground service
plan's section 5, `BatteryState`, the settings pages, and
`SystemEvent::BatteryExemption`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 21:59 EDT — Opening a link in another app (proposed)

Plan: `docs/design/20261023_03_OPEN_LINK_PLAN.md` (Draft). Requested by the handlr
session for repowiki links in its agent view: an outbound call that opens
an http or https URL in the browser. libmobile builds it for Android,
libdevice for the Mac, and libmorph wraps both as
`MorphContext::open_link`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 22:02 EDT — Plans approved

The owner approved, on 2026-10-23 ("approve plans"), with the
recommended answers:
- `docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md`: a feature `battery`
  of its own, shipped with the foreground service in 0.2.3; the
  foreground service plan's section 5 moves to it (amendment there).
- `docs/design/20261023_03_OPEN_LINK_PLAN.md`: core `open_link`, http and
  https only.

## 2026-10-23 22:43 EDT — Battery exemption departures in steps 1 and 3

Plan: `docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md`.
- Step 1 (`e76d8f7`). `BatteryPlugin` adds
  `<queries><package android:name="com.samsung.android.lool" /></queries>`
  to the manifest. Without it Android 11+ hides Device Care from
  `resolveActivity`, so `open_vendor_battery` could never find Samsung's
  page. A candidate is started only if it resolves and is exported.
- Step 1. The pages and the dialog count the app as visible only at
  `IMPORTANCE_FOREGROUND` or `IMPORTANCE_VISIBLE`. The code moved from
  the foreground service (`importance > IMPORTANCE_VISIBLE`) let a
  hidden app with a running service (`IMPORTANCE_FOREGROUND_SERVICE`,
  125) through, and Android then drops the start without an error.
- Step 1. `BatteryState` has no public constructor; it is only returned.
- Step 3 (`6ea2f63`). The exemption is read again on `Focused(true)` as
  well as `Visible(true)`. The Allow/Deny dialog only pauses the
  activity, so on the S23 no `Visible(true)` came after Allow and
  `BatteryExemption(true)` was not sent, which step 3 expects. The event
  is still sent only when the value changed.

## 2026-10-23 22:43 EDT — Open link departures in step 1

Plan: `docs/design/20261023_03_OPEN_LINK_PLAN.md`.
- Step 1 (`3cdec74`). `check_link` takes and refuses exactly what
  libdevice's does (`src/link.rs`, 13d4787), at the coordinator's request,
  so that libmorph can test that the two agree. The host is the text after
  `://` up to the first `/`, `?` or `#`, after any `user@`, up to
  `:port`, or inside `[...]` for IPv6. Its test cases are copied too.
  `OpenLinkError` has `Display` and `std::error::Error`, as libdevice's
  does.
- Step 1. A call from another thread starts the link from the
  Application context with `FLAG_ACTIVITY_NEW_TASK`, not from the
  activity on its UI thread. Only a call made on the UI thread uses the
  activity. On the S23, waiting for the UI thread from `android_main` at
  launch (a `cmd open-link` intent) timed out after 2 s: the UI thread was
  itself waiting for `android_main` while the activity started. Posting
  without waiting could not report `NoHandler`. The API and the errors
  are as planned. The browser opens in its own task, as Chrome's and
  Samsung Internet's starts did anyway (`flg=0x10000000`).
- Step 1. Visibility is checked by importance, as for the battery pages
  (`IMPORTANCE_FOREGROUND` or `IMPORTANCE_VISIBLE`), because Android 10+
  drops a background start without an error. The plugin's `no-handler`
  and `platform` messages leave out the exception's text, which can hold
  the URL.

## 2026-10-23 22:43 EDT — Battery exemption on the S23 (step 3)

Plan: `docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md`, steps 2 and 3.
The `background` example (`a33edb5`, with `6ea2f63`) on the owner's
Galaxy S23 Ultra (One UI on Android 16), over adb on the tailnet; the
phone unlocked, on cellular with Wi-Fi off, the listener `nc -dlk 7999`
on the Mac's tailnet address. As found: not on the Doze allowlist,
`POST_NOTIFICATIONS` granted, not in Doze.

Passed:
- Step 2: it builds (the merged manifest has
  `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` and the Device Care
  `<queries>`), runs, and on the first resume logs
  `battery Ok(BatteryState { ignoring_optimizations: false,
  background_restricted: false, manufacturer: "samsung" })`, then
  `battery request: Ok(())`. The dialog opened:
  `topResumedActivity=… com.android.settings/.fuelgauge.RequestIgnoreBatteryOptimizations`,
  "Stop optimizing battery usage?" with Deny and Allow (`uiautomator dump`).
- The first `subscribe()` reported `BatteryExemption(false)`, and
  `current()` had `battery_exemption: Some(false)`.
- Deny (`input tap`): no event, not on the allowlist.
- Allow, from `--es cmd battery`: `dumpsys deviceidle whitelist` lists
  `user,dev.highbay.libmobile.example.background,10813`, and on return
  `libmobile: system BatteryExemption(true)`. With `e76d8f7` alone it
  did not come: back from the dialog there was only `Focused(true)`, no
  `Visible(true)` (fixed in `6ea2f63`, see the departures entry).
- `--es cmd battery` while exempt: `Ok(())` and no dialog (the example
  stayed the top activity).
- `whitelist -<package>`, Home and back: `Visible(true)`, then
  `BatteryExemption(false)`.
- `battery-settings`: `Ok(OptimizationList)`; Samsung maps the intent to
  `com.android.settings/.Settings$AppBatteryUsageActivity`.
  `app-details`: `Ok(AppDetails)`,
  `com.android.settings/.applications.InstalledAppDetails`.
  `vendor-battery`: `Ok(Vendor)`, the first candidate,
  `com.samsung.android.lool/com.samsung.android.sm.battery.ui.BatteryActivity`
  (in Device Care's own task). The list needs no correction on this phone.
- Doze, forced with the screen on (`dumpsys deviceidle force-idle`,
  `mState=IDLE`, the example hidden with its service, `curProcState=4`):
  - not exempt, 22:33:22 to 22:36:04: lines 5 to 9 arrived at 22:33:58,
    22:34:28, 22:34:59, 22:35:29 and 22:35:59, 30 s apart, as before Doze;
  - exempt (`whitelist +<package>`), 22:36:06 to 22:38:30: lines 10 to 13
    at 22:36:29, 22:37:00, 22:37:30 and 22:38:00.
  So with a foreground service, forced Doze stopped nothing either way;
  this check does not show what the exemption changes.

Not checked:
- Doze with the screen off and the CPU asleep, with and without the
  exemption: turning the screen off locks the phone, and the open link
  checks still needed it unlocked. Needs the owner to leave the phone
  alone with the screen off, once on each setting.
- `NotVisible` from a page call while hidden: the example has no command
  that opens a page from a thread later.
- Samsung's app sleeping lists ("Never sleeping apps" and the rest) were
  not opened or changed; the vendor page is Device Care's battery page,
  one tap from them.

Left as found: not on the Doze allowlist, not in Doze, Wi-Fi off,
`POST_NOTIFICATIONS` as it was, the example force-stopped.

## 2026-10-23 22:43 EDT — Open link on the S23 (step 2)

Plan: `docs/design/20261023_03_OPEN_LINK_PLAN.md`, step 2. The `wgpu`
example (`2781f63`, with `3cdec74`) on the same S23 Ultra, unlocked.
Chrome was the browser role holder; six other apps also take https links
(Samsung Internet, Firefox and others, `cmd package query-activities`).

Passed:
- `--es cmd open-link` at launch, a tap, and `open-link` while in front
  all opened Chrome:
  `START u0 {act=android.intent.action.VIEW dat=https://example.com/... flg=0x10000000 … cmp=com.android.chrome/…}`,
  `topResumedActivity=… com.android.chrome/org.chromium.chrome.browser.ChromeTabbedActivity`,
  `open link: Ok(())`.
- logcat shows `D/libmobile-wgpu: libmobile: open link https://example.com/a<redacted>`;
  no line from the example or libmobile has `secret`.
- `open-link-later`, then Home: `open link: Err(NotVisible)`, and nothing
  started.
- Chrome disabled (`pm disable-user --user 0 com.android.chrome`): the
  link opened in the next handler, Samsung Internet
  (`cmp=com.sec.android.app.sbrowser/.SBrowserLauncherActivity`).
  `NoHandler` was not seen: other apps take the link.

Failed, then fixed:
- With `3cdec74`'s first version (waiting for the UI thread),
  `cmd open-link` at launch gave
  `Err(Platform("platform: the UI thread did not answer"))` and opened
  nothing. Fixed before the commit; see the departures entry.

Found:
- `cmd open-link` sent to the example while it was hidden gave
  `Err(NotVisible)`: the intent is delivered before the activity is
  visible again. A link opened in answer to an intent that brings the app
  back may need to wait for `Resume`.
- Samsung Internet logs the whole URL itself
  (`W/NativePageHandler: current tab is null. url = https://example.com/a?k=secret`).
  That is the browser's log, not libmobile's.
- Disabling Chrome took the browser role from it
  (`RoleControllerServiceImpl: Removing package that no longer qualifies`)
  and gave it to Samsung Internet, which then showed its first-run
  `HelpIntroActivity`.

Restored: Chrome enabled, the browser role given back to Chrome
(`cmd role add-role-holder --user 0 android.app.role.BROWSER com.android.chrome`,
checked: `get-role-holders` gives `com.android.chrome`, and the next
link opened in Chrome), Samsung Internet force-stopped, the example
force-stopped. The owner may want to check that Samsung Internet's
first-run screens and Chrome's default-browser setting look as before.

## 2026-10-23 23:30 EDT — App storage directories (proposed)

Plan: `docs/design/20261023_04_APP_DIRECTORIES_PLAN.md` (Draft). On the owner's request, relayed by the
handlr session: "libmorph should provide a persistent storage location on
all platforms, with mkdir_all". libmobile gives Android's files and cache
directories, libdevice the desktop paths by app id, and libmorph creates
them before `init` as `MorphContext::storage_dir()` and `cache_dir()`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 23:32 EDT — App storage approved

The owner approved the app storage plans in libmobile, libdevice and
libmorph ("approved plan and start"): `data_dir()` on every host with a
filesystem, `Option` kept for the web, a new `MorphApp::APP_ID`, and a
new `cache_dir()`. libmobile and libdevice are built first.

## 2026-10-23 23:36 EDT — App directories step 1 on the S23

Plan: `docs/design/20261023_04_APP_DIRECTORIES_PLAN.md`, step 1
(`ac0f60e` library, `ffad55b` example, `82a1d9c` README). Built with
`examples/build.sh wgpu`, installed on the owner's S23
(`timothys-s23-ultra:5555`) and started. The example calls
`dirs::files_dir()` and `cache_dir()` on `android_main` before `run`, and
again from a spawned thread.

Passed:
- `I libmobile-wgpu: … wgpu example: files_dir Some("/data/user/0/dev.highbay.libmobile.example.wgpu/files")`
- `I libmobile-wgpu: … wgpu example: cache_dir Some("/data/user/0/dev.highbay.libmobile.example.wgpu/cache")`
- `I libmobile-wgpu: … wgpu example: internal_data_path Some("/data/user/0/dev.highbay.libmobile.example.wgpu/files")`
- `I libmobile-wgpu: … wgpu example: files_dir == internal_data_path: true`
- From another thread (tid 23265, `android_main` is 23264), the same two
  paths: `wgpu example: from a thread: files_dir Some("…/files"), cache_dir Some("…/cache")`.
- No libmobile warning, no crash; the example went on to
  `wgpu: up 1080x2316 Rgba8Unorm` (Vulkan, Adreno 740).

`DirsPlugin` is a `MobileActivityPlugin` with no hooks, listed in
`codegen::PLUGINS` as a core plugin like `LinkPlugin`, so it is in every
build. The example was force-stopped afterwards. Step 2 (release) waits
for the owner.

## 2026-10-23 23:36 EDT — Departures and constructors approved

The owner approved them (2026-10-23: "approve, bump versions if needed and
release"):
- `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`: the constructors
  `ServiceOptions::new`, `NotificationAction::new` and
  `NotificationContent::new`, and the other departures of entry 9b4ca69.
- `docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md`: re-reading the
  exemption on `Focused(true)` as well, and the visibility rule, Device
  Care `<queries>` entry (entry 9c2a646).
- `docs/design/20261023_03_OPEN_LINK_PLAN.md`: links opened from a thread
  other than the UI thread start from the Application context in a new
  task (entry 9c2a646). Apps answering an intent while hidden wait for
  `Resume` before `open_link`; that is documented, not changed.

## 2026-10-23 23:37 EDT — Released libmobile 0.2.3

On the owner's word ("approve, bump versions if needed and release"):
libmobile 0.2.3 (`2c50e9d`, tag `v0.2.3`) with the foreground service,
the battery exemption, system events, `open_link` and the app
directories. Open link and app directories are Implemented. The
foreground service (step 4) and the battery exemption (step 3) stay In
implementation until the owner's S23 checks pass.

## 2026-10-24 12:17 EDT — Memory pressure (approved)

Plan: `docs/design/20261024_01_TRIM_MEMORY_PLAN.md` (Draft, approved). Requested by handlr on the
owner's instruction, and approved by the owner ("approve them, the new
hook in stages"): libmorph's `on_low_memory` first, over libmobile's
existing `MobileEvent::LowMemory`; then libmobile's
`MobileEvent::TrimMemory(TrimLevel)` and libmorph's `on_trim_memory`.

## 2026-10-24 13:03 EDT — The device's vendor, make and model (proposed)

Plan: `docs/design/20261024_02_BUILD_INFO_PLAN.md` (Draft). On the owner's request,
relayed by the handlr session: "does not expose private identifiers just
vendor, make and model". libmobile reads `Build.MANUFACTURER`, `BRAND`
and `MODEL` only; libmorph wraps them as `MorphContext::hardware()`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-24 13:06 EDT — Build info approved; 0.2.4 planned

Plans: `docs/design/20261024_02_BUILD_INFO_PLAN.md` and
`docs/design/20261024_01_TRIM_MEMORY_PLAN.md` (both approved). On the
owner's word ("approve, plan a 0.4.0 with what we've planned"), libmobile
0.2.4 carries `build_info()` and `MobileEvent::TrimMemory`, so libmorph
0.4.0 can wrap them with the foreground service, `open_link`, the app
directories and memory pressure.

## 2026-10-24 13:12 EDT — Trim memory and build info: implementation choices

Plans: `docs/design/20261024_01_TRIM_MEMORY_PLAN.md` and
`docs/design/20261024_02_BUILD_INFO_PLAN.md`, step 1 of each
(`0586bec` and `be395e2`). Small choices the plans leave open; no
public API or behaviour beyond the plans:
- Trim memory: a level below 5 has no lower known level, so it maps to
  `RunningModerate`, the lowest, and still arrives (the plan's reason
  for the rule). Above 80 is `Complete`. Host tests cover each known
  value, values between, below 5 and above 80.
- `TrimLevel` is in `libmobile::event`, next to `MobileEvent`, and
  re-exported as `libmobile::TrimLevel`. `SystemState` is unchanged:
  `SystemEvent::TrimMemory` is an event, and `current()` does not keep
  it. The level is published to system-event subscribers from the JNI
  callback, as the service events are, and queued for the main loop.
  Queued levels are dropped when `android_main` returns, like the other
  commands.
- `TrimMemoryPlugin` registers `ComponentCallbacks2` once per process,
  at the first activity's `onCreate`; its `onLowMemory` does nothing,
  since `MobileEvent::LowMemory` keeps coming from NativeActivity.
- Build info: `BuildInfoPlugin.info()` returns the three values as one
  `String[]` (manufacturer, brand, model), read through the Application
  like `DirsPlugin`. `android::build_info()` keeps the first answer in a
  `OnceLock`; a failed call is logged at warn and tried again next time.
  `BuildInfo` is also re-exported as `android::BuildInfo`.

## 2026-10-24 13:19 EDT — Build info step 1 on the S23

Plan: `docs/design/20261024_02_BUILD_INFO_PLAN.md`, step 1 (`0586bec`
library, `2df80ad` example, `ded93e9` README). Built with
`examples/build.sh wgpu`, installed on the owner's S23
(`timothys-s23-ultra:5555`, API 36) and started. The owner was using the
phone (Termux, then handlr, in front), so the check waited until it had
been idle and unlocked for over 2 minutes.

Passed:
- `I libmobile-wgpu: … wgpu example: build_info Some(BuildInfo { manufacturer: "samsung", brand: "samsung", model: "SM-S918U" })`
- `getprop ro.product.manufacturer`, `ro.product.brand` and
  `ro.product.model` give `samsung`, `samsung`, `SM-S918U`: the same.
- No libmobile warning, no crash; the example went on to
  `wgpu: up 1080x2316 Rgba8Unorm`. The generated registry lists
  `bridge.BuildInfoPlugin` and `bridge.TrimMemoryPlugin` with the other
  core bridges.

Step 2 (release) waits for the owner.

## 2026-10-24 13:19 EDT — Trim memory step 1 on the S23

Plan: `docs/design/20261024_01_TRIM_MEMORY_PLAN.md`, step 1 (`be395e2`
library, `60f34d3` example, `fe57842` README), same build and run as
the build info check.

Passed (each line is followed by the example's
`wgpu example: TrimMemory(<level>)` on `android_main`):
- With the app in front: `am send-trim-memory <package> RUNNING_CRITICAL`
  gave `libmobile: trim memory 15 (RunningCritical)`. API 36 still
  delivers a running level sent by `am`.
- Home: the system sent `trim memory 20 (UiHidden)` by itself.
- Hidden: `HIDDEN` gave `trim memory 20 (UiHidden)`; `BACKGROUND` gave
  `trim memory 40 (Background)`; `MODERATE` gave `60 (Moderate)`;
  `COMPLETE` gave `80 (Complete)`.
- Unknown raw levels on the device: `50` gave `trim memory 50
  (Background)`, `70` gave `trim memory 70 (Moderate)`: the nearest
  lower known level.

Refused by `am`, not by libmobile:
- `UI_HIDDEN`: `Error: Unknown level option: UI_HIDDEN`. `am` names
  that level `HIDDEN` (`am help`: `HIDDEN|RUNNING_MODERATE|BACKGROUND|
  RUNNING_LOW|MODERATE|RUNNING_CRITICAL|COMPLETE`); the plan's command
  should read `HIDDEN`.
- `BACKGROUND`, `MODERATE`, `COMPLETE` while the app was in front:
  `IllegalArgumentException: Unable to set a background trim level on a
  foreground process`.
- `RUNNING_LOW` and `RUNNING_MODERATE` after `RUNNING_CRITICAL`:
  `Unable to set a higher trim level than current level` (`am` only
  raises the process's level).

The example was force-stopped afterwards (no pid left). The launcher was
then in front; Termux, which the owner had in front, was not brought back.
Step 2 (release) waits for the owner.

## 2026-10-24 13:32 EDT — Released libmobile 0.2.4

On the owner's word ("Yes, go ahead with all three"): libmobile 0.2.4
(`27f3f74`, tag `v0.2.4`) with `build_info()` and
`MobileEvent::TrimMemory`, both Implemented. libmorph 0.4.1 wraps them.

## 2026-10-24 13:51 EDT — App icons (proposed)

Plan: `docs/design/20261024_03_APP_ICON_PLAN.md` (Draft). On the owner's
request, relayed by the handlr session: icons from PNGs, made into Android
resources automatically, with vector sources and desktop use later.
libmobile generates the resources and manifest attributes behind a
host-only `icons` feature; libmorph-build turns it on.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-24 13:55 EDT — App icons approved

Plan: `docs/design/20261024_03_APP_ICON_PLAN.md` (Draft, approved). The
owner approved the icon plans in libmobile and libmorph, with the
recommended answers. PNG describes the bitmap sources; the generated
files are whatever each platform takes.

## 2026-10-24 14:13 EDT — Alert notifications and taps (proposed)

Plan: `docs/design/20261024_04_NOTIFICATIONS_PLAN.md` (Draft). Requested by
the handlr session with the owner's "yes", and the owner's "updating the
ongoing notification should be supported": alert notifications on the
app's own channels, a tag naming what was tapped, and updating the ongoing
notification from any thread while suspended, as a checked guarantee.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-24 14:16 EDT — Notifications approved

Plan: `docs/design/20261024_04_NOTIFICATIONS_PLAN.md` (Draft, approved).
The owner approved the notification plans in libmobile and libmorph
("approve both"), with the recommended answers.

## 2026-10-24 14:29 EDT — App icons step 1 built; step 2 on the S23 partly checked

Plan: `docs/design/20261024_03_APP_ICON_PLAN.md` (now In implementation),
step 1 (`1ed44b3` library and README, `2acdfc6` example).

Small departure (step 1): the rasters (`icon_images`) are in a private
`icon` module, not `codegen`, because `build.rs` compiles
`src/codegen.rs` directly and has no `image` there.
`codegen::adaptive_icon_xml` is in `codegen` as planned. Both are
crate-private: the public API is only the `icons` feature, `gradle::Icon`
and `Project::icon`, as step 3 says. Generated icon files that a later
build no longer makes are removed from the generated project.

Step 1, passed:
- `cargo fmt --all -- --check`; host `cargo clippy --workspace
  --all-targets` with no features, `--features=icons` and
  `--all-features`; `cargo clippy --target aarch64-linux-android
  --workspace --all-targets` with no features and `--all-features`: no
  warnings. `cargo test --workspace` (and with `--features icons`),
  `cargo test -p libmobile --features icons` (100 lib tests) and
  `--all-features` (131): all pass.
- `cargo tree --target aarch64-linux-android -p libmobile-example-wgpu -e
  normal,build -i image`: `image` only under `[build-dependencies]`.
- `examples/build.sh wgpu` built the APK. `aapt dump badging`
  (build-tools 36.0.0):
  `application-icon-160`, `-240`, `-320`, `-480`, `-640` and
  `-65534:'res/mipmap-anydpi-v26/ic_launcher.xml'`;
  `application: label='libmobile wgpu' icon='res/mipmap-anydpi-v26/ic_launcher.xml'`.
  `aapt2 dump resources`: `color/ic_launcher_background`,
  `mipmap/ic_launcher` (0x7f020000), `_foreground`, `_monochrome`,
  `_round`.
- The generated xxxhdpi files, looked at over a checkerboard: the legacy
  icon is the disc over #20242C with rounded, transparent corners; the
  round one is the same in a circle; the foreground and the white
  monochrome layer sit in the centre of the transparent 432 px canvas.

Step 2 on the S23 (`timothys-s23-ultra:5555`, API 36):
- Installed (`adb install -r`: Success). `cmd package resolve-activity
  -a android.intent.action.MAIN -c android.intent.category.LAUNCHER`
  gives `nonLocalizedLabel=libmobile wgpu icon=0x7f020000`, the APK's
  `mipmap/ic_launcher`.
- Not checked: the launcher. The owner had Termux in front with the screen
  on from 14:10 to 14:29, so no screenshot was taken and the app was not
  started. The launcher icon under One UI's mask, themed icons and the app
  info page need the owner.
- The example was force-stopped afterwards (no pid).

## 2026-10-24 14:30 EDT — Released libmobile 0.2.5

On the owner's word: libmobile 0.2.5 (`411ba59`, tag `v0.2.5`) with the
`icons` feature. The icon plan stays In implementation until the owner
checks the launcher on the S23.

## 2026-10-24 15:34 EDT — The system's dark mode (proposed)

Plan: `docs/design/20261024_05_NIGHT_MODE_PLAN.md` (Draft). Requested by the handlr session with the
owner's "yes", for its Appearance setting's System option: libmobile reads
Android's night mode, and libmorph offers `dark_mode()` and
`on_dark_mode` on every host, with winit's theme on the desktop and the
web.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-24 15:38 EDT — Dark mode approved; 0.2.6 and 0.4.3 planned

Plan: `docs/design/20261024_05_NIGHT_MODE_PLAN.md` (Draft, approved), with the system bars. On the
owner's word ("approve, yes"), the notification and dark mode plans are
built together and released as libmobile 0.2.6 and libmorph 0.4.3.

## 2026-10-24 16:15 EDT — Text input (proposed)

Plan: `docs/design/20261024_06_TEXT_INPUT_OPTIONS_PLAN.md` (Draft). Requested by the handlr session with the
owner's "yes": the soft keyboard with options, the action key and
dismissal (libmobile), paste on the desktop (libdevice), and one text
input API and hook in libmorph on every host.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-24 16:18 EDT — Text input approved

Plan: `docs/design/20261024_06_TEXT_INPUT_OPTIONS_PLAN.md` (Draft, approved). The owner approved the text
input plans ("yes"), with the recommended answers. Released after the
notification and dark mode work.

## 2026-10-24 16:21 EDT — Notifications steps 1 and 2 built; step 3 on the S23 partly checked

Plan: `docs/design/20261024_04_NOTIFICATIONS_PLAN.md` (now In
implementation), step 1 (`6e83eef` library and README, `bf0c356` a
dead-code fix), step 2 (`d1676ad` the `background` example).

Small departures (step 1), none changing the planned API:
- `NotificationContent`, `NotificationAction`, `LockScreenVisibility` and
  `MAX_ACTIONS` moved into a private `notification` module, compiled under
  `foreground-service` or `notifications`, so alerts can use them without
  the service. `service` re-exports them as before, and `notify` (and
  `android::notify`) re-export them too; every existing path still works.
- A tag over 256 bytes is refused before Android is asked, as
  `ServiceError::Platform` and `NotifyError::Platform` with the message
  "the notification's tag is longer than 256 bytes": the plan names the
  limit but no error for it. `NotifyError::BadId` also covers an empty
  button id. Alert button ids may start with `libmobile.` (alerts have no
  built-in controls).
- Besides `libmobile.notification` and `libmobile.tag`, the tap intents
  carry `libmobile.kind` (`service` or `alert`), since an alert may have
  any id, and `libmobile.action` for an alert's button that opens the app.
  The service's notification's id extra is `libmobile.foreground`. A host
  test checks the names against the Kotlin sources.
- The service's body tap now carries `CLEAR_TOP | SINGLE_TOP`, so a
  running activity receives it through `onNewIntent` (before, it only
  brought the task forward, and no intent was delivered). A tap reaching a
  recreated activity (`savedInstanceState` set) is not delivered again.
- An alert's button that does not open the app goes to a new
  `notify.NotifyActionReceiver` (in the manifest, not exported), which loads
  the app's library if no activity has (`NativeLib.ensureLoaded(Context)`)
  and queues `NotifyEvent::Action` for the main loop. Buttons do not
  remove the alert; the app cancels it. Each alert's and button's
  PendingIntent is kept apart with `Intent.setIdentifier` (API 29).
- `bf0c356`: `jni_util::string_array` is compiled only with a feature that
  posts a notification. `6e83eef` gave a dead-code warning when libmobile
  was built for Android alone with no features; the workspace check did not
  show it, as the `background` example turns the features on.

Step 1, passed:
- `cargo fmt --all -- --check`; host `cargo clippy --workspace
  --all-targets` with no features and `--all-features`; `cargo clippy
  --target aarch64-linux-android --workspace --all-targets` with no
  features, `--features notifications`, `--features
  foreground-service,notifications,system-events` and `--all-features`,
  and `-p libmobile` alone: no warnings. `cargo test --workspace` (112
  lib tests) and `cargo test -p libmobile --all-features` (143): all pass.
- A generated project with `notifications` builds (`examples/build.sh
  background`). `aapt dump xmltree`: `POST_NOTIFICATIONS`, and
  `<receiver android:name="dev.highbay.libmobile.notify.NotifyActionReceiver"
  android:exported=false>`; `dexdump`: `notify/NotifyPlugin` and
  `notify/NotifyActionReceiver`.

Step 2: the example builds, installs (`adb install -r`: Success) and runs
on the S23 (API 36): the service started, and `--es cmd alert` posted.

Step 3 on the S23 (`timothys-s23-ultra:5555`), passed:
- The alert is on its own channel: `dumpsys notification --noredact`:
  `NotificationRecord(... pkg=dev.highbay.libmobile.example.background
  id=27757 tag=example importance=4 ... channel=alerts ... flags=AUTO_CANCEL
  actions=2`, `android.title=String (libmobile alert example)`,
  `android.text=String (alert number 1)`, buttons `"Ack" ->
  ... broadcastIntent` and `"Open" -> ... startActivity`, and
  `stats=... isNoisy=true`. The package's channels:
  `NotificationChannel{mId='alerts', mName=Alerts, mImportance=4, ...
  mSound=content://settings/system/notification_sound` and
  `NotificationChannel{mId='libmob...', mName=Running in the background,
  mImportance=2`. Whether the phone sounded was not heard.
- Updates from the network thread while the activity was stopped
  (`Visible(false)` from 15:58:26): `update 2 (tag update-2): Ok(())` at
  15:58:41 and `update 3 (tag update-3): Ok(())` at 15:59:11; the shade
  showed `libmobile background, update 1`, then `update 2`. The ongoing
  notification's flags: `ONGOING_EVENT|ONLY_ALERT_ONCE|NO_CLEAR|FOREGROUND_SERVICE`.
  After the service stopped, `update 4 (tag update-4): Err(NotRunning)`.

Not checked:
- Tapping (the alert's body and buttons, the ongoing notification's body),
  replacing and cancelling, the cold start, the screen-off updates, and
  `PermissionDenied`. Two taps meant for the app's notification group
  in the shade (15:58 to 15:59) landed elsewhere: the shade had closed or
  moved, and the owner appears to have been using the phone at the same
  time (Google's app came forward, every notification was cleared, the
  example's task was removed at 15:59:25, which stopped its service, and
  handlr came forward). From 16:00 to 16:20 Termux or handlr was in front
  with recent touches, so the screen was not taken again.
- The battery dialog the example opens was answered while the phone was
  in use (the example was exempt at 15:59:22); the exemption was removed
  again (`dumpsys deviceidle whitelist -dev.highbay.libmobile.example.background`:
  `Removed`). Night mode, the navigation mode and `POST_NOTIFICATIONS`
  (granted) were not changed. Both examples were force-stopped (no pid).

## 2026-10-24 16:21 EDT — Night mode step 1 built; the S23 not checked

Plan: `docs/design/20261024_05_NIGHT_MODE_PLAN.md` (now In
implementation), step 1 with the system bars amendment (`20f5083` library
and README, `45097f7` the `wgpu` example).

Small departures (step 1):
- Both calls are in a private `android::appearance` module, re-exported as
  `android::night_mode` and `android::set_system_bar_icons`.
- The contrast scrims are turned off and the bars' colours made transparent
  from the first `set_system_bar_icons` call on, not for every app at
  start, so an app that never calls it keeps the bars as before. The
  amendment can be read either way; the owner may prefer always.
- `WindowInsetsPlugin` keeps the latest icons and applies them again in
  `onWindowAttached` and `onResume` (each new window, and before
  `WindowCreated` reaches Rust), and on the UI thread of the current
  activity at each call. Before an activity exists the choice is only kept.

Step 1, passed:
- The same checks as the notifications entry above (fmt; host clippy with
  no features and `--all-features`; Android clippy with no features,
  `--features notifications`, `--features
  foreground-service,notifications,system-events`, `--all-features` and
  `-p libmobile`; `cargo test --workspace` and `-p libmobile
  --all-features`): no warnings, all pass.
- `examples/build.sh wgpu` builds (the Kotlin compiles); installed on the
  S23 (`adb install -r`: Success).

Not checked on the S23: `cmd uimode night yes`/`no` logging
`Some(true)`/`Some(false)` on `ConfigChanged`, and the bar icons
(`dumpsys window` appearance flags and screenshots, 3-button and gesture
navigation, the scrim). From 16:00 to 16:20 Termux or handlr was in front
with recent touches, and switching night mode or the navigation mode
changes what the owner sees. Night mode (`yes`) and the navigation mode
(`0`, 3-button) were read and left unchanged. The example was
force-stopped.

## 2026-10-24 16:22 EDT — Released libmobile 0.2.6

On the owner's word ("approve, yes"): libmobile 0.2.6 (`f7954ed`, tag
`v0.2.6`) with alert notifications, tap tags, night mode and the bar
icons. Both plans stay In implementation until their S23 checks pass.

## 2026-10-28 13:19 EDT — Camera capture and QR scanning (proposed)

Plan: `docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md` (Draft).
Requested by the owner through the flankr session: single-frame capture,
a QR mode with auto zoom, exposure for codes on screens and zoom for a
pinch, a Cargo feature per part, and the CAMERA permission asked for only
when the app uses the camera. libmorph's companion plan wraps it.

The README gains its `## Architectural decisions` table, which it did not
have, with a row for every plan in `docs/design/`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-28 13:43 EDT — Media request: the system chooser with a camera entry (proposed)

Plan: `docs/design/20261028_02_MEDIA_REQUEST_PLAN.md` (Draft). Requested
by the owner through the flankr session: "im thinking of media-request as
open the media selector which has a camera option", then "you can plan
the media request in a new plan, we'll need it soon anyway". A
`media-request` feature beside `camera-intent` and the in-process
`camera`; libmorph's companion plan of the same name wraps it.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-28 15:13 EDT — camera capture and QR plan, steps 1-4: departures

Plan: docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (with its
2026-10-28 14:46 EDT amendment). Steps 1-4 in `de66d72`'s successor tagged
`v0.3.0`. Not yet run on a phone; linked for aarch64 (NDK 28.1, API 29) and
packaged with Gradle in a scratch app.

1. **Preview frames** are `latest_preview(SessionId) ->
   Option<Arc<PreviewFrame>>` with I420 planes for the wgpu viewfinder, not
   the body's Gray8/Rgba8 `Frame`; with `upright: false` the planes are
   unrotated and `quarter_turns` is left to the shader.
2. **Modules** are `src/camera*` and `src/android/camera/` (the body's
   `camera_device`); the gate's pure logic is `src/camera/gate.rs`.
3. **Kotlin:** `CameraPlugin.displayInfo(activity) -> [rotation, width,
   height]` instead of `displayRotation` (the display's aspect picks the
   analysis size, 5.2).
4. **Raw `ndk-sys` bindings** rather than `ndk/media`, so image-reader
   listeners carry an integer context, not a boxed closure; one
   module-level `#![allow(unsafe_code)]` in `src/android/camera/ndk.rs`.
5. **Captures** are served one at a time, oldest first; a Gray8/Rgba8
   capture from a running preview is scaled down by a whole factor; a zoom
   set before `Opened` is applied on opening; `Jpeg` frames report the
   `AImage` size even with `JPEG_ORIENTATION` set (risk in section 10).
6. **A camera event in flight** when the activity is destroyed could reach
   the next `android_main` run (minor).
7. **The README** (step 7) is not yet updated for the rename; `src/gradle.rs`
   docs still say `features = ["camera"]`, now the in-process camera.

## 2026-10-28 15:34 EDT — camera capture and QR plan, step 5: departures

Plan: docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md. Step 5 in the
commit tagged `v0.3.1`. Not yet run on a phone.

1. rqrr 0.11 also brings `lru` 0.18 (with `hashbrown`, `foldhash`).
2. Constants beyond section 11: `QR_ZOOM_MIN_STEP` 1.1, `QR_SAT_LEVEL` 245,
   `QR_RETRY_MODULE_PX` 8, `QR_FINE_ZOOM_MIN` 1.02, `QR_FINE_AIM` 1.05 (the
   fine finish lands at 4.0-4.2 px per module on API 30+; API 29 keeps the
   coarse steps).
3. `k_fit` is worked out per axis; the code still stays within 90%.
4. `QR_DECODE_BUDGET_MS` is only logged; the analysis stream is not shrunk
   to 960x540 automatically.
5. The AF trigger is a CANCEL+START burst; loss sends a CANCEL.
6. Each ladder rung re-meters on the code's current box; held windows at
   the floor last `QR_AE_SETTLE_MS`; loss resets only a ladder a code
   started, to the mode's start; AE's early settle needs AE to leave and
   return to CONVERGED/LOCKED.
7. Codes are reported as soon as read; in `Continuous` every distinct
   payload in a frame is reported.
8. `zoom_by` during auto zoom multiplies the ratio shown (user x auto),
   then resets auto (visible to the app).
9. The example (step 6) and the README (step 7) are not done yet.

## 2026-10-28 23:42 EDT — Reading the clipboard's text (approved)

Plan: `docs/design/20261028_03_CLIPBOARD_PLAN.md` (Draft). Requested
through the flankr session for its bottom bar plan's Paste text: a
`clipboard` feature with `android::clipboard::read_text` and `has_text`,
read only, with Android 10's focus rule and Android 12's toast documented.
Approved by the owner ("plan those in appropriate repos with subagent,
then impl"); libmorph's plan of the same name wraps it. For libmobile
0.3.2.

## 2026-10-28 23:52 EDT — clipboard plan, steps 1 and 2: departures

Plan: docs/design/20261028_03_CLIPBOARD_PLAN.md. Built on the branch
`wt-clipboard`. Not yet run on a phone.

1. Steps 1 and 2 are built together (0.3.2, the README section and the
   `VERSIONS.md` entry), on the owner's approval to plan and implement;
   step 1's S23 check is still open.
2. The Kotlin doc comments do not spell the MIME pattern as `text/*`: in
   Kotlin `/*` opens a nested comment inside a KDoc, and the file would
   not compile.
3. `examples/build.sh wgpu` builds; `dexdump` shows
   `clipboard/ClipboardPlugin` with `readText` and `hasText` as public
   static methods of the signatures the Rust side calls.

## 2026-10-28 23:59 EDT — Share and view: Open in…, Share, Share → the app, and image decoding (proposed)

Plan: `docs/design/20261028_04_SHARE_AND_VIEW_PLAN.md` (Draft). Requested
by the owner through the flankr session: "plan the libmobile
share-and-view plan", the dependency named in flankr's binary files plan
(10.6, and its 21:29 EDT amendment, item 6). Three features: `share-out`,
`share-target` and `image-codec`; libmorph's later plan of the same name
wraps them. For the next libmobile patch after 0.3.2.

Proposed only; nothing is implemented until the owner approves.
