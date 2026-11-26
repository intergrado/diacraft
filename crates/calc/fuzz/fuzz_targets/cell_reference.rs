//! `cargo fuzz run cell_reference`: the text of a cell reference, and the dry run of a formula. The input is any bytes, read as text by lossy
//! UTF-8 (a byte that is no character becomes U+FFFD, so the fuzzer reaches every text and the properties stay about text).
//!
//! **`cell_address`** reads the text as one reference with no sheet before it. It gives a cell or a refusal in words, never a panic, and a cell it gives
//! **has a reference that reads back as it**: `reference_name` of the cell is a text of at most the bytes `cell_address` reads, and `cell_address` of that text
//! is the same cell (the inverse the commands rest on: a program sends back the `ref` it was given). Where the text has white space at its ends or a name in
//! quotes, the reading is the cell all the same.
//!
//! **`evaluate_text`** then tries the same text as a formula, with a small budget, on a document that holds a few cells to read (a page, and the document's
//! sheet with a formula that reads another): the first byte of the input picks the sheet (the document's, the page's, the root style's, one that is not
//! there) and the cell the text is for (none, a cell that holds a formula, a cell of an action row), and the rest is the text. A text is a value, a
//! parse error or the answer for an action cell, and never a panic; **the steps it took never pass the budget**; a parse error's offset is inside the
//! text (or is the parser's own length limit); and **the document is the same after**, which the borrow already says and a clone compared says again.
//! A text of the mutating kind (`SETF`, a cycle, a chain of cells, `EVALTEXT` of `EVALTEXT`) is a value or an error within the budget.
//!
//! **`reference_spans`** reads the same text for where each cell reference sits in it: the parser's error when it does not parse (the one `parse` gives), else spans
//! inside the text on character boundaries in the order the references begin, each covering text that read alone is the reference it holds, with its segments inside it; and
//! **`Locator::locate`** finds the cell each of the first references names on the same document, within the same budget and never a panic, writing nothing.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); nothing here needs a corpus, and none is committed.
#![no_main]

use std::sync::OnceLock;

use diacraft_calc::{Budget, EvalCtx, EvalOptions, EvalResult, Locator, MAX_REFERENCE_BYTES, cell_address, evaluate_text, reference_name};
use diacraft_doc::{Cell, CellAddr, Document, RowKey, SheetRef, Value};
use diacraft_formula::{Expr, parse, reference_spans};
use libfuzzer_sys::fuzz_target;

/// The most steps a run of the dry run may take: small, so that an input stays quick and a loop shows as the budget reached.
const STEPS: u64 = 20_000;

/// A document with the root style, one page, and a document sheet of a few cells that read each other.
fn document() -> &'static Document {
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(|| {
        let mut doc = Document::new_blank();
        let sheet = std::sync::Arc::make_mut(&mut doc.sheet);
        let put = |sheet: &mut diacraft_doc::Sheet, addr: CellAddr, cell: Cell| {
            // A cell that cannot be put leaves the document smaller, which a fuzz run may meet and ignore.
            let _ = sheet.set_cell(&addr, cell);
        };
        put(sheet, CellAddr::direct("Width"), Cell::constant(Value::from_text("2")).with_unit("IN"));
        put(sheet, CellAddr::direct("LocPinX"), Cell::with_formula("Width*0.5", Some(Value::from_text("1"))));
        put(sheet, CellAddr::in_row("User", None, RowKey::name("Total"), "Value"), Cell::with_formula("Width+LocPinX", Some(Value::from_text("3"))));
        put(sheet, CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A"), Cell::constant(Value::from_text("7")));
        put(sheet, CellAddr::in_row("Scratch", None, RowKey::Ix(1), "A"), Cell::with_formula("Scratch.A1+1", Some(Value::from_text("8"))));
        put(sheet, CellAddr::in_row("Actions", None, RowKey::name("Row_1"), "Action"), Cell::with_formula("SETF(GetRef(Width),9)", None));
        doc
    })
}

