//! Reading .docx files.
//!
//! The reader keeps what the original can express. Modern extras either map to
//! the nearest 5.1 feature (list numbering becomes literal numbers and a
//! tab, as the original's Renumber would leave them; superscript becomes a
//! raised, smaller font) or are dropped (comments, content controls'
//! chrome). Text is never dropped on purpose.

use crate::report::{self as notes, ImportReport};
use crate::xml::{self, Element};
use crate::zip::Archive;
use quill_core::document::{FootnotePosition, SavedView};
use quill_core::para::{Mark, Paragraph, chars};
use quill_core::props::{
    Align, BorderLine, BorderStyle, Borders, CellProps, CharProps, LineSpacing, ParaProps, Rgb,
    RowHeight, RowProps, StyleId, TabAlign, TabLeader, TabStop, Underline,
};
use quill_core::{Document, PageNumberFormat, SectionProps, SectionStart, StoryId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub fn read(data: &[u8]) -> Result<Document, String> {
    read_reported(data).map(|(doc, _)| doc)
}

/// Like `read`, with what could not be kept exactly.
pub fn read_reported(data: &[u8]) -> Result<(Document, ImportReport), String> {
    let zip = Archive::open(data).map_err(|e| e.to_string())?;
    let part = |name: &str| -> Result<Option<Element>, String> {
        match zip.read(name).map_err(|e| e.to_string())? {
            Some(bytes) => xml::parse(&bytes).map(Some),
            None => Ok(None),
        }
    };
    let main_path = part("_rels/.rels")?
        .and_then(|rels| {
            rels.children_named("Relationship")
                .find(|r| {
                    r.attr("Type")
                        .is_some_and(|t| t.ends_with("/officeDocument"))
                })
                .and_then(|r| {
                    r.attr("Target")
                        .map(|t| t.trim_start_matches('/').to_string())
                })
        })
        .unwrap_or_else(|| "word/document.xml".to_string());
    let dir = main_path
        .rsplit_once('/')
        .map(|(d, _)| format!("{d}/"))
        .unwrap_or_default();
    let document = part(&main_path)?.ok_or("the file has no document part")?;

    let mut r = Reader {
        doc: Document::new(),
        stamps: HashSet::new(),
        styles: HashMap::new(),
        char_styles: HashMap::new(),
        theme: Theme::default(),
        numbering: Numbering::default(),
        counters: HashMap::new(),
        style_nums: HashMap::new(),
        list_styles: HashMap::new(),
        lists_used: HashSet::new(),
        images: HashMap::new(),
        links: HashMap::new(),
        note_ids: HashMap::new(),
        comment_ids: HashMap::new(),
        open_comments: Vec::new(),
        range_keys: HashMap::new(),
        reading_note: 0,
        part: Part::Main,
        note_comments: Vec::new(),
        custom_mark: None,
        section_refs: Vec::new(),
        report: ImportReport::default(),
        page_sizes: Vec::new(),
        pending: Vec::new(),
        floating: Vec::new(),
        in_box: 0,
        in_table: false,
        table_depth: 0,
        table_styles: HashMap::new(),
        default_table_style: None,
        defaults: (ParaProps::default(), CharProps::default()),
        cell_layers: Vec::new(),
        toc_field: None,
        other_generated: false,
    };
    // .docm and .dotm files keep their Visual Basic project here.
    if zip.names().any(|n| n.ends_with("vbaProject.bin")) {
        r.report.note(notes::MACROS);
    }
    if let Some(theme) = part(&format!("{dir}theme/theme1.xml"))? {
        r.theme = Theme::read(&theme);
    }
    if let Some(styles) = part(&format!("{dir}styles.xml"))? {
        r.read_styles(&styles);
        // Headers and footers the file leaves out are empty ones in its
        // own header and footer styles.
        r.doc.headers = StoryId::HEADERS
            .iter()
            .map(|&id| quill_core::document::blank_header(&r.doc.styles, id))
            .collect();
    }
    if let Some(numbering) = part(&format!("{dir}numbering.xml"))? {
        r.numbering = Numbering::read(&numbering, &r.list_styles);
    }
    if let Some(settings) = part(&format!("{dir}settings.xml"))? {
        r.read_settings(&settings);
    }
    if let Some(core) = part("docProps/core.xml")? {
        r.read_core(&core);
    }
    let body = document.child("body").ok_or("the document has no body")?;
    let file = main_path.rsplit('/').next().unwrap_or("document.xml");
    let main_rels = part(&format!("{dir}_rels/{file}.rels"))?;
    r.images = load_images(&zip, main_rels.as_ref(), &dir);
    r.links = load_links(main_rels.as_ref());
    if has_unkept_changes(body) {
        r.report.note(notes::FORMAT_CHANGES);
    }
    let mut paras = Vec::new();
    r.read_block(body, &mut paras, false);
    if let Some(sect) = body.child("sectPr") {
        // A section with another page size or margins than the last one.
        let last = page_setup(sect);
        if r.page_sizes.iter().any(|p| *p != last) {
            r.report.note(notes::PAGE_STYLES);
        }
        r.read_section(sect);
        // The last section's headers and footers, after the earlier
        // sections' (gathered as their breaks were read).
        r.section_refs.push(header_refs(sect));
    }
    if paras.is_empty() {
        let n = r.doc.styles.get(0);
        paras.push(Paragraph::new(n.pap.clone(), n.chp));
    }
    // A table cannot be the very last thing: Word always ends with a
    // plain paragraph.
    if paras.last().is_some_and(|p| p.pap.in_table) {
        let n = r.doc.styles.get(0);
        paras.push(Paragraph::new(n.pap.clone(), n.chp));
    }
    r.doc.main.paras = paras;
    // Each section's headers and footers, from the parts its sectPr names.
    // A section naming none shares the one before's (Same As Previous);
    // one naming some keeps the before's for the kinds it leaves out, as
    // Word does.
    let section_refs = std::mem::take(&mut r.section_refs);
    r.part = Part::Header;
    for (k, refs) in section_refs.iter().enumerate() {
        r.doc.header_section = k;
        for (story, rid) in refs {
            let target = main_rels.as_ref().and_then(|rels| {
                rels.children_named("Relationship")
                    .find(|x| x.attr("Id") == Some(rid.as_str()))
                    .and_then(|x| x.attr("Target"))
                    .map(str::to_string)
            });
            let Some(target) = target else {
                continue;
            };
            let path = match target.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("{dir}{target}"),
            };
            if let Some(root) = part(&path)? {
                let (hdir, hfile) = path.rsplit_once('/').unwrap_or(("", &path));
                let hrels = part(&format!("{hdir}/_rels/{hfile}.rels"))?;
                r.images = load_images(&zip, hrels.as_ref(), &format!("{hdir}/"));
                r.links = load_links(hrels.as_ref());
                let mut hparas = Vec::new();
                r.read_block(&root, &mut hparas, false);
                if !hparas.is_empty() {
                    r.doc.set_story(*story, quill_core::Story { paras: hparas });
                }
            }
        }
    }
    r.doc.header_section = 0;
    r.part = Part::Note;
    // The notes the text refers to, from footnotes.xml and endnotes.xml.
    if !r.note_ids.is_empty() {
        let mut notes: HashMap<u32, Vec<Paragraph>> = HashMap::new();
        for (kind, end) in [("footnotes", false), ("endnotes", true)] {
            let target = main_rels.as_ref().and_then(|rels| {
                rels.children_named("Relationship")
                    .find(|x| {
                        x.attr("Type")
                            .is_some_and(|t| t.ends_with(&format!("/{kind}")))
                    })
                    .and_then(|x| x.attr("Target"))
                    .map(str::to_string)
            });
            let Some(target) = target else {
                continue;
            };
            let path = match target.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("{dir}{target}"),
            };
            let Some(root) = part(&path)? else {
                continue;
            };
            let (ndir, nfile) = path.rsplit_once('/').unwrap_or(("", &path));
            let nrels = part(&format!("{ndir}/_rels/{nfile}.rels"))?;
            r.images = load_images(&zip, nrels.as_ref(), &format!("{ndir}/"));
            r.links = load_links(nrels.as_ref());
            for note in root.elements() {
                let key = (end, note.attr("id").unwrap_or_default().to_string());
                let Some(&id) = r.note_ids.get(&key) else {
                    continue;
                };
                r.reading_note = id;
                let mut paras = Vec::new();
                r.read_block(note, &mut paras, false);
                r.custom_mark = None;
                if let Some(first) = paras.first_mut()
                    && quill_core::notes::note_start(first) != Some(id)
                {
                    // A note with a mark of its own: put our mark first in
                    // place of the mark's text.
                    if let Some(m) = r.doc.note_marks.get(&id)
                        && first.text.starts_with(m.as_str())
                    {
                        first.remove(0, m.len());
                    }
                    let chp = r.doc.note_mark_chp(first.chp_at(0));
                    first.insert(
                        0,
                        &chars::FOOTNOTE.to_string(),
                        CharProps { object: id, ..chp },
                    );
                }
                notes.insert(id, paras);
            }
            // Word creates an endnotes.xml containing only separator records
            // even when the document has no endnotes. Do not let that empty
            // boilerplate change ordinary footnotes into endnotes.
            if end
                && r.note_ids.keys().any(|(is_end, _)| *is_end)
                && r.note_ids.keys().any(|(is_end, _)| !*is_end)
            {
                r.report.note(notes::ENDNOTES);
            }
            if end && r.note_ids.keys().any(|(is_end, _)| *is_end) {
                r.doc.props.footnote_position = match r.doc.props.footnote_position {
                    FootnotePosition::EndOfSection => FootnotePosition::EndOfSection,
                    _ => FootnotePosition::EndOfDocument,
                };
            }
        }
        for story in &mut r.doc.headers {
            for p in &mut story.paras {
                if inline_notes(p, &notes) {
                    r.report.note(notes::ASIDE_NOTES);
                }
            }
        }
        for &(note, comment) in &r.note_comments {
            let mark = r.doc.main.paras.iter_mut().find_map(|p| {
                let at = p
                    .text
                    .char_indices()
                    .find(|&(at, c)| c == chars::FOOTNOTE && p.chp_of_char(at).object == note)?;
                Some((p, at.0 + chars::FOOTNOTE.len_utf8()))
            });
            if let Some((p, at)) = mark {
                let chp = p.chp_at(at);
                p.insert(
                    at,
                    &chars::ANNOTATION.to_string(),
                    CharProps {
                        object: comment,
                        ..chp
                    },
                );
                r.report.note(notes::NOTE_COMMENTS);
            }
        }
        let mut story = Vec::new();
        for id in quill_core::notes::note_ids(&r.doc.main.paras) {
            story.extend(notes.remove(&id).unwrap_or_default());
        }
        if !story.is_empty() {
            r.doc.set_notes(quill_core::Story { paras: story });
        }
        r.doc.reconcile_notes();
    }
    r.part = Part::Comment;
    // Annotations, from comments.xml.
    let comments = main_rels.as_ref().and_then(|rels| {
        rels.children_named("Relationship")
            .find(|x| x.attr("Type").is_some_and(|t| t.ends_with("/comments")))
            .and_then(|x| x.attr("Target"))
            .map(str::to_string)
    });
    if !r.comment_ids.is_empty()
        && let Some(target) = comments
    {
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("{dir}{target}"),
        };
        let mut notes: HashMap<u32, Vec<Paragraph>> = HashMap::new();
        // Each comment's paragraphs by their w14:paraId, which replies and
        // resolved state (commentsExtended.xml) refer to.
        let mut by_para: HashMap<String, u32> = HashMap::new();
        if let Some(root) = part(&path)? {
            for c in root.elements() {
                let Some(&id) = r.comment_ids.get(c.attr("id").unwrap_or_default()) else {
                    continue;
                };
                for p in c.children_named("p") {
                    if let Some(pid) = p.attr("paraId") {
                        by_para.insert(pid.to_string(), id);
                    }
                }
                r.doc.annotation_info.insert(
                    id,
                    quill_core::notes::AnnotationInfo {
                        initials: c.attr("initials").unwrap_or_default().to_string(),
                        author: c.attr("author").unwrap_or_default().to_string(),
                        // Midnight is how a day-only date is written.
                        date: {
                            let d = c.attr("date").unwrap_or_default();
                            d.strip_suffix("T00:00:00Z")
                                .unwrap_or(d)
                                .chars()
                                .take(32)
                                .collect()
                        },
                        ..Default::default()
                    },
                );
                r.reading_note = id;
                let mut paras = Vec::new();
                r.read_block(c, &mut paras, false);
                if let Some(first) = paras.first_mut()
                    && !first.text.starts_with(chars::ANNOTATION)
                {
                    let chp = first.chp_at(0);
                    first.insert(
                        0,
                        &chars::ANNOTATION.to_string(),
                        CharProps { object: id, ..chp },
                    );
                }
                notes.insert(id, paras);
            }
        }
        let extended = main_rels.as_ref().and_then(|rels| {
            rels.children_named("Relationship")
                .find(|x| {
                    x.attr("Type")
                        .is_some_and(|t| t.ends_with("/commentsExtended"))
                })
                .and_then(|x| x.attr("Target"))
                .map(|t| match t.strip_prefix('/') {
                    Some(abs) => abs.to_string(),
                    None => format!("{dir}{t}"),
                })
        });
        if let Some(path) = extended
            && let Some(root) = part(&path)?
        {
            for ex in root.children_named("commentEx") {
                let Some(&id) = ex.attr("paraId").and_then(|p| by_para.get(p)) else {
                    continue;
                };
                let parent = ex
                    .attr("paraIdParent")
                    .and_then(|p| by_para.get(p))
                    .copied();
                if let Some(info) = r.doc.annotation_info.get_mut(&id) {
                    info.parent = parent.filter(|&p| p != id);
                    info.done = ex.attr("done").is_some_and(|v| v == "1" || v == "true");
                }
            }
        }
        let mut story = Vec::new();
        for id in quill_core::notes::annotation_ids(&r.doc.main.paras) {
            story.extend(notes.remove(&id).unwrap_or_default());
        }
        if !story.is_empty() {
            r.doc.set_annotations(quill_core::Story { paras: story });
        }
        r.doc.reconcile(quill_core::notes::NoteKind::Annotation);
    }
    let real: HashMap<u32, u32> = r
        .range_keys
        .iter()
        .filter_map(|(key, &prov)| Some((prov, *r.comment_ids.get(key)?)))
        .collect();
    crate::comments::settle(&mut r.doc, &real);
    // A TOC field Quill can keep up to date, made as Quill makes its own (at
    // the start, before a section break), stays live.
    if r.toc_field.is_some() && r.doc.existing_toc().is_some() {
        r.doc.props.live_toc = r.toc_field;
        if !r.other_generated {
            r.report.forget(notes::GENERATED_INDEXES);
        }
    }
    r.doc.forget_history();
    Ok((r.doc, r.report))
}

/// A section's page size and margins, to compare one section with another.
fn page_setup(sect: &Element) -> Vec<Option<i32>> {
    let size = sect.child("pgSz");
    let margins = sect.child("pgMar");
    let mut out: Vec<Option<i32>> = ["w", "h"]
        .iter()
        .map(|a| size.and_then(|e| e.int(a)))
        .collect();
    out.extend(
        ["top", "bottom", "left", "right"]
            .iter()
            .map(|a| margins.and_then(|e| e.int(a))),
    );
    out
}

#[derive(Default)]
struct Theme {
    major: Option<String>,
    minor: Option<String>,
}

impl Theme {
    fn read(theme: &Element) -> Theme {
        let mut t = Theme::default();
        fn find<'a>(e: &'a Element, name: &str) -> Option<&'a Element> {
            if e.name == name {
                return Some(e);
            }
            e.elements().find_map(|c| find(c, name))
        }
        let face = |kind: &str| {
            find(theme, kind)
                .and_then(|f| f.child("latin"))
                .and_then(|l| l.attr("typeface"))
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        t.major = face("majorFont");
        t.minor = face("minorFont");
        t
    }
}

/// A list level's look: how its number reads and where it sits.
#[derive(Clone, Default)]
struct Level {
    format: String,
    /// A custom format's pattern (w:numFmt w:format).
    custom: Option<String>,
    text: String,
    start: i32,
    left: Option<i32>,
    hanging: Option<i32>,
    /// The paragraph style this level numbers (w:pStyle), as headings'
    /// outline numbering does.
    style: Option<String>,
    /// Legal numbering (w:isLgl): the levels above this one show as
    /// plain decimal numbers in its label.
    legal: bool,
}

#[derive(Default)]
struct Numbering {
    /// numId -> abstract levels
    nums: HashMap<String, Vec<Level>>,
    /// numId -> the counters it uses. Lists sharing an abstract list
    /// count on from each other, as in Word.
    counters: HashMap<String, String>,
    /// numId -> the levels it restarts (w:startOverride), at the starts
    /// in its levels. Word restarts them where the list is first used,
    /// and the lists sharing its counters go on counting from there.
    restarts: HashMap<String, Vec<usize>>,
}

/// A list level's number format. Word puts newer formats, such as
/// "001, 002, 003", in a compatibility block with a plain fallback.
fn level_format(l: &Element) -> Option<&Element> {
    l.child("numFmt").or_else(|| {
        l.child("AlternateContent")?
            .elements()
            .find(|x| x.name == "Choice" || x.name == "Fallback")?
            .child("numFmt")
    })
}

