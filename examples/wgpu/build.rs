fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-wgpu/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.wgpu")
        .label("libmobile wgpu")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
