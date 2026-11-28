//! BPMN Shapes (`bpmn`, GUID series 15): nineteen masters drawn in code (section 6.2 of the second stencil batch's design, "Stencils: UML, BPMN, C4, network and database", and
//! of the batch's notation spec, which win where they differ: DD6 to DD12).
//!
//! **Original figures.** Each shape is a few outlines and the small markers of [`markers`], made in Rust. BPMN 2.0.2 says what each is in words (an event is a circle with an open centre, a
//! gateway a diamond, an activity a rounded rectangle, a data object a page with a folded corner); no figure of the standard, of the reference application or of another program was traced
//! or copied.
//!
//! * **One stencil of base shapes, and the variants are Shape Data**. The rows are the reference application's, with its words: a task's `BpmnTaskType` holds its
//!   ten words, an event's `BpmnEventType` and `BpmnTriggerOrResult`, a gateway's `BpmnGatewayType`, `BpmnExclusiveType` and `BpmnMarkerVisible`, an activity's `BpmnLoopType`,
//!   `BpmnIsForCompensation`, `BpmnBoundaryType` and `BpmnAdHoc`, a flow's `BpmnConditionType` and `BpmnDirection`. **A word that is no item of its list shows the none marker**: every marker
//!   section is hidden unless its row holds exactly its word.
//! * **A figure is a group of members**: the outline in a member, each form of a marker (hollow or dark) in a member of its own, and the name in the last member, the one typing
//!   on the selected group goes to (`IsTextEditTarget` 0). An event's name sits under it, a task's inside it. A marker that is **dark** (a throwing event's, an end event's, a send task's envelope)
//!   fills with its own line colour; a catching one is outlined and hollow.
//! * **The borders** (DD7). A sub-process or a call activity has a thin, a thick (call), a dotted (**event**: pattern 3, not dashed) or a double (transaction) border by
//!   `BpmnBoundaryType`, each a member of its own, since a member has one line weight and pattern. A non-interrupting event's ring is **dashed** (pattern 2).
//! * **The markers along the bottom of an activity** are centred together: a loop or multi-instance marker, the ad hoc tilde, the compensation marker and, on a sub-process, the
//!   collapsed marker, each 0.2 inch from the next, the row as wide as the markers that are shown. Their places are formulas of the Shape Data.
//! * **Containers.** The Expanded Sub-Process and the Group are container masters (a shape dropped inside joins them). The Expanded Sub-Process is one shape: a thick border is two outlines
//!   drawn inside the first, a transaction's second border one, and its markers stand in a slot each along the bottom (loop, ad hoc, compensation).
//! * **The flows** are connector masters whose `LinePattern` is `GUARD`ed: sequence flow solid, message flow dashed, association and data association dotted, so that a format
//!   command that would change them is refused in words. The sequence flow's begin end is 0, 22 or 23 by `BpmnConditionType`, the association's ends follow `BpmnDirection`.
//! * **The Pool and the Lane are parts of the swimlane structure**: dropped, a pool makes a frame with its name in a strip on the left and one lane, and a lane dropped on a pool is
//!   put into it (`cmd/swimlane.rs`). Their masters here are the pictures the window shows, and what a lane dropped anywhere else is (a shape).
//! * **Search words**, in [`keywords`], include the names of the inventory's samples (Task, Gateway, Start Event, Sequence Flow, End Event, Pool / Lane, Collapsed Sub-Process, Data Store).

use super::container::Side;
use super::group_master::{MemberFrame, MemberSpec};
use super::kit::{self, GRID3, LABEL_BELOW, RING8, SIDES, TIPS4};
use super::{
    CharRow, Co, ConnectorLook, Dress, FieldDef, FieldKind, Flag, LineEnd, MasterSpec, More, Outline, OwnedPath, PartKind, PathSpec, Point, Role,
    Seg, TextBox, fh, fk, fr, fw, pc,
};

mod markers;
#[cfg(test)]
mod tests;

use markers::{
    ASTERISK, BARS_LYING, BARS_UPRIGHT, BLOCK_ARROW, CANCEL, CIRCLED_ENVELOPE, CLOCK, CROSS_X, DISC, DOUBLE_RING_PENTAGON, ENVELOPE, ERROR,
    ESCALATION, GEARS, GRID, Glyph, HAND_GLYPH, LINED_PAGE, LOOP, MULTIPLE, OUTLINED_PLUS, PERSON, PLUS, PLUS_BOX, Place, REWIND, RING,
    RING_PENTAGON, RING_PLUS, SIGNAL, WAVE,
};

/// The series of the stencil's masters in their GUIDs (`D1AC000F-...`).
const SERIES: u8 = 15;

// ---- formulas: what hides a marker ----------------------------------------------------------------------------------------------------

/// `NOT(STRSAME(<row>,"<word>"))` on the group (`Sheet.5`): true, so the section is hidden, unless the row holds exactly the word. A word that is no item of the list hides every marker.
macro_rules! unless {
    ($row:literal, $word:literal) => {
        concat!("NOT(STRSAME(Sheet.5!Prop.", $row, ",\"", $word, "\"))")
    };
}

/// The same on the shape's own cells, for a shape that draws its markers itself (the expanded sub-process).
macro_rules! unless_own {
    ($row:literal, $word:literal) => {
        concat!("NOT(STRSAME(Prop.", $row, ",\"", $word, "\"))")
    };
}

/// An event's marker shows when `BpmnTriggerOrResult` is the word and `BpmnEventType` is one of the kinds the notation allows it for (Table 10.93).
macro_rules! shown_for {
    ($trigger:literal; $first:literal $(, $kind:literal)*) => {
        concat!(
            "NOT(AND(STRSAME(Sheet.5!Prop.BpmnTriggerOrResult,\"", $trigger, "\"),OR(STRSAME(Sheet.5!Prop.BpmnEventType,\"", $first, "\")",
            $(",STRSAME(Sheet.5!Prop.BpmnEventType,\"", $kind, "\")",)*
            ")))"
        )
    };
}

macro_rules! start_any {
    ($t:literal) => {
        shown_for!($t; "Start", "Start (Non-Interrupting)")
    };
}
macro_rules! start_plain {
    ($t:literal) => {
        shown_for!($t; "Start")
    };
}
macro_rules! mid_any {
    ($t:literal) => {
        shown_for!($t; "Intermediate", "Intermediate (Non-Interrupting)")
    };
}
macro_rules! mid_plain {
    ($t:literal) => {
        shown_for!($t; "Intermediate")
    };
}
macro_rules! mid_throwing {
    ($t:literal) => {
        shown_for!($t; "Intermediate (Throwing)")
    };
}
macro_rules! ending {
    ($t:literal) => {
        shown_for!($t; "End")
    };
}

