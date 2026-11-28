//! The feature comparison page: the data is read and checked (`compare::build`), the DiaCraft column is computed from the checklist, the regions of the page are made from them, and the
//! page in the repository is what the data and the checklist give.

use std::collections::BTreeMap;

use super::compare::{self, Comparison, PAGE, Value};
use super::settings::Settings;

const CHECKLIST: &str = "# Parity checklist\n\n> **Last reviewed:** 2026-11-22\n\n<!-- SUMMARY -->\nsummary\n\n## Rows\n\n| Area | Feature | Priority | Status |\n|---|---|---|---|\n\
| A | a | P0 | D |\n| A | b | P1 | D |\n| A | c | P1 | P |\n| A | d | P2 | M |\n| B | e | P2 | M |\n| B | [f](ui-parity.md#f) | P2 | M |\n";

/// A cell of LibreOffice or draw.io, and the rest of a row that does not change.
fn cells(lo: &str, drawio: &str) -> String {
    format!(
        "reference = {{ value = \"Full\", edition = \"desktop\", source = \"inventory\", note = \"\" }}\n\
         [row.libreoffice]\nvalue = \"{lo}\"\nbasis = \"documented\"\nsources = [\"https://help.libreoffice.org/latest/en-US/text/shared/02/18010000.html\"]\nread = \"2026-11-26\"\nnote = \"some of it\"\n\
         [row.drawio]\nvalue = \"{drawio}\"\nbasis = \"documented\"\nsources = [\"https://www.drawio.com/docs/features/\"]\nread = \"2026-11-26\"\nnote = \"some of it\"\n"
    )
}

fn only(id: &str, ships: bool, lacks: [&str; 3]) -> String {
    let cell = |name: &str, lacks: &str| {
        format!(
            "[only.{name}]\nlacks = \"{lacks}\"\nbasis = \"documented\"\nsources = [\"https://www.drawio.com/docs/features/\"]\nread = \"2026-11-26\"\nnote = \"a note\"\n"
        )
    };
    format!(
        "[[only]]\nid = \"{id}\"\nfeature = \"A feature\"\nships = {ships}\ndiacraft_sources = [\"docs/x.md#part\"]\n{}{}{}",
        cell("reference", lacks[0]),
        cell("libreoffice", lacks[1]),
        cell("drawio", lacks[2])
    )
}

fn matrix_with(rows: &str, extra: &str) -> String {
    format!(
        "version = 1\nchecklist = \"docs/parity-checklist.md\"\nvalues = [\"Full\", \"Partial\", \"Not yet\", \"unknown\"]\n\n\
         [sources]\ninventory = \"the checklist rows: features of the reference app's desktop edition, read 2026-10-31\"\nabsent = \"not found in the documentation read\"\n\n\
         [[area]]\nkey = \"first\"\nlabel = \"First area\"\n\n[[area]]\nkey = \"second\"\nlabel = \"Second area\"\n\n{rows}\n{extra}"
    )
}

/// Four rows: [a, b] is done, [c, d] is in part, [e, f] is missing, and the last names no checklist row.
fn rows() -> String {
    let row = |area: &str, id: &str, names: &str, rule: &str| {
        format!(
            "[[row]]\narea = \"{area}\"\nid = \"{id}\"\nfeature = \"Feature {id}\"\nchecklist = [{names}]\ndiacraft = {rule}\n{}",
            cells("Partial", "Not yet")
        )
    };
    [
        row("first", "all-done", "\"a\", \"b\"", "{ rule = \"checklist\" }"),
        row("first", "some-done", "\"c\", \"d\"", "{ rule = \"checklist\" }"),
        row("second", "none-done", "\"e\", \"f\"", "{ rule = \"checklist\" }"),
        row("second", "by-hand", "", "{ rule = \"manual\", value = \"Partial\", source = \"docs/x.md\", note = \"read from the doc\" }"),
    ]
    .join("\n")
}

fn exists(path: &str) -> bool {
    path == "docs/x.md"
}

fn matrix() -> String {
    matrix_with(&rows(), &only("qualifies", true, ["yes", "yes", "yes"]))
}

fn build(matrix: &str) -> Result<Comparison, String> {
    compare::build(matrix, CHECKLIST, &exists)
}

/// The text of `matrix` with `old` made `new`; `old` must be there.
fn with(matrix: &str, old: &str, new: &str) -> String {
    assert!(matrix.contains(old), "{old:?} is not in the data");
    matrix.replacen(old, new, 1)
}

fn settings(comparison: Option<Comparison>) -> Settings {
    Settings {
        domain: "https://example.invalid".into(),
        app_url: "https://app.example.invalid".into(),
        repository: "owner/repo".into(),
        assets: BTreeMap::new(),
        version: "9.9.9".into(),
        licences: Vec::new(),
        comparison: comparison.map(std::sync::Arc::new),
    }
}

