//! The font database: which face draws a name.
//!
//! [`FontDb::pick`] always answers. A name the machine has gets its own face. A name it lacks gets the stand-in for that name,
//! then the default stand-in for its kind of face, then a bundled face, so text is never left undrawn.
//! At each step the faces are searched in this order: those the caller added, the craft-fonts faces built into the binary,
//! the machine's, and the bundled Hack and Ubuntu Light last. Nothing here is ever a Microsoft face's metrics from memory: see
//! [`standin`](crate::standin).
//!
//! The database is built once and shared. A face is parsed the first time it is wanted, behind a `OnceLock`, so lookups take `&self`
//! and the database is `Send + Sync`. A file that can't be read, or a font whose tables are damaged, is skipped for good the first
//! time it is tried, with one log line; it never fails a lookup.
//!
//! **What a search for a fallback costs and keeps.** [`FontDb::covering_checked`] reads each face's `cmap` table once (not the
//! whole file) into a list of ranges, so it holds a few kilobytes a face and loads nothing but the face it finds. It looks at faces
//! in one fixed order that does not depend on what is loaded, so a text lays out the same in any session. A database holds at most
//! 1 GiB of whole font files (for the faces it draws with) and has read at most 512 MiB of `cmap` tables; past either, a search
//! says it was cut short ([`Cover::Cut`]) and does not remember that as an answer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::CRAFT_FONTS;
use crate::catalog::{Budgets, Cov, Entry, Hint, MAX_ENTRIES, Origin, entries_from_bytes, hint};
use crate::face::{Face, FontBytes};
use crate::standin::{GenericClass, class_defaults, classify, is_no_font, key, stand_ins};

/// What a request asks of a face's weight and slant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
}

/// How a request found its face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    /// A face of that name.
    Named,
    /// The stand-in for that name (`Carlito` for `Calibri`).
    StandIn,
    /// The default for its kind of face, or a bundled face: nothing better was found.
    Default,
}

/// A face for a request, and what to make up for where the face falls short.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub face: Face,
    /// The request was for bold and the face isn't: the renderer emboldens the outlines.
    pub synthetic_bold: bool,
    /// The request was for italic and the face isn't: the renderer slants the outlines.
    pub synthetic_italic: bool,
    pub kind: MatchKind,
}

/// What [`FontDb::add_font_data`] did with a font file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddReport {
    /// Faces added.
    pub added: usize,
    /// Faces, or whole files, that couldn't be read.
    pub rejected: usize,
}

/// The most characters whose coverage search is remembered.
const MAX_COVER_CACHE: usize = 4096;
/// The largest font a caller may add by bytes.
const MAX_ADDED_FONT: usize = 128 << 20;

/// The font database.
pub struct FontDb {
    entries: Vec<Entry>,
    /// Entries by normalised family name, best origin first.
    by_key: HashMap<String, Vec<usize>>,
    /// Every entry, best origin first: the order a search for a covering face goes in.
    order: Vec<usize>,
    bundled_sans: Option<usize>,
    bundled_mono: Option<usize>,
    budgets: Budgets,
    searches: AtomicUsize,
    /// Searches that ended with an answer: the entry found, or that none has the characters. A search that was cut short is not here.
    cover_cache: Mutex<HashMap<Vec<char>, Option<usize>>>,
}

/// What a search for a face that has some characters found.
#[derive(Clone, Debug)]
pub enum Cover {
    /// A face that has them all.
    Found(Face),
    /// Every face was looked at, and none has them all.
    Missing,
    /// Not every face could be looked at: the database has read all the `cmap` bytes it may, or holds all the font files it may, so
    /// a face that has them may exist. Not remembered, and not the same as `Missing`.
    Cut,
}

/// Characters that never decide which face draws a cluster: spaces, controls, joiners and variation selectors.
pub fn is_ignorable(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF)
}

impl FontDb {
    /// A database with no faces. [`FontDb::pick`] on it answers with the face that has no glyphs.
    pub fn empty() -> FontDb {
        FontDb::build(Vec::new())
    }

    /// The craft-fonts faces built into the binary, if any, and the bundled Hack and Ubuntu Light: the same on every machine.
    pub fn bundled() -> FontDb {
        FontDb::build(FontDb::built_in())
    }

    /// The built-in faces and the ones installed on the machine, found by reading its font folders. Reading them costs a few
    /// kilobytes a file, once; share the database rather than building another. On `wasm32` there are no folders, so this is
    /// [`FontDb::bundled`].
    pub fn system_and_bundled() -> FontDb {
        FontDb::with_font_dirs(&crate::system::font_dirs())
    }

