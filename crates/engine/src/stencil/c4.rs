//! C4 Model (`c4`, GUID series 16): twelve masters for software architecture diagrams, drawn in code (the design's section 6.3).
//!
//! **Our own extra.** The C4 model prescribes no notation: it asks that a diagram say what every shape, colour, border and line means, and gives each element a name, a type, a technology
//! and a short description. The figures, the colours and the line are DiaCraft's own, drawn from plain geometry (a rounded box, a cylinder, a person made of a circle over a box), and no icon or mark of
//! any product is drawn. The model has no row in the parity check list. The notation pages are published under CC BY 4.0 (`docs/provenance.md`).
//!
//! * **The elements** (a Person, a Software System, a Container, a Component, a Database Container, a Queue Container and an Infrastructure Node) show three **fields of their Shape Data**, as the
//!   organization chart's positions show two: the name (bold, 11 points), the type line `[<Kind>: <Technology>]` (8 points, the colon and the technology left out when the technology is empty) and
//!   the description (9 points). A person and a software system have no technology: their type line is the kind alone. An element that carries an accent has an `External` truth value, which turns
//!   its fill grey.
//! * **The accents** follow the theme (`THEMEVAL("AccentColor", ...)` with our default, one base blue and one grey, design section 4.7) and have those defaults with no theme. Text on a dark fill is
//!   light; the Component is a tint of the blue and its text dark, until it is external.
//! * **The boundary and the deployment node are containers** (`Role::Container`): a shape dropped inside joins them and a move takes it along, and their outline is their master's. Their label sits at
//!   the bottom left, as the model's examples have it: a dress has no paragraph alignment, so it is a block at the bottom left whose text is centred in it.
//! * **The title and the key** are shapes no connector joins (`Role::Pointless`: no connection point). The title is text only; the key is a group of five sample boxes and its own title.
//! * **The relationship** is a line with an arrowhead at its end, dashed (a choice of ours, which the model allows), whose label is its description over its technology in brackets, on a white
//!   background so that the dashes do not run through the words.
//! * **The masters stand in the order of the Shapes window:** the Quick Shapes are the first four that are no line, the Person, the Software System, the Container and the Component.
//!
//! Every formula of a dress has no value in the table: the master's build settles it (`dress.rs`), so a cached value is what the formula gives at the master's default Shape Data.

use super::container::Side;
use super::group_master::{MemberFrame, MemberSpec};
use super::{
    CharRow, Co, ConnectorLook, Dress, FieldDef, FieldKind, FieldText, Flag, LineEnd, MasterSpec, More, Outline, OwnedPath, Point, Role, SIDES, Seg,
    TextBox, TextSpec, fh, fr, fw, kit, pc,
};

/// The series of this stencil's masters in their GUIDs (`MasterSpec::guid`).
const SERIES: u8 = 16;

// ---- colours (design section 4.7) ------------------------------------------------------------------------------------------------------

/// The base blue and the grey are ours; a theme's accent replaces the blue, and its dark colour the grey.
const BLUE: &str = "THEMEVAL(\"AccentColor\",RGB(58,127,192))";

/// An accent: the kind's own colour, or the grey of an external element.
macro_rules! accent {
    ($own:literal) => {
        concat!("IF(Prop.External,MSOTINT(THEMEVAL(\"Dark\",RGB(0,0,0)),45),", $own, ")")
    };
}

