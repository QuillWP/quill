//! Tools > Preferences: a native category chooser and the chosen panel's
//! settings. As in the original there is no OK button:
//! every change takes effect at once, and the close box puts it away.
//! Layout measured from the original.

use crate::dialog::Dialog;
use crate::settings::{self, Unit};

/// Panels: name and icon. Grammar, Thesaurus and Hyphenation have no
/// settings in Quill yet, so their panels are left out.
const PANELS: [&str; 7] = [
    "General",
    "View",
    "Open And Save",
    "Default Font",
    "Spelling",
    "Toolbar",
    "Advanced",
];
/// The Spelling panel, which the Spelling window's Options… opens.
pub const DEFAULT_FONT_PANEL: usize = 3;
pub const SPELLING_PANEL: usize = 4;
pub const TOOLBAR_PANEL: usize = 5;
pub const ADVANCED_PANEL: usize = 6;
/// Item ids for the ten font folder slots.
const FOLDER_ON: [&str; 10] = [
    "fon0", "fon1", "fon2", "fon3", "fon4", "fon5", "fon6", "fon7", "fon8", "fon9",
];
const FOLDER_CHOOSE: [&str; 10] = [
    "fchoose0", "fchoose1", "fchoose2", "fchoose3", "fchoose4", "fchoose5", "fchoose6", "fchoose7",
    "fchoose8", "fchoose9",
];
const FOLDER_CLEAR: [&str; 10] = [
    "fclear0", "fclear1", "fclear2", "fclear3", "fclear4", "fclear5", "fclear6", "fclear7",
    "fclear8", "fclear9",
];
/// How often the recovery copy is saved, in seconds (0 is never).
const AUTOSAVE: [(&str, u32); 6] = [
    ("Never", 0),
    ("Every 30 seconds", 30),
    ("Every minute", 60),
    ("Every 2 minutes", 120),
    ("Every 5 minutes", 300),
    ("Every 10 minutes", 600),
];
const SIZES: [&str; 11] = [
    "9", "10", "12", "14", "18", "24", "36", "48", "60", "72", "96",
];

