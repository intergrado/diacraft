//! What a place that holds one cell, or one child, costs. A map of cells allocates a leaf of eleven slots (1,160 bytes) at its first
//! cell, and a vector room for four at its first push, so a model of places that hold one thing each is built from several times what a per-element figure
//! measured on places of thousands says. The decoder charges the nodes of each map of cells ([`cost::cells_bytes`]) and the room of each list of kept nodes and
//! attributes, as it builds them; these tests hold the counts, and measure what a map of each size costs against what it is charged.
#[cfg(target_os = "linux")]
use std::process::Command;

use diacraft_doc::{Cell, CellName, Cells};
use diacraft_sheetxml::model::cost::{ATTR_BYTES, MODEL_CELLS_INNER_BYTES, MODEL_CELLS_LEAF_BYTES, cells_bytes, element_bytes};
use diacraft_sheetxml::model::{Budget, MemBlobs, URN, decode_page};

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

#[test]
fn the_nodes_of_a_map_of_cells_built_from_sorted_pairs_are_counted_by_leaves_and_inner_nodes() {
    let (leaf, inner) = (MODEL_CELLS_LEAF_BYTES, MODEL_CELLS_INNER_BYTES);
    // 11 slots of a key and a value, a parent pointer and two counts, at the law of the memory tests: the bytes asked for, an eighth of them and 24,
    // rounded up to 16.
    let slots = 11 * (std::mem::size_of::<CellName>() + std::mem::size_of::<Cell>());
    let request = (slots + 12).div_ceil(8) * 8;
    assert!(leaf as usize >= request + request / 8 + 24, "{leaf} bytes for a leaf of {slots} bytes of slots");
    assert!((leaf as usize) < request + request / 8 + 40 && leaf % 16 == 0, "{leaf}");
    // An inner node has twelve more edges, of eight bytes.
    let edged = request + 12 * 8;
    assert!(inner as usize >= edged + edged / 8 + 24 && (inner as usize) < edged + edged / 8 + 40, "{inner}");
    assert_eq!(cells_bytes(0), 0, "no cell, no node");
    for n in 1..=11 {
        assert_eq!(cells_bytes(n), leaf, "{n} cells are one leaf");
    }
    // The twelfth makes a second leaf and the root over them; every twelfth after it a leaf.
    assert_eq!(cells_bytes(12), 2 * leaf + inner);
    assert_eq!(cells_bytes(23), 2 * leaf + inner);
    assert_eq!(cells_bytes(24), 3 * leaf + inner);
    assert_eq!(cells_bytes(143), 12 * leaf + inner, "twelve leaves under one inner node");
    assert_eq!(cells_bytes(144), 13 * leaf + 3 * inner, "a thirteenth leaf: two inner nodes and a root over them");
    assert_eq!(cells_bytes(4096), 342 * leaf + (29 + 3 + 1) * inner);
    // Never less for more, and a size that cannot be built does not wrap.
    let mut last = 0;
    for n in 0..5_000 {
        let now = cells_bytes(n);
        assert!(now >= last, "{n}");
        last = now;
    }
    assert!(cells_bytes(usize::MAX) > 0);
}

/// A part with one row of `k` cells in a section of a sheet of a shape, and what the decode of it is charged.
fn charged(inner: &str) -> u64 {
    let xml = format!(
        "<diacraft:page xmlns:diacraft=\"{URN}\"><diacraft:shape id=\"1\"><diacraft:sheet>{inner}</diacraft:sheet></diacraft:shape></diacraft:page>"
    );
    let el = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
    // Each unit by its own price: what an entry in the table of shared units costs is its own test's.
    let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
    decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
    HUGE - budget.bytes
}

fn cells(k: usize) -> String {
    (0..k).map(|c| format!("<diacraft:Cell N=\"c{c:02}\"/>")).collect()
}