/// A loop or multi-instance marker is shown when `BpmnLoopType` is one of its three words.
macro_rules! loop_on {
    () => {
        "OR(STRSAME(Sheet.5!Prop.BpmnLoopType,\"Standard\"),STRSAME(Sheet.5!Prop.BpmnLoopType,\"Parallel MultiInstance\"),STRSAME(Sheet.5!Prop.BpmnLoopType,\"Sequential MultiInstance\"))"
    };
}

/// How many markers stand along the bottom of a sub-process: the loop, the ad hoc tilde, the compensation and the collapsed marker, which is always there.
macro_rules! row_count {
    () => {
        concat!("(IF(", loop_on!(), ",1,0)+IF(Sheet.5!Prop.BpmnAdHoc,1,0)+IF(Sheet.5!Prop.BpmnIsForCompensation,1,0)+1)")
    };
}

/// The markers of a sub-process stand 0.2 inch apart and are centred together: each one's place is the middle, and the markers before it less half the row. **The 0.2 inch follows the page's drop
/// scale** (`scaled!`).
macro_rules! loop_x {
    () => {
        concat!("Sheet.5!Width*0.5+(0-(", row_count!(), "-1)/2)*", scaled!("0.2"))
    };
}
macro_rules! adhoc_x {
    () => {
        concat!("Sheet.5!Width*0.5+(IF(", loop_on!(), ",1,0)-(", row_count!(), "-1)/2)*", scaled!("0.2"))
    };
}
macro_rules! comp_x {
    () => {
        concat!("Sheet.5!Width*0.5+(IF(", loop_on!(), ",1,0)+IF(Sheet.5!Prop.BpmnAdHoc,1,0)-(", row_count!(), "-1)/2)*", scaled!("0.2"))
    };
}
macro_rules! plus_x {
    () => {
        concat!("Sheet.5!Width*0.5+(", row_count!(), "-1-(", row_count!(), "-1)/2)*", scaled!("0.2"))
    };
}

const fn hide(formula: &'static str) -> Flag {
    Flag::Formula(formula)
}

// ---- Shape Data ------------------------------------------------------------------------------------------------------------------------

/// A fixed list: the row, its label, its items apart at semicolons, its default and its prompt.
const fn pick(row: &'static str, label: &'static str, items: &'static str, value: &'static str, prompt: &'static str) -> FieldDef {
    FieldDef { row, label, kind: FieldKind::FixedList(items), value, prompt }
}

/// A true or false field, false to begin with.
const fn flag(row: &'static str, label: &'static str, prompt: &'static str) -> FieldDef {
    FieldDef { row, label, kind: FieldKind::Boolean, value: "0", prompt }
}

const TASK_TYPES: &str = "None;Abstract;Service;Send;Receive;Instantiating Receive;User;Manual;Business Rule;Script";
const LOOP_TYPES: &str = "None;Standard;Parallel MultiInstance;Sequential MultiInstance";
const BOUNDARY_TYPES: &str = "Default;Call;Event;Transaction";
const START_TRIGGERS: &str = "None;Message;Timer;Conditional;Signal;Multiple;Parallel Multiple;Escalation;Error;Compensation";
const INTERMEDIATE_TRIGGERS: &str = "None;Message;Timer;Escalation;Conditional;Link;Error;Cancel;Compensation;Signal;Multiple;Parallel Multiple";
const END_RESULTS: &str = "None;Message;Escalation;Error;Cancel;Compensation;Signal;Terminate;Multiple";

const LOOP_FIELD: FieldDef =
    pick("BpmnLoopType", "Loop Type", LOOP_TYPES, "None", "None, a standard loop, or a parallel or sequential multi-instance activity");
const COMPENSATION_FIELD: FieldDef = flag("BpmnIsForCompensation", "Is For Compensation", "True for an activity that undoes another");
const AD_HOC_FIELD: FieldDef = flag("BpmnAdHoc", "Ad Hoc", "True for an ad hoc sub-process");

static TASK_DATA: [FieldDef; 3] = [
    pick("BpmnTaskType", "Task Type", TASK_TYPES, "None", "The kind of task: its marker shows in the top left corner"),
    LOOP_FIELD,
    COMPENSATION_FIELD,
];
static SUB_PROCESS_DATA: [FieldDef; 4] = [
    LOOP_FIELD,
    COMPENSATION_FIELD,
    pick("BpmnBoundaryType", "Boundary Type", BOUNDARY_TYPES, "Default", "Default, call (thick), event (dotted) or transaction (double)"),
    AD_HOC_FIELD,
];
static CALL_DATA: [FieldDef; 4] = [
    LOOP_FIELD,
    COMPENSATION_FIELD,
    pick("BpmnBoundaryType", "Boundary Type", BOUNDARY_TYPES, "Call", "Default, call (thick), event (dotted) or transaction (double)"),
    AD_HOC_FIELD,
];
static START_DATA: [FieldDef; 2] = [
    pick("BpmnEventType", "Event Type", "Start;Start (Non-Interrupting)", "Start", "A non-interrupting start event has a dashed circle"),
    pick("BpmnTriggerOrResult", "Trigger Or Result", START_TRIGGERS, "None", "What starts the process"),
];
static INTERMEDIATE_DATA: [FieldDef; 2] = [
    pick(
        "BpmnEventType",
        "Event Type",
        "Intermediate;Intermediate (Non-Interrupting);Intermediate (Throwing)",
        "Intermediate",
        "A non-interrupting event has a dashed circle and a throwing one a dark marker",
    ),
    pick("BpmnTriggerOrResult", "Trigger Or Result", INTERMEDIATE_TRIGGERS, "None", "What the event catches or throws"),
];
static END_DATA: [FieldDef; 2] = [
    pick("BpmnEventType", "Event Type", "End", "End", "An end event has a thick circle"),
    pick("BpmnTriggerOrResult", "Trigger Or Result", END_RESULTS, "None", "What the process ends with"),
];
static GATEWAY_DATA: [FieldDef; 3] = [
    pick(
        "BpmnGatewayType",
        "Gateway Type",
        "Exclusive;Exclusive Event (Instantiate);Inclusive;Parallel;Parallel Event (Instantiate);Complex",
        "Exclusive",
        "The marker inside the diamond",
    ),
    pick("BpmnExclusiveType", "Exclusive Type", "Data;Event", "Data", "An exclusive gateway decides by data or waits for an event"),
    flag("BpmnMarkerVisible", "Marker Visible", "True to draw the X of an exclusive gateway"),
];
static DATA_OBJECT_DATA: [FieldDef; 2] = [
    flag("BpmnCollection", "Collection", "True for a collection of data"),
    pick("BpmnDataRole", "Data Role", "None;Input;Output", "None", "An input has a hollow arrow and an output a dark one"),
];
static SEQUENCE_DATA: [FieldDef; 1] = [pick(
    "BpmnConditionType",
    "Condition Type",
    "None;Expression;Default",
    "None",
    "A conditional flow has a diamond at its start and a default one a slash",
)];
static DIRECTION_DATA: [FieldDef; 1] = [pick("BpmnDirection", "Direction", "None;One;Both", "None", "Which ends have an open head")];
/// A data association is a directed association: its direction begins as One, an open head at its target.
static DATA_DIRECTION_DATA: [FieldDef; 1] = [pick("BpmnDirection", "Direction", "None;One;Both", "One", "Which ends have an open head")];

