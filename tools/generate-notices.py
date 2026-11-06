#!/usr/bin/env python3
"""Generate deduplicated third-party notices from Cargo's locked sources."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "THIRD-PARTY-NOTICES.txt"
NOTICE_NAMES = ("license", "copying", "unlicense", "notice")
DATA_NOTICES = (
    ("Thesaurus data (WordNet, Princeton University)", "WordNet_license.txt"),
    ("Hyphenation patterns (hyph_en_US)", "README_hyph_en_US.txt"),
)


def metadata(target: str | None) -> dict:
    platform = ["--filter-platform", target] if target else []
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", *platform],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def shipped(data: dict) -> set[str]:
    """The ids of the packages quill-app builds with, leaving out test-only ones."""
    nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
    app = next(package["id"] for package in data["packages"] if package["name"] == "quill-app")
    found = set()
    waiting = [app]
    while waiting:
        node = waiting.pop()
        if node in found:
            continue
        found.add(node)
        for dep in nodes[node]["deps"]:
            if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]):
                waiting.append(dep["pkg"])
    return found


def notice_files(package: dict) -> list[pathlib.Path]:
    root = pathlib.Path(package["manifest_path"]).parent
    paths = [
        path
        for path in root.iterdir()
        if path.is_file()
        and path.name.lower().startswith(NOTICE_NAMES)
        and path.stat().st_size <= 1024 * 1024
    ]
    declared = package.get("license_file")
    if declared:
        path = root / declared
        if path.is_file() and path not in paths:
            paths.append(path)
    return sorted(paths, key=lambda path: path.name.lower())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", help="list only the crates built for this target triple")
    parser.add_argument("--output", type=pathlib.Path, default=OUTPUT)
    args = parser.parse_args()
    data = metadata(args.target)
    ids = shipped(data) if args.target else None
    packages = sorted(
        # Crates from crates.io, and the patched copies in vendor/.
        (
            package
            for package in data["packages"]
            if (ids is None or package["id"] in ids)
            and (package.get("source")
                 or pathlib.Path(package["manifest_path"]).is_relative_to(ROOT / "vendor"))
        ),
        key=lambda package: (package["name"].lower(), package["version"]),
    )
    documents: dict[str, tuple[str, list[str]]] = {}
    package_documents = []
    manifest = []
    for package in packages:
        label = f'{package["name"]} {package["version"]}'
        license_expression = package.get("license") or "NOASSERTION"
        authors = ", ".join(package.get("authors") or []) or "not listed"
        manifest.append(f"- {label}: {license_expression}; authors: {authors}")
        paths = notice_files(package)
        package_notices = []
        for path in paths:
            text = path.read_text(encoding="utf-8", errors="replace").strip()
            if not text:
                continue
            digest = hashlib.sha256(text.encode()).hexdigest()
            if digest not in documents:
                documents[digest] = (text, [])
            documents[digest][1].append(f"{label} ({path.name})")
            package_notices.append((digest, path.name))
        package_documents.append((label, license_expression, package_notices))

    # Some crates intentionally omit a duplicate copy of a standard license
    # from their published archive.  Reuse a verified copy from another crate
    # with the same declared license, while retaining the package's authors in
    # the manifest above.  For OR expressions, including one permitted license
    # is sufficient; AND expressions must provide their own complete texts.
    representatives: dict[str, str] = {}
    choices = {
        "MIT": ("mit",),
        "Apache-2.0": ("apache",),
        "BSL-1.0": ("bsl", "boost"),
        "Zlib": ("zlib",),
        "ISC": ("isc",),
        "Unlicense": ("unlicense",),
    }
    for _, expression, notices in package_documents:
        for digest, filename in notices:
            lowered = filename.lower()
            for choice, hints in choices.items():
                if choice in expression and any(hint in lowered for hint in hints):
                    representatives.setdefault(choice, digest)
    # A crate declaring only one license may call its text simply `LICENSE`.
    for _, expression, notices in package_documents:
        if expression in choices and notices:
            representatives.setdefault(expression, notices[0][0])

    missing = []
    for label, expression, notices in package_documents:
        if notices:
            continue
        if " AND " in expression:
            missing.append(f"{label} ({expression})")
            continue
        choice = next(
            (
                candidate
                for candidate in choices
                if candidate in expression and candidate in representatives
            ),
            None,
        )
        if choice is None:
            missing.append(f"{label} ({expression})")
            continue
        digest = representatives[choice]
        documents[digest][1].append(
            f"{label} (declares {expression}; using common {choice} text)"
        )

    if missing:
        print("could not resolve license text for: " + ", ".join(missing), file=sys.stderr)
        return 1

    lines = [
        "Quill third-party notices",
        "========================",
        "",
        "Generated from Cargo.lock by tools/generate-notices.py"
        + (f" for {args.target}." if args.target else "."),
        "The package list records each crate's declared SPDX expression; the",
        "deduplicated source license and notice files follow it.",
        "",
        "Packages",
        "--------",
        *manifest,
    ]
    for index, (_, (text, users)) in enumerate(sorted(documents.items()), 1):
        lines.extend(
            [
                "",
                f"License document {index}",
                "-" * (17 + len(str(index))),
                "Used by: " + ", ".join(users),
                "",
                text,
            ]
        )
    # The language data in data/ is not Cargo-sourced; its license texts live
    # in data/licenses and are repeated here so every package carries them.
    for title, name in DATA_NOTICES:
        # The Mac hyphenates with the system's patterns, so its package leaves
        # Quill's copy out (see copy_common in package.py).
        if args.target and "apple" in args.target and "hyph" in name:
            continue
        text = (ROOT / "data" / "licenses" / name).read_text(encoding="utf-8")
        lines.extend(
            ["", title, "-" * len(title), f"Source: data/licenses/{name}", "", text.strip()]
        )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {args.output} ({len(packages)} packages, {len(documents)} documents)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
