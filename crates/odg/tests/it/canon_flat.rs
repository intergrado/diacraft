//! A flat `.fodg` against a package, and against another flat file (`diacraft_odg::compare_content`; the design's section 4.3, "`.fodg`"): both sides are put
//! into the one flat document a drawing says (`canon.rs`, "A flat `.fodg` against a package"), and every rule of the package check holds in it.
//! The packages and the flat files are built in code from the same pieces (`drawing.rs`, `flat.rs`), laid out as a writer lays a flat file out.

use diacraft_container::canon::Found;
use diacraft_odg::{Limits, OdgError, Work, compare_content, compare_content_counted};

use crate::build::{DRAWING, TEMPLATE, package, replace};
use crate::canon::{at, diff, differs, graphic, only, paragraph, same, shape_with};
use crate::drawing::{Drawing, meta, meta_with, settings};
use crate::flat::{Flat, MIME, base64_lines, frame, picture};

/// A drawing with something in each part: common and automatic styles, a font face, a page layout, a master page, a shape with text, `meta.xml`, `settings.xml`.
fn rich() -> Drawing {
    let mut d = Drawing::a4()
        .common(&format!("{}{}", graphic("Standard", "#ffffff"), paragraph("Body", "1cm")))
        .auto(&graphic("gr1", "#ff0000"))
        .page(&shape_with("s", &at("1cm"), "a<text:s/>b"));
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d.meta = Some(meta_with("Test/1", "<dc:title>A plan</dc:title>"));
    d.settings = Some(settings(""));
    d
}

/// The flat file of `d`, as a string.
fn flat_of(d: &Drawing) -> String {
    Flat::of(d).text()
}

/// `d` as a package in which `name` is `from` replaced by `to`.
fn package_changed(d: &Drawing, name: &str, from: &str, to: &str) -> Vec<u8> {
    let files = d.files();
    let text = files.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap();
    assert!(text.contains(from), "{from} is not in {name}");
    package(DRAWING, replace(files, name, text.replacen(from, to, 1).into_bytes()))
}

// ---------------------------------------------------------------------------------------------------------------------------- the same content

#[test]
fn a_fodg_against_its_package_is_the_same_content() {
    let d = rich();
    same(flat_of(&d).as_bytes(), &d.bytes());
    // The model of a flat file and of the package of the same pieces is one, and so is the content.
    let mut bare = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    bare.common = graphic("Standard", "#ffffff");
    bare.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    same(Flat::of(&bare).bytes().as_slice(), &bare.bytes());
}

#[test]
fn a_flat_file_laid_out_another_way_is_the_same() {
    let d = rich();
    let base = Flat::of(&d);
    same(&base.bytes(), &d.bytes());
    // Another line break and indent, a comment and a declaration.
    let mut other = base.clone();
    other.gap = "\r\n\t\t".to_owned();
    other.declaration = "<?xml version='1.0' encoding='UTF-8'?><!-- a comment -->\n".to_owned();
    same(&base.bytes(), &other.bytes());
    // The children of the root in an order other than the standard's: the order of the file is no content.
    let reordered = format!(
        "{}{}{}{}{}{}{}{}{}</office:document>",
        base.declaration, base.root, base.body, base.masters, base.auto, base.styles, base.fonts, base.settings, base.meta
    );
    same(reordered.as_bytes(), &d.bytes());
    same(reordered.as_bytes(), &base.bytes());
}

#[test]
fn two_flat_files_are_compared_like_a_flat_file_and_a_package() {
    let d = rich();
    let mut wrapped = Flat::of(&d);
    wrapped.gap = "\n\n   ".to_owned();
    same(&Flat::of(&d).bytes(), &wrapped.bytes());
    let mut e = rich();
    e.auto = graphic("gr1", "#ff0001");
    only(&differs(&Flat::of(&d).bytes(), &Flat::of(&e).bytes()), Found::Value);
}

// ------------------------------------------------------------------------------------------------------------------------ planted differences

#[test]
fn a_planted_difference_fails_at_the_plant_whichever_side_holds_the_flat_file() {
    let d = rich();
    let mut plants: Vec<(&str, Found, Drawing)> = Vec::new();
    let mut e = rich();
    e.common = format!("{}{}", graphic("Standard", "#fffffe"), paragraph("Body", "1cm"));
    plants.push(("a common style's colour", Found::Value, e));
    let mut e = rich();
    e.auto = graphic("gr1", "#ff0001");
    plants.push(("an automatic style's colour", Found::Value, e));
    let mut e = rich();
    e.pages = e.pages.replace("svg:x=\"1cm\"", "svg:x=\"1.5cm\"");
    plants.push(("a shape's place", Found::Value, e));
    let mut e = rich();
    e.pages = e.pages.replace("a<text:s/>b", "a<text:s/>c");
    plants.push(("a text", Found::Text, e));
    let mut e = rich();
    e.styles_auto = e.styles_auto.replace("21cm", "20cm");
    plants.push(("the page's width", Found::Value, e));
    let mut e = rich();
    e.meta = Some(meta_with("Test/1", "<dc:title>Another plan</dc:title>"));
    plants.push(("the title", Found::Text, e));
    let mut e = rich();
    e.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Sans"/>"#.to_owned();
    plants.push(("a font face", Found::Value, e));
    for (what, found, e) in &plants {
        let (flat_first, package_first) = (differs(flat_of(e).as_bytes(), &d.bytes()), differs(&d.bytes(), flat_of(e).as_bytes()));
        assert_eq!((flat_first.count, flat_first.shown.first().map(|x| x.found)), (1, Some(*found)), "{what}: {:?}", flat_first.shown);
        assert_eq!((package_first.count, package_first.shown.first().map(|x| x.found)), (1, Some(*found)), "{what}: {:?}", package_first.shown);
        assert_eq!(flat_first.shown[0].part, "office:document", "{what}");
    }
    // A dropped child, an added one.
    let mut f = Flat::of(&d);
    f.settings.clear();
    only(&differs(&f.bytes(), &d.bytes()), Found::Element);
    let mut f = Flat::of(&d);
    f.scripts = r#"<office:scripts><office:script script:language="ooo:Basic"/></office:scripts>"#.to_owned();
    only(&differs(&f.bytes(), &d.bytes()), Found::Element);
    // An empty container is the same as an absent one for the six of the format's 4.7 (`canon_empty.rs` holds the rule): the package of an empty drawing holds
    // an empty `office:font-face-decls` and `office:styles`, and the flat file of the same pieces leaves both out, or writes them empty.
    let before = Drawing::a4().bytes();
    let mut f = Flat::of(&Drawing::a4());
    same(&f.bytes(), &before);
    f.styles = "<office:styles/>".to_owned();
    f.fonts = "<office:font-face-decls/>".to_owned();
    f.scripts = "<office:scripts/>".to_owned();
    same(&f.bytes(), &before);
}

