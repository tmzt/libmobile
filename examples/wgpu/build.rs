use libmobile::gradle::{AppPlugin, Icon, Project};

fn main() {
    // `LIBMOBILE_WGPU_NO_ALL_FILES=1` leaves MANAGE_EXTERNAL_STORAGE out of
    // the manifest, so `--es cmd files-access` shows `NotDeclared`.
    println!("cargo:rerun-if-env-changed=LIBMOBILE_WGPU_NO_ALL_FILES");
    let all_files = std::env::var_os("LIBMOBILE_WGPU_NO_ALL_FILES").is_none_or(|v| v.is_empty());
    // Writes target/<triple>/<profile>/gradle/libmobile-example-wgpu/.
    let project = Project::new("dev.highbay.libmobile.example.wgpu")
        .label("libmobile wgpu")
        // `adb shell am start -a android.intent.action.VIEW -d 'libmobile-wgpu://h/p#secret'`
        .url_scheme("libmobile-wgpu")
        // A test icon: an orange disc on transparency, scaled into the
        // adaptive icon's centre 66 dp over a dark background, with a
        // white silhouette for themed icons.
        .icon(
            Icon::png("icon/icon-1024.png")
                .background_color([0x20, 0x24, 0x2c])
                .monochrome_png("icon/monochrome-1024.png"),
        )
        // The example's own plugin: logs each resume and reports it to the
        // cdylib (`android/java/dev/highbay/libmobile/example/wgpu/`).
        .plugin(
            AppPlugin::new("dev.highbay.libmobile.example.wgpu.ResumeLogPlugin")
                .sources("android/java"),
        );
    // `--es cmd files-access` and `files-access-open`.
    let project = if all_files {
        project.all_files_access()
    } else {
        project
    };
    project
        .generate()
        .expect("libmobile: generating the Gradle project");
}
