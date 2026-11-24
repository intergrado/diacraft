//! A re-emitter for the preservation tests: the model back to the XML tree it was read from, as far as the model holds it.
//!
//! This is **not the M3 writer** and nothing in the crate uses it. M3's writer patches the original tree and keeps what the model does not hold
//! This builds a tree from the model alone, in a fixed order, to ask one question of the reader: does
//! everything the file said come back as the same text **in the same place**? `raw` wins over a typed field wherever it holds the attribute
//! (`crates/vsdx/src/passthrough.rs`), as a writer must do.
//!
//! **Position.** An element's unknown children are kept with `Placed::after`, the number of typed children before them in the file. The
//! emitter puts each one back where that says: after that many typed children, in the order the lists hold them (a sheet's own list first:
//! its triggers, then the element's). The typed children come in the model's order (cells by name, sections, then text, data, foreign data,
//! `Shapes`), which is the order a writer that holds nothing else could give, so fixtures keep their known children in it. [`signature`] then
//! compares the sequence exactly: a comment, an instruction or an unknown element that came back one place off is a different signature.
//! (Until the first review this put every unknown node after the typed ones, and sorted the cells, so five of six mutants of `after` survived.)

use diacraft_doc::{Cell, Document, ForeignData, Master, Page, Passthrough, Row, Section, Shape, ShapeList, Sheet, Style, Text, TextPart};
use diacraft_opc::xml::{El, Node};
use diacraft_vsdx::passthrough::node_to_tree;

pub fn el(name: &str, attrs: Vec<(String, String)>, children: Vec<Node>) -> El {
    El { name: name.to_owned(), attrs, children }
}

/// The typed children with the nodes of each passthrough put back after as many of them as `after` says. The lists are merged by `after`,
/// the earlier list first at an equal one.
fn assemble(typed: Vec<Node>, passes: &[&Passthrough]) -> Vec<Node> {
    let mut pending: Vec<(u32, usize, Node)> = Vec::new();
    for (list, pass) in passes.iter().enumerate() {
        for placed in pass.iter() {
            pending.push((placed.after, list, node_to_tree(&placed.node)));
        }
    }
    pending.sort_by_key(|(after, list, _)| (*after, *list));
    let mut out = Vec::with_capacity(typed.len() + pending.len());
    let mut pending = pending.into_iter().peekable();
    for (at, node) in typed.into_iter().enumerate() {
        while pending.peek().is_some_and(|(after, ..)| (*after as usize) <= at) {
            if let Some((_, _, n)) = pending.next() {
                out.push(n);
            }
        }
        out.push(node);
    }
    out.extend(pending.map(|(_, _, n)| n));
    out
}

/// `typed` unless `raw` holds the attribute, which wins.
fn typed_attr(attrs: &mut Vec<(String, String)>, raw: &diacraft_doc::RawAttrs, name: &str, value: Option<String>) {
    if raw.get(name).is_none()
        && let Some(v) = value
    {
        attrs.push((name.to_owned(), v));
    }
}

fn raw_attrs(attrs: &mut Vec<(String, String)>, raw: &diacraft_doc::RawAttrs) {
    attrs.extend(raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())));
}

pub fn cell(name: &str, c: &Cell) -> El {
    let mut attrs = vec![("N".to_owned(), name.to_owned())];
    if let Some(f) = c.formula.text() {
        attrs.push(("F".into(), f.to_owned()));
    }
    if let Some(v) = &c.value {
        attrs.push(("V".into(), v.text().to_owned()));
    }
    if let Some(u) = &c.unit {
        attrs.push(("U".into(), u.to_string()));
    }
    if let Some(e) = &c.error {
        attrs.push(("E".into(), e.to_string()));
    }
    raw_attrs(&mut attrs, &c.raw);
    let mut children = Vec::new();
    if let Some(extra) = &c.extra {
        if let Some(text) = &extra.text {
            children.push(Node::Text(text.to_string()));
        }
        // A cell's mixed content is all in its passthrough, in order (`after` is 0 for each).
        children.extend(extra.passthrough.iter().map(|p| node_to_tree(&p.node)));
    }
    el("Cell", attrs, children)
}

