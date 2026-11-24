//! DiaCraft's data in VSDX files: keyed, stamped and judged (the native format's design, 5.1, 5.2 and 5.4).
//!
//! A drawing that holds data of the OpenDocument format (what the `.odg` reader keeps in the `odf` side of the slots, the parts and custom properties it tags as its own, and what a
//! newer `.diacraft` wrote that this build does not know) and is saved as a VSDX file **carries that data inside the file**, in DiaCraft's solution part, so that it comes back when
//! DiaCraft reads the file again. A VSDX program that does not know the part ignores it, and the file draws the same without it: **no cell is copied**, the cells are written natively
//! and the data beside them rides.
//!
//! **Items.** The part's `Data` element holds, besides DiaCraft's own settings (`Document::diacraft`), one `item` for the document's data and one for each page, master, style and shape
//! that has some. An item has the fragment of the model that holds its data (a shell: `shell.rs`) as its one element of the codec's vocabulary (`diacraft_sheetxml::model`), its
//! **key**, and its **stamp**:
//!
//! | `kind` | key | the fragment |
//! |---|---|---|
//! | `document` | none | the document part of the codec, with the slots, the faces and the sheet's places that hold something |
//! | `parts` | none | the document part of the codec, with the parts and the custom properties of the other formats: **no stamp**, since they sit beside nothing the file says |
//! | `page`, `master` | `id` | the page or master's part, with its slot, the slots of its sheet and its connects |
//! | `style` | `name`; `id` when it has none or another style has its name | the style |
//! | `shape` | `of` (the page's `id`) and `id` | the shape, without its children |
//! | `master-shape` | `of` (the master's `id`) and `id` | the same, in a master |
//!
//! A shape item also holds an `at` element for each text part that rides, with its place (`n`, the markers before it, and `k`, the bytes of characters since the last one).
//! The stamp is the item's `fp` attribute (`stamp.rs`): what the file says of the object **as VSDX says it**, at the time the item was written.
//!
//! **Bytes** (a macro project, an object, a fill image: anything the document's parts and definitions hold as bytes) are not in the part. Each distinct byte string is a package part of
//! DiaCraft's own, `/diacraft/blob<n>.<ext>`, with the media type for its content type, and a relationship of type [`BLOB_REL_TYPE`] from the solution part to it. The document item's
//! `blob` records (the codec's list: number, media type, size, stamp of the bytes) name the part by the **relationship's `Id`** in their `path`.
//!
//! **The judge** (`read.rs`; the design's 5.4, tier 3). A read keeps an item only while it is good:
//!
//! * **an item whose object is not in the file is dropped, silently**: another program took the page or shape away;
//! * **an item whose stamp no longer matches is dropped**: another program changed what the item sits beside, and its edit wins;
//! * an item that names a part the package does not have, and two blobs that name one part, are dropped **with a note**; so is an entry name that no `.odg` package could hold
//!   (`odf_entry_name_ok`: `..`, an absolute path, a name that reaches outside the package, one that differs from an earlier by case only);
//! * a part is **read only when its item is good, charged to the budget before it is copied, and never parsed, run or opened**: it is bytes (`a_macro_part_is_never_parsed`);
//! * an item that is damaged, or that the pool cannot afford, is left out **and said**: reading never fails the drawing for what the carrier holds.
//!
//! **Caps.** The solution part is read up to [`crate::MAX_SOLUTION_PART_BYTES`] (64 MiB). Each part of DiaCraft's is held to the package's cap for a part (256 MiB) and all of them to
//! the package's: 50,000 parts and 1 GiB. A save that would pass one is refused in words that name the part; nothing is cut.
//!
//! **A part of DiaCraft's is DiaCraft's** when the solution part holds nothing but the `Data` and its relationships are all of the type above: then the model rebuilds both, and the
//! parts they name are not held as parts of the package. Any other solution part (another program's content beside the `Data`) is patched and gets no items; the save says so.

mod read;
mod shell;
mod size;
mod stamp;
mod write;

use diacraft_opc::xml::El;

pub(crate) use self::read::{Item, known_items};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use self::write::too_large;
pub(crate) use self::write::{Built, build, data_element, data_version, same};

/// The relationship type from the solution part to a part of DiaCraft's. A URI, as every relationship type is: DiaCraft's namespace and `:blob` (`diacraft_doc::DIACRAFT_URN`).
pub const BLOB_REL_TYPE: &str = "urn:app.diacraft:extension:blob";

