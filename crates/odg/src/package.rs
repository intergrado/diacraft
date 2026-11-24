//! The package: the ZIP archive an `.odg` is, its `mimetype`, its manifest, and every entry by name.
//!
//! [`Package::read`] reads a package whole and either gives every entry of it, files with their bytes as inflated and folders in their
//! place, with the manifest's rows and what the package says it is, or says why not ([`OdgError`]). It knows nothing of the drawing:
//! what a page or a shape is belongs to the drawing's reader, which reads its parts from here.
//!
//! # The order of a read, and why
//!
//! 1. **The archive's directory**, by the container (`diacraft_container::zip`), with its [`Plain`] judge: names compared exactly (no case
//!    folding, `Object 1/` and `object 1/` are two entries, two of one name are `duplicate_part`, `..` is `path_traversal`), folders kept
//!    Every cap of R6, encryption flags and methods other than stored and deflate refused. Nothing is inflated. The manifest's
//!    entry is held to [`Limits::max_manifest_bytes`] by its header and by what it really inflates to.
//! 2. **The manifest**, the only entry inflated before the next step: no manifest is `missing_manifest`; one that is not
//!    well-formed, or whose root is not `manifest:manifest` in the manifest's namespace, is `manifest_malformed`. Names are matched by
//!    namespace, not by prefix.
//! 3. **Encryption**: a `manifest:encryption-data` anywhere in the manifest, or a `manifest:encrypted-key`, refuses the package
//!    as `encrypted_package` **before any other entry is inflated**, since an encrypted entry inflates to noise. Nothing is decrypted.
//! 4. **What the package is**: the manifest's root row (`/`) wins, and `mimetype` is read when the root row says nothing; a
//!    package that says it is anything but a drawing or a drawing template is `not_a_drawing`. A `mimetype` that is absent, not first,
//!    deflated, with an extra field, unreadable or at odds with the root row is read all the same, with one `mimetype_irregular` warning
//!    A package that says nothing at all is read, with a `media_type_unknown` warning: the drawing's reader decides from
//!    its content.
//! 5. **Every other entry inflated**, in the order of the directory, within the container's caps; each checked against its size and
//!    checksum. ([`Package::read_deferred`] inflates only what a drawing's reader reads, and [`Package::read_kept`], which the drawing's reader uses every
//!    file besides, charged to the media budget before each is inflated, a damaged one left out and named.)
//! 6. **What the manifest and the package say of each other**: a file the manifest does not list (`mimetype` and `META-INF/`
//!    need no row) is `manifest_incomplete`, a row for a file or folder the package lacks is `manifest_lists_missing_file`, each named for
//!    the first [`Limits::max_report_entries`] and counted after. Signatures: a file under `META-INF/` whose name holds
//!    `signatures` makes the package [`signed`](Package::signed), and its bytes are kept like any other's.
//! 7. **The trees of `content.xml` and `styles.xml`**, parsed once each, always, within the container's XML limits and
//!    [`Limits::max_xml_bytes`], and kept ([`Package::tree`]), so that the drawing's reader parses neither again. A part
//!    that does not parse is kept as bytes, with a `part_not_scanned` warning, and its error is [`Package::tree_error`]. Macros
//!    are looked for in both trees, to the end: a file under `Basic/` or `Scripts/`, an `office:script` or a
//!    `script:event-listener` anywhere, or an `office:scripts` or `office:event-listeners` holding any element (a
//!    `presentation:event-listener`, and another such container, aside) or text that is not white space, makes the package
//!    [`has_macros`](Package::has_macros). An empty `office:scripts` or `office:event-listeners` holds no code and is no macro. Nothing is
//!    run, and the bytes are kept like any other's.
//!
//! # Writing
//!
//! [`copy_unchanged`] writes a package again with every entry copied as it is, by the container's
//! [`ArchiveWriter::copy`](diacraft_container::zip::ArchiveWriter::copy): what an unedited save is, and what the round trip of the
//! command line checks before the drawing's reader exists. The writer that splices an edit into a package builds on the same copy.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use diacraft_container::xml::{self, El, Node};
use diacraft_container::xml_scan::{AttrSpan, TagKind, scan_tags, tag_attributes};
use diacraft_container::zip::{self as zip, Archive, ArchiveWriter, Entry, Plain, Verdict};
use diacraft_container::{ContainerError, Work, XmlError};

use crate::Variant;
use crate::error::{LimitKind, MimetypeFault, OdgError, Warning};
use crate::limits::{Limits, MAX_MIMETYPE_BYTES};
use crate::ns::{self, Flow, MANIFEST, OFFICE, PRESENTATION, SCRIPT};

/// The manifest's name in the archive.
pub const MANIFEST_PATH: &str = "META-INF/manifest.xml";
/// The `mimetype` entry's name.
pub const MIMETYPE_PATH: &str = "mimetype";
/// A drawing's media type.
pub const DRAWING_MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.graphics";
/// A drawing template's media type.
pub const TEMPLATE_MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.graphics-template";

/// One row of the manifest, a `manifest:file-entry`: its attributes as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// `manifest:full-path`: `/` for the package itself, a file's name as the archive writes it, or a folder's ending in `/`.
    pub full_path: String,
    /// `manifest:media-type`, which may be the empty string; `None` when the row has none (the schema requires one).
    pub media_type: Option<String>,
    /// `manifest:version`, when given.
    pub version: Option<String>,
    /// `manifest:size`, as written, when given.
    pub size: Option<String>,
    /// The row holds a `manifest:encryption-data`.
    pub encrypted: bool,
    /// Every attribute of the row but `manifest:full-path`, as written and in order (names with their prefix, namespace declarations among them): what the model holds
    /// of the row beside its name.
    pub attrs: Vec<(String, String)>,
    /// Where in [`ManifestEntry::attrs`] the row's `manifest:media-type` stands, when it has one.
    pub media_type_at: Option<usize>,
}

/// The manifest of a package: its root's version, and its rows in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    /// `manifest:version` on the root, when given (the writer keeps the one it read).
    pub version: Option<String>,
    /// The rows (`manifest:file-entry`), in the order written, those without a `manifest:full-path` left out.
    pub entries: Vec<ManifestEntry>,
    /// The manifest holds a `manifest:encrypted-key` (keys sent to recipients), or a `manifest:encryption-data` outside a row.
    pub encrypted_keys: bool,
    /// The first row for each full path, by its index in `entries`.
    index: HashMap<String, usize>,
    /// Rows with no `manifest:full-path`, left out of `entries`.
    skipped: usize,
    /// The attributes of the root element, as written and in order (the namespace declarations among them): the model holds them.
    pub root_attrs: Vec<(String, String)>,
    /// The attributes of each row with no `manifest:full-path`, which `entries` leaves out: the model holds them, with the empty name.
    pub unnamed_rows: Vec<Vec<(String, String)>>,
}

impl Manifest {
    /// Whether the manifest says anything of the package is encrypted: a row with `manifest:encryption-data`, or keys for recipients.
    /// [`Package::read`] refuses such a package; [`Manifest::read`] reads its manifest all the same, to say so.
    pub fn encrypted(&self) -> bool {
        self.encrypted_keys || self.entries.iter().any(|e| e.encrypted)
    }

    /// The first row for `full_path`, exactly as written.
    pub fn entry(&self, full_path: &str) -> Option<&ManifestEntry> {
        self.index.get(full_path).and_then(|&i| self.entries.get(i))
    }

    /// The place in [`Manifest::entries`] of the first row for `full_path`, exactly as written.
    pub fn position(&self, full_path: &str) -> Option<usize> {
        self.index.get(full_path).copied()
    }

    /// The root row (`/`), which stands for the package's own document.
    pub fn root(&self) -> Option<&ManifestEntry> {
        self.entry("/")
    }

