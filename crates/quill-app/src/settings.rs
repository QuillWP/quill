//! Quill's settings: what the original kept in its settings file. Preferences
//! changes them, and Use As Default in the Document and Section dialogs
//! sets the defaults for new documents.
//!
//! They live in one global so anything can read them (the units dialogs
//! show, smart quotes while typing), and are saved as plain "key = value"
//! lines in the platform's usual place for settings.

use quill_core::{
    Document, DocumentProps, PageNumberFormat, SectionProps, SectionStart, Twips,
    document::FootnotePosition,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

// Shared defaults also compile in the parser-only fuzzing library.
/// Neutral light gray behind the pages, configurable in settings.
pub const DEFAULT_PAGE_SURROUND: u32 = 0xE4E4E4;
/// Slightly darker page outline, configurable in settings.
pub const DEFAULT_PAGE_BORDER: u32 = 0xBBBBBB;

/// The view documents open in (Preferences > View).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DefaultView {
    #[default]
    Normal,
    Outline,
    PageLayout,
}

impl DefaultView {
    pub const ALL: [DefaultView; 3] = [
        DefaultView::Normal,
        DefaultView::Outline,
        DefaultView::PageLayout,
    ];

    /// The name Preferences shows.
    pub fn label(self) -> &'static str {
        match self {
            DefaultView::Normal => "Normal",
            DefaultView::Outline => "Outline",
            DefaultView::PageLayout => "Page Layout",
        }
    }

    /// The name the settings file uses.
    fn key(self) -> &'static str {
        match self {
            DefaultView::Normal => "normal",
            DefaultView::Outline => "outline",
            DefaultView::PageLayout => "page_layout",
        }
    }
}

/// Light or dark windows (Preferences > View). Auto follows the system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::Auto, Appearance::Light, Appearance::Dark];

    /// The name Preferences shows.
    pub fn label(self) -> &'static str {
        match self {
            Appearance::Auto => "Auto",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }

    /// The name the settings file uses.
    fn key(self) -> &'static str {
        match self {
            Appearance::Auto => "auto",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    /// Whether windows draw dark, given whether the system is dark now.
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::Auto => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

/// Whether Quill opens again the documents that were open when it last quit
/// (Preferences > Open And Save).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reopen {
    Always,
    Never,
    #[default]
    Prompt,
}

impl Reopen {
    pub const ALL: [Reopen; 3] = [Reopen::Always, Reopen::Never, Reopen::Prompt];

    /// The name Preferences shows.
    pub fn label(self) -> &'static str {
        match self {
            Reopen::Always => "Always",
            Reopen::Never => "Never",
            Reopen::Prompt => "After a crash",
        }
    }

    /// The name the settings file uses.
    fn key(self) -> &'static str {
        match self {
            Reopen::Always => "always",
            Reopen::Never => "never",
            Reopen::Prompt => "prompt",
        }
    }
}

/// The unit lengths are shown in (Preferences > General).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Inch,
    Cm,
    Mm,
    Point,
    Pica,
}

impl Unit {
    pub const ALL: [Unit; 5] = [Unit::Inch, Unit::Cm, Unit::Mm, Unit::Point, Unit::Pica];

    /// The name Preferences shows.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Inch => "Inch",
            Unit::Cm => "Cm",
            Unit::Mm => "Mm",
            Unit::Point => "Points",
            Unit::Pica => "Picas",
        }
    }

    /// The abbreviation measurements are written with.
    pub fn abbrev(self) -> &'static str {
        match self {
            Unit::Inch => "in",
            Unit::Cm => "cm",
            Unit::Mm => "mm",
            Unit::Point => "pt",
            Unit::Pica => "pi",
        }
    }
}

/// Quill remembers this many recent documents; the menu shows `recent_count`.
pub const MAX_RECENT_DOCUMENTS: usize = 100;
/// How many colors Preferences saves for every document.
pub const MAX_NAMED_COLORS: usize = 8;

/// How many documents' kept fonts are remembered, newest last.
pub const MAX_ACCEPTED_DOCUMENTS: usize = 200;

/// Maximum sources in Preferences > Fonts > Additional Font Sources.
pub const MAX_FONT_FOLDERS: usize = 10;
pub const DEFAULT_FONT_LEVELS: u8 = 4;
pub const MAX_FONT_LEVELS: u8 = 7;
pub const DEFAULT_AUTOSAVE_SECS: u32 = 30;

/// One extra font folder, which can be switched on and off.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFolder {
    pub path: PathBuf,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    // General
    pub name: String,
    pub initials: String,
    /// Custom paper size, or 0 for none.
    pub unit: Unit,
    pub smart_quotes: bool,
    /// Make web and mail addresses links as they are typed.
    pub auto_links: bool,
    /// Typing "* " or "- " at the start of a paragraph starts a list.
    pub auto_lists: bool,
    // Spelling
    pub always_suggest: bool,
    pub spelling_as_you_type: bool,
    pub custom_dictionary: Option<PathBuf>,
    pub words_to_system: bool,
    pub ignore_uppercase: bool,
    pub ignore_numbers: bool,
    pub formatted_clipboard: bool,
    // View
    pub show_hidden: bool,
    pub table_gridlines: bool,
    /// How the gridlines look: their color, and dotted or solid.
    pub table_gridline_color: u32,
    pub table_gridline_dotted: bool,
    /// The borders a new table starts with, which it then keeps.
    pub table_border_color: u32,
    pub table_border_dotted: bool,
    /// The gray round the pages in Page Layout.
    pub page_layout_color_background: u32,
    /// The line round each page in Page Layout.
    pub page_layout_color_page_border: u32,
    pub text_boundaries: bool,
    pub default_view: DefaultView,
    /// Light, dark, or as the system is.
    pub appearance: Appearance,
    pub open_with_ruler: bool,
    pub open_with_formatting_toolbar: bool,
    pub open_with_paragraph_toolbar: bool,
    pub recent_files: bool,
    pub recent_count: usize,
    /// Persistent recent documents; macOS also updates NSDocumentController.
    pub recent_documents: Vec<PathBuf>,
    // Advanced
    /// Extra places to load fonts from without installing them.
    pub font_folders: Vec<FontFolder>,
    /// How many folder levels below each font folder Quill searches.
    pub font_folder_levels: u8,
    /// Seconds between recovery saves; 0 turns them off.
    pub autosave_secs: u32,
    // Open and Save
    pub interpret_rtf: bool,
    pub make_backup: bool,
    pub foreign_as_quill: bool,
    pub reopen: Reopen,
    /// The saved documents open now, or when Quill last quit. Kept up to
    /// date while Quill runs, so a crash leaves the list behind.
    pub last_session: Vec<PathBuf>,
    /// Whether Quill last ended through Quit, rather than crashing or being
    /// stopped. Set false while it runs. Prompt asks to reopen the last
    /// session's documents only when this is false.
    pub quit_cleanly: bool,
    // Default Font
    pub font: String,
    /// The Default Font's style: Bold and Italic choices from its menu.
    pub font_bold: bool,
    pub font_italic: bool,
    /// Half-points.
    pub size: u16,
    // Use As Default
    pub document: DocumentProps,
    pub section: SectionProps,
    /// Initial document-view scale as a percentage. This changes only the
    /// screen presentation, never document measurements or saved font sizes.
    pub document_zoom: f64,
    /// Where the last document window closed and how big it was, in
    /// points: left, top, width, height. New windows open there.
    pub window_frame: Option<[i32; 4]>,
    /// Interface size as a percentage of the platform's usual size, for
    /// trying out sizes by editing the settings file.
    pub interface_scale: u16,
    /// The top toolbar's button pitch in points.
    pub toolbar_spacing: u16,
    /// Extra points after each formatting toolbar and ruler control.
    pub formatting_toolbar_spacing: u16,
    /// Text size in points in the font, size and style boxes.
    pub toolbar_picker_font_size: u16,
    /// Blank points before the first button of the toolbars and the
    /// ruler's button row, and above and below their buttons.
    pub bar_padding_horiz: u16,
    pub bar_padding_vert: u16,
    /// Background of the toolbars and the ruler, as 0xRRGGBB.
    pub bar_background: u32,
    /// The flat lines between the bars and along their edges, and the
    /// outlines of the bars' white boxes (font, size and style).
    pub line_color: u32,
    /// The ruler's measuring scale: its paper, and its markings and numbers.
    pub ruler_background: u32,
    pub ruler_foreground: u32,
    // Toolbar
    pub view_toolbar: bool,
    /// The tab-type buttons at the end of the paragraph toolbar.
    pub paragraph_toolbar_tabs: bool,
    /// 0 top (horizontal), 1 left, 2 right (vertical).
    pub toolbar_position: u8,
    /// The Style Sheets palette: shown, docked at the right (not floating),
    /// and its tab (0 All, 1 Paragraph, 2 Character).
    pub show_styles_palette: bool,
    pub styles_palette_docked: bool,
    pub styles_palette_tab: u8,
    /// Customized buttons: position, picture and command name.
    pub toolbar_buttons: Vec<(usize, usize, String)>,
    /// Colors saved for every document, each with its name.
    pub named_colors: Vec<(u32, String)>,
    /// Each document's missing fonts the person chose to keep: opening
    /// that document no longer warns about them. Kept here by path because
    /// .docx and RTF have no place for it, and choosing should not make the
    /// document need saving.
    pub accepted_fonts: Vec<(PathBuf, Vec<String>)>,
    /// View > Highlight Missing Fonts.
    pub highlight_missing_fonts: bool,
    /// Say where to deal with missing fonts after going on without them.
    pub missing_font_reminder: bool,
    /// Markdown source and display fonts, sizes in half-points.
    pub markdown_source_font: String,
    pub markdown_source_size: u16,
    pub markdown_display_font: String,
    pub markdown_display_size: u16,
}

