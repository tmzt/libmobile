fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-audiocapture/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.audiocapture")
        .label("libmobile audiocapture")
        // Asked for at runtime; the example records from the microphone.
        .permission("android.permission.RECORD_AUDIO")
        .generate()
        .expect("libmobile: generating the Gradle project");
}
