//! Opening a stencil file and dropping its masters. Every package is built in code from the specifications' XML
//! No stencil from another application is used. A saved drawing with shapes dropped from an
//! opened stencil is checked with LibreOffice by hand and reported, not here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, Master, Owner, Resolver, SheetRef};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// The document content type of a stencil and of a stencil that allows macros.
pub const CT_STENCIL: &str = "application/vnd.ms-visio.stencil.main+xml"; // brand-ok: a content type the format defines
const CT_STENCIL_MACRO: &str = "application/vnd.ms-visio.stencil.macroEnabled.main+xml"; // brand-ok: a content type the format defines
const CT_TEMPLATE: &str = "application/vnd.ms-visio.template.main+xml"; // brand-ok: a content type the format defines

/// A scratch folder under `TMPDIR`, removed on drop.
pub struct Dir(PathBuf);

impl Dir {
    pub fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-stencil-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> String {
        std::fs::write(self.0.join(name), bytes).unwrap();
        self.path(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn guid(n: u32) -> String {
    format!("{{00000000-0000-4000-8000-{n:012}}}")
}

/// The attributes of a master row: its two GUIDs.
pub fn ids(unique: u32, base: u32) -> String {
    format!(r#" UniqueID="{}" BaseID="{}""#, guid(unique), guid(base))
}

/// The rows of a box `w` by `h` as a file writes them: the geometry as formulas of the size, and a connection point in the middle of each side.
pub fn box_cells(w: f64, h: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{px}" F="Width*0.5"/><Cell N="PinY" V="{py}" F="Height*0.5"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/>
<Cell N="LocPinX" V="{px}" F="Width*0.5"/><Cell N="LocPinY" V="{py}" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="{px}" F="Width*0.5"/><Cell N="Y" V="{h}" F="Height"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="{py}" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row>
<Row IX="2"><Cell N="X" V="{px}" F="Width*0.5"/><Cell N="Y" V="0"/><Cell N="DirX" V="0"/><Cell N="DirY" V="-1"/></Row>
<Row IX="3"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="{py}" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row></Section>"#,
        px = w / 2.0,
        py = h / 2.0
    )
}

/// Line, fill and text cells that make a page draw a shape: the stencils of a test have a root style that holds them.
pub const LOOK: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.01"/><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="#E8F0FF"/>"##;

/// The master "Process" (ID 1): a box 1.5 by 0.8 of a shape linked to style 3, with a custom colour and a face of its own.
pub fn process_master() -> MasterSpec {
    let inner = format!(
        r#"{}<Cell N="FillForegnd" V="25"/><Section N="Character"><Row IX="0"><Cell N="Font" V="Segoe Script"/><Cell N="Size" V="0.1666667"/></Row></Section>"#,
        box_cells(1.5, 0.8)
    );
    MasterSpec::new(1, "Process").attrs(&ids(101, 201)).shapes(&shape_with(5, r#" LineStyle="3" FillStyle="3" TextStyle="3""#, &inner))
}

/// The master "Decision" (ID 2): a box 1.5 by 1, no style link.
pub fn decision_master() -> MasterSpec {
    MasterSpec::new(2, "Decision").attrs(&ids(102, 202)).shapes(&shape(5, &format!("{LOOK}{}", box_cells(1.5, 1.0))))
}

/// The master "Link" (ID 3): a one-dimensional line from (0, 0) to (2, 0), as a file writes one.
pub fn line_master() -> MasterSpec {
    let inner = r#"<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="2"/><Cell N="EndY" V="0"/>
<Cell N="PinX" V="1" F="(BeginX+EndX)/2"/><Cell N="PinY" V="0" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.01"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#;
    MasterSpec::new(3, "Link").attrs(&ids(103, 203)).shapes(&shape(5, inner))
}

/// The styles of the stencil: 3 "Flow Normal" based on 4 "Flow Base" (each link the file's three, here all to 4 and then to the root).
pub const STENCIL_STYLES: &str = r#"<StyleSheet ID="4" NameU="Flow Base" Name="Flow Base" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.0138889"/><Cell N="LineColor" V="2"/></StyleSheet>
<StyleSheet ID="3" NameU="Flow Normal" Name="Flow Normal" LineStyle="4" FillStyle="4" TextStyle="4"><Cell N="FillPattern" V="1"/><Cell N="FillForegnd" V="26"/><Cell N="LinePattern" V="1"/></StyleSheet>"#;

pub const STENCIL_COLORS: &str = r##"<ColorEntry IX="25" RGB="#3366CC"/><ColorEntry IX="26" RGB="#FFCC00"/>"##;
pub const STENCIL_FACES: &str = r#"<FaceName NameU="Segoe Script" Panose="2 11 6 4 2 2 2 2 2 4" Flags="1"/><FaceName NameU="Arial"/>"#;

/// A stencil with Process, Decision and Link.
pub fn stencil_builder() -> VsdxBuilder {
    VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .colors(STENCIL_COLORS)
        .face_names(STENCIL_FACES)
        .more_styles(STENCIL_STYLES)
        .master(process_master())
        .master(decision_master())
        .master(line_master())
        .page(PageSpec::new(1, "Page-1").without_part())
}

pub fn stencil_bytes() -> Vec<u8> {
    stencil_builder().build()
}

pub fn open(s: &mut Session, path: &str) -> Value {
    run(s, "stencil.open", json!({"path": path}))
}

pub fn refusal(s: &mut Session, id: &str, params: Value) -> String {
    let before = snapshot(s);
    let e = s.execute(id, &params).unwrap_err();
    assert_eq!(snapshot(s), before, "a refused {id} changes nothing");
    assert_eq!(check_session(s), Ok(()));
    e.to_string()
}

pub fn master_named<'a>(doc: &'a Document, name: &str) -> Option<&'a Master> {
    doc.masters.iter().map(|m| &**m).find(|m| m.name.as_deref() == Some(name))
}

pub fn masters_named(doc: &Document, name: &str) -> usize {
    doc.masters.iter().filter(|m| m.name.as_deref() == Some(name)).count()
}

pub fn num(d: &Document, page: u32, id: u32, cell: &str) -> f64 {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(page), id).unwrap());
    Resolver::new(d)
        .resolve(&sheet, &CellAddr::direct(cell))
        .and_then(|f| f.value.cell?.value.as_ref()?.number())
        .unwrap_or_else(|| panic!("{id}.{cell}"))
}

/// A session with an open stencil `file-1` (the built-in stencil's ids are words) and a blank drawing.
pub fn with_stencil() -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.write("shapes.vssx", &stencil_bytes());
    let mut s = blank_session();
    open(&mut s, &path);
    (s, dir)
}

