//! Commands and the original's default key bindings.
//!
//! Names match the original's own command names, so the menus and the keymap
//! talk about the same things.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Defines `Cmd` and `ALL` together so the list of every command can never
/// miss one.
macro_rules! commands {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Cmd {
            $($(#[$doc])* $name,)*
        }

        /// Every command, for scripting and the toolbar customizer.
        pub const ALL: &[Cmd] = &[$(Cmd::$name,)*];
    };
}

commands! {
    // File
    New,
    Open,
    Close,
    Save,
    SaveAs,
    SummaryInfo,
    PrintPreview,
    PageSetup,
    Print,
    SavePdf,
    Quit,
    // Edit
    Undo,
    Redo,
    Repeat,
    Cut,
    Copy,
    Paste,
    PasteSpecial,
    Clear,
    SelectAll,
    Find,
    Replace,
    FindAgain,
    GoTo,
    Grep,
    RegexSearch,
    Sed,
    GoBack,
    Glossary,
    InsertGlossaryEntry,
    // View
    ShowMarks,
    ShowChanges,
    NormalView,
    PageLayoutView,
    OutlineView,
    MarkdownSource,
    MarkdownWysiwyg,
    PromoteHeading,
    DemoteHeading,
    MakeBodyText,
    MoveHeadingUp,
    MoveHeadingDown,
    ExpandSubtext,
    CollapseSubtext,
    ShowHeading1,
    ShowHeading2,
    ShowHeading3,
    ShowHeading4,
    ShowHeading5,
    ShowHeading6,
    ShowHeading7,
    ShowHeading8,
    ShowHeading9,
    ShowAllHeadings,
    ShowBodyText,
    FirstLineOnly,
    ShowFormatting,
    OutlinePrefix,
    SplitWindow,
    NewWindow,
    ShowClipboard,
    // Insert
    InsertDate,
    InsertTime,
    Bullet,
    InsertPicture,
    InsertFrame,
    DropCap,
    FrameDialog,
    // Tools
    WordCount,
    TrackChanges,
    ReviewChanges,
    AcceptChange,
    RejectChange,
    NextChange,
    PreviousChange,
    AcceptAllChanges,
    RejectAllChanges,
    ChangeCase,
    Uppercase,
    Lowercase,
    TitleCase,
    SentenceCase,
    ToggleCase,
    CycleCase,
    Sort,
    SortDescending,
    Calculate,
    ZoomIn,
    ZoomOut,
    Zoom100,
    Renumber,
    RepaginateNow,
    // Dialogs
    CharacterDialog,
    ParagraphDialog,
    SectionDialog,
    DocumentDialog,
    BorderDialog,
    /// Borders and shading for the cell the insertion point is in (the
    /// cell right-click menu).
    BorderCells,
    InsertTable,
    TableCells,
    TableLayout,
    SelectTable,
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    InsertCellBefore,
    InsertCellAfter,
    DeleteRows,
    DeleteColumns,
    DeleteCells,
    MergeCells,
    SplitCell,
    ReplaceFonts,
    ConvertToTable,
    UpdateToc,
    MakeTocEditable,
    StyleDialog,
    CharacterStylesDialog,
    Preferences,
    DefaultFont,
    ToolbarMenu,
    // Character formatting
    Bold,
    Italic,
    Underline,
    WordUnderline,
    DoubleUnderline,
    DottedUnderline,
    Strikethru,
    Outline,
    Shadow,
    SmallCaps,
    AllCaps,
    Hidden,
    Superscript,
    Subscript,
    PlainText,
    FontSizeUp,
    FontSizeDown,
    LargerFontSize,
    SmallerFontSize,
    // Paragraph formatting
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    DoubleSpace,
    SpaceBefore12,
    NormalParagraph,
    RevertToStyle,
    ClearFormatting,
    FirstLineIndent,
    HangingIndent,
    Nest,
    Unnest,
    // Movement (extend with Shift)
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    ParaUp,
    ParaDown,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    PageUp,
    PageDown,
    // Typing
    Backspace,
    DeleteForward,
    DeletePrevWord,
    DeleteNextWord,
    NewParagraph,
    LineBreak,
    PageBreak,
    SectionBreak,
    InsertFootnote,
    ViewFootnotes,
    /// Format > Picture…: the selected picture's size and crop.
    PictureProperties,
    /// Right-click on a picture: its handles crop, without Control.
    CropInPlace,
    /// Right-click on a picture: its handles scale again.
    ResizeInPlace,
    /// Edit > Hyperlink…: make or edit a link.
    Hyperlink,
    /// Take the link off the text the cursor is in or the selection holds.
    RemoveHyperlink,
    InsertAnnotation,
    ReplyToAnnotation,
    ResolveAnnotation,
    DeleteAnnotation,
    ViewAnnotations,
    PrintMerge,
    PrintMergeHelper,
    Spelling,
    Hyphenation,
    Thesaurus,
    LookUp,
    InsertToc,
    InsertIndex,
    IndexEntry,
    TocEntry,
    Tab,
    /// Shift+Tab: the previous table cell.
    PrevCell,
    /// Option+Tab: a tab character, even in a table.
    TabChar,
    NonbreakingSpace,
    OptionalHyphen,
    NonbreakingHyphen,
    /// Paste using only text and the insertion point's formatting.
    PasteText,
}

impl Cmd {
    /// Whether the command changes formatting: the Format, Font and style
    /// commands and the dialogs they open. Markdown source is plain text in
    /// one font, so these do nothing there.
    pub fn changes_format(self) -> bool {
        use Cmd::*;
        matches!(
            self,
            CharacterDialog
                | ParagraphDialog
                | SectionDialog
                | DocumentDialog
                | BorderDialog
                | BorderCells
                | TableCells
                | TableLayout
                | InsertRowAbove
                | InsertRowBelow
                | InsertColumnLeft
                | InsertColumnRight
                | InsertCellBefore
                | InsertCellAfter
                | DeleteRows
                | DeleteColumns
                | DeleteCells
                | MergeCells
                | SplitCell
                | ReplaceFonts
                | FrameDialog
                | PictureProperties
                | DropCap
                | StyleDialog
                | CharacterStylesDialog
                | Bold
                | Italic
                | Underline
                | WordUnderline
                | DoubleUnderline
                | DottedUnderline
                | Strikethru
                | Outline
                | Shadow
                | SmallCaps
                | AllCaps
                | Hidden
                | Superscript
                | Subscript
                | PlainText
                | FontSizeUp
                | FontSizeDown
                | LargerFontSize
                | SmallerFontSize
                | AlignLeft
                | AlignCenter
                | AlignRight
                | Justify
                | DoubleSpace
                | SpaceBefore12
                | NormalParagraph
                | RevertToStyle
                | ClearFormatting
                | FirstLineIndent
                | HangingIndent
                | Nest
                | Unnest
        )
    }

    /// The command's name as the original lists it.
    pub fn name(self) -> &'static str {
        use Cmd::*;
        match self {
            New => "New",
            Open => "Open…",
            Close => "Close",
            Save => "Save",
            SaveAs => "Save As…",
            SummaryInfo => "Summary Info…",
            PrintPreview => "Print Preview…",
            PageSetup => "Page Setup…",
            Print => "Print…",
            SavePdf => "Save as PDF…",
            CharacterDialog => "Character…",
            ParagraphDialog => "Paragraph…",
            SectionDialog => "Section…",
            DocumentDialog => "Document Setup…",
            BorderDialog => "Border…",
            BorderCells => "Borders and Shading…",
            InsertTable => "Table…",
            TableCells => "Table Cells…",
            TableLayout => "Table Layout…",
            SelectTable => "Select Table",
            InsertRowAbove => "Insert Row Above",
            InsertRowBelow => "Insert Row Below",
            InsertColumnLeft => "Insert Column Left",
            InsertColumnRight => "Insert Column Right",
            InsertCellBefore => "Insert Cell Before",
            InsertCellAfter => "Insert Cell After",
            DeleteRows => "Delete Rows",
            DeleteColumns => "Delete Columns",
            DeleteCells => "Delete Cells",
            MergeCells => "Merge Cells",
            SplitCell => "Split Cell",
            ReplaceFonts => "Replace Missing Fonts…",
            ConvertToTable => "Convert to Table",
            UpdateToc => "Update Table of Contents",
            MakeTocEditable => "Make Table of Contents Editable",
            StyleDialog => "Paragraph Styles…",
            CharacterStylesDialog => "Character Styles…",
            DefaultFont => "Default Font…",
            ToolbarMenu => "Toolbar Menu",
            Preferences => "Preferences…",
            Quit => "Quit",
            Undo => "Undo",
            Redo => "Redo",
            Repeat => "Repeat",
            Cut => "Cut",
            Copy => "Copy",
            Paste => "Paste",
            PasteText => "Paste as Text",
            PasteSpecial => "Paste Special…",
            Clear => "Clear",
            SelectAll => "Select All",
            Find => "Find…",
            Replace => "Replace…",
            FindAgain => "Find Again",
            GoTo => "Go To…",
            Grep => "Grep…",
            RegexSearch => "Regex Search…",
            Sed => "Sed…",
            GoBack => "Go Back",
            Glossary => "Glossary…",
            InsertGlossaryEntry => "Insert Glossary Entry",
            SplitWindow => "Split Window",
            NewWindow => "New Window",
            ShowClipboard => "Show Clipboard",
            InsertDate => "Date",
            InsertTime => "Time",
            Bullet => "Bullet",
            InsertPicture => "Picture…",
            InsertFrame => "Frame (Insert)",
            DropCap => "Drop Cap…",
            FrameDialog => "Frame…",
            WordCount => "Word Count…",
            TrackChanges => "Track Changes",
            ReviewChanges => "Review Changes…",
            AcceptChange => "Accept Change",
            RejectChange => "Reject Change",
            NextChange => "Next Change",
            PreviousChange => "Previous Change",
            AcceptAllChanges => "Accept All Changes",
            RejectAllChanges => "Reject All Changes",
            ChangeCase => "Change Case…",
            Uppercase => "Uppercase",
            Lowercase => "Lowercase",
            TitleCase => "Title Case",
            SentenceCase => "Sentence Case",
            CycleCase => "Cycle Case",
            ToggleCase => "Toggle Case",
            Sort => "Sort",
            SortDescending => "Sort Descending",
            Calculate => "Calculate",
            ZoomIn => "Zoom In",
            ZoomOut => "Zoom Out",
            Zoom100 => "Zoom 100%",
            Renumber => "Renumber…",
            RepaginateNow => "Repaginate Now",
            ShowMarks => "Show/Hide ¶",
            ShowChanges => "Show Changes",
            NormalView => "Normal",
            PageLayoutView => "Page Layout",
            OutlineView => "Outline",
            MarkdownSource => "Source",
            MarkdownWysiwyg => "WYSIWYG",
            PromoteHeading => "Promote Heading",
            DemoteHeading => "Demote Heading",
            MakeBodyText => "Make Body Text",
            MoveHeadingUp => "Move Heading Up",
            MoveHeadingDown => "Move Heading Down",
            ExpandSubtext => "Expand Subtext",
            CollapseSubtext => "Collapse Subtext",
            ShowHeading1 => "Show Heading 1",
            ShowHeading2 => "Show Heading 2",
            ShowHeading3 => "Show Heading 3",
            ShowHeading4 => "Show Heading 4",
            ShowHeading5 => "Show Heading 5",
            ShowHeading6 => "Show Heading 6",
            ShowHeading7 => "Show Heading 7",
            ShowHeading8 => "Show Heading 8",
            ShowHeading9 => "Show Heading 9",
            ShowAllHeadings => "Show All Headings",
            ShowBodyText => "Show Body Text",
            FirstLineOnly => "Show First Line Only",
            ShowFormatting => "Show Formatting",
            OutlinePrefix => "Outline Command Prefix",
            Bold => "Bold",
            Italic => "Italic",
            Underline => "Underline",
            WordUnderline => "Word Underline",
            DoubleUnderline => "Double Underline",
            DottedUnderline => "Dotted Underline",
            Strikethru => "Strikethru",
            Outline => "Outline (Format)",
            Shadow => "Shadow",
            SmallCaps => "Small Caps",
            AllCaps => "All Caps",
            Hidden => "Hidden Text",
            Superscript => "Superscript 3 pt",
            Subscript => "Subscript 2 pt",
            PlainText => "Plain Text",
            FontSizeUp => "Up",
            FontSizeDown => "Down",
            LargerFontSize => "Larger Font Size",
            SmallerFontSize => "Smaller Font Size",
            AlignLeft => "Paragraph Aligned Left",
            AlignCenter => "Centered",
            AlignRight => "Paragraph Aligned Right",
            Justify => "Justified",
            DoubleSpace => "Line Spacing: Double",
            SpaceBefore12 => "Space Before ¶: 12 points",
            NormalParagraph => "Normal Paragraph",
            RevertToStyle => "Revert To Style",
            ClearFormatting => "Clear Formatting",
            FirstLineIndent => "First Line Indent",
            HangingIndent => "Hanging Indent",
            Nest => "Nest Paragraph",
            Unnest => "Unnest Paragraph",
            Left => "Move to Previous Character",
            Right => "Move to Next Character",
            Up => "Move to Previous Line",
            Down => "Move to Next Line",
            WordLeft => "Move to Previous Word",
            WordRight => "Move to Next Word",
            ParaUp => "Move to Previous Paragraph",
            ParaDown => "Move to Next Paragraph",
            LineStart => "Move to Start of Line",
            LineEnd => "Move to End of Line",
            DocStart => "Move to Start of Document",
            DocEnd => "Move to End of Document",
            PageUp => "Scroll Screen Up",
            PageDown => "Scroll Screen Down",
            Backspace => "Backspace",
            DeleteForward => "Delete Forward",
            DeletePrevWord => "Delete Previous Word",
            DeleteNextWord => "Delete Next Word",
            NewParagraph => "New Paragraph",
            LineBreak => "Line Break",
            PageBreak => "Page Break",
            SectionBreak => "Section Break",
            InsertFootnote => "Footnote…",
            PictureProperties => "Picture…",
            CropInPlace => "Crop in Place",
            ResizeInPlace => "Resize in Place",
            Hyperlink => "Hyperlink…",
            RemoveHyperlink => "Remove Link",
            InsertAnnotation => "Annotation…",
            ReplyToAnnotation => "Reply to Annotation",
            ResolveAnnotation => "Resolve Annotation",
            DeleteAnnotation => "Delete Annotation",
            PrintMerge => "Print Merge…",
            PrintMergeHelper => "Print Merge Helper…",
            ViewAnnotations => "Annotations…",
            Spelling => "Spelling…",
            Hyphenation => "Hyphenation…",
            Thesaurus => "Thesaurus…",
            LookUp => "Look Up",
            InsertToc => "Table of Contents…",
            InsertIndex => "Index…",
            IndexEntry => "Index Entry",
            TocEntry => "TOC Entry",
            ViewFootnotes => "Footnotes",
            Tab => "Insert Tab",
            PrevCell => "Previous Cell",
            TabChar => "Tab Character",
            NonbreakingSpace => "Insert Nonbreaking Space",
            OptionalHyphen => "Insert Optional Hyphen",
            NonbreakingHyphen => "Insert Nonbreaking Hyphen",
        }
    }

    /// Movement commands extend the selection when Shift is held.
    pub fn is_movement(self) -> bool {
        use Cmd::*;
        matches!(
            self,
            Left | Right
                | Up
                | Down
                | WordLeft
                | WordRight
                | ParaUp
                | ParaDown
                | LineStart
                | LineEnd
                | DocStart
                | DocEnd
        )
    }
}

