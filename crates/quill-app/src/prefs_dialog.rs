//! Preferences keeps one draft across all panels until Save or Save and Close.
//! Cancel and the close box discard changes since the last save.

use crate::dialog::Dialog;
use crate::settings::{self, Unit};

/// Panels: name and icon. Grammar, Thesaurus and Hyphenation have no
/// settings in Quill yet, so their panels are left out.
const PANELS: [&str; 8] = [
    "General",
    "View",
    "Open And Save",
    "Fonts",
    "Spelling",
    "Toolbar",
    "Advanced",
    "Colors",
];
/// The Spelling panel, which the Spelling window's Options… opens.
pub const DEFAULT_FONT_PANEL: usize = 3;
pub const SPELLING_PANEL: usize = 4;
pub const TOOLBAR_PANEL: usize = 5;
pub const ADVANCED_PANEL: usize = 6;
pub const COLORS_PANEL: usize = 7;

const COLOR_IDS: [&str; settings::MAX_NAMED_COLORS] = [
    "color0", "color1", "color2", "color3", "color4", "color5", "color6", "color7",
];
const COLOR_NAMES: [&str; settings::MAX_NAMED_COLORS] = [
    "cname0", "cname1", "cname2", "cname3", "cname4", "cname5", "cname6", "cname7",
];
/// The box beside each color that opens the palette, and its Clear button.
const COLOR_BOXES: [&str; settings::MAX_NAMED_COLORS] = [
    "cbox0", "cbox1", "cbox2", "cbox3", "cbox4", "cbox5", "cbox6", "cbox7",
];
const COLOR_CLEAR: [&str; settings::MAX_NAMED_COLORS] = [
    "cclear0", "cclear1", "cclear2", "cclear3", "cclear4", "cclear5", "cclear6", "cclear7",
];
/// What an empty row shows instead of an HTML code.
const NO_COLOR: &str = "None";
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
/// How large the toolbars, ruler and dialogs are drawn, in percent.
const INTERFACE_SIZES: [u16; 5] = [90, 100, 110, 125, 150];
/// The zooms Preferences > View offers for opening documents.
const DOCUMENT_ZOOMS: [u16; 9] = [50, 75, 90, 100, 110, 125, 150, 175, 200];

/// The zooms to offer, with `now` among them if it is not one of the
/// usual ones (the toolbar can leave any zoom), and which is `now`.
fn zoom_choices(now: f64) -> (Vec<u16>, usize) {
    let now = now.round().clamp(10.0, 800.0) as u16;
    let mut zooms = DOCUMENT_ZOOMS.to_vec();
    if !zooms.contains(&now) {
        zooms.push(now);
        zooms.sort_unstable();
    }
    let at = zooms.iter().position(|z| *z == now).unwrap_or(0);
    (zooms, at)
}
const SIZES: [&str; 11] = [
    "9", "10", "12", "14", "18", "24", "36", "48", "60", "72", "96",
];

/// The Preferences window showing `panel`, with `families` for Default Font.
pub fn preferences(panel: usize, families: &[quill_layout::FontFamilyInfo]) -> Dialog {
    preferences_with(panel, families, &settings::get())
}

