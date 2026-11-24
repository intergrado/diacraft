//! The layers the model's pages have, declared in the layer set of `styles.xml` (the layer row of "Pages,
//! layers, package"): the writer writes the union by name of all the pages' `Layer` rows as the document's layer set, **keeping the layers the package already declares, in their order,
//! and adding the new names at the end**. A layer's `draw:display` is said by `Visible` and `Print` (`always`, `screen`, `printer`, `none`) and `draw:protected` by `Lock`.
//!
//! * **A layer the set already declares is left as it is**, bytes and all, unless the page's row says another `Visible`, `Print` or `Lock` than the declaration does: then its
//!   `draw:display` or `draw:protected` is written from the row (regenerated, never a layer added twice). **The row that changed decides** (
//!   `write::layers::deciding`): of the pages' rows of the layer, the last that differs from the declaration; a layer the set does not declare takes the first page's row, and so
//!   does every layer of a drawing that has no `.odg` file, whose package declares none of its layers (no row is a changed one). The five layers of LibreOffice (`layout`,
//!   `background`, `backgroundobjects`, `controls`, `measurelines`) are no rows and are never added from one.
//! * **A drawing that has no layer row changes nothing**: the part is returned as it came, so a package whose styles hold no layer set is not given one, and a part
//!   that already declares every layer the rows name is not written again. A package that has no layer set but a page with rows is given a set holding those layers.
//! * **What the format cannot say stays in DiaCraft's document file**: `Active`, `Snap`, `Glue`, `Color` and `ColorTrans`, and the rows of a page whose layers differ from the union
//!   (tier 2). A shape is written on the first of its layers when the set declares it at the row the model has (`write::layers`).
//!
//! Every row is a step charged before it is looked at, and the part is parsed and written again only when something is to be declared.

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, SectionKey};

use super::containers::Dressed;
use crate::limits::Limits;
use crate::read::layers::PRODUCER_LAYERS;
use crate::write::layers::{Flags, deciding, flags_of};
use crate::write::{WriteError, WriteWork};

/// A layer of the model that the layer set is to declare: its name, and the first row of it on each page that has it, in the pages' order (`Visible`, `Print`, `Lock`).
struct Wanted {
    name: String,
    rows: Vec<Flags>,
}

/// The flags a layer is written with.
struct Chosen {
    visible: bool,
    print: bool,
    protected: bool,
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// The layers of the pages' tables, each name once, in the order the pages and the rows give them, with the first row of it on each page: not a vacant row, not a name that is
/// empty, holds a control character or is one of LibreOffice's five.
fn wanted(doc: &Document, work: &mut WriteWork) -> Result<Vec<Wanted>, WriteError> {
    let key = SectionKey::new("Layer", None);
    let mut out: Vec<Wanted> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for page in doc.pages.iter() {
        let Some(section) = page.sheet.section(&key) else { continue };
        let mut on_page: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for row in section.rows.iter() {
            work.charge_steps(1)?;
            let Some(name) = row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text()) else { continue };
            if name.is_empty() || name.chars().any(char::is_control) || PRODUCER_LAYERS.contains(&name) || !on_page.insert(name) {
                continue;
            }
            let flags = flags_of(row);
            match at.get(name).and_then(|i| out.get_mut(*i)) {
                Some(w) => w.rows.push(flags),
                None => {
                    at.insert(name.to_owned(), out.len());
                    out.push(Wanted { name: name.to_owned(), rows: vec![flags] });
                }
            }
        }
    }
    Ok(out)
}

/// The flags the declaration `el` of a layer gives, as the reader reads them (`read::layers::layer_rows`: `always` and `screen` are visible, `always` and `printer` print, a
/// missing `draw:display` is `always`, and `draw:protected` is a boolean, false when it is missing or is none).
fn declared_flags(el: &El) -> Flags {
    let display = attr_at(el, "display").and_then(|i| el.attrs.get(i)).map_or("always", |(_, v)| v.as_str());
    let protected = attr_at(el, "protected").and_then(|i| el.attrs.get(i)).and_then(|(_, v)| crate::read::units::boolean(v)).unwrap_or(false);
    let (visible, print) = crate::read::layers::display_flags(display);
    (visible, print, protected)
}

/// The row that decides a layer's flags ([`deciding`]): `was` is what the set declares of it, `None` when it declares none. The last row that differs from the declaration
/// decides; with none (and for a layer the set does not declare), the first page's row.
fn chosen(w: &Wanted, was: Option<Flags>) -> Chosen {
    let (visible, print, protected) = chosen_flags(&w.rows, was);
    Chosen { visible, print, protected }
}

