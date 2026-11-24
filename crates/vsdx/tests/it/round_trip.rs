//! A save with nothing edited, over hostile input: any package that reads must save to a package that holds every part it had, byte for
//! byte but for the four the specs name, in every one of the six formats, or the save must be
//! refused with a word. It must never panic, never hang, and never hand back a package that lost something.
//!
//! The inputs are the reader's fuzz seed (`fuzz.rs`) made each of the six types, with a macro project in the macro-enabled ones, and with one or
//! two of its items (any part, any relationships part, the content types) mutated and the archive written again with right checksums. **A clean
//! run proves little about cost** (`timing.rs` builds the inputs that do): it proves what a no-edit save does with the shapes of damage a
//! mutation reaches.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{Pkg, ZipItem, zip};
use diacraft_vsdx::{OriginalBytes, Variant, WriteOptions, read, write_with};
use proptest::prelude::*;

use crate::fuzz::{Gen, mutate, seed_builder};

const EXTENSIONS: [&str; 6] = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"];

/// The seed package of the variant `ext`: the macro-enabled ones hold a project and two signature parts.
pub(crate) fn seed(ext: &str) -> Pkg {
    let variant = Variant::from_extension(ext).unwrap();
    let mut builder = seed_builder().document_type(variant.content_type());
    if variant.macro_enabled {
        builder = builder.with(
            Pkg::new()
                .default_type("bin", "application/vnd.ms-office.vbaProject")
                .part_without_override("/visio/vbaProject.bin", b"vba bytes, never run")
                .rel("/visio/document.xml", "rId50", "http://schemas.microsoft.com/office/2006/relationships/vbaProject", "vbaProject.bin")
                .part("/visio/vbaProjectSignature.bin", "application/vnd.ms-office.vbaProjectSignature", b"sig")
                .rel(
                    "/visio/vbaProject.bin",
                    "rId1",
                    "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature",
                    "vbaProjectSignature.bin",
                ),
        );
    }
    builder.pkg()
}

/// Every item of a package as (name, bytes): the content types stream first, then every part, relationships parts included.
pub(crate) fn flat(pkg: &Pkg) -> Vec<(String, Vec<u8>)> {
    let built = pkg.build();
    let read = Package::read(&built, &Limits::default()).unwrap();
    let mut out = vec![("[Content_Types].xml".to_owned(), pkg.content_types_xml().into_bytes())];
    out.extend(read.parts().map(|name| (name.trim_start_matches('/').to_owned(), read.part(name).unwrap().to_vec())));
    out
}

pub(crate) fn zipped(items: &[(String, Vec<u8>)]) -> Vec<u8> {
    zip(&items.iter().map(|(name, bytes)| ZipItem::deflated(name, bytes)).collect::<Vec<_>>())
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

/// What a save of `bytes` with nothing edited must do, if `bytes` reads: `target` is the type it is saved as (`None`: the type it has). Returns a
/// message when it did something else.
pub(crate) fn check(bytes: &[u8], target: Option<Variant>) -> Result<(), String> {
    let Ok((doc, _)) = read(bytes, &diacraft_vsdx::Limits::default()) else { return Ok(()) };
    let started = Instant::now();
    // The macro project is dropped when the type cannot hold one, as a person who chose to would have it.
    let options = WriteOptions { now: Some(1_700_000_000), variant: target, drop_macros: true, ..WriteOptions::default() };
    let written = write_with(&doc, Some(&OriginalBytes { bytes, baseline: None }), &options)
        .map_err(|e| format!("a package that reads was refused a save: {} ({e})", e.class()))?;
    let took = started.elapsed();
    // A hang guard and no measure of cost: a save of a package the fuzz makes takes milliseconds, and a minute is generous at load 50.
    if took > Duration::from_secs(60) {
        return Err(format!("a package of {} bytes took {took:?} to save", bytes.len()));
    }
    let (before, after) = (parts(bytes), parts(&written.bytes));
    let leaves_macros = target.is_some_and(|t| !t.macro_enabled);
    // **Names.** A part the save keeps is looked for under the name the file gave it, spelt as the file spelt it: OPC takes two names that differ only in ASCII case for
    // one, and the writer keeps the case each part arrived with, so a part kept under another spelling is one gone and one new. The names this
    // check knows (the parts a change of type touches, the folder of the pages a stencil's pages get parts in) are compared as OPC compares them, in any case
    // (the folder was compared as written, and a page part of a file that spelt it otherwise would have been "new").
    // Beyond the writer's name, a change of type changes: the properties a stamp or an edit touches, the relationships and rows that a dropped
    // project or a stencil's page parts touch.
    let changes: &[&str] = if target.is_some() {
        &[
            "/docProps/app.xml",
            "/docProps/core.xml",
            "/_rels/.rels",
            "/visio/_rels/document.xml.rels",
            "/visio/pages/pages.xml",
            "/visio/pages/_rels/pages.xml.rels",
        ]
    } else {
        &["/docProps/app.xml"]
    };
    for (name, old) in &before {
        let lower = name.to_ascii_lowercase();
        let thumbnail_goes = target.is_some() && lower == "/docprops/thumbnail.emf";
        match after.get(name) {
            None if (leaves_macros && lower.contains("vbaproject")) || thumbnail_goes => {}
            None => return Err(format!("{name} is gone after a save with nothing edited")),
            Some(new) => {
                if new != old && !changes.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                    return Err(format!("{name} changed in a save with nothing edited"));
                }
            }
        }
    }
    for name in after.keys() {
        if !before.contains_key(name) && !(target.is_some() && name.to_ascii_lowercase().starts_with("/visio/pages/")) {
            return Err(format!("{name} is new after a save with nothing edited"));
        }
    }
    if target.is_none() {
        check_content_types(bytes, &written.bytes, written.macros_dropped.is_empty())?;
    }
    Ok(())
}

