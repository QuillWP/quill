# Version history

What changed at each tagged version. Each tag is one squashed commit, so this
file is the only place the per-version story is kept in readable form. It was
rebuilt from the tagged commit messages and the diff between each pair of tags.
Add an entry for every new tag as part of the release process.

Versions 0.1.0 to 0.14.0 were all tagged on 2026-11-05.

## 0.20.0 - 2026-11-12

Hang fixes for Windows and Linux dialogs, and a step-by-step event log for
finding hangs.

- quill --with-events writes each step to quill-events.log in the folder Quill
  was started from (or --events-file=PATH, or QUILL_EVENTS_FILE), falling back
  to the settings folder. --with-events=all adds mouse moves, redraws and
  every wake of the event loop.
- It logs startup, window events, key presses and their commands, menu
  actions, dialog outcomes (a begin with no end means it never came back),
  file reads and writes and panics, and the build commit. A watchdog adds a
  STUCK line every two seconds while the program is busy and has logged
  nothing, and a line repeated in a row is shown three times and then counted.
- Open, Save and message dialogs are GTK dialogs on the main thread. rfd ran
  them on a thread of its own, which froze Quill on Ubuntu 24.04. A finished
  dialog is hidden, destroyed and its pending events run so it releases its
  keyboard and mouse grab.
- Save As lets you pick the file type: choosing a filter changes the name's
  extension, as the Mac's save panel does.
- Message boxes go through TaskDialogIndirect owned by Quill's window, with
  their real button labels, and run on a thread of their own. File dialogs run
  on a thread of their own too. Run on the main thread inside winit's event
  handler, they never appeared or hung on the second use.

## 0.15.0 - 2026-11-08

Saving safety, pattern tools, Markdown editing, page breaks and many fixes
from review and fuzzing.

- Naming: the app calls itself Quill everywhere (About box, Info.plist,
  document types, Linux desktop entry and mime type, README and guides), not
  Quill Word Processor. The About box credits Lucas Krupinski and Quill
  contributors.
- Debug builds are a separate app: Quill Debug.app with its own bundle id, no
  document type claims and its own settings, recovery and crash log files
  (QUILL_PROFILE overrides the profile). A debug distribution package gets its
  own bundle, disk volume and place in dist/, and a debug Linux package
  registers nothing.
- Edit > Advanced adds Grep, Regex Search and Sed, using a trimmed regex crate
  (about 440 KB; docs/regex-size.md says what each feature costs). Grep lists
  matching paragraphs and jumps to them, Regex Search works like the Find
  window, and Sed runs s commands with addresses on the document or the
  selection. Each Replace All or Sed run is one Undo step. Follow-up fixes: a
  script that would make the document too big to save changes nothing,
  tracked deleted text is skipped, matches are numbered within the selection,
  Replace All skips empty matches, a refused command leaves Undo and Redo as
  they were, search over many tracked deletions is fast, and Grep searches
  again when its list is older than the document.
- Saving safety: a .qwp save is read back and compared part by part. If it
  differs, Quill says which paragraph and what changed, selects it, and offers
  Save Anyway, which saves, keeps the old file as a backup and makes the window
  match the file in one undoable step. A saved file keeps its permissions when
  it is replaced, and the temporary file is private while it exists.
- Files and limits: .qwp files whose stories nest without end are refused
  instead of overflowing the stack. A document holds at most 4,000 picture
  files and 4,800 header and footer stories, so it can always be saved and
  opened again. Pictures in footnotes and comments are kept when saving .docx.
- Markdown: autosave and crash recovery keep the formatted document while
  Source view is showing, and edited source is recovered as typed. In Source
  view, Paste puts in text only, in the source's font, and formatting commands,
  bars and the ruler are disabled. Pasting into a formatted Markdown file is
  text only too. Return continues numbered items, ends a list on an empty
  item, and takes an empty quoted paragraph out of its quote.
- Fonts and styles: Preferences previews each font in its own face and has a
  Style pop-up for the Default Font's face, which is now saved and used as
  Normal's style in new documents. The panel is called Fonts, and it also now
  saves the default zoom and superscript and subscript sizes. A Color pop-up
  joins the formatting toolbar. Automatic style names show in title case
  (Heading 1, TOC 2, Footnote Text). Revert to Style keeps links, tracked
  changes, comments and table membership.
- Page breaks sit on their own line, can be selected, and their highlight
  stays in their own column or cell. A document that ends in a page break
  shows the page after it. A paragraph that ends in a line break gets an empty
  line after it for the caret.
- Frames and boxes: framed paragraphs are laid out at the frame's width,
  frames measured from their paragraph are placed there, frames are drawn
  whole on the page they start on, and boxes close at page breaks.
- Document-wide changes: Delete Style, Redefine Style, Replace Fonts and
  Replace Color reach every story, including each section's own headers, once
  each and inside Undo.
- Copy and paste: notes and comment replies stay with their copies, pasted
  note ids no longer collide, table cells bring their own notes, and nested
  tables stay whole when cells are pasted.
- Layout: headers and footers are laid out again when the page width, page
  number format or other inputs change. Table rows and merged cells in view
  always have their glyphs. A paragraph can hold any number of fields and
  looks.
- Fixes: Print Merge no longer panics on field names that change length in
  capitals, note ids are safe when a file carries the largest ids, the
  Paragraph dialog keeps a Multiple line spacing it did not change,
  Find/Replace reads font sizes as the Character dialog does, bare modifier
  keys no longer scroll the view, and the settings window stays put when its
  panel changes. An RTF style sheet can no longer put a character style into a
  paragraph style.
