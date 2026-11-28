//! A Save As that was asked to leave out the macros or the embedded objects (`drop_macros`, `drop_objects`), then a Save in the same session with no edit.
//!
//! **The rule: a Save As option holds for every later save into the file it wrote.** The drawing keeps what was left out. Each save into that file leaves it out again, as the
//! Save As did, and compares the file with the drawing as it was given to the writer, never with the drawing whole. A Save with no edit is never refused and is no edit; a real edit after the
//! Save As is still written, and what was left out stays out.
//!
//! Built the rule for a Save As to `.odg` from a VSDX file (`LeftOut`). It found two places it did not hold, which this task closed: a Save into a VSDX file written with
//! `drop_objects` ("a change to the picture of a shape"), and the options of a Save As from an `.odg` to another `.odg` (ignored).
//!
//! Closed what left open, by the same rule, and put the Save As that leaves nothing out (`nothing`) among the cases: a VSDX object saved as `.odg` was written a second time
//! by the next save (the embedding rode into a file that holds the object as a frame), a `.diacraft` drawing of `.odg` frames saved as a VSDX type was refused on its next save (only a drawing read
//! from an `.odg` adopted what the file reads as), and a page sheet with its `User` section before its `Layer` section was an edit in an unedited Save (the sections were compared in the order each
//! model has them).
//!
//! Closed the two cases found and left: an `.odg`'s object held twice by a VSDX file after an *edit* (the file's embedding, and the object's file that rode in DiaCraft's own
//! part: a save into the file is given the drawing without the parts the embeddings were written from, `across::natively`), and a drawing made from an `.otg` template that holds `.odg` frames, whose next
//! Save after a Save As to a VSDX type was refused (a drawing with no working file is made the model its file reads as when the writer was given pictures an `.odg` held). The edit grid counts the
//! copies of the object in a VSDX target too.
//!
//! **Every file is made by code**: the `.odg` by `odg_build`, the VSDX types and the `.diacraft` by saving it.

use diacraft_doc::PartRole;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, rect};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};

use crate::run;

/// The VBA project of [`crate::across::vsdm_with_vba`].
const VBA: &[u8] = b"not code, never run";

/// The files of an `.odg`'s macro project.
fn macro_files(drawing: Drawing) -> Drawing {
    drawing.file("Basic/Standard/Module1.xml", b"<script/>").file("Basic/script-lc.xml", b"<libraries/>")
}

/// An `.odg` of a frame that shows an embedded object (a compound file, with a PNG for its replacement), a rectangle beside it, and a macro project of its own.
fn odg_full() -> Vec<u8> {
    macro_files(
        Drawing::new()
            .page(&format!("{}{}", crate::across::object_frame(), rect("R", (2.0, 8.0, 4.0, 2.0), "")))
            .file("Object 1", &crate::across::compound())
            .file("ObjectReplacements/Object 1", &crate::across::png(10)),
    )
    .bytes()
}

/// A `.vsdm` with a VBA project beside one rectangle, whose `Application` is DiaCraft's (a splice save stamps it on any file, and a file a model writer made keeps another program's: the stamp is not what
/// these tests are about).
fn vsdm_vba() -> Vec<u8> {
    let app = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" "#,
        r#"xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>DiaCraft</Application></Properties>"#
    );
    VsdxBuilder::new()
        .app(app)
        .document_type(diacraft_vsdx::Variant::from_extension("vsdm").unwrap().content_type())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shape(1, &rect_cells(2.0))))
        .with(Pkg::new().default_type("bin", "application/vnd.ms-office.vbaProject").part_without_override("/visio/vbaProject.bin", VBA).rel(
            "/visio/document.xml",
            "rId50",
            VBA_REL,
            "vbaProject.bin",
        ))
        .build()
}

/// The cells of a rectangle of the VSDX file: 2 by 1 inches with its pin at (`x`, 2).
fn rect_cells(x: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>"#
    )
}

/// The relationship type that names a VBA project.
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// An `.odg` of two rectangles and a macro project, with no object.
fn odg_macros() -> Vec<u8> {
    macro_files(Drawing::new().page(&format!("{}{}", rect("R", (2.0, 8.0, 4.0, 2.0), ""), rect("S", (8.0, 8.0, 4.0, 2.0), "")))).bytes()
}

/// A session with the clock pinned that has opened `bytes` as `name` in `dir`.
fn opened(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write(name, bytes) }));
    s
}

/// The file `bytes` (named `from`) makes when it is saved as `to` with nothing left out: a macro-enabled type, `.odg` or `.diacraft`, which hold all of it.
fn made_as(dir: &Dir, from: &str, bytes: &[u8], to: &str) -> Vec<u8> {
    let mut s = opened(dir, from, bytes);
    run(&mut s, "file.save_as", json!({ "path": dir.path(to) }));
    dir.read(to)
}

/// A `.vsdx` of one rectangle whose page sheet holds a `User` section and a `Layer` section, in the order `layer_first` says (the gate's sample-file check found a Save after a Save As to
/// `.odg` answering `edited: true` for a page sheet with its `User` section before its `Layer` section). Names and structure only; the values are made up.
fn vsdx_page_sections(layer_first: bool) -> Vec<u8> {
    let user = concat!(
        r#"<Section N="User"><Row N="PageKey"><Cell N="Value" V="one" U="STR"/></Row>"#,
        r#"<Row N="PageKey2"><Cell N="Value" V="two" U="STR"/></Row></Section>"#
    );
    let layer = concat!(
        r#"<Section N="Layer"><Row IX="0"><Cell N="Name" V="Mine"/><Cell N="NameUniv" V="Mine"/><Cell N="Color" V="255"/><Cell N="Visible" V="1"/>"#,
        r#"<Cell N="Print" V="1"/><Cell N="Snap" V="1"/><Cell N="Glue" V="1"/><Cell N="Active" V="0"/><Cell N="Lock" V="0"/><Cell N="Status" V="0"/>"#,
        r#"<Cell N="ColorTrans" V="0"/></Row></Section>"#
    );
    let (first, second) = if layer_first { (layer, user) } else { (user, layer) };
    let sheet = format!(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>{first}{second}"#);
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(&sheet).shapes(&shape(1, &rect_cells(2.0)))).build()
}

