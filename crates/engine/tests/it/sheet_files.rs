//! **Every ShapeSheet edit survives a save and a reopen** in each of the six types of a VSDX file and in `.diacraft`, with the exact cells, formulas, units, `IX` and `Del`
//! markers, and with nothing that was not edited changed (design 11, "Round trips"; design 9, "File effects").
//!
//! A trip starts from the hand-written drawing of [`world`] (a master, an instance, a shape of its own, a connector, a group, a second page, a style and the two sheets), opens it as
//! a person's file, makes one edit through the sheet commands, and saves it in its own format, which splices into the file (VSDX) or copies what it did not change (`.diacraft`). A
//! trip then asks six things of the file:
//!
//! 1. **The parts that hold no edited sheet are the same bytes**, and the parts that hold one differ only inside the elements the edit reaches, which the edit names
//!    ([`world::only_within`]: every element outside them has the same attributes and stands in the same order).
//! 2. **The file reads back the model that was saved**: every sheet of the document is equal, cell by cell, row by row (`IX`, `N`, `T`, `Del`), unknown attributes and elements included,
//!    and, in a file that a writer made from the model (`.diacraft`, and a VSDX file made by Save As from a `.diacraft` file), **the place of each unknown node among the rows and sections**
//!    for every command (the row and section commands; the cell commands). **A count is its own witness in this comparison** (a writer that left it stale
//!    wrote the node after as many children as it said, and the file read back says the same), so the files that show the node after the row, the cell or the shape it followed are
//!    `sheet_places.rs`, `sheet_places_writers.rs` and `shape_places.rs`, which read the order of the elements.
//! 3. **`sheet.read` of the edited sheets** gives the same lines as the session that saved.
//! 4. **The edit changed something**: the model after it is not the model before.
//! 5. **A plain save is no edit**: the file that was read back, saved again, is the same bytes.
//! 6. For the `.diacraft` entries, as 1, by entry.
//!
//! Each test goes through a family of edits and every type, and **reports every trip that failed** (not the first), so that one run shows the whole of what is wrong. The trips of
//! Save As across the formats (`.vsdx` to `.diacraft` and `.diacraft` to `.vsdx`) ask 2 and 3.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use diacraft_container::zip::Archive;
use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::sheet_files_world as world;
use crate::sheet_files_world::{changed_parts, only_within, parts_of, read_all, sheets_differ};

// -------------------------------------------------------------------------------------------------------------------------------- parts

const P1: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines
const P2: &str = "/visio/pages/page2.xml"; // brand-ok: a part name the format defines
const PAGES: &str = "/visio/pages/pages.xml"; // brand-ok: a part name the format defines
const M1: &str = "/visio/masters/master1.xml"; // brand-ok: a part name the format defines
const MASTERS: &str = "/visio/masters/masters.xml"; // brand-ok: a part name the format defines
const DOC: &str = "/visio/document.xml"; // brand-ok: a part name the format defines

/// The elements of a page part below a shape, and the connector's, the connects', and so on.
const R10: &str = "PageContents/Shapes/Shape[ID=10]/";
const R11: &str = "PageContents/Shapes/Shape[ID=11]/";
const R12: &str = "PageContents/Shapes/Shape[ID=12]/";
const R13: &str = "PageContents/Shapes/Shape[ID=13]/";
const R15: &str = "PageContents/Shapes/Shape[ID=15]/";
const R14: &str = "PageContents/Shapes/Shape[ID=13]/Shapes/Shape[ID=14]/";
const CONNECTS: &str = "PageContents/Connects/";
const R40: &str = "PageContents/Shapes/Shape[ID=40]/";
/// The master part: the shape.
const MR5: &str = "MasterContents/Shapes/Shape[ID=5]/";
/// Page sheets in the pages index, the master's in the masters index, the document's and the style's in the document part.
const PS1: &str = "Pages/Page[ID=1]/PageSheet/";
const PS2: &str = "Pages/Page[ID=2]/PageSheet/";
const MS1: &str = "Masters/Master[ID=1]/PageSheet/";
const DS: &str = "VisioDocument/DocumentSheet/"; // brand-ok: an element name the format defines
const ST3: &str = "VisioDocument/StyleSheets/StyleSheet[ID=3]/"; // brand-ok: an element name the format defines

/// Where a `.diacraft` package keeps what an edit reaches: the page by its place, the master by its place and the document part.
#[derive(Clone, Copy, Debug)]
enum Native {
    Page(usize),
    Master(usize),
    Document,
}

struct Edit {
    name: &'static str,
    run: fn(&mut Session),
    /// The sheets read in full before the edit and after the reopen, as the params that name them.
    reads: Vec<&'static str>,
    /// The VSDX parts the save may change, each with the elements of it that may differ.
    vsdx: Vec<(&'static str, Vec<&'static str>)>,
    /// The `.diacraft` entries the save may change.
    native: Vec<Native>,
    /// Whether the place each unknown node says it stands (the count of typed children before it) is compared when a model writer made the file (Save As from `.diacraft` to a VSDX
    /// file). **True for every command, which keep the counts in step** (for the row and section commands, for the cell commands; the splice into a VSDX
    /// file keeps the nodes attached to elements and is compared without the places: `sheets_differ`, `unplaced`).
    places: bool,
}

fn at(part: &'static str, regions: &[&'static str]) -> (&'static str, Vec<&'static str>) {
    (part, regions.to_vec())
}

fn edit(name: &'static str, reads: &[&'static str], vsdx: Vec<(&'static str, Vec<&'static str>)>, native: &[Native], run: fn(&mut Session)) -> Edit {
    Edit { name, run, reads: reads.to_vec(), vsdx, native: native.to_vec(), places: true }
}

// ------------------------------------------------------------------------------------------------------------------------------ commands

const SH10: &str = r#"{"id":10}"#;
const SH11: &str = r#"{"id":11}"#;
const SH14: &str = r#"{"id":14}"#;
const SH13: &str = r#"{"id":13}"#;
const SH15: &str = r#"{"id":15}"#;
const SH40: &str = r#"{"id":40,"page":2}"#;
const PAGE1: &str = r#"{"page":1}"#;
const PAGE2: &str = r#"{"page":2}"#;
const DOCUMENT: &str = r#"{"document":true}"#;
const STYLE3: &str = r#"{"style":3}"#;
const MASTER5: &str = r#"{"master":1,"id":5}"#;
const MASTER1: &str = r#"{"master":1}"#;

