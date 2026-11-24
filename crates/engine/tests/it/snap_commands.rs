//! The snap commands: the settings and how they are saved, the `snap` parameter of the editing commands, and guides.
//!
//! Lengths are paper inches. The numbers are those of `snap.rs` (the function itself); here the commands are asked what they do with it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::snap::{Home, OPTIONS, Word};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use diacraft_doc::{Document, RowKey, ShapeType};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::{blank_session, check_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-snap-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The session's state but for its journal: a command that changes nothing is journaled all the same, and leaves the document, the history and the
/// revision as they were.
fn unjournaled(s: &Session) -> crate::Snapshot {
    crate::Snapshot { journal: Vec::new(), ..snapshot(s) }
}

fn near(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-6)
}

fn pin(s: &Session, id: u32) -> (f64, f64) {
    let doc = s.document().unwrap();
    let shape = doc.page(1).unwrap().shapes.iter().find(|sh| sh.id == id).unwrap().clone();
    let n = |name: &str| shape.sheet.cell(name).unwrap().value.as_ref().unwrap().number().unwrap();
    (n("PinX"), n("PinY"))
}

// -------------------------------------------------------------------------------------------------------------------- settings

#[test]
fn a_new_drawing_has_the_default_settings_and_the_words_a_vsdx_file_has() {
    // The defaults of the table, and `file.new` writes the four words at them (65847, 9, 34 and 1).
    // A document that holds no words reads as the defaults too (the unit tests of `snap/settings.rs`), and the model's own `Document::new_blank` holds none.
    assert_eq!(diacraft_doc::Document::new_blank().snap_words(), diacraft_doc::SnapWords::default());
    let mut s = Session::new();
    let new = run(&mut s, "file.new", json!({}));
    assert!(new.is_object());
    let r = run(&mut s, "snap.settings", json!({}));
    assert_eq!(r["words"], json!({"SnapSettings": "65847", "GlueSettings": "9", "SnapExtensions": "34", "DynamicGridEnabled": "1"}), "{r}");
    for option in OPTIONS {
        assert_eq!(r[option.name], json!(option.default), "{}", option.name);
    }
    for on in [
        "snap",
        "glue",
        "ruler",
        "grid",
        "guides",
        "snap_connection_points",
        "snap_vertices",
        "snap_geometry",
        "intersections",
        "glue_connection_points",
        "glue_guides",
        "dynamic_grid",
    ] {
        assert_eq!(r[on], json!(true), "{on}");
    }
    for off in ["alignment_box", "snap_handles", "glue_geometry", "glue_handles", "glue_vertices", "ext_isometric"] {
        assert_eq!(r[off], json!(false), "{off}");
    }
    // The new drawing is not changed by being asked, and a template's drawing has the words too.
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], json!(false));
    let templated = run(&mut s, "file.new", json!({"template": "basic_diagram"}));
    assert!(templated.is_object(), "{templated}");
    assert_eq!(run(&mut s, "snap.settings", json!({}))["words"]["SnapSettings"], json!("65847"));
}