// ---- the dresses ------------------------------------------------------------------------------------------------------------------------

/// A group of members that edits the text of its last member (the name) and is picked first, and that people join.
const fn group_dress(data: &'static [FieldDef]) -> Dress {
    Dress { data, select_mode: Some(1), text_edit_target: Some(false), obj_type: Some(1), ..Dress::NONE }
}

static TASK_DRESS: Dress = group_dress(&TASK_DATA);
static SUB_PROCESS_DRESS: Dress = group_dress(&SUB_PROCESS_DATA);
static CALL_DRESS: Dress = group_dress(&CALL_DATA);
static START_DRESS: Dress = group_dress(&START_DATA);
static INTERMEDIATE_DRESS: Dress = group_dress(&INTERMEDIATE_DATA);
static END_DRESS: Dress = group_dress(&END_DATA);
static GATEWAY_DRESS: Dress = group_dress(&GATEWAY_DATA);
static DATA_OBJECT_DRESS: Dress = group_dress(&DATA_OBJECT_DATA);

/// The name of a shape: no fill and no line, so that it is only its text. An event's name is small.
static LABEL_DRESS: Dress = Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), ..Dress::NONE };
static SMALL_LABEL_DRESS: Dress =
    Dress { fill_pattern: Some(0), line_pattern: Some(Flag::Off), chars: &[CharRow { size_pt: 9.0, style: 0, color: None }], ..Dress::NONE };
/// A marker drawn in outlines: a thin line.
static OUTLINE_MARK: Dress = Dress { line_weight_pt: Some(0.75), ..Dress::NONE };
/// A marker drawn in lines that are meant to be bold: the loop's bars, a gateway's X, plus and asterisk.
static BOLD_MARK: Dress = Dress { line_weight_pt: Some(2.5), ..Dress::NONE };
static BARS_MARK: Dress = Dress { line_weight_pt: Some(1.5), ..Dress::NONE };
/// A marker that is filled: with **its own line colour**, so as dark as its outline under any theme, and as the line colour a person puts on the figure: a format put on a group reaches the
/// members and not the group's own cells, so a fill that followed the group's line colour would stay black, and a format leaves a fill that is its member's line colour to the line.
static DARK_MARK: Dress = Dress { fill: Some("LineColor"), line_weight_pt: Some(0.5), ..Dress::NONE };
/// A heavy border.
static THICK_BORDER: Dress = Dress { line_weight_pt: Some(2.25), ..Dress::NONE };
/// A dotted border (pattern 3).
static DOTTED_BORDER: Dress = Dress { line_pattern: Some(Flag::Formula("3")), ..Dress::NONE };
/// A second border inside the first: no fill.
static INNER_BORDER: Dress = Dress { fill_pattern: Some(0), ..Dress::NONE };
/// A ring that is dashed for a non-interrupting event.
static START_RING: Dress =
    Dress { line_pattern: Some(Flag::Formula("IF(STRSAME(Sheet.5!Prop.BpmnEventType,\"Start (Non-Interrupting)\"),2,1)")), ..Dress::NONE };
static INTERMEDIATE_RING: Dress =
    Dress { line_pattern: Some(Flag::Formula("IF(STRSAME(Sheet.5!Prop.BpmnEventType,\"Intermediate (Non-Interrupting)\"),2,1)")), ..Dress::NONE };
/// The end event's ring is the thick one.
static END_RING: Dress = Dress { line_weight_pt: Some(3.0), ..Dress::NONE };

// ---- the frames of the members ----------------------------------------------------------------------------------------------------------

const FULL: MemberFrame =
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: "Sheet.5!Height*0.5", width: "Sheet.5!Width", height: "Sheet.5!Height" };
/// A marker at the middle of an event, 0.55 of the circle.
const EVENT_MARK: MemberFrame =
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: "Sheet.5!Height*0.5", width: "Sheet.5!Width*0.55", height: "Sheet.5!Height*0.55" };
/// The inner ring of an intermediate event, 0.04 inch inside the first. **Every fixed length of a frame is written times the page's drop scale** (`scaled!`), so that a figure
/// dropped beyond the range of eight is the same on paper as at 1:1.
const INNER_RING: MemberFrame = MemberFrame::Formulas {
    pin_x: "Sheet.5!Width*0.5",
    pin_y: "Sheet.5!Height*0.5",
    width: concat!("Sheet.5!Width-", scaled!("0.08")),
    height: concat!("Sheet.5!Height-", scaled!("0.08")),
};
/// An event's name: a block 0.8 inch wider than the circle and 0.36 inch tall, from 0.08 to 0.44 inch under it, as shares of the 0.4 inch circle. **The name's member has the box of the figure** and
/// only its text block hangs under it, so that the group's frame is the box of its members, which an `.odg` save keeps (a `draw:g` has no frame of its own). The group's own text block is the same
/// one, to say where the name is (the group holds no text).
const EVENT_TEXT: TextBox = TextBox { x: -1.0, y: -1.1, w: 3.0, h: 0.9 };

/// A task's name: inside the box, a little in from its edges.
const TASK_LABEL: MemberFrame = MemberFrame::Formulas {
    pin_x: "Sheet.5!Width*0.5",
    pin_y: "Sheet.5!Height*0.5",
    width: concat!("Sheet.5!Width-", scaled!("0.1")),
    height: concat!("Sheet.5!Height-", scaled!("0.1")),
};
/// A task's type marker: 0.2 inch, 0.07 inch in from the top left corner.
const TASK_TYPE: MemberFrame = MemberFrame::Formulas {
    pin_x: scaled!("0.17"),
    pin_y: concat!("Sheet.5!Height-", scaled!("0.17")),
    width: scaled!("0.2"),
    height: scaled!("0.2"),
};
/// A marker along the bottom of an activity: 0.16 inch, 0.06 inch up. The places are formulas of the Shape Data (the markers are centred together).
const fn bottom_mark(pin_x: &'static str) -> MemberFrame {
    MemberFrame::Formulas { pin_x, pin_y: scaled!("0.14"), width: scaled!("0.16"), height: scaled!("0.16") }
}
/// A box 0.1 inch narrower and shorter than the activity, for the inner border of a transaction.
const INSET_BOX: MemberFrame = MemberFrame::Formulas {
    pin_x: "Sheet.5!Width*0.5",
    pin_y: "Sheet.5!Height*0.5",
    width: concat!("Sheet.5!Width-", scaled!("0.1")),
    height: concat!("Sheet.5!Height-", scaled!("0.1")),
};

