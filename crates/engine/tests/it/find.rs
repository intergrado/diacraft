//! `edit.find` and `edit.replace` (Home > Editing > Find and Replace).
//!
//! The drawings are built in code from the specifications' XML and opened through `file.open`, so the cells are the ones a reader makes: a shape's text in runs and a
//! field, a name, shape data and user-defined cells that are constants, quoted strings and formulas, and one that a master gives.

use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, TextPart};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, cell, cell_f, shape, shape_with};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::{bad_params_msg, check_session, run, snapshot};

/// The inner XML of a text of two runs: `Hello world` and ` Hello`.
const TWO_RUNS: &str = r#"<Text><cp IX="0"/>Hello world<cp IX="1"/> Hello</Text>"#;

fn rows(section: &str, rows: &[(&str, String)]) -> String {
    let rows: String = rows.iter().map(|(name, cells)| format!(r#"<Row N="{name}">{cells}</Row>"#)).collect();
    format!(r#"<Section N="{section}">{rows}</Section>"#)
}

/// Page 1 of the main drawing: a text of two runs, a group with two members, a named shape, a shape with shape data and user cells, and a shape with a field.
fn page_one() -> String {
    let group = shape_with(
        2,
        r#" Type="Group""#,
        &format!(r#"{}<Shapes>{}{}</Shapes>"#, cell("PinX", "5"), shape(3, "<Text>world peace</Text>"), shape(4, "<Text>no match</Text>")),
    );
    let named = shape_with(5, r#" Type="Shape" Name="Hello Box" NameU="Hello Box""#, "<Text>box</Text>");
    let data = shape(
        6,
        &[
            rows(
                "Property",
                &[
                    ("Cost", cell("Value", "Hello cost")),
                    ("Note", cell_f("Value", "Hello note", "\"Hello note\"")),
                    ("Calc", cell_f("Value", "Hello calc", "\"Hello \"&\"calc\"")),
                    ("Price", cell("Value", "100")),
                ],
            ),
            rows("User", &[("Tag", cell("Value", "Hello tag"))]),
            "<Text>plain</Text>".to_owned(),
        ]
        .concat(),
    );
    let field = shape(7, r#"<Text>Page <fld IX="0">3</fld> of 9</Text>"#);
    [shape(1, TWO_RUNS), group, named, data, field].concat()
}

fn page_two() -> String {
    shape(1, "<Text>hello again, Hello</Text>")
}

/// The drawing, opened in a session. The master gives shape 8 a row of shape data.
fn drawing() -> (Session, Dir) {
    let master =
        MasterSpec::new(1, "Card").shapes(&shape(1, &rows("Property", &[("Owner", cell("Value", "Hello owner")), ("Size", cell("Value", "XL"))])));
    let inherits = shape_with(8, r#" Master="1""#, "<Text>card</Text>");
    let bytes = VsdxBuilder::new()
        .master(master)
        .page(PageSpec::new(1, "One").shapes(&format!("{}{inherits}", page_one())))
        .page(PageSpec::new(2, "Two").shapes(&page_two()))
        .build();
    let dir = Dir::new();
    let path = dir.write("find.vsdx", &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    (s, dir)
}

/// A match as a short word: `1/1 text 0-5`, or `1/6 shape_data[Cost] 0-5`.
fn word(m: &Value) -> String {
    let place = m["place"].as_str().unwrap();
    let row = m.get("row").and_then(Value::as_str).map(|r| format!("[{r}]")).unwrap_or_default();
    format!("{}/{} {place}{row} {}-{}", m["page"], m["id"], m["start"], m["end"])
}

fn words(result: &Value) -> Vec<String> {
    result["matches"].as_array().unwrap().iter().map(word).collect()
}

fn find(s: &mut Session, params: Value) -> Value {
    run(s, "edit.find", params)
}

fn found(s: &mut Session, params: Value) -> Vec<String> {
    words(&find(s, params))
}

/// The words of a command's refusal of its params.
fn refusal(s: &mut Session, id: &str, params: &Value) -> String {
    bad_params_msg(&s.execute(id, params).unwrap_err()).to_owned()
}

fn doc(s: &Session) -> &Document {
    s.document().unwrap()
}

/// The characters of the text of shape `id` of page `page`, as the parts hold them with the fields left out.
fn chars(s: &Session, page: u32, id: u32) -> String {
    diacraft_engine::draw::text_view(doc(s), page, id).unwrap().chars
}

/// The parts of a shape's text as short words.
fn parts(s: &Session, page: u32, id: u32) -> Vec<String> {
    let d = doc(s);
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    d.shape(&at).unwrap().text.as_ref().map_or_else(Vec::new, |t| {
        t.parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) => format!("{c:?}"),
                TextPart::Cp(i) => format!("cp{i}"),
                TextPart::Pp(i) => format!("pp{i}"),
                TextPart::Tp(i) => format!("tp{i}"),
                TextPart::Fld { ix, text } => format!("fld{ix}={text}"),
                _ => "raw".to_owned(),
            })
            .collect()
    })
}

/// A cell's value text and formula text on a shape.
fn cell_of(s: &Session, page: u32, id: u32, section: &str, row: &str) -> (Option<String>, Option<String>) {
    let d = doc(s);
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    match d.shape(&at).unwrap().sheet.lookup(&CellAddr::in_row(section, None, RowKey::name(row), "Value")) {
        Lookup::Found(c) => (c.value.as_ref().map(|v| v.text().to_owned()), c.formula.text().map(str::to_owned)),
        other => panic!("{section} {row}: {other:?}"),
    }
}

fn shape_names(s: &Session, page: u32, id: u32) -> (Option<String>, Option<String>) {
    let d = doc(s);
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    let shape = d.shape(&at).unwrap();
    (shape.name.as_deref().map(str::to_owned), shape.name_u.as_deref().map(str::to_owned))
}

// ---------------------------------------------------------------------------------------------------------------------------- Find

#[test]
fn find_lists_the_matches_of_a_page_in_document_order_with_their_places() {
    let (mut s, _dir) = drawing();
    // Shape 1 holds "Hello world Hello" in two runs: a match is found across the runs' edge too. A group comes before its members.
    assert_eq!(found(&mut s, json!({"what": "hello"})), ["1/1 text 0-5", "1/1 text 12-17"]);
    assert_eq!(
        found(&mut s, json!({"what": "world"})),
        ["1/1 text 6-11", "1/3 text 0-5"],
        "the member of a group is found, in its place after the group"
    );
    assert_eq!(found(&mut s, json!({"what": "o w"})), ["1/1 text 4-7"]);
}

#[test]
fn find_is_a_query_it_is_not_journaled_and_changes_nothing() {
    let (mut s, _dir) = drawing();
    let before = snapshot(&s);
    find(&mut s, json!({"what": "hello", "scope": "all"}));
    assert_eq!(snapshot(&s), before, "a search leaves the document, the history, the revision and the journal as they were");
}

#[test]
fn match_case_and_whole_words_decide_what_is_found_and_for_is_in_for_and_not_in_forest() {
    let (mut s, _dir) = drawing();
    run(&mut s, "shape.set_text", json!({"id": 4, "page": 1, "text": "for forest For"}));
    assert_eq!(
        found(&mut s, json!({"what": "for", "whole_words": true})),
        ["1/4 text 0-3", "1/4 text 11-14"],
        "`for` is not in `forest`, and case does not matter"
    );
    assert_eq!(found(&mut s, json!({"what": "for", "whole_words": true, "match_case": true})), ["1/4 text 0-3"]);
    assert_eq!(found(&mut s, json!({"what": "for"})), ["1/4 text 0-3", "1/4 text 4-7", "1/4 text 11-14"]);
    assert_eq!(found(&mut s, json!({"what": "For", "match_case": true})), ["1/4 text 11-14"]);
}

#[test]
fn the_scope_is_the_selection_the_page_or_every_page() {
    let (mut s, _dir) = drawing();
    assert_eq!(found(&mut s, json!({"what": "world", "scope": "page", "page": 1})), ["1/1 text 6-11", "1/3 text 0-5"]);
    assert_eq!(found(&mut s, json!({"what": "hello", "scope": "page", "page": 2})), ["2/1 text 0-5", "2/1 text 13-18"]);
    assert_eq!(
        found(&mut s, json!({"what": "hello", "scope": "all"})),
        ["1/1 text 0-5", "1/1 text 12-17", "2/1 text 0-5", "2/1 text 13-18"],
        "every page, in the document's order"
    );
    // The selection: a shape selected and the members of a group selected.
    assert_eq!(found(&mut s, json!({"what": "o", "scope": "selection", "page": 1, "ids": [4]})), ["1/4 text 1-2"]);
    assert_eq!(
        found(&mut s, json!({"what": "world", "scope": "selection", "page": 1, "ids": [2]})),
        ["1/3 text 0-5"],
        "a group selected is searched through its members"
    );
    assert_eq!(
        found(&mut s, json!({"what": "world", "scope": "selection", "page": 1, "ids": []})),
        Vec::<String>::new(),
        "nothing selected, nothing found"
    );
    // The page defaults to the one that holds the shapes selected.
    assert_eq!(
        found(&mut s, json!({"what": "again", "scope": "selection", "ids": [1]})),
        Vec::<String>::new(),
        "shape 1 is first on page 1, whose text has no `again`"
    );
}

#[test]
fn shape_text_is_the_default_and_names_data_and_user_cells_are_looked_in_when_asked() {
    let (mut s, _dir) = drawing();
    assert_eq!(found(&mut s, json!({"what": "Hello"})), ["1/1 text 0-5", "1/1 text 12-17"], "only shape text by default");
    assert_eq!(found(&mut s, json!({"what": "Box", "in": ["shape_name"]})), ["1/5 shape_name 6-9"]);
    assert_eq!(
        found(&mut s, json!({"what": "Hello", "in": ["shape_data"], "scope": "all"})),
        ["1/6 shape_data[Cost] 0-5", "1/6 shape_data[Note] 0-5", "1/6 shape_data[Calc] 0-5", "1/8 shape_data[Owner] 0-5"],
        "constants, quoted strings, cells a formula works out, and rows the master gives; a number is not text"
    );
    assert_eq!(found(&mut s, json!({"what": "Hello", "in": ["user_cell"]})), ["1/6 user_cell[Tag] 0-5"]);
    assert_eq!(
        found(&mut s, json!({"what": "100", "in": ["shape_data"]})),
        ["1/6 shape_data[Price] 0-3"],
        "a number is found as its text, and is not replaced as one"
    );
    // Everything at once, in the order name, text, data, user within a shape.
    let all = found(&mut s, json!({"what": "Hello", "in": ["user_cell", "shape_data", "text", "shape_name"], "page": 1}));
    assert_eq!(
        all,
        [
            "1/1 text 0-5",
            "1/1 text 12-17",
            "1/5 shape_name 0-5",
            "1/6 shape_data[Cost] 0-5",
            "1/6 shape_data[Note] 0-5",
            "1/6 shape_data[Calc] 0-5",
            "1/6 user_cell[Tag] 0-5",
            "1/8 shape_data[Owner] 0-5",
        ]
    );
}

#[test]
fn a_field_shows_in_the_text_and_is_found_there() {
    let (mut s, _dir) = drawing();
    assert_eq!(found(&mut s, json!({"what": "Page 3 of"})), ["1/7 text 0-9"], "the text as it shows, a field's result in place");
}

#[test]
fn find_next_goes_on_from_the_place_of_the_last_match_and_goes_round() {
    let (mut s, _dir) = drawing();
    let first = find(&mut s, json!({"what": "hello", "scope": "all", "limit": 1}));
    assert_eq!(words(&first), ["1/1 text 0-5"]);
    assert_eq!(first["truncated"], true, "more matches than were listed");
    assert_eq!(first["wrapped"], false);
    let mut at = first["matches"][0].clone();
    let mut seen = vec![word(&at)];
    for _ in 0..4 {
        let next = find(
            &mut s,
            json!({"what": "hello", "scope": "all", "limit": 1, "after": {"page": at["page"], "id": at["id"], "place": at["place"], "offset": at["end"]}}),
        );
        at = next["matches"][0].clone();
        seen.push(format!("{}{}", word(&at), if next["wrapped"] == true { " (wrapped)" } else { "" }));
    }
    assert_eq!(
        seen,
        ["1/1 text 0-5", "1/1 text 12-17", "2/1 text 0-5", "2/1 text 13-18", "1/1 text 0-5 (wrapped)"],
        "the fifth goes round to the first"
    );
    // Without wrap the list ends at the end of the scope.
    let end = find(&mut s, json!({"what": "hello", "scope": "all", "after": {"page": 2, "id": 1, "place": "text", "offset": 18}, "wrap": false}));
    assert_eq!(words(&end), Vec::<String>::new());
    assert_eq!(end["wrapped"], false);
    // A cursor whose shape is not in the scope is no cursor.
    let lost = find(&mut s, json!({"what": "hello", "page": 1, "after": {"page": 2, "id": 1, "place": "text", "offset": 0}}));
    assert_eq!(words(&lost), ["1/1 text 0-5", "1/1 text 12-17"]);
    assert_eq!(lost["wrapped"], false);
}

#[test]
fn a_search_from_the_middle_of_a_shape_sees_the_matches_after_the_offset_then_those_before_it_on_the_way_round() {
    let (mut s, _dir) = drawing();
    let from = |s: &mut Session, offset: u32| {
        let r = find(s, json!({"what": "Hello", "page": 1, "after": {"page": 1, "id": 1, "place": "text", "offset": offset}}));
        (words(&r), r["wrapped"].clone())
    };
    assert_eq!(from(&mut s, 0), (vec!["1/1 text 0-5".to_owned(), "1/1 text 12-17".to_owned()], json!(false)));
    assert_eq!(
        from(&mut s, 5),
        (vec!["1/1 text 12-17".to_owned(), "1/1 text 0-5".to_owned()], json!(true)),
        "the first match is before the offset: it comes last, round the end"
    );
    assert_eq!(from(&mut s, 17), (vec!["1/1 text 0-5".to_owned(), "1/1 text 12-17".to_owned()], json!(true)), "both come round the end");
}

#[test]
fn the_listing_is_cut_at_the_limit_and_says_so() {
    let (mut s, _dir) = drawing();
    let r = find(&mut s, json!({"what": "o", "scope": "all", "limit": 3}));
    assert_eq!(r["matches"].as_array().unwrap().len(), 3);
    assert_eq!(r["truncated"], true);
    let all = find(&mut s, json!({"what": "Hello", "scope": "all", "limit": 100}));
    assert_eq!(all["truncated"], false);
}

#[test]
fn the_special_characters_find_a_tab_a_line_break_an_optional_hyphen_and_any_character() {
    let (mut s, _dir) = drawing();
    run(&mut s, "shape.set_text", json!({"id": 4, "page": 1, "text": "a\tb c\u{2028}d co\u{ad}op"}));
    assert_eq!(found(&mut s, json!({"what": "a^tb"})), ["1/4 text 0-3"]);
    assert_eq!(found(&mut s, json!({"what": "c^ld"})), ["1/4 text 4-7"]);
    assert_eq!(found(&mut s, json!({"what": "co^-op"})), ["1/4 text 8-13"]);
    assert_eq!(found(&mut s, json!({"what": "^t^?"})), ["1/4 text 1-3"], "a tab then any character");
    assert_eq!(found(&mut s, json!({"what": "^^"})), Vec::<String>::new(), "a caret is a caret");
}

#[test]
fn the_context_of_a_match_is_safe_to_show() {
    let (mut s, _dir) = drawing();
    run(&mut s, "shape.set_text", json!({"id": 4, "page": 1, "text": "line one\nneedle\ttab"}));
    let r = find(&mut s, json!({"what": "needle"}));
    let context = r["matches"][0]["context"].as_str().unwrap();
    assert!(context.contains("needle") && !context.contains('\n') && !context.contains('\t'), "{context:?}");
}

#[test]
fn bad_params_are_refused_in_words_and_leave_the_document_alone() {
    let (mut s, _dir) = drawing();
    let before = snapshot(&s);
    let long = "a".repeat(1_025);
    for (params, words) in [
        (json!({}), "missing `what`"),
        (json!({"what": ""}), "must hold something to find"),
        (json!({"what": long}), "at most 1024 characters"),
        (json!({"what": "x", "scope": "everywhere"}), "`scope` must be selection, page or all"),
        (json!({"what": "x", "scope": "selection"}), "needs `ids`"),
        (json!({"what": "x", "page": 9}), "there is no page 9"),
        (json!({"what": "x", "in": []}), "must name something to look in"),
        (json!({"what": "x", "in": ["notes"]}), "`in` holds text"),
        (json!({"what": "x", "limit": 0}), "`limit` must be a whole number from 1 to 10000"),
        (json!({"what": "x", "limit": 10_001}), "`limit` must be a whole number from 1 to 10000"),
        (json!({"what": "x", "match_case": "yes"}), "`match_case` must be true or false"),
        (json!({"what": "x", "after": {"page": 1, "id": 1}}), "`after` needs `place`"),
        (json!({"what": "x", "after": 3}), "`after` must be an object"),
        (json!({"what": "x", "nothing": 1}), "unknown parameter `nothing`"),
    ] {
        let message = refusal(&mut s, "edit.find", &params);
        assert!(message.contains(words), "{params}: {message}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_find_needs_a_drawing() {
    let mut s = Session::new();
    for (id, params) in [("edit.find", json!({"what": "x"})), ("edit.replace", json!({"what": "x", "with": "y"}))] {
        let message = s.execute(id, &params).unwrap_err().to_string();
        assert!(message.contains("no document open"), "{id}: {message}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------- Replace

fn replaced(s: &mut Session, params: Value) -> Value {
    run(s, "edit.replace", params)
}

#[test]
fn replace_all_replaces_every_match_of_the_page_as_one_undo_step_and_keeps_the_runs() {
    let (mut s, _dir) = drawing();
    let steps = s.history().unwrap().undo_len();
    let r = replaced(&mut s, json!({"what": "hello", "with": "Bye"}));
    assert_eq!((r["replaced"].clone(), r["shapes"].clone(), r["pages"].clone()), (json!(2), json!(1), json!([1])));
    assert_eq!(chars(&s, 1, 1), "Bye world Bye");
    assert_eq!(parts(&s, 1, 1), ["cp0", "\"Bye world\"", "cp1", "\" Bye\""], "each run keeps its marker and its characters outside the match");
    assert_eq!(chars(&s, 2, 1), "hello again, Hello", "the other page is not touched");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step for all of it");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Replace All"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(chars(&s, 1, 1), "Hello world Hello");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(chars(&s, 1, 1), "Bye world Bye");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn replace_across_every_page_and_with_nothing_takes_the_matches_out() {
    let (mut s, _dir) = drawing();
    let r = replaced(&mut s, json!({"what": "hello", "with": "", "scope": "all"}));
    assert_eq!((r["replaced"].clone(), r["shapes"].clone(), r["pages"].clone()), (json!(4), json!(2), json!([1, 2])));
    assert_eq!(chars(&s, 1, 1), " world ");
    assert_eq!(chars(&s, 2, 1), " again, ");
}

#[test]
fn a_match_across_runs_is_replaced_in_the_first_run_and_the_markers_stay() {
    let (mut s, _dir) = drawing();
    // "world Hello" spans the two runs: the new text joins the first, the second keeps its marker and the rest of its characters.
    let r = replaced(&mut s, json!({"what": "d H", "with": "D-H", "page": 1}));
    assert_eq!(r["replaced"], 1);
    assert_eq!(parts(&s, 1, 1), ["cp0", "\"Hello worlD-H\"", "cp1", "\"ello\""]);
}

#[test]
fn a_match_that_a_field_shows_is_left_and_listed_with_why() {
    let (mut s, _dir) = drawing();
    let before = parts(&s, 1, 7);
    let r = replaced(&mut s, json!({"what": "3", "with": "4"}));
    assert_eq!(r["replaced"], 0);
    assert_eq!(r["skipped_total"], 1);
    assert_eq!(r["skipped"][0]["id"], 7);
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("field"), "{}", r["skipped"][0]["why"]);
    assert_eq!(parts(&s, 1, 7), before, "the field is as it was");
    // What is typed round the field is replaced, and the field stays.
    let r = replaced(&mut s, json!({"what": " of 9", "with": " of ten"}));
    assert_eq!(r["replaced"], 1);
    assert_eq!(parts(&s, 1, 7), ["\"Page \"", "fld0=3", "\" of ten\""]);
}

#[test]
fn nothing_to_replace_is_no_undo_step_and_no_change() {
    let (mut s, _dir) = drawing();
    let before = snapshot(&s);
    let r = replaced(&mut s, json!({"what": "zzz", "with": "y"}));
    assert_eq!((r["replaced"].clone(), r["shapes"].clone(), r["pages"].clone()), (json!(0), json!(0), json!([])));
    let after = snapshot(&s);
    assert_eq!(
        (after.document, after.undo_steps, after.revision),
        (before.document, before.undo_steps, before.revision),
        "no step, no revision, no change"
    );
}

#[test]
fn replace_one_match_replaces_that_one_and_says_where_to_go_on_from() {
    let (mut s, _dir) = drawing();
    let r = replaced(&mut s, json!({"what": "Hello", "with": "Goodbye", "at": {"page": 1, "id": 1, "place": "text", "start": 12, "end": 17}}));
    assert_eq!(r["replaced"], 1);
    assert_eq!(chars(&s, 1, 1), "Hello world Goodbye", "the second only");
    assert_eq!(r["next"], json!({"page": 1, "id": 1, "place": "text", "offset": 19}), "after the new text");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Replace"));
    // Find Next from there.
    let next = find(&mut s, json!({"what": "Hello", "scope": "all", "limit": 1, "after": r["next"].clone()}));
    assert_eq!(words(&next), ["2/1 text 0-5"]);
}

#[test]
fn replacing_a_match_that_is_not_there_any_more_is_refused_and_changes_nothing() {
    let (mut s, _dir) = drawing();
    let before = snapshot(&s);
    let result = s.execute("edit.replace", &json!({"what": "Hello", "with": "x", "at": {"page": 1, "id": 1, "place": "text", "start": 1, "end": 6}}));
    let message = result.unwrap_err().to_string();
    assert!(message.contains("no longer a match"), "{message}");
    assert_eq!(snapshot(&s), before);
    // A match a field shows is the error, with why.
    let result = s.execute("edit.replace", &json!({"what": "3", "with": "x", "at": {"page": 1, "id": 7, "place": "text", "start": 5, "end": 6}}));
    let message = result.unwrap_err().to_string();
    assert!(message.contains("field"), "{message}");
}

#[test]
fn replace_in_the_selection_touches_only_the_shapes_selected_and_their_members() {
    let (mut s, _dir) = drawing();
    let r = replaced(&mut s, json!({"what": "world", "with": "earth", "scope": "selection", "page": 1, "ids": [2]}));
    assert_eq!(r["replaced"], 1);
    assert_eq!(chars(&s, 1, 3), "earth peace");
    assert_eq!(chars(&s, 1, 1), "Hello world Hello", "shape 1 was not selected");
}

#[test]
fn replace_in_names_and_shape_data_and_user_cells() {
    let (mut s, _dir) = drawing();
    let r = replaced(&mut s, json!({"what": "Hello", "with": "Bye", "in": ["shape_name", "shape_data", "user_cell"], "page": 1}));
    // The name; the cost and the note (a quoted string stays a quoted string); the tag; and the owner row that shape 8's master gives. The calc cell is worked out
    // by a formula, and is left.
    assert_eq!((r["replaced"].clone(), r["shapes"].clone()), (json!(5), json!(3)));
    assert_eq!(r["skipped_total"], 1);
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("formula"), "{}", r["skipped"][0]["why"]);
    assert_eq!(shape_names(&s, 1, 5), (Some("Bye Box".to_owned()), Some("Bye Box".to_owned())));
    assert_eq!(cell_of(&s, 1, 6, "Property", "Cost"), (Some("Bye cost".to_owned()), None), "a constant stays a constant");
    assert_eq!(cell_of(&s, 1, 6, "Property", "Note"), (Some("Bye note".to_owned()), Some("\"Bye note\"".to_owned())));
    assert_eq!(cell_of(&s, 1, 6, "Property", "Calc").0.as_deref(), Some("Hello calc"), "a computed cell is left");
    assert_eq!(cell_of(&s, 1, 6, "User", "Tag"), (Some("Bye tag".to_owned()), None));
    assert_eq!(chars(&s, 1, 1), "Hello world Hello", "text was not asked for");
    // A row the master gives is written on the shape that holds it, which is then its own.
    assert_eq!(cell_of(&s, 1, 8, "Property", "Owner").0.as_deref(), Some("Bye owner"));
    assert_eq!(found(&mut s, json!({"what": "Price", "in": ["shape_data"]})), Vec::<String>::new(), "only values are searched, not labels");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_cell_that_holds_text_may_not_be_made_a_number_nor_a_name_empty() {
    let (mut s, _dir) = drawing();
    let r = replaced(&mut s, json!({"what": "Hello cost", "with": "42", "in": ["shape_data"], "page": 1}));
    assert_eq!(r["replaced"], 0, "a number would change what the cell is");
    assert_eq!(r["skipped_total"], 1);
    assert_eq!(cell_of(&s, 1, 6, "Property", "Cost").0.as_deref(), Some("Hello cost"));
    // A quoted string is a string whatever it reads as.
    let r = replaced(&mut s, json!({"what": "Hello note", "with": "42", "in": ["shape_data"], "page": 1}));
    assert_eq!(r["replaced"], 1);
    assert_eq!(cell_of(&s, 1, 6, "Property", "Note"), (Some("42".to_owned()), Some("\"42\"".to_owned())));
    // A name may not be taken out altogether.
    let r = replaced(&mut s, json!({"what": "Hello Box", "with": "", "in": ["shape_name"], "page": 1}));
    assert_eq!(r["replaced"], 0);
    assert_eq!(shape_names(&s, 1, 5).0.as_deref(), Some("Hello Box"));
}

#[test]
fn a_cell_that_reads_the_text_is_worked_out_again_after_a_replace() {
    let echo = shape(
        1,
        r#"<Cell N="PinX" V="2"/><Section N="User"><Row N="Echo"><Cell N="Value" V="stale" F="SHAPETEXT(TheText)"/></Row></Section><Text>old words</Text>"#,
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&echo)).build();
    let dir = Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("echo.vsdx", &bytes)}));
    replaced(&mut s, json!({"what": "old", "with": "new"}));
    assert_eq!(cell_of(&s, 1, 1, "User", "Echo").0.as_deref(), Some("new words"));
}

#[test]
fn the_replacement_may_not_hold_a_control_character_and_a_refusal_changes_nothing() {
    let (mut s, _dir) = drawing();
    let before = snapshot(&s);
    for with in ["a\u{1}b", "a\u{fffe}"] {
        let message = refusal(&mut s, "edit.replace", &json!({"what": "Hello", "with": with}));
        assert!(message.contains("control characters"), "{message}");
    }
    let message = refusal(&mut s, "edit.replace", &json!({"what": "Hello"}));
    assert!(message.contains("missing `with`"), "{message}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_text_that_would_be_too_long_is_left_and_the_others_are_replaced() {
    let (mut s, _dir) = drawing();
    let big = "word ".repeat(19_000);
    run(&mut s, "shape.set_text", json!({"id": 4, "page": 1, "text": big}));
    let r = replaced(&mut s, json!({"what": "word", "with": "wordier", "scope": "all"}));
    assert_eq!(r["skipped_total"], 19_000, "all of the long text's matches");
    assert_eq!(r["skipped"].as_array().unwrap().len(), 100, "the list is cut, the count is not");
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("too long"));
    assert_eq!(chars(&s, 1, 4), big, "left as it was");
    assert_eq!(chars(&s, 1, 3), "world peace", "no `word` there");
    // Replacing with something shorter is allowed.
    let r = replaced(&mut s, json!({"what": "word", "with": "w", "scope": "page", "page": 1}));
    assert_eq!(r["replaced"], 19_000);
    assert_eq!(chars(&s, 1, 4), "w ".repeat(19_000));
}

// --------------------------------------------------------------------------------------------------------------------------- Files

/// Replaces in every place of the drawing: text, names, shape data and user-defined cells.
fn replace_everywhere(s: &mut Session) {
    let r = run(s, "edit.replace", json!({"what": "Hello", "with": "Bye", "scope": "all", "in": ["text", "shape_name", "shape_data", "user_cell"]}));
    assert!(r["replaced"].as_u64().unwrap() >= 8, "{r}");
}

/// What `replace_everywhere` made, read from a session.
fn assert_replaced(s: &Session, what: &str) {
    assert_eq!(chars(s, 1, 1), "Bye world Bye", "{what}: the text of two runs");
    assert_eq!(parts(s, 1, 1), ["cp0", "\"Bye world\"", "cp1", "\" Bye\""], "{what}: the runs");
    assert_eq!(chars(s, 2, 1), "Bye again, Bye", "{what}: the other page");
    assert_eq!(shape_names(s, 1, 5).0.as_deref(), Some("Bye Box"), "{what}: the name");
    assert_eq!(cell_of(s, 1, 6, "Property", "Cost").0.as_deref(), Some("Bye cost"), "{what}: shape data");
    assert_eq!(cell_of(s, 1, 6, "Property", "Note"), (Some("Bye note".to_owned()), Some("\"Bye note\"".to_owned())), "{what}: a quoted string");
    assert_eq!(cell_of(s, 1, 6, "User", "Tag").0.as_deref(), Some("Bye tag"), "{what}: a user cell");
    assert_eq!(cell_of(s, 1, 8, "Property", "Owner").0.as_deref(), Some("Bye owner"), "{what}: a row the master gave");
    assert_eq!(cell_of(s, 1, 6, "Property", "Calc").0.as_deref(), Some("Hello calc"), "{what}: a computed cell is as it was");
    assert_eq!(check_session(s), Ok(()), "{what}");
}

fn reopened(path: &str) -> Session {
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": path }));
    again
}

/// A replacement is in the file that is saved, and in the drawing read from it: a `.vsdx` is saved in place by the splice, and its text, names and cells come back.
#[test]
fn a_replacement_in_a_vsdx_is_saved_in_place_and_read_again() {
    let (mut s, dir) = drawing();
    replace_everywhere(&mut s);
    let saved = run(&mut s, "file.save", json!({}));
    assert!(saved["parts"]["rewritten"].as_u64().unwrap() >= 1, "the pages that changed are rewritten: {saved}");
    assert_replaced(&reopened(&dir.path("find.vsdx")), "vsdx");
}

/// The same drawing saved as `.diacraft`, the format's own, and read again.
#[test]
fn a_replacement_in_a_vsdx_is_saved_as_diacraft_and_read_again() {
    let (mut s, dir) = drawing();
    replace_everywhere(&mut s);
    let path = dir.path("out.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_replaced(&reopened(&path), "diacraft");
}

/// A drawing that came from an `.odg`: its text and its shape names are replaced and written by the splice. (A drawing from a VSDX is not saved as an `.odg` yet: saving
/// across the two formats is not written, and `file.save_as` says so.)
#[test]
fn a_replacement_in_an_odg_is_saved_and_read_again() {
    use crate::odg_build::{Drawing, names_of, open_drawing, rect};
    let dir = Dir::new();
    let drawing =
        Drawing::new().page(&[rect("Hello A", (2.0, 3.0, 4.0, 2.0), "Hello world"), rect("B", (12.0, 3.0, 4.0, 2.0), "say hello")].concat());
    let mut s = open_drawing(&dir, &drawing);
    let r = run(&mut s, "edit.replace", json!({"what": "hello", "with": "Bye", "scope": "all", "in": ["text", "shape_name"]}));
    assert_eq!(r["replaced"], 3, "{r}");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "{saved}");
    let again = reopened(&dir.path("a.odg"));
    assert_eq!(crate::odg_file::text(&again, 0, "Bye A"), "Bye world");
    assert_eq!(crate::odg_file::text(&again, 0, "B"), "say Bye");
    assert_eq!(names_of(again.document().unwrap(), 0), ["Bye A", "B"]);
    assert_eq!(check_session(&again), Ok(()));
}

/// A new drawing, saved as a `.vsdx` by the model writer, and read again.
#[test]
fn a_replacement_in_a_new_drawing_is_saved_as_a_vsdx_by_the_model_writer_and_read_again() {
    let dir = Dir::new();
    let mut s = crate::blank_session();
    let page = doc(&s).pages.get(0).unwrap().id;
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 3.0, "y": 3.0}))["id"].as_u64().unwrap();
    run(&mut s, "shape.set_text", json!({"id": a, "page": page, "text": "Hello there"}));
    let r = run(&mut s, "edit.replace", json!({"what": "Hello", "with": "Bye", "scope": "all", "in": ["text", "shape_name"]}));
    assert_eq!(r["replaced"], 1);
    let path = dir.path("new.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let again = reopened(&path);
    let page = doc(&again).pages.get(0).unwrap().id;
    assert_eq!(chars(&again, page, a as u32), "Bye there");
    assert_eq!(check_session(&again), Ok(()));
}
