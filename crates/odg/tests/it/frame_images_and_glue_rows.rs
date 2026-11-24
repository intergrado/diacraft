//! At the crate's own level (the engine's `picture_frame_images_and_extruded_glue` is the plain visit a person makes): what the reader gives, which the writer's rules rest on, and what the model writer
//! makes of a flat drawing's frame of two images. The inputs are written by hand from the OASIS ODF standard (Part 3); every read-back goes through the real reader.
//!
//! * **A geometry the reader stops at before its glue points** (an extrusion, a fontwork, a view box with no width) gives the shape the four standard rows and none of the points it lists, which
//!   is what the writer's count of the points a geometry lists that are rows (`listed_glue_points`) says.
//! * **A frame's kept child says which side of the shown one it stood on** (`Placed::after`), so that a writer gives the images back in the order the file had them.
//! * **A link that names no entry of the package a flat drawing is written as is written out of it**, and the pictures stay in the order they had.

use diacraft_doc::{Document, RawNode, Shape};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::flat::{Flat, base64_lines, frame, picture};

const LISTED: &str = "10800 0 0 10800 10800 21600 21600 10800";

/// The model of a drawing read from a package.
fn model(drawing: &Drawing) -> Document {
    drawing.read().0
}

fn first_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap()
}

/// How many rows the shape's `Connection` sections hold.
fn connection_rows(shape: &Shape) -> usize {
    shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Connection").map(|s| s.rows.len()).sum()
}

fn custom_shape(geometry_attrs: &str, view_box: &str) -> String {
    format!(
        concat!(
            r#"<draw:custom-shape draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm">"#,
            r#"<draw:enhanced-geometry svg:viewBox="{vb}" draw:type="diamond"{attrs} draw:glue-points="{points}" "#,
            r#"draw:enhanced-path="M 10800 0 L 21600 10800 10800 21600 0 10800 10800 0 Z N"/></draw:custom-shape>"#
        ),
        vb = view_box,
        attrs = geometry_attrs,
        points = LISTED,
    )
}

