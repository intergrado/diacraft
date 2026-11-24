//! The after-save check's remaining misses and false alarms, from the gate's vectors. Each test is the plain visit a person can make (`file.open`, then
//! `file.save_as` to `.diacraft`, then from it to the file's own format, one leg a session), on files built here from the structure the vector's note gives: the OASIS ODF standard for the
//! `.odg` files, the testkit's VSDX builder for the VSDX files.

use diacraft_doc::{Document, Row, Section, Shape};
use diacraft_engine::check::{self, Class};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};

use crate::native_common::{Dir, opened};
use crate::odg_build::Drawing;
use crate::save_check_misses::{ENDS, LINKS, all, drawing, graphic_styles, leg, named, parse, png, resolved};

/// The plain visit of `source` (a file of the extension `ext`): what each leg named, and the directory that holds `a.<ext>`, `b.diacraft` and `c.<ext>`.
fn visit(source: &[u8], ext: &str) -> (Vec<String>, Vec<String>, Dir) {
    let dir = Dir::new();
    dir.write(&format!("a.{ext}"), source);
    let first = leg(&dir, &format!("a.{ext}"), "b.diacraft");
    let second = leg(&dir, "b.diacraft", &format!("c.{ext}"));
    (named(&first), named(&second), dir)
}

// ---------------------------------------------------------------------------------------------------------------- vector 1: a group's members

/// LibreOffice's `standard`: the fill, the line's colour and a shadow, which the shapes' automatic style does not say.
const STANDARD: &str = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffd966" svg:stroke-color="#3465a4" draw:shadow="visible" draw:marker-start-width="0.2cm" draw:marker-end-width="0.2cm"/></style:style>"##;

/// The attributes of the style chain the vector's shapes take from `standard`.
const FROM_STANDARD: [&str; 5] = ["draw:fill", "draw:fill-color", "svg:stroke-color", "draw:shadow", "svg:stroke-width"];

