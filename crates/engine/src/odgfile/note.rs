//! The save note of an `.odg` (the `.odg` design's decision 8).
//!
//! **When.** The note appears **only when the drawing holds behaviour a LibreOffice save would drop**, which is cross-shape formulas and shape data (the design also
//! lists master links; the owner ruled them out of the trigger, 2026-11-17: a shape dropped from a stencil is linked to a master in every session, and the note
//! would be said after every first drop): DiaCraft keeps those in its own markup, LibreOffice does not run them, drops what is kept outside the shapes' styles
//! And does not update what stays. A drawing of plain shapes, text, styles, pages and connectors, which a LibreOffice
//! save keeps whole, gets none. The note is about **the drawing as the file holds it**: the copy the writer is given, in which each new shape is what a new shape of an
//! `.odg` is (its links to its master kept, no data rows: `flatten.rs`), and the shapes the file already had.
//!
//! * **cross-shape formulas**: a shape one of whose cells (its own, or a cell of a row of one of its sections) holds a formula that names another shape (`Sheet.<ID>`
//!   with another shape's `ID`), but the ends and triggers of a connector, which are its glue (ODF holds glue, and a LibreOffice save keeps it);
//! * **shape data**: a shape with a `Property` row (the section the model keeps shape data in, as the VSDX reader builds it), or a `User` row but the `DiaCraftRoute` that marks a connector's route (the engine's own, and the reader gives every
//!   connector one).
//!
//! A shape that is an instance of a master is **not** counted.
//!
//! **Words.** The probe's sentence, after what the drawing holds. **Where.** Programs get the note as `notes` in the result of `file.save` and
//! `file.save_as` for an `.odg`: an array, empty when there is none, of `{class, words, cross_shape_formulas, shape_data}`. It is never a warning
//! (`warnings` says what a save did not write); the window shows it once per session.
//!
//! **Cost.** A shape, a cell, a section, a row and each 64 bytes of a formula is a step, held to [`MAX_STEPS`] like the flattening's: past it the count stops and what
//! was found stands (the note is for information, and a drawing that large has long since shown it).

use diacraft_doc::{Cell, Document, FormulaSlot, MAX_SHAPE_DEPTH, Shape, ShapeList};
use diacraft_formula::{Expr, Qualifier};
use serde_json::{Value, json};

use super::flatten::MAX_STEPS;

/// The `class` of the note in the save's `notes`.
pub(crate) const CLASS: &str = "libreoffice_would_drop";

/// What a LibreOffice save does with DiaCraft's data: the probe's sentence, and the two things that follow from its other rules.
const WHAT_A_SAVE_DOES: &str = "A LibreOffice save keeps the shapes, text, styles, pages and connectors, but drops DiaCraft data kept outside the shapes' styles, renumbers shape ids, glue points and style names, and recomputes connector routes. What is changed in LibreOffice does not update DiaCraft's data, and a shape pasted there copies it.";

/// The cells of a connector the reader rebuilds from the file's glue: the four ends and the two triggers.
const GLUE_CELLS: [&str; 6] = ["BeginX", "BeginY", "EndX", "EndY", "BegTrigger", "EndTrigger"];

/// The name of the `User` row that marks a connector's route: the engine writes it and the reader gives every connector one, so it
/// is no data of a person's.
const ROUTE_ROW: &str = "DiaCraftRoute";

/// What a drawing holds that a LibreOffice save would drop, counted by shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Found {
    /// Shapes with a formula that names another shape.
    pub cross_shape_formulas: u64,
    /// Shapes with shape data (`Property` rows, or `User` rows beside the route marker).
    pub shape_data: u64,
    /// The steps the count took (the cost a test holds to the size of the drawing).
    pub steps: u64,
    /// The count stopped at the bound of steps: what was found stands.
    pub stopped: bool,
}

impl Found {
    /// Whether the drawing holds anything a LibreOffice save would drop.
    pub(crate) fn any(&self) -> bool {
        self.cross_shape_formulas > 0 || self.shape_data > 0
    }
}

