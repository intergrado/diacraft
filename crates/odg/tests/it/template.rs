//! `.otg` drawing templates: a template is read as a drawing and
//! says so, is saved as a template with every rule of the splice writer, and is saved as a drawing (and a drawing as a template) by changing
//! the media type in the two places a package states it, `mimetype` and the manifest's root row, and nothing else.
//!
//! The cases that change the media type are functions of a *saver*, as the edit cases of `write_core.rs` are: the real writer must pass them, and
//! an echo (bytes in, bytes out), and a writer that changes only one of the two statements, must fail them.

use std::panic::{AssertUnwindSafe, catch_unwind};

use diacraft_doc::Document;
use diacraft_odg::package::{MANIFEST_PATH, Package};
use diacraft_odg::{
    Limits, MimetypeFault, OriginalPackage, SaveNote, ShapeIndex, Variant, Warning, WriteError, WriteOptions, WriteWork, Written, copy_account,
    diff_packages, read, sniff, write_counted, write_with,
};
use proptest::prelude::*;

use crate::build::{DRAWING, TEMPLATE, package, with_manifest, write};
use crate::drawing::{Drawing, close, cm, settings, without_package};
use crate::read_families::{libreoffice, openoffice};
use crate::write_core::{Open, PNG, Saver, compare, drawing_of, echo, echo_says_edited, full_meta, move_by, open_bytes, part, part_bytes};

// ----------------------------------------------------------------------------------------------------------------------------------------
// The packages.

/// The parts of a package that has what a producer writes: three shapes, a meta part with a generator, settings, a thumbnail, a picture and
/// `Configurations2/`.
fn parts() -> Drawing {
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    d.settings = Some(settings(""));
    // Not the thumbnail the cases pass, so that an edited save is seen to replace it.
    d.files.push(("Thumbnails/thumbnail.png".to_owned(), b"\x89PNG\r\n\x1a\nold thumbnail".to_vec()));
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    d.files.push(("Configurations2/".to_owned(), Vec::new()));
    d
}

/// The package of `parts` that says it is `variant`, in `mimetype` and in the manifest's root row.
fn package_of(variant: Variant) -> Vec<u8> {
    package(variant.media_type(), parts().files())
}

fn a_template() -> Vec<u8> {
    package_of(Variant::Template)
}

fn a_drawing() -> Vec<u8> {
    package_of(Variant::Drawing)
}

/// The same package with the template's media type in both places: the entries in the order they lie in, `mimetype` rebuilt, and the root row
/// of the manifest (the first row) changed in its text.
fn as_template(bytes: &[u8]) -> Vec<u8> {
    let p = Package::read(bytes, &Limits::default()).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for e in p.entries() {
        if e.name == "mimetype" {
            continue;
        }
        let data = if e.folder { Vec::new() } else { p.part(&e.name).unwrap().to_vec() };
        let data = if e.name == MANIFEST_PATH { String::from_utf8(data).unwrap().replacen(DRAWING, TEMPLATE, 1).into_bytes() } else { data };
        entries.push((e.name.clone(), data));
    }
    write(Some(TEMPLATE), &entries)
}

/// The drawing packages of every family the readers' tests build, by name.
fn families() -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<(String, Vec<u8>)> = ["1.2", "1.3", "1.4"].iter().map(|v| (format!("libreoffice {v}"), libreoffice(v))).collect();
    all.push(("openoffice 1.0 with a document type".to_owned(), openoffice("1.0", true)));
    all.push(("openoffice 1.2".to_owned(), openoffice("1.2", false)));
    all.push(("hand-built".to_owned(), drawing_of(4).bytes()));
    all.push(("every part".to_owned(), a_drawing()));
    all
}

/// The writer, with a thumbnail, asked to write `variant`.
fn as_variant(variant: Option<Variant>) -> impl Fn(&Document, &[u8], &ShapeIndex) -> Result<Written, WriteError> {
    move |doc, bytes, index| {
        write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { thumbnail: Some(PNG.to_vec()), variant, ..WriteOptions::default() })
    }
}

