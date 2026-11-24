//! Layers: which shapes a page's layer table hides, and which colour a layer lays over its one shape.
//!
//! * A shape is **not drawn** only when it lists layers and every one of them is hidden. On no layer, or on a shown one
//!   among hidden ones, it is drawn.
//! * A group's members go by their own lists: a hidden group's own drawing goes, and a member on no layer stays.
//! * A list is read against the table of the page the shape sits on, and a background page has its own. A list that
//!   names a row the page lacks drops that part, and a shape left with nothing is on no layer.
//! * A layer's colour is laid over a shape on **exactly one** layer, and over no other.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, CellName, RowKey, SectionKey, SheetRef, Source};

use crate::read::{Cx, Val};

/// The colour a layer lays over a shape on that one layer: the line and text colour, with the alpha of `trans`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerColour {
    pub color: Rgba,
    pub trans: f64,
}

#[derive(Clone, Debug)]
struct LayerRow {
    visible: bool,
    /// `Print`: shapes on the layer go to paper. A missing cell reads 1.
    print: bool,
    colour: Option<LayerColour>,
    name_univ: Option<Arc<str>>,
}

/// The `Layer` section of one page's (or master's) sheet.
#[derive(Clone, Debug, Default)]
pub(crate) struct LayerTable {
    rows: HashMap<u32, LayerRow>,
}

/// What a shape's list makes of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Verdict {
    pub drawn: bool,
    pub colour: Option<LayerColour>,
}

impl Verdict {
    pub(crate) const PLAIN: Verdict = Verdict { drawn: true, colour: None };
}

impl LayerTable {
    /// The table of the sheet `at`: a page's or a master's. A sheet with no `Layer` section has an empty table.
    pub(crate) fn read(cx: &Cx<'_, '_>, at: &SheetRef) -> LayerTable {
        let key = SectionKey::new("Layer", None);
        let mut table = LayerTable::default();
        let Some(rows) = cx.res.effective_rows(at, &key) else {
            return table;
        };
        for row in rows {
            let RowKey::Ix(ix) = row.key else {
                continue;
            };
            let cell = |name: &str| CellAddr { section: Some(key.clone()), row: Some(row.key.clone()), cell: CellName::new(name) };
            // `Visible` is tested against 0, as truth is, and a missing cell reads shown.
            let visible = cx.flag_at(at, &cell("Visible"), true);
            let print = cx.flag_at(at, &cell("Print"), true);
            let colour = match cx.val(at, &cell("Color")) {
                // 255 says the layer has no colour.
                Val::Num(255.0) => None,
                Val::Missing | Val::Text(_) => None,
                _ => cx.colour_at(at, &cell("Color")).map(|color| LayerColour { color, trans: cx.num_at(at, &cell("ColorTrans"), 0.0) }),
            };
            let name_univ = cx.text_at(at, &cell("NameUniv")).filter(|n| !n.is_empty());
            // A row with a `Status` of 1 or 2 and no name is the place a removed layer left: it keeps its number, and a list that names
            // only it names no layer.
            let vacant = matches!(cx.num_at(at, &cell("Status"), 0.0) as i64, 1 | 2) && cx.text_at(at, &cell("Name")).is_none_or(|n| n.is_empty());
            if vacant {
                continue;
            }
            table.rows.insert(ix, LayerRow { visible, print, colour, name_univ });
        }
        table
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row whose `NameUniv` is `name`.
    fn by_name(&self, name: &str) -> Option<u32> {
        let mut found: Option<u32> = None;
        for (ix, row) in &self.rows {
            if row.name_univ.as_deref() == Some(name) && found.is_none_or(|f| *ix < f) {
                found = Some(*ix);
            }
        }
        found
    }

    /// What the list `members` (row numbers of this table) makes of a shape. On screen a shape is drawn when a listed layer is visible; on
    /// paper (`print`) when a listed layer has `Print` not 0, whether it is visible or not.
    pub(crate) fn verdict(&self, members: &[u32], print: bool) -> Verdict {
        let rows: Vec<&LayerRow> = members.iter().filter_map(|ix| self.rows.get(ix)).collect();
        // Nothing that names a row is on no layer: drawn, with its own colours.
        if rows.is_empty() {
            return Verdict::PLAIN;
        }
        let drawn = rows.iter().any(|r| if print { r.print } else { r.visible });
        // Exactly one layer, and it has a colour.
        let colour = match rows.as_slice() {
            [only] if drawn => only.colour,
            _ => None,
        };
        Verdict { drawn, colour }
    }
}

/// The row numbers a `LayerMember` string lists: split on semicolons, trimmed, empty pieces dropped, a piece that is
/// not digits dropped.
pub(crate) fn parse_members(text: &str) -> Vec<u32> {
    text.split(';').filter_map(|piece| piece.trim().parse::<u32>().ok()).take(1_024).collect()
}

/// The layers of the shape at `at`, as row numbers of the table of its page. A list found on the master is carried through the master's
/// table to the names, and the names to the page's table; a list of the shape's own is taken as it is.
pub(crate) fn members_of(cx: &Cx<'_, '_>, at: &SheetRef, page: &LayerTable, masters: &mut HashMap<u32, LayerTable>) -> Vec<u32> {
    let addr = CellAddr::direct("LayerMember");
    let Some(found) = cx.res.resolve(at, &addr) else {
        return Vec::new();
    };
    let Some(text) = cx.text_at(at, &addr) else {
        return Vec::new();
    };
    let listed = parse_members(&text);
    if listed.is_empty() {
        return listed;
    }
    match &found.value.source {
        Source::Master(master_shape) => {
            let diacraft_doc::Owner::Master(id) = master_shape.owner else {
                return listed;
            };
            let table = masters.entry(id).or_insert_with(|| LayerTable::read(cx, &SheetRef::Master(id)));
            listed
                .iter()
                .filter_map(|ix| table.rows.get(ix))
                .filter_map(|row| row.name_univ.as_deref())
                .filter_map(|name| page.by_name(name))
                .collect()
        }
        _ => listed,
    }
}
