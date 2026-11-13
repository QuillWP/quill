//! The original's menus as Quill builds them. Items for features that have not
//! been built yet are shown disabled, so the menus already have the
//! original's shape and fill in milestone by milestone.

use crate::commands::{Cmd, shortcut_label};
use crate::docview::DocView;
use crate::menu::{Item, ItemStyle, Menu};
use quill_core::{StoryId, Underline};
use quill_layout::{FontFamilyInfo, FontVariant};
use std::path::PathBuf;

/// What choosing a menu item does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Toolbar zoom preset, custom entry or current-page fit.
    Zoom(u8),
    Cmd(Cmd),
    FontVariant(String, bool, bool),
    /// Font size in half-points.
    Size(u16),
    /// Text color, from the formatting toolbar's Color pop-up.
    Color(quill_core::props::Rgb),
    Style(u16),
    CharacterStyle(u16),
    /// Open a header or footer in the pane.
    Pane(quill_core::StoryId),
    ToggleFormattingToolbar,
    ToggleParagraphToolbar,
    ToggleToolbar,
    ToggleRuler,
    /// Show or hide the Style Sheets palette.
    ToggleStylesPalette,
    ToggleMissingFonts,
    ToggleFullScreen,
    /// Dock the toolbar: 0 top, 1 left, 2 right.
    ToolbarPosition(u8),
    CustomizeToolbar,
    About,
    Licenses,
    /// Open an entry from File > Open Recent.
    Recent(usize),
    /// Take one entry out of the recent list (Open Recent > Remove from List).
    RemoveRecent(usize),
    ClearRecent,
    /// Bring the n-th window to the front.
    Window(usize),
    /// Line up the selected cells' text: an index into CELL_ALIGNMENTS.
    CellAlign(usize),
}

/// Installed language and math coverage families with many separate names.
fn coverage_group(name: &str) -> Option<&'static str> {
    if name == "Noto Sans" || name.starts_with("Noto Sans ") || name.starts_with("NotoSans") {
        Some("Noto Sans")
    } else if name.starts_with("STIX") {
        Some("STIX")
    } else {
        None
    }
}

fn grouped_fonts(
    b: &mut Builder,
    families: &[FontFamilyInfo],
    current: &str,
    chp: quill_core::CharProps,
) -> Vec<Item> {
    let mut items = Vec::new();
    let mut emitted = Vec::new();
    for family in families {
        let Some(group) = coverage_group(&family.name) else {
            items.push(font_item(b, family, current, chp));
            continue;
        };
        if emitted.contains(&group) {
            continue;
        }
        emitted.push(group);
        let mut children = Vec::new();
        for family in families
            .iter()
            .filter(|f| coverage_group(&f.name) == Some(group))
        {
            // Keep one submenu level for the portable font popup too.
            let item = font_item(b, family, current, chp);
            if let Some(styles) = item.submenu {
                children.extend(styles.items.into_iter().map(|mut item| {
                    item.label = format!("{} - {}", family.name, item.label);
                    item
                }));
            } else {
                children.push(item);
            }
        }
        let selected = families.iter().any(|f| {
            coverage_group(&f.name) == Some(group) && f.name.eq_ignore_ascii_case(current)
        });
        items.push(b.item(group, Action::None).checked(selected).submenu(Menu {
            title: group.into(),
            items: children,
        }));
    }
    items
}

pub struct MenuContext<'a> {
    pub view: &'a DocView,
    pub font_families: &'a [FontFamilyInfo],
    pub show_formatting_toolbar: bool,
    pub show_paragraph_toolbar: bool,
    pub show_ruler: bool,
    pub full_screen: bool,
    pub recent: &'a [PathBuf],
    /// Window titles, and which one is this window.
    pub windows: &'a [(String, bool)],
}

struct Builder {
    actions: Vec<Action>,
}

impl Builder {
    fn item(&mut self, label: impl Into<String>, action: Action) -> Item {
        self.actions.push(action);
        Item::new(label, (self.actions.len() - 1) as u32)
    }

    fn cmd(&mut self, label: &str, cmd: Cmd) -> Item {
        self.item(label, Action::Cmd(cmd)).key(shortcut_label(cmd))
    }

    /// An item whose feature is not built yet.
    fn later(&mut self, label: &str, key: &str) -> Item {
        self.item(label, Action::None).key(key).enabled(false)
    }
}

fn variant_label(v: FontVariant) -> &'static str {
    match (v.bold, v.italic) {
        (false, false) => "Normal",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (true, true) => "Bold Italic",
    }
}

fn font_item(
    b: &mut Builder,
    family: &FontFamilyInfo,
    current: &str,
    chp: quill_core::CharProps,
) -> Item {
    let selected = family.name.eq_ignore_ascii_case(current);
    let mut item = b
        .item(
            family.name.clone(),
            Action::FontVariant(family.name.clone(), false, false),
        )
        .checked(selected && !chp.bold && !chp.italic);
    // A symbol font's name, or one in letters the font lacks, would come
    // out as pictures or boxes: it is shown in ordinary text.
    if family.draws_name {
        item = item.font(family.name.clone());
    }
    if family.variants.len() > 1 {
        let items = family
            .variants
            .iter()
            .map(|&v| {
                let item = b
                    .item(
                        variant_label(v),
                        Action::FontVariant(family.name.clone(), v.bold, v.italic),
                    )
                    .checked(selected && chp.bold == v.bold && chp.italic == v.italic);
                if !family.draws_name {
                    return item;
                }
                item.font(family.name.clone()).style(ItemStyle {
                    bold: v.bold,
                    italic: v.italic,
                    ..Default::default()
                })
            })
            .collect();
        item = item.submenu(Menu {
            title: family.name.clone(),
            items,
        });
    }
    item
}

/// The menu bar's menus and the action for each item id, with Tools >
/// Commands' changes.
pub fn build(cx: &MenuContext) -> (Vec<Menu>, Vec<Action>) {
    let (mut menus, actions) = build_menus(cx);
    let mut b = Builder { actions };
    customize(&mut menus, &mut b);
    if cx.view.in_markdown_source() {
        for menu in &mut menus {
            disable_formatting(&mut menu.items, &b.actions);
        }
    }
    (menus, b.actions)
}

