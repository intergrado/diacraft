//! Fuzzing the reader (AGENTS.md: importers are fuzzed): a tree of random elements, and the bytes of a sheet mutated, must read or be refused,
//! never panic, never hang, read the same twice, and spend no more than the budgets they were given.
//!
//! **A clean run of this finds nothing a crafted input does not** (25 million clean runs missed a 972 KB file that hung the container):
//! the cost of a sheet's size is in `hostile.rs`, built by hand, and the whole package is fuzzed in `diacraft-vsdx`. `cargo fuzz run
//! sheetxml_read` (`crates/sheetxml/fuzz`) runs the same reading on bytes, with coverage.

use diacraft_container::xml::{El, Node};
use diacraft_doc::{MAX_CELLS_PER_CONTAINER, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET};
use diacraft_sheetxml::build::sheet_only_el;
use diacraft_sheetxml::{Caps, Vocabulary};
use proptest::prelude::*;

use super::{BIG, EXT, Ext, MAIN, Main, read_el};

const NAMES: &[&str] = &["Cell", "Section", "Row", "Trigger", "Foo", "x:Cell", "x:Section", "q:Row", "Cell", "Cell", "Row", "Section"];
const ATTRS: &[&str] = &["N", "N", "V", "F", "U", "E", "IX", "T", "Del", "LocalName", "Odd", "xmlns", "xmlns:x", "LineStyle", "N"];
const VALUES: &[&str] = &[
    "",
    "0",
    "1",
    "007",
    "+5",
    "4294967296",
    "-1",
    "true",
    "Inh",
    "No Formula",
    "1+2",
    "Width*2",
    "((",
    "a b",
    "IN",
    "x",
    "User",
    "Geometry",
    "Connection",
    "A",
    "B",
    "Value",
    "urn:test:main",
    "urn:test:ext",
    "urn:other",
    "#REF!",
    "\u{e9}\u{4e2d}",
];

/// `plain`: no prefix on a name and no namespace declaration on an element. A tree written has the names of the model and not the prefixes it was
/// read under, and a declaration kept in `raw` is written where the element stands, so a tree that is read as one thing may be written as another
/// (a cell `x:Cell` that declares `xmlns="0"` is written as a `Cell` in the namespace `0`); the writer is tried on trees that have neither.
fn name(plain: bool) -> impl Strategy<Value = String> {
    prop::sample::select(NAMES).prop_filter("a prefix", move |n| !plain || !n.contains(':')).prop_map(str::to_owned)
}

fn attrs(plain: bool) -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec((prop::sample::select(ATTRS), prop::sample::select(VALUES)), 0..4).prop_map(move |pairs| {
        // A parsed tree never has an attribute twice.
        let mut seen = std::collections::HashSet::new();
        pairs.into_iter().filter(|(k, _)| !(plain && k.starts_with("xmlns")) && seen.insert(*k)).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
    })
}

fn leaf() -> impl Strategy<Value = Node> {
    prop_oneof![
        Just(Node::Text(" \n ".to_owned())),
        Just(Node::Text("tail".to_owned())),
        Just(Node::CData("cdata".to_owned())),
        Just(Node::Comment("c".to_owned())),
        Just(Node::Pi { target: "NewValue".to_owned(), data: "V".to_owned() }),
    ]
}

fn node(plain: bool) -> impl Strategy<Value = Node> {
    leaf().prop_recursive(5, 48, 5, move |inner| {
        (name(plain), attrs(plain), prop::collection::vec(inner, 0..5)).prop_map(|(name, attrs, children)| Node::El(El { name, attrs, children }))
    })
}

fn tree(plain: bool) -> impl Strategy<Value = El> {
    (attrs(plain), prop::collection::vec(node(plain), 0..8)).prop_map(|(attrs, children)| El { name: "PageSheet".to_owned(), attrs, children })
}

/// The one thing a read that was given budgets must keep: what it spent is what it was given less what is left.
fn spent_within(r: &super::Read, caps: Caps) {
    let stats = r.reader.stats();
    assert!(stats.items <= caps.items && stats.cells <= caps.cells && stats.formula_nodes <= caps.formula_nodes, "{stats:?}");
    assert_eq!(stats.items + r.reader.items_left(), caps.items);
    assert_eq!(stats.cells + r.reader.cells_left(), caps.cells);
    let sheet = &r.sheet;
    assert!(sheet.cells.len() <= MAX_CELLS_PER_CONTAINER && sheet.sections.len() <= MAX_SECTIONS_PER_SHEET);
    for section in &sheet.sections {
        assert!(section.cells.len() <= MAX_CELLS_PER_CONTAINER && section.rows.len() <= MAX_ROWS_PER_SECTION);
        assert!(section.rows.iter().all(|row| row.cells.len() <= MAX_CELLS_PER_CONTAINER));
    }
}

