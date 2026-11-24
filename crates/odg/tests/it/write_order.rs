//! A new shape and a reorder in one save (`draw:z-index`, R9 reorder and add a basic shape):
//! both are written, the new shape at its place in the model's order and each shape that moved at its own. The model saved reads back in the
//! order the model has, the second and the third save are no edit, and only the part the edit owns changes.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, Shape};
use diacraft_odg::{OriginalPackage, WriteError, WriteOptions, WriteWork, Written, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Case, Open, Saver, compare, content_changes, custom, echo_control, open, open_bytes, part, real};
use crate::write_new::rectangle;

/// The names of the page's shapes in the order the model has them.
fn names(doc: &Document) -> Vec<String> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| s.name.as_deref().unwrap().to_owned()).collect()
}

/// A page of `n` custom shapes `s1` to `sn` in one row, none carrying `draw:z-index`.
fn plain(n: usize) -> Drawing {
    Drawing::a4().page(&(1..=n).map(|k| custom(&format!("s{k}"), &format!("{k}cm"))).collect::<String>())
}

/// A page of shapes named `names`, each carrying the `draw:z-index` its place in `z` says (`None`: no attribute), in document order.
fn zed(list: &[(&str, Option<i64>)]) -> Drawing {
    let body: String = list
        .iter()
        .enumerate()
        .map(|(k, (name, z))| {
            let z = z.map(|z| format!(" draw:z-index=\"{z}\"")).unwrap_or_default();
            format!(
                "<draw:custom-shape draw:name=\"{name}\"{z} svg:x=\"{}cm\" svg:y=\"3cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>",
                k + 1
            )
        })
        .collect();
    Drawing::a4().page(&body)
}

/// The model of `o` with its shapes in the order `order` says: a name the page has is that shape, any other a new rectangle of that name.
fn arranged(o: &Open, order: &[&str]) -> Document {
    let mut doc = o.doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let old: Vec<Arc<Shape>> = page.shapes.iter().cloned().collect();
    let mut next = o.index.next_id(0).unwrap();
    let mut list: Vec<Arc<Shape>> = Vec::new();
    for (k, name) in order.iter().enumerate() {
        match old.iter().find(|s| s.name.as_deref() == Some(*name)) {
            Some(s) => list.push(Arc::clone(s)),
            None => {
                list.push(Arc::new(rectangle(next, name, (2.0 + k as f64, 20.0, 1.0, 1.0))));
                next += 1;
            }
        }
    }
    page.shapes = diacraft_doc::ShapeList::from_vec(list);
    doc
}

