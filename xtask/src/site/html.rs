//! Use a document parser here: even with its checks enabled, quick-xml 0.42's event reader accepts
//! illegal element names and multiple root elements. The site's XML grammar forbids both.

use super::File;
use super::links::Origins;
use roxmltree::{Document, Node, ParsingOptions};

fn at(node: Node<'_, '_>, message: &str) -> String {
    let line = node.document().text_pos_at(node.range().start).row;
    format!("line {line}: {message}")
}

fn line_after(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count() + 1
}

pub(super) fn utf8(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|e| format!("line {}: XML: expected UTF-8: {e}", line_after(&bytes[..e.valid_up_to()])))
}

fn document(source: &str) -> Result<Document<'_>, String> {
    if source.len() as u64 > super::PAGE_BUDGET {
        return Err(format!("HTML alone exceeds the {}-byte budget", super::PAGE_BUDGET));
    }
    let start = source.trim_start_matches([' ', '\t', '\r', '\n']);
    if !start.starts_with("<!DOCTYPE html>") {
        let line = line_after(&source.as_bytes()[..source.len() - start.len()]);
        return Err(format!("line {line}: XML: expected <!DOCTYPE html>"));
    }
    // Only the literal, empty HTML5 doctype above is possible. The XML grammar rejects a second doctype;
    // the default entity resolver performs no I/O.
    let options = ParsingOptions { allow_dtd: true, ..ParsingOptions::default() };
    Document::parse_with_options(source, options).map_err(|e| {
        // These errors have no stored position in roxmltree: the parser discovered the missing element at EOF.
        let line = match e {
            roxmltree::Error::UnclosedRootNode | roxmltree::Error::NoRootNode | roxmltree::Error::UnexpectedEndOfStream => {
                line_after(source.as_bytes())
            }
            _ => e.pos().row as usize,
        };
        format!("line {line}: XML: {e}")
    })
}

/// roxmltree replaces non-scalar numeric references with U+FFFD. Reject them where XML expands references,
/// while preserving their literal spelling in comments, processing instructions and CDATA sections.
fn check_numeric_references(doc: &Document<'_>) -> Result<bool, String> {
    let source = doc.input_text();
    if !source.contains("&#") {
        return Ok(false);
    }
    let mut rest = source;
    while let Some(start) = rest.find(['<', '&']) {
        rest = &rest[start..];
        let delimiter = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")].into_iter().find(|(open, _)| rest.starts_with(open));
        if let Some((_, close)) = delimiter {
            // A well-formed document has already been parsed, so these delimiters are paired.
            rest = rest.split_once(close).ok_or("unterminated XML literal")?.1;
        } else if let Some(reference) = rest.strip_prefix("&#") {
            let (digits, tail) = reference.split_once(';').ok_or("unterminated XML reference")?;
            let code = if let Some(hex) = digits.strip_prefix('x') { u32::from_str_radix(hex, 16) } else { digits.parse() };
            if code.ok().and_then(char::from_u32).is_none() {
                let line = doc.text_pos_at(source.len() - rest.len()).row;
                return Err(format!("line {line}: XML: invalid character reference &#{digits};"));
            }
            rest = tail;
        } else {
            rest = &rest[1..];
        }
    }
    Ok(true)
}

/// Missing children point to the enclosing element. Duplicates point to the extra element.
fn required<'a, 's>(parent: Node<'a, 's>, tag: &str, attr: Option<(&str, &str)>) -> Result<Node<'a, 's>, String> {
    let mut found = parent.children().filter(|n| n.has_tag_name(tag) && attr.is_none_or(|(name, value)| n.attribute(name) == Some(value)));
    let label = attr.map_or(tag, |(_, value)| value);
    let first = found.next().ok_or_else(|| at(parent, &format!("missing {label}")))?;
    if let Some(extra) = found.next() {
        return Err(at(extra, &format!("duplicate {label}")));
    }
    Ok(first)
}