/// The content types of a save with nothing edited, in the type the file has: every row of the file is still there, none is added for a
/// part that has a type, and when every part had a type and no project was dropped the stream is the file's bytes.
fn check_content_types(before: &[u8], after: &[u8], nothing_dropped: bool) -> Result<(), String> {
    let (old, new) =
        (Package::read(before, &Limits::default()).map_err(|e| e.to_string())?, Package::read(after, &Limits::default()).map_err(|e| e.to_string())?);
    let (a, b) = (old.content_types(), new.content_types());
    if let Some(row) = a.defaults.iter().chain(&a.overrides).find(|row| !b.defaults.contains(row) && !b.overrides.contains(row)) {
        return Err(format!("the content types row {row:?} is gone after a save with nothing edited"));
    }
    let all_typed = old.parts().all(|name| old.content_type(name).is_some());
    if all_typed && nothing_dropped {
        if old.content_types_bytes() != new.content_types_bytes() {
            return Err("every part had a type, and the content types stream was not written as it was".to_owned());
        }
    } else if nothing_dropped {
        // Rows were added for parts that had no type, and only those: one `Default` for the relationships parts, an `Override` for each of the others.
        let added = (b.defaults.len() + b.overrides.len()).saturating_sub(a.defaults.len() + a.overrides.len());
        let untyped = old.parts().filter(|name| old.content_type(name).is_none()).count();
        if added > untyped {
            return Err(format!("{added} rows added to the content types for {untyped} parts with no type"));
        }
    }
    Ok(())
}

/// How many cases to run: 400, or what `DIACRAFT_ROUND_TRIP_CASES` says (a longer run by hand).
fn cases() -> u32 {
    std::env::var("DIACRAFT_ROUND_TRIP_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(400)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    /// One or two items mutated, a save as the type the file has and as another, and what comes out is the file that went in.
    #[test]
    fn a_save_of_anything_that_reads_keeps_what_it_had(seed_value in any::<u64>()) {
        let mut g = Gen(seed_value | 1);
        let from = EXTENSIONS[g.below(EXTENSIONS.len())];
        let to = EXTENSIONS[g.below(EXTENSIONS.len())];
        let mut items = flat(&seed(from));
        let n = items.len();
        for _ in 0..1 + g.below(2) {
            let at = g.below(n);
            let mut inner = Gen(g.next() | 1);
            if let Some((_, bytes)) = items.get_mut(at) {
                mutate(&mut inner, bytes);
            }
        }
        let bytes = zipped(&items);
        for target in [None, Variant::from_extension(to)] {
            if let Err(message) = check(&bytes, target) {
                prop_assert!(false, "{from} to {}: {message}", target.map_or("itself", Variant::extension));
            }
        }
    }
}

/// Reproduces one seed of the property above, for a failure it printed: `DIACRAFT_ROUND_TRIP_SEED=123 cargo test ... reproduce -- --ignored`.
#[test]
#[ignore = "a debugging aid, run by hand with a seed"]
fn reproduce() {
    let Some(seed_value) = std::env::var("DIACRAFT_ROUND_TRIP_SEED").ok().and_then(|v| v.parse::<u64>().ok()) else { return };
    let mut g = Gen(seed_value | 1);
    let from = EXTENSIONS[g.below(EXTENSIONS.len())];
    let to = EXTENSIONS[g.below(EXTENSIONS.len())];
    let mut items = flat(&seed(from));
    let n = items.len();
    for _ in 0..1 + g.below(2) {
        let at = g.below(n);
        let mut inner = Gen(g.next() | 1);
        if let Some((name, bytes)) = items.get_mut(at) {
            let before = String::from_utf8_lossy(bytes).into_owned();
            mutate(&mut inner, bytes);
            eprintln!("mutated {name}:\n  was {before}\n  now {}", String::from_utf8_lossy(bytes));
        }
    }
    let bytes = zipped(&items);
    let read = read(&bytes, &diacraft_vsdx::Limits::default());
    eprintln!("{from} to {to}: reads: {:?}", read.as_ref().map(|(_, r)| (&r.warnings, &r.variant)));
    for target in [None, Variant::from_extension(to)] {
        eprintln!("{:?}", check(&bytes, target));
    }
}
