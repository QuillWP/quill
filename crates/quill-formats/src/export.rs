//! Shared, deterministic helpers for document exports.

use quill_core::para::Paragraph;
use quill_core::table::Row;

pub fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c < ' ' && !matches!(c, '\n' | '\t' | '\r') => {}
            '\u{fffe}' | '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn points(twips: i32) -> String {
    // Keep twips exact without locale-dependent formatting.
    let mut s = format!("{:.2}", twips as f64 / 20.);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

pub enum Block<'a> {
    Paragraph(&'a Paragraph),
    Table(Vec<Row>),
}

pub fn blocks(paras: &[Paragraph]) -> Vec<Block<'_>> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < paras.len() {
        if let Some(row) = quill_core::table::row_at(paras, i) {
            let mut rows = vec![row];
            i = rows.last().unwrap().end + 1;
            while let Some(row) = quill_core::table::row_at(paras, i) {
                i = row.end + 1;
                rows.push(row);
            }
            result.push(Block::Table(rows));
        } else {
            result.push(Block::Paragraph(&paras[i]));
            i += 1;
        }
    }
    result
}

pub fn base64(data: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for b in data.chunks(3) {
        let n = ((b[0] as u32) << 16)
            | ((b.get(1).copied().unwrap_or(0) as u32) << 8)
            | b.get(2).copied().unwrap_or(0) as u32;
        for shift in [18, 12] {
            out.push(ABC[((n >> shift) & 63) as usize] as char);
        }
        out.push(if b.len() > 1 {
            ABC[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if b.len() > 2 {
            ABC[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
