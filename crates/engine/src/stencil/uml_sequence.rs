//! UML Sequence (`uml-sequence`, GUID series 11): fourteen masters (section 6.1 of the second stencil batch's design, "Stencils: UML, BPMN, C4, network and database", and
//! of the batch's notation spec, which win where they differ: DD2, DD3 and DD5).
//!
//! **Original figures.** Every figure is a rectangle, a stroke or a stick figure the kit draws (`kit::stick_figure`, `kit::note`), put together by formulas of its group. The notation is
//! followed from the public standard (UML 2.5.1, clause 17); no figure of the standard or of another application is traced.
//!
//! * **The order** is the Shapes window's, and the Quick Shapes are the first four that are not lines: Lifeline, Actor Lifeline, Activation, Combined Fragment (so Destruction stands after the
//!   fragment, where the design's table had it before).
//! * **A lifeline** is a group of two members: a **head** 0.4 inch tall that holds the text (the last member, so typing on the selected lifeline goes to it) and a **dashed stem** under it, down
//!   the middle, as long as the rest of the height. The lifeline holds `LockWidth` (LOCKAXIS: a stretch of several keeps each head's width and sizes each stem) and `SelectMode` 1. Its
//!   connection points are three on the head (the middle of the top and of each side) and **twenty pairs on the stem**, one facing left and one right, 0.05 of the box under its first 0.85 inch
//!   apart, from 0.95 of the way up it to its foot (the same on both kinds, so that a message glued to the same pair of two lifelines of one height is level, an actor's and an object's too). An
//!   **actor lifeline** has a stick figure 0.6 inch tall, its name in a strip 0.25 inch under it, and the stem below both.
//! * **An activation** is a thin rectangle that takes the style's fill (white) and not the line colour (DD2). **A destruction** is an X of two strokes, a label below it.
//! * **A combined fragment** is a group of three members, its **frame** (a rectangle with no fill, so that what lies under it shows), its **tag** (a pentagon 0.6 by 0.25 inch at the top left whose
//!   text is a field that shows the Shape Data row `Operator`, a fixed list of the twelve operators, `alt` at first: DD3 keeps the design's default) and its **guard** (the placeholder
//!   `[condition]` in a strip right of the tag, the last member). It is a container: a shape dropped inside joins it and a move takes its members. **An interaction use** is the same frame (white) with a
//!   `ref` tag and the interaction's name over its whole box.
//! * **An operand separator** is a dashed line across the middle of its box with the guard `[else]` above it at the left.
//! * **The messages** are lines with straight routes: the synchronous call a solid head (the 4), the asynchronous one an open head (3), the reply and the create message dashed with an open head, the
//!   note link dashed with no end; none has a begin end. **The self message** is a [`Outline::Fixed1D`]: a loop 0.4 inch out across the line and back, whose ends glue to two points of one stem
//!   and which is stretched along the line and never routed. A line's label sits at its middle and is as wide as the line is long (the engine's rule for every line), so the self message, whose
//!   line is a few tenths of an inch, holds no placeholder text.

use super::container::Side;
use super::group_master::{MemberFrame, MemberSpec};
use super::kit::{self, GRID3, SIDES};
use super::{
    CharRow, Co, ConnectorLook, Dress, FieldDef, FieldKind, FieldText, Flag, LineEnd, MasterSpec, More, Outline, OwnedPath, PathSpec, Point, Role,
    Route, Seg, TextBox, TextSpec, fh, fk, fw, pc, pt,
};

#[cfg(test)]
mod tests;

/// The series of the stencil's masters in their GUIDs (`D1AC000B-...`).
const SERIES: u8 = 11;

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

// ---- shapes of the figures -----------------------------------------------------------------------------------------------------------

/// A rectangle that fills the box, as one closed path that takes the fill.
const RECT: &[Seg] = &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)];

static RECT_PATH: [PathSpec; 1] = [PathSpec { fill: true, segs: RECT, no_show: Flag::Off, no_line: Flag::Off }];

