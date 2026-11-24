//! The reader counts what the drawing does not hold, by class, and says it first among its warnings, a line for each class with its count; and it keeps a master
//! page that holds no shape whole. Every input is built here by hand from the OASIS ODF 1.4 standard (Part 3: §16.9 `style:master-page`, §16.23 `style:font-face`, §10.3 the shapes, §2.2
//! the flat document); only the classes and counts of the gate's sample-file check crossed.

use diacraft_odg::{Limits, NOT_KEPT_HEAD, NOT_KEPT_LINE_CHARS, read};

const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" "#,
    r#"xmlns:dr3d="urn:oasis:names:tc:opendocument:xmlns:dr3d:1.0" xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0""#
);

const LAYOUT: &str =
    r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm"/></style:page-layout>"#;

/// A package whose two parts hold these: the font faces of `styles.xml` and of `content.xml`, the common styles, the master pages (beside the layout `PM1`) and the pages' shapes, on the
/// master page `Default`.
fn package(faces_styles: &str, faces_content: &str, common: &str, masters: &str, shapes: &str) -> Vec<u8> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3"><office:font-face-decls>{faces_content}</office:font-face-decls><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default">{shapes}</draw:page></office:drawing></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3"><office:font-face-decls>{faces_styles}</office:font-face-decls><office:styles>{common}</office:styles><office:automatic-styles>{LAYOUT}</office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/>{masters}</office:master-styles></office:document-styles>"#
    );
    crate::build::package(
        crate::build::DRAWING,
        vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), styles.into_bytes())],
    )
}

fn rect() -> &'static str {
    r#"<draw:rect draw:name="r" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#
}

