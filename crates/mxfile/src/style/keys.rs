//! The keys of a style the reader reads, how each value is read, and the values no style sets (S-R5, S-R9).
//!
//! A key that is in [`Key`] is **retained** in a resolved style, typed: a number (S-R5), a flag, a colour (S-R17), a text, or a text that must be one of a few words. Any other key
//! name changes nothing and is counted by name (S-R6). The list is the keys of the mapping spec's M-STYLE-TABLE, with the few keys the model task needs to see (`glass`, `sketch`,
//! `comic` and `rough`, which it counts as dropped effects, and the three keys that name a stencil's icon, M-KEEP); each is read by the rules of its [`Ty`].
//!
//! **The built-in values of S-R9** are part of the type: a number or a flag that no style sets reads as its [`Ty`] says, so that no consumer decides a default of its own.

use std::fmt;

use crate::decode::trim_js;

/// How the value of a key is read. A value that does not read this way is a bad value (S-R5, S-R17): the key is dropped, so that the built-in value of S-R9 applies, and one
/// `drawio_style_bad_value` names the key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ty {
    /// A finite decimal number (S-R5) within the read's coordinate range, and the built-in value of S-R9 when no style gives one (`None`: the key has no value of its own, and an end
    /// with no `exitX` is floating).
    Num(Option<f64>),
    /// A flag (S-R5): on for a number but 0 and for the text `true`, off for 0 and for any other text. The value when no style gives one. Never a bad value.
    Flag(bool),
    /// A colour or a colour keyword (S-R17), read by [`crate::style::colour`]. An empty value is no colour.
    Colour,
    /// Any text. An empty value is no value.
    Text,
    /// One of these words, as written (case counts). Any other text is a bad value.
    OneOf(&'static [&'static str]),
}

/// `left`, `center` and `right`: `align`, `labelPosition` and `imageAlign`.
const HORIZONTAL: &[&str] = &["left", "center", "right"];
/// `top`, `middle` and `bottom`: `verticalAlign`, `verticalLabelPosition` and `imageVerticalAlign`.
const VERTICAL: &[&str] = &["top", "middle", "bottom"];
/// `east`, `south`, `west` and `north`: `direction` and `gradientDirection`.
const COMPASS: &[&str] = &["east", "south", "west", "north"];
/// `horizontal` and `vertical`: `elbow`.
const ELBOW: &[&str] = &["horizontal", "vertical"];

