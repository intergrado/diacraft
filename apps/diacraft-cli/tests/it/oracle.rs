//! `diacraft-cli oracle`: the binary, run over files built in code (`diacraft-testkit`), and over the reports it printed.

use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::Value;

use crate::{TempDir, cli, json_lines, s, text};

fn drawing() -> Vec<u8> {
    VsdxBuilder::new()
        .app("<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Microsoft Visio</Application></Properties>")
        .page(PageSpec::new(1, "SECRETNAME").shapes(&shape(
            1,
            r#"<Cell N="Height" V="2"/><Cell N="PinX" V="3" F="Height+1"/><Cell N="PinY" V="9" F="Height+1"/><Cell N="Width" V="4" F="Height*2 + SECRETFUNC()"/>"#,
        )))
        .build()
}

#[test]
fn a_package_reports_its_hash_its_category_its_names_and_its_score_as_one_json_line() {
    let dir = TempDir::new("oracle");
    let bytes = drawing();
    let path = dir.write("a.vsdx", &bytes);
    let out = cli(&["oracle", s(&path), "--json", "--source", "owner/repo"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 1);
    let r = &lines[0];
    let sha = r["sha256"].as_str().unwrap();
    assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{sha}");
    assert_eq!(r["schema"], 7);
    // The parts the document holds beside its model and their bytes are in the stats (new fields; the schema is the same).
    let stats = &r["read_report"]["stats"];
    assert!(stats["held_parts"].is_u64() && stats["held_bytes"].is_u64(), "{stats}");
    assert_eq!(r["source"], "owner/repo");
    assert_eq!(r["read"], true);
    assert_eq!(r["category"]["writer"], "reference_app");
    assert_eq!(r["names"]["cells"]["Width"]["formula"], 1);
    assert_eq!(r["names"]["functions"]["_MUL"], 1);
    // The comparer is joined in: PinX agrees, PinY does not, and Width calls a function that nothing lists and nothing here has.
    assert_eq!(r["calc"]["available"], true);
    assert_eq!(r["calc"]["control"]["passed"], true);
    assert_eq!((r["calc"]["compared"].as_u64(), r["calc"]["matched"].as_u64()), (Some(2), Some(1)));
    assert_eq!(r["calc"]["not_calculated"]["total"], 1);
    assert_eq!(r["score"]["score"], 0.5);
    assert_eq!(r["score"]["coverage"], 2.0 / 3.0);
    assert_eq!(r["calc"]["differences"][0]["cell"], "PinY");
    // Every scored file is salted: the cell that agreed was given a wrong saved value, and the comparer caught it.
    let salt = &r["calc"]["real_salt"];
    assert_eq!(
        (&salt["passed"], &salt["salted"], &salt["caught"], &salt["missed"]),
        (&Value::Bool(true), &Value::from(1), &Value::from(1), &Value::from(0))
    );
    assert_eq!(salt["by_kind"]["number"], serde_json::json!({ "salted": 1, "caught": 1 }));
    // No text and no formula from the file crossed.
    let all = text(&out.stdout);
    assert!(!all.contains("SECRET") && !all.contains("Height*2") && !all.contains("Height+1"), "{all}");
    // The same file is the same hash and the same report.
    let again = cli(&["oracle", s(&path), "--json", "--source", "owner/repo"], None);
    assert_eq!(text(&again.stdout), all);
}

#[test]
fn a_file_that_is_not_a_package_is_a_report_with_its_class_and_the_exit_status_is_zero() {
    let dir = TempDir::new("oracle-refused");
    let path = dir.write("x.vsdx", b"<html>not a zip</html>");
    let out = cli(&["oracle", s(&path), "--json"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let r = &json_lines(&out.stdout)[0];
    assert_eq!(r["read"], false);
    assert_eq!(r["refusal"]["class"], "not_zip");
    assert_eq!(r["refusal"]["kind"], "not_a_package");
    assert_eq!(r["score"], Value::Null);
}

#[test]
fn several_files_are_one_line_each_and_one_that_cannot_be_read_is_named_and_ends_with_status_one() {
    let dir = TempDir::new("oracle-many");
    let a = dir.write("a.vsdx", &drawing());
    let b = dir.write("b.vsdx", b"nope");
    let missing = dir.path("missing.vsdx");
    let folder = dir.path("folder.vsdx");
    std::fs::create_dir(&folder).unwrap();
    let out = cli(&["oracle", s(&a), s(&missing), s(&b), s(&folder), "--json"], None);
    assert_eq!(out.status.code(), Some(1));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 2, "the files that could be read were reported");
    assert_eq!(lines[0]["read"], true);
    assert_eq!(lines[1]["refusal"]["class"], "not_zip");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("there is no such file") && stderr.contains("not a file"), "{stderr}");
    assert!(stderr.contains("2 file(s) could not be read"), "{stderr}");
}

#[test]
fn without_json_it_prints_the_control_and_a_few_lines_of_words() {
    let dir = TempDir::new("oracle-words");
    let path = dir.write("a.vsdx", &drawing());
    let out = cli(&["oracle", s(&path)], None);
    assert!(out.status.success());
    let printed = text(&out.stdout);
    assert!(printed.starts_with("control passed: "), "{printed}");
    assert!(printed.contains("sha256 ") && printed.contains("writer reference_app"), "{printed}");
    assert!(printed.contains("formula cells 3: compared 2 (1 agreed, 1 differed), left out 0, not calculated 1"), "{printed}");
    assert!(printed.contains("score 0.5000 (1 of 2), coverage 0.6667"), "{printed}");
    assert!(!printed.contains("SECRET"));
}

/// The reports `oracle --json` prints for a drawing, the same drawing under another source, a package with nothing to score, and two files
/// that are not packages, written to one file of lines.
fn reports(dir: &TempDir) -> std::path::PathBuf {
    let drawing_path = dir.write("a.vsdx", &drawing());
    let bare = dir.write("bare.vsdx", &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, r#"<Cell N="A" V="1"/>"#))).build());
    let not_a_package = dir.write("x.vsdx", b"nope");
    let other_not_a_package = dir.write("y.vsdx", b"nope, not either");
    let mut lines = String::new();
    for (files, source) in [(vec![&drawing_path, &bare, &not_a_package], "a/a"), (vec![&drawing_path, &other_not_a_package], "b/b")] {
        let mut args: Vec<&str> = vec!["oracle", "--json", "--source", source];
        args.extend(files.iter().map(|f| s(f)));
        let out = cli(&args, None);
        assert!(out.status.success(), "{}", text(&out.stderr));
        lines.push_str(&text(&out.stdout));
    }
    dir.write("reports.jsonl", lines.as_bytes())
}

#[test]
fn reports_are_added_up_with_their_totals_and_a_file_in_two_sources_is_one_file() {
    let dir = TempDir::new("oracle-aggregate");
    let path = reports(&dir);
    let out = cli(&["oracle", "--aggregate", s(&path), "--cap", "2"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let r = &json_lines(&out.stdout)[0];
    // Five reports, four files: the drawing is in both sources. Two are not packages. One has nothing to score, and one is scored.
    assert_eq!((r["reports"].as_u64(), r["files"].as_u64(), r["duplicates_dropped"].as_u64()), (Some(5), Some(4), Some(1)));
    assert_eq!((r["refused"].as_u64(), r["scored"].as_u64(), r["control_failed"].as_u64()), (Some(2), Some(1), Some(0)));
    // The real-file salt is added up: one cell salted in the one file that has an agreeing cell, and caught. None failed.
    assert_eq!(r["real_salt_failed"], 0);
    assert_eq!(r["real_salt"], serde_json::json!({ "salted": 1, "caught": 1, "by_allowance": 0, "missed": 0, "not_checked": 0 }));
    assert_eq!((r["compared"].as_u64(), r["matched"].as_u64()), (Some(2), Some(1)));
    assert_eq!(r["not_calculated"], 1);
    assert_eq!(r["by_file"], 0.5);
    assert_eq!(r["coverage"], 2.0 / 3.0);
    assert_eq!(r["cells"]["formula"], 3);
    assert_eq!(r["per_source"]["a/a"]["refused"], 1);
    assert_eq!(r["per_source"]["a/a"]["files"], 3, "the drawing is kept under the lesser source");
    assert!(r["per_source"].get("b/b").is_some());
    assert_eq!(r["per_source"]["b/b"]["files"], 1);
}

#[test]
fn an_aggregate_of_a_run_whose_real_salt_failed_prints_its_line_and_ends_with_status_one() {
    let dir = TempDir::new("oracle-salt-failed");
    let good = reports(&dir);
    let first = std::fs::read_to_string(&good).unwrap().lines().next().unwrap().to_owned();
    let mut report: Value = serde_json::from_str(&first).unwrap();
    // The drawing's one agreeing cell, salted and still agreeing: an echo. Its score is withheld, as the harness does.
    report["calc"]["real_salt"] = serde_json::json!({ "passed": false, "salted": 1, "caught": 0, "by_allowance": 0, "missed": 1, "not_checked": 0 });
    report["score"] = Value::Null;
    report["score_withheld"] = serde_json::json!("real_salt_failed");
    let path = dir.write("failed.jsonl", format!("{report}\n").as_bytes());
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    let r = &json_lines(&out.stdout)[0];
    assert_eq!((r["real_salt_failed"].as_u64(), r["scored"].as_u64(), r["compared"].as_u64()), (Some(1), Some(0), Some(0)));
    assert_eq!(r["real_salt"]["missed"], 1);
    assert!(text(&out.stderr).contains("real-file salt"), "{}", text(&out.stderr));
    // The same report with a score on it cannot be true.
    report["score"] = serde_json::json!({ "score": 0.5, "matched": 1, "compared": 2 });
    let path = dir.write("lying.jsonl", format!("{report}\n").as_bytes());
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("a score under a real-file salt that failed"), "{}", text(&out.stderr));
}

#[test]
fn aggregating_something_that_is_not_a_report_is_an_error_naming_the_line() {
    let dir = TempDir::new("oracle-bad");
    let path = dir.write("r.jsonl", b"{\"sha256\":\"1\"}\nnot json\n");
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("line 1"), "{}", text(&out.stderr));
    let path = dir.write("r2.jsonl", b"[1,2]\n");
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("not a report"), "{}", text(&out.stderr));
    let path = dir.write("r3.jsonl", b"{\"schema\":2,\"sha256\":\"1\",\"read\":true,\"calc\":{\"control\":{\"passed\":true}}}\nnot json\n");
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("line 1"), "{}", text(&out.stderr));
}