fn row(r: &Row) -> El {
    let mut attrs = Vec::new();
    typed_attr(&mut attrs, &r.raw, "N", r.name.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &r.raw, "LocalName", r.local_name.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &r.raw, "IX", r.ix.map(|n| n.to_string()));
    typed_attr(&mut attrs, &r.raw, "T", r.row_type.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &r.raw, "Del", r.del.then(|| "1".to_owned()));
    raw_attrs(&mut attrs, &r.raw);
    let typed: Vec<Node> = r.cells.iter().map(|(n, c)| Node::El(cell(n.as_str(), c))).collect();
    el("Row", attrs, assemble(typed, &[&r.passthrough]))
}

fn section(s: &Section) -> El {
    let mut attrs = vec![("N".to_owned(), s.name.as_str().to_owned())];
    typed_attr(&mut attrs, &s.raw, "IX", s.ix.filter(|_| s.name == "Geometry").map(|n| n.to_string()));
    typed_attr(&mut attrs, &s.raw, "Del", s.del.then(|| "1".to_owned()));
    raw_attrs(&mut attrs, &s.raw);
    // A section's cells and rows are typed children in the order the reader met them: cells, then rows, as fixtures write them.
    let mut typed: Vec<Node> = s.cells.iter().map(|(n, c)| Node::El(cell(n.as_str(), c))).collect();
    typed.extend(s.rows.iter().map(|r| Node::El(row(r))));
    el("Section", attrs, assemble(typed, &[&s.passthrough]))
}

/// The links of a sheet as attributes: a number is `LineStyle="n"` unless the sheet's `raw` holds the attribute.
fn links(attrs: &mut Vec<(String, String)>, sheet: &Sheet) {
    typed_attr(attrs, &sheet.raw, "LineStyle", sheet.links.line.map(|n| n.to_string()));
    typed_attr(attrs, &sheet.raw, "FillStyle", sheet.links.fill.map(|n| n.to_string()));
    typed_attr(attrs, &sheet.raw, "TextStyle", sheet.links.text.map(|n| n.to_string()));
}

/// The typed children of a sheet: cells in name order, then sections.
fn sheet_typed(sheet: &Sheet) -> Vec<Node> {
    let mut typed: Vec<Node> = sheet.cells.iter().map(|(n, c)| Node::El(cell(n.as_str(), c))).collect();
    typed.extend(sheet.sections.iter().map(|s| Node::El(section(s))));
    typed
}

/// A sheet that is its own element (`PageSheet`, `DocumentSheet`): the links and the raw attributes.
pub fn sheet_only(name: &str, sheet: &Sheet) -> El {
    let mut attrs = Vec::new();
    links(&mut attrs, sheet);
    // Sheet-only elements keep every other attribute in `raw`, the links among them when they could not be typed.
    raw_attrs(&mut attrs, &sheet.raw);
    el(name, attrs, assemble(sheet_typed(sheet), &[&sheet.passthrough]))
}

fn text(t: &Text) -> El {
    let children = t
        .parts
        .iter()
        // ODF's markup and a newer build's are never written into a VSDX `Text`.
        .filter(|p| !matches!(p, TextPart::OdfRaw(_) | TextPart::NativeRaw(_)))
        .map(|p| match p {
            TextPart::Chars(c) => Node::Text(c.to_string()),
            TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => Node::Text(String::new()), // filtered out above
            TextPart::Cp(n) => Node::El(el("cp", vec![("IX".into(), n.to_string())], vec![])),
            TextPart::Pp(n) => Node::El(el("pp", vec![("IX".into(), n.to_string())], vec![])),
            TextPart::Tp(n) => Node::El(el("tp", vec![("IX".into(), n.to_string())], vec![])),
            TextPart::Fld { ix, text } => Node::El(el("fld", vec![("IX".into(), ix.to_string())], vec![Node::Text(text.to_string())])),
            TextPart::Raw(node) => node_to_tree(node),
        })
        .collect();
    el("Text", vec![], children)
}

fn foreign(f: &ForeignData) -> El {
    let attrs = f.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    let mut typed = Vec::new();
    if let Some(id) = &f.rel_id {
        typed.push(Node::El(el("Rel", vec![("r:id".into(), id.to_string())], vec![])));
    }
    let mut children = assemble(typed, &[&f.passthrough]);
    if let Some(t) = &f.text {
        // The text of a `ForeignData` is the concatenation of its text nodes, whose places the model does not keep: it is put first.
        children.insert(0, Node::Text(t.to_string()));
    }
    el("ForeignData", attrs, children)
}

