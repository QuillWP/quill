//! Keyboard commands, selection mutation, formatting, and ruler edits.

use super::*;

impl DocView {
    // ----- keyboard -----

    /// Insert typed text, replacing the selection.
    pub fn type_text(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        if text.is_empty() || (self.sel.is_empty() && self.on_row_mark()) {
            return;
        }
        self.doc.begin_typing(self.sel);
        let at = self.delete_selection();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
        let text = if crate::settings::with(|s| s.smart_quotes) {
            let before = self.doc.main.paras[at.para].text[..at.offset]
                .chars()
                .next_back();
            smart_quotes(&text, before)
        } else {
            text
        };
        let end = self.doc.insert_text(MAIN, at, &text, chp);
        self.caret_formats.clear();
        self.finish_edit(Selection::caret(end));
        match &mut self.repeat {
            Some(Repeatable::Text(t)) => t.push_str(&text),
            _ => self.repeat = Some(Repeatable::Text(text)),
        }
    }

    /// Whether the document has room for `extra` more within Quill's size
    /// limit. If not, the status line says so.
    pub(super) fn room_for(&mut self, extra: quill_core::limits::Size) -> bool {
        let now = self.doc.size();
        let after = quill_core::limits::Size {
            paragraphs: now.paragraphs + extra.paragraphs,
            characters: now.characters + extra.characters,
            runs: now.runs + extra.runs,
        };
        match after.too_large() {
            Some(why) => {
                self.status = Some(format!(
                    "Quill didn't paste: the document would be too large ({why})."
                ));
                false
            }
            None => true,
        }
    }

    /// Take back the edit just made (`what`, such as "paste") if it left
    /// the document over Quill's size limit, so it can always be saved.
    pub(super) fn undo_if_too_large(&mut self, what: &str) {
        if let Some(why) = self.doc.size().too_large()
            && let Some(sel) = self.doc.undo()
        {
            self.sel = sel;
            self.status = Some(format!(
                "Quill didn't {what}: the document would be too large ({why})."
            ));
        }
    }

    pub(super) fn delete_selection(&mut self) -> Position {
        if let Some(block) = self.cell_block()
            && !block.whole_table(&self.doc.main.paras)
        {
            return self.clear_cells(&block);
        }
        let (s, e) = self.sel.range();
        if s != e { self.delete_range(s, e) } else { s }
    }

    /// Delete `s..e`, keeping tables in shape: across cell or row marks,
    /// or between a table and the text around it, only the text goes, as
    /// in the original (so Backspace at the start of a cell does nothing).
    pub(super) fn crosses_table(&self, s: Position, e: Position) -> bool {
        let paras = &self.doc.main.paras;
        (s.para..e.para).any(|p| matches!(paras[p].mark, Mark::CellEnd | Mark::RowEnd))
            || paras[s.para].pap.in_table != paras[e.para].pap.in_table
    }

    pub(super) fn delete_range(&mut self, s: Position, e: Position) -> Position {
        if !self.crosses_table(s, e) {
            return self.doc.delete(MAIN, s, e);
        }
        if let Some(at) = self.doc.delete_whole_tables(MAIN, s, e) {
            return at;
        }
        for p in (s.para..=e.para).rev() {
            let len = self.doc.main.paras[p].len();
            let a = if p == s.para { s.offset } else { 0 };
            let b = if p == e.para { e.offset } else { len };
            if a < b {
                self.doc
                    .delete(MAIN, Position::new(p, a), Position::new(p, b));
            }
        }
        s
    }

    /// Whether everything in `s..e` is text (or a paragraph mark) already
    /// deleted with Track Changes.
    pub(super) fn deleted(&self, s: Position, e: Position) -> bool {
        let paras = &self.doc.main.paras;
        let mut at = s;
        while at < e {
            let p = &paras[at.para];
            let rev = if at.offset < p.len() {
                p.chp_of_char(at.offset).rev
            } else {
                p.mark_chp.rev
            };
            if !quill_core::revision::is_deleted(rev) {
                return false;
            }
            let next = self.doc.main.next_char(at);
            if next == at {
                break;
            }
            at = next;
        }
        true
    }

    /// Whether the insertion point is on a row's end mark, where nothing
    /// can be typed.
    pub(super) fn on_row_mark(&self) -> bool {
        self.doc.main.paras[self.sel.focus.para].mark == Mark::RowEnd
    }

    /// Tab in a table: select the next cell's text (Shift+Tab: the
    /// previous one). Tab in the last cell adds a row, as in the original.
    pub(super) fn next_cell(&mut self, back: bool) {
        let paras = &self.doc.main.paras;
        let p = self.sel.focus.para;
        let Some(row) = table::row_at(paras, p) else {
            return;
        };
        // Tab stays in this table: the rows beside this one are the ones
        // as deep.
        let level = paras[row.end].pap.level();
        let k = row.cell_of(p).unwrap_or(row.cells.len());
        let target = if back {
            if k > 0 {
                Some(row.cells[k - 1])
            } else {
                row.first
                    .checked_sub(1)
                    .and_then(|q| table::row_at_level(paras, q, level))
                    .filter(|r| paras[r.end].pap.level() == level)
                    .and_then(|r| r.cells.last().copied())
            }
        } else if k + 1 < row.cells.len() {
            Some(row.cells[k + 1])
        } else if let Some(r) = table::row_at_level(paras, row.end + 1, level) {
            r.cells.first().copied()
        } else {
            let props = table::row_props(paras, &row, self.column_twips(row.first));
            self.doc.begin("Insert Row", self.sel);
            self.doc.insert_rows(MAIN, &row, true, 1, &props);
            let sel = Selection::caret(Position::new(row.end + 1, 0));
            self.finish_edit(sel);
            return;
        };
        if let Some((a, b)) = target {
            let len = self.doc.main.paras[b].len();
            self.sel = Selection::new(Position::new(a, 0), Position::new(b, len));
            self.goal_x = None;
            self.typing_chp = None;
            self.doc.break_typing();
            self.reset_blink();
        }
    }

