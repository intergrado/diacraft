//! `stencil.search`, the shape search of the Shapes window.
//!
//! The words are whole words, in any case, of a master's name and its search words (`ShapeKeywords`); every word must be found (AND, the default) or any one of them
//! (OR, `mode: "any"`). The stencils searched are the built-in ones, open or not, and the stencil files that are open. Every stencil package is built in code.

use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
use serde_json::{Value, json};

use crate::stencil_open::{CT_STENCIL, Dir, box_cells, ids, open};
use crate::{blank_session, check_session, run, snapshot};

/// A master of a stencil file: a box, with `keywords` in its page sheet's `ShapeKeywords` cell when given (as a `V`, or as the element's text).
fn master(id: u32, name: &str, keywords: Option<&str>, as_text: bool) -> MasterSpec {
    let mut spec = MasterSpec::new(id, name).attrs(&ids(id, id + 1_000_000)).shapes(&shape(5, &box_cells(1.0, 1.0)));
    if let Some(words) = keywords {
        spec.sheet =
            Some(if as_text { format!(r#"<Cell N="ShapeKeywords">{words}</Cell>"#) } else { format!(r#"<Cell N="ShapeKeywords" V="{words}"/>"#) });
    }
    spec
}

/// A stencil file holding the masters; opened in `s`, its id is returned.
fn stencil_with(s: &mut diacraft_engine::Session, dir: &Dir, file: &str, masters: Vec<MasterSpec>) -> String {
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for m in masters {
        builder = builder.master(m);
    }
    let path = dir.write(file, &builder.build());
    open(s, &path)["id"].as_str().unwrap().to_owned()
}

/// The names of the masters a search found, with the stencil each is under, in the order the answer gives them.
fn found(answer: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for group in answer["results"].as_array().unwrap() {
        for m in group["masters"].as_array().unwrap() {
            out.push((group["name"].as_str().unwrap().to_owned(), m["name"].as_str().unwrap().to_owned()));
        }
    }
    out
}

fn names(answer: &Value) -> Vec<String> {
    found(answer).into_iter().map(|(_, m)| m).collect()
}

fn refusal(s: &mut diacraft_engine::Session, params: Value) -> String {
    let before = snapshot(s);
    let e = s.execute("stencil.search", &params).unwrap_err().to_string();
    assert_eq!(snapshot(s), before, "a refusal changes nothing");
    e
}

/// "Air Cond" with AND does not find "Air Conditioner"; with OR it is found along with "Air Vent" through "Air".
#[test]
fn all_of_the_words_is_the_default_and_a_word_is_a_whole_word() {
    let dir = Dir::new();
    let mut s = blank_session();
    stencil_with(&mut s, &dir, "air.vssx", vec![master(1, "Air Conditioner", None, false), master(2, "Air Vent", None, false)]);
    let and = run(&mut s, "stencil.search", json!({"text": "Air Cond"}));
    assert_eq!(and["mode"], "all");
    assert_eq!(and["matches"], 0, "{and}");
    assert!(names(&and).is_empty());
    let explicit = run(&mut s, "stencil.search", json!({"text": "Air Cond", "mode": "all"}));
    assert_eq!(explicit["matches"], 0);
    let or = run(&mut s, "stencil.search", json!({"text": "Air Cond", "mode": "any"}));
    assert_eq!(or["mode"], "any");
    assert_eq!(names(&or), ["Air Conditioner", "Air Vent"], "{or}");
    // A whole word is found in the name, and every word of an AND must be there.
    let both = run(&mut s, "stencil.search", json!({"text": "conditioner air"}));
    assert_eq!(names(&both), ["Air Conditioner"], "the order of the words does not matter");
    let one = run(&mut s, "stencil.search", json!({"text": "Vent"}));
    assert_eq!(names(&one), ["Air Vent"]);
}

#[test]
fn the_search_ignores_case_and_punctuation_and_the_results_are_grouped_by_stencil() {
    let dir = Dir::new();
    let mut s = blank_session();
    stencil_with(&mut s, &dir, "a.vssx", vec![master(1, "Pump-House", None, false), master(2, "Valve", None, false)]);
    stencil_with(&mut s, &dir, "b.vssx", vec![master(1, "HOUSE of Cards", None, false)]);
    let v = run(&mut s, "stencil.search", json!({"text": "house"}));
    assert_eq!(found(&v), [("a".to_owned(), "Pump-House".to_owned()), ("b".to_owned(), "HOUSE of Cards".to_owned())], "{v}");
    let groups = v["results"].as_array().unwrap();
    assert_eq!((groups[0]["id"].as_str(), groups[0]["builtin"].as_bool(), groups[0]["open"].as_bool()), (Some("file-1"), Some(false), Some(true)));
    assert_eq!(v["matches"], 2);
    assert_eq!(v["listed"], 2);
    assert_eq!(v["complete"], true);
    // A master is the entry `stencil.list` gives, so a drop takes its `id`.
    let m = &groups[0]["masters"][0];
    assert!(m["id"].is_string() && m["width"].is_number() && m["height"].is_number(), "{m}");
}

#[test]
fn a_master_is_found_by_its_search_words_in_either_form_of_the_cell() {
    let dir = Dir::new();
    let mut s = blank_session();
    stencil_with(
        &mut s,
        &dir,
        "k.vssx",
        vec![
            master(1, "Unit One", Some("chiller cooling"), false),
            master(2, "Unit Two", Some("boiler; heating"), true),
            master(3, "Unit Three", None, false),
        ],
    );
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "chiller"}))), ["Unit One"]);
    assert_eq!(
        names(&run(&mut s, "stencil.search", json!({"text": "Heating boiler"}))),
        ["Unit Two"],
        "the element's text is read too, and `;` parts words"
    );
    // A word of the name and a word of the search words together, for the AND.
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "two heating"}))), ["Unit Two"]);
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "unit"}))), ["Unit One", "Unit Two", "Unit Three"]);
    assert_eq!(run(&mut s, "stencil.search", json!({"text": "cooling heating"}))["matches"], 0, "no master has both");
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "cooling heating", "mode": "any"}))), ["Unit One", "Unit Two"]);
}

