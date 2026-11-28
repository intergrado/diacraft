//! UML Class (`uml-class`, GUID series 9): fifteen masters of the class diagram, based on UML 2.5.1.
//! The figures are the project's own, drawn in code from the notation's generic shapes: a classifier is a rectangle in compartments, a package a tab over a body, a note a dog-eared
//! box, and a relationship a line with the end its kind has. Nothing of the standard's figures, of the reference app's or of another application's is traced or copied.
//!
//! **The class and its kin are group masters** (`group_master`): the compartments are members, each with its own text and look, stacked from the top by rank and each as tall as its text (the group is as
//! tall as they are together and holds `LockHeight`, so a resize changes the width only and typing a line makes the class taller). **The name is the member last in the table**, so typing on the selected
//! class goes to it, though it is the compartment at the top (rank 0 is the top and the order of the table is not the stack's). A keyword (`«interface»`) is a compartment of its
//! own above the name, in plain type and centred, and the name is bold and centred; a list (attributes, operations, literals, slots) is plain and starts at the top left of its compartment.
//! The compartments draw each a rectangle, or only the edges that are the classifier's own: the keyword and the name that follows it share a compartment, so no line stands between them.
//!
//! **Abstract** is a Shape Data field of the class (`Abstract`, a boolean): the name's `Style` is a formula of it, `IF(Sheet.5!Prop.Abstract,3,1)`, bold and italic when it is true.
//! **A package is a container** (its master has the container role): a class dropped inside it joins it, and a move of the package takes its members. Its tab is a quarter inch tall over the left
//! 0.4 of the box and its name is typed at the top left, which is in the tab whatever the size of the package (DD1 keeps the tab always). **The lines** carry the ends and pattern of the design's
//! section 6.1 as their master's look: association (none), directed association (an open head, 3), aggregation (a hollow diamond, 22, at the whole's end, the begin), composition (the
//! filled diamond of `line_ends`, brought in by the connector, at the begin), generalization (a hollow triangle, 16), realization (dashed, 16), dependency (dashed, 3) and the note link (dashed). A relationship is
//! routed at right angles, as the class diagram's are (ours); the note link is straight. **A line's label** is its own text, typed.
//!
//! **Ours** (nothing in the notation says): the sizes, the default names (`Class`, `+ attribute : Type`, `+ operation()`), the 0.4 inch of the compartments' least height, the 0.25 inch tab, the
//! search words, and the order of the masters, which puts the Quick Shapes (Class, Interface, Enumeration, Package) first. Only the class has `Abstract`. **Not built:** the compartment of receptions
//! Members as rows of a list (the compartments follow their text, and a member is a line of it), a stereotype that is a separate property (a stereotype is text), and multiplicity and
//! role names at the ends of a line (type them in the label).

use super::container::Side;
use super::dress::{CellSet, RowCell};
use super::group_master::{MemberFrame, MemberSpec};
use super::kit::{self, GRID3, SIDES};
use super::{
    CharRow, Co, ConnectorLook, Dress, FieldDef, FieldKind, Flag, LineEnd, MasterSpec, More, Outline, OwnedPath, PathSpec, Point, Role, Route, Seg,
    TextBox, TextSpec, fh, fr, fw, pc, pt,
};

/// The GUID series of this stencil's masters (design section 4.1).
const SERIES: u8 = 9;

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

/// A closed rectangle that fills the box.
const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
const RECT_SEGS: [Seg; 5] = [Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)];

// ---- the compartments -----------------------------------------------------------------------------------------------------------------

/// The text of a list is left aligned: `HorzAlign` 0 of the first paragraph.
const LEFT: RowCell = RowCell { section: "Paragraph", ix: 0, cell: "HorzAlign", set: CellSet::Number(0.0) };

/// A name is bold, and bold and italic when the class holds `Abstract` (bit 1 of `Style` is bold, bit 2 italic).
const ABSTRACT_ITALIC: RowCell = RowCell { section: "Character", ix: 0, cell: "Style", set: CellSet::Formula("IF(Sheet.5!Prop.Abstract,3,1)") };

/// 10 point plain type: a list or a keyword's neighbour.
const LIST_CHARS: [CharRow; 1] = [CharRow { size_pt: 10.0, style: 0, color: None }];
/// 9 point plain type: a keyword.
const KEYWORD_CHARS: [CharRow; 1] = [CharRow { size_pt: 9.0, style: 0, color: None }];
/// 11 point bold type: a name.
const NAME_CHARS: [CharRow; 1] = [CharRow { size_pt: 11.0, style: 1, color: None }];
/// 11 point bold and underlined type: the name of an object.
const OBJECT_NAME_CHARS: [CharRow; 1] = [CharRow { size_pt: 11.0, style: 5, color: None }];

