//! Style strings (S-R1 to S-R9, S-R17 to S-R23): the one `style` attribute of a cell, read into a typed [`Style`] of the keys the
//! importer reads, resolved against the stylesheet's defaults and named styles.
//!
//! * [`grammar`] splits a string into pieces in one pass (S-R1). [`defaults`] is what a shape and a connector have before their own style (S-R7), and [`named`] is the named styles
//!   (S-R8). [`keys`] is the list of keys the importer reads, how each value is read (S-R5, with the built-in values of S-R9) and the two names that stand for text: `def(n)` and
//!   `stencil(...)`. [`colour`] reads colours and colour keywords (S-R17), with the CSS names of `css_names`.
//! * [`StyleCache::resolve`] gives the [`Style`] of a cell: the defaults of its [`CellKind`] (unless the string starts with `;`), then the pieces left to right, a named style's keys
//!   copied in at its place, a value of `none` taking its key away (S-R4). **Each distinct style text is parsed once** and shared, however many cells use it, and every loop charges its
//!   units (bytes of style text hashed and walked, pieces and named keys applied) to the read before it runs.
//! * A value that is not what its key reads (a number that is text, a colour that is no colour, a word that is none of the key's) is a **bad value**: the key is dropped, so that its
//!   built-in value applies (a colour: `default`), and one `drawio_style_bad_value` names the key (S-R5, S-R17). A cell is never failed for a style value.
//! * A key the importer does not read changes nothing, and is counted by name in `drawio_style_keys_ignored` (S-R6, S-R38): [`StyleCache::finish`] puts the twenty most frequent in the
//!   report, counted once for each cell that has them.
//! * [`Style::paints`] resolves the colour keywords against the cell's parent, one level and never a loop (S-R17).
//!
//! A style's text is held once, in the [`Style`] that was made from it, and its text values (a picture's data URI, a stencil) are places in it: **a style is never expanded and no
//! value is copied out of it whole**. `image=def(n)` and `shape=def(n)` are read as text; [`keys::def_ref`] says which `def` they name.

mod css_names;

pub mod colour;
pub mod defaults;
pub mod grammar;
pub mod keys;
pub mod named;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub use colour::{Colour, Spec};
pub use defaults::CellKind;
pub use keys::{Key, Ty};

use crate::error::{DrawioError, shown};
use crate::report::{Reading, Unit};

/// The classes of warning this module notes (M-WARN).
pub mod warn {
    /// A value that is not what its key reads (S-R5, S-R17): one for each key name.
    pub const STYLE_BAD_VALUE: &str = "drawio_style_bad_value";
    /// Keys the importer does not read, the twenty most frequent with their counts (S-R6, S-R38).
    pub const STYLE_KEYS_IGNORED: &str = "drawio_style_keys_ignored";
}

/// How many ignored key names the report names (S-R6).
pub const TOP_IGNORED: usize = 20;

/// How many distinct ignored key names one style keeps apart: the rest are counted together. A style has a handful; a hostile one has millions, and each name kept costs memory.
pub const MAX_IGNORED_NAMES: usize = 64;

/// The longest an ignored key's name is kept, in characters: the report cuts it far shorter.
pub const MAX_IGNORED_NAME_CHARS: usize = 64;

/// The shadow `shadow=1` draws (S-R9): black at 0.25 opacity, offset 2 px to the right and 3 px down.
pub const SHADOW_COLOUR: Colour = Colour::BLACK;
/// The opacity of the shadow of S-R9, 0 to 1.
pub const SHADOW_OPACITY: f64 = 0.25;
/// The offset of the shadow of S-R9 in px: right and down.
pub const SHADOW_OFFSET: (f64, f64) = (2.0, 3.0);

/// A place in a style's text: bytes `start` to `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: u32,
    end: u32,
}

/// A value a style holds, typed by its key's [`Ty`].
#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    Num(f64),
    Flag(bool),
    /// Text: a place in the style's own text.
    Text(Span),
    /// Text of the stylesheet's.
    Static(&'static str),
    Colour(Spec),
}

