//! **No writer of the engine forgets the unknown nodes among the cells, the rows, the sections and the shapes it changes** (a ratchet).
//!
//! An unknown node (a foreign element, a comment) is kept as the count of typed children that stood before it, and a model writer puts it back after that many. A writer that puts a cell, a row, a
//! section or a shape in a list, or takes one out, by the list's own methods (`section.cells.insert(..)`, `section.rows.push(..)`, `sheet.sections.retain(..)`, `page.shapes.remove(..)`) leaves the count
//! as it was and the node one place off. **The writers of the model move the counts themselves**: `Sheet::section_mut`, `Sheet::row_mut` and `Sheet::set_cell`, `Row::put_cell` and
//! `Section::put_cell`, `Row::replace_cells`, `Section::insert_row`, `append_row`, `remove_rows`, `retain_rows`, `sort_rows_by_key` and `replace_rows`, `Sheet::insert_section` and
//! `remove_sections`, `Counted` (the same, with the nodes of the shape or the style that holds the sheet) and `Page::push_shape`, `insert_shape`, `remove_shape` and `edit_shapes` with their kin for
//! a master and a group; a list that is **made again** from another (a shape changed to another master, a copy a Save As makes) takes the nodes of the old one with `Sheet::take_places_of`,
//! `Section::take_places_of` and `owner_places_after`.
//!
//! These tests count, file by file, the places where production code changes one of those lists by the list's own methods, and **hold each count to the list below**.
//! Every place listed makes a sheet or a list **nobody holds a node in** (a shape or a master a command makes, a stencil's, a fixture of a cost test), reads a file (the `.odg` reader builds a model, and
//! puts the nodes of the file on it after the cells), is another list of the name, or keeps the count right by hand (and says so). A new place is a new count: use the writers above, or, when the list is a
//! new one, add the count here and say why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const LISTS: [&str; 4] = ["sections", "rows", "shapes", "children"];
const OPS: [&str; 17] = [
    "push",
    "insert",
    "remove",
    "retain",
    "truncate",
    "clear",
    "extend",
    "drain",
    "sort",
    "sort_by",
    "sort_by_key",
    "dedup",
    "swap",
    "pop",
    "splice",
    "append",
    "swap_remove",
];

/// The `.rs` files below `dir`.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The writes of lists one line holds: a call of a method that changes `.rows`, `.sections`, `.shapes` or `.children`, and an assignment to one.
fn writes_in(line: &str) -> usize {
    let line = line.trim_start();
    if line.starts_with("//") {
        return 0;
    }
    let mut n = 0;
    for list in LISTS {
        for op in OPS {
            n += line.matches(&format!(".{list}.{op}(")).count();
        }
        n += line.matches(&format!(".{list} = ")).count();
    }
    n
}

/// The production lines of a file: those before its test module (a module of tests with its body in the file: `mod tests {`; a `#[cfg(test)] mod name;` that names a file of tests
/// ends nothing).
fn production(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines
        .iter()
        .enumerate()
        .position(|(at, line)| {
            line.trim() == "#[cfg(test)]"
                && lines.get(at + 1).is_some_and(|next| next.trim_start().starts_with("mod ") && next.trim_end().ends_with('{'))
        })
        .unwrap_or(lines.len());
    lines.into_iter().take(end).collect()
}

/// The count of writes of each file of the engine's `src` that holds any, by its path below `src`.
fn counts() -> BTreeMap<String, usize> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    let mut found = BTreeMap::new();
    for path in files {
        // The files of the tests: a `tests.rs`, a `..._tests.rs` and anything in a `tests` folder of `src`.
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let below = path.strip_prefix(&src).unwrap_or(&path);
        if name == "tests.rs" || name.ends_with("_tests.rs") || below.components().any(|c| c.as_os_str() == "tests") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let n: usize = production(&text).into_iter().map(writes_in).sum();
        if n > 0 {
            found.insert(path.strip_prefix(&src).unwrap().to_str().unwrap().to_owned(), n);
        }
    }
    found
}

