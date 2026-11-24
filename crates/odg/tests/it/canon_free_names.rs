//! In the `.odg` content check: **the manifest's row order and its rows for entries the writer wrote; `meta.xml`'s field order and
//! generator; the names of automatic styles (a common style's name is kept); `manifest:version` with `office:version`; and an empty `office:settings` under a flat root.** Every
//! equivalence has a vector that passes and the planted differences it must not hide: a layer set added where the source had none, `meta:user-defined` fields added, `meta:creator` removed,
//! `meta:creation-date` added and `meta:print-date` removed are real differences, and so is everything that is not named. Every package is written here by hand from the ODF standard
//! (`build.rs`, `drawing.rs`), and none is a file anyone made with another program.

use diacraft_container::canon::Found;

use crate::build::{DRAWING, manifest, write};
use crate::canon::{at, diff, differs, graphic, only, paragraph, same, shape_with};
use crate::drawing::{Drawing, NS};
use crate::flat::Flat;

pub const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

// ------------------------------------------------------------------------------------------------------------------------------ the manifest

/// A package of `files` and a manifest of `rows` (full path, media type), the manifest's text passed through `edit`.
pub fn packaged(files: Vec<(String, Vec<u8>)>, rows: &[(&str, &str)], edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut entries = files;
    entries.push(("META-INF/manifest.xml".to_owned(), edit(manifest(DRAWING, rows)).into_bytes()));
    write(Some(DRAWING), &entries)
}

pub fn row(path: &str, media: &str) -> (String, String) {
    (path.to_owned(), media.to_owned())
}

/// The rows a package of `d` lists: each file, with the type its name says.
pub fn rows_of(d: &Drawing) -> Vec<(String, String)> {
    d.files().iter().map(|(n, _)| row(n, if n.ends_with(".png") { "image/png" } else { "text/xml" })).collect()
}

pub fn pairs(rows: &[(String, String)]) -> Vec<(&str, &str)> {
    rows.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect()
}

/// A drawing that holds a picture under `Pictures/`.
pub fn with_picture() -> Drawing {
    let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    d
}

/// `d` as a package that lists `rows`.
pub fn listing(d: &Drawing, rows: &[(String, String)]) -> Vec<u8> {
    packaged(d.files(), &pairs(rows), |t| t)
}

#[test]
fn the_manifests_row_order_is_no_content_and_a_row_changed_is() {
    let d = with_picture();
    let rows = rows_of(&d);
    let mut reversed = rows.clone();
    reversed.reverse();
    same(&listing(&d, &rows), &listing(&d, &reversed));
    // The controls, each after the reorder: a row dropped, and a row's media type changed.
    let mut dropped = reversed.clone();
    dropped.pop();
    only(&differs(&listing(&d, &rows), &listing(&d, &dropped)), Found::Element);
    let mut typed = reversed.clone();
    if let Some(r) = typed.iter_mut().find(|r| r.0 == "Pictures/a.png") {
        r.1 = "image/jpeg".to_owned();
    }
    only(&differs(&listing(&d, &rows), &listing(&d, &typed)), Found::Value);
}

#[test]
fn manifest_version_counts_as_nothing_as_office_version_does_and_no_other_attribute_does() {
    let d = with_picture();
    let rows = rows_of(&d);
    // On the manifest's root and on its root row: 1.2 against 1.4.
    let versioned =
        |version: &str| packaged(d.files(), &pairs(&rows), |t| t.replace("manifest:version=\"1.3\"", &format!("manifest:version=\"{version}\"")));
    same(&versioned("1.2"), &versioned("1.4"));
    // On any row: an embedded object's row says the version of the object.
    let mut with_object = d.clone();
    with_object.files.push(("Object 1/content.xml".to_owned(), b"<a/>".to_vec()));
    let object_rows = rows_of(&with_object);
    let object = |attr: &str| {
        packaged(with_object.files(), &pairs(&object_rows), |t| {
            t.replacen("manifest:full-path=\"Object 1/content.xml\"", &format!("manifest:full-path=\"Object 1/content.xml\"{attr}"), 1)
        })
    };
    same(&object(" manifest:version=\"1.2\""), &object(" manifest:version=\"1.3\""));
    same(&object(" manifest:version=\"1.2\""), &object(""));
    // The controls: the same row with another media type is a difference, and so is an attribute of the manifest's namespace that is no version.
    let retyped = packaged(with_object.files(), &pairs(&object_rows), |t| {
        t.replacen("Object 1/content.xml\" manifest:media-type=\"text/xml\"", "Object 1/content.xml\" manifest:media-type=\"text/plain\"", 1)
    });
    only(&differs(&object(" manifest:version=\"1.2\""), &retyped), Found::Value);
    only(&differs(&object(" manifest:note=\"1\""), &object(" manifest:note=\"2\"")), Found::Value);
    only(&differs(&object(" manifest:note=\"1\""), &object("")), Found::Attribute);
}

