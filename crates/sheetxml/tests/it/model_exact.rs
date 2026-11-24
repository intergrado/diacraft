//! The codec's contract: `decode(encode(m)) == m` over generated models, through the XML bytes and the real reader, and an encoding that is a fixed
//! point once a file has been read and written (the design's 2.3). The cases come from `MODEL_CASES` (64 by default); the loop the done-criteria ask for runs 4,096 a round, with a fresh
//! `PROPTEST_RNG_SEED` each round, under plain `cargo test` (an explicit `cases:` beats `PROPTEST_CASES`).
use super::model_diff::first_difference;
use super::model_util::*;
use diacraft_doc::{Document, Passthrough, Placed};
use diacraft_sheetxml::model::vocab;
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;

fn cases_from_env(name: &str, default: u32) -> u32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases_from_env("MODEL_CASES", 64), ..ProptestConfig::default() })]

    #[test]
    fn decode_of_encode_is_the_model(m in any_document(ModelSize::Small)) {
        let back = round_trip(&m);
        prop_assert!(back == m, "{}", first_difference(&back, &m).unwrap_or_default());
    }

    /// A file is the same file once it has been read and written: the first read shares by their text the units the model held apart (equal, and not one allocation),
    /// and the write then says each of them once, so that the encoding of what was read is a fixed point, and so is the model. (The first write may spell units alike
    /// that the first read found different, which the second read then shares: the second and the third encodings are the same.)
    #[test]
    fn encoding_is_a_fixed_point_after_one_read(m in any_document(ModelSize::Small)) {
        let first = pack(&m);
        let (back, _) = unpack(&first).unwrap();
        let second = pack(&back);
        let (again, _) = unpack(&second).unwrap();
        let third = pack(&again);
        prop_assert_eq!(&second.document, &third.document);
        prop_assert_eq!(&second.pages, &third.pages);
        prop_assert_eq!(&second.masters, &third.masters);
        prop_assert!(again == back, "{}", first_difference(&again, &back).unwrap_or_default());
        // And what the first write said is the model's own: what it is read as is the model.
        prop_assert!(back == m, "{}", first_difference(&back, &m).unwrap_or_default());
    }

    /// A place beyond the known children a write puts down is closed up by it, and the document is then exact: the first read after a write is a fixed point.
    #[test]
    fn places_of_unknown_elements_settle_in_one_write(m in any_document(ModelSize::Small), places in prop::collection::vec(0u32..9, 1..6)) {
        let mut m = m;
        let nodes: Vec<Placed> = m
            .ext
            .make_mut()
            .native
            .kept
            .iter()
            .enumerate()
            .map(|(i, p)| Placed { after: places[i % places.len()], node: p.node.clone() })
            .collect();
        m.ext.make_mut().native.kept = Passthrough::from_nodes(nodes);
        let once = round_trip(&m);
        let twice = round_trip(&once);
        prop_assert!(twice == once, "{}", first_difference(&twice, &once).unwrap_or_default());
        prop_assert_eq!(pack(&once).document, pack(&twice).document);
    }

    #[test]
    fn every_element_and_attribute_written_is_in_the_vocabulary_and_none_holds_its_default(m in any_document(ModelSize::Small)) {
        let packed = pack(&m);
        for bytes in packed.all_bytes() {
            let root = diacraft_container::xml::parse_with(bytes, &native_limits()).unwrap();
            check_vocabulary(&root)?;
        }
    }
}

/// Every element of the tree is one the table has, under a parent the table allows, with attributes the table has and none at its default; no element
/// but the root carries a declaration, and every name is under the prefix. What a newer build wrote (an element of another prefix with all that is in it, an
/// attribute that is `future-` something or has a prefix, and the declaration of its prefix) is the one thing the table does not name: it is written back as it
/// was read.
fn check_vocabulary(root: &diacraft_container::xml::El) -> Result<(), TestCaseError> {
    let mut stack = vec![(root, "part root".to_owned())];
    while let Some((e, parent)) = stack.pop() {
        if e.prefix() != diacraft_sheetxml::model::PREFIX {
            // Unknown content in its own namespace: written as it was, and its descendants with it.
            prop_assert!(e.attrs.iter().any(|(k, _)| k.starts_with("xmlns:")) || e.prefix().is_empty(), "{} has no declaration", e.name);
            continue;
        }
        let local = e.local().to_owned();
        let def = vocab::element(&local);
        prop_assert!(def.is_some(), "element {local} is not in the vocabulary");
        let def = def.unwrap();
        prop_assert!(local == vocab::el::LONG || def.inside.contains(&parent.as_str()), "{local} stands in {parent}");
        for (k, v) in &e.attrs {
            if k.starts_with("xmlns") {
                prop_assert!(parent == "part root" || k == "xmlns:q9", "a declaration below the root: {k} on {local}");
                continue;
            }
            if k.starts_with("future-") || k.contains(':') {
                continue;
            }
            let a = vocab::attribute(&local, k);
            prop_assert!(a.is_some(), "attribute {k} of {local} is not in the vocabulary");
            if let Some(default) = a.unwrap().default {
                prop_assert!(v != default || k.ends_with("-b64"), "{local}@{k} is written at its default {default:?}");
            }
        }
        for child in e.elements() {
            stack.push((child, local.clone()));
        }
    }
    Ok(())
}