#[test]
fn a_row_is_full_if_every_checklist_row_is_done_not_yet_if_every_one_is_missing_and_partial_otherwise() {
    let data = build(&matrix()).unwrap();
    // Counts of (Full, Partial, Not yet, unknown) for the reference app, draw.io, LibreOffice and DiaCraft: DiaCraft is Full once (a, b), Partial twice (c and d, and the manual row) and Not yet once.
    let [reference, drawio, libreoffice, ours] = data.counts();
    assert_eq!(ours, [1, 2, 1, 0]);
    assert_eq!(reference, [4, 0, 0, 0]);
    assert_eq!(drawio, [0, 0, 4, 0]);
    assert_eq!(libreoffice, [0, 4, 0, 0]);
    assert!(data.line().contains("DiaCraft is Full in 1, Partial in 2 and Not yet in 1"), "{}", data.line());
}

#[test]
fn a_change_of_a_score_moves_the_cell_and_a_row_that_is_partly_done_is_partial_whatever_the_rest() {
    // `a` and `b` are done: all done. Make `b` partly done and the row is Partial; make both missing and it is Not yet.
    for (b, a, expected) in [("D", "D", [1, 2, 1, 0]), ("P", "D", [0, 3, 1, 0]), ("M", "D", [0, 3, 1, 0]), ("M", "M", [0, 2, 2, 0])] {
        let checklist =
            CHECKLIST.replace("| A | b | P1 | D |", &format!("| A | b | P1 | {b} |")).replace("| A | a | P0 | D |", &format!("| A | a | P0 | {a} |"));
        let data = compare::build(&matrix(), &checklist, &exists).unwrap();
        assert_eq!(data.counts()[3], expected, "a is {a}, b is {b}");
    }
}

#[test]
fn a_markdown_link_in_a_checklist_name_counts_as_its_text() {
    // `[f](ui-parity.md#f)` is named `f`: the row names it and the check passes. A name that is the whole link is not a name.
    build(&matrix()).unwrap();
    let error = build(&with(&matrix(), "\"e\", \"f\"", "\"e\", \"[f](ui-parity.md#f)\"")).unwrap_err();
    assert!(error.contains("[f](ui-parity.md#f)") && error.contains("no row named"), "{error}");
}

#[test]
fn every_checklist_row_is_in_exactly_one_row_and_a_name_the_checklist_does_not_have_is_an_error() {
    let error = build(&with(&matrix(), "\"e\", \"f\"", "\"e\"")).unwrap_err();
    assert!(error.contains("no row of the comparison names") && error.contains("f"), "{error}");
    let error = build(&with(&matrix(), "\"e\", \"f\"", "\"e\", \"f\", \"a\"")).unwrap_err();
    assert!(error.contains("`a` is in two rows"), "{error}");
    let error = build(&with(&matrix(), "\"e\", \"f\"", "\"e\", \"f\", \"nothing\"")).unwrap_err();
    assert!(error.contains("no row named `nothing`"), "{error}");
    // Two checklist rows with one name cannot be told apart.
    let error = compare::build(&matrix(), &format!("{CHECKLIST}| B | e | P2 | D |\n"), &exists).unwrap_err();
    assert!(error.contains("two rows are named `e`"), "{error}");
    // A row of the comparison that computes from the checklist names a row of it.
    let error = build(&with(&matrix(), "\"c\", \"d\"", "")).unwrap_err();
    assert!(error.contains("at least one checklist row"), "{error}");
}

#[test]
fn a_row_with_a_manual_value_names_no_checklist_row_and_a_file_of_the_docs() {
    let error = build(&with(&matrix(), "source = \"docs/x.md\", note", "source = \"docs/missing.md\", note")).unwrap_err();
    assert!(error.contains("docs/missing.md") && error.contains("Markdown file of docs/"), "{error}");
    for bad in ["../x.md", "docs/../x.md", "docs/x.txt", "x.md", "docs/a b.md"] {
        let error = build(&with(&matrix(), "source = \"docs/x.md\", note", &format!("source = \"{bad}\", note"))).unwrap_err();
        assert!(error.contains("not a Markdown file of docs/"), "{bad}: {error}");
    }
    let error = build(&with(&matrix(), "{ rule = \"manual\", value = \"Partial\"", "{ rule = \"manual\", value = \"unknown\"")).unwrap_err();
    assert!(error.contains("Full, Partial or Not yet"), "{error}");
    let error = build(&with(&matrix(), "{ rule = \"checklist\" }", "{ rule = \"guess\" }")).unwrap_err();
    assert!(error.contains("`rule` is `guess`"), "{error}");
}

