//! UML Use Case (`uml-usecase`, GUID series 10): nine masters of the use case diagram, based on UML 2.5.1.
//! The figures are the project's own, drawn in code from the notation's generic shapes (a stick figure, an ellipse, a rectangle and lines) and nothing of the standard's figures, of
//! the reference app's or of another application's is traced or copied.
//!
//! * **Actor**: the kit's stick figure with its name in a block under it (`kit::LABEL_BELOW`). Its connection points are five on the figure: the top of the head, both hands and both feet (the design
//!   says four: the middle of the feet is not on a figure whose legs part, so each foot has its own).
//! * **Use Case**: an ellipse with its name inside and eight points round it.
//! * **Subject**: a rectangle with its name at the top left and a **container** (its master has the container role): a use case dropped inside it joins it, and a move of the subject takes
//!   its members. The name's block is the whole box, flush left and starting at the top, so it stays at the top left at any size.
//! * **Note**: the dog-eared box of the Class stencil, with the same plain text.
//! * **Lines**: association (none), include and extend (dashed, an open head, 3, with their keyword as the line's own text), generalization (a hollow triangle, 16)
//!   and the note link (dashed). They are **straight** (ours: a use case diagram's lines run between figures at any angle, where the class diagram's are at right angles).
//!
//! **Ours** (nothing in the notation says): the sizes, the default names, the search words (the inventory's C69 and C75 names, Subsystem among them) and the order, which puts the Quick Shapes (Actor,
//! Use Case, Subject, Note) first. **Not built:** extension points and the use case's compartments (an extension point is typed in the use case's name), and the actor drawn as a classifier
//! rectangle with `«actor»` (allows it).

use super::container::Side;
use super::dress::{CellSet, RowCell};
use super::kit::{self, GRID3, RING8};
use super::uml_class::NOTE_MORE;
use super::{CharRow, ConnectorLook, Dress, LineEnd, MasterSpec, More, Outline, Point, Role, Route, TextBox, TextSpec, pt};

/// The GUID series of this stencil's masters (design section 4.1).
const SERIES: u8 = 10;

/// 10 point plain type.
const PLAIN: [CharRow; 1] = [CharRow { size_pt: 10.0, style: 0, color: None }];
/// 10 point bold type: the subject's name.
const BOLD: [CharRow; 1] = [CharRow { size_pt: 10.0, style: 1, color: None }];
/// 9 point plain type: a line's keyword.
const SMALL: [CharRow; 1] = [CharRow { size_pt: 9.0, style: 0, color: None }];

/// Flush left (`HorzAlign` 0 of the first paragraph).
const LEFT: RowCell = RowCell { section: "Paragraph", ix: 0, cell: "HorzAlign", set: CellSet::Number(0.0) };

static ACTOR_DRESS: Dress = Dress { text: TextSpec::Plain("Actor"), chars: &PLAIN, obj_type: Some(1), ..Dress::NONE };
static ACTOR_MORE: More = More { dress: Some(&ACTOR_DRESS), hidden: false, brings: &[] };

/// The top of the head, both hands and both feet of the kit's stick figure, each facing away from it.
static ACTOR_POINTS: [Point; 5] =
    [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.62, -1.0, 0.0), pt(1.0, 0.62, 1.0, 0.0), pt(0.0, 0.0, 0.0, -1.0), pt(1.0, 0.0, 0.0, -1.0)];

static USE_CASE_DRESS: Dress = Dress { text: TextSpec::Plain("Use Case"), chars: &PLAIN, obj_type: Some(1), ..Dress::NONE };
static USE_CASE_MORE: More = More { dress: Some(&USE_CASE_DRESS), hidden: false, brings: &[] };

/// The subject's name is typed at the top left of its box, and the box is a container with its margin all round.
static SUBJECT_DRESS: Dress = Dress {
    text: TextSpec::Plain("Subject"),
    chars: &BOLD,
    rows: &[LEFT],
    locks: &[("VerticalAlign", 0.0)],
    obj_type: Some(1),
    role: Role::Container { heading: Side::Top, margin_in: 0.25 },
    ..Dress::NONE
};
static SUBJECT_MORE: More = More { dress: Some(&SUBJECT_DRESS), hidden: false, brings: &[] };

