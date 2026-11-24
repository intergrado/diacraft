//! The package layer: what a package says it is, its manifest, encryption, signatures, macros,
//! and what is kept.

use diacraft_odg::package::{MANIFEST_PATH, Package};
use diacraft_odg::{Limits, Manifest, MimetypeFault, Variant, Warning, Work, copy_unchanged, part_label, sniff};

use crate::build::{DRAWING, PRESENTATION, TEMPLATE, content, drawing, files, manifest, package, page, shapes, with_manifest, write};
use crate::zipbuild::{Item, build};

fn read(bytes: &[u8]) -> Package {
    Package::read(bytes, &Limits::default()).unwrap()
}

fn class(bytes: &[u8]) -> &'static str {
    match Package::read(bytes, &Limits::default()) {
        Ok(_) => "read",
        Err(e) => e.class(),
    }
}

fn classes(package: &Package) -> Vec<&'static str> {
    package.warnings().iter().map(Warning::class).collect()
}

/// The four XML parts and a manifest, as items of the tests' own writer, after `first`.
fn items_after(first: Vec<Item>, media: &str) -> Vec<Item> {
    let mut items = first;
    for (name, data) in with_manifest(media, files(&page("p1", &shapes(1)))) {
        items.push(Item::deflated(&name, &data));
    }
    items
}

#[test]
fn a_drawing_is_read_whole_with_every_entry_by_its_name() {
    let bytes = drawing(&page("p1", &shapes(2)));
    let package = read(&bytes);
    assert_eq!(package.variant(), Some(Variant::Drawing));
    assert_eq!(package.media_type(), Some(DRAWING));
    assert!(package.mimetype_regular());
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
    let names: Vec<&str> = package.parts().collect();
    assert_eq!(names, ["mimetype", "content.xml", "styles.xml", "meta.xml", "settings.xml", MANIFEST_PATH]);
    assert_eq!(package.part("content.xml").unwrap(), content(&page("p1", &shapes(2))).as_bytes());
    assert_eq!(package.part("mimetype").unwrap(), DRAWING.as_bytes());
    assert_eq!(package.rows(), 6);
    let manifest = package.manifest();
    assert_eq!(manifest.version.as_deref(), Some("1.3"));
    assert_eq!(manifest.entries.len(), 5);
    assert_eq!(manifest.root().unwrap().media_type.as_deref(), Some(DRAWING));
    assert_eq!(manifest.entry("content.xml").unwrap().media_type.as_deref(), Some("text/xml"));
    assert!(!manifest.encrypted() && !package.signed() && !package.has_macros());
    assert_eq!(sniff(&bytes), Some(Variant::Drawing));
}

#[test]
fn sniff_tells_a_drawing_a_template_and_anything_else() {
    // The template's media type gives `Template`; a presentation's gives `None`, and reading it is `not_a_drawing`.
    let template = package(TEMPLATE, files(""));
    assert_eq!(sniff(&template), Some(Variant::Template));
    assert_eq!(read(&template).variant(), Some(Variant::Template));
    let presentation = package(PRESENTATION, files(""));
    assert_eq!(sniff(&presentation), None);
    assert_eq!(class(&presentation), "not_a_drawing");
    // A flat XML drawing is not a package; neither is text; a ZIP with no manifest is no OpenDocument package.
    assert_eq!(sniff(content("").as_bytes()), None);
    assert_eq!(sniff(b"not a zip at all"), None);
    assert_eq!(sniff(&write(Some(DRAWING), &[("content.xml".to_owned(), content("").into_bytes())])), None);
    assert_eq!(Variant::Drawing.extension(), "odg");
    assert_eq!((Variant::Template.extension(), Variant::Template.media_type()), ("otg", TEMPLATE));
}

#[test]
fn mimetype_not_first_is_read_with_a_warning() {
    // A `mimetype` that is not first is read, with one `mimetype_irregular` warning, and the media type taken from
    // the manifest's root row.
    let second = build(&items_after(vec![Item::deflated("a.xml", b"<a/>"), Item::stored("mimetype", DRAWING.as_bytes())], DRAWING));
    let package = read(&second);
    assert_eq!(package.variant(), Some(Variant::Drawing));
    assert_eq!(package.warnings().iter().filter(|w| w.class() == "mimetype_irregular").count(), 1, "{:?}", package.warnings());
    assert!(matches!(&package.warnings()[0], Warning::MimetypeIrregular { faults } if faults == &[MimetypeFault::NotFirst]));
    assert!(!package.mimetype_regular());
    // Absent, with the root row: the drawing is read.
    let absent = build(&items_after(Vec::new(), DRAWING));
    let package = read(&absent);
    assert_eq!(package.variant(), Some(Variant::Drawing));
    assert!(matches!(&package.warnings(), [Warning::MimetypeIrregular { faults }] if faults == &[MimetypeFault::Absent]));
    // Deflated, and stored with an extra field (an extended timestamp in its local header): each read, each with its fault.
    let deflated = build(&items_after(vec![Item::deflated("mimetype", DRAWING.as_bytes())], DRAWING));
    assert!(matches!(&read(&deflated).warnings(), [Warning::MimetypeIrregular { faults }] if faults == &[MimetypeFault::Compressed]));
    let stamped = build(&items_after(vec![Item::stored("mimetype", DRAWING.as_bytes()).with_local_extra(&[0x55, 0x54, 1, 0, 1])], DRAWING));
    assert!(matches!(&read(&stamped).warnings(), [Warning::MimetypeIrregular { faults }] if faults == &[MimetypeFault::ExtraField]));
    // Several faults are one warning.
    let both = build(&items_after(vec![Item::deflated("x.xml", b"<x/>"), Item::deflated("mimetype", DRAWING.as_bytes())], DRAWING));
    let package = read(&both);
    assert_eq!(classes(&package), ["mimetype_irregular", "manifest_incomplete"], "x.xml has no row");
    assert!(
        matches!(&package.warnings()[0], Warning::MimetypeIrregular { faults } if faults == &[MimetypeFault::NotFirst, MimetypeFault::Compressed])
    );
}