/// Grey out every item that would change formatting, submenus included,
/// for Markdown source, which takes none.
fn disable_formatting(items: &mut [Item], actions: &[Action]) {
    for it in items {
        if actions
            .get(it.id as usize)
            .is_some_and(Action::changes_format)
        {
            it.enabled = false;
        }
        if let Some(sub) = &mut it.submenu {
            disable_formatting(&mut sub.items, actions);
        }
    }
}

impl Action {
    /// Whether choosing this changes the document's formatting.
    pub fn changes_format(&self) -> bool {
        match self {
            Action::Cmd(c) => c.changes_format(),
            Action::FontVariant(..)
            | Action::Size(_)
            | Action::Color(_)
            | Action::Style(_)
            | Action::CharacterStyle(_)
            | Action::CellAlign(_) => true,
            _ => false,
        }
    }
}

/// The original's own menus.
fn build_menus(cx: &MenuContext) -> (Vec<Menu>, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let sep = Item::separator;
    let v = cx.view;
    let has_sel = !v.sel.is_empty();
    let chp = v.current_chp();
    let in_table = v.current_pap().in_table;

    // On the Mac, Quit lives in the application menu.
    let mac = cfg!(target_os = "macos");
    let mut app_items = vec![
        b.item("About Quill…", Action::About),
        // (The Mac's menu bar leaves out an item titled just "Licenses…".)
        b.item("Quill Licenses…", Action::Licenses),
    ];
    if mac {
        // Mac apps keep their settings here too, under ⌘,.
        app_items.push(sep());
        app_items.push(b.cmd("Settings…", Cmd::Preferences).key("⌘,"));
        app_items.push(b.cmd("Quit Quill", Cmd::Quit));
    }
    let app = Menu {
        title: if mac { "Quill" } else { "Help" }.into(),
        items: app_items,
    };

    let mut file_items = vec![b.cmd("New", Cmd::New), b.cmd("Open…", Cmd::Open)];
    if crate::settings::with(|settings| settings.recent_files) {
        let shown = crate::settings::with(|s| s.recent_count.clamp(1, 50));
        let name_of = |path: &std::path::Path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        };
        let mut items: Vec<Item> = cx
            .recent
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, path)| b.item(name_of(path), Action::Recent(index)))
            .collect();
        if items.is_empty() {
            items.push(b.item("No Recent Documents", Action::None).enabled(false));
        }
        items.push(sep());
        // Quill remembers more documents than the menu shows. Taking one out
        // here lets the next one in line move up into view.
        let removals: Vec<Item> = cx
            .recent
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, path)| b.item(name_of(path), Action::RemoveRecent(index)))
            .collect();
        items.push(
            b.item("Remove from List", Action::None)
                .enabled(!removals.is_empty())
                .submenu(Menu {
                    title: "Remove from List".into(),
                    items: removals,
                }),
        );
        items.push(
            b.item("Clear Menu", Action::ClearRecent)
                .enabled(!cx.recent.is_empty()),
        );
        file_items.push(b.item("Open Recent", Action::None).submenu(Menu {
            title: "Open Recent".into(),
            items,
        }));
    }
    file_items.extend([
        b.cmd("Close", Cmd::Close),
        b.cmd("Save", Cmd::Save),
        b.cmd("Save As…", Cmd::SaveAs),
        b.cmd("Summary Info…", Cmd::SummaryInfo),
        sep(),
        b.cmd("Page Setup…", Cmd::PageSetup),
        b.cmd("Document Setup…", Cmd::DocumentDialog),
        b.cmd("Print…", Cmd::Print),
        b.cmd("Save as PDF…", Cmd::SavePdf),
        b.cmd("Print Merge…", Cmd::PrintMerge),
    ]);
    let file = Menu {
        title: "File".into(),
        items: file_items,
    };
    let mut file = file;
    if !mac {
        file.items.push(sep());
        file.items.push(b.cmd("Settings…", Cmd::Preferences));
        file.items.push(sep());
        file.items.push(b.cmd("Quit", Cmd::Quit));
    }

    let undo = v.undo_label();
    let can_undo = v.can_undo();
    let repeat = v.repeat_label();
    let can_repeat = !repeat.starts_with("Can't");
    let edit = Menu {
        title: "Edit".into(),
        items: vec![
            b.cmd(&undo, Cmd::Undo).enabled(can_undo),
            b.cmd(&repeat, Cmd::Repeat).enabled(can_repeat),
            sep(),
            b.cmd("Cut", Cmd::Cut).enabled(has_sel),
            b.cmd("Copy", Cmd::Copy).enabled(has_sel),
            b.cmd("Paste", Cmd::Paste),
            b.cmd("Paste as Text", Cmd::PasteText),
            b.cmd("Paste Special…", Cmd::PasteSpecial),
            b.cmd("Clear", Cmd::Clear).enabled(has_sel),
            b.cmd("Select All", Cmd::SelectAll),
            sep(),
            b.cmd("Find…", Cmd::Find),
            b.cmd("Replace…", Cmd::Replace),
            b.item("Advanced", Action::None).submenu(Menu {
                title: "Advanced".into(),
                items: vec![
                    b.cmd("Grep…", Cmd::Grep),
                    b.cmd("Regex Search…", Cmd::RegexSearch),
                    b.cmd("Sed…", Cmd::Sed),
                ],
            }),
            b.cmd("Go To…", Cmd::GoTo),
            b.cmd("Glossary…", Cmd::Glossary),
            b.cmd("Hyperlink…", Cmd::Hyperlink).key("⌘⌥K"),
            b.cmd("Remove Link", Cmd::RemoveHyperlink)
                .enabled(v.has_link()),
        ],
    };

    // Header and footer items: Odd and Even when they differ, First when
    // the first page has its own.
    let mut hf = Vec::new();
    let even = v.doc.props.even_odd_headers;
    let (h, f) = if even {
        ("Odd Header", "Odd Footer")
    } else {
        ("Header", "Footer")
    };
    hf.push(b.item(h, Action::Pane(StoryId::Header)));
    hf.push(b.item(f, Action::Pane(StoryId::Footer)));
    if even {
        hf.push(b.item("Even Header", Action::Pane(StoryId::EvenHeader)));
        hf.push(b.item("Even Footer", Action::Pane(StoryId::EvenFooter)));
    }
    let first_pages = v
        .doc
        .sections()
        .iter()
        .any(|r| v.doc.section_props_at(*r.start()).different_first_page);
    if first_pages {
        hf.push(b.item("First Header", Action::Pane(StoryId::FirstHeader)));
        hf.push(b.item("First Footer", Action::Pane(StoryId::FirstFooter)));
    }
    let view = Menu {
        title: "View".into(),
        items: vec![
            b.cmd("Normal", Cmd::NormalView)
                .checked(!v.page_view && !v.outline),
            b.cmd("Outline", Cmd::OutlineView).checked(v.outline),
            b.cmd("Page Layout", Cmd::PageLayoutView)
                .checked(v.page_view),
            b.item(
                if cx.full_screen {
                    "Exit Full Screen"
                } else {
                    "Enter Full Screen"
                },
                Action::ToggleFullScreen,
            )
            .key("⌃⌘F"),
            sep(),
            b.item("Markdown", Action::None).submenu(Menu {
                title: "Markdown".into(),
                items: vec![
                    b.cmd("Source", Cmd::MarkdownSource)
                        .checked(v.in_markdown_source()),
                    b.cmd("WYSIWYG", Cmd::MarkdownWysiwyg)
                        .checked(!v.in_markdown_source()),
                ],
            }),
            sep(),
            b.item("Standard Toolbar", Action::ToggleToolbar)
                .checked(crate::settings::with(|s| s.view_toolbar)),
            b.item("Formatting Toolbar", Action::ToggleFormattingToolbar)
                .key("⌘⌥R")
                .checked(cx.show_formatting_toolbar),
            b.item("Paragraph Toolbar", Action::ToggleParagraphToolbar)
                .checked(cx.show_paragraph_toolbar),
            b.item("Highlight Missing Fonts", Action::ToggleMissingFonts)
                .checked(crate::settings::with(|s| s.highlight_missing_fonts)),
            b.item("Ruler", Action::ToggleRuler)
                .key("⌘R")
                .checked(cx.show_ruler),
            b.cmd("Print Merge Helper…", Cmd::PrintMergeHelper),
            sep(),
            b.cmd("Zoom In", Cmd::ZoomIn),
            b.cmd("Zoom Out", Cmd::ZoomOut),
            b.cmd("Zoom 100%", Cmd::Zoom100),
            sep(),
            b.cmd(
                if v.show_marks { "Hide ¶" } else { "Show ¶" },
                Cmd::ShowMarks,
            ),
            b.cmd("Show Changes", Cmd::ShowChanges)
                .checked(v.show_changes),
            sep(),
            b.cmd("Footnotes", Cmd::ViewFootnotes).key("⌘⇧⌥S"),
            b.cmd("Annotations…", Cmd::ViewAnnotations),
        ],
    };
    let mut view = view;
    let at = view.items.len() - 2;
    view.items.splice(at..at, hf);

    let insert = Menu {
        title: "Insert".into(),
        items: vec![
            b.cmd("Page Break", Cmd::PageBreak).key("⇧⌅"),
            b.cmd("Section Break", Cmd::SectionBreak).key("⌘⌅"),
            b.cmd("Table…", Cmd::InsertTable),
            b.cmd("Footnote…", Cmd::InsertFootnote).key("⌘E"),
            b.cmd("Annotation…", Cmd::InsertAnnotation),
            b.cmd("Reply to Annotation", Cmd::ReplyToAnnotation),
            b.cmd("Resolve Annotation", Cmd::ResolveAnnotation),
            b.cmd("Delete Annotation", Cmd::DeleteAnnotation),
            sep(),
            b.cmd("Date", Cmd::InsertDate),
            b.cmd("Bullet", Cmd::Bullet),
            sep(),
            b.cmd("Index Entry", Cmd::IndexEntry),
            b.cmd("Index…", Cmd::InsertIndex),
            b.cmd("TOC Entry", Cmd::TocEntry),
            b.cmd("Table of Contents…", Cmd::InsertToc),
            sep(),
            b.cmd("Frame…", Cmd::InsertFrame),
            b.cmd("Picture…", Cmd::InsertPicture),
            b.cmd("Drop Cap…", Cmd::DropCap),
        ],
    };

    let plain =
        !chp.bold && !chp.italic && chp.underline == Underline::None && !chp.outline && !chp.shadow;
    let style = |bold, italic, underline| ItemStyle {
        bold,
        italic,
        underline,
        outline: false,
    };
    let format = Menu {
        title: "Format".into(),
        items: vec![
            b.cmd("Character…", Cmd::CharacterDialog),
            b.cmd("Paragraph…", Cmd::ParagraphDialog),
            b.cmd("Section…", Cmd::SectionDialog),
            b.cmd("Document Setup…", Cmd::DocumentDialog),
            b.cmd("Border…", Cmd::BorderDialog),
            b.cmd("Replace Missing Fonts…", Cmd::ReplaceFonts),
            sep(),
            b.cmd("Table Cells…", Cmd::TableCells).enabled(in_table),
            b.cmd("Table Layout…", Cmd::TableLayout).enabled(in_table),
            b.cmd("Select Table", Cmd::SelectTable).enabled(in_table),
            insert_menu(&mut b).enabled(in_table),
            delete_menu(&mut b).enabled(in_table),
            b.cmd("Merge Cells", Cmd::MergeCells)
                .enabled(cells_to_merge(v)),
            b.cmd("Split Cell", Cmd::SplitCell).enabled(in_table),
            b.cmd("Frame…", Cmd::FrameDialog),
            b.cmd("Picture…", Cmd::PictureProperties)
                .enabled(v.selected_picture().is_some()),
            sep(),
            b.cmd("Paragraph Styles…", Cmd::StyleDialog),
            b.cmd("Character Styles…", Cmd::CharacterStylesDialog),
            b.cmd("Revert To Style", Cmd::RevertToStyle),
            sep(),
            b.cmd("Change Case…", Cmd::ChangeCase).enabled(has_sel),
            b.cmd("Plain Text", Cmd::PlainText).checked(plain),
            b.cmd("Bold", Cmd::Bold)
                .checked(chp.bold)
                .style(style(true, false, false)),
            b.cmd("Italic", Cmd::Italic)
                .checked(chp.italic)
                .style(style(false, true, false)),
            b.cmd("Underline", Cmd::Underline)
                .checked(chp.underline == Underline::Single)
                .style(style(false, false, true)),
        ],
    };

    let font_name = v.doc.font_name(chp.font).to_string();
    let mut font_items = Vec::new();
    for pt in [9u16, 10, 12, 14, 18, 24] {
        let item = b
            .item(format!("{pt} Point"), Action::Size(pt * 2))
            .checked(chp.size == pt * 2)
            // The classic Mac outlines sizes the font draws well; elsewhere
            // that convention means nothing, so the sizes are plain.
            .style(ItemStyle {
                outline: cfg!(target_os = "macos"),
                ..Default::default()
            });
        font_items.push(item);
    }
    font_items.push(sep());
    font_items.push(b.cmd("Increase Font Size", Cmd::LargerFontSize));
    font_items.push(b.cmd("Decrease Font Size", Cmd::SmallerFontSize));
    font_items.push(b.cmd("Default Font…", Cmd::DefaultFont));
    font_items.push(sep());
    font_items.extend(grouped_fonts(&mut b, cx.font_families, &font_name, chp));
    let font = Menu {
        title: "Font".into(),
        items: font_items,
    };

    let tools = Menu {
        title: "Tools".into(),
        items: vec![
            b.cmd("Spelling…", Cmd::Spelling).key("⌘L"),
            b.later("Grammar…", "⌘⇧G"),
            b.cmd("Thesaurus…", Cmd::Thesaurus),
            #[cfg(target_os = "macos")]
            b.cmd("Look Up", Cmd::LookUp).key("⌃⌘D"),
            b.cmd("Hyphenation…", Cmd::Hyphenation),
            b.cmd("Word Count…", Cmd::WordCount),
            sep(),
            b.cmd("Track Changes", Cmd::TrackChanges)
                .checked(v.doc.props.track_changes),
            b.cmd("Review Changes…", Cmd::ReviewChanges),
            b.cmd("Accept All Changes", Cmd::AcceptAllChanges)
                .enabled(v.doc.has_changes()),
            b.cmd("Reject All Changes", Cmd::RejectAllChanges)
                .enabled(v.doc.has_changes()),
            sep(),
            b.cmd("Renumber…", Cmd::Renumber),
            b.cmd("Sort", Cmd::Sort).enabled(has_sel),
            b.cmd("Calculate", Cmd::Calculate).enabled(has_sel),
            sep(),
            b.cmd("Commands…", Cmd::CommandsDialog).key("⌘⇧⌥C"),
        ],
    };

    let mut window_items = vec![
        b.cmd("Show Clipboard", Cmd::ShowClipboard),
        b.cmd("New Window", Cmd::NewWindow),
        // A palette is a window of its own (or docks into one), so it is
        // shown and hidden here rather than in View.
        b.item("Style Sheets", Action::ToggleStylesPalette)
            .checked(crate::settings::with(|s| s.show_styles_palette)),
        sep(),
    ];
    for (i, (title, current)) in cx.windows.iter().enumerate() {
        window_items.push(b.item(title.clone(), Action::Window(i)).checked(*current));
    }
    let window = Menu {
        title: "Window".into(),
        items: window_items,
    };

    let mut menus = vec![file, edit, view, insert, format, font, tools, window];
    if mac {
        menus.insert(0, app);
    } else {
        menus.push(app);
    }
    (menus, b.actions)
}

