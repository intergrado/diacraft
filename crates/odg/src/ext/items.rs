//! The VSDX data in DiaCraft's document file, and the bytes beside it in the `DiaCraft/` entries (the native format's design, 5.1, 5.3, 5.4 and 5.6;  `docs/diacraft-format.md` 7.3).
//!
//! A drawing that holds the data of a VSDX file (what the VSDX reader keeps in the `vsdx` side of the slots, the attributes and elements no field has, the parts of the package, the
//! package's relationships and the properties no ODF field has) and is saved as an `.odg` **carries that data inside the file**, so that it comes back when the file is read again. Three places:
//!
//! * **a shape's own automatic style** (tier 2: `payload.rs`, the `vsdx` and `foreign` items): the shapes that have one;
//! * **DiaCraft's document file**, as `vsdx` elements that are children of its root, each an **item**: one of `document`, `parts`, `page`, `style` and `group` (the group of a page has no style of its own).
//!   An item has the fragment of the model that holds its data, as one element of the whole-model codec (a shell, `shell.rs`), its **key** and, where the file says something the item sits beside,
//!   its **stamp** (`fp`);
//! * **the `DiaCraft/` entries**: each distinct byte string the parts hold is one entry, `DiaCraft/blob<n>.<ext>`, with a manifest row, and the manifest lists the folder `DiaCraft/` with a media type
//!   that is not empty (`FOLDER_MEDIA_TYPE`), which is what keeps every entry through a save by another program.
//!
//! | `of` | key | stamp | fragment |
//! |---|---|---|---|
//! | `document` | none | the title and the descriptive properties | the document's shell |
//! | `parts` | none | none (it sits beside nothing the file says) | a document holding the parts and the custom properties of the other formats, and the list of their bytes |
//! | `page` | `name`, `nth` | none | the page's shell |
//! | `style` | `name`, else `id` | none | the style's shell |
//! | `group` | `page`, `page-nth`, `name`, `nth` | the names of the group's members | the group's shell |
//!
//! **The judge.** A read keeps an item only while it is good: **an item whose object is not in the file is dropped silently**; an item whose stamp does not match what the file says now is dropped,
//! so that the other program's edit wins; an item for an object that two items name names nothing and is dropped with a note; an item that names an entry the package does not have, two records that name one
//! entry, and a part whose name no package of its format could hold are dropped **with a note**. An entry is **read only when its item is good**, its bytes charged first, and is never parsed, run or opened.
//!
//! **Caps** (ruled none lower than the design's): an entry at most 256 MiB, all of them at most 512 MiB in a file, [`MAX_CARRIED_ENTRIES`] entries (the manifest is read up to 4 MiB, and a row
//! of a carried entry is about 120 bytes). A save that would pass one is refused in words that name the part; nothing is cut.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, Format, Part, PartRole};
use diacraft_sheetxml::model::{
    BlobId, BlobSource, CodecError, MemBlobs, decode_page, decode_shape, decode_style, encode, encode_page, encode_shape, encode_style, part_lists,
    stamp_of_bytes,
};

use super::body::{self, NoBlobs};
use super::fingerprint::{Digest, Fp};
use super::shell;
use crate::error::OdgError;
use crate::package::Package;
use crate::read::Reader;
use crate::write::WriteError;

/// The folder of DiaCraft's entries, with its slash.
pub(crate) const FOLDER: &str = "DiaCraft/";

/// The media type the manifest's row of the folder `DiaCraft/` has: **it must not be empty**, or a program that edits the drawing drops every entry of the folder when it saves it
/// It is DiaCraft's own, never an OpenDocument media type.
pub(crate) const FOLDER_MEDIA_TYPE: &str = "application/vnd.diacraft.parts";

/// The most bytes one entry of DiaCraft's may have: the package's cap for a part.
pub(crate) const MAX_ENTRY_BYTES: u64 = diacraft_container::limits::MAX_PART_BYTES;

/// The most bytes all the entries of DiaCraft's may have in one file: **half the package's inflated total** (a file of 1 GiB of incompressible carried bytes opened in up to 12 seconds from
/// a cold page cache, and one of 512 MiB in about five).
pub(crate) const MAX_CARRIED_BYTES: u64 = 512 * 1024 * 1024;

/// The most entries of DiaCraft's own one file carries: the manifest is read up to 4 MiB, and a row of an entry named `DiaCraft/blob<n>.<ext>` is about 120 bytes, so 30,000 rows are 3.6 MB. More is refused
/// at the save, in words, and a reader that finds more leaves the rest out.
pub(crate) const MAX_CARRIED_ENTRIES: usize = 30_000;