/// Saves `doc` three times: the first is an edit, the others are no edit and give its bytes.
#[track_caller]
fn saved_thrice(save: &Saver, doc: &Document, o: &Open) -> Written {
    let first = save(doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(first.edited);
    let mut last = first.clone();
    for n in 2..=3 {
        let again = save(doc, &last.bytes, &last.index).unwrap_or_else(|e| panic!("save {n} was refused: {} ({e})", e.class()));
        assert!(!again.edited, "save {n} of the model that was saved is no edit");
        assert_eq!(again.bytes, last.bytes, "save {n} gives the bytes of the save before");
        last = again;
    }
    first
}

/// The order the model has is the order the file reads back in, with every new shape in it once and each with the element the index says.
#[track_caller]
fn check(save: &Saver, o: &Open, order: &[&str]) -> Written {
    let doc = arranged(o, order);
    assert_eq!(names(&doc), order);
    let written = saved_thrice(save, &doc, o);
    let back = open_bytes(written.bytes.clone());
    assert_eq!(names(&back.doc), order, "the file reads back in the model's order: {}", part(&written.bytes, "content.xml"));
    // Only `content.xml` is the edit's (the thumbnail, the generator and the manifest are the save's own).
    let diff = compare(&o.bytes, &written.bytes);
    assert!(diff.parts.iter().all(|p| ["content.xml", "meta.xml", "META-INF/manifest.xml"].contains(&p.part.as_str())), "{diff:?}");
    // Every shape the model has has an element, and the elements are distinct.
    let page = doc.pages.get(0).unwrap();
    let ords: HashSet<u32> = page.shapes.iter().filter_map(|s| written.index.ordinal(0, s.id)).collect();
    assert_eq!(ords.len(), order.len());
    written
}

const TWO_SAVES: &str = "the model saved again is no edit";

/// A reorder and a new shape on a page that carries no `draw:z-index`: the plan puts the moved shapes and the new one in the model's order, by moving
/// the elements that are out of place and putting the new one among them.
fn plain_cases(save: &Saver) {
    let o = open(&plain(4));
    // The first to the front of the others, a new shape last, first, after the one that stays and before the one that moves, and after the one that moves.
    for order in [
        vec!["s2", "s3", "s4", "s1", "R"],
        vec!["R", "s2", "s3", "s4", "s1"],
        vec!["s2", "R", "s3", "s4", "s1"],
        vec!["s2", "s3", "s4", "R", "s1"],
        vec!["s2", "s3", "s4", "s1", "R"],
        // Every shape reversed, the new shape in the middle of the movers: the movers and the new shape go to one place in the model's order.
        vec!["s4", "s3", "R", "s2", "s1"],
        // Two new shapes, one after the other and one apart.
        vec!["s3", "R", "Q", "s1", "s2", "s4", "T"],
        vec!["Q", "s4", "s1", "R", "s2", "s3"],
    ] {
        check(save, &o, &order);
    }
    // One element only differs between the file and what a reorder alone writes: the new shape is the added one.
    let written = check(save, &o, &["s2", "s3", "s4", "s1", "R"]);
    let diff = compare(&o.bytes, &written.bytes);
    let (_, added, removed) = content_changes(&diff);
    assert_eq!(removed, 0, "{TWO_SAVES}: {diff:?}");
    assert!(added >= 3, "the new shape and its two styles: {diff:?}");
}

/// The same on a page whose shapes carry `draw:z-index`: no element moves, the shapes that moved are written with the values their places have, and the
/// new shape with the value of the shape before it, the element put right after that shape's.
fn z_cases(save: &Saver) {
    let o = open(&zed(&[("a", Some(0)), ("b", Some(1)), ("c", Some(2)), ("d", Some(3))]));
    for order in [
        vec!["a", "b", "c", "d", "R"],
        vec!["R", "a", "b", "c", "d"],
        vec!["a", "R", "b", "c", "d"],
        vec!["b", "c", "d", "a", "R"],
        vec!["b", "R", "c", "d", "a"],
        vec!["d", "c", "R", "b", "a"],
        vec!["b", "R", "Q", "a", "d", "T", "c"],
    ] {
        let written = check(save, &o, &order);
        let after = part(&written.bytes, "content.xml");
        assert!(after.contains("draw:name=\"R\"") && after.contains("draw:z-index"), "{after}");
    }
    // The new shape carries the attribute, and the shapes of the file keep their elements where they were.
    let written = check(save, &o, &["b", "R", "c", "d", "a"]);
    let after = part(&written.bytes, "content.xml");
    let at = |n: &str| after.find(&format!("draw:name=\"{n}\"")).unwrap();
    assert!(at("a") < at("b") && at("b") < at("c") && at("c") < at("d"), "no element of the file moved: {after}");
    // Shapes that carry the attribute in a gap are not renumbered: the new shape takes the value of the one before it.
    let o = open(&zed(&[("a", Some(10)), ("b", Some(20)), ("c", Some(30))]));
    let written = check(save, &o, &["a", "b", "R", "c"]);
    let after = part(&written.bytes, "content.xml");
    let new = after.split("draw:name=\"R\"").nth(1).unwrap();
    assert!(new.starts_with(' ') || new.starts_with('>'), "{after}");
    assert!(after.contains("draw:z-index=\"20\""), "{after}");
    // A shape of the file with no attribute is read at its place, which a new element changes: it is given the value the order needs.
    let o = open(&zed(&[("a", Some(0)), ("b", None), ("c", Some(5))]));
    check(save, &o, &["a", "R", "b", "c"]);
    check(save, &o, &["R", "a", "b", "c"]);
    check(save, &o, &["b", "a", "R", "c"]);
    // The value a shape with no attribute is read at moves with a new element put before it, and past the next shape's own value: it is given the value its
    // place had, and only then.
    let o = open(&zed(&[("a", Some(0)), ("b", None), ("c", Some(1))]));
    for order in [vec!["R", "a", "b", "c"], vec!["a", "R", "b", "c"], vec!["a", "b", "R", "c"], vec!["a", "b", "c", "R"]] {
        let written = check(save, &o, &order);
        let after = part(&written.bytes, "content.xml");
        let at = |n: &str| after.find(&format!("draw:name=\"{n}\"")).unwrap();
        let tag = &after[at("b")..];
        let tag = &tag[..tag.find('>').unwrap()];
        let place = |n: &str| order.iter().position(|x| *x == n).unwrap();
        assert_eq!(
            tag.contains("draw:z-index"),
            place("R") < place("b"),
            "{order:?}: b is given its value when a new shape comes before it, and not else: {after}"
        );
    }
}

/// A delete and a new shape and a reorder in one save.
fn delete_case(save: &Saver) {
    let o = open(&plain(4));
    let doc = arranged(&o, &["s3", "R", "s1", "s4"]);
    let written = saved_thrice(save, &doc, &o);
    let back = open_bytes(written.bytes);
    assert_eq!(names(&back.doc), ["s3", "R", "s1", "s4"], "s2 is deleted");
    let o = open(&zed(&[("a", Some(0)), ("b", Some(1)), ("c", Some(2)), ("d", Some(3))]));
    let doc = arranged(&o, &["c", "R", "a", "d"]);
    let written = saved_thrice(save, &doc, &o);
    let back = open_bytes(written.bytes);
    assert_eq!(names(&back.doc), ["c", "R", "a", "d"], "b is deleted");
}

/// A second edit after the save: the new shape is moved again, in the same page that has been saved with a reorder.
fn chained_case(save: &Saver) {
    let o = open(&plain(3));
    let doc = arranged(&o, &["s3", "R", "s1", "s2"]);
    let first = saved_thrice(save, &doc, &o);
    // The model of the first save is the one the next edit starts from; it is reordered again and a second new shape is put in.
    let mut next = doc.clone();
    {
        let page = Arc::make_mut(next.pages.get_mut(0).unwrap());
        let moved = page.shapes.remove(0).unwrap();
        page.shapes.push(moved);
        let id = page.shapes.iter().map(|s| s.id).max().unwrap() + 1;
        page.shapes.insert(1, Arc::new(rectangle(id, "Q", (5.0, 20.0, 1.0, 1.0)))).unwrap();
    }
    let again = save(&next, &first.bytes, &first.index).unwrap_or_else(|e| panic!("the second edit was refused: {} ({e})", e.class()));
    assert!(again.edited);
    let back = open_bytes(again.bytes);
    assert_eq!(names(&back.doc), names(&next));
}

const CASES: [Case; 4] = [
    ("a_new_shape_and_a_reorder_of_a_page_without_z_index", plain_cases),
    ("a_new_shape_and_a_reorder_of_a_page_with_z_index", z_cases),
    ("a_delete_a_new_shape_and_a_reorder", delete_case),
    ("a_second_edit_after_a_reorder_with_a_new_shape", chained_case),
];

#[test]
fn a_new_shape_and_a_reorder_of_a_page_without_z_index_are_written_in_the_models_order() {
    plain_cases(&real);
}

#[test]
fn a_new_shape_and_a_reorder_of_a_page_with_z_index_are_written_in_the_models_order() {
    z_cases(&real);
}

#[test]
fn a_delete_a_new_shape_and_a_reorder_are_written_in_one_save() {
    delete_case(&real);
}

#[test]
fn a_second_edit_after_a_reorder_with_a_new_shape_is_written() {
    chained_case(&real);
}

#[test]
fn echo_control_fails_every_order_case() {
    echo_control(&CASES);
}

/// A new shape alone on a page that carries `draw:z-index` is written with the attribute (the writer used to refuse it; ownership ask for it).
#[test]
fn a_new_shape_on_a_page_that_carries_z_index_is_written_with_the_attribute_of_the_shape_before_it() {
    let o = open(&zed(&[("a", Some(3)), ("b", Some(7))]));
    let doc = arranged(&o, &["a", "b", "R"]);
    let written = real(&doc, &o.bytes, &o.index).unwrap();
    let after = part(&written.bytes, "content.xml");
    let new = after.split("<draw:custom-shape draw:style-name=").nth(1).unwrap();
    assert!(new.contains("draw:z-index=\"7\""), "the value of `b`, which it follows: {after}");
    let back = open_bytes(written.bytes);
    assert_eq!(names(&back.doc), ["a", "b", "R"]);
    // First: the least value, before everything.
    let doc = arranged(&o, &["R", "a", "b"]);
    let written = real(&doc, &o.bytes, &o.index).unwrap();
    assert_eq!(names(&open_bytes(written.bytes).doc), ["R", "a", "b"]);
}

/// Values that repeat cannot always say the order the model has: the page is refused in words, and nothing is written.
#[test]
fn a_page_whose_z_index_values_repeat_is_refused_in_words_when_the_new_shape_cannot_be_put_in_order() {
    // `a`, `b`, `c` all carry 5, so their order is the document's. The model puts `c` first and adds a shape: the values `5` cannot say that.
    let o = open(&zed(&[("a", Some(5)), ("b", Some(5)), ("c", Some(5))]));
    let doc = arranged(&o, &["c", "a", "R", "b"]);
    let refused = real(&doc, &o.bytes, &o.index);
    assert!(matches!(refused, Err(WriteError::Unsupported(what)) if what.contains("z-index")), "{refused:?}");
}

/// The counts of a save of a page of `n` shapes whose first is moved to the end and a new shape put before it, on a page with no `draw:z-index` or with one
/// on every shape.
fn counted_reorder(n: usize, z: bool) -> WriteWork {
    let names: Vec<String> = (1..=n).map(|k| format!("s{k}")).collect();
    let drawing = if z {
        let list: Vec<(&str, Option<i64>)> = names.iter().enumerate().map(|(k, name)| (name.as_str(), Some(k as i64))).collect();
        zed(&list)
    } else {
        plain(n)
    };
    let o = open(&drawing);
    let mut order: Vec<&str> = names.iter().skip(1).map(String::as_str).collect();
    order.push("R");
    order.push("s1");
    let doc = arranged(&o, &order);
    let mut work = WriteWork::default();
    let saved = write_counted(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(saved.edited);
    work
}

/// The plan of a reorder with a new shape pays for the page's shapes in proportion to them, and pays for each at least once for each list the plan walks (the
/// shapes as they were and the model's order): four times the shapes cost about four times the steps, and never fewer steps than the lists are long.
#[test]
fn a_reorder_with_a_new_shape_costs_steps_that_follow_the_shapes_of_the_page() {
    for z in [false, true] {
        let (small, large) = (counted_reorder(100, z), counted_reorder(400, z));
        assert!(
            large.order_steps <= small.order_steps * 9 / 2 + 2_000,
            "z={z}: {} then {} for 4 times the shapes",
            small.order_steps,
            large.order_steps
        );
        // The plan walks the shapes of the file and the model's order, twice over on a page that carries `draw:z-index` (the plan of the values, and the check of
        // the order the reader will read).
        let walks = if z { 4 } else { 2 };
        assert!(
            large.order_steps - small.order_steps >= 300 * walks,
            "z={z}: the shapes of the page are walked {walks} times at least: {} then {}",
            small.order_steps,
            large.order_steps
        );
    }
}
