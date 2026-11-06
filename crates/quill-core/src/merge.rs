//! File > Print Merge, as the original does it. The main document starts with
//! «DATA file» naming the data document; merge fields are written between
//! « and » (Option-\ and Option-Shift-\). The data document's first
//! paragraph (or a table's first row) names the fields, separated by tabs
//! or commas, and each following paragraph (or row) is a record.
//!
//! Fields: «name» for a value; «IF name="text"» … «ELSE» … «ENDIF» (also
//! <>, <, >, or «IF name» for "not empty"); «NEXT» to go on to the next
//! record in the same copy; «NEXTIF test» conditionally does the same;
//! «SKIPIF test» omits a record; «INCLUDE file» inserts another document;
//! and «SET name=text» and «ASK name=text» define reusable values.

use crate::document::Document;
use crate::para::{Mark, Paragraph};
use crate::props::{CharProps, ParaProps};
use crate::story::{Position, Story};
use std::collections::HashMap;

pub const OPEN: char = '\u{00AB}';
pub const CLOSE: char = '\u{00BB}';

/// The data: field names and records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Data {
    pub fields: Vec<String>,
    pub records: Vec<Vec<String>>,
}

// Empty fields still cost a String slot. Bound that overhead separately
// from document text, and reject rather than silently omit recipients.
const MAX_DATA_FIELDS_PER_RECORD: usize = 1024;
const MAX_DATA_CELLS: usize = 1_000_000;
const MAX_DATA_BYTES: usize = crate::limits::MAX_CHARACTERS * 4;

struct DataBudget {
    cells: usize,
    bytes: usize,
}

impl DataBudget {
    fn cell(&mut self, in_record: usize) -> Result<(), String> {
        if in_record >= MAX_DATA_FIELDS_PER_RECORD {
            return Err(format!(
                "Print Merge data has more than {MAX_DATA_FIELDS_PER_RECORD} fields in one record"
            ));
        }
        if self.cells == 0 {
            return Err(format!(
                "Print Merge data has more than {MAX_DATA_CELLS} cells"
            ));
        }
        self.cells -= 1;
        Ok(())
    }

    fn text(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or_else(|| "Print Merge data has too much text".to_string())?;
        Ok(())
    }
}

/// Split one record: by tabs if it has any, else by commas, with the original's
/// quotes around values that hold commas.
fn split_record(line: &str, budget: &mut DataBudget) -> Result<Vec<String>, String> {
    budget.text(line.len())?;
    budget.cell(0)?;
    let mut out = Vec::new();
    if line.contains('\t') {
        for field in line.split('\t') {
            if !out.is_empty() {
                budget.cell(out.len())?;
            }
            out.push(field.trim().to_string());
        }
        return Ok(out);
    }
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                budget.cell(out.len() + 1)?;
                out.push(std::mem::take(&mut cur).trim().to_string());
            }
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    Ok(out)
}

impl Data {
    /// The data in a document: a table's rows, or its paragraphs.
    /// Excessive data returns an error without a partial recipient list.
    pub fn from_document(doc: &Document) -> Result<Data, String> {
        Self::from_document_with_budget(
            doc,
            DataBudget {
                cells: MAX_DATA_CELLS,
                bytes: MAX_DATA_BYTES,
            },
        )
    }

    fn from_document_with_budget(doc: &Document, mut budget: DataBudget) -> Result<Data, String> {
        let paras = &doc.main.paras;
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut i = 0;
        while i < paras.len() {
            if rows.len() >= crate::limits::MAX_PARAGRAPHS {
                return Err("Print Merge data has too many records".into());
            }
            if paras[i].pap.in_table
                && let Some(row) = crate::table::row_at(paras, i)
            {
                let mut cells = Vec::new();
                for &(a, b) in &row.cells {
                    budget.cell(cells.len())?;
                    let mut text = String::new();
                    for (j, p) in paras[a..=b].iter().enumerate() {
                        budget.text(p.text.len() + usize::from(j != 0))?;
                        if j != 0 {
                            text.push('\n');
                        }
                        text.push_str(&p.text);
                    }
                    cells.push(text.trim().to_string());
                }
                rows.push(cells);
                i = row.end + 1;
                continue;
            }
            let t = paras[i].text.as_str();
            if !t.trim().is_empty() || t.contains('\t') {
                rows.push(split_record(t, &mut budget)?);
            }
            i += 1;
        }
        let mut rows = rows.into_iter();
        let fields = rows.next().unwrap_or_default();
        Ok(Data {
            fields,
            records: rows.collect(),
        })
    }

