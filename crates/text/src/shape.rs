//! A paragraph's cells to clusters with a face, a size and a width: grapheme clusters, bidirectional levels, the face of each cluster
//! by script, and shaping.
//!
//! A *cluster* is a grapheme cluster, a base character with its marks, and it is never split across faces. Each gets the face of its
//! script's slot (`Font`, `AsianFont` or `ComplexScriptFont`), then any face that has all its glyphs. Shaping is done on whole
//! stretches of clusters of one face, direction and script, whatever their sizes and colours, so a format change in a word never
//! breaks Arabic joining; sizes, scales and letterspace are applied afterwards to the shaper's font-unit advances. Shaping is at
//! nominal advances, and there is a budget of calls to it (see [`limits`](crate::limits)).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use diacraft_fonts::script::{needs_shaping, script_of, slot_of_script};
use diacraft_fonts::{Cover, Face, FontDb, FontSlot, FontStyle, MatchKind, ScriptTag, ShapeParams, ShapedGlyph, is_ignorable};
use unicode_segmentation::UnicodeSegmentation;

use crate::bidi::cell_levels;
use crate::formats::{RunStyle, no_name};
use crate::laid::Note;
use crate::limits::{MAX_CLUSTER_CHARS, MAX_FACE_SEARCHES, MAX_NOTES, MAX_SHAPE_ITEMS};
use crate::prepare::{Cell, Kind, Paragraph};

/// What raised and lowered letters do where the face's `OS/2` table has no box or holds 0: size and offset as shares of the
/// size. A project rule, from the SVG export's classes.
pub(crate) const DEFAULT_SUPERSCRIPT: (f64, f64) = (0.65, 0.45);
pub(crate) const DEFAULT_SUBSCRIPT: (f64, f64) = (0.65, 0.14);
/// A lower-case letter drawn as a small capital is this share of the size.
pub(crate) const SMALL_CAP_SIZE: f64 = 0.8;

/// One glyph with its advance and offsets in inches, already scaled by size and `FontScale`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyph {
    pub gid: u32,
    pub adv: f64,
    pub dx: f64,
    pub dy: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct Cluster {
    pub cells: Range<usize>,
    pub kind: Kind,
    pub run: u32,
    pub level: u8,
    pub face: usize,
    /// The em the glyphs draw at, after raised or lowered letters and small capitals.
    pub size: f64,
    /// The size counted for the line's height: the `Size` cell, or the complex script size.
    pub line_size: f64,
    pub x_scale: f64,
    /// How far the baseline moves for a raised or lowered letter, up positive.
    pub y_off: f64,
    pub letterspace: f64,
    /// The advance with letterspace. For a tab, one space's advance, kept for a tab that has no stop to go to.
    pub adv: f64,
    /// For a soft hyphen: the width of the hyphen drawn if the line breaks here.
    pub hyphen: f64,
    pub glyphs: Range<usize>,
    /// A space a line may end with and a justified line may stretch.
    pub space: bool,
    /// Where the cluster starts in the paragraph's text, in bytes.
    pub byte: usize,
}

