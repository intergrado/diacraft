//! Crafted small files that a review found to hang the model or to eat memory through plain edits, each built to the shape that did it
//! and run under a hang guard (a minute: tests have raced under load before, and no cost is held to a time; the work each does when it is
//! right is a second or two at most at load 50). A fuzz of random documents does not build these.
//!
//! Where the claim is "linear" or "copies nothing", the test counts: the resolver's own counters, the work counters of `diacraft_doc::work`,
//! and what a write left shared, none of which depends on the speed of the machine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, Connect, Document, Master, Owner, Page, Resolver, Row, Section, SectionKey, Shape, SheetRef, Source, Style,
    Value, WriteOrigin,
};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

/// The hang guard for one crafted case: no measure of cost, which is counted.
const LIMIT: Duration = Duration::from_secs(60);

fn page_shape(path: &[u32]) -> SheetRef {
    SheetRef::Shape(diacraft_doc::ShapeRef { owner: Owner::Page(1), path: path.to_vec() })
}

/// Fails the test when `start` is older than the bound: called inside a loop, so that a hang is a failure after a minute and not after hours.
fn in_time(start: Instant, what: &str, at: u64) {
    assert!(start.elapsed() < LIMIT, "{what}: still running after {LIMIT:?} at step {at}");
}

fn with_cell(shape: &mut Shape, name: &str, cell: Cell) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), cell);
}

// ---- finding 1: `Sheet.N` in every member of a big instance --------------------------------------------------------------------

/// A master (ID 1) holding one group (ID 1) of `n` children, IDs 2 to n+1, each with a `PinX` that names the group; and a page with an
/// instance of it that lists only every second child as a member (the others were omitted, which allows). The member for child `c`
/// is the shape of `ID` 1000+c.
fn instance_with_half_the_members(n: u32) -> Document {
    let mut group = Shape::new(1);
    group.kind = diacraft_doc::ShapeType::Group;
    let mut children = Vec::with_capacity(n as usize);
    // One parsed formula, shared by every child, as a master of many identical parts shares it.
    let pin = Cell::with_formula("Sheet.1!Width", Some(Value::from_text("1")));
    for c in 0..n {
        let mut child = Shape::new(c + 2);
        with_cell(&mut child, "PinX", pin.clone());
        children.push(Arc::new(child));
    }
    group.children = children.into_iter().collect();
    let mut instance = Shape::new(40);
    instance.master = Some(1);
    let members: Vec<Arc<Shape>> = (0..n)
        .filter(|c| c % 2 == 1)
        .map(|c| {
            let mut m = Shape::new(1000 + c);
            m.master_shape = Some(c + 2);
            Arc::new(m)
        })
        .collect();
    instance.children = members.into_iter().collect();
    let mut doc = DocBuilder::new().build();
    doc.masters = vec![Arc::new(Master { id: 1, shapes: vec![Arc::new(group)].into_iter().collect(), ..Master::default() })].into_iter().collect();
    let mut page = Page::new(1, "p");
    page.shapes = vec![Arc::new(instance)].into_iter().collect();
    doc.pages = vec![Arc::new(page)].into_iter().collect();
    doc
}

#[test]
fn a_sheet_n_for_every_member_of_a_big_instance_is_linear_and_not_quadratic() {
    // The review's file: 1,000,000 members is 32 MB of XML and 75 KB deflated. Here 80,000 members, and a `Sheet.N` read for each, and for
    // the master shapes nothing stands for (the omitted ones), which is the worst case of a scan.
    let n = 160_000;
    let doc = instance_with_half_the_members(n);
    let resolver = Resolver::new(&doc);
    let instance = diacraft_doc::ShapeRef { owner: Owner::Page(1), path: vec![0] };
    let start = Instant::now();
    let mut asked = 0_u64;
    for m in 0..n / 2 {
        in_time(start, "rebind", asked);
        let member = page_shape(&[0, m]);
        let r = resolver.resolve(&member, &CellAddr::direct("PinX")).unwrap();
        let rebind = r.rebind.expect("the formula is the master's, so it rebinds");
        // `Sheet.1` is the group, which is the instance; the master child this member stands for is the member itself; the child before it
        // was omitted, so nothing stands for it.
        let child = 2 * m + 1 + 2;
        assert_eq!(resolver.rebind(&rebind, 1).as_ref(), Some(&instance));
        assert_eq!(resolver.rebind(&rebind, child), Some(instance.child(m)));
        assert_eq!(resolver.rebind(&rebind, child - 1), None, "child {}", child - 1);
        asked += 3;
    }
    // The members of the instance were mapped once, and so was the master's: not once for each name asked.
    let stats = resolver.stats();
    eprintln!("{asked} Sheet.N reads over {} members: {:?} in all", n / 2, start.elapsed());
    assert_eq!((stats.member_maps, stats.master_path_maps), (1, 1), "{stats:?}");
}

