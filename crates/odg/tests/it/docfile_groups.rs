//! A group's own data in DiaCraft's document file (the proposal of Spec question 1), read before the pages and put on the group as it
//! is built. Every input is built by hand, as a crafted file is.

use diacraft_doc::SectionKey;
use diacraft_odg::{Limits, read};

use crate::docfile_cost::{file_of, package_with};
use crate::drawing::{Drawing, cell, num, shape};

const PATH: &str = "DiaCraft/document.xml";

/// A rectangle 2 centimetres by 1 at (`x`, 1) centimetres, with no style.
fn rect(x: u32) -> String {
    format!(r#"<draw:rect svg:x="{x}cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#)
}

/// A group called `name` (or none) holding `inner`.
fn group(name: Option<&str>, inner: &str) -> String {
    let name = name.map(|n| format!(r#" draw:name="{n}""#)).unwrap_or_default();
    format!("<draw:g{name}>{inner}</draw:g>")
}

/// The entry for the group `name` of the page `page1`, holding `body` and `id` when given: with the `nth` of the group among those of its name (the writer's entry for
/// a group with no key of its own, which has one even for the first), or with none (`nth` 0: an entry keyed by the name, which the writer saw unique on the page).
fn entry(name: &str, nth: u32, id: Option<u32>, body: &str) -> String {
    let nth = if nth > 0 { format!(r#" nth="{nth}""#) } else { String::new() };
    let id = id.map(|i| format!(r#" id="{i}""#)).unwrap_or_default();
    format!(r#"<diacraft:group page="page1" name="{name}"{nth}{id}>{body}</diacraft:group>"#)
}

/// A sheet child holding one cell `Cost` of the value `v`.
fn cost(v: &str) -> String {
    format!(r#"<diacraft:sheet><diacraft:Cell N="Cost" V="{v}"/></diacraft:sheet>"#)
}

/// The drawing of one page holding `shapes`, with the document file `file`.
fn read_with(shapes: &str, file: &str) -> (diacraft_doc::Document, diacraft_odg::ReadReport) {
    let mut drawing = Drawing::a4().page(shapes);
    drawing.files.push((PATH.to_owned(), file.as_bytes().to_vec()));
    drawing.read()
}

/// The cells, the `ID` and the removals an entry gives a group are put on it when it is built: the cost it names, its `ID`, and the `Connection` rows the entry says
/// are gone (a group read from a file has the four side-middle rows, which the entry takes out).
#[test]
fn an_entry_gives_a_group_its_id_its_cells_and_its_removals() {
    let file = file_of(&entry("Box", 1, Some(7), &cost("42")).replace("<diacraft:group ", r#"<diacraft:group gone="s.Connection._" "#));
    let (doc, report) = read_with(&group(Some("Box"), &rect(1)), &file);
    let g = shape(&doc, 0, 0);
    assert_eq!(g.id, 7, "the group has the `ID` of its entry");
    assert_eq!(num(cell(g, "Cost")), 42.0);
    assert!(g.sheet.section(&SectionKey::new("Connection", None)).is_none(), "and what the entry says is gone is gone");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // With no entry the group has the rows the reader gives every shape.
    let (plain, _) = read_with(&group(Some("Box"), &rect(1)), &file_of(""));
    assert!(shape(&plain, 0, 0).sheet.section(&SectionKey::new("Connection", None)).is_some());
    assert!(cell(shape(&plain, 0, 0), "Cost").is_none());
}

/// The links to a master an entry holds are put on the group: both, one of them, and none when the entry has none or the number is not one.
#[test]
fn an_entry_gives_a_group_its_links_to_a_master() {
    let with = |attrs: &str| {
        let file = file_of(&entry("Box", 1, None, "").replace("<diacraft:group ", &format!("<diacraft:group {attrs} ")));
        let (doc, report) = read_with(&group(Some("Box"), &rect(1)), &file);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let g = shape(&doc, 0, 0);
        (g.master, g.master_shape)
    };
    assert_eq!(with(r#"master="4" master-shape="9""#), (Some(4), Some(9)));
    assert_eq!(with(r#"master="4""#), (Some(4), None));
    assert_eq!(with(r#"master-shape="9""#), (None, Some(9)));
    assert_eq!(with(""), (None, None));
    assert_eq!(with(r#"master="x" master-shape="-1""#), (None, None), "what is not a number is none");
}

/// A group's `ID` from the file is taken before the numbering: the shape after it is numbered round it, and the page's next `ID` is above it.
#[test]
fn the_ids_the_entries_give_are_taken_before_the_numbering_and_count_in_the_pages_high_water_mark() {
    // The group would be numbered 1; its entry says 2, so the member (numbered 2 by its place) takes 1 and the shape after the group 3.
    let file = file_of(&entry("G", 1, Some(2), ""));
    let (doc, report) = read_with(&format!("{}{}", group(Some("G"), &rect(1)), rect(5)), &file);
    let page = doc.pages.get(0).unwrap();
    let ids: Vec<u32> =
        std::iter::once(page.shapes.get(0).unwrap()).flat_map(|g| std::iter::once(g.id).chain(g.children.iter().map(|c| c.id))).collect();
    assert_eq!(ids, [2, 1], "the group keeps 2 and its member is numbered round it");
    assert_eq!(page.shapes.get(1).unwrap().id, 3);
    assert!(report.index.next_id(0).unwrap() > 3, "the next `ID` is above every one given: {:?}", report.index.next_id(0));
}

/// An `ID` the file gives a group that one of the group's members' own style also carries belongs to the group (the group is first in the pre-order): the member reads
/// as a copy, from its markup alone, and the page says so.
#[test]
fn an_id_that_a_group_entry_and_a_members_style_both_carry_is_the_groups_and_the_member_is_a_copy() {
    let style = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" diacraft:version="1" diacraft:id="5"/></style:style>"#;
    let member = r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#;
    let mut drawing = Drawing::a4().auto(style).page(&group(Some("G"), member));
    drawing.files.push((PATH.to_owned(), file_of(&entry("G", 1, Some(5), "")).into_bytes()));
    let (doc, report) = drawing.read();
    let g = shape(&doc, 0, 0);
    assert_eq!(g.id, 5);
    assert_ne!(g.children.get(0).unwrap().id, 5, "the member is no longer 5");
    assert_eq!(report.warning_counts.get("diacraft_id_repeated"), Some(&1), "{:?}", report.warnings);
    assert!(report.index.page(0).unwrap().copies().contains(&g.children.get(0).unwrap().id));
}

/// Two groups of one name are told apart by their number among those of the name, in the pre-order of the page (a group inside a group counts); the first entry for
/// one key stands, and an entry for a group or a page the drawing does not have is a warning and nothing more.
#[test]
fn groups_of_one_name_are_told_apart_by_their_number_and_a_second_entry_or_an_entry_for_nothing_is_warned_of() {
    let shapes = group(Some("T"), &format!("{}{}", rect(1), group(Some("T"), &rect(5)))) + &group(Some("T"), &rect(9));
    let file = file_of(&format!(
        "{}{}{}{}{}",
        entry("T", 1, None, &cost("1")),
        entry("T", 2, None, &cost("2")),
        entry("T", 3, None, &cost("3")),
        entry("T", 2, None, &cost("99")),
        r#"<diacraft:group page="elsewhere" name="T"/>"#
    ));
    let (doc, report) = read_with(&shapes, &file);
    let outer = shape(&doc, 0, 0);
    let inner = outer.children.iter().find(|c| !c.children.is_empty()).unwrap();
    let last = shape(&doc, 0, 1);
    assert_eq!((num(cell(outer, "Cost")), num(cell(inner, "Cost")), num(cell(last, "Cost"))), (1.0, 2.0, 3.0));
    assert_eq!(report.warning_counts.get("diacraft_group_repeated"), Some(&1), "{:?}", report.warnings);
    assert_eq!(report.warning_counts.get("diacraft_group_unknown"), Some(&1), "{:?}", report.warnings);
}

/// A group nested eight deep is found by the number it has through the nesting.
#[test]
fn a_group_eight_deep_is_found_through_the_nesting() {
    let mut inner = rect(1);
    for _ in 0..8 {
        inner = group(Some("D"), &inner);
    }
    let file = file_of(&entry("D", 8, None, &cost("8")));
    let (doc, _) = read_with(&inner, &file);
    let mut g = shape(&doc, 0, 0);
    let mut depth = 1;
    while let Some(next) = g.children.iter().find(|c| !c.children.is_empty()) {
        g = next;
        depth += 1;
    }
    assert_eq!(depth, 8);
    assert_eq!(num(cell(g, "Cost")), 8.0, "the innermost is the eighth of its name");
}

// ------------------------------------------------------------------------------------------------------------------ cost

/// The items of a read of a page of `groups` groups of distinct names, each with an entry (`entries` is whether the file has them).
fn items_of(groups: usize, entries: bool) -> u64 {
    let shapes: String = (0..groups).map(|k| group(Some(&format!("G{k}")), &rect(1))).collect();
    let body: String = if entries { (0..groups).map(|k| entry(&format!("G{k}"), 1, None, &cost("1"))).collect() } else { String::new() };
    let mut drawing = Drawing::a4().page(&shapes);
    drawing.files.push((PATH.to_owned(), file_of(&body).into_bytes()));
    match read(&drawing.bytes(), &Limits::default()) {
        Ok((_, report)) => report.work.items,
        Err(e) => panic!("{groups} groups do not read: {e}"),
    }
}

/// Ten thousand groups, each found by its name, cost items in proportion to their number: four times the groups, about four times the items (the entries' own).
#[test]
fn the_groups_of_a_page_are_found_in_work_that_follows_their_number_and_the_entries_are_charged() {
    let (small, big) = (items_of(2_500, true), items_of(10_000, true));
    assert!(big <= small * 4 + 400 && big >= small * 3, "{small} items for 2,500 groups and {big} for 10,000");
    // The entries are charged: the same groups with none cost less by at least an item for each.
    let none = items_of(2_500, false);
    assert!(small >= none + 2_500, "the entries of 2,500 groups cost {} items more than none", small - none);
}

/// The review: an entry that no group of an existing page takes (the group was renamed in another program) was dropped with no word, and the group
/// lost its `ID`, its data and its link to a master. It is warned of, once, and a group that has its entry reads as before and is not warned of.
#[test]
fn an_entry_no_group_of_its_page_takes_is_warned_of_and_one_that_is_taken_is_not() {
    let file = file_of(&format!("{}{}", entry("Box", 1, Some(7), &cost("4")), entry("Renamed", 1, Some(8), &cost("5"))));
    let (doc, report) = read_with(&group(Some("Box"), &rect(1)), &file);
    assert_eq!(shape(&doc, 0, 0).id, 7, "the group that has its entry reads it");
    assert_eq!(report.warning_counts.get("diacraft_group_unclaimed"), Some(&1), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("renamed or taken away in another program")), "{:?}", report.warnings);
    // Every entry taken: nothing is said.
    let (_, quiet) = read_with(&group(Some("Box"), &rect(1)), &file_of(&entry("Box", 1, Some(7), &cost("4"))));
    assert_eq!(quiet.warning_counts.get("diacraft_group_unclaimed"), None, "{:?}", quiet.warnings);
    // The second group of a name that has one only: its entry (the second of the name) is left over.
    let (_, second) = read_with(&group(Some("Box"), &rect(1)), &file_of(&entry("Box", 2, None, &cost("4"))));
    assert_eq!(second.warning_counts.get("diacraft_group_unclaimed"), Some(&1), "{:?}", second.warnings);
}

/// A crafted file of ten thousand entries for a page's groups, none of which the page has, is read in work that follows the entries and ends as a warning.
#[test]
fn ten_thousand_entries_that_name_no_group_cost_in_proportion_to_their_number() {
    let entries = |n: usize| -> u64 {
        let body: String = (0..n).map(|k| entry(&format!("Nobody{k}"), 1, None, &cost("1"))).collect();
        let (_, report) = read(&package_with(&file_of(&body)), &Limits::default()).unwrap();
        assert_eq!(report.warning_counts.get("diacraft_group_unknown"), None, "the entries are for a page that exists: no warning of a page");
        assert_eq!(report.warning_counts.get("diacraft_group_unclaimed"), Some(&1), "but no group takes them: one warning, whatever their number");
        report.work.items
    };
    let (small, big) = (entries(2_500), entries(10_000));
    assert!(big <= small * 4 + 400 && big >= small * 3, "{small} items for 2,500 entries and {big} for 10,000");
}

/// What one `group` entry costs the reader, exactly (the numbers pin the charges: a charge taken out, or counted another way, changes them): its element and its
/// attributes are charged as the entry is read and again as the file's children are walked, and one more item is the entry's own. A name of one byte costs no more.
#[test]
fn an_entry_costs_the_items_of_its_element_and_attributes_twice_and_one_more() {
    let per_entry = |attrs: &str| -> u64 {
        let body: String = (0..1_000).map(|_| format!(r#"<diacraft:group page="page1" name="N"{attrs}/>"#)).collect();
        let with = read(&package_with(&file_of(&body)), &Limits::default()).unwrap().1.work.items;
        let none = read(&package_with(&file_of("")), &Limits::default()).unwrap().1.work.items;
        (with - none) / 1_000
    };
    assert_eq!(per_entry(""), 7, "an element and two attributes: three items twice, and the entry's own");
    assert_eq!(per_entry(r#" nth="2""#), 9, "one more attribute is one more item at each of the two charges");
    assert_eq!(per_entry(r#" nth="2" id="7""#), 11);
}

/// The survivors of the mutation run: the number of an entry is at least 1 (`nth="0"` is the first, and a page's `page-nth="0"` too), so an entry with none of
/// the sort reads as the first and is not left over; and the overlay of an entry that says a section gone is charged by the sections the group holds, once (an entry that
/// says a cell gone does not), counted.
#[test]
fn an_entry_numbered_zero_is_the_first_and_an_overlay_that_takes_a_section_out_is_charged() {
    let file =
        file_of(r#"<diacraft:group page="page1" name="T" nth="0"><diacraft:sheet><diacraft:Cell N="Cost" V="4"/></diacraft:sheet></diacraft:group>"#);
    let (doc, report) = read_with(&group(Some("T"), &rect(1)), &file);
    assert_eq!(num(cell(shape(&doc, 0, 0), "Cost")), 4.0, "nth 0 is the first");
    assert_eq!(report.warning_counts.get("diacraft_group_unclaimed"), None, "{:?}", report.warnings);
    let paged = file_of(
        r#"<diacraft:group page="page1" page-nth="0" name="T"><diacraft:sheet><diacraft:Cell N="Cost" V="5"/></diacraft:sheet></diacraft:group>"#,
    );
    let (doc, report) = read_with(&group(Some("T"), &rect(1)), &paged);
    assert_eq!(num(cell(shape(&doc, 0, 0), "Cost")), 5.0, "page-nth 0 is the first page of its name");
    assert_eq!(report.warning_counts.get("diacraft_group_unknown"), None, "{:?}", report.warnings);
    // The overlay: a section said gone is one pass over the group's sections; a cell said gone is none.
    let per_group = |gone: &str| -> u64 {
        let shapes: String = (0..1_000).map(|k| group(Some(&format!("G{k}")), &rect(1))).collect();
        let body: String = (0..1_000).map(|k| format!(r#"<diacraft:group page="page1" name="G{k}" gone="{gone}"/>"#)).collect();
        let mut drawing = Drawing::a4().page(&shapes);
        drawing.files.push((PATH.to_owned(), file_of(&body).into_bytes()));
        read(&drawing.bytes(), &Limits::default()).unwrap().1.work.items
    };
    let (section, cell_gone) = (per_group("s.Connection._"), per_group("c.Cost"));
    let more = section - cell_gone;
    assert!(
        (1_000..2_000).contains(&more),
        "at least one item for each group, for the one section it holds (the pass that takes a section out), and no more than one again: {more}"
    );
}

/// The review, reader side: an entry with no `nth` is keyed by the name the writer saw unique on the page, so it binds to a group only while exactly
/// one group of the page has the name. One group of the name: it binds (the `ID`, the cells), and the page says nothing.
#[test]
fn an_entry_with_no_nth_binds_to_the_one_group_that_has_its_name() {
    let file = file_of(&entry("Box", 0, Some(7), &cost("4")));
    let shapes = group(Some("Box"), &rect(1)) + &group(Some("Other"), &rect(5));
    let (doc, report) = read_with(&shapes, &file);
    assert_eq!((shape(&doc, 0, 0).id, num(cell(shape(&doc, 0, 0), "Cost"))), (7, 4.0));
    assert!(cell(shape(&doc, 0, 1), "Cost").is_none(), "the group of another name takes nothing");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// Another program copied the group of the entry's name, or gave another group the name: the name is not unique now, so the entry binds to none of them, whichever
/// is first. It is warned of, once, and no more than that: it is no unclaimed entry too. Its `ID` stays its own, so no group is numbered it (a formula that names the
/// `ID` reads no group and never the wrong one).
#[test]
fn an_entry_with_no_nth_binds_to_none_of_the_groups_once_the_name_is_shared_and_says_so() {
    let file = file_of(&entry("Box", 0, Some(1), &cost("4")));
    for shapes in [
        group(Some("Box"), &rect(1)) + &group(Some("Box"), &rect(5)),
        group(Some("Box"), &rect(1)) + &group(Some("Other"), &rect(5)) + &group(Some("Box"), &rect(9)),
        // The second is inside the first: the groups of the page are counted through the nesting.
        group(Some("Box"), &format!("{}{}", rect(1), group(Some("Box"), &rect(5)))),
    ] {
        let (doc, report) = read_with(&shapes, &file);
        let page = doc.pages.get(0).unwrap();
        let mut groups = Vec::new();
        let mut stack: Vec<_> = page.shapes.iter().collect();
        while let Some(g) = stack.pop() {
            if !g.children.is_empty() {
                groups.push(g.clone());
                stack.extend(g.children.iter());
            }
        }
        assert!(groups.len() >= 2, "{shapes}");
        for g in &groups {
            assert!(cell(g, "Cost").is_none(), "no group of the shared name takes the entry: {shapes}");
            assert_ne!(g.id, 1, "and none is numbered the `ID` the entry holds: {shapes}");
        }
        assert_eq!(report.warning_counts.get("diacraft_group_name_shared"), Some(&1), "{:?}", report.warnings);
        assert_eq!(report.warning_counts.get("diacraft_group_unclaimed"), None, "said once and in its own words: {:?}", report.warnings);
        assert!(report.warnings.iter().any(|w| w.contains("several groups of a page have")), "{:?}", report.warnings);
    }
}

/// An entry with an `nth` is not an entry of a name: it is found by its number among the groups of the name, whether the name is shared or not, and it is found before
/// an entry with none. The entry with none, for a name that is shared, is warned of; the one with an `nth` is not.
#[test]
fn an_entry_with_an_nth_still_binds_by_number_beside_one_with_none_for_the_shared_name() {
    let shapes = group(Some("T"), &rect(1)) + &group(Some("T"), &rect(5));
    let file = file_of(&format!("{}{}", entry("T", 0, None, &cost("9")), entry("T", 2, None, &cost("2"))));
    let (doc, report) = read_with(&shapes, &file);
    assert!(cell(shape(&doc, 0, 0), "Cost").is_none(), "the first takes neither the entry of the shared name nor the second's");
    assert_eq!(num(cell(shape(&doc, 0, 1), "Cost")), 2.0, "the second takes the entry that has its number");
    assert_eq!(report.warning_counts.get("diacraft_group_name_shared"), Some(&1), "{:?}", report.warnings);
    // The name held by one group, with both entries: the one that has the number takes it, and the other is left over (warned of, as an entry no group takes).
    let (doc, report) =
        read_with(&group(Some("T"), &rect(1)), &file_of(&format!("{}{}", entry("T", 0, None, &cost("9")), entry("T", 1, None, &cost("1")))));
    assert_eq!(num(cell(shape(&doc, 0, 0), "Cost")), 1.0);
    assert_eq!(report.warning_counts.get("diacraft_group_unclaimed"), Some(&1), "{:?}", report.warnings);
    assert_eq!(report.warning_counts.get("diacraft_group_name_shared"), None, "{:?}", report.warnings);
}

/// The reservation of an unclaimed entry's `ID` is no more than that: an entry for a group that is not there holds its `ID` too (no other group is numbered it), and an
/// entry that is taken gives its group the `ID` it holds, exactly as before.
#[test]
fn an_entry_no_group_takes_keeps_its_id_from_every_group_and_an_entry_that_is_taken_gives_it() {
    // The first group would be numbered 1, which an entry for a group that is not there holds: it is 2.
    let (doc, _) = read_with(&group(Some("Box"), &rect(1)), &file_of(&entry("Gone", 0, Some(1), "")));
    assert_eq!(shape(&doc, 0, 0).id, 2, "the `ID` of an entry nothing takes is not given to another group");
    let (doc, _) = read_with(&group(Some("Box"), &rect(1)), &file_of(&entry("Box", 0, Some(1), "")));
    assert_eq!(shape(&doc, 0, 0).id, 1, "and the group that takes it has it");
}