/// Modifier keys in Mac terms. On Windows and Linux, Control plays the
/// part of Command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub cmd: bool,
    pub shift: bool,
    pub opt: bool,
    pub ctrl: bool,
}

impl Mods {
    pub fn from_winit(m: ModifiersState) -> Mods {
        if cfg!(target_os = "macos") {
            Mods {
                cmd: m.super_key(),
                shift: m.shift_key(),
                opt: m.alt_key(),
                ctrl: m.control_key(),
            }
        } else {
            Mods {
                cmd: m.control_key(),
                shift: m.shift_key(),
                opt: m.alt_key(),
                ctrl: false,
            }
        }
    }
}

/// A key as the keymap sees it: a named key or a lower-case character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum K {
    Ch(char),
    Named(NamedKey),
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
        InsertCellBefore => "Inserts a cell before the selected cells, in those rows only.",
        InsertCellAfter => "Inserts a cell after the selected cells, in those rows only.",
        DeleteRows => "Deletes the rows holding the selected cells.",
        DeleteColumns => "Deletes the columns holding the selected cells.",
        DeleteCells => "Deletes the selected cells, moving the rest of each row left.",
        MergeCells => "Merges the selected cells into one, across and down.",
        SplitCell => "Splits each selected cell in two.",
        ReplaceFonts => {
            "Chooses installed fonts for the fonts this document uses that are missing."
        }
        ConvertToTable => {
            "Turns the selected tab or comma separated lines into a table, one line to a row."
        }
        LookUp => {
            "Shows the system's definitions and thesaurus for the word at the insertion point."
        }
        ShowChanges => "Shows tracked changes marked up, or the text with them accepted (toggle).",
        MarkdownSource => {
            "Shows the document as Markdown text to read and edit. Switching back without editing the text gives the document back exactly."
        }
        MarkdownWysiwyg => {
            "Shows the document formatted, as it will look; the Markdown text is read back into it."
        }
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

