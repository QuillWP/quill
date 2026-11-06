//! Rebuild the manual review fixtures: cargo run -p quill-formats --example review_samples -- <directory>
use quill_core::notes::AnnotationInfo;
use quill_core::props::CharProps;
use quill_core::revision::{self, Revision, Stamp};
use quill_core::{Document, Position, StoryId, text};
use quill_formats::{Format, save};
use std::path::Path;

fn stamp(author: &str, hour: u8) -> Stamp {
    Stamp::new(author, &format!("2026-10-23T{hour:02}:00:00Z"))
}

fn mark(doc: &mut Document, para: usize, word: &str, rev: Revision) {
    let start = doc.main.paras[para].text.find(word).unwrap();
    let id = revision::intern(rev);
    doc.format_chars(
        StoryId::Main,
        Position::new(para, start),
        Position::new(para, start + word.len()),
        |c| c.rev = id,
    );
}

fn base(title: &str, body: &str) -> Document {
    let mut d = text::from_plain_text(&format!("{title}\n{body}"));
    d.summary.title = title.into();
    let normal = d.styles.get(0).clone();
    let mut title_chp = normal.chp;
    title_chp.bold = true;
    title_chp.size = 32;
    let mut title_pap = normal.pap;
    title_pap.after = 160;
    let title_style = d.styles.add("Title", 0, title_pap, title_chp);
    d.apply_style(StoryId::Main, 0, 0, title_style);
    let c = CharProps::default();
    d.insert_plain(
        StoryId::Header,
        Position::new(0, 0),
        "Quill review samples",
        c,
    );
    d.insert_plain(
        StoryId::Footer,
        Position::new(0, 0),
        "Review copy  |  Page \u{E000}",
        c,
    );
    d
}

fn revisions() -> Document {
    let mut d = base(
        "Three author revision review",
        "Each reviewer has an insertion, a deletion and a formatting change below.\nLucas: Keep the compact useful editor.\nAlex: Save the old reliable document.\nMorgan: Show the unclear readable text.\nUse Review Changes to accept or reject individual changes. This is a display fixture, not a release commitment.",
    );
    for (p, who, inserted, deleted, formatted, hour) in [
        (2, "Lucas", "useful", "compact", "editor", 9),
        (3, "Alex", "reliable", "old", "document", 10),
        (4, "Morgan", "readable", "unclear", "text", 11),
    ] {
        mark(
            &mut d,
            p,
            inserted,
            Revision {
                inserted: Some(stamp(who, hour)),
                ..Default::default()
            },
        );
        mark(
            &mut d,
            p,
            deleted,
            Revision {
                deleted: Some(stamp(who, hour)),
                ..Default::default()
            },
        );
        let start = d.main.paras[p].text.find(formatted).unwrap();
        let old = d.main.paras[p].chp_of_char(start);
        d.format_chars(
            StoryId::Main,
            Position::new(p, start),
            Position::new(p, start + formatted.len()),
            |c| c.bold = true,
        );
        mark(
            &mut d,
            p,
            formatted,
            Revision {
                formatted: Some((stamp(who, hour), old)),
                ..Default::default()
            },
        );
    }
    d.props.track_changes = true;
    d
}

fn info(who: &str, hour: u8) -> AnnotationInfo {
    AnnotationInfo {
        author: who.into(),
        initials: who[..1].into(),
        date: stamp(who, hour).date,
        ..Default::default()
    }
}

fn comment_text(d: &mut Document, id: u32, text: &str) {
    let start = d.annotation_text_start(id).unwrap();
    d.insert_text(StoryId::Annotations, start, text, CharProps::default());
}

