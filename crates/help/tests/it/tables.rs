//! The three tables of a help tree: tips, the help map and the sections (design sections 2.2 and 6.3).

use diacraft_help::tables::{Map, MapTarget, Place, Sections, Tips, split_tsv_row, unescape};
use diacraft_help::{Code, MAX_DESCRIPTION_CHARS, MAX_KEYWORDS, MAX_MAP_ROWS, MAX_ROW_BYTES, MAX_TIPS_ROWS};

fn codes(problems: &[diacraft_help::Problem]) -> Vec<(usize, Code)> {
    problems.iter().map(|p| (p.line, p.code)).collect()
}

#[test]
fn a_tip_row_has_a_key_a_description_and_keywords_and_comments_and_blank_lines_are_skipped() {
    let text = "# key\tdescription\tkeywords\n\nSnapAndGlueDialog\tTurns on snapping.\tsnap glue magnet\nview.fitPage\tFits the page.\t\n";
    let (tips, problems) = Tips::parse(text);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tips.rows().len(), 2);
    let snap = tips.get("SnapAndGlueDialog").unwrap();
    assert_eq!(
        (snap.description.as_str(), snap.keywords.as_slice()),
        ("Turns on snapping.", ["snap", "glue", "magnet"].map(String::from).as_slice())
    );
    assert!(tips.get("view.fitPage").unwrap().keywords.is_empty());
    assert!(tips.get("nope").is_none());
}

#[test]
fn escapes_in_a_tip_are_newline_tab_and_backslash_only() {
    let (tips, problems) = Tips::parse("a.b\tOne\\nTwo\\tThree\\\\Four\t\n");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tips.get("a.b").unwrap().description, "One\nTwo\tThree\\Four");
    let (tips, problems) = Tips::parse("a.b\tBad \\q escape\t\nc.d\tTrailing \\\t\n");
    assert_eq!(codes(&problems), [(1, Code::RowEscape), (2, Code::RowEscape)]);
    assert!(tips.rows().is_empty(), "a row that breaks a rule is skipped");
}

#[test]
fn a_tip_row_that_breaks_a_rule_is_skipped_with_its_line() {
    let long = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
    let many = (0..=MAX_KEYWORDS).map(|n| format!("k{n}")).collect::<Vec<_>>().join(" ");
    let text = format!(
        "ok.one\tFine.\tk\ntwo columns\tonly\nfour\ta\tb\tc\n\tno key\tk\nok.one\tTwice.\tk\nlong.one\t{long}\tk\nmany.one\tFine.\t{many}\nempty.one\t\tk\nctl.one\tA\u{1}B\tk\nok.two\tFine.\tk\n"
    );
    let (tips, problems) = Tips::parse(&text);
    assert_eq!(
        codes(&problems),
        [
            (2, Code::RowColumns),
            (3, Code::RowColumns),
            (4, Code::RowKey),
            (5, Code::RowDuplicate),
            (6, Code::DescriptionLength),
            (7, Code::TooManyKeywords),
            (8, Code::DescriptionLength),
            (9, Code::RowControl),
        ]
    );
    let keys: Vec<&str> = tips.rows().iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["ok.one", "ok.two"]);
    assert_eq!(tips.get("ok.one").unwrap().description, "Fine.", "of two rows with one key the first is kept");
}

#[test]
fn a_description_of_400_characters_and_32_keywords_are_the_most() {
    let at = "é".repeat(MAX_DESCRIPTION_CHARS);
    let words = (0..MAX_KEYWORDS).map(|n| format!("k{n}")).collect::<Vec<_>>().join(" ");
    let (tips, problems) = Tips::parse(&format!("a.b\t{at}\t{words}\n"));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tips.get("a.b").unwrap().keywords.len(), MAX_KEYWORDS);
}

#[test]
fn a_tips_table_keeps_5000_rows_and_says_it_skipped_the_rest() {
    let mut text = String::new();
    for n in 0..MAX_TIPS_ROWS + 3 {
        text.push_str(&format!("key{n}\tD.\tk\n"));
    }
    let (tips, problems) = Tips::parse(&text);
    assert_eq!(tips.rows().len(), MAX_TIPS_ROWS);
    assert_eq!(codes(&problems), [(MAX_TIPS_ROWS + 1, Code::TooManyRows)], "said once, at the first row skipped");
}

#[test]
fn a_row_over_the_byte_cap_and_a_file_that_is_a_wall_of_noise_are_skipped_not_followed() {
    let line = format!("a.b\t{}\tk\nok.row\tFine.\tk\n", "x".repeat(MAX_ROW_BYTES));
    let (tips, problems) = Tips::parse(&line);
    assert_eq!(codes(&problems), [(1, Code::RowTooLong)]);
    assert_eq!(tips.rows().len(), 1);
    let noise = "\t".repeat(100_000);
    let (tips, problems) = Tips::parse(&noise);
    assert!(tips.rows().is_empty() && problems.len() <= diacraft_help::MAX_PROBLEMS);
}

