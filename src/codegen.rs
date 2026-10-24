//! **The compile-time plugin registry and everything rendered from it.**
//!
//! There is no runtime plugin discovery (no `ServiceLoader`, no reflection):
//! which Kotlin plugins the APK carries is decided by the Cargo features this
//! crate was built with, and `build.rs` writes that decision down as source
//! before Gradle ever runs.
//!
//! Each [`Plugin`] declares, next to its Kotlin class, what it
//! [`Contributes`] to the app: AARs, `uses-permission`s, other manifest
//! elements. The active plugins' contributions are merged and rendered into:
//!
//! * `GeneratedPlugins.kt` — `GeneratedPlugins.activePlugins`, the list
//!   `MobileNativeActivity` dispatches every lifecycle hook to.
//! * `libmobile.gradle` — a Groovy script the app module `apply`s. It appends
//!   the staged Kotlin root to `android.sourceSets.main`, adds the AARs, and
//!   splices the manifest contributions into the app's *merged* manifest
//!   (AGP's `MERGED_MANIFEST` transform), so they reach a hand-written
//!   project exactly as they reach a generated one.
//!
//! [`GradleProject`] renders a complete Gradle project around that script for
//! apps that do not keep their own (`crate::gradle`).
//!
//! This module is std-only on purpose: `build.rs` compiles it directly
//! (`#[path = "src/codegen.rs"]`), and the crate compiles it again so the
//! rendering is unit-tested like any other code. Nothing here touches the
//! filesystem.

use std::path::{Path, PathBuf};

/// The root Android package; every Kotlin source lives under it.
pub const PACKAGE: &str = "dev.highbay.libmobile";

/// The Kotlin source tree, relative to the crate root.
pub const KOTLIN_ROOT: &str = "android/java";

/// The package's directory under [`KOTLIN_ROOT`].
pub const PACKAGE_DIR: &str = "dev/highbay/libmobile";

/// The activity a generated manifest launches when none is named.
pub const DEFAULT_ACTIVITY: &str = "dev.highbay.libmobile.MobileNativeActivity";

/// Android Gradle Plugin version the generated project pins.
pub const AGP_VERSION: &str = "9.3.1";

/// What a plugin adds to the app beyond its Kotlin sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contributes {
    /// Maven coordinates for the app's `implementation` dependencies.
    pub dependencies: &'static [&'static str],
    /// `<uses-permission android:name=...>` names.
    pub permissions: &'static [&'static str],
    /// Raw XML elements for directly inside `<manifest>` (`<queries>`,
    /// `<uses-feature>`...).
    pub manifest: &'static [&'static str],
    /// Raw XML elements for directly inside `<application>` (`<provider>`,
    /// `<meta-data>`...).
    pub application: &'static [&'static str],
}

impl Contributes {
    pub const NOTHING: Contributes = Contributes {
        dependencies: &[],
        permissions: &[],
        manifest: &[],
        application: &[],
    };
}

/// One Kotlin `MobileActivityPlugin` object, the feature that gates it, and
/// what it contributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plugin {
    /// `None` for the core bridges every APK carries.
    pub feature: Option<&'static str>,
    /// Subdirectory of [`PACKAGE_DIR`] holding the plugin's sources; the whole
    /// directory is staged when the plugin is active.
    pub dir: &'static str,
    /// Class name relative to [`PACKAGE`].
    pub class: &'static str,
    pub contributes: Contributes,
}

impl Plugin {
    /// Fully-qualified Kotlin name.
    pub fn qualified(&self) -> String {
        format!("{PACKAGE}.{}", self.class)
    }
}

/// Every plugin the crate ships, in dispatch order. Core bridges come first so
/// a feature plugin's `onCreate` can rely on insets/intents already being wired.
pub const PLUGINS: &[Plugin] = &[
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.WindowInsetsPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.IntentPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.TextInputPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.PermissionPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.LinkPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.DirsPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.BuildInfoPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.TrimMemoryPlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: None,
        dir: "store",
        class: "store.SecureStorePlugin",
        contributes: Contributes::NOTHING,
    },
    Plugin {
        feature: Some("google-auth"),
        dir: "auth",
        class: "auth.GoogleAuthPlugin",
        contributes: Contributes {
            dependencies: &[
                "androidx.credentials:credentials:1.5.0",
                "androidx.credentials:credentials-play-services-auth:1.5.0",
                "com.google.android.libraries.identity.googleid:googleid:1.2.0",
            ],
            ..Contributes::NOTHING
        },
    },
    Plugin {
        feature: Some("camera"),
        dir: "camera",
        class: "camera.CameraPlugin",
        contributes: Contributes {
            manifest: &[
                // Package visibility (API 30+): lets the plugin see that a
                // camera app exists to hand off to.
                "<queries><intent><action android:name=\"android.media.action.IMAGE_CAPTURE\" /></intent></queries>",
                // Optional: the plugin degrades to `no_camera_app`, so Play
                // must not filter camera-less devices out.
                "<uses-feature android:name=\"android.hardware.camera.any\" android:required=\"false\" />",
            ],
            ..Contributes::NOTHING
        },
    },
    // The service and its notification
    // (`service/`, `src/android/service.rs`). The `<service>` element and
    // its permissions come from `gradle::Project::foreground_service`, which
    // knows the types ([`foreground_service_manifest`]).
    Plugin {
        feature: Some("foreground-service"),
        dir: "service",
        class: "service.ForegroundServicePlugin",
        contributes: Contributes::NOTHING,
    },
    // Battery optimization and app sleeping (`battery/`,
    // `src/android/battery.rs`). The permission for the direct dialog comes
    // from `gradle::Project::request_ignore_battery_optimizations`. Samsung's
    // Device Care is listed in `<queries>`, or Android 11+ hides its pages
    // from `resolveActivity`.
    Plugin {
        feature: Some("battery"),
        dir: "battery",
        class: "battery.BatteryPlugin",
        contributes: Contributes {
            manifest: &["<queries><package android:name=\"com.samsung.android.lool\" /></queries>"],
            ..Contributes::NOTHING
        },
    },
    // Visibility, network and Doze callbacks on the Application
    // (`events/`, `src/android/system_events.rs`).
    Plugin {
        feature: Some("system-events"),
        dir: "events",
        class: "events.SystemEventsPlugin",
        contributes: Contributes {
            permissions: &["android.permission.ACCESS_NETWORK_STATE"],
            ..Contributes::NOTHING
        },
    },
];