// ------------------------------------------------------------------------------------------------------------ open, list and close

#[test]
fn a_stencil_file_opens_and_its_masters_are_listed_with_names_sizes_and_one_d() {
    let dir = Dir::new();
    let path = dir.write("Flow Parts.vssx", &stencil_bytes());
    let mut s = Session::new();
    let opened = open(&mut s, &path);
    assert_eq!(opened["id"], "file-1");
    assert_eq!(opened["name"], "Flow Parts", "the name is the file's, without its type");
    assert_eq!(opened["already_open"], false);
    assert_eq!(opened["file"]["variant"], "vssx");
    let masters = opened["masters"].as_array().unwrap();
    let names: Vec<&str> = masters.iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Process", "Decision", "Link"]);
    assert_eq!(masters[0]["id"], "1", "ids are text, as the built-in stencils' are");
    assert_eq!((masters[0]["width"].as_f64(), masters[0]["height"].as_f64()), (Some(1.5), Some(0.8)));
    assert_eq!(masters[0]["connection_points"], 4);
    assert_eq!((masters[0]["one_d"].as_bool(), masters[1]["one_d"].as_bool(), masters[2]["one_d"].as_bool()), (Some(false), Some(false), Some(true)));
    assert_eq!(masters[0]["hidden"], false);
    assert_eq!(masters[0]["icon"], Value::Null, "this master has no icon");
    // The listing needs no drawing, names the built-in stencils first, and says which stencils are files.
    let list = run(&mut s, "stencil.list", json!({}));
    let ids: Vec<&str> = list["stencils"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart", "file-1"]);
    assert_eq!(list["stencils"][0]["builtin"], true);
    assert_eq!(list["stencils"][7]["builtin"], false);
    assert_eq!(list["stencils"][7]["masters"], opened["masters"]);
    assert_eq!(list["stencils"][0]["masters"].as_array().unwrap().len(), 12, "the built-in listing is as it was");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_stencil_is_closed_by_its_id_and_ids_are_not_given_again() {
    let (mut s, dir) = with_stencil();
    let msg = refusal(&mut s, "stencil.close", json!({"id": "file-9"}));
    assert!(msg.contains("file-9") && msg.contains("stencil.list"), "{msg}");
    let msg = refusal(&mut s, "stencil.close", json!({"id": "basic"}));
    assert!(msg.contains("built-in"), "{msg}");
    run(&mut s, "stencil.close", json!({"id": "file-1"}));
    let list = run(&mut s, "stencil.list", json!({}));
    assert_eq!(list["stencils"].as_array().unwrap().len(), 7);
    let again = open(&mut s, &dir.path("shapes.vssx"));
    assert_eq!(again["id"], "file-2", "an id that was a stencil's is not given again in the session");
    // Closing a stencil does not touch a drawing that took masters from it.
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-2", "page": 1, "x": 3, "y": 3}));
    run(&mut s, "stencil.close", json!({"id": "file-2"}));
    assert!(master_named(s.document().unwrap(), "Process").is_some());
}

#[test]
fn opening_the_same_file_again_answers_the_stencil_that_is_open() {
    let (mut s, dir) = with_stencil();
    let again = open(&mut s, &dir.path("shapes.vssx"));
    assert_eq!(again["id"], "file-1");
    assert_eq!(again["already_open"], true);
    assert_eq!(run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len(), 8);
}

#[test]
fn a_stencil_that_allows_macros_opens_and_its_macros_are_never_read() {
    let dir = Dir::new();
    let pkg = stencil_builder()
        .document_type(CT_STENCIL_MACRO)
        .pkg()
        // Not a real project: the bytes are junk, and nothing may try to read them.
        .part("/visio/vbaProject.bin", "application/vnd.ms-office.vbaProject", [0xD0u8, 0xCF, 0x11, 0xE0, 1, 2, 3]);
    let path = dir.write("macros.vssm", &pkg.build());
    let mut s = Session::new();
    let opened = open(&mut s, &path);
    assert_eq!(opened["file"]["variant"], "vssm");
    assert_eq!(opened["masters"].as_array().unwrap().len(), 3);
}

#[test]
fn a_file_that_cannot_be_opened_as_a_stencil_is_refused_in_words_with_nothing_changed() {
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    let drawing = dir.write("plan.vsdx", &VsdxBuilder::new().master(process_master()).page(PageSpec::new(1, "Page-1")).build());
    let template = dir.write("plan.vstx", &VsdxBuilder::new().document_type(CT_TEMPLATE).master(process_master()).build());
    let garbage = dir.write("junk.vssx", b"this is not a zip archive at all");
    let mut cut = stencil_bytes();
    cut.truncate(cut.len() / 2);
    let truncated = dir.write("cut.vssx", &cut);
    std::fs::create_dir(dir.path("folder.vssx")).unwrap();
    let cases = [
        (dir.path("nothing-here.vssx"), "no such file"),
        (dir.path("folder.vssx"), "not a file"),
        (garbage, "not a drawing file"),
        (truncated, "damaged"),
        (drawing, "not a stencil"),
        (template, "not a stencil"),
    ];
    for (path, words) in cases {
        let msg = refusal(&mut s, "stencil.open", json!({"path": path}));
        assert!(msg.contains(words), "{path}: {msg}");
        assert!(!msg.contains(&path) && !msg.contains("diacraft-engine-stencil"), "the path is not in the words: {msg}");
    }
    assert_eq!(run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len(), 7, "nothing was opened");
    let msg = refusal(&mut s, "stencil.open", json!({}));
    assert!(msg.contains("path"), "{msg}");
    let msg = refusal(&mut s, "stencil.open", json!({"path": 5}));
    assert!(msg.contains("text"), "{msg}");
    let msg = refusal(&mut s, "stencil.open", json!({"path": "a\u{0}b"}));
    assert!(msg.contains("no such file"), "{msg}");
}

#[cfg(unix)]
#[test]
fn a_link_to_a_stencil_is_read_and_a_pipe_is_refused_without_blocking() {
    let dir = Dir::new();
    let real = dir.write("real.vssx", &stencil_bytes());
    std::os::unix::fs::symlink(&real, dir.path("link.vssx")).unwrap();
    let mut s = Session::new();
    let opened = open(&mut s, &dir.path("link.vssx"));
    assert_eq!(opened["masters"].as_array().unwrap().len(), 3, "a link to a stencil is read, as `file.open` reads it");
    assert_eq!(opened["name"], "link");
    // The same file by its real name is the stencil that is open.
    assert_eq!(open(&mut s, &real)["already_open"], true);
    // A link to a folder and a link that leads nowhere are refused.
    std::os::unix::fs::symlink(dir.path(""), dir.path("folder-link.vssx")).unwrap();
    std::os::unix::fs::symlink(dir.path("gone"), dir.path("dangling.vssx")).unwrap();
    for name in ["folder-link.vssx", "dangling.vssx"] {
        let msg = refusal(&mut s, "stencil.open", json!({"path": dir.path(name)}));
        assert!(msg.contains("not a file") || msg.contains("no such file"), "{name}: {msg}");
    }
    let fifo = dir.path("pipe.vssx");
    if std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|st| st.success()) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut s = Session::new();
            let _ = tx.send(s.execute("stencil.open", &json!({"path": fifo})).map_err(|e| e.to_string()));
        });
        let answer = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("opening a pipe blocked");
        assert!(answer.unwrap_err().contains("not a file"));
    }
}