/// Insert, with what a table can gain.
fn insert_menu(b: &mut Builder) -> Item {
    let items = vec![
        b.cmd("Row Above", Cmd::InsertRowAbove),
        b.cmd("Row Below", Cmd::InsertRowBelow),
        b.cmd("Column Left", Cmd::InsertColumnLeft),
        b.cmd("Column Right", Cmd::InsertColumnRight),
        b.cmd("Cell Before", Cmd::InsertCellBefore),
        b.cmd("Cell After", Cmd::InsertCellAfter),
    ];
    let menu = Menu {
        title: "Insert".into(),
        items,
    };
    b.item("Insert", Action::None).submenu(menu)
}

/// Delete, with what a table can lose.
fn delete_menu(b: &mut Builder) -> Item {
    let items = vec![
        b.cmd("Row", Cmd::DeleteRows),
        b.cmd("Column", Cmd::DeleteColumns),
        b.cmd("Cell", Cmd::DeleteCells),
    ];
    let menu = Menu {
        title: "Delete".into(),
        items,
    };
    b.item("Delete", Action::None).submenu(menu)
}

/// Whether more than one cell of a row is selected, which Merge Cells needs.
fn cells_to_merge(v: &DocView) -> bool {
    v.selected_cells()
        .is_some_and(|block| block.from < block.to || block.rows.len() > 1)
}