#[test]
fn a_blank_document_is_a_fixed_point_and_comes_back() {
    let d = Document::new_blank();
    assert_eq!(round_trip(&d), d);
}

/// Writes seeds for the fuzz target `model_decode` into `$MODEL_SEED_DIR`: for each of a few generated documents, its document part behind a byte of 0, a page
/// part behind a 1 and a master part behind a 2. Run by hand when the vocabulary changes: `MODEL_SEED_DIR=<dir> cargo test ... write_fuzz_seeds -- --ignored`.
#[test]
#[ignore = "writes files; run by hand with MODEL_SEED_DIR set"]
fn write_fuzz_seeds() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let dir = std::path::PathBuf::from(std::env::var("MODEL_SEED_DIR").expect("MODEL_SEED_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[7u8; 32]));
    let strategy = any_document(ModelSize::Small);
    let mut written = 0;
    for n in 0..60 {
        let doc = strategy.new_tree(&mut runner).unwrap().current();
        let p = pack(&doc);
        let mut put = |kind: u8, bytes: &[u8]| {
            let mut seed = vec![kind];
            seed.extend_from_slice(bytes);
            std::fs::write(dir.join(format!("seed-{n:03}-{written:04}")), seed).unwrap();
            written += 1;
        };
        put(0, &p.document);
        if let Some(page) = p.pages.first() {
            put(1, page);
        }
        if let Some(master) = p.masters.first() {
            put(2, master);
        }
    }
}

/// The digest of every part of a generated model, in the order a package stores them: what two processes must agree on.
fn fixed_model_digest() -> String {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[11u8; 32]));
    let strategy = any_document(ModelSize::Medium);
    let mut digest = diacraft_sheetxml::model::Digest::new("processes");
    for _ in 0..8 {
        let doc = strategy.new_tree(&mut runner).unwrap().current();
        let p = pack(&doc);
        for part in p.all_bytes() {
            digest.bytes(part);
        }
        for id in 0..u32::try_from(p.blobs.len()).unwrap() {
            digest.bytes(p.blobs.entry(diacraft_sheetxml::model::BlobId(id)).unwrap().0);
        }
    }
    digest.finish().text()
}

#[test]
#[ignore = "run by the test below in a process of its own"]
fn child_digest() {
    println!("DIGEST {}", fixed_model_digest());
}

/// No `HashMap` or pointer order reaches the encoding: a child process, with its own hash seeds and addresses, writes the same bytes.
#[test]
fn two_processes_encode_the_same_bytes() {
    let mine = fixed_model_digest();
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "model_exact::child_digest", "--ignored", "--nocapture", "--test-threads=1"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let theirs = text
        .lines()
        .find_map(|l| l.split_once("DIGEST ").map(|(_, d)| d.trim()))
        .unwrap_or_else(|| panic!("no digest from the child: {text}\n{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(mine, theirs);
}

/// Models the repository's own code makes (every built-in master, each template, the rich VSDX package as the reader gives it, a session's drawing): what a
/// reader or a command makes and no generator does. They come back as they were, but for the derived data of their themes, which is never stored.
#[test]
fn documents_the_repository_makes_come_back() {
    use diacraft_testkit::model::{fixture_documents, without_derived_theme_data};
    let documents = fixture_documents();
    assert!(documents.len() >= 6, "{} fixtures", documents.len());
    for (name, doc) in &documents {
        let stripped = without_derived_theme_data(doc);
        let back = round_trip(doc);
        assert!(back == stripped, "{name}: {}", first_difference(&back, &stripped).unwrap_or_default());
        assert!(round_trip(&back) == back, "{name} again");
        // A fixed point of the bytes, too.
        assert_eq!(pack(&back).document, pack(&stripped).document, "{name}");
    }
    assert!(documents.iter().any(|(n, d)| n.starts_with("template") && !d.themes.is_empty()), "a template with a theme is among them");
    assert!(documents.iter().any(|(_, d)| d.masters.len() > 20), "the built-in masters are among them");
}
