//! One font face: its names, the numbers a layout reads from its tables, glyph outlines and shaping.
//!
//! A [`Face`] is cheap to clone (an `Arc`). It owns its bytes, which come from the binary (bundled or craft-fonts faces), from a
//! file read when the face was first wanted, or from memory a caller added. It holds one `skrifa` [`Font`], which is the font
//! `harfrust` shapes, so the tables are parsed once and what the shaper prepares from them is kept with the rest. Every table read
//! goes through `skrifa` and `harfrust`, which bounds-check what they read, and a face whose tables won't parse is never made:
//! [`Face::new`] returns `None`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use harfrust::{Buffer, Direction, Feature, Language, Script, ShapeOptions, ShaperFont, Tag};
use kurbo::BezPath;
use skrifa::font::attribute::Style;
use skrifa::font::name::NameId;
use skrifa::font::{Blob, Kind};
use skrifa::raw::tables::name::{Encoding, Name, NameRecord, NameString, language_id_to_bcp47};
use skrifa::raw::types::F48Dot16;
use skrifa::raw::{FontData, TableProvider};
use skrifa::{DrawMemory, Font, GlyphId, PathSink, ScalerFont};

use crate::script::{ScriptTag, default_language};

/// The longest family or style name kept from a face's name table, in characters. A hostile font may hold a megabyte.
const MAX_NAME_CHARS: usize = 256;

/// The first characters of a version 1 `name` table's language tag that are read. `en-US` is five; one more tells it from a longer tag that
/// starts with it. A tag may be 32,767 characters long, and the reader's own check of one reads all of them (for each record that
/// names it, of which there can be 65,535), so the tag is read here as far as the choice of a name needs and no further. A tag whose
/// seventh or later character is not ASCII is therefore a language that is not English, where the reader takes it for one that names none;
/// that only decides between the first of the records and one that names no language, in a font that has no English name.
const LANGUAGE_TAG_PREFIX: usize = 6;

/// Where a face's bytes live.
#[derive(Clone)]
pub(crate) enum FontBytes {
    Static(&'static [u8]),
    Shared(Arc<Vec<u8>>),
}

impl FontBytes {
    fn get(&self) -> &[u8] {
        match self {
            FontBytes::Static(b) => b,
            FontBytes::Shared(b) => b.as_slice(),
        }
    }

    /// The same bytes as the font reader's data. It shares them: nothing is copied.
    pub(crate) fn blob(&self) -> Blob {
        match self {
            FontBytes::Static(b) => Blob::Static(b),
            FontBytes::Shared(b) => Blob::Shared(b.clone()),
        }
    }
}

/// The recommended size and offset of raised or lowered letters, as shares of the em, from the `OS/2` table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptBox {
    /// `ySubscriptYSize` or `ySuperscriptYSize` over the units per em.
    pub size: f64,
    /// `ySubscriptYOffset` or `ySuperscriptYOffset` over the units per em. A lowered offset is measured downwards, a raised one upwards.
    pub offset: f64,
}

/// A line the face recommends: the underline from the `post` table, the strikeout from `OS/2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    /// The distance from the baseline to the top of the line, as a share of the em. Below the baseline it is negative.
    pub top: f64,
    /// The line's thickness, as a share of the em.
    pub thickness: f64,
}

/// One glyph the shaper produced, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapedGlyph {
    pub gid: u32,
    /// The byte offset in the shaped string of the cluster the glyph belongs to.
    pub cluster: u32,
    pub x_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

/// How to shape one stretch of text.
#[derive(Clone, Copy, Debug)]
pub struct ShapeParams<'a> {
    /// Right to left: an odd bidi level. The glyphs come back in visual order.
    pub rtl: bool,
    /// The script, or `None` to let the shaper guess it from the text.
    pub script: Option<ScriptTag>,
    /// A BCP 47 language, or `None` for the script's default.
    pub language: Option<&'a str>,
    /// Draw at nominal advances: no kerning and no optional ligatures. A line's width is then the sum of its characters'
    /// advances, which is what measured. A shaper's required forms, a mark's place and a
    /// script's joining stay on.
    pub nominal: bool,
}

impl Default for ShapeParams<'_> {
    fn default() -> Self {
        ShapeParams { rtl: false, script: None, language: None, nominal: true }
    }
}

