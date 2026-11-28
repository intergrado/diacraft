//! What the patcher does with a row or a section that an edit numbered again, and with unknown nodes whose place among the typed children an edit left stale.
//!
//! An indexed row is told by its `IX`, and the commands that insert and delete rows number the rows after them again: the row that was number 2 is
//! number 3 now. The patcher matches an item to its element by the allocation and then by its key, so the element that held number 2 would be given the content of the new number 2, and
//! the unknown attribute, the unknown element and the comment that the old number 2 held would be given to another row, or refused (`a change to the unknown elements of a row`). An item
//! numbered again has to find **its own element**: the one that held what it holds, with the number left out. And a node that no edit touched is the same node wherever the count of typed
//! children before it says it stands: an insert before it leaves that count as it was, and the tree keeps the node attached to the item it followed.

use std::sync::Arc;

use diacraft_container::xml::{El, write_element};
use diacraft_doc::{Row, Section, Sheet};
use diacraft_sheetxml::PatchError;
use diacraft_sheetxml::patch::patch_sheet_children;

use super::{BIG, Main, parse, read_with};

fn text_of(el: &El) -> String {
    let mut out = Vec::new();
    write_element(&mut out, el).unwrap();
    String::from_utf8(out).unwrap()
}

fn read_sheet(xml: &str) -> Sheet {
    read_with::<Main>(xml, BIG).unwrap().sheet
}

/// The tree `xml` is, patched from its own model to the edited one, and the model read from what came out.
fn patched(xml: &str, edit: impl FnOnce(&mut Sheet)) -> Result<(String, Sheet), PatchError> {
    let base = read_sheet(xml);
    let mut now = base.clone();
    edit(&mut now);
    let mut el = parse(xml);
    let sc = diacraft_sheetxml::Scope::<Main>::empty().enter(&el);
    patch_sheet_children::<Main, PatchError>(&mut el, &sc, &base, &now)?;
    let text = text_of(&el);
    let back = read_sheet(&text);
    Ok((text, back))
}

/// A Scratch section of three rows, the last of which holds an unknown attribute and an unknown element.
const SCRATCH: &str = concat!(
    r#"<PageSheet><Section N="Scratch">"#,
    r#"<Row IX="0"><Cell N="A" V="1"/></Row>"#,
    r#"<Row IX="1"><Cell N="A" V="2"/></Row>"#,
    r#"<Row IX="2" Zip="kept"><Cell N="A" V="3"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#,
    r#"</Section></PageSheet>"#
);

/// The rows of a section after an insert at `at`: a new row takes the number, and the rows from it on are numbered again, one up.
fn insert_at(section: &mut Section, at: u32, a: &str) {
    let rows: Vec<Arc<Row>> = section.rows.iter().cloned().collect();
    let mut out: Vec<Arc<Row>> = Vec::new();
    for row in rows {
        let mut row = (*row).clone();
        if row.ix.is_some_and(|ix| ix >= at) {
            row.ix = row.ix.map(|ix| ix + 1);
            out.push(Arc::new(row));
        } else {
            out.push(Arc::new(row));
        }
    }
    let mut new = Row { ix: Some(at), ..Row::default() };
    new.cells.insert("A".into(), diacraft_doc::Cell::read(None, Some(a), None, None, &mut 0));
    out.insert(at as usize, Arc::new(new));
    section.rows = out.into();
}

