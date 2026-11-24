//! What a save costs (plan, Review Focus 1): counted in the unit each loop walks, at two sizes that differ by 4, so that a linear loop gives
//! a ratio of 4 and a quadratic one of 16 (a seconds bound is machine-specific, and the timed ratios of the first version measured fixed costs at
//! their small size: counts every one). Each test builds the crafted input a review tries: many namespace declarations,
//! scopes of their own beside them, many shapes behind the edit, many entries the manifest lacks, a meta part of many declarations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::Document;
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, Written, write_counted};

use crate::build::{DRAWING, write};
use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, PNG, drawing_of, move_by, open, open_bytes, part, set_num};

/// The save of `o`'s model with `limits`, counted.
fn save(o: &Open, limits: Limits) -> (Result<Written, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), limits, ..WriteOptions::default() };
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    (saved, work)
}

/// A drawing whose root declares `n` prefixes `svg1` to `svgN` (what a fresh prefix must step over), binds `svg` to the W3C namespace, and holds
/// a shape `depth` groups deep that is placed by its transform alone: widening it adds `svg:x` and `svg:y`, which need a prefix of ODF's own.
fn crowded(n: usize, depth: usize) -> Drawing {
    let shape = r#"<draw:rect draw:name="s" svg:width="4cm" svg:height="2cm" draw:transform="translate (5cm 10cm)"/>"#;
    let body = format!("{}{shape}{}", "<draw:g>".repeat(depth), "</draw:g>".repeat(depth));
    let mut d = Drawing::a4().page(&body);
    d.ns = d.ns.replace("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "http://www.w3.org/2000/svg");
    for k in 1..=n {
        d.ns.push_str(&format!(" xmlns:svg{k}=\"urn:example:{k}\""));
    }
    d
}

/// Widens the innermost shape of `crowded`'s page.
fn widen(doc: &mut Document, depth: usize) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let mut shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    for _ in 0..depth {
        shape = Arc::make_mut(shape.children.get_mut(0).unwrap());
    }
    set_num(shape, "Width", cm(6.0));
    set_num(shape, "LocPinX", cm(3.0));
}

