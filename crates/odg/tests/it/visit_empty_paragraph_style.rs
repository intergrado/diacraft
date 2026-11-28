//! Found by the gate of 0.2.3, a side effect: a held paragraph style asked for again by a paragraph whose shape's graphic style already gives every property the style
//! states is written as **a new automatic paragraph style with an empty `style:paragraph-properties` element**. Nothing drawn changes, but an automatic style that says nothing is noise a drawing
//! program lists, and builds before a change wrote that paragraph with no style.
//!
//! The drawing is built in code from the ODF 1.4 Part 3 vocabulary (the paragraph properties §16.30 and `fo:text-align` §20.223, the graphic styles §16.2 and the connector §10.4.13): an ODF 1.2
//! package (and the flat drawing of it), the rectangles `n1` and `n2` and the connector between them. A common graphic style `Wire` gives `fo:text-align="center"`, and `gr2` (the connector's) is
//! its child, so the text of the connector is centred by its shape's style where the text of `n1` is centred by the paragraph style `P2`, which the connector's paragraph names as well.

use diacraft_doc::{Document, SectionKey};
use diacraft_odg::{Limits, read};

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::visit_styles::{part, visit};

/// The common graphic style the connector's is a child of: it aligns the text of its shapes as `align` says.
fn wire(align: &str) -> String {
    format!(r#"<style:style style:name="Wire" style:family="graphic"><style:paragraph-properties fo:text-align="{align}"/></style:style>"#)
}

/// The automatic graphic styles: `gr1` states no paragraph property, `gr2` is the child of `Wire`.
const GRAPHIC: &str = concat!(
    r#"<style:style style:name="gr1" style:family="graphic"/>"#,
    r#"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="Wire"/>"#
);

/// The paragraph style the paragraphs name: `properties` are the attributes of its paragraph properties.
fn p2(properties: &str) -> String {
    format!(r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties {properties}/></style:style>"#)
}

/// Rectangle `n1`, holding a paragraph of `P2` when `text` is.
fn n1(text: bool) -> String {
    let body = if text { r#"<text:p text:style-name="P2">Berth one</text:p>"# } else { "" };
    format!(
        r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="n1" xml:id="n1" draw:id="n1" svg:x="2cm" svg:y="3cm" svg:width="5cm" svg:height="2cm">{body}</draw:rect>"#
    )
}

/// Rectangle `n2`, with no text.
const N2: &str = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="n2" xml:id="n2" draw:id="n2" svg:x="11cm" svg:y="3cm" svg:width="5cm" svg:height="2cm"/>"#;

/// The connector from `n1` to `n2` on `gr2`, holding a paragraph of `P2` with the words `words`.
fn tow_line(name: &str, words: &str) -> String {
    format!(
        concat!(
            r#"<draw:connector draw:style-name="gr2" draw:layer="layout" draw:name="{}" draw:start-shape="n1" draw:start-glue-point="3" draw:end-shape="n2" draw:end-glue-point="1" "#,
            r#"svg:x1="7cm" svg:y1="4cm" svg:x2="11cm" svg:y2="4cm" svg:d="M 7000 4000 L 11000 4000" svg:viewBox="0 0 11001 4001">"#,
            r#"<text:p text:style-name="P2">{}</text:p></draw:connector>"#
        ),
        name, words
    )
}

/// A drawing (ODF 1.2) of `Wire` with the alignment `wire_align`, `gr1`, `gr2` and `P2` with the paragraph properties `properties`, holding `shapes`.
fn drawing(wire_align: &str, properties: &str, shapes: &str) -> Drawing {
    let mut d = Drawing::a4().common(&wire(wire_align)).auto(GRAPHIC).auto(&p2(properties)).page(shapes);
    d.version = "1.2".to_owned();
    d
}

/// **The vector**: `n1`, `n2`, then the connector, `Wire` centred and `P2` aligned as `align` says, `n1` holding text when `text` is; the connector comes first when `connector_first` is.
fn vector(align: &str, text: bool, connector_first: bool) -> Drawing {
    let connector = tow_line("c", "Tow line");
    let shapes = if connector_first { format!("{connector}{}{N2}", n1(text)) } else { format!("{}{N2}{connector}", n1(text)) };
    drawing("center", &format!(r#"fo:text-align="{align}""#), &shapes)
}

/// The value of the attribute `name` in the tag `tag`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_owned())
}

/// The style each `text:p` of `content` names, in the order of the file.
fn paragraph_styles(content: &str) -> Vec<Option<String>> {
    content
        .split("<text:p")
        .skip(1)
        .filter(|rest| rest.starts_with([' ', '>', '/']))
        .map(|rest| attr(&rest[..rest.find('>').unwrap()], "text:style-name"))
        .collect()
}

/// The automatic paragraph styles of `content` as `(name, element)`: the whole element of each, from its tag to its end.
fn automatic_paragraph_styles(content: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (at, _) in content.match_indices("<style:style ") {
        let rest = &content[at..];
        let tag = &rest[..rest.find('>').unwrap()];
        if attr(tag, "style:family").as_deref() != Some("paragraph") {
            continue;
        }
        let end = if tag.ends_with('/') { tag.len() + 1 } else { rest.find("</style:style>").unwrap() + "</style:style>".len() };
        found.push((attr(tag, "style:name").unwrap(), rest[..end].to_owned()));
    }
    found
}

/// The names of the automatic paragraph styles of `content`, in the order of the file.
fn style_names(content: &str) -> Vec<String> {
    automatic_paragraph_styles(content).into_iter().map(|(name, _)| name).collect()
}

/// The value of attribute `attribute` the automatic paragraph style `name` of `content` states, or what is wrong with the style.
fn stated(content: &str, name: &str, attribute: &str) -> Result<String, String> {
    let styles = automatic_paragraph_styles(content);
    let (_, element) = styles.iter().find(|(n, _)| n == name).ok_or_else(|| format!("no automatic paragraph style {name}: {content}"))?;
    attr(element, attribute).ok_or_else(|| format!("paragraph style {name} states no {attribute}: {element}"))
}

/// The automatic paragraph styles of `content` that say nothing: none holds an attribute on any of its properties elements.
fn empty_styles(content: &str) -> Vec<String> {
    automatic_paragraph_styles(content)
        .into_iter()
        .filter(|(_, element)| {
            // A properties element states something when its tag holds an attribute: `<style:paragraph-properties fo:text-align=".."/>`.
            let says = |tag: &str| element.find(tag).is_some_and(|at| element[at + tag.len()..].starts_with(' '));
            !(says("<style:paragraph-properties") || says("<style:text-properties"))
        })
        .map(|(name, element)| format!("{name}: {element}"))
        .collect()
}

/// The visit of `source`: the styles the paragraphs name, in the order of the file, with the content written.
fn written_by(source: &[u8]) -> (Vec<Option<String>>, String) {
    let (written, _) = visit(source).unwrap();
    let content = part(&written, "content.xml");
    (paragraph_styles(&content), content)
}

/// **The vector, red before the fix** (package): the connector's paragraph names a new style `P3` whose paragraph properties element has no attribute. It is to name `P2` or none: nothing is drawn
/// differently, and no style says nothing.
#[test]
fn a_connector_paragraph_whose_graphic_style_states_the_alignment_names_the_held_style_or_none() {
    let (styles, content) = written_by(&vector("center", true, false).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert_eq!(styles.first(), Some(&Some("P2".to_owned())), "n1's paragraph names the held style: {content}");
    let connector = styles.get(1).unwrap();
    assert!(matches!(connector.as_deref(), None | Some("P2")), "the connector's paragraph names {connector:?}: {content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "an automatic paragraph style that says nothing: {content}");
}

/// The visit adds no paragraph style to the one the file holds, and the one it holds is still written as the file had it.
#[test]
fn the_visit_adds_no_paragraph_style_to_the_one_the_file_holds() {
    let (_, content) = written_by(&vector("center", true, false).bytes());
    assert_eq!(style_names(&content), ["P2"], "{content}");
    assert_eq!(stated(&content, "P2", "fo:text-align"), Ok("center".to_owned()), "{content}");
}

/// A flat drawing is read as the package that holds the same elements, so its visit is the same.
#[test]
fn a_flat_visit_writes_no_style_that_says_nothing() {
    let (styles, content) = written_by(&Flat::of(&vector("center", true, false)).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert!(matches!(styles.get(1).and_then(Option::as_deref), None | Some("P2")), "{styles:?}: {content}");
    assert_eq!(style_names(&content), ["P2"], "{content}");
}

/// The connector first: its paragraph is the one that asks for `P2` first, and no new style is written either way.
#[test]
fn the_connector_first_writes_no_style_that_says_nothing() {
    let (styles, content) = written_by(&vector("center", true, true).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    assert_eq!(style_names(&content), ["P2"], "{content}");
}

/// No paragraph in `n1`: the connector's paragraph is the only one that names `P2`, and no style that says nothing is written.
#[test]
fn a_connector_alone_writes_no_style_that_says_nothing() {
    let (styles, content) = written_by(&vector("center", false, false).bytes());
    assert_eq!(styles.len(), 1, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    assert!(matches!(styles.first().and_then(Option::as_deref), None | Some("P2")), "{styles:?}: {content}");
}

/// **However many paragraphs ask**: a connector before `n1`'s paragraph and one after it, the ask that comes before the style was made and the one that comes after. No style says nothing, and
/// each connector names `P2` or none.
#[test]
fn every_connector_paragraph_names_the_held_style_or_none() {
    let shapes = format!("{}{}{N2}{}", tow_line("before", "First line"), n1(true), tow_line("after", "Second line"));
    let (styles, content) = written_by(&drawing("center", r#"fo:text-align="center""#, &shapes).bytes());
    assert_eq!(styles.len(), 3, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    for (k, name) in styles.iter().enumerate() {
        assert!(matches!(name.as_deref(), None | Some("P2")), "paragraph {k} names {name:?}: {content}");
    }
    assert_eq!(style_names(&content), ["P2"], "{content}");
}

/// **The control that keeps the file's value**: `Wire` and `P2` both state `end`, which `P2` states as the file spelled it (`start` and `end` are values the cells have no place for): the connector's
/// paragraph names a style that states `end` and is not left to say nothing. Both paragraphs say what the file said, and no style says nothing.
#[test]
fn an_alignment_the_file_stated_in_its_own_words_is_kept() {
    let shapes = format!("{}{N2}{}", n1(true), tow_line("c", "Tow line"));
    let (styles, content) = written_by(&drawing("end", r#"fo:text-align="end""#, &shapes).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    for (k, name) in styles.iter().enumerate() {
        let name = name.as_deref().unwrap_or_else(|| panic!("paragraph {k} names no style, and the file stated `end` for it: {content}"));
        assert_eq!(stated(&content, name, "fo:text-align"), Ok("end".to_owned()), "paragraph {k}: {content}");
    }
}

/// The same control with the connector first (the first ask is the one that has the held value alone, the second the one that has a cell besides).
#[test]
fn an_alignment_the_file_stated_in_its_own_words_is_kept_with_the_connector_first() {
    let shapes = format!("{}{}{N2}", tow_line("c", "Tow line"), n1(true));
    let (styles, content) = written_by(&drawing("end", r#"fo:text-align="end""#, &shapes).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    for (k, name) in styles.iter().enumerate() {
        let name = name.as_deref().unwrap_or_else(|| panic!("paragraph {k} names no style, and the file stated `end` for it: {content}"));
        assert_eq!(stated(&content, name, "fo:text-align"), Ok("end".to_owned()), "paragraph {k}: {content}");
    }
}

/// `end` for `P2` while `Wire` is centred: the style states what the graphic style does not, so both paragraphs name a style that says `end`.
#[test]
fn an_alignment_the_graphic_style_does_not_give_is_kept() {
    let (styles, content) = written_by(&vector("end", true, false).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    for (k, name) in styles.iter().enumerate() {
        let name = name.as_deref().unwrap_or_else(|| panic!("paragraph {k} names no style, and the connector's would be centred: {content}"));
        assert_eq!(stated(&content, name, "fo:text-align"), Ok("end".to_owned()), "paragraph {k}: {content}");
    }
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
}

/// **What no cell reads is still said**: `P2` holds an attribute the model has no cell for (`fo:keep-together`), so the connector's paragraph, whose graphic style gives the alignment, is written with a
/// style that carries it and is no style that says nothing.
#[test]
fn a_style_that_holds_what_no_cell_reads_still_says_it_for_the_connector() {
    let shapes = format!("{}{N2}{}", n1(true), tow_line("c", "Tow line"));
    let (styles, content) = written_by(&drawing("center", r#"fo:text-align="center" fo:keep-together="always""#, &shapes).bytes());
    assert_eq!(styles.len(), 2, "{content}");
    assert_eq!(empty_styles(&content), Vec::<String>::new(), "{content}");
    for (k, name) in styles.iter().enumerate() {
        let name = name.as_deref().unwrap_or_else(|| panic!("paragraph {k} names no style: {content}"));
        assert_eq!(stated(&content, name, "fo:keep-together"), Ok("always".to_owned()), "paragraph {k}: {content}");
    }
}

/// The paragraph rows of each shape of the first page, as `name=value` for every cell of each row: what is drawn, as the model says it.
fn paragraph_cells(doc: &Document) -> Vec<Vec<String>> {
    let page = doc.pages.get(0).unwrap();
    page.shapes
        .iter()
        .map(|shape| {
            let mut cells: Vec<String> = Vec::new();
            if let Some(section) = shape.sheet.section(&SectionKey::new("Paragraph", None)) {
                for row in section.rows.iter() {
                    for (name, cell) in &row.cells {
                        cells.push(format!("{}={}", name.as_str(), cell.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()));
                    }
                }
            }
            cells.sort();
            cells
        })
        .collect()
}

/// **Nothing drawn changes**: the file the visit writes reads as the file did, in the paragraph rows of every shape, for each arrangement of the vector.
#[test]
fn the_written_file_reads_with_the_paragraph_rows_the_file_had() {
    for (what, drawing) in [
        ("center", vector("center", true, false)),
        ("center, connector first", vector("center", true, true)),
        ("center, no paragraph in n1", vector("center", false, false)),
        ("end", vector("end", true, false)),
        ("end, connector first", vector("end", true, true)),
    ] {
        let source = drawing.bytes();
        let (written, _) = visit(&source).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (before, _) = read(&source, &Limits::default()).unwrap();
        let (after, _) = read(&written, &Limits::default()).unwrap_or_else(|e| panic!("{what}: the file written does not read: {e}"));
        assert_eq!(paragraph_cells(&after), paragraph_cells(&before), "{what}");
    }
}

/// **A statement the writer cannot spell is no style either**: `P2` states `end` under a prefix of its own for the namespace of `fo` (`xo`), which a held value is written without (the writer's
/// prefixes for the namespace are its own), so the connector's paragraph, whose graphic style gives the alignment, has nothing to write: it names no style, and no style says nothing. The paragraph of
/// `n1` still has its cell, in either order.
#[test]
fn a_statement_under_a_prefix_the_writer_does_not_spell_is_no_style_that_says_nothing() {
    let own = r#"xmlns:xo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xo:text-align="end""#;
    for (order, shapes) in [
        ("connector last", format!("{}{N2}{}", n1(true), tow_line("c", "Tow line"))),
        ("connector first", format!("{}{}{N2}", tow_line("c", "Tow line"), n1(true))),
    ] {
        let (styles, content) = written_by(&drawing("end", own, &shapes).bytes());
        assert_eq!(styles.len(), 2, "{order}: {content}");
        assert_eq!(empty_styles(&content), Vec::<String>::new(), "{order}: {content}");
        // Every paragraph that names a style names one that is written.
        let names = style_names(&content);
        for name in styles.iter().flatten() {
            assert!(names.contains(name), "{order}: paragraph names {name}, which is no style of the part: {content}");
        }
    }
}
