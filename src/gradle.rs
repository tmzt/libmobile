//! **Generate the whole Gradle project from the app's `build.rs`.**
//!
//! An app needs no hand-written Gradle at all: its build script describes the
//! app, and this writes a complete project (settings, root and app build
//! scripts, manifest) that packages the cdylib cargo is building.
//!
//! ```toml
//! # the app's Cargo.toml
//! [dependencies]
//! libmobile = { version = "0.1", features = ["camera"] }
//! [build-dependencies]
//! libmobile = "0.1"   # host copy, for this module only
//! ```
//!
//! ```no_run
//! // the app's build.rs
//! # #[allow(clippy::needless_doctest_main)]
//! fn main() {
//!     libmobile::gradle::Project::new("com.example.app")
//!         .label("Example")
//!         .extra_script("android/extra.gradle.kts") // optional customisation
//!         .generate()
//!         .expect("gradle project");
//! }
//! ```
//!
//! Then `cargo build --release --target aarch64-linux-android --lib`, and
//! `gradle -p target/aarch64-linux-android/release/gradle/<package> assembleDebug`.
//!
//! **Prefer your own project?** Don't call this; apply `libmobile.gradle`
//! from your own app module instead (see [`crate::codegen`]). Or keep the
//! generated one and layer on top with [`Project::extra_script`], applied last.
//!
//! **Plugin contributions** (AARs, permissions, manifest elements) are not
//! rendered here: they live in `libmobile.gradle`, which the generated app
//! module applies, so generated and hand-written projects get them the same
//! way. That script belongs to the *normal* libmobile dependency (the one with
//! the app's features); its build script publishes the path through `links`
//! metadata (`DEP_LIBMOBILE_GRADLE_SCRIPT`), which cargo hands only to the
//! build scripts of direct dependents — so call this from the app crate's
//! `build.rs`.
//!
//! **Through a wrapper crate.** When the app depends on a crate that wraps
//! libmobile instead of on libmobile itself, the metadata stops at the
//! wrapper. The wrapper declares its own `links` key and republishes it from
//! its build script with [`forward_metadata`]:
//!
//! ```toml
//! # the wrapper's Cargo.toml
//! [package]
//! links = "libmorph"
//! [dependencies]
//! libmobile = { version = "0.1", optional = true }
//! [build-dependencies]
//! libmobile = { version = "0.1", optional = true }
//! ```
//!
//! ```no_run
//! // the wrapper's build.rs
//! # #[allow(clippy::needless_doctest_main)]
//! fn main() {
//!     libmobile::gradle::forward_metadata();
//! }
//! ```
//!
//! and the app, which still lists libmobile as a build-dependency only (it is
//! not linked into the app twice), reads the wrapper's key:
//!
//! ```no_run
//! // the app's build.rs
//! libmobile::gradle::Project::new("com.example.app")
//!     .env_prefix("libmorph") // reads DEP_LIBMORPH_GRADLE_SCRIPT
//!     .generate()
//!     .expect("gradle project");
//! ```

pub use crate::codegen::ServiceType;
use crate::codegen::{self, GradleProject};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// libmobile's own `links` key, the default [`Project::env_prefix`].
pub const DEFAULT_ENV_PREFIX: &str = "LIBMOBILE";

/// The `links` metadata keys libmobile's build script publishes:
/// `GRADLE_SCRIPT` (the path to `libmobile.gradle` for the active features)
/// and `FEATURES` (those features, comma separated).
pub const METADATA_KEYS: [&str; 2] = ["GRADLE_SCRIPT", "FEATURES"];

/// `libmorph`, `LIBMORPH`, `DEP_LIBMORPH` and `DEP_LIBMORPH_` all name the
/// `links = "libmorph"` key, as the `LIBMORPH` part of `DEP_LIBMORPH_*`.
fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim().to_ascii_uppercase().replace('-', "_");
    let prefix = prefix.strip_prefix("DEP_").unwrap_or(&prefix);
    prefix.trim_end_matches('_').to_string()
}

/// The variable a direct dependent's build script sees for `key` under the
/// `links` key `prefix`.
fn metadata_var(prefix: &str, key: &str) -> String {
    format!("DEP_{}_{key}", normalize_prefix(prefix))
}

/// Republish libmobile's `links` metadata from the build script of a crate
/// that wraps it, so the wrapper's dependents see it under the wrapper's own
/// `links` key (`DEP_<WRAPPER>_GRADLE_SCRIPT`, ...). The wrapper must declare
/// `links` and list libmobile as a normal dependency, plus a build-dependency
/// to call this. Returns false, publishing nothing, when libmobile is not a
/// normal dependency of this build (e.g. an optional one switched off).
pub fn forward_metadata() -> bool {
    let lines = forward_lines(|var| env::var(var).ok());
    for key in METADATA_KEYS {
        println!(
            "cargo:rerun-if-env-changed={}",
            metadata_var(DEFAULT_ENV_PREFIX, key)
        );
    }
    for line in &lines {
        println!("{line}");
    }
    !lines.is_empty()
}