/// A value of a key, as a [`Style`] gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<'a> {
    /// A number (S-R5), finite and within the read's coordinate range.
    Num(f64),
    /// A flag.
    Flag(bool),
    /// Text, as written, never empty.
    Text(&'a str),
    /// A colour or a colour keyword, not yet resolved against the cell and its parent ([`Style::paints`]).
    Colour(Spec),
}

/// A style resolved: the keys the importer reads that the cell's style, its kind's defaults and its named styles set, in the order of [`Key`]. A key that is not here has no value
/// of its own, and the typed reads give the built-in one (S-R9).
#[derive(Debug, PartialEq)]
pub struct Style {
    source: Arc<str>,
    vals: Box<[(Key, Val)]>,
}

impl Style {
    fn val(&self, key: Key) -> Option<&Val> {
        let i = self.vals.binary_search_by_key(&key, |(k, _)| *k).ok()?;
        self.vals.get(i).map(|(_, v)| v)
    }

    fn view<'a>(&'a self, val: &'a Val) -> Option<Value<'a>> {
        Some(match val {
            Val::Num(n) => Value::Num(*n),
            Val::Flag(f) => Value::Flag(*f),
            Val::Colour(spec) => Value::Colour(*spec),
            Val::Static(text) => Value::Text(text),
            Val::Text(span) => Value::Text(self.source.get(span.start as usize..span.end as usize)?),
        })
    }

