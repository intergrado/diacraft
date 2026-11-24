//! The save result's `checked` field as a program reads it (the CLI, the MCP server and the control channel get the engine's result as it is): plain JSON, no type of the engine's.
//!
//! These are the tests that fail on the commit before the check was made: a save result had no `checked` field, so each reads `null` where it wants an object. (`save_check.rs` has the types and the
//! planted losses.)

use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, pinned};
use crate::run;

fn drawing(shapes: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(shapes)).build()
}

fn saved_as(bytes: &[u8], via_diacraft: bool, target: &str) -> Value {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", bytes) }));
    if via_diacraft {
        run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    }
    run(&mut s, "file.save_as", json!({ "path": dir.path(target) }))
}

/// A VSDX drawing saved as an `.odg` loses formulas, and the result says so: the class `formulas` with the number lost, and the values group, in the field's own shape. (The
/// formulas of a shape's own style and frame cells come back from an `.odg`; the place of a member of a group, which the reader works out from the group's frame, is still written as its value.)
#[test]
fn the_result_of_a_save_across_names_the_formulas_lost_as_a_field() {
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
    let shapes = shapes.as_str();
    let saved = saved_as(&drawing(shapes), false, "b.odg");
    assert_eq!(saved["checked"]["done"], true, "{saved}");
    let classes = saved["checked"]["classes"].as_array().unwrap_or_else(|| panic!("{saved}"));
    assert!(classes.contains(&json!({ "class": "formulas", "words": "2 formulas are lost", "count": 2 })), "{saved}");
    assert_eq!(saved["checked"]["reason"], Value::Null);
}

/// The line ends of a VSDX drawing are lost in an `.odg`.
#[test]
fn the_result_of_a_save_across_names_the_line_ends_lost() {
    let shapes = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="BeginArrow" V="3"/><Cell N="EndArrow" V="4"/></Shape>"#;
    let saved = saved_as(&drawing(shapes), false, "b.odg");
    assert_eq!(saved["checked"]["classes"], json!([{ "class": "line-ends", "words": "1 line end differs", "count": 1 }]), "{saved}");
}

/// A trip that loses nothing: the check is done, names nothing, and the field says so.
#[test]
fn the_result_of_a_save_that_loses_nothing_is_a_done_check_with_no_classes() {
    let shapes = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/></Shape>"#;
    let saved = saved_as(&drawing(shapes), true, "b.vsdx");
    assert_eq!(saved["checked"], json!({ "done": true, "classes": [], "as_values": false, "reason": null }), "{saved}");
}

/// A save into the file the drawing was opened from is not checked in 0.2: the field is `null`.
#[test]
fn the_result_of_a_save_into_the_drawings_own_file_is_not_checked() {
    let shapes = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/></Shape>"#;
    let saved = saved_as(&drawing(shapes), false, "b.vsdx");
    assert!(saved.get("checked").is_some_and(Value::is_null), "{saved}");
}
