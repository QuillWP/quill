# Quill

Quill is inspired by Lucas's favorite word processor from 1992. It is built
in Rust for the Mac, and a Windows build is on its way.
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
become links when the file is opened. Pictures beside the file or in a folder under it
load on their own. If the file names pictures anywhere else, Quill leaves them out and asks
once whether to load them; Don't Load is the default, and Show Paths lists the files. Loading
copies the pictures into the document, so load them only from files you trust. In Source view
the pictures stay inside the text.
Saving as Markdown first lists
what the file cannot carry (fonts, colors, page layout, headers and footers, footnotes,
comments, tracked changes, tab stops, frames and the like) and lets you cancel; the
document you are editing keeps them. View > Markdown > Source shows the document as
Markdown text to edit; View > Markdown > WYSIWYG shows it formatted. Going back to
WYSIWYG without changing the text gives the document back exactly; after editing the text, the
document is rebuilt from it.

Paste puts text only into a Markdown file, in either view: the copied text's fonts and pictures
are not brought along. In a Markdown file, Return carries on a numbered list with the next
number, Return on an empty item ends the list, and Return on an empty paragraph in a quote takes
it out of the quote, one level each time.

DOCX itself can carry all the document content of Lucas's favorite word
processor from 1992 that Quill implements. Two Quill preferences have no
standard DOCX equivalent (margin page numbers and Print
Hidden Text), so Quill stores them as
ignorable private XML. Current Microsoft Word removes that private XML
when it resaves a file. Quill warns when an affected document is exported;
keep a `.qwp` copy when those settings matter.

Word also normalizes two otherwise-unused pieces of state from that era: it
changes frames positioned vertically from a margin to its own default
anchor, and discards a stored indent while a table row is centered or
right-aligned. Quill includes those cases in the same targeted warning.

## Grep, Regex Search and Sed

Edit > Advanced has three pattern tools for people who know `grep` and `sed`.
They are built into Quill, so they work the same on macOS, Windows and Linux
and need no other program. All three treat each paragraph as a line.

Patterns use the extended regular expression syntax of Rust's `regex` crate:
`.`, `[a-z]`, `[^0-9]`, `*`, `+`, `?`, `{2,5}`, `|`, `^`, `$`, `\d`, `\w`,
`\s`, `\b`, `(...)` for groups and `(?i)` for ignoring case. Write groups as
`(...)`, not `\(...\)`. `\w`, `\d` and `\b` understand accented and
non-Latin letters. Not supported: look-ahead and look-behind, back-references
inside a pattern (`(a)\1`) and `\p{L}`-style classes. Quill says so when a
pattern uses one.

**Grep** lists the paragraphs that match, with their paragraph numbers.
Click a row, or choose Go To, to select the first match in that paragraph
and scroll to it. Ignore Case, Whole Word (`grep -w`) and Invert Match
(`grep -v`) are check boxes. Only the main text is searched, not footnotes,
headers or comments. Text that tracked changes show as deleted is searched
too.

**Regex Search** is Find and Replace with regular expressions. Find Next
selects the next match after the selection and wraps to the top at the end.
Replace changes the selected match, if it is one, and moves to the next.
Replace All changes every match in the document or the selection as one
step that Undo takes back. The replacement reads `&` (the whole match), `\1`
to `\9`, `\n` and `\t` as in Sed. A match is within one paragraph, and an
empty match is skipped. Ignore Case and Whole Word are check boxes.

**Sed** runs a script of `s` (substitute) commands on the document or the
selection, as one step that Undo takes back.

- `s/old/new/` changes the first match in each paragraph, and `s/old/new/g`
  every match. A number flag changes only that match (`s/a/b/2`), and `2g`
  that one and all after it. `i` ignores case.
- Any character can be the delimiter: `s|/usr|/opt|`.
- In the replacement, `&` is the whole match, `\1` to `\9` are groups, `\n` is
  a new paragraph, `\t` is a tab and `\&` and `\\` are the characters
  themselves.
- Put `;` or a new line between commands. Each command works on the text the
  one before it left.
- An address limits a command to some paragraphs: `3s/a/b/`, `3,7s/a/b/`,
  `3,$s/a/b/`, `$s/a/b/` (the last paragraph) and `/word/s/a/b/` (paragraphs
  that match). Paragraph numbers count from 1 and are read when the command
  runs.
