//! The content check for OpenDocument drawings (`diacraft_odg::compare_content`): the planted differences and the planted reformatting of the
//! design's section 4.3 on real packages, the lists of `CANON_RULES` one entry at a time, the echo control and the salted copy. Every package is
//! built in code from the ODF standard (`build.rs`, `drawing.rs`), and none is a file anyone made with another program.

use diacraft_container::canon::{ContentDiff, Found, KeyFold};
use diacraft_odg::{CANON_RULES, Limits, compare_content};

use crate::build::{DRAWING, TEMPLATE, manifest, write};
use crate::drawing::{Drawing, NS};

const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// The comparison of two packages, in both directions: the count is symmetric.
pub fn diff(a: &[u8], b: &[u8]) -> ContentDiff {
    let (ab, ba) = (compare_content(a, b, &Limits::default()).unwrap(), compare_content(b, a, &Limits::default()).unwrap());
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

#[track_caller]
pub fn same(a: &[u8], b: &[u8]) {
    let d = diff(a, b);
    assert!(d.same(), "{:?}", d.shown);
}

#[track_caller]
pub fn differs(a: &[u8], b: &[u8]) -> ContentDiff {
    let d = diff(a, b);
    assert!(!d.same(), "two packages that differ were called the same");
    d
}

/// Exactly one difference, of this kind.
#[track_caller]
pub fn only(d: &ContentDiff, found: Found) {
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| x.found), Some(found), "{:?}", d.shown);
}

// ----------------------------------------------------------------------------------------------------------------------------- fixtures

pub fn graphic(name: &str, colour: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="graphic" style:parent-style-name="Standard"><style:graphic-properties draw:fill="solid" draw:fill-color="{colour}" svg:stroke-width="0.1cm"/></style:style>"#
    )
}

pub fn paragraph(name: &str, margin: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties fo:margin-left="{margin}"/><style:text-properties fo:font-size="12pt"/></style:style>"#
    )
}

/// A custom shape holding `text` as the content of its first paragraph.
pub fn shape_with(name: &str, attrs: &str, text: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" {attrs}>\n     <text:p>{text}</text:p>\n     <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/>\n    </draw:custom-shape>"
    )
}

