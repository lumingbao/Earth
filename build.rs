fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/earth.ico");
    println!("cargo:rerun-if-env-changed=EARTH_UPDATE_MACOS_TEAM_ID");
    println!("cargo:rerun-if-env-changed=EARTH_UPDATE_WINDOWS_CERT_SHA256");

    // Build scripts execute on the host; this variable describes the actual target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version =
            std::env::var("CARGO_PKG_VERSION").expect("Cargo must supply a package version");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/earth.ico");
        res.set("ProductName", "Earth");
        res.set(
            "FileDescription",
            "Earth — Smart Cross-Platform Clipboard Manager",
        );
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        res.compile()
            .expect("failed to compile Windows icon/version resources for the target");
    }
}