// ---- the paths of a deep master -----------------------------------------------------------------------------------------------

/// `length` shapes nested one in the next, from the shape with `ID` `first`. A member (`stand_for`) has `ID` `first + shift` and stands for
/// the master shape of that `first`.
fn nest(first: u32, length: u32, stand_for: bool, shift: u32) -> Shape {
    let mut node: Option<Shape> = None;
    for k in (0..length).rev() {
        let id = first + k;
        let mut shape = Shape::new(if stand_for { id + shift } else { id });
        if stand_for {
            shape.master_shape = Some(id);
        }
        if let Some(inner) = node.take() {
            shape.children = std::iter::once(Arc::new(inner)).collect();
        }
        node = Some(shape);
    }
    node.unwrap_or_else(|| Shape::new(first))
}

/// A master (ID 1) of one group (ID 1) holding `chains` chains of `length` nested shapes, and a page with an instance (ID 40) of it that
/// lists a member for every one of them: `1 + chains * length` shapes in each, the deepest `length + 1` levels down.
fn deep_master(chains: u32, length: u32) -> Document {
    let tree = |stand_for: bool| {
        let mut group = Shape::new(if stand_for { 40 } else { 1 });
        group.kind = diacraft_doc::ShapeType::Group;
        group.children = (0..chains).map(|c| Arc::new(nest(2 + c * length, length, stand_for, 50_000_000))).collect();
        group
    };
    let mut doc = DocBuilder::new().build();
    doc.masters =
        vec![Arc::new(Master { id: 1, shapes: vec![Arc::new(tree(false))].into_iter().collect(), ..Master::default() })].into_iter().collect();
    let mut instance = tree(true);
    instance.master = Some(1);
    let mut page = Page::new(1, "p");
    page.shapes = vec![Arc::new(instance)].into_iter().collect();
    doc.pages = vec![Arc::new(page)].into_iter().collect();
    doc
}

/// Asks `Sheet.N` for the deepest shape of each chain: the first read of the instance maps the master's shapes and the instance's members.
fn read_the_deepest(doc: &Document, resolver: &Resolver<'_>, chains: u32, length: u32) {
    let rebind = diacraft_doc::Rebind { instance: diacraft_doc::ShapeRef { owner: Owner::Page(1), path: vec![0] }, master: 1 };
    for c in 0..chains {
        let deepest = 2 + c * length + (length - 1);
        let got = resolver.rebind(&rebind, deepest).unwrap_or_else(|| panic!("no member for {deepest}"));
        let mut want = vec![0, c];
        want.extend(std::iter::repeat_n(0, (length - 1) as usize));
        assert_eq!(got.path, want, "chain {c}");
    }
    assert!(doc.pages.get(0).is_some());
}

#[test]
fn the_paths_of_a_deep_master_are_not_kept_one_for_each_shape() {
    // The review: a million shapes 200 levels deep is within the caps, the first member read added 858 MB (a path of up to 200 numbers for
    // each shape), and the map of an instance's members held the same again. A few such masters would abort the browser build. The maps
    // keep each shape's parent and position, and a path is built when a name is asked for.
    let (chains, length) = (500, 199);
    let doc = deep_master(chains, length);
    let shapes = 2 * (1 + u64::from(chains) * u64::from(length));
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    read_the_deepest(&doc, &resolver, chains, length);
    in_time(start, "reads of the deepest shapes", u64::from(chains));
    let stats = resolver.stats();
    eprintln!("{shapes} shapes 200 levels deep: {} numbers held by the path maps, {:?} for {chains} reads", stats.path_slots, start.elapsed());
    // A few numbers for each shape, not a path of ~100 on average.
    assert!(stats.path_slots <= 8 * shapes, "{} numbers for {shapes} shapes: {stats:?}", stats.path_slots);
    assert_eq!((stats.member_maps, stats.master_path_maps), (1, 1), "{stats:?}");
}