#[test]
fn a_place_is_a_kind_a_colon_and_a_name() {
    for (text, place) in [
        ("ribbon:SnapAndGlueDialog", Place::Ribbon("SnapAndGlueDialog".to_owned())),
        ("group:GroupClipboard", Place::Group("GroupClipboard".to_owned())),
        ("tab:HelpTab", Place::Tab("HelpTab".to_owned())),
        ("dialog:snap_glue", Place::Dialog { id: "snap_glue".to_owned(), tab: None }),
        ("dialog:snap_glue/advanced", Place::Dialog { id: "snap_glue".to_owned(), tab: Some("advanced".to_owned()) }),
        ("pane:size_position", Place::Pane("size_position".to_owned())),
        ("backstage:new", Place::Backstage("new".to_owned())),
        ("window:sheet", Place::Window("sheet".to_owned())),
        ("cmd:view.fitPage", Place::Command("view.fitPage".to_owned())),
    ] {
        assert_eq!(Place::parse(text), Some(place), "{text}");
    }
    for bad in
        ["", "ribbon:", ":x", "ribbon", "ribbon:a b", "foo:bar", "dialog:a/b/c", "dialog:/tab", "dialog:a/", "cmd:a:b", "window:", "ribbon:Ünï"]
    {
        assert_eq!(Place::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_map_row_gives_a_place_a_topic_a_topic_and_an_anchor_or_none_with_why() {
    let text = "ribbon:SnapAndGlueDialog\tsnap-and-glue\t\ndialog:snap_glue/advanced\tsnap-and-glue#advanced\t\ncmd:view.fitPage\tzoom-and-pan#fit\tnote kept\nribbon:Quiet\tnone\tnot built yet\n";
    let (map, problems) = Map::parse(text);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(map.rows().len(), 4);
    assert_eq!(map.get(&Place::Ribbon("SnapAndGlueDialog".to_owned())), Some(&MapTarget::Topic { id: "snap-and-glue".to_owned(), anchor: None }));
    assert_eq!(
        map.get(&Place::Dialog { id: "snap_glue".to_owned(), tab: Some("advanced".to_owned()) }),
        Some(&MapTarget::Topic { id: "snap-and-glue".to_owned(), anchor: Some("advanced".to_owned()) })
    );
    assert_eq!(map.get(&Place::Ribbon("Quiet".to_owned())), Some(&MapTarget::None { note: "not built yet".to_owned() }));
    assert_eq!(map.get(&Place::Ribbon("Absent".to_owned())), None);
}

#[test]
fn a_map_row_that_breaks_a_rule_is_skipped_with_its_line() {
    let text = "ribbon:A\ttopic-a\t\nno colon\ttopic-a\t\nribbon:B\tUpper\t\nribbon:C\tnone\t\nribbon:D\ttopic-d#\t\nribbon:E\ttopic-e\t\nribbon:A\ttopic-b\t\nribbon:F\ta\tb\tc\nribbon:G\tNone\tnote\n";
    let (map, problems) = Map::parse(text);
    assert_eq!(
        codes(&problems),
        [
            (2, Code::PlaceSyntax),
            (3, Code::MapTarget),
            (4, Code::MapTarget),
            (5, Code::MapTarget),
            (7, Code::RowDuplicate),
            (8, Code::RowColumns),
            (9, Code::MapTarget)
        ]
    );
    let places: Vec<String> = map.rows().iter().map(|r| format!("{:?}", r.place)).collect();
    assert_eq!(places.len(), 2, "{places:?}");
}

#[test]
fn a_map_keeps_5000_rows() {
    let mut text = String::new();
    for n in 0..MAX_MAP_ROWS + 2 {
        text.push_str(&format!("cmd:a.b{n}\tt\t\n"));
    }
    let (map, problems) = Map::parse(&text);
    assert_eq!(map.rows().len(), MAX_MAP_ROWS);
    assert_eq!(codes(&problems), [(MAX_MAP_ROWS + 1, Code::TooManyRows)]);
}

#[test]
fn sections_group_their_topics_in_the_order_the_rows_come() {
    let text = "# section\ttopic\nGet started\thome-page\nDraw and connect\tconnect\nGet started\tuse-help\nDraw and connect\tglue\nPrint\tprint\n";
    let (sections, problems) = Sections::parse(text);
    assert!(problems.is_empty(), "{problems:?}");
    let shape: Vec<(&str, Vec<&str>)> =
        sections.sections().iter().map(|s| (s.title.as_str(), s.topics.iter().map(String::as_str).collect())).collect();
    assert_eq!(shape, [("Get started", vec!["home-page", "use-help"]), ("Draw and connect", vec!["connect", "glue"]), ("Print", vec!["print"])]);
}

#[test]
fn a_sections_row_that_breaks_a_rule_is_skipped() {
    let text = "A\tone\nA\tone\n\ttwo\nB\tNot An Id\nB\tfine\nonly one column\n";
    let (sections, problems) = Sections::parse(text);
    assert_eq!(codes(&problems), [(2, Code::RowDuplicate), (3, Code::RowKey), (4, Code::TargetId), (6, Code::RowColumns)]);
    assert_eq!(sections.sections().len(), 2);
}

#[test]
fn a_row_is_split_at_tabs_and_a_column_is_unescaped() {
    assert_eq!(split_tsv_row("a\tb\t\tc"), ["a", "b", "", "c"]);
    assert_eq!(split_tsv_row(""), [""]);
    assert_eq!(split_tsv_row("\t"), ["", ""]);
    assert_eq!(unescape("a\\nb\\tc\\\\d").as_deref(), Some("a\nb\tc\\d"));
    assert_eq!(unescape("plain").as_deref(), Some("plain"));
    assert_eq!(unescape("bad \\q"), None);
    assert_eq!(unescape("trailing \\"), None);
    assert_eq!(unescape("").as_deref(), Some(""));
}