/// The Preferences window showing `panel`, with `fonts` for Default Font.
pub fn preferences(panel: usize, fonts: &[String]) -> Dialog {
    let s = settings::get();
    let height = if panel == ADVANCED_PANEL {
        412.0
    } else {
        332.0
    };
    let mut d = Dialog::new("Preferences", 442.0, height);
    d.close_box = true;
    d.modeless = true;
    d.text("Show:", 11.0, 10.0);
    d.popup(
        "panel",
        &PANELS,
        panel.min(PANELS.len() - 1),
        52.0,
        7.0,
        176.0,
    );
    match panel {
        0 => {
            d.text("Your Name:", 11.0, 45.0);
            d.field("name", &s.name, 102.0, 42.0, 327.0);
            d.text("Your Initials:", 11.0, 71.0);
            d.field("initials", &s.initials, 102.0, 69.0, 67.0);
            d.text("Measurement Unit:", 11.0, 102.0);
            let names: Vec<&str> = Unit::ALL.iter().map(|u| u.label()).collect();
            let at = Unit::ALL.iter().position(|u| *u == s.unit).unwrap_or(0);
            d.popup("unit", &names, at, 139.0, 100.0, 89.0);
            d.check(
                "smart",
                "\u{201C}Smart\u{201D} Quotes",
                12.0,
                126.0,
                s.smart_quotes,
            );
            d.check(
                "autolinks",
                "Make Typed Web and Mail Addresses Links",
                12.0,
                143.0,
                s.auto_links,
            );
            d.check(
                "fmtclip",
                "Include Formatted Text in Clipboard",
                12.0,
                160.0,
                s.formatted_clipboard,
            );
            d.text("Default Paper:", 11.0, 184.0);
            let mut papers: Vec<&str> = crate::page_dialogs::PAPERS.iter().map(|p| p.0).collect();
            let current = crate::page_dialogs::PAPERS.iter().position(|p| {
                (p.1 - s.document.page_width.min(s.document.page_height)).abs() < 20
                    && (p.2 - s.document.page_width.max(s.document.page_height)).abs() < 20
            });
            if current.is_none() {
                papers.push("Custom");
            }
            d.popup(
                "defaultpaper",
                &papers,
                current.unwrap_or(papers.len() - 1),
                139.0,
                181.0,
                160.0,
            );
            d.note(
                "paper_help",
                "For new documents. Page Setup changes the current document.",
                12.0,
                215.0,
                410.0,
            );
            d.focus("name");
        }
        1 => {
            d.group("Show", 11.0, 39.0, 364.0, 88.0);
            d.check("hidden", "Hidden Text", 16.0, 62.0, s.show_hidden);
            d.check(
                "gridlines",
                "Table Gridlines",
                16.0,
                80.0,
                s.table_gridlines,
            );
            d.check(
                "boundaries",
                "Text Boundaries in Page Layout View",
                16.0,
                98.0,
                s.text_boundaries,
            );
            d.group("Open Documents", 11.0, 134.0, 364.0, 104.0);
            d.text("In View:", 16.0, 157.0);
            let views: Vec<&str> = settings::DefaultView::ALL
                .iter()
                .map(|v| v.label())
                .collect();
            let at = settings::DefaultView::ALL
                .iter()
                .position(|v| *v == s.default_view)
                .unwrap_or(0);
            d.popup("defaultview", &views, at, 90.0, 154.0, 140.0);
            d.check(
                "formatting",
                "With Formatting Toolbar On",
                16.0,
                175.0,
                s.open_with_formatting_toolbar,
            );
            d.check(
                "paragraph",
                "With Paragraph Toolbar On",
                16.0,
                193.0,
                s.open_with_paragraph_toolbar,
            );
            d.check("ruler", "With Ruler On", 16.0, 211.0, s.open_with_ruler);
            d.group("Menus", 11.0, 246.0, 364.0, 44.0);
            d.check(
                "recent",
                "List Recently Opened Documents",
                16.0,
                269.0,
                s.recent_files,
            );
        }
        2 => {
            d.check("rtf", "Always Interpret RTF", 13.0, 45.0, s.interpret_rtf);
            d.check("backup", "Always Make Backup", 13.0, 65.0, s.make_backup);
            d.check(
                "foreign_quill",
                "Save foreign files in Quill format by default",
                13.0,
                98.0,
                s.foreign_as_quill,
            );
            d.note("foreign_help", "Highly recommended", 32.0, 122.0, 380.0);
            d.text("Reopen last session's documents:", 16.0, 157.0);
            let choices: Vec<&str> = settings::Reopen::ALL.iter().map(|r| r.label()).collect();
            let at = settings::Reopen::ALL
                .iter()
                .position(|r| *r == s.reopen)
                .unwrap_or(2);
            d.popup("reopen", &choices, at, 232.0, 154.0, 130.0);
        }
        SPELLING_PANEL => {
            d.text("Custom Dictionary:", 9.0, 41.0);
            d.button("dictionary_new", "New…", 9.0, 63.0, 80.0, false);
            d.button("dictionary_choose", "Choose…", 101.0, 63.0, 90.0, false);
            let path = crate::local_dictionary::path();
            let name = path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Quill Dictionary".into());
            d.note(
                "dictionary_path",
                &format!("Current: {name}"),
                9.0,
                94.0,
                410.0,
            );
            d.text("Add New Words To:", 9.0, 123.0);
            d.popup(
                "add_words",
                &["Custom Dictionary", "System Dictionary"],
                usize::from(s.words_to_system),
                155.0,
                120.0,
                176.0,
            );
            d.note(
                "dictionary_help",
                "Custom words stay in Quill. System words are shared with other apps.",
                9.0,
                153.0,
                410.0,
            );
            d.check(
                "live_spelling",
                "Check spelling as you type",
                14.0,
                178.0,
                s.spelling_as_you_type,
            );
            d.check("suggest", "Always Suggest", 14.0, 200.0, s.always_suggest);
            d.group("Ignore", 8.0, 225.0, 269.0, 66.0);
            d.check(
                "upper",
                "Words in UPPERCASE",
                14.0,
                248.0,
                s.ignore_uppercase,
            );
            d.check(
                "numbers",
                "Words with Numbers",
                14.0,
                269.0,
                s.ignore_numbers,
            );
            d.button(
                "reset",
                "Reset \u{201C}Ignore All\u{201D} List",
                8.0,
                302.0,
                175.0,
                false,
            );
        }
        TOOLBAR_PANEL => {
            d.check("viewtb", "View Toolbar", 14.0, 41.0, s.view_toolbar);
            d.check(
                "tabbuttons",
                "Tab Buttons on the Paragraph Toolbar",
                14.0,
                59.0,
                s.paragraph_toolbar_tabs,
            );
            d.group("Toolbar Position", 8.0, 90.0, 327.0, 86.0);
            let at = s.toolbar_position;
            d.radio("tbtop", "Top (Horizontal)", 20.0, 113.0, 1, at == 0);
            d.radio("tbleft", "Left (Vertical)", 20.0, 134.0, 1, at == 1);
            d.radio("tbright", "Right (Vertical)", 20.0, 155.0, 1, at == 2);
            d.button("customize", "Customize…", 9.0, 195.0, 110.0, false);
        }
        ADVANCED_PANEL => {
            d.group("Extra Font Folders", 11.0, 36.0, 420.0, 272.0);
            for slot in 0..settings::MAX_FONT_FOLDERS {
                let y = 56.0 + 21.0 * slot as f32;
                let folder = s.font_folders.get(slot);
                let label = folder.map_or("(empty)".to_string(), |f| shorten(&f.path));
                d.check(
                    FOLDER_ON[slot],
                    &label,
                    18.0,
                    y + 2.0,
                    folder.is_some_and(|f| f.enabled),
                );
                d.button(FOLDER_CHOOSE[slot], "Choose…", 318.0, y, 70.0, false);
                d.button(FOLDER_CLEAR[slot], "Clear", 390.0, y, 36.0, false);
                d.enable(FOLDER_ON[slot], folder.is_some());
                d.enable(FOLDER_CLEAR[slot], folder.is_some());
            }
            d.text("Folder levels to search (0-7):", 18.0, 282.0);
            d.field(
                "font_levels",
                &s.font_folder_levels.to_string(),
                200.0,
                279.0,
                40.0,
            );
            d.note(
                "font_help",
                "Fonts load only while a folder is checked. Documents that use them",
                11.0,
                314.0,
                420.0,
            );
            d.note(
                "font_help2",
                "show in other fonts on a computer, or a time, when they are not loaded.",
                11.0,
                327.0,
                420.0,
            );
            d.text("Save a recovery copy:", 11.0, 358.0);
            let at = AUTOSAVE
                .iter()
                .position(|a| a.1 == s.autosave_secs)
                .unwrap_or(1);
            let names: Vec<&str> = AUTOSAVE.iter().map(|a| a.0).collect();
            d.popup("autosave", &names, at, 139.0, 355.0, 150.0);
            d.text("Documents in Open Recent (1-50):", 11.0, 386.0);
            d.field(
                "recent_count",
                &s.recent_count.to_string(),
                200.0,
                383.0,
                48.0,
            );
        }
        _ => {
            d.text("Default Font:", 11.0, 43.0);
            let mut list: Vec<String> = fonts.to_vec();
            if !list.iter().any(|f| f.eq_ignore_ascii_case(&s.font)) {
                list.insert(0, s.font.clone());
            }
            let at = list
                .iter()
                .position(|f| f.eq_ignore_ascii_case(&s.font))
                .unwrap_or(0);
            d.popup_owned("font", list, at, 12.0, 61.0, 276.0);
            d.text("Default Size:", 304.0, 43.0);
            d.field("size", &crate::bars::format_size(s.size), 302.0, 61.0, 46.0);
            let at = SIZES
                .iter()
                .position(|z| crate::bars::format_size(s.size) == *z)
                .unwrap_or(2);
            d.popup("sizes", &SIZES, at, 350.0, 61.0, 18.0);
            d.focus("size");
        }
    }
    d
}