fn comments() -> Document {
    let mut d = base(
        "Three author comment review",
        "Open View > Annotations to read the discussion.\nThe header should identify the document without crowding the page.\nThe footer should make page numbers easy to find.\nThis wording has already been reviewed.",
    );
    let c = CharProps::default();
    let root = d.insert_annotation_on(
        Position::new(2, 4),
        Position::new(2, 10),
        c,
        info("Lucas", 9),
    );
    comment_text(
        &mut d,
        root,
        "Should the header use the document title or a short running title?",
    );
    let reply = d.insert_reply(root, info("Alex", 10)).unwrap();
    comment_text(
        &mut d,
        reply,
        "A short running title will leave more room for the body text.",
    );
    let reply = d.insert_reply(root, info("Morgan", 11)).unwrap();
    comment_text(
        &mut d,
        reply,
        "Agreed. Please check the appearance in Page Layout view.",
    );
    let second = d.insert_annotation_on(
        Position::new(3, 4),
        Position::new(3, 10),
        c,
        info("Alex", 10),
    );
    comment_text(
        &mut d,
        second,
        "Keep the page number in the footer and check its spacing.",
    );
    let done = d.insert_annotation_on(
        Position::new(4, 0),
        Position::new(4, 12),
        c,
        info("Morgan", 11),
    );
    comment_text(
        &mut d,
        done,
        "Wording checked. This thread is resolved and retained for reference.",
    );
    d.set_annotation_done(done, true);
    d
}

fn main() {
    let directory = std::env::args().nth(1).expect("supply an output directory");
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    for (name, doc) in [
        ("three-author-revisions", revisions()),
        ("three-author-comments", comments()),
    ] {
        for (ext, format) in [("qwp", Format::Quill), ("docx", Format::Docx)] {
            let path = directory.join(format!("{name}.{ext}"));
            save(&doc, &path, format).unwrap();
            verify(
                &quill_formats::load(&path).unwrap(),
                name.ends_with("revisions"),
            );
        }
        if name.ends_with("revisions") {
            let path = directory.join(format!("{name}.rtf"));
            save(&doc, &path, Format::Rtf).unwrap();
            verify(&quill_formats::load(&path).unwrap(), true);
        }
    }
    let mut combined = revisions();
    combined.props.track_changes = false;
    let start = combined.main.paras[2].text.find("useful").unwrap();
    let root = combined.insert_annotation_on(
        Position::new(2, start),
        Position::new(2, start + 6),
        CharProps::default(),
        info("Alex", 10),
    );
    comment_text(
        &mut combined,
        root,
        "This insertion reads well. Lucas, does it cover the intended behavior?",
    );
    let reply = combined.insert_reply(root, info("Lucas", 11)).unwrap();
    comment_text(
        &mut combined,
        reply,
        "Yes. Please keep the revision and this discussion visible together.",
    );
    let reply = combined.insert_reply(root, info("Morgan", 12)).unwrap();
    comment_text(
        &mut combined,
        reply,
        "Checked the combined Review pane and the author badges.",
    );
    combined.props.track_changes = true;
    for (ext, format) in [("qwp", Format::Quill), ("docx", Format::Docx)] {
        let path = directory.join(format!("three-author-combined.{ext}"));
        save(&combined, &path, format).unwrap();
        let loaded = quill_formats::load(&path).unwrap();
        verify(&loaded, true);
        assert_eq!(loaded.annotation_info.len(), 3);
    }
}

fn verify(d: &Document, revisions: bool) {
    if revisions {
        let mut changes = std::collections::BTreeSet::new();
        for p in &d.main.paras {
            for (_, _, c) in p.run_bounds() {
                if let Some(r) = revision::get(c.rev) {
                    for (kind, by) in [
                        ("insert", r.inserted.as_ref()),
                        ("delete", r.deleted.as_ref()),
                        ("format", r.formatted.as_ref().map(|(by, _)| by)),
                    ] {
                        if let Some(by) = by {
                            changes.insert((by.author.clone(), kind));
                        }
                    }
                }
            }
        }
        assert_eq!(
            changes.len(),
            9,
            "all three authors must retain all three change kinds"
        );
        for name in ["Lucas", "Alex", "Morgan"] {
            for kind in ["insert", "delete", "format"] {
                assert!(changes.contains(&(name.to_string(), kind)));
            }
        }
    } else {
        assert_eq!(d.annotation_info.len(), 5);
        assert_eq!(
            d.annotation_info
                .values()
                .filter(|i| i.parent.is_some())
                .count(),
            2
        );
        assert_eq!(d.annotation_info.values().filter(|i| i.done).count(), 1);
        let mut ranged = std::collections::BTreeSet::new();
        for p in &d.main.paras {
            for (_, _, c) in p.run_bounds() {
                if c.comment != 0 {
                    ranged.insert(c.comment);
                }
            }
        }
        assert_eq!(ranged.len(), 3);
        for &id in d.annotation_info.keys() {
            assert!(!d.annotation_text(id).trim().is_empty());
        }
    }
}
