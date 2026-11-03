fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icon.ico");
        resource.set("ProductName", "Argon");
        resource.set(
            "FileDescription",
            "Aplicativo desktop de review de Pull Requests do GitHub",
        );
        resource
            .compile()
            .expect("failed to embed windows resources");
    }
}
