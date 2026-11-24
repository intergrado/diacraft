//! References to cells and sheets.
//!
//! The parser keeps the parts of a reference as written and looks nothing up; the document and the evaluator resolve them.
//! So `Char.Size[2]` is a path of two names, the second with an index, and the section and the row `IX` it names are
//! not here.

use diacraft_formula::{Expr, Index, ParseErrorKind, Qualifier, Segment, parse};

use crate::{err, ok, shape};

fn cell_ref(src: &str) -> diacraft_formula::CellRef {
    match ok(src) {
        Expr::Ref(r) => *r,
        other => panic!("{src}: {other:?}"),
    }
}

fn names(src: &str) -> Vec<String> {
    cell_ref(src).path.iter().map(|s| s.name.to_string()).collect()
}

#[test]
fn every_reference_spelling_in_the_index_is_a_reference_with_its_parts() {
    // The 129 spellings of the Formula name column that hold a dot, a bracket or a placeholder, with a row name `Row_1`,
    // row 3 and column 2 filled in. Each is one reference whose dotted names and index are the text's.
    let spellings = crate::names::reference_spellings();
    assert_eq!(spellings.len(), 129);
    for text in &spellings {
        let r = cell_ref(text);
        assert!(r.qualifiers.is_empty(), "{text}");
        let (dotted, index) = match text.strip_suffix(']').and_then(|t| t.rsplit_once('[')) {
            Some((head, i)) => (head, Some(i.parse::<u32>().unwrap())),
            None => (text.as_str(), None),
        };
        let expect: Vec<&str> = dotted.split('.').collect();
        let got: Vec<String> = r.path.iter().map(|s| s.name.to_string()).collect();
        assert_eq!(got, expect, "{text}");
        assert_eq!(r.path.last().and_then(|s| s.index.clone()), index.map(Index::Number), "{text}");
        assert!(r.path[..r.path.len() - 1].iter().all(|s| s.index.is_none()), "{text}");
        // And the same behind a sheet qualifier.
        assert_eq!(cell_ref(&format!("Sheet.4!{text}")).path, r.path, "{text}");
    }
}

#[test]
fn an_indexed_row_and_a_cell_with_no_index() {
    let r = cell_ref("Char.Size[2]");
    assert!(r.qualifiers.is_empty());
    assert_eq!(r.path.len(), 2);
    assert_eq!(&*r.path[0].name, "Char");
    assert_eq!(r.path[0].index, None);
    assert_eq!(&*r.path[1].name, "Size");
    assert_eq!(r.path[1].index, Some(Index::Number(2)));
    // No `[row]` is a missing row, which the resolver reads as row 1.
    assert_eq!(cell_ref("Char.Size").path[1].index, None);
    assert_eq!(shape("Char.Size[2]"), "@Char.Size[2]");
}

#[test]
fn a_one_letter_cell_with_its_row_and_a_geometry_row_are_names() {
    // `Scratch.B3`: section Scratch, cell B, row 3: all in the second name, which the resolver splits.
    assert_eq!(names("Scratch.B3"), ["Scratch", "B3"]);
    assert_eq!(names("Geometry2.X4"), ["Geometry2", "X4"]);
    // The row number in a Geometry reference is IX, and the section's is IX plus one: both are the resolver's.
    assert_eq!(names("Geometry1.NoFill"), ["Geometry1", "NoFill"]);
    assert_eq!(names("Connections.X3"), ["Connections", "X3"]);
    assert_eq!(names("Tabs.32"), ["Tabs", "32"]);
    assert_eq!(names("Scratch.A1"), ["Scratch", "A1"]);
}

#[test]
fn named_rows_with_and_without_a_cell() {
    assert_eq!(names("User.Count"), ["User", "Count"]);
    assert_eq!(names("Actions.Go"), ["Actions", "Go"]);
    assert_eq!(names("Controls.Handle"), ["Controls", "Handle"]);
    assert_eq!(names("Hyperlink.Site"), ["Hyperlink", "Site"]);
    assert_eq!(names("Prop.Cost.Label"), ["Prop", "Cost", "Label"]);
    assert_eq!(names("Prop.Cost.Value"), ["Prop", "Cost", "Value"]);
    assert_eq!(names("User.msvThemeColors"), ["User", "msvThemeColors"]);
    assert_eq!(names("Width"), ["Width"]);
    assert_eq!(names("_X"), ["_X"]);
}

