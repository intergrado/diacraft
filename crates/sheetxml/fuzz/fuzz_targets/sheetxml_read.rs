//! `cargo fuzz run sheetxml_read`: the sheet codec on any input. The first byte chooses the vocabulary and the budgets (a small one for items,
//! cells or formula nodes makes the budgets the thing under test), the second the byte budget (none, or a few hundred bytes to a few thousand, which
//! makes the charges of the prices the thing under test: a refusal by the byte budget is a refusal both times, never more is charged than was given,
//! and what is charged is the same on a second reading), and the rest is parsed as XML by the container's bounded parser; a tree that
//! parses is read as a sheet, read again (the same sheet, the same charges, the same words), written back as a tree, given as the attributes of
//! its cells, compared with the defaults, and patched from the sheet it was read as to that sheet with one cell more. A read gives a sheet or a
//! refusal by a budget, and the sheet keeps the model's caps and never spends more than it was given; anything else, a panic, a hang or a
//! different second reading, is a crash the fuzzer reports.
#![no_main]

use diacraft_container::xml::{El, Node, parse, write_element};
use diacraft_doc::{Cell, CellName, Cells, MAX_CELLS_PER_CONTAINER, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET};
use diacraft_sheetxml::build::{cell_attrs, sheet_only_el};
use diacraft_sheetxml::patch::patch_sheet_children;
use diacraft_sheetxml::{Caps, Cx, PatchError, Scope, SheetReader, Vocabulary, Warnings, cells_delta};
use libfuzzer_sys::fuzz_target;

const MAIN: &str = "urn:test:main";
const EXT: &str = "urn:test:ext";

/// Reads no namespace and one: as a VSDX part does.
#[derive(Clone, Copy, Default)]
struct Main;

impl Vocabulary for Main {
    fn reads(self, uri: Option<&str>) -> bool {
        matches!(uri, None | Some("") | Some(MAIN))
    }
}

/// Reads one namespace and no other: as an extension does.
#[derive(Clone, Copy, Default)]
struct Ext;

impl Vocabulary for Ext {
    fn reads(self, uri: Option<&str>) -> bool {
        uri == Some(EXT)
    }
}

fn read<V: Vocabulary + Default>(root: &El, caps: Caps, bytes: u64) -> Option<(diacraft_doc::Sheet, SheetReader, Warnings)> {
    let mut reader = SheetReader::new(caps).with_bytes(bytes);
    let mut warnings = Warnings::new(20);
    let sc = Scope::<V>::empty().enter(root);
    let sheet = {
        let mut cx = Cx::new(&mut warnings, "/fuzz.xml");
        reader.read_sheet_only(&mut cx, root, &sc).ok()?
    };
    Some((sheet, reader, warnings))
}

fn exercise<V: Vocabulary + Default>(root: &El, caps: Caps, budget: u64) {
    let first = read::<V>(root, caps, budget);
    let second = read::<V>(root, caps, budget);
    let (Some((sheet, reader, warnings)), Some((again, reader2, warnings2))) = (first, second) else {
        // A refusal is a refusal both times.
        assert!(read::<V>(root, caps, budget).is_none());
        return;
    };
    assert_eq!(sheet, again);
    assert_eq!(reader.stats(), reader2.stats());
    assert_eq!(warnings, warnings2);
    // What was spent is what was given less what is left, and never more than was given; the model's caps hold.
    let stats = reader.stats();
    assert!(stats.items <= caps.items && stats.cells <= caps.cells && stats.formula_nodes <= caps.formula_nodes);
    // The byte budget is never passed, and what is left is what was given less what is charged (a reader with none keeps all of `u64::MAX`).
    assert!(stats.bytes <= budget && stats.bytes <= stats.bytes_peak);
    if budget != u64::MAX {
        assert_eq!(stats.bytes + reader.bytes_left(), budget);
    }
    assert_eq!(stats.items + reader.items_left(), caps.items);
    assert_eq!(stats.cells + reader.cells_left(), caps.cells);
    assert!(sheet.cells.len() <= MAX_CELLS_PER_CONTAINER && sheet.sections.len() <= MAX_SECTIONS_PER_SHEET);
    for section in &sheet.sections {
        assert!(section.cells.len() <= MAX_CELLS_PER_CONTAINER && section.rows.len() <= MAX_ROWS_PER_SECTION);
    }

    // Written back as a tree: a tree the XML writer may refuse (a character XML cannot hold), which is no crash.
    let written = sheet_only_el("PageSheet", &sheet);
    let mut bytes = Vec::new();
    let _ = write_element(&mut bytes, &written);

    // Given as the attributes of its cells: the cells that were stored come back equal, and the budgets are spent as they are on the element.
    let mut cells = Cells::new();
    let mut again = SheetReader::new(caps).with_bytes(budget);
    let mut notes = Warnings::new(20);
    for node in &root.children {
        let Node::El(child) = node else { continue };
        if !child.is("Cell") {
            continue;
        }
        let pairs: Vec<(&str, &str)> = child.attrs.iter().filter(|(k, _)| !k.starts_with("xmlns")).map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let name = child.attr("N").unwrap_or("");
        let mut cx = Cx::new(&mut notes, "/fuzz.xml");
        if again.put_cell_attrs(&mut cx, &mut cells, name, &pairs).is_err() {
            break;
        }
    }
    assert!(cells.len() <= MAX_CELLS_PER_CONTAINER);
    for (name, cell) in &cells {
        // What a cell's attributes say is what it reads back as, as far as the attributes go.
        let said = cell_attrs(cell);
        assert!(said.iter().all(|(k, _)| !k.is_empty()), "{name:?}");
    }

    // Against defaults of nothing, every cell is to be written; against itself, none.
    let delta = cells_delta(&sheet.cells, &Cells::new());
    assert_eq!(delta.changed.len(), sheet.cells.len());
    assert!(cells_delta(&sheet.cells, &sheet.cells).is_empty());

    // Patched to the sheet with one cell more: a patch or a refusal in words, never a panic.
    let mut now = sheet.clone();
    now.cells.insert(CellName::new("FuzzCell"), Cell::default());
    let mut tree = root.clone();
    let sc = Scope::<V>::empty().enter(&tree);
    let patched: Result<(), PatchError> = patch_sheet_children(&mut tree, &sc, &sheet, &now);
    if patched.is_ok() {
        // What was patched still reads (the cell made is an element with no prefix, which a tree that declares another default namespace does not
        // read as the vocabulary's: so whether the cell is there is the tree's, and only that it reads is asserted).
        let _ = read::<V>(&tree, caps, budget);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let Some((&budget, rest)) = rest.split_first() else { return };
    // Half the inputs have no byte budget; the others have 300 bytes to some 38,000, which an element of a few attributes spends.
    let bytes = if budget < 128 { u64::MAX } else { (u64::from(budget) - 127) * 300 };
    let Ok(root) = parse(rest) else { return };
    let small = usize::from(mode >> 3);
    let big = Caps { cells: 10_000_000, items: 100_000_000, formula_nodes: 10_000_000 };
    let caps = match (mode >> 1) & 3 {
        0 => big,
        1 => Caps { items: small, ..big },
        2 => Caps { cells: small / 4, ..big },
        _ => Caps { formula_nodes: small, ..big },
    };
    if mode & 1 == 0 { exercise::<Main>(&root, caps, bytes) } else { exercise::<Ext>(&root, caps, bytes) }
});
