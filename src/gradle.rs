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

use crate::codegen::{self, GradleProject};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Builder over [`GradleProject`] that fills the rest from the cargo build.
pub struct Project {
    spec: GradleProject,
    out_dir: Option<PathBuf>,
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
        }
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
        let libmobile_gradle = env::var_os("DEP_LIBMOBILE_GRADLE_SCRIPT")
            .map(PathBuf::from)
            .ok_or_else(|| {
                io::Error::other(
                    "DEP_LIBMOBILE_GRADLE_SCRIPT is unset: libmobile must also be a normal \
                     [dependencies] entry of the crate whose build.rs calls this",
                )
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
