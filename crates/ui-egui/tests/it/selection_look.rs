//! What a net and a selection look like: a translucent grey veil with a one-pixel outline while the button is down and
//! nothing marked on the shapes under it; after the release a dark blue frame on each shape of a multiple selection (twice as thick on the primary), one thin grey
//! box with eight round handles and a rotation handle round the lot, a connector inside the net drawn again in the frame colour, green marks at a single connector's
//! ends. What is checked is what the last frame painted (the shapes egui was given), found by the colours of the theme's tokens; the sizes are points and do not
//! change with the zoom.

use diacraft_ui_egui::theme::{Tokens, select};
use egui::epaint::ColorMode;
use egui::{Color32, Modifiers, Pos2, Rect, Shape, pos2};
use serde_json::json;

use crate::editing::{at, edit_window, frame_of, shape_ids, three_shapes};
use crate::support::{Rest, Window, button, click, move_to, run_command};

/// Every shape the last frame painted, groups opened, in painting order.
pub(crate) fn painted(w: &Window) -> Vec<Shape> {
    fn open(shape: &Shape, into: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(list) => list.iter().for_each(|s| open(s, into)),
            other => into.push(other.clone()),
        }
    }
    let mut all = Vec::new();
    for clipped in &w.output().shapes {
        open(&clipped.shape, &mut all);
    }
    all
}

fn solid(mode: &ColorMode) -> Option<Color32> {
    match mode {
        ColorMode::Solid(c) => Some(*c),
        ColorMode::UV(_) => None,
    }
}

/// The rectangles of the frame colour (or any outline colour) the last frame drew with a stroke of `width` points, whether as a rectangle or as a closed path round a
/// turned shape, as the places they cover.
fn outlines(w: &Window, colour: Color32, width: f32) -> Vec<Rect> {
    painted(w)
        .into_iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.fill == Color32::TRANSPARENT && r.stroke.color == colour && (r.stroke.width - width).abs() < 1e-3 => Some(r.rect),
            Shape::Path(p) if p.closed && solid(&p.stroke.color) == Some(colour) && (p.stroke.width - width).abs() < 1e-3 => {
                Some(Rect::from_points(&p.points))
            }
            _ => None,
        })
        .collect()
}

/// The round handles drawn: filled with the handle colour and ringed in the box grey; their centres and their outer widths.
pub(crate) fn handles(w: &Window) -> Vec<(Pos2, f32)> {
    let t = Tokens::light();
    painted(w)
        .into_iter()
        .filter_map(|s| match s {
            Shape::Circle(c) if c.fill == t.handle_fill && c.stroke.color == t.select_box => Some((c.center, 2.0 * c.radius + c.stroke.width)),
            _ => None,
        })
        .collect()
}

/// Where the net is drawn (the rectangle with the veil's fill), if it is.
fn net(w: &Window, t: &Tokens) -> Option<Rect> {
    painted(w).into_iter().find_map(|s| match s {
        Shape::Rect(r) if r.fill == t.net_fill => Some(r.rect),
        _ => None,
    })
}

/// The marks of a selection: every shape the frame colour or the box grey was drawn with.
fn marks_of_selection(w: &Window, t: &Tokens) -> usize {
    painted(w)
        .into_iter()
        .filter(|s| match s {
            Shape::Rect(r) => r.stroke.color == t.select_frame || r.stroke.color == t.select_box,
            Shape::Path(p) => matches!(solid(&p.stroke.color), Some(c) if c == t.select_frame || c == t.select_box),
            Shape::Circle(c) => c.stroke.color == t.select_box,
            Shape::LineSegment { stroke, .. } => stroke.color == t.select_box || stroke.color == t.select_frame,
            _ => false,
        })
        .count()
}

/// Presses on bare page at `from` and drags to `to`, leaving the button down. → the two places, on the screen.
pub(crate) fn hold_net(w: &mut Window, from: (f64, f64), to: (f64, f64)) -> (Pos2, Pos2) {
    let (a, b) = (at(w, from.0, from.1), at(w, to.0, to.1));
    for _ in 0..3 {
        move_to(w, a);
    }
    button(w, a, true);
    for step in 1..=5 {
        move_to(w, a + (b - a) * (step as f32 / 5.0));
    }
    w.step();
    (a, b)
}

/// The three shapes: the rectangle (3, 6), the diamond (7, 6) and the ellipse (5, 3), with the net that takes the rectangle and the diamond but not the ellipse.
pub(crate) const AROUND_TWO: ((f64, f64), (f64, f64)) = ((1.5, 7.2), (8.6, 4.2));

