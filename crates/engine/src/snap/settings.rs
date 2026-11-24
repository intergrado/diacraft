//! The Snap & Glue settings of a document, and where they are kept.
//!
//! **Our rule (it replaces the layout, which kept every option but two in DiaCraft's part).** The format publishes the
//! bits of the words of `DocumentSettings` and the form of `DynamicGridEnabled`, so each option they document is **the file's own**:
//!
//! * `SnapSettings` (what a shape snaps to): ruler subdivisions 1, grid 2, guides 4, handles 8, vertices 16, connection points 32, shape outlines 256, the
//!   alignment box 512, shape extensions 1024, intersections 65536;
//! * `GlueSettings` (what a connector's end glues to): guides 1, handles 2, vertices 4, connection points 8, shape geometry 32;
//! * `SnapExtensions` (the extension lines): the thirteen flags 1 to 4096, one option each (`ext_*`);
//! * `DynamicGridEnabled`: a boolean, written `1` or `0` and read as `true`, `false`, `1` or `0`.
//!
//! **Reading.** Each option is the bit of its word when the word is a whole number (or the boolean), and its default when the word is absent, empty or text.
//! **Writing.** A change to an option sets or clears *its* flag and nothing else of the word: every other bit the file holds (the flag 32768 that R55 reads as
//! "off" in both words, flags no page names, a negative value, a value past 32 bits) is kept as read. A word the file lacks is made from the settings'
//! documented flags when one of its options changes; a word that is text and not a number (or a boolean) is left alone and the change is **refused** (L2 of
//!). The windows part's copies of the words are never read or written: the document's word is the only one there is.
//!
//! **What stays in DiaCraft's part** (`Document::diacraft`): the **Snap and Glue switches**, `snap` and `glue`. R55 reads the flag
//! 32768 of both words as "snapping off" and "glue off", but no page says the switches (the dialog's own Snap and Glue boxes, `Document.SnapEnabled` and
//! `GlueEnabled`) are stored as that flag, so they are ours until that is confirmed: a child `snap` of the part's data holding `snap` and `glue` as 1 or 0,
//! written when either is off and taken out when both are on. The flag 32768 is kept as read and is neither read as the switch nor set or cleared.
//!
//! **A file that saved** has the other options in that child. They are read as the options' values **when the option's word is absent or is not a number**;
//! when the word is there, the word wins, and the part's attribute for that option is ignored. The next change of any option writes the child again with the two
//! switches only; a value that came from the part for a word the file lacks is first written into that word, so that it is not lost, and one for a word that is
//! text (which can be written nowhere) stays in the part. (A file made
//! whose `SnapSettings` word was made by it from nothing holds flags 1 and 2 alone, and now reads the other options of that word as off.)
//!
//! **Defaults** (they are what a new drawing is written with and what a word, or a file, that gives none reads as): where a page documents a
//! default, that; where it does not, the value most files saved by the reference app hold **for that flag** (1,906 files, 277 sources), which is
//! ours. Each is a statistic of the samples, a count flag by flag, and not the values of any one file: R54 asks that a skeleton's values not be copied from a
//! sample, and none is (the sums 65847, 9, 34 and 1 follow from the flags that most files set; they are not read off a file).
//!
//! | Option | Default | Where it comes from |
//! |---|---|---|
//! | `ext_centre_axes`, `ext_linear` | on | documented: `Document.SnapExtensions` is 34, and 1,885 of 1,906 files hold 34 |
//! | the other `ext_*` | off | the same: 34 sets only those two |
//! | `ruler`, `grid` | on | documented: "Rulers and grid are on by default"; 1,843 and 1,848 files |
//! | `guides`, `snap_connection_points`, `snap_geometry` | on | samples: 1,850, 1,893 and 1,842 files |
//! | `snap_vertices`, `intersections` | on | samples: 1,166 files each (61%) |
//! | `snap_handles`, `alignment_box`, `snap_extensions` | off | samples: 23, 19 and 26 files |
//! | `glue_guides`, `glue_connection_points` | on | samples: 1,865 and 1,898 files |
//! | `glue_handles`, `glue_vertices`, `glue_geometry` | off | samples: 22, 23 and 27 files |
//! | `dynamic_grid` | on | samples: 1,704 files (89%) |
//! | `snap`, `glue` (the switches) | on | ours; 13 files hold snap flag 32768 and 51 glue flag 32768 |
//!
//! These are the words a new drawing is given ([`blank_drawing`]): `SnapSettings` 65847, `GlueSettings` 9, `SnapExtensions` 34 and `DynamicGridEnabled` 1.
//!
//! **Snap strength** (ours). The Advanced tab's six sliders, Rulers, Grid, Guides, Points, Geometry and
//! Extensions, are [`Strength`]: how far, in **screen pixels**, 0 to 999, each kind of target pulls a shape. `DocumentSettings` has **no element for them**
//! (lists the children, and none is a strength), and the Learn pages put the values on the application's settings object, so no file
//! format says where a drawing keeps them: the owner's rule for this task is that they are the document's settings, **saved with the drawing**, so they are DiaCraft's
//! own data, attributes `strength_ruler`, `strength_grid`, `strength_guides`, `strength_points`, `strength_geometry` and `strength_extensions` of the same child `snap` as
//! the switches, **written only for a value that is not the default** (so a drawing with the defaults gets no attribute and no part). A VSDX file keeps them in its
//! DiaCraft solution part and a `.diacraft` or `.odg` file in the model's own data, where the switches go; a file that has none reads the defaults. An attribute that is not
//! a whole number from 0 to 999 is not read (the default stands) and is dropped by the next change.

use std::sync::Arc;

use diacraft_doc::{Document, SnapWords};

/// The most characters of a word that are read: a longer one is not a number a flag mask is.
const MAX_WORD_CHARS: usize = 32;

/// The child of DiaCraft's data that holds the switches, and the snap strengths.
pub(crate) const PART_CHILD: &str = "snap";

/// The most screen pixels a kind of target may pull from: the number box of the Advanced tab's range is 0 to 999.
pub const STRENGTH_MAX: u16 = 999;

/// One snap strength: its name (the key of `snap.set` and `snap.settings`, and the attribute of DiaCraft's part), the dialog's label, what it pulls, and its default.
#[derive(Clone, Copy, Debug)]
pub struct StrengthDef {
    pub name: &'static str,
    /// The label of the slider on the Advanced tab.
    pub label: &'static str,
    /// What the slider governs, in words.
    pub pulls: &'static str,
    /// The default, in screen pixels.
    pub default: u16,
}