#[test]
fn the_cells_of_the_other_tools_are_held_to_what_the_data_says_they_rest_on() {
    // An absent cell says Not yet; an unknown cell has no basis, and a basis that is unknown has no value.
    let absent = with(&matrix(), "[row.drawio]\nvalue = \"Not yet\"\nbasis = \"documented\"", "[row.drawio]\nvalue = \"Full\"\nbasis = \"absent\"");
    assert!(build(&absent).unwrap_err().contains("whose basis is absent says Not yet"));
    let unknown =
        with(&matrix(), "[row.drawio]\nvalue = \"Not yet\"\nbasis = \"documented\"", "[row.drawio]\nvalue = \"unknown\"\nbasis = \"documented\"");
    assert!(build(&unknown).unwrap_err().contains("unknown if and only if its basis is unknown"));
    let bare = with(&matrix(), "sources = [\"https://www.drawio.com/docs/features/\"]", "sources = []");
    assert!(build(&bare).unwrap_err().contains("names its sources"));
    // A cell that says nothing, because no source settles it, may name none.
    let nothing = with(&bare, "value = \"Not yet\"\nbasis = \"documented\"", "value = \"unknown\"\nbasis = \"unknown\"");
    assert_eq!(build(&nothing).unwrap().counts()[1], [0, 0, 3, 1]);
    for (old, new, said) in [
        ("read = \"2026-11-26\"\nnote = \"some of it\"\n[row.drawio]", "read = \"2026-13-26\"\nnote = \"some of it\"\n[row.drawio]", "not a day"),
        ("read = \"2026-11-26\"\nnote = \"some of it\"\n[row.drawio]", "read = \"26 November\"\nnote = \"some of it\"\n[row.drawio]", "not a day"),
        ("basis = \"documented\"\nsources = [\"https://help", "basis = \"known\"\nsources = [\"https://help", "`basis` is `known`"),
        ("value = \"Partial\"\nbasis", "value = \"Some\"\nbasis", "Full, Partial, Not yet or unknown"),
        ("edition = \"desktop\"", "edition = \"cloud\"", "`edition` is `cloud`"),
        ("source = \"inventory\"", "source = \"nowhere\"", "no key of [sources]"),
        ("value = \"Full\", edition", "value = \"unknown\", edition", "Full, Partial or Not yet"),
    ] {
        let error = build(&with(&matrix(), old, new)).unwrap_err();
        assert!(error.contains(said), "{new}: {error}");
    }
}

#[test]
fn a_source_is_an_address_on_a_host_the_page_may_link_a_sentence_of_the_data_or_an_oracle_reading() {
    let source = |text: &str| {
        with(&matrix(), "sources = [\"https://help.libreoffice.org/latest/en-US/text/shared/02/18010000.html\"]", &format!("sources = [{text}]"))
    };
    for good in [
        "\"https://help.libreoffice.org/latest/en-US/text/shared/02/18010000.html\"",
        "\"https://wiki.documentfoundation.org/index.php?oldid=886199\"",
        "\"https://www.drawio.com/docs/features/\"",
        "\"https://github.com/owner/repo/blob/abc/file.js#fn\"",
        "\"oracle: LibreOffice 26.8.2.2 run as a black box\"",
        "\"inventory\"",
    ] {
        build(&source(good)).unwrap_or_else(|e| panic!("{good}: {e}"));
    }
    for bad in [
        "\"http://www.drawio.com/docs/\"",
        "\"https://example.com/docs/\"",
        "\"https://www.drawio.com.evil.example/docs/\"",
        "\"https://www.drawio.com@evil.example/docs/\"",
        "\"https://www.drawio.com/a b\"",
        "\"https://www.drawio.com/a\\\"b\"",
        "\"https://www.drawio.com/<x>\"",
        "\"a page I read\"",
        "\"Oracle: x\"",
        "7",
    ] {
        assert!(build(&source(bad)).is_err(), "{bad} was accepted");
    }
}

#[test]
fn ids_areas_values_and_sizes_are_checked() {
    for (old, new, said) in [
        ("id = \"all-done\"", "id = \"All Done\"", "lower-case letters"),
        ("id = \"all-done\"", "id = \"some-done\"", "lower-case letters"),
        ("area = \"first\"\nid", "area = \"third\"\nid", "not one of the [[area]]"),
        ("key = \"second\"", "key = \"first\"", "no other has"),
        ("version = 1", "version = 2", "version = 1"),
        ("values = [\"Full\", \"Partial\", \"Not yet\", \"unknown\"]", "values = [\"Full\", \"Partial\"]", "in that order"),
        ("feature = \"Feature all-done\"", "feature = \"\"", "a feature name"),
        ("feature = \"Feature all-done\"", "feature = \"A\\u0007B\"", "control character"),
    ] {
        let error = build(&with(&matrix(), old, new)).unwrap_err();
        assert!(error.contains(said), "{new}: {error}");
    }
    // An area with no row has nothing to show.
    let error = build(&matrix_with(&rows().replace("area = \"second\"", "area = \"first\""), "")).unwrap_err();
    assert!(error.contains("an area has at least one row"), "{error}");
    // A string of the data is cut at a size: a damaged file is refused, not rendered.
    let long = "x".repeat(4_001);
    let error = build(&with(&matrix(), "feature = \"Feature all-done\"", &format!("feature = \"{long}\""))).unwrap_err();
    assert!(error.contains("4000 characters"), "{error}");
    assert!(build("this is not toml = ").is_err());
}