/// A color written as six hex digits, "#DDDDDD" or "DDDDDD", as 0xRRGGBB.
fn html_color(v: &str) -> Option<u32> {
    let hex = v.trim().trim_start_matches('#');
    quill_core::props::Rgb::from_hex(hex).map(|c| c.0)
}

/// One `key = value` line of the settings file.
fn put(out: &mut String, key: &str, value: String) {
    out.push_str(key);
    out.push_str(" = ");
    out.push_str(&value);
    out.push('\n');
}

/// A blank line, then `# Title` and an optional `## description`.
fn section(out: &mut String, title: &str, about: Option<&str>) {
    out.push_str("\n# ");
    out.push_str(title);
    out.push('\n');
    if let Some(about) = about {
        out.push_str("## ");
        out.push_str(about);
        out.push('\n');
    }
}

/// A path on one line: `%`, line feeds and returns are written as `%25`,
/// `%0A` and `%0D`.
fn escape_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('%', "%25")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
        .replace('\t', "%09")
}

fn unescape_path(v: &str) -> PathBuf {
    PathBuf::from(
        v.replace("%0A", "\n")
            .replace("%0D", "\r")
            .replace("%09", "\t")
            .replace("%25", "%"),
    )
}

impl Default for Settings {
    /// The original's own defaults.
    fn default() -> Self {
        Settings {
            name: String::new(),
            initials: String::new(),
            unit: Unit::Inch,
            view_toolbar: true,
            paragraph_toolbar_tabs: false,
            document_zoom: 150.0,
            window_frame: None,
            interface_scale: 100,
            toolbar_spacing: 24,
            formatting_toolbar_spacing: 3,
            toolbar_picker_font_size: 12,
            bar_padding_horiz: 6,
            bar_padding_vert: 4,
            bar_background: 0xDDDDDD,
            line_color: 0x888888,
            ruler_background: 0xFFFFFF,
            ruler_foreground: 0x000000,
            toolbar_position: 0,
            show_styles_palette: false,
            styles_palette_docked: true,
            styles_palette_tab: 0,
            toolbar_buttons: Vec::new(),
            named_colors: Vec::new(),
            accepted_fonts: Vec::new(),
            highlight_missing_fonts: false,
            missing_font_reminder: true,
            markdown_source_font: "Courier New".into(),
            markdown_source_size: 20,
            markdown_display_font: "New York".into(),
            markdown_display_size: 24,
            smart_quotes: true,
            auto_links: true,
            auto_lists: true,
            always_suggest: true,
            spelling_as_you_type: true,
            custom_dictionary: None,
            words_to_system: false,
            ignore_uppercase: true,
            ignore_numbers: true,
            formatted_clipboard: true,
            show_hidden: false,
            table_gridlines: true,
            table_gridline_color: 0x808080,
            table_gridline_dotted: false,
            table_border_color: 0x000000,
            table_border_dotted: false,
            page_layout_color_background: DEFAULT_PAGE_SURROUND,
            page_layout_color_page_border: DEFAULT_PAGE_BORDER,
            text_boundaries: false,
            default_view: DefaultView::Normal,
            appearance: Appearance::Auto,
            open_with_ruler: true,
            open_with_formatting_toolbar: true,
            open_with_paragraph_toolbar: true,
            recent_files: true,
            recent_count: 10,
            recent_documents: Vec::new(),
            font_folders: Vec::new(),
            font_folder_levels: DEFAULT_FONT_LEVELS,
            autosave_secs: DEFAULT_AUTOSAVE_SECS,
            interpret_rtf: true,
            make_backup: false,
            foreign_as_quill: true,
            reopen: Reopen::Prompt,
            last_session: Vec::new(),
            quit_cleanly: true,
            font: "Georgia".into(),
            font_bold: false,
            font_italic: false,
            size: 24,
            document: DocumentProps::default(),
            section: SectionProps::default(),
        }
    }
}