/// A list starts at the top of its compartment.
const AT_TOP: [(&str, f64); 1] = [("VerticalAlign", 0.0)];
/// A keyword has no margin under it and the name beneath has none over it, so that the two read as one name compartment.
const NO_MARGIN_BELOW: [(&str, f64); 1] = [("BottomMargin", 0.0)];
const NO_MARGIN_ABOVE: [(&str, f64); 1] = [("TopMargin", 0.0)];

const fn list(text: &'static str) -> Dress {
    Dress { text: TextSpec::Plain(text), chars: &LIST_CHARS, rows: &[LEFT], locks: &AT_TOP, ..Dress::NONE }
}

static ATTRIBUTES: Dress = list("+ attribute : Type");
static OPERATIONS: Dress = list("+ operation()");
static LITERALS: Dress = list("LITERAL");
static SLOTS: Dress = list("attribute = value");

static CLASS_NAME: Dress = Dress { text: TextSpec::Plain("Class"), chars: &NAME_CHARS, rows: &[ABSTRACT_ITALIC], ..Dress::NONE };
static INTERFACE_NAME: Dress = Dress { text: TextSpec::Plain("Interface"), chars: &NAME_CHARS, locks: &NO_MARGIN_ABOVE, ..Dress::NONE };
static ENUMERATION_NAME: Dress = Dress { text: TextSpec::Plain("Enumeration"), chars: &NAME_CHARS, locks: &NO_MARGIN_ABOVE, ..Dress::NONE };
static DATA_TYPE_NAME: Dress = Dress { text: TextSpec::Plain("DataType"), chars: &NAME_CHARS, locks: &NO_MARGIN_ABOVE, ..Dress::NONE };
static OBJECT_NAME: Dress = Dress { text: TextSpec::Plain("object : Class"), chars: &OBJECT_NAME_CHARS, ..Dress::NONE };

static INTERFACE_KEYWORD: Dress =
    Dress { text: TextSpec::Plain("\u{ab}interface\u{bb}"), chars: &KEYWORD_CHARS, locks: &NO_MARGIN_BELOW, ..Dress::NONE };
static ENUMERATION_KEYWORD: Dress =
    Dress { text: TextSpec::Plain("\u{ab}enumeration\u{bb}"), chars: &KEYWORD_CHARS, locks: &NO_MARGIN_BELOW, ..Dress::NONE };
static DATA_TYPE_KEYWORD: Dress =
    Dress { text: TextSpec::Plain("\u{ab}dataType\u{bb}"), chars: &KEYWORD_CHARS, locks: &NO_MARGIN_BELOW, ..Dress::NONE };

/// A compartment that is a whole rectangle.
const fn boxed(name: &'static str, frame: MemberFrame, dress: &'static Dress) -> MemberSpec {
    MemberSpec { name, frame, outline: Outline::Polygon(RECT), points: &[], dress: Some(dress) }
}

/// The top of a classifier whose name has a keyword over it: the fill with no line, and the top and the two sides as a line that stops there, so that no line stands between the keyword and the name.
const TOP_PATHS: [PathSpec; 2] = [
    PathSpec { fill: true, segs: &RECT_SEGS, no_show: Flag::Off, no_line: Flag::On },
    PathSpec { fill: false, segs: &[Seg::Move(Z, Z), Seg::Line(Z, H), Seg::Line(W, H), Seg::Line(W, Z)], no_show: Flag::Off, no_line: Flag::Off },
];

/// The name under a keyword: the fill with no line, and the two sides and the bottom as a line that stops there.
const NAME_UNDER_PATHS: [PathSpec; 2] = [
    PathSpec { fill: true, segs: &RECT_SEGS, no_show: Flag::Off, no_line: Flag::On },
    PathSpec { fill: false, segs: &[Seg::Move(Z, H), Seg::Line(Z, Z), Seg::Line(W, Z), Seg::Line(W, H)], no_show: Flag::Off, no_line: Flag::Off },
];

const fn keyword(name: &'static str, frame: MemberFrame, dress: &'static Dress) -> MemberSpec {
    MemberSpec { name, frame, outline: Outline::Paths { paths: &TOP_PATHS, text: TextBox::FULL }, points: &[], dress: Some(dress) }
}

