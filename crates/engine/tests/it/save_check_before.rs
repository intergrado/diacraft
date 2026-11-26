//! `file.check_compatibility`, the save made in memory and not committed (plan 0.3, PL8; the native-format design's 4.1 and the amendment A1 of the plan).
//!
//! The command makes a save as `file.save_as` (or `file.save`, when it is given no `path`) makes it, up to the disk: the target judged, the writer run in memory, the file read back and compared.
//! It answers `{writer, format, variant, carried, checked, bytes}` and **writes nothing and changes nothing in the session**; it fails as the save would. **Every file here is made by code** (the VSDX
//! files by the testkit's builder, the `.odg` files by `odg_build`), and every read-back goes through the real readers.
//!
//! * [`the_dry_run_writes_no_file_and_changes_nothing`]: the folder, the target, `file.status`, the snapshot, the dirty flag, the journal and the counts of memory, for each writer.
//! * [`the_dry_run_answers_what_the_save_then_says`]: the vectors of (the drawing, a connector whose route style is the page's own, is written by the `.odg` writer, so its dry run and its save both succeed and say the same, which `odg_visit_shapes.rs` asserts) and a macro project across, the dry run's `carried` and `checked` equal the save's as JSON.
//! * [`the_dry_run_fails_as_the_save_would`]: a macro project the type cannot hold, a `.fodg`, a link, a stencil to `.odg`, a file changed on the disk (and `force`), no file yet, a name that is no type.
//! * [`a_dry_run_before_each_save_changes_none_of_them`]: a property over the chains of saves, each run in twin sessions, once with a dry run before every save: the same bytes, the same result.
//! * the costs: the writer and the reader each run once for a dry run and once for a save, 64 dry runs in a row hold nothing, and a dry run with no room is refused in words in a child under `ulimit -v`.

use std::path::Path;

use diacraft_doc::Document;
use diacraft_engine::across::Carried;
use diacraft_engine::{EngineError, Session, check};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use proptest::prelude::Strategy;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Value, json};

use crate::across::vsdx_with_styles as vsdx_drawing;
use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, glued, rect, rect_on};
use crate::resave_sweep::{Chain, Edit, Files, Step, TARGETS, add_live, chain, first_page, plain};
use crate::{Snapshot, run, snapshot};

const CHECK: &str = "file.check_compatibility";

fn dry(s: &mut Session, params: Value) -> Result<Value, EngineError> {
    s.execute(CHECK, &params)
}

fn dry_ok(s: &mut Session, params: Value) -> Value {
    dry(s, params.clone()).unwrap_or_else(|e| panic!("{CHECK} {params}: {e}"))
}

// ------------------------------------------------------------------------------------------------------------------------------- the drawings

fn open(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, bytes) }));
    s
}

/// A drawing of two pages opened from a `.vsdx`.
fn from_vsdx(dir: &Dir) -> Session {
    open(dir, "a.vsdx", &vsdx_drawing())
}

/// The same drawing, saved as `.diacraft`: from here a save as a VSDX type or an `.odg` is written from the model alone.
fn from_diacraft(dir: &Dir) -> Session {
    let mut s = from_vsdx(dir);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    s
}

fn odg_bytes() -> Vec<u8> {
    Drawing::new().page(&format!("{}{}", rect_on("gr1", "A", (2.0, 2.0, 4.0, 2.0), "hello"), rect_on("gr1", "B", (8.0, 2.0, 4.0, 2.0), ""))).bytes()
}

/// An `.odg` of two rectangles, opened.
fn from_odg(dir: &Dir) -> Session {
    open(dir, "a.odg", &odg_bytes())
}

/// A new drawing, with no file.
fn new_drawing(_: &Dir) -> Session {
    let mut s = pinned();
    run(&mut s, "file.new", json!({}));
    s
}

/// One writer of a save, with a drawing that goes through it and the name it is saved as.
struct Writer {
    label: &'static str,
    word: &'static str,
    format: &'static str,
    variant: &'static str,
    target: &'static str,
    start: fn(&Dir) -> Session,
}

const WRITERS: [Writer; 9] = [
    Writer {
        label: "a VSDX drawing as .diacraft",
        word: "diacraft",
        format: "diacraft",
        variant: "diacraft",
        target: "out.diacraft",
        start: from_vsdx,
    },
    Writer {
        label: "a new drawing as .diacraft",
        word: "diacraft",
        format: "diacraft",
        variant: "diacraft",
        target: "out.diacraft",
        start: new_drawing,
    },
    Writer { label: "a .diacraft drawing as .vsdx", word: "vsdx_model", format: "vsdx", variant: "vsdx", target: "out.vsdx", start: from_diacraft },
    Writer { label: "a new drawing as .vsdx", word: "vsdx_model", format: "vsdx", variant: "vsdx", target: "out.vsdx", start: new_drawing },
    Writer { label: "a VSDX drawing as another .vsdx", word: "vsdx_splice", format: "vsdx", variant: "vsdx", target: "out.vsdx", start: from_vsdx },
    Writer { label: "a VSDX drawing as .odg", word: "odg_model", format: "odg", variant: "odg", target: "out.odg", start: from_vsdx },
    Writer { label: "a .diacraft drawing as .odg", word: "odg_model", format: "odg", variant: "odg", target: "out.odg", start: from_diacraft },
    Writer { label: "an .odg drawing as another .odg", word: "odg_splice", format: "odg", variant: "odg", target: "out.odg", start: from_odg },
    Writer {
        label: "an .odg drawing as .diacraft",
        word: "diacraft",
        format: "diacraft",
        variant: "diacraft",
        target: "out.diacraft",
        start: from_odg,
    },
];

// ------------------------------------------------------------------------------------------------------------------------------- what is seen of a session and its folder

/// Everything a dry run may not change: the folder (names and bytes), `file.status`, the snapshot (the document, the history, the revision, the journal and the file), the dirty flag, and the
/// counts of what the session holds.
#[derive(Debug, PartialEq)]
struct Seen {
    files: Vec<(String, Vec<u8>)>,
    status: Value,
    snapshot: Snapshot,
    dirty: bool,
    held: (Option<u64>, u64),
    path: Option<std::path::PathBuf>,
    /// The identifier ledger: for each page, the highest shape number the session will not give again.
    ledger: Vec<(u32, u32)>,
}

/// The identifier ledger of a session: `(page, the mark a new shape must be above)` for each page.
fn ledger_of(s: &Session) -> Vec<(u32, u32)> {
    s.document().map(|doc| doc.pages.iter().map(|page| (page.id, s.id_floor(page.id))).collect()).unwrap_or_default()
}