#[test]
fn the_media_type_is_compared_and_the_version_counts_as_nothing() {
    let d = rich();
    let flat = Flat::of(&d);
    // A template is not a drawing, in either form.
    let template_flat = flat.text().replace(MIME, &format!("{MIME}-template"));
    only(&differs(template_flat.as_bytes(), &d.bytes()), Found::Value);
    let template_package = package(TEMPLATE, d.files());
    only(&differs(&flat.bytes(), &template_package), Found::Value);
    same(template_flat.as_bytes(), &template_package);
    // The version counts as nothing: the flat file's own against the parts', and parts that state two versions (compared both, a
    // `Value` each, before the ruling).
    let mut other = flat.clone();
    other.root = other.root.replace("office:version=\"1.3\"", "office:version=\"1.4\"");
    same(&other.bytes(), &d.bytes());
    let two = package_changed(&d, "styles.xml", "office:version=\"1.3\"", "office:version=\"1.2\"");
    same(&flat.bytes(), &two);
    // A package whose parts agree on 1.4 is the flat file of 1.4.
    let mut e = rich();
    e.version = "1.4".to_owned();
    // (`meta.xml` and `settings.xml` of the fixture say 1.3 whatever the drawing says: the parts must agree for one flat file to say the same.)
    e.meta = None;
    e.settings = None;
    same(Flat::of(&e).bytes().as_slice(), &e.bytes());
}

#[test]
fn a_repeated_child_is_never_merged_and_a_foreign_one_is_kept() {
    let d = rich();
    let mut f = Flat::of(&d);
    // A second `office:styles` is a second element, and not more styles in the first.
    f.styles = format!("{}<office:styles>{}</office:styles>", f.styles, graphic("Extra", "#000000"));
    let extra = differs(&f.bytes(), &d.bytes());
    only(&extra, Found::Element);
    let mut g = rich();
    g.common = format!("{}{}", g.common, graphic("Extra", "#000000"));
    assert!(!diff(&f.bytes(), &g.bytes()).same(), "two containers are not one holding both");
    // A foreign child of the root is a difference too: a package has no place for it in the four parts.
    let mut f = Flat::of(&d);
    f.tail = "<x:ext xmlns:x=\"urn:x\" x:a=\"1\"/>".to_owned();
    only(&differs(&f.bytes(), &d.bytes()), Found::Element);
    same(&f.bytes(), &f.bytes());
}

// ------------------------------------------------------------------------------------------------------------------------ the shared children

#[test]
fn automatic_styles_of_one_name_in_both_parts_are_a_clash_no_flat_file_is_the_same_as() {
    // The package holds `gr1` in both parts. The package reader gives the content its own part's, while a flat file's one container is read first-come, and the
    // converter refuses to join them: the flat file of the same pieces is not the same content (`canon_repairs.rs` plants every case).
    let mut d = rich();
    d.styles_auto = format!("{}{}", graphic("gr1", "#0000ff"), d.styles_auto);
    let found = differs(flat_of(&d).as_bytes(), &d.bytes());
    assert!(found.shown.iter().any(|x| x.found == Found::Attribute && x.path.ends_with("@part-name-clash-0")), "{:?}", found.shown);
    // A flat file with one `gr1` is not that package either.
    let mut one = Flat::of(&d);
    one.auto = one.auto.replacen(&graphic("gr1", "#0000ff"), "", 1);
    assert!(!diff(&one.bytes(), &d.bytes()).same());
}

#[test]
fn a_face_declared_in_both_parts_is_one_and_one_that_differs_is_two() {
    let d = rich();
    // `Drawing` declares its faces in both parts, as the writers of both formats do, and a flat file declares each once.
    same(flat_of(&d).as_bytes(), &d.bytes());
    // The same name in the other part with another family: a name both parts hold, which no flat file is the same as (the joined root says so), and the faces
    // differ from the flat file's one as well.
    let changed = package_changed(&d, "styles.xml", "svg:font-family=\"Mono\"", "svg:font-family=\"Other\"");
    let two = differs(flat_of(&d).as_bytes(), &changed);
    let kinds: Vec<Found> = two.shown.iter().map(|x| x.found).collect();
    assert!(kinds.contains(&Found::Attribute) && kinds.contains(&Found::Value), "{kinds:?}");
    // A face only one part declares is one face.
    let mut e = rich();
    e.fonts = String::new();
    let mut with_face = e.files();
    let styles = String::from_utf8(with_face.iter().find(|(n, _)| n == "styles.xml").map(|(_, b)| b.clone()).unwrap()).unwrap();
    let styles = styles.replace(
        "<office:font-face-decls></office:font-face-decls>",
        "<office:font-face-decls><style:font-face style:name=\"Mono\" svg:font-family=\"Mono\"/></office:font-face-decls>",
    );
    with_face = replace(with_face, "styles.xml", styles.into_bytes());
    let mut flat = Flat::of(&e);
    flat.fonts = "<office:font-face-decls><style:font-face style:name=\"Mono\" svg:font-family=\"Mono\"/></office:font-face-decls>".to_owned();
    // The content part's container is empty, the styles part's holds the face: joined, one container with one face.
    same(&flat.bytes(), &package(DRAWING, with_face));
}

#[test]
fn an_attribute_of_a_parts_root_that_is_no_version_is_reported_and_not_dropped() {
    let d = rich();
    let version = "office:version=\"1.3\"";
    // The attribute stands before the version: the version is still read as the version (it is the flat file's), so the one difference is the attribute.
    let one = package_changed_in(&d, &[("content.xml", version, &format!("foo=\"bar\" {version}"))]);
    let diff = differs(&Flat::of(&d).bytes(), &one);
    assert_eq!(diff.count, 1, "{:?}", diff.shown);
    assert_eq!((diff.shown[0].found, diff.shown[0].path.as_str()), (Found::Attribute, "/document[0]/@part-root-attribute-0"));
    // Two parts with one each are two differences, and an attribute after the version is one too.
    let two = package_changed_in(
        &d,
        &[("content.xml", version, &format!("foo=\"bar\" {version}")), ("styles.xml", version, &format!("{version} baz=\"1\""))],
    );
    assert_eq!(differs(&Flat::of(&d).bytes(), &two).count, 2);
    // The same attribute with another value is another attribute: a package against a package is the container's comparison and sees it as it sees any.
    let other = package_changed_in(&d, &[("content.xml", version, &format!("foo=\"baz\" {version}"))]);
    only(&differs(&one, &other), Found::Value);
}

