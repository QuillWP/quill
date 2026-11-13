#!/usr/bin/env python3
"""Quick smoke test of a built Quill, for dev (quill-review) and prod (quill).

    python3 tools/smoketest.py                     # finds a build in this repo
    python3 tools/smoketest.py --app "dist/Quill.app"
    python3 tools/smoketest.py --app target/debug/quill --launch

It runs the app with its own scratch HOME (and APPDATA and XDG folders, and a
profile name of its own), so settings, recovery files and crash logs never
touch anyone's real ones, and it only opens documents it writes itself. It checks that the program starts, reports its version, draws
a blank page, draws typed text, and opens and draws one small document in each
of these formats: plain text, Markdown, HTML, RTF and DOCX. A panic anywhere
shows up as a crash log in the scratch HOME and fails the run.

--launch also starts the real window for a few seconds (it appears on screen),
then ends it. Leave it off when you do not want a window to open.

Exits 0 when every check passes, 1 otherwise. Needs Python 3.11 or later, as
package.py does.
"""

import argparse
import os
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# The profile the test copy keeps its files under, so even a copy that
# somehow found a real settings folder would not read or write its files.
PROFILE = "Quill Smoketest"

SAMPLE_TEXT = "Smoke test heading\n\nThe quick brown fox jumps over the lazy dog.\n"

SAMPLES = {
    "sample.txt": SAMPLE_TEXT,
    "sample.md": "# Smoke test heading\n\nThe quick **brown** fox jumps over the "
    "lazy dog.\n\n- one\n- two\n",
    "sample.html": "<html><body><h1>Smoke test heading</h1><p>The quick "
    "<b>brown</b> fox jumps over the lazy dog.</p></body></html>",
    "sample.rtf": r"{\rtf1\ansi\deff0{\fonttbl{\f0 Times New Roman;}}"
    r"\f0\fs24 Smoke test heading\par The quick {\b brown} fox jumps over "
    r"the lazy dog.\par}",
}

DOCX_PARTS = {
    "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    "</Types>",
    "_rels/.rels": '<?xml version="1.0" encoding="UTF-8"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    "</Relationships>",
    "word/document.xml": '<?xml version="1.0" encoding="UTF-8"?>'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
    "<w:p><w:r><w:t>Smoke test heading</w:t></w:r></w:p>"
    "<w:p><w:r><w:t>The quick </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>brown</w:t></w:r>"
    "<w:r><w:t> fox jumps over the lazy dog.</w:t></w:r></w:p>"
    "</w:body></w:document>",
}


class Results:
    def __init__(self):
        self.failed = 0

    def check(self, name, ok, detail=""):
        print(f"  {'ok  ' if ok else 'FAIL'} {name}" + (f": {detail}" if detail and not ok else ""))
        if not ok:
            self.failed += 1
        return ok


def find_binary(app):
    """The program an .app bundle names in its Info.plist, or the path itself."""
    if not app.endswith(".app"):
        return app
    try:
        with open(os.path.join(app, "Contents", "Info.plist"), "rb") as f:
            name = plistlib.load(f)["CFBundleExecutable"]
    except (OSError, KeyError, plistlib.InvalidFileException) as e:
        sys.exit(f"smoketest: cannot read the program name from {app}: {e}")
    binary = os.path.join(app, "Contents", "MacOS", name)
    if not os.path.isfile(binary):
        sys.exit(f"smoketest: {app} names {name}, which is not there")
    return binary


def default_app():
    """Look for a build in this repository, newest first."""
    candidates = [
        "dist/Quill.app",
        "dist/Quill Debug.app",
        "target/release/Quill.app",
        "target/debug/Quill Debug.app",
        "target/release/quill",
        "target/debug/quill",
    ]
    found = [os.path.join(ROOT, c) for c in candidates if os.path.exists(os.path.join(ROOT, c))]
    if not found:
        sys.exit("smoketest: no build found; pass --app PATH")
    return max(found, key=os.path.getmtime)


def repo_version():
    """The workspace version in Cargo.toml, read the way package.py reads it."""
    try:
        with open(os.path.join(ROOT, "Cargo.toml"), "rb") as f:
            return tomllib.load(f)["workspace"]["package"]["version"]
    except (OSError, KeyError, tomllib.TOMLDecodeError):
        return None


def scratch_env(home):
    """The environment for the test copy: every folder Quill keeps files in
    points into the scratch HOME, and the profile is the smoke test's own."""
    env = dict(os.environ)
    env.update(
        HOME=home,
        QUILL_PROFILE=PROFILE,
        XDG_CONFIG_HOME=os.path.join(home, ".config"),
        XDG_DATA_HOME=os.path.join(home, ".local", "share"),
        XDG_CACHE_HOME=os.path.join(home, ".cache"),
        XDG_STATE_HOME=os.path.join(home, ".local", "state"),
    )
    if os.name == "nt":
        env.update(
            USERPROFILE=home,
            APPDATA=os.path.join(home, "AppData", "Roaming"),
            LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
        )
    for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME",
                "APPDATA", "LOCALAPPDATA"):
        if key in env and env[key].startswith(home):
            os.makedirs(env[key], exist_ok=True)
    return env


