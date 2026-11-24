//! The new pictures of a package: a new picture shape is a `draw:frame` whose
//! `draw:image` names a new entry under `Pictures/`, and the save puts the entry in the package with a row for it in the manifest.
//!
//! **What this module decides**, each of it a rule of the save and tested as such:
//!
//! * **The type is told by the bytes**, as the reader judges a picture: PNG, JPEG, GIF and BMP, and EMF. The manifest's media type and the
//!   name's extension follow from it. A picture of another format (WMF, SVG, anything) is refused in words and nothing is written: R51 names the bitmap and the
//!   metafile of the kinds DiaCraft draws, and no rule gives a media type or an extension to the others.
//! * **The same bytes are one entry.** A picture that two new frames show (the same bytes, whether the model holds them once or twice) is written once and
//!   named by both frames, as the reader reads a part that many frames name as one copy (the media budget) and as a `draw:image` of a
//!   package is meant to be shared. Bytes are told equal by their length and a hash and then by a comparison of the bytes themselves, each pass charged; a
//!   hash alone would let two pictures be one. **An entry the package already holds is never reused**: that it holds the same bytes would take the entry
//!   inflated (or its checksum taken on trust), and a save reads no entry it is not told to; a picture copied from a frame of the file is written again.
//! * **The name is the writer's own and never the model's.** `Pictures/image<n>.<extension>`, the least `n` from the save's cursor up that no entry of the
//!   package, no row of its manifest and no `href` of the part being written is called, whatever the case (R7 matches exactly, but a file system that does not
//!   would write two entries as one). The cursor only moves on, so each picture costs one look in a set and the names are tried once each over the save. The name
//!   is never taken from [`diacraft_doc::Media::part`], which is the name the picture had in the file it was read from (a VSDX package names its pictures under
//!   a folder of its own format).
//! * **The package that is made must be one DiaCraft reads.** Each cap the reader holds a package to is checked before a byte is copied, and a picture that
//!   would pass one is refused in words: one picture past the container's cap for a part, more entries than it reads, more bytes than it reads over the package, more
//!   rows than the manifest may hold. The entry is deflated, and a picture that deflates past the ratio the container reads makes a package that does not open:
//!   the check of the bytes made says so, and the save is refused with that in words ([`WORDS_RATIO`]).
//! * **The bytes are not copied by the book.** It holds the model's `Arc` of them, and the archive is made from it. What the save then holds beside the model (the
//!   deflated copy and the archive being put together) is taken from the save's memory room when the picture is put in the book, before either is made.

use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::sync::Arc;

use diacraft_container::zip::Method;

use super::archive::Changes;
use super::scan::{Holder, PartKind, scan_part};
use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::package::Opened;

/// The words of a refusal of a picture whose bytes are in none of the formats the reader draws.
pub(crate) const WORDS_NOT_DRAWN: &str = "a new picture in a format the reader does not draw (PNG, JPEG, GIF, BMP and EMF are)";

/// The words of a refusal of a connector glued to a connection point of a new picture that a frame does not have in ODF.
pub(crate) const WORDS_GLUE: &str = "a new connector glued to a connection point of a new picture past the four a frame has in ODF (the file has no place for another point of a frame)";

/// The words of a refusal of a flipped new picture: a frame has no mirror the reader reads (the mirrors are those of a custom shape's geometry).
pub(crate) const WORDS_PICTURE_FLIP: &str = "a new picture that is flipped (a frame has no mirror the file's readers know: the picture would be drawn the right way up by every program but DiaCraft)";

/// The words of a refusal of a picture that deflates past the ratio the package layer reads.
pub(crate) const WORDS_RATIO: &str =
    "a new picture that deflates past the compression ratio the package layer reads (the entry is deflated, and a package with it would not open)";

/// The words of a limit passed by the hashes of the pictures (bytes charged to the save's scan).
const HASH_WORDS: &str = "bytes of pictures looked at to tell the same picture twice";

/// The words of a limit passed by the comparisons of equal pictures (bytes charged to the save's copies: a picture found equal is a copy not made).
const COMPARE_WORDS: &str = "bytes of pictures compared with an equal one (the copy each of them saves)";

/// What an index of the package's entries by name costs the save's memory: each name's bytes and the table's slot, rounded up.
pub(crate) fn index_cost<'a>(names: impl Iterator<Item = &'a str>) -> u64 {
    names.fold(0u64, |sum, name| sum.saturating_add(name.len() as u64 + NAME_BYTES))
}

/// The words of a refusal of a new picture in a file that holds a broken picture link: LibreOffice does not open a drawing that holds an image whose href
/// names no file of the package beside a picture the package holds (measured on 26.8.1.1), so a save that adds the picture would make of a file that opens one that does not.
pub(crate) const WORDS_BROKEN_LINK: &str = "a new picture in a file that holds a broken picture link (an image, fill or background whose href names no file of the package, in content.xml or styles.xml): LibreOffice would not open the result; remove or repair the broken frame first, save, and then add the picture";

/// The words of a refusal of a new picture in a package one of whose XML parts cannot be looked through for the names it already holds.
pub(crate) const WORDS_UNREADABLE_PART: &str = "a new picture in a package whose content.xml or styles.xml is in an encoding the save cannot look through for the names the part already uses (a new picture would take one of them)";

/// What a name in the set of names a part uses costs besides its bytes: the string's own header and the table's slot, rounded up.
const NAME_BYTES: u64 = 96;

/// The namespace of `xlink:href`, which a picture's `draw:image` names its entry by.
pub(crate) const XLINK_URI: &str = "http://www.w3.org/1999/xlink";

