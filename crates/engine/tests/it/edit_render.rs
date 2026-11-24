//! What the editing commands make is drawn: a dropped shape, a connector with its arrowhead, a shape's text. The pixels are named in
//! paper inches, as in `crates/render/tests`; no reference image from another application is used.

use diacraft_engine::Session;
use diacraft_engine::stencil;
use diacraft_render::{Rendered, Renderer, View};
use serde_json::json;

use crate::{blank_session, run};

const PX_PER_INCH: f64 = 100.0;
const PAGE_HEIGHT: f64 = 8.5;

fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    Renderer::new().render(doc, 1, &view)
}

/// The pixel at a place on the page, in inches from its left and bottom edges: premultiplied RGBA.
fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2], p[3]]
}

/// Ink: a pixel clearly darker than white. A line one hundredth of an inch wide on a hundred pixels to the inch is a pixel wide and, off the
/// pixel grid, half a grey on two pixels, so the threshold is a grey and not black.
fn dark(p: [u8; 4]) -> bool {
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> usize {
    let mut n = 0;
    let (mut y, step) = (bottom, 1.0 / PX_PER_INCH);
    while y < top {
        let mut x = left;
        while x < right {
            n += usize::from(dark(px(r, x, y)));
            x += step;
        }
        y += step;
    }
    n
}

#[test]
fn a_dropped_rectangle_is_drawn_with_a_white_fill_and_a_black_outline() {
    let mut s = blank_session();
    let empty = render(&s);
    assert_eq!(ink(&empty, 0.0, 0.0, 11.0, 8.5), 0, "an empty page has no ink");
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 4}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "everything was drawn: {:?}", r.skipped);
    // The 1.5 by 1 box spans x 2.25 to 3.75 and y 3.5 to 4.5: its four edges are inked, the inside is the white of the fill, and the outside is paper.
    assert!(ink(&r, 2.22, 3.9, 2.28, 4.1) > 3, "left edge");
    assert!(ink(&r, 3.72, 3.9, 3.78, 4.1) > 3, "right edge");
    assert!(ink(&r, 2.9, 4.47, 3.1, 4.53) > 3, "top edge");
    assert!(ink(&r, 2.9, 3.47, 3.1, 3.53) > 3, "bottom edge");
    assert_eq!(ink(&r, 2.4, 3.65, 3.6, 4.35), 0, "nothing inside but the fill");
    assert_eq!(px(&r, 3.0, 4.0), [255, 255, 255, 255]);
    assert_eq!(ink(&r, 0.0, 0.0, 2.1, 8.5), 0, "nothing outside");
}

#[test]
fn every_master_is_drawn_when_it_is_dropped() {
    for spec in stencil::masters() {
        let mut s = blank_session();
        run(&mut s, "shape.drop", json!({"master": spec.id, "page": 1, "x": 5.5, "y": 4.25}));
        let r = render(&s);
        assert!(r.skipped.is_empty(), "{}: {:?}", spec.name, r.skipped);
        let (w, h) = (spec.width, spec.height);
        let inked = ink(&r, 5.5 - w / 2.0 - 0.05, 4.25 - h / 2.0 - 0.05, 5.5 + w / 2.0 + 0.05, 4.25 + h / 2.0 + 0.05);
        assert!(inked > 50, "{}: only {inked} dark pixels", spec.name);
        // A point inside every one of the twelve (not the middle: the right triangle's long side runs through it) is filled white, and the
        // page outside the box is bare.
        let inside = (5.5 - w / 2.0 + 0.3 * w, 4.25 - h / 2.0 + 0.3 * h);
        assert_eq!(px(&r, inside.0, inside.1), [255, 255, 255, 255], "{}", spec.name);
        assert_eq!(ink(&r, 0.0, 0.0, 5.5 - w / 2.0 - 0.1, 8.5), 0, "{}", spec.name);
    }
}

#[test]
fn a_connector_is_drawn_between_its_shapes_with_an_arrowhead_at_its_end() {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6, "y": 2}));
    let before = render(&s);
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    let r = render(&s);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    // The two boxes end at x 2.75 and 5.25; the connector runs level between them at y 2.
    assert_eq!(ink(&before, 2.9, 1.9, 5.1, 2.1), 0, "nothing between the shapes before");
    assert!(ink(&r, 3.9, 1.98, 4.1, 2.02) > 10, "the shaft is drawn");
    // Just behind the tip the solid head is wider than the shaft: a column of ink across it, and only a pixel or two on the shaft.
    let across = |x: f64| ink(&r, x, 1.8, x + 0.01, 2.2);
    assert!(across(5.16) >= 5, "the arrowhead is {} pixels across", across(5.16));
    assert!(across(4.0) <= 2, "the shaft is {} pixels across", across(4.0));
    assert!(across(5.16) >= across(4.0) + 3);
    // Nothing is drawn above or below the level run.
    assert_eq!(ink(&r, 3.0, 2.3, 5.0, 8.5), 0);
}

#[test]
fn a_connector_with_a_bend_is_drawn_along_its_legs() {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6, "y": 4}));
    run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    let r = render(&s);
    // The Z of the route: along y 2 to x 4, up to y 4 at x 4, and along to the second box.
    assert!(ink(&r, 3.2, 1.98, 3.8, 2.02) > 10, "first leg");
    assert!(ink(&r, 3.98, 2.5, 4.02, 3.5) > 10, "cross leg");
    assert!(ink(&r, 4.5, 3.98, 5.0, 4.02) > 10, "last leg");
    assert_eq!(ink(&r, 3.2, 3.0, 3.8, 3.5), 0, "nothing off the route");
    // Moving the second box takes the cross leg with it.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 2, "dy": 0}));
    let moved = render(&s);
    assert!(ink(&moved, 4.98, 2.5, 5.02, 3.5) > 10, "the cross leg is at x 5 now");
    assert_eq!(ink(&moved, 3.98, 2.5, 4.02, 3.5), 0, "and not at x 4");
}

#[test]
fn a_shapes_text_is_drawn_inside_it() {
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 4}));
    assert_eq!(ink(&render(&s), 2.4, 3.65, 3.6, 4.35), 0);
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Hello"}));
    let r = render(&s);
    assert!(ink(&r, 2.4, 3.65, 3.6, 4.35) > 30, "the word is inked inside the box");
    // It sits in the middle, not against an edge.
    assert_eq!(ink(&r, 2.3, 3.55, 2.5, 4.45), 0);
}
