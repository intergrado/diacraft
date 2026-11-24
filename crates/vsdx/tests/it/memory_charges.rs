//! What each charge to the read's pool is, counted: a file with one more of a thing is charged the price of that thing more, with the margin
//! (`taken`). The test of a price is the difference between two files that differ by one object, so that what the two share (the document, the page, the
//! index) is not in it, and the figure is held to the function that makes the price (`diacraft_sheetxml::price`, `diacraft_vsdx::cost`) and, for the ones that
//! have a number a person can check, to the number. `memory_price.rs` holds the prices to the memory of a process; these hold each call that charges to its price,
//! so a charge that is taken out, or taken twice, or for the wrong thing, is a failure here.
//!
//! Each call of the reader that charges is one charge, and the margin is on each call's sum, not on the parts of it: the expected figures are written the way
//! the calls are (`taken(a + b)` for one call, `taken(a) + taken(b)` for two).

use diacraft_sheetxml::SHARING_ENTRY_BYTES;
use diacraft_sheetxml::model::cost::push_bytes;
use diacraft_sheetxml::price::{self, PLACED_BYTES, POINTER_BYTES, arc_str, cell_node_bytes, heap, raw_attr_bytes};
use diacraft_testkit::package::{MasterSpec, OD, PageSpec, Pkg, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::cost::{self, MEDIA_BYTES, SHAPE_BYTES};
use diacraft_vsdx::{DiacraftPart, Limits, ReadReport, read_partial};

use crate::{read_ok, read_with};

/// What the pool is charged for a price of `n`: the price and an eighth of it (`MODEL_MARGIN_EIGHTHS`).
fn taken(n: u64) -> u64 {
    n + n.div_ceil(8)
}

fn report_of(bytes: &[u8]) -> ReadReport {
    read_ok(bytes).1
}

/// The bytes the model of a page that holds `shapes` is charged for when the read ends (the trees are given back).
fn charged_page(shapes: &str) -> u64 {
    report_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes)).build()).stats.memory_bytes
}

/// What one more thing costs: the pool's charge for a page of `more` less the charge for a page of `base`.
fn extra(base: &str, more: &str) -> u64 {
    charged_page(more) - charged_page(base)
}

// ------------------------------------------------------------------------------------------------------------------------------------ the prices

#[test]
fn the_figures_the_prices_are_made_of_are_these_where_a_word_is_eight_bytes() {
    // A field the model gains moves a price: this fails, and the figures of `limits.rs` are measured again (`memory_price.rs`).
    if cfg!(target_pointer_width = "64") {
        assert_eq!((SHAPE_BYTES, price::ROW_BYTES, price::SECTION_BYTES, price::SHEET_BYTES), (384, 144, 160, 128));
        assert_eq!((price::CELL_EXTRA_BYTES, price::FORMULA_BYTES), (64, 80));
        assert_eq!((POINTER_BYTES, PLACED_BYTES, price::PAIR_BYTES, price::RAW_NODE_BYTES), (8, 56, 32, 48));
        assert_eq!(MEDIA_BYTES, 80 + 192);
        assert_eq!((cost::TEXT_BYTES, cost::FOREIGN_BYTES), (64, 96));
        assert_eq!((cost::STYLE_BYTES, cost::MASTER_BYTES, cost::PAGE_BYTES), (392, 584, 360));
        assert_eq!((cost::PARSED_BYTES, cost::REL_ROW_BYTES, cost::TABLE_ROW_BYTES), (256, 512, 128));
        assert_eq!(price::SEEN_ENTRY_BYTES, 64);
        assert_eq!(price::INTERN_ENTRY_BYTES, 160);
        assert_eq!(cost::TREE_ITEM_BYTES, 368);
        assert_eq!(price::FORMULA_NODE_BYTES, 128);
        assert_eq!(price::MODEL_MARGIN_EIGHTHS, 1);
    }
    // A string or a buffer is its bytes and twenty-four more, rounded up to sixteen, and never under thirty-two (the law of the counted memory
    // tests, `memory_counted.rs`; eight more before, glibc's own chunk, which was 32, 32, 48, 48, 32, 32, 48).
    assert_eq!((arc_str(0), arc_str(3), arc_str(16), arc_str(17), heap(0), heap(24), heap(25)), (48, 48, 64, 64, 32, 48, 64));
    assert_eq!(cost::GROWTH_BYTES, 64 * 1024 * 1024);
}

#[test]
fn a_map_of_cells_is_priced_at_the_leaf_of_eleven_and_at_five_a_leaf_past_it() {
    let bytes: Vec<u64> = (0..=40).map(price::cells_map_bytes).collect();
    assert_eq!(bytes[0], 0);
    // The first leaf holds eleven, however few there are.
    assert!(bytes[1..=11].iter().all(|b| *b == bytes[1]), "{bytes:?}");
    assert_eq!(bytes[1], 1168);
    // The twelfth makes a split: two leaves and the inner node over them, at the worst three leaves.
    assert_eq!(bytes[12], 3 * 1168 + 1264);
    // And never goes down.
    assert!(bytes.windows(2).all(|w| w[0] <= w[1]), "{bytes:?}");
    // What one more costs adds up to what the map costs.
    let summed: u64 = (0..40).map(cell_node_bytes).sum();
    assert_eq!(summed, bytes[40]);
}

// ------------------------------------------------------------------------------------------------------------------------------------- the cells

/// A shape holding `n` cells named `C0`.., the shape's own cells in one map, each made by `each`.
fn shape_of_cells(n: usize, each: impl Fn(usize) -> String) -> String {
    shape(1, &(0..n).map(each).collect::<String>())
}

#[test]
fn a_cell_is_charged_its_place_in_the_nodes_of_its_map_its_name_and_its_value() {
    // A name the catalogue does not list is an allocation; the value is another.
    for have in [0usize, 1, 5, 10, 11, 12, 13, 15, 16, 30] {
        let one = |n: usize| shape_of_cells(n, |i| cell(&format!("C{i}"), "1.5"));
        let name = format!("C{have}").len();
        assert_eq!(extra(&one(have), &one(have + 1)), taken(cell_node_bytes(have) + arc_str(name) + arc_str(3)), "the cell after {have}");
    }
}

#[test]
fn a_cell_with_a_name_the_catalogue_lists_holds_no_allocation_for_it() {
    let listed = extra(&shape(1, ""), &shape(1, &cell("PinX", "1")));
    let unlisted = extra(&shape(1, ""), &shape(1, &cell("PinZ", "1")));
    assert_eq!((listed, unlisted), (taken(cell_node_bytes(0) + arc_str(1)), taken(cell_node_bytes(0) + arc_str(1) + arc_str(4))));
}

#[test]
fn a_unit_and_an_error_word_are_charged_once_and_a_second_cell_that_says_the_same_costs_nothing_for_it() {
    let table = price::INTERN_ENTRY_BYTES;
    let with = |u: &str, e: &str| shape(1, &format!("<Cell N=\"PinX\" V=\"1\" U=\"{u}\" E=\"{e}\"/>"));
    let first = extra(&shape(1, "<Cell N=\"PinX\" V=\"1\"/>"), &with("IN", "#N/A"));
    assert_eq!(first, taken(cell_node_bytes(0) + arc_str(1) + arc_str(2) + table + arc_str(4) + table) - taken(cell_node_bytes(0) + arc_str(1)));
    // The same words in the next cell cost nothing: the difference of the second cell is that of a cell with a value alone.
    let two = |second: &str| shape(1, &format!("<Cell N=\"PinX\" V=\"1\" U=\"IN\" E=\"#N/A\"/>{second}"));
    let again = extra(&two("<Cell N=\"PinY\" V=\"1\"/>"), &two("<Cell N=\"PinY\" V=\"1\" U=\"IN\" E=\"#N/A\"/>"));
    assert_eq!(again, 0);
    // A different word is charged again.
    let other = extra(&two("<Cell N=\"PinY\" V=\"1\"/>"), &two("<Cell N=\"PinY\" V=\"1\" U=\"MM\"/>"));
    assert_eq!(other, taken(cell_node_bytes(1) + arc_str(1) + arc_str(2) + table) - taken(cell_node_bytes(1) + arc_str(1)));
    // A word over 32 bytes is no word of the table (it is kept by the cell and held by no table): its own bytes, and no entry.
    let long = "u".repeat(40);
    let held = extra(&shape(1, "<Cell N=\"PinX\" V=\"1\"/>"), &shape(1, &format!("<Cell N=\"PinX\" V=\"1\" U=\"{long}\"/>")));
    assert_eq!(held, taken(cell_node_bytes(0) + arc_str(1) + arc_str(40)) - taken(cell_node_bytes(0) + arc_str(1)));
}