pub fn shape(s: &Shape) -> El {
    let mut attrs = Vec::new();
    typed_attr(&mut attrs, &s.raw, "ID", Some(s.id.to_string()));
    typed_attr(&mut attrs, &s.raw, "Name", s.name.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &s.raw, "NameU", s.name_u.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &s.raw, "Type", s.kind.text().map(str::to_owned));
    typed_attr(&mut attrs, &s.raw, "Master", s.master.map(|n| n.to_string()));
    typed_attr(&mut attrs, &s.raw, "MasterShape", s.master_shape.map(|n| n.to_string()));
    typed_attr(&mut attrs, &s.raw, "Del", s.del.then(|| "1".to_owned()));
    links(&mut attrs, &s.sheet);
    raw_attrs(&mut attrs, &s.raw);
    // The sheet's own raw attributes on a shape are the links that could not be typed.
    for (k, v) in s.sheet.raw.iter() {
        attrs.push((k.to_owned(), v.to_owned()));
    }
    let mut typed = sheet_typed(&s.sheet);
    if let Some(t) = &s.text {
        typed.push(Node::El(text(t)));
    }
    for (name, data) in [("Data1", &s.data1), ("Data2", &s.data2), ("Data3", &s.data3)] {
        if let Some(d) = data {
            typed.push(Node::El(el(name, vec![], vec![Node::Text(d.to_string())])));
        }
    }
    if let Some(f) = &s.foreign {
        typed.push(Node::El(foreign(f)));
    }
    if !s.children.is_empty() {
        typed.push(Node::El(shapes(&s.children)));
    }
    // A shape's unknown nodes are in its own list, and its triggers and unknown cells in the sheet's: the sheet's first at an equal place.
    el("Shape", attrs, assemble(typed, &[&s.sheet.passthrough, &s.passthrough]))
}

pub fn shapes(list: &ShapeList) -> El {
    el("Shapes", vec![], list.iter().map(|s| Node::El(shape(s))).collect())
}

/// The text of a tree in a form that does not depend on what the model cannot hold: each element as its name, its attributes sorted (the
/// model holds a typed attribute apart from the raw ones, so their order is not the file's), and its children **in order**, with blank text
/// left out. Position is part of it.
pub fn signature(e: &El) -> String {
    let mut attrs = e.attrs.clone();
    attrs.sort();
    let mut s = format!("<{} {attrs:?}>", e.name);
    for child in &e.children {
        match child {
            Node::El(c) => s.push_str(&signature(c)),
            Node::Text(t) | Node::CData(t) if t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) => {}
            Node::Text(t) | Node::CData(t) => s.push_str(&format!("T[{t}]")),
            Node::Comment(c) => s.push_str(&format!("C[{c}]")),
            Node::Pi { target, data } => s.push_str(&format!("P[{target} {data}]")),
        }
    }
    s.push_str("</>");
    s
}

/// A style: its identity, its links, the rest of its attributes, and its sheet's children.
pub fn style(st: &Style) -> El {
    let mut attrs = Vec::new();
    typed_attr(&mut attrs, &st.raw, "ID", Some(st.id.to_string()));
    typed_attr(&mut attrs, &st.raw, "Name", st.name.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &st.raw, "NameU", st.name_u.as_ref().map(ToString::to_string));
    links(&mut attrs, &st.sheet);
    raw_attrs(&mut attrs, &st.raw);
    for (k, v) in st.sheet.raw.iter() {
        attrs.push((k.to_owned(), v.to_owned()));
    }
    el("StyleSheet", attrs, assemble(sheet_typed(&st.sheet), &[&st.sheet.passthrough, &st.passthrough]))
}

/// The `Connect` rows of a part.
pub fn connects(list: &diacraft_doc::Connects) -> El {
    let rows = list
        .iter()
        .map(|c| {
            let mut attrs = Vec::new();
            typed_attr(&mut attrs, &c.raw, "FromSheet", Some(c.from_sheet.to_string()));
            typed_attr(&mut attrs, &c.raw, "FromCell", c.from_cell.as_ref().map(ToString::to_string));
            typed_attr(&mut attrs, &c.raw, "FromPart", c.from_part.map(|n| n.to_string()));
            typed_attr(&mut attrs, &c.raw, "ToSheet", Some(c.to_sheet.to_string()));
            typed_attr(&mut attrs, &c.raw, "ToCell", c.to_cell.as_ref().map(ToString::to_string));
            typed_attr(&mut attrs, &c.raw, "ToPart", c.to_part.map(|n| n.to_string()));
            raw_attrs(&mut attrs, &c.raw);
            Node::El(el("Connect", attrs, vec![]))
        })
        .collect();
    el("Connects", vec![], rows)
}

