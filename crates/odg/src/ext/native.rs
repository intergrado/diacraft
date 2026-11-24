//! What the reader rebuilds from the native markup of a shape the writer writes, and so what of the shape is tier 2 ("One test decides
//! tier 2 everywhere").
//!
//! **The rebuild is predicted, then compared.** The writer writes a new shape's place, its style cells, its outline, its four side-middle connection rows and
//! its plain text natively, and the reader gives them back by the rules. [`rebuilt`] builds the sheet those rules give, **by
//! taking each cell and section the rebuild reproduces from the model itself** (a cell the model holds as the rebuild would give it is the model's own
//! cell) **and putting the reader's form in place of each it does not** (a different formula, a value that is not the one written, a section that is not
//! the one rebuilt). The delta of the model against that sheet ([`diacraft_sheetxml::sheet_delta`]) is then exactly the cells and sections the rebuild
//! would not give, and the cells the rebuild would give that the model lacks.
//!
//! **What counts as reproduced.** A formula is reproduced by the same formula text, and a constant by a number within the writer's tolerance of the one
//! written (a length goes into the file as the shortest decimal that reads back, and comes out as the frame's centre and size, so `PinX` is the centre,
//! within a rounding), whatever its unit (a unit says only how a value shows). A cell the rebuild leaves out is reproduced when the model
//! holds it as the constant 0 where 0 is its default (`Angle`, `FlipX`, `FlipY`). Nothing is reproduced that the rebuild does not give: a cell of the
//! model that no native markup says (a user cell, a layer, a master's data, a section the writer does not write) is tier 2. **The check of the bytes made
//! reads the file back and compares, so a rebuild predicted too kindly is a refused save and never a silent loss** (the writer's `check_tier2`).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Cells, FormulaSlot, Section, SectionKey, Shape, ShapeType, Sheet, Text, TextPart};
use diacraft_sheetxml::sheet_delta;

use super::names::Gone;
use super::payload::Tier2;

/// How the shape's frame is written: a 2-D shape's by its centre and size, a segment's by its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Frame {
    /// The centre of the frame on the page (inches, y up), its size, and the turn (radians, counterclockwise: the model's `Angle`, which the file's `rotate` holds, or minus it
    /// beside one mirror, `read::turn_beside_mirrors`) and the mirrors (`draw:mirror-horizontal`, `draw:mirror-vertical`) the writer wrote: the reader gives them back as `Angle`,
    /// `FlipX` and `FlipY`.
    TwoD { cx: f64, cy: f64, w: f64, h: f64, angle: f64, flip: (bool, bool) },
    /// A line or a connector: the ends are the cells' own.
    OneD,
}

/// **What the display forms of a style do beside the cells written natively**: a visible shadow, a gradient or a hatch is written for the programs that draw it, and the cells that go
/// with it ride in DiaCraft's data, so the reader (which makes a stand-in for each from the markup) gives back more than the model's cells say, and gives some of the model's fingerprinted
/// cells another value than the model has. The writer says both, so that the rebuild it predicts is the reader's.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Display {
    /// The cells the reader makes from the markup written whatever the model holds (a visible shadow gives `ShapeShdwType` 1, a gradient `FillGradientDir`): rebuilt as a cell the model's own
    /// is carried against, so that a model that lacks one has it taken out again, in tier 2.
    pub adds: Vec<&'static str>,
    /// The sections the reader makes the same way (the stops of a gradient: `FillGradient`).
    pub adds_sections: Vec<&'static str>,
    /// The fingerprinted cells whose native output the markup says another value for than the model's cell does (a hatch's `FillPattern` reads back as the solid stand-in 1, its `FillForegnd` as
    /// the ground): the cell the reader makes, or none.
    pub native: Vec<(&'static str, Option<Cell>)>,
    /// The fingerprinted cells that are **not judged** (no fingerprint is made for them): a cell whose native output is no more than a value the style chain may give as well (a hatch's
    /// `FillPattern` reads as the solid 1, which LibreOffice's parent style `standard` gives too, and the reader then leaves no local cell: a plain save of the file would call the carried
    /// pattern stale and drop it).
    pub unjudged: Vec<&'static str>,
}

/// What the writer wrote natively for a new shape.
#[derive(Clone, Debug)]
pub(crate) struct Native<'a> {
    pub frame: Frame,
    /// The style cells written natively, by name: each of them reads back with the value written.
    pub style: &'a [&'static str],
    /// The `Geometry` sections are written as the outline the reader reads back as the same rows.
    pub geometry: bool,
    /// The shape is a picture frame: the reader gives it no `Geometry` section at all, so `geometry` is not asked and every
    /// `Geometry` section the model holds is tier 2.
    pub picture: bool,
    /// The `Connection` section is the four side middles the reader rebuilds (or none, where the reader adds none).
    pub connection: bool,
    /// The shape is a connector: the reader adds the `User` row `DiaCraftRoute` and the route kind, and rebuilds the glue formulas of its ends.
    pub connector: bool,
    /// For a connector, whether each end is glued: the cells of a glued end and its trigger are rebuilt from the `Connect` row, whatever IDs the model had.
    pub glued: [bool; 2],
    /// The plain characters of the text, one paragraph after another between U+000A.
    pub text: &'a str,
    /// **The file says all of the text**: the paragraphs, runs, lists, headings and fields written natively are read back as the model's text and its `Paragraph`, `Character` and
    /// `Field` rows (each row confirmed by a held stamp, no carriage return, no tab stop marker, no field the file cannot hold). The text and those three sections are then reproduced, and tier 2
    /// carries none of them. False for a text that was written as plain characters, as it always was: the text and its rows are tier 2.
    pub text_native: bool,
    /// The fingerprint of the text's formatting as the markup written says it (`fingerprint::format_fp`): for a text written as plain characters it is the plain text's
    /// (`fingerprint::plain_format_fp`), and for the model writer's text it is what a read of the file made gives (`WriteWork::formats`). `None` when it is not known: the item is then judged by
    /// its characters alone.
    pub format: Option<super::fingerprint::Fp>,
    /// The bytes of the picture the frame written shows, which the shape's foreign data sits beside (`fingerprint::picture_fp`).
    pub picture_bytes: Option<&'a [u8]>,
    /// The kind of shape the reader gives the element written, which a shell carries the shape's kind against.
    pub expected: ShapeType,
    /// The `draw:name` written, which the reader gives both `Name` and `NameU`.
    pub name: Option<&'a str>,
    /// The row of the page's `Layer` section that the `draw:layer` written reads back as (the reader gives the shape that `LayerMember`); `None` when it reads back as
    /// none (a shape written on `layout`, or on one of LibreOffice's layers).
    pub layer: Option<u32>,
    /// The fingerprint of the outline written (`fingerprint::outline_fp`), for the tier-2 items that rest on it; `None` for a shape whose element has no
    /// outline to fingerprint (a line, a connector).
    pub outline: Option<super::fingerprint::Fp>,
    /// What the display forms written beside the cells do; `None` for a shape that has none.
    pub display: Option<&'a Display>,
}

/// Whether a cell holds no formula and a number within the writer's tolerance of `want`.
fn constant_near(cell: Option<&Cell>, want: f64) -> bool {
    let Some(cell) = cell else { return false };
    matches!(cell.formula, FormulaSlot::Absent)
        && cell.extra.is_none()
        && cell.raw.is_empty()
        && cell.value.as_ref().and_then(diacraft_doc::Value::number).is_some_and(|n| crate::write::lengths::same(n, want))
}

/// Whether a cell holds a number and no formula and nothing else of its own: what an end of a segment is (the reader reads each from its attribute).
fn plain_number(cell: Option<&Cell>) -> bool {
    cell.is_some_and(|c| {
        matches!(c.formula, FormulaSlot::Absent)
            && c.extra.is_none()
            && c.raw.is_empty()
            && c.value.as_ref().and_then(diacraft_doc::Value::number).is_some()
    })
}

/// Whether the style cell `name`, as the model holds it, is one the writer's attributes give back with the value the model has (the colour of a
/// fill or a line is written as `#rrggbb` in lower case, and a colour in capitals reads back in lower case, which is another text: it is tier 2).
pub(crate) fn style_reproduced(name: &str, cell: &Cell) -> bool {
    style_reproduced_by(name, cell, false)
}

/// [`style_reproduced`] for a shape an `.odg` reader made: **the reader makes every colour `#RRGGBB` in capitals**, whatever case the file wrote it in, so a colour
/// the model holds in capitals reads back as the model has it (and one in lower case does not).
pub(crate) fn style_reproduced_read(name: &str, cell: &Cell) -> bool {
    style_reproduced_by(name, cell, true)
}

fn style_reproduced_by(name: &str, cell: &Cell, read: bool) -> bool {
    if !matches!(cell.formula, FormulaSlot::Absent) || cell.extra.is_some() || !cell.raw.is_empty() {
        return false;
    }
    let Some(value) = cell.value.as_ref() else { return false };
    match name {
        "FillForegnd" | "LineColor" | "FillBkgnd" | "ShdwForegnd" => {
            let t = value.text();
            let digit = |b: u8| b.is_ascii_digit() || if read { (b'A'..=b'F').contains(&b) } else { (b'a'..=b'f').contains(&b) };
            t.len() == 7 && t.starts_with('#') && t.bytes().skip(1).all(digit)
        }
        _ => true,
    }
}

/// Whether a cell holds exactly the formula `text` of its own.
fn formula_is(cell: Option<&Cell>, text: &str) -> bool {
    cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_)) && c.formula.text() == Some(text) && c.extra.is_none() && c.raw.is_empty())
}

/// A cell that is only here to be unequal to any the model holds: what stands for a cell the reader rebuilds in a form the model does not have.
fn unlike(name: &str) -> Cell {
    let mut cell = Cell::new();
    cell.raw.push("DiaCraftRebuild", name);
    cell
}

/// A section that is only here to be unequal to any the model holds.
fn unlike_section(key: &SectionKey) -> Section {
    let mut s = Section::new(key.name.as_str(), key.ix);
    s.raw.push("DiaCraftRebuild", "1");
    s
}