    pub(super) fn finish_edit(&mut self, sel: Selection) {
        self.status = None;
        self.note_edit(sel.focus);
        self.sel = sel;
        self.doc.commit(sel);
        self.upstream = false;
        self.goal_x = None;
        self.reset_blink();
    }

    /// Hidden deleted runs have two logical ends at one visual caret stop.
    /// Use their right edge consistently so reversing Shift+Arrow can collapse
    /// a selection instead of leaving a selection containing only hidden text.
    fn revision_edge(&self, mut pos: Position, forward: bool) -> Position {
        if self.show_changes {
            return pos;
        }
        let p = &self.doc.main.paras[pos.para];
        if forward {
            for (a, b, chp) in p.run_bounds() {
                if b <= pos.offset {
                    continue;
                }
                if a <= pos.offset && quill_core::revision::is_deleted(chp.rev) {
                    pos.offset = b;
                } else {
                    break;
                }
            }
        } else {
            let mut b = p.len();
            for run in p.runs.iter().rev() {
                let a = b - run.len;
                if a < pos.offset {
                    if quill_core::revision::is_deleted(run.chp.rev) {
                        pos.offset = a;
                    } else {
                        break;
                    }
                }
                b = a;
            }
        }
        pos
    }

    fn move_character(&mut self, forward: bool, extend: bool) {
        let from = self.revision_edge(self.sel.focus, true);
        let next = if forward {
            self.doc.main.next_char(from)
        } else {
            self.doc.main.prev_char(self.revision_edge(from, false))
        };
        let next = self.revision_edge(next, true);
        if extend {
            self.sel.anchor = self.revision_edge(self.sel.anchor, true);
        }
        self.set_caret(next, extend);
    }

    pub(super) fn set_caret(&mut self, pos: Position, extend: bool) {
        // Outline view steps over collapsed paragraphs.
        let pos = if self.outline_hidden(pos.para) {
            let n = self.doc.main.paras.len();
            let down = pos.para > self.sel.focus.para;
            let next = if down {
                (pos.para..n).find(|&j| !self.outline_hidden(j))
            } else {
                (0..pos.para).rev().find(|&j| !self.outline_hidden(j))
            };
            match next {
                Some(j) if down => Position::new(j, 0),
                Some(j) => Position::new(j, self.doc.main.paras[j].len()),
                None => self.sel.focus,
            }
        } else {
            pos
        };
        self.sel = if extend {
            Selection::new(self.sel.anchor, pos)
        } else {
            Selection::caret(pos)
        };
        self.typing_chp = None;
        self.doc.break_typing();
        self.status = None;
        self.reset_blink();
    }

    pub(super) fn line_move(&mut self, down: bool, extend: bool) {
        let pos = self.sel.focus;
        let (x, _, _) = self.caret_geometry(pos, self.upstream);
        let goal = *self.goal_x.get_or_insert(x);
        let p = pos.para;
        let l = self.layout(p);
        let li = l.line_of(pos.offset, self.upstream);
        let cell =
            table::row_at(&self.doc.main.paras, p).and_then(|r| r.cell_of(p).map(|k| r.cells[k]));
        let (new, up) = if down && li + 1 < l.lines.len() {
            let (o, up) = l.hit(li + 1, goal - self.xs[p]);
            (Position::new(p, o), up)
        } else if !down && li > 0 {
            let (o, up) = l.hit(li - 1, goal - self.xs[p]);
            (Position::new(p, o), up)
        } else if let Some((a, b)) = cell.filter(|&(a, b)| if down { p < b } else { p > a }) {
            // Another paragraph in the same cell.
            let _ = (a, b);
            let q = if down { p + 1 } else { p - 1 };
            let ql = self.layout(q);
            let line = if down { 0 } else { ql.lines.len() - 1 };
            let (o, up) = ql.hit(line, goal - self.xs[q]);
            (Position::new(q, o), up)
        } else {
            // Across to the next block down the column, under the same x:
            // in a table that is the row above or below.
            let b = &self.blocks[self.block_at(self.tops[p])];
            let y = if down {
                b.top + b.height + 0.5
            } else {
                b.top - 0.5
            };
            if y < 0.0 {
                (Position::new(0, 0), false)
            } else if y >= self.tops[self.layouts.len()] {
                (self.doc.main.end(), false)
            } else {
                self.hit_doc(goal, y)
            }
        };
        let keep = self.goal_x;
        self.set_caret(new, extend);
        self.upstream = up;
        self.goal_x = keep;
    }

    pub(super) fn line_edge(&mut self, end: bool, extend: bool) {
        let pos = self.sel.focus;
        let l = self.layout(pos.para);
        let li = l.line_of(pos.offset, self.upstream);
        let line = &l.lines[li];
        let last = li + 1 == l.lines.len();
        let offset = if !end {
            line.start
        } else if last {
            self.doc.main.paras[pos.para].len()
        } else {
            line.end
        };
        self.set_caret(Position::new(pos.para, offset), extend);
        self.upstream = end && !last;
    }