/// The six snap strengths, in the order of the Advanced tab's sliders: Rulers, Grid, Guides, Points, Geometry and Extensions.
pub const STRENGTHS: [StrengthDef; 6] = [
    StrengthDef { name: "strength_ruler", label: "Rulers", pulls: "the ruler's subdivisions", default: 4 },
    StrengthDef { name: "strength_grid", label: "Grid", pulls: "the grid's lines", default: 5 },
    StrengthDef { name: "strength_guides", label: "Guides", pulls: "guides", default: 10 },
    StrengthDef { name: "strength_points", label: "Points", pulls: "connection points, vertices and handles", default: 10 },
    StrengthDef { name: "strength_geometry", label: "Geometry", pulls: "the outlines of shapes and where they cross", default: 8 },
    StrengthDef {
        name: "strength_extensions",
        label: "Extensions",
        pulls: "the dynamic grid's alignment lines, margins, spacings and sizes, and shape extension lines",
        default: 13,
    },
];

/// How far each kind of target pulls, in **screen pixels** (the engine takes a pull in pixels at the zoom of the window, `craft_snap::Reach::Screen`). A part of
/// [`SnapSettings`], so of the document. 0 is a pull of nothing: a target pulls only a shape that already lies on it.
///
/// The alignment of other shapes (their edges and middles), the page margins, equal spacings and equal sizes, which the dynamic grid draws, are the **Extensions**
/// slider's ("adjust the Extensions snap strength" for the dynamic grid); the grid and the ruler have their own, and pull no longer from any distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Strength {
    /// Ruler subdivisions.
    pub ruler: u16,
    /// Grid lines.
    pub grid: u16,
    /// Guides.
    pub guides: u16,
    /// Connection points, vertices and handles.
    pub points: u16,
    /// Outlines (geometry) and the intersections of outlines.
    pub geometry: u16,
    /// The alignment boxes of other shapes, the page margins, the spacing between shapes and equal sizes (the dynamic grid), and shape extension lines.
    pub extensions: u16,
}

impl Default for Strength {
    fn default() -> Self {
        Strength { ruler: 4, grid: 5, guides: 10, points: 10, geometry: 8, extensions: 13 }
    }
}

impl Strength {
    /// The index of the strength called `name` in [`STRENGTHS`].
    pub fn index_of(name: &str) -> Option<usize> {
        STRENGTHS.iter().position(|s| s.name == name)
    }

    /// The strength at `index` of [`STRENGTHS`], in pixels; 0 for an index that is none.
    pub fn get(&self, index: usize) -> u16 {
        match index {
            0 => self.ruler,
            1 => self.grid,
            2 => self.guides,
            3 => self.points,
            4 => self.geometry,
            5 => self.extensions,
            _ => 0,
        }
    }

    /// Sets the strength at `index` of [`STRENGTHS`] to `pixels`. → `false`, and nothing set, for an index that is none or for more than [`STRENGTH_MAX`].
    pub fn put(&mut self, index: usize, pixels: u16) -> bool {
        if pixels > STRENGTH_MAX {
            return false;
        }
        let slot = match index {
            0 => &mut self.ruler,
            1 => &mut self.grid,
            2 => &mut self.guides,
            3 => &mut self.points,
            4 => &mut self.geometry,
            5 => &mut self.extensions,
            _ => return false,
        };
        *slot = pixels;
        true
    }
}

/// The pixels a text says: a whole number of decimal digits, from 0 to [`STRENGTH_MAX`] (no sign, no blanks, no fraction). `None` for any other text.
pub fn strength_pixels(text: &str) -> Option<u16> {
    if text.is_empty() || text.len() > 3 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok().filter(|n| *n <= STRENGTH_MAX)
}

/// One of the four words of `DocumentSettings` that hold options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    /// `SnapSettings`.
    Snap,
    /// `GlueSettings`.
    Glue,
    /// `SnapExtensions`.
    Extensions,
    /// `DynamicGridEnabled`, a boolean.
    Dynamic,
}

impl Word {
    /// The four, in the order files write them.
    pub const ALL: [Word; 4] = [Word::Glue, Word::Snap, Word::Extensions, Word::Dynamic];

    /// The element's name.
    pub fn element(self) -> &'static str {
        match self {
            Word::Snap => "SnapSettings",
            Word::Glue => "GlueSettings",
            Word::Extensions => "SnapExtensions",
            Word::Dynamic => "DynamicGridEnabled",
        }
    }

    fn text(self, words: &SnapWords) -> Option<&str> {
        match self {
            Word::Snap => words.snap.as_deref(),
            Word::Glue => words.glue.as_deref(),
            Word::Extensions => words.extensions.as_deref(),
            Word::Dynamic => words.dynamic_grid.as_deref(),
        }
    }

    fn put(self, words: &mut SnapWords, text: String) {
        let text = Some(Arc::<str>::from(text));
        match self {
            Word::Snap => words.snap = text,
            Word::Glue => words.glue = text,
            Word::Extensions => words.extensions = text,
            Word::Dynamic => words.dynamic_grid = text,
        }
    }
}

/// Where an option is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    /// DiaCraft's own part: the switches.
    Part,
    /// A flag of this word (the whole word for `DynamicGridEnabled`).
    Word(Word),
}

/// An option: its name (the key of `snap.set` and `snap.settings`), where it is kept, its flag, and its default.
#[derive(Clone, Copy, Debug)]
pub struct OptionDef {
    pub name: &'static str,
    pub home: Home,
    /// The flag of the option in its word; 0 for a switch, and unused for the boolean word.
    pub flag: i64,
    pub default: bool,
}

macro_rules! options {
    ($($field:ident = $name:literal, $home:expr, $flag:literal, $default:literal, $doc:literal;)*) => {
        /// The options of Snap & Glue the document holds, each a boolean, and the six snap strengths in screen pixels ([`Strength`]).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SnapSettings {
            $(#[doc = $doc] pub $field: bool,)*
            /// How far each kind of target pulls, in screen pixels. DiaCraft's part.
            pub strength: Strength,
        }

        /// Every option, in the order of the struct.
        pub const OPTIONS: &[OptionDef] = &[$(OptionDef { name: $name, home: $home, flag: $flag, default: $default },)*];

        const GETTERS: &[fn(&SnapSettings) -> bool] = &[$(|s| s.$field,)*];
        const SETTERS: &[fn(&mut SnapSettings, bool)] = &[$(|s, on| s.$field = on,)*];

        impl Default for SnapSettings {
            fn default() -> Self {
                SnapSettings { $($field: $default,)* strength: Strength::default() }
            }
        }

        impl SnapSettings {
            /// Every option off (the strengths are the defaults).
            pub fn none() -> SnapSettings {
                SnapSettings { $($field: false,)* strength: Strength::default() }
            }

            /// Every option on (the strengths are the defaults).
            pub fn all() -> SnapSettings {
                SnapSettings { $($field: true,)* strength: Strength::default() }
            }
        }
    };
}