// ------------------------------------------------------------------------------------------------------------- whole parts

/// The `Rel` a page or master row holds, if it names a part.
fn rel(id: &Option<std::sync::Arc<str>>) -> Option<Node> {
    id.as_ref().map(|id| Node::El(el("Rel", vec![("r:id".into(), id.to_string())], vec![])))
}

/// A `Page` row of the pages index.
pub fn page_row(p: &Page) -> El {
    let mut attrs = Vec::new();
    typed_attr(&mut attrs, &p.raw, "ID", Some(p.id.to_string()));
    typed_attr(&mut attrs, &p.raw, "Name", Some(p.name.to_string()));
    typed_attr(&mut attrs, &p.raw, "NameU", p.name_u.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &p.raw, "Background", p.background.then(|| "1".to_owned()));
    typed_attr(&mut attrs, &p.raw, "BackPage", p.back_page.map(|n| n.to_string()));
    raw_attrs(&mut attrs, &p.raw);
    // The page sheet comes first when the page has one that holds anything, as the builder writes it, and the `Rel` after.
    let mut typed = Vec::new();
    if *p.sheet != Sheet::default() {
        typed.push(Node::El(sheet_only("PageSheet", &p.sheet)));
    }
    typed.extend(rel(&p.rel_id));
    el("Page", attrs, assemble(typed, &[&p.passthrough]))
}

/// A `Master` row of the masters index.
pub fn master_row(m: &Master) -> El {
    let mut attrs = Vec::new();
    typed_attr(&mut attrs, &m.raw, "ID", Some(m.id.to_string()));
    typed_attr(&mut attrs, &m.raw, "Name", m.name.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &m.raw, "NameU", m.name_u.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &m.raw, "UniqueID", m.unique_id.as_ref().map(ToString::to_string));
    typed_attr(&mut attrs, &m.raw, "PatternFlags", m.pattern_flags.map(|n| n.to_string()));
    raw_attrs(&mut attrs, &m.raw);
    let mut typed = Vec::new();
    if *m.sheet != Sheet::default() {
        typed.push(Node::El(sheet_only("PageSheet", &m.sheet)));
    }
    typed.extend(rel(&m.rel_id));
    if let Some(icon) = &m.icon {
        typed.push(Node::El(el("Icon", vec![], vec![Node::Text(icon.to_string())])));
    }
    el("Master", attrs, assemble(typed, &[&m.passthrough]))
}

pub fn pages_index(doc: &Document) -> El {
    el("Pages", vec![], doc.pages.iter().map(|p| Node::El(page_row(p))).collect())
}

pub fn masters_index(doc: &Document) -> El {
    el("Masters", vec![], doc.masters.iter().map(|m| Node::El(master_row(m))).collect())
}

/// The document part: the colour table, the font table, the styles and the document sheet, with the document's own unknown children placed.
pub fn document_part(doc: &Document) -> El {
    let mut typed = Vec::new();
    if !doc.colors.is_empty() {
        let rows = doc
            .colors
            .iter()
            .map(|c| Node::El(el("ColorEntry", vec![("IX".into(), c.ix.to_string()), ("RGB".into(), c.rgb.to_string())], vec![])))
            .collect();
        typed.push(Node::El(el("Colors", vec![], rows)));
    }
    if !doc.fonts.is_empty() {
        let rows = doc
            .fonts
            .iter()
            .map(|f| {
                let mut attrs = vec![("NameU".to_owned(), f.name_u.to_string())];
                raw_attrs(&mut attrs, &f.raw);
                Node::El(el("FaceName", attrs, vec![]))
            })
            .collect();
        typed.push(Node::El(el("FaceNames", vec![], rows)));
    }
    if !doc.styles.is_empty() {
        typed.push(Node::El(el("StyleSheets", vec![], doc.styles.iter().map(|s| Node::El(style(s))).collect())));
    }
    if *doc.sheet != Sheet::default() {
        typed.push(Node::El(sheet_only("DocumentSheet", &doc.sheet)));
    }
    el("VisioDocument", vec![], assemble(typed, &[&doc.passthrough]))
}

/// A page or master part: `Shapes` and `Connects`.
pub fn contents(root: &str, shapes_of: &ShapeList, connects_of: &diacraft_doc::Connects) -> El {
    let mut children = Vec::new();
    if !shapes_of.is_empty() {
        children.push(Node::El(shapes(shapes_of)));
    }
    if !connects_of.is_empty() {
        children.push(Node::El(connects(connects_of)));
    }
    el(root, vec![], children)
}