fn seen(s: &mut Session, dir: &Dir) -> Seen {
    Seen {
        files: dir.files().into_iter().map(|name| (dir.read(&name), name)).map(|(bytes, name)| (name, bytes)).collect(),
        status: run(s, "file.status", json!({})),
        snapshot: snapshot(s),
        dirty: s.is_dirty(),
        held: (s.model_held(), s.published_held()),
        path: s.path().map(Path::to_path_buf),
        ledger: ledger_of(s),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- writes nothing, changes nothing

/// **The dry run writes no file and changes nothing**, for each writer, with the target free and with the target there, with the drawing as opened and edited since: the folder's names and bytes,
/// `file.status`, the snapshot, the dirty flag, the journal and the counts of memory are as they were, and the answer says which writer made the file.
#[test]
fn the_dry_run_writes_no_file_and_changes_nothing() {
    for case in WRITERS {
        for edited in [false, true] {
            let dir = Dir::new();
            let mut s = (case.start)(&dir);
            if edited {
                run(&mut s, "page.add", json!({ "name": "Added" }));
            }
            let what = format!("{} (edited: {edited})", case.label);
            let target = dir.path(case.target);
            let before = seen(&mut s, &dir);
            let answer = dry_ok(&mut s, json!({ "path": target }));
            assert_eq!(answer["writer"], case.word, "{what}: {answer}");
            assert_eq!(answer["format"], case.format, "{what}: {answer}");
            assert_eq!(answer["variant"], case.variant, "{what}: {answer}");
            assert!(answer["bytes"].as_u64().is_some_and(|n| n > 0), "{what}: the size of the file the save would write: {answer}");
            assert_eq!(seen(&mut s, &dir), before, "{what}: a dry run onto a free name changes nothing");
            assert!(!Path::new(&target).exists(), "{what}: no file is made");
            // The target is there, with bytes of its own.
            dir.write(case.target, b"something else, which stays");
            let before = seen(&mut s, &dir);
            let again = dry_ok(&mut s, json!({ "path": target }));
            assert_eq!(again, answer, "{what}: the same answer whether the target is there or not");
            assert_eq!(seen(&mut s, &dir), before, "{what}: a dry run onto a file that is there changes nothing");
            assert_eq!(dir.read(case.target), b"something else, which stays", "{what}: the file is not replaced");
            assert_eq!(dir.files().iter().filter(|name| name.starts_with(case.target)).count(), 1, "{what}: no temporary file is left beside it");
        }
    }
}

/// With no `path` the dry run is for the drawing's own file, as `file.save` saves it: the writer the save would use, nothing written and nothing changed.
#[test]
fn with_no_path_the_dry_run_is_for_the_drawings_own_file() {
    for (start, word, format) in
        [(from_vsdx as fn(&Dir) -> Session, "vsdx_splice", "vsdx"), (from_odg, "odg_splice", "odg"), (from_diacraft, "diacraft", "diacraft")]
    {
        let dir = Dir::new();
        let mut s = start(&dir);
        run(&mut s, "page.add", json!({ "name": "Added" }));
        let before = seen(&mut s, &dir);
        let answer = dry_ok(&mut s, json!({}));
        assert_eq!((&answer["writer"], &answer["format"]), (&json!(word), &json!(format)), "{answer}");
        assert_eq!(seen(&mut s, &dir), before, "{word}: nothing changed");
        // And it is what the Save then says.
        let saved = run(&mut s, "file.save", json!({}));
        assert_eq!(
            (&answer["carried"], &answer["checked"], &answer["bytes"], &answer["variant"]),
            (&saved["carried"], &saved["checked"], &saved["bytes"], &saved["variant"])
        );
    }
    // A drawing with no file yet has no file of its own to ask about.
    let dir = Dir::new();
    let mut fresh = new_drawing(&dir);
    let (asked, saved) = (dry(&mut fresh, json!({})).unwrap_err(), fresh.execute("file.save", &json!({})).unwrap_err());
    assert_eq!(kind_and_words(&asked), kind_and_words(&saved));
    // The options of a Save As belong to a path: a Save has none.
    let mut s = from_vsdx(&dir);
    for options in [json!({ "drop_macros": true }), json!({ "drop_objects": true })] {
        let e = dry(&mut s, options.clone()).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }), "{options}: {e}");
        assert!(e.to_string().contains("path"), "{options}: the words name `path`: {e}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- what the save then says

/// A VSDX drawing with a group whose two members' places are formulas of the group (the drawing): saved as an `.odg` they come back as numbers.
fn vsdx_with_formulas() -> Vec<u8> {
    let member = |id: u32, x: f64, k: f64| {
        format!(
            r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}" F="Sheet.1!Width*{k}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/></Shape>"#
        )
    };
    let shapes = format!(
        r#"<Shape ID="1" Type="Group" NameU="Pair"><Cell N="PinX" V="4"/><Cell N="PinY" V="6"/><Cell N="Width" V="4"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Shapes>{}{}</Shapes></Shape>"#,
        member(2, 1.0, 0.25),
        member(3, 3.0, 0.75)
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shapes)).build()
}

/// A VSDX drawing whose fill is `RGB(255,0,0)`: an `.odg` holds the colour, and not the formula.
fn vsdx_with_rgb() -> Vec<u8> {
    let shape = concat!(
        r##"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="FillForegnd" V="#FF0000" F="RGB(255,0,0)"/>"##,
        r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row>"#,
        r#"<Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row></Section></Shape>"#
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(shape)).build()
}

/// An `.odg` of two rectangles and a connector glued from one to the other.
fn odg_with_a_connector() -> Vec<u8> {
    let named =
        |id: &str, frame| rect(id, frame, "").replacen("<draw:custom-shape ", &format!("<draw:custom-shape draw:id=\"{id}\" xml:id=\"{id}\" "), 1);
    Drawing::new()
        .page(&format!(
            "{}{}{}",
            named("b1", (2.0, 2.0, 4.0, 2.0)),
            named("b2", (10.0, 2.0, 4.0, 2.0)),
            glued("C", "b1", "b2", (6.0, 3.0, 10.0, 3.0))
        ))
        .bytes()
}

/// A `.vsdm` with a VBA project beside a rectangle.
fn vsdm() -> Vec<u8> {
    crate::across::vsdm_with_vba()
}

/// An `.odg` with a Basic project of two files.
fn odg_with_basic() -> Vec<u8> {
    Drawing::new()
        .page(&rect_on("gr1", "A", (2.0, 2.0, 4.0, 2.0), "hello"))
        .file("Basic/Standard/Module1.xml", b"<script:module script:name=\"Check\">REM never run</script:module>")
        .file("Basic/script-lc.xml", b"<library:libraries>the check before a save</library:libraries>")
        .bytes()
}

type Plant = fn(&mut Document);

/// One vector: the file the session opens, whether it is saved as `.diacraft` first (so that the save after is written from the model), the name it is saved as, a loss planted in the file read back,
/// and what the answer must show (beside being what the save says).
struct Vector {
    label: &'static str,
    file: &'static str,
    bytes: fn() -> Vec<u8>,
    through_diacraft: bool,
    to: &'static str,
    plant: Option<Plant>,
    shows: fn(&Value) -> bool,
}

