# libmobile

An extended `NativeActivity` framework for Rust apps on Android.

Your app draws every pixel, in Rust, with whatever renderer you like. libmobile
handles the platform pieces `NativeActivity` leaves out: window insets, intents
delivered to a running instance, IME text, and optional system integrations
(Sign in with Google, camera capture). It feeds all of these into one
non-blocking main loop.

- **One crate.** The Rust core, the JNI bridges, and the Kotlin sources
  (`android/java/dev/highbay/libmobile/`) ship together. The Android package
  is `dev.highbay.libmobile`.
- **Plugins are chosen at compile time.** `build.rs` reads the active Cargo
  features and stages only the Kotlin those features need. It also writes
  `GeneratedPlugins.kt`, a static list of the active `MobileActivityPlugin`s,
  so nothing is discovered at runtime.
- **Gradle gets wired up for you.** The build script writes `libmobile.gradle`.
  It adds the staged sources to `android.sourceSets.main` and applies each
  plugin's contributions: AAR dependencies, permissions, and manifest elements.
  The app's own `build.rs` can also generate the entire Gradle project, so the
  app needs no Gradle files of its own.
- **The main loop never blocks.** Lifecycle polling, bridge mailboxes, input,
  and your `tick` all run on the main thread. Background work runs on worker
  threads and sends results back over `mpsc` channels, which the loop drains
  with `try_recv`.

## Layout

```
android/java/dev/highbay/libmobile/
  MobileNativeActivity.kt   NativeActivity subclass; dispatches the lifecycle to plugins
  MobileActivityPlugin.kt   plugin interface (onCreate, onResume, onPause, onDestroy,
                            onNewIntent, onWindowAttached, onActivityResult)
  NativeLib.kt              loads the app's cdylib so JNI exports register
  bridge/                   core bridges: window insets, intents, text input
  auth/                     feature "google-auth": Credential Manager sign-in
  camera/                   feature "camera": capture through the system camera app
  service/                  feature "foreground-service": the foreground service and its notification
  battery/                  feature "battery": battery optimization and the battery settings pages
  events/                   feature "system-events": visibility, network and Doze callbacks
src/
  codegen.rs                plugin registry and rendering for GeneratedPlugins.kt and libmobile.gradle
  gradle.rs                 generates the Gradle project from the app's build.rs
  event.rs, insets.rs, intent.rs, worker.rs, mailbox.rs   platform-free, host-tested
  android/                  JNI bridges and the main loop (target_os = "android")
examples/
  bitmap/                   CPU rendering into the ANativeWindow buffer
  wgpu/                     a full-screen WGSL shader through wgpu
```

## Cargo features