#[test]
fn a_new_drawing_saved_holds_the_four_words_and_no_part_and_opens_with_the_defaults() {
    let dir = Dir::new();
    let path = dir.path("new.vsdx");
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let xml = part_text(&path, "/visio/document.xml");
    for word in [
        "<GlueSettings>9</GlueSettings>",
        "<SnapSettings>65847</SnapSettings>",
        "<SnapExtensions>34</SnapExtensions>",
        "<DynamicGridEnabled>1</DynamicGridEnabled>",
    ] {
        assert!(xml.contains(word), "{word}: {xml}");
    }
    // Between the extensions and the dynamic grid is the empty `SnapAngles` that every sample holds.
    assert!(xml.contains("<SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled>"), "{xml}");
    let bytes = std::fs::read(&path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    assert!(pkg.parts().all(|p| !p.contains("solution")), "the defaults need no part: {:?}", pkg.parts().collect::<Vec<_>>());
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let r = run(&mut t, "snap.settings", json!({}));
    for option in OPTIONS {
        assert_eq!(r[option.name], json!(option.default), "{}", option.name);
    }
    // Saved again with nothing changed, and then with one option changed in place: the words are patched in the bytes, and the types stay.
    let types = types_bytes(&path);
    run(&mut t, "snap.set", json!({"ext_curved": true}));
    run(&mut t, "file.save", json!({}));
    assert_eq!(part_text(&path, "/visio/document.xml"), xml.replace("<SnapExtensions>34<", "<SnapExtensions>98<"));
    assert_eq!(types_bytes(&path), types);
}

/// The Snap & Glue options are View settings, "unsaved, not undo": `snap.set` makes no undo step, and Ctrl+Z leaves them as they are set.
#[test]
fn changing_the_settings_is_no_undo_step_and_naming_nothing_is_no_change() {
    let mut s = blank_session();
    let before = s.history().unwrap().undo_len();
    assert_eq!(before, 0, "a blank session has no step");
    let r = run(&mut s, "snap.set", json!({"grid": false, "alignment_box": true, "dynamic_grid": true, "snap": false}));
    assert_eq!(
        (r["grid"].clone(), r["alignment_box"].clone(), r["snap"].clone(), r["dynamic_grid"].clone()),
        (json!(false), json!(true), json!(false), json!(true))
    );
    assert_eq!(s.history().unwrap().undo_len(), before, "no undo step");
    assert!(s.is_dirty(), "but the drawing is changed: a save keeps the options");
    // The grid and the alignment box are flags of the file's `SnapSettings`: a new drawing's 65847 less the grid (2) and with the alignment box (512);
    // the Snap switch is the part's; the dynamic grid did not change, and no other word is touched.
    assert_eq!(r["words"], json!({"SnapSettings": "66357", "GlueSettings": "9", "SnapExtensions": "34", "DynamicGridEnabled": "1"}), "{r}");
    // The same values again, and no options at all: no step, the drawing as it was.
    let kept = unjournaled(&s);
    run(&mut s, "snap.set", json!({"grid": false, "dynamic_grid": true}));
    run(&mut s, "snap.set", json!({}));
    assert_eq!(unjournaled(&s), kept, "nothing changed, nothing recorded");
    // Undo has nothing to take back, and the words stay.
    let refused = s.execute("edit.undo", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("nothing to undo"), "{refused}");
    assert_eq!(run(&mut s, "snap.settings", json!({}))["words"]["SnapSettings"], json!("66357"));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["grid"], json!(false));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn bad_settings_are_refused_and_change_nothing() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [json!({"grid": "yes"}), json!({"snap": 1}), json!({"nope": true}), json!([true])] {
        let err = s.execute("snap.set", &params).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{params}: {err}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn the_settings_are_saved_in_the_file_and_come_back() {
    let dir = Dir::new();
    let path = dir.path("settings.vsdx");
    let mut s = blank_session();
    run(
        &mut s,
        "snap.set",
        json!({"grid": false, "ruler": true, "guides": false, "glue_connection_points": false, "ext_isometric": true, "dynamic_grid": false, "snap": false}),
    );
    let want = run(&mut s, "snap.settings", json!({}));
    run(&mut s, "file.save_as", json!({"path": path}));
    // Every option but a switch is a flag of its word in the file's own element: SnapSettings is 65847 less the grid (2) and guides (4); GlueSettings is 9
    // less connection points (8); SnapExtensions is 34 with the isometric lines (4096); the dynamic grid is 0. The switch is DiaCraft's part.
    let xml = part_text(&path, "/visio/document.xml");
    for word in [
        "<SnapSettings>65841</SnapSettings>",
        "<GlueSettings>1</GlueSettings>",
        "<SnapExtensions>4130</SnapExtensions>",
        "<DynamicGridEnabled>0</DynamicGridEnabled>",
    ] {
        assert!(xml.contains(word), "{word}: {xml}");
    }
    let solution = part_text(&path, "/visio/solutions/solution1.xml");
    assert!(solution.contains("dc:snap") && solution.contains(r#"snap="0""#) && solution.contains(r#"glue="1""#), "{solution}");
    assert!(!solution.contains("dynamic_grid") && !solution.contains("guides"), "the part holds the switches only: {solution}");
    // A new session opens it and reads the same.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(run(&mut t, "snap.settings", json!({})), want);
    // And a second change, saved over it, is patched.
    run(&mut t, "snap.set", json!({"grid": true, "glue": false}));
    run(&mut t, "file.save", json!({}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": path}));
    assert_eq!(run(&mut u, "snap.settings", json!({}))["grid"], json!(true));
    assert_eq!(run(&mut u, "snap.settings", json!({}))["snap"], json!(false));
    assert_eq!(run(&mut u, "snap.settings", json!({}))["glue"], json!(false));
    assert!(part_text(&path, "/visio/document.xml").contains("<SnapSettings>65843</SnapSettings>"));
}

/// The text of a part of the package at `path`.
fn part_text(path: &str, name: &str) -> String {
    let bytes = std::fs::read(path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(pkg.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// A drawing whose document part holds the three words of `DocumentSettings` as another program might write them.
fn drawing_with_words() -> Vec<u8> {
    let settings = concat!(
        r#"<DocumentSettings TopPage="0" DefaultTextStyle="0" DefaultLineStyle="0" DefaultFillStyle="0" DefaultGuideStyle="0">"#,
        r#"<GlueSettings>9</GlueSettings><SnapSettings>65847</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/>"#,
        r#"<DynamicGridEnabled>1</DynamicGridEnabled></DocumentSettings>"#
    );
    diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "A").shapes(&diacraft_testkit::package::shape(1, r#"<Cell N="Width" V="2.50"/>"#)))
        .document_extra(settings)
        .build()
}

#[test]
fn each_option_changes_its_flag_of_its_word_and_the_rest_of_the_document_part_byte_for_byte() {
    // Every option document is changed on a file that holds the sample words, the drawing saved, and the document part is the original's
    // with that word's text replaced: the other three words, the empty `SnapAngles`, the element's attributes, and every other byte are as they were. The
    // content types stream, which has no row to change, is the file's own bytes. The switches are the part's, and the document part does not move for them.
    let dir = Dir::new();
    let path = dir.path("words.vsdx");
    std::fs::write(&path, drawing_with_words()).unwrap();
    let original = part_text(&path, "/visio/document.xml");
    let types = types_bytes(&path);
    let words = [("SnapSettings", 65847i64), ("GlueSettings", 9), ("SnapExtensions", 34)];
    let mut tried = 0;
    for (index, option) in OPTIONS.iter().enumerate() {
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": path}));
        let was = run(&mut s, "snap.settings", json!({}))[option.name].as_bool().unwrap();
        run(&mut s, "snap.set", json!({option.name: !was}));
        let copy = dir.path(&format!("{}.vsdx", option.name));
        run(&mut s, "file.save_as", json!({"path": copy}));
        let after = part_text(&copy, "/visio/document.xml");
        match option.home {
            Home::Part => {
                assert_eq!(after, original, "{}: a switch is the part's, and moves no word", option.name);
                let solution = part_text(&copy, "/visio/solutions/solution1.xml");
                assert!(solution.contains(&format!(r#"{}="{}""#, option.name, u8::from(!was))), "{}: {solution}", option.name);
            }
            Home::Word(Word::Dynamic) => {
                assert_eq!(
                    after,
                    original.replace("<DynamicGridEnabled>1</DynamicGridEnabled>", "<DynamicGridEnabled>0</DynamicGridEnabled>"),
                    "{}",
                    option.name
                );
            }
            Home::Word(word) => {
                let (element, value) = words.iter().find(|(e, _)| *e == word.element()).copied().unwrap();
                let changed = value ^ option.flag;
                assert_eq!(
                    after,
                    original.replace(&format!("<{element}>{value}</{element}>"), &format!("<{element}>{changed}</{element}>")),
                    "{}: only {element} changed, by flag {}",
                    option.name,
                    option.flag
                );
                let bytes = std::fs::read(&copy).unwrap();
                let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
                assert!(pkg.parts().all(|p| !p.contains("solution")), "{}: a flag of a word needs no part", option.name);
            }
        }
        if matches!(option.home, Home::Word(_)) {
            assert_eq!(types_bytes(&copy), types, "{}: the content types are the file's own bytes", option.name);
        } else {
            // A switch makes the part: its rows are added after the file's, whose bytes stay.
            let (before, now) = (String::from_utf8(types.clone()).unwrap(), String::from_utf8(types_bytes(&copy)).unwrap());
            assert!(now.starts_with(&before[..before.rfind("</Types>").unwrap()]), "{}: {now}", option.name);
        }
        let mut t = Session::new();
        run(&mut t, "file.open", json!({"path": copy}));
        assert_eq!(run(&mut t, "snap.settings", json!({}))[option.name], json!(!was), "{} (option {index}) comes back", option.name);
        tried += 1;
    }
    assert_eq!(tried, OPTIONS.len());
    // All of them at once, and then back: the document part is the original's byte for byte, and no choice, no part.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let was = run(&mut s, "snap.settings", json!({}));
    let flipped: serde_json::Map<String, Value> = OPTIONS.iter().map(|o| (o.name.to_owned(), json!(!was[o.name].as_bool().unwrap()))).collect();
    run(&mut s, "snap.set", Value::Object(flipped));
    let back: serde_json::Map<String, Value> = OPTIONS.iter().map(|o| (o.name.to_owned(), was[o.name].clone())).collect();
    run(&mut s, "snap.set", Value::Object(back));
    let copy = dir.path("back.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    assert_eq!(part_text(&copy, "/visio/document.xml"), original);
    let bytes = std::fs::read(&copy).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    assert!(pkg.parts().all(|p| !p.contains("solution")), "no choice, no part: {:?}", pkg.parts().collect::<Vec<_>>());
    assert_eq!(types_bytes(&copy), types);
}

/// The `[Content_Types].xml` bytes of the package at `path`.
fn types_bytes(path: &str) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap().content_types_bytes().to_vec()
}

#[test]
fn unknown_bits_and_odd_words_are_kept_as_read_through_a_change() {
    // A flag no page names (64 of SnapSettings), the flag read as off (32768), a bit past 32, and a negative word are kept; a word that is text is left
    // alone, and a change to one of its options is refused.
    let dir = Dir::new();
    let path = dir.path("odd.vsdx");
    let settings = concat!(
        r#"<DocumentSettings TopPage="0"><GlueSettings>32777</GlueSettings><SnapSettings>1099511693687</SnapSettings>"#,
        r#"<SnapExtensions>-1</SnapExtensions><SnapAngles/><DynamicGridEnabled>true</DynamicGridEnabled></DocumentSettings>"#
    );
    let build = |settings: &str| {
        diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "A")).document_extra(settings).build()
    };
    std::fs::write(&path, build(settings)).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let r = run(&mut s, "snap.settings", json!({}));
    // 1099511693687 = 2^40 + 65847 + 64: the sample flags with an unknown one and a bit past 32.
    assert_eq!(
        (r["ruler"].clone(), r["grid"].clone(), r["snap_geometry"].clone(), r["snap"].clone(), r["glue"].clone()),
        (json!(true), json!(true), json!(true), json!(true), json!(true))
    );
    assert_eq!(
        (r["ext_isometric"].clone(), r["ext_linear"].clone(), r["dynamic_grid"].clone()),
        (json!(true), json!(true), json!(true)),
        "-1 has every flag: {r}"
    );
    run(&mut s, "snap.set", json!({"grid": false, "glue_geometry": true, "ext_curved": false, "dynamic_grid": false}));
    let copy = dir.path("odd2.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    let xml = part_text(&copy, "/visio/document.xml");
    assert!(xml.contains("<SnapSettings>1099511693685</SnapSettings>"), "grid (2) cleared, 2^40 and 64 kept: {xml}");
    assert!(xml.contains("<GlueSettings>32809</GlueSettings>"), "32777 with flag 32 set; the flag 32768 kept: {xml}");
    assert!(xml.contains("<SnapExtensions>-65</SnapExtensions>"), "-1 with flag 64 cleared: {xml}");
    assert!(xml.contains("<DynamicGridEnabled>0</DynamicGridEnabled>"), "{xml}");
    // Text is not touched.
    std::fs::write(&path, build(&settings.replace("1099511693687", "lots").replace("true", "perhaps"))).unwrap();
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(run(&mut t, "snap.settings", json!({}))["grid"], json!(true), "the default");
    let kept = unjournaled(&t);
    for params in [json!({"grid": false}), json!({"dynamic_grid": false})] {
        let err = t.execute("snap.set", &params).unwrap_err();
        assert!(matches!(err, EngineError::Failed { .. }), "{params}: {err}");
        assert_eq!(unjournaled(&t), kept, "{params}");
    }
    run(&mut t, "snap.set", json!({"glue_vertices": true}));
}

#[test]
fn a_file_from_m323_with_only_diacraft_s_part_reads_its_values_and_a_change_moves_them_into_the_words() {
    // Kept every option but the ruler and the grid in the part, and made no other word. Its values are read where the word is absent.
    let dir = Dir::new();
    let path = dir.path("m323.vsdx");
    let solution = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap snap="0" glue="1" alignment_box="1" intersections="0" guides="0" glue_geometry="1" glue_handles="0" glue_vertices="1" glue_connection_points="0" glue_guides="0" dynamic_grid="0"/></dc:Data></SolutionXML>"#;
    let bytes =
        diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "A")).solution("DiaCraft", solution).build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let r = run(&mut s, "snap.settings", json!({}));
    for (option, value) in [
        ("snap", false),
        ("glue", true),
        ("alignment_box", true),
        ("intersections", false),
        ("guides", false),
        ("glue_geometry", true),
        ("glue_vertices", true),
        ("glue_connection_points", false),
        ("glue_guides", false),
        ("dynamic_grid", false),
    ] {
        assert_eq!(r[option], json!(value), "{option}: the part's value");
    }
    assert_eq!(r["grid"], json!(true), "an option the part never held is its default");
    assert_eq!(r["words"]["SnapSettings"], Value::Null);
    // A change of any option writes the child again with the switches only, and the values the part gave go into their words, so that none is lost.
    run(&mut s, "snap.set", json!({"grid": false}));
    let copy = dir.path("m323b.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    let solution = part_text(&copy, "/visio/solutions/solution1.xml");
    assert!(solution.contains(r#"snap="0""#) && solution.contains(r#"glue="1""#) && !solution.contains("alignment_box"), "{solution}");
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": copy}));
    let after = run(&mut t, "snap.settings", json!({}));
    for (option, value) in [
        ("snap", false),
        ("alignment_box", true),
        ("intersections", false),
        ("guides", false),
        ("glue_geometry", true),
        ("glue_guides", false),
        ("dynamic_grid", false),
        ("grid", false),
    ] {
        assert_eq!(after[option], json!(value), "{option} is where the file left it");
    }
    assert!(after["words"]["GlueSettings"].is_string() && after["words"]["DynamicGridEnabled"] == json!("0"), "{after}");
}

#[test]
fn a_file_with_both_the_part_and_the_words_reads_the_words_where_they_disagree() {
    let dir = Dir::new();
    let path = dir.path("both.vsdx");
    // The part says the opposite of every flag the words hold (SnapSettings 773, GlueSettings 9, boolean true); the switch is only the part's.
    let solution = r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap snap="0" glue="0" guides="0" alignment_box="0" intersections="1" glue_geometry="1" glue_guides="0" dynamic_grid="0"/></dc:Data></SolutionXML>"#;
    let settings = concat!(
        r#"<DocumentSettings TopPage="0"><GlueSettings>9</GlueSettings><SnapSettings>773</SnapSettings><SnapExtensions>34</SnapExtensions>"#,
        r#"<SnapAngles/><DynamicGridEnabled>true</DynamicGridEnabled></DocumentSettings>"#
    );
    let bytes = diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "A"))
        .document_extra(settings)
        .solution("DiaCraft", solution)
        .build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    let r = run(&mut s, "snap.settings", json!({}));
    for (option, value) in [
        ("guides", true),
        ("alignment_box", true),
        ("intersections", false),
        ("glue_geometry", false),
        ("glue_guides", true),
        ("dynamic_grid", true),
        ("snap", false),
        ("glue", false),
    ] {
        assert_eq!(r[option], json!(value), "{option}: the word wins where it is there; the switches have no word");
    }
    // A change of a switch writes the part again with the switches only and leaves the words' bytes.
    run(&mut s, "snap.set", json!({"glue": true}));
    let copy = dir.path("both2.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    let xml = part_text(&copy, "/visio/document.xml");
    assert!(
        xml.contains("<SnapSettings>773</SnapSettings>")
            && xml.contains("<GlueSettings>9</GlueSettings>")
            && xml.contains("<DynamicGridEnabled>true</DynamicGridEnabled>"),
        "{xml}"
    );
    let solution = part_text(&copy, "/visio/solutions/solution1.xml");
    assert!(solution.contains(r#"snap="0""#) && solution.contains(r#"glue="1""#) && !solution.contains("guides"), "{solution}");
}

#[test]
fn the_windows_part_s_copies_are_not_touched() {
    // A document SnapSettings of 773, a drawing window's of 17, and a command that turns the grid on: 775 in the document, 17 in the window,
    // and every other byte of both parts as it was.
    use diacraft_testkit::package::{Pkg, VIS};
    let dir = Dir::new();
    let path = dir.path("windows.vsdx");
    let settings = concat!(
        r#"<DocumentSettings TopPage="0"><GlueSettings>9</GlueSettings><SnapSettings>773</SnapSettings><SnapExtensions>34</SnapExtensions>"#,
        r#"<SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled></DocumentSettings>"#
    );
    let windows = r#"<?xml version="1.0" encoding="utf-8"?><Windows xmlns="http://schemas.microsoft.com/office/visio/2012/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" ClientWidth="1024" ClientHeight="768"><Window ID="0" WindowType="Drawing" WindowState="1073741824"><GlueSettings>9</GlueSettings><SnapSettings>17</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled></Window></Windows>"#;
    let bytes = diacraft_testkit::package::VsdxBuilder::new()
        .page(diacraft_testkit::package::PageSpec::new(1, "A"))
        .document_extra(settings)
        .with(Pkg::new().part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", windows).rel(
            "/visio/document.xml",
            "rId901",
            &format!("{VIS}windows"),
            "windows.xml",
        ))
        .build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["grid"], json!(false), "773 has no grid: the document's word is read, not the window's");
    run(&mut s, "snap.set", json!({"grid": true}));
    let copy = dir.path("windows2.vsdx");
    run(&mut s, "file.save_as", json!({"path": copy}));
    assert_eq!(part_text(&copy, "/visio/windows.xml"), part_text(&path, "/visio/windows.xml"), "the window's copy is as it was, byte for byte");
    // The document part is the original's byte for byte but for the text of the one word (`write/settings.rs` `patch_bytes`).
    let xml = part_text(&copy, "/visio/document.xml");
    assert_eq!(xml, part_text(&path, "/visio/document.xml").replace("<SnapSettings>773</SnapSettings>", "<SnapSettings>775</SnapSettings>"));
    assert_eq!(types_bytes(&copy), types_bytes(&path));
}

#[test]
fn data_of_another_version_in_the_file_reads_as_the_defaults_and_refuses_a_change_to_it() {
    let dir = Dir::new();
    let path = dir.path("later.vsdx");
    let solution =
        r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="9"><dc:snap snap="0"/></dc:Data></SolutionXML>"#;
    let bytes =
        diacraft_testkit::package::VsdxBuilder::new().page(diacraft_testkit::package::PageSpec::new(1, "A")).solution("DiaCraft", solution).build();
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["snap"], json!(true), "not interpreted");
    let kept = unjournaled(&s);
    let err = s.execute("snap.set", &json!({"snap": false})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err}");
    assert_eq!(unjournaled(&s), kept);
    // A flag of the file's word can still be changed.
    run(&mut s, "snap.set", json!({"grid": false}));
}

/// A changed option makes the drawing unsaved, and there is no undo step to make it clean again: it is clean when it is saved.
#[test]
fn a_changed_setting_makes_the_drawing_unsaved_and_undo_leaves_it_so() {
    let mut s = blank_session();
    let clean = run(&mut s, "file.status", json!({}));
    assert_eq!(clean["dirty"], json!(false), "{clean}");
    run(&mut s, "snap.set", json!({"ruler": false}));
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], json!(true));
    assert!(s.execute("edit.undo", &json!({})).is_err(), "nothing to undo");
    assert_eq!(run(&mut s, "file.status", json!({}))["dirty"], json!(true));
    assert_eq!(run(&mut s, "snap.settings", json!({}))["ruler"], json!(false));
}

// ---------------------------------------------------------------------------------------------------------- the snap parameter

#[test]
fn a_dropped_shape_is_snapped_in_the_same_undo_step() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"ruler": false}));
    let steps = s.history().unwrap().undo_len();
    // The rectangle's size is its master's (1.5 by 1): whatever it is, an edge of its box lands on a quarter-inch line, 0.03 inch (2.9 points) off here, within the grid's 5.
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.22, "y": 2.53, "snap": true}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one step for the drop and the snap");
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let (w, h) = (r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap());
    let (x, y) = pin(&s, id);
    let on_grid = |v: f64| ((v / 0.25).round() * 0.25 - v).abs() < 1e-6;
    assert!(on_grid(x - w / 2.0) || on_grid(x) || on_grid(x + w / 2.0), "x edges {} {x} {}", x - w / 2.0, x + w / 2.0);
    assert!(on_grid(y - h / 2.0) || on_grid(y) || on_grid(y + h / 2.0), "y edges {} {y} {}", y - h / 2.0, y + h / 2.0);
    // The result says what it did.
    let snap = &r["snap"];
    assert!(snap["hits"].as_array().is_some_and(|h| !h.is_empty() && h.iter().all(|x| x["target"] == json!("grid"))), "{snap}");
    assert!(near(&json!(x), 3.22 + snap["dx"].as_f64().unwrap()) && near(&json!(y), 2.53 + snap["dy"].as_f64().unwrap()), "{snap} {x} {y}");
    // Without the parameter the shape is where it was asked for.
    let r2 = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.13, "y": 2.6}));
    let id2 = u32::try_from(r2["id"].as_u64().unwrap()).unwrap();
    assert_eq!(pin(&s, id2), (6.13, 2.6));
    assert!(r2.get("snap").is_none());
    // `snap: false` is no snapping either.
    let r3 = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 8.13, "y": 2.6, "snap": false}));
    assert_eq!(pin(&s, u32::try_from(r3["id"].as_u64().unwrap()).unwrap()), (8.13, 2.6));
    // Undo takes the shape and the snap away at once.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.document().unwrap().page(1).unwrap().shapes.is_empty());
}