/// `command` on the sheet `target` names, with `extra` added to its params. Panics when it is refused.
fn call(s: &mut Session, command: &str, target: &str, extra: Value) -> Value {
    let mut params: Value = serde_json::from_str(target).unwrap();
    if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn set(s: &mut Session, target: &str, cell: &str, formula: &str) {
    call(s, "sheet.set_cell", target, json!({"cell": cell, "formula": formula}));
}

fn add_row(s: &mut Session, target: &str, section: &str, extra: Value) {
    let mut params = json!({"section": section});
    params.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    call(s, "sheet.add_row", target, params);
}

fn delete_row(s: &mut Session, target: &str, section: &str, row: Value) {
    call(s, "sheet.delete_row", target, json!({"section": section, "row": row}));
}

fn add_section(s: &mut Session, target: &str, section: &str) {
    call(s, "sheet.add_section", target, json!({"section": section}));
}

fn delete_section(s: &mut Session, target: &str, section: &str) {
    call(s, "sheet.delete_section", target, json!({"section": section}));
}

// ------------------------------------------------------------------------------------------------------------------------------- edits

/// A cell directly under a sheet, a cell of a named row, a Geometry cell, a page's cell, the document's, a style's and a master shape's: each a formula, a value and a clear.
fn cell_edits() -> Vec<Edit> {
    use Native::{Document, Master, Page};
    vec![
        // A shape's own cell. Width reads Height; shape 11's frame, its path, its points and the connector glued to it follow.
        edit("shape: a formula", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| set(s, SH11, "Width", "Height*2")),
        edit("shape: a value", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| set(s, SH11, "Width", "4")),
        edit("shape: a clear", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| set(s, SH11, "Width", "")),
        edit("shape: a value with a unit", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| set(s, SH11, "Width", "250 mm")),
        edit("shape: a cell it did not hold", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| set(s, SH11, "Angle", "30 deg")),
        // An instance: the cell is the master's until the instance writes one.
        edit("instance: a value over the master's", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| set(s, SH10, "Width", "3")),
        edit("instance: a formula over the master's", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| set(s, SH10, "Width", "Height*3")),
        edit("instance: a value, then a clear", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| {
            set(s, SH10, "Width", "3");
            set(s, SH10, "Width", "");
        }),
        edit("instance: a named row's own cell", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| set(s, SH10, "User.Rate", "9")),
        // A named row.
        edit("named row: a formula", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "User.Rate", "Width*2")),
        edit("named row: a value", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "User.Rate", "7")),
        edit("named row: a clear", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "User.Total", "")),
        edit("named row: a text", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Prop.Owner", "Bob")),
        edit("named row: the prompt", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "User.Rate.Prompt", "Per box")),
        edit("named row: a Shape Data value", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Prop.Cost", "99.5")),
        // A Geometry cell: the row's cells and the section's flags.
        edit("geometry: a formula", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Geometry1.X3", "Width*0.75")),
        edit("geometry: a value", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Geometry1.X3", "1.5")),
        edit("geometry: a clear", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Geometry1.Y3", "")),
        edit("geometry: a flag of the section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "Geometry2.NoFill", "0")),
        edit("geometry: a cell of a connector's path", &[r#"{"id":12}"#], vec![at(P1, &[R12])], &[Page(0)], |s| {
            set(s, r#"{"id":12}"#, "Geometry1.Y2", "1")
        }),
        edit("a guarded cell, forced", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| {
            call(s, "sheet.set_cell", SH14, json!({"cell": "Width", "formula": "3", "force": true}));
        }),
        edit("a text that holds the characters of markup", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            set(s, SH11, "Prop.Owner", "=\"R&D <Team> 'x'\"");
        }),
        edit("a row whose name is not ASCII", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| set(s, SH11, "User.Gr\u{f6}\u{df}e", "Width*2")),
        edit("an instance's own cell of one of its master's rows", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            set(s, SH40, "Geometry1.X3", "Width")
        }),
        edit("an instance's own cell in a row it shares with its master", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            set(s, SH40, "Scratch.B2", "42")
        }),
        // A page's sheet: its size, its scale and a named row of its own.
        edit("page: a formula", &[PAGE1], vec![at(PAGES, &[PS1])], &[Page(0)], |s| set(s, PAGE1, "PageWidth", "6+7")),
        edit("page: a value", &[PAGE1], vec![at(PAGES, &[PS1])], &[Page(0)], |s| set(s, PAGE1, "PageHeight", "10")),
        edit("page: a formula, then a clear", &[PAGE1], vec![at(PAGES, &[PS1])], &[Page(0)], |s| {
            set(s, PAGE1, "DrawingScale", "2/2");
            set(s, PAGE1, "DrawingScale", "");
        }),
        edit("page: a named row", &[PAGE1], vec![at(PAGES, &[PS1])], &[Page(0)], |s| set(s, PAGE1, "User.Rate", "2*3")),
        // The document's sheet: what the member of the group reads (TheDoc!User.Rate) follows.
        edit("document: a formula", &[DOCUMENT], vec![at(DOC, &[DS]), at(P1, &[R14])], &[Document, Page(0)], |s| {
            set(s, DOCUMENT, "User.Rate", "3+4")
        }),
        edit("document: a value", &[DOCUMENT], vec![at(DOC, &[DS]), at(P1, &[R14])], &[Document, Page(0)], |s| set(s, DOCUMENT, "User.Rate", "9")),
        edit("document: a formula, then a clear", &[DOCUMENT], vec![at(DOC, &[DS]), at(P1, &[R14])], &[Document, Page(0)], |s| {
            set(s, DOCUMENT, "User.Rate", "2+2");
            set(s, DOCUMENT, "User.Rate", "");
        }),
        edit("document: a direct cell", &[DOCUMENT], vec![at(DOC, &[DS])], &[Document], |s| set(s, DOCUMENT, "DocLangID", "1036")),
        // A style's sheet: the shape that follows it by `LineStyle` holds the line weight as `Inh`.
        edit("style: a formula", &[STYLE3], vec![at(DOC, &[ST3]), at(P1, &[R11])], &[Document, Page(0)], |s| set(s, STYLE3, "LineWeight", "0.01*2")),
        edit("style: a value", &[STYLE3], vec![at(DOC, &[ST3]), at(P1, &[R11])], &[Document, Page(0)], |s| set(s, STYLE3, "LineWeight", "0.03")),
        edit("style: a formula, then a clear", &[STYLE3], vec![at(DOC, &[ST3]), at(P1, &[R11])], &[Document, Page(0)], |s| {
            set(s, STYLE3, "LineColor", "2");
            set(s, STYLE3, "LineColor", "RGB(1,2,3)");
            set(s, STYLE3, "LineColor", "");
        }),
        edit("style: a named row", &[STYLE3], vec![at(DOC, &[ST3])], &[Document], |s| set(s, STYLE3, "User.Rate", "4")),
        // A master's shape: its instances on both pages follow, and the master's own page sheet.
        edit("master shape: a formula", &[MASTER5], vec![at(M1, &[MR5]), at(P1, &[R10, R12])], &[Master(0), Page(0)], |s| {
            set(s, MASTER5, "Width", "Height*3")
        }),
        edit("master shape: a value", &[MASTER5], vec![at(M1, &[MR5]), at(P1, &[R10, R12])], &[Master(0), Page(0)], |s| {
            set(s, MASTER5, "Width", "4")
        }),
        edit("master shape: a clear", &[MASTER5], vec![at(M1, &[MR5]), at(P1, &[R10, R12])], &[Master(0), Page(0)], |s| {
            set(s, MASTER5, "User.Total", "")
        }),
        edit("master shape: a Shape Data value", &[MASTER5], vec![at(M1, &[MR5])], &[Master(0)], |s| set(s, MASTER5, "Prop.Cost", "20")),
        edit("master shape: a Geometry cell", &[MASTER5], vec![at(M1, &[MR5])], &[Master(0)], |s| set(s, MASTER5, "Geometry1.X3", "Width*0.5")),
        edit("master: its page sheet", &[MASTER1], vec![at(MASTERS, &[MS1])], &[Master(0)], |s| set(s, MASTER1, "PageWidth", "3")),
        // The group and its member.
        edit("group member: a value", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| set(s, SH14, "Height", "2")),
        edit("group: a value", &[SH13], vec![at(P1, &[R13])], &[Page(0)], |s| set(s, SH13, "Width", "3")),
        // Several cells in one call.
        edit("a batch of writes", &[SH11], vec![at(P1, &[R11, R12])], &[Page(0)], |s| {
            call(
                s,
                "sheet.set_cells",
                SH11,
                json!({"writes": [
                    {"cell": "Width", "formula": "Height*2"},
                    {"cell": "User.Rate", "formula": "7"},
                    {"cell": "Geometry1.X4", "formula": "Width*0.25"},
                    {"cell": "Scratch.B1", "formula": "5"},
                ]}),
            );
        }),
    ]
}

/// Each row command on each section that takes rows, at the end and in the middle, and the deletes.
fn row_edits() -> Vec<Edit> {
    use Native::{Document, Master, Page};
    vec![
        // Named sections: the new row is `Row_n`, or the name asked for.
        edit("add a User row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "User", json!({}))),
        edit("add a User row by name, after Rate", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "User", json!({"name": "Extra", "after": "Rate"}));
        }),
        edit("add a User row by name, before Rate", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "User", json!({"name": "First", "before": "Rate"}));
        }),
        edit("add Shape Data rows", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "Prop", json!({}));
            add_row(s, SH11, "Prop", json!({"name": "Region"}));
        }),
        edit("add a Hyperlink row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Hyperlink", json!({}))),
        edit("add a Controls row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Controls", json!({}))),
        edit("add an Actions row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Actions", json!({}))),
        edit("add a User row after one whose name is not ASCII", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "User", json!({"name": "Extra", "after": "Gr\u{f6}\u{df}e"}));
        }),
        edit("add a Shape Data row before the one that has a local name", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "Prop", json!({"before": "Owner"}));
        }),
        edit("delete the Shape Data row that has a local name", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            delete_row(s, SH11, "Prop", json!("Owner"))
        }),
        // Indexed sections: a number nobody holds, and the middle.
        edit("add a Scratch row at the end", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Scratch", json!({}))),
        edit(
            "add a Scratch row in the middle: the later rows are numbered again and the formula that names one follows",
            &[SH11],
            vec![at(P1, &[R11])],
            &[Page(0)],
            |s| {
                add_row(s, SH11, "Scratch", json!({"after": 1}));
            },
        ),
        edit("add a Scratch row before the first", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Scratch", json!({"before": 1}))),
        edit("add a Geometry row at the end, an arc", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_row(s, SH11, "Geometry1", json!({"type": "ArcTo"}));
        }),
        edit(
            "add a Geometry row in the middle: the later rows are numbered again and Geometry1.X3 follows",
            &[SH11],
            vec![at(P1, &[R11])],
            &[Page(0)],
            |s| {
                add_row(s, SH11, "Geometry1", json!({"after": 2}));
            },
        ),
        edit("add a Geometry row of each type", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            for kind in [
                "RelMoveTo",
                "RelLineTo",
                "ArcTo",
                "EllipticalArcTo",
                "RelEllipticalArcTo",
                "PolylineTo",
                "NURBSTo",
                "SplineStart",
                "SplineKnot",
                "RelCubBezTo",
                "RelQuadBezTo",
            ] {
                add_row(s, SH11, "Geometry2", json!({"type": kind}));
            }
        }),
        edit("add a connection point at the end", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_row(s, SH11, "Connections", json!({}))),
        edit(
            "add a connection point in the middle: the glue to the points after it follows",
            &[SH11],
            vec![at(P1, &[R11, R12, CONNECTS])],
            &[Page(0)],
            |s| {
                add_row(s, SH11, "Connections", json!({"after": 1}));
            },
        ),
        // A sheet that is not a shape.
        edit(
            "add rows to a page, the document and a style",
            &[PAGE1, DOCUMENT, STYLE3],
            vec![at(PAGES, &[PS1]), at(DOC, &[DS, ST3])],
            &[Page(0), Document],
            |s| {
                add_row(s, PAGE1, "User", json!({}));
                add_row(s, DOCUMENT, "User", json!({"name": "Fee"}));
                add_row(s, STYLE3, "User", json!({"name": "Fee"}));
            },
        ),
        edit("add rows to a master's shape", &[MASTER5], vec![at(M1, &[MR5])], &[Master(0)], |s| {
            add_row(s, MASTER5, "Scratch", json!({}));
            add_row(s, MASTER5, "User", json!({}));
        }),
        edit("add a row to a group member", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| add_row(s, SH14, "User", json!({}))),
        // An instance takes its own rows.
        edit("an instance: a Scratch row at the end", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| add_row(s, SH10, "Scratch", json!({}))),
        edit("an instance: a User row", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| add_row(s, SH10, "User", json!({}))),
        edit("an instance: a Scratch row in the middle, after a copy of the master's rows", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            add_row(s, SH10, "Scratch", json!({"after": 1}));
        }),
        edit("an instance: a Geometry row in the middle, after a copy of the master's path", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| {
            add_row(s, SH10, "Geometry1", json!({"after": 2}));
        }),
        edit(
            "an instance: a connection point in the middle: the connector keeps its glue",
            &[SH10],
            vec![at(P1, &[R10, R12, CONNECTS])],
            &[Page(0)],
            |s| {
                add_row(s, SH10, "Connections", json!({"after": 1}));
            },
        ),
        // Numbers with gaps in them (a file whose rows were deleted by another writer), rows with no number at all, and an instance that overrides a cell of its master's rows.
        edit("a gap: add after the first row takes the number nobody holds", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| {
            add_row(s, SH14, "Scratch", json!({"after": 1}))
        }),
        edit("a gap: add after the middle row takes the next number", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| {
            add_row(s, SH14, "Scratch", json!({"after": 3}))
        }),
        edit("a gap: add at the end is one past the largest", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| add_row(s, SH14, "Scratch", json!({}))),
        edit("a gap: delete the middle row", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| delete_row(s, SH14, "Scratch", json!(3))),
        edit("rows with no number: add one in the middle", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            add_row(s, SH15, "Scratch", json!({"after": 1}))
        }),
        edit("rows with no number: delete the first", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| delete_row(s, SH15, "Scratch", json!(1))),
        edit("rows with no number: add at the end and delete the middle", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            add_row(s, SH15, "Scratch", json!({}));
            delete_row(s, SH15, "Scratch", json!(2));
        }),
        edit("sections with no number: add a Geometry row in the middle of the first", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            add_row(s, SH15, "Geometry1", json!({"after": 1}));
        }),
        edit("sections with no number: add a Geometry row at the end of the second", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            add_row(s, SH15, "Geometry2", json!({"type": "ArcTo"}));
        }),
        edit("an instance that overrides one cell of a row: a Geometry row in the middle", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            add_row(s, SH40, "Geometry1", json!({"after": 2}));
        }),
        edit("an instance that overrides one cell of a row: a Scratch row in the middle", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            add_row(s, SH40, "Scratch", json!({"after": 1}));
        }),
        edit("an instance that overrides one cell of a row: delete that row", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            delete_row(s, SH40, "Geometry1", json!(3))
        }),
        edit("an instance that overrides one cell of a row: delete another", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            delete_row(s, SH40, "Geometry1", json!(2))
        }),
        // Deletes.
        edit(
            "delete a Geometry row in the middle: the later rows close up and Geometry1.X3 reads REF()",
            &[SH11],
            vec![at(P1, &[R11])],
            &[Page(0)],
            |s| {
                delete_row(s, SH11, "Geometry1", json!(3));
            },
        ),
        edit("delete a named row that formulas read", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_row(s, SH11, "User", json!("Rate"))),
        edit("delete a Scratch row in the middle", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_row(s, SH11, "Scratch", json!(1))),
        edit("delete the last Scratch row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_row(s, SH11, "Scratch", json!(3))),
        edit("delete a Shape Data row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_row(s, SH11, "Prop", json!("Cost"))),
        edit("delete a Hyperlink, a Controls and an Actions row", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            delete_row(s, SH11, "Hyperlink", json!("Row_1"));
            delete_row(s, SH11, "Controls", json!("Row_1"));
            delete_row(s, SH11, "Actions", json!("Row_1"));
        }),
        edit("delete a connection point the connector is glued to", &[SH11], vec![at(P1, &[R11, R12, CONNECTS])], &[Page(0)], |s| {
            delete_row(s, SH11, "Connections", json!(2));
        }),
        edit(
            "delete a row of a page's, the document's and a style's",
            &[PAGE1, DOCUMENT, STYLE3],
            vec![at(PAGES, &[PS1]), at(DOC, &[DS, ST3]), at(P1, &[R14])],
            &[Page(0), Document],
            |s| {
                delete_row(s, PAGE1, "User", json!("Rate"));
                delete_row(s, DOCUMENT, "User", json!("Rate"));
                delete_row(s, STYLE3, "User", json!("Rate"));
            },
        ),
        edit(
            "delete rows of a master's shape: the instances read them no more",
            &[MASTER5],
            vec![at(M1, &[MR5]), at(P1, &[R10, R12])],
            &[Master(0), Page(0)],
            |s| {
                delete_row(s, MASTER5, "Scratch", json!(2));
                delete_row(s, MASTER5, "Prop", json!("Cost"));
            },
        ),
        // A row an instance takes from its master is dropped by a `Del` row.
        edit("an instance: delete a row of its master's path", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| {
            delete_row(s, SH10, "Geometry1", json!(2));
        }),
        edit("an instance: delete a named row of its master's", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            delete_row(s, SH10, "User", json!("Total"))
        }),
        edit("an instance: delete a Scratch row of its master's", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            delete_row(s, SH10, "Scratch", json!(1))
        }),
        edit("an instance: delete a Shape Data row of its master's", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            delete_row(s, SH10, "Prop", json!("Cost"))
        }),
        edit(
            "an instance: delete a connection point of its master's, which the connector is glued to",
            &[SH10],
            vec![at(P1, &[R10, R12, CONNECTS])],
            &[Page(0)],
            |s| {
                delete_row(s, SH10, "Connections", json!(1));
            },
        ),
        // Del after a copy: the instance holds its own rows, and one of them is a number its master holds.
        edit(
            "an instance: a copy, an insert in the middle, then a delete of a row the master holds",
            &[SH10],
            vec![at(P1, &[R10])],
            &[Page(0)],
            |s| {
                add_row(s, SH10, "Scratch", json!({"after": 1}));
                delete_row(s, SH10, "Scratch", json!(1));
            },
        ),
        edit(
            "an instance: a copy of the path, an insert in the middle, then a delete at the end",
            &[SH10],
            vec![at(P1, &[R10, R12])],
            &[Page(0)],
            |s| {
                add_row(s, SH10, "Geometry1", json!({"after": 2}));
                delete_row(s, SH10, "Geometry1", json!(6));
            },
        ),
        edit("a delete, then an insert at the same place, takes the number again", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            delete_row(s, SH11, "Scratch", json!(3));
            add_row(s, SH11, "Scratch", json!({}));
        }),
    ]
}