/// The stem of a lifeline: one stroke down the middle of its box.
static STEM_PATH: [PathSpec; 1] =
    [PathSpec { fill: false, segs: &[Seg::Move(fw(0.5), Z), Seg::Line(fw(0.5), H)], no_show: Flag::Off, no_line: Flag::Off }];

/// The X of a destruction: two strokes, corner to corner.
static CROSS_PATHS: [PathSpec; 2] = [
    PathSpec { fill: false, segs: &[Seg::Move(Z, Z), Seg::Line(W, H)], no_show: Flag::Off, no_line: Flag::Off },
    PathSpec { fill: false, segs: &[Seg::Move(Z, H), Seg::Line(W, Z)], no_show: Flag::Off, no_line: Flag::Off },
];

/// The line of an operand separator: one stroke across the middle of the box.
static RULE_PATH: [PathSpec; 1] =
    [PathSpec { fill: false, segs: &[Seg::Move(Z, fh(0.5)), Seg::Line(W, fh(0.5))], no_show: Flag::Off, no_line: Flag::Off }];

/// The loop of a self message, in the line's own frame (x along it, y across it): out 0.4 inch across the line, along it by its length, and back. The head sits at the end, which is on the line.
static LOOP_PATH: [PathSpec; 1] = [PathSpec {
    fill: false,
    segs: &[Seg::Move(Z, Z), Seg::Line(Z, fk(0.4)), Seg::Line(W, fk(0.4)), Seg::Line(W, Z)],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

/// The pentagon of an operator tag: a rectangle whose lower right corner is cut, by 0.12 of its width and 0.3 of its height (shares of the box, since an `.odg` writes an outline that is shares of its
/// box as the outline it draws, and writes one with fixed lengths in it as a rectangle). A tag is the same size whatever the frame (its members' frame is fixed), so the cut is too.
fn tag() -> Vec<OwnedPath> {
    vec![OwnedPath {
        fill: true,
        segs: vec![Seg::Move(Z, Z), Seg::Line(fw(0.88), Z), Seg::Line(W, fh(0.3)), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)],
        no_show: Flag::Off,
        no_line: Flag::Off,
    }]
}

// ---- the text blocks ------------------------------------------------------------------------------------------------------------------

/// An activation's label: to the right of the bar (it is 0.16 inch wide), 6 times as wide as the bar.
const BESIDE: TextBox = TextBox { x: 1.2, y: 0.0, w: 6.0, h: 1.0 };

/// A destruction's label: under the X, three times as wide, from 0.9 of its height below it to 0.1.
const LABEL_WIDE: TextBox = TextBox { x: -1.0, y: -0.9, w: 3.0, h: 0.8 };

/// An operand separator's guard: above the line, in the left half of the box. The block stands from the line to past the top of the box (a line of the default 12 point text is 0.2 inch, and
/// the box is 0.25 inch tall), so that the text is clear of the line.
const GUARD_LEFT: TextBox = TextBox { x: 0.0, y: 0.5, w: 0.5, h: 1.2 };

// ---- the points ---------------------------------------------------------------------------------------------------------------------

/// The steps of a lifeline's stem: twenty pairs, at the twentieths of the stem from 0.95 of the way up it to its foot (shares exact in decimals, so that a cell's value is its formula's).
const STEPS: usize = 20;

/// The points of a lifeline: `top` (three of the head), then the pairs of its stem, which begin `head_in` inches under the top of the box and end at its foot. At step `k` a pair, one point that
/// leaves to the left and one to the right, down the middle: `(Height - head_in) * share` above the foot, `share` being 0.95 for step 0 and 0 for step 19.
const fn lifeline_points(top: [Point; 3], head_in: f64) -> [Point; 43] {
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); 43];
    let mut i = 0;
    while i < 3 {
        out[i] = top[i];
        i += 1;
    }
    let mut k = 0;
    while k < STEPS {
        let share = (STEPS - 1 - k) as f64 / STEPS as f64;
        let y = fh(share).plus(fk(0.0 - head_in * share));
        out[3 + 2 * k] = pc(fw(0.5), y, -1.0, 0.0);
        out[3 + 2 * k + 1] = pc(fw(0.5), y, 1.0, 0.0);
        k += 1;
    }
    out
}

