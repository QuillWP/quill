# Working on Quill

Quill is a word processor written in Rust. See README.md for what it is,
docs/user-guide.md for how it behaves and docs/release.md for releases.

## Layout

- `crates/quill-core`: document model, undo, limits.
- `crates/quill-formats`: readers and writers (.qwp, .docx, RTF, ODT, HTML,
  Markdown, plain text).
- `crates/quill-layout`: fonts and text layout.
- `crates/quill-app`: window, menus, dialogs and drawing.
- `tools/`: packaging and helper scripts.
- `docs/`: guides and notes.

## Build and check

```sh
python3 tools/package.py --debug   # builds target/debug/Quill.app
cargo fmt --all
cargo clippy -p quill-core -p quill-layout -p quill-app -- -D warnings
cargo test --workspace
```

Quit any running Quill before running `package.py`. Do not run `cargo clean`
without asking. macOS is the primary target; Windows and Linux come second.

## Rules

- `vendor/winit` is a patched copy. Read `vendor/README.md` before touching
  it or upgrading winit.
- Match the code around you. Comments say what the code does and why, plainly.
- Every format shares the size limits in `quill_core::limits`. What Quill
  opens it must be able to save and reopen.
- A dialog must change only what the person changed. Read length fields with
  `units::parse_length_from(text, old)`, since the shown text is rounded.
- No migration or compatibility code for earlier alpha builds.
- Test with copies of documents, and run test copies of Quill with their own
  `HOME` so they do not touch anyone's settings or recovery files.