/// Each section command, on each sheet that allows it.
fn section_edits() -> Vec<Edit> {
    use Native::{Document, Master, Page};
    vec![
        edit("add a Geometry section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_section(s, SH11, "Geometry")),
        edit("add an Ellipse section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            call(s, "sheet.add_section", SH11, json!({"section": "Geometry", "variant": "Ellipse"}));
        }),
        edit("add an Infinite Line section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            call(s, "sheet.add_section", SH11, json!({"section": "Geometry", "variant": "InfiniteLine"}));
        }),
        edit("add the Text Transform cells", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_section(s, SH11, "Text Transform")),
        edit("add the Layer Membership cell", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| add_section(s, SH11, "Layer Membership")),
        edit("add sections to a shape that has none of them", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            add_section(s, SH40, "Actions");
            add_section(s, SH40, "Hyperlink");
            add_section(s, SH40, "Controls");
        }),
        edit("add a section to a page that holds no cell", &[PAGE2], vec![at(PAGES, &[PS2])], &[Page(1)], |s| add_section(s, PAGE2, "User")),
        edit("add sections to the document and a style", &[DOCUMENT, STYLE3], vec![at(DOC, &[DS, ST3])], &[Document], |s| {
            add_section(s, DOCUMENT, "Scratch");
            add_section(s, STYLE3, "Prop");
        }),
        edit("add a section to a master's shape", &[MASTER5], vec![at(M1, &[MR5])], &[Master(0)], |s| add_section(s, MASTER5, "Hyperlink")),
        edit("add a section to a group member", &[SH14], vec![at(P1, &[R14])], &[Page(0)], |s| add_section(s, SH14, "Hyperlink")),
        edit("an instance: a Geometry section next to its master's", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| add_section(s, SH10, "Geometry")),
        edit("delete a Scratch section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_section(s, SH11, "Scratch")),
        edit(
            "delete the first Geometry section: the second takes its number and Geometry1.X3 reads REF()",
            &[SH11],
            vec![at(P1, &[R11])],
            &[Page(0)],
            |s| {
                delete_section(s, SH11, "Geometry1");
            },
        ),
        edit("delete the second Geometry section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| delete_section(s, SH11, "Geometry2")),
        edit("delete the Shape Data section and the Hyperlink section", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            delete_section(s, SH11, "Prop");
            delete_section(s, SH11, "Hyperlink");
        }),
        edit("delete the connection points the connector is glued to", &[SH11], vec![at(P1, &[R11, R12, CONNECTS])], &[Page(0)], |s| {
            delete_section(s, SH11, "Connections");
        }),
        edit(
            "delete a section of a page, the document and a style",
            &[PAGE1, DOCUMENT, STYLE3],
            vec![at(PAGES, &[PS1]), at(DOC, &[DS, ST3]), at(P1, &[R14])],
            &[Page(0), Document],
            |s| {
                delete_section(s, PAGE1, "User");
                delete_section(s, DOCUMENT, "User");
                delete_section(s, STYLE3, "User");
            },
        ),
        edit("delete a section of a master's shape", &[MASTER5], vec![at(M1, &[MR5]), at(P1, &[R10, R12])], &[Master(0), Page(0)], |s| {
            delete_section(s, MASTER5, "Scratch")
        }),
        // A section an instance takes from its master is dropped by a `Del` section.
        edit("an instance: delete the Scratch section its master gives", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            delete_section(s, SH10, "Scratch")
        }),
        edit("an instance: delete the path its master gives", &[SH10], vec![at(P1, &[R10, R12])], &[Page(0)], |s| {
            delete_section(s, SH10, "Geometry1")
        }),
        edit("an instance: delete the connection points its master gives", &[SH10], vec![at(P1, &[R10, R12, CONNECTS])], &[Page(0)], |s| {
            delete_section(s, SH10, "Connections");
        }),
        edit("an instance: delete the Shape Data section its master gives", &[SH10], vec![at(P1, &[R10])], &[Page(0)], |s| {
            delete_section(s, SH10, "Prop")
        }),
        edit("sections with no number: delete the first Geometry section", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            delete_section(s, SH15, "Geometry1")
        }),
        edit("sections with no number: delete the second, add one", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            delete_section(s, SH15, "Geometry2");
            add_section(s, SH15, "Geometry");
        }),
        edit("sections with no number: delete the Scratch section", &[SH15], vec![at(P1, &[R15])], &[Page(0)], |s| {
            delete_section(s, SH15, "Scratch")
        }),
        edit("an instance that overrides one cell of a row: delete the path", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            delete_section(s, SH40, "Geometry1")
        }),
        edit("an instance that overrides one cell of a row: delete the Scratch section", &[SH40], vec![at(P2, &[R40])], &[Page(1)], |s| {
            delete_section(s, SH40, "Scratch")
        }),
        edit("add the Text Transform cells, then delete them: the drawing is as it was", &[SH11], vec![], &[], |s| {
            add_section(s, SH11, "Text Transform");
            delete_section(s, SH11, "Text Transform");
        }),
        edit("add a Geometry section, then delete the first: the new one takes number 2", &[SH11], vec![at(P1, &[R11])], &[Page(0)], |s| {
            add_section(s, SH11, "Geometry");
            delete_section(s, SH11, "Geometry1");
        }),
    ]
}