/// The `cargo:` lines [`forward_metadata`] prints, given an env lookup.
fn forward_lines(var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    if var(&metadata_var(DEFAULT_ENV_PREFIX, "GRADLE_SCRIPT")).is_none() {
        return Vec::new();
    }
    METADATA_KEYS
        .iter()
        .filter_map(|key| {
            var(&metadata_var(DEFAULT_ENV_PREFIX, key))
                .map(|value| format!("cargo:{}={value}", key.to_ascii_lowercase()))
        })
        .collect()
}

/// Builder over [`GradleProject`] that fills the rest from the cargo build.
pub struct Project {
    spec: GradleProject,
    out_dir: Option<PathBuf>,
    env_prefix: String,
    url_schemes: Vec<String>,
    /// The declared foreground service types, if any.
    service_types: Option<Vec<ServiceType>>,
    service_keeps_running: bool,
    #[cfg(feature = "icons")]
    icon: Option<Icon>,
}

impl Project {
    /// `application_id` doubles as the Gradle namespace. The library name
    /// defaults to the package name with `-` → `_` (cargo's own default).
    pub fn new(application_id: &str) -> Self {
        let lib = env::var("CARGO_PKG_NAME")
            .unwrap_or_default()
            .replace('-', "_");
        Project {
            spec: GradleProject::new(application_id, &lib),
            out_dir: None,
            env_prefix: normalize_prefix(DEFAULT_ENV_PREFIX),
            url_schemes: Vec::new(),
            service_types: None,
            service_keeps_running: false,
            #[cfg(feature = "icons")]
            icon: None,
        }
    }

    /// Read the `links` metadata under another key: the `links` value of
    /// the crate that wraps libmobile and republishes it with
    /// [`forward_metadata`] (`"libmorph"` reads `DEP_LIBMORPH_GRADLE_SCRIPT`).
    /// Defaults to [`DEFAULT_ENV_PREFIX`], for apps that depend on libmobile
    /// directly.
    pub fn env_prefix(mut self, prefix: &str) -> Self {
        self.env_prefix = normalize_prefix(prefix);
        self
    }

    /// Override the cdylib name (when `[lib] name` differs from the package).
    pub fn lib_name(mut self, name: &str) -> Self {
        self.spec.lib_name = name.to_string();
        self
    }

    pub fn label(mut self, label: &str) -> Self {
        self.spec.label = label.to_string();
        self
    }

    /// A `MobileNativeActivity` subclass of your own, fully qualified.
    pub fn activity(mut self, class: &str) -> Self {
        self.spec.activity = class.to_string();
        self
    }

    /// An app-level `uses-permission` (plugins declare their own).
    pub fn permission(mut self, name: &str) -> Self {
        self.spec.permissions.push(name.to_string());
        self
    }

    /// Open the app for links with this scheme (`handlr` for `handlr://…`).
    /// Lowercase letters, digits, `+`, `-` and `.`, starting with a letter
    /// (RFC 3986); panics otherwise. May be called more than once. The link
    /// arrives as a `MobileEvent::Intent` with action
    /// `android.intent.action.VIEW` and the URL as its `data`.
    pub fn url_scheme(mut self, scheme: &str) -> Self {
        assert!(
            valid_scheme(scheme),
            "libmobile: invalid URL scheme {scheme:?} (lowercase letters, digits, +, - and ., \
             starting with a letter)"
        );
        if !self.url_schemes.iter().any(|s| s == scheme) {
            self.url_schemes.push(scheme.to_string());
        }
        self
    }

    /// Declare the foreground service with the types it may be started
    /// with (`libmobile::android::service`). Adds the `<service>` element,
    /// the `specialUse` reason, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_<TYPE>`
    /// for each type, `POST_NOTIFICATIONS` and `WAKE_LOCK`. `RECORD_AUDIO` for
    /// [`ServiceType::Microphone`] stays the app's own [`Project::permission`].
    /// Without the `foreground-service` feature on the libmobile the app
    /// links, [`Project::generate`] fails with an error that names the
    /// feature. Panics on an empty list. A later call replaces the types.
    pub fn foreground_service(mut self, types: &[ServiceType]) -> Self {
        assert!(
            !types.is_empty(),
            "libmobile: foreground_service needs at least one type"
        );
        self.service_types = Some(types.to_vec());
        self
    }

    /// Swiping the app away from Recents leaves the service running
    /// (`stopWithTask="false"`), and the service reports
    /// `ServiceEvent::TaskRemoved`. By default swiping it away stops the
    /// service.
    pub fn foreground_service_keeps_running(mut self) -> Self {
        self.service_keeps_running = true;
        self
    }

