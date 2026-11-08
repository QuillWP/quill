//! Edit > Glossary (⌘K): named pieces of text to insert by name, the original's
//! standard entries (dates, times, the author) and the user's own. User
//! entries are kept in a file beside the settings, each as RTF so its
//! formatting survives.

use crate::dialog::Dialog;
use crate::docview::DocView;
use crate::glossary_file::{Entry, parse, serialize};
use quill_core::document::Fragment;
use quill_core::para::{Paragraph, chars};
use quill_core::props::ParaProps;
use quill_core::{Position, Selection};
use std::cell::RefCell;

thread_local! {
    static ENTRIES: RefCell<Option<Vec<Entry>>> = const { RefCell::new(None) };
}

fn file() -> Option<std::path::PathBuf> {
    crate::settings::sibling("Glossary", "Quill Glossary.txt", "glossary")
}

/// The glossary is read whole at startup; a larger or non-regular file is
/// ignored instead of read.
const MAX_GLOSSARY_BYTES: u64 = 8 << 20;

fn with_entries<R>(f: impl FnOnce(&mut Vec<Entry>) -> R) -> R {
    ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        let list = e.get_or_insert_with(|| {
            file()
                .and_then(|p| quill_formats::read_file_with_limit(&p, MAX_GLOSSARY_BYTES).ok())
                .map(|b| parse(&b))
                .unwrap_or_default()
        });
        f(list)
    })
}

/// The user's entries, in no particular order.
pub fn entries() -> Vec<Entry> {
    with_entries(|e| e.clone())
}

fn save(list: &[Entry]) -> Result<(), String> {
    let Some(path) = file() else {
        return Ok(());
    };
    crate::settings::write_user_file(&path, &serialize(list))
}

/// The original's standard entries, in its order.
pub const STANDARD: [&str; 38] = [
    "author",
    "date",
    "date abbreviated",
    "date long",
    "date short",
    "day abbreviated",
    "day long",
    "day of month",
    "file name only",
    "file name with path",
    "keywords",
    "month abbreviated",
    "month long",
    "month short",
    "page number",
    "print date",
    "print date abbreviated",
    "print date long",
    "print date short",
    "print day abbreviated",
    "print day long",
    "print day of month",
    "print merge",
    "print month abbreviated",
    "print month long",
    "print month short",
    "print time",
    "print time with seconds",
    "print year long",
    "print year short",
    "section",
    "subject",
    "time",
    "time with seconds",
    "title",
    "version",
    "year long",
    "year short",
];

/// What a standard entry inserts now. Print dates are the date now: Quill
/// prints straight away. The page number is a field.
fn standard_text(name: &str, view: &DocView) -> String {
    let now = crate::clock::now();
    let name = name.strip_prefix("print ").unwrap_or(name);
    match name {
        "author" => {
            let a = view.doc.summary.author.clone();
            if a.is_empty() {
                crate::settings::with(|s| s.name.clone())
            } else {
                a
            }
        }
        "date" => now.date_normal(),
        "date abbreviated" => now.date_abbreviated(),
        "date long" => now.date_long(),
        "date short" => now.date(),
        "day abbreviated" => now.day_name()[..3].to_string(),
        "day long" => now.day_name().to_string(),
        "day of month" => now.day.to_string(),
        "month abbreviated" => now.month_name()[..3].to_string(),
        "month long" => now.month_name().to_string(),
        "month short" => now.month.to_string(),
        "year long" => now.year.to_string(),
        "year short" => format!("{:02}", now.year.rem_euclid(100)),
        "time" => now.time(),
        "time with seconds" => now.time_with_seconds(),
        "file name only" => view.title.clone(),
        "file name with path" => view
            .path
            .as_ref()
            .map_or(view.title.clone(), |p| p.display().to_string()),
        "keywords" => view.doc.summary.keywords.clone(),
        "subject" => view.doc.summary.subject.clone(),
        "title" => view.doc.summary.title.clone(),
        "version" => view.doc.summary.version.clone(),
        "section" => "1".to_string(),
        "merge" => "\u{AB}\u{BB}".to_string(),
        "page number" => chars::FIELD_PAGE.to_string(),
        _ => String::new(),
    }
}