#[test]
fn the_manifests_root_row_wins_over_mimetype() {
    // Where `mimetype` and the root row disagree, the root row wins, with the same warning. A drawing whose `mimetype` says presentation
    // is a drawing; a presentation whose `mimetype` says drawing is not.
    let says_presentation = write(Some(PRESENTATION), &with_manifest(DRAWING, files("")));
    let package = read(&says_presentation);
    assert_eq!(package.variant(), Some(Variant::Drawing));
    assert!(matches!(&package.warnings(), [Warning::MimetypeIrregular { faults }] if faults == &[MimetypeFault::DisagreesWithManifest]));
    assert_eq!(sniff(&says_presentation), Some(Variant::Drawing));
    let is_presentation = write(Some(DRAWING), &with_manifest(PRESENTATION, files("")));
    assert_eq!(class(&is_presentation), "not_a_drawing");
    // A manifest with no root row: `mimetype` says, with a warning that the root row is missing (R8 requires it once `mimetype` exists).
    let rows = [("content.xml", "text/xml")];
    let no_root = manifest(DRAWING, &rows)
        .replace(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{DRAWING}\"/>\n"), "");
    let mut entries = vec![("content.xml".to_owned(), content("").into_bytes())];
    entries.push((MANIFEST_PATH.to_owned(), no_root.clone().into_bytes()));
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(package.variant(), Some(Variant::Drawing));
    assert_eq!(classes(&package), ["manifest_no_root_entry"]);
    // Neither says: read, with no variant, for the drawing's reader to decide.
    let package = read(&write(None, &entries));
    assert_eq!(package.variant(), None);
    assert_eq!(classes(&package), ["mimetype_irregular", "media_type_unknown"]);
    // A `mimetype` of another type with no root row is refused.
    assert_eq!(class(&write(Some(PRESENTATION), &entries)), "not_a_drawing");
    // A `mimetype` that is not a media type (not printable ASCII, or longer than the cap) is not read as one.
    let odd = build(&[vec![Item::stored("mimetype", b"application/vnd.oasis.opendocument.graphics\n")], entries_items(&entries)].concat());
    assert!(matches!(&read(&odd).warnings()[0], Warning::MimetypeIrregular { faults } if faults == &[MimetypeFault::Unreadable]));
    let long = build(&[vec![Item::stored("mimetype", &[b'a'; 257])], entries_items(&entries)].concat());
    assert!(matches!(&read(&long).warnings()[0], Warning::MimetypeIrregular { faults } if faults == &[MimetypeFault::Unreadable]));
}

fn entries_items(entries: &[(String, Vec<u8>)]) -> Vec<Item> {
    entries.iter().map(|(n, d)| Item::deflated(n, d)).collect()
}

#[test]
fn a_missing_or_malformed_manifest_is_refused() {
    // No manifest is `missing_manifest`; a manifest rooted in `manifest:files` is `manifest_malformed`, and so is one that is
    // not well-formed; a manifest whose prefix is bound to another namespace is not the manifest.
    assert_eq!(class(&write(Some(DRAWING), &files(""))), "missing_manifest");
    let with = |text: &str| {
        let mut entries = files("");
        entries.push((MANIFEST_PATH.to_owned(), text.as_bytes().to_vec()));
        write(Some(DRAWING), &entries)
    };
    let files_root = r#"<manifest:files xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
    assert_eq!(class(&with(files_root)), "manifest_malformed");
    assert_eq!(class(&with("<manifest:manifest")), "manifest_malformed");
    assert_eq!(class(&with(r#"<manifest:manifest xmlns:manifest="urn:example:other"/>"#)), "manifest_malformed");
    assert_eq!(class(&with("<manifest:manifest/>")), "manifest_malformed", "an unbound prefix is no namespace");
    // A DTD is refused as the XML layer refuses it, with its class.
    assert_eq!(class(&with("<!DOCTYPE m [<!ENTITY a \"b\">]><m/>")), "xml_entity_refused");
    // Another prefix, or the default namespace, bound to the manifest's namespace: the same manifest.
    let other_prefix = concat!(
        r#"<m:manifest xmlns:m="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" m:version="1.3">"#,
        r#"<m:file-entry m:full-path="/" m:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
        r#"<m:file-entry m:full-path="content.xml" m:media-type="text/xml"/></m:manifest>"#
    );
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), other_prefix.as_bytes().to_vec())];
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(package.manifest().entries.len(), 2);
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
    // The default namespace for the elements; the attributes need a prefix, since an unprefixed attribute is in no namespace.
    let default_ns = concat!(
        r#"<manifest xmlns="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" "#,
        r#"xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
        r#"<file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
        r#"<file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        r#"<file-entry full-path="ignored" media-type="text/xml"/></manifest>"#
    );
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), default_ns.as_bytes().to_vec())];
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(package.manifest().version.as_deref(), Some("1.3"));
    assert_eq!(package.manifest().entries.len(), 2, "a row whose attributes are in no namespace names no file");
    assert_eq!(classes(&package), ["manifest_entry_incomplete"]);
    // The manifest is held to its own cap, by its header and before anything else is read.
    let small = Limits { max_manifest_bytes: 100, ..Limits::default() };
    let error = Package::read(&drawing(""), &small).unwrap_err();
    assert_eq!(error.class(), "part_too_large", "{error}");
}