/// A person is the darkest shade of the blue, a software system a lighter one, a container the blue itself, and a component a tint of it.
const PERSON_OWN: &str = "SHADE(THEMEVAL(\"AccentColor\",RGB(58,127,192)),50)";
const SYSTEM_OWN: &str = "SHADE(THEMEVAL(\"AccentColor\",RGB(58,127,192)),25)";
const COMPONENT_OWN: &str = "MSOTINT(THEMEVAL(\"AccentColor\",RGB(58,127,192)),50)";
const PERSON_FILL: &str = accent!("SHADE(THEMEVAL(\"AccentColor\",RGB(58,127,192)),50)");
const SYSTEM_FILL: &str = accent!("SHADE(THEMEVAL(\"AccentColor\",RGB(58,127,192)),25)");
const CONTAINER_FILL: &str = accent!("THEMEVAL(\"AccentColor\",RGB(58,127,192))");
const COMPONENT_FILL: &str = accent!("MSOTINT(THEMEVAL(\"AccentColor\",RGB(58,127,192)),50)");
/// The deployment node is the lightest tint of the blue, so that what runs on it shows against it.
const NODE_FILL: &str = "MSOTINT(THEMEVAL(\"AccentColor\",RGB(58,127,192)),85)";
const GREY: &str = "MSOTINT(THEMEVAL(\"Dark\",RGB(0,0,0)),45)";

/// Text on a dark fill.
const LIGHT: &str = "THEMEVAL(\"Light\",RGB(255,255,255))";
/// Text on the Component's tint: dark, until the component is external and grey.
const INK_ON_TINT: &str = "IF(Prop.External,THEMEVAL(\"Light\",RGB(255,255,255)),THEMEVAL(\"Dark\",RGB(0,0,0)))";

// ---- text: the name, the type line and the description ---------------------------------------------------------------------------------

/// The type line of an element that has a technology: `[Container]` with none, `[Container: Rust]` with one.
macro_rules! typed {
    ($kind:literal) => {
        concat!("\"[", $kind, "\"&IF(LEN(Prop.Technology)>0,\": \"&Prop.Technology,\"\")&\"]\"")
    };
}

/// The name, the type line and the description, each in its own character format (rows 0, 1 and 2), with a line break after the first two.
const fn element_text(type_formula: &'static str, shown_type: &'static str) -> [FieldText; 3] {
    [
        FieldText { formula: "Prop.Name", shown: "Name", char_row: 0, line_after: true },
        FieldText { formula: type_formula, shown: shown_type, char_row: 1, line_after: true },
        FieldText { formula: "Prop.Description", shown: "Description", char_row: 2, line_after: false },
    ]
}

/// The name bold at 11 points, the type line at 8 and the description at 9, in `color` (the style's when `None`).
const fn element_chars(color: Option<&'static str>) -> [CharRow; 3] {
    [CharRow { size_pt: 11.0, style: 1, color }, CharRow { size_pt: 8.0, style: 0, color }, CharRow { size_pt: 9.0, style: 0, color }]
}

static PERSON_TEXT: [FieldText; 3] = element_text("\"[Person]\"", "[Person]");
static SYSTEM_TEXT: [FieldText; 3] = element_text("\"[Software System]\"", "[Software System]");
static CONTAINER_TEXT: [FieldText; 3] = element_text(typed!("Container"), "[Container]");
static COMPONENT_TEXT: [FieldText; 3] = element_text(typed!("Component"), "[Component]");
static INFRASTRUCTURE_TEXT: [FieldText; 3] = element_text(typed!("Infrastructure Node"), "[Infrastructure Node]");

static LIGHT_CHARS: [CharRow; 3] = element_chars(Some(LIGHT));
static TINT_CHARS: [CharRow; 3] = element_chars(Some(INK_ON_TINT));
static PLAIN_CHARS: [CharRow; 3] = element_chars(None);

// ---- Shape Data ------------------------------------------------------------------------------------------------------------------

const NAME: FieldDef = FieldDef { row: "Name", label: "Name", kind: FieldKind::String, value: "Name", prompt: "The element's name" };
const TECHNOLOGY: FieldDef =
    FieldDef { row: "Technology", label: "Technology", kind: FieldKind::String, value: "", prompt: "The technology it is built with, or runs on" };
const DESCRIPTION: FieldDef = FieldDef {
    row: "Description",
    label: "Description",
    kind: FieldKind::String,
    value: "Description",
    prompt: "A short description of its responsibilities",
};
const EXTERNAL: FieldDef = FieldDef {
    row: "External",
    label: "External",
    kind: FieldKind::Boolean,
    value: "0",
    prompt: "Whether it lies outside the scope of the diagram: it is drawn grey",
};