/// A command by its name in code ("PrintMerge"), as settings keep it.
pub fn by_name(name: &str) -> Option<Cmd> {
    ALL.iter().find(|c| format!("{c:?}") == name).copied()
}

pub fn code_name(cmd: Cmd) -> String {
    format!("{cmd:?}")
}

/// Find the command bound to a key press. Shift is part of the binding for
/// non-movement commands; for movement it means "extend".
pub fn lookup(key: &Key, physical_char: Option<char>, mods: Mods) -> Option<Cmd> {
    use Cmd::*;
    use NamedKey as N;
    let k = match key {
        Key::Named(n) => K::Named(*n),
        Key::Character(s) => {
            // With Command held, use the unshifted key so ⌘⇧> works the
            // same as ⌘⇧. regardless of keyboard layout quirks.
            let c = physical_char.or_else(|| s.chars().next())?;
            K::Ch(c.to_ascii_lowercase())
        }
        _ => return None,
    };
    let (cmd, shift, opt) = (mods.cmd, mods.shift, mods.opt);
    let found = match (k, cmd, shift, opt) {
        // Movement: Shift is ignored here and extends instead.
        (K::Named(N::ArrowLeft), false, _, false) => Left,
        (K::Named(N::ArrowRight), false, _, false) => Right,
        (K::Named(N::ArrowUp), false, _, false) => Up,
        (K::Named(N::ArrowDown), false, _, false) => Down,
        // Decided 2026-10-27 (Lucas): ⌘←/→ move by word, as in the original, and
        // ⌘⇧←/→ go to the start or end of the line (selecting, as Shift always
        // extends). This differs from other Mac apps on purpose; do not change it.
        (K::Named(N::ArrowLeft), true, true, false) => LineStart,
        (K::Named(N::ArrowRight), true, true, false) => LineEnd,
        // The original moves by word with ⌘; Option and (off the Mac) Control do
        // the same, as modern systems expect.
        (K::Named(N::ArrowLeft), true, false, false) | (K::Named(N::ArrowLeft), false, _, true) => {
            WordLeft
        }
        (K::Named(N::ArrowRight), true, false, false)
        | (K::Named(N::ArrowRight), false, _, true) => WordRight,
        (K::Named(N::ArrowUp), true, _, _) | (K::Named(N::ArrowUp), false, _, true) => ParaUp,
        (K::Named(N::ArrowDown), true, _, _) | (K::Named(N::ArrowDown), false, _, true) => ParaDown,
        (K::Named(N::Home), false, _, _) => LineStart,
        (K::Named(N::End), false, _, _) => LineEnd,
        (K::Named(N::Home), true, _, _) => DocStart,
        (K::Named(N::End), true, _, _) => DocEnd,
        (K::Named(N::PageUp), _, _, _) => PageUp,
        (K::Named(N::PageDown), _, _, _) => PageDown,

        (K::Named(N::Backspace), true, false, true)
        | (K::Named(N::Backspace), false, false, true) => DeletePrevWord,
        (K::Named(N::Backspace), true, false, false) => InsertGlossaryEntry,
        (K::Named(N::Backspace), _, _, _) => Backspace,
        (K::Named(N::Delete), false, _, true) => DeleteNextWord,
        (K::Named(N::Delete), _, _, _) => DeleteForward,
        (K::Named(N::Enter), false, true, false) => LineBreak,
        (K::Named(N::Enter), _, _, _) => NewParagraph,
        (K::Named(N::Tab), false, false, true) => TabChar,
        (K::Named(N::Tab), false, false, false) => Tab,
        (K::Named(N::Tab), false, true, false) => PrevCell,
        (K::Named(N::Space), true, false, false) | (K::Named(N::Space), false, false, true) => {
            NonbreakingSpace
        }
        (K::Named(N::Space), true, true, false) => RevertToStyle,
        (K::Named(N::F3), false, true, false) => CycleCase,

        (K::Ch('n'), true, false, false) => New,
        (K::Ch('o'), true, false, false) => Open,
        (K::Ch('w'), true, false, false) => Close,
        (K::Ch('s'), true, false, false) => Save,
        // Decided 2026-10-30 (Lucas): ⌘⇧S is Save As, as in other Mac apps.
        // ⌘⌥S stays Split Window.
        (K::Ch('s'), true, true, false) => SaveAs,
        (K::Ch('q'), true, false, false) => Quit,
        (K::Ch('z'), true, false, false) => Undo,
        (K::Ch('y'), true, false, false) => Repeat,
        (K::Ch('x'), true, false, false) => Cut,
        (K::Ch('c'), true, false, false) => Copy,
        (K::Ch('v'), true, false, false) => Paste,
        (K::Ch('v'), true, true, false) => PasteText,
        (K::Ch('a'), true, false, false) => SelectAll,
        (K::Ch('f'), true, false, false) => Find,
        (K::Ch('h'), true, false, false) | (K::Ch('f'), true, true, false) => Replace,
        (K::Ch('g'), true, false, false) => GoTo,
        (K::Ch('k'), true, false, false) => Glossary,
        (K::Ch('e'), true, false, false) => InsertFootnote,
        (K::Ch('e'), true, true, false) => TrackChanges,
        (K::Ch('l'), true, false, false) => Spelling,
        (K::Ch('s'), true, true, true) => ViewFootnotes,
        (K::Ch('a'), true, false, true) => FindAgain,
        (K::Ch('z'), true, false, true) => GoBack,
        (K::Ch('k'), true, false, true) => Hyperlink,
        (K::Ch('s'), true, false, true) => SplitWindow,
        (K::Ch('='), true, false, false) => ZoomIn,
        (K::Ch('0'), true, false, false) => Zoom100,
        (K::Ch('j'), true, false, false) => ShowMarks,
        (K::Ch('n'), true, false, true) => NormalView,
        (K::Ch('i'), true, false, true) => PrintPreview,
        (K::Ch('p'), true, false, false) => Print,
        (K::Ch('p'), true, false, true) => PageLayoutView,
        (K::Ch('o'), true, false, true) => OutlineView,
        (K::Ch('t'), true, false, true) => OutlinePrefix,
        (K::Ch('d'), true, false, false) => CharacterDialog,
        (K::Ch('m'), true, false, false) => ParagraphDialog,
        (K::Ch('t'), true, false, false) => StyleDialog,
        (K::Ch(','), true, false, false) => Preferences,
        (K::Ch('b'), true, _, false) => Bold,
        (K::Ch('i'), true, _, false) => Italic,
        (K::Ch('u'), true, _, false) => Underline,
        (K::Ch(']'), true, false, false) => FontSizeUp,
        (K::Ch('['), true, false, false) => FontSizeDown,
        (K::Ch('-'), true, false, false) => ZoomOut,
        (K::Ch('`'), true, false, false) => NonbreakingHyphen,

        (K::Ch('z'), true, true, false) => Redo,
        (K::Ch(']'), true, true, false) => WordUnderline,
        (K::Ch('['), true, true, false) => DoubleUnderline,
        (K::Ch('\\'), true, true, false) => DottedUnderline,
        (K::Ch('/'), true, true, false) => Strikethru,
        (K::Ch('d'), true, true, false) => Outline,
        (K::Ch('w'), true, true, false) => Shadow,
        (K::Ch('h'), true, true, false) => SmallCaps,
        (K::Ch('k'), true, true, false) => AllCaps,
        (K::Ch('x'), true, true, false) => Hidden,
        (K::Ch('='), true, true, false) => Superscript,
        (K::Ch('-'), true, true, false) => Subscript,
        (K::Ch('.'), true, true, false) | (K::Ch('>'), true, true, false) => LargerFontSize,
        (K::Ch(','), true, true, false) | (K::Ch('<'), true, true, false) => SmallerFontSize,
        (K::Ch('l'), true, true, false) => AlignLeft,
        (K::Ch('c'), true, true, false) => AlignCenter,
        (K::Ch('r'), true, true, false) => AlignRight,
        (K::Ch('j'), true, true, false) => Justify,
        (K::Ch('y'), true, true, false) => DoubleSpace,
        (K::Ch('o'), true, true, false) => SpaceBefore12,
        (K::Ch('p'), true, true, false) => NormalParagraph,
        (K::Ch('t'), true, true, false) => HangingIndent,
        (K::Ch('n'), true, true, false) => Nest,
        (K::Ch('m'), true, true, false) => Unnest,
        _ => return None,
    };
    Some(found)
}