/// Declares [`Key`]: its variants, their names in a style, and how each is read.
macro_rules! keys {
    ($( $(#[$doc:meta])* $variant:ident = $name:literal => $ty:expr ),* $(,)?) => {
        /// A key the reader reads. The order is the declaration's and has no meaning; `Ord` is for the sorted lists of a style.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Key { $( $(#[$doc])* $variant ),* }

        impl Key {
            /// Every key, in declaration order.
            pub const ALL: &'static [Key] = &[ $( Key::$variant ),* ];

            /// The key's name in a style string.
            pub fn name(self) -> &'static str {
                match self { $( Key::$variant => $name ),* }
            }

            /// How the key's value is read, and its built-in value.
            pub fn ty(self) -> Ty {
                match self { $( Key::$variant => $ty ),* }
            }

            /// The key a name in a style string is, if the reader reads it. Case counts (S-R1).
            pub fn from_name(name: &str) -> Option<Key> {
                match name { $( $name => Some(Key::$variant), )* _ => None }
            }
        }
    };
}

keys! {
    // ---- Flags the stylesheet and S-R9 leave off.
    /// `html`: the label is HTML (S-R24). Off unless set (S-R9).
    Html = "html" => Ty::Flag(false),
    /// `rounded`: rounded corners (S-R19), and rounded bends on a connector.
    Rounded = "rounded" => Ty::Flag(false),
    /// `dashed`: a dashed line (S-R18).
    Dashed = "dashed" => Ty::Flag(false),
    /// `shadow`: a drop shadow (S-R18).
    Shadow = "shadow" => Ty::Flag(false),
    /// `glass`: a gloss (S-R18). Lost: the shape is drawn plain; the model task counts it.
    Glass = "glass" => Ty::Flag(false),
    /// `sketch`: a hand-drawn look (S-R18). Lost, as `glass` is.
    Sketch = "sketch" => Ty::Flag(false),
    /// `comic`: a wobbly line (S-R18). Lost, as `glass` is.
    Comic = "comic" => Ty::Flag(false),
    /// `rough`: a rough line. Lost, as `glass` is.
    Rough = "rough" => Ty::Flag(false),
    /// `absoluteArcSize`: `arcSize` is a length, not a share (S-R19).
    AbsoluteArcSize = "absoluteArcSize" => Ty::Flag(false),
    /// `container`: the shape holds other shapes (S-R33).
    Container = "container" => Ty::Flag(false),
    /// `curved`: a connector's route is a smooth curve (S-R28).
    Curved = "curved" => Ty::Flag(false),
    /// `fixedSize`: `size` is in px, not a share (S-R11).
    FixedSize = "fixedSize" => Ty::Flag(false),
    /// `flipH`: the drawing is mirrored left to right (S-R20).
    FlipH = "flipH" => Ty::Flag(false),
    /// `flipV`: the drawing is mirrored top to bottom (S-R20).
    FlipV = "flipV" => Ty::Flag(false),
    /// `locked`: the cell may not be changed (S-R21, S-R34; M-PROTECT).
    Locked = "locked" => Ty::Flag(false),
    /// `noLabel`: the label is hidden (S-R22).
    NoLabel = "noLabel" => Ty::Flag(false),
    /// `inheritText`: a label cell takes its connector's text format.
    InheritText = "inheritText" => Ty::Flag(false),
    // ---- Flags that are on unless a style turns them off.
    /// `startFill`: the start mark is filled (S-R29).
    StartFill = "startFill" => Ty::Flag(true),
    /// `endFill`: the end mark is filled (S-R29).
    EndFill = "endFill" => Ty::Flag(true),
    /// `entryPerimeter`: the entry point is moved out to the perimeter (S-R30).
    EntryPerimeter = "entryPerimeter" => Ty::Flag(true),
    /// `exitPerimeter`: the exit point is moved out to the perimeter (S-R30).
    ExitPerimeter = "exitPerimeter" => Ty::Flag(true),
    /// `imageAspect`: a picture keeps its ratio (S-R35).
    ImageAspect = "imageAspect" => Ty::Flag(true),
    /// `horizontal`: the label runs along the shape; off turns it a quarter (S-R20).
    Horizontal = "horizontal" => Ty::Flag(true),
    /// `connectable`: connectors may be attached.
    Connectable = "connectable" => Ty::Flag(true),
    /// `movable` (S-R21, M-PROTECT).
    Movable = "movable" => Ty::Flag(true),
    /// `resizable` (S-R21, M-PROTECT).
    Resizable = "resizable" => Ty::Flag(true),
    /// `rotatable` (S-R21, M-PROTECT).
    Rotatable = "rotatable" => Ty::Flag(true),
    /// `deletable` (S-R21, M-PROTECT).
    Deletable = "deletable" => Ty::Flag(true),
    /// `editable` (S-R21, M-PROTECT).
    Editable = "editable" => Ty::Flag(true),
    /// `swimlaneLine`: the header has a line.
    SwimlaneLine = "swimlaneLine" => Ty::Flag(true),
    /// `left`: the left side of a `partialRectangle` is drawn.
    Left = "left" => Ty::Flag(true),
    /// `right`: the right side of a `partialRectangle` is drawn.
    Right = "right" => Ty::Flag(true),
    /// `top`: the top side of a `partialRectangle` is drawn.
    Top = "top" => Ty::Flag(true),
    /// `bottom`: the bottom side of a `partialRectangle` is drawn.
    Bottom = "bottom" => Ty::Flag(true),
    // ---- Numbers.
    /// `fontSize` in px (S-R22). The built-in value is 11 (the vector V2 of the style spec; S-R7 gives the stylesheet's 12 and 11).
    FontSize = "fontSize" => Ty::Num(Some(11.0)),
    /// `fontStyle`: bold 1, italic 2, underline 4, strikethrough 8 (S-R22).
    FontStyle = "fontStyle" => Ty::Num(Some(0.0)),
    /// `strokeWidth` in px (S-R9).
    StrokeWidth = "strokeWidth" => Ty::Num(Some(1.0)),
    /// `opacity`, 0 to 100 (S-R9).
    Opacity = "opacity" => Ty::Num(Some(100.0)),
    /// `fillOpacity`, 0 to 100 (S-R9).
    FillOpacity = "fillOpacity" => Ty::Num(Some(100.0)),
    /// `strokeOpacity`, 0 to 100 (S-R9).
    StrokeOpacity = "strokeOpacity" => Ty::Num(Some(100.0)),
    /// `textOpacity`, 0 to 100 (S-R9).
    TextOpacity = "textOpacity" => Ty::Num(Some(100.0)),
    /// `spacing` in px, added to each side (S-R9).
    Spacing = "spacing" => Ty::Num(Some(2.0)),
    /// `spacingTop` in px (S-R9).
    SpacingTop = "spacingTop" => Ty::Num(Some(0.0)),
    /// `spacingBottom` in px (S-R9).
    SpacingBottom = "spacingBottom" => Ty::Num(Some(0.0)),
    /// `spacingLeft` in px (S-R9).
    SpacingLeft = "spacingLeft" => Ty::Num(Some(0.0)),
    /// `spacingRight` in px (S-R9).
    SpacingRight = "spacingRight" => Ty::Num(Some(0.0)),
    /// `rotation` in degrees clockwise (S-R9, S-R20).
    Rotation = "rotation" => Ty::Num(Some(0.0)),
    /// `imageWidth` in px (S-R35).
    ImageWidth = "imageWidth" => Ty::Num(Some(42.0)),
    /// `imageHeight` in px (S-R35).
    ImageHeight = "imageHeight" => Ty::Num(Some(42.0)),
    /// `arcSize`: the corner (S-R19). No built-in value: it is 15 for a rectangle and 20 for any other outline.
    ArcSize = "arcSize" => Ty::Num(None),
    /// `size`: the shape's own measure (S-R11). No built-in value: it depends on the shape.
    Size = "size" => Ty::Num(None),
    /// `margin`: the inset of a `doubleEllipse`'s inner ellipse. No built-in value here: the outline gives it.
    Margin = "margin" => Ty::Num(None),
    /// `startSize`: a connector's start mark (S-R29, 6 px) or a swimlane's header (S-R9, 40 px). No built-in value: it depends on the cell.
    StartSize = "startSize" => Ty::Num(None),
    /// `endSize`: a connector's end mark in px (S-R29, 6 by default).
    EndSize = "endSize" => Ty::Num(None),
    /// `entryX`: where the target end is on its shape's box, 0 to 1 from the left (S-R30). Not set: the end is floating.
    EntryX = "entryX" => Ty::Num(None),
    /// `entryY`: where the target end is on its shape's box, 0 to 1 from the top (S-R30).
    EntryY = "entryY" => Ty::Num(None),
    /// `entryDx`: the target end's offset in px (S-R30).
    EntryDx = "entryDx" => Ty::Num(Some(0.0)),
    /// `entryDy`: the target end's offset in px (S-R30).
    EntryDy = "entryDy" => Ty::Num(Some(0.0)),
    /// `exitX`: where the source end is on its shape's box, 0 to 1 from the left (S-R30). Not set: the end is floating.
    ExitX = "exitX" => Ty::Num(None),
    /// `exitY`: where the source end is on its shape's box, 0 to 1 from the top (S-R30).
    ExitY = "exitY" => Ty::Num(None),
    /// `exitDx`: the source end's offset in px (S-R30).
    ExitDx = "exitDx" => Ty::Num(Some(0.0)),
    /// `exitDy`: the source end's offset in px (S-R30).
    ExitDy = "exitDy" => Ty::Num(Some(0.0)),
    // ---- Colours.
    /// `fillColor` (S-R17, S-R18).
    FillColor = "fillColor" => Ty::Colour,
    /// `strokeColor` (S-R17, S-R18).
    StrokeColor = "strokeColor" => Ty::Colour,
    /// `fontColor` (S-R17, S-R22).
    FontColor = "fontColor" => Ty::Colour,
    /// `gradientColor`: the colour a gradient fill ends in (S-R18).
    GradientColor = "gradientColor" => Ty::Colour,
    /// `labelBackgroundColor` (S-R22).
    LabelBackgroundColor = "labelBackgroundColor" => Ty::Colour,
    /// `swimlaneFillColor`: the body of a swimlane.
    SwimlaneFillColor = "swimlaneFillColor" => Ty::Colour,
    // ---- Words.
    /// `align`: the label's place across its box (S-R22).
    Align = "align" => Ty::OneOf(HORIZONTAL),
    /// `verticalAlign`: the label's place down its box (S-R22).
    VerticalAlign = "verticalAlign" => Ty::OneOf(VERTICAL),
    /// `labelPosition`: where the label's box is across the shape (S-R22).
    LabelPosition = "labelPosition" => Ty::OneOf(HORIZONTAL),
    /// `verticalLabelPosition`: where the label's box is down the shape (S-R22).
    VerticalLabelPosition = "verticalLabelPosition" => Ty::OneOf(VERTICAL),
    /// `imageAlign`: a picture's place across its box (S-R35).
    ImageAlign = "imageAlign" => Ty::OneOf(HORIZONTAL),
    /// `imageVerticalAlign`: a picture's place down its box (S-R35).
    ImageVerticalAlign = "imageVerticalAlign" => Ty::OneOf(VERTICAL),
    /// `direction`: the quarter turns of the drawing (S-R20).
    Direction = "direction" => Ty::OneOf(COMPASS),
    /// `gradientDirection`: where a gradient ends (S-R18).
    GradientDirection = "gradientDirection" => Ty::OneOf(COMPASS),
    /// `elbow`: which way an elbow connector bends first (S-R28).
    Elbow = "elbow" => Ty::OneOf(ELBOW),
    /// `shape`: the drawing's name, or `stencil(...)` (S-R10, S-R14).
    Shape = "shape" => Ty::Text,
    /// `edgeStyle`: how a connector's route is made (S-R28).
    EdgeStyle = "edgeStyle" => Ty::Text,
    /// `perimeter`: how connectors meet the outline (S-R7).
    Perimeter = "perimeter" => Ty::Text,
    /// `fontFamily`: a name, or a comma list of them (S-R22).
    FontFamily = "fontFamily" => Ty::Text,
    /// `whiteSpace`: only `wrap` wraps (S-R22).
    WhiteSpace = "whiteSpace" => Ty::Text,
    /// `overflow`: `visible`, `hidden`, `fill` or `width` (S-R22).
    Overflow = "overflow" => Ty::Text,
    /// `textDirection`: the direction of the label's paragraphs (S-R40): `rtl` is right to left, and every other value (`ltr`, `auto`, `default`, `vertical-lr`, `vertical-rl`) leaves the label
    /// as it is.
    TextDirection = "textDirection" => Ty::Text,
    /// `startArrow`: the start mark's name (S-R29).
    StartArrow = "startArrow" => Ty::Text,
    /// `endArrow`: the end mark's name (S-R29).
    EndArrow = "endArrow" => Ty::Text,
    /// `image`: a picture's address or data URI, or `def(n)` (S-R3, S-R35).
    Image = "image" => Ty::Text,
    /// `points`: a shape's own connection points (S-R31), as the text of the list.
    Points = "points" => Ty::Text,
    /// `dashPattern`: lengths in multiples of the line width (S-R18).
    DashPattern = "dashPattern" => Ty::Text,
    /// `jumpStyle`: the jump where a connector crosses another (S-R32).
    JumpStyle = "jumpStyle" => Ty::Text,
    /// `defaultLabelBackgroundColor`: `page` and others.
    DefaultLabelBackgroundColor = "defaultLabelBackgroundColor" => Ty::Text,
    /// `portConstraint`: the sides a floating end may use (S-R30).
    PortConstraint = "portConstraint" => Ty::Text,
    /// `sourcePortConstraint`: the connector's own, when its shape has none (S-R30).
    SourcePortConstraint = "sourcePortConstraint" => Ty::Text,
    /// `targetPortConstraint` (S-R30).
    TargetPortConstraint = "targetPortConstraint" => Ty::Text,
    /// `resIcon`: the icon a library shape names (M-KEEP).
    ResIcon = "resIcon" => Ty::Text,
    /// `prIcon`: the icon a library shape names (M-KEEP).
    PrIcon = "prIcon" => Ty::Text,
    /// `grIcon`: the icon a library shape names (M-KEEP).
    GrIcon = "grIcon" => Ty::Text,
}

impl Key {
    /// How many keys there are: the size of the table a style is resolved in.
    pub const COUNT: usize = Key::ALL.len();

    /// Whether the key is read and not drawn: `glass`, `sketch`, `comic` and `rough`. They are kept so that the model task can count the effects it drops, and S-R38 names them among
    /// the keys the importer ignores, so a cell that writes one has it counted in `drawio_style_keys_ignored` as well (a named style that adds one is not counted: the file did not
    /// write it).
    pub fn is_lost(self) -> bool {
        matches!(self, Key::Glass | Key::Sketch | Key::Comic | Key::Rough)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ numbers

/// The longest text read as a number, in bytes. A decimal number past it is far past any range of a style, and is a bad value.
pub const MAX_NUMBER_BYTES: usize = 64;

/// `text` as the decimal number JavaScript's `Number` reads from it, if it is a finite one (S-R5): optional white space round it, an optional sign, digits with an optional fraction
/// (or a fraction alone) and an optional exponent. `0x2`, `Infinity`, `1_000`, `--1` and the empty text are not numbers. `-0` is 0.
///
/// The text is judged by its first [`MAX_NUMBER_BYTES`] bytes after the white space is taken off: a longer one is not a number, and no walk goes further than that.
pub fn parse_decimal(text: &str) -> Option<f64> {
    let t = trim_js(text);
    if t.is_empty() || t.len() > MAX_NUMBER_BYTES || !is_decimal(t.as_bytes()) {
        return None;
    }
    let n: f64 = t.parse().ok()?;
    // `-0` and `1e999` (infinite) are the two surprises of the parse: the first is made 0, the second refused.
    if n.is_finite() { Some(if n == 0.0 { 0.0 } else { n }) } else { None }
}

/// Whether `t` is a decimal literal: `[+-]? (digits [. digits?]? | . digits) ([eE] [+-]? digits)?`.
fn is_decimal(t: &[u8]) -> bool {
    let mut i = 0;
    if matches!(t.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let digits = |from: usize| t.get(from..).map_or(0, |rest| rest.iter().take_while(|c| c.is_ascii_digit()).count());
    let whole = digits(i);
    i += whole;
    let mut fraction = 0;
    if t.get(i) == Some(&b'.') {
        i += 1;
        fraction = digits(i);
        i += fraction;
    }
    if whole == 0 && fraction == 0 {
        return false;
    }
    if matches!(t.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(t.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exponent = digits(i);
        if exponent == 0 {
            return false;
        }
        i += exponent;
    }
    i == t.len()
}

/// `text` as a number a style may hold: a finite decimal ([`parse_decimal`]) of at most `max` in size, either sign. Anything else is `None`, a bad value (S-R5).
pub fn parse_number(text: &str, max: f64) -> Option<f64> {
    parse_decimal(text).filter(|n| n.abs() <= max)
}

/// `text` as a flag (S-R5): on for any number but 0 and for the text `true`; off for 0 and for any other text.
pub fn parse_flag(text: &str) -> bool {
    match parse_decimal(text) {
        Some(n) => n != 0.0,
        None => text == "true",
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ names that stand for text

/// The `n` of a value that is `def(n)` (S-R3): only digits are accepted, and a number too large to name a `def` names none. The consumer looks the `def` up and
/// takes its text in place of the value; **a style's text is never expanded**, so a picture shared by a thousand shapes is held once (design, "Style compression by reference").
pub fn def_ref(value: &str) -> Option<usize> {
    let digits = value.strip_prefix("def(")?.strip_suffix(')')?;
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The text of a `shape` value of the form `stencil(<text>)` (S-R14): what lies between `stencil(` and the last character of the value, which draw.io takes without checking that
/// it is the closing bracket. A value that is not of that form, or too short to hold the bracket, is `None`.
pub fn stencil_payload(shape: &str) -> Option<&str> {
    let rest = shape.strip_prefix("stencil(")?;
    let last = rest.char_indices().next_back()?.0;
    rest.get(..last)
}