/// An element with a technology: the name, the technology, the description and whether it is external.
static WITH_TECHNOLOGY: [FieldDef; 4] = [NAME, TECHNOLOGY, DESCRIPTION, EXTERNAL];
/// A person or a software system: no technology.
static WITHOUT_TECHNOLOGY: [FieldDef; 3] = [NAME, DESCRIPTION, EXTERNAL];
/// An infrastructure node: a technology and no accent, so nothing to be external.
static INFRASTRUCTURE_DATA: [FieldDef; 3] = [NAME, TECHNOLOGY, DESCRIPTION];

// ---- figures ---------------------------------------------------------------------------------------------------------------------

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);
/// A quarter circle's midpoint is `1 - sqrt(0.5)` of the radius from its bounding corner.
const INSET: f64 = 0.292_893_218_813_452_4;

/// The corner of a person's body, in inches (it gives way on a small box).
const BODY_CORNER: f64 = 0.16;
/// Where the body's top edge is, as a share of the height: the head's circle stands over it.
const BODY_TOP: f64 = 0.70;
/// The radius of a person's head, as a share of the shorter side (0.2 inch on the default 1.4 by 1.5 inches): the head is as large as the box is small.
const HEAD: f64 = 0.14;

/// A length that is a share of the shorter side of the box.
const fn shorter(share: f64) -> Co {
    Co { w: 0.0, h: 0.0, min: share, k: 0.0, rad: 0.0, rs: 0.0 }
}

fn rounded_08() -> Vec<OwnedPath> {
    kit::rounded_box(0.08)
}

fn rounded_10() -> Vec<OwnedPath> {
    kit::rounded_box(0.10)
}

fn rounded_06() -> Vec<OwnedPath> {
    kit::rounded_box(0.06)
}

/// A person: a round head over a rounded body, two closed outlines with a little room between them (a head that dipped into the body would have its circle run across it, since a later section
/// does not hide an earlier line). The head is two half circles of the radius [`HEAD`] of the shorter side, standing on the top of the box, and the body is a rounded box under it.
fn person_figure() -> Vec<OwnedPath> {
    let (rx, ry) = (shorter(HEAD), shorter(HEAD));
    let (left, right) = (fw(0.5).plus(shorter(-HEAD)), fw(0.5).plus(shorter(HEAD)));
    let (middle, bottom) = (H.plus(shorter(-HEAD)), H.plus(shorter(-2.0 * HEAD)));
    let head = OwnedPath {
        fill: true,
        no_show: Flag::Off,
        no_line: Flag::Off,
        segs: vec![
            Seg::Move(left, middle),
            Seg::Half { to: (right, middle), via: (fw(0.5), H), rx, ry },
            Seg::Half { to: (left, middle), via: (fw(0.5), bottom), rx, ry },
        ],
    };
    let (r, q) = (fr(BODY_CORNER, 1.0), fr(BODY_CORNER, INSET));
    let (wr, wq) = (W.plus(fr(BODY_CORNER, -1.0)), W.plus(fr(BODY_CORNER, -INSET)));
    let top = fh(BODY_TOP);
    let (top_r, top_q) = (top.plus(fr(BODY_CORNER, -1.0)), top.plus(fr(BODY_CORNER, -INSET)));
    let body = OwnedPath {
        fill: true,
        no_show: Flag::Off,
        no_line: Flag::Off,
        segs: vec![
            Seg::Move(r, Z),
            Seg::Line(wr, Z),
            Seg::Arc { to: (W, r), via: (wq, q) },
            Seg::Line(W, top_r),
            Seg::Arc { to: (wr, top), via: (wq, top_q) },
            Seg::Line(r, top),
            Seg::Arc { to: (Z, top_r), via: (q, top_q) },
            Seg::Line(Z, r),
            Seg::Arc { to: (r, Z), via: (q, q) },
        ],
    };
    vec![head, body]
}

