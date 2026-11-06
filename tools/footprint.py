#!/usr/bin/env python3
"""Quill's footprint report: binary, data and package sizes, and peak memory
drawing a document headless, against the budget below and the
last recorded baseline.

Usage:
    python3 tools/footprint.py            # print the report
    python3 tools/footprint.py --check    # also fail on going over
    python3 tools/footprint.py --record   # save this run as the baseline

Run after `cargo build --release` (and tools/package.sh for the package
size). In CI the report also goes to the job summary.
"""

import glob
import json
import os
import platform
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINE = os.path.join(ROOT, "docs", "footprint.json")
MB = 1024 * 1024

# The budget: stay well under 10 MB for the program and for the download.
BUDGET = {
    "binary": 10 * MB,
    "package": 10 * MB,
}
# How much a measure may grow over the baseline before --check fails.
GROWTH = 0.10


def size_of(path):
    if os.path.isfile(path):
        return os.path.getsize(path)
    total = 0
    for dirpath, _, files in os.walk(path):
        for f in files:
            total += os.path.getsize(os.path.join(dirpath, f))
    return total


def binary_path():
    exe = "quill.exe" if platform.system() == "Windows" else "quill"
    return os.path.join(ROOT, "target", "release", exe)


def package_size():
    found = []
    for pattern in ("*.dmg", "*.tar.gz", "*.zip"):
        found += glob.glob(os.path.join(ROOT, "dist", pattern))
    # Debug packages are far larger and are not what ships.
    found = [p for p in found if "-debug" not in os.path.basename(p)]
    return max((os.path.getsize(p) for p in found), default=None)


def peak_memory():
    """Peak resident memory drawing a typed page headless, in bytes, or
    None where it cannot be measured."""
    out = os.path.join(tempfile.mkdtemp(), "footprint.png")
    text = "The quick brown fox jumps over the lazy dog. " * 40
    cmd = [binary_path(), "--snapshot", out, "--size", "720x820", "--script", "type:" + text]
    system = platform.system()
    if system == "Darwin":
        r = subprocess.run(["/usr/bin/time", "-l"] + cmd, capture_output=True, text=True)
        m = re.search(r"(\d+)\s+maximum resident set size", r.stderr)
        return int(m.group(1)) if m else None
    if system == "Linux" and os.path.exists("/usr/bin/time"):
        r = subprocess.run(["/usr/bin/time", "-v"] + cmd, capture_output=True, text=True)
        m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", r.stderr)
        return int(m.group(1)) * 1024 if m else None
    return None


def fmt(n):
    return "-" if n is None else f"{n / MB:.2f} MB"


def main():
    check = "--check" in sys.argv
    record = "--record" in sys.argv
    key = f"{platform.system()}-{platform.machine()}"
    now = {
        "binary": size_of(binary_path()),
        "data": size_of(os.path.join(ROOT, "data")),
        "package": package_size(),
        "memory": peak_memory(),
    }
    try:
        with open(BASELINE) as f:
            baselines = json.load(f)
    except (OSError, ValueError):
        baselines = {}
    base = baselines.get(key, {})

    lines = [
        f"### Quill footprint ({key})",
        "",
        "| Measure | Now | Baseline | Budget |",
        "|---|---|---|---|",
    ]
    names = {
        "binary": "Binary",
        "data": "Language data",
        "package": "Package",
        "memory": "Peak memory, headless page",
    }
    problems = []
    for k, label in names.items():
        lines.append(f"| {label} | {fmt(now[k])} | {fmt(base.get(k))} | {fmt(BUDGET.get(k))} |")
        if now[k] is None:
            continue
        if k in BUDGET and now[k] > BUDGET[k]:
            problems.append(f"{label} is over budget: {fmt(now[k])} > {fmt(BUDGET[k])}")
        # Memory depends on the machine (fonts installed, display), so it is
        # reported but not held to the baseline.
        if k != "memory" and base.get(k) and now[k] > base[k] * (1 + GROWTH):
            problems.append(f"{label} grew more than {GROWTH:.0%}: {fmt(base[k])} to {fmt(now[k])}")
    report = "\n".join(lines) + "\n"
    if problems:
        report += "\n" + "\n".join(f"- {p}" for p in problems) + "\n"
    print(report)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write(report)
    if record:
        baselines[key] = {k: v for k, v in now.items() if v is not None}
        with open(BASELINE, "w") as f:
            json.dump(baselines, f, indent=2, sort_keys=True)
            f.write("\n")
    if check and problems:
        sys.exit(1)


if __name__ == "__main__":
    main()
