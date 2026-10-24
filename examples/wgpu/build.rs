use libmobile::gradle::{Icon, Project};

fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-wgpu/.
    Project::new("dev.highbay.libmobile.example.wgpu")
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
        .generate()
        .expect("libmobile: generating the Gradle project");
}
