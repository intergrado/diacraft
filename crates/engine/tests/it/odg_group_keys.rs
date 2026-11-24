//! Fix round: the data, the `ID` and the link to a master of a group are found by the group's `draw:name`, which is unique among the page's groups,
//! and never by its place: another program that reorders the groups, puts one before them or takes one away must not move a group's data to another group. A group that
//! `shape.group` makes has no name, so a save gives it one (`Group_<ID>`); each case saves two groups that hold data (one with an `ID` a formula names and one that is
//! an instance of a master), rewrites the file's `content.xml` as the other program would, opens it in a session of its own and asks of each group what it holds.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Value};
use diacraft_engine::Session;
use diacraft_testkit::package::VsdxBuilder;
use serde_json::json;

use crate::odg_build::{Dir, content_of, open_drawing, package, part_bytes};
use crate::odg_file::{four, save};
use crate::odg_masters::pair_master;
use crate::run;
use crate::stencil_open::{CT_STENCIL, Dir as StencilDir, open};

/// What the session knows of the two groups it saved, to be found again.
struct Saved {
    /// The bytes of the saved file.
    bytes: Vec<u8>,
    /// The name of the first member of group A (two shapes dropped and grouped), which tells it from group B (an instance of a master).
    member_of_a: String,
    /// The `ID`s of the two groups, A's named by a formula of another shape.
    id_a: u32,
    id_b: u32,
    /// The master B links to, and the name the drop gave B (a name of its own, unique on the page: it keeps it).
    master_b: u32,
    name_b: String,
}

/// Sets the cell `Cost` of the shape `id` of the first page to `value`.
fn give_cost(s: &mut Session, id: u32, value: &str) {
    s.edit("Test setup: data on a group", |doc: &mut Document| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == id).unwrap();
        let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Cost"), Cell::constant(Value::from_text(value)));
        Ok(())
    })
    .unwrap();
}

/// Two new groups with data saved to an `.odg`: A of a rectangle and an ellipse, with `Cost` 1 and an `ID` that the formula of the shape `C` names; B dropped from a
/// master of two shapes (a link to the master), with `Cost` 2.
fn saved_groups(dir: &Dir, stencils: &StencilDir) -> Saved {
    let mut s = open_drawing(dir, &four());
    let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 6.0, "y": 6.0 }));
    let b = run(&mut s, "shape.drop", json!({ "master": "ellipse", "page": 0, "x": 8.0, "y": 7.0 }));
    let member_of_a = s
        .document()
        .unwrap()
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find(|x| u64::from(x.id) == a["id"].as_u64().unwrap())
        .unwrap()
        .name_u
        .as_deref()
        .unwrap()
        .to_owned();
    let group = run(&mut s, "shape.group", json!({ "page": 0, "ids": [a["id"], b["id"]] }));
    let id_a = u32::try_from(group["id"].as_u64().unwrap()).unwrap();
    give_cost(&mut s, id_a, "1");
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(pair_master()).build();
    let opened = open(&mut s, &stencils.write("pair.vssx", &bytes));
    let dropped = run(&mut s, "shape.drop", json!({ "master": "Pair", "stencil": opened["id"], "page": 0, "x": 12.0, "y": 8.0 }));
    let id_b = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    give_cost(&mut s, id_b, "2");
    let b_shape = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id_b).unwrap().clone();
    let master_b = b_shape.master.expect("an instance of the master");
    let name_b = b_shape.name_u.as_deref().expect("a dropped group has a name").to_owned();
    // The shapes `C` and `D` read the widths of A and B: their `ID`s are named by formulas, so they are kept in the entries.
    for (shape_name, named) in [("C", id_a), ("D", id_b)] {
        s.edit("Test setup: a formula that names a group", |doc: &mut Document| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let at = page.shapes.iter().position(|x| x.name_u.as_deref() == Some(shape_name)).unwrap();
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            let cell = Cell::with_formula(&format!("Sheet.{named}!Width"), Some(Value::from_text("1")));
            Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Link"), cell);
            Ok(())
        })
        .unwrap();
    }
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    Saved { bytes: std::fs::read(dir.path("a.odg")).unwrap(), member_of_a, id_a, id_b, master_b, name_b }
}

/// The byte ranges of the groups at the top level of the page in `content` (the groups here hold no group).
fn groups_in(content: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = content[at..].find("<draw:g") {
        let start = at + found;
        let after = content[start + 7..].chars().next();
        if !matches!(after, Some(' ' | '>')) {
            at = start + 7;
            continue;
        }
        let end = start + content[start..].find("</draw:g>").unwrap() + "</draw:g>".len();
        out.push((start, end));
        at = end;
    }
    out
}

