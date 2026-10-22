fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-securestore/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.securestore")
        .label("libmobile securestore")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