#[test]
fn a_row_numbered_again_takes_its_unknown_attribute_and_its_unknown_element_with_it() {
    let (text, back) = patched(SCRATCH, |sheet| insert_at(Arc::make_mut(&mut sheet.sections[0]), 1, "9")).unwrap();
    // The element that held the third row holds the fourth now, and has its `Zip` and its mark; the new row has neither.
    assert!(text.contains(r#"<Row IX="3" Zip="kept"><Cell N="A" V="3"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#), "{text}");
    assert!(text.contains(r#"<Row IX="1"><Cell N="A" V="9"/></Row>"#), "{text}");
    assert!(text.contains(r#"<Row IX="2"><Cell N="A" V="2"/></Row>"#), "{text}");
    let section = &back.sections[0];
    assert_eq!(section.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(section.rows[3].passthrough.len(), 1, "the unknown element is in the row it was in");
    assert!(section.rows[1].passthrough.is_empty() && section.rows[2].passthrough.is_empty());
}

#[test]
fn a_row_numbered_again_whose_cells_changed_with_the_numbers_still_takes_its_unknown_nodes_with_it() {
    // The row that moved reads the one before it by number, so the insert writes its formula again.
    let xml = concat!(
        r#"<PageSheet><Section N="Scratch">"#,
        r#"<Row IX="0"><Cell N="A" V="1"/></Row>"#,
        r#"<Row IX="1" Zip="kept"><Cell N="A" V="2" F="Scratch.A1+1"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#,
        r#"</Section></PageSheet>"#
    );
    let (text, back) = patched(xml, |sheet| {
        let section = Arc::make_mut(&mut sheet.sections[0]);
        insert_at(section, 0, "9");
        let moved = Arc::make_mut(&mut section.rows[2]);
        moved.cells.insert("A".into(), diacraft_doc::Cell::read(Some("Scratch.A2+1"), Some("2"), None, None, &mut 0));
    })
    .unwrap();
    assert!(text.contains(r#"<Row IX="2" Zip="kept"><Cell N="A" V="2" F="Scratch.A2+1"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#), "{text}");
    assert_eq!(back.sections[0].rows[2].passthrough.len(), 1);
}

#[test]
fn a_geometry_section_numbered_again_takes_its_unknown_nodes_with_it_and_the_one_taken_out_takes_its_own() {
    let xml = concat!(
        r#"<PageSheet>"#,
        r#"<Section N="Geometry" IX="0"><x:Hint xmlns:x="urn:z" a="first"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row></Section>"#,
        r#"<Section N="Geometry" IX="1" Zap="kept"><Row T="MoveTo" IX="1"><Cell N="X" V="5"/></Row><x:Hint xmlns:x="urn:z" a="second"/></Section>"#,
        r#"</PageSheet>"#
    );
    // The first section goes, and the second is the first now.
    let (text, back) = patched(xml, |sheet| {
        sheet.sections.remove(0);
        Arc::make_mut(&mut sheet.sections[0]).ix = Some(0);
    })
    .unwrap();
    assert!(text.contains(r#"IX="0" Zap="kept""#) && text.contains(r#"a="second""#), "{text}");
    assert!(!text.contains(r#"a="first""#), "the unknown element of the section that went goes with it: {text}");
    assert_eq!(back.sections.len(), 1);
    assert_eq!(back.sections[0].ix, Some(0));
}

#[test]
fn rows_that_held_nothing_unknown_are_numbered_again_in_place_and_the_comment_after_one_stays_with_its_neighbours() {
    let xml = concat!(
        r#"<PageSheet><Section N="Scratch">"#,
        r#"<Row IX="0"><Cell N="A" V="1"/></Row><!-- after the first -->"#,
        r#"<Row IX="1"><Cell N="A" V="2"/></Row>"#,
        r#"</Section></PageSheet>"#
    );
    let (text, _) = patched(xml, |sheet| insert_at(Arc::make_mut(&mut sheet.sections[0]), 1, "9")).unwrap();
    // The comment stays right after the row it followed, and the rows that were numbered again are the elements that held them.
    assert_eq!(
        text,
        r#"<PageSheet><Section N="Scratch"><Row IX="0"><Cell N="A" V="1"/></Row><!-- after the first --><Row IX="1"><Cell N="A" V="9"/></Row><Row IX="2"><Cell N="A" V="2"/></Row></Section></PageSheet>"#
    );
}

/// A comment and an unknown element in a section whose rows an edit inserted a row before: the nodes are the same, the count of typed children before them is not.
#[test]
fn the_unknown_nodes_of_a_section_are_the_same_nodes_when_only_the_count_before_them_is_stale() {
    let xml = concat!(
        r#"<PageSheet><Section N="User">"#,
        r#"<Row N="a"><Cell N="Value" V="1"/></Row>"#,
        r#"<!-- a comment --><x:Mark xmlns:x="urn:z"/>"#,
        r#"<Row N="b"><Cell N="Value" V="2"/></Row>"#,
        r#"</Section></PageSheet>"#
    );
    let (text, back) = patched(xml, |sheet| {
        let section = Arc::make_mut(&mut sheet.sections[0]);
        let mut row = Row { name: Some("n".into()), ..Row::default() };
        row.cells.insert("Value".into(), diacraft_doc::Cell::read(None, Some("0"), None, None, &mut 0));
        section.rows.insert(0, Arc::new(row));
        // The model the session holds says the nodes stand after one row (as read); the file the first save wrote has them after two, and is the baseline of the next save: the count is stale.
        for placed in section.passthrough.nodes_mut() {
            placed.after = 0;
        }
    })
    .unwrap();
    assert_eq!(
        text,
        r#"<PageSheet><Section N="User"><Row N="n"><Cell N="Value" V="0"/></Row><Row N="a"><Cell N="Value" V="1"/></Row><!-- a comment --><x:Mark xmlns:x="urn:z"/><Row N="b"><Cell N="Value" V="2"/></Row></Section></PageSheet>"#
    );
    assert_eq!(back.sections[0].passthrough.len(), 2);
}

/// The engine deletes the row and inserts another at the number it had (a number nobody holds is taken): the row that went takes its unknown attribute and element with it, and
/// the one that took its number is a new row and holds none.
#[test]
fn a_row_that_took_the_number_of_a_row_that_went_is_a_new_row_and_the_one_that_went_takes_its_unknown_nodes_with_it() {
    let (text, back) = patched(SCRATCH, |sheet| {
        let section = Arc::make_mut(&mut sheet.sections[0]);
        section.rows.pop();
        let mut new = Row { ix: Some(2), ..Row::default() };
        new.cells.insert("A".into(), diacraft_doc::Cell::read(None, Some("9"), None, None, &mut 0));
        section.rows.push(Arc::new(new));
    })
    .unwrap();
    assert!(text.contains(r#"<Row IX="2"><Cell N="A" V="9"/></Row>"#), "{text}");
    assert!(!text.contains("Zip") && !text.contains("x:Mark"), "{text}");
    assert!(back.sections[0].rows[2].passthrough.is_empty());
}

/// The engine drops a section a master gives by a local `Del` section that holds its key and nothing else: the section the sheet held, with its unknown nodes, is not edited into it.
#[test]
fn a_section_that_became_a_deleted_section_is_written_as_the_model_has_it() {
    let xml = r#"<PageSheet><Section N="User"><Row N="a"/><x:Mark xmlns:x="urn:z" a="1"/></Section></PageSheet>"#;
    let (text, back) = patched(xml, |sheet| {
        let mut gone = Section::new("User", None);
        gone.del = true;
        sheet.sections[0] = Arc::new(gone);
    })
    .unwrap();
    assert_eq!(text, r#"<PageSheet><Section N="User" Del="1"/></PageSheet>"#);
    assert!(back.sections[0].del && back.sections[0].rows.is_empty());
}

/// The held data of a row (what an `.odg` carried for it) is no part of the tree, so a row whose held data changed is the row its element holds: it is patched in place, with its layout.
#[test]
fn a_row_whose_held_data_changed_is_patched_in_place_and_not_made_again() {
    let xml = "<PageSheet>\n  <Section N=\"Scratch\">\n    <Row IX=\"0\">\n      <Cell N=\"A\" V=\"1\"/>\n    </Row>\n  </Section>\n</PageSheet>";
    let (text, _) = patched(xml, |sheet| {
        let section = Arc::make_mut(&mut sheet.sections[0]);
        let row = Arc::make_mut(&mut section.rows[0]);
        row.ext.make_mut().odf.style_name = Some("P1".into());
    })
    .unwrap();
    assert_eq!(text, text_of(&parse(xml)), "the layout of the row stays");
}

/// What the patcher cannot write is still said: the unknown nodes of the sheet itself, which is no list of items numbered again, are not edited.
#[test]
fn a_node_of_the_sheet_that_an_edit_changed_or_took_out_is_still_unsupported() {
    let xml = r#"<PageSheet><Cell N="A"/><x:Mark xmlns:x="urn:z" a="1"/></PageSheet>"#;
    let e = patched(xml, |sheet| sheet.passthrough = Default::default()).unwrap_err();
    assert!(matches!(e, PatchError::Unsupported(what) if what.contains("unknown elements of a sheet")), "{e:?}");
}

// ------------------------------------------------------------------------------------------------------------------------ costs, as counts

/// A toy item: its number, and what it holds of what the file has no field for.
#[derive(Debug)]
struct Toy {
    key: u32,
    unknown: u32,
}

/// What `match_items_with` asks of a list of toys: what it paired, and how many comparisons and fingerprints it made to do it.
fn matched_toys(base: &[Toy], now: &[Toy], fingerprint_of: impl Fn(&Toy) -> u64) -> (Vec<Option<usize>>, usize, usize) {
    use diacraft_sheetxml::patch::{Identity, match_items_with};
    let (compares, prints) = (std::cell::Cell::new(0usize), std::cell::Cell::new(0usize));
    let (b, n): (Vec<&Toy>, Vec<&Toy>) = (base.iter().collect(), now.iter().collect());
    let same = |a: &Toy, b: &Toy| {
        compares.set(compares.get() + 1);
        a.unknown == b.unknown
    };
    let print = |t: &Toy| {
        prints.set(prints.get() + 1);
        fingerprint_of(t)
    };
    let identity = Identity { same: &same, fingerprint: &print };
    let (matched, _) = match_items_with(&b, &n, &|t: &Toy| Some(t.key), Some(&identity));
    (matched, compares.get(), prints.get())
}

/// 31,273 rows with one that holds an unknown node, numbered again by an insert at the front: the pairs the key made are told once each, the two that the key put onto the wrong row are
/// paired again, and nothing is compared with every row.
#[test]
fn a_section_of_31273_rows_numbered_again_costs_one_comparison_a_row_and_a_few_more() {
    let n = 31_273u32;
    let base: Vec<Toy> = (0..n).map(|k| Toy { key: k, unknown: u32::from(k == 5) * 7 }).collect();
    // The new row takes 0; the row that was k is k + 1.
    let mut now: Vec<Toy> = vec![Toy { key: 0, unknown: 0 }];
    now.extend(base.iter().map(|t| Toy { key: t.key + 1, unknown: t.unknown }));
    let (matched, compares, prints) = matched_toys(&base, &now, |t| u64::from(t.unknown));
    // The row with the node is paired with its own row, and every other row with a row that holds none.
    assert_eq!(matched.get(6).copied().flatten(), Some(5), "the row that held the node is the element that held it");
    assert_eq!(matched.iter().filter(|m| m.is_none()).count(), 1, "the rows are one more than they were, so one is new");
    assert!(compares <= n as usize + 64, "{compares} comparisons for {n} rows");
    assert!(prints <= 64, "{prints} fingerprints");
}

/// Nothing is the same and every fingerprint is: the comparisons stop at the bound.
#[test]
fn rows_that_nothing_tells_apart_cost_no_more_than_the_bound() {
    use diacraft_sheetxml::patch::MOVED_COMPARES_PER_ITEM;
    let n = 2_000u32;
    let base: Vec<Toy> = (0..n).map(|k| Toy { key: k, unknown: k }).collect();
    let now: Vec<Toy> = (0..n).map(|k| Toy { key: k, unknown: k + 100_000 }).collect();
    let (matched, compares, _) = matched_toys(&base, &now, |_| 0);
    assert!(compares <= MOVED_COMPARES_PER_ITEM * 2 * n as usize, "{compares} comparisons for {n} rows");
    assert_eq!(matched.len(), n as usize);
}

/// An item the edit did not copy, the very allocation of the other model, is never compared.
#[test]
fn an_item_the_edit_did_not_copy_is_never_compared() {
    use diacraft_sheetxml::patch::{Identity, match_items_with};
    let rows: Vec<Toy> = (0..500u32).map(|k| Toy { key: k, unknown: k }).collect();
    let all: Vec<&Toy> = rows.iter().collect();
    let compared = std::cell::Cell::new(0usize);
    let same = |_: &Toy, _: &Toy| {
        compared.set(compared.get() + 1);
        true
    };
    let identity = Identity { same: &same, fingerprint: &|_: &Toy| 0 };
    let (matched, used) = match_items_with(&all, &all, &|t: &Toy| Some(t.key), Some(&identity));
    assert_eq!(compared.get(), 0);
    assert!(matched.iter().enumerate().all(|(j, m)| *m == Some(j)) && used.iter().all(|u| *u));
}