/// What a picture's own bytes say it is: its media type, as the manifest gives it, and the extension of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PictureType {
    pub media: &'static str,
    pub ext: &'static str,
}

/// The type of a picture by its first bytes, as the reader judges it: never by a name or a type a model says. `None` for a format the reader
/// does not draw.
pub(crate) fn picture_type(bytes: &[u8]) -> Option<PictureType> {
    crate::picturetype::picture_kind(bytes).map(|kind| PictureType { media: kind.media, ext: kind.ext })
}

/// Whether the picture link `href` is broken: a path in the package, and no entry of it is the one it names. What is not a path in the package is no broken link of the package's: an IRI
/// with a scheme (`http:`, `file:`, `mailto:`), a path from the root or out of the package (`/x`, `../x`), a place of the document (`#x`), and an empty href. A link to an object's
/// replacement picture (`ObjectReplacements/..`) is not counted either: LibreOffice opens a file whose replacement is missing (measured on 26.8.1.1, with the forms above). The href is
/// resolved as the reader resolves it (`package::href_entry`: percent-decoded and stripped of `./`, else as written), against the entries by name.
pub(crate) fn is_broken_link(href: &str, index: &std::collections::HashMap<String, usize>) -> bool {
    is_path_in_the_package(href) && crate::package::href_entry(href, index).is_none()
}

/// Whether the link `href` is a path in the package ([`is_broken_link`]'s first half): none of an IRI with a scheme, a path from the root or out of the package, a place of the document, an
/// empty href, or a link to an object's replacement picture.
fn is_path_in_the_package(href: &str) -> bool {
    let decoded = crate::package::percent_decode(href);
    let path = decoded.strip_prefix("./").unwrap_or(&decoded);
    let has_scheme = path.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    let out_of_the_package = path.is_empty()
        || path.starts_with(['/', '#'])
        || path == ".."
        || path.starts_with("../")
        || path.contains("/../")
        || href.is_empty()
        || has_scheme;
    !(out_of_the_package || path.starts_with("ObjectReplacements/"))
}

/// The link `href`, a path in the package that names no entry of it, as a link **out of the package**: `../` and the path, the way a program writes a link to a file beside the document in a
/// package. A flat drawing's relative link (`plan.svg`, `Pictures/p1.svg`) is a file beside the document; the same text in a package names an entry (ODF resolves an IRI with no `../` inside the
/// package), and one the package has not. `None` for a link that is no path in the package.
pub(crate) fn out_of_the_package(href: &str) -> Option<String> {
    is_path_in_the_package(href).then(|| format!("../{}", href.strip_prefix("./").unwrap_or(href)))
}

/// An entry a save adds to the package for a new picture.
#[derive(Clone, Debug)]
pub(crate) struct NewPicture {
    /// The entry's name, which the frames' hrefs and the manifest's row say.
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub media: &'static str,
}

/// The new pictures of one save: the entries to add, and what is needed to name each and tell one from another.
#[derive(Debug, Default)]
pub(crate) struct PictureBook {
    /// The names no new picture may take, lower-cased: the package's entries, its manifest's rows and the hrefs of the parts written.
    taken: HashSet<String>,
    /// The number the next name tries first.
    cursor: u64,
    entries: Vec<NewPicture>,
    /// The `Arc`s of pictures that were told equal to an entry's by their bytes: kept so that the address each is known by stays its own.
    aliases: Vec<Arc<[u8]>>,
    /// The entry of a picture by where its bytes are in memory and how long they are: the model's own `Arc`, shared by every frame that shows it.
    by_address: HashMap<(usize, usize), usize>,
    /// The entries by the length and the hash of their bytes: the candidates a picture of the same length and hash is compared with.
    by_content: HashMap<(usize, u64), Vec<usize>>,
    /// What the package has room for yet: entries, bytes (uncompressed, over the package), manifest rows, and the bytes of one entry.
    parts_left: usize,
    bytes_left: u64,
    rows_left: usize,
    part_cap: u64,
}

impl PictureBook {
    /// The book of a save of `opened`: the names it holds and the room it has left.
    pub(crate) fn of(opened: &Opened<'_>, limits: &Limits, work: &mut WriteWork) -> Result<PictureBook, WriteError> {
        let entries = opened.archive.entries();
        let rows = &opened.manifest.entries;
        let mut taken: HashSet<String> = HashSet::new();
        let wanted = entries.len().saturating_add(rows.len());
        taken.try_reserve(wanted).map_err(|_| WriteError::Limit { what: "memory for the names of a package's entries", max: wanted as u64 })?;
        let mut used = 0u64;
        for entry in entries {
            used = used.saturating_add(entry.size);
            taken.insert(entry.name.to_ascii_lowercase());
        }
        for row in rows {
            taken.insert(row.full_path.to_ascii_lowercase());
        }
        work.manifest_probes = work.manifest_probes.saturating_add(wanted as u64);
        Ok(PictureBook {
            taken,
            cursor: 1,
            parts_left: limits.container.max_parts.saturating_sub(entries.len()),
            bytes_left: limits.container.max_package_bytes.saturating_sub(used),
            rows_left: limits.max_manifest_entries.saturating_sub(rows.len()),
            part_cap: limits.container.max_part_bytes,
            ..PictureBook::default()
        })
    }