    /// The value the style sets for `key`, if it sets one.
    pub fn get(&self, key: Key) -> Option<Value<'_>> {
        self.val(key).and_then(|val| self.view(val))
    }

    /// Whether the style sets `key` (a key a `none` took away, and a bad value, are not set).
    pub fn is_set(&self, key: Key) -> bool {
        self.val(key).is_some()
    }

    /// Every key the style sets, with its value, in the order of [`Key`].
    pub fn entries(&self) -> impl Iterator<Item = (Key, Value<'_>)> {
        self.vals.iter().filter_map(|(key, val)| Some((*key, self.view(val)?)))
    }

    /// The text of a key that holds text, or `None`.
    pub fn text(&self, key: Key) -> Option<&str> {
        match self.get(key)? {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The number of a key that holds a number: the style's, or the built-in value of S-R9 when the style sets none (`None` for a key with no built-in value).
    pub fn num(&self, key: Key) -> Option<f64> {
        match self.get(key) {
            Some(Value::Num(n)) => Some(n),
            Some(_) => None,
            None => match key.ty() {
                Ty::Num(default) => default,
                _ => None,
            },
        }
    }

    /// The flag of a key that holds a flag: the style's, or the built-in value (on for the keys that are on unless a style turns them off).
    pub fn flag(&self, key: Key) -> bool {
        match self.get(key) {
            Some(Value::Flag(f)) => f,
            Some(_) => false,
            None => matches!(key.ty(), Ty::Flag(true)),
        }
    }

    /// `html=1`: the label is HTML. Off unless set (S-R9).
    pub fn html(&self) -> bool {
        self.flag(Key::Html)
    }

    /// `strokeWidth` in px: 1 unless set (S-R9).
    pub fn stroke_width(&self) -> f64 {
        self.num(Key::StrokeWidth).unwrap_or(1.0)
    }

    /// `fontSize` in px, held to 1 to 500 (E6 of the mapping spec); 11 unless set.
    pub fn font_size(&self) -> f64 {
        self.num(Key::FontSize).unwrap_or(11.0).clamp(1.0, 500.0)
    }

    /// An opacity key, 0 to 100, as a share 0 to 1: 1 unless set (S-R9).
    fn share(&self, key: Key) -> f64 {
        self.num(key).unwrap_or(100.0).clamp(0.0, 100.0) / 100.0
    }

    /// `opacity`, 0 to 1.
    pub fn opacity(&self) -> f64 {
        self.share(Key::Opacity)
    }

    /// `fillOpacity`, 0 to 1.
    pub fn fill_opacity(&self) -> f64 {
        self.share(Key::FillOpacity)
    }

    /// `strokeOpacity`, 0 to 1.
    pub fn stroke_opacity(&self) -> f64 {
        self.share(Key::StrokeOpacity)
    }

    /// `textOpacity`, 0 to 1.
    pub fn text_opacity(&self) -> f64 {
        self.share(Key::TextOpacity)
    }

    /// `rotation`, degrees clockwise: 0 unless set.
    pub fn rotation(&self) -> f64 {
        self.num(Key::Rotation).unwrap_or(0.0)
    }

    /// `fontStyle`'s four flags (S-R22).
    pub fn font_style(&self) -> FontStyle {
        let n = self.num(Key::FontStyle).unwrap_or(0.0);
        // A fraction is cut to its whole part and a negative number has no bits set (the census holds only 0 to 5).
        let bits = if (0.0..=f64::from(u32::MAX)).contains(&n) { n as u32 } else { 0 };
        let (bold, italic, underline, strike) = font_style_bits(bits);
        FontStyle { bold, italic, underline, strike }
    }

    /// `align` (S-R22): centred unless set.
    pub fn align(&self) -> HAlign {
        HAlign::of(self.text(Key::Align), HAlign::Center)
    }

    /// `verticalAlign` (S-R22): the middle unless set.
    pub fn vertical_align(&self) -> VAlign {
        VAlign::of(self.text(Key::VerticalAlign), VAlign::Middle)
    }

    /// `labelPosition` (S-R22): the centre of the shape unless set.
    pub fn label_position(&self) -> HAlign {
        HAlign::of(self.text(Key::LabelPosition), HAlign::Center)
    }

    /// `verticalLabelPosition` (S-R22): the middle of the shape unless set.
    pub fn vertical_label_position(&self) -> VAlign {
        VAlign::of(self.text(Key::VerticalLabelPosition), VAlign::Middle)
    }

    /// `imageAlign` (S-R35): left unless set, as the `label` style has it.
    pub fn image_align(&self) -> HAlign {
        HAlign::of(self.text(Key::ImageAlign), HAlign::Left)
    }

    /// `imageVerticalAlign` (S-R35): the middle unless set.
    pub fn image_vertical_align(&self) -> VAlign {
        VAlign::of(self.text(Key::ImageVerticalAlign), VAlign::Middle)
    }

    /// `direction` (S-R20): east unless set.
    pub fn direction(&self) -> Direction {
        Direction::of(self.text(Key::Direction), Direction::East)
    }

    /// `gradientDirection` (S-R18): south unless set.
    pub fn gradient_direction(&self) -> Direction {
        Direction::of(self.text(Key::GradientDirection), Direction::South)
    }

    /// Whether the label wraps: `whiteSpace=wrap`, and nothing else wraps (S-R22).
    pub fn wraps(&self) -> bool {
        self.text(Key::WhiteSpace) == Some("wrap")
    }

    /// The label's margins in px, each side's own plus `spacing` (S-R22).
    pub fn margins(&self) -> Margins {
        let spacing = self.num(Key::Spacing).unwrap_or(2.0);
        let side = |key: Key| spacing + self.num(key).unwrap_or(0.0);
        Margins { top: side(Key::SpacingTop), right: side(Key::SpacingRight), bottom: side(Key::SpacingBottom), left: side(Key::SpacingLeft) }
    }

    /// The corner a shape of `corners` has (S-R19): `None` unless `rounded=1`.
    pub fn corner(&self, corners: Corners) -> Option<Radius> {
        if !self.flag(Key::Rounded) {
            return None;
        }
        let arc = self.num(Key::ArcSize);
        Some(match corners {
            Corners::Box if self.flag(Key::AbsoluteArcSize) => Radius::Px(arc.unwrap_or(15.0).max(0.0) / 2.0),
            Corners::Box => Radius::Percent(arc.unwrap_or(15.0).clamp(0.0, 50.0)),
            Corners::Polygon => Radius::Px(arc.unwrap_or(20.0).max(0.0) / 2.0),
        })
    }

    /// The radius in px of the bends of a connector (S-R19): `None` unless `rounded=1`, which a connector has by default; `arcSize` / 2, 10 px unless set.
    pub fn bend_radius(&self) -> Option<f64> {
        self.flag(Key::Rounded).then(|| self.num(Key::ArcSize).unwrap_or(20.0).max(0.0) / 2.0)
    }

    /// The size in px of the mark at a connector's start (`start`) or end (S-R29): 6 unless set.
    pub fn marker_size(&self, start: bool) -> f64 {
        self.num(if start { Key::StartSize } else { Key::EndSize }).unwrap_or(6.0)
    }

    /// The height of a swimlane's header in px: `startSize`, and 40 when the style sets none (S-R9; the `swimlane` named style sets 23).
    pub fn header_size(&self) -> f64 {
        self.num(Key::StartSize).unwrap_or(40.0)
    }

    /// `dashPattern` (S-R18): at most 16 lengths, in multiples of the line width. `None` when the style has none or any part is not a number of 0 or more.
    pub fn dash_pattern(&self) -> Option<Vec<f64>> {
        let text = self.text(Key::DashPattern)?;
        let mut out = Vec::new();
        for part in text.split(|c: char| c.is_ascii_whitespace()).filter(|p| !p.is_empty()) {
            if out.len() == MAX_DASHES {
                return None;
            }
            out.push(keys::parse_decimal(part).filter(|n| *n >= 0.0)?);
        }
        (!out.is_empty()).then_some(out)
    }

    /// The colours of the cell, with the keywords resolved against the cell and its `parent` (S-R17).
    ///
    /// `default` is the light-mode colour of the key (S-R7). `inherit` is the parent's colour for the same key, `parentFillColor` and `parentStrokeColor` the parent's fill and line; on
    /// a cell whose parent is a layer they all give `default`. `fillColor` and `strokeColor` are the cell's own fill and line. **One level and never a loop**: a key whose keyword
    /// names a key that is itself one of those two keywords reads as `default`. A key that is not set, or that a `none` took away, has no colour.
    pub fn paints(&self, parent: Parent<'_>) -> Paints {
        // The keyword names the parent, and a layer is a parent with nothing to name: `default` of the key the keyword is set on.
        let from_parent = |key: Key, named: Key| match parent {
            Parent::Layer => Some(colour::default_colour(key)),
            Parent::Cell(paints) => paints.of(named),
        };
        let mut out = [None; PAINT_KEYS.len()];
        let mut own: [Option<Spec>; PAINT_KEYS.len()] = [None; PAINT_KEYS.len()];
        // First, every key that does not name another of the cell's own: a colour, `default`, and the parent's.
        for ((key, slot), pending) in PAINT_KEYS.iter().zip(out.iter_mut()).zip(own.iter_mut()) {
            let Some(Value::Colour(spec)) = self.get(*key) else { continue };
            *slot = match spec {
                Spec::Colour(colour) => Some(colour),
                Spec::Default => Some(colour::default_colour(*key)),
                Spec::Inherit => from_parent(*key, *key),
                Spec::ParentFill => from_parent(*key, Key::FillColor),
                Spec::ParentStroke => from_parent(*key, Key::StrokeColor),
                Spec::Fill | Spec::Stroke => {
                    *pending = Some(spec);
                    None
                }
            };
        }
        // Then the keys that name the cell's own fill or line, which are read from the first pass and never from each other: one level.
        let [fill_own, stroke_own, ..] = own;
        let [direct_fill, direct_stroke, ..] = out;
        for ((key, slot), pending) in PAINT_KEYS.iter().zip(out.iter_mut()).zip(own) {
            let Some(spec) = pending else { continue };
            let (named, named_own) = if spec == Spec::Fill { (direct_fill, fill_own) } else { (direct_stroke, stroke_own) };
            *slot = if named_own.is_some() { Some(colour::default_colour(*key)) } else { named };
        }
        let [fill, stroke, font, gradient, label_background, swimlane_fill] = out;
        Paints { fill, stroke, font, gradient, label_background, swimlane_fill }
    }
}

/// The keys that hold a colour, in the order of [`Paints`].
const PAINT_KEYS: [Key; 6] =
    [Key::FillColor, Key::StrokeColor, Key::FontColor, Key::GradientColor, Key::LabelBackgroundColor, Key::SwimlaneFillColor];

/// The most numbers a `dashPattern` has.
const MAX_DASHES: usize = 16;

/// `fontStyle`'s bits (S-R22): bold 1, italic 2, underline 4, strikethrough 8, whatever the other bits are.
pub fn font_style_bits(v: u32) -> (bool, bool, bool, bool) {
    (v & 1 != 0, v & 2 != 0, v & 4 != 0, v & 8 != 0)
}

/// The four flags of `fontStyle` (S-R22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontStyle {
    /// Bit 1.
    pub bold: bool,
    /// Bit 2.
    pub italic: bool,
    /// Bit 4.
    pub underline: bool,
    /// Bit 8.
    pub strike: bool,
}

