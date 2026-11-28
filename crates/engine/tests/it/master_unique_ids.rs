//! The reference app keeps only the first three groups of a master's `UniqueID` (it puts a value of its own in the last two on load) and keeps `BaseID` as written. The
//! masters of a stencil that differ in their last two groups only were one master there: every drop from a DiaCraft stencil gave the first master dropped, and a paste between two DiaCraft
//! drawings gave the destination's master of that place. So **the first three groups of the `UniqueID` DiaCraft writes tell every master apart** (a hash of the master's identity and what it
//! holds), `BaseID` stays the master's fixed identity, and a master a file already had keeps both as the file had them. These tests read the XML a save writes.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_engine::stencil::{ensure_master, stencils};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::stencil_open::{CT_STENCIL, Dir, box_cells};
use crate::{blank_session, run};

/// The attributes of one `Master` row of a saved file's `masters.xml`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    name_u: String,
    unique: String,
    base: String,
}

fn attr(tag: &str, name: &str) -> String {
    let key = format!(" {name}=\"");
    let from = tag.find(&key).map_or(0, |at| at + key.len());
    if from == 0 {
        return String::new();
    }
    tag[from..].split('"').next().unwrap_or_default().to_owned()
}

/// The `Master` rows of the file's `masters.xml`, as the file says them (the text of each attribute, untouched).
fn rows(bytes: &[u8]) -> Vec<Row> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let part = pkg.parts().find(|p| p.ends_with("/masters/masters.xml")).expect("the file has a masters part").to_owned();
    let xml = String::from_utf8(pkg.part(&part).unwrap().to_vec()).unwrap();
    let mut out = Vec::new();
    let mut rest = xml.as_str();
    while let Some(at) = rest.find("<Master ") {
        let tag = &rest[at..];
        let end = tag.find('>').unwrap();
        let tag = &tag[..end];
        out.push(Row { name_u: attr(tag, "NameU"), unique: attr(tag, "UniqueID"), base: attr(tag, "BaseID") });
        rest = &rest[at + end..];
    }
    out
}

/// The first three groups of a GUID text (the whole text when it has fewer groups), in capitals: all the reference app keeps of a `UniqueID`.
fn prefix(guid: &str) -> String {
    let groups: Vec<&str> = guid.trim().split('-').collect();
    let first = if groups.len() >= 3 { groups[..3].join("-") } else { guid.trim().to_owned() };
    first.to_ascii_uppercase()
}

fn assert_distinct_prefixes(what: &str, rows: &[Row]) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for row in rows {
        assert!(!row.unique.is_empty(), "{what}: {} has no UniqueID", row.name_u);
        if let Some(other) = seen.insert(prefix(&row.unique), row.name_u.clone()) {
            panic!("{what}: {} and {other} read as one master in the reference app: {}", row.name_u, row.unique);
        }
    }
}

fn saved(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    run(s, "file.save_as", json!({ "path": dir.path(name) }));
    std::fs::read(dir.path(name)).unwrap()
}

/// A session whose drawing holds every master of `ids` (a drop of each by its id, as a person's drops add them).
fn session_with(ids: &[&str]) -> Session {
    let mut s = blank_session();
    for (n, id) in ids.iter().enumerate() {
        run(&mut s, "shape.drop", json!({"master": id, "page": 1, "x": 1.0 + n as f64, "y": 2.0}));
    }
    s
}

