//! The original's dialogs, built on the dialog toolkit. Layouts follow the
//! original's own.
//!
//! Each dialog remembers the formatting it started from; OK and Apply
//! change only what the user changed, so a selection with mixed formats
//! keeps its mix in every property the dialog was not asked to touch.

use crate::border_dialog::{self, BorderState};
use crate::canvas::{Canvas, Label, Rect};
use crate::dialog::{Dialog, units};
use crate::docview::DocView;
use crate::page_dialogs;
use quill_core::props::{
    CharProps, LineSpacing, ParaProps, Rgb, Script, TabAlign, TabLeader, TabStop, Twips, Underline,
    WORD_COLORS,
};

/// Which dialog is open, and what it started from.
#[derive(Clone, Debug)]
pub enum Which {
    /// The color palette, opened from a Color popup's More Colors….
    Palette(crate::palette::PaletteState),
    /// Its editor, for one color.
    EditColor(crate::palette::EditState),
    /// Paste Special, and the forms it offers.
    PasteSpecial(Vec<crate::paste_special::Form>),
    Character(CharProps),
    Paragraph(ParaProps),
    /// Paragraph formatting staged inside the Style dialog.
    StyleParagraph(ParaProps),
    /// Manual character defaults staged inside the Style dialog.
    StyleCharacter(CharProps),
    /// Edit Character Style for style `id`, or a new style when None.
    CharacterStyleEdit(Option<u16>),
    SummaryInfo,
    /// The tabs as they were, the dialog's working copy (shown on the
    /// ruler while it is open), and the tab being edited.
    Tabs {
        base: Vec<TabStop>,
        tabs: Vec<TabStop>,
        selected: Option<Twips>,
    },
    Border(BorderState),
    InsertTable(crate::table_dialogs::InsertState),
    TableCells,
    TableLayout,
    PageSetup,
    Print,
    Document,
    Section,
    Style(crate::style_dialog::StyleState),
    CharacterStyles,
    /// Redefine or reapply a style chosen for a reformatted paragraph.
    Reapply(quill_core::props::StyleId),
    /// The panel showing, and the fonts for the Default Font panel.
    Preferences {
        panel: usize,
        fonts: Vec<String>,
        original: Box<crate::settings::Settings>,
        draft: Box<crate::settings::Settings>,
    },
    Find(crate::find_dialog::FindState),
    /// A format dialog opened from Find or Replace: for the Replace With
    /// side or not, and Character (0), Paragraph (1) or Style (2).
    FindFormat {
        replace: bool,
        kind: u8,
    },
    GoTo,
    Glossary(crate::glossary::GlossaryState),
    /// The counts and which of them are shown.
    WordCount(crate::tool_dialogs::Counts, [bool; 4]),
    ChangeCase,
    ReviewChanges,
    Footnote,
    Hyperlink(crate::link_dialog::LinkState),
    /// The picture's size and crop.
    Picture(crate::picture_dialog::PictureState),
    /// A page of Replace Missing Fonts.
    ReplaceFonts(crate::replace_fonts::ReplaceState),
    Renumber,
    /// The clipboard's text.
    Clipboard(String),
    Alert(Alert),
    Frame,
    DropCap,
    Toc,
    Index,
    LineNumbers,
    Spelling(crate::lang_dialogs::SpellState),
    PrintMerge,
    PrintMergeAsk(
        Box<crate::merge_dialogs::Session>,
        quill_core::merge::AskRequest,
    ),
    MergeHelper(Vec<String>),
    Commands(crate::commands_dialog::CommandsState),
    Customize(crate::customize::CustomizeState),
    Hyphenation(crate::lang_dialogs::HyphState),
    Thesaurus(crate::lang_dialogs::ThesState),
}

/// What an alert is about.
#[derive(Clone, Debug)]
pub enum Alert {
    /// Just a message with OK.
    Info,
    /// Find reached the end: OK goes on from the other end.
    ContinueSearch(Box<crate::find_dialog::Search>),
    /// Delete a color text uses: OK makes that text black, Choose Color…
    /// asks for another.
    DeleteColor(quill_core::props::Rgb),
}

/// Use As Default in the Document or Section dialog: make the dialog's
/// settings the defaults for new documents, and apply them here too.
/// False if a field is bad.
pub fn use_as_default(which: &mut Which, d: &mut Dialog, view: &mut DocView) -> bool {
    match which {
        Which::Document => {
            let Some(p) = page_dialogs::document_defaults(d, view) else {
                return false;
            };
            crate::settings::update(|s| s.document = p);
        }
        Which::Section => {
            let Some(p) = page_dialogs::section_defaults(d, view) else {
                return false;
            };
            crate::settings::update(|s| s.section = p);
        }
        _ => return false,
    }
    apply(which, d, view)
}

