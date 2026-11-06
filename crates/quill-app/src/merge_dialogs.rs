//! File > Print Merge and View > Print Merge Helper. The Print Merge
//! dialog is laid out from the original; the helper,
//! a bar under the formatting toolbar in the original, is a small window here.

use crate::dialog::Dialog;
use crate::docview::DocView;
use quill_core::Document;
use quill_core::merge::{
    AskKey, AskRequest, CLOSE, Data, MergeError, OPEN, data_instruction, merge_interactive,
    merge_with_includes,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// File > Print Merge.
pub fn print_merge() -> Dialog {
    let mut d = Dialog::new("Print Merge", 291.0, 172.0);
    d.group("Records", 9.0, 17.0, 133.0, 68.0);
    d.radio("all", "All", 15.0, 26.0, 1, true);
    d.radio("range", "From:", 15.0, 43.0, 1, false);
    d.field("from", "", 72.0, 39.0, 66.0);
    d.text("To:", 47.0, 65.0);
    d.field("to", "", 72.0, 62.0, 66.0);
    d.button("ok", "OK", 180.0, 17.0, 99.0, true);
    d.button("cancel", "Cancel", 182.0, 45.0, 95.0, false);
    d.button("helper", "Show Helper", 182.0, 68.0, 95.0, false);
    d.group("Merge Results", 9.0, 102.0, 269.0, 57.0);
    d.radio("print", "Merge and Print Results", 15.0, 110.0, 2, false);
    d.radio(
        "newfile",
        "Merge and Save Results in New File",
        15.0,
        127.0,
        2,
        true,
    );
    d.radio("check", "Only Check for Errors", 15.0, 144.0, 2, false);
    d
}

/// Typing a record number picks From.
pub fn print_merge_changed(d: &mut Dialog, id: &str) {
    if matches!(id, "from" | "to") && !d.field_text(id).is_empty() {
        d.set_on("range", true);
        d.set_on("all", false);
    }
}

/// What the merge made.
pub enum Outcome {
    NewFile(Document),
    Print(Document),
    Checked(Vec<MergeError>),
}

#[derive(Clone, Copy, Debug)]
enum Destination {
    NewFile,
    Print,
    Check,
}

/// A merge paused between native ASK dialogs.
#[derive(Clone, Debug)]
pub struct Session {
    main: Document,
    data: Data,
    range: Option<(usize, usize)>,
    base: Option<PathBuf>,
    destination: Destination,
    answers: HashMap<AskKey, String>,
}

/// The next thing an interactive merge needs.
pub enum Progress {
    Ask(AskRequest),
    Done(Box<Outcome>),
}

/// The data document named in the main document's «DATA», found beside
/// the main document when the name has no folder.
pub fn data_for(view: &DocView) -> Result<(String, Data), String> {
    let Some((name, _, _)) = data_instruction(&view.doc) else {
        return Err("This document has no DATA instruction. Use View > Print Merge Helper to choose a data document.".into());
    };
    let path = std::path::PathBuf::from(&name);
    let path = if path.is_absolute() {
        path
    } else {
        view.path
            .as_ref()
            .and_then(|p| p.parent())
            .map_or(path.clone(), |dir| dir.join(&path))
    };
    let doc = quill_formats::load(&path).map_err(|e| {
        format!("The data document \u{201C}{name}\u{201D} could not be opened: {e}")
    })?;
    Ok((name, Data::from_document(&doc)?))
}

/// Start an interactive merge from the choices in the Print Merge dialog.
pub fn start(d: &Dialog, view: &DocView) -> Result<Session, String> {
    let (_, data) = data_for(view)?;
    let range = if d.radio_on(1) == "range" {
        let n = |id| d.field_text(id).trim().parse::<usize>().ok();
        let from = n("from").unwrap_or(1).max(1);
        let to = n("to").unwrap_or(usize::MAX).max(from);
        Some((from - 1, to - 1))
    } else {
        None
    };
    let destination = match d.radio_on(2) {
        "print" => Destination::Print,
        "check" => Destination::Check,
        _ => Destination::NewFile,
    };
    Ok(Session {
        main: view.doc.clone(),
        data,
        range,
        base: view
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from),
        destination,
        answers: HashMap::new(),
    })
}

fn load_include(base: Option<&std::path::Path>, name: &str) -> Result<Document, String> {
    let path = std::path::PathBuf::from(name);
    let path = if path.is_absolute() {
        path
    } else {
        base.map_or(path.clone(), |dir| dir.join(&path))
    };
    quill_formats::load(&path)
        .map_err(|error| format!("The included document “{name}” could not be opened: {error}"))
}