impl Numbering {
    /// `list_styles` maps numbering styles to the numId each one uses,
    /// for abstract lists that only point at a style (w:numStyleLink).
    fn read(e: &Element, list_styles: &HashMap<String, String>) -> Numbering {
        let mut abstracts: HashMap<String, Vec<Level>> = HashMap::new();
        let mut linked: HashMap<String, String> = HashMap::new();
        for a in e.children_named("abstractNum") {
            let id = a.attr("abstractNumId").unwrap_or_default().to_string();
            if let Some(link) = a.child("numStyleLink").and_then(|l| l.attr("val")) {
                linked.insert(id.clone(), link.to_string());
            }
            let mut levels = vec![Level::default(); 9];
            for l in a.children_named("lvl") {
                let i = l.int("ilvl").unwrap_or(0).clamp(0, 8) as usize;
                let ind = l.child("pPr").and_then(|p| p.child("ind"));
                levels[i] = Level {
                    format: level_format(l)
                        .and_then(|f| f.attr("val"))
                        .unwrap_or("decimal")
                        .to_string(),
                    custom: level_format(l)
                        .and_then(|f| f.attr("format"))
                        .map(str::to_string),
                    style: l
                        .child("pStyle")
                        .and_then(|f| f.attr("val"))
                        .map(str::to_string),
                    legal: l.child("isLgl").is_some_and(|x| x.attr("val") != Some("0")),
                    text: l
                        .child("lvlText")
                        .and_then(|f| f.attr("val"))
                        .unwrap_or("")
                        .to_string(),
                    // Word starts a level at 0 when it doesn't say.
                    start: l.child("start").and_then(|s| s.int("val")).unwrap_or(0),
                    left: ind.and_then(|i| i.int("left").or_else(|| i.int("start"))),
                    hanging: ind.and_then(|i| i.int("hanging")),
                };
            }
            abstracts.insert(id, levels);
        }
        let abstract_of: HashMap<String, String> = e
            .children_named("num")
            .filter_map(|n| {
                let abs = n.child("abstractNumId")?.attr("val")?;
                Some((n.attr("numId")?.to_string(), abs.to_string()))
            })
            .collect();
        // An abstract list that points at a numbering style takes the
        // levels of the list that style uses, and shares its counters.
        let mut same: HashMap<String, String> = HashMap::new();
        for (id, style) in &linked {
            let target = list_styles
                .get(style)
                .and_then(|num| abstract_of.get(num))
                .filter(|t| *t != id);
            if let Some(levels) = target.and_then(|t| abstracts.get(t)).cloned() {
                abstracts.insert(id.clone(), levels);
                same.insert(id.clone(), target.cloned().unwrap_or_default());
            }
        }
        let mut nums = HashMap::new();
        let mut counters = HashMap::new();
        let mut restarts = HashMap::new();
        for n in e.children_named("num") {
            let id = n.attr("numId").unwrap_or_default().to_string();
            let abs = n
                .child("abstractNumId")
                .and_then(|a| a.attr("val"))
                .unwrap_or_default();
            if let Some(levels) = abstracts.get(abs) {
                let mut levels = levels.clone();
                let mut starts = Vec::new();
                for o in n.children_named("lvlOverride") {
                    let i = o.int("ilvl").unwrap_or(0).clamp(0, 8) as usize;
                    if let Some(start) = o.child("startOverride").and_then(|s| s.int("val")) {
                        levels[i].start = start;
                        starts.push(i);
                    }
                }
                let abs = same.get(abs).map_or(abs, String::as_str);
                counters.insert(id.clone(), format!("abstract {abs}"));
                restarts.insert(id.clone(), starts);
                nums.insert(id, levels);
            }
        }
        Numbering {
            nums,
            counters,
            restarts,
        }
    }
}

/// A style as .docx defines it, before inheritance is resolved.
struct RawStyle {
    name: String,
    based_on: Option<String>,
    next: Option<String>,
    ppr: Option<Element>,
    rpr: Option<Element>,
}

/// Word reserves several automatic names for character styles. When Quill's
/// paragraph style has the same name, Word renames that paragraph style with
/// a trailing 1. Map those exact generated ids back on import instead of
/// accumulating a duplicate after every Word round trip.
fn canonical_paragraph_style_name(id: &str, name: &str) -> String {
    match (id, name) {
        ("FootnoteReference1", "Footnote Reference1") => "Footnote Reference".into(),
        ("PageNumber1", "Page Number1") => "Page Number".into(),
        ("LineNumber1", "Line Number1") => "Line Number".into(),
        _ => name.to_string(),
    }
}

/// Read note placement and numbering from either a section (the standard
/// OOXML location) or settings.xml (accepted for Quill's older output).
fn read_note_properties(e: &Element, p: &mut quill_core::DocumentProps) {
    for el in ["footnotePr", "endnotePr"] {
        let Some(pr) = e.child(el) else {
            continue;
        };
        let pos = pr.child("pos").and_then(|x| x.attr("val"));
        let start = pr.child("numStart").and_then(|x| x.int("val"));
        let restart = pr.child("numRestart");
        // Word's settings.xml contains footnotePr/endnotePr elements whose
        // only children identify separator notes. They are not formatting.
        if pos.is_none() && start.is_none() && restart.is_none() {
            continue;
        }
        if let Some(pos) = pos {
            p.footnote_position = match pos {
                "beneathText" => FootnotePosition::BeneathText,
                "sectEnd" => FootnotePosition::EndOfSection,
                "docEnd" => FootnotePosition::EndOfDocument,
                _ => FootnotePosition::BottomOfPage,
            };
        }
        if let Some(n) = start {
            p.footnote_start = n.clamp(1, 32767) as u16;
        }
        p.footnote_restart_each_page = restart.and_then(|x| x.attr("val")) == Some("eachPage");
    }
}

struct Reader {
    doc: Document,
    /// The authors and dates of tracked changes seen, to keep a hostile
    /// file from filling the revision table (see `stamp`).
    stamps: HashSet<quill_core::revision::Stamp>,
    /// .docx style id -> our style id.
    styles: HashMap<String, StyleId>,
    /// Character styles: id -> their rPr, resolved through basedOn.
    char_styles: HashMap<String, Vec<Element>>,
    theme: Theme,
    numbering: Numbering,
    /// Running list counters per (list, level); see `Numbering::counters`.
    counters: HashMap<(String, usize), i32>,
    /// The list numbering each paragraph style gives its paragraphs, by
    /// the file's style id: the numId, and the level when it says.
    style_nums: HashMap<String, (String, Option<usize>)>,
    /// Numbering styles, by the file's style id, and the numId each uses.
    list_styles: HashMap<String, String>,
    /// Table styles by the file's id: each one's w:style element and those
    /// of the styles it is based on, base first.
    table_styles: HashMap<String, Vec<Element>>,
    /// The table style tables without one of their own use.
    default_table_style: Option<String>,
    /// The document's default paragraph and character format.
    defaults: (ParaProps, CharProps),
    /// While a table cell is read: the parts of its table style that apply
    /// to it, in order (the whole table, then banding, first and last rows
    /// and columns). Each may hold a pPr, an rPr and a tcPr.
    cell_layers: Vec<Element>,
    /// The first TOC field met in the main text, as the settings Quill
    /// would make it with.
    toc_field: Option<quill_core::contents::TocOptions>,
    /// An index, or a second table of contents, kept as plain text.
    other_generated: bool,
    /// The lists that have numbered a paragraph, by numId.
    lists_used: HashSet<String>,
    /// The pictures the part being read refers to, by relationship id.
    images: HashMap<String, Arc<[u8]>>,
    /// The hyperlink targets the part being read refers to, by
    /// relationship id.
    links: HashMap<String, String>,
    /// Footnote and endnote references met so far: (endnote, the file's
    /// id) to our note id.
    note_ids: HashMap<(bool, String), u32>,
    /// Comment references met so far: the file's id to our note id.
    comment_ids: HashMap<String, u32>,
    /// The comments whose text is being read (`w:commentRangeStart`), by
    /// our id; the last is the innermost.
    open_comments: Vec<u32>,
    /// Provisional numbers for comment ranges, by Word's id (see
    /// `crate::comments`).
    range_keys: HashMap<String, u32>,
    /// The note being read from footnotes.xml or endnotes.xml.
    reading_note: u32,
    /// Which part of the package is being read.
    part: Part,
    /// Comments made in a footnote: (the note, the comment). Quill keeps
    /// comments only in the main text, so these go on the note's mark.
    note_comments: Vec<(u32, u32)>,
    /// A custom mark's text follows the reference just read.
    custom_mark: Option<u32>,
    /// Each section's header and footer references, in order: the kind
    /// and the relationship id.
    section_refs: Vec<Vec<(StoryId, String)>>,
    report: ImportReport,
    /// The page setup of each section before the last.
    page_sizes: Vec<Vec<Option<i32>>>,
    /// Text box paragraphs, to follow the paragraph being read.
    pending: Vec<Paragraph>,
    /// Floating pictures and text boxes as framed paragraphs, to go before
    /// the paragraph they are anchored to.
    floating: Vec<Paragraph>,
    /// Reading the text of a text box, whose own drawings stay in it.
    in_box: u32,
    /// The paragraph being read is in a table.
    in_table: bool,
    /// How many tables deep the cell being read is: 0 outside any table.
    table_depth: u8,
}

/// Whether the body holds tracked changes Quill accepts rather than
/// keeps: to paragraph, table, section or numbering formatting.
fn has_unkept_changes(e: &Element) -> bool {
    e.elements().any(|c| {
        matches!(
            c.name.as_str(),
            "pPrChange"
                | "sectPrChange"
                | "tblPrChange"
                | "tblPrExChange"
                | "tblGridChange"
                | "trPrChange"
                | "tcPrChange"
                | "numberingChange"
        ) || (c.name == "trPr" && (c.child("ins").is_some() || c.child("del").is_some()))
            || has_unkept_changes(c)
    })
}

/// The children named `name`, including those Word wraps in content
/// controls (`w:sdt`) or custom XML, as it can wrap a table's rows and
/// cells.
fn wrapped<'e>(e: &'e Element, name: &str) -> Vec<&'e Element> {
    let mut out = Vec::new();
    for c in e.elements() {
        if c.name == name {
            out.push(c);
        } else if c.name == "sdt" {
            if let Some(content) = c.child("sdtContent") {
                out.extend(wrapped(content, name));
            }
        } else if matches!(c.name.as_str(), "customXml" | "ins" | "moveTo") {
            out.extend(wrapped(c, name));
        }
    }
    out
}

/// The text boxes in a drawing, not counting boxes inside boxes.
fn text_boxes<'e>(e: &'e Element, out: &mut Vec<&'e Element>) {
    match e.name.as_str() {
        "txbxContent" => out.push(e),
        // Word gives each shape twice: a modern Choice and an old VML
        // Fallback. Read only the first.
        "AlternateContent" => {
            if let Some(first) = e
                .elements()
                .find(|x| x.name == "Choice" || x.name == "Fallback")
            {
                text_boxes(first, out);
            }
        }
        _ => {
            for c in e.elements() {
                text_boxes(c, out);
            }
        }
    }
}

/// A sectPr's header and footer references: the kind and relationship id.
fn header_refs(sect: &Element) -> Vec<(StoryId, String)> {
    let mut out = Vec::new();
    for reference in sect.elements() {
        let footer = match reference.name.as_str() {
            "headerReference" => false,
            "footerReference" => true,
            _ => continue,
        };
        let story = match (reference.attr("type"), footer) {
            (Some("first"), false) => StoryId::FirstHeader,
            (Some("first"), true) => StoryId::FirstFooter,
            (Some("even"), false) => StoryId::EvenHeader,
            (Some("even"), true) => StoryId::EvenFooter,
            (_, false) => StoryId::Header,
            (_, true) => StoryId::Footer,
        };
        if let Some(id) = reference.attr("id") {
            out.push((story, id.to_string()));
        }
    }
    out
}

/// The first element named `name` anywhere inside `e`.
fn descendant<'a>(e: &'a Element, name: &str) -> Option<&'a Element> {
    for c in e.elements() {
        if c.name == name {
            return Some(c);
        }
        if let Some(d) = descendant(c, name) {
            return Some(d);
        }
    }
    None
}

/// The image relationships of a part, with their bytes.
/// Hyperlink relationships: id to target.
fn load_links(rels: Option<&Element>) -> HashMap<String, String> {
    let Some(rels) = rels else {
        return HashMap::new();
    };
    rels.children_named("Relationship")
        .filter(|rel| rel.attr("Type").is_some_and(|t| t.ends_with("/hyperlink")))
        .filter_map(|rel| Some((rel.attr("Id")?.to_string(), rel.attr("Target")?.to_string())))
        .collect()
}

fn load_images(zip: &Archive, rels: Option<&Element>, dir: &str) -> HashMap<String, Arc<[u8]>> {
    let mut out = HashMap::new();
    let mut by_path: HashMap<String, Arc<[u8]>> = HashMap::new();
    let Some(rels) = rels else {
        return out;
    };
    for rel in rels.children_named("Relationship") {
        if !rel.attr("Type").is_some_and(|t| t.ends_with("/image")) {
            continue;
        }
        let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else {
            continue;
        };
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("{dir}{target}"),
        };
        let bytes = if let Some(bytes) = by_path.get(&path) {
            Some(bytes.clone())
        } else if let Ok(Some(bytes)) = zip.read(&path) {
            let bytes: Arc<[u8]> = bytes.into();
            by_path.insert(path, bytes.clone());
            Some(bytes)
        } else {
            None
        };
        if let Some(bytes) = bytes {
            out.insert(id.to_string(), bytes);
        }
    }
    out
}

fn twips_or(e: &Element, name: &str, default: i32) -> i32 {
    e.int(name).unwrap_or(default)
}

impl Reader {
    fn font(&mut self, rfonts: &Element) -> Option<u16> {
        let theme = |v: &str| -> Option<String> {
            if v.starts_with("major") {
                self.theme.major.clone()
            } else {
                self.theme.minor.clone()
            }
        };
        // Only the Latin fonts count. An rFonts naming just the East Asian
        // or complex-script font leaves Latin text in the font it inherits,
        // as in Word.
        let name = rfonts
            .attr("ascii")
            .or_else(|| rfonts.attr("hAnsi"))
            .map(str::to_string)
            .or_else(|| rfonts.attr("asciiTheme").and_then(theme))
            .or_else(|| rfonts.attr("hAnsiTheme").and_then(theme))?;
        Some(self.doc.font_id(&name))
    }

    /// Apply an rPr element on top of `c`.
    /// A comment's text starting or ending (w:commentRangeStart or End),
    /// in the main text.
    fn comment_edge(&mut self, e: &Element) {
        if self.part != Part::Main {
            return;
        }
        let key = e.attr("id").unwrap_or_default().to_string();
        let next = crate::comments::PROVISIONAL + self.range_keys.len() as u32;
        let id = *self.range_keys.entry(key).or_insert(next);
        self.open_comments.retain(|&c| c != id);
        if e.name == "commentRangeStart" {
            self.open_comments.push(id);
        }
    }

    /// Who made the tracked change `e` (a w:ins, w:del or w:rPrChange)
    /// and when. After a few thousand different ones the rest share one
    /// unnamed stamp, so a hostile file can't fill the revision table.
    fn stamp(&mut self, e: &Element) -> quill_core::revision::Stamp {
        const MAX_STAMPS: usize = 10_000;
        let by = quill_core::revision::Stamp::new(
            e.attr("author").unwrap_or("Unknown"),
            e.attr("date").unwrap_or_default(),
        );
        if self.stamps.contains(&by) {
            return by;
        }
        if self.stamps.len() >= MAX_STAMPS {
            return quill_core::revision::Stamp::new("Unknown", "");
        }
        self.stamps.insert(by.clone());
        by
    }

    /// `base` marked inserted (w:ins, w:moveTo) or deleted (w:del,
    /// w:moveFrom) by the change `e`. Text inserted by one person and
    /// deleted by another is both.
    fn revised(&mut self, e: &Element, base: CharProps) -> CharProps {
        let by = self.stamp(e);
        let deleted = matches!(e.name.as_str(), "del" | "moveFrom");
        CharProps {
            rev: quill_core::revision::update(base.rev, |r| {
                if deleted {
                    r.deleted = Some(by);
                } else {
                    r.inserted = Some(by);
                }
            }),
            ..base
        }
    }

    /// A tracked format change in `rpr` (w:rPrChange): the format the
    /// text had before, built on `base` as the current one is.
    fn read_format_change(&mut self, rpr: &Element, base: CharProps, chp: &mut CharProps) {
        let Some(change) = rpr.child("rPrChange") else {
            return;
        };
        let mut old = CharProps { rev: 0, ..base };
        if let Some(old_rpr) = change.child("rPr") {
            let old_rpr = old_rpr.clone();
            self.apply_rpr(&old_rpr, &mut old);
        }
        old.rev = 0;
        // A user can format text and then restore its original appearance.
        // The revision still records who edited it, even when old == current.
        let by = self.stamp(change);
        chp.rev = quill_core::revision::update(chp.rev, |r| r.formatted = Some((by, old)));
    }