/// The vector's drawing: one `draw:g` with no attribute of its own holding two rectangles, and a rectangle beside it, all three on the automatic style `gs1` over `standard`, which sets a stroke
/// width alone.
fn grouped_on_standard() -> Vec<u8> {
    let mut d = Drawing::new().common(STANDARD);
    d.auto.push_str(r#"<style:style style:name="gs1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties svg:stroke-width="0.1cm"/></style:style>"#);
    let rect = |name: &str, x: u32, y: u32| {
        format!(r#"<draw:rect draw:style-name="gs1" draw:name="{name}" svg:x="{x}cm" svg:y="{y}cm" svg:width="3cm" svg:height="2cm"/>"#)
    };
    d.page(&format!("<draw:g>{}{}</draw:g>{}", rect("M1", 2, 2), rect("M2", 6, 2), rect("Top", 2, 8))).bytes()
}

/// **Vector 1 (`odg-group-member-parent`): the members of a group keep their style's parent on the plain visit.** The `.odg` model writer took a group that holds no `.odg` data of its own (a
/// `draw:g` with no attribute) for a new one, and flattened its members with it: their styles lost `standard` as their parent, and with it the fill colour, the line colour and the shadow a program
/// reads (the colours rode only in DiaCraft's own data), while the rectangle beside the group kept it. Red on `f79c5a0b`: the members' styles have no parent.
#[test]
fn the_members_of_a_group_keep_their_style_s_parent_on_the_plain_visit() {
    let source = grouped_on_standard();
    let (first, second, dir) = visit(&source, "odg");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    let written = dir.read("c.odg");
    // The three rectangles (the page has a name too, and no style).
    // (The group, which had no name, is written with the name `Group_1` its entry in DiaCraft's own data is found by.)
    let shapes = |bytes: &[u8]| -> Vec<crate::save_check_misses::Said> {
        resolved(bytes, &FROM_STANDARD).into_iter().filter(|(name, _)| ["M1", "M2", "Top"].contains(&name.as_str())).collect()
    };
    let was = shapes(&source);
    assert_eq!(was.len(), 3, "three named shapes: {was:?}");
    assert!(was.iter().all(|(_, said)| said.iter().all(|(_, v)| v.is_some())), "the source says each: {was:?}");
    assert_eq!(shapes(&written), was, "every shape's fill, line and shadow, by its style chain");
    // Each shape's own style names `standard` as its parent, a member's as the rectangle's beside the group.
    let styles = graphic_styles(&written);
    let root = parse(&crate::odg_build::part_of(&written, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    let parents: Vec<(String, Option<String>)> = list
        .iter()
        .filter(|el| el.name == "draw:rect")
        .map(|el| {
            let style = el.attr("draw:style-name").unwrap_or_default();
            (el.attr("draw:name").unwrap_or_default().to_owned(), styles.get(style).and_then(|(parent, _)| parent.clone()))
        })
        .collect();
    let standard = Some("standard".to_owned());
    assert_eq!(parents, vec![("M1".to_owned(), standard.clone()), ("M2".to_owned(), standard.clone()), ("Top".to_owned(), standard)]);
}

// ---------------------------------------------------------------------------------------------------------------- vector 2: a line end's width

/// **Vector 2 (`odg-line-end-width`): a line end's width beside an empty marker name comes back from the plain visit.** An automatic style that names no marker at an end
/// (`draw:marker-start=""`) and gives that end a width and a centre flag lost both: the reader held the width of an end only when it named a marker, so the written style said
/// neither and the end read `standard`'s 0.2 cm. A line with a marker and a width at both ends is the control. Red on `f79c5a0b`: the widths of the ends with no marker are the parent's.
#[test]
fn a_line_end_s_width_beside_an_empty_marker_name_comes_back_from_the_plain_visit() {
    let arrow = r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0L0 30H20Z"/>"#;
    let mut d = Drawing::new().common(STANDARD).common(arrow);
    d.auto.push_str(concat!(
        r#"<style:style style:name="ge1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-start="" draw:marker-start-width="0.29cm" draw:marker-end="Arrow" draw:marker-end-width="0.29cm" svg:stroke-width="0.05cm"/></style:style>"#,
        r#"<style:style style:name="ge2" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-start="Arrow" draw:marker-start-width="0.29cm" draw:marker-end="Arrow" draw:marker-end-width="0.29cm" svg:stroke-width="0.05cm"/></style:style>"#,
        r#"<style:style style:name="ge3" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-start="" draw:marker-start-width="0.29cm" draw:marker-start-center="false" draw:marker-end="" draw:marker-end-width="0.29cm" draw:marker-end-center="false" svg:stroke-width="0.035cm"/></style:style>"#,
    ));
    let shapes = concat!(
        r#"<draw:line draw:style-name="ge1" draw:name="L1" svg:x1="2cm" svg:y1="3cm" svg:x2="8cm" svg:y2="3cm"/>"#,
        r#"<draw:line draw:style-name="ge2" draw:name="L2" svg:x1="2cm" svg:y1="6cm" svg:x2="8cm" svg:y2="7cm"/>"#,
        r#"<draw:custom-shape draw:style-name="ge3" draw:name="C3" svg:x="2cm" svg:y="9cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#,
    );
    let source = d.page(shapes).bytes();
    let (first, second, dir) = visit(&source, "odg");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    let was = resolved(&source, &ENDS);
    let width = |said: &[crate::save_check_misses::Said], shape: &str, attr: &str| -> Option<String> {
        said.iter().find(|(name, _)| name == shape).and_then(|(_, list)| list.iter().find(|(a, _)| a == attr).and_then(|(_, v)| v.clone()))
    };
    assert_eq!(width(&was, "L1", "draw:marker-start-width").as_deref(), Some("0.29cm"), "the source's own width");
    assert_eq!(resolved(&dir.read("c.odg"), &ENDS), was, "every end's marker, width and centre, by its style chain");
}

// ---------------------------------------------------------------------------------------------------------------- vector 3: a picture's words

/// The vector's picture: a frame whose `draw:image` holds the words of a paragraph (LibreOffice draws them on the picture and keeps them).
fn picture_with_words(words: &str) -> Vec<u8> {
    let frame = format!(
        r#"<draw:frame draw:style-name="gr1" draw:name="Pic" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/p1.png" {LINKS}><text:p>{words}</text:p></draw:image></draw:frame>"#
    );
    drawing("", &frame, &[("Pictures/p1.png", "image/png", png())])
}

/// The words each `draw:image` of a package holds, image by image.
fn image_words(bytes: &[u8]) -> Vec<String> {
    let root = parse(&crate::odg_build::part_of(bytes, "content.xml"));
    let mut list = Vec::new();
    all(&root, &mut list);
    list.iter()
        .filter(|el| el.name == "draw:image")
        .map(|image| {
            let mut inner = Vec::new();
            all(image, &mut inner);
            inner.iter().filter(|el| el.name == "text:p").map(|p| p.text()).collect::<Vec<_>>().join("\n")
        })
        .collect()
}

/// The shape called `name` on the first page.
fn shape_named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    doc.pages.get(0).and_then(|page| page.shapes.iter().find(|s| s.name_u.as_deref() == Some(name))).map(|s| &**s).expect("the shape")
}

/// **Vector 3 (`odg-text-fallback-and-edges`): the words of a picture come back from the plain visit.** The reader read nothing of a picture frame's `draw:image` but its picture, so the paragraph it
/// held (the words LibreOffice draws on the picture) was in no drawing and in no file written from one, and the check had nothing to compare. The reader keeps them whole on the picture (they are
/// not the shape's text), the `.diacraft` file holds them as it holds every kept element, and the `.odg` model writer writes them back inside the `draw:image` they were in; the check
/// counts them as text when they do not come back. Red on `f79c5a0b`: the written `draw:image` holds no words.
#[test]
fn the_words_of_a_picture_frame_come_back_from_the_plain_visit() {
    let source = picture_with_words("alt words");
    let (first, second, dir) = visit(&source, "odg");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    assert_eq!(image_words(&dir.read("c.odg")), vec!["alt words".to_owned()], "the words are where they were");
    let reread = opened(&dir.path("c.odg"));
    let pic = shape_named(reread.document().unwrap(), "Pic");
    assert!(pic.text.is_none(), "the words are the picture's, not the shape's text");
    // A loss of them is named as text.
    let lost = |doc: &mut Document| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let foreign = std::sync::Arc::make_mut(shape.foreign.as_mut().unwrap());
        foreign.ext.make_mut().odf.kept = diacraft_doc::Passthrough::new();
    };
    assert_eq!(named_with(&dir, "b.diacraft", "d.odg", lost), vec![Class::Text.id().to_owned()]);
    // A picture whose image holds only an empty paragraph (as LibreOffice writes every one) keeps nothing, as before.
    let bare = drawing(
        "",
        &format!(
            r#"<draw:frame draw:style-name="gr1" draw:name="Pic" svg:x="2cm" svg:y="2cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/p1.png" {LINKS}><text:p/></draw:image></draw:frame>"#
        ),
        &[("Pictures/p1.png", "image/png", png())],
    );
    let dir = Dir::new();
    let session = opened(&dir.write("a.odg", &bare));
    let foreign = shape_named(session.document().unwrap(), "Pic").foreign.as_deref().unwrap();
    assert!(foreign.ext.get().is_none_or(|data| data.odf.kept.is_empty()), "{foreign:?}");
}

/// The same picture through VSDX and back: a VSDX file has no place for the words of a picture, and DiaCraft's carrier in it holds them (the shape's data), so that they come back to an `.odg`.
#[test]
fn the_words_of_a_picture_frame_cross_to_vsdx_and_back() {
    let dir = Dir::new();
    dir.write("a.odg", &picture_with_words("alt words"));
    let first = leg(&dir, "a.odg", "b.vsdx");
    let second = leg(&dir, "b.vsdx", "c.odg");
    assert_eq!(image_words(&dir.read("c.odg")), vec!["alt words".to_owned()]);
    // The two legs name what they name for the same picture with no words (the control): nothing the words add.
    let control = Dir::new();
    control.write("a.odg", &picture_with_words(""));
    let (first_control, second_control) = (leg(&control, "a.odg", "b.vsdx"), leg(&control, "b.vsdx", "c.odg"));
    assert_eq!((named(&first), named(&second)), (named(&first_control), named(&second_control)));
    assert!(!named(&first).contains(&Class::Text.id().to_owned()) && !named(&second).contains(&Class::Text.id().to_owned()));
}

// ---------------------------------------------------------------------------------------------------------------- vectors 4 and 5: VSDX

/// A VSDX drawing of one rectangle whose sheet holds `sections` beside its frame, its outline and its text.
fn vsdx_with(sections: &str) -> Vec<u8> {
    let shape = format!(
        concat!(
            r#"<Shape ID="1" Type="Shape" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="PinX" V="2" U="IN"/><Cell N="PinY" V="1.5" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
            r#"<Cell N="LocPinX" V="1" U="IN" F="Width*0.5"/><Cell N="LocPinY" V="0.5" U="IN" F="Height*0.5"/>{}<Text>alpha</Text></Shape>"#
        ),
        sections
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shape)).build()
}

/// A rectangle's outline, as a `Geometry` section.
const OUTLINE: &str = concat!(
    r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>"#,
    r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
);

/// The first shape of the first page.
fn first_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| &**s).expect("a shape")
}