#[test]
fn a_cell_that_keeps_attributes_is_charged_the_list_that_holds_them_and_each_name_and_value() {
    let bare = shape(1, "<Cell N=\"PinX\"/>");
    let more = |attrs: &str| shape(1, &format!("<Cell N=\"PinX\"{attrs}/>"));
    let one = extra(&bare, &more(" a=\"x\""));
    assert_eq!(one, taken(cell_node_bytes(0) + raw_attr_bytes(0, 1, 1)) - taken(cell_node_bytes(0)));
    // The list has room for four at its first, and doubles at the fifth.
    let five = extra(&bare, &more(" a=\"x\" b=\"yy\" c=\"zzz\" d=\"w\" e=\"vvvvv\""));
    let sum: u64 = [(1, 1), (1, 2), (1, 3), (1, 1), (1, 5)].iter().enumerate().map(|(have, (k, v))| raw_attr_bytes(have, *k, *v)).sum();
    assert_eq!(five, taken(cell_node_bytes(0) + sum) - taken(cell_node_bytes(0)));
}

#[test]
fn a_formula_is_charged_its_text_its_formula_and_each_node_of_its_tree() {
    let plain = shape(1, "<Cell N=\"PinX\" V=\"1\"/>");
    let formula = |f: &str| shape(1, &format!("<Cell N=\"PinX\" V=\"1\" F=\"{f}\"/>"));
    let base = cell_node_bytes(0) + arc_str(1);
    for f in ["Width*2", "(Width+Height)*(PinX-PinY)+1", "1"] {
        let nodes = report_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&formula(f))).build()).stats.formula_nodes as u64;
        assert!(nodes >= 1, "{f}");
        // The text, the formula and the strings its tree holds (no more than the text) go with the cell, in its one charge; the nodes of the tree are charged when it
        // is made, a charge of their own.
        let expected = taken(base + price::FORMULA_BYTES + arc_str(f.len()) + heap(f.len())) - taken(base) + taken(nodes * price::FORMULA_NODE_BYTES);
        assert_eq!(extra(&plain, &formula(f)), expected, "{f}");
    }
}

#[test]
fn the_markers_of_a_formula_are_charged_nothing_for_it() {
    let plain = shape(1, "<Cell N=\"PinX\" V=\"1\"/>");
    for marker in ["Inh", "No Formula"] {
        assert_eq!(extra(&plain, &shape(1, &format!("<Cell N=\"PinX\" V=\"1\" F=\"{marker}\"/>"))), 0, "{marker}");
    }
}

#[test]
fn a_cell_that_holds_text_is_charged_the_extra_it_makes_and_the_text() {
    let plain = shape(1, "<Cell N=\"PinX\" V=\"1\"/>");
    let text = |t: &str| shape(1, &format!("<Cell N=\"PinX\" V=\"1\">{t}</Cell>"));
    assert_eq!(extra(&plain, &text("hello")), taken(price::CELL_EXTRA_BYTES + arc_str(5) + heap(5)));
    // A cell that holds markup keeps it in order, each node in the passthrough of the cell's extra.
    let markup = shape(1, "<Cell N=\"PinX\" V=\"1\"><RefBy ID=\"1\"/><?NewValue V?></Cell>");
    let refby = arc_str(5) + price::block(16 + 24) + heap(price::PAIR_BYTES) + arc_str(2) + arc_str(1);
    let expected =
        taken(price::CELL_EXTRA_BYTES) + taken(push_bytes(0, PLACED_BYTES) + refby) + taken(push_bytes(1, PLACED_BYTES) + arc_str(8) + arc_str(1));
    assert_eq!(extra(&plain, &markup), expected);
}

// -------------------------------------------------------------------------------------------------------------------------- sections and rows

#[test]
fn a_section_is_charged_itself_its_place_in_the_list_and_its_name_where_the_catalogue_has_none() {
    let sections = |n: usize, name: &str| shape(1, &(0..n).map(|_| format!("<Section N=\"{name}\"/>")).collect::<String>());
    // A `Geometry` section is noted in no tally and holds no name of its own: the section and its place.
    let geometry = |n: usize| shape(1, &(0..n).map(|i| format!("<Section N=\"Geometry\" IX=\"{i}\"/>")).collect::<String>());
    // The reader shares what is equal: each section here is a different one (its own `IX`), and is charged itself and its entry in the table of what was met.
    for have in [0usize, 1, 3, 4, 7, 8] {
        assert_eq!(
            extra(&geometry(have), &geometry(have + 1)),
            taken(price::SECTION_BYTES + push_bytes(have, POINTER_BYTES)) + taken(SHARING_ENTRY_BYTES),
            "the section after {have}"
        );
    }
    // A section that is equal to the one before it is that one: it is charged its place in the list and the rest is given back.
    let listed = extra(&sections(1, "User"), &sections(2, "User"));
    assert_eq!(listed, taken(push_bytes(1, POINTER_BYTES)), "a second section of the same name and content is the first");
    let unlisted = extra(&sections(1, "Mine"), &sections(2, "Mine"));
    assert_eq!(unlisted, taken(push_bytes(1, POINTER_BYTES)));
    // Any other name is noted in the sheet's tally (a bucket and the string), and a name the catalogue does not list is an allocation: the first of a name is charged for them.
    let first_listed = extra(&sections(0, "User"), &sections(1, "User"));
    assert_eq!(
        first_listed,
        taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + price::SEEN_ENTRY_BYTES + heap(4)) + taken(SHARING_ENTRY_BYTES)
    );
    let first_unlisted = extra(&sections(0, "Mine"), &sections(1, "Mine"));
    assert_eq!(
        first_unlisted,
        taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + arc_str(4) + price::SEEN_ENTRY_BYTES + heap(4)) + taken(SHARING_ENTRY_BYTES)
    );
}

#[test]
fn a_section_that_keeps_attributes_is_charged_for_them_and_a_geometry_index_that_reads_as_written_is_not_kept() {
    let geometry = |attrs: &str| shape(1, &format!("<Section N=\"Geometry\"{attrs}/>"));
    // Each is the first of its kind in its read: charged itself and its entry in the table of what was met.
    let entry = taken(SHARING_ENTRY_BYTES);
    let base = taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES)) + entry;
    let at = |attrs: &str| extra(&shape(1, ""), &geometry(attrs));
    assert_eq!(at(" IX=\"0\""), base);
    assert_eq!(at(" IX=\"0\" Z=\"zz\""), taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + raw_attr_bytes(0, 1, 2)) + entry);
    // The second attribute kept goes in the list the first made: the list is charged once.
    assert_eq!(
        at(" IX=\"0\" Z=\"zz\" W=\"w\""),
        taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + raw_attr_bytes(0, 1, 2) + raw_attr_bytes(1, 1, 1)) + entry
    );
    // An index that is not the text of its number is kept as written.
    assert_eq!(at(" IX=\"00\""), taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + raw_attr_bytes(0, 2, 2)) + entry);
    assert_eq!(at(" Del=\"1\""), base);
    assert_eq!(at(" Del=\"true\""), taken(price::SECTION_BYTES + push_bytes(0, POINTER_BYTES) + raw_attr_bytes(0, 3, 4)) + entry);
}

#[test]
fn a_row_is_charged_itself_its_place_its_key_and_its_strings() {
    let rows = |n: usize| shape(1, &format!("<Section N=\"User\">{}</Section>", (0..n).map(|i| format!("<Row N=\"r{i}\"/>")).collect::<String>()));
    for have in [0usize, 1, 3, 4, 8, 9, 10] {
        let name = format!("r{have}").len();
        // The place in the list and the key's entry are one charge, and the row and its strings another.
        let expected = taken(push_bytes(have, POINTER_BYTES) + price::SEEN_ENTRY_BYTES) + taken(price::ROW_BYTES + arc_str(name));
        assert_eq!(extra(&rows(have), &rows(have + 1)), expected, "the row after {have}");
    }
}

