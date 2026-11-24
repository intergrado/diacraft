//! Determinism (Review Focus 5, the public spec's 2.9): the same model gives the same bytes, in this process and in another. A `HashMap` or a pointer order in any encoder passes
//! every test that runs in one process, since a process has one random state: so a child process writes the drawing, and the parent compares its digest with its own and with a
//! digest pinned at the vocabulary freeze.
use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Document, Page, Shape, ShapeList};
use diacraft_native::{Previous, State, WriteOptions, write};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};

use crate::util::*;

const SCENARIO: &str = "NATIVE_DETERMINISM_SCENARIO";

/// The testkit's drawing of 50 shapes and 3 pictures, with a master: shapes with cells and a text, pictures shared by some and shown by others, held in an order that no
/// two runs could vary.
fn drawing() -> Document {
    let pictures: Vec<Arc<[u8]>> = (1..=3u8).map(|i| png(i, 200)).collect();
    let mut pages = Vec::new();
    for p in 1..=3u32 {
        let mut page = Page::new(p, &format!("Page-{p}"));
        let mut shapes = Vec::new();
        for i in 1..=17u32 {
            let id = p * 100 + i;
            let shape = if i % 6 == 0 {
                picture_shape_of(id, &format!("media/image{}.png", i % 3), &pictures[(i as usize / 6 + p as usize) % 3])
            } else {
                let mut shape = Shape::new(id);
                shape.name = Some(Arc::from(format!("Shape {id}").as_str()));
                let mut sheet = diacraft_doc::Sheet::default();
                for (n, v) in [("PinX", i.to_string()), ("PinY", (i * 2).to_string()), ("Width", "1".to_owned()), ("Height", "0.5".to_owned())] {
                    let mut cell = diacraft_doc::Cell::new();
                    cell.value = Some(diacraft_doc::Value::from_text(&v));
                    sheet.cells.insert(diacraft_doc::CellName::new(n), cell);
                }
                shape.sheet = Arc::new(sheet);
                shape
            };
            shapes.push(Arc::new(shape));
        }
        page.shapes = ShapeList::from_vec(shapes);
        pages.push(page);
    }
    let mut doc = drawing_of(pages);
    doc.masters = diacraft_doc::SharedVec::from_vec(vec![Arc::new(master(1, "Box"))]);
    doc
}

/// The first model a fixed generator makes of each kind: the same in every process.
fn generated() -> Vec<Document> {
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        proptest::test_runner::TestRng::from_seed(proptest::test_runner::RngAlgorithm::ChaCha, &[7u8; 32]),
    );
    (0..6)
        .filter_map(|_| any_document(ModelSize::Medium).new_tree(&mut runner).ok().map(|t| t.current()))
        .filter(|d| d.parts.iter().all(|p| p.format != diacraft_doc::Format::Native))
        .collect()
}

/// The digest of every drawing this test writes, joined: what a child prints and the parent compares.
fn digests() -> String {
    let mut out = Vec::new();
    out.push(sha256_hex(&write_doc(&drawing())));
    for doc in generated() {
        match write(&doc, &State::default(), None, &WriteOptions::default()) {
            Ok(w) => out.push(sha256_hex(&w.bytes)),
            Err(e) => out.push(format!("refused:{}", e.class())),
        }
    }
    out.join(" ")
}

#[test]
fn sha256_is_the_standard_one() {
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(&[b'a'; 1_000]), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
}

#[test]
#[ignore = "run by `two_processes_write_the_same_bytes`; alone it needs the scenario in the environment"]
fn child() {
    assert_eq!(std::env::var(SCENARIO).as_deref(), Ok("write"));
    println!("DIGESTS {}", digests());
}

/// The digests of the 50-shape drawing, and the first model the fixed generator makes, as written at the vocabulary freeze of 1.1. A change here is a change of the bytes a
/// model gives: of the vocabulary, the order or the deflate of a part, and it is made on purpose, with the test that says why. **APPID moved it** (the digest of `c65b86151acc1efb`
/// before): every part names the namespace of the domain `diacraft.app` in its root, which is shorter than the one before, and nothing else of the bytes changed (the stamps that the
/// parts' content gives are the ones they gave, `vectors.rs`).
const PINNED_DRAWING: &str = "defd6ec513128578bfb6e9bf7fcc3b2abf29bb24ee01687c3065e4d7eaff6e5f";

#[test]
fn two_processes_write_the_same_bytes() {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let mut seen = Vec::new();
    for _ in 0..2 {
        let out = Command::new(&exe)
            .args(["--exact", "determinism::child", "--ignored", "--nocapture", "--test-threads=1"])
            .env(SCENARIO, "write")
            .output()
            .expect("run the child");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "the child failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        let line = stdout.lines().find_map(|l| l.split_once("DIGESTS ").map(|(_, rest)| rest)).expect("a DIGESTS line").to_owned();
        seen.push(line);
    }
    let mine = digests();
    assert_eq!(seen[0], seen[1], "two children of one binary");
    assert_eq!(seen[0], mine, "a child and its parent write the same bytes");
    let first = mine.split(' ').next().unwrap();
    assert_eq!(first, PINNED_DRAWING, "the bytes of the 50-shape drawing, pinned (all digests: {mine})");
}

#[test]
fn the_same_model_gives_the_same_bytes_in_one_process_whatever_its_allocations() {
    let a = write_doc(&drawing());
    let b = write_doc(&drawing());
    assert_eq!(a, b);
    let (doc, opened) = open_ok(&a);
    let again = write(&doc, &opened.state, Some(&Previous { bytes: &a, decoded: &opened.decoded }), &WriteOptions::default()).unwrap();
    assert_eq!(again.bytes, a);
    let mut doc2 = drawing();
    doc2.title = Arc::from("T");
    assert_ne!(write_doc(&doc2), a, "a changed model is a changed file");
}