| Feature       | Kotlin plugin              | Adds                                                        |
|---------------|----------------------------|-------------------------------------------------------------|
| _(always)_    | `bridge/*`                 | insets, intents, text input, opening links, and app directories |
| `google-auth` | `auth/GoogleAuthPlugin`    | `androidx.credentials` and `googleid` AARs; `MobileEvent::GoogleLogin` |
| `camera`      | `camera/CameraPlugin`      | an `IMAGE_CAPTURE` `<queries>` entry; `MobileEvent::Capture` |
| `cpal`        | none                       | cpal (AAudio), re-exported as `libmobile::android::cpal`    |
| `foreground-service` | `service/ForegroundServicePlugin` | `android::service`; `MobileEvent::Service` |
| `battery`     | `battery/BatteryPlugin`    | `android::battery`; a `<queries>` entry for Samsung's Device Care; with `system-events`, `SystemEvent::BatteryExemption` |
| `system-events` | `events/SystemEventsPlugin` | `system_events::subscribe` and `current`; `ACCESS_NETWORK_STATE` |
| `notifications` | `notify/NotifyPlugin`    | `android::notify` (alerts on the app's own channels); `MobileEvent::Notify`; `POST_NOTIFICATIONS`; with `system-events`, `SystemEvent::Notify` |
| `tokio`       | none                       | `system_events::subscribe_tokio` (turns on `system-events`) |
| `icons`       | none                       | `gradle::Icon` and `Project::icon`: the launcher icon from PNGs (build-dependency only; adds `image`) |

## Usage

The app crate is a `cdylib`. List libmobile twice: once as a normal dependency
(with the features you want) and once as a build-dependency (for the Gradle
generator).

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
libmobile = { version = "0.1", features = ["camera"] }
log = "0.4"

[build-dependencies]
libmobile = "0.1"
```

### The app

```rust
use libmobile::MobileEvent;
use libmobile::android::{AndroidApp, InputEvent, InputStatus, MobileApp, run};
use libmobile::worker::Workers;

struct App {
    jobs: Workers<String>,
}

impl MobileApp for App {
    fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
        match event {
            MobileEvent::WindowCreated => { /* build the swapchain */ }
            // Delivered synchronously, before Android reclaims the window:
            // drop anything that borrows it here.
            MobileEvent::WindowDestroyed => { /* drop the swapchain */ }
            MobileEvent::Insets(insets) => { /* lay out inside the safe area */ }
            MobileEvent::Intent(intent) => { /* e.g. `am start ... --es cmd clear` */ }
            _ => {}
        }
    }

    fn input(&mut self, _app: &AndroidApp, _event: &InputEvent<'_>) -> InputStatus {
        InputStatus::Unhandled
    }

    fn tick(&mut self, _app: &AndroidApp) {
        for result in self.jobs.drain() { // try_recv: never blocks
            log::info!("worker finished: {result}");
        }
        // draw
    }
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let jobs = Workers::new();
    jobs.spawn("fetch", || "done".to_string()).unwrap();
    run(app, App { jobs });
}
```

`android::SurfaceTarget` wraps the current `ANativeWindow` as a
`raw-window-handle` 0.6 target, so wgpu's safe `create_surface` accepts it.
`android_activity` and `ndk` are re-exported, so the app uses the same versions
libmobile links against.

### Gradle: generated

```rust
// build.rs
fn main() {
    libmobile::gradle::Project::new("com.example.app")
        .label("Example")
        .permission("android.permission.INTERNET")       // optional
        .url_scheme("example")                            // optional: open on example:// links
        .extra_script("android/extra.gradle.kts")         // optional, applied last
        .generate()
        .expect("gradle project");
}
```

On host builds `generate()` does nothing, so `cargo test` on the app is
unaffected. For an Android target it writes a complete project (settings, root
and app build scripts, manifest, and `local.properties` from `ANDROID_HOME`)
that packages the `.so` cargo just built:

```sh
cargo build --release --target aarch64-linux-android --lib
gradle -p target/aarch64-linux-android/release/gradle/<package> assembleDebug
```

### App icon

With the `icons` feature on the build-dependency, the build script makes
the launcher icon from PNGs:

```toml
[build-dependencies]
libmobile = { version = "0.2", features = ["icons"] }
```

```rust
use libmobile::gradle::{Icon, Project};

Project::new("com.example.app")
    .icon(
        Icon::png("icon/icon-1024.png")          // square, at least 432 px
            .background_color([0x20, 0x24, 0x2c]) // or .background_png(...); default white
            .monochrome_png("icon/mono-1024.png"), // optional: themed icons (Android 13+)
    )
    .generate()
    .expect("gradle project");
```

The master is scaled (Lanczos3) into the centre 66 dp of the 108 dp
adaptive icon; `.full_bleed()` uses artwork already made for that canvas
unchanged, and `Icon::pngs(&[(size, path), ...])` takes PNGs at known
sizes. The project gets the adaptive icon (`mipmap-anydpi-v26`), its
foreground, background and monochrome layers at every density, legacy
square and round `ic_launcher` PNGs (48 to 192 px), and `android:icon` /
`android:roundIcon` in the manifest. A source that is not a square PNG
fails `generate()` with an error naming it. Without `icon()`, nothing
changes. The feature stays off the app's normal dependency: nothing is
added to the APK's native code.

### Foreground service

With the `foreground-service` feature, a foreground service keeps the
process from being frozen in the background, so the app's own threads keep
running with the screen off. Declare it with its types in the build script:

```rust
use libmobile::gradle::ServiceType;