#[test]
fn a_folder_row_the_other_side_does_not_list_is_no_difference_when_files_stand_under_it() {
    let d = with_picture();
    let rows = rows_of(&d);
    let mut folder = rows.clone();
    folder.push(row("Pictures/", ""));
    // The writer lists the folder of the pictures, which the source did not: the files under it say the folder is there.
    same(&listing(&d, &rows), &listing(&d, &folder));
    // Both listing it, with the same media type, is the same, and the order is no matter.
    let mut reversed = folder.clone();
    reversed.reverse();
    same(&listing(&d, &folder), &listing(&d, &reversed));
}

#[test]
fn the_rows_that_say_more_than_the_files_do_are_still_counted() {
    let d = with_picture();
    let rows = rows_of(&d);
    let plus = |extra: (&str, &str)| {
        let mut all = rows.clone();
        all.push(row(extra.0, extra.1));
        all
    };
    // A folder with a media type that names a kind (an embedded object's folder) is a fact the other side lacks.
    let mut object = d.clone();
    object.files.push(("Object 1/content.xml".to_owned(), b"<a/>".to_vec()));
    let object_rows = rows_of(&object);
    let mut typed_folder = object_rows.clone();
    typed_folder.push(row("Object 1/", "application/vnd.oasis.opendocument.chart"));
    only(&differs(&listing(&object, &object_rows), &listing(&object, &typed_folder)), Found::Element);
    // A folder with no file under it names nothing the package holds.
    only(&differs(&listing(&d, &rows), &listing(&d, &plus(("Nothing/", "")))), Found::Element);
    // A folder row both list, but typed otherwise.
    let (empty, typed) = (plus(("Pictures/", "")), plus(("Pictures/", "image/x-folder")));
    only(&differs(&listing(&d, &empty), &listing(&d, &typed)), Found::Value);
    // A file both packages hold, listed by one: the package that does not list it is one a program offers to repair.
    let unlisted: Vec<(String, String)> = rows.iter().filter(|r| r.0 != "Pictures/a.png").cloned().collect();
    only(&differs(&listing(&d, &unlisted), &listing(&d, &rows)), Found::Element);
    // A row for a file that neither package holds.
    only(&differs(&listing(&d, &rows), &listing(&d, &plus(("Gone.xml", "text/xml")))), Found::Element);
}

#[test]
fn a_row_for_an_entry_the_writer_wrote_is_the_entrys_one_difference_and_not_a_second() {
    let d = with_picture();
    let rows = rows_of(&d);
    // An entry the writer adds that is no carrier of DiaCraft's: a thumbnail. The entry is the difference, and its row says what it says. (DiaCraft's own entries are no difference
    // at all: `canon_extra_carrier.rs`.)
    let mut thumbnail = d.clone();
    thumbnail.files.push(("Thumbnails/thumbnail.png".to_owned(), PNG.to_vec()));
    let mut with_thumbnail = rows.clone();
    with_thumbnail.push(row("Thumbnails/thumbnail.png", "image/png"));
    let found = differs(&listing(&d, &rows), &listing(&thumbnail, &with_thumbnail));
    only(&found, Found::ExtraPart);
    assert_eq!(found.shown[0].part, "Thumbnails/thumbnail.png");
    // The control: a row for a file that is no entry of either side is one that says more (so the count is the entry and the row).
    let mut stale = with_thumbnail.clone();
    stale.push(row("Thumbnails/other.png", "image/png"));
    assert_eq!(differs(&listing(&d, &rows), &listing(&thumbnail, &stale)).count, 2);
}

// ------------------------------------------------------------------------------------------------------------------------------ meta.xml