/// The resident memory of this process, in kilobytes, where the system says (`/proc`).
fn resident_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[test]
#[ignore = "measures the memory of the review's file (a million shapes, 200 levels): run it alone, `-- --ignored --nocapture --test-threads=1`"]
fn a_million_shapes_two_hundred_levels_deep_measured() {
    // 5,025 chains of 199 nested shapes, 1,000,000 in the master and 1,000,000 members of its instance. Other tests' memory would show in
    // this process's, so this one is for reading by hand, not for a bound.
    let (chains, length) = (5_025, 199);
    let doc = deep_master(chains, length);
    let before = resident_kb();
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    read_the_deepest(&doc, &resolver, chains, length);
    let after = resident_kb();
    let stats = resolver.stats();
    eprintln!(
        "first member read of 2 x {} shapes: {:?}; resident {:?} kB before and {:?} kB after; {} numbers held by the path maps",
        1 + u64::from(chains) * u64::from(length),
        start.elapsed(),
        before,
        after,
        stats.path_slots
    );
}

#[test]
fn a_master_of_many_deleted_shapes_is_scanned_once_for_its_one_live_shape() {
    // A master whose first 199,999 top-level shapes are deleted and whose last is live: finding the one live shape is a scan, which an
    // instance's every read repeated.
    let n = 200_000_u32;
    let mut shapes: Vec<Arc<Shape>> = (0..n - 1)
        .map(|c| {
            let mut s = Shape::new(c + 1);
            s.del = true;
            Arc::new(s)
        })
        .collect();
    let mut live = Shape::new(n);
    with_cell(&mut live, "PinX", Cell::constant(Value::from_text("3")));
    shapes.push(Arc::new(live));
    let mut doc = DocBuilder::new().build();
    doc.masters = vec![Arc::new(Master { id: 1, shapes: shapes.into_iter().collect(), ..Master::default() })].into_iter().collect();
    let mut page = Page::new(1, "p");
    page.shapes = (0..5_000_u32)
        .map(|i| {
            let mut s = Shape::new(10 + i);
            s.master = Some(1);
            Arc::new(s)
        })
        .collect();
    doc.pages = vec![Arc::new(page)].into_iter().collect();
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    for i in 0..5_000_u32 {
        in_time(start, "instances of a master of deleted shapes", u64::from(i));
        let r = resolver.resolve(&page_shape(&[i]), &CellAddr::direct("PinX")).unwrap();
        assert!(matches!(r.formula.source, diacraft_doc::Source::Master(_)));
    }
    eprintln!("5,000 instances of a master of 200,000 shapes, all but one deleted: {:?} in all", start.elapsed());
    assert_eq!(resolver.stats().single_scans, 1, "{:?}", resolver.stats());
}

/// The 17 cells of a `Character` row, which are text-family cells: each read of one walks the text style chain.
const CHARACTER_CELLS: [&str; 17] = [
    "AsianFont",
    "Case",
    "Color",
    "ComplexScriptFont",
    "ComplexScriptSize",
    "DoubleStrikethrough",
    "DblUnderline",
    "Font",
    "LangID",
    "Overline",
    "Pos",
    "FontScale",
    "Size",
    "Letterspace",
    "Strikethru",
    "Style",
    "ColorTrans",
];

// ---- finding 2: the flag of every style on a chain ------------------------------------------------------------------------------

/// 63 styles linked in each family, the root (which holds the flags) the 64th step, and a shape linked to the first, in a document where the
/// cell asked for is held nowhere, so that every read walks the whole chain.
fn long_chain() -> Document {
    let mut b = DocBuilder::new();
    for id in 1..=63_u32 {
        let next = if id == 63 { 0 } else { id + 1 };
        b = b.style(id, StyleBuilder::new().links(Some(next), Some(next), Some(next)));
    }
    b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), Some(1), Some(1)))).build()
}

#[test]
fn two_hundred_and_fifty_thousand_reads_along_a_chain_at_the_cap_end() {
    let doc = long_chain();
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    for read in 0..250_000_u64 {
        in_time(start, "reads along a long chain", read);
        let r = resolver.resolve(&page_shape(&[0]), &CellAddr::direct("LineWeight")).unwrap();
        assert!(r.warnings.is_empty(), "63 styles and the root are 64 steps");
    }
    eprintln!("250,000 reads along a chain of 64: {:?} in all, {:?} a read", start.elapsed(), start.elapsed() / 250_000);
    // The flags are read once for the chain, not once for each style on it, and not once for each read.
    assert!(resolver.stats().flag_walks <= 64, "{:?}", resolver.stats());
}

