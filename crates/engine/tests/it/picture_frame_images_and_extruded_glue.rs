//! The two regressions of the gate on `44f9d1ae`, each built in code from the structure of its composed vector, by the OASIS ODF standard. Each test is the plain visit a
//! person can make (`file.open`, then `file.save_as` to `.diacraft`, then from it to the file's own format, one leg a session), or the direct convert (one leg).
//!
//! - **A flat drawing lost its pictures** (13 corpus files): a frame of two images, whose first the reader does not draw (an SVG or a PDF, linked or inline) and whose second is a PNG, was written as the
//!   file's own first image, and the PNG was dropped (`pictures`). A bare relative link of such an image (`plan.svg`) was written into the package as it was, which in a package names an entry
//!   it does not have (LibreOffice 26.8.2 then said a load error).
//! - **An extruded custom shape that lists glue points was written as if its rows were its own** (`own-*-3d`): the file's `draw:enhanced-geometry` says `draw:extrusion="true"`, which the reader does not
//!   draw and reads no glue point of, so the shape has the four standard rows and the glue points it lists count as the rows it has; the writer took them for the rows of a shape numbered from 0 and wrote
//!   the marker and the connector's end 4 more (4 became 8, and came back as 4 on the next visit).

use crate::native_common::Dir;
use crate::odg_build::Drawing;
use crate::save_check_misses::{all, leg, named, parse};

// ------------------------------------------------------------------------------------------------------------------ the pictures

/// A 1 by 1 PNG, base64.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGNgAAAAAgABSK+kcQAAAABJRU5ErkJggg==";
/// An SVG of one rectangle, base64: a picture the reader does not draw.
const SVG: &str = "PD94bWwgdmVyc2lvbj0iMS4wIiBlbmNvZGluZz0iVVRGLTgiPz48c3ZnIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgd2lkdGg9IjMiIGhlaWdodD0iMiI+PHJlY3Qgd2lkdGg9IjMiIGhlaWdodD0iMiIgZmlsbD0iIzNhNyIvPjwvc3ZnPg==";
/// A stub of a PDF, base64: a picture the reader does not draw either.
const PDF: &str = "JVBERi0xLjQKMSAwIG9iajw8Pj5lbmRvYmoKdHJhaWxlcjw8Pj4KJSVFT0YK";
/// The namespace `loext` stands for in the files of the program that writes `loext:page-number` on a picture.
const LOEXT: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";