#[test]
fn each_cell_of_a_place_is_charged_its_own_price_and_the_nodes_it_adds_to_the_map_in_the_sheet_a_section_and_a_row() {
    type Place = fn(&str) -> String;
    let places: [(&str, Place); 3] = [
        ("a sheet", |c| c.to_owned()),
        ("a section", |c| format!("<diacraft:Section N=\"User\">{c}</diacraft:Section>")),
        ("a row", |c| format!("<diacraft:Section N=\"User\"><diacraft:Row N=\"r\">{c}</diacraft:Row></diacraft:Section>")),
    ];
    for (name, place) in places {
        for k in 0..=30 {
            let (before, after) = (charged(&place(&cells(k))), charged(&place(&cells(k + 1))));
            // The cell's own price, its attribute, the three bytes of its name (text, at nine eighths: text is charged at nine eighths of its length, rounded up), and the nodes of the map.
            let expected = element_bytes("Cell") + ATTR_BYTES + diacraft_sheetxml::model::cost::text_bytes(3) + cells_bytes(k + 1) - cells_bytes(k);
            assert_eq!(after - before, expected, "the {}th cell of {name}", k + 1);
        }
    }
}

const CHILD: &str = "SHEETXML_CELLS_CHILD";

#[test]
#[ignore = "run by the test below in a process of its own"]
fn cells_child() {
    let n: usize = std::env::var(CHILD).unwrap_or_default().parse().expect("a number of cells");
    let status = |key: &str| -> u64 {
        let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
            .unwrap_or(0)
    };
    // As many maps as make some 40 MB, built the way the decoder builds each: the cells in the order a file has them, collected.
    let maps = (40_000_000 / cells_bytes(n).max(1) as usize).clamp(20, 100_000);
    let build = |m: usize| -> Vec<Cells> {
        (0..m).map(|_| (0..n).map(|c| (CellName::new(&format!("c{c:03}")), Cell::default())).collect::<Cells>()).collect()
    };
    drop(build(maps.min(50)));
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak");
    let before = status("VmRSS");
    let held = build(maps);
    let grew = status("VmRSS").saturating_sub(before) * 1024;
    assert_eq!(held.len(), maps);
    println!("CELLS n={n} maps={maps} grew_each={} charged_each={}", grew / maps as u64, cells_bytes(n) + n as u64 * 32);
}

