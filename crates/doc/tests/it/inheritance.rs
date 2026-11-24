//! Where a cell's formula and value come from. Each test names the vector it runs and the
//! wrong answer that vector exists to catch; the mutation runs in the task report flip each rule and show these fail.
//!
//! Evaluation is `calc`'s (Task 6): a vector that asks for a computed number (the 3, the 9.4) is run here as far as the
//! model goes, which is the cell to evaluate, the sheet to evaluate it on and the sheet `Sheet.N` rebinds to.

use diacraft_doc::{
    CellAddr, Document, Family, FormulaSlot, Owner, Rebind, Resolved, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Source, ValueKind, Warnings,
};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn page_shape(path: &[u32]) -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: path.to_vec() })
}

fn master_shape(master: u32, path: &[u32]) -> ShapeRef {
    ShapeRef { owner: Owner::Master(master), path: path.to_vec() }
}

fn read<'a>(doc: &'a Document, at: &SheetRef, cell: &str) -> Resolved<'a> {
    Resolver::new(doc).resolve(at, &CellAddr::direct(cell)).unwrap_or_else(|| panic!("{cell} did not resolve on {at:?}"))
}

fn formula_text<'a>(r: &Resolved<'a>) -> Option<&'a str> {
    r.formula.cell.and_then(|c| c.formula.text())
}

fn value_text<'a>(r: &Resolved<'a>) -> Option<&'a str> {
    r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text())
}

fn style(family: Family, id: u32) -> Source {
    Source::Style { id, family: Some(family) }
}

/// One master (ID 1) with one shape holding `Width` 2 and `LocPinX` `Width*0.5` (value 1), and a page with the instances.
fn half_width_master() -> MasterBuilder {
    MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").formula("LocPinX", "Width*0.5", "1"))
}

// ---- the master step -------------------------------------------------------------------------------------------------

#[test]
fn an_inherited_formula_is_found_on_the_master_and_is_to_run_on_the_inheritor() {
    // The instance's Width is 6; `LocPinX` is found on the master, to be evaluated on the instance (3). Copying the master's
    // value, 1, is the naive answer, so the resolution must say "run this on the instance", not hand over 1.
    let doc = DocBuilder::new()
        .master(1, half_width_master())
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("Width", "6")))
        .build();
    let at = page_shape(&[0]);
    let r = read(&doc, &at, "LocPinX");
    assert_eq!(r.formula.source.clone(), Source::Master(master_shape(1, &[0])));
    assert_eq!(formula_text(&r), Some("Width*0.5"));
    assert_eq!(r.on, at, "evaluated on the inheritor, the instance, not on the master's shape");
    assert_eq!(r.rebind, Some(Rebind { instance: ShapeRef { owner: Owner::Page(1), path: vec![0] }, master: 1 }));
    // The instance's own cell is local, not inherited.
    assert_eq!(read(&doc, &at, "Width").formula.source, Source::Local);
    // Until an evaluation the stored value that stands is the master's.
    assert_eq!((r.value.source.clone(), value_text(&r)), (Source::Master(master_shape(1, &[0])), Some("1")));
}

#[test]
fn a_local_value_beside_inh_stands_until_an_evaluation() {
    // `F="Inh"` with V 7 over a master whose value is 1: the value is 7 on load. Resolving the value from the master
    // (1) is the naive answer; so is letting the local `Inh` element end the search for the formula.
    let doc = DocBuilder::new()
        .master(1, half_width_master())
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("Width", "2").inh("LocPinX", Some("7"))))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LocPinX");
    assert_eq!((r.value.source.clone(), value_text(&r)), (Source::Local, Some("7")));
    assert_eq!((r.formula.source.clone(), formula_text(&r)), (Source::Master(master_shape(1, &[0])), Some("Width*0.5")));
}

#[test]
fn a_local_formula_stays_and_an_instance_with_none_follows_the_master() {
    // A local `1 in` stays when the master's formula changes; a second instance follows the master.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").formula("LocPinX", "Width*0.4321", "0.8642"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).formula("LocPinX", "1 in", "1")).shape(ShapeBuilder::new(11).master(1)))
        .build();
    let first = read(&doc, &page_shape(&[0]), "LocPinX");
    assert_eq!((first.formula.source.clone(), formula_text(&first)), (Source::Local, Some("1 in")));
    let second = read(&doc, &page_shape(&[1]), "LocPinX");
    assert_eq!((second.formula.source.clone(), formula_text(&second)), (Source::Master(master_shape(1, &[0])), Some("Width*0.4321")));
}

#[test]
fn no_formula_ends_the_search_and_inh_does_not() {
    // `No Formula` with V 2 over a master formula: no formula, and the value 2. `Inh` with V 2: the master's formula is found,
    // the value is 2. Letting any local element end the search fails the second; inheriting past any element fails the first.
    let doc = DocBuilder::new()
        .master(1, half_width_master())
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(10).master(1).cell("Width", "6").no_formula("LocPinX", "2"))
                .shape(ShapeBuilder::new(11).master(1).cell("Width", "6").inh("LocPinX", Some("2"))),
        )
        .build();
    let blocked = read(&doc, &page_shape(&[0]), "LocPinX");
    assert_eq!(blocked.formula.source.clone(), Source::Local);
    assert_eq!(blocked.formula.cell.map(|c| &c.formula), Some(&FormulaSlot::NoFormula));
    assert_eq!(value_text(&blocked), Some("2"));
    assert!(blocked.rebind.is_none(), "nothing was inherited, so nothing rebinds");

    let cached = read(&doc, &page_shape(&[1]), "LocPinX");
    assert_eq!((cached.formula.source.clone(), formula_text(&cached)), (Source::Master(master_shape(1, &[0])), Some("Width*0.5")));
    assert_eq!((cached.value.source.clone(), value_text(&cached)), (Source::Local, Some("2")));
}

