//! Shapes that inherit: from a master, from a style, and members a master supplies.

use diacraft_testkit::build::{MasterBuilder, StyleBuilder};

use crate::support::*;

#[test]
fn an_instance_draws_its_masters_geometry_and_takes_a_local_cell_over_the_masters() {
    let master = MasterBuilder::new().shape(
        rect(5, 0.0, 0.0, 1.0, 1.0)
            .cell("FillForegnd", "#FF0000")
            .cell("PinX", "0.5")
            .cell("PinY", "0.5")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.5"),
    );
    // The instances hold their own place and size and nothing else; the master supplies geometry and fill.
    let at = |id: u32, x: f64| {
        diacraft_testkit::build::ShapeBuilder::new(id)
            .master(1)
            .cell("PinX", &x.to_string())
            .cell("PinY", "1.5")
            .cell("Width", "1")
            .cell("Height", "1")
    };
    let p = page(4.0, 4.0)
        .shape(at(10, 1.0).cell("LocPinX", "0.5").cell("LocPinY", "0.5"))
        .shape(at(11, 3.0).cell("LocPinX", "0.5").cell("LocPinY", "0.5").cell("FillForegnd", "#0000FF"));
    let pic = draw(&base().master(1, master).page(1, p).build());
    assert!(pic.is_red(1.0, 1.5), "inherited geometry and fill");
    assert!(pic.is(3.0, 1.5, (0, 0, 255), 6), "a local cell wins over the master's");
}

#[test]
fn a_shape_with_no_link_of_its_own_reads_the_style_its_master_shape_links() {
    // R32 step 3: the style link of a family is the sheet's own, else the master shape's. Style 3 holds the fill.
    let master = MasterBuilder::new().shape(
        rect(5, 0.0, 0.0, 1.0, 1.0).links(None, Some(3), None).cell("PinX", "0.5").cell("PinY", "0.5").cell("LocPinX", "0.5").cell("LocPinY", "0.5"),
    );
    let inst = diacraft_testkit::build::ShapeBuilder::new(10)
        .master(1)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5");
    let doc = base()
        .style(3, StyleBuilder::new().cell("FillForegnd", "#00FF00").cell("FillPattern", "1"))
        .master(1, master)
        .page(1, page(4.0, 4.0).shape(inst))
        .build();
    assert!(draw(&doc).is(2.0, 2.0, (0, 255, 0), 6));
}

#[test]
fn a_member_the_instance_leaves_out_is_drawn_from_the_master_in_its_masters_place() {
    // The master group holds a red, a green and a blue square, in that order, that overlap. The instance lists the red and the blue only;
    // the green one it omits takes its place after the red (the nearest master shape before it that the page has), so it is over the red
    // and under the blue. Placed last it would cover the blue where they overlap, first it would be under the red.
    let square = |id: u32, x: f64, color: &str| rect(id, x, x, 2.0, 2.0).cell("FillForegnd", color).cell("LinePattern", "0");
    let group = shape_at(10, 2.0, 2.0, 4.0, 4.0).cell("LocPinX", "2").cell("LocPinY", "2").group();
    let group = group.child(square(11, 0.0, "#FF0000")).child(square(12, 1.0, "#00FF00")).child(square(13, 2.0, "#0000FF"));
    let master = MasterBuilder::new().shape(group);
    let member = |id: u32, of: u32| diacraft_testkit::build::ShapeBuilder::new(id).master_shape(of);
    let inst = diacraft_testkit::build::ShapeBuilder::new(100)
        .master(1)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("Width", "4")
        .cell("Height", "4")
        .cell("LocPinX", "2")
        .cell("LocPinY", "2")
        .group()
        .child(member(101, 11))
        .child(member(103, 13));
    let pic = draw(&base().master(1, master).page(1, page(4.0, 4.0).shape(inst)).build());
    assert!(pic.is_red(0.5, 0.5), "the red square alone");
    assert!(pic.is(1.5, 1.5, (0, 255, 0), 6), "the omitted green square is over the red one");
    assert!(pic.is(2.5, 2.5, (0, 0, 255), 6), "and under the blue one");
    assert!(pic.is(3.5, 3.5, (0, 0, 255), 6));
}
