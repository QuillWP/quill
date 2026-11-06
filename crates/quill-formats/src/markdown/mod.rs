//! Markdown: opening and saving it, and the plain-text form Source view
//! edits.

mod loss;
mod read;
mod write;

pub use loss::losses;
pub use read::read;
pub use write::{PictureFile, write, write_with_pictures};

use quill_core::Document;

/// The font code and Source view are set in.
pub const MONO: &str = "Courier New";

/// Whether a font is a fixed-width one, by its name.
pub(crate) fn is_mono(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "courier",
        "mono",
        "consolas",
        "menlo",
        "lucida console",
        "andale",
        "source code",
        "typewriter",
    ]
    .iter()
    .any(|k| name.contains(k))
}

/// The bullet or number a paragraph's text starts with (before its tab),
/// if it is a typed list item.
pub(crate) fn list_label_of(text: &str) -> Option<(bool, String)> {
    write::list_label(text)
}

/// A document holding `text` as it stands, one paragraph per line, in a
/// fixed-width font: what View > Markdown > Source edits.
pub fn source_document(text: &str) -> Document {
    let mut doc = quill_core::text::from_plain_text(text);
    let mono = doc.font_id(MONO);
    let set = |c: &mut quill_core::CharProps| {
        c.font = mono;
        c.size = 20;
    };
    set(&mut doc.styles.styles[0].chp);
    for p in &mut doc.main.paras {
        let len = p.len();
        p.format_range(0, len, &mut |c| set(c));
        set(&mut p.mark_chp);
    }
    doc.forget_history();
    doc
}