/// A place across a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    /// `left`.
    Left,
    /// `center`.
    Center,
    /// `right`.
    Right,
}

impl HAlign {
    fn of(word: Option<&str>, default: HAlign) -> HAlign {
        match word {
            Some("left") => HAlign::Left,
            Some("center") => HAlign::Center,
            Some("right") => HAlign::Right,
            _ => default,
        }
    }
}

/// A place down a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    /// `top`.
    Top,
    /// `middle`.
    Middle,
    /// `bottom`.
    Bottom,
}

impl VAlign {
    fn of(word: Option<&str>, default: VAlign) -> VAlign {
        match word {
            Some("top") => VAlign::Top,
            Some("middle") => VAlign::Middle,
            Some("bottom") => VAlign::Bottom,
            _ => default,
        }
    }
}

/// A side of a box: `direction` and `gradientDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `east`.
    East,
    /// `south`.
    South,
    /// `west`.
    West,
    /// `north`.
    North,
}

impl Direction {
    fn of(word: Option<&str>, default: Direction) -> Direction {
        match word {
            Some("east") => Direction::East,
            Some("south") => Direction::South,
            Some("west") => Direction::West,
            Some("north") => Direction::North,
            _ => default,
        }
    }
}

/// The margins of a label in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    /// Above the text.
    pub top: f64,
    /// Right of it.
    pub right: f64,
    /// Below it.
    pub bottom: f64,
    /// Left of it.
    pub left: f64,
}

