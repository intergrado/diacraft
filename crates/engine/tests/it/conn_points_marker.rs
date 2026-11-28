//! What a halving takes when the marker holds a term that breaks a rule (the findings of the review of the public 0.2.4 pull request on the marker of
//!). **A halving never removes an original** (owner, 2026-11-26): a row that two terms name is named by neither, whatever the
//! generation each gives (a term of generation 0, 256 or 2^32 still names its row), and a number and a name that name the one row are two terms for it.
//!
//! The tests go through the command, on the Rectangle doubled on its top (rows 4 and 5 added, the top's points being rows 4, 0 and 5 in the order along it), once with its
//! rows numbered and once with them named (`Row_1` to `Row_6`, a number naming a row by its place there).

use diacraft_calc::row_refs::RowName;
use diacraft_doc::{Cell, CellAddr, RowKey, Value as CellValue};
use diacraft_engine::Session;
use diacraft_engine::draw::conn_marker;
use proptest::prelude::*;
use serde_json::json;

use crate::blank_session;
use crate::conn_points::{act, marker_text, params, places, points, put_cell, put_marker, rect};
use crate::conn_points_edges::{page_of, rectangle, session_with};

/// The Rectangle doubled on its top, rows numbered (rows 0 to 3 its own, rows 4 and 5 the two added).
fn numbered() -> (Session, u32) {
    let mut s = blank_session();
    let id = rect(&mut s, 2.0, 1.0);
    act(&mut s, id, "double", json!({"edge": 2}));
    (s, id)
}

/// The Rectangle, its rows named `Row_1` to `Row_4`, doubled on its top: `Row_5` and `Row_6` are the two added.
fn named() -> (Session, u32) {
    let mut shape = rectangle(1, 2.0, 1.0);
    for (i, (x, y, dx, dy)) in [(1.0, 1.0, 0.0, 1.0), (0.0, 0.5, -1.0, 0.0), (1.0, 0.0, 0.0, -1.0), (2.0, 0.5, 1.0, 0.0)].into_iter().enumerate() {
        let key = RowKey::name(&format!("Row_{}", i + 1));
        for (name, value) in [("X", x), ("Y", y), ("DirX", dx), ("DirY", dy), ("Type", 0.0)] {
            shape = shape.row_cell("Connection", None, key.clone(), name, &value.to_string());
        }
    }
    let mut s = session_with(page_of(vec![shape]));
    act(&mut s, 1, "double", json!({"edge": 2}));
    (s, 1)
}

/// The rows, by their place, that a halving of the top takes under `text` as the marker: none when the call is refused, which it is when no point of the top is added.
fn taken(mut s: Session, id: u32, text: &str) -> Vec<usize> {
    put_marker(&mut s, id, text);
    let before = places(&s, id);
    if s.execute("shape.connection_points", &params(id, "halve", json!({"edge": 2}))).is_err() {
        return Vec::new();
    }
    let after = places(&s, id);
    (0..before.len()).filter(|i| before.get(*i).is_some_and(|p| !after.contains(p))).collect()
}

#[test]
fn a_term_that_gives_no_generation_still_names_its_row_so_a_row_named_twice_is_an_original() {
    // Rows 4 and 5 are the top's added points. A row's other term, of generation 0, 256 or 2^32, still names it: the row is named twice, and neither term stands.
    for (text, want) in [
        ("0*(1*Connections.X5+0*Connections.X5+1*Connections.X6)", vec![5]),
        ("0*(0*Connections.X5+1*Connections.X5+1*Connections.X6)", vec![5]),
        ("0*(1*Connections.X5+256*Connections.X5+1*Connections.X6)", vec![5]),
        ("0*(4294967296*Connections.X5+1*Connections.X5+1*Connections.X6)", vec![5]),
        ("0*(1*Connections.X6+0*Connections.X6+1*Connections.X5)", vec![4]),
        ("0*(1*Connections.X5+0*Connections.X5)", vec![]),
        // The review's own: row 0, the top's middle, named twice with a term of generation 0 beside the left side's row 1: every point is an original.
        ("0*(1*Connections.X1+0*Connections.X1+1*Connections.X2)", vec![]),
        // A term that names no row does not count against one that does.
        ("0*(1*Connections.X5+0*Connections.X99+1*Connections.X6)", vec![4, 5]),
        ("0*(1*Connections.X5+1*Connections.X6)", vec![4, 5]),
    ] {
        let (s, id) = numbered();
        assert_eq!(taken(s, id, text), want, "{text}");
    }
}

