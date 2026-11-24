//! The package: its parts by name, their content types, and their relationships.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::archive::{self, PartData};
use crate::content_types::{self, ContentTypes};
use crate::error::{OpcError, Warning};
use crate::limits::Limits;
use crate::name::{fold, rels_part_name, rels_source};
use crate::rels::{self, Rels};

/// How many relationships parts with no source part are named in a [`Warning`] each; the rest are counted in one, as for directory
/// items. The parts are kept either way: this bounds what a package of nothing but orphans makes the warnings cost.
const MAX_ORPHAN_WARNINGS: usize = 100;

/// One part: its name as it arrived (case kept) and its bytes exactly as inflated, or the caller's own bytes that its entry holds ([`Package::read_knowing`]).
struct Part {
    name: String,
    data: PartData,
}

/// An Open Packaging Conventions package, read whole and held in memory.
///
/// Part names are the OPC form with the leading slash (`/visio/document.xml`), decoded (`caf%C3%A9` is `café`), in the case they
/// arrived with. Looking a part up takes the name with or without its slash and in any ASCII case; names are looked up as they are,
/// with no `.` or `..` resolved (use [`resolve`](crate::name::resolve) for a relative target). The ZIP item `[Content_Types].xml` is
/// not a part: [`content_types`](Package::content_types) has its rows.
pub struct Package {
    parts: Vec<Part>,
    /// Folded name to index in `parts`.
    index: HashMap<String, usize>,
    content_types: ContentTypes,
    /// The stream `content_types` was read from, as inflated.
    content_types_raw: Vec<u8>,
    warnings: Vec<Warning>,
    limits: Limits,
    rows: usize,
}

impl Package {
    /// Reads the package in `bytes` within `limits`. Everything is checked before it is trusted: see the crate documentation and
    /// [`Limits`]. Any rule broken, or any limit passed, is an error and there is no half-read package: the result is all of it or
    /// nothing. The parts' bytes are returned unchanged, whether or not they are XML, and whether or not they are well-formed.
    pub fn read(bytes: &[u8], limits: &Limits) -> Result<Package, OpcError> {
        Package::read_with(bytes, limits, None, 1)
    }

    /// [`Package::read`] with the entries inflated **on up to `threads` threads**: the package, the warnings and the error a file is refused with are the ones `read` gives, at any number
    /// of threads (`diacraft_container::zip::Archive::inflate_many`). 1 is `read`.
    pub fn read_threads(bytes: &[u8], limits: &Limits, threads: usize) -> Result<Package, OpcError> {
        Package::read_with(bytes, limits, None, threads)
    }

    /// [`Package::read`], for a caller that **has the bytes some parts should hold already** (a writer that reads back the file it made from them): `known` gives them for a part's name
    /// (canonical, with its slash, in the case the archive has it), or `None`. A part whose entry states the size and the checksum of those bytes, and whose data comes to both when it is
    /// read through the archive's buffer, **is those bytes, shared, and not a copy inflated beside them**: a check of a file made from parts of hundreds of megabytes
    /// holds them once. Every rule and limit of `read` holds for those parts as for the others (a damaged entry is the error it is, and each is counted toward the package total); a part
    /// whose entry holds other bytes is inflated, as `read` inflates it. The result is the package `read` gives, byte for byte.
    pub fn read_knowing(bytes: &[u8], limits: &Limits, known: &dyn Fn(&str) -> Option<Arc<[u8]>>) -> Result<Package, OpcError> {
        Package::read_with(bytes, limits, Some(known), 1)
    }

    fn read_with(bytes: &[u8], limits: &Limits, known: Option<archive::Known<'_>>, threads: usize) -> Result<Package, OpcError> {
        let raw = archive::read_archive(bytes, limits, known, threads)?;
        let (content_types, mut warnings) = content_types::parse(&raw.content_types, limits)?;
        let content_types_raw = raw.content_types;
        let rows = raw.rows;
        let mut all = raw.warnings;
        all.append(&mut warnings);
        let mut parts = Vec::with_capacity(raw.parts.len());
        let mut index = HashMap::with_capacity(raw.parts.len());
        for (name, data) in raw.parts {
            index.insert(fold(&name), parts.len());
            parts.push(Part { name, data });
        }
        // A relationships part whose source part is not here is kept as the part it is, and said so: the first hundred by name, and
        // the rest counted.
        let mut orphans = 0usize;
        for part in &parts {
            if let Some(source) = rels_source(&part.name)
                && source != "/"
                && !index.contains_key(&fold(&source))
            {
                orphans += 1;
                if orphans <= MAX_ORPHAN_WARNINGS {
                    all.push(Warning::OrphanRelationships { part: part.name.clone() });
                }
            }
        }
        if orphans > MAX_ORPHAN_WARNINGS {
            all.push(Warning::MoreOrphanRelationships { count: orphans - MAX_ORPHAN_WARNINGS });
        }
        Ok(Package { parts, index, content_types, content_types_raw, warnings: all, limits: limits.clone(), rows })
    }