#[test]
fn the_snap_switch_off_makes_the_parameter_do_nothing() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"snap": false}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.13, "y": 2.6, "snap": true}));
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    assert_eq!(pin(&s, id), (3.13, 2.6));
    assert_eq!(r["snap"]["hits"], json!([]));
}

#[test]
fn a_moved_shape_comes_to_rest_on_a_target() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"ruler": false, "grid": false, "guides": true}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}));
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let w = r["width"].as_f64().unwrap();
    // A vertical guide at x = 5.0; the shape's right edge, after a move of 2.45 right, is at 2 + w/2 + 2.45: choose the move so that it lands 0.04 short.
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 5.0}));
    let dx = 5.0 - 0.04 - (2.0 + w / 2.0);
    let moved = run(&mut s, "shape.move", json!({"ids": [id], "dx": dx, "dy": 0.0, "snap": true}));
    let (x, _) = pin(&s, id);
    assert!((x + w / 2.0 - 5.0).abs() < 1e-6, "the right edge on the guide: {x}");
    assert_eq!(moved["snap"]["hits"][0]["target"], json!("guide"));
    assert_eq!(moved["snap"]["hits"][0]["anchor"], json!("high"));
    // A move too far from anything is the move asked for.
    let before = pin(&s, id);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 0.0, "dy": 1.0, "snap": true}));
    assert_eq!(pin(&s, id), (before.0, before.1 + 1.0));
    // More shapes than a snap looks into are refused, and nothing moves.
    let kept = snapshot(&s);
    let ids: Vec<u32> = (0..600).collect();
    let err = s.execute("shape.move", &json!({"ids": ids, "dx": 1, "dy": 1, "snap": true})).unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err}");
    assert_eq!(snapshot(&s), kept);
}