/// The sections of a sheet called `name`, in order.
fn sections_named<'a>(shape: &'a Shape, name: &str) -> Vec<&'a Section> {
    shape.sheet.sections.iter().filter(|s| s.name.as_str() == name).map(|s| &**s).collect()
}

/// A row as the test tells it: its `IX`, whether it is deleted, and its cells by name and value.
type RowSaid = (Option<u32>, bool, Vec<(String, String)>);

fn row_said(row: &Row) -> RowSaid {
    (row.ix, row.del, row.cells.iter().map(|(n, c)| (n.to_string(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())).collect())
}

/// The rows of the sections called `name` of the first shape of a file opened, section by section.
fn rows_of(path: &str, name: &str) -> Vec<Vec<RowSaid>> {
    let session = opened(path);
    let doc = session.document().unwrap();
    sections_named(first_shape(doc), name).iter().map(|s| s.rows.iter().map(|r| row_said(r)).collect()).collect()
}

/// The drawing read back, with the value of the cell `cell` of the row at `at` of the `nth` section called `section` of the first shape set to `value`: a planted loss.
fn plant(doc: &mut Document, section: &str, nth: usize, at: usize, cell: &str, value: &str) {
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = std::sync::Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
    let place = sheet.sections.iter().enumerate().filter(|(_, s)| s.name.as_str() == section).nth(nth).map(|(i, _)| i).unwrap();
    let section = std::sync::Arc::make_mut(sheet.sections.get_mut(place).unwrap());
    let row = std::sync::Arc::make_mut(section.rows.get_mut(at).unwrap());
    row.cells.get_mut(cell).unwrap().value = Some(diacraft_doc::Value::from_text(value));
}

/// The classes a save of the drawing at `from` (a `.diacraft` file) as `to` names, with `tamper` planted in the file read back.
fn named_with(dir: &Dir, from: &str, to: &str, tamper: impl FnMut(&mut Document) + 'static) -> Vec<String> {
    let said = check::with_tamper(tamper, || leg(dir, from, to));
    named(&said)
}

/// **Vector 4 (`vsdx-dup-row-ix`): two `Paragraph` rows of one `IX` written back as they were name nothing.** The check paired each row of the drawing with the first row of the file of its key, so
/// the second of two rows of `IX` 0 was compared with the first, and their `HorzAlign` differed: `text-formatting` was named on both legs of a file written back byte for byte. The rows of one key
/// are paired in order now, as the shapes of one `ID` are. The reader keeps both rows, in order, and so does every writer of the visit. Red on `f79c5a0b`: both legs name `text-formatting`.
#[test]
fn two_paragraph_rows_of_one_ix_written_back_as_they_were_name_nothing() {
    let source = vsdx_with(&format!(
        r#"{OUTLINE}<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row><Row IX="0"><Cell N="HorzAlign" V="0"/></Row></Section>"#
    ));
    let (first, second, dir) = visit(&source, "vsdx");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    let both = vec![vec![
        (Some(0), false, vec![("HorzAlign".to_owned(), "1".to_owned())]),
        (Some(0), false, vec![("HorzAlign".to_owned(), "0".to_owned())]),
    ]];
    assert_eq!(rows_of(&dir.path("a.vsdx"), "Paragraph"), both, "the reader keeps both rows, in order");
    assert_eq!(rows_of(&dir.path("c.vsdx"), "Paragraph"), both, "and the visit writes both back");
    // A loss in the second row is still named.
    let planted = named_with(&dir, "b.diacraft", "d.vsdx", |doc| plant(doc, "Paragraph", 0, 1, "HorzAlign", "2"));
    assert_eq!(planted, vec![Class::TextFormatting.id().to_owned()]);
}

/// **Vector 4 (`vsdx-dup-section`): two `Paragraph` sections written back as they were name nothing.** The check paired each section with the first of the file's of its name, so the second was
/// compared with the first. Red on `f79c5a0b`: both legs name `text-formatting`.
#[test]
fn two_paragraph_sections_written_back_as_they_were_name_nothing() {
    let source = vsdx_with(&format!(
        r#"{OUTLINE}<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/></Row></Section><Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="0"/></Row></Section>"#
    ));
    let (first, second, dir) = visit(&source, "vsdx");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    let both = vec![
        vec![(Some(0), false, vec![("HorzAlign".to_owned(), "1".to_owned())])],
        vec![(Some(0), false, vec![("HorzAlign".to_owned(), "0".to_owned())])],
    ];
    assert_eq!(rows_of(&dir.path("a.vsdx"), "Paragraph"), both, "the reader keeps both sections, in order");
    assert_eq!(rows_of(&dir.path("c.vsdx"), "Paragraph"), both);
    let planted = named_with(&dir, "b.diacraft", "d.vsdx", |doc| plant(doc, "Paragraph", 1, 0, "HorzAlign", "2"));
    assert_eq!(planted, vec![Class::TextFormatting.id().to_owned()]);
}

/// **Vector 5 (`vsdx-geometry-del-noix`): rows with `Del` and rows with no `IX` name nothing.** A row with no `IX` reads as having the number of rows before it (ruling
///), so the first such row after a `MoveTo` and two deleted `LineTo`s (`IX` 1, 2 and 3) is row 3 in the drawing, and a writer that writes `IX="3"` on it writes the same outline: the model read
/// back is the very one. The check paired the drawing's row 3 with the file's first row of that key, the deleted one, whose `Y` is none. The rows are paired in order, and a deleted row with a deleted
/// one. Red on `f79c5a0b`: both legs name `geometry`.
#[test]
fn deleted_geometry_rows_and_rows_with_no_ix_name_nothing() {
    let geometry = concat!(
        r#"<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0.5"/></Row>"#,
        r#"<Row T="LineTo" IX="2" Del="1"><Cell N="X" V="0.5"/><Cell N="Y" V="0.75"/></Row><Row T="LineTo" IX="3" Del="1"/>"#,
        r#"<Row T="LineTo"><Cell N="X" V="0"/><Cell N="Y" V="0.25"/></Row><Row T="LineTo"><Cell N="X" V="0.5"/><Cell N="Y" V="0.25"/></Row><Row T="LineTo"><Cell N="X" V="0.5"/><Cell N="Y" V="0.75"/></Row></Section>"#
    );
    let source = vsdx_with(geometry);
    let (first, second, dir) = visit(&source, "vsdx");
    assert_eq!((first, second), (Vec::<String>::new(), Vec::<String>::new()));
    let keys = |path: &str| -> Vec<(Option<u32>, bool)> { rows_of(path, "Geometry").concat().into_iter().map(|(ix, del, _)| (ix, del)).collect() };
    let wanted = vec![(Some(1), false), (Some(2), true), (Some(3), true), (Some(3), false), (Some(4), false), (Some(5), false)];
    assert_eq!(keys(&dir.path("a.vsdx")), wanted, "the reader numbers a row with no IX by the rows before it");
    assert_eq!(keys(&dir.path("c.vsdx")), wanted, "the numbers written are the ones read");
    assert_eq!(rows_of(&dir.path("a.vsdx"), "Geometry"), rows_of(&dir.path("c.vsdx"), "Geometry"), "every cell of the outline");
    // A loss in the row of `IX` 3 that is not deleted is still named.
    let planted = named_with(&dir, "b.diacraft", "d.vsdx", |doc| plant(doc, "Geometry", 0, 3, "Y", "0.5"));
    assert_eq!(planted, vec![Class::Geometry.id().to_owned()]);
}