#[test]
fn a_local_cell_with_a_value_and_no_formula_is_a_constant_that_ends_the_search() {
    // `V` and no `F` is a local value, and R32 step 1 ends the search at a cell with no `F`.
    let doc = DocBuilder::new()
        .master(1, half_width_master())
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("LocPinX", "9")))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LocPinX");
    assert_eq!(r.formula.source.clone(), Source::Local);
    assert_eq!(r.formula.cell.map(|c| &c.formula), Some(&FormulaSlot::Absent));
    assert_eq!(value_text(&r), Some("9"));
}

#[test]
fn members_inherit_from_the_shape_they_name_and_the_instance_from_neither() {
    // A master with two top-level shapes 5 and 6 and an instance group whose members name 5 and 6. The group's own cells
    // inherit from neither; always using the first master shape fails the second member.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "5")).shape(ShapeBuilder::new(6).cell("Width", "6"));
    let group = ShapeBuilder::new(10).master(1).group().child(ShapeBuilder::new(11).master_shape(5)).child(ShapeBuilder::new(12).master_shape(6));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(group)).build();
    let first = read(&doc, &page_shape(&[0, 0]), "Width");
    assert_eq!((first.formula.source.clone(), value_text(&first)), (Source::Master(master_shape(1, &[0])), Some("5")));
    let second = read(&doc, &page_shape(&[0, 1]), "Width");
    assert_eq!((second.formula.source.clone(), value_text(&second)), (Source::Master(master_shape(1, &[1])), Some("6")));
    let own = read(&doc, &page_shape(&[0]), "Width");
    assert_eq!(own.formula.source.clone(), Source::Default, "an instance of a many-shape master inherits from none of its shapes");
}

#[test]
fn a_nested_shape_with_its_own_master_starts_a_new_scope() {
    // Shape 12 inherits from shape 6 in master 3, not in master 2.
    let m2 = MasterBuilder::new().shape(ShapeBuilder::new(6).cell("Width", "from master 2"));
    let m3 = MasterBuilder::new().shape(ShapeBuilder::new(6).cell("Width", "from master 3"));
    let inner = ShapeBuilder::new(11).master(3).child(ShapeBuilder::new(12).master_shape(6));
    let doc = DocBuilder::new().master(2, m2).master(3, m3).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(2).child(inner))).build();
    let r = read(&doc, &page_shape(&[0, 0, 0]), "Width");
    assert_eq!(r.formula.source.clone(), Source::Master(master_shape(3, &[0])));
    assert_eq!(value_text(&r), Some("from master 3"));
}

#[test]
fn a_master_in_a_master_part_is_not_followed() {
    // R16 of the pages spec: master parts have no `Master`; a hostile one that does is read as one level only.
    let loop_master = MasterBuilder::new().shape(ShapeBuilder::new(5).master(1).cell("Width", "2"));
    let doc = DocBuilder::new().master(1, loop_master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1))).build();
    let r = read(&doc, &page_shape(&[0]), "Width");
    assert_eq!(r.formula.source.clone(), Source::Master(master_shape(1, &[0])));
    // And a shape in the master part itself takes no master step: it ends at its own sheet and defaults.
    let on_master = SheetRef::Shape(master_shape(1, &[0]));
    let own = read(&doc, &on_master, "Height");
    assert_eq!(own.formula.source.clone(), Source::Default);
}

#[test]
fn a_master_that_names_nothing_resolves_as_if_unlinked_and_warns() {
    // Keep the shape, resolve as if unlinked, warn.
    let doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(10).master(99).cell("Width", "3"))
                .shape(ShapeBuilder::new(11).master_shape(7).cell("Width", "4")),
        )
        .build();
    let missing = read(&doc, &page_shape(&[0]), "Height");
    assert_eq!(missing.formula.source.clone(), Source::Default);
    assert!(missing.warnings.contains(Warnings::DANGLING_MASTER), "{:?}", missing.warnings);
    // A shape's own cells still resolve, locally, and a member with no instance above it warns once about that.
    assert_eq!(read(&doc, &page_shape(&[0]), "Width").formula.source, Source::Local);
    let orphan = read(&doc, &page_shape(&[1]), "Height");
    assert!(orphan.warnings.contains(Warnings::DANGLING_MASTER_SHAPE), "{:?}", orphan.warnings);
}

#[test]
fn a_master_shape_id_the_master_does_not_hold_warns() {
    let doc = DocBuilder::new()
        .master(1, half_width_master())
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).child(ShapeBuilder::new(11).master_shape(404))))
        .build();
    let r = read(&doc, &page_shape(&[0, 0]), "Width");
    assert_eq!(r.formula.source.clone(), Source::Default);
    assert!(r.warnings.contains(Warnings::DANGLING_MASTER_SHAPE));
}

// ---- Sheet.N ---------------------------------------------------------------------------------------------------------