/// The size from which an entry of DiaCraft's is stored and not deflated: deflate cannot pass a ratio of 1,032 and a stream of zeros under this size cannot come near the ratio the reader refuses
/// (1,000); a bigger entry is stored, which no ratio refuses.
pub(crate) const STORED_FROM: usize = 64 * 1024;

/// The element an item is, as the local name of a child of the document file's root.
pub(crate) const ITEM: &str = "vsdx";

/// The most bytes a part's name (of either format) may have: the container's.
const MAX_NAME_BYTES: usize = diacraft_container::limits::MAX_PART_NAME_BYTES;

// ------------------------------------------------------------------------------------------------------------------ names

/// Whether `name` is an entry name of DiaCraft's own: **under `DiaCraft/`** (compared ASCII case-insensitively), a file and no folder, in no deeper folder than the entries of the package are, the name of no
/// reserved entry, and a name no package refuses (no `..`, no leading slash, no `\`, no drive letter, no control character, no empty or `.` segment).
pub(crate) fn entry_name_ok(name: &str) -> bool {
    let Some(rest) = name.get(..FOLDER.len()).filter(|head| head.eq_ignore_ascii_case(FOLDER)).and(name.get(FOLDER.len()..)) else { return false };
    let bytes = name.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    !(rest.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.ends_with('/')
        || name.starts_with('/')
        || name.contains('\\')
        || drive
        || name.chars().any(char::is_control)
        || name.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || name.eq_ignore_ascii_case(super::docfile::PATH))
}

/// Whether `name` is a part name a VSDX package may hold (the rules of the Open Packaging Conventions): **a slash and then segments joined by slashes**, no segment empty, none that ends in a dot, none that is
/// `.` or `..` (a name that reaches outside the package or names the folder one is in), no control character, no backslash, no more bytes than the container allows, and not the content types stream, which
/// is no part. A name with no leading slash (a relative or an absolute path of a file system, a drive) is none.
pub(crate) fn vsdx_part_name_ok(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('/') else { return false };
    !(rest.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.contains('\\')
        || name.chars().any(char::is_control)
        || rest.split('/').any(|segment| segment.is_empty() || segment.ends_with('.'))
        || name.eq_ignore_ascii_case("/[Content_Types].xml"))
}

/// Whether `name` is a name a part of a newer `.diacraft` package may have (its entries are names of a ZIP: the rules above but for a leading slash, which they have none of).
pub(crate) fn native_part_name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    let drive = matches!((bytes.first(), bytes.get(1)), (Some(c), Some(b':')) if c.is_ascii_alphabetic());
    let bare = name.strip_suffix('/').unwrap_or(name);
    !(bare.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.starts_with('/')
        || name.contains('\\')
        || drive
        || name.chars().any(char::is_control)
        || bare.split('/').any(|segment| segment.is_empty() || segment == "." || segment == ".."))
}

/// Whether a part of the other formats is a macro project, or a part of one: by its role or by its name (**a part is judged by both, and either says it is**): the VBA project of a VSDX package and its
/// signature and data parts. Compared ASCII case-insensitively.
pub(crate) fn is_macro_part(part: &Part) -> bool {
    if matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) {
        return true;
    }
    let folded = part.name.to_ascii_lowercase();
    folded.ends_with("vbaproject.bin")
        || folded.ends_with("vbadata.xml")
        || folded.ends_with("vbaprojectsignature.bin")
        || folded.starts_with("basic/")
        || folded.starts_with("scripts/")
}

/// Whether the part is one that rides: any part of a VSDX package but its thumbnail (the `.odg` has a thumbnail of its own, made again from the drawing, design 4.4), and every part of a newer `.diacraft`. The
/// parts of the `.odg` format are the `.odg` writer's own.
fn rides(part: &Part) -> bool {
    match part.format {
        Format::Odf => false,
        Format::Vsdx => part.role != PartRole::Thumbnail,
        Format::Native => true,
    }
}

// ------------------------------------------------------------------------------------------------------------------ stamps

/// The stamp of the document: its title and the descriptive fields of its properties, **as an `.odg` says them** (the title, the subject, the creator, the keywords, the description and the language): what the
/// meta part holds of it.
pub(crate) fn document_fp(doc: &Document) -> Fp {
    let mut d = Digest::new("document");
    let p = &doc.props;
    d.text(&doc.title);
    for field in [&p.subject, &p.creator, &p.keywords, &p.description, &p.language] {
        match field {
            // A field with no text is no field to the reader of `meta.xml`: the stamp of it is the stamp of none, as the file says it.
            Some(v) if !v.is_empty() => d.text("some").text(v),
            _ => d.text("none"),
        };
    }
    d.finish()
}