/// What kind of outline a corner is rounded on (S-R19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    /// A rectangle-like shape (`rectangle`, `label`, `process` and kin): `arcSize` is a share of the shorter side, 15 unless set.
    Box,
    /// A polygon or a shape with curved sides: `arcSize` / 2 px, 20 unless set, so 10.
    Polygon,
}

/// The radius of a rounded corner (S-R19). The consumer holds it to half of each side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Radius {
    /// A share of the shorter side, 0 to 50 (percent).
    Percent(f64),
    /// A length in px.
    Px(f64),
}

/// The colours of a cell, resolved (S-R17): `None` is no colour (no fill, no line, no text colour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Paints {
    /// `fillColor`.
    pub fill: Option<Colour>,
    /// `strokeColor`.
    pub stroke: Option<Colour>,
    /// `fontColor`.
    pub font: Option<Colour>,
    /// `gradientColor`.
    pub gradient: Option<Colour>,
    /// `labelBackgroundColor`.
    pub label_background: Option<Colour>,
    /// `swimlaneFillColor`.
    pub swimlane_fill: Option<Colour>,
}

impl Paints {
    /// The colour of `key`, a key that holds a colour: `None` for no colour, and for any other key.
    pub fn of(&self, key: Key) -> Option<Colour> {
        match key {
            Key::FillColor => self.fill,
            Key::StrokeColor => self.stroke,
            Key::FontColor => self.font,
            Key::GradientColor => self.gradient,
            Key::LabelBackgroundColor => self.label_background,
            Key::SwimlaneFillColor => self.swimlane_fill,
            _ => None,
        }
    }
}

