//! The placeholder of an object DiaCraft cannot draw: a `Foreign` shape with no picture to
//! draw and a `User` row `ODFObject` whose `Value` names what it is is drawn as its frame's outline, a pale fill, a cross and the label, and the frame says
//! so. A shape with a picture is the picture; a shape with no marker is as it was; a frame of no size draws nothing.
//!
//! Tests assert boxes, notes and where the label's ink lies, never the shape of a letter: the label is drawn in whatever face the renderer has, and a
//! frame with no face still draws the box.

use std::sync::Arc;

use diacraft_doc::{ForeignData, Media, RawAttrs, RowKey, ShapeType};
use diacraft_render::{Limits, Renderer, SkipKind};
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// The pale grey of a placeholder's fill.
const PALE: (u8, u8, u8) = (232, 232, 232);

/// An object with the frame `(x, y)`, `w` by `h`, no picture and the label `label`.
fn object(id: u32, x: f64, y: f64, w: f64, h: f64, label: &str) -> ShapeBuilder {
    shape_at(id, x, y, w, h).kind(ShapeType::Foreign).row_cell("User", None, RowKey::name("ODFObject"), "Value", label)
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&[255, 0, 0, 255].repeat(4)).unwrap();
    out
}

/// The pixels that differ between two pictures of one page, as the box that holds them in paper inches (x0, y0, x1, y1), when any.
fn changed(a: &Pic, b: &Pic) -> Option<(f64, f64, f64, f64)> {
    assert_eq!((a.r.width, a.r.height), (b.r.width, b.r.height));
    let w = a.r.width as usize;
    let (mut x0, mut y0, mut x1, mut y1, mut any) = (usize::MAX, usize::MAX, 0, 0, false);
    for (i, (p, q)) in a.r.rgba_premul.chunks(4).zip(b.r.rgba_premul.chunks(4)).enumerate() {
        if p != q {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, any) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), true);
        }
    }
    any.then(|| (x0 as f64 / a.ppi, a.page_h - (y1 as f64 + 1.0) / a.ppi, (x1 as f64 + 1.0) / a.ppi, a.page_h - y0 as f64 / a.ppi))
}

/// An object with no picture is its frame's box, pale, with a cross, and the frame counts it once, saying what it is; the frame is not said to have a
/// picture shape with no picture in it, nor text with no face (the label is ours, and a face missing is not the file's fault).
#[test]
fn an_object_with_no_picture_is_a_pale_box_with_a_cross_and_the_frame_says_so_once() {
    let pic = draw(&one_page(vec![object(7, 1.0, 1.0, 2.0, 1.5, "Table")]));
    // The fill, away from the cross and from the label.
    assert!(pic.is(1.15, 2.2, PALE, 6) && pic.is(2.85, 1.3, PALE, 6) && pic.is(1.2, 1.1, PALE, 6), "the box is pale grey");
    // The cross: a diagonal is darker than the fill.
    assert!(!pic.is(1.5, 1.375, PALE, 12) && !pic.is(1.5, 2.125, PALE, 12), "both diagonals");
    // Nothing outside the frame.
    for (x, y) in [(0.9, 1.7), (3.1, 1.7), (2.0, 0.9), (2.0, 2.6), (0.5, 0.5)] {
        assert!(pic.is_white(x, y), "({x}, {y}) is outside the frame");
    }
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 1);
    let first = pic.r.skipped.iter().find(|s| s.kind == SkipKind::ObjectPlaceholder).map(|s| s.first.clone()).unwrap_or_default();
    assert!(first.contains("shape 7") && first.contains("Table"), "{first}");
    for kind in
        [SkipKind::NoPicture, SkipKind::MissingGlyphs, SkipKind::TextNote, SkipKind::TextBudget, SkipKind::TextCharBudget, SkipKind::GlyphsCut]
    {
        assert_eq!(pic.r.count(kind), 0, "{kind:?}");
    }
}