/// Several commands, one save; and an edit undone.
fn chained_edits() -> Vec<Edit> {
    use Native::{Document, Master, Page};
    vec![
        edit("a chain: rows, cells, a delete and a section, one save", &[SH11], vec![at(P1, &[R11, R12, CONNECTS])], &[Page(0)], |s| {
            add_row(s, SH11, "Scratch", json!({"after": 1}));
            set(s, SH11, "Scratch.A2", "Width*2");
            add_row(s, SH11, "User", json!({"name": "Fee"}));
            set(s, SH11, "User.Fee", "Width*0.5");
            delete_row(s, SH11, "Prop", json!("Owner"));
            add_section(s, SH11, "Geometry");
            set(s, SH11, "Geometry3.X2", "Width");
            add_row(s, SH11, "Connections", json!({"after": 1}));
            set(s, SH11, "Height", "2");
        }),
        edit(
            "a chain over every part: a page, another page, the document, a style, a master and a group member",
            &[SH11, SH40, PAGE1, DOCUMENT, STYLE3, MASTER5, SH14],
            vec![at(P1, &[R11, R12, R10, R14, R13]), at(P2, &[R40]), at(PAGES, &[PS1]), at(DOC, &[DS, ST3]), at(M1, &[MR5])],
            &[Page(0), Page(1), Document, Master(0)],
            |s| {
                set(s, SH11, "Width", "Height*2");
                add_row(s, SH40, "Scratch", json!({"after": 1}));
                set(s, PAGE1, "PageHeight", "9");
                add_row(s, DOCUMENT, "User", json!({"name": "Fee"}));
                set(s, STYLE3, "LineWeight", "0.05");
                delete_row(s, MASTER5, "Scratch", json!(2));
                add_section(s, SH14, "Hyperlink");
                set(s, DOCUMENT, "User.Fee", "6");
            },
        ),
        edit("every command undone", &[SH11], vec![], &[], |s| {
            set(s, SH11, "Width", "4");
            add_row(s, SH11, "Scratch", json!({"after": 1}));
            delete_row(s, SH11, "Geometry1", json!(3));
            add_section(s, SH11, "Geometry");
            for _ in 0..4 {
                s.execute("edit.undo", &json!({})).unwrap();
            }
        }),
    ]
}

