//! The inside of a hollow line end in an SVG file (the owner's ruling of 2026-11-26): the marks are paths, as they are on the screen (the writer has no `<marker>`
//! elements, so there is no marker to reuse), and the inside of a hollow mark is one more path painted with the shape's fill, written **before** the outline of the mark. The pages
//! here are 4 by 4 inches at 96 pixels to the inch, so paper inch `(x, y)` (y up) is at pixel `(96 x, 384 - 96 y)`.

use diacraft_opc::xml::El;
use diacraft_svg::SvgOptions;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A horizontal line 3 inches long from (0.5, `y`) to (3.5, `y`) as a connector is (`NoFill` on its geometry), a red solid fill, a black line `weight` inches heavy, and the end `number`
/// at size class 6, where the unit `S` is 0.25 inch and the weight: for 0.02 it is 0.27 inch.
fn connector(id: u32, y: f64, weight: f64, number: i64) -> ShapeBuilder {
    let line = geometry_of(shape_at(id, 0.5, y, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])]);
    geom_flag(line, "NoFill", "1")
        .cell("FillPattern", "1")
        .cell("FillForegnd", "#FF0000")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#000000")
        .cell("EndArrow", &number.to_string())
        .cell("EndArrowSize", "6")
}

fn defaults() -> SvgOptions {
    SvgOptions::default()
}

/// The paths painted with `fill`, in order.
fn painted<'a>(root: &'a El, fill: &str) -> Vec<&'a El> {
    named(root, "path").into_iter().filter(|p| p.attr("fill") == Some(fill)).collect()
}

/// The position in the file's order of the first path that satisfies `which`.
fn position(root: &El, which: impl Fn(&El) -> bool) -> Option<usize> {
    named(root, "path").iter().position(|p| which(p))
}

#[test]
fn the_inside_of_a_hollow_end_is_a_path_in_the_fill_colour_inside_the_mark_written_before_its_outline() {
    let (_, root) = export(&one_page(vec![connector(1, 2.0, 0.02, 21)]), &defaults());
    let inside = painted(&root, "#ff0000");
    assert_eq!(inside.len(), 1, "one closed part, one fill");
    // The square 21 is centred on the end (3.5, 2): pixel (336, 192), 0.75 S = 0.2025 inch = 19.44 pixels each way.
    let (x0, y0, x1, y1) = bbox(attr(inside[0], "d"));
    assert!(close(x0, 336.0 - 19.44) && close(x1, 336.0 + 19.44) && close(y0, 192.0 - 19.44) && close(y1, 192.0 + 19.44), "{:?}", (x0, y0, x1, y1));
    assert_eq!(inside[0].attr("fill-rule"), Some("evenodd"), "painted as the shape's own fill is");
    assert_eq!(inside[0].attr("fill-opacity"), None, "an opaque fill needs no opacity");
    // The outline of the mark is a stroked path with no fill, as wide as the line is heavy, and it comes after the inside.
    let inside_at = position(&root, |p| p.attr("fill") == Some("#ff0000")).expect("the inside is written");
    let outline_at = position(&root, |p| {
        p.attr("fill") == Some("none") && p.attr("stroke").is_some_and(|s| s == "#000000") && {
            let (a, b, c, d) = bbox(attr(p, "d"));
            close(a, x0) && close(b, y0) && close(c, x1) && close(d, y1)
        }
    })
    .expect("the mark's outline is written");
    assert!(inside_at < outline_at, "the fill is under the outline: {inside_at} before {outline_at}");
}

#[test]
fn the_fills_transparency_is_the_insides_and_the_outlines_is_the_lines() {
    let (_, root) = export(&one_page(vec![connector(1, 2.0, 0.02, 21).cell("FillForegndTrans", "0.5").cell("LineColorTrans", "0.25")]), &defaults());
    let inside = painted(&root, "#ff0000");
    assert_eq!(inside.len(), 1);
    let opacity: f64 = attr(inside[0], "fill-opacity").parse().expect("a number");
    assert!((opacity - 0.5).abs() < 0.01, "{opacity}");
    // The line is a quarter clear: the shaft and the mark's outline say 0.75, and nothing of the fill's transparency is on them.
    let strokes: Vec<f64> = named(&root, "path")
        .into_iter()
        .filter(|p| p.attr("fill") == Some("none"))
        .filter_map(|p| p.attr("stroke-opacity"))
        .filter_map(|o| o.parse().ok())
        .collect();
    assert!(strokes.len() >= 2 && strokes.iter().all(|o| (o - 0.75).abs() < 0.01), "the shaft and the outline are 25% clear: {strokes:?}");
}