    /// The built-in faces and the font files under `dirs` (ignored on `wasm32`, which has no file system). The scan stops after
    /// eight seconds with the faces it has; see the `system` module for what a folder can make it do.
    pub fn with_font_dirs(dirs: &[PathBuf]) -> FontDb {
        let mut entries = FontDb::built_in();
        crate::system::scan(dirs, Origin::System, MAX_ENTRIES, &mut entries);
        FontDb::build(entries)
    }

    /// As [`FontDb::with_font_dirs`] with a time of its own for the scan, for tests that cannot wait for eight seconds.
    #[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
    #[doc(hidden)]
    pub fn with_font_dirs_for(dirs: &[PathBuf], scan_time: std::time::Duration) -> FontDb {
        let mut entries = FontDb::built_in();
        crate::system::scan_for(dirs, Origin::System, MAX_ENTRIES, &mut entries, scan_time);
        FontDb::build(entries)
    }

    /// The craft-fonts faces, then the bundled ones. A face whose bytes don't parse is dropped with a log line.
    fn built_in() -> Vec<Entry> {
        let mut entries = Vec::new();
        for f in CRAFT_FONTS {
            let (found, rejected) = entries_from_bytes(FontBytes::Static(f.bytes), Origin::Craft);
            if rejected > 0 {
                log::warn!("craft-fonts face {} {} could not be read and is left out", f.family, f.style);
            }
            entries.extend(found);
        }
        for bytes in [epaint_default_fonts::UBUNTU_LIGHT, epaint_default_fonts::HACK_REGULAR] {
            entries.extend(entries_from_bytes(FontBytes::Static(bytes), Origin::Bundled).0);
        }
        entries
    }

    fn build(mut entries: Vec<Entry>) -> FontDb {
        entries.truncate(MAX_ENTRIES);
        let mut db = FontDb {
            entries,
            by_key: HashMap::new(),
            order: Vec::new(),
            bundled_sans: None,
            bundled_mono: None,
            budgets: Budgets::default(),
            searches: AtomicUsize::new(0),
            cover_cache: Mutex::new(HashMap::new()),
        };
        db.index();
        db
    }

    /// Rebuild the lookup tables after the entries change.
    fn index(&mut self) {
        self.by_key.clear();
        self.order = (0..self.entries.len()).collect();
        // Best origin first; within one origin, the order they were catalogued in.
        self.order.sort_by_key(|i| (self.entries.get(*i).map(|e| e.origin), *i));
        for &i in &self.order {
            let Some(e) = self.entries.get(i) else { continue };
            for k in &e.keys {
                self.by_key.entry(k.clone()).or_default().push(i);
            }
        }
        let bundled = |db: &FontDb, mono: bool| {
            db.order.iter().copied().find(|i| db.entries.get(*i).is_some_and(|e| e.origin == Origin::Bundled && (e.info.family == "Hack") == mono))
        };
        self.bundled_sans = bundled(self, false);
        self.bundled_mono = bundled(self, true);
    }

    /// Add the faces in a font file or collection held in memory, as the highest priority faces there are. This is how a
    /// browser shell supplies fonts, and how a document's embedded fonts would be added. Files over 128 MiB, and anything that
    /// isn't a font, are rejected, and a damaged font never panics or poisons the database.
    pub fn add_font_data(&mut self, data: Vec<u8>) -> AddReport {
        if data.len() > MAX_ADDED_FONT || self.entries.len() >= MAX_ENTRIES {
            return AddReport { added: 0, rejected: 1 };
        }
        let (mut found, rejected) = entries_from_bytes(FontBytes::Shared(Arc::new(data)), Origin::Added);
        found.truncate(MAX_ENTRIES - self.entries.len());
        let added = found.len();
        self.entries.extend(found);
        self.index();
        self.cover_cache.lock().unwrap_or_else(PoisonError::into_inner).clear();
        AddReport { added, rejected }
    }

    /// How many searches of the whole catalogue [`FontDb::covering`] has made: an answer it remembered is not one.
    pub fn cover_searches(&self) -> usize {
        self.searches.load(Ordering::Relaxed)
    }

