# Known issues

Quill is beta software. Keep backups of important documents. This list is
what is missing or different today, so you do not have to find it yourself.

## Platforms

- The published builds are for Macs with macOS 11 or later. The Intel builds
  are made on an Apple Silicon Mac and have not been tested on Intel hardware.
  There is no universal build.
- Windows and Linux builds exist in the source but are not supported. The
  Windows build is unreliable and the Linux build is untested.
- Beta builds are not signed or notarized. See the README for the one-time
  steps to open one.
- The regular Mac app runs in the App Sandbox (a build from source or the
  debug package does not). It arrived in 0.30.0, so settings saved by
  0.25.1 and earlier, which ran outside it, are not carried over. It asks once for the folder of a Markdown file with pictures,
  or of a Print Merge data document. Pictures a Markdown file names in other
  folders are not loaded.

## Not there yet

- Saving a document as Markdown, and the Markdown Source and WYSIWYG views, are
  on hold: no menu offers them. Markdown files still open, and one you opened
  still saves as itself.
- Accessibility: VoiceOver support on the Mac is experimental. Only the main
  document text is exposed, and it has not been tested by people who use
  VoiceOver every day. Reports from VoiceOver users are welcome. Voice
  Control and other accessibility tools see the text only while VoiceOver
  is running.
- Text shaping: letters are placed one at a time with kerning. Standard
  ligatures (fi, fl, ff) are an experimental option in Preferences.
  Right-to-left and complex scripts (Arabic, Hebrew, Indic) do not display
  correctly.
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
- The `.qwp` format may still change during the beta.

## Different from other Mac apps

Some Quill keys differ from other Mac apps: Command-Left and Right move by
word; Command-G is Go To; and Command-M opens the Paragraph dialog. Plain Text
has no shortcut, and keys cannot be rebound yet.

## Reporting a problem

See [CONTRIBUTING.md](CONTRIBUTING.md).