#[test]
fn a_shape_on_another_page() {
    // `Pages[Site-2]!Sheet.9!EndY`: two qualifiers, and the cell after them.
    let r = cell_ref("Pages[Site-2]!Sheet.9!EndY");
    assert_eq!(
        r.qualifiers,
        [
            Qualifier::Path(vec![Segment { name: "Pages".into(), index: Some(Index::Name("Site-2".into())) }]),
            Qualifier::Path(vec![Segment::new("Sheet"), Segment::new("9")]),
        ]
    );
    assert_eq!(r.path, [Segment::new("EndY")]);
    // And a shape of this page by its ID.
    let r = cell_ref("Sheet.12!PinX");
    assert_eq!(r.qualifiers, [Qualifier::Path(vec![Segment::new("Sheet"), Segment::new("12")])]);
    assert_eq!(r.path, [Segment::new("PinX")]);
}

#[test]
fn every_form_of_a_sheet_qualifier() {
    for (src, expect) in [
        ("Door!Width", "@Door!Width"),
        ("Door.7!Width", "@Door.7!Width"),
        ("Sheet.12!PinX", "@Sheet.12!PinX"),
        ("'Left-Door'!Width", "@Left-Door!Width"),
        ("ThePage!User.Origin", "@ThePage!User.Origin"),
        ("TheDoc!User.Count", "@TheDoc!User.Count"),
        ("Pages[Site-2]!Sheet.9!EndY", "@Pages[\"Site-2\"]!Sheet.9!EndY"),
        ("Pages[2]!Sheet.9!EndY", "@Pages[2]!Sheet.9!EndY"),
        ("Masters[Pump]!Valve!Width", "@Masters[\"Pump\"]!Valve!Width"),
        ("Styles!Callout!LineWeight", "@Styles!Callout!LineWeight"),
        ("Sheet.7!Prop.Cost.Label", "@Sheet.7!Prop.Cost.Label"),
        ("Sheet.7!Geometry1.X2", "@Sheet.7!Geometry1.X2"),
        ("Sheet.7 ! Width", "@Sheet.7!Width"),
    ] {
        assert_eq!(shape(src), expect, "{src}");
    }
}

#[test]
fn a_call_can_be_a_qualifier_and_a_qualifier_can_precede_a_call() {
    // Both `<sheet>!SHEETREF()` and `LISTSHEETREF(2)!Width`.
    let r = cell_ref("LISTSHEETREF(2)!Width");
    assert_eq!(r.qualifiers.len(), 1);
    let Qualifier::Call { name, args } = &r.qualifiers[0] else { panic!("{r:?}") };
    assert_eq!(&**name, "LISTSHEETREF");
    assert_eq!(args.len(), 1);
    assert_eq!(r.path, [Segment::new("Width")]);
    assert_eq!(shape("LISTSHEETREF(2)!Width"), "@LISTSHEETREF(2)!Width");

    let Expr::Call(c) = ok("Sheet.5!SHEETREF()") else { panic!() };
    assert_eq!(c.qualifiers, [Qualifier::Path(vec![Segment::new("Sheet"), Segment::new("5")])]);
    assert!(c.is("SheetRef") && c.args.is_empty());
    // The census: `SheetRef` in mixed case, straight after a qualifier; and calls chained.
    assert_eq!(shape("Sheet.5!SheetRef()"), "(Sheet.5!call SheetRef)");
    assert_eq!(shape("CONTAINERSHEETREF()!Width"), "@CONTAINERSHEETREF()!Width");
    assert_eq!(shape("Sheet.5!LISTSHEETREF(1)!Width"), "@Sheet.5!LISTSHEETREF(1)!Width");
    assert_eq!(shape("LISTSHEETREF(1)!SHEETREF()"), "(LISTSHEETREF(1)!call SHEETREF)");
}

