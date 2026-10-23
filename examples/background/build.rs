use libmobile::gradle::ServiceType;

fn main() {
    // Writes target/<triple>/<profile>/gradle/libmobile-example-background/.
    libmobile::gradle::Project::new("dev.highbay.libmobile.example.background")
        .label("libmobile background")
        // The example's TCP connection.
        .permission("android.permission.INTERNET")
        .foreground_service(&[ServiceType::SpecialUse(
            "a persistent connection to the user's own server",
        )])
        .request_ignore_battery_optimizations()
        .generate()
        .expect("libmobile: generating the Gradle project");
}