#[test]
fn a_row_that_keeps_a_type_a_local_name_and_attributes_is_charged_for_each() {
    let one = |attrs: &str| shape(1, &format!("<Section N=\"Geometry\" IX=\"0\"><Row IX=\"1\"{attrs}/></Section>"));
    let at = |attrs: &str| extra(&one(""), &one(attrs));
    let table = price::INTERN_ENTRY_BYTES;
    let row = taken(price::ROW_BYTES);
    // `T` is a word that repeats in every file: its own and its entry in the table, once.
    assert_eq!(at(" T=\"LineTo\""), taken(price::ROW_BYTES + arc_str(6) + table) - row);
    assert_eq!(at(" LocalName=\"abc\""), taken(price::ROW_BYTES + arc_str(3)) - row);
    // A flag that is `1` is typed; any other text is kept as written.
    assert_eq!(at(" Del=\"1\""), 0);
    assert_eq!(at(" Del=\"true\""), taken(price::ROW_BYTES + raw_attr_bytes(0, 3, 4)) - row);
    assert_eq!(at(" q=\"1\""), taken(price::ROW_BYTES + raw_attr_bytes(0, 1, 1)) - row);
    assert_eq!(at(" q=\"1\" r=\"22\""), taken(price::ROW_BYTES + raw_attr_bytes(0, 1, 1) + raw_attr_bytes(1, 1, 2)) - row);
    // An empty name is no name (the attribute is kept as written), and an index that is not the text of its number is kept as written too.
    assert_eq!(at(" N=\"\""), taken(price::ROW_BYTES + raw_attr_bytes(0, 1, 0)) - row);
    assert_eq!(at(" N=\"abc\""), taken(price::ROW_BYTES + arc_str(3)) - row);
    let indexed = |attrs: &str| shape(1, &format!("<Section N=\"Geometry\" IX=\"0\"><Row{attrs}/></Section>"));
    assert_eq!(extra(&indexed(" IX=\"1\""), &indexed(" IX=\"01\"")), taken(price::ROW_BYTES + raw_attr_bytes(0, 2, 2)) - row);
}

// ------------------------------------------------------------------------------------------------------------------------------- kept markup

#[test]
fn an_unknown_element_is_charged_its_place_in_the_list_of_kept_nodes_and_the_markup_it_holds() {
    let kept = |n: usize, one: &str| shape(1, &one.repeat(n));
    let attrs = price::block(16 + 24) + heap(price::PAIR_BYTES) + arc_str(1) + arc_str(1);
    for (one, inside) in [("<z/>", arc_str(1)), ("<zz a=\"1\"/>", arc_str(2) + attrs), ("<z>text</z>", arc_str(1) + heap(48) + arc_str(4))] {
        for have in [0usize, 1, 3, 4] {
            assert_eq!(extra(&kept(have, one), &kept(have + 1, one)), taken(push_bytes(have, PLACED_BYTES) + inside), "{one} after {have}");
        }
    }
}

#[test]
fn text_a_comment_and_an_instruction_between_the_children_of_a_shape_are_kept_and_charged() {
    assert_eq!(extra(&shape(1, ""), &shape(1, "<!--abc-->")), taken(push_bytes(0, PLACED_BYTES) + arc_str(3)));
    assert_eq!(extra(&shape(1, ""), &shape(1, "<?go to?>")), taken(push_bytes(0, PLACED_BYTES) + arc_str(2) + arc_str(2)));
    assert_eq!(extra(&shape(1, ""), &shape(1, "some words")), taken(push_bytes(0, PLACED_BYTES) + arc_str(10)));
    // Blank text between elements is layout, and is neither kept nor charged.
    assert_eq!(extra(&shape(1, ""), &shape(1, "   \n  ")), 0);
}

// -------------------------------------------------------------------------------------------------------------------------------- shapes

#[test]
fn a_shape_is_charged_itself_its_sheet_its_id_in_the_table_and_its_place_in_the_list() {
    let shapes = |n: usize| (0..n).map(|i| format!("<Shape ID=\"{i}\"/>")).collect::<String>();
    // Every shape here has the same empty sheet: the first is the one the reader keeps (and charges its entry in the table of what was met), the others are it.
    for have in [0usize, 1, 3, 4, 8, 9] {
        let entry = if have == 0 { taken(SHARING_ENTRY_BYTES) } else { 0 };
        assert_eq!(extra(&shapes(have), &shapes(have + 1)), taken(cost::shape_bytes(have)) + entry, "the shape after {have}");
    }
    assert_eq!(cost::shape_bytes(0), SHAPE_BYTES + push_bytes(0, POINTER_BYTES));
}

#[test]
fn a_shape_that_keeps_names_and_attributes_is_charged_for_each_string() {
    let with = |attrs: &str| shape_with(1, attrs, "");
    let base = charged_page(&with(""));
    let at = |attrs: &str| charged_page(&with(attrs)) - base;
    let this = cost::shape_bytes(0);
    assert_eq!(at(" Name=\"Box\" NameU=\"Box\""), taken(this + arc_str(3) + arc_str(3)) - taken(this));
    assert_eq!(at(" Type=\"Group\""), 0);
    // A number that reads as written is typed; one that does not is kept.
    assert_eq!(at(" Master=\"7\""), 0);
    assert_eq!(at(" Master=\"07\""), taken(this + raw_attr_bytes(0, 6, 2)) - taken(this));
    assert_eq!(at(" Del=\"1\""), 0);
    assert_eq!(at(" Del=\"0\""), taken(this + raw_attr_bytes(0, 3, 1)) - taken(this));
    assert_eq!(at(" u=\"abc\" v=\"d\""), taken(this + raw_attr_bytes(0, 1, 3) + raw_attr_bytes(1, 1, 1)) - taken(this));
    assert_eq!(at(" LineStyle=\"0\""), 0);
    assert_eq!(at(" LineStyle=\"x\""), taken(this + raw_attr_bytes(0, 9, 1)) - taken(this));
}

#[test]
fn the_text_of_a_shape_is_charged_the_text_the_list_of_its_parts_and_each_part() {
    let text = |inner: &str| shape(1, &format!("<Text>{inner}</Text>"));
    let base = charged_page(&text(""));
    let at = |inner: &str| charged_page(&text(inner)) - base;
    // The text and the list of its parts are one charge, made before the parts are read; each part is another.
    let list = taken(cost::TEXT_BYTES + cost::text_parts_bytes(1)) - taken(cost::TEXT_BYTES + cost::text_parts_bytes(0));
    assert_eq!(at("abc"), list + taken(cost::text_part_bytes(3)));
    assert_eq!(at("<cp IX=\"0\"/>"), list);
    assert_eq!(at("<fld IX=\"0\">42</fld>"), list + taken(cost::text_part_bytes(2) + heap(2)));
    assert_eq!(at("<!--c-->"), list + taken(cost::text_part_bytes(1)));
    assert_eq!(at("<?t d?>"), list + taken(cost::text_part_bytes(1) + cost::text_part_bytes(1)));
    // An element that is no marker is kept whole.
    assert_eq!(at("<b>x</b>"), list + taken(arc_str(1) + heap(48) + arc_str(1)));
}

#[test]
fn a_datum_of_a_shape_is_a_string_of_its_own() {
    assert_eq!(extra(&shape(1, ""), &shape(1, "<Data1>abcd</Data1>")), taken(arc_str(4)));
}

// ------------------------------------------------------------------------------------------------------------------------------ the rest

#[test]
fn a_connect_is_charged_its_place_its_names_and_the_attributes_it_keeps() {
    let connects = |n: usize, one: &str| {
        report_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, "")).connects(&one.repeat(n))).build()).stats.memory_bytes
    };
    let row = "<Connect FromSheet=\"1\" FromCell=\"BeginX\" ToSheet=\"1\" ToCell=\"PinX\"/>";
    for have in [0usize, 1, 3, 4] {
        assert_eq!(
            connects(have + 1, row) - connects(have, row),
            taken(cost::connect_bytes(have) + arc_str(6) + arc_str(4)),
            "the connect after {have}"
        );
    }
    let odd = "<Connect FromSheet=\"01\" FromCell=\"BeginX\" ToSheet=\"1\" ToCell=\"PinX\" k=\"v\"/>";
    assert_eq!(
        connects(1, odd) - connects(0, odd),
        taken(cost::connect_bytes(0) + raw_attr_bytes(0, 9, 2) + raw_attr_bytes(0, 1, 1) + arc_str(6) + arc_str(4))
    );
}

