//! A new picture loses nothing: a prediction of what the reader rebuilds from a
//! picture frame that is too kind would pass the check of the bytes made (it reads back only what tier 2 carries) and lose the cell silently. So every cell and section the
//! model of a new picture holds is read back from the file, whichever of them the frame says itself and whichever tier 2 carries.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Shape};
use diacraft_odg::{Limits, read};
use proptest::prelude::*;

use crate::flat_write::small_png;
use crate::write_new::push_shape;
use crate::write_picture::{named, picture, save, three};

/// A cell the pool offers: name, formula, value and unit.
type Offer = (&'static str, Option<&'static str>, &'static str, Option<&'static str>);

/// What a new picture may hold beside its frame: cells the frame's style says, cells it cannot say, and forms of both that no attribute holds.
const POOL: [Offer; 32] = [
    ("ImgOffsetX", None, "0", Some("IN")),
    ("ImgWidth", Some("Width*1"), "1.5", Some("IN")),
    ("Brightness", None, "0.5", None),
    ("FillPattern", None, "1", None),
    ("FillPattern", None, "2", None),
    ("FillForegnd", None, "#aabbcc", None),
    ("FillForegnd", None, "#AABBCC", None),
    ("FillForegnd", Some("THEMEVAL(\"FillColor\")"), "#112233", None),
    ("LineColor", None, "#112233", None),
    ("LinePattern", None, "1", None),
    ("LinePattern", None, "3", None),
    ("LineWeight", None, "0.02", Some("IN")),
    ("LineWeight", Some("THEMEVAL()"), "0.02", Some("IN")),
    ("TopMargin", None, "0.1", Some("IN")),
    ("LeftMargin", None, "0.2", Some("IN")),
    ("BeginArrow", None, "4", None),
    ("EndArrow", None, "254", None),
    ("FillGradientEnabled", None, "0", None),
    ("FillGradientEnabled", None, "1", None),
    ("Comment", None, "a note", None),
    ("LayerMember", None, "0", None),
    ("ShdwPattern", None, "1", None),
    ("BeginArrowSize", None, "4", None),
    ("BeginArrowSize", None, "0", None),
    ("EndArrowSize", None, "3", None),
    ("EndArrowSize", None, "0", None),
    ("BeginArrow", None, "0", None),
    ("EndArrow", None, "4", None),
    ("BeginArrow", None, "1", None),
    ("Angle", None, "0.5", Some("DEG")),
    ("Angle", None, "4.5", Some("DEG")),
    ("FlipX", None, "1", None),
];

fn make_cell(offer: &Offer) -> Cell {
    Cell::read(offer.1, Some(offer.2), offer.3, None, &mut 1_000)
}

/// The sections a case adds: the outline of a rectangle, a user row, and the connection rows (none, the four side middles the reader gives, or five).
fn sections(outline: bool, user: bool, connection: u8) -> Vec<Section> {
    let mut out = Vec::new();
    let row = |kind: &str, ix: u32, cells: &[(&str, Option<&str>, &str)]| {
        let mut r = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
        for (name, formula, value) in cells {
            r.cells.insert(CellName::new(name), Cell::read(*formula, Some(value), Some("IN"), None, &mut 1_000));
        }
        Arc::new(r)
    };
    if outline {
        let mut g = Section::new("Geometry", Some(0));
        g.rows.push(row("MoveTo", 1, &[("X", None, "0"), ("Y", None, "0")]));
        g.rows.push(row("LineTo", 2, &[("X", Some("Width*1"), "1"), ("Y", None, "0")]));
        out.push(g);
    }
    if user {
        let mut u = Section::new("User", None);
        u.rows.push(Arc::new(Row {
            name: Some(Arc::from("Mark")),
            cells: [(CellName::new("Value"), Cell::read(None, Some("5"), None, None, &mut 1_000))].into_iter().collect(),
            ..Row::default()
        }));
        out.push(u);
    }
    let sides: [(&str, &str, &str, &str); 5] = [
        ("Width*0.5", "1", "Height", "2"),
        ("", "0", "Height*0.5", "1"),
        ("Width*0.5", "1", "", "0"),
        ("Width", "2", "Height*0.5", "1"),
        ("Width*0.25", "0.5", "Height*0.25", "0.5"),
    ];
    let wanted = match connection {
        0 => 0,
        1 => 4,
        _ => 5,
    };
    if wanted > 0 {
        let mut c = Section::new("Connection", None);
        for (i, (xf, xv, yf, yv)) in sides.iter().take(wanted).enumerate() {
            let f = |s: &'static str| (!s.is_empty()).then_some(s);
            c.rows.push(row("", i as u32, &[("X", f(xf), xv), ("Y", f(yf), yv)]));
        }
        out.push(c);
    }
    out
}