/// A lifeline's head: the middle of its top, and the middle of each side of a head 0.4 inch tall.
const LIFELINE_HEAD: [Point; 3] = [pt(0.5, 1.0, 0.0, 1.0), pc(Z, H.plus(fk(-0.2)), -1.0, 0.0), pc(W, H.plus(fk(-0.2)), 1.0, 0.0)];

/// An actor's figure: the top of its head and its two hands (0.2 inch to each side of the middle, at 0.62 of the figure's 0.6 inch from its feet).
const ACTOR_HEAD: [Point; 3] =
    [pt(0.5, 1.0, 0.0, 1.0), pc(fw(0.5).plus(fk(-0.2)), H.plus(fk(-0.228)), -1.0, 0.0), pc(fw(0.5).plus(fk(0.2)), H.plus(fk(-0.228)), 1.0, 0.0)];

/// How far under the top of the box the steps of the stem begin, **on every kind of lifeline**: the tallest head (an actor's figure and its name, 0.85 inch). A lifeline's own head is 0.4 inch, but its
/// steps begin where an actor lifeline's do, so that a step is at one height on all of them and a message between an actor and an object is level.
const STEP_TOP: f64 = 0.85;

static LIFELINE_POINTS: [Point; 43] = lifeline_points(LIFELINE_HEAD, STEP_TOP);
static ACTOR_POINTS: [Point; 43] = lifeline_points(ACTOR_HEAD, STEP_TOP);

/// An activation's twelve: five on each long side, at the odd tenths, and the middles of its two ends.
static ACTIVATION_POINTS: [Point; 12] = activation_points();

const fn activation_points() -> [Point; 12] {
    let mut out = [pt(0.0, 0.0, 0.0, 0.0); 12];
    let mut i = 0;
    while i < 5 {
        let share = (2 * i + 1) as f64 / 10.0;
        out[i] = pt(0.0, share, -1.0, 0.0);
        out[5 + i] = pt(1.0, share, 1.0, 0.0);
        i += 1;
    }
    out[10] = pt(0.5, 1.0, 0.0, 1.0);
    out[11] = pt(0.5, 0.0, 0.0, -1.0);
    out
}

/// A destruction's four: the ends of its arms, the corners of its box, each leaving along the horizontal axis (the router knows the four ways out and no more).
static CROSS_POINTS: [Point; 4] = [pt(0.0, 1.0, -1.0, 0.0), pt(1.0, 1.0, 1.0, 0.0), pt(1.0, 0.0, 1.0, 0.0), pt(0.0, 0.0, -1.0, 0.0)];

/// An operand separator's four: the two ends of its line and the middle of it, once up and once down.
static RULE_POINTS: [Point; 4] = [pt(0.0, 0.5, -1.0, 0.0), pt(1.0, 0.5, 1.0, 0.0), pt(0.5, 0.5, 0.0, 1.0), pt(0.5, 0.5, 0.0, -1.0)];

// ---- the dresses --------------------------------------------------------------------------------------------------------------------

/// The twelve operators of a combined fragment, in the design's order.
const OPERATOR: FieldDef = FieldDef {
    row: "Operator",
    label: "Operator",
    kind: FieldKind::FixedList("alt;opt;loop;par;break;critical;neg;assert;strict;seq;ignore;consider"),
    value: "alt",
    prompt: "The interaction operator",
};