fn all_edits() -> Vec<Edit> {
    let mut all = cell_edits();
    all.extend(row_edits());
    all.extend(section_edits());
    all.extend(chained_edits());
    all
}

// ------------------------------------------------------------------------------------------------------------------------------- trips

/// Opens the file at `path`, a template as the file itself.
fn open(path: &str, ext: &str) -> Result<Session, String> {
    let mut s = Session::new();
    let mut params = json!({ "path": path });
    if matches!(ext, "vstx" | "vstm" | "diacraft-template") {
        params["edit_template"] = json!(true);
    }
    s.execute("file.open", &params).map_err(|e| format!("the file does not open: {e}"))?;
    Ok(s)
}

/// Runs the edit, naming a panic in it as a failure of the trip.
fn edited(s: &mut Session, edit: &Edit) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| (edit.run)(s))).map_err(|e| {
        let words = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|w| (*w).to_owned())).unwrap_or_default();
        format!("the edit itself was refused: {words}")
    })
}

fn reads_of(s: &mut Session, edit: &Edit) -> Vec<Value> {
    edit.reads.iter().map(|target| read_all(s, &serde_json::from_str(target).unwrap())).collect()
}

/// What a session read back from a file must agree with in the session that saved it: every sheet, and the lines of the sheets the edit names. With `exact` the place each unknown node
/// says it stands is compared too (a writer that makes the file from the model writes the model as it is: `.diacraft`, and the model writer of a VSDX file, for the commands that keep the
/// counts in step); without it the nodes and their order are (a splice keeps each node attached to the element that held the item it followed, which it finds by what the
/// item holds, and a row that holds nothing is paired with any element that holds nothing, so the node may stand on either side of a row the edit put in; and the file read back counts
/// the typed children before it again).
fn agrees(saved: &mut Session, back: &mut Session, edit: &Edit, reads: &[Value], exact: bool) -> Result<(), String> {
    let differ = sheets_differ(saved.document().unwrap(), back.document().unwrap(), exact);
    if !differ.is_empty() {
        return Err(format!("the file reads back other sheets than the ones that were saved:\n    {}", differ.join("\n    ")));
    }
    for (target, want) in edit.reads.iter().zip(reads) {
        let got = read_all(back, &serde_json::from_str(target).unwrap());
        if &got != want {
            let at = want["lines"].as_array().into_iter().flatten().zip(got["lines"].as_array().into_iter().flatten()).position(|(a, b)| a != b);
            return Err(format!(
                "sheet.read of {target} differs after the reopen (sections equal: {}; lines {} and {}; first line that differs: {:?})",
                want["sections"] == got["sections"],
                want["lines"].as_array().map_or(0, Vec::len),
                got["lines"].as_array().map_or(0, Vec::len),
                at.map(|k| (want["lines"][k].clone(), got["lines"][k].clone()))
            ));
        }
    }
    Ok(())
}