/// What the head of a page says of the page, as the parser reads it (character references resolved): the values the generated social cards and structured data are made from (`seo.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Head {
    pub(super) title: String,
    pub(super) description: String,
    pub(super) og_title: String,
    pub(super) og_description: String,
    pub(super) og_image: String,
    pub(super) og_image_alt: Option<String>,
}

/// The [`Head`] of the page `source`. A page that has no title, description or `og:title`, `og:description` or `og:image` is an error, as is a duplicate of any of them.
pub(super) fn head(source: &str) -> Result<Head, String> {
    let doc = document(source)?;
    let head = required(doc.root_element(), "head", None)?;
    let meta = |key: &str, value: &str| -> Result<String, String> {
        let node = required(head, "meta", Some((key, value)))?;
        node.attribute("content").map(str::to_owned).ok_or_else(|| at(node, &format!("missing content for {value}")))
    };
    let title = required(head, "title", None)?.children().filter_map(|n| n.text().filter(|_| n.is_text())).collect::<String>();
    let alt = head
        .children()
        .filter(|n| n.has_tag_name("meta") && n.attribute("property") == Some("og:image:alt"))
        .map(|n| n.attribute("content").unwrap_or("").to_owned())
        .next();
    Ok(Head {
        title: title.trim().to_owned(),
        description: meta("name", "description")?,
        og_title: meta("property", "og:title")?,
        og_description: meta("property", "og:description")?,
        og_image: meta("property", "og:image")?,
        og_image_alt: alt,
    })
}

/// The `type` of a script that holds data and no program: what a browser does not run, and what a content security policy does not see.
pub(super) const JSON_LD: &str = "application/ld+json";

/// The structured data of a page: JSON that is an object with the schema.org context and a type (`seo.rs` writes it; a browser and a search engine read it as text, so it must parse).
fn json_ld(script: Node<'_, '_>) -> Result<(), String> {
    if script.has_attribute("src") {
        return Err(at(script, "a JSON-LD script has its data inline and no src"));
    }
    let text: String = script.descendants().filter_map(|n| n.text().filter(|_| n.is_text())).collect();
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| at(script, &format!("JSON-LD that is not JSON: {e}")))?;
    let object = value.as_object().ok_or_else(|| at(script, "JSON-LD must be one object"))?;
    if object.get("@context").and_then(serde_json::Value::as_str) != Some("https://schema.org") {
        return Err(at(script, "JSON-LD must have the context https://schema.org"));
    }
    if !object.get("@type").and_then(serde_json::Value::as_str).is_some_and(|t| !t.is_empty()) {
        return Err(at(script, "JSON-LD must have a @type"));
    }
    Ok(())
}