    /// Change the two byte limits (whole files held, `cmap` bytes read), for tests that cannot make a gigabyte of fonts. Call it
    /// before the database is used.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_byte_limits(mut self, held: u64, indexed: u64) -> FontDb {
        self.budgets = Budgets::with_caps(held, indexed);
        self
    }

    /// Change the most ranges of code points kept from `cmap` tables, for the same tests. Call it before the database is used.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_range_limit(mut self, ranges: u64) -> FontDb {
        self.budgets = std::mem::take(&mut self.budgets).with_ranges_cap(ranges);
        self
    }

    /// Bytes of whole font files the database holds in memory now: the faces drawn with, each file once. At most 1 GiB.
    pub fn held_bytes(&self) -> u64 {
        self.budgets.held()
    }

    /// Bytes of `cmap` tables read so far to find fallback faces. At most 512 MiB.
    pub fn indexed_bytes(&self) -> u64 {
        self.budgets.indexed()
    }

    /// The number of faces catalogued.
    pub fn face_count(&self) -> usize {
        self.entries.len()
    }

    /// The family names catalogued, sorted, each once (typographic names; a legacy name is found but not listed).
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries.iter().map(|e| e.info.family.clone()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Is there a face of this name (any case)? Stand-ins don't count.
    pub fn has_family(&self, family: &str) -> bool {
        self.by_key.contains_key(&key(family))
    }

    /// The face that draws `family` in `style`. Always a face: see [`FontDb::resolve`].
    pub fn pick(&self, family: &str, style: FontStyle) -> Face {
        self.resolve(family, style).face
    }

    /// The family of the face a run is drawn in when no cell gives it a face (`""`, which [`FontDb::resolve`] answers with the default of the
    /// sans class: the first of Carlito, Calibri, Liberation Sans, Arimo and Arial the database holds, else a face whose name says it is sans,
    /// else a bundled face). The name a window shows for such a run, so that what it shows is what is drawn: one lookup of the same kind a layout makes.
    /// Regular, bold and italic faces of one family share the name. `None` for a database that holds no face at all.
    pub fn default_family(&self) -> Option<String> {
        let face = self.resolve("", FontStyle::default()).face;
        (!face.is_null() && !face.family().is_empty()).then(|| face.family().to_owned())
    }

    /// The face for a name, and how it was found. A face of that name if the machine has one, else the stand-in for that name,
    /// else the default for its kind of face, else a bundled face. If nothing can be loaded at all, the face with no glyphs.
    /// A missing bold or italic is made up by the renderer, as the flags say.
    pub fn resolve(&self, family: &str, style: FontStyle) -> Resolved {
        let k = key(family);
        let class = if is_no_font(&k) { GenericClass::Sans } else { classify(&k) };
        let named = [k.as_str()];
        let steps: [(&[&str], MatchKind); 3] =
            [(&named[..], MatchKind::Named), (stand_ins(&k), MatchKind::StandIn), (class_defaults(class), MatchKind::Default)];
        for (names, kind) in steps {
            if is_no_font(&k) && kind != MatchKind::Default {
                continue;
            }
            for name in names {
                if let Some(r) = self.by_key.get(*name).and_then(|c| self.choose(c, style, kind)) {
                    return r;
                }
            }
        }
        // Before the bundled faces, a face the caller or the machine has whose name says it is of this class (`Foo Sans`).
        if let Some(r) = self.of_class(class, style) {
            return r;
        }
        let last = if class == GenericClass::Mono { [self.bundled_mono, self.bundled_sans] } else { [self.bundled_sans, self.bundled_mono] };
        let found = last.into_iter().flatten().chain(self.order.iter().copied()).find_map(|i| self.choose(&[i], style, MatchKind::Default));
        found.unwrap_or_else(|| Resolved { face: Face::null(), synthetic_bold: false, synthetic_italic: false, kind: MatchKind::Default })
    }

    /// The best face that is not a bundled one and whose family name says it is of `class`, if there is one.
    fn of_class(&self, class: GenericClass, style: FontStyle) -> Option<Resolved> {
        let first = self.order.iter().copied().find(|&i| {
            self.entries.get(i).is_some_and(|e| e.origin != Origin::Bundled && e.class == Some(class) && e.face(&self.budgets).is_some())
        })?;
        // The whole family of that face, so that bold and italic are found where it has them.
        let family = self.entries.get(first)?.keys.first()?;
        let same: Vec<usize> =
            self.by_key.get(family)?.iter().copied().filter(|i| self.entries.get(*i).is_some_and(|e| e.origin != Origin::Bundled)).collect();
        self.choose(&same, style, MatchKind::Default)
    }

    /// The best of `candidates` for a style that loads, with what has to be made up.
    fn choose(&self, candidates: &[usize], style: FontStyle, kind: MatchKind) -> Option<Resolved> {
        let target = if style.bold { 700i32 } else { 400 };
        let mut ranked: Vec<usize> = candidates.to_vec();
        // A mismatch of weight or slant costs one each; ties keep the origin order the candidates came in (a stable sort).
        ranked.sort_by_key(|i| {
            self.entries.get(*i).map_or((u8::MAX, i32::MAX), |e| {
                let mismatch = u8::from(e.info.bold != style.bold) + u8::from(e.info.italic != style.italic);
                (mismatch, (i32::from(e.info.weight) - target).abs())
            })
        });
        ranked.into_iter().find_map(|i| {
            let face = self.entries.get(i)?.face(&self.budgets)?.clone();
            let (synthetic_bold, synthetic_italic) = (style.bold && !face.is_bold(), style.italic && !face.is_italic());
            Some(Resolved { face, synthetic_bold, synthetic_italic, kind })
        })
    }

    /// A face that has a glyph for every character of `cluster` (spaces, controls and joiners aside), for a cluster the faces
    /// asked for lack, or `None`: see [`FontDb::covering_checked`] to tell that no face has it from not having been able to look.
    pub fn covering(&self, cluster: &[char]) -> Option<Face> {
        match self.covering_checked(cluster) {
            Cover::Found(face) => Some(face),
            Cover::Missing | Cover::Cut => None,
        }
    }

    /// Search for a face that has every character of `cluster` (spaces, controls and joiners aside). Each face's `cmap` is read
    /// once, not the file, and the faces are looked at in one fixed order whatever is loaded: first those whose `OS/2` ranges say they
    /// have the block, then those that say nothing, then those whose ranges leave it out (the ranges are a hint: 15 faces on one
    /// machine hold Han with the bit clear). Within each, the order is the one lookups use: callers' faces, craft-fonts, the
    /// machine's, then the bundled.
    pub fn covering_checked(&self, cluster: &[char]) -> Cover {
        // What decides the answer is the characters that need a glyph; the same ones always get the same answer, so an answer is
        // remembered (for clusters of up to eight such characters), and a text of one uncovered cluster repeated costs one search.
        let wanted: Vec<char> = cluster.iter().copied().filter(|c| !is_ignorable(*c)).collect();
        if wanted.is_empty() {
            return Cover::Missing;
        }
        let cacheable = wanted.len() <= 8;
        if cacheable && let Some(hit) = self.cover_cache.lock().unwrap_or_else(PoisonError::into_inner).get(&wanted).copied() {
            return match hit {
                None => Cover::Missing,
                Some(i) => match self.entries.get(i).and_then(|e| e.face(&self.budgets)) {
                    Some(face) => Cover::Found(face.clone()),
                    None => Cover::Cut,
                },
            };
        }
        self.searches.fetch_add(1, Ordering::Relaxed);
        let (found, cut) = self.search_covering(&wanted);
        let answer = match found {
            Some(i) => self.entries.get(i).and_then(|e| e.face(&self.budgets)).map(|f| (i, f.clone())),
            None => None,
        };
        if cacheable && !cut {
            let mut cache = self.cover_cache.lock().unwrap_or_else(PoisonError::into_inner);
            if cache.len() < MAX_COVER_CACHE {
                cache.insert(wanted, answer.as_ref().map(|(i, _)| *i));
            }
        }
        match answer {
            Some((_, face)) => Cover::Found(face),
            None if cut => Cover::Cut,
            None => Cover::Missing,
        }
    }

    /// The first entry in the fixed order whose coverage has all of `wanted` and that loads, and whether a face that might have
    /// them could not be looked at.
    fn search_covering(&self, wanted: &[char]) -> (Option<usize>, bool) {
        let Some(&first) = wanted.first() else { return (None, false) };
        let mut cut = false;
        for pass in [Hint::Declared, Hint::Silent, Hint::Denied] {
            for &i in &self.order {
                let Some(e) = self.entries.get(i) else { continue };
                if hint(&e.info.ranges, first) != pass {
                    continue;
                }
                match e.coverage(&self.budgets) {
                    Cov::Known(c) if wanted.iter().all(|ch| c.contains(*ch)) => {
                        if e.face(&self.budgets).is_some() {
                            return (Some(i), cut);
                        }
                        // A face that is damaged is skipped for good; one that can't be held right now may be wanted later.
                        cut |= !e.face_failed();
                    }
                    Cov::Known(_) | Cov::Unreadable => {}
                    Cov::OverBudget => cut = true,
                }
            }
        }
        (None, cut)
    }
}
