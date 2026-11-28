//! The vectors, the reader's side: what the root, the style sheets
//! and a shape's local cells hold for each property, and the text's runs, white space, lists and fields. The writer's sides (
//! and the writing halves of the others) are.

use diacraft_doc::{Document, Style, TextPart};

use crate::drawing::{Drawing, cell, cell_at, close, cm, formula, num, only, page, row_cell, text, unit};

/// The style sheet named `name_u`.
fn sheet<'a>(doc: &'a Document, name_u: &str) -> &'a Style {
    doc.styles.iter().map(|s| s.as_ref()).find(|s| s.name_u.as_deref() == Some(name_u)).expect("no such style sheet")
}

fn root(doc: &Document) -> &Style {
    doc.styles.iter().map(|s| s.as_ref()).find(|s| s.id == 0).expect("no root")
}

fn rect(style: &str, inner: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm">{inner}</draw:rect>"#)
}

fn graphic(name: &str, parent: Option<&str>, props: &str) -> String {
    let parent = parent.map_or(String::new(), |p| format!(r#" style:parent-style-name="{p}""#));
    format!(r#"<style:style style:name="{name}" style:family="graphic"{parent}><style:graphic-properties {props}/></style:style>"#)
}

fn paragraph_style(name: &str, para: &str, text_props: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties {para}/><style:text-properties {text_props}/></style:style>"#
    )
}

#[test]
fn odg_styles_default_graphic_style_is_the_root() {
    let (doc, _) = Drawing::a4()
        .common(r##"<style:default-style style:family="graphic"><style:graphic-properties svg:stroke-width="0.03cm" draw:fill-color="#e0ecf8"/></style:default-style>"##)
        .page("")
        .read();
    let r = root(&doc);
    assert_eq!(r.name_u.as_deref(), Some("No Style"));
    let weight = r.sheet.cell("LineWeight");
    close(num(weight), 0.011811023622047244);
    assert_eq!(unit(weight), Some("CM"));
    assert_eq!(text(r.sheet.cell("FillForegnd")), "#E0ECF8");
    // The rest of the root is DiaCraft's new drawing's.
    assert_eq!(text(r.sheet.cell("LinePattern")), "1");
}

#[test]
fn odg_styles_common_style_names_kept_encoded_and_links_to_its_parent() {
    let (doc, _) = Drawing::a4()
        .common(&graphic("Base", None, r##"draw:fill-color="#ffffff""##))
        .common(r##"<style:style style:name="Box_20_Plain" style:display-name="Box Plain" style:family="graphic" style:parent-style-name="Base"><style:graphic-properties draw:fill-color="#808080"/></style:style>"##)
        .page("")
        .read();
    let base = sheet(&doc, "Base");
    let boxed = sheet(&doc, "Box_20_Plain");
    assert_eq!(boxed.name.as_deref(), Some("Box Plain"));
    assert_eq!(base.name.as_deref(), Some("Base"), "no display name: the name");
    let links = boxed.sheet.links;
    assert_eq!((links.line, links.fill, links.text), (Some(base.id), Some(base.id), Some(base.id)));
    assert_eq!((base.sheet.links.line, base.sheet.links.fill), (Some(0), Some(0)), "no parent: the root");
    assert_eq!((base.id, boxed.id), (1, 2), "IDs from 1 in document order");
}

#[test]
fn odg_styles_automatic_style_is_local_cells_over_its_parent() {
    let (doc, _) = Drawing::a4()
        .common(&graphic("Box_20_Plain", None, r##"svg:stroke-width="0.1cm""##))
        .auto(&graphic("gr4", Some("Box_20_Plain"), r##"draw:fill-color="#336699""##))
        .page(&rect("gr4", ""))
        .read();
    let s = only(&doc);
    assert_eq!(text(cell(s, "FillForegnd")), "#336699");
    assert!(cell(s, "LineWeight").is_none(), "inherited, not local");
    let target = sheet(&doc, "Box_20_Plain").id;
    assert_eq!((s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text), (Some(target), Some(target), Some(target)));
}

#[test]
fn odg_styles_a_shape_naming_a_common_style_has_no_local_cells() {
    let (doc, _) = Drawing::a4().common(&graphic("Plain", None, r##"draw:fill-color="#123456""##)).page(&rect("Plain", "")).read();
    let s = only(&doc);
    assert!(cell(s, "FillForegnd").is_none());
    let target = sheet(&doc, "Plain").id;
    assert_eq!(s.sheet.links.fill, Some(target));
}

#[test]
fn odg_styles_a_property_equal_to_the_parent_s_is_not_local() {
    let (doc, _) = Drawing::a4()
        .common(&graphic("P", None, r##"draw:fill-color="#336699" svg:stroke-width="0.1cm""##))
        .auto(&graphic("gr1", Some("P"), r##"draw:fill-color="#336699" svg:stroke-width="0.2cm""##))
        .page(&rect("gr1", ""))
        .read();
    let s = only(&doc);
    assert!(cell(s, "FillForegnd").is_none(), "equal to what the link gives");
    close(num(cell(s, "LineWeight")), cm(0.2));
}

#[test]
fn odg_styles_diacraft_links_read_back_and_a_changed_filled_property_is_local() {
    let styles = |fill: &str| {
        Drawing::a4()
            .common(&graphic("Ln", None, r##"draw:fill-color="#ffffff""##))
            .common(&graphic("Fl", None, r##"draw:fill-color="#aa0000""##))
            .auto(&format!(
                r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Ln"><style:graphic-properties draw:fill-color="{fill}" diacraft:version="1" diacraft:links="Ln Fl Ln" diacraft:filled="draw:fill-color"/></style:style>"#
            ))
            .page(&rect("gr1", ""))
            .read()
    };
    let (doc, report) = styles("#aa0000");
    // `diacraft:version` and `diacraft:links` are read; `diacraft:filled` is kept for the tier-3 check.
    assert_eq!(report.kept("diacraft_attribute"), 1);
    let s = only(&doc);
    let (ln, fl) = (sheet(&doc, "Ln").id, sheet(&doc, "Fl").id);
    assert_eq!((s.sheet.links.line, s.sheet.links.fill, s.sheet.links.text), (Some(ln), Some(fl), Some(ln)));
    assert!(cell(s, "FillForegnd").is_none(), "the filled-in property equals what the fill link gives");
    let (doc, _) = styles("#00aa00");
    assert_eq!(text(cell(only(&doc), "FillForegnd")), "#00AA00", "changed by another program: local");
    // What the parent gives and the automatic style does not set is not the shape's own, even where the fill link is another style.
    let (doc, _) = Drawing::a4()
        .common(&graphic("Ln", None, r##"draw:fill-color="#ffffff""##))
        .common(&graphic("Fl", None, r##"draw:fill-color="#aa0000""##))
        .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Ln"><style:graphic-properties diacraft:version="1" diacraft:links="Ln Fl Ln" diacraft:filled=""/></style:style>"#)
        .page(&rect("gr1", ""))
        .read();
    let s = only(&doc);
    assert_eq!(s.sheet.links.fill, Some(sheet(&doc, "Fl").id));
    assert!(cell(s, "FillForegnd").is_none(), "the parent's white is not local; the fill link gives red");
}

#[test]
fn odg_styles_fill_solid_colour_and_opacity() {
    let (doc, _) = Drawing::a4()
        .auto(&graphic("gr1", None, r##"draw:fill="solid" draw:fill-color="#ffcc00" draw:opacity="60%""##))
        .page(&rect("gr1", ""))
        .read();
    let s = only(&doc);
    assert_eq!(text(cell(s, "FillPattern")), "1");
    assert_eq!(text(cell(s, "FillForegnd")), "#FFCC00");
    close(num(cell(s, "FillForegndTrans")), 0.4);
    close(num(cell(s, "FillBkgndTrans")), 0.4);
}

#[test]
fn odg_styles_line_caps_by_their_drawing() {
    for (cap, n) in [("butt", 1.0), ("square", 2.0), ("round", 0.0)] {
        let (doc, _) = Drawing::a4().auto(&graphic("gr1", None, &format!(r#"svg:stroke-linecap="{cap}""#))).page(&rect("gr1", "")).read();
        close(num(cell(only(&doc), "LineCap")), n);
    }
}

fn dash_drawing(dash: &str, cap: &str) -> Drawing {
    Drawing::a4()
        .common(&format!(r#"<draw:stroke-dash draw:name="D" {dash}/>"#))
        .auto(&graphic("gr1", None, &format!(r#"draw:stroke="dash" draw:stroke-dash="D" svg:stroke-width="1pt" svg:stroke-linecap="{cap}""#)))
        .page(&rect("gr1", ""))
}

#[test]
fn odg_styles_dashes_match_the_pattern_table_by_cap() {
    let round = r#"draw:dots1="1" draw:dots1-length="1900%" draw:dots2="1" draw:dots2-length="700%" draw:distance="500%""#;
    let (doc, report) = dash_drawing(round, "round").read();
    close(num(cell(only(&doc), "LinePattern")), 7.0);
    assert_eq!(report.kept("dash"), 0);
    let flat = r#"draw:dots1="1" draw:dots1-length="2000%" draw:dots2="1" draw:dots2-length="800%" draw:distance="400%""#;
    let (doc, _) = dash_drawing(flat, "butt").read();
    close(num(cell(only(&doc), "LinePattern")), 7.0);
    let close_dash = r#"draw:dots1="1" draw:dots1-length="300%" draw:distance="300%""#;
    let (doc, _) = dash_drawing(close_dash, "round").read();
    close(num(cell(only(&doc), "LinePattern")), 9.0);
    // Lengths as lengths, in weights of the 1 pt line: 3 pt marks and gaps are pattern 9 too.
    let (doc, _) = dash_drawing(r#"draw:dots1="1" draw:dots1-length="3pt" draw:distance="3pt""#, "round").read();
    close(num(cell(only(&doc), "LinePattern")), 9.0);
    // No match: the stand-in 2, and the dash kept.
    let (doc, report) = dash_drawing(r#"draw:dots1="1" draw:dots1-length="250%" draw:distance="300%""#, "round").read();
    close(num(cell(only(&doc), "LinePattern")), 2.0);
    assert_eq!(report.kept("dash"), 1);
}

#[test]
fn odg_styles_line_ends_by_diacraft_s_marker_name_or_a_stand_in() {
    let d = |marker: &str, width: &str| {
        Drawing::a4()
            .common(&format!(r#"<draw:marker draw:name="{marker}" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#))
            .auto(&graphic("gr1", None, &format!(r#"svg:stroke-width="0.75pt" draw:marker-end="{marker}" draw:marker-end-width="{width}""#)))
            .page(&rect("gr1", ""))
            .read()
    };
    let (doc, _) = d("DiaCraft_20_end_20_13", "6.54pt");
    close(num(cell(only(&doc), "EndArrow")), 13.0);
    close(num(cell(only(&doc), "EndArrowSize")), 2.0);
    let (doc, report) = d("Arrow", "0.3cm");
    close(num(cell(only(&doc), "EndArrow")), 4.0);
    close(num(cell(only(&doc), "EndArrowSize")), 2.0);
    assert_eq!(report.kept("marker"), 1);
}

#[test]
fn odg_styles_a_length_reads_as_inches_in_its_own_unit() {
    let (doc, _) = Drawing::a4().page("").read();
    let height = page(&doc, 0).sheet.cell("PageHeight");
    close(num(height), 11.69291338582677);
    assert_eq!(unit(height), Some("CM"));
}

#[test]
fn odg_styles_shadow_with_its_y_offset_sign_changed() {
    let props = r#"draw:shadow="visible" draw:shadow-offset-x="0.3cm" draw:shadow-offset-y="0.3cm" draw:shadow-opacity="40%""#;
    let turned =
        r#"<draw:rect draw:style-name="gr1" svg:width="4cm" svg:height="2.5cm" draw:transform="rotate (1.5707963267948966) translate (5cm 10cm)"/>"#;
    let (doc, _) = Drawing::a4().auto(&graphic("gr1", None, props)).page(&rect("gr1", "")).page(turned).read();
    for page_at in [0, 1] {
        let s = crate::drawing::shape(&doc, page_at, 0);
        assert_eq!(text(cell(s, "ShdwPattern")), "1");
        assert_eq!(text(cell(s, "ShapeShdwType")), "1");
        close(num(cell(s, "ShapeShdwOffsetX")), cm(0.3));
        close(num(cell(s, "ShapeShdwOffsetY")), -cm(0.3));
        close(num(cell(s, "ShdwForegndTrans")), 0.6);
    }
}

#[test]
fn odg_styles_text_area_alignment_and_padding() {
    let (doc, _) =
        Drawing::a4().auto(&graphic("gr1", None, r#"draw:textarea-vertical-align="bottom" fo:padding="0.35cm""#)).page(&rect("gr1", "")).read();
    let s = only(&doc);
    assert_eq!(text(cell(s, "VerticalAlign")), "2");
    for side in ["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"] {
        close(num(cell(s, side)), cm(0.35));
        assert_eq!(unit(cell(s, side)), Some("CM"));
    }
}

#[test]
fn odg_styles_paragraph_alignment_line_height_and_right_to_left() {
    let d =
        |para: &str| Drawing::a4().auto(&paragraph_style("P1", para, "")).page(&rect("none", r#"<text:p text:style-name="P1">x</text:p>"#)).read();
    let (doc, _) = d(r#"fo:text-align="end" fo:line-height="115%""#);
    let s = only(&doc);
    close(num(row_cell(s, "Paragraph", None, 0, "HorzAlign")), 2.0);
    close(num(row_cell(s, "Paragraph", None, 0, "SpLine")), -1.15);
    // Amended: `end` is the end of the line in either direction, so it is 2 in a right-to-left paragraph too.
    let (doc, _) = d(r#"fo:text-align="end" fo:line-height="115%" style:writing-mode="rl-tb""#);
    let s = only(&doc);
    close(num(row_cell(s, "Paragraph", None, 0, "HorzAlign")), 2.0);
    close(num(row_cell(s, "Paragraph", None, 0, "Flags")), 1.0);
}

#[test]
fn odg_styles_weight_and_double_underline_bits() {
    let d =
        |props: &str| Drawing::a4().auto(&paragraph_style("P1", "", props)).page(&rect("none", r#"<text:p text:style-name="P1">x</text:p>"#)).read();
    let (doc, _) = d(r#"fo:font-weight="700" style:text-underline-style="solid" style:text-underline-type="double""#);
    let s = only(&doc);
    close(num(row_cell(s, "Character", None, 0, "Style")), 1.0);
    close(num(row_cell(s, "Character", None, 0, "DblUnderline")), 1.0);
    let (doc, _) = d(r#"fo:font-weight="500""#);
    close(num(row_cell(only(&doc), "Character", None, 0, "Style")), 0.0);
}

#[test]
fn odg_styles_runs_and_paragraphs_numbered_by_first_use() {
    let (doc, _) = Drawing::a4()
        .auto(&paragraph_style("P1", r#"fo:text-align="center""#, ""))
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&rect(
            "none",
            r#"<text:p text:style-name="P1">ab<text:span text:style-name="T1">cd</text:span></text:p><text:p text:style-name="P1">ef</text:p>"#,
        ))
        .read();
    let s = only(&doc);
    let parts = &s.text.as_ref().expect("text").parts;
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "abcd\nef");
    let markers: Vec<String> = parts
        .iter()
        .filter_map(|p| match p {
            TextPart::Pp(ix) => Some(format!("pp{ix}")),
            TextPart::Cp(ix) => Some(format!("cp{ix}")),
            _ => None,
        })
        .collect();
    assert_eq!(markers, ["pp0", "cp1", "cp0"], "one pp, and cp markers at cd and at ef");
    assert_eq!(sequence(parts), "<pp0>ab<cp1>cd\n<cp0>ef", "each marker where its row starts");
    close(num(row_cell(s, "Paragraph", None, 0, "HorzAlign")), 1.0);
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Paragraph", None)).is_some_and(|p| p.rows.len() == 1), "one Paragraph row");
    close(num(row_cell(s, "Character", None, 1, "Style")), 1.0);
    assert!(row_cell(s, "Character", None, 0, "Style").is_none(), "row 0 holds no cells");
}

/// A shape's text parts as one string: the characters, and each marker as `<pp n>`, `<cp n>` or `<fld n>`.
fn sequence(parts: &[TextPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(c) => c.to_string(),
            TextPart::Pp(ix) => format!("<pp{ix}>"),
            TextPart::Cp(ix) => format!("<cp{ix}>"),
            TextPart::Fld { ix, .. } => format!("<fld{ix}>"),
            _ => "<other>".to_owned(),
        })
        .collect()
}

#[test]
fn odg_styles_nested_runs_trailing_space_marks_and_fields() {
    let (doc, report) = Drawing::a4()
        .auto(r##"<style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold" fo:color="#ff0000"/></style:style>"##)
        .auto(r#"<style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>"#)
        .page(&rect(
            "none",
            concat!(
                r#"<text:p><text:span text:style-name="B">a<text:span text:style-name="I">b</text:span></text:span></text:p>"#,
                r#"<text:p>c d <text:bookmark text:name="m"/></text:p>"#,
                r#"<text:p>e<text:span text:style-name="I">f</text:span> </text:p>"#,
                r#"<text:p><text:page-count>2</text:page-count><text:file-name>f</text:file-name><text:date>d</text:date></text:p>"#
            ),
        ))
        .read();
    let s = only(&doc);
    // The inner run is bold from its outer span and italic from its own, and keeps the outer span's colour.
    close(num(row_cell(s, "Character", None, 1, "Style")), 3.0);
    assert_eq!(text(row_cell(s, "Character", None, 1, "Color")), "#FF0000");
    close(num(row_cell(s, "Character", None, 0, "Style")), 1.0);
    let parts = &s.text.as_ref().expect("text").parts;
    // The trailing space goes; a bookmark is no character and nothing kept.
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "ab\nc d\nef\n", "a trailing space goes, in its own run too");
    assert_eq!(report.kept("text_element"), 0);
    // Fields: the page count, the file name, and a date that is not fixed.
    let field = |ix: u32| formula(row_cell(s, "Field", None, ix, "Value")).map(str::to_owned);
    assert_eq!((field(0).as_deref(), field(1).as_deref(), field(2).as_deref()), (Some("PAGECOUNT()"), Some("FILENAME()"), Some("NOW()")));
    assert_eq!(report.kept("field"), 0);
}

#[test]
fn odg_styles_spans_and_links_nest_to_a_fixed_depth_when_the_tree_allows_more() {
    // The container's depth cap (200) keeps nesting shallower than the reader's own 256; a caller that raises it still gets the bound.
    let mut limits = diacraft_odg::Limits::default();
    limits.container.xml.max_depth = 2_000;
    let nested = |open: &str, close: &str| {
        let body = format!("<text:p>{}{}</text:p>", open.repeat(300), close.repeat(300));
        let bytes = Drawing::a4().auto(r#"<style:style style:name="T" style:family="text"/>"#).page(&rect("none", &body)).bytes();
        let (doc, report) = diacraft_odg::read(&bytes, &limits).expect("reads");
        let parts = &only(&doc).text.as_ref().expect("text").parts;
        let read: usize = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.chars().count()) } else { None }).sum();
        (read, report.kept("text_element"))
    };
    assert_eq!(nested(r#"<text:span text:style-name="T">y"#, "</text:span>"), (256, 1), "spans 256 deep are read, the next is kept");
    assert_eq!(nested(r#"<text:a xlink:href="u">x"#, "</text:a>"), (256, 257), "links' text is read 256 deep; each link reached is kept");
}

#[test]
fn odg_styles_white_space_collapses_and_the_elements_stand_for_their_characters() {
    let p = "<text:p>a\n    <text:s text:c=\"2\"/>b<text:tab/>c<text:line-break/>d</text:p>";
    let (doc, _) = Drawing::a4().page(&rect("none", p)).read();
    let parts = &only(&doc).text.as_ref().expect("text").parts;
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "a   b\tc\u{2028}d");
    // Leading and trailing white space go; indentation between elements reads as nothing.
    let (doc, _) = Drawing::a4().page(&rect("none", "<text:p>\n   x  y \n</text:p>\n<text:p>  <text:span>z </text:span> </text:p>")).read();
    let parts = &only(&doc).text.as_ref().expect("text").parts;
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "x y\nz");
}

#[test]
fn odg_styles_bullets_and_their_indents() {
    let list = |bullet: &str| {
        Drawing::a4()
            .auto(&format!(
                r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="{bullet}"><style:list-level-properties><style:list-level-label-alignment fo:margin-left="0.8cm" fo:text-indent="-0.8cm"/></style:list-level-properties></text:list-level-style-bullet></text:list-style>"#
            ))
            .page(&rect("none", r#"<text:list text:style-name="L1"><text:list-item><text:p>a</text:p></text:list-item></text:list>"#))
            .read()
    };
    let (doc, _) = list("\u{2713}");
    let s = only(&doc);
    close(num(row_cell(s, "Paragraph", None, 0, "Bullet")), 7.0);
    close(num(row_cell(s, "Paragraph", None, 0, "IndLeft")), cm(0.8));
    close(num(row_cell(s, "Paragraph", None, 0, "IndFirst")), -cm(0.8));
    assert!(row_cell(s, "Paragraph", None, 0, "BulletStr").is_none());
    let (doc, _) = list("\u{2013}");
    let s = only(&doc);
    close(num(row_cell(s, "Paragraph", None, 0, "Bullet")), 1.0);
    assert_eq!(text(row_cell(s, "Paragraph", None, 0, "BulletStr")), "\u{2013}");
}

#[test]
fn odg_styles_reading_back_the_written_paragraph_and_character_cells() {
    let (doc, _) = Drawing::a4()
        .auto(&paragraph_style(
            "P1",
            r#"fo:text-align="center" fo:line-height="120%""#,
            r#"fo:font-weight="bold" fo:font-style="italic" fo:font-size="14pt""#,
        ))
        .page(&rect("none", r#"<text:p text:style-name="P1">x</text:p>"#))
        .read();
    let s = only(&doc);
    close(num(row_cell(s, "Paragraph", None, 0, "HorzAlign")), 1.0);
    close(num(row_cell(s, "Paragraph", None, 0, "SpLine")), -1.2);
    close(num(row_cell(s, "Character", None, 0, "Style")), 3.0);
    let size = row_cell(s, "Character", None, 0, "Size");
    close(num(size), 14.0 / 72.0);
    assert_eq!(unit(size), Some("PT"));
}

#[test]
fn odg_styles_page_number_field() {
    let (doc, report) = Drawing::a4()
        .page(&rect("none", r#"<text:p>Page <text:page-number text:select-page="current">3</text:page-number> <text:date text:fixed="true">1 May</text:date></text:p>"#))
        .read();
    let s = only(&doc);
    let field = row_cell(s, "Field", None, 0, "Value");
    assert_eq!(formula(field), Some("PAGENUMBER()"));
    assert_eq!(text(field), "3");
    let parts = &s.text.as_ref().expect("text").parts;
    assert!(parts.iter().any(|p| matches!(p, TextPart::Fld { ix: 0, text } if text.as_ref() == "3")));
    // A fixed date is its shown text, kept.
    assert!(row_cell(s, "Field", None, 1, "Value").is_none());
    assert_eq!(report.kept("field"), 1);
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "Page  1 May");
}

#[test]
fn odg_styles_a_style_that_is_not_there_warns_once_per_name() {
    let (doc, report) = Drawing::a4().page(&format!("{}{}", rect("missing", ""), rect("missing", ""))).read();
    assert_eq!(report.warned("style_not_found"), 1);
    assert_eq!(only(&doc).sheet.links.fill, Some(0));
}

#[test]
fn odg_styles_a_parent_cycle_ends_where_it_would_repeat() {
    let (doc, report) = Drawing::a4()
        .common(&graphic("A", Some("B"), r##"draw:fill-color="#111111""##))
        .common(&graphic("B", Some("A"), r##"svg:stroke-width="0.2cm""##))
        .auto(&graphic("gr1", Some("A"), ""))
        .page(&rect("gr1", ""))
        .read();
    assert!(report.warned("style_cycle") >= 1);
    assert_eq!(only(&doc).sheet.links.fill, Some(sheet(&doc, "A").id));
}

#[test]
fn a_style_sheet_s_cells_are_each_charged_once() {
    let (doc, report) = Drawing::a4()
        .common(r##"<draw:gradient draw:name="G1" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:border="20%"/>"##)
        .common(concat!(
            r#"<style:style style:name="S" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="G1" svg:stroke-width="1pt"/>"#,
            r#"<style:paragraph-properties fo:text-align="center"><style:tab-stops><style:tab-stop style:position="1cm"/></style:tab-stops></style:paragraph-properties>"#,
            r#"<style:text-properties fo:font-size="10pt"/></style:style>"#
        ))
        .page("")
        .read();
    let s = &sheet(&doc, "S").sheet;
    let in_sections: usize =
        s.sections.iter().map(|section| section.cells.len() + section.rows.iter().map(|row| row.cells.len()).sum::<usize>()).sum();
    // Every kind of cell is there: direct ones, a paragraph's, a run's, three gradient stops and a tab stop.
    for section in ["Paragraph", "Character", "FillGradient", "Tabs"] {
        assert!(s.sections.iter().any(|x| x.name.as_str() == section), "{section}");
    }
    // The page sheet's 9 cells and the style sheet's, each charged once.
    assert_eq!(report.stats.cells, 9 + s.cells.len() + in_sections);
}

#[test]
fn odg_styles_a_cycle_of_percentage_sizes_multiplies_each_style_once() {
    // A at 150% of B, B at 150% of A: A's walk ends where it would come back to A, so its size is 1.5 times 1.5 the root's 12 pt.
    let size = |name: &str| {
        format!(
            r#"<style:style style:name="{name}" style:family="paragraph" style:parent-style-name="{}"><style:text-properties fo:font-size="150%"/></style:style>"#,
            if name == "A" { "B" } else { "A" }
        )
    };
    let (doc, _) = Drawing::a4().common(&size("A")).common(&size("B")).page("").read();
    let a = sheet(&doc, "A");
    let at = cell_at(&a.sheet, &diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "Size"));
    close(num(at), 0.1666667 * 2.25);
}

#[test]
fn odg_styles_an_automatic_parent_is_followed_with_a_warning() {
    let (doc, report) = Drawing::a4()
        .auto(&graphic("gr0", None, r##"draw:fill-color="#222222""##))
        .auto(&graphic("gr1", Some("gr0"), r#"svg:stroke-width="0.2cm""#))
        .page(&rect("gr1", ""))
        .read();
    assert_eq!(report.warned("style_parent_automatic"), 1);
    let s = only(&doc);
    assert_eq!(text(cell(s, "FillForegnd")), "#222222", "the automatic parent's properties are the shape's own");
}

#[test]
fn odg_styles_a_paragraph_style_link_and_the_graphic_chain_s_text_properties() {
    let (doc, _) = Drawing::a4()
        .common(
            r##"<style:style style:name="G" style:family="graphic"><style:text-properties fo:font-size="18pt" fo:color="#112233"/></style:style>"##,
        )
        .common(&paragraph_style("Body", "", r##"fo:color="#445566""##))
        .page(r#"<draw:rect draw:style-name="G" draw:text-style-name="Body" svg:width="2cm" svg:height="1cm"><text:p>x</text:p></draw:rect>"#)
        .read();
    let s = only(&doc);
    let body = sheet(&doc, "Body");
    assert_eq!(s.sheet.links.text, Some(body.id));
    assert_eq!(s.sheet.links.fill, Some(sheet(&doc, "G").id));
    // The size the paragraph chain leaves unset and the graphic chain sets is the shape's own; the colour the paragraph chain sets is not.
    close(num(row_cell(s, "Character", None, 0, "Size")), 18.0 / 72.0);
    assert!(row_cell(s, "Character", None, 0, "Color").is_none());
    assert_eq!(text(cell_at(&body.sheet, &diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "Color"))), "#445566");
}

/// The cells a rectangle on an automatic style of `graphic` properties gets, and of a paragraph style of `para` and `text_props` on its text.
fn styled(graphic_props: &str, para: &str, text_props: &str) -> (Document, diacraft_odg::ReadReport) {
    Drawing::a4()
        .common(r##"<draw:gradient draw:name="G1" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:start-intensity="50%" draw:border="20%"/><draw:gradient draw:name="G2" draw:style="axial" draw:start-color="#ff0000" draw:end-color="#0000ff"/><draw:gradient draw:name="G3" draw:style="radial" draw:start-color="#ff0000" draw:end-color="#0000ff"/>"##)
        .auto(&graphic("gr1", None, graphic_props))
        .auto(&paragraph_style("P1", para, text_props))
        .fonts_named()
        .page(&rect("gr1", r#"<text:p text:style-name="P1">x</text:p>"#))
        .read()
}

trait Fonts {
    fn fonts_named(self) -> Self;
}

impl Fonts for Drawing {
    fn fonts_named(mut self) -> Drawing {
        self.fonts =
            r#"<style:font-face style:name="F1" svg:font-family="'Liberation Sans'"/><style:font-face style:name="F2" svg:font-family="Noto"/>"#
                .to_owned();
        self
    }
}

#[test]
fn odg_styles_fill_and_line_stand_ins_and_value_forms() {
    let c = |props: &str, name: &str| {
        let (doc, _) = styled(props, "", "");
        cell(only(&doc), name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    assert_eq!(c(r#"draw:fill="none""#, "FillPattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:fill="bitmap""#, "FillPattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:fill="hatch" draw:fill-hatch-solid="true""#, "FillPattern").as_deref(), Some("1"));
    assert_eq!(c(r#"draw:fill="hatch""#, "FillPattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:fill="hatch" draw:fill-hatch-solid="false""#, "FillPattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:fill="solid""#, "FillGradientEnabled").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:fill="pattern""#, "FillPattern"), None, "a value the standard does not define is ignored");
    assert_eq!(c(r#"draw:stroke="none""#, "LinePattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:stroke="solid""#, "LinePattern").as_deref(), Some("1"));
    assert_eq!(c(r#"draw:stroke="dash""#, "LinePattern").as_deref(), Some("2"), "a dash that is not there: the stand-in");
    assert_eq!(c(r#"svg:stroke-opacity="75%""#, "LineColorTrans").as_deref(), Some("0.25"));
    assert_eq!(c(r#"svg:stroke-opacity="0.75""#, "LineColorTrans").as_deref(), Some("0.25"));
    assert_eq!(c(r#"svg:stroke-opacity="0.333""#, "LineColorTrans").as_deref(), Some("0.665"), "to the nearest 0.005");
    assert_eq!(c(r#"svg:stroke-opacity="x""#, "LineColorTrans"), None);
    assert_eq!(c(r#"svg:stroke-linecap="wide""#, "LineCap"), None);
    assert_eq!(c(r##"svg:stroke-color="#0a0b0c""##, "LineColor").as_deref(), Some("#0A0B0C"));
    assert_eq!(c(r#"svg:stroke-width="0.1cm""#, "LineWeight").and_then(|v| v.parse::<f64>().ok()).map(|v| (v * 2.54 * 1e9).round()), Some(1e8));
    assert_eq!(c(r#"draw:marker-start="""#, "BeginArrow").as_deref(), Some("0"), "an empty marker is none");
    assert_eq!(c(r#"draw:marker-start="Nowhere""#, "BeginArrow").as_deref(), Some("0"), "a marker the drawing lacks is none");
    assert_eq!(c(r#"draw:shadow="hidden""#, "ShdwPattern").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:shadow="hidden""#, "ShapeShdwType"), None);
    assert_eq!(c(r##"draw:shadow-color="#123456""##, "ShdwForegnd").as_deref(), Some("#123456"));
    assert_eq!(c(r#"draw:textarea-vertical-align="top""#, "VerticalAlign").as_deref(), Some("0"));
    assert_eq!(c(r#"draw:textarea-vertical-align="middle""#, "VerticalAlign").as_deref(), Some("1"));
    assert_eq!(c(r#"draw:textarea-vertical-align="justify""#, "VerticalAlign").as_deref(), Some("1"));
    assert_eq!(c(r#"style:writing-mode="tb-rl""#, "TextDirection").as_deref(), Some("1"));
    assert_eq!(c(r#"style:writing-mode="sideways-lr""#, "TextDirection").as_deref(), Some("1"));
    assert_eq!(c(r#"style:writing-mode="lr-tb""#, "TextDirection").as_deref(), Some("0"));
    // A side's own padding wins over `fo:padding` on one style.
    let (doc, _) = styled(r#"fo:padding="0.2cm" fo:padding-left="0.5cm""#, "", "");
    close(num(cell(only(&doc), "LeftMargin")), cm(0.5));
    close(num(cell(only(&doc), "RightMargin")), cm(0.2));
}

#[test]
fn odg_styles_gradient_stand_ins() {
    let stops = |name: &str| {
        let (doc, report) = styled(&format!(r#"draw:fill="gradient" draw:fill-gradient-name="{name}""#), "", "");
        let s = only(&doc);
        let at = |ix: u32, n: &str| row_cell(s, "FillGradient", None, ix, n).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
        let dir = cell(s, "FillGradientDir").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
        assert_eq!(text(cell(s, "FillGradientEnabled")), "1");
        assert_eq!(text(cell(s, "FillPattern")), "1");
        assert_eq!(report.kept("gradient"), 1);
        // The cells the renderer and a VSDX file read (the reader wrote `Color` and `Position`, which nothing reads), and the angle in degrees.
        let angle = cell(s, "FillGradientAngle").map(|c| (num(Some(c)).to_degrees() * 1e9).round() / 1e9);
        (dir, angle, (0..4).map(|ix| (at(ix, "GradientStopColor"), at(ix, "GradientStopPosition"))).collect::<Vec<_>>())
    };
    let (dir, angle, rows) = stops("G1");
    assert_eq!((dir.as_deref(), angle), (Some("0"), Some(90.0)), "no `draw:angle` is 0, from the top down: 90 degrees");
    let some = |c: &str, p: &str| (Some(c.to_owned()), Some(p.to_owned()));
    assert_eq!(rows, [some("#800000", "0"), some("#800000", "0.2"), some("#0000FF", "1"), (None, None)], "the start at half intensity, a 20% border");
    let (dir, angle, rows) = stops("G2");
    assert_eq!((dir.as_deref(), angle), (Some("0"), Some(90.0)));
    assert_eq!(rows[..3], [some("#0000FF", "0"), some("#FF0000", "0.5"), some("#0000FF", "1")]);
    // A radial gradient with no centre is drawn from the top-left corner (measured), where LibreOffice ends it at half the way to the far corner.
    let (dir, angle, rows) = stops("G3");
    assert_eq!((dir.as_deref(), angle), (Some("7"), None));
    assert_eq!(rows[..4], [some("#0000FF", "0"), some("#FF0000", "0.5"), some("#FF0000", "1"), (None, None)]);
    let (doc, report) = styled(r#"draw:fill="gradient" draw:fill-gradient-name="Gx""#, "", "");
    assert_eq!(text(cell(only(&doc), "FillGradientEnabled")), "0");
    assert_eq!(report.warned("gradient_not_found"), 1);
}

#[test]
fn odg_styles_paragraph_value_forms() {
    let p = |para: &str, name: &str| {
        let (doc, _) = styled("", para, "");
        row_cell(only(&doc), "Paragraph", None, 0, name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    assert_eq!(p(r#"fo:text-align="left""#, "HorzAlign").as_deref(), Some("0"));
    assert_eq!(p(r#"fo:text-align="start""#, "HorzAlign").as_deref(), Some("0"));
    assert_eq!(p(r#"fo:text-align="right""#, "HorzAlign").as_deref(), Some("2"));
    assert_eq!(p(r#"fo:text-align="justify""#, "HorzAlign").as_deref(), Some("3"));
    assert_eq!(p(r#"fo:text-align="justify" fo:text-align-last="justify""#, "HorzAlign").as_deref(), Some("4"));
    // `start` and `end` are 0 and 2 whatever the writing mode (the short form `rl` too); `left` and `right` are read against a right-to-left one.
    assert_eq!(p(r#"fo:text-align="start" style:writing-mode="rl""#, "HorzAlign").as_deref(), Some("0"));
    assert_eq!(p(r#"fo:text-align="end" style:writing-mode="rl""#, "HorzAlign").as_deref(), Some("2"));
    assert_eq!(p(r#"fo:text-align="left" style:writing-mode="rl""#, "HorzAlign").as_deref(), Some("2"));
    assert_eq!(p(r#"fo:text-align="right" style:writing-mode="rl""#, "HorzAlign").as_deref(), Some("0"));
    assert_eq!(p(r#"style:writing-mode="lr-tb""#, "Flags").as_deref(), Some("0"));
    assert_eq!(p(r#"fo:line-height="normal""#, "SpLine"), None);
    assert!(p(r#"fo:line-height="0.5cm""#, "SpLine").is_some_and(|v| (v.parse::<f64>().unwrap() - cm(0.5)).abs() < 1e-12));
    assert!(p(r#"fo:margin-top="1pt""#, "SpBefore").is_some_and(|v| (v.parse::<f64>().unwrap() - 1.0 / 72.0).abs() < 1e-12));
    assert!(p(r#"fo:margin-bottom="2pt""#, "SpAfter").is_some_and(|v| (v.parse::<f64>().unwrap() - 2.0 / 72.0).abs() < 1e-12));
    assert!(p(r#"fo:margin-right="3pt""#, "IndRight").is_some_and(|v| (v.parse::<f64>().unwrap() - 3.0 / 72.0).abs() < 1e-12));
    assert_eq!(p(r#"fo:margin-left="5%""#, "IndLeft"), None, "a percentage indent is kept");
}

#[test]
fn odg_styles_a_child_that_sets_only_the_last_line_s_alignment_and_line_heights_kept() {
    // The parent justifies; a child that sets only `fo:text-align-last` justifies its last line too: alignment 4, local to the shape.
    let (doc, _) = Drawing::a4()
        .common(r#"<style:style style:name="J" style:family="paragraph"><style:paragraph-properties fo:text-align="justify"/></style:style>"#)
        .auto(r#"<style:style style:name="P1" style:family="paragraph" style:parent-style-name="J"><style:paragraph-properties fo:text-align-last="justify"/></style:style>"#)
        .page(&rect("none", r#"<text:p text:style-name="P1">x</text:p>"#))
        .read();
    close(num(row_cell(only(&doc), "Paragraph", None, 0, "HorzAlign")), 4.0);
    // A line height that is no percentage, length or `normal` is kept; `normal` is not.
    let kept = |height: &str| styled("", &format!(r#"fo:line-height="{height}""#), "").1.kept("property_value");
    assert_eq!(kept("tall"), 1);
    assert_eq!(kept("normal"), 0);
}

#[test]
fn odg_styles_character_value_forms() {
    let c = |props: &str, name: &str| {
        let (doc, _) = styled("", "", props);
        row_cell(only(&doc), "Character", None, 0, name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    assert_eq!(
        c(r#"fo:font-style="oblique" style:font-weight-complex="bold" style:font-style-complex="italic" fo:font-variant="small-caps""#, "Style")
            .as_deref(),
        Some("58")
    );
    assert_eq!(c(r#"style:text-underline-style="solid""#, "Style").as_deref(), Some("4"));
    assert_eq!(c(r#"style:text-underline-style="none""#, "DblUnderline").as_deref(), Some("0"));
    assert_eq!(c(r#"style:text-underline-style="solid""#, "DblUnderline").as_deref(), Some("0"), "a single line");
    assert_eq!(c(r#"style:text-underline-type="double""#, "DblUnderline").as_deref(), Some("0"), "a double line of no style is none");
    assert_eq!(c(r#"style:text-line-through-style="solid""#, "DoubleStrikethrough").as_deref(), Some("0"));
    assert_eq!(c(r#"style:text-line-through-type="double""#, "DoubleStrikethrough").as_deref(), Some("0"));
    assert_eq!(c(r#"style:text-line-through-style="solid""#, "Strikethru").as_deref(), Some("1"));
    assert_eq!(c(r#"style:text-line-through-style="solid" style:text-line-through-type="double""#, "DoubleStrikethrough").as_deref(), Some("1"));
    assert_eq!(c(r#"style:text-line-through-style="solid" style:text-line-through-type="double""#, "Strikethru").as_deref(), Some("0"));
    assert_eq!(c(r#"style:text-overline-style="dash""#, "Overline").as_deref(), Some("1"));
    assert_eq!(c(r#"style:text-overline-style="none""#, "Overline").as_deref(), Some("0"));
    assert_eq!(c(r#"fo:text-transform="uppercase""#, "Case").as_deref(), Some("1"));
    assert_eq!(c(r#"fo:text-transform="capitalize""#, "Case").as_deref(), Some("2"));
    assert_eq!(c(r#"fo:text-transform="none""#, "Case").as_deref(), Some("0"));
    assert_eq!(c(r#"fo:text-transform="lowercase""#, "Case"), None, "kept");
    assert_eq!(c(r#"style:text-position="super 58%""#, "Pos").as_deref(), Some("1"));
    assert_eq!(c(r#"style:text-position="sub""#, "Pos").as_deref(), Some("2"));
    assert_eq!(c(r#"style:text-position="-33% 58%""#, "Pos").as_deref(), Some("2"));
    assert_eq!(c(r#"style:text-position="0% 100%""#, "Pos").as_deref(), Some("0"));
    assert_eq!(c(r##"fo:color="#00ff00""##, "Color").as_deref(), Some("#00FF00"));
    assert_eq!(c(r#"fo:letter-spacing="normal""#, "Letterspace").as_deref(), Some("0"));
    assert!(c(r#"fo:letter-spacing="1pt""#, "Letterspace").is_some_and(|v| (v.parse::<f64>().unwrap() - 1.0 / 72.0).abs() < 1e-12));
    assert_eq!(c(r#"style:text-scale="150%""#, "FontScale").as_deref(), Some("1.5"));
    assert_eq!(c(r#"style:font-name="F1""#, "Font").as_deref(), Some("Liberation Sans"), "quotes removed");
    assert_eq!(c(r#"fo:font-family="'Free Serif'""#, "Font").as_deref(), Some("Free Serif"));
    assert_eq!(c(r#"style:font-name-asian="F2""#, "AsianFont").as_deref(), Some("Noto"));
    assert_eq!(c(r#"style:font-family-complex="Arabic""#, "ComplexScriptFont").as_deref(), Some("Arabic"));
    assert!(
        c(r#"fo:font-size="200%""#, "Size").is_some_and(|v| (v.parse::<f64>().unwrap() - 2.0 * 0.1666667).abs() < 1e-9),
        "a percentage of the root's size"
    );
}

#[test]
fn odg_styles_nested_lists_headers_numbers_and_the_level_s_face() {
    let style = r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties><style:list-level-label-alignment fo:margin-left="0.5cm" fo:text-indent="-0.5cm"/></style:list-level-properties><style:text-properties style:font-name="F1" fo:font-size="10pt"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="2" text:bullet-char="◆"/><text:list-level-style-number text:level="3"/></text:list-style>"#;
    let body = concat!(
        r#"<text:list text:style-name="L1"><text:list-header><text:p>h</text:p></text:list-header>"#,
        r#"<text:list-item><text:p>a</text:p><text:list><text:list-item><text:p>b</text:p><text:list><text:list-item><text:p>c</text:p></text:list-item></text:list></text:list-item></text:list></text:list-item></text:list>"#
    );
    let (doc, report) = Drawing::a4().auto(style).fonts_named().page(&rect("none", body)).read();
    let s = only(&doc);
    let bullet = |ix: u32| row_cell(s, "Paragraph", None, ix, "Bullet").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    // Rows by first use: the header (no bullet) 0, level 1 1, level 2 2; level 3 is numbered, no cell, so the header's row again.
    assert_eq!(bullet(0), None, "a list header has no bullet");
    assert_eq!(bullet(1).as_deref(), Some("1"));
    assert_eq!(bullet(2).as_deref(), Some("2"), "a nested list without a style takes its list's");
    assert_eq!(text(row_cell(s, "Paragraph", None, 1, "BulletFont")), "Liberation Sans");
    close(num(row_cell(s, "Paragraph", None, 1, "BulletFontSize")), 10.0 / 72.0);
    close(num(row_cell(s, "Paragraph", None, 1, "IndLeft")), cm(0.5));
    assert!(row_cell(s, "Paragraph", None, 2, "IndLeft").is_none(), "level 2 gives no indent");
    assert_eq!(report.kept("numbered_list"), 1);
    let parts = &s.text.as_ref().expect("text").parts;
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "h\na\nb\nc");
    let pps: Vec<u32> = parts.iter().filter_map(|p| if let TextPart::Pp(ix) = p { Some(*ix) } else { None }).collect();
    assert_eq!(pps, [1, 2, 0], "the first paragraph's row 0 holds no cells, so it needs no marker");
}

#[test]
fn odg_styles_a_bullet_s_family_loses_its_quotes_and_a_paragraph_s_tab_distance_is_kept() {
    let (doc, report) = Drawing::a4()
        .auto(r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:text-properties fo:font-family="'Free Serif'"/></text:list-level-style-bullet></text:list-style>"#)
        .auto(r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties style:tab-stop-distance="2cm"/></style:style>"#)
        .page(&rect("none", r#"<text:list text:style-name="L1"><text:list-item><text:p text:style-name="P1">a</text:p></text:list-item></text:list>"#))
        .read();
    let s = only(&doc);
    assert_eq!(text(row_cell(s, "Paragraph", None, 0, "BulletFont")), "Free Serif");
    // A paragraph's own default tab distance has no cell of a paragraph row: kept and counted.
    assert_eq!(report.kept("paragraph_tabs"), 1);
    assert!(row_cell(s, "Paragraph", None, 0, "DefaultTabStop").is_none());
}

#[test]
fn odg_styles_a_value_64_parents_up_is_read_and_one_65_up_is_not() {
    let chain = |holder: usize| {
        let mut d = Drawing::a4().common(r#"<draw:stroke-dash draw:name="D9" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/>"#);
        for k in 0..=holder {
            let parent = if k < holder { format!(r#" style:parent-style-name="C{}""#, k + 1) } else { String::new() };
            let props = if k == holder { r#"draw:stroke-dash="D9""# } else { "" };
            d = d.common(&format!(
                r#"<style:style style:name="C{k}" style:family="graphic"{parent}><style:graphic-properties {props}/></style:style>"#
            ));
        }
        let (doc, _) = d
            .auto(&graphic("gr1", Some("C0"), r#"draw:stroke="dash" svg:stroke-width="1pt" svg:stroke-linecap="round""#))
            .page(&rect("gr1", ""))
            .read();
        text(cell(only(&doc), "LinePattern")).to_owned()
    };
    assert_eq!(chain(63), "9", "C63 is the shape style's 64th parent");
    assert_eq!(chain(64), "2", "C64 is its 65th, past the chain: the dash is not found");
}

#[test]
fn odg_styles_a_common_list_style_gives_bullets_too() {
    let (doc, _) = Drawing::a4()
        .common(r#"<text:list-style style:name="LC"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>"#)
        .page(&rect("none", r#"<text:list text:style-name="LC"><text:list-item><text:p>a</text:p></text:list-item></text:list>"#))
        .read();
    assert_eq!(text(row_cell(only(&doc), "Paragraph", None, 0, "Bullet")), "1");
}

#[test]
fn odg_styles_families_names_and_tab_stops() {
    let (doc, report) = Drawing::a4()
        .common(concat!(
            r#"<style:style style:name="T" style:family="table"/>"#,
            r#"<style:style style:family="graphic"/>"#,
            r##"<style:style style:name="Same" style:family="graphic"><style:graphic-properties draw:fill-color="#111111"/></style:style>"##,
            r##"<style:style style:name="Same" style:family="graphic"><style:graphic-properties draw:fill-color="#222222"/></style:style>"##,
            r#"<style:style style:name="Same" style:family="paragraph"/>"#,
            r#"<style:default-style style:family="paragraph"><style:paragraph-properties style:tab-stop-distance="1.25cm"><style:tab-stops><style:tab-stop style:position="1cm"/><style:tab-stop style:position="2cm" style:type="center"/><style:tab-stop style:position="3cm" style:type="right"/><style:tab-stop style:position="4cm" style:type="char"/><style:tab-stop style:type="left"/></style:tab-stops></style:paragraph-properties><style:text-properties fo:font-size="10pt"/></style:default-style>"#,
            r#"<style:style style:name="Big" style:family="paragraph"><style:text-properties fo:font-size="150%"/></style:style>"#,
            r#"<style:style style:name="Bigger" style:family="paragraph" style:parent-style-name="Big"><style:text-properties fo:font-size="200%"/></style:style>"#
        ))
        .auto(r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Same"/>"##)
        .page(&format!("{}{}", rect("gr1", ""), r#"<draw:rect draw:text-style-name="Bigger" svg:width="1cm" svg:height="1cm"><text:p>x</text:p></draw:rect>"#))
        .read();
    assert_eq!(report.kept("style_other_family"), 1);
    assert_eq!(report.kept("style_without_name"), 1);
    assert_eq!(report.warned("style_name_repeated"), 1, "two graphic styles of one name; the paragraph one is another family");
    let same: Vec<&diacraft_doc::Style> = doc.styles.iter().map(|s| s.as_ref()).filter(|s| s.name_u.as_deref() == Some("Same")).collect();
    assert_eq!(same.len(), 3, "each is a style sheet");
    // Only common graphic and paragraph styles are style sheets: not the table style, nor the shape's automatic one.
    assert!(doc.styles.iter().all(|s| !matches!(s.name_u.as_deref(), Some("T" | "gr1"))));
    // Nor an automatic graphic style of styles.xml.
    let mut d = Drawing::a4().page("");
    d.styles_auto.push_str(r#"<style:style style:name="sa" style:family="graphic"><style:graphic-properties draw:fill="none"/></style:style>"#);
    assert_eq!(d.read().0.styles.len(), 1, "the root alone");
    assert_eq!(only(&doc).sheet.links.fill, Some(same[0].id), "the first of one name is the one found");
    let r = root(&doc);
    let at = |name: &str| cell_at(&r.sheet, &diacraft_doc::CellAddr::in_row("Tabs", None, diacraft_doc::RowKey::Ix(0), name));
    close(num(at("Position0")), cm(1.0));
    for (j, alignment) in [(0, 0.0), (1, 1.0), (2, 2.0), (3, 3.0)] {
        close(num(at(&format!("Alignment{j}"))), alignment);
    }
    assert!(at("Position4").is_none(), "a stop with no position is left out");
    assert_eq!(report.kept("tab_stop"), 1);
    close(num(r.sheet.cell("DefaultTabStop")), cm(1.25));
    // Sizes as percentages multiply down the chain from the default's 10 pt: 150% then 200%, 30 pt.
    let bigger = sheet(&doc, "Bigger");
    let size = cell_at(&bigger.sheet, &diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "Size"));
    close(num(size), 30.0 / 72.0);
}

#[test]
fn odg_styles_a_child_that_sets_only_the_second_property_of_a_cell() {
    let parent = |props: &str| {
        Drawing::a4()
            .common(r##"<draw:gradient draw:name="G1" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff"/><draw:gradient draw:name="G3" draw:style="radial" draw:start-color="#ff0000" draw:end-color="#0000ff"/><draw:stroke-dash draw:name="D9" draw:dots1="1" draw:dots1-length="300%" draw:distance="300%"/><draw:marker draw:name="DiaCraft_20_end_20_4"/>"##)
            .common(&graphic("P", None, r#"draw:fill="gradient" draw:fill-gradient-name="G1" draw:stroke="dash" svg:stroke-width="1pt" draw:marker-end="DiaCraft_20_end_20_4" draw:marker-end-width="7.04pt""#))
            .auto(&graphic("gr1", Some("P"), props))
            .page(&rect("gr1", ""))
            .read()
    };
    // Only the gradient's name: the fill is worked out again, a radial one (with no centre, from the top-left corner).
    let (doc, _) = parent(r#"draw:fill-gradient-name="G3""#);
    assert_eq!(text(cell(only(&doc), "FillGradientDir")), "7");
    // Only the dash: the pattern is worked out again.
    let (doc, _) = parent(r#"draw:stroke-dash="D9""#);
    assert_eq!(text(cell(only(&doc), "LinePattern")), "9");
    // Only the marker's width: the end's size class is worked out again (number 4 spreads 2 S, and S is the class's base plus the 1 pt
    // weight, so 7.04 pt is class 2, as the parent has it).
    let (doc, _) = parent(r#"draw:marker-end-width="7.04pt""#);
    assert!(cell(only(&doc), "EndArrowSize").is_none(), "equal to what the parent gives: not local");
    let (doc, _) = parent(r#"draw:marker-end-width="5.6pt""#);
    assert_eq!(text(cell(only(&doc), "EndArrowSize")), "1", "5.6 pt is 2 times (1.8 + 1): class 1");
}

#[test]
fn odg_styles_three_mark_dashes() {
    let pattern = |dash: &str| text(cell(only(&dash_drawing(dash, "round").read().0), "LinePattern")).to_owned();
    assert_eq!(pattern(r#"draw:dots1="2" draw:dots1-length="700%" draw:dots2="1" draw:dots2-length="0%" draw:distance="500%""#), "6");
    assert_eq!(pattern(r#"draw:dots1="1" draw:dots1-length="700%" draw:dots2="2" draw:dots2-length="0%" draw:distance="500%""#), "5");
    assert_eq!(pattern(r#"draw:dots1="1" draw:dots1-length="0%" draw:distance="500%""#), "3", "a dot");
    assert_eq!(pattern(r#"draw:dots1="4" draw:dots1-length="700%" draw:distance="500%""#), "2", "more marks than any pattern has");
    assert_eq!(pattern(r#"draw:distance="500%""#), "2", "no marks");
}

#[test]
fn odg_styles_line_end_spreads_by_number() {
    // The spread for each number, at class 3 (base 3.24 pt) and a 0.75 pt line: the width is the spread times 3.99 pt.
    let d = |n: u32, width_pt: f64| {
        let marker = format!("DiaCraft_20_end_20_{n}");
        Drawing::a4()
            .common(&format!(r#"<draw:marker draw:name="{marker}" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#))
            .auto(&graphic(
                "gr1",
                None,
                &format!(r#"svg:stroke-width="0.75pt" draw:marker-start="{marker}" draw:marker-start-width="{width_pt:.4}pt""#),
            ))
            .page(&rect("gr1", ""))
            .read()
    };
    // The measured ones and the ones DiaCraft's rule gives a box: 8 and 19 are 4's box, 18 is 6's, 34 and 38 are 22's.
    for (n, spread) in [
        (13, 2.0),
        (5, 1.99),
        (6, 2.02),
        (7, 1.92),
        (10, 1.5),
        (33, 1.5),
        (22, 2.24),
        (8, 2.0),
        (19, 2.0),
        (18, 2.02),
        (31, 1.5),
        (34, 2.24),
        (38, 2.24),
        (40, 2.0),
        (44, 2.0),
        (45, 2.0),
    ] {
        let (doc, report) = d(n, spread * 3.99);
        close(num(cell(only(&doc), "BeginArrow")), f64::from(n));
        close(num(cell(only(&doc), "BeginArrowSize")), 3.0);
        assert_eq!(report.kept("marker_width"), 0, "number {n}");
    }
    // A width no class matches: the size stands in as 2, and the width is kept.
    let (doc, report) = d(8, 2.0 * 3.99 + 0.5);
    close(num(cell(only(&doc), "BeginArrow")), 8.0);
    close(num(cell(only(&doc), "BeginArrowSize")), 2.0);
    assert_eq!(report.kept("marker_width"), 1);
    // The line's weight counts in points: at 2 pt, 2 times (3.24 + 2) is class 3 too.
    let (doc, _) = Drawing::a4()
        .common(r#"<draw:marker draw:name="DiaCraft_20_end_20_13" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#)
        .auto(&graphic("gr1", None, r#"svg:stroke-width="2pt" draw:marker-start="DiaCraft_20_end_20_13" draw:marker-start-width="10.48pt""#))
        .page(&rect("gr1", ""))
        .read();
    close(num(cell(only(&doc), "BeginArrowSize")), 3.0);
}

#[test]
fn odg_styles_a_dash_of_second_marks_only() {
    let pattern = |dash: &str| text(cell(only(&dash_drawing(dash, "round").read().0), "LinePattern")).to_owned();
    assert_eq!(pattern(r#"draw:dots2="1" draw:dots2-length="300%" draw:distance="300%""#), "9");
}

#[test]
fn odg_reader_defaults_gradient_stops_and_tab_stops_equal_to_the_links_are_not_local() {
    let stops = r##"<draw:gradient draw:name="G1" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:border="20%"/><draw:gradient draw:name="G4" draw:style="linear" draw:start-color="#00ff00" draw:end-color="#0000ff" draw:border="20%"/><draw:gradient draw:name="G5" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:border="40%"/>"##;
    let tabs = |at: &str| {
        format!(
            r#"<style:paragraph-properties><style:tab-stops><style:tab-stop style:position="{at}"/></style:tab-stops></style:paragraph-properties>"#
        )
    };
    let d = |gradient: &str, tab_at: &str| {
        Drawing::a4()
            .common(stops)
            .common(&format!(
                r#"<style:style style:name="P" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="G1"/>{}</style:style>"#,
                tabs("1cm")
            ))
            .auto(&format!(
                r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="P"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="{gradient}"/>{}</style:style>"#,
                tabs(tab_at)
            ))
            .page(&rect("gr1", ""))
            .read()
            .0
    };
    let colour = |doc: &Document, ix: u32| row_cell(only(doc), "FillGradient", None, ix, "GradientStopColor").map(|c| text(Some(c)).to_owned());
    let position = |doc: &Document, ix: u32| row_cell(only(doc), "FillGradient", None, ix, "GradientStopPosition").map(|c| text(Some(c)).to_owned());
    let tab = |doc: &Document| row_cell(only(doc), "Tabs", None, 0, "Position0").map(|c| num(Some(c)));
    // The same gradient and the same stop as the parent's: nothing local.
    let same = d("G1", "1cm");
    assert_eq!(colour(&same, 0), None);
    assert_eq!(tab(&same), None);
    // Stops of the same count with other colours, or with the same colours at other places, are local.
    let other_colours = d("G4", "1cm");
    assert_eq!(colour(&other_colours, 0).as_deref(), Some("#00FF00"));
    let other_places = d("G5", "1cm");
    assert_eq!(position(&other_places, 1).as_deref(), Some("0.4"));
    // A stop elsewhere is local.
    let moved = d("G1", "3cm");
    close(tab(&moved).expect("a local stop"), cm(3.0));
    assert_eq!(colour(&moved, 0), None);
}

#[test]
fn odg_styles_a_line_end_on_a_line_of_no_stated_weight_takes_the_root_s() {
    // No style states a width, so the line is the root's 0.01 inch, 0.72 pt: number 13 spreads 2 S, and 2 times (1.44 + 0.72) is
    // class 0's 4.32 pt.
    let (doc, report) = Drawing::a4()
        .common(r#"<draw:marker draw:name="DiaCraft_20_end_20_13" svg:viewBox="0 0 10 10" svg:d="M0 0L10 0 5 10Z"/>"#)
        .auto(&graphic("gr1", None, r#"draw:marker-end="DiaCraft_20_end_20_13" draw:marker-end-width="4.32pt""#))
        .page(&rect("gr1", ""))
        .read();
    close(num(cell(only(&doc), "EndArrowSize")), 0.0);
    assert_eq!(report.kept("marker_width"), 0);
}

/// The names of the cells of row `ix` of `section`, sorted.
fn row_names(doc: &Document, section: &str, ix: u32) -> Vec<String> {
    let s = only(doc);
    let Some(sec) = s.sheet.section(&diacraft_doc::SectionKey::new(section, None)) else { return Vec::new() };
    let mut names: Vec<String> =
        sec.rows.iter().find(|r| r.ix == Some(ix)).map(|r| r.cells.keys().map(|k| k.as_str().to_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn odg_styles_a_row_past_row_0_holds_every_cell_its_chain_resolves() {
    // The review's bold span under an 18 pt red graphic style. Row 1 inherits by `IX`, through a style row that does
    // not exist, so it holds the size and the colour as well as the weight.
    let (doc, _) = Drawing::a4()
        .auto(
            r##"<style:style style:name="gr1" style:family="graphic"><style:text-properties fo:font-size="18pt" fo:color="#ff0000"/></style:style>"##,
        )
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&rect("gr1", r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p>"#))
        .read();
    let s = only(&doc);
    close(num(row_cell(s, "Character", None, 1, "Size")), 18.0 / 72.0);
    assert_eq!(text(row_cell(s, "Character", None, 1, "Color")), "#FF0000");
    close(num(row_cell(s, "Character", None, 1, "Style")), 1.0);
    // Row 0 is the shape's own: its size and colour, local to the shape, and no weight.
    close(num(row_cell(s, "Character", None, 0, "Size")), 18.0 / 72.0);
    assert!(row_cell(s, "Character", None, 0, "Style").is_none());
    // A run whose chain resolves what row 0's does shares it.
    let (doc, _) = Drawing::a4()
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&rect("none", r#"<text:p><text:span text:style-name="T1">a</text:span>b<text:span text:style-name="T1">c</text:span></text:p>"#))
        .read();
    let parts = &only(&doc).text.as_ref().expect("text").parts;
    let cps: Vec<u32> = parts.iter().filter_map(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect();
    assert_eq!(cps, [0, 1, 0]);
}

#[test]
fn odg_styles_row_1_holds_the_size_and_the_weight() {
    // The `cd`: bold over a paragraph that sets no character property, so its row holds the weight and the size the chain gives (the
    // root's 12 pt), and nothing else.
    let (doc, _) = Drawing::a4()
        .auto(&paragraph_style("P1", r#"fo:text-align="center""#, ""))
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&rect(
            "none",
            r#"<text:p text:style-name="P1">ab<text:span text:style-name="T1">cd</text:span></text:p><text:p text:style-name="P1">ef</text:p>"#,
        ))
        .read();
    assert_eq!(row_names(&doc, "Character", 1), ["Size", "Style"]);
    close(num(row_cell(only(&doc), "Character", None, 1, "Size")), 0.1666667);
    close(num(row_cell(only(&doc), "Character", None, 1, "Style")), 1.0);
    assert!(row_names(&doc, "Character", 0).is_empty(), "row 0 holds no cells of its own");
}

#[test]
fn odg_styles_marks_in_text_are_counted() {
    let (doc, report) = Drawing::a4()
        .page(&rect(
            "none",
            r#"<text:p>a<text:bookmark text:name="m"/>b<text:soft-page-break/>c<text:reference-mark-start text:name="r"/>d</text:p>"#,
        ))
        .read();
    let parts = &only(&doc).text.as_ref().expect("text").parts;
    let flat: String = parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(c.as_ref()) } else { None }).collect();
    assert_eq!(flat, "abcd");
    assert_eq!(report.kept("text_mark"), 3);
}

#[test]
fn odg_styles_tab_stops_past_a_row_s_cells_are_refused_before_they_are_stored() {
    // Two cells a stop, in one row of at most 4,096 cells: 2,048 stops are read, and 2,049 refused by the reader, not by the model.
    let stops = |n: usize| {
        let list: String = (0..n).map(|k| format!(r#"<style:tab-stop style:position="{}mm"/>"#, k + 1)).collect();
        let style = format!(
            r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties><style:tab-stops>{list}</style:tab-stops></style:paragraph-properties></style:style>"#
        );
        let d = Drawing::a4()
            .auto(&style)
            .page(r#"<draw:rect draw:text-style-name="P1" svg:width="1cm" svg:height="1cm"><text:p>a</text:p></draw:rect>"#);
        diacraft_odg::read(&d.bytes(), &diacraft_odg::Limits::default()).err().map(|e| e.class())
    };
    assert_eq!(stops(diacraft_doc::MAX_CELLS_PER_CONTAINER / 2), None);
    assert_eq!(stops(diacraft_doc::MAX_CELLS_PER_CONTAINER / 2 + 1), Some("xml_too_complex"));
}

#[test]
fn odg_styles_a_row_past_row_0_holds_what_the_default_paragraph_style_gives() {
    // R3 ends a run's chain at the default paragraph style, so its colour is among the cells row 1 holds; the default graphic style's
    // fill is no text property and is not.
    let (doc, _) = Drawing::a4()
        .common(r##"<style:default-style style:family="paragraph"><style:text-properties fo:color="#00ff00"/></style:default-style>"##)
        .common(r##"<style:default-style style:family="graphic"><style:graphic-properties draw:fill-color="#0000ff"/></style:default-style>"##)
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&rect("none", r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p>"#))
        .read();
    let s = only(&doc);
    assert_eq!(text(row_cell(s, "Character", None, 1, "Color")), "#00FF00");
    close(num(row_cell(s, "Character", None, 1, "Style")), 1.0);
    assert_eq!(row_names(&doc, "Character", 1), ["Color", "Size", "Style"]);
}

/// A list style `name` whose level 1 is a bullet of `bullet`, with `props` inside the level.
fn bullet_style(name: &str, bullet: &str, props: &str) -> String {
    format!(
        r#"<text:list-style style:name="{name}"><text:list-level-style-bullet text:level="1" text:bullet-char="{bullet}">{props}</text:list-level-style-bullet></text:list-style>"#
    )
}

/// A list on the style `name` of one item with one paragraph.
fn one_item(name: &str) -> String {
    format!(r#"<text:list text:style-name="{name}"><text:list-item><text:p>x</text:p></text:list-item></text:list>"#)
}

/// A cell of a `Paragraph` row of the shape, as text.
fn paragraph_text(doc: &Document, ix: u32, name: &str) -> Option<String> {
    row_cell(only(doc), "Paragraph", None, ix, name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

#[test]
fn odg_styles_two_lists_whose_bullets_have_equal_lengths_each_show_their_own() {
    // 's MEDIUM: a text was found again by where it was held, and a list's bullet was held in a copy freed when
    // its list ended, so the next list's copy of the same length took its place and showed the first list's bullet. Each pair is two
    // bullets of equal byte length (two of 1 byte, two of 2, two of 6 as two three-byte characters, two of 4), and a pair of unequal ones
    // is read as before. The first list's paragraph is row 0 and the second's row 1.
    for (first, second) in
        [("a", "b"), ("ab", "cd"), ("\u{2192}\u{2192}", "\u{21D2}\u{21D2}"), ("\u{1F600}", "\u{1F601}"), ("ab", "cde"), ("abc", "d")]
    {
        let (doc, _) = Drawing::a4()
            .auto(&format!("{}{}", bullet_style("A", first, ""), bullet_style("B", second, "")))
            .page(&rect("none", &format!("{}{}", one_item("A"), one_item("B"))))
            .read();
        assert_eq!(paragraph_text(&doc, 0, "BulletStr").as_deref(), Some(first), "the first list's bullet, {first} then {second}");
        assert_eq!(paragraph_text(&doc, 1, "BulletStr").as_deref(), Some(second), "the second list's bullet, {first} then {second}");
    }
}

#[test]
fn odg_styles_two_lists_whose_faces_have_equal_lengths_each_show_their_own() {
    // The same, for the face a level names in `fo:font-family`: two list styles whose faces have equal byte lengths, from 5 to 200 (the
    // old copies fell in different allocator bins by length), with the bullet's characters one of the table's or not.
    let face = |family: &str| format!(r#"<style:text-properties fo:font-family="{family}"/>"#);
    for length in [5, 24, 29, 64, 200] {
        for bullet in ["\u{2022}", "ab"] {
            let (alpha, bravo) = ("a".repeat(length), "b".repeat(length));
            let (doc, _) = Drawing::a4()
                .auto(&format!("{}{}", bullet_style("A", bullet, &face(&alpha)), bullet_style("B", bullet, &face(&bravo))))
                .page(&rect("none", &format!("{}{}", one_item("A"), one_item("B"))))
                .read();
            assert_eq!(paragraph_text(&doc, 0, "BulletFont"), Some(alpha), "the first list's face, {length} bytes, bullet {bullet}");
            assert_eq!(paragraph_text(&doc, 1, "BulletFont"), Some(bravo), "the second list's face, {length} bytes, bullet {bullet}");
        }
    }
    // A face named by `style:font-name`, whose family the font declarations hold.
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="F1" svg:font-family="Alpha"/><style:font-face style:name="F2" svg:font-family="Bravo"/>"#.to_owned();
    let named = |name: &str| format!(r#"<style:text-properties style:font-name="{name}"/>"#);
    let (doc, _) = d
        .auto(&format!("{}{}", bullet_style("A", "\u{2022}", &named("F1")), bullet_style("B", "\u{2022}", &named("F2"))))
        .page(&rect("none", &format!("{}{}", one_item("A"), one_item("B"))))
        .read();
    assert_eq!(paragraph_text(&doc, 0, "BulletFont").as_deref(), Some("Alpha"));
    assert_eq!(paragraph_text(&doc, 1, "BulletFont").as_deref(), Some("Bravo"));
}

#[test]
fn odg_styles_three_lists_in_turn_and_a_run_of_text_after_them_keep_their_own_faces() {
    // Three lists of three styles with faces and bullets of one length each, then a span whose family has that length too: nothing takes
    // another's value, and the span's face is its own.
    let props = |family: &str| format!(r#"<style:text-properties fo:font-family="{family}"/>"#);
    let styles = format!(
        "{}{}{}{}",
        bullet_style("A", "ab", &props("Alpha")),
        bullet_style("B", "cd", &props("Bravo")),
        bullet_style("C", "ef", &props("Delta")),
        r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-family="Gamma"/></style:style>"#
    );
    let body = format!("{}{}{}<text:p>y<text:span text:style-name=\"T1\">z</text:span></text:p>", one_item("A"), one_item("B"), one_item("C"));
    let (doc, _) = Drawing::a4().auto(&styles).page(&rect("none", &body)).read();
    for (ix, (bullet, face)) in [("ab", "Alpha"), ("cd", "Bravo"), ("ef", "Delta")].into_iter().enumerate() {
        let ix = u32::try_from(ix).unwrap();
        assert_eq!(paragraph_text(&doc, ix, "BulletStr").as_deref(), Some(bullet));
        assert_eq!(paragraph_text(&doc, ix, "BulletFont").as_deref(), Some(face));
    }
    let span_face =
        (0..8).find_map(|ix| row_cell(only(&doc), "Character", None, ix, "Font").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()));
    assert_eq!(span_face.as_deref(), Some("Gamma"));
}

#[test]
fn odg_styles_a_nested_list_without_a_style_reads_the_style_of_the_list_around_it() {
    // Three lists nested: the outermost names L, the next names M, the innermost names none and reads M's level 3 (the list around it),
    // not L's. Each level's bullet is a text of its own, so the paragraph shows which style and level it read.
    let levels = |prefix: &str| -> String {
        (1..=3).map(|k| format!(r#"<text:list-level-style-bullet text:level="{k}" text:bullet-char="{prefix}{k}x"/>"#)).collect()
    };
    let styles = format!(
        r#"<text:list-style style:name="L">{}</text:list-style><text:list-style style:name="M">{}</text:list-style>"#,
        levels("l"),
        levels("m")
    );
    let body = r#"<text:list text:style-name="L"><text:list-item><text:p>1</text:p><text:list text:style-name="M"><text:list-item><text:p>2</text:p><text:list><text:list-item><text:p>3</text:p></text:list-item></text:list></text:list-item></text:list></text:list-item></text:list>"#;
    let (doc, _) = Drawing::a4().auto(&styles).page(&rect("none", body)).read();
    assert_eq!(paragraph_text(&doc, 0, "BulletStr").as_deref(), Some("l1x"));
    assert_eq!(paragraph_text(&doc, 1, "BulletStr").as_deref(), Some("m2x"));
    assert_eq!(paragraph_text(&doc, 2, "BulletStr").as_deref(), Some("m3x"));
}

#[test]
fn odg_styles_a_list_finds_its_level_by_number_and_the_first_written_of_a_number_wins() {
    // Levels written out of order (3, 1, 2) and level 1 twice: a list at each depth reads the level of its number, level 1 the first
    // written, and a list deeper than the style's levels reads none and has no bullet.
    let level = |number: u32, bullet: &str| format!(r#"<text:list-level-style-bullet text:level="{number}" text:bullet-char="{bullet}"/>"#);
    let style =
        format!(r#"<text:list-style style:name="L">{}{}{}{}</text:list-style>"#, level(3, "c3x"), level(1, "a1x"), level(2, "b2x"), level(1, "a1y"));
    let mut body = String::new();
    for depth in 1..=4 {
        body.push_str(&format!(r#"<text:list{}><text:list-item><text:p>{depth}</text:p>"#, if depth == 1 { r#" text:style-name="L""# } else { "" }));
    }
    for _ in 1..=4 {
        body.push_str("</text:list-item></text:list>");
    }
    let (doc, _) = Drawing::a4().auto(&style).page(&rect("none", &body)).read();
    assert_eq!(paragraph_text(&doc, 0, "BulletStr").as_deref(), Some("a1x"));
    assert_eq!(paragraph_text(&doc, 1, "BulletStr").as_deref(), Some("b2x"));
    assert_eq!(paragraph_text(&doc, 2, "BulletStr").as_deref(), Some("c3x"));
    assert!(row_cell(only(&doc), "Paragraph", None, 3, "Bullet").is_none(), "no level 4: no bullet");
    assert!(row_cell(only(&doc), "Paragraph", None, 3, "BulletStr").is_none());
}

#[test]
fn odg_styles_a_size_keeps_the_unit_it_was_written_in() {
    // The `Size` cell's `U` is the length's own unit, none for `px`; a percentage of the parent's size is in points.
    let unit_of = |size: &str| {
        let (doc, _) = styled("", "", &format!(r#"fo:font-size="{size}""#));
        unit(row_cell(only(&doc), "Character", None, 0, "Size")).map(str::to_owned)
    };
    assert_eq!(unit_of("0.5cm").as_deref(), Some("CM"));
    assert_eq!(unit_of("5mm").as_deref(), Some("MM"));
    assert_eq!(unit_of("12pt").as_deref(), Some("PT"));
    assert_eq!(unit_of("12px"), None);
    assert_eq!(unit_of("150%").as_deref(), Some("PT"), "a percentage of the parent's size: points");
}
