//! The `UniqueID` DiaCraft gives a master it makes: the reference app keeps only the first three groups of it, so those must tell every master apart.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Document, Master};

use super::{guid_text, prefix_key, repair_old_ids};
use crate::stencil::{self, MasterSpec, Outline, SIDES, bring, ensure_master, find_master, line_ends};

/// Every master DiaCraft can make: the shipped ones and the hidden line ends.
fn every_master() -> Vec<&'static MasterSpec> {
    stencil::all_masters().chain(line_ends::MASTERS.iter()).collect()
}

/// A text that is a GUID as DiaCraft writes one: braces, five groups, upper-case hexadecimal digits, version 4, variant 8 to B.
fn assert_v4_form(text: &str) {
    assert_eq!(text.len(), 38, "{text}");
    for (i, c) in text.char_indices() {
        match i {
            0 => assert_eq!(c, '{', "{text}"),
            37 => assert_eq!(c, '}', "{text}"),
            9 | 14 | 19 | 24 => assert_eq!(c, '-', "{text}"),
            _ => assert!(c.is_ascii_digit() || ('A'..='F').contains(&c), "{text}: `{c}` at {i}"),
        }
    }
    assert_eq!(text.as_bytes().get(15), Some(&b'4'), "version 4: {text}");
    assert!(matches!(text.as_bytes().get(20), Some(b'8' | b'9' | b'A' | b'B')), "variant: {text}");
}

fn spec_of(width: f64, identity: (u8, u16)) -> MasterSpec {
    MasterSpec {
        id: "test.unique",
        name: "Unique",
        width,
        height: 1.0,
        identity,
        outline: Outline::Ellipse,
        points: &SIDES,
        more: &stencil::dress::More::NONE,
    }
}

#[test]
fn every_master_has_a_unique_id_of_its_own_in_its_first_three_groups_and_keeps_its_base_id() {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    let all = every_master();
    assert!(all.len() > 150, "the whole catalogue is looked at: {}", all.len());
    for spec in all {
        let unique = spec.unique_id();
        assert_v4_form(&unique);
        assert_ne!(unique, spec.guid(), "{}: the UniqueID is not the BaseID's text", spec.id);
        if let Some(other) = seen.insert(prefix_key(&unique), spec.id) {
            panic!("{} and {other} have one UniqueID in the reference app's view: {unique}", spec.id);
        }
        let master = stencil::build_master(spec, 1);
        assert_eq!(master.unique_id.as_deref(), Some(unique.as_str()), "{}: the master has it", spec.id);
        assert_eq!(master.raw.get("BaseID"), Some(spec.guid().as_str()), "{}: the BaseID is the master's fixed identity", spec.id);
    }
}

#[test]
fn the_same_master_has_the_same_unique_id_whatever_its_place_and_a_changed_master_has_another() {
    let (a, again, wider, moved) = (spec_of(1.0, (1, 1)), spec_of(1.0, (1, 1)), spec_of(2.0, (1, 1)), spec_of(1.0, (1, 2)));
    assert_eq!(a.unique_id(), again.unique_id(), "the same master, the same UniqueID");
    // The `ID` a drawing gives a master is no part of what the master holds.
    assert_eq!(unique_with_id(&a, 1), unique_with_id(&a, 77));
    assert_ne!(prefix_key(&a.unique_id()), prefix_key(&wider.unique_id()), "a master that holds another size");
    assert_ne!(prefix_key(&a.unique_id()), prefix_key(&moved.unique_id()), "another ordinal");
    assert_eq!(a.guid(), wider.guid(), "the BaseID is the identity, and it stays");
}

fn unique_with_id(spec: &MasterSpec, id: u32) -> String {
    stencil::build_master(spec, id).unique_id.as_deref().unwrap().to_owned()
}

#[test]
fn the_same_built_in_master_gets_one_unique_id_in_every_drawing() {
    let spec = find_master("process").unwrap();
    let (mut one, mut two) = (Document::new_blank(), Document::new_blank());
    ensure_master(&mut two, find_master("rectangle").unwrap()).unwrap();
    let (id_one, id_two) = (ensure_master(&mut one, spec).unwrap(), ensure_master(&mut two, spec).unwrap());
    assert_ne!(id_one, id_two, "the drawings number their masters their own way");
    let (a, b) = (one.master(id_one).unwrap(), two.master(id_two).unwrap());
    assert_eq!((a.unique_id.as_deref(), a.raw.get("BaseID")), (b.unique_id.as_deref(), b.raw.get("BaseID")));
}

