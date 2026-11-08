//! Dialog construction, state transitions, and tool-dialog outcomes.

use super::*;

impl WinState {
    fn finish_merge(&mut self, outcome: crate::merge_dialogs::Outcome) {
        use crate::merge_dialogs::Outcome as M;
        match outcome {
            M::NewFile(doc) => {
                self.form_letters += 1;
                self.pending_window = Some((doc, format!("Form Letters{}", self.form_letters)));
            }
            M::Print(doc) => {
                self.print_doc = Some(doc);
                self.pending_print = Some(crate::page_dialogs::PrintJob {
                    copies: 1,
                    pages: None,
                    pdf: false,
                });
            }
            M::Checked(errors) => {
                let message = crate::merge_dialogs::check_message(&errors);
                self.alert(&message, format_dialogs::Alert::Info);
            }
        }
    }

    fn continue_merge(&mut self, mut session: Box<crate::merge_dialogs::Session>) {
        match crate::merge_dialogs::advance(&mut session) {
            crate::merge_dialogs::Progress::Ask(request) => {
                let dialog = crate::merge_dialogs::ask(&request);
                self.dialogs
                    .push((Which::PrintMergeAsk(session, request), dialog));
            }
            crate::merge_dialogs::Progress::Done(outcome) => self.finish_merge(*outcome),
        }
    }

    /// Open one of the original's dialogs over this window.
    pub fn open_dialog(&mut self, id: DialogId, env: &Env) {
        if matches!(id, DialogId::ReviewChanges) {
            self.focus_main();
        }
        if id == DialogId::CharacterStyles && self.pane.as_ref().is_some_and(|p| p.focused) {
            self.alert("Define and change character styles in the main document. Existing styles can be applied in this pane.", format_dialogs::Alert::Info);
            return;
        }
        self.menu = None;
        self.font_names = env.font_names.to_vec();
        self.font_families = env.font_families.to_vec();
        // One modeless window at a time: a new one takes the old one's place.
        let modeless = matches!(
            id,
            DialogId::Find
                | DialogId::Replace
                | DialogId::Grep
                | DialogId::RegexSearch
                | DialogId::Sed
                | DialogId::Glossary
                | DialogId::ShowClipboard
                | DialogId::Spelling
                | DialogId::MergeHelper
                | DialogId::Hyphenation
                | DialogId::Thesaurus
                | DialogId::Preferences
                | DialogId::DefaultFont
                | DialogId::ToolbarPreferences
                | DialogId::ReviewChanges
        );
        if modeless {
            self.dialogs.retain(|(_, d)| !d.modeless);
        }
        // The language tools start from the text, or say why they cannot.
        use crate::lang_dialogs as lang;
        let tool = match id {
            DialogId::Spelling => {
                Some(lang::spelling(&mut self.view).map(|(d, s)| (d, Which::Spelling(s))))
            }
            DialogId::Hyphenation => {
                Some(lang::hyphenation(&mut self.view).map(|(d, s)| (d, Which::Hyphenation(s))))
            }
            DialogId::Thesaurus => {
                Some(lang::thesaurus(&mut self.view).map(|(d, s)| (d, Which::Thesaurus(s))))
            }
            _ => None,
        };
        if let Some(r) = tool {
            match r {
                Ok((d, which)) => self.dialogs.push((which, d)),
                Err(m) => self.alert(m, format_dialogs::Alert::Info),
            }
            return;
        }
        let (d, which) = match id {
            DialogId::Find | DialogId::Replace => {
                if let Some((Which::Find(st), d)) = self.dialogs.last_mut() {
                    crate::find_dialog::switch_mode(st, d, &self.view, id == DialogId::Replace);
                    return;
                }
                let (d, st) = crate::find_dialog::dialog(&self.view, id == DialogId::Replace);
                (d, Which::Find(st))
            }
            DialogId::GoTo => (crate::find_dialog::go_to(&self.view), Which::GoTo),
            DialogId::Grep => {
                let (d, st) = crate::advanced_dialog::grep(&self.view);
                (d, Which::Grep(st))
            }
            DialogId::RegexSearch => (
                crate::advanced_dialog::regex_search(&self.view, false),
                Which::RegexSearch(false),
            ),
            DialogId::Sed => (crate::advanced_dialog::sed(&self.view), Which::Sed),
            DialogId::Glossary => {
                let (d, st) = crate::glossary::dialog(&self.view);
                (d, Which::Glossary(st))
            }
            DialogId::WordCount => {
                let (d, counts) = crate::tool_dialogs::word_count(&self.view);
                let checks = crate::tool_dialogs::count_checks(&d);
                (d, Which::WordCount(counts, checks))
            }
            DialogId::ChangeCase => (crate::tool_dialogs::change_case(), Which::ChangeCase),
            DialogId::Footnote => (crate::tool_dialogs::footnote(), Which::Footnote),
            DialogId::Hyperlink => {
                let (d, st) = crate::link_dialog::dialog(&self.view);
                (d, Which::Hyperlink(st))
            }
            DialogId::Picture => {
                let Some((at, picture)) = self.view.selected_picture_copy() else {
                    return;
                };
                let (d, st) = crate::picture_dialog::dialog(at, picture);
                (d, Which::Picture(st))
            }
            DialogId::Toc => (crate::contents_dialogs::toc(), Which::Toc),
            DialogId::Index => (crate::contents_dialogs::index(), Which::Index),
            DialogId::Renumber => (crate::tool_dialogs::renumber(), Which::Renumber),
            DialogId::PasteSpecial => {
                // The window passes Quill's own clipboard in through
                // `open_paste_special`; this is the system's alone.
                let forms = Clipboard::new().forms();
                (
                    crate::paste_special::dialog(&forms),
                    Which::PasteSpecial(forms),
                )
            }
            DialogId::ShowClipboard => (
                crate::tool_dialogs::clipboard(),
                Which::Clipboard(String::new()),
            ),
            DialogId::Frame => (crate::frame_dialog::frame(&self.view), Which::Frame),
            DialogId::ReviewChanges => (
                crate::review_dialog::dialog(&self.view),
                Which::ReviewChanges,
            ),
            DialogId::DropCap => (
                crate::frame_dialog::drop_cap(&self.view, env.font_names),
                Which::DropCap,
            ),
            DialogId::Character => format_dialogs::character(&self.view, env.font_families),
            DialogId::Paragraph => format_dialogs::paragraph(&self.view),
            DialogId::SummaryInfo => format_dialogs::summary_info(&self.view),
            DialogId::Border => {
                let (d, st) = crate::border_dialog::border(&self.view);
                (d, Which::Border(st))
            }
            DialogId::Document => (crate::page_dialogs::document(&self.view), Which::Document),
            DialogId::InsertTable => {
                let (d, st) = crate::table_dialogs::insert_table(&self.view);
                (d, Which::InsertTable(st))
            }
            DialogId::TableCells => match crate::table_dialogs::table_cells(&self.view) {
                Some(d) => (d, Which::TableCells),
                None => return,
            },
            DialogId::TableLayout => match crate::table_dialogs::table_layout(&self.view) {
                Some(d) => (d, Which::TableLayout),
                None => return,
            },
            DialogId::PageSetup => (
                crate::page_dialogs::page_setup(&self.view),
                Which::PageSetup,
            ),
            DialogId::Section => (crate::page_dialogs::section(&self.view), Which::Section),
            DialogId::Print => (crate::page_dialogs::print(self.main_view()), Which::Print),
            DialogId::Preferences => (
                crate::prefs_dialog::preferences(0, env.font_families),
                Which::Preferences {
                    panel: 0,
                    families: env.font_families.to_vec(),
                    original: Box::new(crate::settings::get()),
                    draft: Box::new(crate::settings::get()),
                },
            ),
            DialogId::DefaultFont => (
                crate::prefs_dialog::preferences(
                    crate::prefs_dialog::DEFAULT_FONT_PANEL,
                    env.font_families,
                ),
                Which::Preferences {
                    panel: crate::prefs_dialog::DEFAULT_FONT_PANEL,
                    families: env.font_families.to_vec(),
                    original: Box::new(crate::settings::get()),
                    draft: Box::new(crate::settings::get()),
                },
            ),
            DialogId::ToolbarPreferences => (
                crate::prefs_dialog::preferences(
                    crate::prefs_dialog::TOOLBAR_PANEL,
                    env.font_families,
                ),
                Which::Preferences {
                    panel: crate::prefs_dialog::TOOLBAR_PANEL,
                    families: env.font_families.to_vec(),
                    original: Box::new(crate::settings::get()),
                    draft: Box::new(crate::settings::get()),
                },
            ),
            DialogId::CharacterStyles => (
                crate::character_style_dialog::dialog(&self.view),
                Which::CharacterStyles,
            ),
            DialogId::Style => {
                let (d, st) = crate::style_dialog::style(&self.view);
                (d, Which::Style(st))
            }
            // Opened above.
            DialogId::Spelling | DialogId::Hyphenation | DialogId::Thesaurus => return,
            DialogId::PrintMerge => (crate::merge_dialogs::print_merge(), Which::PrintMerge),
            DialogId::Commands => {
                let (d, st) = crate::commands_dialog::dialog(None);
                (d, Which::Commands(st))
            }
            DialogId::MergeHelper => match crate::merge_dialogs::helper(&self.view) {
                Ok((d, fields)) => (d, Which::MergeHelper(fields)),
                Err(m) => {
                    self.alert(&m, format_dialogs::Alert::Info);
                    return;
                }
            },
        };
        self.dialogs.push((which, d));
    }

