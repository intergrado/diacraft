//! What a copy does with text the model keeps as the file had it, made to cost (S7, S8) and the fingerprint of a sheet.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{
    Cell as DocCell, CellExtra, CellName, Connect, Connects, Document, ForeignData, Kept, Links, Passthrough, RawAttrs, RawElement, RawNode, Row,
    Section, Shape, ShapeList, Sheet, Style, Text, TextPart,
};

use super::super::{Refusal, STEPS_PUSHED, bring_master, bring_with};
use super::{A, B, BOTH, SHEET_COMPARES, alike_styles, cell, compares, master};

fn element(name: &str, attrs: Vec<(String, String)>, children: Vec<RawNode>) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().map(|(a, b)| (a.as_str(), b.as_str()))), children })
}

/// A master whose one shape keeps `node` after its typed children.
fn master_keeping(node: RawNode) -> Document {
    let mut from = Document::new_blank();
    let mut m = master(1, "Keeps", Some(A), Some(B));
    let mut shape = Shape::new(5);
    shape.passthrough.push(0, node);
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    from
}

fn pushed() -> usize {
    crate::stencil::refs::BINDINGS_PUSHED.with(std::cell::Cell::get)
}

#[test]
fn an_element_of_k_declarations_and_n_children_pushes_k_bindings_and_not_k_times_n() {
    // Sixty declarations (under the cap of sixty-four) and 50,000 children, each with an attribute under one of the prefixes: the first round copied the
    // whole list of bindings for each child, 60 times 50,000 pushed and the same again allocated; one scope pushes the 60 once.
    let (k, n) = (60_usize, 50_000_usize);
    let declarations: Vec<(String, String)> = (0..k).map(|i| (format!("xmlns:p{i}"), format!("urn:p{i}"))).collect();
    let children: Vec<RawNode> = (0..n).map(|i| element("Child", vec![(format!("p{}:a", i % k), "1".to_owned())], Vec::new())).collect();
    let from = master_keeping(element("Wide", declarations, children));
    let mut doc = Document::new_blank();
    crate::stencil::refs::BINDINGS_PUSHED.with(|c| c.set(0));
    let started = Instant::now();
    let brought = bring_master(&mut doc, &from, 0, BOTH);
    println!("60 declarations and 50,000 children: {:?}, {} bindings pushed", started.elapsed(), pushed());
    assert!(brought.is_ok(), "{brought:?}");
    assert_eq!(pushed(), k, "one push for each declaration, and none for a child");
}

#[test]
fn an_element_that_declares_more_prefixes_than_a_copy_reads_is_refused_before_its_children_are_looked_at() {
    // The review's fixture: 30,000 declarations and 30,000 children (about 29 GB at a copy of the bindings for each child).
    let n = 30_000_usize;
    let declarations: Vec<(String, String)> = (0..n).map(|i| (format!("xmlns:p{i}"), format!("urn:p{i}"))).collect();
    let children: Vec<RawNode> = (0..n).map(|i| element("Child", vec![(format!("p{i}:a"), "1".to_owned())], Vec::new())).collect();
    let from = master_keeping(element("Wide", declarations, children));
    let mut doc = Document::new_blank();
    let before = doc.clone();
    crate::stencil::refs::BINDINGS_PUSHED.with(|c| c.set(0));
    let started = Instant::now();
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::KeptTooComplex));
    println!("30,000 declarations and 30,000 children: refused in {:?}, {} bindings pushed", started.elapsed(), pushed());
    assert_eq!(pushed(), 0, "refused before anything was pushed");
    assert_eq!(doc, before);
    // Nested past the scope's cap: five levels of sixty-four.
    let mut node = element("Leaf", Vec::new(), Vec::new());
    for level in (0..5).rev() {
        let declarations = (0..64).map(|i| (format!("xmlns:l{level}p{i}"), format!("urn:{level}:{i}"))).collect();
        node = element("Level", declarations, vec![node]);
    }
    assert_eq!(bring_master(&mut doc, &master_keeping(node), 0, BOTH), Err(Refusal::KeptTooComplex));
    assert!(Refusal::KeptTooComplex.to_string().contains("64 on one element"));
}