/// The entry named `name` (any case) as a fragment for `view`'s document.
fn content(name: &str, view: &mut DocView) -> Option<Fragment> {
    let user = with_entries(|e| {
        e.iter()
            .find(|x| x.name.eq_ignore_ascii_case(name))
            .cloned()
    });
    if let Some(e) = user {
        return Some(
            view.doc
                .import_fragment_styled(&e.frag, &e.fonts, &e.styles, &e.characters),
        );
    }
    let std = STANDARD.iter().find(|s| s.eq_ignore_ascii_case(name))?;
    let text = standard_text(std, view);
    let at = view.sel.start();
    let chp = view.doc.main.paras[at.para].chp_at(at.offset);
    Some(Fragment {
        paras: vec![Paragraph::with_text(&text, chp, ParaProps::default())],
        ends_with_mark: false,
        objects: Vec::new(),
        notes: Vec::new(),
    })
}

/// Insert the entry named `name` at the selection.
pub fn insert(name: &str, view: &mut DocView) -> bool {
    match content(name, view) {
        Some(frag) => {
            view.insert_fragment("Glossary", &frag);
            true
        }
        None => false,
    }
}

/// Insert Glossary Entry (⌘⌫): the selection, or the word before the
/// insertion point, names an entry; replace it with the entry.
pub fn expand(view: &mut DocView) {
    let story = &view.doc.main;
    let (s, e) = if view.sel.is_empty() {
        let e = view.sel.focus;
        let text = &story.paras[e.para].text[..e.offset];
        let start = text
            .char_indices()
            .rev()
            .take_while(|(_, c)| !c.is_whitespace())
            .last()
            .map_or(e.offset, |(i, _)| i);
        (Position::new(e.para, start), e)
    } else {
        view.sel.range()
    };
    let name = story.text_of(s, e);
    if name.is_empty() {
        return;
    }
    if content(&name, view).is_some() {
        view.set_selection(Selection::new(s, e));
        insert(&name, view);
    }
}

// ----- the Glossary window -----

/// The list: "New", then the entries shown, sorted by name. The flag is
/// whether the entry is a standard one.
#[derive(Clone, Debug)]
pub struct GlossaryState {
    pub names: Vec<(String, bool)>,
    pub selected: usize,
}

fn names(standard: bool, user: bool) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    if standard {
        out.extend(STANDARD.iter().map(|s| (s.to_string(), true)));
    }
    if user {
        out.extend(entries().into_iter().map(|e| (e.name, false)));
    }
    out.sort_by_key(|n| n.0.to_lowercase());
    out
}

fn options(names: &[(String, bool)]) -> Vec<String> {
    std::iter::once("New".to_string())
        .chain(names.iter().map(|(name, standard)| {
            if *standard {
                format!("• {name}")
            } else {
                name.clone()
            }
        }))
        .collect()
}

pub fn dialog(view: &DocView) -> (Dialog, GlossaryState) {
    let mut d = Dialog::new("Glossary", 322.0, 182.0);
    d.modeless = true;
    d.close_box = true;
    let names = names(true, true);
    d.popup_owned("list", options(&names), 0, 6.0, 4.0, 242.0);
    d.button("insert", "Insert", 261.0, 12.0, 52.0, true);
    d.button("define", "Define", 261.0, 35.0, 52.0, false);
    d.button("delete", "Delete", 261.0, 58.0, 52.0, false);
    d.button("cancel", "Close", 261.0, 81.0, 52.0, false);
    d.text("Show:", 6.0, 122.0);
    d.check("standard", "Standard Entries", 55.0, 122.0, true);
    d.check("user", "User Entries", 187.0, 122.0, true);
    d.text("Name:", 6.0, 141.0);
    d.field("name", "", 55.0, 138.0, 260.0);
    d.text_as("preview", "", 55.0, 164.0);
    let mut st = GlossaryState { names, selected: 0 };
    refresh(&mut st, &mut d, Some(view), true);
    (d, st)
}

/// The chosen entry's name, unless "New" is chosen.
fn chosen(st: &GlossaryState) -> Option<&(String, bool)> {
    st.selected.checked_sub(1).and_then(|i| st.names.get(i))
}