#[test]
fn a_line_that_cannot_be_true_stops_the_aggregate_and_names_the_line_and_why() {
    let dir = TempDir::new("oracle-impossible");
    let good = reports(&dir);
    let text_of_reports = std::fs::read_to_string(&good).unwrap();
    // The first line says 100 cells matched of the 2 it compared: the line that once gave a score of 10.0.
    let first = text_of_reports.lines().next().unwrap().replace("\"matched\":1", "\"matched\":100");
    let rest: Vec<&str> = text_of_reports.lines().skip(1).collect();
    let path = dir.write("bad.jsonl", format!("{first}\n{}\n", rest.join("\n")).as_bytes());
    let out = cli(&["oracle", "--aggregate", s(&path)], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("line 1") && stderr.contains("matched"), "{stderr}");
}

#[test]
fn a_mistake_in_the_command_line_is_status_two() {
    let out = cli(&["oracle"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("nothing to report on"));
}

#[test]
fn a_refused_file_reports_what_the_reader_knew_and_where_it_stopped_and_names_no_part_a_person_typed() {
    let dir = TempDir::new("oracle-refused-detail");
    // A page part named by a person, malformed: refused as XML, with the reader's own figures and no part name.
    let page = "<PageContents><Shapes></PageContents>";
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").part(page)).build();
    let path = dir.write("a.vsdx", &bytes);
    let out = cli(&["oracle", s(&path), "--json"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let r = &json_lines(&out.stdout)[0];
    assert_eq!(r["read"], false);
    assert_eq!(r["refusal"]["class"], "xml_malformed");
    let detail = r["refusal"]["detail"].as_str().unwrap();
    assert!(detail.contains("not well-formed at byte"), "{detail}");
    assert_eq!(r["read_report"]["stats"]["file_bytes"], bytes.len());
    assert!(r["read_report"]["stats"]["zip_rows"].as_u64().unwrap() >= 8);
    // And the words form says it too.
    let out = cli(&["oracle", s(&path)], None);
    assert!(text(&out.stdout).contains("not well-formed at byte"), "{}", text(&out.stdout));
}
