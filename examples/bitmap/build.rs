fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-bitmap/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.bitmap")
        .label("libmobile bitmap")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
