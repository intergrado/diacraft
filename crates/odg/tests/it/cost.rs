//! What a comparison costs, counted in the units each loop walks ([`DiffWork`]), never timed, on the shapes that make a naive comparison
//! quadratic: deep nesting, many siblings after the changed element, much white space before it, an insertion in the middle of many, a
//! reversed run of many, and a chain whose every level changed. Each bound is set at or near what the input needs, so that a walk that
//! goes back over what it has seen fails it; most cases are read at two sizes, and the work must grow as the input does and no faster.

use diacraft_odg::{DiffCounts, DiffWork, Limits, PackageDiff, diff_packages_counted};

use crate::build::{drawing, page, shape};

fn cost(a: &[u8], b: &[u8]) -> (PackageDiff, DiffWork) {
    let mut work = DiffWork::default();
    let diff = diff_packages_counted(a, b, &Limits::default(), &mut work).unwrap();
    (diff, work)
}

/// `n` custom shapes named `s1` to `sn`, the `k`-th at `k` cm.
fn shape_list(n: usize) -> Vec<String> {
    (1..=n).map(|k| shape(&format!("s{k}"), &format!("{k}cm"))).collect()
}

/// The elements of `content.xml` holding one page of `n` shapes: the root, `office:body`, `office:drawing`, the page, and three to each
/// shape.
fn elements(n: usize) -> u64 {
    4 + 3 * n as u64
}