/// A Cargo feature that only sets a `Boolean` constant in
/// `GeneratedPlugins.kt`, for core plugins that behave differently under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flag {
    pub feature: &'static str,
    /// The constant's name.
    pub constant: &'static str,
    /// Its value when the feature is on; the opposite when it is off.
    pub when_enabled: bool,
}

/// Every flag the crate ships.
pub const FLAGS: &[Flag] = &[
    // StrongBox is tried unless the app opts out (`store/SecureStorePlugin.kt`).
    Flag {
        feature: "secure-store-no-strongbox",
        constant: "SECURE_STORE_STRONGBOX",
        when_enabled: false,
    },
];

/// Every feature name the registry knows (plugin features, then flags),
/// deduplicated, in declaration order.
pub fn known_features() -> Vec<&'static str> {
    let mut out = features_in(PLUGINS);
    for flag in FLAGS {
        if !out.contains(&flag.feature) {
            out.push(flag.feature);
        }
    }
    out
}

/// The plugins `enabled` selects, in [`PLUGINS`] order.
pub fn active_plugins(enabled: &[&str]) -> Vec<Plugin> {
    active_in(PLUGINS, enabled)
}

fn features_in(table: &[Plugin]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for feature in table.iter().filter_map(|p| p.feature) {
        if !out.contains(&feature) {
            out.push(feature);
        }
    }
    out
}

fn active_in(table: &[Plugin], enabled: &[&str]) -> Vec<Plugin> {
    table
        .iter()
        .filter(|p| p.feature.is_none_or(|f| enabled.contains(&f)))
        .copied()
        .collect()
}

/// The subdirectories of [`PACKAGE_DIR`] to stage, deduplicated, in order.
/// (The package's top-level files — the activity and the plugin interface —
/// are always staged and are not listed here.)
pub fn source_dirs(active: &[Plugin]) -> Vec<&'static str> {
    let mut dirs: Vec<&'static str> = Vec::new();
    for plugin in active {
        if !dirs.contains(&plugin.dir) {
            dirs.push(plugin.dir);
        }
    }
    dirs
}

/// The active plugins' contributions, each list deduplicated in plugin order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Merged {
    pub dependencies: Vec<&'static str>,
    pub permissions: Vec<&'static str>,
    pub manifest: Vec<&'static str>,
    pub application: Vec<&'static str>,
}

pub fn merge(active: &[Plugin]) -> Merged {
    fn add(into: &mut Vec<&'static str>, items: &[&'static str]) {
        for item in items {
            if !into.contains(item) {
                into.push(item);
            }
        }
    }
    let mut merged = Merged::default();
    for plugin in active {
        let c = &plugin.contributes;
        add(&mut merged.dependencies, c.dependencies);
        add(&mut merged.permissions, c.permissions);
        add(&mut merged.manifest, c.manifest);
        add(&mut merged.application, c.application);
    }
    merged
}

fn feature_line(enabled: &[&str]) -> String {
    if enabled.is_empty() {
        "<none>".to_string()
    } else {
        enabled.join(", ")
    }
}

/// Render `GeneratedPlugins.kt`.
pub fn generated_plugins_kt(enabled: &[&str], active: &[Plugin]) -> String {
    let mut out = String::new();
    out.push_str(
        "// GENERATED by libmobile's build.rs from the active Cargo features. Do not edit.\n",
    );
    out.push_str(&format!("// features: {}\n", feature_line(enabled)));
    out.push_str(&format!("package {PACKAGE}\n\n"));
    out.push_str("object GeneratedPlugins {\n");
    out.push_str("    val activePlugins: List<MobileActivityPlugin> = listOf(\n");
    for plugin in active {
        out.push_str(&format!("        {},\n", plugin.qualified()));
    }
    out.push_str("    )\n");
    for flag in FLAGS {
        let value = enabled.contains(&flag.feature) == flag.when_enabled;
        out.push_str(&format!(
            "\n    const val {}: Boolean = {value}\n",
            flag.constant
        ));
    }
    out.push_str("}\n");
    out
}