/// A person's connection points: the top of the head, and the middle of each side of the body.
static PERSON_POINTS: [Point; 4] =
    [pc(fw(0.5), H, 0.0, 1.0), pc(Z, fh(BODY_TOP / 2.0), -1.0, 0.0), pc(fw(0.5), Z, 0.0, -1.0), pc(W, fh(BODY_TOP / 2.0), 1.0, 0.0)];

/// The text of a person is in the body, under the head.
const PERSON_BOX: TextBox = TextBox { x: 0.04, y: 0.04, w: 0.92, h: 0.62 };
/// The text of a box with rounded corners.
const BOX_TEXT: TextBox = TextBox { x: 0.03, y: 0.04, w: 0.94, h: 0.92 };
/// The text of a cylinder stands in its body, between the rim of the top and the curve of the bottom.
const DATABASE_BOX: TextBox = TextBox { x: 0.06, y: 0.1, w: 0.88, h: 0.62 };
/// The text of a queue stands between the curve at its left and the one at its right.
const QUEUE_BOX: TextBox = TextBox { x: 0.26, y: 0.08, w: 0.6, h: 0.84 };
/// The label of a boundary and of a deployment node: a block at the bottom left whose text is as low in it as it goes.
const CORNER_BOX: TextBox = TextBox { x: 0.02, y: 0.02, w: 0.46, h: 0.2 };
const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

// ---- the masters' dresses --------------------------------------------------------------------------------------------------------

/// An element that carries an accent: the fill, the three fields, the character formats and the data, and `ObjType` 1 (a shape people join).
const fn element(fill: &'static str, text: &'static [FieldText], chars: &'static [CharRow], data: &'static [FieldDef]) -> Dress {
    Dress { fill: Some(fill), text: TextSpec::Fields(text), chars, data, obj_type: Some(1), ..Dress::NONE }
}

static PERSON_DRESS: Dress = element(PERSON_FILL, &PERSON_TEXT, &LIGHT_CHARS, &WITHOUT_TECHNOLOGY);
static SYSTEM_DRESS: Dress = element(SYSTEM_FILL, &SYSTEM_TEXT, &LIGHT_CHARS, &WITHOUT_TECHNOLOGY);
static CONTAINER_DRESS: Dress = element(CONTAINER_FILL, &CONTAINER_TEXT, &LIGHT_CHARS, &WITH_TECHNOLOGY);
static COMPONENT_DRESS: Dress = element(COMPONENT_FILL, &COMPONENT_TEXT, &TINT_CHARS, &WITH_TECHNOLOGY);
/// A database is a container: the same kind, the same technology.
static DATABASE_DRESS: Dress = element(CONTAINER_FILL, &CONTAINER_TEXT, &LIGHT_CHARS, &WITH_TECHNOLOGY);
static QUEUE_DRESS: Dress = element(CONTAINER_FILL, &CONTAINER_TEXT, &LIGHT_CHARS, &WITH_TECHNOLOGY);
/// An infrastructure node keeps the style's fill: a plain box, whatever theme the page has.
static INFRASTRUCTURE_DRESS: Dress =
    Dress { text: TextSpec::Fields(&INFRASTRUCTURE_TEXT), chars: &PLAIN_CHARS, data: &INFRASTRUCTURE_DATA, obj_type: Some(1), ..Dress::NONE };

/// `VerticalAlign` 2: the text of a corner label sits at the bottom of its block, so that it grows upward from the corner.
static BOTTOM_ALIGNED: [(&str, f64); 1] = [("VerticalAlign", 2.0)];

