//! `diacraft-cli roundtrip` and `diacraft-cli compare` for an OpenDocument drawing.
//!
//! * `roundtrip`, before the drawing's reader exists: the package is read (`diacraft_odg::Package`), written again with every entry copied
//!   as it is through the package writer (`copy_unchanged`), and the two are compared part by part and element by element (`diff_packages`).
//!   What the copy rewrote and left out is said too (`copy_account`: `bytes_dropped` with a word for each reason,
//!   `headers_rewritten`, `rows_reordered`), so that `file_identical` false always has a stated cause.
//! * `compare`: two packages are compared by one call to `diff_packages`, so that a harness can plant an element difference
//!   and see it named.
//! * **A flat drawing (`.fodg`)** is read with the `odg` crate's own reader (`form: "flat"`, what was read, its warnings by class). `roundtrip` of one writes it again
//!   through the crate's flat writer with nothing edited, which is the file's own bytes (the flat writer), to a `--out` that is a `.fodg` (any other name is
//!   `other_type`, `want: "fodg"`); `compare` reads each side and says `parts.comparable: false` with the class `flat_not_compared`, since a flat file has no parts to compare.
//!
//! One line of JSON says what was found, with `parts_identical` and `element_diff`. The rule of the VSDX round trip holds: **no file content
//! crosses into the line**, only the hash, counts, classes, and names that the format fixes. A part is shown by
//! `diacraft_odg::part_label`: its name only when the format fixes it (`content.xml`), and otherwise its role (`picture`, `object`, `macro`,
//! `signature`, `a part`). An element or attribute is shown only by the label the comparison gives it, resolved by namespace URI against the
//! names the specs record, and `(unlisted)` otherwise; a place among siblings is a number. No message of an error is used, only its class.

use std::path::Path;

use diacraft_engine::file::{SaveError, read_file, save_file};
use diacraft_odg::package::Package;
use diacraft_odg::{
    CopyAccount, ElementPath, Form, Limits, OdgError, OriginalPackage, PackageDiff, PartDiff, ReadReport, Variant, WriteOptions, copy_account,
    copy_unchanged, diff_packages, part_label, read, sniff, write_with,
};
use serde_json::{Value, json};

use super::{MOST_LISTED, line};
use crate::args::{CompareArgs, RoundtripArgs};
use crate::oracle::sha256_hex;
use crate::{Failure, write_line};

/// Whether the file is an OpenDocument drawing for the round trip: by its extension (`.odg`, `.otg`, `.fodg`, in any case), or by what its
/// bytes say they are.
pub fn is_odg(path: &Path, bytes: &[u8]) -> bool {
    let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    matches!(extension.as_deref(), Some("odg" | "otg" | "fodg")) || sniff(bytes).is_some()
}

/// A fixed word for why a save could not be written.
fn save_word(e: &SaveError) -> &'static str {
    match e {
        SaveError::NoFolder => "no_folder",
        SaveError::Denied => "denied",
        SaveError::NotAFile => "not_a_file",
        SaveError::Link => "link",
        SaveError::DanglingLink => "dangling_link",
        SaveError::DiskFull => "disk_full",
        // A file kept in memory (the browser's): the command line never writes one, and the word is for the match to be whole.
        SaveError::NoRoom => "no_room",
        SaveError::Failed => "failed",
        // The command line puts no roots in force, so it is never refused by one; the word is for the match to be whole.
        SaveError::Refused(_) => "not_granted",
    }
}

/// The most bytes of a file read: the package cap, and room for the directory and what is not counted.
fn file_cap(limits: &Limits) -> u64 {
    limits.container.max_package_bytes.saturating_add(64 << 20)
}

