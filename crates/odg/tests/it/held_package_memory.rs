//! What the package entries, `settings.xml`, the manifest and the font faces cost in memory, in a child process under an address-space limit
//! (`ulimit -v 3000000`), as `docfile_memory.rs` runs its own. A read that cannot have the memory must end as an error of a class and never as an abort.
//!
//! * **An entry is held once.** A `settings.xml` of 64 MiB (past the cap of a small part, so an entry of the package) and a font of the same bytes (the yardstick: plain content of the
//!   same size) read to the same peak, and after the read the model holds the one copy: the entry's bytes are the package's own and are not copied to make the part.
//! * **A settings tree is charged before it is made.** One of 1,000,000 items reads, and the same file refused for the items it costs leaves the tree not built: the refused read peaks
//!   by the tree less than the one that reads.
//! * **The manifest of 100,000 rows and 65,536 faces** read under the limit.

#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_odg::{Limits, read_partial};

use crate::drawing::Drawing;

const SCENARIO: &str = "ODG_ODFTAG3_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// The settings of about `bytes` bytes, valid XML of 70 bytes an item.
fn settings_of_bytes(bytes: usize) -> Vec<u8> {
    let item = r#"<config:config-item config:name="x" config:type="int">1</config:config-item>"#;
    crate::drawing::settings(&item.repeat(bytes / item.len())).into_bytes()
}

/// The package of a drawing with the part `name` made by `make`, each part deflated so that the file stays small.
fn package_of(name: &str, make: &dyn Fn() -> Vec<u8>) -> Vec<u8> {
    crate::read_hostile::part_by_part(Drawing::a4().page(""), &[name], &|_| make())
}

