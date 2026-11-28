//! Bug 5 of the launch pictures: **a shape that is a stack of pieces sharing edges is not rounded piece by piece by a theme.**
//!
//! A theme with rounded corners (Tidepool, Plumline) gives every shape that follows its `Theme` style a `Rounding` through `THEMEVAL` (each corner of
//! an outline between two straight sides is an arc of that radius, for the stroke and the fill alike). A class is seven pieces that stand one under the next, and each piece rounded all four
//! of its corners: a notch in every join, and white wedges where a keyword's fill meets its name's. The pieces now hold `Rounding` 0 themselves (a cell of the shape outranks its style), so a
//! stack reads as one box with square corners under every theme: the model has no `Rounding` for the outer corners alone, so the stack is square, and the spec's rule for one shape
//! (a rectangle is rounded) is unchanged.
//!
//! * Every piece of the seven masters of compartments reads a `Rounding` of its own, 0, under each theme and each variant, and with no theme.
//! * The picture of each stack under a theme with rounded corners is the picture of the same stack with every `Rounding` written 0: the theme rounds nothing of it.
//! * The same for the lanes of a pool and of a cross-functional flowchart, which also stand one under the next.
//! * The same for the UML package (a tab on a body, one shape of two paths).
//! * A control: a rectangle under the same theme is still rounded (the comparison sees rounding where there is some).

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef};
use diacraft_engine::Session;
use diacraft_render::{Renderer, View};
use serde_json::json;

use crate::edit::doc;
use crate::{blank_session, run};

/// The masters made of compartments: UML Class's five and Crow's Foot's two.
const STACKS: [&str; 7] = [
    "uml-class.class",
    "uml-class.interface",
    "uml-class.enumeration",
    "uml-class.data-type",
    "uml-class.object",
    "crows-foot.entity",
    "crows-foot.entity-with-keys",
];

/// The themes whose corners are rounded (`theme/catalogue.rs`: the looks `SOFT` and `ROUNDER`).
const ROUNDED: [&str; 2] = ["tidepool", "plumline"];

/// The `ID`s of every shape of page 1, at any depth, in document order.
fn all_ids(d: &Document) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = d.page(1).unwrap().shapes.iter().collect();
    stack.reverse();
    while let Some(shape) = stack.pop() {
        out.push(shape.id);
        stack.extend(shape.children.iter().rev());
    }
    out
}

/// The members of the instance `id`: its descendants.
fn members_of(d: &Document, id: u32) -> Vec<u32> {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<_> = d.shape(&at).unwrap().children.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        out.push(shape.id);
        stack.extend(shape.children.iter().rev());
    }
    out
}