#[test]
fn sheet_n_in_an_inherited_formula_rebinds_to_the_instance() {
    // A master with a top-level shape 12 (Width 4) and a member whose PinX is `Sheet.12!Width*0.47`; an instance (ID 40, Width 20)
    // with a member naming it. `Sheet.12` is the instance (so 9.4), and not the page's own shape 12 (Width 9, so 4.23).
    let master = MasterBuilder::new().shape(ShapeBuilder::new(12).group().cell("Width", "4").child(ShapeBuilder::new(13).formula(
        "PinX",
        "Sheet.12!Width*0.47",
        "1.88",
    )));
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(12).cell("Width", "9"))
        .shape(ShapeBuilder::new(40).master(1).cell("Width", "20").child(ShapeBuilder::new(41).master_shape(13)));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let resolver = Resolver::new(&doc);
    let member = page_shape(&[1, 0]);
    let r = resolver.resolve(&member, &CellAddr::direct("PinX")).unwrap();
    assert_eq!(formula_text(&r), Some("Sheet.12!Width*0.47"));
    let rebind = r.rebind.clone().expect("a formula found on the master rebinds");
    let instance = ShapeRef { owner: Owner::Page(1), path: vec![1] };
    assert_eq!(rebind.instance, instance);
    assert_eq!(resolver.rebind(&rebind, 12), Some(instance.clone()), "the master's shape 12 is the instance");
    assert_ne!(resolver.rebind(&rebind, 12), Some(ShapeRef { owner: Owner::Page(1), path: vec![0] }), "not the page's own shape 12");
    assert_eq!(resolver.rebind(&rebind, 13), Some(instance.child(0)), "and the master's member 13 is the instance's member");
    assert_eq!(resolver.rebind(&rebind, 99), None, "a name the master does not hold");
}

#[test]
fn in_a_master_of_several_shapes_the_name_reaches_the_member_standing_for_it() {
    // With several top-level shapes the name reaches the member whose `MasterShape` it is.
    let master =
        MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "1")).shape(ShapeBuilder::new(6).formula("PinX", "Sheet.5!Width", "1"));
    let group = ShapeBuilder::new(40)
        .master(1)
        .group()
        .child(ShapeBuilder::new(41).master_shape(5).cell("Width", "7"))
        .child(ShapeBuilder::new(42).master_shape(6));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(group)).build();
    let resolver = Resolver::new(&doc);
    let r = resolver.resolve(&page_shape(&[0, 1]), &CellAddr::direct("PinX")).unwrap();
    let rebind = r.rebind.unwrap();
    assert_eq!(resolver.rebind(&rebind, 5), Some(ShapeRef { owner: Owner::Page(1), path: vec![0, 0] }));
    assert_eq!(resolver.rebind(&rebind, 6), Some(ShapeRef { owner: Owner::Page(1), path: vec![0, 1] }));
}

#[test]
fn rebinding_does_not_enter_a_nested_instance() {
    // A nested shape with its own `Master` is its own scope; its members stand for shapes of its master, not ours. The nested
    // instance comes first in the group, so a walk that entered it would meet member 13 before member 11.
    let outer = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6).formula("PinX", "1", "1")));
    let inner = MasterBuilder::new().shape(ShapeBuilder::new(6).cell("Width", "inner"));
    let page = PageBuilder::new().shape(
        ShapeBuilder::new(10)
            .master(1)
            .child(ShapeBuilder::new(12).master(2).child(ShapeBuilder::new(13).master_shape(6)))
            .child(ShapeBuilder::new(11).master_shape(6)),
    );
    let doc = DocBuilder::new().master(1, outer).master(2, inner).page(1, page).build();
    let resolver = Resolver::new(&doc);
    let r = resolver.resolve(&page_shape(&[0, 1]), &CellAddr::direct("PinX")).unwrap();
    let found = resolver.rebind(&r.rebind.unwrap(), 6);
    assert_eq!(found, Some(ShapeRef { owner: Owner::Page(1), path: vec![0, 1] }), "member 11, and not member 13 under the nested instance");
    // And the member under the nested instance has the nested master's shape for its master, not ours.
    let under = resolver.resolve(&page_shape(&[0, 0, 0]), &CellAddr::direct("Width")).unwrap();
    assert_eq!(under.formula.source, Source::Master(master_shape(2, &[0])));
}

// ---- the style step --------------------------------------------------------------------------------------------------

fn styled_pair() -> Document {
    // Style 1 (A) holds LineWeight 0.02; style 2 (B) holds LineWeight 0.5 and FillPattern 3.
    DocBuilder::new()
        .style(1, StyleBuilder::new().name("A").cell("LineWeight", "0.02"))
        .style(2, StyleBuilder::new().name("B").cell("LineWeight", "0.5").cell("FillPattern", "3"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), Some(2), None)))
        .build()
}

#[test]
fn each_style_link_reaches_only_its_family() {
    // `LineStyle` A (LineWeight 0.02) and `FillStyle` B (LineWeight 0.5, FillPattern 3): LineWeight is 0.02, FillPattern comes from B.
    // Reading any style, or the first style, gives 0.5 for LineWeight.
    let doc = styled_pair();
    let line = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!((line.formula.source.clone(), value_text(&line)), (style(Family::Line, 1), Some("0.02")));
    let fill = read(&doc, &page_shape(&[0]), "FillPattern");
    assert_eq!((fill.formula.source.clone(), value_text(&fill)), (style(Family::Fill, 2), Some("3")));
}

#[test]
fn a_family_cell_is_not_read_from_another_familys_style() {
    // The text link reaches no line or fill cell: a style named only by `FillStyle` that holds `LineWeight` is not consulted for it.
    let doc = DocBuilder::new()
        .style(2, StyleBuilder::new().cell("LineWeight", "0.5").cell("Size", "9"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(None, Some(2), None)))
        .build();
    assert_eq!(read(&doc, &page_shape(&[0]), "LineWeight").formula.source, Source::Default);
}