#[test]
fn a_style_a_master_and_a_page_are_charged_their_rows_and_the_strings_they_keep() {
    let charged = |b: &VsdxBuilder| report_of(&b.build()).stats.memory_bytes;
    let have = report_of(&VsdxBuilder::new().build()).stats.styles;
    let styles = "<StyleSheet ID=\"100\" Name=\"S\"/>";
    let one = charged(&VsdxBuilder::new().more_styles(styles)) - charged(&VsdxBuilder::new());
    assert_eq!(one, taken(cost::STYLE_BYTES + arc_str(1)) + taken(push_bytes(have, POINTER_BYTES)));
    let master = |name: &str| charged(&VsdxBuilder::new().master(MasterSpec::new(5, name)));
    let page = |name: &str| charged(&VsdxBuilder::new().page(PageSpec::new(2, name)));
    // The name is held twice in the row (`Name` and `NameU`), a string of its own each.
    let long = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(master(long) - master("a"), taken(cost::MASTER_BYTES + 2 * arc_str(26)) - taken(cost::MASTER_BYTES + 2 * arc_str(1)));
    assert_eq!(page(long) - page("a"), taken(cost::PAGE_BYTES + 2 * arc_str(26)) - taken(cost::PAGE_BYTES + 2 * arc_str(1)));
}

#[test]
fn a_relationship_is_charged_a_row() {
    let page = |rels: usize| {
        let mut p = PageSpec::new(1, "P").shapes(&shape(1, ""));
        for k in 0..rels {
            p = p.rel(&format!("rId{}", k + 10), "urn:filler", "x.bin");
        }
        report_of(&VsdxBuilder::new().page(p).build()).stats.memory_bytes
    };
    // Each row is charged a row of the part's own (`REL_ROW_BYTES`), and, since none of them says the whole of a field of the model, is held (`held.rs`): the
    // package's three rows and the document's two come before them in the list the model holds, so the first of these is the sixth.
    let held = |row: usize| taken(cost::held_rel_bytes(5 + row, "rId10".len(), "urn:filler".len(), "x.bin".len()));
    assert_eq!(page(5) - page(1), taken(5 * cost::REL_ROW_BYTES) - taken(cost::REL_ROW_BYTES) + held(1) + held(2) + held(3) + held(4));
}

#[test]
fn a_picture_is_charged_its_bytes_as_they_are_and_what_is_built_around_them_with_the_margin() {
    let one = "<Shape ID=\"1\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId1\"/></ForeignData></Shape>";
    let file = |n: usize| {
        let page = PageSpec::new(1, "P").shapes(one).rel("rId1", &format!("{OD}image"), "../media/i.png");
        VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/i.png", "image/png", vec![7u8; n])).build()
    };
    let (small, big) = (report_of(&file(1_000)).stats, report_of(&file(5_000)).stats);
    // Four thousand bytes more picture are four thousand bytes more charged, with no margin on them (the allocator rounds the 16 of the counts in).
    assert_eq!(big.memory_bytes - small.memory_bytes, arc_str(5_000) - arc_str(1_000));
    assert_eq!(small.media_bytes, 1_000);
    // And the rest of it is what the shape holds of it (`ForeignData`, its attributes, the id of its relationship), the row of the page's relationships, and the
    // `Media` with its names and its entry in the table, each charge with the margin.
    let without = charged_page(&shape_with(1, " Type=\"Foreign\"", ""));
    let around = taken(cost::FOREIGN_BYTES + cost::all_attrs_bytes([("ForeignType", "Bitmap")].into_iter()))
        + taken(arc_str(4))
        + taken(cost::REL_ROW_BYTES)
        + arc_str(1_000)
        + taken(cost::picture_bytes(0, "/visio/media/i.png".len(), "image/png".len()));
    assert_eq!(small.memory_bytes - without, around);
}

// ------------------------------------------------------------------------------------------------------------------------------ the pool

#[test]
fn the_tree_of_a_part_is_held_while_the_part_is_read_and_given_back_after() {
    let unknown = shape(1, &"<z/>".repeat(20_000));
    let one = report_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&unknown)).build()).stats;
    // The model is what is kept; the tree is held beside it at the peak and is not kept.
    assert!(one.max_tree_bytes > 0 && one.memory_bytes > 0);
    assert!(one.memory_peak >= one.memory_bytes + one.max_tree_bytes, "{one:?}");
    assert!(one.memory_peak < one.memory_bytes + 2 * one.max_tree_bytes, "{one:?}");
    // Two parts of the same size: the first tree is given back before the second is parsed, so the peak has one tree in it, and the model has both.
    let page = |k: u32| PageSpec::new(k, "P").shapes(&unknown);
    let two = report_of(&VsdxBuilder::new().page(page(1)).page(page(2)).build()).stats;
    assert!(two.memory_bytes * 10 > one.memory_bytes * 19, "{one:?} {two:?}");
    assert!(two.memory_peak < two.memory_bytes + 2 * two.max_tree_bytes, "{two:?}");
    assert_eq!(two.max_tree_bytes, one.max_tree_bytes);
}

#[test]
fn a_read_past_the_pool_is_refused_in_words_with_the_budget_and_no_document() {
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&"<Shape ID=\"1\"/>".repeat(1_000))).build();
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    // The file, the package and the room a growing list needs are the budget's: a budget they use up leaves no pool, and the first tree is refused.
    for max_memory in [0, room - 1, room, room + 1_000, room + stats.memory_peak / 4] {
        let (result, report) = read_partial(&bytes, &Limits { max_memory, ..Limits::default() });
        let e = result.unwrap_err();
        assert_eq!((e.class(), e.kind()), ("too_much_memory", diacraft_vsdx::ErrorKind::TooBig), "{max_memory}");
        assert_eq!(e.to_string(), format!("reading the file would take more than {max_memory} bytes of memory: it is too large to read here"));
        assert!(report.refused_at.is_some());
    }
    // A budget with room for everything reads it, and the same file is charged the same.
    let (_, again) = read_with(&bytes, &Limits { max_memory: room + stats.memory_peak * 4, ..Limits::default() });
    assert_eq!(again.stats.memory_bytes, stats.memory_bytes);
}

#[test]
fn a_picture_the_pool_cannot_hold_is_left_out_and_the_read_goes_on() {
    let one = "<Shape ID=\"1\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId1\"/></ForeignData></Shape>";
    // A shape of five cells of a megabyte each before the picture: the tree of the page holds them while the page is read, and the model keeps them, so the pool cannot hold
    // the picture beside both, and has room for it when the tree is given back at the end, which is the room the move of the part the picture is held as needs for its one
    // copy (F1).
    let value = "9".repeat(1_000_000);
    let cells: String = (0..5).map(|i| format!("<Cell N=\"C{i}\" V=\"{value}\"/>")).collect();
    let bytes = {
        let page = PageSpec::new(1, "P").shapes(&format!("{}{one}", shape(2, &cells))).rel("rId1", &format!("{OD}image"), "../media/i.png");
        VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/i.png", "image/png", vec![7u8; 3_000_000])).build()
    };
    let stats = report_of(&bytes).stats;
    assert_eq!(stats.media_bytes, 3_000_000);
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    // A pool that holds all but the picture: the picture is left out of the model, with a warning, and the shape shows nothing; the part is held.
    let (doc, report) = read_with(&bytes, &Limits { max_memory: room + stats.memory_peak - 1_600_000, ..Limits::default() });
    assert_eq!((report.stats.media_bytes, report.warning_count("media_over_budget")), (0, 1));
    assert!(doc.pages.get(0).unwrap().shapes.get(1).unwrap().foreign.as_ref().unwrap().picture.is_none());
    // The picture the model could not copy is a part the document holds, and its bytes are the package's: held, and not charged to the pool a second time.
    assert_eq!((report.held_parts, report.held_bytes), (1, 3_000_000));
    // A pool with less is refused in words: the one copy of the part the move makes has no room, or the page has none.
    let (result, _) = read_partial(&bytes, &Limits { max_memory: room + stats.memory_peak - 3_600_000, ..Limits::default() });
    assert_eq!(result.expect_err("no room").class(), "too_much_memory");
    // And one that holds it, reads it.
    let (_, report) = read_with(&bytes, &Limits { max_memory: room + stats.memory_peak + 100_000, ..Limits::default() });
    assert_eq!((report.stats.media_bytes, report.warning_count("media_over_budget")), (3_000_000, 0));
}