pub fn preferences_with(
    panel: usize,
    families: &[quill_layout::FontFamilyInfo],
    s: &settings::Settings,
) -> Dialog {
    let height = if panel == ADVANCED_PANEL {
        480.0
    } else {
        376.0
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
                "autolinks",
                "Make Typed Web and Mail Addresses Links",
                12.0,
                126.0,
                s.auto_links,
            );
            d.check(
                "fmtclip",
                "Include Formatted Text in Clipboard",
                12.0,
                143.0,
                s.formatted_clipboard,
            );
            d.check(
                "autolists",
                "Start Lists from a Typed * or -",
                12.0,
                160.0,
                s.auto_lists,
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
            d.text("Zoom:", 246.0, 157.0);
            let (zooms, at) = zoom_choices(s.document_zoom);
            let names: Vec<String> = zooms.iter().map(|z| format!("{z}%")).collect();
            d.popup_owned("defaultzoom", names, at, 290.0, 154.0, 80.0);
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
            d.text("Interface Size:", 16.0, 303.0);
            let names: Vec<String> = INTERFACE_SIZES.iter().map(|z| format!("{z}%")).collect();
            let at = INTERFACE_SIZES
                .iter()
                .position(|z| *z == s.interface_scale)
                .unwrap_or(1);
            d.popup_owned("ui_size", names, at, 110.0, 300.0, 90.0);
            d.text("Appearance:", 216.0, 303.0);
            let names: Vec<&str> = settings::Appearance::ALL
                .iter()
                .map(|a| a.label())
                .collect();
            let at = settings::Appearance::ALL
                .iter()
                .position(|a| *a == s.appearance)
                .unwrap_or(0);
            d.popup("appearance", &names, at, 290.0, 300.0, 80.0);
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
            let path = s
                .custom_dictionary
                .clone()
                .or_else(crate::local_dictionary::path);
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
        COLORS_PANEL => {
            d.text(
                "Colors for every document. They lead the Custom row of the",
                11.0,
                40.0,
            );
            d.text("palette, and pointing at one shows its name.", 11.0, 56.0);
            d.text("Color", 11.0, 80.0);
            d.text("Name", 136.0, 80.0);
            for row in 0..settings::MAX_NAMED_COLORS {
                let y = 98.0 + 25.0 * row as f32;
                let (color, name) = s.named_colors.get(row).map_or((None, ""), |(c, n)| {
                    (Some(quill_core::props::Rgb(*c)), n.as_str())
                });
                d.swatch_button(COLOR_BOXES[row], 11.0, y, 44.0, 22.0, 0xFFFFFF);
                d.text_as(COLOR_IDS[row], NO_COLOR, 62.0, y + 4.0);
                show_color(&mut d, COLOR_BOXES[row], color);
                d.field(COLOR_NAMES[row], name, 136.0, y, 226.0);
                d.button(COLOR_CLEAR[row], "Clear", 370.0, y, 60.0, false);
            }
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
            d.check(
                "smart",
                "\u{201C}Smart\u{201D} Quotes",
                12.0,
                412.0,
                s.smart_quotes,
            );
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
            // Each row previews its face. Bold and Italic are in the Style
            // pop-up beside it, which lists the faces the chosen family
            // has. A saved font that is not installed stays listed first
            // so saving does not change it.
            let mut list = families.to_vec();
            if !list.iter().any(|f| f.name.eq_ignore_ascii_case(&s.font)) {
                list.insert(
                    0,
                    quill_layout::FontFamilyInfo {
                        name: s.font.clone(),
                        draws_name: false,
                        variants: vec![quill_layout::FontVariant {
                            bold: false,
                            italic: false,
                        }],
                    },
                );
            }
            let at = list
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case(&s.font))
                .unwrap_or(0);
            let style = faces(list.get(at));
            let plain: Vec<quill_layout::FontFamilyInfo> = list
                .iter()
                .map(|f| quill_layout::FontFamilyInfo {
                    variants: vec![quill_layout::FontVariant {
                        bold: false,
                        italic: false,
                    }],
                    ..f.clone()
                })
                .collect();
            d.font_popup("font", &plain, at, 12.0, 61.0, 180.0);
            d.text("Style:", 198.0, 43.0);
            let names: Vec<&str> = style.iter().map(|&f| face_name(f)).collect();
            let chosen = style
                .iter()
                .position(|&f| f == (s.font_bold, s.font_italic))
                .unwrap_or(0);
            d.popup("style", &names, chosen, 198.0, 61.0, 98.0);
            d.text("Default Size:", 304.0, 43.0);
            d.field("size", &crate::bars::format_size(s.size), 302.0, 61.0, 46.0);
            let at = SIZES
                .iter()
                .position(|z| crate::bars::format_size(s.size) == *z)
                .unwrap_or(2);
            d.popup("sizes", &SIZES, at, 350.0, 61.0, 18.0);
            let sc = s.document.scripts;
            d.group("Superscript and Subscript", 11.0, 100.0, 420.0, 92.0);
            d.text("Superscript: raised", 16.0, 124.0);
            d.field("raise", &sc.raise.to_string(), 150.0, 121.0, 40.0);
            d.text("% of the size,", 194.0, 124.0);
            d.field("supsize", &sc.super_size.to_string(), 290.0, 121.0, 40.0);
            d.text("% as big", 334.0, 124.0);
            d.text("Subscript: lowered", 16.0, 152.0);
            d.field("drop", &sc.drop.to_string(), 150.0, 149.0, 40.0);
            d.text("% of the size,", 194.0, 152.0);
            d.field("subsize", &sc.sub_size.to_string(), 290.0, 149.0, 40.0);
            d.text("% as big", 334.0, 152.0);
            d.text(
                "For new documents. Format > Document changes the current one.",
                16.0,
                176.0,
            );
            d.focus("size");
        }
    }
    d.button("cancel", "Cancel", 135.0, height - 35.0, 80.0, false);
    d.button("save", "Save", 223.0, height - 35.0, 70.0, false);
    d.button("ok", "Save and Close", 301.0, height - 35.0, 130.0, true);
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
#[cfg(test)]
fn set_folder(slot: usize, path: std::path::PathBuf) {
    settings::update(|s| set_folder_in(s, slot, path));
}

