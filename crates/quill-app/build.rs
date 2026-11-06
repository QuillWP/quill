fn main() {
    println!("cargo:rerun-if-changed=windows.manifest");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("windows-msvc") {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest");
        #[cfg(windows)]
        embed_icons(&target);
        println!("cargo:rustc-link-arg-bin=quill=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=quill=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}

#[cfg(windows)]
fn embed_icons(target: &str) {
    use std::{env, fs, path::PathBuf, process::Command};

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let app_icon = root.join("assets/quill.ico");
    let document_icon = root.join("assets/quill-document.ico");
    println!("cargo:rerun-if-changed={}", app_icon.display());
    println!("cargo:rerun-if-changed={}", document_icon.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let script = out.join("quill.rc");
    let resource = out.join("quill.res");
    let rc_path = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/");
    fs::write(
        &script,
        format!(
            "1 ICON \"{}\"\n2 ICON \"{}\"\n",
            rc_path(&app_icon),
            rc_path(&document_icon)
        ),
    )
    .expect("write Quill icon resources");

    // cc locates MSVC and includes the Windows SDK tools in its PATH.
    let linker = cc::windows_registry::find_tool(target, "link.exe")
        .expect("find MSVC for Quill's Windows icons");
    let sdk_path = linker
        .get_envs()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"));
    let compiler = sdk_path
        .and_then(|(_, paths)| {
            env::split_paths(paths)
                .map(|p| p.join("rc.exe"))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("rc.exe"));
    let status = Command::new(compiler)
        .envs(linker.get_envs())
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&script)
        .status()
        .expect("run Windows SDK resource compiler");
    assert!(status.success(), "compile Quill icon resources");
    println!("cargo:rustc-link-arg-bin=quill={}", resource.display());
}