    /// Whether the package this save makes holds an entry the picture link `href` names, as the reader resolves it ([`is_broken_link`]), by the names the book knows: the package's entries and
    /// rows, the pictures this save added so far, and the names the parts already use. A link that is no path in the package names none and is not asked about: it counts as held. (The names
    /// are in lower case, so a link that differs from an entry's name by case is held, as the book keeps new names clear of both.)
    pub(crate) fn holds_name(&self, href: &str) -> bool {
        if !is_path_in_the_package(href) {
            return true;
        }
        let decoded = crate::package::percent_decode(href);
        let normalised = decoded.strip_prefix("./").unwrap_or(&decoded);
        let as_written = href.strip_prefix("./").unwrap_or(href);
        [normalised, as_written].into_iter().any(|name| self.taken.contains(&name.to_ascii_lowercase()))
    }

    /// A name no new picture may take: what a part already names by an `href`, in **both** spellings the reader tries: the href percent-decoded and then
    /// stripped of a leading `./` (so `%2E/Pictures/a.png` and `.%2FPictures/a.png` are `Pictures/a.png`), and the href as written with a leading `./` stripped. The names are kept
    /// in lower case, as every name is, and each new one is taken from the save's memory room.
    pub(crate) fn reserve(&mut self, href: &str, work: &mut WriteWork) -> Result<(), WriteError> {
        let decoded = crate::package::percent_decode(href);
        let normalised = decoded.strip_prefix("./").unwrap_or(&decoded);
        let as_written = href.strip_prefix("./").unwrap_or(href);
        for name in [normalised, as_written] {
            let name = name.to_ascii_lowercase();
            if self.taken.contains(&name) {
                continue;
            }
            // What the set holds for the name: its bytes, and the entry of the table that points at them.
            work.hold(name.len() as u64 + NAME_BYTES, "memory for the names a package's parts already use")?;
            self.taken
                .try_reserve(1)
                .map_err(|_| WriteError::Limit { what: "memory for the names a package's parts already use", max: self.taken.len() as u64 })?;
            self.taken.insert(name);
        }
        Ok(())
    }

    /// Looks through **both** XML parts of the package for the names they already use, before any picture is named: `content.xml` and `styles.xml`, whether or not the save changes
    /// them (a frame of a master page in `styles.xml` that names a name no entry holds shows the picture a new entry of that name holds, as a frame of a page does), and each
    /// reserved in both spellings. A part the scan cannot look through is a refusal in words.
    ///
    /// **A broken picture link is a refusal**: an image, a fill, a stroke, a background or a bullet whose href is a path in the package that names no entry of it, in
    /// either part and in any spelling the reader resolves. LibreOffice does not open the file the save would make, though it opens the file as it is.
    pub(crate) fn reserve_parts(&mut self, opened: &mut Opened<'_>, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
        // The entries by name, as the reader resolves a href against them (folders with their slash).
        let entries = opened.archive.entries();
        let index_bytes = index_cost(entries.iter().map(|e| e.name.as_str()));
        work.hold(index_bytes, "memory for the names of a package's entries")?;
        let index: std::collections::HashMap<String, usize> = entries.iter().enumerate().map(|(at, e)| (e.name.clone(), at)).collect();
        let result = self.reserve_parts_in(opened, &index, limits, work);
        work.release(index_bytes);
        result
    }

    fn reserve_parts_in(
        &mut self,
        opened: &mut Opened<'_>,
        index: &std::collections::HashMap<String, usize>,
        limits: &Limits,
        work: &mut WriteWork,
    ) -> Result<(), WriteError> {
        for (kind, name) in [(PartKind::Content, "content.xml"), (PartKind::Styles, "styles.xml")] {
            let Some(at) = opened.archive.position(name) else { continue };
            let bytes = opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?;
            if diacraft_container::xml_scan::is_utf16(&bytes) {
                return Err(WriteError::Unsupported(WORDS_UNREADABLE_PART));
            }
            let found = scan_part(&bytes, kind, limits, work)?;
            drop(bytes);
            for h in &found.hrefs {
                work.manifest_probes = work.manifest_probes.saturating_add(1);
                if matches!(h.holder, Holder::Image | Holder::StyleImage) && is_broken_link(&h.href, index) {
                    return Err(WriteError::Unsupported(WORDS_BROKEN_LINK));
                }
                self.reserve(&h.href, work)?;
            }
        }
        Ok(())
    }

    /// The entry a picture of `bytes`, of type `ty`, is written as: the name an earlier picture of the same bytes has, else a new entry, after each cap is checked
    /// and the memory the save will hold for it is taken from its room.
    pub(crate) fn add(&mut self, bytes: &Arc<[u8]>, ty: PictureType, limits: &Limits, work: &mut WriteWork) -> Result<&str, WriteError> {
        self.add_as(bytes, ty, None, limits, work)
    }