#[test]
fn only_so_many_stencils_stay_open_and_the_refusal_says_so() {
    let dir = Dir::new();
    let bytes = stencil_bytes();
    let mut s = Session::new();
    let mut refused = None;
    for i in 0..40 {
        let path = dir.write(&format!("s{i}.vssx"), &bytes);
        match s.execute("stencil.open", &json!({"path": path})) {
            Ok(_) => {}
            Err(e) => {
                refused = Some((i, e.to_string()));
                break;
            }
        }
    }
    let (at, msg) = refused.expect("a limit");
    assert_eq!(at, 16, "sixteen stencils open, the seventeenth is refused");
    assert!(msg.contains("16") && msg.contains("stencil.close"), "{msg}");
    run(&mut s, "stencil.close", json!({"id": "file-1"}));
    open(&mut s, &dir.path("s16.vssx"));
}

#[test]
fn a_failed_batch_closes_the_stencil_it_opened() {
    let dir = Dir::new();
    let path = dir.write("shapes.vssx", &stencil_bytes());
    let mut s = blank_session();
    let before = snapshot(&s);
    let e = s
        .execute(
            "command.batch",
            &json!({"commands": [{"command": "stencil.open", "params": {"path": path}}, {"command": "shape.drop", "params": {"master": "nope", "page": 1, "x": 1, "y": 1}}]}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("step 1"), "{e}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(run(&mut s, "stencil.list", json!({}))["stencils"].as_array().unwrap().len(), 7, "the stencil the batch opened is closed again");
}

// ----------------------------------------------------------------------------------------------------------------------------- drops

#[test]
fn a_drop_copies_the_master_into_the_document_and_the_instance_holds_only_its_pin() {
    let (mut s, _dir) = with_stencil();
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3.0, "y": 4.5}));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1.5), Some(0.8)));
    assert_eq!(r["name"], "Process.1");
    assert_eq!(r["stencil"], "file-1");
    assert_eq!(r["master_added"], true);
    let d = s.document().unwrap();
    let m = master_named(d, "Process").expect("the master was copied in");
    assert_eq!(r["master"], m.id);
    assert_eq!(m.unique_id.as_deref(), Some(guid(101).as_str()), "a copy keeps its UniqueID");
    assert_eq!(m.raw.get("BaseID"), Some(guid(201).as_str()), "and its BaseID");
    assert_eq!(m.rel_id, None, "the stencil's relationship is not the document's");
    let page = d.page(1).unwrap();
    let inst = &page.shapes.get(0).unwrap();
    assert_eq!((inst.id, inst.master), (r["id"].as_u64().unwrap() as u32, Some(m.id)));
    let held: Vec<&str> = inst.sheet.cells.keys().map(|k| k.as_str()).collect();
    assert_eq!(held, ["PinX", "PinY"], "an instance holds what differs");
    assert_eq!((num(d, 1, inst.id, "PinX"), num(d, 1, inst.id, "PinY")), (3.0, 4.5));
    assert_eq!((num(d, 1, inst.id, "Width"), num(d, 1, inst.id, "Height")), (1.5, 0.8), "the rest is inherited");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_centre_goes_where_it_is_asked_whatever_the_masters_pin() {
    let dir = Dir::new();
    // A box 2 by 1 whose pin is its lower-left corner and that is turned a quarter: its centre is (1, 0.5) from the pin, turned to (-0.5, 1).
    let inner = r#"<Cell N="PinX" V="0"/><Cell N="PinY" V="0"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>
<Cell N="Angle" V="1.5707963267949"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#;
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Corner").shapes(&shape(5, inner))).build();
    let path = dir.write("corner.vssx", &bytes);
    let mut s = blank_session();
    open(&mut s, &path);
    let r = run(&mut s, "shape.drop", json!({"master": "Corner", "stencil": "file-1", "page": 1, "x": 5.0, "y": 4.0}));
    let d = s.document().unwrap();
    let id = r["id"].as_u64().unwrap() as u32;
    assert!(
        (num(d, 1, id, "PinX") - 5.5).abs() < 1e-9 && (num(d, 1, id, "PinY") - 3.0).abs() < 1e-9,
        "{} {}",
        num(d, 1, id, "PinX"),
        num(d, 1, id, "PinY")
    );
    let corners = diacraft_engine::draw::shape_corners(d, 1, id).unwrap();
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    assert!((middle.0 - 5.0).abs() < 1e-9 && (middle.1 - 4.0).abs() < 1e-9, "{middle:?}");
}

