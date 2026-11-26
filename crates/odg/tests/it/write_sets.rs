//! A new custom shape's outline in sets, and with arcs: a section that is not
//! filled, not stroked or not shown is written in a set with `F`, `S` or both, an arc of a circle or of an ellipse with its axes along the frame's as `A` or `W` on the
//! ellipse's box, and an ellipse as `U`. A shape whose outline came out in a form the reader gives back as its own rows carries no tier 2; any other carries its rows,
//! and the file made still draws what the model draws in a program that reads only the outline.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, SectionKey, Shape, Value};
use diacraft_odg::{Limits, read};
use proptest::prelude::*;

use crate::drawing::{Drawing, cm};
use crate::write_core::{custom, open, open_bytes, part, real};
use crate::write_new::{push_shape, rectangle};

/// A shape's frame in inches, and the cells of a row in the forms DiaCraft's basic shapes use: a share of the frame is the constant 0, the bare name or the name times the
/// share.
#[derive(Clone, Copy)]
struct Frame {
    w: f64,
    h: f64,
}

fn cell(formula: Option<&str>, value: f64) -> Cell {
    Cell::read(formula, Some(&value.to_string()), Some("IN"), None, &mut 1_000)
}

fn flag(on: bool) -> Cell {
    Cell::constant(Value::from_number(if on { 1.0 } else { 0.0 }).unwrap())
}

impl Frame {
    /// The cell of a share `k` of `Width` (`axis` `Width`) or `Height`.
    fn share(&self, axis: &str, k: f64) -> Cell {
        let size = if axis == "Width" { self.w } else { self.h };
        if k == 0.0 {
            cell(None, 0.0)
        } else if k == 1.0 {
            cell(Some(axis), size)
        } else {
            cell(Some(&format!("{axis}*{k}")), k * size)
        }
    }

    fn row(&self, kind: &str, ix: u32, cells: Vec<(&str, Cell)>) -> Arc<Row> {
        let mut r = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
        for (name, c) in cells {
            r.cells.insert(CellName::new(name), c);
        }
        Arc::new(r)
    }

    /// A `MoveTo`, `LineTo` of the shares (`kx`, `ky`).
    fn at(&self, kind: &str, ix: u32, (kx, ky): (f64, f64)) -> Arc<Row> {
        self.row(kind, ix, vec![("X", self.share("Width", kx)), ("Y", self.share("Height", ky))])
    }

    /// An `EllipticalArcTo` with `C` 0 to `to` through `via`, whose axes are `ratio` (the length along the frame's x over the one along its y).
    fn earc(&self, ix: u32, to: (f64, f64), via: (f64, f64), ratio: f64) -> Arc<Row> {
        self.row(
            "EllipticalArcTo",
            ix,
            vec![
                ("X", self.share("Width", to.0)),
                ("Y", self.share("Height", to.1)),
                ("A", self.share("Width", via.0)),
                ("B", self.share("Height", via.1)),
                ("C", cell(None, 0.0)),
                ("D", cell(None, ratio)),
            ],
        )
    }

    /// An `ArcTo` to `to` with the bow `bow` (inches).
    fn arc(&self, ix: u32, to: (f64, f64), bow: f64) -> Arc<Row> {
        self.row("ArcTo", ix, vec![("X", self.share("Width", to.0)), ("Y", self.share("Height", to.1)), ("A", cell(None, bow))])
    }

    /// An `Ellipse` row: the centre and a point on each axis.
    fn ellipse(&self, ix: u32, centre: (f64, f64), right: (f64, f64), top: (f64, f64)) -> Arc<Row> {
        self.row(
            "Ellipse",
            ix,
            vec![
                ("X", self.share("Width", centre.0)),
                ("Y", self.share("Height", centre.1)),
                ("A", self.share("Width", right.0)),
                ("B", self.share("Height", right.1)),
                ("C", self.share("Width", top.0)),
                ("D", self.share("Height", top.1)),
            ],
        )
    }

    /// A rectangle of the model's size whose `Geometry` is `sections`.
    fn shape(&self, id: u32, sections: Vec<Arc<Section>>) -> Shape {
        let mut shape = rectangle(id, "S", (2.0, 3.0, self.w * 2.54, self.h * 2.54));
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.sections.retain(|s| s.name.as_str() != "Geometry");
        for s in sections {
            sheet.sections.push(s);
        }
        shape
    }
}

/// A frame of 4 by 2 centimetres, the size of the rectangle.
fn frame() -> Frame {
    Frame { w: cm(4.0), h: cm(2.0) }
}

/// A `Geometry` section `ix` with the flags `flags` set to 1 (the others absent, as the reader makes them).
fn section(ix: u32, flags: &[&str], rows: Vec<Arc<Row>>) -> Arc<Section> {
    let mut s = Section::new("Geometry", Some(ix));
    for f in flags {
        s.cells.insert(CellName::new(f), flag(true));
    }
    for r in rows {
        s.rows.push(r);
    }
    Arc::new(s)
}

/// The closed square of the frame, from its lower left corner: a move and four lines, the last back to the start.
fn square(f: &Frame, ix: u32, flags: &[&str]) -> Arc<Section> {
    section(
        ix,
        flags,
        vec![
            f.at("MoveTo", 1, (0.0, 0.0)),
            f.at("LineTo", 2, (1.0, 0.0)),
            f.at("LineTo", 3, (1.0, 1.0)),
            f.at("LineTo", 4, (0.0, 1.0)),
            f.at("LineTo", 5, (0.0, 0.0)),
        ],
    )
}

