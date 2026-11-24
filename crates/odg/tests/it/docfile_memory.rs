//! What the document file costs in memory, in a child process under an address-space limit (`ulimit -v 3000000`), as `picture_memory.rs` runs its own. A
//! process that is out of memory *aborts*, and a child that aborts fails the parent: a save or a read that cannot have the memory must end as an error of a class, never as an
//! abort.
//!
//! * **A save** holds the memory of the trees it is going to make (counted from the models) before it makes them: a master of 300,000 shapes is saved, and masters of 1.2 million
//!   cells (a tree of 8 million items, which the room of the gate does not hold) are refused as a limit with no tree made.
//! * **A read** of a document file of 3.9 million `shape` elements, 62 MB and nearly the 4 million items a tree of the reader holds, ends as the reader's items refused, and not
//!   as an abort: the codec is given the reader's room and refuses where it is spent.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Master, Shape, ShapeList, Value};
use diacraft_odg::{Limits, OriginalPackage, WriteOptions, WriteWork, write_counted};

const SCENARIO: &str = "ODG_DOCFILE_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// The save of a drawing with one master of `shapes` shapes of `cells` cells each: what it ended as (its class, or `saved`), and the work it counted.
fn save_master(shapes: u32, cells: u32) -> (String, WriteWork, Option<Vec<u8>>) {
    let o = crate::docfile::base();
    let names: Vec<CellName> = (0..cells).map(|k| CellName::new(&format!("C{k}"))).collect();
    let cell = Cell::constant(Value::from_text("1"));
    let members: Vec<Arc<Shape>> = (1..=shapes)
        .map(|id| {
            let mut s = Shape::new(id);
            if cells > 0 {
                let sheet = Arc::make_mut(&mut s.sheet);
                for name in &names {
                    sheet.cells.insert(name.clone(), cell.clone());
                }
            }
            Arc::new(s)
        })
        .collect();
    let mut doc = o.doc.clone();
    doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(members), ..Master::default() }));
    let mut work = WriteWork::default();
    let saved = write_counted(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let class = saved.as_ref().err().map_or("saved".to_owned(), |e| e.class().to_owned());
    (class, work, saved.ok().map(|w| w.bytes))
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        "a_master_of_300000_shapes" => {
            let (class, work, made) = save_master(300_000, 0);
            println!(
                "\nRESULT {scenario} class={class} held={} trees={} items={} VmHWM={}KiB",
                work.docfile_held,
                work.docfile_trees,
                work.docfile_items,
                status_kib("VmHWM")
            );
            assert_eq!(class, "saved", "{scenario}");
            assert_eq!(work.docfile_trees, 1);
            assert!(
                work.docfile_held >= work.docfile_items * diacraft_sheetxml::model::cost::TREE_ITEM_BYTES,
                "the memory held is never below the tree made"
            );
            if let Some(made) = made {
                crate::write_core::assert_reads_back(&scenario, &made);
            }
        }
        "a_master_of_1200000_cells" => {
            let (class, work, _) = save_master(20_000, 60);
            println!("\nRESULT {scenario} class={class} held={} trees={} VmHWM={}KiB", work.docfile_held, work.docfile_trees, status_kib("VmHWM"));
            assert_eq!(class, "write_limit", "{scenario}");
            assert_eq!(work.docfile_trees, 0, "the refusal is before the first tree is made");
        }
        "a_file_of_3900000_shapes" => {
            let shapes = r#"<diacraft:shape/>"#.repeat(3_900_000);
            let file = crate::docfile_cost::file_of(&format!(r#"<diacraft:master id="1">{shapes}</diacraft:master>"#));
            drop(shapes);
            let package = crate::docfile_cost::package_with(&file);
            drop(file);
            let (read, report) = diacraft_odg::read_partial(&package, &Limits::default());
            let class = read.as_ref().err().map_or("read", |e| e.class());
            println!("\nRESULT {scenario} class={class} items={} VmHWM={}KiB", report.work.items, status_kib("VmHWM"));
            // The master holds more shapes than a part may (a million): the codec refuses it before it has built them, and the master is damaged and left out with a word
            // (reading never fails the drawing for what the document file holds, the design's 7.3), or the reader's items are spent first.
            assert!(matches!(class, "read" | "too_many_items"), "{scenario}: {class}");
            if let Ok(doc) = &read {
                assert!(doc.masters.is_empty() && report.warned("diacraft_file_unreadable") == 1, "{scenario}: the master is left out with one word");
            }
        }
        "masters_of_60_shapes_to_the_budget" => {
            let master = format!(r#"<diacraft:master id="1">{}</diacraft:master>"#, r#"<diacraft:shape/>"#.repeat(60));
            let file = crate::docfile_cost::file_of(&master.repeat(60_000));
            drop(master);
            let package = crate::docfile_cost::package_with(&file);
            drop(file);
            // The document's cap of shapes (two million) is taken away, so that what ends the read is the reader's items and no other cap.
            let limits = Limits { max_shapes: usize::MAX, ..Limits::default() };
            let (read, report) = diacraft_odg::read_partial(&package, &limits);
            let class = read.as_ref().err().map_or("read", |e| e.class());
            println!("\nRESULT {scenario} class={class} items={} VmHWM={}KiB", report.work.items, status_kib("VmHWM"));
            // 3.6 million shapes of 448 bytes of model are 1.6 GB, which the reader's items (20 million of 64 bytes) do not hold: refused as its items, not aborted.
            assert_eq!(class, "too_many_items", "{scenario}");
        }
        "twelve_masters_of_100mb_of_kept_text" => {
            // The slots of a master's shapes were no figure of the gate, so masters that held 100 MB of kept text each passed it (125 KB held for a tree of
            // gigabytes) and the save aborted. One text shared by every master is the model's memory (100 MB); the trees the codec would make of it are twelve.
            let big: String = "x".repeat(100 << 20);
            let mut kept = diacraft_doc::Kept::default();
            kept.raw.push("k", &big);
            drop(big);
            let o = crate::docfile::base();
            let mut doc = o.doc.clone();
            for id in 1..=12 {
                let mut s = Shape::new(1);
                s.ext.make_mut().odf.kept = kept.clone();
                doc.masters.push(Arc::new(Master { id, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }));
            }
            let mut work = WriteWork::default();
            let saved = write_counted(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!("\nRESULT {scenario} class={class} held={} trees={} VmHWM={}KiB", work.docfile_held, work.docfile_trees, status_kib("VmHWM"));
            assert_eq!(class, "write_limit", "{scenario}: refused as a limit, not aborted");
            assert_eq!(work.docfile_trees, 0, "no tree was made");
        }
        "a_file_of_3900000_masters" => {
            // A pass that gave every master a declaration before any was charged took 8.4 seconds and 1.6 GB for these, where the file takes 0.5 GB.
            let file = crate::docfile_cost::file_of(&r#"<diacraft:master/>"#.repeat(3_900_000));
            let package = crate::docfile_cost::package_with(&file);
            drop(file);
            let (read, report) = diacraft_odg::read_partial(&package, &Limits::default());
            let class = read.as_ref().err().map_or("read", |e| e.class());
            let peak = status_kib("VmHWM");
            println!("\nRESULT {scenario} class={class} items={} VmHWM={peak}KiB", report.work.items);
            assert!(peak < 1_000_000, "{scenario}: {peak} KiB resident at the peak");
        }
        "a_large_master_declared" | "a_large_master_undeclared" => {
            // And the scan of this task's commits: a master without a declaration of the namespace of its own (the root declares it for its children) was
            // given one through a deep copy of its whole subtree that no charge paid for. The same master, with and without the declaration, must read to the same peak.
            let shapes: String = (1..=400_000u32).map(|k| format!(r#"<diacraft:shape id="{k}"/>"#)).collect();
            let declared =
                if scenario.ends_with("undeclared") { String::new() } else { format!(r#" xmlns:diacraft="{}""#, diacraft_doc::DIACRAFT_URN) };
            let file = crate::docfile_cost::file_of(&format!(r#"<diacraft:master id="1"{declared}>{shapes}</diacraft:master>"#));
            drop(shapes);
            let package = crate::docfile_cost::package_with(&file);
            drop(file);
            let (read, report) = diacraft_odg::read_partial(&package, &Limits::default());
            let doc = read.unwrap_or_else(|e| panic!("{scenario}: {} ({e})", e.class()));
            let held = doc.masters.get(0).map_or(0, |m| m.shapes.len());
            println!(
                "\nRESULT {scenario} masters={} shapes={held} warnings={} VmHWM={}KiB",
                doc.masters.len(),
                report.warnings.len(),
                status_kib("VmHWM")
            );
            assert_eq!((doc.masters.len(), held, report.warnings.len()), (1, 400_000, 0), "{scenario}");
        }
        "four_million_masters_of_id_0" => {
            // The report, concern 5: a model of four million masters of id 0 is no file the reader reads (it reads 65,536), and the save says so, in words, before any
            // tree of the file is made: no master's tree, no memory beside the model but the model's own.
            let o = crate::docfile::base();
            let mut doc = o.doc.clone();
            let bare = Arc::new(Master::default());
            for _ in 0..4_000_000 {
                doc.masters.push(Arc::clone(&bare));
            }
            let mut work = WriteWork::default();
            let saved = write_counted(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            let words = saved.as_ref().err().map(ToString::to_string).unwrap_or_default();
            println!("\nRESULT {scenario} class={class} trees={} VmHWM={}KiB {words}", work.docfile_trees, status_kib("VmHWM"));
            assert_eq!(class, "write_limit", "{scenario}: {words}");
            assert!(words.contains("masters in DiaCraft's document file"), "{scenario}: {words}");
            assert_eq!(work.docfile_trees, 0, "{scenario}: no tree was made");
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact docfile_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

#[test]
fn a_master_of_300000_shapes_is_saved_under_the_address_limit() {
    let (ended_well, stdout) = run_child("a_master_of_300000_shapes", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn masters_of_1200000_cells_are_refused_as_a_limit_before_a_tree_is_made_and_do_not_abort() {
    let (ended_well, stdout) = run_child("a_master_of_1200000_cells", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_document_file_of_3900000_shapes_is_refused_as_a_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("a_file_of_3900000_shapes", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_model_of_four_million_masters_is_refused_in_words_before_a_tree_is_made_and_does_not_abort() {
    let (ended_well, stdout) = run_child("four_million_masters_of_id_0", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn masters_that_pass_the_readers_items_are_refused_as_a_limit_and_do_not_abort() {
    let (ended_well, stdout) = run_child("masters_of_60_shapes_to_the_budget", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn masters_that_hold_100mb_of_kept_text_each_are_refused_as_a_limit_by_the_gate_and_do_not_abort() {
    let (ended_well, stdout) = run_child("twelve_masters_of_100mb_of_kept_text", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_document_file_of_3900000_masters_reads_in_the_memory_of_its_tree_and_no_more() {
    let (ended_well, stdout) = run_child("a_file_of_3900000_masters", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// The peak (`VmHWM`, KiB) a child printed.
fn peak_of(stdout: &str) -> u64 {
    let at = stdout.find("VmHWM=").unwrap_or_else(|| panic!("no VmHWM in {stdout}")) + "VmHWM=".len();
    stdout[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or_else(|_| panic!("no number in {stdout}"))
}

/// A large master with no declaration of the namespace of its own reads to the peak of the same master that has it, under the address limit: the declaration is one attribute put
/// in place, and no copy of the master's subtree is made (a copy of 400,000 shapes is a second tree of 60 MB or more that nothing charged).
#[test]
fn a_large_master_without_its_own_declaration_reads_to_the_peak_of_one_with_it() {
    let (a_well, declared) = run_child("a_large_master_declared", 3_000_000);
    let (b_well, undeclared) = run_child("a_large_master_undeclared", 3_000_000);
    assert!(a_well && b_well, "a child aborted, or its assertion failed: {declared} {undeclared}");
    let (with, without) = (peak_of(&declared), peak_of(&undeclared));
    println!("peak with the declaration {with} KiB, without {without} KiB");
    assert!(without <= with + with / 20 + 4_096, "{without} KiB without the declaration against {with} KiB with it: the master was copied");
}