#[test]
fn a_style_chain_goes_style_to_style_along_the_same_family() {
    // C has no LinePattern, is based on D with 2, which is based on the root with 1: expect 2.
    let doc = DocBuilder::new()
        .root(|r| r.cell("LinePattern", "1"))
        .style(3, StyleBuilder::new().name("C").links(Some(4), None, None))
        .style(4, StyleBuilder::new().name("D").cell("LinePattern", "2").links(Some(0), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(3), None, None)))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LinePattern");
    assert_eq!((r.formula.source.clone(), value_text(&r)), (style(Family::Line, 4), Some("2")));
    // And the root answers when nothing nearer does.
    let root_only = DocBuilder::new()
        .root(|r| r.cell("LinePattern", "1"))
        .style(3, StyleBuilder::new().links(Some(0), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(3), None, None)))
        .build();
    let r = read(&root_only, &page_shape(&[0]), "LinePattern");
    assert_eq!((r.formula.source.clone(), value_text(&r)), (style(Family::Line, 0), Some("1")));
}

#[test]
fn a_shape_only_cell_has_no_style_step() {
    // A shape with no master and no PinX, linked to a style that holds a PinX: expect the default 0.
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("PinX", "5").cell("LineWeight", "0.02"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), Some(1), Some(1))))
        .build();
    let r = read(&doc, &page_shape(&[0]), "PinX");
    assert_eq!(r.formula.source.clone(), Source::Default);
    assert_eq!(r.default.kind, ValueKind::Length);
    assert!(r.value.cell.is_none());
}

#[test]
fn a_cell_in_no_family_reads_the_root_style_and_not_the_default() {
    // A group with no master and no local SelectMode or DisplayMode, a root holding 1 and 2: expect 1 and 2, not the 0; a root
    // holding SelectMode 0 gives 0, so a constant cannot pass.
    let doc = DocBuilder::new()
        .root(|r| r.cell("SelectMode", "1").cell("DisplayMode", "2"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).group()))
        .build();
    let select = read(&doc, &page_shape(&[0]), "SelectMode");
    assert_eq!((select.formula.source.clone(), value_text(&select)), (Source::Style { id: 0, family: None }, Some("1")));
    assert_eq!(value_text(&read(&doc, &page_shape(&[0]), "DisplayMode")), Some("2"));
    let zero = DocBuilder::new().root(|r| r.cell("SelectMode", "0")).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).group())).build();
    assert_eq!(value_text(&read(&zero, &page_shape(&[0]), "SelectMode")), Some("0"));
    // Another style holding the cell is not the root, so it is not read.
    let other = DocBuilder::new()
        .root(|r| r.cell("SelectMode", "1"))
        .style(1, StyleBuilder::new().cell("SelectMode", "2"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).group().links(Some(1), Some(1), Some(1))))
        .build();
    assert_eq!(value_text(&read(&other, &page_shape(&[0]), "SelectMode")), Some("1"));
}

#[test]
fn a_missing_cell_reads_its_default_and_a_few_read_a_printed_formula() {
    // A shape with no LocPinX anywhere: 0 whatever Width holds. A shape with Width 2 and no TxtWidth anywhere: its printed formula
    // `Width*1`, and 0 inches fails.
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "2"))).build();
    let pin = read(&doc, &page_shape(&[0]), "LocPinX");
    assert_eq!((pin.formula.source.clone(), pin.default.kind, pin.default.formula), (Source::Default, ValueKind::Length, None));
    for (cell, formula) in [
        ("TxtWidth", "Width*1"),
        ("TxtHeight", "Height*1"),
        ("TxtPinX", "Width*0.5"),
        ("TxtPinY", "Height*0.5"),
        ("TxtLocPinX", "TxtWidth*0.5"),
        ("TxtLocPinY", "TxtHeight*0.5"),
        ("ImgWidth", "Width*1"),
        ("ImgHeight", "Height*1"),
    ] {
        let r = read(&doc, &page_shape(&[0]), cell);
        assert_eq!((r.formula.source.clone(), r.default.formula), (Source::Default, Some(formula)), "{cell}");
    }
    // The seventh Text Transform cell prints no formula: Learn gives its default as 0 degrees, the same as the kind's.
    let angle = read(&doc, &page_shape(&[0]), "TxtAngle");
    assert_eq!((angle.formula.source.clone(), angle.default.kind, angle.default.formula), (Source::Default, ValueKind::Angle, None));
    // A cell that is there reads itself, not the default.
    let there = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("TxtWidth", "0.5"))).build();
    assert_eq!(read(&there, &page_shape(&[0]), "TxtWidth").formula.source, Source::Local);
}

