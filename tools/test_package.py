"""Archive-content checks; no compiler, signing credentials, or network needed."""

from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="quill package's test ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "source"
        self.root.mkdir()
        for name in package.SOURCE_FILES:
            (self.root / name).write_text("fixture\n")
        for name in ["assets/quill.png", "data/language.dat", "data/licenses/notice.txt",
                     "tests/visual/example.png", "crates/new-uncommitted.rs", "tools/quill.desktop",
                     "fuzz/fuzz_targets/example.rs", "fuzz/support/oracle.rs", "fuzz/Cargo.toml",
                     "assets/quill.icns", "assets/quill-document.icns", "assets/quill-about.jpg",
                     "assets/quill.ico", "assets/quill-document.ico", "assets/quill-document.png",
                     "tools/quill-mime.xml", "tools/install-linux.sh"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")
        self.output = self.root / "dist"
        self.output.mkdir()

    def test_source_keeps_images_data_and_working_files_but_excludes_builds(self):
        for name in ["target/huge.bin", "crates/target/huge.bin", "fuzz/target/huge.bin",
                     "fuzz/corpus/input", "fuzz/artifacts/crash", "Quill.app/generated",
                     "drafts/icon.png", "local/notes.md",
                     ".git/private", "tools/__pycache__/module.pyc", "assets/.DS_Store"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("excluded")
        (self.output / "keep-existing-package.zip").write_bytes(b"preserved")
        archive = package.source_package(self.root, self.output, "1.2.3")
        with zipfile.ZipFile(archive) as bundle:
            names = set(bundle.namelist())
            for name in ["assets/quill.png", "tests/visual/example.png", "data/language.dat",
                         "data/licenses/notice.txt", "crates/new-uncommitted.rs", "Cargo.lock",
                         "fuzz/fuzz_targets/example.rs", "fuzz/support/oracle.rs"]:
                self.assertIn("Quill/" + name, names)
            self.assertFalse(any("huge.bin" in name or "corpus/" in name or ".git/" in name
                                 or ".DS_Store" in name or "__pycache__" in name
                                 or "drafts/" in name or "local/" in name for name in names))
        self.assertEqual((self.output / "keep-existing-package.zip").read_bytes(), b"preserved")

    def test_native_archives_include_runtime_data_licenses_and_executable(self):
        executable = Path(self.tmp.name) / "quill"
        executable.write_bytes(b"fake executable for archive structure verification")
        executable.chmod(0o755)
        with patch.object(package, "ROOT", self.root):
            for system in ["windows", "linux"]:
                archive = package.portable_package(executable, self.output, "1.2.3", system, "aarch64")
                if system == "windows":
                    with zipfile.ZipFile(archive) as bundle:
                        names = set(bundle.namelist())
                    binary = "quill.exe"
                else:
                    with tarfile.open(archive) as bundle:
                        prefix = "quill-1.2.3-linux-aarch64/"
                        names = {name.removeprefix(prefix) for name in bundle.getnames()}
                        self.assertTrue(bundle.getmember(prefix + "quill").mode & 0o111)
                    binary = "quill"
                    self.assertIn("quill.desktop", names)
                for name in [binary, "quill.png", "quill.ico", "quill-document.png", "quill-document.ico",
                             "quill-about.jpg", "quill-mime.xml", "data/language.dat", "data/licenses/notice.txt",
                             "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.txt", "README.md"]:
                    self.assertIn(name, names)

    def test_mac_bundle_uses_separate_app_and_document_icons_and_qwp_type(self):
        import plistlib
        executable = Path(self.tmp.name) / "quill"
        executable.write_bytes(b"executable")
        info = plistlib.loads((package.ROOT / "tools/Info.plist").read_bytes())
        (self.root / "tools/Info.plist").write_bytes(plistlib.dumps(info))
        app = Path(self.tmp.name) / "Quill.app"
        with patch.object(package, "ROOT", self.root), patch.object(package, "run"):
            package.mac_app(executable, app, "1.2.3", None)
        contents = app / "Contents"
        info = plistlib.loads((contents / "Info.plist").read_bytes())
        self.assertEqual(info["CFBundleIdentifier"], "com.quillwp.Quill")
        self.assertEqual(info["CFBundleExecutable"], "quill")
        self.assertNotIn("UTImportedTypeDeclarations", info)
        self.assertEqual(info["CFBundleDocumentTypes"][0]["LSItemContentTypes"],
                         ["com.quillwp.document"])
        native = info["UTExportedTypeDeclarations"][0]["UTTypeTagSpecification"]
        self.assertEqual(native["public.filename-extension"], ["qwp"])
        for name in ["Quill.icns", "QuillDocument.icns", "quill-about.jpg"]:
            self.assertTrue((contents / "Resources" / name).is_file())

    def test_debug_packages_are_a_different_app_from_the_real_one(self):
        import plistlib
        executable = Path(self.tmp.name) / "quill"
        executable.write_bytes(b"executable")
        info = plistlib.loads((package.ROOT / "tools/Info.plist").read_bytes())
        (self.root / "tools/Info.plist").write_bytes(plistlib.dumps(info))

        def fake_run(*args, **kwargs):
            # hdiutil makes the disk image: its last argument.
            if args and args[0] == "hdiutil" and args[1] == "create":
                Path(args[-1]).write_bytes(b"image")

        with patch.object(package, "ROOT", self.root), patch.object(package, "run", fake_run):
            dmg = package.mac_package(executable, self.output, "1.2.3", "arm64", "-debug", True)
            self.assertEqual(dmg.name, "Quill-1.2.3-macos-arm64-debug.dmg")
            # Beside it the app is Quill Debug, never Quill, and says so.
            self.assertFalse((self.output / "Quill.app").exists())
            app = self.output / "Quill Debug.app"
            info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
            self.assertEqual(info["CFBundleIdentifier"], "com.quillwp.Quill.debug")
            self.assertEqual(info["CFBundleName"], "Quill Debug")
            self.assertNotIn("CFBundleDocumentTypes", info)
            # A release package is still Quill.
            package.mac_package(executable, self.output, "1.2.3", "arm64")
            info = plistlib.loads((self.output / "Quill.app/Contents/Info.plist").read_bytes())
            self.assertEqual(info["CFBundleIdentifier"], "com.quillwp.Quill")
            # A portable debug package registers nothing for Linux.
            archive = package.portable_package(executable, self.output, "1.2.3", "linux", "aarch64",
                                               "-debug", True)
            with tarfile.open(archive) as bundle:
                names = {name.rsplit("/", 1)[-1] for name in bundle.getnames()}
            for name in ["quill.desktop", "install-linux.sh", "quill-mime.xml"]:
                self.assertNotIn(name, names)
            # Its program is quill-debug, so it is not taken for Quill.
            self.assertIn("quill-debug", names)
            self.assertNotIn("quill", names)
            # The release package's program is still quill.
            archive = package.portable_package(executable, self.output, "1.2.3", "linux", "aarch64")
            with tarfile.open(archive) as bundle:
                names = {name.rsplit("/", 1)[-1] for name in bundle.getnames()}
            self.assertIn("quill", names)
            self.assertNotIn("quill-debug", names)


if __name__ == "__main__":
    unittest.main()