#[test]
fn a_shape_that_keeps_three_hundred_thousand_declarations_and_attributes_is_refused_in_linear_time() {
    // K declarations and K prefixed attributes on one shape's kept attributes: a scan of the bindings for each attribute is K squared over two compares,
    // 4.5e10 at K = 300,000. The declarations are counted in one pass and refused past the cap, and below it a lookup is a hash.
    let k = 300_000_usize;
    let mut raw: Vec<(String, String)> = (0..k).map(|i| (format!("xmlns:p{i}"), format!("urn:p{i}"))).collect();
    raw.extend((0..k).map(|i| (format!("p{i}:a"), "1".to_owned())));
    let mut from = Document::new_blank();
    let mut m = master(1, "Many", Some(A), Some(B));
    let mut shape = Shape::new(5);
    shape.raw = RawAttrs::from_pairs(raw.iter().map(|(a, b)| (a.as_str(), b.as_str())));
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    crate::stencil::refs::BINDINGS_PUSHED.with(|c| c.set(0));
    let started = Instant::now();
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::KeptTooComplex));
    let took = started.elapsed();
    println!("300,000 declarations and attributes on a shape: refused in {took:?}, {} bindings pushed", pushed());
    // Counted, not timed: refused with no declaration put on the stack, and no attribute looked up in the scope of one. A hang guard follows.
    assert_eq!(pushed(), 0, "refused before anything was pushed");
    assert!(took < std::time::Duration::from_secs(60), "{took:?}");
    // At the cap (sixty-four declarations and as many attributes) one lookup each is all it costs, and the shape copies.
    let mut raw: Vec<(String, String)> = (0..64).map(|i| (format!("xmlns:p{i}"), format!("urn:p{i}"))).collect();
    raw.extend((0..64).map(|i| (format!("p{i}:a"), "1".to_owned())));
    let m = Arc::make_mut(from.masters.get_mut(0).unwrap());
    let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
    shape.raw = RawAttrs::from_pairs(raw.iter().map(|(a, b)| (a.as_str(), b.as_str())));
    assert!(bring_master(&mut doc, &from, 0, BOTH).is_ok());
}

#[test]
fn a_relationship_in_a_texts_kept_element_a_connect_row_or_the_strict_namespace_refuses_the_master() {
    let rel =
        |namespace: &str| element("Thing", vec![("xmlns:q".to_owned(), namespace.to_owned()), ("q:id".to_owned(), "rId1".to_owned())], Vec::new());
    // A shape's text keeps an element with a relationship (walked where it is, not copied).
    let mut from = master_keeping(element("Unrelated", Vec::new(), Vec::new()));
    {
        let m = Arc::make_mut(from.masters.get_mut(0).unwrap());
        let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
        let mut text = Text::default();
        text.parts.push(TextPart::Chars(Arc::from("plain")));
        text.parts.push(TextPart::Raw(rel(crate::stencil::refs::NS_REL)));
        shape.text = Some(Arc::new(text));
    }
    let mut doc = Document::new_blank();
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)), "text");
    // The Strict form of the namespace is the same namespace.
    let from = master_keeping(rel(crate::stencil::refs::NS_REL_STRICT));
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)), "strict");
    // A `Connect` row of the master keeps an attribute the model has no field for.
    let mut from = master_keeping(element("Unrelated", Vec::new(), Vec::new()));
    {
        let m = Arc::make_mut(from.masters.get_mut(0).unwrap());
        let mut c = Connect::default();
        c.raw.push("xmlns:q", crate::stencil::refs::NS_REL);
        c.raw.push("q:id", "rId1");
        m.connects = Connects::from_vec(vec![c]);
    }
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)), "connect");
    assert_eq!(doc, Document::new_blank());
    // Unbound `r` is a relationship whatever its local name, and one bound here to something else is not.
    let from = master_keeping(element("Thing", vec![("r:whatever".to_owned(), "1".to_owned())], Vec::new()));
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)), "unbound r");
    let from =
        master_keeping(element("Thing", vec![("xmlns:r".to_owned(), "urn:other".to_owned()), ("r:id".to_owned(), "1".to_owned())], Vec::new()));
    assert!(bring_master(&mut doc, &from, 0, BOTH).is_ok());
}

// ------------------------------------------------------------------------------------------- a fingerprint of everything `==` compares

/// 2,000 styles of the drawing called "Same" that differ from the chain's only in what `variant` writes into the sheet (the chain's styles are written
/// with nothing there, so are different from every one of them): the work is a hash for each style and no comparison, for every field a sheet is
/// compared on.
fn differing_only_in(name: &str, variant: &dyn Fn(&mut Sheet, usize)) {
    let n = 2_000_u32;
    let from = alike_styles(n, true);
    let mut doc = Document::new_blank();
    for k in 1..=n {
        let next = if k == n { 0 } else { k + 1 };
        let mut sheet = Sheet { links: Links { line: Some(next), fill: Some(next), text: Some(next) }, ..Sheet::default() };
        sheet.cells.insert(CellName::new("LineWeight"), cell(&k.to_string()));
        variant(&mut sheet, k as usize);
        doc.add_style(Style { id: k, name: Some(Arc::from("Same")), name_u: Some(Arc::from("Same")), sheet: Arc::new(sheet), ..Style::default() })
            .unwrap();
    }
    SHEET_COMPARES.with(|c| c.set(0));
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    assert_eq!(brought.styles_added, n as usize, "{name}: none of the drawing's is the chain's");
    assert_eq!(compares(), 0, "{name}: the drawing's styles that differ only there did not hash as the chain's");
}

