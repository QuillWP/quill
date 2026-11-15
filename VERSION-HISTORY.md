# Version history

What changed at each tagged version. Each tag is one squashed commit, so this
file is the only place the per-version story is kept in readable form. It was
rebuilt from the tagged commit messages and the diff between each pair of tags.
Add an entry for every new tag as part of the release process.

Versions 0.1.0 to 0.14.0 were all tagged on 2026-11-05.

## 0.23.0 - 2026-11-15

Safer recovery, Read Aloud on the Mac, color boxes in more dialogs, and a
round of fixes found while testing on Windows and Ubuntu.

**Safety**

- A recovered document stays quarantined until its window has drawn a first
  frame. Before, a document that panicked during its first layout or paint was
  already back in the automatic queue and could stop every later launch.
  Autosave is held off while it is quarantined, and either recovery state is
  cleaned up on save or close.
- A saved clipboard crash is kept as a regression test.
- A picture kept only for Undo no longer stops a .qwp save. The save check now
  compares against the same compacted document the writer saves.
- The table of tracked changes has a budget of 100,000 entries and 32 MiB.
  Opening a file that needs more fails with a message instead of opening with
  changes missing or re-dated. Typing is never blocked.
- Updated tokio and rustix to the first releases that fix their RustSec
  unsoundness advisories, and moved the clipboard library to a Windows
  dependency that is not yanked.

**Read Aloud (Mac)**

- Selected text, or the whole document, can be spoken with the voices already
  installed on the Mac. No voice data is bundled. Stop Reading ends it at
  once. The commands are not in the Windows and Linux builds.

**Colors**

- Edit Character Style, the Hyperlink dialog and Format > Character on Windows
  and Linux use the same color box and palette as Border, with the HTML code
  beside it. A character style may set no color, with a None button.
- Paragraphs and table cells keep one flat fill color. The shading percentage
  is gone from the model and every format. Readers fold an imported percentage
  into a flat color so patterns look the same.

**Fixes**

- Settings and Cmd-comma no longer scroll a scrolled document back to the
  caret.
- Right-clicking a table's handle selects the table and shows the table menu.
  Return at the start of a table that begins the document puts a paragraph
  above it.
- Linux: a menu is no longer closed by the focus change around a mouse press.
- The Styles palette sorts paragraph and character styles together on All.
- Find and Replace by format is removed. Find and Replace work on text only.
- The version in Cargo.toml and the README now matches the release.

## 0.22.0 - 2026-11-13

A reworked Border dialog with color boxes and Apply To Table, borders that
join properly, and the end-of-document bar removed.

- The line color is a color box (with its HTML code beside it) that opens the
  same palette as the Character dialog. A new line color goes to the sides
  ticked, and the side checks are one column.
- A Background color box with No Color sets a plain background for the
  paragraph, cells or table. A background and a gray percentage are kept
  together in the model: DOCX stores both, ODT and HTML store the resulting
  color, and the dialog does not offer percentages.
- Apply To is Paragraph, Cells or Table. What you changed in the dialog goes
  with you when you change the target, and OK still writes only what differs
  from what the target showed.
- A side changed on a cell is also set on the cell across it, so a shared edge
  is one color. Cells that do not touch the selection are left alone.
- Clicking the handle of a selected table opens Border on Table. The right-
  click menu in a cell has Borders and Shading..., which opens it on Cells,
  and no longer offers Hyperlink while cells or a whole table are selected.
- A cell's top and bottom lines run on under its right line, so the corner
  joins.
- The short bar drawn under the last line of every document is removed.

## 0.21.0 - 2026-11-13

Windows dialogs use native controls, and the toolbar's Font Color opens the
color palette.

**Windows dialogs**

- The native panel makes color swatch buttons, the palette popup, sliders
  (trackbars) and shown-only color boxes, so Preferences > Colors, the
  Character style dialog and Edit Color look like the rest of the system.
- Labels sit beneath the fields, and are as wide as their text. Choosing
  another Preferences page changes the window in place instead of closing and
  rebuilding it.
- A dialog opened over another (Edit Color over Preferences) is owned by the
  one below, which stays showing, disabled, until the upper one closes.
  Windows keeps them together and raises them together.

**Colors**

- Font Color on the toolbar opens the same palette that pops out beside a
  dialog's color box, below the button. One click on a box colors the
  selection, Edit Color opens the editor and applies its result to the text,
  and a press elsewhere or Escape closes it. This replaces a menu of the
  original's eight color names.

**Removed**