    /// [`PictureBook::add`] with the name the picture is asked to be written under (the entry a picture of a file was read from): taken when no entry or row of the
    /// package has it, and a name of the writer's own otherwise.
    pub(crate) fn add_as(
        &mut self,
        bytes: &Arc<[u8]>,
        ty: PictureType,
        wanted: Option<&str>,
        limits: &Limits,
        work: &mut WriteWork,
    ) -> Result<&str, WriteError> {
        let len = bytes.len();
        let address = (bytes.as_ptr() as usize, len);
        if let Some(&at) = self.by_address.get(&address) {
            return self.name_at(at);
        }
        // One picture past what a part of a package may hold is not a package the reader opens.
        if len as u64 > self.part_cap {
            return Err(WriteError::Limit { what: "bytes in one picture (the most a part of a package holds)", max: self.part_cap });
        }
        // The same bytes as an entry made earlier in the save: by the length and a hash, and then by the bytes.
        work.charge_scan(len, limits).map_err(|_| WriteError::Limit { what: HASH_WORDS, max: limits.max_write_bytes })?;
        let mut hasher = std::hash::DefaultHasher::new();
        hasher.write(bytes);
        let key = (len, hasher.finish());
        let candidates: Vec<usize> = self.by_content.get(&key).cloned().unwrap_or_default();
        for at in candidates {
            // The comparison is charged to the copies, and not to the scan the hash was: a picture found equal is an entry not made, which would have cost its length in copies,
            // so a picture shown by many frames costs a hash for each and a copy it saves for each, and is saved where different pictures of the same total are.
            work.charge_copy(len, limits).map_err(|_| WriteError::Limit { what: COMPARE_WORDS, max: limits.max_write_bytes })?;
            if self.entries.get(at).is_some_and(|e| e.bytes == *bytes) {
                // This `Arc` is held from now on, so that its address is no other picture's (an `Arc` that is dropped may have its place given to another).
                self.aliases.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the pictures a save adds", max: at as u64 })?;
                self.aliases.push(Arc::clone(bytes));
                self.by_address.insert(address, at);
                return self.name_at(at);
            }
        }
        // A new entry: what the package can hold, and what the save will hold of it.
        if self.parts_left == 0 {
            return Err(WriteError::Limit { what: "entries a package may hold", max: limits.container.max_parts as u64 });
        }
        if self.rows_left == 0 {
            return Err(WriteError::Limit { what: "rows of a manifest", max: limits.max_manifest_entries as u64 });
        }
        self.bytes_left = self
            .bytes_left
            .checked_sub(len as u64)
            .ok_or(WriteError::Limit { what: "bytes in a package (uncompressed)", max: limits.container.max_package_bytes })?;
        // The deflated copy and the archive put together beside the model's own bytes: held before either is made.
        work.hold((len as u64).saturating_mul(2), "bytes of memory a save holds for the pictures it adds to a package")?;
        let name = match wanted {
            Some(wanted) if self.taken.insert(wanted.to_ascii_lowercase()) => wanted.to_owned(),
            _ => self.fresh_name(ty, work),
        };
        self.parts_left -= 1;
        self.rows_left -= 1;
        let at = self.entries.len();
        self.entries.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the pictures a save adds", max: at as u64 })?;
        self.entries.push(NewPicture { name, bytes: Arc::clone(bytes), media: ty.media });
        self.by_address.insert(address, at);
        self.by_content.entry(key).or_default().push(at);
        self.name_at(at)
    }

    fn name_at(&self, at: usize) -> Result<&str, WriteError> {
        self.entries.get(at).map(|e| e.name.as_str()).ok_or(WriteError::Mismatch("a picture the book names is not in it"))
    }

    /// The least name `Pictures/image<n>.<ext>` from the cursor up that nothing takes; the cursor moves past it. Each name tried is a look in a set and a step.
    fn fresh_name(&mut self, ty: PictureType, work: &mut WriteWork) -> String {
        loop {
            let name = format!("Pictures/image{}.{}", self.cursor, ty.ext);
            self.cursor = self.cursor.saturating_add(1);
            work.manifest_probes = work.manifest_probes.saturating_add(1);
            if self.taken.insert(name.to_ascii_lowercase()) {
                return name;
            }
        }
    }

    /// The pictures to add, in the order they were first written.
    pub(crate) fn finish(self) -> Vec<NewPicture> {
        self.entries
    }
}

/// Adds the entries of `pictures` to the package's changes, before anything else that is new (the thumbnail follows them: the order), and gives the manifest's rows
/// for them. The bytes are the model's, shared: nothing is copied here.
pub(crate) fn put_in(pictures: &[NewPicture], changes: &mut Changes) -> Vec<(String, String)> {
    let mut rows = Vec::with_capacity(pictures.len());
    for p in pictures {
        changes.pictures.push((p.name.clone(), Arc::clone(&p.bytes), Method::Deflated));
        rows.push((p.name.clone(), p.media.to_owned()));
    }
    rows
}

/// Whether `bytes` are the package the save made, with each new picture in it: an entry of the name, as long as the bytes, and a row in the manifest that says
/// what it is. The directory and the manifest are read, nothing is inflated.
pub(crate) fn check_made(bytes: &[u8], pictures: &[NewPicture], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if pictures.is_empty() {
        return Ok(());
    }
    let made = crate::package::open(bytes, limits, &mut work.package).map_err(|e| made_error(&e, limits))?;
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let rows: HashMap<&str, Option<&str>> = made.manifest.entries.iter().map(|r| (r.full_path.as_str(), r.media_type.as_deref())).collect();
    let sizes: HashMap<&str, u64> = made.archive.entries().iter().filter(|e| !e.folder).map(|e| (e.name.as_str(), e.size)).collect();
    work.manifest_probes = work.manifest_probes.saturating_add((rows.len() + sizes.len()) as u64);
    for p in pictures {
        work.manifest_probes = work.manifest_probes.saturating_add(2);
        match sizes.get(p.name.as_str()) {
            Some(size) if *size == p.bytes.len() as u64 => {}
            Some(_) => return Err(failed("a new picture's entry holds another number of bytes than the picture")),
            None => return Err(failed("a new picture's entry is not in the package made")),
        }
        if rows.get(p.name.as_str()).copied().flatten() != Some(p.media) {
            return Err(failed("a new picture's entry has no manifest row that says what it is"));
        }
    }
    Ok(())
}