/// The text a cell shows (its number or its text), to compare what was written with what is read back.
fn shows(c: &Cell) -> (Option<String>, String) {
    let v = c.value.as_ref();
    // A colour is the same colour in either case (the reader gives `#RRGGBB`; a new shape's colour that the model holds in lower case is a style attribute and reads back in
    // capitals, for every kind of new shape: the rule, `ext::native::style_reproduced`).
    let text = |v: &diacraft_doc::Value| if v.text().starts_with('#') { v.text().to_ascii_uppercase() } else { v.text().to_owned() };
    (c.formula.text().map(str::to_owned), v.map(|v| v.number().map_or_else(|| text(v), |n| format!("{n:.6}"))).unwrap_or_default())
}

/// The model's cells and sections are all read back from the file, each as it was.
fn assert_nothing_lost(model: &Shape, back: &Shape, what: &str) -> Result<(), TestCaseError> {
    for (name, cell) in &model.sheet.cells {
        // `FillGradientEnabled` is 0 where it is not given (the rule, `ext::native::tier2`): the constant 0 is the cell's default and need not be in the file.
        let default_zero = name.as_str() == "FillGradientEnabled"
            && cell.value.as_ref().and_then(diacraft_doc::Value::number) == Some(0.0)
            && cell.formula.text().is_none();
        if default_zero && back.sheet.cell(name.as_str()).is_none() {
            continue;
        }
        let Some(got) = back.sheet.cell(name.as_str()) else { return Err(TestCaseError::fail(format!("{what}: the cell {name} is lost"))) };
        // The pin and size are written as lengths and read back with the rounding of the shortest decimal; everything else is as it was.
        prop_assert_eq!(shows(cell).0, shows(got).0, "{}: the formula of {}", what, name);
        let (a, b) = (cell.value.as_ref().and_then(diacraft_doc::Value::number), got.value.as_ref().and_then(diacraft_doc::Value::number));
        match (a, b) {
            (Some(x), Some(y)) => prop_assert!((x - y).abs() < 1e-6, "{}: {} was {} and is {}", what, name, x, y),
            _ => prop_assert_eq!(shows(cell).1, shows(got).1, "{}: the value of {}", what, name),
        }
    }
    for section in model.sheet.sections.iter() {
        let Some(got) = back.sheet.sections.iter().find(|s| s.name == section.name && s.ix == section.ix) else {
            return Err(TestCaseError::fail(format!("{what}: the section {} is lost", section.name.as_str())));
        };
        prop_assert_eq!(section.rows.len(), got.rows.len(), "{}: the rows of {}", what, section.name.as_str());
    }
    // And no section the model did not have: a frame is given four connection rows, which a model of none or five takes away.
    for section in back.sheet.sections.iter() {
        prop_assert!(
            model.sheet.sections.iter().any(|s| s.name == section.name),
            "{}: the section {} was not the model's",
            what,
            section.name.as_str()
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        // The size of a longer run is set by `DIACRAFT_PICTURE_TIER2_CASES` (an explicit `cases` beats `PROPTEST_CASES`).
        cases: std::env::var("DIACRAFT_PICTURE_TIER2_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(400),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Whatever a new picture holds, nothing is lost: each cell and each section of its model reads back from the file as it was, and the file's second save is no edit.
    #[test]
    fn nothing_a_new_picture_holds_is_lost_in_the_file(picks in proptest::collection::vec(0usize..POOL.len(), 0..9), outline in any::<bool>(), user in any::<bool>(), connection in 0u8..3) {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        let mut shape = picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png());
        {
            let sheet = Arc::make_mut(&mut shape.sheet);
            // The picture's own style cells and connection rows are replaced by the case's.
            for name in ["FillPattern", "FillForegnd", "LinePattern"] {
                sheet.cells.remove(name);
            }
            sheet.sections.clear();
            for pick in &picks {
                let offer = &POOL[*pick];
                sheet.cells.insert(CellName::new(offer.0), make_cell(offer));
            }
            for section in sections(outline, user, connection) {
                sheet.sections.push(Arc::new(section));
            }
        }
        push_shape(&mut o.doc, shape);
        let what = format!("{picks:?} outline {outline} user {user} connection {connection}");
        let saved = match save(&o.doc, &o.bytes, &o.index) {
            Ok(saved) => saved,
            // What cannot be written is refused in words (a margin that is no length, a line end the table has no art for), never dropped.
            Err(e) => { prop_assert_eq!(e.class(), "write_unsupported", "{}: {}", what, e); return Ok(()); }
        };
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        assert_nothing_lost(named(&o.doc, "Picture.1"), named(&back, "Picture.1"), &what)?;
        let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
        prop_assert!(!again.edited, "{}: a second save is an edit", what);
    }
}