/// Editing actions at the current selection/insertion point.
pub fn context_menu(view: &DocView, can_paste: bool, can_paste_text: bool) -> (Menu, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let selected = !view.sel.is_empty();
    let picture = view.selected_picture().is_some();
    // Breaks can't go inside a table, so a table cell's menu leaves them out.
    let in_table = view.current_pap().in_table;
    let insert = Menu {
        title: "Insert".into(),
        items: vec![
            b.cmd("Page Break", Cmd::PageBreak),
            b.cmd("Section Break", Cmd::SectionBreak),
        ],
    };
    // In a table of contents: update it, or turn it into ordinary text.
    let toc = view
        .doc
        .existing_toc()
        .filter(|&end| view.sel.start().para <= end)
        .map(|_| view.doc.props.live_toc.is_some());
    let mut toc_items = Vec::new();
    if let Some(live) = toc {
        toc_items.push(b.cmd("Update Table of Contents", Cmd::UpdateToc));
        if live {
            toc_items.push(b.cmd("Make Editable", Cmd::MakeTocEditable));
        }
        toc_items.push(Item::separator());
    }
    // By an annotation or in the text it is about.
    if view.annotation_here().is_some() {
        toc_items.push(b.cmd("Reply to Annotation", Cmd::ReplyToAnnotation));
        toc_items.push(b.cmd("Resolve Annotation", Cmd::ResolveAnnotation));
        toc_items.push(b.cmd("Delete Annotation", Cmd::DeleteAnnotation));
        toc_items.push(Item::separator());
    }
    // On a picture: its size and crop.
    if picture {
        let cropping = view.cropping_in_place();
        toc_items.push(b.cmd("Crop in Place", Cmd::CropInPlace).checked(cropping));
        toc_items.push(
            b.cmd("Resize in Place", Cmd::ResizeInPlace)
                .checked(!cropping),
        );
        toc_items.push(Item::separator());
    }
    // On a link: change it or take it off. On other selected text: make one.
    // A selection that runs past a link is new text for the dialog, but
    // Remove Hyperlink still takes off what it holds.
    if view.link_target().link != 0 {
        toc_items.push(b.cmd("Edit Hyperlink\u{2026}", Cmd::Hyperlink));
        toc_items.push(b.cmd("Remove Hyperlink", Cmd::RemoveHyperlink));
        toc_items.push(Item::separator());
    } else if selected {
        toc_items.push(b.cmd("Hyperlink\u{2026}", Cmd::Hyperlink));
        if view.has_link() {
            toc_items.push(b.cmd("Remove Hyperlink", Cmd::RemoveHyperlink));
        }
        toc_items.push(Item::separator());
    }
    // The system's Look Up for the word here, first, as Mac apps put it.
    #[cfg(target_os = "macos")]
    if let Some((s, e)) = view.word_at_caret() {
        let word = &view.doc.main.paras[s.para].text[s.offset..e.offset];
        let short: String = word.chars().take(30).collect();
        toc_items.insert(
            0,
            b.cmd(&format!("Look Up \u{201c}{short}\u{201d}"), Cmd::LookUp),
        );
        toc_items.insert(1, Item::separator());
    }
    // In a table: add a row or a column next to the selected cells.
    if in_table {
        toc_items.push(insert_menu(&mut b));
        toc_items.push(delete_menu(&mut b));
        if cells_to_merge(view) {
            toc_items.push(b.cmd("Merge Cells", Cmd::MergeCells));
        }
        toc_items.push(b.cmd("Split Cell", Cmd::SplitCell));
        toc_items.push(Item::separator());
        let now = view.cell_alignment();
        let items = crate::docview::CELL_ALIGNMENTS
            .iter()
            .enumerate()
            .map(|(i, &(name, v, h))| {
                b.item(name, Action::CellAlign(i))
                    .checked(now == Some((v, h)))
            })
            .collect();
        let align = Menu {
            title: "Cell Alignment".into(),
            items,
        };
        toc_items.push(b.item("Cell Alignment", Action::None).submenu(align));
        toc_items.push(Item::separator());
    }
    // Selected tab or comma separated lines can become a table.
    if crate::table_dialogs::convert_plan(view).is_some() {
        toc_items.push(b.cmd("Convert to Table", Cmd::ConvertToTable));
        toc_items.push(Item::separator());
    }
    let mut menu = Menu {
        title: String::new(),
        items: toc_items
            .into_iter()
            .chain([
                b.cmd("Cut", Cmd::Cut).enabled(selected),
                b.cmd("Copy", Cmd::Copy).enabled(selected),
                b.cmd("Paste", Cmd::Paste).enabled(can_paste),
                b.cmd("Paste as Text", Cmd::PasteText)
                    .enabled(can_paste_text),
                Item::separator(),
            ])
            .chain(
                (!in_table)
                    .then(|| {
                        [
                            b.item("Insert", Action::None).submenu(insert),
                            Item::separator(),
                        ]
                    })
                    .into_iter()
                    .flatten(),
            )
            .chain([
                b.cmd("Font…", Cmd::CharacterDialog),
                b.cmd("Paragraph…", Cmd::ParagraphDialog),
            ])
            .collect(),
    };
    if picture {
        menu.items.push(Item::separator());
        menu.items
            .push(b.cmd("Properties\u{2026}", Cmd::PictureProperties));
    }
    (menu, b.actions)
}