fn meta_of(inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {NS} office:version=\"1.3\"><office:meta>{inner}</office:meta></office:document-meta>"
    )
}

fn with_meta(inner: &str) -> Vec<u8> {
    let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    d.meta = Some(meta_of(inner));
    d.bytes()
}

fn without_meta() -> Vec<u8> {
    Drawing::a4().page(&shape_with("s", &at("1cm"), "x")).bytes()
}

const TITLE: &str = "<dc:title>A plan</dc:title>";
const INITIAL: &str = "<meta:initial-creator>Someone</meta:initial-creator>";
const CREATOR: &str = "<dc:creator>Another</dc:creator>";
const CREATED: &str = "<meta:creation-date>2030-01-02T03:04:05</meta:creation-date>";
const MODIFIED: &str = "<dc:date>2030-01-03T00:00:00</dc:date>";
const PRINTED: &str = "<meta:print-date>2030-01-04T00:00:00</meta:print-date>";
const KEYWORDS: &str = "<meta:keyword>one</meta:keyword><meta:keyword>two</meta:keyword>";
const USER: &str = "<meta:user-defined meta:name=\"Project\" meta:value-type=\"string\">Apollo</meta:user-defined>";

#[test]
fn the_order_of_the_fields_of_meta_xml_is_no_content() {
    let one = with_meta(&format!("{TITLE}{INITIAL}{CREATOR}{CREATED}{MODIFIED}{PRINTED}{KEYWORDS}{USER}"));
    let other = with_meta(&format!("{USER}{PRINTED}{MODIFIED}{CREATED}{CREATOR}{INITIAL}{TITLE}{KEYWORDS}"));
    same(&one, &other);
    // The copies of one field are in the order written: the keywords of a document are a list.
    let swapped = with_meta(&format!(
        "{TITLE}{INITIAL}{CREATOR}{CREATED}{MODIFIED}{PRINTED}<meta:keyword>two</meta:keyword><meta:keyword>one</meta:keyword>{USER}"
    ));
    assert!(!diff(&one, &swapped).same(), "the keywords are a list");
    // A field is told by its name: the creator is not the initial creator.
    assert!(!diff(&with_meta(INITIAL), &with_meta(&INITIAL.replace("meta:initial-creator", "dc:creator"))).same());
}

#[test]
fn a_field_added_or_taken_out_or_changed_is_one_difference() {
    let base = format!("{TITLE}{INITIAL}{CREATOR}{CREATED}{MODIFIED}{PRINTED}{KEYWORDS}");
    let one = with_meta(&base);
    // `meta:user-defined` added, `dc:creator` and `meta:initial-creator` taken out, `meta:creation-date` added where there was none, `meta:print-date` taken out.
    only(&differs(&one, &with_meta(&format!("{base}{USER}"))), Found::Element);
    only(&differs(&one, &with_meta(&base.replace(CREATOR, ""))), Found::Element);
    only(&differs(&one, &with_meta(&base.replace(INITIAL, ""))), Found::Element);
    only(&differs(&with_meta(&base.replace(CREATED, "")), &one), Found::Element);
    only(&differs(&one, &with_meta(&base.replace(PRINTED, ""))), Found::Element);
    // A value changed, whichever field.
    only(&differs(&one, &with_meta(&base.replace("A plan", "Another plan"))), Found::Text);
    only(&differs(&one, &with_meta(&base.replace("2030-01-02", "2030-01-09"))), Found::Text);
    // The order of the others hides none of them: with the fields moved, the one that is missing is the one difference.
    let moved = format!("{PRINTED}{KEYWORDS}{MODIFIED}{CREATED}{CREATOR}{TITLE}");
    only(&differs(&one, &with_meta(&moved)), Found::Element);
    only(&differs(&with_meta(&moved), &with_meta(&format!("{PRINTED}{KEYWORDS}{MODIFIED}{CREATED}{CREATOR}{INITIAL}{TITLE}"))), Found::Element);
}