libmobile::gradle::Project::new("com.example.app")
    .foreground_service(&[ServiceType::SpecialUse("a link to the user's own server")])
    .generate()
    .expect("gradle project");
```

This adds the `<service>` element and its permissions (`FOREGROUND_SERVICE`,
`FOREGROUND_SERVICE_<TYPE>`, `POST_NOTIFICATIONS`, `WAKE_LOCK`). By default,
swiping the app away from Recents stops the service;
`.foreground_service_keeps_running()` changes that.

Start it while the app is visible, from any thread. Ask for
`POST_NOTIFICATIONS` first, or the notification is hidden:

```rust
use libmobile::android::service::{self, NotificationAction, NotificationContent, ServiceOptions};

let mut content = NotificationContent::new("Example", "connected");
content.actions.push(NotificationAction::new("ping", "Ping"));
let mut options = ServiceOptions::new(vec![ServiceType::SpecialUse("")], content);
options.controls.ongoing = true; // posted again if dismissed
options.controls.exit = true; // an Exit button
options.controls.wake_lock_toggle = true; // a wake-lock button
service::start(&options)?;
service::update_text("3 messages")?;
```

Button taps, Exit and the service's own changes arrive as
`MobileEvent::Service(ServiceEvent::…)`. Exit stops the service, delivers
`Exited` and finishes the activity; the app's threads end themselves.

`update` (title, text, buttons and tag) and `update_text` work from any
thread while the service runs, also with the activity stopped and no
frames drawn; each is posted at once, without sound or heads-up. A tap on
the notification's body opens the app and arrives as
`ServiceEvent::Tapped(tag)`, with `NotificationContent::tag` from the
latest `start` or `update`.

### Alert notifications

With the `notifications` feature, `android::notify` posts dismissable
alerts on the app's own channels, apart from the service's, from any
thread. The app asks for `POST_NOTIFICATIONS` itself; without it `post`
gives `NotifyError::PermissionDenied`.

```rust
use libmobile::android::notify::{self, Channel, Importance, NotificationAction, NotificationContent};

let channel = Channel::new("questions", "Questions", Importance::High);
let mut content = NotificationContent::new("Agent 3", "needs an approval");
content.tag = Some("agent-3".into()); // returned on a tap, at most 256 bytes
content.actions.push(NotificationAction::new("approve", "Approve"));
notify::post("agent-3", &content, &channel)?; // the same id replaces it
notify::cancel("agent-3")?;
```

The channel is created on its first post, and Android keeps its
importance and sound from then on: a different importance needs a new
channel id. A tap on the body opens the app, removes the alert and arrives
as `MobileEvent::Notify(NotifyEvent::Tapped { id, tag })`, also when it
starts the app; a button arrives as `NotifyEvent::Action { id, action }`.
`cancel_all()` removes the app's alerts, not the service's notification.

### Battery optimization

With the `battery` feature, `android::battery` reads the app's battery
settings and opens the pages where the user changes them. Call it when the
app decides to, from any thread, while the app is visible:

```rust
use libmobile::android::battery;

let state = battery::state()?; // ignoring_optimizations, background_restricted, manufacturer
if !state.ignoring_optimizations {
    // The Allow/Deny dialog. Needs `.request_ignore_battery_optimizations()`
    // in the build script. Not for apps on Google Play: use open_settings().
    battery::request_ignore_optimizations()?;
}
// On Samsung, the battery page where app sleeping is set; elsewhere, or if
// it is not found, the app's own settings page. Returns which opened.
let page = battery::open_vendor_battery()?;
```

`open_settings()` opens the list of every app's setting and
`open_app_details()` the app's own settings page. Android does not report
the answer: with `system-events`, `SystemEvent::BatteryExemption(bool)`
arrives when the exemption has changed, checked each time the app becomes
visible or focused.

### Opening a link

`android::link::open_link` asks Android to open an http or https link in
the app that handles it, normally the browser. Any thread, while the app is
visible:

```rust
use libmobile::android::link::{self, OpenLinkError};

