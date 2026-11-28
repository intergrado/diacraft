//! Chen's Database Notation (`chen`, GUID series 21): eleven masters drawn in code (section 6.5 of the second stencil batch's design, "Stencils: UML, BPMN, C4, network and database",
//! and and DD15 and DD16 of the batch's notation spec, which win where they differ).
//!
//! **Original figures.** Entity sets are rectangles, relationships diamonds and attributes ovals, each a plain figure of the box. A second outline inside the first (a weak entity's
//! rectangle, an identifying relationship's diamond, a multivalued attribute's oval) is a stroke of its own, 0.06 inch inside the outline (a corner radius that gives way on a small box, so that it
//! never turns inside out). The names are the notation's and the reference app's list of them, and nothing is traced, measured or copied from a standard, an application or a sample file.
//!
//! * **The order** is the Shapes window's, and the Quick Shapes are the first four that are no line: Entity, Relationship, Attribute and Key Attribute (the design's table lists the weak entity and
//!   the identifying relationship beside theirs; the window puts the four most used first).
//! * **The figures.** Entity, Weak Entity (a second rectangle), Associative Entity, Relationship, Identifying Relationship (a second diamond), Attribute, Key Attribute (its text underlined),
//!   Multivalued Attribute (a second oval) and Derived Attribute (a **dotted** oval, DD15, where the design said dashed). A key attribute's underline is the style bit of its first `Character`
//!   row, so what a person types takes it.
//! * **The Associative Entity is ours.** The reference app's stencil has one and no source read gives its figure (DD16), so it is drawn as it is often drawn: the entity's rectangle with a
//!   diamond inscribed in it, the diamond's vertices at the middles of the rectangle's sides, and its text inside the diamond. A project rule, with a check-list row.
//! * **The two lines.** Link joins an attribute, an entity and a relationship: straight, solid, no ends, its label (1, N or M) typed on it. Total Participation is the same with a **double line**
//!   (`CompoundType` 1) of 3 points, so that it stays a routed connector, and the renderer draws the two bands.
//! * **Not built.** The arrow of a key constraint and a thick line (a Link takes a line end from the Format Shape pane), and the multiplicity the reference app's relationship
//!   connector can show on its menu.

use super::kit::{self, GRID3, RING8, TIPS4};
use super::{CharRow, Co, ConnectorLook, Dress, Flag, LineEnd, MasterSpec, More, Outline, PathSpec, Route, Seg, TextBox, fh, fr, fw};

#[cfg(test)]
mod tests;

/// The series of the stencil's masters in their GUIDs (`D1AC0015-...`).
const SERIES: u8 = 21;

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);
/// How far inside the outline a second outline lies: 0.06 inch, or a quarter of the shorter side on a box smaller than a quarter of an inch (the corner radius's rule), so that it never turns
/// inside out.
const GAP: Co = fr(0.06, 1.0);
/// The far side of a second outline: the width and the height less the gap.
const IN_W: Co = W.plus(fr(0.06, -1.0));
const IN_H: Co = H.plus(fr(0.06, -1.0));

/// The text box of a diamond: three fifths as wide and as tall as it, in the middle. A line of text is as wide as the diamond is there (a line 0.17 inch tall has a chord of four fifths of the
/// width), and two short lines fit, so a label wraps at its words and not in the middle of one.
const DIAMOND_TEXT: TextBox = TextBox { x: 0.2, y: 0.2, w: 0.6, h: 0.6 };
/// The text box of the identifying relationship's inner diamond (0.83 of the outer one about the centre): half as wide and two fifths as tall.
const INNER_DIAMOND_TEXT: TextBox = TextBox { x: 0.25, y: 0.3, w: 0.5, h: 0.4 };

/// A path of straight sides, no show flag and no line flag.
const fn path(fill: bool, segs: &'static [Seg]) -> PathSpec {
    PathSpec { fill, segs, no_show: Flag::Off, no_line: Flag::Off }
}

/// The rectangle of an entity: the whole box.
const BOX: [Seg; 5] = [Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)];
/// The weak entity's second rectangle, inside the first.
const INNER_BOX: [Seg; 5] = [Seg::Move(GAP, GAP), Seg::Line(IN_W, GAP), Seg::Line(IN_W, IN_H), Seg::Line(GAP, IN_H), Seg::Line(GAP, GAP)];
/// A diamond that fills the box.
const DIAMOND: [Seg; 5] = [Seg::Move(fw(0.5), Z), Seg::Line(W, fh(0.5)), Seg::Line(fw(0.5), H), Seg::Line(Z, fh(0.5)), Seg::Line(fw(0.5), Z)];
/// The same diamond as 0.83 of itself about the centre: the identifying relationship's second diamond.
const INNER_DIAMOND: [Seg; 5] = [
    Seg::Move(fw(0.5), fh(0.085)),
    Seg::Line(fw(0.915), fh(0.5)),
    Seg::Line(fw(0.5), fh(0.915)),
    Seg::Line(fw(0.085), fh(0.5)),
    Seg::Line(fw(0.5), fh(0.085)),
];
/// The ellipse that fills the box.
const OVAL: [Seg; 1] = [Seg::Ellipse];
/// The multivalued attribute's second oval: two half-ellipses, the gap inside the first.
const INNER_OVAL: [Seg; 3] = [
    Seg::Move(GAP, fh(0.5)),
    Seg::Half { to: (IN_W, fh(0.5)), via: (fw(0.5), IN_H), rx: fw(0.5).plus(fr(0.06, -1.0)), ry: fh(0.5).plus(fr(0.06, -1.0)) },
    Seg::Half { to: (GAP, fh(0.5)), via: (fw(0.5), GAP), rx: fw(0.5).plus(fr(0.06, -1.0)), ry: fh(0.5).plus(fr(0.06, -1.0)) },
];