#[test]
fn the_generator_counts_as_nothing() {
    let generator = |g: &str| format!("<meta:generator>{g}</meta:generator>");
    // Another generator, none, and one among the other fields, wherever it stands.
    same(&with_meta(&format!("{}{TITLE}", generator("LibreOffice/7.3"))), &with_meta(&format!("{TITLE}{}", generator("DiaCraft/0.2"))));
    same(&with_meta(&format!("{}{TITLE}", generator("LibreOffice/7.3"))), &with_meta(TITLE));
    same(&with_meta(&format!("{TITLE}{}{INITIAL}", generator("x"))), &with_meta(&format!("{INITIAL}{TITLE}")));
    // A `meta.xml` that holds only a generator holds nothing: the same as no `meta.xml` (the empty part, with the generator counting as nothing).
    same(&with_meta(&generator("DiaCraft/0.2")), &without_meta());
    same(&without_meta(), &with_meta(&generator("LibreOffice/7.3")));
    // The control: a title beside the generator is content, and so is any other field.
    for field in [TITLE, INITIAL, USER, KEYWORDS, CREATED] {
        let found = differs(&with_meta(&format!("{}{field}", generator("g"))), &without_meta());
        assert!(found.shown.iter().any(|d| d.found == Found::MissingPart && d.part == "meta.xml"), "{field}: {:?}", found.shown);
    }
    // And the same element in another place is not a generator: `meta:generator` counts as nothing as a field of `office:meta`, and nowhere else.
    let nested = |g: &str| with_meta(&format!("<office:other><meta:generator>{g}</meta:generator></office:other>"));
    assert!(!diff(&nested("a"), &nested("b")).same());
}

#[test]
fn the_generator_and_the_order_hold_in_a_flat_file_too() {
    let plain = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    let mut d = plain.clone();
    d.meta = Some(meta_of(&format!("<meta:generator>LibreOffice/7.3</meta:generator>{TITLE}{INITIAL}")));
    let mut e = plain.clone();
    e.meta = Some(meta_of(&format!("{INITIAL}{TITLE}<meta:generator>DiaCraft/0.2</meta:generator>")));
    same(&Flat::of(&d).bytes(), &e.bytes());
    same(&Flat::of(&d).bytes(), &Flat::of(&e).bytes());
    // A generator and nothing else is no `office:meta`, in a flat file as in a package.
    let mut only_generator = plain.clone();
    only_generator.meta = Some(meta_of("<meta:generator>x</meta:generator>"));
    same(&Flat::of(&only_generator).bytes(), &plain.bytes());
    same(&Flat::of(&only_generator).bytes(), &Flat::of(&plain).bytes());
    // The control: the title is content.
    assert!(!diff(&Flat::of(&d).bytes(), &plain.bytes()).same());
}

// ------------------------------------------------------------------------------------------------------------------------ automatic styles

/// A shape that names `style` as its style.
pub fn styled(name: &str, style: &str, x: &str) -> String {
    shape_with(name, &format!("draw:style-name=\"{style}\" {}", at(x)), "x")
}

/// A red and a blue graphic style named `red` and `blue`, and two shapes that use the styles `first` and `second` names.
pub fn drawing_of_two(red: &str, blue: &str, first: &str, second: &str) -> Drawing {
    Drawing::a4().auto(&format!("{}{}", graphic(red, "#ff0000"), graphic(blue, "#0000ff"))).page(&format!(
        "{}{}",
        styled("s1", first, "1cm"),
        styled("s2", second, "3cm")
    ))
}

#[test]
fn automatic_styles_with_other_names_are_the_same_and_a_shape_follows_its_match() {
    let a = drawing_of_two("gr1", "gr2", "gr1", "gr2").bytes();
    // Other names, in the other order, and the shapes pointing to their matches.
    let b = Drawing::a4()
        .auto(&format!("{}{}", graphic("gr7", "#0000ff"), graphic("gr9", "#ff0000")))
        .page(&format!("{}{}", styled("s1", "gr9", "1cm"), styled("s2", "gr7", "3cm")))
        .bytes();
    same(&a, &b);
    // The control: a shape pointing to the other style is the one difference, which is where it points.
    let wrong = Drawing::a4()
        .auto(&format!("{}{}", graphic("gr7", "#0000ff"), graphic("gr9", "#ff0000")))
        .page(&format!("{}{}", styled("s1", "gr7", "1cm"), styled("s2", "gr7", "3cm")))
        .bytes();
    let found = differs(&a, &wrong);
    only(&found, Found::Value);
    assert!(found.shown[0].path.ends_with("/@style-name"), "{:?}", found.shown);
    // The names swapped over the same two styles: the styles are the same, and each shape now points to the other one.
    assert_eq!(differs(&a, &drawing_of_two("gr1", "gr2", "gr2", "gr1").bytes()).count, 2);
}