fn kept_element(k: usize) -> RawNode {
    element("X", vec![("k".to_owned(), k.to_string())], Vec::new())
}

fn attr(k: usize) -> RawAttrs {
    RawAttrs::from_pairs([("X", k.to_string().as_str())])
}

fn user_section(edit: impl FnOnce(&mut Section)) -> Arc<Section> {
    let mut section = Section::new("User", None);
    edit(&mut section);
    Arc::new(section)
}

#[test]
fn styles_that_differ_only_in_what_a_sheet_keeps_hash_apart() {
    let edit_cell = |sheet: &mut Sheet, f: &dyn Fn(&mut DocCell)| f(sheet.cells.get_mut("LineWeight").unwrap());
    differing_only_in("cell raw", &|sheet, k| edit_cell(sheet, &|c| c.raw = attr(k)));
    differing_only_in("cell extra text", &|sheet, k| {
        edit_cell(sheet, &|c| {
            c.extra =
                Some(Arc::new(CellExtra { text: Some(Arc::from(k.to_string().as_str())), passthrough: Passthrough::new(), ..CellExtra::default() }))
        });
    });
    differing_only_in("cell extra passthrough", &|sheet, k| {
        let mut kept = Passthrough::new();
        kept.push(0, kept_element(k));
        edit_cell(sheet, &|c| c.extra = Some(Arc::new(CellExtra { text: None, passthrough: kept.clone(), ..CellExtra::default() })));
    });
    differing_only_in("sheet raw", &|sheet, k| sheet.raw = attr(k));
    differing_only_in("sheet passthrough", &|sheet, k| sheet.passthrough.push(0, kept_element(k)));
    differing_only_in("section raw", &|sheet, k| sheet.sections.push(user_section(|s| s.raw = attr(k))));
    differing_only_in("section passthrough", &|sheet, k| sheet.sections.push(user_section(|s| s.passthrough.push(0, kept_element(k)))));
    differing_only_in("row raw", &|sheet, k| {
        sheet.sections.push(user_section(|s| s.rows.push(Arc::new(Row { name: Some(Arc::from("r")), raw: attr(k), ..Row::default() }))));
    });
    differing_only_in("row passthrough", &|sheet, k| {
        sheet.sections.push(user_section(|s| {
            let mut row = Row { name: Some(Arc::from("r")), ..Row::default() };
            row.passthrough.push(0, kept_element(k));
            s.rows.push(Arc::new(row));
        }));
    });
    differing_only_in("row local name", &|sheet, k| {
        sheet.sections.push(user_section(|s| {
            s.rows.push(Arc::new(Row { name: Some(Arc::from("r")), local_name: Some(Arc::from(k.to_string().as_str())), ..Row::default() }));
        }));
    });
}

// ------------------------------------------------------------------------------------------------: kept text, owners and cost

/// A node that declares `r` as something else, with a child when `with_child`.
fn declaring_r(with_child: bool) -> RawNode {
    let children = if with_child { vec![element("C", Vec::new(), Vec::new())] } else { Vec::new() };
    element("A", vec![("xmlns:r".to_owned(), "urn:other".to_owned())], children)
}

/// A node that holds a relationship under `r`, which is bound nowhere (so the closed rule makes it one).
fn using_r() -> RawNode {
    element("B", vec![("r:id".to_owned(), "rId1".to_owned())], Vec::new())
}

#[test]
fn a_declaration_is_taken_back_when_its_element_is_left_whether_it_has_children_or_not_and_whichever_sibling_comes_first() {
    // The walk takes its nodes from the end, so the two orders are the declaring element reached before the user of the prefix and after it: a
    // binding left behind by a leaf (`cut_back` in the `else`) or by an element with children (the `Leave` step) turns `r:id` into an attribute of
    // `urn:other`, and the relationship goes unseen.
    for (what, with_child) in [("a leaf", false), ("an element with children", true)] {
        for declaring_first in [true, false] {
            let mut doc = Document::new_blank();
            // Siblings at the top of one passthrough.
            let mut nodes = vec![declaring_r(with_child), using_r()];
            if !declaring_first {
                nodes.reverse();
            }
            let mut from = Document::new_blank();
            let mut m = master(1, "Keeps", Some(A), Some(B));
            let mut shape = Shape::new(5);
            for node in nodes {
                shape.passthrough.push(0, node);
            }
            m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
            from.add_master(m).unwrap();
            assert_eq!(
                bring_master(&mut doc, &from, 0, BOTH),
                Err(Refusal::Pictures(1)),
                "{what}, top-level siblings, declaring first: {declaring_first}"
            );
            // Siblings below one element.
            let mut children = vec![declaring_r(with_child), using_r()];
            if !declaring_first {
                children.reverse();
            }
            let from = master_keeping(element("Parent", Vec::new(), children));
            assert_eq!(
                bring_master(&mut doc, &from, 0, BOTH),
                Err(Refusal::Pictures(1)),
                "{what}, nested siblings, declaring first: {declaring_first}"
            );
        }
    }
    // Below the declaration `r` is `urn:other`, and the master copies.
    let declaring = element("A", vec![("xmlns:r".to_owned(), "urn:other".to_owned())], vec![using_r()]);
    let mut doc = Document::new_blank();
    assert!(bring_master(&mut doc, &master_keeping(declaring), 0, BOTH).is_ok());
}