- Other sed commands (`d`, `y`, `p` and so on) and `\U`, `\L` in a
  replacement are not supported, and the script is not run if it uses one.

New text takes the character format of the text it replaces. With Apply to
set to Selection, only matches wholly inside the selection change; `^` and
`$` still mean the start and end of the paragraph.

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
as it was. A .qwp file must also come back exactly as it was saved, since Quill's
own format keeps everything. If it doesn't, the message says which part changed,
and for the main text Quill selects the first paragraph that came back different;
retyping it or removing its formatting may let the document save. Save Anyway
saves the document as Quill reads it back, so the file still opens, and the window
then shows what it holds. Undo takes that change back, except when fonts or
pictures changed, which the message says first. The file as it was is kept as
"Backup of" its name, even when backups are turned off. A document this large is too big for the ODT and HTML exports, which
say so.

Measurements a file can't sensibly mean, such as margins wider than the page, are
brought within range when it opens: margins shrink to leave room for text. Old
Word 97-2003 `.doc` files, zip archives that aren't documents, and other files
that aren't text are refused with an explanation rather than opened as gibberish.
A file named as a document (.docx, .odt, .qwp) that holds plain text can be
opened as text.

## Clipboard

Copy puts HTML, RTF and plain text on the clipboard.
Paste accepts RTF or HTML formatting, preserving fonts, sizes, colors,
bold/italic/underline, paragraph breaks, common paragraph alignment/spacing,
lists and tables.
Use **Edit > Paste as Text** (Command-Shift-V)
or Paste Special > Unformatted Text to use plain text instead. Include Formatted
Text in Clipboard controls outgoing formatted copies. HTML import reads inline
formatting and the page's own style sheet; scripts never run, and linked
style sheets and pictures are never fetched (pictures carried in the page are
kept). RTF supports
the richer document structures handled by Quill's RTF reader. Cross-document paste
remaps fonts and imports conflicting styles under a Pasted name without changing
existing document styles.

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

The Fonts panel sets the font, style (the faces the chosen font has) and size of new documents.

## Platform interface

The percentage field at the right of the top toolbar changes document zoom.
Click the number, type a percentage and press Enter or Tab; Escape cancels an
edit. The dropdown offers 50%, 75%, 100%, 125%, 150%, Custom, Fill Horizontal
and Fill Vertical. Custom focuses the same editable field. Fit uses the visible
page's paper dimensions and the available document viewport, opens Page Layout
if needed, and displays the actual applied percentage to two decimal places.
Zoom accepts 10-800% and remembers the last applied percentage between launches;
it does not change saved document measurements or formatting.

The Print Preview command, toolbar button and shortcut open the macOS print
panel, which shows its own preview.

Right-click in a document for editing actions, page/section breaks, Font, and Paragraph.
On macOS, Control-click works too, except on a link, where it asks whether to open the
link. Clicking inside selected text preserves the selection; clicking elsewhere positions
the insertion point. On a link, the menu has Edit Hyperlink and Remove Hyperlink; on other
selected text, it has Hyperlink. In a table it adds Delete (Row, Column, Cell), Insert Cell
Before and After, Split Cell, and Merge Cells when more than one cell is selected. The
toolbar's font name previews its face. Beside the font size, the Color pop-up shows the
selection's color and offers the original's eight colors, the colors named in Preferences >
Colors, and More Colors…, which opens the Character dialog.

A new table has real borders that print, thin and black unless `table_border_color` or
`table_border_dotted` in the settings file say otherwise. Those are only the starting point:
the table keeps its borders, and Format > Border changes them. Gridlines are a separate screen
guide for cells with no border, as in tables from other programs, and have their own
`table_gridline_color` and `table_gridline_dotted`.

**Format > Paragraph Styles...** (Command-T) lists paragraph styles, laid out like
**Format > Character Styles**: **This Document**
shows Normal, the styles the document uses and the styles it defines; **All Styles** adds
the automatic ones. Under the list are the chosen style's description and how many
paragraphs use it. **+** opens the style editor on a new style made from the
selection's formatting, and **Edit...** (or a double-click) opens it on the chosen style.
**-** removes a style the document defines (Normal and the automatic styles stay). When
paragraphs use it, Quill asks what they become: **No Style (Normal)** or another style
chosen from the menu. Deleting can be undone. **Duplicate** makes a copy named "... copy".
**Apply** gives the style to the selected paragraphs and **Remove Style** puts them back
to Normal. **Update to Selection** redefines the chosen style from the selection's
paragraph and character formatting. Each of these can be undone.