    /// The manifest of the package in `bytes`, read within `limits`, with nothing else of the package inflated: the archive's directory and
    /// the manifest are read and checked as [`Package::read`] reads them, and an encrypted package is not refused, so that a caller can ask
    /// whether one is ([`Manifest::encrypted`]).
    pub fn read(bytes: &[u8], limits: &Limits) -> Result<Manifest, OdgError> {
        Ok(probe(bytes, limits, &mut Work::default())?.manifest)
    }
}

/// One entry of the archive: a file with its bytes as inflated, or a folder with none. The bytes are shared, so that a picture the reader
/// models is the package's bytes and not a copy of them.
struct Part {
    entry: Entry,
    data: Arc<[u8]>,
    /// Whether the file was inflated: a read for the drawing's reader inflates the files its content names and no others.
    inflated: bool,
}

/// An OpenDocument drawing package, read whole and held in memory. See the [module documentation](self).
pub struct Package {
    parts: Vec<Part>,
    /// Name to index in `parts`, exactly as named.
    index: HashMap<String, usize>,
    manifest: Manifest,
    media_type: Option<String>,
    variant: Option<Variant>,
    mimetype_regular: bool,
    signed: bool,
    has_macros: bool,
    warnings: Vec<Warning>,
    rows: usize,
    /// The trees of [`TREE_PARTS`] the package holds, each parsed once, or why it did not parse.
    trees: Vec<(&'static str, Result<El, XmlError>)>,
    /// Bytes of XML parsed so far by [`Package::parse_tree`], against [`Limits::max_xml_bytes`].
    xml_parsed: u64,
    /// The parts [`Package::parse_tree`] has parsed: each is parsed once.
    parsed: Vec<&'static str>,
    /// What `mimetype` holds, when it is plain text the length a media type has.
    mimetype_text: Option<String>,
    /// Bytes of the files [`Package::read_kept`] inflated beyond those the drawing's reader reads, to be kept in the model.
    kept_bytes: u64,
    /// The files [`Package::read_kept`] left uninflated because they are damaged, by name, in the order of the directory.
    damaged: Vec<String>,
}

/// The parts whose trees a read keeps: the drawing's reader needs both, and parses neither again.
pub const TREE_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

impl Package {
    /// Reads the package in `bytes` within `limits`. Any rule broken, or any limit passed, is an error and there is no half-read package.
    pub fn read(bytes: &[u8], limits: &Limits) -> Result<Package, OdgError> {
        Package::read_counted(bytes, limits, &mut Work::default())
    }

    /// [`Package::read`], adding what the read cost to `work` (whether it reads or is refused), in the units each loop walks: rows,
    /// extra records, names, local headers, bytes inflated, XML events, and name comparisons (those of the archive's duplicate rule and
    /// those of the manifest's folder rows).
    pub fn read_counted(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<Package, OdgError> {
        read_package(bytes, limits, work, true, Inflate::All)
    }

    /// [`Package::read_counted`] with the trees of [`TREE_PARTS`] left unparsed: the drawing's reader parses each with
    /// [`Package::parse_tree`] when it converts it and drops it when done, so that a read holds one tree at a time and never both.
    /// Until both are parsed, [`Package::has_macros`] knows only of the `Basic/` and `Scripts/` folders.
    ///
    /// **It inflates only what a drawing's reader reads** (the census found 40 MB of embedded fonts inflated for nothing):
    /// the manifest, `mimetype`, `content.xml`, `styles.xml`, `meta.xml` and `settings.xml`, and each file an `href` attribute of `content.xml` or
    /// `styles.xml` names (a picture, a replacement image: found by a scan of the two parts' tags, counted in `work.scan_events`, before the
    /// parts are parsed). Any other file is listed, with its row, and not inflated, so it costs no memory and no time, and
    /// [`Package::part`] gives `None` for it. A package whose files are damaged where nothing reads them reads all the same.
    pub fn read_deferred(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<Package, OdgError> {
        read_package(bytes, limits, work, false, Inflate::Referenced)
    }

    /// [`Package::read_deferred`], and **every other file inflated too**, so that the model can keep it (`Document.parts` holds each entry no model field
    /// reads, and the census's 40 MB of embedded fonts are no longer inflated for nothing). The bytes of those files are charged to [`Limits::max_media_bytes`] by the size the
    /// directory states, **before** each is inflated: a file that does not fit is [`OdgError::TooComplex`], and nothing is dropped. **A file that is damaged** (its size or its checksum is
    /// not what the directory says) is left uninflated and named in [`Package::damaged`]: a damaged entry nothing reads does not stop a read, as before. A file that is a zip bomb, or
    /// that the memory cannot hold, is refused as every other.
    pub fn read_kept(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<Package, OdgError> {
        read_package(bytes, limits, work, false, Inflate::Kept)
    }

    /// The text of `mimetype`, when it is plain printable text of the length a media type has.
    pub fn mimetype_text(&self) -> Option<&str> {
        self.mimetype_text.as_deref()
    }

    /// Bytes of the files [`Package::read_kept`] inflated to keep, which no part of the drawing's reader names: what the reader charges its media budget.
    pub fn kept_bytes(&self) -> u64 {
        self.kept_bytes
    }

    /// The files [`Package::read_kept`] could not inflate because they are damaged, by name.
    pub fn damaged(&self) -> &[String] {
        &self.damaged
    }

    /// Whether the entry named `name` has been inflated (a folder always has).
    pub fn is_inflated(&self, name: &str) -> bool {
        self.index.get(name).and_then(|&i| self.parts.get(i)).is_some_and(|p| p.inflated)
    }

    /// Parses the tree of `name`, one of [`TREE_PARTS`], once, within the container's XML limits and [`Limits::max_xml_bytes`] (summed over
    /// the parts this package has parsed), looks it through for macros as [`Package::read`] does, and gives it: the package keeps no copy,
    /// and lets the part's bytes go ([`Package::part`] then gives it as empty). `None` when the package holds no such part, the name is not
    /// one of [`TREE_PARTS`], or it was parsed before (a tree [`Package::read`] kept is [`Package::take_tree`]'s). A part that does not
    /// parse gives its error, with the XML class.
    pub fn parse_tree(&mut self, name: &str, limits: &Limits, work: &mut Work) -> Option<Result<El, OdgError>> {
        let name = TREE_PARTS.iter().copied().find(|n| *n == name)?;
        if self.parsed.contains(&name) || self.trees.iter().any(|(n, _)| *n == name) {
            return None;
        }
        let size = self.part(name)?.len() as u64;
        self.parsed.push(name);
        self.xml_parsed = self.xml_parsed.saturating_add(size);
        if self.xml_parsed > limits.max_xml_bytes {
            return Some(Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes }));
        }
        let tree = xml::parse_counted(self.part(name)?, &limits.container.xml, work);
        if let Ok(root) = &tree {
            self.has_macros |= holds_macro(root);
        }
        // The tree holds what a reader needs of the part, so its bytes go now: a hostile file's largest part is not held twice.
        if let Some(part) = self.index.get(name).and_then(|&i| self.parts.get_mut(i)) {
            part.data = Arc::from(&[][..]);
        }
        Some(tree.map_err(|error| OdgError::Xml { part: name.to_owned(), error }))
    }

    /// The bytes of the file named exactly `name`, as inflated, or `None` when there is no such file (a folder is not a file), or when the read
    /// left the file uninflated because nothing in the drawing names it ([`Package::read_deferred`]). [`Package::read`]
    /// inflates every file.
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        let part = self.parts.get(*self.index.get(name)?)?;
        if part.entry.folder || !part.inflated { None } else { Some(&part.data[..]) }
    }

    /// The bytes of the file named exactly `name`, shared rather than copied: what a picture the reader models holds.
    pub fn part_shared(&self, name: &str) -> Option<Arc<[u8]>> {
        let part = self.parts.get(*self.index.get(name)?)?;
        if part.entry.folder || !part.inflated { None } else { Some(Arc::clone(&part.data)) }
    }