#[test]
fn encrypted_package_is_refused() {
    // A manifest in which `content.xml`'s row holds `manifest:encryption-data` refuses the package as `encrypted_package` before
    // any entry but the manifest is inflated.
    let rows = manifest(DRAWING, &[("content.xml", "text/xml"), ("styles.xml", "text/xml")]).replace(
        r#"manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        concat!(
            r#"manifest:full-path="content.xml" manifest:media-type="text/xml" manifest:size="900">"#,
            r#"<manifest:encryption-data manifest:checksum-type="SHA1/1K" manifest:checksum="AAAA">"#,
            r#"<manifest:algorithm manifest:algorithm-name="Blowfish CFB" manifest:initialisation-vector="AAAA"/>"#,
            r#"</manifest:encryption-data></manifest:file-entry>"#
        ),
    );
    // The encrypted entry is stored but holds noise, as R13 says such an entry is.
    let entries = vec![
        ("content.xml".to_owned(), b"\x8f\x12noise that is no XML".to_vec()),
        ("styles.xml".to_owned(), crate::build::styles().into_bytes()),
        (MANIFEST_PATH.to_owned(), rows.clone().into_bytes()),
    ];
    let bytes = write(Some(DRAWING), &entries);
    let mut work = Work::default();
    let error = Package::read_counted(&bytes, &Limits::default(), &mut work).unwrap_err();
    assert_eq!(error.class(), "encrypted_package");
    assert!(error.to_string().contains("password-protected"), "{error}");
    assert_eq!(work.inflated, rows.len() as u64, "only the manifest is inflated");
    // The manifest can still be read, to say so, and the package still says it is a drawing.
    let read_back = Manifest::read(&bytes, &Limits::default()).unwrap();
    assert!(read_back.encrypted() && read_back.entry("content.xml").unwrap().encrypted && !read_back.encrypted_keys);
    assert_eq!(sniff(&bytes), Some(Variant::Drawing));
    // Keys for recipients and no encrypted entry: the same refusal.
    let keyed = manifest(DRAWING, &[("content.xml", "text/xml")])
        .replace("</manifest:manifest>", "<manifest:encrypted-key><manifest:encryption-method/></manifest:encrypted-key></manifest:manifest>");
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), keyed.into_bytes())];
    let bytes = write(Some(DRAWING), &entries);
    assert_eq!(class(&bytes), "encrypted_package");
    assert!(Manifest::read(&bytes, &Limits::default()).unwrap().encrypted_keys);
    // The same elements in another namespace are not encryption.
    let foreign = manifest(DRAWING, &[("content.xml", "text/xml")])
        .replace("</manifest:manifest>", r#"<x:encrypted-key xmlns:x="urn:example:x"/></manifest:manifest>"#);
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), foreign.into_bytes())];
    assert_eq!(class(&write(Some(DRAWING), &entries)), "read");
    // An entry the archive flags as encrypted is refused by the container with the same class and the same words.
    let flagged =
        build(&items_after(vec![Item::stored("mimetype", DRAWING.as_bytes()), Item::stored("Pictures/a.png", b"x").with_flags(1)], DRAWING));
    let error = Package::read(&flagged, &Limits::default()).unwrap_err();
    assert_eq!(error.class(), "encrypted_package");
    assert!(error.to_string().contains("password-protected"));
    assert_eq!(copy_unchanged(&flagged, &Limits::default()).unwrap_err().class(), "encrypted_package");
}

#[test]
fn manifest_lists_absent_part_is_reported() {
    // A manifest row for `Pictures/b.png`, which the package lacks, and a picture `Pictures/a.png` that the manifest does not list:
    // the drawing is read, with one warning of each.
    let mut entries = files("");
    entries.push(("Pictures/a.png".to_owned(), b"\x89PNG".to_vec()));
    let mut rows: Vec<(&str, &str)> = vec![("content.xml", "text/xml"), ("styles.xml", "text/xml"), ("meta.xml", "text/xml")];
    rows.extend([("settings.xml", "text/xml"), ("Pictures/b.png", "image/png")]);
    entries.push((MANIFEST_PATH.to_owned(), manifest(DRAWING, &rows).into_bytes()));
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(
        package.warnings(),
        &[Warning::ManifestIncomplete { name: "Pictures/a.png".to_owned() }, Warning::ManifestListsMissingFile { path: "Pictures/b.png".to_owned() }]
    );
    assert_eq!(classes(&package), ["manifest_incomplete", "manifest_lists_missing_file"]);
    // The manifest's row for the missing file is kept, like every other row.
    assert!(package.manifest().entry("Pictures/b.png").is_some());
    // A folder entry with its row, a sub-document folder that has no entry of its own but files under it, and a row for a
    // folder written without its slash: none is missing. A row for a folder with nothing under it is.
    let mut entries = files("");
    entries.push(("Configurations2/".to_owned(), Vec::new()));
    entries.push(("Object 1/content.xml".to_owned(), content("").into_bytes()));
    entries.push(("Object 2/content.xml".to_owned(), content("").into_bytes()));
    let mut rows: Vec<(&str, &str)> = vec![("content.xml", "text/xml"), ("styles.xml", "text/xml"), ("meta.xml", "text/xml")];
    rows.extend([("settings.xml", "text/xml"), ("Configurations2/", ""), ("Object 1/", "application/vnd.oasis.opendocument.chart")]);
    rows.extend([("Object 1/content.xml", "text/xml"), ("Object 2", "x"), ("Object 2/content.xml", "text/xml"), ("Object 9/", "x")]);
    entries.push((MANIFEST_PATH.to_owned(), manifest(DRAWING, &rows).into_bytes()));
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(package.warnings(), &[Warning::ManifestListsMissingFile { path: "Object 9/".to_owned() }]);
    assert_eq!(package.folders().collect::<Vec<_>>(), ["Configurations2/"]);
    assert!(package.part("Configurations2/").is_none() && package.entry("Configurations2/").is_some_and(|e| e.folder));
    // A row listed twice is one warning, and the first row is the one used.
    let doubled = manifest(DRAWING, &[("content.xml", "text/xml"), ("content.xml", "application/xml")]);
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), doubled.into_bytes())];
    let package = read(&write(Some(DRAWING), &entries));
    assert_eq!(classes(&package), ["manifest_duplicate_entry"]);
    assert_eq!(package.manifest().entry("content.xml").unwrap().media_type.as_deref(), Some("text/xml"));
}

#[test]
fn names_are_compared_exactly_and_kept_with_their_spaces() {
    // `Object 1/content.xml` and `object 1/content.xml` are two entries; two entries named `Pictures/a.png` are
    // `duplicate_part`; `../x.xml` is `path_traversal`.
    let mut entries = files("");
    entries.push(("Object 1/content.xml".to_owned(), b"<a/>".to_vec()));
    entries.push(("object 1/content.xml".to_owned(), b"<b/>".to_vec()));
    let package = read(&package(DRAWING, entries));
    assert_eq!((package.part("Object 1/content.xml").unwrap(), package.part("object 1/content.xml").unwrap()), (&b"<a/>"[..], &b"<b/>"[..]));
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
    let twice = build(&items_after(vec![Item::stored("Pictures/a.png", b"1"), Item::stored("Pictures/a.png", b"2")], DRAWING));
    assert_eq!(class(&twice), "duplicate_part");
    let up = build(&items_after(vec![Item::stored("../x.xml", b"<x/>")], DRAWING));
    assert_eq!(class(&up), "path_traversal");
}