    /// Whether the front dialog has the keyboard: it is modal, or a
    /// modeless window the user last clicked in.
    pub fn dialog_has_keys(&self) -> bool {
        self.dialogs
            .last()
            .is_some_and(|(_, d)| !(d.modeless && d.inactive))
    }

    pub(super) fn front_modeless(&self) -> bool {
        self.dialogs.last().is_some_and(|(_, d)| d.modeless)
    }

    /// Modal dialogs own the document window until they are accepted or
    /// canceled. Mouse and keyboard input already follow this rule; native
    /// menu-bar choices arrive through a separate AppKit callback and use
    /// this same gate.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Open or close the color palette beside color box `anchor`: the
    /// Character dialog's, or one of Preferences > Colors.
    fn toggle_color_popout(&mut self, anchor: &'static str) {
        let Some((which, d)) = self.dialogs.last_mut() else {
            return;
        };
        if d.popout.take().is_some() {
            let same = self.color_pop.as_ref().is_some_and(|(a, _)| *a == anchor);
            self.color_pop = None;
            if same {
                return;
            }
        }
        let prefs = matches!(which, Which::Preferences { .. });
        let start = if prefs {
            crate::prefs_dialog::box_color(d, anchor)
        } else {
            let Some(rgb) = d.swatch_rgb(anchor) else {
                return;
            };
            Some(quill_core::props::Rgb(rgb))
        };
        let st = crate::palette::PaletteState::new(start, &self.view.doc);
        // Preferences' colors are for every document, so Delete is only
        // for a document's own.
        d.popout = Some(crate::palette::popout(&st, anchor, !prefs));
        self.color_pop = Some((anchor, st));
    }

    /// Show the pop-out again after its state changed.
    fn refresh_color_popout(&mut self) {
        let (Some((which, d)), Some((anchor, st))) = (self.dialogs.last_mut(), &self.color_pop)
        else {
            return;
        };
        let delete = !matches!(which, Which::Preferences { .. });
        d.popout = Some(crate::palette::popout(st, anchor, delete));
    }

    /// Show color `c` in the box `anchor` and the HTML code beside it.
    fn show_chosen_color(&mut self, anchor: &'static str, c: quill_core::props::Rgb) {
        match self.dialogs.last_mut() {
            Some((Which::Preferences { .. }, d)) => {
                crate::prefs_dialog::show_color(d, anchor, Some(c));
            }
            Some((_, d)) => {
                d.set_swatch(anchor, c.0);
                d.set_label("color_hex", &crate::palette::hex(c));
            }
            None => {}
        }
    }

    /// A box of the color pop-out was pressed: choose its color, or on a
    /// double-click edit it.
    fn color_popout_box(&mut self, id: &'static str, double: bool) {
        let Some((anchor, st)) = &mut self.color_pop else {
            return;
        };
        let anchor = *anchor;
        let Some(pick) = crate::palette::pop_swatch(st, id, double) else {
            return;
        };
        match pick {
            crate::palette::Pick::Chose(c) => {
                self.show_chosen_color(anchor, c);
                self.refresh_color_popout();
            }
            crate::palette::Pick::Edit(slot) => {
                let (start, target) = crate::palette::edit_of(st, Some(slot));
                self.open_color_editor(start, target);
            }
        }
    }

    /// The pop-out's Edit Color… button: edit the color chosen.
    fn color_popout_edit(&mut self) {
        let Some((_, st)) = &self.color_pop else {
            return;
        };
        let (start, target) = crate::palette::edit_of(st, None);
        self.open_color_editor(start, target);
    }