pub fn run(args: &RoundtripArgs, bytes: &[u8]) -> Result<(), Failure> {
    let limits = Limits::default();
    let mut report = json!({ "sha256": sha256_hex(bytes), "bytes": bytes.len(), "format": "odg" });
    if Form::of(bytes) == Form::Flat {
        return flat_run(args, bytes, &limits, report);
    }
    let package = match Package::read(bytes, &limits) {
        Ok(package) => package,
        Err(e) => {
            report["opened"] = json!(false);
            report["refusal"] = json!({ "class": e.class() });
            return write_line(&line(&report)?);
        }
    };
    let variant = package.variant();
    report["variant"] = json!(variant.map(Variant::extension));
    report["opened"] = json!(true);
    report["read"] = read_report(&package);
    // The package holds every part inflated and two trees: none of it is needed past its report.
    drop(package);
    if args.shift.is_some() {
        // The drawing's reader does not exist yet, so there is no shape to move: the round trip is an unedited one, and says so.
        report["moved"] = json!({ "error": "not_supported" });
    }
    let want = variant.unwrap_or(Variant::Drawing).extension();
    let extension = Path::new(&args.out).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if extension.as_deref() != Some(want) {
        report["saved"] = json!(false);
        report["save_error"] = json!({ "error": "other_type", "want": want });
        return write_line(&line(&report)?);
    }
    let copy = match copy_unchanged(bytes, &limits) {
        Ok(copy) => copy,
        Err(e) => {
            report["saved"] = json!(false);
            report["save_error"] = json!({ "error": "failed", "class": e.class() });
            return write_line(&line(&report)?);
        }
    };
    if let Err(e) = save_file(Path::new(&args.out), &copy, None) {
        report["saved"] = json!(false);
        report["save_error"] = json!({ "error": save_word(&e) });
        return write_line(&line(&report)?);
    }
    drop(copy);
    report["saved"] = json!(true);
    let after = read_file(Path::new(&args.out), file_cap(&limits)).map_err(|e| Failure::Message(format!("cannot read what was saved: {e}")))?;
    report["out_bytes"] = json!(after.len());
    report["file_identical"] = json!(after == bytes);
    match copy_account(bytes, &limits) {
        Ok(account) => copy_report(&mut report, &account),
        Err(e) => report["bytes_dropped"] = json!({ "count": null, "class": e.class() }),
    }
    report["saved_read"] = match Package::read(&after, &limits) {
        Ok(saved) => read_report(&saved),
        Err(e) => json!({ "reads_back": false, "class": e.class() }),
    };
    compared(&mut report, bytes, &after, &limits);
    write_line(&line(&report)?)
}

/// What a person may be told of a flat drawing read: counts, flags, and the class of each warning (never its words). `pages` is how many pages the drawing has.
fn flat_read_report(pages: usize, report: &ReadReport) -> Value {
    let classes: Vec<&'static str> = report.warning_counts.keys().copied().take(MOST_LISTED).collect();
    json!({
        "reads_back": true,
        "form": "flat",
        "pages": pages,
        "has_macros": report.macros,
        "linked_pictures": report.linked_pictures,
        "warning_count": report.warning_counts.values().sum::<u64>(),
        "warnings": classes,
    })
}

/// The fields of a flat file's line that say what it is: `form`, and either `opened: true` with `variant` and `read`, or `opened: false` with the class of the refusal.
/// Nothing of the file's content: counts and classes. Whether the file was read.
fn flat_about(bytes: &[u8], limits: &Limits, about: &mut Value) -> bool {
    about["form"] = json!("flat");
    match read(bytes, limits) {
        Ok((doc, report)) => {
            about["opened"] = json!(true);
            about["variant"] = json!(report.variant.map(Variant::extension));
            about["read"] = flat_read_report(doc.pages.len(), &report);
            true
        }
        Err(e) => {
            flat_refused(about, &e);
            false
        }
    }
}

/// The fields of a line for a flat file the reader refused: its `form`, `opened: false` and the class of the refusal, and nothing of the file.
fn flat_refused(about: &mut Value, e: &OdgError) {
    about["form"] = json!("flat");
    about["opened"] = json!(false);
    about["refusal"] = json!({ "class": e.class() });
}