/// The features a nominal shaping turns off.
fn nominal_features() -> [Feature; 4] {
    [b"kern", b"liga", b"clig", b"calt"].map(|t| Feature::new(Tag::new(t), 0, ..))
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Runs `f` on font data, and gives `None` if it panics.
///
/// `read-fonts` and `harfrust` bounds-check what they read, but a damaged font has made one of them panic (`read-fonts` 0.41.0,
/// in the `glyf` reader, on a glyph with no points), and a drawing must not be lost to a bad font file on the machine. So every
/// call into them from here is contained, and a face that fails answers as a face with nothing in it: no glyph, no outline. The
/// panic message reaches standard error through the panic hook, once: [`Face`] stops calling a font that panicked (see
/// [`Face::is_broken`]). On `wasm32` a panic aborts and nothing can catch it, so there only faces the shell chooses to add can
/// reach this.
pub(crate) fn contained<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

struct FaceInner {
    id: u64,
    family: String,
    style: String,
    bold: bool,
    italic: bool,
    weight: u16,
    bytes: FontBytes,
    index: u32,
    upem: u16,
    subscript: Option<ScriptBox>,
    superscript: Option<ScriptBox>,
    underline: Option<Stroke>,
    strikeout: Option<Stroke>,
    ascent: f64,
    descent: f64,
    /// The font, read once: the names, the character map, the metrics, the outlines and the shaper all read this one, and it keeps what
    /// they prepare from the tables (the layout tables the shaper builds at the first shape). `None` only for the face with nothing in it.
    font: Option<Font>,
    /// Draws outlines from `font` in font units, set up when the first outline is asked for.
    scaler: OnceLock<ScalerFont>,
    /// The Basic Multilingual Plane characters the cmap maps, one bit each, built on first use.
    bmp: OnceLock<Box<[u64]>>,
    /// A face with no glyphs at all, made when nothing else could be: it measures half an em a character and draws nothing.
    null: bool,
    /// A call into the font tables panicked once: the face is not used again, so a damaged font costs one panic and not one a call.
    broken: AtomicBool,
}

/// A loaded font face. Clone it freely.
#[derive(Clone)]
pub struct Face(Arc<FaceInner>);

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Face({} {})", self.0.family, self.0.style)
    }
}

impl PartialEq for Face {
    fn eq(&self, other: &Face) -> bool {
        self.0.id == other.0.id
    }
}

/// What a `name` record says of its language, as far as choosing a name goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordLanguage {
    UsEnglish,
    English,
    Other,
    /// No language the table can name, or a tag that is not ASCII.
    Unsaid,
}

/// The language of `record`: by its language id, or by the tag of a version 1 table that a high id points at, of which
/// [`LANGUAGE_TAG_PREFIX`] characters are read.
fn language_of(table: &Name<'_>, record: &NameRecord, strings: FontData<'_>) -> RecordLanguage {
    let id = record.language_id();
    if table.version() == 1 && id >= 0x8000 {
        let tag = table.lang_tag_record().and_then(|tags| tags.get(usize::from(id - 0x8000)));
        let Some(tag) = tag.and_then(|tag| tag.lang_tag(strings).ok()) else { return RecordLanguage::Unsaid };
        crate::work::language_tag();
        let mut head = [0u8; LANGUAGE_TAG_PREFIX];
        let mut len = 0;
        for c in tag.chars().take(LANGUAGE_TAG_PREFIX) {
            crate::work::language_tag_char();
            match (u8::try_from(c), head.get_mut(len)) {
                (Ok(byte), Some(slot)) if byte.is_ascii() => *slot = byte,
                _ => return RecordLanguage::Unsaid,
            }
            len += 1;
        }
        return language_of_tag(head.get(..len).unwrap_or_default());
    }
    language_id_to_bcp47(record.platform_id(), id).map_or(RecordLanguage::Unsaid, |tag| language_of_tag(tag.as_bytes()))
}

fn language_of_tag(tag: &[u8]) -> RecordLanguage {
    match tag {
        b"en-US" => RecordLanguage::UsEnglish,
        b"en" => RecordLanguage::English,
        _ => RecordLanguage::Other,
    }
}