/// What a cell's parent is, for the colour keywords that name it (S-R17).
#[derive(Clone, Copy, Debug)]
pub enum Parent<'a> {
    /// A layer: the parent keywords give `default`.
    Layer,
    /// Another cell, with the colours it resolved to.
    Cell(&'a Paints),
}

/// One distinct style text of one kind: its style, the unknown keys it names and how many cells use it.
#[derive(Debug)]
struct Entry {
    style: Arc<Style>,
    /// The ignored key names of the text, each with how many times the text names it, by name; the names past [`MAX_IGNORED_NAMES`] are in `other_ignored`.
    ignored: Box<[(Box<str>, u32)]>,
    other_ignored: u32,
    /// How many times the cache gave this style out.
    uses: u64,
}

/// The styles of a read, each distinct text of each [`CellKind`] parsed once. Keep one for the whole read, and call [`StyleCache::finish`] when the last cell has been resolved.
#[derive(Debug, Default)]
pub struct StyleCache {
    maps: [HashMap<Arc<str>, usize>; 2],
    entries: Vec<Entry>,
    /// The keys a bad value has been noted for.
    noted_bad: Vec<Key>,
}

impl StyleCache {
    /// An empty cache.
    pub fn new() -> StyleCache {
        StyleCache::default()
    }

    /// How many distinct styles the cache has parsed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache has parsed none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The style of a cell of `kind` whose `style` attribute is `text` (`None` and an empty text are the kind's defaults alone, S-R2).
    ///
    /// A text the cache has parsed is found by hashing it (its bytes are charged as [`Unit::StyleBytes`]) and costs nothing more; a new one is walked once (its bytes again, and each
    /// piece and each key of a named style as [`Unit::StylePieces`]). A charge past the read's budget is refused as `drawio_too_complex` and nothing is kept.
    pub fn resolve(&mut self, text: Option<&str>, kind: CellKind, reading: &mut Reading<'_>) -> Result<Arc<Style>, DrawioError> {
        let text = text.unwrap_or("");
        let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
        reading.charge(Unit::StyleBytes, bytes)?;
        if let Some(&i) = self.maps.get(kind.slot()).and_then(|map| map.get(text))
            && let Some(entry) = self.entries.get_mut(i)
        {
            entry.uses = entry.uses.saturating_add(1);
            return Ok(entry.style.clone());
        }
        if u32::try_from(text.len()).is_err() {
            return Err(DrawioError::too_large("A style is longer than 4 GiB."));
        }
        reading.charge(Unit::StyleBytes, bytes)?;
        let source: Arc<str> = Arc::from(text);
        let parsed = parse_style(&source, kind, reading)?;
        for key in &parsed.bad {
            if !self.noted_bad.contains(key) {
                self.noted_bad.push(*key);
                let known = reading.report.warnings.get(warn::STYLE_BAD_VALUE).is_some_and(|noted| noted.cases.iter().any(|case| case == key.name()));
                if !known {
                    reading.note(warn::STYLE_BAD_VALUE, key.name());
                }
            }
        }
        let style = Arc::new(Style { source: source.clone(), vals: parsed.vals.into_boxed_slice() });
        let entry = Entry { style: style.clone(), ignored: parsed.ignored, other_ignored: parsed.other_ignored, uses: 1 };
        if let Some(map) = self.maps.get_mut(kind.slot()) {
            map.insert(source, self.entries.len());
            self.entries.push(entry);
        }
        Ok(style)
    }

