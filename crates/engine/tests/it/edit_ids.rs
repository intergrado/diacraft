//! Shape ids are never reused in a session (of the M3 review): a shape deleted and one dropped after it do not share an `ID`, so a
//! file whose deleted shape held a picture still saves, and a formula that named the old shape does not point at the new one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::Owner;
use diacraft_engine::Session;
use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, shape};
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::{blank_session, check_session, run};

/// A scratch file under `TMPDIR`, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(bytes: &[u8]) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("diacraft-engine-ids-{}-{}.vsdx", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
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

/// Shape 6, a plain box, and shape 7 (the highest `ID`), a picture: a `Foreign` shape with a `ForeignData` that reaches an image part.
fn drawing_with_a_picture() -> Vec<u8> {
    let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
    let plain = shape(6, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#);
    let picture = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="6"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{plain}{picture}")).rel("rId1", &format!("{OD}image"), "../media/image1.png"))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png))
        .build()
}

fn ids_of_page_1(s: &Session) -> Vec<u32> {
    doc(s).page(1).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

#[test]
fn deleting_the_top_picture_shape_and_dropping_another_does_not_reuse_its_id_and_the_file_saves() {
    let file = Scratch::new(&drawing_with_a_picture());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    assert_eq!(ids_of_page_1(&s), [6, 7]);
    run(&mut s, "shape.delete", json!({"ids": [7]}));
    let dropped = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    assert_eq!(dropped, 8, "the id of the deleted picture is not given again");
    // The save must succeed: the writer would otherwise find a shape 7 that held a picture and now holds a rectangle.
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["dirty"], false);
    assert_eq!(check_session(&s), Ok(()));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": file.text()}));
    assert_eq!(ids_of_page_1(&reopened), [6, 8]);
    assert!(doc(&reopened).find_shape(Owner::Page(1), 7).is_none());
}

#[test]
fn a_connector_takes_an_id_above_a_deleted_shape_too() {
    let file = Scratch::new(&drawing_with_a_picture());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    assert_eq!(a, 8);
    run(&mut s, "shape.delete", json!({"ids": [7, a]}));
    assert_eq!(ids_of_page_1(&s), [6]);
    let b = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    assert_eq!(b, 9, "above the highest id the session has given, though 7 and 8 are gone");
    let c = drop_shape(&mut s, "rectangle", 7.0, 3.0);
    let connector = run(&mut s, "connector.add", json!({"from": {"id": b}, "to": {"id": c}}));
    assert_eq!(connector["id"], 11);
    run(&mut s, "file.save", json!({}));
}

#[test]
fn an_id_given_in_a_session_is_not_given_again_after_a_delete_or_an_undo() {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 5.0, 2.0), 2);
    run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(drop_shape(&mut s, "rectangle", 5.0, 4.0), 3, "not 2 again");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(ids_of_page_1(&s), [1]);
    assert_eq!(drop_shape(&mut s, "rectangle", 5.0, 4.0), 4, "an undone drop's id is not reused either: a redo may bring it back");
    // A new drawing starts again.
    run(&mut s, "file.new", json!({}));
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
}

#[test]
fn the_id_of_a_shape_of_the_file_deleted_before_a_save_is_not_given_again_after_it() {
    // Of the M3 second slice: a save replaces the session's baseline with the file it wrote, which no longer holds the deleted shape, so the
    // highest `ID` of the baseline it replaced goes into the session's high-water mark first. Open, copy the page's highest shape (7, the picture),
    // delete it, save, paste: the copy is not 7, which a `Sheet.7!` left dangling by the delete would name.
    let file = Scratch::new(&drawing_with_a_picture());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    run(&mut s, "edit.copy", json!({"ids": [7]}));
    run(&mut s, "shape.delete", json!({"ids": [7]}));
    run(&mut s, "file.save", json!({}));
    let pasted = run(&mut s, "edit.paste", json!({}));
    assert_eq!(pasted["ids"], json!([8]), "7 was a shape's, and the save did not make it free");
    // And after a second save, and a drop, and a duplicate: never an id the file held.
    run(&mut s, "file.save", json!({}));
    assert_eq!(drop_shape(&mut s, "rectangle", 3.0, 3.0), 9);
    run(&mut s, "file.save", json!({}));
    let again = run(&mut s, "shape.duplicate", json!({"ids": [6]}));
    assert_eq!(again["ids"], json!([10]));
    assert_eq!(check_session(&s), Ok(()));
    run(&mut s, "file.save", json!({}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": file.text()}));
    assert_eq!(ids_of_page_1(&reopened), [6, 8, 9, 10]);
}