#[test]
fn an_automatic_styles_content_is_still_compared_and_only_its_name_is_free() {
    let a = drawing_of_two("gr1", "gr2", "gr1", "gr2").bytes();
    // A property changed under the same name: one difference, in the style, as it was before names were free.
    let green = Drawing::a4()
        .auto(&format!("{}{}", graphic("gr1", "#00ff00"), graphic("gr2", "#0000ff")))
        .page(&format!("{}{}", styled("s1", "gr1", "1cm"), styled("s2", "gr2", "3cm")))
        .bytes();
    only(&differs(&a, &green), Found::Value);
    // A property changed under another name: the style has no match.
    let renamed_green = Drawing::a4()
        .auto(&format!("{}{}", graphic("gr8", "#00ff00"), graphic("gr2", "#0000ff")))
        .page(&format!("{}{}", styled("s1", "gr8", "1cm"), styled("s2", "gr2", "3cm")))
        .bytes();
    assert!(!diff(&a, &renamed_green).same());
    // A style added is one difference, and a parent changed is one too (the parent is a common style, whose name is kept).
    let more = Drawing::a4()
        .auto(&format!("{}{}{}", graphic("gr1", "#ff0000"), graphic("gr2", "#0000ff"), graphic("gr3", "#000000")))
        .page(&format!("{}{}", styled("s1", "gr1", "1cm"), styled("s2", "gr2", "3cm")))
        .bytes();
    only(&differs(&a, &more), Found::Element);
    let parent = Drawing::a4()
        .auto(
            &format!("{}{}", graphic("g1", "#ff0000"), graphic("g2", "#0000ff"))
                .replace("parent-style-name=\"Standard\"", "parent-style-name=\"Other\""),
        )
        .page(&format!("{}{}", styled("s1", "g1", "1cm"), styled("s2", "g2", "3cm")))
        .bytes();
    assert!(!diff(&a, &parent).same(), "the parent is a common style, which is named as it is");
}

#[test]
fn a_common_styles_name_is_kept_and_so_is_the_name_a_shape_gives_to_it() {
    let common = |name: &str| Drawing::a4().common(&graphic(name, "#ff0000")).page(&styled("s", name, "1cm")).bytes();
    assert!(!diff(&common("Box"), &common("Crate")).same(), "a common style renamed is another style");
    // The same content as an automatic style under two names is the same.
    let auto = |name: &str| Drawing::a4().auto(&graphic(name, "#ff0000")).page(&styled("s", name, "1cm")).bytes();
    same(&auto("gr1"), &auto("gr5"));
    // An automatic style is not a common one: the same style and the same shape, once automatic and once common, are different.
    assert!(!diff(&auto("Box"), &common("Box")).same());
}

#[test]
fn a_name_that_names_a_style_of_another_family_is_never_taken_for_the_automatic_one() {
    // `P1` is an automatic style of the paragraph family. A shape's `draw:style-name` asks for a graphic style, so the shape's `P1` is the common graphic style `P1` (the one the
    // other drawing has too), and renaming the paragraph style must not make the first shape the same as one that names `Q1`.
    let build = |auto_name: &str| {
        Drawing::a4().common(&graphic("P1", "#00ff00")).auto(&paragraph(auto_name, "1cm")).page(&styled("s", auto_name, "1cm")).bytes()
    };
    assert!(!diff(&build("P1"), &build("Q1")).same(), "a reference of the wrong family is no reference to the automatic style");
    // A name that two automatic styles of one family hold is a name the check cannot tell apart: it is compared as written.
    let twice =
        |name: &str| Drawing::a4().auto(&format!("{}{}", graphic(name, "#ff0000"), graphic(name, "#0000ff"))).page(&styled("s", name, "1cm")).bytes();
    assert!(!diff(&twice("g"), &twice("h")).same());
}

