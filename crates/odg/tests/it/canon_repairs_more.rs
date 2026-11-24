//! The second fix round of the content check: the non-blocking items of the narrow re-review, each pinned so that the
//! mutant the reviewer named fails it.
//!
//! * the href of a picture is found by its namespace, never by the written name `xlink:href` (M30, the dangerous direction: a foreign `xlink` is no href);
//! * a style name is its element's namespace and local name, the family and the `style:name` (M34, M36);
//! * the faces a comparison knows for sameness are those within 8 MiB, to the byte (M33);
//! * a flat file can never carry the marker a package's joined root puts on what a flat file has no place for;
//! * a single picture of three quarters of the side budget is refused: the copy is charged as the base64 it makes.

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_container::xml::{self, Node, write_element};
use diacraft_odg::{Limits, compare_content};

use crate::build::{DRAWING, package, replace};
use crate::canon::{differs, graphic, only, same};
use crate::drawing::Drawing;
use crate::flat::{Flat, base64_lines, frame, picture};

const XLINK: &str = "http://www.w3.org/1999/xlink";
const SIMPLE: &str = r#"xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad""#;
/// The media type the converter writes for a `.png` entry whose bytes are a PNG's.
const MIME: &str = " draw:mime-type=\"image/png\"";

/// A drawing with a font face and a style of its own: the containers a package always writes are not empty, so a flat file has them too (an empty one
/// against none is a difference the check reports, and is no part of what these tests are about).
fn base() -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d.common = r#"<style:style style:name="Std" style:family="graphic"/>"#.to_owned();
    d
}

fn data(bytes: &[u8]) -> String {
    format!("<office:binary-data>{}</office:binary-data>", base64_lines(bytes, 72, "\n"))
}

/// A drawing whose page holds `image` in a frame, with `Pictures/a.png` and `Pictures/b.png` as given.
fn with_image(image: &str, a: Option<&[u8]>, b: Option<&[u8]>) -> Drawing {
    let mut d = base().page(&frame(image));
    for (name, bytes) in [("Pictures/a.png", a), ("Pictures/b.png", b)] {
        if let Some(bytes) = bytes {
            d.files.push((name.to_owned(), bytes.to_vec()));
        }
    }
    d
}

/// The flat file whose picture is `bytes` held inline, as the converter writes it (with `mime` as the attribute `draw:mime-type`, or none).
fn flat_inline(bytes: &[u8], mime: &str) -> Vec<u8> {
    Flat::of(&with_image(&format!("<draw:image{mime} {SIMPLE}>{}</draw:image>", data(bytes)), None, None)).bytes()
}

fn paths(d: &ContentDiff) -> Vec<String> {
    let mut paths: Vec<String> = d.shown.iter().map(|x| x.path.clone()).collect();
    paths.sort();
    paths
}

// --------------------------------------------------------------------------------------------------------------------------------------------- M30

#[test]
fn an_href_written_under_an_alias_of_the_link_namespace_is_an_href() {
    let png = picture(300);
    // The alias `xl` is bound by the image itself.
    let own = format!("<draw:image xmlns:xl=\"{XLINK}\" xl:href=\"Pictures/a.png\" {SIMPLE}/>");
    same(&flat_inline(&png, MIME), &with_image(&own, Some(&png), None).bytes());
    // The alias is bound by the root of the part, and the image does not say what it is: the part's declaration is the image's meaning.
    let mut by_root = with_image(&format!("<draw:image xl:href=\"Pictures/a.png\" {SIMPLE}/>"), Some(&png), None);
    by_root.ns = format!("{} xmlns:xl=\"{XLINK}\"", by_root.ns);
    same(&flat_inline(&png, MIME), &by_root.bytes());
    // The entry it names is the picture: another entry is another picture.
    let other = picture(400);
    let found = differs(&flat_inline(&other, MIME), &with_image(&own, Some(&png), None).bytes());
    assert!(found.count >= 1, "{:?}", found.shown);
    // In a flat file the href beside the picture's data is dropped by the namespace too, so the alias is dropped with it.
    let flat_alias = Flat::of(&with_image(
        &format!("<draw:image xmlns:xl=\"{XLINK}\" xl:href=\"Pictures/a.png\" {SIMPLE}>{}</draw:image>", data(&png)),
        None,
        None,
    ))
    .bytes();
    same(&flat_alias, &flat_inline(&png, ""));
}

