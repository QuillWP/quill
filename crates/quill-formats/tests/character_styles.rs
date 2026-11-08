use quill_core::character_styles::CharacterFormat;
use quill_core::{CharProps, Document, Position, Rgb, StoryId};
use quill_formats::{docx, html, native, odt, odt_read, rtf};

fn fixture() -> Document {
    let mut d =
        quill_core::text::from_plain_text("Emphasis and a direct override\nLarge paragraph");
    let id = d
        .add_character_style(
            "Emphasis",
            CharacterFormat::new(
                CharProps {
                    italic: true,
                    ..Default::default()
                },
                1 << 3,
            ),
        )
        .unwrap();
    d.apply_character_style(StoryId::Main, Position::new(0, 0), Position::new(0, 8), id);
    d.apply_character_style(StoryId::Main, Position::new(1, 0), Position::new(1, 5), id);
    d.format_chars(
        StoryId::Main,
        Position::new(0, 2),
        Position::new(0, 4),
        |c| c.italic = false,
    );
    let mut pap = d.styles.get(0).pap.clone();
    pap.left = 720;
    let font = d.font_id("Georgia");
    let large = d.add_style(
        "Large",
        0,
        None,
        pap,
        CharProps {
            font,
            size: 42,
            ..d.styles.get(0).chp
        },
    );
    d.apply_style(StoryId::Main, 1, 1, large);
    d
}

#[test]
fn native_and_exchange_formats_keep_definitions_uses_direct_overrides_and_inheritance() {
    let d = fixture();
    let html_path =
        std::env::temp_dir().join(format!("quill-character-{}.html", std::process::id()));
    std::fs::write(&html_path, html::write(&d)).unwrap();
    let html_back = quill_formats::load(&html_path).unwrap();
    std::fs::remove_file(html_path).unwrap();
    let mut versions = Vec::new();
    for (name, bytes) in [
        ("native", native::write(&d, 0)),
        ("docx", docx::write(&d, 0)),
        ("rtf", rtf::write(&d)),
    ] {
        let back = match name {
            "native" => native::read(&bytes),
            "docx" => docx::read(&bytes),
            _ => rtf::read(&bytes),
        }
        .unwrap();
        versions.push((name, back));
    }
    versions.push(("odt", odt_read::read(&odt::write(&d, 0)).unwrap()));
    versions.push(("html", html_back));
    if let Ok(dir) = std::env::var("QUILL_CHARACTER_FIXTURES") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("character-styles.docx"), docx::write(&d, 0)).unwrap();
        std::fs::write(dir.join("character-styles.qwp"), native::write(&d, 0)).unwrap();
        std::fs::write(dir.join("character-styles.rtf"), rtf::write(&d)).unwrap();
    }
    if let Ok(path) = std::env::var("QUILL_CHARACTER_RETURN") {
        versions.push((
            "Word return",
            quill_formats::load(std::path::Path::new(&path)).unwrap(),
        ));
    }
    for (name, mut back) in versions {
        let id = back
            .styles
            .find_character("Emphasis")
            .unwrap_or_else(|| panic!("{name}: missing definition"));
        assert_eq!(
            back.main.paras[0].chp_of_char(0).character_style,
            id,
            "{name}"
        );
        assert!(back.main.paras[0].chp_of_char(0).italic, "{name}");
        assert!(!back.main.paras[0].chp_of_char(2).italic, "{name}");
        assert_eq!(back.main.paras[1].chp_of_char(0).size, 42, "{name}");
        assert_eq!(
            back.font_name(back.main.paras[1].chp_of_char(0).font),
            "Georgia",
            "{name}"
        );
        let props = CharProps {
            italic: true,
            color: Rgb(0xCC2200),
            ..Default::default()
        };
        back.redefine_character_style(id, CharacterFormat::new(props, (1 << 3) | (1 << 11)));
        assert_eq!(
            back.main.paras[0].chp_of_char(0).color,
            props.color,
            "{name}"
        );
        assert!(!back.main.paras[0].chp_of_char(2).italic, "{name}");
    }
}

