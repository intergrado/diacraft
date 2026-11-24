//! In the `.odg` content check (second round): **DiaCraft's own carrier counts as nothing when it is extra** (the entries under `DiaCraft/`,
//! their manifest rows, and the attributes and elements of DiaCraft's namespace, in the written file), **automatic styles are matched by distinct content** (copies of one style count as
//! one, a shape follows the content it resolves to), and **a reference from one automatic style to another is resolved**, bounded, a circle or a chain too deep compared as written. The
//! reference table follows the ODF 1.4 schema as checked it (the notes page and the handout master ask a drawing-page style; the references of a hyperlink, a
//! conditional paragraph, a section, a ruby, a list level, a data style and a list of class names). The check is **oriented** for the carrier: the first file is the one the drawing came
//! from, and what it holds must be in the second. Every package is written here by hand from the ODF standard.

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_odg::{Limits, compare_content};

use crate::build::{DRAWING, manifest, write};
use crate::canon::{at, diff, differs, graphic, only, paragraph, same, shape_with};
use crate::canon_free_names::{listing, pairs, row, rows_of, styled, with_picture};
use crate::drawing::Drawing;

/// The check with the source first: a carrier makes the answer depend on which file is which, so these tests never ask for the symmetric count.
fn check(source: &[u8], written: &[u8]) -> ContentDiff {
    compare_content(source, written, &Limits::default()).unwrap()
}