pub(crate) fn three() -> (Window, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    // Nothing selected: a click on the bare page clears the ellipse the drop selected.
    crate::editing::click_at(&mut w, 8.0, 1.5);
    assert!(w.state().selection().is_empty());
    (w, ids)
}

fn on_screen(w: &Window, id: u32) -> Rect {
    let (x, y, width, height) = frame_of(w, id);
    Rect::from_two_pos(at(w, x, y), at(w, x + width, y + height))
}

fn near(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 1.0 && (a.max - b.max).length() < 1.0
}

/// The net is a rectangle between the press and the pointer, filled with the veil and outlined by one solid pixel of grey, drawn **over** the picture of the
/// shapes, and it looks the same whichever way it is dragged.
#[test]
fn the_net_is_a_veil_with_a_one_pixel_outline_over_the_shapes() {
    let t = Tokens::light();
    let (mut w, _) = three();
    let (a, b) = hold_net(&mut w, (1.5, 7.2), (8.6, 4.2));
    assert!(w.state().edit_dragging());
    let rect = net(&w, &t).expect("the net is drawn while the button is down");
    assert!(near(rect, Rect::from_two_pos(a, b)), "{rect:?} is between the press and the pointer");
    let shapes = painted(&w);
    let (veil, veil_shape) = shapes
        .iter()
        .enumerate()
        .find_map(|(n, s)| match s {
            Shape::Rect(r) if r.fill == t.net_fill => Some((n, r.clone())),
            _ => None,
        })
        .expect("the veil");
    assert_eq!(veil_shape.stroke.color, t.net_line, "the outline is the net's grey");
    assert_eq!(veil_shape.stroke.width, select::NET_LINE, "and one pixel wide");
    let picture =
        shapes.iter().position(|s| matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default())).expect("the picture of the shapes");
    assert!(picture < veil, "the shapes' picture ({picture}) is under the veil ({veil}): the veil is drawn over them");
    // The other way, lower right to upper left, looks the same.
    button(&mut w, b, false);
    w.rest();
    let (c, d) = hold_net(&mut w, (8.6, 4.2), (1.5, 7.2));
    let again = net(&w, &t).expect("the net is drawn the other way too");
    assert!(near(again, Rect::from_two_pos(c, d)), "{again:?}");
    assert!((again.size() - rect.size()).length() < 1.0, "the same net: {again:?} {rect:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The veil is a veil of light grey at about 40 percent: over white about #E5E5E5 and over a blue (#4D89BA) about #7B9EBB, within the readings' error. It is a
/// translucent token in both themes, and the outline is an opaque grey.
#[test]
fn the_veil_blends_as_the_spec_reads() {
    for t in [Tokens::light(), Tokens::dark()] {
        assert!(t.net_fill.a() > 80 && t.net_fill.a() < 130, "about 40 percent: {}", t.net_fill.a());
        assert_eq!(t.net_line.a(), 255);
        let over = |under: [u8; 3]| -> [u8; 3] {
            let a = f32::from(t.net_fill.a()) / 255.0;
            let mix = |fill: u8, under: u8| (f32::from(fill) + f32::from(under) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
            [mix(t.net_fill.r(), under[0]), mix(t.net_fill.g(), under[1]), mix(t.net_fill.b(), under[2])]
        };
        let white = over([255, 255, 255]);
        assert!(white.iter().all(|c| c.abs_diff(0xE5) <= 4), "over white {white:?}");
        let blue = over([0x4D, 0x89, 0xBA]);
        assert!(blue[0].abs_diff(0x7B) <= 4 && blue[1].abs_diff(0x9E) <= 4 && blue[2].abs_diff(0xBB) <= 4, "over blue {blue:?}");
    }
}

/// Nothing is marked on any shape while the button is down: a shape wholly inside the net and one only partly inside look alike.
#[test]
fn nothing_is_marked_before_the_release() {
    let t = Tokens::light();
    let (mut w, _) = three();
    // The net takes the rectangle and half of the ellipse's box.
    hold_net(&mut w, (1.5, 7.2), (5.2, 2.5));
    assert!(net(&w, &t).is_some());
    assert_eq!(marks_of_selection(&w, &t), 0, "no frame, handle or box while the button is down");
    assert!(w.state().selection().is_empty(), "and nothing is selected yet");
}

/// On the release the veil and its outline go and the marks come in the same frame; the net selects what lies wholly inside it.
#[test]
fn the_net_goes_on_the_release_and_the_marks_come_in_the_same_frame() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    assert!(net(&w, &t).is_some());
    button(&mut w, b, false);
    assert!(net(&w, &t).is_none(), "the veil goes in the frame of the release");
    assert_eq!(w.state().selection(), &ids[..2], "the rectangle and the diamond, not the ellipse (partly inside is not inside)");
    assert!(!outlines(&w, t.select_frame, select::FRAME).is_empty(), "and the frames are there in that frame");
    assert!(painted(&w).iter().all(|s| !matches!(s, Shape::Rect(r) if r.stroke.color == t.net_line)), "no outline of the net is left");
}

/// The veil shows from the first movement: there is no dead zone between the press and the first veil.
#[test]
fn the_veil_shows_from_the_first_movement() {
    let t = Tokens::light();
    let (mut w, _) = three();
    let a = at(&w, 8.0, 1.5);
    for _ in 0..3 {
        move_to(&mut w, a);
    }
    button(&mut w, a, true);
    assert!(net(&w, &t).is_none(), "no veil at the press: the net has no size");
    move_to(&mut w, a + egui::vec2(2.0, 1.0));
    let rect = net(&w, &t).expect("a small veil with the first movement");
    assert!(rect.width() > 1.0 && rect.width() < 4.0, "{rect:?}");
    button(&mut w, a + egui::vec2(2.0, 1.0), false);
    assert!(net(&w, &t).is_none());
}

/// The pointer is a crosshair from the press on bare page to the release, and the arrow before and after.
#[test]
fn the_pointer_is_a_crosshair_from_the_press_to_the_release() {
    let (mut w, _) = three();
    let a = at(&w, 8.0, 1.5);
    for _ in 0..3 {
        move_to(&mut w, a);
    }
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Default, "the arrow before the press");
    button(&mut w, a, true);
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Crosshair, "a crosshair at the press");
    move_to(&mut w, a + egui::vec2(60.0, 40.0));
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Crosshair, "and while it is dragged");
    button(&mut w, a + egui::vec2(60.0, 40.0), false);
    w.step();
    assert_eq!(w.output().platform_output.cursor_icon, egui::CursorIcon::Default, "the arrow again after the release");
}