const fn name_under(name: &'static str, frame: MemberFrame, dress: &'static Dress) -> MemberSpec {
    MemberSpec { name, frame, outline: Outline::Paths { paths: &NAME_UNDER_PATHS, text: TextBox::FULL }, points: &[], dress: Some(dress) }
}

// ---- the classifiers ------------------------------------------------------------------------------------------------------------------

/// The frames of the compartments of each classifier, from the top, with the least height of each in inches.
const CLASS_FRAMES: [MemberFrame; 3] = kit::compartments([0.4, 0.4, 0.4]);
const INTERFACE_FRAMES: [MemberFrame; 4] = kit::compartments([0.26, 0.3, 0.34, 0.34]);
const ENUMERATION_FRAMES: [MemberFrame; 3] = kit::compartments([0.26, 0.3, 0.4]);
const DATA_TYPE_FRAMES: [MemberFrame; 3] = kit::compartments([0.26, 0.3, 0.4]);
const OBJECT_FRAMES: [MemberFrame; 2] = kit::compartments([0.4, 0.4]);

// A table lists the name last (the member typing on the selected classifier goes to), and the rank in its frame puts it where it stands.
static CLASS_MEMBERS: [MemberSpec; 3] = [
    boxed("Operations", CLASS_FRAMES[2], &OPERATIONS),
    boxed("Attributes", CLASS_FRAMES[1], &ATTRIBUTES),
    boxed("Name", CLASS_FRAMES[0], &CLASS_NAME),
];
static INTERFACE_MEMBERS: [MemberSpec; 4] = [
    boxed("Operations", INTERFACE_FRAMES[3], &OPERATIONS),
    boxed("Attributes", INTERFACE_FRAMES[2], &ATTRIBUTES),
    keyword("Keyword", INTERFACE_FRAMES[0], &INTERFACE_KEYWORD),
    name_under("Name", INTERFACE_FRAMES[1], &INTERFACE_NAME),
];
static ENUMERATION_MEMBERS: [MemberSpec; 3] = [
    boxed("Literals", ENUMERATION_FRAMES[2], &LITERALS),
    keyword("Keyword", ENUMERATION_FRAMES[0], &ENUMERATION_KEYWORD),
    name_under("Name", ENUMERATION_FRAMES[1], &ENUMERATION_NAME),
];
static DATA_TYPE_MEMBERS: [MemberSpec; 3] = [
    boxed("Attributes", DATA_TYPE_FRAMES[2], &ATTRIBUTES),
    keyword("Keyword", DATA_TYPE_FRAMES[0], &DATA_TYPE_KEYWORD),
    name_under("Name", DATA_TYPE_FRAMES[1], &DATA_TYPE_NAME),
];
static OBJECT_MEMBERS: [MemberSpec; 2] = [boxed("Slots", OBJECT_FRAMES[1], &SLOTS), boxed("Name", OBJECT_FRAMES[0], &OBJECT_NAME)];

/// What the group of a classifier wears: the group is selected first, typing goes to its members, and a person joins it. A class has the field `Abstract`.
static CLASS_DRESS: Dress = Dress {
    data: &[FieldDef {
        row: "Abstract",
        label: "Abstract",
        kind: FieldKind::Boolean,
        value: "0",
        prompt: "Shows the name in italics when it is true",
    }],
    select_mode: Some(1),
    text_edit_target: Some(false),
    obj_type: Some(1),
    ..Dress::NONE
};
static CLASSIFIER_DRESS: Dress = Dress { select_mode: Some(1), text_edit_target: Some(false), obj_type: Some(1), ..Dress::NONE };
static CLASS_MORE: More = More { dress: Some(&CLASS_DRESS), hidden: false, brings: &[] };
static CLASSIFIER_MORE: More = More { dress: Some(&CLASSIFIER_DRESS), hidden: false, brings: &[] };

// ---- the package ----------------------------------------------------------------------------------------------------------------------

/// How tall the package's tab is, in inches.
const TAB_HEIGHT: f64 = 0.25;

fn owned(segs: Vec<Seg>) -> OwnedPath {
    OwnedPath { fill: true, segs, no_show: Flag::Off, no_line: Flag::Off }
}

/// The package: a body whose top edge is a tab's height under the box's top, and over the left 0.4 of it a tab, a rectangle that stands on the body's top edge.
fn package_figure() -> Vec<OwnedPath> {
    let body_top = body_top();
    let tab_right = fw(0.4);
    vec![
        owned(vec![Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, body_top), Seg::Line(Z, body_top), Seg::Line(Z, Z)]),
        owned(vec![Seg::Move(Z, body_top), Seg::Line(tab_right, body_top), Seg::Line(tab_right, H), Seg::Line(Z, H), Seg::Line(Z, body_top)]),
    ]
}