#[test]
fn a_style_chain_with_a_cycle_ends_with_a_warning_and_the_default() {
    // Style 1 linked to style 2 and style 2 to style 1 for line, neither holding LineWeight: the walk ends, a warning, 0 inches.
    // An unbounded walk hangs.
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().links(Some(2), None, None))
        .style(2, StyleBuilder::new().links(Some(1), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!((r.formula.source.clone(), r.default.kind), (Source::Default, ValueKind::Length));
    assert!(r.warnings.contains(Warnings::STYLE_CYCLE), "{:?}", r.warnings);
}

#[test]
fn a_style_chain_longer_than_the_limit_ends_at_it() {
    // Walk at most 64 steps. A line of 200 styles with the value at the end is not reached.
    let mut b = DocBuilder::new();
    for id in 1..=200_u32 {
        let sb = StyleBuilder::new().links(Some(id + 1), None, None);
        b = b.style(id, if id == 200 { sb.cell("LineWeight", "9") } else { sb });
    }
    let doc = b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build();
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!(r.formula.source.clone(), Source::Default);
    assert!(r.warnings.contains(Warnings::STYLE_CHAIN_TOO_LONG));
    // Within the limit it is found.
    let mut b = DocBuilder::new();
    for id in 1..=40_u32 {
        let sb = StyleBuilder::new().links(Some(id + 1), None, None);
        b = b.style(id, if id == 40 { sb.cell("LineWeight", "9") } else { sb });
    }
    let doc = b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build();
    assert_eq!(value_text(&read(&doc, &page_shape(&[0]), "LineWeight")), Some("9"));
}

#[test]
fn a_link_to_a_style_that_does_not_exist_is_no_style_and_warns() {
    // Treat what follows as no style: the default, not the root, and not the master's link.
    let doc = DocBuilder::new()
        .root(|r| r.cell("LineWeight", "0.01"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(77), None, None)))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!(r.formula.source.clone(), Source::Default);
    assert!(r.warnings.contains(Warnings::DANGLING_STYLE));
    // A link that ends mid-chain at nothing does the same.
    let mid = DocBuilder::new()
        .style(1, StyleBuilder::new().links(Some(77), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    assert!(read(&mid, &page_shape(&[0]), "LineWeight").warnings.contains(Warnings::DANGLING_STYLE));
}

#[test]
fn a_style_that_links_to_itself_is_a_cycle() {
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().links(Some(1), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert!(r.warnings.contains(Warnings::STYLE_CYCLE));
}

fn doc_23(master_line_weight: Option<&str>, instance_link: Option<u32>) -> Document {
    // Style 1 (A) LineWeight 0.03, style 2 (B) LineWeight 0.05. A master shape linked to A, with its own LineWeight 0.02 or none.
    let shape = ShapeBuilder::new(5).links(Some(1), None, None);
    let shape = match master_line_weight {
        Some(v) => shape.cell("LineWeight", v),
        None => shape,
    };
    DocBuilder::new()
        .style(1, StyleBuilder::new().cell("LineWeight", "0.03"))
        .style(2, StyleBuilder::new().cell("LineWeight", "0.05"))
        .master(1, MasterBuilder::new().shape(shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).links(instance_link, None, None)))
        .build()
}

#[test]
fn the_master_shapes_cell_outranks_the_instances_style() {
    // Master shape with local LineWeight 0.02, linked to A (0.03); instance with its own LineStyle B (0.05): 0.02.
    // Trying the instance's style before the master shape gives 0.05.
    let doc = doc_23(Some("0.02"), Some(2));
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!((r.formula.source.clone(), value_text(&r)), (Source::Master(master_shape(1, &[0])), Some("0.02")));
}

#[test]
fn the_instances_own_link_comes_before_the_master_shapes() {
    // Over a master shape with no LineWeight, the instance's own link gives 0.05, and with no link of its own, 0.03.
    // Always using the master's link gives 0.03 in both.
    let with_link = doc_23(None, Some(2));
    let own = read(&with_link, &page_shape(&[0]), "LineWeight");
    assert_eq!((own.formula.source.clone(), value_text(&own)), (style(Family::Line, 2), Some("0.05")));
    let doc = doc_23(None, None);
    let masters = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!((masters.formula.source.clone(), value_text(&masters)), (style(Family::Line, 1), Some("0.03")));
}

#[test]
fn a_style_flag_of_zero_skips_that_style_for_its_family() {
    // The choice, our rule: a style with `EnableLineProps` 0 is skipped for line cells
    // even when it holds one, and the chain goes on along its link.
    let doc = DocBuilder::new()
        .root(|r| r.cell("LineWeight", "0.01"))
        .style(1, StyleBuilder::new().cell("EnableLineProps", "0").cell("LineWeight", "0.9").links(Some(0), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    let r = read(&doc, &page_shape(&[0]), "LineWeight");
    assert_eq!((r.formula.source.clone(), value_text(&r)), (style(Family::Line, 0), Some("0.01")));
    // A flag of 1, or no flag anywhere, leaves the style in.
    let on = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("EnableLineProps", "1").cell("LineWeight", "0.9"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    assert_eq!(value_text(&read(&on, &page_shape(&[0]), "LineWeight")), Some("0.9"));
    // The flag a style does not hold is read along its own link: here nothing nearer holds one, so the root's 1 applies.
    let by_chain = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("LineWeight", "0.9").links(Some(2), None, None))
        .style(2, StyleBuilder::new().links(Some(0), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    assert_eq!(value_text(&read(&by_chain, &page_shape(&[0]), "LineWeight")), Some("0.9"), "the flag of style 1 is the root's 1, by the chain");
    // And where the base style says 0, a style that holds nothing about it takes that 0, and is skipped.
    let by_base = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("LineWeight", "0.9").links(Some(2), None, None))
        .style(2, StyleBuilder::new().cell("EnableLineProps", "0").links(Some(0), None, None))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)))
        .build();
    assert_eq!(read(&by_base, &page_shape(&[0]), "LineWeight").formula.source, Source::Default, "style 1 takes its base's 0");
}

#[test]
fn a_style_resolves_its_own_cells_along_its_own_links() {
    // A style's own `Inh` cell reads along the chain, the family's link. Asking a style for a line cell it lacks walks its line link.
    let doc = DocBuilder::new()
        .root(|r| r.cell("LinePattern", "1"))
        .style(3, StyleBuilder::new().links(Some(4), None, None).inh("LineWeight", Some("0.5")))
        .style(4, StyleBuilder::new().cell("LineWeight", "0.04").cell("LinePattern", "2"))
        .build();
    let weight = read(&doc, &SheetRef::Style(3), "LineWeight");
    assert_eq!(weight.formula.source.clone(), style(Family::Line, 4), "`Inh` does not end the search");
    assert_eq!((weight.value.source.clone(), value_text(&weight)), (Source::Local, Some("0.5")));
    let pattern = read(&doc, &SheetRef::Style(3), "LinePattern");
    assert_eq!((pattern.formula.source.clone(), value_text(&pattern)), (style(Family::Line, 4), Some("2")));
    // A shape-only cell on a style has no chain, and a cell in no family goes to the root.
    assert_eq!(read(&doc, &SheetRef::Style(3), "PinX").formula.source, Source::Default);
}

#[test]
fn hide_for_apply_is_not_inherited_by_a_based_on_style() {
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("HideForApply", "1").links(Some(0), Some(0), Some(0)))
        .style(2, StyleBuilder::new().links(Some(1), Some(1), Some(1)))
        .build();
    assert_eq!(value_text(&read(&doc, &SheetRef::Style(1), "HideForApply")), Some("1"));
    assert_eq!(read(&doc, &SheetRef::Style(2), "HideForApply").formula.source, Source::Default);
}

// ---- page sheets and the document sheet --------------------------------------------------------------------------------

#[test]
fn a_page_sheet_reads_the_root_style_and_never_a_master() {
    // A page sheet with no PageLeftMargin and a root holding 0.25: 0.25, not the 0. Of the page batch: a page sheet
    // linked to style 3, which holds no PagesX, with the root holding 1: 1 (stopping at style 3 gives 0), and a cell style 3 does
    // hold is not read: the link is not walked.
    let doc = DocBuilder::new()
        .root(|r| r.cell("PageLeftMargin", "0.25").cell("PagesX", "1"))
        .style(3, StyleBuilder::new().cell("PageRightMargin", "9"))
        .page(1, PageBuilder::new().links(Some(3), Some(3), Some(3)))
        .master(1, MasterBuilder::new().links(Some(3), Some(3), Some(3)))
        .build();
    for sheet in [SheetRef::Page(1), SheetRef::Master(1)] {
        let left = read(&doc, &sheet, "PageLeftMargin");
        assert_eq!((left.formula.source.clone(), value_text(&left)), (Source::Style { id: 0, family: None }, Some("0.25")), "{sheet:?}");
        assert_eq!(value_text(&read(&doc, &sheet, "PagesX")), Some("1"), "{sheet:?}");
        assert_eq!(read(&doc, &sheet, "PageRightMargin").formula.source, Source::Default, "{sheet:?}: style 3 is not walked");
    }
}

#[test]
fn the_page_sheets_own_cell_wins_over_the_root_and_a_root_no_formula_zero_is_a_value() {
    // A root holding PageWidth `No Formula` and V 0 and a page sheet holding 11: 11. Where the page sheet holds nothing the root's
    // zero is what is found: that it means no setting is the caller's.
    let doc = DocBuilder::new()
        .root(|r| r.no_formula("PageWidth", "0").no_formula("PageHeight", "0"))
        .page(1, PageBuilder::new().cell("PageWidth", "11"))
        .build();
    let width = read(&doc, &SheetRef::Page(1), "PageWidth");
    assert_eq!((width.formula.source.clone(), value_text(&width)), (Source::Local, Some("11")));
    let height = read(&doc, &SheetRef::Page(1), "PageHeight");
    assert_eq!(
        (height.formula.source.clone(), value_text(&height), height.formula.cell.map(|c| c.formula.clone())),
        (Source::Style { id: 0, family: None }, Some("0"), Some(FormulaSlot::NoFormula))
    );
}

#[test]
fn the_document_sheet_walks_local_then_root() {
    // Is open for the document sheet: the model reads local, root, default, as for a page sheet, and says so.
    let doc = DocBuilder::new().root(|r| r.cell("PreviewQuality", "1")).doc_cell("OutputFormat", "2").build();
    assert_eq!(read(&doc, &SheetRef::Document, "OutputFormat").formula.source, Source::Local);
    assert_eq!(read(&doc, &SheetRef::Document, "PreviewQuality").formula.source, Source::Style { id: 0, family: None });
}

// ---- rows ------------------------------------------------------------------------------------------------------------------

fn geometry_doc() -> Document {
    // Master: Geometry section 0, row IX 2 of type LineTo with X 1, Y 2; row IX 3 with X 3. Instance: row 2 holds only X 7; row 3 is Del.
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .row_type("Geometry", Some(0), RowKey::Ix(2), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "1")
            .row_cell("Geometry", Some(0), RowKey::Ix(2), "Y", "2")
            .row_type("Geometry", Some(0), RowKey::Ix(3), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(3), "X", "3")
            .row_cell("Geometry", Some(1), RowKey::Ix(1), "X", "9"),
    );
    let instance = ShapeBuilder::new(10).master(1).row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "7").del_row("Geometry", Some(0), RowKey::Ix(3));
    DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build()
}