options! {
    snap = "snap", Home::Part, 0, true, "The Snap switch (`SnapToggle`): off, nothing pulls a shape while it is drawn or moved. DiaCraft's part.";
    glue = "glue", Home::Part, 0, true, "The Glue switch (`GlueToggle`): off, a connector's end glues to nothing. DiaCraft's part.";
    ruler = "ruler", Home::Word(Word::Snap), 1, true, "Snap to the ruler's subdivisions: flag 1 of `SnapSettings`.";
    grid = "grid", Home::Word(Word::Snap), 2, true, "Snap to the grid: flag 2 of `SnapSettings`.";
    guides = "guides", Home::Word(Word::Snap), 4, true, "Snap to guides: flag 4 of `SnapSettings`.";
    snap_handles = "snap_handles", Home::Word(Word::Snap), 8, false, "Snap a shape to the handles of other shapes: flag 8 of `SnapSettings`.";
    snap_vertices = "snap_vertices", Home::Word(Word::Snap), 16, true, "Snap a shape to the vertices of other shapes: flag 16 of `SnapSettings`.";
    snap_connection_points = "snap_connection_points", Home::Word(Word::Snap), 32, true, "Snap a shape to the connection points of other shapes: flag 32 of `SnapSettings`.";
    snap_geometry = "snap_geometry", Home::Word(Word::Snap), 256, true, "Snap a shape to the outlines of other shapes, as drawn: flag 256 of `SnapSettings`.";
    alignment_box = "alignment_box", Home::Word(Word::Snap), 512, false, "Snap to the alignment boxes of other shapes (their edges and middles): flag 512 of `SnapSettings`.";
    snap_extensions = "snap_extensions", Home::Word(Word::Snap), 1024, false, "Snap to shape extensions: flag 1024 of `SnapSettings`. Kept and set; nothing draws extension lines yet.";
    intersections = "intersections", Home::Word(Word::Snap), 65536, true, "Snap to the intersections of shape geometry: flag 65536 of `SnapSettings`.";
    glue_guides = "glue_guides", Home::Word(Word::Glue), 1, true, "Glue to guides: flag 1 of `GlueSettings`.";
    glue_handles = "glue_handles", Home::Word(Word::Glue), 2, false, "Glue a connector's end to shape handles: flag 2 of `GlueSettings`.";
    glue_vertices = "glue_vertices", Home::Word(Word::Glue), 4, false, "Glue a connector's end to shape vertices: flag 4 of `GlueSettings`.";
    glue_connection_points = "glue_connection_points", Home::Word(Word::Glue), 8, true, "Glue a connector's end to connection points: flag 8 of `GlueSettings`.";
    glue_geometry = "glue_geometry", Home::Word(Word::Glue), 32, false, "Glue a connector's end to a point on a shape's outline: flag 32 of `GlueSettings`.";
    ext_alignment_box = "ext_alignment_box", Home::Word(Word::Extensions), 1, false, "Extension line: alignment box extension. Flag 1 of `SnapExtensions`.";
    ext_centre_axes = "ext_centre_axes", Home::Word(Word::Extensions), 2, true, "Extension line: centre axes. Flag 2 of `SnapExtensions`.";
    ext_tangents = "ext_tangents", Home::Word(Word::Extensions), 4, false, "Extension line: tangents inside curves. Flag 4 of `SnapExtensions`.";
    ext_segment_ends = "ext_segment_ends", Home::Word(Word::Extensions), 8, false, "Extension line: segment end points. Flag 8 of `SnapExtensions`.";
    ext_segment_midpoints = "ext_segment_midpoints", Home::Word(Word::Extensions), 16, false, "Extension line: segment midpoints. Flag 16 of `SnapExtensions`.";
    ext_linear = "ext_linear", Home::Word(Word::Extensions), 32, true, "Extension line: linear extension. Flag 32 of `SnapExtensions`.";
    ext_curved = "ext_curved", Home::Word(Word::Extensions), 64, false, "Extension line: curved extension. Flag 64 of `SnapExtensions`.";
    ext_perpendicular_end = "ext_perpendicular_end", Home::Word(Word::Extensions), 128, false, "Extension line: perpendicular at an end point. Flag 128 of `SnapExtensions`.";
    ext_perpendicular_mid = "ext_perpendicular_mid", Home::Word(Word::Extensions), 256, false, "Extension line: perpendicular at a midpoint. Flag 256 of `SnapExtensions`.";
    ext_horizontal_end = "ext_horizontal_end", Home::Word(Word::Extensions), 512, false, "Extension line: horizontal line at an end point. Flag 512 of `SnapExtensions`.";
    ext_vertical_end = "ext_vertical_end", Home::Word(Word::Extensions), 1024, false, "Extension line: vertical line at an end point. Flag 1024 of `SnapExtensions`.";
    ext_ellipse_centres = "ext_ellipse_centres", Home::Word(Word::Extensions), 2048, false, "Extension line: ellipse centre points. Flag 2048 of `SnapExtensions`.";
    ext_isometric = "ext_isometric", Home::Word(Word::Extensions), 4096, false, "Extension line: isometric angle lines. Flag 4096 of `SnapExtensions`.";
    dynamic_grid = "dynamic_grid", Home::Word(Word::Dynamic), 1, true, "The dynamic grid: `DynamicGridEnabled`, a boolean.";
}

/// What a word holds, as far as a setting can read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// The word is not there, or is empty.
    Absent,
    /// A whole number of any size an `i64` holds (a negative one, or one past 32 bits, keeps every bit it holds).
    Number(i64),
    /// `true`, `false`, `1` or `0`: the literals of an `xsd:boolean`.
    Boolean(bool),
    /// Text that is none of those, or too long: kept as read, and never changed.
    Unreadable,
}

impl Word {
    fn held(self, words: &SnapWords) -> Held {
        let Some(text) = self.text(words).map(str::trim).filter(|t| !t.is_empty()) else { return Held::Absent };
        if text.chars().count() > MAX_WORD_CHARS {
            return Held::Unreadable;
        }
        if self == Word::Dynamic {
            return match text {
                "true" | "1" => Held::Boolean(true),
                "false" | "0" => Held::Boolean(false),
                _ => Held::Unreadable,
            };
        }
        text.parse::<i64>().map_or(Held::Unreadable, Held::Number)
    }
}

/// What a word becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordChange {
    /// It already says it.
    Same,
    /// This text, with every other bit as it was.
    Write(Arc<str>),
    /// The word is text that is not a whole number (or a boolean), and is not written over.
    Refuse,
}

impl SnapSettings {
    /// The index of the option called `name`.
    pub fn index_of(name: &str) -> Option<usize> {
        OPTIONS.iter().position(|o| o.name == name)
    }