/// **The reader makes the four standard rows and none of the listed ones** of a geometry it stops at before its glue points (the writer counts the points such a geometry lists as none), and
/// the standard rows and the four listed ones of any other.
#[test]
fn the_reader_reads_no_listed_glue_point_of_an_extrusion_a_fontwork_or_a_box_with_no_width() {
    for (what, attrs, view_box, rows) in [
        ("a plain geometry", "", "0 0 21600 21600", 8),
        ("an extrusion switched off", r#" draw:extrusion="false""#, "0 0 21600 21600", 8),
        ("an extrusion", r#" draw:extrusion="true""#, "0 0 21600 21600", 4),
        ("a fontwork", r#" draw:text-path="true""#, "0 0 21600 21600", 4),
        ("a view box with no width", "", "0 0 0 21600", 4),
        ("a view box with no height", "", "0 0 21600 0", 4),
        ("an unreadable view box", "", "0 0 21600", 8),
    ] {
        let doc = model(&Drawing::a4().page(&custom_shape(attrs, view_box)));
        assert_eq!(connection_rows(first_shape(&doc)), rows, "{what}");
    }
}

fn svg() -> String {
    base64_lines(br#"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="2"/>"#, 72, "\n")
}

fn inline_image(mime: &str, bytes: &[u8]) -> String {
    format!(r#"<draw:image draw:mime-type="{mime}"><office:binary-data>{}</office:binary-data></draw:image>"#, base64_lines(bytes, 72, "\n"))
}

fn inline_svg() -> String {
    format!(r#"<draw:image draw:mime-type="image/svg+xml"><office:binary-data>{}</office:binary-data></draw:image>"#, svg())
}

/// The kept children of a shape: each element's name and its `after`.
fn kept_after(shape: &Shape) -> Vec<(String, u32)> {
    shape
        .ext
        .get()
        .map(|e| {
            e.odf
                .kept
                .kept
                .iter()
                .filter_map(|p| if let RawNode::Element(el) = &p.node { Some((el.name.to_string(), p.after)) } else { None })
                .collect()
        })
        .unwrap_or_default()
}

/// **A frame's kept child says which side of the child the frame shows it stood on**: 0 before it and 1 after it, in a flat drawing and in a package.
#[test]
fn a_frames_kept_images_say_which_side_of_the_shown_picture_they_stood_on() {
    let png = picture(300);
    // SVG, PNG (shown), SVG, SVG.
    let images = format!("{}{}{}{}", inline_svg(), inline_image("image/png", &png), inline_svg(), inline_svg());
    for (form, doc) in [("flat", Flat::of(&Drawing::a4().page(&frame(&images))).read().0), ("package", model(&Drawing::a4().page(&frame(&images))))] {
        let shape = first_shape(&doc);
        assert_eq!(
            shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()),
            Some(png.clone()),
            "{form}: the frame shows the PNG"
        );
        assert_eq!(
            kept_after(shape),
            vec![("draw:image".to_owned(), 0), ("draw:image".to_owned(), 1), ("draw:image".to_owned(), 1)],
            "{form}: the first image stood before the shown one, the others after it"
        );
    }
}

/// The `content.xml` of a package.
fn content(bytes: &[u8]) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap()
}

/// The `draw:image` elements of the first frame of a part, each as `(link, holds its picture)`.
fn images_written(content: &str) -> Vec<(Option<String>, bool)> {
    let root = diacraft_container::xml::parse(content.as_bytes()).unwrap();
    fn walk<'a>(el: &'a diacraft_container::xml::El, out: &mut Vec<&'a diacraft_container::xml::El>) {
        for child in el.elements() {
            out.push(child);
            walk(child, out);
        }
    }
    let mut list = Vec::new();
    walk(&root, &mut list);
    let frame = list.iter().find(|el| el.name == "draw:frame").expect("a frame");
    frame
        .elements()
        .filter(|el| el.name == "draw:image")
        .map(|el| (el.attr("xlink:href").map(str::to_owned), el.child("binary-data").is_some()))
        .collect()
}

/// **The model writer writes a flat drawing's frame of two images with both, the link out of the package, and the notes empty**: a frame of an SVG link, the PNG it shows and another SVG
/// link, whose links are a file beside the document in the flat file and name no entry of the package.
#[test]
fn the_model_writer_writes_a_flat_frame_of_two_images_with_both_and_its_links_out_of_the_package() {
    let png = picture(300);
    let link = |href: &str| {
        format!(r#"<draw:image xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type="image/svg+xml"/>"#)
    };
    let images = format!("{}{}{}", link("plan.svg"), inline_image("image/png", &png), link("../beside/other.svg"));
    let doc = Flat::of(&Drawing::a4().page(&frame(&images))).read().0;
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    assert_eq!(written.notes, Vec::new(), "nothing is left out");
    let written_images = images_written(&content(&written.bytes));
    assert_eq!(written_images.len(), 3, "{written_images:?}");
    assert_eq!(written_images[0], (Some("../plan.svg".to_owned()), false), "a link that names no entry of the package is a link out of it");
    assert!(
        written_images[1].0.as_deref().is_some_and(|h| h.starts_with("Pictures/")) && !written_images[1].1,
        "the picture the frame shows is an entry: {written_images:?}"
    );
    assert_eq!(
        written_images[2],
        (Some("../beside/other.svg".to_owned()), false),
        "a link out of the package stays as it is, and stands after the shown picture"
    );
    // The package holds the PNG once, and read back the frame shows it and keeps both links as the writer wrote them.
    let back = read(&written.bytes, &Limits::default()).unwrap().0;
    let shape = first_shape(&back);
    assert_eq!(shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()), Some(png));
    assert_eq!(kept_after(shape), vec![("draw:image".to_owned(), 0), ("draw:image".to_owned(), 1)]);
}

/// **A package's link that names an entry stays as it is**: the drawing is a package whose frame holds an SVG entry and the PNG it shows, and its model written again names the SVG entry
/// as it did (the link is a path in the package that the package has).
#[test]
fn a_links_name_of_an_entry_the_package_has_is_written_as_it_was() {
    let png = picture(300);
    let svg_bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="2"/>"#.to_vec();
    let link = |href: &str, mime: &str| {
        format!(r#"<draw:image xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type="{mime}"/>"#)
    };
    let images = format!("{}{}", link("Pictures/p1.svg", "image/svg+xml"), link("Pictures/p2.png", "image/png"));
    let mut drawing = Drawing::a4().page(&frame(&images));
    drawing.files.push(("Pictures/p1.svg".to_owned(), svg_bytes));
    drawing.files.push(("Pictures/p2.png".to_owned(), png));
    let doc = model(&drawing);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let written_images = images_written(&content(&written.bytes));
    assert_eq!(written_images.len(), 2, "{written_images:?}");
    assert_eq!(written_images[0], (Some("Pictures/p1.svg".to_owned()), false), "the SVG entry is named as it was");
    assert_eq!(written_images[1], (Some("Pictures/p2.png".to_owned()), false), "and the picture the frame shows keeps its own entry");
}

/// **Two styles of different families may share a name** (found by the fuzz run of this task on a flat drawing whose style `gr1` had a family of garbage beside the graphic style `gr1` of its frame,
/// and present on `44f9d1ae`): the names of ODF's styles are unique within a family, and the save's check of the styles it made found a graphic style `gr1` twice when a style of another
/// family had the name, and refused the save as its own fault. A kept style of the family `presentation` or a table's, named as a shape's graphic style is, is written beside it.
#[test]
fn a_style_of_another_family_with_the_name_of_a_graphic_style_does_not_refuse_the_save() {
    for family in ["presentation", "table-cell", "chart", "graphiMETA-INFc"] {
        let frame_with_style = r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/>"#;
        let drawing = Drawing::a4()
            .auto(&format!(r#"<style:style style:name="gr1" style:family="graphic"/><style:style style:name="gr1" style:family="{family}" style:parent-style-name="standard"/>"#))
            .page(frame_with_style);
        let doc = model(&drawing);
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{family}: the save is refused: {e}"));
        assert!(read(&written.bytes, &Limits::default()).is_ok(), "{family}: the package written reads again");
    }
}

/// **Many frames, each with a link out of the package and the picture it shows, are written once each** (a count and no clock: the save cost is the frames', and each link is one look in the
/// names the save holds): 3,000 frames of the vector, each link a link out of the package, the notes empty, and every picture an entry of the package.
#[test]
fn thousands_of_frames_of_two_images_are_each_written_with_both() {
    let png = picture(300);
    let link = r#"<draw:image xlink:href="plan.svg" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type="image/svg+xml"/>"#;
    let one = frame(&format!("{link}{}", inline_image("image/png", &png)));
    let frames = 3_000;
    let doc = Flat::of(&Drawing::a4().page(&one.repeat(frames))).read().0;
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    assert_eq!(written.notes, Vec::new(), "nothing is left out");
    let text = content(&written.bytes);
    assert_eq!(text.matches(r#"xlink:href="../plan.svg""#).count(), frames, "each link is a link out of the package");
    assert_eq!(text.matches(r#"xlink:href="Pictures/"#).count(), frames, "each frame names the entry of the picture it shows");
    let package = diacraft_odg::package::Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(package.parts().filter(|name| name.starts_with("Pictures/")).count(), 1, "the same bytes are one entry");
}