fuzz_target!(|data: &[u8]| {
    let all = String::from_utf8_lossy(data);

    // The reference: read, and spelled back.
    match cell_address(&all) {
        Ok(addr) => {
            let spelled = reference_name(&addr).expect("a cell that was read has a reference");
            assert!(spelled.len() <= MAX_REFERENCE_BYTES, "a reference of {} bytes", spelled.len());
            assert_eq!(cell_address(&spelled), Ok(addr), "`{spelled}` does not read back as the cell it was spelled for");
        }
        Err(e) => assert!(!e.to_string().is_empty()),
    }

    // Where the references of the text sit, and which cell each names.
    match reference_spans(&all) {
        Err(e) => assert_eq!(Some(e), parse(&all).ast.err(), "the error is the parser's"),
        Ok(spans) => {
            let doc = document();
            let before = doc.clone();
            let mut ctx = EvalCtx::new(EvalOptions::default(), Budget { steps: STEPS, bytes: 16 << 20 });
            let mut locator = Locator::new(doc, &mut ctx);
            let mut last = 0;
            for (n, span) in spans.iter().enumerate() {
                assert!(
                    last <= span.start && span.start <= span.end && span.end <= all.len(),
                    "a span of {}..{} in {} bytes",
                    span.start,
                    span.end,
                    all.len()
                );
                last = span.start;
                let alone = all.get(span.start..span.end).expect("a span is on character boundaries");
                match parse(alone).ast {
                    Ok(Expr::Ref(r)) => assert_eq!(*r, span.reference, "`{alone}` in `{all}`"),
                    other => panic!("`{alone}` in `{all}` is no reference: {other:?}"),
                }
                assert_eq!(span.segments.len(), span.reference.path.len());
                let mut at = span.start;
                for segment in &span.segments {
                    assert!(at <= segment.name.start && segment.name.start <= segment.name.end && segment.name.end <= span.end);
                    at = segment.name.end;
                    if let Some(digits) = &segment.index {
                        assert!(at <= digits.start && digits.start < digits.end && digits.end <= span.end);
                        assert!(all.get(digits.clone()).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())));
                        at = digits.end;
                    }
                }
                if n < 64 {
                    // The cell a reference names, from the page's sheet, the document's and a shape's that is not there: some cell or none, and never a panic.
                    for sheet in [SheetRef::Page(1), SheetRef::Document, SheetRef::Page(99)] {
                        let _ = locator.locate(&sheet, &span.reference);
                    }
                }
            }
            drop(locator);
            assert_eq!(doc, &before, "locating writes nothing");
        }
    }

    // The dry run: the first byte chooses the sheet and the cell, the rest is the text.
    let Some((&pick, rest)) = data.split_first() else { return };
    let text = String::from_utf8_lossy(rest);
    let doc = document();
    let sheet = match pick % 4 {
        0 => SheetRef::Document,
        1 => SheetRef::Page(1),
        2 => SheetRef::Style(0),
        _ => SheetRef::Page(99),
    };
    let at = match (pick / 4) % 4 {
        0 => None,
        1 => Some(CellAddr::direct("LocPinX")),
        2 => Some(CellAddr::in_row("Actions", None, RowKey::name("Row_1"), "Action")),
        _ => Some(CellAddr::in_row("User", None, RowKey::name("Total"), "Value")),
    };
    let before = doc.clone();
    let budget = Budget { steps: STEPS, bytes: 16 << 20 };
    let got = evaluate_text(doc, &sheet, at.as_ref(), &text, &budget);
    assert!(got.steps <= STEPS, "{} steps of {STEPS}", got.steps);
    if let EvalResult::ParseError(e) = &got.result {
        assert!(e.offset <= text.len().max(1 << 20), "an offset of {} in a text of {} bytes", e.offset, text.len());
        assert_eq!(got.nodes, 0);
    }
    if got.budget_exhausted {
        assert!(matches!(got.result, EvalResult::Value(_)), "a spent budget is a value");
    }
    assert_eq!(doc, &before, "a dry run writes nothing");
});