/// Every shape of a multiple selection has a frame of two points on its own box; the primary (the one selected first) has a frame of four.
#[test]
fn each_shape_of_a_multiple_selection_has_a_frame_and_the_primary_a_thicker_one() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, (1.0, 7.5), (9.0, 1.8));
    button(&mut w, b, false);
    w.rest();
    assert_eq!(w.state().selection(), ids.as_slice(), "all three");
    let thin = outlines(&w, t.select_frame, select::FRAME);
    let thick = outlines(&w, t.select_frame, select::FRAME_PRIMARY);
    assert_eq!(thin.len(), 2, "two ordinary frames: {thin:?}");
    assert_eq!(thick.len(), 1, "one primary frame: {thick:?}");
    assert!(
        near(thick[0], on_screen(&w, ids[0])),
        "the primary is the first selected (the backmost of a net's): {:?} against {:?}",
        thick[0],
        on_screen(&w, ids[0])
    );
    for id in &ids[1..] {
        let expected = on_screen(&w, *id);
        assert!(thin.iter().any(|r| near(*r, expected)), "shape {id} has a frame on its own box {expected:?}: {thin:?}");
    }
}

/// A Ctrl-click on a shape adds it with a plain frame and the primary keeps its thick one; removing the primary hands the thick frame to the first that is left.
#[test]
fn the_thick_frame_follows_the_primary() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    // Ctrl-click the ellipse: it joins, with a plain frame.
    crate::editing::click_with(&mut w, (5.0, 3.0), Modifiers::CTRL);
    assert_eq!(w.state().selection(), ids.as_slice());
    let thick = outlines(&w, t.select_frame, select::FRAME_PRIMARY);
    assert_eq!((thick.len(), outlines(&w, t.select_frame, select::FRAME).len()), (1, 2));
    assert!(near(thick[0], on_screen(&w, ids[0])));
    // Ctrl-click the primary: it goes and the thick frame passes to the next.
    crate::editing::click_with(&mut w, (3.0, 6.0), Modifiers::CTRL);
    assert_eq!(w.state().selection(), &ids[1..]);
    let thick = outlines(&w, t.select_frame, select::FRAME_PRIMARY);
    assert_eq!(thick.len(), 1);
    assert!(near(thick[0], on_screen(&w, ids[1])), "{:?}", thick[0]);
}