impl Settings {
    /// A new, empty document with these defaults.
    pub fn new_document(&self) -> Document {
        let mut d = Document::with_default_font(&self.font, self.size);
        // A Bold or Italic Default Font is Normal's own style, so the
        // styles based on Normal follow it.
        if self.font_bold || self.font_italic {
            let normal = d.styles.get(0);
            let mut chp = normal.chp;
            chp.bold = self.font_bold;
            chp.italic = self.font_italic;
            let pap = normal.pap.clone();
            d.redefine_style(0, pap, chp);
        }
        d.props = self.document.clone();
        d.section = self.section.clone();
        d.summary.author = self.name.clone();
        d.keep_in_bounds();
        d.forget_history();
        d
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# Quill Settings\n");
        let b = |v: bool| if v { "1" } else { "0" }.to_string();

        section(&mut out, "General", Some("Preferences > General"));
        put(&mut out, "name", self.name.clone());
        put(&mut out, "initials", self.initials.clone());
        put(&mut out, "unit", self.unit.abbrev().into());
        put(&mut out, "smart_quotes", b(self.smart_quotes));
        put(&mut out, "auto_links", b(self.auto_links));
        put(&mut out, "auto_lists", b(self.auto_lists));
        put(&mut out, "formatted_clipboard", b(self.formatted_clipboard));

        section(&mut out, "View", Some("Preferences > View"));
        put(&mut out, "show_hidden", b(self.show_hidden));
        put(&mut out, "table_gridlines", b(self.table_gridlines));
        put(
            &mut out,
            "table_border_color",
            format!("{:06X}", self.table_border_color & 0xFF_FFFF),
        );
        put(&mut out, "table_border_dotted", b(self.table_border_dotted));
        put(
            &mut out,
            "table_gridline_color",
            format!("{:06X}", self.table_gridline_color & 0xFF_FFFF),
        );
        put(
            &mut out,
            "table_gridline_dotted",
            b(self.table_gridline_dotted),
        );
        put(
            &mut out,
            "page_layout_color_background",
            format!("{:06X}", self.page_layout_color_background & 0xFF_FFFF),
        );
        put(
            &mut out,
            "page_layout_color_page_border",
            format!("{:06X}", self.page_layout_color_page_border & 0xFF_FFFF),
        );
        put(&mut out, "text_boundaries", b(self.text_boundaries));
        put(
            &mut out,
            "default_view",
            self.default_view.key().to_string(),
        );
        put(&mut out, "appearance", self.appearance.key().to_string());
        put(&mut out, "open_with_ruler", b(self.open_with_ruler));
        put(
            &mut out,
            "open_with_formatting_toolbar",
            b(self.open_with_formatting_toolbar),
        );
        put(
            &mut out,
            "open_with_paragraph_toolbar",
            b(self.open_with_paragraph_toolbar),
        );
        put(&mut out, "recent_files", b(self.recent_files));
        put(&mut out, "recent_count", self.recent_count.to_string());

        section(&mut out, "Advanced", Some("Preferences > Advanced"));
        put(&mut out, "autosave_secs", self.autosave_secs.to_string());
        put(
            &mut out,
            "font_folder_levels",
            self.font_folder_levels.to_string(),
        );
        for folder in &self.font_folders {
            put(
                &mut out,
                "font_folder",
                format!("{}|{}", b(folder.enabled), escape_path(&folder.path)),
            );
        }

        section(
            &mut out,
            "Interface",
            Some(
                "Sizes are percentages; spacing and padding are in points. Quit Quill before editing.",
            ),
        );
        put(&mut out, "document_zoom", self.document_zoom.to_string());
        if let Some([x, y, w, h]) = self.window_frame {
            put(&mut out, "window_frame", format!("{x} {y} {w} {h}"));
        }
        put(
            &mut out,
            "interface_scale",
            self.interface_scale.to_string(),
        );
        put(
            &mut out,
            "toolbar_spacing",
            self.toolbar_spacing.to_string(),
        );
        put(
            &mut out,
            "formatting_toolbar_spacing",
            self.formatting_toolbar_spacing.to_string(),
        );
        put(
            &mut out,
            "toolbar_picker_font_size",
            self.toolbar_picker_font_size.to_string(),
        );
        put(
            &mut out,
            "bar_padding_horiz",
            self.bar_padding_horiz.to_string(),
        );
        put(
            &mut out,
            "bar_padding_vert",
            self.bar_padding_vert.to_string(),
        );

        section(
            &mut out,
            "Colors",
            Some("HTML codes like #DDDDDD. Bars are the toolbars and the ruler."),
        );
        let hex = |c: u32| format!("#{c:06X}");
        put(&mut out, "bar_background", hex(self.bar_background));
        put(&mut out, "line_color", hex(self.line_color));
        put(&mut out, "ruler_background", hex(self.ruler_background));
        put(&mut out, "ruler_foreground", hex(self.ruler_foreground));

        section(
            &mut out,
            "Open and Save",
            Some("Preferences > Open And Save"),
        );
        put(&mut out, "interpret_rtf", b(self.interpret_rtf));
        put(&mut out, "make_backup", b(self.make_backup));
        put(&mut out, "foreign_as_quill", b(self.foreign_as_quill));
        put(&mut out, "reopen_documents", self.reopen.key().into());
        put(&mut out, "quit_cleanly", b(self.quit_cleanly));

        section(
            &mut out,
            "Fonts",
            Some("Preferences > Fonts; size is in half-points"),
        );
        put(&mut out, "font", self.font.clone());
        put(&mut out, "font_bold", b(self.font_bold));
        put(&mut out, "font_italic", b(self.font_italic));
        put(&mut out, "size", self.size.to_string());
        // New documents' superscript and subscript sizes, in percent.
        let sc = self.document.scripts;
        put(&mut out, "superscript_raise", sc.raise.to_string());
        put(&mut out, "superscript_size", sc.super_size.to_string());
        put(&mut out, "subscript_drop", sc.drop.to_string());
        put(&mut out, "subscript_size", sc.sub_size.to_string());

        section(&mut out, "Spelling", Some("Preferences > Spelling"));
        put(&mut out, "always_suggest", b(self.always_suggest));
        put(
            &mut out,
            "spelling_as_you_type",
            b(self.spelling_as_you_type),
        );
        put(&mut out, "ignore_uppercase", b(self.ignore_uppercase));
        put(&mut out, "ignore_numbers", b(self.ignore_numbers));
        put(&mut out, "words_to_system", b(self.words_to_system));
        if let Some(path) = &self.custom_dictionary {
            put(&mut out, "custom_dictionary", escape_path(path));
        }

        section(
            &mut out,
            "Toolbar",
            Some("Preferences > Toolbar; position 0 top, 1 left, 2 right"),
        );
        put(&mut out, "view_toolbar", b(self.view_toolbar));
        put(
            &mut out,
            "paragraph_toolbar_tabs",
            b(self.paragraph_toolbar_tabs),
        );
        put(
            &mut out,
            "toolbar_position",
            self.toolbar_position.to_string(),
        );
        put(&mut out, "show_styles_palette", b(self.show_styles_palette));
        put(
            &mut out,
            "styles_palette_docked",
            b(self.styles_palette_docked),
        );
        put(
            &mut out,
            "styles_palette_tab",
            self.styles_palette_tab.to_string(),
        );
        put(
            &mut out,
            "toolbar_buttons",
            self.toolbar_buttons
                .iter()
                .map(|(p, i, c)| format!("{p}|{i}|{c}"))
                .collect::<Vec<_>>()
                .join(";"),
        );

        section(
            &mut out,
            "Colors",
            Some("The custom colors of the color palette, newest first, as HTML codes"),
        );
        for (path, fonts) in &self.accepted_fonts {
            let line = format!("{}\t{}", escape_path(path), fonts.join("\t"));
            put(&mut out, "accepted_fonts", line);
        }
        put(
            &mut out,
            "highlight_missing_fonts",
            b(self.highlight_missing_fonts),
        );
        put(
            &mut out,
            "missing_font_reminder",
            b(self.missing_font_reminder),
        );
        put(
            &mut out,
            "markdown_source_font",
            self.markdown_source_font.clone(),
        );
        put(
            &mut out,
            "markdown_source_size",
            self.markdown_source_size.to_string(),
        );
        put(
            &mut out,
            "markdown_display_font",
            self.markdown_display_font.clone(),
        );
        put(
            &mut out,
            "markdown_display_size",
            self.markdown_display_size.to_string(),
        );
        for (color, name) in &self.named_colors {
            put(&mut out, "named_color", format!("{color:06X}\t{name}"));
        }

        section(
            &mut out,
            "Document Defaults",
            Some("Saved by Use As Default; lengths in twips, 1440 to the inch"),
        );
        let d = &self.document;
        put(&mut out, "doc.page_width", d.page_width.to_string());
        put(&mut out, "doc.page_height", d.page_height.to_string());
        put(&mut out, "doc.margin_left", d.margin_left.to_string());
        put(&mut out, "doc.margin_right", d.margin_right.to_string());
        put(&mut out, "doc.margin_top", d.margin_top.to_string());
        put(&mut out, "doc.margin_bottom", d.margin_bottom.to_string());
        put(&mut out, "doc.top_exact", b(d.top_exact));
        put(&mut out, "doc.bottom_exact", b(d.bottom_exact));
        put(&mut out, "doc.gutter", d.gutter.to_string());
        put(&mut out, "doc.mirror_margins", b(d.mirror_margins));
        put(&mut out, "doc.default_tab", d.default_tab.to_string());
        put(&mut out, "doc.widow_control", b(d.widow_control));
        put(&mut out, "doc.print_hidden", b(d.print_hidden));
        put(&mut out, "doc.even_odd_headers", b(d.even_odd_headers));
        put(
            &mut out,
            "doc.footnote_position",
            (d.footnote_position as u8).to_string(),
        );
        put(
            &mut out,
            "doc.footnote_restart_each_page",
            b(d.footnote_restart_each_page),
        );
        put(&mut out, "doc.footnote_start", d.footnote_start.to_string());
        let s = &self.section;
        put(&mut out, "sec.start", (s.start as u8).to_string());
        put(&mut out, "sec.columns", s.columns.to_string());
        put(&mut out, "sec.column_spacing", s.column_spacing.to_string());
        put(
            &mut out,
            "sec.header_from_top",
            s.header_from_top.to_string(),
        );
        put(
            &mut out,
            "sec.footer_from_bottom",
            s.footer_from_bottom.to_string(),
        );
        put(
            &mut out,
            "sec.different_first_page",
            b(s.different_first_page),
        );
        put(
            &mut out,
            "sec.page_number_format",
            (s.page_number_format as u8).to_string(),
        );
        put(
            &mut out,
            "sec.restart_page_numbers",
            b(s.restart_page_numbers),
        );
        put(
            &mut out,
            "sec.margin_page_numbers",
            b(s.margin_page_numbers),
        );
        put(
            &mut out,
            "sec.page_number_from_top",
            s.page_number_from_top.to_string(),
        );
        put(
            &mut out,
            "sec.page_number_from_right",
            s.page_number_from_right.to_string(),
        );
        put(&mut out, "sec.include_endnotes", b(s.include_endnotes));
        put(
            &mut out,
            "sec.line_numbers",
            (s.line_numbers as u8).to_string(),
        );
        put(&mut out, "sec.line_count_by", s.line_count_by.to_string());
        put(
            &mut out,
            "sec.line_from_text",
            s.line_from_text.map_or("auto".into(), |t| t.to_string()),
        );

        if !self.recent_documents.is_empty() {
            section(&mut out, "Recent Documents", None);
        }
        for path in &self.recent_documents {
            put(&mut out, "recent_document", escape_path(path));
        }
        if !self.last_session.is_empty() {
            section(&mut out, "Open When Quill Last Quit", None);
        }
        for path in &self.last_session {
            put(&mut out, "last_session_document", escape_path(path));
        }
        out
    }