    /// The pop-out's Delete: take one of the document's own colors out of
    /// the custom row. Text in it needs another color first, so ask.
    fn color_popout_delete(&mut self) {
        let Some((_, st)) = &mut self.color_pop else {
            return;
        };
        let Some(c) = crate::palette::deletable(st) else {
            return;
        };
        if !crate::palette::uses(&self.view.doc, c) {
            crate::palette::forget(st, c);
            self.refresh_color_popout();
            return;
        }
        if let Some((_, d)) = self.dialogs.last_mut() {
            d.popout = None;
        }
        self.color_pop = None;
        let message = format!(
            "Some text in this document is in color {}. Deleting the color changes that \
             text to black, or to a color you choose.",
            crate::palette::hex(c)
        );
        let d = Dialog::alert(
            &message,
            &[
                ("ok", "Use Black"),
                ("choose", "Choose Color…"),
                ("cancel", "Cancel"),
            ],
        );
        self.dialogs
            .push((Which::Alert(format_dialogs::Alert::DeleteColor(c)), d));
    }

    /// Delete color `old`: text in it takes `new`, as one step to undo,
    /// and the Character dialog shows `new` where it showed `old`.
    fn delete_color(&mut self, old: quill_core::props::Rgb, new: quill_core::props::Rgb) {
        self.view.replace_color(old, new);
        crate::palette::drop_edits_of(old);
        let Some((Which::Character(base), d)) = self.dialogs.last_mut() else {
            return;
        };
        if base.color == old {
            base.color = new;
        }
        if d.swatch_rgb("color") == Some(old.0) {
            d.set_swatch("color", new.0);
            d.set_label("color_hex", &crate::palette::hex(new));
        }
        // Back to the palette, without the color.
        self.toggle_color_popout("color");
    }

    fn open_color_editor(&mut self, start: quill_core::props::Rgb, target: crate::palette::Target) {
        if let Some((_, d)) = self.dialogs.last_mut() {
            d.popout = None;
        }
        self.edit_color(start, target);
    }

    /// Edit a color: the macOS Colors panel in a real window on the Mac,
    /// Quill's own Edit Color dialog elsewhere and in tests.
    fn edit_color(&mut self, start: quill_core::props::Rgb, target: crate::palette::Target) {
        #[cfg(all(target_os = "macos", not(test)))]
        if self.on_screen {
            if let Some(color) = crate::mac_color_panel::pick(start) {
                self.color_edited(target, color);
            }
            return;
        }
        let (editor, es) = crate::palette::editor(start, target);
        self.dialogs.push((Which::EditColor(es), editor));
    }

    /// Edit Color's OK: the color goes back to whatever asked for it.
    fn color_edited(&mut self, target: crate::palette::Target, color: quill_core::props::Rgb) {
        if let crate::palette::Target::Instead(old) = target {
            self.delete_color(old, color);
            return;
        }
        let anchor = self.color_pop.take().map(|(a, _)| a);
        match self.dialogs.last_mut() {
            Some((Which::Palette(ps), pd)) => {
                crate::palette::edited(ps, pd, target, color);
            }
            // A box of Preferences > Colors: saved with Preferences.
            Some((Which::Preferences { .. }, d)) => {
                if let Some(anchor) = anchor {
                    crate::prefs_dialog::show_color(d, anchor, Some(color));
                }
            }
            // From the pop-out beside a color: it becomes the
            // dialog's, and is remembered if the dialog's OK is.
            Some((_, cd)) if cd.swatch_rgb("color").is_some() => {
                crate::palette::hold_edit(target, color);
                cd.set_swatch("color", color.0);
                cd.set_label("color_hex", &crate::palette::hex(color));
            }
            _ => {}
        }
    }

    /// A slider in the open dialog moved under the pointer.
    pub(super) fn slider_moved(&mut self, o: Outcome) {
        if let Outcome::Changed(id) = o
            && let Some((Which::EditColor(st), d)) = self.dialogs.last_mut()
        {
            crate::palette::edit_changed(st, d, id);
        }
    }

    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(super) fn modal_dialog_open(&self) -> bool {
        self.dialogs.last().is_some_and(|(_, d)| !d.modeless)
    }