/// The places held, with the count of each. **A file that holds more than its count has a new writer; a file that holds fewer has lost one, and its count comes down.**
const HELD: &[(&str, usize)] = &[
    // The lists a Save As across the formats rebuilds (it puts back what the other format left out, and strips what it cannot show), **one shape for one**: each shape of a list is given back as
    // itself or as the shape it was, in the same place, so no shape comes or goes and the counts of the nodes among them stay right (`across::dropped`'s test holds it).
    ("across/dropped.rs", 2),
    ("across/hold.rs", 4),
    // (`onto.rs` gives the shapes whose ends hold plain places the glue formulas, one shape for one too, and the connectors of DiaCraft's that lack them the cells the
    // VSDX application routes them by, and the shapes of an `.odg` no style link that is their master's: a page whose list of shapes is put whole for each, a
    // shape for a shape.)
    ("across/onto.rs", 3),
    // (`text_rows.rs` gives the text rows past the first of a shape the cells they lack, one shape for one as well: a page's and a master's list of shapes, and a group's members,
    // put whole, a shape for a shape; the cells go in by `Row::put_cell`.)
    ("across/text_rows.rs", 3),
    // A sheet, a section or a list of shapes that is **new**: a shape, a connector, a guide, a path, a master or a style a command makes, the sections a stencil dresses an instance with, the
    // geometry a shape operation makes for its result. Nobody holds a node in it.
    ("draw/change.rs", 1),
    ("draw/clipboard.rs", 1),
    ("draw/connectors.rs", 3),
    ("draw/pen/edit.rs", 1),
    ("draw/shapeops/apply.rs", 2),
    ("draw/shapeops/carry.rs", 1),
    ("draw/shapeops/look.rs", 3),
    ("draw/shapes.rs", 1),
    ("snap/guides.rs", 2),
    ("stencil.rs", 9),
    ("stencil/bring.rs", 2),
    ("stencil/callout.rs", 9),
    ("stencil/container.rs", 13),
    ("stencil/drawn.rs", 2),
    ("stencil/dress.rs", 16),
    ("stencil/group_master.rs", 3),
    ("stencil/instance.rs", 4),
    ("stencil/orgchart.rs", 9),
    ("stencil/picture.rs", 1),
    ("stencil/text_box.rs", 2),
    ("theme/style.rs", 4),
    // The merge of the rows a copy into an instance puts in (`put_in_order`), which moves the nodes by hand in one pass.
    ("draw/sheet_copy.rs", 1),
    // The copy an `.odg` save is given (`odgfile/flatten.rs`): the sections of a sheet it builds again, whose nodes are moved with the cells and the sections they followed once the sheet is
    // complete (`Sheet::take_places_of`, `Section::take_places_of` and `owner_places_after`), and the members of a group, one for one.
    ("odgfile/flatten.rs", 13),
    ("odgfile/hold.rs", 2),
    // The fixtures of cost tests, declared under `cfg(test)`.
    ("draw/relayout/glue_cost.rs", 3),
    ("draw/relayout/hostile.rs", 1),
    ("draw/relayout/org_hostile.rs", 5),
    ("draw/route_cost.rs", 7),
    ("odgfile/preview_cost.rs", 1),
    // A field of another name that the count takes for a list (`layer.shapes`, `Remap::rows`, a builder's `rows`, `Spent::Rows`, the budget of a scene).
    ("draw/conn_points/edges.rs", 2),
    ("draw/conn_refs.rs", 1),
    ("draw/find/replace.rs", 1),
    ("draw/layers.rs", 1),
    ("draw/route_scene/put.rs", 8),
    ("draw/shapeops/operand.rs", 4),
    ("draw/sheet_rows.rs", 4),
    ("formatting/mod.rs", 4),
    ("formatting/runs.rs", 1),
    ("sheetview/outline.rs", 2),
];

/// Holds `found` to `held`: the two are the same, or the test fails saying which files differ and what the counts now are.
fn hold_to(found: BTreeMap<String, usize>, held: &[(&str, usize)], advice: &str) {
    let held: BTreeMap<String, usize> = held.iter().map(|(file, n)| ((*file).to_owned(), *n)).collect();
    if found == held {
        return;
    }
    let mut report = Vec::new();
    for (file, n) in &found {
        match held.get(file) {
            None => report.push(format!("{file}: {n} writes, and the file is not on the list")),
            Some(held) if held != n => report.push(format!("{file}: {n} writes, and the list holds {held}")),
            Some(_) => {}
        }
    }
    for file in held.keys().filter(|file| !found.contains_key(*file)) {
        report.push(format!("{file}: no write any more, and the file is on the list"));
    }
    let table: Vec<String> = found.iter().map(|(file, n)| format!("    (\"{file}\", {n}),")).collect();
    panic!("{advice}:\n{}\n\nthe counts now:\n{}", report.join("\n"), table.join("\n"));
}

#[test]
fn no_writer_of_the_engine_changes_a_list_of_rows_sections_or_shapes_by_its_own_methods_but_where_this_list_says() {
    hold_to(
        counts(),
        HELD,
        "a writer changes a list of rows, sections or shapes by the list's own methods, and the unknown nodes among them are left where they stood: use `Section::insert_row`, `remove_rows`, \
         `retain_rows`, `replace_rows`, `Sheet::insert_section`, `remove_sections`, `Counted` or `Page::push_shape` and its kin, or put the count on the list in `places_guard.rs` with the reason",
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------- the cells

/// The methods of a list of cells that put a cell in it or take one out. (A cell put under a name the list holds replaces the cell and moves no node, which a line of text cannot tell: such a place is
/// listed, with that reason.)
const CELL_OPS: [&str; 11] =
    ["insert", "remove", "entry", "retain", "clear", "extend", "append", "pop_first", "pop_last", "split_off", "remove_entry"];

/// The lines of production code with each chain that rustfmt broke (a line that begins with `.`) joined to the line before it, and the comments left out.
fn logical_lines(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.starts_with("//") || line.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if line.starts_with('.') => last.push_str(line),
            _ => out.push(line.to_owned()),
        }
    }
    out
}