#[test]
fn a_number_and_a_name_that_name_the_one_row_are_two_terms_for_it() {
    // Row_5 is at place 4 (`X5`) and Row_6 at place 5 (`X6`): R17 numbers a named section's rows by their place.
    for (text, want) in [
        ("0*(1*Connections.X5+1*Connections.Row_5.X+1*Connections.Row_6.X)", vec![5]),
        ("0*(1*Connections.Row_5.X+1*Connections.X5+1*Connections.Row_6.X)", vec![5]),
        ("0*(1*Connections.X6+1*Connections.row_6.X+1*Connections.Row_5.X)", vec![4]),
        ("0*(1*Connections.Row_5.X+0*Connections.X5+1*Connections.Row_6.X)", vec![5]),
        // A lone term still stands: by its name.
        ("0*(1*Connections.Row_5.X+1*Connections.Row_6.X)", vec![4, 5]),
        ("0*(1*Connections.ROW_5.X+2*Connections.row_6.X)", vec![5]),
    ] {
        let (s, id) = named();
        assert_eq!(taken(s, id, text), want, "{text}");
    }
}

// ----: names that differ only in letter case are different rows (the exact spelling first, then any case) -------------------------

/// The rows' names, in the section's order.
fn names_of(s: &Session, id: u32) -> Vec<String> {
    points(s, id).into_iter().filter_map(|p| p.name).collect()
}

/// A connection point written by hand as a row of its own name, at the end of the section: the five cells of a row, constants.
fn put_named_point(s: &mut Session, id: u32, name: &str, (x, y): (f64, f64)) {
    for (cell, value, unit) in [("X", x, Some("IN")), ("Y", y, Some("IN")), ("DirX", 0.0, None), ("DirY", 0.0, None), ("Type", 0.0, None)] {
        let mut made = Cell::constant(CellValue::from_text(&value.to_string()));
        if let Some(unit) = unit {
            made = made.with_unit(unit);
        }
        put_cell(s, id, &CellAddr::in_row("Connection", None, RowKey::name(name), cell), made);
    }
}

/// The Rectangle with these named rows in this order, each `(name, x, y)` an ordinary inward point with no way out and no marker.
fn cased(rows: &[(&str, f64, f64)]) -> (Session, u32) {
    let mut shape = rectangle(1, 2.0, 1.0);
    for (name, x, y) in rows {
        let key = RowKey::name(name);
        for (cell, value) in [("X", *x), ("Y", *y), ("DirX", 0.0), ("DirY", 0.0), ("Type", 0.0)] {
            shape = shape.row_cell("Connection", None, key.clone(), cell, &value.to_string());
        }
    }
    (session_with(page_of(vec![shape])), 1)
}

/// The terms a marker's text names, each by the row's name as the text spells it, or by its number.
fn written_terms(s: &Session, id: u32) -> Vec<RowName> {
    marker_text(s, id).and_then(|text| conn_marker::parse_formula(&text)).map(|raw| raw.into_iter().map(|t| t.row).collect()).unwrap_or_default()
}

#[test]
fn a_halving_keeps_an_original_row_that_differs_from_an_added_one_only_in_letter_case() {
    // The Rectangle doubled on its top: `Row_5` (1.5, 1) and `Row_6` (0.5, 1) are added. A person then puts a point of their own, `ROW_5`, between them on the top.
    let (mut s, id) = named();
    put_named_point(&mut s, id, "ROW_5", (1.25, 1.0));
    assert_eq!(names_of(&s, id), ["Row_1", "Row_2", "Row_3", "Row_4", "Row_5", "Row_6", "ROW_5"]);
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["Row_1", "Row_2", "Row_3", "Row_4", "ROW_5"], "only the two added points went: the original ROW_5 stays");
    assert_eq!(marker_text(&s, id), None, "the marker's terms went with their rows");
    // And nothing more can be halved: ROW_5 was never added.
    assert!(s.execute("shape.connection_points", &params(id, "halve", json!({"edge": 2}))).is_err());
}

