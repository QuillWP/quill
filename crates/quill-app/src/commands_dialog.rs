//! Tools > Commands (⌘⌥⇧C): every command, what it does, and where it is:
//! which menu holds it and which keys run it, both changeable, kept in
//! the settings file, which can be saved, opened, listed and reset.
//! Laid out from the original.

use crate::app_menus::MENU_TITLES;
use crate::canvas::{Canvas, Label, Rect, color, round_rect};
use crate::commands::{ALL, Cmd, code_name, keys_for};
use crate::dialog::{Dialog, wrap};
use crate::listbox::{Click, ListBox, Row};

const LIST: Rect = Rect {
    x: 9.0,
    y: 8.0,
    w: 226.0,
    h: 146.0,
};
const KEYS: Rect = Rect {
    x: 245.0,
    y: 180.0,
    w: 140.0,
    h: 61.0,
};
const DESCRIPTION: Rect = Rect {
    x: 240.0,
    y: 82.0,
    w: 234.0,
    h: 72.0,
};

#[derive(Clone, Debug)]
pub struct CommandsState {
    cmds: Vec<Cmd>,
    list: ListBox,
    keys: ListBox,
    /// Add… was pressed: the next key pressed is the new key.
    pub capturing: bool,
}

/// What the app should do after a button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    DoIt(Cmd),
    OpenSettings,
    SaveSettings,
    List,
    Reset,
}

