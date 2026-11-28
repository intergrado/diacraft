//! The properties the targets hold, in one place. A property that fails is a panic, which libFuzzer reports as a crash with the input that made it.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, Format, Part, PartRole, Passthrough, RawNode};
use diacraft_opc::Package;
use diacraft_vsdx::{Limits, ReadReport, VsdxError, read_partial};

/// Whether a message is words: not empty, and no control character (a name or a value from a file never makes one).
pub fn in_words(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

/// A refusal of the reader is in words and has a class word.
pub fn refusal_in_words(e: &VsdxError) {
    assert!(in_words(&e.to_string()), "a refusal that is not in words: {e:?}");
    assert!(
        !e.class().is_empty() && e.class().bytes().all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit()),
        "a class that is not a word: {:?}",
        e.class()
    );
}

/// What a report says is in words: no warning holds a control character.
pub fn warnings_in_words(report: &ReadReport) {
    for warning in &report.warnings {
        assert!(in_words(warning), "a warning that is not in words: {warning:?}");
    }
}

/// The bytes of the parts a package holds under `/diacraft/` (a part of DiaCraft's own, as the carrier reads it: any case, not a folder, not a relationships part).
pub fn bytes_of_diacraft_parts(package: &Package) -> HashSet<Vec<u8>> {
    package
        .parts()
        .filter(|name| {
            let folded = name.to_ascii_lowercase();
            folded.starts_with("/diacraft/")
                && folded.len() > "/diacraft/".len()
                && !folded.ends_with('/')
                && !diacraft_opc::name::is_rels_part(&folded)
        })
        .filter_map(|name| package.part(name).map(<[u8]>::to_vec))
        .collect()
}

/// Whether the name of a part of the other format is a macro project's, by the rule of `.odg` packages, any case.
pub fn macro_name(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.starts_with("basic/") || folded.starts_with("scripts/") || folded == "meta-inf/macrosignatures.xml"
}

/// What a read that kept parts of the other format holds of them, whatever the file said (the carrier's promises, `carry.rs`):
///
/// * **no macro part reaches a type that allows no macros** (by the role or by the name, any case), and a file whose type is none of the six allows none;
/// * **nothing outside `/diacraft/` is taken as a blob**: the bytes of every part of the other format, and of every definition of the document, are the bytes of a part of the package under
///   `/diacraft/` (an empty part is the folder of a package, which has none);
/// * **a name is one an entry of an `.odg` package may have** (no `..`, no leading slash, no backslash, no control character, no empty segment), one entry for one name by ASCII case, and
///   none of the entries a package makes itself.
pub fn carried_parts_are_sound(input: &[u8], doc: &Document, report: &ReadReport) {
    let Ok(package) = Package::read(input, &Limits::default().opc) else { return };
    let blobs = bytes_of_diacraft_parts(&package);
    let macros = report.variant.is_some_and(|v| v.macro_enabled);
    let mut names: HashSet<String> = HashSet::new();
    for part in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        let is_macro = matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) || macro_name(&part.name);
        assert!(
            macros || !is_macro,
            "a macro part of the other format came into a type that allows none: role {:?}, {} bytes",
            part.role,
            part.bytes.len()
        );
        assert!(
            part.bytes.is_empty() || blobs.contains(&*part.bytes),
            "a part of the other format holds bytes that no part under /diacraft/ holds ({} bytes)",
            part.bytes.len()
        );
        let name: &str = &part.name;
        let bare = name.strip_suffix('/').unwrap_or(name);
        assert!(
            !bare.is_empty()
                && !name.starts_with('/')
                && !name.contains('\\')
                && !name.chars().any(char::is_control)
                && bare.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
            "a part of the other format has a name no package could hold: {name:?}"
        );
        if part.format == Format::Odf {
            let folded = bare.to_ascii_lowercase();
            assert!(
                !["mimetype", "content.xml", "styles.xml", "meta.xml", "settings.xml", "meta-inf/manifest.xml"].contains(&folded.as_str())
                    && !folded.starts_with("diacraft/")
                    && folded != "diacraft",
                "a part of the other format has the name of an entry a package makes itself: {name:?}"
            );
        }
        assert!(names.insert(name.to_ascii_lowercase()), "two parts of the other format have one name by case: {name:?}");
    }
    if let Some(data) = doc.ext.get() {
        for definition in &data.odf.definitions {
            if let Some(bytes) = &definition.bytes {
                assert!(
                    bytes.is_empty() || blobs.contains(&**bytes),
                    "a definition holds bytes that no part under /diacraft/ holds ({} bytes)",
                    bytes.len()
                );
            }
        }
    }
}

