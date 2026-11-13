//! The kitchen-sink round trip: one document using every feature Quill
//! keeps, saved as .docx and read back, compared on everything a document
//! keeps (not undo history, caches or the modified flag). Any difference is
//! something a user would lose by saving, so the test lists them all.

use quill_core::document::FootnotePosition;
use quill_core::notes::AnnotationInfo;
use quill_core::para::chars;
use quill_core::props::{BorderStyle, Frame, FrameH, FrameRel, FrameV};
use quill_core::table::{row_at, row_props};
use quill_core::{
    Align, CharProps, Crop, Document, LineNumbering, LineSpacing, PageNumberFormat, ParaProps,
    Picture, Position, Rgb, RowHeight, SectionStart, Selection, Story, StoryId, TabAlign,
    TabLeader, TabStop, Underline,
};

mod common;

/// A small PNG, so the picture is real data the reader has to keep.
fn png(w: u32, h: u32) -> Vec<u8> {
    common::png(w, h, |x, y| [(x * 40) as u8, (y * 40) as u8, 128])
}

/// Type `text` in `chp` at the end of main paragraph `para`.
fn append(doc: &mut Document, para: usize, text: &str, chp: CharProps) {
    let at = Position::new(para, doc.main.paras[para].len());
    doc.insert_text(StoryId::Main, at, text, chp);
}