- Fuzzing and tests: generated editing programs also check Save Anyway, the
  overnight campaign can run chosen targets (--targets), and the merge data,
  Markdown and clipboard corpora are seeded. The fuzz lock file includes the
  regex and GTK crates.
- README: the 0.14.0 disk image size is corrected to 5.1 MB.

## 0.14.0

Security hardening and publishing, from the eight findings of a security review.

- Numbers from files and dialogs are bounded before arithmetic (DOCX integers,
  HTML list numbers and sizes, dialog lengths). A bad Table Cells row height
  keeps the table as it was.
- HTML export and clipboard write links only for safe schemes; javascript: and
  data: links become plain text.
- HTML style sheets have rule, byte and matching budgets.
- Linux print PDFs are written to a private directory as a new private file.
- Picture, settings, glossary and dictionary files are read with size limits
  and refused if they are pipes, devices or directories. Oversized clipboard
  data is ignored. The link table has size limits.
- Markdown pictures outside the file's folder are left out until you agree.
  A document stops taking pictures after 256 MiB in total.
- Linux: native GTK menus on X11, window icon and application id, and
  tools/install-linux.sh for the desktop entry.
- Publishing: README and user guide cover the Mac (measured sizes, downloads,
  opening an unsigned alpha), BUILD_INSTRUCTIONS_MAC.md, crates marked
  publish = false, third-party notices regenerated.
- Tests pass on Windows. CI runs every crate even after a failure, with the
  Mac job on an Intel runner. The Linux tar.gz sets its own file permissions.

## 0.13.0

- The overnight fuzz script also seeds the DOCX and RTF targets (and the zip
  target from .docx files) from sample documents, and skips Office lock files.
  Version bump only otherwise.

## 0.12.0

- Better font handling in layout: faces supply styles a file leaves out, and
  symbol fonts draw their symbols. Refinements to the DOCX, ODT, HTML and RTF
  readers and writers, which now keep vertical cell merges, symbol font codes
  and hand-set superscripts.
- Tables: cells merge down a column and split again, and adding a column
  that would take a table past the page asks first.
- Colors: deleting a palette color the document uses asks for a replacement.
  The Document dialog sets superscript and subscript sizes. Interface Size
  moves to Preferences > View, which also gains a default zoom.
- Quitting from outside Quill (the Dock, logging out, shutting down) now asks
  about unsaved documents, and the next start no longer takes it for a crash.
- Drop caps wrap the requested number of lines.
- Print Merge, notes and the rich clipboard extended; export tests added.
- Visual baseline images updated.

## 0.11.0

- Branding: new app and document icons, About artwork, MIME type file for
  Linux.
- Character Style dialog and Style dialog finished: Format > Style edits a
  paragraph style's Paragraph and Character settings. A native macOS color
  panel.
- Preferences keeps edits in a draft: Save, Save and Close, or Cancel.
- Alpha builds no longer carry over settings or recovery files from earlier
  versions.
- Feature seed documents and a Markdown fuzz target; license viewer and
  package checks updated.

## 0.10.0

- Character styles added to the document model and carried through the native,
  DOCX, RTF, ODT and HTML formats and the rich clipboard.
- First Character Style dialog (a temporary Character Styles menu) and a
  Preferences pane. Links get a Hyperlink character style.
- Save As in any format, Text Only, ODT and HTML included, makes the new file
  the active document, with a warning first for formats that lose features.
  ODT and HTML move from export only to full save.
- New fuzz targets for character styles, the clipboard and the native format,
  with a seed script.

## 0.9.0

- Border and Picture dialogs, with picture cropping and paragraph and cell
  shading colors; bulleted and numbered lists, started by typing * or -;
  Replace Fonts and Highlight Missing Fonts; native alerts.
- New tables get real borders that print. Context menus for links and
  tables. An editable table of contents sends its edits to the headings.
- Tables of contents and indexes, links, Print Merge and table dialogs
  extended.
- Saving and opening check files more carefully. A script makes combination
  seeds for the fuzzers.

## 0.8.0

- Native macOS Save and Print panels.
- Hyperlinks in the text.
- Fonts given by PostScript name (as other Mac apps and some RTF files name
  them) are recognized.

## 0.7.0

- Picture selection and resizing, moving table rows, and the Table dialogs
  polished, with matching document model changes.

## 0.6.0

- Markdown reader and writer that report what Markdown cannot keep, and a
  Markdown view.
- Comment cards, a crash log and a license viewer.
- winit is built from a patched copy in vendor/winit so a click during a modal
  panel no longer crashes (see vendor/README.md).
- AGENTS.md and the user guide added.

## 0.5.0

- Table Cells dialogs, a color palette (replacing the macOS color well),
  selection across table cells, paragraph and cell borders.
- macOS hyphenation and the system Look Up panel.
- More native macOS dialogs; zoom reworked.
- RTF, ODT and HTML carry more table detail; nested table tests.

## 0.4.0

- Spelling checked as you type; color emoji in text and PDF.
- Paper size and units chosen from the region on first run.
- Footnotes, endnotes, table operations, file handling and Preferences
  extended; more menu commands.

## 0.3.0

- Revisions pane expanded; DOCX table styles, document defaults and tables of
  contents read.
- Handle for selecting and moving a whole table.
- Tables of contents, Find and settings extended; a color well using the macOS
  Colors panel (replaced in 0.5.0).

## 0.2.0

- Editing fuzz target, shared round-trip checks, property tests for the
  readers and writers, and fuzz/VALIDATION.md.
- DOCX and RTF readers keep a formatting revision even when the text was put
  back to its old format.

## 0.1.0

The starting point of the history: document model, format readers and writers,
text layout and the app, with the simulated classic window decoration replaced
by native windows.