/// `roundtrip` of a flat drawing: read with the `odg` crate's reader, and written again through its flat writer with nothing edited, which is the
/// file's own bytes, so the line says whether `--out` is the same file. `--out` must be a flat file's name (`.fodg`): a flat document is not written
/// under another type's name. There is no package to copy and no part to compare, and `--move` cannot be honoured.
fn flat_run(args: &RoundtripArgs, bytes: &[u8], limits: &Limits, mut report: Value) -> Result<(), Failure> {
    let (doc, read_report) = match read(bytes, limits) {
        Ok(read) => read,
        Err(e) => {
            flat_refused(&mut report, &e);
            return write_line(&line(&report)?);
        }
    };
    report["form"] = json!("flat");
    report["opened"] = json!(true);
    report["variant"] = json!(read_report.variant.map(Variant::extension));
    report["read"] = flat_read_report(doc.pages.len(), &read_report);
    if args.shift.is_some() {
        report["moved"] = json!({ "error": "not_supported" });
    }
    let extension = Path::new(&args.out).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if extension.as_deref() != Some("fodg") {
        report["saved"] = json!(false);
        report["save_error"] = json!({ "error": "other_type", "want": "fodg" });
        return write_line(&line(&report)?);
    }
    let original = OriginalPackage { bytes, index: &read_report.index };
    let written = match write_with(&doc, Some(&original), &WriteOptions::default()) {
        Ok(written) => written,
        Err(e) => {
            report["saved"] = json!(false);
            report["save_error"] = json!({ "error": "failed", "class": e.class() });
            return write_line(&line(&report)?);
        }
    };
    if let Err(e) = save_file(Path::new(&args.out), &written.bytes, None) {
        report["saved"] = json!(false);
        report["save_error"] = json!({ "error": save_word(&e) });
        return write_line(&line(&report)?);
    }
    report["saved"] = json!(true);
    report["out_bytes"] = json!(written.bytes.len());
    report["file_identical"] = json!(written.bytes == bytes);
    report["saved_read"] = match read(&written.bytes, limits) {
        Ok((saved, saved_report)) => flat_read_report(saved.pages.len(), &saved_report),
        Err(e) => json!({ "reads_back": false, "class": e.class() }),
    };
    write_line(&line(&report)?)
}

/// `compare FIRST SECOND`: each file's hash and read, and what differs between them.
pub fn compare(args: &CompareArgs) -> Result<(), Failure> {
    let limits = Limits::default();
    let from_disk = |path: &str| read_file(Path::new(path), file_cap(&limits)).map_err(|e| Failure::Message(format!("cannot read the file: {e}")));
    let (first, second) = (from_disk(&args.first)?, from_disk(&args.second)?);
    let mut report = json!({ "format": "odg" });
    let mut opened = true;
    let mut flat = false;
    for (side, bytes) in [("first", &first), ("second", &second)] {
        let mut about = json!({ "sha256": sha256_hex(bytes), "bytes": bytes.len() });
        if Form::of(bytes) == Form::Flat {
            flat = true;
            opened &= flat_about(bytes, &limits, &mut about);
            report[side] = about;
            continue;
        }
        match Package::read(bytes, &limits) {
            Ok(package) => {
                about["opened"] = json!(true);
                about["variant"] = json!(package.variant().map(Variant::extension));
                about["read"] = read_report(&package);
            }
            Err(e) => {
                opened = false;
                about["opened"] = json!(false);
                about["refusal"] = json!({ "class": e.class() });
            }
        }
        report[side] = about;
    }
    if opened && flat {
        // A flat drawing is one XML document: it has no parts, and the comparison is of two packages' parts and elements.
        report["parts"] = json!({ "comparable": false, "class": "flat_not_compared" });
    } else if opened {
        compared(&mut report, &first, &second, &limits);
    }
    write_line(&line(&report)?)
}

/// The comparison's fields of a report: `parts_identical`, `parts` and `element_diff`, or `parts.comparable: false` with the class.
fn compared(report: &mut Value, first: &[u8], second: &[u8], limits: &Limits) {
    match diff_packages(first, second, limits) {
        Ok(diff) => {
            report["parts_identical"] = json!(parts_identical(&diff));
            report["parts"] = parts_report(&diff);
            report["element_diff"] = element_report(&diff);
        }
        Err(e) => report["parts"] = json!({ "comparable": false, "class": e.class() }),
    }
}

/// What the copy rewrote and left out: `bytes_dropped` (its `count`, and under `reasons` the bytes of each reason that
/// has any, by the container's fixed word), `headers_rewritten` (entries copied under headers written anew) and `rows_reordered`. The copy
/// is the file, byte for byte, exactly when all three say nothing; counts and words only.
fn copy_report(report: &mut Value, account: &CopyAccount) {
    let reasons: serde_json::Map<String, Value> =
        account.dropped.reasons().into_iter().filter(|&(_, bytes)| bytes > 0).map(|(word, bytes)| (word.to_owned(), json!(bytes))).collect();
    report["bytes_dropped"] = json!({ "count": account.dropped.total(), "reasons": reasons });
    report["headers_rewritten"] = json!(account.rewritten);
    report["rows_reordered"] = json!(account.rows_reordered);
}