#[test]
fn many_siblings_after_the_change_are_looked_at_once() {
    for n in [5_000usize, 20_000] {
        let a = drawing(&page("p1", &shape_list(n).concat()));
        let mut edited = shape_list(n);
        edited[0] = edited[0].replace(r#"svg:x="1cm""#, r#"svg:x="1.5cm""#);
        let b = drawing(&page("p1", &edited.concat()));
        let (diff, work) = cost(&a, &b);
        assert_eq!(diff.parts[0].counts, DiffCounts { changed: 1, ..DiffCounts::default() });
        // Each element of each side given a digest once.
        assert_eq!(work.hashed, 2 * elements(n));
        // The pairs walked are the path down to the changed shape, and the shape: the root, the body, the drawing, the page, the shape.
        assert_eq!(work.pairs, 5, "n = {n}");
        // Each pair's children looked at once on each side (one, one, one, the page's n, the shape's two): no sibling twice.
        assert_eq!(work.children, 2 * (3 + n as u64 + 2), "n = {n}");
        assert_eq!(work.ordered_steps, 0, "nothing equal in the middle to order: the change is the whole middle");
    }
}

#[test]
fn deep_nesting_is_walked_once_down_the_chain() {
    // Groups 190 deep (within the 200 the container reads), the innermost changed: a pair for each level, and each level's one child once.
    for depth in [95usize, 190] {
        let nest = |x: &str| format!("{}<draw:rect svg:x=\"{x}\"/>{}", "<draw:g>".repeat(depth), "</draw:g>".repeat(depth));
        let (a, b) = (drawing(&page("p1", &nest("1cm"))), drawing(&page("p1", &nest("2cm"))));
        let (diff, work) = cost(&a, &b);
        assert_eq!(diff.parts[0].counts, DiffCounts { changed: 1, ..DiffCounts::default() });
        let levels = depth as u64 + 5; // the root, the body, the drawing, the page, the groups and the rectangle
        assert_eq!(work.pairs, levels, "depth {depth}");
        assert_eq!(work.children, 2 * (levels - 1), "depth {depth}");
        assert_eq!(work.hashed, 2 * levels);
        assert_eq!(work.labelled, 2 * levels, "each element of each side labelled once");
        // One path, of every level: the bytes written are that path's and its label's.
        let change = &diff.parts[0].changed[0];
        assert_eq!(work.path_bytes, (change.path.len() + change.label.len()) as u64);
    }
}

#[test]
fn much_white_space_before_the_change_is_put_through_the_digest_once() {
    // 8 MiB of white space in the page before one shape, and the shape changed: the white space is hashed once on each side and compared
    // once as the page's own white space, and nothing else is done with it.
    let blank = " ".repeat(8 << 20);
    let a = drawing(&page("p1", &format!("{blank}{}", shape("s", "1cm"))));
    let b = drawing(&page("p1", &format!("{blank}{}", shape("s", "2cm"))));
    let (diff, work) = cost(&a, &b);
    assert_eq!(diff.parts[0].counts, DiffCounts { changed: 1, ..DiffCounts::default() });
    assert_eq!(work.pairs, 5);
    let text = blank.len() as u64;
    assert!(work.hashed_bytes >= 2 * text && work.hashed_bytes <= 2 * text + 4_096, "{}", work.hashed_bytes);
}

#[test]
fn an_insertion_in_the_middle_of_many_is_one_added_element() {
    for n in [5_000usize, 20_000] {
        let mut shapes = shape_list(n);
        let a = drawing(&page("p1", &shapes.concat()));
        shapes.insert(n / 2, shape("new", "0cm"));
        let b = drawing(&page("p1", &shapes.concat()));
        let (diff, work) = cost(&a, &b);
        let part = &diff.parts[0];
        assert_eq!((part.added.len(), part.counts.changed, part.counts.moved, part.counts.removed), (1, 0, 0, 0));
        assert_eq!(work.pairs, 4, "the path down to the page; the equal runs at its front and back are not walked");
        assert_eq!(work.children, 2 * 3 + (2 * n as u64 + 1), "n = {n}");
        assert_eq!(work.ordered_steps, 0);
    }
}

#[test]
fn a_reversed_run_costs_its_ordering_once() {
    // Every shape of a page in reverse order: all match by digest, the longest run that keeps its order is one shape, and the rest moved.
    // The search for the run is a patience sort: a step for each shape and for each comparison of its binary search.
    for n in [1_000usize, 4_000] {
        let shapes = shape_list(n);
        let reversed: Vec<String> = shapes.iter().rev().cloned().collect();
        let (diff, work) = cost(&drawing(&page("p1", &shapes.concat())), &drawing(&page("p1", &reversed.concat())));
        assert_eq!(diff.parts[0].counts.moved, n - 1);
        let log = u64::from(usize::BITS - n.leading_zeros());
        assert!(work.ordered_steps <= n as u64 * (log + 1), "n = {n}: {}", work.ordered_steps);
        assert!(work.ordered_steps >= n as u64, "a step for each");
        assert_eq!(work.pairs, 4);
    }
}

#[test]
fn a_chain_whose_every_level_changed_writes_each_path_once() {
    // Groups 190 deep, each with an attribute that changed: a change at every level, each with its own path. A path is built by walking up
    // from its element, so the bytes written are the sum of the paths listed, each at most the path cap; nothing is built twice.
    let depth = 190;
    let nest = |v: &str| format!("{}{}", format!("<draw:g draw:name=\"{v}\">").repeat(depth), "</draw:g>".repeat(depth));
    let (diff, work) = cost(&drawing(&page("p1", &nest("a"))), &drawing(&page("p1", &nest("b"))));
    let part = &diff.parts[0];
    assert_eq!(part.counts.changed, depth);
    let written: u64 = part.changed.iter().map(|c| (c.path.len() + c.label.len()) as u64).sum();
    assert_eq!(work.path_bytes, written);
    assert!(part.changed.iter().all(|c| c.path.len() <= diacraft_odg::limits::MAX_PATH_BYTES + 4));
    assert_eq!(work.pairs, depth as u64 + 4);
}

#[test]
fn a_comparison_parses_the_manifests_and_the_parts_that_differ_and_nothing_else() {
    // Reading the two packages parses each manifest; the comparison parses each side of the one part that differs. No part is looked
    // through for macros, and no part with the same bytes is parsed.
    let a = drawing(&page("p1", &shape_list(3).concat()));
    let mut edited = shape_list(3);
    edited[1] = edited[1].replace(r#"svg:x="2cm""#, r#"svg:x="2.5cm""#);
    let b = drawing(&page("p1", &edited.concat()));
    let events = |bytes: &[u8], name: &str| {
        let package = diacraft_odg::Package::read(bytes, &Limits::default()).unwrap();
        let mut work = diacraft_odg::Work::default();
        diacraft_container::xml::parse_counted(package.part(name).unwrap(), &diacraft_container::XmlLimits::default(), &mut work).unwrap();
        work.xml_events
    };
    let want = events(&a, "META-INF/manifest.xml") + events(&b, "META-INF/manifest.xml") + events(&a, "content.xml") + events(&b, "content.xml");
    let (diff, work) = cost(&a, &b);
    assert_eq!(diff.parts.len(), 1);
    assert_eq!(work.xml.xml_events, want);
}

// ---- The copy's walk of the local headers' extra fields and the reason words of what a copy leaves out.

/// A package of `mimetype`, `content.xml`, `styles.xml`, the manifest and `folders` folders whose local headers each hold `records` empty
/// extra records (the review's shape: 2,000 folders of 16,383 made 131 MB).
fn empty_records(folders: usize, records: usize) -> Vec<u8> {
    use crate::build::{DRAWING, files, manifest};
    use crate::zipbuild::{Item, build};
    let area: Vec<u8> = (0..records).flat_map(|_| [0x99u8, 0x99, 0, 0]).collect();
    let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
    for (name, bytes) in files("") {
        items.push(Item::deflated(&name, &bytes));
    }
    for k in 0..folders {
        items.push(Item::stored(&format!("d{k}/"), b"").with_local_extra(&area));
    }
    let rows = [("content.xml", "text/xml"), ("styles.xml", "text/xml"), ("meta.xml", "text/xml"), ("settings.xml", "text/xml")];
    items.push(Item::deflated("META-INF/manifest.xml", manifest(DRAWING, &rows).as_bytes()));
    build(&items)
}

#[test]
fn the_copy_and_its_account_count_the_records_of_the_extra_fields_they_walk() {
    let walked = |folders: usize| {
        let bytes = empty_records(folders, 500);
        let (mut copy, mut account) = (diacraft_container::Work::default(), diacraft_container::Work::default());
        diacraft_odg::copy_unchanged_counted(&bytes, &Limits::default(), &mut copy).unwrap();
        diacraft_odg::copy_account_counted(&bytes, &Limits::default(), &mut account).unwrap();
        (copy.local_extra_records, account.local_extra_records)
    };
    let (small, big) = (walked(100), walked(400));
    assert_eq!(small, (50_000, 50_000), "each record of each folder once by the copy and once by the account");
    assert_eq!(big, (200_000, 200_000));
}

#[test]
fn what_a_copy_leaves_out_has_six_reasons_each_a_fixed_word() {
    let words: Vec<&str> = diacraft_odg::Dropped::default().reasons().iter().map(|(word, _)| *word).collect();
    assert_eq!(words, ["unannounced", "descriptor_form", "directory_tail", "zip64_end_records", "archive_comment", "after_end_record"]);
}