#[test]
fn only_a_feature_that_ships_and_that_every_tool_lacks_is_listed() {
    let listed = |items: &[(&str, bool, [&str; 3])]| {
        let extra: String = items.iter().map(|(id, ships, lacks)| only(id, *ships, *lacks)).collect::<Vec<_>>().join("\n");
        let data = build(&matrix_with(&rows(), &extra)).unwrap();
        let page = compare::render(&template(), &data, &settings(None)).unwrap();
        items.iter().filter(|(id, ..)| page.contains(&format!("<li id=\"{id}\">"))).map(|(id, ..)| (*id).to_owned()).collect::<Vec<_>>()
    };
    let items = [
        ("all-lack", true, ["yes", "yes", "yes"]),
        ("not-built", false, ["yes", "yes", "yes"]),
        ("one-has-it", true, ["yes", "no", "yes"]),
        ("one-unknown", true, ["yes", "yes", "unknown"]),
        ("reference-has-it", true, ["no", "yes", "yes"]),
        ("also-all-lack", true, ["yes", "yes", "yes"]),
    ];
    assert_eq!(listed(&items), ["all-lack", "also-all-lack"]);
    assert!(listed(&[]).is_empty());
    // An item with a bad cell is an error even if it would not be listed.
    let error = build(&matrix_with(&rows(), &only("odd", false, ["maybe", "yes", "yes"]))).unwrap_err();
    assert!(error.contains("`lacks` is `maybe`"), "{error}");
}

/// A page with the six regions, as the real page has them, and a line of its own around each.
fn template() -> String {
    let mut page = String::from("<main>\n");
    for name in ["facts", "asof", "editions", "toc", "areas", "only"] {
        page.push_str(&format!("  <!-- {name}:begin -->\n  <!-- {name}:end -->\n"));
    }
    page.push_str("</main>\n");
    page
}

#[test]
fn the_regions_are_made_again_and_the_same_page_is_made_however_often() {
    let data = build(&matrix()).unwrap();
    let settings = settings(Some(data.clone()));
    let page = compare::render(&template(), &data, &settings).unwrap();
    assert_eq!(compare::render(&page, &data, &settings).unwrap(), page, "a made page is made the same again");
    // Text a person wrote around the regions is kept, and what was inside a region is replaced.
    let mut edited = page.replace("<main>", "<main>\n  <p>kept</p>").replace("<b>1</b>", "<b>stale</b>");
    edited = edited.replace("Full</span>", "Not yet</span>");
    let made = compare::render(&edited, &data, &settings).unwrap();
    assert_eq!(made, page.replace("<main>", "<main>\n  <p>kept</p>"));
    for text in ["First area", "Second area", "Feature all-done", "Feature by-hand", "href=\"#area-first\"", "id=\"area-second\""] {
        assert!(page.contains(text), "{text}");
    }
    // Every row is a row of a table with the tools in their order, and each area has one table.
    assert_eq!(page.matches("<table ").count(), 2);
    assert_eq!(page.matches("<tbody").count(), 2);
    assert_eq!(page.matches("scope=\"row\"").count(), 4);
    assert_eq!(page.matches("scope=\"col\"").count(), 2 * 5);
}

#[test]
fn a_page_with_a_missing_doubled_or_misplaced_marker_is_refused_and_not_guessed_at() {
    let data = build(&matrix()).unwrap();
    let settings = settings(Some(data.clone()));
    let render = |page: &str| compare::render(page, &data, &settings);
    assert!(render(&template().replace("  <!-- toc:begin -->\n", "")).unwrap_err().contains("no `<!-- toc:begin -->` line"));
    assert!(render(&template().replace("  <!-- toc:end -->\n", "")).unwrap_err().contains("no `<!-- toc:end -->` line"));
    assert!(render(&template().replace("<main>", "<main>\n<!-- toc:begin -->")).unwrap_err().contains("twice"));
    let swapped = template()
        .replace("<!-- areas:begin -->", "<!-- areas:TMP -->")
        .replace("<!-- areas:end -->", "<!-- areas:begin -->")
        .replace("<!-- areas:TMP -->", "<!-- areas:end -->");
    assert!(render(&swapped).unwrap_err().contains("comes before"));
    assert!(render(&template().replace("  <!-- facts:begin -->", "x <!-- facts:begin -->")).unwrap_err().contains("not at the start of a line"));
    assert!(render("<main>\n  <!-- facts:begin -->").is_err());
}

/// What the page's line of dates says between its markers.
fn asof_of(page: &str) -> String {
    page.split("<!-- asof:begin -->").nth(1).and_then(|rest| rest.split("<!-- asof:end -->").next()).unwrap().to_owned()
}

#[test]
fn the_line_of_dates_says_where_the_cells_come_from_and_names_a_black_box_only_when_a_cell_rests_on_one() {
    // Every cell documented: the line says public documentation and nothing else, and never that the tools were not run.
    let page = compare::render(&template(), &build(&matrix()).unwrap(), &settings(None)).unwrap();
    let line = asof_of(&page);
    assert!(line.contains("Every cell comes from public documentation: the pages of"), "{line}");
    assert!(!line.contains("black box") && !line.contains("running"), "{line}");
    // A cell of LibreOffice Draw that rests on a copy of it run as a black box, in a row or in the list of what only DiaCraft has, is named.
    let says = "Every cell comes from public documentation or, for some of LibreOffice Draw's, from a copy of it run as a black box: the pages of";
    let in_a_row = with(
        &matrix(),
        "basis = \"documented\"\nsources = [\"https://help.libreoffice.org",
        "basis = \"oracle\"\nsources = [\"https://help.libreoffice.org",
    );
    let in_the_list =
        with(&matrix(), "[only.libreoffice]\nlacks = \"yes\"\nbasis = \"documented\"", "[only.libreoffice]\nlacks = \"yes\"\nbasis = \"oracle\"");
    for data in [in_a_row, in_the_list] {
        let page = compare::render(&template(), &build(&data).unwrap(), &settings(None)).unwrap();
        let line = asof_of(&page);
        assert!(line.contains(says), "{line}");
        assert!(!line.contains("running"), "{line}");
    }
    // A draw.io cell that says it was read from a black box is not LibreOffice Draw's: the line does not name one for it.
    let drawio =
        with(&matrix(), "basis = \"documented\"\nsources = [\"https://www.drawio.com", "basis = \"oracle\"\nsources = [\"https://www.drawio.com");
    let page = compare::render(&template(), &build(&drawio).unwrap(), &settings(None)).unwrap();
    assert!(!asof_of(&page).contains("black box"), "{}", asof_of(&page));
}