/// Draw a dialog's custom items.
pub fn draw_custom(which: &Which, id: &str, r: Rect, c: &mut Canvas, label: &mut Label) {
    if id == "alert_icon" {
        draw_note_icon(r, c);
        return;
    }
    if let Which::Commands(st) = which {
        crate::commands_dialog::draw(st, id, r, c, label);
    }
    if let Which::Palette(st) = which {
        crate::palette::draw(st, id, r, c);
    }
}

/// The note icon of an alert: a page with lines of text, as the original's.
fn draw_note_icon(r: Rect, c: &mut Canvas) {
    use crate::canvas::color;
    let page = Rect::new(r.x + 4.0, r.y, 24.0, 30.0);
    c.fill(page, color::WHITE);
    c.frame(page, color::BLACK);
    for k in 0..6 {
        let y = page.y + 6.0 + 4.0 * k as f32;
        c.hline(page.x + 4.0, page.right() - 4.0, y, color::GRAY_DARK);
    }
}

/// A checkbox, radio button, popup or field changed.
pub fn changed(which: &mut Which, d: &mut Dialog, view: &DocView, id: &str) {
    match which {
        Which::CharacterStyles if id == "list" => crate::character_style_dialog::changed(d, view),
        Which::Style(st) => crate::style_dialog::changed(st, d, view, id),
        Which::Character(_) => character_changed(d, id),
        Which::Border(st) if id == "applyto" => border_dialog::target_changed(st, d, view),
        Which::Border(st) => border_dialog::changed(st, d, id),
        Which::Section if id == "margin_numbers" => page_dialogs::margin_numbers_changed(d),
        Which::Toc | Which::Index => crate::contents_dialogs::level_changed(d, id),
        Which::Print => page_dialogs::print_changed(d, id),
        Which::InsertTable(_) => crate::table_dialogs::insert_changed(view, d, id),
        Which::TableLayout => crate::table_dialogs::table_layout_changed(d),
        Which::Preferences {
            panel,
            fonts,
            draft,
            ..
        } => {
            if id == "panel" {
                crate::prefs_dialog::changed_in(*panel, d, id, draft);
                let selected = d.selected("panel");
                if selected != *panel {
                    *panel = selected;
                    let origin = d.origin;
                    *d = crate::prefs_dialog::preferences_with(selected, fonts, draft);
                    d.origin = origin;
                }
            } else {
                crate::prefs_dialog::changed_in(*panel, d, id, draft);
            }
        }
        _ => {}
    }
}

const TAB_TYPES: [(&str, &str, TabAlign); 5] = [
    ("t_left", "Left", TabAlign::Left),
    ("t_center", "Center", TabAlign::Center),
    ("t_right", "Right", TabAlign::Right),
    ("t_decimal", "Decimal", TabAlign::Decimal),
    ("t_bar", "Bar", TabAlign::Bar),
];

const LEADERS: [(&str, &str, TabLeader); 4] = [
    ("l_none", "None", TabLeader::None),
    ("l_dots", ".....", TabLeader::Dots),
    ("l_dashes", "-----", TabLeader::Dashes),
    ("l_line", "_____", TabLeader::Underline),
];

/// The Tabs dialog, with `selected` (a tab position) chosen if given.
pub fn tabs(view: &DocView, selected: Option<Twips>) -> (Dialog, Which) {
    let tabs = view.current_pap().tabs.clone();
    let chosen = selected.and_then(|p| tabs.iter().find(|t| t.pos == p).copied());
    let mut d = Dialog::new("Tabs", 312.0, 128.0);
    d.group("Type", 10.0, 4.0, 88.0, 116.0);
    for (k, (id, label, align)) in TAB_TYPES.into_iter().enumerate() {
        let on = chosen.map_or(align == TabAlign::Left, |t| t.align == align);
        d.radio(id, label, 16.0, 22.0 + 18.0 * k as f32, 1, on);
    }
    d.group("Leader", 106.0, 4.0, 76.0, 98.0);
    for (k, (id, label, leader)) in LEADERS.into_iter().enumerate() {
        let on = chosen.map_or(leader == TabLeader::None, |t| t.leader == leader);
        d.radio(id, label, 112.0, 22.0 + 18.0 * k as f32, 2, on);
    }
    d.button("set", "Set", 192.0, 10.0, 52.0, false);
    d.button("ok", "OK", 252.0, 10.0, 52.0, true);
    d.button("clear", "Clear", 192.0, 40.0, 52.0, false);
    d.button("cancel", "Cancel", 252.0, 40.0, 52.0, false);
    d.button("clearall", "Clear All", 192.0, 70.0, 80.0, false);
    d.text("Position:", 192.0, 104.0);
    let pos = chosen.map(|t| units::length(t.pos)).unwrap_or_default();
    d.field("pos", &pos, 258.0, 102.0, 46.0);
    d.focus("pos");
    let base = tabs.clone();
    (
        d,
        Which::Tabs {
            base,
            tabs,
            selected: chosen.map(|t| t.pos),
        },
    )
}