    /// The archive's row for the entry named exactly `name`, file or folder (a folder's name ends in `/`).
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.parts.get(*self.index.get(name)?).map(|p| &p.entry)
    }

    /// Every entry, files and folders, in the order of the archive's directory.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.parts.iter().map(|p| &p.entry)
    }

    /// The names of the files, in the order of the archive's directory.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter(|p| !p.entry.folder).map(|p| p.entry.name.as_str())
    }

    /// The names of the folders (`Configurations2/`), in the order of the archive's directory.
    pub fn folders(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter(|p| p.entry.folder).map(|p| p.entry.name.as_str())
    }

    /// The manifest's rows.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// What the package says it is: the manifest's root row, else `mimetype`; `None` when neither says.
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    /// A drawing or a drawing template, by [`Package::media_type`]; `None` when the package does not say (a package that says it is
    /// something else is refused).
    pub fn variant(&self) -> Option<Variant> {
        self.variant
    }

    /// Whether `mimetype` is what R2 asks: first, stored, with no extra field, and saying what the manifest says.
    pub fn mimetype_regular(&self) -> bool {
        self.mimetype_regular
    }

    /// Whether the package holds a signature file (a file under `META-INF/` whose name holds `signatures`). Nothing is verified.
    pub fn signed(&self) -> bool {
        self.signed
    }

    /// Whether the package holds macros or scripts: see the [module documentation](self). Nothing is run.
    pub fn has_macros(&self) -> bool {
        self.has_macros
    }

    /// What was odd about the package and was read anyway.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Rows of the archive's central directory: files and folders.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The tree of `name`, one of [`TREE_PARTS`], as the read parsed it (once), when the package holds that part and it parsed.
    pub fn tree(&self, name: &str) -> Option<&El> {
        self.trees.iter().find(|(n, _)| *n == name).and_then(|(_, tree)| tree.as_ref().ok())
    }

    /// Why the part `name`, one of [`TREE_PARTS`], did not parse, when the package holds it and it did not: what the drawing's reader
    /// refuses the drawing with, without parsing it again.
    pub fn tree_error(&self, name: &str) -> Option<&XmlError> {
        self.trees.iter().find(|(n, _)| *n == name).and_then(|(_, tree)| tree.as_ref().err())
    }

    /// Takes the tree of `name`, one of [`TREE_PARTS`], out of the package: the tree as the read parsed it, or why it did not parse, and
    /// `None` when the package holds no such part or it was taken before. A caller that converts a tree takes it and drops it when done, so
    /// that it holds no tree it has finished with; the part's bytes stay ([`Package::part`]). [`Package::tree`] and
    /// [`Package::tree_error`] then answer `None` for it. (The drawing's reader goes further and parses each tree only when it needs it:
    /// [`Package::read_deferred`].)
    pub fn take_tree(&mut self, name: &str) -> Option<Result<El, XmlError>> {
        let at = self.trees.iter().position(|(n, _)| *n == name)?;
        Some(self.trees.swap_remove(at).1)
    }
}

impl fmt::Debug for Package {
    /// The entry names and sizes, not their bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<(&str, usize)> = self.parts.iter().map(|p| (p.entry.name.as_str(), p.data.len())).collect();
        f.debug_struct("Package")
            .field("parts", &parts)
            .field("media_type", &self.media_type)
            .field("signed", &self.signed)
            .field("has_macros", &self.has_macros)
            .field("warnings", &self.warnings)
            .field("trees", &self.trees.iter().map(|(name, tree)| (*name, tree.is_ok())).collect::<Vec<_>>())
            .finish()
    }
}

/// The container's plain judge, with the manifest's entry held to the manifest's cap.
struct OdfJudge {
    plain: Plain,
    manifest_cap: u64,
}

impl zip::Judge for OdfJudge {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, kept: &[Entry]) -> Result<Verdict, ContainerError> {
        match self.plain.entry(entry, kept)? {
            Verdict::Keep { rename, cap, counted } if !entry.folder && entry.name == MANIFEST_PATH => {
                Ok(Verdict::Keep { rename, cap: cap.min(self.manifest_cap), counted })
            }
            verdict => Ok(verdict),
        }
    }

    fn listed(&mut self, kept: &[Entry], work: &mut Work) -> Result<(), ContainerError> {
        self.plain.listed(kept, work)
    }
}

/// The archive in `bytes`, read with the package's judge.
fn read_archive<'a>(bytes: &'a [u8], limits: &Limits, work: &mut Work) -> Result<Archive<'a>, OdgError> {
    let mut judge = OdfJudge { plain: Plain::new(&limits.container), manifest_cap: limits.max_manifest_bytes };
    Ok(Archive::read_with(bytes, &limits.container, &mut judge, work)?)
}

/// What steps 1 and 2 give: the archive, its manifest, the manifest's bytes and where its entry is.
struct Probe<'a> {
    archive: Archive<'a>,
    manifest: Manifest,
    manifest_bytes: Vec<u8>,
    manifest_at: usize,
}

/// Steps 1 and 2: the archive's directory and the manifest, nothing else inflated, nothing refused for encryption.
fn probe<'a>(bytes: &'a [u8], limits: &Limits, work: &mut Work) -> Result<Probe<'a>, OdgError> {
    let mut archive = read_archive(bytes, limits, work)?;
    let manifest_at = archive.entries().iter().position(|e| !e.folder && e.name == MANIFEST_PATH).ok_or(OdgError::MissingManifest)?;
    let manifest_bytes = archive.inflate(manifest_at, work)?;
    let manifest = parse_manifest(&manifest_bytes, limits, work)?;
    Ok(Probe { archive, manifest, manifest_bytes, manifest_at })
}