/// A face with what has to be made up for it.
#[derive(Clone, Debug)]
pub(crate) struct FaceEntry {
    pub face: Face,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Shaped {
    pub clusters: Vec<Cluster>,
    pub glyphs: Vec<Glyph>,
    /// The paragraph's characters, for finding where lines may break.
    pub text: String,
}

/// Everything shared by the paragraphs of one layout: the faces looked up, the shaping budget, what to tell the caller.
pub(crate) struct Shaping<'a, 'r> {
    pub db: &'a FontDb,
    pub styles: &'a [RunStyle<'r>],
    pub entries: Vec<FaceEntry>,
    by_request: HashMap<(&'r str, bool, bool), usize>,
    by_cover: HashMap<(u64, bool, bool), usize>,
    cover_memo: HashMap<(Vec<char>, bool, bool), Option<usize>>,
    noted: HashSet<&'r str>,
    pub notes: Vec<Note>,
    items_left: usize,
    searches_left: usize,
    pub marks_cut: bool,
    pub budget_used: bool,
    pub search_cut: bool,
    /// Stand-in notes left out because [`MAX_NOTES`] was reached.
    pub standins_omitted: usize,
    /// Text clusters that draw a face's missing-glyph box (or nothing, where the face has none).
    pub missing_glyphs: usize,
    /// Faces whose shaper could not be used, by [`Face::id`]: their text was set from the cmap alone.
    pub shaping_failed: HashSet<u64>,
    /// A bullet string was cut at the most characters a bullet takes.
    pub bullet_cut: bool,
}

/// What one cluster needs to pick a face.
struct Request {
    slot: FontSlot,
    run: u32,
}

/// Breaking spaces: those a line may end with and a justified line stretches. A no-break space is not one.
pub(crate) fn is_break_space(c: char) -> bool {
    c == ' ' || matches!(c, '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}' | '\u{205F}' | '\u{3000}')
}

impl<'a, 'r> Shaping<'a, 'r> {
    pub fn new(db: &'a FontDb, styles: &'a [RunStyle<'r>]) -> Shaping<'a, 'r> {
        Shaping {
            db,
            styles,
            entries: Vec::new(),
            by_request: HashMap::new(),
            by_cover: HashMap::new(),
            cover_memo: HashMap::new(),
            noted: HashSet::new(),
            notes: Vec::new(),
            items_left: MAX_SHAPE_ITEMS,
            searches_left: MAX_FACE_SEARCHES,
            marks_cut: false,
            budget_used: false,
            search_cut: false,
            standins_omitted: 0,
            missing_glyphs: 0,
            shaping_failed: HashSet::new(),
            bullet_cut: false,
        }
    }

    fn style(&self, run: u32) -> Option<&RunStyle<'r>> {
        self.styles.get(run as usize)
    }

    /// The face entry for a name, a weight and a slant, made once. A name the machine lacks is noted once.
    fn entry(&mut self, name: &'r str, bold: bool, italic: bool) -> usize {
        if let Some(i) = self.by_request.get(&(name, bold, italic)) {
            return *i;
        }
        let r = self.db.resolve(name, FontStyle { bold, italic });
        if r.kind != MatchKind::Named && !no_name(name) && self.noted.insert(name) {
            if self.notes.len() < MAX_NOTES {
                self.notes.push(Note::StandIn { requested: name.to_owned(), used: r.face.family().to_owned() });
            } else {
                self.standins_omitted += 1;
            }
        }
        self.entries.push(FaceEntry { face: r.face, bold: r.synthetic_bold, italic: r.synthetic_italic });
        let i = self.entries.len() - 1;
        self.by_request.insert((name, bold, italic), i);
        i
    }

    /// The entry for a face found by coverage, with the weight and slant asked for.
    fn covering_entry(&mut self, face: Face, bold: bool, italic: bool) -> usize {
        let key = (face.id(), bold, italic);
        if let Some(i) = self.by_cover.get(&key) {
            return *i;
        }
        let (b, it) = (bold && !face.is_bold(), italic && !face.is_italic());
        self.entries.push(FaceEntry { face, bold: b, italic: it });
        let i = self.entries.len() - 1;
        self.by_cover.insert(key, i);
        i
    }

    /// The entry for a face by name, in the regular weight and slant: for a bullet's mark.
    pub fn entry_for(&mut self, name: &'r str) -> usize {
        self.entry(name, false, false)
    }

    /// The entry for a face found some other way.
    pub fn entry_for_face(&mut self, face: Face) -> usize {
        self.covering_entry(face, false, false)
    }

    fn covers(&self, entry: usize, chars: &[char]) -> bool {
        self.entries.get(entry).is_some_and(|e| chars.iter().all(|c| is_ignorable(*c) || e.face.covers(*c)))
    }

