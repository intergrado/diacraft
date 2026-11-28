//! The inside of a hollow line end in a PDF file (the owner's ruling of 2026-11-26): the marks are paths, and the inside of a hollow mark is one more path filled with the
//! shape's fill, under the outline. The pages are 4 by 4 inches at 72 points to the inch, so paper inch `(x, y)` (y up) is at point `(72 x, 288 - 72 y)`.

use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A horizontal line 3 inches long from (0.5, 2) to (3.5, 2) as a connector is (`NoFill` on its geometry), with a red solid fill, a black line 0.02 inch heavy, and the end `number` at
/// size class 6 (the unit `S` is 0.27 inch).
fn connector(number: i64) -> ShapeBuilder {
    let line = geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])]);
    line.set(diacraft_doc::CellAddr::in_section("Geometry", Some(0), "NoFill"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")))
        .cell("FillPattern", "1")
        .cell("FillForegnd", "#FF0000")
        .cell("LineWeight", "0.02")
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#000000")
        .cell("EndArrow", &number.to_string())
        .cell("EndArrowSize", "6")
}

#[test]
fn the_inside_of_a_hollow_end_is_a_path_in_the_fill_colour_inside_the_mark_and_not_stroked() {
    let (_, seen) = export(&one_page(vec![connector(21)]), &plain());
    let inside = seen.with_colour([255, 0, 0]);
    assert_eq!(inside.len(), 1, "one closed part, one fill");
    // The square 21 is centred on the end (3.5, 2): point (252, 144), 0.75 S = 0.2025 inch = 14.58 points each way.
    let b = inside[0].bbox;
    assert!(close(b.x0, 252.0 - 14.58) && close(b.x1, 252.0 + 14.58) && close(b.y0, 144.0 - 14.58) && close(b.y1, 144.0 + 14.58), "{b:?}");
    assert!(!inside[0].stroked && inside[0].even_odd, "filled as the shape's own fill is");
    // Its outline is a stroked path of the line's colour over the same box, and it is written after the inside.
    let black: Vec<usize> =
        seen.paths.iter().enumerate().filter(|(_, p)| p.stroked && p.rgba.is_some_and(|c| c[..3] == [0, 0, 0])).map(|(k, _)| k).collect();
    let inside_at = seen.paths.iter().position(|p| p.rgba.is_some_and(|c| c[..3] == [255, 0, 0])).expect("the inside is written");
    let outline_at = black
        .iter()
        .copied()
        .find(|k| {
            let o = seen.paths[*k].bbox;
            close(o.x0, b.x0) && close(o.x1, b.x1)
        })
        .expect("the mark's outline is written over the same box");
    assert!(inside_at < outline_at, "the fill is under the outline");
}

#[test]
fn the_fills_transparency_is_the_insides() {
    let (_, seen) = export(&one_page(vec![connector(21).cell("FillForegndTrans", "0.5")]), &plain());
    let inside = seen.with_colour([255, 0, 0]);
    assert_eq!(inside.len(), 1);
    let alpha = inside[0].rgba.expect("a colour")[3];
    assert!((126..=129).contains(&alpha), "{alpha}");
}

#[test]
fn a_shape_with_no_fill_and_the_solid_and_open_marks_write_no_inside() {
    for (what, shape) in [
        ("no fill", connector(21).cell("FillPattern", "0")),
        ("a solid mark", connector(13)),
        ("an open mark", connector(3)),
        ("no mark", connector(0)),
    ] {
        let (_, seen) = export(&one_page(vec![shape]), &plain());
        assert!(seen.with_colour([255, 0, 0]).is_empty(), "{what}: nothing is painted in the fill colour");
    }
}