    /// Read settings written by `to_text`. Unknown or broken lines are
    /// skipped so hand-edited files can be read safely.
    pub fn from_text(text: &str) -> Settings {
        Self::from_text_with_defaults(text, Settings::default())
    }

    fn from_text_with_defaults(text: &str, mut s: Settings) -> Self {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let flag = v == "1";
            let num = |cur: Twips| v.parse().unwrap_or(cur);
            let d = &mut s.document;
            let sec = &mut s.section;
            match k {
                "name" => s.name = v.into(),
                "view_toolbar" => s.view_toolbar = flag,
                "paragraph_toolbar_tabs" => s.paragraph_toolbar_tabs = flag,
                "document_zoom" => {
                    let zoom = v
                        .parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite())
                        .unwrap_or(150.0)
                        .clamp(10.0, 800.0);
                    s.document_zoom = zoom;
                }
                "window_frame" => {
                    let n: Vec<i32> = v
                        .split_whitespace()
                        .filter_map(|n| n.parse().ok())
                        .collect();
                    s.window_frame = match n[..] {
                        [x, y, w, h] if w > 0 && h > 0 => Some([x, y, w, h]),
                        _ => None,
                    };
                }
                "interface_scale" => s.interface_scale = v.parse().unwrap_or(100u16).clamp(50, 300),
                "toolbar_spacing" => s.toolbar_spacing = v.parse().unwrap_or(24u16).clamp(16, 48),
                "formatting_toolbar_spacing" => {
                    s.formatting_toolbar_spacing = v.parse().unwrap_or(3u16).min(20)
                }
                "bar_background" => s.bar_background = html_color(v).unwrap_or(s.bar_background),
                "ruler_background" => {
                    s.ruler_background = html_color(v).unwrap_or(s.ruler_background)
                }
                "ruler_foreground" => {
                    s.ruler_foreground = html_color(v).unwrap_or(s.ruler_foreground)
                }
                "toolbar_picker_font_size" => {
                    s.toolbar_picker_font_size =
                        v.parse().unwrap_or(s.toolbar_picker_font_size).clamp(6, 36)
                }
                "bar_padding_horiz" => {
                    s.bar_padding_horiz = v.parse().unwrap_or(s.bar_padding_horiz).min(40)
                }
                "bar_padding_vert" => {
                    s.bar_padding_vert = v.parse().unwrap_or(s.bar_padding_vert).min(20)
                }
                "line_color" => s.line_color = html_color(v).unwrap_or(s.line_color),
                "toolbar_position" => s.toolbar_position = v.parse().unwrap_or(0u8).min(2),
                "show_styles_palette" => s.show_styles_palette = flag,
                "styles_palette_docked" => s.styles_palette_docked = flag,
                "styles_palette_tab" => s.styles_palette_tab = v.parse().unwrap_or(0u8).min(2),
                "toolbar_buttons" => {
                    s.toolbar_buttons = v
                        .split(';')
                        .filter_map(|b| {
                            let mut f = b.split('|');
                            Some((
                                f.next()?.parse().ok()?,
                                f.next()?.parse().ok()?,
                                f.next()?.to_string(),
                            ))
                        })
                        .collect()
                }
                "accepted_fonts" => {
                    let mut parts = v.split('\t');
                    if let Some(path) = parts.next()
                        && s.accepted_fonts.len() < MAX_ACCEPTED_DOCUMENTS
                    {
                        let fonts: Vec<String> = parts.map(str::to_string).collect();
                        if !fonts.is_empty() {
                            s.accepted_fonts.push((unescape_path(path), fonts));
                        }
                    }
                }
                "highlight_missing_fonts" => s.highlight_missing_fonts = flag,
                "missing_font_reminder" => s.missing_font_reminder = flag,
                "markdown_source_font" if !v.is_empty() => s.markdown_source_font = v.into(),
                "markdown_source_size" => {
                    s.markdown_source_size = v.parse().unwrap_or(20u16).clamp(2, 3276)
                }
                "markdown_display_font" if !v.is_empty() => s.markdown_display_font = v.into(),
                "markdown_display_size" => {
                    s.markdown_display_size = v.parse().unwrap_or(24u16).clamp(2, 3276)
                }
                "named_color" => {
                    // A color with no name has nothing after its tab.
                    let (color, name) = v.split_once('\t').unwrap_or((v, ""));
                    if let Some(color) = html_color(color)
                        && s.named_colors.len() < MAX_NAMED_COLORS
                    {
                        s.named_colors.push((color, name.to_string()));
                    }
                }
                "initials" => s.initials = v.into(),
                "unit" => {
                    s.unit = Unit::ALL
                        .into_iter()
                        .find(|u| u.abbrev() == v)
                        .unwrap_or_default()
                }
                "smart_quotes" => s.smart_quotes = flag,
                "auto_links" => s.auto_links = flag,
                "auto_lists" => s.auto_lists = flag,
                "always_suggest" => s.always_suggest = flag,
                "spelling_as_you_type" => s.spelling_as_you_type = flag,
                "ignore_uppercase" => s.ignore_uppercase = flag,
                "ignore_numbers" => s.ignore_numbers = flag,
                "formatted_clipboard" => s.formatted_clipboard = flag,
                "show_hidden" => s.show_hidden = flag,
                "table_gridlines" => s.table_gridlines = flag,
                "table_border_color" => {
                    if let Some(c) = html_color(v) {
                        s.table_border_color = c;
                    }
                }
                "table_border_dotted" => s.table_border_dotted = flag,
                "table_gridline_color" => {
                    if let Some(c) = html_color(v) {
                        s.table_gridline_color = c;
                    }
                }
                "page_layout_color_page_border" => {
                    if let Some(c) = html_color(v) {
                        s.page_layout_color_page_border = c;
                    }
                }
                "page_layout_color_background" => {
                    if let Some(c) = html_color(v) {
                        s.page_layout_color_background = c;
                    }
                }
                "table_gridline_dotted" => s.table_gridline_dotted = flag,
                "text_boundaries" => s.text_boundaries = flag,
                "default_view" => {
                    s.default_view = DefaultView::ALL
                        .into_iter()
                        .find(|d| d.key() == v)
                        .unwrap_or_default()
                }
                "appearance" => {
                    s.appearance = Appearance::ALL
                        .into_iter()
                        .find(|a| a.key() == v)
                        .unwrap_or_default()
                }
                "open_with_ruler" => s.open_with_ruler = flag,
                "open_with_paragraph_toolbar" => s.open_with_paragraph_toolbar = flag,
                "open_with_formatting_toolbar" => s.open_with_formatting_toolbar = flag,
                "recent_files" => s.recent_files = flag,
                "recent_count" => s.recent_count = v.parse().unwrap_or(10usize).clamp(1, 50),
                "autosave_secs" => {
                    s.autosave_secs = v.parse().unwrap_or(DEFAULT_AUTOSAVE_SECS).min(3600)
                }
                "font_folder_levels" => {
                    s.font_folder_levels = v
                        .parse()
                        .unwrap_or(DEFAULT_FONT_LEVELS)
                        .min(MAX_FONT_LEVELS)
                }
                "font_folder" => {
                    if let Some((on, path)) = v.split_once('|')
                        && !path.is_empty()
                        && s.font_folders.len() < MAX_FONT_FOLDERS
                    {
                        s.font_folders.push(FontFolder {
                            path: unescape_path(path),
                            enabled: on == "1",
                        });
                    }
                }
                "foreign_as_quill" => s.foreign_as_quill = flag,
                "quit_cleanly" => s.quit_cleanly = flag,
                "words_to_system" => s.words_to_system = flag,
                "custom_dictionary" if !v.is_empty() => {
                    s.custom_dictionary = Some(unescape_path(v))
                }
                "recent_document" => {
                    if !v.is_empty() && s.recent_documents.len() < MAX_RECENT_DOCUMENTS {
                        s.recent_documents.push(unescape_path(v));
                    }
                }
                "last_session_document" => {
                    if !v.is_empty() && s.last_session.len() < 200 {
                        s.last_session.push(unescape_path(v));
                    }
                }
                "reopen_documents" => {
                    s.reopen = Reopen::ALL
                        .into_iter()
                        .find(|r| r.key() == v)
                        .unwrap_or_default()
                }
                "interpret_rtf" => s.interpret_rtf = flag,
                "make_backup" => s.make_backup = flag,
                "font" if !v.is_empty() => s.font = v.into(),
                "font_bold" => s.font_bold = flag,
                "font_italic" => s.font_italic = flag,
                "size" => s.size = v.parse().unwrap_or(24u16).clamp(2, 3276),
                "superscript_raise" | "superscript_size" | "subscript_drop" | "subscript_size" => {
                    if let Ok(n) = v.trim().parse::<u16>() {
                        let sc = &mut s.document.scripts;
                        match k {
                            "superscript_raise" => sc.raise = n,
                            "superscript_size" => sc.super_size = n,
                            "subscript_drop" => sc.drop = n,
                            _ => sc.sub_size = n,
                        }
                        *sc = sc.clamped();
                    }
                }
                "doc.page_width" => d.page_width = num(d.page_width).clamp(1440, 31680),
                "doc.page_height" => d.page_height = num(d.page_height).clamp(1440, 31680),
                "doc.margin_left" => d.margin_left = num(d.margin_left),
                "doc.margin_right" => d.margin_right = num(d.margin_right),
                "doc.margin_top" => d.margin_top = num(d.margin_top),
                "doc.margin_bottom" => d.margin_bottom = num(d.margin_bottom),
                "doc.top_exact" => d.top_exact = flag,
                "doc.bottom_exact" => d.bottom_exact = flag,
                "doc.gutter" => d.gutter = num(d.gutter),
                "doc.mirror_margins" => d.mirror_margins = flag,
                "doc.default_tab" => d.default_tab = num(d.default_tab).max(1),
                "doc.widow_control" => d.widow_control = flag,
                "doc.print_hidden" => d.print_hidden = flag,
                "doc.even_odd_headers" => d.even_odd_headers = flag,
                "doc.footnote_position" => {
                    d.footnote_position = match v {
                        "1" => FootnotePosition::BeneathText,
                        "2" => FootnotePosition::EndOfSection,
                        "3" => FootnotePosition::EndOfDocument,
                        _ => FootnotePosition::BottomOfPage,
                    }
                }
                "doc.footnote_restart_each_page" => d.footnote_restart_each_page = flag,
                "doc.footnote_start" => d.footnote_start = v.parse().unwrap_or(1),
                "sec.start" => {
                    sec.start = match v {
                        "1" => SectionStart::NewColumn,
                        "2" => SectionStart::NewPage,
                        "3" => SectionStart::EvenPage,
                        "4" => SectionStart::OddPage,
                        _ => SectionStart::NoBreak,
                    }
                }
                "sec.columns" => sec.columns = v.parse().unwrap_or(1u16).max(1),
                "sec.column_spacing" => sec.column_spacing = num(sec.column_spacing),
                "sec.header_from_top" => sec.header_from_top = num(sec.header_from_top),
                "sec.footer_from_bottom" => sec.footer_from_bottom = num(sec.footer_from_bottom),
                "sec.different_first_page" => sec.different_first_page = flag,
                "sec.page_number_format" => {
                    sec.page_number_format = match v {
                        "1" => PageNumberFormat::UpperRoman,
                        "2" => PageNumberFormat::LowerRoman,
                        "3" => PageNumberFormat::UpperLetter,
                        "4" => PageNumberFormat::LowerLetter,
                        _ => PageNumberFormat::Arabic,
                    }
                }
                "sec.restart_page_numbers" => sec.restart_page_numbers = flag,
                "sec.margin_page_numbers" => sec.margin_page_numbers = flag,
                "sec.page_number_from_top" => {
                    sec.page_number_from_top = num(sec.page_number_from_top)
                }
                "sec.page_number_from_right" => {
                    sec.page_number_from_right = num(sec.page_number_from_right)
                }
                "sec.include_endnotes" => sec.include_endnotes = flag,
                "sec.line_numbers" => {
                    use quill_core::LineNumbering as L;
                    sec.line_numbers = match v {
                        "1" => L::ByPage,
                        "2" => L::BySection,
                        "3" => L::Continuous,
                        _ => L::Off,
                    }
                }
                "sec.line_count_by" => sec.line_count_by = v.parse().unwrap_or(1u16).max(1),
                "sec.line_from_text" => sec.line_from_text = v.parse().ok(),
                _ => {}
            }
        }
        s.document.keep_in_bounds();
        s.section.keep_in_bounds();
        s.recent_documents.truncate(MAX_RECENT_DOCUMENTS);
        s
    }
}