#[test]
fn a_hidden_master_is_not_found() {
    let dir = Dir::new();
    let mut s = blank_session();
    let hidden = MasterSpec::new(2, "Secret Pump").attrs(&format!(r#"{} Hidden="1""#, ids(2, 3))).shapes(&shape(5, &box_cells(1.0, 1.0)));
    stencil_with(&mut s, &dir, "h.vssx", vec![master(1, "Open Pump", None, false), hidden]);
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "pump"}))), ["Open Pump"]);
}

#[test]
fn the_built_in_stencils_are_searched_by_name_and_by_their_words_open_or_not() {
    let mut s = blank_session();
    // By name: the whole word, and not a part of one.
    let v = run(&mut s, "stencil.search", json!({"text": "diamond"}));
    let all = found(&v);
    assert!(all.contains(&("Basic Shapes".to_owned(), "Diamond".to_owned())), "{all:?}");
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "rect"}))), Vec::<String>::new(), "a part of a word is not a word");
    assert!(names(&run(&mut s, "stencil.search", json!({"text": "rectangle"}))).contains(&"Rectangle".to_owned()));
    // By the words the stencil gives its master: a rhombus is a diamond, a branch is a decision.
    let rhombus = run(&mut s, "stencil.search", json!({"text": "Rhombus"}));
    assert!(found(&rhombus).contains(&("Basic Shapes".to_owned(), "Diamond".to_owned())), "{rhombus}");
    let branch = run(&mut s, "stencil.search", json!({"text": "branch"}));
    assert!(found(&branch).contains(&("Basic Flowchart".to_owned(), "Decision".to_owned())), "{branch}");
    // A built-in stencil that a template left closed is searched too, and the answer says it is not open.
    run(&mut s, "file.new", json!({ "template": "Basic Diagram" }));
    let process = run(&mut s, "stencil.search", json!({"text": "process"}));
    let groups: Vec<(String, bool)> =
        process["results"].as_array().unwrap().iter().map(|g| (g["name"].as_str().unwrap().to_owned(), g["open"].as_bool().unwrap())).collect();
    assert!(groups.contains(&("Basic Flowchart".to_owned(), false)), "{groups:?}");
    let rect = run(&mut s, "stencil.search", json!({"text": "rectangle"}));
    assert!(rect["results"].as_array().unwrap().iter().any(|g| g["name"] == "Basic Shapes" && g["open"] == true), "{rect}");
}

