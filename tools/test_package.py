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
                     "fuzz/fuzz_targets/example.rs", "fuzz/Cargo.toml"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")
        self.output = self.root / "dist"
        self.output.mkdir()

    def test_source_keeps_images_data_and_working_files_but_excludes_builds(self):
        for name in ["target/huge.bin", "crates/target/huge.bin", "fuzz/target/huge.bin",
                     "fuzz/corpus/input", "fuzz/artifacts/crash", "Quill.app/generated",
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
                         "fuzz/fuzz_targets/example.rs"]:
                self.assertIn("Quill/" + name, names)
            self.assertFalse(any("huge.bin" in name or "corpus/" in name or ".git/" in name
                                 or ".DS_Store" in name or "__pycache__" in name for name in names))
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
                for name in [binary, "quill.png", "data/language.dat", "data/licenses/notice.txt",
                             "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.txt", "README.md"]:
                    self.assertIn(name, names)


if __name__ == "__main__":
    unittest.main()