/// The tab the dialog's Type, Leader and Position describe. `selected` is
/// the chosen tab's position, kept exactly while its field is unedited.
fn tab_from(d: &Dialog, selected: Option<Twips>) -> Option<TabStop> {
    let pos = tab_pos(d, selected).filter(|_| !d.field_text("pos").trim().is_empty())?;
    let align = TAB_TYPES
        .iter()
        .find(|(id, _, _)| d.is_on(id))
        .map_or(TabAlign::Left, |t| t.2);
    let leader = LEADERS
        .iter()
        .find(|(id, _, _)| d.is_on(id))
        .map_or(TabLeader::None, |l| l.2);
    (pos >= 0).then_some(TabStop { pos, align, leader })
}

/// Show a tab's settings in the dialog, as clicking it on the ruler does.
pub fn choose_tab(d: &mut Dialog, t: &TabStop) {
    for (id, _, align) in TAB_TYPES {
        d.set_on(id, align == t.align);
    }
    for (id, _, leader) in LEADERS {
        d.set_on(id, leader == t.leader);
    }
    d.set_field("pos", &units::length(t.pos));
}

/// The Position field, as the chosen tab's exact position while it is
/// unedited.
fn tab_pos(d: &Dialog, selected: Option<Twips>) -> Option<Twips> {
    let text = d.field_text("pos");
    match selected {
        Some(old) => units::parse_length_from(&text, old),
        None => units::parse_length(&text),
    }
}

/// Set, Clear and Clear All in the Tabs dialog.
pub fn tabs_button(which: &mut Which, d: &mut Dialog, id: &str) {
    let Which::Tabs { tabs, selected, .. } = which else {
        return;
    };
    match id {
        "set" => {
            if let Some(t) = tab_from(d, *selected) {
                // Setting with the chosen tab moved replaces it.
                if let Some(old) = *selected
                    && old != t.pos
                {
                    tabs.retain(|x| x.pos != old);
                }
                tabs.retain(|x| x.pos != t.pos);
                let at = tabs.partition_point(|x| x.pos < t.pos);
                tabs.insert(at, t);
                *selected = Some(t.pos);
            }
        }
        "clear" => {
            let pos = tab_pos(d, *selected).or(*selected);
            if let Some(p) = pos {
                tabs.retain(|x| x.pos != p);
            }
            *selected = None;
            d.set_field("pos", "");
        }
        "clearall" => {
            tabs.clear();
            *selected = None;
            d.set_field("pos", "");
        }
        _ => {}
    }
}

const UNDERLINES: [&str; 5] = ["None", "Single", "Word", "Double", "Dotted"];

fn underline_index(u: Underline) -> usize {
    match u {
        Underline::None => 0,
        Underline::Single => 1,
        Underline::Word => 2,
        Underline::Double => 3,
        Underline::Dotted => 4,
    }
}

pub(crate) const UNDERLINE_KINDS: [Underline; 5] = [
    Underline::None,
    Underline::Single,
    Underline::Word,
    Underline::Double,
    Underline::Dotted,
];

/// Whether the Character dialog's color is a button that pops out the
/// color palette (the Mac's native dialogs have the pop-out), instead of
/// a popup whose More Colors… opens the palette in a window.
pub const COLOR_BUTTON: bool = cfg!(target_os = "macos");

/// Format > Character (⌘D).
pub fn character(
    view: &DocView,
    font_families: &[quill_layout::FontFamilyInfo],
) -> (Dialog, Which) {
    build_character(view, font_families, COLOR_BUTTON)
}

/// The Character dialog with its color button, whatever the platform.
#[cfg(test)]
pub fn build_character_with_button(view: &DocView) -> (Dialog, Which) {
    build_character(view, &[], true)
}

/// The Character dialog as Find uses it: its color a popup that can say
/// any color.
pub fn character_for_find(
    view: &DocView,
    font_families: &[quill_layout::FontFamilyInfo],
) -> (Dialog, Which) {
    build_character(view, font_families, false)
}