#[test]
fn a_guid_text_is_of_the_form_version_4_variant_a_and_is_made_of_the_bits_given() {
    for (hi, lo) in [(0_u64, 0_u64), (u64::MAX, u64::MAX), (0x0123_4567_89AB_CDEF, 0xFEDC_BA98_7654_3210), (1, 1)] {
        assert_v4_form(&guid_text(hi, lo));
    }
    // The first three groups are the first 60 bits of `hi`, with the version in the third group.
    assert_eq!(guid_text(0x0123_4567_89AB_CDEF, 0), "{01234567-89AB-4CDE-8000-000000000000}");
    // The fourth and fifth groups are `lo`'s 62 bits after the variant.
    assert_eq!(guid_text(0, 0x3FFF_FFFF_FFFF_FFFF), "{00000000-0000-4000-BFFF-FFFFFFFFFFFF}");
    assert_ne!(guid_text(1, 0), guid_text(0, 1));
}

#[test]
fn the_first_three_groups_of_a_text_are_what_the_reference_app_keeps_of_it() {
    assert_eq!(prefix_key("{d1ac0001-0000-4000-8000-000000000001}"), "{D1AC0001-0000-4000");
    assert_eq!(prefix_key("  {D1AC0001-0000-4000-8E40-00608CF305B2} "), "{D1AC0001-0000-4000");
    assert_eq!(prefix_key("{D1AC0001-0000-4000-8000-000000000001}"), prefix_key("{D1AC0001-0000-4000-8E40-00608CF305B2}"));
    assert_ne!(prefix_key("{D1AC0001-0000-4000-8000-000000000001}"), prefix_key("{D1AC0002-0000-4000-8000-000000000001}"));
    // A text that is no GUID of five groups is its own key, whole.
    assert_eq!(prefix_key("{q-z}"), "{Q-Z}");
    assert_eq!(prefix_key(" Box "), "BOX");
    assert_eq!(prefix_key(""), "");
    assert_eq!(prefix_key("{A-B-C}"), "{A-B-C}");
}

/// A master of a document, with the `UniqueID` and `BaseID` given.
fn master(id: u32, unique: &str, base: &str) -> Master {
    let mut m = Master { id, name: Some(Arc::from(format!("M{id}").as_str())), unique_id: Some(Arc::from(unique)), ..Master::default() };
    m.raw.push("BaseID", base);
    m
}

#[test]
fn a_renewed_unique_id_is_unique_in_its_first_three_groups_among_every_master_of_the_drawing() {
    // A drawing that holds a master whose UniqueID starts like the copy's (the reference app takes the two for one master).
    let mut doc = Document::new_blank();
    doc.add_master(master(1, "{D1AC0001-0000-4000-8E40-00608CF305B2}", "{B-1}")).unwrap();
    let taken = bring::unique_id_for(&doc, Some("{D1AC0001-0000-4000-8000-000000000007}")).unwrap();
    assert_ne!(prefix_key(&taken), prefix_key("{D1AC0001-0000-4000-8E40-00608CF305B2}"), "{taken}");
    assert_v4_form(&taken);
    // One the drawing holds nothing like is kept as it was.
    let free = bring::unique_id_for(&doc, Some("{D1AC0002-0000-4000-8000-000000000007}")).unwrap();
    assert_eq!(&*free, "{D1AC0002-0000-4000-8000-000000000007}");
    // Copy after copy of one source: each renewal differs from every master the drawing holds.
    let mut ids = vec![prefix_key("{D1AC0001-0000-4000-8E40-00608CF305B2}")];
    for n in 2..40_u32 {
        let next = bring::unique_id_for(&doc, Some("{D1AC0001-0000-4000-8000-000000000007}")).unwrap();
        assert!(!ids.contains(&prefix_key(&next)), "copy {n}: {next}");
        ids.push(prefix_key(&next));
        doc.add_master(master(n, &next, "{B-2}")).unwrap();
    }
}

// ---- the repair on save (owner ruling 2026-11-28) ------------------------------------------------------------------------------------

/// A built-in master as 0.2.3 made it: the `UniqueID` is the `BaseID`'s text.
fn as_0_2_3_made_it(id: &str, at: u32) -> Master {
    let spec = find_master(id).unwrap();
    let mut master = stencil::build_master(spec, at);
    master.unique_id = Some(Arc::from(spec.guid().as_str()));
    master
}

fn doc_of(masters: Vec<Master>) -> Document {
    let mut doc = Document::new_blank();
    for m in masters {
        doc.add_master(m).unwrap();
    }
    doc
}

