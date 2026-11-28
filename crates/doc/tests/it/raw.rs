//! What the model has no typed place for is kept as the file had it, in order, and the small data
//! types around it hold what they were given.

use std::sync::Arc;

use diacraft_doc::{Cell, CellExtra, ForeignData, FormulaSlot, Passthrough, Placed, RawAttrs, RawElement, RawNode, Shape, ShapeType, Text, TextPart};

#[test]
fn attributes_keep_their_order_their_duplicates_and_their_case() {
    let attrs = RawAttrs::from_pairs([("z", "1"), ("a", "2"), ("x:mine", "3"), ("a", "4"), ("A", "5")]);
    assert_eq!(attrs.iter().collect::<Vec<_>>(), [("z", "1"), ("a", "2"), ("x:mine", "3"), ("a", "4"), ("A", "5")]);
    assert_eq!(attrs.len(), 5);
    // Names are matched exactly; the first of two with one name is the one found.
    assert_eq!(attrs.get("a"), Some("2"));
    assert_eq!(attrs.get("A"), Some("5"));
    assert_eq!(attrs.get("x:mine"), Some("3"));
    assert_eq!(attrs.get("mine"), None);
    assert!(RawAttrs::new().is_empty() && RawAttrs::from_pairs(Vec::<(&str, &str)>::new()).is_empty());
    assert_eq!(RawAttrs::new().get("a"), None);
}

#[test]
fn values_come_back_exactly_even_when_they_are_odd() {
    // Line breaks, blanks, entity-looking text and a prefix that only a later attribute uses are values like any other.
    let odd = "  two\nlines &amp; \u{e9} \t ";
    let attrs = RawAttrs::from_pairs([("mc:Ignorable", "x"), ("v", odd)]);
    assert_eq!(attrs.get("v"), Some(odd));
}

#[test]
fn a_copy_that_is_added_to_leaves_the_other_alone() {
    let mut a = RawAttrs::from_pairs([("one", "1")]);
    let b = a.clone();
    a.push("two", "2");
    assert_eq!(a.iter().collect::<Vec<_>>(), [("one", "1"), ("two", "2")]);
    assert_eq!(b.iter().collect::<Vec<_>>(), [("one", "1")]);
    let mut from_nothing = RawAttrs::new();
    from_nothing.push("only", "1");
    assert_eq!(from_nothing.len(), 1);
}

#[test]
fn unknown_nodes_keep_where_they_sat_and_their_order() {
    // A foreign element between the typed children stays between them.
    let note = RawNode::Element(RawElement {
        name: Arc::from("x:note"),
        attrs: RawAttrs::from_pairs([("xmlns:x", "urn:example")]),
        children: vec![
            RawNode::Text(Arc::from("hi")),
            RawNode::Comment(Arc::from(" c ")),
            RawNode::Pi { target: Arc::from("NewValue"), data: Arc::from("V") },
        ],
    });
    let mut p = Passthrough::new();
    assert!(p.is_empty());
    p.push(1, note.clone());
    p.push(3, RawNode::Text(Arc::from("  ")));
    p.push(3, RawNode::Pi { target: Arc::from("NewValue"), data: Arc::from("V") });
    assert_eq!(p.len(), 3);
    let placed: Vec<(u32, &RawNode)> = p.iter().map(|n| (n.after, &n.node)).collect();
    assert_eq!(placed[0], (1, &note));
    assert_eq!(placed[1].0, 3);
    assert_eq!(placed[2], (3, &RawNode::Pi { target: Arc::from("NewValue"), data: Arc::from("V") }));
    // A copy that is added to leaves the other alone.
    let before = p.clone();
    p.push(9, RawNode::Comment(Arc::from("later")));
    assert_eq!((before.len(), p.len()), (3, 4));
}