fn classes(answer: &Value) -> Vec<(String, u64)> {
    answer["checked"]["classes"]
        .as_array()
        .map(|list| list.iter().map(|d| (d["class"].as_str().unwrap_or("").to_owned(), d["count"].as_u64().unwrap_or(0))).collect())
        .unwrap_or_default()
}

fn drop_shape_two(doc: &mut Document) {
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == 2).unwrap();
    drop(page.shapes.remove(at));
}

fn fill_as_a_colour(doc: &mut Document) {
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    for at in 0..page.shapes.len() {
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
        let cell = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#ff0000"));
        std::sync::Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new("FillForegnd"), cell);
    }
}

fn drop_character_sections(doc: &mut Document) {
    fn walk(list: &mut diacraft_doc::ShapeList) {
        for at in 0..list.len() {
            if let Some(slot) = list.get_mut(at) {
                let shape = std::sync::Arc::make_mut(slot);
                std::sync::Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "Character");
                walk(&mut shape.children);
            }
        }
    }
    for at in 0..doc.pages.len() {
        if let Some(slot) = doc.pages.get_mut(at) {
            walk(&mut std::sync::Arc::make_mut(slot).shapes);
        }
    }
}

fn vectors() -> Vec<Vector> {
    vec![
        Vector {
            label: "Formulas of a group's members lost in an .odg",
            file: "a.vsdx",
            bytes: vsdx_with_formulas,
            through_diacraft: false,
            to: "b.odg",
            plant: None,
            shows: |a| classes(a).contains(&("formulas".to_owned(), 2)),
        },
        Vector {
            label: "A shape missing in the file read back",
            file: "a.vsdx",
            bytes: vsdx_drawing,
            through_diacraft: true,
            to: "b.vsdx",
            plant: Some(drop_shape_two),
            shows: |a| classes(a).contains(&("shapes".to_owned(), 1)),
        },
        Vector {
            label: "RGB(255,0,0) comes back as #ff0000",
            file: "a.vsdx",
            bytes: vsdx_with_rgb,
            through_diacraft: true,
            to: "b.odg",
            plant: Some(fill_as_a_colour),
            shows: |a| classes(a).iter().any(|(class, count)| class == "formulas" && *count >= 1),
        },
        Vector {
            label: "A glued connector of an .odg, as .diacraft and across",
            file: "a.odg",
            bytes: odg_with_a_connector,
            through_diacraft: false,
            to: "b.vsdx",
            plant: None,
            shows: |a| a["checked"].is_object() && a["writer"] == "vsdx_model",
        },
        Vector {
            label: "Text formatting lost in the file read back",
            file: "a.vsdx",
            bytes: vsdx_drawing,
            through_diacraft: true,
            to: "b.vsdx",
            plant: Some(drop_character_sections),
            shows: |a| a["checked"].is_object(),
        },
        Vector {
            label: "a VBA project saved across as an .odg",
            file: "a.vsdm",
            bytes: vsdm,
            through_diacraft: false,
            to: "b.odg",
            plant: None,
            shows: |a| a["carried"]["macros"] == json!(true),
        },
        Vector {
            label: "a Basic project saved across as .vsdm",
            file: "a.odg",
            bytes: odg_with_basic,
            through_diacraft: false,
            to: "b.vsdm",
            plant: None,
            shows: |a| a["carried"]["macros"] == json!(true),
        },
    ]
}

/// A session of the vector, and the Save As that follows with the dry run before it.
fn session_of(dir: &Dir, vector: &Vector) -> Session {
    let mut s = open(dir, vector.file, &(vector.bytes)());
    if vector.through_diacraft {
        run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    }
    s
}