/// One edit saved into a file of the type `ext` and read back.
fn trip_vsdx(ext: &str, edit: &Edit) -> Result<(), String> {
    trip_vsdx_in(&world::package(ext), ext, edit)
}

/// One edit saved into the file `original` of the type `ext` and read back.
fn trip_vsdx_in(original: &[u8], ext: &str, edit: &Edit) -> Result<(), String> {
    let dir = Dir::new();
    let path = dir.write(&format!("world.{ext}"), original);
    let mut s = open(&path, ext)?;
    let before = s.document().unwrap().clone();
    edited(&mut s, edit)?;
    let after = s.document().unwrap().clone();
    if edit.vsdx.is_empty() {
        if !sheets_differ(&before, &after, edit.places).is_empty() {
            return Err("the edit was meant to leave the drawing as it was, and changed a sheet".into());
        }
    } else if sheets_differ(&before, &after, false).is_empty() {
        return Err("the edit changed no sheet of the drawing".into());
    }
    let reads = reads_of(&mut s, edit);
    s.execute("file.save", &json!({})).map_err(|e| format!("the save: {e}"))?;
    let saved = dir.read(&format!("world.{ext}"));
    // 1. The parts: only those the edit reaches differ, and inside them only the elements it reaches.
    let (changed, removed, added) = changed_parts(original, &saved);
    if !removed.is_empty() || !added.is_empty() {
        return Err(format!("the save removed {removed:?} and added {added:?}"));
    }
    let (old, new) = (parts_of(original), parts_of(&saved));
    for name in &changed {
        if name.starts_with("/docProps/") {
            continue;
        }
        let listed = edit.vsdx.iter().find(|(part, _)| part == name);
        let regions: Vec<&str> = listed.map(|(_, regions)| regions.clone()).unwrap_or_default();
        only_within(&old[name], &new[name], &regions)
            .map_err(|why| format!("{name}{}: {why}", if listed.is_some() { "" } else { " (no sheet of it is edited)" }))?;
        if listed.is_none() {
            return Err(format!("a part that holds no edited sheet changed, in its bytes and in no element: {name}"));
        }
    }

    // 2 and 3. The file reads back what was saved.
    let mut back = open(&path, ext)?;
    agrees(&mut s, &mut back, edit, &reads, false)?;

    // 5. A plain save is no edit.
    back.execute("file.save", &json!({})).map_err(|e| format!("the second save: {e}"))?;
    if dir.read(&format!("world.{ext}")) != saved {
        return Err("a second save with no edit wrote other bytes".into());
    }
    Ok(())
}

/// One edit, a save, a second edit and a second save, in one session: **the second save is no refusal** (the model the first save left in the session holds the unknown nodes where the
/// edit left them, and the baseline is the file read back), and the file after it reads back what the session holds.
fn trip_twice(original: &[u8], ext: &str, edit: &Edit) -> Result<(), String> {
    let dir = Dir::new();
    let path = dir.write(&format!("world.{ext}"), original);
    let mut s = open(&path, ext)?;
    edited(&mut s, edit)?;
    s.execute("file.save", &json!({})).map_err(|e| format!("the first save: {e}"))?;
    // A second edit of another sheet, which no part of the first reaches but the page's own.
    set(&mut s, PAGE2, "PageHeight", "12");
    set(&mut s, SH40, "Width", "5");
    let reads: Vec<Value> = Vec::new();
    s.execute("file.save", &json!({})).map_err(|e| format!("the second save: {e}"))?;
    let mut back = open(&path, ext)?;
    let second = Edit { name: edit.name, run: edit.run, reads: Vec::new(), vsdx: Vec::new(), native: Vec::new(), places: edit.places };
    agrees(&mut s, &mut back, &second, &reads, ext.starts_with("diacraft"))
}

/// The names of the entries of a `.diacraft` package that hold the pages and the masters, in order, with every entry's bytes.
fn entries(bytes: &[u8]) -> (BTreeMap<String, Vec<u8>>, Vec<String>, Vec<String>) {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).unwrap();
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut all = BTreeMap::new();
    for (k, name) in names.iter().enumerate() {
        all.insert(name.clone(), archive.inflate(k, &mut diacraft_container::Work::default()).unwrap());
    }
    let manifest = diacraft_opc::xml::parse(&all["manifest.xml"]).unwrap();
    let by_role = |role: &str| -> Vec<String> {
        manifest.elements().filter(|e| e.is("part") && e.attr("role") == Some(role)).filter_map(|e| e.attr("path").map(str::to_owned)).collect()
    };
    (all, by_role("page"), by_role("master"))
}

/// One edit saved into a `.diacraft` file and read back.
fn trip_native(edit: &Edit) -> Result<(), String> {
    trip_native_as("diacraft", edit)
}