/// The label is drawn inside the box, at its middle, in a size that fits: the ink the label adds (the same box with a label of one space, which has no letters, is the comparison) lies
/// within the frame and about its centre, and never above 0.2 inch tall (a label is a caption, not the box's own size, however big the box).
#[test]
fn the_label_is_drawn_inside_the_frame_about_its_middle_and_is_never_big() {
    for (w, h) in [(2.0, 1.5), (3.6, 3.0), (3.0, 0.8)] {
        let (x, y) = ((4.0 - w) / 2.0, (4.0 - h) / 2.0);
        let with = draw(&one_page(vec![object(1, x, y, w, h, "Embedded object")]));
        let without = draw(&one_page(vec![object(1, x, y, w, h, " ")]));
        let (x0, y0, x1, y1) = changed(&with, &without).unwrap_or_else(|| panic!("{w} by {h}: the label draws nothing"));
        assert!(x0 >= x - 0.02 && x1 <= x + w + 0.02 && y0 >= y - 0.02 && y1 <= y + h + 0.02, "{w} by {h}: the label is inside: {x0} {y0} {x1} {y1}");
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        assert!((cx - (x + w / 2.0)).abs() < 0.12 && (cy - (y + h / 2.0)).abs() < 0.12, "{w} by {h}: about the middle: {cx} {cy}");
        assert!(y1 - y0 <= 0.2, "{w} by {h}: {} inch tall", y1 - y0);
    }
}

/// The label stands on a plate of the box's own fill: along the two diagonals of the cross, where they run under the label, the picture is pale for the
/// most part and not the cross's grey (without the plate the cross runs through the letters and they cannot be read). The same places with no label are the
/// cross.
#[test]
fn the_label_stands_on_a_pale_plate_and_the_cross_does_not_run_through_it() {
    let with = draw(&one_page(vec![object(1, 1.0, 1.0, 2.0, 1.5, "Embedded object")]));
    let without = draw(&one_page(vec![object(1, 1.0, 1.0, 2.0, 1.5, " ")]));
    let (x0, y0, x1, y1) = changed(&with, &without).expect("the label");
    // The two diagonals of the frame, (1, 1) to (3, 2.5) and (1, 2.5) to (3, 1), at the places inside the plate's box, a hundred to each.
    let places: Vec<(f64, f64)> = (0..100)
        .flat_map(|k| {
            let x = 1.0 + 2.0 * f64::from(k) / 99.0;
            [(x, 1.0 + 0.75 * (x - 1.0)), (x, 2.5 - 0.75 * (x - 1.0))]
        })
        .filter(|(x, y)| *x >= x0 + 0.02 && *x <= x1 - 0.02 && *y >= y0 + 0.02 && *y <= y1 - 0.02)
        .collect();
    assert!(places.len() >= 12, "{} places under the label", places.len());
    let not_pale = |pic: &Pic| places.iter().filter(|(x, y)| !pic.is(*x, *y, PALE, 6)).count();
    assert!(not_pale(&without) * 10 >= places.len() * 8, "the cross is there with no label: {} of {}", not_pale(&without), places.len());
    assert!(not_pale(&with) * 10 <= places.len() * 5, "the plate covers the cross under the label: {} of {}", not_pale(&with), places.len());
}

/// A frame too small for a label that can be read is the box and the cross alone: the picture is the same as the one with a label of a space; and the label
/// shrinks to fit a narrow frame (a long label in a frame of 0.9 inch stays within it).
#[test]
fn a_label_that_cannot_be_read_in_the_frame_is_left_off_and_one_that_is_long_is_made_to_fit() {
    let tiny = draw(&one_page(vec![object(1, 1.0, 1.0, 0.25, 0.12, "Embedded object")]));
    let bare = draw(&one_page(vec![object(1, 1.0, 1.0, 0.25, 0.12, " ")]));
    assert_eq!(tiny.r.rgba_premul, bare.r.rgba_premul, "no label in a frame of a quarter inch");
    assert_eq!(tiny.r.count(SkipKind::ObjectPlaceholder), 1, "still a placeholder");
    let narrow = draw(&one_page(vec![object(1, 1.0, 1.0, 0.9, 1.5, "Embedded object")]));
    let narrow_bare = draw(&one_page(vec![object(1, 1.0, 1.0, 0.9, 1.5, " ")]));
    let (x0, _, x1, _) = changed(&narrow, &narrow_bare).expect("a label fits");
    assert!(x0 >= 1.0 - 0.02 && x1 <= 1.9 + 0.02, "{x0} {x1}");
}