/// The stamp of a group: the names of its members, in order (the group sits beside the shapes it holds; a member another program took out, put in or renamed is a change of what the group is).
pub(crate) fn group_fp<'a>(members: impl Iterator<Item = Option<&'a str>>) -> Fp {
    let mut d = Digest::new("group");
    let mut count = 0u64;
    for name in members {
        count = count.saturating_add(1);
        match name {
            Some(n) => d.text("some").text(n),
            None => d.text("none"),
        };
    }
    d.field(&count.to_le_bytes());
    d.finish()
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// A byte string that rides in an entry of its own.
#[derive(Clone)]
pub(crate) struct Blob {
    /// The entry's name, `DiaCraft/blob<n>.<ext>`.
    pub name: String,
    pub media_type: String,
    pub bytes: Arc<[u8]>,
}

impl std::fmt::Debug for Blob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Blob({}, {} bytes)", self.name, self.bytes.len())
    }
}

/// What a model carries in the file: the elements of the document file, the entries, and what was left out.
#[derive(Clone, Debug, Default)]
pub(crate) struct Built {
    /// The `vsdx` elements, children of the document file's root, in the order: the document's, the parts', the pages', the styles', the groups'.
    pub elements: Vec<El>,
    /// The entries, in the order of their numbers.
    pub blobs: Vec<Blob>,
    /// What was left out at the caller's choice (`drop_macros`): the names of the parts of a macro project.
    pub macros_dropped: Vec<String>,
    /// The classes of data the file carries, for the note (the design's 5.6).
    pub classes: Vec<&'static str>,
    /// A macro project rides, embedded objects ride.
    pub macros: bool,
    pub objects: bool,
}

/// Why a model's data cannot be carried: the codec's refusal, in the writer's words.
fn refused(e: CodecError) -> WriteError {
    match e {
        CodecError::Limit { what, max } => WriteError::Limit { what, max },
        CodecError::Malformed { why, .. } => WriteError::Unsupported(why),
        CodecError::Xml(_) => {
            WriteError::Unsupported("data of the drawing with a character XML cannot hold, which an .odg carries in DiaCraft's own file")
        }
    }
}

fn item(of: &str, keys: Vec<(String, String)>, fp: Option<Fp>, fragment: El) -> El {
    let mut attrs = vec![("of".to_owned(), of.to_owned())];
    attrs.extend(keys);
    if let Some(fp) = fp {
        attrs.push(("fp".to_owned(), fp.text()));
    }
    El { name: format!("{}:{ITEM}", body::P), attrs, children: vec![Node::El(fragment)] }
}

/// The parts of the other formats that ride, judged: a name no package of its format could hold is refused in words, a part past the cap of an entry is refused in words naming it, the parts of a macro project are
/// left out when the caller chose to (`drop_macros`), and a name that differs from an earlier one only in case is left out and said.
fn parts_that_ride(doc: &Document, drop_macros: bool, b: &mut Built) -> Result<Vec<Arc<Part>>, WriteError> {
    let mut out: Vec<Arc<Part>> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut total = 0u64;
    for part in doc.parts.iter().filter(|p| rides(p)) {
        if is_macro_part(part) && drop_macros {
            b.macros_dropped.push(part.name.to_string());
            continue;
        }
        let ok = match part.format {
            Format::Vsdx => vsdx_part_name_ok(&part.name),
            _ => native_part_name_ok(&part.name),
        };
        if !ok {
            return Err(WriteError::Carried {
                part: part.name.to_string(),
                why: "its name is not one a package of its format can hold (it reaches outside the package, is empty, or is one the package makes itself)",
            });
        }
        let len = u64::try_from(part.bytes.len()).unwrap_or(u64::MAX);
        if len > MAX_ENTRY_BYTES {
            return Err(WriteError::Carried {
                part: part.name.to_string(),
                why: "it is larger than the 256 MiB an entry of a file may be; leave it out, or save the drawing as a .diacraft file to keep it",
            });
        }
        if !seen.insert(part.name.to_ascii_lowercase()) {
            continue;
        }
        total = total.saturating_add(len);
        out.push(Arc::clone(part));
    }
    if total > MAX_CARRIED_BYTES {
        return Err(WriteError::Carried {
            part: String::new(),
            why: "the parts of the drawing that an .odg carries are more than the 512 MiB DiaCraft's entries of one file may hold; leave out the macro project or the objects, or save the drawing as a .diacraft file to keep them",
        });
    }
    Ok(out)
}

