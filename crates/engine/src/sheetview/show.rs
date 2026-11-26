//! Which display groups a sheet shows (design 5.4, settled).
//!
//! | Sheet | Shows |
//! |---|---|
//! | A shape | Every display group of a shape, but **1-D Endpoints** only on a one-dimensional shape (its four end cells are there, own or inherited), **Alignment** only when one of its six cells is there, **Group Properties** only on a group, **Foreign Image Info** only on a foreign object, **Text Transform** only when one of its seven cells is there and **Layer Membership** only when the shape holds it. Glue Info shows on a 2-D shape too and Image Properties on every shape (a project rule) |
//! | A page or a master's page sheet | Page Properties, Page Layout and Print Properties, and Ruler & Grid on a page's sheet only; every other group the sheet holds a cell of |
//! | The document | Document Properties; every group it holds a cell of |
//! | A style | Style Properties and Group Properties; every group the style holds a cell of |
//!
//! "Holds a cell of" is the sheet's **own** cells, found by one look at each of them; a cell of a master or a style that a shape inherits counts only for the three groups
//! whose rule says "on the shape or inherited" (Alignment, Text Transform, Layer Membership), and for the one-dimensional shape. Those are the **probes** ([`Counts::probes`](super::Counts)):
//! at most 18 cells looked up through the resolver whatever the sheet holds, so that listing the sections costs no cell of the rows.
//!
//! `all: true` shows the groups of the sheet's own kind that these rules leave out (a 1-D shape's end points on a box, Group Properties on a plain shape); a head of another
//! kind of sheet (Page Properties on a shape) is shown only where the sheet holds one of its cells.

use std::collections::HashSet;

use diacraft_doc::{CellAddr, Resolver, ShapeType, Sheet, SheetKind, SheetRef, display_group};

use super::Counts;

/// What decides which display groups a sheet shows.
#[derive(Debug)]
pub(crate) struct Facts {
    pub kind: SheetKind,
    /// A master's page sheet (a page's has Ruler & Grid, a master's does not).
    pub master_page: bool,
    pub group: bool,
    pub foreign: bool,
    pub one_d: bool,
    pub alignment: bool,
    pub text_transform: bool,
    pub layer_member: bool,
    /// The display groups the sheet's own cells belong to.
    pub held: HashSet<&'static str>,
}

/// The cells of Alignment, of Text Transform and of Layer Membership: the three groups a shape shows when it has one, own or inherited.
const ALIGNMENT: [&str; 6] = ["AlignBottom", "AlignCenter", "AlignLeft", "AlignMiddle", "AlignRight", "AlignTop"];
const TEXT_TRANSFORM: [&str; 7] = ["TxtAngle", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtPinX", "TxtPinY", "TxtWidth"];
const END_POINTS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

impl Facts {
    /// The facts of `sheet`, whose own sheet is `local`. A shape's facts take at most 18 probes through the resolver.
    pub(crate) fn of(res: &Resolver<'_>, sheet: &SheetRef, local: &Sheet, counts: &mut Counts) -> Facts {
        let kind = SheetKind::of(sheet);
        let held: HashSet<&'static str> = local.cells.keys().filter_map(|name| display_group(name.as_str())).collect();
        let mut facts = Facts {
            kind,
            master_page: matches!(sheet, SheetRef::Master(_)),
            group: false,
            foreign: false,
            one_d: false,
            alignment: false,
            text_transform: false,
            layer_member: false,
            held,
        };
        if let SheetRef::Shape(at) = sheet {
            if let Some(shape) = res.shape(at) {
                facts.group = shape.kind == ShapeType::Group;
                facts.foreign = shape.kind == ShapeType::Foreign || shape.foreign.is_some();
            }
            let mut probe = |name: &str| {
                counts.probes = counts.probes.saturating_add(1);
                res.resolve(sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some())
            };
            facts.one_d = END_POINTS.iter().all(|name| probe(name));
            facts.alignment = ALIGNMENT.iter().any(|name| probe(name));
            facts.text_transform = TEXT_TRANSFORM.iter().any(|name| probe(name));
            facts.layer_member = probe("LayerMember");
        }
        facts
    }
}

/// Whether the display group `label` shows on the sheet the facts describe.
pub(crate) fn group_shown(label: &str, f: &Facts, all: bool) -> bool {
    let held = f.held.contains(label);
    match label {
        "Document Properties" => f.kind == SheetKind::Document || held,
        "Style Properties" => f.kind == SheetKind::Style || held,
        "Page Properties" | "Page Layout" | "Print Properties" => f.kind == SheetKind::Page || held,
        "Ruler & Grid" => (f.kind == SheetKind::Page && !f.master_page) || held,
        _ => match f.kind {
            SheetKind::Shape => match label {
                "1-D Endpoints" => f.one_d || all,
                "Alignment" => f.alignment || all,
                "Group Properties" => f.group || all,
                "Foreign Image Info" => f.foreign || all,
                "Text Transform" => f.text_transform || all,
                "Layer Membership" => f.layer_member || all,
                _ => true,
            },
            SheetKind::Style if label == "Group Properties" => true,
            SheetKind::Page | SheetKind::Document | SheetKind::Style => held || all,
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn facts(kind: SheetKind) -> Facts {
        Facts {
            kind,
            master_page: false,
            group: false,
            foreign: false,
            one_d: false,
            alignment: false,
            text_transform: false,
            layer_member: false,
            held: HashSet::new(),
        }
    }

    #[test]
    fn a_shape_shows_its_groups_but_the_ones_its_kind_and_cells_leave_out() {
        let plain = facts(SheetKind::Shape);
        for shown in ["Shape Transform", "Protection", "Glue Info", "Image Properties", "Theme Properties"] {
            assert!(group_shown(shown, &plain, false), "{shown}");
        }
        for hidden in [
            "1-D Endpoints",
            "Alignment",
            "Group Properties",
            "Foreign Image Info",
            "Text Transform",
            "Layer Membership",
            "Page Properties",
            "Document Properties",
        ] {
            assert!(!group_shown(hidden, &plain, false), "{hidden}");
        }
        assert!(group_shown("Group Properties", &Facts { group: true, ..facts(SheetKind::Shape) }, false));
        assert!(group_shown("Group Properties", &plain, true), "all shows what the rules leave out");
        assert!(!group_shown("Page Properties", &plain, true), "but not another kind's head");
    }

    #[test]
    fn a_style_a_page_and_the_document_show_their_head_and_what_they_hold() {
        let mut style = facts(SheetKind::Style);
        assert!(group_shown("Style Properties", &style, false) && group_shown("Group Properties", &style, false));
        assert!(!group_shown("Line Format", &style, false));
        style.held.insert("Line Format");
        assert!(group_shown("Line Format", &style, false));
        let page = facts(SheetKind::Page);
        assert!(group_shown("Ruler & Grid", &page, false) && group_shown("Print Properties", &page, false));
        let master_page = Facts { master_page: true, ..facts(SheetKind::Page) };
        assert!(!group_shown("Ruler & Grid", &master_page, false) && group_shown("Page Layout", &master_page, false));
        assert!(!group_shown("Shape Transform", &page, false) && group_shown("Shape Transform", &page, true));
        assert!(group_shown("Document Properties", &facts(SheetKind::Document), false));
        assert!(!group_shown("Document Properties", &page, false));
    }
}