/// A frame of no height, a dimension line, is drawn as the line it stands for with its label beside it, and says so; no frame at all draws nothing and
/// says nothing.
#[test]
fn a_frame_of_no_height_is_a_line_with_its_label_and_a_frame_of_no_size_is_nothing() {
    let line = draw(&one_page(vec![object(1, 0.5, 2.0, 3.0, 0.0, "Dimension line")]));
    assert!(line.ink_pixels() > 0, "the line is drawn");
    assert_eq!(line.r.count(SkipKind::ObjectPlaceholder), 1);
    let bare = draw(&one_page(vec![object(1, 0.5, 2.0, 3.0, 0.0, " ")]));
    assert!(changed(&line, &bare).is_some(), "the label is drawn beside it");
    let (lx0, ly0, lx1, ly1) = changed(&bare, &draw(&one_page(Vec::new()))).expect("the line");
    assert!(ly0 >= 1.97 && ly1 <= 2.03 && lx0 >= 0.47 && lx1 <= 3.53 && lx1 - lx0 > 2.9, "the line lies on the frame: {lx0} {ly0} {lx1} {ly1}");
    let (_, label_y0, _, label_y1) = changed(&line, &bare).expect("the label");
    assert!(label_y0 >= 1.99, "the label stands on the line, not under it: {label_y0}");
    assert!(label_y1 - label_y0 > 0.1, "and is not clipped to the margin of the line: {} inch", label_y1 - label_y0);
    for (w, h) in [(0.0, 0.0), (-1.0, 2.0), (2.0, -1.0), (-1.0, -1.0)] {
        let none = draw(&one_page(vec![object(1, 1.0, 1.0, w, h, "Table")]));
        assert_eq!((none.ink_pixels(), none.r.count(SkipKind::ObjectPlaceholder)), (0, 0), "{w} by {h}");
    }
    // A frame of no width is a line too, upright.
    let upright = draw(&one_page(vec![object(1, 2.0, 0.5, 0.0, 3.0, " ")]));
    let (ux0, uy0, ux1, uy1) = changed(&upright, &draw(&one_page(Vec::new()))).expect("the upright line");
    assert!(ux0 >= 1.97 && ux1 <= 2.03 && uy0 >= 0.47 && uy1 <= 3.53 && uy1 - uy0 > 2.9, "{ux0} {uy0} {ux1} {uy1}");
    assert_eq!(upright.r.count(SkipKind::ObjectPlaceholder), 1);
}

/// A label wider than its frame (twenty wide letters in 0.9 inch) does not run past it: the layout breaks the word, and nothing of the label (its plate
/// included) lies outside the frame and its margin.
#[test]
fn a_label_wider_than_its_frame_stays_in_it() {
    for letters in [12, 20, 40] {
        let text = "W".repeat(letters);
        let with = draw(&one_page(vec![object(1, 1.55, 1.0, 0.9, 1.5, &text)]));
        let without = draw(&one_page(vec![object(1, 1.55, 1.0, 0.9, 1.5, " ")]));
        let Some((x0, y0, x1, y1)) = changed(&with, &without) else { continue };
        assert!(x0 >= 1.55 - 0.03 && x1 <= 2.45 + 0.03 && y0 >= 1.0 - 0.03 && y1 <= 2.5 + 0.03, "{letters} letters: {x0} {y0} {x1} {y1}");
    }
}

