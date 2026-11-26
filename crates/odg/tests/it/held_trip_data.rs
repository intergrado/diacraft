//! What the model holds, precisely (the measurement of `held_trip.rs` says only that a word survives): where each construct is, with what stamp, and what is **not**
//! held (a value the cells say exactly, a layer set the rows say all of, a bare paragraph), which the writer reads to tell a construct from nothing.

use std::sync::Arc;

use diacraft_doc::{
    CellName, Document, HeldGroup, OdfListLevel, OdfParagraph, OdfStyle, RawAttrs, RawElement, RawNode, Sheet, Style, TextGroup, stamp_all,
    stamp_cells, stamp_sections, stamp_text_group,
};

use crate::build::{DRAWING, package, replace};
use crate::drawing::{Drawing, only, page};
use crate::flat::Flat;

const NS_X: &str = r#" xmlns:x="urn:example:extension""#;

fn with_x(mut d: Drawing) -> Drawing {
    d.ns.push_str(NS_X);
    d
}

/// The style sheet called `name`, and its ODF data.
fn sheet_style<'d>(doc: &'d Document, name: &str) -> (&'d Style, OdfStyle) {
    let style = doc.styles.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no style sheet {name}"));
    (style, style.ext.get().map(|d| d.odf.clone()).unwrap_or_default())
}

fn attr<'a>(attrs: &'a RawAttrs, name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
}

fn row0(sheet: &Sheet, section: &str) -> diacraft_doc::Row {
    sheet
        .section(&diacraft_doc::SectionKey::new(section, None))
        .and_then(|s| s.row(&diacraft_doc::RowKey::Ix(0)))
        .map_or_else(diacraft_doc::Row::default, |row| (**row).clone())
}

/// The package of `d` with each edit made, one after the other: `from` replaced by `to` once in the part `name`.
fn patched(d: &Drawing, edits: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut files = d.files();
    for (name, from, to) in edits {
        let text = files.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap();
        assert!(text.contains(from), "{from} is not in {name}");
        files = replace(files, name, text.replacen(from, to, 1).into_bytes());
    }
    package(DRAWING, files)
}

fn read(bytes: &[u8]) -> Document {
    diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{} ({e})", e.class())).0
}

// ------------------------------------------------------------------------------------------------------------------------------ a common style

const GRAPHIC: &str = concat!(
    r##"<style:style style:name="Cs" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H" draw:fill-hatch-solid="true" draw:fill-color="#ff0000" "##,
    r##"draw:opacity-name="Op" draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="0.1cm" draw:marker-start="M" draw:marker-start-width="0.77cm" "##,
    r##"draw:shadow="visible" draw:shadow-opacity="37%" draw:textarea-vertical-align="justify" fo:padding="0.17cm" style:writing-mode="tb-rl"/></style:style>"##
);