static WEAK_ENTITY: [PathSpec; 2] = [path(true, &BOX), path(false, &INNER_BOX)];
/// The associative entity's rectangle and the diamond inscribed in it.
static ASSOCIATIVE_ENTITY: [PathSpec; 2] = [path(true, &BOX), path(false, &DIAMOND)];
static IDENTIFYING_RELATIONSHIP: [PathSpec; 2] = [path(true, &DIAMOND), path(false, &INNER_DIAMOND)];
static MULTIVALUED_ATTRIBUTE: [PathSpec; 2] = [path(true, &OVAL), path(false, &INNER_OVAL)];

/// The key attribute's text is underlined: bit 4 of the style of its first `Character` row, at the size the style gives (12 points), so that what is typed takes it. People join it (`ObjType` 1).
static KEY_DRESS: Dress = Dress { chars: &[CharRow { size_pt: 12.0, style: 4, color: None }], obj_type: Some(1), ..Dress::PLACEABLE };
static KEY_MORE: More = More { dress: Some(&KEY_DRESS), hidden: false, brings: &[] };
/// The derived attribute's line is dotted (pattern 3): its own cell, a formula of no cell, which a format a person applies replaces as it replaces any.
static DERIVED_DRESS: Dress = Dress { line_pattern: Some(Flag::Formula("3")), ..Dress::PLACEABLE };
static DERIVED_MORE: More = More { dress: Some(&DERIVED_DRESS), hidden: false, brings: &[] };

/// The link: straight, solid, with no ends.
const LINK: ConnectorLook = ConnectorLook { route: Route::Straight, ..ConnectorLook::plain(LineEnd::None, LineEnd::None) };
/// The double line of total participation: `CompoundType` 1 at 3 points, so that each band is a point wide with a point between them.
const TOTAL_PARTICIPATION: ConnectorLook =
    ConnectorLook { route: Route::Straight, weight_pt: Some(3.0), compound: Some(1), ..ConnectorLook::plain(LineEnd::None, LineEnd::None) };

pub(super) static MASTERS: [MasterSpec; 11] = [
    MasterSpec {
        identity: (SERIES, 1),
        id: "chen.entity",
        name: "Entity",
        width: 1.4,
        height: 0.7,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &GRID3,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 2),
        id: "chen.relationship",
        name: "Relationship",
        width: 1.4,
        height: 0.8,
        outline: Outline::Code { build: kit::diamond, text: DIAMOND_TEXT },
        points: &TIPS4,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 3),
        id: "chen.attribute",
        name: "Attribute",
        width: 1.2,
        height: 0.6,
        outline: Outline::Ellipse,
        points: &RING8,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 4),
        id: "chen.key-attribute",
        name: "Key Attribute",
        width: 1.2,
        height: 0.6,
        outline: Outline::Ellipse,
        points: &RING8,
        more: &KEY_MORE,
    },
    MasterSpec {
        identity: (SERIES, 5),
        id: "chen.weak-entity",
        name: "Weak Entity",
        width: 1.4,
        height: 0.7,
        outline: Outline::Paths { paths: &WEAK_ENTITY, text: TextBox::FULL },
        points: &GRID3,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 6),
        id: "chen.associative-entity",
        name: "Associative Entity",
        width: 2.0,
        height: 1.0,
        outline: Outline::Paths { paths: &ASSOCIATIVE_ENTITY, text: DIAMOND_TEXT },
        points: &GRID3,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 7),
        id: "chen.identifying-relationship",
        name: "Identifying Relationship",
        width: 1.4,
        height: 0.8,
        outline: Outline::Paths { paths: &IDENTIFYING_RELATIONSHIP, text: INNER_DIAMOND_TEXT },
        points: &TIPS4,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 8),
        id: "chen.multivalued-attribute",
        name: "Multivalued Attribute",
        width: 1.2,
        height: 0.6,
        outline: Outline::Paths { paths: &MULTIVALUED_ATTRIBUTE, text: TextBox::FULL },
        points: &RING8,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 9),
        id: "chen.derived-attribute",
        name: "Derived Attribute",
        width: 1.2,
        height: 0.6,
        outline: Outline::Ellipse,
        points: &RING8,
        more: &DERIVED_MORE,
    },
    MasterSpec {
        identity: (SERIES, 10),
        id: "chen.link",
        name: "Link",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(LINK),
        points: &[],
        more: &More::NONE,
    },
    MasterSpec {
        identity: (SERIES, 11),
        id: "chen.total-participation",
        name: "Total Participation",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(TOTAL_PARTICIPATION),
        points: &[],
        more: &More::NONE,
    },
];

/// The search words of the master with this id, space separated, or `""`. Plain English: what the figure is called elsewhere and what it is used for, and the names the
/// reference app's list gives for the notation (C78: Relationship Connector, Primary Key Attribute, Multi-valued Attribute) that its purpose serves.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "chen.entity" => "entity set table thing object",
        "chen.relationship" => "relationship diamond association relation",
        "chen.attribute" => "attribute oval ellipse property column",
        "chen.key-attribute" => "primary key attribute underlined identifier unique oval",
        "chen.weak-entity" => "weak entity double border dependent owner",
        "chen.associative-entity" => "associative entity many to many gerund intersection junction diamond",
        "chen.identifying-relationship" => "identifying relationship double diamond weak owner",
        "chen.multivalued-attribute" => "multivalued multi valued attribute double oval several values",
        "chen.derived-attribute" => "derived attribute dotted oval computed calculated",
        "chen.link" => "relationship connector link line join cardinality",
        "chen.total-participation" => "total participation double line mandatory every connector",
        _ => "",
    }
}