/// A Groovy single-quoted string literal.
fn groovy_str(raw: &str) -> String {
    format!(
        "'{}'",
        raw.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}

fn groovy_path(path: &Path) -> String {
    groovy_str(&path.to_string_lossy().replace('\\', "/"))
}

fn groovy_list(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

/// The Groovy half of the manifest contribution: a `MERGED_MANIFEST`
/// transform that splices XML into the app's merged manifest. A script
/// applied with `apply(from = ...)` cannot see AGP's classes by name, so the
/// artifact type is looked up through the variant's own class loader.
const MANIFEST_TRANSFORM: &str = r#"
abstract class LibmobileManifestTask extends DefaultTask {
    @InputFile abstract RegularFileProperty getMergedManifest()
    @OutputFile abstract RegularFileProperty getUpdatedManifest()
    @Input abstract ListProperty<String> getPermissions()
    @Input abstract ListProperty<String> getManifestXml()
    @Input abstract ListProperty<String> getApplicationXml()

    @TaskAction
    void splice() {
        def text = mergedManifest.get().asFile.getText('UTF-8')
        // Permissions the app (or a library) already declares are skipped:
        // a duplicate would be a second element, not a no-op.
        def root = permissions.get()
            .findAll { !(text =~ /<uses-permission[^>]*android:name="${java.util.regex.Pattern.quote(it)}"/) }
            .collect { "    <uses-permission android:name=\"${it}\" />\n" }
            .join('') + manifestXml.get().collect { "    ${it}\n" }.join('')
        if (root) {
            def at = text.indexOf('<application')
            if (at < 0) throw new GradleException('libmobile: merged manifest has no <application>')
            text = text.substring(0, at) + root.trim() + '\n\n    ' + text.substring(at)
        }
        def app = applicationXml.get().collect { "        ${it}\n" }.join('')
        if (app) {
            def at = text.lastIndexOf('</application>')
            if (at < 0) throw new GradleException('libmobile: merged manifest has no </application>')
            text = text.substring(0, at) + app.trim() + '\n    ' + text.substring(at)
        }
        updatedManifest.get().asFile.write(text, 'UTF-8')
    }
}

androidComponents {
    onVariants(selector().all()) { variant ->
        def task = project.tasks.register("libmobile${variant.name.capitalize()}Manifest", LibmobileManifestTask) {
            permissions.set(LIBMOBILE_PERMISSIONS)
            manifestXml.set(LIBMOBILE_MANIFEST_XML)
            applicationXml.set(LIBMOBILE_APPLICATION_XML)
        }
        def mergedManifest = variant.artifacts.getClass().classLoader
            .loadClass('com.android.build.api.artifact.SingleArtifact$MERGED_MANIFEST').INSTANCE
        variant.artifacts.use(task)
            .wiredWithFiles({ it.mergedManifest }, { it.updatedManifest })
            .toTransform(mergedManifest)
    }
}
"#;

/// Render `libmobile.gradle`, which the app module `apply`s.
///
/// Groovy rather than Kotlin script on purpose: an `apply(from = ...)`'d `.kts`
/// script gets no type-safe accessors for the `android {}` extension, while
/// Groovy resolves it dynamically — so the generated file needs no knowledge
/// of the AGP version it lands in.
pub fn gradle_script(enabled: &[&str], kotlin_src: &Path, merged: &Merged) -> String {
    let quoted = |items: &[&str]| -> String {
        groovy_list(&items.iter().map(|s| groovy_str(s)).collect::<Vec<_>>())
    };
    let mut out = String::new();
    out.push_str(
        "// GENERATED by libmobile's build.rs from the active Cargo features. Do not edit.\n",
    );
    out.push_str(&format!("// features: {}\n", feature_line(enabled)));
    out.push_str("//\n// Apply from the app module, after the Android plugin:\n");
    out.push_str("//   apply(from = \"<this file>\")\n\n");

    // Both: AGP 9's built-in Kotlin compiles only `kotlin` source dirs, and
    // `java` keeps the root visible to tooling that reads only that set.
    let root = groovy_path(kotlin_src);
    out.push_str("android {\n    sourceSets {\n        main {\n");
    out.push_str(&format!("            java.srcDir {root}\n"));
    out.push_str(&format!("            kotlin.srcDir {root}\n"));
    out.push_str("        }\n    }\n}\n");

    if !merged.dependencies.is_empty() {
        out.push_str("\ndependencies {\n");
        for coordinate in &merged.dependencies {
            out.push_str(&format!("    implementation {}\n", groovy_str(coordinate)));
        }
        out.push_str("}\n");
    }

    if !(merged.permissions.is_empty()
        && merged.manifest.is_empty()
        && merged.application.is_empty())
    {
        out.push_str("\n// Manifest contributions of the active plugins.\n");
        out.push_str(&format!(
            "ext.LIBMOBILE_PERMISSIONS = {}\n",
            quoted(&merged.permissions)
        ));
        out.push_str(&format!(
            "ext.LIBMOBILE_MANIFEST_XML = {}\n",
            quoted(&merged.manifest)
        ));
        out.push_str(&format!(
            "ext.LIBMOBILE_APPLICATION_XML = {}\n",
            quoted(&merged.application)
        ));
        out.push_str(MANIFEST_TRANSFORM);
    }
    out
}

/// Everything a generated Gradle project needs to know about the app. The
/// file I/O around it is `crate::gradle`; this is the pure description.
///
/// Plugin contributions are *not* part of it: they arrive through
/// `libmobile.gradle`, which the project applies, so a generated and a
/// hand-written project get them the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradleProject {
    /// `applicationId` and the Gradle `namespace`.
    pub application_id: String,
    /// The cdylib's name without `lib`/`.so` — `android.app.lib_name`.
    pub lib_name: String,
    /// Launcher label.
    pub label: String,
    /// The activity class, fully qualified.
    pub activity: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub compile_sdk: u32,
    pub version_code: u32,
    pub version_name: String,
    /// The app's own `uses-permission` names.
    pub permissions: Vec<String>,
    /// Gradle scripts the app module applies after everything generated —
    /// the hook for signing configs, extra dependencies, flavors.
    pub extra_scripts: Vec<PathBuf>,
}

impl GradleProject {
    pub fn new(application_id: &str, lib_name: &str) -> Self {
        GradleProject {
            application_id: application_id.to_string(),
            lib_name: lib_name.to_string(),
            label: lib_name.to_string(),
            activity: DEFAULT_ACTIVITY.to_string(),
            min_sdk: 29,
            target_sdk: 35,
            compile_sdk: 35,
            version_code: 1,
            version_name: "0.1.0".to_string(),
            permissions: Vec::new(),
            extra_scripts: Vec::new(),
        }
    }

    /// Render every file of the project, as `(relative path, contents)`.
    ///
    /// * `libmobile_gradle` — the feature-specific `libmobile.gradle`.
    /// * `native_lib` — the built `lib<name>.so`; the app module copies it into
    ///   its jniLibs as `abi` before every build, so Gradle always packages
    ///   what cargo just produced.
    pub fn render(
        &self,
        libmobile_gradle: &Path,
        native_lib: &Path,
        abi: &str,
    ) -> Vec<(&'static str, String)> {
        const HEADER: &str = "// GENERATED by libmobile::gradle from the app's build.rs. Do not edit:\n// every cargo build rewrites it. Customise with `Project::extra_script`.\n";
        let settings = format!(
            "{HEADER}
pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}

dependencyResolutionManagement {{
    repositories {{
        google()
        mavenCentral()
    }}
}}

rootProject.name = {name}
include(\":app\")
",
            name = kotlin_str(&self.lib_name)
        );
        let root = format!(
            "{HEADER}
plugins {{
    id(\"com.android.application\") version \"{AGP_VERSION}\" apply false
}}
"
        );
        let properties = "# GENERATED by libmobile::gradle. Do not edit.
android.useAndroidX=true
org.gradle.jvmargs=-Xmx3g -Dfile.encoding=UTF-8
"
        .to_string();

        let mut app = format!(
            "{HEADER}
plugins {{
    id(\"com.android.application\")
}}

android {{
    namespace = {id}
    compileSdk = {compile}

    defaultConfig {{
        applicationId = {id}
        minSdk = {min}
        targetSdk = {target}
        versionCode = {code}
        versionName = {name}
        ndk {{ abiFilters += {abi} }}
    }}

    // Filled by `copyRustLib` below, never by hand.
    sourceSets[\"main\"].jniLibs.srcDir(layout.buildDirectory.dir(\"rustJniLibs\").get().asFile)

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}
}}

// cargo builds the .so; Gradle only packages it.
val copyRustLib by tasks.registering(Copy::class) {{
    from({lib})
    into(layout.buildDirectory.dir({abi_dir}))
}}
tasks.named(\"preBuild\") {{ dependsOn(copyRustLib) }}

// libmobile's Kotlin sources, AARs and manifest entries for its active features.
apply(from = {libmobile})
",
            id = kotlin_str(&self.application_id),
            compile = self.compile_sdk,
            min = self.min_sdk,
            target = self.target_sdk,
            code = self.version_code,
            name = kotlin_str(&self.version_name),
            abi = kotlin_str(abi),
            lib = kotlin_str(&native_lib.to_string_lossy()),
            abi_dir = kotlin_str(&format!("rustJniLibs/{abi}")),
            libmobile = kotlin_str(&libmobile_gradle.to_string_lossy()),
        );
        for script in &self.extra_scripts {
            app.push_str(&format!(
                "apply(from = {})\n",
                kotlin_str(&script.to_string_lossy())
            ));
        }

        let mut manifest = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>
<!-- GENERATED by libmobile::gradle. Do not edit. -->
<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">
",
        );
        for permission in &self.permissions {
            manifest.push_str(&format!(
                "    <uses-permission android:name=\"{}\" />\n",
                xml_attr(permission)
            ));
        }
        // singleTop: a later intent reaches the running instance through
        // onNewIntent. configChanges: Rust handles rotation/density/night mode
        // as events instead of losing the activity (and its surface) to them.
        manifest.push_str(&format!(
            "
    <application
        android:allowBackup=\"false\"
        android:hasCode=\"true\"
        android:label=\"{label}\"
        android:theme=\"@android:style/Theme.DeviceDefault.NoActionBar\">

        <activity
            android:name=\"{activity}\"
            android:exported=\"true\"
            android:launchMode=\"singleTop\"
            android:configChanges=\"orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|density|uiMode\">

            <meta-data
                android:name=\"android.app.lib_name\"
                android:value=\"{lib}\" />

            <intent-filter>
                <action android:name=\"android.intent.action.MAIN\" />
                <category android:name=\"android.intent.category.LAUNCHER\" />
            </intent-filter>
        </activity>
    </application>
</manifest>
",
            label = xml_attr(&self.label),
            activity = xml_attr(&self.activity),
            lib = xml_attr(&self.lib_name),
        ));

        vec![
            ("settings.gradle.kts", settings),
            ("build.gradle.kts", root),
            ("gradle.properties", properties),
            ("app/build.gradle.kts", app),
            ("app/src/main/AndroidManifest.xml", manifest),
        ]
    }
}

/// One VIEW intent filter per scheme, for the activity element of the
/// manifest, so links with those schemes open the app. Indented to sit
/// inside `<activity>`; empty for no schemes.
pub fn url_scheme_filters(schemes: &[String]) -> String {
    schemes
        .iter()
        .map(|scheme| {
            format!(
                "
            <intent-filter>
                <action android:name=\"android.intent.action.VIEW\" />
                <category android:name=\"android.intent.category.DEFAULT\" />
                <category android:name=\"android.intent.category.BROWSABLE\" />
                <data android:scheme=\"{}\" />
            </intent-filter>
",
                xml_attr(scheme)
            )
        })
        .collect()
}

/// The generated foreground service's class, fully qualified.
pub const FOREGROUND_SERVICE_CLASS: &str = "dev.highbay.libmobile.service.MobileForegroundService";

/// A type the foreground service may be started with
/// (`android:foregroundServiceType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceType {
    /// `dataSync`: limited to 6 hours in 24 from Android 15.
    DataSync,
    /// `microphone`: capture while in the background. Needs `RECORD_AUDIO`
    /// granted when the service starts.
    Microphone,
    /// `specialUse`, with the stated reason, for Play review.
    SpecialUse(&'static str),
    /// `remoteMessaging`.
    RemoteMessaging,
}

impl ServiceType {
    /// The manifest's name for the type.
    pub fn manifest_name(self) -> &'static str {
        match self {
            ServiceType::DataSync => "dataSync",
            ServiceType::Microphone => "microphone",
            ServiceType::SpecialUse(_) => "specialUse",
            ServiceType::RemoteMessaging => "remoteMessaging",
        }
    }

    /// `FOREGROUND_SERVICE_<TYPE>`, the permission the type needs.
    pub fn permission(self) -> &'static str {
        match self {
            ServiceType::DataSync => "android.permission.FOREGROUND_SERVICE_DATA_SYNC",
            ServiceType::Microphone => "android.permission.FOREGROUND_SERVICE_MICROPHONE",
            ServiceType::SpecialUse(_) => "android.permission.FOREGROUND_SERVICE_SPECIAL_USE",
            ServiceType::RemoteMessaging => {
                "android.permission.FOREGROUND_SERVICE_REMOTE_MESSAGING"
            }
        }
    }

    /// `ServiceInfo.FOREGROUND_SERVICE_TYPE_*`.
    pub fn bits(self) -> i32 {
        match self {
            ServiceType::DataSync => 1,
            ServiceType::Microphone => 1 << 7,
            ServiceType::SpecialUse(_) => 1 << 30,
            ServiceType::RemoteMessaging => 1 << 9,
        }
    }
}

/// What a declared foreground service adds to the manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceManifest {
    /// `uses-permission` names, in order.
    pub permissions: Vec<&'static str>,
    /// The `<service>` element, indented to sit inside `<application>`.
    pub application: String,
}