/// What a document that read holds of its package (the plan's, as `tests/it/fuzz.rs` holds it): **the bytes of each held part are the package's, exactly**, no two
/// held parts are one part (ASCII case folded), the report says how many and how many bytes, a held part is never the document part or a part the model names as a page's or a master's,
/// the rows held come from parts of the package that are not held, and nothing is skipped.
pub fn held_is_the_packages(bytes: &[u8], doc: &Document, report: &ReadReport) {
    let Ok(package) = Package::read(bytes, &Limits::default().opc) else { panic!("a package that read opens again") };
    let mut names = HashSet::new();
    let mut sum = 0u64;
    // The parts of the other format that DiaCraft's part carries are the model's, and are no part of this package.
    for part in doc.parts.iter().filter(|p| p.format == Format::Vsdx) {
        let original = package.part(&part.name).unwrap_or_else(|| panic!("a held part, {:?}, that the package does not have", part.name));
        assert_eq!(&*part.bytes, original, "the held bytes are the package's");
        assert!(names.insert(part.name.to_ascii_lowercase()), "two held parts are one part: {:?}", part.name);
        assert!(!part.name.eq_ignore_ascii_case(&report.document_part), "the document part is held");
        assert!(
            !diacraft_opc::name::is_rels_part(&part.name) || part.rels.is_empty(),
            "a relationships part held as bytes has no relationships of its own"
        );
        sum += part.bytes.len() as u64;
    }
    assert_eq!((report.held_parts, report.held_bytes), (doc.parts.iter().filter(|p| p.format == Format::Vsdx).count(), sum));
    assert!(report.skipped_counts.is_empty(), "{:?}", report.skipped_counts);
    for named in doc.pages.iter().filter_map(|p| p.part_name.as_ref()).chain(doc.masters.iter().filter_map(|m| m.part_name.as_ref())) {
        assert!(package.part(named).is_some(), "a page or master names a part the package has: {named:?}");
        assert!(!names.contains(&named.to_ascii_lowercase()), "a part that is a page's or a master's is not held: {named:?}");
    }
    if let Some(held) = &doc.vsdx {
        for row in &held.rels {
            assert!(&*row.source == "/" || package.part(&row.source).is_some(), "a held row comes from a part of the package: {:?}", row.source);
            assert!(!names.contains(&row.source.to_ascii_lowercase()), "the rows of a held part are its own: {:?}", row.source);
        }
    }
}

/// Reads with a pool of any size beside the file and the package: **a read, or a refusal for memory in words, and every tree the read made was held in the pool** (the parse is bounded
/// before the tree is made, so a tree that was made, and counted, was charged).
pub fn read_in_a_pool(bytes: &[u8], pool: u64) {
    let (_, first) = read_partial(bytes, &Limits::default());
    let room = first.stats.file_bytes + first.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    let (result, report) = read_partial(bytes, &Limits { max_memory: room + pool, ..Limits::default() });
    assert!(
        report.stats.max_tree_bytes <= report.stats.memory_peak,
        "a tree of {} bytes was made and the pool held {} at the most (a pool of {pool}): {:?}",
        report.stats.max_tree_bytes,
        report.stats.memory_peak,
        report.stats
    );
    assert!(report.stats.memory_peak <= room + pool, "the pool was passed: {:?}", report.stats);
    match result {
        Ok(doc) => {
            doc.check_limits().expect("a document that read in a small pool passes the model's own caps");
            warnings_in_words(&report);
        }
        Err(e) => refusal_in_words(&e),
    }
}