#[test]
fn the_attributes_of_a_parts_root_are_charged_before_they_are_kept() {
    // Each is kept on the joined root, which is a copy: one side may add a quarter of the cap on a part (here 64 KiB), and three of 28 KB are past it.
    let d = rich();
    let version = "office:version=\"1.3\"";
    let big = "x".repeat(28_000);
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 256 * 1024;
    let class = |attrs: &str| {
        let pkg = package_changed_in(&d, &[("content.xml", version, &format!("{attrs} {version}"))]);
        compare_content(&Flat::of(&d).bytes(), &pkg, &limits).map(|_| ()).map_err(|e: OdgError| e.class())
    };
    assert_eq!(class(&format!("a=\"{big}\" b=\"{big}\"")), Ok(()), "two attributes fit");
    assert_eq!(class(&format!("a=\"{big}\" b=\"{big}\" c=\"{big}\"")), Err("too_much_xml"), "the third is past the budget");
}

#[test]
fn faces_are_compared_for_sameness_up_to_a_budget_and_kept_as_they_are_past_it() {
    // Each face is about 1 KB. The budget is 8 MiB of faces written out: 3,000 faces declared in both parts are one each, and 9,000 are not all (the faces past
    // the budget are kept in both, which only reports more and never calls different content the same).
    let build = |n: usize| {
        let mut d = Drawing::a4();
        d.fonts = (0..n).map(|i| format!("<style:font-face style:name=\"F{i}\" svg:font-family=\"{}\"/>", "x".repeat(1000))).collect();
        d.common = graphic("S", "#000000");
        d
    };
    let fits = build(3_000);
    same(Flat::of(&fits).bytes().as_slice(), &fits.bytes());
    let past = build(9_000);
    let d = differs(Flat::of(&past).bytes().as_slice(), &past.bytes());
    // A face past the budget is not known to be the same as the first part's, so it is a name both parts hold (the root says so) and the face is a second one.
    assert!(
        d.shown.iter().all(|x| x.found == Found::Element || (x.found == Found::Attribute && x.path.ends_with("@part-name-clash-0"))),
        "{:?}",
        d.shown.iter().take(3).collect::<Vec<_>>()
    );
    assert!(d.shown.iter().any(|x| x.found == Found::Attribute), "the clash is reported");
}

#[test]
fn a_prefix_means_in_the_flat_file_what_it_meant_in_its_part() {
    let mut d = rich();
    d.auto = r#"<style:style style:name="one" style:family="graphic" x:note="1"/>"#.to_owned();
    d.meta = None;
    d.settings = None;
    d.styles_auto = r#"<style:style style:name="two" style:family="graphic" x:note="1"/>"#.to_owned();
    // `x` is bound to one namespace by the root of `content.xml` and to another by the root of `styles.xml`.
    let files = d.files();
    let content = String::from_utf8(files.iter().find(|(n, _)| n == "content.xml").map(|(_, b)| b.clone()).unwrap()).unwrap();
    let styles = String::from_utf8(files.iter().find(|(n, _)| n == "styles.xml").map(|(_, b)| b.clone()).unwrap()).unwrap();
    let content = content.replacen("<office:document-content ", "<office:document-content xmlns:x=\"urn:one\" ", 1);
    let styles = styles.replacen("<office:document-styles ", "<office:document-styles xmlns:x=\"urn:two\" ", 1);
    let files = replace(replace(files, "content.xml", content.into_bytes()), "styles.xml", styles.into_bytes());
    let pkg = package(DRAWING, files);
    // The flat file binds the prefix on each style, as the parts did for them.
    let flat = |one: &str, two: &str| {
        let mut f = Flat::of(&d);
        f.auto = format!(
            "<office:automatic-styles><style:style xmlns:x=\"{two}\" style:name=\"two\" style:family=\"graphic\" x:note=\"1\"/><style:style xmlns:x=\"{one}\" style:name=\"one\" style:family=\"graphic\" x:note=\"1\"/></office:automatic-styles>"
        );
        f.bytes()
    };
    same(&flat("urn:one", "urn:two"), &pkg);
    // Each swapped is another document, and a union that took the prefix from one root would call one of these the same.
    assert!(!diff(&flat("urn:two", "urn:two"), &pkg).same());
    assert!(!diff(&flat("urn:one", "urn:one"), &pkg).same());
    // Both swapped is the same two styles under each other's names, and the names are free: no shape names either, so nothing tells the file from the package.
    same(&flat("urn:two", "urn:one"), &pkg);
}

// -------------------------------------------------------------------------------------------------------------------------------- pictures

const LINES: &str = "\n      ";

fn with_picture(picture_in: &str) -> Drawing {
    let mut d = rich();
    d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(picture_in)));
    d.files.push(("Pictures/a.png".to_owned(), picture(300)));
    d
}

const HREF: &str = r#"<draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;

/// What the converter writes beside the picture it puts inline, and what a package's picture takes before the comparison (ruling Call 23): the media type the manifest
/// gives `Pictures/a.png`.
const IMAGE_MIME: &str = " draw:mime-type=\"image/png\"";

fn inline_form(png: &[u8], width: usize, extra: &str) -> String {
    format!(
        "<draw:image{extra} xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><office:binary-data>{}</office:binary-data></draw:image>",
        base64_lines(png, width, LINES)
    )
}

#[test]
fn a_picture_is_compared_by_its_bytes_in_a_flat_file_and_in_a_package() {
    let package_d = with_picture(HREF);
    let png = picture(300);
    // Inline in lines of any width and indent, or in one line: the picture's bytes.
    for width in [40, 72, 76, 10_000] {
        let inline = with_picture(&inline_form(&png, width, IMAGE_MIME));
        let mut flat = Flat::of(&inline);
        flat.settings = Flat::of(&package_d).settings;
        same(flat.bytes().as_slice(), &package_d.bytes());
    }
    // A picture's byte changed, in either form, is one difference at the picture.
    let mut changed = png.clone();
    changed[100] ^= 1;
    let inline = with_picture(&inline_form(&changed, 72, IMAGE_MIME));
    only(&differs(Flat::of(&inline).bytes().as_slice(), &package_d.bytes()), Found::Text);
    let mut other = package_d.clone();
    for (n, b) in &mut other.files {
        if n == "Pictures/a.png" {
            *b = changed.clone();
        }
    }
    only(&differs(Flat::of(&with_picture(&inline_form(&png, 72, IMAGE_MIME))).bytes().as_slice(), &other.bytes()), Found::Text);
    // An `xlink:href` beside inline data is ignored.
    let both = with_picture(&inline_form(&png, 72, &format!("{IMAGE_MIME} xlink:href=\"x.png\"")));
    same(Flat::of(&both).bytes().as_slice(), &package_d.bytes());
    // A picture the flat file holds and the package lacks (a dropped entry) is the package's link to nothing: a difference.
    let mut lost = package_d.clone();
    lost.files.retain(|(n, _)| n != "Pictures/a.png");
    assert!(!diff(Flat::of(&with_picture(&inline_form(&png, 72, IMAGE_MIME))).bytes().as_slice(), &lost.bytes()).same());
    // Two flat files whose pictures are the same bytes in other lines are the same.
    let (a, b) = (with_picture(&inline_form(&png, 40, "")), with_picture(&inline_form(&png, 72, "")));
    same(Flat::of(&a).bytes().as_slice(), Flat::of(&b).bytes().as_slice());
}