/// A common style of the graphic family holds the values R22 lists that its cells only approximate, **as a shape's own automatic style gives them to the shape**, each with the
/// stamp of the cells of the style sheet (and none for the values R22 writes as held on every save).
#[test]
fn a_common_graphic_style_holds_what_its_cells_approximate_each_with_the_stamp_of_its_cells() {
    let doc = Drawing::a4().common(GRAPHIC).page("").read().0;
    let (style, odf) = sheet_style(&doc, "Cs");
    let sheet = &*style.sheet;
    let held = |group: HeldGroup| odf.held.iter().filter(move |h| h.group == group).collect::<Vec<_>>();
    // The fill: a hatch, with its name and parameters, stamped by the fill cells and the gradient's rows and by the fill colour a hatch rests on.
    let fill = held(HeldGroup::Fill);
    assert_eq!(fill.len(), 2, "the hatch, and the opacity's name");
    let hatch = fill.iter().find(|h| attr(&h.attrs, "draw:fill") == Some("hatch")).expect("the hatch");
    assert_eq!((attr(&hatch.attrs, "draw:fill-hatch-name"), attr(&hatch.attrs, "draw:fill-hatch-solid")), (Some("H"), Some("true")));
    let by_fill = stamp_all(&[
        stamp_cells(sheet, &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillForegnd"]),
        stamp_sections(sheet, &["FillGradient"]),
    ]);
    assert_eq!(hatch.stamp, Some(by_fill));
    let opacity = fill.iter().find(|h| attr(&h.attrs, "draw:opacity-name").is_some()).expect("the opacity's name");
    assert_eq!(opacity.stamp, Some(stamp_cells(sheet, &["FillForegndTrans"])));
    // The dash a drawing does not define: held as written, written as held on every save, so with no stamp. Beside it, `draw:stroke="dash"`, stamped by `LinePattern`.
    let line = held(HeldGroup::Line);
    assert_eq!((line.len(), attr(&line[0].attrs, "draw:stroke-dash"), line[0].stamp), (2, Some("D"), None));
    let stroke = line.iter().find(|h| attr(&h.attrs, "draw:stroke") == Some("dash")).expect("the stroke");
    assert_eq!(stroke.stamp, Some(stamp_cells(sheet, &["LinePattern"])));
    // A marker the drawing does not define: its name and its width, stamped by that end's cells.
    let ends = held(HeldGroup::LineEnds);
    assert_eq!(ends.len(), 1);
    assert_eq!((attr(&ends[0].attrs, "draw:marker-start"), attr(&ends[0].attrs, "draw:marker-start-width")), (Some("M"), Some("0.77cm")));
    assert_eq!(ends[0].stamp, Some(stamp_cells(sheet, &["BeginArrow", "BeginArrowSize"])));
    // The shadow's attributes, the text area's justification, the padding and the writing mode.
    let shadow = held(HeldGroup::Shadow);
    assert_eq!((shadow.len(), attr(&shadow[0].attrs, "draw:shadow-opacity")), (1, Some("37%")));
    // Stamped by every cell the reader sets from the shadow's attributes.
    let cells = ["ShdwPattern", "ShapeShdwType", "ShdwForegnd", "ShdwForegndTrans", "ShapeShdwOffsetX", "ShapeShdwOffsetY"];
    assert_eq!(shadow[0].stamp, Some(stamp_cells(sheet, &cells)));
    let text = held(HeldGroup::Text);
    let names: Vec<&str> = text.iter().flat_map(|h| h.attrs.iter().map(|(k, _)| k)).collect();
    for wanted in ["draw:textarea-vertical-align", "fo:padding", "style:writing-mode"] {
        assert!(names.contains(&wanted), "{wanted} in {names:?}");
    }
    // What a cell says exactly is held by no value: the stroke width, the fill colour and the shadow's opacity are in the cells.
    let every: Vec<&str> = odf.held.iter().flat_map(|h| h.attrs.iter().map(|(k, _)| k)).collect();
    assert!(!every.contains(&"svg:stroke-width") && !every.contains(&"draw:fill-color"), "{every:?}");
    // **The stamp is good while the cells are as read, and no longer when one is changed.**
    let mut edited = sheet.clone();
    edited.cells.insert(CellName::new("FillPattern"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(7.0).unwrap()));
    assert_ne!(
        Some(stamp_all(&[
            stamp_cells(&edited, &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillForegnd"]),
            stamp_sections(&edited, &["FillGradient"]),
        ])),
        hatch.stamp
    );
}

/// A common style of the paragraph family holds the values of its paragraph and text properties the `Paragraph` and `Character` cells (row 0) only approximate, each with the
/// stamp of its group of that row, and the tab stops with their leaders.
#[test]
fn a_common_paragraph_style_holds_its_text_values_with_the_stamp_of_its_row_and_its_tab_stops() {
    let doc = Drawing::a4()
        .common(concat!(
            r##"<style:style style:name="Ps" style:family="paragraph"><style:paragraph-properties fo:text-align="start" style:line-spacing="0.45cm">"##,
            r##"<style:tab-stops><style:tab-stop style:position="3cm" style:leader-text="L"/></style:tab-stops></style:paragraph-properties>"##,
            r##"<style:text-properties style:text-underline-style="wave" style:text-underline-color="#123456" fo:text-transform="lowercase" style:text-position="30% 58%"/></style:style>"##
        ))
        .page("")
        .read()
        .0;
    let (style, odf) = sheet_style(&doc, "Ps");
    let (paragraph, run) = (row0(&style.sheet, "Paragraph"), row0(&style.sheet, "Character"));
    let groups: Vec<TextGroup> = odf.text_held.iter().map(|h| h.group).collect();
    // The last is the spelling of the group `Style`: the style underlines, so the cell `Style` says it, and says no weight, no style and no small caps, which the file did not spell
    // either; the cells would write all five.
    assert_eq!(groups, [TextGroup::HorzAlign, TextGroup::SpLine, TextGroup::Underline, TextGroup::Case, TextGroup::Pos, TextGroup::Style]);
    for value in &odf.text_held {
        let row = if value.group.is_paragraph() { &paragraph } else { &run };
        assert_eq!(value.stamp, Some(stamp_text_group(row, value.group)), "{:?}", value.group);
    }
    assert_eq!(attr(&odf.text_held[0].attrs, "fo:text-align"), Some("start"));
    assert_eq!(attr(&odf.text_held[2].attrs, "style:text-underline-color"), Some("#123456"));
    // The tab stops are held whole (the leader is no cell of the `Tabs` row), as a graphic style's are: written as held, with no stamp.
    let tabs: Vec<_> = odf.held.iter().filter(|h| h.group == HeldGroup::Text).collect();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].stamp, None);
    let element =
        tabs[0].kept.iter().find_map(|p| if let RawNode::Element(e) = &p.node { Some(e.clone()) } else { None }).expect("the tab stops' element");
    assert_eq!(&*element.name, "style:tab-stops");
}