/// The shape is saved as a new one on a page that holds one shape, and the file made is read again: the path written, the file, and the shape read back.
struct Saved {
    path: String,
    content: String,
    read: Shape,
}

fn save(shape_of: &dyn Fn(u32) -> Shape) -> Saved {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, shape_of(id));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    let path = content.split("draw:enhanced-path=\"").last().unwrap().split('"').next().unwrap().to_owned();
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().as_ref().clone();
    Saved { path, content, read }
}

fn geometry(shape: &Shape) -> Vec<Arc<Section>> {
    let mut v: Vec<Arc<Section>> = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").cloned().collect();
    v.sort_by_key(|s| s.ix);
    v
}

fn flag_of(s: &Section, name: &str) -> bool {
    s.cells.get(name).and_then(|c| c.value.as_ref()?.number()) == Some(1.0)
}

/// The path text a program that reads only the outline sees: the shape's frame with the path in a file with nothing of DiaCraft's in it, read, and the sections it gives.
fn plain_read(f: &Frame, path: &str) -> Vec<Arc<Section>> {
    let shape = format!(
        r#"<draw:custom-shape draw:name="p" svg:x="1cm" svg:y="1cm" svg:width="{w}cm" svg:height="{h}cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="{path}"/></draw:custom-shape>"#,
        w = f.w * 2.54,
        h = f.h * 2.54
    );
    let (doc, _) = Drawing::a4().page(&shape).read();
    geometry(doc.pages.get(0).unwrap().shapes.get(0).unwrap())
}