/// A master whose kept text sits in `case`, a place the model has a field for that declares `prefix` as `uri`.
fn keeping_in(case: &str, node: RawNode, prefix: &str, uri: &str) -> Document {
    let key = format!("xmlns:{prefix}");
    let declared = || RawAttrs::from_pairs([(key.as_str(), uri)]);
    let kept = |node: RawNode| {
        let mut p = Passthrough::new();
        p.push(0, node);
        p
    };
    let mut doc = Document::new_blank();
    let mut m = master(1, "Keeps", Some(A), Some(B));
    let mut shape = Shape::new(5);
    let mut sheet = Sheet::default();
    match case {
        "shape" => {
            shape.raw = declared();
            shape.passthrough = kept(node);
        }
        "text of a shape" => {
            shape.raw = declared();
            let mut text = Text::default();
            text.parts.push(TextPart::Raw(node));
            shape.text = Some(Arc::new(text));
        }
        "sheet of a shape" => {
            shape.raw = declared();
            sheet.passthrough = kept(node);
        }
        "sheet" => {
            sheet.raw = declared();
            sheet.passthrough = kept(node);
        }
        "foreign data" => shape.foreign = Some(Arc::new(ForeignData { attrs: declared(), passthrough: kept(node), ..ForeignData::default() })),
        "cell" => {
            let mut c = cell("1");
            c.raw = declared();
            c.extra = Some(Arc::new(CellExtra { text: None, passthrough: kept(node), ..CellExtra::default() }));
            sheet.cells.insert(CellName::new("Foo"), c);
        }
        "section" => {
            let mut section = Section::new("User", None);
            section.raw = declared();
            section.passthrough = kept(node);
            sheet.sections.push(Arc::new(section));
        }
        "row" => {
            let mut section = Section::new("User", None);
            section.rows.push(Arc::new(Row { name: Some(Arc::from("r")), raw: declared(), passthrough: kept(node), ..Row::default() }));
            sheet.sections.push(Arc::new(section));
        }
        "master" => {
            m.raw.push(&key, uri);
            m.passthrough = kept(node);
        }
        "style" => {
            sheet.links = Links { line: Some(1), fill: None, text: None };
            doc.add_style(Style {
                id: 1,
                name: Some(Arc::from("S")),
                name_u: Some(Arc::from("S")),
                raw: declared(),
                passthrough: kept(node),
                ..Style::default()
            })
            .unwrap();
        }
        other => panic!("no case {other}"),
    }
    shape.sheet = Arc::new(sheet);
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    doc.add_master(m).unwrap();
    doc
}

#[test]
fn the_declarations_of_the_element_the_text_is_kept_in_are_in_scope_for_it() {
    // `<Shape xmlns:q="...relationships"><q:x q:custom="rId1"/></Shape>`: `q` is bound on the element the model has a field for and not inside the kept
    // text, and `custom` is no name the namespace defines, so only the binding says it is a relationship.
    let custom = || element("q:x", vec![("q:custom".to_owned(), "rId1".to_owned())], Vec::new());
    for case in PLACES {
        for uri in [crate::stencil::refs::NS_REL, crate::stencil::refs::NS_REL_STRICT] {
            let mut doc = Document::new_blank();
            let refused = bring_master(&mut doc, &keeping_in(case, custom(), "q", uri), 0, BOTH);
            assert_eq!(refused, Err(Refusal::Pictures(1)), "{case} binds q to {uri}");
        }
    }
}

const PLACES: [&str; 10] = ["shape", "text of a shape", "sheet of a shape", "sheet", "foreign data", "cell", "section", "row", "master", "style"];