#[test]
fn a_comment_in_inline_picture_data_is_no_data_and_breaks_nothing() {
    let package_d = with_picture(HREF);
    // In lines with indent, as a producer wraps it, so that what the data is read as matters (without the reading the white space between the lines stays).
    let encoded = base64_lines(&picture(300), 40, LINES);
    let (first, second) = encoded.split_at(100);
    let inline = with_picture(&format!(
        "<draw:image draw:mime-type=\"image/png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><office:binary-data>{first}<!-- a comment -->{second}</office:binary-data></draw:image>"
    ));
    same(Flat::of(&inline).bytes().as_slice(), &package_d.bytes());
}

#[test]
fn the_href_of_an_image_is_found_among_its_other_attributes_and_no_other_element_has_its_picture_put_in() {
    let png = picture(300);
    // The href is not the first attribute: it is the one of `xlink:href`, and no other.
    let first_is_the_media_type =
        r#"<draw:image draw:mime-type="image/png" xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;
    let inline = with_picture(&inline_form(&png, 72, " draw:mime-type=\"image/png\""));
    same(Flat::of(&inline).bytes().as_slice(), &with_picture(first_is_the_media_type).bytes());
    // Only a `draw:image` or a `draw:fill-image` is a picture: another element that names an entry of the package by an href keeps the href, in the package and
    // in the flat file (a plugin names a file; an element of another vocabulary named `image` is none of ours).
    for element in
        [r#"<draw:plugin xlink:href="Pictures/a.png" draw:mime-type="image/png"/>"#, r#"<x:image xmlns:x="urn:x" xlink:href="Pictures/a.png"/>"#]
    {
        let d = with_picture(element);
        same(Flat::of(&d).bytes().as_slice(), &d.bytes());
    }
    // Data is `office:binary-data` and nothing else: a child of the office namespace of another kind (event listeners), or one of another vocabulary named
    // alike, is no data, so the href still names the picture and is put in.
    for child in ["<office:event-listeners/>", "<x:binary-data xmlns:x=\"urn:x\"/>"] {
        let by_href = with_picture(&format!(
            "<draw:image xlink:href=\"Pictures/a.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\">{child}</draw:image>"
        ));
        let inline = with_picture(&format!(
            "<draw:image draw:mime-type=\"image/png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><office:binary-data>{}</office:binary-data>{child}</draw:image>",
            base64_lines(&png, 72, LINES)
        ));
        same(Flat::of(&inline).bytes().as_slice(), &by_href.bytes());
    }
}

#[test]
fn a_fill_image_and_a_linked_picture_and_the_mime_type() {
    let png = picture(64);
    let mut d = rich();
    d.common = format!("{}<draw:fill-image draw:name=\"fi\" xlink:href=\"Pictures/a.png\"/>", d.common);
    d.files.push(("Pictures/a.png".to_owned(), png.clone()));
    let mut f = rich();
    f.common = format!(
        "{}<draw:fill-image draw:name=\"fi\"><office:binary-data>{}</office:binary-data></draw:fill-image>",
        f.common,
        base64_lines(&png, 76, "\n")
    );
    same(Flat::of(&f).bytes().as_slice(), &d.bytes());
    // A linked picture names a file that is no entry of the package: its href stays, on both sides.
    let linked = |href: &str| {
        let mut e = rich();
        e.pages = e.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&format!("<draw:image xlink:href=\"{href}\"/>"))));
        e
    };
    same(Flat::of(&linked("x.png")).bytes().as_slice(), &linked("x.png").bytes());
    only(&differs(Flat::of(&linked("x.png")).bytes().as_slice(), &linked("y.png").bytes()), Found::Value);
    // `draw:mime-type` is content (ruling Call 23): a package's picture named by an href takes the media type the converter writes for it (here the manifest's, `image/png`),
    // so a flat file with that type is the same, one with none is a difference of an attribute, and one with another type a difference of a value.
    let with_mime = with_picture(&inline_form(&picture(300), 72, IMAGE_MIME));
    same(Flat::of(&with_mime).bytes().as_slice(), &with_picture(HREF).bytes());
    let without = with_picture(&inline_form(&picture(300), 72, ""));
    only(&differs(Flat::of(&without).bytes().as_slice(), &with_picture(HREF).bytes()), Found::Attribute);
    let other_type = with_picture(&inline_form(&picture(300), 72, " draw:mime-type=\"image/jpeg\""));
    only(&differs(Flat::of(&other_type).bytes().as_slice(), &with_picture(HREF).bytes()), Found::Value);
    // Text in `office:binary-data` that is not base64 is text.
    let broken = |t: &str| {
        let mut e = rich();
        e.pages = e.pages.replace(
            "</draw:page>",
            &format!("{}</draw:page>", frame(&format!("<draw:image><office:binary-data>{t}</office:binary-data></draw:image>"))),
        );
        Flat::of(&e).bytes()
    };
    only(&differs(&broken("not base64!"), &broken("not base64?")), Found::Text);
    same(&broken("not base64!"), &broken("not base64!"));
}

#[test]
fn what_a_flat_file_has_no_place_for_is_not_compared() {
    let d = rich();
    let mut with = d.clone();
    with.files.push(("Thumbnails/thumbnail.png".to_owned(), picture(40)));
    with.files.push(("Configurations2/".to_owned(), Vec::new()));
    with.files.push(("DiaCraft/blob.bin".to_owned(), vec![1, 2, 3]));
    with.files.push(("Pictures/unnamed.png".to_owned(), picture(40)));
    // The thumbnail, the folders, DiaCraft's entries and a picture no part names are left out, and so a flat file is the same as this package.
    same(flat_of(&d).as_bytes(), &with.bytes());
}

// ---------------------------------------------------------------------------------------------------------------------------------- hostile

fn class(a: &[u8], b: &[u8]) -> &'static str {
    match compare_content(a, b, &Limits::default()) {
        Ok(d) => panic!("no error: {} differences", d.count),
        Err(e) => e.class(),
    }
}