#[test]
fn a_mark_is_a_shape_and_a_word_and_unknown_is_a_question_mark() {
    let data = build(&matrix_with(&rows().replacen("value = \"Not yet\"\nbasis = \"documented\"", "value = \"Not yet\"\nbasis = \"absent\"", 1), ""))
        .unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    assert!(page.contains("<span class=\"cmp-mark cmp-full\"><i aria-hidden=\"true\"></i>Full</span>"));
    assert!(page.contains("<span class=\"cmp-mark cmp-part\"><i aria-hidden=\"true\"></i>Partial</span>"));
    assert!(page.contains("<span class=\"cmp-mark cmp-no\"><i aria-hidden=\"true\"></i>Not yet</span>"));
    // A Not yet that rests on a documentation that does not mention the feature is shown apart and says so in words.
    assert!(page.contains(
        "<span class=\"cmp-mark cmp-no cmp-nf\"><i aria-hidden=\"true\"></i>Not yet</span><span class=\"cmp-sub\">not found in docs</span>"
    ));
    // Unknown is a question mark that a screen reader reads as the word.
    let unknown = with(&matrix(), "value = \"Partial\"\nbasis = \"documented\"", "value = \"unknown\"\nbasis = \"unknown\"");
    let page = compare::render(&template(), &build(&unknown).unwrap(), &settings(None)).unwrap();
    assert!(page.contains("<span class=\"cmp-mark cmp-unk\"><i aria-hidden=\"true\">?</i><span class=\"visually-hidden\">Unknown</span></span>"));
}

#[test]
fn every_cell_of_the_other_tools_carries_its_sources_or_its_note_and_the_day_it_was_read() {
    let data = build(&matrix_with(&rows(), "")).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    // Each of the four rows has a LibreOffice cell and a draw.io cell with a link to a source and the day.
    assert_eq!(
        page.matches("<a href=\"https://help.libreoffice.org/latest/en-US/text/shared/02/18010000.html\">help.libreoffice.org/…/18010000.html</a>")
            .count(),
        4
    );
    assert_eq!(page.matches("<a href=\"https://www.drawio.com/docs/features/\">drawio.com/docs/features</a>").count(), 4);
    assert_eq!(page.matches("<p class=\"cmp-read\">Read 26 Nov 2026</p>").count(), 8);
    assert_eq!(page.matches("<summary>Sources</summary>").count(), 8);
    // The DiaCraft cell says how many checklist rows are done, names each with its score, and links the checklist.
    assert!(page.contains("<summary>2 of 2 done</summary>"), "{page}");
    assert!(page.contains("<p>2 done, 0 in part and 0 not yet, of the 2 checklist rows this row covers.</p>"), "{page}");
    assert!(
        page.contains("<summary>0 of 2 done</summary><p>0 done, 0 in part and 2 not yet, of the 2 checklist rows this row covers.</p>"),
        "{page}"
    );
    assert!(
        page.contains("<summary>0 of 2 done</summary>") && !page.contains("<li><b>"),
        "the checklist's row names are not on the page: they hold other companies' names"
    );
    assert!(page.contains("<a data-repo=\"/blob/HEAD/docs/parity-checklist.md\" href=\"https://github.com/owner/repo/blob/HEAD/docs/parity-checklist.md\">The full checklist</a>"));
    // A row with no checklist row links the document its value is read from.
    assert!(page.contains("<summary>Source</summary><p>Read from the doc</p><p><a data-repo=\"/blob/HEAD/docs/x.md\""), "{page}");
    // Each DiaCraft cell gives the row's id, which a correction names.
    for id in ["all-done", "some-done", "none-done", "by-hand"] {
        assert!(page.contains(&format!("<p class=\"cmp-read\">Row id {id}</p>")), "{id}: {page}");
    }
}

#[test]
fn text_of_the_data_is_escaped_so_that_the_page_stays_well_formed() {
    let hostile = with(&matrix(), "feature = \"Feature all-done\"", "feature = \"A <b> & \\\"C\\\"\"");
    let hostile = with(&hostile, "note = \"some of it\"\n[row.drawio]", "note = \"</details><script>x</script> & more\"\n[row.drawio]");
    let data = build(&hostile).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    assert!(page.contains("A &lt;b&gt; &amp; &quot;C&quot;"), "{page}");
    assert!(!page.contains("<script>") && page.contains("&lt;/details&gt;&lt;script&gt;x&lt;/script&gt; &amp; more"));
    roxmltree::Document::parse(&page).unwrap_or_else(|e| panic!("not well-formed: {e}\n{page}"));
}