const GATEWAY_THICK: MemberFrame =
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: "Sheet.5!Height*0.5", width: "Sheet.5!Width*0.45", height: "Sheet.5!Height*0.45" };
const GATEWAY_THIN: MemberFrame = MemberFrame::Formulas {
    pin_x: "Sheet.5!Width*0.5",
    pin_y: "Sheet.5!Height*0.5",
    width: "Sheet.5!Width*0.66667",
    height: "Sheet.5!Height*0.66667",
};
/// A gateway's name: a block 1.2 inch wide and 0.36 inch tall, from 0.08 to 0.44 inch under the 0.6 inch diamond.
const GATEWAY_TEXT: TextBox = TextBox { x: -0.5, y: -0.733_333_333, w: 2.0, h: 0.6 };

const DATA_ARROW: MemberFrame = MemberFrame::Formulas {
    pin_x: scaled!("0.12"),
    pin_y: concat!("Sheet.5!Height-", scaled!("0.12")),
    width: scaled!("0.16"),
    height: scaled!("0.16"),
};
const DATA_BARS: MemberFrame =
    MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: scaled!("0.11"), width: scaled!("0.14"), height: scaled!("0.14") };
/// A data object's name: a block 1.2 inch wide and 0.36 inch tall, from 0.08 to 0.44 inch under the 0.5 by 0.65 inch page.
const DATA_TEXT: TextBox = TextBox { x: -0.7, y: -0.676_923_077, w: 2.4, h: 0.553_846_154 };

const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

// ---- outlines made by code ----------------------------------------------------------------------------------------------------------------

/// The paths with a `NoShow` of their own.
fn hidden_by(paths: Vec<OwnedPath>, flag: Flag) -> Vec<OwnedPath> {
    paths.into_iter().map(|p| OwnedPath { no_show: flag, ..p }).collect()
}

/// The rounded box of an activity: a corner of 0.1 inch.
fn body() -> Vec<OwnedPath> {
    hidden_by(kit::rounded_box(0.1), hide("OR(STRSAME(Sheet.5!Prop.BpmnBoundaryType,\"Call\"),STRSAME(Sheet.5!Prop.BpmnBoundaryType,\"Event\"))"))
}

/// The same box when it is a plain task: always shown.
fn task_body() -> Vec<OwnedPath> {
    kit::rounded_box(0.1)
}

fn body_thick() -> Vec<OwnedPath> {
    hidden_by(kit::rounded_box(0.1), hide(unless!("BpmnBoundaryType", "Call")))
}

fn body_dotted() -> Vec<OwnedPath> {
    hidden_by(kit::rounded_box(0.1), hide(unless!("BpmnBoundaryType", "Event")))
}

fn inner_border() -> Vec<OwnedPath> {
    hidden_by(kit::rounded_box(0.06), hide(unless!("BpmnBoundaryType", "Transaction")))
}

/// What a task's type marker shows by `BpmnTaskType`: every type but send, outlined.
static TASK_TYPE_MARKS: [(&Glyph, Flag); 7] = [
    (&GEARS, hide(unless!("BpmnTaskType", "Service"))),
    (&ENVELOPE, hide(unless!("BpmnTaskType", "Receive"))),
    (&CIRCLED_ENVELOPE, hide(unless!("BpmnTaskType", "Instantiating Receive"))),
    (&PERSON, hide(unless!("BpmnTaskType", "User"))),
    (&HAND_GLYPH, hide(unless!("BpmnTaskType", "Manual"))),
    (&GRID, hide(unless!("BpmnTaskType", "Business Rule"))),
    (&LINED_PAGE, hide(unless!("BpmnTaskType", "Script"))),
];
/// A send task's envelope is dark.
static TASK_SENT_MARKS: [(&Glyph, Flag); 1] = [(&ENVELOPE, hide(unless!("BpmnTaskType", "Send")))];
static LOOP_MARKS: [(&Glyph, Flag); 3] = [
    (&LOOP, hide(unless!("BpmnLoopType", "Standard"))),
    (&BARS_UPRIGHT, hide(unless!("BpmnLoopType", "Parallel MultiInstance"))),
    (&BARS_LYING, hide(unless!("BpmnLoopType", "Sequential MultiInstance"))),
];
static COMPENSATION_MARKS: [(&Glyph, Flag); 1] = [(&REWIND, hide("NOT(Sheet.5!Prop.BpmnIsForCompensation)"))];
static AD_HOC_MARKS: [(&Glyph, Flag); 1] = [(&WAVE, hide("NOT(Sheet.5!Prop.BpmnAdHoc)"))];
static COLLAPSED_MARKS: [(&Glyph, Flag); 1] = [(&PLUS_BOX, Flag::Off)];

fn task_type() -> Vec<OwnedPath> {
    markers::member(&TASK_TYPE_MARKS, false, Place::Share)
}
fn task_sent() -> Vec<OwnedPath> {
    markers::member(&TASK_SENT_MARKS, true, Place::Share)
}
fn loop_marks() -> Vec<OwnedPath> {
    markers::member(&LOOP_MARKS, false, Place::Share)
}
fn compensation_marks() -> Vec<OwnedPath> {
    markers::member(&COMPENSATION_MARKS, false, Place::Share)
}
fn ad_hoc_marks() -> Vec<OwnedPath> {
    markers::member(&AD_HOC_MARKS, false, Place::Share)
}
fn collapsed_marks() -> Vec<OwnedPath> {
    markers::member(&COLLAPSED_MARKS, false, Place::Share)
}