/// A key spec as this system writes shortcuts: "⌘⇧V" on the Mac and
/// "Ctrl+Shift+V" elsewhere. Specs keep the Mac symbols in settings, so
/// only the text shown changes.
pub fn display_key(spec: &str) -> String {
    if cfg!(target_os = "macos") {
        spec.to_string()
    } else {
        pc_key(spec)
    }
}

/// The Windows/Linux spelling of a key spec.
fn pc_key(spec: &str) -> String {
    let (mut ctrl, mut shift, mut alt) = (false, false, false);
    let mut key = spec;
    // ⌘ is Control here, so a stray ⌃ means the same thing.
    while let Some(c) = key.chars().next() {
        match c {
            '⌘' | '⌃' => ctrl = true,
            '⇧' => shift = true,
            '⌥' => alt = true,
            _ => break,
        }
        key = &key[c.len_utf8()..];
    }
    let key = match key {
        "↩" | "⌅" => "Enter",
        "⇥" => "Tab",
        "⌫" => "Backspace",
        "⌦" => "Delete",
        "␣" => "Space",
        "←" => "Left",
        "→" => "Right",
        "↑" => "Up",
        "↓" => "Down",
        other => other,
    };
    let mut out = String::new();
    for (on, name) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(key);
    out
}

/// Tooltip text for a toolbar or ruler control: its name and, if
/// the command has one, its first shortcut, as "Bold (Ctrl+B)".
pub fn tip(name: &str, cmd: Option<Cmd>) -> String {
    let key = cmd.map(shortcut_label).filter(|k| !k.is_empty());
    match key {
        Some(k) => format!("{name} ({})", display_key(k)),
        None => name.to_string(),
    }
}