#[test]
fn a_halving_keeps_an_original_whose_row_comes_before_the_added_row_of_the_same_name_in_another_case() {
    // `ROW_5` is the section's first row, an original on the top; `Row_5` and `Row_6` are the added points the marker names. A name read in any case finds `ROW_5` first.
    let rows = [
        ("ROW_5", 1.25, 1.0),
        ("Row_1", 1.0, 1.0),
        ("Row_2", 0.0, 0.5),
        ("Row_3", 1.0, 0.0),
        ("Row_4", 2.0, 0.5),
        ("Row_5", 1.5, 1.0),
        ("Row_6", 0.5, 1.0),
    ];
    let (mut s, id) = cased(&rows);
    put_marker(&mut s, id, "0*(1*Connections.Row_5.X+1*Connections.Row_6.X)");
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["ROW_5", "Row_1", "Row_2", "Row_3", "Row_4"]);
    assert_eq!(marker_text(&s, id), None, "the term of Row_5 went with its row: ROW_5 is not the row it names");
}

#[test]
fn a_halving_drops_the_term_of_the_row_it_takes_and_not_that_of_a_row_that_spells_the_name_in_another_case() {
    // `Row_5` is of the first doubling and `Row_6` of the second: the halving takes `Row_6`, the newest, and the older `Row_5` stays. `ROW_6`, the section's first row, is an
    // original on the top, which a name read in any case would find before `Row_6`.
    let rows = [
        ("ROW_6", 0.25, 1.0),
        ("Row_1", 1.0, 1.0),
        ("Row_2", 0.0, 0.5),
        ("Row_3", 1.0, 0.0),
        ("Row_4", 2.0, 0.5),
        ("Row_5", 1.5, 1.0),
        ("Row_6", 0.5, 1.0),
    ];
    let (mut s, id) = cased(&rows);
    put_marker(&mut s, id, "0*(1*Connections.Row_5.X+2*Connections.Row_6.X)");
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["ROW_6", "Row_1", "Row_2", "Row_3", "Row_4", "Row_5"]);
    assert_eq!(written_terms(&s, id), [RowName::Name("Row_5".into())], "the term of Row_6 went with its row, and the term of Row_5 is kept");
    // The next halving takes Row_5 and nothing else, and the marker goes with it.
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["ROW_6", "Row_1", "Row_2", "Row_3", "Row_4"]);
    assert_eq!(marker_text(&s, id), None);
}

#[test]
fn a_doubling_beside_rows_that_differ_only_in_case_adds_rows_no_case_of_a_name_has_and_a_halving_takes_those_alone() {
    // The probe's section (Row_5 and Row_6 added, an original ROW_5), doubled again: the new rows take names no row has in any case, and each halving after takes the newest
    // generation, down to the first, with ROW_5 never among them.
    let (mut s, id) = named();
    put_named_point(&mut s, id, "ROW_5", (1.25, 1.0));
    act(&mut s, id, "double", json!({"edge": 2}));
    let names = names_of(&s, id);
    assert_eq!(names.len(), 7 + 5, "the top held Row_1, Row_5, ROW_5 and Row_6: five portions, five new points {names:?}");
    assert!(names.iter().take(7).eq(["Row_1", "Row_2", "Row_3", "Row_4", "Row_5", "Row_6", "ROW_5"].iter()), "{names:?}");
    let new: Vec<&String> = names.iter().skip(7).collect();
    assert!(new.iter().all(|n| !["row_5", "row_6"].contains(&n.to_ascii_lowercase().as_str())), "{new:?}");
    // The newest generation first.
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["Row_1", "Row_2", "Row_3", "Row_4", "Row_5", "Row_6", "ROW_5"]);
    act(&mut s, id, "halve", json!({"edge": 2}));
    assert_eq!(names_of(&s, id), ["Row_1", "Row_2", "Row_3", "Row_4", "ROW_5"]);
    assert_eq!(marker_text(&s, id), None);
}

