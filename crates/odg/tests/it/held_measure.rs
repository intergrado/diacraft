//! The items the reader charges for shapes of the kinds a LibreOffice drawing holds, measured so that the capacity change of the task can be named (`--ignored`,
//! `--nocapture`). The same file is run on the commit before the task for the figure "before".

use diacraft_odg::{Limits, read};

use crate::drawing::Drawing;

/// The items, and the bytes a save estimates it will hold (`ReadCost::save_bytes`, which the memory gate compares), of a drawing of `n` shapes made by `one(k)`, less those of
/// the same drawing with none, over `n`.
fn per_shape(n: usize, one: impl Fn(usize) -> String, auto: &dyn Fn(usize) -> String) -> (f64, f64) {
    let body = |n: usize| (0..n).map(&one).collect::<String>();
    let figures = |n: usize| {
        let styles: String = (0..n).map(auto).collect();
        let (_, report) = read(&Drawing::a4().auto(&styles).page(&body(n)).bytes(), &Limits::default()).expect("reads");
        (report.work.items, report.index.read_cost().save_bytes())
    };
    let (empty, full) = (figures(0), figures(n));
    ((full.0 - empty.0) as f64 / n as f64, (full.1 as f64 - empty.1 as f64) / n as f64)
}

/// A LibreOffice-shaped drawing: shapes with an automatic style (and a text style), on layer `layout`, each of the kinds a diagram holds.
#[test]
#[ignore = "a measurement, not a check"]
fn items_per_shape_of_a_libreoffice_shaped_drawing() {
    let shared = concat!(
        r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#729fcf" draw:stroke="solid" svg:stroke-color="#3465a4" draw:textarea-horizontal-align="center" draw:textarea-vertical-align="middle"/></style:style>"##,
        r##"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-end="Arrow" draw:marker-end-width="0.3cm" draw:fill="none"/></style:style>"##,
        r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"##,
        r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="12pt"/></style:style>"##,
    );
    let rect = |k: usize| {
        format!(
            r##"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" svg:width="3cm" svg:height="1.5cm" svg:x="{}cm" svg:y="1cm"><text:p text:style-name="P1"><text:span text:style-name="T1">Box {k}</text:span></text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"##,
            k % 20
        )
    };
    let plain = |k: usize| {
        format!(r##"<draw:rect draw:style-name="gr1" draw:layer="layout" svg:width="3cm" svg:height="1.5cm" svg:x="{}cm" svg:y="1cm"/>"##, k % 20)
    };
    let line = |k: usize| {
        format!(r##"<draw:line draw:style-name="gr2" draw:layer="layout" svg:x1="{}cm" svg:y1="1cm" svg:x2="3cm" svg:y2="3cm"/>"##, k % 20)
    };
    let turned = |k: usize| {
        format!(
            r##"<draw:rect draw:style-name="gr1" draw:layer="layout" svg:width="3cm" svg:height="1.5cm" draw:transform="rotate (0.5) translate ({}cm 1cm)"/>"##,
            k % 20
        )
    };
    // A style of its own for each shape (what a save of the model makes of a new shape, and what a drawing of many looks holds): the styles of the shapes, one each.
    let own = |k: usize| {
        format!(
            r##"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#{:06x}" draw:stroke="solid" svg:stroke-color="#3465a4"/></style:style>"##,
            k % 0xffffff
        )
    };
    let own_rect = |k: usize| {
        format!(r##"<draw:rect draw:style-name="gr{k}" draw:layer="layout" svg:width="3cm" svg:height="1.5cm" svg:x="{}cm" svg:y="1cm"/>"##, k % 20)
    };
    let (items, bytes) = per_shape(2_000, own_rect, &own);
    println!("ITEMS rect with an automatic style of its own: {items:.2} items and {bytes:.0} bytes of the save's estimate per shape");
    let auto = |k: usize| if k == 0 { shared.to_owned() } else { String::new() };
    for (what, f) in
        [("custom shape with text", &rect as &dyn Fn(usize) -> String), ("plain rect", &plain), ("line", &line), ("turned rect", &turned)]
    {
        let (items, bytes) = per_shape(2_000, f, &auto);
        println!("ITEMS {what}: {items:.2} items and {bytes:.0} bytes of the save's estimate per shape");
    }
}