    /// Run a command. Returns what the application should do next.
    pub fn exec(&mut self, cmd: Cmd, extend: bool, clip: &mut Clipboard) -> Effect {
        if !self.captions {
            return self.exec_inner(cmd, extend, clip);
        }
        // The comments pane: a comment's box holds only its own text.
        if matches!(cmd, Cmd::SelectAll) {
            let (start, end) = self.comment_body(self.sel.focus.para);
            self.sel = Selection::new(start, end);
            self.reset_blink();
            return Effect::None;
        }
        let Some(cmd) = self.contain_delete(cmd) else {
            return Effect::None;
        };
        let ids = self.comment_ids();
        let effect = self.exec_inner(cmd, extend, clip);
        if matches!(cmd, Cmd::Paste | Cmd::PasteText) && self.comment_ids() != ids {
            // Pasted text carried comment marks: put it back as it was.
            self.exec_inner(Cmd::Undo, false, clip);
        }
        self.confine_to_comment();
        effect
    }

    fn exec_inner(&mut self, cmd: Cmd, extend: bool, clip: &mut Clipboard) -> Effect {
        use Cmd::*;
        let story = &self.doc.main;
        let focus = self.sel.focus;
        let collapse_to =
            |sel: Selection, forward: bool| if forward { sel.end() } else { sel.start() };
        match cmd {
            New => return Effect::NewWindow,
            Open => return Effect::Open,
            Close => return Effect::Close,
            Save => return Effect::Save,
            SaveAs => return Effect::SaveAs,
            SummaryInfo => return Effect::Dialog(DialogId::SummaryInfo),
            CharacterDialog => return Effect::Dialog(DialogId::Character),
            ParagraphDialog => return Effect::Dialog(DialogId::Paragraph),
            SectionDialog => return Effect::Dialog(DialogId::Section),
            PrintPreview => return Effect::NativePreview,
            Print => return Effect::Print,
            SavePdf => return Effect::SavePdf,
            PageSetup => return Effect::Dialog(DialogId::PageSetup),
            DocumentDialog => return Effect::Dialog(DialogId::Document),
            BorderDialog => return Effect::Dialog(DialogId::Border),
            InsertTable => return Effect::Dialog(DialogId::InsertTable),
            TableCells => return Effect::Dialog(DialogId::TableCells),
            TableLayout => return Effect::Dialog(DialogId::TableLayout),
            SelectTable => self.select_table(),
            ZoomIn => return Effect::Zoom(1),
            LookUp => return Effect::LookUp,
            ZoomOut => return Effect::Zoom(-1),
            Zoom100 => return Effect::Zoom(0),
            InsertRowAbove => crate::table_dialogs::insert_rows(self, false),
            InsertRowBelow => crate::table_dialogs::insert_rows(self, true),
            InsertColumnLeft => crate::table_dialogs::insert_columns(self, false),
            InsertColumnRight => crate::table_dialogs::insert_columns(self, true),
            ConvertToTable => crate::table_dialogs::convert_command(self),
            UpdateToc => self.update_toc(),
            MakeTocEditable => self.make_toc_editable(),
            StyleDialog => return Effect::Dialog(DialogId::Style),
            Preferences => return Effect::Dialog(DialogId::Preferences),
            DefaultFont => return Effect::Dialog(DialogId::DefaultFont),
            ToolbarMenu => return Effect::Dialog(DialogId::ToolbarPreferences),
            Find => return Effect::Dialog(DialogId::Find),
            Replace => return Effect::Dialog(DialogId::Replace),
            GoTo => return Effect::Dialog(DialogId::GoTo),
            Glossary => return Effect::Dialog(DialogId::Glossary),
            WordCount => return Effect::Dialog(DialogId::WordCount),
            ChangeCase if !self.sel.is_empty() => return Effect::Dialog(DialogId::ChangeCase),
            Renumber => return Effect::Dialog(DialogId::Renumber),
            ShowClipboard => return Effect::Dialog(DialogId::ShowClipboard),
            FindAgain => return Effect::FindAgain,
            SplitWindow => return Effect::Split,
            InsertFootnote => return Effect::Dialog(DialogId::Footnote),
            Hyperlink => return Effect::Dialog(DialogId::Hyperlink),
            RemoveHyperlink => self.remove_links(),
            Spelling => return Effect::Dialog(DialogId::Spelling),
            CommandsDialog => return Effect::Dialog(DialogId::Commands),
            PrintMerge => return Effect::Dialog(DialogId::PrintMerge),
            PrintMergeHelper => {
                return if quill_core::merge::data_instruction(&self.doc).is_some() {
                    Effect::Dialog(DialogId::MergeHelper)
                } else {
                    Effect::ChooseMergeData
                };
            }
            Hyphenation => return Effect::Dialog(DialogId::Hyphenation),
            Thesaurus => return Effect::Dialog(DialogId::Thesaurus),
            InsertToc => return Effect::Dialog(DialogId::Toc),
            InsertIndex => return Effect::Dialog(DialogId::Index),
            IndexEntry => self.insert_entry('i'),
            TocEntry => self.insert_entry('c'),
            ViewFootnotes => return Effect::Pane(StoryId::Notes),
            InsertAnnotation => {
                let id = self.insert_annotation();
                return Effect::OpenNote(StoryId::Annotations, id);
            }
            ReplyToAnnotation => return Effect::ReplyToAnnotation,
            ResolveAnnotation => return Effect::ResolveAnnotation,
            DeleteAnnotation => return Effect::DeleteAnnotation,
            ViewAnnotations => {
                return match self.next_annotation() {
                    Some(id) => Effect::OpenNote(StoryId::Annotations, id),
                    None => Effect::Pane(StoryId::Annotations),
                };
            }
            NewWindow => return Effect::Mirror,
            InsertPicture => return Effect::InsertPicture,
            FrameDialog => return Effect::Dialog(DialogId::Frame),
            DropCap => return Effect::Dialog(DialogId::DropCap),
            InsertFrame => {
                // Frame the selected paragraphs where they are, and show
                // them placed.
                self.format_paras("Insert Frame", |p| {
                    if p.frame.is_none() && !p.in_table {
                        p.frame = Some(quill_core::Frame::default());
                    }
                });
                self.page_view = true;
            }
            GoBack => self.go_back(),
            InsertGlossaryEntry => crate::glossary::expand(self),
            InsertDate => {
                let t = crate::clock::now().date_normal();
                self.type_text_raw(&t);
            }
            InsertTime => {
                let t = crate::clock::now().time();
                self.type_text_raw(&t);
            }
            Bullet => self.bullet(),
            Uppercase => self.change_case(quill_core::tools::Case::Upper),
            Lowercase => self.change_case(quill_core::tools::Case::Lower),
            TitleCase => self.change_case(quill_core::tools::Case::Title),
            SentenceCase => self.change_case(quill_core::tools::Case::Sentence),
            ToggleCase => self.change_case(quill_core::tools::Case::Toggle),
            CycleCase => self.cycle_case(),
            Sort | SortDescending => self.sort(cmd == SortDescending),
            Calculate => self.calculate(clip),
            RepaginateNow => self.invalidate_layout(),
            Quit => return Effect::Quit,

            Left if !extend && !self.sel.is_empty() => self.set_caret(
                self.revision_edge(collapse_to(self.sel, false), true),
                false,
            ),
            Right if !extend && !self.sel.is_empty() => {
                self.set_caret(self.revision_edge(collapse_to(self.sel, true), true), false)
            }
            Left => self.move_character(false, extend),
            Right => self.move_character(true, extend),
            WordLeft => self.set_caret(story.prev_word(focus), extend),
            WordRight => self.set_caret(story.next_word(focus), extend),
            ParaUp => self.set_caret(story.prev_paragraph(focus), extend),
            ParaDown => self.set_caret(story.next_paragraph(focus), extend),
            DocStart => self.set_caret(story.start(), extend),
            DocEnd => self.set_caret(story.end(), extend),
            Up => self.line_move(false, extend),
            Down => self.line_move(true, extend),
            LineStart => self.line_edge(false, extend),
            LineEnd => self.line_edge(true, extend),
            PageUp | PageDown => {
                let page = (self.area.h - 24.0).max(24.0);
                let dy = if cmd == PageUp { -page } else { page };
                let (x, y, _) = self.caret_geometry(focus, self.upstream);
                let (wx, wy) = self.to_window(x, y);
                self.scroll_by(dy);
                let (pos, up) = self.hit(wx, (wy + dy).max(self.area.y));
                self.set_caret(pos, extend);
                self.upstream = up;
                return Effect::None;
            }

            SelectAll => {
                self.sel = Selection::new(story.start(), story.end());
                self.reset_blink();
            }
            Backspace | DeleteForward | DeletePrevWord | DeleteNextWord => {
                let (mut s, mut e) = self.sel.range();
                let tracking = self.doc.props.track_changes;
                if s == e {
                    match cmd {
                        Backspace => s = story.prev_char(s),
                        DeletePrevWord => s = story.prev_word(s),
                        DeleteForward => e = story.next_char(e),
                        _ => e = story.next_word(e),
                    }
                    // Text deleted with Track Changes stays where it was,
                    // struck through or hidden. Step over it while tracking
                    // (it is deleted already) or while it is hidden (the
                    // key would do nothing you can see), so the key reaches
                    // text that is still there.
                    let step_over = tracking || !self.show_changes;
                    while step_over && cmd == Backspace && s != story.start() && self.deleted(s, e)
                    {
                        s = story.prev_char(s);
                    }
                    while step_over
                        && cmd == DeleteForward
                        && e != story.end()
                        && self.deleted(s, e)
                    {
                        e = story.next_char(e);
                    }
                    // Without tracking, that one character goes for good
                    // and the hidden deletion beside it stays as it was.
                    if step_over && !tracking {
                        match cmd {
                            Backspace => e = story.next_char(s),
                            DeleteForward => s = story.prev_char(e),
                            _ => {}
                        }
                        if self.deleted(s, e) {
                            return Effect::None;
                        }
                    }
                }
                // One key over a cell or table edge does nothing.
                if s == e || (self.sel.is_empty() && self.crosses_table(s, e)) {
                    return Effect::None;
                }
                self.doc.begin_typing(self.sel);
                let forward = matches!(cmd, DeleteForward | DeleteNextWord);
                let at = if tracking && forward && !self.crosses_table(s, e) {
                    // The deleted text is still there: carry on after it.
                    self.doc.delete_and_end(MAIN, s, e)
                } else if self.sel.is_empty() {
                    self.delete_range(s, e)
                } else {
                    self.delete_selection()
                };
                self.typing_chp = None;
                self.finish_edit(Selection::caret(at));
            }
            NewParagraph if self.on_row_mark() => {}
            NewParagraph => {
                self.doc.begin_typing(self.sel);
                let at = self.delete_selection();
                let style = self.doc.main.paras[at.para].pap.style;
                let typing = self
                    .typing_chp
                    .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
                let at = self.doc.split_paragraph_typing(MAIN, at, Some(typing));
                // In Outline view a new paragraph after a heading is a
                // heading of the same level.
                if self.outline
                    && self.doc.styles.heading_level(style).is_some()
                    && self.doc.main.paras[at.para].pap.style != style
                {
                    self.doc.apply_style(MAIN, at.para, at.para, style);
                }
                self.finish_edit(Selection::caret(at));
            }
            LineBreak => self.type_special(chars::LINE_BREAK),
            // A break can't go inside a table cell: say so rather than
            // do nothing, or break only the line.
            PageBreak | SectionBreak if self.doc.main.paras[focus.para].pap.in_table => {
                self.status = Some(format!(
                    "A {} can't go inside a table.",
                    if cmd == PageBreak {
                        "page break"
                    } else {
                        "section break"
                    }
                ));
            }
            PageBreak => self.type_special(chars::PAGE_BREAK),
            SectionBreak => {
                self.doc.begin("Section Break", self.sel);
                let at = self.delete_selection();
                let at = self.doc.insert_section_break(at);
                self.finish_edit(Selection::caret(at));
            }
            Tab | PrevCell if self.doc.main.paras[focus.para].pap.in_table => {
                self.next_cell(cmd == PrevCell);
            }
            Tab | TabChar => self.type_special(chars::TAB),
            PrevCell => {}
            NonbreakingSpace => self.type_special(chars::NONBREAKING_SPACE),
            OptionalHyphen => self.type_special(chars::OPTIONAL_HYPHEN),
            NonbreakingHyphen => self.type_special(chars::NONBREAKING_HYPHEN),

            Undo => {
                // A format chosen at the insertion point, with nothing
                // typed in it yet, is undone first.
                if self.caret_format_pending()
                    && let Some(before) = self.caret_formats.pop()
                {
                    self.typing_chp = before;
                    return Effect::None;
                }
                self.caret_formats.clear();
                if let Some(sel) = self.doc.undo() {
                    self.sel = sel;
                    self.typing_chp = None;
                    self.reset_blink();
                }
            }
            Repeat => self.repeat_last(clip),
            Cut | Copy => {
                if self.sel.is_empty() {
                    return Effect::None;
                }
                let (s, e) = self.sel.range();
                let block = self
                    .cell_block()
                    .filter(|b| !b.whole_table(&self.doc.main.paras));
                let frag = match &block {
                    Some(b) => {
                        let tw = self.column_twips(s.para);
                        self.doc.copy_cells(MAIN, &b.rows, b.from, b.to, tw)
                    }
                    None => self.doc.copy(MAIN, s, e),
                };
                clip.put(&self.doc, frag);
                if cmd == Cut {
                    self.doc.begin("Cut", self.sel);
                    let at = match &block {
                        Some(b) => self.clear_cells(b),
                        None => self.delete_range(s, e),
                    };
                    self.finish_edit(Selection::caret(at));
                }
            }
            Paste => {
                let Some(content) = clip.get() else {
                    return Effect::None;
                };
                if let Clip::Picture(bytes) = content {
                    // Bytes that are not a picture Quill can read paste
                    // nothing, and leave the selection alone.
                    if let Some(pic) = quill_formats::images::picture(bytes) {
                        self.insert_picture(pic);
                    }
                    return Effect::None;
                }
                if let Clip::Plain(text) = &content
                    && !self.room_for(quill_core::limits::Size::of_text(text))
                {
                    return Effect::None;
                }
                self.doc.begin("Paste", self.sel);
                if let Some(block) = self
                    .cell_block()
                    .filter(|b| !b.whole_table(&self.doc.main.paras))
                {
                    let at = self.paste_into_cells(&block, content);
                    self.finish_edit(Selection::caret(at));
                    self.undo_if_too_large("paste");
                    return Effect::None;
                }
                let at = self.delete_selection();
                let end = match content {
                    Clip::Rich(rich) => {
                        let fragment = rich.prepare(&mut self.doc);
                        self.doc.insert_fragment(MAIN, at, &fragment)
                    }
                    Clip::Plain(text) => {
                        // Plain text takes the format of the insertion point.
                        let chp = self.doc.main.paras[at.para].chp_at(at.offset);
                        self.doc.insert_plain(MAIN, at, &text, chp)
                    }
                    Clip::Picture(_) => at,
                };
                self.finish_edit(Selection::caret(end));
                self.undo_if_too_large("paste");
            }
            PasteSpecial => return Effect::Dialog(DialogId::PasteSpecial),
            PasteText => self.paste_as(crate::paste_special::Form::Unformatted, clip),
            Clear => {
                if !self.sel.is_empty() {
                    self.doc.begin("Clear", self.sel);
                    let at = self.delete_selection();
                    self.finish_edit(Selection::caret(at));
                }
            }
            NormalView | PageLayoutView => {
                self.set_outline(false);
                self.page_view = cmd == PageLayoutView;
                self.scroll_to_caret();
                return Effect::None;
            }
            OutlineView => {
                self.set_outline(true);
                self.scroll_to_caret();
                return Effect::None;
            }
            MarkdownSource => return Effect::MarkdownSource,
            MarkdownWysiwyg => return Effect::MarkdownWysiwyg,
            OutlinePrefix => self.outline_prefix = true,
            PromoteHeading => self.promote(),
            DemoteHeading => self.demote(),
            MakeBodyText => self.demote_to_body(),
            MoveHeadingUp | MoveHeadingDown if self.outline => {
                self.move_paragraphs(cmd == MoveHeadingDown)
            }
            ExpandSubtext if self.outline => self.expand(),
            CollapseSubtext if self.outline => self.collapse(),
            ShowHeading1 | ShowHeading2 | ShowHeading3 | ShowHeading4 | ShowHeading5
            | ShowHeading6 | ShowHeading7 | ShowHeading8 | ShowHeading9
                if self.outline =>
            {
                let n = format!("{cmd:?}")
                    .trim_start_matches("ShowHeading")
                    .parse()
                    .unwrap_or(1);
                self.show_level(n);
            }
            ShowAllHeadings if self.outline => self.show_level(9),
            ShowBodyText if self.outline => self.show_level(0),
            FirstLineOnly if self.outline => {
                self.first_line_only = !self.first_line_only;
                self.outline_dirty = true;
            }
            ShowFormatting if self.outline => {
                self.show_formatting = !self.show_formatting;
                self.invalidate_layout();
            }
            ShowMarks => {
                self.show_marks = !self.show_marks;
                self.invalidate_layout();
            }
            ShowChanges => {
                self.show_changes = !self.show_changes;
                self.invalidate_layout();
            }
            TrackChanges => self.toggle_tracking(),
            ReviewChanges => return Effect::Dialog(DialogId::ReviewChanges),
            NextChange | PreviousChange => {
                if !self.go_to_change(cmd == PreviousChange) {
                    self.status = Some("There are no more tracked changes that way.".into());
                }
            }
            AcceptChange | RejectChange => {
                if !self.resolve_change(cmd == AcceptChange) {
                    self.status = Some("There's no tracked change here.".into());
                }
            }
            AcceptAllChanges | RejectAllChanges => self.resolve_all(cmd == AcceptAllChanges),
            _ => {
                if self.format(cmd) {
                    self.repeat = Some(Repeatable::Command(cmd));
                }
            }
        }
        if cmd.is_movement() {
            if !matches!(cmd, Up | Down) {
                self.goal_x = None;
            }
            if !matches!(cmd, LineEnd | Up | Down) {
                self.upstream = false;
            }
        }
        Effect::None
    }