#[test]
fn a_resize_snaps_the_edges_it_moves_and_not_the_ones_it_leaves() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"ruler": false}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    let id = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    let (w, h) = (r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap());
    // The left edge stays at 3 - w/2 and the right edge is dragged to 5.04 (3.8 points off the line); the grid pulls the right edge to 5.0 (0.25 lines).
    let (x0, y0) = (3.0 - w / 2.0, 3.0 - h / 2.0);
    let width = 5.04 - x0;
    let out = run(&mut s, "shape.resize", json!({"id": id, "x": x0, "y": y0, "width": width, "height": h, "snap": true}));
    let doc = s.document().unwrap();
    let shape = doc.page(1).unwrap().shapes.iter().find(|sh| sh.id == id).unwrap().clone();
    let num = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()).and_then(|v| v.number());
    // Width/PinX are written on the instance by the resize.
    let (new_w, pin_x) = (num("Width").unwrap(), num("PinX").unwrap());
    let loc = num("LocPinX").unwrap_or(new_w / 2.0);
    assert!((pin_x - loc + new_w - 5.0).abs() < 1e-6, "the right edge is on 5.0: {pin_x} {loc} {new_w} {out}");
    assert!((pin_x - loc - x0).abs() < 1e-6, "the left edge did not move: {pin_x} {loc}");
    assert_eq!(out["snap"]["hits"][0]["anchor"], json!("high"));
    assert_eq!(out["snap"]["hits"].as_array().unwrap().len(), 1, "only the edge that moved is snapped: {out}");
}