/// The type that builds made before the id moved to `diacraft.app` wrote (`diacraft_doc::DIACRAFT_URN_LEGACY` and `:blob`; APPID). **A reader reads it as [`BLOB_REL_TYPE`]
/// ([`is_blob_rel_type`]), and a writer never writes it.**
pub const BLOB_REL_TYPE_LEGACY: &str = "urn:io.github.one_bit.diacraft:extension:blob";

/// Whether a relationship type is the one that leads from the solution part to a part of DiaCraft's: [`BLOB_REL_TYPE`], or the one earlier builds wrote.
pub fn is_blob_rel_type(ty: &str) -> bool {
    ty == BLOB_REL_TYPE || ty == BLOB_REL_TYPE_LEGACY
}

/// The folder DiaCraft's parts are named in, with its slash.
pub(crate) const BLOB_DIR: &str = "/diacraft/";

/// The most parts of DiaCraft's own one file carries: **a relationships part is at most 4 MiB** (the package's cap for a part of its own kind), and a row of the solution part's, to a part named
/// `/diacraft/blob<n>.<ext>`, is about 130 bytes, so 30,000 rows are 3.9 MB. More is refused at the save, in words (`write.rs`), and a reader that finds more refuses the relationships part as any.
pub(crate) const MAX_CARRIED_PARTS: usize = 30_000;

/// Whether a part, by the name the package gives it (absolute, with its slash), may be one of DiaCraft's own: **under `/diacraft/`** (compared ASCII case-insensitively), not a folder and not a
/// relationships part. A relationship of the type of a part of DiaCraft's own that leads anywhere else (a page, a master, the document) is no part of DiaCraft's, whatever it says: the
/// reader never reads such a part as bytes the data names, and **a save never takes it out** (it is another part's, and is kept as the file has it).
pub(crate) fn is_blob_target(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.starts_with(BLOB_DIR) && folded.len() > BLOB_DIR.len() && !folded.ends_with('/') && !diacraft_opc::name::is_rels_part(&folded)
}

/// What a save wrote as real VSDX, and the carrier therefore leaves out (`write/objects.rs`): **the parts of the other format that went natively** as embeddings (their bytes are the
/// embedding's, and ride nowhere else), by the allocation of the part, and **the shapes whose object frame went natively**, by the allocation of the shape, whose item then holds no `draw:object-ole`
/// element: the model read from the file holds the embedding, and no markup that names an entry that is no longer there. A part that a frame which did not go natively names too is not in the set.
#[derive(Default)]
pub(crate) struct Natively {
    pub parts: std::collections::HashSet<usize>,
    pub shapes: std::collections::HashSet<usize>,
}

impl Natively {
    /// The names of the parts of `doc` that went natively, in the model's order: what a caller that saves into this file again is told, so that it gives the writer the drawing without
    /// them. The file holds each such object once, as an embedding, and the drawing keeps the part the embedding was written from.
    pub(crate) fn part_names(&self, doc: &diacraft_doc::Document) -> Vec<std::sync::Arc<str>> {
        if self.parts.is_empty() {
            return Vec::new();
        }
        // One look at each part of the model, as the carrier's own walk of them is counted.
        crate::work::carry_walked(doc.parts.len());
        doc.parts
            .iter()
            .filter(|part| self.parts.contains(&std::sync::Arc::as_ptr(part).addr()))
            .map(|part| std::sync::Arc::clone(&part.name))
            .collect()
    }
}

/// The local name of an item.
pub(crate) const ITEM: &str = "item";

/// The kinds of items this build writes and reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Document,
    /// The parts and custom properties of the other formats: bytes and fields that sit beside nothing the file says, so the item has no stamp (the design's 7.5: an item with no
    /// stamp is not judged).
    Parts,
    Page,
    Master,
    Style,
    Shape,
    MasterShape,
    /// Which comments answer which: a VSDX file's comments part holds no thread, so a reply is written as a comment of its own and this item names the links. Its stamp is
    /// what the comments part says, so a file whose comments another program changed has the links left out.
    Comments,
}