    pub(super) fn type_special(&mut self, c: char) {
        self.type_text_raw(&c.to_string());
    }

    /// Like `type_text` but keeps control characters such as line breaks.
    pub(super) fn type_text_raw(&mut self, text: &str) {
        self.doc.begin_typing(self.sel);
        let at = self.delete_selection();
        let chp = self
            .typing_chp
            .unwrap_or_else(|| self.doc.main.paras[at.para].chp_at(at.offset));
        let end = self.doc.insert_text(MAIN, at, text, chp);
        self.finish_edit(Selection::caret(end));
    }

    pub(super) fn repeat_last(&mut self, clip: &mut Clipboard) {
        if self.doc.redo_label().is_some() {
            if let Some(sel) = self.doc.redo() {
                self.sel = sel;
                self.reset_blink();
            }
            return;
        }
        match self.repeat.clone() {
            Some(Repeatable::Text(t)) => {
                self.doc.break_typing();
                self.type_text_raw(&t);
            }
            Some(Repeatable::Command(c)) => {
                self.exec(c, false, clip);
            }
            None => {}
        }
    }

    /// Label for the Edit menu's first item.
    /// Whether a format chosen at the insertion point is waiting to be
    /// undone before the document's own history.
    fn caret_format_pending(&self) -> bool {
        self.typing_chp.is_some() && !self.caret_formats.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        self.caret_format_pending() || self.doc.undo_label().is_some()
    }