/// A way to make the drawing a chain starts from: a fresh session each time.
type Start<'a> = Box<dyn Fn() -> Session + 'a>;

/// Every drawing the chains start from, each holding a macro project: the `.odg` with an object, the `.odg` with no object, the files that drawing makes (a `.vsdm` and a `.diacraft`, which
/// hold the object and the project of the other format), a `.vsdm` with a VBA project, and the `.odg` that carries that VBA project, whose project is of the other format.
fn starts(dir: &Dir) -> Vec<(&'static str, Start<'_>)> {
    let vsdm = made_as(dir, "full-source.odg", &odg_full(), "full-made.vsdm");
    let native = made_as(dir, "full-source.odg", &odg_full(), "full-made.diacraft");
    let carrying = made_as(dir, "vba-source.vsdm", &vsdm_vba(), "vba-made.odg");
    let plain = made_as(dir, "plain-source.odg", &odg_macros(), "plain-made.diacraft");
    vec![
        ("an .odg with an object and a macro project", Box::new(|| opened(dir, "full.odg", &odg_full()))),
        ("an .odg with a macro project", Box::new(|| opened(dir, "macros.odg", &odg_macros()))),
        ("a .vsdm with an object and an ODF macro project", Box::new(move || opened(dir, "full.vsdm", &vsdm))),
        ("a .diacraft with an object and a macro project", Box::new(move || opened(dir, "full.diacraft", &native))),
        ("a .vsdm with a VBA project", Box::new(|| opened(dir, "vba.vsdm", &vsdm_vba()))),
        ("an .odg that carries a VBA project", Box::new(move || opened(dir, "carrying.odg", &carrying))),
        // Appended by the rows of the options test count from the front.
        ("a .diacraft of two rectangles and a macro project", Box::new(move || opened(dir, "plain.diacraft", &plain))),
        ("a .vsdx whose page sheet has a User section before a Layer section", Box::new(|| opened(dir, "sections.vsdx", &vsdx_page_sections(false)))),
        ("a .vsdx whose page sheet has a Layer section before a User section", Box::new(|| opened(dir, "control.vsdx", &vsdx_page_sections(true)))),
    ]
}

/// What a file holds of what the options leave out: whether it holds an embedded object, and whether it holds a macro project. Told by the part's role and name and by its bytes, so that a part
/// of either format and an entry of DiaCraft's own folder are known for what they are. A `.diacraft` is the whole model and is not told apart.
fn holds(name: &str, bytes: &[u8]) -> (bool, bool) {
    let extension = name.rsplit('.').next().unwrap_or("");
    let compound = crate::across::compound();
    if extension.starts_with("vs") {
        let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
        let object = doc.parts.iter().any(|p| matches!(p.role, PartRole::Embedding | PartRole::ObjectFile) || &*p.bytes == compound.as_slice());
        let macros = doc.parts.iter().any(|p| {
            matches!(p.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData)
                || p.name.starts_with("Basic/")
                || &*p.bytes == VBA
        });
        (object, macros)
    } else {
        let package =
            diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
        let (mut object, mut macros) = (false, false);
        for entry in package.entries() {
            let held = package.part(&entry.name);
            object |= (entry.name.starts_with("Object") && !entry.name.starts_with("ObjectReplacements/"))
                || held.is_some_and(|b| b == compound.as_slice());
            macros |= entry.name.starts_with("Basic/") || held.is_some_and(|b| b == VBA);
        }
        (object, macros)
    }
}

