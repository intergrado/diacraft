//! The list patcher and its helpers, on a toy list that is not a sheet: `patch_list` is what a format patches its own lists with (pages, masters,
//! shapes), so its rules are tried on their own. The rules are in its documentation: the children of the parent that are not items are trailers
//! that go with the item before them; an item taken out leaves its trailer with the item before it; a new item goes right after the item before
//! it in the new list, ahead of that item's trailer, and one with none before it goes where `first_at` says.

use std::cell::Cell;

use diacraft_container::xml::{El, Node, write_element};
use diacraft_sheetxml::patch::{Items, after_last, at_end, has_name, is_ours, match_items, patch_list, set_attr, sync_attrs};
use diacraft_sheetxml::{PatchError, Scope};

use super::{Main, parse};

#[derive(Clone, Debug, PartialEq)]
struct Item {
    key: u32,
    text: &'static str,
}

fn item(key: u32, text: &'static str) -> Item {
    Item { key, text }
}

fn text_of(el: &El) -> String {
    let mut out = Vec::new();
    write_element(&mut out, el).unwrap();
    String::from_utf8(out).unwrap()
}

fn made(i: &Item) -> El {
    El { name: "i".to_owned(), attrs: vec![("k".to_owned(), i.key.to_string()), ("t".to_owned(), i.text.to_owned())], children: vec![] }
}

/// What patching `xml` from `base` to `now` gives, and how many items were patched in place.
fn patched(xml: &str, base: &[Item], now: &[Item], first_at: &dyn Fn(&[Node]) -> usize) -> Result<(String, usize), PatchError> {
    let mut el = parse(xml);
    let sc = Scope::<Main>::empty().enter(&el);
    let kind = Items {
        modelled: &|c: &El, csc: &Scope<Main>| is_ours(c, csc, "i"),
        key: &|i: &Item| Some(i.key),
        first_at,
        is_it: &|e: &El, i: &Item| e.attr("k") == Some(i.key.to_string().as_str()),
    };
    let calls = Cell::new(0usize);
    let (b, n): (Vec<&Item>, Vec<&Item>) = (base.iter().collect(), now.iter().collect());
    patch_list(
        &mut el,
        &sc,
        &kind,
        &b,
        &n,
        &mut |e, _, old, new| {
            calls.set(calls.get() + 1);
            sync_attrs(e, &[("t".to_owned(), old.text.to_owned())], &[("t".to_owned(), new.text.to_owned())]);
            Ok(())
        },
        &mut |i| Ok(made(i)),
    )?;
    Ok((text_of(&el), calls.get()))
}

const TWO: &str = r#"<p><h/><i k="1" t="a"/>T1<i k="2" t="b"/>T2</p>"#;

#[test]
fn a_list_that_did_not_change_is_not_touched_and_nothing_is_patched() {
    let list = [item(1, "a"), item(2, "b")];
    assert_eq!(patched(TWO, &list, &list, &at_end).unwrap(), (text_of(&parse(TWO)), 0));
}