#[test]
fn a_master_is_found_by_id_by_name_and_a_stencil_that_is_not_open_is_refused() {
    let (mut s, _dir) = with_stencil();
    for key in [json!("2"), json!(2), json!("decision"), json!("  DECISION ")] {
        let r = run(&mut s, "shape.drop", json!({"master": key, "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
        assert!(r["name"].as_str().unwrap().starts_with("Decision"), "{key}: {r}");
    }
    assert_eq!(masters_named(s.document().unwrap(), "Decision"), 1, "one master for four drops");
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Nope", "stencil": "file-1", "page": 1, "x": 1, "y": 1}));
    assert!(msg.contains("Nope") && msg.contains("file-1"), "{msg}");
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-7", "page": 1, "x": 1, "y": 1}));
    assert!(msg.contains("file-7") && msg.contains("stencil.open"), "{msg}");
    // Without `stencil` the built-in stencils are searched, as they were; a built-in stencil can be named.
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Link", "page": 1, "x": 1, "y": 1}));
    assert!(msg.contains("built-in"), "{msg}");
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "flowchart", "page": 1, "x": 1, "y": 1}));
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Process", "stencil": "basic", "page": 1, "x": 1, "y": 1}));
    assert!(msg.contains("basic"), "{msg}");
}

#[test]
fn a_second_drop_reuses_the_master_and_a_drawing_that_has_none_gets_it_again() {
    let (mut s, _dir) = with_stencil();
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 2, "y": 2}));
    let styles = s.document().unwrap().styles.len();
    let second = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 5, "y": 2}));
    assert_eq!(second["master_added"], false);
    let d = s.document().unwrap();
    assert_eq!(masters_named(d, "Process"), 1);
    assert_eq!(d.styles.len(), styles, "the styles it needed are there already");
    assert_eq!(d.page(1).unwrap().shapes.len(), 2);
    // A new drawing has none of it.
    run(&mut s, "file.new", json!({}));
    let third = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 2, "y": 2}));
    assert_eq!(third["master_added"], true);
    assert_eq!(masters_named(s.document().unwrap(), "Process"), 1);
}

#[test]
fn a_drop_is_one_undo_step_and_undo_takes_the_master_away_too() {
    let (mut s, _dir) = with_stencil();
    let before = s.document().unwrap().clone();
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 2, "y": 2}));
    assert_eq!(s.history().unwrap().undo_len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before, "no master, no style, no colour and no face is left");
    run(&mut s, "edit.redo", json!({}));
    assert!(master_named(s.document().unwrap(), "Process").is_some());
}