    fn apply_rpr(&mut self, e: &Element, c: &mut CharProps) {
        if let Some(st) = e.child("rStyle").and_then(|s| s.attr("val"))
            && let Some(layers) = self.char_styles.get(st).cloned()
        {
            // Bold, italic, caps and the like toggle in Word: a character
            // style that turns one on over a paragraph style that has it on
            // turns it off.
            let under = *c;
            let mut styled = CharProps {
                bold: false,
                italic: false,
                all_caps: false,
                small_caps: false,
                strike: false,
                outline: false,
                shadow: false,
                hidden: false,
                ..under
            };
            for layer in &layers {
                self.apply_rpr(layer, &mut styled);
            }
            styled.bold ^= under.bold;
            styled.italic ^= under.italic;
            styled.all_caps ^= under.all_caps;
            styled.small_caps ^= under.small_caps;
            styled.strike ^= under.strike;
            styled.outline ^= under.outline;
            styled.shadow ^= under.shadow;
            styled.hidden ^= under.hidden;
            *c = styled;
        }
        let mut superscript = None;
        for p in e.elements() {
            match p.name.as_str() {
                "rFonts" => {
                    if let Some(f) = self.font(p) {
                        c.font = f;
                    }
                }
                "b" => c.bold = p.on(),
                "i" => c.italic = p.on(),
                "caps" => c.all_caps = p.on(),
                "smallCaps" => c.small_caps = p.on(),
                "strike" | "dstrike" => c.strike = p.on(),
                "outline" => c.outline = p.on(),
                "shadow" => c.shadow = p.on(),
                "vanish" => c.hidden = p.on(),
                "color" => {
                    if let Some(v) = p.attr("val") {
                        c.color = parse_color(v).unwrap_or(Rgb::BLACK);
                    }
                }
                "spacing" => c.spacing = p.int("val").unwrap_or(0),
                "position" => c.position = p.int("val").unwrap_or(0) * 10,
                "sz" => {
                    if let Some(v) = p.int("val") {
                        c.size = v.clamp(2, 3200) as u16;
                    }
                }
                "u" => {
                    c.underline = match p.attr("val").unwrap_or("single") {
                        "none" => Underline::None,
                        "words" => Underline::Word,
                        "double" | "wavyDouble" => Underline::Double,
                        "dotted" | "dottedHeavy" | "dash" | "dashedHeavy" | "dotDash"
                        | "dotDotDash" => Underline::Dotted,
                        _ => Underline::Single,
                    }
                }
                "vertAlign" => superscript = p.attr("val").map(str::to_string),
                _ => {}
            }
        }
        // The original has no superscript-and-shrink: raise or lower by a third
        // of the size and make the text two thirds as big, which looks the
        // same.
        match superscript.as_deref() {
            Some("superscript") => {
                c.position = crate::superscript_rise(c.size);
                c.size = (c.size * 2).div_ceil(3);
            }
            Some("subscript") => {
                c.position = -crate::subscript_drop(c.size);
                c.size = (c.size * 2).div_ceil(3);
            }
            Some("baseline") => c.position = 0,
            _ => {}
        }
    }

    /// Apply a pPr element on top of `p` (pStyle is handled by the caller).
    fn apply_ppr(&mut self, e: &Element, p: &mut ParaProps) {
        for x in e.elements() {
            match x.name.as_str() {
                "framePr" => p.frame = Some(read_frame(x)),
                "jc" => {
                    p.align = match x.attr("val").unwrap_or("left") {
                        "center" => Align::Center,
                        "right" | "end" => Align::Right,
                        "both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida" => {
                            Align::Justify
                        }
                        _ => Align::Left,
                    }
                }
                "ind" => {
                    if let Some(v) = x.int("left").or_else(|| x.int("start")) {
                        p.left = v;
                    }
                    if let Some(v) = x.int("right").or_else(|| x.int("end")) {
                        p.right = v;
                    }
                    if let Some(v) = x.int("firstLine") {
                        p.first = v;
                    }
                    if let Some(v) = x.int("hanging") {
                        p.first = -v;
                    }
                }
                "spacing" => {
                    if let Some(v) = x.int("before") {
                        p.before = v;
                    }
                    if let Some(v) = x.int("after") {
                        p.after = v;
                    }
                    if let Some(v) = x.int("line") {
                        p.line = match x.attr("lineRule").unwrap_or("auto") {
                            "atLeast" => LineSpacing::AtLeast(v),
                            "exact" | "exactly" => LineSpacing::Exactly(v.abs()),
                            _ if v == 240 => LineSpacing::Auto,
                            _ => LineSpacing::Multiple(v.clamp(1, 2400) as u16),
                        };
                    }
                }
                "tabs" => {
                    for t in x.children_named("tab") {
                        let pos = t.int("pos").unwrap_or(0);
                        let val = t.attr("val").unwrap_or("left");
                        if val == "clear" {
                            p.clear_tab(pos);
                            continue;
                        }
                        let align = match val {
                            "center" => TabAlign::Center,
                            "right" | "end" => TabAlign::Right,
                            "decimal" => TabAlign::Decimal,
                            "bar" => TabAlign::Bar,
                            _ => TabAlign::Left,
                        };
                        let leader = match t.attr("leader").unwrap_or("none") {
                            "dot" | "middleDot" => TabLeader::Dots,
                            "hyphen" => TabLeader::Dashes,
                            "underscore" | "heavy" => TabLeader::Underline,
                            _ => TabLeader::None,
                        };
                        if pos >= 0 {
                            p.set_tab(TabStop { pos, align, leader });
                        }
                    }
                }
                "keepNext" => p.keep_with_next = x.on(),
                "keepLines" => p.keep_together = x.on(),
                "pageBreakBefore" => p.page_break_before = x.on(),
                "suppressLineNumbers" => p.suppress_line_numbers = x.on(),
                "pBdr" => p.borders = read_borders(x, &p.borders),
                "shd" => (p.shading, p.shading_color) = read_shading(x, &mut self.report),
                _ => {}
            }
        }
    }

    fn read_styles(&mut self, e: &Element) {
        // Document defaults are what Normal builds on.
        let mut base_chp = CharProps {
            font: 0,
            size: 20,
            ..Default::default()
        };
        let mut base_pap = ParaProps::default();
        if let Some(d) = e.child("docDefaults") {
            if let Some(rpr) = d.child("rPrDefault").and_then(|r| r.child("rPr")) {
                base_chp.font = self.doc.font_id("Times New Roman");
                let rpr = rpr.clone();
                self.apply_rpr(&rpr, &mut base_chp);
            }
            if let Some(ppr) = d.child("pPrDefault").and_then(|r| r.child("pPr")) {
                let ppr = ppr.clone();
                self.apply_ppr(&ppr, &mut base_pap);
                if ppr.child("widowControl").is_some_and(|w| !w.on()) {
                    self.doc.props.widow_control = false;
                }
            }
        }
        let mut raw: HashMap<String, RawStyle> = HashMap::new();
        let mut char_raw: HashMap<String, RawStyle> = HashMap::new();
        let mut table_raw: HashMap<String, (Option<String>, Element)> = HashMap::new();
        let mut order = Vec::new();
        for s in e.children_named("style") {
            let id = s.attr("styleId").unwrap_or_default().to_string();
            let style = RawStyle {
                name: s
                    .child("name")
                    .and_then(|n| n.attr("val"))
                    .unwrap_or(&id)
                    .to_string(),
                based_on: s
                    .child("basedOn")
                    .and_then(|b| b.attr("val"))
                    .map(str::to_string),
                next: s
                    .child("next")
                    .and_then(|b| b.attr("val"))
                    .map(str::to_string),
                ppr: s.child("pPr").cloned(),
                rpr: s.child("rPr").cloned(),
            };
            match s.attr("type").unwrap_or("paragraph") {
                // CommentText formats Word's comment balloons, not body
                // paragraphs. Quill keeps annotations separately and would
                // otherwise expose this generated utility style in its menu.
                "paragraph" if id != "CommentText" => {
                    order.push(id.clone());
                    raw.insert(id, style);
                }
                "paragraph" => {}
                "character" => {
                    char_raw.insert(id, style);
                }
                "table" => {
                    if s.attr("default").is_some_and(|v| v == "1" || v == "true") {
                        self.default_table_style = Some(id.clone());
                    }
                    table_raw.insert(id, (style.based_on.clone(), s.clone()));
                }
                "numbering" => {
                    let num = style.ppr.as_ref().and_then(|p| p.child("numPr"));
                    if let Some(n) = num
                        .and_then(|n| n.child("numId"))
                        .and_then(|n| n.attr("val"))
                    {
                        self.list_styles.insert(id, n.to_string());
                    }
                }
                _ => {}
            }
        }
        // Table styles become their chain of styles, base first.
        for id in table_raw.keys() {
            let mut chain = Vec::new();
            let mut cur = table_raw.get(id);
            while let Some((based_on, e)) = cur
                && chain.len() < 20
            {
                chain.insert(0, e.clone());
                cur = based_on.as_ref().and_then(|b| table_raw.get(b));
            }
            self.table_styles.insert(id.clone(), chain);
        }
        // Character styles become lists of rPr layers to apply, base first.
        for (id, style) in &char_raw {
            let mut layers = Vec::new();
            let mut cur = Some(style);
            let mut depth = 0;
            while let Some(st) = cur
                && depth < 20
            {
                if let Some(r) = &st.rpr {
                    layers.insert(0, r.clone());
                }
                cur = st.based_on.as_ref().and_then(|b| char_raw.get(b));
                depth += 1;
            }
            self.char_styles.insert(id.clone(), layers);
        }
        // Resolve each paragraph style through its basedOn chain.
        let mut resolved: HashMap<String, (ParaProps, CharProps)> = HashMap::new();
        fn resolve(
            r: &mut Reader,
            id: &str,
            raw: &HashMap<String, RawStyle>,
            resolved: &mut HashMap<String, (ParaProps, CharProps)>,
            base: &(ParaProps, CharProps),
            depth: u32,
        ) -> (ParaProps, CharProps) {
            if let Some(done) = resolved.get(id) {
                return done.clone();
            }
            let Some(s) = raw.get(id) else {
                return base.clone();
            };
            let parent = match &s.based_on {
                Some(b) if depth < 20 && b != id => resolve(r, b, raw, resolved, base, depth + 1),
                _ => base.clone(),
            };
            let (mut pap, mut chp) = parent;
            if let Some(p) = &s.ppr {
                r.apply_ppr(p, &mut pap);
            }
            if let Some(c) = &s.rpr {
                r.apply_rpr(c, &mut chp);
            }
            resolved.insert(id.to_string(), (pap.clone(), chp));
            (pap, chp)
        }
        self.defaults = (base_pap.clone(), base_chp);
        let base = (base_pap, base_chp);
        for id in &order {
            let (pap, chp) = resolve(self, id, &raw, &mut resolved, &base, 0);
            let name = canonical_paragraph_style_name(id, &raw[id].name);
            let our = match self
                .doc
                .styles
                .styles
                .iter()
                .position(|style| style.name.eq_ignore_ascii_case(&name))
            {
                Some(existing) => existing as StyleId,
                None => self.doc.styles.add(&name, 0, pap.clone(), chp),
            };
            let st = &mut self.doc.styles.styles[our as usize];
            st.pap = pap;
            st.pap.style = our;
            st.chp = chp;
            self.styles.insert(id.clone(), our);
        }
        // A style's list numbering: the list and the level each come from
        // the style itself or the nearest style it is based on that gives
        // them, so a heading 2 can take heading 1's list at its own level.
        for id in &order {
            let mut cur = raw.get(id);
            let mut depth = 0;
            let mut num_id = None;
            let mut level = None;
            while let Some(st) = cur
                && depth < 20
                && num_id.is_none()
            {
                if let Some(num) = st.ppr.as_ref().and_then(|p| p.child("numPr")) {
                    num_id = num.child("numId").and_then(|n| n.attr("val"));
                    level = level.or_else(|| num.child("ilvl").and_then(|l| l.int("val")));
                }
                cur = st.based_on.as_ref().and_then(|b| raw.get(b));
                depth += 1;
            }
            if let Some(num_id) = num_id {
                self.style_nums.insert(
                    id.clone(),
                    (num_id.to_string(), level.map(|l| l.clamp(0, 8) as usize)),
                );
            }
        }
        // Without a Normal style, the defaults are Normal.
        if !self.styles.values().any(|&s| s == 0) {
            let st = &mut self.doc.styles.styles[0];
            st.pap = base.0.clone();
            st.chp = base.1;
        }
        for id in &order {
            let s = &raw[id];
            let our = self.styles[id];
            let based = s
                .based_on
                .as_ref()
                .and_then(|b| self.styles.get(b))
                .copied();
            let next = s.next.as_ref().and_then(|n| self.styles.get(n)).copied();
            let st = &mut self.doc.styles.styles[our as usize];
            if our != 0 {
                st.based_on = based.or(Some(0));
            }
            if let Some(n) = next {
                st.next = n;
            }
        }
    }

    fn read_settings(&mut self, e: &Element) {
        self.doc.props.view = match e.child("view").and_then(|v| v.attr("val")) {
            Some("normal") => Some(SavedView::Normal),
            Some("outline") => Some(SavedView::Outline),
            Some("print") => Some(SavedView::PageLayout),
            _ => None,
        };
        if let Some(v) = e.child("defaultTabStop").and_then(|t| t.int("val"))
            && v > 0
        {
            self.doc.props.default_tab = v;
        }
        self.doc.props.even_odd_headers = e.child("evenAndOddHeaders").is_some_and(|x| x.on());
        self.doc.props.mirror_margins = e.child("mirrorMargins").is_some_and(|x| x.on());
        self.doc.props.track_changes = e.child("trackRevisions").is_some_and(|x| x.on());
        // Quill's own (quill: namespace).
        self.doc.props.print_hidden = e.child("printHiddenText").is_some();
        read_note_properties(e, &mut self.doc.props);
    }

    fn read_core(&mut self, e: &Element) {
        let get = |name: &str| e.child(name).map(|x| x.text()).unwrap_or_default();
        let s = &mut self.doc.summary;
        s.title = get("title");
        s.subject = get("subject");
        s.author = get("creator");
        s.keywords = get("keywords");
        s.version = get("version");
    }

    fn read_section(&mut self, e: &Element) {
        self.doc.section = section_settings(e);
        read_note_properties(e, &mut self.doc.props);
        let p = &mut self.doc.props;
        if let Some(sz) = e.child("pgSz") {
            p.page_width = twips_or(sz, "w", p.page_width);
            p.page_height = twips_or(sz, "h", p.page_height);
            if sz.attr("orient") == Some("landscape") && p.page_width < p.page_height {
                std::mem::swap(&mut p.page_width, &mut p.page_height);
            }
        }
        if let Some(m) = e.child("pgMar") {
            let top = twips_or(m, "top", p.margin_top);
            let bottom = twips_or(m, "bottom", p.margin_bottom);
            p.top_exact = top < 0;
            p.bottom_exact = bottom < 0;
            p.margin_top = top.abs();
            p.margin_bottom = bottom.abs();
            p.margin_left = twips_or(m, "left", p.margin_left);
            p.margin_right = twips_or(m, "right", p.margin_right);
            p.gutter = twips_or(m, "gutter", 0);
        }
    }

    /// Read block content (paragraphs and tables) into `out`.
    fn read_block(&mut self, e: &Element, out: &mut Vec<Paragraph>, in_table: bool) {
        for child in e.elements() {
            match child.name.as_str() {
                "p" => {
                    let p = self.read_paragraph(child, in_table);
                    out.append(&mut self.floating);
                    out.push(p);
                    out.append(&mut self.pending);
                }
                "tbl" => self.read_table(child, out),
                "sdt" => {
                    self.note_control(child);
                    if let Some(c) = child.child("sdtContent") {
                        self.read_block(c, out, in_table);
                    }
                }
                "customXml" | "ins" | "moveTo" => self.read_block(child, out, in_table),
                "commentRangeStart" | "commentRangeEnd" => self.comment_edge(child),
                "del" | "moveFrom" => self.report.note(notes::TRACKED_CHANGES),
                "altChunk" => self.report.note(notes::EMBEDDED_DOCUMENTS),
                _ => {}
            }
        }
    }

    /// A table style's paragraph and character formats sit between the
    /// document's defaults and the paragraph's style: they show wherever
    /// the paragraph style leaves a setting at its default.
    fn table_style_under(&mut self, pap: &mut ParaProps, chp: &mut CharProps) {
        let (dp, dc) = self.defaults.clone();
        let (mut tp, mut tc) = (dp.clone(), dc);
        for layer in self.cell_layers.clone() {
            if let Some(p) = layer.child("pPr") {
                self.apply_ppr(p, &mut tp);
            }
            if let Some(r) = layer.child("rPr") {
                self.apply_rpr(r, &mut tc);
            }
        }
        macro_rules! under {
            ($to:ident, $from:ident, $default:ident, $($f:ident),*) => {
                $( if $to.$f == $default.$f { $to.$f = $from.$f.clone(); } )*
            };
        }
        under!(
            pap,
            tp,
            dp,
            align,
            left,
            right,
            first,
            before,
            after,
            line,
            keep_with_next,
            keep_together
        );
        under!(
            chp, tc, dc, font, size, bold, italic, underline, strike, small_caps, all_caps, color
        );
    }