#[test]
fn what_cannot_be_read_is_refused_in_words() {
    let d = rich();
    let (flat, pkg) = (flat_of(&d).into_bytes(), d.bytes());
    // A flat file whose root is not `office:document`.
    let wrong = flat_of(&d).replace("office:document", "office:document-content");
    assert_eq!(class(wrong.as_bytes(), &pkg), "xml_malformed");
    assert_eq!(class(&pkg, wrong.as_bytes()), "xml_malformed");
    // A DOCTYPE, and a tree that is not well-formed.
    let doctype = format!("<!DOCTYPE x [<!ENTITY e \"v\">]>{}", flat_of(&d));
    assert_eq!(class(doctype.as_bytes(), &pkg), "xml_entity_refused");
    assert_eq!(class(b"<office:document", &pkg), "xml_malformed");
    // A package part with another root, a package that is not a drawing, and bytes that are neither.
    let part = package_changed(&d, "content.xml", "office:document-content", "office:document-styles");
    assert_eq!(class(&flat, &part), "xml_malformed");
    assert_eq!(class(&flat, b"not a file at all"), "not_zip");
    let presentation = package("application/vnd.oasis.opendocument.presentation", d.files());
    assert_eq!(class(&flat, &presentation), "not_a_drawing");
    // Past the limits: a tree too deep and one with too many items.
    let deep = format!("{}{}", "<k>".repeat(600), "</k>".repeat(600));
    let nested = flat_of(&d).replacen("<office:body>", &format!("<office:body>{deep}"), 1);
    assert_eq!(class(nested.as_bytes(), &pkg), "xml_too_complex");
    let mut small = Limits::default();
    small.container.xml.max_nodes = 50;
    assert_eq!(compare_content(flat_of(&d).as_bytes(), &pkg, &small).map(|_| ()).map_err(|e: OdgError| e.class()), Err("xml_too_complex"));
    assert_eq!(
        compare_content(&pkg, &pkg, &small).map(|_| ()).map_err(|e: OdgError| e.class()),
        Ok(()),
        "two equal packages are the same bytes: no tree is built"
    );
}

#[test]
fn a_picture_named_many_times_is_refused_and_not_expanded() {
    // One picture of 90,000 bytes (120,000 characters of base64 and a media type) named by a frame each; the pictures of one side may add up to a quarter of a part's cap.
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 1 << 20;
    let build = |frames: usize| {
        let mut d = rich();
        let image = HREF.to_owned();
        d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&image).repeat(frames)));
        d.files.push(("Pictures/a.png".to_owned(), picture(90_000)));
        d
    };
    let flat = flat_of(&rich());
    let ok = build(2);
    assert!(compare_content(flat.as_bytes(), &ok.bytes(), &limits).is_ok(), "two references of 120,000 characters are 240,000 of the 262,144");
    let many = build(3);
    let refused = compare_content(flat.as_bytes(), &many.bytes(), &limits).map(|_| ()).map_err(|e| e.class());
    assert_eq!(refused, Err("too_much_xml"), "three references are 360,000 bytes");
    // The same the other way about, and with the flat file's own data.
    assert_eq!(compare_content(&many.bytes(), flat.as_bytes(), &limits).map(|_| ()).map_err(|e| e.class()), Err("too_much_xml"));
    let data = base64_lines(&picture(300_000), 76, "\n");
    let big = rich();
    let mut f = Flat::of(&big);
    f.body = f.body.replace(
        "</draw:page>",
        &format!("{}</draw:page>", frame(&format!("<draw:image><office:binary-data>{data}</office:binary-data></draw:image>"))),
    );
    assert_eq!(compare_content(f.bytes().as_slice(), &big.bytes(), &limits).map(|_| ()).map_err(|e| e.class()), Err("too_much_xml"));
}

// ------------------------------------------------------------------------------------------------------- joining, repeats and contexts

/// `d`'s package with `from` replaced by `to` in both of `content.xml` and `styles.xml`'s text, once each.
fn package_changed_in(d: &Drawing, changes: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut files = d.files();
    for (name, from, to) in changes {
        let text = files.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap();
        assert!(text.contains(from), "{from} is not in {name}");
        files = replace(files, name, text.replacen(from, to, 1).into_bytes());
    }
    package(DRAWING, files)
}

#[test]
fn a_face_in_two_contexts_that_mean_different_things_is_two_and_in_one_context_is_one() {
    let face = "<x:face x:name=\"Mono\"/>";
    let mut d = rich();
    d.fonts = format!("{}{face}", d.fonts);
    // Both parts bind `x` to one namespace: the face is declared twice and is one.
    let one = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:document-content ", "<office:document-content xmlns:x=\"urn:one\" "),
            ("styles.xml", "<office:document-styles ", "<office:document-styles xmlns:x=\"urn:one\" "),
        ],
    );
    let flat = |faces: &[&str]| {
        let mut f = Flat::of(&d);
        f.fonts = format!("<office:font-face-decls xmlns:x=\"urn:one\">{}{}</office:font-face-decls>", d.fonts.replace(face, ""), faces.concat());
        f.bytes()
    };
    same(&flat(&[face]), &one);
    // The parts bind it to two namespaces: two faces, each in its own namespace.
    let two = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:document-content ", "<office:document-content xmlns:x=\"urn:one\" "),
            ("styles.xml", "<office:document-styles ", "<office:document-styles xmlns:x=\"urn:two\" "),
        ],
    );
    let mut f = Flat::of(&d);
    f.fonts = format!(
        "<office:font-face-decls>{}<x:face xmlns:x=\"urn:two\" x:name=\"Mono\"/><x:face xmlns:x=\"urn:one\" x:name=\"Mono\"/></office:font-face-decls>",
        d.fonts.replace(face, "")
    );
    same(&f.bytes(), &two);
    only(&differs(&flat(&[face]), &two), Found::Element);
}

#[test]
fn shared_children_whose_own_attributes_disagree_stay_two_and_one_that_adds_an_attribute_is_joined() {
    let d = rich();
    // The attribute only the later part's container has is the joined container's.
    let added = package_changed_in(&d, &[("content.xml", "<office:automatic-styles>", "<office:automatic-styles foo=\"1\">")]);
    let mut f = Flat::of(&d);
    f.auto = f.auto.replacen("<office:automatic-styles>", "<office:automatic-styles foo=\"1\">", 1);
    same(&f.bytes(), &added);
    // Two containers that disagree about it are two elements, and a flat file has one.
    let clash = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:automatic-styles>", "<office:automatic-styles foo=\"1\">"),
            ("styles.xml", "<office:automatic-styles>", "<office:automatic-styles foo=\"2\">"),
        ],
    );
    // Neither value makes the one container of a flat file the same: the joined container would hold the first's value and both parts' styles, and the two
    // containers do not (the first is `styles.xml`'s, whose value is 2).
    for value in ["1", "2"] {
        let mut f = Flat::of(&d);
        f.auto = f.auto.replacen("<office:automatic-styles>", &format!("<office:automatic-styles foo=\"{value}\">"), 1);
        assert!(!diff(&f.bytes(), &clash).same(), "a flat file whose one container says foo={value}");
    }
    let mut f = Flat::of(&d);
    f.auto = f.auto.replacen("<office:automatic-styles>", "<office:automatic-styles foo=\"1\">", 1);
    // Agreeing, they are one.
    let agree = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:automatic-styles>", "<office:automatic-styles foo=\"1\">"),
            ("styles.xml", "<office:automatic-styles>", "<office:automatic-styles foo=\"1\">"),
        ],
    );
    same(&f.bytes(), &agree);
}