fn value(c: Option<&Cell>) -> f64 {
    c.and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

// ---- the sets --------------------------------------------------------------------------------------------------------------------------------------

/// A closed square (fill and line) and an open line with `NoFill` 1 are written as a set with neither flag and then a set with `F`; the file read back holds the same two
/// sections and no tier 2. An open run of a section that fills and strokes is an open subpath of the first set, with no `Z`, and reads back as the same three sides.
#[test]
fn v17_a_stroke_only_section_goes_in_a_set_with_f_after_the_filled_set() {
    let f = frame();
    let made = |id| {
        let line = section(1, &["NoFill"], vec![f.at("MoveTo", 1, (0.0, 0.5)), f.at("LineTo", 2, (1.0, 0.5))]);
        f.shape(id, vec![square(&f, 0, &[]), line])
    };
    let saved = save(&made);
    assert_eq!(saved.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N M 0 10800 L 21600 10800 F N", "{}", saved.content);
    assert!(!saved.content.contains(" diacraft:"), "the reader gives both sections back as they are: no tier 2: {}", saved.content);
    let back = geometry(&saved.read);
    assert_eq!(back.len(), 2);
    assert!(!flag_of(&back[0], "NoFill") && flag_of(&back[1], "NoFill") && !flag_of(&back[1], "NoLine") && !flag_of(&back[1], "NoShow"));
    assert_eq!((back[0].rows.len(), back[1].rows.len()), (5, 2));
    // The same outline read by a program that reads only the outline.
    let plain = plain_read(&f, &saved.path);
    assert_eq!(plain.len(), 2);
    assert!(!flag_of(&plain[0], "NoFill") && flag_of(&plain[1], "NoFill"));

    // An open three-sided run of fill and line: an open subpath of the first set, with no `Z`, and the reader leaves `NoFill` 0 on it.
    let open_run = |id| {
        let run = section(
            0,
            &[],
            vec![f.at("MoveTo", 1, (0.0, 0.0)), f.at("LineTo", 2, (1.0, 0.0)), f.at("LineTo", 3, (1.0, 1.0)), f.at("LineTo", 4, (0.0, 1.0))],
        );
        f.shape(id, vec![run])
    };
    let saved = save(&open_run);
    assert_eq!(saved.path, "M 0 21600 L 21600 21600 21600 0 0 0 N", "{}", saved.content);
    assert!(!saved.content.contains(" diacraft:"), "no tier 2: {}", saved.content);
    let back = geometry(&saved.read);
    assert_eq!(back.len(), 1);
    assert!(!flag_of(&back[0], "NoFill"), "an open run that fills is read as filled");
    assert_eq!(back[0].rows.len(), 4);
}

/// A section of two open runs (an inner move) that fills and strokes is one ring with `S` (its inner move written as a line, closed with `Z`) and then its runs with `F`;
/// the section is tier 2, and the file read again gives it back as it was.
#[test]
fn v18_a_section_of_two_open_runs_is_written_as_a_ring_with_s_and_runs_with_f() {
    let f = frame();
    let made = |id| {
        let rows = vec![
            f.at("MoveTo", 1, (0.0, 0.0)),
            f.at("LineTo", 2, (1.0, 0.0)),
            f.at("LineTo", 3, (1.0, 1.0)),
            f.at("MoveTo", 4, (0.0, 0.5)),
            f.at("LineTo", 5, (0.5, 0.5)),
        ];
        f.shape(id, vec![section(0, &[], rows)])
    };
    let saved = save(&made);
    assert_eq!(
        saved.path, "M 0 21600 L 21600 21600 21600 0 L 0 10800 L 10800 10800 Z S N M 0 21600 L 21600 21600 21600 0 M 0 10800 L 10800 10800 F N",
        "{}",
        saved.content
    );
    assert!(saved.content.contains(" diacraft:"), "the section is tier 2: {}", saved.content);
    let back = geometry(&saved.read);
    assert_eq!(back.len(), 1, "the section comes back whole, as the model had it");
    let kinds: Vec<_> = back[0].rows.iter().map(|r| r.row_type.as_deref().unwrap_or_default().to_owned()).collect();
    assert_eq!(kinds, ["MoveTo", "LineTo", "LineTo", "MoveTo", "LineTo"]);
    // What a program that reads only the outline gets: the ring that fills and the two runs that stroke.
    let plain = plain_read(&f, &saved.path);
    assert_eq!(plain.len(), 3);
    assert!(flag_of(&plain[0], "NoLine") && !flag_of(&plain[0], "NoFill"), "the ring fills and is not stroked");
    assert!(flag_of(&plain[1], "NoFill") && flag_of(&plain[2], "NoFill"), "the runs stroke and do not fill");
}

/// A section with `NoLine` is a set with `S`, and one with `NoShow` a set with `F` and `S`; both read back as they were, with no tier 2. A section that has neither fill nor
/// line but is shown (`NoFill` and `NoLine` both) draws nothing and is written like a hidden one, which the reader gives back as `NoShow`: its flags are tier 2.
#[test]
fn a_no_line_section_is_a_set_with_s_and_a_hidden_section_a_set_with_f_and_s() {
    let f = frame();
    let no_line = save(&|id| f.shape(id, vec![square(&f, 0, &["NoLine"])]));
    assert_eq!(no_line.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z S N", "{}", no_line.content);
    assert!(!no_line.content.contains(" diacraft:"), "{}", no_line.content);
    let back = geometry(&no_line.read);
    assert!(flag_of(&back[0], "NoLine") && !flag_of(&back[0], "NoFill") && !flag_of(&back[0], "NoShow"));

    let hidden = save(&|id| {
        let line = section(1, &["NoShow"], vec![f.at("MoveTo", 1, (0.0, 0.5)), f.at("LineTo", 2, (1.0, 0.5))]);
        f.shape(id, vec![square(&f, 0, &[]), line])
    });
    assert_eq!(hidden.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N M 0 10800 L 21600 10800 F S N", "{}", hidden.content);
    assert!(!hidden.content.contains(" diacraft:"), "{}", hidden.content);
    let back = geometry(&hidden.read);
    assert!(flag_of(&back[1], "NoShow") && !flag_of(&back[1], "NoFill") && !flag_of(&back[1], "NoLine"));

    let nothing = save(&|id| f.shape(id, vec![square(&f, 0, &["NoFill", "NoLine"])]));
    assert_eq!(nothing.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z F S N", "{}", nothing.content);
    assert!(nothing.content.contains(" diacraft:"), "NoFill and NoLine read back as NoShow: the flags are carried: {}", nothing.content);
    let back = geometry(&nothing.read);
    assert!(flag_of(&back[0], "NoFill") && flag_of(&back[0], "NoLine") && !flag_of(&back[0], "NoShow"), "the model's flags come back");
}

/// Two sections that fill, one of them with its line (`NoLine` 0) and the other not (`NoLine` 1), no longer match as two sets: one ring set holds both and one set strokes the
/// line of the first; the sections are tier 2.
#[test]
fn a_fill_only_and_a_fill_and_stroke_section_are_one_ring_set_and_one_set_of_strokes() {
    let f = frame();
    let saved = save(&|id| {
        let inner = section(
            1,
            &["NoLine"],
            vec![f.at("MoveTo", 1, (0.25, 0.25)), f.at("LineTo", 2, (0.75, 0.25)), f.at("LineTo", 3, (0.75, 0.75)), f.at("LineTo", 4, (0.25, 0.25))],
        );
        f.shape(id, vec![square(&f, 0, &[]), inner])
    });
    assert_eq!(
        saved.path,
        "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z M 5400 16200 L 16200 16200 16200 5400 5400 16200 Z S N M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z F N",
        "{}",
        saved.content
    );
    assert!(saved.content.contains(" diacraft:"), "{}", saved.content);
    let back = geometry(&saved.read);
    assert_eq!(back.len(), 2);
    assert!(!flag_of(&back[0], "NoLine") && flag_of(&back[1], "NoLine"), "the model's sections come back");
}

/// A section whose runs are all closed is written as a subpath for each (the single or closed), and reads back as a section for each: it is tier 2.
#[test]
fn a_section_of_two_closed_runs_is_two_subpaths_of_one_set() {
    let f = frame();
    let saved = save(&|id| {
        let rows = vec![
            f.at("MoveTo", 1, (0.0, 0.0)),
            f.at("LineTo", 2, (0.5, 0.0)),
            f.at("LineTo", 3, (0.5, 0.5)),
            f.at("LineTo", 4, (0.0, 0.0)),
            f.at("MoveTo", 5, (0.5, 0.5)),
            f.at("LineTo", 6, (1.0, 0.5)),
            f.at("LineTo", 7, (1.0, 1.0)),
            f.at("LineTo", 8, (0.5, 0.5)),
        ];
        f.shape(id, vec![section(0, &[], rows)])
    });
    assert_eq!(saved.path, "M 0 21600 L 10800 21600 10800 10800 0 21600 Z M 10800 10800 L 21600 10800 21600 0 10800 10800 Z N", "{}", saved.content);
    let back = geometry(&saved.read);
    assert_eq!(back.len(), 1, "the model's one section comes back, not the two the file holds: {}", saved.content);
    assert_eq!(back[0].rows.len(), 8);
}

// ---- the arcs and the ellipse ---------------------------------------------------------------------------------------------------------------------

/// An `ArcTo` from (0, 0.5 cm) to (2 cm, 0.5 cm) with the bow of -0.5 cm on a 2 by 1 cm frame is `W` on the circle's box with the two ends as rays; a bow of +0.5 cm
/// is the other arc, as `A`, and a bow of 0 a straight line.
#[test]
fn v21_an_arc_to_is_a_or_w_on_the_circles_box_with_the_two_ends_as_rays() {
    let f = Frame { w: cm(2.0), h: cm(1.0) };
    let with_bow = |bow_cm: f64| {
        save(&|id| {
            let run = section(0, &["NoFill"], vec![f.at("MoveTo", 1, (0.0, 0.5)), f.arc(2, (1.0, 0.5), cm(bow_cm))]);
            f.shape(id, vec![run])
        })
    };
    let down = with_bow(-0.5);
    assert_eq!(down.path, "M 0 10800 W -2700 0 24300 54000 0 10800 21600 10800 F N", "{}", down.content);
    let up = with_bow(0.5);
    assert_eq!(up.path, "M 0 10800 A -2700 -32400 24300 21600 0 10800 21600 10800 F N", "{}", up.content);
    let line = with_bow(0.0);
    assert_eq!(line.path, "M 0 10800 L 21600 10800 F N", "{}", line.content);
    // The arc is carried by DiaCraft's data and comes back as the row it was.
    let back = geometry(&down.read);
    assert_eq!(back[0].rows.get(1).and_then(|r| r.row_type.as_deref()), Some("ArcTo"));
    assert!((value(back[0].rows.get(1).unwrap().cells.get("A")) - cm(-0.5)).abs() < 1e-9);
    // A program that reads only the outline: the arc is the upper one, 0.5 cm above the chord at its middle.
    let plain = plain_read(&f, &down.path);
    let arc = plain[0].rows.get(1).unwrap();
    assert_eq!(arc.row_type.as_deref(), Some("EllipticalArcTo"));
    let (mid_x, mid_y) = (value(arc.cells.get("A")), value(arc.cells.get("B")));
    assert!((mid_x - cm(1.0)).abs() < 1e-6 && (mid_y - (cm(0.5) + cm(0.5))).abs() < 1e-6, "the middle of the arc is at ({mid_x}, {mid_y})");
}

/// An `EllipticalArcTo` with `C` 0 is `A` or `W` on the ellipse's box, by the way it goes, and reads back drawing the same: the arc of the file ends where the row ends,
/// passes through the row's middle point, and holds the row's ratio of axes.
#[test]
fn an_elliptical_arc_with_c_0_is_written_as_a_or_w_on_the_ellipses_box_and_reads_back_drawing_the_same() {
    let f = frame();
    // A half circle of 2 cm about the middle of the bottom edge, from the left to the right over the top (clockwise as seen), closed by the bottom edge.
    let circle = save(&|id| {
        let run = section(0, &[], vec![f.at("MoveTo", 1, (0.0, 0.0)), f.earc(2, (1.0, 0.0), (0.5, 1.0), 1.0), f.at("LineTo", 3, (0.0, 0.0))]);
        f.shape(id, vec![run])
    });
    assert_eq!(circle.path, "M 0 21600 W 0 0 21600 43200 0 21600 21600 21600 L 0 21600 Z N", "{}", circle.content);
    assert!(circle.content.contains(" diacraft:"), "the row comes back with another form: it is carried: {}", circle.content);
    let back = geometry(&circle.read);
    assert_eq!(back[0].rows.get(1).and_then(|r| r.row_type.as_deref()), Some("EllipticalArcTo"));
    assert_eq!(back[0].rows.get(1).unwrap().cells.get("D").and_then(|c| c.formula.text()), None, "the model's own cell comes back");
    // A flat half ellipse (axes 2 cm by 1 cm: the ratio is 2), clockwise, and the same one back the other way, counterclockwise (`A`).
    for (from, to, command, box_and_rays) in [
        ((0.0, 0.0), (1.0, 0.0), "W", "0 10800 21600 32400 0 21600 21600 21600"),
        ((1.0, 0.0), (0.0, 0.0), "A", "0 10800 21600 32400 21600 21600 0 21600"),
    ] {
        let flat = save(&|id| {
            let run = section(0, &["NoFill"], vec![f.at("MoveTo", 1, from), f.earc(2, to, (0.5, 0.5), 2.0)]);
            f.shape(id, vec![run])
        });
        let start = format!("M {} {}", from.0 * 21600.0, (1.0 - from.1) * 21600.0);
        assert_eq!(flat.path, format!("{start} {command} {box_and_rays} F N"), "{}", flat.content);
        // Drawing the same: the arc read from the outline alone ends at the row's end, passes through its middle point and has its ratio.
        let plain = plain_read(&f, &flat.path);
        let arc = plain[0].rows.get(1).unwrap();
        assert_eq!(arc.row_type.as_deref(), Some("EllipticalArcTo"));
        let near = |name: &str, want: f64| {
            let got = value(arc.cells.get(name));
            assert!((got - want).abs() < 1e-6, "{name}: {got} against {want}");
        };
        near("X", to.0 * f.w);
        near("Y", to.1 * f.h);
        near("A", 0.5 * f.w);
        near("B", 0.5 * f.h);
        near("D", 2.0);
    }
}

/// An `EllipticalArcTo` whose three points are on a line is its chord, and one with a `C` that is not 0 or a ratio that is not above 0 is not written (the shape keeps a
/// rectangle for other programs and carries its rows).
#[test]
fn an_arc_that_is_a_line_is_its_chord_and_an_arc_with_a_turned_axis_stays_a_rectangle_for_other_programs() {
    let f = frame();
    let chord = save(&|id| {
        let run = section(0, &["NoFill"], vec![f.at("MoveTo", 1, (0.0, 0.0)), f.earc(2, (1.0, 1.0), (0.5, 0.5), 1.0)]);
        f.shape(id, vec![run])
    });
    assert_eq!(chord.path, "M 0 21600 L 21600 0 F N", "{}", chord.content);
    for (c, d) in [(0.5, 1.0), (0.0, 0.0), (0.0, -1.0)] {
        let saved = save(&|id| {
            let mut arc = f.earc(2, (1.0, 0.0), (0.5, 1.0), d);
            let row = Arc::make_mut(&mut arc);
            row.cells.insert(CellName::new("C"), cell(None, c));
            f.shape(id, vec![section(0, &[], vec![f.at("MoveTo", 1, (0.0, 0.0)), arc])])
        });
        assert_eq!(saved.path, "M 0 0 L 21600 0 21600 21600 0 21600 Z N", "C {c}, D {d}: the placeholder rectangle: {}", saved.content);
        let back = geometry(&saved.read);
        assert_eq!(back[0].rows.get(1).and_then(|r| r.row_type.as_deref()), Some("EllipticalArcTo"), "C {c}, D {d}: the row is carried");
    }
}

/// An `Ellipse` row with its axes along the frame's is `U` from 0 to 360 degrees whatever its size and place, the point on the right to the left of the centre as well; one with
/// an axis turned is not written.
#[test]
fn an_ellipse_row_with_axes_along_x_and_y_is_written_as_u() {
    let f = frame();
    let one = |centre, right, top| {
        save(&|id| {
            let s = section(0, &[], vec![f.ellipse(1, centre, right, top)]);
            f.shape(id, vec![s])
        })
    };
    // The ellipse of the whole frame, in DiaCraft's form, and a smaller one off the middle with its points on the other sides of its centre.
    let whole = one((0.5, 0.5), (1.0, 0.5), (0.5, 1.0));
    assert_eq!(whole.path, "U 10800 10800 10800 10800 0 360 N", "{}", whole.content);
    assert!(!whole.content.contains(" diacraft:"), "DiaCraft's own ellipse reads back as it is: {}", whole.content);
    let small = one((0.25, 0.75), (0.0, 0.75), (0.25, 0.5));
    assert_eq!(small.path, "U 5400 5400 5400 5400 0 360 N", "{}", small.content);
    assert!(small.content.contains(" diacraft:"), "the row's other form is carried: {}", small.content);
    let back = geometry(&small.read);
    assert_eq!(back[0].rows.first().and_then(|r| r.row_type.as_deref()), Some("Ellipse"));
    let plain = plain_read(&f, &small.path);
    let row = plain[0].rows.first().unwrap();
    assert_eq!(row.row_type.as_deref(), Some("Ellipse"));
    assert!((value(row.cells.get("X")) - 0.25 * f.w).abs() < 1e-6 && (value(row.cells.get("Y")) - 0.75 * f.h).abs() < 1e-6);
    // An ellipse of the whole frame that is a stroke only goes in a set with `F`.
    let outline = save(&|id| f.shape(id, vec![section(0, &["NoFill"], vec![f.ellipse(1, (0.5, 0.5), (1.0, 0.5), (0.5, 1.0))])]));
    assert_eq!(outline.path, "U 10800 10800 10800 10800 0 360 F N", "{}", outline.content);
    // An axis that is turned (the point on the right is above the centre) is not written.
    let turned = one((0.5, 0.5), (1.0, 0.75), (0.5, 1.0));
    assert_eq!(turned.path, "M 0 0 L 21600 0 21600 21600 0 21600 Z N", "{}", turned.content);
}

/// A coordinate whose formula names another cell or a function is written as its value at the written size, and the rows are carried; one whose formula is the shape's own
/// size and a length (`Width*0.5+0.25`) stays refused, as before (the equation of R33 is a later version's).
#[test]
fn a_coordinate_that_names_another_cell_is_written_as_its_value_and_one_that_is_an_equation_form_stays_refused() {
    let f = frame();
    let corner = |formula: &str, value: f64| {
        save(&|id| {
            let mut r = f.at("LineTo", 2, (1.0, 0.0));
            Arc::make_mut(&mut r).cells.insert(CellName::new("X"), cell(Some(formula), value));
            f.shape(id, vec![section(0, &[], vec![f.at("MoveTo", 1, (0.0, 0.0)), r])])
        })
    };
    // 0.1 of the shorter side (the height), a quarter of the width less that.
    let h = f.h;
    let min = corner("MIN(Width,Height)*0.1", h * 0.1);
    assert_eq!(min.path, format!("M 0 21600 L {} 21600 N", (h * 0.1 / f.w * 21600.0 * 1e6).round() / 1e6), "{}", min.content);
    let both = corner("Width*0.5+Height*0.25", f.w * 0.5 + h * 0.25);
    assert!(both.path.starts_with("M 0 21600 L "), "{}", both.path);
    let affine = corner("Width*0.5+0.25", f.w * 0.5 + 0.25);
    assert_eq!(affine.path, "M 0 0 L 21600 0 21600 21600 0 21600 Z N", "refused as before: {}", affine.content);
}

// ---- hostile and random input ----------------------------------------------------------------------------------------------------------------------------

/// A shape whose outline would be more subpaths than a file's reader reads (it makes a section of each subpath and stops a little short of the sheet's limit) is not written
/// with them: the outline is refused in words, the shape keeps a rectangle for other programs and its rows as tier 2, and the file made reads again.
#[test]
fn an_outline_of_more_subpaths_than_the_reader_reads_is_refused_in_words_and_keeps_its_rows() {
    let f = frame();
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    // As many sections as a sheet holds beside its connection section, each a closed run: a subpath for each.
    let sections: Vec<Arc<Section>> = (0..diacraft_doc::MAX_SECTIONS_PER_SHEET - 2)
        .map(|k| {
            let rows = vec![f.at("MoveTo", 1, (0.0, 0.0)), f.at("LineTo", 2, (0.5, 0.0)), f.at("LineTo", 3, (0.0, 0.0))];
            section(u32::try_from(k).unwrap(), &[], rows)
        })
        .collect();
    push_shape(&mut o.doc, f.shape(id, sections));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("M 0 0 L 21600 0 21600 21600 0 21600 Z N"), "the placeholder rectangle stands in");
    assert!(saved.notes.iter().any(|n| n.class() == "outline_approximated"), "{:?}", saved.notes);
    assert!(read(&saved.bytes, &Limits::default()).is_ok(), "the file made is read again");
}

/// One random section: its rows (a move, then lines, arcs of both kinds and sometimes another move), its flags, or a whole ellipse.
#[derive(Clone, Debug)]
enum Piece {
    Line(u8, u8),
    Arc(u8, u8, i8),
    Half(u8, u8, u8, u8),
    Move(u8, u8),
}

#[derive(Clone, Debug)]
struct Random {
    flags: (bool, bool, bool),
    ellipse: Option<(u8, u8, u8, u8)>,
    start: (u8, u8),
    pieces: Vec<Piece>,
}

fn share_of(n: u8) -> f64 {
    f64::from(n) / 8.0
}

fn pieces() -> impl Strategy<Value = Piece> {
    let n = 0..=8u8;
    prop_oneof![
        4 => (n.clone(), n.clone()).prop_map(|(x, y)| Piece::Line(x, y)),
        2 => (n.clone(), n.clone(), -6..=6i8).prop_map(|(x, y, b)| Piece::Arc(x, y, b)),
        2 => (n.clone(), n.clone(), n.clone(), n.clone()).prop_map(|(x, y, a, b)| Piece::Half(x, y, a, b)),
        1 => (n.clone(), n.clone()).prop_map(|(x, y)| Piece::Move(x, y)),
    ]
}

fn randoms() -> impl Strategy<Value = Random> {
    let n = 0..=8u8;
    (
        (any::<bool>(), any::<bool>(), any::<bool>()),
        proptest::option::weighted(0.15, (1..=4u8, 1..=4u8, 1..=3u8, 1..=3u8)),
        (n.clone(), n),
        proptest::collection::vec(pieces(), 0..=31),
    )
        .prop_map(|(flags, ellipse, start, pieces)| Random { flags, ellipse, start, pieces })
}

fn random_section(f: &Frame, ix: u32, r: &Random) -> Arc<Section> {
    let mut flags = Vec::new();
    for (on, name) in [(r.flags.0, "NoFill"), (r.flags.1, "NoLine"), (r.flags.2, "NoShow")] {
        if on {
            flags.push(name);
        }
    }
    if let Some((cx, cy, rx, ry)) = r.ellipse {
        // The ellipse about (cx, cy) quarters, with half-axes of rx and ry eighths, kept in the frame.
        let (cx, cy) = (f64::from(cx) / 4.0, f64::from(cy) / 4.0);
        let (rx, ry) = (f64::from(rx) / 8.0, f64::from(ry) / 8.0);
        return section(ix, &flags, vec![f.ellipse(1, (cx, cy), (cx + rx, cy), (cx, cy + ry))]);
    }
    let mut rows = vec![f.at("MoveTo", 1, (share_of(r.start.0), share_of(r.start.1)))];
    for (i, p) in r.pieces.iter().enumerate() {
        let k = u32::try_from(i + 2).unwrap();
        rows.push(match *p {
            Piece::Line(x, y) => f.at("LineTo", k, (share_of(x), share_of(y))),
            Piece::Move(x, y) => f.at("MoveTo", k, (share_of(x), share_of(y))),
            // A bow of up to three quarters of the frame's height.
            Piece::Arc(x, y, b) => f.arc(k, (share_of(x), share_of(y)), f64::from(b) / 8.0 * f.h),
            Piece::Half(x, y, a, b) => f.earc(k, (share_of(x), share_of(y)), (share_of(a), share_of(b)), 1.0 + f64::from(a) / 8.0),
        });
    }
    section(ix, &flags, rows)
}

/// The sampled ink a list of sections puts down in the frame's own inches: the polylines of the sections that stroke (a run each) and the rings of the sections that fill (the
/// runs joined, as the VSDX application joins them), each as points. An arc is sampled from its pen, its middle point and its end (a circle for an `ArcTo` by its bow, an
/// ellipse with its axes along the frame's and its ratio for an `EllipticalArcTo`).
struct Ink {
    strokes: Vec<Vec<(f64, f64)>>,
    rings: Vec<Vec<(f64, f64)>>,
}

/// The points a whole ellipse is sampled at: the chord of a step is within 2E-3 inches (`EPS`) of an ellipse of up to 1.6 inches across. An arc is sampled by the angle it turns.
const SAMPLES: u32 = 192;

/// How far two sampled curves of one drawing may be apart, in inches.
const EPS: f64 = 2e-3;

fn arc_points(pen: (f64, f64), row: &Row) -> Vec<(f64, f64)> {
    let at = |a: &str, b: &str| (value(row.cells.get(a)), value(row.cells.get(b)));
    let end = at("X", "Y");
    let circle = |p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), d: f64| -> Option<(f64, f64, f64, bool)> {
        let (p0, p1, p2) = ((p0.0 / d, p0.1), (p1.0 / d, p1.1), (p2.0 / d, p2.1));
        let (bx, by, cx, cy) = (p1.0 - p0.0, p1.1 - p0.1, p2.0 - p0.0, p2.1 - p0.1);
        let det = 2.0 * (bx * cy - by * cx);
        if !det.is_finite() || det.abs() <= 1e-9 * bx.hypot(by) * cx.hypot(cy) {
            return None;
        }
        let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
        let (ux, uy) = ((cy * b2 - by * c2) / det, (bx * c2 - cx * b2) / det);
        Some((p0.0 + ux, p0.1 + uy, ux.hypot(uy), det > 0.0))
    };
    let (via, d) = if row.row_type.as_deref() == Some("ArcTo") {
        // The middle of the arc: the chord's middle moved by the bow to the right of the way.
        let bow = value(row.cells.get("A"));
        let (dx, dy) = (end.0 - pen.0, end.1 - pen.1);
        let c = dx.hypot(dy);
        if c < 1e-12 || bow.abs() < 1e-9 * c {
            return vec![pen, end];
        }
        ((pen.0 + dx / 2.0 + bow * dy / c, pen.1 + dy / 2.0 - bow * dx / c), 1.0)
    } else {
        (at("A", "B"), value(row.cells.get("D")))
    };
    let Some((cx, cy, r, ccw)) = circle(pen, via, end, d) else { return vec![pen, end] };
    let angle = |p: (f64, f64)| (p.1 - cy).atan2(p.0 / d - cx);
    let (a0, a1) = (angle(pen), angle(end));
    let two_pi = std::f64::consts::TAU;
    let sweep = if ccw { (a1 - a0).rem_euclid(two_pi) } else { -((a0 - a1).rem_euclid(two_pi)) };
    let mut points = Vec::new();
    // A step of a hundredth of a radian: its chord is within 1.25E-5 of the radius from the arc.
    let steps = ((sweep.abs() / 0.01).ceil() as u32).clamp(2, 4_000);
    for k in 0..=steps {
        let a = a0 + sweep * f64::from(k) / f64::from(steps);
        points.push(((cx + r * a.cos()) * d, cy + r * a.sin()));
    }
    points
}

fn ink(sections: &[Arc<Section>]) -> Ink {
    let mut out = Ink { strokes: Vec::new(), rings: Vec::new() };
    for s in sections {
        let no_fill = flag_of(s, "NoFill");
        let no_line = flag_of(s, "NoLine");
        let hidden = flag_of(s, "NoShow") || (no_fill && no_line);
        if hidden {
            continue;
        }
        let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut pen = (0.0, 0.0);
        for row in s.rows.iter() {
            let kind = row.row_type.as_deref().unwrap_or_default();
            let end = (value(row.cells.get("X")), value(row.cells.get("Y")));
            match kind {
                "MoveTo" => runs.push(vec![end]),
                "LineTo" => runs.last_mut().unwrap().push(end),
                "ArcTo" | "EllipticalArcTo" => runs.last_mut().unwrap().extend(arc_points(pen, row)),
                "Ellipse" => {
                    let c = end;
                    let (rx, ry) = ((value(row.cells.get("A")) - c.0).abs(), (value(row.cells.get("D")) - c.1).abs());
                    runs.push(
                        (0..=SAMPLES)
                            .map(|k| {
                                let a = f64::from(k) / f64::from(SAMPLES) * std::f64::consts::TAU;
                                (c.0 + rx * a.cos(), c.1 + ry * a.sin())
                            })
                            .collect(),
                    );
                }
                other => panic!("a row of kind {other}"),
            }
            pen = if kind == "Ellipse" { pen } else { end };
        }
        if !no_line {
            out.strokes.extend(runs.iter().cloned());
        }
        if !no_fill {
            out.rings.push(runs.into_iter().flatten().collect());
        }
    }
    out
}

/// Whether two polylines are the same curve within `eps` inches: the points of one are each near the other's.
fn near_curve(a: &[(f64, f64)], b: &[(f64, f64)], eps: f64) -> bool {
    let on = |p: (f64, f64), line: &[(f64, f64)]| {
        line.windows(2).any(|w| {
            let (x, y) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let len2 = x * x + y * y;
            let t = if len2 == 0.0 { 0.0 } else { (((p.0 - w[0].0) * x + (p.1 - w[0].1) * y) / len2).clamp(0.0, 1.0) };
            (p.0 - (w[0].0 + t * x)).hypot(p.1 - (w[0].1 + t * y)) <= eps
        }) || line.iter().any(|q| (p.0 - q.0).hypot(p.1 - q.1) <= eps)
    };
    a.iter().all(|p| on(*p, b)) && b.iter().all(|p| on(*p, a))
}

/// Whether `p` is filled by the odd-crossings rule of the rings together.
fn filled(rings: &[Vec<(f64, f64)>], p: (f64, f64)) -> bool {
    let mut crossings = 0usize;
    for ring in rings {
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                crossings += 1;
            }
        }
    }
    crossings % 2 == 1
}

/// How far a sample point is from the nearest edge of the rings (the points near an edge are not judged: two sampled curves differ there).
fn from_edges(rings: &[Vec<(f64, f64)>], p: (f64, f64)) -> f64 {
    let mut best = f64::MAX;
    for ring in rings {
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            let (x, y) = (b.0 - a.0, b.1 - a.1);
            let len2 = x * x + y * y;
            let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * x + (p.1 - a.1) * y) / len2).clamp(0.0, 1.0) };
            best = best.min((p.0 - (a.0 + t * x)).hypot(p.1 - (a.1 + t * y)));
        }
    }
    best
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 200, ..ProptestConfig::default() })]

    /// Random sections (fills, strokes, hidden, lines and arcs; at most 16 sections of 32 rows) are written as a new shape, saved (the save reads the file back and compares what
    /// is carried, within the rebuild test's tolerance) and read again: the shape has the sections it had. The outline alone, read by a program that does not read DiaCraft's
    /// data, puts down the same ink: every stroked run is a curve of the model's, and a point away from every edge is filled when the model fills it.
    #[test]
    fn random_sections_are_written_read_back_and_draw_the_same(sections in proptest::collection::vec(randoms(), 1..=16)) {
        let f = frame();
        let made: Vec<Arc<Section>> = sections.iter().enumerate().map(|(i, r)| random_section(&f, u32::try_from(i).unwrap(), r)).collect();
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, f.shape(id, made.clone()));
        let saved = match real(&o.doc, &o.bytes, &o.index) {
            Ok(saved) => saved,
            Err(e) => return Err(TestCaseError::fail(format!("not saved: {e}"))),
        };
        let content = part(&saved.bytes, "content.xml");
        let path = content.split("draw:enhanced-path=\"").last().unwrap().split('"').next().unwrap().to_owned();
        let back = open_bytes(saved.bytes.clone());
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        // The model comes back: the same sections, row by row, within the writer's tolerance.
        let got = geometry(&read);
        prop_assert_eq!(got.len(), made.len(), "{}", content);
        for (a, b) in made.iter().zip(got.iter()) {
            prop_assert_eq!(a.rows.len(), b.rows.len());
            for (x, y) in a.rows.iter().zip(b.rows.iter()) {
                prop_assert_eq!(x.row_type.as_deref(), y.row_type.as_deref());
                for (name, cx) in x.cells.iter() {
                    let (u, v) = (value(Some(cx)), value(y.cells.get(name.as_str())));
                    prop_assert!((u - v).abs() < 1e-6, "{} {}: {} against {}", name.as_str(), content, u, v);
                }
            }
            for name in ["NoFill", "NoLine", "NoShow"] {
                prop_assert_eq!(flag_of(a, name), flag_of(b, name), "{}", name);
            }
        }
        // Every section is in a form the writer writes: the outline is the shape's own, and not the stand-in rectangle written for an outline it cannot write.
        prop_assert!(path != "M 0 0 L 21600 0 21600 21600 0 21600 Z N", "the outline is the stand-in rectangle: {}", content);
        {
            let plain = plain_read(&f, &path);
            let (want, have) = (ink(&made), ink(&plain));
            for stroke in &want.strokes {
                prop_assert!(have.strokes.iter().any(|s| near_curve(stroke, s, EPS)), "a stroke is missing from the outline {}", path);
            }
            for stroke in &have.strokes {
                prop_assert!(want.strokes.iter().any(|s| near_curve(stroke, s, EPS)), "a stroke is added in the outline {}", path);
            }
            for i in 0..12 {
                for j in 0..8 {
                    let p = ((f64::from(i) + 0.37) / 12.0 * f.w, (f64::from(j) + 0.29) / 8.0 * f.h);
                    if from_edges(&want.rings, p).min(from_edges(&have.rings, p)) > 2.0 * EPS {
                        prop_assert_eq!(filled(&want.rings, p), filled(&have.rings, p), "the point {:?} of the outline {}", p, path);
                    }
                }
            }
        }
        // The section of the model with the key the reader would give it.
        prop_assert!(read.sheet.section(&SectionKey::new("Geometry", Some(0))).is_some());
    }
}