    /// Notes the keys the importer does not read: `drawio_style_keys_ignored` with the number of times they were named (once for each cell that has them) and the
    /// [`TOP_IGNORED`] most frequent as its cases, each "name count" (S-R6). Call it once, after the last cell.
    pub fn finish(self, reading: &mut Reading<'_>) -> Result<(), DrawioError> {
        // The names each style kept are visited once, which is charged before they are.
        let visited: u64 = self.entries.iter().map(|entry| entry.ignored.len() as u64).sum();
        reading.charge(Unit::StylePieces, visited)?;
        let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
        let mut count = 0u64;
        for entry in &self.entries {
            for (name, n) in entry.ignored.iter() {
                let uses = u64::from(*n).saturating_mul(entry.uses);
                let total = totals.entry(name).or_insert(0);
                *total = total.saturating_add(uses);
                count = count.saturating_add(uses);
            }
            count = count.saturating_add(u64::from(entry.other_ignored).saturating_mul(entry.uses));
        }
        if count == 0 {
            return Ok(());
        }
        let mut ranked: Vec<(&str, u64)> = totals.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let room = TOP_IGNORED.min(reading.limits.max_cases);
        let max_cases = reading.limits.max_cases;
        let noted = reading.report.warnings.entry(warn::STYLE_KEYS_IGNORED).or_default();
        noted.count = noted.count.saturating_add(count);
        for (name, n) in ranked.into_iter().take(room) {
            if noted.cases.len() < max_cases {
                noted.cases.push(format!("{} {n}", shown(name, CASE_NAME_CHARS)));
            }
        }
        Ok(())
    }
}

/// The longest an ignored key's name is in a case of the report, in characters, escaped and cut.
const CASE_NAME_CHARS: usize = 120;

/// What reading one style text gave.
struct Parsed {
    vals: Vec<(Key, Val)>,
    /// The keys that had a bad value, each once.
    bad: Vec<Key>,
    ignored: Box<[(Box<str>, u32)]>,
    other_ignored: u32,
}

/// Reads the style text `source` of a cell of `kind`: the defaults of the kind unless it starts with `;`, then its pieces in order (S-R1, S-R2). Each piece is charged to the read
/// before it is applied, and a named style's keys before they are.
fn parse_style(source: &str, kind: CellKind, reading: &mut Reading<'_>) -> Result<Parsed, DrawioError> {
    let mut resolver = Resolver { vals: Vec::new(), bad: Vec::new(), seen_bad: [false; Key::COUNT], max_number: reading.limits.max_coord_px };
    let mut ignored = Ignored::default();
    if !source.starts_with(';') {
        let defaults = defaults::pairs(kind);
        reading.charge(Unit::StylePieces, defaults.len() as u64)?;
        for (key, value) in defaults {
            resolver.set_static(key, value);
        }
    }
    for piece in grammar::pieces(source) {
        reading.charge(Unit::StylePieces, 1)?;
        if piece.text.is_empty() {
            continue;
        }
        match piece.pair() {
            Some((name, value)) => match Key::from_name(name) {
                Some(key) => {
                    // The value is the rest of the piece after the first `=`: a place in the text, not a copy.
                    let end = piece.offset.saturating_add(piece.text.len());
                    let start = end.saturating_sub(value.len());
                    resolver.set(key, value, Val::Text(Span { start: as_u32(start), end: as_u32(end) }));
                    if key.is_lost() {
                        ignored.add(key.name());
                    }
                }
                None => ignored.add(name),
            },
            None => {
                if let Some(style) = named::find(piece.text) {
                    reading.charge(Unit::StylePieces, style.len() as u64)?;
                    resolver.apply_named(style);
                }
            }
        }
    }
    let other_ignored = ignored.other;
    Ok(Parsed { vals: resolver.vals, bad: resolver.bad, ignored: ignored.into_sorted(), other_ignored })
}

/// A place in a text as a `u32`: a text that has one longer than that is refused before it is read, so that this never saturates.
fn as_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What a value of a key, read as the key reads it, comes to.
enum Typed {
    /// No value: a `none`, or an empty text or colour. The key is taken away.
    Remove,
    /// A bad value: the key is taken away (so its built-in value applies), or given the value the type says instead, and the key is named in one warning.
    Bad(Option<Val>),
    /// A good value.
    Set(Val),
}