#[test]
fn a_stencil_that_is_closed_is_not_searched() {
    let dir = Dir::new();
    let mut s = blank_session();
    let id = stencil_with(&mut s, &dir, "c.vssx", vec![master(1, "Quartz Crystal", None, false)]);
    assert_eq!(names(&run(&mut s, "stencil.search", json!({"text": "quartz"}))), ["Quartz Crystal"]);
    run(&mut s, "stencil.close", json!({ "id": id }));
    assert_eq!(run(&mut s, "stencil.search", json!({"text": "quartz"}))["matches"], 0);
}

#[test]
fn a_search_is_no_edit_and_runs_without_a_document() {
    let mut s = diacraft_engine::Session::new();
    let v = run(&mut s, "stencil.search", json!({"text": "arrow"}));
    assert!(v["matches"].as_u64().unwrap() >= 4, "{v}");
    let mut s = blank_session();
    let before = snapshot(&s);
    run(&mut s, "stencil.search", json!({"text": "arrow"}));
    assert_eq!(snapshot(&s), before);
    assert!(!s.is_dirty());
    assert_eq!(s.history().unwrap().undo_len(), 0, "a query is no undo step");
    check_session(&s).unwrap();
}

#[test]
fn what_is_not_a_search_is_refused_in_words_and_nothing_changes() {
    let mut s = blank_session();
    for (params, words) in [
        (json!({}), "missing `text`"),
        (json!({"text": 12}), "`text` must be text"),
        (json!({"text": "   "}), "no words"),
        (json!({"text": "!!! ???"}), "no words"),
        (json!({"text": "a", "mode": "some"}), "`mode` must be `all` or `any`"),
        (json!({"text": "a", "mode": 3}), "`mode` must be text"),
        (json!({"text": (0..40).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ")}), "at most 16 different words"),
        (json!({"text": ["a"]}), "`text` must be text"),
        (json!({"text": "x".repeat(65)}), "at most 64 characters"),
        (json!({"text": "word ".repeat(60)}), "at most 256 characters"),
    ] {
        let e = refusal(&mut s, params.clone());
        assert!(e.contains(words), "{params}: {e}");
    }
    // The same word again is one word, and sixteen different words are the most.
    let sixteen: Vec<String> = (0..16).map(|i| format!("w{i}")).collect();
    run(&mut s, "stencil.search", json!({"text": sixteen.join(" ")}));
    let seventeen: Vec<String> = (0..17).map(|i| format!("w{i}")).collect();
    assert!(refusal(&mut s, json!({"text": seventeen.join(" ")})).contains("at most 16 different words"));
    run(&mut s, "stencil.search", json!({"text": "same same same same same same same same same same same same same same same same same same"}));
}

/// More than the most listed: `matches` says how many there are, `listed` how many are in the answer, and the answer says so in words.
#[test]
fn a_search_lists_so_many_and_counts_them_all() {
    let dir = Dir::new();
    let mut s = blank_session();
    let masters: Vec<MasterSpec> = (1..=700).map(|i| master(i, &format!("Pump {i}"), None, false)).collect();
    stencil_with(&mut s, &dir, "many.vssx", masters);
    let v = run(&mut s, "stencil.search", json!({"text": "pump"}));
    assert_eq!((v["matches"].as_u64(), v["listed"].as_u64()), (Some(700), Some(500)), "{}", v["matches"]);
    assert_eq!(names(&v).len(), 500);
    assert_eq!(v["truncated"], true);
    let notes = v["notes"].as_array().unwrap();
    assert!(notes.iter().any(|n| n.as_str().unwrap().contains("700 masters match") && n.as_str().unwrap().contains("first 500")), "{notes:?}");
    // The listed ones are the first in the stencil's order, and each drops by its `id`.
    assert_eq!(names(&v)[0], "Pump 1");
    // A word that one master has is found past the listed part.
    let one = run(&mut s, "stencil.search", json!({"text": "699"}));
    assert_eq!(names(&one), ["Pump 699"]);
}
