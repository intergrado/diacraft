//! The review: each charge the reader makes for what a document file puts into the model has a test that fails when the
//! charge is taken out.
//!
//! The file is built by hand, as a hostile file is. A charge for bytes is on top of the backstop (the file's size, one item for 64 bytes, charged once for the
//! whole file), so a test of one holds the growth of the items to about twice what the backstop alone makes of the same bytes: with the branch's charge gone the
//! growth is the backstop's, and under the bound.

use crate::docfile_cost::{file_of, items};
use crate::drawing::Drawing;
use diacraft_odg::{Limits, read};

const NS: &str = "urn:app.diacraft:extension";

/// The items of a document file read with the drawing of `pages` pages (the default of one).
fn items_with_pages(file: &str, pages: usize) -> u64 {
    let mut drawing = Drawing::a4();
    for k in 0..pages {
        drawing = drawing.page(&crate::write_core::custom(&format!("s{k}"), "1cm"));
    }
    drawing.files.push(("DiaCraft/document.xml".to_owned(), file.as_bytes().to_vec()));
    match read(&drawing.bytes(), &Limits::default()) {
        Ok((_, report)) => report.work.items,
        Err(e) => panic!("the file does not read: {e}"),
    }
}

/// A subtree the `data` child keeps whole is charged by its nodes (on top of its bytes).
#[test]
fn the_nodes_of_the_data_a_file_keeps_whole_are_charged() {
    let data = |n: usize| file_of(&format!("<diacraft:data><x>{}</x></diacraft:data>", "<y/>".repeat(n)));
    let (none, many) = (items(&data(0)), items(&data(400)));
    assert!(many - none >= 400, "400 nodes cost {} items, fewer than their number", many - none);
}

/// The pages are indexed by their names once, and the index is charged for what it holds (one item for each page) before it is made.
#[test]
fn the_index_of_the_pages_by_name_is_charged_for_the_pages_it_holds() {
    let page_entry = file_of(r#"<diacraft:page name="page1"/>"#);
    let none = file_of("");
    let (few, many) =
        (items_with_pages(&page_entry, 5) - items_with_pages(&none, 5), items_with_pages(&page_entry, 405) - items_with_pages(&none, 405));
    assert!(
        many >= few + 400 - 4,
        "an entry over 405 pages costs {many} items more than none, an entry over 5 pages {few}: the index of 400 more pages is not charged"
    );
}

/// The strings an entry or a theme holds are charged by their bytes, over the backstop's charge of the whole file: 1.8 times the bytes' worth, at 64 bytes an item.
#[test]
fn the_name_of_a_page_the_bytes_of_a_theme_and_the_name_of_its_part_are_charged_by_their_bytes() {
    const N: usize = 400_000;
    let big = "a".repeat(N);
    let small = "a";
    let bound = |what: &str, large: u64, tiny: u64| {
        let grown = large - tiny;
        let backstop = (N / 64) as u64;
        assert!(
            grown >= backstop * 18 / 10,
            "{what}: {N} bytes cost {grown} items, fewer than 1.8 times the backstop's {backstop}: the branch does not charge them"
        );
    };
    let name_u = |v: &str| file_of(&format!(r#"<diacraft:page name="page1" name-u="{v}"/>"#));
    bound("the name-u of a page", items(&name_u(&big)), items(&name_u(small)));
    // A theme of N characters of base64 (a multiple of four, decoded to bytes).
    let theme = |part: &str, body: &str| file_of(&format!(r#"<diacraft:theme part="{part}">{body}</diacraft:theme>"#));
    let encoded = "QUJD".repeat(N / 4);
    bound("the base64 text of a theme", items(&theme("t", &encoded)), items(&theme("t", "QUJD")));
    bound("the part name of a theme", items(&theme(&big, "QUJD")), items(&theme(small, "QUJD")));
}

/// A colour is two items and the bytes of its value; the colours are charged one by one.
#[test]
fn the_colours_of_a_file_are_charged_two_items_each_and_their_values_by_their_bytes() {
    let colors = |n: usize, rgb: &str| {
        file_of(&format!("<diacraft:colors>{}</diacraft:colors>", format!(r#"<diacraft:color ix="1" rgb="{rgb}"/>"#).repeat(n)))
    };
    let (none, many) = (items(&colors(0, "#112233")), items(&colors(2_000, "#112233")));
    // Each colour is 40 bytes (a backstop's three fifths of an item) and two items of the branch's own.
    assert!(many - none >= 2 * 2_000, "2,000 colours cost {} items, fewer than the 4,000 of two each", many - none);
    let value = "a".repeat(100_000);
    let (short, long) = (items(&colors(8, "a")), items(&colors(8, &value)));
    assert!(
        long - short >= 8 * (100_000 / 64) * 18 / 10,
        "8 values of 100,000 bytes cost {} items, fewer than 1.8 times the backstop's",
        long - short
    );
}

/// The tokens of the removals of a page or the document's sheet are charged one item for each (the list is capped, and charged for what is read of it).
#[test]
fn the_removals_of_an_entry_are_charged_one_item_for_each_token() {
    let gone = |n: usize| {
        let tokens: String = (0..n).map(|k| format!("c.G{k} ")).collect();
        file_of(&format!(r#"<diacraft:page name="page1" gone="{tokens}"/>"#))
    };
    let (none, many) = (items(&gone(1)), items(&gone(500)));
    // 500 tokens of 7 bytes: under a tenth of an item each for the bytes, one each for the tokens.
    assert!(many - none >= 499, "500 tokens cost {} items, fewer than their number", many - none);
    let _ = NS;
}

/// The review, M14: the style sheets are indexed by their names once, when the first entry for one is met, and the index is charged for
/// what it holds (one item for each style sheet) before it is made. A file with an entry for a style costs the items of the styles the drawing has more than one without it.
#[test]
fn the_index_of_the_style_sheets_by_name_is_charged_for_the_styles_it_holds() {
    let cost = |file: &str, styles: usize| {
        let common: String = (0..styles).map(|k| format!(r#"<style:style style:name="S{k}" style:family="graphic"/>"#)).collect();
        let mut drawing = Drawing::a4().common(&common).page(&crate::write_core::custom("s0", "1cm"));
        drawing.files.push(("DiaCraft/document.xml".to_owned(), file.as_bytes().to_vec()));
        match read(&drawing.bytes(), &Limits::default()) {
            Ok((_, report)) => report.work.items,
            Err(e) => panic!("the file does not read: {e}"),
        }
    };
    let (entry, none) = (file_of(r#"<diacraft:style name="S0"/>"#), file_of(""));
    for styles in [100usize, 400] {
        let extra = cost(&entry, styles) - cost(&none, styles);
        assert!(extra >= styles as u64, "an entry in a drawing of {styles} style sheets costs {extra} items, fewer than the sheets it indexes");
    }
}