/// The options that make a case, each as the parameters of `file.save_as`. **`nothing` is a case too**: a Save As that leaves nothing out is where the two bugs found and left
/// open are (the object written twice, and the refusal after a crossing); a Save As that leaves something out had been closed.
fn asks() -> [(&'static str, Value); 4] {
    [
        ("nothing", json!({})),
        ("drop_objects", json!({ "drop_objects": true })),
        ("drop_macros", json!({ "drop_macros": true })),
        ("both", json!({ "drop_objects": true, "drop_macros": true })),
    ]
}

/// How many entries of a file hold the bytes of the object (a compound file): the object frame's `Object N` is one, and **a second copy of it in DiaCraft's own folder is the bug of an object written
/// twice**. A VSDX file holds it as an embedding, once. A `.diacraft` is the whole model and is not counted.
fn copies_of_the_object(name: &str, bytes: &[u8]) -> usize {
    let compound = crate::across::compound();
    if name.ends_with(".diacraft") {
        return 0;
    }
    if name.rsplit('.').next().unwrap_or("").starts_with("vs") {
        let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
        return doc.parts.iter().filter(|p| &*p.bytes == compound.as_slice()).count();
    }
    let package =
        diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
    package.entries().filter(|entry| package.part(&entry.name).is_some_and(|held| held == compound.as_slice())).count()
}

/// The types a Save As writes, as the extension of the name: a macro-free and a macro-enabled VSDX type, the OpenDocument drawing, and DiaCraft's own.
const TARGETS: [&str; 4] = ["vsdx", "vsdm", "odg", "diacraft"];

/// The Save As of a chain, with the options in `asked`: `Ok(Some(bytes))` the file it wrote, `Ok(None)` the refusal that a macro project gets in a type that cannot hold one, and `Err` anything
/// else.
fn save_as(s: &mut Session, dir: &Dir, name: &str, asked: &Value) -> Result<Option<Vec<u8>>, String> {
    let mut params = asked.clone();
    params["path"] = json!(dir.path(name));
    match s.execute("file.save_as", &params) {
        Ok(_) => Ok(Some(dir.read(name))),
        Err(EngineError::MacrosNotAllowed { .. }) => Ok(None),
        Err(e) => Err(format!("the save as was refused: {e}")),
    }
}

/// The save after a Save As, with no edit between: no refusal, no edit, not dirty, the same bytes.
fn saved_again_as_it_was(s: &mut Session, dir: &Dir, name: &str, first: &[u8]) -> Result<(), String> {
    let again = s.execute("file.save", &json!({})).map_err(|e| format!("the save after it was refused: {e}"))?;
    if again.get("edited").is_some_and(|edited| *edited != json!(false)) {
        return Err(format!("the save after it counts as an edit: {again}"));
    }
    if again["dirty"] != json!(false) || s.is_dirty() {
        return Err(format!("the drawing is dirty after the save: {again}"));
    }
    if dir.read(name) != first {
        return Err("the save after it wrote other bytes than the save as".to_owned());
    }
    Ok(())
}

/// **The bug (concern 1, and its neighbours).** From each drawing, a Save As to each type with each of the options, then a Save: no refusal, no edit, the same bytes. A macro
/// project that the type cannot hold is refused, in words, unless the Save As is asked to leave it out. Every chain is run before anything is asserted, so that one failure does not hide another.
#[test]
fn a_save_after_a_save_as_that_left_things_out_is_no_edit_in_every_pair_of_formats() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let (mut chains, mut refused) = (0, 0);
    for (at, (what, start)) in starts(&dir).into_iter().enumerate() {
        for target in TARGETS {
            for (word, asked) in asks() {
                chains += 1;
                let name = format!("out-{at}-{word}.{target}");
                let mut s = start();
                let label = format!("{what}, saved as .{target} with {word}");
                match save_as(&mut s, &dir, &name, &asked) {
                    Err(why) => failures.push(format!("{label}: {why}")),
                    Ok(None) => {
                        refused += 1;
                        if target != "vsdx" || asked["drop_macros"] == json!(true) {
                            failures.push(format!("{label}: refused for its macro project"));
                        }
                    }
                    Ok(Some(first)) => {
                        if target == "vsdx" && holds(&name, &first).1 {
                            failures.push(format!("{label}: written with a macro project in a type that cannot hold one"));
                        }
                        if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &first) {
                            failures.push(format!("{label}: {why}"));
                        }
                    }
                }
            }
        }
    }
    assert!(chains >= 9 * 4 * 4, "every drawing, type and option: {chains} chains");
    assert!(refused >= 6, "the macro-free type refuses what holds macros and is not asked to leave them out: {refused}");
    assert!(failures.is_empty(), "{} of {chains} chains failed:\n{}", failures.len(), failures.join("\n"));
}

/// The place of a shape in the drawing, which the file written must give it too.
fn pin(s: &Session, id: u32) -> (f64, f64) {
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let frame = diacraft_engine::draw::shape_frame(doc, page, id).unwrap();
    (frame.pin_x, frame.pin_y)
}

/// How many drawings [`starts`] gives: each has a test of its own below, so that no test runs more than a drawing's share of the grid.
const STARTS: usize = 9;

/// **A real edit after the Save As is still written, and what was left out stays out.** From each drawing, a Save As to each type with each of the options, then a shape moved (the first,
/// which shows the object where there is one, and the last, a rectangle) and a Save: the edit is written, the file read again has the shape where the drawing has it, and the file holds nothing
/// that the file the Save As wrote did not (no object or macro project is put back); the save after that is no edit. A `.diacraft` holds the whole drawing and is not told apart.
///
/// **One test for each drawing** (`edit_grid_tests!`: the grid is 9 drawings by 4 types by 4 options by 2 shapes, 288 chains, each writing a file twice and reading it back): the whole grid in one
/// test took 44 to 100 seconds alone on a busy machine, nearly all of it waiting for the disk (the file system's commits: the same test with its files on a RAM disk took 9 seconds, 5.7 of
/// them the CPU's), and nextest ends a test at 120. Each test runs `at`'s share, a count of chains that does not move with the load (32 at most, 24 at the least); the whole grid is the ignored
/// `the_whole_grid_of_edits_after_a_save_as_that_left_things_out`.
fn edit_grid(only: usize) {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut chains = 0;
    let all = starts(&dir);
    assert_eq!(all.len(), STARTS, "a drawing was added to `starts`: add its test to `edit_grid_tests!`, and raise STARTS");
    for (at, (what, start)) in all.into_iter().enumerate().filter(|(at, _)| *at == only) {
        for target in TARGETS {
            for (word, asked) in asks() {
                for (which, last) in [("first", false), ("last", true)] {
                    let label = format!("{what}, saved as .{target} with {word}, the {which} shape moved");
                    let name = format!("edit-{at}-{word}-{which}.{target}");
                    let mut s = start();
                    let Ok(Some(first)) = save_as(&mut s, &dir, &name, &asked) else { continue };
                    chains += 1;
                    let shapes: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
                    let id = if last { shapes.last() } else { shapes.first() }.copied().unwrap();
                    run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
                    let saved = match s.execute("file.save", &json!({})) {
                        Ok(saved) => saved,
                        Err(e) => {
                            failures.push(format!("{label}: the edit was refused: {e}"));
                            continue;
                        }
                    };
                    let written = dir.read(&name);
                    if written == first || saved.get("edited").is_some_and(|edited| *edited == json!(false)) {
                        failures.push(format!("{label}: the edit was not written: {saved}"));
                        continue;
                    }
                    if target != "diacraft" {
                        let (was, now) = (holds(&name, &first), holds(&name, &written));
                        if (now.0 && !was.0) || (now.1 && !was.1) {
                            failures.push(format!(
                                "{label}: what was left out is in the file again (object {} to {}, macros {} to {})",
                                was.0, now.0, was.1, now.1
                            ));
                        }
                        // An edit does not write the object a second time, into any type (for the `.odg`; for the VSDX types, where an edit held an `.odg`'s object twice: the
                        // embedding, and the object's file that rode in DiaCraft's own part), and an object is held once.
                        let (had, has) = (copies_of_the_object(&name, &first), copies_of_the_object(&name, &written));
                        if has > had || has > 1 {
                            failures.push(format!("{label}: the object is in the file {has} times, the Save As wrote it {had}"));
                        }
                    }
                    let mut back = pinned();
                    run(&mut back, "file.open", json!({ "path": dir.path(&name) }));
                    let (want, got) = (pin(&s, id), pin(&back, id));
                    if (want.0 - got.0).abs() > 1e-6 || (want.1 - got.1).abs() > 1e-6 {
                        failures.push(format!("{label}: the file has the shape at {got:?}, the drawing at {want:?}"));
                    }
                    if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &written) {
                        failures.push(format!("{label}: {why}"));
                    }
                }
            }
        }
    }
    assert!(chains >= 24, "every type, option and shape of drawing {only}: {chains} chains");
    assert!(failures.is_empty(), "{} of {chains} chains failed:\n{}", failures.len(), failures.join("\n"));
}