/// The text chain of [`long_chain`] with a `Character` section of `rows` rows of the 17 cells held **in the last style only** (style 63),
/// so that every read of a cell walks the whole chain and each style on it must be asked whether it takes part. Every other style holds
/// `extra` sections of its own with names no cell is in (what a hostile file can add: 1,023 a sheet).
fn rows_in_the_last_style(rows: u32, extra: u32) -> Document {
    let mut doc = long_chain();
    let mut section = Section::new("Character", None);
    for i in 0..rows {
        let mut row = Row { ix: Some(i), ..Row::default() };
        for name in CHARACTER_CELLS {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_text("1")));
        }
        section.rows.push(Arc::new(row));
    }
    let section = Arc::new(section);
    for at in 0..63_usize {
        let style = Arc::make_mut(doc.styles.get_mut(at + 1).unwrap());
        let sheet = Arc::make_mut(&mut style.sheet);
        for i in 0..extra {
            sheet.sections.push(Arc::new(Section::new(&format!("Extra{at}x{i}"), None)));
        }
        if at == 62 {
            sheet.sections.push(Arc::clone(&section));
        }
    }
    doc
}

#[test]
fn the_rows_of_a_big_section_held_in_the_last_style_of_a_long_chain_end() {
    // 16,384 rows of 17 cells under the text style chain: the review measured about ten seconds for each such shape. The cells are in the
    // last style, not on the shape, so each read walks the chain and asks each style for its flag: with the flags asked again for each
    // read this took 18 s, and 117 ms when the cells were held on the shape (where no read walked anything).
    let doc = rows_in_the_last_style(16_384, 0);
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    let rows = resolver.effective_rows(&page_shape(&[0]), &SectionKey::new("Character", None)).unwrap();
    assert_eq!(rows.len(), 16_384);
    assert!(rows.iter().all(|r| r.cells.len() == 17));
    assert!(rows.iter().all(|r| matches!(r.cells[0].1.formula.source, diacraft_doc::Source::Style { id: 63, .. })), "from the last style");
    in_time(start, "effective rows", 0);
    eprintln!("effective rows of 16,384 x 17 cells in the last style of a chain of 64: {:?}", start.elapsed());
    assert!(resolver.stats().flag_walks <= 64, "{:?}", resolver.stats());
}

#[test]
fn sections_that_no_cell_is_in_do_not_make_a_read_through_the_chain_cost_more() {
    // The review: a row read through 63 styles with 1,023 sections each took 53 µs against 2.1 µs without them, since each step scanned
    // the sections. Each sheet with many sections is indexed once; a read finds its section by name in the index.
    let reads = 20_000_u32;
    // Counted, not timed: the sections a scan looked at (`diacraft_doc::work`) and the indexes built.
    let cost = |extra: u32| {
        let doc = rows_in_the_last_style(64, extra);
        let resolver = Resolver::new(&doc);
        let addr = CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(5), "Size");
        let start = Instant::now();
        diacraft_doc::work::reset();
        for read in 0..reads {
            in_time(start, "reads past sections", u64::from(read));
            assert!(resolver.resolve(&page_shape(&[0]), &addr).is_some());
        }
        (diacraft_doc::work::sections_examined(), resolver.stats())
    };
    let (plain, _) = cost(0);
    let (heavy, stats) = cost(1_023);
    eprintln!("{reads} reads through 63 styles: {plain} sections looked at with no extra sections, {heavy} with 1,023 in each style");
    // One index for each sheet that has many sections: the 62 styles that hold them (the 63rd holds one, and the shape's own sheet none).
    // Not one for each read.
    assert!((62..=64).contains(&stats.section_indexes), "{stats:?}");
    // The sections that do not make a read cost more are not looked at: the reads past 1,023 sections in each style look at no more than
    // the reads past none (a scan of them all for each step would be 63 times 1,023 for each of the 20,000 reads).
    assert!(heavy <= plain + u64::from(reads), "{heavy} sections looked at against {plain} with none");
}