#[test]
fn the_page_says_when_the_sources_were_read_and_where_to_report_a_correction() {
    let data = build(&matrix()).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    assert!(page.contains("<strong>As of 31 October 2026 and 26 November 2026.</strong>"), "{page}");
    assert!(page.contains("were read on 31 October 2026") && page.contains("on 26 November 2026"), "{page}");
    assert!(page.contains("last reviewed on 22 November 2026, for release 9.9.9"), "{page}");
    assert!(page.contains("<a data-repo=\"/issues/new\" href=\"https://github.com/owner/repo/issues/new\">Report a correction</a>"));
    assert!(page.contains("<dt>Sources read</dt><dd>31 Oct 2026 · 26 Nov 2026</dd>"), "{page}");
    // No day in the reference column's sources is an error, and so is a checklist with no review day.
    let error = build(&with(&matrix(), "read 2026-10-31", "read some day")).unwrap_err();
    assert!(error.contains("name no day"), "{error}");
    let error = compare::build(&matrix(), &CHECKLIST.replace("> **Last reviewed:** 2026-11-22", ""), &exists).unwrap_err();
    assert!(error.contains("Last reviewed"), "{error}");
}

#[test]
fn the_reference_app_is_named_as_a_page_may_name_it_and_the_column_heads_are_bold() {
    let data = build(&matrix()).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    let name = reference_name();
    assert!(
        page.contains(&format!(
            "<th scope=\"col\" role=\"columnheader\"><b>{name}</b></th><th scope=\"col\" role=\"columnheader\"><b>draw.io</b></th>"
        )),
        "{page}"
    );
    assert!(page.contains("<th scope=\"col\" role=\"columnheader\"><b>LibreOffice®</b> Draw</th><th scope=\"col\" role=\"columnheader\" class=\"cmp-us\"><b>DiaCraft</b></th>"));
    // The sentences of the data that say "the reference app" say the name on the page.
    let cell = with(&matrix(), "note = \"\" }", "note = \"the reference app has it\" }");
    let page = compare::render(&template(), &build(&cell).unwrap(), &settings(None)).unwrap();
    assert!(
        page.contains(&format!("Features of {name}'s desktop edition")) || page.contains(&format!("features of {name}'s desktop edition")),
        "{page}"
    );
    assert!(!page.contains("reference app"), "{page}");
}

/// The owner's ruling: draw.io is written in lower case, always, also where a sentence of the data begins with it.
#[test]
fn a_note_that_begins_with_drawio_keeps_its_lower_case_and_any_other_note_gets_a_capital() {
    let data = build(&with(&matrix(), "note = \"some of it\"\n[row.drawio]", "note = \"draw.io has a thing\"\n[row.drawio]")).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    assert!(page.contains("<p>draw.io has a thing</p>"), "{page}");
    assert!(page.contains("<p>Some of it</p>"), "{page}");
    // The all-capitals form is put together from pieces: the export's strip takes a word like it for a task id and leaves the string empty.
    let shouted = ["DRAW", ".IO"].concat();
    assert!(!page.contains("Draw.io") && !page.contains(&shouted), "{page}");
}

#[test]
fn a_caption_names_the_plan_of_the_reference_app_and_the_legend_lists_the_captions_in_use() {
    let rows = rows().replacen("edition = \"desktop\"", "edition = \"plan2\"", 1).replacen("edition = \"desktop\"", "edition = \"web\"", 1);
    let data = build(&matrix_with(&rows, "")).unwrap();
    let page = compare::render(&template(), &data, &settings(None)).unwrap();
    assert!(page.contains("<span class=\"cmp-sub\">Desktop plan</span>") && page.contains("<span class=\"cmp-sub\">Web app</span>"));
    assert!(page.contains("<li><strong>Desktop plan:</strong>") && page.contains("<li><strong>Web app:</strong>"));
    assert!(!page.contains("Plans 1 and 2"), "a caption that no cell has is not in the legend");
}

// ---------------------------------------------------------------------------------------------------------------------
// The page and the data of the repository