thread_local! {
    static CURRENT: RefCell<Settings> = RefCell::new(Settings::default());
}

/// Whether changes are written to disk. Off for snapshots and tests.
static PERSIST: AtomicBool = AtomicBool::new(false);

/// Where the settings file lives.
/// A settings file is a few KB of text; anything past 1 MiB, or not a
/// regular file, is refused before it is read.
const MAX_SETTINGS_BYTES: u64 = 1 << 20;

pub fn read_text(path: &std::path::Path) -> Result<String, String> {
    let bytes = quill_formats::read_file_with_limit(path, MAX_SETTINGS_BYTES)?;
    String::from_utf8(bytes).map_err(|_| "The file is not UTF-8 text.".to_string())
}

/// The name this copy of Quill keeps its files under. A debug build uses its
/// own, so it never reads or overwrites the settings and recovery files of an
/// installed Quill. `QUILL_PROFILE` picks another name for a test run.
pub fn profile() -> String {
    std::env::var("QUILL_PROFILE")
        .ok()
        .and_then(|name| safe_profile(&name))
        .unwrap_or_else(|| {
            if cfg!(debug_assertions) {
                "Quill Debug".to_string()
            } else {
                "Quill".to_string()
            }
        })
}

/// A profile is one short file name, never a path.
fn safe_profile(name: &str) -> Option<String> {
    let mut safe = String::new();
    let mut separator = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_') {
            safe.push(c);
            separator = false;
        } else if !safe.is_empty() {
            separator = true;
        }
        if safe.len() >= 64 {
            break;
        }
        if separator {
            safe.push('-');
            separator = false;
        }
    }
    let safe = safe.trim_matches([' ', '-', '_']).to_string();
    (!safe.is_empty()).then_some(safe)
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let profile = profile();
    if cfg!(target_os = "macos") {
        return Some(home?.join(format!("Library/Preferences/{profile} Settings")));
    }
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
        return Some(base.join(format!("{profile}/Quill Settings.txt")));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))?;
    let folder = profile.to_lowercase().replace(' ', "-");
    Some(base.join(format!("{folder}/settings")))
}