/// Makes the items of a model: see the module documentation. `drop_macros` is the caller's choice to leave out the macro projects of the other format.
pub(crate) fn build(doc: &Document, drop_macros: bool) -> Result<Built, WriteError> {
    let mut b = Built::default();
    let parts = parts_that_ride(doc, drop_macros, &mut b)?;
    if parts.len() > MAX_CARRIED_ENTRIES {
        return Err(WriteError::Limit {
            what: "parts of another format that one .odg can carry as entries of DiaCraft's own (the manifest is read up to 4 MiB); save the drawing as a .diacraft file to keep them",
            max: MAX_CARRIED_ENTRIES as u64,
        });
    }
    // The document's own data.
    let mut sink = MemBlobs::new();
    if let Some(shell) = shell::document_shell(doc) {
        let encoded = encode(&shell, &mut NoSink).map_err(refused)?;
        b.elements.push(item("document", Vec::new(), Some(document_fp(doc)), encoded.document));
        b.classes.push("document data");
    }
    // The parts and the custom properties of the other formats: one item, with no stamp.
    let custom: Vec<_> = doc.props.custom.iter().filter(|c| c.format != Format::Odf).cloned().collect();
    if !parts.is_empty() || !custom.is_empty() {
        let mut shell = Document::empty();
        shell.parts = diacraft_doc::SharedVec::from_vec(parts.clone());
        shell.props = Arc::new(diacraft_doc::DocProps { custom, ..diacraft_doc::DocProps::default() });
        let mut encoded = encode(&shell, &mut sink).map_err(refused)?;
        // The records of the list name each entry by the path of its entry in the package.
        for node in &mut encoded.document.children {
            if let Node::El(record) = node
                && record.name.rsplit(':').next() == Some("blob")
            {
                let id: u32 = record.attrs.iter().find(|(k, _)| k == "id").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                let media = sink.entry(BlobId(id)).map(|(_, m)| m.to_owned()).unwrap_or_default();
                let path = format!("{FOLDER}blob{id}.{}", diacraft_sheetxml::model::blob_extension(&media));
                match record.attrs.iter_mut().find(|(k, _)| k == "path") {
                    Some(slot) => slot.1 = path,
                    None => record.attrs.push(("path".to_owned(), path)),
                }
            }
        }
        for id in 0..sink.len() {
            let Some((bytes, media)) = u32::try_from(id).ok().and_then(|id| sink.entry(BlobId(id))) else { continue };
            b.blobs.push(Blob {
                name: format!("{FOLDER}blob{id}.{}", diacraft_sheetxml::model::blob_extension(media)),
                media_type: media.to_owned(),
                bytes: Arc::clone(bytes),
            });
        }
        if b.blobs.len() > MAX_CARRIED_ENTRIES {
            return Err(WriteError::Limit { what: "entries of DiaCraft's own that one .odg can carry", max: MAX_CARRIED_ENTRIES as u64 });
        }
        b.macros = parts.iter().any(|p| is_macro_part(p));
        b.objects = parts.iter().any(|p| p.role == PartRole::Embedding || p.role == PartRole::ObjectFile);
        if parts.iter().any(|p| !is_macro_part(p) && p.role != PartRole::Embedding && p.role != PartRole::ObjectFile)
            || !shell.props.custom.is_empty()
        {
            b.classes.push("package parts");
        }
        if b.macros {
            b.classes.push("macro project");
        }
        if b.objects {
            b.classes.push("embedded objects");
        }
        b.elements.push(item("parts", Vec::new(), None, encoded.document));
    }
    // The pages, by name and number among the pages of the name.
    let mut seen: HashMap<&str, u32> = HashMap::new();
    for page in doc.pages.iter() {
        let count = seen.entry(&*page.name).or_insert(0);
        *count = count.saturating_add(1);
        let nth = *count;
        if let Some(shell) = shell::page_shell(page) {
            let fragment = encode_page(&shell, &mut NoSink).map_err(refused)?;
            let mut keys = vec![("name".to_owned(), page.name.to_string())];
            if nth > 1 {
                keys.push(("nth".to_owned(), nth.to_string()));
            }
            b.elements.push(item("page", keys, None, fragment));
            if !b.classes.contains(&"page data") {
                b.classes.push("page data");
            }
        }
    }
    // The style sheets, by the name the file holds them by, else by their number.
    let mut names: HashMap<&str, u32> = HashMap::new();
    for style in doc.styles.iter() {
        if let Some(n) = style.name_u.as_deref() {
            *names.entry(n).or_insert(0) += 1;
        }
    }
    for (at, style) in doc.styles.iter().enumerate() {
        let Some(shell) = shell::style_shell(style) else { continue };
        let fragment = encode_style(&shell, &mut NoSink).map_err(refused)?;
        // The root has no element of its own: it is keyed by its place (what the element of a VSDX file's root held that no field reads rides like any other style's).
        let keys = match style.name_u.as_deref() {
            _ if at == 0 => vec![("root".to_owned(), "1".to_owned())],
            Some(n) if names.get(n) == Some(&1) => vec![("name".to_owned(), n.to_owned())],
            _ => vec![("id".to_owned(), style.id.to_string())],
        };
        b.elements.push(item("style", keys, None, fragment));
        if !b.classes.contains(&"style data") {
            b.classes.push("style data");
        }
    }
    Ok(b)
}