#[test]
fn the_themes_and_the_diacraft_part_are_read_within_what_the_pool_has_left() {
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements/></a:theme>"#;
    // Two thousand elements in the part: its tree and its copy cost a thousand times what the bytes of the part do, which is what the pool is short of below, and the part
    // left out is held as the bytes it is in what is left.
    let data = format!(
        "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\">{}</dc:Data></SolutionXML>",
        "<a/>".repeat(2_000)
    );
    let bytes = VsdxBuilder::new().theme(theme.as_bytes()).solution("DiaCraft", &data).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 1);
    assert!(doc.diacraft.is_some());
    // The theme is charged three times its bytes and a few kilobytes, with the margin.
    let without = report_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build()).stats.memory_bytes;
    assert!(report.stats.memory_bytes - without >= taken(theme.len() as u64 * 3 + 4096), "{}", report.stats.memory_bytes - without);
    // A pool that is spent when the DiaCraft part is reached reads the drawing and the theme, and leaves the part out.
    let room = report.stats.file_bytes + report.stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let (doc, report) = read_with(&bytes, &Limits { max_memory: room + report.stats.memory_peak - 1_000, ..Limits::default() });
    assert_eq!(doc.themes.len(), 1);
    assert!(doc.diacraft.is_none());
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(_))), "{:?}", report.diacraft);
}

#[test]
fn the_tree_of_the_document_part_is_given_back_before_the_masters_and_the_pages_are_parsed() {
    // A document part of 20,000 unknown elements: its tree is held while the part is read and let go before the pages are, so the pool at the end holds the model of the
    // elements the document keeps and not their tree, and the peak held the tree beside the model.
    let bytes = VsdxBuilder::new().document_extra(&"<z/>".repeat(20_000)).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    let stats = report_of(&bytes).stats;
    assert!(stats.max_tree_bytes > 20_000 * 80, "{stats:?}");
    // The peak is the model and the tree, and the end is the model alone: the tree was let go (a file that held it to the end has no such difference).
    assert!(stats.memory_peak > stats.memory_bytes + stats.max_tree_bytes / 2, "{stats:?}");
    assert!(stats.memory_bytes < 3 * 20_000 * 112, "the model of 20,000 kept elements is some 100 bytes each: {stats:?}");
}

#[test]
fn a_part_that_is_no_masters_index_or_pages_index_gives_its_tree_back_all_the_same() {
    // The masters and pages parts are reached by their relationships and their roots are not `Masters` and `Pages`: read as unknown, kept in the bytes, warned of, and
    // their trees are given back, so that nothing of them is held to the end.
    let filler = "<z/>".repeat(20_000);
    let wrong = |root: &str| format!("<{root} xmlns=\"{}\">{filler}</{root}>", diacraft_testkit::package::NS_2012);
    let base = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).master(MasterSpec::new(5, "M"));
    let plain = report_of(&base.build()).stats.memory_bytes;
    let pkg = |builder: &VsdxBuilder, part: &str, content_type: &str, xml: String| builder.pkg().part(part, content_type, xml);
    let _ = pkg;
    let with = |part_xml: String, which: &str| {
        let mut b = VsdxBuilder::new();
        if which == "masters" {
            b = b.master(MasterSpec::new(5, "M")).page(PageSpec::new(1, "P").shapes(&shape(1, "")));
            report_of(
                &b.pkg()
                    .map_parts(|name, bytes| {
                        if name.ends_with("/masters/masters.xml") {
                            *bytes = part_xml.clone().into_bytes()
                        }
                    })
                    .build(),
            )
        } else {
            b = b.page(PageSpec::new(1, "P").shapes(&shape(1, "")));
            report_of(
                &b.pkg()
                    .map_parts(|name, bytes| {
                        if name.ends_with("/pages/pages.xml") {
                            *bytes = part_xml.clone().into_bytes()
                        }
                    })
                    .build(),
            )
        }
    };
    for (xml, which) in [(wrong("NotMasters"), "masters"), (wrong("NotPages"), "pages")] {
        let report = with(xml, which);
        assert_eq!(report.warning_count("part_unexpected_root"), 1, "{which}");
        assert!(report.stats.max_tree_bytes > 20_000 * 80, "{which}: {:?}", report.stats);
        // What is held at the end is the model of what the file has: no tree.
        assert!(report.stats.memory_bytes < report.stats.max_tree_bytes / 2 + plain, "{which}: {:?}", report.stats);
    }
}

#[test]
fn a_part_whose_tree_would_fit_at_its_real_size_but_not_at_the_dearest_is_refused_before_it_is_made() {
    // The parse is bounded by the items the pool can hold at 368 bytes each, less the room its text can take. A part of 40,000 elements that nothing keeps (children of the root that the reader skips)
    // has a tree of some 150 bytes an item and a model of nothing: a pool that holds the tree as it is, and not 40,000 items at 368 bytes, refuses it as the tree is
    // built, in words, and the tree is never made.
    let part = format!("<PageContents xmlns=\"{}\">{}</PageContents>", diacraft_testkit::package::NS_2012, "<z/>".repeat(40_000));
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build();
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let tree = stats.max_tree_bytes;
    assert!(tree > 40_000 * 100 && tree < 40_000 * 180, "{tree}");
    let tight = Limits { max_memory: room + tree + 500_000, ..Limits::default() };
    assert!(tree + 500_000 < 40_000 * cost::TREE_ITEM_BYTES);
    let (result, _) = read_partial(&bytes, &tight);
    assert_eq!(result.unwrap_err().class(), "too_much_memory");
    // The pool with room for the dearest tree of that many items reads it, and a pool that holds the tree and no more than 40,000 items at the dearest less a thousand
    // refuses it.
    let (_, report) = read_with(&bytes, &Limits { max_memory: room + 40_010 * cost::TREE_ITEM_BYTES + 1_000_000, ..Limits::default() });
    assert_eq!(report.stats.max_tree_bytes, tree);
    let (result, _) = read_partial(&bytes, &Limits { max_memory: room + 39_000 * cost::TREE_ITEM_BYTES, ..Limits::default() });
    assert_eq!(result.unwrap_err().class(), "too_much_memory");
}

#[test]
fn what_a_small_file_is_charged_is_pinned_where_a_word_is_eight_bytes() {
    // Every charge of the document, the page, the index and the parts is in these totals; a charge taken out or put twice moves one. The figures move with the model's
    // types (`the_figures_the_prices_are_made_of_are_these...` says when one does): measure them again then.
    if cfg!(target_pointer_width = "64") {
        let bare = report_of(&VsdxBuilder::new().build()).stats.memory_bytes;
        let seed = report_of(&crate::fuzz::seed_package().build()).stats.memory_bytes;
        // 4,923 and 35,836 before a change. What the file holds beside the model now is in these: the package's three relationships and the document's two (the
        // rows no field says: 1.6 KB), the rest of the property parts (`Application`), the `xml:space` of the roots of the document and the two indexes (three slots), and
        // the `VsdxHeld` of the document; the seed adds a slot and a part name to each page and master, the rows of the shape and the master that have a relationship, and
        // the solutions index and its part, which another program wrote, so they are held as parts. The seed's part of DiaCraft's is rebuilt by the model and not held beside
        // it, and a shape that holds kept nodes in its own element and in its sheet's holds the order they came in (`KeptOrder`: the key, the entry and the slot's record).
        // merged from main adds 540 to the seed: the reader's table of the sections and sheets it has met (4,923 and 36,376 on main).
        // Prices every string and buffer at the law of the counted memory tests, 24 bytes over what is asked and not glibc's 8 (`price::block`), and a
        // face's attributes as the list they are pushed into: 9,189 and 43,522 before it.
        assert_eq!((bare, seed), (9_819, 45_932));
    }
}