/// The string `table` has for name `id` in English, else its first: the choice the reader's own `english_or_first` makes (US English,
/// then English, then a record that names no language, then the first of the records), over the records of this id that the reader can
/// decode, which a record in an encoding it doesn't know is not.
fn english_or_first<'a>(table: &Name<'a>, strings: FontData<'a>, id: NameId) -> Option<NameString<'a>> {
    let mut best: Option<(u8, NameString<'a>)> = None;
    let records = table.name_record().iter().filter(|record| record.name_id() == id);
    let decodable = records
        .filter(|record| Encoding::new(record.platform_id(), record.encoding_id()) != Encoding::Unknown)
        .filter_map(|record| Some((record, record.string(strings).ok()?)));
    for (index, (record, string)) in decodable.enumerate() {
        let rank = match language_of(table, record, strings) {
            RecordLanguage::UsEnglish => return Some(string),
            RecordLanguage::English => 3,
            RecordLanguage::Unsaid => 2,
            RecordLanguage::Other if index == 0 => 1,
            RecordLanguage::Other => continue,
        };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, string));
        }
    }
    best.map(|(_, string)| string)
}

fn name_of(font: &Font, ids: &[NameId]) -> Option<String> {
    let table = font.tables().name().ok()?;
    let strings = table.string_data();
    ids.iter().find_map(|id| {
        let s: String = english_or_first(&table, strings, *id)?.chars().take(MAX_NAME_CHARS).collect();
        let s = s.trim().to_owned();
        (!s.is_empty()).then_some(s)
    })
}

