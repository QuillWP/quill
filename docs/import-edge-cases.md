# Import edge cases

What Quill's importers still cannot handle, found by running every file in
LibreOffice's Writer test suite (`sw/qa`, kept outside the repository)
through Quill's readers on 2026-10-21. The number
after each item is how many test files hit it. These files are edge cases
by design, so ordinary documents hit these far less often.

Everything not marked "not warned" is reported by the warning Quill shows
after opening a file.

To run the sweep again:

```sh
cargo run -p quill-formats --example corpus_sweep path/to/test-files > sweep.tsv
```

Each line is the outcome, extension, time, paragraph count, how much of the
body text was kept (DOCX and ODT), the open-warning notes, and the path.
`cargo run -p quill-formats --example texts FILE` prints what Quill read from
one file.

## Results at the time of writing

| Format | Opened | Refused |
|---|---|---|
| DOCX | 1,797 of 1,800 | 3 damaged |
| ODT | 913 of 916 | 3 password-protected |
| FODT | 566 of 568 | 2 damaged |
| OTT | 7 of 7 | |
| RTF | 554 of 557 | 3 damaged |

Of the files with text, 1,416 of 1,427 DOCX and 707 of 709 ODT keep at least
98% of their letters and digits (1,381 and 704 before equations were read).
Fields that became live Quill fields also lower the score without any real
loss. No file crashes or hangs.

## Won't open

- Password-protected ODT (3). Quill says the file is password-protected.
- Word 97-2003 `.doc` (216 in the corpus): no reader.
- OpenOffice 1 `.sxw` (3): not planned for now. The format was replaced in
  2005 and LibreOffice converts it.
- ReqIF `.xhtml` fragments (not whole pages) and `.xhtml` in general are not
  offered by Open.
- Deliberately damaged files (10), correctly refused: malformed XML (3), a
  duplicated XML attribute (1), broken ZIP compression (1), corrupt RTF hex
  data (3) and 2 more malformed FODT.

## Text that is lost

- Text in DrawingML shapes and charts (`a:txBody`), as opposed to text boxes
  (for example `ooxmlexport/data/fdo76249.docx`).
- Comments in headers and footers (1). Quill keeps comments only in the main
  text.

## Kept, but changed

- Drawings and shapes left out (438).
- Automatic numbering and bullets kept as typed text; they no longer
  renumber (343).
- Fields other than page number, date and time kept as their last text
  (320). Locked or fixed DOCX dates (`w:fldLock`) become live date fields
  (not warned).
- Floating pictures and text boxes become frames before their paragraph,
  placed by margin, page or column. Offsets from a paragraph are lost, and
  those in tables, headers and other boxes, or in front of or behind the
  text, stay in the text.
- Hyperlinks become plain text (186).
- Tracked changes accepted; the history is not kept (155).
- Embedded objects and charts left out, or kept only as their picture (147).
- Pictures in formats Quill cannot decode (94 files; 118 before GIF, BMP,
  TIFF and WebP were read). Files holding each kind: SVM, LibreOffice's own
  metafile, mostly the saved look of charts and objects (33); WMF (18);
  EMF (14); SVG (2). SVG is not planned for now: Word and LibreOffice
  store a PNG copy beside it, which Quill shows. The rest of the warned
  files link to pictures outside them.
- Content controls and form fields become plain text (116).
- One page size and set of margins for the whole document (107).
- Nested tables flattened into the outer table's cells (95).
- Tables of contents and indexes kept as static text (91); index and table
  of contents entries dropped (11).
- Bookmarks dropped (89).
- Endnotes in a document that also has footnotes become footnotes (9).
- Equations (DOCX, ODT and RTF, about 55) become one line of text that
  reads the same, such as `x = (−b±√(b²−4ac))/2a`, with scripts raised and
  lowered. They can no longer be edited as equations. Word's older
  Equation Editor objects are still only their picture.
- RTF bytes in Japanese, Chinese or Korean double-byte code pages are read
  as Latin (not warned). Single-byte code pages, per font or for the whole
  document, are read correctly.
- Footnotes in headers keep their text in place of the mark (1).
- Comments in footnotes are moved to the footnote's mark in the text (1).
- Ruby guide text follows its base text in brackets (10).
- Macros in `.docm` and `.dotm` files are left out.
- Character styles become direct formatting (not warned; nothing visible
  changes).
- Table rows with more than 255 cells: the extra cells' text goes in the
  last cell (none in the corpus).

## Fixed during the sweep

These were losing text without a warning and now read correctly:

- RTF: large revision, list and template ids were refused as invalid numbers
  (156 files), as were files with trailing data or cut off early (11).
- RTF: list numbers and bullets were dropped; PAGE, DATE and TIME fields came
  in as fixed text; deleted tracked-change text came in as ordinary text.
- DOCX: text box text was dropped (about 320 files), including in drawing
  groups that also hold a picture.
- DOCX: table rows and cells wrapped in content controls were dropped.
- DOCX: tables inside a paragraph were dropped.
- DOCX: ruby base text was dropped. The earlier fix only covered ruby placed
  directly in a paragraph; real files put it inside a run.
- DOCX: a comment in a footnote was dropped, and a footnote in a header left
  a stray mark and lost its text, both without a warning.
- `.docm`, `.dotx` and `.dotm` are offered in Open (11 in the corpus, all
  read).
- MACROBUTTON and GOTOBUTTON fields (DOCX and RTF) show the text in their
  instruction.
- Equations in DOCX, ODT and RTF were dropped; they are now linear text.
- ODF master documents (`.odm`) open with the copy of each linked section
  saved in them; the linked files are not opened.
- HTML opens (58 of 60 corpus pages before tolerating a tag cut off at the
  end, 60 of 60 after): headings as heading styles, lists, real tables,
  pictures carried in the page, MathML, the page's own style sheet and
  Quill's own exports with their styles. Linked pictures and style sheets
  are never fetched and are reported.
- ODT: rows with more than 64 cells folded the extra text into the last cell
  (the limit is now 255).
- ODT: white space next to bookmarks, change marks and frames now collapses
  the way LibreOffice's does (checked against its `whitespace.odt` test).