/// The manifest's tree, read. The manifest alone may carry a DOCTYPE of an external id and no internal subset, which old
/// writers put there and which is read and ignored; every other DTD, here and in every other part, is refused.
fn parse_manifest(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<Manifest, OdgError> {
    let options = xml::ParseOptions { allow_external_doctype: true };
    let root = match xml::parse_with_options(bytes, &limits.manifest_xml(), options, work) {
        Ok(root) => root,
        Err(XmlError::Malformed { offset, message }) => {
            return Err(OdgError::ManifestMalformed { detail: format!("not well-formed at byte {offset}: {message}") });
        }
        Err(error) => return Err(OdgError::Xml { part: MANIFEST_PATH.to_owned(), error }),
    };
    let mut manifest = Manifest::default();
    let mut root_ok = false;
    // The row being read (the last element at depth 1, when it is a `manifest:file-entry`), and whether the walk passed the row cap.
    let mut row: Option<usize> = None;
    let mut over = false;
    let max_rows = limits.max_manifest_entries;
    ns::walk(&root, |el, depth, scopes| {
        let (uri, local) = scopes.element(&el.name);
        let in_manifest = uri == Some(MANIFEST);
        if depth == 0 {
            root_ok = in_manifest && local == "manifest";
            if !root_ok {
                return Flow::Stop;
            }
            manifest.version = scopes.attr(el, MANIFEST, "version").map(str::to_owned);
            manifest.root_attrs = el.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            return Flow::Into;
        }
        if depth == 1 {
            row = None;
            if in_manifest && local == "file-entry" {
                // The rows with no name are held too (the model keeps them), so they count against the cap with the rest.
                if manifest.entries.len().saturating_add(manifest.unnamed_rows.len()) >= max_rows {
                    over = true;
                    return Flow::Stop;
                }
                let attr = |name: &str| scopes.attr(el, MANIFEST, name).map(str::to_owned);
                // The attributes besides the name, as written: the media type's place among them is found by its namespace and not by its prefix.
                let is = |key: &str, local: &str| !ns::is_declaration(key) && scopes.attribute(key) == (Some(MANIFEST), local);
                let mut attrs: Vec<(String, String)> = Vec::with_capacity(el.attrs.len());
                let (mut named, mut media_type_at) = (false, None);
                for (key, value) in &el.attrs {
                    if !named && is(key, "full-path") {
                        named = true;
                        continue;
                    }
                    if media_type_at.is_none() && is(key, "media-type") {
                        media_type_at = Some(attrs.len());
                    }
                    attrs.push((key.clone(), value.clone()));
                }
                if let Some(full_path) = attr("full-path") {
                    manifest.entries.push(ManifestEntry {
                        full_path,
                        media_type: attr("media-type"),
                        version: attr("version"),
                        size: attr("size"),
                        encrypted: false,
                        attrs,
                        media_type_at,
                    });
                    row = Some(manifest.entries.len() - 1);
                } else {
                    manifest.skipped = manifest.skipped.saturating_add(1);
                    manifest.unnamed_rows.push(attrs);
                }
            }
        }
        if in_manifest && local == "encryption-data" {
            match row.and_then(|i| manifest.entries.get_mut(i)) {
                Some(entry) if depth == 2 => entry.encrypted = true,
                _ => manifest.encrypted_keys = true,
            }
        }
        if in_manifest && local == "encrypted-key" {
            manifest.encrypted_keys = true;
        }
        Flow::Into
    });
    if !root_ok {
        return Err(OdgError::ManifestMalformed {
            detail: format!("the root is <{}>, not manifest:manifest in the manifest's namespace", root.name),
        });
    }
    if over {
        return Err(OdgError::Limit { kind: LimitKind::ManifestEntries, max: u64::try_from(max_rows).unwrap_or(u64::MAX) });
    }
    for (i, entry) in manifest.entries.iter().enumerate() {
        manifest.index.entry(entry.full_path.clone()).or_insert(i);
    }
    Ok(manifest)
}

/// What step 4 found of `mimetype` and the media type.
struct Media {
    media_type: Option<String>,
    faults: Vec<MimetypeFault>,
    /// The index of the `mimetype` entry and its bytes, when it was read.
    mimetype: Option<(usize, Vec<u8>)>,
    mimetype_present: bool,
    /// What `mimetype` holds when it is plain printable text of the length a media type has: the text the model holds.
    text: Option<String>,
}

/// Step 4: `mimetype`, and what the package says it is.
fn media(archive: &mut Archive<'_>, manifest: &Manifest, work: &mut Work) -> Result<Media, OdgError> {
    let mut faults = Vec::new();
    let mut value = None;
    let mut read = None;
    let at = archive.entries().iter().position(|e| !e.folder && e.name == MIMETYPE_PATH);
    match at.and_then(|i| archive.entries().get(i).cloned().map(|e| (i, e))) {
        None => faults.push(MimetypeFault::Absent),
        Some((i, entry)) => {
            if entry.local_offset != 0 {
                faults.push(MimetypeFault::NotFirst);
            }
            if entry.compression() != Some(zip::Method::Stored) {
                faults.push(MimetypeFault::Compressed);
            }
            if entry.local_extra_len != 0 {
                faults.push(MimetypeFault::ExtraField);
            }
            if entry.size <= MAX_MIMETYPE_BYTES {
                let bytes = archive.inflate(i, work)?;
                if !bytes.is_empty() && bytes.iter().all(|b| (0x21..=0x7E).contains(b)) {
                    value = Some(String::from_utf8_lossy(&bytes).into_owned());
                } else {
                    faults.push(MimetypeFault::Unreadable);
                }
                read = Some((i, bytes));
            } else {
                faults.push(MimetypeFault::Unreadable);
            }
        }
    }
    let text = value.clone();
    let root = manifest.root().and_then(|r| r.media_type.clone()).filter(|t| !t.is_empty());
    let media_type = match (root, value) {
        (Some(root), Some(value)) => {
            if root != value {
                faults.push(MimetypeFault::DisagreesWithManifest);
            }
            Some(root)
        }
        (Some(root), None) => Some(root),
        (None, value) => value,
    };
    Ok(Media { media_type, faults, mimetype: read, mimetype_present: at.is_some(), text })
}

/// What the package says it is, and the variant: refused when it says it is something other than a drawing.
fn variant_of(media_type: Option<&str>) -> Result<Option<Variant>, OdgError> {
    match media_type {
        None => Ok(None),
        Some(t) => match Variant::from_media_type(t) {
            Some(variant) => Ok(Some(variant)),
            None => Err(OdgError::NotADrawing { media_type: t.chars().take(64).collect() }),
        },
    }
}

/// The media type of the package in `bytes` (steps 1, 2 and 4), for [`crate::sniff`]: an encrypted package is not refused, since its
/// manifest says what it is.
pub(crate) fn sniff_media(bytes: &[u8], limits: &Limits) -> Result<Option<String>, OdgError> {
    let mut work = Work::default();
    let mut probe = probe(bytes, limits, &mut work)?;
    Ok(media(&mut probe.archive, &probe.manifest, &mut work)?.media_type)
}

/// Warnings of one kind, the first `most` named and the rest counted.
struct Capped {
    named: Vec<Warning>,
    more: usize,
    most: usize,
}

impl Capped {
    fn new(most: usize) -> Capped {
        Capped { named: Vec::new(), more: 0, most }
    }

    fn push(&mut self, warning: impl FnOnce() -> Warning) {
        if self.named.len() < self.most {
            self.named.push(warning());
        } else {
            self.more = self.more.saturating_add(1);
        }
    }
}

/// The whole read (steps 1 to 7); `trees` says whether the trees of [`TREE_PARTS`] are parsed and kept (a read for the drawing's reader) or
/// not (a comparison or a copy, which parse what they need themselves).
pub(crate) fn read_package(bytes: &[u8], limits: &Limits, work: &mut Work, trees: bool, inflate: Inflate) -> Result<Package, OdgError> {
    let Probe { mut archive, manifest, manifest_bytes, manifest_at } = probe(bytes, limits, work)?;
    if manifest.encrypted() {
        return Err(OdgError::EncryptedPackage);
    }
    let media = media(&mut archive, &manifest, work)?;
    let variant = variant_of(media.media_type.as_deref())?;
    let mut warnings = Vec::new();
    if archive.bytes_after_end_record() > 0 {
        warnings.push(Warning::BytesAfterEndRecord { count: archive.bytes_after_end_record() });
    }
    let mimetype_regular = media.faults.is_empty();
    if !mimetype_regular {
        warnings.push(Warning::MimetypeIrregular { faults: media.faults.clone() });
    }
    if media.mimetype_present && manifest.root().is_none() {
        warnings.push(Warning::ManifestNoRootEntry);
    }
    if media.media_type.is_none() {
        warnings.push(Warning::MediaTypeUnknown);
    }
    if manifest.skipped > 0 {
        warnings.push(Warning::ManifestEntryIncomplete { count: manifest.skipped });
    }
    // Step 5: every other entry, in the order of the directory, or those a drawing's reader reads.
    let rows = archive.rows();
    let mut index = HashMap::with_capacity(archive.entries().len());
    for (i, entry) in archive.entries().iter().enumerate() {
        index.entry(entry.name.clone()).or_insert(i);
    }
    let (data, kept) = inflate_parts(&mut archive, (manifest_at, manifest_bytes, media.mimetype), &index, inflate, limits, work)?;
    let entries = archive.into_entries();
    let damaged: Vec<String> = kept.damaged.iter().filter_map(|&i| entries.get(i)).map(|e| e.name.clone()).collect();
    let parts: Vec<Part> = entries
        .into_iter()
        .zip(data)
        .map(|(entry, data)| Part { inflated: data.is_some() || entry.folder, entry, data: Arc::from(data.unwrap_or_default()) })
        .collect();
    // Step 6.
    let (incomplete, missing) = cross_check(&parts, &index, &manifest, limits, work);
    for (capped, more) in [(incomplete, Kind::Incomplete), (missing, Kind::Missing)] {
        warnings.extend(capped.named);
        if capped.more > 0 {
            warnings.push(match more {
                Kind::Incomplete => Warning::MoreManifestIncomplete { count: capped.more },
                Kind::Missing => Warning::MoreManifestListsMissingFile { count: capped.more },
            });
        }
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut duplicates = Capped::new(limits.max_report_entries);
    for entry in &manifest.entries {
        if !seen.insert(entry.full_path.as_str()) {
            duplicates.push(|| Warning::ManifestDuplicateEntry { path: entry.full_path.clone() });
        }
    }
    warnings.extend(duplicates.named);
    if duplicates.more > 0 {
        warnings.push(Warning::MoreManifestDuplicateEntries { count: duplicates.more });
    }
    let files = || parts.iter().filter(|p| !p.entry.folder).map(|p| p.entry.name.as_str());
    let signed = files().any(|name| name.strip_prefix("META-INF/").is_some_and(|rest| rest.contains("signatures")));
    let mut has_macros = files().any(|name| name.starts_with("Basic/") || name.starts_with("Scripts/"));
    // Step 7: both trees, each parsed once and kept, and both looked through to the end, whatever was found before.
    let mut kept_trees = Vec::new();
    if trees {
        let mut parsed = 0u64;
        for name in TREE_PARTS {
            let Some(part) = index.get(name).and_then(|&i| parts.get(i)).filter(|p| !p.entry.folder) else { continue };
            parsed = parsed.saturating_add(part.data.len() as u64);
            if parsed > limits.max_xml_bytes {
                return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
            }
            let tree = xml::parse_counted(&part.data, &limits.container.xml, work);
            match &tree {
                Ok(root) => has_macros |= holds_macro(root),
                Err(error) => warnings.push(Warning::PartNotScanned { part: name.to_owned(), class: error.class() }),
            }
            kept_trees.push((name, tree));
        }
    }
    Ok(Package {
        parts,
        index,
        manifest,
        media_type: media.media_type,
        variant,
        mimetype_regular,
        signed,
        has_macros,
        warnings,
        rows,
        trees: kept_trees,
        xml_parsed: 0,
        parsed: Vec::new(),
        mimetype_text: media.text,
        kept_bytes: kept.bytes,
        damaged,
    })
}

/// Which files a read inflates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inflate {
    /// Every file: what a comparison needs, which compares every part's bytes.
    All,
    /// The files a drawing's reader reads: the structure, the four XML parts, and each file the content names by an `href` attribute.
    Referenced,
    /// The files [`Inflate::Referenced`] reads, and every other file besides, for the model to keep: charged to the media budget before it is inflated, and a
    /// damaged one left out and named.
    Kept,
}

/// What the files inflated to be kept came to: their bytes, and the entries left out as damaged.
#[derive(Default)]
struct KeptFiles {
    bytes: u64,
    damaged: Vec<usize>,
}

/// The files a drawing's reader reads whatever the drawing holds.
const ALWAYS_READ: [&str; 5] = ["content.xml", "styles.xml", "meta.xml", "settings.xml", crate::ext::docfile::PATH];

/// What the inflating gave: the bytes of each entry, `None` for a file left uninflated, and what the files inflated to be kept came to.
type Inflated = (Vec<Option<Vec<u8>>>, KeptFiles);

/// What the probe inflated: the manifest's entry, its bytes, and the `mimetype` entry with its bytes when it was read.
type Probed = (usize, Vec<u8>, Option<(usize, Vec<u8>)>);

/// The bytes of each entry, in the order of the archive's entries: `None` for a file left uninflated. The manifest and `mimetype` were
/// inflated by the probe and are given back. Under [`Inflate::Referenced`] the structure and the four XML parts come first, and then each file
/// those parts name.
fn inflate_parts(
    archive: &mut Archive<'_>,
    probed: Probed,
    index: &HashMap<String, usize>,
    inflate: Inflate,
    limits: &Limits,
    work: &mut Work,
) -> Result<Inflated, OdgError> {
    let (manifest_at, manifest_bytes, mimetype) = probed;
    let (mut manifest_bytes, mut mimetype) = (Some(manifest_bytes), mimetype);
    let threads = read_threads(limits);
    let now: Vec<bool> = archive.entries().iter().map(|e| inflate == Inflate::All || e.folder || ALWAYS_READ.contains(&e.name.as_str())).collect();
    let mut data: Vec<Option<Vec<u8>>> = Vec::with_capacity(now.len());
    // The entries inflated now, in the order of the directory: on threads when the limits say so (the same bytes, counts and first error as one at a time: `Archive::inflate_many`).
    let mut wanted: Vec<usize> = Vec::new();
    for (i, now) in now.into_iter().enumerate() {
        data.push(if i == manifest_at {
            Some(manifest_bytes.take().unwrap_or_default())
        } else if mimetype.as_ref().is_some_and(|(at, _)| *at == i) {
            mimetype.take().map(|(_, bytes)| bytes)
        } else {
            if now {
                wanted.push(i);
            }
            None
        });
    }
    place(&mut data, &wanted, archive.inflate_many(&wanted, threads, work)?);
    if matches!(inflate, Inflate::Referenced | Inflate::Kept) {
        let named: Vec<usize> = referenced(&data, index, limits, work).into_iter().filter(|&i| data.get(i).is_some_and(Option::is_none)).collect();
        place(&mut data, &named, archive.inflate_many(&named, threads, work)?);
    }
    let mut kept = KeptFiles::default();
    if inflate == Inflate::Kept {
        inflate_kept(archive, &mut data, limits, threads, work, &mut kept)?;
    }
    Ok((data, kept))
}

/// The threads a read of this package may use: as many as `limits` ask and the address space has room for beside the memory a read may hold (`diacraft_container::spare`); none on the web.
fn read_threads(limits: &Limits) -> usize {
    if cfg!(target_arch = "wasm32") {
        1
    } else {
        diacraft_container::spare::threads_beside(limits.threads, diacraft_container::spare::READ_ROOM_BYTES)
    }
}

/// Puts the bytes of the entries at `at` in their places in `data`.
fn place(data: &mut [Option<Vec<u8>>], at: &[usize], bytes: Vec<Vec<u8>>) {
    for (&i, bytes) in at.iter().zip(bytes) {
        if let Some(slot) = data.get_mut(i) {
            *slot = Some(bytes);
        }
    }
}

/// The files still uninflated, inflated to be kept: each charged to [`Limits::max_media_bytes`] by the size the directory states **before** it is inflated, so
/// that a file past the budget is refused with nothing inflated for it, and a damaged one (its size or checksum is not what the directory says) is left out and named in `kept`.
/// Any other failure is the read's.
fn inflate_kept(
    archive: &mut Archive<'_>,
    data: &mut [Option<Vec<u8>>],
    limits: &Limits,
    threads: usize,
    work: &mut Work,
    kept: &mut KeptFiles,
) -> Result<(), OdgError> {
    let mut room = limits.max_media_bytes;
    let mut start = 0usize;
    if threads > 1 {
        // The files that the budget lets in whatever happens to the ones before them (the budget is only ever given back, so a file that fits when none is) are inflated together, on threads, in
        // the order of the directory; the rest are read one at a time below, with what the damaged ones gave back.
        let mut wanted: Vec<usize> = Vec::new();
        let mut left = room;
        start = data.len();
        for (i, slot) in data.iter().enumerate() {
            let Some(entry) = archive.entries().get(i) else { break };
            if slot.is_some() || entry.folder {
                continue;
            }
            match left.checked_sub(entry.size) {
                Some(rest) => {
                    left = rest;
                    wanted.push(i);
                }
                None => {
                    start = i;
                    break;
                }
            }
        }
        if wanted.len() >= 2 {
            let damaged = |e: &ContainerError| matches!(e, ContainerError::ZipCorrupt { .. });
            let results = archive.inflate_each(&wanted, threads, &damaged, work);
            for (&i, result) in wanted.iter().zip(results) {
                let size = archive.entries().get(i).map_or(0, |e| e.size);
                room = room.checked_sub(size).ok_or(OdgError::TooComplex { what: "bytes of package entries kept", max: limits.max_media_bytes })?;
                match result {
                    Ok(bytes) => {
                        kept.bytes = kept.bytes.saturating_add(size);
                        if let Some(slot) = data.get_mut(i) {
                            *slot = Some(bytes);
                        }
                    }
                    Err(ContainerError::ZipCorrupt { .. }) => {
                        room = room.saturating_add(size);
                        kept.damaged.push(i);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        } else {
            start = 0;
        }
    }
    for (i, slot) in data.iter_mut().enumerate().skip(start) {
        let Some(entry) = archive.entries().get(i) else { break };
        if slot.is_some() || entry.folder {
            continue;
        }
        let size = entry.size;
        room = room.checked_sub(size).ok_or(OdgError::TooComplex { what: "bytes of package entries kept", max: limits.max_media_bytes })?;
        match archive.inflate(i, work) {
            Ok(bytes) => {
                kept.bytes = kept.bytes.saturating_add(size);
                *slot = Some(bytes);
            }
            Err(ContainerError::ZipCorrupt { .. }) => {
                // Nothing reads the file and the model cannot keep damage: it is named, and the budget is given back.
                room = room.saturating_add(size);
                kept.damaged.push(i);
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// The entries named by an `href` attribute (any prefix) of a `draw:image` or a `draw:fill-image` (any prefix) in `content.xml` or `styles.xml`, in the order of the
/// entries, each once: a superset of what the reader reads, which is the pictures its frames show and the pictures its fill-image definitions hold, and
/// nothing else it names (the reader reads an `xlink:href` of no other element). Found by one scan of the two parts' tags (counted in `work.scan_events`). A part that does not scan names
/// nothing: its tree is refused by the reader anyway.
///
/// **An embedded font is not read**: ODF names one by `svg:font-face-uri xlink:href`, and a font of megabytes named so is left in the package,
/// listed and not inflated (the first version took every `href`, and inflated the fonts it was written to leave).
fn referenced(data: &[Option<Vec<u8>>], index: &HashMap<String, usize>, limits: &Limits, work: &mut Work) -> Vec<usize> {
    let mut found = vec![false; data.len()];
    let mut spans: Vec<AttrSpan> = Vec::new();
    for name in TREE_PARTS {
        let Some(bytes) = index.get(name).and_then(|&i| data.get(i)).and_then(Option::as_deref) else { continue };
        let mut scan = Work::default();
        // A part that does not scan is refused by the reader when it parses it, with the class that says why: the scan's own error is not ours.
        let _scanned = scan_tags::<()>(bytes, &limits.container.xml, &mut scan, |tag| {
            let image = matches!(tag.name.rsplit(':').next(), Some("image" | "fill-image"));
            if tag.kind != TagKind::Close && image && tag_attributes(bytes, tag.start, tag.end, spans_cleared(&mut spans)).is_some() {
                for a in &spans {
                    let key = bytes.get(a.name.0..a.name.1).unwrap_or_default();
                    if key.rsplit(|b| *b == b':').next() == Some(b"href") {
                        let value = unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default());
                        if let Some(slot) = href_entry(&value, index).and_then(|i| found.get_mut(i)) {
                            *slot = true;
                        }
                    }
                }
            }
            Ok(())
        });
        work.scan_events = work.scan_events.saturating_add(scan.xml_events);
    }
    found.into_iter().enumerate().filter(|(_, wanted)| *wanted).map(|(i, _)| i).collect()
}

/// `spans`, emptied, for the next tag.
fn spans_cleared(spans: &mut Vec<AttrSpan>) -> &mut Vec<AttrSpan> {
    spans.clear();
    spans
}

/// An attribute value's text: the five predefined entities and the numeric references resolved; any other `&` is kept as written.
pub(crate) fn unescape(raw: &[u8]) -> String {
    unescape_counted(raw, &mut 0)
}

/// [`unescape`], adding to `steps` the bytes it looked at after the `&`s it found: the digits of a numeric reference and the characters of a
/// name it compared, each once. **The work is the value's length and no more**: a reference of any length is read as the XML parser reads it
/// (`&#0000000047;` is `/`, the digits are the length of the reference and nothing caps them), the scan of a reference stops at the first byte
/// that is no part of one, so a value of many `&` costs one step each, and the value is bounded by the part it is in, which the scan that found
/// it charged (the manifest's bytes by the writer's `charge_scan`, the content's by the tag scan's events).
pub(crate) fn unescape_counted(raw: &[u8], steps: &mut u64) -> String {
    let text = String::from_utf8_lossy(raw);
    if !text.contains('&') {
        return text.into_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest: &str = &text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        rest = rest.get(at..).unwrap_or_default();
        match reference_at(rest, steps) {
            Some((c, used)) => {
                out.push(c);
                rest = rest.get(used..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = rest.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character the reference at the start of `rest` (which starts with `&`) stands for, and how many bytes it takes, `&` and `;` included; `None`
/// when what follows the `&` is no reference. A predefined entity is compared by its fixed name; a numeric one is read by its digits, which stop it
/// at the first byte that is not one, and a value past the characters there are is no character.
fn reference_at(rest: &str, steps: &mut u64) -> Option<(char, usize)> {
    let body = rest.get(1..)?;
    for (name, c) in [("amp;", '&'), ("lt;", '<'), ("gt;", '>'), ("quot;", '"'), ("apos;", '\'')] {
        *steps = steps.saturating_add(1);
        if body.starts_with(name) {
            return Some((c, 1 + name.len()));
        }
    }
    let number = body.strip_prefix('#')?;
    let (digits, radix, prefix) = match number.strip_prefix('x') {
        Some(hex) => (hex, 16, 3),
        None => (number, 10, 2),
    };
    let (mut value, mut count) = (0u32, 0usize);
    for b in digits.bytes() {
        let Some(digit) = char::from(b).to_digit(radix) else { break };
        // Saturating: a value that has passed the characters stays past them, however many digits follow.
        value = value.saturating_mul(radix).saturating_add(digit);
        count += 1;
    }
    *steps = steps.saturating_add(count as u64 + 1);
    if count == 0 || digits.as_bytes().get(count) != Some(&b';') {
        return None;
    }
    Some((char::from_u32(value)?, prefix + count + 1))
}

/// The entry an `href` names: percent-decoded with a leading `./` stripped, matched exactly; else as written. Folders
/// are entries too, named with their slash.
pub(crate) fn href_entry(href: &str, index: &HashMap<String, usize>) -> Option<usize> {
    let decoded = percent_decode(href);
    let stripped = decoded.strip_prefix("./").unwrap_or(&decoded);
    if let Some(&i) = index.get(stripped) {
        return Some(i);
    }
    index.get(href.strip_prefix("./").unwrap_or(href)).copied()
}

/// `%XX` sequences decoded, as UTF-8; text that does not decode is given back as written.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let Some(hex) = bytes.get(i + 1..i + 3)
            && let Ok(s) = std::str::from_utf8(hex)
            && let Ok(v) = u8::from_str_radix(s, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Which list a capped warning goes to.
enum Kind {
    Incomplete,
    Missing,
}

/// Step 6: the files the manifest does not list, and the rows for what the package lacks. One pass over each, a set lookup for each
/// file, and a binary search of the sorted names for each folder row: each comparison of two names is counted in `work.name_comparisons`.
fn cross_check(parts: &[Part], index: &HashMap<String, usize>, manifest: &Manifest, limits: &Limits, work: &mut Work) -> (Capped, Capped) {
    let mut incomplete = Capped::new(limits.max_report_entries);
    for part in parts.iter().filter(|p| !p.entry.folder) {
        let name = part.entry.name.as_str();
        if name == MIMETYPE_PATH || name.starts_with("META-INF/") {
            continue;
        }
        if manifest.entry(name).is_none() {
            incomplete.push(|| Warning::ManifestIncomplete { name: name.to_owned() });
        }
    }
    let mut sorted: Vec<&str> = parts.iter().map(|p| p.entry.name.as_str()).collect();
    let mut comparisons = 0u64;
    sorted.sort_unstable_by(|a, b| {
        comparisons = comparisons.saturating_add(1);
        a.cmp(b)
    });
    // Whether any entry's name starts with `folder` (which ends in `/`): the first name not less than it is the one that would.
    let mut under = |folder: &str| {
        let at = sorted.partition_point(|name| {
            comparisons = comparisons.saturating_add(1);
            *name < folder
        });
        sorted.get(at).is_some_and(|name| name.starts_with(folder))
    };
    let mut missing = Capped::new(limits.max_report_entries);
    let mut checked: HashSet<&str> = HashSet::new();
    for entry in &manifest.entries {
        let path = entry.full_path.as_str();
        if path == "/" || !checked.insert(path) {
            continue;
        }
        let present = if path.ends_with('/') {
            index.contains_key(path) || under(path)
        } else {
            index.get(path).and_then(|&i| parts.get(i)).is_some_and(|p| !p.entry.folder) || {
                // A folder's row written without its slash.
                let folder = format!("{path}/");
                index.contains_key(&folder) || under(&folder)
            }
        };
        if !present {
            missing.push(|| Warning::ManifestListsMissingFile { path: path.to_owned() });
        }
    }
    work.name_comparisons = work.name_comparisons.saturating_add(comparisons);
    (incomplete, missing)
}

/// Whether a tree holds code or binds an event to it: an `office:script` or a `script:event-listener` anywhere;
/// or, directly in an `office:scripts` or `office:event-listeners`, an element that is neither another of those nor a
/// `presentation:event-listener` (which binds an event to an action of the presentation vocabulary), or text that is not white space. **An
/// `office:script` that only declares empty Basic libraries holds no code** (its only content is an `ooo:libraries`, in the namespace
/// `http://openoffice.org/2004/office`, holding nothing but empty `ooo:library-embedded` elements), whatever its attributes, and is no macro; one
/// that holds a module, source, an `ooo:library-linked`, any other element or text is. The tree is walked to its end, once.
pub(crate) fn holds_macro(root: &El) -> bool {
    let mut found = false;
    // Whether the last element walked at each depth is an `office:scripts` or an `office:event-listeners`: the walk is in document order,
    // so the one at the depth above an element is its parent.
    let mut containers: Vec<bool> = Vec::new();
    ns::walk(root, |el, depth, scopes| {
        containers.truncate(depth);
        let (uri, local) = scopes.element(&el.name);
        let container = uri == Some(OFFICE) && matches!(local, "scripts" | "event-listeners");
        let script = uri == Some(OFFICE) && local == "script";
        let declares_nothing = script && declares_only_empty_libraries(el, scopes);
        let code = (script && !declares_nothing) || (uri == Some(SCRIPT) && local == "event-listener");
        let presentation = uri == Some(PRESENTATION) && local == "event-listener";
        let in_container = depth.checked_sub(1).and_then(|above| containers.get(above)).copied().unwrap_or(false);
        let text = container && el.children.iter().any(|n| matches!(n, Node::Text(t) | Node::CData(t) if !is_blank(t)));
        found |= code || text || (in_container && !container && !presentation && !declares_nothing);
        containers.push(container);
        Flow::Into
    });
    found
}

/// The namespace of the Basic libraries' declarations (`ooo:libraries`, `ooo:library-embedded`).
const OOO: &str = "http://openoffice.org/2004/office";

/// Whether an `office:script` only declares empty Basic libraries: its children are `ooo:libraries` elements, each holding only empty
/// `ooo:library-embedded` elements (no child element, no text but white space), and it holds no text but white space. The names are matched by
/// namespace, with the bindings the walk holds at the `office:script` and those the elements below declare on themselves.
fn declares_only_empty_libraries(script: &El, scopes: &ns::Scopes<'_>) -> bool {
    // No text but white space: comments say nothing, and anything else (text, CDATA, an instruction) is content.
    let blank = |el: &El| {
        el.children.iter().all(|n| matches!(n, Node::Comment(_) | Node::El(_)) || matches!(n, Node::Text(t) | Node::CData(t) if is_blank(t)))
    };
    if !blank(script) {
        return false;
    }
    let mut libraries = false;
    for child in script.elements() {
        // A prefix an element below declares on itself: found in the element's own attributes first.
        let uri_of = |el: &El, name: &str| -> Option<String> {
            let prefix = name.split_once(':').map_or("", |(p, _)| p);
            let key = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
            el.attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
        };
        let local_of = |name: &str| name.split_once(':').map_or(name.to_owned(), |(_, l)| l.to_owned());
        let (uri, _) = scopes.element(&child.name);
        let child_uri = uri_of(child, &child.name).or_else(|| uri.map(str::to_owned));
        if child_uri.as_deref() != Some(OOO) || local_of(&child.name) != "libraries" {
            return false;
        }
        libraries = true;
        for lib in child.elements() {
            let lib_uri = uri_of(lib, &lib.name).or_else(|| uri_of(child, &lib.name)).or_else(|| scopes.element(&lib.name).0.map(str::to_owned));
            if lib_uri.as_deref() != Some(OOO) || local_of(&lib.name) != "library-embedded" || lib.elements().next().is_some() || !blank(lib) {
                return false;
            }
        }
        if !blank(child) {
            return false;
        }
    }
    libraries
}

/// Whether text is white space only, as XML defines it.
fn is_blank(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// The package in `bytes` written again with every entry copied as it is: its compressed bytes, method, flags, extra fields, dates
/// and attributes, in the order the entries lie in the file, which keeps `mimetype` at byte 0 when it was there. The package is read
/// first as far as [`Package::read`] reads it before it inflates the entries (steps 1 to 4), so an encrypted package, or one that is not a
/// drawing, is refused and nothing is written; no entry but the manifest and `mimetype` is inflated. An entry whose headers cannot be kept
/// as they are is copied under headers written anew from its row (the container's rule), and the file is then not byte for byte the same,
/// though every entry's bytes are.
pub fn copy_unchanged(bytes: &[u8], limits: &Limits) -> Result<Vec<u8>, OdgError> {
    copy_unchanged_counted(bytes, limits, &mut Work::default())
}

/// [`copy_unchanged`], adding what the copy cost to `work` in the units each loop walks (rows, extra records, names, bytes inflated, XML
/// events, and the records of the local headers' extra fields the copy looked through: `local_extra_records`).
pub fn copy_unchanged_counted(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<Vec<u8>, OdgError> {
    let mut probe = probe(bytes, limits, work)?;
    if probe.manifest.encrypted() {
        return Err(OdgError::EncryptedPackage);
    }
    variant_of(media(&mut probe.archive, &probe.manifest, work)?.media_type.as_deref())?;
    let archive = &probe.archive;
    let mut order: Vec<(usize, usize)> = archive.entries().iter().enumerate().map(|(i, e)| (e.local_offset, i)).collect();
    order.sort_unstable();
    let mut writer = ArchiveWriter::new();
    for (_, i) in order {
        writer.copy_counted(archive, i, work)?;
    }
    Ok(writer.finish()?)
}

/// A package opened for a save to splice an edit into it: the archive, with nothing inflated but the manifest and `mimetype`, and what the
/// package says of itself. The writer inflates the parts it changes and copies every other entry as it is.
pub(crate) struct Opened<'a> {
    pub archive: Archive<'a>,
    pub manifest: Manifest,
    /// The manifest's bytes and where its entry is in the archive.
    pub manifest_bytes: Vec<u8>,
    pub manifest_at: usize,
    /// What the package says it is, and whether its `mimetype` is as R2 asks.
    pub media_type: Option<String>,
    pub mimetype_regular: bool,
    /// The package holds a signature file.
    pub signed: bool,
}

/// Opens the package in `bytes` for a save (steps 1 to 4 of a read): an encrypted package, or one that is not a drawing, is refused.
pub(crate) fn open<'a>(bytes: &'a [u8], limits: &Limits, work: &mut Work) -> Result<Opened<'a>, OdgError> {
    let mut probe = probe(bytes, limits, work)?;
    if probe.manifest.encrypted() {
        return Err(OdgError::EncryptedPackage);
    }
    let media = media(&mut probe.archive, &probe.manifest, work)?;
    variant_of(media.media_type.as_deref())?;
    let signed =
        probe.archive.entries().iter().any(|e| !e.folder && e.name.strip_prefix("META-INF/").is_some_and(|rest| rest.contains("signatures")));
    Ok(Opened {
        mimetype_regular: media.faults.is_empty(),
        media_type: media.media_type,
        signed,
        archive: probe.archive,
        manifest: probe.manifest,
        manifest_bytes: probe.manifest_bytes,
        manifest_at: probe.manifest_at,
    })
}

/// The names of a package that the format fixes, and so are shown as they are.
const STRUCTURE: [&str; 11] = [
    "mimetype",
    "content.xml",
    "styles.xml",
    "meta.xml",
    "settings.xml",
    "manifest.rdf",
    "META-INF/manifest.xml",
    "META-INF/documentsignatures.xml",
    "Thumbnails/thumbnail.png",
    "Configurations2/",
    crate::ext::docfile::PATH,
];

/// A part's name for a report that a person who must not see the file reads: the name itself only when the format fixes it
/// (`content.xml`, `META-INF/manifest.xml`, `Thumbnails/thumbnail.png`), and otherwise its role, since any other name can be one a
/// person chose (`Pictures/MyPlan.png`): `picture` under `Pictures/`, `object` for a sub-document or a replacement image
/// (`Object 1/`, `ObjectReplacements/`), `macro` under `Basic/` or `Scripts/`, `signature` for a signature file under
/// `META-INF/`, and `a part` for anything else.
pub fn part_label(name: &str) -> &str {
    if STRUCTURE.contains(&name) {
        return name;
    }
    if name.starts_with("Pictures/") {
        "picture"
    } else if name.starts_with("ObjectReplacements/") || name.starts_with("Object ") {
        "object"
    } else if name.starts_with("Basic/") || name.starts_with("Scripts/") {
        "macro"
    } else if name.strip_prefix("META-INF/").is_some_and(|rest| rest.contains("signatures")) {
        "signature"
    } else {
        "a part"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attribute_value_resolves_the_five_entities_and_numeric_references() {
        assert_eq!(unescape(b"a&amp;b"), "a&b");
        assert_eq!(unescape(b"a&lt;b"), "a<b");
        assert_eq!(unescape(b"a&gt;b"), "a>b");
        assert_eq!(unescape(b"a&quot;b"), "a\"b");
        assert_eq!(unescape(b"a&apos;b"), "a'b");
        assert_eq!(unescape(b"&#65;&#x42;&#x63;"), "ABc");
        assert_eq!(unescape(b"Pictures/a&amp;b&#46;png"), "Pictures/a&b.png");
        assert_eq!(unescape(b"no references"), "no references");
    }

    #[test]
    fn what_is_no_reference_is_kept_as_written() {
        // An unknown name, no semicolon, a body past the longest reference, a lone `&`, a bad number, a number that is no character.
        for text in ["a&nbsp;b", "a&b", "a&amp", "a&abcdefghijklm;b", "a&", "&;", "&#xZZ;", "&#;", "&#xD800;", "&#1114112;"] {
            assert_eq!(unescape(text.as_bytes()), text, "{text}");
        }
        // The kept `&` does not hide a reference after it.
        assert_eq!(unescape(b"&&amp;"), "&&");
        // What is no number: a sign, a capital X, a digit past the radix, a reference with no end.
        for text in ["&#+65;", "&#X41;", "&#x4G;", "&#4A;", "&#65", "&#x41"] {
            assert_eq!(unescape(text.as_bytes()), text, "{text}");
        }
    }

    /// A numeric reference of any length is read as the XML parser reads it, so the manifest's root row `&#0000000047;` is `/`.
    #[test]
    fn a_numeric_reference_has_no_length_cap() {
        assert_eq!(unescape(b"&#0000000047;"), "/");
        assert_eq!(unescape(b"&#x000000000002F;"), "/");
        assert_eq!(unescape(format!("a&#{}47;b", "0".repeat(100_000)).as_bytes()), "a/b");
        assert_eq!(unescape(format!("&#x{}41;", "0".repeat(100_000)).as_bytes()), "A");
        // A value past the characters is no character, however it is written, and the digits after it do not wrap round to one.
        assert_eq!(unescape(b"&#99999999999999999999;"), "&#99999999999999999999;");
        let wraps = format!("&#{}0000000000;", "9".repeat(50));
        assert_eq!(unescape(wraps.as_bytes()), wraps);
        // Each reference of a value of many is read where it is.
        assert_eq!(unescape(b"&#0000000065;&#x0000000042;&amp;&#67;"), "AB&C");
    }

    /// The work is the value's length: counted in the bytes looked at after each `&`, for the shapes that cost the most: a value of nothing but `&`,
    /// of `&#` over and over, of references that are never ended, and one reference of very many digits (a quadratic scan for the `;` of each `&` took
    /// the square of the length).
    #[test]
    fn the_work_of_unescaping_is_the_length_of_the_value() {
        let big = 200_000usize;
        let shapes: Vec<(&str, String)> = vec![
            ("only ampersands", "&".repeat(big)),
            ("hash after hash", "&#".repeat(big / 2)),
            ("hex digits never ended", "&#x1".repeat(big / 4)),
            ("digits then another reference", "&#1".repeat(big / 3)),
            ("references ended", "&#65;".repeat(big / 5)),
            ("names", "&amp;&lt;&gt;&quot;&apos;".repeat(big / 25)),
            ("one long reference", format!("&#{};", "0".repeat(big))),
            ("a long number and no end", format!("&#{}", "7".repeat(big))),
        ];
        for (what, value) in shapes {
            let mut steps = 0u64;
            let out = unescape_counted(value.as_bytes(), &mut steps);
            // Every byte of the value costs a step or two (the five names are each compared); never the square of the value.
            assert!(steps <= 7 * value.len() as u64, "{what}: {steps} steps for {} bytes", value.len());
            assert!(!out.is_empty(), "{what}");
        }
        // A value of 4,000 bytes of `&` and one of 8,000 differ by the factor of their lengths: 6 steps a byte at most, and the same at both.
        let steps = |n: usize| {
            let mut steps = 0;
            unescape_counted("&".repeat(n).as_bytes(), &mut steps);
            steps
        };
        assert_eq!(steps(8_000), 2 * steps(4_000));
        // The unit is exact: one step for each name compared (the five are tried in order), and the digits of a number and one more for the byte that
        // ended it, whatever the radix.
        let steps_of = |text: &str| {
            let mut steps = 0;
            unescape_counted(text.as_bytes(), &mut steps);
            steps
        };
        for (text, steps) in [
            ("&amp;", 1),
            ("&lt;", 2),
            ("&gt;", 3),
            ("&quot;", 4),
            ("&apos;", 5),
            ("&", 5),
            ("&#65;", 5 + 2 + 1),
            ("&#x41;", 5 + 2 + 1),
            ("&#x0000000041;", 5 + 10 + 1),
            ("&#;", 5 + 1),
            ("&#65", 5 + 2 + 1),
            ("&#6A5;", 5 + 1 + 1),
            ("a&amp;b&amp;c", 2),
            ("no reference", 0),
        ] {
            assert_eq!(steps_of(text), steps, "{text}");
        }
    }
}