fn set_folder_in(s: &mut settings::Settings, slot: usize, path: std::path::PathBuf) {
    let folder = settings::FontFolder {
        path,
        enabled: true,
    };
    if let Some(old) = s.font_folders.get_mut(slot) {
        *old = folder;
    } else if s.font_folders.len() < settings::MAX_FONT_FOLDERS {
        s.font_folders.push(folder);
    }
}

#[cfg(test)]
fn clear_folder(slot: usize) {
    settings::update(|s| clear_folder_in(s, slot));
}

fn clear_folder_in(s: &mut settings::Settings, slot: usize) {
    if slot < s.font_folders.len() {
        s.font_folders.remove(slot);
    }
}

/// Carry out Choose… or Clear for a font folder slot. Choosing asks for a
/// folder and warns that documents depend on it.
pub fn folder_button(id: &str, s: &mut settings::Settings) {
    if let Some(slot) = FOLDER_CLEAR.iter().position(|b| *b == id) {
        clear_folder_in(s, slot);
    } else if let Some(slot) = FOLDER_CHOOSE.iter().position(|b| *b == id) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose a Font Folder");
        if let Some(old) = s.font_folders.get(slot).map(|f| f.path.clone()) {
            dialog = dialog.set_directory(old);
        }
        if let Some(path) = dialog.pick_folder() {
            set_folder_in(s, slot, path);
            crate::app::warn(
                "These fonts are not installed.",
                "Quill loads them only while the folder is checked. A document that uses them \
                 will show in other fonts, and lines may break differently, on a computer \
                 without these fonts or when the folder is switched off.",
            );
        }
    }
}

/// Every face a Style pop-up can offer, in the order it lists them, as
/// (bold, italic).
const FACES: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

fn face_name(face: (bool, bool)) -> &'static str {
    match face {
        (false, false) => "Normal",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (true, true) => "Bold Italic",
    }
}

/// The faces `family` has, for its Style pop-up: always Normal, then any
/// of Bold, Italic and Bold Italic it has.
fn faces(family: Option<&quill_layout::FontFamilyInfo>) -> Vec<(bool, bool)> {
    FACES
        .into_iter()
        .filter(|&f| {
            f == (false, false)
                || family.is_some_and(|fam| fam.variants.iter().any(|v| (v.bold, v.italic) == f))
        })
        .collect()
}

/// After the Default Font's family changes: a face the new family does not
/// have goes back to Normal. The window is then built again so its Style
/// pop-up lists the new family's faces.
pub fn font_changed(families: &[quill_layout::FontFamilyInfo], s: &mut settings::Settings) {
    let family = families
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(&s.font));
    if !faces(family).contains(&(s.font_bold, s.font_italic)) {
        s.font_bold = false;
        s.font_italic = false;
    }
}