/// Whether the model holds what the baseline held of everything the items carry but the groups' (which are compared with the entries of the pages): the document's shell, the parts and the custom properties of
/// the other formats, and the shells of the pages and the styles. A value comparison: no tree is built. **With `drop_macros` the parts of a macro project are none of it**: the save leaves
/// them out ([`build`]), so a drawing that holds one, saved into a file a save made without it, holds what the file holds.
pub(crate) fn same(doc: &Document, base: &Document, drop_macros: bool) -> bool {
    let parts = |d: &Document| -> Vec<Arc<Part>> { d.parts.iter().filter(|p| rides(p) && !(drop_macros && is_macro_part(p))).cloned().collect() };
    let custom = |d: &Document| -> Vec<diacraft_doc::CustomProp> { d.props.custom.iter().filter(|c| c.format != Format::Odf).cloned().collect() };
    let pages = |d: &Document| -> Vec<(Arc<str>, Option<diacraft_doc::Page>)> {
        d.pages.iter().map(|p| (Arc::clone(&p.name), shell::page_shell(p))).collect()
    };
    let styles = |d: &Document| -> Vec<Option<diacraft_doc::Style>> { d.styles.iter().map(|s| shell::style_shell(s)).collect() };
    shell::document_shell(doc) == shell::document_shell(base)
        && parts(doc) == parts(base)
        && custom(doc) == custom(base)
        && pages(doc) == pages(base)
        && styles(doc) == styles(base)
}

/// A sink for a fragment that holds no bytes: a shell never holds a picture or a part.
struct NoSink;

impl diacraft_sheetxml::model::BlobSink for NoSink {
    fn put(&mut self, _: &Arc<[u8]>, _: &str) -> Result<BlobId, CodecError> {
        Err(CodecError::Malformed { part: "item".to_owned(), why: "a fragment that holds bytes" })
    }
}

/// The element of a group's item: its shell and its stamp, under the key the group's entry has (`page`, `page-nth`, `name`, and `nth` for a group whose name is not its alone).
pub(crate) fn group_item(
    keys: Vec<(String, String)>,
    shell: &diacraft_doc::Shape,
    members: impl Iterator<Item = Option<String>>,
) -> Result<El, WriteError> {
    let fragment = encode_shape(shell, &mut NoSink).map_err(refused)?;
    let names: Vec<Option<String>> = members.collect();
    let fp = group_fp(names.iter().map(|n| n.as_deref()));
    Ok(item("group", keys, Some(fp), fragment))
}

// ------------------------------------------------------------------------------------------------------------------ reading

/// The source of the bytes a decode asks for, by the number of the blob.
struct Blobs(HashMap<u32, Arc<[u8]>>);

impl BlobSource for Blobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.0.get(&id.0).cloned()
    }
}

/// The `vsdx` children of the root of the document file, and the key an item names its object by.
pub(crate) fn elements_of<'e>(root: &'e El, sc: &body::Sc) -> Vec<&'e El> {
    root.elements().filter(|e| sc.enter(e).is(e, ITEM)).collect()
}

/// The items of one kind.
fn of<'e>(items: &[&'e El], kind: &str) -> Vec<&'e El> {
    items.iter().copied().filter(|e| e.attr("of") == Some(kind)).collect()
}

/// Reads the items of the pages and the styles over the document the reader made (but the groups', which are put on the groups as they are built: [`read_group_item`]), and judges them: see the module
/// documentation. The pages and the styles are the markup's and the document file's own when this is called.
pub(crate) fn read_early(r: &mut Reader<'_>, doc: &mut Document, root: &El) -> Result<(), OdgError> {
    let sc = body::Sc::empty().enter(root);
    let items = elements_of(root, &sc);
    if items.is_empty() {
        return Ok(());
    }
    r.report.carry.items = r.report.carry.items.saturating_add(items.len() as u64);
    pages_items(r, doc, &of(&items, "page"))?;
    styles_items(r, doc, &of(&items, "style"))?;
    Ok(())
}

/// Reads the items of the document and of its parts, **once the document's title and properties are read** (`keep::apply`: the stamp of the document is of them, and a user-defined property of the other
/// formats replaces the ODF one that was written for it).
pub(crate) fn read_late(r: &mut Reader<'_>, doc: &mut Document, root: &El, package: Option<&Package>) -> Result<(), OdgError> {
    let sc = body::Sc::empty().enter(root);
    let items = elements_of(root, &sc);
    for kind in ["document", "parts"] {
        let list = of(&items, kind);
        match list.as_slice() {
            [] => {}
            [one] => {
                r.saw_element();
                r.take_items(1 + one.attrs.len())?;
                if kind == "document" {
                    document_item(r, doc, one)?;
                } else {
                    parts_item(r, doc, one, package, true)?;
                }
            }
            many => {
                r.report.carry.left_out = r.report.carry.left_out.saturating_add(many.len() as u64);
                r.warn("diacraft_item_ambiguous", || format!("DiaCraft's document file has more than one item for the {kind}, and none is read"));
            }
        }
    }
    Ok(())
}