/// The document: every character and paragraph format, special
/// characters, frames and a drop cap, footnotes with numbers and with
/// marks of their own, an annotation, a scaled and cropped picture, a
/// custom style, a table with merged cells, two sections, all six headers
/// and footers, and the document's own settings.
fn kitchen_sink() -> Document {
    let mut doc = quill_core::text::from_plain_text(
        "Kitchen sink\n\nParagraph formatting\n\nFramed paragraph\nD\nrop cap paragraph\n\
         Notes here\n\nCustom style paragraph\nAt least\nMultiple\nBefore table\nAfter table\n\
         End of section one\nLast section",
    );
    let base = doc.main.paras[1].mark_chp;
    let h1 = doc.styles.find("heading 1").unwrap();
    doc.apply_style(StoryId::Main, 0, 0, h1);

    // 1: every character format, one run each.
    let mut runs: Vec<(&str, CharProps)> = vec![
        ("plain ", base),
        ("bold ", CharProps { bold: true, ..base }),
        (
            "italic ",
            CharProps {
                italic: true,
                ..base
            },
        ),
        (
            "strike ",
            CharProps {
                strike: true,
                ..base
            },
        ),
        (
            "outline ",
            CharProps {
                outline: true,
                ..base
            },
        ),
        (
            "shadow ",
            CharProps {
                shadow: true,
                ..base
            },
        ),
        (
            "small caps ",
            CharProps {
                small_caps: true,
                ..base
            },
        ),
        (
            "all caps ",
            CharProps {
                all_caps: true,
                ..base
            },
        ),
        (
            ".i.Index entry;",
            CharProps {
                hidden: true,
                ..base
            },
        ),
        (
            " red ",
            CharProps {
                color: Rgb(0xCC2200),
                ..base
            },
        ),
        (
            "raised",
            CharProps {
                position: 60,
                ..base
            },
        ),
        (
            "lowered",
            CharProps {
                position: -60,
                ..base
            },
        ),
        (
            " wide ",
            CharProps {
                spacing: 40,
                ..base
            },
        ),
        (
            "narrow ",
            CharProps {
                spacing: -20,
                ..base
            },
        ),
        ("big ", CharProps { size: 36, ..base }),
        (
            "a link",
            CharProps {
                link: quill_core::link::intern(quill_core::Link::new(
                    "https://example.com/a?b=1&c=\"d\"",
                )),
                color: quill_core::link::BROWSER_BLUE,
                underline: Underline::Single,
                ..base
            },
        ),
        (" ", base),
        (
            "a new window",
            CharProps {
                link: quill_core::link::intern(quill_core::Link {
                    new_window: true,
                    ..quill_core::Link::new("mailto:someone@example.com")
                }),
                color: quill_core::link::BROWSER_BLUE,
                underline: Underline::Single,
                ..base
            },
        ),
        (" ", base),
    ];
    let courier = doc.font_id("Courier");
    runs.push((
        "courier ",
        CharProps {
            font: courier,
            ..base
        },
    ));
    for u in [
        Underline::Single,
        Underline::Word,
        Underline::Double,
        Underline::Dotted,
    ] {
        runs.push((
            "under ",
            CharProps {
                underline: u,
                ..base
            },
        ));
    }
    for (text, chp) in runs {
        append(&mut doc, 1, text, chp);
    }

    // 2: every paragraph format.
    doc.format_paras(StoryId::Main, 2, 2, |p| {
        p.align = Align::Justify;
        p.left = 360;
        p.right = 720;
        p.first = -360;
        p.before = 120;
        p.after = 240;
        p.line = LineSpacing::Exactly(300);
        for (pos, align, leader) in [
            (720, TabAlign::Left, TabLeader::None),
            (1440, TabAlign::Center, TabLeader::Dots),
            (2160, TabAlign::Right, TabLeader::Dashes),
            (2880, TabAlign::Decimal, TabLeader::Underline),
            (3600, TabAlign::Bar, TabLeader::None),
        ] {
            p.set_tab(TabStop { pos, align, leader });
        }
        p.page_break_before = true;
        p.keep_with_next = true;
        p.keep_together = true;
        p.suppress_line_numbers = true;
        p.borders.top.style = BorderStyle::Single;
        p.borders.bottom.style = BorderStyle::Double;
        p.borders.bottom.space = 40;
        p.borders.left.style = BorderStyle::Thick;
        p.borders.right.style = BorderStyle::Dotted;
        p.borders.between.style = BorderStyle::Hairline;
        p.borders.shadow = true;
        p.shading = 250;
    });

    // 3: special characters and fields.
    append(
        &mut doc,
        3,
        &format!(
            "Tab\there, line{}break, no{}break, opt{}ional, non{}breaking, page {} date {} time {}, \
             page{}break",
            chars::LINE_BREAK,
            chars::NONBREAKING_SPACE,
            chars::OPTIONAL_HYPHEN,
            chars::NONBREAKING_HYPHEN,
            chars::FIELD_PAGE,
            chars::FIELD_DATE,
            chars::FIELD_TIME,
            chars::PAGE_BREAK,
        ),
        base,
    );

    // 4: a frame placed on the page; 5: a drop cap.
    doc.format_paras(StoryId::Main, 4, 4, |p| {
        p.frame = Some(Frame {
            h: FrameH::At(1440),
            h_rel: FrameRel::Page,
            v: FrameV::At(2880),
            v_rel: FrameRel::Margin,
            width: Some(2880),
            from_text: 180,
            drop_lines: 0,
            in_margin: false,
        });
    });
    doc.format_paras(StoryId::Main, 5, 5, |p| {
        p.frame = Some(Frame {
            drop_lines: 3,
            ..Frame::default()
        });
    });

    // 7: a numbered footnote, one with its own mark, and an annotation.
    let end = doc.main.paras[7].len();
    let numbered = doc.insert_note(Position::new(7, end), base, None);
    let end = doc.main.paras[7].len();
    let marked = doc.insert_note(Position::new(7, end), base, Some("*".into()));
    let end = doc.main.paras[7].len();
    let note = doc.insert_annotation(
        Position::new(7, end),
        base,
        AnnotationInfo {
            initials: "LK".into(),
            author: "Lucas".into(),
            date: "2026-10-15".into(),
            ..Default::default()
        },
    );
    for (id, text) in [(numbered, "First note."), (marked, "Starred note.")] {
        let at = doc.note_text_start(id).unwrap();
        let chp = doc.notes.paras[at.para].mark_chp;
        doc.insert_text(StoryId::Notes, at, text, chp);
    }
    let at = doc.annotation_text_start(note).unwrap();
    let chp = doc.annotations.paras[at.para].mark_chp;
    doc.insert_text(StoryId::Annotations, at, "A remark.", chp);

    // 8: a picture, scaled and cropped.
    let mut pic = Picture::from_bytes(png(6, 4)).unwrap();
    pic.size = (1440, 720);
    pic.crop = Crop {
        left: 100,
        top: 0,
        right: 200,
        bottom: 50,
    };
    doc.insert_picture(StoryId::Main, Position::new(8, 0), pic, base);

    // 9: a style of the document's own.
    let normal = doc.styles.find("Normal").unwrap();
    let pap = ParaProps {
        left: 720,
        after: 120,
        ..doc.styles.get(normal).pap.clone()
    };
    let chp = CharProps {
        italic: true,
        color: Rgb(0x2255AA),
        ..doc.styles.get(normal).chp
    };
    let mine = doc.add_style("Quotation", normal, Some(normal), pap, chp);
    doc.apply_style(StoryId::Main, 9, 9, mine);
    // A built-in style redefined but not used anywhere yet.
    let h3 = doc.styles.find("heading 3").unwrap();
    let (pap, chp) = {
        let st = doc.styles.get(h3);
        (
            ParaProps {
                before: 480,
                ..st.pap.clone()
            },
            CharProps {
                color: Rgb(0x008800),
                ..st.chp
            },
        )
    };
    doc.redefine_style(h3, pap, chp);

    // 10 and 11: the other line spacings.
    doc.format_paras(StoryId::Main, 10, 10, |p| {
        p.line = LineSpacing::AtLeast(360)
    });
    doc.format_paras(StoryId::Main, 11, 11, |p| {
        p.line = LineSpacing::Multiple(360)
    });

    // The section break at the end of 14, then the table before 13, so
    // the paragraph numbers above stay put.
    let end = doc.main.paras[14].len();
    doc.insert_section_break(Position::new(14, end));
    doc.format_sections(0, 0, |s| {
        s.start = SectionStart::NewPage;
        s.columns = 2;
        s.column_spacing = 540;
        s.page_number_format = PageNumberFormat::UpperRoman;
        s.restart_page_numbers = true;
        s.line_numbers = LineNumbering::ByPage;
        s.line_count_by = 5;
        s.line_from_text = Some(360);
        s.margin_page_numbers = true;
        s.page_number_from_top = 400;
        s.page_number_from_right = 300;
        s.different_first_page = true;
    });
    doc.insert_table(StoryId::Main, Position::new(13, 0), 2, 3, 2000);
    let r1 = row_at(&doc.main.paras, 13).unwrap();
    let mut p1 = row_props(&doc.main.paras, &r1, 8640);
    p1.cells[1].width = 3000;
    p1.cells[0].borders.top.style = BorderStyle::Single;
    p1.cells[2].shading = 100;
    p1.height = RowHeight::AtLeast(400);
    p1.indent = 360;
    p1.align = Align::Center;
    doc.set_row(StoryId::Main, r1.end, p1);
    let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
    let mut p2 = row_props(&doc.main.paras, &r2, 8640);
    p2.align = Align::Right;
    p2.indent = 200;
    p2.height = RowHeight::Exactly(500);
    doc.set_row(StoryId::Main, r2.end, p2);
    let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
    let p2 = row_props(&doc.main.paras, &r2, 8640);
    doc.merge_cells(StoryId::Main, &r2, &p2, 0, 1);
    let first_cell = r1.first;
    let chp = doc.main.paras[first_cell].mark_chp;
    doc.insert_text(StoryId::Main, Position::new(first_cell, 0), "Cell", chp);

    // The last section's own settings.
    doc.section.start = SectionStart::OddPage;
    doc.section.different_first_page = true;
    doc.section.header_from_top = 600;
    doc.section.footer_from_bottom = 700;
    doc.section.margin_page_numbers = true;
    doc.section.page_number_from_top = 500;
    doc.section.page_number_from_right = 600;

    // All six headers and footers.
    doc.props.even_odd_headers = true;
    for (k, id) in StoryId::HEADERS.into_iter().enumerate() {
        let mut story: Story = doc.story(id).clone();
        let chp = story.paras[0].mark_chp;
        story.paras[0].insert(0, &format!("{} {k}\t{}", id.name(), chars::FIELD_PAGE), chp);
        doc.set_story(id, story);
    }

    // A third section, sharing the second's (Same As Previous).
    let last = doc.main.paras.len() - 1;
    doc.insert_section_break(Position::new(last, 4));

    // The second section's own header (its footers stay the first's).
    doc.header_section = 1;
    let mut own: Story = doc.story(StoryId::Header).clone();
    let chp = own.paras[0].mark_chp;
    own.paras[0] =
        quill_core::Paragraph::with_text("Second section", chp, own.paras[0].pap.clone());
    doc.set_story(StoryId::Header, own);
    doc.header_section = 0;

    // A picture in the header.
    let mut logo = Picture::from_bytes(png(3, 2)).unwrap();
    logo.size = (720, 480);
    let chp = doc.story(StoryId::Header).paras[0].mark_chp;
    doc.insert_picture(StoryId::Header, Position::new(0, 0), logo, chp);

    // The document's settings and Summary Info.
    let p = &mut doc.props;
    p.page_width = 11906;
    p.page_height = 16838;
    p.margin_left = 1500;
    p.margin_right = 1600;
    p.margin_top = 1700;
    p.margin_bottom = 1800;
    p.top_exact = true;
    p.bottom_exact = true;
    p.print_hidden = true;
    p.gutter = 360;
    p.mirror_margins = true;
    p.default_tab = 360;
    p.widow_control = false;
    p.footnote_restart_each_page = true;
    p.footnote_start = 3;
    let s = &mut doc.summary;
    s.title = "Kitchen sink".into();
    s.subject = "Round trips".into();
    s.author = "Lucas".into();
    s.version = "2".into();
    s.keywords = "docx, test".into();
    doc
}