/// The reference app's name as a page may write it: the one name of the deny list a page may hold, with a capital (the sources never spell it).
fn reference_name() -> String {
    let name = xtask::brands::SITE_PAGE_NAMES[0];
    format!("{}{}", name[..1].to_uppercase(), &name[1..])
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(xtask::root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The settings of the repository's site, with the data the page is made from.
fn real_settings() -> Settings {
    let root = xtask::root();
    let version = super::workspace_version(&root).unwrap();
    let mut settings = super::settings::load(&root.join("site"), &version).unwrap();
    settings.comparison = Some(std::sync::Arc::new(compare::load(&root).unwrap()));
    settings
}

#[test]
fn the_data_of_the_repository_is_read_and_covers_every_checklist_row_once() {
    let data = compare::load(&xtask::root()).unwrap();
    let [reference, drawio, libreoffice, ours] = data.counts();
    let rows = data.line();
    let total = |counts: [usize; 4]| counts.iter().sum::<usize>();
    assert_eq!([total(reference), total(drawio), total(libreoffice), total(ours)], [total(reference); 4], "{rows}");
    assert_eq!(reference[3] + ours[3], 0, "the reference column and DiaCraft's are never unknown");
    // The research's counts for the columns that no checklist moves (docs/parity-matrix.md, "Counts at this commit").
    assert_eq!(reference, [150, 1, 4, 0]);
    assert_eq!(libreoffice, [22, 79, 49, 5]);
    assert_eq!(drawio, [17, 116, 22, 0]);
}

/// The owner's ruling (2026-11-26): three features qualify as only DiaCraft's, and the page uses only those. Another is a decision for the owner, and this list says so.
#[test]
fn the_page_lists_the_three_features_that_only_diacraft_has_and_no_other() {
    let page = read(&format!("site/{PAGE}"));
    let listed: Vec<&str> = page.split("<li id=\"only-").skip(1).filter_map(|rest| rest.split('"').next()).collect();
    assert_eq!(listed, ["vsdx-and-odg", "native-superset", "offline-browser"]);
    for left_out in ["only-mcp-cli", "only-control-channel"] {
        assert!(!page.contains(left_out), "{left_out}: another tool has it, so the page does not list it");
    }
}

#[test]
fn the_page_in_the_repository_is_what_the_data_and_the_checklist_give() {
    let page = read(&format!("site/{PAGE}"));
    let settings = real_settings();
    let comparison = settings.comparison.clone().unwrap();
    let made = compare::render(&page, &comparison, &settings).unwrap();
    assert!(made == page, "site/{PAGE} is not what docs/parity-matrix.toml and docs/parity-checklist.md give: run `cargo xtask site --write`");
}

/// The page follows the checklist: change one score and the page that was made is no longer the page the data gives.
#[test]
fn a_changed_score_of_the_checklist_makes_the_page_stale() {
    let page = read(&format!("site/{PAGE}"));
    let settings = real_settings();
    let matrix = read("docs/parity-matrix.toml");
    let checklist = read("docs/parity-checklist.md");
    let changed = checklist.replacen("| Shapes | Select all | P0 | D |", "| Shapes | Select all | P0 | M |", 1);
    assert_ne!(changed, checklist, "the row `Select all` is a done row of the checklist");
    let data = compare::build(&matrix, &changed, &|path| xtask::root().join(path).is_file()).unwrap();
    let made = compare::render(&page, &data, &settings).unwrap();
    assert!(made != page, "the page did not move with the checklist");
    // And a cell edited by hand is put back.
    let (before, tables) = page.split_once("<!-- areas:begin -->").unwrap();
    let full = "<span class=\"cmp-mark cmp-full\"><i aria-hidden=\"true\"></i>Full</span>";
    assert!(tables.contains(full));
    let edited = format!(
        "{before}<!-- areas:begin -->{}",
        tables.replacen(full, "<span class=\"cmp-mark cmp-no\"><i aria-hidden=\"true\"></i>Not yet</span>", 1)
    );
    assert_ne!(edited, page);
    let comparison = settings.comparison.clone().unwrap();
    assert_eq!(compare::render(&edited, &comparison, &settings).unwrap(), page);
}

/// The text of the page that a person reads in its main part, with the tags dropped.
fn visible(page: &str) -> String {
    let body = page.split("<main").nth(1).and_then(|rest| rest.split("</main>").next()).unwrap_or("");
    let mut out = String::new();
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// The page without its regions: what a person wrote.
fn hand_written(page: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in page.split_inclusive('\n') {
        let marker = line.trim();
        if marker.starts_with("<!-- ") && marker.ends_with(":begin -->") {
            inside = true;
        } else if marker.starts_with("<!-- ") && marker.ends_with(":end -->") {
            inside = false;
        } else if !inside {
            out.push_str(line);
        }
    }
    out
}

#[test]
fn the_page_carries_the_notices_the_trademarks_ask_for() {
    let page = read(&format!("site/{PAGE}"));
    // The notice in one line of the page, and the sentences the owner required.
    assert!(page.lines().any(|line| line.contains(xtask::brands::COMPARE_NOTICE)), "the page has the notice in one line");
    let reference = reference_name();
    for sentence in [
        "draw.io is a registered trademark of draw.io Ltd.".to_owned(),
        "LibreOffice is a registered trademark of The Document Foundation.".to_owned(),
        "DiaCraft is not affiliated with or endorsed by Microsoft, draw.io Ltd or The Document Foundation.".to_owned(),
        format!("{reference} is a trademark of the Microsoft group of companies."),
    ] {
        assert!(xtask::brands::COMPARE_NOTICE.contains(&sentence), "{sentence}");
    }
    // LibreOffice: every mention in the head carries the mark; the first in the page is bold with it; every column head is bold with it; the name is written whole and no logo is on the page.
    let (head, body) = page.split_once("<main").unwrap();
    for (at, _) in head.match_indices("LibreOffice") {
        assert!(
            head[at..].starts_with("LibreOffice®"),
            "a mention in the head without the mark: {}",
            &head[at.saturating_sub(40)..(at + 40).min(head.len())]
        );
    }
    let first = body.find("LibreOffice").unwrap();
    assert!(
        body[..first].ends_with("<strong>") && body[first..].starts_with("LibreOffice®</strong>"),
        "the first mention is set apart: {}",
        &body[first.saturating_sub(40)..first + 60]
    );
    assert_eq!(page.matches("<b>LibreOffice®</b> Draw</th>").count(), page.matches("<table ").count(), "each table's head sets it apart");
    for short in ["LibreOffice®®", "Libre Office", "LO ", "Libre Draw", "LibreDraw"] {
        assert!(!page.contains(short), "{short}");
    }
    // draw.io is written in lower case, always.
    let lower = page.to_lowercase();
    assert_eq!(lower.matches("draw.io").count(), page.matches("draw.io").count(), "draw.io is never written with a capital");
    assert!(!lower.contains("diagrams.net") && !lower.contains("drawio.com/images"), "no other name of draw.io's and no logo of its site");
    // No picture of a logo: the one picture is the closing band's octopus, and the page loads nothing else.
    let pictures: Vec<&str> = page.split("<img ").skip(1).collect();
    assert!(pictures.iter().all(|img| img.contains("/assets/")), "every picture is the site's own");
    assert!(!lower.contains("logo") || lower.contains("no logo"), "the page says `no logo` and nothing about a logo of another");
    // Not endorsed, and the words that would imply a link with the foundation or the others, in the words a person wrote (the notes of the data say what a feature is, a "partner gallery" among them).
    let text = visible(&hand_written(&page)).to_lowercase();
    for implied in ["official", "partner", "in cooperation with", "approved by", "certified by", "sponsored by"] {
        assert!(!text.contains(implied), "the page says `{implied}`");
    }
}

#[test]
fn the_page_names_no_other_project_and_describes_what_it_compares() {
    let page = read(&format!("site/{PAGE}"));
    let text = visible(&page);
    for rival in
        ["Dia ", "yEd", "Gliffy", "Inkscape", "Pencil", "Mermaid", "PlantUML", "Graphviz", "diagrams.net", "Miro", "OpenOffice", "Excalidraw"]
    {
        assert!(!text.contains(rival), "the page names {rival}");
    }
    let head = page.split("</head>").next().unwrap();
    for needle in ["feature comparison", reference_name().as_str(), "draw.io"] {
        assert!(head.to_lowercase().contains(&needle.to_lowercase()), "the head does not say `{needle}`");
    }
}

#[test]
fn the_tables_are_accessible_tables_and_every_area_is_listed_and_anchored() {
    let page = read(&format!("site/{PAGE}"));
    let data = compare::load(&xtask::root()).unwrap();
    let tables = page.matches("<table ").count();
    assert!(
        tables >= 10 && page.matches("<caption ").count() == tables && page.matches("<thead ").count() == tables,
        "a table has a caption and a head"
    );
    assert_eq!(page.matches("scope=\"col\"").count(), tables * 5);
    assert_eq!(page.matches("scope=\"row\"").count(), data.line().split(' ').nth(3).and_then(|n| n.parse::<usize>().ok()).unwrap());
    // The list of the page's sections has an entry for each area, and each goes to a section of the page.
    let toc = page.split("aria-label=\"On this page\"").nth(1).and_then(|rest| rest.split("</nav>").next()).unwrap();
    for target in toc.split("href=\"#").skip(1).filter_map(|rest| rest.split('"').next()) {
        assert!(page.contains(&format!("id=\"{target}\"")), "the list goes to #{target}, which is not on the page");
    }
    assert_eq!(toc.matches("href=\"#area-").count(), tables);
    // Every cell of a row but the first is a cell with a mark and a word, and a scroll region has a name and can be focused.
    assert_eq!(page.matches("tabindex=\"0\"").count(), tables);
    assert_eq!(page.matches("role=\"region\"").count(), tables);
    assert!(!Value::Unknown.eq(&Value::Full));
}

#[test]
fn the_pages_footer_and_the_features_page_link_the_comparison() {
    let pages: Vec<String> = ["site", "site/guides"]
        .into_iter()
        .flat_map(|dir| {
            std::fs::read_dir(xtask::root().join(dir)).unwrap().map(move |e| format!("{dir}/{}", e.unwrap().file_name().into_string().unwrap()))
        })
        .filter(|name| name.ends_with(".html"))
        .collect();
    for page in &pages {
        let footer = read(page).split("<footer").nth(1).unwrap().to_owned();
        assert!(footer.contains(&format!("<a href=\"/{PAGE}\">Feature comparison</a>")), "{page}: the footer does not link the comparison");
    }
    assert!(read("site/features.html").contains(&format!("href=\"/{PAGE}\"")), "the Features page links the comparison");
    // The top menu stays as it is.
    for page in &pages {
        let nav = read(page).split("aria-label=\"Main navigation\">").nth(1).and_then(|rest| rest.split("</nav>").next()).unwrap().to_owned();
        assert!(!nav.contains(PAGE), "{page}: the comparison is not in the top menu");
    }
}