/// The first element child of an item that is its fragment.
fn fragment_of(item: &El) -> Option<&El> {
    item.elements().next()
}

fn damaged(r: &mut Reader<'_>, what: &str) {
    r.report.carry.left_out = r.report.carry.left_out.saturating_add(1);
    r.warn("diacraft_item_damaged", || format!("an item of DiaCraft's document file for {what} is damaged, and is left out"));
}

fn document_item(r: &mut Reader<'_>, doc: &mut Document, item: &El) -> Result<(), OdgError> {
    let Some(fp) = item.attr("fp").and_then(Fp::parse) else {
        damaged(r, "the document");
        return Ok(());
    };
    if document_fp(doc) != fp {
        r.report.carry.stale = r.report.carry.stale.saturating_add(1);
        return Ok(());
    }
    let Some(fragment) = fragment_of(item) else {
        damaged(r, "the document");
        return Ok(());
    };
    let Some(shell) = decode_document(r, fragment, &Blobs(HashMap::new()), "the document")? else { return Ok(()) };
    let lost = shell::document_put(doc, &shell);
    if lost > 0 {
        r.warn("diacraft_item_damaged", || {
            "an item of DiaCraft's document file for the document names faces the file does not have, which are left out".to_owned()
        });
    }
    r.report.carry.kept = r.report.carry.kept.saturating_add(1);
    Ok(())
}

/// Decodes a document fragment within the room the reader has left. `None` for a fragment that is damaged (said).
fn decode_document(r: &mut Reader<'_>, fragment: &El, blobs: &dyn BlobSource, what: &str) -> Result<Option<Document>, OdgError> {
    let made = body::decode_in_at(r, &format!("an item of DiaCraft's document file for {what}"), |budget, notes| {
        diacraft_sheetxml::model::decode(fragment, &[], &[], blobs, budget, notes)
    })?;
    if made.is_none() {
        r.report.carry.left_out = r.report.carry.left_out.saturating_add(1);
    }
    Ok(made)
}

fn parts_item(r: &mut Reader<'_>, doc: &mut Document, item: &El, package: Option<&Package>, keep: bool) -> Result<(), OdgError> {
    let Some(fragment) = fragment_of(item) else {
        damaged(r, "the parts");
        return Ok(());
    };
    // The records of the bytes: each names an entry of the package under `DiaCraft/` by its path, with its size and the stamp of its bytes.
    let Ok(lists) = part_lists(fragment) else {
        damaged(r, "the parts");
        return Ok(());
    };
    let mut blobs: HashMap<u32, Arc<[u8]>> = HashMap::new();
    let mut named: HashSet<String> = HashSet::new();
    let (mut missing, mut twice, mut refused, mut over) = (0u64, 0u64, 0u64, 0u64);
    for record in &lists.blobs {
        r.take_items(2)?;
        if !named.insert(record.path.to_ascii_lowercase()) {
            twice += 1;
            continue;
        }
        if !entry_name_ok(&record.path) {
            refused += 1;
            continue;
        }
        let Some(bytes) = package.and_then(|p| p.part_shared(&record.path)) else {
            missing += 1;
            continue;
        };
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        // A part whose size or stamp is not the one the record states is not the part it describes.
        if len != record.size || stamp_of_bytes(&bytes) != record.stamp {
            missing += 1;
            continue;
        }
        if len > MAX_ENTRY_BYTES || len > r.carried_left {
            over += 1;
            continue;
        }
        r.carried_left -= len;
        r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(len);
        if keep {
            blobs.insert(record.id.0, bytes);
        }
    }
    for (n, what) in [
        (twice, "name an entry that another record names"),
        (refused, "name an entry that no package could hold"),
        (missing, "name an entry the package does not have, or one that is not what they describe"),
        (over, "name entries past what a file's entries may hold"),
    ] {
        if n > 0 {
            r.report.carry.left_out = r.report.carry.left_out.saturating_add(n);
            r.warn("diacraft_item_entry", || format!("{n} records of DiaCraft's document file {what}, and are left out"));
        }
    }
    let Some(shell) = decode_document(r, fragment, &Blobs(blobs), "the parts")? else { return Ok(()) };
    put_parts(r, doc, &shell, keep)?;
    // A user-defined property of the other formats is written as an ODF one, by its name and value, where the file says properties (`meta.xml`): **the exact property replaces that one** while the file still
    // says the same name and value; one another program changed or took out is stale or gone, and the file's own is read.
    let carried: Vec<_> = shell.props.custom.iter().filter(|c| c.format != Format::Odf).cloned().collect();
    if !carried.is_empty() {
        r.take_items(doc.props.custom.len().saturating_add(carried.len()))?;
        let mut custom = doc.props.custom.clone();
        let mut taken: HashSet<usize> = HashSet::new();
        let mut index: HashMap<(Arc<str>, Arc<str>), Vec<usize>> = HashMap::new();
        for (at, c) in custom.iter().enumerate() {
            if c.format == Format::Odf {
                index.entry((Arc::clone(&c.name), Arc::clone(&c.value))).or_default().push(at);
            }
        }
        for c in carried {
            let at = index.get(&(Arc::clone(&c.name), Arc::clone(&c.value))).and_then(|places| places.iter().copied().find(|a| !taken.contains(a)));
            match at {
                Some(at) => {
                    taken.insert(at);
                    if let Some(slot) = custom.get_mut(at) {
                        *slot = c;
                    }
                }
                None => r.report.carry.gone = r.report.carry.gone.saturating_add(1),
            }
        }
        if keep {
            Arc::make_mut(&mut doc.props).custom = custom;
        }
    }
    r.report.carry.kept = r.report.carry.kept.saturating_add(1);
    Ok(())
}