- The Include Endnotes section setting. DOCX has no equivalent, so it was
  stored as private XML that other word processors drop when they resave.
  Notes set to go at the end of a section now always print at the end of the
  document.

## 0.20.0 - 2026-11-12

Hang fixes for Windows and Linux dialogs, and a step-by-step event log for
finding hangs.

**Event log**

- quill --with-events writes each step to quill-events.log in the folder Quill
  was started from (or --events-file=PATH, or QUILL_EVENTS_FILE), falling back
  to the settings folder. --with-events=all adds mouse moves, redraws and
  every wake of the event loop.
- It logs startup, window events, key presses and their commands, menu
  actions, dialog outcomes (a begin with no end means it never came back),
  file reads and writes and panics, and the build commit. A watchdog adds a
  STUCK line every two seconds while the program is busy and has logged
  nothing, and a line repeated in a row is shown three times and then counted.

**Linux**

- Open, Save and message dialogs are GTK dialogs on the main thread. rfd ran
  them on a thread of its own, which froze Quill on Ubuntu 24.04. A finished
  dialog is hidden, destroyed and its pending events run so it releases its
  keyboard and mouse grab.
- Save As lets you pick the file type: choosing a filter changes the name's
  extension, as the Mac's save panel does.

**Windows**

- Message boxes go through TaskDialogIndirect owned by Quill's window, with
  their real button labels, and run on a thread of their own. File dialogs run
  on a thread of their own too. Run on the main thread inside winit's event
  handler, they never appeared or hung on the second use.

## 0.19.0 - 2026-11-12

Paragraph Styles matches Character Styles, Clear Formatting arrives, and Tools
> Commands is taken out.

**Paragraph Styles**

- Paragraph Styles uses the Character Styles layout: +, -, Edit and Duplicate
  under the list, Apply, Remove Style and Update to Selection on the right,
  and the This Document or All Styles choice kept. Update to Selection
  redefines the style from the selection.
- Apply works (it did nothing at first), the question before replacing all
  settings from a file is back, and the formatting toolbar's style list is No
  Style, Clear Formatting, Normal, then every other style by name.
  Character... in a style editor uses the same color palette as Format >
  Character.

**Clear Formatting**

- Format > Clear Formatting (below Revert To Style) puts the selected
  paragraphs back to Normal with no manual paragraph or character formatting
  and no character styles, as one undo step. Text, pictures and a table cell's
  place are kept.
- It is also a row in the toolbar's style list and in the Style Sheets
  palette, and a button in both style lists. In Character Styles it clears
  only the selected text, or makes the next text typed plain when there is
  just a caret.

**Removed for now**

- Style import is deferred until after version 1.0. The Import buttons and
  everything behind them are removed from both style lists.
- Tools > Commands is removed. A key changed there was saved and listed but
  did nothing, because the key handling never read the changes.

## 0.18.0 - 2026-11-11

A security and robustness pass: hard limits on hostile files, faster readers,
writers that stop at the size limits, large files split up, and questions
before risky saves.

**Hostile and oversized files**

- Invalid headers nested inside native stories are dropped. Go To page
  arithmetic saturates, and dialog values, Renumber starts and roman numerals
  stay within the document limits.
- Picture metadata, format numbers and .docx list number text are bounded,
  with checked arithmetic.
- The .odt reader charges every paragraph to the read budget, so a small file
  of millions of empty paragraphs, or a text:s element that makes a billion
  spaces, is refused with the usual too-large message in about 0.2 s. Unique
  pictures are counted as .docx and .odt readers decode them.
- The .qwp, .docx and .odt writers and the Markdown table writer stop building
  a document once it passes the size limits. Oversized Markdown stays in
  Source view.

**Speed**

- Unclosed Markdown links and angle brackets no longer rescan the paragraph:
  seven 80,000-opener files that took up to 8.8 s each now open in about 0.1
  s. HTML raw-text endings are scanned without copying.
- DOCX table style XML is shared across cells, repeated XML and header parts
  are cached, and converted .docx and .odt pictures are reused.

**Questions before risky actions**

- Saving over a file that changed on disk asks Replace File or Cancel.
- Save As says so when it changes an unknown extension to .qwp, and lets you
  cancel.
- Replacing all settings from a file asks first, defaulting to not replacing.
  Print Merge asks before reading files outside the document's own.
- Recovery files are marked while they are opened, license text is written
  inside a private folder, Windows links open through ShellExecuteW, and the
  active modal print panel is identified.

**Editing**