fn build_character(
    view: &DocView,
    font_families: &[quill_layout::FontFamilyInfo],
    color_button: bool,
) -> (Dialog, Which) {
    let c = view.current_chp();
    let mut d = Dialog::new("Character", 384.0, 262.0);
    let current = view.doc.font_name(c.font).to_string();
    let mut families = font_families.to_vec();
    if !families
        .iter()
        .any(|f| f.name.eq_ignore_ascii_case(&current))
    {
        families.insert(
            0,
            quill_layout::FontFamilyInfo {
                name: current.clone(),
                draws_name: false,
                variants: vec![quill_layout::FontVariant {
                    bold: false,
                    italic: false,
                }],
            },
        );
    }
    let sel = families
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(&current))
        .unwrap_or(0);
    d.text("Font:", 12.0, 8.0);
    d.font_popup("font", &families, sel, 12.0, 26.0, 200.0);
    d.text("Size:", 228.0, 8.0);
    d.field("size", &crate::bars::format_size(c.size), 228.0, 26.0, 52.0);
    d.button("ok", "OK", 304.0, 8.0, 68.0, true);
    d.button("cancel", "Cancel", 304.0, 38.0, 68.0, false);
    d.button("apply", "Apply", 304.0, 66.0, 68.0, false);
    d.text("Underline:", 12.0, 56.0);
    d.popup(
        "underline",
        &UNDERLINES,
        underline_index(c.underline),
        12.0,
        74.0,
        120.0,
    );
    d.text("Color:", 146.0, 56.0);
    if color_button {
        d.swatch_button("color", 146.0, 74.0, 44.0, 22.0, c.color.0);
        d.text_as("color_hex", &crate::palette::hex(c.color), 198.0, 78.0);
    } else {
        let (colors, color) = color_options(Some(c.color), false);
        d.popup_owned("color", colors, color, 146.0, 74.0, 134.0);
    }

    d.group("Style", 12.0, 104.0, 128.0, 152.0);
    let styles = [
        ("bold", "Bold", c.bold),
        ("italic", "Italic", c.italic),
        ("outline", "Outline", c.outline),
        ("shadow", "Shadow", c.shadow),
        ("strike", "Strikethru", c.strike),
        ("smallcaps", "Small Caps", c.small_caps),
        ("allcaps", "All Caps", c.all_caps),
        ("hidden", "Hidden", c.hidden),
    ];
    for (k, (id, label, on)) in styles.into_iter().enumerate() {
        d.check(id, label, 20.0, 122.0 + 16.0 * k as f32, on);
    }

    d.group("Position", 150.0, 104.0, 222.0, 74.0);
    d.radio(
        "pos_normal",
        "Normal",
        158.0,
        122.0,
        1,
        c.script == Script::None,
    );
    d.radio(
        "pos_super",
        "Superscript",
        158.0,
        140.0,
        1,
        c.script == Script::Super,
    );
    d.radio(
        "pos_sub",
        "Subscript",
        158.0,
        158.0,
        1,
        c.script == Script::Sub,
    );
    // A shift set by hand, apart from the superscript or subscript, which
    // the document's settings size: up, or down when negative.
    d.text("Raised By:", 290.0, 122.0);
    let by = if c.position == 0 {
        String::new()
    } else {
        units::points(c.position)
    };
    d.field("pos_by", &by, 296.0, 140.0, 66.0);

    d.group("Spacing", 150.0, 182.0, 222.0, 74.0);
    d.radio("sp_normal", "Normal", 158.0, 200.0, 2, c.spacing == 0);
    d.radio("sp_cond", "Condensed", 158.0, 218.0, 2, c.spacing < 0);
    d.radio("sp_exp", "Expanded", 158.0, 236.0, 2, c.spacing > 0);
    d.text("By:", 296.0, 200.0);
    let by = if c.spacing == 0 {
        String::new()
    } else {
        units::points(c.spacing.abs())
    };
    d.field("sp_by", &by, 296.0, 218.0, 66.0);
    d.focus("size");
    // Left over if a window closed under its dialog.
    crate::palette::drop_edits();
    (d, Which::Character(c))
}

/// Keep the "By:" fields in step with their radio buttons, filling in
/// The original's defaults (3 pt up, 2 pt down, 1.5 pt condensed, 3 pt expanded).
/// The last entry in the Color popup, which opens the color palette.
pub const OTHER_COLOR: &str = "More Colors…";