impl Kind {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Kind::Document => "document",
            Kind::Parts => "parts",
            Kind::Page => "page",
            Kind::Master => "master",
            Kind::Style => "style",
            Kind::Shape => "shape",
            Kind::MasterShape => "master-shape",
            Kind::Comments => "comments",
        }
    }

    pub(crate) fn of(word: &str) -> Option<Kind> {
        Some(match word {
            "document" => Kind::Document,
            "parts" => Kind::Parts,
            "page" => Kind::Page,
            "master" => Kind::Master,
            "style" => Kind::Style,
            "shape" => Kind::Shape,
            "master-shape" => Kind::MasterShape,
            "comments" => Kind::Comments,
            _ => return None,
        })
    }

    /// The local name of the element of the codec's vocabulary that the item's fragment is.
    pub(crate) fn fragment(self) -> &'static str {
        match self {
            Kind::Document | Kind::Parts => "document",
            Kind::Page => "page",
            Kind::Master => "master",
            Kind::Style => "style",
            Kind::Shape | Kind::MasterShape => "shape",
            Kind::Comments => "comments",
        }
    }
}

/// The most bytes an entry name of the other format may have: the container's.
const MAX_ENTRY_NAME_BYTES: usize = diacraft_container::limits::MAX_PART_NAME_BYTES;

/// The entries an `.odg` package makes itself and a model never holds as parts, which a carried part may not take the name of (compared ASCII case-insensitively), and the folder of
/// DiaCraft's own entries there.
const ODF_RESERVED: [&str; 4] = ["mimetype", "content.xml", "styles.xml", "meta-inf/manifest.xml"];
const ODF_OWN_FOLDER: &str = "diacraft/";

/// The entries an `.odg` package makes itself from the model's fields **or takes whole from a part of the model of that name** (the `.odg` reader holds a `meta.xml` that is not
/// of the shape the fields hold, and a `settings.xml` past the cap of a small part, as parts of bytes, and the `.odg` model writer writes such a part in the place of the one it would make).
/// A carried part may have **exactly** such a name (a drawing read from an `.odg` holding one could not be saved as a VSDX file); another spelling of it is refused as a
/// reserved name is, since it would stand beside the part the writer makes and take its place where names are compared without case.
const ODF_STAND_INS: [&str; 2] = ["meta.xml", "settings.xml"];

/// Whether `name` is an entry name an OpenDocument package may hold, and a part of the other format may carry: **the rules of the container that writes one** (a name with `..` as a
/// segment, a leading slash, a backslash or a drive letter reaches outside the package or is no name), with a segment never empty or a control character, and not the name of an
/// entry the package makes itself or of the folder of DiaCraft's own, but for the two parts the package takes from the model as they are ([`ODF_STAND_INS`], spelt exactly). A folder's
/// name ends in a slash. Used before a name is ever written into an `.odg`, and before a part is put in the model.
pub(crate) fn odf_entry_name_ok(name: &str) -> bool {
    let bare = name.strip_suffix('/').unwrap_or(name);
    let bytes = name.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    if bare.is_empty()
        || name.len() > MAX_ENTRY_NAME_BYTES
        || name.starts_with('/')
        || name.contains('\\')
        || drive
        || name.chars().any(|c| c.is_control())
        || bare.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return false;
    }
    if ODF_STAND_INS.contains(&name) {
        return true;
    }
    let folded = bare.to_ascii_lowercase();
    !(ODF_RESERVED.contains(&folded.as_str())
        || ODF_STAND_INS.contains(&folded.as_str())
        || folded.starts_with(ODF_OWN_FOLDER)
        || folded == "diacraft")
}

/// What an entry of an `.odg` package is for, **by its name**: the rule of the `.odg` reader, which fills the role of the parts it keeps. A part that rides in a VSDX file
/// is given the role its name says when it is read, whatever the item that carries it declares: **a role in a file is a claim, and a name is what the entry is**.
pub(crate) fn odf_role_of(name: &str) -> diacraft_doc::PartRole {
    use diacraft_doc::PartRole;
    let object = name.strip_prefix("Object ").is_some_and(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest.get(digits..).is_some_and(|tail| tail.starts_with('/'))
    });
    if name.starts_with("Thumbnails/") && !name.ends_with('/') {
        PartRole::Thumbnail
    } else if odf_macro_name(name) && name != "META-INF/macrosignatures.xml" {
        PartRole::MacroProject
    } else if name == "META-INF/macrosignatures.xml" {
        PartRole::MacroSignature
    } else if name == "META-INF/documentsignatures.xml" {
        PartRole::Signature
    } else if name.starts_with("ObjectReplacements/") {
        PartRole::ObjectReplacement
    } else if object {
        PartRole::ObjectFile
    } else if name.starts_with("Fonts/") {
        PartRole::Font
    } else {
        PartRole::Other
    }
}