#[test]
fn signatures_are_kept_and_reported() {
    // A file under `META-INF/` whose name holds `signatures` makes the package signed; its bytes are kept, and other
    // `META-INF/` files are kept without a warning.
    let mut entries = files("");
    entries.push(("META-INF/documentsignatures.xml".to_owned(), b"<dsig:document-signatures/>".to_vec()));
    entries.push(("META-INF/extra.xml".to_owned(), b"<x/>".to_vec()));
    let signed = read(&package(DRAWING, entries));
    assert!(signed.signed());
    assert_eq!(signed.part("META-INF/documentsignatures.xml").unwrap(), b"<dsig:document-signatures/>");
    assert!(signed.warnings().is_empty(), "{:?}", signed.warnings());
    // Outside `META-INF/` the name does not sign anything.
    let mut entries = files("");
    entries.push(("signatures.xml".to_owned(), b"<x/>".to_vec()));
    assert!(!read(&package(DRAWING, entries)).signed());
}

#[test]
fn macros_are_found_and_kept_as_bytes() {
    // A file under `Basic/` or `Scripts/`, an `office:script`, or a `script:event-listener` on a shape.
    let mut entries = files("");
    entries.push(("Basic/Standard/Module1.xml".to_owned(), b"<module/>".to_vec()));
    assert!(read(&package(DRAWING, entries)).has_macros());
    let mut entries = files("");
    entries.push(("Scripts/python/a.py".to_owned(), b"print(1)".to_vec()));
    assert!(read(&package(DRAWING, entries)).has_macros());
    let scripts = content("").replace(
        " <office:body>",
        r#" <office:scripts><office:script script:language="ooo:Basic"><x/></office:script></office:scripts><office:body>"#,
    );
    let one = |text: String| read(&package(DRAWING, crate::build::replace(files(""), "content.xml", text.into_bytes())));
    assert!(one(scripts).has_macros());
    let listener = content(&page("p1", &crate::build::shape("s1", "1cm"))).replace(
        "<text:p/>",
        r#"<office:event-listeners><script:event-listener script:language="ooo:script" script:event-name="dom:click" xlink:href="vnd.sun.star.script:x"/></office:event-listeners><text:p/>"#,
    );
    let package = one(listener.clone());
    assert!(package.has_macros());
    assert_eq!(package.part("content.xml").unwrap(), listener.as_bytes(), "kept byte for byte");
    // An empty `office:scripts`, and `office:event-listeners` with nothing in it, run nothing.
    let empty = content("").replace(" <office:body>", "<office:scripts/><office:event-listeners/> <office:body>");
    assert!(!one(empty).has_macros());
    // The namespace decides, not the prefix: a `script` element in another vocabulary is no script.
    let foreign = content("").replace(" <office:body>", r#"<x:script xmlns:x="urn:example:x"/> <office:body>"#);
    assert!(!one(foreign).has_macros());
    // A `content.xml` that does not parse is kept, and said not to have been looked through.
    let package = one("<office:document-content".to_owned());
    assert_eq!(classes(&package), ["part_not_scanned"]);
    assert_eq!(package.part("content.xml").unwrap(), b"<office:document-content");
}

#[test]
fn an_unchanged_copy_is_the_same_file() {
    // Every entry copied as it is; a package this crate's writer made comes back byte for byte, `mimetype` first.
    let mut entries = files(&page("p1", &shapes(3)));
    entries.push(("Configurations2/".to_owned(), Vec::new()));
    entries.push(("Pictures/a b.png".to_owned(), b"\x89PNG".to_vec()));
    entries.push(("Thumbnails/thumbnail.png".to_owned(), b"\x89PNG thumb".to_vec()));
    let bytes = package(DRAWING, entries);
    let copy = copy_unchanged(&bytes, &Limits::default()).unwrap();
    assert_eq!(copy, bytes);
    assert_eq!((&copy[..2], &copy[30..38], &copy[38..38 + DRAWING.len()]), (&b"PK"[..], &b"mimetype"[..], DRAWING.as_bytes()));
    // A package whose `mimetype` is second keeps it second: an unedited save repeats the fault, an edited one mends it.
    let second = build(&items_after(vec![Item::deflated("a.xml", b"<a/>"), Item::stored("mimetype", DRAWING.as_bytes())], DRAWING));
    assert_eq!(copy_unchanged(&second, &Limits::default()).unwrap(), second);
    // What is not a drawing is not copied.
    assert_eq!(copy_unchanged(&package(PRESENTATION, files("")), &Limits::default()).unwrap_err().class(), "not_a_drawing");
}

#[test]
fn a_part_label_is_the_name_only_when_the_format_fixes_it() {
    // A name the format fixes is shown; any other is its role, since a person may have chosen it.
    for name in ["mimetype", "content.xml", "styles.xml", "meta.xml", "settings.xml", "META-INF/manifest.xml", "Thumbnails/thumbnail.png"] {
        assert_eq!(part_label(name), name);
    }
    assert_eq!(part_label("Configurations2/"), "Configurations2/");
    assert_eq!(part_label("Pictures/MyPlan.png"), "picture");
    assert_eq!(part_label("Pictures/100000000000.png"), "picture");
    assert_eq!(part_label("Object 1/content.xml"), "object");
    assert_eq!(part_label("ObjectReplacements/Object 1"), "object");
    assert_eq!(part_label("Basic/Standard/MyModule.xml"), "macro");
    assert_eq!(part_label("Scripts/python/mine.py"), "macro");
    assert_eq!(part_label("META-INF/documentsignatures.xml"), "META-INF/documentsignatures.xml");
    assert_eq!(part_label("META-INF/macrosignatures.xml"), "signature");
    for other in ["MySecretPlan.xml", "META-INF/notes.xml", "content.xml.bak", "", "Thumbnails/other.png"] {
        assert_eq!(part_label(other), "a part", "{other}");
    }
}

#[test]
fn a_mimetype_is_read_up_to_its_cap_and_an_empty_one_is_unreadable() {
    let with = |mimetype: &[u8]| {
        let mut items = vec![Item::stored("mimetype", mimetype)];
        items.extend(with_manifest(DRAWING, files("")).iter().map(|(n, d)| Item::deflated(n, d)));
        read(&build(&items))
    };
    let faults = |package: &Package| match package.warnings().first() {
        Some(Warning::MimetypeIrregular { faults }) => faults.clone(),
        other => panic!("{other:?}"),
    };
    // 256 printable bytes are read (and disagree with the root row); 257 are not read at all; none is not a media type.
    assert_eq!(faults(&with(&[b'a'; 256])), [MimetypeFault::DisagreesWithManifest]);
    assert_eq!(faults(&with(&[b'a'; 257])), [MimetypeFault::Unreadable]);
    assert_eq!(faults(&with(b"")), [MimetypeFault::Unreadable]);
    assert_eq!(faults(&with(b"application/vnd.oasis.opendocument graphics")), [MimetypeFault::Unreadable], "a space is no part of a media type");
    assert!(with(DRAWING.as_bytes()).warnings().is_empty());
}

#[test]
fn the_manifest_row_cap_holds_at_its_number() {
    // 50 rows (the root and 49) under a cap of 50 are read; 51 are refused.
    let rows: Vec<String> = (0..50).map(|i| format!("f{i}.xml")).collect();
    let make = |n: usize| {
        let listed: Vec<(&str, &str)> = rows.iter().take(n).map(|r| (r.as_str(), "text/xml")).collect();
        let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), manifest(DRAWING, &listed).into_bytes())];
        write(Some(DRAWING), &entries)
    };
    let capped = Limits { max_manifest_entries: 50, ..Limits::default() };
    let package = Package::read(&make(49), &capped).unwrap();
    assert_eq!(package.manifest().entries.len(), 50);
    let error = Package::read(&make(50), &capped).unwrap_err();
    assert_eq!(error.class(), "too_many_manifest_entries");
}