/// One edit saved into a file of the kind `ext` (`diacraft`, `diacraft-template` or `diacraft-stencil`) and read back. The file is the drawing saved as that kind from a `.vsdx` file (the
/// stencil from a `.vssx` file that holds masters and no page).
fn trip_native_as(ext: &str, edit: &Edit) -> Result<(), String> {
    let dir = Dir::new();
    let (source_ext, source) = if ext == "diacraft-stencil" { ("vssx", world::stencil_package("vssx")) } else { ("vsdx", world::package("vsdx")) };
    let vsdx = dir.write(&format!("world.{source_ext}"), &source);
    let mut maker = open(&vsdx, source_ext)?;
    let native = dir.path(&format!("world.{ext}"));
    maker.execute("file.save_as", &json!({ "path": native })).map_err(|e| format!("the drawing does not save as .{ext}: {e}"))?;
    let original = dir.read(&format!("world.{ext}"));

    let mut s = open(&native, ext)?;
    let before = s.document().unwrap().clone();
    edited(&mut s, edit)?;
    let after = s.document().unwrap().clone();
    if edit.vsdx.is_empty() {
        if !sheets_differ(&before, &after, edit.places).is_empty() {
            return Err("the edit was meant to leave the drawing as it was, and changed a sheet".into());
        }
    } else if sheets_differ(&before, &after, false).is_empty() {
        return Err("the edit changed no sheet of the drawing".into());
    }
    let reads = reads_of(&mut s, edit);
    s.execute("file.save", &json!({})).map_err(|e| format!("the save: {e}"))?;
    let saved = dir.read(&format!("world.{ext}"));

    // 6. The entries: only those the edit reaches differ (the state and the preview are the file's own bookkeeping).
    let (old, pages, masters) = entries(&original);
    let (new, _, _) = entries(&saved);
    let mut reached: Vec<String> = vec!["state.xml".into(), "preview.png".into()];
    for place in &edit.native {
        match place {
            Native::Page(n) => reached.extend(pages.get(*n).cloned()),
            Native::Master(n) => reached.extend(masters.get(*n).cloned()),
            Native::Document => reached.push("document.xml".into()),
        }
    }
    for name in old.keys().chain(new.keys()).collect::<std::collections::BTreeSet<_>>() {
        if old.get(name) != new.get(name) && !reached.contains(name) {
            return Err(format!("an entry that holds no edited sheet changed: {name}"));
        }
    }

    let mut back = open(&native, ext)?;
    agrees(&mut s, &mut back, edit, &reads, true)?;

    back.execute("file.save", &json!({})).map_err(|e| format!("the second save: {e}"))?;
    if dir.read(&format!("world.{ext}")) != saved {
        return Err("a second save with no edit wrote other bytes".into());
    }
    Ok(())
}

/// One edit made in a drawing read from `from`, saved as `to` (a model writer) and read back.
fn trip_across(from: &str, to: &str, edit: &Edit) -> Result<(), String> {
    let dir = Dir::new();
    let vsdx = dir.write("world.vsdx", &world::package("vsdx"));
    let source = if from == "diacraft" {
        let mut maker = open(&vsdx, "vsdx")?;
        let native = dir.path("world.diacraft");
        maker.execute("file.save_as", &json!({ "path": native })).map_err(|e| format!("the drawing does not save as .diacraft: {e}"))?;
        native
    } else {
        vsdx
    };
    let mut s = open(&source, from)?;
    edited(&mut s, edit)?;
    let reads = reads_of(&mut s, edit);
    let out = dir.path(&format!("out.{to}"));
    s.execute("file.save_as", &json!({ "path": out })).map_err(|e| format!("the save as {to}: {e}"))?;
    let mut back = open(&out, to)?;
    agrees(&mut s, &mut back, edit, &reads, to == "diacraft" || edit.places)
}

/// Runs `trip` for each edit and each name, and fails with every trip that failed.
fn all_trips(edits: &[Edit], names: &[&str], trip: impl Fn(&str, &Edit) -> Result<(), String>) {
    let mut failures: Vec<String> = Vec::new();
    for edit in edits {
        for name in names {
            if let Err(why) = trip(name, edit) {
                failures.push(format!("[{name}] {}: {why}", edit.name));
            }
        }
    }
    assert!(failures.is_empty(), "{} of {} trips failed:\n{}", failures.len(), edits.len() * names.len(), failures.join("\n"));
}

// ------------------------------------------------------------------------------------------------------------------------------ tests