#[test]
#[cfg(target_os = "linux")]
fn a_map_of_cells_built_the_way_the_decoder_builds_it_costs_no_more_than_it_is_charged() {
    let exe = std::env::current_exe().unwrap();
    for n in [1usize, 2, 5, 11, 12, 23, 24, 100, 144, 1000, 4096] {
        let out = Command::new(&exe)
            .args(["--exact", "model_sparse::cells_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD, n.to_string())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let line = text.lines().find(|l| l.contains("CELLS n=")).unwrap_or_else(|| panic!("no measure for {n}: {text}"));
        let field = |key: &str| -> u64 { line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number") };
        let (grew, charged) = (field("grew_each="), field("charged_each="));
        eprintln!("{n:>5} cells: a map grew by {grew} bytes, charged {charged}");
        // The 24 bytes of the map itself are in the owner, a row or a section or a sheet, which prices them; here they are in a vector that doubles (up to 48).
        // The nodes are counted exactly: what a map grew by is its charge to the byte but for the page the measure rounds to and the allocator's own tables, a
        // hundredth at the most.
        assert!(grew <= charged + charged / 50 + 48, "a map of {n} cells grew by {grew} bytes and is charged {charged}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- lists

/// What a page of one shape with `inner` in it is charged.
fn charged_shape(inner: &str) -> u64 {
    let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\"><diacraft:shape id=\"1\">{inner}</diacraft:shape></diacraft:page>");
    let el = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
    decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
    HUGE - budget.bytes
}

/// The charge of the `k+1`th of a list less the charge of the `k`th, by `unit(k)`, must be what one unit costs and what the push adds to the list.
fn each_push_is_charged_the_growth_of_its_list(unit: impl Fn(usize) -> String, growth: impl Fn(usize) -> u64, what: &str) {
    let list = |k: usize| (0..k).map(&unit).collect::<String>();
    // What one unit costs by itself is the first's charge less what the first push adds (the room for four and the header).
    let alone = charged_shape(&list(1)) - charged_shape("") - growth(0);
    for k in 0..40 {
        let step = charged_shape(&list(k + 1)) - charged_shape(&list(k));
        assert_eq!(step, alone + growth(k), "the {}th {what}: a unit's own cost {alone} and the growth of the list", k + 1);
    }
}

#[test]
fn each_kept_node_of_an_element_is_charged_the_room_it_makes_in_the_list_of_kept_nodes() {
    use diacraft_sheetxml::model::cost::PLACED_BYTES;
    use diacraft_sheetxml::model::cost::push_bytes;
    each_push_is_charged_the_growth_of_its_list(
        |i| format!("<diacraft:kept><diacraft:txt>{i:02}</diacraft:txt></diacraft:kept>"),
        |k| push_bytes(k, PLACED_BYTES),
        "kept node",
    );
}

#[test]
fn each_kept_attribute_of_an_element_is_charged_the_room_it_makes_in_the_list_of_attributes() {
    use diacraft_sheetxml::model::cost::ATTR_PAIR_BYTES;
    use diacraft_sheetxml::model::cost::push_bytes;
    each_push_is_charged_the_growth_of_its_list(
        |i| format!("<diacraft:attr name=\"a{i:02}\" value=\"v\"/>"),
        |k| push_bytes(k, ATTR_PAIR_BYTES),
        "kept attribute",
    );
}

#[test]
fn each_unknown_attribute_of_an_element_is_charged_the_room_it_makes_in_the_list_of_attributes() {
    use diacraft_sheetxml::model::cost::ATTR_PAIR_BYTES;
    use diacraft_sheetxml::model::cost::push_bytes;
    let shape = |k: usize| {
        let attrs: String = (0..k).map(|i| format!(" fa{i:02}=\"v\"")).collect();
        let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\"><diacraft:shape id=\"1\"{attrs}/></diacraft:page>");
        let el = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    let alone = shape(1) - shape(0) - push_bytes(0, ATTR_PAIR_BYTES);
    for k in 0..40 {
        assert_eq!(shape(k + 1) - shape(k), alone + push_bytes(k, ATTR_PAIR_BYTES), "the {}th unknown attribute", k + 1);
    }
}

#[test]
fn a_kept_element_is_charged_the_room_for_its_children_by_the_children_it_has() {
    use diacraft_sheetxml::model::cost::RAW_NODE_BYTES;
    use diacraft_sheetxml::model::cost::vec_bytes;
    let tree = |k: usize| {
        let children: String = (0..k).map(|i| format!("<diacraft:el name=\"c{i:02}\"/>")).collect();
        charged_shape(&format!("<diacraft:kept><diacraft:el name=\"p\">{children}</diacraft:el></diacraft:kept>"))
    };
    let alone = tree(1) - tree(0) - vec_bytes(1, RAW_NODE_BYTES);
    for k in 0..40 {
        assert_eq!(tree(k + 1) - tree(k), alone + vec_bytes(k + 1, RAW_NODE_BYTES) - vec_bytes(k, RAW_NODE_BYTES), "the {}th child", k + 1);
    }
}

#[test]
fn an_unknown_element_is_charged_the_room_for_its_children_by_the_children_it_has() {
    use diacraft_sheetxml::model::cost::RAW_NODE_BYTES;
    use diacraft_sheetxml::model::cost::vec_bytes;
    let tree = |k: usize| {
        let children: String = (0..k).map(|i| format!("<c{i:02}/>")).collect();
        charged_shape(&format!("<zz>{children}</zz>"))
    };
    let alone = tree(1) - tree(0) - vec_bytes(1, RAW_NODE_BYTES);
    for k in 0..40 {
        assert_eq!(tree(k + 1) - tree(k), alone + vec_bytes(k + 1, RAW_NODE_BYTES) - vec_bytes(k, RAW_NODE_BYTES), "the {}th child", k + 1);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- a charge that fails is a refusal

/// The decode of `inner` in a shape, with `bytes` of model to spend: whether it was refused for them.
fn refused_with(inner: &str, attrs: &str, bytes: u64) -> bool {
    let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\"><diacraft:shape id=\"1\"{attrs}>{inner}</diacraft:shape></diacraft:page>");
    let el = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let out = decode_page(&el, &MemBlobs::new(), &mut Budget::new(HUGE, bytes, HUGE), &mut Vec::new());
    match out {
        Ok(_) => false,
        Err(diacraft_sheetxml::model::CodecError::Limit { what: "bytes of the model in memory", .. }) => true,
        Err(other) => panic!("a refusal that is not the model's bytes: {other}"),
    }
}

/// What a charge costs that is not paid is a refusal and nothing less: with the bytes the decode needs it is read, and with any fewer, down to `window` fewer, it
/// is refused. A charge whose failure is not passed on (a result that is dropped) is a decode that reads with fewer than it costs.
fn is_refused_with_any_fewer_bytes_than_it_costs(inner: &str, attrs: &str, window: u64) {
    let needed = {
        let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\"><diacraft:shape id=\"1\"{attrs}>{inner}</diacraft:shape></diacraft:page>");
        let el = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap();
        HUGE - budget.bytes
    };
    assert!(!refused_with(inner, attrs, needed), "the bytes it costs are enough: {needed}");
    for short in 1..=window.min(needed) {
        assert!(refused_with(inner, attrs, needed - short), "{short} bytes short of {needed} is refused: {inner} {attrs}");
    }
}

#[test]
fn a_kept_node_an_attribute_and_a_tree_that_the_budget_cannot_pay_for_are_refused_to_the_byte() {
    for (inner, attrs) in [
        ("<diacraft:kept><diacraft:txt>x</diacraft:txt></diacraft:kept>", ""),
        ("<diacraft:attr name=\"a\" value=\"v\"/>", ""),
        ("<diacraft:kept><diacraft:el name=\"p\"><diacraft:attr name=\"a\" value=\"v\"/><diacraft:el name=\"c\"/></diacraft:el></diacraft:kept>", ""),
        ("", " fa=\"v\""),
        ("<zz a=\"1\"><y/>text</zz>", ""),
        ("<diacraft:sheet><diacraft:Cell N=\"a\"/><diacraft:Cell N=\"b\"/></diacraft:sheet>", ""),
        ("<diacraft:ext><diacraft:odf><diacraft:held group=\"fill\"/></diacraft:odf></diacraft:ext>", ""),
        (
            "<diacraft:text><diacraft:ext><diacraft:odf zz=\"1\"><next k=\"2\"/><diacraft:paragraph/></diacraft:odf></diacraft:ext></diacraft:text>",
            "",
        ),
        (
            "<diacraft:sheet><diacraft:Section N=\"User\"><diacraft:Row N=\"r\"><diacraft:ext><diacraft:odf><diacraft:held group=\"fill\"/><diacraft:held group=\"line\"/></diacraft:odf></diacraft:ext></diacraft:Row></diacraft:Section></diacraft:sheet>",
            "",
        ),
        ("<q:x xmlns:q=\"urn:q\" q:a=\"1\"/>", " xmlns:p=\"urn:p\""),
    ] {
        is_refused_with_any_fewer_bytes_than_it_costs(inner, attrs, 3_000);
    }
}

#[test]
fn a_kept_element_inside_a_kept_element_is_charged_the_room_for_its_own_children_too() {
    use diacraft_sheetxml::model::cost::RAW_NODE_BYTES;
    use diacraft_sheetxml::model::cost::vec_bytes;
    // The middle element is opened by the loop and not by the call: its children are what this counts.
    let tree = |k: usize| {
        let children: String = (0..k).map(|i| format!("<diacraft:el name=\"g{i:02}\"/>")).collect();
        charged_shape(&format!(
            "<diacraft:kept><diacraft:el name=\"p\"><diacraft:el name=\"c\">{children}</diacraft:el></diacraft:el></diacraft:kept>"
        ))
    };
    let alone = tree(1) - tree(0) - vec_bytes(1, RAW_NODE_BYTES);
    for k in 0..40 {
        assert_eq!(tree(k + 1) - tree(k), alone + vec_bytes(k + 1, RAW_NODE_BYTES) - vec_bytes(k, RAW_NODE_BYTES), "the {}th grandchild", k + 1);
    }
}