#[test]
fn a_shape_with_no_fill_and_the_solid_and_open_marks_write_no_inside() {
    // No fill: nothing is painted red. A solid mark (13) is painted in the line's colour and an open one (3) has no closed part: no path is red either.
    for (what, shape) in [
        ("no fill", connector(1, 2.0, 0.02, 21).cell("FillPattern", "0")),
        ("a solid mark", connector(1, 2.0, 0.02, 13)),
        ("an open mark", connector(1, 2.0, 0.02, 3)),
        ("no mark", connector(1, 2.0, 0.02, 0)),
    ] {
        let (_, root) = export(&one_page(vec![shape]), &defaults());
        assert!(painted(&root, "#ff0000").is_empty(), "{what}: nothing is painted in the fill colour");
    }
    // The solid mark is a path in the line's black, filled.
    let (_, root) = export(&one_page(vec![connector(1, 2.0, 0.02, 13)]), &defaults());
    assert_eq!(painted(&root, "#000000").len(), 1, "number 13 is one filled triangle in the line's colour");
}

/// `count` connectors, each with a hollow square at its begin and a hollow long diamond at its end, on a page of `side` inches, one row of them above another; with a left to right
/// gradient from red to blue when `gradient` is set.
fn many(count: u32, gradient: bool) -> diacraft_doc::Document {
    let mut page = page(8.0, 8.0);
    for k in 0..count {
        let (x, y) = (0.5 + f64::from(k % 5) * 1.4, 0.4 + f64::from(k / 5) * 0.3);
        let mut line = geom_flag(
            geometry_of(shape_at(k + 1, x, y, 1.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0), ("Y", 0.0)])]),
            "NoFill",
            "1",
        )
        .cell("FillPattern", "1")
        .cell("FillForegnd", "#FF0000")
        .cell("LineWeight", "0.01")
        .cell("LinePattern", "1")
        .cell("LineColor", "#000000")
        .cell("BeginArrow", "21")
        .cell("EndArrow", "22")
        .cell("BeginArrowSize", "1")
        .cell("EndArrowSize", "1");
        if gradient {
            line = line.cell("FillGradientEnabled", "1").cell("FillGradientDir", "0").cell("FillGradientAngle", "0");
            for (ix, pos, colour) in [(0u32, "0", "#FF0000"), (1, "1", "#0000FF")] {
                let key = diacraft_doc::RowKey::Ix(ix);
                line = line.row_cell("FillGradient", None, key.clone(), "GradientStopColor", colour).row_cell(
                    "FillGradient",
                    None,
                    key,
                    "GradientStopPosition",
                    pos,
                );
            }
        }
        page = page.shape(line);
    }
    base().page(1, page).build()
}

#[test]
fn a_page_of_connectors_with_hollow_ends_writes_one_fill_for_each_closed_part_and_no_more() {
    // Counted: 100 connectors, each with two hollow marks of one closed part each: 200 inside paths and nothing else red; and the same page with gradients holds 200 gradient
    // definitions, one for each closed part, each used by one path (there is no marker to share, a mark being a path of its own). The file stays within what the paths need.
    let count = 100;
    let (flat, root) = export(&many(count, false), &defaults());
    assert_eq!(painted(&root, "#ff0000").len(), 2 * count as usize);
    assert!(flat.bytes.len() < 300 * 1024, "{} bytes for {count} connectors", flat.bytes.len());
    let (graded, root) = export(&many(count, true), &defaults());
    let defined = named(&root, "linearGradient");
    assert_eq!(defined.len(), 2 * count as usize, "one gradient for each closed part");
    let used = named(&root, "path").into_iter().filter(|p| p.attr("fill").is_some_and(|f| f.starts_with("url(#"))).count();
    assert_eq!(used, 2 * count as usize, "each is painted by one path");
    assert!(graded.bytes.len() < 600 * 1024, "{} bytes", graded.bytes.len());
}
