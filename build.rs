fn main() {
    println!("cargo:rerun-if-changed=assets/active.ico");
    println!("cargo:rerun-if-changed=assets/idle.ico");
    println!("cargo:rerun-if-changed=assets/caffeinate.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The manifest carries its own assembly version, which is a second
        // place to remember to bump and therefore a place that silently drifts.
        // Checking it here turns that into a build failure instead.
        let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets this");
        let manifest = std::fs::read_to_string("assets/caffeinate.manifest")
            .expect("assets/caffeinate.manifest is missing");
        let expected = format!("version=\"{version}.0\"");
        assert!(
            manifest.contains(&expected),
            "assets/caffeinate.manifest must declare {expected} to match Cargo.toml"
        );

        let mut res = winresource::WindowsResource::new();
        // Without this manifest the process is DPI_AWARENESS_UNAWARE, so
        // Windows bitmap-stretches the UI at any scaling other than 100% and
        // the menu text comes out blurry.
        res.set_manifest_file("assets/caffeinate.manifest");
        // Id 1 is the lowest numbered ICON resource, which Windows also uses as
        // the executable's icon in Explorer, so it holds the active (amber)
        // version.
        res.set_icon_with_id("assets/active.ico", "1");
        res.set_icon_with_id("assets/idle.ico", "2");
        res.compile()
            .expect("resource compilation failed (check that mingw's windres.exe is on PATH)");
    }
}