/// A page of 400 units by 400 drawn at a hundredth of an inch to the unit (a drawing scale of 100): four inches square, its shapes in units.
fn at_a_hundredth(shapes: Vec<ShapeBuilder>) -> Pic {
    let mut p = diacraft_testkit::build::PageBuilder::new()
        .cell("PageWidth", "400")
        .cell("PageHeight", "400")
        .cell("PageScale", "1")
        .cell("DrawingScale", "100");
    for shape in shapes {
        p = p.shape(shape);
    }
    draw(&base().page(1, p).build())
}

/// The size of the label follows the frame **on paper**, whatever the scale of the page: on a page at a hundredth, a frame 0.9 inch wide and one 0.4 inch high
/// have the size the same frames have at scale 1 (the width gives 0.1 inch to the first, the height 0.12 to the second), and the plate under the label is as
/// tall as that size makes it, not as tall as the largest size a label has.
#[test]
fn the_size_of_a_label_follows_the_frame_on_paper_whatever_the_scale_of_the_page() {
    let plate = |x: f64, y: f64, w: f64, h: f64| {
        let with = at_a_hundredth(vec![object(1, x, y, w, h, "Embedded object")]);
        let without = at_a_hundredth(vec![object(1, x, y, w, h, " ")]);
        let (_, y0, _, y1) = changed(&with, &without).expect("a label is drawn");
        y1 - y0
    };
    // 0.9 inch wide and 1.0 high: the width limits the size (0.9 over 15 letters of 0.6 em is 0.1).
    let narrow = plate(155.0, 150.0, 90.0, 100.0);
    // 3 inches wide and 0.4 high: the height limits it (0.3 of 0.4 is 0.12).
    let low = plate(50.0, 180.0, 300.0, 40.0);
    // The plate is about 1.2 of the size: 0.12 and 0.144 inch; at the largest size, which a width or a height in drawing units would give, 0.168.
    assert!((0.10..=0.13).contains(&narrow), "the width limits the size: {narrow}");
    assert!((0.13..=0.155).contains(&low), "the height limits it: {low}");
}

/// The label is the grey of the box's own lines (90), not black: the darkest pixel the label adds is no darker than that.
#[test]
fn the_label_is_grey_and_not_black() {
    let with = draw(&one_page(vec![object(1, 0.2, 1.0, 3.6, 1.5, "Embedded object")]));
    let without = draw(&one_page(vec![object(1, 0.2, 1.0, 3.6, 1.5, " ")]));
    let darkest = with
        .r
        .rgba_premul
        .chunks(4)
        .zip(without.r.rgba_premul.chunks(4))
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a[0].min(a[1]).min(a[2]))
        .min()
        .expect("a label is drawn");
    assert!((80..=100).contains(&darkest), "the darkest pixel of the label is {darkest}");
}

/// The margin round a frame, which keeps a line of no height from being clipped away, is a fiftieth of an inch of paper whatever the page's scale: on a
/// page drawn at a hundredth (a drawing scale of 100, the page 400 units for 4 inches) a line of 300 units is as long, as thin and where the line of 3 inches
/// is on a page at scale 1; with a margin in drawing units it would be clipped to nothing.
#[test]
fn the_margin_round_a_line_is_paper_inches_on_a_page_of_any_scale() {
    let scaled = at_a_hundredth;
    let line = shape_at(1, 50.0, 200.0, 300.0, 0.0).kind(ShapeType::Foreign).row_cell("User", None, RowKey::name("ODFObject"), "Value", " ");
    let (x0, y0, x1, y1) = changed(&scaled(vec![line]), &scaled(Vec::new())).expect("the line is drawn");
    assert!(y0 >= 1.97 && y1 <= 2.03 && x0 >= 0.47 && x1 <= 3.53 && x1 - x0 > 2.9, "{x0} {y0} {x1} {y1}");
}