#[test]
fn a_connector_end_given_as_a_place_is_glued_when_it_lands_on_a_connection_point() {
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 2.0}));
    let b = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 2.0}));
    let (ida, idb) = (u32::try_from(a["id"].as_u64().unwrap()).unwrap(), u32::try_from(b["id"].as_u64().unwrap()).unwrap());
    let w = a["width"].as_f64().unwrap();
    // The right-hand connection point of the first rectangle is at (2 + w/2, 2); end the connector 0.05 off it, and at a place in the open.
    let c = run(&mut s, "connector.add", json!({"from": {"x": 2.0 + w / 2.0 + 0.05, "y": 2.04}, "to": {"x": 4.0, "y": 5.0}, "snap": true}));
    assert_eq!(c["begin"]["id"], json!(ida), "{c}");
    assert!(c["begin"]["point"].is_u64());
    assert_eq!(c["end"], Value::Null, "the other end is a free place");
    assert_eq!(c["snap"]["from"]["glue"]["kind"], json!("connection_point"));
    assert_eq!(c["snap"]["to"]["glue"], Value::Null);
    // Without the parameter the end stays a free place.
    let d = run(&mut s, "connector.add", json!({"from": {"x": 2.0 + w / 2.0 + 0.05, "y": 2.04}, "to": {"x": 4.0, "y": 5.0}}));
    assert_eq!(d["begin"], Value::Null);
    // Reconnecting an end to a place near the second rectangle's left point glues it.
    let id = u32::try_from(d["id"].as_u64().unwrap()).unwrap();
    let r = run(&mut s, "connector.reconnect", json!({"id": id, "end": "end", "target": {"x": 6.0 - w / 2.0 - 0.04, "y": 2.03}, "snap": true}));
    assert_eq!(r["end"]["id"], json!(idb), "{r}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_snap_parameter_is_checked() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for snap in [json!("yes"), json!(3), json!([true]), json!({"zoom": 0}), json!({"zoom": -2}), json!({"zoom": "fast"}), json!({"pull": 5})] {
        let err = s.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3, "y": 3, "snap": snap})).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{snap}: {err}");
    }
    assert_eq!(snapshot(&s), before);
    // A zoom is accepted, and a variable grid follows it: at 768 points to an inch the lines are an eighth apart.
    run(&mut s, "snap.set", json!({"ruler": false}));
    let at = run(&mut s, "snap.at", json!({"page": 1, "x": 1.12, "y": 1.12, "zoom": 768}));
    assert!(near(&at["x"], 1.125) && near(&at["y"], 1.125), "{at}");
    let at = run(&mut s, "snap.at", json!({"page": 1, "x": 1.02, "y": 1.02}));
    assert!(near(&at["x"], 1.0) && near(&at["y"], 1.0), "{at}");
}