/// One test for each drawing of [`starts`], each named for it and in its order: `edit_grid(n)` is the grid of the nth.
macro_rules! edit_grid_tests {
    ($($name:ident: $at:expr,)*) => {
        $(
            #[test]
            fn $name() {
                edit_grid($at);
            }
        )*
    };
}

edit_grid_tests! {
    an_edit_after_a_save_as_from_an_odg_with_an_object_and_a_macro_project_is_written_and_what_was_left_out_stays_out: 0,
    an_edit_after_a_save_as_from_an_odg_with_a_macro_project_is_written_and_what_was_left_out_stays_out: 1,
    an_edit_after_a_save_as_from_a_vsdm_with_an_object_and_an_odf_macro_project_is_written_and_what_was_left_out_stays_out: 2,
    an_edit_after_a_save_as_from_a_diacraft_with_an_object_and_a_macro_project_is_written_and_what_was_left_out_stays_out: 3,
    an_edit_after_a_save_as_from_a_vsdm_with_a_vba_project_is_written_and_what_was_left_out_stays_out: 4,
    an_edit_after_a_save_as_from_an_odg_that_carries_a_vba_project_is_written_and_what_was_left_out_stays_out: 5,
    an_edit_after_a_save_as_from_a_diacraft_of_two_rectangles_and_a_macro_project_is_written_and_what_was_left_out_stays_out: 6,
    an_edit_after_a_save_as_from_a_vsdx_with_a_user_section_before_a_layer_section_is_written_and_what_was_left_out_stays_out: 7,
    an_edit_after_a_save_as_from_a_vsdx_with_a_layer_section_before_a_user_section_is_written_and_what_was_left_out_stays_out: 8,
}

/// **The whole grid of the edits in one test**, which the test of each drawing above splits (`#[ignore]`: nextest ends a test at 120 seconds, and the 288 chains took 44 to 100 seconds where
/// the disk was busy). `cargo test -p diacraft-engine --test it drop_resave::the_whole_grid_of_edits_after_a_save_as_that_left_things_out -- --ignored`.
#[test]
#[ignore = "the whole grid of 288 chains in one test: a minute where the disk is busy"]
fn the_whole_grid_of_edits_after_a_save_as_that_left_things_out() {
    for at in 0..STARTS {
        edit_grid(at);
    }
}