/// Another file kept beside the settings (the glossary), when files are
/// being written at all. On the Mac `mac` is the file's name after the
/// profile ("Glossary" becomes "Quill Glossary"); the other systems keep
/// the profile in the folder name instead.
pub fn sibling(mac: &str, windows: &str, other: &str) -> Option<PathBuf> {
    if !PERSIST.load(Ordering::Relaxed) {
        return None;
    }
    let dir = path()?.parent()?.to_path_buf();
    Some(dir.join(if cfg!(target_os = "macos") {
        format!("{} {mac}", profile())
    } else if cfg!(windows) {
        windows.to_string()
    } else {
        other.to_string()
    }))
}

/// Safely replace one of Quill's small user-data files, creating its settings
/// folder on first use. Callers decide how to describe a failure to the user.
pub fn write_user_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create \u{201c}{}\u{201d}: {e}", dir.display()))?;
    }
    quill_formats::write_atomic(path, bytes)
}

/// Load the settings file and save changes from now on.
pub fn load() {
    let (w, h, metric) = crate::regional::detect();
    let mut defaults = Settings {
        unit: if metric { Unit::Cm } else { Unit::Inch },
        ..Default::default()
    };
    defaults.document.page_width = w;
    defaults.document.page_height = h;
    CURRENT.with(|c| *c.borrow_mut() = defaults);
    if let Some(p) = path()
        && let Ok(text) = read_text(&p)
    {
        let s = Settings::from_text_with_defaults(&text, get());
        // Write the file back in its full layout when it is missing
        // settings or laid out by hand, so every setting shows with its
        // current value.
        let tidy = s.to_text();
        if tidy != text
            && let Some(destination) = path()
        {
            let _ = write_user_file(&destination, tidy.as_bytes());
        }
        CURRENT.with(|c| *c.borrow_mut() = s);
    }
    sync_markdown_fonts();
    PERSIST.store(true, Ordering::Relaxed);
}

thread_local! {
    /// Markdown fonts changed since the windows last looked: the fonts
    /// before the first change and the ones now.
    static MARKDOWN_CHANGE: RefCell<Option<(quill_formats::markdown::Fonts, quill_formats::markdown::Fonts)>> =
        const { RefCell::new(None) };
}

fn markdown_fonts_of(s: &Settings) -> quill_formats::markdown::Fonts {
    quill_formats::markdown::Fonts {
        source_font: s.markdown_source_font.clone(),
        source_size: s.markdown_source_size,
        display_font: s.markdown_display_font.clone(),
        display_size: s.markdown_display_size,
    }
}

/// The Markdown font change the open windows have not been given yet, once.
pub fn take_markdown_font_change() -> Option<(
    quill_formats::markdown::Fonts,
    quill_formats::markdown::Fonts,
)> {
    MARKDOWN_CHANGE.with(|c| c.borrow_mut().take())
}

/// Tell the Markdown reader which fonts the settings choose.
fn sync_markdown_fonts() {
    quill_formats::markdown::set_fonts(with(markdown_fonts_of));
}

/// A copy of the current settings.
pub fn get() -> Settings {
    CURRENT.with(|c| c.borrow().clone())
}

/// Read one thing from the settings without copying them.
pub fn with<R>(f: impl FnOnce(&Settings) -> R) -> R {
    CURRENT.with(|c| f(&c.borrow()))
}