/// A value no cell can show (the standard does not define it) is held with the attributes no cell reads, as written; a value a cell shows is not.
#[test]
fn a_value_no_cell_can_show_is_held_as_written_and_one_it_shows_is_not() {
    let doc = Drawing::a4()
        .common(concat!(
            r##"<style:style style:name="U" style:family="graphic"><style:graphic-properties svg:stroke-width="one parsec" draw:fill-color="not-a-colour" draw:stroke="wavy" "##,
            r##"draw:shadow="maybe" svg:stroke-linecap="pointy" draw:fill-hatch-solid="perhaps" fo:padding="0.2cm" svg:stroke-color="#112233"/></style:style>"##
        ))
        .page("")
        .read()
        .0;
    let (_, odf) = sheet_style(&doc, "U");
    for (name, value) in [
        ("svg:stroke-width", "one parsec"),
        ("draw:fill-color", "not-a-colour"),
        ("draw:stroke", "wavy"),
        ("draw:shadow", "maybe"),
        ("svg:stroke-linecap", "pointy"),
        ("draw:fill-hatch-solid", "perhaps"),
    ] {
        assert_eq!(attr(&odf.unmapped, &format!("style:graphic-properties/{name}")), Some(value), "{name}");
    }
    assert_eq!(attr(&odf.unmapped, "style:graphic-properties/svg:stroke-color"), None, "a colour a cell shows");
}

/// An automatic style holds what its cells only approximate where it has no shape, paragraph or run to hold it with: the text values of a graphic or a paragraph style, and a
/// tab stop that is more than a position and a type. A style whose tab stops are exact holds none.
#[test]
fn an_automatic_style_holds_its_text_values_and_its_tab_stops_in_its_remainder() {
    let doc = Drawing::a4()
        .auto(concat!(
            r##"<style:style style:name="g" style:family="graphic"><style:text-properties style:text-underline-style="wave"/></style:style>"##,
            r##"<style:style style:name="p1" style:family="paragraph"><style:text-properties style:text-line-through-style="dash"/>"##,
            r##"<style:paragraph-properties><style:tab-stops><style:tab-stop style:position="2cm" style:leader-text="L"/></style:tab-stops></style:paragraph-properties></style:style>"##,
            r##"<style:style style:name="p2" style:family="paragraph"><style:paragraph-properties><style:tab-stops><style:tab-stop style:position="2cm" style:type="right"/></style:tab-stops></style:paragraph-properties></style:style>"##
        ))
        // The shape and the paragraphs name the styles (a style nothing names is held whole as an unused one, and its test is `visit_styles.rs`'s).
        .page(concat!(
            r#"<draw:rect draw:style-name="g" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#,
            r#"<draw:frame svg:x="1cm" svg:y="5cm" svg:width="5cm" svg:height="2cm"><draw:text-box><text:p text:style-name="p1">a</text:p><text:p text:style-name="p2">b</text:p></draw:text-box></draw:frame>"#
        ))
        .read()
        .0;
    let styles = &doc.ext.get().expect("the slot").odf.styles;
    let holds = |family: &str, name: &str| styles.iter().find(|s| &*s.family == family && &*s.name == name).map(|s| s.element.clone());
    let text_of = |e: &Arc<RawElement>| format!("{e:?}");
    let g = holds("auto:graphic", "g").expect("a graphic style's text values");
    assert!(text_of(&g).contains("style:text-underline-style") && text_of(&g).contains("wave"), "{g:?}");
    let p1 = holds("auto:paragraph", "p1").expect("a paragraph style's");
    assert!(
        text_of(&p1).contains("text-line-through-style") && text_of(&p1).contains("style:tab-stops") && text_of(&p1).contains("style:leader-text"),
        "{p1:?}"
    );
    assert!(holds("auto:paragraph", "p2").is_none(), "exact tab stops, a position and a type: the cells say them");
}