/// What a person may be told of a package read: counts, flags, and the class of each warning (never its words).
fn read_report(package: &Package) -> Value {
    let mut classes: Vec<&'static str> = package.warnings().iter().map(|w| w.class()).collect();
    classes.dedup();
    json!({
        "reads_back": true,
        "files": package.parts().count(),
        "folders": package.folders().count(),
        "manifest_rows": package.manifest().entries.len(),
        "mimetype_regular": package.mimetype_regular(),
        "signed": package.signed(),
        "has_macros": package.has_macros(),
        "warning_count": package.warnings().len(),
        "warnings": classes.into_iter().take(MOST_LISTED).collect::<Vec<_>>(),
    })
}

/// Every file of the one package is a file of the other with the same bytes.
fn parts_identical(diff: &PackageDiff) -> bool {
    diff.parts.is_empty() && diff.bytes_only.is_empty() && diff.unparsed.is_empty() && diff.added.is_empty() && diff.removed.is_empty()
}

/// Part names as a person may see them, the first [`MOST_LISTED`].
fn labels<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    names.take(MOST_LISTED).map(part_label).collect()
}

fn parts_report(diff: &PackageDiff) -> Value {
    let headers: Vec<Value> =
        diff.headers_changed.iter().take(MOST_LISTED).map(|h| json!({ "part": part_label(&h.name), "fields": h.fields })).collect();
    json!({
        "identical": diff.identical,
        "element_changed": labels(diff.parts.iter().map(|p| p.part.as_str())),
        "element_changed_count": diff.parts.len(),
        "bytes_only": labels(diff.bytes_only.iter().map(String::as_str)),
        "bytes_only_count": diff.bytes_only.len(),
        "unparsed": diff.unparsed.iter().take(MOST_LISTED).map(|u| json!({ "part": part_label(&u.part), "class": u.class })).collect::<Vec<_>>(),
        "added": labels(diff.added.iter().map(String::as_str)),
        "added_count": diff.added.len(),
        "removed": labels(diff.removed.iter().map(String::as_str)),
        "removed_count": diff.removed.len(),
        "methods_changed": labels(diff.methods_changed.iter().map(String::as_str)),
        "headers_changed": headers,
        "headers_changed_count": diff.headers_changed.len(),
        "order_changed": diff.order_changed,
    })
}

/// Element paths as a person may see them, by their labels, the first [`MOST_LISTED`].
fn paths(list: &[ElementPath]) -> Vec<&str> {
    list.iter().take(MOST_LISTED).map(|p| p.label.as_str()).collect()
}

fn part_report(part: &PartDiff) -> Value {
    let changed: Vec<Value> = part
        .changed
        .iter()
        .take(MOST_LISTED)
        .map(|c| {
            json!({
                "path": c.label,
                "attributes": c.attribute_labels,
                "attribute_count": c.attribute_count,
                "reordered": c.reordered,
                "text": c.text,
            })
        })
        .collect();
    let moved: Vec<Value> = part.moved.iter().take(MOST_LISTED).map(|m| json!({ "from": m.from.label, "to": m.to.label })).collect();
    let c = part.counts;
    json!({
        "part": part_label(&part.part),
        "counts": { "added": c.added, "removed": c.removed, "changed": c.changed, "moved": c.moved, "whitespace": c.whitespace },
        "added": paths(&part.added),
        "removed": paths(&part.removed),
        "changed": changed,
        "moved": moved,
        "whitespace": paths(&part.whitespace),
    })
}

