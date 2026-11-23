# Releasing Quill

A checklist for each release. Work down it in order; every box is a thing
to check by hand. The workflows in `.github/workflows` are not relied on
today (the Mac runners cannot build Quill yet, and the Windows and Linux
builds are not trustworthy enough to gate a release), so the checks below are
run on the maintainer's own machines.

On macOS, run the tests with access to the desktop's native services. A restricted
runner can deny the spelling service and substitute printer paper dimensions;
those host checks must also pass in a normal desktop session. Editing tests use
a private clipboard so they do not replace the person's copied content.

## Before tagging

Use Python 3.11+ and run `python3 tools/package.py --dist --check --with-source`
on macOS/Linux, or `py -3 tools/package.py --dist --check --with-source` in Windows
PowerShell. Packages are built on their matching operating system and written to
`dist/`: macOS `.dmg` (plus `Quill.app`), Linux `.tar.gz`, Windows `.zip`, and one
portable source `.zip`. Other packages in `dist/` are retained. The source archive
contains current working-tree files, artwork, test images and language data;
build caches, generated fuzz corpora and compiled packages are excluded.
`tools/package.sh` remains a compatibility wrapper for existing commands.

The Release workflow can build artifacts through GitHub Actions, but releases are
currently built and published by hand from the maintainer's Mac.

- [ ] `python3 tools/package.py --check` passes on the Mac (format, clippy with
      `--all-targets -D warnings`, and the whole test suite). Run
      `cargo check --target x86_64-pc-windows-msvc -p quill-app` as well.
- [ ] `python3 tools/footprint.py --check` passes on each system: binary
      under 10 MB, package under 10 MB, no measure more than 10% over the
      baseline in docs/footprint.json. Record a new baseline with
      `--record` only when the growth is understood and worth it.
- [ ] The latest overnight run (`tests/overnight/runs/latest.txt`) is on the
      commit being released, and its report shows nothing worse and no
      failures, or each is understood.
- [ ] The version in the workspace Cargo.toml is the one being released.
- [ ] The README's status line says what works.
- [ ] License files: the crates say "MIT OR Apache-2.0"; LICENSE-MIT and
      LICENSE-APACHE must be in the repository and in each package.
- [ ] data/README.md still matches the hyphenation and thesaurus data
      (sources, licenses);
      the files in data/licenses ship with every package (they do, as part
      of data/).
- [ ] No artwork copied from the original product, such as pixel copies of
      its toolbar icons, is in the repository; the icons are clearly Quill's
      own.
- [ ] The bundle identifier in tools/Info.plist (com.quillwp.Quill) is the one
      to keep: changing it later loses users' settings on the Mac.
- [ ] For a public macOS build, import the Developer ID Application
      certificate and set `MACOS_SIGN_IDENTITY` to its full identity name.
      Store notarization credentials with `xcrun notarytool store-credentials`,
      then set `MACOS_NOTARY_PROFILE` to that keychain profile. The packaging
      script signs the app, submits the DMG, and staples the accepted ticket.
      Without those variables it produces a completely ad-hoc-signed local
      build, suitable for testing but not for distribution through Gatekeeper.

## Each package

- [ ] macOS (.dmg): double-clicking a .docx in the Finder opens it in
      Quill, whether Quill is running or not (QUILL_OPEN_LOG=<file> records
      each document opened, for checking).
- [ ] macOS (.dmg): the app opens from /Applications, finds its language
      data (Tools > Spelling works), opens and saves .docx, .rtf and .txt,
      prints. The release package runs in the App Sandbox (the debug package does
      not; its settings are in `~/Library/Preferences`): `codesign -d --entitlements -
      Quill.app` shows `com.apple.security.app-sandbox`, and its settings
      appear under `~/Library/Containers/com.quillwp.Quill`. Saving makes a
      `.qwp-backup` beside the document when backups are on, and a Markdown
      file with pictures asks once for its folder. Confirm `codesign --verify --deep --strict Quill.app`,
      `spctl --assess --type execute Quill.app`, and `xcrun stapler validate`
      all succeed for the public package.
- [ ] Windows (.zip): unzip anywhere, run quill.exe, Spelling works, Open
      and Save dialogs appear, printing hands a PDF to the default handler.
- [ ] Linux (.tar.gz): runs on X11 and Wayland (on a Wayland desktop it
      opens through XWayland, with the theme's title bar and GTK's menu bar;
      also try `QUILL_BACKEND=wayland`), Spelling works, printing
      goes through lp. The debug package's program is quill-debug. Run install-linux.sh from the unpacked folder to get
      Quill in the application menu with its icon (Wayland shows no icon
      without it).
- [ ] Each opens a .docx made by current Microsoft Word, and Word opens
      what Quill saves.

## Publishing several tags at once

When quill-review has tags that have not reached the public repository,
publish them oldest first, one commit per tag, built from the tag's own tree
(`git archive vX.Y.Z`), never from the working tree. For each: set the
workspace version and the README status line to that version, add its section
to `VERSION-HISTORY.md` headed with the date of the tag commit in
quill-review, run the checks above, build the release and debug packages
(`--dist` and `--dist --debug`; each makes an Apple Silicon and an Intel
disk image) and keep all the disk images of every version in `dist/`, check
the Apple Silicon app is still under the 8 MB the README claims, and commit and tag. Review
all the commits together before pushing, then push, and make one GitHub
release per tag with all the disk images and that version's history section.
State in the release which packages were built (Mac only, the Intel ones built
but not tested on Intel hardware, ad hoc signed unless the signing variables
were set) and which are debug.

## Tagging

- [ ] Tag the release (`git tag vX.Y.Z && git push --tags`) and create the
      GitHub release by hand, attaching the Mac disk images.
- [ ] Write the release notes (what is new, known gaps), check
      the attached packages download and open, then publish the draft.