#[test]
fn an_instruction_or_text_in_the_root_of_a_part_is_kept_and_a_comment_is_not() {
    let d = rich();
    // `<?pi data?>` is content (a program may act on it), in a part's root as in the flat file's root.
    let with =
        |inside: &str| package_changed_in(&d, &[("content.xml", "</office:document-content>", &format!("{inside}</office:document-content>"))]);
    let mut f = Flat::of(&d);
    f.tail = "<?pi data?>".to_owned();
    same(&f.bytes(), &with("<?pi data?>"));
    assert!(!diff(&Flat::of(&d).bytes(), &with("<?pi data?>")).same(), "an instruction only one side has");
    only(&differs(&f.bytes(), &with("<?pi other?>")), Found::Instruction);
    // A comment is no content: the same as none, on either side.
    same(&Flat::of(&d).bytes(), &with("<!-- a comment -->"));
    f.tail = "<!-- a comment -->".to_owned();
    same(&f.bytes(), &d.bytes());
    // Text that is not blank is content.
    let mut text = Flat::of(&d);
    text.tail = "some text".to_owned();
    assert!(!diff(&text.bytes(), &d.bytes()).same());
}

#[test]
fn a_child_a_part_holds_twice_is_kept_beside_the_first_in_both_forms() {
    let d = rich();
    let extra = graphic("Extra", "#000000");
    // A second `office:styles` in `styles.xml` and in the flat file: two elements in both, in the same place.
    let pkg = package_changed_in(
        &d,
        &[("styles.xml", "</office:document-styles>", &format!("<office:styles>{extra}</office:styles></office:document-styles>"))],
    );
    let mut f = Flat::of(&d);
    f.tail = format!("<office:styles>{extra}</office:styles>");
    same(&f.bytes(), &pkg);
    // Not merged: one `office:styles` holding both is another document.
    let mut merged = rich();
    merged.common = format!("{}{extra}", merged.common);
    assert!(!diff(&Flat::of(&merged).bytes(), &pkg).same());
    // In another place in the flat file (before the first, in the file's order) it is the same: the order of the root's children is no content, the second is.
    let mut before = Flat::of(&d);
    before.fonts = format!("<office:styles>{extra}</office:styles>{}", before.fonts);
    assert!(!diff(&before.bytes(), &pkg).same(), "the repeat is the second `office:styles`, and it was put first");
}

// ------------------------------------------------------------------------------------------------------ what a child needs of its root

#[test]
fn a_child_in_the_default_namespace_of_its_parts_root_keeps_it() {
    let d = rich();
    // `content.xml`'s root binds the default namespace and a foreign child is written with no prefix.
    let pkg = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:document-content ", "<office:document-content xmlns=\"urn:d\" "),
            ("content.xml", "</office:document-content>", "<k/></office:document-content>"),
        ],
    );
    let mut f = Flat::of(&d);
    f.root = f.root.replacen("<office:document ", "<office:document xmlns=\"urn:d\" ", 1);
    f.tail = "<k/>".to_owned();
    same(&f.bytes(), &pkg);
    // The same child with no default namespace is another element.
    let mut bare = Flat::of(&d);
    bare.tail = "<k/>".to_owned();
    assert!(!diff(&bare.bytes(), &pkg).same());
}

#[test]
fn a_child_that_uses_a_prefix_only_in_an_attribute_keeps_that_prefix() {
    let d = rich();
    let pkg = package_changed_in(
        &d,
        &[
            ("content.xml", "<office:document-content ", "<office:document-content xmlns:p0=\"urn:p\" "),
            ("content.xml", "</office:document-content>", "<k p0:a=\"1\"/></office:document-content>"),
        ],
    );
    let mut f = Flat::of(&d);
    f.root = f.root.replacen("<office:document ", "<office:document xmlns:p0=\"urn:p\" ", 1);
    f.tail = "<k p0:a=\"1\"/>".to_owned();
    same(&f.bytes(), &pkg);
    // Bound to another address, the attribute is another one.
    let mut other = Flat::of(&d);
    other.root = other.root.replacen("<office:document ", "<office:document xmlns:p0=\"urn:q\" ", 1);
    other.tail = "<k p0:a=\"1\"/>".to_owned();
    // One attribute is missing and one is extra: the same local name in two namespaces.
    let diff = differs(&other.bytes(), &pkg);
    assert_eq!(diff.count, 2, "{:?}", diff.shown);
    assert!(diff.shown.iter().all(|x| x.found == Found::Attribute));
}

#[test]
fn a_child_named_like_one_of_in_another_namespace_is_no_child_of() {
    let d = rich();
    // `x:styles` is not `office:styles`: in a package's `styles.xml` and in a flat file it is a foreign child, kept after the eight, and does not take the
    // place of the styles.
    let pkg = package_changed_in(&d, &[("styles.xml", "</office:document-styles>", "<x:styles xmlns:x=\"urn:x\"/></office:document-styles>")]);
    let mut f = Flat::of(&d);
    f.fonts = format!("<x:styles xmlns:x=\"urn:x\"/>{}", f.fonts);
    same(&f.bytes(), &pkg);
    // And in the package's part that may hold `office:styles` it is just as foreign: the styles are still the styles.
    let mut moved = Flat::of(&d);
    moved.tail = "<x:styles xmlns:x=\"urn:x\"/>".to_owned();
    same(&moved.bytes(), &pkg);
}

#[test]
fn a_flat_file_past_the_size_limits_is_refused_in_words() {
    let d = rich();
    let flat = Flat::of(&d).bytes();
    let pkg = d.bytes();
    let mut small = Limits::default();
    small.container.max_part_bytes = 1_000;
    assert!(flat.len() > 1_000);
    assert_eq!(compare_content(&flat, &pkg, &small).map(|_| ()).map_err(|e: OdgError| e.class()), Err("part_too_large"));
    // At the limit it is read (one byte less than its length is past it).
    let mut at = Limits::default();
    at.container.max_part_bytes = flat.len() as u64;
    assert!(compare_content(&flat, &flat, &at).is_ok(), "a flat file of exactly the cap is read");
    at.container.max_part_bytes = flat.len() as u64 - 1;
    assert_eq!(compare_content(&flat, &flat, &at).map(|_| ()).map_err(|e: OdgError| e.class()), Err("part_too_large"));
    // The XML a side may hold, for the flat file, for the parts of a package, and for two packages.
    let xml = Limits { max_xml_bytes: 600, ..Limits::default() };
    assert_eq!(compare_content(&flat, &flat, &xml).map(|_| ()).map_err(|e: OdgError| e.class()), Err("too_much_xml"), "a flat file");
    assert_eq!(compare_content(&pkg, &flat, &xml).map(|_| ()).map_err(|e: OdgError| e.class()), Err("too_much_xml"), "the parts of a package");
    assert_eq!(compare_content(&pkg, &pkg, &xml).map(|_| ()).map_err(|e: OdgError| e.class()), Err("too_much_xml"), "two packages");
    assert!(compare_content(&pkg, &flat, &Limits::default()).is_ok());
}