/// Whether an entry name is a macro project's of an `.odg` package: `Basic/` and `Scripts/` (and what is in them), and the macro signatures. **Compared ASCII case-insensitively**, so that a
/// name that differs in case only is not a way round the rule: a program that unzips on a case-folding file system sees the same entry.
pub(crate) fn odf_macro_name(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.starts_with("basic/") || folded.starts_with("scripts/") || folded == "meta-inf/macrosignatures.xml"
}

/// Whether a part of the other format is a file of an embedded object: by its role (an embedding, an object's own file or the picture that stands for it), or by its name (an `Object N`
/// entry or folder, or an entry of `ObjectReplacements/`: the `.odg` reader gives the file of an OLE object no role, `Other`).
pub(crate) fn is_object_part(part: &diacraft_doc::Part) -> bool {
    use diacraft_doc::PartRole;
    let named = part.name.starts_with("ObjectReplacements/")
        || part.name.strip_prefix("Object ").is_some_and(|rest| {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            digits > 0 && rest.get(digits..).is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
        });
    matches!(part.role, PartRole::Embedding | PartRole::ObjectFile | PartRole::ObjectReplacement) || named
}

/// Whether a part of the other format is a macro project, or a part of one: by its role, or by its name (**a part is judged by both, and either says it is**).
pub(crate) fn is_macro_part(part: &diacraft_doc::Part) -> bool {
    use diacraft_doc::PartRole;
    matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) || odf_macro_name(&part.name)
}

/// Whether `name` is a name a part of a newer `.diacraft` package may have (the same rules but for the entries an `.odg` makes).
pub(crate) fn native_entry_name_ok(name: &str) -> bool {
    let bare = name.strip_suffix('/').unwrap_or(name);
    let bytes = name.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    !(bare.is_empty()
        || name.len() > MAX_ENTRY_NAME_BYTES
        || name.starts_with('/')
        || name.contains('\\')
        || drive
        || name.chars().any(|c| c.is_control())
        || bare.split('/').any(|segment| segment.is_empty() || segment == "." || segment == ".."))
}

/// The content type a part is given in the package: the media type it had when that is one (a `type/subtype` of token characters), else `application/octet-stream`. A media type
/// comes from a file and may be anything.
pub(crate) fn content_type_of(media_type: &str) -> &str {
    let token = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b));
    match media_type.split_once('/') {
        Some((kind, sub)) if token(kind) && token(sub) => media_type,
        _ => "application/octet-stream",
    }
}

/// The `Data` element as the model keeps it: DiaCraft's own settings and anything else the data holds, **without the items this build reads** (`skip`, by their place among the
/// children), which are the carrier's and not the document's.
pub(crate) fn raw_without(data: &El, skip: &std::collections::HashSet<usize>) -> diacraft_doc::RawElement {
    diacraft_doc::RawElement {
        name: std::sync::Arc::from(data.name.as_str()),
        attrs: diacraft_sheetxml::passthrough::all_attrs(data),
        children: data
            .children
            .iter()
            .enumerate()
            .filter(|(at, _)| !skip.contains(at))
            .map(|(_, node)| diacraft_sheetxml::passthrough::raw_node(node))
            .collect(),
    }
}

/// Puts a name on an item: its element, with DiaCraft's prefix where the data has one.
pub(crate) fn item_name(prefix: &str) -> String {
    if prefix.is_empty() { ITEM.to_owned() } else { format!("{prefix}:{ITEM}") }
}

/// An attribute holding text that XML may not hold as it is: the codec's own rule (`-b64` on the name and the bytes in base64), for a name from a person.
pub(crate) fn put_text(el: &mut El, name: &str, value: &str) {
    use diacraft_sheetxml::model::escape::{base64_encode, needs_base64};
    if needs_base64(value) {
        el.attrs.push((format!("{name}-b64"), base64_encode(value.as_bytes())));
    } else {
        el.attrs.push((name.to_owned(), value.to_owned()));
    }
}