    fn read_table(&mut self, tbl: &Element, out: &mut Vec<Paragraph>) {
        if self.table_depth >= quill_core::limits::MAX_TABLE_LEVEL {
            self.report.note(notes::NESTED_TABLES);
            // Past the depth Quill keeps, the inner table's text stays as
            // paragraphs of the cell it is in.
            for tr in wrapped(tbl, "tr") {
                for tc in wrapped(tr, "tc") {
                    self.read_block(tc, out, true);
                }
            }
            return;
        }
        self.table_depth += 1;
        // The table's own settings, shared by its rows.
        let tblpr = tbl.child("tblPr");
        // Its table style, or the document's default one.
        let chain: Vec<Element> = tblpr
            .and_then(|p| p.child("tblStyle"))
            .and_then(|s| s.attr("val"))
            .map(str::to_string)
            .or_else(|| self.default_table_style.clone())
            .and_then(|id| self.table_styles.get(&id))
            .cloned()
            .unwrap_or_default();
        let look = Look::read(tblpr.and_then(|p| p.child("tblLook")));
        let band_rows = chain
            .iter()
            .rev()
            .find_map(|s| s.child("tblPr")?.child("tblStyleRowBandSize")?.int("val"))
            .unwrap_or(1)
            .max(1) as usize;
        let band_cols = chain
            .iter()
            .rev()
            .find_map(|s| s.child("tblPr")?.child("tblStyleColBandSize")?.int("val"))
            .unwrap_or(1)
            .max(1) as usize;
        // The table's borders: the style's, then its own, side by side.
        let mut edges = TableBorders::default();
        for pr in chain.iter().filter_map(|s| s.child("tblPr")).chain(tblpr) {
            if let Some(b) = pr.child("tblBorders") {
                edges.overlay(b);
            }
        }
        let n_rows = wrapped(tbl, "tr").len();
        let grid: Vec<i32> = tbl
            .child("tblGrid")
            .map(|g| {
                g.children_named("gridCol")
                    .map(|c| twips_or(c, "w", 0))
                    .collect()
            })
            .unwrap_or_default();
        // The space between cells: their margins, by default Word's 0.08
        // in a side. A row can set its own.
        let gap_in = |pr: Option<&Element>, default: (i32, i32)| {
            let mar = pr.and_then(|p| p.child("tblCellMar"));
            let side = |names: [&str; 2], default: i32| {
                mar.and_then(|m| m.child(names[0]).or_else(|| m.child(names[1])))
                    .map_or(default, |c| twips_or(c, "w", default))
            };
            (
                side(["left", "start"], default.0),
                side(["right", "end"], default.1),
            )
        };
        let table_margins = chain
            .iter()
            .filter_map(|s| s.child("tblPr"))
            .chain(tblpr)
            .fold((115, 115), |m, pr| gap_in(Some(pr), m));
        let ind = tblpr
            .and_then(|p| p.child("tblInd"))
            .map(|i| twips_or(i, "w", 0))
            .unwrap_or(0);
        let align = match tblpr
            .and_then(|p| p.child("jc"))
            .and_then(|j| j.attr("val"))
        {
            Some("center") => Align::Center,
            Some("right") | Some("end") => Align::Right,
            _ => Align::Left,
        };
        for (r, tr) in wrapped(tbl, "tr").into_iter().enumerate() {
            let row_start = out.len();
            let n_cells = wrapped(tr, "tc").len();
            let mut row_chp = self.doc.styles.get(0).chp;
            let mut cells = Vec::new();
            let trpr = tr.child("trPr");
            let (left, right) = gap_in(tr.child("tblPrEx"), table_margins);
            let gap = (left + right).max(0);
            // A row can start further in than the table, skipping columns.
            let before = trpr
                .and_then(|p| p.child("gridBefore"))
                .and_then(|g| g.int("val"))
                .unwrap_or(0)
                .max(0) as usize;
            let skipped: i32 = grid.iter().take(before).sum();
            let mut col = before;
            let align = match trpr.and_then(|p| p.child("jc")).and_then(|j| j.attr("val")) {
                Some("center") => Align::Center,
                Some("right") | Some("end") => Align::Right,
                Some(_) => Align::Left,
                None => align,
            };
            // A centered or right-aligned row's own indent, kept for when
            // it is aligned left again.
            let own_ind = tr
                .child("tblPrEx")
                .and_then(|p| p.child("tblInd"))
                .map(|i| twips_or(i, "w", 0));
            let indent = match (align, own_ind) {
                (Align::Left, _) => ind + skipped + gap / 2,
                (_, Some(i)) => i + gap / 2,
                _ => ind + gap / 2,
            };
            for (c, tc) in wrapped(tr, "tc").into_iter().enumerate() {
                let start = out.len();
                let at = CellAt {
                    row: r,
                    rows: n_rows,
                    col: c,
                    cols: n_cells,
                };
                let layers = style_layers(&chain, &look.conditions(at, band_rows, band_cols));
                let held = std::mem::replace(&mut self.cell_layers, layers);
                self.read_block(tc, out, true);
                let layers = std::mem::replace(&mut self.cell_layers, held);
                // A cell ends in a paragraph of its own, even after a table
                // in it.
                if out.len() == start
                    || out.last().is_some_and(|p| p.pap.level() > self.table_depth)
                {
                    let n = self.doc.styles.get(0);
                    let mut p = Paragraph::new(n.pap.clone(), n.chp);
                    p.pap.set_level(self.table_depth);
                    out.push(p);
                }
                let last = out.last_mut().unwrap();
                last.mark = Mark::CellEnd;
                row_chp = last.mark_chp;
                // The cell's width: its grid columns, else its own width.
                let tcpr = tc.child("tcPr");
                let span = tcpr
                    .and_then(|p| p.child("gridSpan"))
                    .and_then(|g| g.int("val"))
                    .unwrap_or(1)
                    .max(1) as usize;
                let from_grid: i32 = grid.iter().skip(col).take(span).sum();
                col += span;
                let own = tcpr
                    .and_then(|p| p.child("tcW"))
                    .filter(|w| w.attr("type").is_none_or(|t| t == "dxa"))
                    .map(|w| twips_or(w, "w", 0))
                    .unwrap_or(0);
                let width = if from_grid > 0 { from_grid } else { own };
                // Borders and shading: the table's edges, then its style's
                // for this cell, then the cell's own.
                let mut borders = edges.for_cell(at);
                let mut shading = 0;
                let mut shading_color = None;
                let mut valign = quill_core::props::VAlign::Top;
                for pr in layers.iter().filter_map(|l| l.child("tcPr")).chain(tcpr) {
                    if let Some(v) = pr.child("vAlign") {
                        valign = match v.attr("val") {
                            Some("center") => quill_core::props::VAlign::Center,
                            Some("bottom") => quill_core::props::VAlign::Bottom,
                            _ => quill_core::props::VAlign::Top,
                        };
                    }
                    if let Some(b) = pr.child("tcBorders") {
                        borders = cell_borders(b, &borders, at);
                    }
                    if let Some(shd) = pr.child("shd") {
                        (shading, shading_color) = read_shading(shd, &mut self.report);
                    }
                }
                cells.push(CellProps {
                    width,
                    borders,
                    shading,
                    shading_color,
                    valign,
                });
            }
            let height = tr
                .child("trPr")
                .and_then(|p| p.child("trHeight"))
                .map(|h| {
                    let v = twips_or(h, "val", 0);
                    match h.attr("hRule") {
                        Some("exact") => RowHeight::Exactly(v),
                        Some("auto") => RowHeight::Auto,
                        _ if v > 0 => RowHeight::AtLeast(v),
                        _ => RowHeight::Auto,
                    }
                })
                .unwrap_or(RowHeight::Auto);
            // Cells with no width share the text column.
            let text_width = self.doc.props.text_width();
            let n = cells.len().max(1) as i32;
            for c in &mut cells {
                if c.width <= 0 {
                    c.width = text_width / n;
                }
            }
            let n = self.doc.styles.get(0);
            let mut row_end = Paragraph::new(n.pap.clone(), row_chp);
            row_end.pap.set_level(self.table_depth);
            // A row whose cells are all in one frame ends in it too: a
            // .docx row end has no paragraph settings to say so.
            let cells_frame = out.get(row_start).and_then(|p| p.pap.frame);
            if out[row_start..].iter().all(|p| p.pap.frame == cells_frame) {
                row_end.pap.frame = cells_frame;
            }
            row_end.mark = Mark::RowEnd;
            row_end.row = Some(Box::new(RowProps {
                cells,
                gap,
                // tblInd is the cell edge; the original measures to the text.
                indent,
                height,
                align,
            }));
            out.push(row_end);
        }
        self.table_depth -= 1;
    }

    fn read_paragraph(&mut self, e: &Element, in_table: bool) -> Paragraph {
        let ppr = e.child("pPr");
        let style_id = ppr
            .and_then(|p| p.child("pStyle"))
            .and_then(|s| s.attr("val"))
            .unwrap_or("Normal")
            .to_string();
        let style = self.styles.get(&style_id).copied().unwrap_or(0);
        let st = self.doc.styles.get(style);
        let mut pap = st.pap.clone();
        let mut base_chp = st.chp;
        if in_table && !self.cell_layers.is_empty() {
            self.table_style_under(&mut pap, &mut base_chp);
        }
        let mut mark_chp = base_chp;
        let mut prefix = None;
        if let Some(ppr) = ppr {
            let ppr = ppr.clone();
            self.apply_ppr(&ppr, &mut pap);
            if let Some(rpr) = ppr.child("rPr") {
                self.apply_rpr(rpr, &mut mark_chp);
                mark_chp.rev = 0;
                for e in rpr.elements() {
                    if matches!(e.name.as_str(), "ins" | "del" | "moveTo" | "moveFrom") {
                        mark_chp = self.revised(e, mark_chp);
                    }
                }
                let rpr = rpr.clone();
                self.read_format_change(&rpr, base_chp, &mut mark_chp);
            }
        }
        // The paragraph's own numbering, or else its style's; a level the
        // paragraph does not give is the one numbering its style.
        let own = ppr.and_then(|p| p.child("numPr"));
        let own_id = own
            .and_then(|n| n.child("numId"))
            .and_then(|n| n.attr("val"));
        let own_level = own
            .and_then(|n| n.child("ilvl"))
            .and_then(|l| l.int("val"))
            .map(|l| l.clamp(0, 8) as usize);
        let list = match (own_id, self.style_nums.get(&style_id)) {
            (Some(id), _) => Some((id.to_string(), own_level)),
            (None, Some((id, level))) => Some((id.clone(), own_level.or(*level))),
            (None, None) => None,
        };
        if let Some((id, level)) = list {
            let level = level.unwrap_or_else(|| {
                self.numbering
                    .nums
                    .get(&id)
                    .and_then(|levels| {
                        levels
                            .iter()
                            .position(|l| l.style.as_deref() == Some(style_id.as_str()))
                    })
                    .unwrap_or(0)
            });
            let own_indent = ppr.is_some_and(|p| p.child("ind").is_some());
            prefix = self.list_prefix(id, level, &mut pap, own_indent);
        }
        pap.style = style;
        pap.in_table = in_table;
        pap.nest = if in_table {
            self.table_depth.saturating_sub(1)
        } else {
            0
        };
        let mut p = Paragraph::new(pap, mark_chp);
        if let Some(sect) = ppr.and_then(|x| x.child("sectPr")) {
            self.page_sizes.push(page_setup(sect));
            p.mark = Mark::SectionBreak;
            p.section = Some(Box::new(section_settings(sect)));
            self.section_refs.push(header_refs(sect));
        }
        if let Some(text) = prefix {
            self.report.note(notes::NUMBERING);
            // Inside a comment's range, the number is part of it like the
            // text after it.
            let comment = match self.part {
                Part::Main => self.open_comments.last().copied().unwrap_or(0),
                _ => 0,
            };
            p.insert(
                0,
                &text,
                CharProps {
                    comment,
                    ..base_chp
                },
            );
        }
        let mut field = FieldState::default();
        let was = self.in_table;
        self.in_table = in_table;
        self.read_runs(e, &mut p, base_chp, &mut field);
        self.in_table = was;
        p
    }

    /// Read the text boxes in `x` into the paragraphs that follow this one.
    /// Returns whether there were any.
    fn read_text_boxes(&mut self, x: &Element) -> bool {
        let paras = self.text_box_paras(x);
        let any = !paras.is_empty();
        self.pending.extend(paras);
        any
    }

    /// The paragraphs of the text boxes in a drawing.
    fn text_box_paras(&mut self, x: &Element) -> Vec<Paragraph> {
        let mut boxes = Vec::new();
        text_boxes(x, &mut boxes);
        let mut paras = Vec::new();
        if boxes.is_empty() {
            return paras;
        }
        let held = std::mem::take(&mut self.pending);
        let floating = std::mem::take(&mut self.floating);
        let in_table = self.in_table;
        self.in_box += 1;
        for b in boxes {
            self.read_block(b, &mut paras, in_table);
        }
        self.in_box -= 1;
        self.in_table = in_table;
        self.pending = held;
        self.floating = floating;
        paras
    }

    /// Whether a drawing anchored here can become a frame: in the main
    /// text, outside tables and other text boxes, as the original allows.
    fn can_float(&self) -> bool {
        self.part == Part::Main && !self.in_table && self.in_box == 0
    }

    /// A floating drawing as framed paragraphs before its paragraph: its
    /// picture, then its text boxes' text. False when it has neither.
    fn float_drawing(&mut self, x: &Element, anchor: &Element, chp: CharProps) -> bool {
        // Behind or in front of the text, which does not flow round it: a
        // frame would push the text aside, so it stays in the text.
        if anchor.child("wrapNone").is_some() || anchor.attr("behindDoc").is_some_and(|v| v == "1")
        {
            return false;
        }
        let picture = self.read_drawing(x);
        let width = descendant(anchor, "extent")
            .and_then(|e| e.attr("cx"))
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map(|emu| (emu / 635) as i32);
        self.float_parts(x, picture, anchor_frame(anchor), width, chp)
    }

    /// A floating picture and the text boxes in `x` as framed paragraphs
    /// before the paragraph being read; `width` is the box's, for a frame
    /// that holds only text. False when there is neither.
    fn float_parts(
        &mut self,
        x: &Element,
        picture: Option<quill_core::Picture>,
        mut frame: quill_core::Frame,
        width: Option<i32>,
        chp: CharProps,
    ) -> bool {
        let mut paras = Vec::new();
        if let Some(pic) = picture {
            let n = self.doc.styles.get(0);
            let mut p = Paragraph::new(n.pap.clone(), n.chp);
            let id = self.doc.add_object(quill_core::Object::Picture(pic));
            p.insert(
                0,
                &chars::OBJECT.to_string(),
                CharProps { object: id, ..chp },
            );
            paras.push(p);
            self.report.note(notes::ANCHORED);
        }
        let boxes = self.text_box_paras(x);
        if !boxes.is_empty() {
            self.report.note(notes::TEXT_BOXES);
            // A box is as wide as Word drew it; a picture sets its own.
            if paras.is_empty() {
                frame.width = width.map(|w| w.clamp(144, 31_680));
            }
        }
        paras.extend(boxes);
        if paras.is_empty() {
            return false;
        }
        for p in &mut paras {
            p.pap.frame = Some(frame);
        }
        self.floating.extend(paras);
        true
    }

    /// The literal number or bullet a list paragraph starts with, and the
    /// list's indents when the paragraph sets none of its own.
    fn list_prefix(
        &mut self,
        id: String,
        level: usize,
        pap: &mut ParaProps,
        own_indent: bool,
    ) -> Option<String> {
        let levels = self.numbering.nums.get(&id)?.clone();
        let lvl = levels.get(level)?.clone();
        if lvl.format == "none" && !lvl.text.contains(|c: char| c.is_alphanumeric()) {
            return None;
        }
        let key = self
            .numbering
            .counters
            .get(&id)
            .cloned()
            .unwrap_or(id.clone());
        // A list that restarts some levels does so where it's first used.
        if self.lists_used.insert(id.clone()) {
            for &k in self.numbering.restarts.get(&id).into_iter().flatten() {
                for level in k..9 {
                    self.counters.remove(&(key.clone(), level));
                }
            }
        }
        let id = key;
        // Count this paragraph; restart deeper levels.
        let n = {
            let c = self
                .counters
                .entry((id.clone(), level))
                .or_insert(lvl.start - 1);
            *c += 1;
            *c
        };
        for deeper in level + 1..9 {
            self.counters.remove(&(id.clone(), deeper));
        }
        if !own_indent {
            if let Some(l) = lvl.left {
                pap.left = l;
            }
            if let Some(h) = lvl.hanging {
                pap.first = -h;
            }
        }
        let text = if lvl.format == "bullet" {
            match lvl.text.chars().next() {
                // Symbol and Wingdings bullets live in the private use area.
                Some(c) if ('\u{F000}'..='\u{F0FF}').contains(&c) || c == '·' => "•".to_string(),
                Some('o') => "◦".to_string(),
                Some(c) => c.to_string(),
                None => "•".to_string(),
            }
        } else {
            let mut t = lvl.text.clone();
            for k in (0..=level).rev() {
                // A level above that hasn't numbered anything yet shows its
                // start, and counts as used: its next paragraph is one on.
                let value = if k == level {
                    n
                } else {
                    *self
                        .counters
                        .entry((id.clone(), k))
                        .or_insert(levels[k].start)
                };
                let format = match levels[k].format.as_str() {
                    _ if lvl.legal && k < level => "decimal",
                    "none" => "",
                    f => f,
                };
                let shown = if format.is_empty() {
                    String::new()
                } else {
                    crate::numfmt::format_number(value, format, levels[k].custom.as_deref())
                };
                t = t.replace(&format!("%{}", k + 1), &shown);
            }
            t
        };
        if text.trim().is_empty() {
            return None;
        }
        Some(format!("{text}\t"))
    }