#[test]
fn encryption_data_anywhere_in_the_manifest_refuses_the_package() {
    // Deeper than a row's own child, outside any row, or in a row that names no file: the manifest says something is encrypted, and the
    // package is refused.
    for inner in [
        r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"><x><manifest:encryption-data/></x></manifest:file-entry>"#,
        r#"<x><manifest:encryption-data/></x>"#,
        r#"<manifest:file-entry manifest:media-type="text/xml"><manifest:encryption-data/></manifest:file-entry>"#,
    ] {
        let text = manifest(DRAWING, &[]).replace("</manifest:manifest>", &format!("{inner}</manifest:manifest>"));
        let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), text.into_bytes())];
        let bytes = write(Some(DRAWING), &entries);
        assert_eq!(class(&bytes), "encrypted_package", "{inner}");
        let read_back = Manifest::read(&bytes, &Limits::default()).unwrap();
        assert!(read_back.encrypted_keys && read_back.entries.iter().all(|e| !e.encrypted), "{inner}");
    }
}

#[test]
fn bytes_after_the_end_record_are_read_past_with_a_warning() {
    let mut bytes = drawing("");
    bytes.extend_from_slice(b"trailing");
    let package = read(&bytes);
    assert_eq!(package.warnings(), &[Warning::BytesAfterEndRecord { count: 8 }]);
}

#[test]
fn a_copy_keeps_mimetype_first_when_the_directory_lists_it_last() {
    // The entries lie in the file with `mimetype` first, and the directory lists them the other way: the copy writes the entries in the
    // order they lie, so `mimetype` stays at byte 0, and every part is the same.
    let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
    items.extend(with_manifest(DRAWING, files(&page("p1", &shapes(2)))).iter().map(|(n, d)| Item::deflated(n, d)));
    let bytes = crate::zipbuild::build_listed(&items, true);
    let original = read(&bytes);
    assert_eq!(original.parts().last(), Some("mimetype"), "the directory lists it last");
    assert!(original.mimetype_regular(), "its local header is at byte 0");
    let copy = copy_unchanged(&bytes, &Limits::default()).unwrap();
    assert_eq!(&copy[30..38], b"mimetype");
    let again = read(&copy);
    assert_eq!(again.parts().next(), Some("mimetype"));
    for name in original.parts() {
        assert_eq!(original.part(name), again.part(name), "{name}");
    }
}

#[test]
fn errors_and_warnings_say_what_they_are_and_repeat_the_file_only_escaped_and_short() {
    use diacraft_odg::{ErrorKind, LimitKind, OdgError};
    // A media type from the file, with a terminal escape and far too long: escaped, and cut to 64 characters.
    let hostile = format!("application/x\u{1b}[31m{}", "y".repeat(200));
    let shown = OdgError::NotADrawing { media_type: hostile }.to_string();
    assert!(!shown.contains('\u{1b}') && shown.contains("\\u{1b}"), "{shown}");
    assert!(shown.contains("…") && shown.len() < 140, "{shown}");
    assert!(shown.starts_with("this is not a drawing"), "{shown}");
    let detail = OdgError::ManifestMalformed { detail: "the root is <x\nmanifest>".to_owned() }.to_string();
    assert!(detail.contains("the root is <x\\nmanifest>"), "{detail}");
    // A cap says what it counts and its number.
    let rows = OdgError::Limit { kind: LimitKind::ManifestEntries, max: 50 }.to_string();
    assert_eq!(rows, "the package holds more than 50 rows in the manifest");
    let xml = OdgError::Limit { kind: LimitKind::XmlBytes, max: 9 }.to_string();
    assert_eq!(xml, "the package holds more than 9 bytes of XML in the parts read");
    // What a person is told, in three kinds.
    assert_eq!(OdgError::EncryptedPackage.kind(), ErrorKind::NotAPackage);
    assert_eq!(OdgError::Limit { kind: LimitKind::XmlBytes, max: 1 }.kind(), ErrorKind::TooBig);
    assert_eq!(OdgError::ManifestMalformed { detail: String::new() }.kind(), ErrorKind::Damaged);
    assert_eq!(Package::read(b"plain words", &Limits::default()).unwrap_err().kind(), ErrorKind::NotAPackage);
    assert_eq!(Package::read(&drawing("")[..40], &Limits::default()).unwrap_err().kind(), ErrorKind::Damaged, "a ZIP cut short");
    // A warning names what the file holds, escaped and cut, and its words do not depend on the name.
    let warning = Warning::ManifestIncomplete { name: format!("Pictures/\u{7}{}", "z".repeat(100)) }.to_string();
    assert!(warning.contains("\\u{7}") && warning.contains("…") && warning.len() < 120, "{warning}");
    let irregular = Warning::MimetypeIrregular { faults: vec![MimetypeFault::NotFirst, MimetypeFault::Compressed] }.to_string();
    assert!(irregular.ends_with("(not_first, compressed)"), "{irregular}");
    // The error underneath: the container's, or the XML one, and none for the rest.
    use std::error::Error;
    assert!(OdgError::Container(diacraft_container::ContainerError::NotZip).source().is_some());
    let xml = diacraft_container::XmlError::TooDeep { limit: 1 };
    assert!(OdgError::Xml { part: "content.xml".to_owned(), error: xml }.source().is_some());
    assert!(OdgError::MissingManifest.source().is_none());
    // A package's own words in a log: its entry names and what it says, never its bytes.
    let shown = format!("{:?}", read(&drawing("")));
    assert!(shown.contains("content.xml") && shown.contains("media_type") && !shown.contains("office:document-content"), "{shown}");
}

