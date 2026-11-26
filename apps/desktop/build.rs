fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/mcw_launcher.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        let ico = std::path::Path::new("../../assets/icons/mcw_launcher.ico");
        if ico.is_file() {
            res.set_icon(ico.to_string_lossy().as_ref());
        }
        res.set("ProductName", "MCW Launcher: Rust Edition");
        res.set(
            "FileDescription",
            "MCW Launcher: Rust Edition - Native Minecraft Launcher",
        );
        res.set("CompanyName", "MCW Launcher Open Source");
        res.set("LegalCopyright", "MIT License");
        res.set("OriginalFilename", "MCW-Launcher.exe");
        if let Err(err) = res.compile() {
            println!("cargo:warning=winresource compile skipped: {err}");
        }
    }
}