    /// The option at `index` of [`OPTIONS`]; off for an index that is none.
    pub fn get(&self, index: usize) -> bool {
        GETTERS.get(index).is_some_and(|get| get(self))
    }

    /// Sets the option at `index` of [`OPTIONS`].
    pub fn put(&mut self, index: usize, on: bool) {
        if let Some(set) = SETTERS.get(index) {
            set(self, on);
        }
    }

    /// The settings a document holds: the defaults, then the values of DiaCraft's part (the switches, and the options an earlier build kept there), then the
    /// options of every word the document holds as a number (or boolean), which win over the part.
    pub fn of(doc: &Document) -> SnapSettings {
        let mut settings = SnapSettings::default();
        if let Some(child) = doc.diacraft_child(PART_CHILD) {
            for (name, value) in child.attrs.iter() {
                if let (Some(index), Some(on)) = (SnapSettings::index_of(name), part_value(value)) {
                    settings.put(index, on);
                } else if let (Some(index), Some(pixels)) = (Strength::index_of(name), strength_pixels(value)) {
                    settings.strength.put(index, pixels);
                }
            }
        }
        settings.read_words(&doc.snap_words());
        settings
    }

    /// The settings these words say: the defaults, but for the options of every word that is a number (or, for `DynamicGridEnabled`, a boolean).
    pub fn from_words(words: &SnapWords) -> SnapSettings {
        let mut settings = SnapSettings::default();
        settings.read_words(words);
        settings
    }

    fn read_words(&mut self, words: &SnapWords) {
        for word in Word::ALL {
            let held = word.held(words);
            for (index, option) in OPTIONS.iter().enumerate() {
                if option.home != Home::Word(word) {
                    continue;
                }
                match held {
                    Held::Number(n) => self.put(index, n & option.flag != 0),
                    Held::Boolean(b) => self.put(index, b),
                    Held::Absent | Held::Unreadable => {}
                }
            }
        }
    }

    /// The word `word` as these settings put it, starting from `base` (the bits the file holds, or 0 for a word made new): each of its options' flags is set
    /// or cleared and every other bit is `base`'s.
    fn fold(&self, word: Word, base: i64) -> i64 {
        let mut value = base;
        for (index, option) in OPTIONS.iter().enumerate() {
            if option.home == Home::Word(word) {
                value = if self.get(index) { value | option.flag } else { value & !option.flag };
            }
        }
        value
    }

    /// What the word `word` becomes for these settings, for a document whose words are `now`.
    pub fn word_change(&self, word: Word, now: &SnapWords) -> WordChange {
        match (word, word.held(now)) {
            (_, Held::Unreadable) => WordChange::Refuse,
            (Word::Dynamic, Held::Boolean(b)) if b == self.dynamic_grid => WordChange::Same,
            (Word::Dynamic, _) => WordChange::Write(Arc::from(if self.dynamic_grid { "1" } else { "0" })),
            (_, Held::Number(n)) => {
                let value = self.fold(word, n);
                if value == n { WordChange::Same } else { WordChange::Write(Arc::from(value.to_string())) }
            }
            (_, Held::Absent | Held::Boolean(_)) => WordChange::Write(Arc::from(self.fold(word, 0).to_string())),
        }
    }

    /// The four words for a drawing that has none: every flag these settings give, in the form a file saved by the reference app writes it.
    pub fn words(&self) -> SnapWords {
        let mut words = SnapWords::default();
        for word in Word::ALL {
            let text = if word == Word::Dynamic { (if self.dynamic_grid { "1" } else { "0" }).to_owned() } else { self.fold(word, 0).to_string() };
            word.put(&mut words, text);
        }
        words
    }

    /// The attributes DiaCraft's part should hold for these settings, for a document whose words are `now`: the two switches when either is off, and, for an
    /// option whose word is text that is not a number (so that the option can be written nowhere else), its value when it is not the default. None when
    /// there is nothing to hold.
    fn part_attrs(&self, now: &SnapWords) -> Vec<(&'static str, String)> {
        let value = |on: bool| (if on { "1" } else { "0" }).to_owned();
        let mut attrs = Vec::new();
        if !(self.snap && self.glue) {
            attrs.push(("snap", value(self.snap)));
            attrs.push(("glue", value(self.glue)));
        }
        for (i, option) in OPTIONS.iter().enumerate() {
            if let Home::Word(word) = option.home
                && word.held(now) == Held::Unreadable
                && self.get(i) != option.default
            {
                attrs.push((option.name, value(self.get(i))));
            }
        }
        // The snap strengths that are not the defaults, in the order of the Advanced tab.
        for (i, def) in STRENGTHS.iter().enumerate() {
            let pixels = self.strength.get(i);
            if pixels != def.default {
                attrs.push((def.name, pixels.to_string()));
            }
        }
        attrs
    }

    /// Makes the document hold these settings. **Each word whose options changed is changed in its flags and its flags alone**; a word no option of which changed
    /// is not touched, even when it is not there. The switches go into DiaCraft's part (or out of it when both are on), and the part's child is written again
    /// with those two attributes (and the strengths that are not the defaults) only when it differs from them, so the options an earlier build kept there go with the next change (a value that was the
    /// part's alone, for a word the file lacks, is written into the word first; one for a word that is text stays in the part, as it can be written nowhere
    /// else). → `false`, and nothing written, for a word that is text and not a number (or a
    /// boolean) when one of its options changes, and for a document with data of another version in its part when a switch changes.
    pub fn apply(&self, doc: &mut Document) -> bool {
        let was = SnapSettings::of(doc);
        if was == *self {
            return true;
        }
        let now = doc.snap_words();
        let wanted = self.part_attrs(&now);
        let held: Vec<(String, String)> =
            doc.diacraft_child(PART_CHILD).map(|c| c.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()).unwrap_or_default();
        let clean = held.len() == wanted.len() && held.iter().zip(&wanted).all(|((k, v), (wk, wv))| k == wk && v == wv);
        let defaults = SnapSettings::default();
        let mut next = now.clone();
        for word in Word::ALL {
            let in_word = |o: &OptionDef| o.home == Home::Word(word);
            let changed = OPTIONS.iter().enumerate().any(|(i, o)| in_word(o) && was.get(i) != self.get(i));
            // A word the file lacks, whose options hold values only the part gave, is made when the part is rewritten, so that they are not lost.
            let carried =
                !clean && word.held(&now) == Held::Absent && OPTIONS.iter().enumerate().any(|(i, o)| in_word(o) && self.get(i) != defaults.get(i));
            if !(changed || carried) {
                continue;
            }
            match self.word_change(word, &now) {
                WordChange::Same => {}
                WordChange::Write(text) => word.put(&mut next, text.to_string()),
                WordChange::Refuse => return false,
            }
        }
        if !clean {
            let wrote = if wanted.is_empty() {
                doc.set_diacraft_child(PART_CHILD, None)
            } else {
                let attrs: Vec<(&str, &str)> = wanted.iter().map(|(k, v)| (*k, v.as_str())).collect();
                doc.set_diacraft_child(PART_CHILD, Some(attrs.as_slice()))
            };
            if !wrote {
                return false;
            }
        }
        if next != now {
            doc.set_snap_words(&next);
        }
        true
    }
}