/// A face's typographic family and subfamily, falling back to the legacy pair.
pub(crate) fn face_names(font: &Font) -> Option<(String, String)> {
    let family = name_of(font, &[NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME])?;
    let style = name_of(font, &[NameId::TYPOGRAPHIC_SUBFAMILY_NAME, NameId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".to_owned());
    Some((family, style))
}

/// The legacy family (name ID 1), when it differs from the typographic one: documents may name a face that way.
pub(crate) fn legacy_family(font: &Font, family: &str) -> Option<String> {
    let legacy = name_of(font, &[NameId::FAMILY_NAME])?;
    (!legacy.eq_ignore_ascii_case(family)).then_some(legacy)
}

/// The units per em the `head` table of a font may state: the range the OpenType specification gives. The reader reads a number outside it
/// as 1000 (what HarfBuzz does), so the em the face measures by and the em the shaper and the reader's own metrics use would differ.
const UPEM_RANGE: std::ops::RangeInclusive<u16> = 16..=16_384;

/// The units per em of `font` if it can be a face, and `None` if it can't: it is not an `sfnt` font (the reader also takes a Type 1 or a
/// bare CFF font, which have no `head`), or its `head` table is missing or states an em outside 16 to 16,384, or its `maxp` table is
/// missing or states no glyphs. The catalogue asks the same of a font it lists, whether it got the font from a file or from bytes.
///
/// The number is the one the `head` table states, which is also the one the reader and the shaper use, since it is in range.
///
/// The glyph count is asked because the reader limits what it reads by it: a font without one has a character map that maps nothing, and
/// glyphs that are zero wide. Such a font would be a face that covers no character and measures nothing, silently, so it is refused as a
/// font with no `head` is. The em is asked because the reader answers a half em (the width of a glyph it cannot measure) in the em it
/// clamped to 1000, and a face that divided it by the em the table states, 8, would measure it 62 ems wide.
pub(crate) fn face_upem(font: &Font) -> Option<u16> {
    if !matches!(font.kind(), Kind::Sfnt(..)) || font.num_glyphs() == 0 {
        return None;
    }
    Some(font.metrics().raw_units_per_em).filter(|upem| UPEM_RANGE.contains(upem))
}

/// A length in font units as a share of the em.
fn share(v: F48Dot16, upem: f64) -> f64 {
    v.to_f64() / upem
}

/// The `OS/2` raised and lowered boxes. A table that holds 0 for the size says nothing, so it counts as absent.
fn script_boxes(font: &Font, upem: f64) -> (Option<ScriptBox>, Option<ScriptBox>) {
    let style = font.style_metrics();
    let make = |m: Option<skrifa::font::metrics::ScriptMetrics>| {
        let m = m.filter(|m| m.y_size > F48Dot16::ZERO)?;
        Some(ScriptBox { size: share(m.y_size, upem), offset: share(m.y_offset, upem) })
    };
    (make(style.subscript), make(style.superscript))
}

fn stroke(d: Option<skrifa::font::metrics::Decoration>, upem: f64) -> Option<Stroke> {
    let d = d?;
    let (top, thickness) = (share(d.position, upem), share(d.thickness, upem));
    (top.is_finite() && thickness.is_finite() && thickness > 0.0 && thickness < 0.5 && top.abs() < 2.0).then_some(Stroke { top, thickness })
}

/// The weight and slant a face declares: the `OS/2` class, else its style name.
pub(crate) fn declared_style(font: &Font, style: &str) -> (u16, bool) {
    let attrs = font.default_attributes();
    let s = style.to_ascii_lowercase();
    let italic = !matches!(attrs.style, Style::Normal) || s.contains("italic") || s.contains("oblique");
    let named = if s.contains("bold") || s.contains("black") || s.contains("heavy") { 700.0 } else { 400.0 };
    let from_table = attrs.weight.value();
    // A face whose table says 400 (the default when `OS/2` is absent) but whose name says Bold is bold.
    let weight = if (from_table - 400.0).abs() < f32::EPSILON { named } else { from_table };
    (weight.clamp(1.0, 1000.0) as u16, italic)
}

impl Face {
    /// A face from `bytes`, the `index`th of a collection (0 for a plain font). `None` if the tables don't parse, or the font can't be a
    /// face: see [`face_upem`] (no `head` table, an em out of range, no glyphs, not an `sfnt` font).
    pub(crate) fn new(bytes: FontBytes, index: u32) -> Option<Face> {
        contained(|| Face::parse(bytes, index)).flatten()
    }

    fn parse(bytes: FontBytes, index: u32) -> Option<Face> {
        let font = Font::new(bytes.blob(), index)?;
        Face::from_font(bytes, index, font)
    }

    /// A face of `font`, which was read from `bytes`.
    fn from_font(bytes: FontBytes, index: u32, font: Font) -> Option<Face> {
        let upem = face_upem(&font)?;
        let (family, style) = face_names(&font).unwrap_or_else(|| ("Unnamed".to_owned(), "Regular".to_owned()));
        let (weight, italic) = declared_style(&font, &style);
        let u = f64::from(upem);
        let (subscript, superscript) = script_boxes(&font, u);
        let styles = font.style_metrics();
        // The line the face recommends for horizontal text; a face with no line metrics at all has none.
        let line = font.metrics().h_line();
        let inner = FaceInner {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            family,
            style,
            bold: weight >= 600,
            italic,
            weight,
            index,
            upem,
            subscript,
            superscript,
            underline: stroke(styles.underline, u),
            strikeout: stroke(styles.strike_through, u),
            ascent: line.map_or(0.0, |l| share(l.ascender, u)),
            descent: line.map_or(0.0, |l| share(l.descender, u).abs()),
            font: Some(font),
            scaler: OnceLock::new(),
            bmp: OnceLock::new(),
            null: false,
            broken: AtomicBool::new(false),
            bytes,
        };
        Some(Face(Arc::new(inner)))
    }

    /// The face that stands in when no face at all could be loaded, which can't happen with the bundled faces in the binary.
    /// It has no glyphs: every character is glyph 0 and half an em wide.
    pub fn null() -> Face {
        Face(Arc::new(FaceInner {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            family: "(no font)".to_owned(),
            style: "Regular".to_owned(),
            bold: false,
            italic: false,
            weight: 400,
            bytes: FontBytes::Static(&[]),
            index: 0,
            upem: 1000,
            subscript: None,
            superscript: None,
            underline: None,
            strikeout: None,
            ascent: 0.8,
            descent: 0.2,
            font: None,
            scaler: OnceLock::new(),
            bmp: OnceLock::new(),
            null: true,
            broken: AtomicBool::new(false),
        }))
    }

    /// A number that differs for every face loaded in this process, for a renderer's glyph cache.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// The typographic family name, such as `Liberation Sans`.
    pub fn family(&self) -> &str {
        &self.0.family
    }
    /// The subfamily, such as `Bold Italic`.
    pub fn style_name(&self) -> &str {
        &self.0.style
    }
    /// Is the face itself bold (a weight of 600 or more)?
    pub fn is_bold(&self) -> bool {
        self.0.bold
    }
    /// Is the face itself italic or oblique?
    pub fn is_italic(&self) -> bool {
        self.0.italic
    }
    /// The `OS/2` weight class, 1 to 1000; 400 is regular.
    pub fn weight(&self) -> u16 {
        self.0.weight
    }
    /// Is this the face with no glyphs ([`Face::null`])?
    pub fn is_null(&self) -> bool {
        self.0.null
    }
    /// Glyph count only for static TrueType outline faces. This deliberately excludes variable,
    /// CFF, colour and bitmap outlines: callers admitting raw bytes to another renderer can validate
    /// every ordinary `glyf` outline first, without an alternate representation bypassing that check.
    /// Every horizontal line metric is also held to `max_line_em` ems (including the combined line
    /// height), so another renderer cannot pick an unchecked `hhea` or `OS/2` metric. This is metadata,
    /// not outline validation; use [`Face::outline_glyph`] for each glyph.
    pub fn static_truetype_glyph_count(&self, max_line_em: f64) -> Option<u32> {
        if !max_line_em.is_finite() || max_line_em <= 0.0 {
            return None;
        }
        self.guard(|| {
            let raw = skrifa::raw::FontRef::from_index(self.data(), self.index()).ok()?;
            let forbidden = raw.table_directory().table_records().iter().any(|record| {
                matches!(
                    &record.tag().to_be_bytes(),
                    b"VARC" | b"fvar" | b"gvar" | b"CFF " | b"CFF2" | b"COLR" | b"SVG " | b"CBDT" | b"CBLC" | b"EBDT" | b"EBLC" | b"sbix"
                )
            });
            if forbidden || raw.table_data(Tag::new(b"glyf")).is_none() || raw.table_data(Tag::new(b"loca")).is_none() {
                return None;
            }
            let is_long = match raw.head().ok()?.index_to_loc_format() {
                0 => false,
                1 => true,
                _ => return None,
            };
            let glyphs = raw.maxp().ok()?.num_glyphs();
            // Some renderers index loca directly, without enforcing maxp. Every reachable outline
            // must be included in the caller's preflight, with no hidden entries beyond this count.
            if raw.loca(is_long).ok()?.len() != usize::from(glyphs) {
                return None;
            }
            let limit = max_line_em * self.units_per_em();
            let hhea = raw.hhea().ok()?;
            let horizontal = [hhea.ascender().to_i16(), hhea.descender().to_i16(), hhea.line_gap().to_i16()];
            if horizontal.into_iter().map(|v| f64::from(v).abs()).sum::<f64>() > limit {
                return None;
            }
            if let Ok(os2) = raw.os2() {
                let typographic = [os2.s_typo_ascender(), os2.s_typo_descender(), os2.s_typo_line_gap()];
                if typographic.into_iter().map(|v| f64::from(v).abs()).sum::<f64>() > limit
                    || f64::from(os2.us_win_ascent()) + f64::from(os2.us_win_descent()) > limit
                {
                    return None;
                }
            }
            Some(u32::from(glyphs))
        })
        .flatten()
    }

    /// The font file's bytes, for a renderer that draws glyphs itself, with [`Face::index`].
    pub fn data(&self) -> &[u8] {
        self.0.bytes.get()
    }
    /// Which face of a collection this is; 0 for a plain font.
    pub fn index(&self) -> u32 {
        self.0.index
    }
    /// Font units to the em.
    pub fn units_per_em(&self) -> f64 {
        f64::from(self.0.upem)
    }
    /// The distance from the baseline to the top of the face's ascent, as a share of the em.
    pub fn ascent_share(&self) -> f64 {
        self.0.ascent
    }
    /// The distance from the baseline to the bottom of the face's descent, as a share of the em, positive. A line is this and
    /// [`Face::ascent_share`] together: what a metafile's positive font height is the cell of.
    pub fn descent_share(&self) -> f64 {
        self.0.descent
    }
    /// The raised or lowered box the face recommends, or `None` if its `OS/2` table has none or holds 0.
    pub fn script_box(&self, raised: bool) -> Option<ScriptBox> {
        if raised { self.0.superscript } else { self.0.subscript }
    }
    /// The underline the face recommends, from its `post` table.
    pub fn underline(&self) -> Option<Stroke> {
        self.0.underline
    }
    /// The strikeout the face recommends, from its `OS/2` table.
    pub fn strikeout(&self) -> Option<Stroke> {
        self.0.strikeout
    }

    /// Has a call into this face's tables ever panicked? Such a face answers as one with nothing in it from then on, and a layout says
    /// so (`Note::FaceFailed`).
    pub fn is_broken(&self) -> bool {
        self.0.broken.load(Ordering::Relaxed)
    }

    /// May the font be embedded in a document? The `OS/2` table's `fsType` says no for a face whose licence is restricted (bit 1) and one
    /// that may be embedded as bitmaps only (bit 9); a face with no `OS/2` table states no restriction. A face with nothing in it, or one
    /// that cannot be read, says no. Reading `fsType` is all this does: the licence itself is the font maker's (PDF export).
    pub fn may_embed(&self) -> bool {
        if self.0.null {
            return false;
        }
        let allowed = self.guard(|| {
            let font = self.font()?;
            match font.tables().os2() {
                Ok(os2) => Some(os2.fs_type() & 0x0202 == 0),
                Err(_) => Some(true),
            }
        });
        allowed.flatten().unwrap_or(false)
    }

    /// Run `f` on the font, contained. A panic marks the face broken, and a broken face runs nothing.
    fn guard<T>(&self, f: impl FnOnce() -> T) -> Option<T> {
        if self.is_broken() {
            return None;
        }
        let got = contained(f);
        if got.is_none() {
            self.0.broken.store(true, Ordering::Relaxed);
        }
        got
    }

    /// Runs `f` as every read of the font's tables is run: contained, a panic setting the face aside, and nothing run at all on a face
    /// already set aside. For tests of what such a panic does to a face and to a layout, since the reader this crate uses has no known
    /// panic to provoke with a damaged font (`read-fonts` 0.41.0 had one). The panic is raised outside the reader; see
    /// [`Face::with_table_watch`] for one raised inside it.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn guarded_read<T>(&self, f: impl FnOnce() -> T) -> Option<T> {
        self.guard(f)
    }

    /// A face of the font in `bytes`, a plain font and not a collection (`None` for a `ttcf` header), whose tables reach the reader through
    /// a function, which calls `on_table` with the tag each time the reader first asks for a table, before it hands the table over. A test that makes
    /// `on_table` panic for one tag raises the panic inside the reader, in the middle of the read that asked for that table, which is
    /// where a damaged font would raise one: the face's containment is then tried on a real read. `None` if `bytes` is no face, or if
    /// `on_table` panics while the face is being made. Only [`Face::data`] still holds the whole file.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_table_watch(bytes: Vec<u8>, on_table: impl Fn([u8; 4]) + Send + Sync + 'static) -> Option<Face> {
        use std::ops::Range;

        /// A table of the file, which the reader holds as bytes.
        struct Slice {
            file: Arc<Vec<u8>>,
            range: Range<usize>,
        }
        impl AsRef<[u8]> for Slice {
            fn as_ref(&self) -> &[u8] {
                self.file.get(self.range.clone()).unwrap_or_default()
            }
        }

        if bytes.starts_with(b"ttcf") {
            return None;
        }
        let be = |at: usize, len: usize| bytes.get(at..at.checked_add(len)?);
        let count = be(4, 2).and_then(|b| b.try_into().ok()).map(u16::from_be_bytes)?;
        let mut tables: Vec<([u8; 4], Range<usize>)> = Vec::new();
        for i in 0..usize::from(count) {
            let rec = 12usize.checked_add(i.checked_mul(16)?)?;
            let tag: [u8; 4] = be(rec, 4)?.try_into().ok()?;
            let offset = usize::try_from(u32::from_be_bytes(be(rec.checked_add(8)?, 4)?.try_into().ok()?)).ok()?;
            let len = usize::try_from(u32::from_be_bytes(be(rec.checked_add(12)?, 4)?.try_into().ok()?)).ok()?;
            tables.push((tag, offset..offset.checked_add(len)?));
        }
        let file = Arc::new(bytes);
        let lookup: Arc<dyn Fn(Tag) -> Option<Blob> + Send + Sync> = {
            let file = file.clone();
            Arc::new(move |tag: Tag| {
                let tag = tag.to_be_bytes();
                on_table(tag);
                let range = tables.iter().find(|(t, _)| *t == tag)?.1.clone();
                Some(Blob::Shared(Arc::new(Slice { file: file.clone(), range })))
            })
        };
        contained(|| {
            let font = Font::new(lookup, 0)?;
            Face::from_font(FontBytes::Shared(file), 0, font)
        })
        .flatten()
    }

    fn font(&self) -> Option<&Font> {
        self.0.font.as_ref()
    }

    /// The scaler that draws the font's outlines in font units, made the first time one is wanted.
    fn scaler(&self) -> Option<&ScalerFont> {
        let font = self.font()?;
        Some(self.0.scaler.get_or_init(|| ScalerFont::unhinted(font.clone(), None)))
    }

    /// Does the face map `c` to a glyph?
    pub fn covers(&self, c: char) -> bool {
        let cp = c as u32;
        if cp < 0x1_0000 {
            let bits = self.0.bmp.get_or_init(|| self.bmp_bits());
            return bits.get((cp / 64) as usize).is_some_and(|w| w & (1 << (cp % 64)) != 0);
        }
        self.guard(|| self.font().is_some_and(|f| f.charmap().map_unicode(c).is_some())).unwrap_or(false)
    }

    fn bmp_bits(&self) -> Box<[u64]> {
        let mut bits = vec![0u64; 1024].into_boxed_slice();
        // A cmap that panics half way leaves the characters read so far, which is the most it can be trusted for.
        self.guard(|| {
            if let Some(f) = self.font() {
                for (cp, _) in f.charmap().unicodes() {
                    if let Some(w) = bits.get_mut((cp / 64) as usize).filter(|_| cp < 0x1_0000) {
                        *w |= 1 << (cp % 64);
                    }
                }
            }
        });
        bits
    }

    /// The glyph the cmap gives `c`; 0, the `.notdef` glyph, if there is none.
    pub fn glyph_for(&self, c: char) -> u32 {
        self.guard(|| self.font().and_then(|f| f.charmap().map_unicode(c)).map_or(0, |g| g.to_u32())).unwrap_or(0)
    }

    /// A glyph's advance in font units. A face that can't say gives half an em.
    pub fn advance(&self, gid: u32) -> f64 {
        let half = f64::from(self.0.upem) / 2.0;
        // A glyph past the end of the font has no advance, and the reader would answer 0 for it: it is a glyph that can't be said, half an em.
        let width = self.guard(|| {
            let font = self.font()?;
            (gid < font.num_glyphs()).then(|| font.glyph_metrics().h_advance(GlyphId::new(gid)))
        });
        width.flatten().map_or(half, f64::from)
    }

    /// A glyph's outline in font units, y up, or `None` if it has none (a space) or can't be read: see [`Face::outline_glyph`] to
    /// tell the two apart.
    ///
    /// Each call reads the font again; a renderer draws many glyphs many times, so it keeps the paths it gets, by [`Face::id`] and
    /// glyph.
    pub fn outline(&self, gid: u32) -> Option<BezPath> {
        match self.outline_glyph(gid) {
            Outline::Drawn(path) => Some(path),
            Outline::Blank | Outline::Broken => None,
        }
    }

    /// A glyph's outline in font units, y up, and whether it has none because it is blank or because it could not be read.
    pub fn outline_glyph(&self, gid: u32) -> Outline {
        let got = self.guard(|| {
            let outline = self.scaler()?.outline(GlyphId::new(gid))?;
            let mut pen = PathPen::new();
            outline.draw(DrawMemory::Scratch, &mut pen).ok()?;
            // A glyph too big, or with a point no drawing can use, is one that could not be drawn.
            (!pen.spoiled).then_some(pen.path)
        });
        match got.flatten() {
            Some(path) if path.elements().is_empty() => Outline::Blank,
            Some(path) => Outline::Drawn(path),
            None => Outline::Broken,
        }
    }

    /// Shape `text` in this face. Glyphs come back in visual order, left to right on the page, so a right-to-left stretch comes
    /// back reversed. Clusters are byte offsets into `text`. A face with no shaper, or text it can't shape, gives one glyph a
    /// character from the cmap, so that a measure never fails.
    pub fn shape(&self, text: &str, params: &ShapeParams<'_>) -> Vec<ShapedGlyph> {
        self.shape_checked(text, params).0
    }

    /// As [`Face::shape`], and whether the shaper could not be used, so that the glyphs are the cmap's, one for a character, with no
    /// joining, no marks placed and no reordering: Arabic and Indic text then comes out unshaped.
    pub fn shape_checked(&self, text: &str, params: &ShapeParams<'_>) -> (Vec<ShapedGlyph>, bool) {
        match self.guard(|| self.shape_with_shaper(text, params)).flatten() {
            Some(glyphs) => (glyphs, false),
            None => (self.shape_from_cmap(text, params.rtl), true),
        }
    }

    fn shape_with_shaper(&self, text: &str, params: &ShapeParams<'_>) -> Option<Vec<ShapedGlyph>> {
        let font = ShaperFont::new(self.font()?);
        let mut buffer = Buffer::new();
        for (i, c) in text.char_indices() {
            buffer.push(u32::from(c), u32::try_from(i).ok()?);
        }
        buffer.guess_segment_properties();
        if let Some(s) = params.script.and_then(|t| Script::from_iso15924_tag(Tag::new(&t))) {
            buffer.set_script(Some(s));
        }
        let lang = params.language.or_else(|| params.script.and_then(default_language));
        if let Some(l) = lang.and_then(|l| l.parse::<Language>().ok()) {
            buffer.set_language(Some(l));
        }
        buffer.set_direction(if params.rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        let features = if params.nominal { nominal_features().to_vec() } else { Vec::new() };
        // An error is a misuse of the call (a buffer without a direction, shaped twice): neither is possible here, but a shaper that
        // says no is treated as one that could not be used, and the caller measures from the cmap.
        harfrust::shape(&font, &mut buffer, ShapeOptions::new().features(&features)).ok()?;
        let glyphs = buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .map(|(i, p)| ShapedGlyph { gid: i.glyph_id, cluster: i.cluster, x_advance: p.x_advance, x_offset: p.x_offset, y_offset: p.y_offset })
            .collect();
        Some(glyphs)
    }

    fn shape_from_cmap(&self, text: &str, rtl: bool) -> Vec<ShapedGlyph> {
        let mut out: Vec<ShapedGlyph> = text
            .char_indices()
            .map(|(i, c)| {
                let gid = self.glyph_for(c);
                let advance = self.advance(gid).round().clamp(0.0, 65_535.0) as i32;
                ShapedGlyph { gid, cluster: u32::try_from(i).unwrap_or(u32::MAX), x_advance: advance, x_offset: 0, y_offset: 0 }
            })
            .collect();
        if rtl {
            out.reverse();
        }
        out
    }
}

/// What a glyph's outline is.
#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    /// The contours to fill.
    Drawn(BezPath),
    /// The glyph exists and has nothing to draw: a space, or the missing-glyph box of a face that has none.
    Blank,
    /// The glyph could not be read: the face has no outline for this number, or its tables are damaged. A drawing is wrong here.
    Broken,
}