#[test]
fn rows_merge_cell_by_cell() {
    // A master row IX 2 with X 1 and Y 2, an instance row IX 2 holding only X 7: X is 7, Y is 2, and T is the master's. A row `Del`
    // is gone from the effective section.
    let doc = geometry_doc();
    let at = page_shape(&[0]);
    let x = Resolver::new(&doc).resolve(&at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X")).unwrap();
    assert_eq!((x.formula.source.clone(), value_text(&x)), (Source::Local, Some("7")));
    let y = Resolver::new(&doc).resolve(&at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "Y")).unwrap();
    assert_eq!((y.formula.source.clone(), value_text(&y)), (Source::Master(master_shape(1, &[0])), Some("2")));
    // The deleted row is gone, whatever the master holds there.
    assert!(Resolver::new(&doc).resolve(&at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(3), "X")).is_none());

    let rows = Resolver::new(&doc).effective_rows(&at, &SectionKey::new("Geometry", Some(0))).unwrap();
    assert_eq!(rows.len(), 1, "row 3 is deleted");
    assert_eq!(rows[0].key, RowKey::Ix(2));
    assert_eq!(rows[0].row_type.as_deref(), Some("LineTo"), "the master's T");
    let names: Vec<&str> = rows[0].cells.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["X", "Y"]);
    assert_eq!(rows[0].cells[0].1.formula.source, Source::Local);
    assert!(matches!(rows[0].cells[1].1.formula.source, Source::Master(_)));
}