/// Capture the current panel in the shared draft.
pub fn changed_in(panel: usize, d: &mut Dialog, id: &str, s: &mut settings::Settings) {
    if id == "sizes" {
        let z = SIZES[d.selected("sizes").min(SIZES.len() - 1)];
        d.set_field("size", z);
    }
    let on = |id| d.is_on(id);
    match panel {
        0 => {
            let unit = Unit::ALL[d.selected("unit").min(Unit::ALL.len() - 1)];
            {
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
                s.auto_links = on("autolinks");
                s.formatted_clipboard = on("fmtclip");
                s.auto_lists = on("autolists");
            }
        }
        1 => {
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
            s.appearance = settings::Appearance::ALL
                .get(d.selected("appearance"))
                .copied()
                .unwrap_or_default();
            if id == "ui_size" {
                s.interface_scale = INTERFACE_SIZES[d.selected("ui_size").min(4)];
            }
            // Only a choice made here changes it: the list rounds an odd zoom
            // left by the toolbar to the nearest whole percent.
            if id == "defaultzoom" {
                let (zooms, _) = zoom_choices(s.document_zoom);
                if let Some(z) = zooms.get(d.selected("defaultzoom")) {
                    s.document_zoom = *z as f64;
                }
            }
        }
        2 => {
            s.interpret_rtf = on("rtf");
            s.make_backup = on("backup");
            s.foreign_as_quill = on("foreign_quill");
            s.reopen = settings::Reopen::ALL
                .get(d.selected("reopen"))
                .copied()
                .unwrap_or_default();
        }
        TOOLBAR_PANEL => {
            let at = match d.radio_on(1) {
                "tbleft" => 1,
                "tbright" => 2,
                _ => 0,
            };
            {
                s.view_toolbar = on("viewtb");
                s.paragraph_toolbar_tabs = on("tabbuttons");
                s.toolbar_position = at;
            }
        }
        SPELLING_PANEL => {
            s.always_suggest = on("suggest");
            s.spelling_as_you_type = on("live_spelling");
            s.words_to_system = d.selected("add_words") == 1;
            s.ignore_uppercase = on("upper");
            s.ignore_numbers = on("numbers");
        }
        COLORS_PANEL => {
            s.named_colors = COLOR_IDS
                .iter()
                .zip(COLOR_NAMES)
                .filter_map(|(hex, name)| {
                    let c = crate::palette::parse_hex(d.item_label(hex))?;
                    Some((c.0, d.field_text(name).trim().to_string()))
                })
                .collect();
        }
        ADVANCED_PANEL => {
            for (slot, folder) in s.font_folders.iter_mut().enumerate() {
                folder.enabled = on(FOLDER_ON[slot]);
            }
            if let Ok(levels) = d.field_text("font_levels").trim().parse::<u8>()
                && levels <= settings::MAX_FONT_LEVELS
            {
                s.font_folder_levels = levels;
            }
            s.smart_quotes = on("smart");
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
        }
        _ => {
            let font = d.selected_text("font");
            let size: Option<f32> = d.field_text("size").trim().parse().ok();
            {
                if !font.is_empty() {
                    s.font = font;
                }
                // The face chosen in the Style pop-up.
                if let Some(&(bold, italic)) = FACES
                    .iter()
                    .find(|&&f| face_name(f) == d.selected_text("style"))
                {
                    s.font_bold = bold;
                    s.font_italic = italic;
                }
                if let Some(z) = size.filter(|z| (1.0..=1638.0).contains(z)) {
                    s.size = (z * 2.0).round() as u16;
                }
                // A field left as it was, or not a number, keeps its value.
                let pct = |id: &str, old: u16, range: std::ops::RangeInclusive<u16>| {
                    d.field_text(id)
                        .trim()
                        .trim_end_matches('%')
                        .parse::<u16>()
                        .map_or(old, |n| n.clamp(*range.start(), *range.end()))
                };
                let sc = &mut s.document.scripts;
                use quill_core::ScriptSizes as Sizes;
                sc.raise = pct("raise", sc.raise, Sizes::OFFSETS);
                sc.super_size = pct("supsize", sc.super_size, Sizes::SIZES);
                sc.drop = pct("drop", sc.drop, Sizes::OFFSETS);
                sc.sub_size = pct("subsize", sc.sub_size, Sizes::SIZES);
            }
        }
    }
}