/// The writes of lists of cells one logical line holds: a call of a method that changes a list called `cells` (a field, `section.cells`, or a local that holds one) and an assignment to one.
/// A name that only ends in `cells` (`new_cells`, `own_cells`) is another list.
fn cell_writes_in(line: &str) -> usize {
    let mut n = 0;
    for op in CELL_OPS {
        let needle = format!("cells.{op}(");
        n += line.match_indices(&needle).filter(|(at, _)| !line[..*at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_')).count();
    }
    n + line.matches(".cells = ").count()
}

/// The count of writes of lists of cells of each production file of every crate's `src`, by its path below `crates/` (`engine/src/draw/rows.rs`).
fn cell_counts() -> BTreeMap<String, usize> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the engine is in the crates folder").to_path_buf();
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let src = entry.unwrap().path().join("src");
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        sources(&src, &mut files);
        for path in files {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let below = path.strip_prefix(&src).unwrap_or(&path);
            if name == "tests.rs" || name.ends_with("_tests.rs") || below.components().any(|c| c.as_os_str() == "tests") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let n: usize = logical_lines(&production(&text)).iter().map(|line| cell_writes_in(line)).sum();
            if n > 0 {
                found.insert(path.strip_prefix(&crates).unwrap().to_str().unwrap().to_owned(), n);
            }
        }
    }
    found
}

