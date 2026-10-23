//! # libmobile
//!
//! An extended `NativeActivity` framework for Rust apps on Android. The app
//! owns every pixel (Rust + its renderer of choice); libmobile owns the
//! platform seams `NativeActivity` does not cover — window insets, intents to
//! a running instance, IME text, and feature-gated system integrations — and a
//! non-blocking main loop to fold them into.
//!
//! ## Pieces
//!
//! * **Kotlin** (`android/java/dev/highbay/libmobile/`): `MobileNativeActivity`
//!   extends `NativeActivity` and dispatches its lifecycle to every
//!   `MobileActivityPlugin` in `GeneratedPlugins.activePlugins`. Core bridges
//!   live in `bridge/`; each feature-gated plugin in a directory of its own.
//! * **`build.rs`** stages the Kotlin sources the active Cargo features select,
//!   writes `GeneratedPlugins.kt`, and writes `libmobile.gradle`, which appends
//!   the staged root to Gradle's `sourceSets` and applies the plugins'
//!   contributions (AARs, permissions, manifest elements). See [`codegen`].
//! * **Rust**: platform-free types ([`insets`], [`intent`], [`event`],
//!   [`worker`], [`mailbox`]) that build and test on any host, and the
//!   `android` module (target_os = "android") with the JNI bridges and
//!   [`android::run`].
//!
//! ## Using it
//!
//! ```ignore
//! // The app crate: crate-type = ["cdylib"].
//! use libmobile::android::{AndroidApp, MobileApp, run};
//! use libmobile::MobileEvent;
//!
//! struct App;
//! impl MobileApp for App {
//!     fn event(&mut self, app: &AndroidApp, event: MobileEvent) { /* ... */ }
//! }
//!
//! #[unsafe(no_mangle)]
//! fn android_main(app: AndroidApp) {
//!     run(app, App);
//! }
//! ```
//!
//! Gradle side: let the app's `build.rs` generate the whole project
//! ([`gradle::Project`]), or, in a project of your own, apply the generated
//! script from the app module:
//!
//! ```kotlin
//! apply(from = "<target>/aarch64-linux-android/release/libmobile-gradle/libmobile.gradle")
//! ```
//!
//! and in that manifest, make the activity `dev.highbay.libmobile.MobileNativeActivity`
//! (or a subclass) with `android.app.lib_name` naming the app's cdylib and
//! `android:launchMode="singleTop"` so `onNewIntent` reaches the running
//! instance. Either way, `libmobile.gradle` brings each active plugin's AARs
//! and manifest entries ([`codegen::Contributes`]).

// deny (not forbid): each JNI export needs one scoped `#[allow(unsafe_code)]`
// for `#[unsafe(no_mangle)]`, and the Rust -> Kotlin calls one for borrowing
// the activity reference android-activity owns. Everything else is safe.
#![deny(unsafe_code)]

#[cfg(feature = "cpal")]
pub mod audio_capture;
#[cfg(feature = "google-auth")]
pub mod auth;
#[cfg(feature = "camera")]
pub mod camera;
pub mod codegen;
pub mod event;
pub mod gradle;
pub mod insets;
pub mod intent;
pub mod key_store;
pub mod mailbox;
pub mod secure_store;
#[cfg(feature = "foreground-service")]
pub mod service;
#[cfg(feature = "foreground-service")]
pub mod system_events;
pub mod worker;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(feature = "cpal")]
pub use audio_capture::{CaptureError, CaptureFormat, InputPreset};
pub use event::{MobileEvent, PermissionRequest, PermissionsOutcome};
pub use insets::{Insets, SafeArea};
pub use intent::IntentData;
pub use secure_store::{PutOptions, SecureStoreError, SecurityLevel};
