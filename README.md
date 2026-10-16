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
| _(always)_    | `bridge/*`                 | insets, intents, and text input                             |
| `google-auth` | `auth/GoogleAuthPlugin`    | `androidx.credentials` and `googleid` AARs; `MobileEvent::GoogleLogin` |
| `camera`      | `camera/CameraPlugin`      | an `IMAGE_CAPTURE` `<queries>` entry; `MobileEvent::Capture` |
| `cpal`        | none                       | cpal (AAudio), re-exported as `libmobile::android::cpal`    |

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