/// The body's top edge: the box's top less the tab's height. **The tab is a corner-radius term** (`MIN(0.25,MIN(Width,Height)*0.25)`: a quarter inch, less in a box smaller than an inch), and
/// not a fixed length: an `.odg` has no place for `Height - 0.25` and so wrote the outline as a rectangle for other programs, where it writes a term that calls a function as its value at the size written.
const fn body_top() -> Co {
    H.plus(fr(TAB_HEIGHT, -1.0))
}

/// A share `f` of the body's height, which is the box's less the tab's: `f * (Height - tab)`.
const fn body_share(f: f64) -> Co {
    Co { w: 0.0, h: f, min: 0.0, k: 0.0, rad: TAB_HEIGHT, rs: -f }
}

/// Twelve points on the package's outline, facing out: one on the tab's top, two on the body's top beside the tab, three on each of the other sides.
static PACKAGE_POINTS: [Point; 12] = [
    pt(0.2, 1.0, 0.0, 1.0),
    pc(fw(0.7), body_top(), 0.0, 1.0),
    pc(fw(0.9), body_top(), 0.0, 1.0),
    pt(0.0, 0.75, -1.0, 0.0),
    pt(0.0, 0.5, -1.0, 0.0),
    pt(0.0, 0.25, -1.0, 0.0),
    pt(0.25, 0.0, 0.0, -1.0),
    pt(0.5, 0.0, 0.0, -1.0),
    pt(0.75, 0.0, 0.0, -1.0),
    pc(W, body_share(0.25), 1.0, 0.0),
    pc(W, body_share(0.5), 1.0, 0.0),
    pc(W, body_share(0.75), 1.0, 0.0),
];

/// The package's name is typed at the top left of its box, which is in the tab: the text block is the whole box, flush left and starting at the top.
static PACKAGE_DRESS: Dress = Dress {
    text: TextSpec::Plain("Package"),
    chars: &NAME_CHARS,
    rows: &[LEFT],
    locks: &AT_TOP,
    obj_type: Some(1),
    no_rounding: true,
    role: Role::Container { heading: Side::Top, margin_in: 0.25 },
    ..Dress::NONE
};
static PACKAGE_MORE: More = More { dress: Some(&PACKAGE_DRESS), hidden: false, brings: &[] };

// ---- the note -------------------------------------------------------------------------------------------------------------------------

/// A note's text is plain, flush left and at the top, as a comment's is. (The Use Case stencil's note is this one.)
pub(super) static NOTE_DRESS: Dress =
    Dress { text: TextSpec::Plain("Note"), chars: &LIST_CHARS, rows: &[LEFT], locks: &AT_TOP, obj_type: Some(1), ..Dress::NONE };
pub(super) static NOTE_MORE: More = More { dress: Some(&NOTE_DRESS), hidden: false, brings: &[] };

// ---- the lines ------------------------------------------------------------------------------------------------------------------------

/// A relationship: a solid or dashed line at right angles with these ends.
const fn relationship(begin: LineEnd, end: LineEnd, pattern: u8) -> ConnectorLook {
    ConnectorLook { begin, end, pattern, guard_pattern: false, route: Route::RightAngle, weight_pt: None, compound: None }
}

/// A line to a note: dashed, no ends, straight.
const NOTE_LINK: ConnectorLook = ConnectorLook {
    begin: LineEnd::None,
    end: LineEnd::None,
    pattern: 2,
    guard_pattern: false,
    route: Route::Straight,
    weight_pt: None,
    compound: None,
};

/// The filled diamond of composition is a hidden master the connector brings into the drawing.
const FILLED_DIAMOND: LineEnd = LineEnd::Master { id: "line-end.filled-diamond", name: "Filled Diamond" };
static COMPOSITION_MORE: More = More { dress: None, hidden: false, brings: &["line-end.filled-diamond"] };

/// A one-dimensional master: 1.5 inches long.
const fn line(id: &'static str, name: &'static str, ordinal: u16, look: ConnectorLook, more: &'static More) -> MasterSpec {
    MasterSpec { id, name, width: 1.5, height: 0.0, identity: (SERIES, ordinal), outline: Outline::Connector(look), points: &[], more }
}

// ---- the masters ----------------------------------------------------------------------------------------------------------------------