/// Bring the buttons, name field and preview in line with the choice.
fn refresh(st: &mut GlossaryState, d: &mut Dialog, view: Option<&DocView>, set_name: bool) {
    d.set_options("list", options(&st.names), st.selected);
    let has_sel = view.is_some_and(|v| !v.sel.is_empty());
    match chosen(st).cloned() {
        Some((name, standard)) => {
            if set_name {
                d.set_field("name", &name);
            }
            d.enable("insert", true);
            d.enable("define", !standard && has_sel);
            d.enable("delete", !standard);
            let preview = match view {
                Some(v) if standard => standard_text(&name, v).replace(chars::FIELD_PAGE, "1"),
                _ => entries()
                    .into_iter()
                    .find(|e| e.name == name)
                    .map(|e| e.frag.plain_text().replace('\n', " "))
                    .unwrap_or_default(),
            };
            let preview: String = preview.chars().take(40).collect();
            d.set_label("preview", &preview);
        }
        None => {
            if set_name {
                d.set_field("name", "");
            }
            d.enable("insert", false);
            d.enable("define", has_sel);
            d.enable("delete", false);
            d.set_label("preview", "");
        }
    }
}

/// A checkbox, list choice or the name field changed.
pub fn changed(st: &mut GlossaryState, d: &mut Dialog, view: &DocView, id: &str) {
    if id == "list" {
        st.selected = d.selected("list").min(st.names.len());
        refresh(st, d, Some(view), true);
        return;
    }
    match id {
        "standard" | "user" => {
            st.names = names(d.is_on("standard"), d.is_on("user"));
            st.selected = 0;
            refresh(st, d, Some(view), true);
        }
        "name" => {
            // Typing a name chooses the entry it names.
            let typed = d.field_text("name");
            let k = st
                .names
                .iter()
                .position(|(n, _)| n.eq_ignore_ascii_case(typed.trim()))
                .map_or(0, |i| i + 1);
            st.selected = k;
            refresh(st, d, Some(view), false);
        }
        _ => {}
    }
}

/// Insert, Define or Delete. Returns true when the window should close.
pub fn button(st: &mut GlossaryState, d: &mut Dialog, view: &mut DocView, id: &str) -> bool {
    match id {
        "insert" => {
            if let Some((name, _)) = chosen(st).cloned() {
                insert(&name, view);
                return true;
            }
        }
        "define" => {
            if view.sel.is_empty() {
                return false;
            }
            let (s, e) = view.sel.range();
            let mut name = d.field_text("name").trim().to_string();
            if name.is_empty() {
                // The original suggests the start of the text as the name.
                name = view.doc.main.text_of(s, e).chars().take(16).collect();
                name = name.trim().to_string();
            }
            if name.is_empty() || STANDARD.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                return false;
            }
            let frag = view.doc.copy(quill_core::StoryId::Main, s, e);
            let entry = Entry {
                name: name.clone(),
                characters: view.doc.styles.characters.clone(),
                frag,
                fonts: view.doc.fonts.clone(),
                styles: view
                    .doc
                    .styles
                    .styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect(),
            };
            let saved = with_entries(|list| {
                let mut next = list.clone();
                next.retain(|e| !e.name.eq_ignore_ascii_case(&name));
                next.push(entry);
                save(&next)?;
                *list = next;
                Ok::<(), String>(())
            });
            if let Err(e) = saved {
                crate::app::alert("Quill could not save the glossary.", &e);
                return false;
            }
            st.names = names(d.is_on("standard"), d.is_on("user"));
            let k = st
                .names
                .iter()
                .position(|(n, _)| *n == name)
                .map_or(0, |i| i + 1);
            st.selected = k;
            refresh(st, d, Some(view), true);
        }
        "delete" => {
            if let Some((name, false)) = chosen(st).cloned() {
                let saved = with_entries(|list| {
                    let mut next = list.clone();
                    next.retain(|e| e.name != name);
                    save(&next)?;
                    *list = next;
                    Ok::<(), String>(())
                });
                if let Err(e) = saved {
                    crate::app::alert("Quill could not save the glossary.", &e);
                    return false;
                }
                st.names = names(d.is_on("standard"), d.is_on("user"));
                st.selected = 0;
                refresh(st, d, Some(view), true);
            }
        }
        _ => {}
    }
    false
}