#[test]
fn a_quoted_name_doubles_its_quote() {
    // A single quote inside the name is doubled.
    let r = cell_ref("'O''Brien'!Width");
    assert_eq!(r.qualifiers, [Qualifier::Path(vec![Segment::new("O'Brien")])]);
    // The reference page allows any character inside the quotes; MS-VSDX only letters, digits and underscores. Both read.
    assert_eq!(names("'Left-Door'!Width"), ["Width"]);
    let r = cell_ref("'Left-Door'!Width");
    assert_eq!(r.qualifiers, [Qualifier::Path(vec![Segment::new("Left-Door")])]);
    let r = cell_ref("'a b.c!d(e)[f]'!X");
    assert_eq!(r.qualifiers, [Qualifier::Path(vec![Segment::new("a b.c!d(e)[f]")])]);
    // A quoted name as the cell, and in the middle of a path.
    assert_eq!(names("'Width'"), ["Width"]);
    assert_eq!(names("Prop.'Row 1'.Value"), ["Prop", "Row 1", "Value"]);
    assert_eq!(names("'é ü'"), ["é ü"]);
    // And its errors.
    assert_eq!(err("'abc").kind, ParseErrorKind::BadQuotedName);
    assert_eq!(err("''").kind, ParseErrorKind::BadQuotedName);
    assert_eq!(err("'a''").kind, ParseErrorKind::BadQuotedName);
    assert_eq!(err("Prop.'").kind, ParseErrorKind::BadQuotedName);
}

#[test]
fn a_qualifier_reaches_the_one_reference_after_it() {
    let Expr::Binary(_, a, b) = ok("Sheet.7!Width+Width") else { panic!() };
    assert_eq!(sexp_of(&a), "@Sheet.7!Width");
    assert_eq!(sexp_of(&b), "@Width");
    assert_eq!(shape("Sheet.7!Width+Width"), "(+ @Sheet.7!Width @Width)");
    assert_eq!(shape("Sheet.7!Width+Sheet.8!Width"), "(+ @Sheet.7!Width @Sheet.8!Width)");
}

fn sexp_of(e: &Expr) -> String {
    crate::sexp(e)
}

#[test]
fn m1_ruling_13_names_keep_their_case_and_are_not_folded_together() {
    // The parser keeps a cell's name as written; whether `width` is a cell is the document's, by exact case. So the trees of
    // two spellings differ, and nothing is lower-cased on the way.
    assert_ne!(ok("Width"), ok("width"));
    assert_ne!(ok("Width"), ok("WIDTH"));
    assert_eq!(names("Width"), ["Width"]);
    assert_eq!(names("width"), ["width"]);
    assert_eq!(names("XCon"), ["XCon"]);
    assert_eq!(names("xCon"), ["xCon"]);
    assert_eq!(names("Controls.Row_1.XCon"), ["Controls", "Row_1", "XCon"]);
    assert_eq!(names("controls.row_1.xcon"), ["controls", "row_1", "xcon"]);
    assert_ne!(ok("Prop.Cost"), ok("prop.cost"));
    assert_ne!(ok("Sheet.7!PinX"), ok("sheet.7!pinx"));
    // The case of a function name is kept too, though it is matched without regard to it.
    assert_eq!(shape("GetRef(Scratch.A1)"), "(call GetRef @Scratch.A1)");
    assert_eq!(shape("getref(Scratch.A1)"), "(call getref @Scratch.A1)");
    // And a name in quotes keeps its case, spaces and all.
    assert_eq!(names("'Pump Room'"), ["Pump Room"]);
    // `TRUE` and `FALSE` are words in any case, but a quoted one is a name (and `Width` is not a word).
    assert_eq!(shape("true"), "TRUE");
    assert_eq!(shape("'true'"), "@true");
    assert_eq!(shape("True.Width"), "@True.Width");
    assert_eq!(shape("Sheet.1!true"), "@Sheet.1!true");
}