/// Whether a turn or flip cell the model holds is one the reader would also be given by the markup the writer wrote (`written`), or one the style carries that says
/// another (a carried turn that is the markup's within LibreOffice's grid is no reason for the reader to leave the pin alone).
fn turn_agrees(m: &Sheet, angle: f64, flip: (bool, bool)) -> bool {
    [("Angle", angle), ("FlipX", f64::from(u8::from(flip.0))), ("FlipY", f64::from(u8::from(flip.1)))].into_iter().all(|(name, written)| {
        let Some(c) = m.cells.get(name) else { return true };
        // The cell is carried unless the rebuild gives it as it is (the number written, or the constant 0 where nothing is written).
        let rebuilt_as_is = constant_near(Some(c), written);
        rebuilt_as_is || super::tier3::turn_close(c.value.as_ref().and_then(diacraft_doc::Value::number), written)
    })
}

/// The pin the reader gives a two-dimensional shape the writer wrote with this frame (`centre`, `size`, the `angle` and `flip` of `draw:transform`), whose model is
/// `m`: **the markup's edge plus the place the shape carries**, by the map of the turn and the mirrors the markup has (`tier3::derive_pins` and `derive_turned`:
/// the writer predicts the reader with the same functions), when the model carries a pin's place that is not the default and no turn that is not the markup's; the
/// centre otherwise. A pin that is this is not carried.
pub(crate) fn expected_pin(m: &Sheet, centre: (f64, f64), size: (f64, f64), angle: f64, flip: (bool, bool)) -> (f64, f64) {
    let (w, h) = size;
    let place = |name: &str, size_name: &str, extent: f64| -> Option<f64> {
        let c = m.cells.get(name)?;
        if formula_is(Some(c), &format!("{size_name}*0.5")) {
            return None;
        }
        super::tier3::place_value(c, Some(extent), size_name)
    };
    let (lx, ly) = (place("LocPinX", "Width", w), place("LocPinY", "Height", h));
    if (lx.is_none() && ly.is_none()) || !turn_agrees(m, angle, flip) {
        return centre;
    }
    // The vector from the pin's place to the centre of the frame, in the shape's own axes, as the page sees it.
    let (ox, oy) = super::tier3::map_of(angle, flip)(w / 2.0 - lx.unwrap_or(w / 2.0), h / 2.0 - ly.unwrap_or(h / 2.0));
    (centre.0 - ox, centre.1 - oy)
}

/// The cells the reader gives a two-dimensional shape written with this frame, as numbers: the pin is the frame's centre, its place half the size, and the turn and the
/// flips the markup says (`None` for a line, whose frame is its ends).
fn frame_sheet(frame: Frame) -> Option<Sheet> {
    let Frame::TwoD { cx, cy, w, h, angle, flip } = frame else { return None };
    let mut sheet = Sheet::default();
    let mut put = |name: &str, v: f64| {
        if let Some(v) = diacraft_doc::Value::from_number(v) {
            sheet.cells.insert(CellName::new(name), Cell::constant(v));
        }
    };
    for (name, v) in [("PinX", cx), ("PinY", cy), ("LocPinX", w / 2.0), ("LocPinY", h / 2.0), ("Width", w), ("Height", h)] {
        put(name, v);
    }
    for (name, v) in [("Angle", angle), ("FlipX", f64::from(u8::from(flip.0))), ("FlipY", f64::from(u8::from(flip.1)))] {
        if v != 0.0 {
            put(name, v);
        }
    }
    Some(sheet)
}

/// Whether the reader would take out, or give another value to, a pin cell the tier 2 of a new shape carries: the pin is carried when it is not what the frame and the
/// place of the pin give (`expected_pin`), and **the reader keeps a carried pin only where it puts the frame's edge where the markup has it** (`tier3::derive_pins`).
/// A model whose cells agree has no such pin; a model whose cells do not (a width set, and the place of the pin, which is half of it, not calculated again) has one,
/// and the file would read back with the frame's centre as the pin, which is not the model's. The writer refuses it in words, and not after the check of the bytes.
pub(crate) fn carried_pin_the_reader_drops(n: &Native<'_>, t: &Tier2) -> bool {
    if !t.cells.contains_key("PinX") && !t.cells.contains_key("PinY") {
        return false;
    }
    let Some(native) = frame_sheet(n.frame) else { return false };
    let mut read = t.clone();
    super::tier3::derive_pins(&mut read, &native);
    ["PinX", "PinY"].iter().any(|name| t.cells.get(*name).is_some_and(|carried| read.cells.get(*name) != Some(carried)))
}

/// Puts into the rebuilt sheet the cells and sections the reader makes from the display forms written.
fn put_display(p: &mut Sheet, display: &Display) {
    for name in &display.adds {
        if !p.cells.contains_key(*name) {
            p.cells.insert(CellName::new(name), unlike(name));
        }
    }
    for name in &display.adds_sections {
        p.sections.push(Arc::new(unlike_section(&SectionKey::new(name, None))));
    }
}

/// The sheet the reader rebuilds from what was written natively. See the module documentation.
pub(crate) fn rebuilt(shape: &Shape, n: &Native<'_>) -> Sheet {
    let m: &Sheet = &shape.sheet;
    let mut p = Sheet { links: m.links, ..Sheet::default() };
    let keep = |p: &mut Sheet, name: &str, ok: bool| {
        let name = CellName::new(name);
        match m.cells.get(&name) {
            Some(c) if ok => {
                p.cells.insert(name, c.clone());
            }
            // The reader rebuilds the cell and the model's is not what it gives: the reader's form is another cell, so that the model's is written.
            _ => {
                let label = name.as_str().to_owned();
                p.cells.insert(name, unlike(&label));
            }
        }
    };
    match n.frame {
        Frame::TwoD { cx, cy, w, h, angle, flip } => {
            let pin = |name: &str| m.cells.get(name);
            keep(&mut p, "Width", constant_near(pin("Width"), w));
            keep(&mut p, "Height", constant_near(pin("Height"), h));
            keep(&mut p, "LocPinX", formula_is(pin("LocPinX"), "Width*0.5"));
            keep(&mut p, "LocPinY", formula_is(pin("LocPinY"), "Height*0.5"));
            // The pin is what the reader gives: the markup's edge plus the place the shape carries, by the map of the turn and the mirrors the markup has, when the
            // reader derives it (`tier3::derive_pins`, `tier3::derive_turned`), and the centre otherwise; a pin that is what the reader gives is not carried.
            let (px, py) = expected_pin(m, (cx, cy), (w, h), angle, flip);
            keep(&mut p, "PinX", constant_near(pin("PinX"), px));
            keep(&mut p, "PinY", constant_near(pin("PinY"), py));
            // `Angle`, `FlipX` and `FlipY` are written only when they are not 0: the constant 0 is the default the reader gives a cell
            // it leaves out. A shape the writer wrote turned (`draw:transform`) or mirrored has the cell the reader gives it: the turn, and 1. A cell that
            // is not the number written is the model's alone (a turn of more than a half turn is written as the turn it is, the other way round).
            let written = [("Angle", angle), ("FlipX", f64::from(u8::from(flip.0))), ("FlipY", f64::from(u8::from(flip.1)))];
            for (name, value) in written {
                if value != 0.0 {
                    keep(&mut p, name, constant_near(m.cells.get(name), value));
                    continue;
                }
                // Nothing written: a cell that is the constant 0 is the default the reader gives. Any other cell is not in the rebuild, so it is the model's alone and
                // is carried (the reader would have none).
                if let Some(c) = m.cells.get(name)
                    && constant_near(Some(c), 0.0)
                {
                    p.cells.insert(CellName::new(name), c.clone());
                }
            }
        }
        Frame::OneD => {
            let pin = |name: &str| m.cells.get(name);
            let end = |name: &str, glued: bool| glued || plain_number(pin(name));
            keep(&mut p, "BeginX", end("BeginX", n.glued[0]));
            keep(&mut p, "BeginY", end("BeginY", n.glued[0]));
            keep(&mut p, "EndX", end("EndX", n.glued[1]));
            keep(&mut p, "EndY", end("EndY", n.glued[1]));
            keep(&mut p, "Width", formula_is(pin("Width"), "SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"));
            keep(&mut p, "Height", constant_near(pin("Height"), 0.0));
            keep(&mut p, "Angle", formula_is(pin("Angle"), "ATAN2(EndY-BeginY,EndX-BeginX)"));
            keep(&mut p, "PinX", formula_is(pin("PinX"), "(BeginX+EndX)/2"));
            keep(&mut p, "PinY", formula_is(pin("PinY"), "(BeginY+EndY)/2"));
            keep(&mut p, "LocPinX", formula_is(pin("LocPinX"), "Width*0.5"));
            keep(&mut p, "LocPinY", formula_is(pin("LocPinY"), "Height*0.5"));
        }
    }
    // The style cells written natively: each reads back as written, and **one with a formula as the constant it was worked out to** (its attribute says the value, so its
    // formula is carried beside it).
    for name in n.style {
        if let Some(c) = m.cells.get(*name) {
            let read = if matches!(c.formula, FormulaSlot::Local(_)) {
                Cell { value: c.value.clone(), unit: c.unit.clone(), ..Cell::default() }
            } else {
                c.clone()
            };
            p.cells.insert(CellName::new(name), read);
        }
    }
    // The cells and sections the reader makes from the display forms written: where the model holds the cell as the reader gives it, it is the cell above; otherwise the reader's cell is
    // another than the model's, and the model's is carried against it (or taken out, when the model has none).
    if let Some(display) = n.display {
        put_display(&mut p, display);
    }
    if n.connector {
        // The route kind and the triggers of a glued end are the reader's, from the file's own `draw:type` and `Connect` rows.
        for name in ["ShapeRouteStyle", "ConLineRouteExt"] {
            if let Some(c) = m.cells.get(name) {
                p.cells.insert(CellName::new(name), c.clone());
            }
        }
        for (name, glued) in [("BegTrigger", n.glued[0]), ("EndTrigger", n.glued[1])] {
            if glued && let Some(c) = m.cells.get(name) {
                p.cells.insert(CellName::new(name), c.clone());
            }
        }
    }
    // The layer: what the reader makes of the `draw:layer` written (a row of the page's layers, as text), which the model's `LayerMember` is when it says the same.
    if let Some(row) = n.layer {
        let reads = Cell::constant(diacraft_doc::Value::from_text(&row.to_string()));
        p.cells.insert(CellName::new("LayerMember"), reads);
    }
    // The outline: the same sections, or a section that is not the one written. A picture frame has none: nothing is rebuilt.
    let geometry_key = |ix: u32| SectionKey::new("Geometry", Some(ix));
    if n.picture {
    } else if n.geometry {
        for s in m.sections.iter().filter(|s| s.name.as_str() == "Geometry") {
            // A segment's outline reads back as rows through its points and no flag of the section's own: one that holds a flag
            // (`NoFill` 1, as `connector.add` writes) is not what the reader gives, and is carried.
            if matches!(n.frame, Frame::OneD) && !s.cells.is_empty() {
                let mut rows = Section::clone(s);
                rows.cells = diacraft_doc::Cells::new();
                p.sections.push(Arc::new(rows));
            } else {
                p.sections.push(Arc::clone(s));
            }
        }
    } else {
        // The reader rebuilds section 0 of the outline that was written in its place (a placeholder rectangle).
        p.sections.push(Arc::new(unlike_section(&geometry_key(0))));
    }
    // A text the file says whole: its rows are what the reader gives back (modulo the `IX`es it numbers anew). A size of 0 is written as the least the file takes, which the
    // reader gives back: the `Character` rows ride in tier 2, with the 0.
    if n.text_native {
        for s in m.sections.iter().filter(|s| matches!(s.name.as_str(), "Paragraph" | "Character" | "Field")) {
            if crate::write::runs::zero_size_in(s) {
                p.sections.push(Arc::new(unlike_section(&SectionKey::new(s.name.as_str(), s.ix))));
            } else {
                p.sections.push(Arc::clone(s));
            }
        }
    }
    // The reader gives every shape it reads the four standard rows, a line and a connector included: a model that holds no `Connection` section has it
    // gone, whatever its frame (a segment with none came back with four, and the check after the save named them).
    let connection_key = SectionKey::new("Connection", None);
    match m.sections.iter().find(|s| s.name.as_str() == "Connection") {
        Some(s) if n.connection => p.sections.push(Arc::clone(s)),
        _ => p.sections.push(Arc::new(unlike_section(&connection_key))),
    }
    if n.connector {
        // The `DiaCraftRoute` row marks a route the router may route again; a connector that lacks it, or holds other user rows, is tier 2.
        let user_key = SectionKey::new("User", None);
        match m.sections.iter().find(|s| s.name.as_str() == "User") {
            Some(s) if s.rows.iter().all(|r| r.name.as_deref() == Some("DiaCraftRoute")) && s.cells.is_empty() => p.sections.push(Arc::clone(s)),
            _ => p.sections.push(Arc::new(unlike_section(&user_key))),
        }
    }
    p
}