/// **The judge's work stays under its budget** (`carry/read.rs`): an item is judged at most once, a record of a part is looked at once, and a read that did not stop for the work it was allowed
/// (no `diacraft_items_not_read` word) took twice the steps of stamping and the nodes copied from the budget, and so spent at most `steps` of them. The counts are those of `diacraft_vsdx::work`,
/// of the calling thread, from the last `work::reset()`.
pub fn judge_stays_in_budget(steps: u64, report: &ReadReport) {
    use diacraft_vsdx::work;
    let (stamped, copied) = (work::carry_items_stamped(), work::carry_nodes_copied());
    assert!(work::carry_items_judged() <= report.carry.items, "{} items judged of {}", work::carry_items_judged(), report.carry.items);
    assert!(report.carry.kept <= report.carry.items, "{:?}", report.carry);
    assert_eq!(work::carry_parts_read(), report.carry.parts_seen, "a record of a part is looked at once");
    if report.warning_count("diacraft_items_not_read") == 0 {
        assert!(
            stamped.saturating_mul(2).saturating_add(copied) <= steps,
            "the judge took {stamped} steps of stamping and {copied} of copying from a budget of {steps} and did not say it stopped"
        );
    }
}

/// **Nothing that lies outside `/diacraft/` is taken as a blob**, by its bytes: the target plants parts outside it whose bytes begin with `marker`, and none of the other format's parts and
/// definitions begins with it.
pub fn nothing_of_outside_is_carried(marker: &[u8], doc: &Document) {
    for part in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        assert!(!part.bytes.starts_with(marker), "a part outside /diacraft/ was taken as a blob: {:?}", part.name);
    }
    if let Some(data) = doc.ext.get() {
        for definition in &data.odf.definitions {
            assert!(definition.bytes.as_ref().is_none_or(|b| !b.starts_with(marker)), "a part outside /diacraft/ was taken as a definition's bytes");
        }
    }
}

/// What a package holds, by part: the name (ASCII case folded, with its slash) and the bytes of each part, the content types stream not among them.
fn parts_of(bytes: &[u8]) -> Option<std::collections::BTreeMap<String, Vec<u8>>> {
    let package = Package::read(bytes, &Limits::default().opc).ok()?;
    Some(package.parts().filter_map(|name| Some((name.to_ascii_lowercase(), package.part(name)?.to_vec()))).collect())
}

/// **A save with no edit leaves the parts of the file as they were**: the same names and the same bytes in each, but for the extended properties, where the save names itself as the writer
/// (`Application`, whether or not an edit was made).
pub fn same_parts(before: &[u8], after: &[u8]) {
    same_parts_but(before, after, &[]);
}

/// [`same_parts`], **but for the parts in `rewritten`**: the parts a save of a file of DiaCraft's rewrote to give the `User` and `Field` text its unit and the time of the change.
/// The names are the same, and the bytes of every other part.
pub fn same_parts_but(before: &[u8], after: &[u8], rewritten: &[String]) {
    let (Some(mut before), Some(mut after)) = (parts_of(before), parts_of(after)) else { panic!("a package that read does not open again") };
    before.remove("/docprops/app.xml");
    after.remove("/docprops/app.xml");
    for name in rewritten {
        let name = name.to_ascii_lowercase();
        before.remove(&name);
        after.remove(&name);
    }
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "an unedited save changed the parts of the file");
    for (name, bytes) in &before {
        assert!(after.get(name) == Some(bytes), "an unedited save changed the bytes of {name:?}");
    }
}

/// The content type a save gives a held part that has none: the relationships type for a relationships part, else `application/octet-stream` (the writer's rule: every
/// part of a package has a type, and a save says in a warning that it gave one).
fn type_a_save_gives(name: &str) -> &'static str {
    if diacraft_opc::name::is_rels_part(name) { "application/vnd.openxmlformats-package.relationships+xml" } else { "application/octet-stream" }
}