#[test]
fn an_outer_declaration_never_lifts_the_closed_relationship_default() {
    // The element the model has a field for keeps its `xmlns:` declarations, but nothing makes the writer give them back at the same element: a
    // kept `r:id` under an `r` it rebound would be written under the part's root, where `r` is the relationships namespace, and dangle. So a binding
    // there to another namespace is not believed: `r`, and the names the namespace defines under any prefix, stay relationships.
    let element_with = |name: &str, attr: &str| element(name, vec![(attr.to_owned(), "rId1".to_owned())], Vec::new());
    for case in PLACES {
        let refused = |what: &str, prefix: &str, node: RawNode| {
            let mut doc = Document::new_blank();
            let got = bring_master(&mut doc, &keeping_in(case, node, prefix, "urn:other"), 0, BOTH);
            assert_eq!(got, Err(Refusal::Pictures(1)), "{case}: {prefix} rebound to urn:other by the owner, {what}");
        };
        refused("`r:id`", "r", element_with("x", "r:id"));
        refused("`r:whatever`, a name the namespace does not define", "r", element_with("x", "r:whatever"));
        refused("`q:embed`, a name it defines under another prefix", "q", element_with("x", "q:embed"));
        refused("`q:topLeft`, a name pending the spec check", "q", element_with("x", "q:topLeft"));
        // Nothing else becomes a relationship by it.
        let mut doc = Document::new_blank();
        let copied = bring_master(&mut doc, &keeping_in(case, element_with("x", "q:custom"), "q", "urn:other"), 0, BOTH);
        assert!(copied.is_ok(), "{case}: `q:custom` under a rebound prefix is no relationship: {copied:?}");
        // The kept element's own declaration is copied with it, and does lift it; so does one above it inside the kept text.
        let own = element("x", vec![("xmlns:r".to_owned(), "urn:third".to_owned()), ("r:id".to_owned(), "7".to_owned())], Vec::new());
        let mut doc = Document::new_blank();
        let copied = bring_master(&mut doc, &keeping_in(case, own, "r", "urn:other"), 0, BOTH);
        assert!(copied.is_ok(), "{case}: declared on the kept element itself: {copied:?}");
        let above = element("y", vec![("xmlns:r".to_owned(), "urn:third".to_owned())], vec![element_with("x", "r:id")]);
        let mut doc = Document::new_blank();
        let copied = bring_master(&mut doc, &keeping_in(case, above, "r", "urn:other"), 0, BOTH);
        assert!(copied.is_ok(), "{case}: declared above the kept element inside the kept text: {copied:?}");
    }
}

#[test]
fn a_node_is_charged_before_it_is_stacked_so_a_wide_element_is_refused_with_next_to_nothing_stacked() {
    let n = 200_000_usize;
    // Below one element.
    let children: Vec<RawNode> = (0..n).map(|_| element("C", Vec::new(), Vec::new())).collect();
    let from = master_keeping(element("Wide", Vec::new(), children));
    let mut doc = Document::new_blank();
    STEPS_PUSHED.with(|c| c.set(0));
    assert_eq!(bring_with(&mut doc, &from, 0, BOTH, 1_000), Err(Refusal::TooBig));
    let below = STEPS_PUSHED.with(std::cell::Cell::get);
    println!("{n} children, a budget of 1,000: {below} steps stacked");
    assert!(below <= 1_000, "{below} steps were stacked before the meter said no");
    // At the top of one passthrough.
    let mut from = Document::new_blank();
    let mut m = master(1, "Keeps", Some(A), Some(B));
    let mut shape = Shape::new(5);
    for _ in 0..n {
        shape.passthrough.push(0, element("C", Vec::new(), Vec::new()));
    }
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    STEPS_PUSHED.with(|c| c.set(0));
    assert_eq!(bring_with(&mut doc, &from, 0, BOTH, 1_000), Err(Refusal::TooBig));
    let top = STEPS_PUSHED.with(std::cell::Cell::get);
    println!("{n} top-level nodes, a budget of 1,000: {top} steps stacked");
    assert!(top <= 1_000, "{top} steps were stacked before the meter said no");
    // With the room, the same master copies and every node is stacked once.
    STEPS_PUSHED.with(|c| c.set(0));
    assert!(bring_with(&mut doc, &from, 0, BOTH, 10_000_000).is_ok());
    assert_eq!(STEPS_PUSHED.with(std::cell::Cell::get), n);
}

/// A drawing and a stencil that each hold one style "Same", the same in both, whose cell `LineWeight` is `edit`ed (one allocation each, so the two
/// are equal and not the same). The stencil's master is linked to it.
fn same_big_style(edit: &dyn Fn(&mut DocCell)) -> (Document, Document) {
    let build = |with_master: bool| {
        let mut doc = Document::new_blank();
        let mut sheet = Sheet::default();
        let mut c = cell("1");
        edit(&mut c);
        sheet.cells.insert(CellName::new("LineWeight"), c);
        doc.add_style(Style { id: 1, name: Some(Arc::from("Same")), name_u: Some(Arc::from("Same")), sheet: Arc::new(sheet), ..Style::default() })
            .unwrap();
        if with_master {
            let mut m = master(1, "Linked", Some(A), Some(B));
            let mut shape = Shape::new(5);
            shape.sheet = Arc::new(Sheet { links: Links { line: Some(1), fill: None, text: None }, ..Sheet::default() });
            m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
            doc.add_master(m).unwrap();
        }
        doc
    };
    (build(false), build(true))
}

