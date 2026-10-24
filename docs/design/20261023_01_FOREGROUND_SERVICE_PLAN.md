---
title: Foreground service, notification controls and system events
date: 2026-10-23
status: In implementation
covers:
  - "A generated foreground service (feature `foreground-service`),
    declared from the app's build script and controlled from Rust on any
    thread: `android::service`"
  - "Its notification: generic content and actions, and Termux-style
    controls the app turns on at start: not dismissable, an Exit button,
    and a wake-lock toggle button"
  - "A battery-optimization exemption: `android::battery`"
  - "System events from any thread, with no async runtime by default
    (feature `system-events`), and a `tokio` receiver (feature `tokio`)"
  - What the service does and does not guarantee about process lifetime on
    Android 14 to 16
sources:
  - "libmorph docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md (Draft):
    its sections 2 to 4, steps 1 to 4, and every amendment to it up to
    \"a generic notification\" (e8c3162). That plan's libmobile half is
    written here so that it is built in libmobile first (libmorph AGENTS.md,
    Platform Layering). Where the two differ, this plan is the
    specification for libmobile."
  - "Owner (2026-10-23, relayed by the handlr session, asking for this
    plan): \"ask it to write a plan, I'll approve it\". Also: \"we should
    fix the foreground service, no notification so that was probably
    forgotten\"."
  - "Owner (2026-10-23, relayed by the handlr session): \"notification
    needs to have flags to enable the exit instead of swipe to exit and the
    wake lock buttons similar to [Termux]\". Termux's notification cannot
    be swiped away; its Exit button stops the service and the app; its
    second button reads \"Acquire wakelock\" or \"Release wakelock\" and
    toggles a partial wake lock, and the text then says \"(wake lock
    held)\"."
  - "handlr at acee3d3 (github.com/tmzt/handlr, the companion app in
    apps/companion), from the handlr session: one long-lived Noise link to
    the daemon over the tailnet (plan 20261022_02), with a ping after 30 s
    of quiet and a reconnect if a ping goes unanswered for 10 s. It must
    keep running with the screen off and in the background. Read-aloud
    (TTS, plan 20261023_05) and voice confirmations (STT on the
    microphone) run while the user is not looking. The service is started
    when the phone is paired and connects, kept while paired, and stopped
    on unpair or quit; never for an unpaired phone. The notification only
    needs to say handlr is connected, and to open the app on a tap."
  - "On the S23, the daemon sees the link close about 7 s after the app is
    backgrounded, which fits Android 12+'s cached-app freezer."
  - "Android: `Service.startForeground(int, Notification, int)` (API 29;
    the type argument is required from API 34); `foregroundServiceType`
    and `FOREGROUND_SERVICE_<TYPE>` permissions (API 34);
    `ForegroundServiceStartNotAllowedException` (API 31); while-in-use
    types such as `microphone`; `dataSync`'s 6 hours in 24 and
    `Service.onTimeout(int, int)` (API 35); `POST_NOTIFICATIONS` (API 33);
    from Android 14, users can dismiss a foreground service's notification
    even with `setOngoing(true)`, and the service keeps running;
    `Notification.Builder.setDeleteIntent`; `PowerManager.PARTIAL_WAKE_LOCK`;
    `ConnectivityManager.registerDefaultNetworkCallback` (API 24);
    `PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED`;
    `Application.registerActivityLifecycleCallbacks`."
  - "libmobile today: Kotlin plugins on `MobileNativeActivity`
    (`MobileActivityPlugin`), JNI through the Application context
    (`jni_util::with_application_class`, used by the secure store), a
    generated manifest (`gradle::Project`), and a main loop that drains
    plugin mailboxes into `MobileEvent`s."
related:
  - libmorph docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md (wraps
    this)
  - docs/design/20261022_03_SECURE_STORE_PLAN.md (calls from any thread
    through the Application context)
  - docs/design/20261022_05_URL_SCHEMES_PLAN.md (manifest generation)
---

# Foreground service, notification controls and system events

## 1. Goal

An app keeps working while it is in the background:
- **Kept alive:** a foreground service, with its notification, stops
  Android from caching and freezing the process, so the app's own threads
  (a network thread, an async runtime, audio) keep running with the screen
  off.
- **Controlled by the user from the notification**, as in Termux: the
  notification stays until the user taps **Exit**, and a button acquires
  or releases a partial wake lock. The app turns each of these on when it
  starts the service.
- **Told what happens:** the app's threads learn about visibility, the
  service, network changes and Doze from any thread, without the UI loop.

No app logic runs in Kotlin, and libmobile needs no async runtime unless a
runtime feature is turned on.