/// A lifeline: a shape people join, selected as a group first, whose text is its head's, and whose width is locked.
static LIFELINE_DRESS: Dress =
    Dress { obj_type: Some(1), locks: &[("LockWidth", 1.0)], select_mode: Some(1), text_edit_target: Some(false), ..Dress::NONE };

/// A combined fragment: a container whose heading is its tag, and whose text is its guard's.
static FRAGMENT_DRESS: Dress = Dress {
    data: &[OPERATOR],
    select_mode: Some(1),
    text_edit_target: Some(false),
    obj_type: Some(1),
    role: Role::Container { heading: Side::Top, margin_in: 0.2 },
    ..Dress::NONE
};

/// A group of members whose text is the last member's, selected as a group first (an interaction use).
static GROUP_DRESS: Dress = Dress { obj_type: Some(1), select_mode: Some(1), text_edit_target: Some(false), ..Dress::NONE };

/// The dashed stem of a lifeline, and the rule of an operand separator: pattern 2.
static DASHED: Dress = Dress { line_pattern: Some(Flag::Formula("2")), ..Dress::NONE };

/// An operand separator: a shape people join, dashed, with the guard `[else]`.
static RULE_DRESS: Dress = Dress { line_pattern: Some(Flag::Formula("2")), text: TextSpec::Plain("[else]"), obj_type: Some(1), ..Dress::NONE };

/// A lifeline's head holds the notation's `name : Type`.
static HEAD_DRESS: Dress = Dress { text: TextSpec::Plain("object : Class"), ..Dress::NONE };

/// A member that draws nothing and holds text: no fill and no line.
static NAME_DRESS: Dress = Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), text: TextSpec::Plain("Actor"), ..Dress::NONE };
static GUARD_DRESS: Dress = Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), text: TextSpec::Plain("[condition]"), ..Dress::NONE };
static INTERACTION_DRESS: Dress = Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), text: TextSpec::Plain("Interaction"), ..Dress::NONE };

/// A frame with no fill: what lies under it shows.
static OPEN_FRAME: Dress = Dress { fill_pattern: Some(0), ..Dress::NONE };

/// An operator's tag shows the group's `Operator` as a field, in bold.
static TAG_FIELDS: [FieldText; 1] = [FieldText { formula: "Sheet.5!Prop.Operator", shown: "alt", char_row: 0, line_after: false }];
static TAG_DRESS: Dress = Dress { text: TextSpec::Fields(&TAG_FIELDS), chars: &[CharRow { size_pt: 8.0, style: 1, color: None }], ..Dress::NONE };

/// The tag of an interaction use says `ref`, in bold.
static REF_DRESS: Dress = Dress { text: TextSpec::Plain("ref"), chars: &[CharRow { size_pt: 8.0, style: 1, color: None }], ..Dress::NONE };

/// A destruction's X is a little heavier than a line.
static CROSS_DRESS: Dress = Dress { obj_type: Some(1), line_weight_pt: Some(1.5), ..Dress::NONE };

/// The placeholder text of the messages that have one.
static CALL_DRESS: Dress = Dress { text: TextSpec::Plain("message()"), ..Dress::NONE };
static SIGNAL_DRESS: Dress = Dress { text: TextSpec::Plain("signal"), ..Dress::NONE };
static CREATE_DRESS: Dress = Dress { text: TextSpec::Plain("\u{ab}create\u{bb}"), ..Dress::NONE };

static LIFELINE_MORE: More = More { dress: Some(&LIFELINE_DRESS), hidden: false, brings: &[] };
static FRAGMENT_MORE: More = More { dress: Some(&FRAGMENT_DRESS), hidden: false, brings: &[] };
static INTERACTION_MORE: More = More { dress: Some(&GROUP_DRESS), hidden: false, brings: &[] };
static RULE_MORE: More = More { dress: Some(&RULE_DRESS), hidden: false, brings: &[] };
static CROSS_MORE: More = More { dress: Some(&CROSS_DRESS), hidden: false, brings: &[] };
static CALL_MORE: More = More { dress: Some(&CALL_DRESS), hidden: false, brings: &[] };
static SIGNAL_MORE: More = More { dress: Some(&SIGNAL_DRESS), hidden: false, brings: &[] };
static CREATE_MORE: More = More { dress: Some(&CREATE_DRESS), hidden: false, brings: &[] };