static BOUNDARY_TEXT: [FieldText; 2] = [
    FieldText { formula: "Prop.Name", shown: "Name", char_row: 0, line_after: true },
    FieldText { formula: "\"[\"&Prop.Kind&\"]\"", shown: "[Software System]", char_row: 1, line_after: false },
];
static BOUNDARY_CHARS: [CharRow; 2] = [CharRow { size_pt: 11.0, style: 1, color: None }, CharRow { size_pt: 8.0, style: 0, color: None }];
static BOUNDARY_DATA: [FieldDef; 2] = [
    NAME,
    FieldDef {
        row: "Kind",
        label: "Kind",
        kind: FieldKind::FixedList("Enterprise;Software System;Container;Group"),
        value: "Software System",
        prompt: "What the boundary encloses",
    },
];
/// A boundary: a dashed outline round what it holds, with no fill, a container whose outline is its master's.
static BOUNDARY_DRESS: Dress = Dress {
    fill_pattern: Some(0),
    line_pattern: Some(Flag::Formula("2")),
    text: TextSpec::Fields(&BOUNDARY_TEXT),
    chars: &BOUNDARY_CHARS,
    data: &BOUNDARY_DATA,
    locks: &BOTTOM_ALIGNED,
    obj_type: Some(1),
    role: Role::Container { heading: Side::Bottom, margin_in: 0.25 },
    ..Dress::NONE
};

static NODE_TEXT: [FieldText; 3] = [
    FieldText { formula: "Prop.Name", shown: "Name", char_row: 0, line_after: true },
    FieldText { formula: typed!("Deployment Node"), shown: "[Deployment Node]", char_row: 1, line_after: true },
    FieldText { formula: "\"x \"&Prop.Instances", shown: "x 1", char_row: 2, line_after: false },
];
static NODE_CHARS: [CharRow; 3] = [
    CharRow { size_pt: 11.0, style: 1, color: None },
    CharRow { size_pt: 8.0, style: 0, color: None },
    CharRow { size_pt: 8.0, style: 0, color: None },
];
static NODE_DATA: [FieldDef; 3] = [
    NAME,
    TECHNOLOGY,
    FieldDef { row: "Instances", label: "Instances", kind: FieldKind::String, value: "1", prompt: "How many run, or a range such as 1-3" },
];
/// A deployment node: a light tint of the accent, a container whose outline is its master's.
static NODE_DRESS: Dress = Dress {
    fill: Some(NODE_FILL),
    text: TextSpec::Fields(&NODE_TEXT),
    chars: &NODE_CHARS,
    data: &NODE_DATA,
    locks: &BOTTOM_ALIGNED,
    obj_type: Some(1),
    role: Role::Container { heading: Side::Bottom, margin_in: 0.25 },
    ..Dress::NONE
};

static TITLE_TEXT: [FieldText; 2] = [
    FieldText { formula: "\"[\"&Prop.Diagram&\"] \"&Prop.Title", shown: "[System Context] Title", char_row: 0, line_after: true },
    FieldText { formula: "Prop.Description", shown: "Description", char_row: 1, line_after: false },
];
static TITLE_CHARS: [CharRow; 2] = [CharRow { size_pt: 12.0, style: 1, color: None }, CharRow { size_pt: 9.0, style: 0, color: None }];
static TITLE_DATA: [FieldDef; 3] = [
    FieldDef {
        row: "Diagram",
        label: "Diagram",
        kind: FieldKind::FixedList("System Context;Container;Component;Deployment;Dynamic;System Landscape"),
        value: "System Context",
        prompt: "The kind of diagram",
    },
    FieldDef { row: "Title", label: "Title", kind: FieldKind::String, value: "Title", prompt: "What the diagram is about" },
    FieldDef { row: "Description", label: "Description", kind: FieldKind::String, value: "Description", prompt: "What the diagram shows" },
];
/// A title: text only, no line and no fill, which no connector joins.
static TITLE_DRESS: Dress = Dress {
    fill_pattern: Some(0),
    line_pattern: Some(Flag::Off),
    text: TextSpec::Fields(&TITLE_TEXT),
    chars: &TITLE_CHARS,
    data: &TITLE_DATA,
    obj_type: Some(1),
    role: Role::Pointless,
    ..Dress::NONE
};