/// The drawing as a build before this fix saved it: each master's `UniqueID` is its `BaseID`.
fn as_an_old_build_saved_it(s: &mut Session) {
    s.edit("Old UniqueIDs", |doc| {
        for at in 0..doc.masters.len() {
            let master = Arc::make_mut(doc.masters.get_mut(at).unwrap());
            let base = master.raw.get("BaseID").map(str::to_owned);
            if let Some(base) = base {
                master.unique_id = Some(Arc::from(base.as_str()));
            }
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_stencil_file_of_each_built_in_stencil_holds_masters_whose_unique_ids_differ_in_their_first_three_groups() {
    let dir = Dir::new();
    // Every master written, by its BaseID (a hidden line end that two stencils' connectors bring is one master).
    let mut all: BTreeMap<String, Row> = BTreeMap::new();
    for stencil in stencils() {
        let mut s = blank_session();
        s.edit("All masters", |doc| {
            for master in stencil.masters() {
                ensure_master(doc, master).unwrap();
            }
            Ok(())
        })
        .unwrap();
        let bytes = saved(&mut s, &dir, &format!("{}.vssx", stencil.id));
        let found = rows(&bytes);
        assert!(found.len() >= stencil.masters().len(), "{}: a master row for each master", stencil.id);
        assert_distinct_prefixes(stencil.id, &found);
        for row in &found {
            assert_ne!(row.unique, row.base, "{}: the UniqueID is not the BaseID's text: {}", stencil.id, row.name_u);
            assert!(row.base.starts_with("{D1AC"), "{}: the BaseID is the fixed identity: {}", stencil.id, row.base);
        }
        for row in found {
            all.insert(row.base.clone(), row);
        }
    }
    // Across stencils too: a drawing that holds masters of several of them (a drop from each) tells every one apart.
    assert!(all.len() > 150, "the whole catalogue: {}", all.len());
    assert_distinct_prefixes("every stencil", &all.values().cloned().collect::<Vec<_>>());
}

#[test]
fn two_drawings_with_different_built_in_masters_at_the_same_place_write_unique_ids_that_differ_in_their_first_three_groups() {
    let dir = Dir::new();
    let (mut one, mut two) = (session_with(&["process"]), session_with(&["decision"]));
    let (a, b) = (rows(&saved(&mut one, &dir, "one.vsdx")), rows(&saved(&mut two, &dir, "two.vsdx")));
    assert_eq!((a.len(), b.len()), (1, 1), "the one master each drawing's drop made");
    assert_eq!((a[0].name_u.as_str(), b[0].name_u.as_str()), ("Process", "Decision"));
    assert_ne!(prefix(&a[0].unique), prefix(&b[0].unique), "{} {}", a[0].unique, b[0].unique);
    assert_ne!(a[0].base, b[0].base);
}

#[test]
fn the_same_built_in_master_in_two_new_drawings_has_the_same_unique_id_and_base_id() {
    let dir = Dir::new();
    let (mut one, mut two) = (session_with(&["rectangle"]), session_with(&["ellipse", "rectangle"]));
    let (a, b) = (rows(&saved(&mut one, &dir, "one.vsdx")), rows(&saved(&mut two, &dir, "two.vsdx")));
    let again = b.iter().find(|r| r.name_u == "Rectangle").expect("the second drawing has a Rectangle");
    assert_eq!(&a[0], again, "the same text in both files");
    assert_eq!(a[0].base, "{D1AC0001-0000-4000-8000-000000000001}", "the BaseID keeps the text the master had");
    assert_ne!(a[0].unique, a[0].base);
}

/// A file another program wrote, whose two masters have one first three groups (as the reference app writes them after it has read a file of DiaCraft's).
fn foreign() -> Vec<u8> {
    let sheet = |w: f64| shape(5, &box_cells(w, 1.0));
    VsdxBuilder::new()
        .master(
            MasterSpec::new(1, "Box")
                .attrs(r#" UniqueID="{AB000001-0000-4000-8E40-00608CF305B2}" BaseID="{BA5E0001-1111-4222-8333-444444444444}""#)
                .shapes(&sheet(1.0)),
        )
        .master(
            MasterSpec::new(2, "Tall")
                .attrs(r#" UniqueID="{AB000001-0000-4000-8E40-00608CF305B3}" BaseID="{ba5e0002-1111-4222-8333-444444444444}""#)
                .shapes(&sheet(2.0)),
        )
        .page(PageSpec::new(1, "Page-1").shapes(&shape(7, &box_cells(2.0, 1.0))))
        .build()
}

#[test]
fn a_file_from_another_program_keeps_every_master_id_it_had_when_saved_unedited_and_edited() {
    let dir = Dir::new();
    let first = rows(&foreign());
    assert_eq!(first.len(), 2);
    // Saved as another file (written from the drawing), unedited.
    let mut as_copy = Session::new();
    run(&mut as_copy, "file.open", json!({ "path": dir.write("foreign.vsdx", &foreign()) }));
    assert_eq!(rows(&saved(&mut as_copy, &dir, "copy.vsdx")), first, "a Save As of the drawing as it was");
    // Saved over its own file (spliced into it): unedited, after a move, and after a drop that adds a master of DiaCraft's.
    let path = dir.write("own.vsdx", &foreign());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    run(&mut s, "file.save", json!({}));
    assert_eq!(rows(&std::fs::read(&path).unwrap()), first, "a Save with no edit");
    run(&mut s, "shape.move", json!({"ids": [7], "page": 1, "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    assert_eq!(rows(&std::fs::read(&path).unwrap()), first, "a Save after a move");
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 5.0}));
    run(&mut s, "file.save", json!({}));
    let spliced = rows(&std::fs::read(&path).unwrap());
    assert_eq!(spliced.len(), 3, "the master the drop added is in the file: {spliced:?}");
    assert_eq!(&spliced[..2], &first[..], "a Save after the drop: the file's own masters, as they were");
    assert_ne!(prefix(&spliced[2].unique), prefix(&first[0].unique), "the one added is told apart from the file's");
    assert_ne!(spliced[2].unique, spliced[2].base);
    // The same drawing written from the model gives the added master the same ids as the splice does.
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.write("again.vsdx", &foreign()) }));
    run(&mut again, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 5.0}));
    assert_eq!(rows(&saved(&mut again, &dir, "dropped.vsdx")), spliced, "a Save As gives the added master the ids a Save does");
}

/// The `UniqueID` of the master called `name` in the model of a session.
fn model_unique(s: &Session, name: &str) -> Option<String> {
    s.document().unwrap().masters.iter().find(|m| m.name_u.as_deref() == Some(name)).and_then(|m| m.unique_id.as_deref().map(str::to_owned))
}

#[test]
fn a_drawing_of_0_2_3_is_found_by_its_base_ids_while_it_is_open_and_every_master_has_an_id_of_its_own_once_it_is_saved() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("old.vsdx", &old_form_file(false)) }));
    // Opened in this build: a drop of a master the drawing holds uses it (no second master), the ids it holds are as the file had them, and a drop of another adds one with the new form.
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 4.0, "y": 4.0}));
    run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 6.0, "y": 4.0}));
    assert_eq!(s.document().unwrap().masters.len(), 6, "two drops of masters the drawing held add none");
    assert_eq!((model_unique(&s, "Rectangle").as_deref(), model_unique(&s, "Process").as_deref()), (Some(RECTANGLE), Some(PROCESS)), "as read");
    run(&mut s, "shape.drop", json!({"master": "decision", "page": 1, "x": 8.0, "y": 4.0}));
    let decision = model_unique(&s, "Decision").expect("a master was added");
    assert_ne!(
        Some(decision.as_str()),
        s.document().unwrap().masters.iter().next_back().and_then(|m| m.raw.get("BaseID")),
        "the new master has the new form"
    );
    // Saved: the three of 0.2.3 and the new one have an id of their own, and the file's other masters are as they were.
    let saved_rows = rows(&saved(&mut s, &dir, "after.vsdx"));
    assert_eq!(saved_rows.len(), 7);
    let old = rows(&old_form_file(false));
    assert_repaired("the drawing after the drops", &saved_rows[..6], &old);
    assert_eq!(saved_rows[6].unique, decision, "the master the drop added keeps the id it was made with");
    let keys: std::collections::BTreeSet<String> = [0, 1, 2, 6].iter().map(|at| prefix(&saved_rows[*at].unique)).collect();
    assert_eq!(keys.len(), 4, "told apart in the first three groups: {saved_rows:?}");
}