    fn read_runs(
        &mut self,
        e: &Element,
        p: &mut Paragraph,
        base: CharProps,
        field: &mut FieldState,
    ) {
        for child in e.elements() {
            match child.name.as_str() {
                "r" => self.read_run(child, p, base, field),
                "fldSimple" => match child.attr("instr").and_then(|instr| {
                    // A locked field, or one Quill would show differently,
                    // keeps its stored result when it has one.
                    let locked = child
                        .attr("fldLock")
                        .is_some_and(|v| v == "1" || v == "true" || v == "on");
                    let stored = child.child("r").is_some();
                    known_field(instr)
                        .filter(|_| !locked)
                        .or_else(|| field_kind(instr).filter(|_| !stored))
                }) {
                    // Page numbers, dates and times become Quill's fields,
                    // in the format of the field's first run.
                    Some(c) => {
                        let mut chp = base;
                        if let Some(rpr) = child.child("r").and_then(|r| r.child("rPr")) {
                            let rpr = rpr.clone();
                            self.apply_rpr(&rpr, &mut chp);
                        }
                        let at = p.len();
                        p.insert(at, &c.to_string(), chp);
                    }
                    None if child.attr("instr").and_then(hyperlink_field).is_some() => {
                        let link = child.attr("instr").and_then(hyperlink_field);
                        let link = link.map_or(0, quill_core::link::intern);
                        self.read_runs(child, p, CharProps { link, ..base }, field)
                    }
                    None => {
                        let instr = child.attr("instr").unwrap_or_default();
                        self.note_field(instr);
                        match button_text(instr).filter(|_| child.child("r").is_none()) {
                            Some(shown) => {
                                let at = p.len();
                                p.insert(at, &shown, base);
                            }
                            None => self.read_runs(child, p, base, field),
                        }
                    }
                },
                "hyperlink" => {
                    let target = child.attr("id").and_then(|id| self.links.get(id));
                    let link = target.map(|url| {
                        let mut url = url.clone();
                        if let Some(anchor) = child.attr("anchor") {
                            url = format!("{url}#{anchor}");
                        }
                        quill_core::link::intern(quill_core::Link {
                            new_window: child.attr("tgtFrame") == Some("_blank"),
                            ..quill_core::Link::new(&url)
                        })
                    });
                    match link {
                        Some(link) => self.read_runs(child, p, CharProps { link, ..base }, field),
                        None => {
                            // A link to a bookmark in this document, which
                            // Quill does not have.
                            self.report.note(notes::HYPERLINKS);
                            self.read_runs(child, p, base, field)
                        }
                    }
                }
                // A break written straight into the paragraph, outside a
                // run. Word shows it, so it is kept, not run together.
                "br" | "cr" => {
                    let c = if child.attr("type") == Some("page") {
                        chars::PAGE_BREAK
                    } else {
                        chars::LINE_BREAK
                    };
                    let at = p.len();
                    p.insert(at, &c.to_string(), base);
                }
                "smartTag" | "customXml" | "bdo" | "dir" => self.read_runs(child, p, base, field),
                "commentRangeStart" | "commentRangeEnd" => self.comment_edge(child),
                "ins" | "moveTo" | "del" | "moveFrom" => {
                    let base = self.revised(child, base);
                    self.read_runs(child, p, base, field)
                }
                "sdt" => {
                    self.note_control(child);
                    if let Some(c) = child.child("sdtContent") {
                        self.read_runs(c, p, base, field);
                    }
                }
                "oMath" | "oMathPara" => {
                    self.report.note(notes::EQUATIONS);
                    crate::math::omml(child, p, base);
                }
                // Ruby keeps its base text; the reading guide above it goes.
                "ruby" => self.read_ruby(child, p, base, field),
                // Word accepts a table inside a paragraph; its text follows
                // the paragraph.
                "tbl" => {
                    self.report.note(notes::NESTED_TABLES);
                    let mut held = std::mem::take(&mut self.pending);
                    let in_table = self.in_table;
                    for tr in wrapped(child, "tr") {
                        for tc in wrapped(tr, "tc") {
                            self.read_block(tc, &mut held, in_table);
                        }
                    }
                    self.in_table = in_table;
                    self.pending = held;
                }
                // Word marks the last edit with a hidden _GoBack bookmark;
                // only bookmarks someone made are worth a note.
                "bookmarkStart" if !child.attr("name").unwrap_or_default().starts_with('_') => {
                    self.report.note(notes::BOOKMARKS)
                }
                _ => {}
            }
        }
    }

    /// Ruby text: the base characters, then the guide that goes above
    /// them in brackets, since Quill cannot set it above.
    fn read_ruby(
        &mut self,
        ruby: &Element,
        p: &mut Paragraph,
        base: CharProps,
        field: &mut FieldState,
    ) {
        self.report.note(notes::RUBY);
        if let Some(base_el) = ruby.child("rubyBase") {
            self.read_runs(base_el, p, base, field);
        }
        let Some(rt) = ruby.child("rt") else {
            return;
        };
        let mut guide = Paragraph::new(p.pap.clone(), base);
        self.read_runs(rt, &mut guide, base, field);
        if guide.text.trim().is_empty() {
            return;
        }
        let (open, close) = ruby_brackets(&guide.text);
        let at = p.len();
        p.insert(at, open, p.chp_at(at));
        for (s, e, chp) in guide.run_bounds() {
            let at = p.len();
            p.insert(at, &guide.text[s..e], *chp);
        }
        let at = p.len();
        p.insert(at, close, p.chp_at(at));
    }

    fn read_run(
        &mut self,
        r: &Element,
        p: &mut Paragraph,
        base: CharProps,
        field: &mut FieldState,
    ) {
        let mut chp = base;
        if let Some(rpr) = r.child("rPr") {
            let rpr = rpr.clone();
            self.apply_rpr(&rpr, &mut chp);
            chp.rev = base.rev;
            self.read_format_change(&rpr, base, &mut chp);
        }
        // The text a comment is about, in the main text.
        chp.comment = match self.part {
            Part::Main => self.open_comments.last().copied().unwrap_or(0),
            _ => 0,
        };
        if field.link != 0 {
            chp.link = field.link;
        }
        let mut text = String::new();
        for x in r.elements() {
            match x.name.as_str() {
                "fldChar" => match x.attr("fldCharType") {
                    Some("begin") => {
                        field.depth += 1;
                        if field.depth == 1 {
                            field.instr.clear();
                            field.known = None;
                            field.placed = false;
                            field.separated = false;
                            field.locked = x
                                .attr("fldLock")
                                .is_some_and(|v| v == "1" || v == "true" || v == "on");
                        }
                    }
                    Some("separate") => {
                        if field.depth == 1 {
                            field.separated = true;
                            // A locked field keeps the result Word stored.
                            field.known = known_field(&field.instr).filter(|_| !field.locked);
                            field.link =
                                hyperlink_field(&field.instr).map_or(0, quill_core::link::intern);
                            if field.link != 0 {
                                chp.link = field.link;
                            } else if field.known.is_none() {
                                let instr = field.instr.clone();
                                self.note_field(&instr);
                            }
                        }
                        if let Some(c) = field.known.filter(|_| !field.placed) {
                            text.push(c);
                            field.placed = true;
                        }
                        field.showing = field.depth;
                    }
                    Some("end") => {
                        if field.depth == 1 {
                            // A field with no stored result.
                            if let Some(c) = field_kind(&field.instr).filter(|_| !field.separated) {
                                text.push(c);
                            } else if !field.separated {
                                let instr = field.instr.clone();
                                self.note_field(&instr);
                                text.push_str(&button_text(&instr).unwrap_or_default());
                            }
                            field.known = None;
                            if field.link != 0 {
                                // The result's text so far is the link's.
                                let at = p.len();
                                p.insert(at, &std::mem::take(&mut text), chp);
                                field.link = 0;
                                chp.link = 0;
                            }
                        }
                        field.depth = field.depth.saturating_sub(1);
                        field.showing = field.showing.min(field.depth);
                    }
                    _ => {}
                },
                "instrText" | "delInstrText" if field.depth == 1 && field.showing < 1 => {
                    field.instr.push_str(&x.text());
                }
                _ if field.depth > field.showing => {}
                // A known field's stored result is replaced by the field.
                _ if field.known.is_some() && field.depth > 0 => {}
                "t" if self.custom_mark.is_some() => {
                    let id = self.custom_mark.take().unwrap_or(0);
                    if self.reading_note == 0 {
                        self.doc.note_marks.insert(id, x.text());
                    }
                }
                "t" | "delText" => text.push_str(&x.text()),
                "ruby" => {
                    let at = p.len();
                    p.insert(at, &std::mem::take(&mut text), chp);
                    self.read_ruby(x, p, chp, field);
                }
                "commentReference" if self.part != Part::Main => {
                    let key = x.attr("id").unwrap_or_default().to_string();
                    if self.part == Part::Note {
                        let next = (self.note_ids.len() + self.comment_ids.len()) as u32 + 1;
                        let id = *self.comment_ids.entry(key).or_insert(next);
                        self.note_comments.push((self.reading_note, id));
                    } else {
                        self.report.note(notes::ASIDE_COMMENTS);
                    }
                }
                "commentReference" | "annotationRef" => {
                    let id = if x.name == "annotationRef" {
                        self.reading_note
                    } else {
                        let key = x.attr("id").unwrap_or_default().to_string();
                        let next = (self.note_ids.len() + self.comment_ids.len()) as u32 + 1;
                        *self.comment_ids.entry(key).or_insert(next)
                    };
                    let at = p.len();
                    p.insert(at, &std::mem::take(&mut text), chp);
                    let at = p.len();
                    p.insert(
                        at,
                        &chars::ANNOTATION.to_string(),
                        CharProps { object: id, ..chp },
                    );
                }
                "footnoteReference" | "endnoteReference" | "footnoteRef" | "endnoteRef" => {
                    let id = if x.name.ends_with("Ref") {
                        self.reading_note
                    } else {
                        let key = (
                            x.name.starts_with("end"),
                            x.attr("id").unwrap_or_default().to_string(),
                        );
                        let next = (self.note_ids.len() + self.comment_ids.len()) as u32 + 1;
                        *self.note_ids.entry(key).or_insert(next)
                    };
                    if x.attr("customMarkFollows")
                        .is_some_and(|v| v == "1" || v == "true")
                    {
                        self.custom_mark = Some(id);
                    }
                    let at = p.len();
                    p.insert(at, &std::mem::take(&mut text), chp);
                    let at = p.len();
                    p.insert(
                        at,
                        &chars::FOOTNOTE.to_string(),
                        CharProps { object: id, ..chp },
                    );
                }
                "tab" | "ptab" => text.push(chars::TAB),
                "br" => text.push(if x.attr("type") == Some("page") {
                    chars::PAGE_BREAK
                } else {
                    chars::LINE_BREAK
                }),
                "cr" => text.push(chars::LINE_BREAK),
                // Word's modern shape, with an old VML copy beside it.
                "AlternateContent"
                    if self.can_float()
                        && let Some(d) = x
                            .elements()
                            .find(|c| c.name == "Choice" || c.name == "Fallback")
                            .and_then(|c| c.child("drawing"))
                        && let Some(anchor) = d.child("anchor")
                        && self.float_drawing(d, anchor, chp) => {}
                // An old VML shape: a picture, text boxes, or both.
                "pict" | "object" => {
                    let shape = vml_shape(x);
                    let style = shape.map(vml_style).unwrap_or_default();
                    let picture = self.vml_picture(x, &style);
                    let width = style.get("width").and_then(|w| vml_length(w));
                    if self.can_float()
                        && vml_wraps(x)
                        && let Some(frame) = vml_frame(&style)
                        && self.float_parts(x, picture.clone(), frame, width, chp)
                    {
                        if x.name == "object" {
                            self.report.note(notes::OBJECTS);
                        }
                        continue;
                    }
                    if self.read_text_boxes(x) {
                        self.report.note(notes::TEXT_BOXES);
                    }
                    match picture {
                        Some(pic) => {
                            if x.name == "object" {
                                self.report.note(notes::OBJECTS);
                            }
                            let at = p.len();
                            p.insert(at, &std::mem::take(&mut text), chp);
                            let id = self.doc.add_object(quill_core::Object::Picture(pic));
                            let at = p.len();
                            p.insert(
                                at,
                                &chars::OBJECT.to_string(),
                                CharProps { object: id, ..chp },
                            );
                        }
                        None if descendant(x, "txbxContent").is_none() => self.note_drawing(x),
                        None => {}
                    }
                }
                "AlternateContent" => {
                    if self.read_text_boxes(x) {
                        self.report.note(notes::TEXT_BOXES);
                    } else {
                        self.note_drawing(x);
                    }
                }
                "drawing"
                    if self.can_float()
                        && let Some(anchor) = x.child("anchor")
                        && self.float_drawing(x, anchor, chp) => {}
                "drawing" => {
                    let picture = self.read_drawing(x);
                    // A group can hold a picture and text boxes both.
                    let boxes = self.read_text_boxes(x);
                    if boxes {
                        self.report.note(notes::TEXT_BOXES);
                    }
                    match &picture {
                        Some(_) if x.child("anchor").is_some() => self.report.note(notes::ANCHORED),
                        Some(_) => {}
                        None if boxes => {}
                        None => self.note_drawing(x),
                    }
                    if let Some(pic) = picture {
                        // The picture is its own character in its own run.
                        let at = p.len();
                        p.insert(at, &std::mem::take(&mut text), chp);
                        let id = self.doc.add_object(quill_core::Object::Picture(pic));
                        let at = p.len();
                        p.insert(
                            at,
                            &chars::OBJECT.to_string(),
                            CharProps { object: id, ..chp },
                        );
                    }
                }
                "softHyphen" => text.push(chars::OPTIONAL_HYPHEN),
                "noBreakHyphen" => text.push(chars::NONBREAKING_HYPHEN),
                "sym" => {
                    if let Some(c) = x.attr("char").and_then(|h| u32::from_str_radix(h, 16).ok()) {
                        // Symbol font characters sit at F0xx; the low byte is
                        // the classic Symbol code.
                        let c = if (0xF000..0xF100).contains(&c) {
                            c - 0xF000
                        } else {
                            c
                        };
                        text.extend(char::from_u32(c));
                    }
                }
                _ => {}
            }
        }
        // Stray carriage returns or newlines inside text are line breaks.
        let text: String = text
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' {
                    chars::LINE_BREAK
                } else {
                    c
                }
            })
            .collect();
        let at = p.len();
        p.insert(at, &text, chp);
    }
}

impl Reader {
    /// What a drawing, VML picture or embedded object Quill cannot keep
    /// held: a text box, a picture, a chart or object, or a shape.
    fn note_drawing(&mut self, x: &Element) {
        let what = if descendant(x, "txbxContent").is_some() || descendant(x, "textbox").is_some() {
            notes::TEXT_BOXES_LOST
        } else if x.name == "object"
            || descendant(x, "chart").is_some()
            || descendant(x, "OLEObject").is_some()
        {
            notes::OBJECTS
        } else if descendant(x, "blip").is_some() || descendant(x, "imagedata").is_some() {
            notes::PICTURES
        } else {
            notes::DRAWINGS
        };
        self.report.note(what);
    }

    /// A field Quill keeps as its current text.
    fn note_field(&mut self, instr: &str) {
        let toc = super::write::toc_options(instr);
        if self.part == Part::Main && self.toc_field.is_none() && toc.is_some() {
            self.toc_field = toc;
        } else if matches!(
            instr
                .split_whitespace()
                .next()
                .map(str::to_ascii_uppercase)
                .as_deref(),
            Some("TOC" | "INDEX")
        ) {
            self.other_generated = true;
        }
        let name = instr.split_whitespace().next().unwrap_or_default();
        let what = match name.to_ascii_uppercase().as_str() {
            "" => return,
            "HYPERLINK" => notes::HYPERLINKS,
            "TOC" | "INDEX" => notes::GENERATED_INDEXES,
            _ => notes::FIELDS,
        };
        self.report.note(what);
    }

    /// A content control: a table of contents in one is a generated index.
    fn note_control(&mut self, sdt: &Element) {
        let gallery = descendant(sdt, "docPartGallery").and_then(|g| g.attr("val"));
        if gallery.is_some_and(|g| g.contains("Table of Contents")) {
            self.report.note(notes::GENERATED_INDEXES);
        } else if gallery.is_none() {
            self.report.note(notes::CONTROLS);
        }
    }

    /// A picture from a `w:drawing`: its file, the size it is shown at and
    /// its cropping.
    /// The picture in a VML shape (`v:imagedata`), at the size its style
    /// gives.
    fn vml_picture(
        &self,
        x: &Element,
        style: &HashMap<String, String>,
    ) -> Option<quill_core::Picture> {
        let data = descendant(x, "imagedata")?;
        let bytes = self.images.get(data.attr("id")?)?;
        let mut pic = crate::images::picture(bytes.clone())?;
        let size = |k: &str| style.get(k).and_then(|v| vml_length(v));
        if let (Some(w), Some(h)) = (size("width"), size("height")) {
            pic.size = (w.max(20), h.max(20));
        }
        Some(pic)
    }

    fn read_drawing(&self, d: &Element) -> Option<quill_core::Picture> {
        let blip = descendant(d, "blip")?;
        let bytes = self.images.get(blip.attr("embed")?)?;
        let mut pic = crate::images::picture(bytes.clone())?;
        if let Some(ext) = descendant(d, "extent") {
            let emu = |n: &str| ext.attr(n).and_then(|v| v.parse::<i64>().ok());
            if let (Some(cx), Some(cy)) = (emu("cx"), emu("cy")) {
                pic.size = ((cx / 635).max(20) as i32, (cy / 635).max(20) as i32);
            }
        }
        if let Some(src) = descendant(d, "srcRect") {
            let v = |n: &str| src.int(n).unwrap_or(0);
            pic.crop = quill_core::Crop {
                left: v("l"),
                top: v("t"),
                right: v("r"),
                bottom: v("b"),
            };
        }
        Some(pic)
    }
}

/// The first VML shape in a `w:pict` or `w:object`: the element that
/// carries its CSS-like style.
fn vml_shape(e: &Element) -> Option<&Element> {
    e.elements()
        .find(|c| c.attr("style").is_some())
        .or_else(|| e.elements().find_map(vml_shape))
}

