//! The review: each charge the reader makes for the attributes of DiaCraft's extension on a shape's style has a test that
//! fails when the charge is taken out.
//!
//! A mutation run that deletes statements found the charges of `ext/payload.rs` (a fragment's bytes, the cells and items its sheet reader spent, the nodes of a
//! text, an unknown element of a text, a list of removals) deletable with no test failing. Each case reads a style that carries one kind of fragment, once small and
//! once large, and holds the growth of the items (or of the cells) the read charged to what the fragment stands for: a charge that is not made leaves the growth at
//! what the other charges make of it, which is under the bound.

use diacraft_odg::{Limits, ReadReport, read};

use crate::drawing::Drawing;

/// An automatic graphic style `gr1` whose graphic properties carry `attrs`, on a rectangle.
fn drawing(attrs: &str) -> Drawing {
    let style = format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#);
    let rect = r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#;
    Drawing::a4().auto(&style).page(rect)
}

/// `xml` as the value of an attribute.
fn value(xml: &str) -> String {
    xml.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn report_of(attrs: &str) -> ReadReport {
    match read(&drawing(attrs).bytes(), &Limits::default()) {
        Ok((_, report)) => report,
        Err(e) => panic!("the drawing does not read: {e}"),
    }
}

fn attrs_of(local: &str, fragment: &str) -> String {
    format!(r#"diacraft:version="1" diacraft:id="7" diacraft:{local}="{}""#, value(fragment))
}

/// A `Sheet` fragment of `n` cells.
fn sheet_of(n: usize) -> String {
    let cells: String = (0..n).map(|k| format!(r#"<Cell N="C{k}" V="1"/>"#)).collect();
    format!("<Sheet>{cells}</Sheet>")
}

/// The bytes of the attribute that holds a fragment are charged (by the reader of the style, before the fragment's own charge, which the writer's decode shows by
/// itself: `payload::tests::the_bytes_of_a_fragment_are_charged_by_the_writers_decode`), whatever it holds: white space after the root of one that reads is 400,000 bytes, and costs the items
/// they stand for at 64 bytes an item (`ITEM_BYTES`) beyond what the fragment costs without it.
#[test]
fn the_bytes_of_a_fragment_are_charged_before_it_is_parsed() {
    let plain = sheet_of(1);
    let padded = format!("{plain}{}", " ".repeat(400_000));
    let (small, large) = (report_of(&attrs_of("cells", &plain)).work.items, report_of(&attrs_of("cells", &padded)).work.items);
    assert!(large >= small + 400_000 / 64 - 8, "400,000 bytes of white space cost {} items, fewer than the 6,250 they stand for", large - small);
}

/// The cells a fragment's sheet reader spent are charged to the document's cells and items: the cells (`stats.cells`, and `ITEMS_PER_CELL` items each) and the
/// items the sheet reader counted besides (at least one for each cell).
#[test]
fn the_cells_and_the_items_a_fragments_sheet_reader_spent_are_charged() {
    let (few, many) = (report_of(&attrs_of("cells", &sheet_of(10))), report_of(&attrs_of("cells", &sheet_of(310))));
    assert_eq!(many.stats.cells - few.stats.cells, 300, "each cell of a fragment is counted in the document's cells");
    // 300 cells more: `ITEMS_PER_CELL` (4) items for each from the cells' charge, at least one more from the sheet reader's items, and the bytes of their text.
    let grown = many.work.items - few.work.items;
    assert!(grown >= 300 * 5, "300 cells cost {grown} items, fewer than the 1,500 the cells' charge and the sheet reader's make");
    // And the document's cells are a budget the fragments draw on: a cap of 100 cells refuses 310 of them.
    let limits = Limits { max_cells: 100, ..Limits::default() };
    match read(&drawing(&attrs_of("cells", &sheet_of(310))).bytes(), &limits) {
        Err(e) => assert!(e.class().contains("cells") || e.class().contains("complex"), "{e}"),
        Ok(_) => panic!("310 cells of a fragment went through a cap of 100"),
    }
}

/// The nodes of a text fragment are charged one item for each, and an element of it that no marker is, by the nodes it holds.
#[test]
fn the_nodes_of_a_text_fragment_and_an_unknown_element_in_it_are_charged() {
    let marks: String = (0..500).map(|_| r#"<cp IX="1"/>"#).collect();
    let (none, marked) =
        (report_of(&attrs_of("text", "<Text>a</Text>")).work.items, report_of(&attrs_of("text", &format!("<Text>{marks}</Text>"))).work.items);
    assert!(marked - none >= 500, "500 markers cost {} items, fewer than the 500 nodes", marked - none);
    // An unknown element holding 300 children: the element and what it holds are charged by their nodes, beyond the bytes of its text.
    let inner: String = (0..300).map(|_| "<y/>").collect();
    let small = report_of(&attrs_of("text", "<Text><x/></Text>")).work.items;
    let big = report_of(&attrs_of("text", &format!("<Text><x>{inner}</x></Text>"))).work.items;
    assert!(big - small >= 300, "an element of 300 children costs {} items more than an empty one", big - small);
}

/// A list of removals is charged one item for each token.
#[test]
fn the_tokens_of_a_list_of_removals_are_charged() {
    let tokens: String = (0..500).map(|k| format!("c.G{k} ")).collect();
    let (none, many) = (report_of(&attrs_of("gone", "c.A")).work.items, report_of(&attrs_of("gone", &tokens)).work.items);
    assert!(many - none >= 499, "500 tokens cost {} items, fewer than their number", many - none);
}