#[test]
fn a_cell_keeps_its_text_unknown_attributes_and_child_nodes() {
    // text kept, written back as element text, E9 (`<?NewValue V?>` kept).
    let mut cell = Cell::read(None, None, None, None, &mut 0);
    cell.raw.push("Extra", "kept");
    let mut passthrough = Passthrough::new();
    passthrough.push(0, RawNode::Pi { target: Arc::from("NewValue"), data: Arc::from("V") });
    cell.extra = Some(Arc::new(CellExtra { text: Some(Arc::from("Some text")), passthrough, ..CellExtra::default() }));
    let copy = cell.clone();
    assert_eq!(copy, cell);
    assert_eq!(copy.text(), Some("Some text"));
    assert_eq!(copy.raw.get("Extra"), Some("kept"));
    assert_eq!(copy.extra.as_ref().map(|e| e.passthrough.len()), Some(1));
    assert_eq!(Cell::new().text(), None);
}

#[test]
fn the_formula_slot_gives_back_the_text_of_the_attribute() {
    assert_eq!(FormulaSlot::Absent.text(), None);
    assert_eq!(FormulaSlot::Inherit.text(), Some("Inh"));
    assert_eq!(FormulaSlot::NoFormula.text(), Some("No Formula"));
    let verbatim = "  Width * 0.5  ";
    assert_eq!(Cell::read(Some(verbatim), None, None, None, &mut 100).formula.text(), Some(verbatim));
}

#[test]
fn a_shape_type_no_spec_lists_comes_back_as_the_text_it_was() {
    // Four values, and absent; anything else is kept.
    for (text, expected) in [("Shape", ShapeType::Shape), ("Group", ShapeType::Group), ("Foreign", ShapeType::Foreign), ("Guide", ShapeType::Guide)] {
        assert_eq!(ShapeType::read(Some(text)), expected);
        assert_eq!(expected.text(), Some(text));
    }
    assert_eq!(ShapeType::read(None), ShapeType::Unspecified);
    assert_eq!(ShapeType::Unspecified.text(), None);
    for odd in ["Ink", "shape", "GROUP", ""] {
        let kind = ShapeType::read(Some(odd));
        assert_eq!(kind, ShapeType::Other(Arc::from(odd)));
        assert_eq!(kind.text(), Some(odd));
    }
}

#[test]
fn a_shape_holds_what_the_file_gave_it() {
    let mut shape = Shape::new(4_294_967_295);
    assert_eq!(shape.id, u32::MAX);
    assert!(!shape.is_instance());
    shape.master = Some(0);
    assert!(shape.is_instance(), "master 0 is a master");
    // An `ID` the type could not hold is kept as text in `raw`, and the shape reads without error.
    shape.raw.push("ID", "4294967296");
    assert_eq!(shape.raw.get("ID"), Some("4294967296"));
}

#[test]
fn text_is_kept_in_pieces_with_its_blanks() {
    // `ab`, a `cp`, `cd`, a field; and R4 of the document spec: blanks are significant.
    let text = Text {
        parts: vec![
            TextPart::Cp(0),
            TextPart::Chars(Arc::from("  two  spaces ")),
            TextPart::Cp(1),
            TextPart::Chars(Arc::from("cd")),
            TextPart::Fld { ix: 0, text: Arc::from("3") },
            TextPart::Raw(RawNode::Comment(Arc::from("kept"))),
        ],
        ..Default::default()
    };
    assert_eq!(text.clone(), text);
    assert!(matches!(&text.parts[1], TextPart::Chars(c) if &**c == "  two  spaces "));
}