/// A zoom so small that the strongest pull, 999 points, is more inches than the largest number (below about 5.6E-306 points to an inch) is refused in words by `snap.at` and by the
/// `snap` parameter of a command: at such a zoom the reach of an alignment, which is at least a pull, was no number, and nothing lined up. A zoom just above it is taken.
#[test]
fn a_zoom_at_which_the_strongest_pull_passes_the_largest_number_is_refused() {
    let mut s = blank_session();
    let before = snapshot(&s);
    let err = s.execute("snap.at", &json!({"page": 1, "x": 1.0, "y": 1.0, "zoom": 1e-306})).unwrap_err();
    assert!(
        matches!(err, EngineError::BadParams { .. })
            && err.to_string().contains("`zoom` must be a number above 0")
            && err.to_string().contains("999"),
        "{err}"
    );
    let err = s.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3, "y": 3, "snap": {"zoom": 1e-306}})).unwrap_err();
    assert!(
        matches!(err, EngineError::BadParams { .. })
            && err.to_string().contains("`snap.zoom` must be a number above 0")
            && err.to_string().contains("999"),
        "{err}"
    );
    assert_eq!(snapshot(&s), before, "a refusal changes nothing");
    run(&mut s, "snap.at", json!({"page": 1, "x": 1.0, "y": 1.0, "zoom": 1e-305}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 3, "snap": {"zoom": 1e-305}}));
}

#[test]
fn snap_at_tells_where_a_place_or_a_box_would_land_and_changes_nothing() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"ruler": false}));
    let before = snapshot(&s);
    let p = run(&mut s, "snap.at", json!({"page": 1, "x": 1.22, "y": 2.53}));
    assert!(near(&p["x"], 1.25) && near(&p["y"], 2.5), "{p}");
    assert_eq!(p["hits"].as_array().unwrap().len(), 2);
    let b = run(&mut s, "snap.at", json!({"page": 1, "left": 1.12, "bottom": 1.0, "right": 2.2, "top": 1.5}));
    assert!(near(&b["left"], 1.17) && near(&b["right"], 2.25), "{b}");
    assert_eq!(snapshot(&s), before, "a query changes nothing and is not journaled");
    // Both a place and a box, or neither, or a missing page, are refused.
    for params in [
        json!({"page": 1}),
        json!({"page": 1, "x": 1, "y": 1, "left": 0, "bottom": 0, "right": 1, "top": 1}),
        json!({"page": 1, "x": 1}),
        json!({"page": 9, "x": 1, "y": 1}),
        json!({"x": 1, "y": 1}),
    ] {
        assert!(s.execute("snap.at", &params).is_err(), "{params}");
    }
}

// -------------------------------------------------------------------------------------------------------------------- guides

#[test]
fn guides_are_added_listed_moved_and_deleted_each_in_one_undo_step() {
    let mut s = blank_session();
    let steps = s.history().unwrap().undo_len();
    let a = run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.0}));
    let b = run(&mut s, "guide.add", json!({"page": 1, "orientation": "horizontal", "at": 2.5}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 2);
    assert_eq!((a["orientation"].clone(), a["at"].clone()), (json!("vertical"), json!(3.0)));
    let (ia, ib) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap());
    assert_ne!(ia, ib);
    let list = run(&mut s, "guide.list", json!({}));
    assert_eq!(list["guides"], json!([{"id": ia, "orientation": "vertical", "at": 3.0}, {"id": ib, "orientation": "horizontal", "at": 2.5}]));
    // A guide is a Guide shape, which the picture leaves out and the page's shapes include.
    let doc = s.document().unwrap();
    assert!(doc.page(1).unwrap().shapes.iter().all(|sh| sh.kind == diacraft_doc::ShapeType::Guide));
    // Moved.
    let m = run(&mut s, "guide.move", json!({"id": ia, "at": 4.25}));
    assert_eq!(m["moved"], json!(true));
    assert_eq!(run(&mut s, "guide.list", json!({}))["guides"][0]["at"], json!(4.25));
    // Moved to where it is: no step.
    let kept = unjournaled(&s);
    assert_eq!(run(&mut s, "guide.move", json!({"id": ia, "at": 4.25}))["moved"], json!(false));
    assert_eq!(unjournaled(&s), kept);
    // Deleted, then undone.
    run(&mut s, "guide.delete", json!({"id": ia}));
    assert_eq!(run(&mut s, "guide.list", json!({}))["guides"].as_array().unwrap().len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "guide.list", json!({}))["guides"].as_array().unwrap().len(), 2);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn bad_guides_are_refused() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for (cmd, params) in [
        ("guide.add", json!({"page": 1, "orientation": "diagonal", "at": 1})),
        ("guide.add", json!({"page": 1, "orientation": "vertical"})),
        ("guide.add", json!({"page": 9, "orientation": "vertical", "at": 1})),
        ("guide.add", json!({"page": 1, "orientation": "vertical", "at": 1.0e12})),
        ("guide.move", json!({"id": 7, "at": 1})),
        ("guide.delete", json!({"id": 7})),
        ("guide.list", json!({"page": 9})),
    ] {
        assert!(s.execute(cmd, &params).is_err(), "{cmd} {params}");
    }
    assert_eq!(snapshot(&s), before);
    // A shape that is not a guide is not one to move or delete.
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 3}));
    let id = r["id"].as_u64().unwrap();
    assert!(s.execute("guide.delete", &json!({"id": id})).is_err());
    assert!(s.execute("guide.move", &json!({"id": id, "at": 2})).is_err());
}