## 2. What the service guarantees, and what it does not

The contract apps build on. Items marked *checked* are verified on the
S23 in step 4.

- **Not frozen while it runs.** The freezer freezes only cached processes,
  and a process with a started foreground service is not cached. *Checked.*
- **The process can still die**, under memory pressure or by Force stop.
  The service returns `START_NOT_STICKY`, so Android never restarts it
  after the process dies. Bringing it back is the app's job, the next time
  it is visible.
- **The CPU may sleep** with the screen off. Packets on an open socket wake
  it, but the app's own timers can run late. A partial wake lock keeps
  them on time, at a cost in battery. The user turns it on and off from
  the notification (section 4.3), or the app sets it.
- **Doze.** Network access in Doze is restricted, and how far a running
  foreground service is exempt varies by version and vendor. This plan
  promises nothing there. It reports Doze entry and exit (section 6) so the
  app can reconnect on exit. Step 4 records what happens on the S23, with
  and without the battery exemption (section 5). *Checked, and recorded.*
- **Start rules.** The service is started while the app is visible.
  Android refuses a start from the background, and the `microphone` type
  also needs `RECORD_AUDIO` granted. Updating and stopping work from any
  thread at any time.
- **Notification permission.** On Android 13+, with `POST_NOTIFICATIONS`
  denied, the service still runs, but its notification shows only in the
  system's running-apps list, so the Exit and wake-lock buttons are not
  reachable. This is reported as `ServiceEvent::NotificationHidden`, not
  an error. The app asks for `POST_NOTIFICATIONS` itself, through
  `request_permissions`, before starting the service.
- **Type limits.** `dataSync` is limited to 6 hours in 24 on Android 15+.
  At the limit libmobile stops the service and reports
  `ServiceEvent::TimedOut`. `specialUse` and `microphone` have no limit.
  `specialUse` carries a stated reason
  (`PROPERTY_SPECIAL_USE_FGS_SUBTYPE`), which matters only for Play review.
- **Samsung.** One UI's "Sleeping" and "Deep sleeping" apps, and its
  default battery setting, can stop background work even with a service
  running. Step 4 runs its background checks with the app's battery
  setting on "Unrestricted" and on the default, and records both.

## 3. The service and its manifest

Feature `foreground-service`. It stages
`android/java/dev/highbay/libmobile/service/MobileForegroundService.kt`
(a `Service`) and its plugin, `service.ForegroundServicePlugin`. The
service:
- creates the notification channel `libmobile.foreground`, of low
  importance (no sound, no heads-up);
- calls `startForeground(1, notification, types)`;
- builds and rebuilds the notification (section 4);
- holds and releases the partial wake lock, tagged
  `libmobile:foreground`;
- handles `onTimeout` by stopping itself;
- returns `START_NOT_STICKY`.

The app declares the types in its build script:

```rust
// libmobile::gradle
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceType {
    DataSync,
    Microphone,
    /// The stated reason, for Play review.
    SpecialUse(&'static str),
    RemoteMessaging,
}

impl Project {
    /// Declare the foreground service with the types it may be started
    /// with. Without the `foreground-service` feature, `generate` fails
    /// with an error that names the feature.
    pub fn foreground_service(self, types: &[ServiceType]) -> Self;
    /// Swiping the app away from Recents leaves the service running
    /// (`stopWithTask="false"`). By default it stops it (section 7,
    /// question 2).
    pub fn foreground_service_keeps_running(self) -> Self;
    /// Declare REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, so
    /// `battery::request_ignore_optimizations` may ask directly.
    pub fn request_ignore_battery_optimizations(self) -> Self;
}
```

```rust
libmobile::gradle::Project::new("dev.tmzt.handlr")
    .foreground_service(&[
        ServiceType::SpecialUse("persistent link to the user's own server"),
        ServiceType::Microphone,
    ])
    .generate()?;
```

From the declared types, `Project::generate` adds to the manifest, by a
pure, tested function `codegen::foreground_service_manifest`:
- `<service android:name="dev.highbay.libmobile.service.MobileForegroundService"
  android:exported="false" android:stopWithTask="true"
  android:foregroundServiceType="specialUse|microphone">`;
- the `specialUse` subtype `<property>`;
- `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_<TYPE>` for each type,
  `POST_NOTIFICATIONS` and `WAKE_LOCK`.

`RECORD_AUDIO` for `Microphone` stays the app's own permission, declared
and asked for as now.

## 4. Control from Rust

`src/android/service.rs`, exported as `libmobile::android::service`.
Every call is synchronous, works from any thread through the Application
context, and needs no `AndroidApp`.