/// The plain text the reader rebuilds from the characters written: one run of characters, and no text at all for none.
fn rebuilt_text(plain: &str) -> Option<Text> {
    (!plain.is_empty()).then(|| Text { parts: vec![TextPart::Chars(Arc::from(plain))], ..Default::default() })
}

/// A shape's tier 2: what of it the rebuild from `n` does not give. `referenced`: another shape's formula or glue names this shape, so that it keeps its
/// `ID`.
pub(crate) fn tier2(shape: &Shape, n: &Native<'_>, referenced: bool) -> Tier2 {
    let p = rebuilt(shape, n);
    let delta = sheet_delta(&shape.sheet, &p);
    let mut t = Tier2::default();
    // `FillGradientEnabled` is 0 by default, and the reader gives the cell as 0 to a solid fill: a model that holds none, or the constant 0, is reproduced
    // (the same value, resolved).
    let default_zero = |name: &str, cell: Option<&Cell>| name == "FillGradientEnabled" && cell.is_none_or(|c| constant_near(Some(c), 0.0));
    for (name, cell) in delta.cells.changed {
        if !default_zero(name.as_str(), Some(cell)) {
            t.cells.insert(name.clone(), cell.clone());
        }
    }
    let mut gone: Vec<Gone> =
        delta.cells.removed.iter().filter(|name| !default_zero(name.as_str(), None)).map(|name| Gone::Cell(name.as_str().to_owned())).collect();
    // A placeholder cell the reader would rebuild and the model lacks is "gone"; one the model has is carried (above).
    for section in delta.sections {
        t.sections.push(Arc::new(section.section.clone()));
    }
    gone.extend(delta.removed_sections.into_iter().map(Gone::Section));
    // The text: the characters written, as one run.
    let wanted = rebuilt_text(n.text);
    match (&shape.text, &wanted) {
        // The file says the whole text: the reader gives back the paragraphs the model has.
        (Some(_), Some(_)) if n.text_native => {}
        (Some(have), Some(want)) if **have == *want => {}
        (None, None) => {}
        (Some(have), _) => t.text = Some((**have).clone()),
        (None, Some(_)) => gone.push(Gone::Text),
    }
    // The names: the reader gives `Name` and `NameU` the `draw:name` written.
    let written = n.name.map(Arc::<str>::from);
    if shape.name != written {
        match &shape.name {
            Some(v) => t.name = Some(v.to_string()),
            None => gone.push(Gone::Name),
        }
    }
    if shape.name_u != written {
        match &shape.name_u {
            Some(v) => t.name_u = Some(v.to_string()),
            None => gone.push(Gone::NameU),
        }
    }
    t.master = shape.master;
    t.master_shape = shape.master_shape;
    // A guide has no element of its own in ODF: the writer writes it as a shape, and the extension says what it is.
    t.kind = (shape.kind == ShapeType::Guide).then_some(ShapeType::Guide);
    // The VSDX data no ODF attribute says, and the foreign data beyond the picture: shells of the shape and of its foreign data.
    t.vsdx = super::shell::shape_shell(shape, &n.expected).map(Arc::new);
    let reads = n.picture_bytes.map(|bytes| crate::picturetype::picture_kind(bytes).map(|kind| kind.foreign));
    t.foreign = shape.foreign.as_deref().and_then(|f| super::shell::foreign_shell(f, reads));
    gone.sort_by_cached_key(Gone::token);
    gone.dedup();
    t.gone = gone;
    if !t.holds_nothing_but_id() || referenced {
        t.id = Some(shape.id);
    }
    // Tier 3: the fingerprint of the native outputs each item rests on, as written.
    let cells = super::tier3::Written {
        native: n.style,
        display: n.display.map_or(&[][..], |d| &d.native[..]),
        unjudged: n.display.map_or(&[][..], |d| &d.unjudged[..]),
    };
    super::tier3::stamp_new(&mut t, n.text, n.format, n.name, n.outline, n.picture_bytes, cells);
    t
}