    pub fn undo_label(&self) -> String {
        if self.caret_format_pending() {
            return "Undo Formatting".to_string();
        }
        match self.doc.undo_label() {
            Some(l) => format!("Undo {l}"),
            None => "Can't Undo".to_string(),
        }
    }

    pub fn repeat_label(&self) -> String {
        if let Some(l) = self.doc.redo_label() {
            return format!("Redo {l}");
        }
        match &self.repeat {
            Some(Repeatable::Text(_)) => "Repeat Typing".to_string(),
            Some(Repeatable::Command(c)) => format!("Repeat {}", c.name()),
            None => "Can't Repeat".to_string(),
        }
    }

    // ----- formatting -----

    /// The character format the selection shows in the formatting toolbar
    /// and menus: its first character's, or the insertion format.
    pub fn current_chp(&self) -> CharProps {
        if let Some(c) = self.typing_chp {
            return c;
        }
        let s = self.sel.start();
        let p = &self.doc.main.paras[s.para];
        if self.sel.is_empty() {
            p.chp_at(s.offset)
        } else {
            p.chp_of_char(s.offset)
        }
    }

    pub fn current_pap(&self) -> &ParaProps {
        &self.doc.main.paras[self.sel.start().para].pap
    }

    /// Apply a character change to the selection, or to the insertion
    /// format when nothing is selected.
    pub fn format_chars(&mut self, label: &'static str, f: impl Fn(&mut CharProps)) {
        if self.sel.is_empty() {
            let mut c = self.current_chp();
            f(&mut c);
            self.caret_formats.push(self.typing_chp);
            self.typing_chp = Some(c);
            return;
        }
        let (s, e) = self.sel.range();
        self.doc.begin(label, self.sel);
        match self.cell_block() {
            Some(block) => {
                for (a, b) in block.cells() {
                    let len = self.doc.main.paras[b].len();
                    self.doc
                        .format_chars(MAIN, Position::new(a, 0), Position::new(b, len), &f);
                }
            }
            None => self.doc.format_chars(MAIN, s, e, f),
        }
        self.finish_edit(self.sel);
    }

