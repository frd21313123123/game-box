fn main() {
    // With Steam enabled the binary links libsteam_api dynamically; let it be
    // found next to the executable on Linux/macOS (Windows does this by default).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if std::env::var_os("CARGO_FEATURE_STEAM").is_some() {
        match target_os.as_str() {
            "linux" => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN"),
            "macos" => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path"),
            _ => {}
        }
    }
}