#[test]
fn repeated_docx_and_rtf_cycles_keep_character_style_names_and_masks() {
    let mut doc = fixture();
    for i in 0..12 {
        doc.add_character_style(
            &format!("Unused {i}"),
            CharacterFormat::new(
                CharProps {
                    bold: true,
                    ..Default::default()
                },
                1 << 2,
            ),
        )
        .unwrap();
    }
    let expected: std::collections::BTreeMap<_, _> = doc
        .styles
        .characters
        .iter()
        .map(|s| (s.name.clone(), s.format))
        .collect();
    for _ in 0..3 {
        doc = docx::read(&docx::write(&doc, 0)).unwrap();
        doc = rtf::read(&rtf::write(&doc)).unwrap();
        let actual: std::collections::BTreeMap<_, _> = doc
            .styles
            .characters
            .iter()
            .map(|s| (s.name.clone(), s.format))
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn rtf_appearance_remains_correct_without_expanding_style_references() {
    let doc = fixture();
    let text = String::from_utf8(rtf::write(&doc)).unwrap();
    let mut without_references = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("\\cs") {
        without_references.push_str(&rest[..at]);
        rest = &rest[at + 3..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        rest = &rest[digits..];
    }
    without_references.push_str(rest);
    let back = rtf::read(without_references.as_bytes()).unwrap();
    assert!(back.main.paras[0].chp_of_char(0).italic);
    assert!(!back.main.paras[0].chp_of_char(2).italic);
    assert_eq!(back.main.paras[1].chp_of_char(0).size, 42);
    assert_eq!(
        back.font_name(back.main.paras[1].chp_of_char(0).font),
        "Georgia"
    );
}

/// A heading style linked to a Callout character style (18 pt, Bold).
fn linked() -> (Document, u16) {
    let mut d = quill_core::text::from_plain_text("Title\nBody");
    let heading = d.styles.find("heading 1").unwrap();
    d.apply_style(StoryId::Main, 0, 0, heading);
    let callout = d
        .add_character_style(
            "Callout",
            CharacterFormat::new(
                CharProps {
                    bold: true,
                    size: 36,
                    ..Default::default()
                },
                (1 << 1) | (1 << 2),
            ),
        )
        .unwrap();
    let (pap, own) = (d.styles.get(heading).pap.clone(), d.styles.get(heading).chp);
    d.redefine_style_linked(heading, pap, own, callout);
    (d, heading)
}

#[test]
fn quill_files_keep_a_paragraph_styles_link_to_a_character_style() {
    let (d, heading) = linked();
    let back = native::read(&native::write(&d, 0)).unwrap();
    let (a, b) = (d.styles.get(heading), back.styles.get(heading));
    assert_eq!(b.character, a.character);
    assert_eq!(b.own(), a.own());
    assert_eq!(b.chp, a.chp);
    assert_eq!(back.styles, d.styles);
}

#[test]
fn exchange_formats_keep_the_linked_formatting_without_the_link() {
    let (d, heading) = linked();
    let look = d.styles.get(heading).chp;
    let back = docx::read(&docx::write(&d, 0)).unwrap();
    let id = back.styles.find("heading 1").unwrap();
    let style = back.styles.get(id);
    assert_eq!(style.character, 0);
    assert_eq!((style.chp.size, style.chp.bold), (look.size, look.bold));
    let text = back.main.paras[0].chp_of_char(0);
    assert_eq!((text.size, text.bold), (look.size, look.bold));
}

/// A style that sets every property a character style can: font, size,
/// color, underline, position, spacing and each on/off property.
fn every_property() -> Document {
    let mut d = quill_core::text::from_plain_text("Styled words and plain words");
    let font = d.font_id("Georgia");
    let everything = d
        .add_character_style(
            "Everything",
            CharacterFormat::new(
                CharProps {
                    font,
                    size: 30,
                    bold: true,
                    italic: true,
                    underline: quill_core::Underline::Single,
                    strike: true,
                    outline: true,
                    shadow: true,
                    small_caps: true,
                    all_caps: true,
                    hidden: true,
                    color: Rgb(0x1F5FAA),
                    position: 60,
                    spacing: 40,
                    ..Default::default()
                },
                CharacterFormat::ALL,
            ),
        )
        .unwrap();
    d.apply_character_style(
        StoryId::Main,
        Position::new(0, 0),
        Position::new(0, 12),
        everything,
    );
    d
}

#[test]
fn every_property_a_character_style_sets_survives_docx_rtf_and_odt() {
    let d = every_property();
    let want = d.styles.character(1).unwrap().clone();
    let look = d.main.paras[0].chp_of_char(0);
    // QUILL_CHARACTER_FIXTURES=dir writes the files for checking in Word and
    // other word processors; QUILL_EVERYTHING_RETURN=a.docx,b.docx checks
    // what those programs saved back.
    if let Ok(dir) = std::env::var("QUILL_CHARACTER_FIXTURES") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("everything.docx"), docx::write(&d, 0)).unwrap();
        std::fs::write(dir.join("everything.rtf"), rtf::write(&d)).unwrap();
        std::fs::write(dir.join("everything.odt"), odt::write(&d, 0)).unwrap();
    }
    let mut versions = vec![
        ("docx".to_string(), docx::read(&docx::write(&d, 0)).unwrap()),
        ("rtf".to_string(), rtf::read(&rtf::write(&d)).unwrap()),
        (
            "odt".to_string(),
            odt_read::read(&odt::write(&d, 0)).unwrap(),
        ),
    ];
    if let Ok(paths) = std::env::var("QUILL_EVERYTHING_RETURN") {
        for path in paths.split(',') {
            let back = quill_formats::load(std::path::Path::new(path)).unwrap();
            versions.push((path.to_string(), back));
        }
    }
    for (name, back) in versions {
        let id = back
            .styles
            .find_character("Everything")
            .unwrap_or_else(|| panic!("{name}: the style is gone"));
        let got = back.styles.character(id).unwrap();
        assert_eq!(
            got.format.mask, want.format.mask,
            "{name}: what the style sets"
        );
        let mut props = got.format.props;
        props.font = 0;
        let mut expected = want.format.props;
        expected.font = 0;
        assert_eq!(props, expected, "{name}: the style's values");
        assert_eq!(back.font_name(got.format.props.font), "Georgia", "{name}");
        let c = back.main.paras[0].chp_of_char(0);
        assert_eq!(c.character_style, id, "{name}: the text keeps the style");
        assert_eq!(c.direct_format, 0, "{name}: nothing became hand formatting");
        assert_eq!(
            (
                c.size,
                c.bold,
                c.italic,
                c.underline,
                c.color,
                c.position,
                c.spacing
            ),
            (
                look.size,
                look.bold,
                look.italic,
                look.underline,
                look.color,
                look.position,
                look.spacing
            ),
            "{name}: the text looks the same"
        );
        assert_eq!(
            back.main.paras[0].chp_of_char(13).character_style,
            0,
            "{name}"
        );
    }
}

#[test]
fn linked_paragraph_styles_save_their_look_in_rtf_and_odt_too() {
    let (d, heading) = linked();
    let look = d.styles.get(heading).chp;
    for (name, back) in [
        ("rtf", rtf::read(&rtf::write(&d)).unwrap()),
        ("odt", odt_read::read(&odt::write(&d, 0)).unwrap()),
    ] {
        let text = back.main.paras[0].chp_of_char(0);
        assert_eq!((text.size, text.bold), (look.size, look.bold), "{name}");
        assert!(
            back.styles.styles.iter().all(|s| s.character == 0),
            "{name}"
        );
    }
}
