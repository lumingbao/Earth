fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/earth.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/earth.ico");
        res.set("ProductName", "Earth");
        res.set("FileDescription", "Earth — Smart Cross-Platform Clipboard Manager");
        res.compile().expect("failed to compile Windows icon/version resources");
    }
}