fn structure(doc: &Document<'_>, canonical: Option<&str>) -> Result<(), String> {
    let root = doc.root_element();
    if !root.has_tag_name("html") {
        return Err(at(root, "expected html as the root element"));
    }
    let head = required(root, "head", None)?;
    let body = required(root, "body", None)?;
    for child in root.children() {
        if child.is_element() && child != head && child != body || child.is_text() && !child.text().unwrap_or("").trim().is_empty() {
            return Err(at(child, "only head and body belong directly under html"));
        }
    }
    if head.next_sibling_element() != Some(body) {
        return Err(at(body, "body must follow head"));
    }
    let title = required(head, "title", None)?;
    if !title.children().any(|n| n.is_text() && n.text().is_some_and(|s| !s.trim().is_empty())) {
        return Err(at(title, "empty title"));
    }
    let description = required(head, "meta", Some(("name", "description")))?;
    if description.attribute("content").is_none_or(|s| s.trim().is_empty()) {
        return Err(at(description, "empty description"));
    }
    for (tag, selector, field) in
        [("link", ("rel", "canonical"), "href"), ("meta", ("property", "og:url"), "content"), ("meta", ("property", "og:image"), "content")]
    {
        let node = required(head, tag, Some(selector))?;
        let value = node.attribute(field).ok_or_else(|| at(node, &format!("missing {field} for {}", selector.1)))?;
        if selector.1 != "og:image"
            && let Some(expected) = canonical
            && value != expected
        {
            return Err(at(node, &format!("expected {} to be {expected}; run cargo xtask site --write to refresh it", selector.1)));
        }
    }
    for element in root.descendants().filter(Node::is_element) {
        match element.tag_name().name() {
            "html" | "head" | "body" if element != root && element != head && element != body => {
                return Err(at(element, "misplaced html, head or body"));
            }
            "base" | "svg" => return Err(at(element, "unsupported base or svg element")),
            _ if element.has_attribute("srcdoc") => return Err(at(element, "unsupported embedded document")),
            "script" if element.attribute("type") == Some(JSON_LD) => json_ld(element)?,
            "img" => validate_img(element)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_img(img: Node<'_, '_>) -> Result<(), String> {
    let positive = |s: &str| s.bytes().all(|b| b.is_ascii_digit()) && s.bytes().any(|b| b != b'0');
    if !img.has_attribute("alt")
        || !img.has_attribute("src") && !img.has_attribute("srcset")
        || !img.attribute("width").is_some_and(positive)
        || !img.attribute("height").is_some_and(positive)
    {
        return Err(at(img, "img requires alt, src or srcset, and positive integer width and height"));
    }
    Ok(())
}

/// Check the source before replacing settings-dependent values in it.
pub(super) fn before_write(source: &str) -> Result<(), String> {
    let doc = document(source)?;
    check_numeric_references(&doc)?;
    structure(&doc, None)
}

pub(super) fn parse(name: &str, source: &str, origins: &Origins, file: &mut File) -> Result<(), String> {
    let route = name.strip_suffix("index.html").filter(|parent| parent.is_empty() || parent.ends_with('/')).or_else(|| name.strip_suffix(".html"));
    let canonical = format!("{}/{}", origins.domain, route.ok_or("invalid HTML filename")?);
    let doc = document(source)?;
    let numeric_refs = check_numeric_references(&doc)?;
    let allowed = super::allowed(name);
    structure(&doc, Some(&canonical))?;
    for node in doc.descendants() {
        if node.is_element() {
            collect(node, file).map_err(|e| at(node, &e))?;
        }
        if numeric_refs {
            let text = node.text().filter(|_| node.is_text()).into_iter().chain(node.attributes().map(|a| a.value()));
            for value in text {
                super::brands(value, &allowed).and_then(|()| super::links::hosts(value, origins, name)).map_err(|e| at(node, &e))?;
            }
        }
    }
    Ok(())
}

fn collect(node: Node<'_, '_>, file: &mut File) -> Result<(), String> {
    let tag = node.tag_name().name();
    for attr in node.attributes().filter(|a| a.namespace().is_none()) {
        let value = attr.value();
        match (tag, attr.name()) {
            (_, "id") if value.is_empty() || !file.ids.insert(value.to_owned()) => return Err(format!("empty or duplicate id {value:?}")),
            ("a" | "area", "href") | (_, "action" | "formaction" | "cite") => file.links.push(value.to_owned()),
            (_, "src" | "poster") | ("object", "data") => file.assets.push(value.to_owned()),
            ("link", "href") if node.attribute("rel") != Some("canonical") => file.assets.push(value.to_owned()),
            ("meta", "content") if node.attribute("property") == Some("og:image") || node.attribute("name") == Some("twitter:image") => {
                file.assets.push(value.to_owned())
            }
            (_, "srcset" | "imagesrcset") => {
                for candidate in value.split(',') {
                    let url = candidate.split_ascii_whitespace().next().ok_or("empty srcset candidate")?;
                    file.assets.push(url.to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(())
}