/// The file with its `content.xml` replaced, as another program that did not know DiaCraft's document file would leave it with the other parts as they were.
fn with_content(saved: &Saved, content: &str) -> Vec<u8> {
    let mut files = vec![("content.xml".to_owned(), content.as_bytes().to_vec())];
    for name in ["styles.xml", "meta.xml", "DiaCraft/document.xml"] {
        if let Some(bytes) = part_bytes(&saved.bytes, name) {
            files.push((name.to_owned(), bytes));
        }
    }
    package("application/vnd.oasis.opendocument.graphics", files)
}

/// What a group of the file read again holds: whether it has A's first member, its `ID`, its `Cost` and its master.
fn held(doc: &Document, member_of_a: &str) -> Vec<(bool, u32, Option<f64>, Option<u32>)> {
    doc.pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .filter(|g| !g.children.is_empty())
        .map(|g| {
            let has_a = g.children.iter().any(|m| m.name_u.as_deref() == Some(member_of_a));
            (has_a, g.id, g.sheet.cell("Cost").and_then(|c| c.value.as_ref()?.number()), g.master)
        })
        .collect()
}

/// A session that opens `bytes`, and what it holds of the groups.
fn read_groups(dir: &Dir, bytes: &[u8], member_of_a: &str) -> Vec<(bool, u32, Option<f64>, Option<u32>)> {
    let path = dir.write("other.odg", bytes);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": path }));
    held(again.document().unwrap(), member_of_a)
}

#[test]
fn a_save_names_the_groups_it_writes_and_the_names_are_unique_on_the_page() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    assert_eq!(spans.len(), 2, "{content}");
    for (name, (start, _)) in [format!("Group_{}", saved.id_a), saved.name_b.clone()].iter().zip(&spans) {
        let tag = &content[*start..content[*start..].find('>').map(|e| start + e).unwrap()];
        assert!(tag.contains(&format!("draw:name=\"{name}\"")), "the group is named {name}: {tag}");
    }
    let document = String::from_utf8(part_bytes(&saved.bytes, "DiaCraft/document.xml").unwrap()).unwrap();
    assert_eq!(document.matches("<diacraft:group ").count(), 2, "{document}");
    assert!(document.contains(&format!("name=\"Group_{}\"", saved.id_a)) && document.contains(&format!("name=\"{}\"", saved.name_b)), "{document}");
    for tag in document.split("<diacraft:group ").skip(1) {
        let tag = &tag[..tag.find('>').unwrap()];
        assert!(!tag.contains(" nth="), "a name that is unique is a key by itself: {tag}");
    }
    // Read as it is, each group holds its own.
    let read = read_groups(&dir, &saved.bytes, &saved.member_of_a);
    assert_eq!(read, [(true, saved.id_a, Some(1.0), None), (false, saved.id_b, Some(2.0), Some(saved.master_b))]);
}

/// Another program puts the second group before the first: each group keeps its own data, `ID` and master (the review's case 1).
#[test]
fn groups_put_in_another_order_by_another_program_keep_their_own_data() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    let (first, second) = (&content[spans[0].0..spans[0].1], &content[spans[1].0..spans[1].1]);
    let swapped = format!("{}{second}{first}{}", &content[..spans[0].0], &content[spans[1].1..]);
    let read = read_groups(&dir, &with_content(&saved, &swapped), &saved.member_of_a);
    assert_eq!(read, [(false, saved.id_b, Some(2.0), Some(saved.master_b)), (true, saved.id_a, Some(1.0), None)], "no entry moved to another group");
}

/// Another program puts a group of its own, with no name, before the two: neither entry moves to it or to the other (the review's case 2).
#[test]
fn a_group_another_program_puts_first_takes_no_entry_and_moves_none() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    let extra = r#"<draw:g><draw:rect svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"><text:p/></draw:rect></draw:g>"#;
    let inserted = format!("{}{extra}{}", &content[..spans[0].0], &content[spans[0].0..]);
    let read = read_groups(&dir, &with_content(&saved, &inserted), &saved.member_of_a);
    assert_eq!(read.len(), 3);
    assert_eq!(read[0], (false, read[0].1, None, None), "the group that is new has nothing of the others'");
    assert_eq!(read[1], (true, saved.id_a, Some(1.0), None));
    assert_eq!(read[2], (false, saved.id_b, Some(2.0), Some(saved.master_b)));
}

/// Another program takes the first group away: the second keeps its own, and does not take the first's (the review's case 3).
#[test]
fn a_group_another_program_takes_away_leaves_the_others_theirs() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    let removed = format!("{}{}", &content[..spans[0].0], &content[spans[0].1..]);
    let read = read_groups(&dir, &with_content(&saved, &removed), &saved.member_of_a);
    assert_eq!(read, [(false, saved.id_b, Some(2.0), Some(saved.master_b))], "the group that was left keeps its own `ID`, data and master");
}