/// The row of Colors whose box `id` is.
pub fn color_box(id: &str) -> Option<usize> {
    COLOR_BOXES.iter().position(|b| *b == id)
}

/// The color a row of Colors has, by its box's id.
pub fn box_color(d: &Dialog, id: &str) -> Option<quill_core::props::Rgb> {
    let row = color_box(id)?;
    crate::palette::parse_hex(d.item_label(COLOR_IDS[row]))
}

/// Show `color` in the row of box `id`: the box and the HTML code, or an
/// empty box and "None".
pub fn show_color(d: &mut Dialog, id: &str, color: Option<quill_core::props::Rgb>) {
    let Some(row) = color_box(id) else {
        return;
    };
    d.set_swatch(COLOR_BOXES[row], color.map_or(0xFFFFFF, |c| c.0));
    let text = color.map_or(NO_COLOR.to_string(), crate::palette::hex);
    d.set_label(COLOR_IDS[row], &text);
}

/// A row's Clear button: no color and no name.
pub fn clear_color(d: &mut Dialog, id: &str) -> bool {
    let Some(row) = COLOR_CLEAR.iter().position(|b| *b == id) else {
        return false;
    };
    show_color(d, COLOR_BOXES[row], None);
    d.set_field(COLOR_NAMES[row], "");
    true
}

/// Apply only settings edited in Preferences, preserving unrelated changes made elsewhere.
pub fn save(original: &mut settings::Settings, draft: &settings::Settings) {
    settings::update(|current| {
        macro_rules! copy_changes {
            ($($field:ident),* $(,)?) => {$(
                if draft.$field != original.$field {
                    current.$field.clone_from(&draft.$field);
                }
            )*};
        }
        copy_changes!(
            name,
            initials,
            unit,
            auto_links,
            formatted_clipboard,
            auto_lists,
            show_hidden,
            text_boundaries,
            table_gridlines,
            default_view,
            appearance,
            open_with_ruler,
            open_with_formatting_toolbar,
            open_with_paragraph_toolbar,
            recent_files,
            interpret_rtf,
            make_backup,
            foreign_as_quill,
            reopen,
            view_toolbar,
            paragraph_toolbar_tabs,
            toolbar_position,
            always_suggest,
            spelling_as_you_type,
            words_to_system,
            ignore_uppercase,
            ignore_numbers,
            named_colors,
            font_folders,
            font_folder_levels,
            smart_quotes,
            interface_scale,
            autosave_secs,
            recent_count,
            font,
            font_bold,
            font_italic,
            size,
            document_zoom,
            custom_dictionary,
        );
        if draft.document.page_width != original.document.page_width {
            current.document.page_width = draft.document.page_width;
        }
        if draft.document.page_height != original.document.page_height {
            current.document.page_height = draft.document.page_height;
        }
        if draft.document.scripts != original.document.scripts {
            current.document.scripts = draft.document.scripts;
        }
    });
    *original = draft.clone();
    crate::language::reload_dictionary();
}

