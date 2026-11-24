//! **An `.odg` read on threads is the `.odg` read on one** (`Limits::threads`): the files of the package that the drawing names or keeps are inflated on threads, each held to the size the directory
//! states for it, and the document, the report and every refusal (the media budget, a damaged file) are the single-threaded read's at 2, 7 and 31 threads.

use diacraft_odg::{Limits, OdgError, ReadReport, read_partial};

use crate::build::{DRAWING, files, package, shapes};

const THREADS: [usize; 4] = [1, 2, 7, 31];

/// Bytes that deflate to a part of their size, from a seed.
fn bytes(len: usize, mut seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        out.extend(seed.to_le_bytes().iter().map(|b| b & 0x0F));
    }
    out.truncate(len);
    out
}

/// A drawing of 40 shapes and `kept` files of `sizes` bytes that nothing in it reads (the package keeps them in the model), half of them stored (`.png`) and half deflated.
fn drawing(sizes: &[usize]) -> Vec<(String, Vec<u8>)> {
    let mut parts = files(&crate::build::page("Page 1", &shapes(40)));
    for (k, size) in sizes.iter().enumerate() {
        let name = if k % 2 == 0 { format!("Pictures/p{k}.png") } else { format!("Objects/o{k}.dat") };
        parts.push((name, bytes(*size, k as u64 + 1)));
    }
    parts
}

type Outcome = (Result<diacraft_doc::Document, OdgError>, ReadReport);

fn read_on(file: &[u8], limits: &Limits, threads: usize) -> Outcome {
    read_partial(file, &Limits { threads, ..limits.clone() })
}

/// The reads at every count of threads are the read at one: the document, the report and the error.
fn assert_alike(file: &[u8], limits: &Limits, what: &str) -> Outcome {
    let one = read_on(file, limits, 1);
    for threads in THREADS.iter().copied().filter(|t| *t > 1) {
        let many = read_on(file, limits, threads);
        assert_eq!(many.0, one.0, "{what}: the result at {threads} threads");
        assert_eq!(many.1, one.1, "{what}: the report at {threads} threads");
    }
    one
}

const SIZES: [usize; 12] = [300_000, 90_000, 2_000, 400_000, 70_000, 100, 150_000, 80_000, 3_000, 260_000, 66_000, 500];

#[test]
fn a_package_that_keeps_many_files_is_the_same_document_and_report_at_any_number_of_threads() {
    let file = package(DRAWING, drawing(&SIZES));
    let (doc, report) = assert_alike(&file, &Limits::default(), "12 kept files");
    let doc = doc.expect("the drawing reads");
    assert_eq!(doc.parts.iter().filter(|p| p.name.contains("Pictures/") || p.name.contains("Objects/")).count(), SIZES.len(), "every file is kept");
    assert!(report.stats.shapes > 0);
}

#[test]
fn a_damaged_file_is_left_out_and_named_alike_at_any_number_of_threads() {
    // A stored file whose bytes are not its checksum's: it is not kept and the read goes on, whichever thread found it.
    let mut parts = drawing(&SIZES);
    let marked: Vec<u8> = (0..150_000u32).map(|i| (i % 251) as u8).collect();
    parts.push(("Pictures/damaged.png".to_owned(), marked.clone()));
    parts.push(("Pictures/after.png".to_owned(), bytes(120_000, 99)));
    let mut file = package(DRAWING, parts);
    let at = file.windows(64).position(|w| w == &marked[1000..1064]).expect("the stored bytes are in the file");
    file[at] ^= 0xFF;
    let (doc, report) = assert_alike(&file, &Limits::default(), "a damaged file");
    let doc = doc.expect("a damaged file nothing reads does not stop the read");
    assert!(report.warning_counts.get("part_not_kept").copied().unwrap_or(0) >= 1, "it is named: {:?}", report.warning_counts);
    assert!(doc.parts.iter().any(|p| p.name.contains("after.png")), "the files after it are kept");
    assert!(!doc.parts.iter().any(|p| p.name.contains("damaged.png")));
}

#[test]
fn the_media_budget_refuses_at_the_same_file_and_gives_back_what_a_damaged_one_took_at_any_number_of_threads() {
    // 100 KB files under a budget of 250 KB: with none damaged the third does not fit; with the second damaged its 100 KB are given back, so three files are read (the third fits) and four are not
    // (the fourth does not). The budget is held to as one file at a time holds it, whichever thread inflated which.
    for (count, damage, reads) in [(3usize, false, false), (3, true, true), (4, true, false), (2, false, true)] {
        let mut parts = drawing(&[]);
        let marked: Vec<u8> = (0..100_000u32).map(|i| (i % 249) as u8).collect();
        for k in 0..count {
            let data = if k == 1 && damage { marked.clone() } else { bytes(100_000, k as u64 + 7) };
            parts.push((format!("Pictures/p{k}.png"), data));
        }
        let mut file = package(DRAWING, parts);
        if damage {
            let at = file.windows(64).position(|w| w == &marked[2000..2064]).expect("the stored bytes are in the file");
            file[at] ^= 0xFF;
        }
        let limits = Limits { max_media_bytes: 250_000, ..Limits::default() };
        let (result, report) = assert_alike(&file, &limits, &format!("{count} files, damage {damage}"));
        if reads {
            let doc = result.unwrap_or_else(|e| panic!("{count} files, damage {damage}: {e}"));
            let kept = doc.parts.iter().filter(|p| p.name.contains("Pictures/p")).count();
            assert_eq!(kept, count - usize::from(damage), "{count} files, damage {damage}");
            assert_eq!(report.warning_counts.get("part_not_kept").copied().unwrap_or(0), u64::from(damage));
        } else {
            let error = result.expect_err("the budget refuses the file");
            assert!(error.to_string().contains("package entries kept"), "{count} files, damage {damage}: {error}");
        }
    }
}

#[test]
fn a_package_whose_manifest_or_entries_are_refused_is_refused_alike() {
    // A package that holds more than the package limit says (a lying row) and one whose part is not XML: the same class at every count.
    let mut parts = drawing(&SIZES);
    parts[0].1 = b"<office:document-content this is not xml".to_vec();
    let file = package(DRAWING, parts);
    let (result, _) = assert_alike(&file, &Limits::default(), "a content part that is not XML");
    assert!(result.is_err());
    let small =
        Limits { container: diacraft_container::Limits { max_package_bytes: 700_000, ..diacraft_container::Limits::default() }, ..Limits::default() };
    let (result, _) = assert_alike(&package(DRAWING, drawing(&SIZES)), &small, "a package past the limit");
    assert!(result.is_err());
}