/// Tools > Commands' changes: items taken out of menus, and commands added
/// at the end of a menu.
fn customize(menus: &mut [Menu], b: &mut Builder) {
    use crate::commands::{by_name, code_name};
    let (add, remove) = crate::settings::with(|s| (s.menu_add.clone(), s.menu_remove.clone()));
    if add.is_empty() && remove.is_empty() {
        return;
    }
    for m in menus.iter_mut() {
        let title = m.title.clone();
        m.items.retain(|it| match b.actions.get(it.id as usize) {
            Some(Action::Cmd(c)) if !it.separator => !remove
                .iter()
                .any(|(t, n)| *t == title && *n == code_name(*c)),
            _ => true,
        });
        let extra: Vec<Cmd> = add
            .iter()
            .filter(|(t, _)| *t == title)
            .filter_map(|(_, n)| by_name(n))
            .collect();
        if !extra.is_empty() {
            m.items.push(Item::separator());
            for c in extra {
                let item = b.cmd(c.name(), c);
                m.items.push(item);
            }
        }
    }
}

/// The menu titles Tools > Commands can add to.
pub const MENU_TITLES: [&str; 8] = [
    "File", "Edit", "View", "Insert", "Format", "Font", "Tools", "Window",
];

/// Which menu holds a command in the original's own arrangement, before any
/// changes in Tools > Commands.
pub fn default_menu(cmd: Cmd) -> Option<String> {
    thread_local! {
        static MAP: std::cell::RefCell<Option<Vec<(Cmd, String)>>> =
            const { std::cell::RefCell::new(None) };
    }
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        let map = m.get_or_insert_with(|| {
            let view = DocView::new(quill_core::Document::new(), String::new());
            let cx = MenuContext {
                view: &view,
                font_families: &[],
                show_formatting_toolbar: true,
                show_paragraph_toolbar: true,
                show_ruler: true,
                full_screen: false,
                recent: &[],
                windows: &[],
            };
            let (menus, actions) = build_menus(&cx);
            let mut out = Vec::new();
            for menu in &menus {
                for it in &menu.items {
                    if let Some(Action::Cmd(c)) = actions.get(it.id as usize)
                        && !it.separator
                        && MENU_TITLES.contains(&menu.title.as_str())
                    {
                        out.push((*c, menu.title.clone()));
                    }
                }
            }
            out
        });
        map.iter().find(|(c, _)| *c == cmd).map(|(_, t)| t.clone())
    })
}