    fn value(&self, record: usize, name: &str) -> Option<String> {
        let k = self
            .fields
            .iter()
            .position(|f| f.eq_ignore_ascii_case(name))?;
        Some(
            self.records
                .get(record)?
                .get(k)
                .cloned()
                .unwrap_or_default(),
        )
    }
}

/// The data document a main document names in its «DATA file», and where
/// that instruction is.
pub fn data_instruction(doc: &Document) -> Option<(String, Position, Position)> {
    for (i, p) in doc.main.paras.iter().enumerate().take(3) {
        if let Some(s) = p.text.find(OPEN) {
            let inner = &p.text[s + OPEN.len_utf8()..];
            let e = inner.find(CLOSE)?;
            let body = inner[..e].trim();
            let name = body
                .strip_prefix("DATA")
                .or_else(|| body.strip_prefix("data"))?;
            let end = s + OPEN.len_utf8() + e + CLOSE.len_utf8();
            return Some((
                name.trim().trim_matches('"').to_string(),
                Position::new(i, s),
                Position::new(i, end),
            ));
        }
    }
    None
}

/// A problem found while merging (Only Check for Errors lists them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeError {
    pub message: String,
}

/// One ASK response, identified by the data record and variable name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AskKey {
    pub record: usize,
    pub name: String,
}

/// Input needed before an interactive merge can continue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskRequest {
    pub key: AskKey,
    pub prompt: String,
    pub default: String,
}

/// Something in the main document: a character with its format, a
/// paragraph's end, or a field.
enum Item {
    Text(char, CharProps),
    End(Box<Paragraph>),
    Field(String, CharProps),
}

const MAX_INCLUDE_DEPTH: usize = 8;

/// The main document as items, without its «DATA» instruction.
fn items(doc: &Document) -> Vec<Item> {
    let skip = data_instruction(doc).map(|(_, s, e)| (s, e));
    let mut out = Vec::new();
    for (i, p) in doc.main.paras.iter().enumerate() {
        let mut field: Option<(String, CharProps)> = None;
        for (off, c) in p.text.char_indices() {
            let here = Position::new(i, off);
            if skip.is_some_and(|(s, e)| here >= s && here < e) {
                continue;
            }
            let chp = p.chp_of_char(off);
            match (c, &mut field) {
                (OPEN, None) => field = Some((String::new(), chp)),
                (CLOSE, Some(_)) => {
                    let (f, chp) = field.take().unwrap();
                    out.push(Item::Field(f.trim().to_string(), chp));
                }
                (c, Some((f, _))) => f.push(c),
                (c, None) => out.push(Item::Text(c, chp)),
            }
        }
        // The «DATA» paragraph itself goes if nothing else is in it.
        let only_data = skip.is_some_and(|(s, e)| {
            s.para == i
                && e.para == i
                && p.text[..s.offset].trim().is_empty()
                && p.text[e.offset..].trim().is_empty()
        });
        if !only_data {
            out.push(Item::End(Box::new(p.clone())));
        }
    }
    out
}