/// The parts of a decoded item go into the model: each under a name a package of its format could hold that no earlier part of the model has, even by case, and as many as the model may hold.
/// **A part's role is the role its name says** for the parts of an OpenDocument package; a macro project is bytes, whatever the type of the file.
fn put_parts(r: &mut Reader<'_>, doc: &mut Document, shell: &Document, keep: bool) -> Result<(), OdgError> {
    let mut seen: HashSet<String> = doc.parts.iter().filter(|p| p.format != Format::Odf).map(|p| p.name.to_ascii_lowercase()).collect();
    let (mut refused, mut twice, mut over) = (0u64, 0u64, 0u64);
    for part in shell.parts.iter().filter(|p| p.format != Format::Odf) {
        r.take_items(2)?;
        let ok = match part.format {
            Format::Vsdx => vsdx_part_name_ok(&part.name),
            _ => native_part_name_ok(&part.name),
        };
        if !ok {
            refused += 1;
            continue;
        }
        if !seen.insert(part.name.to_ascii_lowercase()) {
            twice += 1;
            continue;
        }
        if doc.parts.len() >= diacraft_doc::MAX_PARTS {
            over += 1;
            continue;
        }
        r.report.carry.parts_kept = r.report.carry.parts_kept.saturating_add(1);
        if keep {
            doc.parts.push(Arc::clone(part));
        }
    }
    for (n, what) in [
        (refused, "have a name that no package of their format could hold"),
        (twice, "have a name that an earlier part has, even by case"),
        (over, "are more than a drawing holds"),
    ] {
        if n > 0 {
            r.report.carry.left_out = r.report.carry.left_out.saturating_add(n);
            r.warn("diacraft_part_name_refused", || format!("{n} parts that DiaCraft's document file carries {what}, and are left out"));
        }
    }
    Ok(())
}

/// The items of one kind whose key is theirs alone, by `key`: two items for one object name nothing and are all left out, with a note.
fn unique<'e>(r: &mut Reader<'_>, list: Vec<&'e El>, what: &str, key: impl Fn(&El) -> Option<String>) -> Vec<(&'e El, String)> {
    let keys: Vec<Option<String>> = list.iter().map(|e| key(e)).collect();
    let mut count: HashMap<&str, u32> = HashMap::with_capacity(keys.len());
    for k in keys.iter().flatten() {
        *count.entry(k.as_str()).or_insert(0) += 1;
    }
    let (mut damaged_n, mut twice) = (0u64, 0u64);
    let mut out = Vec::with_capacity(list.len());
    for (e, k) in list.iter().zip(&keys) {
        match k.as_deref() {
            None => damaged_n += 1,
            Some(k) if count.get(k).copied().unwrap_or(0) == 1 => out.push((*e, k.to_owned())),
            Some(_) => twice += 1,
        }
    }
    if damaged_n > 0 || twice > 0 {
        r.report.carry.left_out = r.report.carry.left_out.saturating_add(damaged_n).saturating_add(twice);
        r.warn("diacraft_item_ambiguous", || {
            format!("DiaCraft's document file has items for {what} that have no key, or name an object that another item names: they are left out")
        });
    }
    out
}