#[test]
fn an_item_that_changed_is_patched_in_place_and_the_others_and_the_trailers_stay() {
    let (out, calls) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(1, "a"), item(2, "B")], &at_end).unwrap();
    assert_eq!((out.as_str(), calls), (r#"<p><h/><i k="1" t="a"/>T1<i k="2" t="B"/>T2</p>"#, 1));
}

#[test]
fn an_item_taken_out_leaves_its_trailer_with_the_item_before_it_or_with_the_start() {
    let (first_gone, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(2, "b")], &at_end).unwrap();
    assert_eq!(first_gone, r#"<p><h/>T1<i k="2" t="b"/>T2</p>"#);
    let (last_gone, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(1, "a")], &at_end).unwrap();
    assert_eq!(last_gone, r#"<p><h/><i k="1" t="a"/>T1T2</p>"#);
    let (all_gone, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[], &at_end).unwrap();
    assert_eq!(all_gone, "<p><h/>T1T2</p>");
}

#[test]
fn a_new_item_goes_right_after_the_item_before_it_ahead_of_that_items_trailer() {
    let (out, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(1, "a"), item(3, "c"), item(2, "b")], &at_end).unwrap();
    assert_eq!(out, r#"<p><h/><i k="1" t="a"/><i k="3" t="c"/>T1<i k="2" t="b"/>T2</p>"#);
    let (at_the_end, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(1, "a"), item(2, "b"), item(3, "c")], &at_end).unwrap();
    assert_eq!(at_the_end, r#"<p><h/><i k="1" t="a"/>T1<i k="2" t="b"/><i k="3" t="c"/>T2</p>"#);
}

#[test]
fn a_new_item_with_none_before_it_goes_where_the_list_says_and_ahead_of_the_first_item() {
    let first_after_h = |lead: &[Node]| after_last(lead, &["h"]);
    let (out, _) = patched(r#"<p><h/><x/></p>"#, &[], &[item(1, "a")], &first_after_h).unwrap();
    assert_eq!(out, r#"<p><h/><i k="1" t="a"/><x/></p>"#);
    // With items already there, one put first goes where the list says, which is before them, and the lead's other nodes stay ahead of it.
    let (before, _) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(3, "c"), item(1, "a"), item(2, "b")], &first_after_h).unwrap();
    assert_eq!(before, r#"<p><h/><i k="3" t="c"/><i k="1" t="a"/>T1<i k="2" t="b"/>T2</p>"#);
    // With no `h` the index is 0, ahead of everything; and `first_at` past the end is held to the end.
    let (zero, _) = patched(r#"<p><x/></p>"#, &[], &[item(1, "a")], &|l| after_last(l, &["h"])).unwrap();
    assert_eq!(zero, r#"<p><i k="1" t="a"/><x/></p>"#);
    let (past, _) = patched(r#"<p><x/></p>"#, &[], &[item(1, "a")], &|_| 99).unwrap();
    assert_eq!(past, r#"<p><x/><i k="1" t="a"/></p>"#);
}

#[test]
fn items_in_another_order_move_with_their_trailers() {
    let (out, calls) = patched(TWO, &[item(1, "a"), item(2, "b")], &[item(2, "b"), item(1, "a")], &at_end).unwrap();
    assert_eq!(out, r#"<p><h/><i k="2" t="b"/>T2<i k="1" t="a"/>T1</p>"#);
    assert_eq!(calls, 0);
}

#[test]
fn a_tree_that_does_not_hold_the_items_the_model_was_read_from_is_a_mismatch() {
    // One item too few, one too many, and one that is another item than the model's.
    let two = [item(1, "a"), item(2, "b")];
    for (xml, base) in [
        (r#"<p><i k="1" t="a"/></p>"#, &two[..]),
        (TWO, &two[..1]),
        (r#"<p><i k="1" t="a"/><i k="9" t="b"/></p>"#, &two[..]),
        (r#"<p><i k="2" t="b"/><i k="1" t="a"/></p>"#, &two[..]),
    ] {
        let e = patched(xml, base, base, &at_end).unwrap_err();
        assert_eq!(e, PatchError::Mismatch("a part does not hold the items the model was read from"), "{xml}");
    }
}

#[test]
fn items_are_matched_by_the_same_allocation_first_and_then_by_key_first_come_first_served() {
    let key = |i: &Item| Some(i.key);
    let (a, b) = (item(1, "a"), item(1, "b"));
    // The same allocation twice in the new list: it is the base item once, and the second is new.
    let (matched, used) = match_items(&[&a], &[&a, &a], &key);
    assert_eq!((matched, used), (vec![Some(0), None], vec![true]));
    // Two items of one key, replaced by two others of it: in order.
    let (c, d) = (item(1, "c"), item(1, "d"));
    let (matched, used) = match_items(&[&a, &b], &[&c, &d], &key);
    assert_eq!((matched, used), (vec![Some(0), Some(1)], vec![true, true]));
    // A base item that the allocation claimed is not left for the key: `c` has the key of both, and takes the one the pointer did not.
    let (matched, used) = match_items(&[&a, &b], &[&b, &c], &key);
    assert_eq!((matched, used), (vec![Some(1), Some(0)], vec![true, true]));
    // An item with no key, and one whose key the base has not, are new.
    let (e, f) = (item(7, "e"), item(8, "f"));
    let (matched, used) = match_items(&[&a], &[&e, &f], &|i: &Item| (i.key != 8).then_some(i.key));
    assert_eq!((matched, used), (vec![None, None], vec![false]));
}

#[test]
fn the_helpers_that_find_places_and_names_say_what_they_say() {
    let nodes = parse(r#"<p><a/>t<b/><a/>u<c/></p>"#).children;
    assert_eq!(after_last(&nodes, &["a"]), 4);
    assert_eq!(after_last(&nodes, &["b", "c"]), 6);
    assert_eq!(after_last(&nodes, &["a", "b", "c"]), 6);
    assert_eq!(after_last(&nodes, &["z"]), 0);
    assert_eq!(at_end(&nodes), 6);
    assert_eq!(at_end(&[]), 0);
    assert!(has_name(&parse(r#"<Cell N="A"/>"#)));
    assert!(!has_name(&parse(r#"<Cell N=""/>"#)));
    assert!(!has_name(&parse(r#"<Cell/>"#)));
    assert!(!has_name(&parse(r#"<Cell Name="A"/>"#)));
}

#[test]
fn an_attribute_is_set_in_its_place_or_last_and_synced_where_the_two_lists_differ() {
    let mut el = parse(r#"<c a="1" b="2" z="9"/>"#);
    set_attr(&mut el, "b", "x");
    set_attr(&mut el, "n", "new");
    assert_eq!(text_of(&el), r#"<c a="1" b="x" z="9" n="new"/>"#);
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    // `a` is the same in both and is left alone (even where the element holds another text), `b` changed, `c` was dropped, `d` is new.
    let mut el = parse(r#"<c a="kept" b="2" c="3" z="9"/>"#);
    sync_attrs(&mut el, &[pair("a", "1"), pair("b", "2"), pair("c", "3")], &[pair("a", "1"), pair("b", "5"), pair("d", "4")]);
    assert_eq!(text_of(&el), r#"<c a="kept" b="5" z="9" d="4"/>"#);
    // A name the two lists give the same text is not set, and one that only the old list had is removed.
    let mut el = parse(r#"<c a="kept" gone="1"/>"#);
    sync_attrs(&mut el, &[pair("a", "1"), pair("gone", "1")], &[pair("a", "1")]);
    assert_eq!(text_of(&el), r#"<c a="kept"/>"#);
}

// ------------------------------------------------------------------------------------------------------------- what an error leaves

/// Patches `xml` from `base` to `now` with the closures given, and gives the result and the text of the tree whatever the result is.
fn patched_any(
    xml: &str,
    base: &[Item],
    now: &[Item],
    first_at: &dyn Fn(&[Node]) -> usize,
    patch: &mut dyn FnMut(&mut El, &Item, &Item) -> Result<(), PatchError>,
    make: &mut dyn FnMut(&Item) -> Result<El, PatchError>,
) -> (Result<(), PatchError>, String) {
    let mut el = parse(xml);
    let sc = Scope::<Main>::empty().enter(&el);
    let kind = Items {
        modelled: &|c: &El, csc: &Scope<Main>| is_ours(c, csc, "i"),
        key: &|i: &Item| Some(i.key),
        first_at,
        is_it: &|e: &El, i: &Item| e.attr("k") == Some(i.key.to_string().as_str()),
    };
    let (b, n): (Vec<&Item>, Vec<&Item>) = (base.iter().collect(), now.iter().collect());
    let result = patch_list(&mut el, &sc, &kind, &b, &n, &mut |e, _, old, new| patch(e, old, new), make);
    (result, text_of(&el))
}

/// The nodes of `xml`'s root, each as text, sorted: what is in a tree, whatever the order.
fn nodes_of(xml: &str) -> Vec<String> {
    let mut nodes: Vec<String> = parse(xml)
        .children
        .iter()
        .map(|n| match n {
            Node::El(e) => text_of(e),
            Node::Text(t) => format!("text:{t}"),
            other => format!("{other:?}"),
        })
        .collect();
    nodes.sort();
    nodes
}

fn set_t(e: &mut El, text: &str) {
    set_attr(e, "t", text);
}

#[test]
fn a_tree_that_does_not_hold_the_items_of_the_model_is_left_exactly_as_it_was() {
    // The mismatch is found before anything is moved or changed, so the parent keeps every child, in the order it had them.
    let two = [item(1, "a"), item(2, "b")];
    for (xml, base) in [
        (r#"<p><h/><i k="1" t="a"/>T1</p>"#, &two[..]),
        (TWO, &two[..1]),
        (r#"<p>x<i k="1" t="a"/><!--c--><i k="9" t="b"/>y<h/></p>"#, &two[..]),
        (r#"<p><i k="2" t="b"/>T<i k="1" t="a"/></p>"#, &two[..]),
        (r#"<p/>"#, &two[..]),
    ] {
        let (result, after) = patched_any(xml, base, base, &at_end, &mut |_, _, _| Ok(()), &mut |i| Ok(made(i)));
        assert_eq!(result, Err(PatchError::Mismatch("a part does not hold the items the model was read from")), "{xml}");
        assert_eq!(after, text_of(&parse(xml)), "{xml}");
    }
}

#[test]
fn an_item_that_fails_in_the_middle_leaves_every_node_in_the_tree_and_the_edits_before_it() {
    let (base, now) = ([item(1, "a"), item(2, "b")], [item(1, "A"), item(2, "B")]);
    // The second item fails after setting its text: the first is patched, the second is as the closure left it, and the rest is where it was.
    let (result, after) = patched_any(
        TWO,
        &base,
        &now,
        &at_end,
        &mut |e, _, new| {
            set_t(e, new.text);
            if new.key == 2 { Err(PatchError::Unsupported("no")) } else { Ok(()) }
        },
        &mut |i| Ok(made(i)),
    );
    assert_eq!(result, Err(PatchError::Unsupported("no")));
    assert_eq!(after, r#"<p><h/><i k="1" t="A"/>T1<i k="2" t="B"/>T2</p>"#);

    // An item taken out is still there when a later one fails, and so is each trailer: nothing is lost, whatever the order.
    let (result, after) = patched_any(TWO, &base, &[item(2, "B")], &at_end, &mut |_, _, _| Err(PatchError::Unsupported("no")), &mut |i| Ok(made(i)));
    assert_eq!(result, Err(PatchError::Unsupported("no")));
    assert_eq!(nodes_of(&after), nodes_of(TWO));
    assert_eq!(parse(&after).children.len(), parse(TWO).children.len());
}

#[test]
fn a_new_item_that_cannot_be_made_leaves_every_node_in_the_tree() {
    let first_after_h = |lead: &[Node]| after_last(lead, &["h"]);
    // After an item that was matched, and before any.
    let after_one =
        patched_any(TWO, &[item(1, "a"), item(2, "b")], &[item(1, "a"), item(3, "c"), item(2, "b")], &at_end, &mut |_, _, _| Ok(()), &mut |_| {
            Err(PatchError::Unsupported("no made"))
        });
    assert_eq!(after_one, (Err(PatchError::Unsupported("no made")), text_of(&parse(TWO))));
    let xml = r#"<p><h/><x/></p>"#;
    let before_any = patched_any(xml, &[], &[item(1, "a")], &first_after_h, &mut |_, _, _| Ok(()), &mut |_| Err(PatchError::Unsupported("no made")));
    assert_eq!(before_any, (Err(PatchError::Unsupported("no made")), text_of(&parse(xml))));
}