#[track_caller]
fn nothing(source: &[u8], written: &[u8]) {
    let d = check(source, written);
    assert!(d.same(), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------------------ the carrier's entries

/// `d` with the entries `extra` (name, bytes, media type) and their rows, and the row of the folder `DiaCraft/` typed `folder` when there is one.
fn carrying(d: &Drawing, extra: &[(&str, &[u8], &str)], folder: Option<&str>) -> Vec<u8> {
    let mut files = d.files();
    let mut rows = rows_of(d);
    for (name, bytes, media) in extra {
        files.push(((*name).to_owned(), bytes.to_vec()));
        rows.push(row(name, media));
    }
    if let Some(media) = folder {
        rows.push(row("DiaCraft/", media));
    }
    let rows = pairs(&rows);
    let mut entries = files;
    entries.push(("META-INF/manifest.xml".to_owned(), manifest(DRAWING, &rows).into_bytes()));
    write(Some(DRAWING), &entries)
}

const DOCUMENT: (&str, &[u8], &str) =
    ("DiaCraft/document.xml", b"<document xmlns=\"urn:d\" version=\"2\"/>", "application/vnd.diacraft.document+xml");
const BLOB: (&str, &[u8], &str) = ("DiaCraft/blob0.bin", b"abc", "application/octet-stream");

#[test]
fn a_carrier_the_written_file_adds_is_no_difference() {
    let d = with_picture();
    let plain = listing(&d, &rows_of(&d));
    // The entries, the folder's row (typed as DiaCraft types it or as anything), and the rows of the entries: nothing.
    nothing(&plain, &carrying(&d, &[DOCUMENT, BLOB], Some("application/vnd.diacraft.parts")));
    nothing(&plain, &carrying(&d, &[DOCUMENT], Some("application/x-other")));
    nothing(&plain, &carrying(&d, &[BLOB], None));
    // Another case, as the reader reads the folder's name.
    nothing(&plain, &carrying(&d, &[("diacraft/y.bin", b"x", "application/octet-stream"), ("DIACRAFT/z.xml", b"<a/>", "text/xml")], None));
    // The carrier is DiaCraft's and no other entry is: a folder of another name that is extra is a difference.
    let other = check(&plain, &carrying(&d, &[("DiaCraft2/y.bin", b"x", "application/octet-stream")], None));
    only(&other, Found::ExtraPart);
}

#[test]
fn what_the_source_held_of_the_carrier_is_compared_as_written() {
    let d = with_picture();
    let plain = listing(&d, &rows_of(&d));
    let both = carrying(&d, &[DOCUMENT, BLOB], Some("application/vnd.diacraft.parts"));
    // The source's entries must be in the written file.
    let lost = check(&both, &plain);
    assert!(lost.shown.iter().any(|x| x.found == Found::MissingPart && x.part == "DiaCraft/document.xml"), "{:?}", lost.shown);
    assert!(lost.shown.iter().any(|x| x.found == Found::MissingPart && x.part == "DiaCraft/blob0.bin"), "{:?}", lost.shown);
    // One of two lost, and one changed.
    let one = carrying(&d, &[DOCUMENT], Some("application/vnd.diacraft.parts"));
    assert!(!check(&both, &one).same());
    nothing(&one, &both);
    nothing(&both, &both);
    let changed = carrying(
        &d,
        &[("DiaCraft/document.xml", b"<document xmlns=\"urn:d\" version=\"3\"/>", DOCUMENT.2), BLOB],
        Some("application/vnd.diacraft.parts"),
    );
    assert!(!check(&both, &changed).same(), "an entry the source held, written otherwise");
    let bytes = carrying(&d, &[DOCUMENT, ("DiaCraft/blob0.bin", b"abd", BLOB.2)], Some("application/vnd.diacraft.parts"));
    assert!(!check(&both, &bytes).same());
}

#[test]
fn an_object_that_rides_only_in_the_carrier_still_counts() {
    let d = with_picture();
    let mut with_object = d.clone();
    with_object.files.push(("Object 1/content.xml".to_owned(), b"<a/>".to_vec()));
    let source = listing(&with_object, &rows_of(&with_object));
    // The written file holds it only as a carrier entry: its source entry is missing.
    let written = carrying(&d, &[BLOB], Some("application/vnd.diacraft.parts"));
    let found = check(&source, &written);
    only(&found, Found::MissingPart);
    assert_eq!(found.shown[0].part, "Object 1/content.xml");
}

// ------------------------------------------------------------------------------------------------------------------- the carrier's markup

fn marked(name: &str, colour: &str, id: &str) -> String {
    graphic(name, colour).replace("<style:graphic-properties ", &format!("<style:graphic-properties diacraft:version=\"1\" diacraft:id=\"{id}\" "))
}

fn with_style(style: &str) -> Vec<u8> {
    Drawing::a4().auto(style).page(&styled("s", "gr1", "1cm")).bytes()
}

#[test]
fn attributes_of_the_extension_count_as_nothing_when_only_the_written_file_has_them() {
    let plain = with_style(&graphic("gr1", "#ff0000"));
    let marked_one = with_style(&marked("gr1", "#ff0000", "5"));
    nothing(&plain, &marked_one);
    // The source's own must be there, with its value.
    let lost = check(&marked_one, &plain);
    assert_eq!((lost.count, lost.shown[0].found), (2, Found::Attribute), "{:?}", lost.shown);
    nothing(&marked_one, &marked_one);
    only(&check(&marked_one, &with_style(&marked("gr1", "#ff0000", "6"))), Found::Value);
    // Beside a change the carrier hides none: the colour is the one difference.
    only(&check(&plain, &with_style(&marked("gr1", "#ff0001", "5"))), Found::Value);
}

#[test]
fn an_element_of_the_extension_counts_as_nothing_when_only_the_written_file_has_it() {
    let shape = |inner: &str| {
        Drawing::a4().page(&shape_with("s", &at("1cm"), "x").replace("</draw:custom-shape>", &format!("{inner}</draw:custom-shape>"))).bytes()
    };
    let plain = shape("");
    let note = shape("<diacraft:note diacraft:a=\"1\"/>");
    nothing(&plain, &note);
    only(&check(&note, &plain), Found::Element);
    nothing(&note, &note);
    only(&check(&note, &shape("<diacraft:note diacraft:a=\"2\"/>")), Found::Element);
    // Two of the source's: both in the written file (among more of its own), and one of them lost.
    let two = shape("<diacraft:note/><diacraft:other/>");
    nothing(&two, &shape("<diacraft:other/><diacraft:note/><diacraft:more/>"));
    only(&check(&two, &shape("<diacraft:other/>")), Found::Element);
}

#[test]
fn what_only_the_written_file_marked_does_not_move_the_children_off_their_places() {
    let shapes = |marks: bool, dropped: Option<usize>| -> Vec<u8> {
        let body: String = (0..6)
            .filter(|i| Some(*i) != dropped)
            .map(|i| {
                shape_with(
                    &format!("s{i}"),
                    &format!(
                        "{}svg:x=\"{i}cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"",
                        if marks { format!("diacraft:id=\"{i}\" ") } else { String::new() }
                    ),
                    "x",
                )
            })
            .collect();
        Drawing::a4().page(&body).bytes()
    };
    // Every shape of the written file is marked and the third was taken out: one difference, not a shift of all that follows.
    only(&check(&shapes(false, None), &shapes(true, Some(2))), Found::Element);
    nothing(&shapes(false, None), &shapes(true, None));
}

// ------------------------------------------------------------------------------------------------------------------- copies of a style

fn pages(auto: &str, per_page: &[&str]) -> Vec<u8> {
    let mut d = Drawing::a4().auto(auto);
    for (i, style) in per_page.iter().enumerate() {
        d = d.page(&styled(&format!("s{i}"), style, "1cm"));
    }
    d.bytes()
}

#[test]
fn copies_of_one_style_are_one_style_and_a_shape_follows_the_content() {
    // A writer's copies, one for each page: five shapes of one shared style against five copies of it.
    let shared = pages(&graphic("gr1", "#ff0000"), &["gr1", "gr1", "gr1", "gr1", "gr1"]);
    let copies = pages(&(1..=5).map(|i| graphic(&format!("gr{i}"), "#ff0000")).collect::<String>(), &["gr1", "gr2", "gr3", "gr4", "gr5"]);
    same(&shared, &copies);
    // Some copies unused, some named otherwise: still one.
    let spare = pages(&(1..=4).map(|i| graphic(&format!("c{i}"), "#ff0000")).collect::<String>(), &["c3", "c4"]);
    same(&pages(&graphic("gr1", "#ff0000"), &["gr1", "gr1"]), &spare);
    // The controls: a copy that differs is a style that differs, and a shape that names the other one differs.
    let three = pages(&graphic("gr1", "#ff0000"), &["gr1", "gr1", "gr1"]);
    let other = pages(&format!("{}{}{}", graphic("gr1", "#ff0000"), graphic("gr2", "#ff0001"), graphic("gr3", "#ff0000")), &["gr1", "gr2", "gr3"]);
    assert!(!diff(&three, &other).same());
    let two = |first: &str, second: &str| pages(&format!("{}{}", graphic("a", "#ff0000"), graphic("b", "#0000ff")), &[first, second]);
    assert!(!diff(&two("a", "b"), &two("a", "a")).same(), "a shape that names the other style");
}

#[test]
fn the_copies_the_writer_marks_are_the_style_it_copied() {
    // The second page's shape names a copy that carries DiaCraft's attributes, which the source's one style lacks.
    let source = pages(&graphic("gr1", "#ff0000"), &["gr1", "gr1"]);
    let written = pages(&format!("{}{}", graphic("gr1", "#ff0000"), marked("gr3", "#ff0000", "2")), &["gr1", "gr3"]);
    nothing(&source, &written);
    // The copy that differs in what is no carrier is a style of its own.
    let changed = pages(&format!("{}{}", graphic("gr1", "#ff0000"), marked("gr3", "#ff0001", "2")), &["gr1", "gr3"]);
    assert!(!check(&source, &changed).same());
}

#[test]
fn copies_that_have_no_match_count_as_one_and_not_each() {
    let two = |name: &str, colour: &str| graphic(name, colour);
    let a = Drawing::a4().auto(&format!("{}{}", two("x1", "#ff0000"), two("x2", "#ff0000"))).bytes();
    let b = Drawing::a4().auto(&two("y", "#0000ff")).bytes();
    // The style of A is missing once, and the style of B is extra once.
    assert_eq!(differs(&a, &b).count, 2);
}

// -------------------------------------------------------------------------------------------- references from one automatic style to another

fn list_style(name: &str, level: &str) -> String {
    format!(
        "<text:list-style style:name=\"{name}\"><text:list-level-style-bullet text:level=\"1\" {level} text:bullet-char=\"o\"/></text:list-style>"
    )
}

fn text_style(name: &str, weight: &str) -> String {
    format!("<style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties fo:font-weight=\"{weight}\"/></style:style>")
}

fn listed_paragraph(name: &str, list: &str) -> String {
    format!(
        "<style:style style:name=\"{name}\" style:family=\"paragraph\" style:list-style-name=\"{list}\"><style:paragraph-properties fo:margin-left=\"1cm\"/></style:style>"
    )
}

fn with_body(auto: &str, body: &str) -> Vec<u8> {
    let mut d = Drawing::a4().auto(auto);
    d.pages = format!("<draw:page draw:name=\"p1\" draw:master-page-name=\"Default\">{body}</draw:page>");
    d.bytes()
}

fn text_p(style: &str) -> String {
    format!("<text:p text:style-name=\"{style}\">x</text:p>")
}

#[test]
fn a_style_that_names_a_list_style_follows_its_content() {
    let build = |p: &str, l: &str, t: &str| {
        with_body(&format!("{}{}{}", listed_paragraph(p, l), list_style(l, &format!("text:style-name=\"{t}\"")), text_style(t, "bold")), &text_p(p))
    };
    // A paragraph style, the list style it names, and the text style that list names: renamed all the way down.
    same(&build("P1", "L1", "T1"), &build("P7", "L5", "T9"));
    // The controls: the list's text style differs, and the paragraph style names another list.
    let weight = |w: &str| {
        with_body(&format!("{}{}{}", listed_paragraph("P1", "L1"), list_style("L1", "text:style-name=\"T1\""), text_style("T1", w)), &text_p("P1"))
    };
    assert!(!diff(&weight("bold"), &weight("normal")).same());
    let two = |first: &str, second: &str| {
        with_body(
            &format!(
                "{}{}{}{}",
                listed_paragraph("P1", first),
                listed_paragraph("P2", second),
                list_style("L1", "text:level-x=\"1\""),
                list_style("L2", "text:level-x=\"2\"")
            ),
            &format!("{}{}", text_p("P1"), text_p("P2")),
        )
    };
    assert!(!diff(&two("L1", "L2"), &two("L2", "L1")).same(), "a paragraph style that names the other list");
    // A name no definition has (a common list style) is compared as written.
    let common = |l: &str| with_body(&listed_paragraph("P1", l), &text_p("P1"));
    assert!(!diff(&common("Numbering_20_1"), &common("Numbering_20_2")).same());
}

#[test]
fn a_style_that_names_a_data_style_follows_its_content() {
    let number = |name: &str, digits: &str| {
        format!(
            "<number:number-style style:name=\"{name}\" xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\"><number:number number:min-integer-digits=\"{digits}\"/></number:number-style>"
        )
    };
    let data = |style: &str, name: &str| format!("<style:style style:name=\"{style}\" style:family=\"paragraph\" style:data-style-name=\"{name}\"/>");
    let build = |style: &str, name: &str, digits: &str| with_body(&format!("{}{}", number(name, digits), data(style, name)), &text_p(style));
    same(&build("P1", "N1", "1"), &build("P4", "N7", "1"));
    assert!(!diff(&build("P1", "N1", "1"), &build("P1", "N1", "2")).same());
}

#[test]
fn a_list_level_names_its_text_style_and_its_number_format() {
    let build = |list: &str, text: &str, format: &str, weight: &str, digits: &str| {
        with_body(
            &format!(
                "<text:list-style style:name=\"{list}\"><text:list-level-style-number text:level=\"1\" text:style-name=\"{text}\" style:num-list-format-name=\"{format}\"/></text:list-style>\
                 {}<number:num-list-format style:name=\"{format}\" xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\" number:digits=\"{digits}\"/>",
                text_style(text, weight)
            ),
            &format!("<text:list text:style-name=\"{list}\"/>"),
        )
    };
    same(&build("L1", "T1", "F1", "bold", "1"), &build("L7", "T8", "F9", "bold", "1"));
    assert!(!diff(&build("L1", "T1", "F1", "bold", "1"), &build("L1", "T1", "F1", "italic", "1")).same(), "the level's text style differs");
    assert!(!diff(&build("L1", "T1", "F1", "bold", "1"), &build("L1", "T1", "F1", "bold", "2")).same(), "the number format differs");
}

#[test]
fn a_name_in_a_style_that_names_no_definition_of_the_other_file_is_a_difference() {
    // The first file's paragraph style names the list `L1`, which it defines; the second file's paragraph style names `L1` too, but its list is `L9`: the second file's style names a list
    // that does not exist. The two lists are the same content under two names, and the two paragraph styles read the same text, so a check that read the text alone called the files the same
    // (a false same that the first round of this ruling, had for a name inside a style).
    let build = |list: &str| with_body(&format!("{}{}", listed_paragraph("P1", "L1"), list_style(list, "text:level-x=\"1\"")), &text_p("P1"));
    assert!(!diff(&build("L1"), &build("L9")).same());
    // And the other way round: the second file names the list it has, under another name, and the first names its own.
    let both = |list: &str| with_body(&format!("{}{}", listed_paragraph("P1", list), list_style(list, "text:level-x=\"1\"")), &text_p("P1"));
    same(&both("L1"), &both("L9"));
}

#[test]
fn a_parent_is_never_an_automatic_style_and_is_compared_as_written() {
    // ODF 1.4 §19.510: the parent of a style is a common style, so a name there is not read as the content of a definition.
    let build = |child: &str, parent: &str| {
        let style = |name: &str, parent: &str| {
            format!("<style:style style:name=\"{name}\" style:family=\"paragraph\" style:parent-style-name=\"{parent}\"/>")
        };
        with_body(&format!("{}{}", style(child, parent), paragraph(parent, "1cm")), &text_p(child))
    };
    assert!(!diff(&build("P1", "P0"), &build("P1", "P6")).same(), "the parent's name is kept");
}

/// `n` text styles and list styles that name one another in a chain: a paragraph style names the first list, whose level names a text style, which names the next list, and so on.
fn chain(n: usize, prefix: &str) -> Vec<u8> {
    let mut auto = String::from(&listed_paragraph(&format!("{prefix}P"), &format!("{prefix}L0")));
    for i in 0..n {
        let next = if i + 1 < n { format!("{prefix}T{}", i + 1) } else { String::from("Common") };
        auto.push_str(&list_style(&format!("{prefix}L{i}"), &format!("text:style-name=\"{next}\"")));
        if i + 1 < n {
            auto.push_str(&format!(
                "<style:style style:name=\"{prefix}T{}\" style:family=\"text\" style:list-style-name=\"{prefix}L{}\"><style:text-properties fo:font-weight=\"bold\"/></style:style>",
                i + 1,
                i + 1
            ));
        }
    }
    with_body(&auto, &text_p(&format!("{prefix}P")))
}

#[test]
fn a_chain_of_references_is_resolved_and_one_that_is_too_long_is_compared_as_written() {
    // Short: renamed all the way.
    same(&chain(8, "a"), &chain(8, "b"));
    // Long: more than the check follows, so the names are compared as written (a difference, never a false same), whichever file is first.
    assert!(!diff(&chain(80, "a"), &chain(80, "b")).same());
    // The same long chain, written as it was, is the same.
    nothing(&chain(80, "a"), &chain(80, "a"));
}

#[test]
fn a_circle_of_references_is_compared_as_written() {
    // A text style names a list, whose level names the text style.
    let circle = |t: &str, l: &str| {
        let text = format!(
            "<style:style style:name=\"{t}\" style:family=\"text\" style:list-style-name=\"{l}\"><style:text-properties fo:font-weight=\"bold\"/></style:style>"
        );
        with_body(
            &format!("{text}{}", list_style(l, &format!("text:style-name=\"{t}\""))),
            &format!("<text:span text:style-name=\"{t}\">x</text:span>"),
        )
    };
    nothing(&circle("T1", "L1"), &circle("T1", "L1"));
    assert!(!diff(&circle("T1", "L1"), &circle("T9", "L9")).same(), "a circle is compared as written");
    // Beside it a style that is not in the circle is still renamed.
    let both = |t: &str, l: &str, p: &str| {
        let plain = paragraph(p, "1cm");
        let text = format!("<style:style style:name=\"{t}\" style:family=\"text\" style:list-style-name=\"{l}\"/>");
        with_body(&format!("{plain}{text}{}", list_style(l, &format!("text:style-name=\"{t}\""))), &text_p(p))
    };
    let (x, y) = (both("T1", "L1", "P1"), both("T1", "L1", "P8"));
    same(&x, &y);
}

// ------------------------------------------------------------------------------------------------------------------- the reference table

fn drawing_page(name: &str) -> String {
    format!("<style:style style:name=\"{name}\" style:family=\"drawing-page\"><style:drawing-page-properties draw:fill=\"none\"/></style:style>")
}

#[test]
fn a_notes_page_and_a_handout_master_name_a_drawing_page_style() {
    // ODF 1.4 §19.219.25 and §19.219.26: `draw:style-name` of `presentation:notes` and of `style:handout-master` is a drawing-page style, not a graphic one.
    let notes = |name: &str| with_body(&drawing_page(name), &format!("<presentation:notes draw:style-name=\"{name}\"/>"));
    same(&notes("dp1"), &notes("dp9"));
    let handout = |name: &str, layout: &str| {
        let mut d = Drawing::a4();
        d.styles_auto = format!(
            "<style:page-layout style:name=\"{layout}\"><style:page-layout-properties fo:page-width=\"21cm\"/></style:page-layout>{}",
            drawing_page(name)
        );
        d.masters = format!("<style:handout-master draw:style-name=\"{name}\" style:page-layout-name=\"{layout}\"/>");
        d.bytes()
    };
    same(&handout("dp1", "PM1"), &handout("dp4", "PM7"));
    // A graphic style of the name is not what they ask for: a drawing page of one name and a graphic style of the other are not the same.
    let graphic_named = |name: &str| with_body(&graphic(name, "#ff0000"), &format!("<presentation:notes draw:style-name=\"{name}\"/>"));
    assert!(!diff(&graphic_named("dp1"), &graphic_named("dp9")).same());
}

#[test]
fn a_hyperlink_a_conditional_paragraph_a_section_and_a_ruby_name_the_families_the_schema_gives() {
    let link = |style: &str, visited: &str, t1: &str, t2: &str| {
        with_body(
            &format!("{}{}", text_style(t1, "bold"), text_style(t2, "italic")),
            &format!("<text:a xlink:href=\"u\" text:style-name=\"{style}\" text:visited-style-name=\"{visited}\">x</text:a>"),
        )
    };
    same(&link("T1", "T2", "T1", "T2"), &link("T8", "T9", "T8", "T9"));
    // The same two styles, the link naming the other one as its visited style: another link.
    assert!(!diff(&link("T1", "T2", "T1", "T2"), &link("T1", "T1", "T1", "T2")).same(), "the visited style is the other");
    let conditional = |p: &str, q: &str| {
        with_body(
            &format!("{}{}", paragraph(p, "1cm"), paragraph(q, "2cm")),
            &format!("<text:p text:style-name=\"{p}\" text:cond-style-name=\"{q}\">x</text:p>"),
        )
    };
    same(&conditional("P1", "P2"), &conditional("P5", "P6"));
    let section = |name: &str| {
        with_body(
            &format!(
                "<style:style style:name=\"{name}\" style:family=\"section\"><style:section-properties text:dont-balance-text-columns=\"true\"/></style:style>"
            ),
            &format!("<text:section text:style-name=\"{name}\"/>"),
        )
    };
    same(&section("Sect1"), &section("Sect9"));
    let ruby = |name: &str| {
        with_body(
            &format!("<style:style style:name=\"{name}\" style:family=\"ruby\"><style:ruby-properties style:ruby-position=\"above\"/></style:style>"),
            &format!("<text:ruby text:style-name=\"{name}\"/>"),
        )
    };
    same(&ruby("Ru1"), &ruby("Ru8"));
    // A section style is not a paragraph style: a `text:p` that names a section style is a reference that asks the wrong family.
    let wrong = |name: &str| {
        with_body(
            &format!(
                "<style:style style:name=\"{name}\" style:family=\"section\"><style:section-properties text:dont-balance-text-columns=\"true\"/></style:style>"
            ),
            &text_p(name),
        )
    };
    assert!(!diff(&wrong("Sect1"), &wrong("Sect9")).same());
}

#[test]
fn a_list_of_class_names_names_each_style_it_lists() {
    let named = |a: &str, b: &str| {
        with_body(&format!("{}{}", paragraph(a, "1cm"), paragraph(b, "2cm")), &format!("<text:p text:class-names=\"{a} {b}\">x</text:p>"))
    };
    same(&named("P1", "P2"), &named("P7", "P8"));
    // The same two styles listed in the other order are another list.
    let listed = |list: &str| {
        with_body(&format!("{}{}", paragraph("P1", "1cm"), paragraph("P2", "2cm")), &format!("<text:p text:class-names=\"{list}\">x</text:p>"))
    };
    assert!(!diff(&listed("P1 P2"), &listed("P2 P1")).same(), "the list is in order");
    let shapes = |a: &str, b: &str| {
        with_body(
            &format!("{}{}", graphic(a, "#ff0000"), graphic(b, "#0000ff")),
            &shape_with("s", &format!("draw:class-names=\"{a} {b}\" {}", at("1cm")), "x"),
        )
    };
    same(&shapes("gr1", "gr2"), &shapes("gr6", "gr7"));
    let presented = |a: &str| {
        with_body(
            &format!("<style:style style:name=\"{a}\" style:family=\"presentation\"><style:graphic-properties draw:fill=\"none\"/></style:style>"),
            &shape_with("s", &format!("presentation:class-names=\"{a}\" {}", at("1cm")), "x"),
        )
    };
    same(&presented("pr1"), &presented("pr9"));
    // A name in the list that no automatic style has stays as written beside one that has.
    let mixed = |a: &str| with_body(&paragraph(a, "1cm"), &format!("<text:p text:class-names=\"{a} Common\">x</text:p>"));
    same(&mixed("P1"), &mixed("P5"));
}
