fn main() {
    #[cfg(target_os = "windows")]
    {
        // whisper-rs's ggml-backend uses Windows Registry APIs (RegCloseKey, etc.)
        println!("cargo:rustc-link-lib=advapi32");

        // ct2rs compiles CTranslate2 with /MT (static CRT). With +crt-static,
        // Rust and whisper-rs C/C++ also use /MT. However, whisper-rs's CUDA
        // objects (compiled by nvcc) always use /MD, pulling in msvcprt/msvcrt
        // (dynamic CRT import libs) which conflict with the static CRT.
        //
        // Suppress the dynamic CRT import libs so the linker only sees the
        // static CRT (libcmt/libcpmt). CUDA objects' symbol references are
        // identical and get resolved by the static libs.
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:msvcrt");
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:msvcprt");

        // /FORCE turns the remaining LNK2038 metadata mismatches (between
        // CUDA /MD objects and ct2rs /MT objects) into warnings instead of
        // errors. Combined with NODEFAULTLIB above, this is safe — all
        // symbols resolve from the static CRT.
        println!("cargo:rustc-link-arg=/FORCE");
    }
}