fn element_report(diff: &PackageDiff) -> Value {
    json!({
        "parts_changed": diff.parts.len(),
        "parts": diff.parts.iter().take(MOST_LISTED).map(part_report).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_odg::{DiffCounts, Dropped, ElementChange, HeaderChange, Moved, Unparsed};

    use super::*;

    #[test]
    fn the_copy_report_gives_each_reason_that_has_bytes_by_its_word() {
        // A count, the bytes of each reason that has any, and the two other causes; nothing for a copy that is the file.
        let account = CopyAccount {
            exact: 3,
            rewritten: 1,
            rows_reordered: true,
            dropped: Dropped { unannounced: 64, archive_comment: 5, ..Dropped::default() },
            ..CopyAccount::default()
        };
        let mut report = json!({});
        copy_report(&mut report, &account);
        assert_eq!(report["bytes_dropped"], json!({ "count": 69, "reasons": { "unannounced": 64, "archive_comment": 5 } }));
        assert_eq!((report["headers_rewritten"].clone(), report["rows_reordered"].clone()), (json!(1), json!(true)));
        copy_report(&mut report, &CopyAccount::default());
        assert_eq!(report["bytes_dropped"], json!({ "count": 0, "reasons": {} }));
        assert_eq!((report["headers_rewritten"].clone(), report["rows_reordered"].clone()), (json!(0), json!(false)));
    }

    /// An element path whose path as written holds a person's word and whose label does not.
    fn at(path: &str, label: &str) -> ElementPath {
        ElementPath { path: path.to_owned(), label: label.to_owned() }
    }

    #[test]
    fn element_paths_and_part_names_a_person_made_up_are_not_in_the_report() {
        // The report shows the labels the comparison gives, never the paths as written, and part names by role: every place a list holds
        // a name is given one a person made up here.
        let page = "/office:document-content/office:body/office:drawing/draw:page[2]";
        let part = PartDiff {
            part: "Object Secret/content.xml".to_owned(),
            added: vec![at(&format!("{page}/x:Secret[3]"), &format!("{page}/(unlisted)[3]"))],
            removed: vec![at(&format!("{page}/draw:g/Secret"), &format!("{page}/draw:g/(unlisted)"))],
            changed: vec![ElementChange {
                path: format!("{page}/Secret:custom-shape[7]"),
                label: format!("{page}/draw:custom-shape[7]"),
                attributes: vec!["svg:x".to_owned(), "x:Secret".to_owned()],
                attribute_labels: vec!["svg:x", "(unlisted)"],
                attribute_count: 2,
                reordered: false,
                text: true,
            }],
            moved: vec![Moved { from: at("/x:Secret[1]", "/(unlisted)[1]"), to: at("/x:Secret[2]", "/(unlisted)[2]") }],
            whitespace: vec![at(&format!("{page}/Secret:x"), &format!("{page}/(unlisted)"))],
            counts: DiffCounts { added: 1, removed: 1, changed: 1, moved: 1, whitespace: 1 },
        };
        let diff = PackageDiff {
            parts: vec![part, PartDiff { part: "content.xml".to_owned(), ..PartDiff::default() }],
            bytes_only: vec!["Pictures/Secret plan.png".to_owned(), "Pictures/SecretPlan.png".to_owned()],
            unparsed: vec![Unparsed { part: "SecretPlan.xml".to_owned(), class: "xml_malformed" }],
            added: vec!["Secret/".to_owned(), "ObjectReplacements/Secret".to_owned()],
            headers_changed: vec![HeaderChange { name: "Basic/Secret.xml".to_owned(), fields: vec!["time"] }],
            ..PackageDiff::default()
        };
        let report = json!({ "parts": parts_report(&diff), "element_diff": element_report(&diff) });
        let all = report.to_string();
        assert!(!all.contains("Secret"), "{all}");
        assert_eq!(report["parts"]["bytes_only"], json!(["picture", "picture"]));
        assert_eq!(report["parts"]["added"], json!(["a part", "object"]));
        assert_eq!(report["parts"]["headers_changed"], json!([{ "part": "macro", "fields": ["time"] }]));
        let first = &report["element_diff"]["parts"][0];
        assert_eq!(first["part"], "object");
        assert_eq!(first["changed"][0]["path"], format!("{page}/draw:custom-shape[7]"));
        assert_eq!(first["changed"][0]["attributes"], json!(["svg:x", "(unlisted)"]));
        assert_eq!(first["added"], json!([format!("{page}/(unlisted)[3]")]));
        assert_eq!(report["element_diff"]["parts"][1]["part"], "content.xml");
        assert!(!parts_identical(&diff));
        assert!(parts_identical(&PackageDiff { methods_changed: vec!["content.xml".to_owned()], ..PackageDiff::default() }));
    }

    #[test]
    fn an_odg_is_told_by_its_extension_or_its_bytes() {
        assert!(is_odg(Path::new("a.ODG"), b""));
        assert!(is_odg(Path::new("a.otg"), b""));
        assert!(!is_odg(Path::new("a.vsdx"), b"not a package"));
        let all = [
            SaveError::NoFolder,
            SaveError::Denied,
            SaveError::NotAFile,
            SaveError::Link,
            SaveError::DanglingLink,
            SaveError::DiskFull,
            SaveError::NoRoom,
            SaveError::Failed,
        ];
        let words: std::collections::BTreeSet<&str> = all.iter().map(save_word).collect();
        assert_eq!(words.len(), all.len(), "a word for each");
    }
}