/// The document as a save leaves it, which the read of a saved file is: **a save names itself as the writer** in the extended properties (`Application` is `DiaCraft` after any save, an edit or
/// not), and **gives each held part that had no content type one** (and a warning says so). The read of a saved file differs from the read of the file by those and by no other thing, so
/// both are taken out of what the two documents are compared by.
pub fn after_a_save(doc: &Document) -> Document {
    let mut out = doc.clone();
    if let Some(held) = out.vsdx.as_mut() {
        for (_, kept) in Arc::make_mut(held).props_rest.iter_mut() {
            let mut rest = Passthrough::new();
            for placed in kept.kept.iter() {
                if !matches!(&placed.node, RawNode::Element(e) if &*e.name == "Application") {
                    rest.push(placed.after, placed.node.clone());
                }
            }
            kept.kept = rest;
        }
    }
    for at in 0..out.parts.len() {
        let Some(slot) = out.parts.get_mut(at) else { continue };
        if slot.format == Format::Vsdx && slot.media_type.is_empty() {
            let typed = Part { media_type: Arc::from(type_a_save_gives(&slot.name)), ..(**slot).clone() };
            *slot = Arc::new(typed);
        }
    }
    out
}

/// [`after_a_save`] for a save that was an edit of the file (the unit it gave to the `User` and `Field` text): such a save also **writes the time of the change**
/// (`modified` of `docProps/core.xml`, with the attributes of the element it made), so the read of it differs from the read of the file by that time too, which is taken out of both.
pub fn after_an_edited_save(doc: &Document) -> Document {
    let mut out = after_a_save(doc);
    Arc::make_mut(&mut out.props).modified = None;
    if let Some(held) = out.vsdx.as_mut() {
        Arc::make_mut(held).props_rest.retain(|(name, _)| &**name != "coreProperties/modified");
    }
    out
}

/// What the model says of one part of the other format: its folded name, the role and format, the bytes, the media type, and its name as written.
pub type OtherPart = (String, String, Vec<u8>, String, String);

/// The parts of the other format a model holds, in order of name.
pub fn other_parts(doc: &Document) -> Vec<OtherPart> {
    let mut out: Vec<OtherPart> = doc
        .parts
        .iter()
        .filter(|p| p.format != Format::Vsdx)
        .map(|p| {
            (p.name.to_ascii_lowercase(), format!("{:?}/{:?}", p.format, p.role), p.bytes.to_vec(), p.media_type.to_string(), p.name.to_string())
        })
        .collect();
    out.sort();
    out
}

/// **A package the writer made is one the judge refuses nothing of**: none of the carrier's words of refusal is in the read of it (a macro project in a type that allows none, a name no package
/// could hold, a part that two records name, a part the package does not have, a record that is not what its part is).
pub fn nothing_refused_in(report: &ReadReport) {
    for class in [
        "diacraft_macro_refused",
        "diacraft_part_name_refused",
        "diacraft_part_twice",
        "diacraft_part_missing",
        "diacraft_part_role",
        "diacraft_item_damaged",
        "diacraft_item_ambiguous",
        "diacraft_picture_lost",
    ] {
        assert_eq!(report.warning_count(class), 0, "the read of what the writer made refuses ({class}): {:?}", report.warnings);
    }
}

/// Whether the part is one that holds the cells of the drawing: a page, a master or the document's own part (by name, in any case). The parts whose cells a save into a file of DiaCraft's may give the unit
/// `STR`.
pub fn is_drawing_part(name: &str) -> bool {
    let (name, dir) = (name.to_ascii_lowercase(), crate::package::DIR.to_ascii_lowercase());
    name.starts_with(&format!("{dir}/pages/")) || name.starts_with(&format!("{dir}/masters/")) || name == format!("{dir}/document.xml")
}

