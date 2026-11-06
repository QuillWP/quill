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