    /// Declare `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, so
    /// `android::battery::request_ignore_optimizations` (feature `battery`)
    /// may ask the user directly.
    /// Play allows that only for some kinds of app; see that function.
    pub fn request_ignore_battery_optimizations(mut self) -> Self {
        if !self
            .spec
            .permissions
            .iter()
            .any(|p| p == IGNORE_BATTERY_OPTIMIZATIONS)
        {
            self.spec
                .permissions
                .push(IGNORE_BATTERY_OPTIMIZATIONS.to_string());
        }
        self
    }

    /// The launcher icon, made from PNGs into the project's resources
    /// (`mipmap-*`, the adaptive icon) with `android:icon` and
    /// `android:roundIcon` on `<application>`. Without it the app has the
    /// system's default icon. A later call replaces the icon.
    #[cfg(feature = "icons")]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn sdk(mut self, min: u32, target: u32, compile: u32) -> Self {
        self.spec.min_sdk = min;
        self.spec.target_sdk = target;
        self.spec.compile_sdk = compile;
        self
    }

    pub fn version(mut self, code: u32, name: &str) -> Self {
        self.spec.version_code = code;
        self.spec.version_name = name.to_string();
        self
    }

    /// A Gradle script (relative to the crate root, or absolute) the app
    /// module applies after everything generated: signing, extra
    /// dependencies, resources.
    pub fn extra_script(mut self, path: impl AsRef<Path>) -> Self {
        self.spec.extra_scripts.push(crate_path(path.as_ref()));
        self
    }

    /// Write the project somewhere other than the default
    /// `<target>/<triple>/<profile>/gradle/<package>/`.
    /// `$LIBMOBILE_GRADLE_PROJECT_DIR` overrides both.
    pub fn out_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.out_dir = Some(crate_path(dir.as_ref()));
        self
    }

    /// Write the project and return its directory. `Ok(None)` (nothing
    /// written) when the build is not for Android, so host builds and tests
    /// of the app crate are unaffected.
    pub fn generate(mut self) -> io::Result<Option<PathBuf>> {
        for var in [
            "LIBMOBILE_GRADLE_PROJECT_DIR",
            "ANDROID_HOME",
            "ANDROID_SDK_ROOT",
        ] {
            println!("cargo:rerun-if-env-changed={var}");
        }
        if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
            return Ok(None);
        }
        let target = env_var("TARGET")?;
        let abi = codegen::android_abi(&target)
            .ok_or_else(|| io::Error::other(format!("no Android ABI for target {target}")))?;
        let script_var = metadata_var(&self.env_prefix, "GRADLE_SCRIPT");
        println!("cargo:rerun-if-env-changed={script_var}");
        let libmobile_gradle = env::var_os(&script_var).map(PathBuf::from).ok_or_else(|| {
            io::Error::other(if self.env_prefix == DEFAULT_ENV_PREFIX {
                format!(
                    "{script_var} is unset: libmobile must also be a normal [dependencies] \
                         entry of the crate whose build.rs calls this (or, through a wrapper \
                         crate, set Project::env_prefix to the wrapper's `links` key)"
                )
            } else {
                format!(
                    "{script_var} is unset: the crate with `links = \"{}\"` must be a \
                         normal [dependencies] entry of this crate, with libmobile enabled, and \
                         call libmobile::gradle::forward_metadata() from its build.rs",
                    self.env_prefix.to_ascii_lowercase()
                )
            })
        })?;
        let features_var = metadata_var(&self.env_prefix, "FEATURES");
        println!("cargo:rerun-if-env-changed={features_var}");
        let service = match &self.service_types {
            Some(types) => {
                check_service_feature(env::var(&features_var).ok().as_deref())?;
                Some(codegen::foreground_service_manifest(
                    types,
                    self.service_keeps_running,
                ))
            }
            None => None,
        };
        if let Some(service) = &service {
            for permission in &service.permissions {
                if !self.spec.permissions.iter().any(|p| p == permission) {
                    self.spec.permissions.push(permission.to_string());
                }
            }
        }
        // Read and check every icon source before writing anything.
        #[cfg(feature = "icons")]
        let icon_files = match &self.icon {
            Some(icon) => {
                for path in icon.paths() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
                let (files, warnings) = icon.render()?;
                for warning in warnings {
                    println!("cargo:warning={warning}");
                }
                files
            }
            None => Vec::new(),
        };
        #[cfg(not(feature = "icons"))]
        let icon_files: Vec<(String, Vec<u8>)> = Vec::new();
        let profile_dir = profile_dir(&PathBuf::from(env_var("OUT_DIR")?))?;
        let native_lib = profile_dir.join(format!("lib{}.so", self.spec.lib_name));

        let dir = match (
            env::var_os("LIBMOBILE_GRADLE_PROJECT_DIR"),
            self.out_dir.take(),
        ) {
            (Some(dir), _) if !dir.is_empty() => PathBuf::from(dir),
            (_, Some(dir)) => dir,
            _ => profile_dir
                .join("gradle")
                .join(env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "app".into())),
        };
        for (name, contents) in self.spec.render(&libmobile_gradle, &native_lib, abi) {
            let contents = if name == MANIFEST {
                self.finish_manifest(&contents, service.as_ref())
            } else {
                contents
            };
            write_if_changed(&dir.join(name), &contents)?;
        }
        for (name, contents) in &icon_files {
            write_if_changed(&dir.join(name), contents)?;
        }
        // Icon files an earlier build made and this one does not.
        for name in codegen::icon_resource_paths() {
            if !icon_files.iter().any(|(n, _)| *n == name) {
                remove_if_present(&dir.join(name))?;
            }
        }
        // Remember the SDK cargo was run with, so a bare `gradle -p <dir>`
        // works without ANDROID_HOME in that shell.
        if let Some(sdk) = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .find_map(|var| env::var(var).ok().filter(|v| !v.is_empty()))
        {
            write_if_changed(&dir.join("local.properties"), local_properties(&sdk))?;
        }
        Ok(Some(dir))
    }
}