#[test]
fn a_local_rows_type_wins_over_the_masters_and_the_masters_stands_when_the_local_row_has_none() {
    // V13 gives the master's `T` for a local row with none; a local `T` is the local rule's and replaces it.
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .row_type("Geometry", Some(0), RowKey::Ix(2), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "1")
            .row_type("Geometry", Some(0), RowKey::Ix(3), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(3), "X", "3"),
    );
    let instance = ShapeBuilder::new(10)
        .master(1)
        .row_type("Geometry", Some(0), RowKey::Ix(2), "ArcTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "Y", "2")
        .row_cell("Geometry", Some(0), RowKey::Ix(3), "Y", "4");
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let rows = Resolver::new(&doc).effective_rows(&page_shape(&[0]), &SectionKey::new("Geometry", Some(0))).unwrap();
    let types: Vec<(RowKey, Option<&str>)> = rows.iter().map(|r| (r.key.clone(), r.row_type.as_deref())).collect();
    assert_eq!(types, [(RowKey::Ix(2), Some("ArcTo")), (RowKey::Ix(3), Some("LineTo"))]);
}

/// `Resolver::row_type` gives the type `effective_rows` gives, for one row, without merging the section: a row's own `T`, the master's when the
/// row has none, a local `T` over the master's, none for a row that is deleted or not there.
#[test]
fn a_rows_type_is_found_without_merging_the_section() {
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .row_type("Geometry", Some(0), RowKey::Ix(2), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "1")
            .row_type("Geometry", Some(0), RowKey::Ix(3), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(3), "X", "3")
            .row_type("Geometry", Some(0), RowKey::Ix(4), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(4), "X", "4"),
    );
    let instance = ShapeBuilder::new(10)
        .master(1)
        .row_type("Geometry", Some(0), RowKey::Ix(2), "ArcTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(3), "Y", "4")
        .del_row("Geometry", Some(0), RowKey::Ix(4));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let res = Resolver::new(&doc);
    let section = SectionKey::new("Geometry", Some(0));
    let at = page_shape(&[0]);
    let type_of = |ix: u32| res.row_type(&at, &section, &RowKey::Ix(ix)).map(|t| t.to_string());
    assert_eq!(type_of(2).as_deref(), Some("ArcTo"), "the local T");
    assert_eq!(type_of(3).as_deref(), Some("LineTo"), "the master's, the local row having none");
    assert_eq!(type_of(4), None, "a deleted row is gone, whatever the master holds");
    assert_eq!(type_of(9), None, "no such row");
    assert_eq!(res.row_type(&at, &SectionKey::new("Geometry", Some(1)), &RowKey::Ix(2)), None, "no such section");
    // And it is what the merge says, row by row.
    for row in res.effective_rows(&at, &section).unwrap() {
        assert_eq!(res.row_type(&at, &section, &row.key), row.row_type, "{:?}", row.key);
    }
}

#[test]
fn sections_merge_too_by_name_and_geometry_index() {
    // An inherited Geometry section with no local counterpart is inherited whole; the local one is found by its IX.
    let doc = geometry_doc();
    let at = page_shape(&[0]);
    let second = Resolver::new(&doc).resolve(&at, &CellAddr::in_row("Geometry", Some(1), RowKey::Ix(1), "X")).unwrap();
    assert_eq!((second.formula.source.clone(), value_text(&second)), (Source::Master(master_shape(1, &[0])), Some("9")));
    let rows = Resolver::new(&doc).effective_rows(&at, &SectionKey::new("Geometry", Some(1))).unwrap();
    assert_eq!(rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(1)]);
}

#[test]
fn a_deleted_section_is_gone_with_everything_the_master_holds_in_it() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Scratch", None, RowKey::Ix(0), "X", "4"));
    let instance = ShapeBuilder::new(10).master(1).del_row("Scratch", None, RowKey::Ix(0));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    assert!(resolver.resolve(&page_shape(&[0]), &CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X")).is_none());
    // The same cell on an instance that did not delete the row is the master's: so None above is the delete and not a miss.
    let other = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Scratch", None, RowKey::Ix(0), "X", "4")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let kept = Resolver::new(&other).resolve(&page_shape(&[0]), &CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X")).unwrap();
    assert_eq!(value_text(&kept), Some("4"));
}