#[test]
fn a_master_whose_id_an_earlier_one_holds_is_left_out_of_the_listing_and_counted() {
    let dir = Dir::new();
    // Master ID 0 is allowed, and two masters with one ID are a hostile file: the first is the one a name or an ID finds.
    let first = MasterSpec::new(0, "Zero").shapes(&shape(5, &box_cells(1.0, 1.0)));
    let twin = MasterSpec::new(0, "Twin of zero").shapes(&shape(5, &box_cells(4.0, 4.0)));
    let long = MasterSpec::new(7, &"n".repeat(5_000)).shapes(&shape(5, &box_cells(1.0, 1.0)));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(first).master(twin).master(long).build();
    let mut s = blank_session();
    let opened = open(&mut s, &dir.write("twins.vssx", &bytes));
    let names: Vec<&str> = opened["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], "Zero");
    assert!(names[1].chars().count() <= 129 && names[1].ends_with('…'), "a name is cut for the answer: {} characters", names[1].chars().count());
    assert_eq!((opened["masters_total"].as_u64(), opened["masters_ignored"].as_u64()), (Some(2), Some(1)));
    let r = run(&mut s, "shape.drop", json!({"master": 0, "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!(r["width"], 1.0, "the first master 0, not its twin");
    // A name past what a name can be is never looked at, so a search by name does not find it and costs nothing.
    let msg = refusal(&mut s, "shape.drop", json!({"master": "n".repeat(300), "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert!(msg.contains("no master"), "{msg}");
}

#[test]
fn a_master_with_an_element_of_the_file_that_names_a_part_is_refused_too() {
    let dir = Dir::new();
    // Not a picture the model knows, but an element the model keeps as it was, with a relationship in it (here on a row of a section too).
    let on_shape = shape(5, &format!("{}<Mystery r:id=\"rId3\"/>", box_cells(1.0, 1.0)));
    let on_row = shape(
        5,
        &format!(
            "{}<Section N=\"Odd\"><Row N=\"One\"><Cell N=\"V\" V=\"1\"/><Inner><Deep r:embed=\"rId4\"/></Inner></Row></Section>",
            box_cells(1.0, 1.0)
        ),
    );
    let bytes = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "OnShape").shapes(&on_shape))
        .master(MasterSpec::new(2, "OnRow").shapes(&on_row))
        .master(MasterSpec { id: "3".to_owned(), ..decision_master() })
        .build();
    let mut s = blank_session();
    open(&mut s, &dir.write("refs.vssx", &bytes));
    for name in ["OnShape", "OnRow"] {
        let msg = refusal(&mut s, "shape.drop", json!({"master": name, "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
        assert!(msg.contains(name) && msg.contains("of the stencil's file"), "{msg}");
    }
    run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
}

#[test]
fn a_one_dimensional_master_is_dropped_with_its_ends_about_the_place() {
    let (mut s, _dir) = with_stencil();
    let r = run(&mut s, "shape.drop", json!({"master": "Link", "stencil": "file-1", "page": 1, "x": 5.0, "y": 3.0}));
    let id = r["id"].as_u64().unwrap() as u32;
    let d = s.document().unwrap();
    let want = [("BeginX", 4.0), ("BeginY", 3.0), ("EndX", 6.0), ("EndY", 3.0), ("PinX", 5.0), ("PinY", 3.0), ("Width", 2.0)];
    for (cell, v) in want {
        assert!((num(d, 1, id, cell) - v).abs() < 1e-9, "{cell}: {}", num(d, 1, id, cell));
    }
    // A move of it moves its ends, as it does a connector's.
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1, "dy": 1}));
    let d = s.document().unwrap();
    assert!((num(d, 1, id, "BeginX") - 5.0).abs() < 1e-9 && (num(d, 1, id, "EndY") - 4.0).abs() < 1e-9);
}

/// A group master (ID 5) with members 10, 11 (a group of its own, holding 12 and 13) and 14, each a box.
pub fn group_master(members: &str) -> MasterSpec {
    MasterSpec::new(1, "Cluster").shapes(&shape_with(5, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(3.0, 2.0))))
}

pub fn cluster_members() -> String {
    let leaf = |id: u32| shape(id, &format!("{LOOK}{}", box_cells(0.5, 0.5)));
    let inner = shape_with(11, r#" Type="Group""#, &format!("{}<Shapes>{}{}</Shapes>", box_cells(1.0, 1.0), leaf(12), leaf(13)));
    format!("{}{inner}{}", leaf(10), leaf(14))
}

#[test]
fn a_group_master_is_dropped_with_its_members_listed_as_a_file_lists_them() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&cluster_members())).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("group.vssx", &bytes));
    let r = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    let d = s.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    assert_eq!((r["id"].as_u64(), inst.kind.text()), (Some(1), Some("Group")));
    // One member for each descendant, depth first in the master's order, each with an ID of its own above the instance's and the shape it
    // stands for by `MasterShape`; none holds a cell.
    let listed: Vec<(u32, Option<u32>, usize)> = inst.children.iter().map(|m| (m.id, m.master_shape, m.children.len())).collect();
    assert_eq!(listed, [(2, Some(10), 0), (3, Some(11), 2), (6, Some(14), 0)]);
    let inner = inst.children.get(1).unwrap();
    let below: Vec<(u32, Option<u32>)> = inner.children.iter().map(|m| (m.id, m.master_shape)).collect();
    assert_eq!(below, [(4, Some(12)), (5, Some(13))]);
    assert!(inst.children.iter().all(|m| m.sheet.cells.is_empty() && m.master.is_none()));
    assert_eq!(inner.kind.text(), Some("Group"), "a member has its master shape's Type");
    // The session's mark is above the last member, so the next drop does not take an ID a member has.
    let next = run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 8, "y": 4}));
    assert_eq!(next["id"], 7);
    // Saved and read back, the page holds the same shapes.
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let e = t.document().unwrap();
    let again = e.page(1).unwrap().shapes.get(0).unwrap();
    assert_eq!(again.children.iter().map(|m| (m.id, m.master_shape)).collect::<Vec<_>>(), [(2, Some(10)), (3, Some(11)), (6, Some(14))]);
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn a_group_master_of_more_than_the_listed_members_is_dropped_with_them_left_to_the_master() {
    let dir = Dir::new();
    let leaves = |n: u32| (0..n).map(|k| format!("<Shape ID=\"{}\"/>", 10 + k)).collect::<String>();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&leaves(5_001))).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("many.vssx", &bytes));
    run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.get(0).unwrap().children.len(), 0);
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(group_master(&leaves(5_000))).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("fewer.vssx", &bytes));
    run(&mut s, "shape.drop", json!({"master": "Cluster", "stencil": "file-1", "page": 1, "x": 4, "y": 4}));
    assert_eq!(s.document().unwrap().page(1).unwrap().shapes.get(0).unwrap().children.len(), 5_000);
}

// ------------------------------------------------------------------------------------------------------------------- master matching

/// A drawing, opened from a file, whose masters are `masters`.
pub fn drawing_with(dir: &Dir, masters: Vec<MasterSpec>) -> Session {
    let mut b = VsdxBuilder::new().page(PageSpec::new(1, "Page-1"));
    for m in masters {
        b = b.master(m);
    }
    let path = dir.write("plan.vsdx", &b.build());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

pub fn simple_master(id: u32, name: &str, attrs: &str) -> MasterSpec {
    MasterSpec::new(id, name).attrs(attrs).shapes(&shape(5, &format!("{LOOK}{}", box_cells(3.0, 3.0))))
}

#[test]
fn a_master_the_drawing_has_by_its_unique_id_is_reused() {
    let dir = Dir::new();
    let mut s = drawing_with(&dir, vec![simple_master(9, "Earlier copy", &ids(101, 201))]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!((r["master"].as_u64(), r["master_added"].as_bool()), (Some(9), Some(false)));
    let d = s.document().unwrap();
    assert_eq!(d.masters.len(), 1);
    assert_eq!(num(d, 1, r["id"].as_u64().unwrap() as u32, "Width"), 3.0, "it draws as the drawing's master does");
}

#[test]
fn a_master_with_the_same_base_id_and_another_unique_id_is_the_same_master() {
    let dir = Dir::new();
    let mut s = drawing_with(&dir, vec![simple_master(4, "Process", &ids(555, 201))]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!((r["master"].as_u64(), r["master_added"].as_bool()), (Some(4), Some(false)));
    assert_eq!(s.document().unwrap().masters.len(), 1);
}

#[test]
fn a_unique_id_the_drawing_holds_for_another_master_is_not_repeated_in_the_copy() {
    let dir = Dir::new();
    // Same UniqueID, another BaseID: not the same master, and the copy needs a UniqueID of its own.
    let mut s = drawing_with(&dir, vec![simple_master(4, "Unrelated", &ids(101, 999))]);
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    let r = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert_eq!(r["master_added"], true);
    let d = s.document().unwrap();
    assert_eq!(d.masters.len(), 2);
    let copy = master_named(d, "Process").unwrap();
    assert_ne!(copy.unique_id.as_deref(), Some(guid(101).as_str()));
    assert!(copy.unique_id.is_some());
    assert_eq!(copy.raw.get("BaseID"), Some(guid(201).as_str()), "the BaseID stays: it names the master across drawings");
    let uniques: std::collections::HashSet<_> = d.masters.iter().filter_map(|m| m.unique_id.clone()).collect();
    assert_eq!(uniques.len(), 2);
    // And the next drop finds the copy by the BaseID.
    let again = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 5, "y": 5}));
    assert_eq!(again["master_added"], false);
}