fn unique_of(doc: &Document, id: u32) -> String {
    doc.master(id).unwrap().unique_id.as_deref().unwrap().to_owned()
}

#[test]
fn a_drawing_with_no_master_of_the_old_form_is_not_repaired_and_gives_no_copy() {
    assert!(repair_old_ids(&Document::new_blank()).is_none());
    let mut doc = Document::new_blank();
    ensure_master(&mut doc, find_master("process").unwrap()).unwrap();
    assert!(repair_old_ids(&doc).is_none(), "a master of today");
    let foreign = doc_of(vec![
        master(1, "{AB000001-0000-4000-8E40-00608CF305B2}", "{BA5E0001-1111-4222-8333-444444444444}"),
        master(2, "{AB000001-0000-4000-8E40-00608CF305B3}", "{BA5E0002-1111-4222-8333-444444444444}"),
    ]);
    assert!(repair_old_ids(&foreign).is_none(), "masters of another program");
}

#[test]
fn a_built_in_master_whose_unique_id_is_the_text_of_its_base_id_gets_the_unique_id_it_has_today_and_keeps_its_base_id() {
    let (rectangle, process) = (find_master("rectangle").unwrap(), find_master("process").unwrap());
    let before = doc_of(vec![
        as_0_2_3_made_it("rectangle", 1),
        as_0_2_3_made_it("process", 2),
        master(3, "{AB000001-0000-4000-8E40-00608CF305B2}", "{BA5E0001-1111-4222-8333-444444444444}"),
    ]);
    let after = repair_old_ids(&before).expect("two masters to repair");
    assert_eq!(unique_of(&after, 1), rectangle.unique_id());
    assert_eq!(unique_of(&after, 2), process.unique_id());
    assert_eq!(unique_of(&after, 3), "{AB000001-0000-4000-8E40-00608CF305B2}", "a master of another program is as it was");
    for id in 1..=3 {
        let (was, is) = (before.master(id).unwrap(), after.master(id).unwrap());
        assert_eq!(was.raw.get("BaseID"), is.raw.get("BaseID"), "{id}: the BaseID is kept");
        // Nothing else of the master is touched: the shapes and the sheet are the very ones.
        assert!(
            Arc::ptr_eq(&was.sheet, &is.sheet) && was.shapes == is.shapes && (was.id, &was.name, &was.name_u) == (is.id, &is.name, &is.name_u),
            "{id}"
        );
    }
    // The drawing the repair was asked of is as it was, and the repaired one is repaired no more.
    assert_eq!(unique_of(&before, 1), rectangle.guid());
    assert!(repair_old_ids(&after).is_none(), "done once");
}

#[test]
fn a_master_that_only_looks_like_one_of_ours_is_left_as_it_is() {
    let rectangle = find_master("rectangle").unwrap();
    let text = rectangle.guid();
    let mut kept = Vec::new();
    // The text of a built-in master as its `UniqueID`, and another `BaseID`.
    kept.push(master(1, &text, "{BA5E0001-1111-4222-8333-444444444444}"));
    // The text as `UniqueID`, and no `BaseID`.
    let mut bare = master(2, &text, "{x}");
    bare.raw = diacraft_doc::RawAttrs::new();
    kept.push(bare);
    // The `UniqueID` of a master that went through the application that opens the file (its last two groups are its own), and one of our series that no master has.
    kept.push(master(3, "{D1AC0001-0000-4000-8E40-00608CF305B2}", "{D1AC0001-0000-4000-8E40-00608CF305B2}"));
    kept.push(master(4, "{D1AC0001-0000-4000-8000-0000000000FF}", "{D1AC0001-0000-4000-8000-0000000000FF}"));
    // The text in lower case, with white space round it, and without braces: not the text DiaCraft wrote.
    kept.push(master(5, &text.to_ascii_lowercase(), &text.to_ascii_lowercase()));
    kept.push(master(6, &format!(" {text}"), &format!(" {text}")));
    kept.push(master(7, text.trim_matches(['{', '}']), text.trim_matches(['{', '}'])));
    // A `UniqueID` that is the text and a `BaseID` that is the same text in another case.
    kept.push(master(8, &text, &text.to_ascii_lowercase()));
    let doc = doc_of(kept);
    assert!(repair_old_ids(&doc).is_none(), "{:?}", doc.masters.iter().map(|m| m.unique_id.clone()).collect::<Vec<_>>());
    // Beside a master that is repaired, they are still as they were.
    let mut more = Document::clone(&doc);
    more.add_master(as_0_2_3_made_it("square", 9)).unwrap();
    let repaired = repair_old_ids(&more).expect("the square");
    for id in 1..=8 {
        assert_eq!(unique_of(&repaired, id), unique_of(&doc, id), "{id}");
    }
    assert_eq!(unique_of(&repaired, 9), find_master("square").unwrap().unique_id());
}