#[test]
fn a_drop_from_an_opened_stencil_of_either_form_finds_the_master_the_drawing_holds_by_its_base_id() {
    let dir = Dir::new();
    // A stencil file as 0.2.3 wrote it, and one as this build does, each with the Rectangle.
    let old_stencil = dir.write("old.vssx", &old_form_file(true));
    let new_stencil = saved(&mut session_with(&["rectangle"]), &dir, "new.vssx");
    assert_ne!(rows(&new_stencil)[0].unique, rows(&new_stencil)[0].base);
    for (stencil_name, drawing_old) in [("old.vssx", false), ("new.vssx", false), ("old.vssx", true), ("new.vssx", true)] {
        let mut s = Session::new();
        if drawing_old {
            run(&mut s, "file.open", json!({ "path": dir.write("held.vsdx", &old_form_file(false)) }));
        } else {
            s = session_with(&["rectangle"]);
        }
        let (held, unique) = (s.document().unwrap().masters.len(), model_unique(&s, "Rectangle"));
        assert!(unique.is_some());
        let path = if stencil_name == "old.vssx" { old_stencil.clone() } else { dir.path(stencil_name) };
        run(&mut s, "stencil.open", json!({ "path": path }));
        run(&mut s, "shape.drop", json!({"master": "Rectangle", "stencil": "file-1", "page": 1, "x": 5.0, "y": 5.0}));
        let what = format!("{stencil_name} into a drawing of the {} form", if drawing_old { "old" } else { "new" });
        assert_eq!(s.document().unwrap().masters.len(), held, "{what}: the master the drawing holds is used");
        assert_eq!(model_unique(&s, "Rectangle"), unique, "{what}: and it keeps its id");
    }
}