/// The triggers of a start event: all hollow. A non-interrupting one may not be an error or a compensation (Table 10.93).
static START_MARKS: [(&Glyph, Flag); 9] = [
    (&ENVELOPE, hide(start_any!("Message"))),
    (&CLOCK, hide(start_any!("Timer"))),
    (&LINED_PAGE, hide(start_any!("Conditional"))),
    (&SIGNAL, hide(start_any!("Signal"))),
    (&MULTIPLE, hide(start_any!("Multiple"))),
    (&OUTLINED_PLUS, hide(start_any!("Parallel Multiple"))),
    (&ESCALATION, hide(start_any!("Escalation"))),
    (&ERROR, hide(start_plain!("Error"))),
    (&REWIND, hide(start_plain!("Compensation"))),
];
/// The triggers of an intermediate event that catches (outlined). A boundary event holds an interrupting trigger an event on the sequence does not: the master keeps no attachment, so it
/// allows the union.
static INTERMEDIATE_MARKS: [(&Glyph, Flag); 11] = [
    (&ENVELOPE, hide(mid_any!("Message"))),
    (&CLOCK, hide(mid_any!("Timer"))),
    (&LINED_PAGE, hide(mid_any!("Conditional"))),
    (&SIGNAL, hide(mid_any!("Signal"))),
    (&MULTIPLE, hide(mid_any!("Multiple"))),
    (&OUTLINED_PLUS, hide(mid_any!("Parallel Multiple"))),
    (&ESCALATION, hide(mid_any!("Escalation"))),
    (&BLOCK_ARROW, hide(mid_plain!("Link"))),
    (&ERROR, hide(mid_plain!("Error"))),
    (&CANCEL, hide(mid_plain!("Cancel"))),
    (&REWIND, hide(mid_plain!("Compensation"))),
];
/// The triggers of an intermediate event that throws (dark).
static THROWING_MARKS: [(&Glyph, Flag); 6] = [
    (&ENVELOPE, hide(mid_throwing!("Message"))),
    (&ESCALATION, hide(mid_throwing!("Escalation"))),
    (&REWIND, hide(mid_throwing!("Compensation"))),
    (&BLOCK_ARROW, hide(mid_throwing!("Link"))),
    (&SIGNAL, hide(mid_throwing!("Signal"))),
    (&MULTIPLE, hide(mid_throwing!("Multiple"))),
];
/// The results of an end event: all dark.
static END_MARKS: [(&Glyph, Flag); 8] = [
    (&ENVELOPE, hide(ending!("Message"))),
    (&ESCALATION, hide(ending!("Escalation"))),
    (&ERROR, hide(ending!("Error"))),
    (&CANCEL, hide(ending!("Cancel"))),
    (&REWIND, hide(ending!("Compensation"))),
    (&SIGNAL, hide(ending!("Signal"))),
    (&DISC, hide(ending!("Terminate"))),
    (&MULTIPLE, hide(ending!("Multiple"))),
];

fn start_marks() -> Vec<OwnedPath> {
    markers::member(&START_MARKS, false, Place::Share)
}
fn intermediate_marks() -> Vec<OwnedPath> {
    markers::member(&INTERMEDIATE_MARKS, false, Place::Share)
}
fn throwing_marks() -> Vec<OwnedPath> {
    markers::member(&THROWING_MARKS, true, Place::Share)
}
fn end_marks() -> Vec<OwnedPath> {
    markers::member(&END_MARKS, true, Place::Share)
}

/// A gateway's bold markers (the X of an exclusive gateway that shows it, the plus, the asterisk and the circle) and its thin ones (the event-based forms: a pentagon or a plus in rings).
static GATEWAY_BOLD_MARKS: [(&Glyph, Flag); 4] = [
    (
        &CROSS_X,
        hide(
            "NOT(AND(STRSAME(Sheet.5!Prop.BpmnGatewayType,\"Exclusive\"),STRSAME(Sheet.5!Prop.BpmnExclusiveType,\"Data\"),Sheet.5!Prop.BpmnMarkerVisible))",
        ),
    ),
    (&PLUS, hide(unless!("BpmnGatewayType", "Parallel"))),
    (&ASTERISK, hide(unless!("BpmnGatewayType", "Complex"))),
    (&RING, hide(unless!("BpmnGatewayType", "Inclusive"))),
];
static GATEWAY_THIN_MARKS: [(&Glyph, Flag); 3] = [
    (&DOUBLE_RING_PENTAGON, hide("NOT(AND(STRSAME(Sheet.5!Prop.BpmnGatewayType,\"Exclusive\"),STRSAME(Sheet.5!Prop.BpmnExclusiveType,\"Event\")))")),
    (&RING_PENTAGON, hide(unless!("BpmnGatewayType", "Exclusive Event (Instantiate)"))),
    (&RING_PLUS, hide(unless!("BpmnGatewayType", "Parallel Event (Instantiate)"))),
];

fn gateway_bold() -> Vec<OwnedPath> {
    markers::member(&GATEWAY_BOLD_MARKS, false, Place::Share)
}
fn gateway_thin() -> Vec<OwnedPath> {
    markers::member(&GATEWAY_THIN_MARKS, false, Place::Share)
}

static DATA_INPUT_MARKS: [(&Glyph, Flag); 1] = [(&BLOCK_ARROW, hide(unless!("BpmnDataRole", "Input")))];
static DATA_OUTPUT_MARKS: [(&Glyph, Flag); 1] = [(&BLOCK_ARROW, hide(unless!("BpmnDataRole", "Output")))];
static DATA_COLLECTION_MARKS: [(&Glyph, Flag); 1] = [(&BARS_UPRIGHT, hide("NOT(Sheet.5!Prop.BpmnCollection)"))];

fn data_input() -> Vec<OwnedPath> {
    markers::member(&DATA_INPUT_MARKS, false, Place::Share)
}
fn data_output() -> Vec<OwnedPath> {
    markers::member(&DATA_OUTPUT_MARKS, true, Place::Share)
}
fn data_collection() -> Vec<OwnedPath> {
    markers::member(&DATA_COLLECTION_MARKS, false, Place::Share)
}

/// The page of a data object: a document with a folded corner.
fn data_page() -> Vec<OwnedPath> {
    kit::folded_document()
}

/// An envelope as a shape of its own: a box and the flap's two lines.
fn message_outline() -> Vec<OwnedPath> {
    let (z, w, h) = (fw(0.0), fw(1.0), fh(1.0));
    vec![
        OwnedPath {
            fill: true,
            segs: vec![Seg::Move(z, fh(0.0)), Seg::Line(w, fh(0.0)), Seg::Line(w, h), Seg::Line(z, h), Seg::Line(z, fh(0.0))],
            no_show: Flag::Off,
            no_line: Flag::Off,
        },
        OwnedPath { fill: false, segs: vec![Seg::Move(z, h), Seg::Line(fw(0.5), fh(0.5)), Seg::Line(w, h)], no_show: Flag::Off, no_line: Flag::Off },
    ]
}

/// A vertical cylinder.
fn data_store_outline() -> Vec<OwnedPath> {
    kit::cylinder_v()
}

/// A group's frame: a rounded box with a corner of 0.15 inch.
fn group_outline() -> Vec<OwnedPath> {
    kit::rounded_box(0.15)
}

/// The midpoint of a quarter circle is this share of the radius from its bounding corner.
const QUARTER: f64 = 0.292_893_218_813_452_4;