    /// The bytes of the part named `name`, exactly as stored in the archive (inflated), or `None` if there is no such part.
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        let row = *self.index.get(&fold(name))?;
        self.parts.get(row).map(|p| p.data.bytes())
    }

    /// Moves the bytes of the part named `name` out of the package, so that they live once in the caller's hands and not twice (a reader that keeps
    /// a part as bytes beside the package it was read from would hold every such part twice). The part stays in the package, named and empty: [`Package::part`] gives
    /// no bytes for it from then on, and a second call gives none either. `None` when there is no such part (names are looked up as [`Package::part`] looks them up). A part that holds
    /// the caller's own bytes ([`Package::read_knowing`]) is given up as a copy of them: the package never owned them.
    pub fn take_part(&mut self, name: &str) -> Option<Vec<u8>> {
        let row = *self.index.get(&fold(name))?;
        self.parts.get_mut(row).map(|p| match std::mem::replace(&mut p.data, PartData::Own(Vec::new())) {
            PartData::Own(bytes) => bytes,
            PartData::Shared(bytes) => bytes.to_vec(),
        })
    }

    /// Lets go of the bytes of every part that `keep` says no to, the name of each given as [`Package::parts`] gives it: a reader that has taken what it needs of a
    /// package frees the rest before it copies any of it, so that the copies are made beside less. The parts stay named and empty, as after [`Package::take_part`].
    pub fn release_parts(&mut self, mut keep: impl FnMut(&str) -> bool) {
        for part in &mut self.parts {
            if !keep(&part.name) {
                part.data = PartData::Own(Vec::new());
            }
        }
    }

    /// The relationships of the part named `part` (`/` or nothing for the package itself): those in its `.rels` part, which is
    /// parsed on each call. A part with no relationships part has none, and that is not an error; a relationships part that is not
    /// well-formed, or has more rows than the limit, is. Targets that name no part, and cycles, are data (see [`Rels`]).
    ///
    /// **A target that names a part of the package is that part's name as the package holds it**: a `Target` of
    /// `VISIO/Document.XML` from the package is the part `/visio/document.xml` when that is how the archive names it, so that a reader that takes a name from a
    /// relationship, and a writer that makes a part in the folder of one, keep the case each part arrived with. The target as written is in `target_raw`.
    pub fn rels(&self, part: &str) -> Result<Rels, OpcError> {
        self.rels_within(part, usize::MAX)
    }

    /// [`Package::rels`], with at most `max_nodes` items in the tree of the relationships part (the least of it and the package's limit holds): for a caller that keeps
    /// a budget of memory and says how many items of a tree it can hold. A part with more is `xml_too_complex`, as it is past the package's own.
    pub fn rels_within(&self, part: &str, max_nodes: usize) -> Result<Rels, OpcError> {
        let rels_name = rels_part_name(part);
        let Some(bytes) = self.part(&rels_name) else { return Ok(Rels::default()) };
        let mut rels = rels::parse(bytes, &rels_name, part, &self.limits, max_nodes)?;
        for rel in &mut rels.items {
            if let Some(target) = rel.target.as_mut()
                && let Some(held) = self.name_of(target)
                && held != target.as_str()
            {
                // The two are one name with its ASCII case folded, so of one length: the string is written over, and does not grow.
                target.clear();
                target.push_str(held);
            }
        }
        Ok(rels)
    }

    /// The name of the part `name` names, as the package holds it (in the case it arrived with, with its leading slash), or `None` if there is no such part.
    /// Names are looked up as [`Package::part`] looks them up: with or without the slash, in any ASCII case.
    pub fn name_of(&self, name: &str) -> Option<&str> {
        let row = *self.index.get(&fold(name))?;
        self.parts.get(row).map(|p| p.name.as_str())
    }

    /// The content type of the part named `part`: the `Override` for its name, else the `Default` for its extension, each
    /// ASCII case-insensitive; `None` if no row gives it one. The part need not exist.
    pub fn content_type(&self, part: &str) -> Option<&str> {
        self.content_types.lookup(part)
    }

    /// The names of the parts, in the order of the archive's central directory (which means nothing), each in the case it
    /// arrived with and with its leading slash. Directory items and the content types stream are not parts.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|p| p.name.as_str())
    }

    /// The rows of `[Content_Types].xml`, as written (all kept, including rows for no part).
    pub fn content_types(&self) -> &ContentTypes {
        &self.content_types
    }

    /// The bytes of `[Content_Types].xml` exactly as inflated, whatever its layout: its declaration, its line ends, the white space between
    /// rows, its quotes. [`Package::content_types`] has the rows; these are for a writer that changes none and wants the stream back as it was.
    pub fn content_types_bytes(&self) -> &[u8] {
        &self.content_types_raw
    }

    /// Rows of the archive's central directory: the parts, the folders and the content types stream. What `Limits::max_parts` is checked against
    /// (twice over, and sixteen more), so a report of it shows how far a real file is from that limit.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// What was odd about the package and was read anyway: directory items dropped, repeated content type rows, relationships parts
    /// whose source part is missing.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

impl fmt::Debug for Package {
    /// The part names and sizes, not their bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<(&str, usize)> = self.parts.iter().map(|p| (p.name.as_str(), p.data.bytes().len())).collect();
        f.debug_struct("Package").field("parts", &parts).field("content_types", &self.content_types).field("warnings", &self.warnings).finish()
    }
}