#[test]
fn a_fresh_prefix_costs_one_walk_of_the_declarations_and_the_cap_holds_inside_it() {
    // The review's input: 50,000 declarations on the root and deep nesting, and a shape that needs a prefix of ODF's own. Looking each candidate up
    // by a walk of the chain was quadratic in the declarations and not charged to the step cap.
    let steps = |n: usize| {
        let mut o = open(&crowded(n, 20));
        widen(&mut o.doc, 20);
        let (saved, work) = save(&o, Limits::default());
        let saved = saved.unwrap();
        let after = part(&saved.bytes, "content.xml");
        assert!(
            after.contains(&format!(" xmlns:svg{}=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"", n + 1)),
            "the first free prefix"
        );
        (work.scope_steps, saved.bytes.len())
    };
    let (small, big) = (steps(5_000).0, steps(20_000).0);
    println!("COST fresh prefix: {small} scope steps at 5,000 declarations, {big} at 20,000");
    assert!(big < small * 6, "{small} steps at 5,000 declarations, {big} at 20,000: linear is 4, quadratic is 16");
    // A cap below what one walk costs refuses the save, and quickly: the cap is held inside the walk.
    let mut o = open(&crowded(20_000, 20));
    widen(&mut o.doc, 20);
    let started = Instant::now();
    let (refused, work) = save(&o, Limits { max_write_steps: 30_000, ..Limits::default() });
    assert_eq!(refused.err().map(|e| e.class()), Some("write_limit"));
    println!("COST fresh prefix, cap 30,000: stopped at {} steps in {:?}", work.scope_steps, started.elapsed());
    assert!(work.scope_steps < 200_000, "{} steps past a cap of 30,000", work.scope_steps);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A drawing whose root declares `declarations` prefixes besides ODF's own, and whose page holds `shapes` rectangles with no `svg:x`, each declaring
/// a prefix of its own, so that each is a scope of its own beside the root's long run of declarations. With `hostile` the root's prefixes `m0`, `m1`,
/// ... are all bound to ODF's svg namespace and each rectangle rebinds `m0` to something else; without, they are bound to nothing the writer
/// uses and each rectangle declares `q`.
fn scoped(declarations: usize, shapes: usize, hostile: bool) -> Drawing {
    let mut d = Drawing::a4();
    for k in 0..declarations {
        if hostile {
            d.ns.push_str(&format!(" xmlns:m{k}=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\""));
        } else {
            d.ns.push_str(&format!(" xmlns:j{k}=\"urn:j:{k}\""));
        }
    }
    let own = if hostile { r#"xmlns:m0="urn:junk""# } else { r#"xmlns:q="urn:q""# };
    let body: String = (0..shapes).map(|k| format!("\n<draw:rect {own} draw:name=\"s{k}\" svg:width=\"1cm\" svg:height=\"1cm\"/>")).collect();
    d.page(&body)
}

#[test]
fn prefix_lookups_cost_the_scopes_and_the_declarations_and_never_their_product() {
    // The review's input: 50,000 declarations on the root and 40,000 rectangles that each declare a prefix of their own, all moved. A
    // shape that declares a prefix was a scope of its own, and its lookups walked the whole chain behind it: scopes times declarations, 15.6 s and
    // refused at 400 million steps. Sizes of four times each: counted steps grow by four, and a product would grow by sixteen.
    for hostile in [false, true] {
        let cost = |declarations: usize, shapes: usize| {
            let mut o = open(&scoped(declarations, shapes, hostile));
            {
                let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
                for k in 0..page.shapes.len() {
                    let shape = Arc::make_mut(page.shapes.get_mut(k).unwrap());
                    let x = crate::write_core::num_of(shape, "PinX");
                    set_num(shape, "PinX", x + cm(0.1));
                }
            }
            let started = Instant::now();
            let (saved, work) = save(&o, Limits::default());
            let saved = saved.unwrap_or_else(|e| panic!("{declarations} declarations, {shapes} shapes, hostile {hostile}: {e:?}"));
            assert!(saved.edited);
            let after = part(&saved.bytes, "content.xml");
            // Each rectangle was given `svg:x` and `svg:y`, under the prefix of ODF's svg namespace in force on it: the least, which is `svg`, or
            // `m1` when `m0` is hidden (and `m1` is less than `svg`).
            let prefix = if hostile { "m1" } else { "svg" };
            let last = format!("draw:name=\"s{}\" svg:width=\"1cm\" svg:height=\"1cm\" {prefix}:x=\"", shapes - 1);
            assert!(after.contains(&last), "{prefix}: {}", &after[after.len().saturating_sub(300)..]);
            println!(
                "COST chained scopes, hostile {hostile}, {declarations} declarations and {shapes} shapes: {} scope steps in {:?}",
                work.scope_steps,
                started.elapsed()
            );
            work.scope_steps
        };
        let (small, big) = (cost(2_500, 2_000), cost(10_000, 8_000));
        near(&format!("scope steps, hostile {hostile}"), small, big, 4.0);
    }
}

/// A page of `n` shapes that each carry `draw:z-index`, in `Open` form.
fn z_page(n: usize) -> Open {
    let body: String = (0..n)
        .map(|k| format!("<draw:rect draw:name=\"s{k}\" draw:z-index=\"{k}\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>"))
        .collect();
    open(&Drawing::a4().page(&body))
}

#[test]
fn a_delete_and_a_reorder_on_a_page_that_carries_z_index_are_linear_in_its_shapes() {
    // The plan of `draw:z-index` looked each shape up in a list of the others, uncharged: quadratic in the shapes of the page. Its steps are
    // counted now, the lookups are by map, and the same save at 5,000 and at 20,000 shapes costs four times as much in every unit it walks.
    type Edit = fn(&mut Document, usize);
    let delete: Edit = |doc, _| {
        Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    };
    let reorder: Edit = |doc, n| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let last = page.shapes.remove(n - 1).unwrap();
        page.shapes.insert(0, last).unwrap();
    };
    for (what, edit) in [("delete", delete), ("reorder", reorder)] {
        let cost = |n: usize| {
            let mut o = z_page(n);
            edit(&mut o.doc, n);
            let (saved, work) = save(&o, Limits::default());
            let saved = saved.unwrap();
            // The reader reads what the model has: the order, whatever was written for it.
            let back = open_bytes(saved.bytes);
            assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), if what == "delete" { n - 1 } else { n });
            (work.order_steps, work.edits, work.shapes_compared, work.bytes_copied)
        };
        let (small, big) = (cost(5_000), cost(20_000));
        // A delete writes no attribute (the values the others had stay), a reorder gives every place the value of another.
        let edits = (what == "reorder").then_some(("edits", small.1, big.1));
        for (unit, a, b) in
            [("order steps", small.0, big.0), ("shapes compared", small.2, big.2), ("bytes copied", small.3, big.3)].into_iter().chain(edits)
        {
            assert!(a > 0, "{what}: no {unit}");
            println!("COST z-index {what}, {unit}: {a} at 5,000 shapes, {b} at 20,000");
            let ratio = b as f64 / a as f64;
            assert!((3.0..5.0).contains(&ratio), "{what}: {unit}: {a} at 5,000 shapes and {b} at 20,000, a ratio of {ratio:.2}");
        }
    }
}

/// A drawing package holding `n` pictures its manifest does not list.
fn unlisted(n: usize) -> Vec<u8> {
    let d = drawing_of(2);
    let mut files = d.files();
    let rows = [("content.xml", "text/xml"), ("styles.xml", "text/xml")];
    for k in 0..n {
        files.push((format!("Pictures/p{k}.png"), PNG.to_vec()));
    }
    files.push(("META-INF/manifest.xml".to_owned(), crate::build::manifest(DRAWING, &rows).into_bytes()));
    write(Some(DRAWING), &files)
}

#[test]
fn many_files_the_manifest_lacks_get_their_rows_with_a_few_looks_each() {
    // Counted (the timed ratio of the first version measured fixed costs at its small size and was no guard): every entry of the package
    // is looked up in the manifest's rows, the dropped entries and the names queued, once each, however many entries the package has. A look through
    // the rows for each entry would be the entries' square. **The looks are counted, and so are the comparisons of names they make**
    // (the units walked: a set that is looked up compares a name about once, one that is searched compares as many as it has), because a look that
    // searched the whole set counted as one look all the same.
    let cost = |n: usize| {
        let mut o = open_bytes(unlisted(n));
        move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        let (saved, work) = save(&o, Limits::default());
        let manifest = part(&saved.unwrap().bytes, "META-INF/manifest.xml");
        assert_eq!(manifest.matches("Pictures/p").count(), n, "a row for each, once");
        println!("COST manifest probes: {} looks, {} comparisons at {n} unlisted entries", work.manifest_probes, work.manifest_compares);
        // Looking a name up in a set costs a comparison for a name it holds and, by chance, a few for one it does not: under one for each entry.
        // A walk through the set for each look is the entries' square (about 8 x 10^8 at 40,000), and a walk through the rows for each is as much.
        assert!(work.manifest_compares <= n as u64, "{} comparisons of names for {n} entries: a look walked the set", work.manifest_compares);
        (work.manifest_probes, work.manifest_compares)
    };
    let ((small, _), (big, compared)) = (cost(10_000), cost(40_000));
    near("manifest probes", small, big, 4.0);
    assert!(compared <= 40_000, "{compared} comparisons");
    // Three looks an entry the manifest lacks, and a few for the entries it does not lack: its place among the dropped, its row, and the queue.
    assert!((3 * 40_000..=3 * 40_000 + 40).contains(&big), "{big} looks for 40,000 entries");
}

/// A `meta.xml` whose root declares `n` prefixes that all name the meta namespace (so that the one in force is chosen from among them), after
/// `x`, and that holds `n` elements `x:generator` (a name the scan must tell from `meta:generator` by what `x` is) before the real one.
fn crowded_meta(n: usize) -> String {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:x=\"urn:example:x\"",
    );
    for k in 0..n {
        text.push_str(&format!(" xmlns:m{k}=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\""));
    }
    text.push_str(" office:version=\"1.3\"><office:meta>");
    for _ in 0..n {
        text.push_str("<x:generator/>");
    }
    text.push_str("<meta:generator>Other/1</meta:generator></office:meta></office:document-meta>");
    text
}