/// The edit the cases make: shape 2 moved 1 cm right and half a centimetre down.
fn edit(o: &mut Open) {
    move_by(&mut o.doc, 0, 2, 1.0, -0.5);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Reading.

#[test]
fn a_template_reads_as_the_drawing_it_is_and_says_so() {
    let (t, d) = (a_template(), a_drawing());
    assert_eq!(sniff(&t), Some(Variant::Template));
    assert_eq!(sniff(&d), Some(Variant::Drawing));
    let (tdoc, treport) = read(&t, &Limits::default()).unwrap();
    let (ddoc, dreport) = read(&d, &Limits::default()).unwrap();
    assert_eq!((treport.variant, dreport.variant), (Some(Variant::Template), Some(Variant::Drawing)));
    assert_eq!(tdoc.pages.len(), 1);
    assert_eq!(tdoc.pages.get(0).map(|p| p.shapes.len()), Some(3));
    assert_eq!(without_package(&tdoc), without_package(&ddoc), "the model is the same as that of the same file with the drawing media type");
    assert!(treport.warning_counts.is_empty(), "{:?}", treport.warning_counts);
    assert_eq!(treport.warning_counts, dreport.warning_counts);
    assert_eq!((treport.office_version, treport.generator), (dreport.office_version, dreport.generator));
    let package = Package::read(&t, &Limits::default()).unwrap();
    assert_eq!((package.variant(), package.media_type()), (Some(Variant::Template), Some(TEMPLATE)));
    assert!(package.mimetype_regular() && package.warnings().is_empty());
}

#[test]
fn every_family_as_a_template_reads_as_the_same_model() {
    for (name, drawing) in families() {
        let template = as_template(&drawing);
        assert_ne!(template, drawing, "{name}");
        assert_eq!(sniff(&template), Some(Variant::Template), "{name}");
        let (tdoc, treport) = read(&template, &Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (ddoc, dreport) = read(&drawing, &Limits::default()).unwrap();
        assert_eq!(treport.variant, Some(Variant::Template), "{name}");
        assert_eq!(without_package(&tdoc), without_package(&ddoc), "{name}");
        assert_eq!(treport.warning_counts, dreport.warning_counts, "{name}");
    }
}

#[test]
fn a_template_whose_two_statements_disagree_is_read_by_the_root_entry() {
    // Where `mimetype` and the root entry disagree the root entry wins, with one warning that names the disagreement.
    let root_template = write(Some(DRAWING), &with_manifest(TEMPLATE, parts().files()));
    let root_drawing = write(Some(TEMPLATE), &with_manifest(DRAWING, parts().files()));
    for (what, bytes, variant) in
        [("root entry template", &root_template, Variant::Template), ("root entry drawing", &root_drawing, Variant::Drawing)]
    {
        assert_eq!(sniff(bytes), Some(variant), "{what}");
        let (_, report) = read(bytes, &Limits::default()).unwrap();
        assert_eq!(report.variant, Some(variant), "{what}");
        assert_eq!(report.warning_counts.get("mimetype_irregular"), Some(&1), "{what}: {:?}", report.warning_counts);
        let package = Package::read(bytes, &Limits::default()).unwrap();
        let faults: Vec<&[MimetypeFault]> = package
            .warnings()
            .iter()
            .filter_map(|w| match w {
                Warning::MimetypeIrregular { faults } => Some(faults.as_slice()),
                _ => None,
            })
            .collect();
        assert_eq!(faults, [&[MimetypeFault::DisagreesWithManifest][..]], "{what}");
    }
    // With no root entry at all, `mimetype` says it; with no `mimetype`, the root entry does.
    let no_root = write(Some(TEMPLATE), &no_root_entry());
    assert_eq!(sniff(&no_root), Some(Variant::Template));
    assert_eq!(read(&no_root, &Limits::default()).unwrap().1.warning_counts.get("manifest_no_root_entry"), Some(&1));
    let no_mimetype = write(None, &with_manifest(TEMPLATE, parts().files()));
    assert_eq!(sniff(&no_mimetype), Some(Variant::Template));
    assert_eq!(read(&no_mimetype, &Limits::default()).unwrap().1.warning_counts.get("mimetype_irregular"), Some(&1));
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Saving a template as a template.

#[test]
fn an_unedited_save_of_a_template_is_the_file_byte_for_byte() {
    for (name, drawing) in families() {
        let template = as_template(&drawing);
        let o = open_bytes(template.clone());
        for asked in [None, Some(Variant::Template)] {
            let saved = as_variant(asked)(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!saved.edited && saved.notes.is_empty(), "{name}");
            assert_eq!(saved.bytes, template, "{name}: an unedited save of a template is the template");
        }
    }
}

#[test]
fn an_edited_save_of_a_template_keeps_the_media_type_and_mimetype_first_and_stored() {
    let mut o = open_bytes(a_template());
    edit(&mut o);
    for asked in [None, Some(Variant::Template)] {
        let saved = as_variant(asked)(&o.doc, &o.bytes, &o.index).unwrap();
        assert!(saved.edited);
        // The offsets a program tells the package by, with the template's media type.
        assert_eq!(&saved.bytes[..2], b"PK");
        assert_eq!(&saved.bytes[30..38], b"mimetype");
        assert_eq!(&saved.bytes[38..38 + TEMPLATE.len()], TEMPLATE.as_bytes());
        let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
        assert!(package.mimetype_regular() && package.warnings().is_empty(), "{:?}", package.warnings());
        assert_eq!(package.variant(), Some(Variant::Template));
        assert_eq!(sniff(&saved.bytes), Some(Variant::Template));
        // Neither statement was touched: the manifest is the file's, the `mimetype` entry is its own bytes, and the edit owns the rest.
        assert_eq!(part_bytes(&saved.bytes, MANIFEST_PATH), part_bytes(&o.bytes, MANIFEST_PATH));
        let diff = compare(&o.bytes, &saved.bytes);
        let mut changed: Vec<&str> = diff.parts.iter().map(|p| p.part.as_str()).collect();
        changed.sort_unstable();
        assert_eq!(changed, ["content.xml", "meta.xml"], "{diff:?}");
        assert_eq!(diff.bytes_only, ["Thumbnails/thumbnail.png"]);
        assert!(diff.added.is_empty() && diff.removed.is_empty() && diff.headers_changed.is_empty() && !diff.order_changed, "{diff:?}");
        // The model it reads back as: the shape is where it was moved to, and the package says it is a template.
        let back = open_bytes(saved.bytes.clone());
        close(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap(), cm(5.0));
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Saving a template as a drawing, and a drawing as a template.

/// The package in `from`, saved through `save` (which writes `to`), differs from it in the two statements of the media type and, with an edit,
/// in what the edit owns, and in nothing else.
fn retyped(save: &Saver, from: Variant, to: Variant, edited: bool) {
    let mut o = open_bytes(package_of(from));
    if edited {
        edit(&mut o);
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.edited, edited, "the flag says whether the model changed, not whether the media type did");
    let (old, new) = (from.media_type(), to.media_type());
    // `mimetype` is first, stored, and says the new media type.
    assert_eq!(&saved.bytes[..2], b"PK");
    assert_eq!(&saved.bytes[30..38], b"mimetype");
    assert_eq!(&saved.bytes[38..38 + new.len()], new.as_bytes());
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert!(package.mimetype_regular() && package.warnings().is_empty(), "{:?}", package.warnings());
    assert_eq!((package.variant(), package.media_type()), (Some(to), Some(new)));
    assert_eq!(sniff(&saved.bytes), Some(to));
    assert_eq!(package.manifest().root().and_then(|r| r.media_type.as_deref()), Some(new));
    // The manifest is the file's but for the root row's media type, in the row's own place and quotes.
    let (before, after) = (part(&o.bytes, MANIFEST_PATH), part(&saved.bytes, MANIFEST_PATH));
    assert_eq!(after, before.replacen(old, new, 1));
    assert_ne!(after, before);
    // Nothing else of the package changed: the element-level comparison, the parts' bytes, the headers and the order.
    let diff = compare(&o.bytes, &saved.bytes);
    let mut names: Vec<&str> = diff.parts.iter().map(|p| p.part.as_str()).collect();
    names.sort_unstable();
    let manifest = diff.parts.iter().find(|p| p.part == MANIFEST_PATH).expect("the manifest changed");
    assert_eq!((manifest.counts.added, manifest.counts.removed, manifest.changed.len()), (0, 0, 1), "{manifest:?}");
    assert_eq!(manifest.changed.first().map(|c| c.attributes.clone()), Some(vec!["manifest:media-type".to_owned()]));
    if edited {
        assert_eq!(names, [MANIFEST_PATH, "content.xml", "meta.xml"], "{diff:?}");
        assert_eq!(diff.bytes_only, ["mimetype", "Thumbnails/thumbnail.png"]);
        assert_eq!(diff.identical, 3, "the styles, the settings and the picture are the file's bytes");
    } else {
        assert_eq!(names, [MANIFEST_PATH], "{diff:?}");
        assert_eq!(diff.bytes_only, ["mimetype"]);
        assert_eq!(diff.identical, 6, "every other part is the file's bytes");
    }
    assert!(diff.added.is_empty() && diff.removed.is_empty() && diff.unparsed.is_empty(), "{diff:?}");
    assert!(diff.methods_changed.is_empty() && diff.headers_changed.is_empty() && !diff.order_changed, "{diff:?}");
    // The model: the file's, or the edited one.
    let back = open_bytes(saved.bytes.clone());
    if edited {
        let moved = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
        close(moved.sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap(), cm(5.0));
    } else {
        assert_eq!(without_package(&back.doc), without_package(&o.doc));
        // The package says its new kind in the two places the model holds it.
        assert_eq!(back.doc.ext.get().and_then(|d| d.odf.mimetype.as_deref()), Some(new));
    }
}

/// The cases that change the media type: what each is called, the kind the package is, the kind it is saved as, and whether the model is edited.
const CASES: &[(&str, Variant, Variant, bool)] = &[
    ("a template saved as a drawing, unedited", Variant::Template, Variant::Drawing, false),
    ("a template saved as a drawing, edited", Variant::Template, Variant::Drawing, true),
    ("a drawing saved as a template, unedited", Variant::Drawing, Variant::Template, false),
    ("a drawing saved as a template, edited", Variant::Drawing, Variant::Template, true),
];

#[test]
fn a_template_is_saved_as_a_drawing_and_a_drawing_as_a_template() {
    for (case, from, to, edited) in CASES {
        println!("{case}");
        retyped(&as_variant(Some(*to)), *from, *to, *edited);
    }
}

/// A writer that gives a package whose `mimetype` says `mimetype` and whose root row says `root`, whatever it is asked, and says `edited`: one
/// that states the new kind in one of the two places only has done half of the change.
fn states(mimetype: Variant, root: Variant, edited: bool) -> impl Fn(&Document, &[u8], &ShapeIndex) -> Result<Written, WriteError> {
    move |_doc, _bytes, index| {
        let bytes = write(Some(mimetype.media_type()), &with_manifest(root.media_type(), parts().files()));
        Ok(Written { bytes, index: index.clone(), notes: Vec::new(), edited, work: WriteWork::default() })
    }
}

/// The echo control: a writer that gives the file back, one that says it edited, and ones that change only one of the two statements, whatever they
/// say of the edit, each fail every case that changes the media type, and the real writer passes each. A case one of them passed would check nothing.
#[test]
fn echo_control_fails_every_case_that_changes_the_media_type() {
    for (case, from, to, edited) in CASES {
        let savers: Vec<(String, Box<Saver>)> = vec![
            ("echo".to_owned(), Box::new(echo)),
            ("echo that says it edited".to_owned(), Box::new(echo_says_edited)),
            ("only mimetype, says unedited".to_owned(), Box::new(states(*to, *from, false))),
            ("only mimetype, says edited".to_owned(), Box::new(states(*to, *from, true))),
            ("only the manifest, says unedited".to_owned(), Box::new(states(*from, *to, false))),
            ("only the manifest, says edited".to_owned(), Box::new(states(*from, *to, true))),
        ];
        for (saver, save) in &savers {
            let outcome = catch_unwind(AssertUnwindSafe(|| retyped(save.as_ref(), *from, *to, *edited)));
            let Err(payload) = outcome else { panic!("the writer {saver} passed the case {case}: the case checks nothing") };
            let said =
                payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            println!("{saver} FAILS {case}: {}", said.lines().next().unwrap_or_default().chars().take(140).collect::<String>());
        }
        let real = as_variant(Some(*to));
        assert!(catch_unwind(AssertUnwindSafe(|| retyped(&real, *from, *to, *edited))).is_ok(), "the real writer failed the case {case}");
    }
}

#[test]
fn a_template_saved_as_a_drawing_is_the_drawing_saved_as_a_drawing_and_back() {
    // The oracle that is not the writer's own text: for every family, the template retyped is the drawing it was made from, byte for byte; the
    // same edit saved from the template as a drawing and from the drawing as a drawing is one file, and so is the reverse.
    for (name, drawing) in families() {
        let template = as_template(&drawing);
        let (mut from_template, mut from_drawing) = (open_bytes(template.clone()), open_bytes(drawing.clone()));
        let (to_drawing, to_template) = (as_variant(Some(Variant::Drawing)), as_variant(Some(Variant::Template)));
        let retyped = to_drawing(&from_template.doc, &from_template.bytes, &from_template.index).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(retyped.bytes, drawing, "{name}: the template saved as a drawing, unedited, is the drawing");
        let retyped = to_template(&from_drawing.doc, &from_drawing.bytes, &from_drawing.index).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(retyped.bytes, template, "{name}: the drawing saved as a template, unedited, is the template");
        edit(&mut from_template);
        edit(&mut from_drawing);
        let a = to_drawing(&from_template.doc, &from_template.bytes, &from_template.index).unwrap_or_else(|e| panic!("{name}: {e}"));
        let b = as_variant(None)(&from_drawing.doc, &from_drawing.bytes, &from_drawing.index).unwrap();
        assert_eq!(a.bytes, b.bytes, "{name}: the same edit saved as a drawing, from the template and from the drawing");
        let c = to_template(&from_drawing.doc, &from_drawing.bytes, &from_drawing.index).unwrap();
        let d = as_variant(None)(&from_template.doc, &from_template.bytes, &from_template.index).unwrap();
        assert_eq!(c.bytes, d.bytes, "{name}: and as a template");
        assert_ne!(a.bytes, c.bytes);
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Second saves.

#[test]
fn a_second_save_is_no_edit_in_every_form_and_a_save_back_is_the_save_from_the_file() {
    for (from, to) in [
        (Variant::Template, Variant::Template),
        (Variant::Template, Variant::Drawing),
        (Variant::Drawing, Variant::Template),
        (Variant::Drawing, Variant::Drawing),
    ] {
        for edited in [false, true] {
            let what = format!("{from:?} saved as {to:?}, edited: {edited}");
            let mut o = open_bytes(package_of(from));
            if edited {
                edit(&mut o);
            }
            let first = as_variant(Some(to))(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
            // Again and again through the index of the save before, asking for the same kind and not asking: no edit, and the same bytes.
            let mut last = first.clone();
            for round in 1..=3 {
                for asked in [Some(to), None] {
                    let again = as_variant(asked)(&o.doc, &last.bytes, &last.index).unwrap_or_else(|e| panic!("{what}, round {round}: {e}"));
                    assert!(!again.edited && again.notes.is_empty(), "{what}, round {round}, asked {asked:?}: a second save is no edit");
                    assert_eq!(again.bytes, first.bytes, "{what}, round {round}, asked {asked:?}");
                    last = again;
                }
            }
            // Saved as the other kind from the save, then: the save from the file as that kind, byte for byte.
            let other = if to == Variant::Template { Variant::Drawing } else { Variant::Template };
            let back = as_variant(Some(other))(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("{what}, back: {e}"));
            assert!(!back.edited, "{what}");
            let direct = as_variant(Some(other))(&o.doc, &o.bytes, &o.index).unwrap();
            assert_eq!(back.bytes, direct.bytes, "{what}: saved as {other:?} from the save or from the file");
            assert_eq!(sniff(&back.bytes), Some(other));
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// What the media type is stated in: refusals, the manifest's spellings, the header of `mimetype`.

/// The parts' manifest with no row for the package's own document.
fn no_root_entry() -> Vec<(String, Vec<u8>)> {
    let mut entries = with_manifest(TEMPLATE, parts().files());
    for (name, data) in &mut entries {
        if name == MANIFEST_PATH {
            let text = String::from_utf8(std::mem::take(data)).unwrap();
            let kept: Vec<&str> = text.lines().filter(|l| !l.contains("manifest:full-path=\"/\"")).collect();
            assert_eq!(kept.len() + 1, text.lines().count());
            *data = (kept.join("\n") + "\n").into_bytes();
        }
    }
    entries
}

/// The parts' manifest with the root row given as `root` (its text).
fn root_row_is(root: &str) -> Vec<(String, Vec<u8>)> {
    let mut entries = with_manifest(TEMPLATE, parts().files());
    for (name, data) in &mut entries {
        if name == MANIFEST_PATH {
            let text = String::from_utf8(std::mem::take(data)).unwrap();
            let kept: Vec<String> =
                text.lines().map(|l| if l.contains("manifest:full-path=\"/\"") { root.to_owned() } else { l.to_owned() }).collect();
            *data = (kept.join("\n") + "\n").into_bytes();
        }
    }
    entries
}

#[test]
fn a_save_the_change_of_media_type_cannot_make_is_refused_in_words_before_the_file_is_read() {
    let cases = [
        ("no root entry", write(Some(TEMPLATE), &no_root_entry()), "root entry"),
        ("a root entry with no media type", write(Some(TEMPLATE), &root_row_is(" <manifest:file-entry manifest:full-path=\"/\"/>")), "no media type"),
    ];
    for (what, bytes, words) in cases {
        for edited in [false, true] {
            let mut o = open_bytes(bytes.clone());
            assert_eq!(sniff(&o.bytes), Some(Variant::Template), "{what}: the package reads as a template");
            if edited {
                edit(&mut o);
            }
            let mut work = WriteWork::default();
            let options = WriteOptions { variant: Some(Variant::Drawing), ..WriteOptions::default() };
            let refused = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
            let Err(error) = refused else { panic!("{what}, edited {edited}: saved as a drawing") };
            assert!(matches!(error, WriteError::MediaType(_)), "{what}: {error:?}");
            assert_eq!(error.class(), "write_media_type");
            assert!(error.to_string().contains(words), "{what}: {error}");
            assert_eq!((work.baseline_read.items, work.scan_bytes), (0, 0), "{what}: refused before the file was read");
            // Saved as a template, which it is, it saves: a package that cannot state a new media type can state its own.
            for asked in [None, Some(Variant::Template)] {
                let saved = as_variant(asked)(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(saved.edited, edited);
                assert_eq!(sniff(&saved.bytes), Some(Variant::Template));
                if !edited {
                    assert_eq!(saved.bytes, o.bytes);
                }
            }
        }
    }
}

#[test]
fn an_empty_media_type_in_the_root_row_and_a_package_that_says_nothing_are_given_the_media_type() {
    // A root row whose media type is empty states nothing: `mimetype` says it is a template, and a save as a drawing writes the new type in both.
    let bytes = write(Some(TEMPLATE), &root_row_is(" <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"\"/>"));
    let o = open_bytes(bytes);
    assert_eq!(sniff(&o.bytes), Some(Variant::Template));
    let saved = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(
        part(&saved.bytes, MANIFEST_PATH),
        part(&o.bytes, MANIFEST_PATH).replacen("manifest:media-type=\"\"", &format!("manifest:media-type=\"{DRAWING}\""), 1)
    );
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert!(package.mimetype_regular() && package.warnings().is_empty() && package.variant() == Some(Variant::Drawing));
    // As what it says it is, a save changes nothing of the statements.
    assert_eq!(as_variant(Some(Variant::Template))(&o.doc, &o.bytes, &o.index).unwrap().bytes, o.bytes);
    // No `mimetype` and an empty root row: the package says nothing; it is read, and a save as a drawing states the type in both places.
    let nothing = write(None, &root_row_is(" <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"\"/>"));
    assert_eq!(sniff(&nothing), None);
    let o = open_bytes(nothing);
    assert_eq!(as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap().bytes, o.bytes, "nothing asked, nothing stated");
    let saved = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(&saved.bytes[30..38], b"mimetype");
    assert_eq!(&saved.bytes[38..38 + DRAWING.len()], DRAWING.as_bytes());
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert!(package.mimetype_regular() && package.warnings().is_empty(), "{:?}", package.warnings());
    assert_eq!(package.variant(), Some(Variant::Drawing));
}

#[test]
fn statements_that_disagree_are_saved_by_the_root_entry_and_a_missing_mimetype_is_written_first() {
    let root_template = write(Some(DRAWING), &with_manifest(TEMPLATE, parts().files()));
    let root_drawing = write(Some(TEMPLATE), &with_manifest(DRAWING, parts().files()));
    let absent = write(None, &with_manifest(TEMPLATE, parts().files()));
    for (what, bytes, says) in [
        ("root template, mimetype drawing", root_template, Variant::Template),
        ("root drawing, mimetype template", root_drawing, Variant::Drawing),
        ("no mimetype", absent, Variant::Template),
    ] {
        let other = if says == Variant::Template { Variant::Drawing } else { Variant::Template };
        let mut o = open_bytes(bytes);
        assert_eq!(sniff(&o.bytes), Some(says), "{what}");
        // The kind it says it is, unedited: the file copied, faults and all. Edited: `mimetype` as R2 asks, saying what the root entry does.
        let same = as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap();
        assert_eq!(same.bytes, o.bytes, "{what}");
        let other_unedited = as_variant(Some(other))(&o.doc, &o.bytes, &o.index).unwrap();
        edit(&mut o);
        let kept = as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap();
        let package = Package::read(&kept.bytes, &Limits::default()).unwrap();
        assert!(package.mimetype_regular() && package.warnings().is_empty(), "{what}: {:?}", package.warnings());
        assert_eq!((package.variant(), &kept.bytes[30..38]), (Some(says), &b"mimetype"[..]), "{what}");
        assert_eq!(part_bytes(&kept.bytes, MANIFEST_PATH), part_bytes(&o.bytes, MANIFEST_PATH), "{what}: the root entry said it already");
        // The other kind, unedited and edited: both statements say it, `mimetype` is first and stored, and the root row's value is the only change.
        let edited = as_variant(Some(other))(&o.doc, &o.bytes, &o.index).unwrap();
        for (saved, edit) in [(&other_unedited, false), (&edited, true)] {
            let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
            assert!(package.mimetype_regular() && package.warnings().is_empty(), "{what}, edited {edit}: {:?}", package.warnings());
            assert_eq!(package.variant(), Some(other), "{what}, edited {edit}");
            assert_eq!(&saved.bytes[30..38], b"mimetype");
            assert_eq!(&saved.bytes[38..38 + other.media_type().len()], other.media_type().as_bytes());
            assert_eq!(package.manifest().root().and_then(|r| r.media_type.as_deref()), Some(other.media_type()), "{what}");
        }
    }
}

/// The parts' package with `mimetype` put second in the archive, deflated, under the manifest `entries` (so it is not as R2 asks).
fn mimetype_second(mut entries: Vec<(String, Vec<u8>)>, media: &str) -> Vec<u8> {
    entries.insert(1, ("mimetype".to_owned(), media.as_bytes().to_vec()));
    write(None, &entries)
}

/// Census: a save that asks for a kind always states it, even for a model that is not edited, so that the two statements
/// agree. The census file whose root row says drawing and whose `mimetype` says template reads as a drawing, and a save that asked for a drawing
/// copied it with the template's `mimetype`.
#[test]
fn a_save_that_asks_for_the_kind_the_package_says_writes_a_mimetype_as_asks_even_unedited() {
    let mut signed = parts();
    signed.files.push(("META-INF/documentsignatures.xml".to_owned(), b"<dsig:document-signatures xmlns:dsig=\"urn:example:dsig\"/>".to_vec()));
    let cases: Vec<(&str, Vec<u8>, Variant)> = vec![
        ("root drawing, mimetype template (the census file)", write(Some(TEMPLATE), &with_manifest(DRAWING, parts().files())), Variant::Drawing),
        ("root template, mimetype drawing", write(Some(DRAWING), &with_manifest(TEMPLATE, parts().files())), Variant::Template),
        ("no mimetype", write(None, &with_manifest(TEMPLATE, parts().files())), Variant::Template),
        ("mimetype second and deflated", mimetype_second(with_manifest(DRAWING, parts().files()), DRAWING), Variant::Drawing),
        ("mimetype second, deflated, no root row", mimetype_second(no_root_entry(), TEMPLATE), Variant::Template),
        ("signed, mimetype disagrees", write(Some(TEMPLATE), &with_manifest(DRAWING, signed.files())), Variant::Drawing),
    ];
    for (what, bytes, says) in cases {
        let o = open_bytes(bytes);
        assert_eq!(sniff(&o.bytes), Some(says), "{what}");
        let irregular =
            |bytes: &[u8]| Package::read(bytes, &Limits::default()).unwrap().warnings().iter().filter(|w| w.class() == "mimetype_irregular").count();
        assert_eq!(irregular(&o.bytes), 1, "{what}: the file's own mimetype is not as R2 asks");
        // Nothing asked: the file as it is, whatever is wrong with its `mimetype`.
        assert_eq!(as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap().bytes, o.bytes, "{what}");
        // The kind it says asked for: `mimetype` first, stored, saying it; the manifest and every other part the file's own.
        let saved = as_variant(Some(says))(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(!saved.edited, "{what}");
        assert_eq!(&saved.bytes[..2], b"PK", "{what}");
        assert_eq!(&saved.bytes[30..38], b"mimetype", "{what}");
        assert_eq!(&saved.bytes[38..38 + says.media_type().len()], says.media_type().as_bytes(), "{what}");
        assert_eq!((irregular(&saved.bytes), sniff(&saved.bytes)), (0, Some(says)), "{what}");
        assert_eq!(part_bytes(&saved.bytes, MANIFEST_PATH), part_bytes(&o.bytes, MANIFEST_PATH), "{what}: the root row says it already");
        let diff = compare(&o.bytes, &saved.bytes);
        assert!(diff.parts.is_empty() && diff.removed.is_empty() && diff.unparsed.is_empty(), "{what}: {diff:?}");
        assert!(diff.methods_changed.iter().all(|n| n == "mimetype"), "{what}: {diff:?}");
        assert!(diff.bytes_only.iter().chain(&diff.added).all(|n| n == "mimetype"), "{what}: only `mimetype` is written: {diff:?}");
        // A signed package says its signature no longer matches; an unsigned one says nothing.
        let signed = Package::read(&o.bytes, &Limits::default()).unwrap().signed();
        assert_eq!(saved.notes, if signed { vec![SaveNote::SignatureStale] } else { Vec::new() }, "{what}");
        // And again, asking and not asking: no edit, the same bytes.
        for asked in [Some(says), None] {
            let again = as_variant(asked)(&o.doc, &saved.bytes, &saved.index).unwrap();
            assert!(!again.edited && again.notes.is_empty(), "{what}, {asked:?}");
            assert_eq!(again.bytes, saved.bytes, "{what}, {asked:?}");
        }
    }
}

/// Census: a manifest flagged with a data descriptor, as streaming writers write it (the local header states no checksum and no sizes, and the
/// 16 bytes after the data do). A save that changes the entry writes it under headers that state both and no descriptor, and the comparison does not call
/// the bit a change of the header: 16 of 19 real templates carry one.
fn template_with_a_descriptor() -> Vec<u8> {
    use crate::zipbuild::{Item, build};
    let entries: Vec<(String, Vec<u8>)> = with_manifest(TEMPLATE, parts().files()).into_iter().filter(|(name, _)| !name.ends_with('/')).collect();
    let mut items = vec![Item::stored("mimetype", TEMPLATE.as_bytes())];
    for (name, data) in &entries {
        let item = if name.ends_with(".png") { Item::stored(name, data) } else { Item::deflated(name, data) };
        items.push(if name == MANIFEST_PATH {
            let mut descriptor = 0x0807_4B50u32.to_le_bytes().to_vec();
            descriptor.extend_from_slice(&item.crc.to_le_bytes());
            descriptor.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
            descriptor.extend_from_slice(&(item.size as u32).to_le_bytes());
            Item { after_data: descriptor, ..item.with_flags(0x0008) }
        } else {
            item
        });
    }
    let mut bytes = build(&items);
    let at = Package::read(&bytes, &Limits::default()).unwrap().entry(MANIFEST_PATH).unwrap().local_offset;
    bytes[at + 14..at + 26].fill(0);
    bytes
}

#[test]
fn a_manifest_with_a_data_descriptor_is_written_without_one_and_that_is_no_change_of_its_header() {
    let template = template_with_a_descriptor();
    let flags_of = |bytes: &[u8]| Package::read(bytes, &Limits::default()).unwrap().entry(MANIFEST_PATH).unwrap().flags;
    assert_eq!(flags_of(&template) & 0x0008, 0x0008, "the fixture's manifest announces a descriptor");
    for edited in [false, true] {
        let mut o = open_bytes(template.clone());
        assert_eq!(sniff(&o.bytes), Some(Variant::Template));
        if edited {
            edit(&mut o);
        }
        let saved = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
        let what = format!("edited {edited}");
        assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing), "{what}");
        assert_eq!(flags_of(&saved.bytes) & 0x0008, 0, "{what}: the manifest is written under headers that state its sizes, with no descriptor");
        let diff = compare(&o.bytes, &saved.bytes);
        assert!(diff.headers_changed.is_empty(), "{what}: the descriptor's bit is the save's: {:?}", diff.headers_changed);
        assert!(diff.methods_changed.is_empty() && diff.added.is_empty() && diff.removed.is_empty() && !diff.order_changed, "{what}: {diff:?}");
        let manifest = diff.parts.iter().find(|p| p.part == MANIFEST_PATH).expect("the manifest changed");
        assert_eq!((manifest.counts.added, manifest.counts.removed, manifest.changed.len()), (0, 0, 1), "{what}: {manifest:?}");
        assert_eq!(manifest.changed.first().map(|c| c.attributes.clone()), Some(vec!["manifest:media-type".to_owned()]), "{what}");
        assert!(diff.bytes_only.iter().any(|n| n == "mimetype"), "{what}: {diff:?}");
        // The copy of every other entry is the file's, and a second save is no edit.
        let again = as_variant(Some(Variant::Drawing))(&o.doc, &saved.bytes, &saved.index).unwrap();
        assert!(!again.edited, "{what}");
        assert_eq!(again.bytes, saved.bytes, "{what}");
    }
    // Saved as a template, which it is, an unedited save is the file with its descriptor.
    let o = open_bytes(template.clone());
    assert_eq!(as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap().bytes, template);
}

#[test]
fn the_root_row_is_edited_in_the_quotes_prefix_order_and_spelling_the_file_wrote_it() {
    let uri = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
    let rows = |p: &str, q: char, order: bool| -> String {
        let row = |path: &str, media: &str| {
            if order {
                format!("\t<{p}file-entry {p}media-type={q}{media}{q} {p}full-path={q}{path}{q}/>\n")
            } else {
                format!("\t<{p}file-entry {p}full-path={q}{path}{q} {p}media-type={q}{media}{q}/>\n")
            }
        };
        row("content.xml", "text/xml") + &row("styles.xml", "text/xml")
    };
    let root = |p: &str, q: char, media: &str| format!("\t<{p}file-entry {p}media-type={q}{media}{q} {p}full-path={q}/{q}/>\n");
    let reference = TEMPLATE.replace('-', "&#45;");
    // (what, the manifest, the root row's text of the media type as written there)
    let cases: Vec<(&str, String, String)> = vec![
        (
            "single quotes, attributes in another order, a document type",
            format!(
                "<?xml version='1.0' encoding='UTF-8'?>\n<!DOCTYPE manifest:manifest PUBLIC \"-//Example//DTD Manifest 1.0//EN\" \"Manifest.dtd\">\n<manifest:manifest xmlns:manifest='{uri}' manifest:version='1.2'>\n{}{}</manifest:manifest>\n",
                root("manifest:", '\'', TEMPLATE),
                rows("manifest:", '\'', true)
            ),
            format!("'{TEMPLATE}'"),
        ),
        (
            "another prefix",
            format!("<m:manifest xmlns:m=\"{uri}\">\n{}{}</m:manifest>", root("m:", '"', TEMPLATE), rows("m:", '"', false)),
            format!("\"{TEMPLATE}\""),
        ),
        (
            "a character reference in the value",
            format!(
                "<manifest:manifest xmlns:manifest=\"{uri}\">\n{}{}</manifest:manifest>",
                root("manifest:", '"', &reference),
                rows("manifest:", '"', false)
            ),
            format!("\"{reference}\""),
        ),
        (
            "the root row last, with spaces about the equals signs",
            format!(
                "<manifest:manifest xmlns:manifest=\"{uri}\">\n{}\t<manifest:file-entry   manifest:full-path = \"/\"   manifest:media-type = \"{TEMPLATE}\" />\n</manifest:manifest>",
                rows("manifest:", '"', false)
            ),
            format!("\"{TEMPLATE}\""),
        ),
        (
            "two root rows, the first the one that counts",
            format!(
                "<manifest:manifest xmlns:manifest=\"{uri}\">\n{}{}{}</manifest:manifest>",
                root("manifest:", '"', TEMPLATE),
                root("manifest:", '"', TEMPLATE),
                rows("manifest:", '"', false)
            ),
            format!("\"{TEMPLATE}\""),
        ),
        // The root row's path is a numeric reference of more than ten characters, which the XML parser decodes; the writer's own
        // scan found no root row, and refused the save as a missing root entry (and with a literal `/` row after it, as a failed self-check).
        (
            "the root row's path a long numeric reference",
            format!(
                "<manifest:manifest xmlns:manifest=\"{uri}\">\n{}{}</manifest:manifest>",
                root("manifest:", '"', TEMPLATE).replace("manifest:full-path=\"/\"", "manifest:full-path=\"&#0000000047;\""),
                rows("manifest:", '"', false)
            ),
            format!("\"{TEMPLATE}\""),
        ),
        (
            "a long numeric reference for `/`, a literal `/` row after it",
            format!(
                "<manifest:manifest xmlns:manifest=\"{uri}\">\n{}{}{}</manifest:manifest>",
                root("manifest:", '"', TEMPLATE).replace("manifest:full-path=\"/\"", "manifest:full-path=\"&#x000000002F;\""),
                root("manifest:", '"', TEMPLATE),
                rows("manifest:", '"', false)
            ),
            format!("\"{TEMPLATE}\""),
        ),
    ];
    for (what, manifest, written) in cases {
        let mut entries = Drawing::a4().page(&crate::write_core::custom("s1", "1cm")).files();
        entries.push((MANIFEST_PATH.to_owned(), manifest.clone().into_bytes()));
        let bytes = write(Some(TEMPLATE), &entries);
        let o = open_bytes(bytes);
        assert_eq!(sniff(&o.bytes), Some(Variant::Template), "{what}");
        let saved = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        let quote = &written[..1];
        let expected = manifest.replacen(&written, &format!("{quote}{DRAWING}{quote}"), 1);
        assert_eq!(part(&saved.bytes, MANIFEST_PATH), expected, "{what}: only the value of the root row's media type, whole, in its quotes");
        assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing), "{what}");
        let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
        assert!(package.mimetype_regular(), "{what}: {:?}", package.warnings());
        // And an edit beside it, which adds rows after the last: the retype and the rows are one splice.
        let mut moved = open_bytes(o.bytes.clone());
        move_by(&mut moved.doc, 0, 1, 1.0, 0.0);
        let saved = as_variant(Some(Variant::Drawing))(&moved.doc, &moved.bytes, &moved.index).unwrap_or_else(|e| panic!("{what}, edited: {e}"));
        let text = part(&saved.bytes, MANIFEST_PATH);
        // The root row's value is the one that changed (a second root row, which the package layer does not read, is as it was).
        assert!(text.contains(&format!("{quote}{DRAWING}{quote}")), "{what}: {text}");
        assert_eq!(text.matches(&written).count() + 1, manifest.matches(&written).count(), "{what}: {text}");
        assert!(text.contains("Thumbnails/thumbnail.png") && text.contains("meta.xml"), "{what}: the rows an edit adds: {text}");
        assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing), "{what}");
    }
}

/// The archive with the first entry's modification time and date set in both its headers: the first local header, and the first row of the directory.
fn with_first_entry_dated(mut bytes: Vec<u8>, time: u16, date: u16) -> Vec<u8> {
    bytes[10..12].copy_from_slice(&time.to_le_bytes());
    bytes[12..14].copy_from_slice(&date.to_le_bytes());
    let end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    let dir = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    bytes[dir + 12..dir + 14].copy_from_slice(&time.to_le_bytes());
    bytes[dir + 14..dir + 16].copy_from_slice(&date.to_le_bytes());
    bytes
}

#[test]
fn a_regular_mimetype_keeps_its_header_in_every_save_and_when_the_media_type_changes() {
    // A changed entry keeps its headers, the checksum and the sizes aside, and an entry that is not changed is copied; only an irregular `mimetype`
    // is made anew. The date of the entry is what tells a header kept from one made again.
    let dated = with_first_entry_dated(a_template(), 0x6B20, 0x5A21);
    assert_eq!(&dated[..2], b"PK");
    for asked in [None, Some(Variant::Template), Some(Variant::Drawing)] {
        for edited in [false, true] {
            let mut o = open_bytes(dated.clone());
            if edited {
                edit(&mut o);
            }
            let saved = as_variant(asked)(&o.doc, &o.bytes, &o.index).unwrap();
            let what = format!("asked {asked:?}, edited {edited}");
            assert_eq!(&saved.bytes[10..12], &0x6B20u16.to_le_bytes(), "{what}: the time");
            assert_eq!(&saved.bytes[12..14], &0x5A21u16.to_le_bytes(), "{what}: the date");
            let media = asked.unwrap_or(Variant::Template).media_type();
            assert_eq!(&saved.bytes[38..38 + media.len()], media.as_bytes(), "{what}");
            let diff = compare(&o.bytes, &saved.bytes);
            assert!(diff.headers_changed.is_empty(), "{what}: {:?}", diff.headers_changed);
        }
    }
    // An irregular one (here: second in the archive, so its header is not where R2 asks) is written as R2 asks when the save is an edit, or changes the
    // media type: first, with a header made anew.
    let mut entries = with_manifest(TEMPLATE, parts().files());
    entries.insert(1, ("mimetype".to_owned(), TEMPLATE.as_bytes().to_vec()));
    let second = write(None, &entries);
    assert_ne!(&second[30..38], b"mimetype");
    let mut o = open_bytes(second);
    for asked in [None, Some(Variant::Drawing)] {
        edit(&mut o);
        let saved = as_variant(asked)(&o.doc, &o.bytes, &o.index).unwrap();
        assert_eq!(&saved.bytes[30..38], b"mimetype", "asked {asked:?}");
        assert!(Package::read(&saved.bytes, &Limits::default()).unwrap().mimetype_regular());
    }
}

#[test]
fn a_signed_package_saved_as_another_kind_says_the_signature_no_longer_matches() {
    // Every file of the package is referenced by the document signature, `mimetype` and the manifest among them, so a change of the media
    // type breaks it; the file is kept as it was. A template saved as a template is not changed, and says nothing.
    let mut d = parts();
    d.files.push(("META-INF/documentsignatures.xml".to_owned(), b"<dsig:document-signatures xmlns:dsig=\"urn:example:dsig\"/>".to_vec()));
    let o = open_bytes(package(TEMPLATE, d.files()));
    assert!(Package::read(&o.bytes, &Limits::default()).unwrap().signed());
    let kept = as_variant(Some(Variant::Template))(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(kept.notes.is_empty());
    assert_eq!(kept.bytes, o.bytes);
    let saved = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(saved.notes, [SaveNote::SignatureStale]);
    assert_eq!(part_bytes(&saved.bytes, "META-INF/documentsignatures.xml"), part_bytes(&o.bytes, "META-INF/documentsignatures.xml"));
    let diff = compare(&o.bytes, &saved.bytes);
    assert_eq!(diff.bytes_only, ["mimetype"], "{diff:?}");
    assert_eq!(diff.identical, 7);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Cost: counted, in the units the loops walk.

fn many_rows(n: usize) -> Vec<u8> {
    let mut d = drawing_of(2);
    for k in 0..n {
        d.files.push((format!("Pictures/p{k}.png"), PNG.to_vec()));
    }
    package(TEMPLATE, d.files())
}

#[test]
fn a_change_of_media_type_costs_one_pass_over_the_manifest_and_one_edit() {
    // The manifest is scanned once (its bytes, its tags), the splice makes one edit that writes the new media type, and nothing else of the package is
    // looked at: no part is scanned, no entry is looked up in the rows. Counted at 1,000 and at 4,000 rows.
    let cost = |n: usize| {
        let o = open_bytes(many_rows(n));
        let mut work = WriteWork::default();
        let options = WriteOptions { variant: Some(Variant::Drawing), ..WriteOptions::default() };
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work).unwrap();
        let original = part(&o.bytes, MANIFEST_PATH);
        let rows = original.matches("<manifest:file-entry").count() as u64;
        assert_eq!(rows, n as u64 + 3, "the root, the two parts and a row for each picture");
        assert_eq!(sniff(&saved.bytes), Some(Variant::Drawing));
        println!("COST retype at {n} pictures: {work:?}");
        assert_eq!(work.scan_bytes, original.len() as u64, "only the manifest was scanned, once");
        assert_eq!(work.elements_walked, rows + 2, "each tag of the manifest once: the rows and the root's two");
        assert_eq!(work.edits, 1, "one edit: the root row's media type");
        assert_eq!(work.bytes_written, DRAWING.len() as u64, "and the text it wrote is the new media type");
        assert_eq!(
            (work.manifest_probes, work.manifest_compares, work.attributes_written),
            (0, 0, 0),
            "no entry is looked up and no attribute written"
        );
        work
    };
    let (a, b) = (cost(1_000), cost(4_000));
    for (what, small, big) in [("bytes scanned", a.scan_bytes, b.scan_bytes), ("elements walked", a.elements_walked, b.elements_walked)] {
        let ratio = big as f64 / small as f64;
        assert!((3.9..4.1).contains(&ratio), "{what}: {small} at 1,000 rows and {big} at 4,000, a ratio of {ratio:.3}");
    }
    // The bytes copied are the entries' compressed bytes, a fixed part and a little for each row.
    assert!(b.bytes_copied > a.bytes_copied && b.bytes_copied < 5 * a.bytes_copied, "{} and {}", a.bytes_copied, b.bytes_copied);
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// Fuzzing (AGENTS.md): damaged templates and any kind asked for give a package or an error, and a package that holds what was asked.

/// The classes of the errors a save may give.
const CLASSES: &[&str] = &[
    "not_zip",
    "zip_corrupt",
    "path_traversal",
    "invalid_part_name",
    "zip_ratio_exceeded",
    "part_too_large",
    "package_too_large",
    "too_many_parts",
    "unsupported_compression",
    "encrypted_package",
    "duplicate_part",
    "out_of_memory",
    "xml_malformed",
    "xml_entity_refused",
    "xml_too_complex",
    "missing_manifest",
    "manifest_malformed",
    "not_a_drawing",
    "no_drawing_content",
    "too_many_manifest_entries",
    "too_much_xml",
    "write_media_type",
    "write_unsupported",
    "write_mismatch",
    "write_limit",
];

/// Saves what `bytes` reads as, edited or not, asking for `asked`: a package that holds what was asked and saves again as no edit, or a known
/// class.
fn check(bytes: &[u8], asked: Option<Variant>, edited: bool) {
    let Ok((doc, report)) = read(bytes, &Limits::default()) else { return };
    let mut o = Open { bytes: bytes.to_vec(), doc, index: report.index };
    if edited {
        edit_if_any(&mut o);
    }
    let says = sniff(bytes);
    match as_variant(asked)(&o.doc, &o.bytes, &o.index) {
        Ok(saved) => {
            // A part of the file that was damaged in a way the read does not look at (a picture's bytes) is copied as it is, damaged: the save reads
            // exactly when the file does.
            let Ok(package) = Package::read(&saved.bytes, &Limits::default()) else {
                assert!(Package::read(bytes, &Limits::default()).is_err(), "a save that does not read, of a file that does");
                return;
            };
            if let Some(asked) = asked {
                assert_eq!(package.variant(), Some(asked), "the package holds the kind that was asked");
                // A save that asked for a kind states it, edited or not: `mimetype` is as R2 asks.
                assert!(package.mimetype_regular(), "{:?}", package.warnings());
            }
            // An unedited save of another kind than the package's own changes the root row's media type, the one element, and
            // the bytes of `mimetype`, and nothing else; whatever the manifest and the archive are like.
            if let (Some(asked), false) = (asked, saved.edited)
                && says != Some(asked)
                && let Ok(diff) = diff_packages(bytes, &saved.bytes, &Limits::default())
            {
                assert_eq!(diff.parts.iter().map(|p| p.part.as_str()).collect::<Vec<_>>(), [MANIFEST_PATH], "{diff:?}");
                let manifest = &diff.parts[0];
                assert_eq!((manifest.counts.added, manifest.counts.removed, manifest.changed.len()), (0, 0, 1), "{manifest:?}");
                let attributes = &manifest.changed[0].attributes;
                assert!(attributes.len() == 1 && attributes[0].rsplit(':').next() == Some("media-type"), "{attributes:?}");
                assert!(diff.removed.is_empty() && diff.unparsed.is_empty(), "{diff:?}");
                assert!(diff.bytes_only.iter().chain(&diff.added).all(|n| n == "mimetype"), "{diff:?}");
                // A `mimetype` as R2 asks is replaced where it lies, with the kind's bytes (which differ), and no header of any entry changes but
                // what a copy of an archive that is not whole rewrites.
                if Package::read(bytes, &Limits::default()).is_ok_and(|p| p.mimetype_regular()) {
                    assert_eq!(diff.bytes_only, ["mimetype"], "{diff:?}");
                    assert!(diff.methods_changed.is_empty() && !diff.order_changed && diff.added.is_empty(), "{diff:?}");
                    if copy_account(bytes, &Limits::default()).is_ok_and(|a| a.is_exact()) {
                        assert!(diff.headers_changed.is_empty(), "{diff:?}");
                    }
                }
            }
            let again = as_variant(asked)(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("a second save was refused: {e}"));
            assert!(!again.edited, "a second save is no edit");
            assert_eq!(again.bytes, saved.bytes);
        }
        Err(e) => assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}"),
    }
}

/// Moves the first shape of the first page when there is one.
fn edit_if_any(o: &mut Open) {
    let Some(id) = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id) else { return };
    move_by(&mut o.doc, 0, id, 1.0, 0.0);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn a_damaged_template_saved_as_any_kind_gives_a_package_or_an_error(
        flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..5),
        asked in prop_oneof![Just(None), Just(Some(Variant::Drawing)), Just(Some(Variant::Template))],
        edited in any::<bool>(),
    ) {
        let mut bytes = a_template();
        for (at, value) in flips {
            let i = at.index(bytes.len());
            bytes[i] = value;
        }
        check(&bytes, asked, edited);
    }

    #[test]
    fn a_manifest_of_any_shape_saved_as_any_kind_gives_a_package_or_an_error(
        root in prop_oneof![Just(TEMPLATE.to_owned()), Just(DRAWING.to_owned()), Just(String::new()), "[a-z./-]{0,20}"],
        mimetype in prop_oneof![Just(Some(TEMPLATE)), Just(Some(DRAWING)), Just(None)],
        rows in proptest::collection::vec(("[a-zA-Z0-9 ./%-]{0,12}", "[a-z/.-]{0,10}"), 0..6),
        duplicate_root in any::<bool>(),
        asked in prop_oneof![Just(None), Just(Some(Variant::Drawing)), Just(Some(Variant::Template))],
        edited in any::<bool>(),
    ) {
        let mut text = String::from("<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">");
        text.push_str(&format!("<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{root}\"/>"));
        if duplicate_root {
            text.push_str(&format!("<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{TEMPLATE}\"/>"));
        }
        for (path, media) in &rows {
            text.push_str(&format!("<manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{media}\"/>"));
        }
        text.push_str("</manifest:manifest>");
        let mut entries = parts().files();
        entries.push((MANIFEST_PATH.to_owned(), text.into_bytes()));
        check(&write(mimetype, &entries), asked, edited);
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------------
// A black box (LibreOffice, `soffice --headless --convert-to pdf`): the files to open, written by hand.

/// Writes a template, the template saved as a drawing (unedited and edited), and the drawing saved as a template to the folder in
/// `DIACRAFT_TEMPLATE_DIR`, for another reader to convert. Not a test unless asked for.
#[test]
#[ignore = "writes template and drawing files to the folder in DIACRAFT_TEMPLATE_DIR, for another reader to convert"]
fn write_templates_and_drawings() {
    let Some(dir) = std::env::var_os("DIACRAFT_TEMPLATE_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let template = a_template();
    std::fs::write(dir.join("template.otg"), &template).unwrap();
    let mut o = open_bytes(template);
    let unedited = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
    std::fs::write(dir.join("template-as-drawing.odg"), &unedited.bytes).unwrap();
    std::fs::write(dir.join("template-as-template.otg"), &as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    edit(&mut o);
    let edited = as_variant(Some(Variant::Drawing))(&o.doc, &o.bytes, &o.index).unwrap();
    std::fs::write(dir.join("template-edited-as-drawing.odg"), &edited.bytes).unwrap();
    std::fs::write(dir.join("template-edited-as-template.otg"), &as_variant(None)(&o.doc, &o.bytes, &o.index).unwrap().bytes).unwrap();
    let drawing = open_bytes(a_drawing());
    let as_template = as_variant(Some(Variant::Template))(&drawing.doc, &drawing.bytes, &drawing.index).unwrap();
    std::fs::write(dir.join("drawing-as-template.otg"), &as_template.bytes).unwrap();
}
