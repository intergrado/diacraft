//! APPID: an OpenDocument drawing that a build made before the id moved to `diacraft.app` wrote holds DiaCraft's markup in `urn:io.github.one_bit.diacraft:extension`: the attributes of its
//! styles, the entries under `DiaCraft/` and the extension of a flat file. It opens as it did, what a save writes is in the current namespace, the content check reads the two as one, and a
//! drawing that carries both reads as one. The two packages are the ones `main` at 3f696318 wrote (`diacraft_testkit::old_namespace`), not packages this code built by hand; the flat files
//! are made from the model by `Flat` with the extension in either namespace.

use std::sync::Arc;

use diacraft_doc::{DIACRAFT_URN, RawAttrs, RawElement, RawNode};
use diacraft_odg::{Limits, OriginalPackage, WriteOptions, compare_content, read, write_with};
use diacraft_testkit::old_namespace::{DRAWING_ODG, OLD_NAMESPACE, VSDX_AS_ODG, file, parts};
use diacraft_testkit::package::{ZipItem, zip};

use crate::flat::Flat;
use crate::write_core::{drawing_of, open_bytes};

/// The package of `name` with the old namespace put in the current form in every part that holds it, and (when `only` names parts) in those parts alone.
fn current(name: &str, only: &[&str]) -> Vec<u8> {
    let items: Vec<ZipItem> = parts(name)
        .into_iter()
        .map(|(entry, bytes)| match String::from_utf8(bytes) {
            Ok(text) if entry == "mimetype" => ZipItem::stored(&entry, text.as_bytes()),
            Ok(text) if only.is_empty() || only.contains(&entry.as_str()) => {
                ZipItem::deflated(&entry, text.replace(OLD_NAMESPACE, DIACRAFT_URN).as_bytes())
            }
            Ok(text) => ZipItem::deflated(&entry, text.as_bytes()),
            Err(bytes) => ZipItem::deflated(&entry, bytes.as_bytes()),
        })
        .collect();
    zip(&items)
}

fn names_old(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    package
        .entries()
        .filter(|e| String::from_utf8_lossy(package.part(&e.name).unwrap_or_default()).contains("one_bit"))
        .map(|e| e.name.clone())
        .collect()
}

#[test]
fn a_package_of_the_old_namespace_reads_as_the_package_of_the_current_one() {
    for name in [DRAWING_ODG, VSDX_AS_ODG] {
        let old = file(name);
        assert!(!names_old(&old).is_empty(), "{name} is a file a build before the move wrote");
        let (doc, report) = read(&old, &Limits::default()).unwrap();
        let (new, new_report) = read(&current(name, &[]), &Limits::default()).unwrap();
        assert!(doc == new, "{name}: the same drawing, whichever namespace the file says");
        assert_eq!(report.warnings, new_report.warnings, "{name}");
        assert_eq!(
            doc.diacraft.as_ref().map(|d| d.attrs.get("xmlns:dc").map(str::to_owned)),
            Some(Some(DIACRAFT_URN.to_owned())),
            "{name}: the data the model holds is as this build writes it"
        );
        assert!(doc.pages.len() == 2 && doc.pages.iter().all(|p| !p.shapes.is_empty() || p.name.as_ref() == "Second"), "{name}");
    }
}

#[test]
fn a_package_that_holds_both_namespaces_reads_as_one_drawing() {
    // Each part of the package alone in the current namespace, then the carrier alone and the rest alone: the same drawing every time.
    for name in [DRAWING_ODG, VSDX_AS_ODG] {
        let (expected, _) = read(&file(name), &Limits::default()).unwrap();
        for only in [vec!["content.xml"], vec!["DiaCraft/document.xml"], vec!["content.xml", "styles.xml"]] {
            let (doc, _) = read(&current(name, &only), &Limits::default()).unwrap();
            assert!(doc == expected, "{name}, {only:?} in the current namespace: one drawing, each page and shape once");
        }
    }
}