/// `value` as `key` reads it (S-R4, S-R5, S-R17). `text` is the value as a text of the style, which a key that holds text keeps; `max` is the largest number.
fn typed(key: Key, value: &str, text: Val, max: f64) -> Typed {
    if value == "none" {
        return Typed::Remove;
    }
    match key.ty() {
        Ty::Num(_) => keys::parse_number(value, max).map_or(Typed::Bad(None), |n| Typed::Set(Val::Num(n))),
        Ty::Flag(_) => Typed::Set(Val::Flag(keys::parse_flag(value))),
        Ty::Colour if value.is_empty() => Typed::Remove,
        Ty::Colour => colour::parse(value).map_or(Typed::Bad(Some(Val::Colour(Spec::Default))), |spec| Typed::Set(Val::Colour(spec))),
        Ty::Text | Ty::OneOf(_) if value.is_empty() => Typed::Remove,
        Ty::Text => Typed::Set(text),
        Ty::OneOf(words) if words.contains(&value) => Typed::Set(text),
        Ty::OneOf(_) => Typed::Bad(None),
    }
}

/// The keys of a style as they are set, piece by piece, in the order of [`Key`].
struct Resolver {
    vals: Vec<(Key, Val)>,
    /// The keys that had a bad value, each once, in the order they were met.
    bad: Vec<Key>,
    seen_bad: [bool; Key::COUNT],
    max_number: f64,
}

impl Resolver {
    /// Sets `key` to `value` as the key reads it, or takes it away.
    fn set(&mut self, key: Key, value: &str, text: Val) {
        let new = match typed(key, value, text, self.max_number) {
            Typed::Remove => None,
            Typed::Set(val) => Some(val),
            Typed::Bad(fallback) => {
                if let Some(seen) = self.seen_bad.get_mut(key as usize).filter(|seen| !**seen) {
                    *seen = true;
                    self.bad.push(key);
                }
                fallback
            }
        };
        match (self.vals.binary_search_by_key(&key, |(k, _)| *k), new) {
            (Ok(i), Some(val)) => {
                if let Some(slot) = self.vals.get_mut(i) {
                    slot.1 = val;
                }
            }
            (Ok(i), None) => {
                self.vals.remove(i);
            }
            (Err(i), Some(val)) => self.vals.insert(i, (key, val)),
            (Err(_), None) => {}
        }
    }

    /// A pair of the stylesheet's, whose value is a text of its own.
    fn set_static(&mut self, key: &str, value: &'static str) {
        if let Some(key) = Key::from_name(key) {
            self.set(key, value, Val::Static(value));
        }
    }

    /// The pairs of a named style: those of the style it extends, then its own.
    fn apply_named(&mut self, style: &named::Named) {
        let base = style.extends.and_then(named::find).map_or(&[][..], |base| base.pairs);
        for (key, value) in base.iter().chain(style.pairs) {
            self.set_static(key, value);
        }
    }
}

/// The names of the keys a style text has that the importer does not read, counted: at most [`MAX_IGNORED_NAMES`] apart, the rest together.
#[derive(Default)]
struct Ignored<'a> {
    names: HashMap<&'a str, u32>,
    other: u32,
}

impl<'a> Ignored<'a> {
    /// Counts one use of the key `name`, cut to [`MAX_IGNORED_NAME_CHARS`] characters.
    fn add(&mut self, name: &'a str) {
        let name = name.char_indices().nth(MAX_IGNORED_NAME_CHARS).and_then(|(i, _)| name.get(..i)).unwrap_or(name);
        if let Some(n) = self.names.get_mut(name) {
            *n = n.saturating_add(1);
        } else if self.names.len() < MAX_IGNORED_NAMES {
            self.names.insert(name, 1);
        } else {
            self.other = self.other.saturating_add(1);
        }
    }

    /// The names with their counts, by name.
    fn into_sorted(self) -> Box<[(Box<str>, u32)]> {
        let mut names: Vec<(Box<str>, u32)> = self.names.into_iter().map(|(name, n)| (Box::from(name), n)).collect();
        names.sort();
        names.into_boxed_slice()
    }
}