#[test]
fn rows_of_a_named_section_merge_by_name_and_keep_the_masters_order() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("User", None, RowKey::name("a"), "Value", "1").row_cell(
        "User",
        None,
        RowKey::name("b"),
        "Value",
        "2",
    ));
    let instance = ShapeBuilder::new(10).master(1).row_cell("User", None, RowKey::name("b"), "Value", "20").row_cell(
        "User",
        None,
        RowKey::name("c"),
        "Value",
        "3",
    );
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let rows = Resolver::new(&doc).effective_rows(&page_shape(&[0]), &SectionKey::new("User", None)).unwrap();
    let keys: Vec<RowKey> = rows.iter().map(|r| r.key.clone()).collect();
    assert_eq!(keys, [RowKey::name("a"), RowKey::name("b"), RowKey::name("c")]);
    assert_eq!(
        value_text(&Resolver::new(&doc).resolve(&page_shape(&[0]), &CellAddr::in_row("User", None, RowKey::name("b"), "Value")).unwrap()),
        Some("20")
    );
}

#[test]
fn text_rows_take_their_cells_from_the_text_style() {
    // A `Character` row is text-family: row 0's Size comes from the TextStyle chain when neither shape nor master holds it.
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().row_cell("Character", None, RowKey::Ix(0), "Size", "0.1667"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(None, None, Some(1))))
        .build();
    let r = Resolver::new(&doc).resolve(&page_shape(&[0]), &CellAddr::in_row("Character", None, RowKey::Ix(0), "Size")).unwrap();
    assert_eq!((r.formula.source.clone(), value_text(&r)), (style(Family::Text, 1), Some("0.1667")));
    let rows = Resolver::new(&doc).effective_rows(&page_shape(&[0]), &SectionKey::new("Character", None)).unwrap();
    assert_eq!(rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(0)]);
}

// ---- e3, r20 -------------------------------------------------------------------------------------------------------------

#[test]
fn inh_with_no_source_keeps_its_value_and_has_no_formula() {
    // The stored V stands, and on an evaluation the formula is empty and R19 applies.
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).inh("PinX", Some("7")))).build();
    let r = read(&doc, &page_shape(&[0]), "PinX");
    assert_eq!((r.value.source.clone(), value_text(&r)), (Source::Local, Some("7")));
    assert_eq!(r.formula.source.clone(), Source::Default);
    assert_eq!(r.default.kind, ValueKind::Length);
}

#[test]
fn a_cell_with_a_unit_and_no_value_reads_its_unit_default() {
    // R20 / V11, through the resolution: the stopping cell has U `MM` and no V, and a missing value is 0 inches, by the unit.
    let doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new().shape(
                ShapeBuilder::new(10)
                    .set(CellAddr::direct("Width"), diacraft_doc::Cell::read(None, None, Some("MM"), None, &mut 0))
                    .set(CellAddr::direct("LockWidth"), diacraft_doc::Cell::read(None, None, Some("BOOL"), None, &mut 0))
                    .set(CellAddr::direct("FillForegnd"), diacraft_doc::Cell::read(None, None, Some("COLOR"), None, &mut 0))
                    .set(CellAddr::direct("Size"), diacraft_doc::Cell::read(None, None, None, None, &mut 0)),
            ),
        )
        .build();
    let at = page_shape(&[0]);
    for (cell, kind) in
        [("Width", ValueKind::Length), ("LockWidth", ValueKind::Scalar), ("FillForegnd", ValueKind::Color), ("Size", ValueKind::Scalar)]
    {
        let r = read(&doc, &at, cell);
        assert_eq!(r.formula.source.clone(), Source::Local, "{cell}");
        assert!(r.value.cell.is_none() || value_text(&r).is_none(), "{cell}");
        assert_eq!(r.kind_if_no_value(), kind, "{cell}");
    }
}

#[test]
fn an_unknown_cell_resolves_like_any_other_and_a_case_variant_is_another_cell() {
    // `pinx` is not `PinX`: it is an unknown cell, found locally if it is there, and a default if not.
    let doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("PinX", "3").cell("Mystery", "m")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("pinx", "9")))
        .build();
    let at = page_shape(&[0]);
    assert_eq!(value_text(&read(&doc, &at, "pinx")), Some("9"));
    let upper = read(&doc, &at, "PinX");
    assert_eq!(
        (upper.formula.source.clone(), value_text(&upper)),
        (Source::Master(master_shape(1, &[0])), Some("3")),
        "the exact-case cell is inherited"
    );
    let unknown = read(&doc, &at, "Mystery");
    assert_eq!((unknown.formula.source.clone(), value_text(&unknown)), (Source::Master(master_shape(1, &[0])), Some("m")));
    assert_eq!(read(&doc, &at, "PINX").formula.source, Source::Default);
}

#[test]
fn the_free_function_resolves_a_direct_cell() {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "2"))).build();
    let r = diacraft_doc::resolve(&doc, page_shape(&[0]), &diacraft_doc::CellName::new("Width")).unwrap();
    assert_eq!(value_text(&r), Some("2"));
    assert!(diacraft_doc::resolve(&doc, page_shape(&[4]), &diacraft_doc::CellName::new("Width")).is_none(), "no such shape");
    assert!(diacraft_doc::resolve(&doc, SheetRef::Style(99), &diacraft_doc::CellName::new("Width")).is_none(), "no such style");
}
