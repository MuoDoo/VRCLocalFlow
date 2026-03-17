fn main() {
    #[cfg(target_os = "windows")]
    {
        // whisper-rs's ggml-backend uses Windows Registry APIs (RegCloseKey, etc.)
        println!("cargo:rustc-link-lib=advapi32");
    }
}