/// A `draw:image` that holds its picture: the media type, the bytes (base64) and, when given, the attributes beside them.
fn inline(mime: &str, base64: &str, attrs: &str) -> String {
    format!(r#"<draw:image draw:mime-type="{mime}"{attrs}><office:binary-data>{base64}</office:binary-data></draw:image>"#)
}

/// A `draw:image` that is a link: `href`, as the file spells it.
fn linked(href: &str, mime: &str) -> String {
    format!(r#"<draw:image xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type="{mime}"/>"#)
}

/// One `draw:frame` that holds `images`, in the order given.
fn frame(images: &[String]) -> String {
    format!(r#"<draw:frame draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="4cm">{}</draw:frame>"#, images.concat())
}

/// The drawing as one flat document, with the prefix `loext` declared on its root when `with_loext` says so.
fn flat(page: &str, with_loext: bool) -> Vec<u8> {
    let text = String::from_utf8(Drawing::new().page(page).flat_bytes()).unwrap();
    let text = if with_loext { text.replacen("<office:document ", &format!("<office:document xmlns:loext=\"{LOEXT}\" "), 1) } else { text };
    text.into_bytes()
}

/// What one `draw:image` of a written package says: its `xlink:href` and whether it holds its picture.
#[derive(Debug, PartialEq, Eq)]
struct Image {
    href: Option<String>,
    inline: bool,
    mime: Option<String>,
}

/// The `draw:image` children of the first `draw:frame` of a package's `content.xml`, in order.
fn images_of(bytes: &[u8]) -> Vec<Image> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    let Some(frame) = list.iter().find(|el| el.name == "draw:frame") else { return Vec::new() };
    frame
        .elements()
        .filter(|el| el.name == "draw:image")
        .map(|el| Image {
            href: el.attr("xlink:href").map(str::to_owned),
            inline: el.child("binary-data").is_some(),
            mime: el.attr("draw:mime-type").map(str::to_owned),
        })
        .collect()
}

/// The entries of a package that are PNG pictures (by their first bytes), by name.
fn png_entries(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    package.parts().filter(|name| package.part(name).is_some_and(|b| b.starts_with(&[0x89, b'P', b'N', b'G']))).map(str::to_owned).collect()
}

/// The links of a package's `content.xml` and `styles.xml` that are a path in the package and name no entry of it (a LibreOffice-free check that every part name the content refers to exists): an
/// `xlink:href` is a path of the package unless it has a scheme, starts with `/`, `#` or `../`, or is empty; one that names an object's replacement is not counted either, as LibreOffice opens it.
fn dangling_links(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut out = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = parse(&crate::odg_build::part_of(bytes, part));
        let mut list = Vec::new();
        all(&root, &mut list);
        for el in list {
            let Some(href) = el.attr("xlink:href") else { continue };
            let path = href.strip_prefix("./").unwrap_or(href);
            let has_scheme = path.split_once(':').is_some_and(|(scheme, _)| scheme.starts_with(|c: char| c.is_ascii_alphabetic()));
            if path.is_empty() || has_scheme || path.starts_with(['/', '#']) || path.starts_with("../") || path.starts_with("ObjectReplacements/") {
                continue;
            }
            if package.part(path).is_none() && !package.parts().any(|name| name == path) {
                out.push(format!("{part}: {href}"));
            }
        }
    }
    out
}

/// The plain visit of `source` (a file called `a.<ext>`): to `.diacraft` and from it to `.odg`. What each leg named, and the package.
fn plain_visit(source: &[u8], ext: &str) -> (Vec<String>, Vec<u8>) {
    let dir = Dir::new();
    dir.write(&format!("a.{ext}"), source);
    let mut said = named(&leg(&dir, &format!("a.{ext}"), "b.diacraft"));
    said.extend(named(&leg(&dir, "b.diacraft", "c.odg")));
    (said, dir.read("c.odg"))
}

/// The direct convert of `source` (a file called `a.<ext>`) to `.odg`.
fn direct(source: &[u8], ext: &str) -> (Vec<String>, Vec<u8>) {
    let dir = Dir::new();
    dir.write(&format!("a.{ext}"), source);
    let said = named(&leg(&dir, &format!("a.{ext}"), "b.odg"));
    (said, dir.read("b.odg"))
}

/// What a frame of two images, the first of them a picture the reader does not draw and the second the PNG, comes to in the package written: **both stay**. The PNG is an entry of the package that the
/// frame's second image names; the first is what the file had (its picture inline, or its link, which is a link and no name of an entry), and nothing is said of a picture.
fn assert_both_kept(written: &[u8], said: &[String], first: &Image, what: &str) {
    let images = images_of(written);
    assert_eq!(images.len(), 2, "{what}: the frame holds both images: {images:?}");
    assert_eq!(&images[0], first, "{what}: the first image is the file's own");
    let png = png_entries(written);
    assert_eq!(png.len(), 1, "{what}: the package holds the PNG once: {png:?}");
    assert_eq!(images[1].href.as_deref(), png.first().map(String::as_str), "{what}: the second image names the PNG's entry");
    assert!(!images[1].inline, "{what}: the PNG is an entry, not inline data");
    assert_eq!(dangling_links(written), Vec::<String>::new(), "{what}: every name of an entry the content refers to is one");
    assert!(!said.iter().any(|c| c == "pictures"), "{what}: the check names {said:?}");
}

/// The first image of each form of the vectors, with the picture the file holds for it: `(what, first image of the source, the image as written)`.
fn cases() -> Vec<(&'static str, String, Image)> {
    let svg_inline = Image { href: None, inline: true, mime: Some("image/svg+xml".to_owned()) };
    let pdf_inline = Image { href: None, inline: true, mime: Some("application/pdf".to_owned()) };
    vec![
        // A bare relative link, which in a flat file is a file beside the document: in a package, a link out of it.
        (
            "fodg-frame-link-then-png",
            linked("plan.svg", "image/svg+xml"),
            Image { href: Some("../plan.svg".to_owned()), inline: false, mime: Some("image/svg+xml".to_owned()) },
        ),
        (
            "fodg-frame-ref-then-inline",
            linked("Pictures/p1.svg", "image/svg+xml"),
            Image { href: Some("../Pictures/p1.svg".to_owned()), inline: false, mime: Some("image/svg+xml".to_owned()) },
        ),
        ("fodg-frame-svg-then-png", inline("image/svg+xml", SVG, ""), svg_inline),
        ("fodg-frame-pdf-then-png", inline("application/pdf", PDF, ""), pdf_inline),
    ]
}

/// **A flat frame of two images keeps both on the plain visit** (`fodg-frame-link-then-png`, `-ref-then-inline`, `-svg-then-png`, `-pdf-then-png`): the PNG was dropped, and a link was written as
/// the name of an entry the package does not have. Red on `44f9d1ae`.
#[test]
fn a_flat_frame_of_two_images_keeps_both_on_the_plain_visit() {
    for (what, first, expected) in cases() {
        let source = flat(&frame(&[first, inline("image/png", PNG, "")]), false);
        let (said, written) = plain_visit(&source, "fodg");
        assert_both_kept(&written, &said, &expected, what);
    }
}

/// The same on the direct convert (a flat drawing saved as an `.odg`).
#[test]
fn a_flat_frame_of_two_images_keeps_both_on_the_direct_convert() {
    for (what, first, expected) in cases() {
        let source = flat(&frame(&[first, inline("image/png", PNG, "")]), false);
        let (said, written) = direct(&source, "fodg");
        assert_both_kept(&written, &said, &expected, what);
    }
}

/// **Three visits in a row keep both**: the second and the third leave the package as the first made it.
#[test]
fn a_frame_of_two_images_stays_as_it_is_through_three_visits() {
    for (what, first, expected) in cases() {
        let source = flat(&frame(&[first, inline("image/png", PNG, "")]), false);
        let dir = Dir::new();
        dir.write("v0.fodg", &source);
        let mut said = named(&leg(&dir, "v0.fodg", "d0.diacraft"));
        said.extend(named(&leg(&dir, "d0.diacraft", "v1.odg")));
        for k in 1..3 {
            said.extend(named(&leg(&dir, &format!("v{k}.odg"), &format!("d{k}.diacraft"))));
            said.extend(named(&leg(&dir, &format!("d{k}.diacraft"), &format!("v{}.odg", k + 1))));
        }
        for k in 1..=3 {
            assert_both_kept(&dir.read(&format!("v{k}.odg")), &said, &expected, &format!("{what}, visit {k}"));
        }
    }
}

/// **The picture the file holds beside an attribute of another vocabulary** (`fodg-frame-pdf-loext-then-png`, `-svg-loext-then-png`): the prefix `loext` is bound by the root of the file only. Both stay.
#[test]
fn a_flat_frame_whose_first_image_names_an_attribute_of_another_vocabulary_keeps_both() {
    for (what, mime, data) in [("fodg-frame-pdf-loext-then-png", "application/pdf", PDF), ("fodg-frame-svg-loext-then-png", "image/svg+xml", SVG)] {
        let first = inline(mime, data, r#" loext:page-number="1""#);
        let source = flat(&frame(&[first, inline("image/png", PNG, "")]), true);
        let (said, written) = plain_visit(&source, "fodg");
        let images = images_of(&written);
        assert_eq!(images.len(), 2, "{what}: {images:?}");
        assert_eq!((images[0].inline, images[0].mime.as_deref()), (true, Some(mime)), "{what}");
        assert_eq!(png_entries(&written).len(), 1, "{what}");
        assert_eq!(images[1].href.as_deref(), png_entries(&written).first().map(String::as_str), "{what}");
        assert!(dangling_links(&written).is_empty(), "{what}");
        assert!(!said.iter().any(|c| c == "pictures"), "{what}: {said:?}");
    }
}

/// The package form of the same frame (`odg-frame-svg-then-png-package`): the first image names an entry of the package that holds an SVG, the second one that holds the PNG. The plain visit
/// kept the SVG and dropped the PNG; both stay, each under the name it had.
#[test]
fn a_package_frame_of_two_images_keeps_both_on_the_plain_visit() {
    let svg = base64_bytes(SVG);
    let png = base64_bytes(PNG);
    let page = frame(&[linked("Pictures/p1.svg", "image/svg+xml"), linked("Pictures/p2.png", "image/png")]);
    let source = Drawing::new().page(&page).file("Pictures/p1.svg", &svg).file("Pictures/p2.png", &png).bytes();
    let (said, written) = plain_visit(&source, "odg");
    let images = images_of(&written);
    assert_eq!(images.len(), 2, "{images:?}");
    assert_eq!(images[0].href.as_deref(), Some("Pictures/p1.svg"));
    assert_eq!(images[1].href.as_deref(), Some("Pictures/p2.png"));
    assert!(png_entries(&written).contains(&"Pictures/p2.png".to_owned()));
    assert!(dangling_links(&written).is_empty(), "{:?}", dangling_links(&written));
    assert!(!said.iter().any(|c| c == "pictures"), "{said:?}");
}

/// **A kept image that names an entry the package has stays as it is, even when the entry is the picture another frame shows** (two frames of two PNGs each, every PNG an entry of the package, each
/// frame showing one and keeping a link to the other): the entries are the pictures' own, written under the names they had, and no link is made a link out of the package.
#[test]
fn a_kept_link_to_the_picture_another_frame_shows_is_a_link_to_an_entry_of_the_package() {
    let (a, b) = (base64_bytes(PNG_2X2), base64_bytes(PNG_3X2));
    let page = format!(
        "{}{}",
        frame(&[linked("Pictures/p2.png", "image/png"), linked("Pictures/p3.png", "image/png")]),
        frame(&[linked("Pictures/p3.png", "image/png"), linked("Pictures/p2.png", "image/png")])
    );
    let source = Drawing::new().page(&page).file("Pictures/p2.png", &a).file("Pictures/p3.png", &b).bytes();
    let (said, written) = plain_visit(&source, "odg");
    let content = crate::odg_build::part_of(&written, "content.xml");
    assert!(!content.contains("../Pictures"), "no link of a picture the package has is made a link out of it: {content}");
    assert_eq!(dangling_links(&written), Vec::<String>::new());
    assert!(!said.iter().any(|c| c == "pictures"), "the check names {said:?}");
}

/// The PNG of 2 by 2 pixels and the PNG of 3 by 2, base64: two pictures the reader draws, which a frame may hold both of.
const PNG_2X2: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR42mNgYDgRxXCCQaPiRFPFCQAcEAUVJX9h5QAAAABJRU5ErkJggg==";
const PNG_3X2: &str = "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAIAAAASFvFNAAAAHUlEQVR42mNgYDgRxXBiC8MJBo2KE00VJ+5UnAAARxgIrU/u7roAAAAASUVORK5CYII=";

/// The picture the first shape of the first page of a package shows, by its bytes.
fn shown_picture(bytes: &[u8]) -> Vec<u8> {
    let (doc, _) = crate::odg_build::read_back(bytes);
    let page = doc.pages.get(0).expect("a page");
    let shape = page.shapes.get(0).expect("a shape");
    shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()).expect("the frame shows a picture")
}

/// **The images of a frame come back in the order the file had them** (`fodg-frame-two-inline`: two PNGs, the frame shows the first and keeps the second): the kept image was written before
/// the frame's own, so the frame showed the other picture on its next read and the check named `pictures`. A kept image that stood after the shown one is written after it, and one that stood
/// before it, before.
#[test]
fn the_images_of_a_frame_come_back_in_the_order_the_file_had_them() {
    let (a, b, svg) = (inline("image/png", PNG_2X2, ""), inline("image/png", PNG_3X2, ""), inline("image/svg+xml", SVG, ""));
    let shown = base64_bytes(PNG_2X2);
    // The images of the source, and where the one the frame shows (the 2 by 2 PNG, the first the reader draws) stands among them.
    for (what, images, own_at) in [
        ("two PNGs", vec![a.clone(), b.clone()], 0),
        ("a PNG then an SVG", vec![a.clone(), svg.clone()], 0),
        ("an SVG then a PNG", vec![svg.clone(), a.clone()], 1),
        ("a PNG, an SVG and a PNG", vec![a.clone(), svg.clone(), b.clone()], 0),
        ("an SVG, a PNG and an SVG", vec![svg.clone(), a.clone(), svg.clone()], 1),
    ] {
        let (said, written) = plain_visit(&flat(&frame(&images), false), "fodg");
        let found = images_of(&written);
        assert_eq!(found.len(), images.len(), "{what}: {found:?}");
        assert_eq!(shown_picture(&written), shown, "{what}: the frame shows the picture it showed");
        // The frame's own picture is an entry of the package, and stands where the picture it shows stood among the images.
        let own = found.iter().position(|i| !i.inline && i.href.as_deref().is_some_and(|h| h.starts_with("Pictures/")));
        assert_eq!(own, Some(own_at), "{what}: {found:?}");
        assert!(!said.iter().any(|c| c == "pictures"), "{what}: the check names {said:?}");
    }
}

/// The controls: a frame of one inline PNG, in a flat file and in a package, is written and linked as before.
#[test]
fn a_frame_of_one_png_is_still_written_and_linked() {
    let flat_source = flat(&frame(&[inline("image/png", PNG, "")]), false);
    let (said, written) = plain_visit(&flat_source, "fodg");
    let images = images_of(&written);
    assert_eq!(images.len(), 1, "{images:?}");
    assert_eq!(images[0].href.as_deref(), png_entries(&written).first().map(String::as_str));
    assert!(dangling_links(&written).is_empty());
    assert!(!said.iter().any(|c| c == "pictures"), "{said:?}");
    let png = base64_bytes(PNG);
    let package = Drawing::new().page(&frame(&[linked("Pictures/p2.png", "image/png")])).file("Pictures/p2.png", &png).bytes();
    let (said, written) = plain_visit(&package, "odg");
    assert_eq!(images_of(&written).len(), 1);
    assert!(png_entries(&written).contains(&"Pictures/p2.png".to_owned()));
    assert!(!said.iter().any(|c| c == "pictures"), "{said:?}");
}

/// Base64 (the standard alphabet, with padding) decoded: the vectors' pictures as bytes.
fn base64_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|c| *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            _ => 63,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------ the glue marker

/// The vector's four listed glue points on a custom shape (`draw:glue-points`, 8 numbers): the middles of the top, the left side, the bottom and the right side.
const LISTED: &str = "10800 0 0 10800 10800 21600 21600 10800";

/// The vector's structure: a rectangle (`id2`) and a custom shape (`id1`) whose geometry lists four glue points and has `geometry_attrs` beside them (`draw:extrusion="true"`), and, when `end` is
/// given, a line connector from the rectangle's glue point 1 to the shape's glue point `end`. The shape's style is `style`.
fn shapes(geometry_attrs: &str, end: Option<u32>, style: &str, own_point: bool) -> Vec<u8> {
    shapes_in(geometry_attrs, "0 0 21600 21600", end, style, own_point)
}

/// [`shapes`] with the geometry's view box given.
fn shapes_in(geometry_attrs: &str, view_box: &str, end: Option<u32>, style: &str, own_point: bool) -> Vec<u8> {
    let own = if own_point { r#"<draw:glue-point draw:id="8" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="auto"/>"# } else { "" };
    let rect = r#"<draw:rect draw:id="id2" xml:id="id2" draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="2cm"/>"#;
    let shape = format!(
        concat!(
            r#"<draw:custom-shape draw:id="id1" xml:id="id1" draw:style-name="{style}" svg:x="9cm" svg:y="2cm" svg:width="4cm" svg:height="2cm">{own}<text:p>Gate</text:p>"#,
            r#"<draw:enhanced-geometry svg:viewBox="{view_box}" draw:type="diamond"{attrs} draw:glue-points="{points}" "#,
            r#"draw:enhanced-path="M 10800 0 L 21600 10800 10800 21600 0 10800 10800 0 Z N"/></draw:custom-shape>"#
        ),
        style = style,
        own = own,
        attrs = geometry_attrs,
        view_box = view_box,
        points = LISTED,
    );
    let connector = end
        .map(|end| {
            format!(
                concat!(
                    r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="5cm" svg:y1="3cm" svg:x2="11cm" svg:y2="2cm" draw:start-shape="id2" draw:start-glue-point="1" "#,
                    r#"draw:end-shape="id1" draw:end-glue-point="{end}"/>"#
                ),
                end = end
            )
        })
        .unwrap_or_default();
    let mut drawing = Drawing::new();
    // A style that says the rows start at 0 with the first listed glue point (DiaCraft's marker `diacraft:glue="own"`, declared where it is used).
    drawing.auto.push_str(
        r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties xmlns:diacraft="urn:app.diacraft:extension" diacraft:glue="own"/></style:style>"#,
    );
    drawing.page(&format!("{rect}{shape}{connector}")).bytes()
}

/// The `draw:end-glue-point` of each `draw:connector` of a package's `content.xml`.
fn ends(bytes: &[u8]) -> Vec<String> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter().filter(|el| el.name == "draw:connector").map(|el| el.attr("draw:end-glue-point").unwrap_or_default().to_owned()).collect()
}

/// Whether the package's `content.xml` holds a style that says `diacraft:glue="own"` **beyond the one the vector holds itself** (`gr3`, which no shape names: a visit keeps a style nothing names
/// so that one comes back and is no marker the file did not have).
fn wrote_a_marker(bytes: &[u8]) -> bool {
    crate::odg_build::part_of(bytes, "content.xml").matches(r#"diacraft:glue="own""#).count() > 1
}

/// `visits` plain visits of `source`, an `.odg` called `v0.odg`: what each leg named, and the directory that holds `v0.odg`, `v1.odg`, ... and the `.diacraft` files between them.
fn visits(source: &[u8], visits: usize) -> (Vec<Vec<String>>, Dir) {
    let dir = Dir::new();
    dir.write("v0.odg", source);
    let mut legs = Vec::new();
    for k in 0..visits {
        legs.push(named(&leg(&dir, &format!("v{k}.odg"), &format!("d{k}.diacraft"))));
        legs.push(named(&leg(&dir, &format!("d{k}.diacraft"), &format!("v{}.odg", k + 1))));
    }
    (legs, dir)
}

/// The classes a leg named that this task is about: a connection that differs, a formula that is lost.
fn about_glue(legs: &[Vec<String>]) -> Vec<&str> {
    legs.iter().flatten().map(String::as_str).filter(|c| matches!(*c, "connections" | "formulas")).collect()
}

/// **An extruded custom shape that lists glue points is written with no marker the file did not have** (`own-lone-process-3d`, `own-lone-diamond-3d`): the plain visit wrote the shape's style with
/// `diacraft:glue="own"` and the check named `connections` (4) and `formulas` (6). Red on `44f9d1ae`.
#[test]
fn an_extruded_shape_that_lists_glue_points_gets_no_marker_on_the_plain_visit() {
    for attrs in [r#" draw:extrusion="true""#, r#" draw:text-path="true""#] {
        let (legs, dir) = visits(&shapes(attrs, None, "gr1", false), 3);
        for k in 1..=3 {
            assert!(!wrote_a_marker(&dir.read(&format!("v{k}.odg"))), "{attrs}: visit {k} wrote a marker the file did not have");
        }
        assert_eq!(about_glue(&legs), Vec::<&str>::new(), "{attrs}: the legs named {legs:?}");
    }
}

/// **A connector glued to the extruded shape keeps its end** (`own-glued-diamond-3d`): the end 4 was written 8, past the shape's last listed point (7), and came back as 4 on the next visit.
#[test]
fn a_connector_end_on_an_extruded_shape_keeps_its_index_through_three_visits() {
    for attrs in [r#" draw:extrusion="true""#, r#" draw:text-path="true""#] {
        for end in [4, 5, 2, 0] {
            let (legs, dir) = visits(&shapes(attrs, Some(end), "gr1", false), 3);
            for k in 1..=3 {
                let written = dir.read(&format!("v{k}.odg"));
                assert_eq!(ends(&written), vec![end.to_string()], "{attrs}: the end {end} after {k} visits");
                assert!(!wrote_a_marker(&written), "{attrs}: the end {end} after {k} visits wrote a marker");
            }
            assert_eq!(about_glue(&legs), Vec::<&str>::new(), "{attrs}: the end {end}: the legs named {legs:?}");
        }
    }
}

/// **A glue point of the extruded shape's own** (a `draw:glue-point` of id 8, the shape lists four more): its end keeps the index too, and no marker is written.
#[test]
fn a_connector_end_on_an_extruded_shapes_own_glue_point_keeps_its_index() {
    for end in [8, 2] {
        let (legs, dir) = visits(&shapes(r#" draw:extrusion="true""#, Some(end), "gr1", true), 3);
        for k in 1..=3 {
            let written = dir.read(&format!("v{k}.odg"));
            assert_eq!(ends(&written), vec![end.to_string()], "the end {end} after {k} visits");
            assert!(!wrote_a_marker(&written), "the end {end} after {k} visits wrote a marker");
        }
        assert_eq!(about_glue(&legs), Vec::<&str>::new(), "the end {end}: {legs:?}");
    }
}

/// **A geometry whose view box has no width draws as its frame and reads no glue point either** (the reader stops before them): the same bug, found beside the vectors.
#[test]
fn a_shape_whose_view_box_is_empty_gets_no_marker_and_keeps_its_connector_end() {
    for end in [4, 2] {
        let (legs, dir) = visits(&shapes_in("", "0 0 0 21600", Some(end), "gr1", false), 3);
        for k in 1..=3 {
            let written = dir.read(&format!("v{k}.odg"));
            assert_eq!(ends(&written), vec![end.to_string()], "the end {end} after {k} visits");
            assert!(!wrote_a_marker(&written), "the end {end} after {k} visits wrote a marker");
        }
        assert_eq!(about_glue(&legs), Vec::<&str>::new(), "the end {end}: {legs:?}");
    }
}

/// The controls: the same shape with no extrusion keeps the index and writes no marker, as before.
#[test]
fn the_flat_twin_of_the_extruded_shape_keeps_its_index() {
    for end in [4, 5, 2] {
        let (legs, dir) = visits(&shapes("", Some(end), "gr1", false), 3);
        for k in 1..=3 {
            let written = dir.read(&format!("v{k}.odg"));
            assert_eq!(ends(&written), vec![end.to_string()], "the end {end} after {k} visits");
            assert!(!wrote_a_marker(&written), "the end {end} after {k} visits");
        }
        assert_eq!(about_glue(&legs), Vec::<&str>::new(), "{legs:?}");
    }
}