/// Reads `bytes` with the peak reset first, and prints what the read held: its peak, and what is resident with its model held.
fn measured(scenario: &str, bytes: &[u8], limits: &Limits) -> (Option<&'static str>, diacraft_doc::Document) {
    let before = status_kib("VmRSS");
    let _reset = std::fs::write("/proc/self/clear_refs", "5");
    let (result, report) = read_partial(bytes, limits);
    let class = result.as_ref().err().map(|e| e.class());
    println!(
        "\nRESULT {scenario} class={} items={} media={} RSS_BEFORE={before}KiB RSS_AFTER={}KiB VmHWM={}KiB",
        class.unwrap_or("read"),
        report.work.items,
        report.work.media_bytes,
        status_kib("VmRSS"),
        status_kib("VmHWM")
    );
    (class, result.unwrap_or_else(|_| diacraft_doc::Document::empty()))
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        "settings_64mib" | "font_64mib" => {
            let (name, held) = if scenario == "settings_64mib" { ("settings.xml", "settings.xml") } else { ("Fonts/big.ttf", "Fonts/big.ttf") };
            let bytes = package_of(name, &|| settings_of_bytes(64 << 20));
            let (class, doc) = measured(&scenario, &bytes, &Limits::default());
            assert_eq!(class, None, "{scenario}");
            let part = doc.parts.iter().find(|p| &*p.name == held).expect("the entry is a part");
            assert!(part.bytes.len() >= 64 << 20, "{scenario}");
        }
        "settings_1m_items" | "settings_1m_items_refused" => {
            let settings = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {} office:version=\"1.3\"><office:settings>{}</office:settings></office:document-settings>",
                crate::drawing::NS,
                "<a/>".repeat(1_000_000)
            );
            let mut d = Drawing::a4().page("");
            d.settings = Some(settings);
            let bytes = d.bytes();
            drop(d);
            // The items the whole read costs, found by reading it first (the peak is the larger of the two reads, so the refused one runs in its own process).
            let (limits, expected) = if scenario.ends_with("refused") {
                (Limits { max_items: 3_000_000, ..Limits::default() }, Some("too_many_items"))
            } else {
                (Limits::default(), None)
            };
            let (class, doc) = measured(&scenario, &bytes, &limits);
            assert_eq!(class, expected, "{scenario}");
            if expected.is_none() {
                assert!(doc.ext.get().is_some_and(|d| d.odf.settings.is_some()));
            }
        }
        "manifest_100000_rows" => {
            let c = crate::held_package_cost::Crafted { present: 40_000, unnamed: 9_980, missing: 40_000, twice: 20_000 };
            let bytes = c.bytes();
            let (class, doc) = measured(&scenario, &bytes, &Limits::default());
            assert_eq!(class, None, "{scenario}");
            assert_eq!(doc.parts.len(), 49_980);
        }
        "faces_65536" => {
            let mut d = Drawing::a4().page("");
            d.fonts = (0..65_536)
                .map(|k| format!(r#"<style:font-face style:name="f{k:05}" svg:font-family="f{k:05}" style:font-pitch="fixed"/>"#))
                .collect();
            let bytes = d.bytes();
            drop(d);
            let (class, doc) = measured(&scenario, &bytes, &Limits::default());
            assert_eq!(class, None, "{scenario}");
            assert_eq!(doc.fonts.len(), 65_536);
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact held_package_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// A number a child printed (`KEY=<digits>`).
fn number_of(stdout: &str, key: &str) -> u64 {
    let at = stdout.find(&format!("{key}=")).unwrap_or_else(|| panic!("no {key} in {stdout}")) + key.len() + 1;
    stdout[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or_else(|_| panic!("no number after {key} in {stdout}"))
}

/// A `settings.xml` of 64 MiB reads to the peak of a font of the same bytes, under the address limit, and the model holds one copy of it: the part is the package's own bytes.
#[test]
fn a_settings_part_of_64_mib_is_held_once_and_peaks_as_a_plain_part_of_its_size_does() {
    let (a_well, settings) = run_child("settings_64mib", 3_000_000);
    let (b_well, font) = run_child("font_64mib", 3_000_000);
    assert!(a_well && b_well, "a child aborted, or its assertion failed: {settings} {font}");
    let (peak_a, peak_b) = (number_of(&settings, "VmHWM"), number_of(&font, "VmHWM"));
    println!("peak of the settings part {peak_a} KiB, of the font {peak_b} KiB");
    assert!(peak_a <= peak_b + peak_b / 20 + 4_096, "{peak_a} KiB for the settings part against {peak_b} KiB for the font of the same bytes");
    // After the read the model holds the part once, beside what the process held before: one copy of 64 MiB (65,536 KiB) and a small margin, never two.
    let held = number_of(&settings, "RSS_AFTER").saturating_sub(number_of(&settings, "RSS_BEFORE"));
    assert!(held <= 65_536 + 65_536 / 8 + 8_192, "the process holds {held} KiB more after the read: the entry was copied");
}

/// A `settings.xml` of 1,000,000 items reads under the limit; the same file refused for its items peaks lower by the tree the first holds, because the charge is paid before the tree is
/// made.
#[test]
fn a_settings_tree_of_a_million_items_reads_and_is_refused_before_the_tree_is_made() {
    let (a_well, read) = run_child("settings_1m_items", 3_000_000);
    let (b_well, refused) = run_child("settings_1m_items_refused", 3_000_000);
    assert!(a_well && b_well, "a child aborted, or its assertion failed: {read} {refused}");
    let (peak_read, peak_refused) = (number_of(&read, "VmHWM"), number_of(&refused, "VmHWM"));
    println!("peak of the read {peak_read} KiB, of the refused read {peak_refused} KiB");
    // The tree of a million empty elements is a node and a name each: measured 43 MB (150,244 KiB refused against 192,940 KiB read), and the least that tells it from a tree made
    // before it is charged (the two peaks equal) is 30 MB.
    assert!(peak_refused + 30_000 <= peak_read, "{peak_refused} KiB refused against {peak_read} KiB read: the tree was made before it was charged");
}

#[test]
fn a_manifest_of_100000_rows_reads_under_the_address_limit() {
    let (ended_well, stdout) = run_child("manifest_100000_rows", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn sixty_five_thousand_faces_read_under_the_address_limit() {
    let (ended_well, stdout) = run_child("faces_65536", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}