/// A drawing whose automatic styles are of every kind a shape names, under the names given: a paragraph, a text, a drawing-page and a list style.
fn kinds(p: &str, t: &str, dp: &str, l: &str) -> Vec<u8> {
    let mut d = Drawing::a4().auto(&format!(
        "{}<style:style style:name=\"{t}\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
         <style:style style:name=\"{dp}\" style:family=\"drawing-page\"><style:drawing-page-properties draw:fill=\"none\"/></style:style>\
         <text:list-style style:name=\"{l}\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"o\"/></text:list-style>",
        paragraph(p, "1cm")
    ));
    d.pages = format!(
        "<draw:page draw:name=\"page1\" draw:style-name=\"{dp}\" draw:master-page-name=\"Default\">\
         <draw:custom-shape draw:name=\"s\" {}><text:p text:style-name=\"{p}\">a<text:span text:style-name=\"{t}\">b</text:span></text:p>\
         <text:list text:style-name=\"{l}\"><text:list-item><text:p>x</text:p></text:list-item></text:list>\
         <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape></draw:page>",
        at("1cm")
    );
    d.bytes()
}

#[test]
fn paragraph_text_drawing_page_and_list_styles_are_renamed_by_their_own_references() {
    same(&kinds("P1", "T1", "dp1", "L1"), &kinds("P7", "T3", "dp9", "L5"));
    // A style named by the wrong kind of reference is not found there: a span that names a paragraph style keeps the name as written, and so differs.
    let span = |name: &str| {
        Drawing::a4()
            .auto(&paragraph(name, "1cm"))
            .page(&shape_with("s", &at("1cm"), &format!("<text:span text:style-name=\"{name}\">b</text:span>")))
            .bytes()
    };
    assert!(!diff(&span("P1"), &span("P2")).same(), "a span's style is of the text family, and a paragraph style is no such style");
}

#[test]
fn page_layouts_and_master_pages_follow_the_names_too() {
    let named = |layout: &str, drawing_page: &str| {
        let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
        d.styles_auto = format!(
            "<style:page-layout style:name=\"{layout}\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout>\
             <style:style style:name=\"{drawing_page}\" style:family=\"drawing-page\"><style:drawing-page-properties draw:fill=\"none\"/></style:style>"
        );
        d.masters = format!("<style:master-page style:name=\"Default\" style:page-layout-name=\"{layout}\" draw:style-name=\"{drawing_page}\"/>");
        d.bytes()
    };
    same(&named("PM1", "Mdp1"), &named("PM0", "Mdp4"));
    // A master page that names a layout that is not defined is another master page.
    let plain = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    let mut other = plain.clone();
    other.masters = other.masters.replace("PM1", "PM2");
    assert!(!diff(&plain.bytes(), &other.bytes()).same());
}

#[test]
fn a_flat_file_and_a_package_are_the_same_when_their_automatic_styles_differ_in_their_names() {
    let d = drawing_of_two("gr1", "gr2", "gr1", "gr2");
    let mut e = d.clone();
    e.auto = d.auto.replace("gr1", "gr7").replace("gr2", "gr8");
    e.pages = d.pages.replace("\"gr1\"", "\"gr7\"").replace("\"gr2\"", "\"gr8\"");
    same(&Flat::of(&d).bytes(), &e.bytes());
    same(&Flat::of(&e).bytes(), &d.bytes());
    same(&Flat::of(&d).bytes(), &Flat::of(&e).bytes());
    // The control, which of the two a shape points to.
    let mut wrong = e.clone();
    wrong.pages = e.pages.replace("\"gr7\"", "\"gr9\"").replace("\"gr8\"", "\"gr7\"").replace("\"gr9\"", "\"gr8\"");
    assert!(!diff(&Flat::of(&d).bytes(), &wrong.bytes()).same());
    assert!(!diff(&Flat::of(&wrong).bytes(), &d.bytes()).same());
}

// -------------------------------------------------------------------------------------------------------------------- layers and the flat settings

#[test]
fn a_layer_set_added_where_the_source_had_none_is_a_difference() {
    let build = |masters: &str| {
        let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
        d.masters = masters.to_owned();
        d.bytes()
    };
    let master = "<style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/>";
    let layers = format!("<draw:layer-set><draw:layer draw:name=\"layout\"/><draw:layer draw:name=\"background\"/></draw:layer-set>{master}");
    only(&differs(&build(master), &build(&layers)), Found::Element);
    // Even a set with no layer in it is a set, and a drawing that has layers has the same on both sides, in order.
    only(&differs(&build(master), &build(&format!("<draw:layer-set/>{master}"))), Found::Element);
    let other = format!("<draw:layer-set><draw:layer draw:name=\"background\"/><draw:layer draw:name=\"layout\"/></draw:layer-set>{master}");
    only(&differs(&build(&layers), &build(&other)), Found::Order);
}

