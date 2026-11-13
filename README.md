<p align="center">
  <img src="assets/quill.png" alt="Quill app icon" width="128" height="128">
</p>

<h1 align="center">Quill</h1>

<p align="center">
  A capable word processor for the Mac in a small package.<br>
  Styles, pictures, tables, long-document tools and modern file formats.
</p>

<p align="center"><a href="https://quillwp.com">quillwp.com</a></p>

**Status: alpha · version 0.17.0.** Alpha testers wanted.
**[Download a build](https://github.com/QuillWP/quill/releases)** from the releases page.

![Quill editing a document set in Avenir Next with headings, a bulleted list and a table](docs/images/screenshot.png)

## Small on purpose

Everything in the app, measured from the release build for Apple Silicon Macs:

| Part | Size |
|---|---:|
| The program itself | 5.5 MB |
| Thesaurus data | 1.4 MB |
| Icons and About artwork | 0.9 MB |
| Licenses, notices and readme | 0.2 MB |
| **Whole app** | **8.0 MB** |
| **Download (disk image)** | **5.4 MB** |

Quill is written from scratch in Rust, draws its own pages and uses native Mac
menus and dialogs. It uses the fonts already installed on your Mac.

## What it does

- **Styles that travel.** Named paragraph and character styles, kept in Quill's
  own format and exchanged with DOCX and RTF.
- **A two-way table of contents.** Headings update the contents. Edit an entry
  and Quill can apply the new name to its heading.
- **Pictures and page layout.** Resize, crop and position pictures, with frames,
  borders, shading, sections, columns, headers and footers.
- **Long documents.** Footnotes and endnotes, outline view, tables of contents,
  indexes, hyperlinks and page numbering.
- **Tables.** Nested tables, merged and split cells, pasting into selected cells
  and cell borders you can drag.
- **Review.** Comments with replies and resolution, and tracked changes with
  accept and reject in one Review pane.
- **Writing tools.** Live spelling, custom dictionaries, thesaurus, hyphenation,
  Find and Replace, word count, sorting and print merge.
- **Markdown, two ways.** Open and save Markdown, and switch between editable
  source and a formatted view.
- **Fonts without installing them.** Load fonts from folders you choose without
  adding them to the system Fonts folder.
- **Your choice of format.** Open and save `.qwp`, DOCX, RTF, ODT, HTML,
  Markdown and plain text, and export PDF. Quill warns when a format cannot
  keep everything.

![A Quill document showing character effects, a drop cap, borders, shading, justified text and hyphenation](docs/images/formatting.png)

## Built to protect your work

Quill has multi-level undo, document recovery, atomic saving and read-back
validation: before it replaces a file, it checks that it can open what it just
wrote. Import reports and format warnings explain anything that could not be
kept.

Quill is still alpha software. Keep backups of important documents. The native
`.qwp` format may change during the alpha.

## Why Quill exists

In 1992 a capable Mac word processor fit in under a megabyte and still had
styles, tables, footnotes, pictures and outlines. Quill explores what that idea
looks like on a modern computer: focused word processing, modern document
formats and useful writing tools, in an app measured in megabytes.

## Help test the alpha

Quill is a good fit if you use a Mac with a recent macOS, write real documents
(letters, reports, manuscripts, notes), have DOCX, RTF or OpenDocument files to
try, and are willing to report what breaks. Please
[open an issue](https://github.com/QuillWP/quill/issues) for anything that breaks, changes or looks wrong.

Not yet: screen readers cannot read the document area.

### Installing an alpha build

Quill needs macOS 11 Big Sur or later on an Apple Silicon Mac (M1 or newer).

1. Download the newest disk image from the
   [releases page](https://github.com/QuillWP/quill/releases), open it and drag
   Quill to Applications.
2. Alpha builds are not signed by Apple yet; version 1.0 will be. The first time
   you open Quill, macOS says it cannot check it for malicious software. Click
   Done (or Cancel), open System Settings > Privacy & Security, scroll down to
   the message about Quill and click Open Anyway, then confirm. You do this once
   for each new version. On macOS 14 or earlier you can instead Control-click
   Quill in Applications and choose Open.

If you would rather build Quill yourself, follow the
[build instructions for the Mac](BUILD_INSTRUCTIONS_MAC.md).

## Building

Step-by-step instructions, including installing the tools, are in
[BUILD_INSTRUCTIONS_MAC.md](BUILD_INSTRUCTIONS_MAC.md). In short, you need the
Xcode Command Line Tools, Rust and Python 3.11 or newer.

```sh
python3 tools/package.py            # target/release/Quill.app on the Mac
python3 tools/package.py --debug    # debug build (Linux: target/debug/quill-debug)
python3 tools/package.py --check    # fmt, clippy and the full test suite first
cargo test --workspace
```

See the [user guide](docs/user-guide.md) and the
[release instructions](docs/release.md) for more.

## License

Available under either the [MIT license](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. Packaged builds include
[third-party notices](THIRD-PARTY-NOTICES.txt).
