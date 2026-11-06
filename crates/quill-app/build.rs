fn main() {
    println!("cargo:rerun-if-changed=windows.manifest");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("windows-msvc") {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest");
        println!("cargo:rustc-link-arg-bin=quill=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=quill=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
