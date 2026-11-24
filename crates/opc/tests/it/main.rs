//! The `opc` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code: by hand from (its vectors carry their id, `V1`...) or as a hostile case
//! that no honest writer makes (`zipbuild.rs`). No file from the reference app, or from anyone, is read. Tests may unwrap and panic: a failing
//! assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod content_types;
mod directory;
mod errors;
mod fuzz;
mod limits;
mod memory;
mod names;
mod package;
mod par_read;
mod rels;
mod write;
mod xml_hostile;
mod xml_tree;
mod zip_hostile;
mod zipbuild;

use diacraft_opc::{Limits, OpcError, Package};
use zipbuild::{Item, build};

/// `[Content_Types].xml` as text, from `Default` rows and `Override` rows.
pub fn content_types_xml(defaults: &[(&str, &str)], overrides: &[(&str, &str)]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
    );
    for (ext, ct) in defaults {
        s.push_str(&format!("<Default Extension=\"{ext}\" ContentType=\"{ct}\"/>"));
    }
    for (part, ct) in overrides {
        s.push_str(&format!("<Override PartName=\"{part}\" ContentType=\"{ct}\"/>"));
    }
    s.push_str("</Types>");
    s
}

/// A relationships part as text: rows of `(Id, Type, Target, TargetMode)`.
pub fn rels_xml(rows: &[(&str, &str, &str, Option<&str>)]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (id, ty, target, mode) in rows {
        match mode {
            Some(m) => s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\" TargetMode=\"{m}\"/>")),
            None => s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"/>")),
        }
    }
    s.push_str("</Relationships>");
    s
}

/// The two items every package has: the content types stream (a `rels` and an `xml` default, and nothing else) first.
pub fn base_items() -> Vec<Item> {
    vec![Item::deflated(
        "[Content_Types].xml",
        content_types_xml(
            &[("rels", "application/vnd.openxmlformats-package.relationships+xml"), ("xml", "application/xml")],
            &[("/visio/document.xml", "application/vnd.ms-visio.drawing.main+xml")],
        )
        .as_bytes(),
    )]
}

/// A small package: the base items, then `more`.
pub fn package_with(more: Vec<Item>) -> Vec<u8> {
    let mut items = base_items();
    items.extend(more);
    build(&items)
}

/// Read `items` (the content types stream is NOT added) with the default limits.
pub fn read_items(items: &[Item]) -> Result<Package, OpcError> {
    Package::read(&build(items), &Limits::default())
}

/// The class of the error reading `bytes` gives, with `limits`. Panics if it reads.
pub fn class_of(bytes: &[u8], limits: &Limits) -> &'static str {
    match Package::read(bytes, limits) {
        Ok(p) => panic!("expected an error and read {} parts: {p:?}", p.parts().count()),
        Err(e) => e.class(),
    }
}

/// The class of the error reading a package made of the base items and `more`, with the default limits.
pub fn class_of_more(more: Vec<Item>) -> &'static str {
    class_of(&package_with(more), &Limits::default())
}