// ---- the groups' members --------------------------------------------------------------------------------------------------------------

/// A member that is as wide as the group (`Sheet.5` is the group's shape) and `height` formula tall, with its middle at `pin_y`. **Every fixed length in a frame follows the page's drop scale**
/// (`scaled!`), so that a lifeline, an actor or a fragment dropped beyond the range of eight is the same figure on paper as at 1:1.
const fn strip(pin_y: &'static str, height: &'static str) -> MemberFrame {
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y, width: "Sheet.5!Width", height }
}

/// A member of the group's own size.
const WHOLE: MemberFrame =
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: "Sheet.5!Height*0.5", width: "Sheet.5!Width", height: "Sheet.5!Height" };

/// A lifeline: the stem (dashed, down from the head to the foot) and the head (0.4 inch tall at the top), the head last because typing goes to it.
static LIFELINE_MEMBERS: [MemberSpec; 2] = [
    MemberSpec {
        name: "Stem",
        frame: MemberFrame::Formulas {
            pin_x: "Sheet.5!Width*0.5",
            pin_y: "Height*0.5",
            width: "Sheet.5!Width",
            height: concat!("MAX(0,Sheet.5!Height-", scaled!("0.4"), ")"),
        },
        outline: Outline::Paths { paths: &STEM_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&DASHED),
    },
    MemberSpec {
        name: "Head",
        frame: strip("Sheet.5!Height-Height*0.5", concat!("MIN(", scaled!("0.4"), ",Sheet.5!Height)")),
        outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&HEAD_DRESS),
    },
];

/// An actor lifeline: the stem (from 0.85 inch under the top), the figure (0.4 by 0.6 inch at the top) and its name in a strip 0.25 inch tall under it, the name last.
static ACTOR_MEMBERS: [MemberSpec; 3] = [
    MemberSpec {
        name: "Stem",
        frame: MemberFrame::Formulas {
            pin_x: "Sheet.5!Width*0.5",
            pin_y: "Height*0.5",
            width: "Sheet.5!Width",
            height: concat!("MAX(0,Sheet.5!Height-", scaled!("0.85"), ")"),
        },
        outline: Outline::Paths { paths: &STEM_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&DASHED),
    },
    MemberSpec {
        name: "Figure",
        frame: MemberFrame::Formulas {
            pin_x: "Sheet.5!Width*0.5",
            pin_y: concat!("Sheet.5!Height-", scaled!("0.3")),
            width: scaled!("0.4"),
            height: scaled!("0.6"),
        },
        outline: Outline::Code { build: kit::stick_figure, text: TextBox::FULL },
        points: &[],
        dress: None,
    },
    MemberSpec {
        name: "Name",
        frame: strip(concat!("Sheet.5!Height-", scaled!("0.725")), scaled!("0.25")),
        outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&NAME_DRESS),
    },
];

/// A combined fragment: the frame (no fill), the tag at its top left and the guard in a strip right of the tag, the guard last because typing goes to it.
static FRAGMENT_MEMBERS: [MemberSpec; 3] = [
    MemberSpec {
        name: "Frame",
        frame: WHOLE,
        outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&OPEN_FRAME),
    },
    MemberSpec {
        name: "Tag",
        frame: MemberFrame::Formulas {
            pin_x: scaled!("0.3"),
            pin_y: concat!("Sheet.5!Height-", scaled!("0.125")),
            width: scaled!("0.6"),
            height: scaled!("0.25"),
        },
        outline: Outline::Code { build: tag, text: TextBox::FULL },
        points: &[],
        dress: Some(&TAG_DRESS),
    },
    MemberSpec {
        name: "Guard",
        frame: MemberFrame::Formulas {
            pin_x: concat!(scaled!("0.65"), "+Width*0.5"),
            pin_y: concat!("Sheet.5!Height-", scaled!("0.125")),
            width: concat!("MAX(", scaled!("0.1"), ",MIN(", scaled!("1"), ",Sheet.5!Width-", scaled!("0.7"), "))"),
            height: scaled!("0.25"),
        },
        outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&GUARD_DRESS),
    },
];

