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
    pub fn generate(self) -> io::Result<Option<PathBuf>> {
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
        let profile_dir = profile_dir(&PathBuf::from(env_var("OUT_DIR")?))?;
        let native_lib = profile_dir.join(format!("lib{}.so", self.spec.lib_name));

        let dir = match (env::var_os("LIBMOBILE_GRADLE_PROJECT_DIR"), self.out_dir) {
            (Some(dir), _) if !dir.is_empty() => PathBuf::from(dir),
            (_, Some(dir)) => dir,
            _ => profile_dir
                .join("gradle")
                .join(env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "app".into())),
        };
        for (name, contents) in self.spec.render(&libmobile_gradle, &native_lib, abi) {
            write_if_changed(&dir.join(name), &contents)?;
        }
        // Remember the SDK cargo was run with, so a bare `gradle -p <dir>`
        // works without ANDROID_HOME in that shell.
        if let Some(sdk) = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .find_map(|var| env::var(var).ok().filter(|v| !v.is_empty()))
        {
            write_if_changed(&dir.join("local.properties"), &local_properties(&sdk))?;
        }
        Ok(Some(dir))
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
fn write_if_changed(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

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