// ------------------------------------------------------------------------------------------------------------------------ counts pinned

/// What a comparison cost, as the check counts it: its steps, its bytes and the XML events it read.
fn cost(a: &[u8], b: &[u8]) -> (u64, u64, u64) {
    let mut work = Work::default();
    let diff = compare_content_counted(a, b, &Limits::default(), &mut work).unwrap();
    assert!(diff.same());
    (work.canon_steps, work.canon_bytes, work.xml_events)
}

/// A flat file whose page holds a picture with no data at all, and the package that names an empty entry by an href.
fn empty_pictures() -> (Vec<u8>, Vec<u8>) {
    let mut inline = rich();
    inline.pages = inline.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&inline_form(&[], 72, IMAGE_MIME))));
    let mut linked = rich();
    linked.pages = linked.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&HREF.replace("a.png", "e.png"))));
    linked.files.push(("Pictures/e.png".to_owned(), Vec::new()));
    (Flat::of(&inline).bytes(), linked.bytes())
}

#[test]
fn the_counts_of_small_flat_comparisons_are_pinned() {
    // Exact counts, so that no statement that counts the work can be taken out unseen: an upper bound alone lets a mutant that counts less pass.
    let d = rich();
    let png = picture(300);
    let mut with_pictures = rich();
    with_pictures.pages = with_pictures.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(HREF)));
    with_pictures.files.push(("Pictures/a.png".to_owned(), png.clone()));
    let mut inline = rich();
    inline.pages = inline.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&inline_form(&png, 72, IMAGE_MIME))));
    // A child of a part's root that binds a prefix itself.
    let bound = package_changed_in(&d, &[("content.xml", "</office:document-content>", "<x:k xmlns:x=\"urn:x\"/></office:document-content>")]);
    let mut bound_flat = Flat::of(&d);
    bound_flat.tail = "<x:k xmlns:x=\"urn:x\"/>".to_owned();
    let (empty_flat, empty_linked) = empty_pictures();
    let got = [
        cost(Flat::of(&d).bytes().as_slice(), &d.bytes()),
        cost(Flat::of(&inline).bytes().as_slice(), &with_pictures.bytes()),
        cost(Flat::of(&d).bytes().as_slice(), Flat::of(&d).bytes().as_slice()),
        cost(&d.bytes(), &d.bytes()),
        cost(&bound_flat.bytes(), &bound),
        cost(&empty_flat, &empty_flat),
        cost(&empty_flat, &empty_linked),
    ];
    println!("PINNED {got:?}");
    assert_eq!(got, PINNED);
}

/// (the steps of the first, second and fifth rows grew by 12 for the children of the joined containers that the join now looks at for a name, and the bytes of
/// the picture rows by what a package's picture is given: its media type, which the flat file of the row holds as well.: the first, second, fifth and seventh
/// rows grew by 36, the look the rule of empty containers takes at each element of the six that stands in a document (`holds_nothing`: a few steps, as far as the first
/// thing it holds), on each side; the rows that are byte for byte one file are not compared and did not change.)
/// A flat file against its package, the same with a picture each, a flat file against itself, a package against itself, a child that binds a prefix itself,
/// a picture with no data against itself and against an empty entry.
/// (a package against itself grew by 80 steps and 38 events, the look each side takes at `meta.xml` and `settings.xml` to tell whether either is an empty part;  the flat rows by a step, the look at an `office:settings` under the root, and lost 9 bytes, the three versions no longer compared.)
/// (the same rows grew by 40 steps more: the walk of the references between the definitions and the copies of a class, a step for each definition and reference.
/// The first, second, fifth and seventh rows grew by 64 steps and about 800 bytes: the container of automatic styles is weighed and matched by its content, a step for each
/// element of the document's one container and the content of each style written out once; and the fixture's `meta.xml` holds a title beside its generator, which counts as nothing.)
/// (the first, second, fifth and seventh rows grew by 2 steps, the look at the generator, which counts as nothing and is still asked whether it breaks a constraint of
/// XML Namespaces 1.0 (a part that does is bytes): a step for it and for each of its attributes, on each side.)
///
/// **APPID moved the byte counts** (the steps and the items did not): the documents of these rows name DiaCraft's namespace, which is 14 bytes shorter than the one before
/// (`urn:app.diacraft:extension` for `urn:io.github.one_bit.diacraft:extension`), once in each flat file and in the parts of each package, so that a count fell by 14 for each
/// time the namespace stands in what it reads (14, 14, 42, 56, 14, 42 and 14: once, three times and four times).
const PINNED: [(u64, u64, u64); 7] =
    [(932, 10504, 301), (1042, 14042, 327), (219, 8841, 148), (134, 7131, 112), (948, 10550, 307), (243, 9540, 160), (1041, 11267, 322)];

#[test]
fn a_comment_in_the_root_of_a_part_costs_the_comparison_nothing() {
    // A comment is no content, so it is not carried to the joined root: not its step, and not its bytes.
    let d = rich();
    let flat = Flat::of(&d).bytes();
    let with = package_changed_in(&d, &[("content.xml", "</office:document-content>", "<!-- a comment --></office:document-content>")]);
    let (plain, commented) = (cost(&flat, &d.bytes()), cost(&flat, &with));
    assert_eq!((commented.0, commented.1), (plain.0, plain.1));
}

// ------------------------------------------------------------------------------------------------------- what the survivors of the hand mutants pin

