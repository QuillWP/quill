//! Tools > Preferences: a native category chooser and the chosen panel's
//! settings. As in the original there is no OK button:
//! every change takes effect at once, and the close box puts it away.
//! Layout measured from the original.

use crate::dialog::Dialog;
use crate::settings::{self, Unit};

/// Panels: name and icon. Grammar, Thesaurus and Hyphenation have no
/// settings in Quill yet, so their panels are left out.
const PANELS: [&str; 6] = [
    "General",
    "View",
    "Open And Save",
    "Default Font",
    "Spelling",
    "Toolbar",
];
/// The Spelling panel, which the Spelling window's Options… opens.
pub const DEFAULT_FONT_PANEL: usize = 3;
pub const SPELLING_PANEL: usize = 4;
pub const TOOLBAR_PANEL: usize = 5;
const SIZES: [&str; 11] = [
    "9", "10", "12", "14", "18", "24", "36", "48", "60", "72", "96",
];

/// The Preferences window showing `panel`, with `fonts` for Default Font.
pub fn preferences(panel: usize, fonts: &[String]) -> Dialog {
    let s = settings::get();
    let mut d = Dialog::new("Preferences", 442.0, 332.0);
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
            // Paper size is a document's own (Page Setup), so General
            // has no custom paper size.
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
                "fmtclip",
                "Include Formatted Text in Clipboard",
                12.0,
                143.0,
                s.formatted_clipboard,
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
            d.check(
                "pagelayout",
                "In Page Layout View",
                16.0,
                157.0,
                s.open_page_layout,
            );
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
            d.group("Menus", 11.0, 246.0, 364.0, 80.0);
            d.check(
                "recent",
                "List Recently Opened Documents",
                16.0,
                269.0,
                s.recent_files,
            );
            d.text("Documents to remember (1-50):", 16.0, 296.0);
            d.field(
                "recent_count",
                &s.recent_count.to_string(),
                274.0,
                293.0,
                48.0,
            );
            d.enable("recent_count", s.recent_files);
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
            d.check("suggest", "Always Suggest", 14.0, 187.0, s.always_suggest);
            d.group("Ignore", 8.0, 212.0, 269.0, 66.0);
            d.check(
                "upper",
                "Words in UPPERCASE",
                14.0,
                235.0,
                s.ignore_uppercase,
            );
            d.check(
                "numbers",
                "Words with Numbers",
                14.0,
                256.0,
                s.ignore_numbers,
            );
            d.button(
                "reset",
                "Reset \u{201C}Ignore All\u{201D} List",
                8.0,
                284.0,
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

/// Something changed in the panel: put it into effect.
pub fn changed(panel: usize, d: &mut Dialog, id: &str) {
    if panel == 1 && id == "recent" {
        d.enable("recent_count", d.is_on("recent"));
    }
    if id == "sizes" {
        let z = SIZES[d.selected("sizes").min(SIZES.len() - 1)];
        d.set_field("size", z);
    }
    let on = |id| d.is_on(id);
    match panel {
        0 => {
            let unit = Unit::ALL[d.selected("unit").min(3)];
            settings::update(|s| {
                s.name = d.field_text("name");
                s.initials = d.field_text("initials");
                s.unit = unit;
                s.smart_quotes = on("smart");
                s.formatted_clipboard = on("fmtclip");
            });
        }
        1 => settings::update(|s| {
            s.show_hidden = on("hidden");
            s.text_boundaries = on("boundaries");
            s.table_gridlines = on("gridlines");
            s.open_page_layout = on("pagelayout");
            s.open_with_ruler = on("ruler");
            s.open_with_formatting_toolbar = on("formatting");
            s.open_with_paragraph_toolbar = on("paragraph");
            s.recent_files = on("recent");
            if let Ok(count) = d.field_text("recent_count").parse::<usize>()
                && (1..=50).contains(&count)
            {
                s.recent_count = count;
            }
        }),
        2 => settings::update(|s| {
            s.interpret_rtf = on("rtf");
            s.make_backup = on("backup");
            s.foreign_as_quill = on("foreign_quill");
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
            s.words_to_system = d.selected("add_words") == 1;
            s.ignore_uppercase = on("upper");
            s.ignore_numbers = on("numbers");
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
    fn recent_count_and_save_format_preferences_are_live_and_bounded() {
        settings::update(|s| *s = settings::Settings::default());
        let mut d = preferences(1, &[]);
        d.set_field("recent_count", "25");
        changed(1, &mut d, "recent_count");
        assert_eq!(settings::get().recent_count, 25);
        d.set_field("recent_count", "0");
        changed(1, &mut d, "recent_count");
        assert_eq!(settings::get().recent_count, 25);
        d.set_on("recent", false);
        changed(1, &mut d, "recent");
        assert!(!d.is_enabled("recent_count"));
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
                    .all(|item| item.id == "recent_count")
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
}