/// The `ID`s of the sheets that the formulas of a shape's cells name (`Sheet.5!Width`, `Sheet.5`), in order and once each, leaving out the cells named in
/// `skip` (the cells the reader rebuilds from the file's own glue, whatever `ID`s the model had).
pub(crate) fn named_sheets(sheet: &Sheet, skip: &[&str]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut scan = |text: Option<&str>| {
        if let Some(text) = text {
            collect_sheet_ids(text, &mut out);
        }
    };
    for (name, cell) in &sheet.cells {
        if !skip.contains(&name.as_str()) {
            scan(cell.formula.text());
        }
    }
    for section in &sheet.sections {
        for cell in section.cells.values() {
            scan(cell.formula.text());
        }
        for row in section.rows.iter() {
            for cell in row.cells.values() {
                scan(cell.formula.text());
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Every number that follows `Sheet.` in `text`, in any case.
fn collect_sheet_ids(text: &str, out: &mut Vec<u32>) {
    let lower = text.to_ascii_lowercase();
    for (at, word) in lower.match_indices("sheet.") {
        let digits: String = lower.get(at + word.len()..).unwrap_or_default().chars().take_while(char::is_ascii_digit).collect();
        if let Ok(id) = digits.parse::<u32>() {
            out.push(id);
        }
    }
}

/// The `ID`s of the sheets the formula of one cell names.
pub(crate) fn formula_sheets(cell: &Cell) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(text) = cell.formula.text() {
        collect_sheet_ids(text, &mut out);
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------ the check of the file made

/// Whether two cells are the same for what a save promises: the same formula text, and where there is none the same value (the same text, or the same
/// number within a rounding), the same error, attributes and content; the unit is not compared, for it says only how a value shows.
fn cell_reads_back(a: &Cell, b: &Cell) -> bool {
    if a.formula.text() != b.formula.text() || a.error != b.error || a.extra != b.extra || a.raw != b.raw {
        return false;
    }
    if a.formula.text().is_some() {
        return true;
    }
    match (&a.value, &b.value) {
        (Some(x), Some(y)) => {
            x.text() == y.text() || x.number().zip(y.number()).is_some_and(|(m, n)| (m - n).abs() <= 1e-9 * (1.0 + m.abs().max(n.abs())))
        }
        (None, None) => true,
        _ => false,
    }
}

fn cells_read_back(a: &Cells, b: &Cells, steps: &mut u64) -> bool {
    let default_zero = |name: &CellName, c: Option<&Cell>| name.as_str() == "FillGradientEnabled" && c.is_none_or(|c| constant_near(Some(c), 0.0));
    let names: std::collections::BTreeSet<&CellName> = a.keys().chain(b.keys()).collect();
    names.into_iter().all(|name| {
        *steps = steps.saturating_add(1);
        match (a.get(name), b.get(name)) {
            (Some(x), Some(y)) => cell_reads_back(x, y),
            (x, y) => default_zero(name, x.or(y)),
        }
    })
}

/// The parts of a text with the runs of characters that follow each other made one, and the empty ones left out (one pass, with one string for a run).
fn runs_joined(text: &Text) -> Vec<TextPart> {
    let mut out: Vec<TextPart> = Vec::new();
    let mut run = String::new();
    for part in &text.parts {
        match part {
            TextPart::Chars(c) => run.push_str(c),
            // What the text holds whole of the file's markup (a mark, a drawing object in a paragraph) is no part of a tier-2 text (`model_tags.rs`): the writer leaves it out, and a save that
            // leaves it out is no text read back otherwise (a copy of a text frame that holds a picture in its paragraph is saved as it was before the frame was held).
            TextPart::OdfRaw(_) => {}
            other => {
                if !run.is_empty() {
                    out.push(TextPart::Chars(Arc::from(std::mem::take(&mut run))));
                }
                out.push(other.clone());
            }
        }
    }
    if !run.is_empty() {
        out.push(TextPart::Chars(Arc::from(run)));
    }
    out
}

/// Whether the shape `read`, as the file made reads, holds what tier 2 `t` carried and none of what it said is gone: its names and links to a master, its text, its
/// cells, and its sections with their rows and cells, each as it was written; and no cell, section, text or name that `gone` names. Only what tier 2 carries is
/// compared: what is written natively has the checks of its own, and a read adds cells and sections of its own (the layer, the rows of the text) that the model
/// of a shape never read does not have. A row type of `""` is none. `Err` says what differs, in words.
///
/// The work is linear in what tier 2 carries and in the sections of the shape read: its sections are indexed by their keys once, and each section carried is a look
/// in the index (a search of the shape's sections for each was a pass over them for each), every turn counted in `steps`.
pub(crate) fn reads_back(t: &Tier2, read: &Shape, steps: &mut u64) -> Result<(), &'static str> {
    if let Some(want) = &t.name
        && read.name.as_deref() != Some(want.as_str())
    {
        return Err("the name");
    }
    if let Some(want) = &t.name_u
        && read.name_u.as_deref() != Some(want.as_str())
    {
        return Err("the name");
    }
    if t.master != read.master || t.master_shape != read.master_shape {
        return Err("the links to a master");
    }
    if let Some(want) = &t.kind
        && read.kind != *want
    {
        return Err("the kind of the shape");
    }
    if let Some(want) = &t.text
        && read.text.as_deref().is_none_or(|text| text.parts != want.parts)
    {
        // Two runs of characters that follow each other are one run of the file's text, and an empty run is none: the text is the same text when its runs are.
        *steps = steps.saturating_add(want.parts.len() as u64);
        if read.text.as_deref().map(runs_joined) != Some(runs_joined(want)) {
            return Err("the text");
        }
    }
    let sheet = &*read.sheet;
    for (name, cell) in &t.cells {
        *steps = steps.saturating_add(1);
        match sheet.cells.get(name) {
            Some(back) if cell_reads_back(cell, back) => {}
            _ => return Err("a cell"),
        }
    }
    let mut by_key: std::collections::BTreeMap<(&str, Option<u32>), &Arc<Section>> = std::collections::BTreeMap::new();
    *steps = steps.saturating_add(sheet.sections.len() as u64);
    for s in &sheet.sections {
        by_key.entry((s.name.as_str(), s.ix)).or_insert(s);
    }
    for section in &t.sections {
        *steps = steps.saturating_add(1);
        let Some(back) = by_key.get(&(section.name.as_str(), section.ix)) else { return Err("a section that is not in the file") };
        if !cells_read_back(&section.cells, &back.cells, steps) || section.rows.len() != back.rows.len() {
            return Err("a section");
        }
        for (x, y) in section.rows.iter().zip(back.rows.iter()) {
            *steps = steps.saturating_add(1);
            let kind = |r: &diacraft_doc::Row| r.row_type.as_deref().filter(|t| !t.is_empty()).map(str::to_owned);
            if x.name != y.name
                || x.ix != y.ix
                || kind(x) != kind(y)
                || x.local_name != y.local_name
                || x.del != y.del
                || !cells_read_back(&x.cells, &y.cells, steps)
            {
                return Err("a row of a section");
            }
        }
    }
    for gone in &t.gone {
        *steps = steps.saturating_add(1);
        let held = match gone {
            Gone::Cell(name) => sheet.cells.contains_key(name.as_str()),
            Gone::Section(key) => by_key.contains_key(&(key.name.as_str(), key.ix)),
            Gone::Name => read.name.is_some(),
            Gone::NameU => read.name_u.is_some(),
            Gone::Text => read.text.is_some(),
        };
        if held {
            return Err("something said to be gone");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_doc::{CellExtra, Links, Row};

    fn cell(formula: Option<&str>, value: &str) -> Cell {
        Cell::read(formula, Some(value), Some("IN"), None, &mut 1_000)
    }

    fn with_extra(mut c: Cell) -> Cell {
        c.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("t")), ..CellExtra::default() }));
        c
    }

    fn key(name: &str, ix: Option<u32>) -> SectionKey {
        SectionKey::new(name, ix)
    }

    fn put(shape: &mut Shape, name: &str, formula: Option<&str>, value: &str) {
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), cell(formula, value));
    }

    fn push_section(shape: &mut Shape, section: Section) {
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    }

    fn has_section(sections: &[Arc<Section>], name: &str) -> bool {
        sections.iter().any(|s| s.name.as_str() == name)
    }

    const NATIVE: Native<'static> = Native {
        frame: Frame::TwoD { cx: 1.0, cy: 0.5, w: 2.0, h: 1.0, angle: 0.0, flip: (false, false) },
        style: &[],
        geometry: true,
        picture: false,
        connection: true,
        connector: false,
        glued: [false, false],
        text: "",
        text_native: false,
        format: None,
        picture_bytes: None,
        expected: ShapeType::Unspecified,
        name: None,
        layer: None,
        outline: None,
        display: None,
    };

    /// A rectangle as the writer writes one and the reader gives it back: tier 2 holds nothing of it.
    fn two_d() -> (Shape, Native<'static>) {
        let mut s = Shape::new(1);
        for (name, formula, value) in [
            ("Width", None, "2"),
            ("Height", None, "1"),
            ("LocPinX", Some("Width*0.5"), "1"),
            ("LocPinY", Some("Height*0.5"), "0.5"),
            ("PinX", None, "1"),
            ("PinY", None, "0.5"),
        ] {
            put(&mut s, name, formula, value);
        }
        push_section(&mut s, Section::new("Geometry", Some(0)));
        push_section(&mut s, Section::new("Connection", None));
        (s, NATIVE)
    }

    /// A line (or, with `connector`, a connector) from (0, 0) to (1, 0) as the writer writes one.
    fn one_d(connector: bool) -> (Shape, Native<'static>) {
        let mut s = Shape::new(1);
        for (name, formula, value) in [
            ("BeginX", None, "0"),
            ("BeginY", None, "0"),
            ("EndX", None, "1"),
            ("EndY", None, "0"),
            ("Width", Some("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"), "1"),
            ("Height", None, "0"),
            ("Angle", Some("ATAN2(EndY-BeginY,EndX-BeginX)"), "0"),
            ("PinX", Some("(BeginX+EndX)/2"), "0.5"),
            ("PinY", Some("(BeginY+EndY)/2"), "0"),
            ("LocPinX", Some("Width*0.5"), "0.5"),
            ("LocPinY", Some("Height*0.5"), "0"),
        ] {
            put(&mut s, name, formula, value);
        }
        push_section(&mut s, Section::new("Geometry", Some(0)));
        // The four standard rows the reader gives every shape, a segment too: written natively, as a rectangle's are.
        push_section(&mut s, Section::new("Connection", None));
        if connector {
            let mut user = Section::new("User", None);
            user.rows.push(Arc::new(Row { name: Some(Arc::from("DiaCraftRoute")), ..Row::default() }));
            push_section(&mut s, user);
        }
        (s, Native { frame: Frame::OneD, connector, ..NATIVE })
    }

    fn gone_section(t: &Tier2, name: &str) -> bool {
        t.gone.contains(&Gone::Section(key(name, None)))
    }

    /// A cell with a number of its own and no more is what an end of a segment is; a formula, anything else of the cell's, no value and a value that is not a number
    /// are none, whichever of the three conditions fails.
    #[test]
    fn a_plain_number_is_a_number_and_nothing_else_of_the_cell() {
        assert!(plain_number(Some(&cell(None, "1.5"))));
        assert!(!plain_number(None));
        assert!(!plain_number(Some(&cell(Some("1+1"), "2"))), "a formula");
        assert!(!plain_number(Some(&with_extra(cell(None, "1.5")))), "an extra");
        assert!(!plain_number(Some(&cell(None, "text"))), "not a number");
        assert!(!plain_number(Some(&Cell::new())), "no value");
    }

    /// A cell is a constant near a number when it holds no formula, nothing else of its own and a number within the writer's tolerance; each condition by itself
    /// A constant cell with text or nodes of its own is not what the reader rebuilds, and the frame cells and the pin of a
    /// shape would be judged rebuilt and the extra dropped by the save, which no read back of what is carried would see.
    #[test]
    fn a_constant_is_near_a_number_only_when_it_holds_nothing_else_of_its_own() {
        assert!(constant_near(Some(&cell(None, "2")), 2.0));
        assert!(constant_near(Some(&cell(None, "2.0000000000001")), 2.0), "within the tolerance");
        assert!(!constant_near(None, 2.0), "no cell");
        assert!(!constant_near(Some(&cell(Some("1+1"), "2")), 2.0), "a formula");
        assert!(!constant_near(Some(&with_extra(cell(None, "2"))), 2.0), "an extra");
        assert!(!constant_near(Some(&cell(None, "3")), 2.0), "another number");
        assert!(!constant_near(Some(&cell(None, "x")), 2.0), "no number");
        // The consequence: the width of a rectangle that holds text of its own is carried, and one that holds none is not.
        let (plain, native) = two_d();
        assert!(tier2(&plain, &native, false).cells.is_empty(), "a plain rectangle carries nothing");
        let (mut extra, native) = two_d();
        Arc::make_mut(&mut extra.sheet).cells.insert(CellName::new("Width"), with_extra(cell(None, "2")));
        assert!(tier2(&extra, &native, false).cells.contains_key("Width"), "a width with an extra is carried");
    }

    /// A style cell is reproduced when the cell holds no formula and nothing else of its own, and a colour when it is `#rrggbb` in lower case.
    #[test]
    fn a_style_cell_is_reproduced_when_it_is_plain_and_a_colour_when_it_is_in_lower_case() {
        for name in ["FillForegnd", "LineColor", "FillBkgnd"] {
            for good in ["#aabbcc", "#012345", "#ffffff"] {
                assert!(style_reproduced(name, &cell(None, good)), "{name} {good}");
            }
            for bad in ["#AABBCC", "#abc", "#aabbc", "aabbccd", "#aabbccd", "#aabbcg", "red", ""] {
                assert!(!style_reproduced(name, &cell(None, bad)), "{name} {bad:?}");
            }
        }
        assert!(style_reproduced("LineWeight", &cell(None, "anything")), "another name is not a colour");
        assert!(!style_reproduced("LineColor", &Cell::new()), "no value");
        assert!(!style_reproduced("LinePattern", &cell(Some("1+1"), "1")), "a formula");
        assert!(!style_reproduced("LinePattern", &with_extra(cell(None, "1"))), "an extra");
    }

    /// A cell holds exactly a formula of its own when it is a local formula of that text and holds nothing else of its own.
    #[test]
    fn a_cell_holds_exactly_a_formula_when_it_is_local_of_that_text_and_has_no_extra() {
        let f = cell(Some("Width*0.5"), "1");
        assert!(formula_is(Some(&f), "Width*0.5"));
        assert!(!formula_is(None, "Width*0.5"));
        assert!(!formula_is(Some(&f), "Height*0.5"), "another text");
        assert!(!formula_is(Some(&with_extra(f.clone())), "Width*0.5"), "an extra");
        assert!(!formula_is(Some(&cell(None, "1")), "Width*0.5"), "no formula");
        let inherited = Cell { formula: FormulaSlot::Inherit, ..cell(None, "1") };
        assert!(!formula_is(Some(&inherited), "Inh"), "the marker of an inherited formula is no formula of its own");
    }

    /// The stand-ins are unequal to any cell and section the model holds.
    #[test]
    fn the_stand_ins_are_not_equal_to_what_a_model_holds() {
        let u = unlike("PinX");
        assert_ne!(u, Cell::new());
        assert_eq!(u.raw.get("DiaCraftRebuild"), Some("PinX"));
        let s = unlike_section(&key("Geometry", Some(0)));
        assert_ne!(s, Section::new("Geometry", Some(0)));
        assert_eq!(s.raw.get("DiaCraftRebuild"), Some("1"));
    }

    /// A shape the writer writes natively has no tier 2: a rectangle, a line and a connector.
    #[test]
    fn a_shape_the_writer_writes_natively_carries_nothing() {
        for (shape, native) in [two_d(), one_d(false), one_d(true)] {
            assert_eq!(tier2(&shape, &native, false), Tier2::default(), "{:?}", native.frame);
        }
        // Only an id that is named by a formula of another shape keeps its `ID`.
        let (shape, native) = two_d();
        assert_eq!(tier2(&shape, &native, true).id, Some(1));
    }

    /// The links of the sheet are the model's.
    #[test]
    fn the_rebuilt_sheet_has_the_models_links() {
        let (mut s, n) = two_d();
        Arc::make_mut(&mut s.sheet).links = Links { line: Some(2), fill: Some(3), text: Some(4) };
        assert_eq!(rebuilt(&s, &n).links, s.sheet.links);
    }

    /// An end of a segment that is glued is rebuilt from the glue, whatever its cells hold; one that is not glued is its own and a formula in it is carried.
    #[test]
    fn a_glued_end_is_rebuilt_from_the_glue_and_a_free_end_with_a_formula_is_carried() {
        let (mut s, mut n) = one_d(true);
        for name in ["BeginX", "BeginY"] {
            put(&mut s, name, Some("Sheet.5!PinX"), "0");
        }
        n.glued = [true, false];
        let t = tier2(&s, &n, false);
        assert!(t.cells.is_empty(), "a glued end is the reader's: {:?}", t.cells.keys().collect::<Vec<_>>());
        n.glued = [false, false];
        let t = tier2(&s, &n, false);
        let names: Vec<&str> = t.cells.keys().map(CellName::as_str).collect();
        assert_eq!(names, ["BeginX", "BeginY"], "a free end holding a formula is carried");
    }

    /// The connection section is the writer's four side middles only where it wrote them.
    #[test]
    fn a_connection_section_is_carried_unless_it_is_the_rebuilt_one() {
        // Written natively: not carried. Not written natively: the model's is carried.
        let (s, mut n) = two_d();
        assert!(!has_section(&tier2(&s, &n, false).sections, "Connection"));
        n.connection = false;
        assert!(has_section(&tier2(&s, &n, false).sections, "Connection"), "the writer wrote none, so the model's is carried");
        // The model holds none and the reader gives a rectangle four: it is gone. **A line and a connector too**: the reader gives every shape the four, a segment
        // included, and a segment with none came back with four.
        let (mut s, n) = two_d();
        Arc::make_mut(&mut s.sheet).sections.retain(|x| x.name.as_str() != "Connection");
        assert!(gone_section(&tier2(&s, &n, false), "Connection"), "a rectangle without one");
        for connector in [false, true] {
            let (mut s, n) = one_d(connector);
            assert!(!gone_section(&tier2(&s, &n, false), "Connection"), "a segment with the four (connector {connector})");
            Arc::make_mut(&mut s.sheet).sections.retain(|x| x.name.as_str() != "Connection");
            assert!(gone_section(&tier2(&s, &n, false), "Connection"), "a segment with none (connector {connector})");
            let n = Native { connection: false, ..n };
            assert!(
                gone_section(&tier2(&s, &n, false), "Connection"),
                "none written and none held is a section the rebuild would not give (connector {connector})"
            );
        }
    }

    /// A segment's outline is read back as rows through its points and no flag of the section's own, so a section that holds one
    /// (`NoFill` 1, as a connector `connector.add` makes holds) is carried whole, and one that holds none is not (the flag was lost, and a bent connector given a fill would fill).
    #[test]
    fn a_segments_outline_that_holds_a_flag_of_its_own_is_carried() {
        for connector in [false, true] {
            let (mut s, n) = one_d(connector);
            assert!(!has_section(&tier2(&s, &n, false).sections, "Geometry"), "no flag (connector {connector})");
            let sheet = Arc::make_mut(&mut s.sheet);
            let outline = sheet.sections.iter_mut().find(|x| x.name.as_str() == "Geometry").unwrap();
            Arc::make_mut(outline).cells.insert(CellName::new("NoFill"), cell(None, "1"));
            assert!(has_section(&tier2(&s, &n, false).sections, "Geometry"), "NoFill 1 (connector {connector})");
        }
        // A rectangle's outline keeps what it holds: its rows are read back with its flags, so nothing is carried.
        let (mut s, n) = two_d();
        let sheet = Arc::make_mut(&mut s.sheet);
        let outline = sheet.sections.iter_mut().find(|x| x.name.as_str() == "Geometry").unwrap();
        Arc::make_mut(outline).cells.insert(CellName::new("NoFill"), cell(None, "1"));
        assert!(!has_section(&tier2(&s, &n, false).sections, "Geometry"));
    }

    /// A connector's user section is the route marker alone; any other row or a cell of the section's own is carried, and a connector that lacks it has it gone.
    #[test]
    fn a_connectors_user_section_is_the_route_marker_alone() {
        let carried = |s: &Shape, n: &Native<'_>| has_section(&tier2(s, n, false).sections, "User");
        let user_with = |extra_row: bool, own_cell: bool| {
            let mut user = Section::new("User", None);
            user.rows.push(Arc::new(Row { name: Some(Arc::from("DiaCraftRoute")), ..Row::default() }));
            if extra_row {
                user.rows.push(Arc::new(Row { name: Some(Arc::from("Other")), ..Row::default() }));
            }
            if own_cell {
                user.cells.insert(CellName::new("Flag"), cell(None, "1"));
            }
            user
        };
        let (mut s, n) = one_d(true);
        assert!(!carried(&s, &n), "the marker alone is the reader's");
        for (extra_row, own_cell) in [(true, false), (false, true), (true, true)] {
            Arc::make_mut(&mut s.sheet).sections.retain(|x| x.name.as_str() != "User");
            push_section(&mut s, user_with(extra_row, own_cell));
            assert!(carried(&s, &n), "row {extra_row} cell {own_cell}");
        }
        // A user section with no row at all holds only what the marker would, and a model with none is gone one.
        Arc::make_mut(&mut s.sheet).sections.retain(|x| x.name.as_str() != "User");
        assert!(gone_section(&tier2(&s, &n, false), "User"), "the reader adds the row the model lacks");
        // A shape that is no connector has no marker to rebuild.
        let (mut line, n) = one_d(false);
        push_section(&mut line, user_with(false, false));
        assert!(!gone_section(&tier2(&line, &n, false), "User"));
    }

    /// The plain text written is the one run the reader gives back: the same text is not carried, another is, and none where text was written is gone.
    #[test]
    fn a_text_is_carried_only_when_it_is_not_the_one_run_the_reader_gives_back() {
        let (mut s, mut n) = two_d();
        n.text = "hello";
        s.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("hello"))], ..Default::default() }));
        let t = tier2(&s, &n, false);
        assert!(t.text.is_none() && t.gone.is_empty(), "{t:?}");
        s.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("hel")), TextPart::Chars(Arc::from("lo"))], ..Default::default() }));
        assert!(tier2(&s, &n, false).text.is_some(), "two runs are not the one the reader gives");
        s.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("bye"))], ..Default::default() }));
        assert_eq!(tier2(&s, &n, false).text.map(|t| t.parts.len()), Some(1), "another text");
        s.text = None;
        assert_eq!(tier2(&s, &n, false).gone, [Gone::Text], "the reader gives a text the model lacks");
        n.text = "";
        assert!(tier2(&s, &n, false).gone.is_empty(), "no text written and none held");
    }

    /// The names the reader gives a shape are the `draw:name` written.
    #[test]
    fn the_names_are_carried_when_they_are_not_the_one_written_and_gone_when_the_model_lacks_them() {
        let (mut s, mut n) = two_d();
        n.name = Some("box");
        let gone = tier2(&s, &n, false).gone;
        assert!(gone.len() == 2 && gone.contains(&Gone::Name) && gone.contains(&Gone::NameU), "{gone:?}");
        s.name = Some(Arc::from("box"));
        s.name_u = Some(Arc::from("box"));
        assert_eq!(tier2(&s, &n, false), Tier2::default());
        s.name = Some(Arc::from("other"));
        s.name_u = Some(Arc::from("other.1"));
        let t = tier2(&s, &n, false);
        assert_eq!((t.name.as_deref(), t.name_u.as_deref()), (Some("other"), Some("other.1")));
    }

    /// The sheets a formula names, in order of appearance, in any case; and none for a cell with no formula or a `Sheet.` with no number.
    #[test]
    fn the_sheets_a_formula_names_are_the_numbers_after_sheet() {
        assert_eq!(formula_sheets(&cell(Some("Sheet.5!Width+SHEET.12!X+sheet.5"), "1")), [5, 12, 5]);
        assert!(formula_sheets(&cell(None, "1")).is_empty());
        assert!(formula_sheets(&cell(Some("Sheet.x!Width"), "1")).is_empty());
    }

    /// Two cells read back as each other when every part of them is the same, bar the unit, and a number within a rounding is the same number.
    #[test]
    fn two_cells_are_the_same_for_a_save_when_every_part_is_the_same_bar_the_unit() {
        let a = cell(None, "1");
        assert!(cell_reads_back(&a, &cell(None, "1")));
        let mut other_unit = a.clone();
        other_unit.unit = Some(Arc::from("MM"));
        assert!(cell_reads_back(&a, &other_unit), "a unit says only how a value shows");
        // Each part alone.
        assert!(!cell_reads_back(&cell(Some("1+0"), "1"), &cell(Some("1+1"), "1")), "the formula");
        assert!(!cell_reads_back(&cell(Some("1"), "1"), &a), "a formula against none");
        let mut error = a.clone();
        error.error = Some(Arc::from("#REF!"));
        assert!(!cell_reads_back(&a, &error), "the error");
        assert!(!cell_reads_back(&a, &with_extra(a.clone())), "the extra");
        let mut raw = a.clone();
        raw.raw.push("X", "1");
        assert!(!cell_reads_back(&a, &raw), "the raw attributes");
        // Where there is a formula the values are not compared.
        assert!(cell_reads_back(&cell(Some("1+1"), "2"), &cell(Some("1+1"), "3")));
        // Numbers: within a rounding, relative to their size; not otherwise.
        assert!(cell_reads_back(&cell(None, "1"), &cell(None, "1.000000001")));
        assert!(cell_reads_back(&cell(None, "0"), &cell(None, "0.0")));
        assert!(cell_reads_back(&cell(None, "1000"), &cell(None, "1000.0000005")), "the tolerance grows with the value");
        assert!(!cell_reads_back(&cell(None, "1"), &cell(None, "1.00001")));
        assert!(!cell_reads_back(&cell(None, "1"), &cell(None, "2")));
        assert!(!cell_reads_back(&cell(None, "0"), &cell(None, "0.00000001")));
        assert!(!cell_reads_back(&cell(None, "x"), &cell(None, "y")));
        // No value on both sides is the same, and one against none is not.
        assert!(cell_reads_back(&Cell::new(), &Cell::new()));
        assert!(!cell_reads_back(&Cell::new(), &a));
        assert!(!cell_reads_back(&a, &Cell::new()));
    }

    /// Two lists of cells are the same when each name is, and a cell on one side alone is the same only where it is the default of its name; the work is counted.
    #[test]
    fn two_lists_of_cells_are_the_same_when_each_is_and_a_default_on_one_side_is_nothing() {
        let list = |cells: &[(&str, Cell)]| -> Cells {
            let mut c = Cells::new();
            for (n, v) in cells {
                c.insert(CellName::new(n), v.clone());
            }
            c
        };
        let mut steps = 0;
        let a = list(&[("A", cell(None, "1")), ("B", cell(None, "2"))]);
        assert!(cells_read_back(&a, &a.clone(), &mut steps));
        assert_eq!(steps, 2, "a step for each name");
        assert!(!cells_read_back(&a, &list(&[("A", cell(None, "1")), ("B", cell(None, "3"))]), &mut steps));
        // A cell on one side alone: only the gradient flag at its default is nothing.
        let zero = cell(None, "0");
        assert!(cells_read_back(&list(&[("FillGradientEnabled", zero.clone())]), &list(&[]), &mut steps));
        assert!(cells_read_back(&list(&[]), &list(&[("FillGradientEnabled", zero.clone())]), &mut steps));
        assert!(!cells_read_back(&list(&[("FillGradientEnabled", cell(None, "1"))]), &list(&[]), &mut steps), "a gradient that is on");
        assert!(!cells_read_back(&list(&[("Other", zero.clone())]), &list(&[]), &mut steps), "another name with a zero");
        assert!(!cells_read_back(&list(&[("A", cell(None, "1"))]), &list(&[]), &mut steps));
    }

    /// The parts of a text with the runs that follow each other joined and the empty ones dropped.
    #[test]
    fn runs_of_characters_that_follow_each_other_are_one_and_empty_ones_are_none() {
        let chars = |t: &str| TextPart::Chars(Arc::from(t));
        let text = |parts: Vec<TextPart>| Text { parts, ..Default::default() };
        assert_eq!(
            runs_joined(&text(vec![chars("a"), chars("b"), TextPart::Tp(1), chars(""), chars("c"), chars("d")])),
            [chars("ab"), TextPart::Tp(1), chars("cd")]
        );
        assert_eq!(runs_joined(&text(vec![TextPart::Tp(1), chars("a")])), [TextPart::Tp(1), chars("a")]);
        assert_eq!(runs_joined(&text(vec![chars("a"), TextPart::Tp(2)])), [chars("a"), TextPart::Tp(2)]);
        assert_eq!(runs_joined(&text(vec![chars(""), TextPart::Tp(3), chars("")])), [TextPart::Tp(3)]);
        assert!(runs_joined(&text(vec![chars(""), chars("")])).is_empty());
        assert!(runs_joined(&text(Vec::new())).is_empty());
    }

    fn check(t: &Tier2, read: &Shape) -> Result<(), &'static str> {
        reads_back(t, read, &mut 0)
    }

    fn section_of(name: &str, rows: Vec<Row>, cells: &[(&str, Cell)]) -> Section {
        let mut s = Section::new(name, None);
        for row in rows {
            s.rows.push(Arc::new(row));
        }
        for (n, c) in cells {
            s.cells.insert(CellName::new(n), c.clone());
        }
        s
    }

    fn named_row(name: &str) -> Row {
        Row { name: Some(Arc::from(name)), ..Row::default() }
    }

    fn text_of_runs(parts: &[&str]) -> Text {
        Text { parts: parts.iter().map(|p| TextPart::Chars(Arc::from(*p))).collect(), ..Default::default() }
    }

    /// A shape as the file reads, with its names and links to a master, and what tier 2 carries of that alone.
    fn read_shape() -> (Shape, Tier2) {
        let mut read = Shape::new(1);
        read.name = Some(Arc::from("n"));
        read.name_u = Some(Arc::from("u"));
        read.master = Some(2);
        read.master_shape = Some(3);
        let base = Tier2 { name: Some("n".to_owned()), name_u: Some("u".to_owned()), master: Some(2), master_shape: Some(3), ..Tier2::default() };
        (read, base)
    }

    /// The names, the links to a master and the text of a read are those that tier 2 carried, and each difference names itself.
    #[test]
    fn the_names_the_links_to_a_master_and_the_text_are_compared_and_every_difference_is_named() {
        let (read, base) = read_shape();
        assert_eq!(check(&base, &read), Ok(()));
        assert_eq!(check(&Tier2 { name: Some("x".to_owned()), ..base.clone() }, &read), Err("the name"));
        assert_eq!(check(&Tier2 { name_u: Some("x".to_owned()), ..base.clone() }, &read), Err("the name"));
        // The links to a master: each alone.
        assert_eq!(check(&Tier2 { master: Some(9), ..base.clone() }, &read), Err("the links to a master"));
        assert_eq!(check(&Tier2 { master_shape: Some(9), ..base.clone() }, &read), Err("the links to a master"));
        assert_eq!(check(&Tier2 { master: None, ..base.clone() }, &read), Err("the links to a master"));
        // The text: the same runs, the same text in other runs, another text, and none.
        let with_text = |t: Text| {
            let mut s = read.clone();
            s.text = Some(Arc::new(t));
            s
        };
        let wants = Tier2 { text: Some(text_of_runs(&["ab"])), ..base.clone() };
        assert_eq!(check(&wants, &with_text(text_of_runs(&["ab"]))), Ok(()));
        assert_eq!(check(&wants, &with_text(text_of_runs(&["a", "b"]))), Ok(()), "two runs that follow each other are one");
        assert_eq!(check(&wants, &with_text(text_of_runs(&["a", "", "b"]))), Ok(()), "and an empty run is none");
        assert_eq!(check(&wants, &with_text(text_of_runs(&["ac"]))), Err("the text"));
        assert_eq!(check(&wants, &read), Err("the text"), "the file has no text");
    }

    /// The kind a shape carries is compared with the kind it is read with, and only a guide is carried: a shape that is not one carries none, and a
    /// tier 2 that holds a kind holds more than an `ID`.
    #[test]
    fn the_kind_of_a_guide_is_carried_and_compared() {
        let (read, base) = read_shape();
        let guide = Tier2 { kind: Some(ShapeType::Guide), ..base.clone() };
        assert_eq!(check(&guide, &read), Err("the kind of the shape"), "the file reads it a shape");
        let mut read_guide = read.clone();
        read_guide.kind = ShapeType::Guide;
        assert_eq!(check(&guide, &read_guide), Ok(()));
        assert_eq!(check(&base, &read_guide), Ok(()), "a tier 2 that carries no kind compares none");
        assert!(!Tier2 { id: Some(1), kind: Some(ShapeType::Guide), ..Tier2::default() }.holds_nothing_but_id());
        assert!(Tier2 { id: Some(1), ..Tier2::default() }.holds_nothing_but_id());
        // The rebuild gives the tier 2 of a guide its kind and a shape of any other kind none.
        let n = Native {
            frame: Frame::TwoD { cx: 1.0, cy: 0.5, w: 2.0, h: 1.0, angle: 0.0, flip: (false, false) },
            style: &[],
            geometry: true,
            picture: false,
            connection: true,
            connector: false,
            glued: [false, false],
            text: "",
            text_native: false,
            format: None,
            picture_bytes: None,
            expected: ShapeType::Unspecified,
            name: None,
            layer: None,
            outline: None,
            display: None,
        };
        let mut shape = Shape::new(4);
        assert_eq!(tier2(&shape, &n, false).kind, None);
        for kind in [ShapeType::Shape, ShapeType::Group, ShapeType::Foreign, ShapeType::Unspecified] {
            shape.kind = kind;
            assert_eq!(tier2(&shape, &n, false).kind, None);
        }
        shape.kind = ShapeType::Guide;
        let t = tier2(&shape, &n, false);
        assert_eq!(t.kind, Some(ShapeType::Guide));
        assert_eq!(t.id, Some(4), "a tier 2 that carries a kind carries the shape's ID");
    }

    /// The turn and the flips the writer wrote are what the reader gives back (`Angle`, `FlipX`, `FlipY`): a cell that holds the number written is the
    /// rebuild's own and costs no tier 2, one that holds another is carried, and a cell that is 0 where nothing is written is the reader's default.
    #[test]
    fn a_turn_and_the_flips_that_are_written_are_reproduced_and_another_number_is_carried() {
        let frame = |angle: f64, flip: (bool, bool)| Frame::TwoD { cx: 1.0, cy: 0.5, w: 2.0, h: 1.0, angle, flip };
        let native = |angle: f64, flip: (bool, bool)| Native {
            frame: frame(angle, flip),
            style: &[],
            geometry: true,
            picture: false,
            connection: true,
            connector: false,
            glued: [false, false],
            text: "",
            text_native: false,
            format: None,
            picture_bytes: None,
            expected: ShapeType::Unspecified,
            name: None,
            layer: None,
            outline: None,
            display: None,
        };
        let mut shape = Shape::new(4);
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, v) in [("Width", "2"), ("Height", "1"), ("PinX", "1"), ("PinY", "0.5")] {
            sheet.cells.insert(CellName::new(name), cell(None, v));
        }
        for (name, f) in [("LocPinX", "Width*0.5"), ("LocPinY", "Height*0.5")] {
            sheet.cells.insert(CellName::new(name), cell(Some(f), "1"));
        }
        let carried = |s: &Shape, n: &Native<'_>| -> Vec<String> { tier2(s, n, false).cells.keys().map(|k| k.as_str().to_owned()).collect() };
        // Nothing turned, nothing flipped, and no cell: nothing is carried; the cells at 0 are the reader's default.
        assert!(carried(&shape, &native(0.0, (false, false))).is_empty());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), cell(None, "0"));
        assert!(carried(&shape, &native(0.0, (false, false))).is_empty(), "0 is the default");
        // A turn that is written, and the cell that holds it: reproduced. Another number in the cell is carried; so is a turn that was not written.
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), cell(None, "0.5"));
        assert!(carried(&shape, &native(0.5, (false, false))).is_empty(), "the turn written is the turn the reader gives");
        assert_eq!(carried(&shape, &native(0.4, (false, false))), ["Angle"], "another turn is carried");
        assert_eq!(carried(&shape, &native(0.0, (false, false))), ["Angle"], "a turn that is not written is carried");
        // The same for each flip alone and both.
        for (cells, flip) in [(&["FlipX"][..], (true, false)), (&["FlipY"][..], (false, true)), (&["FlipX", "FlipY"][..], (true, true))] {
            let mut s = shape.clone();
            Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("Angle"), cell(None, "0"));
            for c in cells {
                Arc::make_mut(&mut s.sheet).cells.insert(CellName::new(c), cell(None, "1"));
            }
            assert!(carried(&s, &native(0.0, flip)).is_empty(), "flips {flip:?} written and held");
            let mut other = carried(&s, &native(0.0, (false, false)));
            other.sort();
            assert_eq!(other, cells.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>(), "flips {flip:?} held and not written are carried");
            if cells.len() == 1 {
                let wrong = if flip.0 { (false, true) } else { (true, false) };
                assert!(!carried(&s, &native(0.0, wrong)).is_empty(), "the other flip written: the cell is not the rebuild's");
            }
        }
    }

    /// The cells carried are in the read as they were, within a rounding, and none is missing.
    #[test]
    fn the_cells_carried_are_compared_and_a_missing_or_different_one_is_named() {
        let (read, base) = read_shape();
        let mut cells = Cells::new();
        cells.insert(CellName::new("A"), cell(None, "1"));
        let carries = Tier2 { cells, ..base };
        let with_cell = |value: Option<&str>| {
            let mut s = read.clone();
            if let Some(v) = value {
                put(&mut s, "A", None, v);
            }
            s
        };
        assert_eq!(check(&carries, &with_cell(Some("1"))), Ok(()));
        assert_eq!(check(&carries, &with_cell(Some("2"))), Err("a cell"));
        assert_eq!(check(&carries, &with_cell(None)), Err("a cell"));
    }

    /// A section carried is found in the read by its key and holds the same cells and the same number of rows.
    #[test]
    fn the_sections_carried_are_found_by_key_and_hold_the_same_cells_and_rows() {
        let (read, base) = read_shape();
        let carries = |s: Section| Tier2 { sections: vec![Arc::new(s)], ..base.clone() };
        let with_section = |s: Option<Section>| {
            let mut shape = read.clone();
            if let Some(s) = s {
                push_section(&mut shape, s);
            }
            shape
        };
        let user = |rows: Vec<Row>, flag: &str| section_of("User", rows, &[("Flag", cell(None, flag))]);
        assert_eq!(check(&carries(user(vec![named_row("a")], "1")), &with_section(Some(user(vec![named_row("a")], "1")))), Ok(()));
        assert_eq!(check(&carries(user(vec![named_row("a")], "1")), &with_section(None)), Err("a section that is not in the file"));
        assert_eq!(check(&carries(user(vec![named_row("a")], "1")), &with_section(Some(user(vec![named_row("a")], "2")))), Err("a section"), "cells");
        assert_eq!(
            check(&carries(user(vec![named_row("a")], "1")), &with_section(Some(user(vec![named_row("a"), named_row("b")], "1")))),
            Err("a section"),
            "rows"
        );
        // The work done is counted.
        let mut steps = 0;
        assert_eq!(reads_back(&carries(user(vec![named_row("a")], "1")), &with_section(Some(user(vec![named_row("a")], "1"))), &mut steps), Ok(()));
        assert!(steps >= 4, "{steps}");
    }

    /// The rows of a section carried hold the same name, index, type, local name, deletion and cells; a row type of nothing is none.
    #[test]
    fn the_rows_of_a_section_are_compared_part_by_part_and_a_type_of_nothing_is_none() {
        let (read, base) = read_shape();
        let one = |r: Row| Tier2 { sections: vec![Arc::new(section_of("User", vec![r], &[]))], ..base.clone() };
        let there = |r: Row| {
            let mut shape = read.clone();
            push_section(&mut shape, section_of("User", vec![r], &[]));
            shape
        };
        let base_row = || Row {
            name: Some(Arc::from("r")),
            ix: Some(1),
            local_name: Some(Arc::from("L")),
            row_type: Some(Arc::from("MoveTo")),
            ..Row::default()
        };
        assert_eq!(check(&one(base_row()), &there(base_row())), Ok(()));
        let mut other_cells = base_row();
        other_cells.cells.insert(CellName::new("X"), cell(None, "1"));
        let differs = vec![
            ("name", Row { name: Some(Arc::from("s")), ..base_row() }),
            ("ix", Row { ix: Some(2), ..base_row() }),
            ("type", Row { row_type: Some(Arc::from("LineTo")), ..base_row() }),
            ("local name", Row { local_name: None, ..base_row() }),
            ("del", Row { del: true, ..base_row() }),
            ("cells", other_cells),
        ];
        for (what, other) in differs {
            assert_eq!(check(&one(base_row()), &there(other)), Err("a row of a section"), "{what}");
        }
        let none = || Row { row_type: None, ..base_row() };
        let empty = || Row { row_type: Some(Arc::from("")), ..base_row() };
        assert_eq!(check(&one(none()), &there(empty())), Ok(()), "a row type of nothing is none");
        assert_eq!(check(&one(empty()), &there(none())), Ok(()));
        assert_eq!(check(&one(base_row()), &there(none())), Err("a row of a section"), "a type against none");
    }

    /// What tier 2 says is gone is not in the read: each kind of removal, held and not held.
    #[test]
    fn what_is_said_to_be_gone_is_not_in_the_read() {
        let (read, base) = read_shape();
        let mut held = read;
        put(&mut held, "A", None, "1");
        push_section(&mut held, Section::new("User", None));
        held.text = Some(Arc::new(text_of_runs(&["t"])));
        let kinds = || [Gone::Cell("A".to_owned()), Gone::Section(key("User", None)), Gone::Name, Gone::NameU, Gone::Text];
        for g in kinds() {
            assert_eq!(check(&Tier2 { gone: vec![g.clone()], ..base.clone() }, &held), Err("something said to be gone"), "{g:?}");
        }
        let mut bare = Shape::new(1);
        bare.master = Some(2);
        bare.master_shape = Some(3);
        for g in kinds() {
            let t = Tier2 { master: Some(2), master_shape: Some(3), gone: vec![g.clone()], ..Tier2::default() };
            assert_eq!(check(&t, &bare), Ok(()), "{g:?} is not there");
        }
    }

    /// The pin the writer expects the reader to give: **the markup's edge plus the place the shape carries** (the reader's derivation, `tier3::derive_pins`), the centre
    /// where the place is the default, and the centre again for a shape a style cell turns or flips (the reader derives nothing there). A pin that is what the reader gives
    /// is not carried, and one that is not is: the cell the rebuild holds is the model's own when the reader gives it, and another cell when it does not.
    #[test]
    fn the_pin_the_writer_expects_is_the_edge_plus_the_carried_place_and_the_centre_for_a_turned_shape() {
        let reads_back = |s: &Shape| rebuilt(s, &NATIVE).cells.get("PinX") == s.sheet.cells.get("PinX");
        // The frame is 2 wide with the centre at 1: the edge is at 0. The place `Width*0` puts the pin at the edge, `Width*0.25` half a unit in.
        let (mut s, _) = two_d();
        put(&mut s, "LocPinX", Some("Width*0"), "0");
        put(&mut s, "PinX", None, "0");
        assert!(reads_back(&s), "the pin at the edge, its place at 0: the reader derives it");
        put(&mut s, "PinX", None, "1");
        assert!(!reads_back(&s), "the pin at the centre with a place at 0 is not what the reader derives: carried");
        put(&mut s, "LocPinX", Some("Width*0.25"), "0.5");
        put(&mut s, "PinX", None, "0.5");
        assert!(reads_back(&s), "edge 0 plus the place 0.5");
        // A place that is a constant is its value.
        put(&mut s, "LocPinX", None, "0.5");
        assert!(reads_back(&s), "a constant place of 0.5");
        // A place that is no number (the reader derives nothing from it, so its pin is the centre): the pin the writer expects is the centre.
        let (mut text, _) = two_d();
        put(&mut text, "LocPinX", None, "left");
        assert!(
            text.sheet.cells.get("LocPinX").is_some_and(|c| c.value.as_ref().and_then(diacraft_doc::Value::number).is_none()),
            "the place is a text"
        );
        assert!(reads_back(&text), "the pin at the centre with a place that is no number");
        put(&mut text, "PinX", None, "0.5");
        assert!(!reads_back(&text), "a pin that is not the centre with it is carried");
        // The default place: the centre.
        let (mut d, _) = two_d();
        assert!(reads_back(&d), "the default pin is the centre");
        put(&mut d, "PinX", None, "0.5");
        assert!(!reads_back(&d), "a pin that is not the centre with the default place is carried");
        // A style cell that turns the shape: the reader derives nothing, so the pin is the centre however the place is carried.
        for (turn, value) in [("Angle", "0.5"), ("FlipX", "1"), ("FlipY", "1")] {
            let (mut t, _) = two_d();
            put(&mut t, turn, None, value);
            put(&mut t, "LocPinX", Some("Width*0"), "0");
            put(&mut t, "PinX", None, "0");
            assert!(!reads_back(&t), "{turn}: a pin at the edge is not the centre the reader gives: carried");
            put(&mut t, "PinX", None, "1");
            assert!(reads_back(&t), "{turn}: the pin at the centre is what the reader gives");
        }
        // A turn of 0 is none.
        let (mut z, _) = two_d();
        put(&mut z, "Angle", None, "0");
        put(&mut z, "LocPinX", Some("Width*0"), "0");
        put(&mut z, "PinX", None, "0");
        assert!(reads_back(&z), "an Angle of 0 turns nothing");
    }

    /// A picture frame (`Native::picture`) is rebuilt with no `Geometry` section at all, whatever `geometry` says: the outline a model
    /// holds is tier 2, a model with none carries nothing of an outline, and the four connection rows are the standard ones, as for any frame.
    #[test]
    fn a_picture_frame_is_rebuilt_with_no_outline_and_the_models_outline_is_tier_2() {
        let (shape, native) = two_d();
        // The same shape as a rectangle: its outline is what the reader gives back, so tier 2 holds nothing.
        assert!(tier2(&shape, &native, false).sections.is_empty());
        // As a picture frame, with `geometry` either way: the outline is the model's own, carried, and nothing is said to be gone.
        for geometry in [true, false] {
            let n = Native { picture: true, geometry, ..native.clone() };
            let t = tier2(&shape, &n, false);
            let carried: Vec<&str> = t.sections.iter().map(|s| s.name.as_str()).collect();
            assert_eq!(carried, ["Geometry"], "geometry {geometry}");
            assert!(!gone_section(&t, "Geometry") && !gone_section(&t, "Connection"), "geometry {geometry}");
        }
        // A model with no outline: nothing is carried for one, and no outline is said to be gone (the reader gives none to take away).
        let mut bare = shape.clone();
        Arc::make_mut(&mut bare.sheet).sections.retain(|s| s.name.as_str() != "Geometry");
        let t = tier2(&bare, &Native { picture: true, ..native.clone() }, false);
        assert!(t.sections.is_empty() && t.gone.is_empty() && t.holds_nothing_but_id(), "{t:?}");
        // The frame cells are the same as for any frame: a width that is not the one written is carried.
        let mut wide = bare.clone();
        put(&mut wide, "Width", None, "9");
        assert!(tier2(&wide, &Native { picture: true, ..native.clone() }, false).cells.contains_key("Width"));
        // And the connection rows are the frame's: a model without them is told to lose the four the reader gives.
        let mut none = bare;
        Arc::make_mut(&mut none.sheet).sections.clear();
        assert!(gone_section(&tier2(&none, &Native { picture: true, connection: false, ..native }, false), "Connection"));
    }

    /// The pin the writer expects of a turned or flipped shape is the centre less the map of the place (the reader's `derive_turned`), also where the model's turn is a
    /// formula the markup's number comes to; a turn of the model that is not the markup's, and no place carried, give the centre.
    #[test]
    fn the_pin_expected_of_a_turned_shape_is_the_centre_less_the_map_of_the_place() {
        let quarter = std::f64::consts::FRAC_PI_2;
        let model = |cells: &[(&str, Option<&str>, &str)]| {
            let mut s = Shape::new(1);
            for (name, formula, value) in cells {
                put(&mut s, name, *formula, value);
            }
            s
        };
        let at = |m: &Shape, angle: f64, flip: (bool, bool)| expected_pin(&m.sheet, (1.0, 0.5), (2.0, 1.0), angle, flip);
        let close = |got: (f64, f64), want: (f64, f64)| (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9;
        let left = model(&[("LocPinX", Some("Width*0"), "0")]);
        assert!(close(at(&left, 0.0, (false, false)), (0.0, 0.5)), "unturned: the left edge");
        assert!(close(at(&left, quarter, (false, false)), (1.0, -0.5)), "a quarter turn: the left of the middle line is below the centre");
        assert!(close(at(&left, 0.0, (true, false)), (2.0, 0.5)), "flipped across x: the left is on the right");
        // The place of the other axis, with the turn.
        let bottom = model(&[("LocPinY", Some("Height*0"), "0")]);
        assert!(close(at(&bottom, quarter, (false, false)), (1.5, 0.5)), "the bottom of the middle column after a quarter turn");
        // No place carried (the default places), or one that is no number: the centre.
        let centred = model(&[("LocPinX", Some("Width*0.5"), "1")]);
        assert!(close(at(&centred, quarter, (false, false)), (1.0, 0.5)));
        assert!(close(at(&model(&[]), quarter, (false, false)), (1.0, 0.5)));
        assert!(close(at(&model(&[("LocPinX", None, "left")]), quarter, (false, false)), (1.0, 0.5)));
        // A model whose turn is a formula that is the markup's number: derived; one that says another turn: the centre.
        let formula = model(&[("LocPinX", Some("Width*0"), "0"), ("Angle", Some("30 deg"), "1.5707963267948966")]);
        assert!(close(at(&formula, quarter, (false, false)), (1.0, -0.5)), "the carried turn is the markup's");
        let another = model(&[("LocPinX", Some("Width*0"), "0"), ("Angle", Some("30 deg"), "0.5")]);
        assert!(close(at(&another, quarter, (false, false)), (1.0, 0.5)), "a turn that is not the markup's: nothing is derived");
        // The number the markup has, as the model's constant: it is rebuilt, not carried, and does not stop the derivation.
        let same = model(&[("LocPinX", Some("Width*0"), "0"), ("Angle", None, "1.5707963267948966")]);
        assert!(close(at(&same, quarter, (false, false)), (1.0, -0.5)));
        // The tolerance of the turn is inclusive.
        let near = |delta: f64| model(&[("LocPinX", Some("Width*0"), "0"), ("Angle", Some("a"), &format!("{}", quarter + delta))]);
        assert!(close(at(&near(1.7e-4), quarter, (false, false)), (1.0, -0.5)));
        assert!(close(at(&near(1.9e-4), quarter, (false, false)), (1.0, 0.5)));
    }

    /// A pin the tier 2 carries that the reader would take out (it is not where the markup's frame puts it) or give another value is told before the file is
    /// written: no pin carried, a line, a pin that is the frame's with the place carried, and a turn that is not the markup's (the reader works nothing out) are not; a pin that
    /// is not the centre with the default place, and a pin of the centre with a place that moved the edge, are.
    #[test]
    fn a_carried_pin_the_reader_would_not_keep_is_told() {
        let carried = |cells: &[(&str, f64)]| {
            let mut t = Tier2::default();
            for (name, v) in cells {
                t.cells.insert(CellName::new(name), Cell::constant(diacraft_doc::Value::from_number(*v).unwrap()));
            }
            t
        };
        // The frame: centre (1, 0.5), 2 by 1: the left edge at 0, the bottom edge at 0.
        let drops = |frame: Frame, t: &Tier2| carried_pin_the_reader_drops(&Native { frame, ..NATIVE }, t);
        let upright = NATIVE.frame;
        assert!(!drops(upright, &Tier2::default()), "nothing carried");
        assert!(!drops(upright, &carried(&[("Width", 3.0)])), "no pin carried");
        assert!(!drops(Frame::OneD, &carried(&[("PinX", 7.0)])), "a line's frame is its ends");
        // A pin that is not the centre, with the default place: the edge is not where the frame has it.
        assert!(drops(upright, &carried(&[("PinX", 0.25)])), "PinX off the centre");
        assert!(drops(upright, &carried(&[("PinY", 7.0)])), "PinY off the centre");
        // A pin that is the centre is what the reader gives anyway (carried, it stays).
        assert!(!drops(upright, &carried(&[("PinX", 1.0)])));
        // The place carried at the edge (0): the pin is the edge, 0, and the carried pin of the edge stays; the centre is not the pin then.
        assert!(!drops(upright, &carried(&[("PinX", 0.0), ("LocPinX", 0.0)])), "the pin at the left edge with the place 0");
        assert!(drops(upright, &carried(&[("PinX", 1.0), ("LocPinX", 0.0)])), "the pin at the centre with the place 0: another value is given");
        // The same on the other axis (the frame is 1 high, its centre at 0.5: the bottom edge is at 0).
        assert!(!drops(upright, &carried(&[("PinY", 0.0), ("LocPinY", 0.0)])), "the pin at the bottom edge with the place 0");
        assert!(drops(upright, &carried(&[("PinY", 0.5), ("LocPinY", 0.0)])), "the pin at the centre with the place 0");
        // A turn that is not the markup's (an unturned frame, a carried angle): the reader works nothing out, and the pin stays.
        assert!(!drops(upright, &carried(&[("PinX", 0.25), ("Angle", 0.5)])));
        // A turned frame, as the reader has it, works its pin out by the map of the turn.
        let quarter = std::f64::consts::FRAC_PI_2;
        let turned = Frame::TwoD { cx: 1.0, cy: 0.5, w: 2.0, h: 1.0, angle: quarter, flip: (false, false) };
        assert!(drops(turned, &carried(&[("PinX", 0.25)])));
        assert!(
            !drops(turned, &carried(&[("PinX", 1.0), ("PinY", -0.5), ("LocPinX", 0.0)])),
            "the pin at the left of the middle line, turned a quarter"
        );
    }
}
