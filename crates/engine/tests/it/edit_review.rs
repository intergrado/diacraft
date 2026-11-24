//! Smaller fixes from the review of the M3 editor: L4, a member of a group is moved and resized in its group's frame by a distance and a
//! point of the page, and L5, a move that moves nothing is no undo step, and `deleted` counts shapes and not names.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape_with};
use serde_json::json;

use crate::edit::{drop_shape, id_of};
use crate::{blank_session, check_session, run, snapshot};

/// A scratch file under `TMPDIR`, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(extension: &str, bytes: &[u8]) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path =
            std::env::temp_dir().join(format!("diacraft-engine-review-{}-{}.{extension}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, bytes).unwrap();
        Scratch(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Group 10 at (5, 5), 4 by 2 with its local pin at (2, 1), turned by `angle`; member 11 in it, 1 by 1 with its pin at its middle by formula, at
/// (1, 1) of the group's frame.
fn group_drawing(angle: &str) -> Vec<u8> {
    let member = shape_with(
        11,
        "",
        r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#,
    );
    let group = shape_with(
        10,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="5"/><Cell N="PinY" V="5"/><Cell N="Width" V="4"/><Cell N="Height" V="2"/>
<Cell N="LocPinX" V="2"/><Cell N="LocPinY" V="1"/><Cell N="Angle" V="{angle}"/><Shapes>{member}</Shapes>"#
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&group)).build()
}

fn opened(bytes: &[u8]) -> (Session, Scratch) {
    let file = Scratch::new("vsdx", bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    (s, file)
}

/// A cell of the member (11, inside group 10), by inheritance.
fn member_cell(d: &Document, name: &str) -> f64 {
    let at = d.find_shape(Owner::Page(1), 11).unwrap();
    Resolver::new(d)
        .resolve(&SheetRef::Shape(at), &CellAddr::direct(name))
        .and_then(|f| f.value.cell?.value.as_ref()?.number())
        .unwrap_or_else(|| panic!("member.{name}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_member_of_a_group_moves_by_a_distance_of_the_page_carried_into_the_groups_frame() {
    // A group that is not turned: the page's distance is the group's.
    let (mut s, _file) = opened(&group_drawing("0"));
    let moved = run(&mut s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 0.5}));
    assert_eq!(moved["moved"], 1);
    let d = s.document().unwrap();
    assert!(near(member_cell(d, "PinX"), 2.0) && near(member_cell(d, "PinY"), 1.5), "{} {}", member_cell(d, "PinX"), member_cell(d, "PinY"));
    // A group turned a quarter turn: a step to the right on the page is a step down in the group's frame. The member's pin, at (1, 1) in the
    // group's frame, is at (5, 4) on the page (the group's pin is at (5, 5) and its local pin at (2, 1)), and after the step at (6, 4).
    let (mut t, _file) = opened(&group_drawing("1.5707963267948966"));
    run(&mut t, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 0.0}));
    let d = t.document().unwrap();
    assert!(near(member_cell(d, "PinX"), 1.0) && near(member_cell(d, "PinY"), 0.0), "{} {}", member_cell(d, "PinX"), member_cell(d, "PinY"));
    assert_eq!(check_session(&t), Ok(()));
}

#[test]
fn a_member_of_a_group_is_resized_to_a_box_of_the_page_and_a_turned_group_refuses() {
    let (mut s, _file) = opened(&group_drawing("0"));
    // The group's frame has its origin at (3, 4) on the page. A box 2 by 1 with its lower-left corner at (4.5, 4.5) on the page is at (1.5, 0.5)
    // in the frame, and the pin, at the middle of the new size, at (2.5, 1).
    run(&mut s, "shape.resize", json!({"id": 11, "x": 4.5, "y": 4.5, "width": 2.0, "height": 1.0}));
    let d = s.document().unwrap();
    assert!(near(member_cell(d, "Width"), 2.0) && near(member_cell(d, "Height"), 1.0));
    assert!(near(member_cell(d, "PinX"), 2.5) && near(member_cell(d, "PinY"), 1.0), "{} {}", member_cell(d, "PinX"), member_cell(d, "PinY"));
    let (mut t, _file) = opened(&group_drawing("0.5"));
    let before = snapshot(&t);
    let error = t.execute("shape.resize", &json!({"id": 11, "x": 4.5, "y": 4.5, "width": 2.0, "height": 1.0})).unwrap_err();
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    assert_eq!(snapshot(&t), before);
}

#[test]
fn moving_a_connector_whose_two_ends_are_glued_moves_nothing_and_is_no_undo_step() {
    let dir = std::env::temp_dir().join(format!("diacraft-engine-review-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.vsdx");
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    run(&mut s, "file.save_as", json!({"path": path.to_str().unwrap()}));
    assert!(!s.is_dirty());
    let before = snapshot(&s);
    let result = run(&mut s, "shape.move", json!({"ids": [connector], "dx": 1.0, "dy": 1.0}));
    assert_eq!(result["moved"], 0);
    assert!(!s.is_dirty(), "nothing moved, so nothing is unsaved");
    assert_eq!(s.history().unwrap().undo_len(), before.undo_steps, "and no undo step was recorded");
    assert_eq!(s.revision(), before.revision);
    // With a shape that does move, the connector is not counted: it follows.
    let result = run(&mut s, "shape.move", json!({"ids": [connector, a], "dx": 1.0, "dy": 0.0}));
    assert_eq!(result["moved"], 1);
    assert!(s.is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleted_counts_the_shapes_and_not_the_ids_named() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let result = run(&mut s, "shape.delete", json!({"ids": [a, a, a]}));
    assert_eq!(result["deleted"], 1);
    let result = run(&mut s, "shape.delete", json!({"ids": [b]}));
    assert_eq!(result["deleted"], 1);
    assert_eq!(check_session(&s), Ok(()));
}