/// Continue until another ASK is needed or the merge is complete.
pub fn advance(session: &mut Session) -> Progress {
    let base = session.base.as_deref();
    let mut include = |name: &str| load_include(base, name);
    let result = if matches!(session.destination, Destination::Check) {
        Ok(merge_with_includes(
            &session.main,
            &session.data,
            session.range,
            &mut include,
        ))
    } else {
        merge_interactive(
            &session.main,
            &session.data,
            session.range,
            &mut include,
            &session.answers,
        )
    };
    match result {
        Err(request) => Progress::Ask(request),
        Ok((_, errors)) if !errors.is_empty() => Progress::Done(Box::new(Outcome::Checked(errors))),
        Ok((doc, errors)) => Progress::Done(Box::new(match session.destination {
            Destination::Print => Outcome::Print(doc),
            Destination::Check => Outcome::Checked(errors),
            Destination::NewFile => Outcome::NewFile(doc),
        })),
    }
}

/// Record an ASK response before advancing the merge again.
pub fn answer(session: &mut Session, request: &AskRequest, answer: String) {
    session.answers.insert(request.key.clone(), answer);
}

/// A standard native-control ASK prompt for one recipient.
pub fn ask(request: &AskRequest) -> Dialog {
    let mut d = Dialog::new("Print Merge", 370.0, 118.0);
    d.text(&format!("Record {}", request.key.record + 1), 12.0, 12.0);
    d.text(&request.prompt, 12.0, 34.0);
    d.field("answer", &request.default, 12.0, 57.0, 346.0);
    d.button("ok", "OK", 198.0, 88.0, 76.0, true);
    d.button("cancel", "Cancel", 282.0, 88.0, 76.0, false);
    d.focus("answer");
    d
}