/// The `<service>` element and permissions for a foreground service with
/// `types`. Each kind is declared once; of several `SpecialUse` reasons the
/// first is used. `keeps_running` sets `stopWithTask="false"`, so swiping
/// the app away from Recents leaves the service running.
pub fn foreground_service_manifest(types: &[ServiceType], keeps_running: bool) -> ServiceManifest {
    let mut kinds: Vec<ServiceType> = Vec::new();
    for ty in types {
        if !kinds.iter().any(|k| k.bits() == ty.bits()) {
            kinds.push(*ty);
        }
    }
    let mut permissions = vec!["android.permission.FOREGROUND_SERVICE"];
    permissions.extend(kinds.iter().map(|k| k.permission()));
    permissions.push("android.permission.POST_NOTIFICATIONS");
    permissions.push("android.permission.WAKE_LOCK");
    let names: Vec<&str> = kinds.iter().map(|k| k.manifest_name()).collect();
    let mut application = format!(
        "
        <service
            android:name=\"{FOREGROUND_SERVICE_CLASS}\"
            android:exported=\"false\"
            android:stopWithTask=\"{}\"
            android:foregroundServiceType=\"{}\"",
        !keeps_running,
        names.join("|"),
    );
    let reason = kinds.iter().find_map(|k| match k {
        ServiceType::SpecialUse(reason) => Some(*reason),
        _ => None,
    });
    match reason {
        Some(reason) => application.push_str(&format!(
            ">
            <property
                android:name=\"android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE\"
                android:value=\"{}\" />
        </service>
",
            xml_attr(reason)
        )),
        None => application.push_str(" />\n"),
    }
    ServiceManifest {
        permissions,
        application,
    }
}