fn reads_alike<V: Vocabulary + Default>(tree: &El, caps: Caps) {
    let first = read_el::<V>(tree, caps);
    let second = read_el::<V>(tree, caps);
    match (first, second) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.sheet, b.sheet);
            assert_eq!(a.reader.stats(), b.reader.stats());
            assert_eq!(a.warnings, b.warnings);
            spent_within(&a, caps);
        }
        (Err(a), Err(b)) => assert_eq!(a, b),
        (a, b) => panic!("one read and then another gave {:?} and {:?}", a.is_ok(), b.is_ok()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, max_shrink_iters: 200, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn a_random_tree_reads_or_is_refused_and_reads_the_same_twice(tree in tree(false)) {
        reads_alike::<Main>(&tree, BIG);
        reads_alike::<Ext>(&tree, BIG);
    }

    #[test]
    fn a_random_tree_under_small_budgets_is_refused_by_the_budget_and_never_passes_it(
        tree in tree(false),
        items in 0usize..60,
        cells in 0usize..8,
        nodes in 0usize..12,
    ) {
        let caps = Caps { items, cells, formula_nodes: nodes };
        reads_alike::<Main>(&tree, caps);
    }

    #[test]
    fn a_sheet_that_was_read_is_written_and_read_again_as_the_same_sheet(tree in tree(true)) {
        let Ok(first) = read_el::<Main>(&tree, BIG) else { return Ok(()) };
        // Two cells of one name are kept (the first is the one found) and the writer puts the cells in the order of their names, so which of the two
        // is first can change in a tree that is written: that is the writer's rule for what it makes new, not what is tried here.
        prop_assume!(first.warnings.count("duplicate_cell") + first.warnings.count("duplicate_section") + first.warnings.count("duplicate_row_key") == 0);
        let written = sheet_only_el("PageSheet", &first.sheet);
        let again = read_el::<Main>(&written, BIG).unwrap();
        // The tree written is a tree the reader reads, and what it reads writes as the same tree. (The sheet read from it is the first one but for
        // the order of the attributes the model has no field for: they are written after the typed ones, as `build.rs` says, wherever they stood.)
        prop_assert_eq!(again.sheet.cells.len(), first.sheet.cells.len());
        prop_assert_eq!(again.sheet.sections.len(), first.sheet.sections.len());
        prop_assert_eq!(again.sheet.links, first.sheet.links);
        prop_assert_eq!(sheet_only_el("PageSheet", &again.sheet), written);
    }

    #[test]
    fn the_bytes_of_a_sheet_mutated_parse_or_are_refused_and_what_parses_reads_or_is_refused(
        at in 0usize..400,
        cut in 0usize..12,
        snippet in prop::sample::select(&["<", ">", "</", "/>", "&", "\"", "'", "<![CDATA[", "]]>", "<!--", "-->", "<?", "xmlns=\"urn:test:ext\"", "N=\"\"", "\u{0}"][..]),
    ) {
        let seed = format!(
            r#"<PageSheet xmlns="{MAIN}" LineStyle="3"><Cell N="PinX" V="1.50" U="IN" F="Width*0.5"/><Section N="Geometry" IX="0"><Row T="MoveTo"><Cell N="X" V="0"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="x"><RefBy ID="4"/></Cell></Row></Section><Trigger N="t"/></PageSheet>"#
        );
        let at = at.min(seed.len());
        let mut at = at;
        while !seed.is_char_boundary(at) {
            at -= 1;
        }
        let mut end = (at + cut).min(seed.len());
        while !seed.is_char_boundary(end) {
            end += 1;
        }
        let mutated = format!("{}{snippet}{}", &seed[..at], &seed[end..]);
        if let Ok(tree) = diacraft_container::xml::parse(mutated.as_bytes()) {
            reads_alike::<Main>(&tree, BIG);
            let _ = EXT;
        }
    }
}
