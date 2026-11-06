//! A small element tree built with quick-xml. Names lose their namespace
//! prefixes (`w:p` becomes `p`), which is all the .docx reader needs.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

// Room for the largest document Quill holds (quill_core::limits), with
// heavy formatting and text that is mostly escapes.
const MAX_XML_BYTES: usize = 512 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 16_000_000;
// Bound per-element bookkeeping (including duplicate-name checks), and
// charge raw attribute bytes before decoding or copying their strings.
const MAX_ELEMENT_ATTRIBUTES: usize = 256;
const MAX_ELEMENT_ATTRIBUTE_BYTES: usize = 1024 * 1024;
const MAX_ATTRIBUTE_BYTES: usize = 128 * 1024 * 1024;

fn add_nodes(nodes: &mut usize, count: usize, max: usize) -> Result<(), String> {
    *nodes = nodes
        .checked_add(count)
        .filter(|&n| n <= max)
        .ok_or_else(|| "XML document has too many nodes".to_string())?;
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug)]
pub enum Node {
    Element(Element),
    Text(String),
}

fn local(s: &str) -> String {
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

// unescape_value is deprecated in favour of an XML-version-aware call; it
// does exactly what .docx needs.
#[allow(deprecated)]
fn start(
    e: &BytesStart,
    nodes: &mut usize,
    max_nodes: usize,
    attribute_bytes: &mut usize,
) -> Result<Element, String> {
    let bytes = e.attributes_raw().len();
    if bytes > MAX_ELEMENT_ATTRIBUTE_BYTES {
        return Err("XML element has too many attribute bytes".into());
    }
    *attribute_bytes = attribute_bytes
        .checked_add(bytes)
        .filter(|&n| n <= MAX_ATTRIBUTE_BYTES)
        .ok_or_else(|| "XML document has too many attribute bytes".to_string())?;
    add_nodes(nodes, 1, max_nodes)?;
    let mut attrs = Vec::new();
    for (index, attr) in e.attributes().enumerate() {
        if index >= MAX_ELEMENT_ATTRIBUTES {
            return Err("XML element has too many attributes".into());
        }
        add_nodes(nodes, 1, max_nodes)?;
        let attr = attr.map_err(|e| format!("XML attribute error: {e}"))?;
        let value = attr
            .unescape_value()
            .map_err(|e| format!("XML attribute value error: {e}"))?
            .into_owned();
        attrs.push((local(attr.key.as_ref()), value));
    }
    Ok(Element {
        name: local(e.name().as_ref()),
        attrs,
        children: Vec::new(),
    })
}

/// Parse a whole document into a tree. Returns the root element.
pub fn parse(data: &[u8]) -> Result<Element, String> {
    parse_with_limits(data, MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_NODES)
}

fn parse_with_limits(
    data: &[u8],
    max_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
) -> Result<Element, String> {
    if data.len() > max_bytes {
        return Err("XML part is too large".into());
    }
    let mut reader = Reader::from_reader(data);
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut buf = Vec::new();
    let mut nodes = 0usize;
    let mut attribute_bytes = 0usize;
    // Text, character references and CDATA next to each other are one
    // text node, counted once: "a &lt; b" is one piece of text, not three.
    let add_text = |nodes: &mut usize, el: &mut Element, text: &str| -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if let Some(Node::Text(last)) = el.children.last_mut() {
            last.push_str(text);
            return Ok(());
        }
        add_nodes(nodes, 1, max_nodes)?;
        el.children.push(Node::Text(text.to_string()));
        Ok(())
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if stack.len() > max_depth {
                    return Err("XML document is nested too deeply".into());
                }
                let el = start(&e, &mut nodes, max_nodes, &mut attribute_bytes)?;
                stack.push(el);
            }
            Ok(Event::Empty(e)) => {
                if stack.len() > max_depth {
                    return Err("XML document is nested too deeply".into());
                }
                let el = start(&e, &mut nodes, max_nodes, &mut attribute_bytes)?;
                stack.last_mut().unwrap().children.push(Node::Element(el));
            }
            Ok(Event::End(_)) => {
                if stack.len() == 1 {
                    return Err("XML document has an unexpected closing tag".into());
                }
                let el = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(Node::Element(el));
            }
            Ok(Event::Text(t)) => {
                let raw: &str = &t;
                let text = quick_xml::escape::unescape(raw)
                    .map(|s| s.into_owned())
                    .map_err(|e| format!("XML text error: {e}"))?;
                add_text(&mut nodes, stack.last_mut().unwrap(), &text)?;
            }
            Ok(Event::GeneralRef(r)) => {
                // Entity and character references inside text.
                let name: String = r.to_string();
                let ch = match name.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16)
                        .ok()
                        .and_then(char::from_u32),
                    n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
                    _ => None,
                };
                let c =
                    ch.ok_or_else(|| format!("XML has an unknown character reference: &{name};"))?;
                add_text(
                    &mut nodes,
                    stack.last_mut().unwrap(),
                    c.encode_utf8(&mut [0; 4]),
                )?;
            }
            Ok(Event::CData(t)) => {
                let text: String = t.to_string();
                add_text(&mut nodes, stack.last_mut().unwrap(), &text)?;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("XML error at {}: {e}", reader.buffer_position())),
        }
        buf.clear();
    }
    if stack.len() != 1 {
        return Err("XML document ends before its elements are closed".into());
    }
    let root = stack.pop().unwrap();
    let mut document = None;
    for child in root.children {
        match child {
            Node::Element(e) if document.is_none() => document = Some(e),
            Node::Element(_) => return Err("XML document has more than one root element".into()),
            Node::Text(t) if !t.trim().is_empty() => {
                return Err("XML document has text outside its root element".into());
            }
            Node::Text(_) => {}
        }
    }
    document.ok_or_else(|| "empty XML document".to_string())
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// All text inside this element, concatenated.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in &self.children {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }

    /// An integer attribute.
    pub fn int(&self, name: &str) -> Option<i32> {
        self.attr(name)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|f| f.round() as i32)
    }

    /// A WordprocessingML on/off property: present means on unless its
    /// `val` says otherwise.
    pub fn on(&self) -> bool {
        !matches!(self.attr("val"), Some("0" | "false" | "off" | "none"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaced_document() {
        let xml = br#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body><w:p><w:r><w:t xml:space="preserve">a &amp; b</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.name, "document");
        let body = root.child("body").unwrap();
        assert_eq!(body.elements().count(), 2);
        let t = body
            .child("p")
            .unwrap()
            .child("r")
            .unwrap()
            .child("t")
            .unwrap();
        assert_eq!(t.text(), "a & b");
        assert_eq!(t.attr("space"), Some("preserve"));
    }

    #[test]
    fn on_off() {
        let root = parse(br#"<r><b/><i val="0"/></r>"#).unwrap();
        assert!(root.child("b").unwrap().on());
        assert!(!root.child("i").unwrap().on());
    }

    #[test]
    fn rejects_truncated_and_multiple_root_documents() {
        assert!(parse(b"<root><child>").is_err());
        assert!(parse(b"<one/><two/>").is_err());
        assert!(parse(b"text<root/>").is_err());
    }

    #[test]
    fn references_in_text_are_part_of_the_text_not_nodes_of_their_own() {
        // Three hundred escapes, under a limit of ten nodes.
        let xml = format!("<a>{}</a>", "x &lt; y &amp; ".repeat(150));
        let root = parse_with_limits(xml.as_bytes(), 1 << 20, 10, 10).unwrap();
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.text(), "x < y & ".repeat(150));
    }

    #[test]
    fn rejects_excessive_depth_nodes_and_bytes() {
        assert!(parse_with_limits(b"<a><b><c/></b></a>", 100, 2, 100).is_err());
        assert!(parse_with_limits(b"<a><b/><c/></a>", 100, 10, 2).is_err());
        assert!(parse_with_limits(b"<root/>", 6, 10, 10).is_err());
    }

    #[test]
    fn rejects_attributes_before_decoding_when_node_budget_is_exhausted() {
        for xml in [
            br#"<root a="ok" b="&bad;"/>"#.as_slice(),
            br#"<root a="ok" b="&bad;"></root>"#.as_slice(),
        ] {
            assert_eq!(
                parse_with_limits(xml, 100, 10, 2).unwrap_err(),
                "XML document has too many nodes"
            );
        }
        assert!(parse_with_limits(b"<r><a x='1'/><b y='2'/></r>", 100, 10, 5).is_ok());
        assert!(parse_with_limits(b"<r><a x='1'/><b y='2'/></r>", 100, 10, 4).is_err());
    }

    #[test]
    fn bounds_attribute_count_and_bytes_before_copying() {
        let attrs: String = (0..MAX_ELEMENT_ATTRIBUTES)
            .map(|i| format!(" a{i}='v'"))
            .collect();
        assert!(parse(format!("<r{attrs}/>").as_bytes()).is_ok());
        let too_many = format!("<r{attrs} extra='&bad;'/>");
        assert_eq!(
            parse(too_many.as_bytes()).unwrap_err(),
            "XML element has too many attributes"
        );
        let oversized = format!("<r a='{}'/>", "x".repeat(MAX_ELEMENT_ATTRIBUTE_BYTES));
        assert_eq!(
            parse(oversized.as_bytes()).unwrap_err(),
            "XML element has too many attribute bytes"
        );
        let e = BytesStart::from_content("r a='&bad;'", 1);
        let mut total = MAX_ATTRIBUTE_BYTES - e.attributes_raw().len();
        let mut nodes = 0;
        // The last bytes fit, so decoding still reports the malformed value.
        assert!(
            start(&e, &mut nodes, 10, &mut total)
                .unwrap_err()
                .contains("value error")
        );
        assert_eq!(total, MAX_ATTRIBUTE_BYTES);
        assert_eq!(
            start(&e, &mut nodes, 10, &mut total).unwrap_err(),
            "XML document has too many attribute bytes"
        );
    }

    #[test]
    fn rejects_bad_attributes_and_character_references() {
        assert!(parse(br#"<root value="&noSuchEntity;"/>"#).is_err());
        assert!(parse(br#"<root value="unterminated/>"#).is_err());
        assert!(parse(b"<root>&noSuchEntity;</root>").is_err());
    }
}