/// What the package a save made refused as, when it holds new pictures: the ratio is the picture's own fault and is said in words, a cap of the package that the book's count
/// could not see (the parts the save makes new beside the pictures: the grown `content.xml`, a thumbnail, a `meta.xml`) is a limit, and any other refusal is the save's.
pub(crate) fn made_error(e: &crate::OdgError, limits: &Limits) -> WriteError {
    match e.class() {
        "zip_ratio_exceeded" => WriteError::Unsupported(WORDS_RATIO),
        "package_too_large" => WriteError::Limit { what: "bytes in a package (uncompressed)", max: limits.container.max_package_bytes },
        "too_many_parts" => WriteError::Limit { what: "entries a package may hold", max: limits.container.max_parts as u64 },
        "too_many_manifest_entries" => WriteError::Limit { what: "rows of a manifest", max: limits.max_manifest_entries as u64 },
        _ => WriteError::SelfCheck(format!("{e} ({})", e.class())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(bytes: &[u8]) -> Arc<[u8]> {
        Arc::from(bytes)
    }

    const PNG: PictureType = PictureType { media: "image/png", ext: "png" };

    /// The book holds the names the package has, in every spelling the reader resolves, and a link that is no path in the package counts as held; a path that names no entry is not held, and
    /// is made a link out of the package.
    #[test]
    fn the_book_tells_a_link_that_names_an_entry_from_one_that_names_none() {
        let mut book = PictureBook::default();
        let mut work = WriteWork::default();
        for entry in ["Pictures/p1.svg", "Pictures/b c.png"] {
            book.reserve(entry, &mut work).unwrap();
        }
        for held in [
            "Pictures/p1.svg",
            "./Pictures/p1.svg",
            "%2E/Pictures/p1.svg",
            "Pictures/p%31.svg",
            "Pictures/b%20c.png",
            "pictures/P1.SVG",
            "../plan.svg",
            "https://example.org/a.svg",
            "#x",
            "ObjectReplacements/Object 1",
        ] {
            assert!(book.holds_name(held), "{held} is held");
        }
        for not_held in ["plan.svg", "Pictures/p2.svg", "./Pictures/gone.png", "Pictures/sub/p1.svg"] {
            assert!(!book.holds_name(not_held), "{not_held} names no entry");
            assert_eq!(out_of_the_package(not_held), Some(format!("../{}", not_held.strip_prefix("./").unwrap_or(not_held))));
        }
        for outside in ["../plan.svg", "https://example.org/a.svg", "#x", "", "/tmp/x.png", "ObjectReplacements/Object 1"] {
            assert_eq!(out_of_the_package(outside), None, "{outside:?} is no path in the package");
        }
    }

    #[test]
    fn a_picture_is_told_by_its_first_bytes_and_by_nothing_else() {
        let t = |b: &[u8]| picture_type(b).map(|t| (t.media, t.ext));
        assert_eq!(t(&[0x89, b'P', b'N', b'G', 1]), Some(("image/png", "png")));
        assert_eq!(t(&[0xFF, 0xD8, 0]), Some(("image/jpeg", "jpg")));
        assert_eq!(t(b"GIF89a"), Some(("image/gif", "gif")));
        assert_eq!(t(b"BM\0\0"), Some(("image/bmp", "bmp")));
        let mut emf = vec![0u8; 40];
        emf.extend_from_slice(b" EMF");
        assert_eq!(t(&emf), Some(("image/emf", "emf")));
        // Near misses and other formats are none: too short, the wrong byte, the signature at another place, WMF, SVG.
        for none in [&b""[..], b"GIF", b"B", &[0xFF, 0xD7], &[0x89, b'P', b'N'], b"<svg xmlns=", &[0xD7, 0xCD, 0xC6, 0x9A, 0, 0]] {
            assert_eq!(t(none), None, "{none:?}");
        }
        let mut late = vec![0u8; 41];
        late.extend_from_slice(b" EMF");
        assert_eq!(t(&late), None, "the signature of a metafile is at bytes 40 to 43");
        assert_eq!(t(&emf[..43]), None, "four bytes of it, not three");
    }

    #[test]
    fn the_same_bytes_are_one_entry_by_address_and_by_content_and_other_bytes_are_another() {
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let mut book = PictureBook { cursor: 1, parts_left: 10, bytes_left: 1 << 20, rows_left: 10, part_cap: 1 << 20, ..PictureBook::default() };
        let a = arc(b"\x89PNG one");
        let first = book.add(&a, PNG, &limits, &mut work).unwrap().to_owned();
        assert_eq!(first, "Pictures/image1.png");
        assert_eq!(book.add(&a, PNG, &limits, &mut work).unwrap(), first, "the same Arc");
        assert_eq!(book.add(&arc(b"\x89PNG one"), PNG, &limits, &mut work).unwrap(), first, "the same bytes in another Arc");
        assert_eq!(book.add(&arc(b"\x89PNG two"), PNG, &limits, &mut work).unwrap(), "Pictures/image2.png", "other bytes of the same length");
        assert_eq!(book.add(&arc(b"\x89PNG one!"), PNG, &limits, &mut work).unwrap(), "Pictures/image3.png", "other bytes of another length");
        assert_eq!(book.entries.len(), 3);
        // The room: the entries, the rows and the bytes are each taken once for an entry and not for a picture that is one already made.
        assert_eq!((book.parts_left, book.rows_left, book.bytes_left), (7, 7, (1 << 20) - 8 - 8 - 9));
    }

    #[test]
    fn a_name_that_the_package_the_manifest_or_a_part_holds_is_not_given_whatever_its_case() {
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let mut book = PictureBook { cursor: 1, parts_left: 10, bytes_left: 1 << 20, rows_left: 10, part_cap: 1 << 20, ..PictureBook::default() };
        book.taken.insert("pictures/image1.png".to_owned());
        book.reserve("./Pictures/IMAGE2.png", &mut work).unwrap();
        book.reserve("Pictures/image%33.png", &mut work).unwrap();
        let name = book.add(&arc(b"\x89PNG"), PNG, &limits, &mut work).unwrap().to_owned();
        assert_eq!(name, "Pictures/image4.png");
        // A name of another extension is another name: the cursor runs on, and each number is tried once.
        let jpeg = PictureType { media: "image/jpeg", ext: "jpg" };
        assert_eq!(book.add(&arc(&[0xFF, 0xD8, 1]), jpeg, &limits, &mut work).unwrap(), "Pictures/image5.jpg");
    }

    #[test]
    fn each_cap_refuses_past_it_and_not_at_it() {
        let limits = Limits::default();
        let book = |parts: usize, bytes: u64, rows: usize, part: u64| PictureBook {
            cursor: 1,
            parts_left: parts,
            bytes_left: bytes,
            rows_left: rows,
            part_cap: part,
            ..PictureBook::default()
        };
        let png = |n: usize| {
            let mut v = vec![0x89, b'P', b'N', b'G'];
            v.resize(n, 9);
            arc(&v)
        };
        let words = |r: Result<&str, WriteError>| match r {
            Err(WriteError::Limit { what, .. }) => what,
            Ok(_) => "no refusal",
            Err(_) => "another error",
        };
        // One picture of exactly the cap for a part is written; one byte more is not.
        assert!(book(1, 100, 1, 10).add(&png(10), PNG, &limits, &mut WriteWork::default()).is_ok());
        assert!(words(book(1, 100, 1, 10).add(&png(11), PNG, &limits, &mut WriteWork::default())).contains("one picture"));
        // The bytes over the package: exactly what is left is written, one more is not.
        assert!(book(1, 10, 1, 100).add(&png(10), PNG, &limits, &mut WriteWork::default()).is_ok());
        assert!(words(book(1, 9, 1, 100).add(&png(10), PNG, &limits, &mut WriteWork::default())).contains("in a package"));
        // The entries and the rows: the last of each is written, none left is refused.
        assert!(book(1, 100, 1, 100).add(&png(5), PNG, &limits, &mut WriteWork::default()).is_ok());
        assert!(words(book(0, 100, 1, 100).add(&png(5), PNG, &limits, &mut WriteWork::default())).contains("entries"));
        assert!(words(book(1, 100, 0, 100).add(&png(5), PNG, &limits, &mut WriteWork::default())).contains("rows of a manifest"));
        // The memory of the save: twice the picture is held, exactly the room is enough and one byte less is not.
        let mut work = WriteWork { memory_room: 20, memory_cap: 20, ..WriteWork::default() };
        assert!(book(1, 100, 1, 100).add(&png(10), PNG, &limits, &mut work).is_ok());
        assert_eq!(work.memory_room, 0);
        let mut work = WriteWork { memory_room: 19, memory_cap: 19, ..WriteWork::default() };
        assert!(words(book(1, 100, 1, 100).add(&png(10), PNG, &limits, &mut work)).contains("memory"));
    }

    /// A minimal package of a drawing: `mimetype`, the entries given, and a manifest with the root row and `rows`.
    fn package_of(entries: &[(&str, Vec<u8>)], rows: &[(&str, &str)]) -> Vec<u8> {
        use diacraft_container::zip::ArchiveWriter;
        let media = "application/vnd.oasis.opendocument.graphics";
        let mut w = ArchiveWriter::new();
        w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(name).unwrap();
            } else {
                w.add(name, data, Method::Deflated).unwrap();
            }
        }
        let mut manifest = format!(
            "<?xml version=\"1.0\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{media}\"/>\n"
        );
        for (path, kind) in rows {
            manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{kind}\"/>\n"));
        }
        manifest.push_str("</manifest:manifest>\n");
        w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
        w.finish().unwrap()
    }

    fn new_picture(name: &str, bytes: &[u8], media: &'static str) -> NewPicture {
        NewPicture { name: name.to_owned(), bytes: arc(bytes), media }
    }

    /// The names are tried once each over the save: the cursor only moves on, so the looks in the set are the names the package holds that are in the way, once, and one for each
    /// picture made. A search from the first number for each picture would be quadratic (the count would be the product).
    #[test]
    fn the_names_tried_over_a_save_are_the_ones_in_the_way_once_and_one_for_each_picture() {
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let mut book =
            PictureBook { cursor: 1, parts_left: 10_000, bytes_left: 1 << 30, rows_left: 10_000, part_cap: 1 << 20, ..PictureBook::default() };
        let held = 500u64;
        for n in 1..=held {
            book.taken.insert(format!("pictures/image{n}.png"));
        }
        let made = 300u64;
        for k in 0..made {
            let mut bytes = vec![0x89, b'P', b'N', b'G'];
            bytes.extend_from_slice(&k.to_le_bytes());
            let name = book.add(&arc(&bytes), PNG, &limits, &mut work).unwrap().to_owned();
            assert_eq!(name, format!("Pictures/image{}.png", held + k + 1));
        }
        assert_eq!(work.manifest_probes, held + made, "each name in the way once and each name given once");
    }

    /// The entries of the pictures go into the changes first, with the model's own bytes shared (no copy), and their rows are given in the same order with the media type.
    #[test]
    fn the_pictures_are_put_into_the_changes_in_order_with_their_bytes_shared_and_their_rows() {
        let (a, b) = (new_picture("Pictures/image1.png", b"\x89PNG a", "image/png"), new_picture("Pictures/image2.jpg", b"\xFF\xD8 b", "image/jpeg"));
        let mut changes = Changes::default();
        let rows = put_in(&[a.clone(), b.clone()], &mut changes);
        assert_eq!(rows, [("Pictures/image1.png".to_owned(), "image/png".to_owned()), ("Pictures/image2.jpg".to_owned(), "image/jpeg".to_owned())]);
        let names: Vec<&str> = changes.pictures.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(names, ["Pictures/image1.png", "Pictures/image2.jpg"]);
        assert!(changes.pictures.iter().zip([&a, &b]).all(|((_, bytes, method), p)| Arc::ptr_eq(bytes, &p.bytes) && *method == Method::Deflated));
        assert!(changes.add.is_empty() && put_in(&[], &mut Changes::default()).is_empty());
    }

    /// The check of the package made: each new picture has an entry of its name and size and a row that says what it is; one that lacks any is the save's fault, in words. No
    /// picture, nothing is opened.
    #[test]
    fn the_check_of_the_package_made_wants_each_entry_with_its_size_and_a_row_that_says_what_it_is() {
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let pic = b"\x89PNG data".to_vec();
        let wanted = [new_picture("Pictures/image1.png", &pic, "image/png")];
        let good = package_of(&[("Pictures/image1.png", pic.clone())], &[("Pictures/image1.png", "image/png")]);
        assert!(check_made(&good, &wanted, &limits, &mut work).is_ok());
        assert!(check_made(b"not a package", &[], &limits, &mut work).is_ok(), "no pictures: nothing is opened");
        let why = |bytes: &[u8]| match check_made(bytes, &wanted, &limits, &mut WriteWork::default()) {
            Err(WriteError::SelfCheck(why)) => why,
            other => panic!("not a fault of the save: {other:?}"),
        };
        assert!(why(&package_of(&[], &[("Pictures/image1.png", "image/png")])).contains("not in the package"));
        assert!(
            why(&package_of(&[("Pictures/image1.png", b"\x89PNG".to_vec())], &[("Pictures/image1.png", "image/png")]))
                .contains("another number of bytes")
        );
        assert!(why(&package_of(&[("Pictures/image1.png", pic.clone())], &[])).contains("no manifest row"));
        assert!(why(&package_of(&[("Pictures/image1.png", pic.clone())], &[("Pictures/image1.png", "image/jpeg")])).contains("no manifest row"));
        // A folder of that name is no entry of the picture's.
        assert!(why(&package_of(&[("Pictures/image1.png/", Vec::new())], &[("Pictures/image1.png", "image/png")])).contains("not in the package"));
        // A package that does not open is a fault of the save, and the ratio is the picture's own, in words.
        assert!(matches!(check_made(b"not a package", &wanted, &limits, &mut work), Err(WriteError::SelfCheck(_))));
        let mut zeros = vec![0x89, b'P', b'N', b'G'];
        zeros.resize(3_000_000, 0);
        let squeezed = package_of(&[("Pictures/image1.png", zeros.clone())], &[("Pictures/image1.png", "image/png")]);
        let wanted = [new_picture("Pictures/image1.png", &zeros, "image/png")];
        assert!(matches!(check_made(&squeezed, &wanted, &limits, &mut work), Err(WriteError::Unsupported(w)) if w == WORDS_RATIO));
    }

    /// The names a part already names are reserved as the reader resolves them: with a leading `./` taken off and percent-decoded, in any case.
    #[test]
    fn an_href_is_reserved_in_both_spellings_the_reader_tries_and_each_new_name_costs_the_saves_memory() {
        let mut book = PictureBook::default();
        let mut work = WriteWork { memory_room: 10_000, memory_cap: 10_000, ..WriteWork::default() };
        for href in [
            "./Pictures/A.png",
            "Pictures/b%20c.png",
            "Pictures/plain.png",
            "%2E/Pictures/Dot.png",
            ".%2FPictures/Slash.png",
            "./Pictures/x%2Ey.png",
            "Pictures/plain.png",
        ] {
            book.reserve(href, &mut work).unwrap();
        }
        let mut held: Vec<&str> = book.taken.iter().map(String::as_str).collect();
        held.sort_unstable();
        assert_eq!(
            held,
            [
                "%2e/pictures/dot.png",
                ".%2fpictures/slash.png",
                "pictures/a.png",
                "pictures/b c.png",
                "pictures/b%20c.png",
                "pictures/dot.png",
                "pictures/plain.png",
                "pictures/slash.png",
                "pictures/x%2ey.png",
                "pictures/x.y.png",
            ],
            "the href decoded and then stripped of ./, and as written with ./ stripped, in lower case; a name held is held once"
        );
        let cost: u64 = held.iter().map(|n| n.len() as u64 + NAME_BYTES).sum();
        assert_eq!(work.memory_room, 10_000 - cost, "each name held costs its bytes and the table's slot, once");
        // Past the room: a limit, and nothing taken.
        let mut tight = WriteWork { memory_room: NAME_BYTES + 3, memory_cap: 99, ..WriteWork::default() };
        let mut other = PictureBook::default();
        assert!(matches!(other.reserve("a.png", &mut tight), Err(WriteError::Limit { max: 99, .. })));
        assert!(other.taken.is_empty(), "{:?}", other.taken);
    }

    /// A copy of a picture costs the hash of its bytes in the scan (each picture does) and the comparison in the copies it saves; the first picture, and the same `Arc` again, cost
    /// no comparison. Each edge is one byte wide.
    #[test]
    fn a_copy_costs_a_hash_in_the_scan_and_a_comparison_in_the_copies_it_saves() {
        let limits = Limits { max_write_bytes: 200, ..Limits::default() };
        let mut book = PictureBook { cursor: 1, parts_left: 10, bytes_left: 1 << 20, rows_left: 10, part_cap: 1 << 20, ..PictureBook::default() };
        let mut bytes = vec![0x89, b'P', b'N', b'G'];
        bytes.resize(100, 7);
        let (first, copy) = (arc(&bytes), arc(&bytes));
        let mut work = WriteWork::default();
        book.add(&first, PNG, &limits, &mut work).unwrap();
        assert_eq!((work.scan_bytes, work.bytes_copied), (100, 0), "a picture of its own: a hash");
        book.add(&first, PNG, &limits, &mut work).unwrap();
        assert_eq!((work.scan_bytes, work.bytes_copied), (100, 0), "the same Arc: nothing");
        book.add(&copy, PNG, &limits, &mut work).unwrap();
        assert_eq!((work.scan_bytes, work.bytes_copied), (200, 100), "a copy: a hash, and the comparison against the copy it saves");
        // The comparison's edge: 100 bytes of copies already, and it is exactly the ceiling of 200; one more byte of copies is a limit in its own words.
        let words = |copied: u64| {
            let mut book = PictureBook { cursor: 1, parts_left: 10, bytes_left: 1 << 20, rows_left: 10, part_cap: 1 << 20, ..PictureBook::default() };
            let mut work = WriteWork::default();
            book.add(&arc(&bytes), PNG, &limits, &mut work).unwrap();
            work.scan_bytes = 0;
            work.bytes_copied = copied;
            match book.add(&arc(&bytes), PNG, &limits, &mut work) {
                Err(WriteError::Limit { what, max: 200 }) => Some(what),
                Err(other) => panic!("{other:?}"),
                Ok(_) => None,
            }
        };
        assert_eq!(words(100), None, "at the ceiling");
        assert!(words(101).is_some_and(|w| w.contains("compared")), "one past it");
        // The hash's edge is the scan's: a ceiling of 99 refuses a picture of 100 bytes in the words of the hash.
        let low = Limits { max_write_bytes: 99, ..Limits::default() };
        let mut work = WriteWork::default();
        let mut big = vec![0x89, b'P', b'N', b'G'];
        big.resize(100, 9);
        assert!(matches!(book.add(&arc(&big), PNG, &low, &mut work), Err(WriteError::Limit { what, .. }) if what.contains("looked at")));
    }

    /// The package made is refused as the cap it passed says: the ratio is the picture's own fault, the entries, the bytes and the rows of the manifest are limits, and any other
    /// refusal is a fault of the save.
    #[test]
    fn what_the_package_made_refused_as_is_said_in_the_words_of_its_cause() {
        use crate::{LimitKind, OdgError};
        let limits = Limits { max_manifest_entries: 7, ..Limits::default() };
        let why = |e: OdgError| made_error(&e, &limits);
        assert!(matches!(
            why(OdgError::Limit { kind: LimitKind::ManifestEntries, max: 7 }),
            WriteError::Limit { what: "rows of a manifest", max: 7 }
        ));
        let class = |e: OdgError| why(e).class();
        assert_eq!(class(OdgError::Limit { kind: LimitKind::ManifestEntries, max: 7 }), "write_limit");
        assert_eq!(class(OdgError::EncryptedPackage), "write_self_check", "any other refusal is the save's own fault");
    }

    /// A picture link is broken when it is a path in the package that names no entry, in every spelling the reader resolves; a link that is not a path in the package is not (the
    /// forms LibreOffice opens), nor one at `ObjectReplacements/`.
    #[test]
    fn a_picture_link_is_broken_when_it_is_a_path_in_the_package_that_names_no_entry() {
        let index: std::collections::HashMap<String, usize> =
            [("Pictures/a.png", 0), ("Pictures/", 1), ("Pictures/b c.png", 2)].into_iter().map(|(n, i)| (n.to_owned(), i)).collect();
        for held in [
            "Pictures/a.png",
            "./Pictures/a.png",
            "%2E/Pictures/a.png",
            ".%2FPictures/a.png",
            "Pictures/%61.png",
            "Pictures/b%20c.png",
            "Pictures/",
            "./Pictures/b c.png",
        ] {
            assert!(!is_broken_link(held, &index), "{held} names an entry");
        }
        for broken in [
            "Pictures/gone.png",
            "./Pictures/gone.png",
            "%2E/Pictures/gone.png",
            "gone.png",
            "./Object 1",
            "Pictures/sub/a.png",
            "pictures/a.png",
            "Pictures/a.PNG",
            "Pictures/a.png/",
        ] {
            assert!(is_broken_link(broken, &index), "{broken} names no entry");
        }
        for outside in [
            "",
            "http://example.org/x.png",
            "https://example.org/Pictures/a.png",
            "file:///tmp/x.png",
            "mailto:someone@example.org",
            "C:/x.png",
            "/tmp/x.png",
            "#Page 2",
            "../x.png",
            "..",
            "./../x.png",
            "Pictures/../x.png",
            "ObjectReplacements/Object 1",
            "./ObjectReplacements/Object 1",
            "%2E/ObjectReplacements/Object%201",
        ] {
            assert!(!is_broken_link(outside, &index), "{outside} is no path of the package's");
        }
        // What only looks like a scheme is a path: a colon after a slash is a file name's.
        assert!(is_broken_link("Pictures/a:b.png", &index));
        assert!(is_broken_link("1x:y.png", &index), "a scheme begins with a letter");
    }

    /// The index of the entries costs the bytes of each name and a slot for it, and none for no entry.
    #[test]
    fn the_index_of_the_entries_costs_each_name_and_its_slot() {
        assert_eq!(index_cost(std::iter::empty()), 0);
        assert_eq!(index_cost(["a", "bb"].into_iter()), 1 + NAME_BYTES + 2 + NAME_BYTES);
        assert_eq!(index_cost(["Pictures/image1.png"].into_iter()), 19 + NAME_BYTES);
    }
}