#[test]
fn the_attributes_of_a_style_a_master_and_a_page_row_that_a_field_holds_cost_nothing_and_any_other_is_kept() {
    let charged = |b: &VsdxBuilder| report_of(&b.build()).stats.memory_bytes;
    let raw = |have: usize, name: usize, value: usize| raw_attr_bytes(have, name, value);
    // A style: a link that reads as written is typed, one that does not is kept.
    let style = |attrs: &str| charged(&VsdxBuilder::new().more_styles(&format!("<StyleSheet ID=\"100\"{attrs}/>")));
    let (bare, this) = (style(""), taken(cost::STYLE_BYTES));
    assert_eq!(style(" LineStyle=\"0\"") - bare, 0);
    assert_eq!(style(" LineStyle=\"x\"") - bare, taken(cost::STYLE_BYTES + raw(0, 9, 1)) - this);
    assert_eq!(style(" FillStyle=\"00\" q=\"zz\"") - bare, taken(cost::STYLE_BYTES + raw(0, 9, 2) + raw(1, 1, 2)) - this);
    // A master: the `ID` and the pattern flags that read as written are typed, the `UniqueID` is held twice, any other attribute is kept.
    let master = |attrs: &str| charged(&VsdxBuilder::new().master(MasterSpec::new(5, "M").attrs(attrs)));
    let (bare, this) = (master(""), taken(cost::MASTER_BYTES + 2 * arc_str(1)));
    let guid = "{12345678-1234-1234-1234-123456789ABC}";
    assert_eq!(master(" PatternFlags=\"3\"") - bare, 0);
    assert_eq!(master(" PatternFlags=\"x\"") - bare, taken(cost::MASTER_BYTES + 2 * arc_str(1) + raw(0, 12, 1)) - this);
    assert_eq!(
        master(&format!(" UniqueID=\"{guid}\"")) - bare,
        taken(cost::MASTER_BYTES + 2 * arc_str(1) + arc_str(guid.len()) + heap(guid.len())) - this
    );
    assert_eq!(master(" q=\"zz\"") - bare, taken(cost::MASTER_BYTES + 2 * arc_str(1) + raw(0, 1, 2)) - this);
    // A page: `Background` is typed when it is `1` and kept as written otherwise, and so is a back page that does not read as written.
    let page = |attrs: &str| charged(&VsdxBuilder::new().page(PageSpec::new(2, "P").attrs(attrs)));
    let (bare, this) = (page(""), taken(cost::PAGE_BYTES + 2 * arc_str(1)));
    assert_eq!(page(" Background=\"1\"") - bare, 0);
    assert_eq!(page(" Background=\"true\"") - bare, taken(cost::PAGE_BYTES + 2 * arc_str(1) + raw(0, 10, 4)) - this);
    assert_eq!(page(" BackPage=\"3\"") - bare, 0);
    assert_eq!(page(" BackPage=\"03\"") - bare, taken(cost::PAGE_BYTES + 2 * arc_str(1) + raw(0, 8, 2)) - this);
}

#[test]
fn a_colour_and_a_face_name_are_charged_a_row_of_their_table_and_the_strings_they_keep() {
    let charged = |b: &VsdxBuilder| report_of(&b.build()).stats.memory_bytes;
    let none = charged(&VsdxBuilder::new());
    assert_eq!(charged(&VsdxBuilder::new().colors("<ColorEntry IX=\"24\" RGB=\"#123456\"/>")) - none, taken(cost::TABLE_ROW_BYTES + arc_str(7)));
    // The name of a face is held, and the attributes besides it are kept (in the list they are pushed into one at a time, with room for four at the first: its
    // `Arc`, its list and its strings; the price was the list made whole).
    let face = |attrs: &str| charged(&VsdxBuilder::new().face_names(&format!("<FaceName NameU=\"Sans\"{attrs}/>"))) - none;
    let row = cost::TABLE_ROW_BYTES + arc_str(4);
    assert_eq!(face(""), taken(row));
    assert_eq!(face(" Panose=\"1\""), taken(row + cost::pushed_attrs_bytes([("Panose", "1")].into_iter())));
    assert_eq!(face(" Panose=\"1\" Unicode=\"abc\""), taken(row + cost::pushed_attrs_bytes([("Panose", "1"), ("Unicode", "abc")].into_iter())));
    // A list pushed into holds the room of four at its first: a face of one attribute is charged that, and not the list made whole of one.
    assert!(cost::pushed_attrs_bytes([("Panose", "1")].into_iter()) > cost::all_attrs_bytes([("Panose", "1")].into_iter()));
}

// ------------------------------------------------------------------------------------------------------------------ a tree that cannot be held is not made

/// A page of `n` chains of 190 nested elements, one child each: the tree that costs the most for the items it has (335 bytes an item, where the tree of a drawing is some
/// 140), because a list of children has room for four and an element that has one child holds the room of four.
fn chains_bytes(n: usize) -> Vec<u8> {
    let chain = format!("{}{}", "<a>".repeat(190), "</a>".repeat(190));
    let part = format!(
        "<PageContents xmlns=\"{}\"><Shapes><Shape ID=\"1\">{}</Shape></Shapes></PageContents>",
        diacraft_testkit::package::NS_2012,
        chain.repeat(n)
    );
    VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build()
}

/// What the largest tree a read made is, and what the pool held at the most: **a tree that was made was charged**, which only a parse bounded before the tree is made
/// can promise (a tree made and then found not to fit is the abort of a process that has no memory for it, and the check after is too late).
fn assert_every_tree_made_was_held(stats: &diacraft_vsdx::Stats, what: &str) {
    assert!(
        stats.max_tree_bytes <= stats.memory_peak,
        "{what}: a tree of {} bytes was made, and the pool held {} at the most: {stats:?}",
        stats.max_tree_bytes,
        stats.memory_peak
    );
}

#[test]
fn a_tree_the_pool_cannot_hold_is_never_made_whatever_the_pool_is() {
    // 100 chains of 190 elements: 19,000 items and a tree of 6.4 MB. For every pool from nothing to more than the read needs, the read is a read or a refusal in words,
    // and no tree was made that the pool did not hold beside what it held before it (the parse was bounded at 224 bytes an item, and a chain is 335, so a
    // pool between the two let the tree be made and refused it after).
    let bytes = chains_bytes(100);
    let stats = report_of(&bytes).stats;
    // The container's lists of children are made tight now (an eighth of room and not a doubling), so a chain costs about a third of what it cost: 110 bytes an item and not 335.
    assert!(stats.max_tree_bytes > 19_000 * 100, "{stats:?}");
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let (mut read, mut refused) = (0, 0);
    for step in 0..=80u64 {
        let (result, report) = read_partial(&bytes, &Limits { max_memory: room + step * 250_000, ..Limits::default() });
        assert_every_tree_made_was_held(&report.stats, &format!("a pool of {} bytes", step * 250_000));
        match result {
            Ok(_) => read += 1,
            Err(e) => {
                assert_eq!(e.class(), "too_much_memory");
                refused += 1;
            }
        }
    }
    assert!(read > 10 && refused > 10, "the pools cover both ends: {read} reads, {refused} refusals");
}

#[test]
fn the_bound_of_a_parse_is_the_items_at_the_dearest_and_the_text_it_can_take_and_a_pool_under_it_refuses_the_part_before_it_is_parsed() {
    let bytes = chains_bytes(100);
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let page = bytes_of_page(&bytes);
    let size = diacraft_opc::xml::parse_size(&page);
    let items = stats.max_xml_items as u64;
    let need = cost::tree_bound(items, size);
    assert_eq!(need, items * cost::TREE_ITEM_BYTES + 2 * page.len() as u64);
    // The page is the largest part, and the model it makes is charged after its tree: a pool that holds the bound reads the page; one that is under the bound by the
    // model's own bytes and the room the other parts take does not hold the tree of 19,000 items at 368 bytes, and is refused before it is made.
    let (_, report) = read_with(&bytes, &Limits { max_memory: room + need + 2 * stats.memory_bytes, ..Limits::default() });
    assert_eq!(report.stats.max_tree_bytes, stats.max_tree_bytes);
    let (result, report) = read_partial(&bytes, &Limits { max_memory: room + need / 2, ..Limits::default() });
    assert_eq!(result.unwrap_err().class(), "too_much_memory");
    assert!(report.stats.max_tree_bytes < 50_000, "the tree of the page was not made: {:?}", report.stats);
}

/// The bytes of the part that holds the page's shapes, from the package.
fn bytes_of_page(file: &[u8]) -> Vec<u8> {
    let package = diacraft_opc::Package::read(file, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|p| p.ends_with("/pages/page1.xml")).expect("a page part").to_owned();
    package.part(&name).expect("the page").to_vec()
}

/// `count` characters of the CJK block as a page of the drawing in UTF-16 with a byte order mark, with one line break written as CR LF.
fn utf16_page_bytes(count: usize) -> Vec<u8> {
    let text = format!("<PageContents xmlns=\"{}\">{}\r\nend</PageContents>", diacraft_testkit::package::NS_2012, "\u{4E00}".repeat(count));
    let mut part = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        part.extend_from_slice(&unit.to_le_bytes());
    }
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").part("<PageContents/>"))
        .pkg()
        .map_parts(|name, bytes| {
            if name.ends_with("pages/page1.xml") {
                bytes.clone_from(&part);
            }
        })
        .build()
}

