//! The byte budget of a reader: what `with_bytes`, `take_bytes`, `take_exact` and `give_bytes` do, what the reader charges for a sheet, and what it
//! does when the budget is spent. The prices themselves are `price.rs`'s; these hold each charge of the reader to them, counted.

use diacraft_sheetxml::model::cost::push_bytes;
use diacraft_sheetxml::price::{self, FORMULA_BYTES, FORMULA_NODE_BYTES, PLACED_BYTES, arc_str, cell_node_bytes};
use diacraft_sheetxml::{LimitKind, SheetReader};

use super::{BIG, Main, parse, read, read_el, read_with};

/// The charge of a price with the margin.
fn taken(n: u64) -> u64 {
    n + n.div_ceil(8) * price::MODEL_MARGIN_EIGHTHS
}

#[test]
fn a_reader_without_a_byte_budget_has_none_and_counts_what_it_would_have_charged() {
    let read = read(r#"<PageSheet><Cell N="Qz" V="1"/></PageSheet>"#);
    let stats = read.reader.stats();
    assert_eq!(read.reader.bytes_left(), u64::MAX);
    // The leaf of the map, the name (`Qz` is no name of the catalogue) and the value.
    assert_eq!(stats.bytes, taken(cell_node_bytes(0) + arc_str(2) + arc_str(1)));
    assert_eq!(stats.bytes_peak, stats.bytes);
}

#[test]
fn a_charge_takes_the_margin_and_an_exact_charge_does_not_and_a_charge_that_does_not_fit_takes_nothing() {
    let mut reader = SheetReader::new(BIG).with_bytes(1_000);
    reader.take_bytes(100).unwrap();
    assert_eq!((reader.bytes_left(), reader.stats().bytes), (1_000 - 113, 113));
    reader.take_exact(100).unwrap();
    assert_eq!((reader.bytes_left(), reader.stats().bytes), (1_000 - 213, 213));
    // 800 and its margin do not fit in what is left, and nothing is taken.
    let e = reader.take_bytes(800).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::Bytes, 1_000));
    assert_eq!((reader.bytes_left(), reader.stats().bytes), (1_000 - 213, 213));
    // The most that fits is taken, and no more.
    reader.take_exact(787).unwrap();
    assert_eq!(reader.bytes_left(), 0);
    assert!(reader.take_exact(1).is_err());
    reader.take_exact(0).unwrap();
}

#[test]
fn a_charge_given_back_is_available_again_and_more_than_was_taken_is_not_given() {
    let mut reader = SheetReader::new(BIG).with_bytes(1_000);
    reader.take_exact(600).unwrap();
    reader.give_bytes(250);
    assert_eq!((reader.bytes_left(), reader.stats().bytes), (650, 350));
    assert_eq!(reader.stats().bytes_peak, 600, "the peak is the most that was charged at once");
    reader.give_bytes(10_000);
    assert_eq!((reader.bytes_left(), reader.stats().bytes), (1_000, 0));
    reader.take_exact(1_000).unwrap();
    assert_eq!(reader.stats().bytes_peak, 1_000);
}

#[test]
fn the_class_word_of_the_byte_budget_is_too_much_memory_and_the_error_says_what_it_was() {
    let e = diacraft_sheetxml::LimitError { kind: LimitKind::Bytes, max: 7 };
    assert_eq!((e.kind.class(), e.kind.what()), ("too_much_memory", "bytes of memory to read the file"));
    assert_eq!(e.to_string(), "the file holds more than 7 bytes of memory to read the file");
}

#[test]
fn a_sheet_that_does_not_fit_the_byte_budget_is_refused_whole_and_one_that_does_is_read() {
    let xml = r#"<PageSheet><Cell N="A" V="1"/><Cell N="B" V="2"/><Section N="User"><Row N="x"><Cell N="Value" V="3"/></Row></Section></PageSheet>"#;
    let needed = read_with::<Main>(xml, BIG).unwrap().reader.stats().bytes;
    let at = |bytes: u64| {
        let el = parse(xml);
        let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
        let mut reader = SheetReader::new(BIG).with_bytes(bytes);
        let mut warnings = diacraft_sheetxml::Warnings::new(5);
        let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
        reader.read_sheet_only(&mut cx, &el, &sc).map(|_| reader.stats().bytes)
    };
    assert_eq!(at(needed), Ok(needed));
    let e = at(needed - 1).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::Bytes, needed - 1));
    assert!(at(0).is_err());
}