/// A `meta.xml` whose root binds `meta`, `meta1`, ... `meta{n}` to namespaces that are not the meta namespace, and whose `office:meta` is empty: the
/// generator is added in a prefix of its own, which is the first that nothing in force is bound to.
fn taken_meta(n: usize) -> String {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:meta=\"urn:junk\"",
    );
    for k in 1..=n {
        text.push_str(&format!(" xmlns:meta{k}=\"urn:junk:{k}\""));
    }
    text.push_str(" office:version=\"1.3\"><office:meta/></office:document-meta>");
    text
}

#[test]
fn a_meta_part_of_many_declarations_and_elements_is_edited_with_a_few_looks_for_each() {
    // Counted: the prefixes in force are looked up once for each element that asks and bound and unbound once for each declaration, and the least
    // prefix of the meta namespace and the set of those in force are found by one pass over them, so the looks grow with the declarations and the
    // elements and never with their product; and the first free prefix is found by a look for each taken.
    let cost = |n: usize, meta: String, check: &dyn Fn(&str)| {
        let mut d = drawing_of(2);
        d.meta = Some(meta);
        let mut o = open(&d);
        move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        let (saved, work) = save(&o, Limits::default());
        check(&part(&saved.unwrap().bytes, "meta.xml"));
        println!("COST meta probes: {} looks, {} comparisons at {n}", work.meta_probes, work.meta_compares);
        // The comparisons are the units the looks walked: a prefix that is looked up is compared about once, and one that is
        // searched for among the n in force would be compared with each. Between one for each declaration or element and a few.
        assert!((n as u64..=4 * n as u64 + 100).contains(&work.meta_compares), "{} comparisons of prefixes at {n}", work.meta_compares);
        // Five looks for each declaration (bound, unbound, and the two passes over the prefixes in force when `office:meta` is met), or for each
        // element or each prefix tried, and a few for the rest of the part.
        assert!((5 * n as u64..=5 * n as u64 + 100).contains(&work.meta_probes), "{} looks at {n}", work.meta_probes);
        work.meta_probes
    };
    let crowded = |generator: &str| {
        assert!(
            generator.contains("<meta:generator>DiaCraft/") && generator.contains("<x:generator/>"),
            "{}",
            &generator[generator.len().saturating_sub(200)..]
        );
    };
    let (small, big) = (cost(5_000, crowded_meta(5_000), &crowded), cost(20_000, crowded_meta(20_000), &crowded));
    near("meta probes, declarations and elements", small, big, 4.0);
    let free =
        |n: usize| move |generator: &str| assert!(generator.contains(&format!("<meta{}:generator xmlns:meta{}=", n + 1, n + 1)), "{generator}");
    let (small, big) = (cost(5_000, taken_meta(5_000), &free(5_000)), cost(20_000, taken_meta(20_000), &free(20_000)));
    near("meta probes, the first free prefix", small, big, 4.0);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// The splice, counted in the units it walks (plan, `splice_cost_is_linear_in_the_part`; the review's crafted inputs).

/// A drawing of `pages` pages that hold `n` custom shapes between them, `n / pages` on each, each shape `s{k}`.
fn pages_of(n: usize, pages: usize) -> Drawing {
    let per = n / pages;
    let mut d = Drawing::a4();
    for p in 0..pages {
        let shapes: String = (0..per).map(|k| crate::write_core::custom(&format!("s{}", p * per + k + 1), &format!("{}mm", (k % 150) + 1))).collect();
        d = d.page(&shapes);
    }
    d
}

/// The cost of moving the shape `id` of page `page` of `o` by 1 cm: what the writer counted.
fn moved(o: &mut Open, page: usize, id: u32) -> WriteWork {
    move_by(&mut o.doc, page, id, 1.0, 0.0);
    let (saved, work) = save(o, Limits::default());
    let saved = saved.unwrap();
    assert!(saved.edited);
    work
}

fn near(what: &str, small: u64, big: u64, factor: f64) {
    let ratio = big as f64 / small as f64;
    println!("COST {what}: {small} at the small size, {big} at the large, ratio {ratio:.2}");
    assert!(
        ratio > factor * 0.85 && ratio < factor * 1.15,
        "{what}: {small} at the small size and {big} at the large: a ratio of {ratio:.2}, not near {factor}"
    );
}

#[test]
fn splice_cost_is_linear_in_the_part() {
    // 20,000 shapes over 50 pages, an edit at the start: bytes copied and elements walked grow as the part does, 5,000 to 20,000 shapes, 4 to 1.
    let (mut small, mut big) = (open(&pages_of(5_000, 50)), open(&pages_of(20_000, 50)));
    let (a, b) = (moved(&mut small, 0, 1), moved(&mut big, 0, 1));
    near("bytes copied", a.bytes_copied, b.bytes_copied, 4.0);
    near("bytes scanned", a.scan_bytes, b.scan_bytes, 4.0);
    near("elements walked", a.elements_walked, b.elements_walked, 4.0);
    near("shapes compared", a.shapes_compared, b.shapes_compared, 4.0);
    near("cells compared", a.cells_compared, b.cells_compared, 4.0);
    // One shape's attribute and the generator: two edits, whatever the part is.
    assert_eq!((a.edits, b.edits), (2, 2));
    // The same edit at the end of the part, in the last page's last shape, costs what it did at the start: the sweep does not depend on where.
    let (mut at_end, mut at_start) = (open(&pages_of(20_000, 50)), open(&pages_of(20_000, 50)));
    let (end, start) = (moved(&mut at_end, 49, 400), moved(&mut at_start, 0, 1));
    // The few bytes of difference are the two edits' own text (a value of another length).
    assert!(end.bytes_copied.abs_diff(start.bytes_copied) < 16, "{} and {}", end.bytes_copied, start.bytes_copied);
    // The part is scanned twice, the file's and the one made (the check of the bytes made is a scan of its tags and not a read), so the scanned
    // bytes differ by the same few.
    assert!(end.scan_bytes.abs_diff(start.scan_bytes) < 16, "{} and {}", end.scan_bytes, start.scan_bytes);
    assert_eq!(end.elements_walked, start.elements_walked);
}

#[test]
fn white_space_before_the_edit_is_copied_once_and_not_walked() {
    // The review's input: megabytes of white space between the shapes before the edit. It is bytes copied and scanned, once each, and no element.
    let build = |spaces: usize| {
        let gap = " ".repeat(spaces);
        let shapes = format!("{gap}{}{gap}{}", crate::write_core::custom("a", "1cm"), crate::write_core::custom("b", "2cm"));
        open(&Drawing::a4().page(&shapes))
    };
    let (mut small, mut big) = (build(1 << 20), build(4 << 20));
    let (a, b) = (moved(&mut small, 0, 2), moved(&mut big, 0, 2));
    assert_eq!(a.elements_walked, b.elements_walked, "white space is no element");
    let (grown, extra) = (b.scan_bytes - a.scan_bytes, 2 * 3 * (1u64 << 20));
    assert_eq!(grown, 2 * extra, "each megabyte of it is scanned twice: to splice, and to check the part made");
    // Copied twice by the part's own count: into the part by the sweep, and into the archive when the part replaces the entry.
    assert_eq!(b.bytes_copied - a.bytes_copied, 2 * extra, "and copied once into the part and once into the package");
}

#[test]
fn many_edits_in_one_save_cost_in_proportion_to_the_edits() {
    // Every shape of a page moved: the edits, the attributes written and the bytes written grow as the shapes do.
    let cost = |n: usize| {
        let mut o = open(&pages_of(n, 1));
        for id in 1..=u32::try_from(n).unwrap() {
            move_by(&mut o.doc, 0, id, 1.0, 0.0);
        }
        let (saved, work) = save(&o, Limits::default());
        let saved = saved.unwrap();
        let back = open_bytes(saved.bytes);
        assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), n);
        work
    };
    let (a, b) = (cost(2_500), cost(10_000));
    near("edits", a.edits, b.edits, 4.0);
    near("attributes written", a.attributes_written, b.attributes_written, 4.0);
    near("bytes written", a.bytes_written, b.bytes_written, 4.0);
    near("bytes copied", a.bytes_copied, b.bytes_copied, 4.0);
}