/// The most path elements one glyph may make. One simple outline has at most 65,535 points and so at most three elements a point (a move,
/// a segment and a close, if every point were a contour of its own), 196,605; a composite of the `glyf` table is held to 65,536 points
/// by the reader. A variable composite is held only to 2,048 references, each of which can draw a whole outline of its own into the pen:
/// 2,047 references to one outline of 65,535 points are 134 million elements, about 7.5 GB, in one call. Four times 65,536 lets through
/// any one outline and a composite of a few; the path of a glyph at the limit is 15 MB.
const MAX_PATH_ELEMENTS: usize = 4 * 65_536;

/// The largest coordinate, in font units, a point of a glyph may have: 64 ems of the largest em a face has (16,384). Composite
/// transforms multiply (a scale of up to 32 at each of 64 levels), and a point past this, or past what 32 bits hold, is not a glyph any
/// drawing can use: it would be infinity, or not a number, or a path so far from its box that flattening it never ends.
const MAX_COORD: f32 = 1_048_576.0;

/// Collects a glyph's outline into a `kurbo` path, y up, as the font has it, and stops collecting when the glyph is more than
/// [`MAX_PATH_ELEMENTS`] elements or has a point that is not a number or is past [`MAX_COORD`]: the glyph is then `spoiled`, what was
/// collected is let go, and the draw is left to finish without keeping anything.
struct PathPen {
    path: BezPath,
    spoiled: bool,
}