/// The value of an attribute of the part: `1` or `0`.
fn part_value(text: &str) -> Option<bool> {
    match text {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// A new drawing: the blank document, with the four words of `DocumentSettings` a file saved by the reference app holds, at the defaults of [`SnapSettings`].
pub fn blank_drawing() -> Document {
    let mut doc = Document::new_blank();
    doc.set_snap_words(&SnapSettings::default().words());
    doc
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn words(glue: Option<&str>, snap: Option<&str>, extensions: Option<&str>, dynamic: Option<&str>) -> SnapWords {
        let a = |s: Option<&str>| s.map(Arc::<str>::from);
        SnapWords { glue: a(glue), snap: a(snap), extensions: a(extensions), dynamic_grid: a(dynamic) }
    }

    fn doc_with(words: &SnapWords) -> Document {
        let mut doc = Document::new_blank();
        doc.set_snap_words(words);
        doc
    }

    /// The names of the options of `word` that are on.
    fn on(s: &SnapSettings, word: Word) -> Vec<&'static str> {
        OPTIONS.iter().enumerate().filter(|(i, o)| o.home == Home::Word(word) && s.get(*i)).map(|(_, o)| o.name).collect()
    }

    #[test]
    fn the_table_has_each_documented_flag_once_and_the_defaults_make_the_words_a_vsdx_file_has() {
        // Ten flags of SnapSettings, five of GlueSettings, thirteen of SnapExtensions; DynamicGridEnabled; the two switches are the part's.
        let count = |w: Word| OPTIONS.iter().filter(|o| o.home == Home::Word(w)).count();
        assert_eq!((count(Word::Snap), count(Word::Glue), count(Word::Extensions), count(Word::Dynamic)), (10, 5, 13, 1));
        assert_eq!(OPTIONS.iter().filter(|o| o.home == Home::Part).map(|o| o.name).collect::<Vec<_>>(), ["snap", "glue"]);
        let mut names: Vec<_> = OPTIONS.iter().map(|o| o.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OPTIONS.len(), "no name twice");
        for word in [Word::Snap, Word::Glue, Word::Extensions] {
            let mut flags: Vec<i64> = OPTIONS.iter().filter(|o| o.home == Home::Word(word)).map(|o| o.flag).collect();
            let all = flags.len();
            flags.sort_unstable();
            flags.dedup();
            assert_eq!(flags.len(), all, "{word:?}: no flag twice");
            assert!(flags.iter().all(|f| f.count_ones() == 1), "{word:?}: each a single flag");
            assert!(!flags.contains(&32768), "{word:?}: the flag read as off is no option");
        }
        // The new drawing's words: the sums of the defaults (the sample values 65847, 9 and 34, and the 1).
        let w = SnapSettings::default().words();
        assert_eq!(
            (w.snap.as_deref(), w.glue.as_deref(), w.extensions.as_deref(), w.dynamic_grid.as_deref()),
            (Some("65847"), Some("9"), Some("34"), Some("1"))
        );
        assert_eq!(SnapSettings::from_words(&w), SnapSettings::default(), "and they read back as the defaults");
    }

    #[test]
    fn a_document_with_no_words_and_no_part_has_the_defaults() {
        assert_eq!(SnapSettings::of(&Document::new_blank()), SnapSettings::default());
        assert_eq!(SnapSettings::of(&blank_drawing()), SnapSettings::default());
        let d = SnapSettings::default();
        assert!(d.snap && d.glue && d.ruler && d.grid && d.guides && d.snap_connection_points && d.glue_connection_points && d.glue_guides);
        assert!(d.dynamic_grid && d.intersections && d.snap_vertices && d.snap_geometry && d.ext_centre_axes && d.ext_linear);
        assert!(!d.alignment_box && !d.glue_geometry && !d.glue_handles && !d.glue_vertices && !d.snap_handles && !d.ext_isometric);
    }

    #[test]
    fn every_documented_flag_is_read_from_its_word() {
        // 773 is the ruler, guides, outlines and the alignment box; 32800 is glue off with shape geometry kept.
        let s = SnapSettings::from_words(&words(None, Some("773"), None, None));
        assert_eq!(on(&s, Word::Snap), ["ruler", "guides", "snap_geometry", "alignment_box"]);
        let s = SnapSettings::from_words(&words(Some("32800"), None, None, None));
        assert_eq!(on(&s, Word::Glue), ["glue_geometry"]);
        assert!(s.glue, "32768 is kept and is not the switch");
        // One flag at a time, in each word: that option is on, every other option of the words is off, and the switches are the defaults.
        for (index, option) in OPTIONS.iter().enumerate() {
            let Home::Word(word) = option.home else { continue };
            if word == Word::Dynamic {
                continue;
            }
            let mut w = words(Some("0"), Some("0"), Some("0"), Some("0"));
            word.put(&mut w, option.flag.to_string());
            let s = SnapSettings::from_words(&w);
            for (j, other) in OPTIONS.iter().enumerate() {
                let expect = j == index || (other.home == Home::Part && other.default);
                assert_eq!(s.get(j), expect, "{} set alone, reading {}", option.name, other.name);
            }
        }
    }

    #[test]
    fn a_boolean_word_takes_all_four_literals_and_anything_else_is_not_read() {
        for (text, expect) in [("true", true), ("1", true), ("false", false), ("0", false), (" 1 ", true)] {
            assert_eq!(SnapSettings::from_words(&words(None, None, None, Some(text))).dynamic_grid, expect, "{text}");
        }
        for text in ["yes", "2", "TRUE", "-1", ""] {
            assert_eq!(SnapSettings::from_words(&words(None, None, None, Some(text))).dynamic_grid, SnapSettings::default().dynamic_grid, "{text:?}");
        }
    }

    #[test]
    fn a_word_that_is_not_a_number_is_a_word_that_is_absent() {
        for text in ["", "x", "1.5", "99999999999999999999", &"9".repeat(500)] {
            assert_eq!(SnapSettings::from_words(&words(Some(text), Some(text), Some(text), Some(text))), SnapSettings::default(), "{text:?}");
        }
    }

    #[test]
    fn a_change_sets_or_clears_its_flag_and_leaves_every_other_bit_of_the_word() {
        // The sample word 65847 with unknown bits added: 64 and 128 (named by no page), 32768 (the flag read as off), and a bit past 32.
        let held = 65847 | 64 | 128 | 32768 | (1 << 40);
        let mut doc = doc_with(&words(Some("9"), Some(&held.to_string()), Some("34"), Some("1")));
        let was = SnapSettings::of(&doc);
        assert!(was.ruler && was.grid && was.snap && !was.alignment_box);
        assert!(SnapSettings { grid: false, alignment_box: true, snap_vertices: false, ..was }.apply(&mut doc));
        let now: i64 = doc.snap_words().snap.unwrap().parse().unwrap();
        assert_eq!(now, (held & !2 & !16) | 512, "flags 2 and 16 cleared, 512 set, nothing else");
        let after = doc.snap_words();
        assert_eq!((after.glue.as_deref(), after.extensions.as_deref(), after.dynamic_grid.as_deref()), (Some("9"), Some("34"), Some("1")));
        assert!(doc.diacraft.is_none(), "no part for options of the words");
        // Back again.
        assert!(was.apply(&mut doc));
        assert_eq!(doc.snap_words().snap.as_deref(), Some(held.to_string().as_str()));
    }

    #[test]
    fn each_word_changes_alone() {
        let all = words(Some("9"), Some("65847"), Some("34"), Some("1"));
        for (option, word, expect) in [
            ("glue_geometry", Word::Glue, "41"),
            ("glue_connection_points", Word::Glue, "1"),
            ("ext_isometric", Word::Extensions, "4130"),
            ("ext_linear", Word::Extensions, "2"),
            ("alignment_box", Word::Snap, "66359"),
            ("dynamic_grid", Word::Dynamic, "0"),
        ] {
            let mut doc = doc_with(&all);
            let mut s = SnapSettings::of(&doc);
            let index = SnapSettings::index_of(option).unwrap();
            s.put(index, !s.get(index));
            assert!(s.apply(&mut doc), "{option}");
            let after = doc.snap_words();
            for other in Word::ALL {
                if other == word {
                    assert_eq!(other.text(&after), Some(expect), "{option}");
                } else {
                    assert_eq!(other.text(&after), other.text(&all), "{option} left {other:?} alone");
                }
            }
            assert_eq!(SnapSettings::of(&doc), s, "{option} reads back");
        }
    }

    #[test]
    fn a_boolean_is_written_one_or_zero_and_a_literal_that_already_says_it_stays() {
        let mut doc = doc_with(&words(None, None, None, Some("true")));
        assert!(SnapSettings { grid: false, ..SnapSettings::of(&doc) }.apply(&mut doc));
        assert_eq!(doc.snap_words().dynamic_grid.as_deref(), Some("true"), "not an option of that word that moved");
        assert!(SnapSettings { dynamic_grid: false, ..SnapSettings::of(&doc) }.apply(&mut doc));
        assert_eq!(doc.snap_words().dynamic_grid.as_deref(), Some("0"));
        assert!(SnapSettings { dynamic_grid: true, ..SnapSettings::of(&doc) }.apply(&mut doc));
        assert_eq!(doc.snap_words().dynamic_grid.as_deref(), Some("1"));
    }

    #[test]
    fn a_word_the_file_lacks_is_made_from_the_flags_and_only_when_one_of_its_options_changes() {
        let mut doc = Document::new_blank();
        assert!(SnapSettings::default().apply(&mut doc));
        assert_eq!(doc.snap_words(), SnapWords::default(), "nothing moved, nothing written");
        // A grid change makes SnapSettings from the settings' flags (the defaults' 65847 less the grid), and no other word.
        assert!(SnapSettings { grid: false, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.snap_words().snap.as_deref(), Some("65845"));
        assert!(doc.snap_words().glue.is_none() && doc.snap_words().dynamic_grid.is_none() && doc.snap_words().extensions.is_none());
        // An empty word is made too.
        let mut doc = doc_with(&words(None, Some(""), None, None));
        assert!(SnapSettings { ruler: false, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.snap_words().snap.as_deref(), Some("65846"));
    }

    #[test]
    fn a_word_that_is_text_is_left_alone_and_a_change_to_one_of_its_options_is_refused() {
        let mut doc = doc_with(&words(Some("9"), Some("junk"), Some("34"), Some("maybe")));
        let kept = doc.snap_words();
        assert!(!SnapSettings { ruler: false, ..SnapSettings::default() }.apply(&mut doc), "refused");
        assert!(!SnapSettings { dynamic_grid: false, ..SnapSettings::default() }.apply(&mut doc), "refused");
        assert_eq!(doc.snap_words(), kept);
        assert!(doc.diacraft.is_none(), "and a refused change writes nothing of the part either");
        assert!(!SnapSettings { ruler: false, glue: false, ..SnapSettings::default() }.apply(&mut doc));
        assert!(doc.diacraft.is_none());
        // An option of another word still changes.
        assert!(SnapSettings { glue_geometry: true, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.snap_words().glue.as_deref(), Some("41"));
    }

    #[test]
    fn a_word_that_is_negative_or_past_32_bits_keeps_every_bit_it_holds() {
        for (word, grid_off) in [("-1", "-3"), ("4294967299", "4294967297"), ("-4294967296", "-4294967296")] {
            let mut doc = doc_with(&words(None, Some(word), None, None));
            let s = SnapSettings::of(&doc);
            if s.grid {
                assert!(SnapSettings { grid: false, ..s }.apply(&mut doc), "{word}");
                assert_eq!(doc.snap_words().snap.as_deref(), Some(grid_off), "{word}: grid off");
            } else {
                assert!(SnapSettings { grid: true, ..s }.apply(&mut doc), "{word}");
                assert_eq!(doc.snap_words().snap.as_deref(), Some("-4294967294"));
            }
        }
        let mut doc = doc_with(&words(None, Some("99999999999999999999"), None, None));
        assert!(!SnapSettings { grid: false, ..SnapSettings::default() }.apply(&mut doc));
    }

    #[test]
    fn the_switches_are_the_parts_and_nothing_else_is() {
        let mut doc = doc_with(&SnapSettings::default().words());
        let before = doc.snap_words();
        assert!(SnapSettings { snap: false, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.snap_words(), before, "a switch moves no word, and the flag 32768 is not set");
        let child = doc.diacraft_child("snap").unwrap();
        assert_eq!((child.attrs.get("snap"), child.attrs.get("glue"), child.attrs.len()), (Some("0"), Some("1"), 2));
        assert!(!SnapSettings::of(&doc).snap && SnapSettings::of(&doc).glue);
        // Back on, and the child goes (a file with no choice has no part).
        assert!(SnapSettings::default().apply(&mut doc));
        assert!(doc.diacraft_child("snap").is_none() && doc.diacraft.is_none());
        // A held flag 32768 is not the switch.
        let doc = doc_with(&words(Some("32777"), Some("98615"), None, None));
        let s = SnapSettings::of(&doc);
        assert!(s.snap && s.glue);
    }

    #[test]
    fn a_part_from_m323_gives_its_options_where_the_word_is_absent_and_the_word_wins_where_it_is_there() {
        let part = [
            ("snap", "0"),
            ("alignment_box", "1"),
            ("intersections", "1"),
            ("guides", "0"),
            ("glue_geometry", "1"),
            ("glue_guides", "0"),
            ("dynamic_grid", "0"),
            ("from_a_later_build", "1"),
        ];
        // Only the part: its values stand for the options now in words.
        let mut doc = Document::new_blank();
        assert!(doc.set_diacraft_child("snap", Some(&part)));
        let s = SnapSettings::of(&doc);
        assert!(!s.snap && s.alignment_box && s.intersections && !s.guides && s.glue_geometry && !s.glue_guides && !s.dynamic_grid);
        // Both, and they disagree: the word's bit, whatever the part says. SnapSettings 773 has guides on and intersections off; GlueSettings 9 has glue to
        // guides on and to geometry off; the boolean is true.
        doc.set_snap_words(&words(Some("9"), Some("773"), None, Some("true")));
        let s = SnapSettings::of(&doc);
        assert!(s.guides && s.alignment_box && !s.intersections, "SnapSettings 773 is read: {s:?}");
        assert!(!s.glue_geometry && s.glue_guides && s.dynamic_grid, "GlueSettings 9 and the boolean are read, not the part: {s:?}");
        assert!(!s.snap, "the switch has no word, and is the part's");
        // A word that is absent leaves the part's value for its options.
        doc.set_snap_words(&words(Some("9"), Some("773"), None, None));
        assert!(!SnapSettings::of(&doc).dynamic_grid);
        // The next change of any option writes the child again with the switches only, and the part's value for the absent word goes into the word.
        assert!(SnapSettings { grid: true, ..SnapSettings::of(&doc) }.apply(&mut doc));
        let child = doc.diacraft_child("snap").unwrap();
        assert_eq!((child.attrs.get("snap"), child.attrs.get("glue"), child.attrs.len()), (Some("0"), Some("1"), 2), "the old attributes are gone");
        assert_eq!(doc.snap_words().dynamic_grid.as_deref(), Some("0"), "and the dynamic grid the part turned off is off in its word");
        assert!(!SnapSettings::of(&doc).dynamic_grid && SnapSettings::of(&doc).grid);
    }

    #[test]
    fn a_part_of_another_version_reads_as_the_defaults_and_is_not_changed() {
        let mut doc = Document::new_blank();
        doc.diacraft = Some(Arc::new(diacraft_doc::RawElement {
            name: Arc::from("dc:Data"),
            attrs: diacraft_doc::RawAttrs::from_pairs([("xmlns:dc", diacraft_doc::DIACRAFT_URN), ("dc:version", "7")]),
            children: vec![diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
                name: Arc::from("dc:snap"),
                attrs: diacraft_doc::RawAttrs::from_pairs([("snap", "0")]),
                children: Vec::new(),
            })],
        }));
        let kept = doc.diacraft.clone();
        assert_eq!(SnapSettings::of(&doc), SnapSettings::default());
        assert!(!SnapSettings { glue: false, ..SnapSettings::default() }.apply(&mut doc), "refused");
        assert_eq!(doc.diacraft, kept);
        assert!(doc.snap_words().snap.is_none(), "and nothing of it was written");
        // An option of a word can still change.
        assert!(SnapSettings { grid: false, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.diacraft, kept);
    }

    #[test]
    fn a_value_the_part_gives_to_a_word_that_is_text_stays_in_the_part_through_a_change_elsewhere() {
        // L3b: the part's guides=0 is read for a SnapSettings that is text; the next change of an option of another word rewrote the child
        // with the switches only and dropped it, so guides came back on.
        let mut doc = doc_with(&words(Some("9"), Some("junk"), Some("34"), Some("1")));
        assert!(doc.set_diacraft_child("snap", Some(&[("guides", "0"), ("alignment_box", "1"), ("glue", "1")])));
        let was = SnapSettings::of(&doc);
        assert!(!was.guides && was.alignment_box, "the part's values are read, the word being text");
        assert!(SnapSettings { glue_geometry: true, ..was }.apply(&mut doc));
        let child = doc.diacraft_child("snap").unwrap();
        let held: Vec<(&str, &str)> = child.attrs.iter().collect();
        assert_eq!(
            held,
            [("guides", "0"), ("alignment_box", "1")],
            "the values that cannot go in a word stay, in the table's order; the switches are on and not there"
        );
        let after = SnapSettings::of(&doc);
        assert!(!after.guides && after.alignment_box && after.glue_geometry);
        assert_eq!(doc.snap_words().snap.as_deref(), Some("junk"));
        assert_eq!(doc.snap_words().glue.as_deref(), Some("41"));
        // With a switch off as well: the switches first. And a value back at its default is not kept.
        assert!(SnapSettings { snap: false, ..after }.apply(&mut doc));
        let held: Vec<(String, String)> = doc.diacraft_child("snap").unwrap().attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        assert_eq!(held.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["snap", "glue", "guides", "alignment_box"]);
        let again = SnapSettings::of(&doc);
        assert!(
            !SnapSettings { guides: true, alignment_box: false, snap: true, ..again }.apply(&mut doc),
            "a change to an option of the text word is refused"
        );
        assert!(SnapSettings { snap: true, ..again }.apply(&mut doc));
        let held: Vec<(&str, &str)> = doc.diacraft_child("snap").unwrap().attrs.iter().collect();
        assert_eq!(held, [("guides", "0"), ("alignment_box", "1")]);
    }

    #[test]
    fn an_attribute_that_is_not_a_choice_is_left_and_not_read() {
        let mut doc = Document::new_blank();
        assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "1"), ("glue", "maybe"), ("from_a_later_build", "1"), ("guides", "0")])));
        let s = SnapSettings::of(&doc);
        assert!(s.alignment_box && s.glue && !s.guides);
    }

    #[test]
    fn the_six_strengths_are_in_the_order_of_the_advanced_tab_with_the_defaults_of_the_spec() {
        // Rulers 4, Grid 5, Guides 10, Points 10, Geometry 8, Extensions 13.
        let labels: Vec<&str> = STRENGTHS.iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Rulers", "Grid", "Guides", "Points", "Geometry", "Extensions"]);
        let defaults: Vec<u16> = STRENGTHS.iter().map(|s| s.default).collect();
        assert_eq!(defaults, [4, 5, 10, 10, 8, 13]);
        let d = Strength::default();
        assert_eq!((0..6).map(|i| d.get(i)).collect::<Vec<_>>(), defaults, "the struct and the table agree");
        assert_eq!(SnapSettings::default().strength, d);
        assert_eq!(SnapSettings::none().strength, d, "none turns off options, not strengths");
        let mut names: Vec<&str> = STRENGTHS.iter().map(|s| s.name).collect();
        names.extend(OPTIONS.iter().map(|o| o.name));
        names.sort_unstable();
        let all = names.len();
        names.dedup();
        assert_eq!(names.len(), all, "no strength is named as an option is");
        assert!(STRENGTHS.iter().all(|s| s.default <= STRENGTH_MAX));
        // get, put and index_of agree, and a put past the range or of no kind is refused and sets nothing.
        let mut s = Strength::default();
        for (i, def) in STRENGTHS.iter().enumerate() {
            assert_eq!(Strength::index_of(def.name), Some(i));
            assert!(s.put(i, 999) && s.get(i) == 999);
            assert!(s.put(i, 0) && s.get(i) == 0);
            assert!(!s.put(i, 1000) && s.get(i) == 0, "{} past 999", def.name);
        }
        assert!(!s.put(6, 5) && s.get(6) == 0 && Strength::index_of("strength_nothing").is_none());
    }

    #[test]
    fn strength_pixels_reads_a_whole_number_from_0_to_999_and_nothing_else() {
        for (text, want) in
            [("0", Some(0)), ("5", Some(5)), ("13", Some(13)), ("999", Some(999)), ("007", Some(7)), ("1000", None), ("", None), ("-1", None)]
        {
            assert_eq!(strength_pixels(text), want, "{text:?}");
        }
        for text in ["1.5", " 4", "4 ", "+4", "0x10", "ten", "99999999999999999999", "0999"] {
            assert_eq!(strength_pixels(text), None, "{text:?}");
        }
        // Digits of another script are not digits here.
        assert_eq!(strength_pixels("\u{0661}\u{0662}"), None);
        assert_eq!(strength_pixels("\u{ff11}\u{ff12}"), None);
    }

    #[test]
    fn the_strengths_are_kept_in_the_part_for_the_values_that_are_not_the_defaults_only() {
        let mut doc = doc_with(&SnapSettings::default().words());
        let before = doc.snap_words();
        assert!(SnapSettings { strength: Strength { grid: 9, extensions: 0, ..Strength::default() }, ..SnapSettings::default() }.apply(&mut doc));
        assert_eq!(doc.snap_words(), before, "no word holds a strength");
        let child = doc.diacraft_child("snap").unwrap();
        let held: Vec<(&str, &str)> = child.attrs.iter().collect();
        assert_eq!(held, [("strength_grid", "9"), ("strength_extensions", "0")], "in the order of the tab, the defaults left out");
        let read = SnapSettings::of(&doc);
        assert_eq!((read.strength.grid, read.strength.extensions, read.strength.ruler), (9, 0, 4));
        // A switch beside them: the switches first, and the strengths stay.
        assert!(SnapSettings { snap: false, ..read }.apply(&mut doc));
        let held: Vec<(&str, &str)> = doc.diacraft_child("snap").unwrap().attrs.iter().collect();
        assert_eq!(held, [("snap", "0"), ("glue", "1"), ("strength_grid", "9"), ("strength_extensions", "0")]);
        // A strength beside a switch: the switch stays.
        let read = SnapSettings::of(&doc);
        assert!(SnapSettings { strength: Strength { guides: 11, ..read.strength }, ..read }.apply(&mut doc));
        let now = SnapSettings::of(&doc);
        assert!(!now.snap && now.glue && now.strength.guides == 11 && now.strength.grid == 9);
        // The defaults again take the attributes out, and the child, and the part.
        assert!(SnapSettings::default().apply(&mut doc));
        assert!(doc.diacraft_child("snap").is_none() && doc.diacraft.is_none(), "a drawing with the defaults has no part");
    }

    #[test]
    fn an_attribute_that_is_not_a_strength_in_range_is_not_read_and_is_dropped_by_the_next_change() {
        let mut doc = Document::new_blank();
        let odd = [
            ("strength_grid", "1000"),
            ("strength_ruler", "-1"),
            ("strength_guides", "ten"),
            ("strength_points", " 7"),
            ("strength_extensions", "21"),
        ];
        assert!(doc.set_diacraft_child("snap", Some(&odd)));
        let read = SnapSettings::of(&doc);
        assert_eq!(read.strength, Strength { extensions: 21, ..Strength::default() });
        assert!(SnapSettings { strength: Strength { points: 3, ..read.strength }, ..read }.apply(&mut doc));
        let held: Vec<(&str, &str)> = doc.diacraft_child("snap").unwrap().attrs.iter().collect();
        assert_eq!(held, [("strength_points", "3"), ("strength_extensions", "21")], "what was not read is not kept");
    }

    #[test]
    fn data_of_another_version_holds_no_strength_and_refuses_one() {
        let mut doc = Document::new_blank();
        doc.diacraft = Some(Arc::new(diacraft_doc::RawElement {
            name: Arc::from("dc:Data"),
            attrs: diacraft_doc::RawAttrs::from_pairs([("xmlns:dc", diacraft_doc::DIACRAFT_URN), ("dc:version", "7")]),
            children: vec![diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
                name: Arc::from("dc:snap"),
                attrs: diacraft_doc::RawAttrs::from_pairs([("strength_grid", "40")]),
                children: Vec::new(),
            })],
        }));
        let kept = doc.diacraft.clone();
        assert_eq!(SnapSettings::of(&doc).strength, Strength::default());
        assert!(!SnapSettings { strength: Strength { grid: 8, ..Strength::default() }, ..SnapSettings::default() }.apply(&mut doc), "refused");
        assert_eq!(doc.diacraft, kept);
    }

    #[test]
    fn apply_then_of_gives_back_a_spread_of_settings_over_all_the_options() {
        // 400 settings over all 31 options, each written to the words and the part and read back, into a document with no words and one with the sample words.
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        for round in 0..400 {
            let mut s = SnapSettings::none();
            for i in 0..OPTIONS.len() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                s.put(i, seed & 1 == 1);
            }
            for (i, def) in STRENGTHS.iter().enumerate() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                // Half of them the default, the rest anywhere in 0 to 999.
                let px = if seed & 1 == 1 { def.default } else { u16::try_from((seed >> 8) % 1000).unwrap_or(0) };
                s.strength.put(i, px);
            }
            let mut doc = if round % 3 == 0 { Document::new_blank() } else { doc_with(&words(Some("9"), Some("65847"), Some("34"), Some("1"))) };
            assert!(s.apply(&mut doc));
            assert_eq!(SnapSettings::of(&doc), s, "round {round}");
        }
    }
}