/// The Color popup's entries and which is selected: the original's eight colors,
/// `have` if it is none of those (as "#RRGGBB"), then Other…. With `any`,
/// a blank first entry means any color, for Find and Replace.
pub fn color_options(have: Option<Rgb>, any: bool) -> (Vec<String>, usize) {
    let mut names: Vec<String> = Vec::new();
    if any {
        names.push(String::new());
    }
    names.extend(WORD_COLORS.iter().map(|(n, _)| n.to_string()));
    let selected = match have {
        None => 0,
        Some(c) => match WORD_COLORS.iter().position(|(_, x)| *x == c) {
            Some(i) => i + usize::from(any),
            None => {
                names.push(format!("#{:06X}", c.0 & 0xFF_FFFF));
                names.len() - 1
            }
        },
    };
    names.push(OTHER_COLOR.to_string());
    (names, selected)
}

/// The color a Color popup entry names: one of the original's, or "#RRGGBB".
pub fn color_named(name: &str) -> Option<Rgb> {
    if let Some(hex) = name.strip_prefix('#') {
        return u32::from_str_radix(hex, 16).ok().map(Rgb);
    }
    WORD_COLORS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Whether the Color popup has been taken to Other…, which opens the
/// color palette.
pub fn wants_palette(d: &Dialog) -> bool {
    d.selected_text("color") == OTHER_COLOR
}

/// Put `color` in the Color popup (None for any color, in Find), as it
/// is after the palette has been used or canceled.
pub fn show_color(d: &mut Dialog, color: Option<Rgb>) {
    let any = d.options("color").first().is_some_and(|n| n.is_empty());
    let (names, selected) = color_options(color, any);
    d.set_options("color", names, selected);
}

pub fn character_changed(d: &mut Dialog, id: &str) {
    // The color popup's Other… is taken up by the window, which opens
    // the palette.
    if id == "color" {
        return;
    }
    let default = match id {
        "pos_normal" | "sp_normal" => Some(""),
        "pos_super" => Some("3 pt"),
        "pos_sub" => Some("2 pt"),
        "sp_cond" => Some("1.5 pt"),
        "sp_exp" => Some("3 pt"),
        _ => None,
    };
    if let Some(v) = default {
        let field = if id.starts_with("pos") {
            "pos_by"
        } else {
            "sp_by"
        };
        d.set_field(field, v);
    }
}

/// The character format the dialog describes, starting from `base`.
pub fn read_character(d: &Dialog, base: &CharProps, view: &mut DocView) -> Option<CharProps> {
    let mut c = *base;
    let font = d.selected_text("font");
    if !font.is_empty() {
        c.font = view.doc.font_id(&font);
    }
    let size: f32 = d
        .field_text("size")
        .trim()
        .trim_end_matches("pt")
        .trim()
        .parse()
        .ok()?;
    if !(1.0..=1638.0).contains(&size) {
        return None;
    }
    c.size = (size * 2.0).round() as u16;
    c.underline = UNDERLINE_KINDS[d.selected("underline").min(4)];
    // A color button says what it shows; with a popup, More Colors… with
    // nothing picked leaves the color as it was.
    c.color = match d.swatch_rgb("color") {
        Some(rgb) => Rgb(rgb),
        None => color_named(&d.selected_text("color")).unwrap_or(base.color),
    };
    c.bold = d.is_on("bold");
    c.italic = d.is_on("italic");
    c.outline = d.is_on("outline");
    c.shadow = d.is_on("shadow");
    c.strike = d.is_on("strike");
    c.small_caps = d.is_on("smallcaps");
    c.all_caps = d.is_on("allcaps");
    c.hidden = d.is_on("hidden");
    let by = |id| units::parse(&d.field_text(id), "pt").map(|t| t.abs());
    c.script = match d.radio_on(1) {
        "pos_super" => Script::Super,
        "pos_sub" => Script::Sub,
        _ => Script::None,
    };
    let raised = d.field_text("pos_by");
    c.position = if raised.trim().is_empty() {
        0
    } else {
        units::parse(&raised, "pt")?
    };
    c.spacing = match d.radio_on(2) {
        "sp_cond" => -by("sp_by")?,
        "sp_exp" => by("sp_by")?,
        _ => 0,
    };
    Some(c)
}

/// Format > Paragraph (⌘M).
/// The Line popup in Paragraph. Multiple is a share of the auto height
/// ("1.5 li"); the original had no such setting, but documents from elsewhere
/// use it.
pub const LINE_RULES: [&str; 4] = ["Auto", "At Least", "Exactly", "Multiple"];

/// A Multiple line spacing (240 = single) as the field shows it.
pub fn lines(m: u16) -> String {
    let v = format!("{:.2}", m as f32 / 240.0);
    format!("{} li", v.trim_end_matches('0').trim_end_matches('.'))
}

/// Read the Line popup choice `rule` (an index into `LINE_RULES`) and
/// its "line_by" field. None means the field could not be read.
pub fn read_line_rule(d: &Dialog, rule: usize) -> Option<Option<LineSpacing>> {
    let text = d.field_text("line_by");
    if rule == 3 {
        let m = units::parse(&text, "li")?.abs();
        return Some(Some(match m {
            0 => LineSpacing::Auto,
            m => LineSpacing::Multiple(m.clamp(60, 2400) as u16),
        }));
    }
    let by = if text.trim().is_empty() {
        0
    } else {
        units::parse(&text, "pt")?.abs()
    };
    Some(Some(match rule {
        0 => LineSpacing::Auto,
        1 if by == 0 => LineSpacing::Auto,
        1 => LineSpacing::AtLeast(by),
        _ => LineSpacing::Exactly(by.max(20)),
    }))
}

pub fn paragraph(view: &DocView) -> (Dialog, Which) {
    let p = view.current_pap().clone();
    (paragraph_properties(&p), Which::Paragraph(p))
}

pub fn paragraph_properties(p: &ParaProps) -> Dialog {
    let mut d = Dialog::new("Paragraph", 438.0, 200.0);
    d.group("Spacing", 10.0, 4.0, 334.0, 70.0);
    d.text("Before:", 18.0, 24.0);
    d.field("before", &units::length(p.before), 84.0, 22.0, 66.0);
    d.text("After:", 18.0, 48.0);
    d.field("after", &units::length(p.after), 84.0, 46.0, 66.0);
    d.text("Line Spacing:", 164.0, 24.0);
    let (rule, value) = match p.line {
        LineSpacing::Auto => (0, String::new()),
        LineSpacing::AtLeast(t) => (1, units::points(t)),
        LineSpacing::Exactly(t) => (2, units::points(t)),
        LineSpacing::Multiple(m) => (3, lines(m)),
    };
    d.popup("line", &LINE_RULES, rule, 164.0, 46.0, 72.0);
    d.field("line_by", &value, 240.0, 46.0, 54.0);

    d.group("Indentation", 10.0, 80.0, 140.0, 102.0);
    d.text("Left:", 18.0, 100.0);
    d.field("left", &units::length(p.left), 70.0, 98.0, 70.0);
    d.text("Right:", 18.0, 126.0);
    d.field("right", &units::length(p.right), 70.0, 124.0, 70.0);
    d.text("First:", 18.0, 152.0);
    d.field("first", &units::length(p.first), 70.0, 150.0, 70.0);

    d.group("Pagination", 158.0, 80.0, 186.0, 76.0);
    d.check(
        "pagebreak",
        "Page Break Before",
        166.0,
        98.0,
        p.page_break_before,
    );
    d.check("keepnext", "Keep With Next", 166.0, 116.0, p.keep_with_next);
    d.check(
        "keeplines",
        "Keep Lines Together",
        166.0,
        134.0,
        p.keep_together,
    );
    d.check(
        "noline",
        "Suppress Line Numbers",
        166.0,
        164.0,
        p.suppress_line_numbers,
    );
    d.disable("noline");

    d.button("ok", "OK", 358.0, 8.0, 68.0, true);
    d.button("cancel", "Cancel", 358.0, 38.0, 68.0, false);
    d.button("apply", "Apply", 358.0, 66.0, 68.0, false);
    d.button("tabs", "Tabs…", 358.0, 108.0, 68.0, false);
    d.button("border", "Border…", 358.0, 136.0, 68.0, false);
    d.button("frame", "Frame…", 358.0, 164.0, 68.0, false);
    d.focus("left");
    d
}

pub fn read_paragraph(d: &Dialog, base: &ParaProps) -> Option<ParaProps> {
    let mut p = base.clone();
    let len = |id, old| units::parse_length_from(&d.field_text(id), old);
    p.before = len("before", base.before)?.max(0);
    p.after = len("after", base.after)?.max(0);
    p.line = read_line_rule(d, d.selected("line"))?.unwrap_or_default();
    p.left = len("left", base.left)?;
    p.right = len("right", base.right)?;
    p.first = len("first", base.first)?;
    p.page_break_before = d.is_on("pagebreak");
    p.keep_with_next = d.is_on("keepnext");
    p.keep_together = d.is_on("keeplines");
    Some(p)
}

/// File > Summary Info.
pub fn summary_info(view: &DocView) -> (Dialog, Which) {
    let s = &view.doc.summary;
    let mut d = Dialog::new("Summary Info", 380.0, 150.0);
    let rows = [
        ("title", "Title:", &s.title),
        ("subject", "Subject:", &s.subject),
        ("author", "Author:", &s.author),
        ("version", "Version:", &s.version),
        ("keywords", "Keywords:", &s.keywords),
    ];
    for (k, (id, label, value)) in rows.into_iter().enumerate() {
        let y = 10.0 + 27.0 * k as f32;
        d.text(label, 10.0, y + 2.0);
        d.field(id, value, 90.0, y, 200.0);
    }
    d.button("ok", "OK", 302.0, 10.0, 68.0, true);
    d.button("cancel", "Cancel", 302.0, 40.0, 68.0, false);
    d.focus("title");
    (d, Which::SummaryInfo)
}

/// Apply a dialog's settings to the document. Returns false (and changes
/// nothing) when a field holds something that is not a valid measurement,
/// so the dialog can stay open, as the original's does.
pub fn apply(which: &mut Which, d: &mut Dialog, view: &mut DocView) -> bool {
    match which {
        Which::CharacterStyles => true,
        Which::Style(st) => crate::style_dialog::apply(st, d, view),
        Which::Reapply(id) => crate::style_dialog::reapply_ok(*id, d, view),
        Which::ReplaceFonts(st) => crate::replace_fonts::apply(st, d, view),
        Which::Character(base) => {
            let Some(target) = read_character(d, base, view) else {
                return false;
            };
            let base = *base;
            crate::palette::drop_edits();
            view.format_chars("Formatting", move |c| {
                macro_rules! changed {
                    ($($f:ident),*) => { $( if target.$f != base.$f { c.$f = target.$f; } )* };
                }
                changed!(
                    font, size, bold, italic, underline, strike, outline, shadow, small_caps,
                    all_caps, hidden, color, position, spacing, script
                );
            });
            true
        }
        Which::Paragraph(base) => {
            let Some(target) = read_paragraph(d, base) else {
                return false;
            };
            let base = base.clone();
            view.format_paras("Paragraph Formatting", move |p| {
                macro_rules! changed {
                    ($($f:ident),*) => { $( if target.$f != base.$f { p.$f = target.$f.clone(); } )* };
                }
                changed!(before, after, line, left, right, first, page_break_before, keep_with_next, keep_together);
            });
            true
        }
        Which::Tabs {
            base,
            tabs,
            selected,
        } => {
            // OK sets what the dialog shows, as if Set had been pressed.
            let mut tabs = tabs.clone();
            if let Some(t) = tab_from(d, *selected) {
                if let Some(old) = *selected {
                    tabs.retain(|x| x.pos != old);
                }
                tabs.retain(|x| x.pos != t.pos);
                let at = tabs.partition_point(|x| x.pos < t.pos);
                tabs.insert(at, t);
            }
            if tabs != *base {
                view.format_paras("Paragraph Formatting", move |p| p.tabs = tabs.clone());
            }
            true
        }
        Which::Border(st) => border_dialog::apply(st, d, view),
        Which::Document => page_dialogs::apply_document(d, view),
        Which::PageSetup => page_dialogs::apply_page_setup(d, view),
        Which::InsertTable(st) => crate::table_dialogs::insert_apply(st, d, view),
        Which::TableCells => crate::table_dialogs::table_cells_apply(d, view),
        Which::TableLayout => true,
        // The window turns the dialog into a print job itself.
        Which::Print => true,
        // Preferences take effect as they change.
        Which::Preferences { .. } => true,
        // The milestone 8 dialogs are handled by the window.
        Which::Find(_)
        | Which::StyleCharacter(_)
        | Which::StyleParagraph(_)
        | Which::CharacterStyleEdit(_)
        | Which::FindFormat { .. }
        | Which::GoTo
        | Which::Glossary(_)
        | Which::WordCount(..)
        | Which::ChangeCase
        | Which::ReviewChanges
        | Which::Footnote
        | Which::Hyperlink(_)
        | Which::Picture(_)
        | Which::Spelling(_)
        | Which::PrintMerge
        | Which::PrintMergeAsk(..)
        | Which::Commands(_)
        | Which::Customize(_)
        | Which::MergeHelper(_)
        | Which::Hyphenation(_)
        | Which::Thesaurus(_)
        | Which::Renumber
        | Which::Clipboard(_)
        | Which::PasteSpecial(_)
        | Which::Alert(_)
        | Which::Palette(_)
        | Which::EditColor(_)
        | Which::Frame
        | Which::DropCap => true,
        Which::Section => page_dialogs::apply_section(d, view),
        Which::LineNumbers => crate::contents_dialogs::apply_line_numbers(d, view),
        Which::Toc => match crate::contents_dialogs::read_toc(d) {
            Some(o) => {
                view.insert_toc(o);
                true
            }
            None => false,
        },
        Which::Index => match crate::contents_dialogs::read_index(d) {
            Some(o) => {
                view.insert_index(o);
                true
            }
            None => false,
        },
        Which::SummaryInfo => {
            let new = quill_core::SummaryInfo {
                title: d.field_text("title"),
                subject: d.field_text("subject"),
                author: d.field_text("author"),
                version: d.field_text("version"),
                keywords: d.field_text("keywords"),
            };
            view.set_summary(new);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{Document, Position, Selection};

    fn view(text: &str) -> DocView {
        let mut v = DocView::new(Document::new(), "t".into());
        v.type_text(text);
        v.set_selection(Selection::new(
            Position::new(0, 0),
            Position::new(0, text.len()),
        ));
        v
    }

    #[test]
    fn ok_without_changes_keeps_spacing_and_indents_exact() {
        // None of these shows exactly in inches: 12 pt is "0.17 in", which
        // would read back as 12.25 pt, and 0.5 cm is "0.2 in".
        let mut v = view("Some text");
        let pap = &mut v.doc.main.paras[0].pap;
        (pap.before, pap.after, pap.left, pap.right, pap.first) = (240, 120, 284, 567, -170);
        let before = v.doc.main.paras[0].pap.clone();
        let (mut d, mut which) = paragraph(&v);
        assert_eq!(d.field_text("before"), "0.17 in");
        assert!(apply(&mut which, &mut d, &mut v));
        assert_eq!(v.doc.main.paras[0].pap, before);
        // A field that is edited is read as typed.
        let (mut d, mut which) = paragraph(&v);
        d.set_field("before", "0.25 in");
        assert!(apply(&mut which, &mut d, &mut v));
        assert_eq!(v.doc.main.paras[0].pap.before, 360);
        assert_eq!(v.doc.main.paras[0].pap.after, 120);
    }

    #[test]
    fn ok_without_changes_keeps_line_spacing() {
        for line in [
            LineSpacing::Auto,
            LineSpacing::AtLeast(300),
            LineSpacing::Exactly(280),
            LineSpacing::Multiple(360),
            LineSpacing::Multiple(276),
        ] {
            let mut v = view("Some text");
            v.doc.main.paras[0].pap.line = line;
            let (mut d, mut which) = paragraph(&v);
            assert!(apply(&mut which, &mut d, &mut v));
            assert_eq!(v.doc.main.paras[0].pap.line, line);
        }
        assert_eq!(lines(360), "1.5 li");
        assert_eq!(lines(480), "2 li");
    }

    #[test]
    fn ok_without_changes_keeps_a_color_not_in_the_menu() {
        let mut v = view("Some text");
        v.format_chars("Formatting", |c| c.color = Rgb(0x123456));
        // With the color button, it shows what the color is.
        let (mut d, mut which) = build_character(&v, &[], true);
        assert_eq!(d.swatch_rgb("color"), Some(0x123456));
        assert_eq!(d.item_label("color_hex"), "#123456");
        assert!(apply(&mut which, &mut d, &mut v));
        assert_eq!(v.doc.main.paras[0].chp_of_char(0).color, Rgb(0x123456));
        // A color chosen is read from the button.
        d.set_swatch("color", 0xFF8000);
        assert!(apply(&mut which, &mut d, &mut v));
        assert_eq!(v.doc.main.paras[0].chp_of_char(0).color, Rgb(0xFF8000));
        // With a popup (Find, and platforms without the pop-out) the color
        // not in the menu is its own entry.
        let (mut d, mut which) = build_character(&v, &[], false);
        assert_eq!(d.selected_text("color"), "#FF8000");
        assert!(apply(&mut which, &mut d, &mut v));
        assert_eq!(v.doc.main.paras[0].chp_of_char(0).color, Rgb(0xFF8000));
        // More Colors… asks for the palette; a Cancel there puts the
        // color it had back, and a choice puts that in.
        let n = d.options("color").len();
        d.set_selected("color", n - 1);
        assert!(wants_palette(&d));
        show_color(&mut d, Some(Rgb(0x123456)));
        assert_eq!(d.selected_text("color"), "#123456");
        assert!(!wants_palette(&d));
        show_color(&mut d, Some(Rgb(0xABCDEF)));
        assert_eq!(d.selected_text("color"), "#ABCDEF");
        assert_eq!(color_named(&d.selected_text("color")), Some(Rgb(0xABCDEF)));
        assert_eq!(color_named("Red"), Some(Rgb(0xFF0000)));
        assert_eq!(color_named("#00FF7F"), Some(Rgb(0x00FF7F)));
    }
}