#[test]
fn an_href_is_the_entry_it_names_percent_decoded_with_a_leading_dot_slash_dropped_or_as_written() {
    // The picture inline is what each href names.
    let png = picture(300);
    let reference = Flat::of(&with_picture(&inline_form(&png, 72, IMAGE_MIME))).bytes();
    // The href, and the name of the entry that holds the picture.
    for (href, entry) in [
        ("./Pictures/a.png", "Pictures/a.png"),
        ("Pictures/a%20b.png", "Pictures/a b.png"),
        ("./Pictures/a%20b.png", "Pictures/a b.png"),
        // A name that holds a percent sign is found as written when no entry is named by the decoded text.
        ("Pictures/a%20b.png", "Pictures/a%20b.png"),
        ("./Pictures/a%20b.png", "Pictures/a%20b.png"),
    ] {
        let mut d = rich();
        let image = format!(r#"<draw:image xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#);
        d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&image)));
        d.files.push((entry.to_owned(), png.clone()));
        let (a, b) = (diff(&reference, &d.bytes()), diff(&d.bytes(), &reference));
        assert!(a.same() && b.same(), "{href} names {entry}: {:?}", a.shown);
    }
    // An href that names no entry stays as it is, and the picture is not the one inline.
    let mut lost = rich();
    let image = r#"<draw:image xlink:href="Pictures/none.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;
    lost.pages = lost.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(image)));
    lost.files.push(("Pictures/a.png".to_owned(), png));
    assert!(!diff(&reference, &lost.bytes()).same());
}

#[test]
fn a_prefix_rebound_in_one_element_is_not_rebound_for_the_elements_after_it() {
    let png = picture(300);
    let note = "<x:note xmlns:x=\"urn:x\" xmlns:draw=\"urn:other\"/>";
    // The walk of the pictures: a `draw:image` after an element that rebinds `draw` is still a picture, so an href beside its data is ignored.
    let build = |href: &str| {
        let mut d = rich();
        d.pages = d.pages.replace("</draw:page>", &format!("{note}{}</draw:page>", frame(&inline_form(&png, 72, href))));
        Flat::of(&d).bytes()
    };
    same(&build(" xlink:href=\"x.png\""), &build(""));
    // The children of a `draw:image`: one that rebinds `office` does not make the data after it another element, and the href beside the data is still ignored.
    let image = |href: &str| {
        format!(
            "<draw:image{href} xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><x:note xmlns:x=\"urn:x\" xmlns:office=\"urn:other\"/><office:binary-data>{}</office:binary-data></draw:image>",
            base64_lines(&png, 72, LINES)
        )
    };
    let with_note = |href: &str| {
        let mut d = rich();
        d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(&image(href))));
        Flat::of(&d).bytes()
    };
    same(&with_note(" xlink:href=\"x.png\""), &with_note(""));
    // The children of the root: an element that rebinds `office` before the others does not make them foreign, so they are still in the order.
    let base = Flat::of(&rich());
    let note = "<x:note xmlns:x=\"urn:x\" xmlns:office=\"urn:other\"/>";
    let in_order = format!(
        "{}{}{note}{}{}{}{}{}{}{}</office:document>",
        base.declaration, base.root, base.meta, base.settings, base.fonts, base.styles, base.auto, base.masters, base.body
    );
    let reversed = format!(
        "{}{}{note}{}{}{}{}{}{}{}</office:document>",
        base.declaration, base.root, base.body, base.masters, base.auto, base.styles, base.fonts, base.settings, base.meta
    );
    same(in_order.as_bytes(), reversed.as_bytes());
}

#[test]
fn inline_data_in_a_cdata_section_is_data_like_text() {
    let encoded = base64_lines(&picture(300), 40, LINES);
    let inline = with_picture(&format!(
        "<draw:image draw:mime-type=\"image/png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><office:binary-data><![CDATA[{encoded}]]></office:binary-data></draw:image>"
    ));
    same(Flat::of(&inline).bytes().as_slice(), &with_picture(HREF).bytes());
}

#[test]
fn a_package_that_states_no_version_is_the_flat_file_that_states_none() {
    let mut d = rich();
    d.meta = None;
    d.settings = None;
    let version = " office:version=\"1.3\"";
    let pkg = package_changed_in(&d, &[("content.xml", version, ""), ("styles.xml", version, "")]);
    let mut f = Flat::of(&d);
    f.root = f.root.replace(version, "");
    same(&f.bytes(), &pkg);
    // The flat file that states one is the same document: the version counts as nothing (found the attribute on one side before it).
    same(&Flat::of(&d).bytes(), &pkg);
}

#[test]
fn a_foreign_child_named_like_one_of_that_comes_first_does_not_take_its_place() {
    let d = rich();
    // `x:styles` stands before `office:styles` in `styles.xml`: the styles are still the styles, and it is a foreign child kept after the eight.
    let pkg = package_changed_in(&d, &[("styles.xml", "<office:styles>", "<x:styles xmlns:x=\"urn:x\"/><office:styles>")]);
    let mut f = Flat::of(&d);
    f.tail = "<x:styles xmlns:x=\"urn:x\"/>".to_owned();
    same(&f.bytes(), &pkg);
}

#[test]
fn a_part_whose_root_is_not_its_fixed_one_is_refused_and_named() {
    let mut d = rich();
    d.meta = Some(meta("Test/1"));
    d.settings = Some(settings(""));
    let flat = flat_of(&d).into_bytes();
    // Each part's root, in the office namespace, is another part's: well-formed, and not what the part holds.
    for (part, right, wrong) in [
        ("content.xml", "office:document-content", "office:document-styles"),
        ("styles.xml", "office:document-styles", "office:document-content"),
        ("meta.xml", "office:document-meta", "office:document-settings"),
        ("settings.xml", "office:document-settings", "office:document-meta"),
    ] {
        let files = d.files();
        let text = files.iter().find(|(n, _)| n == part).map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap();
        let pkg = package(DRAWING, replace(files, part, text.replace(right, wrong).into_bytes()));
        for (a, b) in [(&flat, &pkg), (&pkg, &flat)] {
            match compare_content(a, b, &Limits::default()) {
                Err(OdgError::UnexpectedRoot { part: named }) => assert_eq!(named, part),
                other => panic!("{part}: {:?}", other.map(|d| d.count)),
            }
        }
    }
}

#[test]
fn an_attribute_of_a_parts_root_is_put_on_the_joined_root_under_a_name_no_flat_file_can_spell() {
    let d = rich();
    let version = "office:version=\"1.3\"";
    let pkg = package_changed_in(&d, &[("content.xml", version, &format!("foo=\"bar\" {version}"))]);
    // The joined root says it as `part-root-attribute-0`, which the flat file of the same pieces has not (the one difference).
    let found = differs(&Flat::of(&d).bytes(), &pkg);
    assert_eq!((found.count, found.shown[0].path.as_str()), (1, "/document[0]/@part-root-attribute-0"), "{:?}", found.shown);
    // A flat file that writes that name, with the text the joined root gives it (the part, the attribute and its value), is not the same: the
    // name is that file's own attribute, renamed, and the package's marker is still an attribute the flat file lacks (`canon_repairs_more.rs` has the rest).
    let mut f = Flat::of(&d);
    f.root = f.root.replacen("<office:document ", "<office:document part-root-attribute-0=\"content.xml foo=bar\" ", 1);
    assert_eq!(differs(&f.bytes(), &pkg).count, 2);
}