/// What a command does, in a sentence.
pub fn describe(cmd: Cmd) -> String {
    use Cmd::*;
    let fixed = match cmd {
        New => "Opens a new, untitled document.",
        Open => "Opens an existing document.",
        Close => "Closes the active window.",
        Save => "Saves the active document.",
        SaveAs => "Saves the active document under a new name or in another format.",
        Print => "Prints the active document.",
        Quit => "Quits Quill.",
        Undo => "Reverses the last action.",
        Cut => "Removes the selection and puts it on the Clipboard.",
        Copy => "Copies the selection to the Clipboard.",
        Paste => "Inserts the Clipboard contents at the insertion point.",
        PasteText => "Pastes plain text using the insertion point's formatting.",
        PasteSpecial => "Pastes the Clipboard as formatted text, plain text or a picture.",
        Bold => "Makes the selection bold (toggle).",
        Italic => "Makes the selection italic (toggle).",
        Underline => "Underlines the selection (toggle).",
        PageBreak => "Inserts a page break at the insertion point.",
        SectionBreak => "Inserts a section break at the insertion point.",
        Spelling => "Checks the spelling of the document or selection.",
        TrackChanges => {
            "Marks insertions, deletions and format changes as tracked changes (toggle)."
        }
        ReviewChanges => "Steps through the tracked changes to accept or reject each one.",
        ReplyToAnnotation => "Adds a reply to the annotation at the insertion point.",
        ResolveAnnotation => {
            "Marks the annotation at the insertion point resolved, or open again (toggle)."
        }
        DeleteAnnotation => "Deletes the annotation at the insertion point and its replies.",
        InsertRowAbove => "Inserts a row above the selected rows, across the whole table.",
        InsertRowBelow => "Inserts a row below the selected rows, across the whole table.",
        InsertColumnLeft => "Inserts a column left of the selected cells, in every row.",
        InsertColumnRight => "Inserts a column right of the selected cells, in every row.",
        ConvertToTable => {
            "Turns the selected tab or comma separated lines into a table, one line to a row."
        }
        LookUp => {
            "Shows the system's definitions and thesaurus for the word at the insertion point."
        }
        ShowChanges => "Shows tracked changes marked up, or the text with them accepted (toggle).",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    let name = cmd.name();
    match name.strip_suffix('…') {
        Some(d) => format!("Opens the {d} dialog box."),
        None => format!("Carries out {name}."),
    }
}

/// The commands, by name.
fn sorted() -> Vec<Cmd> {
    let mut v: Vec<Cmd> = ALL.to_vec();
    v.retain(|cmd| *cmd != Cmd::RepaginateNow);
    v.sort_by_key(|c| c.name().to_lowercase());
    v
}

/// Tools > Commands, on `start` if given.
pub fn dialog(start: Option<Cmd>) -> (Dialog, CommandsState) {
    let mut d = Dialog::new("Commands", 484.0, 334.0);
    d.close_box = true;
    d.custom("list", LIST.x, LIST.y, LIST.w, LIST.h);
    d.text("Command:", 242.0, 6.0);
    d.text_as("name", "", 242.0, 26.0);
    d.button("doit", "Do It", 419.0, 38.0, 54.0, false);
    d.text("Description:", 242.0, 64.0);
    d.custom(
        "description",
        DESCRIPTION.x,
        DESCRIPTION.y,
        DESCRIPTION.w,
        DESCRIPTION.h,
    );
    d.group("Menu", 9.0, 166.0, 226.0, 80.0);
    d.popup("menu", &MENU_TITLES, 0, 15.0, 180.0, 149.0);
    d.button("madd", "Add", 171.0, 179.0, 60.0, false);
    d.button("mremove", "Remove", 171.0, 203.0, 60.0, false);
    d.text("Add", 15.0, 206.0);
    d.popup("where", &["Auto"], 0, 15.0, 222.0, 149.0);
    d.group("Keys", 240.0, 166.0, 234.0, 80.0);
    d.custom("keys", KEYS.x, KEYS.y, KEYS.w, KEYS.h);
    d.button("kadd", "Add…", 395.0, 179.0, 72.0, false);
    d.button("kremove", "Remove", 395.0, 203.0, 72.0, false);
    d.group("Settings File", 9.0, 257.0, 465.0, 36.0);
    d.text("Quill Settings", 15.0, 270.0);
    d.button("open", "Open…", 159.0, 268.0, 72.0, false);
    d.button("saveas", "Save As…", 235.0, 268.0, 75.0, false);
    d.button("listcmds", "List…", 314.0, 268.0, 76.0, false);
    d.button("reset", "Reset…", 394.0, 268.0, 76.0, false);
    d.button("cancel", "Close", 394.0, 305.0, 76.0, true);
    let cmds = sorted();
    let selected = start
        .and_then(|c| cmds.iter().position(|x| *x == c))
        .unwrap_or(0);
    let mut st = CommandsState {
        list: ListBox {
            selected,
            scroll: 0,
            len: cmds.len(),
        },
        cmds,
        keys: ListBox::default(),
        capturing: false,
    };
    if selected != usize::MAX {
        st.list
            .select(selected, Rect::new(0.0, 0.0, LIST.w, LIST.h));
    }
    refresh(&mut st, &mut d, None);
    (d, st)
}

fn chosen(st: &CommandsState) -> Option<Cmd> {
    st.cmds.get(st.list.selected).copied()
}

/// Select a row reported by the native macOS command or shortcut table.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn select_native(st: &mut CommandsState, d: &mut Dialog, id: &str, row: usize) {
    if id == "list" && row < st.cmds.len() {
        st.list.selected = row;
        st.capturing = false;
        let menu = st.cmds.get(row).and_then(|&c| {
            MENU_TITLES
                .iter()
                .find(|title| in_menu(c, title))
                .map(|title| title.to_string())
        });
        refresh(st, d, menu);
    } else if id == "keys" && row < st.keys.len {
        st.keys.selected = row;
    }
}