#[test]
fn the_hidden_line_ends_of_the_old_form_are_repaired_too() {
    let spec = &line_ends::MASTERS[0];
    let mut old = stencil::build_master(spec, 1);
    old.unique_id = Some(Arc::from(spec.guid().as_str()));
    let after = repair_old_ids(&doc_of(vec![old])).expect("a line end of 0.2.3");
    assert_eq!(unique_of(&after, 1), spec.unique_id());
}

#[test]
fn two_masters_of_the_old_form_of_one_built_in_text_are_repaired_to_two_ids_that_differ_in_their_first_three_groups() {
    // (A drawing that holds the master twice, as a hostile file or a hand copy does: they read as one master to the application, and must not after the repair.)
    let doc = doc_of(vec![as_0_2_3_made_it("rectangle", 1), as_0_2_3_made_it("rectangle", 2), as_0_2_3_made_it("rectangle", 3)]);
    let after = repair_old_ids(&doc).unwrap();
    let ids: Vec<String> = (1..=3).map(|id| unique_of(&after, id)).collect();
    let keys: std::collections::HashSet<String> = ids.iter().map(|u| prefix_key(u)).collect();
    assert_eq!(keys.len(), 3, "{ids:?}");
    assert_eq!(ids[0], find_master("rectangle").unwrap().unique_id(), "the first has the one of today");
    ids.iter().for_each(|u| assert_v4_form(u));
}

#[test]
fn a_repaired_id_never_takes_the_first_three_groups_of_a_master_the_drawing_holds() {
    // The drawing already holds a master that has the id the rectangle gets today (another master of one BaseID, as a copy or a stranger may be).
    let today = find_master("rectangle").unwrap().unique_id();
    let doc = doc_of(vec![master(1, &today, "{BA5E0001-1111-4222-8333-444444444444}"), as_0_2_3_made_it("rectangle", 2)]);
    let after = repair_old_ids(&doc).unwrap();
    assert_eq!(unique_of(&after, 1), today, "the stranger is as it was");
    assert_ne!(prefix_key(&unique_of(&after, 2)), prefix_key(&today), "{}", unique_of(&after, 2));
    assert_v4_form(&unique_of(&after, 2));
}

#[test]
fn a_drawing_of_thousands_of_masters_is_repaired_in_one_bounded_pass() {
    let masters: Vec<Master> = (1..=3_000_u32).map(|n| as_0_2_3_made_it(if n % 2 == 0 { "rectangle" } else { "ellipse" }, n)).collect();
    let doc = doc_of(masters);
    let after = repair_old_ids(&doc).unwrap();
    let keys: std::collections::HashSet<String> = after.masters.iter().filter_map(|m| m.unique_id.as_deref()).map(prefix_key).collect();
    assert_eq!(keys.len(), 3_000, "every master of the drawing is told apart");
}

#[test]
fn masters_that_hold_the_ids_a_repair_would_try_cost_it_no_more_than_the_masters_it_looks_at() {
    // A hostile drawing: a thousand masters that hold, in their first three groups, the first thousand ids a repair of the rectangle would try, and a thousand masters of the old form of the rectangle.
    // A repair that began again at the first try for each of the thousand would make a million.
    let wanted = find_master("rectangle").unwrap().unique_id();
    let mut masters = Vec::new();
    for n in 1..=1_000_u32 {
        masters.push(master(n, &super::renewed(&wanted, u64::from(n)), "{BA5E0001-1111-4222-8333-444444444444}"));
    }
    for n in 1..=1_000_u32 {
        masters.push(as_0_2_3_made_it("rectangle", 1_000 + n));
    }
    let doc = doc_of(masters);
    super::TRIES.with(|t| t.set(0));
    let after = repair_old_ids(&doc).expect("a thousand masters to repair");
    let tries = super::TRIES.with(|t| t.get());
    assert!(tries <= 2_000 + 64, "{tries} ids were tried for a drawing of 2000 masters");
    let keys: std::collections::HashSet<String> = after.masters.iter().filter_map(|m| m.unique_id.as_deref()).map(prefix_key).collect();
    assert_eq!(keys.len(), 2_000, "every master of the drawing is told apart after it");
}