/// What a term of the property names: the spelling, the row it is (by place) and whether it is spelled by the row's name.
type Spelling = (&'static str, Option<usize>, bool);

/// The Rectangle's rows by number, the places of rows 0 to 5 being their `IX`. Row 0 and rows 4 and 5 are the top's points; `X2` is the left side's; `X99` is no row.
const BY_NUMBER: [Spelling; 5] = [
    ("Connections.X1", Some(0), false),
    ("Connections.X5", Some(4), false),
    ("Connections.X6", Some(5), false),
    ("Connections.X2", Some(1), false),
    ("Connections.X99", None, false),
];

/// The rows: a number names a row by its place, a name in any case names the row.
const BY_NAME: [Spelling; 11] = [
    ("Connections.X1", Some(0), false),
    ("Connections.Row_1.X", Some(0), true),
    ("Connections.X5", Some(4), false),
    ("Connections.Row_5.X", Some(4), true),
    ("Connections.row_5.X", Some(4), true),
    ("Connections.X6", Some(5), false),
    ("Connections.ROW_6.X", Some(5), true),
    ("Connections.Row_6.X", Some(5), true),
    ("Connections.X2", Some(1), false),
    ("Connections.X99", None, false),
    ("Connections.Row_2.X", Some(1), true),
];

/// The generations a term may write: those of a point, and those no point has.
const GENERATIONS: [u64; 7] = [0, 1, 2, 3, 255, 256, 4_294_967_296];

/// The text of the marker of these terms.
fn marker_of(spellings: &[Spelling], terms: &[(u64, usize)]) -> String {
    let parts: Vec<String> = terms.iter().filter_map(|(g, at)| spellings.get(*at).map(|s| format!("{g}*{}", s.0))).collect();
    format!("0*({})", parts.join("+"))
}

/// The rows a halving of the top takes by the rules, worked out from the terms alone. The top's points are `top`; a point is added with the generation its
/// term gives when that is the only term that names it (whatever the generation of the others) and the generation is from 1 to 255; a term by a number names a named
/// row for the count and stands for nothing (the marker reads a row by the spelling it has it in), so a section keyed by name gives `names`, its rows' names by place, and
/// a term must be spelled by the row's name to stand there. **Two terms that spell a name alike but for its case stand for neither**, though they name two rows (the
/// marker keeps no pair a halving could mistake for one row). Returns the rows taken and the rows that may be: those with one term of a generation a point has.
fn expected(spellings: &[Spelling], terms: &[(u64, usize)], top: &[usize], names: &[&str]) -> (Vec<usize>, Vec<usize>) {
    let named = !names.is_empty();
    let rows: Vec<(usize, u64, bool)> = terms.iter().filter_map(|(g, at)| spellings.get(*at).and_then(|s| s.1.map(|row| (row, *g, s.2)))).collect();
    let times = |row: usize| rows.iter().filter(|(r, ..)| *r == row).count();
    let folded = |row: usize| names.get(row).map(|n| n.to_ascii_lowercase());
    let alike = |row: usize| rows.iter().filter(|(r, _, by_name)| *by_name && folded(*r) == folded(row)).count();
    let valid = |g: u64| (1..=255).contains(&g);
    let lone = |row: usize, by_name: bool| times(row) == 1 && (!by_name || alike(row) == 1);
    // The rows named by one term of a generation a point has: the ones that may be added.
    let single: Vec<usize> = rows.iter().filter(|(r, g, by_name)| lone(*r, *by_name) && valid(*g)).map(|(r, ..)| *r).collect();
    let stands =
        |row: usize| rows.iter().find(|(r, g, by_name)| *r == row && lone(row, *by_name) && valid(*g) && (by_name | !named)).map(|(_, g, _)| *g);
    let newest = top.iter().filter_map(|row| stands(*row)).max();
    let mut takes: Vec<usize> = top.iter().copied().filter(|row| newest.is_some() && stands(*row) == newest).collect();
    takes.sort_unstable();
    (takes, single)
}

/// The names of the rows a call of halving takes from the top one after the other, newest generation first, until the call is refused; `Err` when a marker it leaves
/// names a row that is not there by the spelling it has, which is the sign of a term that was taken for another row's.
fn halved_down(mut s: Session, id: u32, text: &str) -> Result<Vec<String>, String> {
    put_marker(&mut s, id, text);
    let mut gone = Vec::new();
    for _ in 0..12 {
        let before = names_of(&s, id);
        if s.execute("shape.connection_points", &params(id, "halve", json!({"edge": 2}))).is_err() {
            break;
        }
        let after = names_of(&s, id);
        gone.extend(before.iter().filter(|n| !after.contains(n)).cloned());
        for term in written_terms(&s, id) {
            let there = match &term {
                RowName::Name(n) => after.contains(n),
                RowName::Number(n) => usize::try_from(*n).is_ok_and(|n| (1..=after.len()).contains(&n)),
                RowName::Default => false,
            };
            if !there {
                return Err(format!("the marker {:?} names {term:?}, a row that is not there: {after:?}", marker_text(&s, id)));
            }
        }
    }
    Ok(gone)
}

/// The rows of a section whose names differ in letter case, by place: row 0 and the rows 4 to 8 are the top's points (`Row_5`, `ROW_5` and `row_5` three rows of one name in
/// three cases, as are `Row_6` and `ROW_6`); rows 1 to 3 are the other sides'.
const CASED_ROWS: [(&str, f64, f64); 9] = [
    ("Row_1", 1.0, 1.0),
    ("Row_2", 0.0, 0.5),
    ("Row_3", 1.0, 0.0),
    ("Row_4", 2.0, 0.5),
    ("Row_5", 1.5, 1.0),
    ("ROW_5", 1.25, 1.0),
    ("row_5", 1.75, 1.0),
    ("Row_6", 0.5, 1.0),
    ("ROW_6", 0.25, 1.0),
];

/// The places of the top's points among [`CASED_ROWS`].
const CASED_TOP: [usize; 6] = [0, 4, 5, 6, 7, 8];

/// What names a row of [`CASED_ROWS`]: a number by its place, a name by the exact spelling first (`ROW_5` the row `ROW_5`, `row_5` the row `row_5`), and with no exact
/// spelling the first row of any case (`rOw_5` the row `Row_5`, `row_6` the row `Row_6`).
const BY_CASE: [Spelling; 15] = [
    ("Connections.X1", Some(0), false),
    ("Connections.Row_1.X", Some(0), true),
    ("Connections.row_1.X", Some(0), true),
    ("Connections.X5", Some(4), false),
    ("Connections.Row_5.X", Some(4), true),
    ("Connections.ROW_5.X", Some(5), true),
    ("Connections.row_5.X", Some(6), true),
    ("Connections.rOw_5.X", Some(4), true),
    ("Connections.X6", Some(5), false),
    ("Connections.X7", Some(6), false),
    ("Connections.Row_6.X", Some(7), true),
    ("Connections.ROW_6.X", Some(8), true),
    ("Connections.row_6.X", Some(7), true),
    ("Connections.X8", Some(7), false),
    ("Connections.X99", None, false),
];

fn case_names() -> Vec<&'static str> {
    CASED_ROWS.iter().map(|r| r.0).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Over markers of terms of every shape (generations a point has and ones none has, rows named twice, by a number and by a name, rows that are not there): a halving of
    /// the top takes exactly the newest of the added points of the top that the rules leave, and **never a row that two terms name or one a term gives no generation**.
    #[test]
    fn a_halving_never_takes_an_original_of_a_numbered_section(terms in proptest::collection::vec((proptest::sample::select(GENERATIONS.to_vec()), 0..BY_NUMBER.len()), 1..9)) {
        let (s, id) = numbered();
        let got = taken(s, id, &marker_of(&BY_NUMBER, &terms));
        let (want, single) = expected(&BY_NUMBER, &terms, &[4, 0, 5], &[]);
        prop_assert_eq!(&got, &want, "{}", marker_of(&BY_NUMBER, &terms));
        prop_assert!(got.iter().all(|row| single.contains(row)), "an original was taken: {got:?}");
    }

    /// The same for a section keyed by name, where a number names a row by its place and a name in any letter case names it as well.
    #[test]
    fn a_halving_never_takes_an_original_of_a_section_keyed_by_name(terms in proptest::collection::vec((proptest::sample::select(GENERATIONS.to_vec()), 0..BY_NAME.len()), 1..9)) {
        let (s, id) = named();
        let text = marker_of(&BY_NAME, &terms);
        let got = taken(s, id, &text);
        let (want, single) = expected(&BY_NAME, &terms, &[4, 0, 5], &["Row_1", "Row_2", "Row_3", "Row_4", "Row_5", "Row_6"]);
        prop_assert_eq!(&got, &want, "{}", text);
        prop_assert!(got.iter().all(|row| single.contains(row)), "an original was taken: {got:?}");
    }
}