#[test]
fn a_comparison_of_styles_is_charged_for_the_bytes_it_compares_as_well_as_the_nodes() {
    // 3,200,000 bytes are 100,000 units at 32 bytes to the unit, as `Survey::cell` counts a formula's.
    let big = || "x".repeat(3_200_000);
    type Edit = Box<dyn Fn(&mut DocCell)>;
    let cases: [(&str, Edit, usize); 2] = [
        // A formula is charged by the survey once (100,000): the budget leaves room for that and not for a comparison of it as well.
        ("formula", Box::new(move |c: &mut DocCell| *c = DocCell::with_formula(&big(), None)), 150_000),
        // A kept attribute's value is charged by nothing but the comparison.
        ("kept attribute", Box::new(move |c: &mut DocCell| c.raw = RawAttrs::from_pairs([("X", big().as_str())])), 20_000),
    ];
    for (what, edit, budget) in cases {
        let (mut doc, from) = same_big_style(&*edit);
        assert_eq!(bring_with(&mut doc, &from, 0, BOTH, budget).map(|_| ()), Err(Refusal::TooBig), "{what}: a budget of {budget}");
        let (mut doc, from) = same_big_style(&*edit);
        SHEET_COMPARES.with(|c| c.set(0));
        let brought = bring_with(&mut doc, &from, 0, BOTH, 10_000_000).unwrap();
        assert_eq!(
            (brought.styles_added, compares()),
            (0, 1),
            "{what}: with the room, the drawing's style is the stencil's, found by one comparison"
        );
    }
}

// ------------------------------------------------------------------------------- the declarations of the elements above a kept node, in one sheet

/// A master whose shape's sheet holds a cell `Top`, and a section `User` with a cell `Mid` and a row `r` with a cell `Low`. `declarer` (`shape`,
/// `sheet`, `section` or `row`) declares `q` as `uri` and `node` is kept at `user`: a `... cell` is that cell's extra content, `row` and `section`
/// their own kept elements.
fn nested(declarer: &str, user: &str, uri: &str, node: RawNode) -> Document {
    let declared = || RawAttrs::from_pairs([("xmlns:q", uri)]);
    let kept = |at: &str| {
        let mut p = Passthrough::new();
        if user == at {
            p.push(0, node.clone());
        }
        p
    };
    let cell_at = |at: &str| {
        let mut c = cell("1");
        c.extra = Some(Arc::new(CellExtra { text: None, passthrough: kept(at), ..CellExtra::default() }));
        c
    };
    let mut shape = Shape::new(5);
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("Top"), cell_at("sheet cell"));
    let mut section = Section::new("User", None);
    section.cells.insert(CellName::new("Mid"), cell_at("section cell"));
    section.passthrough = kept("section");
    let mut row = Row { name: Some(Arc::from("r")), passthrough: kept("row"), ..Row::default() };
    row.cells.insert(CellName::new("Low"), cell_at("row cell"));
    match declarer {
        "shape" => shape.raw = declared(),
        "sheet" => sheet.raw = declared(),
        "section" => section.raw = declared(),
        "row" => row.raw = declared(),
        other => panic!("no declarer {other}"),
    }
    section.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(section));
    shape.sheet = Arc::new(sheet);
    let mut m = master(1, "Keeps", Some(A), Some(B));
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::new_blank();
    doc.add_master(m).unwrap();
    doc
}

#[test]
fn a_prefix_bound_on_an_element_above_a_kept_node_in_the_same_sheet_is_judged_by_what_it_is_bound_to() {
    // `custom` is no name the namespace defines, so only the binding makes `q:custom` a relationship. A binding to another namespace on an element above
    // adds none and, as an outer declaration, does not lift the closed default either: `q:id` is refused and `q:custom` is not.
    let custom = || element("q:x", vec![("q:custom".to_owned(), "rId1".to_owned())], Vec::new());
    let named = || element("x", vec![("q:id".to_owned(), "rId1".to_owned())], Vec::new());
    let cases = [
        ("shape", "sheet cell"),
        ("shape", "section"),
        ("shape", "section cell"),
        ("shape", "row"),
        ("shape", "row cell"),
        ("sheet", "section cell"),
        ("sheet", "row cell"),
        ("section", "row"),
        ("section", "row cell"),
        ("row", "row cell"),
    ];
    for (declarer, user) in cases {
        let mut doc = Document::new_blank();
        let refused = bring_master(&mut doc, &nested(declarer, user, crate::stencil::refs::NS_REL, custom()), 0, BOTH);
        assert_eq!(refused, Err(Refusal::Pictures(1)), "`q` bound on the {declarer}, used by the kept text of the {user}");
        let mut doc = Document::new_blank();
        let kept_out = bring_master(&mut doc, &nested(declarer, user, "urn:other", named()), 0, BOTH);
        assert_eq!(kept_out, Err(Refusal::Pictures(1)), "`q` rebound on the {declarer} does not lift `q:id` of the {user}");
        let mut doc = Document::new_blank();
        let copied = bring_master(&mut doc, &nested(declarer, user, "urn:other", custom()), 0, BOTH);
        assert!(copied.is_ok(), "`q` bound to another namespace on the {declarer} makes `q:custom` of the {user} no relationship: {copied:?}");
    }
}

