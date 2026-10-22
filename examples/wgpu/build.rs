fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-wgpu/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.wgpu")
        .label("libmobile wgpu")
        // For the runtime permission requests on a tap.
        .permission("android.permission.RECORD_AUDIO")
        .permission("android.permission.CAMERA")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