/// Sanity of the drawing: a save with no edit changes no part of a file but the properties, and reads back the sheets it read.
#[test]
fn a_plain_save_of_the_drawing_is_no_edit_in_any_type() {
    let mut failures = Vec::new();
    for ext in world::EXTENSIONS {
        let dir = Dir::new();
        let original = world::package(ext);
        let path = dir.write(&format!("world.{ext}"), &original);
        let mut s = match open(&path, ext) {
            Ok(s) => s,
            Err(why) => {
                failures.push(format!("[{ext}] {why}"));
                continue;
            }
        };
        if let Err(e) = s.execute("file.save", &json!({})) {
            failures.push(format!("[{ext}] the save: {e}"));
            continue;
        }
        let (changed, removed, added) = changed_parts(&original, &dir.read(&format!("world.{ext}")));
        let changed: Vec<String> = changed.into_iter().filter(|n| !n.starts_with("/docProps/")).collect();
        if !(changed.is_empty() && removed.is_empty() && added.is_empty()) {
            failures.push(format!("[{ext}] changed {changed:?}, removed {removed:?}, added {added:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_cell_written_survives_a_save_and_a_reopen_in_every_type_of_a_vsdx_file() {
    all_trips(&cell_edits(), &world::EXTENSIONS, trip_vsdx);
}

#[test]
fn a_row_added_or_deleted_survives_a_save_and_a_reopen_in_every_type_of_a_vsdx_file() {
    all_trips(&row_edits(), &world::EXTENSIONS, trip_vsdx);
}

#[test]
fn a_section_added_or_deleted_survives_a_save_and_a_reopen_in_every_type_of_a_vsdx_file() {
    all_trips(&section_edits(), &world::EXTENSIONS, trip_vsdx);
}

#[test]
fn a_chain_of_edits_and_an_edit_undone_survive_a_save_in_every_type_of_a_vsdx_file() {
    all_trips(&chained_edits(), &world::EXTENSIONS, trip_vsdx);
}

#[test]
fn every_edit_survives_a_save_and_a_reopen_in_a_diacraft_file() {
    all_trips(&all_edits(), &["diacraft"], |_, edit| trip_native(edit));
}

#[test]
fn every_edit_of_a_vsdx_drawing_survives_a_save_as_diacraft() {
    all_trips(&all_edits(), &["diacraft"], |to, edit| trip_across("vsdx", to, edit));
}

#[test]
fn every_edit_of_a_diacraft_drawing_survives_a_save_as_vsdx() {
    all_trips(&all_edits(), &["vsdx"], |to, edit| trip_across("diacraft", to, edit));
}

/// A file whose elements are written with a prefix and no default namespace: the patcher makes the cells, rows and sections it adds with that prefix, or the reader would not find them.
#[test]
fn every_edit_survives_in_a_file_whose_elements_carry_a_prefix() {
    all_trips(&all_edits(), &["vsdx", "vstm"], |ext, edit| trip_vsdx_in(&world::prefixed_package(ext), ext, edit));
}

/// The edits of a master's shape, in a stencil that has masters and no page, in both its types.
#[test]
fn a_master_edited_in_a_stencil_with_no_page_survives_a_save_and_a_reopen() {
    let edits: Vec<Edit> = all_edits().into_iter().filter(|e| e.reads.iter().all(|r| r.contains("\"master\""))).collect();
    assert!(edits.len() >= 10, "{} edits of a master", edits.len());
    all_trips(&edits, &["vssx", "vssm"], |ext, edit| trip_vsdx_in(&world::stencil_package(ext), ext, edit));
}

/// A save, another edit, and a save again, in one session, whatever the first edit was: the file the second save makes holds both.
#[test]
fn a_second_save_after_any_edit_is_no_refusal_and_keeps_both() {
    all_trips(&all_edits(), &["vsdx", "vssx"], |ext, edit| trip_twice(&world::package(ext), ext, edit));
}

/// A `.diacraft` template and a `.diacraft` stencil hold the sheets as a drawing does (the stencil has masters and no page: the edits of a master's shape).
#[test]
fn every_edit_survives_in_a_diacraft_template_and_a_master_edit_in_a_diacraft_stencil() {
    all_trips(&all_edits(), &["diacraft-template"], trip_native_as);
    let masters: Vec<Edit> = all_edits().into_iter().filter(|e| e.reads.iter().all(|r| r.contains("\"master\""))).collect();
    all_trips(&masters, &["diacraft-stencil"], trip_native_as);
}

/// A file whose parts are in the namespace the standard states, and one whose parts are in none.
#[test]
fn every_edit_survives_in_a_file_of_another_namespace() {
    use diacraft_testkit::package::Namespace;
    for ns in [Namespace::Core2011, Namespace::None] {
        all_trips(&all_edits(), &["vsdx"], |ext, edit| trip_vsdx_in(&world::package_in(ext, ns), ext, edit));
    }
}

/// The text of a part of a package.
fn text_of(bytes: &[u8], part: &str) -> String {
    String::from_utf8(parts_of(bytes).remove(part).unwrap_or_else(|| panic!("no part {part}"))).unwrap()
}

/// **A section a save rewrites has no `IX` but a Geometry section's, and a section no edit reaches keeps the file's own**. The drawing holds
/// an `IX` on the Connection section of shape 11 and on the User section of shape 10 (a file written by another program may), and on every Geometry section; an edit that reaches the first
/// takes it off, the second stays as the file had it, and every Geometry section keeps its `IX`; the sheets read back as they were edited (the model keeps the attribute until the file is read).
#[test]
fn a_section_a_sheet_edit_reaches_loses_its_ix_but_a_geometry_section_and_a_section_no_edit_reaches_keep_theirs() {
    let count = |xml: &str, tag: &str| xml.matches(tag).count();
    let original = world::package("vsdx");
    let before = text_of(&original, P1);
    let (connection, user, geometry) = ("<Section N=\"Connection\" IX=\"0\"", "<Section N=\"User\" IX=\"0\"", "<Section N=\"Geometry\" IX=\"");
    assert_eq!((count(&before, connection), count(&before, user)), (1, 1), "the drawing holds both");
    let dir = Dir::new();
    let path = dir.write("world.vsdx", &original);
    let mut s = open(&path, "vsdx").unwrap();
    add_row(&mut s, SH11, "Connections", json!({}));
    s.execute("file.save", &json!({})).unwrap();
    let saved = dir.read("world.vsdx");
    let after = text_of(&saved, P1);
    assert_eq!(count(&after, connection), 0, "the section the edit reached has no IX");
    assert!(after.contains("<Section N=\"Connection\">"), "{after}");
    assert_eq!(count(&after, user), 1, "the section no edit reached keeps the file's");
    assert_eq!(count(&after, geometry), count(&before, geometry), "every Geometry section keeps its IX");
    let mut back = open(&path, "vsdx").unwrap();
    assert!(sheets_differ(s.document().unwrap(), back.document().unwrap(), false).is_empty());
    // An edit that reaches the User section of shape 10 takes its number off too, and keeps the other as it was.
    set(&mut back, SH10, "User.Rate", "9");
    back.execute("file.save", &json!({})).unwrap();
    let last = text_of(&dir.read("world.vsdx"), P1);
    assert_eq!((count(&last, connection), count(&last, user)), (0, 0));
    assert_eq!(count(&last, geometry), count(&before, geometry));
}

/// A section of 31,273 rows (the most a file of the sample set holds in one) takes an insert near its start: every row after it is numbered again, the save patches each row's `IX` and no
/// other part of the file is touched, and the file reads back the rows as they are numbered. **The cost is held to counts** in the patcher's own test (`crates/sheetxml/tests/it/patch_moved.rs`):
/// here the save and the reopen end, in the size of the file.
#[test]
fn a_section_of_31273_rows_numbered_again_is_saved_and_read_back() {
    let n = 31_273u32;
    let rows: String = (1..=n)
        .map(|k| {
            let t = if k == 1 { "MoveTo" } else { "RelLineTo" };
            format!(r#"<Row T="{t}" IX="{k}"><Cell N="X" V="0.001"/><Cell N="Y" V="0"/></Row>"#)
        })
        .collect();
    let shape = format!(
        r#"<Shape ID="11" Type="Shape"><Cell N="Width" V="3"/><Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>{rows}</Section><Section N="Scratch"><Row IX="0"><Cell N="A" V="1"/></Row></Section></Shape>"#
    );
    let original = diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "Page-1").shapes(&shape)).build();
    let dir = Dir::new();
    let path = dir.write("big.vsdx", &original);
    let mut s = open(&path, "vsdx").unwrap();
    let started = std::time::Instant::now();
    call(&mut s, "sheet.add_row", SH11, json!({"section": "Geometry1", "after": 2}));
    s.execute("file.save", &json!({})).unwrap();
    eprintln!("an insert in 31,273 rows, renumbered and saved: {:?}", started.elapsed());
    let saved = dir.read("big.vsdx");
    let (changed, removed, added) = changed_parts(&original, &saved);
    let changed: Vec<String> = changed.into_iter().filter(|n| !n.starts_with("/docProps/")).collect();
    assert_eq!((changed, removed, added), (vec![P1.to_owned()], Vec::new(), Vec::new()));
    let back = open(&path, "vsdx").unwrap();
    let doc = back.document().unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry").unwrap();
    assert_eq!(section.rows.len(), n as usize + 1);
    assert_eq!(section.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), (1..=n + 1).map(Some).collect::<Vec<_>>());
    assert!(sheets_differ(s.document().unwrap(), back.document().unwrap(), false).is_empty());
}

/// The drawing as a `.diacraft` file's bytes.
fn native_world() -> Vec<u8> {
    let dir = Dir::new();
    let vsdx = dir.write("world.vsdx", &world::package("vsdx"));
    let mut maker = open(&vsdx, "vsdx").unwrap();
    let native = dir.path("world.diacraft");
    maker.execute("file.save_as", &json!({ "path": native })).unwrap();
    dir.read("world.diacraft")
}

/// The same in a `.diacraft` file: the entries a save copied are the baseline of the next, and a second save keeps both edits.
#[test]
fn a_second_save_after_any_edit_keeps_both_in_a_diacraft_file() {
    let bytes = native_world();
    all_trips(&all_edits(), &["diacraft"], |ext, edit| trip_twice(&bytes, ext, edit));
}