/// The flags of the row of `rows` (the first row of a layer on each page) that [`chosen`] takes: the last that differs from `was`, else the first; a visible, printed and unlocked
/// layer for no row.
pub(crate) fn chosen_flags(rows: &[Flags], was: Option<Flags>) -> Flags {
    let changed = was.and_then(|was| deciding(&rows.iter().map(|now| (*now, was)).collect::<Vec<_>>()));
    rows.get(changed.unwrap_or(0)).copied().unwrap_or((true, true, false))
}

/// The `draw:display` word of a layer that is visible and prints as given.
fn display_of(visible: bool, print: bool) -> &'static str {
    match (visible, print) {
        (true, true) => "always",
        (true, false) => "screen",
        (false, true) => "printer",
        (false, false) => "none",
    }
}

/// What a `draw:display` word says: whether the layer is visible, and whether it prints. `None` for a word the standard does not give.
fn says(display: &str) -> Option<(bool, bool)> {
    match display {
        "always" => Some((true, true)),
        "screen" => Some((true, false)),
        "printer" => Some((false, true)),
        "none" => Some((false, false)),
        _ => None,
    }
}

fn attr_at(el: &El, name: &str) -> Option<usize> {
    el.attrs.iter().position(|(k, _)| local(k) == name)
}

/// Puts `value` in the attribute `name` of `el`: where it stands, else as `draw:<name>`.
fn set_attr(el: &mut El, name: &str, value: &str) {
    match attr_at(el, name) {
        Some(at) => {
            if let Some(slot) = el.attrs.get_mut(at) {
                slot.1 = value.to_owned();
            }
        }
        None => el.attrs.push((format!("draw:{name}"), value.to_owned())),
    }
}

/// A new `draw:layer` named `name`. `draw:display` is written when it is not the default, and `draw:protected` when it is true.
fn new_layer(name: &str, w: &Chosen) -> El {
    let mut el = El { name: "draw:layer".to_owned(), attrs: vec![("draw:name".to_owned(), name.to_owned())], children: Vec::new() };
    let display = display_of(w.visible, w.print);
    if display != "always" {
        el.attrs.push(("draw:display".to_owned(), display.to_owned()));
    }
    if w.protected {
        el.attrs.push(("draw:protected".to_owned(), "true".to_owned()));
    }
    el
}