/// The formula of the `Link` cell of the shape `C` (it reads `Width` of the shape with A's `ID`), the `ID` and width of every group of the page and the number of
/// warnings the open gave, of the file read again.
fn link_and_widths(dir: &Dir, bytes: &[u8]) -> (String, Vec<(u32, Option<f64>)>, u64) {
    let path = dir.write("other.odg", bytes);
    let mut again = Session::new();
    let opened = run(&mut again, "file.open", json!({ "path": path }));
    let page = again.document().unwrap().pages.get(0).unwrap().clone();
    let link = page.shapes.iter().find(|s| s.name_u.as_deref() == Some("C")).and_then(|c| c.sheet.cell("Link")).map(|c| format!("{:?}", c.formula));
    let widths = page
        .shapes
        .iter()
        .filter(|g| !g.children.is_empty())
        .map(|g| (g.id, g.sheet.cell("Width").and_then(|c| c.value.as_ref()?.number())))
        .collect();
    (link.unwrap_or_default(), widths, opened["file"]["warning_count"].as_u64().unwrap_or(0))
}

/// `group` with the first `svg:width` in it made `to` wide: the copy of a group that another program stretched.
fn widened(group: &str, to: &str) -> String {
    let at = group.find("svg:width=\"").expect("a member with a width") + "svg:width=\"".len();
    let end = at + group[at..].find('"').unwrap();
    format!("{}{to}{}", &group[..at], &group[end..])
}

/// Another program copies group A and puts the copy before it, so that two groups of the page carry A's name: the entry of A has no `nth` (the name was unique when it
/// was saved), so it binds to neither. The original keeps nothing of the entry (the file does not say which of the two A is), the copy takes none, the page warns,
/// and the formula of `C` that read A's width does not read the copy's (the review's reader-side case 1: an entry with no `nth` bound to the first group of its name).
#[test]
fn a_copy_of_a_group_put_before_it_leaves_the_entry_of_the_name_unread_and_the_open_says_so() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    let a = &content[spans[0].0..spans[0].1];
    let copied = format!("{}{}{}", &content[..spans[0].0], widened(a, "30cm"), &content[spans[0].0..]);
    let bytes = with_content(&saved, &copied);
    let read = read_groups(&dir, &bytes, &saved.member_of_a);
    assert_eq!(read.len(), 3, "{copied}");
    for (at, group) in read.iter().take(2).enumerate() {
        assert_eq!((group.2, group.3), (None, None), "neither group of the shared name takes A's entry (group {at}): {read:?}");
        assert_ne!(group.1, saved.id_a, "and neither takes A's `ID` (group {at}): {read:?}");
    }
    assert_eq!(read[2], (false, saved.id_b, Some(2.0), Some(saved.master_b)), "B keeps its own");
    // The shape C still names A's `ID` (what it reads is the file's and is not changed), and no group has it now: it reads the width of none, and not the copy's.
    let (link, widths, warnings) = link_and_widths(&dir, &bytes);
    assert!(link.contains(&format!("Sheet.{}!Width", saved.id_a)), "{link}");
    assert!(widths.iter().all(|(id, _)| *id != saved.id_a), "no group takes the `ID` of A's entry: {widths:?}");
    assert!(warnings >= 1, "the data A's entry held is not read, and the open says so");
}

/// Another program renames B to A's name and puts it before A: it takes neither the data nor the `ID` of A, and A's entry binds to neither (the review's reader-side
/// case 2). B's own entry (its name is no longer there) is left over.
#[test]
fn a_group_renamed_to_the_name_of_another_and_put_before_it_takes_nothing_of_the_others_and_the_open_says_so() {
    let (dir, stencils) = (Dir::new(), StencilDir::new());
    let saved = saved_groups(&dir, &stencils);
    let content = content_of(&saved.bytes);
    let spans = groups_in(&content);
    let (a, b) = (&content[spans[0].0..spans[0].1], &content[spans[1].0..spans[1].1]);
    let renamed = b.replacen(&format!("draw:name=\"{}\"", saved.name_b), &format!("draw:name=\"Group_{}\"", saved.id_a), 1);
    assert_ne!(renamed, b, "B was renamed: {b}");
    let swapped = format!("{}{renamed}{a}{}", &content[..spans[0].0], &content[spans[1].1..]);
    let bytes = with_content(&saved, &swapped);
    let read = read_groups(&dir, &bytes, &saved.member_of_a);
    assert_eq!(read.len(), 2);
    for (at, group) in read.iter().enumerate() {
        assert_eq!((group.2, group.3), (None, None), "no group of the shared name takes an entry (group {at}): {read:?}");
        assert_ne!(group.1, saved.id_a, "and none takes A's `ID` (group {at}): {read:?}");
    }
    let (link, widths, warnings) = link_and_widths(&dir, &bytes);
    assert!(link.contains(&format!("Sheet.{}!Width", saved.id_a)), "{link}");
    assert!(widths.iter().all(|(id, _)| *id != saved.id_a), "no group takes the `ID` of A's entry, the renamed one neither: {widths:?}");
    assert!(warnings >= 1, "the data A's and B's entries held is not read, and the open says so");
}