/// The formatting toolbar's font popup: every font, current one checked.
pub fn font_popup(cx: &MenuContext) -> (Menu, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let current = cx
        .view
        .doc
        .font_name(cx.view.current_chp().font)
        .to_string();
    let chp = cx.view.current_chp();
    let items = grouped_fonts(&mut b, cx.font_families, &current, chp);
    (
        Menu {
            title: String::new(),
            items,
        },
        b.actions,
    )
}

/// The formatting toolbar's size popup: the original's standard sizes.
pub fn size_popup(cx: &MenuContext) -> (Menu, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let current = cx.view.current_chp().size;
    let items = [7u16, 9, 10, 12, 14, 18, 24, 36, 48, 60, 72]
        .iter()
        .map(|&pt| {
            b.item(pt.to_string(), Action::Size(pt * 2))
                .checked(current == pt * 2)
        })
        .collect();
    (
        Menu {
            title: String::new(),
            items,
        },
        b.actions,
    )
}

/// The formatting toolbar's Color pop-up: the original's eight colors, the
/// colors named in Preferences > Colors, the selection's own color if it is
/// none of those, and More Colors…, which opens the Character dialog and
/// its palette.
pub fn color_popup(cx: &MenuContext) -> (Menu, Vec<Action>) {
    use quill_core::props::{Rgb, WORD_COLORS};
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let current = cx.view.current_chp().color;
    let mut listed: Vec<Rgb> = Vec::new();
    let mut items = Vec::new();
    for (name, c) in WORD_COLORS {
        items.push(b.item(name, Action::Color(c)).checked(current == c));
        listed.push(c);
    }
    let named = crate::settings::with(|s| s.named_colors.clone());
    if !named.is_empty() {
        items.push(Item::separator());
    }
    for (rgb, name) in named {
        let c = Rgb(rgb);
        if listed.contains(&c) {
            continue;
        }
        items.push(b.item(name, Action::Color(c)).checked(current == c));
        listed.push(c);
    }
    if !listed.contains(&current) {
        items.push(Item::separator());
        items.push(
            b.item(crate::palette::hex(current), Action::Color(current))
                .checked(true),
        );
    }
    items.push(Item::separator());
    items.push(b.cmd(crate::format_dialogs::OTHER_COLOR, Cmd::CharacterDialog));
    (
        Menu {
            title: String::new(),
            items,
        },
        b.actions,
    )
}

/// The menu that drops from the toolbar's last button: where the toolbar
/// sits, and Customize. The position is the same setting Preferences shows.
pub fn toolbar_menu(position: u8) -> (Menu, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let items = vec![
        b.item("Top", Action::ToolbarPosition(0))
            .checked(position == 0),
        b.item("Left", Action::ToolbarPosition(1))
            .checked(position == 1),
        b.item("Right", Action::ToolbarPosition(2))
            .checked(position == 2),
        Item::separator(),
        b.item("Customize…", Action::CustomizeToolbar),
    ];
    (
        Menu {
            title: String::new(),
            items,
        },
        b.actions,
    )
}

