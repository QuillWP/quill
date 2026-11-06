# Releasing Quill

A checklist for each release. Work down it in order; every box is a thing
to check by hand, since CI only proves that the code builds and passes its
tests.

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

The Release workflow also supports Run workflow for building downloadable artifacts
for all three platforms through GitHub Actions. Only tag pushes create a draft release.

- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`
      and `cargo test` pass on macOS, Linux and Windows (the CI workflow).
- [ ] `python3 tools/footprint.py --check` passes on each system: binary
      under 10 MB, package under 10 MB, no measure more than 10% over the
      baseline in docs/footprint.json. Record a new baseline with
      `--record` only when the growth is understood and worth it.
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
      prints. Confirm `codesign --verify --deep --strict Quill.app`,
      `spctl --assess --type execute Quill.app`, and `xcrun stapler validate`
      all succeed for the public package.
- [ ] Windows (.zip): unzip anywhere, run quill.exe, Spelling works, Open
      and Save dialogs appear, printing hands a PDF to the default handler.
- [ ] Linux (.tar.gz): runs on X11 and Wayland, Spelling works, printing
      goes through lp. Run install-linux.sh from the unpacked folder to get
      Quill in the application menu with its icon (Wayland shows no icon
      without it).
- [ ] Each opens a .docx made by current Microsoft Word, and Word opens
      what Quill saves.

## Tagging

- [ ] `git tag vX.Y.Z && git push --tags` runs the Release workflow, which
      attaches the three packages to a draft GitHub release.
- [ ] Write the release notes (what is new, known gaps), check
      the attached packages download and open, then publish the draft.