#[test]
fn an_empty_settings_element_under_a_flat_root_is_none() {
    let d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    let mut empty = Flat::of(&d);
    empty.settings = "<office:settings/>".to_owned();
    let mut spaced = Flat::of(&d);
    spaced.settings = "<office:settings>\n  </office:settings>".to_owned();
    let none = Flat::of(&d);
    // Against a flat file that has none, against another empty form, and against the package that has no `settings.xml` or an empty one.
    same(&empty.bytes(), &none.bytes());
    same(&spaced.bytes(), &none.bytes());
    same(&empty.bytes(), &spaced.bytes());
    same(&empty.bytes(), &d.bytes());
    let mut with_empty_part = d.clone();
    with_empty_part.settings = Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {NS} office:version=\"1.2\"><office:settings/></office:document-settings>"
    ));
    same(&empty.bytes(), &with_empty_part.bytes());
    same(&none.bytes(), &with_empty_part.bytes());
    // The controls: a setting is content in a flat file, and an `office:settings` under another parent is the element it is.
    let mut set = Flat::of(&d);
    set.settings = "<office:settings><config:config-item-set config:name=\"s\"/></office:settings>".to_owned();
    only(&differs(&set.bytes(), &none.bytes()), Found::Element);
    let mut elsewhere = Flat::of(&d);
    elsewhere.body = elsewhere.body.replace("<office:drawing>", "<office:settings/><office:drawing>");
    assert!(!diff(&elsewhere.bytes(), &none.bytes()).same(), "an office:settings inside the body is not the one the rule lets go");
}

/// A drawing as the writer says it: 1.4 on every part and row, a generator of its own, the fields in its order, an empty `settings.xml`, a row for the folder of the pictures, the automatic styles
/// under names of its own, and `colour` for the shape's fill.
fn written(colour: &str) -> Vec<u8> {
    let mut d = Drawing::a4()
        .auto(&format!("{}{}", paragraph("P3", "1cm"), graphic("gr9", colour)))
        .page(&shape_with("s", &format!("draw:style-name=\"gr9\" {}", at("1cm")), "x").replace("<text:p>", "<text:p text:style-name=\"P3\">"));
    d.version = "1.4".to_owned();
    d.meta = Some(meta_of(&format!("{CREATED}{INITIAL}{TITLE}<meta:generator>DiaCraft/0.2</meta:generator>")));
    d.settings = Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {NS} office:version=\"1.4\"><office:settings/></office:document-settings>"
    ));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let mut rows = rows_of(&d);
    rows.reverse();
    rows.push(row("Pictures/", ""));
    packaged(d.files(), &pairs(&rows), |t| t.replace("manifest:version=\"1.3\"", "manifest:version=\"1.4\""))
}

#[test]
fn a_drawing_that_went_through_the_writer_is_the_same_content() {
    // What a trip changes and the ruling counts as nothing, all at once: the versions (1.2 against 1.4, on every part and in the manifest), the generator, the order of the fields, an empty
    // settings part, the folders' rows and the names of the automatic styles. What it does not change is the drawing.
    let mut d = Drawing::a4()
        .auto(&format!("{}{}", graphic("gr1", "#ff0000"), paragraph("P1", "1cm")))
        .page(&shape_with("s", &format!("draw:style-name=\"gr1\" {}", at("1cm")), "x").replace("<text:p>", "<text:p text:style-name=\"P1\">"));
    d.version = "1.2".to_owned();
    d.meta = Some(meta_of(&format!("<meta:generator>LibreOffice/7.3</meta:generator>{TITLE}{INITIAL}{CREATED}")));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let rows = rows_of(&d);
    let source = packaged(d.files(), &pairs(&rows), |t| t.replace("manifest:version=\"1.3\"", "manifest:version=\"1.2\""));
    same(&source, &written("#ff0000"));
    // The control after all of that: one salted value is the one difference.
    let found = differs(&source, &written("#ff0001"));
    assert!(!found.same(), "{:?}", found.shown);
}