/// An interaction use: the frame (filled), the `ref` tag at its top left (0.4 by 0.25 inch) and the name over the whole box, the name last because typing goes to it.
static INTERACTION_MEMBERS: [MemberSpec; 3] = [
    MemberSpec { name: "Frame", frame: WHOLE, outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL }, points: &[], dress: None },
    MemberSpec {
        name: "Tag",
        frame: MemberFrame::Formulas {
            pin_x: scaled!("0.2"),
            pin_y: concat!("Sheet.5!Height-", scaled!("0.125")),
            width: scaled!("0.4"),
            height: scaled!("0.25"),
        },
        outline: Outline::Code { build: tag, text: TextBox::FULL },
        points: &[],
        dress: Some(&REF_DRESS),
    },
    MemberSpec {
        name: "Name",
        frame: WHOLE,
        outline: Outline::Paths { paths: &RECT_PATH, text: TextBox::FULL },
        points: &[],
        dress: Some(&INTERACTION_DRESS),
    },
];

// ---- the lines ----------------------------------------------------------------------------------------------------------------------

/// A straight line with these ends and this pattern.
const fn straight(end: LineEnd, pattern: u8) -> ConnectorLook {
    ConnectorLook { pattern, route: Route::Straight, ..ConnectorLook::plain(LineEnd::None, end) }
}

/// The synchronous call: a solid head, solid; the asynchronous one and the reply and the create message: an open head, solid or dashed; the note link: no head, dashed.
const CALL: ConnectorLook = straight(LineEnd::Number(4), 1);
const SIGNAL: ConnectorLook = straight(LineEnd::Number(3), 1);
const DASHED_OPEN: ConnectorLook = straight(LineEnd::Number(3), 2);
const NOTE_LINK: ConnectorLook = straight(LineEnd::None, 2);