### 4.1 Content

```rust
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NotificationContent {
    pub title: String,
    pub text: String,
    /// The app's own buttons. With the built-in controls (4.3), at most 3
    /// buttons in all, Android's limit; more is ServiceError::TooManyActions.
    pub actions: Vec<NotificationAction>,
    /// On the lock screen. Default Private.
    pub visibility: LockScreenVisibility,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NotificationAction {
    /// The app's own id, returned on a tap. Not empty, and not starting
    /// with `libmobile.` (reserved for 4.3).
    pub id: String,
    pub label: String,
    /// Also bring the activity forward on a tap.
    pub opens_app: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockScreenVisibility { Public, #[default] Private, Secret }
```

Tapping the notification's body opens the app.

### 4.2 Starting, updating, stopping

```rust
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ServiceOptions {
    /// A subset of the declared types.
    pub types: Vec<ServiceType>,
    pub content: NotificationContent,
    pub controls: Controls,
    /// Hold the wake lock from the start.
    pub wake_lock: bool,
}

pub fn start(options: &ServiceOptions) -> Result<(), ServiceError>;
pub fn update(content: &NotificationContent) -> Result<(), ServiceError>;
pub fn update_text(text: &str) -> Result<(), ServiceError>;
pub fn stop() -> Result<(), ServiceError>;
pub fn is_running() -> bool;
pub fn set_wake_lock(held: bool) -> Result<(), ServiceError>;
pub fn wake_lock_held() -> bool;

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The build script did not declare the service, or these types.
    NotDeclared,
    /// Started while the app was not visible.
    StartNotAllowed,
    /// For example RECORD_AUDIO for Microphone.
    MissingPermission(String),
    /// update, set_wake_lock or an action while stopped.
    NotRunning,
    TooManyActions,
    /// An action id that is empty or reserved.
    BadActionId,
    Platform(String),
}
```

`startForegroundService` raises the background refusal at once. A
failure inside the service's own `startForeground` arrives as
`ServiceEvent::Failed(String)` (section 6).

### 4.3 Built-in controls (Termux-style)

Flags the app sets at start. All are off in `Controls::default()`, so an
app gets only what it asks for.

```rust
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Controls {
    /// The notification cannot be swiped away (see below).
    pub ongoing: bool,
    /// An Exit button.
    pub exit: bool,
    /// A button that acquires or releases the wake lock.
    pub wake_lock_toggle: bool,
    /// Their text. Default: "Exit", "Acquire wake lock",
    /// "Release wake lock", and " (wake lock held)" after the text.
    pub labels: ControlLabels,
}
```