/// The message after Only Check for Errors.
pub fn check_message(errors: &[MergeError]) -> String {
    if errors.is_empty() {
        "No errors were found.".into()
    } else {
        errors
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ----- the helper -----

const INSTRUCTIONS: [(&str, &str); 10] = [
    ("IF…ENDIF", "\u{AB}IF \u{BB}\u{AB}ENDIF\u{BB}"),
    (
        "IF…ELSE…ENDIF",
        "\u{AB}IF \u{BB}\u{AB}ELSE\u{BB}\u{AB}ENDIF\u{BB}",
    ),
    ("ELSE", "\u{AB}ELSE\u{BB}"),
    ("ENDIF", "\u{AB}ENDIF\u{BB}"),
    ("NEXT", "\u{AB}NEXT\u{BB}"),
    ("NEXTIF", "\u{AB}NEXTIF \u{BB}"),
    ("SKIPIF", "\u{AB}SKIPIF \u{BB}"),
    ("INCLUDE", "\u{AB}INCLUDE \u{BB}"),
    ("SET", "\u{AB}SET =\u{BB}"),
    ("ASK", "\u{AB}ASK =\u{BB}"),
];

/// View > Print Merge Helper, once a data document is named: its fields
/// and the original's merge instructions, each inserted at the insertion point.
pub fn helper(view: &DocView) -> Result<(Dialog, Vec<String>), String> {
    let (name, data) = data_for(view)?;
    let mut d = Dialog::new("Print Merge Helper", 460.0, 118.0);
    d.modeless = true;
    d.close_box = true;
    d.text("Data:", 10.0, 8.0);
    d.text_as("data", &name, 52.0, 8.0);
    let fields: Vec<&str> = data.fields.iter().map(String::as_str).collect();
    d.menu_button("field", "Insert Field Name", &fields, 10.0, 32.0, 170.0);
    let names: Vec<&str> = INSTRUCTIONS.iter().map(|x| x.0).collect();
    d.menu_button(
        "instruction",
        "Insert Print Merge Field",
        &names,
        190.0,
        32.0,
        180.0,
    );
    d.note("format_help", "Data: comma- or tab-separated text, or a document table.\nFirst row: field names. Each following row: one record.", 10.0, 70.0, 440.0);
    Ok((d, data.fields))
}

/// A choice from one of the helper's menus: put it in the text.
pub fn helper_changed(fields: &[String], d: &Dialog, view: &mut DocView, id: &str) {
    let text = match id {
        "field" => fields
            .get(d.selected("field"))
            .map(|f| format!("{OPEN}{f}{CLOSE}")),
        "instruction" => INSTRUCTIONS
            .get(d.selected("instruction"))
            .map(|x| x.1.to_string()),
        _ => None,
    };
    if let Some(t) = text {
        view.insert_plain_text("Insert Field", &t);
    }
}

/// Name `path` as the data document: a «DATA» instruction at the start,
/// replacing any there. The name is relative when the data sits beside
/// the document.
pub fn attach_data(view: &mut DocView, path: &std::path::Path) {
    let name = match (view.path.as_ref().and_then(|p| p.parent()), path.parent()) {
        (Some(a), Some(b)) if a == b => path.file_name().map_or(path.display().to_string(), |n| {
            n.to_string_lossy().into_owned()
        }),
        _ => path.display().to_string(),
    };
    let instruction = format!("{OPEN}DATA {name}{CLOSE}");
    match data_instruction(&view.doc) {
        Some((_, s, e)) => {
            view.set_selection(quill_core::Selection::new(s, e));
            view.insert_plain_text("Print Merge Helper", &instruction);
        }
        None => {
            view.set_selection(quill_core::Selection::caret(quill_core::Position::new(
                0, 0,
            )));
            view.insert_plain_text("Print Merge Helper", &format!("{instruction}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_with_a_data_file_beside_the_document() {
        let dir = std::env::temp_dir().join(format!("quill-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("people.txt"),
            "Name\tCity\nAda\tLondon\nBob\tParis\n",
        )
        .unwrap();
        let doc = quill_core::text::from_plain_text("Dear \u{AB}Name\u{BB} of \u{AB}City\u{BB}");
        let mut v = DocView::new(doc, "Letter".into());
        v.path = Some(dir.join("letter.docx"));
        attach_data(&mut v, &dir.join("people.txt"));
        assert_eq!(v.doc.main.paras[0].text, "\u{AB}DATA people.txt\u{BB}");
        let mut session = start(&print_merge(), &v).unwrap();
        let Progress::Done(outcome) = advance(&mut session) else {
            panic!("merge failed");
        };
        let Outcome::NewFile(out) = *outcome else {
            panic!("merge made the wrong result");
        };
        let texts: Vec<&str> = out.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Dear Ada of London", "Dear Bob of Paris"]);
        let (h, fields) = helper(&v).unwrap();
        assert_eq!(fields, ["Name", "City"]);
        assert!(h.modeless);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_merge_never_prints_or_opens_incomplete_letters() {
        for destination in [Destination::Print, Destination::NewFile, Destination::Check] {
            let mut session = Session {
                main: quill_core::text::from_plain_text("Dear «Nmae»"),
                data: Data {
                    fields: vec!["Name".into()],
                    records: vec![vec!["Ada".into()]],
                },
                range: None,
                base: None,
                destination,
                answers: HashMap::new(),
            };
            let Progress::Done(outcome) = advance(&mut session) else {
                panic!("unexpected prompt")
            };
            let Outcome::Checked(errors) = *outcome else {
                panic!("incomplete letters escaped")
            };
            assert_eq!(errors.len(), 1);
            assert!(check_message(&errors).contains("Nmae"));
            assert_eq!(session.main.main.paras[0].text, "Dear «Nmae»");
        }
    }

    #[test]
    fn ask_uses_a_native_prompt_for_each_record() {
        let dir = std::env::temp_dir().join(format!("quill-ask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("people.txt"), "Name\nAda\nBob\n").unwrap();
        let doc = quill_core::text::from_plain_text(
            "«DATA people.txt»\n«ASK greeting=?Greeting?»«greeting» «Name»",
        );
        let mut view = DocView::new(doc, "Letter".into());
        view.path = Some(dir.join("letter.qwp"));
        let mut session = start(&print_merge(), &view).unwrap();

        let Progress::Ask(first) = advance(&mut session) else {
            panic!("first ASK was not requested");
        };
        let prompt = ask(&first);
        assert_eq!(first.key.record, 0);
        assert!(
            prompt
                .items
                .iter()
                .all(|item| !matches!(item.kind, crate::dialog::Kind::Custom))
        );
        answer(&mut session, &first, "Hello".into());

        let Progress::Ask(second) = advance(&mut session) else {
            panic!("second ASK was not requested");
        };
        assert_eq!(second.key.record, 1);
        assert_eq!(second.default, "Hello");
        answer(&mut session, &second, "Welcome".into());

        let Progress::Done(outcome) = advance(&mut session) else {
            panic!("merge did not finish");
        };
        let Outcome::NewFile(out) = *outcome else {
            panic!("merge made the wrong result");
        };
        assert_eq!(out.main.paras[0].text, "Hello Ada");
        assert_eq!(out.main.paras[1].text, "Welcome Bob");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
