fn main() {
    // screencapturekit crate uses Swift interop and needs the Swift runtime libs.
    // Its build.rs sets rpaths but they don't propagate to the final binary,
    // so we add them here.
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    tauri_build::build()
}
