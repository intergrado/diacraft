//! The fix round of the content check: what the review of `canon.rs` found, each planted so that it fails on the code the review read.
//!
//! * a package's picture whose href and whose inline data disagree is what the href names (the package reader follows the href), not what the data holds;
//! * a name that both parts of a package hold is a package no flat file is the same as (`convert.rs` refuses to join them);
//! * the media type `convert.rs` writes for a picture it puts inline is content: a package's picture takes it before the comparison;
//! * percentages compare as numbers;
//! * the refusal of a side's budget says what it is, and the budget charges what a copy makes and not the white space inside a flat file's own data.

use diacraft_container::canon::Found;
use diacraft_odg::{Limits, compare_content};

use crate::build::{DRAWING, manifest, write};
use crate::canon::{diff, differs, graphic, only, same};
use crate::drawing::Drawing;
use crate::flat::{Flat, base64_lines, frame, picture};

fn base() -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d.common = r#"<style:style style:name="Std" style:family="graphic"/>"#.to_owned();
    d
}

const SIMPLE: &str = r#"xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad""#;
/// The media type the manifest of the tests' packages gives a `.png` entry, and so the one the converter writes for it.
const MIME: &str = " draw:mime-type=\"image/png\"";

fn data(bytes: &[u8]) -> String {
    format!("<office:binary-data>{}</office:binary-data>", base64_lines(bytes, 72, "\n"))
}

/// A drawing whose page holds `image`, with `Pictures/a.png` holding `entry` when given.
fn with_image(image: &str, entry: Option<&[u8]>) -> Drawing {
    let mut d = base().page(&frame(image));
    if let Some(bytes) = entry {
        d.files.push(("Pictures/a.png".to_owned(), bytes.to_vec()));
    }
    d
}

/// The flat file of a drawing whose picture is `bytes` held inline, as the converter writes it (with the media type, unless `mime` is empty).
fn flat_inline(bytes: &[u8], mime: &str) -> Vec<u8> {
    Flat::of(&with_image(&format!("<draw:image{mime} {SIMPLE}>{}</draw:image>", data(bytes)), None)).bytes()
}

fn names_the_href(href: &str, inline: Option<&[u8]>) -> String {
    format!("<draw:image xlink:href=\"{href}\" {SIMPLE}>{}</draw:image>", inline.map(data).unwrap_or_default())
}

#[test]
fn a_packages_href_wins_over_the_inline_data_beside_it_as_the_package_reader_has_it() {
    let (png, other) = (picture(300), picture(400));
    // The entry holds `other` and the data beside the href holds `png`: the reader shows `other`.
    let package = with_image(&names_the_href("Pictures/a.png", Some(&png)), Some(&other)).bytes();
    same(&flat_inline(&other, MIME), &package);
    let wrong = differs(&flat_inline(&png, MIME), &package);
    assert!(wrong.count >= 1, "{:?}", wrong.shown);
    // Two packages that differ only in the entry are different from each other and not both the same as one flat file.
    let agree = with_image(&names_the_href("Pictures/a.png", Some(&png)), Some(&png)).bytes();
    same(&flat_inline(&png, MIME), &agree);
    assert!(diff(&flat_inline(&png, MIME), &package).count >= 1, "the package whose entry is not the picture inline");
    assert!(diff(&agree, &package).count >= 1, "the two packages differ in the entry");
    // The same for a fill image: it takes no media type.
    let fill = |inline: Option<&[u8]>, entry: &[u8]| {
        let mut d = base();
        d.common = format!(
            "{}<draw:fill-image draw:name=\"fi\" xlink:href=\"Pictures/a.png\" {SIMPLE}>{}</draw:fill-image>",
            d.common,
            inline.map(data).unwrap_or_default()
        );
        d.files.push(("Pictures/a.png".to_owned(), entry.to_vec()));
        d
    };
    let flat_fill = |bytes: &[u8]| {
        let mut d = base();
        d.common = format!("{}<draw:fill-image draw:name=\"fi\" {SIMPLE}>{}</draw:fill-image>", d.common, data(bytes));
        Flat::of(&d).bytes()
    };
    same(&flat_fill(&other), &fill(Some(&png), &other).bytes());
    assert!(diff(&flat_fill(&png), &fill(Some(&png), &other).bytes()).count >= 1, "a fill image follows its href too");
}