/// Rows and selection used by the native macOS tables. The platform view
/// remains a projection of the same dialog state as the canvas fallback.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_rows(d: &Dialog, id: &str) -> Vec<String> {
    let name = d.item_label("name");
    let chosen = sorted().into_iter().find(|cmd| cmd.name() == name);
    match id {
        "list" => sorted()
            .into_iter()
            .map(|cmd| cmd.name().to_string())
            .collect(),
        "keys" => chosen
            .map(keys_for)
            .unwrap_or_default()
            .iter()
            .map(|key| crate::commands::display_key(key))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_selection(d: &Dialog, id: &str) -> Option<usize> {
    match id {
        "list" => {
            let name = d.item_label("name");
            sorted().iter().position(|cmd| cmd.name() == name)
        }
        "keys" => None,
        _ => None,
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn native_description(d: &Dialog) -> String {
    let name = d.item_label("name");
    sorted()
        .into_iter()
        .find(|cmd| cmd.name() == name)
        .map(describe)
        .unwrap_or_default()
}

/// Bring the labels and buttons in line with the chosen command. `menu`
/// is the menu holding it, when known.
pub fn refresh(st: &mut CommandsState, d: &mut Dialog, menu: Option<String>) {
    let c = chosen(st);
    d.set_label("name", c.map_or("", |c| c.name()));
    d.enable("doit", c.is_some());
    let keys = c.map(keys_for).unwrap_or_default();
    st.keys = ListBox {
        selected: if keys.is_empty() { usize::MAX } else { 0 },
        scroll: 0,
        len: keys.len(),
    };
    d.enable("kadd", c.is_some());
    d.enable("kremove", !keys.is_empty());
    if let Some(m) = menu
        && let Some(k) = MENU_TITLES.iter().position(|t| *t == m)
    {
        d.set_selected("menu", k);
    }
    let title = MENU_TITLES[d.selected("menu").min(MENU_TITLES.len() - 1)];
    let in_menu = c.is_some_and(|c| in_menu(c, title));
    d.enable("madd", c.is_some() && !in_menu);
    d.enable("mremove", in_menu);
    if st.capturing {
        d.set_label("name", "Press the new key…");
    }
}

/// Whether `cmd` is in menu `title` now, counting Tools > Commands'
/// changes on top of the original's menus (as recorded by `menu_of`).
fn in_menu(cmd: Cmd, title: &str) -> bool {
    let name = code_name(cmd);
    crate::settings::with(|s| {
        let added = s.menu_add.iter().any(|(t, n)| t == title && *n == name);
        let removed = s.menu_remove.iter().any(|(t, n)| t == title && *n == name);
        added || (!removed && crate::app_menus::default_menu(cmd).as_deref() == Some(title))
    })
}

pub fn changed(st: &mut CommandsState, d: &mut Dialog, id: &str) {
    if id == "menu" {
        refresh(st, d, None);
    }
}

/// A click in the command list (choosing one shows its menu) or the keys.
pub fn clicked(st: &mut CommandsState, d: &mut Dialog, id: &str, x: f32, y: f32) {
    if id == "list" {
        if let Click::Chose(k) = st.list.click(x, y, Rect::new(0.0, 0.0, LIST.w, LIST.h)) {
            st.capturing = false;
            let menu = st.cmds.get(k).and_then(|&c| {
                MENU_TITLES
                    .iter()
                    .find(|t| in_menu(c, t))
                    .map(|t| t.to_string())
            });
            refresh(st, d, menu);
        }
    } else if id == "keys" {
        st.keys.click(x, y, Rect::new(0.0, 0.0, KEYS.w, KEYS.h));
    }
}

/// Scroll the command or shortcut list under the pointer.
pub fn scroll(st: &mut CommandsState, d: &Dialog, x: f32, y: f32, rows: i32) -> bool {
    if d.item_rect("list").is_some_and(|r| r.contains(x, y)) {
        st.list.scroll_by(rows, Rect::new(0.0, 0.0, LIST.w, LIST.h));
        true
    } else if d.item_rect("keys").is_some_and(|r| r.contains(x, y)) {
        st.keys.scroll_by(rows, Rect::new(0.0, 0.0, KEYS.w, KEYS.h));
        true
    } else {
        false
    }
}

pub fn button(st: &mut CommandsState, d: &mut Dialog, id: &str) -> Action {
    let Some(cmd) = chosen(st) else {
        return match id {
            "open" => Action::OpenSettings,
            "saveas" => Action::SaveSettings,
            "listcmds" => Action::List,
            "reset" => Action::Reset,
            _ => Action::None,
        };
    };
    let name = code_name(cmd);
    let title = MENU_TITLES[d.selected("menu").min(MENU_TITLES.len() - 1)].to_string();
    match id {
        "doit" => return Action::DoIt(cmd),
        "madd" => crate::settings::update(|s| {
            s.menu_remove.retain(|(t, n)| !(*t == title && *n == name));
            if crate::app_menus::default_menu(cmd).as_deref() != Some(title.as_str()) {
                s.menu_add.push((title.clone(), name.clone()));
            }
        }),
        "mremove" => crate::settings::update(|s| {
            let before = s.menu_add.len();
            s.menu_add.retain(|(t, n)| !(*t == title && *n == name));
            if s.menu_add.len() == before {
                s.menu_remove.push((title.clone(), name.clone()));
            }
        }),
        "kadd" => st.capturing = true,
        "kremove" => {
            if let Some(key) = keys_for(cmd).get(st.keys.selected).cloned() {
                crate::settings::update(|s| {
                    let before = s.keys_add.len();
                    s.keys_add.retain(|(k, n)| !(*k == key && *n == name));
                    if s.keys_add.len() == before {
                        s.keys_remove.push((key.clone(), name.clone()));
                    }
                });
            }
        }
        "open" => return Action::OpenSettings,
        "saveas" => return Action::SaveSettings,
        "listcmds" => return Action::List,
        "reset" => return Action::Reset,
        _ => {}
    }
    refresh(st, d, None);
    Action::None
}

/// The key pressed after Add…: it now runs the chosen command (and
/// nothing else it ran before).
pub fn captured(st: &mut CommandsState, d: &mut Dialog, spec: String) {
    st.capturing = false;
    if let Some(cmd) = chosen(st) {
        let name = code_name(cmd);
        crate::settings::update(|s| {
            s.keys_add.retain(|(k, _)| *k != spec);
            s.keys_remove.retain(|(k, n)| !(*k == spec && *n == name));
            s.keys_add.push((spec, name));
        });
    }
    refresh(st, d, None);
}

/// Escape leaves shortcut capture without changing the selected command.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn cancel_capture(st: &mut CommandsState, d: &mut Dialog) {
    st.capturing = false;
    refresh(st, d, None);
}

/// Reset…: the original's own menus, keys and toolbar again.
pub fn reset() {
    crate::settings::update(|s| {
        s.menu_add.clear();
        s.menu_remove.clear();
        s.keys_add.clear();
        s.keys_remove.clear();
        s.toolbar_buttons.clear();
    });
}

/// List…: a document of every command with its keys and menu, as the original's
/// List… made.
pub fn list_document() -> quill_core::Document {
    let mut text = String::from("Command\tKeys\tMenu\n");
    for c in sorted() {
        let menu = MENU_TITLES
            .iter()
            .find(|t| in_menu(c, t))
            .copied()
            .unwrap_or("");
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            c.name(),
            keys_for(c)
                .iter()
                .map(|key| crate::commands::display_key(key))
                .collect::<Vec<_>>()
                .join(", "),
            menu
        ));
    }
    let mut doc = quill_core::text::from_plain_text(&text);
    // Tabs for the columns.
    let last = doc.main.paras.len() - 1;
    doc.format_paras(quill_core::StoryId::Main, 0, last, |p| {
        p.tabs = vec![
            quill_core::props::TabStop {
                pos: 2 * 1440 + 720,
                ..Default::default()
            },
            quill_core::props::TabStop {
                pos: 4 * 1440,
                ..Default::default()
            },
        ];
    });
    doc.forget_history();
    doc
}