const MANIFEST: &str = "app/src/main/AndroidManifest.xml";

impl Project {
    /// The rendered manifest with what the builder adds: the URL scheme
    /// filters, the service and the icon attributes.
    fn finish_manifest(
        &self,
        manifest: &str,
        service: Option<&codegen::ServiceManifest>,
    ) -> String {
        let manifest = with_url_schemes(manifest, &self.url_schemes);
        let manifest = match service {
            Some(service) => with_service(&manifest, &service.application),
            None => manifest,
        };
        #[cfg(feature = "icons")]
        let manifest = match &self.icon {
            Some(_) => with_icon(&manifest),
            None => manifest,
        };
        manifest
    }
}

/// The rendered manifest with `android:icon` and `android:roundIcon` on
/// `<application>`.
#[cfg(feature = "icons")]
fn with_icon(manifest: &str) -> String {
    const AFTER: &str = "        android:hasCode=\"true\"\n";
    manifest.replacen(AFTER, &format!("{AFTER}{}", codegen::ICON_ATTRIBUTES), 1)
}

/// RFC 3986 `scheme`, lowercase only (Android matches schemes
/// case-sensitively, and links arrive lowercased).
fn valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c))
}

/// The rendered manifest with a VIEW filter per scheme, before the end of
/// the activity element.
fn with_url_schemes(manifest: &str, schemes: &[String]) -> String {
    if schemes.is_empty() {
        return manifest.to_string();
    }
    let filters = codegen::url_scheme_filters(schemes);
    manifest.replacen(
        "        </activity>",
        &format!("{filters}        </activity>"),
        1,
    )
}

/// The rendered manifest with the service element at the end of
/// `<application>`.
fn with_service(manifest: &str, service: &str) -> String {
    manifest.replacen(
        "    </application>",
        &format!("{service}    </application>"),
        1,
    )
}

const IGNORE_BATTERY_OPTIMIZATIONS: &str =
    "android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS";

/// The feature the service needs, given the `FEATURES` metadata of the
/// libmobile the app links.
fn check_service_feature(features: Option<&str>) -> io::Result<()> {
    let on = features.is_some_and(|f| f.split(',').any(|f| f.trim() == "foreground-service"));
    if on {
        Ok(())
    } else {
        Err(io::Error::other(
            "Project::foreground_service needs libmobile's `foreground-service` feature \
             on the libmobile the app links (or its wrapper's passthrough)",
        ))
    }
}

/// `sdk.dir`, escaped for a `.properties` file (`\\` and `:` are special).
fn local_properties(sdk: &str) -> String {
    let escaped = sdk.replace('\\', "\\\\").replace(':', "\\:");
    format!("# GENERATED by libmobile::gradle from ANDROID_HOME. Do not edit.\nsdk.dir={escaped}\n")
}

/// `<profile>/build/<pkg>-<hash>/out` → `<profile>`.
fn profile_dir(out_dir: &Path) -> io::Result<PathBuf> {
    out_dir
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other(format!("unexpected OUT_DIR {}", out_dir.display())))
}

fn crate_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match env::var_os("CARGO_MANIFEST_DIR") {
        Some(root) => PathBuf::from(root).join(path),
        None => path.to_path_buf(),
    }
}

fn env_var(name: &str) -> io::Result<String> {
    env::var(name).map_err(|_| io::Error::other(format!("{name} is unset (not under cargo?)")))
}

/// Unchanged files keep their mtime, so Gradle's up-to-date checks hold.
fn write_if_changed(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let contents = contents.as_ref();
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(feature = "icons")]
pub use icons::Icon;

#[cfg(feature = "icons")]
mod icons {
    use super::crate_path;
    use crate::codegen::{self, IconBackground};
    use crate::icon::{self, Background, Sources};
    use image::RgbaImage;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    /// Generated files, as `(path relative to the project, bytes)`.
    type Files = Vec<(String, Vec<u8>)>;

    /// Below this a source is scaled up to the largest adaptive layer.
    const MIN_SIZE: u32 = 432;