/// A rounded box drawn only as a line, `inset` inches inside the box and with corners of `rad` inches: a thick border is three of them side by side, and a transaction's second border one.
fn rounded_stroke(inset: f64, rad: f64, hidden: Flag) -> OwnedPath {
    let (width, height) = (fw(1.0), fh(1.0));
    let (left, bottom, right, top) = (fk(inset), fk(inset), width.plus(fk(-inset)), height.plus(fk(-inset)));
    let (r, q) = (fr(rad, 1.0), fr(rad, QUARTER));
    let (nr, nq) = (fr(rad, -1.0), fr(rad, -QUARTER));
    let segs = vec![
        Seg::Move(left.plus(r), bottom),
        Seg::Line(right.plus(nr), bottom),
        Seg::Arc { to: (right, bottom.plus(r)), via: (right.plus(nq), bottom.plus(q)) },
        Seg::Line(right, top.plus(nr)),
        Seg::Arc { to: (right.plus(nr), top), via: (right.plus(nq), top.plus(nq)) },
        Seg::Line(left.plus(r), top),
        Seg::Arc { to: (left, top.plus(nr)), via: (left.plus(q), top.plus(nq)) },
        Seg::Line(left, bottom.plus(r)),
        Seg::Arc { to: (left.plus(r), bottom), via: (left.plus(q), bottom.plus(q)) },
    ];
    OwnedPath { fill: false, segs, no_show: hidden, no_line: Flag::Off }
}

/// The expanded sub-process: its rounded box, a second and a third outline inside it for a call (a thick border), one for a transaction, and its markers, a slot each along the bottom.
fn expanded_outline() -> Vec<OwnedPath> {
    let call = hide(unless_own!("BpmnBoundaryType", "Call"));
    let mut paths = kit::rounded_box(0.1);
    paths.push(rounded_stroke(0.012, 0.1, call));
    paths.push(rounded_stroke(0.024, 0.1, call));
    paths.push(rounded_stroke(0.04, 0.1, hide(unless_own!("BpmnBoundaryType", "Transaction"))));
    let slot = |dx: f64| Place::Fixed { x: fw(0.5).plus(fk(dx - 0.08)), y: fk(0.06), size: 0.16 };
    let loops: [(&Glyph, Flag); 3] = [
        (&LOOP, hide(unless_own!("BpmnLoopType", "Standard"))),
        (&BARS_UPRIGHT, hide(unless_own!("BpmnLoopType", "Parallel MultiInstance"))),
        (&BARS_LYING, hide(unless_own!("BpmnLoopType", "Sequential MultiInstance"))),
    ];
    paths.extend(markers::member(&loops, false, slot(-0.2)));
    paths.extend(markers::member(&[(&WAVE, hide("NOT(Prop.BpmnAdHoc)"))], false, slot(0.0)));
    paths.extend(markers::member(&[(&REWIND, hide("NOT(Prop.BpmnIsForCompensation)"))], false, slot(0.2)));
    paths
}

// ---- the members of each group master -----------------------------------------------------------------------------------------------------

const fn member(name: &'static str, frame: MemberFrame, outline: Outline, dress: &'static Dress) -> MemberSpec {
    MemberSpec { name, frame, outline, points: &[], dress: Some(dress) }
}

/// A name that hangs under its figure: a box of the figure's size that draws nothing (its member has no fill and no line) and whose text block is under it.
const fn name_under(text: TextBox) -> Outline {
    Outline::Paths { paths: &BOX_PATH, text }
}

const fn marks(build: fn() -> Vec<OwnedPath>) -> Outline {
    Outline::Code { build, text: TextBox::FULL }
}

static TASK_MEMBERS: [MemberSpec; 6] = [
    member("Body", FULL, marks(task_body), &Dress::NONE),
    member("Type", TASK_TYPE, marks(task_type), &OUTLINE_MARK),
    member("Type (dark)", TASK_TYPE, marks(task_sent), &DARK_MARK),
    member(
        "Loop",
        bottom_mark(concat!("Sheet.5!Width*0.5-IF(Sheet.5!Prop.BpmnIsForCompensation,", scaled!("0.1"), ",0)")),
        marks(loop_marks),
        &BARS_MARK,
    ),
    member(
        "Compensation",
        bottom_mark(concat!("Sheet.5!Width*0.5+IF(", loop_on!(), ",", scaled!("0.1"), ",0)")),
        marks(compensation_marks),
        &OUTLINE_MARK,
    ),
    member("Label", TASK_LABEL, Outline::Polygon(RECT), &LABEL_DRESS),
];

/// The members of a sub-process and of a call activity: the four borders, the four markers along the bottom and the name.
static SUB_PROCESS_MEMBERS: [MemberSpec; 9] = [
    member("Body", FULL, marks(body), &Dress::NONE),
    member("Body (thick)", FULL, marks(body_thick), &THICK_BORDER),
    member("Body (dotted)", FULL, marks(body_dotted), &DOTTED_BORDER),
    member("Inner border", INSET_BOX, marks(inner_border), &INNER_BORDER),
    member("Loop", bottom_mark(loop_x!()), marks(loop_marks), &BARS_MARK),
    member("Ad hoc", bottom_mark(adhoc_x!()), marks(ad_hoc_marks), &OUTLINE_MARK),
    member("Compensation", bottom_mark(comp_x!()), marks(compensation_marks), &OUTLINE_MARK),
    member("Collapsed", bottom_mark(plus_x!()), marks(collapsed_marks), &OUTLINE_MARK),
    member("Label", TASK_LABEL, Outline::Polygon(RECT), &LABEL_DRESS),
];

static START_MEMBERS: [MemberSpec; 3] = [
    member("Ring", FULL, Outline::Ellipse, &START_RING),
    member("Marker", EVENT_MARK, marks(start_marks), &OUTLINE_MARK),
    member("Label", FULL, name_under(EVENT_TEXT), &SMALL_LABEL_DRESS),
];
static INTERMEDIATE_MEMBERS: [MemberSpec; 5] = [
    member("Ring", FULL, Outline::Ellipse, &INTERMEDIATE_RING),
    member("Inner ring", INNER_RING, Outline::Ellipse, &INTERMEDIATE_RING),
    member("Marker", EVENT_MARK, marks(intermediate_marks), &OUTLINE_MARK),
    member("Marker (dark)", EVENT_MARK, marks(throwing_marks), &DARK_MARK),
    member("Label", FULL, name_under(EVENT_TEXT), &SMALL_LABEL_DRESS),
];
static END_MEMBERS: [MemberSpec; 3] = [
    member("Ring", FULL, Outline::Ellipse, &END_RING),
    member("Marker (dark)", EVENT_MARK, marks(end_marks), &DARK_MARK),
    member("Label", FULL, name_under(EVENT_TEXT), &SMALL_LABEL_DRESS),
];
static GATEWAY_MEMBERS: [MemberSpec; 4] = [
    member("Diamond", FULL, marks(kit::diamond), &Dress::NONE),
    member("Marker", GATEWAY_THICK, marks(gateway_bold), &BOLD_MARK),
    member("Marker (thin)", GATEWAY_THIN, marks(gateway_thin), &OUTLINE_MARK),
    member("Label", FULL, name_under(GATEWAY_TEXT), &SMALL_LABEL_DRESS),
];
static DATA_OBJECT_MEMBERS: [MemberSpec; 5] = [
    member("Page", FULL, marks(data_page), &Dress::NONE),
    member("Arrow", DATA_ARROW, marks(data_input), &OUTLINE_MARK),
    member("Arrow (dark)", DATA_ARROW, marks(data_output), &DARK_MARK),
    member("Collection", DATA_BARS, marks(data_collection), &BARS_MARK),
    member("Label", FULL, name_under(DATA_TEXT), &SMALL_LABEL_DRESS),
];