/// A keyword over a line: `«include»` and `«extend»`, in small plain type.
static INCLUDE_DRESS: Dress = Dress { text: TextSpec::Plain("\u{ab}include\u{bb}"), chars: &SMALL, ..Dress::NONE };
static EXTEND_DRESS: Dress = Dress { text: TextSpec::Plain("\u{ab}extend\u{bb}"), chars: &SMALL, ..Dress::NONE };
static INCLUDE_MORE: More = More { dress: Some(&INCLUDE_DRESS), hidden: false, brings: &[] };
static EXTEND_MORE: More = More { dress: Some(&EXTEND_DRESS), hidden: false, brings: &[] };

/// A straight line with these ends.
const fn straight(begin: LineEnd, end: LineEnd, pattern: u8) -> ConnectorLook {
    ConnectorLook { begin, end, pattern, guard_pattern: false, route: Route::Straight, weight_pt: None, compound: None }
}

const fn line(id: &'static str, name: &'static str, ordinal: u16, look: ConnectorLook, more: &'static More) -> MasterSpec {
    MasterSpec { id, name, width: 1.5, height: 0.0, identity: (SERIES, ordinal), outline: Outline::Connector(look), points: &[], more }
}

/// The masters in the order the Shapes window lists them: the first four that are no lines are the Quick Shapes.
pub(super) static MASTERS: [MasterSpec; 9] = [
    MasterSpec {
        id: "uml-usecase.actor",
        name: "Actor",
        width: 0.5,
        height: 1.0,
        identity: (SERIES, 1),
        outline: Outline::Code { build: kit::stick_figure, text: kit::LABEL_BELOW },
        points: &ACTOR_POINTS,
        more: &ACTOR_MORE,
    },
    MasterSpec {
        id: "uml-usecase.use-case",
        name: "Use Case",
        width: 1.6,
        height: 0.8,
        identity: (SERIES, 2),
        outline: Outline::Ellipse,
        points: &RING8,
        more: &USE_CASE_MORE,
    },
    MasterSpec {
        id: "uml-usecase.subject",
        name: "Subject",
        width: 3.0,
        height: 4.0,
        identity: (SERIES, 3),
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &GRID3,
        more: &SUBJECT_MORE,
    },
    MasterSpec {
        id: "uml-usecase.note",
        name: "Note",
        width: 1.4,
        height: 0.8,
        identity: (SERIES, 4),
        outline: Outline::Code { build: kit::note, text: TextBox::FULL },
        points: &kit::SIDES,
        more: &NOTE_MORE,
    },
    line("uml-usecase.association", "Association", 5, straight(LineEnd::None, LineEnd::None, 1), &More::NONE),
    line("uml-usecase.include", "Include", 6, straight(LineEnd::None, LineEnd::Number(3), 2), &INCLUDE_MORE),
    line("uml-usecase.extend", "Extend", 7, straight(LineEnd::None, LineEnd::Number(3), 2), &EXTEND_MORE),
    line("uml-usecase.generalization", "Generalization", 8, straight(LineEnd::None, LineEnd::Number(16), 1), &More::NONE),
    line("uml-usecase.note-link", "Note Link", 9, straight(LineEnd::None, LineEnd::None, 2), &More::NONE),
];

/// The search words of the master with this id, space separated, or `""`. The names a drawing of the reference app carries for these figures (the inventory's C69 and C75:
/// Subsystem, Use Case, Actor, Association, Include, Extend, Generalization) are words of the master that serves the purpose.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "uml-usecase.actor" => "user person role stakeholder stick figure external",
        "uml-usecase.use-case" => "scenario goal function oval ellipse requirement",
        "uml-usecase.subject" => "subsystem system boundary scope container",
        "uml-usecase.note" => "comment annotation remark text",
        "uml-usecase.association" => "relationship link line communication connector",
        "uml-usecase.include" => "includes uses relationship dependency connector",
        "uml-usecase.extend" => "extends optional extension relationship dependency connector",
        "uml-usecase.generalization" => "inheritance specialization is-a connector",
        "uml-usecase.note-link" => "comment annotation anchor connector",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