/// The text of a Source view document.
pub fn source_text(doc: &Document) -> String {
    quill_core::text::to_plain_text(doc, "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::para::chars;

    fn texts(doc: &Document) -> Vec<String> {
        doc.main.paras.iter().map(|p| p.text.clone()).collect()
    }

    const SAMPLE: &str = "\
# Title

Some *italic*, **bold**, ***both***, ~~struck~~ and `code` text with a [link](https://example.com/a_b) and <u>under</u>.

## Lists

- one
- two
    - nested
- three

1. first
2. second

> quoted text
> over two lines

---

```
fn main() {}
```

| Name | Count |
| --- | ---: |
| apples | 3 |
| pears | 12 |
";

    #[test]
    fn reads_the_blocks_of_a_document() {
        let (doc, report) = read(SAMPLE, None);
        assert!(report.is_empty(), "{:?}", report.notes());
        let t = texts(&doc);
        assert_eq!(t[0], "Title");
        assert_eq!(
            doc.styles.heading_level(doc.main.paras[0].pap.style),
            Some(1)
        );
        assert_eq!(
            t[1],
            "Some italic, bold, both, struck and code text with a link and under."
        );
        assert_eq!(
            doc.styles.heading_level(doc.main.paras[2].pap.style),
            Some(2)
        );
        assert_eq!(
            &t[3..6],
            ["\u{2022}\tone", "\u{2022}\ttwo", "\u{25E6}\tnested"]
        );
        assert_eq!(t[6], "\u{2022}\tthree");
        assert_eq!(&t[7..9], ["1.\tfirst", "2.\tsecond"]);
        assert_eq!(t[9], "quoted text over two lines");
        assert_eq!(
            doc.main.paras[9].pap.borders.left.style,
            quill_core::props::BorderStyle::Single
        );
        assert!(doc.main.paras[10].text.is_empty());
        assert_eq!(t[11], "fn main() {}");
        assert!(doc.main.paras[11].pap.shading > 0);
        // The table: two rows of two cells and a row end each, then the
        // paragraph a document ends with.
        let table: Vec<_> = doc.main.paras[12..]
            .iter()
            .map(|p| (p.text.as_str(), p.mark))
            .collect();
        use quill_core::Mark::*;
        assert_eq!(
            table,
            [
                ("Name", CellEnd),
                ("Count", CellEnd),
                ("", RowEnd),
                ("apples", CellEnd),
                ("3", CellEnd),
                ("", RowEnd),
                ("pears", CellEnd),
                ("12", CellEnd),
                ("", RowEnd),
                ("", Paragraph),
            ]
        );
        assert_eq!(doc.main.paras[13].pap.align, quill_core::Align::Right);
    }

    #[test]
    fn reads_character_formats_and_links() {
        let (doc, _) = read("a *b* **c** ~~d~~ [e](https://x.org/) `f`", None);
        let p = &doc.main.paras[0];
        let look = |s: &str| p.chp_of_char(p.text.find(s).unwrap());
        assert!(look("b").italic && !look("b").bold);
        assert!(look("c").bold);
        assert!(look("d").strike);
        assert_eq!(quill_core::link::url(look("e").link), "https://x.org/");
        assert!(is_mono(doc.font_name(look("f").font)));
        assert!(!look("a").italic);
    }

    #[test]
    fn reference_links_escapes_entities_and_breaks() {
        let (doc, _) = read(
            "[site][s] and [s] \\*not\\* &amp; &#65;\nsoft  \nhard\\\nlast\n\n[s]: https://s.example/ \"title\"\n",
            None,
        );
        let p = &doc.main.paras[0];
        assert_eq!(
            p.text,
            format!(
                "site and s *not* & A soft{b}hard{b}last",
                b = chars::LINE_BREAK
            )
        );
        let link = p.chp_of_char(0).link;
        assert_eq!(quill_core::link::url(link), "https://s.example/");
        assert_eq!(doc.main.paras.len(), 1);
    }

    #[test]
    fn setext_headings_and_indented_code_and_lazy_quotes() {
        let (doc, _) = read(
            "Big\n===\n\nSmall\n---\n\n    code line\n\n> lazy\ncontinued\n",
            None,
        );
        let level = |i: usize| doc.styles.heading_level(doc.main.paras[i].pap.style);
        assert_eq!((level(0), level(1)), (Some(1), Some(2)));
        assert_eq!(doc.main.paras[2].text, "code line");
        assert_eq!(doc.main.paras[3].text, "lazy continued");
    }

    #[test]
    fn html_and_footnote_markers_are_kept_as_text_and_reported() {
        let (doc, report) = read(
            "<div>raw</div>\n\nText with <span>tag</span>[^1]\n\n[^1]: note\n",
            None,
        );
        assert_eq!(doc.main.paras[0].text, "<div>raw</div>");
        assert!(report.notes().contains(&crate::report::MARKDOWN_HTML));
        assert!(report.notes().contains(&crate::report::MARKDOWN_NOTES));
    }

    #[test]
    fn front_matter_is_kept_and_written_back_unfenced() {
        let text = "---\ntitle: Hi\n---\n\n# Head\n";
        let (doc, _) = read(text, None);
        assert_eq!(doc.main.paras[0].text, "---");
        assert_eq!(write(&doc), text);
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let (doc, _) = read(SAMPLE, None);
        let first = write(&doc);
        let (again, report) = read(&first, None);
        assert!(report.is_empty());
        assert_eq!(texts(&doc), texts(&again));
        assert_eq!(write(&again), first);
    }

    #[test]
    fn writes_the_marks_the_text_needs() {
        let (doc, _) = read(
            "# Head\n\nA **bold *and italic* run**, `` a ` b `` tick, snake_case, a\\_b.\n\n1\\. not a list\n\n\\# no\n\n1. one\n2. two\n\n- x\n  more\n",
            None,
        );
        let out = write(&doc);
        let (back, _) = read(&out, None);
        assert_eq!(texts(&back), texts(&doc), "{out}");
        assert!(out.contains("**bold *and italic* run**"), "{out}");
        assert!(out.contains("snake_case"), "{out}");
        assert!(out.contains("\n1\\. not a list"), "{out}");
        assert!(out.contains("\n\\# no"), "{out}");
        assert!(out.contains("``a ` b``"), "{out}");
    }

    #[test]
    fn spaces_stay_outside_marks() {
        let mut doc = Document::new();
        let bold = quill_core::CharProps {
            bold: true,
            ..doc.styles.get(0).chp
        };
        let plain = doc.styles.get(0).chp;
        let mut p = quill_core::Paragraph::new(doc.styles.get(0).pap.clone(), plain);
        p.insert(0, "a", plain);
        p.insert(1, " word ", bold);
        p.insert(7, "b", plain);
        doc.main.paras = vec![p];
        assert_eq!(write(&doc), "a **word** b\n");
    }

    #[test]
    fn nested_lists_quotes_and_continuations() {
        let text = "- a\n    - b\n\n    para in b\n- c\n\n> q1\n>\n> q2\n>\n> > deeper\n";
        let (doc, _) = read(text, None);
        let out = write(&doc);
        let (back, _) = read(&out, None);
        assert_eq!(texts(&back), texts(&doc), "{out}");
        assert_eq!(write(&back), out);
    }

    #[test]
    fn tables_keep_alignment_and_header() {
        let text = "| L | C | R |\n| :--- | :---: | ---: |\n| a | b | c |\n";
        let (doc, _) = read(text, None);
        let out = write(&doc);
        assert_eq!(
            out,
            "| L | C | R |\n| --- | :---: | ---: |\n| a | b | c |\n"
        );
    }

    #[test]
    fn pictures_are_kept_as_reference_definitions() {
        // A 1 x 1 PNG.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let text = format!("Look: ![dot][d] here\n\n[d]: data:image/png;base64,{png}\n");
        let (doc, report) = read(&text, None);
        assert!(report.is_empty(), "{:?}", report.notes());
        assert_eq!(doc.objects.len(), 1);
        assert!(doc.main.paras[0].text.contains(chars::OBJECT));
        let out = write(&doc);
        assert!(out.starts_with("Look: ![Picture][img1] here\n\n[img1]: data:image/png;base64,"));
        let (back, _) = read(&out, None);
        assert_eq!(back.objects.len(), 1);
    }

    #[test]
    fn a_picture_that_is_not_found_leaves_its_description() {
        let (doc, report) = read("![a cat](missing.png) and ![](https://x.org/a.png)", None);
        assert_eq!(doc.main.paras[0].text, "a cat and [image]");
        assert!(report.notes().contains(&crate::report::MARKDOWN_PICTURES));
        assert!(
            report
                .notes()
                .contains(&crate::report::MARKDOWN_LINKED_PICTURES)
        );
    }

    #[test]
    fn a_picture_on_the_web_stays_a_link_to_it() {
        let text = "A ![logo [v2]](https://x.org/a_b.png) and ![](https://x.org/c.png).\n";
        let (doc, _) = read(text, None);
        assert_eq!(doc.objects.len(), 0);
        let p = &doc.main.paras[0];
        assert_eq!(p.text, "A [image: logo [v2]] and [image].");
        let link = p.chp_at(3).link;
        assert_ne!(link, 0);
        assert_eq!(quill_core::link::url(link), "https://x.org/a_b.png");
        // Written back as the same picture references, not as text links.
        let out = write(&doc);
        assert_eq!(
            out,
            "A ![logo \\[v2\\]](https://x.org/a_b.png) and ![](https://x.org/c.png).\n"
        );
        let (again, _) = read(&out, None);
        assert_eq!(write(&again), out);
    }

    #[test]
    fn bare_web_addresses_become_links() {
        let text = "See https://example.com/a_b?x=1, www.example.org. (http://x.io/p) and **https://b.com**.\n";
        let (doc, _) = read(text, None);
        let p = &doc.main.paras[0];
        let links: Vec<(String, String)> = p
            .runs
            .iter()
            .scan(0, |at, run| {
                let seg = p.text[*at..*at + run.len].to_string();
                *at += run.len;
                Some((seg, run.chp.link))
            })
            .filter(|(_, l)| *l != 0)
            .map(|(t, l)| (t, quill_core::link::url(l)))
            .collect();
        assert_eq!(
            links,
            [
                (
                    "https://example.com/a_b?x=1".into(),
                    "https://example.com/a_b?x=1".into()
                ),
                ("www.example.org".into(), "http://www.example.org".into()),
                ("http://x.io/p".into(), "http://x.io/p".into()),
                ("https://b.com".into(), "https://b.com".into()),
            ]
        );
        assert!(p.text.contains(", "));
        // Inside a link's own text an address is not linked a second time, and
        // a word that merely ends in "www." or "http" is left alone.
        let (doc, _) = read(
            "[https://a.org](https://b.org) awww.example xhttp://y.org",
            None,
        );
        let p = &doc.main.paras[0];
        assert_eq!(
            quill_core::link::url(p.chp_of_char(0).link),
            "https://b.org"
        );
        assert_eq!(p.chp_at(p.text.len() - 1).link, 0);
        // Written and read again, an address is stable.
        let (doc, _) = read("go to http://a.org/x now\n", None);
        let out = write(&doc);
        assert_eq!(out, "go to http://a.org/x now\n");
        assert_eq!(write(&read(&out, None).0), out);
    }

    #[test]
    fn saving_copies_the_pictures_into_a_folder_beside_the_file() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let (doc, _) = read(
            &format!("One ![a][d] and ![b][d].\n\n[d]: data:image/png;base64,{png}\n"),
            None,
        );
        let (text, files) = write_with_pictures(&doc, "Trip_images");
        assert_eq!(files.len(), 1);
        let name = &files[0].name;
        assert!(name.starts_with("image1-") && name.ends_with(".png"));
        assert_eq!(
            text,
            format!("One ![Picture](Trip_images/{name}) and ![Picture](Trip_images/{name}).\n")
        );
        // Through save: the folder is made and the Markdown opens with the pictures.
        let dir = std::env::temp_dir().join(format!("quill-md-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Trip 2026.md");
        crate::save(&doc, &path, crate::Format::Markdown).unwrap();
        assert!(dir.join("Trip_2026_images").join(name).is_file());
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains(&format!("(Trip_2026_images/{name})")),
            "{saved}"
        );
        assert!(!saved.contains("base64"));
        let (back, report) = read(&saved, Some(&dir));
        assert!(report.is_empty(), "{:?}", report.notes());
        assert_eq!(back.objects.len(), 1);
        // A document with no pictures makes no folder.
        let plain = dir.join("Plain.md");
        crate::save(
            &read("Just text\n", None).0,
            &plain,
            crate::Format::Markdown,
        )
        .unwrap();
        assert!(!dir.join("Plain_images").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_save_does_not_replace_pictures_used_by_the_old_markdown() {
        let dir = std::env::temp_dir().join(format!("quill-md-failed-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Notes.md");
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let (doc, _) = read(&format!("![a](data:image/png;base64,{png})"), None);
        crate::save(&doc, &path, crate::Format::Markdown).unwrap();
        let (_, files) = write_with_pictures(&doc, "Notes_images");
        let picture = dir.join("Notes_images").join(&files[0].name);
        let original = std::fs::read(&picture).unwrap();
        let mut changed = doc.clone();
        let quill_core::Object::Picture(pic) = &mut changed.objects[0] else {
            panic!("expected a picture")
        };
        // A PNG with trailing bytes is still readable but has different content.
        let mut bytes = pic.data.to_vec();
        bytes.push(0);
        pic.data = bytes.into();
        let (_, changed_files) = write_with_pictures(&changed, "Notes_images");
        assert_ne!(files[0].name, changed_files[0].name);
        // A directory at the final destination makes the final replace fail.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(crate::save(&changed, &path, crate::Format::Markdown).is_err());
        assert_eq!(std::fs::read(&picture).unwrap(), original);
        // A conflicting user-owned file must also survive an otherwise valid save.
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, b"old Markdown").unwrap();
        std::fs::write(&picture, b"keep this existing file").unwrap();
        assert!(crate::save(&doc, &path, crate::Format::Markdown).is_err());
        assert_eq!(std::fs::read(&picture).unwrap(), b"keep this existing file");
        assert_eq!(std::fs::read(&path).unwrap(), b"old Markdown");
        #[cfg(unix)]
        {
            let other = dir.join("Other.md");
            let outside = dir.join("outside");
            std::fs::create_dir(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("Other_images")).unwrap();
            assert!(crate::save(&doc, &other, crate::Format::Markdown).is_err());
            assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_document_from_markdown_loses_nothing() {
        let (doc, _) = read(SAMPLE, None);
        assert_eq!(
            losses(&doc, &quill_core::DocumentProps::default()),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn features_markdown_cannot_carry_are_named() {
        let mut doc = Document::new();
        let font = doc.font_id("Zapfino");
        let mut chp = doc.styles.get(0).chp;
        chp.font = font;
        chp.color = quill_core::Rgb(0xFF0000);
        chp.small_caps = true;
        let mut pap = doc.styles.get(0).pap.clone();
        pap.align = quill_core::Align::Center;
        pap.tabs.push(quill_core::TabStop {
            pos: 1440,
            ..Default::default()
        });
        doc.main.paras = vec![quill_core::Paragraph::with_text("Styled", chp, pap)];
        doc.props.page_width = 11906;
        doc.summary.author = "Someone".into();
        let found = losses(&doc, &quill_core::DocumentProps::default());
        for expected in [
            loss::FONTS,
            loss::COLORS,
            loss::EFFECTS,
            loss::ALIGNMENT,
            loss::TABS,
            loss::PAGE,
            loss::SUMMARY,
        ] {
            assert!(
                found.contains(&expected),
                "{expected} missing from {found:?}"
            );
        }
        assert!(!found.contains(&loss::HEADERS));
    }

    #[test]
    fn headers_footnotes_changes_and_text_boxes_are_named() {
        let mut doc = Document::new();
        let n = doc.styles.get(0).clone();
        let mut framed = quill_core::Paragraph::with_text("Boxed", n.chp, n.pap.clone());
        framed.pap.frame = Some(Default::default());
        let mut broken = quill_core::Paragraph::with_text("Next", n.chp, n.pap.clone());
        broken.pap.page_break_before = true;
        doc.main.paras = vec![framed, broken];
        doc.headers[0].paras = vec![quill_core::Paragraph::with_text(
            "Running head",
            n.chp,
            n.pap.clone(),
        )];
        let found = losses(&doc, &quill_core::DocumentProps::default());
        assert!(found.contains(&loss::FRAMES), "{found:?}");
        assert!(found.contains(&loss::BREAKS), "{found:?}");
        assert!(found.contains(&loss::HEADERS), "{found:?}");
    }

    #[test]
    fn source_view_text_is_the_markdown_itself() {
        let text = "# Head\n\n*x*  \n";
        let doc = source_document(text);
        assert_eq!(source_text(&doc), text);
        assert!(is_mono(doc.font_name(doc.main.paras[0].chp_at(0).font)));
        assert!(!doc.is_modified());
    }

    #[test]
    fn saves_and_opens_files_by_extension() {
        let dir = std::env::temp_dir().join(format!("quill-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.md", "b.markdown"] {
            let path = dir.join(name);
            std::fs::write(&path, SAMPLE).unwrap();
            let doc = crate::load(&path).unwrap();
            assert_eq!(doc.main.paras[0].text, "Title");
            let out = dir.join(format!("out-{name}"));
            crate::save(&doc, &out, crate::Format::Markdown).unwrap();
            let text = std::fs::read_to_string(&out).unwrap();
            assert_eq!(text, write(&doc));
            assert_eq!(
                crate::load(&out).unwrap().main.paras.len(),
                doc.main.paras.len()
            );
        }
        assert_eq!(
            crate::Format::from_path(std::path::Path::new("x.MD")),
            Some(crate::Format::Markdown)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_too_big_for_quill_is_refused_before_it_is_built() {
        let dir = std::env::temp_dir().join(format!("quill-md-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.md");
        std::fs::write(&path, "x\n".repeat(quill_core::limits::MAX_PARAGRAPHS + 1)).unwrap();
        let error = crate::load(&path).err().unwrap_or_default();
        assert!(error.contains("too large"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_input_stays_bounded() {
        let deep = "> ".repeat(500) + "x";
        let (doc, _) = read(&deep, None);
        assert!(!doc.main.paras.is_empty());
        let list = (0..200)
            .map(|k| format!("{}- x", " ".repeat(k * 2)))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = read(&list, None);
        let brackets = "[".repeat(5000) + &"]".repeat(5000);
        let _ = read(&brackets, None);
        let _ = read(&"*a ".repeat(5000), None);
        let wide = format!(
            "{}\n{}\n{}",
            "|a".repeat(400) + "|",
            "|-".repeat(400) + "|",
            "|b".repeat(400) + "|"
        );
        let (doc, _) = read(&wide, None);
        assert!(doc.main.paras.len() < 2000);
    }
}