#[test]
fn the_charges_of_a_cell_a_row_a_section_and_kept_markup_are_counted() {
    // A `User` section of one row of one cell, and a kept element, a comment and an instruction under the sheet.
    let read = read(r#"<PageSheet><Section N="User"><Row N="x"><Cell N="Value" V="3"/></Row></Section><z/><!--c--><?p d?></PageSheet>"#);
    let section = taken(price::SECTION_BYTES + push_bytes(0, price::POINTER_BYTES) + price::SEEN_ENTRY_BYTES + price::heap(4));
    let row = taken(push_bytes(0, price::POINTER_BYTES) + price::SEEN_ENTRY_BYTES) + taken(price::ROW_BYTES + arc_str(1));
    let cell = taken(cell_node_bytes(0) + arc_str(1));
    let element = taken(push_bytes(0, PLACED_BYTES) + arc_str(1));
    let comment = taken(push_bytes(1, PLACED_BYTES) + arc_str(1));
    let instruction = taken(push_bytes(2, PLACED_BYTES) + arc_str(1) + arc_str(1));
    assert_eq!(read.reader.stats().bytes, section + row + cell + element + comment + instruction);
}

#[test]
fn a_formula_the_byte_budget_cannot_afford_is_kept_as_text_and_the_sheet_is_read() {
    let xml = r#"<PageSheet><Cell N="Qz" V="1" F="1+2+3+4+5+6+7+8+9"/></PageSheet>"#;
    let free = read(xml);
    let nodes = free.reader.stats().formula_nodes as u64;
    assert!(nodes >= 9, "{nodes}");
    let cell_only = taken(cell_node_bytes(0) + arc_str(2) + arc_str(1) + FORMULA_BYTES + arc_str(17) + price::heap(17));
    assert_eq!(free.reader.stats().bytes, cell_only + taken(nodes * FORMULA_NODE_BYTES));
    // A budget with room for the cell and four nodes: the parser is given four nodes to make, cannot make nine, and the formula is kept as its text.
    let budget = cell_only + taken(4 * FORMULA_NODE_BYTES);
    let el = parse(xml);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let mut reader = SheetReader::new(BIG).with_bytes(budget);
    let mut warnings = diacraft_sheetxml::Warnings::new(5);
    let sheet = {
        let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
        reader.read_sheet_only(&mut cx, &el, &sc).unwrap()
    };
    assert_eq!(sheet.cell("Qz").unwrap().formula.text(), Some("1+2+3+4+5+6+7+8+9"));
    assert_eq!(reader.stats().formula_nodes, 0);
    assert_eq!(warnings.formula_errors.get("too_many_nodes"), Some(&1));
    assert_eq!(reader.stats().bytes, cell_only);
}

#[test]
fn the_price_of_a_reader_that_reads_the_same_tree_twice_is_twice_the_price() {
    let el = parse(
        r#"<PageSheet><Cell N="A" V="1" U="IN"/><Section N="Geometry" IX="0"><Row IX="1" T="LineTo"><Cell N="X" V="1"/></Row></Section></PageSheet>"#,
    );
    let once = read_el::<Main>(&el, BIG).unwrap().reader.stats().bytes;
    // A second sheet read through the same reader holds the words of the first (the unit, the row type): they cost nothing the second time, and the rest does.
    let mut reader = SheetReader::new(BIG);
    for _ in 0..2 {
        let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
        let mut warnings = diacraft_sheetxml::Warnings::new(5);
        let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
        reader.read_sheet_only(&mut cx, &el, &sc).unwrap();
    }
    let words = taken(price::INTERN_ENTRY_BYTES + arc_str(2) + price::INTERN_ENTRY_BYTES + arc_str(6));
    assert!(reader.stats().bytes < 2 * once, "{} {}", reader.stats().bytes, once);
    assert!(reader.stats().bytes > 2 * once - 2 * words, "{} {} {}", reader.stats().bytes, once, words);
}

/// Reads `xml` as a sheet through `reader`.
fn read_through(reader: &mut SheetReader, xml: &str) {
    let el = parse(xml);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    let mut warnings = diacraft_sheetxml::Warnings::new(5);
    let mut cx = diacraft_sheetxml::Cx::new(&mut warnings, "/p.xml");
    reader.read_sheet_only(&mut cx, &el, &sc).unwrap();
}

#[test]
fn the_table_of_repeated_words_holds_four_thousand_and_ninety_six_and_a_word_past_that_has_no_entry() {
    let words = |n: usize| -> String { (0..n).map(|i| format!("<Cell N=\"C{i}\" U=\"w{i}\"/>")).collect() };
    let next = |reader: &mut SheetReader| {
        let before = reader.stats().bytes;
        read_through(reader, "<PageSheet><Cell N=\"Qz\" U=\"new\"/></PageSheet>");
        reader.stats().bytes - before
    };
    let (cell, word) = (cell_node_bytes(0) + arc_str(2), arc_str(3));
    // With 4,095 words held the table has room for one more: the word is charged its own bytes and its entry in the table.
    let mut reader = SheetReader::new(BIG);
    read_through(&mut reader, &format!("<PageSheet>{}</PageSheet>", words(4_095)));
    assert_eq!(next(&mut reader), taken(cell + word + price::INTERN_ENTRY_BYTES));
    // With 4,096 held (that word was the last the table took) it is full: a word past it is charged its bytes alone, and so is the next.
    assert_eq!(next(&mut reader), taken(cell_node_bytes(0) + arc_str(2)) + taken(0), "the word is held now");
    let mut full = SheetReader::new(BIG);
    read_through(&mut full, &format!("<PageSheet>{}</PageSheet>", words(4_096)));
    assert_eq!(next(&mut full), taken(cell + word));
}

#[test]
fn a_word_of_thirty_two_bytes_is_held_once_and_one_of_thirty_three_is_not_held_and_the_price_says_the_same() {
    // The reader holds a short word once however many cells name it, and charges the table's entry for it the first time and nothing the next. The word is short at 32
    // bytes (the longest the table takes) and not at 33, in the table and in the price alike (the two said it each their own way).
    let cost = |word: &str, times: usize| -> Vec<u64> {
        let mut reader = SheetReader::new(BIG);
        (0..times)
            .map(|_| {
                let before = reader.stats().bytes;
                read_through(&mut reader, &format!("<PageSheet><Cell N=\"Qz\" U=\"{word}\"/></PageSheet>"));
                reader.stats().bytes - before
            })
            .collect()
    };
    let cell = cell_node_bytes(0) + arc_str(2);
    let (short, long) = ("s".repeat(32), "l".repeat(33));
    assert_eq!(cost(&short, 3), [taken(cell + arc_str(32) + price::INTERN_ENTRY_BYTES), taken(cell), taken(cell)], "the 32 bytes are held");
    assert_eq!(cost(&long, 3), [taken(cell + arc_str(33)); 3], "the 33 bytes are not, and each cell holds its own");
}
