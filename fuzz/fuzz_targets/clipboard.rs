#![no_main]

use libfuzzer_sys::fuzz_target;
use quill_app_fuzz::clipboard_rich::RichContent;
use quill_core::character_styles::CharacterFormat;
use quill_core::{CharProps, Document, Position, Selection, StoryId};

#[path = "../support/editing.rs"]
mod editing;

/// A document that already has names a paste is likely to bring with it.
fn target() -> Document {
    let mut doc = quill_core::text::from_plain_text("Existing text");
    let props = CharProps {
        bold: true,
        ..Default::default()
    };
    for name in ["Emphasis", "Hyperlink", "Code"] {
        doc.add_character_style(name, CharacterFormat::new(props, 1 << 2));
    }
    doc.forget_history();
    doc
}

// The first byte chooses RTF or clipboard HTML and the answer to the missing
// font question. The rest is what another application put on the clipboard.
fuzz_target!(|data: &[u8]| {
    let Some((&choice, body)) = data.split_first() else {
        return;
    };
    let doc = if choice & 1 == 0 {
        quill_formats::rtf::read(body)
    } else {
        std::str::from_utf8(body)
            .map_err(|e| e.to_string())
            .and_then(quill_formats::html::read_clipboard)
    };
    let Ok(mut doc) = doc else {
        return;
    };
    doc.keep_in_bounds();
    let mut rich = RichContent::from_document(doc, None);
    let replace = choice & 2 != 0;
    let installed = |name: &str| name.len() % 3 != 0;
    if rich
        .resolve_missing_fonts("Georgia", installed, |_, _| Some(replace))
        .is_none()
    {
        return;
    }
    let mut target = target();
    let before = editing::round_trip::snapshot(&target);
    let selection = Selection::caret(Position::new(0, 4));
    target.begin("Paste", selection);
    let fragment = rich.prepare(&mut target);
    target.insert_fragment(StoryId::Main, Position::new(0, 4), &fragment);
    target.commit(selection);
    editing::valid(&target);
    editing::character_styles::check(&target);
    editing::round_trip::check_imported(&target);
    target.undo();
    assert_eq!(editing::round_trip::snapshot(&target), before, "Undo");
});