#[test]
fn the_model_written_from_a_package_of_the_old_namespace_names_it_nowhere_and_the_content_check_says_what_it_says_of_the_current_form() {
    for name in [DRAWING_ODG, VSDX_AS_ODG] {
        let (old, new) = (file(name), current(name, &[]));
        let (old_model, _) = read(&old, &Limits::default()).unwrap();
        let (new_model, _) = read(&new, &Limits::default()).unwrap();
        let written_from_old = write_with(&old_model, None, &WriteOptions::default()).unwrap().bytes;
        let written_from_new = write_with(&new_model, None, &WriteOptions::default()).unwrap().bytes;
        assert_eq!(written_from_old, written_from_new, "{name}: the model of one is the model of the other, and so is what is written from it");
        assert!(names_old(&written_from_old).is_empty(), "{name}: {:?}", names_old(&written_from_old));
        // The source first: what the check finds of DiaCraft's markup the source held, in the file written. The file of before and the file of now are the same content as
        // each other whichever is first, and the check says of the one what it says of the other (the files of a new drawing have two differences of their own, in `fp-master`,
        // which the build before the move gave as well).
        let (of_old, of_new) = (
            compare_content(&old, &written_from_old, &Limits::default()).unwrap(),
            compare_content(&new, &written_from_new, &Limits::default()).unwrap(),
        );
        assert_eq!(of_old.count, of_new.count, "{name}: {:?} against {:?}", of_old.shown, of_new.shown);
        let paths =
            |d: &diacraft_container::canon::ContentDiff| d.shown.iter().map(|x| (x.part.clone(), x.path.clone(), x.found)).collect::<Vec<_>>();
        assert_eq!(paths(&of_old), paths(&of_new), "{name}");
        for (a, b) in [(&old, &new), (&new, &old)] {
            let diff = compare_content(a, b, &Limits::default()).unwrap();
            assert!(diff.same(), "{name}: {:?}", diff.shown);
        }
        // Not vacuous: a page named another name, beside the namespace, is a difference.
        let changed: Vec<ZipItem> = parts(name)
            .into_iter()
            .map(|(entry, bytes)| {
                let text = String::from_utf8(bytes.clone())
                    .ok()
                    .map(|t| if entry == "content.xml" { t.replacen("draw:name=\"Page-1\"", "draw:name=\"Page-9\"", 1) } else { t });
                match text {
                    Some(text) if entry == "mimetype" => ZipItem::stored(&entry, text.as_bytes()),
                    Some(text) => ZipItem::deflated(&entry, text.as_bytes()),
                    None => ZipItem::deflated(&entry, &bytes),
                }
            })
            .collect();
        let diff = compare_content(&old, &zip(&changed), &Limits::default()).unwrap();
        assert!(!diff.same(), "{name}: a page named another name is a difference");
    }
}

/// The extension a flat file holds, in the namespace `ns`.
fn extension(ns: &str) -> String {
    format!("<dc:Data xmlns:dc=\"{ns}\" dc:version=\"1\"><dc:snap n=\"1\"/></dc:Data>")
}

fn flat_with(extension: &str) -> Vec<u8> {
    let mut flat = Flat::of(&drawing_of(2));
    flat.tail = extension.to_owned();
    flat.bytes()
}

#[test]
fn a_flat_drawing_whose_extension_is_in_the_old_namespace_reads_and_a_save_replaces_it_where_it_stands() {
    let old = flat_with(&extension(OLD_NAMESPACE));
    let (doc, _) = read(&old, &Limits::default()).unwrap();
    let data = doc.diacraft.clone().expect("the extension is read");
    assert_eq!(data.attrs.get("xmlns:dc"), Some(DIACRAFT_URN), "it is held as this build writes it");
    let (current, _) = read(&flat_with(&extension(DIACRAFT_URN)), &Limits::default()).unwrap();
    assert!(doc == current, "the same drawing, whichever namespace the extension is in");
    // An edit of the extension: the one element is replaced where it stands, in the current namespace, and none is added beside it.
    let mut o = open_bytes(old);
    let mut changed = RawElement {
        name: Arc::from("dc:Data"),
        attrs: RawAttrs::from_pairs([("xmlns:dc", DIACRAFT_URN), ("dc:version", "1")]),
        children: Vec::new(),
    };
    changed.children.push(RawNode::Element(RawElement {
        name: Arc::from("dc:snap"),
        attrs: RawAttrs::from_pairs([("n", "2")]),
        children: Vec::new(),
    }));
    o.doc.diacraft = Some(Arc::new(changed));
    let saved = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default()).unwrap();
    let text = String::from_utf8(saved.bytes.clone()).unwrap();
    assert!(saved.edited);
    assert_eq!(text.matches("dc:Data xmlns").count(), 1, "one element: {text}");
    assert!(text.contains("snap n=\"2\"") && !text.contains("snap n=\"1\"") && !text.contains("one_bit"), "{text}");
    assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().0.diacraft, o.doc.diacraft);
}

