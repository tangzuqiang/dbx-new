fn main() {
    // The desktop process performs some database metadata work on Tauri's main
    // thread. Windows' default executable stack (1 MiB) is too small for the
    // deeper call chains used when a table workspace is opened, causing
    // STATUS_STACK_OVERFLOW in debug builds. Reserve the same practical amount
    // commonly available to worker threads without changing heap behaviour.
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-arg=/STACK:8388608");

    let is_win7_target = std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("win7");
    if is_win7_target && std::env::var_os("CARGO_FEATURE_CUSTOM_PROTOCOL").is_none() {
        panic!("Windows 7 release builds must enable the custom-protocol feature");
    }

    // Force rebuild to re-embed frontend assets
    tauri_build::build()
}