/// The shape a formula is in: its `ID`, and its `NameU` when it has one (a formula reads a shape by `Sheet.<ID>` or by its name, `B!Width`, `B.2!Width`).
#[derive(Clone, Copy)]
struct Own<'a> {
    id: u32,
    name: Option<&'a str>,
}

/// Whether a formula that parsed reads a shape other than the one it is in (the review: the note looked for `Sheet.<ID>` in the text and missed
/// `B!Width`). The qualifiers of every reference and every call are walked, in the arguments of calls and the operands of operators too, with a stack of its own so that
/// a tree of any depth is walked: `Sheet.<ID>` is another shape when the `ID` is not `own`'s; a name is another shape unless it is `own`'s name (and the `ID` after it,
/// `B.2`, is `own`'s); `ThePage`, `TheDoc` and `Styles` name no shape, and a qualifier after `Pages[..]` or `Masters[..]` is a shape of another part, and a call that
/// returns a sheet (`LISTSHEETREF(2)!Width`) reads one the formula chooses: both count.
fn reads_another_shape(expr: &Expr, own: Own<'_>) -> bool {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(r) => {
                if qualifiers_read_another(&r.qualifiers, own, &mut stack) {
                    return true;
                }
            }
            Expr::Call(c) => {
                if qualifiers_read_another(&c.qualifiers, own, &mut stack) {
                    return true;
                }
                stack.extend(c.args.iter().flatten());
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            _ => {}
        }
    }
    false
}