    /// The face for one cluster: the first of the slot's face and `Font` that has all its glyphs, else any face that does, else the
    /// first of them, which draws its missing-glyph box.
    fn face_for(&mut self, rq: &Request, chars: &[char]) -> usize {
        let Some(style) = self.style(rq.run).cloned() else { return self.entry("", false, false) };
        let (bold, italic) = match rq.slot {
            FontSlot::Complex => (style.complex_bold, style.complex_italic),
            _ => (style.bold, style.italic),
        };
        let slot_name = match rq.slot {
            FontSlot::Asian => style.asian,
            FontSlot::Complex => style.complex,
            FontSlot::Latin => "",
        };
        let mut chain = [None, None];
        if !no_name(slot_name) {
            chain[0] = Some(self.entry(slot_name, bold, italic));
        }
        chain[1] = Some(self.entry(style.font, bold, italic));
        if let Some(i) = chain.into_iter().flatten().find(|i| self.covers(*i, chars)) {
            return i;
        }
        let fallback = chain.into_iter().flatten().next().unwrap_or(0);
        self.covering_face(chars, bold, italic).unwrap_or(fallback)
    }

    /// A face found by searching the whole database for one that has all of a cluster's glyphs. What was found, or that nothing was, is
    /// remembered for the layout, so a letter repeated costs one search and uses up one of the layout's searches, not one a time.
    fn covering_face(&mut self, chars: &[char], bold: bool, italic: bool) -> Option<usize> {
        let key = (chars.iter().copied().filter(|c| !is_ignorable(*c)).collect::<Vec<char>>(), bold, italic);
        if let Some(found) = self.cover_memo.get(&key) {
            return *found;
        }
        if self.searches_left == 0 {
            self.search_cut = true;
            return None;
        }
        self.searches_left -= 1;
        crate::work::face_search();
        let found = match self.db.covering_checked(&key.0) {
            Cover::Found(face) => Some(self.covering_entry(face, bold, italic)),
            Cover::Missing => None,
            // The database could not look at every face: the letter is drawn as a box, and the layout says it was cut short.
            Cover::Cut => {
                self.search_cut = true;
                None
            }
        };
        self.cover_memo.insert(key, found);
        found
    }

    /// The size a cluster draws at, how far its baseline moves, and the size its line counts.
    fn sizes(&self, style: &RunStyle<'r>, slot: FontSlot, face: usize, small_cap: bool) -> (f64, f64, f64) {
        let base = if slot == FontSlot::Complex { style.complex_size } else { style.size };
        let face = self.entries.get(face).map(|e| &e.face);
        let (mut size, y_off) = match style.pos {
            1 => {
                let (s, o) = face.and_then(|f| f.script_box(true)).map_or(DEFAULT_SUPERSCRIPT, |b| (b.size, b.offset));
                (base * s, base * o)
            }
            2 => {
                let (s, o) = face.and_then(|f| f.script_box(false)).map_or(DEFAULT_SUBSCRIPT, |b| (b.size, b.offset));
                (base * s, -(base * o))
            }
            _ => (base, 0.0),
        };
        if small_cap {
            size *= SMALL_CAP_SIZE;
        }
        (size, y_off, base)
    }
}

/// Split the cells into grapheme clusters, each at most [`MAX_CLUSTER_CHARS`] cells, as ranges of cells.
fn cluster_ranges(text: &str, cells: &[Cell], marks_cut: &mut bool) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut cell = 0usize;
    let mut byte = 0usize;
    for (_, g) in text.grapheme_indices(true) {
        let start = cell;
        let end_byte = byte + g.len();
        while cell < cells.len() && byte < end_byte {
            byte += cells.get(cell).map_or(1, |c| c.ch.len_utf8());
            cell += 1;
        }
        let end = if cell - start > MAX_CLUSTER_CHARS {
            *marks_cut = true;
            start + MAX_CLUSTER_CHARS
        } else {
            cell
        };
        out.push(start..end);
    }
    out
}

/// Where each cell starts in the paragraph's text, in bytes.
fn cell_bytes(cells: &[Cell]) -> Vec<usize> {
    let mut at = 0usize;
    cells
        .iter()
        .map(|c| {
            let here = at;
            at += c.ch.len_utf8();
            here
        })
        .collect()
}