- Command-Shift-Z is Redo, and Edit > Redo names the change. Repeat
  (Command-Y) only repeats. Plain Text has no default key.
- On the Mac the window's close button shows a dot while the document has
  unsaved changes.

**Structure and size**

- Tests move out of app.rs, docview.rs, the formats lib.rs and the core
  document.rs into files of their own. Message boxes, tooltips, font loading,
  menu tracking, printing, effects, and key, text and mouse handling move out
  of app.rs into their own modules, and the atomic write code into atomic.rs.
- Unused code is deleted, six-digit colors are read in one place, and the HTML
  reader looks entity names up in a small table of the 252 HTML 4 references
  instead of a 133 KB function. The format bodies are no longer generic.
  Packaged binaries no longer carry the builder's folder names, and the About
  picture is not copied into the Mac app's Resources twice.
- KNOWN-ISSUES, CONTRIBUTING and SECURITY are added, and stale documentation
  is fixed.

## 0.17.0 - 2026-11-11

Font sources get their own preferences, Linux windows and appearance behave,
and Format > Style becomes a Paragraph Styles list.

**Fonts**

- Additional font sources move into Preferences > Fonts, and a macOS app
  bundle can be a source (only its Resources and Library/Fonts are searched).
- Preferences > Fonts has Markdown settings: the font and size of Source view
  (default Courier New 10) and the body font and size of Markdown files
  (default New York 12). Open documents take a change in place without being
  marked modified.
- Sources are added one at a time in the order they were added. A family the
  system already has keeps its own faces, and a later copy is left out whole.
- A source that has been moved or deleted is dropped from the list. Choose
  refuses the top of a disk, the home folder and system and program folders.
  One search visits at most 2,000 folders and 20,000 font files, and runs on a
  thread of its own so the window does not stop.
- Add Source is greyed out while a row is still waiting for its folder.

**Linux**

- Windows open through X11 (XWayland on a Wayland desktop) so they get the
  theme's title bar and GTK's menu bar. QUILL_BACKEND=wayland keeps native
  Wayland windows.
- The debug program is quill-debug, with its own window class and app ID.
- Auto appearance follows GNOME's color-scheme setting (or a -dark GTK theme,
  or GTK_THEME). The Style Sheets window stays above the documents without
  taking the keyboard, and the palette's chosen row starts over when another
  document comes to the front.

**Styles**

- Style Sheets moves from View to Window. The Character Styles menu is gone;
  Character Styles... is in Format.
- Format > Style is now Paragraph Styles... (still Command-T): a list with
  This Document or All Styles, the style's description and use count, and New,
  Edit and Delete. OK in the editor saves the style without restyling the
  selection; Apply saves it and also gives it to the selected paragraphs.
- Deleting a style that paragraphs use asks what they become: No Style
  (Normal) or another style.

## 0.16.0 - 2026-11-11

The Style Sheets palette, a Light, Dark or Auto appearance and a build stamp
in debug builds.

**Style Sheets palette**

- View > Style Sheets is one list of paragraph and character styles with All,
  Paragraph and Character tabs (Para and Char when it is narrow). Clicking a
  row applies the style the way the toolbar's style menus do. New, Edit, Copy
  and Delete sit beside it and leave Normal, the automatic styles and No Style
  alone.
- It docks to the right of the text, or comes out into a small window of its
  own with the system's title bar (a floating tool window on the Mac that
  hides when Quill is not in front). Whether it shows, its tab and whether it
  is docked are saved in the settings.
- No Style at the top of the paragraph styles puts the selected paragraphs
  back to Normal and takes off their character styles, as one undo step.
- The Style dialog's Character Style menu ends with New Character Style, which
  opens the editor and returns with the new style linked.
- The palette and the style dialogs now share one copy and one delete. That
  fixes the next text taking the wrong style after deleting a character style
  and copies being named "Body copy copy".

**Appearance**

- Preferences > View has Appearance: Auto (follows the system while Quill
  runs), Light or Dark. On the Mac the whole application takes it, including
  alerts and the Open and Save panels.
- In dark mode the page is drawn dark: white paper goes near-black, black and
  automatic text near-white, white text dark, and colors keep their hue.
  Pictures are not touched. Printing, PDF and export are as before.

**Builds and testing**

- A debug build's About window shows when and from what it was built (time,
  branch, commit, and "modified" for uncommitted changes). Release builds are
  unchanged.
- tools/smoketest.py checks a build quickly (version, drawing, opening a small
  document in each main format) in a scratch home folder with its own profile,
  so it never touches real settings.

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