#[test]
fn nesting_is_followed_with_a_stack_and_never_recursion() {
    // A shape 90 groups deep (two levels of XML each stay under the container's 200) moved: only it changes, and the file reads back.
    let depth = 90;
    let body = format!("{}{}{}", "<draw:g>".repeat(depth), crate::write_core::custom("deep", "2cm"), "</draw:g>".repeat(depth));
    let mut o = open(&Drawing::a4().page(&body));
    let mut shape_id = 1;
    {
        // The innermost shape is the last of the pre-order.
        let mut shapes = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        for _ in 0..depth - 1 {
            shapes = shapes.children.get(0).unwrap().clone();
            shape_id += 1;
        }
        shape_id += 1;
        assert_eq!(shapes.children.get(0).map(|s| s.id), Some(shape_id));
    }
    // Moving the innermost member moves nothing else: it is in the group's frame, which the model's edit leaves.
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let mut group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    for _ in 0..depth - 1 {
        group = Arc::make_mut(group.children.get_mut(0).unwrap());
    }
    let member = Arc::make_mut(group.children.get_mut(0).unwrap());
    let x = crate::write_core::num_of(member, "PinX");
    set_num(member, "PinX", x + cm(1.0));
    let (saved, _) = save(&o, Limits::default());
    let after = part(&saved.unwrap().bytes, "content.xml");
    assert!(after.contains("draw:name=\"deep\" svg:x=\"3cm\""), "{}", after.len());
    assert_eq!(after.matches("<draw:g>").count(), depth);
}