/// The clusters of a paragraph with their faces and sizes, before shaping.
fn build_clusters(sh: &mut Shaping<'_, '_>, para: &Paragraph, text: &str, base_rtl: bool) -> Shaped {
    let mut shaped = Shaped { text: text.to_owned(), ..Shaped::default() };
    let ranges = cluster_ranges(text, &para.cells, &mut sh.marks_cut);
    let levels = cell_levels(text, &para.cells, base_rtl);
    let bytes = cell_bytes(&para.cells);
    let mut chars: Vec<char> = Vec::new();
    for r in ranges {
        let Some(first) = para.cells.get(r.start) else { continue };
        chars.clear();
        chars.extend(para.cells.get(r.clone()).unwrap_or(&[]).iter().map(|c| c.ch));
        let text_cluster = first.kind == Kind::Text;
        let slot = if text_cluster { chars.iter().find_map(|c| script_of(*c)).map_or(FontSlot::Latin, slot_of_script) } else { FontSlot::Latin };
        // A tab, a soft hyphen and a forced break take the face that draws the space or hyphen they stand for.
        let probe: &[char] = match first.kind {
            Kind::Text => &chars,
            Kind::SoftHyphen => &['-'],
            _ => &[' '],
        };
        let face = sh.face_for(&Request { slot, run: first.run }, probe);
        let Some(style) = sh.style(first.run).cloned() else { continue };
        let (size, y_off, line_size) = sh.sizes(&style, slot, face, first.small_cap);
        shaped.clusters.push(Cluster {
            cells: r.clone(),
            kind: first.kind,
            run: first.run,
            level: levels.get(r.start).copied().unwrap_or(0),
            face,
            size: if text_cluster { size } else { style.size },
            line_size,
            x_scale: style.scale,
            y_off,
            letterspace: style.letterspace,
            adv: 0.0,
            hyphen: 0.0,
            glyphs: 0..0,
            // A space with a mark on it is not a space a line may end with: the mark would be lost.
            space: text_cluster && r.len() == 1 && is_break_space(first.ch),
            byte: bytes.get(r.start).copied().unwrap_or(0),
        });
    }
    shaped
}

/// The shaper's glyphs of one stretch, put with the clusters they belong to and scaled to inches.
fn distribute(shaped: &mut Shaped, range: Range<usize>, starts: &[usize], glyphs: &[ShapedGlyph], upem: f64) {
    let mut placed: Vec<(usize, Glyph)> = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        let local = starts.partition_point(|b| *b <= g.cluster as usize).saturating_sub(1);
        let Some(c) = shaped.clusters.get(range.start + local) else { continue };
        let k = c.size * c.x_scale / upem;
        placed.push((
            range.start + local,
            Glyph { gid: g.gid, adv: f64::from(g.x_advance) * k, dx: f64::from(g.x_offset) * k, dy: f64::from(g.y_offset) * c.size / upem },
        ));
    }
    // Glyphs of one cluster are together, in the order the shaper gave them; a stable sort puts the clusters in logical order.
    placed.sort_by_key(|(c, _)| *c);
    for (ci, g) in placed {
        let at = shaped.glyphs.len();
        shaped.glyphs.push(g);
        if let Some(c) = shaped.clusters.get_mut(ci) {
            if c.glyphs.is_empty() {
                c.glyphs = at..at;
            }
            c.glyphs.end = at + 1;
        }
    }
}

/// Is this cluster to be shaped: a text cluster that draws (size above 0)?
fn shapeable(c: &Cluster) -> bool {
    c.kind == Kind::Text && c.size > 0.0
}

/// Shape the stretch of clusters `range` in one face, or measure it from the cmap if the budget of shaper calls is spent.
fn shape_item(sh: &mut Shaping<'_, '_>, shaped: &mut Shaped, cells: &[Cell], range: Range<usize>, script: Option<ScriptTag>, lang: &str) {
    let Some(first) = shaped.clusters.get(range.start) else { return };
    let (face_idx, level) = (first.face, first.level);
    let Some(face) = sh.entries.get(face_idx).map(|e| e.face.clone()) else { return };
    let mut text = String::new();
    let mut starts = Vec::with_capacity(range.len());
    for c in shaped.clusters.get(range.clone()).unwrap_or(&[]) {
        starts.push(text.len());
        text.extend(cells.get(c.cells.clone()).unwrap_or(&[]).iter().map(|cell| cell.ch));
    }
    let upem = face.units_per_em().max(1.0);
    let glyphs = if sh.items_left > 0 {
        sh.items_left -= 1;
        let params = ShapeParams { rtl: level % 2 == 1, script, language: (!lang.is_empty()).then_some(lang), nominal: true };
        crate::work::shaper_call();
        let (glyphs, fell_back) = face.shape_checked(&text, &params);
        if fell_back {
            sh.shaping_failed.insert(face.id());
        }
        glyphs
    } else {
        sh.budget_used = true;
        text.char_indices()
            .map(|(i, c)| {
                let gid = face.glyph_for(c);
                ShapedGlyph {
                    gid,
                    cluster: u32::try_from(i).unwrap_or(u32::MAX),
                    x_advance: face.advance(gid).round() as i32,
                    x_offset: 0,
                    y_offset: 0,
                }
            })
            .collect()
    };
    distribute(shaped, range, &starts, &glyphs, upem);
}