pub(super) static MASTERS: [MasterSpec; 14] = [
    MasterSpec {
        identity: (SERIES, 1),
        id: "uml-sequence.lifeline",
        name: "Lifeline",
        width: 1.2,
        height: 3.0,
        outline: Outline::Group { members: &LIFELINE_MEMBERS, text: TextBox::FULL },
        points: &LIFELINE_POINTS,
        more: &LIFELINE_MORE,
    },
    MasterSpec {
        identity: (SERIES, 2),
        id: "uml-sequence.actor-lifeline",
        name: "Actor Lifeline",
        width: 1.0,
        height: 3.0,
        outline: Outline::Group { members: &ACTOR_MEMBERS, text: TextBox::FULL },
        points: &ACTOR_POINTS,
        more: &LIFELINE_MORE,
    },
    MasterSpec {
        identity: (SERIES, 3),
        id: "uml-sequence.activation",
        name: "Activation",
        width: 0.16,
        height: 1.0,
        outline: Outline::Paths { paths: &RECT_PATH, text: BESIDE },
        points: &ACTIVATION_POINTS,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 4),
        id: "uml-sequence.fragment",
        name: "Combined Fragment",
        width: 3.0,
        height: 1.6,
        outline: Outline::Group { members: &FRAGMENT_MEMBERS, text: TextBox::FULL },
        points: &GRID3,
        more: &FRAGMENT_MORE,
    },
    MasterSpec {
        identity: (SERIES, 5),
        id: "uml-sequence.destruction",
        name: "Destruction",
        width: 0.3,
        height: 0.3,
        outline: Outline::Paths { paths: &CROSS_PATHS, text: LABEL_WIDE },
        points: &CROSS_POINTS,
        more: &CROSS_MORE,
    },
    MasterSpec {
        identity: (SERIES, 6),
        id: "uml-sequence.operand-separator",
        name: "Operand Separator",
        width: 3.0,
        height: 0.25,
        outline: Outline::Paths { paths: &RULE_PATH, text: GUARD_LEFT },
        points: &RULE_POINTS,
        more: &RULE_MORE,
    },
    MasterSpec {
        identity: (SERIES, 7),
        id: "uml-sequence.interaction-use",
        name: "Interaction Use",
        width: 2.4,
        height: 0.8,
        outline: Outline::Group { members: &INTERACTION_MEMBERS, text: TextBox::FULL },
        points: &GRID3,
        more: &INTERACTION_MORE,
    },
    MasterSpec {
        identity: (SERIES, 8),
        id: "uml-sequence.note",
        name: "Note",
        width: 1.4,
        height: 0.8,
        outline: Outline::Code { build: kit::note, text: TextBox::FULL },
        points: &SIDES,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 9),
        id: "uml-sequence.message",
        name: "Message",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(CALL),
        points: &[],
        more: &CALL_MORE,
    },
    MasterSpec {
        identity: (SERIES, 10),
        id: "uml-sequence.async-message",
        name: "Asynchronous Message",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(SIGNAL),
        points: &[],
        more: &SIGNAL_MORE,
    },
    MasterSpec {
        identity: (SERIES, 11),
        id: "uml-sequence.reply-message",
        name: "Reply Message",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(DASHED_OPEN),
        points: &[],
        more: &More::NONE,
    },
    MasterSpec {
        identity: (SERIES, 12),
        id: "uml-sequence.create-message",
        name: "Create Message",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(DASHED_OPEN),
        points: &[],
        more: &CREATE_MORE,
    },
    MasterSpec {
        identity: (SERIES, 13),
        id: "uml-sequence.self-message",
        name: "Self Message",
        width: 0.6,
        height: 0.0,
        outline: Outline::Fixed1D { look: ConnectorLook::plain(LineEnd::None, LineEnd::Number(4)), paths: &LOOP_PATH },
        points: &[],
        more: &More::NONE,
    },
    MasterSpec {
        identity: (SERIES, 14),
        id: "uml-sequence.note-link",
        name: "Note Link",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(NOTE_LINK),
        points: &[],
        more: &More::NONE,
    },
];

/// The search words of the master with this id, space separated, or `""`. Plain English: what the figure is called elsewhere and what it is for, and the names of the
/// inventory's sample lists (C68, C75) that its purpose serves: an object lifeline, an actor lifeline, an activation bar, an alternative fragment, an interaction operand, a return message.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "uml-sequence.lifeline" => "object lifeline participant instance head timeline dashed stem",
        "uml-sequence.actor-lifeline" => "actor lifeline participant user person stick figure",
        "uml-sequence.activation" => "activation bar execution occurrence focus of control",
        "uml-sequence.fragment" => {
            "alternative fragment combined interaction operator alt opt optional loop par break critical neg assert strict seq ignore consider"
        }
        "uml-sequence.destruction" => "destroy delete terminate end lifeline cross x",
        "uml-sequence.operand-separator" => "interaction operand else guard divider section alternative",
        "uml-sequence.interaction-use" => "reference ref interaction use sd diagram",
        "uml-sequence.note" => "comment annotation remark",
        "uml-sequence.message" => "synchronous call invocation request",
        "uml-sequence.async-message" => "asynchronous signal send event fire forget",
        "uml-sequence.reply-message" => "return message response answer result",
        "uml-sequence.create-message" => "creation new instantiate construct",
        "uml-sequence.self-message" => "reflexive recursive loop self call to itself",
        "uml-sequence.note-link" => "comment link anchor attach connector dashed",
        _ => "",
    }
}