match link::open_link("https://example.com/a?k=v") {
    Ok(()) => {}                           // sent; it does not wait for the browser
    Err(OpenLinkError::NoHandler) => {}    // no app takes it
    Err(OpenLinkError::NotVisible) => {}   // the app is hidden
    Err(err) => log::warn!("{err}"),       // BadUrl: not http or https
}
```

Other schemes (`file:`, `intent:`, `mailto:` and the rest) are refused.
`link::check_link` makes the same check without opening anything. The link
is logged at debug level without its query or fragment.

### App directories

`android::dirs::files_dir()` and `cache_dir()` give the app's private
directories (`Context.getFilesDir()` and `getCacheDir()`) from any thread,
with no `AndroidApp`. The files directory is kept until the app is
uninstalled or its data cleared, and is the same as
`AndroidApp::internal_data_path()`; Android may delete the cache
directory's files when storage is low. Both exist when returned. `None`
only if the JNI call fails, which is logged.

```rust
use libmobile::android::dirs;

let files = dirs::files_dir();   // Some("/data/user/0/<package>/files")
let cache = dirs::cache_dir();   // Some("/data/user/0/<package>/cache")
```

### Memory trim levels

`MobileEvent::TrimMemory(TrimLevel)` reports each
`ComponentCallbacks2.onTrimMemory` call, through the main loop like the
other bridges. Levels are ordered by pressure (`RunningModerate` up to
`Complete`); from API 34 Android sends only `UiHidden` and `Background`.
A level Android adds later maps to the nearest lower known one.
`MobileEvent::LowMemory` is unchanged. With `system-events`,
`SystemEvent::TrimMemory` also reaches subscribers on other threads.

```rust
use libmobile::{MobileEvent, TrimLevel};

match event {
    MobileEvent::TrimMemory(level) if level >= TrimLevel::Background => { /* drop caches */ }
    _ => {}
}
```

### Night mode and the system bars

`android::night_mode(&app)` reads the system's night (dark) mode from the
configuration: `Some(true)`, `Some(false)`, or `None` when undefined. A
change arrives as `MobileEvent::ConfigChanged`, without recreating the
activity; read it again then.

The app draws behind the status and navigation bars (libmobile makes every
app edge-to-edge), so it paints their background itself, in its own frame.
`android::set_system_bar_icons(status_dark, navigation_dark)` picks dark or
light icons to go on top, from any thread. From its first call the
system's contrast scrims are off and the bars' own colours transparent, so
only what the app draws shows behind them; the choice is applied again to
each new window.

```rust
use libmobile::android::{night_mode, set_system_bar_icons};

let dark = night_mode(app).unwrap_or(false);
set_system_bar_icons(!dark, !dark); // dark icons over a light theme
```

### Build info

`android::build_info()` gives the device's manufacturer, brand and model
(`Build.MANUFACTURER`, `BRAND` and `MODEL`) from any thread, read once and
kept. Only those three: libmobile never reads the serial, `ANDROID_ID`,
the IMEI, the advertising ID, the fingerprint, or anything else that
identifies the unit or its owner. A missing value is empty; `None` only if
the JNI call fails, which is logged.

```rust
if let Some(info) = libmobile::android::build_info() {
    // "samsung", "samsung", "SM-S918U"
    log::info!("{} {} {}", info.manufacturer, info.brand, info.model);
}
```

### System events

With the `system-events` feature, any thread can follow what happens to the
app, without the main loop or an async runtime:

```rust
use libmobile::system_events::{self, SystemEvent};