static RELATIONSHIP_TEXT: [FieldText; 2] = [
    FieldText { formula: "Prop.Description", shown: "Description", char_row: 0, line_after: true },
    FieldText { formula: "IF(LEN(Prop.Technology)>0,\"[\"&Prop.Technology&\"]\",\"\")", shown: "", char_row: 1, line_after: false },
];
static RELATIONSHIP_CHARS: [CharRow; 2] = [CharRow { size_pt: 9.0, style: 0, color: None }, CharRow { size_pt: 8.0, style: 0, color: None }];
static RELATIONSHIP_DATA: [FieldDef; 2] = [
    FieldDef {
        row: "Description",
        label: "Description",
        kind: FieldKind::String,
        value: "Description",
        prompt: "What the relationship means, in the direction it points",
    },
    FieldDef { row: "Technology", label: "Technology", kind: FieldKind::String, value: "", prompt: "The protocol or technology it uses" },
];
/// `TextBkgnd` 2 is white (the table's second colour: the reader takes one less than the number): the label hides the dashes behind it.
static WHITE_LABEL: [(&str, f64); 1] = [("TextBkgnd", 2.0)];
static RELATIONSHIP_DRESS: Dress =
    Dress { text: TextSpec::Fields(&RELATIONSHIP_TEXT), chars: &RELATIONSHIP_CHARS, data: &RELATIONSHIP_DATA, locks: &WHITE_LABEL, ..Dress::NONE };

// ---- the key: a group of a title and five samples, each box in the colours of the master it stands for -------------------------------------

/// A sample of the key: a box filled with the kind's colour, holding the kind's name.
const fn sample(fill: &'static str, name: &'static str, chars: &'static [CharRow]) -> Dress {
    Dress { fill: Some(fill), text: TextSpec::Plain(name), chars, ..Dress::NONE }
}

static LIGHT_SAMPLE: [CharRow; 1] = [CharRow { size_pt: 9.0, style: 0, color: Some(LIGHT) }];
static DARK_SAMPLE: [CharRow; 1] = [CharRow { size_pt: 9.0, style: 0, color: None }];
static PERSON_SAMPLE: Dress = sample(PERSON_OWN, "Person", &LIGHT_SAMPLE);
static SYSTEM_SAMPLE: Dress = sample(SYSTEM_OWN, "Software System", &LIGHT_SAMPLE);
static CONTAINER_SAMPLE: Dress = sample(BLUE, "Container", &LIGHT_SAMPLE);
static COMPONENT_SAMPLE: Dress = sample(COMPONENT_OWN, "Component", &DARK_SAMPLE);
static EXTERNAL_SAMPLE: Dress = sample(GREY, "External", &LIGHT_SAMPLE);
static KEY_TITLE_CHARS: [CharRow; 1] = [CharRow { size_pt: 11.0, style: 1, color: None }];
static KEY_TITLE: Dress =
    Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), text: TextSpec::Plain("Key"), chars: &KEY_TITLE_CHARS, ..Dress::NONE };

/// A sample's frame in the key: as wide as the key and 0.36 inch tall, its middle at `pin_y`. The title (0.3 inch) and the five samples, 0.06 inch apart, fill the key from its top to
/// its bottom, so that the box of the members is the key's box (an `.odg` holds a group as its members, and keeps its frame only when that is so). **The heights and the distances follow the
/// page's drop scale** (`scaled!`), so that a key dropped beyond the range of eight is the box its members fill at 1:1, on paper.
const fn row(pin_y: &'static str) -> MemberFrame {
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y, width: "Sheet.5!Width", height: scaled!("0.36") }
}