def crash_logs(home):
    """Crash logs anywhere under the scratch HOME, by the name each system
    gives them ("<profile> Crash Log" on the Mac, "Quill Crash Log.txt" on
    Windows, "crash-log" elsewhere)."""
    found = []
    for dirpath, _, files in os.walk(home):
        found += [os.path.join(dirpath, f) for f in files
                  if "Crash Log" in f or f == "crash-log"]
    return found


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        return None
    return struct.unpack(">II", head[16:24])


def write_samples(folder):
    paths = []
    for name, text in SAMPLES.items():
        path = os.path.join(folder, name)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        paths.append(path)
    path = os.path.join(folder, "sample.docx")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for part, text in DOCX_PARTS.items():
            z.writestr(part, text)
    paths.append(path)
    return paths


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--app", help="a Quill .app bundle or the bare quill program")
    ap.add_argument("--launch", action="store_true", help="also open the real window for a few seconds")
    args = ap.parse_args()

    app = os.path.abspath(args.app) if args.app else default_app()
    if not os.path.exists(app):
        sys.exit(f"smoketest: {app} does not exist")
    binary = find_binary(app)
    if not os.access(binary, os.X_OK):
        sys.exit(f"smoketest: cannot run {binary}")
    print(f"Testing {app}")

    scratch = tempfile.mkdtemp(prefix="quill-smoketest-")
    home = os.path.join(scratch, "home")
    docs = os.path.join(scratch, "docs")
    out = os.path.join(scratch, "out")
    for d in (home, docs, out):
        os.makedirs(d)
    env = scratch_env(home)
    print(f"Scratch HOME: {home}")
    r = Results()

    def run(extra, timeout=60):
        return subprocess.run([binary] + extra, env=env, capture_output=True, text=True, timeout=timeout)

    def snapshot(name, extra=()):
        png = os.path.join(out, name + ".png")
        try:
            p = run(["--snapshot", png, "--size", "720x820", "--scale", "1"] + list(extra))
        except subprocess.TimeoutExpired:
            return None, "timed out"
        if p.returncode != 0:
            return None, (p.stderr.strip().splitlines() or [f"exit {p.returncode}"])[-1]
        if not os.path.exists(png) or png_size(png) is None:
            return None, "no valid PNG written"
        return png, ""

    print("Start and version")
    p = run(["--version"])
    shown = p.stdout.strip()
    r.check("--version runs", p.returncode == 0 and shown.startswith("Quill "), shown or p.stderr)
    want = repo_version()
    # A build from another folder may be another version, so only compare
    # when the program sits inside this repository.
    if want and app.startswith(ROOT):
        r.check(f"version is {want}", shown == f"Quill {want}", shown)

    print("Drawing")
    blank, why = snapshot("blank")
    r.check("blank page draws", blank is not None, why)
    if blank:
        r.check("page is 720x820", png_size(blank) == (720, 820), str(png_size(blank)))
    typed, why = snapshot("typed", ["--script", "type:" + SAMPLE_TEXT.replace("\n", " ")])
    r.check("typed text draws", typed is not None, why)
    if blank and typed:
        same = open(blank, "rb").read() == open(typed, "rb").read()
        r.check("typed text changes the page", not same)

    print("Opening documents")
    for path in write_samples(docs):
        name = os.path.basename(path)
        png, why = snapshot(name, ["--open", path])
        r.check(f"{name} opens and draws", png is not None, why)
        if blank and png:
            same = open(blank, "rb").read() == open(png, "rb").read()
            r.check(f"{name} shows its text", not same)

    if args.launch:
        print("Real window")
        start = os.path.join(docs, "sample.txt")
        proc = subprocess.Popen([binary, start], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        time.sleep(4)
        alive = proc.poll() is None
        r.check("window stays up for 4 seconds", alive, f"exit {proc.returncode}")
        if alive:
            # terminate() is SIGTERM where there are signals and
            # TerminateProcess on Windows, which has no SIGTERM to send.
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()

    print("Crashes")
    crashes = crash_logs(home)
    r.check("no crash log in the scratch HOME", not crashes, ", ".join(crashes))
    for c in crashes:
        print(open(c, errors="replace").read()[-800:])

    if r.failed:
        print(f"\n{r.failed} check(s) failed. Scratch files kept in {scratch}")
        sys.exit(1)
    shutil.rmtree(scratch, ignore_errors=True)
    print("\nAll checks passed.")


if __name__ == "__main__":
    main()