/// An attribute written by [`put_text`].
pub(crate) fn get_text(el: &El, name: &str) -> Option<String> {
    use diacraft_sheetxml::model::escape::base64_string;
    match el.attr(name) {
        Some(value) => Some(value.to_owned()),
        None => el.attr(&format!("{name}-b64")).and_then(base64_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_that_reach_outside_a_package_are_not_names() {
        for bad in [
            "",
            "/",
            "/abs",
            "..",
            "../x",
            "a/../b",
            "a/..",
            "a//b",
            "a\\b",
            "C:x",
            "c:/x",
            "x\u{0}y",
            "./x",
            "mimetype",
            "Content.XML",
            "META-INF/manifest.xml",
            "diacraft/x",
            "DiaCraft/",
            "DIACRAFT",
            // The two parts a package takes from the model are names only as the package spells them.
            "Meta.xml",
            "SETTINGS.XML",
            "meta.xml/",
        ] {
            assert!(!odf_entry_name_ok(bad), "{bad:?}");
        }
        for good in [
            "meta.xml",
            "settings.xml",
            "Object 1/content.xml",
            "Object 1/",
            "Basic/Standard/Module1.xml",
            "Pictures/a b.png",
            "META-INF/documentsignatures.xml",
            "ObjectReplacements/Object 1",
            "..a",
            "a..",
            "a.b/c",
        ] {
            assert!(odf_entry_name_ok(good), "{good:?}");
        }
        assert!(!odf_entry_name_ok(&"x".repeat(MAX_ENTRY_NAME_BYTES + 1)));
        assert!(odf_entry_name_ok(&"x".repeat(MAX_ENTRY_NAME_BYTES)));
    }

    #[test]
    fn a_macro_project_is_known_by_its_name_in_any_case_and_a_role_is_made_from_the_name() {
        use diacraft_doc::PartRole;
        for name in [
            "Basic/Standard/m.xml",
            "Basic/",
            "Scripts/python/x.py",
            "basic/x",
            "BASIC/X",
            "SCRIPTS/y",
            "META-INF/macrosignatures.xml",
            "meta-inf/MacroSignatures.xml",
        ] {
            assert!(odf_macro_name(name), "{name}");
        }
        for name in ["Pictures/p.png", "Object 1/content.xml", "basics/x", "xBasic/x", "META-INF/documentsignatures.xml", "ScriptsX/y", ""] {
            assert!(!odf_macro_name(name), "{name}");
        }
        for (name, role) in [
            ("Basic/x.xml", PartRole::MacroProject),
            ("Scripts/x.py", PartRole::MacroProject),
            ("META-INF/macrosignatures.xml", PartRole::MacroSignature),
            ("META-INF/documentsignatures.xml", PartRole::Signature),
            ("Thumbnails/thumbnail.png", PartRole::Thumbnail),
            ("Thumbnails/", PartRole::Other),
            ("ObjectReplacements/Object 1", PartRole::ObjectReplacement),
            ("Object 12/content.xml", PartRole::ObjectFile),
            ("Object /x", PartRole::Other),
            ("Object 1", PartRole::Other),
            ("Fonts/a.ttf", PartRole::Font),
            ("Pictures/p.png", PartRole::Other),
        ] {
            assert_eq!(odf_role_of(name), role, "{name}");
        }
    }

    #[test]
    fn only_a_part_under_the_folder_of_diacraft_is_one_of_its_own() {
        for name in ["/diacraft/blob1.bin", "/DiaCraft/Blob2.XML", "/diacraft/sub/x.png"] {
            assert!(is_blob_target(name), "{name}");
        }
        for name in [
            "/visio/pages/page1.xml",
            "/visio/document.xml",
            "/diacraft/",
            "/diacraft",
            "/diacraft/_rels/blob1.bin.rels",
            "/diacraftx/blob1.bin",
            "diacraft/blob1.bin",
            "/x/diacraft/blob1.bin",
            "",
        ] {
            assert!(!is_blob_target(name), "{name}");
        }
    }

    #[test]
    fn a_native_part_may_be_named_as_an_odf_reserved_entry_but_not_outside_the_package() {
        assert!(native_entry_name_ok("content.xml") && native_entry_name_ok("extras/x.bin"));
        for bad in ["", "..", "../x", "/x", "a\\b", "c:\\x"] {
            assert!(!native_entry_name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_content_type_is_a_media_type_or_octet_stream() {
        assert_eq!(content_type_of("image/png"), "image/png");
        assert_eq!(content_type_of("application/vnd.sun.star.basic"), "application/vnd.sun.star.basic");
        for bad in ["", "x", "a/b/c", "a b/c", "text/xml; charset=utf-8", "/x", "x/", "a/\u{1}"] {
            assert_eq!(content_type_of(bad), "application/octet-stream", "{bad:?}");
        }
    }

    #[test]
    fn a_text_attribute_round_trips_what_xml_cannot_hold() {
        for text in ["plain", "", "a \"quoted\" <name>", "ctrl\u{1}char", "ünï"] {
            let mut el = El { name: "x".into(), attrs: Vec::new(), children: Vec::new() };
            put_text(&mut el, "name", text);
            assert_eq!(get_text(&el, "name").as_deref(), Some(text), "{text:?}");
        }
    }
}