#[test]
fn a_stencil_file_of_another_program_whose_masters_share_three_groups_is_opened_as_it_is() {
    let dir = Dir::new();
    let path = dir.write(
        "foreign.vssx",
        &VsdxBuilder::new()
            .document_type(CT_STENCIL)
            .master(
                MasterSpec::new(1, "Box")
                    .attrs(r#" UniqueID="{AB000001-0000-4000-8E40-00608CF305B2}" BaseID="{BA5E0001-1111-4222-8333-444444444444}""#)
                    .shapes(&shape(5, &box_cells(1.0, 1.0))),
            )
            .build(),
    );
    let mut s = blank_session();
    run(&mut s, "stencil.open", json!({ "path": path }));
    run(&mut s, "shape.drop", json!({"master": "Box", "stencil": "file-1", "page": 1, "x": 3.0, "y": 3.0}));
    let found = rows(&saved(&mut s, &dir, "out.vsdx"));
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].unique.as_str(), found[0].base.as_str()),
        ("{AB000001-0000-4000-8E40-00608CF305B2}", "{BA5E0001-1111-4222-8333-444444444444}"),
        "a master brought in keeps its ids"
    );
}

// ---- the repair on save (owner ruling 2026-11-28) -------------------------------------------------------------------------------------

const RECTANGLE: &str = "{D1AC0001-0000-4000-8000-000000000001}";
const SQUARE: &str = "{D1AC0001-0000-4000-8000-000000000002}";
const PROCESS: &str = "{D1AC0002-0000-4000-8000-000000000001}";
const ELLIPSE: &str = "{D1AC0001-0000-4000-8000-000000000003}";