/// A VML style ("position:absolute;width:2in") as lower-case properties.
fn vml_style(shape: &Element) -> HashMap<String, String> {
    shape
        .attr("style")
        .unwrap_or_default()
        .split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// A VML length in twips: pt, in, cm, mm, pc or px (a plain number is in
/// pixels, as in CSS).
fn vml_length(v: &str) -> Option<i32> {
    let v = v.trim();
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(v.len());
    let n: f64 = v[..split].parse().ok()?;
    let per = match v[split..].trim() {
        "pt" => 20.0,
        "in" => 1440.0,
        "cm" => 1440.0 / 2.54,
        "mm" => 144.0 / 2.54,
        "pc" => 240.0,
        "px" | "" => 15.0,
        _ => return None,
    };
    let t = n * per;
    t.is_finite()
        .then(|| t.clamp(-31_680.0, 31_680.0).round() as i32)
}

/// Whether the text flows round a VML shape. Without a `w10:wrap` type
/// the shape is in front of the text (or behind it), and the text runs
/// under it.
fn vml_wraps(e: &Element) -> bool {
    descendant(e, "wrap")
        .and_then(|w| w.attr("type"))
        .is_some_and(|t| t != "none")
}

/// Where an absolutely placed VML shape sits, as a frame; None for one
/// that sits in the text.
fn vml_frame(style: &HashMap<String, String>) -> Option<quill_core::Frame> {
    use quill_core::{FrameH, FrameRel, FrameV};
    if style.get("position").map(String::as_str) != Some("absolute") {
        return None;
    }
    let get = |k: &str| style.get(k).map(String::as_str);
    let len = |k: &str| get(k).and_then(vml_length).unwrap_or(0);
    let h_rel = match get("mso-position-horizontal-relative") {
        Some("margin") => FrameRel::Margin,
        Some("page" | "left-margin-area" | "right-margin-area") => FrameRel::Page,
        _ => FrameRel::Column,
    };
    let h = match get("mso-position-horizontal") {
        Some("center") => FrameH::Center,
        Some("right") => FrameH::Right,
        Some("inside") => FrameH::Inside,
        Some("outside") => FrameH::Outside,
        Some("left") => FrameH::Left,
        _ => FrameH::At(len("margin-left") + len("left")),
    };
    let v_rel = match get("mso-position-vertical-relative") {
        Some("margin") => Some(FrameRel::Margin),
        Some("page" | "top-margin-area" | "bottom-margin-area") => Some(FrameRel::Page),
        _ => None,
    };
    let v = match (v_rel, get("mso-position-vertical")) {
        (None, _) => FrameV::InLine,
        (Some(_), Some("center")) => FrameV::Center,
        (Some(_), Some("bottom")) => FrameV::Bottom,
        (Some(_), Some("top")) => FrameV::Top,
        (Some(_), _) => FrameV::At(len("margin-top") + len("top")),
    };
    let from_text = len("mso-wrap-distance-left").max(len("mso-wrap-distance-right"));
    Some(quill_core::Frame {
        h,
        h_rel,
        v,
        v_rel: v_rel.unwrap_or(FrameRel::Margin),
        width: None,
        from_text: from_text.clamp(0, 1440),
        ..quill_core::Frame::default()
    })
}

/// A floating drawing's place (`wp:anchor`) as a frame. The original's frames
/// cannot sit at an offset from a paragraph, so a drawing placed by its
/// paragraph goes where the paragraph is.
fn anchor_frame(a: &Element) -> quill_core::Frame {
    use quill_core::{FrameH, FrameRel, FrameV};
    let emu = |text: String| text.trim().parse::<i64>().ok().map(|e| (e / 635) as i32);
    let place = |name: &str| {
        let p = a.child(name);
        let from = p.and_then(|p| p.attr("relativeFrom")).unwrap_or_default();
        let align = p
            .and_then(|p| p.child("align"))
            .map(|x| x.text().trim().to_string());
        let offset = p
            .and_then(|p| p.child("posOffset"))
            .and_then(|x| emu(x.text()));
        (from.to_string(), align, offset)
    };
    let (from, align, offset) = place("positionH");
    let h_rel = match from.as_str() {
        "margin" => FrameRel::Margin,
        "page" | "leftMargin" | "rightMargin" | "insideMargin" | "outsideMargin" => FrameRel::Page,
        _ => FrameRel::Column,
    };
    let h = match (from.as_str(), align.as_deref(), offset) {
        ("rightMargin", _, _) => FrameH::Right,
        (_, Some("center"), _) => FrameH::Center,
        (_, Some("right"), _) => FrameH::Right,
        (_, Some("inside"), _) => FrameH::Inside,
        (_, Some("outside"), _) => FrameH::Outside,
        (_, None, Some(t)) => FrameH::At(t),
        _ => FrameH::Left,
    };
    let (from, align, offset) = place("positionV");
    let v_rel = match from.as_str() {
        "margin" => FrameRel::Margin,
        _ => FrameRel::Page,
    };
    let v = match (from.as_str(), align.as_deref(), offset) {
        ("margin" | "page" | "topMargin", Some("center"), _) => FrameV::Center,
        ("margin" | "page", Some("bottom"), _) | ("bottomMargin", _, _) => FrameV::Bottom,
        ("margin" | "page" | "topMargin", Some(_), _) => FrameV::Top,
        ("margin" | "page" | "topMargin", None, Some(t)) => FrameV::At(t),
        _ => FrameV::InLine,
    };
    let dist = |n: &str| a.attr(n).and_then(|v| emu(v.to_string())).unwrap_or(0);
    quill_core::Frame {
        h,
        h_rel,
        v,
        v_rel: if v == FrameV::InLine {
            FrameRel::Margin
        } else {
            v_rel
        },
        width: None,
        from_text: dist("distL").max(dist("distR")).clamp(0, 1440),
        ..quill_core::Frame::default()
    }
}

/// A `w:framePr` as a frame.
fn read_frame(x: &Element) -> quill_core::Frame {
    use quill_core::{FrameH, FrameRel, FrameV};
    let rel = |v: Option<&str>| match v {
        Some("page") => FrameRel::Page,
        Some("margin") => FrameRel::Margin,
        _ => FrameRel::Column,
    };
    let drop = x.attr("dropCap").filter(|v| *v != "none");
    let h = match (x.attr("xAlign"), x.int("x")) {
        (Some("center"), _) => FrameH::Center,
        (Some("right"), _) => FrameH::Right,
        (Some("inside"), _) => FrameH::Inside,
        (Some("outside"), _) => FrameH::Outside,
        (Some(_), _) => FrameH::Left,
        (None, Some(t)) => FrameH::At(t),
        (None, None) => FrameH::Left,
    };
    let v_anchor = x.attr("vAnchor");
    let v = match (x.attr("yAlign"), x.int("y")) {
        (Some("center"), _) => FrameV::Center,
        (Some("bottom"), _) => FrameV::Bottom,
        (Some("inline"), _) => FrameV::InLine,
        (Some(_), _) => FrameV::Top,
        // Anchored to the text with no offset: where the paragraph is.
        (None, Some(t)) if t != 0 || v_anchor != Some("text") => FrameV::At(t),
        _ => FrameV::InLine,
    };
    quill_core::Frame {
        h,
        // ECMA's omitted default is page, but Microsoft Word uses text for
        // both omitted anchors. Follow the producer whose files we read.
        h_rel: rel(x.attr("hAnchor")),
        v,
        v_rel: if v == FrameV::InLine {
            // The anchor is semantically unused for an inline frame; keep
            // Quill's latent default so its own round trip remains exact.
            FrameRel::Margin
        } else {
            match v_anchor {
                Some("page") => FrameRel::Page,
                Some("margin") => FrameRel::Margin,
                _ => FrameRel::Column,
            }
        },
        width: x.int("w").filter(|w| *w > 0),
        from_text: x.int("hSpace").unwrap_or(0),
        drop_lines: if drop.is_some() {
            x.int("lines").unwrap_or(3).clamp(1, 10) as u8
        } else {
            0
        },
        in_margin: drop == Some("margin"),
    }
}

/// The parts of a package that hold text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Main,
    Header,
    Note,
    Comment,
}

/// Put the text of each note `p` refers to in place of its mark, since
/// Quill keeps notes only in the main text. True when there were any.
fn inline_notes(p: &mut Paragraph, notes: &HashMap<u32, Vec<Paragraph>>) -> bool {
    let marks: Vec<usize> = p
        .text
        .char_indices()
        .filter(|&(_, c)| c == chars::FOOTNOTE)
        .map(|(at, _)| at)
        .collect();
    for &at in marks.iter().rev() {
        let id = p.chp_of_char(at).object;
        let chp = CharProps {
            object: 0,
            ..p.chp_at(at)
        };
        let text: Vec<String> = notes
            .get(&id)
            .into_iter()
            .flatten()
            .map(|n| {
                n.text
                    .trim_start_matches(chars::FOOTNOTE)
                    .trim()
                    .to_string()
            })
            .filter(|t| !t.is_empty())
            .collect();
        p.remove(at, at + chars::FOOTNOTE.len_utf8());
        p.insert(at, &text.join(" "), chp);
    }
    !marks.is_empty()
}

/// Where we are inside complex fields: text between "begin" and
/// "separate" is the field code (skipped); between "separate" and "end" is
/// the result (kept).
#[derive(Default)]
struct FieldState {
    depth: u32,
    showing: u32,
    /// The outermost field's instruction, as read so far.
    instr: String,
    /// The outermost field is a page number, date or time: Quill keeps it
    /// as that field's character and drops the stored result.
    known: Option<char>,
    /// The known field's character has been written.
    placed: bool,
    /// The outermost field is locked (w:fldLock): Word doesn't update it.
    locked: bool,
    /// The outermost field has a stored result.
    separated: bool,
    /// The outermost field is a hyperlink: its number, for the runs of
    /// its result.
    link: u32,
}

/// The link a HYPERLINK field instruction makes, as Word writes it:
/// `HYPERLINK "address" \l "place" \t "_blank"`. None for other fields
/// and for links to a place in this document (only `\l`).
pub(crate) fn hyperlink_field(instr: &str) -> Option<quill_core::Link> {
    let mut words = field_words(instr).into_iter();
    if !words.next()?.eq_ignore_ascii_case("HYPERLINK") {
        return None;
    }
    let (mut url, mut place, mut new_window) = (String::new(), String::new(), false);
    while let Some(w) = words.next() {
        match w.as_str() {
            "\\l" => place = words.next().unwrap_or_default(),
            "\\t" => new_window = words.next().is_some_and(|t| t == "_blank"),
            "\\o" | "\\s" => {
                words.next();
            }
            "\\m" | "\\n" | "\\h" => {}
            _ if url.is_empty() => url = w,
            _ => {}
        }
    }
    if url.is_empty() {
        return None;
    }
    if !place.is_empty() {
        url = format!("{url}#{place}");
    }
    Some(quill_core::Link {
        new_window,
        ..quill_core::Link::new(&url)
    })
}

/// A field instruction's words, with quoted ones unquoted and Word's
/// doubled backslashes in them undone.
fn field_words(instr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = instr.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut w = String::new();
        if c == '"' {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' if matches!(chars.peek(), Some('\\' | '"')) => {
                        w.extend(chars.next());
                    }
                    _ => w.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                w.push(c);
                chars.next();
            }
        }
        out.push(w);
    }
    out
}

/// The Quill field a field instruction stands for: page, date or time,
/// however Word shows it.
fn field_kind(instr: &str) -> Option<char> {
    match instr
        .split_whitespace()
        .next()?
        .to_ascii_uppercase()
        .as_str()
    {
        "PAGE" => Some(chars::FIELD_PAGE),
        "DATE" => Some(chars::FIELD_DATE),
        "TIME" => Some(chars::FIELD_TIME),
        _ => None,
    }
}

/// The Quill field a field becomes: a page number, or a date or time Word
/// shows the way Quill does. A date or time Word shows some other way,
/// "21 October 2026" say, keeps the text Word stored for it instead.
pub(crate) fn known_field(instr: &str) -> Option<char> {
    let c = field_kind(instr)?;
    let words = field_words(instr);
    // The picture can follow \@ with or without a space between.
    let picture = words.iter().position(|w| w.starts_with("\\@")).map(|i| {
        match words[i][2..].trim_matches('"') {
            "" => words.get(i + 1).cloned().unwrap_or_default(),
            p => p.to_string(),
        }
    });
    let shown_as_quill = match (c, picture) {
        (_, None) | (chars::FIELD_PAGE, _) => true,
        (chars::FIELD_DATE, Some(p)) => p == "M/d/yy",
        (_, Some(p)) => p.eq_ignore_ascii_case("h:mm am/pm"),
    };
    shown_as_quill.then_some(c)
}

/// The text a MACROBUTTON or GOTOBUTTON field shows, which Word keeps in
/// the instruction after the macro or bookmark name rather than in a result.
pub(crate) fn button_text(instr: &str) -> Option<String> {
    let (name, rest) = instr.trim_start().split_once(char::is_whitespace)?;
    if !["MACROBUTTON", "GOTOBUTTON"].contains(&name.to_ascii_uppercase().as_str()) {
        return None;
    }
    let (_, shown) = rest.trim_start().split_once(char::is_whitespace)?;
    Some(shown.trim().to_string()).filter(|s| !s.is_empty())
}

/// The brackets ruby guide text goes in: full width beside Chinese,
/// Japanese or Korean.
pub(crate) fn ruby_brackets(guide: &str) -> (&'static str, &'static str) {
    let cjk = guide.chars().any(|c| {
        matches!(c as u32, 0x3040..=0x30FF | 0x3100..=0x312F | 0x3400..=0x9FFF | 0xAC00..=0xD7AF)
    });
    if cjk {
        ("\u{FF08}", "\u{FF09}")
    } else {
        ("(", ")")
    }
}

fn parse_color(v: &str) -> Option<Rgb> {
    if v == "auto" {
        return Some(Rgb::BLACK);
    }
    u32::from_str_radix(v, 16).ok().map(Rgb)
}

fn read_border(e: &Element) -> (BorderLine, bool) {
    let sz = e.int("sz").unwrap_or(4);
    let style = match e.attr("val").unwrap_or("single") {
        "nil" | "none" => BorderStyle::None,
        "double" | "triple" | "thinThickSmallGap" | "thickThinSmallGap" => BorderStyle::Double,
        "dotted" | "dashed" | "dotDash" | "dotDotDash" | "dashSmallGap" => BorderStyle::Dotted,
        "thick" => BorderStyle::Thick,
        _ if sz <= 2 => BorderStyle::Hairline,
        _ if sz >= 12 => BorderStyle::Thick,
        _ => BorderStyle::Single,
    };
    let shadow = e
        .attr("shadow")
        .is_some_and(|v| v == "1" || v == "true" || v == "on");
    (
        BorderLine {
            style,
            space: e.int("space").unwrap_or(0) * 20,
            color: e.attr("color").and_then(parse_color).unwrap_or(Rgb::BLACK),
        },
        shadow,
    )
}

/// Borders over `was`, the style's or the paragraph's so far: each side
/// the element names replaces that side, as in Word, and the others stay.
/// Where a cell is in its table.
#[derive(Clone, Copy, Debug)]
struct CellAt {
    row: usize,
    rows: usize,
    col: usize,
    cols: usize,
}

/// A table's w:tblLook: which of its style's special rows and columns
/// it shows.
#[derive(Clone, Copy, Debug)]
struct Look {
    first_row: bool,
    last_row: bool,
    first_col: bool,
    last_col: bool,
    h_bands: bool,
    v_bands: bool,
}

impl Look {
    fn read(e: Option<&Element>) -> Look {
        // Word's own default: header row, first column and row banding.
        let mut look = Look {
            first_row: true,
            last_row: false,
            first_col: true,
            last_col: false,
            h_bands: true,
            v_bands: false,
        };
        let Some(e) = e else {
            return look;
        };
        // Older files give the flags as one hexadecimal number.
        if let Some(v) = e.attr("val").and_then(|v| u32::from_str_radix(v, 16).ok()) {
            look.first_row = v & 0x20 != 0;
            look.last_row = v & 0x40 != 0;
            look.first_col = v & 0x80 != 0;
            look.last_col = v & 0x100 != 0;
            look.h_bands = v & 0x200 == 0;
            look.v_bands = v & 0x400 == 0;
        }
        let flag = |name: &str, was: bool| e.attr(name).map_or(was, |v| v == "1" || v == "true");
        look.first_row = flag("firstRow", look.first_row);
        look.last_row = flag("lastRow", look.last_row);
        look.first_col = flag("firstColumn", look.first_col);
        look.last_col = flag("lastColumn", look.last_col);
        look.h_bands = !flag("noHBand", !look.h_bands);
        look.v_bands = !flag("noVBand", !look.v_bands);
        look
    }

    /// The style's special formats that apply to the cell at `at`, in the
    /// order Word applies them, later ones winning.
    fn conditions(&self, at: CellAt, band_rows: usize, band_cols: usize) -> Vec<&'static str> {
        let first_row = self.first_row && at.row == 0;
        let last_row = self.last_row && at.row + 1 == at.rows;
        let first_col = self.first_col && at.col == 0;
        let last_col = self.last_col && at.col + 1 == at.cols;
        let mut out = vec!["wholeTable"];
        if self.v_bands && !first_col && !last_col {
            let k = at.col - usize::from(self.first_col);
            out.push(if (k / band_cols).is_multiple_of(2) {
                "band1Vert"
            } else {
                "band2Vert"
            });
        }
        if self.h_bands && !first_row && !last_row {
            let k = at.row - usize::from(self.first_row);
            out.push(if (k / band_rows).is_multiple_of(2) {
                "band1Horz"
            } else {
                "band2Horz"
            });
        }
        for (on, name) in [
            (first_col, "firstCol"),
            (last_col, "lastCol"),
            (first_row, "firstRow"),
            (last_row, "lastRow"),
            (first_row && last_col, "neCell"),
            (first_row && first_col, "nwCell"),
            (last_row && last_col, "seCell"),
            (last_row && first_col, "swCell"),
        ] {
            if on {
                out.push(name);
            }
        }
        out
    }
}