/// Expand INCLUDE instructions before processing records. Included documents
/// retain their paragraphs and formatting, and may themselves contain merge
/// fields. The depth bound prevents a pair of documents from including one
/// another forever.
fn expand_includes(
    source: Vec<Item>,
    include: &mut dyn FnMut(&str) -> Result<Document, String>,
    depth: usize,
    errors: &mut Vec<MergeError>,
) -> Vec<Item> {
    let mut out = Vec::new();
    for item in source {
        let Item::Field(field, _) = &item else {
            out.push(item);
            continue;
        };
        let Some((word, rest)) = field.split_once(char::is_whitespace) else {
            out.push(item);
            continue;
        };
        if !word.eq_ignore_ascii_case("INCLUDE") {
            out.push(item);
            continue;
        }
        let name = rest.trim().trim_matches('"');
        if name.is_empty() {
            errors.push(MergeError {
                message: "INCLUDE has no document name".into(),
            });
        } else if depth >= MAX_INCLUDE_DEPTH {
            errors.push(MergeError {
                message: format!("Too many nested INCLUDE instructions at “{name}”"),
            });
        } else {
            match include(name) {
                Ok(doc) => out.extend(expand_includes(items(&doc), include, depth + 1, errors)),
                Err(message) => errors.push(MergeError { message }),
            }
        }
    }
    out
}

/// Evaluate an IF's test against record values and SET values.
fn test(cond: &str, value: &dyn Fn(&str) -> Option<String>) -> bool {
    for op in ["<>", "=", "<", ">"] {
        if let Some((name, want)) = cond.split_once(op) {
            let have = value(name.trim()).unwrap_or_default();
            let want = want.trim().trim_matches('"');
            let num = |s: &str| s.trim().parse::<f64>().ok();
            return match (op, num(&have), num(want)) {
                ("=", _, _) => have.eq_ignore_ascii_case(want),
                ("<>", _, _) => !have.eq_ignore_ascii_case(want),
                ("<", Some(a), Some(b)) => a < b,
                (">", Some(a), Some(b)) => a > b,
                ("<", _, _) => have.to_lowercase() < want.to_lowercase(),
                _ => have.to_lowercase() > want.to_lowercase(),
            };
        }
    }
    !value(cond.trim()).unwrap_or_default().is_empty()
}

/// Merge `main` with `data` for records `from..to` (all if None): one
/// copy of the main document per record, each in a section of its own.
/// Returns the merged document and any problems found.
pub fn merge(
    main: &Document,
    data: &Data,
    range: Option<(usize, usize)>,
) -> (Document, Vec<MergeError>) {
    merge_with_includes(main, data, range, &mut |name| {
        Err(format!(
            "The included document “{name}” could not be opened"
        ))
    })
}

/// Merge while resolving each INCLUDE instruction to a document. The caller
/// owns path resolution and file decoding so the document model remains
/// independent of storage formats.
pub fn merge_with_includes(
    main: &Document,
    data: &Data,
    range: Option<(usize, usize)>,
    include: &mut dyn FnMut(&str) -> Result<Document, String>,
) -> (Document, Vec<MergeError>) {
    merge_using(main, data, range, include, None).unwrap()
}

/// Merge with per-record responses for ASK instructions. If an active ASK
/// has no response yet, return it without producing a partial document. Add
/// the response to `answers` and call again; this keeps the UI asynchronous
/// while the merge evaluator itself remains deterministic.
pub fn merge_interactive(
    main: &Document,
    data: &Data,
    range: Option<(usize, usize)>,
    include: &mut dyn FnMut(&str) -> Result<Document, String>,
    answers: &HashMap<AskKey, String>,
) -> Result<(Document, Vec<MergeError>), AskRequest> {
    merge_using(main, data, range, include, Some(answers))
}