#[test]
fn two_masters_of_a_stencil_with_one_unique_id_are_not_merged() {
    let dir = Dir::new();
    // Read them, and do not merge them. Their shapes differ, so a drop of each must draw its own.
    let a = MasterSpec::new(1, "Wide").attrs(&ids(7, 70)).shapes(&shape(5, &format!("{LOOK}{}", box_cells(3.0, 1.0))));
    let b = MasterSpec::new(2, "Tall").attrs(&ids(7, 71)).shapes(&shape(5, &format!("{LOOK}{}", box_cells(1.0, 3.0))));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(a).master(b).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("twin.vssx", &bytes));
    let wide = run(&mut s, "shape.drop", json!({"master": "Wide", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let tall = run(&mut s, "shape.drop", json!({"master": "Tall", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    assert_eq!((wide["width"].as_f64(), tall["width"].as_f64()), (Some(3.0), Some(1.0)));
    let d = s.document().unwrap();
    assert_eq!(d.masters.len(), 2);
    assert_ne!(wide["master"], tall["master"]);
    assert_eq!(num(d, 1, tall["id"].as_u64().unwrap() as u32, "Width"), 1.0, "the tall one draws as the tall one");
    // Dropping each again reuses what each got, and adds nothing.
    let w2 = run(&mut s, "shape.drop", json!({"master": "Wide", "stencil": "file-1", "page": 1, "x": 3, "y": 6}));
    let t2 = run(&mut s, "shape.drop", json!({"master": "Tall", "stencil": "file-1", "page": 1, "x": 6, "y": 6}));
    assert_eq!((w2["master"].clone(), t2["master"].clone()), (wide["master"].clone(), tall["master"].clone()));
    assert_eq!(s.document().unwrap().masters.len(), 2);
}

// ------------------------------------------------------------------------------------------- what a master references and the document lacks

#[test]
fn the_styles_a_master_uses_come_along_by_name_and_definition_under_free_ids() {
    let dir = Dir::new();
    // The drawing's style 3 is another style, so the stencil's comes along under a free ID, and the link is
    // rewritten on the copy only. Style 4, based on, has no counterpart: it comes too, and 3's link to it follows.
    let target = VsdxBuilder::new()
        .more_styles(
            r#"<StyleSheet ID="3" NameU="Mine" Name="Mine" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.05"/></StyleSheet>"#,
        )
        .page(PageSpec::new(1, "Page-1"))
        .build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let d = s.document().unwrap();
    assert_eq!(d.styles.len(), 4, "root, the drawing's 3, and the two that came");
    assert_eq!(d.style(3).unwrap().name_u.as_deref(), Some("Mine"), "the drawing's own style is untouched");
    let flow = d.styles.iter().find(|st| st.name_u.as_deref() == Some("Flow Normal")).expect("copied");
    let base = d.styles.iter().find(|st| st.name_u.as_deref() == Some("Flow Base")).expect("copied");
    assert!(flow.id > 3 && base.id > 3 && flow.id != base.id);
    assert_eq!(flow.sheet.links.line, Some(base.id), "the copy of 3 is based on the copy of 4");
    let m = master_named(d, "Process").unwrap();
    let top = m.shapes.get(0).unwrap();
    assert_eq!((top.sheet.links.line, top.sheet.links.fill, top.sheet.links.text), (Some(flow.id), Some(flow.id), Some(flow.id)));
    // The stencil is as it was: opening it again and listing it shows the same, and a second drop adds no style.
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    assert_eq!(s.document().unwrap().styles.len(), 4);
}

#[test]
fn a_style_the_drawing_has_with_the_same_name_and_definition_is_used_and_not_copied() {
    let dir = Dir::new();
    let styles = STENCIL_STYLES.to_owned();
    // The same two styles, under other IDs.
    let moved = styles
        .replace(r#"ID="4""#, r#"ID="14""#)
        .replace(r#"ID="3""#, r#"ID="13""#)
        .replace(r#"LineStyle="4" FillStyle="4" TextStyle="4""#, r#"LineStyle="14" FillStyle="14" TextStyle="14""#);
    let target = VsdxBuilder::new().colors(STENCIL_COLORS).more_styles(&moved).page(PageSpec::new(1, "Page-1")).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let d = s.document().unwrap();
    assert_eq!(d.styles.len(), 3, "no style was added");
    let top = master_named(d, "Process").unwrap().shapes.get(0).unwrap();
    assert_eq!((top.sheet.links.line, top.sheet.links.fill, top.sheet.links.text), (Some(13), Some(13), Some(13)));
}

#[test]
fn the_root_style_is_the_drawings_and_a_link_to_a_style_the_stencil_lacks_is_dropped() {
    let dir = Dir::new();
    let inner = format!("{LOOK}{}", box_cells(1.0, 1.0));
    // Links to the root (0), to a style that is not there (9), and to a pair of styles that are based on each other.
    let styles = r#"<StyleSheet ID="5" NameU="Loop A" LineStyle="6" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.02"/></StyleSheet>
<StyleSheet ID="6" NameU="Loop B" LineStyle="5" FillStyle="0" TextStyle="0"><Cell N="LineWeight" V="0.03"/></StyleSheet>"#;
    let m1 = MasterSpec::new(1, "Odd").shapes(&shape_with(5, r#" LineStyle="0" FillStyle="9" TextStyle="5""#, &inner));
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).more_styles(styles).master(m1).build();
    let mut s = blank_session();
    open(&mut s, &dir.write("odd.vssx", &bytes));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let r = s.execute("shape.drop", &json!({"master": "Odd", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
            let _ = tx.send(r.map(|v| v["id"].clone()).map_err(|e| e.to_string()));
        });
        rx.recv_timeout(std::time::Duration::from_secs(20)).expect("styles that name each other stalled the drop").unwrap();
    });
    let d = s.document().unwrap();
    let top = master_named(d, "Odd").unwrap().shapes.get(0).unwrap();
    assert_eq!(top.sheet.links.line, Some(0), "the root stays the root");
    assert_eq!(top.sheet.links.fill, None, "a link to nothing is no link");
    let a = d.styles.iter().find(|st| st.name_u.as_deref() == Some("Loop A")).expect("the cycle came");
    let b = d.styles.iter().find(|st| st.name_u.as_deref() == Some("Loop B")).expect("both");
    assert_eq!(top.sheet.links.text, Some(a.id));
    assert!(a.sheet.links.line == Some(b.id) || b.sheet.links.line == Some(a.id), "one end of the cycle is kept, the other is cut where it closes");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn custom_colours_and_faces_come_along_and_a_clash_in_the_colour_table_is_renumbered() {
    let dir = Dir::new();
    // The drawing's table holds 25 for another colour and has the colour 26 asks for under another number; it has the face Arial only.
    let target = VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="25" RGB="#112233"/><ColorEntry IX="30" RGB="#FFCC00"/>"##)
        .face_names(r#"<FaceName NameU="Arial"/>"#)
        .page(PageSpec::new(1, "Page-1"))
        .build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &target)}));
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes()));
    run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    let d = s.document().unwrap();
    // 25 is #3366CC in the stencil: not in the drawing, and 25 is taken, so it gets the next free number; 26 is #FFCC00, which is 30 in the drawing.
    let blue = d.colors.iter().find(|c| c.rgb.eq_ignore_ascii_case("#3366CC")).expect("the colour came");
    assert_ne!(blue.ix, 25);
    assert!(blue.ix > 23 && blue.ix <= 253);
    assert_eq!(d.colors.iter().filter(|c| c.rgb.eq_ignore_ascii_case("#FFCC00")).count(), 1, "the drawing's own entry is used");
    assert_eq!(d.color(25), Some(diacraft_color::Rgba::from_hex("#112233").unwrap()), "the drawing's own 25 is as it was");
    // The master's shape reads the colour it had in the stencil.
    let m = master_named(d, "Process").unwrap();
    let sheet = SheetRef::Shape(diacraft_doc::ShapeRef::top(Owner::Master(m.id), 0));
    let fore = Resolver::new(d).resolve(&sheet, &CellAddr::direct("FillForegnd")).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap();
    assert_eq!(d.color(fore as i64), Some(diacraft_color::Rgba::from_hex("#3366CC").unwrap()));
    // The face it names comes; the one the drawing had stays, once.
    let faces: Vec<&str> = d.fonts.iter().map(|f| &*f.name_u).collect();
    assert_eq!(faces, ["Arial", "Segoe Script"]);
    assert_eq!(d.fonts.get(1).unwrap().raw.get("Flags"), Some("1"), "the face comes with its attributes");
}

#[test]
fn a_master_that_holds_a_picture_is_refused_in_words_and_nothing_is_changed() {
    let dir = Dir::new();
    let foreign = r#"<ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId7"/></ForeignData>"#;
    let m = MasterSpec::new(1, "Logo").shapes(&shape_with(5, r#" Type="Foreign""#, &format!("{}{foreign}", box_cells(1.0, 1.0)))).rel(
        "rId7",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
        "../media/image1.png",
    );
    let pkg = VsdxBuilder::new().document_type(CT_STENCIL).master(m).master(decision_master()).pkg().part(
        "/visio/media/image1.png",
        "image/png",
        [0x89u8, b'P', b'N', b'G'],
    );
    let mut s = blank_session();
    open(&mut s, &dir.write("logos.vssx", &pkg.build()));
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Logo", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert!(msg.contains("picture") && msg.contains("Logo"), "{msg}");
    assert!(s.document().unwrap().masters.is_empty());
    // Another master of the same stencil still drops.
    run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
}

#[test]
fn a_master_of_none_or_of_more_top_level_shapes_than_a_drop_lists_is_refused_in_words() {
    let dir = Dir::new();
    let none = MasterSpec::new(2, "Empty");
    // Each top-level shape needs a member of its own on the page, so there is a limit (5,000), and 5,001 are past it.
    let many: String = (0..5_001).map(|k| shape(10 + k, &box_cells(1.0, 1.0))).collect();
    let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(none).master(MasterSpec::new(3, "Crowd").shapes(&many)).build();
    let mut s = blank_session();
    let opened = open(&mut s, &dir.write("odd.vssx", &bytes));
    assert_eq!(opened["masters"].as_array().unwrap().len(), 2, "they are listed");
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Empty", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert!(msg.contains("Empty") && msg.contains("no shapes"), "{msg}");
    let msg = refusal(&mut s, "shape.drop", json!({"master": "Crowd", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    assert!(msg.contains("Crowd") && msg.contains("5001 shapes at its top level"), "{msg}");
    // The refusals left no master behind.
    assert!(s.document().unwrap().masters.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------- save and reopen

#[test]
fn a_drawing_with_shapes_from_a_stencil_saves_and_reads_back() {
    let (mut s, dir) = with_stencil();
    let a = run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3, "y": 3}));
    run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 6, "y": 3}));
    run(&mut s, "shape.drop", json!({"master": "Link", "stencil": "file-1", "page": 1, "x": 4.5, "y": 1}));
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 8, "y": 6}));
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let (d, e) = (s.document().unwrap(), t.document().unwrap());
    assert_eq!(e.masters.len(), 4);
    assert_eq!(e.page(1).unwrap().shapes.len(), 4);
    let id = a["id"].as_u64().unwrap() as u32;
    for cell in ["PinX", "PinY", "Width", "Height"] {
        assert_eq!(num(d, 1, id, cell), num(e, 1, id, cell), "{cell}");
    }
    assert_eq!(e.styles.len(), d.styles.len());
    assert_eq!(e.colors, d.colors);
    assert_eq!(e.fonts, d.fonts);
    let m = master_named(e, "Process").unwrap();
    assert_eq!(m.unique_id.as_deref(), Some(guid(101).as_str()));
    assert_eq!(check_session(&t), Ok(()));
    // The saved drawing, saved again after another drop, patches and does not rewrite.
    run(&mut t, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 8, "y": 2}));
    run(&mut t, "file.save", json!({}));
}