/// What the VSDX side of an extension slot holds, as text: each key and what is held under it.
fn rest_text(out: &mut String, rest: &diacraft_doc::VsdxRest) {
    for (key, kept) in &rest.0 {
        out.push_str(&format!("[{key}{}]", crate::held::dump(kept)));
    }
}

/// What the slots of a shape and of what it holds (its text, its picture, its members) hold, as text.
fn shape_rest(out: &mut String, list: &ShapeList) {
    for s in list.iter() {
        if let Some(e) = s.ext.get() {
            rest_text(out, &e.vsdx);
        }
        if let Some(e) = s.text.as_ref().and_then(|t| t.ext.get()) {
            rest_text(out, &e.vsdx);
        }
        if let Some(e) = s.foreign.as_ref().and_then(|f| f.ext.get()) {
            rest_text(out, &e.vsdx);
        }
        shape_rest(out, &s.children);
    }
}

/// What the model holds besides its fields, as text: the `vsdx` side of every extension slot a reader of a package fills, and the document's
/// `VsdxHeld` and held parts.
pub fn held_slots(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(e) = doc.ext.get() {
        rest_text(&mut out, &e.vsdx);
    }
    for f in doc.fonts.iter() {
        if let Some(e) = f.ext.get() {
            rest_text(&mut out, &e.vsdx);
        }
    }
    for p in doc.pages.iter() {
        if let Some(e) = p.ext.get() {
            rest_text(&mut out, &e.vsdx);
        }
        shape_rest(&mut out, &p.shapes);
        for c in p.connects.iter() {
            if let Some(e) = c.ext.get() {
                rest_text(&mut out, &e.vsdx);
            }
        }
    }
    for m in doc.masters.iter() {
        if let Some(e) = m.ext.get() {
            rest_text(&mut out, &e.vsdx);
        }
        shape_rest(&mut out, &m.shapes);
        for c in m.connects.iter() {
            if let Some(e) = c.ext.get() {
                rest_text(&mut out, &e.vsdx);
            }
        }
    }
    // The property parts: the fields (the title and `DocProps`) and what each part holds beside them, by the element it came from (`VsdxHeld::props_rest`).
    out.push_str(&format!("[title {}][props {:?}]", doc.title, doc.props));
    if let Some(held) = &doc.vsdx {
        for (key, kept) in &held.props_rest {
            out.push_str(&format!("[{key}{}]", crate::held::dump(kept)));
        }
    }
    out
}

/// Everything the model holds of a document, as the text of its parts' trees and of what its slots hold: what a marker injected into a part must be in.
pub fn everything(doc: &Document) -> String {
    let mut out = String::new();
    for tree in [document_part(doc), pages_index(doc), masters_index(doc)] {
        out.push_str(&to_xml(&tree));
    }
    for p in doc.pages.iter() {
        out.push_str(&to_xml(&contents("PageContents", &p.shapes, &p.connects)));
    }
    for m in doc.masters.iter() {
        out.push_str(&to_xml(&contents("MasterContents", &m.shapes, &m.connects)));
    }
    out.push_str(&held_slots(doc));
    out
}

// ------------------------------------------------------------------------------------------------------------------ writing a tree

fn escape(text: &str, quote: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\n' if quote => out.push_str("&#10;"),
            '\r' if quote => out.push_str("&#13;"),
            '\t' if quote => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

/// A tree as XML text. Recursive: the trees of the tests are shallow.
pub fn to_xml(e: &El) -> String {
    let mut s = format!("<{}", e.name);
    for (k, v) in &e.attrs {
        s.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
    }
    if e.children.is_empty() {
        s.push_str("/>");
        return s;
    }
    s.push('>');
    for child in &e.children {
        match child {
            Node::El(c) => s.push_str(&to_xml(c)),
            Node::Text(t) | Node::CData(t) => s.push_str(&escape(t, false)),
            Node::Comment(c) => s.push_str(&format!("<!--{c}-->")),
            Node::Pi { target, data } => {
                if data.is_empty() {
                    s.push_str(&format!("<?{target}?>"));
                } else {
                    s.push_str(&format!("<?{target} {data}?>"));
                }
            }
        }
    }
    s.push_str(&format!("</{}>", e.name));
    s
}
