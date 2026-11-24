//! Layers: the layer set of `office:master-styles`, or of a page or master page
//! that has its own, as the `Layer` rows of a page, and LibreOffice's five layers left out of them (they are not VSDX layers, and a
//! shape on one has no `LayerMember`; its `draw:layer` is kept on the shape).

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{CellName, Section, Sheet};

use super::Reader;
use super::cells;
use super::names::{Names, Ns};
use super::units;
use crate::error::OdgError;

/// A `draw:layer`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layer {
    pub name: String,
    pub display: String,
    pub protected: bool,
}

/// The five layers LibreOffice writes into every document: not VSDX layers.
pub(crate) const PRODUCER_LAYERS: [&str; 5] = ["layout", "background", "backgroundobjects", "controls", "measurelines"];

/// Reads a `draw:layer-set`.
pub(crate) fn read_layer_set<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<Vec<Layer>, OdgError> {
    let mut layers = Vec::new();
    for child in el.elements() {
        r.saw_element();
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "layer") {
            let a = names.attrs(child);
            r.saw_attrs(a.len());
            if let Some(name) = a.peek(Ns::Draw, "name") {
                r.take_items(1)?;
                r.take_bytes(name.len().saturating_add(a.peek(Ns::Draw, "display").map_or(0, str::len)))?;
                layers.push(Layer {
                    name: name.to_owned(),
                    display: a.peek(Ns::Draw, "display").unwrap_or("always").to_owned(),
                    protected: a.peek(Ns::Draw, "protected").and_then(units::boolean).unwrap_or(false),
                });
            }
        } else {
            r.keep("layer_set_child");
        }
        names.leave(mark);
    }
    Ok(layers)
}

/// Whether the `Layer` rows a page's own layer set gives say all that it says: the layer set has no attribute, and holds only layers that have a name no other
/// has and none of LibreOffice's five (the rows leave those out), no attribute but `draw:name`, `draw:display` (one of its four words) and `draw:protected` (a boolean), and no
/// child. A layer set that is not so is held whole, with the page's other markup.
pub(crate) fn rows_say_all<'t>(names: &mut Names<'t>, set: &'t El) -> bool {
    if names.attrs(set).len() != 0 {
        return false;
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for child in set.elements() {
        let mark = names.enter(child);
        let exact = names.is(child, Ns::Draw, "layer") && {
            let attrs = names.attrs(child);
            let name = attrs.peek(Ns::Draw, "name");
            attrs.iter().all(|a| a.ns == Ns::Draw && matches!(a.local, "name" | "display" | "protected"))
                && matches!(attrs.peek(Ns::Draw, "display").unwrap_or("always"), "always" | "screen" | "printer" | "none")
                && attrs.peek(Ns::Draw, "protected").is_none_or(|v| units::boolean(v).is_some())
                && name.is_some_and(|n| !PRODUCER_LAYERS.contains(&n) && seen.insert(n))
                && child.elements().next().is_none()
        };
        names.leave(mark);
        if !exact {
            return false;
        }
    }
    true
}

/// The `Layer` row `ix` that a `draw:layer` named `name` gives a page: `Name` and `NameUniv` the name, and `Visible`, `Print` and `Lock` from its display and protection.
/// The writer makes the rows a layer it declares will read as with this too, so that what it predicts of the markup is what the reader makes.
pub(crate) fn layer_row(ix: u32, name: &str, visible: bool, print: bool, protected: bool) -> diacraft_doc::Row {
    let mut row = diacraft_doc::Row { ix: Some(ix), ..Default::default() };
    let flag = |on: bool| cells::plain(if on { 1.0 } else { 0.0 });
    for (cell, value) in
        [("Name", cells::text(name)), ("NameUniv", cells::text(name)), ("Visible", flag(visible)), ("Print", flag(print)), ("Lock", flag(protected))]
    {
        row.cells.insert(CellName::new(cell), value);
    }
    row
}

/// What a layer's `draw:display` says of it: whether it is seen on the screen and whether it prints. `always` is both, `screen` and `printer` one each, and any other word
/// neither.
pub(crate) fn display_flags(display: &str) -> (bool, bool) {
    (matches!(display, "always" | "screen"), matches!(display, "always" | "printer"))
}

/// The layer rows of a page: every layer of `layers` but LibreOffice's five, in order. Gives each name's
/// row, the first of a name where two layers share one, for a shape's `draw:layer` to be looked up in one step.
///
/// More layers than a section holds rows are refused before any row is built.
pub(crate) fn layer_rows(r: &mut Reader, sheet: &mut Sheet, layers: Option<&[Layer]>) -> Result<HashMap<String, u32>, OdgError> {
    let mut names = HashMap::new();
    let Some(layers) = layers else { return Ok(names) };
    let rows = layers.iter().filter(|l| !PRODUCER_LAYERS.contains(&l.name.as_str())).count();
    if rows > diacraft_doc::MAX_ROWS_PER_SECTION {
        return Err(OdgError::TooComplex { what: "layers in one page", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
    }
    let mut section = Section::new("Layer", None);
    for layer in layers {
        if PRODUCER_LAYERS.contains(&layer.name.as_str()) {
            continue;
        }
        let ix = u32::try_from(section.rows.len()).unwrap_or(u32::MAX);
        let (visible, print) = display_flags(&layer.display);
        let row = layer_row(ix, &layer.name, visible, print, layer.protected);
        r.take_cells(5)?;
        r.take_items(super::cost::ROW)?;
        // The name twice in the row's cells, and once more as the lookup's key.
        r.take_bytes(layer.name.len().saturating_mul(3))?;
        section.rows.push(Arc::new(row));
        names.entry(layer.name.clone()).or_insert(ix);
    }
    if !section.rows.is_empty() {
        sheet.sections.push(Arc::new(section));
    }
    Ok(names)
}