/// The parts of a table style (its chain, base first) that apply to a
/// cell: each style's whole-table settings, then each condition's, in
/// order.
fn style_layers(chain: &[Element], conditions: &[&str]) -> Vec<Element> {
    let mut out: Vec<Element> = chain.to_vec();
    for cond in conditions {
        for s in chain {
            out.extend(
                s.children_named("tblStylePr")
                    .filter(|c| c.attr("type") == Some(cond))
                    .cloned(),
            );
        }
    }
    out
}

/// A table's outside and inside borders (w:tblBorders).
#[derive(Clone, Copy, Debug, Default)]
struct TableBorders {
    top: BorderLine,
    bottom: BorderLine,
    left: BorderLine,
    right: BorderLine,
    inside_h: BorderLine,
    inside_v: BorderLine,
}

impl TableBorders {
    /// Take each side `e` gives; the sides it leaves out stay.
    fn overlay(&mut self, e: &Element) {
        for (names, side) in [
            (&["top"][..], &mut self.top),
            (&["bottom"], &mut self.bottom),
            (&["left", "start"], &mut self.left),
            (&["right", "end"], &mut self.right),
            (&["insideH"], &mut self.inside_h),
            (&["insideV"], &mut self.inside_v),
        ] {
            if let Some(b) = names.iter().find_map(|n| e.child(n)) {
                *side = read_border(b).0;
            }
        }
    }

    /// The borders of the cell at `at`: the table's outside edges on the
    /// outside, its inside lines between cells.
    fn for_cell(&self, at: CellAt) -> Borders {
        let pick = |outer: bool, edge: BorderLine, inside: BorderLine| {
            if outer { edge } else { inside }
        };
        Borders {
            top: pick(at.row == 0, self.top, self.inside_h),
            bottom: pick(at.row + 1 == at.rows, self.bottom, self.inside_h),
            left: pick(at.col == 0, self.left, self.inside_v),
            right: pick(at.col + 1 == at.cols, self.right, self.inside_v),
            ..Borders::default()
        }
    }
}

/// A cell's w:tcBorders over `was`. In a table style's special rows and
/// columns, insideH and insideV give the lines between that part's cells.
fn cell_borders(e: &Element, was: &Borders, at: CellAt) -> Borders {
    let mut b = read_borders(e, was);
    if let Some(h) = e.child("insideH") {
        let line = read_border(h).0;
        if at.row > 0 && e.child("top").is_none() {
            b.top = line;
        }
        if at.row + 1 < at.rows && e.child("bottom").is_none() {
            b.bottom = line;
        }
    }
    if let Some(v) = e.child("insideV") {
        let line = read_border(v).0;
        if at.col > 0 && e.child("left").or_else(|| e.child("start")).is_none() {
            b.left = line;
        }
        if at.col + 1 < at.cols && e.child("right").or_else(|| e.child("end")).is_none() {
            b.right = line;
        }
    }
    b
}

fn read_borders(e: &Element, was: &Borders) -> Borders {
    let side = |names: &[&str], was: &BorderLine| {
        names
            .iter()
            .find_map(|n| e.child(n))
            .map(read_border)
            .unwrap_or((*was, false))
    };
    let (top, _) = side(&["top"], &was.top);
    let (left, _) = side(&["left", "start"], &was.left);
    let (bottom, s1) = side(&["bottom"], &was.bottom);
    let (right, s2) = side(&["right", "end"], &was.right);
    let (between, _) = side(&["between"], &was.between);
    // The shadow is on the bottom and right sides.
    let named = ["bottom", "right", "end"]
        .iter()
        .any(|n| e.child(n).is_some());
    Borders {
        top,
        bottom,
        left,
        right,
        between,
        shadow: if named { s1 || s2 } else { was.shadow },
    }
}

/// Solid backgrounds stay RGB; patterns use a legacy gray approximation.
fn read_shading(e: &Element, report: &mut ImportReport) -> (u16, Option<Rgb>) {
    let val = e.attr("val").unwrap_or("clear");
    if val == "nil" {
        return (0, None);
    }
    let fill = e
        .attr("fill")
        .filter(|v| *v != "auto")
        .and_then(parse_color);
    let foreground = e
        .attr("color")
        .filter(|v| *v != "auto")
        .and_then(parse_color);
    if val == "clear" {
        if fill.is_none() && e.attr("themeFill").is_some() {
            report.note(notes::SHADING_COLORS);
        }
        return (0, fill);
    }
    if val == "solid" {
        if foreground.is_none() && e.attr("themeColor").is_some() {
            report.note(notes::SHADING_COLORS);
        }
        return (1000, foreground);
    }
    // Colored patterns still need a pattern renderer. Keep the gray
    // approximation and report that conversion rather than invent a fill.
    if [fill, foreground]
        .into_iter()
        .flatten()
        .any(|c| c.r() != c.g() || c.g() != c.b())
    {
        report.note(notes::SHADING_COLORS);
    }
    if let Some(pct) = val.strip_prefix("pct") {
        let n: u16 = pct.parse().unwrap_or(0);
        return (
            match n {
                12 => 125,
                37 => 375,
                62 => 625,
                87 => 875,
                n => n.min(100) * 10,
            },
            None,
        );
    }
    if let Some(c) = fill {
        let lum = (c.r() as u32 * 30 + c.g() as u32 * 59 + c.b() as u32 * 11) / 100;
        let pct = ((255 - lum) * 1000 / 255) as u16;
        return (
            quill_core::props::SHADING_STEPS
                .iter()
                .copied()
                .min_by_key(|s| s.abs_diff(pct))
                .unwrap_or(0),
            None,
        );
    }
    (0, None)
}