/// **The dry run answers what the save then says**: for each vector of the spec the dry run's `carried`, `checked`, `bytes` and `variant` equal the `file.save_as` that follows, as JSON, and the
/// answer shows what the vector expects of the check.
#[test]
fn the_dry_run_answers_what_the_save_then_says() {
    for vector in vectors() {
        let dir = Dir::new();
        let mut s = session_of(&dir, &vector);
        let params = json!({ "path": dir.path(vector.to) });
        let (answer, saved) = match vector.plant {
            Some(plant) => check::with_tamper(plant, || (dry_ok(&mut s, params.clone()), run(&mut s, "file.save_as", params.clone()))),
            None => (dry_ok(&mut s, params.clone()), run(&mut s, "file.save_as", params.clone())),
        };
        assert!((vector.shows)(&answer), "{}: the answer shows what the vector expects: {answer}", vector.label);
        for field in ["carried", "checked", "bytes", "variant"] {
            assert_eq!(answer[field], saved[field], "{}: `{field}` is what the save says\n dry run: {answer}\n save:    {saved}", vector.label);
        }
        // The bytes the save wrote are as many as the dry run said.
        assert_eq!(dir.read(vector.to).len() as u64, answer["bytes"].as_u64().unwrap(), "{}", vector.label);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- fails as the save would

/// An error as a person's card sees it: its kind and its words, with no command id.
fn kind_and_words(e: &EngineError) -> (&'static str, String) {
    match e {
        EngineError::Failed { msg, .. } => ("failed", msg.clone()),
        EngineError::BadParams { msg, .. } => ("bad_params", msg.clone()),
        EngineError::ChangedOnDisk { msg, .. } => ("changed_on_disk", msg.clone()),
        EngineError::MacrosNotAllowed { msg, .. } => ("macros_not_allowed", msg.clone()),
        EngineError::Internal { msg, .. } => ("internal", msg.clone()),
        other => panic!("not an error of a save: {other:?}"),
    }
}

/// The dry run of `params` and then the save of them (`save_as`, or `file.save` when `params` has no `path`) in `s`: both must end in the same kind of error with the same words, and leave the session
/// and the folder as they were. → the kind and the words.
fn fails_alike(s: &mut Session, dir: &Dir, params: Value, what: &str) -> (&'static str, String) {
    let before = seen(s, dir);
    let asked = dry(s, params.clone()).expect_err(&format!("{what}: the dry run must fail"));
    assert_eq!(seen(s, dir), before, "{what}: a dry run that fails changes nothing");
    let command = if params.get("path").is_some() { "file.save_as" } else { "file.save" };
    let saved = s.execute(command, &params).expect_err(&format!("{what}: the save must fail"));
    assert_eq!(kind_and_words(&asked), kind_and_words(&saved), "{what}: the dry run fails as the save does");
    assert!(asked.to_string().contains(CHECK), "{what}: the error names the command that was run: {asked}");
    kind_and_words(&asked)
}

/// A stencil file: a `.vssx` of one master.
fn vssx() -> Vec<u8> {
    VsdxBuilder::new()
        .document_type(diacraft_vsdx::Variant::from_extension("vssx").unwrap().content_type())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#))
        .build()
}

/// **The dry run fails as the save would** (the same error of the same kind, in the same words): a macro project into a type that cannot hold one (and the same with `drop_macros`, which is no error),
/// a `.fodg`, a link, a stencil to `.odg`, a file changed on the disk (and with `force`, which is no error), a drawing with no file, a name that is no type.
#[test]
fn the_dry_run_fails_as_the_save_would() {
    // A macro project into a type that cannot hold one: from the file, and from the model.
    for through_diacraft in [false, true] {
        let dir = Dir::new();
        let mut s = open(&dir, "a.vsdm", &vsdm());
        if through_diacraft {
            run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        }
        let (kind, words) = fails_alike(&mut s, &dir, json!({ "path": dir.path("b.vsdx") }), "a macro project into .vsdx");
        assert_eq!(kind, "macros_not_allowed", "{words}");
        assert!(words.contains("macro"), "{words}");
        // The caller's answer to it: leave the macros out. That is no error, and the save says the same.
        let params = json!({ "path": dir.path("c.vsdx"), "drop_macros": true });
        let answer = dry_ok(&mut s, params.clone());
        let saved = run(&mut s, "file.save_as", params);
        assert_eq!(
            (&answer["carried"], &answer["checked"], &answer["bytes"]),
            (&saved["carried"], &saved["checked"], &saved["bytes"]),
            "drop_macros"
        );
    }
    // A `.fodg` is no target of a Save As.
    let dir = Dir::new();
    let mut s = from_vsdx(&dir);
    let (kind, words) = fails_alike(&mut s, &dir, json!({ "path": dir.path("b.fodg") }), "a .fodg");
    assert_eq!(kind, "bad_params", "{words}");
    // A name that is no type.
    let (kind, words) = fails_alike(&mut s, &dir, json!({ "path": dir.path("b.xyz") }), "a name that is no type");
    assert_eq!(kind, "bad_params", "{words}");
    assert!(words.contains(".diacraft"), "{words}");
    // A link to another file.
    std::fs::write(dir.path("other.vsdx"), b"another file").unwrap();
    std::os::unix::fs::symlink(dir.path("other.vsdx"), dir.path("link.vsdx")).unwrap();
    let (kind, words) = fails_alike(&mut s, &dir, json!({ "path": dir.path("link.vsdx") }), "a link");
    assert_eq!(kind, "failed", "{words}");
    assert_eq!(dir.read("other.vsdx"), b"another file", "the file a link leads to is never written");
    // A stencil is no OpenDocument file.
    let mut stencil = open(&dir, "s.vssx", &vssx());
    let (kind, words) = fails_alike(&mut stencil, &dir, json!({ "path": dir.path("s.odg") }), "a stencil to .odg");
    assert_eq!(kind, "bad_params", "{words}");
    assert!(words.contains("stencil"), "{words}");
    // A refusal of a writer: a page with no size is no page an `.odg` is made for, which the writer says in words.
    let mut sizeless = open(&dir, "u.vsdx", &crate::native_common::vsdx_drawing());
    let (kind, words) = fails_alike(&mut sizeless, &dir, json!({ "path": dir.path("u.odg") }), "a refusal of the .odg writer");
    assert_eq!(kind, "failed", "{words}");
    assert!(words.contains("first page"), "{words}");
    assert!(!Path::new(&dir.path("u.odg")).exists());
    // A drawing with no file: Save has nowhere to write.
    let mut fresh = new_drawing(&dir);
    let (kind, words) = fails_alike(&mut fresh, &dir, json!({}), "a drawing with no file");
    assert_eq!(kind, "failed", "{words}");
    // A file that changed on the disk since the drawing was opened: not replaced without `force`.
    let mut s = from_vsdx(&dir);
    std::fs::write(dir.path("a.vsdx"), b"changed by something else").unwrap();
    for params in [json!({}), json!({ "path": dir.path("a.vsdx") })] {
        let (kind, words) = fails_alike(&mut s, &dir, params, "a file changed on the disk");
        assert_eq!(kind, "changed_on_disk", "{words}");
        assert!(words.contains("force"), "{words}");
    }
    // With `force` it is no error: it answers what the save with `force` says.
    let answer = dry_ok(&mut s, json!({ "force": true }));
    let saved = run(&mut s, "file.save", json!({ "force": true }));
    assert_eq!((&answer["carried"], &answer["checked"], &answer["bytes"]), (&saved["carried"], &saved["checked"], &saved["bytes"]));
}

// ------------------------------------------------------------------------------------------------------------------------------- a splice is not checked

/// **A splice target is not checked** (as it is not checked after the save): the writer is `vsdx_splice` or `odg_splice`, `checked` is `null` and `carried` is empty, and the save into the file is
/// refused in the words the dry run gave when the splice writer cannot write it (the splice writer runs in memory).
#[test]
fn a_splice_target_is_not_checked() {
    let none = Carried::none().to_json();
    for (start, to, word) in [(from_vsdx as fn(&Dir) -> Session, "b.vsdx", "vsdx_splice"), (from_odg, "b.odg", "odg_splice")] {
        let dir = Dir::new();
        let mut s = start(&dir);
        run(&mut s, "page.add", json!({ "name": "Added" }));
        for params in [json!({ "path": dir.path(to) }), json!({})] {
            let answer = dry_ok(&mut s, params.clone());
            assert_eq!(answer["writer"], word, "{answer}");
            assert_eq!(answer["checked"], Value::Null, "{word}: not checked: {answer}");
            assert_eq!(answer["carried"], none, "{word}: nothing carried: {answer}");
        }
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path(to) }));
        assert_eq!((saved["checked"].clone(), saved["carried"].clone()), (Value::Null, none.clone()), "{word}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- the registry

/// **The command is in the registry** with the label, the place and the params the plan gives it, is a query (no journal entry, no undo step), is enabled when a drawing is open, and is not
/// when none is.
#[test]
fn the_command_is_in_the_registry() {
    let spec = diacraft_engine::find_command(CHECK).expect("the command is registered");
    assert_eq!((spec.label, spec.menu, spec.shortcut, spec.journal), ("Check Compatibility", &["File", "Info"][..], None, false));
    assert_eq!(diacraft_engine::param_keys(spec.params).unwrap(), ["path", "drop_macros", "drop_objects", "force"]);
    let save_as = diacraft_engine::find_command("file.save_as").unwrap();
    for key in ["path", "drop_macros", "drop_objects", "force"] {
        assert!(spec.params.contains(&format!("{key}:")), "the params doc says what `{key}` is");
        assert!(save_as.params.contains(&format!("{key}:")), "file.save_as takes it too");
    }
    let info = |s: &Session| s.commands().into_iter().find(|c| c.id == CHECK).expect("listed");
    assert!(!info(&Session::new()).enabled, "no drawing, no check");
    assert!(info(&pinned()).enabled, "a drawing is open");
    let dir = Dir::new();
    let mut s = from_vsdx(&dir);
    let journal = s.journal().len();
    dry_ok(&mut s, json!({ "path": dir.path("b.odg") }));
    assert_eq!(s.journal().len(), journal, "a query is not journaled");
    assert!(Session::new().execute(CHECK, &json!({ "path": "x.odg" })).is_err(), "no drawing, no check");
}

// ------------------------------------------------------------------------------------------------------------------------------- what it costs

/// A VSDX file of one page of `count` shapes, `id` the number of the `k`-th (1 to `count`).
fn many_shapes_vsdx(count: u32, id: impl Fn(u32) -> u32) -> Vec<u8> {
    let shapes: String = (1..=count)
        .map(|k| {
            diacraft_testkit::package::shape(
                id(k),
                &format!(
                    r##"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/><Cell N="Width" V="0.5"/><Cell N="Height" V="0.25"/><Cell N="FillForegnd" V="#aabbcc"/>"##,
                    1 + k % 200,
                    1 + k / 200
                ),
            )
        })
        .collect();
    VsdxBuilder::new().page(PageSpec::new(1, "P").sheet(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>"#).shapes(&shapes)).build()
}

/// A counter of the comparisons the check makes in this thread: the hook a test plants in the file read back is called once for each (`check::with_tamper`).
fn comparisons() -> (std::rc::Rc<std::cell::Cell<u32>>, impl FnMut(&mut Document) + 'static) {
    comparisons_with(|_| {})
}

/// [`comparisons`], and `plant` changes the file read back as well.
fn comparisons_with(mut plant: impl FnMut(&mut Document) + 'static) -> (std::rc::Rc<std::cell::Cell<u32>>, impl FnMut(&mut Document) + 'static) {
    let count = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seen = std::rc::Rc::clone(&count);
    (count, move |doc: &mut Document| {
        seen.set(seen.get() + 1);
        plant(doc);
    })
}

/// The number of the last shape of the first page changed in the file read back: the check must find which shape of the many of one number it was, which is a scan of them all for each.
fn last_shape_renumbered(doc: &mut Document) {
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let last = page.shapes.len() - 1;
    std::sync::Arc::make_mut(page.shapes.get_mut(last).unwrap()).id = 2;
}

/// **A dry run is one save's work, and a save is one save's work**: the writer makes the file once and the reader reads it back once, for the dry run and for the save that follows (and the save does
/// not make its file twice, which a save made of two halves could). Counted, never timed: the files made in memory (`saves_made`) and the comparisons of the check, for 20,000 shapes through
/// each model writer, and a splice (which is not checked).
#[test]
fn the_writer_and_the_reader_each_run_once_for_a_dry_run_and_once_for_a_save() {
    let bytes = many_shapes_vsdx(20_000, |k| k);
    // (the writer, the target, whether the file is read back and compared, whether the drawing is first saved as .diacraft)
    for (to, through_diacraft, writer, compared) in [
        ("b.odg", false, "odg_model", 1),
        ("b.vsdx", true, "vsdx_model", 1),
        ("b.vsdx", false, "vsdx_splice", 0),
        ("b.diacraft", false, "diacraft", 1),
    ] {
        let dir = Dir::new();
        let mut s = open(&dir, "a.vsdx", &bytes);
        if through_diacraft {
            run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
        }
        let params = json!({ "path": dir.path(to) });
        let (reads, hook) = comparisons();
        let first = diacraft_engine::file::saves_made();
        let answer = check::with_tamper(hook, || dry_ok(&mut s, params.clone()));
        assert_eq!(answer["writer"], writer, "{answer}");
        assert_eq!(diacraft_engine::file::saves_made() - first, 1, "{writer}: the dry run makes one file");
        assert_eq!(reads.get(), compared, "{writer}: the dry run reads it back and compares it {compared} time(s)");
        let (reads, hook) = comparisons();
        let first = diacraft_engine::file::saves_made();
        let saved = check::with_tamper(hook, || run(&mut s, "file.save_as", params.clone()));
        assert_eq!(diacraft_engine::file::saves_made() - first, 1, "{writer}: the save makes one file, and not a second after the first");
        assert_eq!(reads.get(), compared, "{writer}: the save reads it back and compares it {compared} time(s)");
        assert_eq!((&answer["carried"], &answer["checked"], &answer["bytes"]), (&saved["carried"], &saved["checked"], &saved["bytes"]), "{writer}");
    }
}

/// **A hundred thousand shapes of one number**: saved as `.diacraft` (the file read back has the last of them renumbered, which the check must find among the rest) the dry run and the save say the same
/// and each made one file and compared once; and saved as an `.odg` the writer refuses the file it made ("does not read back") and the dry run is refused in the same words.
#[test]
fn a_hundred_thousand_shapes_of_one_number_are_answered_or_refused_alike_by_the_dry_run_and_the_save() {
    let bytes = many_shapes_vsdx(100_000, |_| 1);
    let dir = Dir::new();
    let mut s = open(&dir, "a.vsdx", &bytes);
    // Refused by the `.odg` writer, alike, before the check is reached.
    let (kind, words) = fails_alike(&mut s, &dir, json!({ "path": dir.path("b.odg") }), "100,000 shapes of one number as an .odg");
    assert_eq!(kind, "failed", "{words}");
    assert!(words.contains("does not read back"), "{words}");
    // As `.diacraft` the file is made, and the check is out of steps in both.
    let params = json!({ "path": dir.path("b.diacraft") });
    let (reads, hook) = comparisons_with(last_shape_renumbered);
    let first = diacraft_engine::file::saves_made();
    let (answer, saved) = check::with_tamper(hook, || (dry_ok(&mut s, params.clone()), run(&mut s, "file.save_as", params.clone())));
    assert_eq!((&answer["carried"], &answer["checked"], &answer["bytes"]), (&saved["carried"], &saved["checked"], &saved["bytes"]));
    // (The plan expected the budget of steps to be reached here. It is not: the check pairs shapes of one number in the order they are in, and finds the one renumbered at its place, in steps in
    // proportion to the shapes. The answer is the same in both, and each made one file and compared once.)
    assert_eq!(classes(&answer), vec![("shape-numbers".to_owned(), 1)], "{}", answer["checked"]);
    assert_eq!(answer["checked"]["done"], json!(true), "{}", answer["checked"]);
    assert_eq!(diacraft_engine::file::saves_made() - first, 2, "one file for the dry run and one for the save");
    assert_eq!(reads.get(), 2, "one comparison for each");
}

/// **64 dry runs in a row hold nothing**: what the process holds (the memory ledger) and what the session holds are as they were after the first (which may work out the pending content stamp, as the
/// save does), and the first leaves them as they were before it when the drawing has a stamp to make. Run in its own process by nextest: another test's session in the same process would move the ledger.
#[test]
fn the_dry_run_holds_nothing_after_it() {
    for case in WRITERS {
        let dir = Dir::new();
        let mut s = (case.start)(&dir);
        run(&mut s, "page.add", json!({ "name": "Added" }));
        let held = |s: &Session| (diacraft_engine::memory::held(), s.model_held(), s.published_held());
        let before = held(&s);
        let params = json!({ "path": dir.path(case.target) });
        let first = dry_ok(&mut s, params.clone());
        let after_one = held(&s);
        for _ in 0..63 {
            assert_eq!(dry_ok(&mut s, params.clone()), first, "{}: the same answer each time", case.label);
        }
        assert_eq!(held(&s), after_one, "{}: 64 dry runs hold what one holds", case.label);
        assert_eq!(after_one, before, "{}: and what the process held before the first", case.label);
        assert!(dir.files().iter().all(|name| !name.starts_with(case.target)), "{}: no file", case.label);
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- a dry run before a save changes none of that save

/// What of a session the twins of a chain must hold alike after every step: the drawing, the history, the revision, the length of the journal, the dirty flag, `file.status` and the bytes of the
/// file the drawing stands on. (Not the file's name: the twins write files of their own.)
#[derive(Debug, PartialEq)]
struct Kept {
    document: Option<Document>,
    undo: usize,
    redo: usize,
    revision: u64,
    journal: usize,
    dirty: bool,
    status: Value,
    file: Option<Vec<u8>>,
    ledger: Vec<(u32, u32)>,
}

fn kept(s: &mut Session) -> Kept {
    let snap = snapshot(s);
    Kept {
        document: snap.document,
        undo: snap.undo_steps,
        redo: snap.redo_steps,
        revision: snap.revision,
        journal: snap.journal.len(),
        dirty: s.is_dirty(),
        status: s.execute("file.status", &json!({})).unwrap_or(Value::Null),
        file: snap.source.map(|(_, bytes)| bytes),
        ledger: ledger_of(s),
    }
}

/// The edit of a step, made in a session: the same as makes it, so that the twins make the same one. `picture` is the number of the picture an edit inserts.
fn make_edit(s: &mut Session, files: &Files, edit: Edit, which: usize, text: &str, picture: u8) {
    let page = first_page(s);
    let Some(doc) = s.document() else { return };
    let top: Vec<u32> = doc
        .pages
        .get(0)
        .map(|p| {
            p.shapes
                .iter()
                .filter(|shape| edit != Edit::Text || (shape.children.is_empty() && shape.foreign.is_none() && !plain(shape).contains("site")))
                .filter(|shape| edit != Edit::Move || shape.sheet.cell("BeginX").is_none())
                .map(|shape| shape.id)
                .collect()
        })
        .unwrap_or_default();
    let id = top.get(which % top.len().max(1)).copied();
    // (An edit that is refused or changes nothing leaves the twins as alike as one that is made.)
    let _ = match (edit, id) {
        (Edit::Move, Some(id)) => s.execute("shape.move", &json!({ "ids": [id], "dx": 0.25, "dy": 0.5, "page": page })),
        (Edit::Text, Some(id)) => s.execute("shape.set_text", &json!({ "id": id, "text": text, "page": page })),
        (Edit::Delete, Some(id)) => s.execute("shape.delete", &json!({ "ids": [id], "page": page })),
        (Edit::Picture, _) if picture < 8 => {
            s.execute("insert.picture", &json!({ "path": files.dir.path(&format!("edit-picture-{picture}.png")), "page": page, "x": 5.0, "y": 4.0 }))
        }
        _ => Ok(Value::Null),
    };
}

/// The params of the command a step runs in the twin `tag`: a Save As writes a file of its own for each twin.
type ParamsOf<'a> = Box<dyn Fn(&str) -> Value + 'a>;

/// Runs `chain` in two sessions that start alike: in the first a dry run asks before every save (a Save As, a Save, and the Save after an edit), in the second the saves are made as they are. They must
/// agree after every step: the same result or the same error in the same words, the same bytes in the files the saves wrote, the same session; and the dry run's answer must be the first's save.
fn twin_chain(files: &Files, chain: &Chain, case: usize, tally: &mut Tally) -> Result<(), String> {
    let (mut a, mut b) = (files.open(chain.start, &format!("twin-{case}-a"))?, files.open(chain.start, &format!("twin-{case}-b"))?);
    if chain.live.any() {
        for s in [&mut a, &mut b] {
            add_live(s, chain.live, &files.picture).map_err(|w| format!("before the first step: {w}"))?;
        }
    }
    let mut pictures = 0u8;
    for (at, step) in chain.steps.iter().enumerate() {
        let here = |what: String| format!("step {at} ({step:?}): {what}");
        if let Step::Edit { edit, which } = *step {
            let text = format!("Edited {case} {at}");
            for s in [&mut a, &mut b] {
                make_edit(s, files, edit, which, &text, pictures);
            }
            pictures = pictures.saturating_add(u8::from(edit == Edit::Picture));
        }
        let (command, params_of): (&str, ParamsOf) = match *step {
            Step::SaveAs { target, drop_objects, drop_macros } => (
                "file.save_as",
                Box::new(
                    move |tag| json!({ "path": files.dir.path(&format!("twin-{case}-{at}-{tag}.{}", TARGETS[target])), "drop_objects": drop_objects, "drop_macros": drop_macros }),
                ),
            ),
            Step::Save | Step::Edit { .. } => {
                if a.path().is_none() {
                    continue;
                }
                ("file.save", Box::new(|_| json!({})))
            }
        };
        let asked = a.execute(CHECK, &params_of("a"));
        let saved_a = a.execute(command, &params_of("a"));
        let saved_b = b.execute(command, &params_of("b"));
        // The dry run said what the save then said.
        match (&asked, &saved_a) {
            (Ok(answer), Ok(saved)) => {
                tally.answered += 1;
                tally.checked += usize::from(!answer["checked"].is_null());
                tally.carried += usize::from(answer["carried"] != Carried::none().to_json());
                for field in ["carried", "checked", "bytes", "variant"] {
                    if answer[field] != saved[field] {
                        return Err(here(format!("the dry run's `{field}` is not the save's:\n dry run: {answer}\n save:    {saved}")));
                    }
                }
            }
            (Err(asked), Err(saved)) if kind_and_words(asked) == kind_and_words(saved) => tally.refused += 1,
            (asked, saved) => return Err(here(format!("the dry run and the save end differently: {asked:?} and {saved:?}"))),
        }
        // The save after a dry run is the save with none.
        match (&saved_a, &saved_b) {
            (Ok(with), Ok(without)) => {
                if with != without {
                    return Err(here(format!("the save after a dry run answers otherwise:\n with:    {with}\n without: {without}")));
                }
                let (path_a, path_b) = (a.path().map(Path::to_path_buf), b.path().map(Path::to_path_buf));
                let bytes = |path: &Option<std::path::PathBuf>| path.as_ref().and_then(|p| std::fs::read(p).ok());
                if bytes(&path_a) != bytes(&path_b) {
                    return Err(here("the save after a dry run wrote other bytes".to_owned()));
                }
            }
            (Err(with), Err(without)) if kind_and_words(with) == kind_and_words(without) => {}
            (with, without) => return Err(here(format!("the save after a dry run ends differently: {with:?} and {without:?}"))),
        }
        if kept(&mut a) != kept(&mut b) {
            return Err(here("the sessions differ after the step".to_owned()));
        }
    }
    Ok(())
}

/// What the twin chains did, counted: the dry runs that answered (and how many of those said something was checked, and something was carried) and the dry runs that were refused.
#[derive(Debug, Default)]
struct Tally {
    answered: usize,
    checked: usize,
    carried: usize,
    refused: usize,
}

/// Runs `cases` random chains from `seed`, and gives each that failed with the words.
fn twin_sweep(cases: u32, seed: u64) -> (Tally, Vec<(Chain, String)>) {
    let files = Files::new();
    let mut seed_bytes = [0u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let config = Config { cases, failure_persistence: None, ..Config::default() };
    let mut runner = TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes));
    let strategy = chain();
    let mut failures = Vec::new();
    let mut tally = Tally::default();
    for case in 0..cases as usize {
        let chain = strategy.new_tree(&mut runner).unwrap().current();
        if let Err(words) = twin_chain(&files, &chain, case, &mut tally) {
            failures.push((chain, words));
        }
    }
    (tally, failures)
}

/// **A dry run before each save changes none of them** (a property over the chains: random chains of two to six steps from every start, a fixed seed, and a case count that
/// `DIACRAFT_CHECK_BEFORE_CASES` sets, 150 when it is not): each chain is run twice, once with a dry run before every save and once with none, and the saves write the same bytes, answer the same, and
/// leave the same session; and each dry run said what the save after it said.
#[test]
fn a_dry_run_before_each_save_changes_none_of_them() {
    let cases = std::env::var("DIACRAFT_CHECK_BEFORE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let (tally, failures) = twin_sweep(cases, 0x5EED_C4EC);
    eprintln!("RESULT {cases} chains: {tally:?}");
    let shown: Vec<String> =
        failures.iter().take(5).map(|(chain, words)| format!("{:?} {:?} {:?}\n  {words}", chain.start, chain.live, chain.steps)).collect();
    assert!(failures.is_empty(), "{} of {cases} chains failed ({tally:?}):\n{}", failures.len(), shown.join("\n"));
    // The property is about dry runs that answered, and refusals, and checks and carried data: a sweep of none of them would hold for nothing.
    assert!(tally.answered >= cases as usize / 2 && tally.checked > 0 && tally.carried > 0 && tally.refused > 0, "{tally:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------- no room: refused in words, not an abort

#[cfg(target_os = "linux")]
mod children {
    use std::process::Command;
    use std::sync::Arc;

    use diacraft_doc::{Document, Format, Part, PartRole, RawAttrs};
    use diacraft_engine::memory::Hold;
    use diacraft_engine::{EngineError, Session};
    use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, cell, shape};
    use serde_json::{Value, json};

    use super::{CHECK, kind_and_words};
    use crate::native_common::Dir;

    const SCENARIO: &str = "ENGINE_CHECK_BEFORE_SCENARIO";
    const FILE: &str = "ENGINE_CHECK_BEFORE_FILE";
    const CT_OLE: &str = "application/vnd.openxmlformats-officedocument.oleObject";

    /// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
    fn status_kib(key: &str) -> u64 {
        let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
            .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
    }

    /// `n` bytes no compression makes smaller.
    fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.extend_from_slice(&state.to_le_bytes());
        }
        out.truncate(n);
        out
    }

    /// A drawing of one page with one embedded object of `each` bytes, stored (so that a file of the size is made at once), written to `dir`.
    fn file_with_a_part(dir: &Dir, each: usize) -> String {
        // (The part folder of the format, in two pieces: the public tree is scanned for the product's name, which a part's own folder is not.)
        let pkg = Pkg::new().stored().part(concat!("/vis", "io/embeddings/oleObject0.bin"), CT_OLE, noise(each, 1));
        let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).with(pkg).build();
        dir.write("held.vsdx", &bytes)
    }

    /// A document of one blank page holding a part of `each` bytes that nothing models (an embedding, kept as bytes and charged to the blob pool).
    fn heavy_document(each: usize) -> Document {
        let mut doc = Document::new_blank();
        doc.parts.push(Arc::new(Part {
            format: Format::Native,
            name: Arc::from("embeddings/big0.bin"),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::default(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(noise(each, 7)),
        }));
        doc
    }

    /// What a command ended as, for a line the parent reads: `ok`, or the first words of the refusal.
    fn how(result: &Result<Value, EngineError>) -> String {
        match result {
            Ok(_) => "ok".to_owned(),
            Err(e) => e.to_string().replace(['\n', '\r'], " ").chars().take(200).collect(),
        }
    }

    /// The dry run and the save of the same params end alike: both answer with the same `carried`, `checked`, `bytes` and `variant`, or both are refused in the same words.
    fn assert_alike(what: &str, asked: &Result<Value, EngineError>, saved: &Result<Value, EngineError>) {
        match (asked, saved) {
            (Ok(answer), Ok(saved)) => {
                for field in ["carried", "checked", "bytes", "variant"] {
                    assert_eq!(answer[field], saved[field], "{what}: `{field}`");
                }
            }
            (Err(asked), Err(saved)) => assert_eq!(kind_and_words(asked), kind_and_words(saved), "{what}: refused alike"),
            _ => panic!("{what}: the dry run ended {} and the save {}", how(asked), how(saved)),
        }
    }

    #[test]
    #[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
    fn dry_run_child() {
        let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
        let dir = Dir::new();
        println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
        match scenario.as_str() {
            // A drawing that holds a part of 256 MiB, as a dry run and then as a save, three times in a row under an address limit of 3 GB: a dry run that kept its file, or its copy of the drawing, would
            // leave the save after it, or the dry run after that, no room, and the process would abort.
            "held_part" => {
                let path = std::env::var(FILE).expect("the file of the drawing");
                let mut s = Session::new();
                // (The clock is pinned: a file written from the model holds the time, whose digits change the size of the deflated part that holds them by a byte.)
                s.pin_clock(Some(1_900_000_000));
                s.execute("file.open", &json!({ "path": path })).expect("the reader admits the package");
                let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().take(1).map(|x| x.id).collect();
                s.execute("shape.move", &json!({ "ids": ids, "dx": 0.5, "dy": 0.0 })).expect("an edit");
                // (Each round is the dry run, which is asked twice in the first, and the save with the same params: nine files of 256 MiB made in memory, the last for `.diacraft`, which deflates it.)
                for (to, twice) in [("dry0.vsdx", true), ("dry1.vsdx", false), ("dry2.vsdx", false), ("dry3.diacraft", false)] {
                    let params = json!({ "path": dir.path(to) });
                    let first = s.execute(CHECK, &params);
                    if twice {
                        assert_alike(&format!("{to}, two dry runs"), &first, &s.execute(CHECK, &params));
                    }
                    let saved = s.execute("file.save_as", &params);
                    assert_alike(to, &first, &saved);
                    println!(
                        "\nRESULT held_part {to} dry={} save={} VmHWM={}KiB VmPeak={}KiB",
                        how(&first),
                        how(&saved),
                        status_kib("VmHWM"),
                        status_kib("VmPeak")
                    );
                }
            }
            // A drawing that fits the budget alone, beside what the rest of the process holds: the dry run is refused in the save's words and writes nothing, and answers once the hold is given back.
            "beside" => {
                let mut s = Session::new();
                s.pin_clock(Some(1_900_000_000));
                s.execute("file.new", &json!({})).unwrap();
                let doc = heavy_document(120 << 20);
                s.edit("Test edit", |d| {
                    *d = doc.clone();
                    Ok(())
                })
                .unwrap();
                drop(doc);
                let others = Hold::new(2_300_000_000);
                let target = dir.path("out.diacraft");
                let asked = s.execute(CHECK, &json!({ "path": target }));
                let saved = s.execute("file.save_as", &json!({ "path": target }));
                println!("\nRESULT beside dry={} VmHWM={}KiB VmPeak={}KiB", how(&asked), status_kib("VmHWM"), status_kib("VmPeak"));
                match (&asked, &saved) {
                    (Err(asked), Err(saved)) => {
                        assert!(asked.to_string().contains("not enough memory beside what is open"), "{asked}");
                        assert_eq!(kind_and_words(asked), kind_and_words(saved), "refused in the words of the save");
                    }
                    other => panic!("the dry run or the save was made beside a hold of 2.3 GB: {other:?}"),
                }
                assert!(dir.files().is_empty(), "a refused dry run writes nothing, and neither does a refused save: {:?}", dir.files());
                drop(others);
                let asked = s.execute(CHECK, &json!({ "path": target }));
                let saved = s.execute("file.save_as", &json!({ "path": target }));
                assert!(asked.is_ok() && saved.is_ok(), "alone it answers and saves: {} and {}", how(&asked), how(&saved));
                assert_alike("alone", &asked, &saved);
            }
            other => panic!("no scenario {other}"),
        }
    }

    /// Runs the child's `scenario` under `ulimit -v 3000000`: whether it ended well, and what it printed.
    fn run_child(scenario: &str, file: Option<&str>) -> (bool, String) {
        let exe = std::env::current_exe().expect("the path of this test binary");
        let script = "ulimit -v 3000000 && exec \"$0\" --exact save_check_before::children::dry_run_child --ignored --nocapture --test-threads=1";
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script).arg(exe).env(SCENARIO, scenario);
        if let Some(file) = file {
            command.env(FILE, file);
        }
        let out = command.output().expect("run /bin/sh");
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
        if !out.status.success() {
            eprintln!("child `{scenario}` ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
        }
        (out.status.success(), stdout)
    }

    /// **A dry run with no room is refused in words**, and one that has room gives it back: a drawing that holds a part of 256 MiB is asked about and saved three times in a row to two types under
    /// `ulimit -v 3000000`, and each dry run says what the save says; and a drawing whose save fits the budget once is refused beside a hold of 2.3 GB in the save's own words, with nothing written, and answers
    /// when the hold is given back.
    #[test]
    fn a_dry_run_with_no_room_refuses_in_words_in_a_child_and_one_with_room_gives_it_back() {
        let dir = Dir::new();
        let path = file_with_a_part(&dir, 256 << 20);
        let (ended_well, stdout) = run_child("held_part", Some(&path));
        assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
        assert!(stdout.contains("RESULT held_part"), "{stdout}");
        eprintln!("{}", stdout.lines().filter(|l| l.contains("RESULT")).collect::<Vec<_>>().join("\n"));
        let (ended_well, stdout) = run_child("beside", None);
        assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
        assert!(stdout.contains("RESULT beside"), "{stdout}");
        eprintln!("{}", stdout.lines().filter(|l| l.contains("RESULT")).collect::<Vec<_>>().join("\n"));
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- timed, by hand

/// **A dry run is not slower than the save it stands for** (the yardstick of the task's crafted inputs: the `file.save_as` of the same drawing, back to back, in a twin session). `#[ignore]`d: no test
/// of the ordinary run asserts a time (the counts above are what the ordinary run holds). Run by hand: `cargo test -p diacraft-engine --test it save_check_before::timed -- --ignored --nocapture`. It prints
/// both times for 20,000 shapes through each model writer, and fails when the dry run takes more than a quarter longer than the save and more than 100 ms over it.
#[test]
#[ignore = "a timed run, by hand: cargo test -p diacraft-engine --test it save_check_before::timed -- --ignored --nocapture"]
fn timed_a_dry_run_is_not_slower_than_the_save_it_stands_for() {
    let bytes = many_shapes_vsdx(20_000, |k| k);
    for (to, through_diacraft) in [("b.odg", false), ("b.vsdx", true), ("b.diacraft", false)] {
        let (dir_a, dir_b) = (Dir::new(), Dir::new());
        let (mut asked, mut saved) = (open(&dir_a, "a.vsdx", &bytes), open(&dir_b, "a.vsdx", &bytes));
        if through_diacraft {
            run(&mut asked, "file.save_as", json!({ "path": dir_a.path("a.diacraft") }));
            run(&mut saved, "file.save_as", json!({ "path": dir_b.path("a.diacraft") }));
        }
        let start = std::time::Instant::now();
        let answer = dry_ok(&mut asked, json!({ "path": dir_a.path(to) }));
        let dry = start.elapsed();
        let start = std::time::Instant::now();
        let result = run(&mut saved, "file.save_as", json!({ "path": dir_b.path(to) }));
        let save = start.elapsed();
        println!("{to}: the dry run {dry:?}, the save {save:?}, {} bytes", answer["bytes"]);
        assert_eq!(answer["bytes"], result["bytes"]);
        assert!(dry <= save + save / 4 + std::time::Duration::from_millis(100), "{to}: the dry run took {dry:?} against the save's {save:?}");
    }
}