/// Change the settings and save them.
pub fn update(f: impl FnOnce(&mut Settings)) {
    let text = CURRENT.with(|c| {
        let mut s = c.borrow_mut();
        let before = markdown_fonts_of(&s);
        f(&mut s);
        let after = markdown_fonts_of(&s);
        if before != after {
            MARKDOWN_CHANGE.with(|m| {
                let mut m = m.borrow_mut();
                let first = m.take().map_or(before, |(first, _)| first);
                *m = Some((first, after));
            });
        }
        s.to_text()
    });
    sync_markdown_fonts();
    if !PERSIST.load(Ordering::Relaxed) {
        return;
    }
    if let Some(p) = path()
        && let Err(e) = write_user_file(&p, text.as_bytes())
    {
        crate::app::alert(
            "Quill could not save your settings.",
            &format!("{e}\n\nThe change will apply until Quill closes."),
        );
    }
}

/// Take out the extra font sources that are no longer there: moved,
/// renamed or deleted. Checked at launch and then at most every few
/// seconds, since it looks at the disk and is asked on every wake.
pub fn drop_missing_font_folders() {
    use std::time::{Duration, Instant};
    thread_local! {
        static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }
    let now = Instant::now();
    if LAST
        .get()
        .is_some_and(|at| now < at + Duration::from_secs(3))
    {
        return;
    }
    LAST.set(Some(now));
    let mut folders = with(|s| s.font_folders.clone());
    if without_missing(&mut folders) {
        update(|s| s.font_folders = folders);
    }
}

