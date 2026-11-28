//! A halving after a save and a reopen removes the same points it would have removed before the save (design §11.4), in VSDX and `.diacraft`; and a file whose
//! marker was taken out has originals only and nothing to halve. The `.odg` writer for changed points is CPDOUBLE.ODG's (its tests are `conn_points_files`): what is held here
//! of `.odg` is only that the marker, which no `.odg` of ours wrote before, does not stop a drawing being saved and read again.

use serde_json::json;

use crate::conn_points::{act, marker, params, places, rect, refused, v21_b};
use crate::edit::doc;
use crate::native_common::{Dir, opened, pinned};
use crate::run;

/// Saves the drawing of `s` as `name.<ext>` and opens it in a session of its own.
fn reopened(s: &mut diacraft_engine::Session, ext: &str) -> diacraft_engine::Session {
    let dir = Dir::new();
    run(s, "file.save_as", json!({"path": dir.path(&format!("doubled.{ext}"))}));
    let again = opened(&dir.path(&format!("doubled.{ext}")));
    // The session keeps no folder alive: the file has been read.
    drop(dir);
    again
}

#[test]
fn a_halving_after_a_save_and_a_reopen_removes_the_same_points_in_vsdx_and_diacraft() {
    for ext in ["diacraft", "vsdx"] {
        let (mut first, a) = v21_b();
        // What the halving of the top takes before the save, on a drawing made the same way and never saved.
        let (mut control, _) = v21_b();
        act(&mut control, a, "halve", json!({"edge": 2}));
        let want = places(&control, a);
        let mut second = reopened(&mut first, ext);
        assert_eq!(marker(&second, a), marker(&first, a), "{ext}: the marker reads the same after the reopen");
        assert_eq!(places(&second, a), places(&first, a), "{ext}: the rows are the same");
        act(&mut second, a, "halve", json!({"edge": 2}));
        assert_eq!(places(&second, a), want, "{ext}: the halving after the reopen takes the same points");
        // And every halving after it, down to the originals, as before the save.
        act(&mut second, a, "halve", json!({}));
        assert_eq!(places(&second, a).len(), 4, "{ext}");
        let words = refused(&mut second, &params(a, "halve", json!({})));
        assert!(words.contains("nothing to halve"), "{ext}: {words}");
    }
}

#[test]
fn a_file_whose_marker_was_taken_out_has_originals_only_and_nothing_to_halve() {
    for ext in ["diacraft", "vsdx"] {
        let mut s = pinned();
        let id = rect(&mut s, 2.0, 1.0);
        act(&mut s, id, "double", json!({}));
        let mut again = reopened(&mut s, ext);
        assert_eq!(places(&again, id).len(), 12, "{ext}");
        // A program that drops the marker leaves the points as they are, every one an original.
        run(&mut again, "sheet.delete_row", json!({"id": id, "section": "User", "row": "DiaCraftAddedPoints"}));
        assert_eq!(marker(&again, id), Vec::<(u32, u8)>::new(), "{ext}");
        let words = refused(&mut again, &params(id, "halve", json!({})));
        assert!(words.contains("nothing to halve"), "{ext}: {words}");
        assert_eq!(places(&again, id).len(), 12, "{ext}: nothing was removed");
    }
}

#[test]
fn a_doubled_shape_saved_as_odg_is_written_and_read_again() {
    // The marker is a `User` row of the shape: the save note does not call it shape data, and the drawing reopens with the rows it had.
    let mut s = pinned();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    let dir = Dir::new();
    let saved = run(&mut s, "file.save_as", json!({"path": dir.path("doubled.odg")}));
    let notes = saved["notes"].as_array().cloned().unwrap_or_default();
    assert!(notes.iter().all(|n| n["shape_data"] == 0), "the marker of doubled points is no shape data: {notes:?}");
    let again = opened(&dir.path("doubled.odg"));
    assert!(doc(&again).pages.get(0).is_some_and(|p| p.shapes.iter().next().is_some()));
    // The marker and the six points ride in DiaCraft's own data of the file (tier 2), so the drawing reads back as it was saved.
    assert_eq!(places(&again, id).len(), 6);
    assert_eq!(crate::conn_points::marker_text(&again, id).as_deref(), Some("0*(1*Connections.X5+1*Connections.X6)"));
}