/// How many cases a property of the section of rows that differ in case runs: `CP_PROPTEST_CASES` when it is set (a longer run of the same properties), else `default`.
fn cases(default: u32) -> u32 {
    std::env::var("CP_PROPTEST_CASES").ok().and_then(|text| text.parse().ok()).unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(1_000), ..ProptestConfig::default() })]

    /// Over markers of a section whose rows differ only in letter case (`Row_5`, `ROW_5` and `row_5`; `Row_6` and `ROW_6`), with terms that spell a name in
    /// any case, by the exact spelling or in another one, and by a number: a halving of the top takes exactly the newest of the added points the rules leave, and **never a
    /// row no term of a generation a point has names**. A name names the row of that spelling first and a row of another case only when none has it.
    #[test]
    fn a_halving_never_takes_an_original_of_a_section_whose_names_differ_only_in_case(terms in proptest::collection::vec((proptest::sample::select(GENERATIONS.to_vec()), 0..BY_CASE.len()), 1..11)) {
        let (s, id) = cased(&CASED_ROWS);
        let text = marker_of(&BY_CASE, &terms);
        let got = taken(s, id, &text);
        let (want, single) = expected(&BY_CASE, &terms, &CASED_TOP, &case_names());
        prop_assert_eq!(&got, &want, "{}", text);
        prop_assert!(got.iter().all(|row| single.contains(row)), "an original was taken: {got:?} of {text}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(400), ..ProptestConfig::default() })]

    /// The same, halving the top again and again: each call takes the newest generation left, and in all they take exactly the added points of the top the rules leave and
    /// no other row, so that a marker one halving writes does not make an original of the next one an added point. Each marker written names rows that are there.
    #[test]
    fn halving_again_and_again_never_takes_an_original_of_a_section_whose_names_differ_only_in_case(terms in proptest::collection::vec((proptest::sample::select(GENERATIONS.to_vec()), 0..BY_CASE.len()), 1..11)) {
        let (s, id) = cased(&CASED_ROWS);
        let text = marker_of(&BY_CASE, &terms);
        let got = halved_down(s, id, &text);
        prop_assert!(got.is_ok(), "{}: {:?}", text, got);
        let mut got = got.unwrap_or_default();
        got.sort();
        let (_, single) = expected(&BY_CASE, &terms, &CASED_TOP, &case_names());
        let mut want: Vec<String> = taken_over_all_halvings(&terms).iter().map(|row| CASED_ROWS.get(*row).map_or_else(String::new, |r| r.0.to_owned())).collect();
        want.sort();
        prop_assert_eq!(&got, &want, "{}", text);
        prop_assert!(got.iter().all(|name| single.iter().any(|row| CASED_ROWS.get(*row).is_some_and(|r| r.0 == name))), "an original was taken: {got:?} of {text}");
    }
}

/// The rows of the top that every halving takes in all, one generation after the other: those that stand under `terms`, each with its term gone once it is taken.
fn taken_over_all_halvings(terms: &[(u64, usize)]) -> Vec<usize> {
    let (mut gone, mut left) = (Vec::new(), terms.to_vec());
    loop {
        let (takes, _) = expected(&BY_CASE, &left, &CASED_TOP, &case_names());
        if takes.is_empty() {
            return gone;
        }
        left.retain(|(_, at)| BY_CASE.get(*at).and_then(|s| s.1).is_none_or(|row| !takes.contains(&row)));
        gone.extend(takes);
    }
}
