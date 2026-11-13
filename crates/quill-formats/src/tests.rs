use super::*;

#[test]
fn a_qwp_save_must_read_back_unchanged() {
    let doc = quill_core::text::from_plain_text("One\nTwo");
    let back = native::read(&native::write(&doc, 0)).unwrap();
    assert_eq!(native_loss(&doc, &back), None);
    assert!(native_check(&doc).is_none());
    // The message names the paragraph and what changed in it.
    let loss = |lost: &Document| native_loss(&doc, lost).unwrap();
    let mut lost = back.clone();
    lost.main.paras.pop();
    let l = loss(&lost);
    assert_eq!((l.part, l.para), ("text", Some(1)));
    assert_eq!(
        l.to_string(),
        "the text came back different at paragraph 2 (\u{201c}Two\u{201d}): it was missing"
    );
    let mut lost = back.clone();
    lost.main.paras[1].pap.left += 20;
    assert_eq!(
        loss(&lost).detail,
        "at paragraph 2 (\u{201c}Two\u{201d}): its paragraph formatting changed"
    );
    let mut lost = back.clone();
    lost.main.paras[0].text.push('!');
    assert_eq!(loss(&lost).para, Some(0));
    assert!(loss(&lost).detail.ends_with("its text changed"));
    let mut lost = back;
    lost.props.page_width += 20;
    assert_eq!(
        loss(&lost).to_string(),
        "the document settings came back different"
    );
    // A normal save still goes through.
    let dir = std::env::temp_dir().join(format!("quill-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved.qwp");
    save(&doc, &path, Format::Quill).unwrap();
    assert_eq!(native_loss(&doc, &load(&path).unwrap()), None);
    // Save Anyway hands back what the file now holds.
    let back = save_anyway(&doc, &path).unwrap();
    assert_eq!(native_loss(&back, &load(&path).unwrap()), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn files_that_are_not_what_their_names_say_get_a_clear_answer() {
    let dir = std::env::temp_dir().join(format!("quill-names-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let error = |path: &Path| load(path).err().unwrap_or_default();

    // Text with a Word document's name: refused, but it can open as text.
    let fake = file("notes.docx", b"Just some text\r\non two lines\r\n");
    assert!(error(&fake).contains("isn't a Word document: it holds plain text"));
    assert!(holds_text_not_its_format(&fake));
    let doc = load_as_text(&fake).unwrap();
    assert_eq!(doc.main.paras[0].text, "Just some text");

    // An old .doc, whatever its name.
    let mut old = OLD_OFFICE.to_vec();
    old.extend([0; 600]);
    assert!(error(&file("report.docx", &old)).contains("Word 97-2003"));
    assert!(error(&file("report.txt", &old)).contains("Word 97-2003"));

    // A .docx with a password uses the same container.
    old.extend("EncryptedPackage".encode_utf16().flat_map(u16::to_le_bytes));
    let locked = error(&file("locked.docx", &old));
    assert!(locked.contains("protected by a password"), "{locked}");

    // A zip that isn't a document, and a file that isn't text.
    let mut zip = crate::zip::Writer::new(0);
    zip.add("readme.txt", b"hello");
    assert!(error(&file("bundle.zip", &zip.finish())).contains("zip archive"));
    assert!(
        error(&file("photo.txt", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")).contains("isn't a document")
    );
    assert!(!holds_text_not_its_format(&file("real.txt", b"text")));

    // Windows Notepad's Unicode text.
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("Caf\u{e9}".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(
        load(&file("notepad.txt", &utf16)).unwrap().main.paras[0].text,
        "Caf\u{e9}"
    );

    // UTF-16 without its mark, and Windows-1252.
    let bare: Vec<u8> = "Price \u{20ac}5"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(
        load(&file("bare.txt", &bare)).unwrap().main.paras[0].text,
        "Price \u{20ac}5"
    );
    let bare: Vec<u8> = "Big end"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    assert_eq!(
        load(&file("be.txt", &bare)).unwrap().main.paras[0].text,
        "Big end"
    );
    assert_eq!(
        load(&file("w1252.txt", b"\x93Caf\xe9\x94 \x80" as &[u8]))
            .unwrap()
            .main
            .paras[0]
            .text,
        "\u{201c}Caf\u{e9}\u{201d} \u{20ac}"
    );

    // Text with more paragraphs than Quill holds is refused before it is
    // turned into paragraphs.
    let lines = "\n".repeat(quill_core::limits::MAX_PARAGRAPHS + 1);
    assert!(error(&file("lines.txt", lines.as_bytes())).contains("too large for Quill"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn documents_over_the_size_limit_are_not_saved() {
    let mut doc = quill_core::text::from_plain_text("x");
    let p = doc.main.paras[0].clone();
    doc.main.paras = vec![p; quill_core::limits::MAX_PARAGRAPHS + 1];
    let path = std::env::temp_dir().join(format!("quill-over-{}.qwp", std::process::id()));
    let e = save(&doc, &path, Format::Quill).unwrap_err();
    assert!(e.contains("Quill's limit is"), "{e}");
    assert!(!path.exists());
}

/// Slow and needs over 2 GB of memory, so it runs only when asked:
/// cargo test --release -p quill-formats -- --ignored at_the_size_limit
#[test]
#[ignore]
fn documents_at_the_size_limit_save_and_open_again() {
    use quill_core::{CharProps, Paragraph, Rgb};
    let mut doc = Document::new();
    let font = doc.font_id("Helvetica Neue");
    let pap = doc.main.paras[0].pap.clone();
    let looks = [
        CharProps {
            bold: true,
            size: 28,
            ..Default::default()
        },
        CharProps {
            italic: true,
            color: Rgb(0xC00000),
            font,
            ..Default::default()
        },
        CharProps {
            underline: quill_core::Underline::Single,
            spacing: 20,
            position: 60,
            ..Default::default()
        },
        CharProps::default(),
    ];
    // Four runs of 32 characters a paragraph, many of them escapes.
    let piece: String = "A <tag> & \u{e9}\u{1F680} words.."
        .chars()
        .cycle()
        .take(32)
        .collect();
    let headers = doc.size().paragraphs - 1;
    doc.main.paras = (0..quill_core::limits::MAX_PARAGRAPHS - headers)
        .map(|_| {
            let mut p = Paragraph::new(pap.clone(), CharProps::default());
            for chp in looks {
                let at = p.len();
                p.insert(at, &piece, chp);
            }
            p
        })
        .collect();
    assert!(doc.size().too_large().is_none());
    for format in [Format::Quill, Format::Docx, Format::Rtf] {
        let path = std::env::temp_dir().join(format!(
            "quill-limit-{}.{}",
            std::process::id(),
            format.extensions()[0]
        ));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.size(), doc.size(), "{format:?}");
        let _ = std::fs::remove_file(&path);
    }
}
use quill_core::{Position, StoryId, text};

#[test]
fn the_saved_view_survives_every_format_that_has_one() {
    use quill_core::document::SavedView;
    let dir = std::env::temp_dir().join(format!("quill-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for view in [SavedView::Normal, SavedView::Outline, SavedView::PageLayout] {
        let mut doc = quill_core::Document::new();
        doc.props.view = Some(view);
        for format in [Format::Quill, Format::Docx, Format::Rtf] {
            let path = dir.join(format!("a.{}", format.extensions()[0]));
            save(&doc, &path, format).unwrap();
            assert_eq!(load(&path).unwrap().props.view, Some(view), "{format:?}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_formats_without_a_page_say_so() {
    let dir = std::env::temp_dir().join(format!("quill-paper-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = quill_core::Document::new();
    doc.insert_text(StoryId::Main, Position::new(0, 0), "Hi", Default::default());
    for (format, no_page) in [
        (Format::Quill, false),
        (Format::Docx, false),
        (Format::Rtf, false),
        (Format::Odt, false),
        (Format::Html, true),
        (Format::Markdown, true),
        (Format::Text, true),
    ] {
        let path = dir.join(format!("a.{}", format.extensions()[0]));
        save(&doc, &path, format).unwrap();
        let (_, report) = load_reported(&path, true).unwrap();
        assert_eq!(report.no_page, no_page, "{format:?}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn superscripts_and_shifts_by_hand_survive_every_format() {
    use quill_core::Script;
    let mut doc = quill_core::text::from_plain_text("E=mc2 and H2O and up");
    let at = |d: &Document, i: usize| d.main.paras[0].chp_of_char(i);
    let p = &mut doc.main.paras[0];
    p.format_range(4, 5, &mut |c| c.script = Script::Super);
    p.format_range(11, 12, &mut |c| c.script = Script::Sub);
    p.format_range(18, 20, &mut |c| c.position = 60);
    doc.props.scripts.super_size = 80;
    let dir = std::env::temp_dir().join(format!("quill-scripts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for format in [
        Format::Quill,
        Format::Docx,
        Format::Rtf,
        Format::Odt,
        Format::Html,
        Format::Markdown,
    ] {
        let path = dir.join(format!("a.{}", format.extensions()[0]));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(at(&back, 4).script, Script::Super, "{format:?}");
        assert_eq!(at(&back, 11).script, Script::Sub, "{format:?}");
        // The text keeps its own size; only drawing makes it smaller.
        assert_eq!(at(&back, 4).size, at(&back, 0).size, "{format:?}");
        assert_eq!(at(&back, 0).script, Script::None, "{format:?}");
        if format != Format::Markdown {
            assert_eq!(at(&back, 18).position, 60, "{format:?}");
            assert_eq!(at(&back, 18).script, Script::None, "{format:?}");
        }
        if format == Format::Quill {
            assert_eq!(back.props.scripts, doc.props.scripts);
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn vertical_merges_survive_every_format() {
    use quill_core::VMerge;
    let mut doc = quill_core::text::from_plain_text("a\tb\n\tc\n\td\ne\tf");
    doc.convert_to_table(
        StoryId::Main,
        0,
        3,
        quill_core::table::Convert::Tabs,
        |_| 2000,
    );
    let rows = quill_core::table::table_rows(&doc.main.paras, 0);
    doc.merge_cells_down(StoryId::Main, &rows[..3], &[0, 0, 0], 8640);
    let tw = doc.props.text_width();
    let states = |d: &Document| -> Vec<Vec<VMerge>> {
        quill_core::table::table_rows(&d.main.paras, 0)
            .iter()
            .map(|r| {
                quill_core::table::row_props(&d.main.paras, r, tw)
                    .cells
                    .iter()
                    .map(|c| c.vmerge)
                    .collect()
            })
            .collect()
    };
    let want = states(&doc);
    assert_eq!(want[0][0], VMerge::Restart);
    assert_eq!(want[2][0], VMerge::Continue);
    let dir = std::env::temp_dir().join(format!("quill-vmerge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for format in [
        Format::Quill,
        Format::Docx,
        Format::Rtf,
        Format::Odt,
        Format::Html,
    ] {
        let path = dir.join(format!("a.{}", format.extensions()[0]));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(states(&back), want, "{format:?}");
        let top = quill_core::table::row_at(&back.main.paras, 0).unwrap();
        assert!(
            back.main.paras[top.cells[0].1].text.is_empty()
                || back.main.paras[top.cells[0].0].text == "a",
            "{format:?}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn border_colors_survive_every_format() {
    use quill_core::props::{BorderLine, BorderStyle, Rgb};
    let mut doc = quill_core::Document::new();
    doc.insert_table(StoryId::Main, Position::new(0, 0), 1, 2, 2000);
    let row = quill_core::table::row_at(&doc.main.paras, 0).unwrap();
    let mut props = quill_core::table::row_props(&doc.main.paras, &row, 4000);
    let line = BorderLine {
        style: BorderStyle::Single,
        color: Rgb(0x336699),
        ..Default::default()
    };
    props.cells[0].borders.top = line;
    props.cells[1].borders.left = line;
    doc.set_row(StoryId::Main, row.end, props);
    doc.main.paras.last_mut().unwrap().pap.borders.bottom = line;
    let dir = std::env::temp_dir().join(format!("quill-line-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for format in [Format::Quill, Format::Docx, Format::Rtf, Format::Odt] {
        let path = dir.join(format!("a.{}", format.extensions()[0]));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        let row = quill_core::table::row_at(&back.main.paras, 0).unwrap();
        let props = quill_core::table::row_props(&back.main.paras, &row, 4000);
        assert_eq!(props.cells[0].borders.top.color, line.color, "{format:?}");
        assert_eq!(props.cells[1].borders.left.color, line.color, "{format:?}");
        assert_eq!(
            props.cells[0].borders.bottom.color,
            Rgb::BLACK,
            "{format:?}"
        );
        // ODT export leaves paragraph borders out altogether.
        if format != Format::Odt {
            let last = back.main.paras.last().unwrap();
            assert_eq!(last.pap.borders.bottom.color, line.color, "{format:?}");
        }
    }
    // Another program's RTF says it in the standard words.
    let rtf = br"{\rtf1{\colortbl;\red51\green102\blue153;}
\trowd\clbrdrt\brdrs\brdrw15\brdrcf1\cellx2000 A\cell\row}";
    let back = rtf::read(rtf).unwrap();
    let row = quill_core::table::row_at(&back.main.paras, 0).unwrap();
    let props = quill_core::table::row_props(&back.main.paras, &row, 4000);
    assert_eq!(props.cells[0].borders.top.color, line.color);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cell_vertical_alignment_survives_every_format() {
    use quill_core::props::VAlign;
    let mut doc = quill_core::Document::new();
    doc.insert_table(StoryId::Main, Position::new(0, 0), 1, 3, 2000);
    let row = quill_core::table::row_at(&doc.main.paras, 0).unwrap();
    let mut props = quill_core::table::row_props(&doc.main.paras, &row, 6000);
    props.cells[1].valign = VAlign::Center;
    props.cells[2].valign = VAlign::Bottom;
    doc.set_row(StoryId::Main, row.end, props);
    let dir = std::env::temp_dir().join(format!("quill-valign-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for format in [Format::Quill, Format::Docx, Format::Rtf] {
        let path = dir.join(format!("a.{}", format.extensions()[0]));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        let row = quill_core::table::row_at(&back.main.paras, 0).unwrap();
        let props = quill_core::table::row_props(&back.main.paras, &row, 6000);
        let got: Vec<VAlign> = props.cells.iter().map(|c| c.valign).collect();
        assert_eq!(
            got,
            [VAlign::Top, VAlign::Center, VAlign::Bottom],
            "{format:?}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_and_load_files() {
    let dir = std::env::temp_dir().join(format!("quill-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = text::from_plain_text("One\nTwo");
    for (name, fmt) in [
        ("a.qwp", Format::Quill),
        ("a.docx", Format::Docx),
        ("a.rtf", Format::Rtf),
        ("a.txt", Format::Text),
    ] {
        let path = dir.join(name);
        assert_eq!(Format::from_path(&path), Some(fmt));
        save(&doc, &path, fmt).unwrap();
        let back = load(&path).unwrap();
        let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["One", "Two"], "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bounded_file_read_refuses_oversized_input() {
    let dir = std::env::temp_dir().join(format!("quill-read-limit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("large.txt");
    std::fs::write(&path, b"12345").unwrap();

    let error = read_file_with_limit(&path, 4).unwrap_err();

    assert!(error.contains("too large"), "{error}");
    assert!(error.contains("large.txt"), "{error}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn damaged_document_errors_name_the_file_and_format() {
    let dir = std::env::temp_dir().join(format!("quill-error-context-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("damaged.rtf");
    std::fs::write(&path, b"{\\rtf1 \\'zz}").unwrap();

    let error = load(&path).unwrap_err();

    assert!(error.contains("damaged.rtf"), "{error}");
    assert!(error.contains("RTF"), "{error}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn document_reading_refuses_devices_before_reading() {
    let error = read_file_with_limit(Path::new("/dev/zero"), 8).unwrap_err();
    assert!(error.contains("Not a regular file"), "{error}");
    assert!(!holds_text_not_its_format(Path::new("/dev/null")));
}

#[test]
fn picture_files_are_created_once_and_never_overwritten() {
    let dir = std::env::temp_dir().join(format!("quill-immutable-pictures-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("image.png");
    write_picture_file(&path, b"first").unwrap();
    write_picture_file(&path, b"first").unwrap();
    // Same length deliberately exercises the content comparison.
    assert!(write_picture_file(&path, b"other").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"first");
    #[cfg(unix)]
    {
        let link = dir.join("linked.png");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(write_picture_file(&link, b"first").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn atomic_write_replaces_an_existing_file() {
    let dir = std::env::temp_dir().join(format!("quill-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("document.qwp");
    std::fs::write(&path, b"old").unwrap();

    write_atomic(&path, b"new").unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn failed_atomic_replace_keeps_the_old_file_and_removes_the_temporary_one() {
    let dir = std::env::temp_dir().join(format!("quill-atomic-fail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("document.qwp");
    std::fs::write(&path, b"old").unwrap();

    let error = write_atomic_with(&path, b"new", |_, _| {
        Err(std::io::Error::other("pretend replacement failed"))
    })
    .unwrap_err();

    assert!(error.contains("pretend replacement failed"));
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn transient_windows_replacement_errors_are_retried_with_a_bound() {
    let mut calls = 0;
    let mut waits = Vec::new();
    retry_io(
        || {
            calls += 1;
            if calls < 3 {
                Err(std::io::Error::from_raw_os_error(32))
            } else {
                Ok(())
            }
        },
        transient_windows_replace_error,
        |delay| waits.push(delay),
    )
    .unwrap();
    assert_eq!(calls, 3);
    assert_eq!(
        waits,
        [
            std::time::Duration::from_millis(25),
            std::time::Duration::from_millis(50),
        ]
    );
}

#[test]
fn permanent_replacement_errors_are_not_retried() {
    let mut calls = 0;
    let error = retry_io(
        || {
            calls += 1;
            Err(std::io::Error::from_raw_os_error(3))
        },
        transient_windows_replace_error,
        |_| panic!("a permanent error must not sleep"),
    )
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.raw_os_error(), Some(3));
}

#[test]
fn transient_replacement_retries_stop_after_six_attempts() {
    let mut calls = 0;
    let mut waits = 0;
    let error = retry_io(
        || {
            calls += 1;
            Err(std::io::Error::from_raw_os_error(33))
        },
        transient_windows_replace_error,
        |_| waits += 1,
    )
    .unwrap_err();
    assert_eq!((calls, waits), (6, 5));
    assert_eq!(error.raw_os_error(), Some(33));
}

#[test]
fn atomic_copy_replaces_an_existing_backup() {
    let dir = std::env::temp_dir().join(format!("quill-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("document.qwp");
    let backup = dir.join("Backup of document.qwp");
    std::fs::write(&source, b"current document").unwrap();
    std::fs::write(&backup, b"older backup").unwrap();

    copy_atomic(&source, &backup).unwrap();

    assert_eq!(std::fs::read(&backup).unwrap(), b"current document");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn saving_keeps_a_private_file_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    let dir = std::env::temp_dir().join(format!("quill-private-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("secret.qwp");
    std::fs::write(&path, b"old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    write_atomic(&path, b"new").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert_eq!(mode(&path), 0o600);

    // A backup of a private document is private too, new or replaced.
    let backup = dir.join("Backup of secret.qwp");
    copy_atomic(&path, &backup).unwrap();
    assert_eq!(mode(&backup), 0o600);
    std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o640)).unwrap();
    copy_atomic(&path, &backup).unwrap();
    assert_eq!(mode(&backup), 0o640);

    // A file that did not exist gets the usual permissions, those any
    // new file gets under the umask, and not the temporary file's 0600.
    let fresh = dir.join("fresh.qwp");
    write_atomic(&fresh, b"x").unwrap();
    let usual = dir.join("usual");
    std::fs::write(&usual, b"x").unwrap();
    assert_eq!(mode(&fresh), mode(&usual));
    std::fs::remove_file(&usual).unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn atomic_copy_replaces_a_read_only_backup_when_the_folder_allows_it() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("quill-backup-read-only-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("document.qwp");
    let backup = dir.join("Backup of document.qwp");
    std::fs::write(&source, b"current document").unwrap();
    std::fs::write(&backup, b"older backup").unwrap();
    std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o444)).unwrap();

    copy_atomic(&source, &backup).unwrap();

    assert_eq!(std::fs::read(&backup).unwrap(), b"current document");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn failed_save_after_backup_preserves_both_saved_versions() {
    let dir = std::env::temp_dir().join(format!("quill-backup-save-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("document.qwp");
    let backup = dir.join("Backup of document.qwp");
    std::fs::write(&path, b"last saved version").unwrap();
    copy_atomic(&path, &backup).unwrap();

    write_atomic_with(&path, b"new version", |_, _| {
        Err(std::io::Error::other("pretend save failed"))
    })
    .unwrap_err();

    assert_eq!(std::fs::read(&path).unwrap(), b"last saved version");
    assert_eq!(std::fs::read(&backup).unwrap(), b"last saved version");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rtf_round_trip() {
    let mut doc = text::from_plain_text("Title\nCafé {braces} \\ back\tslash\nThird");
    let h1 = doc.styles.find("heading 1").unwrap();
    doc.apply_style(StoryId::Main, 0, 0, h1);
    doc.format_chars(
        StoryId::Main,
        Position::new(1, 0),
        Position::new(1, 5),
        |c| {
            c.bold = true;
            c.color = quill_core::Rgb(0xFF0000);
        },
    );
    doc.format_paras(StoryId::Main, 2, 2, |p| {
        p.align = quill_core::Align::Right;
        p.set_tab(quill_core::TabStop {
            pos: 2880,
            align: quill_core::TabAlign::Decimal,
            ..Default::default()
        });
    });
    doc.summary.author = "Lucas".into();
    let back = super::rtf::read(&super::rtf::write(&doc)).unwrap();
    let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["Title", "Café {braces} \\ back\tslash", "Third"]);
    assert_eq!(
        back.styles.get(back.main.paras[0].pap.style).name,
        "heading 1"
    );
    let c = back.main.paras[1].chp_of_char(0);
    assert!(c.bold);
    assert_eq!(c.color, quill_core::Rgb(0xFF0000));
    assert!(!back.main.paras[1].chp_of_char(6).bold);
    assert_eq!(back.main.paras[2].pap.align, quill_core::Align::Right);
    assert_eq!(back.main.paras[2].pap.tabs.len(), 1);
    assert_eq!(back.summary.author, "Lucas");
    assert_eq!(
        back.font_name(back.main.paras[0].chp_of_char(0).font),
        "Helvetica"
    );
}

#[test]
fn reads_original_style_mac_rtf() {
    // The shape of RTF the original wrote: \mac character set,
    // fonts listed in one group, Mac Roman bytes.
    let rtf = br"{\rtf1\mac\deff2 {\fonttbl{\f0\fswiss Chicago;}{\f2\froman New York;}{\f22\fmodern Courier;}}
{\stylesheet{\f2 \sbasedon222\snext0 Normal;}}\margl1800\margr1800
\pard\plain \f2 Caf\'8e and \'d2quotes\'d3\par
\pard\plain \qc \f22\b Bold Courier\par }";
    let doc = super::rtf::read(rtf).unwrap();
    let texts: Vec<_> = doc.main.paras.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["Café and “quotes”", "Bold Courier"]);
    assert_eq!(
        doc.font_name(doc.main.paras[1].chp_of_char(0).font),
        "Courier"
    );
    assert!(doc.main.paras[1].chp_of_char(0).bold);
    assert_eq!(doc.main.paras[1].pap.align, quill_core::Align::Center);
}

#[test]
fn docx_round_trip() {
    let mut doc = text::from_plain_text("Title\nHello world\nThird");
    let h1 = doc.styles.find("heading 1").unwrap();
    doc.apply_style(StoryId::Main, 0, 0, h1);
    doc.format_chars(
        StoryId::Main,
        Position::new(1, 6),
        Position::new(1, 11),
        |c| c.italic = true,
    );
    doc.format_paras(StoryId::Main, 2, 2, |p| p.align = quill_core::Align::Center);
    doc.summary.title = "Test & check".into();
    let bytes = super::docx::write(&doc, 1_760_000_000);
    let back: Document = super::docx::read(&bytes).unwrap();
    let texts: Vec<_> = back.main.paras.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["Title", "Hello world", "Third"]);
    assert_eq!(
        back.styles.get(back.main.paras[0].pap.style).name,
        "heading 1"
    );
    assert!(back.main.paras[1].chp_of_char(7).italic);
    assert!(!back.main.paras[1].chp_of_char(0).italic);
    assert_eq!(back.main.paras[2].pap.align, quill_core::Align::Center);
    assert_eq!(back.summary.title, "Test & check");
    assert_eq!(
        back.font_name(back.main.paras[1].chp_of_char(0).font),
        "New York"
    );
}

#[test]
fn docx_keeps_section_and_document_settings() {
    use quill_core::document::FootnotePosition;
    use quill_core::{PageNumberFormat, SectionStart};
    let mut doc = text::from_plain_text("One");
    doc.section.start = SectionStart::OddPage;
    doc.section.columns = 2;
    doc.section.column_spacing = 360;
    doc.section.header_from_top = 1080;
    doc.section.different_first_page = true;
    doc.section.page_number_format = PageNumberFormat::LowerRoman;
    doc.section.restart_page_numbers = true;
    doc.props.margin_left = 2160;
    doc.props.mirror_margins = true;
    doc.props.default_tab = 360;
    doc.props.footnote_position = FootnotePosition::BeneathText;
    doc.props.footnote_restart_each_page = true;
    doc.props.footnote_start = 3;
    doc.format_paras(StoryId::Main, 0, 0, |p| {
        p.borders.top.style = quill_core::props::BorderStyle::Double;
        p.borders.top.space = 80;
        p.shading = 200;
    });
    let back = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
    assert_eq!(back.section, doc.section);
    assert_eq!(back.props, doc.props);
    let p = &back.main.paras[0].pap;
    assert_eq!(p.borders.top.style, quill_core::props::BorderStyle::Double);
    assert_eq!(p.borders.top.space, 80);
    assert_eq!(p.shading, 200);
}

#[test]
fn docx_keeps_headers_footers_and_fields() {
    use quill_core::para::chars;
    let mut doc = text::from_plain_text("Body");
    let mut header = doc.story(StoryId::Header).clone();
    let chp = header.paras[0].mark_chp;
    header.paras[0].insert(0, &format!("Report\tPage {}", chars::FIELD_PAGE), chp);
    doc.set_story(StoryId::Header, header);
    let mut footer = doc.story(StoryId::FirstFooter).clone();
    let chp = footer.paras[0].mark_chp;
    footer.paras[0].insert(
        0,
        &format!("Printed {} at {}", chars::FIELD_DATE, chars::FIELD_TIME),
        chp,
    );
    doc.set_story(StoryId::FirstFooter, footer);
    doc.section.different_first_page = true;
    let bytes = super::docx::write(&doc, 0);
    let back = super::docx::read(&bytes).unwrap();
    assert_eq!(
        back.story(StoryId::Header).paras[0].text,
        format!("Report\tPage {}", chars::FIELD_PAGE)
    );
    assert_eq!(
        back.story(StoryId::FirstFooter).paras[0].text,
        format!("Printed {} at {}", chars::FIELD_DATE, chars::FIELD_TIME)
    );
    assert!(back.story_is_empty(StoryId::Footer));
    assert_eq!(
        back.styles
            .get(back.story(StoryId::Header).paras[0].pap.style)
            .name,
        "header"
    );
    assert_eq!(back.main.paras[0].text, "Body");
}

#[test]
fn docx_reads_other_fields_as_their_results() {
    // A complex field Quill does not know keeps its shown text.
    let xml = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> AUTHOR </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Ada</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:fldSimple w:instr=" PAGE "><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p>"#;
    let doc = super::docx::tests_read_body(xml);
    assert_eq!(
        doc.main.paras[0].text,
        format!("Ada{}", quill_core::para::chars::FIELD_PAGE)
    );
}

#[test]
fn docx_keeps_table_layout() {
    use quill_core::props::BorderStyle;
    use quill_core::table::{row_at, row_props};
    use quill_core::{RowHeight, TABLE_GAP};
    let mut doc = text::from_plain_text("Intro\nEnd");
    doc.insert_table(StoryId::Main, Position::new(1, 0), 2, 3, 2000);
    // Row 1: widen the middle cell, border and shade the first, set a height.
    let r1 = row_at(&doc.main.paras, 1).unwrap();
    let mut p1 = row_props(&doc.main.paras, &r1, 8640);
    p1.cells[1].width = 3000;
    p1.cells[0].borders.top.style = BorderStyle::Single;
    p1.cells[0].shading = 200;
    p1.height = RowHeight::AtLeast(400);
    p1.indent = 360;
    doc.set_row(StoryId::Main, r1.end, p1.clone());
    // Row 2: merge the first two cells.
    let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
    let mut p2 = row_props(&doc.main.paras, &r2, 8640);
    p2.indent = 360;
    doc.set_row(StoryId::Main, r2.end, p2.clone());
    let r2 = row_at(&doc.main.paras, r1.end + 1).unwrap();
    doc.merge_cells(StoryId::Main, &r2, &p2, 0, 1);

    let back = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
    let paras = &back.main.paras;
    let b1 = row_at(paras, 1).unwrap();
    let q1 = row_props(paras, &b1, 8640);
    assert_eq!(
        q1.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [2000, 3000, 2000]
    );
    assert_eq!(q1.cells[0].borders.top.style, BorderStyle::Single);
    assert_eq!(q1.cells[0].shading, 200);
    assert_eq!(q1.height, RowHeight::AtLeast(400));
    assert_eq!(q1.indent, 360);
    assert_eq!(q1.gap, TABLE_GAP);
    let b2 = row_at(paras, b1.end + 1).unwrap();
    let q2 = row_props(paras, &b2, 8640);
    assert_eq!(
        q2.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        [4000, 2000]
    );
    assert_eq!(paras.last().unwrap().text, "End");
}

/// A small PNG (the header is all the size reading needs).
fn png() -> Vec<u8> {
    let mut d = b"\x89PNG\r\n\x1a\n".to_vec();
    d.extend_from_slice(&13u32.to_be_bytes());
    d.extend_from_slice(b"IHDR");
    d.extend_from_slice(&40u32.to_be_bytes());
    d.extend_from_slice(&20u32.to_be_bytes());
    d.extend_from_slice(&[8, 2, 0, 0, 0, 1, 2, 3, 4]);
    d.extend_from_slice(b"rest of the file");
    d
}

fn doc_with_picture() -> Document {
    let mut doc = text::from_plain_text("Before after");
    let chp = doc.main.paras[0].chp_of_char(0);
    let mut pic = quill_core::Picture::from_bytes(png()).unwrap();
    pic.size = (1200, 300);
    pic.crop.left = 10_000;
    pic.crop.right = 10_000;
    doc.insert_picture(
        quill_core::StoryId::Main,
        quill_core::Position::new(0, 7),
        pic,
        chp,
    );
    doc
}

#[test]
fn as_many_pictures_as_allowed_can_be_saved_and_opened_again() {
    use quill_core::limits::MAX_PICTURES;
    let mut doc = text::from_plain_text("Pictures");
    let chp = doc.main.paras[0].chp_of_char(0);
    let add = |doc: &mut Document, i: usize| {
        let mut data = png();
        data.extend_from_slice(format!(" picture {i}").as_bytes());
        let pic = quill_core::Picture::from_bytes(data).unwrap();
        doc.insert_picture(
            quill_core::StoryId::Main,
            quill_core::Position::new(0, 0),
            pic,
            chp,
        );
    };
    for i in 0..MAX_PICTURES {
        add(&mut doc, i);
    }
    assert_eq!(doc.size().pictures, MAX_PICTURES);
    assert!(doc.size().too_large().is_none());
    for format in [Format::Quill, Format::Docx] {
        let path = std::env::temp_dir().join(format!(
            "quill-pictures-{}.{}",
            std::process::id(),
            format.extensions()[0]
        ));
        save(&doc, &path, format).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.size().pictures, MAX_PICTURES, "{format:?}");
        let _ = std::fs::remove_file(&path);
    }
    // One more is over the limit, and saving says so instead of
    // writing a file that cannot be opened.
    add(&mut doc, MAX_PICTURES);
    let why = doc.size().too_large().unwrap();
    assert!(why.contains("pictures"), "{why}");
    let path = std::env::temp_dir().join(format!("quill-pictures-over-{}.qwp", std::process::id()));
    assert!(save(&doc, &path, Format::Quill).is_err());
    assert!(!path.exists());
}

#[test]
fn many_sections_with_their_own_headers_save_and_open_again() {
    use quill_core::para::Mark;
    // 700 sections, each with headers of its own, and a picture: a
    // .docx writes each header and footer as a part with its own list
    // of pictures, more parts than the archive reader used to take.
    let mut doc = text::from_plain_text(&"Section\n".repeat(700));
    let chp = doc.main.paras[0].chp_of_char(0);
    let pic = quill_core::Picture::from_bytes(png()).unwrap();
    doc.insert_picture(
        quill_core::StoryId::Main,
        quill_core::Position::new(0, 0),
        pic,
        chp,
    );
    let header = text::from_plain_text("Own header").main;
    let n = doc.main.paras.len();
    for p in &mut doc.main.paras[..n - 1] {
        p.mark = Mark::SectionBreak;
        let mut section = doc.section.clone();
        section.headers = Some(std::sync::Arc::new(vec![header.clone(); 6]));
        p.section = Some(Box::new(section));
    }
    assert!(doc.size().headers > 4_000);
    assert!(doc.size().too_large().is_none());
    let path = std::env::temp_dir().join(format!("quill-sections-{}.docx", std::process::id()));
    save(&doc, &path, Format::Docx).unwrap();
    let back = load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(back.sections().len(), doc.sections().len());
    assert_eq!(back.size().pictures, 1);
    // Past the limit, it is too large, by its headers.
    let mut more = doc.clone();
    let extra = quill_core::limits::MAX_HEADERS / 6;
    let mut section = doc.section.clone();
    section.headers = Some(std::sync::Arc::new(vec![header.clone(); 6]));
    for _ in 0..extra {
        let mut p = more.main.paras[0].clone();
        p.mark = Mark::SectionBreak;
        p.section = Some(Box::new(section.clone()));
        more.main.paras.insert(0, p);
    }
    let why = more.size().too_large().unwrap();
    assert!(why.contains("headers"), "{why}");
}

#[test]
fn pictures_survive_docx_and_rtf() {
    let doc = doc_with_picture();
    let want = doc.picture(1).unwrap().clone();
    let docx = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
    let rtf = super::rtf::read(&super::rtf::write(&doc)).unwrap();
    for back in [docx, rtf] {
        let p = &back.main.paras[0];
        assert_eq!(p.text, "Before \u{FFFC}after");
        let pic = back.picture(p.chp_of_char(7).object).unwrap();
        assert_eq!(pic.data, want.data);
        // RTF keeps the scale in whole percents.
        assert!((pic.size.0 - want.size.0).abs() <= want.size.0 / 100);
        assert!((pic.size.1 - want.size.1).abs() <= want.size.1 / 100);
        assert_eq!(pic.crop, want.crop);
        assert_eq!(p.chp_of_char(10).object, 0);
    }
}

#[test]
fn docx_reuses_one_image_part_for_repeated_placements() {
    let mut doc = doc_with_picture();
    let chp = doc.main.paras[0].mark_chp;
    let second = doc.picture(1).unwrap().scaled_to(600, 150);
    doc.insert_picture(StoryId::Main, Position::new(0, 0), second, chp);
    let bytes = super::docx::write(&doc, 0);
    let archive = super::zip::Archive::open(&bytes).unwrap();
    assert_eq!(
        archive
            .names()
            .filter(|n| n.starts_with("word/media/"))
            .count(),
        1
    );
    let back = super::docx::read(&bytes).unwrap();
    let first = back
        .picture(back.main.paras[0].chp_of_char(0).object)
        .unwrap();
    let second = back
        .picture(back.main.paras[0].chp_of_char(10).object)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&first.data, &second.data));
}

#[test]
fn frames_survive_docx_and_rtf() {
    use quill_core::{Frame, FrameH, FrameRel, FrameV};
    let mut doc = text::from_plain_text("O\nnce upon\nPlaced\nEnd");
    let drop = Frame {
        drop_lines: 3,
        from_text: 40,
        ..Default::default()
    };
    let placed = Frame {
        h: FrameH::Right,
        h_rel: FrameRel::Margin,
        v: FrameV::At(1440),
        v_rel: FrameRel::Page,
        width: Some(2880),
        from_text: 180,
        drop_lines: 0,
        in_margin: false,
    };
    doc.main.paras[0].pap.frame = Some(drop);
    doc.main.paras[2].pap.frame = Some(placed);
    let docx = super::docx::read(&super::docx::write(&doc, 0)).unwrap();
    let rtf = super::rtf::read(&super::rtf::write(&doc)).unwrap();
    for back in [docx, rtf] {
        assert_eq!(back.main.paras[0].pap.frame, Some(drop));
        assert_eq!(back.main.paras[1].pap.frame, None);
        assert_eq!(back.main.paras[2].pap.frame, Some(placed));
    }
}

#[test]
fn imported_drop_caps_ignore_horizontal_frame_position_only() {
    use quill_core::{FrameH, FrameRel, FrameV};
    // The cap's frame controls come from the same producer as ordinary
    // frames, but its horizontal position belongs to the following text.
    for (kind, rtf_kind, is_drop) in [("drop", 1, true), ("margin", 2, true), ("none", 0, false)] {
        let body = format!(
            r#"<w:p><w:pPr><w:framePr w:dropCap="{kind}" w:lines="3" w:hAnchor="page" w:xAlign="center" w:vAnchor="text" w:hSpace="40"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="96"/></w:rPr><w:t>W</w:t></w:r></w:p><w:p><w:r><w:t>rapped text</w:t></w:r></w:p>"#
        );
        let docx = super::docx::tests_read_body(&body);
        // Check both control orders: normalization must wait until all
        // paragraph properties have been read.
        for drop_first in [false, true] {
            let drop = if is_drop {
                format!(r"\dropcapli3\dropcapt{rtf_kind}")
            } else {
                String::new()
            };
            let position = r"\phpg\posxc\pvpara\dxfrtext40";
            let controls = if drop_first {
                format!("{drop}{position}")
            } else {
                format!("{position}{drop}")
            };
            let rtf = format!(r"{{\rtf1\pard{controls}{{\b\fs96 W}}\par\pard rapped text\par}}");
            let rtf = super::rtf::read(rtf.as_bytes()).unwrap();
            for doc in [&docx, &rtf] {
                let cap = &doc.main.paras[0];
                assert_eq!(cap.text, "W");
                assert!(cap.chp_of_char(0).bold);
                assert_eq!(cap.chp_of_char(0).size, 96);
                let f = cap.pap.frame.unwrap();
                assert_eq!(
                    f.h,
                    if is_drop {
                        FrameH::Left
                    } else {
                        FrameH::Center
                    }
                );
                assert_eq!(
                    f.h_rel,
                    if is_drop {
                        FrameRel::Column
                    } else {
                        FrameRel::Page
                    }
                );
                assert_eq!(f.v, FrameV::InLine);
                assert_eq!(f.width, None);
                assert_eq!(f.from_text, 40);
                assert_eq!(f.drop_lines, if is_drop { 3 } else { 0 });
                assert_eq!(f.in_margin, kind == "margin");
                assert_eq!(doc.main.paras[1].text, "rapped text");
                assert_eq!(doc.main.paras[1].pap.frame, None);
                let native = super::native::read(&super::native::write(doc, 0)).unwrap();
                let saved_docx = super::docx::read(&super::docx::write(&native, 0)).unwrap();
                let saved_rtf = super::rtf::read(&super::rtf::write(&native)).unwrap();
                for back in [
                    super::rtf::read(&super::rtf::write(&saved_docx)).unwrap(),
                    super::docx::read(&super::docx::write(&saved_rtf, 0)).unwrap(),
                    native,
                    saved_docx,
                    saved_rtf,
                ] {
                    // An inline frame's vertical anchor is unused and
                    // DOCX canonicalizes it to Margin.
                    let back_frame = back.main.paras[0].pap.frame.unwrap();
                    assert_eq!(
                        back_frame,
                        quill_core::Frame {
                            v_rel: back_frame.v_rel,
                            ..f
                        }
                    );
                    assert_eq!(back.main.paras[0].chp_of_char(0), cap.chp_of_char(0));
                }
            }
        }
    }
}

#[test]
fn sections_survive_docx_and_rtf() {
    use quill_core::{PageNumberFormat, SectionStart};
    // Title, a section break, then the body in two columns.
    let mut doc = text::from_plain_text("Title\nBody");
    doc.insert_section_break(Position::new(0, 5));
    doc.format_sections(0, 0, |s| {
        s.columns = 1;
        s.page_number_format = PageNumberFormat::LowerRoman;
    });
    doc.format_sections(2, 2, |s| {
        s.columns = 2;
        s.column_spacing = 360;
        s.start = SectionStart::NoBreak;
        s.restart_page_numbers = true;
        s.line_numbers = quill_core::LineNumbering::Continuous;
        s.line_count_by = 5;
        s.line_from_text = Some(360);
    });
    for back in [
        super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
        super::rtf::read(&super::rtf::write(&doc)).unwrap(),
    ] {
        assert_eq!(back.sections(), doc.sections());
        for p in [0, 2] {
            assert_eq!(back.section_props_at(p), doc.section_props_at(p));
        }
    }
}

#[test]
fn footnotes_survive_docx_and_rtf() {
    use quill_core::document::FootnotePosition;
    use quill_core::notes::note_ids;
    let mut doc = text::from_plain_text("Text with notes\nMore");
    let a = doc.insert_note(Position::new(0, 4), Default::default(), None);
    let b = doc.insert_note(Position::new(1, 4), Default::default(), Some("*".into()));
    let at = doc.note_text_start(a).unwrap();
    doc.insert_text(StoryId::Notes, at, "First note.", Default::default());
    let at = doc.note_text_start(b).unwrap();
    doc.insert_text(StoryId::Notes, at, "Starred.", Default::default());
    let texts = |d: &Document| -> Vec<String> {
        let labels = d.note_labels(None);
        let ranges = d.note_ranges();
        note_ids(&d.main.paras)
            .into_iter()
            .map(|id| {
                let r = ranges[&id].clone();
                let body: String = d.notes.paras[r].iter().map(|p| p.text.clone()).collect();
                format!("{}|{}", labels[&id], body)
            })
            .collect()
    };
    let want = texts(&doc);
    assert_eq!(want.len(), 2);
    for pos in [
        FootnotePosition::BottomOfPage,
        FootnotePosition::EndOfDocument,
    ] {
        doc.props.footnote_position = pos;
        for back in [
            super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
            super::rtf::read(&super::rtf::write(&doc)).unwrap(),
        ] {
            assert_eq!(texts(&back), want);
            assert_eq!(back.props.footnote_position, pos);
            assert_eq!(back.main.paras[0].text, doc.main.paras[0].text);
        }
    }
}

#[test]
fn annotations_survive_docx_and_rtf() {
    use quill_core::notes::{AnnotationInfo, annotation_ids};
    let mut doc = text::from_plain_text("Some text to annotate");
    let info = AnnotationInfo {
        initials: "LK".into(),
        author: "Lucas K".into(),
        date: "2026-10-14".into(),
        ..Default::default()
    };
    let id = doc.insert_annotation(Position::new(0, 4), Default::default(), info.clone());
    let at = doc.annotation_text_start(id).unwrap();
    doc.insert_text(StoryId::Annotations, at, "Check this.", Default::default());
    for (back, dated) in [
        (
            super::docx::read(&super::docx::write(&doc, 0)).unwrap(),
            true,
        ),
        (super::rtf::read(&super::rtf::write(&doc)).unwrap(), false),
    ] {
        let ids = annotation_ids(&back.main.paras);
        assert_eq!(ids.len(), 1);
        assert_eq!(back.annotation_text(ids[0]), "Check this.");
        let got = &back.annotation_info[&ids[0]];
        assert_eq!(
            (got.initials.as_str(), got.author.as_str()),
            ("LK", "Lucas K")
        );
        if dated {
            assert_eq!(got.date, "2026-10-14");
        }
        assert_eq!(
            back.main.paras[0]
                .text
                .replace(quill_core::para::chars::ANNOTATION, ""),
            "Some text to annotate"
        );
    }
}
