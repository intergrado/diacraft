//! The items the narrow re-reviews of parked. Each case runs with the real writer, and the echo control at the end fails
//! every one of the cases written as `fn(&Saver)`.
//!
//! * H02: the row a new shape's layer is read back as is the **markup's** first row of the name, never the model's (two rows of one name, the model's first renamed).
//! * A group that gains an `Angle` (or a flip) at 0 beside another cell that differs is saved, not refused for a pin it did not move.
//! * `container.xml.max_nodes` below `max_read_xml_nodes`: the room a save has for copies is the lesser, as the reader's cap on a tree is.
//! * A legacy part (ODF 1.0, 1.1, or no version) says so when a connector's route could not be written.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use crate::drawing::Drawing;
use crate::write_core::{Open, Saver, echo, echo_says_edited, open, open_bytes, part, real};
use crate::write_kept_dropped::{layer_attr, layered_drawing};
use crate::write_layers_review::{many_runs, save_with, set_row_of, set_text_of};
use crate::write_new::{push_shape, rectangle};
use crate::write_runs::chars;
use diacraft_doc::{Cell, CellName, Document, Section, SectionKey, Value};
use diacraft_odg::{Limits, SaveNote, WriteError, Written, read};

/// The text a cell holds.
fn text_of(cell: Option<&Cell>) -> Option<String> {
    cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The `LayerMember` of the shape named `name` on page `page`.
fn layer_member(doc: &Document, page: usize, name: &str) -> Option<String> {
    let shape = doc.pages.get(page)?.shapes.iter().find(|s| s.name.as_deref() == Some(name))?;
    text_of(shape.sheet.cell("LayerMember"))
}

/// The names of the rows of the `Layer` section of page `page`.
fn layer_names(doc: &Document, page: usize) -> Vec<String> {
    let Some(section) = doc.pages.get(page).and_then(|p| p.sheet.section(&SectionKey::new("Layer", None)).cloned()) else { return Vec::new() };
    section.rows.iter().map(|r| text_of(r.cells.get("Name")).unwrap_or_default()).collect()
}

/// A rectangle named `name` on the layer `member` (`LayerMember`) of its page.
fn rectangle_on(id: u32, name: &str, member: &str) -> diacraft_doc::Shape {
    let mut shape = rectangle(id, name, (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(member)));
    shape
}

/// Puts the rows `names` as the `Layer` section of page `page`, each a row of the cells a read gives it.
fn set_layers(doc: &mut Document, page: usize, names: &[&str]) {
    let mut section = Section::new("Layer", None);
    for (ix, name) in names.iter().enumerate() {
        let mut row = diacraft_doc::Row { ix: Some(u32::try_from(ix).unwrap_or(u32::MAX)), ..diacraft_doc::Row::default() };
        for (cell, value) in [("Name", *name), ("NameUniv", *name)] {
            row.cells.insert(CellName::new(cell), Cell::constant(Value::from_text(value)));
        }
        section.rows.push(Arc::new(row));
    }
    let Some(page) = doc.pages.get_mut(page) else { return };
    let sheet = Arc::make_mut(&mut Arc::make_mut(page).sheet);
    sheet.sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
    sheet.sections.push(Arc::new(section));
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// H02: the row a name is read back as.

/// The markup's layers are `Notes` and `Notes` (two rows of one name); the model's row 0 is renamed `Comments`, so the model's rows are `Comments` and `Notes`. A new shape
/// on the model's row 1 (`Notes`) is written under `Notes` (the markup's row 1 has that name), and the reader gives that name back as the **markup's** first row of it, row
/// 0: not the model's row 1, so the cell is carried and the shape reopens on row 1. A writer that predicts the row from the model's first row of `Notes` (row 1) would
/// find the cell the model's, carry nothing, and the shape would reopen on row 0.
fn a_new_shape_on_a_name_two_rows_share_reads_back_as_the_markups_first_row_and_keeps_its_own(save: &Saver) {
    let mut d = layered_drawing();
    d.masters = d.masters.replace(r#"<draw:layer draw:name="Other" draw:display="printer"/>"#, r#"<draw:layer draw:name="Notes"/>"#);
    let mut o = open(&d);
    assert_eq!(layer_names(&o.doc, 0), ["Notes", "Notes"], "the markup's two rows of one name");
    set_layers(&mut o.doc, 0, &["Comments", "Notes"]);
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "R", "1"));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(layer_attr(&content, "R").as_deref(), Some("Notes"), "{content}");
    assert!(content.contains("diacraft:version"), "the layer the reader gives (row 0) is not the model's (row 1), so the cell is carried: {content}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(layer_member(&back.doc, 0, "R").as_deref(), Some("1"), "the shape reopens on the row it was on");
    assert_eq!(layer_names(&back.doc, 0), ["Comments", "Notes"]);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A group that gains a turn cell at 0.

/// A group of the file (`draw:g`, two shapes): the reader gives it no `Angle`, `FlipX` or `FlipY` (it makes them only when they are not 0).
fn group_drawing() -> Drawing {
    let body = format!(r#"<draw:g draw:name="G">{}{}</draw:g>"#, crate::write_core::custom("A", "1cm"), crate::write_core::custom("B", "6cm"));
    Drawing::a4().page(&body)
}

/// What a save of the group's sheet made by `change` gave: the model and the result.
type Saved = (Document, Result<Written, WriteError>);

/// The group's sheet changed by `change`, saved.
fn group_saved(o: &Open, save: &Saver, change: &dyn Fn(&mut diacraft_doc::Sheet)) -> Saved {
    let id = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|g| g.id).unwrap_or_else(|| panic!("no group"));
    let mut doc = o.doc.clone();
    change(Arc::make_mut(&mut crate::write_core::shape_mut(&mut doc, 0, id).sheet));
    let saved = save(&doc, &o.bytes, &o.index);
    (doc, saved)
}

fn zero() -> Cell {
    Cell::constant(Value::from_number(0.0).unwrap_or_else(|| Value::from_text("0")))
}

/// A group whose sheet gains an `Angle`, a `FlipX` or a `FlipY` at 0 (a cell the reader leaves out at 0, which is the same turn) beside another cell that differs is saved: the
/// cell at 0 is no change of the group's pin, and the other cell is the document file's. The save said the group's pin was changed, and refused, when the cell was in the
/// change the other cell made (`delta.cells` held the `(None, Some)` entry of the pose cell). A turn that is not 0, and a formula on the cell, are what they were: refused.
fn a_group_that_gains_an_angle_or_a_flip_at_0_beside_another_cell_is_saved(save: &Saver) {
    let o = open(&group_drawing());
    let group = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).cloned().unwrap_or_else(|| panic!("no group"));
    assert_eq!(group.kind, diacraft_doc::ShapeType::Group);
    for turn in ["Angle", "FlipX", "FlipY"] {
        assert!(group.sheet.cell(turn).is_none(), "the reader's group has no {turn}");
    }
    let comment = || Cell::constant(Value::from_text("hello"));
    for cells in [&["Angle"][..], &["FlipX"], &["FlipY"], &["Angle", "FlipX", "FlipY"]] {
        let (doc, saved) = group_saved(&o, save, &|sheet| {
            for cell in cells {
                sheet.cells.insert(CellName::new(cell), zero());
            }
            sheet.cells.insert(CellName::new("Comment"), comment());
        });
        let saved = saved.unwrap_or_else(|e| panic!("{cells:?}: the group was refused: {e}"));
        assert!(saved.edited, "{cells:?}: the comment is an edit");
        let back = open_bytes(saved.bytes.clone());
        let held = back.doc.pages.get(0).and_then(|p| p.shapes.get(0)).and_then(|g| text_of(g.sheet.cell("Comment")));
        assert_eq!(held.as_deref(), Some("hello"), "{cells:?}: the comment is carried");
        let again = save(&doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{cells:?}: {e}"));
        assert!(!again.edited && again.bytes == saved.bytes, "{cells:?}: a second save is no edit");
    }
    // The cell at 0 alone is the frame's, as it was: nothing to write.
    let (_, saved) = group_saved(&o, save, &|sheet| {
        sheet.cells.insert(CellName::new("Angle"), zero());
    });
    assert!(saved.is_ok_and(|w| !w.edited), "an Angle at 0 alone is no edit");
    // What was refused still is: a turn that is not 0 (a group's turn is not written), and a formula on the gained cell (the document file has no place for it).
    let (_, turned) = group_saved(&o, save, &|sheet| {
        sheet.cells.insert(CellName::new("Angle"), Cell::constant(Value::from_number(0.5).unwrap_or_else(|| Value::from_text("0.5"))));
        sheet.cells.insert(CellName::new("Comment"), comment());
    });
    assert!(matches!(&turned, Err(WriteError::Unsupported(words)) if words.contains("turned or flipped")), "a turn: {:?}", turned.map(|w| w.edited));
    let (_, formula) = group_saved(&o, save, &|sheet| {
        sheet.cells.insert(CellName::new("Angle"), Cell::read(Some("0 deg"), Some("0"), Some("DEG"), None, &mut 1_000));
        sheet.cells.insert(CellName::new("Comment"), comment());
    });
    assert!(matches!(&formula, Err(WriteError::Unsupported(words)) if words.contains("group's pin")), "a formula: {:?}", formula.map(|w| w.edited));
    // A frame cell taken out beside another change is refused, as before.
    let (_, taken) = group_saved(&o, save, &|sheet| {
        sheet.cells.remove("LocPinX");
        sheet.cells.insert(CellName::new("Comment"), comment());
    });
    assert!(
        matches!(&taken, Err(WriteError::Unsupported(words)) if words.contains("group's pin")),
        "a cell taken out: {:?}",
        taken.map(|w| w.edited)
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The cap on a tree is the lesser of `max_read_xml_nodes` and `container.xml.max_nodes`.

/// The recipe of a style of 100 unmapped attributes named by many spans, each run given a size of its own: a copy of the whole style for each run,
/// with the reader's cap on a tree set by **`container.xml.max_nodes`** (60,000) and `max_read_xml_nodes` left at its default (4,000,000). The reader's trees are capped by
/// the lesser of the two (`read::mod`), so a save is held to the lesser: the file it makes reads again with the limits it was saved with, or the save is refused in words.
/// A room taken from `max_read_xml_nodes` alone let the copies of 600 runs through, and the file they made was `xml_too_complex` when it was opened again.
#[test]
fn a_save_is_held_to_the_lesser_of_the_two_caps_on_a_tree_when_container_xml_max_nodes_is_it() {
    let mut limits = Limits::default();
    limits.container.xml.max_nodes = 60_000;
    assert!(limits.max_read_xml_nodes > limits.container.xml.max_nodes, "the other cap is the larger");
    for (spans, saves) in [(30usize, true), (400, true), (600, false), (1_000, false)] {
        let o = many_runs(spans, 100, &limits);
        let (saved, work) = save_with(&o, &limits);
        match saved {
            Ok(saved) => {
                assert!(saves, "{spans} spans: the save made a file with {} items charged and was expected to be refused", work.copied_items);
                let back =
                    read(&saved.bytes, &limits).unwrap_or_else(|e| panic!("{spans} spans: the file a save made does not read: {} ({e})", e.class()));
                assert!(back.0.pages.get(0).is_some_and(|p| !p.shapes.is_empty()), "{spans} spans: the shape is there");
            }
            Err(error) => {
                assert!(!saves, "{spans} spans: the save was refused and was expected to be made: {error}");
                assert_eq!(error.class(), "write_limit", "{error}");
                assert!(error.to_string().contains("items in a part"), "in words: {error}");
                // The words name the cap that held: the lesser.
                assert!(matches!(error, WriteError::Limit { max: 60_000, .. }), "the cap that held is named: {error:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The index of a part's styles is not walked at each edit in place.

/// What the shapes `x<k>` of [`in_place_page`] are given.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edit {
    /// Each paragraph is set to the right: its style is edited where it stands (nothing else names it).
    InPlace,
    /// Each shape is moved: no style is touched, the same number of shapes is edited and the same file is read.
    Moved,
    /// Each paragraph's text is retyped (`ab` to `ac`): no style is touched, and the shape goes the way of a shape whose text changed (the yardstick of the timed run).
    Retyped,
}

/// A page of `n` shapes `x0` to `x<n-1>`, each of one paragraph on a style of its own (`P<k>`: centred, with a left margin that is its number in millimetres, so that the
/// `n` styles are `n` keys apart), and `w`, the first shape, with one plain paragraph. Each `x<k>` is edited as `edit` says, and `w` gains a justified paragraph, which asks
/// the part's styles for one that says so and makes the index of them (every one of them, `n + 1` keys) **before** the edits of the `x`s, which is the order the shapes are
/// written in.
pub(crate) fn in_place_page(n: usize, edit: Edit) -> Open {
    let styles: String = (0..n)
        .map(|k| {
            format!(
                r#"<style:style style:name="P{k}" style:family="paragraph"><style:paragraph-properties fo:text-align="center" fo:margin-left="{k}mm"/></style:style>"#
            )
        })
        .collect();
    let mut body = crate::write_runs::shape("w", "0cm", "<text:p>zz</text:p>");
    for k in 0..n {
        body.push_str(&crate::write_runs::shape(&format!("x{k}"), "6cm", &format!(r#"<text:p text:style-name="P{k}">ab</text:p>"#)));
    }
    let mut o = open(&Drawing::a4().auto(&styles).page(&body));
    let ids: Vec<u32> = o.doc.pages.get(0).map(|p| p.shapes.iter().map(|s| s.id).collect()).unwrap_or_default();
    assert_eq!(ids.len(), n + 1);
    set_row_of(&mut o, ids[0], "Paragraph", 1, &[("HorzAlign", 3.0)]);
    set_text_of(&mut o, ids[0], vec![chars("zz\n"), diacraft_doc::TextPart::Pp(1), chars("new")]);
    // Every `x` edited, in one pass over the page (its other cells as they were).
    let number = |v: f64| Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0")));
    let page = crate::write_page_ops::page_mut(&mut o.doc, 0);
    for at in 1..=n {
        let Some(shape) = page.shapes.get_mut(at) else { panic!("no shape {at}") };
        let shape = Arc::make_mut(shape);
        if edit == Edit::Retyped {
            let text = Arc::make_mut(shape.text.as_mut().unwrap_or_else(|| panic!("no text")));
            for part in &mut text.parts {
                if matches!(part, diacraft_doc::TextPart::Chars(c) if &**c == "ab") {
                    *part = chars("ac");
                }
            }
            continue;
        }
        let sheet = Arc::make_mut(&mut shape.sheet);
        match edit {
            Edit::InPlace => {
                let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Paragraph").unwrap_or_else(|| panic!("no Paragraph section"));
                let row = Arc::make_mut(Arc::make_mut(section).rows.iter_mut().find(|r| r.ix == Some(0)).unwrap_or_else(|| panic!("no row 0")));
                row.cells.insert(CellName::new("HorzAlign"), number(2.0));
            }
            Edit::Moved => {
                let pin = sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0);
                sheet.cells.insert(CellName::new("PinX"), number(pin + 0.1));
            }
            Edit::Retyped => {}
        }
    }
    o
}

/// What a save of [`in_place_page`] of `n` edits took of steps, after it has been shown that every edit was made in place (a style edited where it stands is four items
/// of the part here, two attributes and the room of an element, and a copy is more) and that the styles say right and keep their margins.
fn steps_of_in_place_edits(n: usize) -> u64 {
    let o = in_place_page(n, Edit::InPlace);
    let (saved, work) = save_with(&o, &Limits::default());
    let saved = saved.unwrap_or_else(|e| panic!("{n} edits: {e}"));
    assert_eq!(
        work.copied_items,
        4 * n as u64,
        "{n} edits: each is a style edited where it stands (its ops and the room of an element), and no copy is made"
    );
    let content = part(&saved.bytes, "content.xml");
    for k in [0, n / 2, n - 1] {
        let style = crate::write_kept_dropped::style_of(&content, &format!("P{k}"));
        assert!(
            style.as_ref().is_some_and(|s| s.contains(r#"fo:text-align="right""#) && s.contains(&format!(r#"fo:margin-left="{k}mm""#))),
            "{n} edits: P{k} is edited where it stands: {style:?}"
        );
    }
    assert!(
        crate::write_kept_dropped::style_of(&content, &format!("P{n}")).is_some(),
        "{n} edits: the one paragraph put in has a style of its own: {content}"
    );
    work.order_steps
}

/// A page of N edits in place takes work linear in N: the steps of 400, 800 and 1,600 edits grow with them (a doubling is a doubling of the steps, within a tenth).
/// Walking the index of the part's styles to forget the one an edit changed (`index.retain`, a step for each of its keys, counted once it could be seen) gave 83,407, 326,807 and
/// 1,293,607 steps for these three: four times the steps for twice the edits. Without the walk they are 3,207, 6,407 and 12,807.
#[test]
fn a_page_of_edits_in_place_takes_work_linear_in_the_edits() {
    let steps: Vec<u64> = [400usize, 800, 1_600].iter().map(|n| steps_of_in_place_edits(*n)).collect();
    eprintln!("IN-PLACE steps for 400, 800 and 1,600 edits: {steps:?}");
    for pair in steps.windows(2) {
        let (small, large) = (pair[0] as f64, pair[1] as f64);
        assert!(large > small * 1.9 && large < small * 2.1, "doubling the edits does not double the steps: {steps:?}");
    }
}

/// The shapes `Document::find_shape` visited in a save of [`in_place_page`] of `n` edits (the read back of the paragraphs asks for each shape it checks, in a part where a
/// paragraph was put in), by the model's own counter.
fn finds_of_in_place_edits(n: usize, edit: Edit) -> u64 {
    let o = in_place_page(n, edit);
    diacraft_doc::work::reset();
    let (saved, _) = save_with(&o, &Limits::default());
    assert!(saved.is_ok_and(|w| w.edited), "{n} edits: saved");
    diacraft_doc::work::finds_visited()
}

/// The read back of the paragraphs of a part where one was put in checks each shape whose text was written, and looks each one up **in a pass over the page made once**: the
/// shapes visited by those lookups grow with the edits and not with their square. A search of the page from the top for each shape (`Document::find_shape`) visited 80,000,
/// 320,000 and 1,280,000 shapes for 400, 800 and 1,600 edits: four times for twice the edits.
#[test]
fn the_read_back_of_a_page_of_edits_looks_each_shape_up_in_one_pass() {
    for edit in [Edit::InPlace, Edit::Retyped] {
        let finds: Vec<u64> = [400usize, 800, 1_600].iter().map(|n| finds_of_in_place_edits(*n, edit)).collect();
        eprintln!("FINDS visited for 400, 800 and 1,600 edits ({}): {finds:?}", if edit == Edit::InPlace { "in place" } else { "retyped" });
        assert!(finds[2] <= finds[0] * 4 + 64 && finds[1] <= finds[0] * 2 + 64, "doubling the edits more than doubles the shapes visited: {finds:?}");
        assert!(finds[2] <= 16 * 1_600, "{finds:?}");
    }
}

/// The time of the same, by hand: 20,000, 40,000 and 80,000 edits in place beside two yardsticks of the same size (the same page, the same number of shapes each moved, or each
/// with its text retyped, no style touched). What is looked at is the ratio of the edits in place to the retyped: it stays about where it is when the edits cost linear time
/// and grows with the page when each edit walks the index.
/// `cargo nextest run -p diacraft-odg --run-ignored only -E 'test(edits_in_place_timed)' --no-capture`.
#[test]
#[ignore = "timing, by hand: 20,000 to 80,000 edits in place beside a yardstick"]
fn edits_in_place_timed_beside_a_yardstick() {
    for n in [20_000usize, 40_000, 80_000] {
        let time = |edit: Edit| {
            let o = in_place_page(n, edit);
            eprintln!(
                "SAVE-START n={n} {}",
                if edit == Edit::InPlace {
                    "in place"
                } else if edit == Edit::Retyped {
                    "retyped"
                } else {
                    "moved"
                }
            );
            let started = std::time::Instant::now();
            let (saved, work) = save_with(&o, &Limits::default());
            let elapsed = started.elapsed();
            assert!(saved.is_ok_and(|w| w.edited), "{n}: saved");
            (elapsed, work.order_steps)
        };
        let ((edits, edit_steps), (retyped, retyped_steps), (moved, moved_steps)) = (time(Edit::InPlace), time(Edit::Retyped), time(Edit::Moved));
        eprintln!(
            "IN-PLACE-TIME n={n}: edits in place {:.3} s ({edit_steps} steps), yardstick retyped {:.3} s ({retyped_steps} steps), yardstick moved {:.3} s ({moved_steps} steps), ratio to retyped {:.2}",
            edits.as_secs_f64(),
            retyped.as_secs_f64(),
            moved.as_secs_f64(),
            edits.as_secs_f64() / retyped.as_secs_f64()
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A connector routed anew in a part of ODF 1.0 or 1.1.

/// A connector routed anew (its shape moved, its begin with it, and its route set as the engine sets one) in a part of ODF 1.0, ODF 1.1 or one that declares no version:
/// the ends are written, and no `svg:d`, `svg:viewBox` or `draw:line-skew` (those versions have no place for them, and one the file had is taken out), and **the save says
/// so** with one note that names the connector. Before, it said nothing. In ODF 1.2 and later the route is written and nothing is said. A connector that is not routed
/// anew (its line weight changed) and a save again with nothing changed say nothing either.
fn a_connector_routed_anew_in_a_legacy_part_is_said_to_have_lost_its_route(save: &Saver) {
    use crate::drawing::cm;
    use crate::write_core::{move_by, num_of, set_num, set_num_new, shape_mut};
    use crate::write_faults::versioned;
    use crate::write_more::{CONNECTOR_BODY, set_route_end};
    let bare =
        CONNECTOR_BODY.replacen("xml:id=\"a\"", "draw:id=\"a\"", 1).replacen(" svg:d=\"M 6000 4000 h 4000\" svg:viewBox=\"0 0 4001 1\"", "", 1);
    let skewed = CONNECTOR_BODY.replacen("svg:x1=\"6cm\"", "draw:line-skew=\"1cm\" svg:x1=\"6cm\"", 1);
    assert!(!bare.contains("svg:d") && skewed.contains("draw:line-skew=\"1cm\" svg:x1") && skewed.contains("svg:d"));
    for version in [None, Some("1.0"), Some("1.1"), Some("1.2"), Some("1.3")] {
        let legacy = !matches!(version, Some("1.2" | "1.3"));
        for (held, body) in [("no path in the file", bare.as_str()), ("a path and a skew in the file", skewed.as_str())] {
            let what = format!("version {version:?}, {held}");
            let o = open_bytes(versioned(&Drawing::a4().page(body), version));
            let connector_tag = |bytes: &[u8]| {
                let content = part(bytes, "content.xml");
                let at = content.find("<draw:connector").unwrap_or_else(|| panic!("{what}: no connector: {content}"));
                content[at..at + content[at..].find('>').unwrap_or(0) + 1].to_owned()
            };
            // The shape moves 1 cm and the connector follows and is routed again, as the engine does it.
            let mut doc = o.doc.clone();
            move_by(&mut doc, 0, 1, 1.0, 0.0);
            {
                let c = shape_mut(&mut doc, 0, 2);
                let x = num_of(c, "BeginX");
                set_num(c, "BeginX", x + cm(1.0));
                set_route_end(c, cm(3.0));
            }
            let saved = save(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            let tag = connector_tag(&saved.bytes);
            assert!(tag.contains("svg:x1=\"7cm\" svg:y1=\"4cm\" svg:x2=\"10cm\" svg:y2=\"4cm\""), "{what}: the ends follow the shape: {tag}");
            assert!(!tag.contains("draw:line-skew"), "{what}: a route written anew has no skew: {tag}");
            if legacy {
                assert!(!tag.contains("svg:d=") && !tag.contains("svg:viewBox="), "{what}: no place for a path: {tag}");
                assert_eq!(saved.notes, vec![SaveNote::RouteDroppedForVersion { page: 0, shape: 2 }], "{what}: said, naming the connector");
                let words = saved.notes[0].to_string();
                assert!(words.contains("connector 2 on page 0") && words.contains("ODF 1.0 or 1.1"), "{what}: {words}");
            } else {
                assert!(tag.contains("svg:d=\"M 7000 4000 h 3000\" svg:viewBox=\"0 0 3001 1\""), "{what}: the route is written: {tag}");
                assert!(saved.notes.is_empty(), "{what}: nothing to say: {:?}", saved.notes);
            }
            // A second save of the model as it is: no edit, nothing said again.
            let again = save(&doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert!(!again.edited && again.notes.is_empty(), "{what}: said once: {:?}", again.notes);
            // The connector's line weight alone: its route is not written anew, and nothing is said.
            let mut weight = o.doc.clone();
            set_num_new(shape_mut(&mut weight, 0, 2), "LineWeight", 0.05);
            let styled = save(&weight, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert!(styled.notes.is_empty(), "{what}: a restyle routes nothing anew: {:?}", styled.notes);
            assert_eq!(
                tag_without_style(&connector_tag(&styled.bytes)),
                tag_without_style(&connector_tag(&o.bytes)),
                "{what}: the route is the file's"
            );
        }
    }
}

/// A start tag without its style name (a restyle gives the connector one).
fn tag_without_style(tag: &str) -> String {
    let closed = tag.ends_with("/>");
    let words: Vec<&str> = tag.trim_end_matches("/>").trim_end_matches('>').split(' ').filter(|word| !word.starts_with("draw:style-name=")).collect();
    format!("{}{}", words.join(" "), if closed { "/>" } else { ">" })
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A black-box check against LibreOffice (not part of the suite: both tests are ignored, and run by hand with `ODG_R8FIX_DIR` set to an empty folder).
//
// 1. `lo_write_2` writes `in/*.odg`: the save of a group that gained an `Angle` at 0 beside a comment (the markup that changed: the group's own entry of DiaCraft's
//    document file), and the saves of a connector routed anew in a part of ODF 1.0, 1.1 and no version (the markup that changed: the ends, no path, view box or skew), for a
//    connector that had a path and a skew in the file and one that had none, and in ODF 1.3 for comparison.
// 2. LibreOffice converts them as a black box: `soffice -env:UserInstallation=file://<folder>/profile --headless --convert-to odg --outdir out in/*.odg`, and `--convert-to pdf`;
//    only a missing output file is a failure.
// 3. `lo_read_2` reads `out/*.odg` with the reader and says what came back: the group's members, and the ends of each connector.

fn lo_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ODG_R8FIX_DIR").expect("set ODG_R8FIX_DIR to an empty folder"))
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_R8FIX_DIR set"]
fn lo_write_2() {
    use crate::drawing::cm;
    use crate::write_core::{move_by, num_of, set_num, shape_mut};
    use crate::write_faults::versioned;
    use crate::write_more::{CONNECTOR_BODY, set_route_end};
    let dir = lo_dir().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    // The group: an `Angle` at 0 and a comment.
    let o = open(&group_drawing());
    let (_, saved) = group_saved(&o, &real, &|sheet| {
        sheet.cells.insert(CellName::new("Angle"), zero());
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("hello")));
    });
    put("group_angle0_comment.odg", &saved.unwrap().bytes);
    // The connector, routed anew in each version, with a path and a skew in the file and with none.
    let bare =
        CONNECTOR_BODY.replacen("xml:id=\"a\"", "draw:id=\"a\"", 1).replacen(" svg:d=\"M 6000 4000 h 4000\" svg:viewBox=\"0 0 4001 1\"", "", 1);
    let skewed = CONNECTOR_BODY.replacen("svg:x1=\"6cm\"", "draw:line-skew=\"1cm\" svg:x1=\"6cm\"", 1);
    for (version, label) in [(None, "none"), (Some("1.0"), "1_0"), (Some("1.1"), "1_1"), (Some("1.3"), "1_3")] {
        for (held, body) in [("bare", &bare), ("skewed", &skewed)] {
            let o = open_bytes(versioned(&Drawing::a4().page(body), version));
            let mut doc = o.doc.clone();
            move_by(&mut doc, 0, 1, 1.0, 0.0);
            {
                let c = shape_mut(&mut doc, 0, 2);
                let x = num_of(c, "BeginX");
                set_num(c, "BeginX", x + cm(1.0));
                set_route_end(c, cm(3.0));
            }
            let saved = real(&doc, &o.bytes, &o.index).unwrap();
            println!("{label} {held}: notes {:?}", saved.notes.iter().map(ToString::to_string).collect::<Vec<_>>());
            put(&format!("connector_{label}_{held}.odg"), &saved.bytes);
        }
    }
    println!("wrote the files in {}", dir.display());
}

#[test]
#[ignore = "reads the files LibreOffice converted, run by hand with ODG_R8FIX_DIR set"]
fn lo_read_2() {
    let out = lo_dir().join("out");
    let mut names: Vec<String> =
        std::fs::read_dir(&out).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.ends_with(".odg")).collect();
    names.sort();
    for name in names {
        let bytes = std::fs::read(out.join(&name)).unwrap();
        let (doc, _) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("out/{name} did not read: {e}"));
        let page = doc.pages.get(0).unwrap();
        let described: Vec<String> = page
            .shapes
            .iter()
            .map(|s| {
                let n = |c: &str| s.sheet.cell(c).and_then(|c| c.value.as_ref()?.number()).map(|v| (v * 2.54 * 100.0).round() / 100.0);
                match s.kind {
                    diacraft_doc::ShapeType::Group => {
                        format!("group {:?} with {} members, comment {:?}", s.name, s.children.len(), text_of(s.sheet.cell("Comment")))
                    }
                    _ if s.sheet.cell("BeginX").is_some() => {
                        format!("connector {:?} from ({:?}, {:?}) to ({:?}, {:?}) cm", s.name, n("BeginX"), n("BeginY"), n("EndX"), n("EndY"))
                    }
                    _ => format!("shape {:?} at ({:?}, {:?}) cm", s.name, n("PinX"), n("PinY")),
                }
            })
            .collect();
        println!("{name}: {described:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------

/// The cases of the echo control.
type Case = (&'static str, fn(&Saver));

const CASES: &[Case] = &[
    (
        "a_connector_routed_anew_in_a_legacy_part_is_said_to_have_lost_its_route",
        a_connector_routed_anew_in_a_legacy_part_is_said_to_have_lost_its_route,
    ),
    (
        "a_new_shape_on_a_name_two_rows_share_reads_back_as_the_markups_first_row_and_keeps_its_own",
        a_new_shape_on_a_name_two_rows_share_reads_back_as_the_markups_first_row_and_keeps_its_own,
    ),
    (
        "a_group_that_gains_an_angle_or_a_flip_at_0_beside_another_cell_is_saved",
        a_group_that_gains_an_angle_or_a_flip_at_0_beside_another_cell_is_saved,
    ),
];

#[test]
fn a_new_shape_on_a_name_two_rows_share_reads_back_as_the_markups_first_row_and_keeps_its_own_test() {
    a_new_shape_on_a_name_two_rows_share_reads_back_as_the_markups_first_row_and_keeps_its_own(&real);
}

#[test]
fn a_group_that_gains_an_angle_or_a_flip_at_0_beside_another_cell_is_saved_test() {
    a_group_that_gains_an_angle_or_a_flip_at_0_beside_another_cell_is_saved(&real);
}

#[test]
fn a_connector_routed_anew_in_a_legacy_part_is_said_to_have_lost_its_route_test() {
    a_connector_routed_anew_in_a_legacy_part_is_said_to_have_lost_its_route(&real);
}

/// The echo control: a writer that gives the original back fails every case, and the real writer passes each.
#[test]
fn echo_control_fails_every_case() {
    for (name, case) in CASES {
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo))).is_err(), "the echo writer passed the case {name}: the case checks nothing");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&real))).is_ok(), "the real writer failed the case {name}");
    }
}