#[test]
fn a_part_in_utf16_holds_the_copy_it_is_decoded_into_and_the_room_its_text_takes_in_the_pool_before_it_is_parsed() {
    // 100,000 characters of three bytes in UTF-8 and two in UTF-16: a part of 200 KB that the parse decodes into a string of 300 KB, and whose text can be held in the tree
    // up to twice (a string that grows by doubling): 900 KB of the pool beside the items, and not one byte of it was counted.
    let bytes = utf16_page_bytes(100_000);
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let page = bytes_of_page(&bytes);
    assert!(page.starts_with(&[0xFF, 0xFE]) && page.len() > 200_000, "{}", page.len());
    let size = diacraft_opc::xml::parse_size(&page);
    assert_eq!((size.text_bytes, size.copy_bytes), ((page.len() as u64 - 2) * 3 / 2, (page.len() as u64 - 2) * 3 / 2));
    let fixed = cost::parse_fixed_bytes(size);
    assert_eq!(fixed, 3 * size.text_bytes);
    // A pool that cannot hold the copy and the room is refused before anything is made of the part; one that holds them and more reads it.
    for pool in [0, fixed / 2, fixed - 1] {
        let (result, report) = read_partial(&bytes, &Limits { max_memory: room + pool, ..Limits::default() });
        assert_eq!(result.unwrap_err().class(), "too_much_memory", "a pool of {pool}");
        assert!(report.stats.max_tree_bytes < 50_000, "a pool of {pool}: the tree of the page was not made: {:?}", report.stats);
    }
    let (_, report) = read_with(&bytes, &Limits { max_memory: room + fixed + 1_000_000, ..Limits::default() });
    assert!(report.stats.max_tree_bytes > 300_000, "{:?}", report.stats);
    assert_every_tree_made_was_held(&report.stats, "a pool that holds the part");
}

/// A package whose document has `rows` relationships of a kind nothing reads, to parts that are not there.
fn many_relationships(rows: usize) -> Vec<u8> {
    let pkg = (0..rows).fold(Pkg::new(), |pkg, i| {
        pkg.rel("/visio/document.xml", &format!("rId{}", 1_000 + i), "http://example.com/relationships/other", &format!("other/part{i}.xml"))
    });
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).with(pkg).build()
}

#[test]
fn the_tree_of_a_relationships_part_is_bounded_by_the_pool_as_every_parse_is() {
    // 5,000 relationships: a part of 600 KB, 20,000 items, a tree of 2.8 MB that `opc` makes and lets go inside its own read, and rows that are charged (512 bytes each,
    // the model's). The tree was bounded by `opc`'s limits and by nothing of the pool: a pool of 5 MB read it and held 2.8 MB more than it counted.
    let bytes = many_relationships(5_000);
    let stats = report_of(&bytes).stats;
    assert!(stats.max_rels_rows >= 5_000, "{stats:?}");
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let rels = stats.max_rels_bytes;
    let items = 1 + 4 * stats.max_rels_rows as u64;
    let need = items * cost::TREE_ITEM_BYTES + 2 * rels;
    // The pool must hold the tree at the dearest beside what the read holds when it reaches the part: the 5,000 rows are charged after it.
    let (result, _) = read_partial(&bytes, &Limits { max_memory: room + need / 2, ..Limits::default() });
    assert_eq!(result.unwrap_err().class(), "too_much_memory");
    let (_, report) = read_with(&bytes, &Limits { max_memory: room + need + 2 * stats.memory_bytes, ..Limits::default() });
    assert_eq!(report.stats.memory_bytes, stats.memory_bytes);
}

#[test]
fn the_tree_of_a_theme_part_is_bounded_by_the_pool_and_a_theme_whose_tree_does_not_fit_is_kept_as_its_bytes_and_said_to_be_unreadable() {
    // A theme of 20,000 elements: a part of 120 KB and a tree of some 2.6 MB. It is read when the pool holds the tree at the dearest. When the pool does not, the part is
    // kept as its bytes (as every theme that cannot be read is) and the report says it could not be read, and the tree is not made.
    let theme = format!(
        "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"T\"><a:themeElements/>{}</a:theme>",
        "<a:x/>".repeat(20_000)
    );
    let with = VsdxBuilder::new().theme(theme.as_bytes()).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    let (doc, report) = read_ok(&with);
    assert_eq!((doc.themes.len(), report.warning_count("theme_unreadable")), (1, 0));
    let stats = report.stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let need = 20_004 * cost::TREE_ITEM_BYTES + 2 * theme.len() as u64;
    // A pool that holds the tree at 300 bytes an item (what the bound of 224 let through) and not at 368.
    let (doc, report) = read_with(&with, &Limits { max_memory: room + 20_004 * 300 + stats.memory_bytes, ..Limits::default() });
    assert_eq!((doc.themes.len(), report.warning_count("theme_unreadable"), report.warning_count("themes_over_budget")), (1, 1, 0));
    assert_eq!(report.stats.memory_bytes, stats.memory_bytes, "the bytes of the part are kept and charged as they are");
    // With room for the bound, it is read.
    let (doc, report) = read_with(&with, &Limits { max_memory: room + need + stats.memory_bytes, ..Limits::default() });
    assert_eq!((doc.themes.len(), report.warning_count("theme_unreadable")), (1, 0));
}

#[test]
fn a_theme_part_whose_text_the_pool_cannot_hold_beside_its_charge_is_left_out_and_what_was_charged_for_it_is_given_back() {
    // A theme of a megabyte: charged three times its bytes and a few kilobytes, with the margin, and its tree can hold the text up to twice. A pool that has room for the
    // charge and not for the charge and the text beside it takes the charge and then finds the parse cannot be bounded: the part is left out, in words, and the charge is
    // given back (a charge taken for a part that was then not read stayed charged).
    let theme = format!(
        "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"T\"><a:themeElements/><!--{}--></a:theme>",
        "c".repeat(1_000_000)
    );
    let with = VsdxBuilder::new().theme(theme.as_bytes()).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    let (doc, report) = read_ok(&with);
    let stats = report.stats;
    assert_eq!(doc.themes.len(), 1);
    let charge = taken(theme.len() as u64 * 3 + 4096);
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    // The model of the file less the theme, and four times the part: room for the charge (3.4 times) and not for the 2 times the text beside it.
    let (doc, report) = read_with(&with, &Limits { max_memory: room + (stats.memory_bytes - charge) + 4 * theme.len() as u64, ..Limits::default() });
    assert_eq!((doc.themes.len(), report.warning_count("themes_over_budget")), (0, 1));
    // What was charged for the theme is given back, and the part the model left out is a part the document holds (before, only the file held it), charged its
    // bytes once and the bookkeeping of a part.
    assert_eq!((report.held_parts, report.held_bytes), (1, theme.len() as u64));
    let now = report.stats.memory_bytes - (stats.memory_bytes - charge);
    assert!((arc_str(theme.len())..arc_str(theme.len()) + 1_000).contains(&now), "{now} for a part of {} bytes", theme.len());
}

#[test]
fn the_tree_of_the_solutions_index_is_held_in_the_pool_while_the_solution_is_read_beside_it() {
    // An index of 20,000 rows before the DiaCraft one: a tree of some 3 MB that is alive while the solution's part is read, and was held by nothing (the part's own tree and
    // its copy were counted). The pool holds it now, and gives it back: the peak has it and the end has not.
    let data = "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"><a/></dc:Data></SolutionXML>";
    let pad: String = (0..20_000).map(|i| format!("<Solution Name=\"Pad{i}\"/>")).collect();
    let bytes = VsdxBuilder::new()
        .solution("DiaCraft", data)
        .page(PageSpec::new(1, "P").shapes(&shape(1, "")))
        .pkg()
        .map_parts(|name, bytes| {
            if name.ends_with("solutions/solutions.xml") {
                let text = String::from_utf8(std::mem::take(bytes)).expect("text");
                *bytes = text.replacen("<Solution ", &format!("{pad}<Solution "), 1).into_bytes();
            }
        })
        .build();
    let (doc, report) = read_ok(&bytes);
    assert!(doc.diacraft.is_some(), "{:?}", report.diacraft);
    let stats = report.stats;
    assert!(stats.memory_peak >= stats.memory_bytes + 20_000 * 100, "the index's tree is in the peak: {stats:?}");
    // And a pool that cannot hold it reads the drawing and leaves the part out, in words.
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let (doc, report) = read_with(&bytes, &Limits { max_memory: room + stats.memory_peak - 1_000_000, ..Limits::default() });
    assert!(doc.diacraft.is_none());
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable(_))), "{:?}", report.diacraft);
}

// -------------------------------------------------------------------------------------------------------------- a charge that is taken and not kept is given back