#[test]
fn what_else_an_image_holds_stays_when_its_href_replaces_the_data_beside_it() {
    let (png, other) = (picture(300), picture(400));
    let title = "<svg:title>a picture</svg:title>";
    // The data is not the image's first child, and there are two of it: both are the data beside the href, and the title stays.
    let flat = Flat::of(&with_image(&format!("<draw:image{MIME} {SIMPLE}>{}{title}</draw:image>", data(&other)), None)).bytes();
    let one = with_image(&format!("<draw:image xlink:href=\"Pictures/a.png\" {SIMPLE}>{title}{}</draw:image>", data(&png)), Some(&other)).bytes();
    same(&flat, &one);
    let two =
        with_image(&format!("<draw:image xlink:href=\"Pictures/a.png\" {SIMPLE}>{}{title}{}</draw:image>", data(&png), data(&png)), Some(&other))
            .bytes();
    same(&flat, &two);
}

#[test]
fn an_href_that_names_no_entry_beside_inline_data_is_kept_in_a_package_and_dropped_only_in_a_flat_file() {
    let png = picture(300);
    for href in ["http://example.invalid/x.png", "Pictures/none.png", "../outside.png"] {
        let package = with_image(&names_the_href(href, Some(&png)), None).bytes();
        // The package reader shows no picture for it: the flat file that holds the data inline is another picture, and the difference is the href.
        for mime in [MIME, ""] {
            let d = differs(&flat_inline(&png, mime), &package);
            assert!(d.shown.iter().any(|x| x.found == Found::Attribute), "{href}: {:?}", d.shown);
        }
    }
    // In a flat file R33 says the href beside inline data is ignored: it is the picture inline.
    let flat_with_href = Flat::of(&with_image(&names_the_href("http://example.invalid/x.png", Some(&png)), None)).bytes();
    same(&flat_with_href, &flat_inline(&png, ""));
}

/// A package whose entry `Pictures/a.png` has the manifest row `row` (its media type), holding `bytes`, named by an href.
fn linked(row: &str, bytes: &[u8], image: &str) -> Vec<u8> {
    let d = with_image(image, Some(bytes));
    let mut files = d.files();
    let rows: Vec<(String, String)> =
        files.iter().map(|(n, _)| (n.clone(), if n == "Pictures/a.png" { row.to_owned() } else { "text/xml".to_owned() })).collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    files.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
    write(Some(DRAWING), &files)
}