/// The places held, by their path below `crates/`, with the count of each. **A file that holds more than its count has a new writer; a file that holds fewer has lost one, and its count comes down.**
const HELD_CELLS: &[(&str, usize)] = &[
    // The writers of the model themselves: `put` and `take` of `Sheet::set_cell` and `remove_cell` (and so of `Row::put_cell`, `Section::put_cell` and `Counted`) move the nodes with the cell, and
    // `Row::replace_cells` does with `cells_changed`.
    ("doc/src/places.rs", 1),
    ("doc/src/sheet.rs", 4),
    // A sheet, a section or a row that is **new**: the cells of a blank document, a stencil's master, the instance and the members a drop makes, the shape or the connector a command makes, a
    // style, a guide, the geometry a shape operation makes for its result, a route's rows, a row of `alt_text`. Nobody holds a node in it.
    ("doc/src/alt_text.rs", 2),
    ("doc/src/document.rs", 2),
    ("engine/src/draw/connectors.rs", 1),
    ("engine/src/draw/container/swimlane.rs", 1),
    ("engine/src/draw/drop_scale.rs", 2),
    ("engine/src/draw/group.rs", 6),
    ("engine/src/draw/rows.rs", 1),
    ("engine/src/draw/conn_points/edit.rs", 1),
    ("engine/src/draw/conn_points/local.rs", 3),
    ("engine/src/draw/shapeops/apply.rs", 3),
    ("engine/src/draw/shapeops/carry.rs", 2),
    ("engine/src/draw/shapeops/rows.rs", 1),
    ("engine/src/draw/shapes.rs", 1),
    ("engine/src/draw/sheet_rows.rs", 2),
    ("engine/src/snap/guides.rs", 1),
    ("engine/src/stencil.rs", 1),
    ("engine/src/stencil/container.rs", 1),
    ("engine/src/stencil/dress.rs", 5),
    ("engine/src/stencil/instance.rs", 6),
    ("engine/src/stencil/orgchart.rs", 4),
    ("engine/src/stencil/swimlane.rs", 3),
    ("engine/src/theme/style.rs", 3),
    // The cells of a row a command **plans** in a map of its own (a copy of the row's, or a new row's), which `Row::replace_cells` or a new row writes: the model's writers move the nodes.
    ("engine/src/draw/change.rs", 2),
    ("engine/src/formatting/effects.rs", 3),
    ("engine/src/formatting/quick.rs", 1),
    ("engine/src/formatting/text.rs", 2),
    // The cells a row past the first is given (`root_text::complete_row_cells`) in the map a command plans for a row it writes whole, or a stencil makes new; a row of the model is
    // given them by `Row::put_cell` (`complete_row_with`).
    ("doc/src/root_text.rs", 1),
    // The merge of the rows a copy into an instance puts in, which keeps the count right by hand (`cell_names` and `cells_changed`, `put_in_order`).
    ("engine/src/draw/sheet_copy.rs", 4),
    // A cell **put under the name it holds** replaces it and makes none: no node moves (the colours of a stencil's style a bring maps to the drawing's palette).
    ("engine/src/stencil/bring.rs", 3),
    // The copy an `.odg` save is given (`flatten.rs`): a sheet, a section or a row it makes new, whose nodes are put on it when it is complete (`Sheet::take_places_of`, `Section::take_places_of` and
    // `owner_places_after`), the merge of a master's row, which keeps the count right by hand, and the colour cells a copy of a shape the file holds puts over their own.
    ("engine/src/odgfile/flatten.rs", 22),
    // Another list of the name: the cells to work out again (`Vec<(SheetRef, CellAddr)>`), the numbers a command asks for, a count of cells, a list of a view, a plan of a page's cells.
    ("calc/src/changes.rs", 1),
    ("doc/src/resolve.rs", 1),
    ("engine/src/cmd/relayout.rs", 3),
    ("engine/src/draw/clipboard.rs", 3),
    ("engine/src/draw/conn_refs.rs", 1),
    ("engine/src/draw/page_fit.rs", 2),
    ("engine/src/draw/page_setup.rs", 1),
    ("engine/src/draw/refit.rs", 5),
    ("engine/src/draw/relayout/mod.rs", 1),
    ("engine/src/draw/route_scene/put.rs", 1),
    ("engine/src/draw/shapeops/snap.rs", 1),
    ("engine/src/draw/sheet.rs", 1),
    ("engine/src/sheetview/cell.rs", 1),
    ("engine/src/sheetview/lines.rs", 3),
    ("ui-egui/src/sheet/view.rs", 2),
    ("vsdx/src/read.rs", 2),
    ("odg/src/figures.rs", 1),
    ("odg/src/read/mod.rs", 2),
    // The fixtures of cost tests, declared under `cfg(test)`.
    ("engine/src/draw/relayout/glue_cost.rs", 1),
    ("engine/src/draw/relayout/hostile.rs", 1),
    ("engine/src/draw/relayout/org_hostile.rs", 3),
    ("engine/src/draw/loops_cost.rs", 1),
    ("engine/src/draw/route_cost.rs", 4),
    // Test support: a fixture of the tests.
    ("testkit/src/fixtures.rs", 1),
    ("testkit/src/model.rs", 1),
    // A file being read: a reader makes a sheet from the file's elements, and **the nodes of the file are put on it as it is made** (the count of the children seen so far), or, for an `.odg`, after
    // the cells (`shell::apply`), so no node is held while a cell goes in.
    ("sheetxml/src/read.rs", 2),
    ("odg/src/read/connectors.rs", 1),
    ("odg/src/read/keep.rs", 1),
    ("odg/src/read/custom.rs", 3),
    ("odg/src/read/layers.rs", 1),
    ("odg/src/read/outputs.rs", 1),
    ("odg/src/read/pages.rs", 1),
    ("odg/src/read/shapes.rs", 9),
    ("odg/src/ext/payload.rs", 3),
    // What an `.odg` writer compares: the sheet the reader would rebuild from the markup, and the data a shape carries beside it (`Tier2`, which holds cells and sections and no node: the nodes ride in
    // the shell). None of them is the model's, and none is given back to it.
    ("odg/src/ext/docfile.rs", 5),
    ("odg/src/ext/native.rs", 13),
    ("odg/src/ext/tier3.rs", 8),
    ("odg/src/write/compare.rs", 1),
    ("odg/src/write/own.rs", 4),
    ("odg/src/write/plan.rs", 2),
    ("odg/src/write/runs/model.rs", 2),
    ("odg/src/write/styles.rs", 5),
    // Tests of the `.odg` writer, declared under `cfg(test)`.
    ("odg/src/write/own/section_agrees.rs", 15),
    ("odg/src/write/own/stamp_agrees.rs", 1),
    ("odg/src/write/runs/held_paragraphs.rs", 1),
    ("odg/src/write/runs/row_key_agrees.rs", 2),
    ("odg/src/write/runs/text_stamp_agrees.rs", 1),
];

#[test]
fn no_writer_puts_a_cell_in_a_list_of_cells_or_takes_one_out_by_the_list_s_own_methods_but_where_this_list_says() {
    hold_to(
        cell_counts(),
        HELD_CELLS,
        "a writer puts a cell in a list of cells, or takes one out, by the list's own methods, and the unknown nodes among the cells are left where they stood: use `Sheet::set_cell`, `remove_cell`, \
         `Row::put_cell`, `Section::put_cell`, `Row::replace_cells` or `Counted` (`Shape::counted_sheet`, `Document::counted_mut`), or put the count on the list in `places_guard.rs` with the reason",
    );
}
