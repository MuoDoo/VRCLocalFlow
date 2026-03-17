fn main() {
    // whisper-rs's ggml-backend uses Windows Registry APIs (RegCloseKey, etc.)
    // which require advapi32.lib. The Tauri app linked this implicitly via its
    // Windows dependencies, but the standalone engine binary needs it explicitly.
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-lib=advapi32");
}