#[test]
fn an_xlink_prefix_bound_to_a_foreign_namespace_is_no_href() {
    let png = picture(300);
    // `xlink:href`, written so, in a namespace that is not the link namespace: it is some other attribute, and the image names no entry.
    let foreign = format!("<draw:image xmlns:xlink=\"urn:foreign\" xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
    let package = with_image(&foreign, Some(&png), None).bytes();
    // It is the same as a flat file that spells the same element: nothing was made of it.
    same(&Flat::of(&with_image(&foreign, None, None)).bytes(), &package);
    // And it is not the picture the entry holds, in a flat file's inline form.
    let found = differs(&flat_inline(&png, MIME), &package);
    assert!(found.count >= 1, "{:?}", found.shown);
    // The same in a flat file: an attribute beside inline data is not an href, so it is not dropped (R33 drops the link's): it is the one difference.
    let beside_data = |href: &str| {
        let image = format!("<draw:image xmlns:xlink=\"urn:foreign\" {href} {SIMPLE}>{}</draw:image>", data(&png));
        Flat::of(&with_image(&image, None, None)).bytes()
    };
    only(&differs(&beside_data("xlink:href=\"Pictures/a.png\""), &beside_data("")), Found::Attribute);
}

#[test]
fn of_two_href_attributes_the_one_in_the_link_namespace_is_the_href_whichever_comes_first() {
    let (png, other) = (picture(300), picture(400));
    // The foreign `xlink:href` stands first and names `b.png`; the alias in the link namespace stands second and names `a.png`.
    let both = format!("<draw:image xmlns:xlink=\"urn:foreign\" xmlns:xl=\"{XLINK}\" xlink:href=\"Pictures/b.png\" xl:href=\"Pictures/a.png\"/>");
    let package = with_image(&both, Some(&png), Some(&other)).bytes();
    // The picture is `a.png`'s, and the foreign attribute stays on the element as the attribute it is.
    let flat = |bytes: &[u8]| {
        let image = format!("<draw:image xmlns:xlink=\"urn:foreign\" xlink:href=\"Pictures/b.png\"{MIME}>{}</draw:image>", data(bytes));
        Flat::of(&with_image(&image, None, None)).bytes()
    };
    same(&flat(&png), &package);
    let found = differs(&flat(&other), &package);
    assert!(found.count >= 1, "{:?}", found.shown);
}

// --------------------------------------------------------------------------------------------------------------------------------------------- M34, M36

const NUMBER: &str = "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0";

fn marked(d: &ContentDiff) -> bool {
    d.shown.iter().any(|x| x.found == Found::Attribute && x.path.ends_with("@part-name-clash-0"))
}

#[test]
fn two_elements_of_one_namespace_and_one_style_name_with_different_local_names_are_two_names() {
    // `number:date-style` in the automatic styles of `styles.xml` and `number:time-style` of the same `style:name` in those of `content.xml`: two names, so no
    // clash, and the flat file that holds both is the same.
    let mut d = base();
    d.ns = format!("{} xmlns:number=\"{NUMBER}\"", d.ns);
    d.styles_auto = format!("{}<number:date-style style:name=\"N\"/>", d.styles_auto);
    d.auto = r#"<number:time-style style:name="N"/>"#.to_owned();
    same(&Flat::of(&d).bytes(), &d.bytes());
    // The control: the same element and name in both parts is a clash, and no flat file is the same as it.
    let mut twin = d.clone();
    twin.auto = r#"<number:date-style style:name="N"/>"#.to_owned();
    assert!(marked(&differs(&Flat::of(&twin).bytes(), &twin.bytes())));
}

#[test]
fn one_local_name_and_style_name_in_two_namespaces_is_two_names() {
    // `style:style` and `draw:style` of one family and one name: the namespaces tell them apart.
    let (style, draw) =
        (r#"<style:style style:name="N" style:family="graphic"/>"#.to_owned(), r#"<draw:style style:name="N" style:family="graphic"/>"#.to_owned());
    let mut d = base();
    d.styles_auto = format!("{}{style}", d.styles_auto);
    d.auto = draw.clone();
    same(&Flat::of(&d).bytes(), &d.bytes());
    // The control: the same namespace in both parts is a clash.
    let mut twin = d.clone();
    twin.auto = style.clone();
    assert!(marked(&differs(&Flat::of(&twin).bytes(), &twin.bytes())));
    // What a name means is what its prefix is bound to in its own part, not how it is written: `x:style` in both parts, `x` bound to the style namespace in the
    // one part and to the drawing namespace in the other, is the two names above; bound to one namespace in both, it is a clash.
    let spelled = |styles_x: &str, content_x: &str| {
        let mut d = base();
        d.styles_auto = format!("{}<x:style style:name=\"N\" style:family=\"graphic\"/>", d.styles_auto);
        d.auto = r#"<x:style style:name="N" style:family="graphic"/>"#.to_owned();
        let files = d.files();
        let text = |name: &str| String::from_utf8(files.iter().find(|(n, _)| n == name).map(|(_, b)| b.clone()).unwrap()).unwrap();
        let styles = text("styles.xml").replacen("<office:document-styles ", &format!("<office:document-styles xmlns:x=\"{styles_x}\" "), 1);
        let content = text("content.xml").replacen("<office:document-content ", &format!("<office:document-content xmlns:x=\"{content_x}\" "), 1);
        package(DRAWING, replace(replace(files, "styles.xml", styles.into_bytes()), "content.xml", content.into_bytes()))
    };
    const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
    const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    same(&Flat::of(&d).bytes(), &spelled(STYLE, DRAW));
    assert!(marked(&differs(&Flat::of(&twin).bytes(), &spelled(STYLE, STYLE))));
}

#[test]
fn a_name_whose_prefix_the_child_binds_itself_is_the_name_in_that_namespace() {
    const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
    const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    let (plain, own) = (r#"<style:style style:name="N" style:family="graphic"/>"#, |uri: &str| {
        format!("<y:style xmlns:y=\"{uri}\" style:name=\"N\" style:family=\"graphic\"/>")
    });
    let mut d = base();
    // The later part's child binds `y` to the style namespace itself: it is the `style:style` of the same name and family in the first part, a clash.
    d.styles_auto = format!("{}{plain}", d.styles_auto);
    d.auto = own(STYLE);
    assert!(marked(&differs(&Flat::of(&d).bytes(), &d.bytes())));
    // The first part's child does: the same.
    let mut first = base();
    first.styles_auto = format!("{}{}", first.styles_auto, own(STYLE));
    first.auto = plain.to_owned();
    assert!(marked(&differs(&Flat::of(&first).bytes(), &first.bytes())));
    // Bound to the drawing namespace it is another name, and no clash.
    d.auto = own(DRAW);
    same(&Flat::of(&d).bytes(), &d.bytes());
    first.styles_auto = format!("{}{}", base().styles_auto, own(DRAW));
    same(&Flat::of(&first).bytes(), &first.bytes());
}

// --------------------------------------------------------------------------------------------------------------------------------------------- M33

/// The most bytes of faces a comparison knows for sameness (`MOST_FACE_BYTES` of `canon.rs`, which this test names and checks by writing a face out).
const MOST_FACE_BYTES: usize = 8 * 1024 * 1024;

/// A face named `name` whose text, as the check writes it out to know whether it is one seen before, is `len` bytes (its text is the one child).
fn face(name: &str, len: usize) -> String {
    let (open, close) = (format!("<style:font-face style:name=\"{name}\">"), "</style:font-face>");
    format!("{open}{}{close}", "x".repeat(len - open.len() - close.len()))
}

/// The length of `face` as `write_element` writes it, which is what the check measures.
fn written_len(face: &str) -> usize {
    let doc = format!(
        "<office:font-face-decls xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\">{face}</office:font-face-decls>"
    );
    let root = xml::parse(doc.as_bytes()).unwrap();
    let Some(Node::El(child)) = root.children.first() else { panic!("no face") };
    let mut out = Vec::new();
    write_element(&mut out, child).unwrap();
    out.len()
}

/// The flat file of `base()` whose one container of faces holds `faces`.
fn flat_faces(faces: &[&str]) -> Vec<u8> {
    let mut f = Flat::of(&base());
    f.fonts = format!("<office:font-face-decls>{}</office:font-face-decls>", faces.concat());
    f.bytes()
}

fn compared(a: &[u8], b: &[u8]) -> ContentDiff {
    compare_content(a, b, &Limits::default()).expect("compares")
}

#[test]
fn a_face_of_exactly_eight_mebibytes_is_known_for_sameness_and_one_byte_more_is_not() {
    // At the budget: written twice it is one face, as it is written once.
    let at = face("F", MOST_FACE_BYTES);
    assert_eq!(written_len(&at), MOST_FACE_BYTES, "the face is the budget to the byte");
    let found = compared(&flat_faces(&[&at, &at]), &flat_faces(&[&at]));
    assert!(found.same(), "{:?}", found.shown);
    // One byte past it the face is not remembered, so the second is kept: a face more, which only reports.
    let past = face("F", MOST_FACE_BYTES + 1);
    assert_eq!(written_len(&past), MOST_FACE_BYTES + 1);
    let found = compared(&flat_faces(&[&past, &past]), &flat_faces(&[&past]));
    only(&found, Found::Element);
}

#[test]
fn the_budget_of_faces_is_spent_by_what_was_remembered_before_each() {
    // Two small faces first, then a large one that brings the total to the budget, to the byte: remembered, so its repeat is dropped.
    let (one, two) = (face("S", 1_000), face("T", 2_000));
    let at = face("F", MOST_FACE_BYTES - 3_000);
    assert_eq!(written_len(&one) + written_len(&two) + written_len(&at), MOST_FACE_BYTES);
    let found = compared(&flat_faces(&[&one, &two, &at, &at]), &flat_faces(&[&one, &two, &at]));
    assert!(found.same(), "{:?}", found.shown);
    // One byte more: both small faces were remembered, so the large one is past the budget and not remembered, and its repeat is kept. (What was spent is the sum
    // of the faces before, not the last of them.)
    let past = face("F", MOST_FACE_BYTES - 2_999);
    assert_eq!(written_len(&one) + written_len(&two) + written_len(&past), MOST_FACE_BYTES + 1);
    let found = compared(&flat_faces(&[&one, &two, &past, &past]), &flat_faces(&[&one, &two, &past]));
    only(&found, Found::Element);
}

#[test]
fn a_face_both_parts_of_a_package_hold_is_one_up_to_the_budget_to_the_byte_and_a_clash_one_byte_past_it() {
    let package_with = |len: usize| {
        let mut d = base();
        d.fonts = face("F", len);
        d
    };
    // At the budget the same face in both parts is the one face of the flat file that holds it once.
    let at = package_with(MOST_FACE_BYTES);
    let found = compared(&Flat::of(&at).bytes(), &at.bytes());
    assert!(found.same(), "{:?}", found.shown);
    // One byte past it the face of the second part is not known to be the first part's: a name both parts hold, which the root says.
    let past = package_with(MOST_FACE_BYTES + 1);
    let found = compared(&Flat::of(&past).bytes(), &past.bytes());
    assert!(marked(&found), "{:?}", found.shown);
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the marker

/// A package of `d` whose `part` has the root attribute `attribute` after its version.
fn with_root_attribute(d: &Drawing, part: &str, attribute: &str) -> Vec<u8> {
    let files = d.files();
    let text = String::from_utf8(files.iter().find(|(n, _)| n == part).map(|(_, b)| b.clone()).unwrap()).unwrap();
    let version = format!("office:version=\"{}\"", d.version);
    assert!(text.contains(&version));
    package(DRAWING, replace(files, part, text.replacen(&version, &format!("{version} {attribute}"), 1).into_bytes()))
}

/// The flat file of `d` whose root also holds `attributes`, as written.
fn flat_with_root(d: &Drawing, attributes: &str) -> Vec<u8> {
    let mut f = Flat::of(d);
    f.root = f.root.replacen("<office:document ", &format!("<office:document {attributes} "), 1);
    f.bytes()
}

#[test]
fn a_flat_file_that_spells_the_marker_of_a_part_roots_attribute_is_not_the_package_that_has_it() {
    let d = base();
    let package = with_root_attribute(&d, "content.xml", "foo=\"bar\"");
    // The package says it in `part-root-attribute-0`; a flat file has no such attribute of its own, though it may write the name.
    assert_eq!(paths(&differs(&Flat::of(&d).bytes(), &package)), ["/document[0]/@part-root-attribute-0"]);
    let spelled = flat_with_root(&d, "part-root-attribute-0=\"content.xml foo=bar\"");
    let found = differs(&spelled, &package);
    assert_eq!(found.count, 2, "{:?}", found.shown);
    assert!(found.shown.iter().all(|x| x.found == Found::Attribute), "{:?}", found.shown);
    assert_eq!(paths(&found), ["/document[0]/@flat-part-root-attribute-0", "/document[0]/@part-root-attribute-0"]);
    // And not by any spelling of the value: the part and the attribute, the value, or nothing at all.
    for value in ["content.xml foo=bar", "styles.xml foo=bar", "foo=bar", ""] {
        let found = differs(&flat_with_root(&d, &format!("part-root-attribute-0=\"{value}\"")), &package);
        assert_eq!(found.count, 2, "{value}: {:?}", found.shown);
    }
}

#[test]
fn a_flat_file_that_spells_the_marker_of_a_name_both_parts_hold_is_not_the_package_that_has_it() {
    // `gr1` blue in `styles.xml`, red in `content.xml`: the package says it in `part-name-clash-0`, and no flat file is the same as it.
    let mut d = base();
    d.styles_auto = format!("{}{}", d.styles_auto, graphic("gr1", "#0000ff"));
    d.auto = graphic("gr1", "#ff0000");
    let package = d.bytes();
    assert_eq!(paths(&differs(&Flat::of(&d).bytes(), &package)), ["/document[0]/@part-name-clash-0"]);
    let words = "automatic-styles: a name of the later part is the first part's too";
    let spelled = flat_with_root(&d, &format!("part-name-clash-0=\"{words}\""));
    let found = differs(&spelled, &package);
    assert_eq!(found.count, 2, "{:?}", found.shown);
    assert!(found.shown.iter().all(|x| x.found == Found::Attribute), "{:?}", found.shown);
    assert_eq!(paths(&found), ["/document[0]/@flat-part-name-clash-0", "/document[0]/@part-name-clash-0"]);
    // The other container's words (a face both parts hold, and not the same) are no more a way in.
    let faces = flat_with_root(&d, "part-name-clash-0=\"font-face-decls: a name of the later part is the first part's too\"");
    assert_eq!(differs(&faces, &package).count, 2);
}

#[test]
fn a_flat_file_that_spells_both_markers_in_either_order_is_not_the_package_that_has_both() {
    // A package with a name both parts hold and an attribute on a part's root: the joined root has both markers.
    let mut d = base();
    d.styles_auto = format!("{}{}", d.styles_auto, graphic("gr1", "#0000ff"));
    d.auto = graphic("gr1", "#ff0000");
    let package = with_root_attribute(&d, "content.xml", "foo=\"bar\"");
    let root_attribute = "part-root-attribute-0=\"content.xml foo=bar\"";
    let clash = "part-name-clash-0=\"automatic-styles: a name of the later part is the first part's too\"";
    assert_eq!(paths(&differs(&Flat::of(&d).bytes(), &package)), ["/document[0]/@part-name-clash-0", "/document[0]/@part-root-attribute-0"]);
    // Each is a marker on the package's side, and the flat file's own attribute of the same name is another: four differences, whichever comes first.
    for spelled in [format!("{root_attribute} {clash}"), format!("{clash} {root_attribute}")] {
        let found = differs(&flat_with_root(&d, &spelled), &package);
        assert_eq!(found.count, 4, "{spelled}: {:?}", found.shown);
        assert_eq!(
            paths(&found),
            [
                "/document[0]/@flat-part-name-clash-0",
                "/document[0]/@flat-part-root-attribute-0",
                "/document[0]/@part-name-clash-0",
                "/document[0]/@part-root-attribute-0"
            ]
        );
    }
}

#[test]
fn what_a_flat_file_has_of_its_own_in_the_names_a_marker_is_given_is_compared_as_it_was_written() {
    let d = base();
    let flat = |attributes: &str| flat_with_root(&d, attributes);
    // It is the file's own attribute, compared with its value like any other: the same against itself, a value against another.
    for name in ["part-root-attribute-0", "part-name-clash-0", "part-x", "flat-x", "flat-part-x", "part-"] {
        let one = flat(&format!("{name}=\"1\""));
        same(&one, &one);
        same(&one, &flat(&format!("{name}=\"1\"")));
        only(&differs(&one, &flat(&format!("{name}=\"2\""))), Found::Value);
        // And it is an attribute the file with none lacks.
        only(&differs(&one, &Flat::of(&d).bytes()), Found::Attribute);
    }
    // Written one way or the other it is another attribute: the name a flat file's attribute is given in place of a marker is itself one a flat file may write.
    for (a, b) in [
        ("part-x", "flat-part-x"),
        ("part-root-attribute-0", "flat-part-root-attribute-0"),
        ("flat-x", "flat-flat-x"),
        ("part-name-clash-0", "flat-part-name-clash-0"),
    ] {
        let found = differs(&flat(&format!("{a}=\"1\"")), &flat(&format!("{b}=\"1\"")));
        assert_eq!(found.count, 2, "{a} against {b}: {:?}", found.shown);
    }
    // An attribute whose name has a prefix is in a namespace and is no marker, whatever the prefix is called: `part-ns` bound to a namespace is that namespace.
    let bound = |prefix: &str| flat(&format!("xmlns:{prefix}=\"urn:example:ns\" {prefix}:thing=\"1\""));
    same(&bound("part-ns"), &bound("q"));
    same(&bound("flat-ns"), &bound("q"));
    // An attribute of another name is left as it is, and a package never has one.
    only(&differs(&flat("x=\"1\""), &d.bytes()), Found::Attribute);
}

#[test]
fn a_root_attribute_name_is_put_five_bytes_longer_when_it_is_renamed_and_one_at_the_limit_is_refused_in_words() {
    let d = base();
    let mut limits = Limits::default();
    limits.container.xml.max_attr_bytes = 64;
    let refused = |name: &str| {
        compare_content(&flat_with_root(&d, &format!("{name}=\"1\"")), &Flat::of(&d).bytes(), &limits)
            .map(|x| x.count)
            .map_err(|e| (e.class(), e.to_string()))
    };
    // A name no marker is renamed from may be 64 bytes, the limit.
    assert_eq!(refused(&format!("q{}", "x".repeat(63))), Ok(1));
    // One that is renamed is five bytes longer: 59 bytes make 64, 60 make 65 and are refused as one past the limit, in the XML's own words.
    assert_eq!(refused(&format!("part-{}", "x".repeat(54))), Ok(1));
    assert_eq!(refused(&format!("flat-{}", "x".repeat(54))), Ok(1));
    for name in [format!("part-{}", "x".repeat(55)), format!("flat-{}", "x".repeat(55))] {
        let (class, words) = refused(&name).unwrap_err();
        assert_eq!(class, "xml_too_complex", "{words}");
        assert!(words.contains("longer than 64 bytes"), "{words}");
    }
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the budget

/// A package of `base()` whose one frame names `Pictures/a.png`, which holds `bytes` bytes.
fn naming(bytes: usize) -> Vec<u8> {
    with_image(&format!("<draw:image xlink:href=\"Pictures/a.png\" {SIMPLE}/>"), Some(&picture(bytes)), None).bytes()
}

#[test]
fn a_single_picture_of_three_quarters_of_the_side_budget_is_refused_because_its_copy_is_charged_as_base64() {
    // A side may add a quarter of the cap on a part (64 MiB of the 256 MiB), and a copy of a picture is charged the base64 it makes, four thirds of its bytes: so
    // one picture of 48 MiB is refused however few times it is named. At a cap of 1 MiB the budget is 262,144 bytes and the same picture is 192 KiB (196,608).
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 1 << 20;
    let flat = Flat::of(&base()).bytes();
    let fits = |bytes: usize| compare_content(&flat, &naming(bytes), &limits).is_ok();
    // The largest entry that is compared, found by halving.
    let (mut low, mut high) = (0usize, 262_144usize);
    assert!(fits(low) && !fits(high));
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    let largest = low;
    // Its base64 (four thirds) and the attribute for its media type (`draw:mime-type="image/png"` and the 64 an attribute is charged) are all that could be charged
    // if the budget were only for the picture: the declarations of the parts' roots, which the elements that move out of them are given, are charged too.
    let attribute = "draw:mime-type".len() + "image/png".len() + 64;
    let ceiling = 3 * ((262_144 - attribute) / 4);
    assert_eq!(ceiling, 196_542);
    assert!((ceiling - 4_000..=ceiling).contains(&largest), "the largest entry that fits is {largest}");
    // Three quarters of the budget, and anything larger, is refused in words that say what it is.
    for bytes in [largest + 1, 3 * 65_536, 3 * 65_536 + 1, 300_000] {
        let refused = compare_content(&flat, &naming(bytes), &limits).map(|_| ()).unwrap_err();
        assert_eq!(refused.class(), "too_much_xml", "{bytes}");
        assert!(refused.to_string().contains("copy") && refused.to_string().contains("262144"), "{refused}");
    }
    // The same picture held inline in a flat file is charged what it decodes to, so one of 250,000 bytes (more than the package's copy may be) is compared.
    let inline = Flat::of(&with_image(&format!("<draw:image{MIME} {SIMPLE}>{}</draw:image>", data(&picture(250_000))), None, None)).bytes();
    assert!(compare_content(&inline, &flat, &limits).is_ok());
    // A picture that fits is the same as the flat file that holds it inline.
    let found = compare_content(&flat_inline(&picture(largest), MIME), &naming(largest), &limits).expect("compares");
    assert!(found.same(), "{:?}", found.shown);
}