fn pages_items(r: &mut Reader<'_>, doc: &mut Document, list: &[&El]) -> Result<(), OdgError> {
    if list.is_empty() {
        return Ok(());
    }
    r.take_items(doc.pages.len())?;
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (at, p) in doc.pages.iter().enumerate() {
        by_name.entry(p.name.to_string()).or_default().push(at);
    }
    let keyed = unique(r, list.to_vec(), "pages", |e| {
        let nth = e.attr("nth").and_then(diacraft_sheetxml::passthrough::canonical_u32).unwrap_or(1);
        e.attr("name").map(|n| format!("{n}\u{0}{nth}"))
    });
    for (item, key) in keyed {
        r.saw_element();
        r.take_items(1 + item.attrs.len())?;
        let (name, nth) = key.split_once('\u{0}').unwrap_or((&key, "1"));
        let nth: usize = nth.parse().unwrap_or(1);
        let Some(at) = by_name.get(name).and_then(|places| places.get(nth.saturating_sub(1))).copied() else {
            r.report.carry.gone = r.report.carry.gone.saturating_add(1);
            continue;
        };
        let Some(fragment) = fragment_of(item) else {
            damaged(r, "a page");
            continue;
        };
        let Some(shell) =
            body::decode_in_at(r, "an item of DiaCraft's document file for a page", |budget, notes| decode_page(fragment, &NoBlobs, budget, notes))?
        else {
            continue;
        };
        if let Some(page) = doc.pages.get_mut(at).map(Arc::make_mut) {
            shell::page_put(page, &shell);
            r.report.carry.kept = r.report.carry.kept.saturating_add(1);
        }
    }
    Ok(())
}

fn styles_items(r: &mut Reader<'_>, doc: &mut Document, list: &[&El]) -> Result<(), OdgError> {
    if list.is_empty() {
        return Ok(());
    }
    r.take_items(doc.styles.len())?;
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut by_id: HashMap<u32, usize> = HashMap::new();
    for (at, s) in doc.styles.iter().enumerate().skip(1) {
        if let Some(n) = s.name_u.as_deref() {
            by_name.entry(n.to_owned()).or_insert(at);
        }
        by_id.entry(s.id).or_insert(at);
    }
    let keyed = unique(r, list.to_vec(), "styles", |e| match (e.attr("root"), e.attr("name"), e.attr("id")) {
        (Some(_), _, _) => Some("r".to_owned()),
        (None, Some(n), _) => Some(format!("n{n}")),
        (None, None, Some(id)) => Some(format!("i{id}")),
        _ => None,
    });
    for (item, key) in keyed {
        r.saw_element();
        r.take_items(1 + item.attrs.len())?;
        let at = match key.split_at(1) {
            ("r", _) => Some(0),
            ("n", name) => by_name.get(name).copied(),
            (_, id) => id.parse::<u32>().ok().and_then(|id| by_id.get(&id).copied()),
        };
        let Some(at) = at else {
            r.report.carry.gone = r.report.carry.gone.saturating_add(1);
            continue;
        };
        let Some(fragment) = fragment_of(item) else {
            damaged(r, "a style");
            continue;
        };
        let Some(shell) = body::decode_in_at(r, "an item of DiaCraft's document file for a style", |budget, notes| {
            decode_style(fragment, &NoBlobs, budget, notes)
        })?
        else {
            continue;
        };
        if let Some(style) = doc.styles.get_mut(at).map(Arc::make_mut) {
            shell::style_put(style, &shell);
            r.report.carry.kept = r.report.carry.kept.saturating_add(1);
        }
    }
    Ok(())
}

/// What a group's item holds, read before the pages are: its shell and its stamp, to be put on the group when it is built and judged against its members.
pub(crate) struct GroupItem {
    pub fp: Option<Fp>,
    pub shell: diacraft_doc::Shape,
}

/// Reads the shell of a group's item (a `vsdx` element of `of="group"`), within the room the reader has left. `None` for an item that is damaged, with a note.
pub(crate) fn read_group_item(r: &mut Reader<'_>, item: &El) -> Result<Option<GroupItem>, OdgError> {
    let Some(fragment) = fragment_of(item) else {
        damaged(r, "a group");
        return Ok(None);
    };
    let fp = item.attr("fp").and_then(Fp::parse);
    let Some(shell) =
        body::decode_in_at(r, "an item of DiaCraft's document file for a group", |budget, notes| decode_shape(fragment, &NoBlobs, budget, notes))?
    else {
        r.report.carry.left_out = r.report.carry.left_out.saturating_add(1);
        return Ok(None);
    };
    Ok(Some(GroupItem { fp, shell }))
}

/// Whether `OdgError` is a cap the read may not go past: for the callers that keep the reading of items going on a damaged one.
#[allow(dead_code)]
pub(crate) fn is_cap(e: &OdgError) -> bool {
    matches!(e, OdgError::Limit { .. } | OdgError::TooComplex { .. })
}