/// **The options leave out what they are asked to, in every pair of formats** (the second bug of the report: a Save As from an `.odg` to another `.odg` ignored them). What is
/// *of the other format* is left out of a file of this one: the objects and macros of an `.odg` from a VSDX file, the objects and the macro project of the `.odg` format from an `.odg` that
/// carries a VBA project. A macro-enabled type holds a macro project, so a Save As to one keeps it. Each row: the drawing, the type, the options, and whether the file written holds an
/// object and a macro project.
#[test]
fn the_options_leave_out_what_the_other_format_put_in_the_file() {
    let dir = Dir::new();
    let both = json!({ "drop_objects": true, "drop_macros": true });
    let drop_macros = json!({ "drop_macros": true });
    let nothing = json!({});
    // (drawing, type, options, object held, macros held)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(usize, &str, &Value, bool, bool)> = vec![
        // The `.odg` with an object: its object and project are the `.odg`'s own in an `.odg`, and the other format's in a VSDX file.
        (0, "vsdx", &both, false, false),
        (0, "vsdm", &both, false, true),
        (0, "odg", &both, true, true),
        // The VBA project of a `.vsdm` is the other format's in an `.odg` and the type's own in a macro-enabled type.
        (4, "vsdx", &drop_macros, false, false),
        (4, "vsdm", &drop_macros, false, true),
        (4, "odg", &drop_macros, false, false),
        (4, "odg", &nothing, false, true),
        // The `.odg` that carries it: left out of another `.odg`, which was ignored.
        (5, "odg", &drop_macros, false, false),
        (5, "odg", &nothing, false, true),
        (5, "vsdx", &drop_macros, false, false),
        (5, "vsdm", &drop_macros, false, true),
    ];
    let starts = starts(&dir);
    let mut failures = Vec::new();
    for (at, (index, target, asked, object, macros)) in rows.into_iter().enumerate() {
        let (what, start) = &starts[index];
        let name = format!("left-{at}.{target}");
        let mut s = start();
        let label = format!("{what}, saved as .{target} with {asked}");
        match save_as(&mut s, &dir, &name, asked) {
            Ok(Some(written)) => {
                let held = holds(&name, &written);
                if held != (object, macros) {
                    failures.push(format!("{label}: the file holds (object, macros) {held:?}, not {:?}", (object, macros)));
                }
            }
            other => failures.push(format!("{label}: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **What a Save As left out is out of the file, and the drawing keeps it.** A `.diacraft` saved from the drawing after a Save As that left the object and the macro project out still holds
/// both, and the Save into the file the Save As wrote, with no edit and then with one, does not take them from the drawing.
#[test]
fn the_drawing_keeps_what_the_save_as_left_out() {
    let dir = Dir::new();
    let both = json!({ "drop_objects": true, "drop_macros": true });
    let compound = crate::across::compound();
    let mut failures = Vec::new();
    for (at, (what, start)) in starts(&dir).into_iter().enumerate().filter(|(at, _)| [0, 2, 3].contains(at)) {
        for target in ["vsdx", "vsdm", "odg"] {
            let name = format!("kept-{at}.{target}");
            let mut s = start();
            if !matches!(save_as(&mut s, &dir, &name, &both), Ok(Some(_))) {
                failures.push(format!("{what} as .{target}: not written"));
                continue;
            }
            // The first shape moved and saved: the drawing still holds the object and the project afterwards.
            let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).next_back().unwrap();
            run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.0 }));
            if let Err(e) = s.execute("file.save", &json!({})) {
                failures.push(format!("{what} as .{target}: the edit was refused: {e}"));
                continue;
            }
            let all = format!("kept-{at}-{target}.diacraft");
            run(&mut s, "file.save_as", json!({ "path": dir.path(&all) }));
            let mut t = pinned();
            run(&mut t, "file.open", json!({ "path": dir.path(&all) }));
            let kept = t.document().unwrap();
            let objects = kept.parts.iter().filter(|p| &*p.bytes == compound.as_slice()).count();
            let macro_parts = kept.parts.iter().filter(|p| p.name.starts_with("Basic/")).count();
            if objects == 0 || macro_parts == 0 {
                failures
                    .push(format!("{what} as .{target}: the drawing lost what the Save As left out (objects {objects}, macro parts {macro_parts})"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **An object written twice** (the first bug left open). A `.vsdm` that holds an embedded object, saved as `.odg` with the object left in, and saved again with no edit: the
/// model writer wrote the object as an object frame and took the VSDX embedding out of what rides, and the next save, spliced into that file, was given the embedding as riding. It counted as an edit
/// and wrote the object a second time, under DiaCraft's own folder. Now the Save is no edit and the same bytes, and an edit written after it is the one edit: the object is in the file once.
#[test]
fn a_vsdx_object_saved_as_an_odg_is_written_once_by_every_save_into_it() {
    let dir = Dir::new();
    let vsdm = made_as(&dir, "object-source.odg", &odg_full(), "object-made.vsdm");
    for (word, asked) in [("nothing", json!({})), ("drop_macros", json!({ "drop_macros": true }))] {
        let mut s = opened(&dir, "object.vsdm", &vsdm);
        let name = format!("once-{word}.odg");
        let first = save_as(&mut s, &dir, &name, &asked).unwrap().unwrap();
        assert_eq!(copies_of_the_object(&name, &first), 1, "{word}: the Save As writes the object once");
        saved_again_as_it_was(&mut s, &dir, &name, &first).unwrap_or_else(|why| panic!("{word}: {why}"));
        let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).next_back().unwrap();
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.0 }));
        let saved = run(&mut s, "file.save", json!({}));
        let written = dir.read(&name);
        assert!(written != first && saved["edited"] != json!(false), "{word}: the move is written: {saved}");
        assert_eq!(copies_of_the_object(&name, &written), 1, "{word}: the object is in the file once after an edit");
        saved_again_as_it_was(&mut s, &dir, &name, &written).unwrap_or_else(|why| panic!("{word}: after the edit, {why}"));
    }
}

/// **A refused Save after a crossing** (the second bug left open). A `.diacraft` drawing that holds an `.odg`'s object frames, saved as a VSDX type with nothing left out, and
/// saved again with no edit: the VSDX writer is given each frame as the picture it shows, with the box of a picture, which the file reads back and the drawing does not hold, and the splice writer
/// refused "a change to the picture of a shape". Only a drawing read from an `.odg` adopted the model the file reads as. Now the Save is no edit, the same bytes, and an edit is written.
#[test]
fn a_diacraft_drawing_of_odg_frames_saved_as_a_vsdx_type_is_saved_again_as_it_was_and_an_edit_is_written() {
    let dir = Dir::new();
    let native = made_as(&dir, "frames-source.odg", &odg_full(), "frames-made.diacraft");
    for target in ["vsdm", "vsdx"] {
        // (A macro project is of the `.odg`'s own format and rides in a macro-enabled type only: the macro-free type is asked to leave it out, which holds the objects.)
        let asked = if target == "vsdx" { json!({ "drop_macros": true }) } else { json!({}) };
        let mut s = opened(&dir, "frames.diacraft", &native);
        let name = format!("frames.{target}");
        let first = save_as(&mut s, &dir, &name, &asked).unwrap().unwrap();
        saved_again_as_it_was(&mut s, &dir, &name, &first).unwrap_or_else(|why| panic!(".{target}: {why}"));
        let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).next_back().unwrap();
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!(".{target}: the edit was refused: {e}"));
        assert!(dir.read(&name) != first && saved["edited"] != json!(false), ".{target}: the move is written: {saved}");
        let mut back = pinned();
        run(&mut back, "file.open", json!({ "path": dir.path(&name) }));
        let (want, got) = (pin(&s, id), pin(&back, id));
        assert!(
            (want.0 - got.0).abs() < 1e-6 && (want.1 - got.1).abs() < 1e-6,
            ".{target}: the file has the shape at {got:?}, the drawing at {want:?}"
        );
    }
}