fn read_ok(bytes: &[u8]) -> (diacraft_doc::Document, diacraft_odg::ReadReport) {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

/// The note's lines a read put first among its warnings: the head and the lines after it, up to the first warning that is no line of the note.
fn note(report: &diacraft_odg::ReadReport) -> Vec<String> {
    report.warnings.iter().take(usize::try_from(report.warned("not_kept")).unwrap()).cloned().collect()
}

/// **A shape of a kind the model keeps only as a placeholder is counted** (a dimension line, a callout, a form control, a 3-D scene: a copy written from the model writes each as a plain
/// shape), on a page and in a group, and a frame that holds an object is not: it is written back as a frame. Red on main (770a162c): no count.
#[test]
fn a_shape_kept_only_as_a_placeholder_is_counted_and_a_frame_s_object_is_not() {
    let shapes = concat!(
        r#"<draw:measure svg:x1="1cm" svg:y1="5cm" svg:x2="6cm" svg:y2="5cm"/>"#,
        r#"<draw:caption svg:x="1cm" svg:y="7cm" svg:width="3cm" svg:height="2cm" draw:caption-point-x="-1cm" draw:caption-point-y="3cm"/>"#,
        r#"<draw:g><draw:control draw:control="c1" svg:x="1cm" svg:y="10cm" svg:width="3cm" svg:height="1cm"/><draw:g><dr3d:scene svg:x="5cm" svg:y="10cm" svg:width="3cm" svg:height="3cm"/></draw:g></draw:g>"#,
        r#"<draw:frame svg:x="10cm" svg:y="10cm" svg:width="3cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/></draw:frame>"#,
        r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#
    );
    let (_, report) = read_ok(&package("", "", "", "", shapes));
    assert_eq!(report.not_kept("element_kind"), 4, "{:?}", report.not_kept);
    assert_eq!(
        note(&report),
        [
            NOT_KEPT_HEAD.to_owned(),
            "4 shapes of kinds DiaCraft keeps only as placeholders (dimension lines, callouts, form controls or 3-D scenes), which a copy writes as plain shapes".to_owned()
        ]
    );
    // A drawing of plain shapes is told nothing.
    let (_, plain) = read_ok(&package("", "", "", "", rect()));
    assert!(plain.not_kept.values().all(|n| *n == 0) && note(&plain).is_empty(), "{:?}", plain.warnings);
}

/// **A shape in a hyperlink that a modelled shape follows is counted** (the model keeps the wrapper unmodelled, and a copy writes it after the shapes it models, so over the ones that were
/// over it), on a page and in a group; one that is last keeps its place and is not. Red on main (770a162c): no count.
#[test]
fn a_shape_in_a_hyperlink_that_a_shape_follows_is_counted() {
    let link = r#"<draw:a xlink:href="https://example.org/"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></draw:a>"#;
    let rect = r#"<draw:rect svg:x="4cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/>"#;
    let cases = [
        (format!("{link}{rect}"), 1, "first, with a shape after it"),
        (format!("{rect}{link}"), 0, "last"),
        (format!("<draw:g>{link}{rect}</draw:g>{link}"), 1, "first in a group, and last on the page"),
    ];
    for (shapes, counted, what) in cases {
        let (_, report) = read_ok(&package("", "", "", "", &shapes));
        assert_eq!(report.not_kept("shape_order"), counted, "{what}");
    }
}

/// **A font declaration the one table cannot hold apart is counted, and one it can is not**: the same declaration in both parts is one face of both (the programs that write both write
/// it so), an exact repeat within one part is held once, and a name the two parts, or one part twice, declare differently is held twice under one name. An unused declaration is held, and
/// comes back: it is not counted. Red on main (770a162c): no count.
#[test]
fn a_font_declaration_the_table_cannot_hold_apart_is_counted() {
    let arial = r#"<style:font-face style:name="Arial" svg:font-family="Arial" style:font-family-generic="swiss"/>"#;
    let arial_roman = r#"<style:font-face style:name="Arial" svg:font-family="Arial" style:font-family-generic="roman"/>"#;
    let unused = r#"<style:font-face style:name="Unused" svg:font-family="Unused"/>"#;
    let cases: [(&str, String, String, u64, usize); 5] = [
        ("the same in both parts", arial.to_owned(), arial.to_owned(), 0, 1),
        ("unused, in one part", format!("{arial}{unused}"), arial.to_owned(), 0, 2),
        ("a repeat in one part", format!("{arial}{arial}"), arial.to_owned(), 1, 1),
        ("one name declared otherwise in the other part", arial.to_owned(), arial_roman.to_owned(), 1, 2),
        ("one name declared twice otherwise in one part", format!("{arial}{arial_roman}"), String::new(), 1, 2),
    ];
    for (what, styles, content, counted, faces) in cases {
        let (doc, report) = read_ok(&package(&styles, &content, "", "", rect()));
        assert_eq!(report.not_kept("font_declaration"), counted, "{what}");
        assert_eq!(doc.fonts.len(), faces, "{what}: the faces the table holds");
    }
}

/// **A default tab distance is counted** on every style that says one but a default style, which is kept whole: a cell reads it, and no copy writes that cell. Red on main (770a162c): no
/// count.
#[test]
fn a_default_tab_distance_is_counted_but_on_a_default_style() {
    let common = concat!(
        r#"<style:default-style style:family="graphic"><style:paragraph-properties style:tab-stop-distance="1.25cm"/></style:default-style>"#,
        r#"<style:style style:name="G" style:family="graphic"><style:paragraph-properties style:tab-stop-distance="1cm"/></style:style>"#,
        r#"<style:style style:name="P" style:family="paragraph"><style:paragraph-properties style:tab-stop-distance="2cm"/></style:style>"#,
        r#"<style:style style:name="Q" style:family="paragraph"><style:paragraph-properties fo:margin-left="1cm"/></style:style>"#
    );
    let (_, report) = read_ok(&package("", "", common, "", rect()));
    assert_eq!(report.not_kept("style_property"), 2, "{:?}", report.not_kept);
}

/// **A master page that holds no shape is kept whole**, whether a page is on it or not, with its attributes and children, and one that holds shapes is a background page and
/// is not; nothing of a master page is left in the document's other kept markup, where a copy could not place it. Red on main (770a162c): no master page is kept.
#[test]
fn a_master_page_with_no_shape_is_kept_whole() {
    let masters = concat!(
        r#"<style:master-page style:name="Unused" style:display-name="Not used" style:page-layout-name="PM1"><presentation:notes><draw:page-thumbnail svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/></presentation:notes></style:master-page>"#,
        r#"<style:master-page style:name="Shapes" style:page-layout-name="PM1"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></style:master-page>"#
    );
    let (doc, report) = read_ok(&package("", "", "", masters, rect()));
    let odf = &doc.ext.get().unwrap().odf;
    let kept: Vec<&diacraft_doc::KeptStyle> = odf.styles.iter().filter(|s| &*s.family == "master:master-page").collect();
    let names: Vec<&str> = kept.iter().map(|s| &*s.name).collect();
    assert_eq!(names, ["Default", "Unused"], "the master pages with no shape, in order");
    let unused = &kept[1].element;
    assert_eq!(unused.attrs.get("style:display-name"), Some("Not used"));
    assert!(matches!(unused.children.as_slice(), [diacraft_doc::RawNode::Element(e)] if &*e.name == "presentation:notes"));
    // The children are kept once: not as the document's other kept markup too.
    assert!(odf.kept.kept.iter().all(|p| !format!("{:?}", p.node).contains("master-page")), "{:?}", odf.kept.kept);
    // The master page of shapes is a background page, as before, and nothing is counted.
    assert_eq!(doc.pages.iter().filter(|p| p.background).count(), 1);
    assert!(report.not_kept.is_empty(), "{:?}", report.not_kept);
}

/// **A flat drawing's containers' markup that no field reads is counted** (a package keeps it in the document's data): an `office:scripts`, an unknown child of the root, a second child of
/// one name, a master styles' unknown child and a layer set's. A package of the same markup counts nothing. Red on main (770a162c): no count.
#[test]
fn a_flat_drawing_s_unheld_markup_is_counted_and_a_package_s_is_kept() {
    let flat = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document {NS} office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics"><office:scripts/><office:styles/><office:automatic-styles>{LAYOUT}</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name="layout"/><text:p/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/><text:p/></office:master-styles><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default">{}</draw:page></office:drawing></office:body><office:unknown/><office:styles/></office:document>"#,
        rect()
    );
    let (_, report) = read_ok(flat.as_bytes());
    // `office:scripts`, `office:unknown`, the second `office:styles`, the master styles' `text:p` and the layer set's.
    assert_eq!(report.not_kept("container_markup"), 5, "{:?}", report.not_kept);
    assert_eq!(note(&report).get(1).map(String::as_str), Some("5 pieces of the file's markup that no part of the drawing holds"));
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3"><office:styles/><office:automatic-styles>{LAYOUT}</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name="layout"/><text:p/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/><text:p/></office:master-styles></office:document-styles>"#
    );
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3"><office:scripts/><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="Default">{}</draw:page></office:drawing></office:body></office:document-content>"#,
        rect()
    );
    let bytes = crate::build::package(
        crate::build::DRAWING,
        vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), styles.into_bytes())],
    );
    let (_, report) = read_ok(&bytes);
    assert_eq!(report.not_kept("container_markup"), 0);
}

