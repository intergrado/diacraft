//! A refusal is not an abort (done-criterion D3): the content check's claim that what is past a limit is an error in words, and not a crash, run in a
//! child process under `ulimit -v 3000000` (an address space of 2.86 GiB). The pattern is `crates/container/tests/it/canon_memory.rs`: the child is
//! `#[ignore]` and does the work, and each parent runs it with one scenario and reads what it printed.
//!
//! These are the inputs of the flat path, which builds a tree for each side: a picture named again and again, a picture of text past what one side may
//! inline, and a flat file or a part at the item limit.

use std::process::Command;

use diacraft_odg::{Limits, compare_content};

use crate::drawing::Drawing;
use crate::flat::{Flat, frame, picture};

const SCENARIO: &str = "ODG_CANON_MEMORY_SCENARIO";

fn class_of(a: &[u8], b: &[u8]) -> String {
    match compare_content(a, b, &Limits::default()) {
        Ok(d) => format!("compared count={}", d.count),
        Err(e) => e.class().to_owned(),
    }
}

fn base() -> Drawing {
    let mut d = Drawing::a4().page(r#"<draw:custom-shape draw:name="s" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#);
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d.common = r#"<style:style style:name="S" style:family="graphic"/>"#.to_owned();
    d
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let flat = Flat::of(&base()).bytes();
    match scenario.as_str() {
        // One picture of 2 MiB named by 40 frames: 80 MiB of the 64 MiB one side may inline, which is refused at the 33rd and not expanded.
        "picture_named_many_times" => {
            let mut d = base();
            let image = r#"<draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;
            d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", frame(image).repeat(40)));
            d.files.push(("Pictures/a.png".to_owned(), picture(2 << 20)));
            println!("\nRESULT {}", class_of(&flat, &d.bytes()));
        }
        // The inline data of 100 MB of text in a flat file: refused before it is decoded.
        "huge_inline_data" => {
            let mut f = Flat::of(&base());
            let data = "A".repeat(100_000_000);
            f.body = f.body.replace(
                "</draw:page>",
                &format!("{}</draw:page>", frame(&format!("<draw:image><office:binary-data>{data}</office:binary-data></draw:image>"))),
            );
            println!("\nRESULT {}", class_of(&f.bytes(), &base().bytes()));
        }
        // A flat file past the reader's item limit (4,000,000 items of a tree), against a package.
        "flat_past_the_item_limit" => {
            let mut f = Flat::of(&base());
            f.body = f.body.replace("</draw:page>", &format!("{}</draw:page>", "<draw:g/>".repeat(4_100_000)));
            println!("\nRESULT {}", class_of(&f.bytes(), &base().bytes()));
        }
        // A package part past the container's item limit (6,000,000), against a flat file.
        "part_past_the_item_limit" => {
            let mut d = base();
            d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", "<draw:g/>".repeat(6_100_000)));
            println!("\nRESULT {}", class_of(&flat, &d.bytes()));
        }
        // The same two, one side refused before the other is read: the flat file is the second.
        "part_past_the_item_limit_second" => {
            let mut d = base();
            d.pages = d.pages.replace("</draw:page>", &format!("{}</draw:page>", "<draw:g/>".repeat(6_100_000)));
            println!("\nRESULT {}", class_of(&d.bytes(), &flat));
        }
        // One declaration of a 900 KB address and 100,000 children that use it: 90 GB of copies if each child were given it, which is refused at the 75th.
        "address_used_by_many_children" => {
            let (flat, pkg) = crate::canon_hostile::declaring(1, 100_000, &"x".repeat(900_000), 1);
            println!("\nRESULT {}", class_of(&flat, &pkg));
        }
        // Two parts of 1.1 million items each: each is under the 2 million the parts may hold together (and under the 6 million of one part), and the two are over 2 million.
        "two_parts_under_the_limit_each" => {
            let g = "<draw:g/>".repeat(1_100_000);
            let mut d = base();
            d.pages = d.pages.replace("</draw:page>", &format!("{g}</draw:page>"));
            d.masters = format!("{}{g}", d.masters);
            println!("\nRESULT {}", class_of(&flat, &d.bytes()));
        }
        // A paragraph of 30 MiB of text in each of two drawings that differ in its last character, as two packages and as a flat file against a package: the
        // paragraph rule makes it one string of its length on each side, which is compared, one difference, and is no abort.
        "paragraph_of_thirty_mib" => {
            let build = |last: char| {
                let mut d = base();
                let shape = format!(
                    "<draw:custom-shape draw:name=\"t\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p>{}{last}</text:p></draw:custom-shape>",
                    "x".repeat(30 << 20)
                );
                d.pages = d.pages.replace("</draw:page>", &format!("{shape}</draw:page>"));
                d
            };
            let (a, b) = (build('a'), build('b'));
            let packages = class_of(&a.bytes(), &b.bytes());
            let flat_against_package = class_of(&Flat::of(&a).bytes(), &b.bytes());
            println!("\nRESULT {packages}; {flat_against_package}");
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact canon_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

#[track_caller]
fn refused(scenario: &str, class: &str) {
    let (ended_well, stdout) = run_child(scenario, 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_line(&stdout), format!("RESULT {class}"));
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_picture_named_many_times_is_refused_in_words_and_does_not_abort() {
    refused("picture_named_many_times", "too_much_xml");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn inline_data_past_what_one_side_may_inline_is_refused_before_it_is_decoded() {
    refused("huge_inline_data", "too_much_xml");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_flat_file_past_the_item_limit_is_refused_in_words_and_does_not_abort() {
    refused("flat_past_the_item_limit", "xml_too_complex");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_paragraph_of_thirty_megabytes_is_compared_whole_and_does_not_abort() {
    refused("paragraph_of_thirty_mib", "compared count=1; compared count=1");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn an_address_that_many_children_use_is_refused_in_words_and_does_not_abort() {
    refused("address_used_by_many_children", "too_much_xml");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn two_parts_each_under_the_item_limit_and_over_it_together_are_refused_and_do_not_abort() {
    refused("two_parts_under_the_limit_each", "xml_too_complex");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_package_part_past_the_item_limit_in_a_flat_comparison_is_refused_whichever_side_it_is() {
    refused("part_past_the_item_limit", "xml_too_complex");
    refused("part_past_the_item_limit_second", "xml_too_complex");
}