impl PathPen {
    fn new() -> PathPen {
        PathPen { path: BezPath::new(), spoiled: false }
    }

    /// May a command with these coordinates be kept? A command that may not spoils the glyph.
    fn keeps(&mut self, coordinates: &[f32]) -> bool {
        if self.spoiled {
            return false;
        }
        if self.path.elements().len() >= MAX_PATH_ELEMENTS || coordinates.iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
            self.spoiled = true;
            self.path = BezPath::new();
            return false;
        }
        crate::work::path_elements_held(self.path.elements().len() + 1);
        true
    }
}

impl PathSink for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.keeps(&[x, y]) {
            self.path.move_to((f64::from(x), f64::from(y)));
        }
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if self.keeps(&[x, y]) {
            self.path.line_to((f64::from(x), f64::from(y)));
        }
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        if self.keeps(&[cx0, cy0, x, y]) {
            self.path.quad_to((f64::from(cx0), f64::from(cy0)), (f64::from(x), f64::from(y)));
        }
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        if self.keeps(&[cx0, cy0, cx1, cy1, x, y]) {
            self.path.curve_to((f64::from(cx0), f64::from(cy0)), (f64::from(cx1), f64::from(cy1)), (f64::from(x), f64::from(y)));
        }
    }
    fn close(&mut self) {
        if self.keeps(&[]) {
            self.path.close_path();
        }
    }
}