/// A new drawing of two rectangles with text, the clock pinned.
fn new_drawing() -> Session {
    let mut s = pinned();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    for (k, x) in [2.0, 5.0].into_iter().enumerate() {
        let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": x, "y": 4.0 }));
        run(&mut s, "shape.set_text", json!({ "id": made["id"], "text": format!("Box {k}") }));
    }
    s
}

/// **A new drawing's saves are what they were** (the gate that makes a drawing the model its VSDX file reads as is widened to a `.diacraft` drawing that holds frames, and to nothing
/// else). A new drawing saved to each type and saved again: the drawing in the session is the one the person has (never a model adopted from the file: the same allocation before and after), the
/// Save after it is no edit, and writes the first save's bytes. A VSDX type is written from the model alone, where the gate is.
#[test]
fn a_new_drawings_first_and_second_saves_are_not_changed_by_what_adopts_the_model_the_file_reads_as() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut written = 0;
    for target in ["vsdx", "vsdm", "vstx", "vstm", "vssx", "vssm", "odg", "otg", "diacraft"] {
        let mut s = new_drawing();
        if !s.save_as_allows(target) {
            continue;
        }
        let drawing = std::ptr::from_ref(s.document().unwrap());
        let name = format!("new.{target}");
        let first = match save_as(&mut s, &dir, &name, &json!({})) {
            Ok(Some(bytes)) => bytes,
            other => {
                failures.push(format!(".{target}: the Save As is not written: {:?}", other.map(|saved| saved.map(|bytes| bytes.len()))));
                continue;
            }
        };
        written += 1;
        if !std::ptr::eq(drawing, s.document().unwrap()) {
            failures.push(format!(".{target}: the drawing became another model after the first save"));
        }
        if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &first) {
            failures.push(format!(".{target}: {why}"));
        }
        if !std::ptr::eq(drawing, s.document().unwrap()) {
            failures.push(format!(".{target}: the drawing became another model after the second save"));
        }
    }
    assert!(written >= 7, "the types a new drawing is saved as: {written}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **A page sheet's sections are no edit by their order** (from the gate's sample-file check: one of three sampled drawings that cross to `.odg` answered `edited: true` to an unedited Save,
/// with the bytes the Save As wrote). A `.vsdx` whose page sheet has a `User` section before a `Layer` section, saved as `.odg` and saved again with no edit, is no edit and the same bytes, as it is
/// with the `Layer` section first; an edit after it is written, and the save after that is no edit.
#[test]
fn a_page_sheet_with_its_user_section_before_its_layer_section_is_no_edit_after_a_save_as_to_an_odg() {
    for (word, layer_first) in [("user before layer", false), ("layer before user", true)] {
        let dir = Dir::new();
        let mut s = opened(&dir, "page.vsdx", &vsdx_page_sections(layer_first));
        let first = save_as(&mut s, &dir, "page.odg", &json!({})).unwrap().unwrap();
        saved_again_as_it_was(&mut s, &dir, "page.odg", &first).unwrap_or_else(|why| panic!("{word}: {why}"));
        let id = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).next_back().unwrap();
        run(&mut s, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.5 }));
        let saved = run(&mut s, "file.save", json!({}));
        let written = dir.read("page.odg");
        assert!(written != first && saved["edited"] != json!(false), "{word}: the move is written: {saved}");
        saved_again_as_it_was(&mut s, &dir, "page.odg", &written).unwrap_or_else(|why| panic!("{word}: after the edit, {why}"));
    }
}

/// The six types of a VSDX file, as the extension of the name a Save As writes.
const VSDX_TYPES: [&str; 6] = ["vsdx", "vsdm", "vstx", "vstm", "vssx", "vssm"];

/// What a Save As to `target` of a drawing that holds the macro project of an `.odg` is asked: a macro-free type is asked to leave the project out (it is of the other format), and a
/// macro-enabled type holds it. The object is left in.
fn keeps_the_object(target: &str) -> Value {
    if matches!(target, "vsdm" | "vstm" | "vssm") { json!({}) } else { json!({ "drop_macros": true }) }
}

/// The first shape moved by (`dx`, `dy`) and the Save after it: the answer of the save, and the bytes of the file.
fn moved_and_saved(s: &mut Session, dir: &Dir, name: &str, which_last: bool, dx: f64) -> Result<(Value, Vec<u8>), String> {
    let shapes: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect();
    let id = if which_last { shapes.last() } else { shapes.first() }.copied().unwrap();
    run(s, "shape.move", json!({ "ids": [id], "dx": dx, "dy": 0.5 }));
    let saved = s.execute("file.save", &json!({})).map_err(|e| format!("the edit was refused: {e}"))?;
    Ok((saved, dir.read(name)))
}