/// **A save that writes the solution part again takes out nothing that is not DiaCraft's**: every part of the original except those under the folder of DiaCraft's own, the relationships
/// parts, the solution parts and the parts of the properties (a save with edits sets `modified` and takes the thumbnail out) is in the package after, with its bytes.
///
/// **A file of DiaCraft's** (`ours`: its extended properties name DiaCraft) **is given the unit of the text of its `User` and `Field` rows by any save**, so the parts of the
/// drawing ([`is_drawing_part`]) are the writer's to write there.
pub fn kept_what_is_not_diacraft_s(before: &[u8], after: &[u8], ours: bool) {
    let (Some(before), Some(after)) = (parts_of(before), parts_of(after)) else { panic!("a package that read does not open again") };
    for (name, bytes) in &before {
        let mine = name.starts_with("/diacraft/")
            || name.starts_with("/docprops/")
            || diacraft_opc::name::is_rels_part(name)
            || name.contains("/solutions/")
            || (ours && is_drawing_part(name));
        if mine {
            continue;
        }
        assert!(after.get(name) == Some(bytes), "a save that wrote the solution part again changed or took out {name:?}");
    }
}

/// A part of the other format as a file carries it: its format, name, bytes and media type (the role is the name's, for an `.odg` entry, whatever the model said).
pub type Carried = (String, String, Vec<u8>, String);

/// The parts of the other format a document that was read holds, in order of name.
pub fn parts_read(doc: &Document) -> Vec<Carried> {
    let mut out: Vec<Carried> = doc
        .parts
        .iter()
        .filter(|p| p.format != Format::Vsdx)
        .map(|p| (format!("{:?}", p.format), p.name.to_string(), p.bytes.to_vec(), p.media_type.to_string()))
        .collect();
    out.sort();
    out
}

/// The parts of the other format a model holds that ride in a package that allows macros or not: **a macro project (by its role or by its name, any case) only into a type that allows one, and of
/// two parts whose names differ at most by case the first**.
pub fn parts_that_ride(doc: &Document, macros: bool) -> Vec<Carried> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Carried> = Vec::new();
    for p in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        let is_macro = matches!(p.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) || macro_name(&p.name);
        if is_macro && !macros {
            continue;
        }
        if seen.insert(p.name.to_ascii_lowercase()) {
            out.push((format!("{:?}", p.format), p.name.to_string(), p.bytes.to_vec(), p.media_type.to_string()));
        }
    }
    out.sort();
    out
}

/// The first eight bytes of a compound file.
const COMPOUND_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The embeddings a read document holds (the parts of the package that an `oleObject` relationship leads to), by their bytes.
pub fn embeddings_read(doc: &Document) -> Vec<Vec<u8>> {
    doc.parts.iter().filter(|p| p.format == Format::Vsdx && p.role == PartRole::Embedding).map(|p| p.bytes.to_vec()).collect()
}

/// **What an object frame carried back may do to the parts of the other format**: the parts that came back are parts that ride, and a part that rides and did not come back is
/// there as an embedding, byte for byte, and a compound file at its head. Nothing is lost, and nothing is made: every embedding is the bytes of a part the model held.
pub fn objects_lose_nothing(doc: &Document, back: &Document, macros: bool) {
    let rides = parts_that_ride(doc, macros);
    let read = parts_read(back);
    let embedded = embeddings_read(back);
    for part in &read {
        assert!(rides.contains(part), "a part came back that did not ride: {}", part.1);
    }
    for part in rides.iter().filter(|p| !read.contains(p)) {
        assert!(embedded.contains(&part.2), "a part is neither carried nor an embedding: {}", part.1);
        assert!(part.2.starts_with(&COMPOUND_SIGNATURE), "an embedding that is no compound file: {}", part.1);
    }
    for bytes in &embedded {
        assert!(bytes.starts_with(&COMPOUND_SIGNATURE), "an embedding that is no compound file");
        assert!(
            doc.parts.iter().any(|p| p.format != Format::Vsdx && p.bytes[..] == bytes[..]),
            "an embedding that is the bytes of no part the model held"
        );
    }
}
