# Known issues

Quill is alpha software. Keep backups of important documents. This list is
what is missing or different today, so you do not have to find it yourself.

## Platforms

- The published builds are for Apple Silicon Macs (macOS 11 or later) only.
  There is no Intel or universal build.
- Windows and Linux builds exist in the source but are not supported. The
  Windows build is unreliable and the Linux build is untested.
- Alpha builds are not signed or notarized. See the README for the one-time
  steps to open one.

## Not there yet

- Accessibility: screen readers (VoiceOver) cannot read the document area.
- Text shaping: letters are placed one at a time with kerning. There are no
  ligatures, and right-to-left and complex scripts (Arabic, Hebrew, Indic) do
  not display correctly.
- Grammar checking (Tools > Grammar is always disabled) and Use As Default in
  the Document, Section and Style dialogs.
- Spelling follows the system. The thesaurus (and hyphenation off the Mac) are
  US English only. The interface is English only.
- ODT and HTML saves are basic: no headers, footers, columns, frames or
  cropping. Saving to them, or to text or Markdown, keeps what the format can
  hold and the rest is gone once you close the window.
- Some RTF features are dropped on import: shapes and text boxes, highlight,
  bookmarks, nested tables (flattened) and double-byte code pages. Old `.doc`
  files are refused.
- The `.qwp` format may change during the alpha.

## Different from other Mac apps

Quill follows its model word processor from 1992, so some keys differ (Command-Shift-Z
is Redo, and Plain Text has no key):
Command-Left and Right move by word, Command-G is Go To, Command-Backspace
inserts a glossary entry and Command-M opens the Paragraph dialog.
Keys cannot be rebound yet.

## Reporting a problem

See [CONTRIBUTING.md](CONTRIBUTING.md).