/// The ruler's style popup: styles in use in the document plus Normal and
/// the headings, as the original lists "document styles".
pub fn style_popup(cx: &MenuContext) -> (Menu, Vec<Action>) {
    let mut b = Builder {
        actions: vec![Action::None],
    };
    let doc = &cx.view.doc;
    let current = cx.view.current_pap().style;
    let mut used: Vec<u16> = doc.main.paras.iter().map(|p| p.pap.style).collect();
    used.extend(
        doc.styles
            .styles
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.builtin || s.name.starts_with("heading "))
            .map(|(i, _)| i as u16),
    );
    used.push(0);
    used.sort();
    used.dedup();
    let items = used
        .iter()
        .map(|&id| {
            b.item(doc.styles.get(id).display_name(), Action::Style(id))
                .checked(id == current)
        })
        .collect();
    (
        Menu {
            title: String::new(),
            items,
        },
        b.actions,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_menu_offers_table_changes_and_merge_with_cells_selected() {
        use quill_core::{Position, Selection};
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.doc.begin("t", view.sel);
        view.doc
            .insert_table(quill_core::StoryId::Main, Position::new(0, 0), 2, 2, 2000);
        let sel = view.sel;
        view.doc.commit(sel);
        view.sel = Selection::caret(Position::new(0, 0));
        let label = |view: &DocView, name: &str| {
            let (menu, _) = context_menu(view, false, false);
            menu.items.iter().any(|i| i.label == name)
        };
        assert!(label(&view, "Delete"));
        assert!(label(&view, "Split Cell"));
        assert!(!label(&view, "Merge Cells"));
        view.sel = Selection::new(Position::new(0, 0), Position::new(1, 0));
        assert!(label(&view, "Merge Cells"));
        let (menu, actions) = context_menu(&view, false, false);
        let delete = menu.items.iter().find(|i| i.label == "Delete").unwrap();
        let items = &delete.submenu.as_ref().unwrap().items;
        assert_eq!(
            actions[items[1].id as usize],
            Action::Cmd(Cmd::DeleteColumns)
        );
    }

    #[test]
    fn context_menu_offers_link_items_on_links_and_selections() {
        use quill_core::{Link, Position, Selection};
        let labels = |view: &DocView| -> Vec<String> {
            let (menu, _) = context_menu(view, false, false);
            let links = [
                "Edit Hyperlink\u{2026}",
                "Hyperlink\u{2026}",
                "Remove Hyperlink",
            ];
            menu.items
                .iter()
                .filter(|i| links.contains(&i.label.as_str()))
                .map(|i| i.label.clone())
                .collect()
        };
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.type_text("plain linked plain");
        // Nothing selected and no link: nothing to offer.
        view.sel = Selection::caret(Position::new(0, 2));
        assert!(labels(&view).is_empty());
        view.sel = Selection::new(Position::new(0, 0), Position::new(0, 5));
        assert_eq!(labels(&view), ["Hyperlink\u{2026}"]);
        let target = crate::docview::LinkTarget {
            start: Position::new(0, 6),
            end: Position::new(0, 12),
            text: "linked".into(),
            link: 0,
            within: 0,
            color: quill_core::link::BROWSER_BLUE,
        };
        view.apply_link(
            &target,
            "linked",
            Link::new("https://example.com"),
            quill_core::link::BROWSER_BLUE,
        );
        // The insertion point in the link, and a selection inside it.
        let on_link = ["Edit Hyperlink\u{2026}", "Remove Hyperlink"];
        view.sel = Selection::caret(Position::new(0, 8));
        assert_eq!(labels(&view), on_link);
        view.sel = Selection::new(Position::new(0, 7), Position::new(0, 9));
        assert_eq!(labels(&view), on_link);
        // A selection running past the link.
        view.sel = Selection::new(Position::new(0, 2), Position::new(0, 15));
        assert_eq!(labels(&view), ["Hyperlink\u{2026}", "Remove Hyperlink"]);
    }

    #[test]
    fn context_actions_follow_selection_and_clipboard_formats() {
        let mut view = DocView::new(quill_core::Document::new(), String::new());
        view.type_text("selected text");
        // Cut, Copy, Paste and Paste as Text follow Look Up, on the Mac.
        let cut = |m: &Menu| m.items.iter().position(|i| i.label == "Cut").unwrap();
        let (menu, _) = context_menu(&view, false, false);
        let at = cut(&menu);
        assert!(menu.items[at..at + 4].iter().all(|i| !i.enabled));
        view.sel = quill_core::Selection::new(
            quill_core::Position::new(0, 0),
            quill_core::Position::new(0, 8),
        );
        let (menu, actions) = context_menu(&view, true, false);
        #[cfg(target_os = "macos")]
        assert_eq!(menu.items[0].label, "Look Up \u{201c}selected\u{201d}");
        let at = cut(&menu);
        assert!(menu.items[at..at + 3].iter().all(|i| i.enabled));
        assert!(!menu.items[at + 3].enabled); // A picture cannot be pasted as text.
        assert_eq!(actions[menu.items[at].id as usize], Action::Cmd(Cmd::Cut));
        let insert = menu
            .items
            .iter()
            .find(|i| i.label == "Insert")
            .unwrap()
            .submenu
            .as_ref()
            .unwrap();
        assert_eq!(
            actions[insert.items[0].id as usize],
            Action::Cmd(Cmd::PageBreak)
        );
        assert_eq!(
            actions[insert.items[1].id as usize],
            Action::Cmd(Cmd::SectionBreak)
        );
        assert!(
            menu.items
                .iter()
                .filter(|i| !i.separator)
                .all(|i| !i.checkable)
        );
    }

    #[test]
    fn settings_and_application_menus_follow_the_host_platform() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menus, actions) = build_menus(&cx);
        let settings: Vec<_> = menus
            .iter()
            .flat_map(|m| {
                m.items.iter().filter_map(|i| {
                    (actions[i.id as usize] == Action::Cmd(Cmd::Preferences))
                        .then_some(m.title.as_str())
                })
            })
            .collect();
        if cfg!(target_os = "macos") {
            assert_eq!(menus[0].title, "Quill");
            assert_eq!(settings, ["Quill"]);
        } else {
            assert_eq!(menus[0].title, "File");
            assert_eq!(menus.last().unwrap().title, "Help");
            assert!(menus.iter().all(|m| m.title != "Quill"));
            assert_eq!(settings, ["File"]);
        }
    }

    #[test]
    fn the_licenses_item_sits_under_about_in_the_help_or_application_menu() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menus, actions) = build_menus(&cx);
        let menu = if cfg!(target_os = "macos") {
            &menus[0]
        } else {
            menus.last().unwrap()
        };
        let actions_in_menu: Vec<_> = menu
            .items
            .iter()
            .map(|i| actions[i.id as usize].clone())
            .collect();
        assert_eq!(actions_in_menu[0], Action::About);
        assert_eq!(actions_in_menu[1], Action::Licenses);
    }

    #[test]
    fn coverage_groups_preserve_exact_font_and_style_actions() {
        let families = [
            "Arial",
            "Noto Sans Armenian",
            "NotoSansBamum",
            "STIXGeneral",
            "STIXIntegralsD",
            "Noto Serif",
        ]
        .map(|name| FontFamilyInfo {
            name: name.into(),
            draws_name: false,
            variants: vec![
                FontVariant {
                    bold: false,
                    italic: false,
                },
                FontVariant {
                    bold: true,
                    italic: false,
                },
            ],
        });
        let mut b = Builder { actions: vec![] };
        let items = grouped_fonts(
            &mut b,
            &families,
            "NotoSansBamum",
            quill_core::CharProps::default(),
        );
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["Arial", "Noto Sans", "STIX", "Noto Serif"]
        );
        let noto = &items[1];
        assert!(noto.checked);
        let children = &noto.submenu.as_ref().unwrap().items;
        assert_eq!(children.len(), 4);
        assert!(children.iter().all(|i| i.submenu.is_none()));
        assert_eq!(
            b.actions[children[3].id as usize],
            Action::FontVariant("NotoSansBamum".into(), true, false)
        );
        let single = grouped_fonts(
            &mut b,
            &families[..1],
            "Arial",
            quill_core::CharProps::default(),
        );
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].label, "Arial");
    }

    #[test]
    fn font_families_get_only_their_installed_variants() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let families = vec![
            FontFamilyInfo {
                name: "Arial".into(),
                draws_name: true,
                variants: vec![
                    FontVariant {
                        bold: false,
                        italic: false,
                    },
                    FontVariant {
                        bold: true,
                        italic: false,
                    },
                    FontVariant {
                        bold: false,
                        italic: true,
                    },
                ],
            },
            FontFamilyInfo {
                name: "Wingdings".into(),
                draws_name: false,
                variants: vec![FontVariant {
                    bold: false,
                    italic: false,
                }],
            },
        ];
        let cx = MenuContext {
            view: &view,
            font_families: &families,
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menu, actions) = font_popup(&cx);
        let arial = &menu.items[0];
        let variants = arial.submenu.as_ref().unwrap();
        assert_eq!(
            variants
                .items
                .iter()
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["Normal", "Bold", "Italic"]
        );
        assert_eq!(
            actions[arial.id as usize],
            Action::FontVariant("Arial".into(), false, false)
        );
        assert!(menu.items[1].submenu.is_none());
        // Arial's name is shown in Arial; Wingdings' in ordinary text.
        assert_eq!(arial.font.as_deref(), Some("Arial"));
        assert_eq!(variants.items[1].font.as_deref(), Some("Arial"));
        assert_eq!(menu.items[1].font, None);
    }

    #[test]
    fn formatting_items_are_greyed_out_in_markdown_source() {
        let (doc, _) = quill_formats::markdown::read("Some text\n", None);
        let mut view = DocView::new(doc, String::new());
        fn enabled(view: &DocView, title: &str, label: &str) -> bool {
            let cx = MenuContext {
                view,
                font_families: &[],
                show_formatting_toolbar: true,
                show_paragraph_toolbar: true,
                show_ruler: true,
                full_screen: false,
                recent: &[],
                windows: &[],
            };
            let (menus, _) = build(&cx);
            let menu = menus.iter().find(|m| m.title == title).unwrap();
            menu.items
                .iter()
                .find(|i| i.label == label)
                .unwrap()
                .enabled
        }
        assert!(enabled(&view, "Format", "Bold"));
        assert!(enabled(&view, "Font", "12 Point"));
        view.show_markdown_source();
        assert!(!enabled(&view, "Format", "Bold"));
        assert!(!enabled(&view, "Format", "Character…"));
        assert!(!enabled(&view, "Font", "12 Point"));
        // Editing and the switch back are still there.
        assert!(enabled(&view, "Edit", "Select All"));
        assert!(enabled(&view, "Font", "Default Font…"));
    }

    #[test]
    fn the_color_pop_up_offers_the_eight_colors_the_selections_own_and_more() {
        use quill_core::props::Rgb;
        let mut doc = quill_core::text::from_plain_text("Some text");
        doc.main.paras[0].format_range(0, 9, &mut |c| c.color = Rgb(0x336699));
        let mut view = DocView::new(doc, String::new());
        view.sel = quill_core::Selection::caret(quill_core::Position::new(0, 4));
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menu, actions) = color_popup(&cx);
        let labels: Vec<&str> = menu
            .items
            .iter()
            .filter(|i| !i.separator)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(
            &labels[..8],
            [
                "Black", "Blue", "Cyan", "Green", "Magenta", "Red", "Yellow", "White"
            ]
        );
        assert_eq!(
            &labels[8..],
            ["#336699", crate::format_dialogs::OTHER_COLOR]
        );
        let own = menu.items.iter().find(|i| i.label == "#336699").unwrap();
        assert!(own.checked);
        assert_eq!(actions[own.id as usize], Action::Color(Rgb(0x336699)));
        assert!(Action::Color(Rgb::BLACK).changes_format());
        // Choosing a color sets the selection's.
        view.sel = quill_core::Selection::new(
            quill_core::Position::new(0, 0),
            quill_core::Position::new(0, 4),
        );
        view.set_color(Rgb(0xFF0000));
        assert_eq!(view.doc.main.paras[0].chp_of_char(0).color, Rgb(0xFF0000));
        assert_eq!(view.doc.main.paras[0].chp_of_char(5).color, Rgb(0x336699));
    }

    #[test]
    fn recent_documents_are_paths_in_a_clearable_submenu() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let recent = vec![PathBuf::from("/Documents/First.qwp")];
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &recent,
            windows: &[],
        };
        let (menus, actions) = build_menus(&cx);
        let file = menus.iter().find(|menu| menu.title == "File").unwrap();
        let submenu = file
            .items
            .iter()
            .find(|item| item.label == "Open Recent")
            .and_then(|item| item.submenu.as_ref())
            .unwrap();
        assert_eq!(submenu.title, "Open Recent");
        assert_eq!(submenu.items[0].label, "First.qwp");
        assert_eq!(actions[submenu.items[0].id as usize], Action::Recent(0));
        assert_eq!(
            actions[submenu.items.last().unwrap().id as usize],
            Action::ClearRecent
        );
        let remove = submenu
            .items
            .iter()
            .find(|i| i.label == "Remove from List")
            .and_then(|i| i.submenu.as_ref())
            .expect("a Remove from List submenu");
        assert_eq!(remove.items[0].label, "First.qwp");
        assert_eq!(
            actions[remove.items[0].id as usize],
            Action::RemoveRecent(0)
        );
    }

    #[test]
    fn legacy_font_and_help_entries_are_not_dead_ends() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menus, actions) = build_menus(&cx);
        let font = menus.iter().find(|menu| menu.title == "Font").unwrap();
        let default_font = font
            .items
            .iter()
            .find(|item| item.label == "Default Font…")
            .unwrap();

        assert_eq!(
            actions[default_font.id as usize],
            Action::Cmd(Cmd::DefaultFont)
        );
        assert!(!font.items.iter().any(|item| item.label == "Other…"));

        let window = menus.iter().find(|menu| menu.title == "Window").unwrap();
        assert!(!window.items.iter().any(|item| item.label == "Help…"));
    }

    #[test]
    fn styles_are_managed_from_format_and_the_palette_is_in_window() {
        let view = DocView::new(quill_core::Document::new(), String::new());
        let cx = MenuContext {
            view: &view,
            font_families: &[],
            show_formatting_toolbar: true,
            show_paragraph_toolbar: true,
            show_ruler: true,
            full_screen: false,
            recent: &[],
            windows: &[],
        };
        let (menus, actions) = build_menus(&cx);
        let menu = |title: &str| menus.iter().find(|m| m.title == title).unwrap();
        let action = |title: &str, label: &str| {
            menu(title)
                .items
                .iter()
                .find(|i| i.label == label)
                .map(|i| actions[i.id as usize].clone())
        };
        assert!(!menus.iter().any(|m| m.title == "Character Styles"));
        assert_eq!(
            action("Format", "Paragraph Styles…"),
            Some(Action::Cmd(Cmd::StyleDialog))
        );
        assert_eq!(
            action("Format", "Character Styles…"),
            Some(Action::Cmd(Cmd::CharacterStylesDialog))
        );
        assert_eq!(
            action("Window", "Style Sheets"),
            Some(Action::ToggleStylesPalette)
        );
        assert_eq!(action("View", "Style Sheets"), None);
        assert_eq!(
            default_menu(Cmd::CharacterStylesDialog).as_deref(),
            Some("Format")
        );
    }
}