fn docx_losses(doc: &Document) -> Vec<String> {
    let bytes = quill_formats::docx::write(doc, 1_760_000_000);
    let back = quill_formats::docx::read(&bytes).expect("Quill reads its own .docx");
    common::differences(doc, &back)
}

fn rtf_losses(doc: &Document) -> Vec<String> {
    let bytes = quill_formats::rtf::write(doc);
    let back = quill_formats::rtf::read(&bytes).expect("Quill reads its own .rtf");
    common::differences(doc, &back)
}

#[test]
fn docx_keeps_everything() {
    let doc = kitchen_sink();
    // QUILL_KITCHEN_SINK=/path/sink.docx (or sink.qwp) saves the document,
    // to open in Word or Quill or to look inside.
    if let Ok(path) = std::env::var("QUILL_KITCHEN_SINK") {
        let bytes = if path.ends_with(".qwp") {
            quill_formats::native::write(&doc, SECS)
        } else {
            quill_formats::docx::write(&doc, SECS)
        };
        std::fs::write(path, bytes).unwrap();
    }
    // QUILL_WORD_RESAVED=/path/word-copy.docx checks the same document after
    // current Microsoft Word has opened and rewritten it. This keeps the
    // external interoperability check repeatable without putting Word in CI.
    if let Ok(path) = std::env::var("QUILL_WORD_RESAVED") {
        let bytes = std::fs::read(&path).expect("read Word-resaved DOCX");
        let back = quill_formats::docx::read(&bytes).expect("read Word-resaved DOCX in Quill");
        let diffs = common::differences(&doc, &back);
        assert!(
            diffs.is_empty(),
            "{} differences after Microsoft Word rewrote {path}:\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }
    let diffs = docx_losses(&doc);
    assert!(
        diffs.is_empty(),
        "{} differences after saving and reading .docx:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn docx_keeps_every_note_position() {
    for at in [
        FootnotePosition::BeneathText,
        FootnotePosition::EndOfSection,
        FootnotePosition::EndOfDocument,
    ] {
        let mut doc = kitchen_sink();
        doc.props.footnote_position = at;
        let diffs = docx_losses(&doc);
        assert!(
            diffs.is_empty(),
            "{} differences with notes {at:?}:\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }
}

#[test]
fn rtf_keeps_everything_it_can_represent() {
    let doc = kitchen_sink();
    let diffs = rtf_losses(&doc);
    assert!(
        diffs.is_empty(),
        "{} differences after saving and reading .rtf:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

// ----- .qwp, Quill's own format: nothing may change at all -----

const SECS: u64 = 1_760_000_000;

/// The document through .qwp and back, checked part by part.
fn quill_round_trip(doc: &Document) {
    let bytes = quill_formats::native::write(doc, SECS);
    let back = quill_formats::native::read(&bytes).expect("Quill reads its own .qwp");
    assert_eq!(back.fonts, doc.fonts);
    assert_eq!(back.styles, doc.styles);
    assert_eq!(back.props, doc.props);
    assert_eq!(back.section, doc.section);
    assert_eq!(back.summary, doc.summary);
    assert_eq!(back.main, doc.main);
    assert_eq!(back.notes, doc.notes);
    assert_eq!(back.annotations, doc.annotations);
    assert_eq!(back.headers, doc.headers);
    assert_eq!(back.objects, doc.objects);
    assert_eq!(back.note_marks, doc.note_marks);
    assert_eq!(back.annotation_info, doc.annotation_info);
    assert_eq!(common::describe(&back), common::describe(doc));
    // Saving what was read gives the same file, byte for byte.
    assert_eq!(quill_formats::native::write(&back, SECS), bytes);
}

#[test]
fn quill_keeps_everything_exactly() {
    quill_round_trip(&kitchen_sink());
    for at in [
        FootnotePosition::BeneathText,
        FootnotePosition::EndOfSection,
        FootnotePosition::EndOfDocument,
    ] {
        let mut doc = kitchen_sink();
        doc.props.footnote_position = at;
        quill_round_trip(&doc);
    }
    // A live table of contents keeps the settings it was made with.
    let mut doc = kitchen_sink();
    doc.props.live_toc = Some(quill_core::contents::TocOptions {
        outline: true,
        page_numbers: false,
        from: 2,
        to: 4,
    });
    quill_round_trip(&doc);
    quill_round_trip(&Document::new());
}

/// A document with every kind of tracked change: an insertion, a
/// deletion, someone else's insertion deleted, a format change, and an
/// inserted and a deleted paragraph mark.
fn tracked_changes() -> Document {
    // A fixed date, so a copy saved by Word earlier still compares equal.
    quill_core::revision::set_clock(|| "2026-10-23T09:00:00Z".to_string());
    let mut doc = quill_core::text::from_plain_text("First paragraph\nSecond one\nThird");
    doc.props.track_changes = true;
    doc.author = "Ada Lovelace".into();
    let at = |para, offset| Position::new(para, offset);
    doc.insert_text(StoryId::Main, at(0, 5), " new", CharProps::default());
    doc.delete(StoryId::Main, at(1, 0), at(1, 7));
    doc.format_chars(StoryId::Main, at(2, 0), at(2, 5), |c| c.bold = true);
    doc.split_paragraph(StoryId::Main, at(0, 9));
    doc.delete(StoryId::Main, at(2, 10), at(3, 0));
    doc.author = "Bo".into();
    doc.delete(StoryId::Main, at(0, 5), at(0, 7));
    assert!(doc.has_changes());
    doc
}

#[test]
fn quill_keeps_tracked_changes_exactly() {
    quill_round_trip(&tracked_changes());
}

/// A document with a comment on a stretch of text running into the next
/// paragraph, a reply to it, and a resolved comment at a point.
fn commented() -> Document {
    let mut doc = quill_core::text::from_plain_text("The first paragraph\nThe second one");
    let at = |para, offset| Position::new(para, offset);
    let info = |who: &str, date: &str| AnnotationInfo {
        initials: who[..1].to_string(),
        author: who.to_string(),
        date: date.to_string(),
        ..Default::default()
    };
    let main = doc.insert_annotation_on(
        at(0, 4),
        at(1, 3),
        CharProps::default(),
        info("Ada", "2026-10-23T09:00:00Z"),
    );
    let reply = doc
        .insert_reply(main, info("Bo", "2026-10-23T09:05:00Z"))
        .unwrap();
    let end = doc.main.paras[1].len();
    let done = doc.insert_annotation(
        at(1, end),
        CharProps::default(),
        info("Cy", "2026-10-23T10:00:00Z"),
    );
    doc.set_annotation_done(done, true);
    for (id, text) in [(main, "Is this right?"), (reply, "Yes."), (done, "Fixed.")] {
        let start = doc.annotation_text_start(id).unwrap();
        doc.insert_text(StoryId::Annotations, start, text, CharProps::default());
    }
    doc
}

/// Each annotation's text, writer, reply and resolved state, and the text
/// each one is about.
fn comments(d: &Document) -> Vec<(String, String, Option<String>, bool, String)> {
    quill_core::notes::annotation_ids(&d.main.paras)
        .into_iter()
        .map(|id| {
            let info = &d.annotation_info[&id];
            let about: String = d
                .main
                .paras
                .iter()
                .flat_map(|p| {
                    p.run_bounds()
                        .filter(|(_, _, c)| c.comment == id)
                        .map(|(s, e, _)| p.text[s..e].to_string())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
                .join("|");
            (
                d.annotation_text(id),
                format!("{} {}", info.author, info.date),
                info.parent.map(|p| d.annotation_text(p)),
                info.done,
                about,
            )
        })
        .collect()
}

#[test]
fn comments_keep_their_text_replies_and_resolved_state() {
    let doc = commented();
    let expected = comments(&doc);
    assert_eq!(expected[0].4, "first paragraph|The", "the range runs on");
    assert_eq!(expected[1].2.as_deref(), Some("Is this right?"));
    assert!(expected[2].3);
    quill_round_trip(&doc);
    // QUILL_COMMENTS_DOCX=/path/comments.docx saves it, to open in Word.
    if let Ok(path) = std::env::var("QUILL_COMMENTS_DOCX") {
        std::fs::write(path, quill_formats::docx::write(&doc, SECS)).unwrap();
    }
    let back = quill_formats::docx::read(&quill_formats::docx::write(&doc, SECS)).unwrap();
    assert_eq!(comments(&back), expected);
    let back = quill_formats::rtf::read(&quill_formats::rtf::write(&doc)).unwrap();
    assert_eq!(comments(&back), expected, "through RTF");
    // QUILL_COMMENTS_WORD=/path/word-copy.docx checks Word's own save of it.
    if let Ok(path) = std::env::var("QUILL_COMMENTS_WORD") {
        let word = quill_formats::docx::read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(comments(&word), expected);
    }
}

#[test]
fn rtf_keeps_tracked_changes() {
    let doc = tracked_changes();
    // QUILL_TRACKED_RTF=/path/tracked.rtf saves it, to open in Word (whose
    // .docx of it QUILL_TRACKED_WORD then checks).
    if let Ok(path) = std::env::var("QUILL_TRACKED_RTF") {
        std::fs::write(path, quill_formats::rtf::write(&doc)).unwrap();
    }
    let back = quill_formats::rtf::read(&quill_formats::rtf::write(&doc))
        .expect("Quill reads its own RTF");
    assert!(back.props.track_changes);
    let diffs = common::differences(&doc, &back);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    assert_eq!(revisions(&back), revisions(&doc));
    // QUILL_TRACKED_WORD_RTF=/path/word.rtf checks Word's own RTF of the
    // same document.
    if let Ok(path) = std::env::var("QUILL_TRACKED_WORD_RTF") {
        let bytes = std::fs::read(&path).expect("read Word's RTF");
        let word = quill_formats::rtf::read(&bytes).expect("read Word's RTF in Quill");
        assert!(word.props.track_changes);
        assert_eq!(revisions(&word), revisions(&doc));
    }
}

/// Every run's text and tracked change, and each paragraph mark's.
fn revisions(d: &Document) -> Vec<(String, Option<quill_core::revision::Revision>)> {
    d.main
        .paras
        .iter()
        .flat_map(|p| {
            p.run_bounds()
                .map(|(s, e, c)| {
                    (
                        p.text[s..e].to_string(),
                        quill_core::revision::get(c.rev).map(|r| (*r).clone()),
                    )
                })
                .chain([(
                    "¶".to_string(),
                    quill_core::revision::get(p.mark_chp.rev).map(|r| (*r).clone()),
                )])
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn docx_keeps_tracked_changes() {
    let doc = tracked_changes();
    // QUILL_TRACKED_DOCX=/path/tracked.docx saves it, to open in Word.
    if let Ok(path) = std::env::var("QUILL_TRACKED_DOCX") {
        std::fs::write(path, quill_formats::docx::write(&doc, SECS)).unwrap();
    }
    let bytes = quill_formats::docx::write(&doc, SECS);
    let back = quill_formats::docx::read(&bytes).expect("Quill reads its own .docx");
    assert!(back.props.track_changes);
    let diffs = common::differences(&doc, &back);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    assert_eq!(revisions(&back), revisions(&doc));
    // QUILL_TRACKED_WORD=/path/word-copy.docx checks the same document after
    // Microsoft Word has opened and saved it: every change must survive.
    if let Ok(path) = std::env::var("QUILL_TRACKED_WORD") {
        let bytes = std::fs::read(&path).expect("read Word's copy");
        let word = quill_formats::docx::read(&bytes).expect("read Word's copy in Quill");
        assert!(word.props.track_changes);
        assert_eq!(revisions(&word), revisions(&doc));
    }
}

#[test]
fn combined_feature_edits_undo_redo_and_save_together() {
    let mut doc = kitchen_sink();
    doc.forget_history();
    let before = quill_formats::native::write(&doc, SECS);
    let selection = Selection::caret(Position::new(0, 0));

    doc.begin("Combined document edit", selection);
    doc.format_sections(0, 0, |section| {
        section.columns = 3;
        section.column_spacing = 360;
    });
    let framed = doc
        .main
        .paras
        .iter()
        .position(|para| para.pap.frame.is_some())
        .unwrap();
    doc.format_paras(StoryId::Main, framed, framed, |props| {
        props.frame.as_mut().unwrap().from_text += 120;
    });
    let note_para = 7;
    let at = Position::new(note_para, doc.main.paras[note_para].len());
    let chp = doc.main.paras[note_para].mark_chp;
    doc.insert_note(at, chp, None);
    let table_para = doc
        .main
        .paras
        .iter()
        .position(|para| para.pap.in_table)
        .unwrap();
    let row = row_at(&doc.main.paras, table_para).unwrap();
    let mut row_settings = row_props(&doc.main.paras, &row, doc.props.text_width());
    row_settings.height = RowHeight::AtLeast(620);
    doc.set_row(StoryId::Main, row.end, row_settings);
    let picture_at = Position::new(0, doc.main.paras[0].len());
    let picture_chp = doc.main.paras[0].mark_chp;
    doc.insert_picture(
        StoryId::Main,
        picture_at,
        Picture::from_bytes(png(4, 3)).unwrap(),
        picture_chp,
    );
    doc.commit(selection);

    let edited = quill_formats::native::write(&doc, SECS);
    assert_ne!(edited, before);
    assert_eq!(doc.undo_label(), Some("Combined document edit"));
    doc.undo().unwrap();
    assert_eq!(quill_formats::native::write(&doc, SECS), before);
    doc.redo().unwrap();
    assert_eq!(quill_formats::native::write(&doc, SECS), edited);

    let dir = std::env::temp_dir().join(format!("quill-combined-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, format) in [
        ("combined.qwp", quill_formats::Format::Quill),
        ("combined.docx", quill_formats::Format::Docx),
    ] {
        let path = dir.join(name);
        quill_formats::save(&doc, &path, format).unwrap();
        let back = quill_formats::load(&path).unwrap();
        assert!(
            common::differences(&doc, &back).is_empty(),
            "combined feature document changed after saving {name}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn quill_keeps_one_original_and_preview_for_repeated_placements() {
    let mut doc = Document::new();
    let chp = doc.main.paras[0].mark_chp;
    let mut pic = Picture::from_bytes(png(6, 4)).unwrap();
    let preview: std::sync::Arc<[u8]> = png(3, 2).into();
    pic.preview = Some(preview.clone());
    pic.preview_pixels = Some((3, 2));
    doc.insert_picture(StoryId::Main, Position::new(0, 0), pic.clone(), chp);
    doc.insert_picture(
        StoryId::Main,
        Position::new(0, chars::OBJECT.len_utf8()),
        pic.scaled_to(200, 100),
        chp,
    );

    let bytes = quill_formats::native::write(&doc, SECS);
    let names: Vec<_> = entries(&bytes).into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names.iter().filter(|n| n.starts_with("objects/")).count(),
        1
    );
    assert_eq!(
        names.iter().filter(|n| n.starts_with("previews/")).count(),
        1
    );
    let back = quill_formats::native::read(&bytes).unwrap();
    let first = back.picture(1).unwrap();
    let second = back.picture(2).unwrap();
    assert_eq!(first.preview_pixels, Some((3, 2)));
    assert_eq!(first.preview.as_deref(), Some(preview.as_ref()));
    assert!(std::sync::Arc::ptr_eq(&first.data, &second.data));
    assert!(std::sync::Arc::ptr_eq(
        first.preview.as_ref().unwrap(),
        second.preview.as_ref().unwrap()
    ));
}

/// The archive's entries, to tamper with.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let z = quill_formats::zip::Archive::open(bytes).unwrap();
    let names: Vec<String> = z.names().map(str::to_string).collect();
    names
        .into_iter()
        .map(|n| {
            let d = z.read(&n).unwrap().unwrap();
            (n, d)
        })
        .collect()
}

fn rezip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = quill_formats::zip::Writer::new(SECS);
    for (n, d) in entries {
        w.add(n, d);
    }
    w.finish()
}

#[test]
fn quill_skips_fields_from_newer_versions() {
    let doc = kitchen_sink();
    let mut parts = entries(&quill_formats::native::write(&doc, SECS));
    let bin = &mut parts
        .iter_mut()
        .find(|(n, _)| n == "document.bin")
        .unwrap()
        .1;
    // An unknown field (tag 900, three bytes) at the end, as a later minor
    // version might add.
    bin.extend_from_slice(&[0x84, 0x07, 3, 1, 2, 3]);
    bin[10] = 7; // and a later minor version number
    let back = quill_formats::native::read(&rezip(&parts)).unwrap();
    assert_eq!(back.main, doc.main);
}

#[test]
fn quill_refuses_newer_major_versions() {
    let mut parts = entries(&quill_formats::native::write(&kitchen_sink(), SECS));
    let bin = &mut parts
        .iter_mut()
        .find(|(n, _)| n == "document.bin")
        .unwrap()
        .1;
    bin[8] = 2;
    let err = quill_formats::native::read(&rezip(&parts)).err().unwrap();
    assert!(err.contains("newer version"), "{err}");
}

#[test]
fn damaged_quill_files_give_errors_not_panics() {
    let bytes = quill_formats::native::write(&kitchen_sink(), SECS);
    // A flipped byte inside the compressed document: the checksum says so.
    let mut bad = bytes.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 0x55;
    assert!(quill_formats::native::read(&bad).is_err());
    // Cut short anywhere.
    for cut in [10, bytes.len() / 3, bytes.len() - 30] {
        assert!(quill_formats::native::read(&bytes[..cut]).is_err());
    }
    // Garbage inside document.bin with a correct checksum: reading may
    // fail or repair, but never panics, and what it returns is usable.
    let parts = entries(&bytes);
    let k = parts.iter().position(|(n, _)| n == "document.bin").unwrap();
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for _ in 0..300 {
        let mut p = parts.clone();
        let bin = &mut p[k].1;
        for _ in 0..1 + next() % 4 {
            let at = 12 + next() % (bin.len() - 12);
            bin[at] = next() as u8;
        }
        if let Ok(doc) = quill_formats::native::read(&rezip(&p)) {
            // Usable: every paragraph's runs cover its text.
            for para in &doc.main.paras {
                let total: usize = para.runs.iter().map(|r| r.len).sum();
                assert_eq!(total, para.text.len());
            }
            let _ = quill_core::text::to_plain_text(&doc, "\n");
        }
    }
}

#[test]
fn quill_files_open_whatever_they_are_called() {
    let dir = std::env::temp_dir().join(format!("quill-native-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = kitchen_sink();
    let path = dir.join("misnamed.docx");
    quill_formats::save(&doc, &path, quill_formats::Format::Quill).unwrap();
    let back = quill_formats::load(&path).unwrap();
    assert_eq!(back.main, doc.main);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_live_table_of_contents_is_a_toc_field_in_docx() {
    let mut doc = quill_core::text::from_plain_text("Intro\t1\nBody");
    let toc1 = doc.styles.find("toc 1").unwrap();
    doc.apply_style(StoryId::Main, 0, 0, toc1);
    doc.split_paragraph(StoryId::Main, Position::new(1, 0));
    let p = &mut doc.main.paras[0];
    p.mark = quill_core::Mark::SectionBreak;
    p.section = Some(Box::new(doc.section.clone()));
    assert_eq!(doc.existing_toc(), Some(0));
    let o = quill_core::contents::TocOptions {
        outline: true,
        page_numbers: true,
        from: 1,
        to: 3,
    };
    doc.props.live_toc = Some(o);
    let bytes = quill_formats::docx::write(&doc, 1_760_000_000);
    let xml = String::from_utf8_lossy(
        &quill_formats::zip::Archive::open(&bytes)
            .unwrap()
            .read("word/document.xml")
            .unwrap()
            .unwrap(),
    )
    .into_owned();
    assert!(xml.contains(r#"TOC \o "1-3""#));
    let back = quill_formats::docx::read(&bytes).unwrap();
    assert_eq!(back.props.live_toc, Some(o));
    assert_eq!(back.main.paras[0].text, "Intro\t1");
}

/// The same picture twice in a row shares one object but keeps one run per
/// picture, as editing does, after every format reads it back.
#[test]
fn the_same_picture_twice_keeps_a_run_each() {
    let mut doc = Document::new();
    let chp = doc.main.paras[0].mark_chp;
    let pic = Picture::from_bytes(png(3, 2)).unwrap();
    let after = doc.insert_picture(StoryId::Main, Position::new(0, 0), pic.clone(), chp);
    doc.insert_picture(StoryId::Main, after, pic, chp);
    assert_eq!(doc.main.paras[0].runs.len(), 2);
    quill_round_trip(&doc);
    let docx = quill_formats::docx::write(&doc, SECS);
    let rtf = quill_formats::rtf::write(&doc);
    for back in [
        quill_formats::docx::read(&docx).unwrap(),
        quill_formats::rtf::read(&rtf).unwrap(),
        quill_formats::odt_read::read(&quill_formats::odt::write(&doc, SECS)).unwrap(),
    ] {
        assert_eq!(back.main.paras[0].runs.len(), 2);
    }
}

/// The pictures in a story, as their file bytes, in order. Only object
/// characters count: a note's mark also carries an id in its run.
fn story_pictures(doc: &Document, story: &Story) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for p in &story.paras {
        for (at, c) in p.text.char_indices() {
            if c == chars::OBJECT
                && let Some(pic) = doc.picture(p.chp_of_char(at).object)
            {
                out.push(pic.data.to_vec());
            }
        }
    }
    out
}

#[test]
fn docx_keeps_pictures_in_footnotes_and_comments() {
    let mut doc = quill_core::text::from_plain_text("Body text");
    let base = doc.main.paras[0].mark_chp;
    let pictures = [png(3, 2), png(5, 4), png(7, 3)];
    let at = Position::new(0, 0);
    let main = Picture::from_bytes(pictures[0].clone()).unwrap();
    doc.insert_picture(StoryId::Main, at, main, base);

    let end = doc.main.paras[0].len();
    let note = doc.insert_note(Position::new(0, end), base, None);
    let end = doc.main.paras[0].len();
    let remark = doc.insert_annotation(
        Position::new(0, end),
        base,
        AnnotationInfo {
            initials: "LK".into(),
            author: "Lucas".into(),
            date: "2026-10-15".into(),
            ..Default::default()
        },
    );
    let at = doc.note_text_start(note).unwrap();
    let chp = doc.notes.paras[at.para].mark_chp;
    let pic = Picture::from_bytes(pictures[1].clone()).unwrap();
    doc.insert_picture(StoryId::Notes, at, pic, chp);
    let at = doc.annotation_text_start(remark).unwrap();
    let chp = doc.annotations.paras[at.para].mark_chp;
    let pic = Picture::from_bytes(pictures[2].clone()).unwrap();
    doc.insert_picture(StoryId::Annotations, at, pic, chp);

    let back = quill_formats::docx::read(&quill_formats::docx::write(&doc, SECS)).unwrap();
    assert_eq!(story_pictures(&back, &back.main), [pictures[0].clone()]);
    assert_eq!(story_pictures(&back, &back.notes), [pictures[1].clone()]);
    assert_eq!(
        story_pictures(&back, &back.annotations),
        [pictures[2].clone()]
    );
}