pub fn draw(st: &CommandsState, id: &str, r: Rect, c: &mut Canvas, label: &mut Label) {
    match id {
        "list" => {
            let rows: Vec<Row> = st
                .cmds
                .iter()
                .map(|c| Row {
                    text: c.name(),
                    mark: "",
                    bold: false,
                    underline: false,
                })
                .collect();
            st.list.draw(c, label, r, &rows);
        }
        "keys" => {
            let keys: Vec<String> = chosen(st)
                .map(keys_for)
                .unwrap_or_default()
                .iter()
                .map(|key| crate::commands::display_key(key))
                .collect();
            let rows: Vec<Row> = keys
                .iter()
                .map(|k| Row {
                    text: k,
                    mark: "",
                    bold: false,
                    underline: false,
                })
                .collect();
            st.keys.draw(c, label, r, &rows);
        }
        "description" => {
            let hair = c.hair();
            round_rect(c, r, 5.0, 0xA8A8A8);
            round_rect(c, r.inset(hair), 4.5, color::WHITE);
            if let Some(cmd) = chosen(st) {
                for (k, line) in wrap(&describe(cmd), 34).iter().enumerate() {
                    label.draw(
                        c,
                        quill_layout::fonts::UI_FONT,
                        11.0,
                        false,
                        r.x + 5.0,
                        r.y + 13.0 + 12.0 * k as f32,
                        line,
                        color::BLACK,
                    );
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_read_well() {
        assert_eq!(describe(Cmd::New), "Opens a new, untitled document.");
        assert_eq!(
            describe(Cmd::CharacterDialog),
            "Opens the Character dialog box."
        );
        assert!(crate::commands::by_name("PrintMerge").is_some());
    }

    #[test]
    fn commands_has_a_title_and_close_button_at_the_bottom() {
        let (dialog, state) = dialog(None);
        assert_eq!(dialog.title, "Commands");
        assert!(!dialog.plain);
        assert!(dialog.close_box);
        assert!(dialog.item_rect("cancel").unwrap().y > dialog.item_rect("reset").unwrap().y);
        assert_eq!(native_rows(&dialog, "list").len(), state.cmds.len());
        assert_eq!(native_selection(&dialog, "list"), Some(0));
        assert!(!native_description(&dialog).is_empty());
    }

    #[test]
    fn wheel_scrolls_the_command_list() {
        let (mut dialog, mut state) = dialog(None);
        dialog.place(800.0, 700.0, 0.0);
        let list = dialog.item_rect("list").unwrap();
        assert!(scroll(&mut state, &dialog, list.x + 10.0, list.y + 10.0, 3));
        assert_eq!(state.list.scroll, 3);
    }

    #[test]
    fn keys_and_menus_can_be_changed() {
        use crate::commands::{Mods, lookup};
        use winit::keyboard::Key;
        let (mut d, mut st) = dialog(Some(Cmd::WordCount));
        assert!(!keys_for(Cmd::WordCount).iter().any(|k| k == "⌘⇧⌥J"));
        st.capturing = true;
        captured(&mut st, &mut d, "⌘⇧⌥J".into());
        assert!(keys_for(Cmd::WordCount).iter().any(|k| k == "⌘⇧⌥J"));
        let mods = Mods {
            cmd: true,
            shift: true,
            opt: true,
            ctrl: false,
        };
        assert_eq!(
            lookup(&Key::Character("j".into()), Some('j'), mods),
            Some(Cmd::WordCount)
        );
        // Into the File menu and out again.
        d.set_selected("menu", 0);
        assert!(!in_menu(Cmd::WordCount, "File"));
        button(&mut st, &mut d, "madd");
        assert!(in_menu(Cmd::WordCount, "File"));
        button(&mut st, &mut d, "mremove");
        assert!(!in_menu(Cmd::WordCount, "File"));
        assert_eq!(
            crate::app_menus::default_menu(Cmd::WordCount).as_deref(),
            Some("Tools")
        );
        reset();
        assert!(!keys_for(Cmd::WordCount).iter().any(|k| k == "⌘⇧⌥J"));
    }

    #[test]
    fn shortcut_capture_can_be_canceled() {
        let (mut dialog, mut state) = dialog(Some(Cmd::WordCount));
        state.capturing = true;
        refresh(&mut state, &mut dialog, None);
        assert_eq!(dialog.item_label("name"), "Press the new key…");

        cancel_capture(&mut state, &mut dialog);

        assert!(!state.capturing);
        assert_eq!(dialog.item_label("name"), Cmd::WordCount.name());
        assert!(!native_description(&dialog).is_empty());
    }
}
