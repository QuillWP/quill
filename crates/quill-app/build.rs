fn main() {
    println!("cargo:rerun-if-changed=windows.manifest");
    build_info();
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

/// Record when the program was built and from which branch and commit, for
/// the About window (`build_info.rs`). This runs again whenever any crate's
/// source or the checked-out commit changes, so the time is that of the
/// build that last changed the program. Without git (a source zip) only the
/// time is recorded.
fn build_info() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    for dir in [
        "src",
        "../quill-core/src",
        "../quill-formats/src",
        "../quill-layout/src",
    ] {
        println!("cargo:rerun-if-changed={dir}");
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    // A commit or a branch switch moves HEAD or the branch's ref.
    for path in ["HEAD", "packed-refs"] {
        if let Some(p) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    let branch = git(&["symbolic-ref", "-q", "--short", "HEAD"]);
    if let Some(branch) = &branch
        && let Some(p) = git(&["rev-parse", "--git-path", &format!("refs/heads/{branch}")])
    {
        println!("cargo:rerun-if-changed={p}");
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=QUILL_BUILD_TIME={secs}");
    // The same time on the building machine's clock, when `date` can say it
    // (-r on the Mac and BSD, -d on Linux). The app falls back to UTC.
    let local = ["-r", "-d"].iter().find_map(|flag| {
        let at = if *flag == "-d" {
            format!("@{secs}")
        } else {
            secs.to_string()
        };
        Command::new("date")
            .args([*flag, at.as_str(), "+%Y-%m-%d %H:%M %Z"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    });
    if let Some(local) = local {
        println!("cargo:rustc-env=QUILL_BUILD_LOCAL_TIME={local}");
    }
    if let Some(commit) = git(&["rev-parse", "--short", "HEAD"]) {
        println!("cargo:rustc-env=QUILL_BUILD_COMMIT={commit}");
        // Changes to tracked files that are not committed.
        let dirty =
            git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        println!("cargo:rustc-env=QUILL_BUILD_DIRTY={}", u8::from(dirty));
    }
    if let Some(branch) = branch {
        println!("cargo:rustc-env=QUILL_BUILD_BRANCH={branch}");
    }
}