    /// Show an alert over everything else.
    pub(super) fn alert(&mut self, message: &str, alert: format_dialogs::Alert) {
        let buttons: &[(&'static str, &str)] = match alert {
            format_dialogs::Alert::Info => &[("ok", "OK")],
            _ => &[("ok", "OK"), ("cancel", "Cancel")],
        };
        let d = Dialog::alert(message, buttons);
        self.dialogs.push((Which::Alert(alert), d));
    }

    /// After a search: alert if it ran out, and remember the match.
    pub(super) fn after_search(
        &mut self,
        r: crate::find_dialog::Hit,
        search: &crate::find_dialog::Search,
    ) {
        use crate::find_dialog::{Hit, UP};
        let found = (r == Hit::Match).then(|| self.view.sel.range());
        if let Some((Which::Find(st), d)) = self.dialogs.last_mut() {
            st.found = found;
            crate::find_dialog::update(st, d, &self.view);
        }
        match r {
            Hit::Match => {}
            Hit::NotFound => self.alert(
                "The search text was not found.",
                format_dialogs::Alert::Info,
            ),
            Hit::ReachedEnd => {
                let m = if search.dir == UP {
                    "Reached the beginning of the document. Continue searching at the end?"
                } else {
                    "Reached the end of the document. Continue searching at the beginning?"
                };
                self.alert(
                    m,
                    format_dialogs::Alert::ContinueSearch(Box::new(search.clone())),
                );
            }
        }
    }

    pub fn open_paste_special(&mut self, forms: Vec<crate::paste_special::Form>) {
        self.menu = None;
        let mut d = crate::paste_special::dialog(&forms);
        if forms.is_empty() {
            d.text("The Clipboard is empty.", 20.0, 36.0);
            d.disable("ok");
        }
        self.dialogs.push((Which::PasteSpecial(forms), d));
    }

    pub fn set_clipboard_text(&mut self, text: String) {
        if let Some((Which::Clipboard(t), dialog)) = self.dialogs.last_mut() {
            *t = text;
            crate::tool_dialogs::update_clipboard(dialog, t);
        }
    }

    /// Find Again (⌘⌥A): search for what Find last looked for.
    pub fn find_again(&mut self) {
        let s = crate::find_dialog::last();
        let r = crate::find_dialog::find_next(&mut self.view, &s);
        self.after_search(r, &s);
    }

    /// The milestone 8 dialogs. Returns false for dialogs handled the
    /// older way, in `dialog_outcome`.
    pub(super) fn tool_outcome(&mut self, o: &Outcome, clip: &mut Clipboard) -> bool {
        use crate::find_dialog as fd;
        let Some((which, d)) = self.dialogs.last_mut() else {
            return false;
        };
        match which {
            Which::StyleCharacter(base) => match o {
                Outcome::Ok => {
                    let base = *base;
                    let n = self.dialogs.len();
                    let Some((Which::Style(st), _)) = self.dialogs.get(n.wrapping_sub(2)) else {
                        return true;
                    };
                    let mut pending = crate::style_dialog::character_view(st);
                    let Some(chp) =
                        format_dialogs::read_character(&self.dialogs[n - 1].1, &base, &mut pending)
                    else {
                        return true;
                    };
                    self.dialogs.pop();
                    crate::palette::drop_edits();
                    if let Some((Which::Style(st), parent)) = self.dialogs.last_mut() {
                        st.chp = chp;
                        st.fonts = pending.doc.fonts;
                        crate::style_dialog::update_description(st, parent, &self.view);
                    }
                }
                Outcome::Changed(id) => {
                    if *id == "color" && format_dialogs::wants_palette(d) {
                        // As in Format > Character: the system Colors panel on
                        // the Mac, and Cancel keeps the style's color.
                        #[cfg(all(target_os = "macos", not(test)))]
                        if self.on_screen {
                            let start = base.color;
                            let color = crate::mac_color_panel::pick(start).unwrap_or(start);
                            format_dialogs::show_color(d, Some(color));
                            return true;
                        }
                        let (palette, st) = crate::palette::palette(None, &self.view.doc);
                        self.dialogs.push((Which::Palette(st), palette));
                    } else {
                        format_dialogs::character_changed(d, id);
                    }
                }
                Outcome::Cancel => {
                    crate::palette::drop_edits();
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::CharacterStyleEdit(id) => match o {
                Outcome::Ok => {
                    let id = *id;
                    let n = self.dialogs.len();
                    let (rest, last) = self.dialogs.split_at_mut(n - 1);
                    let Some((Which::CharacterStyles, list)) = rest.last_mut() else {
                        self.dialogs.pop();
                        return true;
                    };
                    match crate::character_style_dialog::finish_editor(
                        &last[0].1,
                        list,
                        &mut self.view,
                        id,
                    ) {
                        Ok(_) => {
                            self.dialogs.pop();
                            crate::palette::drop_edits();
                        }
                        Err(why) => self.alert(why, format_dialogs::Alert::Info),
                    }
                }
                Outcome::Changed(id) => {
                    if *id == "color" && format_dialogs::wants_palette(d) {
                        // The system Colors panel on the Mac; Cancel keeps
                        // the style's color, or none.
                        #[cfg(all(target_os = "macos", not(test)))]
                        if self.on_screen {
                            let start = format_dialogs::color_named(&d.selected_text("color"));
                            let color = crate::mac_color_panel::pick(
                                start.unwrap_or(quill_core::props::Rgb::BLACK),
                            )
                            .or(start);
                            format_dialogs::show_color(d, color);
                            return true;
                        }
                        let (palette, st) = crate::palette::palette(None, &self.view.doc);
                        self.dialogs.push((Which::Palette(st), palette));
                    } else {
                        format_dialogs::character_changed(d, id);
                    }
                }
                Outcome::Cancel => {
                    crate::palette::drop_edits();
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::StyleParagraph(base) => match o {
                Outcome::Ok => {
                    let Some(pap) = format_dialogs::read_paragraph(d, base) else {
                        return true;
                    };
                    self.dialogs.pop();
                    if let Some((Which::Style(st), parent)) = self.dialogs.last_mut() {
                        st.pap = pap;
                        crate::style_dialog::update_description(st, parent, &self.view);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Find(st) => match o {
                Outcome::Button("mode") => {
                    let replace = !st.replace;
                    fd::switch_mode(st, d, &self.view, replace);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                Outcome::Changed(id) => {
                    if let fd::Ask::Format { replace, kind } = fd::changed(st, d, &self.view, id) {
                        let (spec_c, spec_p) = if replace {
                            (&st.search.rep_chp, &st.search.rep_pap)
                        } else {
                            (&st.search.find_chp, &st.search.find_pap)
                        };
                        let fd_dialog = match kind {
                            0 => fd::character_spec(&self.view, &self.font_families, spec_c),
                            1 => fd::paragraph_spec(&self.view, spec_p),
                            _ => fd::style_spec(&self.view, spec_p),
                        };
                        self.dialogs
                            .push((Which::FindFormat { replace, kind }, fd_dialog));
                    }
                }
                Outcome::Ok | Outcome::Button("findnext") => {
                    fd::update(st, d, &self.view);
                    let s = st.search.clone();
                    let r = fd::find_next(&mut self.view, &s);
                    self.after_search(r, &s);
                }
                Outcome::Button("replace") if st.replace => {
                    fd::update(st, d, &self.view);
                    let s = st.search.clone();
                    let r = fd::replace(st, &mut self.view, clip);
                    self.after_search(r, &s);
                }
                Outcome::Button("replaceall") if st.replace => {
                    fd::update(st, d, &self.view);
                    let (_, in_selection) = fd::replace_all(st, &mut self.view, clip);
                    fd::update(st, d, &self.view);
                    if in_selection {
                        self.alert("End of selection reached.", format_dialogs::Alert::Info);
                    }
                }
                _ => {}
            },
            Which::FindFormat { replace, kind } => match o {
                Outcome::Ok => {
                    let (replace, kind) = (*replace, *kind);
                    let n = self.dialogs.len();
                    let base = match self.dialogs.get(n.wrapping_sub(2)) {
                        Some((Which::Find(st), _)) => st.search.clone(),
                        _ => fd::Search::default(),
                    };
                    let d = &self.dialogs[n - 1].1;
                    let (cur_c, cur_p) = if replace {
                        (base.rep_chp, base.rep_pap)
                    } else {
                        (base.find_chp, base.find_pap)
                    };
                    let spec = match kind {
                        0 => fd::read_character_spec(d, &mut self.view).map(|c| (c, cur_p.clone())),
                        1 => fd::read_paragraph_spec(d).map(|mut p| {
                            p.style = cur_p.style;
                            (cur_c.clone(), p)
                        }),
                        _ => Some((cur_c.clone(), fd::read_style_spec(d, &cur_p))),
                    };
                    let Some((c, p)) = spec else {
                        return true;
                    };
                    self.dialogs.pop();
                    if let Some((Which::Find(st), d)) = self.dialogs.last_mut() {
                        if replace {
                            st.search.rep_chp = c;
                            st.search.rep_pap = p;
                        } else {
                            st.search.find_chp = c;
                            st.search.find_pap = p;
                        }
                        fd::update(st, d, &self.view);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                Outcome::Changed(id) if *kind == 0 => {
                    if *id == "color" && format_dialogs::wants_palette(d) {
                        #[cfg(all(target_os = "macos", not(test)))]
                        if self.on_screen {
                            let color = crate::mac_color_panel::pick(quill_core::props::Rgb::BLACK);
                            format_dialogs::show_color(d, color);
                            return true;
                        }
                        let (palette, st) = crate::palette::palette(None, &self.view.doc);
                        self.dialogs.push((Which::Palette(st), palette));
                    } else {
                        format_dialogs::character_changed(d, id);
                    }
                }
                _ => {}
            },
            Which::Alert(alert) => match o {
                Outcome::Ok => {
                    let alert = alert.clone();
                    self.dialogs.pop();
                    match alert {
                        format_dialogs::Alert::ContinueSearch(s) => {
                            let r = if fd::find_wrapped(&mut self.view, &s) {
                                fd::Hit::Match
                            } else {
                                fd::Hit::NotFound
                            };
                            self.after_search(r, &s);
                        }
                        format_dialogs::Alert::DeleteColor(c) => {
                            self.delete_color(c, quill_core::props::Rgb::BLACK);
                        }
                        format_dialogs::Alert::Info => {}
                    }
                }
                Outcome::Button("choose") => {
                    if let format_dialogs::Alert::DeleteColor(c) = *alert {
                        self.dialogs.pop();
                        let target = crate::palette::Target::Instead(c);
                        self.edit_color(quill_core::props::Rgb::BLACK, target);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::GoTo => match o {
                Outcome::Ok => {
                    if let Some(page) = fd::go_to_page(d, &self.view) {
                        self.dialogs.pop();
                        self.view.go_to_page(page);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Grep(st) => match o {
                Outcome::Ok | Outcome::Button("search") => {
                    crate::advanced_dialog::grep_search(st, d, &self.view);
                }
                Outcome::Changed("hits") | Outcome::Button("go") => {
                    crate::advanced_dialog::grep_go(st, d, &mut self.view);
                }
                Outcome::Changed(_) => {
                    crate::advanced_dialog::grep_search(st, d, &self.view);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::RegexSearch(replace) => match o {
                Outcome::Ok | Outcome::Button("findnext") => {
                    if d.is_enabled("findnext") {
                        crate::advanced_dialog::rx_find_next(d, &mut self.view, *replace);
                    }
                }
                Outcome::Button("mode") => {
                    *replace = !*replace;
                    crate::advanced_dialog::rx_switch(d, &self.view, *replace);
                }
                Outcome::Button("replace") if *replace => {
                    crate::advanced_dialog::rx_replace(d, &mut self.view);
                }
                Outcome::Button("replaceall") if *replace => {
                    crate::advanced_dialog::rx_replace_all(d, &mut self.view);
                }
                Outcome::Changed(_) => crate::advanced_dialog::rx_check(d, *replace),
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Sed => match o {
                Outcome::Ok | Outcome::Button("apply") => {
                    if d.is_enabled("apply") {
                        let _ = crate::advanced_dialog::sed_apply(d, &mut self.view);
                    }
                }
                Outcome::Changed("script") => crate::advanced_dialog::sed_check(d),
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Glossary(st) => {
                let close = match o {
                    Outcome::Cancel => true,
                    Outcome::Changed(id) => {
                        crate::glossary::changed(st, d, &self.view, id);
                        false
                    }
                    Outcome::Button(id) => crate::glossary::button(st, d, &mut self.view, id),
                    _ => false,
                };
                if close {
                    self.dialogs.pop();
                }
            }
            Which::Spelling(_) | Which::Hyphenation(_) | Which::Thesaurus(_) => {
                use crate::lang_dialogs::{self as lang, Action};
                let view = &mut self.view;
                let action = match (which, o) {
                    (_, Outcome::Cancel) => Action::Close(None),
                    (Which::Spelling(st), Outcome::Button(id)) => {
                        lang::spelling_button(st, d, view, id)
                    }
                    (Which::Spelling(st), Outcome::Ok) => {
                        lang::spelling_button(st, d, view, "ignore")
                    }
                    (Which::Spelling(st), Outcome::Changed(id)) => {
                        lang::spelling_changed(st, d, id);
                        Action::None
                    }
                    (Which::Hyphenation(st), Outcome::Button(id)) => {
                        lang::hyphenation_button(st, d, view, id)
                    }
                    (Which::Hyphenation(st), Outcome::Ok) => {
                        lang::hyphenation_button(st, d, view, "nochange")
                    }
                    (Which::Hyphenation(st), Outcome::Changed(id)) => {
                        lang::hyphenation_changed(st, d, view, id);
                        Action::None
                    }
                    (Which::Thesaurus(st), Outcome::Button(id)) => {
                        lang::thesaurus_button(st, d, view, id)
                    }
                    (Which::Thesaurus(st), Outcome::Ok) => {
                        lang::thesaurus_button(st, d, view, "lookup")
                    }
                    (Which::Thesaurus(st), Outcome::Changed(id)) => {
                        lang::thesaurus_changed(st, d, id);
                        Action::None
                    }
                    _ => Action::None,
                };
                match action {
                    Action::None => {}
                    Action::Close(message) => {
                        self.dialogs.pop();
                        if let Some(m) = message {
                            self.alert(m, format_dialogs::Alert::Info);
                        }
                    }
                    Action::Options => {
                        let d = crate::prefs_dialog::preferences(
                            crate::prefs_dialog::SPELLING_PANEL,
                            &self.font_families,
                        );
                        self.dialogs.push((
                            Which::Preferences {
                                panel: crate::prefs_dialog::SPELLING_PANEL,
                                families: self.font_families.clone(),
                                original: Box::new(crate::settings::get()),
                                draft: Box::new(crate::settings::get()),
                            },
                            d,
                        ));
                    }
                }
            }
            Which::Customize(st) => {
                let close = match o {
                    Outcome::Cancel => true,
                    Outcome::Changed(id) => {
                        crate::customize::changed(st, d, id);
                        false
                    }
                    Outcome::Button(id) => crate::customize::button(st, d, id),
                    _ => false,
                };
                if close {
                    self.dialogs.pop();
                }
            }
            Which::Commands(st) => {
                use crate::commands_dialog::{self as cd, Action};
                let action = match o {
                    Outcome::Cancel => Action::Close,
                    Outcome::Clicked(id, x, y, _) => {
                        cd::clicked(st, d, id, *x, *y);
                        Action::None
                    }
                    Outcome::Changed(id) => {
                        cd::changed(st, d, id);
                        Action::None
                    }
                    Outcome::Button(id) => cd::button(st, d, id),
                    _ => Action::None,
                };
                match action {
                    Action::None => {}
                    Action::Close => {
                        self.dialogs.pop();
                    }
                    Action::DoIt(cmd) => {
                        self.dialogs.pop();
                        self.pending_cmd = Some(cmd);
                    }
                    Action::OpenSettings => self.pending_settings = Some(true),
                    Action::SaveSettings => self.pending_settings = Some(false),
                    Action::List => {
                        self.pending_window = Some((cd::list_document(), "Commands".into()));
                    }
                    Action::Reset => {
                        cd::reset();
                        if let Some((Which::Commands(st), d)) = self.dialogs.last_mut() {
                            cd::refresh(st, d, None);
                        }
                        self.alert(
                            "The menus, keys and toolbar are back to their defaults.",
                            format_dialogs::Alert::Info,
                        );
                    }
                }
            }
            Which::PrintMerge => match o {
                Outcome::Changed(id) => crate::merge_dialogs::print_merge_changed(d, id),
                Outcome::Ok => {
                    let result = crate::merge_dialogs::start(d, &self.view);
                    self.dialogs.pop();
                    match result {
                        Err(m) => self.alert(&m, format_dialogs::Alert::Info),
                        Ok(session) => self.continue_merge(Box::new(session)),
                    }
                }
                Outcome::Button("helper") => {
                    self.dialogs.pop();
                    match crate::merge_dialogs::helper(&self.view) {
                        Ok((d, fields)) => self.dialogs.push((Which::MergeHelper(fields), d)),
                        Err(m) => self.alert(&m, format_dialogs::Alert::Info),
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::PrintMergeAsk(session, request) => match o {
                Outcome::Ok => {
                    crate::merge_dialogs::answer(session, request, d.field_text("answer"));
                    match crate::merge_dialogs::advance(session) {
                        crate::merge_dialogs::Progress::Ask(next) => {
                            let origin = d.origin;
                            *request = next;
                            *d = crate::merge_dialogs::ask(request);
                            d.origin = origin;
                        }
                        crate::merge_dialogs::Progress::Done(outcome) => {
                            self.dialogs.pop();
                            self.finish_merge(*outcome);
                        }
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::MergeHelper(fields) => match o {
                Outcome::Changed(id) => {
                    let fields = fields.clone();
                    crate::merge_dialogs::helper_changed(&fields, d, &mut self.view, id);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::WordCount(counts, checks) => match o {
                Outcome::Changed(_) => {
                    *checks = crate::tool_dialogs::count_checks(d);
                    crate::tool_dialogs::update_count_labels(d, counts, *checks);
                }
                Outcome::Cancel | Outcome::Ok => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Footnote => match o {
                Outcome::Changed(id) => crate::tool_dialogs::footnote_changed(d, id),
                Outcome::Ok => {
                    let mark = crate::tool_dialogs::footnote_mark(d);
                    self.dialogs.pop();
                    self.close_pane();
                    let id = self.view.insert_footnote(mark);
                    self.open_note(id);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Picture(st) => match o {
                Outcome::Changed(id) => crate::picture_dialog::changed(d, st, id),
                Outcome::Ok => {
                    if let Some(new) = crate::picture_dialog::read(d, st) {
                        let (at, old) = (st.at, st.picture.clone());
                        self.dialogs.pop();
                        if new != old {
                            self.view.apply_picture(at, new);
                        }
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Hyperlink(st) => match o {
                Outcome::Changed(_) => crate::link_dialog::changed(d, st),
                Outcome::Ok => {
                    // Insert stays dimmed until both fields are filled, but
                    // Return still reaches here.
                    if let Some((text, link, color)) = crate::link_dialog::read(d, st) {
                        let target = st.target.clone();
                        self.dialogs.pop();
                        self.view.apply_link(&target, &text, link, color);
                    }
                }
                Outcome::Button("remove") => {
                    let (start, end) = (st.target.start, st.target.end);
                    self.dialogs.pop();
                    self.view.remove_link_from(start, end);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::ChangeCase => match o {
                Outcome::Ok => {
                    let case = crate::tool_dialogs::chosen_case(d);
                    self.dialogs.pop();
                    self.view.change_case(case);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Renumber => match o {
                Outcome::Ok => {
                    if let Some(opts) = crate::tool_dialogs::read_renumber(d) {
                        self.dialogs.pop();
                        self.view.renumber(&opts);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Clipboard(_) => {
                if *o == Outcome::Cancel {
                    self.dialogs.pop();
                }
            }
            Which::ReviewChanges => {
                let note = match o {
                    Outcome::Cancel => {
                        self.dialogs.pop();
                        return true;
                    }
                    Outcome::Ok | Outcome::Button("next") => (!self.view.go_to_change(false))
                        .then_some("There are no more changes after this point."),
                    Outcome::Button("previous") => (!self.view.go_to_change(true))
                        .then_some("There are no more changes before this point."),
                    Outcome::Button("accept") => {
                        self.view.resolve_change(true);
                        None
                    }
                    Outcome::Button("reject") => {
                        self.view.resolve_change(false);
                        None
                    }
                    Outcome::Button("acceptall") => {
                        self.view.resolve_all(true);
                        None
                    }
                    Outcome::Button("rejectall") => {
                        self.view.resolve_all(false);
                        None
                    }
                    _ => None,
                };
                crate::review_dialog::update(d, &self.view, note);
            }
            Which::Frame => match o {
                Outcome::Changed(id) => crate::frame_dialog::changed(d, id),
                Outcome::Ok | Outcome::Button("position") => {
                    let base = self.view.current_pap().frame.unwrap_or_default();
                    let Some(f) = crate::frame_dialog::read(d, &base) else {
                        return true;
                    };
                    let position = *o == Outcome::Button("position");
                    self.dialogs.pop();
                    self.view.format_paras("Frame", |p| {
                        if !p.in_table {
                            p.frame = Some(quill_core::Frame {
                                drop_lines: p.frame.map_or(0, |x| x.drop_lines),
                                in_margin: p.frame.is_some_and(|x| x.in_margin),
                                ..f
                            });
                        }
                    });
                    // Position… shows the page, where the frame can be
                    // dragged.
                    if position {
                        self.focus_main();
                        self.view.page_view = true;
                        self.view.show_page(self.view.caret_page());
                        self.position_frames = true;
                        self.view.status =
                            Some("Drag the frame to position it on the page.".into());
                    }
                }
                Outcome::Button("unframe") => {
                    self.dialogs.pop();
                    self.view.format_paras("Unframe", |p| p.frame = None);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::Palette(st) => match o {
                Outcome::Clicked(id, x, y, _) => {
                    if let crate::palette::Click::Edit(slot) =
                        crate::palette::clicked(st, d, id, *x, *y)
                    {
                        let (start, target) = crate::palette::edit_of(st, Some(slot));
                        self.edit_color(start, target);
                    }
                }
                Outcome::Changed(id) => crate::palette::changed(st, d, id),
                Outcome::Button("edit") => {
                    let (start, target) = crate::palette::edit_of(st, None);
                    self.edit_color(start, target);
                }
                Outcome::Ok | Outcome::Cancel => {
                    let (chosen, start) = if *o == Outcome::Ok {
                        (Some(crate::palette::finish(st)), st.start)
                    } else {
                        (None, st.start)
                    };
                    self.dialogs.pop();
                    // Back in the dialog that asked: the color chosen, or
                    // the one it had.
                    if let Some((_, parent)) = self.dialogs.last_mut() {
                        format_dialogs::show_color(parent, chosen.or(start));
                    }
                }
                _ => {}
            },
            Which::EditColor(st) => match o {
                Outcome::Changed(id) => crate::palette::edit_changed(st, d, id),
                Outcome::Ok => {
                    let (target, color) = (st.target, crate::palette::edit_color(d));
                    self.dialogs.pop();
                    self.color_edited(target, color);
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Which::DropCap => match o {
                Outcome::Changed(id) => crate::frame_dialog::drop_cap_changed(d, id),
                Outcome::Ok => {
                    let Some(dc) = crate::frame_dialog::read_drop_cap(d) else {
                        return true;
                    };
                    self.dialogs.pop();
                    if let Err(m) = self.view.drop_cap(&dc) {
                        self.alert(m, format_dialogs::Alert::Info);
                    }
                }
                Outcome::Cancel => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            _ => return false,
        }
        true
    }

    /// Act on what happened in the open dialog.
    pub(super) fn dialog_outcome(&mut self, o: Outcome, clip: &mut Clipboard) {
        if self.tool_outcome(&o, clip) {
            return;
        }
        let Some((which, d)) = self.dialogs.last_mut() else {
            return;
        };
        if let Which::Preferences {
            panel,
            original,
            draft,
            ..
        } = which
            && matches!(o, Outcome::Ok | Outcome::Button("save"))
        {
            crate::prefs_dialog::changed_in(*panel, d, "save", draft);
            crate::prefs_dialog::save(original, draft);
            if o == Outcome::Ok {
                self.dialogs.pop();
            }
            return;
        }
        match o {
            Outcome::Ok | Outcome::Button("savepdf") if matches!(which, Which::Print) => {
                let pdf = o == Outcome::Button("savepdf");
                // The dialog is borrowed, so reach the main view by its fields.
                let main = match &mut self.pane {
                    Some(p) if p.focused => &mut p.other,
                    _ => &mut self.view,
                };
                let job = crate::page_dialogs::print_job(d, main, pdf);
                if let Some(job) = job {
                    self.pending_print = Some(job);
                    self.dialogs.pop();
                }
            }
            Outcome::Ok if matches!(which, Which::PasteSpecial(_)) => {
                if let Which::PasteSpecial(forms) = which
                    && let Some(form) = crate::paste_special::chosen(d, forms)
                {
                    self.dialogs.pop();
                    self.view.paste_as(form, clip);
                }
            }
            Outcome::Ok => {
                let next = match which {
                    Which::ReplaceFonts(st) => st.next_page(),
                    _ => None,
                };
                if format_dialogs::apply(which, d, &mut self.view) {
                    self.dialogs.pop();
                    if let Some((d, st)) = next {
                        self.dialogs.push((Which::ReplaceFonts(st), d));
                    }
                } else if matches!(which, Which::Document) {
                    // Say why OK did nothing, as the original does.
                    self.alert(
                        "Quill can't use these settings. Check that each measurement is a \
                         number, and that the margins leave at least half an inch for text.",
                        format_dialogs::Alert::Info,
                    );
                }
            }
            Outcome::Apply if matches!(which, Which::CharacterStyles) => {
                crate::character_style_dialog::button(d, &mut self.view, "apply");
            }
            Outcome::Apply => {
                if format_dialogs::apply(which, d, &mut self.view) {
                    // Later changes compare against what is now applied.
                    match which {
                        Which::Character(base) => *base = self.view.current_chp(),
                        Which::Paragraph(base) => *base = self.view.current_pap().clone(),
                        _ => {}
                    }
                }
            }
            Outcome::Cancel => {
                crate::palette::drop_edits();
                self.dialogs.pop();
            }
            Outcome::Button("color") if matches!(which, Which::Character(_)) => {
                self.toggle_color_popout("color");
            }
            Outcome::Button(id)
                if matches!(which, Which::Preferences { .. })
                    && crate::prefs_dialog::color_box(id).is_some() =>
            {
                self.toggle_color_popout(id);
            }
            Outcome::Button(id)
                if matches!(which, Which::Preferences { .. })
                    && crate::prefs_dialog::clear_color(d, id) =>
            {
                d.popout = None;
                self.color_pop = None;
            }
            Outcome::Swatch { id, double }
                if matches!(which, Which::Character(_) | Which::Preferences { .. }) =>
            {
                self.color_popout_box(id, double);
            }
            Outcome::Button("edit")
                if matches!(which, Which::Character(_) | Which::Preferences { .. }) =>
            {
                self.color_popout_edit();
            }
            Outcome::Button("delete_color") if matches!(which, Which::Character(_)) => {
                self.color_popout_delete();
            }
            Outcome::Changed("color")
                if matches!(which, Which::Character(_)) && format_dialogs::wants_palette(d) =>
            {
                let start = match which {
                    Which::Character(base) => Some(base.color),
                    _ => None,
                };
                #[cfg(all(target_os = "macos", not(test)))]
                if self.on_screen {
                    if let Some(start) = start {
                        let color = crate::mac_color_panel::pick(start).unwrap_or(start);
                        format_dialogs::show_color(d, Some(color));
                    }
                    return;
                }
                let (palette, st) = crate::palette::palette(start, &self.view.doc);
                self.dialogs.push((Which::Palette(st), palette));
            }
            Outcome::Changed(id) => format_dialogs::changed(which, d, &self.view, id),
            Outcome::Clicked(..) | Outcome::Swatch { .. } => {}
            Outcome::Button("usedefault") => {
                if format_dialogs::use_as_default(which, d, &mut self.view) {
                    self.dialogs.pop();
                }
            }
            Outcome::Button(id) => match which {
                Which::Preferences { .. } if id == "reset" => crate::language::reset_ignored(),
                Which::Preferences { .. } if id == "customize" => {
                    let (d, st) = crate::customize::dialog();
                    self.dialogs.push((Which::Customize(st), d));
                }
                Which::Preferences {
                    panel,
                    families,
                    draft,
                    ..
                } if crate::prefs_dialog::is_folder_button(id) => {
                    crate::prefs_dialog::changed_in(*panel, d, id, draft);
                    crate::prefs_dialog::folder_button(id, draft);
                    let origin = d.origin;
                    *d = crate::prefs_dialog::preferences_with(*panel, families, draft);
                    d.origin = origin;
                }
                Which::Preferences {
                    panel,
                    families,
                    draft,
                    ..
                } if matches!(id, "dictionary_new" | "dictionary_choose") => {
                    crate::prefs_dialog::changed_in(*panel, d, id, draft);
                    match crate::local_dictionary::choose_file(id == "dictionary_new") {
                        Ok(Some(path)) => {
                            draft.custom_dictionary = Some(path);
                            draft.words_to_system = false;
                        }
                        Ok(None) => {}
                        Err(error) => crate::app::alert(
                            "Quill could not select the custom dictionary.",
                            &error,
                        ),
                    }
                    let origin = d.origin;
                    *d = crate::prefs_dialog::preferences_with(*panel, families, draft);
                    d.origin = origin;
                }
                Which::CharacterStyles if matches!(id, "new" | "edit") => {
                    let style = crate::character_style_dialog::selected_style(d);
                    let edit = (id == "edit" && style != 0).then_some(style);
                    if id == "edit" && edit.is_none() {
                        return;
                    }
                    let editor = crate::character_style_dialog::editor(
                        &self.view,
                        &self.font_families,
                        edit,
                    );
                    self.dialogs.push((Which::CharacterStyleEdit(edit), editor));
                }
                Which::CharacterStyles if id == "import" => {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("Import Character Styles")
                        .add_filter("Documents", quill_formats::READABLE)
                        .pick_file()
                    else {
                        return;
                    };
                    let status = match quill_formats::load(&path) {
                        Ok(source) => {
                            let (added, skipped) =
                                crate::character_style_dialog::import(d, &mut self.view, &source);
                            match (added, skipped) {
                                (0, 0) => "That document has no character styles.".to_string(),
                                (added, 0) => format!("Imported {added} character styles."),
                                (added, skipped) => format!(
                                    "Imported {added}; left out {skipped} whose names this document already uses."
                                ),
                            }
                        }
                        Err(why) => format!("Quill could not open that document: {why}"),
                    };
                    if let Some((Which::CharacterStyles, d)) = self.dialogs.last_mut() {
                        d.set_label("status", &status);
                    }
                }
                Which::CharacterStyles => {
                    crate::character_style_dialog::button(d, &mut self.view, id)
                }
                Which::Style(st) if id == "character" => {
                    let pending = crate::style_dialog::character_view(st);
                    let (mut editor, _) =
                        format_dialogs::character_for_find(&pending, &self.font_families);
                    editor.title = format!("Character: {}", d.field_text("name"));
                    editor.items.retain(|item| item.id != "apply");
                    let base = st.chp;
                    self.dialogs.push((Which::StyleCharacter(base), editor));
                }
                Which::Style(st) if id == "paragraph" => {
                    let base = st.pap.clone();
                    let mut editor = format_dialogs::paragraph_properties(&base);
                    editor.title = format!("Paragraph Style: {}", d.field_text("name"));
                    editor
                        .items
                        .retain(|item| !matches!(item.id, "apply" | "tabs" | "border" | "frame"));
                    self.dialogs.push((Which::StyleParagraph(base), editor));
                }
                Which::Style(st) if id == "define" => {
                    crate::style_dialog::define(st, d, &mut self.view);
                }
                Which::Style(st) if id == "delete" => {
                    crate::style_dialog::delete(st, d, &mut self.view);
                }
                Which::Tabs { .. } => format_dialogs::tabs_button(which, d, id),
                Which::Section if id == "linenumbers" => {
                    let d = crate::contents_dialogs::line_numbers(&self.view);
                    self.dialogs.push((Which::LineNumbers, d));
                }
                Which::Paragraph(_) if id == "tabs" => {
                    let (d, which) = format_dialogs::tabs(&self.view, None);
                    self.dialogs.push((which, d));
                }
                Which::InsertTable(_) if id == "format" => {
                    // Format…: insert the table, then go on to Table Cells.
                    if format_dialogs::apply(which, d, &mut self.view) {
                        self.dialogs.pop();
                        if let Some(d) = crate::table_dialogs::table_cells(&self.view) {
                            self.dialogs.push((Which::TableCells, d));
                        }
                    }
                }
                Which::TableCells if id == "border" => {
                    let (mut d, st) = crate::border_dialog::border(&self.view);
                    d.set_selected("applyto", 1);
                    let st = crate::border_dialog::for_cells(st, &self.view, &mut d);
                    self.dialogs.push((Which::Border(st), d));
                }
                Which::TableLayout => {
                    crate::table_dialogs::table_layout_button(d, &mut self.view, id);
                    self.dialogs.pop();
                }
                Which::PageSetup if id == "document" => {
                    let d = crate::page_dialogs::document(&self.view);
                    self.dialogs.push((Which::Document, d));
                }
                Which::Paragraph(_) if id == "frame" => {
                    let d = crate::frame_dialog::frame(&self.view);
                    self.dialogs.push((Which::Frame, d));
                }
                Which::Paragraph(_) if id == "border" => {
                    let (d, st) = crate::border_dialog::border(&self.view);
                    self.dialogs.push((Which::Border(st), d));
                }
                _ => {}
            },
            Outcome::None => {}
        }
    }

    /// The Tabs dialog's working tabs and chosen tab, when it is in front,
    /// so the ruler can show them.
    pub(super) fn dialog_tabs(
        &self,
    ) -> Option<(&[quill_core::TabStop], Option<quill_core::Twips>)> {
        match self.dialogs.last() {
            Some((Which::Tabs { tabs, selected, .. }, _)) => Some((tabs, *selected)),
            _ => None,
        }
    }

    /// A key while a dialog is open.
    pub fn dialog_key(&mut self, key: DKey, text: Option<&str>, clip: &mut Clipboard) {
        if let Some((_, d)) = self.dialogs.last_mut() {
            let o = d.key(key, text);
            self.dialog_outcome(o, clip);
        }
    }
}