#[test]
fn a_picture_named_by_an_href_takes_the_media_type_the_converter_writes_before_it_is_compared() {
    let png = picture(300);
    let image = format!("<draw:image xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
    let package = linked("image/png", &png, &image);
    // The manifest's type, written as `draw:mime-type`: a flat file with it is the same, without it or with another it is not.
    same(&flat_inline(&png, MIME), &package);
    only(&differs(&flat_inline(&png, ""), &package), Found::Attribute);
    only(&differs(&flat_inline(&png, " draw:mime-type=\"image/jpeg\""), &package), Found::Value);
    // The manifest's word wins, whatever the entry holds; a row with no type gives the type the bytes say (not the name's).
    same(&flat_inline(&png, " draw:mime-type=\"application/x-thing\""), &linked("application/x-thing", &png, &image));
    same(&flat_inline(&png, MIME), &linked("", &png, &image));
    let jpeg = [&[0xFF, 0xD8, 0xFF][..], &[7u8; 40]].concat();
    same(&flat_inline(&jpeg, " draw:mime-type=\"image/jpeg\""), &linked("", &jpeg, &image));
    let unknown = vec![5u8; 40];
    same(&flat_inline(&unknown, " draw:mime-type=\"application/octet-stream\""), &linked("", &unknown, &image));
    // An image that states its own type keeps it; and a package's own inline picture, with no href, is given none.
    let states = format!("<draw:image draw:mime-type=\"image/x-own\" xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
    same(&flat_inline(&png, " draw:mime-type=\"image/x-own\""), &linked("image/png", &png, &states));
    let inline_only = with_image(&format!("<draw:image {SIMPLE}>{}</draw:image>", data(&png)), None).bytes();
    same(&flat_inline(&png, ""), &inline_only);
    // The element in the default namespace: the media type is written under a prefix of the drawing namespace all the same.
    let default_ns = format!("<image xmlns=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
    same(&flat_inline(&png, MIME), &linked("image/png", &png, &default_ns));
}

/// The flat files that could be the pieces of `d`'s container (`auto` or the faces) in another way: the two parts' pieces in the other order, and each part's alone
/// (the first is the faithful assembly, `Flat::of(d)`).
fn flats_of(d: &Drawing, styles_piece: &str, content_piece: &str, container: &str) -> Vec<Vec<u8>> {
    let mut out = vec![Flat::of(d).bytes()];
    for (first, second) in [(content_piece, styles_piece), (styles_piece, ""), ("", content_piece)] {
        let mut f = Flat::of(d);
        let inner = format!("{first}{second}");
        if container == "auto" {
            f.auto = format!("<office:automatic-styles>{inner}</office:automatic-styles>");
        } else {
            f.fonts = format!("<office:font-face-decls>{inner}</office:font-face-decls>");
        }
        out.push(f.bytes());
    }
    out
}

fn marked(d: &diacraft_container::canon::ContentDiff) -> bool {
    d.shown.iter().any(|x| x.found == Found::Attribute && x.path.ends_with("@part-name-clash-0"))
}

#[test]
fn an_automatic_style_that_both_parts_hold_makes_the_package_differ_from_every_flat_file() {
    let (blue, red) = (graphic("gr1", "#0000ff"), graphic("gr1", "#ff0000"));
    // The review's case: `gr1` blue in `styles.xml` and red in `content.xml`. The package's shapes use the content's; a flat file reads the first it meets.
    let mut d = base();
    d.styles_auto = format!("{}{blue}", d.styles_auto);
    d.auto = red.clone();
    for (i, flat) in flats_of(&d, &d.styles_auto, &red, "auto").iter().enumerate() {
        let found = differs(flat, &d.bytes());
        assert!(found.count >= 1, "flat file {i}");
    }
    let found = differs(&Flat::of(&d).bytes(), &d.bytes());
    assert!(marked(&found), "{:?}", found.shown);
    // The same style twice, the same in both parts: still two of one name, for the reader of the package takes the content's.
    let mut twin = base();
    twin.styles_auto = format!("{}{red}", twin.styles_auto);
    twin.auto = red.clone();
    assert!(marked(&differs(&Flat::of(&twin).bytes(), &twin.bytes())));
    // A page layout of one name in both parts.
    let layout = |colour: &str| {
        format!("<style:page-layout style:name=\"PM9\"><style:page-layout-properties fo:margin-top=\"{colour}\"/></style:page-layout>")
    };
    let mut both = base();
    both.styles_auto = format!("{}{}", both.styles_auto, layout("1cm"));
    both.auto = layout("2cm");
    assert!(marked(&differs(&Flat::of(&both).bytes(), &both.bytes())));
    // What is no clash is the same still: other names, one name of two families (they are two keys), and a name in one part only.
    let mut apart = base();
    apart.styles_auto = format!("{}{}", apart.styles_auto, graphic("gr1", "#0000ff"));
    apart.auto = graphic("gr2", "#ff0000");
    same(&Flat::of(&apart).bytes(), &apart.bytes());
    let mut families = base();
    families.styles_auto = format!("{}{}", families.styles_auto, graphic("gr1", "#0000ff"));
    families.auto = r#"<style:style style:name="gr1" style:family="paragraph"/>"#.to_owned();
    same(&Flat::of(&families).bytes(), &families.bytes());
}

#[test]
fn a_face_that_both_parts_hold_and_is_not_the_same_makes_the_package_differ_and_the_same_face_is_one() {
    let (mono, other) =
        (r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#, r#"<style:font-face style:name="Mono" svg:font-family="Other"/>"#);
    // The review's case: `Mono` is another family in `styles.xml`.
    let d = base();
    let files = d.files();
    let styles = String::from_utf8(files.iter().find(|(n, _)| n == "styles.xml").map(|(_, b)| b.clone()).unwrap()).unwrap();
    let changed = crate::build::package(
        DRAWING,
        crate::build::replace(files, "styles.xml", styles.replacen("svg:font-family=\"Mono\"", "svg:font-family=\"Other\"", 1).into_bytes()),
    );
    for faces in [format!("{other}{mono}"), format!("{mono}{other}"), other.to_owned(), mono.to_owned()] {
        let mut f = Flat::of(&d);
        f.fonts = format!("<office:font-face-decls>{faces}</office:font-face-decls>");
        let found = differs(&f.bytes(), &changed);
        assert!(found.count >= 1, "{faces}");
    }
    let found = differs(&Flat::of(&d).bytes(), &changed);
    assert!(found.shown.iter().any(|x| x.found == Found::Attribute && x.path.ends_with("@part-name-clash-0")), "{:?}", found.shown);
    // The same face in both parts is one, and a flat file that holds it twice (as the converter writes it) is the same as one that holds it once.
    same(&Flat::of(&d).bytes(), &d.bytes());
    let mut twice = Flat::of(&d);
    twice.fonts = format!("<office:font-face-decls>{mono}{mono}</office:font-face-decls>");
    same(&twice.bytes(), &d.bytes());
    same(&twice.bytes(), &Flat::of(&d).bytes());
    // Two faces of one name that differ, in one flat file, are two.
    let mut two = Flat::of(&d);
    two.fonts = format!("<office:font-face-decls>{mono}{other}</office:font-face-decls>");
    assert!(!diff(&two.bytes(), &Flat::of(&d).bytes()).same());
    // A face only one part holds is one, and another name in the other part is no clash.
    let mut e = base();
    e.fonts = String::new();
    let mut files = e.files();
    let styles = String::from_utf8(files.iter().find(|(n, _)| n == "styles.xml").map(|(_, b)| b.clone()).unwrap()).unwrap();
    files = crate::build::replace(
        files,
        "styles.xml",
        styles
            .replace("<office:font-face-decls></office:font-face-decls>", &format!("<office:font-face-decls>{mono}</office:font-face-decls>"))
            .into_bytes(),
    );
    let mut flat = Flat::of(&e);
    flat.fonts = format!("<office:font-face-decls>{mono}</office:font-face-decls>");
    same(&flat.bytes(), &crate::build::package(DRAWING, files));
}

#[test]
fn a_name_is_the_element_the_family_and_the_style_name_and_nothing_else() {
    // A page layout and a list style of one name are two names: neither has a family, and the element tells them apart.
    let mut d = base();
    d.styles_auto = format!("{}<style:page-layout style:name=\"L\"/>", d.styles_auto);
    d.auto = r#"<text:list-style style:name="L"/>"#.to_owned();
    same(&Flat::of(&d).bytes(), &d.bytes());
    // The same element and name is one name, whatever else its attributes say.
    let mut twin = base();
    twin.styles_auto = format!("{}<style:page-layout style:name=\"L\" style:print-orientation=\"portrait\"/>", twin.styles_auto);
    twin.auto = r#"<style:page-layout style:name="L"/>"#.to_owned();
    assert!(differs(&Flat::of(&twin).bytes(), &twin.bytes()).shown.iter().any(|x| x.path.ends_with("@part-name-clash-0")));
}

// --------------------------------------------------------------------------------------------------------------------------------------------- percentages

/// A drawing whose common styles hold `wrapper` with the attribute `name` = `value`.
fn percent_drawing(wrapper: &str, name: &str, value: &str) -> Vec<u8> {
    let mut d = base();
    d.common = match wrapper {
        "gradient" => format!("{}<draw:gradient draw:name=\"G\" draw:style=\"linear\" {name}=\"{value}\"/>", d.common),
        "dash" => format!("{}<draw:stroke-dash draw:name=\"D\" {name}=\"{value}\"/>", d.common),
        other => {
            format!("{}<style:style style:name=\"P\" style:family=\"graphic\"><style:{other}-properties {name}=\"{value}\"/></style:style>", d.common)
        }
    };
    d.bytes()
}

#[test]
fn a_percentage_is_a_number_and_75_percent_is_75_point_0_percent() {
    // (the element, the attribute) of each percentage the specs list: fill, shadow, line, character (odg-styles), a gradient's and a dash's.
    let listed = [
        ("graphic", "draw:opacity"),
        ("graphic", "draw:shadow-opacity"),
        ("graphic", "svg:stroke-opacity"),
        ("text", "style:text-scale"),
        ("text", "style:text-position"),
        ("gradient", "draw:start-intensity"),
        ("gradient", "draw:end-intensity"),
        ("gradient", "draw:border"),
        ("gradient", "draw:cx"),
        ("gradient", "draw:cy"),
        ("dash", "draw:dots1-length"),
        ("dash", "draw:dots2-length"),
        ("dash", "draw:distance"),
    ];
    for (wrapper, name) in listed {
        let one = |v: &str| percent_drawing(wrapper, name, v);
        same(&one("75%"), &one("75.0%"));
        same(&one("75%"), &one("75.00%"));
        assert!(!diff(&one("75%"), &one("76%")).same(), "{name}: 75% against 76%");
        assert!(!diff(&one("75%"), &one("75")).same(), "{name}: 75% against 75");
        assert!(!diff(&one("75%"), &one("7.5%")).same(), "{name}: 75% against 7.5%");
        assert!(!diff(&one("75%"), &one("75 %")).same(), "{name}: the unit stands directly after the number");
    }
    // A stroke's opacity is a number from 0 to 1 or a percentage: `0.5` is `0.50` (the number's spelling may differ).
    let opacity = |v: &str| percent_drawing("graphic", "svg:stroke-opacity", v);
    same(&opacity("0.5"), &opacity("0.50"));
    assert!(!diff(&opacity("0.5"), &opacity("0.6")).same());
    // Lengths and the paragraph's own percentages stay what they were.
    let line = |v: &str| percent_drawing("graphic", "fo:line-height", v);
    same(&line("150%"), &line("150.0%"));
    assert!(!diff(&line("150%"), &line("1.5cm")).same());
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the budget

/// The least cap on a part with which the flat file of `base()` compares with `package`, found by halving: what one side may add is a quarter of the cap
/// (rounded down), so the least cap is four times what the package's copies are charged.
fn least_cap(package: &[u8]) -> u64 {
    let flat = Flat::of(&base()).bytes();
    let fits = |cap: u64| {
        let mut limits = Limits::default();
        limits.container.max_part_bytes = cap;
        compare_content(&flat, package, &limits).is_ok()
    };
    let (mut low, mut high) = (0u64, 8 << 20);
    assert!(fits(high) && !fits(low));
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

fn naming(frames: usize, bytes: usize) -> Vec<u8> {
    let image = format!("<draw:image xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
    let mut d = base().page(&frame(&image).repeat(frames));
    d.files.push(("Pictures/a.png".to_owned(), picture(bytes)));
    d.bytes()
}

#[test]
fn a_copy_of_a_picture_is_charged_the_base64_it_makes_four_thirds_of_its_bytes() {
    // A picture of 3,000 bytes is 4,000 characters of base64, and the reference is given its media type, `draw:mime-type="image/png"`: 14 and 9 characters and the
    // 64 of an attribute's place. The least cap with one more reference is four times that greater, to the byte.
    let attribute = "draw:mime-type".len() + "image/png".len() + 64;
    let (three, four) = (least_cap(&naming(3, 3_000)), least_cap(&naming(4, 3_000)));
    assert_eq!(four - three, 4 * (4_000 + attribute) as u64);
    // Not a multiple of three: the padding is made too.
    let (a, b) = (least_cap(&naming(3, 3_001)), least_cap(&naming(4, 3_001)));
    assert_eq!(b - a, 4 * (4_004 + attribute) as u64);
}

#[test]
fn an_image_in_the_default_namespace_is_charged_the_declaration_it_is_given_too() {
    // `<image xmlns="...drawing...">` has no prefix of its own to write `draw:mime-type` under, so it declares one (`xmlns:canon-draw`) and the attribute is
    // `canon-draw:mime-type`: the reference is charged its base64, the attribute's name and value, the declaration's, and the 64 of an attribute's place.
    const DRAW_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
    let frames = |n: usize| {
        let image = format!("<image xmlns=\"{DRAW_NS}\" xlink:href=\"Pictures/a.png\" {SIMPLE}/>");
        let mut d = base().page(&frame(&image).repeat(n));
        d.files.push(("Pictures/a.png".to_owned(), picture(3_000)));
        d.bytes()
    };
    let per_reference = 4_000 + "canon-draw:mime-type".len() + "image/png".len() + "xmlns:canon-draw".len() + DRAW_NS.len() + 64;
    assert_eq!(least_cap(&frames(4)) - least_cap(&frames(3)), 4 * per_reference as u64);
}

#[test]
fn white_space_inside_a_flat_files_own_data_is_not_a_made_byte() {
    // Four bytes of picture and 40,000 characters of white space inside the data. One side may add a quarter of the cap: 16 KiB here; the white space is the
    // file's own and costs nothing, what is charged is what the data decodes to.
    let d =
        base().page(&frame(&format!("<draw:image {SIMPLE}><office:binary-data>QUJD{}QUJD</office:binary-data></draw:image>", " ".repeat(40_000))));
    let flat = Flat::of(&d).bytes();
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 64 << 10;
    assert!(flat.len() < 64 << 10);
    let found = compare_content(&flat, &flat, &limits).expect("compares");
    assert!(found.same());
    // The same picture with no white space is the same picture.
    let tight =
        Flat::of(&base().page(&frame(&format!("<draw:image {SIMPLE}><office:binary-data>QUJDQUJD</office:binary-data></draw:image>")))).bytes();
    let found = compare_content(&flat, &tight, &limits).expect("compares");
    assert!(found.same(), "{:?}", found.shown);
    // Past what the data decodes to, the refusal stands: 20,000 characters of data are 15,000 bytes, and a quarter of 32 KiB is 8,192.
    let big = base().page(&frame(&format!("<draw:image {SIMPLE}><office:binary-data>{}</office:binary-data></draw:image>", "QUJD".repeat(5_000))));
    limits.container.max_part_bytes = 32 << 10;
    let refused = compare_content(&Flat::of(&big).bytes(), &tight, &limits).map(|_| ()).map_err(|e| e.class());
    assert_eq!(refused, Err("too_much_xml"));
}

#[test]
fn the_refusal_of_a_sides_budget_says_what_it_is_in_words() {
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 1 << 20;
    let flat = Flat::of(&base()).bytes();
    let refused = compare_content(&flat, &naming(3, 100_000), &limits).map(|_| ()).unwrap_err();
    let words = refused.to_string();
    assert_eq!(refused.class(), "too_much_xml");
    assert!(words.contains("copy") && words.contains("262144"), "{words}");
    assert!(!words.contains("of XML") && !words.contains("package holds"), "{words}");
    // A budget of the flat file's own data, and of declarations, says the same.
    let data = format!("<draw:image {SIMPLE}><office:binary-data>{}</office:binary-data></draw:image>", "QUJD".repeat(100_000));
    let big = Flat::of(&base().page(&frame(&data))).bytes();
    let refused = compare_content(&big, &flat, &limits).map(|_| ()).unwrap_err();
    assert!(refused.to_string().contains("copy"), "{refused}");
    let (flat, pkg) = crate::canon_hostile::declaring(1, 1_000, &"x".repeat(100_000), 1);
    let refused = compare_content(&flat, &pkg, &limits).map(|_| ()).unwrap_err();
    assert!(refused.to_string().contains("copy"), "{refused}");
    // The caps that are the XML's own keep their words.
    let xml = Limits { max_xml_bytes: 600, ..Limits::default() };
    let refused = compare_content(&flat, &pkg, &xml).map(|_| ()).unwrap_err();
    assert!(refused.to_string().contains("of XML"), "{refused}");
}