    /// The app's launcher icon, from PNGs (feature `icons`), for
    /// [`Project::icon`](super::Project::icon).
    ///
    /// ```no_run
    /// use libmobile::gradle::{Icon, Project};
    ///
    /// Project::new("com.example.app")
    ///     .icon(
    ///         Icon::png("icon/icon-1024.png")
    ///             .background_color([0x20, 0x40, 0x80])
    ///             .monochrome_png("icon/monochrome-1024.png"),
    ///     )
    ///     .generate()
    ///     .expect("gradle project");
    /// ```
    ///
    /// Paths are relative to the crate root (or absolute). Each source is a
    /// square PNG, treated as sRGB; one that is not a PNG, not square or
    /// cannot be read fails [`Project::generate`](super::Project::generate)
    /// with an error naming the file. Every size is scaled from them with
    /// Lanczos3.
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct Icon {
        /// `(declared size, path)`; `None` for the single master.
        artwork: Vec<(Option<u32>, PathBuf)>,
        background: BackgroundSource,
        monochrome: Option<PathBuf>,
        full_bleed: bool,
    }

    #[derive(Clone, Debug)]
    enum BackgroundSource {
        Color([u8; 3]),
        Png(PathBuf),
    }

    impl Icon {
        /// One square master PNG, at least 432 px (1024 recommended), with
        /// transparency around the artwork. Every size is made from it. A
        /// smaller one is scaled up, with a `cargo:warning`.
        pub fn png(path: impl AsRef<Path>) -> Self {
            Icon::new(vec![(None, crate_path(path.as_ref()))])
        }

        /// PNGs at known sizes, each square: an output whose artwork is that
        /// many pixels uses it unscaled, and a size not given is made from
        /// the largest one given. Panics on an empty list.
        pub fn pngs(sizes: &[(u32, &Path)]) -> Self {
            assert!(
                !sizes.is_empty(),
                "libmobile: Icon::pngs needs at least one PNG"
            );
            Icon::new(
                sizes
                    .iter()
                    .map(|(size, path)| (Some(*size), crate_path(path)))
                    .collect(),
            )
        }

        fn new(artwork: Vec<(Option<u32>, PathBuf)>) -> Self {
            Icon {
                artwork,
                background: BackgroundSource::Color([0xff, 0xff, 0xff]),
                monochrome: None,
                full_bleed: false,
            }
        }

        /// The adaptive icon's background: a colour (the default, white) ...
        pub fn background_color(mut self, rgb: [u8; 3]) -> Self {
            self.background = BackgroundSource::Color(rgb);
            self
        }

        /// ... or a square PNG, used at full bleed.
        pub fn background_png(mut self, path: impl AsRef<Path>) -> Self {
            self.background = BackgroundSource::Png(crate_path(path.as_ref()));
            self
        }

        /// A single-colour PNG for themed icons (Android 13+), laid out like
        /// the foreground.
        pub fn monochrome_png(mut self, path: impl AsRef<Path>) -> Self {
            self.monochrome = Some(crate_path(path.as_ref()));
            self
        }

        /// The master already fills the 108 dp adaptive canvas (its artwork
        /// inside the centre 66 dp); use it as the foreground unchanged.
        /// Without this, the master is scaled into the centre 66 dp.
        pub fn full_bleed(mut self) -> Self {
            self.full_bleed = true;
            self
        }

        /// Every source file.
        pub(super) fn paths(&self) -> Vec<&Path> {
            let mut paths: Vec<&Path> = self.artwork.iter().map(|(_, p)| p.as_path()).collect();
            if let BackgroundSource::Png(path) = &self.background {
                paths.push(path);
            }
            paths.extend(self.monochrome.as_deref());
            paths
        }

        /// The generated files, as `(path relative to the project, bytes)`,
        /// and the warnings to print.
        pub(super) fn render(&self) -> io::Result<(Files, Vec<String>)> {
            let mut warnings = Vec::new();
            let mut artwork = Vec::new();
            for (declared, path) in &self.artwork {
                let image = read_png(path)?;
                if let Some(size) = declared
                    && image.width() != *size
                {
                    return Err(invalid(
                        path,
                        &format!("is {0}x{0} px, declared as {size} px", image.width()),
                    ));
                }
                artwork.push(image);
            }
            let largest = (0..artwork.len())
                .max_by_key(|&i| artwork[i].width())
                .expect("an icon has artwork");
            warn_if_small(&self.artwork[largest].1, &artwork[largest], &mut warnings);
            let background_png = match &self.background {
                BackgroundSource::Png(path) => {
                    let image = read_png(path)?;
                    warn_if_small(path, &image, &mut warnings);
                    Some(image)
                }
                BackgroundSource::Color(_) => None,
            };
            let monochrome = match &self.monochrome {
                Some(path) => {
                    let image = read_png(path)?;
                    warn_if_small(path, &image, &mut warnings);
                    Some(image)
                }
                None => None,
            };
            let (background, xml_background) = match (&self.background, &background_png) {
                (BackgroundSource::Color(rgb), _) => {
                    (Background::Color(*rgb), IconBackground::Color(*rgb))
                }
                (BackgroundSource::Png(_), Some(png)) => {
                    (Background::Png(png), IconBackground::Png)
                }
                (BackgroundSource::Png(_), None) => unreachable!("read above"),
            };
            let mut files = icon::icon_images(&Sources {
                artwork: &artwork,
                background,
                monochrome: monochrome.as_ref(),
                full_bleed: self.full_bleed,
            });
            files.extend(
                codegen::adaptive_icon_xml(xml_background, monochrome.is_some())
                    .into_iter()
                    .map(|(name, xml)| (name, xml.into_bytes())),
            );
            Ok((files, warnings))
        }
    }