#[test]
fn the_rows_under_a_long_chain_of_styles_that_each_hold_many_sections_end() {
    // The review: effective rows of 16,384 x 17 cells from the last style took 15.5 s with 1,023 sections in each style.
    let doc = rows_in_the_last_style(16_384, 1_023);
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    let rows = resolver.effective_rows(&page_shape(&[0]), &SectionKey::new("Character", None)).unwrap();
    assert_eq!(rows.len(), 16_384);
    in_time(start, "effective rows past sections", 0);
    eprintln!("effective rows of 16,384 x 17 cells in the last style, 1,023 sections in each style: {:?}", start.elapsed());
    assert!(resolver.stats().section_indexes <= 64, "{:?}", resolver.stats());
}

#[test]
fn the_section_names_of_a_shape_whose_chains_hold_a_thousand_sections_each_are_listed_in_time() {
    // The review: 189 styles (63 in each of the three families) with 1,023 sections each, every name its own: 46.7 s for one shape, since
    // each name was looked for by a scan of the names so far and then of every section of every style.
    let mut b = DocBuilder::new();
    for family in 0..3_u32 {
        for step in 1..=63_u32 {
            let id = family * 100 + step;
            let next = if step == 63 { 0 } else { id + 1 };
            let links = match family {
                0 => (Some(next), None, None),
                1 => (None, Some(next), None),
                _ => (None, None, Some(next)),
            };
            let mut style = StyleBuilder::new().links(links.0, links.1, links.2);
            for i in 0..1_023_u32 {
                style = style.del_section(&format!("S{id}x{i}"), None);
            }
            b = b.style(id, style);
        }
    }
    let doc = b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), Some(101), Some(201)))).build();
    let resolver = Resolver::new(&doc);
    let start = Instant::now();
    let names = resolver.effective_section_names(&page_shape(&[0])).unwrap();
    in_time(start, "section names", 0);
    eprintln!("the section names of a shape under 189 styles of 1,023 sections each: {:?} for {} names", start.elapsed(), names.len());
    assert!(names.is_empty(), "every one of those sections is deleted, and of a name no class lists: nothing is effective");
}

// ---- findings 3 and 4: plain edits and their copies -----------------------------------------------------------------------------

fn connect(i: u32) -> Connect {
    Connect { from_sheet: i, to_sheet: i + 1, from_part: Some(9), to_part: Some(100), ..Connect::default() }
}

/// What one cell write, with the version before it kept (as the engine's history does), left unshared with that version: the page, the chunks
/// of its shapes and the shapes copied, and each list the document holds that was copied whole. Counted, not timed: a count is the same on
/// any machine at any load.
fn copies_made_by_a_write(doc: &mut Document, i: u32) -> usize {
    let kept = doc.clone();
    let edit = CellEdit::Value(Value::from_text(&i.to_string()));
    doc.write_cell(&page_shape(&[i % 40]), &CellAddr::direct("Width"), edit, WriteOrigin::Automation).unwrap();
    let (now, then) = (doc.pages.get(0).unwrap(), kept.pages.get(0).unwrap());
    let shapes_copied = (0..now.shapes.len()).filter(|k| !Arc::ptr_eq(now.shapes.get(*k).unwrap(), then.shapes.get(*k).unwrap())).count();
    usize::from(!Arc::ptr_eq(now, then))
        + (now.shapes.chunk_count() - now.shapes.shared_chunks_with(&then.shapes))
        + shapes_copied
        + usize::from(!now.connects.shares_with(&then.connects))
        + usize::from(!doc.styles.is_same_list_as(&kept.styles))
        + usize::from(!doc.masters.is_same_list_as(&kept.masters))
}

fn small_document() -> Document {
    let mut doc = DocBuilder::new().build();
    let mut page = Page::new(1, "p");
    page.shapes = (0..40_u32).map(|i| Arc::new(Shape::new(i + 4))).collect();
    doc.pages = vec![Arc::new(page)].into_iter().collect();
    doc
}

#[test]
fn ten_thousand_connects_do_not_make_an_edit_copy_them() {
    // The review: a write on a page with 10,000 connects went from 2.2 µs to 180 µs and 625 KB a step.
    let mut plain = small_document();
    let mut heavy = small_document();
    let page = Arc::make_mut(heavy.pages.get_mut(0).unwrap());
    set_connects(page, 10_000);
    // Counted, not timed: the page, one chunk of its shapes and the shape are copied, and the 10,000 connects are not (a list copied whole
    // would be one more, and 625 KB).
    let (plain_copies, heavy_copies) = (copies_made_by_a_write(&mut plain, 7), copies_made_by_a_write(&mut heavy, 7));
    assert_eq!(plain_copies, 3, "the page, one chunk of its shapes, the shape");
    assert_eq!(heavy_copies, plain_copies, "a write copied the connects");
}