/// A shape with a picture is the picture: no placeholder, no note. A replacement with no picture to show (foreign data that holds none) is a placeholder.
#[test]
fn an_object_with_a_picture_is_the_picture_and_one_whose_foreign_data_holds_none_is_a_placeholder() {
    let media = Arc::new(Media { part: Arc::from("Pictures/r.png"), content_type: Arc::from("image/png"), bytes: Arc::from(png()) });
    let with = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]), picture: Some(media), ..ForeignData::default() };
    let picture = object(1, 1.0, 1.0, 2.0, 2.0, "Embedded object")
        .foreign(with)
        .cell("ImgWidth", "2")
        .cell("ImgHeight", "2")
        .cell("ImgOffsetX", "0")
        .cell("ImgOffsetY", "0");
    let pic = draw(&one_page(vec![picture]));
    assert!(pic.is_red(2.0, 2.0) && pic.r.count(SkipKind::ObjectPlaceholder) == 0, "the picture, and nothing said");
    let empty = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Object")]), ..ForeignData::default() };
    let none = draw(&one_page(vec![object(1, 1.0, 1.0, 2.0, 2.0, "Embedded object").foreign(empty)]));
    assert!(none.is(1.15, 2.4, PALE, 6) && none.r.count(SkipKind::ObjectPlaceholder) == 1);
    assert_eq!(none.r.count(SkipKind::NoPicture), 0, "the placeholder is the answer, not a second note");
}

/// The marker alone makes no placeholder: a shape that is not a foreign one with a user row of that name is drawn as it is.
#[test]
fn a_shape_that_is_not_foreign_with_the_row_is_not_a_placeholder() {
    let shape = shape_at(1, 1.0, 1.0, 2.0, 2.0).row_cell("User", None, RowKey::name("ODFObject"), "Value", "Table");
    let pic = draw(&one_page(vec![shape]));
    assert_eq!((pic.ink_pixels(), pic.r.count(SkipKind::ObjectPlaceholder)), (0, 0));
}

/// A foreign shape that holds no picture and names nothing is as it was: the file's own word for it (`NoPicture`), no placeholder, nothing drawn.
#[test]
fn a_foreign_shape_with_no_picture_and_no_name_is_still_a_picture_with_none_in_it() {
    let empty = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]), ..ForeignData::default() };
    let pic = draw(&one_page(vec![shape_at(1, 1.0, 1.0, 2.0, 2.0).kind(ShapeType::Foreign).foreign(empty)]));
    assert_eq!((pic.r.count(SkipKind::NoPicture), pic.r.count(SkipKind::ObjectPlaceholder), pic.ink_pixels()), (1, 0, 0));
}

/// A turned frame is a turned box (the pose of the shape applies as it does to any shape), a flipped one is the box where the flip puts it, and neither is
/// drawn outside its frame. A hostile frame (very large, infinite, not a number) is a frame that is drawn or refused and never a panic or a hang.
#[test]
fn a_turned_or_flipped_frame_is_a_box_where_the_pose_puts_it_and_a_hostile_one_is_survived() {
    // 2 wide, 1.5 high, pinned at its centre (LocPin 1, 0.75) at (2, 2), turned a quarter turn: 1.5 wide, 2 high.
    let turned = shape_at(1, 2.0, 2.0, 2.0, 1.5)
        .kind(ShapeType::Foreign)
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.75")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string())
        .row_cell("User", None, RowKey::name("ODFObject"), "Value", "Table");
    let pic = draw(&one_page(vec![turned]));
    assert!(pic.is(2.5, 2.9, PALE, 8) && pic.is(1.5, 1.2, PALE, 8), "the box stands up: 1.5 wide and 2 high about (2, 2)");
    assert!(pic.is_white(3.0, 2.0) && pic.is_white(2.0, 3.2) && pic.is_white(2.0, 0.8), "and its frame was 2 wide: nothing where it was");
    let flipped = object(1, 2.0, 1.0, 2.0, 1.5, "Table").cell("LocPinX", "1").cell("FlipX", "1");
    let pic = draw(&one_page(vec![flipped]));
    assert!(pic.is(1.15, 2.2, PALE, 6) && pic.is_white(0.9, 1.7) && pic.is_white(3.1, 1.7));
    for (w, h) in [(1.0e9, 1.0e9), (1.0e300, 2.0), (f64::MAX, f64::MAX), (1.0e-300, 1.0e-300)] {
        let pic = draw(&one_page(vec![object(1, 1.0, 1.0, w, h, "Table")]));
        let said = pic.r.count(SkipKind::ObjectPlaceholder) + pic.r.count(SkipKind::OutOfRange) + pic.r.count(SkipKind::GeometryFailed);
        assert!(said <= 2, "{w} by {h}: {:?}", pic.r.skipped);
    }
    for text in ["inf", "NaN", "-inf", ""] {
        let odd = shape_at(1, 1.0, 1.0, 1.0, 1.0).kind(ShapeType::Foreign).cell("Width", text).row_cell(
            "User",
            None,
            RowKey::name("ODFObject"),
            "Value",
            "Table",
        );
        let _ = draw(&one_page(vec![odd]));
    }
}