    fn warn_if_small(path: &Path, image: &RgbaImage, warnings: &mut Vec<String>) {
        if image.width() < MIN_SIZE {
            warnings.push(format!(
                "libmobile: icon {} is {} px, under {MIN_SIZE} px; it is scaled up",
                path.display(),
                image.width()
            ));
        }
    }

    fn invalid(path: &Path, what: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("libmobile: icon {} {what}", path.display()),
        )
    }

    /// A square PNG, decoded to RGBA.
    pub(super) fn read_png(path: &Path) -> io::Result<RgbaImage> {
        const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
        let bytes = fs::read(path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("libmobile: icon {} cannot be read: {err}", path.display()),
            )
        })?;
        if !bytes.starts_with(SIGNATURE) {
            return Err(invalid(path, "is not a PNG"));
        }
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|err| invalid(path, &format!("is not a valid PNG: {err}")))?
            .to_rgba8();
        if image.width() != image.height() || image.width() == 0 {
            return Err(invalid(
                path,
                &format!("is not square ({}x{} px)", image.width(), image.height()),
            ));
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_schemes_are_validated() {
        for ok in ["handlr", "libmobile-wgpu", "a1+b.c"] {
            assert!(valid_scheme(ok), "{ok}");
        }
        for bad in ["", "1abc", "Handlr", "a b", "a:b", "-a", "ä"] {
            assert!(!valid_scheme(bad), "{bad}");
        }
    }

    #[test]
    #[should_panic(expected = "invalid URL scheme")]
    fn url_scheme_panics_on_invalid() {
        let _ = Project::new("dev.example.app").url_scheme("Bad");
    }

    #[test]
    fn url_schemes_go_inside_the_activity() {
        let spec = GradleProject::new("dev.example.app", "example_app");
        let files = spec.render(Path::new("/l.gradle"), Path::new("/l.so"), "arm64-v8a");
        let manifest = &files.iter().find(|(n, _)| *n == MANIFEST).unwrap().1;
        assert_eq!(with_url_schemes(manifest, &[]), *manifest);
        let with = with_url_schemes(manifest, &["handlr".into()]);
        let filter = with.find("android:scheme=\"handlr\"").unwrap();
        let launcher = with.find("android.intent.category.LAUNCHER").unwrap();
        let end = with.find("</activity>").unwrap();
        assert!(launcher < filter && filter < end);
        assert_eq!(with.matches("</activity>").count(), 1);
    }

    fn rendered_manifest() -> String {
        let spec = GradleProject::new("dev.example.app", "example_app");
        let files = spec.render(Path::new("/l.gradle"), Path::new("/l.so"), "arm64-v8a");
        files.into_iter().find(|(n, _)| *n == MANIFEST).unwrap().1
    }

    #[test]
    fn manifest_without_icon_is_unchanged() {
        let manifest = rendered_manifest();
        let plain = Project::new("dev.example.app");
        assert_eq!(plain.finish_manifest(&manifest, None), manifest);
        assert!(!manifest.contains("android:icon") && !manifest.contains("roundIcon"));
        let service = codegen::foreground_service_manifest(&[ServiceType::DataSync], false);
        let full = Project::new("dev.example.app").url_scheme("handlr");
        assert_eq!(
            full.finish_manifest(&manifest, Some(&service)),
            with_service(
                &with_url_schemes(&manifest, &["handlr".into()]),
                &service.application
            )
        );
    }

    #[cfg(feature = "icons")]
    mod icons {
        use super::*;
        use crate::icon::tests::square;

        struct TempDir(PathBuf);
        impl TempDir {
            fn new(name: &str) -> Self {
                let dir = env::temp_dir()
                    .join(format!("libmobile-icon-test-{name}-{}", std::process::id()));
                fs::create_dir_all(&dir).unwrap();
                TempDir(dir)
            }
            fn png(&self, name: &str, image: &image::RgbaImage) -> PathBuf {
                let path = self.0.join(name);
                image
                    .save_with_format(&path, image::ImageFormat::Png)
                    .unwrap();
                path
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn icon_adds_the_application_attributes() {
            let manifest = rendered_manifest();
            let with = Project::new("dev.example.app")
                .icon(Icon::png("/icon.png"))
                .finish_manifest(&manifest, None);
            let application = with.find("<application").unwrap();
            let icon = with.find("android:icon=\"@mipmap/ic_launcher\"").unwrap();
            let round = with
                .find("android:roundIcon=\"@mipmap/ic_launcher_round\"")
                .unwrap();
            let end = with[application..].find('>').unwrap() + application;
            assert!(application < icon && icon < round && round < end);
            assert_eq!(with.matches("android:icon=").count(), 1);
            assert_eq!(with.replace(codegen::ICON_ATTRIBUTES, ""), manifest);
        }

        #[test]
        fn icon_renders_every_file() {
            let dir = TempDir::new("render");
            let master = dir.png("icon.png", &square(1024, 100, 924, [255, 0, 0, 255]));
            let mono = dir.png("mono.png", &square(1024, 100, 924, [255, 255, 255, 255]));
            let icon = Icon::png(&master)
                .background_color([1, 2, 3])
                .monochrome_png(&mono);
            assert_eq!(icon.paths(), [master.as_path(), mono.as_path()]);
            let (files, warnings) = icon.render().unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(files.len(), 5 * 4 + 3);
            let names: Vec<_> = files.iter().map(|(n, _)| n.clone()).collect();
            let known = codegen::icon_resource_paths();
            assert!(names.iter().all(|n| known.contains(n)), "{names:?}");
            assert!(names.contains(&"app/src/main/res/values/ic_launcher_background.xml".into()));
            assert!(
                !names
                    .iter()
                    .any(|n| n.ends_with("ic_launcher_background.png"))
            );
        }

        #[test]
        fn background_png_replaces_the_colour() {
            let dir = TempDir::new("background");
            let master = dir.png("icon.png", &square(512, 0, 512, [255, 0, 0, 255]));
            let back = dir.png("back.png", &square(300, 0, 300, [0, 0, 255, 255]));
            let (files, warnings) = Icon::png(&master).background_png(&back).render().unwrap();
            assert_eq!(files.len(), 5 * 4 + 2);
            assert!(
                files
                    .iter()
                    .any(|(n, _)| n.ends_with("mipmap-mdpi/ic_launcher_background.png"))
            );
            assert!(
                !files
                    .iter()
                    .any(|(n, _)| n.ends_with(".xml") && n.contains("values"))
            );
            assert_eq!(warnings.len(), 1, "the 300 px background: {warnings:?}");
            assert!(warnings[0].contains("back.png"));
        }

        #[test]
        fn small_master_is_scaled_up_with_a_warning() {
            let dir = TempDir::new("small");
            let master = dir.png("small.png", &square(256, 0, 256, [0, 255, 0, 255]));
            let (files, warnings) = Icon::png(&master).render().unwrap();
            assert_eq!(files.len(), 5 * 3 + 3);
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("small.png") && warnings[0].contains("256 px"));
        }

        #[test]
        fn non_square_source_fails_naming_the_file() {
            let dir = TempDir::new("square");
            let wide = dir.png("wide.png", &image::RgbaImage::new(600, 500));
            let err = Icon::png(&wide).render().unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            let message = err.to_string();
            assert!(
                message.contains("wide.png") && message.contains("not square"),
                "{message}"
            );
            let good = dir.png("good.png", &square(512, 0, 512, [1, 1, 1, 255]));
            let err = Icon::png(&good).monochrome_png(&wide).render().unwrap_err();
            assert!(err.to_string().contains("wide.png"));
        }

        #[test]
        fn non_png_source_fails_naming_the_file() {
            let dir = TempDir::new("format");
            let jpeg = dir.0.join("icon.jpg");
            fs::write(&jpeg, b"\xff\xd8\xff\xe0 not a png").unwrap();
            let err = Icon::png(&jpeg).render().unwrap_err();
            assert!(err.to_string().contains("icon.jpg") && err.to_string().contains("not a PNG"));
            let broken = dir.0.join("broken.png");
            fs::write(&broken, b"\x89PNG\r\n\x1a\ntruncated").unwrap();
            let err = Icon::png(&broken).render().unwrap_err();
            assert!(err.to_string().contains("broken.png"), "{err}");
            let missing = dir.0.join("missing.png");
            let err = Icon::png(&missing).render().unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
            assert!(err.to_string().contains("missing.png"));
        }

        #[test]
        fn declared_sizes_are_checked_and_used() {
            let dir = TempDir::new("sizes");
            let big = dir.png("big.png", &square(1024, 0, 1024, [9, 9, 9, 255]));
            let small = dir.png("small.png", &square(66, 0, 66, [7, 7, 7, 255]));
            let (files, warnings) = Icon::pngs(&[(1024, &big), (66, &small)]).render().unwrap();
            assert!(warnings.is_empty());
            let fg = &files
                .iter()
                .find(|(n, _)| n.ends_with("mipmap-mdpi/ic_launcher_foreground.png"))
                .unwrap()
                .1;
            let fg = image::load_from_memory(fg).unwrap().to_rgba8();
            assert_eq!(
                fg.get_pixel(54, 54).0,
                [7, 7, 7, 255],
                "mdpi artwork is 66 px"
            );
            let err = Icon::pngs(&[(512, &big)]).render().unwrap_err();
            assert!(err.to_string().contains("declared as 512 px"), "{err}");
        }

        #[test]
        #[should_panic(expected = "at least one PNG")]
        fn pngs_needs_a_png() {
            let _ = Icon::pngs(&[]);
        }
    }

    #[test]
    fn stale_icon_files_are_removed() {
        let dir = env::temp_dir().join(format!("libmobile-stale-test-{}", std::process::id()));
        let file = dir.join("res/a.png");
        write_if_changed(&file, b"png").unwrap();
        remove_if_present(&file).unwrap();
        assert!(!file.exists());
        remove_if_present(&file).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn service_needs_its_feature() {
        assert!(check_service_feature(Some("camera,foreground-service")).is_ok());
        for features in [None, Some(""), Some("camera"), Some("foreground-services")] {
            let err = check_service_feature(features).unwrap_err().to_string();
            assert!(err.contains("`foreground-service`"), "{err}");
        }
    }

    #[test]
    fn service_goes_inside_the_application() {
        let spec = GradleProject::new("dev.example.app", "example_app");
        let files = spec.render(Path::new("/l.gradle"), Path::new("/l.so"), "arm64-v8a");
        let manifest = &files.iter().find(|(n, _)| *n == MANIFEST).unwrap().1;
        let service = codegen::foreground_service_manifest(&[ServiceType::DataSync], false);
        let with = with_service(manifest, &service.application);
        let at = with.find("<service").unwrap();
        assert!(with.find("</activity>").unwrap() < at);
        assert!(at < with.find("</application>").unwrap());
    }

    #[test]
    fn builder_records_the_service() {
        let project = Project::new("a.b")
            .foreground_service(&[ServiceType::Microphone])
            .foreground_service_keeps_running()
            .request_ignore_battery_optimizations()
            .request_ignore_battery_optimizations();
        assert_eq!(project.service_types, Some(vec![ServiceType::Microphone]));
        assert!(project.service_keeps_running);
        assert_eq!(project.spec.permissions, [IGNORE_BATTERY_OPTIMIZATIONS]);
    }

    #[test]
    #[should_panic(expected = "at least one type")]
    fn foreground_service_needs_a_type() {
        let _ = Project::new("a.b").foreground_service(&[]);
    }

    #[test]
    fn prefixes_name_the_links_key() {
        for prefix in [
            "libmorph",
            "LIBMORPH",
            "DEP_LIBMORPH",
            "DEP_LIBMORPH_",
            " libmorph ",
        ] {
            assert_eq!(
                metadata_var(prefix, "GRADLE_SCRIPT"),
                "DEP_LIBMORPH_GRADLE_SCRIPT"
            );
        }
        assert_eq!(
            metadata_var("my-shell", "FEATURES"),
            "DEP_MY_SHELL_FEATURES"
        );
        assert_eq!(
            Project::new("a.b").env_prefix,
            DEFAULT_ENV_PREFIX,
            "direct dependents read libmobile's own key"
        );
    }

    #[test]
    fn forward_republishes_both_keys() {
        let lines = forward_lines(|var| match var {
            "DEP_LIBMOBILE_GRADLE_SCRIPT" => Some("/out/libmobile.gradle".into()),
            "DEP_LIBMOBILE_FEATURES" => Some("camera".into()),
            _ => None,
        });
        assert_eq!(
            lines,
            [
                "cargo:gradle_script=/out/libmobile.gradle",
                "cargo:features=camera"
            ]
        );
    }

    #[test]
    fn forward_without_libmobile_publishes_nothing() {
        assert!(forward_lines(|_| None).is_empty());
        // Features alone (no script) means no usable handoff either.
        assert!(
            forward_lines(|var| (var == "DEP_LIBMOBILE_FEATURES").then(String::new)).is_empty()
        );
    }

    #[test]
    fn profile_dir_is_three_up() {
        let out = Path::new("/t/aarch64-linux-android/release/build/app-1234/out");
        assert_eq!(
            profile_dir(out).unwrap(),
            Path::new("/t/aarch64-linux-android/release")
        );
    }

    #[test]
    fn local_properties_escapes_windows_paths() {
        assert!(local_properties("/opt/sdk").ends_with("sdk.dir=/opt/sdk\n"));
        assert!(local_properties(r"C:\sdk").contains(r"sdk.dir=C\:\\sdk"));
    }

    #[test]
    fn write_if_changed_keeps_identical_files() {
        let dir = env::temp_dir().join(format!("libmobile-gradle-test-{}", std::process::id()));
        let file = dir.join("a/b.txt");
        write_if_changed(&file, "x").unwrap();
        let first = fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_if_changed(&file, "x").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), first);
        write_if_changed(&file, "y").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "y");
        fs::remove_dir_all(dir).unwrap();
    }
}