**Format > Clear Formatting** puts the selected paragraphs (or the paragraph at the
caret) back to Normal with nothing set by hand: paragraph formatting, character formatting
and character styles are all removed, and the text, pictures and a table cell's place stay.
It is one step to undo. It has no key. The same
command is the second choice in the formatting toolbar's style list (under **No Style**), a row
in the Style Sheets palette, and a button in the Paragraph Styles list. **No Style** differs:
it keeps what was set by hand. The **Clear Formatting** button in the Character Styles list, and
the matching palette row on the Character tab, clear only the selected text (its character
styles and hand formatting) and leave the paragraph alone; with just a caret, the next text
typed is plain.

In the style editor, **Paragraph...** edits the style's indents, spacing and pagination,
and **Character...** its manual font, size, color and character formatting. Each of those
returns to the editor without changing the document. **OK** saves the style in this
document, updating the paragraphs that use it, and closes the editor; it does not restyle
the selection. **Apply** saves it and also gives it to the selected paragraphs, leaving the
editor open. None of these writes the document file: use **Save**. Cancel discards edits
not yet saved. Page margins are document settings in **Format > Document Setup...**.
**Use As Default** remains unavailable.

**View > Stylesheets Palette** shows the Style Sheets palette: one list of the document's
paragraph styles (marked with a paragraph sign) and character styles (marked with an A),
with **All**, **Paragraph** and **Character** tabs (shortened to Para and Char in a narrow
palette) that choose what the list shows. Clicking a style applies it to the selected
paragraphs or text. **No Style** at the top of the paragraph styles puts the selected
paragraphs back to Normal and takes their character styles off, so styles can be applied
afresh (manual formatting stays); **No Style** at the top of the character styles takes only
the character style off. The row for the style at
the insertion point is highlighted. **New** and **Edit** open the style's editor, **Copy**
makes a copy named "... copy", and **Delete** removes a style the document defines (Normal,
the automatic styles and No Style cannot be deleted), asking first what the paragraphs of a
paragraph style in use become; each of these can be undone. The
palette starts docked at the right of the document window, taking its width from the text.
The button at the right of its title bar (or in the tab row) moves it into a small window of
its own, which can be dragged anywhere on the screen, outside the document window too, and
back again. That window has the system's title bar and close button, and follows whichever
document window is in front: it lists that document's styles, and the row chosen for Edit,
Copy and Delete is cleared when another document comes in front. It stays above Quill's
document windows and hides while another program is in front. Whether it
shows, its tab and whether it is docked are saved in the settings (`show_styles_palette`,
`styles_palette_tab`, `styles_palette_docked`).

The bottom **Character Style** menu ends with **New Character Style...**, which opens the
character style editor. Saving it closes the editor and returns to the style editor with the
new style listed and chosen; Cancel leaves the menu as it was.

The **Character Style** menu links the paragraph style to a character style. **No Style**
uses the manual character formatting. Choosing a named style strikes the manual summary
through, and saving makes the style's text take the character style; going back to **No
Style** brings the manual values back.

**Format > Character Styles...** lists the document's character styles. To make one,
format and select some text, press **+**, name it and save it. Select other text and
press **Apply**, or click the style in the Style Sheets palette, or choose it at the
caret before typing. The style stores differences from the paragraph's
character defaults, so an emphasis style can inherit the paragraph's font and size.
Bold, italic and similar flags follow DOCX character-style toggle rules.

To change a style, format an example, select it, choose the style in the list and
press **Update to Selection**, or press **Edit...** to change what it sets. Direct
overrides on other text stay in place. **Remove Style** takes off the named layer; **-**
deletes the definition and leaves its text looking as it does now. These changes support Undo.
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
saving them.


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

