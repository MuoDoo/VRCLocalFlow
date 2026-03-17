fn main() {
    #[cfg(target_os = "windows")]
    {
        // whisper-rs's ggml-backend uses Windows Registry APIs (RegCloseKey, etc.)
        // which require advapi32.lib.
        println!("cargo:rustc-link-lib=advapi32");

        // whisper-rs-sys compiles whisper.cpp with /MT (static CRT) while ct2rs
        // compiles CTranslate2 with /MD (dynamic CRT). Rust on MSVC defaults to
        // /MD. Tell the linker to ignore the static CRT libs to resolve the
        // MT_StaticRelease vs MD_DynamicRelease mismatch (LNK2038/LNK2005).
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcmt");
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcpmt");
    }
}