/// Declares the layers of the model's pages in the layer set of the dressed `styles.xml`.
pub(super) fn declare(dressed: Dressed, doc: &Document, limits: &Limits, work: &mut WriteWork) -> Result<Dressed, WriteError> {
    let wanted = wanted(doc, work)?;
    if wanted.is_empty() {
        return Ok(dressed);
    }
    let mut root = diacraft_container::xml::parse(&dressed.bytes).map_err(|e| WriteError::SelfCheck(format!("the package made is not XML: {e}")))?;
    let Some(masters) = root.children.iter_mut().find_map(|n| match n {
        Node::El(el) if local(&el.name) == "master-styles" => Some(el),
        _ => None,
    }) else {
        return Ok(dressed);
    };
    let at = masters.children.iter().position(|n| matches!(n, Node::El(el) if local(&el.name) == "layer-set"));
    let at = match at {
        Some(at) => at,
        None => {
            masters.children.insert(0, Node::El(El { name: "draw:layer-set".to_owned(), attrs: Vec::new(), children: Vec::new() }));
            0
        }
    };
    let Some(Node::El(set)) = masters.children.get_mut(at) else { return Ok(dressed) };
    let mut changed = false;
    // The layers the set declares, by name (the first of a name), found once: a table of thousands of layers is no quadratic walk.
    let mut declared: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, node) in set.children.iter().enumerate() {
        work.charge_steps(1)?;
        if let Node::El(el) = node
            && local(&el.name) == "layer"
            && let Some((_, name)) = attr_at(el, "name").and_then(|at| el.attrs.get(at))
        {
            declared.entry(name.clone()).or_insert(i);
        }
    }
    for wanted in &wanted {
        work.charge_steps(1)?;
        let Some(declared) = declared.get(&wanted.name).copied() else {
            set.children.push(Node::El(new_layer(&wanted.name, &chosen(wanted, None))));
            changed = true;
            continue;
        };
        let Some(Node::El(el)) = set.children.get_mut(declared) else { continue };
        let w = chosen(wanted, Some(declared_flags(el)));
        let display = attr_at(el, "display").and_then(|i| el.attrs.get(i)).map_or("always", |(_, v)| v.as_str());
        if says(display).is_some_and(|(visible, print)| (visible, print) != (w.visible, w.print)) {
            set_attr(el, "display", display_of(w.visible, w.print));
            changed = true;
        }
        let protected = attr_at(el, "protected").and_then(|i| el.attrs.get(i)).map(|(_, v)| v.as_str());
        let says_protected = match protected {
            None | Some("false") => Some(false),
            Some("true") => Some(true),
            Some(_) => None,
        };
        if says_protected.is_some_and(|p| p != w.protected) {
            set_attr(el, "protected", if w.protected { "true" } else { "false" });
            changed = true;
        }
    }
    if !changed {
        // A set that was made for nothing (the part had none and no layer was wanted) is not possible here: a set is made only with a layer to put in it.
        return Ok(dressed);
    }
    let bytes = diacraft_container::xml::to_bytes(&root, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", b"\n", true)
        .map_err(|_| WriteError::Unsupported("a layer name the layer set cannot hold"))?;
    work.charge_copy(bytes.len(), limits)?;
    Ok(Dressed { bytes, left: dressed.left })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Page, Row, Section, Value};

    use super::*;

    /// A page `id` whose `Layer` section has a row `Notes` with the flags given.
    fn page_with_notes(id: u32, (visible, print, lock): Flags) -> Page {
        let mut row = Row { ix: Some(0), ..Row::default() };
        let flag = |on: bool| Cell::constant(Value::from_text(if on { "1" } else { "0" }));
        row.cells.insert(CellName::new("Name"), Cell::constant(Value::from_text("Notes")));
        for (cell, on) in [("Visible", visible), ("Print", print), ("Lock", lock)] {
            row.cells.insert(CellName::new(cell), flag(on));
        }
        let mut section = Section::new("Layer", None);
        section.rows.push(Arc::new(row));
        let mut page = Page::new(id, &format!("P{id}"));
        Arc::make_mut(&mut page.sheet).sections.push(Arc::new(section));
        page
    }

    /// The `draw:layer` start tag of `Notes` in the part made.
    fn notes_tag(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes);
        let at = text.find("draw:name=\"Notes\"").unwrap_or_else(|| panic!("no Notes in {text}"));
        let start = text[..at].rfind('<').unwrap();
        text[start..at + text[at..].find('>').unwrap()].to_owned()
    }

    /// In the model writer: of the pages' rows of a layer the package declares, the last that differs from the declaration decides; with none, the declaration
    /// stays; a layer the package does not declare takes the first page's row.
    #[test]
    fn the_row_that_differs_from_the_declaration_decides_and_else_the_first_page() {
        let styles = |layers: &str| {
            format!(
                "<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\">\
                 <office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/>{layers}</draw:layer-set></office:master-styles></office:document-styles>"
            )
        };
        let declare_for = |pages: &[Flags], declared: &str| {
            let mut doc = Document::empty();
            for (k, flags) in pages.iter().enumerate() {
                doc.pages.push(Arc::new(page_with_notes(u32::try_from(k).unwrap() + 1, *flags)));
            }
            let mut work = WriteWork { memory_room: u64::MAX, memory_cap: u64::MAX, step_cap: u64::MAX, byte_cap: u64::MAX, ..WriteWork::default() };
            let dressed = Dressed { bytes: styles(declared).into_bytes(), left: super::super::document::Left::default() };
            notes_tag(&declare(dressed, &doc, &Limits::default(), &mut work).unwrap().bytes)
        };
        let printer = "<draw:layer draw:name=\"Notes\" draw:display=\"printer\"/>";
        // The package says printer only, (false, true, false): page 1 is as declared, page 2 is shown too.
        let tag = declare_for(&[(false, true, false), (true, true, false)], printer);
        assert!(tag.contains("draw:display=\"always\""), "the second page's change: {tag}");
        // Both changed: the later page.
        let tag = declare_for(&[(true, false, false), (false, false, true)], printer);
        assert!(tag.contains("draw:display=\"none\"") && tag.contains("draw:protected=\"true\""), "the later page: {tag}");
        // None changed: the declaration as it was.
        let tag = declare_for(&[(false, true, false), (false, true, false)], printer);
        assert!(tag.contains("draw:display=\"printer\"") && !tag.contains("protected"), "{tag}");
        // Not declared: the first page's row.
        let tag = declare_for(&[(true, false, false), (false, true, true)], "");
        assert!(tag.contains("draw:display=\"screen\"") && !tag.contains("protected"), "the first page: {tag}");
    }
}