/// A folder path for a checkbox label: the home folder as ~, and the middle
/// cut out when it is too long to fit.
fn shorten(path: &std::path::Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home)
    {
        text = format!("~/{}", rest.display());
    }
    const MAX: usize = 46;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX {
        return text;
    }
    let head: String = chars[..MAX / 3].iter().collect();
    let tail: String = chars[chars.len() - (MAX - MAX / 3 - 1)..].iter().collect();
    format!("{head}…{tail}")
}

/// Whether `id` is a Choose… or Clear button of a font folder slot.
pub fn is_folder_button(id: &str) -> bool {
    FOLDER_CHOOSE.contains(&id) || FOLDER_CLEAR.contains(&id)
}

/// Put `path` in font folder slot `slot` (the next free one if the slot is
/// empty), switched on.
fn set_folder(slot: usize, path: std::path::PathBuf) {
    settings::update(|s| {
        let folder = settings::FontFolder {
            path,
            enabled: true,
        };
        if let Some(old) = s.font_folders.get_mut(slot) {
            *old = folder;
        } else if s.font_folders.len() < settings::MAX_FONT_FOLDERS {
            s.font_folders.push(folder);
        }
    });
}

fn clear_folder(slot: usize) {
    settings::update(|s| {
        if slot < s.font_folders.len() {
            s.font_folders.remove(slot);
        }
    });
}