#[test]
fn three_saves_through_the_new_indexes_cost_what_one_does() {
    let mut o = open(&pages_of(5_000, 5));
    let first = save(&o, Limits::default());
    assert!(!first.0.unwrap().edited, "nothing changed yet");
    let mut cost = Vec::new();
    let (mut bytes, mut index) = (o.bytes.clone(), o.index.clone());
    for (page, id) in [(0usize, 7u32), (2, 100), (4, 990)] {
        move_by(&mut o.doc, page, id, 0.5, 0.0);
        let mut work = WriteWork::default();
        let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &bytes, index: &index }), &options, &mut work).unwrap();
        assert!(saved.edited);
        cost.push((work.scan_bytes, work.bytes_copied, work.elements_walked, work.edits));
        (bytes, index) = (saved.bytes, saved.index);
    }
    // The three saves walk what one does, to the few bytes that an edit and the generator's string add.
    // (The first save adds a `meta.xml` the file lacks; the next two scan the one it has.)
    println!("COST three saves (scan bytes, bytes copied, elements, edits): {cost:?}");
    let (a, c) = (cost[1], cost[2]);
    assert_eq!((a.2, a.3), (c.2, c.3));
    assert!(a.0.abs_diff(c.0) < 200 && a.1.abs_diff(c.1) < 4_000, "{cost:?}");
    assert!(cost[0].0.abs_diff(c.0) < 2_000, "the first save walks what the others do: {cost:?}");
    // And the file holds all three moves.
    let back = open_bytes(bytes);
    assert_eq!(back.doc.pages.len(), 5);
}