/// The label of a hostile length is cut: a label of a million characters is drawn (or not) in the time and the characters the frame has, and the box is
/// there.
#[test]
fn a_label_of_a_million_characters_is_cut_and_the_box_is_drawn() {
    let long = "W".repeat(1_000_000);
    let pic = draw(&one_page(vec![object(1, 1.0, 1.0, 2.0, 1.5, &long)]));
    assert!(pic.is(1.15, 2.2, PALE, 6));
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 1);
    assert_eq!(
        pic.r.count(SkipKind::GlyphsCut) + pic.r.count(SkipKind::TextCharBudget),
        0,
        "the label is cut by the placeholder, not by the page's budgets"
    );
}

/// The label is cut at 64 characters, and not at 63 or 65: the sixty-fourth character is drawn (a different one gives a different picture), the sixty-fifth is
/// not (a different one gives the same), and what comes after is as little drawn.
#[test]
fn a_label_is_cut_at_sixty_four_characters() {
    let of = |text: String| draw(&one_page(vec![object(1, 0.2, 1.0, 3.6, 1.5, &text)])).r.rgba_premul;
    let narrow = |n: usize, rest: &str| format!("{}{rest}", "i".repeat(n));
    assert_ne!(of(narrow(63, "X")), of(narrow(63, "Y")), "the sixty-fourth is drawn");
    assert_eq!(of(narrow(64, "X")), of(narrow(64, "Y")), "the sixty-fifth is not");
    assert_eq!(of(narrow(64, "XXXXXX")), of(narrow(64, "")), "nor what comes after it");
}

/// Counted cost: a label is laid out once for each label and size, however many frames hold it (2,000 placeholders of one label at one size are one
/// layout, 40 labels are 40), and the page's budget of characters for text bounds what a page of many different labels lays out; the box is drawn either
/// way, and a label past the budget is left off without a word about text (it is no text of the file's).
#[test]
fn the_labels_of_many_placeholders_are_laid_out_once_for_each_label_and_size_and_within_the_budget() {
    let page_of = |shapes: Vec<ShapeBuilder>| one_page(shapes);
    let mut renderer = Renderer::new();
    let same: Vec<ShapeBuilder> = (1..=2_000).map(|id| object(id, 1.0, 1.0, 1.2, 0.8, "Table")).collect();
    let pic = draw_with(&mut renderer, &page_of(same));
    assert_eq!(renderer.texts_laid_out(), 1, "one label at one size is one layout");
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 2_000);
    let mut renderer = Renderer::new();
    let many: Vec<ShapeBuilder> = (1..=40).map(|id| object(id, 1.0, 1.0, 1.2, 0.8, &format!("Kind {id}"))).collect();
    draw_with(&mut renderer, &page_of(many));
    assert_eq!(renderer.texts_laid_out(), 40);
    // A budget of 100 characters of labels (their own, beside the file's): the labels past it are not laid out, and the boxes are all there.
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { max_label_chars: 100, ..Limits::default() });
    let many: Vec<ShapeBuilder> = (1..=40).map(|id| object(id, 0.1 * f64::from(id % 30), 1.0, 1.2, 0.8, &format!("Object kind {id}"))).collect();
    let pic = draw_with(&mut renderer, &page_of(many));
    assert!(renderer.texts_laid_out() < 40 && renderer.texts_laid_out() >= 1, "{}", renderer.texts_laid_out());
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 40);
    assert_eq!(pic.r.count(SkipKind::TextCharBudget), 0, "a label is not text of the file's");
}