/// The Android ABI directory for a Rust target triple.
pub fn android_abi(target: &str) -> Option<&'static str> {
    match target.split('-').next()? {
        "aarch64" => Some("arm64-v8a"),
        "armv7" | "thumbv7neon" => Some("armeabi-v7a"),
        "x86_64" => Some("x86_64"),
        "i686" => Some("x86"),
        _ => None,
    }
}

/// A Kotlin string literal (paths use `/`, and `$` must not interpolate).
fn kotlin_str(raw: &str) -> String {
    let escaped = raw
        .replace('\\', "/")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

fn xml_attr(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: Plugin = Plugin {
        feature: None,
        dir: "bridge",
        class: "bridge.Core",
        contributes: Contributes::NOTHING,
    };
    const AARS: Plugin = Plugin {
        feature: Some("aars"),
        dir: "aars",
        class: "aars.Aars",
        contributes: Contributes {
            dependencies: &["g:a:1"],
            ..Contributes::NOTHING
        },
    };
    const MANIFEST: Plugin = Plugin {
        feature: Some("manifest"),
        dir: "manifest",
        class: "manifest.Manifest",
        contributes: Contributes {
            permissions: &["android.permission.X"],
            manifest: &["<uses-feature android:name=\"f\" />"],
            ..Contributes::NOTHING
        },
    };
    const TABLE: &[Plugin] = &[CORE, AARS, MANIFEST];

    #[test]
    fn features_select_plugins_in_table_order() {
        let classes = |enabled: &[&str]| -> Vec<&str> {
            active_in(TABLE, enabled).iter().map(|p| p.class).collect()
        };
        assert_eq!(classes(&[]), vec!["bridge.Core"]);
        assert_eq!(
            classes(&["manifest", "aars"]),
            vec!["bridge.Core", "aars.Aars", "manifest.Manifest"]
        );
        assert_eq!(classes(&["unknown"]), vec!["bridge.Core"]);
        assert_eq!(features_in(TABLE), vec!["aars", "manifest"]);
        assert_eq!(
            source_dirs(&active_in(TABLE, &["aars"])),
            vec!["bridge", "aars"]
        );
    }

    #[test]
    fn core_plugins_come_before_feature_plugins() {
        let first_feature = PLUGINS.iter().position(|p| p.feature.is_some());
        if let Some(i) = first_feature {
            assert!(PLUGINS[i..].iter().all(|p| p.feature.is_some()));
        }
    }

    #[test]
    fn known_features_match_cargo_manifest() {
        // Keep in step with the Kotlin plugin features in Cargo.toml's
        // `[features]`; `cpal` has no plugin and is not listed.
        assert_eq!(
            known_features(),
            vec![
                "google-auth",
                "camera",
                "foreground-service",
                "battery",
                "system-events",
                "secure-store-no-strongbox"
            ]
        );
    }

    #[test]
    fn foreground_service_manifest_declares_types_and_permissions() {
        let m = foreground_service_manifest(
            &[
                ServiceType::SpecialUse("a link & more"),
                ServiceType::Microphone,
                ServiceType::SpecialUse("ignored"),
            ],
            false,
        );
        assert_eq!(
            m.permissions,
            [
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_SPECIAL_USE",
                "android.permission.FOREGROUND_SERVICE_MICROPHONE",
                "android.permission.POST_NOTIFICATIONS",
                "android.permission.WAKE_LOCK",
            ]
        );
        let xml = &m.application;
        assert!(
            xml.contains("android:name=\"dev.highbay.libmobile.service.MobileForegroundService\"")
        );
        assert!(xml.contains("android:exported=\"false\""));
        assert!(xml.contains("android:stopWithTask=\"true\""));
        assert!(xml.contains("android:foregroundServiceType=\"specialUse|microphone\""));
        assert!(xml.contains("android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE"));
        assert!(xml.contains("android:value=\"a link &amp; more\""));
        assert!(!xml.contains("ignored"));
        assert!(xml.trim_end().ends_with("</service>"));
    }

    #[test]
    fn foreground_service_manifest_each_type() {
        for (ty, name, permission) in [
            (
                ServiceType::DataSync,
                "dataSync",
                "FOREGROUND_SERVICE_DATA_SYNC",
            ),
            (
                ServiceType::Microphone,
                "microphone",
                "FOREGROUND_SERVICE_MICROPHONE",
            ),
            (
                ServiceType::RemoteMessaging,
                "remoteMessaging",
                "FOREGROUND_SERVICE_REMOTE_MESSAGING",
            ),
        ] {
            let m = foreground_service_manifest(&[ty], true);
            assert!(
                m.application
                    .contains(&format!("android:foregroundServiceType=\"{name}\""))
            );
            assert!(m.application.contains("android:stopWithTask=\"false\""));
            assert!(m.application.trim_end().ends_with("/>"));
            assert!(!m.application.contains("<property"));
            assert!(
                m.permissions
                    .contains(&&*format!("android.permission.{permission}"))
            );
        }
        assert_eq!(ServiceType::DataSync.bits(), 1);
        assert_eq!(ServiceType::Microphone.bits(), 128);
        assert_eq!(ServiceType::RemoteMessaging.bits(), 512);
        assert_eq!(ServiceType::SpecialUse("").bits(), 1 << 30);
    }

    #[test]
    fn system_events_plugin_reads_network_state() {
        assert!(!active_plugins(&[]).iter().any(|p| p.dir == "events"));
        let active = active_plugins(&["system-events"]);
        assert!(
            active
                .iter()
                .any(|p| p.class == "events.SystemEventsPlugin")
        );
        assert_eq!(
            merge(&active).permissions,
            ["android.permission.ACCESS_NETWORK_STATE"]
        );
    }

    #[test]
    fn foreground_service_plugin_is_feature_gated() {
        assert!(!active_plugins(&[]).iter().any(|p| p.dir == "service"));
        let active = active_plugins(&["foreground-service"]);
        assert!(
            active
                .iter()
                .any(|p| p.class == "service.ForegroundServicePlugin")
        );
    }

    #[test]
    fn battery_plugin_is_feature_gated_and_sees_device_care() {
        assert!(!active_plugins(&[]).iter().any(|p| p.dir == "battery"));
        let active = active_plugins(&["battery"]);
        assert!(active.iter().any(|p| p.class == "battery.BatteryPlugin"));
        let merged = merge(&active);
        assert_eq!(
            merged.manifest,
            ["<queries><package android:name=\"com.samsung.android.lool\" /></queries>"]
        );
        assert!(merged.permissions.is_empty());
    }

    #[test]
    fn strongbox_flag_is_on_unless_opted_out() {
        let kt = generated_plugins_kt(&[], &active_plugins(&[]));
        assert!(kt.contains("    const val SECURE_STORE_STRONGBOX: Boolean = true\n"));
        let off = ["secure-store-no-strongbox"];
        let kt = generated_plugins_kt(&off, &active_plugins(&off));
        assert!(kt.contains("    const val SECURE_STORE_STRONGBOX: Boolean = false\n"));
    }

    #[test]
    fn secure_store_plugin_is_core() {
        assert!(
            active_plugins(&[])
                .iter()
                .any(|p| p.class == "store.SecureStorePlugin")
        );
    }

    #[test]
    fn merge_dedups_in_plugin_order() {
        const A: Plugin = Plugin {
            feature: None,
            dir: "a",
            class: "a.A",
            contributes: Contributes {
                permissions: &["P1", "P2"],
                dependencies: &["d:1"],
                ..Contributes::NOTHING
            },
        };
        const B: Plugin = Plugin {
            feature: None,
            dir: "b",
            class: "b.B",
            contributes: Contributes {
                permissions: &["P2", "P3"],
                application: &["<meta-data />"],
                ..Contributes::NOTHING
            },
        };
        let merged = merge(&[A, B]);
        assert_eq!(merged.permissions, vec!["P1", "P2", "P3"]);
        assert_eq!(merged.dependencies, vec!["d:1"]);
        assert_eq!(merged.application, vec!["<meta-data />"]);
    }

    #[test]
    fn generated_plugins_kt_lists_qualified_objects() {
        let enabled = ["aars"];
        let kt = generated_plugins_kt(&enabled, &active_in(TABLE, &enabled));
        assert!(kt.starts_with("// GENERATED"));
        assert!(kt.contains("// features: aars\n"));
        assert!(kt.contains("package dev.highbay.libmobile\n"));
        assert!(kt.contains("val activePlugins: List<MobileActivityPlugin> = listOf(\n"));
        assert!(kt.contains("        dev.highbay.libmobile.bridge.Core,\n"));
        assert!(kt.contains("        dev.highbay.libmobile.aars.Aars,\n"));
        assert!(!kt.contains("Manifest"));
    }

    #[test]
    fn gradle_script_adds_source_root_and_aars() {
        let enabled = ["aars"];
        let script = gradle_script(
            &enabled,
            Path::new("/t/out/java"),
            &merge(&active_in(TABLE, &enabled)),
        );
        assert!(script.contains("java.srcDir '/t/out/java'\n"));
        assert!(script.contains("kotlin.srcDir '/t/out/java'\n"));
        assert!(script.contains("dependencies {\n    implementation 'g:a:1'\n}\n"));
        // No manifest contributions: no transform at all.
        assert!(!script.contains("MERGED_MANIFEST"));
    }

    #[test]
    fn gradle_script_splices_manifest_contributions() {
        let enabled = ["manifest"];
        let script = gradle_script(
            &enabled,
            Path::new("/t/java"),
            &merge(&active_in(TABLE, &enabled)),
        );
        assert!(script.contains("ext.LIBMOBILE_PERMISSIONS = ['android.permission.X']\n"));
        assert!(
            script
                .contains("ext.LIBMOBILE_MANIFEST_XML = ['<uses-feature android:name=\"f\" />']\n")
        );
        assert!(script.contains("ext.LIBMOBILE_APPLICATION_XML = []\n"));
        assert!(script.contains("SingleArtifact$MERGED_MANIFEST"));
        assert!(!script.contains("dependencies {"));
    }

    #[test]
    fn gradle_script_without_contributions_is_sources_only() {
        let script = gradle_script(&[], Path::new("/t/java"), &Merged::default());
        assert!(script.contains("// features: <none>\n"));
        assert!(!script.contains("dependencies"));
        assert!(!script.contains("MERGED_MANIFEST"));
    }

    #[test]
    fn window_insets_plugin_is_core() {
        assert!(
            active_plugins(&[])
                .iter()
                .any(|p| p.class == "bridge.WindowInsetsPlugin")
        );
    }

    #[test]
    fn intent_plugin_is_core() {
        assert!(
            active_plugins(&[])
                .iter()
                .any(|p| p.class == "bridge.IntentPlugin")
        );
    }

    #[test]
    fn text_input_plugin_is_core() {
        assert!(
            active_plugins(&[])
                .iter()
                .any(|p| p.class == "bridge.TextInputPlugin")
        );
    }

    #[test]
    fn link_plugin_is_core_and_needs_nothing() {
        let active = active_plugins(&[]);
        assert!(active.iter().any(|p| p.class == "bridge.LinkPlugin"));
        let merged = merge(&active);
        assert!(merged.permissions.is_empty() && merged.manifest.is_empty());
    }

    #[test]
    fn dirs_plugin_is_core_and_needs_nothing() {
        let active = active_plugins(&[]);
        assert!(active.iter().any(|p| p.class == "bridge.DirsPlugin"));
        let merged = merge(&active);
        assert!(merged.permissions.is_empty() && merged.manifest.is_empty());
    }

    #[test]
    fn build_info_plugin_is_core_and_needs_nothing() {
        let active = active_plugins(&[]);
        assert!(active.iter().any(|p| p.class == "bridge.BuildInfoPlugin"));
        let merged = merge(&active);
        assert!(merged.permissions.is_empty() && merged.manifest.is_empty());
    }

    #[test]
    fn trim_memory_plugin_is_core_and_needs_nothing() {
        let active = active_plugins(&[]);
        assert!(active.iter().any(|p| p.class == "bridge.TrimMemoryPlugin"));
        let merged = merge(&active);
        assert!(merged.permissions.is_empty() && merged.manifest.is_empty());
    }

    #[test]
    fn permission_plugin_is_core() {
        assert!(
            active_plugins(&[])
                .iter()
                .any(|p| p.class == "bridge.PermissionPlugin")
        );
    }

    #[test]
    fn google_auth_plugin_brings_credential_manager() {
        assert!(!active_plugins(&[]).iter().any(|p| p.dir == "auth"));
        let active = active_plugins(&["google-auth"]);
        assert!(active.iter().any(|p| p.class == "auth.GoogleAuthPlugin"));
        let merged = merge(&active);
        assert!(
            merged
                .dependencies
                .contains(&"androidx.credentials:credentials:1.5.0")
        );
        assert!(merged.manifest.is_empty() && merged.permissions.is_empty());
    }

    #[test]
    fn camera_plugin_declares_queries_and_optional_feature() {
        assert!(!active_plugins(&[]).iter().any(|p| p.dir == "camera"));
        let active = active_plugins(&["camera"]);
        assert!(active.iter().any(|p| p.class == "camera.CameraPlugin"));
        let merged = merge(&active);
        assert!(
            merged
                .manifest
                .iter()
                .any(|x| x.contains("android.media.action.IMAGE_CAPTURE"))
        );
        assert!(
            merged
                .manifest
                .iter()
                .any(|x| x.contains("android:required=\"false\""))
        );
        assert!(merged.permissions.is_empty());
    }

    #[test]
    fn groovy_strings_are_escaped() {
        assert_eq!(groovy_path(Path::new(r"C:\a'b")), r"'C:/a\'b'");
        assert_eq!(groovy_str("<a b=\"c\"/>"), "'<a b=\"c\"/>'");
    }

    fn project_files() -> Vec<(&'static str, String)> {
        let mut project = GradleProject::new("dev.example.app", "example_app");
        project.label = "A & B".into();
        project
            .permissions
            .push("android.permission.INTERNET".into());
        project
            .extra_scripts
            .push("/crate/android/extra.gradle.kts".into());
        project.render(
            Path::new("/t/out/libmobile.gradle"),
            Path::new("/t/release/libexample_app.so"),
            "arm64-v8a",
        )
    }

    fn file<'a>(files: &'a [(&'static str, String)], name: &str) -> &'a str {
        &files.iter().find(|(n, _)| *n == name).unwrap().1
    }

    #[test]
    fn project_has_every_file() {
        let names: Vec<_> = project_files().iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "settings.gradle.kts",
                "build.gradle.kts",
                "gradle.properties",
                "app/build.gradle.kts",
                "app/src/main/AndroidManifest.xml",
            ]
        );
    }

    #[test]
    fn app_module_packages_the_so_and_applies_scripts_in_order() {
        let files = project_files();
        let app = file(&files, "app/build.gradle.kts");
        assert!(app.contains("applicationId = \"dev.example.app\""));
        assert!(app.contains("from(\"/t/release/libexample_app.so\")"));
        assert!(app.contains("into(layout.buildDirectory.dir(\"rustJniLibs/arm64-v8a\"))"));
        let generated = app
            .find("apply(from = \"/t/out/libmobile.gradle\")")
            .unwrap();
        let extra = app
            .find("apply(from = \"/crate/android/extra.gradle.kts\")")
            .unwrap();
        assert!(generated < extra, "custom scripts must apply last");
    }

    #[test]
    fn manifest_names_activity_lib_and_permissions() {
        let files = project_files();
        let manifest = file(&files, "app/src/main/AndroidManifest.xml");
        assert!(manifest.contains("android:name=\"dev.highbay.libmobile.MobileNativeActivity\""));
        assert!(manifest.contains("android:value=\"example_app\""));
        assert!(manifest.contains("android:label=\"A &amp; B\""));
        assert!(manifest.contains("android.permission.INTERNET"));
        assert!(manifest.contains("android:launchMode=\"singleTop\""));
    }

    #[test]
    fn url_scheme_filters_one_per_scheme() {
        assert_eq!(url_scheme_filters(&[]), "");
        let xml = url_scheme_filters(&["handlr".into(), "b-2".into()]);
        assert_eq!(xml.matches("<intent-filter>").count(), 2);
        assert_eq!(xml.matches("android.intent.action.VIEW").count(), 2);
        assert_eq!(xml.matches("android.intent.category.BROWSABLE").count(), 2);
        assert!(xml.contains("<data android:scheme=\"handlr\" />"));
        assert!(xml.contains("<data android:scheme=\"b-2\" />"));
        assert!(!xml.contains("autoVerify"));
    }

    #[test]
    fn kotlin_strings_do_not_interpolate() {
        assert_eq!(kotlin_str(r#"C:\a$b"c"#), r#""C:/a\$b\"c""#);
    }

    #[test]
    fn abi_from_target() {
        assert_eq!(android_abi("aarch64-linux-android"), Some("arm64-v8a"));
        assert_eq!(android_abi("armv7-linux-androideabi"), Some("armeabi-v7a"));
        assert_eq!(android_abi("x86_64-linux-android"), Some("x86_64"));
        assert_eq!(android_abi("riscv64-linux-android"), None);
    }
}