/// The samples and then the title, which is the member last in the table: typing on the selected key goes to it.
static KEY_MEMBERS: [MemberSpec; 6] = [
    MemberSpec {
        name: "Person",
        frame: row(concat!("Sheet.5!Height-", scaled!("0.54"))),
        outline: Outline::Code { build: rounded_06, text: TextBox::FULL },
        points: &[],
        dress: Some(&PERSON_SAMPLE),
    },
    MemberSpec {
        name: "Software System",
        frame: row(concat!("Sheet.5!Height-", scaled!("0.96"))),
        outline: Outline::Code { build: rounded_06, text: TextBox::FULL },
        points: &[],
        dress: Some(&SYSTEM_SAMPLE),
    },
    MemberSpec {
        name: "Container",
        frame: row(concat!("Sheet.5!Height-", scaled!("1.38"))),
        outline: Outline::Code { build: rounded_06, text: TextBox::FULL },
        points: &[],
        dress: Some(&CONTAINER_SAMPLE),
    },
    MemberSpec {
        name: "Component",
        frame: row(concat!("Sheet.5!Height-", scaled!("1.80"))),
        outline: Outline::Code { build: rounded_06, text: TextBox::FULL },
        points: &[],
        dress: Some(&COMPONENT_SAMPLE),
    },
    MemberSpec {
        name: "External",
        frame: row(concat!("Sheet.5!Height-", scaled!("2.22"))),
        outline: Outline::Code { build: rounded_06, text: TextBox::FULL },
        points: &[],
        dress: Some(&EXTERNAL_SAMPLE),
    },
    MemberSpec {
        name: "Title",
        frame: MemberFrame::Formulas {
            pin_x: "Sheet.5!Width*0.5",
            pin_y: concat!("Sheet.5!Height-", scaled!("0.15")),
            width: "Sheet.5!Width",
            height: scaled!("0.3"),
        },
        outline: Outline::Polygon(RECT),
        points: &[],
        dress: Some(&KEY_TITLE),
    },
];

/// The key keeps its height (its samples are boxes of a fixed height), so a resize changes its width only; typing on it goes to its title.
static KEY_LOCKS: [(&str, f64); 1] = [("LockHeight", 1.0)];
static KEY_DRESS: Dress =
    Dress { locks: &KEY_LOCKS, select_mode: Some(1), text_edit_target: Some(false), obj_type: Some(1), role: Role::Pointless, ..Dress::NONE };

// ---- the masters -----------------------------------------------------------------------------------------------------------------

macro_rules! more {
    ($name:ident, $dress:ident) => {
        static $name: More = More { dress: Some(&$dress), hidden: false, brings: &[] };
    };
}
more!(PERSON_MORE, PERSON_DRESS);
more!(SYSTEM_MORE, SYSTEM_DRESS);
more!(CONTAINER_MORE, CONTAINER_DRESS);
more!(COMPONENT_MORE, COMPONENT_DRESS);
more!(DATABASE_MORE, DATABASE_DRESS);
more!(QUEUE_MORE, QUEUE_DRESS);
more!(BOUNDARY_MORE, BOUNDARY_DRESS);
more!(NODE_MORE, NODE_DRESS);
more!(INFRASTRUCTURE_MORE, INFRASTRUCTURE_DRESS);
more!(TITLE_MORE, TITLE_DRESS);
more!(KEY_MORE, KEY_DRESS);
more!(RELATIONSHIP_MORE, RELATIONSHIP_DRESS);

/// The relationship: dashed, with a solid arrowhead at its end and none at its begin, routed as the page routes.
const RELATIONSHIP_LOOK: ConnectorLook = ConnectorLook { pattern: 2, ..ConnectorLook::plain(LineEnd::None, LineEnd::Number(4)) };