In Preferences > Fonts, Additional Font Sources opens a separate dialog for
loading fonts without installing them. Choose fills a source row, + Add Source
adds a row, and Delete removes it. Done returns changes to Preferences; Save or
Save and Close applies them. Cancel leaves the previous settings unchanged.
On macOS, Choose also accepts `.app` bundles: select an application to use the
fonts stored inside it. Inside an application Quill searches only
`Contents/Resources` and `Contents/Library/Fonts`, and the folder search depth counts
from the application, so the default of four levels reaches `Contents/Resources/Fonts`.
Increase it for fonts stored deeper inside an application. Keep the source checked to use its fonts.
If the application is moved, removed, or updated, its fonts may no longer be
available, and documents that use them may look different. When a font family is
installed on the system and also in a source, the system's copy is used; when two
sources have the same family, the one added first is used. A family is taken whole from
one place, so two versions of a font never mix. A source that is no longer there
(moved, renamed or deleted) is taken off the list.

So that one source cannot set Quill searching a whole disk, Choose refuses the top of a
disk, the home folder or a folder holding it, the folders the system already loads fonts
from, and the big program and system folders (`/Applications`, `/System` and `/Library`
on macOS; `C:\Windows`, `Program Files` and `ProgramData` on Windows; `/usr`, `/opt` and
the like on Linux). A folder inside one of them, such as one application, is fine. One
search looks in at most 2,000 folders and takes at most 20,000 font files across all the
sources; when it stops early, Additional Font Sources says so. The sources are searched in
the background, and the new fonts come into use when the search is done.

Preferences > View > Appearance chooses Light, Dark or Auto. Auto, the default, follows
the system's light or dark setting and changes with it while Quill is running. In Dark the
toolbars, rulers, panes and dialogs go dark, and so does the document: the page turns
near-black, black and automatic text shows white, white text shows dark, and other colors
turn lighter or darker so text stays as readable against its highlight or shading as it was.
Pictures keep their colors. Only the screen changes: the document keeps its own colors, and
printing, Save as PDF and every saved format are the same as in Light.

## Building

    cargo build --release               # target/release/quill
    python3 tools/package.py            # the Mac app, in target/release/Quill.app
    python3 tools/package.py --debug    # a debug build, in target/debug/Quill Debug.app
                                        # (on Linux, the program target/debug/quill-debug)
    python3 tools/package.py --dist     # the disk image to share, in dist/
    python3 tools/footprint.py          # sizes and memory against the budget

The debug build is a separate program from Quill, with its own settings and
recovery files under the "Quill Debug" profile. On Linux it is called `quill-debug`,
and its debug package does not install itself as Quill.

On Linux, Quill opens its windows through X11 whenever it can, which on a Wayland
desktop means XWayland. There the desktop draws the title bar in its own theme and
the menu bar is GTK's. Set `QUILL_BACKEND=wayland` to open native Wayland windows
instead; GNOME then leaves the title bar to Quill, which draws a plain one.

Packaging requires Python 3.11 or newer and the Rust toolchain. Without
`--dist` the script builds the app to run in place; with `--dist` it also makes
a disk image. Use `--check` to run formatting, strict lint and tests before
building.

To transfer the source to another machine, run `python3 tools/package.py --source`.
The resulting
`dist/quill-<version>-source.zip` includes source, images, tests, tools, and language
data, including uncommitted working-tree changes. It excludes build caches,
compiled applications, Git history, and generated fuzzing output. `--with-source`
builds both the disk image and the source ZIP; `--output <folder>` selects a
different output directory.

Hyphenation patterns and thesaurus data are in `data/`, loaded on first
use; packages carry them beside the program. Spelling uses the host OS
service and its installed languages. Added words default to Quill's own
dictionary. Settings > Spelling now provides New and Choose for custom UTF-8
word lists and an Add New Words To chooser. System Dictionary is an explicit opt-in,
remembered across sessions; the Spelling window also lets you change that choice. The Quill dictionary is created on
first Add, beside Quill's settings, as `Quill Dictionary.txt`. It contains one
UTF-8 word per line;
you can edit it in a text editor while Quill is closed. See
[docs/release.md](release.md) for making a release.

## License

Quill is available under either the [MIT license](../LICENSE-MIT) or the
[Apache License 2.0](../LICENSE-APACHE). Packaged builds also include
[third-party notices](../THIRD-PARTY-NOTICES.txt) generated from the locked
Cargo dependency set, listing what that platform's build uses. `Quill Licenses`, in the
`Quill` menu on the Mac, shows the MIT and Apache texts and links to those notices.