/// Whether the qualifiers before a `!` read a shape other than `own`; the arguments of a qualifier that is a call are put on `stack` to be walked.
fn qualifiers_read_another<'e>(qualifiers: &'e [Qualifier], own: Own<'_>, stack: &mut Vec<&'e Expr>) -> bool {
    let (mut elsewhere, mut styles) = (false, false);
    for qualifier in qualifiers {
        let segments = match qualifier {
            Qualifier::Call { args, .. } => {
                stack.extend(args.iter().flatten());
                return true;
            }
            Qualifier::Path(segments) => segments,
        };
        let word = |s: &diacraft_formula::Segment, w: &str| s.index.is_none() && s.name.eq_ignore_ascii_case(w);
        match segments.as_slice() {
            [one] if word(one, "ThePage") || word(one, "TheDoc") => {}
            [one] if word(one, "Styles") => styles = true,
            [one] if one.index.is_some() && (one.name.eq_ignore_ascii_case("Pages") || one.name.eq_ignore_ascii_case("Masters")) => elsewhere = true,
            [sheet, id] if word(sheet, "Sheet") && id.index.is_none() => {
                if let Ok(n) = id.name.parse::<u32>()
                    && (elsewhere || n != own.id)
                {
                    return true;
                }
            }
            [name] if name.index.is_none() && !styles => {
                if elsewhere || !own.name.is_some_and(|n| name.name.eq_ignore_ascii_case(n)) {
                    return true;
                }
            }
            [name, id] if name.index.is_none() && id.index.is_none() && !styles => {
                let same = own.name.is_some_and(|n| name.name.eq_ignore_ascii_case(n)) && id.name.parse::<u32>().is_ok_and(|n| n == own.id);
                if elsewhere || !same {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Whether `formula` names a shape by its `ID` (`Sheet.<ID>`, in any case) other than `own`: the scan of a text that did not parse.
pub(crate) fn names_another_shape(formula: &str, own: u32) -> bool {
    let bytes = formula.as_bytes();
    let mut at: usize = 0;
    while let Some(window) = bytes.get(at..at.saturating_add(6)) {
        if window.eq_ignore_ascii_case(b"sheet.") {
            let digits = bytes.get(at + 6..).unwrap_or_default().iter().take_while(|b| b.is_ascii_digit()).count();
            let number = formula.get(at + 6..at + 6 + digits).and_then(|d| d.parse::<u32>().ok());
            if number.is_some_and(|id| id != own) {
                return true;
            }
            at += 6;
        } else {
            at += 1;
        }
    }
    false
}

/// The steps a bounded count has left to take, and what it counted.
struct Count {
    found: Found,
    most: u64,
}

impl Count {
    /// Takes `n` steps. `false` when that is past the bound: the count stops.
    fn take(&mut self, n: u64) -> bool {
        self.found.steps = self.found.steps.saturating_add(n);
        if self.found.steps > self.most {
            self.found.stopped = true;
        }
        !self.found.stopped
    }

    /// Looks at the cells of a shape, a section or a row: each is a step, and each 64 bytes of a formula another. Sets `cross` when one names a shape other than
    /// `own` (and looks at no formula after that). `false` when the count stopped.
    fn cells<'a>(&mut self, cells: impl Iterator<Item = &'a Cell>, own: Own<'_>, cross: &mut bool) -> bool {
        for cell in cells {
            let Some(text) = cell.formula.text().filter(|_| !*cross) else {
                if !self.take(1) {
                    return false;
                }
                continue;
            };
            let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
            if !self.take((bytes / 64).saturating_add(1)) {
                return false;
            }
            *cross = match &cell.formula {
                FormulaSlot::Local(f) => match &f.ast {
                    Ok(expr) => reads_another_shape(expr, own),
                    Err(_) => names_another_shape(text, own.id),
                },
                _ => false,
            };
        }
        true
    }
}

/// What the shape holds that a LibreOffice save would drop: (a formula that names another shape, shape data). `None` when the count stopped.
fn of_shape(count: &mut Count, shape: &Shape) -> Option<(bool, bool)> {
    if !count.take(1) {
        return None;
    }
    let (mut cross, mut data) = (false, false);
    let own = Own { id: shape.id, name: shape.name_u.as_deref() };
    // The ends of a connector and its triggers are rebuilt from its glue, which ODF holds (a LibreOffice save keeps a connector glued): their formulas name the shapes
    // glued, whoever made them, and are no behaviour that is dropped.
    let own_cells = shape.sheet.cells.iter().filter(|(name, _)| !GLUE_CELLS.contains(&name.as_str())).map(|(_, cell)| cell);
    if !count.cells(own_cells, own, &mut cross) {
        return None;
    }
    for section in &shape.sheet.sections {
        if !count.take(1) || !count.cells(section.cells.values(), own, &mut cross) {
            return None;
        }
        for row in section.rows.iter() {
            if !count.take(1) || !count.cells(row.cells.values(), own, &mut cross) {
                return None;
            }
            data = data
                || match section.name.as_str() {
                    "Property" => true,
                    "User" => row.name.as_deref() != Some(ROUTE_ROW),
                    _ => false,
                };
        }
    }
    Some((cross, data))
}

/// What `doc` holds that a LibreOffice save would drop, counted by shape over every page, groups and members included, in at most `most` steps.
pub(crate) fn find_within(doc: &Document, most: u64) -> Found {
    let mut count = Count { found: Found::default(), most };
    'pages: for page in &doc.pages {
        // A stack of lists with the place in each: the space it takes follows the depth and not the page's width.
        let mut stack: Vec<(&ShapeList, usize)> = vec![(&page.shapes, 0)];
        while let Some((shapes, next)) = stack.last_mut() {
            let Some(shape) = shapes.get(*next) else {
                stack.pop();
                continue;
            };
            *next = next.saturating_add(1);
            let Some((cross, data)) = of_shape(&mut count, shape) else { break 'pages };
            count.found.cross_shape_formulas += u64::from(cross);
            count.found.shape_data += u64::from(data);
            if !shape.children.is_empty() && stack.len() < MAX_SHAPE_DEPTH {
                stack.push((&shape.children, 0));
            }
        }
    }
    count.found
}

/// What `doc` holds that a LibreOffice save would drop, in the bound every flattening is held to.
pub(crate) fn find(doc: &Document) -> Found {
    find_within(doc, MAX_STEPS)
}

/// A shape or two shapes, and the other counts, in words: "2 shapes with formulas that read other shapes, 1 shape with data rows".
fn parts(found: &Found) -> String {
    let one = |n: u64, singular: &str, plural: &str| format!("{n} {}", if n == 1 { singular } else { plural });
    let mut said: Vec<String> = Vec::new();
    if found.cross_shape_formulas > 0 {
        said.push(one(found.cross_shape_formulas, "shape with a formula that reads another shape", "shapes with formulas that read other shapes"));
    }
    if found.shape_data > 0 {
        said.push(one(found.shape_data, "shape with data rows", "shapes with data rows"));
    }
    said.join(", ")
}

/// The note for what was found, as the save reports it: `None` when the drawing holds nothing a LibreOffice save would drop.
pub(crate) fn note(found: &Found) -> Option<Value> {
    found.any().then(|| {
        let more = if found.stopped { " (and perhaps more: the drawing is too large to count it all)" } else { "" };
        json!({
            "class": CLASS,
            "words": format!("This drawing holds behaviour that a LibreOffice save would drop: {}{more}. {WHAT_A_SAVE_DOES}", parts(found)),
            "cross_shape_formulas": found.cross_shape_formulas,
            "shape_data": found.shape_data,
        })
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use diacraft_doc::{Shape, ShapeList};

    #[test]
    fn a_formula_names_another_shape_by_sheet_dot_and_an_id_that_is_not_its_own() {
        for (formula, own, names) in [
            ("Sheet.2!Width", 1, true),
            ("sheet.2!Width*2", 1, true),
            ("SHEET.17!PinX+Sheet.1!PinX", 1, true),
            ("Sheet.1!Width", 1, false),
            ("Sheet.1!Width+Sheet.1!Height", 1, false),
            ("Width*0.5", 1, false),
            ("Sheet.!Width", 1, false),
            ("Sheet.x!Width", 1, false),
            ("Sheet.99999999999!Width", 1, false),
            ("Sheet.", 1, false),
            ("Sheet.2", 1, true),
            ("Sheet.2", 2, false),
            ("Sheets.2!Width", 1, false),
            ("", 1, false),
            ("Sheet.Sheet.3!X", 1, true),
        ] {
            assert_eq!(names_another_shape(formula, own), names, "{formula} on shape {own}");
        }
    }

    #[test]
    fn the_words_count_each_kind_and_say_one_and_many() {
        let found = Found { cross_shape_formulas: 1, shape_data: 2, ..Found::default() };
        assert_eq!(parts(&found), "1 shape with a formula that reads another shape, 2 shapes with data rows");
        let one = Found { shape_data: 1, ..Found::default() };
        assert_eq!(parts(&one), "1 shape with data rows");
        assert_eq!(note(&Found::default()), None, "nothing found, no note");
        let note = note(&found).unwrap();
        assert_eq!(note["class"], CLASS);
        assert!(
            note["words"].as_str().unwrap().contains("renumbers shape ids, glue points and style names, and recomputes connector routes"),
            "{note}"
        );
        assert_eq!((note["cross_shape_formulas"].as_u64(), note["shape_data"].as_u64()), (Some(1), Some(2)));
        assert!(note.get("master_links").is_none(), "a master link is no trigger: {note}");
        let stopped = Found { shape_data: 1, stopped: true, ..Found::default() };
        assert!(self::note(&stopped).unwrap()["words"].as_str().unwrap().contains("perhaps more"));
    }

    /// A session with `n` rectangles dropped on the first page: each an instance of the master the drop adds.
    fn session_with(n: usize) -> crate::Session {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for k in 0..n {
            s.execute("shape.drop", &json!({ "master": "rectangle", "page": 1, "x": 2.0 + k as f64, "y": 4.0 })).unwrap();
        }
        s
    }

    /// Gives the shape at `at` on the first page a cell, and a `User` row with a cell, each holding `formula`.
    fn give(s: &mut crate::Session, at: usize, section: &str, row: &str, formula: &str) {
        s.edit("Test edit", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            let cell = Cell::with_formula(formula, Some(diacraft_doc::Value::from_text("1")));
            let mut made = diacraft_doc::Section::new(section, None);
            let mut r = diacraft_doc::Row { name: Some(std::sync::Arc::from(row)), ..diacraft_doc::Row::default() };
            r.cells.insert(diacraft_doc::CellName::new("Value"), cell);
            made.rows.push(std::sync::Arc::new(r));
            sheet.sections.push(std::sync::Arc::new(made));
            Ok(())
        })
        .unwrap();
    }

    /// A session with `n` dropped rectangles, each given a `Property` row: `n` shapes with data.
    fn session_with_data(n: usize) -> crate::Session {
        let mut s = session_with(n);
        for k in 0..n {
            give(&mut s, k, "Property", "p", "1");
        }
        s
    }

    /// A shape that is an instance of a master is no trigger of the note (a dropped shape is one in every session), whatever number of them.
    #[test]
    fn a_shape_that_is_an_instance_of_a_master_is_no_trigger_and_nothing_is_found_in_plain_shapes() {
        let s = session_with(3);
        assert!(s.document().unwrap().pages.get(0).unwrap().shapes.iter().all(|x| x.master.is_some()), "each is an instance of a master");
        let found = find(s.document().unwrap());
        assert_eq!((found.cross_shape_formulas, found.shape_data), (0, 0), "{found:?}");
        assert!(!found.any() && !found.stopped, "{found:?}");
        assert!(note(&found).is_none());
        let none = find(session_with(0).document().unwrap());
        assert!(!none.any(), "{none:?}");
    }

    #[test]
    fn a_shape_is_counted_once_for_each_kind_however_many_cells_hold_it_and_the_route_marker_is_no_data() {
        let mut s = session_with(2);
        give(&mut s, 0, "User", "k", "Sheet.2!Width");
        give(&mut s, 0, "Property", "p", "Sheet.2!Height+Sheet.2!Width");
        give(&mut s, 1, "User", ROUTE_ROW, "Width*2");
        let found = find(s.document().unwrap());
        assert_eq!(
            (found.cross_shape_formulas, found.shape_data),
            (1, 1),
            "the first shape reads the second twice and holds two rows; the second only the marker: {found:?}"
        );
    }

    #[test]
    fn a_formula_that_names_the_shape_itself_is_no_cross_shape_formula() {
        let mut s = session_with(1);
        let id = s.document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
        give(&mut s, 0, "User", ROUTE_ROW, &format!("Sheet.{id}!Width"));
        assert_eq!(find(s.document().unwrap()).cross_shape_formulas, 0);
    }

    /// A session whose first shape holds, `levels` deep, a shape numbered 50 with a cell whose formula names shape 99: a chain of members each holding the next.
    fn nested(levels: usize) -> crate::Session {
        let mut s = session_with(1);
        s.edit("Test edit", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let top = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
            let mut inner = Shape::new(50);
            let cost = Cell::with_formula("Sheet.99!Width", Some(diacraft_doc::Value::from_text("1")));
            std::sync::Arc::make_mut(&mut inner.sheet).cells.insert(diacraft_doc::CellName::new("Cost"), cost);
            for level in 1..levels {
                let mut outer = Shape::new(u32::try_from(50 + level).unwrap());
                outer.children = ShapeList::from_vec(vec![std::sync::Arc::new(inner)]);
                inner = outer;
            }
            top.children = ShapeList::from_vec(vec![std::sync::Arc::new(inner)]);
            Ok(())
        })
        .unwrap();
        s
    }

    #[test]
    fn the_members_of_a_group_are_looked_at_to_the_depth_of_the_model_and_no_further() {
        let found = find(nested(1).document().unwrap());
        assert_eq!((found.cross_shape_formulas, found.shape_data), (1, 0), "a member's formula: {found:?}");
        // The walk goes down to the model's depth and stops there.
        let deepest = MAX_SHAPE_DEPTH - 1;
        assert_eq!(find(nested(deepest).document().unwrap()).cross_shape_formulas, 1, "at the deepest level the model has");
        assert_eq!(find(nested(deepest + 1).document().unwrap()).cross_shape_formulas, 0, "one level past it is not looked at, and the walk ends");
    }

    #[test]
    fn the_steps_follow_the_size_of_the_drawing_and_a_count_past_the_bound_stops_and_keeps_what_it_found() {
        let (small, big) = (find(session_with_data(10).document().unwrap()), find(session_with_data(40).document().unwrap()));
        assert!(
            small.steps > 10 && big.steps > small.steps * 3 && big.steps < small.steps * 5,
            "{} steps for 10 shapes, {} for 40",
            small.steps,
            big.steps
        );
        let s = session_with_data(10);
        let cut = find_within(s.document().unwrap(), small.steps / 2);
        assert!(cut.stopped && cut.steps > small.steps / 2 && cut.shape_data < 10, "{cut:?}");
        let exact = find_within(s.document().unwrap(), small.steps);
        assert!(!exact.stopped && exact.shape_data == 10, "the bound itself is allowed: {exact:?}");
    }

    #[test]
    fn a_step_is_taken_for_each_thing_looked_at_and_the_count_stops_at_the_bound() {
        let mut count = Count { found: Found::default(), most: 3 };
        assert!(count.take(1) && count.take(2), "three steps are the bound");
        assert!(!count.take(1), "the fourth is past it");
        assert!(count.found.stopped && count.found.steps == 4);
        assert!(!count.take(1), "and once stopped it stays stopped");
    }

    /// Each kind alone is enough for there to be a note, and a kind that is not there is not said (the survivors of the mutation run: a count of zero said as "0 shapes").
    #[test]
    fn each_kind_alone_makes_a_note_and_says_only_itself() {
        for (found, words) in [
            (Found { cross_shape_formulas: 1, ..Found::default() }, "1 shape with a formula that reads another shape"),
            (Found { shape_data: 2, ..Found::default() }, "2 shapes with data rows"),
        ] {
            assert!(found.any(), "{found:?}");
            assert_eq!(parts(&found), words);
            let said = note(&found).unwrap();
            assert!(said["words"].as_str().unwrap().contains(&format!("drop: {words}. ")), "{said}");
        }
        assert!(!Found::default().any());
    }

    /// A formula costs a step and one for each 64 bytes it holds: 63 bytes one step, 64 two, 640 eleven; a cell with no formula one.
    #[test]
    fn a_formula_costs_a_step_for_each_sixty_four_bytes_and_one_more() {
        let cost = |formula: &str| {
            let mut count = Count { found: Found::default(), most: u64::MAX };
            let cell = Cell::with_formula(formula, Some(diacraft_doc::Value::from_text("1")));
            let mut cross = false;
            assert!(count.cells(std::iter::once(&cell), Own { id: 1, name: None }, &mut cross));
            (count.found.steps, cross)
        };
        let of = |bytes: usize| format!("{}11", "1+".repeat(bytes / 2 - 1));
        assert_eq!(of(64).len(), 64);
        assert_eq!(cost(&of(62)), (1, false), "62 bytes");
        assert_eq!(cost(&of(64)), (2, false), "64 bytes");
        assert_eq!(cost(&of(126)), (2, false), "126 bytes");
        assert_eq!(cost(&of(128)), (3, false), "128 bytes");
        assert_eq!(cost(&of(640)), (11, false), "640 bytes");
        let constant = Cell::constant(diacraft_doc::Value::from_text("1"));
        let mut count = Count { found: Found::default(), most: u64::MAX };
        assert!(count.cells(std::iter::once(&constant), Own { id: 1, name: None }, &mut false));
        assert_eq!(count.found.steps, 1, "a cell with no formula");
    }

    /// A count that has no step left at the header of a section stops there: the shape it is in is not counted (a mutant went on to count it).
    #[test]
    fn a_count_that_runs_out_at_a_sections_header_stops_there_and_does_not_count_the_shape() {
        let mut s = session_with(1);
        s.edit("Test edit", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
            // A cell of its own that reads another shape, and one section with no row: the header of the section is the last step.
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            sheet.cells.insert(diacraft_doc::CellName::new("Cost"), Cell::with_formula("Sheet.99!Width", Some(diacraft_doc::Value::from_text("1"))));
            let mut empty = diacraft_doc::Section::new("Other", None);
            empty.rows.clear();
            sheet.sections.clear();
            sheet.sections.push(std::sync::Arc::new(empty));
            Ok(())
        })
        .unwrap();
        let all = find(s.document().unwrap());
        assert!(!all.stopped && all.cross_shape_formulas == 1, "{all:?}");
        let cut = find_within(s.document().unwrap(), all.steps - 1);
        assert!(cut.stopped, "{cut:?}");
        assert_eq!(cut.cross_shape_formulas, 0, "the shape in which the count ran out is not counted: {cut:?}");
    }

    /// What a parsed formula reads: `Sheet.<ID>`, a name (and the `ID` after it) in the first or any qualifier, in calls, operands and the arguments of a qualifier that is
    /// a call; what names no shape and what names the shape it is in are not another shape.
    #[test]
    fn a_parsed_formula_reads_another_shape_by_id_by_name_and_through_any_operand_or_call() {
        let reads = |formula: &str, own: u32, name: Option<&str>| {
            let parsed = diacraft_formula::parse(formula);
            let expr = parsed.ast.as_ref().unwrap_or_else(|e| panic!("{formula} does not parse: {e:?}"));
            reads_another_shape(expr, Own { id: own, name })
        };
        for (formula, own, name, another) in [
            ("Sheet.2!Width", 1, Some("A"), true),
            ("Sheet.1!Width", 1, Some("A"), false),
            ("sheet.1!Width", 1, None, false),
            ("B!Width", 1, Some("A"), true),
            ("b!Width", 1, Some("B"), false),
            ("B!Width", 1, None, true),
            ("B.2!Width", 2, Some("B"), false),
            ("B.2!Width", 3, Some("B"), true),
            ("B.2!Width", 2, Some("A"), true),
            ("A.1!Width", 1, Some("A"), false),
            ("ThePage!PageWidth", 1, Some("A"), false),
            ("TheDoc!Width", 1, Some("A"), false),
            ("Styles!Normal!LineColor", 1, Some("A"), false),
            ("Pages[1]!A!Width", 1, Some("A"), true),
            ("Masters[1]!Sheet.1!Width", 1, Some("A"), true),
            ("LISTSHEETREF(2)!Width", 1, Some("A"), true),
            ("Width*2", 1, Some("A"), false),
            ("Prop.Cost", 1, Some("A"), false),
            ("1+B!Width", 1, Some("A"), true),
            ("-B!Width", 1, Some("A"), true),
            ("B!Width%", 1, Some("A"), true),
            ("IF(1,2,B!Width)", 1, Some("A"), true),
            ("MAX(1,MIN(2,Sheet.9!X))", 1, Some("A"), true),
            ("MAX(1,MIN(2,Width))", 1, Some("A"), false),
            ("LISTSHEETREF(Sheet.9!X)!Width", 1, Some("A"), true),
            // A qualifier with an index that is no `Pages` or `Masters`, and a name after `Styles`, name no shape (and set nothing for the qualifier that follows).
            ("Foo[1]!B!Width", 1, Some("B"), false),
            ("Styles!Normal.3!LineColor", 1, Some("A"), false),
            ("Foo[1].5!Width", 1, Some("A"), false),
            ("Foo.5[2]!Width", 1, Some("A"), false),
            ("Styles!Normal!LineColor", 1, Some("A"), false),
        ] {
            assert_eq!(reads(formula, own, name), another, "{formula} in shape {own} {name:?}");
        }
        // A formula that does not parse is scanned as text for `Sheet.<ID>`, as before.
        let broken = Cell::with_formula("Sheet.9!(", Some(diacraft_doc::Value::from_text("1")));
        let mut cross = false;
        let mut count = Count { found: Found::default(), most: u64::MAX };
        assert!(count.cells(std::iter::once(&broken), Own { id: 1, name: None }, &mut cross));
        assert!(cross, "an unparsed formula is scanned as text");
        // The deepest tree the parser builds is walked without recursion.
        let deep = format!("{}B!Width{}", "(".repeat(150), ")".repeat(150));
        let parsed = diacraft_formula::parse(&deep);
        if let Ok(expr) = &parsed.ast {
            assert!(reads_another_shape(expr, Own { id: 1, name: Some("A") }));
        }
    }
}
