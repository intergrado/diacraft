//! The settings the router is given from the page (item 6): the clearances and spacings clamped, the costs in drawing units, the cells that say which
//! wires a route keeps apart from, and the margin of one paper inch.

use diacraft_route::{Overlap, Separate};
use diacraft_testkit::build::{DocBuilder, PageBuilder};

use super::plenty;
use crate::draw::route_scene::{Scene, clamp_spacing};

/// A document of one empty page holding these cells on its sheet and these on the root style.
fn doc_with(page: &[(&str, &str)], root: &[(&str, &str)]) -> diacraft_doc::Document {
    let base = root.iter().fold(DocBuilder::new(), |b, (name, value)| b.root(|r| r.cell(name, value)));
    base.page(1, page.iter().fold(PageBuilder::new(), |p, (name, value)| p.cell(name, value))).build()
}

fn settings_of(page: &[(&str, &str)], root: &[(&str, &str)]) -> diacraft_route::Settings {
    Scene::read(&doc_with(page, root), 1, &mut plenty()).settings()
}

/// A spacing that is no number or is below 0 is the root's 0.125, 0 is 0, one over one paper inch is one paper inch, and a scale that is nothing is 1.
#[test]
fn a_spacing_is_clamped_to_one_paper_inch_and_a_bad_one_is_the_roots() {
    assert_eq!(clamp_spacing(0.3, 1.0), 0.3);
    assert_eq!(clamp_spacing(0.0, 1.0), 0.0, "0 is 0");
    assert_eq!(clamp_spacing(f64::NAN, 1.0), 0.125, "not a number");
    assert_eq!(clamp_spacing(-0.2, 1.0), 0.125, "below 0");
    assert_eq!(clamp_spacing(f64::NEG_INFINITY, 1.0), 0.125);
    assert_eq!(clamp_spacing(3.0, 1.0), 1.0, "over one paper inch");
    assert_eq!(clamp_spacing(f64::INFINITY, 1.0), 1.0);
    assert_eq!(clamp_spacing(3.0, 10.0), 3.0, "a drawing scale of 10 makes a paper inch 10 drawing units");
    assert_eq!(clamp_spacing(30.0, 10.0), 10.0);
    assert_eq!(clamp_spacing(0.5, 0.0), 0.5, "a scale that is 0 is 1");
    assert_eq!(clamp_spacing(5.0, f64::NAN), 1.0);
    assert_eq!(clamp_spacing(5.0, -3.0), 1.0);
    assert_eq!(clamp_spacing(f64::NAN, 0.05), 0.05, "the root's value is no more than a paper inch either");
    for per_inch in [1.0, 0.05, 10.0, 1.0e9, f64::NAN, -1.0] {
        for v in [0.0, 0.125, 1.0, 1.0e30, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MIN_POSITIVE] {
            let got = clamp_spacing(v, per_inch);
            assert!(got.is_finite() && got >= 0.0, "clamp_spacing({v}, {per_inch}) = {got}");
        }
    }
}

/// On a page of drawing scale 1, `LineToNodeX` of -0.2 is a clearance of 0.125 beside upright legs, 0 is none, 3 is 1; with a drawing scale of 10 inches to the page's 1, 3 is 3; a page with no
/// `LineToNodeY` and a root holding 0.0984251969 is that beside level legs.
#[test]
fn the_clearance_is_the_pages_cell_clamped_in_drawing_units() {
    let root = [("LineToNodeX", "0.125"), ("LineToNodeY", "0.0984251969")];
    assert_eq!(settings_of(&[("LineToNodeX", "-0.2")], &root).clearance.0, 0.125, "below 0 is the root's US value");
    assert_eq!(settings_of(&[("LineToNodeX", "0")], &root).clearance.0, 0.0, "0 is a leg along a box's edge");
    assert_eq!(settings_of(&[("LineToNodeX", "3")], &root).clearance.0, 1.0, "over one paper inch is one");
    assert_eq!(
        settings_of(&[("LineToNodeX", "3"), ("PageScale", "1"), ("DrawingScale", "10")], &root).clearance.0,
        3.0,
        "a paper inch is ten drawing units"
    );
    assert_eq!(settings_of(&[("LineToNodeX", "30"), ("PageScale", "1"), ("DrawingScale", "10")], &root).clearance.0, 10.0);
    let metric = settings_of(&[("LineToNodeX", "0.25")], &root);
    assert_eq!(metric.clearance, (0.25, 0.0984251969), "the page's own X and the root's Y");
    assert_eq!(settings_of(&[], &[]).clearance, (0.125, 0.125), "found nowhere: the root's US value");
    let spaced = settings_of(&[("LineToLineX", "0.2"), ("LineToLineY", "7")], &root);
    assert_eq!(spaced.spacing, (0.2, 1.0), "the spacing is read and clamped as the clearance is");
}

/// The costs are paper inches times the page's scale, the near cost per unit is 1, and the first area reaches one paper inch past the ends.
#[test]
fn the_costs_and_the_margin_are_paper_lengths_in_drawing_units() {
    let plain = settings_of(&[], &[]);
    assert_eq!((plain.bend, plain.cross, plain.near, plain.margin), (0.5, 0.25, 1.0, 1.0));
    assert!(plain.lean.is_none(), "no lean: that is");
    let scaled = settings_of(&[("PageScale", "1"), ("DrawingScale", "10")], &[]);
    assert_eq!(
        (scaled.bend, scaled.cross, scaled.near, scaled.margin),
        (5.0, 2.5, 1.0, 10.0),
        "a paper inch is ten drawing units; the near cost is per unit and does not scale"
    );
    let small = settings_of(&[("PageScale", "4"), ("DrawingScale", "1")], &[]);
    assert_eq!((small.bend, small.margin), (0.125, 0.25), "a drawing of a quarter: a paper inch is a quarter of a drawing unit");
    let broken = settings_of(&[("PageScale", "0"), ("DrawingScale", "0")], &[]);
    assert_eq!(broken.margin, 1.0, "a scale that is nothing is 1");
}

/// Separate 3 reads as 0 and Overlap 0 reads as 3, whatever the style.
#[test]
fn separate_and_overlap_are_mapped_as_the_routing_rules_say() {
    for (code, want) in
        [("0", Separate::Unrelated), ("1", Separate::All), ("2", Separate::None), ("3", Separate::Unrelated), ("9", Separate::Unrelated)]
    {
        assert_eq!(settings_of(&[("LineAdjustFrom", code)], &[]).separate, want, "LineAdjustFrom {code}");
    }
    for (code, want) in [("0", Overlap::Related), ("1", Overlap::Any), ("2", Overlap::Never), ("3", Overlap::Related), ("9", Overlap::Related)] {
        assert_eq!(settings_of(&[("LineAdjustTo", code)], &[]).overlap, want, "LineAdjustTo {code}");
    }
    let root = settings_of(&[], &[("LineAdjustFrom", "1"), ("LineAdjustTo", "2")]);
    assert_eq!((root.separate, root.overlap), (Separate::All, Overlap::Never), "read through inheritance, from the root");
}