/// What the shape `id` of page 1 finds for a cell, through its master and its styles: the number of a cell it holds itself, or `None` for the theme's marker, which is no number.
fn found_number(d: &Document, id: u32, cell: &str) -> Option<f64> {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// A blank drawing with the master dropped on page 1, and the theme (and its variant) applied when one is named.
fn scene(master: &str, theme: Option<(&str, u8)>) -> (Session, u32) {
    let mut s = blank_session();
    let dropped = run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": 4.0, "y": 4.0}));
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    if let Some((theme, variant)) = theme {
        run(&mut s, "theme.apply", json!({"theme": theme, "variant": variant}));
    }
    (s, id)
}

/// Writes a `Rounding` of 0 into every shape of page 1: what a drawing holds when no theme's rounding can reach a piece.
fn square_everything(s: &mut Session) {
    for id in all_ids(doc(s)) {
        run(s, "sheet.set_cell", json!({"page": 1, "id": id, "cell": "Rounding", "formula": "0"}));
    }
}

/// Page 1 drawn at 100 pixels to the inch on white paper: its size and its pixels.
fn picture(s: &Session) -> (u32, u32, Vec<u8>) {
    let d = doc(s);
    let view = View::fit_page(d, 1, 100.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(d, 1, &view);
    // A face the machine lacks is drawn in another (a stand-in), the same in both pictures of a pair: it is not what is compared.
    assert!(frame.skipped.iter().all(|s| format!("{:?}", s.kind) == "FaceStandIn"), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

/// How many pixels of the two pictures differ.
fn differing(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>)) -> usize {
    assert_eq!((a.0, a.1), (b.0, b.1), "the pictures are the same size");
    a.2.iter().zip(b.2.iter()).filter(|(x, y)| x != y).count()
}

#[test]
fn every_piece_of_a_stack_holds_a_rounding_of_its_own_under_every_theme_and_variant_and_with_none() {
    let listed = run(&mut blank_session(), "theme.list", json!({}));
    let themes: Vec<String> = listed["themes"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
    assert!(themes.iter().any(|t| t == "tidepool") && themes.iter().any(|t| t == "plumline"), "the catalogue's themes: {themes:?}");
    for master in STACKS {
        let (none, id) = scene(master, None);
        let members = members_of(doc(&none), id);
        assert!(members.len() >= 2, "{master}: a stack of at least two pieces, {members:?}");
        for member in &members {
            assert_eq!(found_number(doc(&none), *member, "Rounding"), Some(0.0), "{master}: piece {member} with no theme");
        }
        for theme in &themes {
            for variant in 0..4_u8 {
                let (s, id) = scene(master, Some((theme, variant)));
                for member in members_of(doc(&s), id) {
                    assert_eq!(
                        found_number(doc(&s), member, "Rounding"),
                        Some(0.0),
                        "{master} under {theme} (variant {variant}): piece {member} leaves its rounding to the theme"
                    );
                }
            }
        }
    }
}

#[test]
fn a_stack_under_a_theme_with_rounded_corners_is_drawn_as_it_is_with_every_rounding_written_0() {
    for theme in ROUNDED {
        for master in STACKS {
            let (mut s, _) = scene(master, Some((theme, 0)));
            let as_is = picture(&s);
            square_everything(&mut s);
            let squared = picture(&s);
            assert!(as_is.2.iter().any(|v| *v != 255), "{master} under {theme}: the picture is not blank");
            let off = differing(&as_is, &squared);
            assert_eq!(off, 0, "{master} under {theme}: {off} values of the picture are the theme's rounding of a piece of the stack");
        }
    }
}

#[test]
fn a_rectangle_under_a_theme_with_rounded_corners_is_still_rounded() {
    // The control of the comparison above: a shape of one piece follows the theme's rounding, and the picture shows it.
    for theme in ROUNDED {
        let (mut s, _) = scene("rectangle", Some((theme, 0)));
        let as_is = picture(&s);
        square_everything(&mut s);
        let squared = picture(&s);
        assert!(differing(&as_is, &squared) > 0, "a rectangle under {theme} is rounded by the theme");
    }
}

#[test]
fn a_package_is_not_rounded_tab_by_body_by_a_theme() {
    // A package is one shape of two paths, a tab standing on a body: each path rounded all four of its corners and left a notch where the tab meets the body.
    for theme in ROUNDED {
        let (mut s, id) = scene("uml-class.package", Some((theme, 0)));
        assert_eq!(found_number(doc(&s), id, "Rounding"), Some(0.0), "the package under {theme} holds a rounding of its own");
        let as_is = picture(&s);
        square_everything(&mut s);
        let off = differing(&as_is, &picture(&s));
        assert_eq!(off, 0, "the package under {theme}: {off} values of the picture are the theme's rounding of its tab and its body");
    }
}

/// What makes a drawing of lanes, with a theme or none.
type Maker = fn(Option<&str>) -> Session;

/// The lanes of a BPMN pool: a pool dropped, and two more lanes under its first.
fn pool_session(theme: Option<&str>) -> Session {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "bpmn.pool", "stencil": "bpmn", "page": 1, "x": 5.0, "y": 6.0}));
    run(&mut s, "shape.drop", json!({"master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.0, "y": 6.2}));
    run(&mut s, "shape.drop", json!({"master": "bpmn.lane", "stencil": "bpmn", "page": 1, "x": 5.0, "y": 5.2}));
    if let Some(theme) = theme {
        run(&mut s, "theme.apply", json!({"theme": theme}));
    }
    s
}

/// The cross-functional flowchart template: three lanes under three phases.
fn flowchart_session(theme: Option<&str>) -> Session {
    let mut s = blank_session();
    run(&mut s, "file.new", json!({"template": "cross_functional_flowchart"}));
    if let Some(theme) = theme {
        run(&mut s, "theme.apply", json!({"theme": theme}));
    }
    s
}

#[test]
fn the_lanes_of_a_pool_and_of_a_cross_functional_flowchart_are_not_rounded_one_by_one_by_a_theme() {
    let makers: [(&str, Maker); 2] = [("a pool", pool_session), ("a cross-functional flowchart", flowchart_session)];
    for theme in ROUNDED {
        for (what, make) in makers {
            let mut s = make(Some(theme));
            let ids = all_ids(doc(&s));
            assert!(ids.len() >= 4, "{what}: a frame, its lists and lanes, {ids:?}");
            let as_is = picture(&s);
            square_everything(&mut s);
            let squared = picture(&s);
            assert!(as_is.2.iter().any(|v| *v != 255), "{what} under {theme}: the picture is not blank");
            let off = differing(&as_is, &squared);
            assert_eq!(off, 0, "{what} under {theme}: {off} values of the picture are the theme's rounding of a part of the structure");
        }
    }
}

#[test]
fn a_stack_with_no_theme_is_the_picture_it_was() {
    // No theme: the root style's `Rounding` is 0 already, so a cell of 0 in each piece changes nothing.
    for master in STACKS {
        let (mut s, _) = scene(master, None);
        let as_is = picture(&s);
        square_everything(&mut s);
        assert_eq!(differing(&as_is, &picture(&s)), 0, "{master}: with no theme");
    }
}
