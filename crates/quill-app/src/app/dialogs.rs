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
        self.menu = None;
        self.font_names = env.font_names.to_vec();
        self.font_families = env.font_families.to_vec();
        // One modeless window at a time: a new one takes the old one's place.
        let modeless = matches!(
            id,
            DialogId::Find
                | DialogId::Replace
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
                let (d, st) = crate::find_dialog::dialog(&self.view, id == DialogId::Replace);
                (d, Which::Find(st))
            }
            DialogId::GoTo => (crate::find_dialog::go_to(&self.view), Which::GoTo),
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
                let (mut d, st) = crate::link_dialog::dialog(&self.view);
                crate::link_dialog::changed(&mut d, &st);
                (d, Which::Hyperlink(st))
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
                crate::prefs_dialog::preferences(0, env.font_names),
                Which::Preferences {
                    panel: 0,
                    fonts: env.font_names.to_vec(),
                },
            ),
            DialogId::DefaultFont => (
                crate::prefs_dialog::preferences(
                    crate::prefs_dialog::DEFAULT_FONT_PANEL,
                    env.font_names,
                ),
                Which::Preferences {
                    panel: crate::prefs_dialog::DEFAULT_FONT_PANEL,
                    fonts: env.font_names.to_vec(),
                },
            ),
            DialogId::ToolbarPreferences => (
                crate::prefs_dialog::preferences(
                    crate::prefs_dialog::TOOLBAR_PANEL,
                    env.font_names,
                ),
                Which::Preferences {
                    panel: crate::prefs_dialog::TOOLBAR_PANEL,
                    fonts: env.font_names.to_vec(),
                },
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
                    "Quill reached the beginning of the document. Continue searching at the end?"
                } else {
                    "Quill reached the end of the document. Continue searching at the beginning?"
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
            Which::Find(st) => match o {
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
                Outcome::Button("replace") => {
                    fd::update(st, d, &self.view);
                    let s = st.search.clone();
                    let r = fd::replace(st, &mut self.view, clip);
                    self.after_search(r, &s);
                }
                Outcome::Button("replaceall") => {
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
                    format_dialogs::character_changed(d, id, None)
                }
                _ => {}
            },
            Which::Alert(alert) => match o {
                Outcome::Ok => {
                    let alert = alert.clone();
                    self.dialogs.pop();
                    if let format_dialogs::Alert::ContinueSearch(s) = alert {
                        let r = if fd::find_wrapped(&mut self.view, &s) {
                            fd::Hit::Match
                        } else {
                            fd::Hit::NotFound
                        };
                        self.after_search(r, &s);
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
                            &self.font_names,
                        );
                        self.dialogs.push((
                            Which::Preferences {
                                panel: crate::prefs_dialog::SPELLING_PANEL,
                                fonts: self.font_names.clone(),
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
                    let Some(f) = crate::frame_dialog::read(d) else {
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
                if format_dialogs::apply(which, d, &mut self.view) {
                    self.dialogs.pop();
                } else if matches!(which, Which::Document) {
                    // Say why OK did nothing, as the original does.
                    self.alert(
                        "Quill can't use these settings. Check that each measurement is a \
                         number, and that the margins leave at least half an inch for text.",
                        format_dialogs::Alert::Info,
                    );
                }
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
                self.dialogs.pop();
            }
            Outcome::Changed(id) => format_dialogs::changed(which, d, &self.view, id),
            Outcome::Clicked(..) => {}
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
                Which::Preferences { panel, fonts }
                    if matches!(id, "dictionary_new" | "dictionary_choose") =>
                {
                    if let Err(error) = crate::local_dictionary::choose_file(id == "dictionary_new")
                    {
                        crate::app::alert("Quill could not select the custom dictionary.", &error);
                    }
                    let origin = d.origin;
                    *d = crate::prefs_dialog::preferences(*panel, fonts);
                    d.origin = origin;
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
                    let st = crate::border_dialog::for_cells(st, &self.view);
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