#[test]
fn a_cells_own_kept_attribute_is_judged_with_the_declarations_above_it() {
    // The shape binds `q` to the relationships namespace; the cell has an attribute `q:custom` of its own.
    let mut shape = Shape::new(5);
    shape.raw = RawAttrs::from_pairs([("xmlns:q", crate::stencil::refs::NS_REL)]);
    let mut sheet = Sheet::default();
    let mut c = cell("1");
    c.raw = RawAttrs::from_pairs([("q:custom", "rId1")]);
    sheet.cells.insert(CellName::new("Foo"), c);
    shape.sheet = Arc::new(sheet);
    let mut m = master(1, "Keeps", Some(A), Some(B));
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut from = Document::new_blank();
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)));
}

/// A master with two siblings at one level of a sheet (`cell`, `row` or `section`): sibling `first` declares `q` as the relationships namespace and the
/// other keeps `q:custom` in its kept text.
fn siblings(level: &str, first: usize) -> Document {
    let declared = RawAttrs::from_pairs([("xmlns:q", crate::stencil::refs::NS_REL)]);
    let using = || {
        let mut p = Passthrough::new();
        p.push(0, element("q:x", vec![("q:custom".to_owned(), "rId1".to_owned())], Vec::new()));
        p
    };
    let mut sheet = Sheet::default();
    for k in 0..2_usize {
        let declaring = k == first;
        match level {
            "cell" => {
                let mut c = cell("1");
                if declaring {
                    c.raw = declared.clone();
                } else {
                    c.extra = Some(Arc::new(CellExtra { text: None, passthrough: using(), ..CellExtra::default() }));
                }
                sheet.cells.insert(CellName::new(&format!("C{k}")), c);
            }
            "row" => {
                if k == 0 {
                    sheet.sections.push(Arc::new(Section::new("User", None)));
                }
                let mut row = Row { name: Some(Arc::from(format!("r{k}").as_str())), ix: Some(k as u32), ..Row::default() };
                if declaring {
                    row.raw = declared.clone();
                } else {
                    row.passthrough = using();
                }
                if let Some(section) = sheet.sections.get_mut(0) {
                    Arc::make_mut(section).rows.push(Arc::new(row));
                }
            }
            "section" => {
                let mut section = Section::new(&format!("User{k}"), None);
                if declaring {
                    section.raw = declared.clone();
                } else {
                    section.passthrough = using();
                }
                sheet.sections.push(Arc::new(section));
            }
            other => panic!("no level {other}"),
        }
    }
    let mut shape = Shape::new(5);
    shape.sheet = Arc::new(sheet);
    let mut m = master(1, "Keeps", Some(A), Some(B));
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::new_blank();
    doc.add_master(m).unwrap();
    doc
}

#[test]
fn a_binding_declared_on_one_sibling_is_not_seen_by_the_next() {
    // Whichever of the two declares, the other's kept `q:custom` is under an unbound prefix and a name outside the list: not a relationship. A binding
    // that outlived its cell, row or section would make it one.
    for level in ["cell", "row", "section"] {
        for first in [0, 1] {
            let mut doc = Document::new_blank();
            let copied = bring_master(&mut doc, &siblings(level, first), 0, BOTH);
            assert!(copied.is_ok(), "{level}, sibling {first} declares: {copied:?}");
        }
    }
}

// ----------------------------------------------------------------------------- the master's own slot (the security review)

/// A master whose slot keeps, for its part's `Key` (`Shapes` or `Connects`), an attribute of the element and `n` elements of one attribute `child` each, and, when `root` is given, that
/// declaration on the root of its part.
fn master_with_kept(key: &str, n: usize, child: (&str, &str), root: Option<(&str, &str)>) -> Document {
    let mut from = Document::new_blank();
    let mut m = master(1, "Keeps", Some(A), Some(B));
    m.shapes = ShapeList::from_vec(vec![Arc::new(Shape::new(5))]);
    let mut kept = Passthrough::new();
    for _ in 0..n {
        kept.push(0, element("zz:x", vec![(child.0.to_owned(), child.1.to_owned())], Vec::new()));
    }
    let slot = m.ext.make_mut();
    if let Some(declared) = root {
        slot.vsdx.0.push((Arc::from("MasterContents"), Kept { raw: RawAttrs::from_pairs([declared]), kept: Passthrough::new() }));
    }
    slot.vsdx.0.push((Arc::from(key), Kept { raw: RawAttrs::from_pairs([("zz:kept", "1")]), kept }));
    from.add_master(m).unwrap();
    from
}

