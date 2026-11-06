#!/usr/bin/env python3
"""Build native Quill packages, or a small source ZIP for any build host.

Python 3.11+, standard library only. Run --help for options.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SOURCE_DIRS = (".github", "assets", "crates", "data", "docs", "tests", "tools")
SOURCE_FILES = (
    "Cargo.toml", "Cargo.lock", ".gitignore", "README.md",
    "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.txt",
)
EXCLUDED_DIRS = {"target", "dist", ".git", "__pycache__", "Quill.app"}


def run(*command: str, capture: bool = False) -> str:
    print("+ " + " ".join(command), flush=True)
    result = subprocess.run(command, cwd=ROOT, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout or ""


def source_files(root: Path):
    """Walk only source directories; never visit the large build trees."""
    for name in SOURCE_FILES:
        path = root / name
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Required source file is missing or a symlink: {path}")
        yield path
    directories = [root / name for name in SOURCE_DIRS]
    # Keep the developer fuzz harness, but not generated corpora/crash artifacts.
    if (root / "fuzz").is_dir():
        for name in ("Cargo.toml", "Cargo.lock", "README.md", ".gitignore"):
            path = root / "fuzz" / name
            if path.is_file() and not path.is_symlink():
                yield path
        directories.append(root / "fuzz" / "fuzz_targets")
    for directory in directories:
        if not directory.exists():
            continue
        if directory.is_symlink():
            raise ValueError(f"Source directory is a symlink: {directory}")
        for base, dirs, files in os.walk(directory):
            dirs[:] = sorted(name for name in dirs
                             if name not in EXCLUDED_DIRS
                             and not (Path(base) / name).is_symlink())
            for name in sorted(files):
                path = Path(base) / name
                if name == ".DS_Store" or name.endswith((".pyc", ".pyo")):
                    continue
                if path.is_symlink():
                    raise ValueError(f"Source file is a symlink: {path}")
                yield path


def source_package(root: Path, output: Path, version: str) -> Path:
    destination = output / f"quill-{version}-source.zip"
    count = 0
    with tempfile.TemporaryDirectory(prefix=".qwp-source-", dir=output) as tmp:
        archive = Path(tmp) / destination.name
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
            for path in source_files(root):
                if output != root and path.is_relative_to(output):
                    continue
                bundle.write(path, Path("Quill") / path.relative_to(root))
                count += 1
        archive.replace(destination)
    print(f"Source: {destination} ({count} files, {destination.stat().st_size / 1024**2:.2f} MiB)")
    return destination


def native_target() -> tuple[str, str]:
    host = next(line.removeprefix("host: ") for line in run("rustc", "-vV", capture=True).splitlines()
                if line.startswith("host: "))
    target = os.environ.get("CARGO_BUILD_TARGET", host)
    system = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system())
    expected = {"macos": "apple-darwin", "linux": "linux", "windows": "windows-msvc"}
    if system not in expected or expected[system] not in target:
        raise ValueError(f"Build the app package on its matching OS; {platform.system()} cannot package {target}.")
    return system, target


def build(check: bool) -> Path:
    if check:
        run("cargo", "fmt", "--all", "--check")
        run("cargo", "clippy", "--workspace", "--all-targets",
            "--locked", "--", "-D", "warnings")
        run("cargo", "test", "--workspace", "--locked")
    # Cargo reports the actual executable path, respecting CARGO_TARGET_DIR.
    messages = run("cargo", "build", "--release", "--locked",
                   "--package", "quill-app", "--bin", "quill",
                   "--message-format=json-render-diagnostics", capture=True)
    for line in reversed(messages.splitlines()):
        message = json.loads(line)
        if (message.get("reason") == "compiler-artifact"
                and message.get("target", {}).get("name") == "quill"
                and message.get("executable")):
            return Path(message["executable"])
    raise ValueError("Cargo did not report a Quill executable.")


def copy_common(destination: Path) -> None:
    shutil.copytree(ROOT / "data", destination / "data",
                    ignore=shutil.ignore_patterns(".DS_Store"))
    for name in ("README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.txt"):
        shutil.copy2(ROOT / name, destination / name)


def portable_package(executable: Path, output: Path, version: str,
                     system: str, arch: str) -> Path:
    name = f"quill-{version}-{system}-{arch}"
    suffix = ".zip" if system == "windows" else ".tar.gz"
    destination = output / (name + suffix)
    with tempfile.TemporaryDirectory(prefix=".qwp-package-", dir=output) as tmp:
        stage = Path(tmp) / name
        stage.mkdir()
        shutil.copy2(executable, stage / ("quill.exe" if system == "windows" else "quill"))
        copy_common(stage)
        shutil.copy2(ROOT / "assets/quill.png", stage / "quill.png")
        if system == "linux":
            shutil.copy2(ROOT / "tools/quill.desktop", stage / "quill.desktop")
        archive = Path(tmp) / destination.name
        if system == "windows":
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
                for path in sorted(stage.rglob("*")):
                    if path.is_file():
                        bundle.write(path, path.relative_to(stage))
        else:
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(stage, arcname=name)
        archive.replace(destination)
    return destination


def mac_package(executable: Path, output: Path, version: str, arch: str) -> Path:
    identity = os.environ.get("MACOS_SIGN_IDENTITY")
    notary = os.environ.get("MACOS_NOTARY_PROFILE")
    destination = output / f"Quill-{version}-macos-{arch}.dmg"
    with tempfile.TemporaryDirectory(prefix=".qwp-package-", dir=output) as tmp:
        root = Path(tmp)
        image_root = root / "image"
        app = image_root / "Quill.app"
        contents = app / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        resources = contents / "Resources"
        resources.mkdir()
        shutil.copy2(executable, contents / "MacOS/quill")
        copy_common(resources)
        run("sips", "-s", "format", "icns", str(ROOT / "assets/quill.png"),
            "--out", str(resources / "Quill.icns"))
        info = plistlib.loads((ROOT / "tools/Info.plist").read_bytes())
        info["CFBundleVersion"] = info["CFBundleShortVersionString"] = version
        (contents / "Info.plist").write_bytes(plistlib.dumps(info))
        if identity:
            run("codesign", "--force", "--options", "runtime", "--timestamp", "--sign", identity, str(app))
        else:
            run("codesign", "--force", "--sign", "-", str(app))
        run("codesign", "--verify", "--deep", "--strict", str(app))
        (image_root / "Applications").symlink_to("/Applications")
        image = root / destination.name
        run("hdiutil", "create", "-quiet", "-volname", "Quill", "-srcfolder", str(image_root),
            "-ov", "-format", "UDZO", str(image))
        run("hdiutil", "verify", "-quiet", str(image))
        if notary:
            run("codesign", "--force", "--timestamp", "--sign", identity, str(image))
            run("xcrun", "notarytool", "submit", str(image), "--keychain-profile", notary, "--wait")
            run("xcrun", "stapler", "staple", str(app))
            run("xcrun", "stapler", "staple", str(image))
            run("xcrun", "stapler", "validate", str(image))
        # Publish only after staging/signing succeeds; preserve other packages.
        app_output = output / "Quill.app"
        if app_output.is_symlink():
            raise ValueError(f"Refusing to replace symlink: {app_output}")
        if app_output.exists():
            shutil.rmtree(app_output)
        shutil.move(str(app), str(app_output))
        image.replace(destination)
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", action="store_true", help="create only a source ZIP; no Rust toolchain needed")
    parser.add_argument("--with-source", action="store_true", help="also create a source ZIP alongside the native package")
    parser.add_argument("--check", action="store_true", help="run formatting, strict lint, and tests before building")
    parser.add_argument("--output", type=Path, default=ROOT / "dist", help="output directory (default: dist)")
    args = parser.parse_args()
    if args.source and (args.with_source or args.check):
        parser.error("--source cannot be combined with --with-source or --check")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output = args.output.resolve()
    if any(output.is_relative_to(ROOT / name) for name in SOURCE_DIRS):
        parser.error("Choose an output directory outside the source directories (for example dist).")
    output.mkdir(parents=True, exist_ok=True)
    if args.source:
        source_package(ROOT, output, version)
        return 0
    if os.environ.get("MACOS_NOTARY_PROFILE") and not os.environ.get("MACOS_SIGN_IDENTITY"):
        raise ValueError("MACOS_NOTARY_PROFILE requires MACOS_SIGN_IDENTITY.")
    system, target = native_target()
    executable = build(args.check)
    run(sys.executable, str(ROOT / "tools/generate-notices.py"))
    arch = target.split("-")[0]
    if system == "macos":
        package = mac_package(executable, output, version, arch)
    else:
        package = portable_package(executable, output, version, system, arch)
    print(f"Package: {package} ({package.stat().st_size / 1024**2:.2f} MiB)")
    if args.with_source:
        source_package(ROOT, output, version)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"package.py: {error}", file=sys.stderr)
        raise SystemExit(1)