/// A multiple selection has one thin grey box round all of it, with eight round handles on its line (four corners and four side middles) and the rotation handle
/// above the top middle; no handle belongs to any single shape.
#[test]
fn a_multiple_selection_has_one_box_with_eight_round_handles() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    let boxes = outlines(&w, t.select_box, select::BOX_LINE);
    assert_eq!(boxes.len(), 1, "one box: {boxes:?}");
    let expected = on_screen(&w, ids[0]).union(on_screen(&w, ids[1])).expand(select::BOX_MARGIN);
    assert!(near(boxes[0], expected), "the box takes in every member, with its margin: {:?} against {expected:?}", boxes[0]);
    let found = handles(&w);
    assert_eq!(found.len(), 8, "eight handles: {found:?}");
    let r = boxes[0];
    let spots =
        [r.left_top(), r.center_top(), r.right_top(), r.right_center(), r.right_bottom(), r.center_bottom(), r.left_bottom(), r.left_center()];
    for spot in spots {
        assert!(found.iter().any(|(c, _)| (*c - spot).length() < 1.0), "a handle at {spot:?}: {found:?}");
    }
    for (_, width) in &found {
        assert!((width - select::HANDLE).abs() < 0.1 && (11.0..=12.0).contains(width), "handles are 11 to 12 across: {width}");
    }
    // The rotation handle: a stem from the top middle handle up to a glyph.
    let stems: Vec<[Pos2; 2]> = painted(&w)
        .into_iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == t.select_box && (stroke.width - select::ROTATE_STEM).abs() < 1e-3 => {
                Some(points)
            }
            _ => None,
        })
        .collect();
    assert_eq!(stems.len(), 1, "one stem: {stems:?}");
    assert!((stems[0][0].x - r.center().x).abs() < 1.0 && (stems[0][1].x - r.center().x).abs() < 1.0, "above the top middle: {stems:?}");
    assert!(stems[0][0].y < r.top() && stems[0][1].y < stems[0][0].y, "going up from the box: {stems:?}");
    // The shapes themselves wear no handle: the eight are the box's.
    assert_eq!(w.state().ui_panics(), 0);
}

/// The rotation handle has the proportions of the handle it is drawn beside: with handles of 12, the glyph is 19 to 24 across, the stem 18 to 23 long and
/// the glyph's centre 35 to 38 above the box line.
#[test]
fn the_rotation_handle_has_the_documented_proportions() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    let _ = ids;
    let r = outlines(&w, t.select_box, select::BOX_LINE)[0];
    let stem = painted(&w)
        .into_iter()
        .find_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == t.select_box && (stroke.width - select::ROTATE_STEM).abs() < 1e-3 => {
                Some(points)
            }
            _ => None,
        })
        .expect("a stem");
    let length = (stem[0] - stem[1]).length();
    assert!((18.0..=23.0).contains(&length), "stem {length}");
    let ring: Vec<Pos2> = painted(&w)
        .into_iter()
        .filter_map(|s| match s {
            Shape::Path(p) if !p.closed && solid(&p.stroke.color) == Some(t.select_box) && (p.stroke.width - select::ROTATE_RING).abs() < 1e-3 => {
                Some(p.points)
            }
            _ => None,
        })
        .max_by_key(|points| points.len())
        .expect("the glyph's ring");
    let bounds = Rect::from_points(&ring);
    let across = bounds.width() + select::ROTATE_RING;
    assert!((19.0..=24.0).contains(&across), "glyph {across}");
    // The ring runs round most of the circle, so its bounds are as wide as the circle and reach its bottom.
    let centre_y = bounds.bottom() + select::ROTATE_RING / 2.0 - across / 2.0;
    let above = r.top() - centre_y;
    assert!((35.0..=38.0).contains(&above), "centre {above} above the box line");
}

/// A single selected shape has no frame and no primary mark: one box on the shape's own box (no margin), eight handles and the rotation handle.
#[test]
fn a_single_selection_has_the_box_and_the_handles_and_no_frame() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    crate::editing::click_at(&mut w, 3.0, 6.0);
    assert_eq!(w.state().selection(), &ids[..1]);
    assert!(outlines(&w, t.select_frame, select::FRAME).is_empty() && outlines(&w, t.select_frame, select::FRAME_PRIMARY).is_empty(), "no frame");
    let boxes = outlines(&w, t.select_box, select::BOX_LINE);
    assert_eq!(boxes.len(), 1);
    assert!(near(boxes[0], on_screen(&w, ids[0])), "the box is the shape's own: {:?}", boxes[0]);
    assert_eq!(handles(&w).len(), 8);
    assert!(
        painted(&w).iter().any(|s| matches!(s, Shape::LineSegment { stroke, .. } if stroke.color == t.select_box)),
        "and the rotation handle's stem"
    );
}