fn merge_using(
    main: &Document,
    data: &Data,
    range: Option<(usize, usize)>,
    include: &mut dyn FnMut(&str) -> Result<Document, String>,
    answers: Option<&HashMap<AskKey, String>>,
) -> Result<(Document, Vec<MergeError>), AskRequest> {
    let mut errors = Vec::new();
    let items = expand_includes(items(main), include, 0, &mut errors);
    let (first, last) = range.unwrap_or((0, data.records.len().saturating_sub(1)));
    let last = last.min(data.records.len().saturating_sub(1));
    let mut out: Vec<Paragraph> = Vec::new();
    let mut sets: HashMap<String, String> = HashMap::new();
    let mut known: Vec<String> = data.fields.clone();
    let mut record = first;
    let blank = |pap: &ParaProps, chp: CharProps| Paragraph::new(pap.clone(), chp);
    while record <= last && !data.records.is_empty() {
        let out_start = out.len();
        let mut skip = false;
        let mut para: Option<Paragraph> = None;
        // Branches: whether each open IF is taking its text.
        let mut stack: Vec<bool> = Vec::new();
        for it in &items {
            let active = stack.iter().all(|&b| b);
            match it {
                Item::Text(c, chp) if active => {
                    let p = para.get_or_insert_with(|| blank(&ParaProps::default(), *chp));
                    let at = p.len();
                    p.insert(at, &c.to_string(), *chp);
                }
                Item::End(src) if active => {
                    let mut p = para.take().unwrap_or_else(|| blank(&src.pap, src.mark_chp));
                    p.pap = src.pap.clone();
                    p.mark = if src.mark == Mark::SectionBreak {
                        Mark::Paragraph
                    } else {
                        src.mark
                    };
                    p.mark_chp = src.mark_chp;
                    p.row = src.row.clone();
                    out.push(p);
                }
                Item::Field(f, chp) => {
                    let upper = f.to_uppercase();
                    let word = upper.split_whitespace().next().unwrap_or("");
                    let rest = f[word.len().min(f.len())..].trim();
                    let value = |name: &str| {
                        sets.get(&name.to_lowercase())
                            .cloned()
                            .or_else(|| data.value(record, name))
                    };
                    match word {
                        // Each IF's own test; text is taken while all hold.
                        "IF" => stack.push(test(rest, &value)),
                        "ELSE" => {
                            if let Some(top) = stack.last_mut() {
                                *top = !*top;
                            } else {
                                errors.push(MergeError {
                                    message: "ELSE without IF".into(),
                                });
                            }
                        }
                        "ENDIF" => {
                            if stack.pop().is_none() {
                                errors.push(MergeError {
                                    message: "ENDIF without IF".into(),
                                });
                            }
                        }
                        "NEXT" if active => {
                            if record < last {
                                record += 1;
                            }
                        }
                        "NEXTIF" if active => {
                            if test(rest, &value) && record < last {
                                record += 1;
                            }
                        }
                        "SKIPIF" if active => {
                            if test(rest, &value) {
                                skip = true;
                                break;
                            }
                        }
                        "SET" if active => {
                            if let Some((name, v)) = rest.split_once('=') {
                                let name = name.trim().to_lowercase();
                                known.push(name.clone());
                                sets.insert(name, v.trim().trim_matches('"').to_string());
                            }
                        }
                        "ASK" if active => {
                            if let Some((name, prompt)) = rest.split_once('=') {
                                let name = name.trim().to_lowercase();
                                known.push(name.clone());
                                let raw = prompt.trim().trim_matches('"');
                                let prompt = raw.strip_prefix('?').unwrap_or(raw).trim();
                                let answer = if let Some(answers) = answers {
                                    let key = AskKey {
                                        record,
                                        name: name.clone(),
                                    };
                                    match answers.get(&key) {
                                        Some(answer) => answer.clone(),
                                        None => {
                                            let default = answers
                                                .iter()
                                                .filter(|(other, _)| {
                                                    other.name == name && other.record < record
                                                })
                                                .max_by_key(|(other, _)| other.record)
                                                .map_or_else(String::new, |(_, answer)| {
                                                    answer.clone()
                                                });
                                            return Err(AskRequest {
                                                key,
                                                prompt: if prompt.is_empty() {
                                                    format!("Enter text for “{name}”:")
                                                } else {
                                                    prompt.to_string()
                                                },
                                                default,
                                            });
                                        }
                                    }
                                } else {
                                    raw.to_string()
                                };
                                sets.insert(name, answer);
                            }
                        }
                        _ if active => {
                            let v = value(f);
                            if v.is_none() && !known.iter().any(|k| k.eq_ignore_ascii_case(f)) {
                                errors.push(MergeError {
                                    message: format!(
                                        "\u{201C}{f}\u{201D} is not a field in the data document"
                                    ),
                                });
                            }
                            let p = para.get_or_insert_with(|| blank(&ParaProps::default(), *chp));
                            let at = p.len();
                            p.insert(at, &v.unwrap_or_default(), *chp);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !stack.is_empty() {
            errors.push(MergeError {
                message: "IF without ENDIF".into(),
            });
        }
        if let Some(p) = para.take() {
            out.push(p);
        }
        if skip {
            out.truncate(out_start);
            record += 1;
            continue;
        }
        // Each copy ends its own section, starting the next on a new page.
        if record < last
            && let Some(p) = out.last_mut()
        {
            p.mark = Mark::SectionBreak;
            p.section = Some(Box::new(crate::document::SectionProps {
                start: crate::document::SectionStart::NewPage,
                ..main.section.clone()
            }));
        }
        record += 1;
    }
    errors.dedup();
    let mut doc = main.clone();
    if out.is_empty() {
        out.push(main.main.paras[0].clone());
    }
    doc.main = Story { paras: out };
    doc.section.start = crate::document::SectionStart::NewPage;
    doc.forget_history();
    doc.touch();
    Ok((doc, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        crate::text::from_plain_text(text)
    }

    #[test]
    fn data_from_tabs_commas_and_quotes() {
        let d = Data::from_document(&doc("Name\tCity\nAda\tLondon\n\"Smith, J\",x")).unwrap();
        assert_eq!(d.fields, ["Name", "City"]);
        assert_eq!(d.records[0], ["Ada", "London"]);
        assert_eq!(d.records[1], ["Smith, J", "x"]);
    }

    #[test]
    fn data_limits_fields_in_headers_and_later_records() {
        for delimiter in [",", "\t"] {
            let accepted = format!("a{}", delimiter.repeat(MAX_DATA_FIELDS_PER_RECORD - 1));
            assert_eq!(
                Data::from_document(&doc(&accepted)).unwrap().fields.len(),
                MAX_DATA_FIELDS_PER_RECORD
            );
            let excessive = format!("{accepted}{delimiter}");
            assert!(
                Data::from_document(&doc(&excessive))
                    .unwrap_err()
                    .contains("fields in one record")
            );
            assert!(Data::from_document(&doc(&format!("Name\n{excessive}"))).is_err());
        }
        assert!(Data::from_document(&doc(&",".repeat(8192))).is_err());
        let empty_columns = Data::from_document(&doc("Name\tCity\t\nAda\tLondon\t\n\t\t")).unwrap();
        assert_eq!(empty_columns.fields, ["Name", "City", ""]);
        assert_eq!(
            empty_columns.records,
            [vec!["Ada", "London", ""], vec!["", "", ""]]
        );
        let quoted = format!("\"{}\",\"A\"\"B\"", ",".repeat(8192));
        let data = Data::from_document(&doc(&quoted)).unwrap();
        assert_eq!(data.fields, [",".repeat(8192), "A\"B".into()]);
    }

    #[test]
    fn data_budgets_are_cumulative_for_text_and_tables() {
        let text = doc("Name,City\nAda,London");
        let read = |doc: &Document, cells, bytes| {
            Data::from_document_with_budget(doc, DataBudget { cells, bytes })
        };
        assert!(read(&text, 4, 19).is_ok());
        assert!(read(&text, 3, 19).unwrap_err().contains("cells"));
        assert!(read(&text, 4, 18).unwrap_err().contains("text"));

        let mut table = doc("");
        table.insert_table(crate::StoryId::Main, Position::new(0, 0), 2, 2, 1440);
        let mut cell_number = 0;
        for p in &mut table.main.paras {
            if p.mark == Mark::CellEnd {
                p.text = format!("c{cell_number}");
                cell_number += 1;
            }
        }
        assert_eq!(cell_number, 4);
        let data = read(&table, 4, 8).unwrap();
        assert_eq!(data.fields, ["c0", "c1"]);
        assert_eq!(data.records, [vec!["c2", "c3"]]);
        assert!(read(&table, 3, 8).unwrap_err().contains("cells"));
        assert!(read(&table, 4, 7).unwrap_err().contains("text"));
        assert!(Data::from_document(&doc("")).unwrap().fields.is_empty());
    }

    #[test]
    fn merges_fields_ifs_and_records() {
        let main = doc(
            "«DATA people»\nDear «Name»,\n«IF City=\"London\"»See you soon.«ELSE»Write soon.«ENDIF»",
        );
        assert_eq!(data_instruction(&main).unwrap().0, "people");
        let data = Data::from_document(&doc("Name,City\nAda,London\nBob,Paris")).unwrap();
        let (out, errors) = merge(&main, &data, None);
        assert!(errors.is_empty(), "{errors:?}");
        let texts: Vec<&str> = out.main.paras.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Dear Ada,", "See you soon.", "Dear Bob,", "Write soon."]
        );
        assert_eq!(out.main.paras[1].mark, Mark::SectionBreak);
        let (_, errors) = merge(&doc("«DATA x»\n«Nmae»"), &data, Some((0, 0)));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn nextif_advances_and_skipif_omits_matching_records() {
        let data = Data::from_document(&doc("Name,City\nAda,London\nBob,Paris\nCy,Rome")).unwrap();
        let (out, errors) = merge(
            &doc("«DATA people»\n«SKIPIF City=\"Paris\"»«Name»"),
            &data,
            None,
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            out.main
                .paras
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>(),
            ["Ada", "Cy"]
        );

        let (out, errors) = merge(
            &doc("«DATA people»\n«Name» «NEXTIF City=\"London\"»«Name»"),
            &data,
            Some((0, 1)),
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(out.main.paras[0].text, "Ada Bob");
        assert_eq!(out.main.paras.len(), 1);
    }

    #[test]
    fn include_inserts_and_merges_another_document() {
        let main = doc("«DATA people»\nBefore\n«INCLUDE \"part.qwp\"»After");
        let data = Data::from_document(&doc("Name\nAda")).unwrap();
        let mut names = Vec::new();
        let (out, errors) = merge_with_includes(&main, &data, None, &mut |name| {
            names.push(name.to_string());
            Ok(doc("Included «Name»"))
        });
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(names, ["part.qwp"]);
        assert_eq!(
            out.main
                .paras
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>(),
            ["Before", "Included Ada", "After"]
        );
    }

    #[test]
    fn ask_requests_and_reuses_a_response_for_each_record() {
        let main = doc("«DATA people»\n«ASK greeting=?Greeting for this person?»«greeting» «Name»");
        let data = Data::from_document(&doc("Name\nAda\nBob")).unwrap();
        let mut answers = HashMap::new();
        let mut no_includes = |name: &str| Err(format!("unexpected include {name}"));

        let first = merge_interactive(&main, &data, None, &mut no_includes, &answers).unwrap_err();
        assert_eq!(first.key.record, 0);
        assert_eq!(first.key.name, "greeting");
        assert_eq!(first.prompt, "Greeting for this person?");
        assert!(first.default.is_empty());
        answers.insert(first.key, "Hello".into());

        let second = merge_interactive(&main, &data, None, &mut no_includes, &answers).unwrap_err();
        assert_eq!(second.key.record, 1);
        assert_eq!(second.default, "Hello");
        answers.insert(second.key, "Welcome".into());

        let (out, errors) =
            merge_interactive(&main, &data, None, &mut no_includes, &answers).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(out.main.paras[0].text, "Hello Ada");
        assert_eq!(out.main.paras[1].text, "Welcome Bob");
    }
}