/// The smallest meter the master of `from` is brought in with.
fn least_budget(from: &Document) -> usize {
    let (mut lo, mut hi) = (0_usize, 1_usize << 22);
    assert!(bring_with(&mut Document::new_blank(), from, 0, BOTH, hi).is_ok(), "the master is brought in with a large meter");
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if bring_with(&mut Document::new_blank(), from, 0, BOTH, mid).is_ok() {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

#[test]
fn what_a_masters_slot_keeps_is_charged_to_the_meter_by_its_size() {
    // An element kept beside the shapes costs the meter one for itself and one for each attribute, as every kept part does: 2 for each of these, so the least meter the master is
    // brought in with grows by exactly that, for the nodes of its `Shapes` and of its `Connects`.
    let child = ("zz:a", "1");
    for key in ["Shapes", "Connects"] {
        let none = least_budget(&master_with_kept(key, 0, child, None));
        for n in [1_usize, 1_000, 2_000] {
            assert_eq!(least_budget(&master_with_kept(key, n, child, None)), none + 2 * n, "{key}, {n} kept elements");
        }
    }
    // The root's prefixes are charged too, once for the entry and once for each entry they are in scope for.
    let declared = least_budget(&master_with_kept("Shapes", 0, child, Some(("xmlns:zz", "urn:zz"))));
    assert_eq!(
        declared,
        least_budget(&master_with_kept("Shapes", 0, child, None)) + 3,
        "the root's entry (1), its prefix (1) and the prefix in scope for `Shapes` (1)"
    );
}

#[test]
fn a_master_whose_slot_keeps_more_than_the_meter_holds_is_refused_in_words_and_the_drawing_is_unchanged() {
    // Without the charge, a stencil master's kept markup is copied into the drawing whatever its size: the meter saw none of it.
    let base = least_budget(&master_with_kept("Shapes", 0, ("zz:a", "1"), None));
    for key in ["Shapes", "Connects"] {
        let from = master_with_kept(key, 5_000, ("zz:a", "1"), None);
        let mut doc = Document::new_blank();
        let before = doc.clone();
        let refused = bring_with(&mut doc, &from, 0, BOTH, base + 100);
        assert_eq!(refused, Err(Refusal::TooBig), "{key}");
        assert!(Refusal::TooBig.to_string().contains("too large to copy"), "{}", Refusal::TooBig);
        assert_eq!(doc, before, "{key}: nothing is changed");
    }
}

/// The `Shapes` entry of the master `brought` in `doc`, and whether one of its kept elements still has the attribute `name`.
fn kept_shapes_has(doc: &Document, brought: u32, name: &str) -> (usize, bool) {
    let copy = doc.master(brought).unwrap();
    let shapes =
        copy.ext.get().and_then(|e| e.vsdx.0.iter().find(|(k, _)| &**k == "Shapes").map(|(_, v)| v.clone())).expect("the copy keeps its `Shapes`");
    let has = shapes.kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if e.attrs.get(name).is_some()));
    (shapes.kept.iter().count(), has)
}

#[test]
fn a_relationship_in_what_a_masters_slot_keeps_is_left_out_and_said_judged_with_the_prefixes_of_its_parts_root() {
    // The second security review: such an attribute names a row of the stencil master's part, which the drawing does not have, so it would dangle. It is taken out (the rest stays), the
    // answer says so, and the master is brought in: a reference costs one attribute and not the master.
    let rel = crate::stencil::refs::NS_REL;
    for key in ["Shapes", "Connects"] {
        let from = master_with_kept(key, 2, ("q:target", "rId9"), Some(("xmlns:q", rel)));
        let mut doc = Document::new_blank();
        let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert!(brought.notes.iter().any(|n| n.contains("2 reference(s) to parts of the stencil's file")), "{key}: {:?}", brought.notes);
        let copy = doc.master(brought.id).unwrap();
        let entry = copy.ext.get().and_then(|e| e.vsdx.0.iter().find(|(k, _)| &**k == key).map(|(_, v)| v.clone())).unwrap();
        assert_eq!(entry.kept.iter().count(), 2, "{key}: the elements stay");
        assert!(entry.kept.iter().all(|p| matches!(&p.node, RawNode::Element(e) if e.attrs.get("q:target").is_none())), "{key}: their references go");
        assert_eq!(entry.raw.get("zz:kept"), Some("1"), "{key}: the element's own attribute stays");
    }
    // With no declaration of the prefix, the same attribute is in no namespace the copy knows: it stays, and nothing is said. The root's prefixes are what decided.
    let from = master_with_kept("Shapes", 1, ("q:target", "rId9"), None);
    let mut doc = Document::new_blank();
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    assert!(brought.notes.is_empty(), "{:?}", brought.notes);
    assert_eq!(kept_shapes_has(&doc, brought.id, "q:target"), (1, true));
    // The source is untouched.
    let source = from.masters.iter().next().unwrap();
    assert!(source.ext.get().is_some_and(|e| !e.vsdx.0.is_empty()));
}