/// Carry out Choose… or Clear for a font folder slot. Choosing asks for a
/// folder and warns that documents depend on it.
pub fn folder_button(id: &str) {
    if let Some(slot) = FOLDER_CLEAR.iter().position(|b| *b == id) {
        clear_folder(slot);
    } else if let Some(slot) = FOLDER_CHOOSE.iter().position(|b| *b == id) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose a Font Folder");
        if let Some(old) = settings::with(|s| s.font_folders.get(slot).map(|f| f.path.clone())) {
            dialog = dialog.set_directory(old);
        }
        if let Some(path) = dialog.pick_folder() {
            set_folder(slot, path);
            crate::app::warn(
                "These fonts are not installed.",
                "Quill loads them only while the folder is checked. A document that uses them \
                 will show in other fonts, and lines may break differently, on a computer \
                 without these fonts or when the folder is switched off.",
            );
        }
    }
}

/// Something changed in the panel: put it into effect.
pub fn changed(panel: usize, d: &mut Dialog, id: &str) {
    if id == "sizes" {
        let z = SIZES[d.selected("sizes").min(SIZES.len() - 1)];
        d.set_field("size", z);
    }
    let on = |id| d.is_on(id);
    match panel {
        0 => {
            let unit = Unit::ALL[d.selected("unit").min(Unit::ALL.len() - 1)];
            settings::update(|s| {
                s.name = d.field_text("name");
                s.initials = d.field_text("initials");
                s.unit = unit;
                if id == "defaultpaper"
                    && let Some(&(_, w, h)) =
                        crate::page_dialogs::PAPERS.get(d.selected("defaultpaper"))
                {
                    let landscape = s.document.page_width > s.document.page_height;
                    (s.document.page_width, s.document.page_height) =
                        if landscape { (h, w) } else { (w, h) };
                }
                s.smart_quotes = on("smart");
                s.auto_links = on("autolinks");
                s.formatted_clipboard = on("fmtclip");
            });
        }
        1 => settings::update(|s| {
            s.show_hidden = on("hidden");
            s.text_boundaries = on("boundaries");
            s.table_gridlines = on("gridlines");
            s.default_view = settings::DefaultView::ALL
                .get(d.selected("defaultview"))
                .copied()
                .unwrap_or_default();
            s.open_with_ruler = on("ruler");
            s.open_with_formatting_toolbar = on("formatting");
            s.open_with_paragraph_toolbar = on("paragraph");
            s.recent_files = on("recent");
        }),
        2 => settings::update(|s| {
            s.interpret_rtf = on("rtf");
            s.make_backup = on("backup");
            s.foreign_as_quill = on("foreign_quill");
            s.reopen = settings::Reopen::ALL
                .get(d.selected("reopen"))
                .copied()
                .unwrap_or_default();
        }),
        TOOLBAR_PANEL => {
            let at = match d.radio_on(1) {
                "tbleft" => 1,
                "tbright" => 2,
                _ => 0,
            };
            settings::update(|s| {
                s.view_toolbar = on("viewtb");
                s.paragraph_toolbar_tabs = on("tabbuttons");
                s.toolbar_position = at;
            });
        }
        SPELLING_PANEL => settings::update(|s| {
            s.always_suggest = on("suggest");
            s.spelling_as_you_type = on("live_spelling");
            s.words_to_system = d.selected("add_words") == 1;
            s.ignore_uppercase = on("upper");
            s.ignore_numbers = on("numbers");
        }),
        ADVANCED_PANEL => settings::update(|s| {
            for (slot, folder) in s.font_folders.iter_mut().enumerate() {
                folder.enabled = on(FOLDER_ON[slot]);
            }
            if let Ok(levels) = d.field_text("font_levels").trim().parse::<u8>()
                && levels <= settings::MAX_FONT_LEVELS
            {
                s.font_folder_levels = levels;
            }
            if id == "autosave"
                && let Some(&(_, secs)) = AUTOSAVE.get(d.selected("autosave"))
            {
                s.autosave_secs = secs;
            }
            if let Ok(count) = d.field_text("recent_count").trim().parse::<usize>()
                && (1..=50).contains(&count)
            {
                s.recent_count = count;
            }
        }),
        _ => {
            let font = d.selected_text("font");
            let size: Option<f32> = d.field_text("size").trim().parse().ok();
            settings::update(|s| {
                if !font.is_empty() {
                    s.font = font;
                }
                if let Some(z) = size.filter(|z| (1.0..=1638.0).contains(z)) {
                    s.size = (z * 2.0).round() as u16;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopen_documents_preference_saves_and_reads_back() {
        settings::update(|s| *s = settings::Settings::default());
        let mut d = preferences(2, &[]);
        assert_eq!(d.selected("reopen"), 2);
        d.set_selected("reopen", 0);
        changed(2, &mut d, "reopen");
        assert_eq!(settings::get().reopen, settings::Reopen::Always);
        settings::update(|s| s.last_session = vec!["/a/One.qwp".into(), "/b/Two.rtf".into()]);
        let back = settings::Settings::from_text(&settings::get().to_text());
        assert_eq!(back.reopen, settings::Reopen::Always);
        assert_eq!(back.last_session, settings::get().last_session);
        for item in &d.items {
            assert!(
                item.rect.right() <= d.width,
                "{} falls outside Preferences",
                item.id
            );
        }
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn recent_count_and_save_format_preferences_are_live_and_bounded() {
        settings::update(|s| *s = settings::Settings::default());
        assert_eq!(settings::get().recent_count, 10);
        let mut d = preferences(ADVANCED_PANEL, &[]);
        d.set_field("recent_count", "25");
        changed(ADVANCED_PANEL, &mut d, "recent_count");
        assert_eq!(settings::get().recent_count, 25);
        d.set_field("recent_count", "0");
        changed(ADVANCED_PANEL, &mut d, "recent_count");
        assert_eq!(settings::get().recent_count, 25);
        let mut d = preferences(2, &[]);
        assert!(d.is_on("foreign_quill"));
        d.set_on("foreign_quill", false);
        changed(2, &mut d, "foreign_quill");
        assert!(!settings::get().foreign_as_quill);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn custom_dictionary_is_default_but_system_is_an_explicit_preference() {
        settings::update(|s| *s = settings::Settings::default());
        let mut d = preferences(SPELLING_PANEL, &[]);
        assert_eq!(d.selected("add_words"), 0);
        assert!(d.is_enabled("dictionary_new"));
        assert!(d.is_enabled("dictionary_choose"));
        d.set_selected("add_words", 1);
        changed(SPELLING_PANEL, &mut d, "add_words");
        assert!(settings::get().words_to_system);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn live_spelling_checkbox_applies_and_fits_the_panel() {
        settings::update(|s| *s = settings::Settings::default());
        let mut d = preferences(SPELLING_PANEL, &[]);
        assert!(d.is_on("live_spelling"));
        d.set_on("live_spelling", false);
        changed(SPELLING_PANEL, &mut d, "live_spelling");
        assert!(!settings::get().spelling_as_you_type);
        assert!(!settings::Settings::from_text(&settings::get().to_text()).spelling_as_you_type);
        for item in &d.items {
            assert!(
                item.rect.bottom() <= d.height,
                "{} falls outside Preferences",
                item.id
            );
        }
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn every_panel_builds() {
        for p in 0..PANELS.len() {
            let d = preferences(p, &["Arial".to_string()]);
            assert!(d.close_box);
            assert!(d.modeless);
            assert!(
                !d.items
                    .iter()
                    .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
            );
            assert_eq!(d.selected("panel"), p);
            assert!(
                d.items
                    .iter()
                    .filter(|item| !item.enabled)
                    .all(|item| item.id.starts_with("fon") || item.id.starts_with("fclear"))
            );
        }
    }

    #[test]
    fn changes_take_effect_at_once() {
        let mut d = preferences(0, &[]);
        d.set_field("name", "Ada");
        d.set_selected("unit", 1);
        changed(0, &mut d, "unit");
        let s = settings::get();
        assert_eq!(s.name, "Ada");
        assert_eq!(s.unit, Unit::Cm);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn recent_files_is_a_live_view_preference() {
        let mut d = preferences(1, &[]);
        d.set_on("recent", false);
        changed(1, &mut d, "recent");
        assert!(!settings::get().recent_files);
        settings::update(|s| *s = settings::Settings::default());
    }
    #[test]
    fn default_paper_is_persistent_and_only_affects_new_documents() {
        settings::update(|s| *s = settings::Settings::default());
        let existing = settings::get().new_document();
        let mut d = preferences(0, &[]);
        d.set_selected("defaultpaper", 2);
        changed(0, &mut d, "defaultpaper");
        let s = settings::get();
        assert_eq!(
            (s.document.page_width, s.document.page_height),
            (11906, 16838)
        );
        assert_eq!(existing.props.page_width, 12240);
        assert_eq!(
            settings::Settings::from_text(&s.to_text())
                .new_document()
                .props
                .page_width,
            11906
        );
    }

    #[test]
    fn font_folders_toggle_clear_and_fit_the_advanced_panel() {
        settings::update(|s| *s = settings::Settings::default());
        set_folder(0, "/Fonts/Google".into());
        set_folder(1, "/Fonts/Trial".into());
        let mut d = preferences(ADVANCED_PANEL, &[]);
        assert!(d.is_on("fon0") && d.is_on("fon1"));
        assert!(!d.is_enabled("fon2") && !d.is_enabled("fclear2"));
        d.set_on("fon1", false);
        changed(ADVANCED_PANEL, &mut d, "fon1");
        let folders = settings::get().font_folders;
        assert!(folders[0].enabled && !folders[1].enabled);
        // The off folder stays in its slot and survives a save and reload.
        let back = settings::Settings::from_text(&settings::get().to_text());
        assert_eq!(back.font_folders, folders);
        clear_folder(0);
        assert_eq!(settings::get().font_folders.len(), 1);
        assert_eq!(
            settings::get().font_folders[0].path,
            std::path::Path::new("/Fonts/Trial")
        );
        for item in &d.items {
            assert!(item.rect.right() <= d.width, "{} is too wide", item.id);
            assert!(item.rect.bottom() <= d.height, "{} is too low", item.id);
        }
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn font_search_depth_and_autosave_are_bounded_and_saved() {
        settings::update(|s| *s = settings::Settings::default());
        assert_eq!(settings::get().font_folder_levels, 4);
        assert_eq!(settings::get().autosave_secs, 30);
        let mut d = preferences(ADVANCED_PANEL, &[]);
        d.set_field("font_levels", "7");
        changed(ADVANCED_PANEL, &mut d, "font_levels");
        assert_eq!(settings::get().font_folder_levels, 7);
        d.set_field("font_levels", "8");
        changed(ADVANCED_PANEL, &mut d, "font_levels");
        assert_eq!(settings::get().font_folder_levels, 7);
        d.set_selected("autosave", 4);
        changed(ADVANCED_PANEL, &mut d, "autosave");
        assert_eq!(settings::get().autosave_secs, 300);
        let back = settings::Settings::from_text(&settings::get().to_text());
        assert_eq!((back.font_folder_levels, back.autosave_secs), (7, 300));
        d.set_selected("autosave", 0);
        changed(ADVANCED_PANEL, &mut d, "autosave");
        assert_eq!(settings::get().autosave_secs, 0);
        assert_eq!(
            settings::Settings::from_text("font_folder_levels = 99\n").font_folder_levels,
            7
        );
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn long_folder_paths_are_shortened_in_the_middle() {
        let long = std::path::Path::new("/Volumes/Archive/Fonts/Collections/Downloads/Latest");
        let text = shorten(long);
        assert!(text.chars().count() <= 46);
        assert!(text.contains('…') && text.starts_with("/Volumes") && text.ends_with("Latest"));
        assert_eq!(shorten(std::path::Path::new("/Fonts")), "/Fonts");
    }
}
