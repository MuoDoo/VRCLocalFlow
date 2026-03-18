fn main() {
    #[cfg(target_os = "windows")]
    {
        // whisper-rs's ggml-backend uses Windows Registry APIs (RegCloseKey, etc.)
        println!("cargo:rustc-link-lib=advapi32");

        // ct2rs compiles CTranslate2 with /MT (static CRT) while whisper-rs's
        // CUDA objects use /MD (dynamic CRT). These are fundamentally incompatible
        // at the metadata level (LNK2038). At runtime both CRT variants work fine
        // when the exe links dynamically — /FORCE tells the linker to produce a
        // valid binary despite the metadata mismatch.
        println!("cargo:rustc-link-arg=/FORCE");
    }
}