/// Plain text for a new user entry, for tests.
#[cfg(test)]
fn entry(name: &str, text: &str) -> Entry {
    use quill_core::props::CharProps;
    Entry {
        name: name.into(),
        frag: Fragment {
            paras: vec![Paragraph::with_text(
                text,
                CharProps::default(),
                ParaProps::default(),
            )],
            ends_with_mark: false,
            objects: Vec::new(),
            notes: Vec::new(),
        },
        fonts: vec!["Geneva".into()],
        styles: vec!["Normal".into()],
        characters: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Document;

    #[test]
    fn glossary_serialization_keeps_character_definitions_and_maps_conflicting_names() {
        use quill_core::character_styles::CharacterFormat;
        use quill_core::{Position, Selection, StoryId};
        let mut doc = quill_core::text::from_plain_text("Styled");
        let id = doc
            .add_character_style(
                "Emphasis",
                CharacterFormat::new(
                    quill_core::CharProps {
                        italic: true,
                        ..Default::default()
                    },
                    1 << 3,
                ),
            )
            .unwrap();
        doc.apply_character_style(StoryId::Main, Position::new(0, 0), Position::new(0, 6), id);
        let entry = Entry {
            name: "Example".into(),
            frag: doc.copy(StoryId::Main, Position::new(0, 0), Position::new(0, 6)),
            fonts: doc.fonts.clone(),
            styles: doc.styles.styles.iter().map(|s| s.name.clone()).collect(),
            characters: doc.styles.characters.clone(),
        };
        let back = parse(&serialize(&[entry]));
        assert_eq!(back[0].characters[0].name, "Emphasis");
        let mut target = Document::new();
        target.add_character_style(
            "Emphasis",
            CharacterFormat::new(
                quill_core::CharProps {
                    bold: true,
                    ..Default::default()
                },
                1 << 2,
            ),
        );
        target.commit(Selection::default());
        let e = &back[0];
        let fragment = target.import_fragment_styled(&e.frag, &e.fonts, &e.styles, &e.characters);
        let c = fragment.paras[0].chp_of_char(0);
        assert!(c.italic);
        assert_eq!(
            target
                .styles
                .character(c.character_style)
                .unwrap()
                .format
                .mask,
            1 << 3
        );
    }

    #[test]
    fn entries_survive_the_file() {
        let mut bold = entry("sig", "Yours truly");
        bold.frag.paras[0].format_range(0, 5, &mut |c| c.bold = true);
        let list = vec![bold, entry("addr", "1 Infinite Loop")];
        let back = parse(&serialize(&list));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "sig");
        assert_eq!(back[0].frag.paras[0].text, "Yours truly");
        assert!(back[0].frag.paras[0].chp_of_char(0).bold);
        assert!(!back[0].frag.paras[0].chp_of_char(6).bold);
        assert_eq!(back[1].frag.plain_text(), "1 Infinite Loop");
        assert!(!back[1].frag.ends_with_mark);
    }

    #[test]
    fn impossible_entry_lengths_are_rejected_without_overflow() {
        let data = format!("Quill Glossary 1\nname\t0\t1\t{}\n", usize::MAX);
        assert!(parse(data.as_bytes()).is_empty());
    }

    #[test]
    fn standard_entries_fill_in() {
        let mut v = DocView::new(Document::new(), "Letter".into());
        v.doc.summary.title = "A Title".into();
        assert_eq!(standard_text("title", &v), "A Title");
        assert_eq!(standard_text("file name only", &v), "Letter");
        let now = crate::clock::now();
        assert_eq!(standard_text("date", &v), now.date_normal());
        assert_eq!(standard_text("print year long", &v), now.year.to_string());
        assert!(insert("date", &mut v));
        assert_eq!(v.doc.main.paras[0].text, now.date_normal());
    }

    #[test]
    fn command_delete_expands_a_name() {
        let mut v = DocView::new(Document::new(), "x".into());
        v.insert_text("Today is title");
        v.doc.summary.title = "Quill".into();
        expand(&mut v);
        assert_eq!(v.doc.main.paras[0].text, "Today is Quill");
    }

    #[test]
    fn glossary_uses_only_standard_controls() {
        let view = DocView::new(Document::new(), "Test".into());
        let (dialog, _) = dialog(&view);
        assert!(dialog.modeless);
        assert!(
            !dialog
                .items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
    }
}