    /// Apply a paragraph change to the selected paragraphs. A selection
    /// that ends at the very start of a paragraph leaves that one alone.
    pub fn format_paras(&mut self, label: &'static str, mut f: impl FnMut(&mut ParaProps)) {
        let (first, last) = self.selected_paras();
        self.doc.begin(label, self.sel);
        match self.cell_block() {
            Some(block) => {
                for (a, b) in block.cells() {
                    self.doc.format_paras(MAIN, a, b, &mut f);
                }
            }
            None => self.doc.format_paras(MAIN, first, last, f),
        }
        let sel = self.sel;
        self.finish_edit(sel);
    }

    /// Formatting commands. Returns false for commands it does not handle.
    pub(super) fn format(&mut self, cmd: Cmd) -> bool {
        use Cmd::*;
        let cur = self.current_chp();
        const F: &str = "Formatting";
        const P: &str = "Paragraph Formatting";
        match cmd {
            Bold => self.format_chars(F, move |c| c.bold = !cur.bold),
            Italic => self.format_chars(F, move |c| c.italic = !cur.italic),
            Underline => toggle_underline(self, cur, quill_core::Underline::Single),
            WordUnderline => toggle_underline(self, cur, quill_core::Underline::Word),
            DoubleUnderline => toggle_underline(self, cur, quill_core::Underline::Double),
            DottedUnderline => toggle_underline(self, cur, quill_core::Underline::Dotted),
            Strikethru => self.format_chars(F, move |c| c.strike = !cur.strike),
            Outline => self.format_chars(F, move |c| c.outline = !cur.outline),
            Shadow => self.format_chars(F, move |c| c.shadow = !cur.shadow),
            SmallCaps => self.format_chars(F, move |c| c.small_caps = !cur.small_caps),
            AllCaps => self.format_chars(F, move |c| c.all_caps = !cur.all_caps),
            Hidden => self.format_chars(F, move |c| c.hidden = !cur.hidden),
            Superscript => self.format_chars(F, move |c| {
                c.position = if cur.position > 0 { 0 } else { 60 }
            }),
            Subscript => self.format_chars(F, move |c| {
                c.position = if cur.position < 0 { 0 } else { -40 }
            }),
            PlainText => self.format_chars(F, |c| c.clear_styles()),
            FontSizeUp => self.format_chars(F, |c| c.size = (c.size + 2).min(3200)),
            FontSizeDown => self.format_chars(F, |c| c.size = c.size.saturating_sub(2).max(8)),
            LargerFontSize => self.format_chars(F, |c| {
                let pt = c.size / 2;
                c.size = SIZE_STEPS
                    .iter()
                    .find(|&&s| s > pt)
                    .map_or(c.size, |s| s * 2);
            }),
            SmallerFontSize => self.format_chars(F, |c| {
                let pt = c.size.div_ceil(2);
                c.size = SIZE_STEPS
                    .iter()
                    .rev()
                    .find(|&&s| s < pt)
                    .map_or(c.size, |s| s * 2);
            }),
            AlignLeft => self.format_paras(P, |p| p.align = Align::Left),
            AlignCenter => self.format_paras(P, |p| p.align = Align::Center),
            AlignRight => self.format_paras(P, |p| p.align = Align::Right),
            Justify => self.format_paras(P, |p| p.align = Align::Justify),
            DoubleSpace => self.format_paras(P, |p| p.line = LineSpacing::AtLeast(480)),
            SpaceBefore12 => {
                self.format_paras(P, |p| p.before = if p.before == 240 { 0 } else { 240 })
            }
            FirstLineIndent => {
                self.format_paras(P, |p| p.first = if p.first == 720 { 0 } else { 720 })
            }
            HangingIndent => self.format_paras(P, |p| {
                p.left += 720;
                p.first = -720;
            }),
            Nest => self.format_paras(P, |p| p.left = (p.left / 720 + 1) * 720),
            Unnest => self.format_paras(P, |p| p.left = ((p.left - 1).max(0) / 720) * 720),
            NormalParagraph => {
                let (first, last) = self.selected_paras();
                let styles = self.doc.styles.clone();
                self.doc.begin(P, self.sel);
                self.doc.format_paras(MAIN, first, last, |p| {
                    let style = p.style;
                    *p = styles.get(style).pap.clone();
                });
                let sel = self.sel;
                self.finish_edit(sel);
            }
            RevertToStyle => {
                let (s, e) = self.sel.range();
                if s == e {
                    let style = self.doc.main.paras[s.para].pap.style;
                    self.typing_chp = Some(self.doc.styles.get(style).chp);
                    return true;
                }
                self.doc.begin(F, self.sel);
                for para in s.para..=e.para {
                    let style = self.doc.styles.get(self.doc.main.paras[para].pap.style).chp;
                    let ps = if para == s.para {
                        s
                    } else {
                        Position::new(para, 0)
                    };
                    let pe = if para == e.para {
                        e
                    } else {
                        Position::new(para, self.doc.main.paras[para].len())
                    };
                    self.doc.format_chars(MAIN, ps, pe, |c| {
                        let object = c.object;
                        *c = style;
                        c.object = object;
                    });
                }
                let sel = self.sel;
                self.finish_edit(sel);
            }
            _ => return false,
        }
        true
    }