/// **An object held twice after an edit** (the case found and left). A drawing of an `.odg` or a `.diacraft` file that holds an object frame, saved as a VSDX type with the
/// object left in (a `.vsdx`, `.vstx` or `.vssx` with `drop_macros`, which is the macro project of the other format going, and a macro-enabled type with nothing left out): the file holds the
/// object once, as an embedding. A Save with no edit is no edit, and **a Save after an edit, which writes the pages again, wrote the object a second time**, in the object's file that rides in
/// DiaCraft's own part. Now every save into the file holds it once, an edit is written, the file read again has the frame an object and at the place the drawing has it, and the save after the
/// edit is no edit.
#[test]
fn an_object_of_an_odg_saved_as_a_vsdx_type_is_held_once_by_every_save_into_it() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut chains = 0;
    for (at, (what, start)) in starts(&dir).into_iter().enumerate().filter(|(at, _)| [0, 3].contains(at)) {
        for target in VSDX_TYPES {
            for which_last in [false, true] {
                let asked = keeps_the_object(target);
                let name = format!("held-{at}-{which_last}.{target}");
                let label = format!("{what}, saved as .{target}, the {} shape moved", if which_last { "last" } else { "first" });
                let mut s = start();
                let first = match save_as(&mut s, &dir, &name, &asked) {
                    Ok(Some(first)) => first,
                    other => {
                        failures.push(format!("{label}: the Save As is not written: {:?}", other.map(|saved| saved.map(|bytes| bytes.len()))));
                        continue;
                    }
                };
                chains += 1;
                let copies = copies_of_the_object(&name, &first);
                if copies != 1 {
                    failures.push(format!("{label}: the Save As wrote the object {copies} times"));
                    continue;
                }
                if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &first) {
                    failures.push(format!("{label}: {why}"));
                    continue;
                }
                // Two edits, each saved: the object is in the file once after each, and the save after each is no edit.
                let mut last = first;
                for (round, dx) in [(1, 1.0), (2, -0.5)] {
                    let (saved, written) = match moved_and_saved(&mut s, &dir, &name, which_last, dx) {
                        Ok(done) => done,
                        Err(why) => {
                            failures.push(format!("{label}: edit {round}: {why}"));
                            break;
                        }
                    };
                    if written == last || saved["edited"] == json!(false) {
                        failures.push(format!("{label}: edit {round}: the edit was not written: {saved}"));
                        break;
                    }
                    let copies = copies_of_the_object(&name, &written);
                    if copies != 1 {
                        failures.push(format!("{label}: edit {round}: the object is in the file {copies} times"));
                    }
                    if !holds(&name, &written).0 {
                        failures.push(format!("{label}: edit {round}: the object is gone from the file"));
                    }
                    if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &written) {
                        failures.push(format!("{label}: edit {round}: {why}"));
                    }
                    last = written;
                }
            }
        }
    }
    assert!(chains >= 24, "every drawing, type and shape: {chains} chains");
    assert!(failures.is_empty(), "{} of {chains} chains failed:\n{}", failures.len(), failures.join("\n"));
}

/// **What the file remembers of its objects goes on to the next file.** The object is held once by the file a Save As wrote, by the file a second Save As writes from it (a splice of the first file's
/// bytes, to another name, and in another VSDX type), and by every save into that one, edited or not: the parts the first save wrote as embeddings are told to the saves after it, whichever file
/// they write.
#[test]
fn an_object_is_held_once_by_a_second_file_written_from_the_first_and_by_every_save_into_it() {
    let dir = Dir::new();
    for (at, (what, start)) in starts(&dir).into_iter().enumerate().filter(|(at, _)| [0, 3].contains(at)) {
        for (first, second) in [("vsdm", "vsdm"), ("vsdm", "vstm")] {
            let label = format!("{what}, saved as .{first} and then as .{second}");
            let mut s = start();
            let (one, two) = (format!("twice-{at}-{first}-1.{first}"), format!("twice-{at}-{second}-2.{second}"));
            let written = save_as(&mut s, &dir, &one, &json!({})).unwrap().unwrap();
            assert_eq!(copies_of_the_object(&one, &written), 1, "{label}: the first file");
            let (_, edited) = moved_and_saved(&mut s, &dir, &one, false, 1.0).unwrap_or_else(|why| panic!("{label}: {why}"));
            assert_eq!(copies_of_the_object(&one, &edited), 1, "{label}: the first file after an edit");
            let other = save_as(&mut s, &dir, &two, &json!({})).unwrap().unwrap();
            assert_eq!(copies_of_the_object(&two, &other), 1, "{label}: the file a second Save As wrote from the first");
            saved_again_as_it_was(&mut s, &dir, &two, &other).unwrap_or_else(|why| panic!("{label}: {why}"));
            let (_, edited) = moved_and_saved(&mut s, &dir, &two, true, -0.5).unwrap_or_else(|why| panic!("{label}: {why}"));
            assert_eq!(copies_of_the_object(&two, &edited), 1, "{label}: the second file after an edit");
            assert!(holds(&two, &edited).0, "{label}: and it is still an object");
        }
    }
}

/// The `.otg` template of the frame of [`odg_full`] and its macro project, as bytes.
fn otg_full() -> Vec<u8> {
    macro_files(
        Drawing::new()
            .page(&format!("{}{}", crate::across::object_frame(), rect("R", (2.0, 8.0, 4.0, 2.0), "")))
            .file("Object 1", &crate::across::compound())
            .file("ObjectReplacements/Object 1", &crate::across::png(10)),
    )
    .bytes_of_type(crate::odg_build::TEMPLATE)
}