#[test]
fn a_flat_drawing_that_holds_the_extension_in_both_namespaces_reads_the_first_and_says_it_is_repeated() {
    let both = format!("{}{}", extension(OLD_NAMESPACE), extension(DIACRAFT_URN).replace("n=\"1\"", "n=\"9\""));
    let (doc, report) = read(&flat_with(&both), &Limits::default()).unwrap();
    let data = doc.diacraft.expect("the first element is the extension");
    let snap = data.children.iter().find_map(|c| if let RawNode::Element(e) = c { Some(e) } else { None }).unwrap();
    assert_eq!(snap.attrs.get("n"), Some("1"), "the first, once");
    assert!(report.warnings.iter().any(|w| w.contains("more than once")), "{:?}", report.warnings);
}

/// A flat drawing whose root binds the prefix `dia` to `ns`, with an extension that declares nothing of its own and so is in whatever namespace the root binds `dia` to.
fn flat_binding_on_the_root(ns: &str) -> Vec<u8> {
    let mut flat = Flat::of(&drawing_of(2));
    flat.root = flat.root.replacen("<office:document ", &format!("<office:document xmlns:dia=\"{ns}\" "), 1);
    flat.tail = "<dia:Data dia:version=\"1\"><dia:snap n=\"1\"/></dia:Data>".to_owned();
    flat.bytes()
}

#[test]
fn a_flat_drawing_whose_root_binds_the_prefix_of_the_extension_to_the_old_namespace_has_its_extension_written_in_the_current_one() {
    let old = flat_binding_on_the_root(OLD_NAMESPACE);
    let (doc, _) = read(&old, &Limits::default()).unwrap();
    let data = doc.diacraft.clone().expect("the extension is read");
    assert_eq!(
        data.attrs.get("xmlns:dia"),
        Some(DIACRAFT_URN),
        "the data the model holds names the current namespace itself, and is no longer in the root's old one"
    );
    let (current, _) = read(&flat_binding_on_the_root(DIACRAFT_URN), &Limits::default()).unwrap();
    assert_eq!(doc.pages.len(), current.pages.len());
    // A save that changes nothing writes the extension where it stands, as it was.
    let mut o = open_bytes(old.clone());
    let same = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default()).unwrap();
    assert_eq!(same.bytes, old, "no change, no bytes written");
    // An edit of the extension writes it again, in the current namespace only: it declares the namespace itself, so the root's old binding is no part of it.
    assert!(o.doc.set_diacraft_child("snap", Some(&[("n", "2")])));
    let saved = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default()).unwrap();
    let text = String::from_utf8(saved.bytes.clone()).unwrap();
    assert!(saved.edited);
    assert_eq!(text.matches("<dia:Data").count(), 1, "one element: {text}");
    assert!(text.contains(&format!("<dia:Data xmlns:dia=\"{DIACRAFT_URN}\"")), "the element declares the current namespace for itself: {text}");
    assert!(text.contains("snap n=\"2\"") && !text.contains("snap n=\"1\""), "{text}");
    let after = text.split("<dia:Data").nth(1).unwrap_or_default();
    assert!(!after.contains("one_bit"), "the extension names no old namespace: {text}");
    assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().0.diacraft, o.doc.diacraft, "the extension reads back as the model has it");
}