fn set_connects(page: &mut Page, n: u32) {
    page.connects = (0..n).map(connect).collect();
}

#[test]
fn sixty_five_thousand_styles_do_not_make_an_edit_copy_them() {
    // The review: 1,000,000 styles, 47 KB deflated, and a clone of 19 ms and 7.8 MB for each undo step. At the cap, 65,536.
    let mut plain = small_document();
    let mut heavy = small_document();
    fill_styles(&mut heavy, 65_536);
    // Counted, not timed: the 65,536 styles are one list, shared with the version kept, and the count of copies is the plain document's.
    let (plain_copies, heavy_copies) = (copies_made_by_a_write(&mut plain, 7), copies_made_by_a_write(&mut heavy, 7));
    assert_eq!(plain_copies, 3, "the page, one chunk of its shapes, the shape");
    assert_eq!(heavy_copies, plain_copies, "a write copied the styles");
}

fn fill_styles(doc: &mut Document, n: u32) {
    doc.styles = (1..=n).map(|id| Arc::new(Style { id, ..Style::default() })).collect();
}

#[test]
fn a_style_chain_of_exactly_the_cap_has_no_warning_and_one_more_has() {
    // At most 64 steps. The 64th style is read; a 65th is not, and says so.
    let chain = |styles: u32| {
        let mut b = DocBuilder::new();
        for id in 1..=styles {
            let next = if id == styles { None } else { Some(id + 1) };
            b = b.style(id, StyleBuilder::new().links(next, None, None));
        }
        b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build()
    };
    for (styles, warns) in [(64, false), (65, true)] {
        let doc = chain(styles);
        let r = Resolver::new(&doc).resolve(&page_shape(&[0]), &CellAddr::direct("LineWeight")).unwrap();
        assert_eq!(!r.warnings.is_empty(), warns, "{styles} styles");
    }
}

// ---- a memo must not depend on who asked first ------------------------------------------------------------------------------------

/// 100 styles in a line chain (style `n` is based on `n + 1`, the last on the root), the last holding `EnableLineProps` 0 and the first and the fortieth each a
/// `LineWeight` of their own; shape 0 is linked to style 1 and shape 1 to style 40.
fn chain_of_a_hundred() -> Document {
    let mut b = DocBuilder::new().root(|root| root.cell("LineWeight", "0.01"));
    for id in 1..=100_u32 {
        let next = if id == 100 { 0 } else { id + 1 };
        let mut style = StyleBuilder::new().links(Some(next), None, None);
        if id == 100 {
            style = style.cell("EnableLineProps", "0");
        }
        if id == 1 || id == 40 {
            style = style.cell("LineWeight", &id.to_string());
        }
        b = b.style(id, style);
    }
    b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None)).shape(ShapeBuilder::new(11).links(Some(40), None, None)))
        .build()
}

#[test]
fn what_a_style_chain_past_the_cap_reads_does_not_depend_on_which_shape_was_asked_first() {
    // The review: on a chain of 100 a flag walk ran out of steps at the 64th style and kept "no flag" for every style it had passed, among them
    // the 40th, whose own 64 steps reach the flag on the 100th. So the 40th read as taking part or not by who asked before. A walk that runs
    // out keeps its answer for its first style only, and a style takes the answer of one further down only if that one's flag is within
    // its own reach.
    let doc = chain_of_a_hundred();
    let weight = CellAddr::direct("LineWeight");
    let read = |resolver: &Resolver<'_>, shape: u32| resolver.resolve(&page_shape(&[shape]), &weight).unwrap().formula.source;
    let fresh = |shape: u32| read(&Resolver::new(&doc), shape);
    // Style 1: its 64 steps stop at style 64, short of the flag, so it takes part and gives its own `LineWeight`. Style 40: its steps reach the
    // flag on the 100th, which switches the line properties off for it and for every style from it on: the weight is the root's.
    let (first, second) = (fresh(0), fresh(1));
    assert!(matches!(first, Source::Style { id: 1, .. }), "{first:?}");
    assert!(matches!(second, Source::Style { id: 0, .. }), "{second:?}");
    for order in [[0, 1], [1, 0]] {
        let resolver = Resolver::new(&doc);
        for shape in order {
            let got = read(&resolver, shape);
            assert_eq!(got, if shape == 0 { first.clone() } else { second.clone() }, "shape {shape} asked in the order {order:?}");
        }
    }
    // And a style asked about directly says the same whatever was read before it.
    let resolver = Resolver::new(&doc);
    assert!(matches!(read(&resolver, 0), Source::Style { id: 1, .. }));
    assert!(matches!(read(&resolver, 1), Source::Style { id: 0, .. }));
}