let events = system_events::subscribe();
std::thread::spawn(move || loop {
    match events.next_blocking() {
        SystemEvent::Visible(seen) => log::info!("visible: {seen}"),
        SystemEvent::Network(None) => log::info!("no network"),
        SystemEvent::DeviceIdle(false) => { /* Doze ended: reconnect */ }
        _ => {}
    }
});
```

Events are visibility, focus, the foreground service, the default network,
Doze and, with `battery`, the battery exemption. Each subscriber gets every event after it subscribes, in order.
`system_events::current()` gives the latest of each kind. With the `tokio`
feature, `subscribe_tokio()` returns a tokio `UnboundedReceiver` instead.

### Gradle: your own project

Skip `gradle::Project`. Apply the generated script from your app module
instead:

```kotlin
apply(from = "<target>/aarch64-linux-android/release/libmobile-gradle/libmobile.gradle")
```

In your manifest, set the activity to `dev.highbay.libmobile.MobileNativeActivity`
(or a subclass), point the `android.app.lib_name` meta-data at your cdylib, and
set `android:launchMode="singleTop"` so `onNewIntent` reaches the running
instance. Plugin permissions and manifest entries are still merged in by
`libmobile.gradle`.

To open the app from `myapp://` links (what `Project::url_scheme("myapp")`
generates), add a filter to the activity:

```xml
<intent-filter>
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />
    <data android:scheme="myapp" />
</intent-filter>
```

The link arrives as a `MobileEvent::Intent` whose `data` is the URL.
libmobile logs it only up to its path (`intent::redact_url`).

### Gradle: through a wrapper crate

Cargo hands libmobile's `links` metadata (`DEP_LIBMOBILE_GRADLE_SCRIPT`) only
to crates that depend on libmobile directly. A crate that wraps libmobile
declares its own `links` key and republishes the metadata from its build
script:

```rust
// the wrapper's build.rs (libmobile is also one of its build-dependencies)
fn main() {
    libmobile::gradle::forward_metadata();
}
```

The app then depends on the wrapper alone. It keeps libmobile only as a
build-dependency, and reads the wrapper's key:

```rust
// the app's build.rs
libmobile::gradle::Project::new("com.example.app")
    .env_prefix("libmorph") // the wrapper's `links`: DEP_LIBMORPH_GRADLE_SCRIPT
    .generate()
    .expect("gradle project");
```

## Examples

Both examples have no Gradle files of their own: each one's `build.rs` generates
the project. `examples/build.sh` runs both stages:

```sh
examples/build.sh bitmap             # build the APK
examples/build.sh wgpu --install     # build, install with adb, and launch
```

The script needs the `aarch64-linux-android` rustup target, an Android SDK
(platform 35) with an NDK, Gradle 9.x, and JDK 17 or 21. If it can't find
them, set `ANDROID_HOME`, `ANDROID_NDK_ROOT`, and `JAVA_HOME`.

### `bitmap`

No GPU at all. Each frame is computed on the CPU and written straight into the
window's locked buffer at about 30 fps. It shows the plumbing in its plainest
form:

- hatched edges mark the system bars and cutout reported by the insets bridge,
  and a bouncing box stays inside them;
- touches leave white dots;
- an intent to the running instance clears the dots:

  ```sh
  adb shell am start -n dev.highbay.libmobile.example.bitmap/dev.highbay.libmobile.MobileNativeActivity --es cmd clear
  ```

The scene renderer is platform-free, so `cargo test -p libmobile-example-bitmap`
runs on the host.

### `wgpu`

A full-screen WGSL fragment shader (`src/shader.wgsl`). Its uniforms carry the
surface size, the time, the safe area, and the last touch. The part worth
copying is the surface lifecycle:

- the device and pipeline are built once and survive backgrounding;
- `WindowCreated` builds a swapchain from `SurfaceTarget`;
- `WindowDestroyed` drops it synchronously, inside the poll callback, before
  Android takes the window back.

A tap, or `--es cmd open-link`, opens `https://example.com/a?k=secret`
in the browser; `--es cmd open-link-later` tries again from a thread after
5 s, so a call while the app is hidden can be tried after Home.
At start it logs the files and cache directories (`android::dirs`) and
whether the files directory is `internal_data_path()`, and the build info
(`android::build_info`). It logs each `MobileEvent::TrimMemory`
(`adb shell am send-trim-memory dev.highbay.libmobile.example.wgpu UI_HIDDEN`).

## Development

```sh
cargo test --workspace                     # host: codegen, gradle, platform-free types
cargo test -p libmobile --all-features
cargo clippy --target aarch64-linux-android --workspace --all-features
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