/// A file as 0.2.3 wrote one (and the first builds of 0.2.4): the built-in masters' `UniqueID` is their `BaseID`'s text, whose first three groups the VSDX application takes for one. After the three of
/// DiaCraft's, three masters that are not: one that has passed through that application (its last two groups are its own), one that holds a built-in text as its `UniqueID` and another `BaseID`, and one
/// of another program. The page holds one instance of the first master. Built by hand: a save of this build would repair it.
fn old_form_file(stencil: bool) -> Vec<u8> {
    let rows: [(&str, &str, &str); 6] = [
        ("Rectangle", RECTANGLE, RECTANGLE),
        ("Square", SQUARE, SQUARE),
        ("Process", PROCESS, PROCESS),
        ("Passed", "{D1AC0001-0000-4000-8E40-00608CF305B2}", "{D1AC0001-0000-4000-8E40-00608CF305B2}"),
        ("Lookalike", ELLIPSE, "{BA5E0004-1111-4222-8333-444444444444}"),
        ("Other", "{CC000001-0000-4000-8000-000000000001}", "{CC000002-0000-4000-8000-000000000001}"),
    ];
    let mut b = VsdxBuilder::new();
    if stencil {
        b = b.document_type(CT_STENCIL);
    }
    for (n, (name, unique, base)) in rows.iter().enumerate() {
        let id = u32::try_from(n + 1).unwrap();
        b = b.master(MasterSpec::new(id, name).attrs(&format!(r#" UniqueID="{unique}" BaseID="{base}""#)).shapes(&shape(5, &box_cells(1.0, 1.0))));
    }
    b.page(PageSpec::new(1, "Page-1").shapes(&shape_with(7, r#" Master="1""#, &box_cells(1.0, 1.0)))).build()
}

/// The `UniqueID` a master gets today, by name: what the repair must write for the three built-in masters of [`old_form_file`].
fn today() -> BTreeMap<String, String> {
    let dir = Dir::new();
    let mut s = session_with(&["rectangle", "square", "process"]);
    rows(&saved(&mut s, &dir, "today.vsdx")).into_iter().map(|r| (r.name_u, r.unique)).collect()
}

/// A saved file holds what the old one did, but for the `UniqueID` of the built-in masters of 0.2.3, which are the ones of today (`BaseID`s as they were, and every other master as it was).
#[track_caller]
fn assert_repaired(what: &str, got: &[Row], old: &[Row]) {
    let wanted = today();
    assert_eq!(got.len(), old.len(), "{what}: the same masters");
    for (g, o) in got.iter().zip(old) {
        assert_eq!((&g.name_u, &g.base), (&o.name_u, &o.base), "{what}: the BaseID is kept");
        match wanted.get(&o.name_u) {
            Some(new) => {
                assert_eq!(&g.unique, new, "{what}: {} has the UniqueID a master has today", o.name_u);
                assert_ne!(g.unique, o.unique, "{what}: {} is no more the old text", o.name_u);
            }
            None => assert_eq!(g.unique, o.unique, "{what}: {} is no master of the built-in stencils and keeps its UniqueID as written", o.name_u),
        }
    }
    assert_distinct_prefixes(what, &got.iter().filter(|r| wanted.contains_key(&r.name_u)).cloned().collect::<Vec<_>>());
}

#[test]
fn the_file_of_0_2_3_holds_what_the_tests_below_assume() {
    let old = rows(&old_form_file(false));
    assert_eq!(old.iter().map(|r| r.name_u.as_str()).collect::<Vec<_>>(), ["Rectangle", "Square", "Process", "Passed", "Lookalike", "Other"]);
    assert!(old[..3].iter().all(|r| r.unique == r.base), "the built-in three have the text of their BaseID as their UniqueID");
    assert_eq!(prefix(&old[0].unique), prefix(&old[1].unique), "and the VSDX application takes the first two for one master");
    assert_eq!(today().len(), 3);
}

#[test]
fn a_save_into_the_file_of_0_2_3_repairs_the_unique_ids_of_its_built_in_masters_only_edited_or_not() {
    let dir = Dir::new();
    let old = rows(&old_form_file(false));
    for edited in [false, true] {
        let path = dir.write(&format!("own-{edited}.vsdx"), &old_form_file(false));
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": path }));
        if edited {
            run(&mut s, "shape.move", json!({"ids": [7], "page": 1, "dx": 1.0, "dy": 0.5}));
        }
        run(&mut s, "file.save", json!({}));
        let first = rows(&std::fs::read(&path).unwrap());
        assert_repaired(&format!("a Save, edited {edited}"), &first, &old);
        // Saved again after an edit: the same ids (the repair is done once).
        run(&mut s, "shape.move", json!({"ids": [7], "page": 1, "dx": 0.5, "dy": 0.5}));
        run(&mut s, "file.save", json!({}));
        assert_eq!(rows(&std::fs::read(&path).unwrap()), first, "a second Save, edited {edited}");
    }
}

#[test]
fn a_save_as_of_the_file_of_0_2_3_to_a_vsdx_a_vssx_and_a_vstx_repairs_them_too() {
    let dir = Dir::new();
    let old = rows(&old_form_file(false));
    for name in ["as.vsdx", "as.vssx", "as.vstx"] {
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": dir.write("from.vsdx", &old_form_file(false)) }));
        assert_repaired(name, &rows(&saved(&mut s, &dir, name)), &old);
    }
    // A stencil file of 0.2.3, saved as a stencil.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("from.vssx", &old_form_file(true)) }));
    assert_repaired("a stencil of 0.2.3", &rows(&saved(&mut s, &dir, "again.vssx")), &old);
}

#[test]
fn the_model_writer_repairs_a_drawing_of_0_2_3_that_went_through_dot_diacraft_and_one_made_in_memory() {
    let dir = Dir::new();
    let old = rows(&old_form_file(false));
    for name in ["model.vsdx", "model.vssx"] {
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": dir.write("from.vsdx", &old_form_file(false)) }));
        run(&mut s, "file.save_as", json!({ "path": dir.path("mid.diacraft") }));
        assert_repaired(name, &rows(&saved(&mut s, &dir, name)), &old);
    }
    // A drawing a session holds in the old form (its masters as 0.2.3 made them) and saves for the first time.
    for name in ["new.vsdx", "new.vssx"] {
        let mut s = session_with(&["rectangle", "square", "process"]);
        as_an_old_build_saved_it(&mut s);
        let got = rows(&saved(&mut s, &dir, name));
        let wanted = today();
        assert_eq!(got.len(), 3);
        for r in &got {
            assert_eq!(r.unique, wanted[&r.name_u], "{name}: {}", r.name_u);
            assert_ne!(r.unique, r.base);
        }
    }
}

#[test]
fn a_repaired_drawing_finds_its_masters_by_the_base_id_when_it_is_opened_dropped_on_and_copied_from() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("from.vsdx", &old_form_file(false)) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("repaired.vsdx") }));
    let repaired = rows(&std::fs::read(dir.path("repaired.vsdx")).unwrap());
    // Opened again: a drop of each built-in master the file holds adds none and changes none.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": dir.path("repaired.vsdx") }));
    for (n, master) in ["rectangle", "square", "process"].iter().enumerate() {
        run(&mut t, "shape.drop", json!({"master": master, "page": 1, "x": 2.0 + n as f64, "y": 5.0}));
    }
    assert_eq!(t.document().unwrap().masters.len(), 6, "no master was added: the six of the file");
    // A copy and a paste of the shape that is an instance of the first master uses that master.
    run(&mut t, "edit.copy", json!({"ids": [7]}));
    run(&mut t, "edit.paste", json!({}));
    assert_eq!(t.document().unwrap().masters.len(), 6, "a paste of an instance adds no master");
    run(&mut t, "file.save_as", json!({ "path": dir.path("again.vsdx") }));
    assert_eq!(rows(&std::fs::read(dir.path("again.vsdx")).unwrap()), repaired, "and a save of it holds the ids it had");
    // The drawing of 0.2.3 itself, not saved: a drop of a built-in master finds it by its BaseID, and the ids it holds are as it read them until a save repairs them.
    let mut u = Session::new();
    run(&mut u, "file.open", json!({ "path": dir.write("from2.vsdx", &old_form_file(false)) }));
    run(&mut u, "shape.drop", json!({"master": "square", "page": 1, "x": 5.0, "y": 5.0}));
    let doc = u.document().unwrap();
    assert_eq!(doc.masters.len(), 6);
    assert!(doc.masters.iter().take(3).all(|m| m.unique_id.as_deref() == m.raw.get("BaseID")), "read as written");
}

#[test]
fn a_stencil_of_0_2_3_opened_beside_a_repaired_drawing_drops_the_masters_the_drawing_holds() {
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.write("from.vsdx", &old_form_file(false)) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("repaired.vsdx") }));
    run(&mut s, "stencil.open", json!({ "path": dir.write("old.vssx", &old_form_file(true)) }));
    run(&mut s, "shape.drop", json!({"master": "Rectangle", "stencil": "file-1", "page": 1, "x": 5.0, "y": 5.0}));
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 7.0, "y": 5.0}));
    assert_eq!(s.document().unwrap().masters.len(), 6, "the drawing's own Rectangle and Process were used");
}