/// The sizes are screen points: the frames, the box line, the handles and the margin keep their size at any zoom (the preference of the brief).
#[test]
fn the_marks_keep_their_size_whatever_the_zoom() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    let widths_at_100 = (handles(&w).first().map(|h| h.1), outlines(&w, t.select_frame, select::FRAME_PRIMARY).len());
    run_command(&mut w, "view.zoom", json!({"percent": 200}));
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], ids[1]]}));
    w.rest();
    let found = handles(&w);
    assert!(found.iter().all(|(_, width)| (width - select::HANDLE).abs() < 0.1), "handles are as wide as at 100%: {found:?}");
    assert_eq!(widths_at_100.0.map(|w| w.round()), Some(select::HANDLE.round()));
    assert_eq!(outlines(&w, t.select_frame, select::FRAME_PRIMARY).len(), 1, "the frame is as thick at 200% as at 100%");
    assert_eq!(outlines(&w, t.select_frame, select::FRAME).len(), 1);
    let boxes = outlines(&w, t.select_box, select::BOX_LINE);
    let expected = on_screen(&w, ids[0]).union(on_screen(&w, ids[1])).expand(select::BOX_MARGIN);
    assert!(near(boxes[0], expected), "the margin is points at any zoom: {:?} against {expected:?}", boxes[0]);
}

/// A connector wholly inside the net joins the selection and is drawn again along its own path in the frame colour at two points (four for the primary); one with an end
/// outside is not selected and is left as it was.
#[test]
fn a_connector_inside_the_net_is_drawn_again_in_the_frame_colour() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let page = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    let inside = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}, "page": page})).unwrap();
    let outside = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[2]}, "to": {"id": ids[0]}, "page": page})).unwrap();
    w.rest();
    let (inside, outside) = (inside["id"].as_u64().unwrap() as u32, outside["id"].as_u64().unwrap() as u32);
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    let selected = w.state().selection().to_vec();
    assert!(selected.contains(&inside), "the connector between the two is selected: {selected:?}");
    assert!(!selected.contains(&outside), "the one that leaves the net is not: {selected:?}");
    let lines: Vec<(f32, Vec<Pos2>)> = painted(&w)
        .into_iter()
        .filter_map(|s| match s {
            Shape::Path(p) if !p.closed && solid(&p.stroke.color) == Some(t.select_frame) => Some((p.stroke.width, p.points)),
            Shape::LineSegment { points, stroke } if stroke.color == t.select_frame => Some((stroke.width, points.to_vec())),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 1, "the one connector's path, once: {lines:?}");
    assert!((lines[0].0 - select::FRAME).abs() < 1e-3, "two points wide: {}", lines[0].0);
    // It runs between the two shapes along y = 6 (screen), not along the box.
    let (y, left, right) = (at(&w, 5.0, 6.0).y, at(&w, 3.7, 6.0).x, at(&w, 6.3, 6.0).x);
    assert!(lines[0].1.iter().all(|p| (p.y - y).abs() < 2.0), "{:?}", lines[0].1);
    assert!(lines[0].1.iter().any(|p| p.x > left) && lines[0].1.iter().any(|p| p.x < right));
}

/// A single selected connector shows its two ends: a green disc where it is glued, and a square where it is free.
#[test]
fn the_ends_of_a_single_selected_connector_are_marked_green_when_glued_and_square_when_free() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let page = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    let glued = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}, "page": page})).unwrap();
    let free = w.state_mut().execute("connector.add", &json!({"from": {"x": 1.0, "y": 2.0}, "to": {"x": 2.5, "y": 1.2}, "page": page})).unwrap();
    w.rest();
    let discs = |w: &Window| painted(w).into_iter().filter(|s| matches!(s, Shape::Circle(c) if c.fill == t.glue_on)).count();
    let squares = |w: &Window| {
        painted(w).into_iter().filter(|s| matches!(s, Shape::Rect(r) if r.stroke.color == t.select_box && r.rect.width() < select::HANDLE)).count()
    };
    run_command(&mut w, "edit.select", json!({"ids": [glued["id"].as_u64().unwrap()]}));
    assert_eq!((discs(&w), squares(&w)), (2, 0), "both ends glued: two green discs");
    run_command(&mut w, "edit.select", json!({"ids": [free["id"].as_u64().unwrap()]}));
    assert_eq!((discs(&w), squares(&w)), (0, 2), "both ends free: two squares");
    assert!(outlines(&w, t.select_frame, select::FRAME).is_empty(), "no frame on a connector alone");
}

