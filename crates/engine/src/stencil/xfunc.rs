//! The Cross-Functional Flowchart stencil: two original masters, **Swimlane** and **Separator**, which the Shapes window lists and the template opens beside the flowchart shapes.
//!
//! **A drop from this stencil is not a shape of the master's outline** (ours): a Swimlane dropped on a page that has lanes is **inserted** in the list of lanes, before or after
//! the lane it is dropped on, and a Separator splits the phase it is dropped across (`cmd/swimlane.rs`, `draw/container/swimlane.rs`). The outlines here are the picture the window shows: a
//! lane with its label's strip, and a dashed line down the page. Nothing is traced from another application's stencil.
//!
//! **Both masters hold `ObjType` 4** (`More::NEITHER`; item 2): neither placeable nor routable, so no route goes round a Swimlane or a Separator
//! and a glue to one writes nothing on it, as the reference's swimlane and separator masters hold 4. A drawing saved before holds the older masters, which a drop finds again by their GUID and which hold none.

use super::{Co, Flag, MasterSpec, More, Outline, PathSpec, Seg, TextBox, fh, fw, pt};

const Z: Co = fw(0.0);
const W: Co = fw(1.0);
const H: Co = fh(1.0);

/// The id of the stencil.
pub const ID: &str = "cross-functional";
/// The id of the Swimlane master and of the Separator.
pub const SWIMLANE: &str = "xfunc.swimlane";
pub const SEPARATOR: &str = "xfunc.separator";

pub(super) static MASTERS: [MasterSpec; 2] = [
    MasterSpec {
        id: SWIMLANE,
        name: "Swimlane",
        width: 3.0,
        height: 0.75,
        identity: (7, 1),
        more: &More::NEITHER,
        // A lane with the tab of its label notched out of its top left corner: one filled path, so that an `.odg` writes it as it is drawn.
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.075, 1.0), (0.075, 0.86), (0.0, 0.86)]),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        id: SEPARATOR,
        name: "Separator",
        width: 0.1,
        height: 3.0,
        identity: (7, 2),
        more: &More::NEITHER,
        outline: Outline::Paths {
            // A line down the page, drawn as a thin box.
            paths: &[PathSpec {
                fill: true,
                segs: &[Seg::Move(Z, Z), Seg::Line(W, Z), Seg::Line(W, H), Seg::Line(Z, H), Seg::Line(Z, Z)],
                no_show: Flag::Off,
                no_line: Flag::Off,
            }],
            text: TextBox::FULL,
        },
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.5, 1.0, 0.0)],
    },
];

/// Whether a master is one whose drop is an insert (`cmd/shape.rs` gives it to `cmd/swimlane.rs`).
pub fn is_part(master_id: &str) -> bool {
    master_id == SWIMLANE || master_id == SEPARATOR
}