pub fn at(x: &str) -> String {
    format!(r#"svg:x="{x}" svg:y="1cm" svg:width="2cm" svg:height="1cm""#)
}

/// A drawing of one page holding `shape`.
fn drawing_of(shape: &str) -> Drawing {
    Drawing::a4().page(shape)
}

fn text_of(text: &str) -> Vec<u8> {
    drawing_of(&shape_with("s", &at("1cm"), text)).bytes()
}

/// How a test orders the manifest's rows, as (full path, media type) pairs.
type Reorder<'a> = &'a dyn Fn(&mut Vec<(String, String)>);

/// A package of `d`'s files (a manifest row for each, in order `rows`) and the media type `media`.
fn with_rows(d: &Drawing, media: &str, order: Reorder<'_>) -> Vec<u8> {
    let files = d.files();
    let mut rows: Vec<(String, String)> = files
        .iter()
        .map(|(n, _)| {
            (
                n.clone(),
                if n.ends_with('/') {
                    String::new()
                } else if n.ends_with(".png") {
                    "image/png".to_owned()
                } else {
                    "text/xml".to_owned()
                },
            )
        })
        .collect();
    order(&mut rows);
    let rows_ref: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    let mut entries = files.clone();
    entries.push(("META-INF/manifest.xml".to_owned(), manifest(media, &rows_ref).into_bytes()));
    write(Some(media), &entries)
}

// -------------------------------------------------------------------------------------------------------- keyed siblings, one entry each

#[test]
fn styles_reordered_pass() {
    let (a, b, c) = (graphic("a", "#ff0000"), paragraph("b", "1cm"), graphic("c", "#00ff00"));
    let one = Drawing::a4().common(&format!("{a}{b}{c}")).auto(&format!("{a}{c}")).page(&shape_with("s", &at("1cm"), "x")).bytes();
    let two = Drawing::a4().common(&format!("{c}{a}{b}")).auto(&format!("{c}{a}")).page(&shape_with("s", &at("1cm"), "x")).bytes();
    same(&one, &two);
    // The same names in the automatic styles of the styles part (a page layout beside them keeps its place among the unkeyed).
    let mut d = Drawing::a4();
    d.styles_auto = format!("{a}{c}{}", d.styles_auto);
    let mut e = Drawing::a4();
    e.styles_auto = format!("{c}{a}{}", e.styles_auto);
    same(&d.bytes(), &e.bytes());
}

#[test]
fn a_style_is_found_by_its_family_and_its_name() {
    let g = |name: &str, family: &str| format!(r#"<style:style style:name="{name}" style:family="{family}"/>"#);
    let base = |x: &str| Drawing::a4().common(x).bytes();
    // Two styles of one name in two families are two keys: swapped, they are the same set.
    same(&base(&format!("{}{}", g("N", "graphic"), g("N", "paragraph"))), &base(&format!("{}{}", g("N", "paragraph"), g("N", "graphic"))));
    // The families swapped between the names are another set.
    let swapped = base(&format!("{}{}", g("A", "paragraph"), g("B", "graphic")));
    assert!(!diff(&base(&format!("{}{}", g("A", "graphic"), g("B", "paragraph"))), &swapped).same());
    // A name changed, a style dropped, and a style added are each one difference.
    let one = base(&g("A", "graphic"));
    // A name changed is a style dropped and a style added; one dropped, or one added, is one.
    let renamed = diff(&one, &base(&g("B", "graphic")));
    assert_eq!((renamed.count, renamed.shown.iter().all(|x| x.found == Found::Element)), (2, true), "{:?}", renamed.shown);
    only(&diff(&one, &base("")), Found::Element);
    only(&diff(&base(""), &one), Found::Element);
}

#[test]
fn a_changed_style_fails_after_a_reorder() {
    let (a, b, c) = (graphic("a", "#ff0000"), graphic("b", "#00ff00"), graphic("c", "#0000ff"));
    let one = Drawing::a4().common(&format!("{a}{b}{c}")).bytes();
    let two = Drawing::a4().common(&format!("{c}{}{a}", graphic("b", "#00ff01"))).bytes();
    only(&diff(&one, &two), Found::Value);
}

/// Each of the named definitions of `office:styles`: its element and an example.
const DEFINITIONS: [(&str, &str); 6] = [
    ("marker", r#"<draw:marker draw:name="{n}" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"#),
    ("gradient", r##"<draw:gradient draw:name="{n}" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff"/>"##),
    ("opacity", r#"<draw:opacity draw:name="{n}" draw:style="linear" draw:start="100%" draw:end="0%"/>"#),
    ("hatch", r##"<draw:hatch draw:name="{n}" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##),
    ("stroke-dash", r#"<draw:stroke-dash draw:name="{n}" draw:style="rect" draw:dots1="1" draw:dots1-length="0.2cm" draw:distance="0.1cm"/>"#),
    ("fill-image", r#"<draw:fill-image draw:name="{n}" xlink:href="Pictures/a.png"/>"#),
];

#[test]
fn a_definition_reordered_passes() {
    for (kind, example) in DEFINITIONS {
        let one = |n: &str| example.replace("{n}", n);
        let base = |x: &str| Drawing::a4().common(x).bytes();
        let ab = base(&format!("{}{}{}", one("a"), one("b"), one("c")));
        let ca = base(&format!("{}{}{}", one("c"), one("a"), one("b")));
        same(&ab, &ca);
        // A name changed or a definition dropped is a difference after the reorder too.
        let changed = base(&format!("{}{}{}", one("c"), one("a"), one("z")));
        assert!(!diff(&ab, &changed).same(), "{kind}: a name changed");
        let dropped = base(&format!("{}{}", one("c"), one("a")));
        assert!(!diff(&ab, &dropped).same(), "{kind}: a definition dropped");
    }
    // A definition's attribute changed, beside a reorder.
    let hatch = |n: &str, d: &str| DEFINITIONS[3].1.replace("{n}", n).replace("0.1cm", d);
    let base = |x: &str| Drawing::a4().common(x).bytes();
    only(
        &diff(&base(&format!("{}{}", hatch("a", "0.1cm"), hatch("b", "0.1cm"))), &base(&format!("{}{}", hatch("b", "0.2cm"), hatch("a", "0.1cm")))),
        Found::Value,
    );
}

#[test]
fn font_faces_reordered_pass() {
    let face = |n: &str| format!(r#"<style:font-face style:name="{n}" svg:font-family="{n}"/>"#);
    let base = |x: String| {
        let mut d = Drawing::a4();
        d.fonts = x;
        d.bytes()
    };
    same(&base(format!("{}{}{}", face("A"), face("B"), face("C"))), &base(format!("{}{}{}", face("C"), face("A"), face("B"))));
    // A face changed is one dropped and one added, in each of the two parts that declare it.
    let d = diff(&base(format!("{}{}", face("A"), face("B"))), &base(format!("{}{}", face("B"), face("Z"))));
    assert_eq!(d.count, 4, "{:?}", d.shown);
    assert!(d.shown.iter().all(|x| x.found == Found::Element));
}

#[test]
fn manifest_rows_reordered_pass() {
    let d = drawing_of(&shape_with("s", &at("1cm"), "x"));
    let plain = with_rows(&d, DRAWING, &|_| {});
    same(&plain, &with_rows(&d, DRAWING, &|rows| rows.reverse()));
    // A row's media type changed, and a row dropped, after the reorder.
    let changed = with_rows(&d, DRAWING, &|rows| {
        rows.reverse();
        if let Some(row) = rows.iter_mut().find(|r| r.0 == "meta.xml" || r.0 == "settings.xml" || r.0 == "styles.xml") {
            row.1 = "text/plain".to_owned();
        }
    });
    only(&diff(&plain, &changed), Found::Value);
    let dropped = with_rows(&d, DRAWING, &|rows| {
        rows.reverse();
        rows.pop();
    });
    only(&diff(&plain, &dropped), Found::Element);
}

#[test]
fn names_that_differ_only_in_case_are_two_keys_and_reordered_they_pass() {
    // ODF names are compared as written: `Name` and `name` are two styles, `A.png` and `a.png` two files, and either order is the same set.
    let g = |name: &str| format!(r#"<style:style style:name="{name}" style:family="graphic"/>"#);
    let styled = |x: String| Drawing::a4().common(&x).bytes();
    same(&styled(format!("{}{}", g("Name"), g("name"))), &styled(format!("{}{}", g("name"), g("Name"))));
    let mut d = Drawing::a4();
    d.files.push(("Pictures/A.png".to_owned(), PNG.to_vec()));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    same(&with_rows(&d, DRAWING, &|_| {}), &with_rows(&d, DRAWING, &|rows| rows.reverse()));
    // And two styles whose names differ in case are not one: one dropped is a difference.
    only(&diff(&styled(format!("{}{}", g("Name"), g("name"))), &styled(g("name"))), Found::Element);
}

#[test]
fn the_layer_set_reordered_fails() {
    let layers = |names: &[&str]| {
        let mut d = Drawing::a4();
        let set: String = names.iter().map(|n| format!(r#"<draw:layer draw:name="{n}"/>"#)).collect();
        d.masters = format!(r#"<draw:layer-set>{set}</draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#);
        d.bytes()
    };
    let (a, b) = (layers(&["layout", "background", "controls"]), layers(&["background", "layout", "controls"]));
    only(&differs(&a, &b), Found::Order);
    same(&a, &layers(&["layout", "background", "controls"]));
}

#[test]
fn pages_shapes_and_paragraphs_keep_their_order() {
    let (s1, s2) = (shape_with("one", &at("1cm"), "x"), shape_with("two", &at("3cm"), "x"));
    only(&differs(&drawing_of(&format!("{s1}{s2}")).bytes(), &drawing_of(&format!("{s2}{s1}")).bytes()), Found::Order);
    let pages = |first: &str, second: &str| Drawing::a4().page(first).page(second).bytes();
    assert!(!diff(&pages(&s1, &s2), &pages(&s2, &s1)).same(), "the pages are in order");
    assert!(!diff(&text_of("<text:span>a</text:span>b"), &text_of("b<text:span>a</text:span>")).same(), "a paragraph's runs are in order");
    // Master pages are not a keyed set (no source says so): swapped, they differ.
    let masters = |a: &str, b: &str| {
        let mut d = Drawing::a4();
        d.masters = format!(
            r#"<style:master-page style:name="{a}" style:page-layout-name="PM1"/><style:master-page style:name="{b}" style:page-layout-name="PM1"/>"#
        );
        d.bytes()
    };
    only(&differs(&masters("A", "B"), &masters("B", "A")), Found::Order);
}

// ------------------------------------------------------------------------------------------------------------------ paragraph white space

#[test]
fn text_s_against_spaces_passes_in_a_paragraph() {
    // One explicit space between letters is a literal space, and the other way about.
    same(&text_of("a<text:s/>b"), &text_of("a b"));
    same(&text_of("a<text:s text:c=\"1\"/>b"), &text_of("a b"));
    // A run of literal white space is one space (ODF), so it equals one `text:s` as well.
    same(&text_of("a   b"), &text_of("a<text:s/>b"));
    same(&text_of("a\n      b"), &text_of("a b"));
    // A literal space and `text:s` add up, and counts add up.
    same(&text_of("a <text:s/>b"), &text_of("a<text:s text:c=\"2\"/>b"));
    same(&text_of("a<text:s/><text:s/>b"), &text_of("a<text:s text:c=\"2\"/>b"));
    // The edges of a paragraph: leading and trailing white space is dropped, an explicit space is not.
    same(&text_of("  a  "), &text_of("a"));
    assert!(!diff(&text_of("<text:s/>a"), &text_of("a")).same(), "an explicit space at the start stays");
    assert!(!diff(&text_of("a<text:s/>"), &text_of("a")).same(), "an explicit space at the end stays");
    // Spaces in a span, as in the paragraph.
    same(&text_of("a<text:span text:style-name=\"T1\">b<text:s/>c</text:span>"), &text_of("a<text:span text:style-name=\"T1\">b c</text:span>"));
}

#[test]
fn a_different_number_of_spaces_is_a_difference() {
    differs(&text_of("a<text:s/>b"), &text_of("a<text:s text:c=\"2\"/>b"));
    // Two literal spaces are one (ODF), and so differ from an explicit two.
    differs(&text_of("a  b"), &text_of("a<text:s text:c=\"2\"/>b"));
    differs(&text_of("ab"), &text_of("a b"));
}

#[test]
fn a_different_number_of_spaces_is_text_that_differs() {
    only(&differs(&text_of("a<text:s/>b"), &text_of("a<text:s text:c=\"2\"/>b")), Found::Text);
    only(&differs(&text_of("a  b"), &text_of("a<text:s text:c=\"2\"/>b")), Found::Text);
    only(&differs(&text_of("ab"), &text_of("a b")), Found::Text);
}

#[test]
fn spaces_collapse_only_in_paragraph_content() {
    let title =
        |text: &str| drawing_of(&shape_with("s", &at("1cm"), "x").replace("<text:p>", &format!("<svg:title>{text}</svg:title><text:p>"))).bytes();
    // In an element that is not a paragraph the text is exact.
    only(&differs(&title("a  b"), &title("a b")), Found::Text);
    only(&differs(&title("a<!--c--> b"), &title("a  b")), Found::Text);
    same(&title("a  b"), &title("a  b"));
    // An attribute's value is exact too.
    let name = |n: &str| drawing_of(&shape_with(n, &at("1cm"), "x")).bytes();
    only(&differs(&name("a  b"), &name("a b")), Found::Value);
}

#[test]
fn spaces_collapse_in_paragraph_content() {
    same(&text_of("a  b"), &text_of("a b"));
    same(&text_of("a\n   b"), &text_of("a b"));
    same(&text_of("  a b  "), &text_of("a b"));
    // Not across a `text:s`, which is its own count.
    assert!(!diff(&text_of("a  <text:s/>b"), &text_of("a b")).same());
}

#[test]
fn white_space_between_elements_inside_inline_markup_is_not_spacing() {
    // `text:a` is paragraph content that is not a paragraph element: a lone space between its runs is a space, and never indentation.
    let link = |between: &str| text_of(&format!(r#"<text:a xlink:href="u"><text:span>a</text:span>{between}<text:span>b</text:span></text:a>"#));
    only(&differs(&link(" "), &link("")), Found::Text);
    only(&differs(&link("\n  "), &link("")), Found::Text);
    same(&link(" "), &link(" "));
    // The same in the paragraph itself.
    let direct = |between: &str| text_of(&format!("<text:span>a</text:span>{between}<text:span>b</text:span>"));
    only(&differs(&direct(" "), &direct("")), Found::Text);
    // A lone space before and after the children of `text:meta`.
    let meta = |lead: &str| text_of(&format!("<text:meta>{lead}<text:span>a</text:span></text:meta>"));
    only(&differs(&meta(" "), &meta("")), Found::Text);
    // White space between the shapes and the pages is indentation, and is ignored.
    let (s1, s2) = (shape_with("one", &at("1cm"), "x"), shape_with("two", &at("3cm"), "x"));
    same(&drawing_of(&format!("{s1}{s2}")).bytes(), &drawing_of(&format!("{s1}\n\n   {s2}")).bytes());
}

// -------------------------------------------------------------------------------------------------------------------------------- numbers

#[test]
fn a_length_in_another_unit_or_value_or_beyond_odfs_spelling_fails() {
    let x = |v: &str| drawing_of(&shape_with("s", &at(v), "x")).bytes();
    only(&differs(&x("2.5cm"), &x("25mm")), Found::Value);
    only(&differs(&x("2.5cm"), &x("2.5CM")), Found::Value);
    only(&differs(&x("2.5cm"), &x("2.6cm")), Found::Value);
    only(&differs(&x("2.5cm"), &x("2.5")), Found::Value);
    only(&differs(&x("2.5cm"), &x("2.5in")), Found::Value);
    // A sign is content, and a text is compared as text.
    only(&differs(&x("2.5cm"), &x("-2.5cm")), Found::Value);
    only(&differs(&x("2.5cm"), &x("2.5cm ")), Found::Value);
    only(&differs(&x("abc"), &x("abd")), Found::Value);
    // A length has no `+` and no exponent, so a respelling with either is another value (this holds before the length rule is on, and
    // is what the strict mode must keep when it is).
    only(&differs(&x("+2.5cm"), &x("2.5cm")), Found::Value);
    only(&differs(&x("25e-1cm"), &x("2.5cm")), Found::Value);
    only(&differs(&x("2.5e0cm"), &x("2.5cm")), Found::Value);
}

#[test]
fn a_length_respelled_in_its_unit_passes() {
    let x = |v: &str| drawing_of(&shape_with("s", &at(v), "x")).bytes();
    same(&x("2.50cm"), &x("2.5cm"));
    same(&x("2.500mm"), &x("2.5mm"));
    same(&x("-2.50cm"), &x("-2.5cm"));
    same(&x("25.0pt"), &x("25pt"));
}

/// A drawing with every listed length attribute, each on the element the writer puts it on, all with the value `v`; `fo` and `svg` are the namespaces
/// the file binds those prefixes to.
fn styled(fo: &str, svg: &str, v: &str) -> Vec<u8> {
    let mut d = Drawing::a4();
    d.ns = NS
        .replace("urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0", fo)
        .replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", svg);
    d.common = format!(
        r#"<style:style style:name="S" style:family="graphic"><style:graphic-properties svg:stroke-width="{v}" fo:padding-top="{v}" fo:padding-bottom="{v}" fo:padding-left="{v}" fo:padding-right="{v}" draw:marker-start-width="{v}" draw:marker-end-width="{v}"/></style:style><style:style style:name="P" style:family="paragraph"><style:paragraph-properties fo:margin-left="{v}" fo:margin-right="{v}" fo:margin-top="{v}" fo:margin-bottom="{v}" fo:text-indent="{v}" fo:line-height="{v}"/><style:text-properties fo:font-size="{v}" fo:letter-spacing="{v}"/></style:style>"#
    );
    d.styles_auto = format!(
        r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="{v}" fo:page-height="{v}" fo:margin-top="{v}" fo:margin-left="{v}"/></style:page-layout>"#
    );
    let shapes = format!(
        r#"<draw:line draw:name="l" svg:x1="{v}" svg:y1="{v}" svg:x2="{v}" svg:y2="{v}"/><draw:circle draw:name="c" svg:cx="{v}" svg:cy="{v}" svg:r="{v}" svg:x="{v}" svg:y="{v}" svg:width="{v}" svg:height="{v}"/><draw:ellipse draw:name="e" svg:cx="{v}" svg:cy="{v}" svg:rx="{v}" svg:ry="{v}"/>"#
    );
    d.pages = format!(r#"<draw:page draw:name="p" draw:master-page-name="Default">{shapes}</draw:page>"#);
    d.bytes()
}

const ODF_FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const ODF_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const W3C_FO: &str = "http://www.w3.org/1999/XSL/Format";
const W3C_SVG: &str = "http://www.w3.org/2000/svg";

#[test]
fn every_length_attribute_is_a_difference_in_another_unit() {
    for (fo, svg) in [(ODF_FO, ODF_SVG), (W3C_FO, W3C_SVG), (ODF_FO, W3C_SVG), (W3C_FO, ODF_SVG)] {
        let d = differs(&styled(fo, svg, "2.5cm"), &styled(fo, svg, "25mm"));
        // One difference for each of the 34 attributes of the fixture that holds a length (none passed unseen).
        assert_eq!(d.count, 34, "{fo} {svg}: {:?}", d.shown.iter().take(40).map(|x| &x.path).collect::<Vec<_>>());
        assert!(d.shown.iter().all(|x| x.found == Found::Value));
    }
    // The same attributes under the other URI are other names: a mixed pair is not the same drawing.
    assert!(!diff(&styled(ODF_FO, ODF_SVG, "2.5cm"), &styled(W3C_FO, W3C_SVG, "2.5cm")).same());
}

#[test]
fn every_listed_length_passes_respelled() {
    for (fo, svg) in [(ODF_FO, ODF_SVG), (W3C_FO, W3C_SVG), (ODF_FO, W3C_SVG), (W3C_FO, ODF_SVG)] {
        same(&styled(fo, svg, "2.50cm"), &styled(fo, svg, "2.5cm"));
    }
}

#[test]
fn a_value_that_holds_text_or_an_integer_is_never_respelled() {
    let name = |n: &str| drawing_of(&shape_with(n, &at("1cm"), "x")).bytes();
    only(&differs(&name("007"), &name("7")), Found::Value);
    only(&differs(&name("1.50"), &name("1.5")), Found::Value);
    // Integer, angle and index attributes are not lengths.
    let with = |attr: &str, v: &str| drawing_of(&shape_with("s", &format!("{} {attr}=\"{v}\"", at("1cm")), "x")).bytes();
    for attr in ["draw:z-index", "draw:start-angle", "draw:corners", "draw:id", "draw:transform", "svg:viewBox", "draw:corner-radius", "svg:d"] {
        only(&differs(&with(attr, "007"), &with(attr, "7")), Found::Value);
        only(&differs(&with(attr, "2.50cm"), &with(attr, "2.5cm")), Found::Value);
    }
    // `text:c` counts spaces: it is read as a count, never as a respelled number.
    differs(&text_of("a<text:s text:c=\"2\"/>b"), &text_of("a<text:s text:c=\"3\"/>b"));
    // Percentages are respelled as lengths are (ruling Call 22): `75%` is `75.0%` and is not `76%` (every one of them is planted in `canon_repairs.rs`).
    let opacity = |v: &str| {
        Drawing::a4()
            .common(&format!(r#"<style:style style:name="S" style:family="graphic"><style:graphic-properties draw:opacity="{v}"/></style:style>"#))
            .bytes()
    };
    same(&opacity("75%"), &opacity("75.0%"));
    only(&differs(&opacity("75%"), &opacity("76%")), Found::Value);
}

#[test]
fn the_lists_are_what_the_module_says() {
    assert_eq!(CANON_RULES.keyed.len(), 9, "the manifest rows, the styles, the font faces and the six definitions, and nothing else");
    assert!(CANON_RULES.exact_text.iter().any(|n| n.local == "a"), "the lists name `text:a`");
}

/// The paragraph and length rules are on (CANON-1F1 bounds a paragraph's text and reads a length as ODF does): the three paragraph elements are
/// paragraphs and not mixed content, the lengths are in `lengths` and never in the looser `numbers`, and every key is folded exactly.
#[test]
fn the_paragraph_and_length_rules_are_on() {
    assert_eq!(CANON_RULES.paragraphs.iter().map(|n| n.local).collect::<Vec<_>>(), ["p", "h", "span"]);
    assert!(CANON_RULES.exact_text.iter().all(|n| !["p", "h", "span"].contains(&n.local)), "a paragraph is read by ODF's rule, not as mixed content");
    assert_eq!(CANON_RULES.lengths.len(), 70, "29 attributes under two namespaces each, two of `draw`, and 12 percentages");
    assert_eq!(CANON_RULES.numbers.len(), 2, "a stroke's opacity, under two namespaces, is the one number: a length is read as ODF reads one");
    assert!(CANON_RULES.keyed.iter().all(|k| k.fold == KeyFold::Exact), "ODF names are compared as written");
}

// ----------------------------------------------------------------------------------------------------------------------- the package level

#[test]
fn a_file_against_itself_is_the_same() {
    let mut with_everything =
        Drawing::a4().common(&format!("{}{}", graphic("a", "#ff0000"), paragraph("p", "1cm"))).page(&shape_with("s", &at("1cm"), "a<text:s/>b"));
    with_everything.meta = Some(crate::drawing::meta("Test/1"));
    with_everything.settings = Some(crate::drawing::settings(""));
    with_everything.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    with_everything.files.push(("Thumbnails/thumbnail.png".to_owned(), PNG.to_vec()));
    with_everything.files.push(("Configurations2/".to_owned(), Vec::new()));
    let template = crate::build::package(TEMPLATE, with_everything.files());
    for bytes in [Drawing::a4().bytes(), with_everything.bytes(), template, text_of("a  b")] {
        let d = compare_content(&bytes, &bytes, &Limits::default()).unwrap();
        assert!(d.same(), "{:?}", d.shown);
        assert_eq!(d.count, 0);
    }
}

#[test]
fn a_file_against_a_salted_copy_fails_at_the_salt_only() {
    let base = |x: &str, style: &str| {
        Drawing::a4()
            .common(&format!("{}{}", graphic("a", style), paragraph("p", "1cm")))
            .page(&format!("{}{}", shape_with("one", &at("1cm"), "x"), shape_with("two", &at(x), "y")))
            .bytes()
    };
    let echo = base("3cm", "#ff0000");
    // A shape's place.
    let salted = base("3.01cm", "#ff0000");
    let d = differs(&echo, &salted);
    only(&d, Found::Value);
    let path = &d.shown[0].path;
    assert!(path.ends_with("/@x"), "{path}");
    assert_eq!(d.shown[0].part, "content.xml");
    // A style's colour.
    let d = differs(&echo, &base("3cm", "#ff0001"));
    only(&d, Found::Value);
    assert_eq!(d.shown[0].part, "styles.xml");
}

#[test]
fn a_respelling_is_no_salt() {
    let base = |x: &str| drawing_of(&shape_with("two", &at(x), "y")).bytes();
    same(&base("3cm"), &base("3.00cm"));
}

#[test]
fn a_dropped_part_a_dropped_folder_and_a_changed_picture_byte_fail() {
    let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    d.meta = Some(crate::drawing::meta_with("Test/1", "<dc:title>A plan</dc:title>"));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    d.files.push(("Configurations2/".to_owned(), Vec::new()));
    let full = d.bytes();
    // A part dropped, with its manifest row: one missing part (the row says what the entry says, and is no second difference).
    let mut e = d.clone();
    e.meta = None;
    let diff_ = differs(&full, &e.bytes());
    assert!(diff_.shown.iter().any(|x| x.found == Found::MissingPart && x.part == "meta.xml"), "{:?}", diff_.shown);
    assert_eq!(diff_.count, 1, "the part: {:?}", diff_.shown);
    // The folder is an entry: dropped, it is a missing part (and its row).
    let mut e = d.clone();
    e.files.retain(|(n, _)| n != "Configurations2/");
    let diff_ = differs(&full, &e.bytes());
    assert!(diff_.shown.iter().any(|x| x.found == Found::MissingPart && x.part == "Configurations2/"), "{:?}", diff_.shown);
    // A picture's byte.
    let mut e = d.clone();
    if let Some((_, bytes)) = e.files.iter_mut().find(|(n, _)| n == "Pictures/a.png") {
        bytes[8] ^= 1;
    }
    let diff_ = differs(&full, &e.bytes());
    only(&diff_, Found::Bytes);
    assert_eq!(diff_.shown[0].part, "Pictures/a.png");
    // A part added.
    only(&differs(&e.bytes(), &e.bytes_without("Pictures/a.png")), Found::MissingPart);
}

trait Without {
    fn bytes_without(&self, name: &str) -> Vec<u8>;
}

impl Without for Drawing {
    /// The package with the file `name` left out of the archive but its manifest row kept.
    fn bytes_without(&self, name: &str) -> Vec<u8> {
        let files = self.files();
        let rows: Vec<(String, &str)> = files
            .iter()
            .map(|(n, _)| {
                (
                    n.clone(),
                    if n.ends_with('/') {
                        ""
                    } else if n.ends_with(".png") {
                        "image/png"
                    } else {
                        "text/xml"
                    },
                )
            })
            .collect();
        let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        let mut entries: Vec<(String, Vec<u8>)> = files.into_iter().filter(|(n, _)| n != name).collect();
        entries.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
        write(Some(DRAWING), &entries)
    }
}

#[test]
fn a_template_is_not_a_drawing() {
    let d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    let (drawing, template) = (d.bytes(), crate::build::package(TEMPLATE, d.files()));
    let diff_ = differs(&drawing, &template);
    assert!(diff_.shown.iter().any(|x| x.found == Found::Bytes && x.part == "mimetype"), "{:?}", diff_.shown);
    assert!(diff_.shown.iter().any(|x| x.part == "META-INF/manifest.xml"), "the manifest's root row says it too: {:?}", diff_.shown);
}

#[test]
fn entry_names_are_compared_exactly() {
    // OPC folds the case of part names and OpenDocument does not: `Pictures/A.png` is not `Pictures/a.png`.
    let with = |name: &str| {
        let mut d = Drawing::a4();
        d.files.push((name.to_owned(), PNG.to_vec()));
        d.bytes()
    };
    let d = differs(&with("Pictures/a.png"), &with("Pictures/A.png"));
    assert!(d.shown.iter().any(|x| x.found == Found::MissingPart && x.part == "Pictures/a.png"), "{:?}", d.shown);
    assert!(d.shown.iter().any(|x| x.found == Found::ExtraPart && x.part == "Pictures/A.png"), "{:?}", d.shown);
    same(&with("Pictures/a.png"), &with("Pictures/a.png"));
}

/// A package of `d`'s files and `extra` ones, whose manifest rows are `rows` (the four parts' rows are always there).
fn with_extra(extra: Vec<(&str, Vec<u8>)>, rows: &[(&str, &str)]) -> Vec<u8> {
    let d = Drawing::a4();
    let mut files = d.files();
    let mut all: Vec<(String, String)> = files.iter().map(|(n, _)| (n.clone(), "text/xml".to_owned())).collect();
    for (name, bytes) in extra {
        files.push((name.to_owned(), bytes));
    }
    for (name, media) in rows {
        all.push(((*name).to_owned(), (*media).to_owned()));
    }
    let rows: Vec<(&str, &str)> = all.iter().map(|(n, m)| (n.as_str(), m.as_str())).collect();
    files.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
    write(Some(DRAWING), &files)
}

#[test]
fn signatures_and_rdf_are_xml_by_their_names_or_their_rows_and_other_files_are_bytes() {
    let sig = |inner: &str| format!("<s:Signatures xmlns:s=\"urn:s\">{inner}</s:Signatures>").into_bytes();
    // A signature part of `META-INF/` is XML whatever the manifest says (the manifest does not list it).
    same(
        &with_extra(vec![("META-INF/documentsignatures.xml", sig("<s:a/><s:b/>"))], &[]),
        &with_extra(vec![("META-INF/documentsignatures.xml", sig("\n <s:a/>\n <s:b/>\n"))], &[]),
    );
    // `manifest.rdf` is XML by its row.
    let rdf = |inner: &str| format!("<r:RDF xmlns:r=\"urn:r\">{inner}</r:RDF>").into_bytes();
    same(
        &with_extra(vec![("manifest.rdf", rdf("<r:a/>"))], &[("manifest.rdf", "application/rdf+xml")]),
        &with_extra(vec![("manifest.rdf", rdf("\n<r:a/>\n"))], &[("manifest.rdf", "application/rdf+xml")]),
    );
    // A file that no row calls XML and no list names is bytes: its spacing is content.
    only(&differs(&with_extra(vec![("Basic/x.bin", rdf("<r:a/>"))], &[]), &with_extra(vec![("Basic/x.bin", rdf("\n<r:a/>\n"))], &[])), Found::Bytes);
    // A picture of an XML type is bytes whatever its row says.
    only(
        &differs(
            &with_extra(vec![("Pictures/a.svg", rdf("<r:a/>"))], &[("Pictures/a.svg", "image/svg+xml")]),
            &with_extra(vec![("Pictures/a.svg", rdf("\n<r:a/>\n"))], &[("Pictures/a.svg", "image/svg+xml")]),
        ),
        Found::Bytes,
    );
}

#[test]
fn a_changed_instruction_a_missing_attribute_and_an_extra_element_fail() {
    let base = |text: &str| drawing_of(&shape_with("s", &at("1cm"), text)).bytes();
    only(&differs(&base("a<?x one?>b"), &base("a<?x two?>b")), Found::Instruction);
    only(&differs(&base("a"), &base("a<text:line-break/>")), Found::Element);
    let with = |attrs: &str| drawing_of(&shape_with("s", attrs, "x")).bytes();
    only(&differs(&with(&at("1cm")), &with(&format!("{} draw:layer=\"layout\"", at("1cm")))), Found::Attribute);
    only(&differs(&with(&at("1cm")), &with(&at("1cm").replace("svg:height=\"1cm\"", ""))), Found::Attribute);
    // Spacing, attribute order, prefixes, comments and CDATA are not content.
    let spaced = shape_with("s", &at("1cm"), "x").replace("<text:p>x</text:p>", "<text:p><![CDATA[x]]><!-- c --></text:p>");
    same(&drawing_of(&spaced).bytes(), &base("x"));
    let reordered = at("1cm").split(' ').rev().collect::<Vec<_>>().join(" ");
    same(&with(&at("1cm")), &with(&reordered));
}

#[test]
fn what_is_xml_is_told_by_the_manifest_or_by_the_name_and_a_disagreement_is_bytes() {
    // An embedded object's content: XML when its row says so, and then spacing is not content.
    let build = |inner: &str, media: &str| {
        let d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
        let mut files = d.files();
        files.push(("Object 1/content.xml".to_owned(), format!("<a xmlns=\"urn:x\"><b/>{inner}</a>").into_bytes()));
        let mut rows: Vec<(String, String)> = files.iter().map(|(n, _)| (n.clone(), "text/xml".to_owned())).collect();
        for row in &mut rows {
            if row.0 == "Object 1/content.xml" {
                row.1 = media.to_owned();
            }
        }
        let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        files.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
        write(Some(DRAWING), &files)
    };
    same(&build("<c/>", "text/xml"), &build("\n  <c/>\n", "text/xml"));
    // One side's row calls it XML and the other's does not: the two are compared as bytes, and they differ (and so does the row).
    let d = differs(&build("<c/>", "text/xml"), &build("\n  <c/>\n", "application/octet-stream"));
    assert!(d.shown.iter().any(|x| x.found == Found::Bytes && x.part == "Object 1/content.xml"), "{:?}", d.shown);
    // Neither calls it XML: bytes, and the spacing is content.
    let d = differs(&build("<c/>", "application/octet-stream"), &build("\n  <c/>\n", "application/octet-stream"));
    only(&d, Found::Bytes);
    // A picture of an XML type is bytes: the same bytes for every picture.
    let d = differs(&build("<c/>", "image/svg+xml"), &build("\n  <c/>\n", "image/svg+xml"));
    only(&d, Found::Bytes);
    // The four parts and the manifest are XML whatever their rows say.
    let no_rows = |spacing: &str| {
        let d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
        let mut files = d.files();
        for (n, b) in &mut files {
            if n == "content.xml" {
                let text = String::from_utf8(b.clone()).unwrap().replace("<office:body>", &format!("<office:body>{spacing}"));
                *b = text.into_bytes();
            }
        }
        let rows: Vec<(&str, &str)> = vec![("content.xml", "application/octet-stream"), ("styles.xml", "application/octet-stream")];
        files.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
        write(Some(DRAWING), &files)
    };
    same(&no_rows(""), &no_rows("\n   "));
}

#[test]
fn a_part_that_does_not_parse_is_compared_as_bytes() {
    let broken = |tail: &str| {
        let mut d = Drawing::a4();
        d.settings = Some(format!("<office:document-settings {NS}><oops{tail}"));
        d.bytes()
    };
    only(&differs(&broken(">"), &broken(" >")), Found::Bytes);
    same(&broken(">"), &broken(">"));
}