/// The script a shaper is told for each cluster: the complex script of its letters, or none (Latin, Greek, Cyrillic and East Asian
/// text is shaped from its own letters). A cluster of no script of its own, a space, a digit or a mark, takes the script of the text
/// before it, as UAX 24 itemises, so that one stretch of Arabic stays one item and a face joins across a space or a kashida.
fn item_scripts(shaped: &Shaped, cells: &[Cell]) -> Vec<Option<ScriptTag>> {
    let mut current: Option<ScriptTag> = None;
    shaped
        .clusters
        .iter()
        .map(|c| {
            if c.kind != Kind::Text {
                current = None;
            } else if let Some(own) = cells.get(c.cells.clone()).unwrap_or(&[]).iter().find_map(|cell| script_of(cell.ch)) {
                current = needs_shaping(own).then_some(own);
            }
            current
        })
        .collect()
}

/// The advance of `ch` in the cluster's face at its size: a tab's fallback width and a soft hyphen's mark.
fn special_advance(sh: &Shaping<'_, '_>, c: &Cluster, ch: char) -> f64 {
    let Some(e) = sh.entries.get(c.face) else { return 0.0 };
    let upem = e.face.units_per_em().max(1.0);
    e.face.advance(e.face.glyph_for(ch)) / upem * c.size * c.x_scale
}

/// Cluster advances: the shaper's glyph advances and letterspace, and for tabs and soft hyphens the widths they may need.
fn finish_widths(sh: &mut Shaping<'_, '_>, shaped: &mut Shaped) {
    let Shaped { clusters, glyphs, .. } = shaped;
    for c in clusters.iter_mut() {
        let own = glyphs.get(c.glyphs.clone()).unwrap_or(&[]);
        if c.kind == Kind::Text && c.size > 0.0 && own.iter().any(|g| g.gid == 0) {
            sh.missing_glyphs += 1;
        }
        let glyph_sum: f64 = own.iter().map(|g| g.adv).sum();
        match c.kind {
            Kind::Text if c.size > 0.0 => c.adv = glyph_sum + c.letterspace,
            Kind::Tab => c.adv = special_advance(sh, c, ' '),
            Kind::SoftHyphen => c.hyphen = special_advance(sh, c, '-'),
            _ => {}
        }
    }
}

/// Shape a paragraph: its clusters, with widths.
pub(crate) fn shape_paragraph(sh: &mut Shaping<'_, '_>, para: &Paragraph, base_rtl: bool) -> Shaped {
    let text: String = para.cells.iter().map(|c| c.ch).collect();
    let mut shaped = build_clusters(sh, para, &text, base_rtl);
    // Items: runs of clusters in one face, direction, language and, for complex scripts, script.
    let scripts = item_scripts(&shaped, &para.cells);
    let mut i = 0;
    while i < shaped.clusters.len() {
        let Some(c) = shaped.clusters.get(i) else { break };
        if !shapeable(c) {
            i += 1;
            continue;
        }
        let key = (c.face, c.level, scripts.get(i).copied().flatten(), sh.style(c.run).map_or("", |s| s.lang));
        let mut j = i + 1;
        while let Some(n) = shaped.clusters.get(j) {
            if !shapeable(n) || (n.face, n.level, scripts.get(j).copied().flatten(), sh.style(n.run).map_or("", |s| s.lang)) != key {
                break;
            }
            j += 1;
        }
        shape_item(sh, &mut shaped, &para.cells, i..j, key.2, key.3);
        i = j;
    }
    finish_widths(sh, &mut shaped);
    shaped
}