/// The budgets are exact, and counted (never timed): a label of five characters is laid out with a budget of five characters and not with four; and with a
/// time budget of nothing the first label is laid out (no time is spent yet) and the others, after it, are not.
#[test]
fn the_budgets_of_a_label_are_its_own_and_exact() {
    let laid = |limits: Limits, labels: &[&str]| {
        let mut renderer = Renderer::new();
        renderer.set_limits(limits);
        let shapes: Vec<ShapeBuilder> = labels.iter().enumerate().map(|(k, l)| object(k as u32 + 1, 1.0, 1.0, 1.2, 0.8, l)).collect();
        let pic = draw_with(&mut renderer, &one_page(shapes));
        assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), labels.len(), "the boxes are all there");
        renderer.texts_laid_out()
    };
    assert_eq!(laid(Limits { max_label_chars: 5, ..Limits::default() }, &["Table"]), 1, "five characters of five");
    assert_eq!(laid(Limits { max_label_chars: 4, ..Limits::default() }, &["Table"]), 0, "five of four");
    let none = Limits { label_time: Some(std::time::Duration::ZERO), ..Limits::default() };
    assert_eq!(laid(none, &["Table", "Scene", "Callout"]), 1, "the first, and none after the time is spent");
    // What was laid out is kept: a label seen before, with others between, is not laid out again.
    assert_eq!(laid(Limits::default(), &["Table", "Scene", "Table", "Scene"]), 2, "two labels, however often they come");
}

/// The label is never mirrored: a frame flipped across, down or both (about its middle, which leaves the box where it was) shows the label as the frame that
/// is not flipped does, letter for letter, and not turned about.
#[test]
fn the_label_is_never_mirrored_whatever_the_flips_of_the_shape() {
    let drawn = |flip_x: bool, flip_y: bool, label: &str| {
        let mut shape = object(1, 2.0, 1.75, 2.0, 1.5, label).cell("LocPinX", "1").cell("LocPinY", "0.75");
        if flip_x {
            shape = shape.cell("FlipX", "1");
        }
        if flip_y {
            shape = shape.cell("FlipY", "1");
        }
        draw(&one_page(vec![shape]))
    };
    let (upright, bare) = (drawn(false, false, "Practical"), drawn(false, false, " "));
    let ink = upright.r.rgba_premul.chunks(4).zip(bare.r.rgba_premul.chunks(4)).filter(|(a, b)| a != b).count();
    assert!(ink > 100, "the label has ink: {ink}");
    for (x, y) in [(true, false), (false, true), (true, true)] {
        let flipped = drawn(x, y, "Practical");
        let apart = upright.r.rgba_premul.chunks(4).zip(flipped.r.rgba_premul.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(apart * 10 <= ink, "flip x {x} y {y}: {apart} pixels differ from the upright box, whose label has {ink}");
    }
}

// ----: the labels have a budget of their own, and a placeholder outside the view costs nothing (M20) -----------------------

/// A text box of the file's own, below the placeholders of these tests: black text of 22 characters on a frame that nothing else covers.
fn file_text() -> ShapeBuilder {
    use diacraft_doc::RowKey;
    rect(900_000, 0.2, 0.2, 3.6, 0.6)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text("The file's own text...")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.3")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
}

/// `n` placeholders of one label at widths no two share (so that no layout is the cache's), drawn **before** the file's text box.
fn placeholders_then_the_text(n: u32) -> Vec<ShapeBuilder> {
    let mut shapes: Vec<ShapeBuilder> = (1..=n).map(|id| object(id, 0.2, 1.5, 1.0 + f64::from(id) * 1.0e-5, 1.2, "Embedded object")).collect();
    shapes.push(file_text());
    shapes
}

/// Whether the file's text was drawn: ink in the box it stands in, and no word of a budget about text.
fn file_text_is_drawn(pic: &Pic) -> bool {
    let in_its_box = (0..36).any(|k| pic.is_dark(0.25 + f64::from(k) * 0.1, 0.5));
    in_its_box && pic.r.count(SkipKind::TextCharBudget) == 0 && pic.r.count(SkipKind::TextBudget) == 0
}

/// **The characters**: placeholders that lay their labels out spend the labels' budget and not the file's. With the file's budget at
/// 100 characters, ten labels of fifteen (150) laid out first would have left the file's 22 for nothing.
#[test]
fn the_labels_of_placeholders_do_not_spend_the_characters_the_files_text_has() {
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { max_text_chars: 100, ..Limits::unlimited_time() });
    let pic = draw_with(&mut renderer, &one_page(placeholders_then_the_text(10)));
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 10);
    assert!(file_text_is_drawn(&pic), "the file's text is laid out in its own 100 characters: {:?}", pic.r.skipped);
    assert_eq!(renderer.texts_laid_out(), 11, "ten labels, each at a width of its own, and the text");
}