/// Drop the sources whose path is gone, and say whether any were.
fn without_missing(folders: &mut Vec<FontFolder>) -> bool {
    let before = folders.len();
    folders.retain(|f| f.path.exists());
    folders.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_sources_that_are_gone_are_dropped() {
        let here = std::env::temp_dir().join(format!("quill-font-source-{}", std::process::id()));
        std::fs::create_dir_all(&here).unwrap();
        let gone = here.join("moved away");
        let mut folders = vec![
            FontFolder {
                path: here.clone(),
                enabled: true,
            },
            FontFolder {
                path: gone,
                enabled: false,
            },
        ];
        assert!(without_missing(&mut folders));
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, here);
        assert!(!without_missing(&mut folders));
        std::fs::remove_dir_all(&here).unwrap();
    }

    #[test]
    fn preferences_persist_explicit_choices() {
        assert!(Settings::default().smart_quotes);
        assert!(!Settings::from_text("smart_quotes = 0\n").smart_quotes);
        let s = Settings {
            smart_quotes: false,
            auto_links: false,
            auto_lists: false,
            recent_count: 25,
            foreign_as_quill: false,
            custom_dictionary: Some(PathBuf::from("/Custom/100%\nwords.txt")),
            words_to_system: true,
            quit_cleanly: false,
            markdown_source_font: "Menlo".into(),
            markdown_source_size: 26,
            markdown_display_font: "Georgia".into(),
            markdown_display_size: 28,
            ..Default::default()
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(Settings::from_text("recent_count = 999\n").recent_count, 50);
        assert_eq!(Settings::from_text("recent_count = 0\n").recent_count, 1);
    }

    #[test]
    fn interface_sizes_read_back_and_stay_in_range() {
        let s = Settings::from_text("interface_scale = 90\ntoolbar_spacing = 22\n");
        assert_eq!((s.interface_scale, s.toolbar_spacing), (90, 22));
        assert!(
            !Settings::from_text("open_with_formatting_toolbar = 0\n").open_with_formatting_toolbar
        );
        let back = Settings::from_text(&s.to_text());
        assert_eq!((back.interface_scale, back.toolbar_spacing), (90, 22));
        let wild = Settings::from_text("interface_scale = 5\ntoolbar_spacing = x\n");
        assert_eq!((wild.interface_scale, wild.toolbar_spacing), (50, 24));
    }

    #[test]
    fn bar_colors_read_html_codes() {
        let s = Settings::from_text("bar_background = #E8E8E8\nline_color = a0a0a0\n");
        assert_eq!((s.bar_background, s.line_color), (0xE8E8E8, 0xA0A0A0));
        assert!(
            s.to_text()
                .contains("bar_background = #E8E8E8\nline_color = #A0A0A0\n")
        );
        let bad = Settings::from_text("bar_background = gray\nline_color = #12345\n");
        assert_eq!((bad.bar_background, bad.line_color), (0xDDDDDD, 0x888888));
    }

    #[test]
    fn settings_file_is_grouped_in_sections() {
        let text = Settings::default().to_text();
        assert!(
            text.starts_with("# Quill Settings\n\n# General\n## Preferences > General\nname = ")
        );
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate().skip(1) {
            if line.starts_with("# ") {
                assert_eq!(lines[i - 1], "", "no blank line before {line}");
            }
        }
        for title in ["View", "Interface", "Toolbar", "Document Defaults"] {
            assert!(
                text.contains(&format!("\n\n# {title}\n")),
                "{title} missing"
            );
        }
        assert_eq!(Settings::from_text(&text).to_text(), text);
    }

    #[test]
    fn round_trips_through_text() {
        let mut s = Settings {
            name: "Ada".into(),
            unit: Unit::Cm,
            smart_quotes: true,
            font: "Times".into(),
            size: 20,
            recent_documents: vec![PathBuf::from("/Documents/100%\nfinal.qwp")],
            ..Default::default()
        };
        s.document.margin_left = 2000;
        s.document.footnote_position = FootnotePosition::EndOfSection;
        s.show_styles_palette = true;
        s.styles_palette_docked = false;
        s.styles_palette_tab = 2;
        s.section.columns = 3;
        s.section.start = SectionStart::OddPage;
        s.section.page_number_format = PageNumberFormat::UpperLetter;
        assert_eq!(Settings::from_text(&s.to_text()), s);
    }

    #[test]
    fn junk_lines_are_skipped() {
        let s = Settings::from_text("nonsense\nsize = big\nname = Bo\ndocument_zoom = 999\n");
        assert_eq!(s.name, "Bo");
        assert_eq!(s.size, 24);
        assert_eq!(s.document_zoom, 800.0);
    }

    #[test]
    fn profiles_are_file_names_not_paths() {
        assert_eq!(safe_profile(" Test Copy ").as_deref(), Some("Test Copy"));
        assert_eq!(safe_profile("../../outside").as_deref(), Some("outside"));
        assert_eq!(
            safe_profile("team/red\\blue").as_deref(),
            Some("team-red-blue")
        );
        assert_eq!(safe_profile("../..").as_deref(), None);
    }

    #[test]
    fn zoom_and_font_choices_are_read_as_saved() {
        assert_eq!(
            Settings::from_text("document_zoom = 125\n").document_zoom,
            125.0
        );
        assert_eq!(
            Settings::from_text("document_zoom = 110\n").document_zoom,
            110.0
        );
        assert_eq!(Settings::from_text("font = New York\n").font, "New York");
        assert_eq!(Settings::from_text("font = Palatino\n").font, "Palatino");
    }

    #[test]
    fn user_file_write_creates_its_folder_and_replaces_safely() {
        let dir = std::env::temp_dir().join(format!("quill-user-file-{}", std::process::id()));
        let path = dir.join("nested/settings");
        let _ = std::fs::remove_dir_all(&dir);

        write_user_file(&path, b"first").unwrap();
        write_user_file(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_files_are_read_with_a_size_limit_and_only_when_regular() {
        let dir = std::env::temp_dir().join(format!("quill-settings-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.txt");
        std::fs::write(&ok, "unit = cm\n").unwrap();
        assert_eq!(read_text(&ok).unwrap(), "unit = cm\n");
        let big = dir.join("big.txt");
        std::fs::write(&big, vec![b'#'; MAX_SETTINGS_BYTES as usize + 1]).unwrap();
        assert!(read_text(&big).unwrap_err().contains("too large"));
        let binary = dir.join("binary.txt");
        std::fs::write(&binary, [0xff, 0xfe]).unwrap();
        assert!(read_text(&binary).is_err());
        assert!(read_text(&dir).unwrap_err().contains("Not a regular file"));
        #[cfg(unix)]
        {
            let pipe = dir.join("pipe");
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&pipe)
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(read_text(&pipe).unwrap_err().contains("Not a regular file"));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hostile_defaults_are_bounded_before_creating_or_measuring_documents() {
        let s = Settings::from_text(
            "doc.margin_left = 2147483647\ndoc.margin_right = 2147483647\n\
             doc.margin_top = -2147483648\ndoc.gutter = 2147483647\n\
             doc.default_tab = 2147483647\nsec.columns = 65535\n\
             sec.column_spacing = 2147483647\nsec.header_from_top = -2147483648\n",
        );
        assert!(s.document.text_width() >= quill_core::limits::MIN_TEXT);
        assert!(s.section.columns <= quill_core::limits::MAX_COLUMNS);
        assert_eq!(s.document.margin_top, 0);
        assert_eq!(s.section.header_from_top, 0);
        let mut direct = s;
        direct.document.margin_left = i32::MAX;
        direct.document.margin_right = i32::MAX;
        direct.section.columns = u16::MAX;
        let doc = direct.new_document();
        assert!(doc.props.text_width() >= quill_core::limits::MIN_TEXT);
        assert!(doc.section.columns <= quill_core::limits::MAX_COLUMNS);
        assert!(!doc.is_modified());
    }

    #[test]
    fn new_documents_use_the_defaults() {
        let mut s = Settings {
            font: "Geneva".into(),
            size: 20,
            name: "Ada".into(),
            ..Default::default()
        };
        s.document.margin_left = 720;
        let d = s.new_document();
        assert_eq!(d.props.margin_left, 720);
        assert_eq!(d.summary.author, "Ada");
        let chp = d.styles.get(0).chp;
        assert_eq!(d.font_name(chp.font), "Geneva");
        assert_eq!(chp.size, 20);
        assert!(!d.is_modified());
    }
    #[test]
    fn window_frame_round_trips_and_rejects_nonsense() {
        let s = Settings {
            window_frame: Some([-1200, 40, 900, 700]),
            ..Default::default()
        };
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.window_frame, Some([-1200, 40, 900, 700]));
        for bad in ["1 2 3", "1 2 0 5", "a b c d"] {
            let text = format!("window_frame = {bad}\n");
            assert_eq!(Settings::from_text(&text).window_frame, None);
        }
    }
    #[test]
    fn saved_colors_keep_their_names() {
        let s = Settings {
            named_colors: vec![
                (0xFF0000, "Company Color".into()),
                (0x00FF00, String::new()),
            ],
            ..Default::default()
        };
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.named_colors, s.named_colors);
    }

    #[test]
    fn kept_fonts_belong_to_a_document() {
        let mut s = Settings::default();
        s.accepted_fonts
            .push((PathBuf::from("/Docs/A b.qwp"), vec!["Nosuch Sans".into()]));
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.accepted_fonts, s.accepted_fonts);
    }

    #[test]
    fn new_table_borders_default_to_black_and_round_trip() {
        let s = Settings::default();
        assert_eq!((s.table_border_color, s.table_border_dotted), (0, false));
        assert_eq!(s.table_gridline_color, 0x808080);
        let s = Settings::from_text("table_border_color = #336699\ntable_border_dotted = 1\n");
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.table_border_color, 0x336699);
        assert!(back.table_border_dotted);
    }

    #[test]
    fn gridline_look_round_trips() {
        let s = Settings::default();
        assert_eq!(
            (s.table_gridline_color, s.table_gridline_dotted),
            (0x808080, false)
        );
        let s = Settings::from_text("table_gridline_color = #C0C0C0\ntable_gridline_dotted = 1\n");
        assert_eq!(
            (s.table_gridline_color, s.table_gridline_dotted),
            (0xC0C0C0, true)
        );
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.table_gridline_color, 0xC0C0C0);
        assert!(back.table_gridline_dotted);
        // Nonsense keeps the default.
        let s = Settings::from_text("table_gridline_color = gray\n");
        assert_eq!(s.table_gridline_color, 0x808080);
        let s = Settings::from_text("page_layout_color_background = D9D9D9\n");
        assert_eq!(
            Settings::from_text(&s.to_text()).page_layout_color_background,
            0xD9D9D9
        );
        assert_eq!(Settings::default().page_layout_color_background, 0xE4E4E4);
        let s = Settings::from_text("page_layout_color_page_border = 999999\n");
        assert_eq!(
            Settings::from_text(&s.to_text()).page_layout_color_page_border,
            0x999999
        );
    }
    #[test]
    fn appearance_reads_back_and_defaults_to_auto() {
        assert_eq!(Settings::default().appearance, Appearance::Auto);
        for a in Appearance::ALL {
            let s = Settings {
                appearance: a,
                ..Settings::default()
            };
            assert_eq!(Settings::from_text(&s.to_text()).appearance, a);
        }
        let s = Settings::from_text("appearance = purple\n");
        assert_eq!(s.appearance, Appearance::Auto);
    }

    #[test]
    fn appearance_decides_dark_with_the_system() {
        assert!(Appearance::Auto.is_dark(true));
        assert!(!Appearance::Auto.is_dark(false));
        assert!(!Appearance::Light.is_dark(true));
        assert!(Appearance::Dark.is_dark(false));
    }

    #[test]
    fn default_view_reads_back() {
        for d in DefaultView::ALL {
            let s = Settings {
                default_view: d,
                ..Default::default()
            };
            assert_eq!(Settings::from_text(&s.to_text()).default_view, d);
        }
    }
    #[test]
    fn regional_seeds_preserve_saved_units_and_new_document_paper() {
        let mut german = Settings {
            unit: Unit::Cm,
            ..Default::default()
        };
        german.document.page_width = 11906;
        german.document.page_height = 16838;
        let seeded = Settings::from_text_with_defaults("name = Ada\n", german.clone());
        assert_eq!(seeded.unit, Unit::Cm);
        assert_eq!(seeded.new_document().props.page_width, 11906);
        let mut chosen = Settings {
            unit: Unit::Pica,
            ..Default::default()
        };
        chosen.document.page_width = 14400;
        chosen.document.page_height = 18000;
        let loaded = Settings::from_text_with_defaults(&chosen.to_text(), german);
        assert_eq!(loaded.unit, Unit::Pica);
        assert_eq!(loaded.new_document().props.page_width, 14400);
        assert_eq!(loaded.document.page_height, 18000);
    }
}