/// A command's name for a tooltip. Most use their menu name without the
/// "…"; a few menu names need their menu to make sense on their own.
pub fn tip_name(cmd: Cmd) -> &'static str {
    use Cmd::*;
    match cmd {
        FontSizeUp => "Increase Font Size",
        FontSizeDown => "Decrease Font Size",
        NormalView => "Normal View",
        PageLayoutView => "Page Layout View",
        InsertTable => "Insert Table",
        InsertPicture => "Insert Picture",
        AlignLeft => "Align Left",
        AlignRight => "Align Right",
        Superscript => "Superscript",
        Subscript => "Subscript",
        other => other.name().trim_end_matches('…'),
    }
}

/// Shortcut text for menus, like "⌘B" or "⌘⇧Z".
pub fn shortcut_label(cmd: Cmd) -> &'static str {
    use Cmd::*;
    match cmd {
        New => "⌘N",
        Open => "⌘O",
        Close => "⌘W",
        Save => "⌘S",
        SaveAs => "⌘⇧S",
        Quit => "⌘Q",
        Undo => "⌘Z",
        Redo => "⌘⇧Z",
        Repeat => "⌘Y",
        Cut => "⌘X",
        Copy => "⌘C",
        Paste => "⌘V",
        PasteText => "⌘⇧V",
        SelectAll => "⌘A",
        Find => "⌘F",
        Replace => "⌘⇧F",
        GoTo => "⌘G",
        Glossary => "⌘K",
        Hyperlink => "⌘⌥K",
        ZoomIn => "⌘=",
        ZoomOut => "⌘-",
        Zoom100 => "⌘0",
        ShowMarks => "⌘J",
        TrackChanges => "⌘⇧E",
        NormalView => "⌘⌥N",
        PrintPreview => "⌘⌥I",
        Print => "⌘P",
        PageLayoutView => "⌘⌥P",
        OutlineView => "⌘⌥O",
        CharacterDialog => "⌘D",
        ParagraphDialog => "⌘M",
        StyleDialog => "⌘T",
        Bold => "⌘B",
        Italic => "⌘I",
        Underline => "⌘U",
        RevertToStyle => "⌘⇧␣",
        CycleCase => "⇧F3",
        LargerFontSize => "⌘⇧>",
        SmallerFontSize => "⌘⇧<",
        FontSizeUp => "⌘]",
        FontSizeDown => "⌘[",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_shortcuts_spell_out_modifiers_and_keys() {
        assert_eq!(pc_key("⌘V"), "Ctrl+V");
        assert_eq!(pc_key("⌘⇧V"), "Ctrl+Shift+V");
        assert_eq!(pc_key("⌘⇧⌥C"), "Ctrl+Alt+Shift+C");
        assert_eq!(pc_key("⌘⌅"), "Ctrl+Enter");
        assert_eq!(pc_key("⇧⌅"), "Shift+Enter");
        assert_eq!(pc_key("⌘⇧␣"), "Ctrl+Shift+Space");
        assert_eq!(pc_key("⌘⇧>"), "Ctrl+Shift+>");
        assert_eq!(pc_key("⌘⌃⌫"), "Ctrl+Backspace");
        assert_eq!(pc_key("F5"), "F5");
        assert_eq!(pc_key("Page Up"), "Page Up");
        if cfg!(target_os = "macos") {
            assert_eq!(display_key("⌘⇧V"), "⌘⇧V");
        } else {
            assert_eq!(display_key("⌘⇧V"), "Ctrl+Shift+V");
        }
    }

    fn cmd() -> Mods {
        Mods {
            cmd: true,
            ..Default::default()
        }
    }

    fn cmd_shift() -> Mods {
        Mods {
            cmd: true,
            shift: true,
            ..Default::default()
        }
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn default_shortcuts() {
        assert_eq!(lookup(&ch("f"), None, cmd()), Some(Cmd::Find));
        assert_eq!(lookup(&ch("F"), Some('f'), cmd_shift()), Some(Cmd::Replace));
        assert_eq!(lookup(&ch("b"), None, cmd()), Some(Cmd::Bold));
        // Command-Shift-Z is Redo, as on every other Mac app, and no longer
        // strips formatting from the selection.
        assert_eq!(lookup(&ch("Z"), Some('z'), cmd_shift()), Some(Cmd::Redo));
        assert_eq!(lookup(&ch("z"), None, cmd()), Some(Cmd::Undo));
        assert_eq!(lookup(&ch("s"), None, cmd()), Some(Cmd::Save));
        assert_eq!(lookup(&ch("S"), Some('s'), cmd_shift()), Some(Cmd::SaveAs));
        assert_eq!(lookup(&ch("J"), Some('j'), cmd_shift()), Some(Cmd::Justify));
        assert_eq!(lookup(&ch("j"), None, cmd()), Some(Cmd::ShowMarks));
        assert_eq!(lookup(&ch("]"), None, cmd()), Some(Cmd::FontSizeUp));
        assert_eq!(
            lookup(&ch(">"), Some('.'), cmd_shift()),
            Some(Cmd::LargerFontSize)
        );
    }

    #[test]
    fn plain_letters_are_not_commands() {
        assert_eq!(lookup(&ch("b"), None, Mods::default()), None);
    }

    #[test]
    fn shift_return_is_a_line_break() {
        let shift = Mods {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            lookup(&Key::Named(NamedKey::Enter), None, shift),
            Some(Cmd::LineBreak)
        );
        assert_eq!(
            lookup(&Key::Named(NamedKey::Enter), None, Mods::default()),
            Some(Cmd::NewParagraph)
        );
    }

    #[test]
    fn arrows_extend_with_shift() {
        let shift = Mods {
            shift: true,
            ..Default::default()
        };
        let c = lookup(&Key::Named(NamedKey::ArrowLeft), None, shift).unwrap();
        assert!(c.is_movement());
    }

    #[test]
    fn command_arrows_move_by_word_and_with_shift_to_the_line_ends() {
        let cmd = Mods {
            cmd: true,
            ..Default::default()
        };
        let cmd_shift = Mods {
            cmd: true,
            shift: true,
            ..Default::default()
        };
        let left = Key::Named(NamedKey::ArrowLeft);
        let right = Key::Named(NamedKey::ArrowRight);
        assert_eq!(lookup(&left, None, cmd), Some(Cmd::WordLeft));
        assert_eq!(lookup(&right, None, cmd), Some(Cmd::WordRight));
        assert_eq!(lookup(&left, None, cmd_shift), Some(Cmd::LineStart));
        assert_eq!(lookup(&right, None, cmd_shift), Some(Cmd::LineEnd));
    }
}
