//! What the model has no typed place for is kept as the file had it, in order, and the small data
//! types around it hold what they were given.

use std::sync::Arc;

use diacraft_doc::{Cell, CellExtra, ForeignData, FormulaSlot, Passthrough, RawAttrs, RawElement, RawNode, Shape, ShapeType, Text, TextPart};

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