#[test]
fn a_guide_is_saved_in_the_file_as_a_guide_shape_and_read_back_as_one() {
    let dir = Dir::new();
    let path = dir.path("guides.vsdx");
    let mut s = blank_session();
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.5}));
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "horizontal", "at": 1.25}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let bytes = std::fs::read(&path).unwrap();
    let pkg = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    let page = pkg.parts().find(|p| p.starts_with("/visio/pages/page") && !p.ends_with("pages.xml")).unwrap().to_owned();
    let xml = String::from_utf8(pkg.part(&page).unwrap().to_vec()).unwrap();
    assert!(xml.matches(r#"Type="Guide""#).count() == 2 && xml.contains("InfiniteLine"), "{xml}");
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    assert_eq!(run(&mut t, "guide.list", json!({}))["guides"], run(&mut s, "guide.list", json!({}))["guides"]);
    // Snapping to the guide works in the reopened drawing.
    run(&mut t, "snap.set", json!({"grid": false, "ruler": false}));
    let at = run(&mut t, "snap.at", json!({"page": 1, "x": 3.53, "y": 6.0}));
    assert!(near(&at["x"], 3.5) && near(&at["y"], 6.0), "{at}");
}

#[test]
fn a_page_in_a_scaled_drawing_keeps_its_guides_in_drawing_units() {
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "metric_1_100"}));
    run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 2.0}));
    let list = run(&mut s, "guide.list", json!({}));
    assert_eq!(list["guides"][0]["at"], json!(2.0), "paper inches, as the commands have them");
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
    let at = run(&mut s, "snap.at", json!({"page": 1, "x": 2.05, "y": 3.0}));
    assert!(near(&at["x"], 2.0), "{at}");
}

#[test]
fn a_guide_that_shapes_are_glued_to_is_not_deleted_and_moves_with_its_pin() {
    // A file's guide with a `Connect` row naming it holds shapes whose formulas follow it; deleting it would leave them naming a shape that is gone.
    let mut s = blank_session();
    let guide = run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 3.0}));
    let id = u32::try_from(guide["id"].as_u64().unwrap()).unwrap();
    s.edit("fixture", |doc| {
        let mut page = (**doc.pages.iter().next().unwrap()).clone();
        page.connects.push(diacraft_doc::Connect {
            from_sheet: 9,
            from_cell: Some("AlignLeft".into()),
            from_part: Some(1),
            to_sheet: id,
            to_cell: Some("PinX".into()),
            to_part: Some(1),
            raw: diacraft_doc::RawAttrs::new(),
            ext: Default::default(),
        });
        doc.pages.set(0, std::sync::Arc::new(page)).unwrap();
        Ok(())
    })
    .unwrap();
    let kept = unjournaled(&s);
    let err = s.execute("guide.delete", &json!({"id": id})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err}");
    assert_eq!(unjournaled(&s), kept, "refused, nothing changed");
    // It can still be moved: the pin moves, and what reads it is worked out again.
    assert_eq!(run(&mut s, "guide.move", json!({"id": id, "at": 3.5}))["moved"], json!(true));
    assert_eq!(run(&mut s, "guide.list", json!({}))["guides"][0]["at"], json!(3.5));
}

#[test]
fn snap_at_leaves_the_shapes_that_move_out_of_the_targets() {
    let mut s = blank_session();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false, "alignment_box": true}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    let id = r["id"].as_u64().unwrap();
    let w = r["width"].as_f64().unwrap();
    // A place whose right edge is 0.03 inch off the shape's right edge, level with its middle: its own edge pulls it unless it is the one moving.
    // (row 13, R43: a side lines up with the same side only, so the box's right edge is the one off the shape's right edge; its left edge
    // was the one before, which met the right edge.)
    let p = json!({"page": 1, "left": 3.0 + w / 2.0 + 0.03 - 1.0, "right": 3.0 + w / 2.0 + 0.03, "bottom": 4.0, "top": 4.5});
    let pulled = run(&mut s, "snap.at", p.clone());
    assert!(near(&pulled["dx"], -0.03), "{pulled}");
    let mut moving = p;
    moving["ids"] = json!([id]);
    let free = run(&mut s, "snap.at", moving);
    assert!(near(&free["dx"], 0.0) && free["hits"] == json!([]), "{free}");
}

#[test]
fn a_guide_is_no_obstacle_to_where_autoconnect_puts_a_shape() {
    // The AutoConnect keeps a new shape off the boxes of the page's other shapes; a guide (which holds `ShapePermeable*` 1) is not one,
    // whatever box a file gave it. The guide here is made 2 inches square over the place the copy would go (centre 5.25, 4).
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 4.0}));
    let a = u32::try_from(a["id"].as_u64().unwrap()).unwrap();
    let g = run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 5.0}));
    let g = u32::try_from(g["id"].as_u64().unwrap()).unwrap();
    s.edit("fixture", |doc| {
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), g).unwrap();
        let sheet = diacraft_doc::SheetRef::Shape(at);
        for (name, value) in [("PinX", "4.5"), ("PinY", "3.0"), ("Width", "2"), ("Height", "2"), ("Angle", "0")] {
            doc.write_cell(
                &sheet,
                &diacraft_doc::CellAddr::direct(name),
                diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text(value)),
                diacraft_doc::WriteOrigin::Drawing,
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right"}));
    assert!(near(&r["x"], 5.25) && near(&r["y"], 4.0), "the copy is where it would be with no guide: {r}");
}

// -----------------------------------------------------------------------------------------------: the shapes' own points (M4), cost (H1)

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// A 1 by 1 shape at (x, y) with a connection point `(cx, cy)` in its own frame.
fn shape_with_a_point(id: u32, at: (f64, f64), point: (f64, f64)) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &at.0.to_string())
        .cell("PinY", &at.1.to_string())
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0")
        .row_cell("Connection", None, RowKey::Ix(0), "X", &point.0.to_string())
        .row_cell("Connection", None, RowKey::Ix(0), "Y", &point.1.to_string())
}

