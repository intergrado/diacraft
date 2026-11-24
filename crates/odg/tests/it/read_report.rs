//! The read report's own promises: warnings counted by class and named up to a cap, the warnings given once per document, the small parts
//! left unread past their cap, and the edges of a view box.

use diacraft_odg::{Limits, read};

use crate::drawing::{Drawing, close, num, odf_page, only, page, row_cell, row_type, rows, settings};

fn rects_naming(styles: &[&str]) -> String {
    styles.iter().map(|s| format!(r#"<draw:rect draw:style-name="{s}" svg:width="1cm" svg:height="1cm"/>"#)).collect()
}

#[test]
fn warnings_are_counted_by_class_and_named_up_to_the_cap() {
    let d = Drawing::a4().page(&rects_naming(&["a", "b", "c", "d", "e", "a"]));
    let (_, report) = d.read();
    assert_eq!(report.warned("style_not_found"), 5, "one per name");
    assert_eq!(report.warnings.len(), 5);
    assert_eq!(report.warned("shear_left_out"), 0);
    let limits = Limits { max_report_entries: 2, ..Limits::default() };
    let (_, report) = read(&d.bytes(), &limits).unwrap();
    assert_eq!(report.warned("style_not_found"), 5, "counted past the cap");
    assert_eq!(report.warnings.len(), 2, "named up to it");
}

#[test]
fn a_length_in_px_warns_once_per_document() {
    let (doc, report) = Drawing::a4()
        .page(r#"<draw:rect svg:x="96px" svg:y="0px" svg:width="96px" svg:height="1cm"/><draw:rect svg:width="2px" svg:height="1cm"/>"#)
        .read();
    assert_eq!(report.warned("length_in_px"), 1);
    close(num(crate::drawing::cell(crate::drawing::shape(&doc, 0, 0), "Width")), 1.0);
    let (_, report) = Drawing::a4().page(r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#).read();
    assert_eq!(report.warned("length_in_px"), 0);
}

#[test]
fn a_small_part_past_its_cap_is_left_unread_and_one_at_it_is_read() {
    let items = r#"<config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">50</config:config-item>"#;
    let mut d = Drawing::a4().page("");
    d.settings = Some(settings(items));
    let size = settings(items).len() as u64;
    let scale = |limits: &Limits| {
        let (doc, report) = read(&d.bytes(), limits).unwrap();
        (num(page(&doc, 0).sheet.cell("DrawingScale")), report.warned("part_left_unread"))
    };
    let (at_cap, warned) = scale(&Limits { max_small_part_bytes: size, ..Limits::default() });
    assert!((at_cap * 2.54 - 50.0).abs() < 1e-9 && warned == 0, "at the cap: read");
    let (past, warned) = scale(&Limits { max_small_part_bytes: size - 1, ..Limits::default() });
    assert!((past * 2.54 - 1.0).abs() < 1e-9 && warned == 1, "past it: left unread, and 1 to 1");
}

#[test]
fn odg_shapes_a_view_box_s_origin_is_taken_off() {
    let (doc, _) = Drawing::a4()
        .page(r#"<draw:polygon svg:width="2cm" svg:height="2cm" svg:viewBox="100 200 1000 1000" draw:points="100,200 1100,200 600,1200"/>"#)
        .read();
    let s = only(&doc);
    close(num(row_cell(s, "Geometry", Some(0), 1, "X")), 0.0);
    close(num(row_cell(s, "Geometry", Some(0), 1, "Y")), 1.0);
    close(num(row_cell(s, "Geometry", Some(0), 3, "X")), 0.5);
    close(num(row_cell(s, "Geometry", Some(0), 3, "Y")), 0.0);
}

#[test]
fn odg_reader_defaults_a_view_box_empty_in_either_direction_draws_the_frame() {
    for vb in ["0 0 0 10", "0 0 10 0", "0 0 10 -5"] {
        let shape = format!(r#"<draw:polyline svg:width="2cm" svg:height="2cm" svg:viewBox="{vb}" draw:points="0,0 10,10"/>"#);
        let (doc, report) = Drawing::a4().page(&shape).read();
        assert_eq!(report.warned("view_box_empty"), 1, "{vb}");
        let s = only(&doc);
        assert_eq!(rows(s, 0), 5, "The rectangle: {vb}");
        assert_eq!(row_type(s, 0, 1).as_deref(), Some("RelMoveTo"));
    }
    let (_, report) =
        Drawing::a4().page(r#"<draw:polyline svg:width="2cm" svg:height="2cm" svg:viewBox="0 0 10 10" draw:points="0,0 10,10"/>"#).read();
    assert_eq!(report.warned("view_box_empty"), 0);
}

#[test]
fn odg_shapes_pages_settings_and_meta_edges() {
    use crate::drawing::{NS, meta};
    let class = |d: &Drawing| read(&d.bytes(), &Limits::default()).err().map(|e| e.class());
    // A scale item alone, or one of 0, is unreadable: 1 to 1, with a warning.
    for items in [
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">2</config:config-item>"#,
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">0</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">5</config:config-item>"#,
        r#"<config:config-item config:name="ScaleNumerator" config:type="int">5</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">0</config:config-item>"#,
        r#"<config:config-item config:name="ScaleDenominator" config:type="int">2</config:config-item>"#,
    ] {
        let mut d = Drawing::a4().page("");
        d.settings = Some(settings(items));
        let (doc, report) = d.read();
        assert_eq!(report.warned("scale_unreadable"), 1);
        close(num(page(&doc, 0).sheet.cell("PageScale")), 1.0 / 2.54);
    }
    // Items outside `ooo:configuration-settings` are not the scale.
    let mut d = Drawing::a4().page("");
    d.settings = Some(format!(
        "<?xml version=\"1.0\"?>\n<office:document-settings {NS}><office:settings><config:config-item-set config:name=\"ooo:view-settings\"><config:config-item config:name=\"ScaleNumerator\">3</config:config-item><config:config-item config:name=\"ScaleDenominator\">7</config:config-item></config:config-item-set></office:settings></office:document-settings>"
    ));
    let (doc, _) = d.read();
    close(num(page(&doc, 0).sheet.cell("DrawingScale")), 1.0 / 2.54);
    // The roots of meta.xml and settings.xml are their fixed ones.
    let mut d = Drawing::a4().page("");
    d.meta = Some(format!("<office:document-settings {NS}/>"));
    assert_eq!(class(&d), Some("xml_malformed"));
    let mut d = Drawing::a4().page("");
    d.settings = Some(format!("<office:document-meta {NS}/>"));
    assert_eq!(class(&d), Some("xml_malformed"));
    let mut d = Drawing::a4().page("");
    d.meta = Some(meta("First/1").replace("</office:meta>", "<meta:generator>Second/2</meta:generator></office:meta>"));
    assert_eq!(d.read().1.generator.as_deref(), Some("First/1"), "the first generator");
    // Only the first `office:meta` of the root says it: a generator inside another element before it, or in a second `office:meta`, says nothing.
    let mut d = Drawing::a4().page("");
    d.meta =
        Some(meta("First/1").replace("<office:meta>", r#"<x:wrap xmlns:x="urn:x"><meta:generator>Wrapped/9</meta:generator></x:wrap><office:meta>"#));
    assert_eq!(d.read().1.generator.as_deref(), Some("First/1"), "a generator inside a foreign element is no one's");
    let mut d = Drawing::a4().page("");
    d.meta = Some(
        meta("First/1")
            .replace("</office:document-meta>", "<office:meta><meta:generator>Second/2</meta:generator></office:meta></office:document-meta>"),
    );
    assert_eq!(d.read().1.generator.as_deref(), Some("First/1"), "a second office:meta is not looked in");
    // A page naming a master page the drawing lacks, or a master page with no layout: DiaCraft's new page's size, with warnings.
    let mut d = Drawing::a4();
    d.pages = r#"<draw:page draw:master-page-name="Nowhere"/>"#.to_owned();
    let (doc, report) = d.read();
    assert_eq!(report.warned("master_page_not_found"), 1);
    assert_eq!(report.warned("page_size_missing"), 1);
    close(num(page(&doc, 0).sheet.cell("PageWidth")), 11.0);
    close(num(page(&doc, 0).sheet.cell("PageHeight")), 8.5);
    assert_eq!(page(&doc, 0).name.as_ref(), "Page-1", "no draw:name");
    // A page's own layer set gives its rows, and the page's attributes are kept but its name.
    let mut d = Drawing::a4();
    d.pages = r#"<draw:page draw:name="p" draw:style-name="dp1" draw:master-page-name="Default"><draw:layer-set><draw:layer draw:name="Mine" draw:protected="true" draw:display="screen"/></draw:layer-set><draw:rect draw:layer="Mine" svg:width="1cm" svg:height="1cm"/></draw:page>"#.to_owned();
    let (doc, report) = d.read();
    let p = page(&doc, 0);
    // The layer set is read, so it is not also kept as an unknown page child.
    assert!(odf_page(p).kept.is_empty());
    assert_eq!(report.kept("page_child"), 0);
    let at = |name: &str| crate::drawing::cell_at(&p.sheet, &diacraft_doc::CellAddr::in_row("Layer", None, diacraft_doc::RowKey::Ix(0), name));
    assert_eq!(crate::drawing::text(at("Name")), "Mine");
    assert_eq!(crate::drawing::text(at("Visible")), "1");
    assert_eq!(crate::drawing::text(at("Print")), "0");
    assert_eq!(crate::drawing::text(at("Lock")), "1");
    assert_eq!(odf_page(p).raw.get("draw:style-name"), Some("dp1"));
    assert_eq!(odf_page(p).raw.get("draw:name"), None);
    // Margins and a landscape orientation.
    let mut d = Drawing::a4().page("");
    d.styles_auto = r#"<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="29.7cm" fo:page-height="21cm" fo:margin-left="1cm" fo:margin-right="2cm" fo:margin-top="3cm" fo:margin-bottom="4cm" style:print-orientation="landscape"/></style:page-layout>"#.to_owned();
    let (doc, _) = d.read();
    let s = &page(&doc, 0).sheet;
    for (name, v) in [("PageLeftMargin", 1.0), ("PageRightMargin", 2.0), ("PageTopMargin", 3.0), ("PageBottomMargin", 4.0)] {
        close(num(s.cell(name)), v / 2.54);
    }
    close(num(s.cell("PrintPageOrientation")), 2.0);
}

#[test]
fn odg_shapes_the_first_scale_items_and_the_styles_part_s_font_faces() {
    use crate::drawing::NS;
    // The first of two items wins; no items, no warning; items in a set of another vocabulary are not the scale.
    let mut d = Drawing::a4().page("");
    d.settings = Some(settings(concat!(
        r#"<config:config-item config:name="ScaleNumerator">1</config:config-item><config:config-item config:name="ScaleNumerator">9</config:config-item>"#,
        r#"<config:config-item config:name="ScaleDenominator">4</config:config-item><config:config-item config:name="ScaleDenominator">8</config:config-item>"#
    )));
    let (doc, report) = d.read();
    close(num(page(&doc, 0).sheet.cell("DrawingScale")), 4.0 / 2.54);
    close(num(page(&doc, 0).sheet.cell("PageScale")), 1.0 / 2.54);
    assert_eq!(report.warned("scale_unreadable"), 0);
    let mut d = Drawing::a4().page("");
    d.settings = Some(settings(""));
    assert_eq!(d.read().1.warned("scale_unreadable"), 0);
    let mut d = Drawing::a4().page("");
    d.settings = Some(format!(
        "<?xml version=\"1.0\"?>\n<office:document-settings {NS} xmlns:x=\"urn:example:x\"><office:settings><x:config-item-set config:name=\"ooo:configuration-settings\"><config:config-item config:name=\"ScaleNumerator\">3</config:config-item><config:config-item config:name=\"ScaleDenominator\">7</config:config-item></x:config-item-set></office:settings></office:document-settings>"
    ));
    close(num(page(&d.read().0, 0).sheet.cell("DrawingScale")), 1.0 / 2.54);
    // The generator is `meta:generator`, not the first element of `office:meta`.
    let mut d = Drawing::a4().page("");
    d.meta = Some(format!(
        "<?xml version=\"1.0\"?>\n<office:document-meta {NS}><office:meta><meta:initial-creator>Someone</meta:initial-creator><meta:generator>Tool/3</meta:generator></office:meta></office:document-meta>"
    ));
    assert_eq!(d.read().1.generator.as_deref(), Some("Tool/3"));
    // A face declared only in styles.xml names the font of a common style.
    let mut d = Drawing::a4()
        .common(r#"<style:style style:name="Body" style:family="paragraph"><style:text-properties style:font-name="F9"/></style:style>"#)
        .page("");
    d.fonts = r#"<style:font-face style:name="F9" svg:font-family="Gentium"/>"#.to_owned();
    let files: Vec<(String, Vec<u8>)> = d
        .files()
        .into_iter()
        .map(|(name, data)| {
            if name == "content.xml" {
                let text = String::from_utf8(data).unwrap().replace(r#"<style:font-face style:name="F9" svg:font-family="Gentium"/>"#, "");
                (name, text.into_bytes())
            } else {
                (name, data)
            }
        })
        .collect();
    let (doc, _) = read(&crate::build::package(crate::build::DRAWING, files), &Limits::default()).unwrap();
    let body = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Body")).unwrap();
    let font = crate::drawing::cell_at(&body.sheet, &diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "Font"));
    assert_eq!(crate::drawing::text(font), "Gentium");
}

#[test]
fn pages_masters_fonts_and_the_page_cap_at_its_edge() {
    // A master page holding a layer set and no shape is no background page.
    let mut d = Drawing::a4().page("");
    d.masters = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:layer-set><draw:layer draw:name="L"/></draw:layer-set></style:master-page>"#.to_owned();
    let (doc, report) = d.read();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(report.warned("master_page_not_found"), 0);
    // A face declared only in content.xml names the font of an automatic style there.
    let mut d = Drawing::a4()
        .auto(r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties style:font-name="C1"/></style:style>"#)
        .page(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p text:style-name="P1">x</text:p></draw:rect>"#);
    d.fonts = r#"<style:font-face style:name="C1" svg:font-family="Content Face"/>"#.to_owned();
    let files: Vec<(String, Vec<u8>)> = d
        .files()
        .into_iter()
        .map(|(name, data)| {
            if name == "styles.xml" {
                let text = String::from_utf8(data).unwrap().replace(r#"<style:font-face style:name="C1" svg:font-family="Content Face"/>"#, "");
                (name, text.into_bytes())
            } else {
                (name, data)
            }
        })
        .collect();
    let (doc, _) = read(&crate::build::package(crate::build::DRAWING, files), &Limits::default()).unwrap();
    assert_eq!(crate::drawing::text(row_cell(only(&doc), "Character", None, 0, "Font")), "Content Face");
    // Exactly at the page cap reads; one more is refused.
    let pages = (0..5).fold(Drawing::a4(), |d, _| d.page(""));
    assert!(read(&pages.bytes(), &Limits { max_pages: 5, ..Limits::default() }).is_ok());
    // ...and the background pages count against it too.
    let mut d = (0..4).fold(Drawing::a4(), |d, _| d.page(""));
    d.masters =
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect svg:width="1cm" svg:height="1cm"/></style:master-page>"#
            .to_owned();
    assert!(read(&d.bytes(), &Limits { max_pages: 5, ..Limits::default() }).is_ok());
    assert_eq!(read(&d.bytes(), &Limits { max_pages: 4, ..Limits::default() }).err().map(|e| e.class()), Some("too_many_pages"));
}

#[test]
fn the_nearer_padding_wins_and_defaults_serve_their_own_kinds() {
    let (doc, _) = Drawing::a4()
        .common(r#"<style:style style:name="P" style:family="graphic"><style:graphic-properties fo:padding-left="0.5cm"/></style:style>"#)
        .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="P"><style:graphic-properties fo:padding="0.2cm"/></style:style>"#)
        .page(r#"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"#)
        .read();
    close(num(crate::drawing::cell(only(&doc), "LeftMargin")), 0.2 / 2.54);
    // A default paragraph style's graphic properties are not the root's fill: the graphic default's are.
    let (doc, _) = Drawing::a4()
        .common(concat!(
            r##"<style:default-style style:family="paragraph"><style:graphic-properties draw:fill-color="#abcdef"/></style:default-style>"##,
            r##"<style:default-style style:family="graphic"><style:graphic-properties draw:fill-color="#123456"/></style:default-style>"##
        ))
        .page("")
        .read();
    let root = doc.styles.iter().find(|s| s.id == 0).unwrap();
    assert_eq!(crate::drawing::text(root.sheet.cell("FillForegnd")), "#123456");
}

#[test]
fn a_shape_s_own_tab_stops_and_the_cells_it_is_charged() {
    let (doc, report) = Drawing::a4()
        .auto(r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties><style:tab-stops><style:tab-stop style:position="2cm" style:type="right"/></style:tab-stops></style:paragraph-properties></style:style>"#)
        .page(r#"<draw:rect draw:text-style-name="P1" svg:width="1cm" svg:height="1cm"><text:p>a</text:p></draw:rect>"#)
        .read();
    let s = only(&doc);
    close(num(row_cell(s, "Tabs", None, 0, "Position0")), 2.0 / 2.54);
    close(num(row_cell(s, "Tabs", None, 0, "Alignment0")), 2.0);
    // Every cell is charged: the page sheet's 9, the shape's frame 6, its rectangle 10, its connection rows 20 and its 2 tab cells.
    assert_eq!(report.stats.cells, 9 + 6 + 10 + 20 + 2);
    assert!(report.work.items >= 4 * report.stats.cells as u64, "four items a cell, and more for the rest");
}

#[test]
fn a_style_is_charged_for_itself_and_what_it_states() {
    let items = |styles: &str| Drawing::a4().auto(styles).page("").read().1.work.items;
    let none = items("");
    let style = |name: &str, parent: &str, props: &str, tabs: &str| {
        format!(
            r#"<style:style style:name="{name}" style:family="graphic"{parent}><style:graphic-properties {props}/><style:paragraph-properties>{tabs}</style:paragraph-properties></style:style>"#
        )
    };
    let three = r##"draw:fill-color="#111111" svg:stroke-width="1pt" draw:shadow="visible""##;
    let one = r##"draw:fill-color="#222222""##;
    let stops = r#"<style:tab-stops><style:tab-stop style:position="1cm"/><style:tab-stop style:position="2cm"/></style:tab-stops>"#;
    let delta = |styles: String| items(&styles) - none;
    // A style no shape names is kept as read: one item, one for each property and each tab stop it states, and one for each 64 bytes of
    // its names and values (its name `gx`, 2 bytes; each colour 7, the width 3 and the shadow 7).
    let bytes = |n: u64| n.div_ceil(64);
    // **And, the style is kept as what no cell reads** (`odfdoc.rs`): none of these styles holds an attribute that a cell does not read (a fill's colour,
    // a stroke's width, a shadow, a gradient's name: each is a cell), so none is kept and the figure is the style's own. A list style is kept whole (its record of 2 items, each element
    // (3 items) and attribute (2), the bytes of every name and value at 64 to the item, and the bytes of its family and name).
    let kept = |family: &str, name: &str, elements: &[(&str, &[(&str, &str)])]| -> u64 {
        let (mut items, mut raw) = (2u64, 0usize);
        for (element, attrs) in elements {
            items += 3 + 2 * attrs.len() as u64;
            raw += element.len() + attrs.iter().map(|(n, v)| n.len() + v.len()).sum::<usize>();
        }
        items + bytes(raw as u64) + bytes((family.len() + name.len()) as u64)
    };
    let whole = |_name: &str, _extra: &[(&str, &str)], _props: &[(&str, &str)], _tabs: bool| -> u64 { 0 };
    // The values a shape's own style gives are made once, when the style is read, and charged then: a template and a pair for each attribute, and the tab stops' element.
    let template = |pairs: &[(&str, &str)], element: u64| -> u64 {
        1 + 2 * pairs.len() as u64 + element + bytes(pairs.iter().map(|(n, v)| (n.len() + v.len()) as u64).sum())
    };
    let shadow = template(&[("draw:shadow", "visible")], 0);
    // `style:tab-stops` and its two stops: 3 + 2 * (3 + 2) items, and the bytes of their names and values.
    let stops_element =
        3 + 2 * (3 + 2) + bytes(("style:tab-stops".len() + 2 * ("style:tab-stop".len() + "style:position".len() + "1cm".len())) as u64);
    let one_p: &[(&str, &str)] = &[("draw:fill-color", "#222222")];
    let three_p: &[(&str, &str)] = &[("draw:fill-color", "#111111"), ("svg:stroke-width", "1pt"), ("draw:shadow", "visible")];
    assert_eq!(delta(style("gx", "", one, "")), 1 + 1 + bytes(2 + 7) + whole("gx", &[], one_p, false));
    assert_eq!(delta(style("gx", "", three, "")), 1 + 3 + bytes(2 + 7 + 3 + 7) + whole("gx", &[], three_p, false) + shadow);
    assert_eq!(
        delta(style("gx", "", three, stops)),
        1 + 3 + 2 + bytes(2 + 7 + 3 + 7) + whole("gx", &[], three_p, true) + shadow + template(&[], stops_element)
    );
    let parent_gp: &[(&str, &str)] = &[("style:parent-style-name", "gp")];
    assert_eq!(
        delta(format!("{}{}", style("gp", "", three, ""), style("gx", r#" style:parent-style-name="gp""#, one, ""))),
        (1 + 3 + bytes(2 + 17)) + (1 + 1 + bytes(2 + 2 + 7)) + whole("gp", &[], three_p, false) + shadow + whole("gx", parent_gp, one_p, false)
    );
    // The bytes are summed and then charged: the name's 2 and a value's 70 are two items.
    let long = format!(r#"draw:fill-gradient-name="{}""#, "g".repeat(70));
    let gradient_name = "g".repeat(70);
    // A style that names a gradient holds the fill value (the name is held whatever `draw:fill` the style says), made once and charged as a template.
    let named = template(&[("draw:fill-gradient-name", &gradient_name)], 0);
    assert_eq!(
        delta(style("gx", "", &long, "")),
        1 + 1 + bytes(2 + 70) + whole("gx", &[], &[("draw:fill-gradient-name", &gradient_name)], false) + named
    );
    // A display name and a parent's name are held too: 70 bytes each beside the name and the colour.
    let display = format!(r#" style:display-name="{}""#, "d".repeat(70));
    let shown = "d".repeat(70);
    assert_eq!(delta(style("gx", &display, one, "")), 1 + 1 + bytes(2 + 70 + 7) + whole("gx", &[("style:display-name", &shown)], one_p, false));
    let parent = format!(r#" style:parent-style-name="{}""#, "p".repeat(70));
    let named = "p".repeat(70);
    assert_eq!(delta(style("gx", &parent, one, "")), 1 + 1 + bytes(2 + 70 + 7) + whole("gx", &[("style:parent-style-name", &named)], one_p, false));
    // A list style: one item, one for each level, and its strings' bytes (its name and its levels' bullets, 1 byte each; then a face's
    // name of 100). It is also kept whole.
    let list = |name: &str, levels: &[Vec<(&str, &str)>], extra: &[(&str, Vec<(&str, &str)>)]| -> u64 {
        let mut elements: Vec<(&str, Vec<(&str, &str)>)> = vec![("text:list-style", vec![("style:name", name)])];
        elements.extend(levels.iter().map(|a| ("text:list-level-style-bullet", a.clone())));
        elements.extend(extra.iter().cloned());
        let borrowed: Vec<(&str, &[(&str, &str)])> = elements.iter().map(|(e, a)| (*e, a.as_slice())).collect();
        kept("auto:list-style", name, &borrowed)
    };
    let levels = r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/><text:list-level-style-bullet text:level="2" text:bullet-char="y"/></text:list-style>"#;
    let two = [vec![("text:level", "1"), ("text:bullet-char", "x")], vec![("text:level", "2"), ("text:bullet-char", "y")]];
    assert_eq!(delta(levels.to_owned()), 1 + 2 + bytes(3) + list("L", &two, &[]));
    let faced = format!(
        r#"<text:list-style style:name="L"><text:list-level-style-bullet text:level="1" text:bullet-char="x"><style:text-properties style:font-name="{}"/></text:list-level-style-bullet></text:list-style>"#,
        "f".repeat(100)
    );
    let face = "f".repeat(100);
    assert_eq!(
        delta(faced),
        1 + 1 + bytes(1 + 1 + 100) + list("L", &two[..1], &[("style:text-properties", vec![("style:font-name", face.as_str())])])
    );
    // The name's bytes and the levels' are summed: 70 and 70.
    let long = format!(
        r#"<text:list-style style:name="{}"><text:list-level-style-bullet text:level="1" text:bullet-char="{}"/></text:list-style>"#,
        "l".repeat(70),
        "x".repeat(70)
    );
    let (long_name, long_char) = ("l".repeat(70), "x".repeat(70));
    assert_eq!(delta(long), 1 + 1 + bytes(140) + list(&long_name, &[vec![("text:level", "1"), ("text:bullet-char", long_char.as_str())]], &[]));
}