/// A package of `content.xml` and the manifest `text`, `mimetype` first.
fn with_manifest_text(text: &str) -> Vec<u8> {
    write(Some(DRAWING), &[("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), text.as_bytes().to_vec())])
}

#[test]
fn the_manifest_is_held_to_its_own_item_cap() {
    // The manifest's tree may hold fewer items than any other part's: the least of the two caps holds.
    let manifest_cap = Limits { max_manifest_nodes: 10, ..Limits::default() };
    assert_eq!(Package::read(&drawing(""), &manifest_cap).unwrap_err().class(), "xml_too_complex");
    let xml_cap = Limits {
        container: diacraft_container::Limits {
            xml: diacraft_container::XmlLimits { max_nodes: 10, ..diacraft_container::XmlLimits::default() },
            ..diacraft_container::Limits::default()
        },
        ..Limits::default()
    };
    assert_eq!(Package::read(&drawing(""), &xml_cap).unwrap_err().class(), "xml_too_complex");
    assert!(Package::read(&drawing(""), &Limits { max_manifest_nodes: 100, ..Limits::default() }).is_ok());
}

#[test]
fn a_prefix_bound_inside_a_row_is_unbound_after_it() {
    // The first row binds `manifest` to another namespace for itself alone: it is no row, and the row after it is the manifest's again.
    let text = format!(
        concat!(
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0">"#,
            r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="{}"/>"#,
            r#"<manifest:file-entry xmlns:manifest="urn:example:other" manifest:full-path="ghost.xml" manifest:media-type="x"/>"#,
            r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
        ),
        DRAWING
    );
    let package = read(&with_manifest_text(&text));
    let paths: Vec<&str> = package.manifest().entries.iter().map(|e| e.full_path.as_str()).collect();
    assert_eq!(paths, ["/", "content.xml"]);
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
}

#[test]
fn a_declaration_is_no_attribute_and_only_rows_are_rows() {
    // `xmlns:full-path` declares a prefix and is not the attribute `full-path`, though a (forbidden) `xmlns:xmlns` binds `xmlns` to the
    // manifest's namespace; the row names no file. An element of another vocabulary called `file-entry`, and one of the manifest's that is
    // not `file-entry`, are no rows either.
    let text = format!(
        concat!(
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" "#,
            r#"xmlns:xmlns="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" xmlns:x="urn:example:x">"#,
            r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="{}"/>"#,
            r#"<manifest:file-entry xmlns:full-path="evil.xml" manifest:media-type="text/xml"/>"#,
            r#"<x:file-entry manifest:full-path="ghost.xml" manifest:media-type="text/xml"/>"#,
            r#"<manifest:other manifest:full-path="ghost2.xml" manifest:media-type="text/xml"/>"#,
            r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
        ),
        DRAWING
    );
    let package = read(&with_manifest_text(&text));
    let paths: Vec<&str> = package.manifest().entries.iter().map(|e| e.full_path.as_str()).collect();
    assert_eq!(paths, ["/", "content.xml"]);
    assert_eq!(package.warnings(), &[Warning::ManifestEntryIncomplete { count: 1 }]);
}

#[test]
fn a_presentation_event_listener_is_no_macro() {
    // `presentation:event-listener` binds an event to an action of the presentation vocabulary (go to a page, play a sound), not to a
    // script: no macro. A `script:event-listener` beside it is one.
    let presentation = r#"<office:event-listeners xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"><presentation:event-listener script:event-name="dom:click" presentation:action="next-page"/></office:event-listeners>"#;
    let with = |listeners: &str| {
        let text = content(&page("p1", &crate::build::shape("s1", "1cm"))).replace("<text:p/>", &format!("{listeners}<text:p/>"));
        read(&package(DRAWING, crate::build::replace(files(""), "content.xml", text.into_bytes())))
    };
    assert!(!with(presentation).has_macros());
    let script = r#"<script:other/>"#;
    assert!(!with(script).has_macros(), "another element of the script vocabulary binds nothing");
}

#[test]
fn a_script_container_holding_anything_is_a_macro_and_an_empty_one_is_not() {
    // The review's case: code under an `office:scripts` in a vocabulary of its own, or as bare text, is code; white space,
    // an empty container, and a container holding only empty containers are none.
    let with = |scripts: &str| {
        let text = content("").replace(" <office:body>", &format!("{scripts} <office:body>"));
        read(&package(DRAWING, crate::build::replace(files(""), "content.xml", text.into_bytes())))
    };
    for code in [
        r#"<office:scripts><x:code xmlns:x="urn:x">Sub Main</x:code></office:scripts>"#,
        "<office:scripts>Sub Main</office:scripts>",
        "<office:scripts><![CDATA[Sub Main]]></office:scripts>",
        r#"<office:event-listeners><x:on xmlns:x="urn:x"/></office:event-listeners>"#,
        "<office:event-listeners>call()</office:event-listeners>",
        r#"<office:scripts><office:event-listeners><x:on xmlns:x="urn:x"/></office:event-listeners></office:scripts>"#,
        // A listener of another vocabulary that only shares the presentation listener's local name, and a presentation element that is no
        // listener: neither is the presentation listener set aside.
        r#"<office:event-listeners><x:event-listener xmlns:x="urn:x"/></office:event-listeners>"#,
        r#"<office:event-listeners xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"><presentation:sound/></office:event-listeners>"#,
        // A script, or a binding to one, outside any container.
        r#"<office:script script:language="ooo:Basic"/>"#,
        r#"<script:event-listener script:event-name="dom:click"/>"#,
    ] {
        assert!(with(code).has_macros(), "{code}");
    }
    for none in [
        "<office:scripts>\n  </office:scripts>",
        "<office:scripts><office:event-listeners/></office:scripts>",
        "<office:scripts><!-- a note --></office:scripts>",
        r#"<x:wrapper xmlns:x="urn:x"><x:code>Sub Main</x:code></x:wrapper>"#,
    ] {
        assert!(!with(none).has_macros(), "{none}");
    }
    // A macro in `styles.xml` counts as one in `content.xml` does.
    let styles = crate::build::styles().replace("<office:styles/>", "<office:styles/><office:scripts>Sub Main</office:scripts>");
    assert!(read(&package(DRAWING, crate::build::replace(files(""), "styles.xml", styles.into_bytes()))).has_macros());
}

#[test]
fn both_trees_are_parsed_once_and_kept_whatever_was_found_first() {
    // A `Basic/` file is a macro already, and `content.xml` holds one too; both trees are still parsed, each once, and
    // kept for the drawing's reader. The XML events of the read are exactly those of the manifest and the two parts.
    let body = page("p1", &shapes(2));
    let text = content(&body).replace(" <office:body>", "<office:scripts>Sub Main</office:scripts> <office:body>");
    let mut entries = crate::build::replace(files(&body), "content.xml", text.clone().into_bytes());
    entries.push(("Basic/Standard/Module1.xml".to_owned(), b"<module/>".to_vec()));
    let bytes = package(DRAWING, entries);
    let events = |part: &[u8]| {
        let mut work = Work::default();
        diacraft_container::xml::parse_counted(part, &diacraft_container::XmlLimits::default(), &mut work).unwrap();
        work.xml_events
    };
    let mut work = Work::default();
    let package = Package::read_counted(&bytes, &Limits::default(), &mut work).unwrap();
    assert!(package.has_macros());
    let want = events(package.part(MANIFEST_PATH).unwrap()) + events(text.as_bytes()) + events(crate::build::styles().as_bytes());
    assert_eq!(work.xml_events, want, "the manifest and each of the two parts parsed once");
    let tree = package.tree("content.xml").unwrap();
    assert_eq!(tree, &diacraft_container::xml::parse(text.as_bytes()).unwrap());
    assert!(package.tree("styles.xml").is_some_and(|t| t.name == "office:document-styles"));
    assert!(package.tree("meta.xml").is_none() && package.tree_error("content.xml").is_none());
    // A part that does not parse: no tree, its error, and the warning.
    let broken = read(&package_of("<office:document-content"));
    assert!(broken.tree("content.xml").is_none());
    assert_eq!(broken.tree_error("content.xml").map(|e| e.class()), Some("xml_malformed"));
    assert!(broken.tree("styles.xml").is_some(), "the other is kept all the same");
}

/// A drawing whose `content.xml` is `text`.
fn package_of(text: &str) -> Vec<u8> {
    package(DRAWING, crate::build::replace(files(""), "content.xml", text.as_bytes().to_vec()))
}

#[test]
fn duplicate_rows_past_the_named_ones_are_counted() {
    let rows: Vec<(&str, &str)> = std::iter::repeat_n(("content.xml", "text/xml"), 6).collect();
    let entries = vec![("content.xml".to_owned(), content("").into_bytes()), (MANIFEST_PATH.to_owned(), manifest(DRAWING, &rows).into_bytes())];
    let limits = Limits { max_report_entries: 2, ..Limits::default() };
    let package = Package::read(&write(Some(DRAWING), &entries), &limits).unwrap();
    let duplicates: Vec<&Warning> = package.warnings().iter().filter(|w| w.class() == "manifest_duplicate_entry").collect();
    assert_eq!(duplicates.len(), 3, "{duplicates:?}");
    assert_eq!(duplicates[2], &Warning::MoreManifestDuplicateEntries { count: 3 });
}

/// The OpenOffice.org 2 manifest's DOCTYPE, an external id with no internal subset (the census).
const OOO_DOCTYPE: &str = "<!DOCTYPE manifest:manifest PUBLIC \"-//OpenOffice.org//DTD Manifest 1.0//EN\" \"Manifest.dtd\">";

/// A drawing whose manifest has `doctype` after its XML declaration.
fn with_manifest_doctype(doctype: &str) -> Vec<u8> {
    let entries = crate::build::with_manifest(DRAWING, files(&page("p1", &shapes(1))));
    let entries: Vec<(String, Vec<u8>)> = entries
        .into_iter()
        .map(|(name, data)| {
            if name != MANIFEST_PATH {
                return (name, data);
            }
            let text = String::from_utf8(data).unwrap().replacen("?>\n", &format!("?>\n{doctype}\n"), 1);
            (name, text.into_bytes())
        })
        .collect();
    write(Some(DRAWING), &entries)
}

#[test]
fn a_manifest_with_an_external_doctype_alone_is_read() {
    // The OpenOffice.org 2 form, and a SYSTEM-only one, are read and ignored in the manifest.
    for doctype in [OOO_DOCTYPE, "<!DOCTYPE manifest:manifest SYSTEM \"Manifest.dtd\">"] {
        let package = read(&with_manifest_doctype(doctype));
        assert_eq!(package.variant(), Some(Variant::Drawing), "{doctype}");
        assert_eq!(package.manifest().entries.len(), 5, "{doctype}");
        assert!(package.warnings().is_empty(), "{doctype}: {:?}", package.warnings());
        assert_eq!(sniff(&with_manifest_doctype(doctype)), Some(Variant::Drawing));
    }
    // An internal subset is still refused, and so is an entity reference after an accepted DOCTYPE.
    assert_eq!(class(&with_manifest_doctype("<!DOCTYPE manifest:manifest [ <!ENTITY x \"y\"> ]>")), "xml_entity_refused");
    // A DOCTYPE not written `<!DOCTYPE` and a space is not XML, so the manifest is not well-formed.
    for misspelled in [
        "<!doctype manifest:manifest SYSTEM \"Manifest.dtd\">",
        "<!DocType manifest:manifest SYSTEM \"Manifest.dtd\">",
        "<!DOCTYPEmanifest:manifest SYSTEM \"Manifest.dtd\">",
    ] {
        assert_eq!(class(&with_manifest_doctype(misspelled)), "manifest_malformed", "{misspelled}");
    }
    let bytes = with_manifest_doctype(OOO_DOCTYPE);
    let referenced = {
        let entries = crate::build::with_manifest(DRAWING, files(""));
        let entries: Vec<(String, Vec<u8>)> = entries
            .into_iter()
            .map(|(name, data)| {
                if name != MANIFEST_PATH {
                    return (name, data);
                }
                let text = String::from_utf8(data).unwrap().replacen("?>\n", &format!("?>\n{OOO_DOCTYPE}\n"), 1);
                (name, text.replacen("text/xml", "text/&x;", 1).into_bytes())
            })
            .collect();
        write(Some(DRAWING), &entries)
    };
    assert_eq!(class(&referenced), "xml_entity_refused");
    // The same DOCTYPE in `content.xml` is refused: the part is kept as bytes, its tree is its error.
    let text = content("").replacen("?>\n", &format!("?>\n{}\n", OOO_DOCTYPE.replace("manifest:manifest", "office:document-content")), 1);
    let package = read(&package(DRAWING, crate::build::replace(files(""), "content.xml", text.into_bytes())));
    assert_eq!(package.tree_error("content.xml").map(|e| e.class()), Some("xml_entity_refused"));
    assert_eq!(classes(&package), ["part_not_scanned"]);
    // A comparison reads the manifest as the reader does: two packages whose manifests differ, each with the DOCTYPE, compare by element.
    let other = {
        let text = String::from_utf8(read(&bytes).part(MANIFEST_PATH).unwrap().to_vec()).unwrap().replacen("text/xml", "application/xml", 1);
        let mut entries: Vec<(String, Vec<u8>)> = files(&page("p1", &shapes(1)));
        entries.push((MANIFEST_PATH.to_owned(), text.into_bytes()));
        write(Some(DRAWING), &entries)
    };
    let d = diacraft_odg::diff_packages(&bytes, &other, &Limits::default()).unwrap();
    assert!(d.unparsed.is_empty(), "{d:?}");
    assert_eq!(d.parts.len(), 1);
    assert_eq!(d.parts[0].part, MANIFEST_PATH);
}

#[test]
fn a_tree_is_taken_once_and_a_deferred_read_parses_each_tree_once_on_demand() {
    // `Package::take_tree`: the tree `Package::read` kept, once; the part's bytes stay.
    let bytes = drawing(&page("p1", &shapes(1)));
    let mut kept = read(&bytes);
    assert!(kept.take_tree("content.xml").is_some_and(|t| t.is_ok()));
    assert!(kept.take_tree("content.xml").is_none());
    assert!(kept.tree("content.xml").is_none() && kept.tree_error("content.xml").is_none());
    assert!(kept.part("content.xml").is_some());
    assert!(kept.take_tree("meta.xml").is_none(), "not a tree part");
    // `Package::read_deferred`: no tree parsed until one is asked for, each once, and the macro scan on the trees asked for.
    let scripts = content("").replace(
        " <office:body>",
        r#" <office:scripts><office:script script:language="ooo:Basic"><x/></office:script></office:scripts><office:body>"#,
    );
    let bytes = package(DRAWING, crate::build::replace(files(""), "content.xml", scripts.into_bytes()));
    let limits = Limits::default();
    let mut work = Work::default();
    let mut deferred = Package::read_deferred(&bytes, &limits, &mut work).unwrap();
    let mut full = Work::default();
    Package::read_counted(&bytes, &limits, &mut full).unwrap();
    assert!(work.xml_events > 0 && work.xml_events < full.xml_events / 2, "only the manifest was parsed: {} of {}", work.xml_events, full.xml_events);
    assert!(deferred.tree("content.xml").is_none());
    assert!(!deferred.has_macros(), "not looked through yet");
    let mut more = Work::default();
    assert!(deferred.parse_tree("content.xml", &limits, &mut more).is_some_and(|t| t.is_ok()));
    assert!(more.xml_events > 0);
    assert!(deferred.has_macros());
    assert!(deferred.parse_tree("content.xml", &limits, &mut more).is_none(), "parsed once");
    assert!(deferred.parse_tree("meta.xml", &limits, &mut more).is_none(), "not a tree part");
    assert!(deferred.parse_tree("styles.xml", &limits, &mut more).is_some());
    // A part that does not parse gives its class, and the XML budget is summed over the parts parsed.
    let broken = package(DRAWING, crate::build::replace(files(""), "content.xml", b"<office:document-content>".to_vec()));
    let mut p = Package::read_deferred(&broken, &limits, &mut Work::default()).unwrap();
    assert_eq!(p.parse_tree("content.xml", &limits, &mut Work::default()).and_then(Result::err).map(|e| e.class()), Some("xml_malformed"));
    let small = Limits { max_xml_bytes: crate::build::styles().len() as u64 + 10, ..Limits::default() };
    let mut p = Package::read_deferred(&bytes, &small, &mut Work::default()).unwrap();
    let first = p.parse_tree("styles.xml", &small, &mut Work::default()).unwrap();
    assert!(first.is_ok());
    let second = p.parse_tree("content.xml", &small, &mut Work::default()).unwrap();
    assert_eq!(second.err().map(|e| e.class()), Some("too_much_xml"));
    // A tree `Package::read` kept is not parsed again, the other one is; once parsed, the part's bytes are let go.
    let no_styles: Vec<(String, Vec<u8>)> = files("").into_iter().filter(|(n, _)| n != "styles.xml").collect();
    let mut p = read(&package(DRAWING, no_styles));
    assert!(p.parse_tree("content.xml", &limits, &mut Work::default()).is_none(), "kept by the read");
    assert!(p.part_shared("content.xml").is_some_and(|b| !b.is_empty()));
    let mut p2 = Package::read_deferred(&bytes, &limits, &mut Work::default()).unwrap();
    assert!(p2.part("content.xml").is_some_and(|b| !b.is_empty()));
    assert!(p2.parse_tree("content.xml", &limits, &mut Work::default()).is_some());
    assert_eq!(p2.part("content.xml").map(<[u8]>::len), Some(0));
}
