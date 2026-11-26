//! The content check sets the ordered children of an element side by side by what they are, and not by their place, on VSDX packages built in code from the specs.
//! Of 11 differences measured in the VSDX visit, 7 were "a shape's children written in another order": a different order of a shape's children is one **order**
//! difference (the writer's to mend: a shape's children follow the schema's sequence), and what is inside each child is still compared. Set side by side by
//! place, the children that stood in another order were each compared with the child in their place, counted as an element each, and nothing inside them was looked at.
//!
//! Each test has a **masking control**: a difference planted deep inside a child that stands in another place, or after a shape put in or taken out, is counted and found.

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Limits, compare_content};

/// The folder of the format's parts, written in pieces.
const PAGE1: &str = concat!("/vi", "sio", "/pages/page1.xml");

/// A package of one page holding `shapes` (the XML inside the page's `Shapes`).
fn package(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(shapes)).build()
}

/// The comparison of two one-page packages, both ways round: the count is symmetric.
fn diff(a: &str, b: &str) -> ContentDiff {
    let (pa, pb) = (package(a), package(b));
    let (ab, ba) = (compare_content(&pa, &pb, &Limits::default()).unwrap(), compare_content(&pb, &pa, &Limits::default()).unwrap());
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

/// The differences as `(kind, path)`, sorted, all in the page.
fn seen(d: &ContentDiff) -> Vec<(&'static str, String)> {
    assert!(d.shown.iter().all(|x| x.part == PAGE1), "{:?}", d.shown);
    let mut out: Vec<(&'static str, String)> = d.shown.iter().map(|x| (x.found.word(), x.path.clone())).collect();
    out.sort();
    out
}

fn kinds(d: &ContentDiff, found: Found) -> usize {
    d.shown.iter().filter(|x| x.found == found).count()
}

// ------------------------------------------------------------------------------------------------------------- a shape's children in another order

/// A group of ID 10: the child shapes it holds with a cell of value `v`, and a text, in the order the writer writes them (the shapes before the text), or the other.
fn group(v: &str, text_first: bool) -> String {
    let children = format!("<Shapes>{}</Shapes>", shape(11, &cell("PinX", v)));
    let text = "<Text>hello</Text>";
    shape(10, &if text_first { format!("{text}{children}") } else { format!("{children}{text}") })
}

#[test]
fn a_shapes_children_in_another_order_are_one_order() {
    let d = diff(&group("1", false), &group("1", true));
    assert_eq!(seen(&d), [("order", "/PageContents[0]/Shapes[0]/Shape[0]".to_owned())], "{:?}", d.shown);
}

#[test]
fn a_change_inside_a_child_that_stands_in_another_place_is_one_order_and_one_value() {
    // The child shapes and the text swapped, and the cell of the child shape changed: the order, and the value, found where it is. (Set side by side by place, the container
    // `Shapes` was compared with the `Text` and the `Text` with the container: two elements, and the cell was never seen.)
    let d = diff(&group("1", false), &group("2", true));
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Order), 1, "{:?}", d.shown);
    let value: Vec<&str> = d.shown.iter().filter(|x| x.found == Found::Value).map(|x| x.path.as_str()).collect();
    // The container is the shape's first child on the first side, and the cell is the first of the child shape, in the first of its keyed children.
    assert_eq!(value, ["/PageContents[0]/Shapes[0]/Shape[0]/Shapes[0]/Shape[0]/Cell#0/@V"], "{:?}", d.shown);
    // The masking control: the same with the cell changed again deep inside is one more.
    let planted = diff(&group("1", false), &group("3", true));
    assert_eq!(planted.count, 2, "{:?}", planted.shown);
    let deeper = diff(
        &group("1", false).replace("</Shapes>", &format!("{}</Shapes>", shape(12, &cell("PinX", "5")))),
        &group("1", true).replace("</Shapes>", &format!("{}</Shapes>", shape(12, &cell("PinX", "6")))),
    );
    assert_eq!(deeper.count, 2, "{:?}", deeper.shown);
    assert_eq!(kinds(&deeper, Found::Order), 1, "{:?}", deeper.shown);
}

#[test]
fn sections_written_in_another_order_are_one_order_and_what_changed_in_them_is_found() {
    let section =
        |ix: u32, x: &str| format!("<Section N=\"Geometry\" IX=\"{ix}\"><Row T=\"MoveTo\" IX=\"1\"><Cell N=\"X\" V=\"{x}\"/></Row></Section>");
    let a = shape(1, &format!("{}{}", section(0, "0"), section(1, "1")));
    let b = shape(1, &format!("{}{}", section(1, "1"), section(0, "0")));
    let d = diff(&a, &b);
    assert_eq!(seen(&d), [("order", "/PageContents[0]/Shapes[0]/Shape[0]".to_owned())], "{:?}", d.shown);
    // The second section's cell changed as well.
    let c = shape(1, &format!("{}{}", section(1, "9"), section(0, "0")));
    let d = diff(&a, &c);
    assert_eq!(d.count, 2, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Order), 1, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Value), 1, "{:?}", d.shown);
}

// ---------------------------------------------------------------------------------------------------------------------- a shape put in or taken out

fn shapes(ids: &[u32], v: &str) -> String {
    ids.iter().map(|&id| shape(id, &cell("PinX", &format!("{id}{v}")))).collect()
}

#[test]
fn a_shape_put_in_first_is_one_difference_and_each_of_the_others_is_compared_with_its_own() {
    let d = diff(&shapes(&[1, 2, 3, 4], "a"), &format!("{}{}", shape(9, ""), shapes(&[1, 2, 3, 4], "b")));
    assert_eq!(d.count, 5, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Element), 1, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Value), 4, "{:?}", d.shown);
    // Taken out of the middle.
    let d = diff(&shapes(&[1, 2, 3, 4], "a"), &shapes(&[1, 3, 4], "b"));
    assert_eq!(d.count, 4, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Element), 1, "{:?}", d.shown);
    assert_eq!(kinds(&d, Found::Value), 3, "{:?}", d.shown);
}

#[test]
fn a_change_planted_deep_inside_a_shape_after_one_put_in_is_counted() {
    let deep = |v: &str| shape(4, &format!("<Section N=\"Geometry\" IX=\"0\"><Row T=\"LineTo\" IX=\"1\"><Cell N=\"X\" V=\"{v}\"/></Row></Section>"));
    let a = format!("{}{}", shapes(&[1, 2, 3], "a"), deep("1"));
    let clean = format!("{}{}{}", shape(9, ""), shapes(&[1, 2, 3], "a"), deep("1"));
    let planted = format!("{}{}{}", shape(9, ""), shapes(&[1, 2, 3], "a"), deep("2"));
    let (base, with) = (diff(&a, &clean), diff(&a, &planted));
    assert_eq!(seen(&base), [("element", "/PageContents[0]/Shapes[0]/Shape[0]".to_owned())], "{:?}", base.shown);
    assert_eq!(with.count, base.count + 1, "{:?}", with.shown);
    assert!(with.shown.iter().any(|x| x.found == Found::Value && x.path.ends_with("/Section[0]/Row[0]/Cell#0/@V")), "{:?}", with.shown);
}