- **`ongoing`.** The notification is built with `setOngoing(true)`. On
  Android 13 and earlier that alone prevents a swipe. From Android 14, the
  user can dismiss it anyway, and the service keeps running without a
  visible notification. So with `ongoing`, the notification also carries a
  delete intent, and the service posts it again straight away when it is
  dismissed. Leaving is then only through Exit (or the app's own `stop()`).
- **`exit`.** A button labelled Exit. A tap:
  1. releases the wake lock and stops the service, removing the
     notification;
  2. delivers `ServiceEvent::Exited` to system-event subscribers and as
     `MobileEvent::Service(ServiceEvent::Exited)` on the main loop, so the
     app's threads can close their connections;
  3. finishes the activity's task (`finishAndRemoveTask`) if it is alive,
     and the normal teardown follows (`WindowDestroyed`, then `Destroy`).

  libmobile does not kill the process. The app's threads end themselves
  on `Exited`.
- **`wake_lock_toggle`.** A button that reads "Acquire wake lock" while
  none is held, and "Release wake lock" while one is. A tap toggles the
  service's partial wake lock, rebuilds the notification, and delivers
  `ServiceEvent::WakeLock(bool)`. While it is held, the label
  " (wake lock held)" follows the app's text. `set_wake_lock` does the
  same from Rust.
- Built-in buttons come first, then the app's, within the limit of 3.
  Their ids are `libmobile.exit` and `libmobile.wake_lock`, and their taps
  are not also delivered as `ServiceEvent::Action`.

For handlr this is `Controls { ongoing: true, exit: true,
wake_lock_toggle: true, .. }`, with "connected" as its text, and its own
connection state later through `update_text`.

## 5. Battery-optimization exemption

`src/android/battery.rs`, under `foreground-service`:

```rust
/// PowerManager.isIgnoringBatteryOptimizations(package).
pub fn is_ignoring_optimizations() -> Result<bool, ServiceError>;
/// ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS: the system's Allow/Deny
/// dialog. While visible only; NotDeclared without
/// `Project::request_ignore_battery_optimizations`.
pub fn request_ignore_optimizations() -> Result<(), ServiceError>;
/// ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS: the system's list.
pub fn open_settings() -> Result<(), ServiceError>;
```

Play allows asking directly only for some kinds of app; a Play build uses
`open_settings`, and the rustdoc says so. The answer is read back with
`is_ignoring_optimizations` on the next `Visible(true)`.

## 6. System events

Feature `system-events`. It adds the core plugin
`events.SystemEventsPlugin` and `ACCESS_NETWORK_STATE`.

```rust
// libmobile::system_events: platform-free types, Android source
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SystemEvent {
    /// onActivityStarted / onActivityStopped: the app can be seen.
    Visible(bool),
    /// onActivityResumed / onActivityPaused.
    Focused(bool),
    Service(ServiceEvent),
    /// The default network: Some on available or changed, None on lost.
    Network(Option<NetworkInfo>),
    /// Doze entered or left.
    DeviceIdle(bool),
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ServiceEvent {
    Started,
    Stopped,
    Failed(String),
    TimedOut,
    NotificationHidden,
    /// The app's own button (4.1), by id.
    Action(String),
    /// The Exit button (4.3).
    Exited,
    /// The wake lock is now held, or not.
    WakeLock(bool),
    /// The app was swiped away while `foreground_service_keeps_running`.
    TaskRemoved,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NetworkInfo { pub validated: bool, pub metered: bool, pub transport: Transport }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transport { Wifi, Cellular, Ethernet, Vpn, Other }

/// Every event after the call, in order, never dropped (unbounded; events
/// are rare).
pub fn subscribe() -> Events;
impl Events {
    pub fn try_next(&self) -> Option<SystemEvent>;
    pub fn next_blocking(&self) -> SystemEvent;
}
/// The latest of each kind, for a subscriber that starts late.
pub fn current() -> SystemState;

/// Feature `tokio` (enables `system-events`; adds tokio with
/// default-features off and `sync`).
pub fn subscribe_tokio() -> tokio::sync::mpsc::UnboundedReceiver<SystemEvent>;
```

- The Kotlin callbacks push into each subscriber's queue on whatever
  thread runs them. Nothing waits for the main loop or for frames.
- The callbacks are registered on the Application context at the first
  `subscribe()` and stay for the life of the process. `MobileActivityPlugin`
  does not change.
- `ServiceEvent`s also arrive on the main loop as
  `MobileEvent::Service(ServiceEvent)` when `foreground-service` is on, so
  an app without `system-events` still sees Exit and its buttons.
- No `futures-core` under any feature. Other runtimes (`smol`,
  `async-compat`) are later amendments, each feature named after its
  crate.
- Tests: the queue (order, no loss, a blocking wait woken from another
  thread) with no features; `subscribe_tokio` with `--features tokio`.

## 7. Open questions for the owner

1. **Microphone in step 4.** The owner's earlier scope was `specialUse`
   only. handlr now needs capture (voice confirmations) and playback
   (read-aloud) while the screen is off. Playback needs nothing more than
   the running service. Capture needs `Microphone` in the types when the
   service starts, while visible. Should step 4 check background capture
   on the S23 now? Recommended: yes, since handlr depends on it.
2. **Swiping the app away.** Should it stop the service by default
   (`stopWithTask="true"`, opt-out `foreground_service_keeps_running`), as
   proposed, or keep it running by default? Termux keeps running; with
   `ongoing` and `exit`, keeping it running may suit handlr better.
3. **The wake lock at start.** handlr's earlier question 2 is answered by
   the toggle: the user chooses. Should handlr start with it held or
   released? Recommended: released, as Termux does.

## 8. Steps

1. **The service.** Feature `foreground-service`, the Kotlin service and
   plugin, `Project::foreground_service`,
   `foreground_service_keeps_running`,
   `request_ignore_battery_optimizations`,
   `codegen::foreground_service_manifest` (tested), `android::service`
   with content, actions and controls, `android::battery`, and
   `MobileEvent::Service`.
   **Done when:** `cargo test --workspace` passes, clippy is clean on the
   host and on Android with every feature, and a generated project with
   each type builds with `gradle assembleDebug`.
2. **System events.** Feature `system-events`, the plugin, the subscriber
   queue, `subscribe_tokio` under `tokio`, and the tests of section 6.
   **Done when:** the checks of step 1, plus those tests.
3. **Example.** A `background` example. On its first resume it asks for
   `POST_NOTIFICATIONS` and then starts the service (`specialUse`, plus
   `Microphone` if question 1 is answered yes), with all three controls
   and one app action, `"ping"`. A plain thread holds a TCP connection to
   a host given by an intent extra (`--es host <ip:port>`), writes a line
   every 30 s, logs the reply and each `SystemEvent`, and updates the text
   with the count. With `Microphone`, it opens a capture stream and logs
   its level every 30 s.
   **Done when:** it builds and runs on the S23.
4. **On the S23.**
   **Done when:** on the owner's Galaxy S23 Ultra, with `nc -lk 7999` on
   the Mac:
   - with the screen off and the app in the background for over 10
     minutes, lines keep arriving every 30 s, with the battery setting on
     "Unrestricted" and on the default (both recorded);
   - swiping the notification does not remove it (it comes back);
   - Exit removes the notification, delivers `Exited`, closes the
     activity, and the example's thread stops sending;
   - the wake-lock button switches between its two labels, the text shows
     " (wake lock held)" while held, `dumpsys power` lists
     `libmobile:foreground` while held and not after, and `WakeLock(bool)`
     is logged;
   - `"ping"` is logged as `Action("ping")`;
   - Home and return log `Visible(false)` then `Visible(true)`; Wi-Fi off
     and on logs `Network(None)` then `Network(Some(..))`;
     `dumpsys deviceidle force-idle` then `unforce` logs `DeviceIdle(true)`
     then `DeviceIdle(false)`, and whether lines arrived in Doze, with and
     without the battery exemption, is recorded;
   - with `POST_NOTIFICATIONS` revoked, the service still starts and logs
     `NotificationHidden`;
   - a start from the background (an `am broadcast` hook in the example)
     gives `StartNotAllowed`;
   - swiping the app from Recents does what question 2 decides;
   - with `Microphone`: capture levels keep arriving with the screen off.

   The results go in CHANGES.md.
5. **Release** 0.2.3, on the owner's word. Additive: new features, modules,
   types, builder methods, and `MobileEvent::Service` (`MobileEvent` is
   `#[non_exhaustive]`; step 1 confirms it, or this becomes 0.3.0).
   **Done when:** the version commit is tagged `v0.2.3` and recorded in
   `VERSIONS.md`.

libmorph then wraps it under its own plan, which gets an amendment
pointing here once this plan is approved.

## Amendment — 2026-10-23 18:09 EDT: approved

The owner approved this plan on 2026-10-23 ("approve plan and build next
release"), without separate answers to section 7, so its recommendations
and proposals stand:
1. Step 4 checks background capture with `Microphone`, and the example
   declares and starts with it.
2. Swiping the app away stops the service by default
   (`stopWithTask="true"`); `foreground_service_keeps_running` opts out.
3. handlr starts with the wake lock released; the user acquires it from
   the notification.

The status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-23 18:18 EDT: no background microphone

The owner's instruction (2026-10-23, relayed by the handlr session and
confirmed in the libmorph session: "approve plans in both repos"):
"microphone shouldn't work in the background, if it needs to be in the
background service that's a different question, but hiding the app
should stop the input. it's better to let Android enforce that rule than
do it ourselves".

This replaces answer 1 of the amendment "approved", section 7's question
1, and the microphone parts of steps 3 and 4:
- The service is for keeping a link and playback alive. When the app is
  hidden, Android stops microphone input itself, and libmobile does not
  try to keep or stop it.
- Step 3's `background` example declares and starts `specialUse` only,
  and opens no capture stream.
- Step 4 has no background-capture check.
- `ServiceType::Microphone` stays in the API and the manifest code as
  sections 3 and 4.2 define them, for a later plan.

## Amendment — 2026-10-23 18:20 EDT: implementation has started; the document is now immutable

Implementation has started: step 1 (the service) starts with this commit.
The status is now `In implementation`, and this document is immutable.
Departures are recorded in CHANGES.md.

## Amendment — 2026-10-23 22:02 EDT: the battery calls move to their own plan

On the owner's instruction (2026-10-23: "approve plans", approving
`docs/design/20261023_02_BATTERY_EXEMPTION_PLAN.md` with its question 1
answered yes). This replaces section 5 and the `android::battery` part
of step 1: the battery-optimization calls move to the feature `battery`
under that plan, return its `BatteryError`, and their Kotlin moves to
its `BatteryPlugin`. `Project::request_ignore_battery_optimizations`
stays. Step 4's battery-exemption comparison becomes that plan's step 3.
The code built for section 5 (a81a650) is moved by that plan's step 1;
nothing was released. Both ship together in 0.2.3.
