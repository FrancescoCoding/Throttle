//! Build script: embed a Windows application manifest requesting
//! Administrator elevation (WinDivert needs it to open its device).

fn main() {
    // Only embed in release builds: the manifest also lands in test binaries,
    // which then refuse to run without elevation (os error 740).
    #[cfg(target_os = "windows")]
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        use embed_manifest::manifest::ExecutionLevel;
        use embed_manifest::{embed_manifest, new_manifest};

        embed_manifest(
            new_manifest("Throttle.App")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("failed to embed requireAdministrator manifest");
    }

    // Embed the application icon into the executable so it shows in Explorer,
    // the taskbar, and shortcuts. Non-fatal: a missing resource compiler must
    // not break the build (the in-app window icon is set separately at runtime).
    #[cfg(target_os = "windows")]
    if std::path::Path::new("assets/throttle.ico").exists() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/throttle.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon embed skipped: {e}");
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=throttle.exe.manifest");
    println!("cargo:rerun-if-changed=assets/throttle.ico");
}