pub(super) static MASTERS: [MasterSpec; 12] = [
    MasterSpec {
        id: "c4.person",
        name: "Person",
        width: 1.4,
        height: 1.5,
        identity: (SERIES, 1),
        outline: Outline::Code { build: person_figure, text: PERSON_BOX },
        points: &PERSON_POINTS,
        more: &PERSON_MORE,
    },
    MasterSpec {
        id: "c4.software-system",
        name: "Software System",
        width: 2.0,
        height: 1.3,
        identity: (SERIES, 2),
        outline: Outline::Code { build: rounded_08, text: BOX_TEXT },
        points: &kit::GRID3,
        more: &SYSTEM_MORE,
    },
    MasterSpec {
        id: "c4.container",
        name: "Container",
        width: 2.0,
        height: 1.3,
        identity: (SERIES, 3),
        outline: Outline::Code { build: rounded_08, text: BOX_TEXT },
        points: &kit::GRID3,
        more: &CONTAINER_MORE,
    },
    MasterSpec {
        id: "c4.component",
        name: "Component",
        width: 2.0,
        height: 1.2,
        identity: (SERIES, 4),
        outline: Outline::Code { build: rounded_08, text: BOX_TEXT },
        points: &kit::GRID3,
        more: &COMPONENT_MORE,
    },
    MasterSpec {
        id: "c4.database-container",
        name: "Database Container",
        width: 1.8,
        height: 1.4,
        identity: (SERIES, 5),
        outline: Outline::Code { build: kit::cylinder_v, text: DATABASE_BOX },
        points: &SIDES,
        more: &DATABASE_MORE,
    },
    MasterSpec {
        id: "c4.queue-container",
        name: "Queue Container",
        width: 2.2,
        height: 0.9,
        identity: (SERIES, 6),
        outline: Outline::Code { build: kit::cylinder_h, text: QUEUE_BOX },
        points: &SIDES,
        more: &QUEUE_MORE,
    },
    MasterSpec {
        id: "c4.boundary",
        name: "Boundary",
        width: 4.0,
        height: 3.0,
        identity: (SERIES, 7),
        outline: Outline::Code { build: rounded_10, text: CORNER_BOX },
        points: &kit::GRID3,
        more: &BOUNDARY_MORE,
    },
    MasterSpec {
        id: "c4.deployment-node",
        name: "Deployment Node",
        width: 4.0,
        height: 3.0,
        identity: (SERIES, 8),
        outline: Outline::Code { build: rounded_08, text: CORNER_BOX },
        points: &kit::GRID3,
        more: &NODE_MORE,
    },
    MasterSpec {
        id: "c4.infrastructure-node",
        name: "Infrastructure Node",
        width: 1.8,
        height: 1.0,
        identity: (SERIES, 9),
        outline: Outline::Polygon(RECT),
        points: &kit::GRID3,
        more: &INFRASTRUCTURE_MORE,
    },
    MasterSpec {
        id: "c4.title",
        name: "Diagram Title",
        width: 4.0,
        height: 0.6,
        identity: (SERIES, 10),
        outline: Outline::Polygon(RECT),
        points: &[],
        more: &TITLE_MORE,
    },
    MasterSpec {
        id: "c4.key",
        name: "Key",
        width: 2.2,
        height: 2.4,
        identity: (SERIES, 11),
        outline: Outline::Group { members: &KEY_MEMBERS, text: TextBox::FULL },
        points: &[],
        more: &KEY_MORE,
    },
    MasterSpec {
        id: "c4.relationship",
        name: "Relationship",
        width: 1.5,
        height: 0.0,
        identity: (SERIES, 12),
        outline: Outline::Connector(RELATIONSHIP_LOOK),
        points: &[],
        more: &RELATIONSHIP_MORE,
    },
];

/// The search words of the master with this id, space separated, or `""`. Plain words for what each is called in other places or is used for (the C4 model's own names for
/// its elements and its diagrams, and the words people use for a user, a system, a service and a queue).
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "c4.person" => "actor user role persona customer stakeholder human",
        "c4.software-system" => "system application product software service",
        "c4.container" => "application app service web api runtime deployable",
        "c4.component" => "module class package part library",
        "c4.database-container" => "database db data store sql schema storage",
        "c4.queue-container" => "queue topic message broker bus stream",
        "c4.boundary" => "boundary scope system enterprise group border frame region",
        "c4.deployment-node" => "deployment node server host machine environment cluster",
        "c4.infrastructure-node" => "infrastructure dns load balancer gateway proxy appliance",
        "c4.title" => "title heading diagram caption name",
        "c4.key" => "key legend explanation",
        "c4.relationship" => "relationship line dependency interaction uses connector flow",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
