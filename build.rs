//! Writes the Gradle handoff for the active Cargo features.
//!
//! * `$OUT_DIR/java/dev/highbay/libmobile/` — the package's top-level Kotlin
//!   files, the subdirectories the active plugins live in, and
//!   `GeneratedPlugins.kt`. Staged (copied) rather than referenced in place: a
//!   Gradle source root is a package-tree root, so `android/java` could only
//!   be added whole, and a disabled feature's sources would compile against
//!   AARs the app does not have. `OUT_DIR` is per feature set, so each set
//!   keeps its own tree.
//! * `libmobile.gradle` — appends that root to `android.sourceSets.main`, adds
//!   the plugins' AARs, and splices their manifest contributions into the
//!   merged manifest. Written to `OUT_DIR` and to the stable handoff
//!   directory the app module applies it from: `$LIBMOBILE_GRADLE_DIR` if set,
//!   else `<target>/<triple>/<profile>/libmobile-gradle/`, beside the `.so`.
//!
//! The last build in a profile directory wins the stable script. Cargo caches
//! build-script runs per feature set, so the stable script is also a
//! `rerun-if-changed` input: when a build with other features rewrites it,
//! switching back reruns this script and points it home again. It is only
//! written when its content changes, so an unchanged build settles after one
//! extra run instead of rerunning forever.
//!
//! The rendering lives in `src/codegen.rs` (unit-tested there); this file is
//! only the I/O around it.

#[path = "src/codegen.rs"]
#[allow(dead_code)]
mod codegen;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/codegen.rs");
    println!("cargo:rerun-if-changed={}", codegen::KOTLIN_ROOT);
    println!("cargo:rerun-if-env-changed=LIBMOBILE_GRADLE_DIR");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    let enabled: Vec<&str> = codegen::known_features()
        .into_iter()
        .filter(|f| feature_enabled(f))
        .collect();
    let active = codegen::active_plugins(&enabled);

    let handoff = handoff_dir(&out_dir);
    let stable_script = handoff.join("libmobile.gradle");
    println!("cargo:rerun-if-changed={}", stable_script.display());
    let java_root = out_dir.join("java");
    let package_src = manifest_dir
        .join(codegen::KOTLIN_ROOT)
        .join(codegen::PACKAGE_DIR);
    let package_dst = java_root.join(codegen::PACKAGE_DIR);

    if let Err(err) = stage(&package_src, &package_dst, &active, &java_root) {
        panic!(
            "libmobile: staging Kotlin sources into {} failed: {err}",
            java_root.display()
        );
    }
    write(
        &package_dst.join("GeneratedPlugins.kt"),
        &codegen::generated_plugins_kt(&enabled, &active),
    );
    let script = codegen::gradle_script(&enabled, &java_root, &codegen::merge(&active));
    write(&out_dir.join("libmobile.gradle"), &script);
    if fs::read_to_string(&stable_script).ok().as_deref() != Some(script.as_str()) {
        write(&stable_script, &script);
    }

    println!("cargo:rustc-env=LIBMOBILE_GRADLE_DIR={}", handoff.display());
}

fn feature_enabled(feature: &str) -> bool {
    let var = format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"));
    env::var_os(var).is_some()
}

/// `$LIBMOBILE_GRADLE_DIR`, else the profile directory three levels above
/// `OUT_DIR` (`<profile>/build/libmobile-<hash>/out`).
fn handoff_dir(out_dir: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("LIBMOBILE_GRADLE_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    out_dir
        .ancestors()
        .nth(3)
        .map(|profile| profile.join("libmobile-gradle"))
        .unwrap_or_else(|| out_dir.join("libmobile-gradle"))
}

fn stage(src: &Path, dst: &Path, active: &[codegen::Plugin], java_root: &Path) -> io::Result<()> {
    if java_root.exists() {
        fs::remove_dir_all(java_root)?;
    }
    fs::create_dir_all(dst)?;
    // Top-level package files: the activity, the plugin interface, NativeLib.
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    for dir in codegen::source_dirs(active) {
        copy_tree(&src.join(dir), &dst.join(dir))?;
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("libmobile: mkdir {}: {e}", parent.display()));
    }
    fs::write(path, contents)
        .unwrap_or_else(|e| panic!("libmobile: write {}: {e}", path.display()));
}