#[test]
fn what_was_charged_for_the_diacraft_part_and_was_not_kept_is_given_back() {
    // The part's tree is charged, and its copy, and then its items; a document that has no items left for the part leaves it out, in words, and the tree and the copy are
    // given back (they stayed charged to the end of the read). The part is the last thing the read keeps, so the items it needs are the last of the total.
    let data = "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"><a/><b/></dc:Data></SolutionXML>";
    let bytes = VsdxBuilder::new().solution("DiaCraft", data).page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    let (doc, report) = read_ok(&bytes);
    assert!(doc.diacraft.is_some(), "{:?}", report.diacraft);
    let full = report.stats;
    let tree = diacraft_opc::xml::parse(data.as_bytes()).expect("the solution part");
    let copy = taken(diacraft_sheetxml::price::kept_bytes(tree.child("Data").expect("the data")) + price::SEEN_ENTRY_BYTES);
    let (doc, report) = read_with(&bytes, &Limits { max_items: full.items - 1, ..Limits::default() });
    assert!(doc.diacraft.is_none());
    assert!(matches!(report.diacraft, Some(DiacraftPart::Unreadable("too many items to keep"))), "{:?}", report.diacraft);
    assert_eq!(report.warning_count("diacraft_not_kept"), 1);
    // What was charged for the part is given back, the tree and the copy; the index and the part are held as the bytes they are now, which the model does not rebuild
    // And are charged for that.
    assert_eq!(report.held_parts, 2);
    let held = report.stats.memory_bytes - (full.memory_bytes - copy);
    assert!((report.held_bytes..report.held_bytes + 3_000).contains(&held), "{held} for {} bytes", report.held_bytes);
    // The pool is not what refuses it here: the peak is the part's tree and its copy beside the model, and the end is the model and what the part left charged.
    assert!(report.stats.memory_peak + held > report.stats.memory_bytes + copy, "{:?}", report.stats);
}

#[test]
fn a_picture_past_the_media_cap_is_held_as_a_part_charged_its_bytes_once_and_what_the_model_would_have_charged_for_it_is_not() {
    // A picture the model leaves out (here past `max_media_bytes`, the cap and not the pool) is a part the document holds, charged its bytes once and the
    // bookkeeping of a part: its bytes are the package's, and the model's figure gains them when they move.
    let one = "<Shape ID=\"1\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId1\"/></ForeignData></Shape>";
    let bytes = {
        let page = PageSpec::new(1, "P").shapes(one).rel("rId1", &format!("{OD}image"), "../media/i.png");
        VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/i.png", "image/png", vec![7u8; 3_000_000])).build()
    };
    let (doc, whole) = read_ok(&bytes);
    assert_eq!((whole.held_parts, doc.parts.len()), (0, 0), "a picture in the model is not held as well");
    let (doc, held) = read_with(&bytes, &Limits { max_media_bytes: 1_000_000, ..Limits::default() });
    assert_eq!(held.warning_count("media_over_budget"), 1);
    assert_eq!((held.held_parts, held.held_bytes, held.stats.media_bytes), (1, 3_000_000, 0));
    assert!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.is_none());
    let part = doc.parts.get(0).unwrap();
    assert_eq!((&*part.name, &*part.media_type, part.bytes.len()), ("/visio/media/i.png", "image/png", 3_000_000));
    // The page's row of that picture is held, since no picture in the model says it.
    let rows: Vec<&str> = doc.vsdx.as_ref().unwrap().rels.iter().filter(|r| &*r.source == "/visio/pages/page1.xml").map(|r| &*r.id).collect();
    assert_eq!(rows, ["rId1"]);
    // Charged once: what the part is charged less what the picture was, which is the price of the `Media` and its names against the price of a `Part` and its names.
    let off = held.stats.memory_bytes.abs_diff(whole.stats.memory_bytes);
    assert!(off < 1_500, "the picture is held and charged once, {} against {}", held.stats.memory_bytes, whole.stats.memory_bytes);
}

#[test]
fn what_was_charged_for_a_picture_that_was_then_left_out_is_given_back() {
    // A picture is charged its bytes and then the rest of its price; a pool that holds the bytes and not the rest leaves the picture out, in words, and the bytes are given
    // back (they stayed charged, and the shapes after it found the pool short by a picture that was not there).
    let one = "<Shape ID=\"1\" Type=\"Foreign\"><ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId1\"/></ForeignData></Shape>";
    let bytes = {
        let page = PageSpec::new(1, "P").shapes(one).rel("rId1", &format!("{OD}image"), "../media/i.png");
        VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/i.png", "image/png", vec![7u8; 3_000_000])).build()
    };
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    // What the model is without the picture: the cap of the pictures leaves it out, and the pool has room for all that follows.
    let (_, without) = read_with(&bytes, &Limits { max_media_bytes: 1_000_000, ..Limits::default() });
    assert_eq!(without.warning_count("media_over_budget"), 1);
    // What the model is without the picture's bytes: the model, and the bookkeeping of the part the picture is held as (F1: its bytes are the package's, and
    // the model's figure gains them when they move into the document).
    let rest = without.stats.memory_bytes - arc_str(3_000_000);
    assert_eq!((without.held_parts, without.held_bytes), (1, 3_000_000));
    // Pools around the place where the bytes fit and the rest of the picture's price does not (from 4,554 bytes over what the model and the bytes need, where the page's tree
    // is held beside them): whenever the picture is left out, nothing of its price stays charged, whether the read goes on or the bookkeeping of its part does not fit.
    let around = room + rest + arc_str(3_000_000);
    let mut left_out = 0;
    for pool in (around - 8_000..around + 8_000).step_by(3) {
        let (result, report) = read_partial(&bytes, &Limits { max_memory: pool, ..Limits::default() });
        if report.warning_count("media_over_budget") == 1 {
            left_out += 1;
            let charged = report.stats.memory_bytes.saturating_sub(if result.is_ok() { arc_str(3_000_000) } else { 0 });
            assert!(charged <= rest + 2_000, "a pool of {pool}: the picture is left out and {charged} bytes are charged, the model being {rest}");
            match result {
                Ok(_) => assert_eq!((report.held_parts, report.held_bytes), (1, 3_000_000), "a pool of {pool}"),
                Err(e) => assert_eq!(e.class(), "too_much_memory", "a pool of {pool}: {e}"),
            }
        }
    }
    assert!(left_out > 20, "the pools reach the case: {left_out}");
}

#[test]
fn a_relationships_part_past_the_packages_own_cap_is_too_complex_and_one_the_pool_cannot_hold_is_memory_to_the_item() {
    // The root relationships part is the first thing the read parses, before anything is charged, so the pool it is bounded by is the budget less the file, the package and the
    // room a growing list needs, and the bound is that, less twice the part's text, over 368 an item. Forty rows are 162 items of a part that is a package's own limit of 100.
    let rows = (0..40).fold(Pkg::new(), |pkg, i| {
        pkg.rel("/", &format!("rId{}", 1_000 + i), "http://example.com/relationships/other", &format!("other/part{i}.xml"))
    });
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).with(rows).build();
    let stats = report_of(&bytes).stats;
    let room = stats.file_bytes + stats.uncompressed_bytes + cost::GROWTH_BYTES;
    let rels_len = {
        let package = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).expect("a package");
        package.part("/_rels/.rels").expect("the root relationships").len() as u64
    };
    let class = |max_memory: u64, nodes: usize| {
        let opc = diacraft_opc::Limits { max_package_xml_nodes: nodes, ..diacraft_opc::Limits::default() };
        let (result, _) = read_partial(&bytes, &Limits { max_memory, opc, ..Limits::default() });
        result.expect_err("the part is past a cap").class()
    };
    // The package's cap is 100 items and the pool is the budget: the part is too complex, whatever the pool says (a pool with room for a million items is no more than that).
    assert_eq!(class(2_600_000_000, 100), "xml_too_complex");
    assert_eq!(class(room + 2 * rels_len + 368 * 1_000_000, 100), "xml_too_complex");
    // A pool that holds exactly 100 items: the part is past the package's cap, and the pool does not hold less than the cap, so it is the cap that refuses it. One byte under
    // 100 items, and it is the pool; and 368 bytes over, and it is the cap again.
    let hundred = room + 2 * rels_len + 368 * 100;
    assert_eq!(class(hundred, 100), "xml_too_complex", "the pool holds 100 items and the cap is 100");
    assert_eq!(class(hundred - 1, 100), "too_much_memory", "the pool holds 99");
    assert_eq!(class(hundred + 367, 100), "xml_too_complex");
    assert_eq!(class(hundred + 368, 100), "xml_too_complex", "the pool holds 101");
}