/// **The note is first, a line for each class, and each line fits the window's** whatever the count; the count of the warnings holds one for each line.
#[test]
fn the_note_is_first_and_each_line_fits() {
    let shapes = format!(
        r#"<draw:measure svg:x1="1cm" svg:y1="5cm" svg:x2="6cm" svg:y2="5cm"/>{}"#,
        r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm" draw:transform="skewX (0.5)"/>"#
    );
    let arial = r#"<style:font-face style:name="Arial" svg:font-family="Arial"/>"#;
    let (_, report) = read_ok(&package(&format!("{arial}{arial}"), "", "", "", &shapes));
    let lines = note(&report);
    assert_eq!(lines.len(), 3, "{:?}", report.warnings);
    assert_eq!(lines[0], NOT_KEPT_HEAD);
    assert!(report.warnings.len() > lines.len(), "the reader's own warnings follow: {:?}", report.warnings);
    assert!(lines.iter().all(|l| l.chars().count() <= NOT_KEPT_LINE_CHARS));
}

/// A package of `n` declarations of the font `Arial`, all in `styles.xml`: each the same (`repeat`), or each of its own name.
fn many_faces(n: usize, repeat: bool) -> Vec<u8> {
    let mut faces = String::with_capacity(n * 80);
    for k in 0..n {
        if repeat {
            faces.push_str(r#"<style:font-face style:name="Arial" svg:font-family="Arial"/>"#);
        } else {
            faces.push_str(&format!(r#"<style:font-face style:name="A{k:05}" svg:font-family="Arial"/>"#));
        }
    }
    package(&faces, "", "", "", rect())
}

/// **The count is charged and bounded** (the brief's 100,000 declarations): 100,000 repeated declarations are one face and one line with the count. Against a yardstick of as many declarations of
/// their own names (60,000: a read holds 65,536 faces at most, and refuses one more in words), the repeats walk within a small constant of it (the same elements; the attributes twice, since a
/// repeat is compared with the face it repeats) and hold fewer items: the count is kept as the read meets each, and nothing is walked again for it. Declarations no run names are the yardstick
/// itself: each is held and comes back, and the note says nothing of them.
#[test]
fn a_hundred_thousand_repeats_are_one_line_and_cost_what_a_plain_read_costs() {
    let (doc, repeats) = read_ok(&many_faces(100_000, true));
    assert_eq!(doc.fonts.len(), 1);
    assert_eq!(
        note(&repeats),
        [NOT_KEPT_HEAD.to_owned(), "99999 font declarations that repeat a name, which DiaCraft's one table of fonts holds once".to_owned()]
    );
    const N: usize = 60_000;
    let (_, repeats) = read_ok(&many_faces(N, true));
    let (_, yardstick) = read_ok(&many_faces(N, false));
    assert_eq!(repeats.not_kept("font_declaration"), (N - 1) as u64);
    assert_eq!(yardstick.not_kept("font_declaration"), 0);
    assert!(note(&yardstick).is_empty(), "unused declarations are held: {:?}", yardstick.warnings);
    let (a, b) = (repeats.work, yardstick.work);
    assert!(
        a.elements <= b.elements + 16 && a.attributes <= 2 * b.attributes + 16,
        "walked {:?} against {:?}",
        (a.elements, a.attributes),
        (b.elements, b.attributes)
    );
    assert!(a.items <= b.items, "a repeat holds no more than a face of its own: {} against {}", a.items, b.items);
}

/// The same, in real time, against the yardstick, back to back (run by hand: `--ignored`; no default test asserts a time).
#[test]
#[ignore]
fn a_hundred_thousand_repeats_open_within_a_constant_of_the_yardstick() {
    let repeats = many_faces(60_000, true);
    let yardstick = many_faces(60_000, false);
    let time = |bytes: &[u8]| {
        (0..3)
            .map(|_| {
                let at = std::time::Instant::now();
                read_ok(bytes);
                at.elapsed()
            })
            .min()
            .unwrap()
    };
    let (a, b) = (time(&repeats), time(&yardstick));
    eprintln!("60,000 repeats {a:?}, yardstick {b:?}");
    assert!(a <= b * 3 + std::time::Duration::from_millis(500), "{a:?} against {b:?}");
}

/// A drawing of `n` pages, each on a master page of its own that holds a child, and `n` master pages no page is on.
fn many_masters(n: usize) -> Vec<u8> {
    let mut masters = String::new();
    let mut pages = String::new();
    for k in 0..n {
        masters.push_str(&format!(r#"<style:master-page style:name="M{k}" style:page-layout-name="PM1"><presentation:notes/></style:master-page>"#));
        masters.push_str(&format!(r#"<style:master-page style:name="U{k}" style:page-layout-name="PM1"/>"#));
        pages.push_str(&format!(r#"<draw:page draw:name="p{k}" draw:master-page-name="M{k}"/>"#));
    }
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3"><office:body><office:drawing>{pages}</office:drawing></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3"><office:styles/><office:automatic-styles>{LAYOUT}</office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"#
    );
    crate::build::package(
        crate::build::DRAWING,
        vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), styles.into_bytes())],
    )
}

/// **The kept master pages are written in steps linear in them** (a copy written from the model): each page's master page is found in one lookup, not by a walk of every master page the
/// drawing keeps, so five times the pages and master pages cost about five times the steps, not twenty-five; and every master page comes back.
#[test]
fn kept_master_pages_are_written_in_linear_steps() {
    let cost = |n: usize| {
        let (doc, _) = read_ok(&many_masters(n));
        let written = diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
        let styles =
            String::from_utf8(diacraft_odg::Package::read(&written.bytes, &Limits::default()).unwrap().part("styles.xml").unwrap().to_vec()).unwrap();
        assert_eq!(styles.matches("<style:master-page ").count(), 2 * n, "every master page is written");
        assert_eq!(styles.matches("<presentation:notes/>").count(), n, "with its child");
        written.work.steps_used()
    };
    let (small, large) = (cost(200), cost(1_000));
    assert!(large < small * 8, "{small} steps for 200, {large} for 1,000");
}
