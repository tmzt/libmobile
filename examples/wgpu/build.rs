fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-wgpu/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.wgpu")
        .label("libmobile wgpu")
        // `adb shell am start -a android.intent.action.VIEW -d 'libmobile-wgpu://h/p#secret'`
        .url_scheme("libmobile-wgpu")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