// ------------------------------------------------------------------------------------------------------------------------------ the document's containers

/// What the containers hold that no field reads: their attributes are among the roots under `<part>/<element>/...`, their children in `OdfDoc.kept`, each inside the elements
/// of the containers it stands in (from the root of its part) and **at the place of the children the reader read before it**.
#[test]
fn what_the_containers_hold_that_no_field_reads_is_held_under_their_path_and_at_their_place() {
    let d = with_x(Drawing::a4().page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##).page(""));
    let bytes = patched(&d, &[("content.xml", "<office:body>", r##"<office:scripts><x:s a="S"/></office:scripts><office:body x:ba="B">"##)]);
    let doc = read(&bytes);
    let odf = &doc.ext.get().expect("the slot").odf;
    // The body's attribute, by its path; the roots' own are by the part's name.
    let roots: Vec<&str> = odf.roots.iter().map(|(k, _)| &**k).collect();
    assert!(roots.contains(&"content.xml") && roots.contains(&"content.xml/office:body"), "{roots:?}");
    let body = odf.roots.iter().find(|(k, _)| &**k == "content.xml/office:body").map(|(_, a)| a).unwrap();
    assert_eq!(attr(body, "x:ba"), Some("B"));
    // The script: inside the root's element, after the two children of the root that are read before it (the font faces and the automatic styles).
    let kept: Vec<_> = odf.kept.kept.iter().collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].after, 2);
    let RawNode::Element(root) = &kept[0].node else { panic!("an element") };
    assert_eq!(&*root.name, "office:document-content");
    assert!(root.attrs.is_empty(), "a container's element holds no attribute: they are in the roots");
    let [RawNode::Element(script)] = root.children.as_slice() else { panic!("one child: {root:?}") };
    assert_eq!(&*script.name, "office:scripts");
}

/// A child of the drawing stands inside the body's and the drawing's elements, after the pages read before it; one of the master styles inside the styles part's root and the master
/// styles'; and a master page that holds no shape is kept whole with its child.
#[test]
fn a_child_stands_inside_the_elements_of_the_containers_it_stands_in() {
    let d = with_x(Drawing::a4().page("").page(""));
    let bytes = patched(
        &d,
        &[
            ("content.xml", "</office:drawing>", r##"<x:k a="D"/></office:drawing>"##),
            ("styles.xml", "<office:master-styles>", r##"<office:master-styles><x:m a="M"/>"##),
            (
                "styles.xml",
                r##"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"##,
                r##"<style:master-page style:name="Default" style:page-layout-name="PM1"><x:p a="P"/></style:master-page>"##,
            ),
        ],
    );
    let doc = read(&bytes);
    let odf = &doc.ext.get().expect("the slot").odf;
    // The path of each    // The path of each, by the names of the elements, outermost first, and its place.
    let path = |node: &RawNode| -> Vec<String> {
        let mut out = Vec::new();
        let mut at = node;
        while let RawNode::Element(e) = at {
            out.push(match attr(&e.attrs, "style:name") {
                Some(name) => format!("{}[{name}]", e.name),
                None => e.name.to_string(),
            });
            match e.children.as_slice() {
                [only] => at = only,
                _ => break,
            }
        }
        out
    };
    let found: Vec<(u32, Vec<String>)> = odf.kept.kept.iter().map(|p| (p.after, path(&p.node))).collect();
    let drawing = found.iter().find(|(_, p)| p.last().map(String::as_str) == Some("x:k")).expect("the drawing's child");
    assert_eq!(drawing.1, ["office:document-content", "office:body", "office:drawing", "x:k"]);
    assert_eq!(drawing.0, 2, "after the two pages");
    let master_styles = found.iter().find(|(_, p)| p.last().map(String::as_str) == Some("x:m")).expect("the master styles' child");
    assert_eq!(master_styles.1, ["office:document-styles", "office:master-styles", "x:m"]);
    // A master page that holds no shape is kept whole, its child in it, and not among these: a copy writes it back with the master page, where it stood.
    assert!(!found.iter().any(|(_, p)| p.last().map(String::as_str) == Some("x:p")), "{found:?}");
    let master = odf.styles.iter().find(|s| &*s.family == "master:master-page" && &*s.name == "Default").expect("the master page, kept whole");
    assert!(matches!(master.element.children.as_slice(), [RawNode::Element(e)] if &*e.name == "x:p"), "{:?}", master.element);
}

/// A master page that holds a shape is a background page, which keeps its other children as a page does: they are not held twice.
#[test]
fn a_master_page_that_holds_a_shape_keeps_its_children_with_its_page() {
    let d = with_x(Drawing::a4().page(""));
    let bytes = patched(
        &d,
        &[(
            "styles.xml",
            r##"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"##,
            r##"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect svg:width="1cm" svg:height="1cm"/><x:p a="P"/></style:master-page>"##,
        )],
    );
    let doc = read(&bytes);
    let held = format!("{:?}", doc.ext.get().expect("the slot").odf.kept);
    assert!(!held.contains("x:p"), "not at the document: {held}");
    let background = doc.pages.iter().find(|p| p.background).expect("the background page");
    assert!(format!("{:?}", background.ext.get().map(|d| d.odf.kept.clone())).contains("x:p"), "with the page");
}

/// A flat drawing holds none of the document-level markup (its root holds both parts' children), **but its root's attributes, as the roots of the two parts they stand for** (
/// the declarations bind the prefixes of the markup the model keeps, which a package made from the model must declare again; the media type is the `mimetype` entry's, no part's).
#[test]
fn a_flat_drawing_holds_no_document_level_markup_but_its_roots_declarations() {
    let d = with_x(Drawing::a4().page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##));
    let flat = Flat::of(&d).bytes();
    let doc = read(&flat);
    let odf = doc.ext.get().map(|d| d.odf.clone()).unwrap_or_default();
    assert!(odf.kept.kept.is_empty(), "no container's markup");
    let names: Vec<&str> = odf.roots.iter().map(|(name, _)| &**name).collect();
    assert_eq!(names, ["content.xml", "styles.xml"], "the roots of the two parts the one root stands for, and no other");
    for (name, attrs) in &odf.roots {
        assert_eq!(attr(attrs, "xmlns:x"), Some("urn:example:extension"), "{name}: the declaration a prefix of the markup needs");
        assert_eq!(attr(attrs, "xmlns:draw"), Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"), "{name}");
        assert!(attrs.iter().all(|(k, _)| k != "office:mimetype"), "{name}: the media type is no part's");
    }
}

/// A page's own layer set is held beside its `Layer` rows only when the rows do not say all of it.
#[test]
fn a_pages_own_layer_set_is_held_only_when_its_rows_do_not_say_all_of_it() {
    let page_with = |layer_set: &str| -> Document {
        let mut d = with_x(Drawing::a4());
        d.pages = format!(
            r##"<draw:page draw:name="p" draw:master-page-name="Default">{layer_set}<draw:rect svg:width="1cm" svg:height="1cm"/></draw:page>"##
        );
        read(&d.bytes())
    };
    let kept = |doc: &Document| page(doc, 0).ext.get().map(|d| d.odf.kept.kept.len()).unwrap_or(0);
    // Rows that say all: a name, `draw:display` and `draw:protected` only.
    assert_eq!(
        kept(&page_with(r##"<draw:layer-set><draw:layer draw:name="Mine" draw:protected="true" draw:display="screen"/></draw:layer-set>"##)),
        0
    );
    // What they do not: another attribute, a child, an attribute of the set, a producer's layer, a name twice, a second layer set.
    for (what, set) in [
        ("an attribute of a layer", r##"<draw:layer-set><draw:layer draw:name="Mine" x:a="1"/></draw:layer-set>"##),
        ("a child of a layer", r##"<draw:layer-set><draw:layer draw:name="Mine"><x:c/></draw:layer></draw:layer-set>"##),
        ("an attribute of the layer set", r##"<draw:layer-set x:a="1"><draw:layer draw:name="Mine"/></draw:layer-set>"##),
        ("a child of the layer set", r##"<draw:layer-set><draw:layer draw:name="Mine"/><x:c/></draw:layer-set>"##),
        ("a producer's layer", r##"<draw:layer-set><draw:layer draw:name="background"/></draw:layer-set>"##),
        ("a name twice", r##"<draw:layer-set><draw:layer draw:name="Mine"/><draw:layer draw:name="Mine"/></draw:layer-set>"##),
        ("a display word that is none of the four", r##"<draw:layer-set><draw:layer draw:name="Mine" draw:display="sometimes"/></draw:layer-set>"##),
        ("a layer with no name", r##"<draw:layer-set><draw:layer/></draw:layer-set>"##),
    ] {
        assert_eq!(kept(&page_with(set)), 1, "{what}");
    }
    // A second layer set of the page is not read for rows, and is held.
    let two = r##"<draw:layer-set><draw:layer draw:name="A"/></draw:layer-set><draw:layer-set><draw:layer draw:name="B"/></draw:layer-set>"##;
    assert_eq!(kept(&page_with(two)), 1);
}

// ------------------------------------------------------------------------------------------------------------------------------ a text

fn paragraphs(doc: &Document, shape: usize) -> Vec<OdfParagraph> {
    let shape = crate::drawing::shape(doc, 0, shape);
    shape.text.as_ref().and_then(|t| t.ext.get().map(|d| d.odf.paragraphs.clone())).unwrap_or_default()
}

fn names(e: &Option<Arc<RawElement>>) -> String {
    e.as_ref().map_or_else(|| "-".to_owned(), |e| e.name.to_string())
}

fn shell(level: &OdfListLevel) -> String {
    format!("{}/{}", names(&level.list), names(&level.item))
}

/// A heading is a paragraph whose element is `text:h` with its outline level; a paragraph with an attribute of its own holds its element; a plain `text:p` and one with a style
/// name only hold none. A list is held by the paragraph that opens each list and each item at each depth, and `None` by the ones that continue it.
#[test]
fn a_paragraph_holds_its_element_and_the_lists_it_is_in_by_the_ones_it_opens() {
    let doc = Drawing::a4()
        .page(concat!(
            r##"<draw:rect svg:width="4cm" svg:height="2cm"><text:h text:outline-level="2" text:style-name="Hs">Title</text:h><text:p text:class-names="C">plain</text:p>"##,
            r##"<text:list text:style-name="Ls"><text:list-item><text:p>one</text:p><text:p>two</text:p>"##,
            r##"<text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item>"##,
            r##"<text:list-item text:start-value="9"><text:p>three</text:p></text:list-item></text:list></draw:rect>"##
        ))
        .read()
        .0;
    let ps = paragraphs(&doc, 0);
    let shown: Vec<(String, String)> = ps.iter().map(|p| (names(&p.element), p.levels.iter().map(shell).collect::<Vec<_>>().join(" "))).collect();
    let expected = [
        ("text:h", ""),
        ("text:p", ""),
        ("-", "text:list/text:list-item"),
        ("-", "-/-"),
        ("-", "-/- text:list/text:list-item"),
        ("-", "-/text:list-item"),
    ];
    assert_eq!(shown, expected.map(|(a, b)| (a.to_owned(), b.to_owned())));
    // The attributes: the heading's level (its style name is the paragraph's own), the class names, the list's style, the item's start value.
    let heading = ps[0].element.as_ref().unwrap();
    assert_eq!((attr(&heading.attrs, "text:outline-level"), attr(&heading.attrs, "text:style-name")), (Some("2"), None));
    assert_eq!(ps[0].style.name.as_deref(), Some("Hs"));
    assert_eq!(attr(&ps[1].element.as_ref().unwrap().attrs, "text:class-names"), Some("C"));
    assert_eq!(attr(&ps[2].levels[0].list.as_ref().unwrap().attrs, "text:style-name"), Some("Ls"));
    assert_eq!(attr(&ps[5].levels[0].item.as_ref().unwrap().attrs, "text:start-value"), Some("9"));
    // Held without children: the characters are the text's.
    assert!(heading.children.is_empty());
}

/// A hyperlink, a field the text has no cell for and a field it has one for are runs of the characters they show, with their elements; a span that has an attribute besides its
/// style name holds its element; a plain span holds none; the runs nest by where they lie.
#[test]
fn a_hyperlink_a_field_and_a_span_with_an_attribute_are_runs_with_their_elements() {
    let doc = Drawing::a4()
        .auto(r##"<style:style style:name="T1" style:family="text"/>"##)
        .page(concat!(
            r##"<draw:rect svg:width="4cm" svg:height="2cm"><text:p>a<text:a xlink:href="http://example.org/u"><text:span text:style-name="T1">bc</text:span></text:a>"##,
            r##"<text:span text:style-name="T1" text:class-names="K">d</text:span><text:span text:style-name="T1">e</text:span>"##,
            r##"<text:title>Ttl</text:title><text:page-number text:select-page="current">7</text:page-number><text:date text:fixed="true">2026</text:date></text:p></draw:rect>"##
        ))
        .read()
        .0;
    let p = &paragraphs(&doc, 0)[0];
    let runs: Vec<(u32, u32, String, bool)> = p.runs.iter().map(|r| (r.from, r.to, names(&r.element), r.style.name.is_some())).collect();
    assert_eq!(
        runs,
        [
            (1, 3, "text:a".to_owned(), false),
            (1, 3, "-".to_owned(), true),
            (3, 4, "text:span".to_owned(), true),
            (4, 5, "-".to_owned(), true),
            (5, 8, "text:title".to_owned(), false),
            (8, 9, "text:page-number".to_owned(), false),
            (9, 13, "text:date".to_owned(), false),
        ]
    );
    let link = p.runs[0].element.as_ref().unwrap();
    assert_eq!(attr(&link.attrs, "xlink:href"), Some("http://example.org/u"));
    assert_eq!(attr(&p.runs[2].element.as_ref().unwrap().attrs, "text:class-names"), Some("K"));
    assert_eq!(attr(&p.runs[2].element.as_ref().unwrap().attrs, "text:style-name"), None, "the style's name is the run's style");
    assert_eq!(attr(&p.runs[5].element.as_ref().unwrap().attrs, "text:select-page"), Some("current"));
}

/// A text with no characters is no text, and what its children say is held with the shape's markup: a heading, a list, a mark, a paragraph with a style or an attribute. A text
/// that has characters, and a bare `text:p`, hold nothing of the sort.
#[test]
fn what_an_empty_text_says_is_held_with_the_shape_and_a_bare_paragraph_says_nothing() {
    let rect = |inner: &str| format!(r##"<draw:rect svg:width="1cm" svg:height="1cm">{inner}</draw:rect>"##);
    let kept_of = |inner: &str| -> Vec<String> {
        let doc = Drawing::a4().page(&rect(inner)).read().0;
        let shape = only(&doc);
        assert!(shape.text.is_none(), "{inner}: no characters, no text");
        crate::drawing::odf(shape)
            .kept
            .iter()
            .map(|p| match &p.node {
                RawNode::Element(e) => e.name.to_string(),
                other => format!("{other:?}"),
            })
            .collect()
    };
    assert!(kept_of("").is_empty() && kept_of("<text:p/>").is_empty(), "a bare paragraph is nothing");
    assert_eq!(kept_of(r##"<text:p><text:bookmark text:name="B"/></text:p>"##), ["text:p"]);
    assert_eq!(kept_of(r##"<text:p text:style-name="P1"/>"##), ["text:p"]);
    assert_eq!(kept_of(r##"<text:h text:outline-level="1"/>"##), ["text:h"]);
    assert_eq!(kept_of(r##"<text:list><text:list-item><text:p/></text:list-item></text:list>"##), ["text:list"]);
    // A text with characters holds its paragraphs, and nothing of the shape's markup.
    let doc = Drawing::a4().page(&rect(r##"<text:p>x</text:p>"##)).read().0;
    assert!(crate::drawing::odf(only(&doc)).kept.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------------------ cost and bounds

fn items_of(shape: &str) -> u64 {
    Drawing::a4().page(shape).read().1.work.items
}

/// **Each list item costs what every other does** (the figures are exact, and linear): a list of `4n` items less one of `n` is three times `n` less none; the same for the
/// runs of a paragraph that holds `n` hyperlinks.
#[test]
fn the_items_of_a_list_and_of_hyperlinks_are_linear() {
    let list = |n: usize| {
        format!(
            r##"<draw:rect svg:width="1cm" svg:height="1cm"><text:list>{}</text:list></draw:rect>"##,
            r##"<text:list-item><text:p>x</text:p></text:list-item>"##.repeat(n)
        )
    };
    let (one, thousand, four_thousand) = (items_of(&list(1)), items_of(&list(1_000)), items_of(&list(4_000)));
    assert_eq!((thousand - one) % 999, 0, "the same for each item");
    assert_eq!(four_thousand - one, 3_999 * ((thousand - one) / 999), "linear: {} for each item", (thousand - one) / 999);
    let links = |n: usize| {
        format!(
            r##"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"##,
            r##"<text:a xlink:href="http://e.org/u">x</text:a>"##.repeat(n)
        )
    };
    let (one, thousand, four_thousand) = (items_of(&links(1)), items_of(&links(1_000)), items_of(&links(4_000)));
    assert_eq!((thousand - one) % 999, 0, "the same for each hyperlink");
    assert_eq!(four_thousand - one, 3_999 * ((thousand - one) / 999), "linear: {} for each hyperlink", (thousand - one) / 999);
}

/// Lists inside lists as deep as the tree allows are read and held level by level (the cap of a paragraph's levels is the tree's depth cap, which is not reached first); more
/// levels than the model's cap are refused by its decoder (`sheetxml`'s tests).
#[test]
fn lists_nested_as_deep_as_the_tree_allows_are_held_level_by_level() {
    let depth = 90usize;
    let open = r##"<text:list><text:list-item>"##.repeat(depth);
    let close = r##"</text:list-item></text:list>"##.repeat(depth);
    let doc =
        Drawing::a4().page(&format!(r##"<draw:rect svg:width="1cm" svg:height="1cm">{open}<text:p>deep</text:p>{close}</draw:rect>"##)).read().0;
    let ps = paragraphs(&doc, 0);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].levels.len(), depth);
    assert!(ps[0].levels.iter().all(|l| l.list.is_some() && l.item.is_some()));
}

// ------------------------------------------------------------------------------------------------------------------------------ the named data follows the file

/// What a model writer is given of the named data is the file's as it is now (a splice save writes markers, page layouts, styles and font faces of its own), **and the faces the
/// session added since stay**: a text formatted in a font the file does not declare adds a face to the table, an edit of the drawing no save has written yet.
#[test]
fn the_named_data_of_the_file_goes_on_the_model_and_the_faces_the_session_added_stay() {
    let old = with_x(
        Drawing::a4()
            .common(r##"<draw:marker draw:name="OldMarker" svg:viewBox="0 0 1 1" svg:d="M0 0"/>"##)
            .page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##),
    );
    let mut new = with_x(
        Drawing::a4()
            .common(r##"<draw:marker draw:name="NewMarker" svg:viewBox="0 0 1 1" svg:d="M0 0"/><text:list-style style:name="NewList"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style>"##)
            .page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##),
    );
    new.fonts = r#"<style:font-face style:name="FileFace" svg:font-family="FileFace"/>"#.to_owned();
    let mut model = read(&old.bytes());
    // A face the session added, and one the file declares too.
    let mut faces = (*model.fonts).clone();
    faces.push(diacraft_doc::FaceName { name_u: Arc::from("SessionFace"), raw: Default::default(), ext: Default::default() });
    faces.push(diacraft_doc::FaceName { name_u: Arc::from("FileFace"), raw: Default::default(), ext: Default::default() });
    model.fonts = Arc::new(faces);
    let data = diacraft_odg::read_package_data(&new.bytes(), &diacraft_odg::Limits::default()).expect("the data of the file");
    data.put(&mut model);
    let odf = &model.ext.get().expect("the slot").odf;
    let defined: Vec<&str> = odf.definitions.iter().map(|d| &*d.name).collect();
    assert_eq!(defined, ["NewMarker"], "the definitions are the file's");
    assert!(odf.styles.iter().any(|s| &*s.name == "NewList"), "the style elements too");
    let faces: Vec<&str> = model.fonts.iter().map(|f| &*f.name_u).collect();
    assert_eq!(
        faces,
        ["FileFace", "SessionFace"],
        "the file's faces first, as it declares them (the face it declares is held once), then the one the session added"
    );
    // The pages are the model's.
    assert_eq!(model.pages.len(), 1);
}