#[cfg(test)]
pub fn changed(panel: usize, d: &mut Dialog, id: &str) {
    settings::update(|s| changed_in(panel, d, id, s));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(name: &str) -> quill_layout::FontFamilyInfo {
        quill_layout::FontFamilyInfo {
            name: name.into(),
            draws_name: true,
            variants: vec![
                quill_layout::FontVariant {
                    bold: false,
                    italic: false,
                },
                quill_layout::FontVariant {
                    bold: true,
                    italic: false,
                },
            ],
        }
    }

    #[test]
    fn default_font_menu_lists_families_with_their_styles() {
        settings::update(|s| *s = settings::Settings::default());
        settings::update(|s| s.font = "Georgia".into());
        let d = preferences(DEFAULT_FONT_PANEL, &[family("Arial"), family("Georgia")]);
        assert_eq!(d.options("font"), ["Arial", "Georgia"]);
        assert_eq!(d.selected_text("font"), "Georgia");
        // A saved font that is not installed stays listed and selected.
        settings::update(|s| s.font = "Gone Sans".into());
        let d = preferences(DEFAULT_FONT_PANEL, &[family("Arial")]);
        assert_eq!(d.options("font"), ["Gone Sans", "Arial"]);
        assert_eq!(d.selected_text("font"), "Gone Sans");
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn saving_keeps_every_choice_the_font_panel_makes() {
        settings::update(|s| *s = settings::Settings::default());
        let families = [family("Arial"), family("Georgia")];
        let mut original = settings::get();
        let mut draft = original.clone();
        let mut d = preferences_with(DEFAULT_FONT_PANEL, &families, &draft);
        d.set_selected("font", 1);
        changed_in(DEFAULT_FONT_PANEL, &mut d, "font", &mut draft);
        let mut d = preferences_with(DEFAULT_FONT_PANEL, &families, &draft);
        d.set_selected("style", 1);
        d.set_field("raise", "40");
        changed_in(DEFAULT_FONT_PANEL, &mut d, "save", &mut draft);
        save(&mut original, &draft);
        let s = settings::get();
        assert_eq!(
            (s.font.as_str(), s.font_bold, s.font_italic),
            ("Georgia", true, false)
        );
        assert_eq!(s.document.scripts.raise, 40);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn the_default_fonts_bold_and_italic_choices_are_kept_and_used() {
        settings::update(|s| *s = settings::Settings::default());
        let mut arial = family("Arial");
        arial.variants.push(quill_layout::FontVariant {
            bold: true,
            italic: true,
        });
        let mut plain = family("Plain Sans");
        plain.variants.truncate(1);
        let families = [arial, family("Georgia"), plain];
        let mut d = preferences(DEFAULT_FONT_PANEL, &families);
        // The family pop-up has no submenus; the Style pop-up lists the
        // chosen family's faces.
        d.set_selected("font", 0);
        settings::update(|s| font_changed(&families, s));
        changed(DEFAULT_FONT_PANEL, &mut d, "font");
        let mut d = preferences(DEFAULT_FONT_PANEL, &families);
        assert_eq!(d.options("style"), ["Normal", "Bold", "Bold Italic"]);
        d.set_selected("style", 2);
        changed(DEFAULT_FONT_PANEL, &mut d, "style");
        let s = settings::get();
        assert_eq!(
            (s.font.as_str(), s.font_bold, s.font_italic),
            ("Arial", true, true)
        );
        // They are written to the settings file and read back.
        let back = settings::Settings::from_text(&s.to_text());
        assert_eq!((back.font_bold, back.font_italic), (true, true));
        // New documents start in them: Normal, and a style based on it.
        let doc = s.new_document();
        let normal = doc.styles.get(0).chp;
        assert!(normal.bold && normal.italic);
        let note = doc.styles.find("footnote text").unwrap();
        assert!(doc.styles.get(note).chp.italic);
        // Changing only the size leaves the style as it was, and so does a
        // window built again.
        d.set_field("size", "14");
        changed(DEFAULT_FONT_PANEL, &mut d, "size");
        assert!(settings::get().font_bold);
        let d = preferences(DEFAULT_FONT_PANEL, &families);
        assert_eq!(d.selected_text("style"), "Bold Italic");
        // A family with only one face takes the Style back to Normal.
        let mut d = d;
        d.set_selected("font", 2);
        changed(DEFAULT_FONT_PANEL, &mut d, "font");
        settings::update(|s| font_changed(&families, s));
        assert!(!settings::get().font_bold && !settings::get().font_italic);
        let d = preferences(DEFAULT_FONT_PANEL, &families);
        assert_eq!(d.options("style"), ["Normal"]);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn the_colors_panel_saves_a_named_color() {
        let mut d = preferences(COLORS_PANEL, &[]);
        assert_eq!(d.item_label("color0"), "None");
        show_color(&mut d, "cbox0", Some(quill_core::props::Rgb(0xFF0000)));
        d.set_field("cname0", "Company Color");
        // A name with no color is not saved.
        d.set_field("cname1", "Nothing yet");
        show_color(&mut d, "cbox2", Some(quill_core::props::Rgb(0x00FF00)));
        d.set_field("cname2", "Gone");
        assert!(clear_color(&mut d, "cclear2"));
        assert_eq!(d.item_label("color2"), "None");
        assert_eq!(d.field_text("cname2"), "");
        changed(COLORS_PANEL, &mut d, "cname0");
        let saved = settings::with(|s| s.named_colors.clone());
        let again = preferences(COLORS_PANEL, &[]);
        settings::update(|s| s.named_colors.clear());
        assert_eq!(saved, [(0xFF0000, "Company Color".to_string())]);
        assert_eq!(again.item_label("color0"), "#FF0000");
        assert_eq!(again.swatch_rgb("cbox0"), Some(0xFF0000));
    }

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
            let d = preferences(p, &[family("Arial")]);
            assert!(d.close_box);
            assert!(d.modeless);
            assert!(
                !d.items
                    .iter()
                    .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
            );
            assert_eq!(d.selected("panel"), p);
            for id in ["cancel", "save", "ok"] {
                let button = d.items.iter().find(|i| i.id == id).unwrap();
                assert!(button.rect.bottom() <= d.height);
                assert!(button.rect.y >= d.height - 40.0);
            }
            assert!(
                d.items
                    .iter()
                    .filter(|i| !["cancel", "save", "ok"].contains(&i.id))
                    .all(|i| i.rect.bottom() <= d.height - 44.0)
            );
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
    fn interface_size_is_a_live_view_preference() {
        let mut d = preferences(1, &[]);
        assert_eq!(d.selected("ui_size"), 1);
        d.set_selected("ui_size", 3);
        changed(1, &mut d, "ui_size");
        assert_eq!(settings::get().interface_scale, 125);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn appearance_is_chosen_in_the_view_panel() {
        let mut d = preferences(1, &[]);
        assert_eq!(d.selected("appearance"), 0);
        d.set_selected("appearance", 2);
        changed(1, &mut d, "appearance");
        assert_eq!(settings::get().appearance, settings::Appearance::Dark);
        let d = preferences(1, &[]);
        assert_eq!(d.selected("appearance"), 2);
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
    fn superscript_and_subscript_amounts_are_set_in_default_font() {
        settings::update(|s| *s = settings::Settings::default());
        let mut d = preferences(DEFAULT_FONT_PANEL, &[family("Georgia")]);
        assert_eq!(d.field_text("raise"), "33");
        d.set_field("raise", "25");
        d.set_field("supsize", "100%");
        d.set_field("subsize", "nonsense");
        changed(DEFAULT_FONT_PANEL, &mut d, "raise");
        let sc = settings::get().document.scripts;
        assert_eq!(
            (sc.raise, sc.super_size, sc.drop, sc.sub_size),
            (25, 100, 17, 67)
        );
        let back = settings::Settings::from_text(&settings::get().to_text());
        assert_eq!(back.document.scripts, sc);
        // New documents take them.
        assert_eq!(settings::get().new_document().props.scripts, sc);
        settings::update(|s| *s = settings::Settings::default());
    }

    #[test]
    fn default_zoom_is_chosen_in_the_view_panel() {
        settings::update(|s| *s = settings::Settings::default());
        settings::update(|s| s.document_zoom = 133.0);
        let mut d = preferences(1, &[]);
        // An odd zoom from the toolbar is listed so it shows as it is.
        let (zooms, at) = zoom_choices(133.0);
        assert_eq!(zooms[at], 133);
        assert_eq!(d.selected("defaultzoom"), at);
        let z125 = zooms.iter().position(|z| *z == 125).unwrap();
        d.set_selected("defaultzoom", z125);
        changed(1, &mut d, "defaultzoom");
        assert_eq!(settings::get().document_zoom, 125.0);
        // Another change in the panel leaves it alone.
        changed(1, &mut d, "hidden");
        assert_eq!(settings::get().document_zoom, 125.0);
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