/// A group master of compartments: its height is the least heights' sum, which the master's own build settles.
const fn classifier(
    id: &'static str,
    name: &'static str,
    ordinal: u16,
    (width, height): (f64, f64),
    members: &'static [MemberSpec],
    more: &'static More,
) -> MasterSpec {
    MasterSpec {
        id,
        name,
        width,
        height,
        identity: (SERIES, ordinal),
        outline: Outline::Group { members, text: TextBox::FULL },
        points: &GRID3,
        more,
    }
}

/// The masters in the order the Shapes window lists them: the first four that are no lines are the Quick Shapes.
pub(super) static MASTERS: [MasterSpec; 15] = [
    classifier("uml-class.class", "Class", 1, (1.6, 1.2), &CLASS_MEMBERS, &CLASS_MORE),
    classifier("uml-class.interface", "Interface", 2, (1.6, 1.24), &INTERFACE_MEMBERS, &CLASSIFIER_MORE),
    classifier("uml-class.enumeration", "Enumeration", 3, (1.6, 0.96), &ENUMERATION_MEMBERS, &CLASSIFIER_MORE),
    MasterSpec {
        id: "uml-class.package",
        name: "Package",
        width: 2.4,
        height: 1.6,
        identity: (SERIES, 4),
        outline: Outline::Code { build: package_figure, text: TextBox::FULL },
        points: &PACKAGE_POINTS,
        more: &PACKAGE_MORE,
    },
    classifier("uml-class.data-type", "Data Type", 5, (1.6, 0.96), &DATA_TYPE_MEMBERS, &CLASSIFIER_MORE),
    classifier("uml-class.object", "Object", 6, (1.6, 0.8), &OBJECT_MEMBERS, &CLASSIFIER_MORE),
    MasterSpec {
        id: "uml-class.note",
        name: "Note",
        width: 1.4,
        height: 0.8,
        identity: (SERIES, 7),
        outline: Outline::Code { build: kit::note, text: TextBox::FULL },
        points: &SIDES,
        more: &NOTE_MORE,
    },
    line("uml-class.association", "Association", 8, relationship(LineEnd::None, LineEnd::None, 1), &More::NONE),
    line("uml-class.directed-association", "Directed Association", 9, relationship(LineEnd::None, LineEnd::Number(3), 1), &More::NONE),
    line("uml-class.aggregation", "Aggregation", 10, relationship(LineEnd::Number(22), LineEnd::None, 1), &More::NONE),
    line("uml-class.composition", "Composition", 11, relationship(FILLED_DIAMOND, LineEnd::None, 1), &COMPOSITION_MORE),
    line("uml-class.generalization", "Generalization", 12, relationship(LineEnd::None, LineEnd::Number(16), 1), &More::NONE),
    line("uml-class.realization", "Realization", 13, relationship(LineEnd::None, LineEnd::Number(16), 2), &More::NONE),
    line("uml-class.dependency", "Dependency", 14, relationship(LineEnd::None, LineEnd::Number(3), 2), &More::NONE),
    line("uml-class.note-link", "Note Link", 15, NOTE_LINK, &More::NONE),
];

/// The search words of the master with this id, space separated, or `""`. The names a drawing of the reference app carries for these figures (the inventory's C67 and C75: Member,
/// Composite, Inheritance, Interface Realization, Package) are words of the master that serves the purpose.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "uml-class.class" => "classifier type entity member composite attribute operation method field object-oriented",
        "uml-class.interface" => "contract protocol api classifier realization",
        "uml-class.enumeration" => "enum literal values list constants",
        "uml-class.data-type" => "datatype primitive value type classifier",
        "uml-class.object" => "instance slot snapshot",
        "uml-class.package" => "namespace module folder subsystem group container expanded collapsed",
        "uml-class.note" => "comment annotation remark text",
        "uml-class.association" => "relationship link line connector",
        "uml-class.directed-association" => "navigable one-way relationship connector",
        "uml-class.aggregation" => "whole part shared diamond has relationship connector",
        "uml-class.composition" => "whole part owned filled diamond contains relationship connector",
        "uml-class.generalization" => "inheritance extends subclass superclass specialization is-a connector",
        "uml-class.realization" => "interface realization implements connector",
        "uml-class.dependency" => "uses depends client supplier relationship connector",
        "uml-class.note-link" => "comment annotation anchor connector",
        _ => "",
    }
}

#[cfg(test)]
mod dress_tests;
#[cfg(test)]
mod hostile_tests;
#[cfg(test)]
pub(crate) mod tests;