    pub fn set_size(&mut self, half_points: u16) {
        self.format_chars("Formatting", move |c| c.size = half_points);
    }

    pub fn set_style(&mut self, style: u16) {
        let (first, last) = self.selected_paras();
        self.doc.begin("Style", self.sel);
        self.doc.apply_style(MAIN, first, last, style);
        let sel = self.sel;
        self.finish_edit(sel);
    }

    /// Start dragging something on the ruler. Returns the formats of the
    /// selected paragraphs as they are now, to apply the drag to.
    pub fn begin_ruler_drag(&mut self) -> Vec<ParaProps> {
        let (first, last) = self.selected_paras();
        self.doc.begin("Paragraph Formatting", self.sel);
        self.doc.main.paras[first..=last]
            .iter()
            .map(|p| p.pap.clone())
            .collect()
    }

    /// Show the ruler drag so far: each paragraph gets its starting format
    /// with `f` applied.
    pub fn ruler_drag(&mut self, start: &[ParaProps], f: impl Fn(&mut ParaProps, &ParaProps)) {
        let (first, last) = self.selected_paras();
        let mut i = 0;
        self.doc.format_paras(MAIN, first, last, |p| {
            if let Some(st) = start.get(i) {
                f(p, st);
            }
            i += 1;
        });
    }