// ---- the figures that are one shape ----------------------------------------------------------------------------------------------------------

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

/// The box of a member that is only a text block.
static BOX_PATH: [PathSpec; 1] = [PathSpec {
    fill: true,
    segs: &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

/// The pool as the window shows it: a box and the line beside its name strip, 0.3 inch in from the left.
static POOL_PATHS: [PathSpec; 2] = [
    PathSpec {
        fill: true,
        segs: &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)],
        no_show: Flag::Off,
        no_line: Flag::Off,
    },
    PathSpec { fill: false, segs: &[Seg::Move(fk(0.3), Z), Seg::Line(fk(0.3), H)], no_show: Flag::Off, no_line: Flag::Off },
];

/// The length of a bracket's arm as a share of the width: 0.15 inch of the 1.6 inch box. **A share, and not a fixed length**, so that an `.odg` writes the outline as it is drawn (a length beside
/// the size is a form it does not write yet): the arms grow and shrink with the width.
const ARM: Co = fw(0.09375);

/// A text annotation: an open bracket, its spine on the left and an arm at the top and at the bottom.
static BRACKET: [PathSpec; 1] = [PathSpec {
    fill: false,
    segs: &[Seg::Move(ARM, H), Seg::Line(Z, H), Seg::Line(Z, Z), Seg::Line(ARM, Z)],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];
/// The bracket's middle, its two corners and the ends of its arms.
static BRACKET_POINTS: [Point; 5] = [pc(Z, fh(0.5), -1.0, 0.0), pc(Z, H, 0.0, 1.0), pc(Z, Z, 0.0, -1.0), pc(ARM, H, 0.0, 1.0), pc(ARM, Z, 0.0, -1.0)];
/// The text goes to the right of the bracket.
const ANNOTATION_TEXT: TextBox = TextBox { x: 0.1, y: 0.0, w: 0.9, h: 1.0 };

/// A name at the top of a container.
const TOP_STRIP: TextBox = TextBox { x: 0.0, y: 0.88, w: 1.0, h: 0.12 };

static EXPANDED_DRESS: Dress = Dress {
    data: &SUB_PROCESS_DATA,
    line_pattern: Some(Flag::Formula("IF(STRSAME(Prop.BpmnBoundaryType,\"Event\"),3,1)")),
    obj_type: Some(1),
    role: Role::Container { heading: Side::Top, margin_in: 0.2 },
    ..Dress::NONE
};
/// The Group: dash-dot (4), no fill, a container with its category name at the top.
static GROUP_DRESS: Dress = Dress {
    fill_pattern: Some(0),
    line_pattern: Some(Flag::Formula("4")),
    obj_type: Some(1),
    role: Role::Container { heading: Side::Top, margin_in: 0.2 },
    ..Dress::NONE
};
static ANNOTATION_DRESS: Dress = Dress { obj_type: Some(1), ..Dress::NONE };
/// A data store, a message: shapes people join, with their name under them.
static FIGURE_DRESS: Dress = Dress { obj_type: Some(1), ..Dress::NONE };
/// The pool and the lane are parts of a structure of swimlanes, which no route goes round (the reference's swimlanes hold 4).
static POOL_DRESS: Dress = Dress { obj_type: Some(4), role: Role::Part(PartKind::Pool), ..Dress::NONE };
static LANE_DRESS: Dress = Dress { obj_type: Some(4), role: Role::Part(PartKind::Lane), ..Dress::NONE };

// ---- the flows -----------------------------------------------------------------------------------------------------------------------------

/// A flow's look: its ends, the pattern guarded, the page's route.
const fn flow(begin: LineEnd, end: LineEnd, pattern: u8) -> ConnectorLook {
    ConnectorLook { pattern, guard_pattern: true, ..ConnectorLook::plain(begin, end) }
}

const CONDITION_BEGIN: &str = "IF(STRSAME(Prop.BpmnConditionType,\"Expression\"),22,IF(STRSAME(Prop.BpmnConditionType,\"Default\"),23,0))";
const DIRECTION_BEGIN: &str = "IF(STRSAME(Prop.BpmnDirection,\"Both\"),3,0)";
const DIRECTION_END: &str = "IF(OR(STRSAME(Prop.BpmnDirection,\"One\"),STRSAME(Prop.BpmnDirection,\"Both\")),3,0)";

static SEQUENCE_DRESS: Dress = Dress { data: &SEQUENCE_DATA, ..Dress::NONE };
static ASSOCIATION_DRESS: Dress = Dress { data: &DIRECTION_DATA, ..Dress::NONE };
static DATA_ASSOCIATION_DRESS: Dress = Dress { data: &DATA_DIRECTION_DATA, ..Dress::NONE };

// ---- the masters -----------------------------------------------------------------------------------------------------------------------------

const fn more(dress: &'static Dress) -> More {
    More { dress: Some(dress), hidden: false, brings: &[] }
}

static TASK_MORE: More = more(&TASK_DRESS);
static GATEWAY_MORE: More = more(&GATEWAY_DRESS);
static START_MORE: More = more(&START_DRESS);
static END_MORE: More = more(&END_DRESS);
static INTERMEDIATE_MORE: More = more(&INTERMEDIATE_DRESS);
static SUB_PROCESS_MORE: More = more(&SUB_PROCESS_DRESS);
static EXPANDED_MORE: More = more(&EXPANDED_DRESS);
static CALL_MORE: More = more(&CALL_DRESS);
static DATA_OBJECT_MORE: More = more(&DATA_OBJECT_DRESS);
static DATA_STORE_MORE: More = more(&FIGURE_DRESS);
static MESSAGE_MORE: More = more(&FIGURE_DRESS);
static GROUP_MORE: More = more(&GROUP_DRESS);
static ANNOTATION_MORE: More = more(&ANNOTATION_DRESS);
static POOL_MORE: More = more(&POOL_DRESS);
static LANE_MORE: More = more(&LANE_DRESS);
static SEQUENCE_MORE: More = more(&SEQUENCE_DRESS);
static ASSOCIATION_MORE: More = more(&ASSOCIATION_DRESS);
static DATA_ASSOCIATION_MORE: More = more(&DATA_ASSOCIATION_DRESS);

const fn master(
    at: u16,
    id: &'static str,
    name: &'static str,
    size: (f64, f64),
    outline: Outline,
    points: &'static [Point],
    more: &'static More,
) -> MasterSpec {
    MasterSpec { identity: (SERIES, at), id, name, width: size.0, height: size.1, outline, points, more }
}

const fn group(members: &'static [MemberSpec], text: TextBox) -> Outline {
    Outline::Group { members, text }
}

/// The masters, in the order the Shapes window lists them. **The Quick Shapes are the first four that are no lines: Task, Gateway, Start Event, End Event.**
pub(super) static MASTERS: [MasterSpec; 19] = [
    master(1, "bpmn.task", "Task", (1.4, 0.9), group(&TASK_MEMBERS, TextBox::FULL), &GRID3, &TASK_MORE),
    master(2, "bpmn.gateway", "Gateway", (0.6, 0.6), group(&GATEWAY_MEMBERS, GATEWAY_TEXT), &TIPS4, &GATEWAY_MORE),
    master(3, "bpmn.start-event", "Start Event", (0.4, 0.4), group(&START_MEMBERS, EVENT_TEXT), &RING8, &START_MORE),
    master(4, "bpmn.end-event", "End Event", (0.4, 0.4), group(&END_MEMBERS, EVENT_TEXT), &RING8, &END_MORE),
    master(5, "bpmn.intermediate-event", "Intermediate Event", (0.4, 0.4), group(&INTERMEDIATE_MEMBERS, EVENT_TEXT), &RING8, &INTERMEDIATE_MORE),
    master(6, "bpmn.sub-process", "Sub-Process", (1.4, 0.9), group(&SUB_PROCESS_MEMBERS, TextBox::FULL), &GRID3, &SUB_PROCESS_MORE),
    master(
        7,
        "bpmn.expanded-sub-process",
        "Expanded Sub-Process",
        (3.0, 2.0),
        Outline::Code { build: expanded_outline, text: TOP_STRIP },
        &GRID3,
        &EXPANDED_MORE,
    ),
    master(8, "bpmn.call-activity", "Call Activity", (1.4, 0.9), group(&SUB_PROCESS_MEMBERS, TextBox::FULL), &GRID3, &CALL_MORE),
    master(9, "bpmn.data-object", "Data Object", (0.5, 0.65), group(&DATA_OBJECT_MEMBERS, DATA_TEXT), &SIDES, &DATA_OBJECT_MORE),
    master(10, "bpmn.data-store", "Data Store", (0.7, 0.6), Outline::Code { build: data_store_outline, text: LABEL_BELOW }, &SIDES, &DATA_STORE_MORE),
    master(11, "bpmn.message", "Message", (0.6, 0.4), Outline::Code { build: message_outline, text: LABEL_BELOW }, &SIDES, &MESSAGE_MORE),
    master(12, "bpmn.group", "Group", (2.5, 1.6), Outline::Code { build: group_outline, text: TOP_STRIP }, &GRID3, &GROUP_MORE),
    master(
        13,
        "bpmn.text-annotation",
        "Text Annotation",
        (1.6, 0.6),
        Outline::Paths { paths: &BRACKET, text: ANNOTATION_TEXT },
        &BRACKET_POINTS,
        &ANNOTATION_MORE,
    ),
    master(14, "bpmn.pool", "Pool", (8.0, 2.0), Outline::Paths { paths: &POOL_PATHS, text: TextBox::FULL }, &SIDES, &POOL_MORE),
    master(15, "bpmn.lane", "Lane", (7.6, 1.5), Outline::Polygon(RECT), &SIDES, &LANE_MORE),
    master(
        16,
        "bpmn.sequence-flow",
        "Sequence Flow",
        (1.5, 0.0),
        Outline::Connector(flow(LineEnd::Formula { formula: CONDITION_BEGIN, value: 0 }, LineEnd::Number(4), 1)),
        &[],
        &SEQUENCE_MORE,
    ),
    master(
        17,
        "bpmn.message-flow",
        "Message Flow",
        (1.5, 0.0),
        Outline::Connector(flow(LineEnd::Number(20), LineEnd::Number(16), 2)),
        &[],
        &More::NONE,
    ),
    master(
        18,
        "bpmn.association",
        "Association",
        (1.5, 0.0),
        Outline::Connector(flow(LineEnd::Formula { formula: DIRECTION_BEGIN, value: 0 }, LineEnd::Formula { formula: DIRECTION_END, value: 0 }, 3)),
        &[],
        &ASSOCIATION_MORE,
    ),
    master(
        19,
        "bpmn.data-association",
        "Data Association",
        (1.5, 0.0),
        Outline::Connector(flow(LineEnd::Formula { formula: DIRECTION_BEGIN, value: 0 }, LineEnd::Formula { formula: DIRECTION_END, value: 3 }, 3)),
        &[],
        &DATA_ASSOCIATION_MORE,
    ),
];

/// The search words of the master with this id, space separated, or `""`. The names of the inventory's samples (C5: Task, Gateway, Start Event, Sequence Flow, End Event, Pool / Lane,
/// Collapsed Sub-Process, Data Store) are among them, and the words are the notation's own.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "bpmn.task" => "activity work step action user service send receive manual script business rule abstract business process",
        "bpmn.gateway" => "decision exclusive inclusive parallel complex event based xor and or fork join split merge diamond",
        "bpmn.start-event" => "begin trigger timer message conditional signal multiple escalation error compensation circle",
        "bpmn.end-event" => "finish terminate result message signal error cancel compensation escalation circle",
        "bpmn.intermediate-event" => "catch throw boundary timer message link signal escalation conditional cancel compensation error circle",
        "bpmn.sub-process" => "collapsed sub-process subprocess embedded event transaction ad hoc activity plus",
        "bpmn.expanded-sub-process" => "expanded sub-process subprocess embedded event transaction ad hoc container",
        "bpmn.call-activity" => "call reusable global activity thick sub-process subprocess",
        "bpmn.data-object" => "data input output collection document artifact",
        "bpmn.data-store" => "data store database storage cylinder repository",
        "bpmn.message" => "message envelope mail communication",
        "bpmn.group" => "group category grouping dash dot container artifact",
        "bpmn.text-annotation" => "text annotation note comment bracket artifact",
        "bpmn.pool" => "pool lane participant swimlane swim lane partition organisation organization actor",
        "bpmn.lane" => "pool lane swimlane swim lane partition role responsibility",
        "bpmn.sequence-flow" => "sequence flow arrow connector conditional default control flow",
        "bpmn.message-flow" => "message flow connector between pools dashed envelope communication",
        "bpmn.association" => "association dotted line link artifact annotation connector",
        "bpmn.data-association" => "data association input output dotted arrow connector",
        _ => "",
    }
}