// ------------------------------------------------------------------------------------------------------------------------ the icon

#[test]
fn a_masters_icon_is_listed_by_its_kind_and_size_and_answered_as_bytes() {
    let dir = Dir::new();
    // The first bytes of an icon file (reserved 0, type 1, one image), base64: AAABAAEAEBAAAAEAIABoBAAA...
    let ico_start = [0u8, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0, 0x68, 4, 0, 0, 22, 0, 0, 0];
    let text = base64(&ico_start);
    let bytes = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(process_master().icon(&text))
        .master(decision_master().icon("this is !! not base64"))
        .master(line_master())
        .build();
    let mut s = Session::new();
    let opened = open(&mut s, &dir.write("icons.vssx", &bytes));
    assert_eq!(opened["masters"][0]["icon"], json!({"format": "ico", "bytes": ico_start.len()}));
    assert_eq!(opened["masters"][1]["icon"], Value::Null, "text that is not base64 is no icon");
    let icon = run(&mut s, "stencil.icon", json!({"stencil": "file-1", "master": "1"}));
    assert_eq!((icon["format"].as_str(), icon["bytes"].as_u64()), (Some("ico"), Some(ico_start.len() as u64)));
    assert_eq!(icon["data"], text);
    let msg = refusal(&mut s, "stencil.icon", json!({"stencil": "file-1", "master": "3"}));
    assert!(msg.contains("no icon"), "{msg}");
    let msg = refusal(&mut s, "stencil.icon", json!({"stencil": "file-1", "master": "77"}));
    assert!(msg.contains("77"), "{msg}");
}