/// Only the connection points are targets.
fn only_connection_points(s: &mut Session) {
    run(
        s,
        "snap.set",
        json!({"ruler": false, "grid": false, "alignment_box": false, "intersections": false, "guides": false, "snap_geometry": false,
               "snap_handles": false, "snap_vertices": false, "snap_connection_points": true, "glue_geometry": false, "glue_handles": false,
               "glue_vertices": false, "glue_connection_points": true, "dynamic_grid": false}),
    );
}

#[test]
fn a_moved_shapes_own_connection_point_lands_on_another_shapes_though_its_box_does_not() {
    // A shape that moves is pulled by its connection points as well as by its box. A's point is the middle of its right side; B's is
    // a quarter of the way in and a tenth of the way up, 0.4 from every corner of B and every point of its box.
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .cell("PageHeight", "8.5")
        .shape(shape_with_a_point(1, (3.0, 3.0), (1.0, 0.5)))
        .shape(shape_with_a_point(2, (7.0, 3.0), (0.25, 0.1)));
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    only_connection_points(&mut s);
    // B's point is at (pin.x - 0.25, pin.y - 0.4): it is 0.05 right of and 0.04 above A's (3.5, 3) when B's pin is at (3.8, 3.44).
    let moved = run(&mut s, "shape.move", json!({"ids": [2], "dx": 3.8 - 7.0, "dy": 0.44, "snap": true}));
    assert!(near(&json!(pin(&s, 2).0), 3.75) && near(&json!(pin(&s, 2).1), 3.4), "{moved}");
    assert_eq!(moved["snap"]["hits"][0]["target"], json!("connection_point"));
    assert_eq!(moved["snap"]["hits"][0]["shape"], json!(1));
    // `snap.at` says the same for the box given, with the shape named: B's point would be at (3.55, 3.04).
    let at = run(&mut s, "snap.at", json!({"page": 1, "ids": [2], "left": 3.3, "bottom": 2.94, "right": 4.3, "top": 3.94}));
    assert!(near(&at["dx"], -0.05) && near(&at["dy"], -0.04), "{at}");
    assert_eq!(at["hits"][0]["target"], json!("connection_point"));
    // Without the shape named, the box alone is asked, and nothing is near it.
    let alone = run(&mut s, "snap.at", json!({"page": 1, "left": 3.3, "bottom": 2.94, "right": 4.3, "top": 3.94}));
    assert_eq!(alone["hits"], json!([]), "{alone}");
    // Without the parameter, the shape goes where it is told.
    let before = pin(&s, 2);
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.04, "dy": 0.03}));
    assert_eq!(pin(&s, 2), (before.0 + 0.04, before.1 + 0.03));
}

#[test]
fn a_dropped_shape_lands_with_its_connection_point_on_another_shapes() {
    let mut s = blank_session();
    only_connection_points(&mut s);
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0}));
    let w = a["width"].as_f64().unwrap();
    // The new rectangle's left point is 0.05 right of and 0.04 above the first one's right point.
    let b = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0 + w + 0.05, "y": 3.04, "snap": true}));
    let id = u32::try_from(b["id"].as_u64().unwrap()).unwrap();
    let (x, y) = pin(&s, id);
    assert!(near(&json!(x), 3.0 + w) && near(&json!(y), 3.0), "{b} {x} {y}");
    assert_eq!(b["snap"]["hits"][0]["target"], json!("connection_point"));
}

#[test]
fn moving_one_of_64_stacked_instances_of_a_big_master_merges_no_row_for_the_snap() {
    // At the command: the rows a move merges are the same with the snap and without it.
    let mut s = session_with(crate::snap::instances_of_a_big_master(64, 100_000, ShapeType::Shape));
    run(
        &mut s,
        "snap.set",
        json!({"glue_geometry": true, "glue_vertices": true, "intersections": true, "dynamic_grid": true, "alignment_box": true}),
    );
    diacraft_doc::work::reset();
    run(&mut s, "shape.move", json!({"ids": [100], "dx": 0.04, "dy": 0.03}));
    let without = diacraft_doc::work::rows_merged();
    run(&mut s, "edit.undo", json!({}));
    diacraft_doc::work::reset();
    let moved = run(&mut s, "shape.move", json!({"ids": [100], "dx": 0.04, "dy": 0.03, "snap": true}));
    let with = diacraft_doc::work::rows_merged();
    eprintln!("rows merged by a move of one of 64 instances of a master of 100,000 rows: {without} without the snap, {with} with it");
    assert_eq!(with, without, "the snap merged {} rows: {moved}", with.saturating_sub(without));
    assert!(moved["snap"]["truncated"].as_bool().unwrap_or(false), "a section left unread is said: {moved}");
    // A box dropped on it, and the settings asked for: the same.
    diacraft_doc::work::reset();
    let at = run(&mut s, "snap.at", json!({"page": 1, "left": 5.53, "bottom": 3.03, "right": 6.53, "top": 4.03, "ids": [100]}));
    assert_eq!(diacraft_doc::work::rows_merged(), 0, "{at}");
}

#[test]
fn a_word_a_file_made_huge_is_shown_cut_and_the_settings_are_read_all_the_same() {
    // `snap.settings` shows the words as the document holds them, and a megabyte of text is not copied into every answer.
    let mut s = blank_session();
    s.edit("fixture", |doc| {
        doc.set_snap_words(&diacraft_doc::SnapWords {
            snap: Some(std::sync::Arc::from("9".repeat(1 << 20))),
            glue: Some(std::sync::Arc::from("x".repeat(1 << 20))),
            ..diacraft_doc::SnapWords::default()
        });
        Ok(())
    })
    .unwrap();
    let r = run(&mut s, "snap.settings", json!({}));
    for word in ["SnapSettings", "GlueSettings"] {
        let shown = r["words"][word].as_str().unwrap_or_default();
        assert!(!shown.is_empty() && shown.chars().count() <= 80, "{word}: {} characters", shown.chars().count());
    }
    assert!(r.to_string().len() < 4_000, "the answer is {} bytes", r.to_string().len());
    // A word that is text and no number is not written over by a ruler or a grid change: the change is refused, and nothing changes.
    let kept = snapshot(&s);
    let err = s.execute("snap.set", &json!({"grid": false})).unwrap_err();
    assert!(matches!(err, EngineError::Failed { .. }), "{err}");
    assert_eq!(snapshot(&s), kept);
}