#[test]
fn an_index_holds_a_number_or_a_name() {
    let idx = |src: &str| cell_ref(src).path.last().and_then(|s| s.index.clone());
    assert_eq!(idx("Char.Size[2]"), Some(Index::Number(2)));
    assert_eq!(idx("Char.Size[ 2 ]"), Some(Index::Number(2)));
    assert_eq!(idx("Char.Size[007]"), Some(Index::Number(7)));
    assert_eq!(idx("Char.Size[0]"), Some(Index::Number(0)));
    assert_eq!(idx("Pages[Site-2]"), Some(Index::Name("Site-2".into())));
    assert_eq!(idx("Pages[Site 2]"), Some(Index::Name("Site 2".into())));
    assert_eq!(idx("Pages[ Site 2 ]"), Some(Index::Name("Site 2".into())));
    // A quoted index is a name even when it looks like a number.
    assert_eq!(idx("Pages['2']"), Some(Index::Name("2".into())));
    assert_eq!(idx("Pages['O''B']"), Some(Index::Name("O'B".into())));
    assert_eq!(idx("Pages['a]b']"), Some(Index::Name("a]b".into())));
    // Too big for a row number is a name; none, empty and open ones are errors.
    assert_eq!(idx("Pages[99999999999]"), Some(Index::Name("99999999999".into())));
    assert_eq!(err("Char.Size[]").kind, ParseErrorKind::BadIndex);
    assert_eq!(err("Char.Size[ ]").kind, ParseErrorKind::BadIndex);
    assert_eq!(err("Char.Size[2").kind, ParseErrorKind::BadIndex);
    assert_eq!(err("Char.Size[").kind, ParseErrorKind::BadIndex);
    assert_eq!(err("Pages['a'x]").kind, ParseErrorKind::BadIndex);
    let long = "x".repeat(300);
    assert_eq!(err(&format!("Pages[{long}]")).kind, ParseErrorKind::BadIndex);
    // An index in the middle of a path, and on a qualifier.
    assert_eq!(shape("A[1].B[2]"), "@A[1].B[2]");
    // No space before the bracket: `A [1]` is a name and then an operand that does not belong.
    assert!(parse("A [1]").ast.is_err());
}

#[test]
fn malformed_references_are_errors_not_guesses() {
    for (src, kind) in [
        ("Prop.", ParseErrorKind::BadName),
        ("Prop..Value", ParseErrorKind::BadName),
        ("Prop.-1", ParseErrorKind::BadName),
        (".Prop", ParseErrorKind::ExpectedOperand),
        ("Width!", ParseErrorKind::UnexpectedEnd),
        ("!Width", ParseErrorKind::ExpectedOperand),
        ("Width!!X", ParseErrorKind::ExpectedOperand),
        ("A!(1)", ParseErrorKind::ExpectedOperand),
        ("Prop.Cost(1)", ParseErrorKind::ExpectedOperator),
        ("A[1](2)", ParseErrorKind::ExpectedOperator),
        ("'F'(1)", ParseErrorKind::ExpectedOperator),
    ] {
        assert_eq!(err(src).kind, kind, "{src}");
    }
}

#[test]
fn white_space_inside_a_path_ends_it() {
    // Dots and brackets are not spaced: `A . B` is not a path.
    assert!(parse("A . B").ast.is_err());
    assert!(parse("A .B").ast.is_err());
    assert!(parse("A. B").ast.is_err());
    // A qualifier's `!` may be (white space by example only).
    assert_eq!(shape("A ! B"), "@A!B");
}

#[test]
fn a_qualifier_chain_is_bounded() {
    // 17 qualifiers is far beyond any formula; 1,000 are hostile. Either is an error and not a slow parse.
    let chain = |n: usize| format!("{}X", "A!".repeat(n));
    assert!(parse(&chain(16)).ast.is_ok());
    assert_eq!(err(&chain(1000)).kind, ParseErrorKind::TooDeep);
}