mod memo_is_order_independent {
    use proptest::prelude::*;

    use super::*;

    /// A line chain of `flags.len()` styles (style `n` on `n + 1`, the last on the root), the flag of each style one of absent, 0 or 1 and a
    /// `LineWeight` on some; one shape linked to each style.
    fn chain(flags: &[(u8, bool)]) -> Document {
        let count = u32::try_from(flags.len()).unwrap_or(1);
        let mut b = DocBuilder::new().root(|root| root.cell("LineWeight", "0.01"));
        for (at, (flag, weight)) in flags.iter().enumerate() {
            let id = u32::try_from(at).unwrap_or(0) + 1;
            let mut style = StyleBuilder::new().links(Some(if id == count { 0 } else { id + 1 }), None, None);
            match flag % 3 {
                0 => {}
                1 => style = style.cell("EnableLineProps", "0"),
                _ => style = style.cell("EnableLineProps", "1"),
            }
            if *weight {
                style = style.cell("LineWeight", &id.to_string());
            }
            b = b.style(id, style);
        }
        let mut page = PageBuilder::new();
        for id in 1..=count {
            page = page.shape(ShapeBuilder::new(id + 10).links(Some(id), None, None));
        }
        b.page(1, page).build()
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 120, ..ProptestConfig::default() })]

        /// What a shape reads through its style chain is the same whether the resolver is new for each read or has answered others first, in
        /// any order, on chains from 1 to 130 styles (the cap is 64 steps, so the longer ones are the ones a walk runs out on).
        #[test]
        fn a_read_is_the_same_whoever_asked_before(
            flags in proptest::collection::vec((0u8..3, any::<bool>()), 1..130),
            order in proptest::collection::vec(any::<usize>(), 1..40),
        ) {
            let doc = chain(&flags);
            let shared = Resolver::new(&doc);
            let weight = CellAddr::direct("LineWeight");
            for pick in order {
                let shape = u32::try_from(pick % flags.len()).unwrap_or(0);
                let alone = Resolver::new(&doc).resolve(&page_shape(&[shape]), &weight).map(|r| (r.formula.source, r.warnings));
                let after_others = shared.resolve(&page_shape(&[shape]), &weight).map(|r| (r.formula.source, r.warnings));
                prop_assert_eq!(after_others, alone, "shape {} of {} styles", shape, flags.len());
            }
        }
    }
}

#[test]
fn a_flag_on_the_64th_style_from_a_style_switches_it_off_and_one_on_the_65th_does_not() {
    // A flag is looked for along the style's own chain over 64 styles, the style itself the first. On a chain of 70, style 1 reaches
    // style 64 and not 65: a flag on the 64th switches the line properties off for it (and for every style up to that one, so the shape
    // reads nothing but the default, the walk of the cell having run out of its 64 steps too), and a flag on the 65th is out of reach.
    let chain = |flag_at: u32| {
        let mut b = DocBuilder::new().root(|root| root.cell("LineWeight", "0.01"));
        for id in 1..=70_u32 {
            let mut style = StyleBuilder::new().links(Some(if id == 70 { 0 } else { id + 1 }), None, None);
            if id == flag_at {
                style = style.cell("EnableLineProps", "0");
            }
            if id == 1 {
                style = style.cell("LineWeight", "1");
            }
            b = b.style(id, style);
        }
        b.page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build()
    };
    let read = |flag_at: u32| {
        let doc = chain(flag_at);
        let r = Resolver::new(&doc).resolve(&page_shape(&[0]), &CellAddr::direct("LineWeight")).unwrap();
        r.formula.source
    };
    assert_eq!(read(64), Source::Default, "the flag on the 64th style is within reach of style 1");
    assert!(matches!(read(65), Source::Style { id: 1, .. }), "the flag on the 65th is not, so style 1 takes part and holds the weight");
}