    pub fn end_ruler_drag(&mut self) {
        let sel = self.sel;
        self.finish_edit(sel);
    }

    /// In a table: the current cell's text edge and width, and the row's
    /// cell boundaries, as window x positions (for the table ruler).
    pub fn table_ruler(&self) -> Option<(f32, f32, Vec<f32>)> {
        let p = self.sel.focus.para;
        // A table inside a cell has no table ruler of its own yet.
        let pap = &self.doc.main.paras.get(p)?.pap;
        if !pap.in_table || pap.nest > 0 || self.blocks.is_empty() {
            return None;
        }
        let b = &self.blocks[self.block_at(self.tops[p])];
        let row = b.row.as_ref()?;
        let k = row
            .cells
            .iter()
            .position(|c| (c.0..=c.1).contains(&p))
            .unwrap_or(row.cells.len() - 1);
        let (_, _, x0, x1) = row.cells[k];
        let pad = row.props.gap as f32 / 40.0;
        let dx = if self.page_view {
            self.on_page(b.top).1
        } else {
            0.0
        };
        let wx = |x: f32| self.area.x + self.text_x() + dx + x;
        let mut edges: Vec<f32> = row.cells.iter().map(|c| wx(c.2)).collect();
        edges.push(wx(row.cells.last().map_or(0.0, |c| c.3)));
        Some((wx(x0 + pad), (x1 - x0 - 2.0 * pad).max(1.0), edges))
    }

    /// Start dragging a column boundary on the table ruler: the rows it
    /// changes, with their settings now.
    pub fn begin_column_drag(&mut self) -> Vec<(usize, quill_core::RowProps)> {
        let Some(ts) = crate::table_dialogs::table_selection(self) else {
            return Vec::new();
        };
        self.doc.begin("Column Width", self.sel);
        let tw = self.column_twips(self.sel.focus.para);
        ts.rows
            .iter()
            .map(|r| (r.end, table::row_props(&self.doc.main.paras, r, tw)))
            .collect()
    }

    /// Move boundary `edge` (0 is the row's left edge) `dx` twips from
    /// where it started, in each row: the cell to its left grows, the ones
    /// to its right move along. The left edge moves the whole row.
    pub fn column_drag(&mut self, rows: &[(usize, quill_core::RowProps)], edge: usize, dx: i32) {
        for (end, start) in rows {
            let mut p = start.clone();
            if edge == 0 {
                p.indent = start.indent + dx;
            } else if let Some(c) = p.cells.get_mut(edge - 1) {
                c.width = (start.cells[edge - 1].width + dx).max(p.gap + 72);
            }
            self.doc.set_row(MAIN, *end, p);
        }
    }

    pub fn style_name(&self) -> &str {
        let id = self.current_pap().style;
        &self.doc.styles.get(id).name
    }
}

/// Curl straight quotes, as Preferences > "Smart" Quotes does.
pub(super) fn smart_quotes(text: &str, mut before: Option<char>) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let opens = before.is_none_or(|b| b.is_whitespace() || "([{\u{201C}\u{2018}".contains(b));
        let quote = match c {
            '"' if opens => '\u{201C}',
            '"' => '\u{201D}',
            '\'' if opens => '\u{2018}',
            '\'' => '\u{2019}',
            c => c,
        };
        out.push(quote);
        before = Some(quote);
    }
    out
}

fn toggle_underline(view: &mut DocView, current: CharProps, kind: Underline) {
    let next = if current.underline == kind {
        Underline::None
    } else {
        kind
    };
    view.format_chars("Formatting", move |props| props.underline = next);
}