/// Standard base64 with padding, for a test's icon.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn the_command_sweep_names_stay_in_step() {
    // The registry holds the three new commands; the sweep's own list is in `command_sweep.rs`.
    let ids: Vec<&str> = diacraft_engine::command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("stencil.")).collect();
    assert_eq!(
        ids,
        [
            "stencil.list",
            "stencil.open",
            "stencil.close",
            "stencil.search",
            "stencil.quick",
            "stencil.add_quick",
            "stencil.remove_quick",
            "stencil.reset",
            "stencil.icon"
        ]
    );
    let e = Session::new().execute("stencil.open", &json!({"path": "x", "colour": 1})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }));
}

// ---------------------------------------------------------------------------------------------------------------------------- drawn

mod drawn {
    use diacraft_render::{Rendered, Renderer, View};

    use super::*;

    const PX_PER_INCH: f64 = 100.0;
    const PAGE_HEIGHT: f64 = 8.5;

    fn render(s: &Session) -> Rendered {
        let doc = s.document().unwrap();
        let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
        Renderer::new().render(doc, 1, &view)
    }

    /// The pixel at a place on the page, in inches from its left and bottom edges: premultiplied RGBA.
    fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
        let (width, height) = (r.width as usize, r.height as usize);
        let col = ((x * PX_PER_INCH) as usize).min(width - 1);
        let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
        let at = (row * width + col) * 4;
        let p = &r.rgba_premul[at..at + 4];
        [p[0], p[1], p[2], p[3]]
    }

    #[test]
    fn what_is_dropped_is_drawn_with_the_look_the_stencil_gave_it() {
        let (mut s, _dir) = with_stencil();
        run(&mut s, "shape.drop", json!({"master": "Process", "stencil": "file-1", "page": 1, "x": 3.0, "y": 4.0}));
        run(&mut s, "shape.drop", json!({"master": "Decision", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
        run(&mut s, "shape.drop", json!({"master": "Link", "stencil": "file-1", "page": 1, "x": 4.5, "y": 2.0}));
        let r = render(&s);
        assert!(r.skipped.is_empty(), "everything was drawn: {:?}", r.skipped);
        // Process: its own FillForegnd 25 (the stencil's table says #3366CC, and the drawing's table now holds it) over a box 1.5 by 0.8.
        assert_eq!(px(&r, 3.0, 4.0), [0x33, 0x66, 0xCC, 255], "the colour came with the master");
        assert_eq!(px(&r, 3.0, 4.6), [255, 255, 255, 255], "paper above the box");
        // Its line is the line of the style it is linked to and that style's base: red (colour 2), 0.0138889 in wide.
        let edge = px(&r, 3.0, 4.4);
        assert!(edge[0] > 150 && edge[1] < 120 && edge[2] < 120, "a red edge from the copied styles: {edge:?}");
        // Decision: the colour cells it holds itself.
        assert_eq!(px(&r, 6.0, 4.0), [0xE8, 0xF0, 0xFF, 255]);
        // Link: a line from (3.5, 2) to (5.5, 2).
        let on_line = px(&r, 4.5, 2.0);
        assert!(on_line[3] > 0 && u32::from(on_line[0]) + u32::from(on_line[1]) + u32::from(on_line[2]) < 600, "{on_line:?}");
        assert_eq!(px(&r, 4.5, 2.3), [255, 255, 255, 255]);
        // The same page read back from a saved file draws the same.
        let dir = Dir::new();
        let path = dir.path("again.vsdx");
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({"path": path}));
        let again = render(&t);
        assert_eq!(again.rgba_premul, r.rgba_premul, "a reopened drawing is drawn as it was");
    }
}