/// A `w:sectPr`'s section settings (the page size and margins are the
/// document's and read separately).
fn section_settings(e: &Element) -> SectionProps {
    let mut s = SectionProps::default();
    let sec = &mut s;
    // With no w:type, a section starts on a new page.
    sec.start = match e.child("type").and_then(|t| t.attr("val")) {
        Some("continuous") => SectionStart::NoBreak,
        Some("nextColumn") => SectionStart::NewColumn,
        Some("evenPage") => SectionStart::EvenPage,
        Some("oddPage") => SectionStart::OddPage,
        _ => SectionStart::NewPage,
    };
    if let Some(n) = e.child("pgNumType") {
        sec.page_number_format = match n.attr("fmt") {
            Some("upperRoman") => PageNumberFormat::UpperRoman,
            Some("lowerRoman") => PageNumberFormat::LowerRoman,
            Some("upperLetter") => PageNumberFormat::UpperLetter,
            Some("lowerLetter") => PageNumberFormat::LowerLetter,
            _ => PageNumberFormat::Arabic,
        };
        sec.restart_page_numbers = n.attr("start").is_some();
    }
    if let Some(c) = e.child("cols") {
        sec.columns = c
            .attr("num")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1u16)
            .max(1);
        sec.column_spacing = twips_or(c, "space", sec.column_spacing);
    }
    sec.different_first_page = e.child("titlePg").is_some_and(|t| t.on());
    if let Some(l) = e.child("lnNumType")
        && let Some(n) = l.int("countBy").filter(|n| *n > 0)
    {
        sec.line_count_by = n.min(100) as u16;
        sec.line_from_text = l.int("distance");
        sec.line_numbers = match l.attr("restart") {
            Some("newSection") => quill_core::LineNumbering::BySection,
            Some("continuous") => quill_core::LineNumbering::Continuous,
            _ => quill_core::LineNumbering::ByPage,
        };
    }
    if let Some(m) = e.child("pgMar") {
        sec.header_from_top = twips_or(m, "header", sec.header_from_top);
        sec.footer_from_bottom = twips_or(m, "footer", sec.footer_from_bottom);
    }
    // Quill's own settings (quill: namespace; prefixes are dropped on reading).
    if let Some(m) = e.child("marginPageNumbers") {
        sec.margin_page_numbers = true;
        sec.page_number_from_top = twips_or(m, "top", sec.page_number_from_top);
        sec.page_number_from_right = twips_or(m, "right", sec.page_number_from_right);
    }
    sec.include_endnotes = e.child("includeEndnotes").is_some();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A .docx with just this body.
    fn docx(body: &str) -> Vec<u8> {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="w" xmlns:wps="s" xmlns:wp="p" xmlns:mc="m" xmlns:v="v"><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.finish()
    }

    #[test]
    fn latin_text_keeps_the_default_font_past_other_scripts_fonts() {
        let ns = r#"xmlns:w="w""#;
        let styles = format!(
            r#"<w:styles {ns}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="MS Mincho" w:cs="Times New Roman"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:rPr><w:rFonts w:eastAsia="Times New Roman" w:cs="Times New Roman"/></w:rPr></w:style></w:styles>"#
        );
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let font = doc.main.paras[0].chp_of_char(0).font;
        assert_eq!(doc.font_name(font), "Calibri");
    }

    #[test]
    fn tables_take_their_style() {
        let ns = r#"xmlns:w="w""#;
        let line = |side: &str| format!(r#"<w:{side} w:val="single" w:sz="4"/>"#);
        let grid: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
            .iter()
            .map(|s| line(s))
            .collect();
        let styles = format!(
            r#"<w:styles {ns}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style><w:style w:type="table" w:styleId="Grid"><w:name w:val="Grid"/><w:basedOn w:val="TableNormal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:tblPr><w:tblBorders>{grid}</w:tblBorders></w:tblPr><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="000000"/></w:tcPr></w:tblStylePr></w:style></w:styles>"#
        );
        let cell = |t: &str| format!(r#"<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#);
        let row = |a: &str, b: &str| format!("<w:tr>{}{}</w:tr>", cell(a), cell(b));
        let table = |style: &str, look: &str| {
            format!(
                r#"<w:tbl><w:tblPr>{style}{look}</w:tblPr><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>{}{}</w:tbl><w:p/>"#,
                row("H1", "H2"),
                row("a", "b")
            )
        };
        let body = table(r#"<w:tblStyle w:val="Grid"/>"#, "")
            + &table("", "")
            + &table(
                r#"<w:tblStyle w:val="Grid"/>"#,
                r#"<w:tblLook w:firstRow="0"/>"#,
            );
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let paras = &doc.main.paras;
        // Each table: H1 H2 row-end a b row-end, then the paragraph after.
        let (grid, plain, no_header) = (0, 7, 14);
        let row = |p: usize| paras[p + 2].row.as_ref().unwrap();
        // The style's borders go round every cell; the default style has none.
        let b = &row(grid).cells[0].borders;
        assert!(!b.top.is_none() && !b.left.is_none() && !b.bottom.is_none() && !b.right.is_none());
        assert!(row(plain).cells[0].borders.is_none());
        // Its paragraph spacing replaces the document default.
        assert_eq!(paras[grid + 3].pap.after, 0);
        assert_eq!(paras[grid + 3].pap.line, LineSpacing::Auto);
        assert_eq!(paras[plain + 3].pap.after, 160);
        // The header row is bold and shaded, unless the table turns it off.
        assert!(paras[grid].chp_of_char(0).bold);
        assert!(row(grid).cells[0].shading_color.is_some());
        assert!(!paras[grid + 3].chp_of_char(0).bold);
        assert!(!paras[no_header].chp_of_char(0).bold);
        assert_eq!(row(no_header).cells[0].shading, 0);
        // The default style's cell margins: 108 a side.
        assert_eq!(row(plain).gap, 216);
    }

    #[test]
    fn a_break_outside_any_run_is_kept() {
        let doc = read(&docx(
            r#"<w:p><w:r><w:t>First</w:t></w:r><w:br/><w:r><w:t>Second</w:t></w:r><w:br w:type="page"/><w:r><w:t>Third</w:t></w:r></w:p>"#,
        ))
        .unwrap();
        assert_eq!(
            doc.main.paras[0].text,
            format!("First{}Second{}Third", chars::LINE_BREAK, chars::PAGE_BREAK)
        );
    }

    #[test]
    fn headings_numbered_through_their_styles() {
        let ns = r#"xmlns:w="w""#;
        let styles = format!(
            r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:numPr><w:numId w:val="5"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Heading1"/></w:style></w:styles>"#
        );
        let numbering = format!(
            r#"<w:numbering {ns}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:pStyle w:val="Heading1"/><w:lvlText w:val="CH %1"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:isLgl/><w:numFmt w:val="decimalZero"/><w:pStyle w:val="Heading2"/><w:lvlText w:val="Sect %1.%2"/></w:lvl></w:abstractNum><w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
        );
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>One</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr><w:r><w:t>Foo</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Two</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr><w:r><w:t>Plain</w:t></w:r></w:p>"#;
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        zip.add("word/numbering.xml", numbering.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            ["CH I\tOne", "Sect 1.01\tFoo", "CH II\tTwo", "Plain"]
        );
    }

    #[test]
    fn lists_count_the_way_word_does() {
        let ns = r#"xmlns:w="w" xmlns:mc="m""#;
        // Heading 2 takes heading 1's list at its own level; the Steps
        // numbering style points at list 2.
        let styles = format!(
            r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="H1"><w:name w:val="H1"/><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="H2"><w:name w:val="H2"/><w:basedOn w:val="H1"/><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr></w:style><w:style w:type="numbering" w:styleId="Steps"><w:name w:val="Steps"/><w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style></w:styles>"#
        );
        // The first level has no start, so starts at 0. The second has
        // a newer format in a compatibility block.
        let numbering = format!(
            r#"<w:numbering {ns}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><mc:AlternateContent><mc:Choice><w:numFmt w:val="custom" w:format="001, 002, 003, ..."/></mc:Choice><mc:Fallback><w:numFmt w:val="decimal"/></mc:Fallback></mc:AlternateContent><w:lvlText w:val="%1.%2"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="4"><w:numStyleLink w:val="Steps"/></w:abstractNum><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num><w:num w:numId="7"><w:abstractNumId w:val="4"/></w:num></w:numbering>"#
        );
        let item = |num: u32, level: u32, text: &str| {
            format!(
                r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{num}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
            )
        };
        let body = [
            item(2, 1, "a"),
            item(2, 0, "b"),
            item(3, 0, "c"),
            item(2, 0, "d"),
            item(7, 0, "e"),
            r#"<w:p><w:pPr><w:pStyle w:val="H2"/></w:pPr><w:r><w:t>f</w:t></w:r></w:p>"#.into(),
        ]
        .concat();
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        zip.add("word/numbering.xml", numbering.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        // The level above "a" shows its start and counts as used, so "b"
        // is one on. List 3 restarts at 5 where it's first used and list 2
        // counts on from there, as does list 7 through the Steps style.
        assert_eq!(
            texts,
            ["0.001\ta", "1.\tb", "5.\tc", "6.\td", "7.\te", "7.001\tf"]
        );
    }

    #[test]
    fn dates_keep_word_s_text_unless_quill_shows_them_the_same_way() {
        let field = |begin: &str, instr: &str, result: &str| {
            format!(
                r#"<w:p><w:r><w:fldChar w:fldCharType="begin"{begin}/></w:r><w:r><w:instrText xml:space="preserve">{instr}</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>{result}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
            )
        };
        let body = [
            field(r#" w:fldLock="1""#, r#"DATE \@"M/d/yy""#, "07/24/14"),
            field("", r#" DATE \@ "dd MMMM yyyy" "#, "21 October 2026"),
            field("", r#" TIME \@ "dd MMMM yyyy" "#, "21 October 2026"),
            field("", r#" DATE \@ "M/d/yy" "#, "10/22/26"),
            field("", " TIME ", "4:05 PM"),
            r#"<w:p><w:fldSimple w:instr=" DATE \@ &quot;yyyy&quot; "/></w:p>"#.into(),
            r#"<w:p><w:fldSimple w:instr=" DATE \@ &quot;yyyy&quot; "><w:r><w:t>2026</w:t></w:r></w:fldSimple></w:p>"#.into(),
            r#"<w:p><w:fldSimple w:instr=" PAGE \* roman "><w:r><w:t>iii</w:t></w:r></w:fldSimple></w:p>"#.into(),
        ]
        .concat();
        let doc = read(&docx(&body)).unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        let (date, time, page) = (
            chars::FIELD_DATE.to_string(),
            chars::FIELD_TIME.to_string(),
            chars::FIELD_PAGE.to_string(),
        );
        assert_eq!(
            texts,
            [
                "07/24/14",
                "21 October 2026",
                "21 October 2026",
                &date,
                &time,
                &date,
                "2026",
                &page
            ]
        );
    }

    #[test]
    fn character_styles_toggle_what_the_paragraph_style_turns_on() {
        let ns = r#"xmlns:w="w""#;
        let styles = format!(
            r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Loud"><w:name w:val="Loud"/><w:rPr><w:b/><w:caps/></w:rPr></w:style><w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/><w:rPr><w:b/><w:i/></w:rPr></w:style></w:styles>"#
        );
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Loud"/></w:pPr><w:r><w:t>a</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr><w:t>b</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Strong"/><w:b/></w:rPr><w:t>c</w:t></w:r></w:p>"#;
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let p = &doc.main.paras[0];
        let (a, b, c) = (p.chp_of_char(0), p.chp_of_char(1), p.chp_of_char(2));
        assert!(a.bold && a.all_caps && !a.italic);
        assert!(!b.bold, "bold twice is not bold");
        assert!(b.italic && b.all_caps);
        assert!(c.bold, "direct formatting sets it outright");
    }

    #[test]
    fn borders_replace_only_the_sides_they_name() {
        let ns = r#"xmlns:w="w""#;
        let styles = format!(
            r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:pBdr><w:top w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/></w:pBdr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Boxed"><w:name w:val="Boxed"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:top w:val="nil"/><w:left w:val="double"/></w:pBdr></w:pPr></w:style></w:styles>"#
        );
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Boxed"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#;
        let xml = format!(
            r#"<?xml version="1.0"?><w:document {ns}><w:body>{body}</w:body></w:document>"#
        );
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml.as_bytes());
        zip.add("word/styles.xml", styles.as_bytes());
        let doc = read(&zip.finish()).unwrap();
        let b = &doc.main.paras[0].pap.borders;
        assert_eq!(b.top.style, BorderStyle::None);
        assert_eq!(b.bottom.style, BorderStyle::Single, "kept from Normal");
        assert_eq!(b.left.style, BorderStyle::Double);

        // Saved and read again, a style without Normal's borders keeps
        // having none.
        let back = read(&crate::docx::write(&doc, 0)).unwrap();
        let boxed = back.styles.find("Boxed").unwrap();
        assert_eq!(back.styles.get(boxed).pap.borders, *b);
    }

    #[test]
    fn empty_headers_keep_the_document_s_header_style() {
        let mut doc = Document::new();
        let arial = doc.font_id("Arial");
        let header = doc.styles.find("header").unwrap();
        doc.styles.styles[header as usize].chp.font = arial;
        doc.headers = StoryId::HEADERS
            .iter()
            .map(|&id| quill_core::document::blank_header(&doc.styles, id))
            .collect();
        // One with formatting of its own is written even though it's empty.
        doc.headers[1].paras[0].pap.left = 720;
        let back = read(&crate::docx::write(&doc, 0)).unwrap();
        for (k, id) in StoryId::HEADERS.iter().enumerate() {
            let (p, q) = (&doc.headers[k].paras[0], &back.headers[k].paras[0]);
            assert_eq!(
                back.font_name(q.mark_chp.font),
                doc.font_name(p.mark_chp.font),
                "{id:?}"
            );
            assert_eq!(q.pap.left, p.pap.left, "{id:?}");
        }
    }

    #[test]
    fn a_row_keeps_its_own_space_between_cells() {
        use quill_core::Position;
        let mut doc = quill_core::text::from_plain_text("x");
        doc.insert_table(StoryId::Main, Position::new(0, 0), 2, 2, 2000);
        let ends: Vec<usize> = (0..doc.main.paras.len())
            .filter(|&i| doc.main.paras[i].mark == Mark::RowEnd)
            .collect();
        let row = doc.main.paras[ends[1]].row.as_mut().unwrap();
        row.gap = 400;
        row.indent = 200;
        let rows = |d: &Document| -> Vec<quill_core::RowProps> {
            d.main
                .paras
                .iter()
                .filter_map(|p| p.row.as_deref().cloned())
                .collect()
        };
        let back = read(&crate::docx::write(&doc, 0)).unwrap();
        assert_eq!(rows(&back), rows(&doc));
    }

    #[test]
    fn rows_and_cells_in_content_controls_are_read() {
        let doc = read(&docx(
            "<w:tbl><w:sdt><w:sdtContent><w:tr><w:sdt><w:sdtContent><w:tc><w:p><w:r><w:t>wrapped</w:t></w:r></w:p></w:tc></w:sdtContent></w:sdt></w:tr></w:sdtContent></w:sdt></w:tbl>",
        ))
        .unwrap();
        assert_eq!(doc.main.paras[0].text, "wrapped");
        assert!(doc.main.paras[1].row.is_some());
    }

    #[test]
    fn text_box_text_follows_its_paragraph_once_and_is_reported() {
        let (doc, report) = read_reported(&docx(
            "<w:p><w:r><w:t>Before</w:t></w:r><w:r><mc:AlternateContent><mc:Choice><w:drawing><wps:txbx><w:txbxContent><w:p><w:r><w:t>Boxed</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback><w:pict><v:textbox><w:txbxContent><w:p><w:r><w:t>Boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p><w:p><w:r><w:t>After</w:t></w:r></w:p>",
        ))
        .unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Before", "Boxed", "After"]);
        assert_eq!(report.notes(), [notes::TEXT_BOXES]);
    }

    #[test]
    fn button_fields_show_the_text_in_their_instruction() {
        let (doc, report) = read_reported(&docx(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MACROBUTTON  NoMacro [Click here] </w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t xml:space="preserve"> then </w:t></w:r><w:fldSimple w:instr="GOTOBUTTON top Back to top"/></w:p>"#,
        ))
        .unwrap();
        assert_eq!(doc.main.paras[0].text, "[Click here] then Back to top");
        assert_eq!(report.notes(), [notes::FIELDS]);
        assert_eq!(button_text("MACROBUTTON NoMacro"), None);
        assert_eq!(button_text("REF x"), None);
    }

    #[test]
    fn comments_in_notes_move_to_the_mark_and_notes_in_headers_stay_in_place() {
        let w = r#"xmlns:w="w" xmlns:r="r""#;
        let part = |name: &str, body: &str| format!(r#"<w:{name} {w}>{body}</w:{name}>"#);
        let mut zip = crate::zip::Writer::new(0);
        zip.add(
            "word/document.xml",
            part(
                "document",
                r#"<w:body><w:p><w:r><w:t>Main</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="h"/></w:sectPr></w:body>"#,
            )
            .as_bytes(),
        );
        zip.add(
            "word/footnotes.xml",
            part(
                "footnotes",
                r#"<w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>Note</w:t></w:r><w:r><w:commentReference w:id="5"/></w:r></w:p></w:footnote><w:footnote w:id="3"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t> Aside</w:t></w:r></w:p></w:footnote>"#,
            )
            .as_bytes(),
        );
        zip.add(
            "word/header1.xml",
            part(
                "hdr",
                r#"<w:p><w:r><w:t xml:space="preserve">Head </w:t></w:r><w:r><w:footnoteReference w:id="3"/></w:r><w:r><w:commentReference w:id="6"/></w:r></w:p>"#,
            )
            .as_bytes(),
        );
        zip.add(
            "word/comments.xml",
            part(
                "comments",
                r#"<w:comment w:id="5" w:author="A"><w:p><w:r><w:t>Comment</w:t></w:r></w:p></w:comment>"#,
            )
            .as_bytes(),
        );
        zip.add(
            "word/_rels/document.xml.rels",
            br#"<Relationships><Relationship Id="h" Type="x/header" Target="header1.xml"/><Relationship Id="f" Type="x/footnotes" Target="footnotes.xml"/><Relationship Id="c" Type="x/comments" Target="comments.xml"/></Relationships>"#,
        );
        let (doc, report) = read_reported(&zip.finish()).unwrap();
        let text = |story| doc.story(story).paras[0].text.clone();
        assert_eq!(text(StoryId::Main), "Main\u{E003}\u{E004}");
        assert_eq!(text(StoryId::Notes), "\u{E003}Note");
        assert_eq!(text(StoryId::Annotations), "\u{E004}Comment");
        assert_eq!(text(StoryId::Header), "Head Aside");
        assert_eq!(
            report.notes(),
            [
                notes::ASIDE_COMMENTS,
                notes::ASIDE_NOTES,
                notes::NOTE_COMMENTS
            ]
        );
    }

    #[test]
    fn ruby_guide_text_follows_its_base_in_brackets() {
        let (doc, report) = read_reported(&docx(
            "<w:p><w:r><w:ruby><w:rubyPr/><w:rt><w:r><w:t>\u{304B}\u{3093}\u{3058}</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>\u{6F22}\u{5B57}</w:t></w:r></w:rubyBase></w:ruby></w:r><w:r><w:ruby><w:rt><w:r><w:t>guide</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>base</w:t></w:r></w:rubyBase></w:ruby></w:r></w:p>",
        ))
        .unwrap();
        assert_eq!(
            doc.main.paras[0].text,
            "\u{6F22}\u{5B57}\u{FF08}\u{304B}\u{3093}\u{3058}\u{FF09}base(guide)"
        );
        assert_eq!(report.notes(), [notes::RUBY]);
    }

    #[test]
    fn floating_pictures_and_text_boxes_become_frames_before_their_paragraph() {
        use quill_core::{FrameH, FrameRel, FrameV};
        let anchor = |pos: &str, inner: &str| {
            format!(
                r#"<w:r><mc:AlternateContent><mc:Choice><w:drawing><wp:anchor distL="114300" distR="114300"><wp:positionH relativeFrom="{pos}"><wp:align>right</wp:align></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx="1828800" cy="914400"/>{inner}</wp:anchor></w:drawing></mc:Choice><mc:Fallback/></mc:AlternateContent></w:r>"#
            )
        };
        let boxed = anchor(
            "margin",
            "<wps:txbx><w:txbxContent><w:p><w:r><w:t>Boxed</w:t></w:r></w:p></w:txbxContent></wps:txbx>",
        );
        let doc = read(&docx(&format!(
            r#"<w:p><w:r><w:t>Text</w:t></w:r>{boxed}</w:p><w:tbl><w:tr><w:tc><w:p>{boxed}</w:p></w:tc></w:tr></w:tbl>"#
        )))
        .unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts[..4], ["Boxed", "Text", "", "Boxed"]);
        let frame = doc.main.paras[0].pap.frame.unwrap();
        assert_eq!(
            (frame.h, frame.h_rel, frame.v, frame.width, frame.from_text),
            (
                FrameH::Right,
                FrameRel::Margin,
                FrameV::InLine,
                Some(2880),
                180
            )
        );
        // Not in a table, where the original has no frames.
        assert!(doc.main.paras[3].pap.frame.is_none());
    }

    #[test]
    fn old_vml_text_boxes_float_when_placed_absolutely() {
        use quill_core::{FrameH, FrameRel, FrameV};
        let shape = |style: &str, wrap: &str| {
            format!(
                r#"<w:r><w:pict><v:shape style="{style}">{wrap}<v:textbox><w:txbxContent><w:p><w:r><w:t>Box</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r>"#
            )
        };
        let doc = read(&docx(&format!(
            "<w:p><w:r><w:t>Text</w:t></w:r>{}</w:p><w:p>{}</w:p>",
            shape("position:absolute;margin-left:1in;margin-top:36pt;width:2in;height:1in;mso-position-horizontal-relative:page;mso-position-vertical-relative:page;mso-wrap-distance-left:9pt", r#"<w10:wrap type="square"/>"#),
            shape("width:2in;height:1in", ""),
        )))
        .unwrap();
        let texts: Vec<&str> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Box", "Text", "", "Box"]);
        let f = doc.main.paras[0].pap.frame.unwrap();
        assert_eq!(
            (f.h, f.h_rel, f.v, f.v_rel, f.width, f.from_text),
            (
                FrameH::At(1440),
                FrameRel::Page,
                FrameV::At(720),
                FrameRel::Page,
                Some(2880),
                180
            )
        );
        assert!(doc.main.paras[3].pap.frame.is_none());
        // In front of the text, which runs under it: it stays in the text.
        let doc = read(&docx(&format!(
            "<w:p>{}</w:p>",
            shape(
                "position:absolute;margin-left:1in;width:2in;height:1in;z-index:-1",
                ""
            )
        )))
        .unwrap();
        assert!(doc.main.paras.iter().all(|p| p.pap.frame.is_none()));
        assert_eq!(vml_length("2.54cm"), Some(1440));
        assert_eq!(vml_length("96"), Some(1440));
        assert_eq!(vml_length("wide"), None);
    }

    #[test]
    fn list_numbers_with_a_leading_zero_or_as_ordinals() {
        assert_eq!(crate::numfmt::format_number(1, "decimalZero", None), "01");
        assert_eq!(crate::numfmt::format_number(12, "decimalZero", None), "12");
        assert_eq!(
            [1, 2, 3, 4, 11, 12, 13, 21, 102]
                .map(|n| crate::numfmt::format_number(n, "ordinal", None)),
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "102nd"
            ]
        );
    }

    #[test]
    fn solid_shading_preserves_colors_without_a_conversion_warning() {
        let body = "<w:p><w:pPr><w:shd w:fill='EBF0FA'/></w:pPr><w:r><w:t>blue</w:t></w:r></w:p>\
            <w:tbl><w:tblGrid><w:gridCol w:w='3000'/></w:tblGrid><w:tr><w:tc>\
            <w:tcPr><w:shd w:fill='FF0000'/></w:tcPr><w:p><w:r><w:t>red cell</w:t></w:r></w:p>\
            </w:tc></w:tr></w:tbl>";
        let (doc, report) = read_reported(&docx(body)).unwrap();
        assert!(report.is_empty());
        assert_eq!(doc.main.paras[0].pap.shading_color, Some(Rgb(0xEBF0FA)));
        let cell = doc.main.paras.iter().find_map(|p| p.row.as_ref()).unwrap();
        assert_eq!(cell.cells[0].shading_color, Some(Rgb(0xFF0000)));
        let gray = "<w:p><w:pPr><w:shd w:fill='808080'/></w:pPr></w:p>";
        assert!(read_reported(&docx(gray)).unwrap().1.is_empty());
    }

    #[test]
    fn unsupported_color_patterns_and_theme_only_fills_still_warn() {
        let body = "<w:p><w:pPr><w:shd w:val='pct25' w:fill='FF0000'/></w:pPr></w:p>\
            <w:p><w:pPr><w:shd w:themeFill='accent1'/></w:pPr></w:p>";
        let (doc, report) = read_reported(&docx(body)).unwrap();
        assert_eq!(report.notes(), [notes::SHADING_COLORS]);
        assert_eq!(doc.main.paras[0].pap.shading, 250);
        assert_eq!(doc.main.paras[0].pap.shading_color, None);
    }

    #[test]
    fn excessive_shading_percentages_are_bounded_before_multiplication() {
        for (pattern, expected) in [
            ("pct65535", 1000),
            ("pct100", 1000),
            ("pct12", 125),
            ("pct0", 0),
        ] {
            let body = format!(
                "<w:p><w:pPr><w:shd w:val='{pattern}'/></w:pPr><w:r><w:t>kept</w:t></w:r></w:p>"
            );
            let doc = read(&docx(&body)).unwrap();
            assert_eq!(doc.main.paras[0].pap.shading, expected);
            assert_eq!(doc.main.paras[0].text, "kept");
        }
    }

    #[test]
    fn macros_are_reported() {
        let xml = br#"<?xml version="1.0"?><w:document xmlns:w="w"><w:body><w:p><w:r><w:t>text</w:t></w:r></w:p></w:body></w:document>"#;
        let mut zip = crate::zip::Writer::new(0);
        zip.add("word/document.xml", xml);
        zip.add("word/vbaProject.bin", b"macros");
        let (doc, report) = read_reported(&zip.finish()).unwrap();
        assert_eq!(doc.main.paras[0].text, "text");
        assert_eq!(report.notes(), [notes::MACROS]);
    }

    #[test]
    fn equations_are_reported_and_deleted_text_stays_a_tracked_change() {
        let (doc, report) = read_reported(&docx(
            "<w:p><w:r><w:t>kept</w:t></w:r><w:del w:author=\"Ada\" w:date=\"2026-10-23T10:00:00Z\"><w:r><w:delText>gone</w:delText></w:r></w:del><m:oMath xmlns:m=\"m\"><m:r><m:t>a=1</m:t></m:r></m:oMath></w:p>",
        ))
        .unwrap();
        let p = &doc.main.paras[0];
        assert_eq!(p.text, "keptgonea = 1");
        let gone = quill_core::revision::get(p.chp_of_char(4).rev).unwrap();
        assert_eq!(gone.deleted.as_ref().unwrap().author, "Ada");
        assert_eq!(quill_core::text::to_plain_text(&doc, "\n"), "kepta = 1");
        assert_eq!(report.notes(), [notes::EQUATIONS]);
    }

    #[test]
    fn paragraph_format_changes_are_accepted_and_said_so() {
        let (_, report) = read_reported(&docx(
            "<w:p><w:pPr><w:jc w:val=\"center\"/><w:pPrChange w:id=\"1\" w:author=\"Ada\"><w:pPr/></w:pPrChange></w:pPr><w:r><w:t>x</w:t></w:r></w:p>",
        ))
        .unwrap();
        assert_eq!(report.notes(), [notes::FORMAT_CHANGES]);
    }

    #[test]
    fn word_generated_paragraph_style_names_are_canonical() {
        assert_eq!(
            canonical_paragraph_style_name("FootnoteReference1", "Footnote Reference1"),
            "Footnote Reference"
        );
        assert_eq!(
            canonical_paragraph_style_name("PageNumber1", "Page Number1"),
            "Page Number"
        );
        assert_eq!(
            canonical_paragraph_style_name("UserStyle1", "User Style1"),
            "User Style1"
        );
    }

    #[test]
    fn separator_references_do_not_reset_note_settings() {
        let xml = br#"<w:settings xmlns:w="w"><w:footnotePr><w:footnote w:id="-1"/></w:footnotePr></w:settings>"#;
        let element = xml::parse(xml).unwrap();
        let mut props = quill_core::DocumentProps {
            footnote_position: FootnotePosition::BeneathText,
            footnote_restart_each_page: true,
            footnote_start: 7,
            ..Default::default()
        };
        read_note_properties(&element, &mut props);
        assert_eq!(props.footnote_position, FootnotePosition::BeneathText);
        assert!(props.footnote_restart_each_page);
        assert_eq!(props.footnote_start, 7);
    }

    #[test]
    fn omitted_frame_anchors_follow_microsoft_words_text_default() {
        let xml = br#"<w:framePr xmlns:w="w" w:x="100" w:y="200"/>"#;
        let element = xml::parse(xml).unwrap();
        let frame = read_frame(&element);
        assert_eq!(frame.h_rel, quill_core::FrameRel::Column);
        assert_eq!(frame.v_rel, quill_core::FrameRel::Column);
    }
}