/// A group alone is one box with the handles and no frame on its members.
#[test]
fn a_group_alone_is_one_box_with_the_handles_and_no_member_frames() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], ids[1]]}));
    run_command(&mut w, "shape.group", json!({"ids": [ids[0], ids[1]]}));
    let group = *shape_ids(&w).last().unwrap();
    run_command(&mut w, "edit.select", json!({"ids": [group]}));
    assert_eq!(w.state().selection(), &[group]);
    assert!(
        outlines(&w, t.select_frame, select::FRAME).is_empty() && outlines(&w, t.select_frame, select::FRAME_PRIMARY).is_empty(),
        "no member frame"
    );
    assert_eq!(outlines(&w, t.select_box, select::BOX_LINE).len(), 1, "one box");
    assert_eq!(handles(&w).len(), 8, "eight handles");
}

/// The marks are readable in the dark theme: the dark theme draws with its own tokens, and its frame can be seen on the white page and on the dark workspace round it;
/// the grey marks can be seen on both themes' workspace.
#[test]
fn the_marks_are_drawn_in_the_dark_themes_tokens_and_can_be_seen() {
    let (mut w, ids) = three();
    run_command(&mut w, "window.theme", json!({"theme": "dark"}));
    let t = Tokens::dark();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    assert!(net(&w, &t).is_some(), "the net is drawn in the dark theme");
    button(&mut w, b, false);
    w.rest();
    assert_eq!(w.state().selection(), &ids[..2]);
    assert_eq!(outlines(&w, t.select_frame, select::FRAME_PRIMARY).len(), 1, "the dark theme's frame colour");
    assert_eq!(outlines(&w, t.select_box, select::BOX_LINE).len(), 1);
    assert_ne!(Tokens::light().select_frame, t.select_frame, "the dark theme has a blue of its own");
    let contrast = |a: Color32, b: Color32| {
        let lum = |c: Color32| {
            let f = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    for t in [Tokens::light(), Tokens::dark()] {
        for ground in [t.page_fill, t.canvas_fill] {
            assert!(contrast(t.select_frame, ground) >= 3.0, "the frame on {ground:?}: {}", contrast(t.select_frame, ground));
            assert!(contrast(t.select_box, ground) >= 1.9, "the grey on {ground:?}: {}", contrast(t.select_box, ground));
            assert!(contrast(t.net_line, ground) >= 1.5, "the net's outline on {ground:?}: {}", contrast(t.net_line, ground));
        }
    }
}

/// The handles of a combined box are not working handles yet: a click on one keeps the selection and sends nothing, where a click on the bare page clears it.
#[test]
fn a_click_on_a_handle_of_the_combined_box_keeps_the_selection() {
    let t = Tokens::light();
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    let corner = handles(&w).into_iter().map(|(c, _)| c).min_by(|a, b| (a.x + a.y).total_cmp(&(b.x + b.y))).unwrap();
    let revision = w.state().session().revision();
    click(&mut w, corner);
    w.rest();
    assert_eq!(w.state().selection(), &ids[..2], "the selection is kept");
    assert_eq!(w.state().session().revision(), revision, "and nothing is sent");
    assert_eq!(outlines(&w, t.select_frame, select::FRAME_PRIMARY).len(), 1);
    // A click on bare page well away from the box clears it.
    crate::editing::click_at(&mut w, 8.0, 1.5);
    assert!(w.state().selection().is_empty());
    let _ = pos2(0.0, 0.0);
}

/// Escape in the middle of a net drops it: the veil goes at once and nothing is selected when the button comes up.
#[test]
fn escape_drops_the_net_and_selects_nothing() {
    let t = Tokens::light();
    let (mut w, _) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    assert!(net(&w, &t).is_some());
    crate::editing::key(&mut w, egui::Key::Escape, Modifiers::NONE);
    assert!(net(&w, &t).is_none(), "the veil is gone");
    button(&mut w, b, false);
    w.rest();
    assert!(w.state().selection().is_empty(), "and the net took nothing");
    assert_eq!(marks_of_selection(&w, &t), 0);
}