/// **A refused Save after a template drawing crosses to VSDX** (the case found and left). A drawing made from an `.otg` template that holds an `.odg`'s object frame has no
/// working file, and the gate that makes the drawing what its VSDX file reads as did not take it: the VSDX writer is given the frame as the picture it shows, with the box cells of a picture,
/// which the file reads back and the drawing lacks, and the next Save was refused "a change to the picture of a shape". Now, in each of the six VSDX types, the Save as it was is no edit and the
/// same bytes, an edit is written (with the object in the file once), and the save after it is no edit.
#[test]
fn a_template_drawing_of_odg_frames_saved_as_a_vsdx_type_is_saved_again_as_it_was_and_an_edit_is_written() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut chains = 0;
    for target in VSDX_TYPES {
        for which_last in [false, true] {
            let name = format!("template-{which_last}.{target}");
            let label = format!("an .otg drawing saved as .{target}, the {} shape moved", if which_last { "last" } else { "first" });
            let mut s = opened(&dir, "frames.otg", &otg_full());
            assert!(s.source().is_none(), "a drawing made from a template has no working file");
            if !s.save_as_allows(target) {
                continue;
            }
            let first = match save_as(&mut s, &dir, &name, &keeps_the_object(target)) {
                Ok(Some(first)) => first,
                other => {
                    failures.push(format!("{label}: the Save As is not written: {:?}", other.map(|saved| saved.map(|bytes| bytes.len()))));
                    continue;
                }
            };
            chains += 1;
            if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &first) {
                failures.push(format!("{label}: {why}"));
                continue;
            }
            let (saved, written) = match moved_and_saved(&mut s, &dir, &name, which_last, 1.0) {
                Ok(done) => done,
                Err(why) => {
                    failures.push(format!("{label}: {why}"));
                    continue;
                }
            };
            if written == first || saved["edited"] == json!(false) {
                failures.push(format!("{label}: the edit was not written: {saved}"));
                continue;
            }
            let copies = copies_of_the_object(&name, &written);
            if copies != 1 {
                failures.push(format!("{label}: the object is in the file {copies} times after the edit"));
            }
            if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &written) {
                failures.push(format!("{label}: after the edit, {why}"));
            }
        }
    }
    assert!(chains >= 12, "every type and shape: {chains} chains");
    assert!(failures.is_empty(), "{} of {chains} chains failed:\n{}", failures.len(), failures.join("\n"));
}

/// The ways to make a drawing that has no working file and holds no picture of an `.odg`: a new drawing of two rectangles with text, a drawing made from an `.otg` template of two rectangles, one
/// made from a `.vstx` template, and **a new drawing with a picture inserted from a file** (its second Save into a VSDX file was refused "a change to the picture of a shape"; the picture
/// the drawing holds by its bytes is the one the file names by a row of its own, `patch::shows_the_same`, and the drawing is still not made another model).
fn drawings_with_no_file_and_no_odg_picture(dir: &Dir) -> Vec<(&'static str, Start<'_>)> {
    let vstx = VsdxBuilder::new()
        .document_type(diacraft_vsdx::Variant::from_extension("vstx").unwrap().content_type())
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shape(1, &rect_cells(2.0))))
        .build();
    let rects = format!("{}{}", rect("R", (2.0, 8.0, 4.0, 2.0), ""), rect("S", (8.0, 8.0, 4.0, 2.0), ""));
    let otg = Drawing::new().page(&rects).bytes_of_type(crate::odg_build::TEMPLATE);
    vec![
        ("a new drawing", Box::new(new_drawing)),
        ("a drawing made from an .otg of two rectangles", Box::new(move || opened(dir, "rects.otg", &otg))),
        ("a drawing made from a .vstx", Box::new(move || opened(dir, "template.vstx", &vstx))),
        ("a new drawing with a picture inserted from a file", Box::new(move || with_a_picture(dir))),
    ]
}

/// A new drawing of two rectangles with text and a picture inserted from a file.
fn with_a_picture(dir: &Dir) -> Session {
    let mut s = new_drawing();
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let path = dir.write("inserted.png", &crate::across::png(91));
    run(&mut s, "insert.picture", json!({ "path": path, "page": page, "x": 4.0, "y": 6.0 }));
    s
}

/// **A blank drawing's first and second saves are unchanged by the gate that adopts the model the file reads as** (the gate takes a drawing with no working file when the writer was given
/// pictures an `.odg` held, so that a template's frames are saved again as they were, and takes none that holds no such picture). Each drawing that has no file and holds no picture of an `.odg`,
/// saved as each type: the drawing in the session is the allocation it was before the save and after the second (never the model the file reads as), the second save is no edit, and writes the
/// first save's bytes.
#[test]
fn a_drawing_with_no_file_and_no_odg_picture_is_not_made_another_model_by_a_save_to_any_type() {
    let dir = Dir::new();
    let mut failures = Vec::new();
    let mut written = 0;
    for (at, (what, start)) in drawings_with_no_file_and_no_odg_picture(&dir).into_iter().enumerate() {
        for target in ["vsdx", "vsdm", "vstx", "vstm", "vssx", "vssm", "odg", "otg", "diacraft"] {
            let mut s = start();
            if !s.save_as_allows(target) {
                continue;
            }
            let drawing = std::ptr::from_ref(s.document().unwrap());
            let name = format!("blank-{at}.{target}");
            let label = format!("{what}, saved as .{target}");
            let first = match save_as(&mut s, &dir, &name, &json!({})) {
                Ok(Some(bytes)) => bytes,
                other => {
                    failures.push(format!("{label}: the Save As is not written: {:?}", other.map(|saved| saved.map(|bytes| bytes.len()))));
                    continue;
                }
            };
            written += 1;
            if !std::ptr::eq(drawing, s.document().unwrap()) {
                failures.push(format!("{label}: the drawing became another model after the first save"));
            }
            if let Err(why) = saved_again_as_it_was(&mut s, &dir, &name, &first) {
                failures.push(format!("{label}: {why}"));
            }
            if !std::ptr::eq(drawing, s.document().unwrap()) {
                failures.push(format!("{label}: the drawing became another model after the second save"));
            }
        }
    }
    assert!(written >= 27, "the drawings and the types they are saved as: {written}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