#[test]
fn foreign_data_keeps_every_attribute_in_order() {
    let data = ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("ObjectWidth", "1.5"), ("CompressionType", "PNG")]),
        rel_id: Some(Arc::from("rId2")),
        ..ForeignData::default()
    };
    assert_eq!(data.attr("ForeignType"), Some("Bitmap"));
    assert_eq!(data.attr("compressiontype"), None);
    assert_eq!(data.attrs.iter().map(|(n, _)| n).collect::<Vec<_>>(), ["ForeignType", "ObjectWidth", "CompressionType"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------- places

/// Nodes that stood after these counts of typed children, each a comment named by its count and its place in the list.
fn nodes_after(counts: &[u32]) -> Passthrough {
    Passthrough::from_nodes(
        counts.iter().enumerate().map(|(k, after)| Placed { after: *after, node: RawNode::Comment(Arc::from(format!("n{k}"))) }).collect(),
    )
}

fn places(nodes: &Passthrough) -> Vec<u32> {
    nodes.iter().map(|placed| placed.after).collect()
}

/// A section of two cells and then three rows: a node among the cells (1), one after them (2), and one after each row (3, 4, 5).
const SECTION: [u32; 5] = [1, 2, 3, 4, 5];

#[test]
fn a_row_put_in_goes_ahead_of_the_node_that_followed_the_row_before_it_and_the_nodes_before_it_stay() {
    // (the place of the new row among the rows, whether the nodes after the cells stay ahead of the first row) -> the places now.
    let cases: [(usize, bool, [u32; 5]); 5] =
        [(0, true, [1, 2, 4, 5, 6]), (0, false, [1, 3, 4, 5, 6]), (1, true, [1, 2, 4, 5, 6]), (2, true, [1, 2, 3, 5, 6]), (3, true, [1, 2, 3, 4, 6])];
    for (at, lead_stays, want) in cases {
        let mut nodes = nodes_after(&SECTION);
        nodes.children_inserted(2, 3, &[at], lead_stays);
        assert_eq!(places(&nodes), want, "a row put in at {at}, lead stays {lead_stays}");
    }
}

#[test]
fn rows_taken_out_leave_their_nodes_with_the_row_before() {
    let cases: [(&[usize], [u32; 5]); 5] =
        [(&[1], [1, 2, 3, 3, 4]), (&[0], [1, 2, 2, 3, 4]), (&[2], [1, 2, 3, 4, 4]), (&[0, 2], [1, 2, 2, 3, 3]), (&[0, 1, 2], [1, 2, 2, 2, 2])];
    for (gone, want) in cases {
        let mut nodes = nodes_after(&SECTION);
        nodes.children_removed(2, 3, gone);
        assert_eq!(places(&nodes), want, "rows {gone:?} taken out");
    }
}

#[test]
fn several_rows_put_in_at_once_are_the_same_as_one_after_the_other() {
    // The places are those the rows have once all are in; the rows are put in one at a time, in rising order, to the same end.
    let batches: [&[usize]; 4] = [&[0, 2], &[1, 2], &[0, 1, 2, 3], &[3, 4, 5]];
    for at in batches {
        for lead_stays in [true, false] {
            let mut together = nodes_after(&SECTION);
            together.children_inserted(2, 3, at, lead_stays);
            let mut one_by_one = nodes_after(&SECTION);
            for (k, place) in at.iter().enumerate() {
                one_by_one.children_inserted(2, 3 + k, &[*place], lead_stays);
            }
            assert_eq!(places(&together), places(&one_by_one), "rows put in at {at:?}, lead stays {lead_stays}");
        }
    }
}

#[test]
fn a_row_put_in_and_taken_out_again_leaves_every_node_where_it_was() {
    for at in 0..=3usize {
        let mut nodes = nodes_after(&SECTION);
        nodes.children_inserted(2, 3, &[at], true);
        nodes.children_removed(2, 4, &[at]);
        assert_eq!(places(&nodes), SECTION, "a row put in at {at} and out again");
    }
}

#[test]
fn nodes_after_the_run_move_by_what_the_run_gained_or_lost_and_nodes_before_it_do_not() {
    // The run is the sheet's three sections, after four cells; the nodes after it are the shape's `Text` and members.
    let mut nodes = nodes_after(&[0, 3, 4, 8, 9]);
    nodes.children_inserted(4, 3, &[1], false);
    assert_eq!(places(&nodes), [0, 3, 4, 9, 10], "a section put in before the second");
    nodes.children_removed(4, 4, &[0, 3]);
    assert_eq!(places(&nodes), [0, 3, 4, 7, 8], "two sections taken out");
}

#[test]
fn a_list_no_edit_moves_is_not_copied_and_one_that_moves_leaves_its_copy_alone() {
    let unmoved = nodes_after(&[0, 1]);
    let mut shared = unmoved.clone();
    shared.children_inserted(2, 0, &[0], true);
    assert!(Arc::ptr_eq(unmoved.shared_list().unwrap(), shared.shared_list().unwrap()), "nothing moved, so the list is still the one it shares");
    let moved = nodes_after(&[1, 2]);
    let mut copy = moved.clone();
    copy.children_inserted(0, 2, &[0], true);
    assert_eq!(places(&moved), [1, 2]);
    assert_eq!(places(&copy), [2, 3]);
    // No node at all is no work and no list.
    let mut none = Passthrough::new();
    none.children_inserted(0, 5, &[1, 2], true);
    none.children_removed(0, 5, &[1, 2]);
    assert!(none.is_empty() && none.shared_list().is_none());
}

#[test]
fn places_at_the_edge_of_what_a_count_holds_move_without_a_panic() {
    let mut nodes = nodes_after(&[u32::MAX, u32::MAX - 1, 0]);
    nodes.children_inserted(0, usize::MAX - 1, &[0, usize::MAX], false);
    nodes.children_removed(usize::MAX, usize::MAX, &[0]);
    nodes.children_removed(1, 0, &[usize::MAX, 0, 7]);
    nodes.children_inserted(usize::MAX, 0, &[usize::MAX, 3], true);
    nodes.reposition(usize::MAX, usize::MAX, 0, |b| b);
    nodes.reposition(0, 0, usize::MAX, |_| usize::MAX);
    assert_eq!(nodes.len(), 3, "no node is lost");
}

// -------------------------------------------------------------------------------------------------------------------------- a run rebuilt

/// The names of the nodes in their order.
fn named(nodes: &Passthrough) -> Vec<String> {
    nodes
        .iter()
        .map(|placed| match &placed.node {
            RawNode::Comment(text) => text.to_string(),
            other => panic!("{other:?}"),
        })
        .collect()
}

/// Children `a b c d`, with a node before the first (n0) and one after each (n1 to n4).
fn abcd() -> Passthrough {
    nodes_after(&[0, 1, 2, 3, 4])
}

#[test]
fn a_run_rebuilt_moves_each_node_with_the_child_it_followed_and_takes_out_and_puts_in_as_the_other_writers_do() {
    type Rebuilt = (&'static str, &'static [char], &'static [char], &'static [u32]);
    let cases: [Rebuilt; 9] = [
        ("b taken out: the node that followed it follows a", &['a', 'b', 'c', 'd'], &['a', 'c', 'd'], &[0, 1, 1, 2, 3]),
        ("a taken out: its node leads", &['a', 'b', 'c', 'd'], &['b', 'c', 'd'], &[0, 0, 1, 2, 3]),
        ("d taken out: its node follows c", &['a', 'b', 'c', 'd'], &['a', 'b', 'c'], &[0, 1, 2, 3, 3]),
        ("e put in at the end: ahead of the node that trailed d", &['a', 'b', 'c', 'd'], &['a', 'b', 'c', 'd', 'e'], &[0, 1, 2, 3, 5]),
        ("e put in in the middle", &['a', 'b', 'c', 'd'], &['a', 'b', 'e', 'c', 'd'], &[0, 1, 3, 4, 5]),
        ("e put in first: the node before the first stays before it", &['a', 'b', 'c', 'd'], &['e', 'a', 'b', 'c', 'd'], &[0, 2, 3, 4, 5]),
        ("b and c taken out, g put in where they were", &['a', 'b', 'c', 'd'], &['a', 'g', 'd'], &[0, 2, 2, 2, 3]),
        ("d to the front", &['a', 'b', 'c', 'd'], &['d', 'a', 'b', 'c'], &[0, 2, 3, 4, 1]),
        ("reversed", &['a', 'b', 'c', 'd'], &['d', 'c', 'b', 'a'], &[0, 4, 3, 2, 1]),
    ];
    for (what, old, new, want) in cases {
        let mut nodes = abcd();
        nodes.children_followed(0, old, new);
        let mut got = places(&nodes);
        got.sort_unstable();
        let mut want_sorted = want.to_vec();
        want_sorted.sort_unstable();
        assert_eq!(got, want_sorted, "{what}: the places of the nodes, in order");
        // The nodes are in order of their places, and each is where its child went.
        assert!(places(&nodes).windows(2).all(|w| w[0] <= w[1]), "{what}: {:?}", places(&nodes));
        let by_name: std::collections::HashMap<String, u32> = named(&nodes).into_iter().zip(places(&nodes)).collect();
        let in_old_order: Vec<u32> = (0..5).map(|k| by_name[&format!("n{k}")]).collect();
        assert_eq!(in_old_order, want, "{what}: the place of n0 to n4");
    }
}

#[test]
fn a_run_rebuilt_leaves_the_nodes_before_it_and_after_it_alone_but_for_what_it_gained_or_lost() {
    // Two cells, then the run `a b c`, then the shape's text: nodes before the cells (0), among them (1), after them (2), after each child (3, 4, 5), after the text (6).
    let mut nodes = nodes_after(&[0, 1, 2, 3, 4, 5, 6]);
    nodes.children_followed(2, &['a', 'b', 'c'], &['a', 'x', 'c']);
    assert_eq!(
        places(&nodes),
        [0, 1, 2, 4, 4, 5, 6],
        "b is gone and x is new: the nodes that followed a and b stand after a and x, which went in ahead of them"
    );
    let mut nodes = nodes_after(&[0, 1, 2, 3, 4, 5, 6]);
    nodes.children_followed(2, &['a', 'b', 'c'], &['a', 'b', 'c', 'd', 'e']);
    assert_eq!(
        places(&nodes),
        [0, 1, 2, 3, 4, 7, 8],
        "two put in at the end: the node that followed c is after both, the one after the text moves by two"
    );
}

#[test]
fn a_run_that_is_the_same_is_not_read_or_copied_and_a_list_with_no_node_is_not_read_at_all() {
    diacraft_doc::work::reset();
    let held = abcd();
    let mut same = held.clone();
    same.children_followed(0, &['a', 'b', 'c', 'd'], &['a', 'b', 'c', 'd']);
    assert!(Arc::ptr_eq(held.shared_list().unwrap(), same.shared_list().unwrap()));
    assert_eq!(diacraft_doc::work::follow_keys(), 0);
    let mut none = Passthrough::new();
    none.children_followed(0, &[1u32, 2, 3], &[3u32, 2, 1]);
    assert!(none.is_empty());
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (0, 0));
    // A run that changes reads the keys once each and the nodes once.
    let mut moved = abcd();
    moved.children_followed(0, &['a', 'b', 'c', 'd'], &['b', 'a', 'c', 'd']);
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (8, 5));
}

#[test]
fn a_run_rebuilt_with_a_key_that_comes_twice_or_a_run_that_is_empty_moves_without_a_panic() {
    let mut nodes = nodes_after(&[0, 1, 2, 3, 4]);
    nodes.children_followed(0, &[1u32, 1, 1, 1], &[1u32, 1]);
    assert_eq!(nodes.len(), 5);
    assert!(places(&nodes).iter().all(|place| *place <= 2), "{:?}", places(&nodes));
    let mut nodes = nodes_after(&[0, 1, 2, 3, 4]);
    nodes.children_followed(0, &[1u32, 2, 3, 4], &[]);
    assert_eq!(places(&nodes), [0, 0, 0, 0, 0], "no child left: every node leads");
    let mut nodes = nodes_after(&[0]);
    nodes.children_followed(0, &[], &[7u32, 8]);
    assert_eq!(places(&nodes), [0], "a node at the start of a run that was empty stays before its first child");
    let mut nodes = nodes_after(&[u32::MAX, 3]);
    nodes.children_followed(usize::MAX, &[1u32], &[2u32]);
    nodes.children_followed(0, &[1u32, 2], &[2u32, 1]);
    assert_eq!(nodes.len(), 2);
}