/// **The time** (the same finding): the labels' layouts are not charged to the time the file's text may take. With the file's text time at nothing, the first block
/// is laid out and the next refused, so a label laid out first would have taken the file's turn.
#[test]
fn the_labels_of_placeholders_do_not_spend_the_time_the_files_text_has() {
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { text_time: Some(std::time::Duration::ZERO), ..Limits::unlimited_time() });
    let pic = draw_with(&mut renderer, &one_page(placeholders_then_the_text(3)));
    assert!(file_text_is_drawn(&pic), "the file's text is the first block of the file's time: {:?}", pic.r.skipped);
}

/// **The review's input**: 27,000 placeholders of widths no two share, then one text box of 22 characters, drawn with no time limit (so that the frame's time
/// is not what ends it). At fifteen characters a label, 27,000 are 405,000: over the 400,000 characters a frame lays out, which is what took the file's text.
/// The file's text is drawn, every box is there, and the labels stop at their own budget and not at the file's.
#[test]
fn file_text_after_27000_placeholders_is_still_drawn() {
    // (The shape limit is not what this test meets.)
    const _: () = assert!(diacraft_render::limits::MAX_SHAPES_PER_FRAME > 27_001);
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits::unlimited_time());
    let pic = draw_with(&mut renderer, &one_page(placeholders_then_the_text(27_000)));
    assert_eq!(pic.r.count(SkipKind::ObjectPlaceholder), 27_000, "every box is there");
    assert!(file_text_is_drawn(&pic), "the file's text: {:?}", pic.r.skipped);
}

/// **M20**: a placeholder outside the view is no placeholder of the frame: no note and no layout, whatever its label. The view shows the left half of the page
/// (0 to 2 inches); a frame starting at 2.05 inches is outside it though inside the margin the walk gives a shape (which is how it meets the check at all), and
/// the same frame at 1 inch, inside the view, is noted and laid out once.
#[test]
fn a_placeholder_outside_the_view_is_not_noted_and_lays_nothing_out() {
    let half = |x: f64| {
        let doc = one_page(vec![object(1, x, 1.0, 0.8, 0.8, "Table")]);
        let view = diacraft_render::View { to_px: kurbo::Affine::new([100.0, 0.0, 0.0, -100.0, 0.0, 400.0]), width: 200, height: 400 };
        let mut renderer = Renderer::new();
        let rendered = renderer.render(&doc, 1, &view);
        (rendered.count(SkipKind::ObjectPlaceholder), renderer.texts_laid_out())
    };
    assert_eq!(half(1.0), (1, 1), "inside the view: noted, and its label laid out");
    assert_eq!(half(2.05), (0, 0), "outside it: nothing");
}
