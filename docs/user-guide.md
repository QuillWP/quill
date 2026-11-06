# Quill

Quill Word Processor, shortened to Quill, is inspired by Lucas's favorite
word processor from 1992. It is built in Rust for macOS, Windows and Linux.
The website is [quillwp.com](https://quillwp.com).
Documents use `.qwp`.
Alpha builds do not import earlier versions' settings, glossary, dictionary or
document recovery files.

The original offered styles, tables, frames, pictures, footnotes, headers and
footers, outlines, tables of contents and indexes in a 914 KB program that
asked for 2 MB of memory. Quill aims for the same features in a footprint
measured in single-digit megabytes. Documents are saved in Quill's own
`.qwp` format by default, so nothing is lost; .docx, RTF and text open
and save for exchanging documents with other programs. Word templates and
macro files (.dotx, .docm, .dotm, without their macros) and OpenDocument Text
(.odt, .ott and flat .fodt, as LibreOffice writes them) and HTML pages open.

Every format in **Save As**, including **Text Only (.txt)**, **OpenDocument Text
(.odt)** and **HTML (.html/.htm)**, becomes the active document after saving.
The title changes to the new filename, and subsequent Save commands write to that
file. The previous file keeps its last saved contents. Closing without further
edits does not ask to save the previous file.

Text Only, ODT and HTML show a warning before Save As: they cannot keep every
feature of a `.qwp` document. You can cancel before the file is written. Richer
features remain on screen for the current session, but reopening restores only
what the chosen format saved. Save a `.qwp` copy first when those features matter.
HTML is one self-contained UTF-8 file with embedded images and CSS: named document
styles become numbered CSS classes (with their original names recorded on paragraphs),
and direct font, size, color, bold, italic, underline, and other character formatting
becomes inline CSS. Paragraph alignment, indentation, spacing, basic tables, and
footnotes are included. ODT exports these as OpenDocument styles, tables, images,
and notes, with the document's page dimensions and margins. These are basic exports:
headers/footers, section columns, positioned frames, image cropping, print-merge
behavior, and other advanced layout settings are not reproduced. HTML page/date/time
fields are placeholders, and browser printing can paginate differently from Quill.

Markdown (.md, .markdown) opens and saves. It carries headings 1 to 6, bold, italic,
strikethrough, links, bulleted and numbered lists, block quotes, code, rules, tables
(with column alignment) and pictures; underline, superscript and subscript are written
as the HTML tags <u>, <sup> and <sub>. Bullets and numbers are typed text in Quill, so a
paragraph that starts with a bullet or a number and a tab is written as a list item. Saving
copies the pictures into a folder beside the file (for Trip.md, a folder called
Trip_images) and names them where they stand, so keep the folder with the file.
Changed pictures get new file names; existing picture files are never overwritten.
If a picture file conflicts, saving stops and leaves the old Markdown file alone. A picture
that the Markdown links to on the web stays a link to it, and web addresses in the text
become links when the file is opened. In Source view the pictures stay inside the text.
Saving as Markdown first lists
what the file cannot carry (fonts, colors, page layout, headers and footers, footnotes,
comments, tracked changes, tab stops, frames and the like) and lets you cancel; the
document you are editing keeps them. View > Markdown > Source shows the document as
Markdown text to edit; View > Markdown > WYSIWYG shows it formatted. Going back to
WYSIWYG without changing the text gives the document back exactly; after editing the text, the
document is rebuilt from it.

DOCX itself can carry all the document content of Lucas's favorite word
processor from 1992 that Quill implements. Three Quill preferences have no
standard DOCX equivalent (margin page numbers, Include Endnotes and Print
Hidden Text), so Quill stores them as
ignorable private XML. Current Microsoft Word removes that private XML
when it resaves a file. Quill warns when an affected document is exported;
keep a `.qwp` copy when those settings matter.

Word also normalizes two otherwise-unused pieces of state from that era: it
changes frames positioned vertically from a margin to its own default
anchor, and discards a stored indent while a table row is centered or
right-aligned. Quill includes those cases in the same targeted warning.

Status: alpha, with platform interaction and release verification still in progress.
Implemented features include editing and formatting,
styles, tables, pictures and frames, sections and columns, footnotes,
headers and footers, Outline view, tables of contents and indexes,
spelling, hyphenation and the thesaurus, Find and Replace, Print Merge,
annotations, tracked changes, the Commands dialog and a customizable toolbar,
saving .qwp, .docx, RTF, text and PDF, opening and saving ODT and HTML.

## Tracked changes

Tools > Track Changes (Command-Shift-E) records insertions, deletions and
character format changes, signed with Preferences' Your Name and dated. They show
in their author's color: insertions underlined, deletions struck through, format
changes double underlined, with a bar in the margin. View > Show Changes switches
to the text as it would be with every change accepted. Tools > Review Changes steps
through them to accept or reject each one, or all at once.
When changes are hidden, Left/Right and Shift+Left/Right skip deleted text;
showing the markup makes those original characters navigable again.

One Review pane on the right holds comments and tracked changes. All shows the
selected change above the editable comment threads; Comments and Changes show
each on its own. Click marked text or use Previous and Next to see who changed
it and when, then Accept or Reject in the pane. Every contributor is listed when
different people inserted, deleted or formatted the same text. Long change
details scroll independently from comments.
Drag the divider between the document and Review left or right to adjust their
widths. The pane keeps its width when switching filters or reopening it in the
same window.
The document scrollbar stays at the window's right edge as the divider moves;
comments keep their own scroll control inside Review.
While dragging, the pane follows the divider with its current text wrapping;
comments rewrap to the new width when you release the mouse.

Initials badges beside changed lines identify their editors. Hover for full
names and change types; click to select a change and reveal it in Review.
Changes sharing a line are grouped in one badge; click again to cycle them.
A plus sign means the group includes multiple authors. Closing Review keeps the
badges available, and View > Annotations reopens the pane for comments.

Tracked changes are kept in .qwp, .docx and RTF, and Word reads and writes them
the same way. Word's tracked changes to paragraph, table and section formatting are
accepted when the file opens, with a note saying so. Plain text, the ODT and HTML
exports and copies to other programs get the text with every change accepted.

## Annotations

Insert > Annotation adds a note in the annotations pane, signed with Preferences'
initials. With text selected, the annotation is about that text, which is shaded.
Insert > Reply to Annotation answers one, and Insert > Resolve Annotation marks the
thread resolved. They are Word's comments in .docx, replies and resolved state
included; RTF keeps each annotation at its point.

## Document size and damaged files

A document can hold up to 250,000 paragraphs (about 10,000 pages), 32 million
characters of text and a million changes of character formatting, counting its
notes, annotations, headers and footers. The limit is the same whatever the format,
so anything Quill opens can be saved as .qwp, .docx or RTF and opened again. A
larger file is refused with the reason, and so is a paste that would take a
document past the limit. Before Save replaces a file, Quill reads the result back,
and if it couldn't open it again the save is refused and the file on disk is left
as it was. A document this large is too big for the ODT and HTML exports, which
say so.

Measurements a file can't sensibly mean, such as margins wider than the page, are
brought within range when it opens: margins shrink to leave room for text. Old
Word 97-2003 `.doc` files, zip archives that aren't documents, and other files
that aren't text are refused with an explanation rather than opened as gibberish.
A file named as a document (.docx, .odt, .qwp) that holds plain text can be
opened as text.

## Clipboard

Copy publishes HTML and plain text on all platforms, plus RTF on macOS and Windows.
Paste accepts RTF or HTML formatting, preserving fonts, sizes, colors,
bold/italic/underline, paragraph breaks, common paragraph alignment/spacing,
lists and tables.
Use **Edit > Paste as Text** (Command-Shift-V; Control-Shift-V on Windows/Linux)
or Paste Special > Unformatted Text to use plain text instead. Include Formatted
Text in Clipboard controls outgoing formatted copies. HTML import reads inline
formatting and the page's own style sheet; scripts never run, and linked
style sheets and pictures are never fetched (pictures carried in the page are
kept). RTF supports
the richer document structures handled by Quill's RTF reader. Cross-document paste
remaps fonts and imports conflicting styles under a Pasted name without changing
existing document styles. Windows/Linux clipboard interaction still needs live
verification on those systems.

## Pictures

Click a picture to select it. Drag any of its eight handles to scale it at its own
proportions, or hold Command to stretch it in the direction pulled. Hold Control and drag a
handle to crop: the frame moves in and the picture stays where it is. Right-click a picture
for Properties..., Crop in Place, which makes the handles crop without Control until you
choose Resize in Place or click elsewhere, and Resize in Place. Properties..., also Format >
Picture..., sets the width, height and scale, with Keep Proportions, and how much to crop
from each edge, and shows the final size as you type.

## Preferences

Changes remain in a draft while you switch panels. **Save** applies all edited settings and
keeps Preferences open; **Save and Close** applies them and closes it. **Cancel** or the close
button discards changes since the last save.

Smart Quotes starts enabled and can be turned off in Preferences. View lets you choose 1-50 recent documents (default 10).
Open And Save includes “Save foreign files in Quill format by default”, enabled and
recommended. On the first Save of an imported document this opens Save As for a
`.qwp` copy. Turning it off permits normal Save back to the imported format; an
explicit format chosen in Save As is respected on subsequent saves.

General > Start Lists from a Typed * or - is on by default. With it, typing `* ` or `- ` at
the start of a paragraph makes a bulleted or dashed list, and Undo gives back what was typed.
In any list, Return starts the next item, Return on an empty item ends the list, and
Backspace at the start of an item takes off its bullet and keeps the text.

## Platform interface

The percentage field at the right of the top toolbar changes document zoom.
Click the number, type a percentage and press Enter or Tab; Escape cancels an
edit. The dropdown offers 50%, 75%, 100%, 125%, 150%, Custom, Fill Horizontal
and Fill Vertical. Custom focuses the same editable field. Fit uses the visible
page's paper dimensions and the available document viewport, opens Page Layout
if needed, and displays the actual applied percentage to two decimal places.
Zoom accepts 10-800% and remembers the last applied percentage between launches;
it does not change saved document measurements or formatting.

The legacy Print Preview command, toolbar button and shortcut use the macOS
system print panel. On Windows and Linux they prepare a PDF and open it in the
default PDF viewer for preview and printing. Quill's old internal preview renderer
has been removed. A system PDF viewer is required on Windows/Linux. Ordinary
Print and Save as PDF retain their existing platform routes.

Right-click in a document for editing actions, page/section breaks, Font, and Paragraph.
On macOS, Control-click works too, except on a link, where it asks whether to open the
link. Clicking inside selected text preserves the selection; clicking elsewhere positions
the insertion point. On a link, the menu has Edit Hyperlink and Remove Hyperlink; on other
selected text, it has Hyperlink. In a table it adds Delete (Row, Column, Cell), Insert Cell
Before and After, Split Cell, and Merge Cells when more than one cell is selected. The
toolbar's font name previews its face.

A new table has real borders that print, thin and black unless `table_border_color` or
`table_border_dotted` in the settings file say otherwise. Those are only the starting point:
the table keeps its borders, and Format > Border changes them. Gridlines are a separate screen
guide for cells with no border, as in tables from other programs, and have their own
`table_gridline_color` and `table_gridline_dotted`.

**Format > Style...** manages paragraph styles. Select a style and choose
**Paragraph...** to edit its indents, spacing and pagination, or **Character...**
to edit its manual font, size, color and character formatting. Each editor's OK
returns to the style dialog without changing the document. **Define** saves the
definition in this document and updates paragraphs using that style, leaving the
dialog open. **Apply** also applies the style to selected paragraphs; **OK** does
that and closes the dialog. None of these writes the document file: use **Save**.
Cancel discards edits not yet defined or applied. Page margins are document settings
in **Format > Document Setup...**. **Use As Default** remains unavailable.

The bottom **Character Style** menu is a link preview. **No Style** uses the manual
character defaults. Selecting a named style strikes the manual summary through;
returning to **No Style** restores its display without losing any manual values.
Saving a linked definition is not implemented yet, so Define, Apply and OK are
unavailable while previewing a named style. Choose No Style to save manual defaults.

Character styles are available through a temporary **Character Styles** menu. To make
one, format and select some text, choose **Manage Character Styles...**, enter a name
and press **Define New**. Select other text and choose that style from the menu, or
choose it at the caret before typing. The style stores differences from the paragraph's
character defaults, so an emphasis style can inherit the paragraph's font and size.
Bold, italic and similar flags follow DOCX character-style toggle rules.

To change a style, format an example, select it, choose the style in the manager and
press **Redefine**. Direct overrides on other text stay in place. **Rename** changes
only its name. **None** or **Remove** takes off the named layer; **Delete Style** removes
the definition and leaves its text looking as it does now. These changes support Undo.
Manage definitions in the main document; existing styles can be applied in editing panes.
Redefinition updates notes, comments and section headers as well as the main text.

The first link creates a **Hyperlink** style for its normal color and underline.
Later links default to that style; choosing a different color in the link dialog makes
an explicit override for that link. Hover and visited colors remain properties of each
link. Native files preserve the full named-style model in format 1.7. DOCX and RTF use
named character styles; HTML and ODT exports also carry Quill metadata for editable
round trips. Other programs may discard that metadata. Existing foreign ODT character
styles still import as direct formatting with a warning; imported style inheritance is
resolved when read rather than kept as a live parent relationship. Older app versions
can open the native files as direct formatting but lose named-style identity when
saving them. Final integration
with the paragraph-style interface is deferred while this menu is evaluated.


Format > Border also offers solid shading colors for paragraphs and, with Apply To:
Cells, table cells. Imported solid RGB fills appear by name or as `#RRGGBB` and stay
exact through native saves, DOCX, RTF, HTML, ODT and PDF output. Changing only borders
keeps the existing fills; choosing None or a gray percentage replaces the color.
Colored patterns and DOCX theme-only fills without a resolved RGB value still use a
fallback and an import warning. Older app versions can open the new native files but
ignore their added color fields and lose those colors if they save the document.


When a document uses fonts that are not installed, the message on opening it can keep them,
which stops the message for that document, or replace them, ten fonts to a page. Format >
Replace Missing Fonts does the same at any time, and View > Highlight Missing Fonts marks the
text they affect on screen. Text typed into a table of contents made editable goes to its
headings when you choose Update Table of Contents, unless lines were added or removed.
To rename headings through six levels, right-click the contents and choose Make Editable,
edit the entry names, then choose Update Table of Contents to make it live again.
The matching headings change together as one Undo step. Subsequent heading edits
continue updating the live contents.

A document opens in the view it was saved in; Preferences > View > In View is for new
documents. The Custom row of the color palette holds the colors saved in Preferences >
Colors, for every document, and then the colors this document uses. Pointing at a saved
color shows its name. Preferences > View has Interface Size, and Advanced has Smart Quotes.

Windows and Linux keep Settings in File and About Quill in the rightmost Help menu.
Their menu labels use the host UI font; macOS retains its application menu and native menus.
Windows dialogs use native Win32 controls, system fonts, and window decorations. The Symbol
window keeps Quill's glyph grid, and Commands uses native scrolling lists. Windows Print opens
the common system Print dialog and prints directly through GDI, including driver printer
options and page ranges. Pages are rasterized individually at up to 300 dpi with bounded
memory and fitted to the selected printer paper; Save as PDF retains vector PDF output.
These Windows UI/printing paths pass cross-target checks but still need live VM/driver testing.
Linux interaction and menu-font checks also remain to be run on a Linux desktop.

## Building

    cargo build --release               # target/release/quill
    python3 tools/package.py            # the Mac app, in target/release/Quill.app
    python3 tools/package.py --debug    # a debug build, in target/debug/Quill.app
    python3 tools/package.py --dist     # a package for this system, in dist/
    python3 tools/footprint.py          # sizes and memory against the budget

Packaging requires Python 3.11 or newer and the native Rust build tools.
On Windows, use `py -3 tools/package.py` in PowerShell; Git Bash is not required.
On Linux, install `libgtk-3-dev` and `libxkbcommon-dev` (Debian/Ubuntu package names).
Without `--dist` the script builds the program to run in place: an app bundle on
the Mac, the bare program elsewhere. With `--dist`, run it on each target OS: it
produces a macOS DMG, a Linux tar.gz, or a Windows ZIP. Architecture comes from the
Rust toolchain, including Windows ARM64.
Use `--check` to run formatting, strict lint, and tests before building.

To transfer the source to another machine, run `python3 tools/package.py --source`
(Windows: `py -3 tools/package.py --source`). The resulting
`dist/quill-<version>-source.zip` includes source, images, tests, tools, and language
data, including uncommitted working-tree changes. It excludes build caches,
compiled applications, Git history, and generated fuzzing output. The same source
ZIP builds on all three systems. `--with-source` builds both the native package and
source ZIP; `--output <folder>` selects a different output directory. Existing
packages for other platforms are preserved.

Hyphenation patterns and thesaurus data are in `data/`, loaded on first
use; packages carry them beside the program. Spelling uses the host OS
service and its installed languages. Added words default to Quill's own
dictionary. Settings > Spelling now provides New and Choose for custom UTF-8
word lists and an Add New Words To chooser. System Dictionary is an explicit opt-in,
remembered across sessions; the Spelling window also lets you change that choice. The Quill dictionary is created on
first Add, beside Quill's settings, as `Quill Dictionary.txt` on macOS and
Windows or `dictionary.txt` on Linux. It contains one UTF-8 word per line;
you can edit it in a text editor while Quill is closed. See
[docs/release.md](release.md) for making a release.

## License

Quill is available under either the [MIT license](../LICENSE-MIT) or the
[Apache License 2.0](../LICENSE-APACHE). Packaged builds also include
[third-party notices](../THIRD-PARTY-NOTICES.txt) generated from the locked
Cargo dependency set, listing what that platform's build uses. `Quill Licenses`, in the
`Quill` menu on the Mac, shows the MIT and Apache texts and links to those notices.
