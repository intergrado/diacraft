//! ODF's held values and kept data: what an `.odg` holds that no cell means exactly (the native-format design's §3.2 to §3.4).
//!
//! **Held values** keep a construct exactly beside the stand-in cells that can only approximate it (an ODF hatch is a fill pattern in the cells and the
//! hatch's own definition here). **Each value has a stamp of its own**, the [`Stamp`] of the cells it stands beside as the reader set them, over the cells
//! names for that value: the groups of [`HeldGroup`] only group the values, and a group may hold several. A writer
//! of ODF uses a value **while its stamp still matches**, and otherwise writes from the cells as for a new shape: a held value never overrides a cell
//! (§3.1). An edit does not drop a held value (an undo may need it); it is stale until the cells come back. The frame's value and a circle's centre form
//! carry no stamp: the pose of the shape on the page, through every enclosing group, says whether they are good.
//!
//! Everything here is **ODF's**: filled by the `.odg` reader, written by the `.odg` writer, and carried by the VSDX writer, which cannot say it. **Today the reader fills**
//! the `kept` side of a shape's, a group's and a page's slot (the attributes and the children of the element that no field reads, first part), **the document's own
//! data**: the roots of `content.xml`, `styles.xml`, `meta.xml`, `settings.xml` and the manifest ([`OdfDoc::roots`]), `mimetype`, the manifest's rows beyond those
//! the package entries carry ([`OdfDoc::manifest_rows`]), the fields of `meta.xml` that no property reads ([`OdfDoc::meta`], beside `Document::title` and `DocProps`, whose `custom`
//! list holds the user-defined fields), `settings.xml` whole ([`OdfDoc::settings`]), the font faces (`Document::fonts`, each declaration held exactly in the face's own slot), and every
//! package entry the model reads as nothing else in [`Document::parts`](crate::Document::parts), **and:** the document's `definitions`, `layers` and `styles`
//! (every style element that is no style sheet, kept whole; an automatic style of the graphic, paragraph and text families is kept as what no cell reads), a style sheet's names,
//! family, `unmapped` properties and `units`, a page's master page, layout, drawing-page style, units and stamp, a shape's `style_name`, `held` values (each with the stamp of the
//! list for that value) and `units`, a `Connection` row's glue point, a picture's `draw:image` attributes, and a text's paragraphs. These are plain data in the `odf` side of an
//! [`Ext`](crate::package::Ext) slot.
//!
//! **What the `.odg` reader puts where** (conventions of its slots; the model holds the words as it is given them):
//!
//! * [`KeptStyle::family`] is `<container>:<kind>`: the container is `common` (`office:styles`), `auto` (an `office:automatic-styles` of either part, `styles.xml`'s first) or
//!   `master` (a master page of `office:master-styles` that holds no shape, kept whole), the kind the `style:family` of a `style:style` or `style:default-style`, or the
//!   element's local name (`list-style`, `page-layout`, `master-page`, ...). A default style's name is empty.
//! * [`OdfStyle::unmapped`] is keyed `<properties element>/<attribute>` as written (`style:graphic-properties/draw:fill-hatch-name`).
//! * [`OdfShape::units`] and the other `units` lists hold a length's unit as written (`px`, `cm`) **only where no cell spells it**: the cell's own unit is the unit of the length it
//!   was read from, and another unit than the cell's, or `px`, which a cell has none for, is held here.
//! * A **held value that rests on cells and on sections** has the one stamp [`crate::stamp_all`] makes of both; a value of the Geometry group that is the kept `draw:enhanced-geometry`
//!   holds **nothing but its stamp**, the element itself staying in the shape's `kept` markup, and a plain shape's outline is an element of the shape's own name holding the outline's
//!   attributes. A value is **copied from** the shape's own element or automatic style, and not lifted out of its kept markup.

use std::sync::Arc;

use crate::package::Kept;
use crate::raw::{Passthrough, RawAttrs, RawElement};
use crate::stamp::Stamp;

/// The most held definitions of one family a document holds: 65,536. A project rule (the native format's global constraints); checked at the door by
/// [`crate::Document::check_limits`].
pub const MAX_DEFINITIONS: usize = 65_536;

/// How the held values are grouped: the eight groups of native format, plan "Shared interfaces".
///
/// **A group is only a grouping**. A shape's `held` list may hold several values of one group, and each carries its own stamp
/// ([`Held::stamp`]) over the cells R22 names for that value, so that an edit of one cell drops the values that rest on it and no others (a colour edit
/// leaves a dash alone). The lists [`HeldGroup::cells`], [`HeldGroup::sections`] and [`HeldGroup::row_prefixes`] are what the table gives each group in
/// all, the union of what its values rest on: **the reader of a file stamps a value by the spec's list for that value, never by a group's.** `LineWeight`
/// is in one group only, `LineEnds`, for the width of DiaCraft's own marker that no size class matches: a foreign marker's width and a dash's lengths are
/// their own, so a weight edit leaves both right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeldGroup {
    /// `draw:fill` as `gradient`, `hatch` or `bitmap`, its names and parameters, `draw:opacity-name`: the gradient stops are the rows of `FillGradient`.
    Fill,
    /// A dash DiaCraft did not write and `draw:stroke-linejoin`.
    Line,
    /// A marker DiaCraft did not write at either end, with its width and centre flag.
    LineEnds,
    /// The shadow attributes the table does not map.
    Shadow,
    /// The text area's alignment (`justify`), padding as one attribute and writing mode.
    Text,
    /// A shear and the extra terms of `draw:transform`, and the form a circle or ellipse given by its centre was written in. **Not stamped**: these are
    /// judged by the shape's pose on the page through every enclosing group, so a value of this group has no stamp and the group stamps no cell.
    Frame,
    /// A glue point's element: held on a `Connection` row and stamped by that row **whole** ([`crate::stamp::stamp_row`]: every cell the row holds, and not four
    /// that a list names), so the group names no cell and [`HeldGroup::whole_rows`] says the row.
    Glue,
    /// The kept `draw:enhanced-geometry`, whole: its stamp is rows, the `Geometry` sections and the `User` rows `ODFEq<i>`, and no cell.
    Geometry,
}

impl HeldGroup {
    /// The cells the group's values rest on, in all: direct cells of the shape's sheet, stamped by [`crate::stamp::stamp_cells`]. **Empty for
    /// [`HeldGroup::Geometry`]**, whose stamp is rows, **for [`HeldGroup::Glue`]**, whose stamp is its whole row ([`HeldGroup::whole_rows`]), **and for
    /// [`HeldGroup::Frame`]**, which is not stamped. A value's own stamp covers the cells the spec names for it, which may be fewer.
    ///
    /// A stamp covers the shape's own sheet: the cells of an enclosing group, and the page's, are for the caller to mix in.
    pub fn cells(self) -> &'static [&'static str] {
        match self {
            // `FillForegnd` is the colour of a hatch read as `FillPattern` 1, which a colour set on it makes solid; the opacity name rests on `FillForegndTrans`; and a fill's
            // `FillGradientAngle`, the angle of a linear or axial gradient's stand-in, which the `.odg` reader's fill stamp covers.
            HeldGroup::Fill => &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillGradientAngle", "FillForegnd", "FillForegndTrans"],
            // The join rests on `LineCap` and `draw:stroke="dash"` on `LinePattern`; a foreign dash is written as held on every save, beside the `draw:stroke` a `LinePattern` of 0 or 1 writes.
            HeldGroup::Line => &["LineCap", "LinePattern"],
            // `LineWeight` is for the width of DiaCraft's own marker that no size class matches, which the splice writer names again on a weight edit.
            HeldGroup::LineEnds => &["BeginArrow", "BeginArrowSize", "EndArrow", "EndArrowSize", "LineWeight"],
            HeldGroup::Shadow => &["ShdwPattern", "ShapeShdwType", "ShdwForegnd", "ShdwForegndTrans", "ShapeShdwOffsetX", "ShapeShdwOffsetY"],
            HeldGroup::Text => &["VerticalAlign", "TopMargin", "BottomMargin", "LeftMargin", "RightMargin", "TextDirection"],
            // The pose of the shape and of every group around it says whether a frame value is good: no cell of the shape is stamped for it.
            HeldGroup::Frame => &[],
            HeldGroup::Glue => &[],
            HeldGroup::Geometry => &[],
        }
    }

    /// The sections whose rows the group's values rest on, all of them, stamped by [`crate::stamp::stamp_sections`]: `FillGradient` for the fill's
    /// stops and `Geometry` for the outline. None for the groups that are cells alone, and for [`HeldGroup::Glue`], whose one row is stamped on its own
    /// ([`HeldGroup::whole_rows`]).
    pub fn sections(self) -> &'static [&'static str] {
        match self {
            HeldGroup::Fill => &["FillGradient"],
            HeldGroup::Geometry => &["Geometry"],
            HeldGroup::Line | HeldGroup::LineEnds | HeldGroup::Shadow | HeldGroup::Text | HeldGroup::Frame | HeldGroup::Glue => &[],
        }
    }

    /// The sections whose **own row** a value of the group rests on, whole: the row the value is held on, stamped by [`crate::stamp::stamp_row`] with every cell it
    /// holds. `Connection` for [`HeldGroup::Glue`] ("that row's cells (`stamp_row`)"); none for the others.
    pub fn whole_rows(self) -> &'static [&'static str] {
        match self {
            HeldGroup::Glue => &["Connection"],
            HeldGroup::Fill
            | HeldGroup::Line
            | HeldGroup::LineEnds
            | HeldGroup::Shadow
            | HeldGroup::Text
            | HeldGroup::Frame
            | HeldGroup::Geometry => &[],
        }
    }

    /// The rows, by section name and the start of their `N`, that the group's values rest on beside its sections, stamped by
    /// [`crate::stamp::stamp_rows_named`]: the geometry's equations, the `User` rows `ODFEq<i>`. The `ODFMod<n>` rows beside them are left out: a
    /// handle's drag changes a modifier's value and not the geometry.
    pub fn row_prefixes(self) -> &'static [(&'static str, &'static str)] {
        match self {
            HeldGroup::Geometry => &[("User", "ODFEq")],
            HeldGroup::Fill | HeldGroup::Line | HeldGroup::LineEnds | HeldGroup::Shadow | HeldGroup::Text | HeldGroup::Frame | HeldGroup::Glue => &[],
        }
    }
}

/// A held value: the attributes (and the child elements) of one value of a group exactly as the file had them, and the stamp of the cells it stands beside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    /// The group it belongs to: a grouping only, since each value has a stamp of its own.
    pub group: HeldGroup,
    /// The stamp of the cells this value stands beside, over the cells names for it, as the reader set them: the value is good while
    /// the cells' stamp is this. `None` for a value that is not stamped: the frame's and a circle's centre form, which are judged by the shape's pose on
    /// the page through every enclosing group, a 1-D shape's transform and a connector's route, judged by the ends on the page and the
    /// page's height, and the values R22 says are written as held on every save (a foreign dash, a tab stop, a layer, a connector's type, a corner radius).
    pub stamp: Option<Stamp>,
    /// The attributes, verbatim and in order.
    pub attrs: RawAttrs,
    /// The child nodes, each at its place.
    pub kept: Passthrough,
}

/// A family of named definitions in `office:styles`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefFamily {
    /// `draw:marker`.
    Marker,
    /// `draw:gradient`.
    Gradient,
    /// `draw:opacity`.
    Opacity,
    /// `draw:hatch`.
    Hatch,
    /// `draw:stroke-dash`.
    StrokeDash,
    /// `draw:fill-image`.
    FillImage,
}

/// A named definition of the document, exactly as written: a marker's view box and path, a gradient's stops. At most [`MAX_DEFINITIONS`] of each family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// Its family.
    pub family: DefFamily,
    /// Its `draw:name`, by which shapes name it.
    pub name: Arc<str>,
    /// The element, kept whole: its display name and attributes as written.
    pub element: Arc<RawElement>,
    /// A fill image's bytes, which are a blob in a `.diacraft` file; `None` for every other family.
    pub bytes: Option<Arc<[u8]>>,
}

/// A style element of another family than the model's style sheets (paragraph, text, drawing-page, presentation, default and list styles, data styles),
/// kept whole by family and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptStyle {
    /// Its family (`style:family`, or the element's own kind for a list style).
    pub family: Arc<str>,
    /// Its `style:name`.
    pub name: Arc<str>,
    /// The element, kept whole.
    pub element: Arc<RawElement>,
}

/// The document's ODF data (`Document.ext.odf`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfDoc {
    /// The attributes and children of the document that no other field here holds.
    pub kept: Kept,
    /// `mimetype`, as the file had it.
    pub mimetype: Option<Arc<str>>,
    /// The attributes and namespace declarations on the roots of `content.xml`, `styles.xml`, `meta.xml`, `settings.xml` and the manifest, by file.
    pub roots: Vec<(Arc<str>, RawAttrs)>,
    /// The manifest's rows beyond the parts', by the part each names: media type and other attributes.
    pub manifest_rows: Vec<(Arc<str>, RawAttrs)>,
    /// `meta.xml`'s fields that no `DocProps` field reads, in order.
    pub meta: Passthrough,
    /// `settings.xml`, whole.
    pub settings: Option<Arc<RawElement>>,
    /// The named definitions of every family, in file order. At most [`MAX_DEFINITIONS`] of each family.
    pub definitions: Vec<Definition>,
    /// The layer set (`draw:layer-set`), each layer in order with its attributes.
    pub layers: Vec<Arc<RawElement>>,
    /// The other style families, kept whole.
    pub styles: Vec<KeptStyle>,
}

/// A page's ODF data (`Page.ext.odf`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfPage {
    /// What the page's markup holds that no other field here does.
    pub kept: Kept,
    /// `draw:master-page-name`.
    pub master_page: Option<Arc<str>>,
    /// The name of the page layout.
    pub layout: Option<Arc<str>>,
    /// The master page's own attributes.
    pub master_page_attrs: RawAttrs,
    /// The layout's properties that no page cell reads.
    pub layout_props: RawAttrs,
    /// The drawing-page style, whole.
    pub drawing_page_style: Option<Arc<RawElement>>,
    /// The unit each length attribute of the page was written in.
    pub units: RawAttrs,
    /// The stamp of the page's cells (size, margins, orientation) when the reader set them.
    pub stamp: Option<Stamp>,
}

/// A style sheet's ODF data (`Style.ext.odf`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfStyle {
    /// What the style's markup holds that no other field here does.
    pub kept: Kept,
    /// `style:name`.
    pub name: Option<Arc<str>>,
    /// `style:display-name`.
    pub display_name: Option<Arc<str>>,
    /// `style:parent-style-name`.
    pub parent: Option<Arc<str>>,
    /// `style:family`.
    pub family: Option<Arc<str>>,
    /// The properties of the style that no cell reads, exactly.
    pub unmapped: RawAttrs,
    /// The unit each length attribute of the style was written in.
    pub units: RawAttrs,
    /// The values the style gives that its cells only approximate (a hatch, a gradient or a picture for a fill, a dash or a marker DiaCraft did not write, a shadow, the text
    /// area's justification and padding and writing mode, a tab stop), each exactly as the file wrote it, with the stamp of the cells of the style it stands beside
    /// **the same values, held in the same way, as a shape's own automatic style gives the shape** ([`OdfShape::held`]), for a common style a shape names.
    pub held: Vec<Held>,
    /// The values the style's paragraph and text properties give that its `Paragraph` and `Character` cells (row 0) only approximate (an alignment spelled `start`, a line
    /// spacing, an underline's colour and a style other than `solid`, a lowercase transform, a position in percent), each exactly as the file wrote it, with its own stamp
    /// The values [`OdfTextStyle::held`] holds for a paragraph or a run that names an automatic style, for a common style.
    pub text_held: Vec<HeldStyleValue>,
}

/// A shape's ODF data (`Shape.ext.odf`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfShape {
    /// What the shape's markup holds that no other field here does.
    pub kept: Kept,
    /// The automatic style the shape named (`draw:style-name`).
    pub style_name: Option<Arc<str>>,
    /// The shape's held values, one for each group at the most.
    pub held: Vec<Held>,
    /// The unit each length attribute of the shape was written in.
    pub units: RawAttrs,
}

/// A row's ODF data (`Row.ext.odf`): a `Connection` row's glue point, or a text row's automatic style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfRow {
    /// What the row's markup holds that no other field here does.
    pub kept: Kept,
    /// The automatic style a paragraph or run named.
    pub style_name: Option<Arc<str>>,
    /// The row's held value (a glue point's).
    pub held: Option<Held>,
}

/// The groups of cells of a paragraph's or a run's row that feed one set of attributes of a text style: **the groups of `odg`'s splice writer** (`write/runs.rs`,
/// `owned` and `Part::sig`), under the same names. The table of the values held in such a style stamps by them: the alignment by `HorzAlign` and `Flags`, the
/// underline by bit 4 of `Style` and `DblUnderline` (and the rest of `Style`'s bits by the `Style` group, so a bold edit leaves a held underline as it is), the
/// line-through by `Strikethru` and `DoubleStrikethrough`, and each of the others by its own cell. A group is stamped by [`crate::stamp::stamp_text_group`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextGroup {
    /// A run's `Size`.
    Size,
    /// A run's `Style`, but for its underline bit (bit 4).
    Style,
    /// A run's underline: bit 4 of `Style`, and `DblUnderline`.
    Underline,
    /// A run's line-through: `Strikethru` and `DoubleStrikethrough`.
    Strike,
    /// A run's `Overline`.
    Overline,
    /// A run's `Case`.
    Case,
    /// A run's `Pos`.
    Pos,
    /// A run's `Color`.
    Color,
    /// A run's `Letterspace`.
    Letterspace,
    /// A run's `FontScale`.
    FontScale,
    /// A run's `Font`.
    Font,
    /// A run's `AsianFont`.
    AsianFont,
    /// A run's `ComplexScriptFont`.
    ComplexScriptFont,
    /// A paragraph's alignment: `HorzAlign`, which `Flags` (the writing mode) also feeds.
    HorzAlign,
    /// A paragraph's `Flags`.
    Flags,
    /// A paragraph's `IndLeft`.
    IndLeft,
    /// A paragraph's `IndRight`.
    IndRight,
    /// A paragraph's `IndFirst`.
    IndFirst,
    /// A paragraph's `SpBefore`.
    SpBefore,
    /// A paragraph's `SpAfter`.
    SpAfter,
    /// A paragraph's `SpLine`.
    SpLine,
}

impl TextGroup {
    /// Every group, runs' first.
    pub const ALL: [TextGroup; 21] = [
        TextGroup::Size,
        TextGroup::Style,
        TextGroup::Underline,
        TextGroup::Strike,
        TextGroup::Overline,
        TextGroup::Case,
        TextGroup::Pos,
        TextGroup::Color,
        TextGroup::Letterspace,
        TextGroup::FontScale,
        TextGroup::Font,
        TextGroup::AsianFont,
        TextGroup::ComplexScriptFont,
        TextGroup::HorzAlign,
        TextGroup::Flags,
        TextGroup::IndLeft,
        TextGroup::IndRight,
        TextGroup::IndFirst,
        TextGroup::SpBefore,
        TextGroup::SpAfter,
        TextGroup::SpLine,
    ];

    /// The group's name, the one the splice writer's `owned` and `Part::id` use.
    pub fn id(self) -> &'static str {
        match self {
            TextGroup::Size => "Size",
            TextGroup::Style => "Style",
            TextGroup::Underline => "Underline",
            TextGroup::Strike => "Strike",
            TextGroup::Overline => "Overline",
            TextGroup::Case => "Case",
            TextGroup::Pos => "Pos",
            TextGroup::Color => "Color",
            TextGroup::Letterspace => "Letterspace",
            TextGroup::FontScale => "FontScale",
            TextGroup::Font => "Font",
            TextGroup::AsianFont => "AsianFont",
            TextGroup::ComplexScriptFont => "ComplexScriptFont",
            TextGroup::HorzAlign => "HorzAlign",
            TextGroup::Flags => "Flags",
            TextGroup::IndLeft => "IndLeft",
            TextGroup::IndRight => "IndRight",
            TextGroup::IndFirst => "IndFirst",
            TextGroup::SpBefore => "SpBefore",
            TextGroup::SpAfter => "SpAfter",
            TextGroup::SpLine => "SpLine",
        }
    }

    /// Whether the group is a paragraph row's (its attributes are on `style:paragraph-properties`), and not a run's.
    pub fn is_paragraph(self) -> bool {
        matches!(
            self,
            TextGroup::HorzAlign
                | TextGroup::Flags
                | TextGroup::IndLeft
                | TextGroup::IndRight
                | TextGroup::IndFirst
                | TextGroup::SpBefore
                | TextGroup::SpAfter
                | TextGroup::SpLine
        )
    }

    /// The cells of the row the group reads, **whole cells' names**: `Style` is read by bit (the underline's bit 4 alone for [`TextGroup::Underline`], the
    /// other bits for [`TextGroup::Style`]), which [`crate::stamp::stamp_text_group`] does.
    pub fn cells(self) -> &'static [&'static str] {
        match self {
            TextGroup::Size => &["Size"],
            TextGroup::Style => &["Style"],
            TextGroup::Underline => &["Style", "DblUnderline"],
            TextGroup::Strike => &["Strikethru", "DoubleStrikethrough"],
            TextGroup::Overline => &["Overline"],
            TextGroup::Case => &["Case"],
            TextGroup::Pos => &["Pos"],
            TextGroup::Color => &["Color"],
            TextGroup::Letterspace => &["Letterspace"],
            TextGroup::FontScale => &["FontScale"],
            TextGroup::Font => &["Font"],
            TextGroup::AsianFont => &["AsianFont"],
            TextGroup::ComplexScriptFont => &["ComplexScriptFont"],
            TextGroup::HorzAlign => &["HorzAlign", "Flags"],
            TextGroup::Flags => &["Flags"],
            TextGroup::IndLeft => &["IndLeft"],
            TextGroup::IndRight => &["IndRight"],
            TextGroup::IndFirst => &["IndFirst"],
            TextGroup::SpBefore => &["SpBefore"],
            TextGroup::SpAfter => &["SpAfter"],
            TextGroup::SpLine => &["SpLine"],
        }
    }
}

/// A held value of a paragraph's or a run's style: attributes of the style exactly as written (the table of "held in the style", say `fo:text-align`
/// spelled `start`, or an underline's colour and width), with a stamp of its own over the cells of the paragraph's or the run's row that its keeping rests
/// on. It is no member of a [`HeldGroup`]: paragraph and run styles are held by name, not in the groups; its group is a [`TextGroup`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldStyleValue {
    /// The group of cells the value rests on: the stamp is [`crate::stamp::stamp_text_group`] of the row for it.
    pub group: TextGroup,
    /// The stamp of the cells this value stands beside, as the reader set them: the value is good while the row's stamp for its group is this. `None` for a
    /// value that is not stamped.
    pub stamp: Option<Stamp>,
    /// The attributes, verbatim and in order.
    pub attrs: RawAttrs,
    /// The child nodes, each at its place.
    pub kept: Passthrough,
}

/// What an `.odg` named for **one paragraph or one run** of a text: the automatic style it names, held
/// for that paragraph or run and never for the `Paragraph` or `Character` row it reads as, since paragraphs whose styles resolve alike share one row and
/// may name different styles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfTextStyle {
    /// The `style:name` the `text:p` or `text:span` names. `None` is "named none", which is held too: with a `stamp`, the paragraph or run names none
    /// while its row holds the cells it was read with.
    pub name: Option<Arc<str>>,
    /// The stamp of the paragraph's or the run's row, as read, by [`crate::stamp::stamp_text_row`]: the splice writer's key of the row (each cell's value text and
    /// unit verbatim, and not the row's `IX`, which the engine numbers anew when it formats part of a text). The name is written while the row is as read, and
    /// otherwise a style of the row's mapped attributes is.
    pub stamp: Option<Stamp>,
    /// The style's held values, each with a stamp of its own (the per-cell keeps inside a style).
    pub held: Vec<HeldStyleValue>,
}

/// One run of a paragraph, by where it lies in the paragraph's characters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfRun {
    /// Where the run starts and ends in the paragraph's characters (the file's paragraph, as the reader found it; the unit is the character): the writer's
    /// paragraph plan compares them.
    pub from: u32,
    /// See `from`.
    pub to: u32,
    /// The style the run's `text:span` names, and its held values. Empty for a run that is not a `text:span`.
    pub style: OdfTextStyle,
    /// The element the run is made of when it is not a plain `text:span`: its name and its attributes as the file had them, **without the style name a
    /// span's `style` holds and without its children**, which are the paragraph's characters and the runs inside this one. A `text:span` with an attribute besides its style
    /// name (`text:class-names`), a hyperlink (`text:a`), a field the model has no cell for (`text:title`, a date that is fixed) and a field it has one for, any other
    /// element of the paragraph's text: **the run is the characters the element shows**, and runs nest by where they lie. `None` for a `text:span` that has no other attribute.
    pub element: Option<Arc<RawElement>>,
}

/// The lists round a paragraph, at one depth: the `text:list` and the `text:list-item` or `text:list-header` the paragraph is in, **each held by the paragraph
/// that opens it and `None` for the paragraphs that continue it**. Each is the element's name and attributes as the file had them (`text:style-name`, `text:continue-numbering`,
/// `text:start-value`, `xml:id`), without its children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfListLevel {
    /// The `text:list` this paragraph opens at this depth.
    pub list: Option<Arc<RawElement>>,
    /// The `text:list-item` or `text:list-header` this paragraph opens at this depth.
    pub item: Option<Arc<RawElement>>,
}

/// The most levels of lists one paragraph is in: a list is an element of the file's tree, which is not deeper than the container's depth cap (an `.odg` of 200 levels has no
/// paragraph that is more than 100 lists deep).
pub const MAX_LIST_LEVELS: usize = 256;

/// One paragraph of **the file**: a `text:p` it had, in the file's order, and its runs. It is no paragraph of the text's parts (the `n`th stretch between U+000A):
/// those change with every edit, and the writer's paragraph plan matches the file's paragraphs to them by content when it saves (`Text.ext`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfParagraph {
    /// The style the `text:p` names, and its held values.
    pub style: OdfTextStyle,
    /// The stamp of the paragraph's characters when read: with `runs` and the rows' stamps, how the writer tells a paragraph an edit touched from one it
    /// did not (a retext adds `draw:auto-grow-height` only for a paragraph it edits).
    pub characters: Option<Stamp>,
    /// The runs the paragraph's `text:span`s and other elements gave, in the order they begin (an outer run before an inner one that begins where it does).
    pub runs: Vec<OdfRun>,
    /// The element of the paragraph when it is not a `text:p` with no attribute but its style name: a `text:h` (its `text:outline-level`), or a `text:p` with
    /// `text:class-names` or `xml:id`. Its name and attributes as the file had them, **without the style name `style` holds and without its children**.
    pub element: Option<Arc<RawElement>>,
    /// The lists the paragraph is in, outermost first: at most [`MAX_LIST_LEVELS`]. A paragraph of no list has none. **A list or an item that holds no paragraph is not held.**
    pub levels: Vec<OdfListLevel>,
}

/// A text's ODF data (`Text.ext.odf`): what the paragraphs and runs of its markup named, held **per paragraph and per run**, and what its markup held that
/// no other field here does. **It is the file's, and an edit of the text carries it forward**: the paragraphs are the file's, in its order, and the writer
/// matches them to the text's by content when it saves (`write/runs.rs`, `plan`), so an edit of one paragraph, or a newline typed before another, leaves the
/// held styles of the rest as they were. A text made new starts with none. Where these live was left; this is
/// the place the model gives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdfText {
    /// What the text's markup holds that no other field here does.
    pub kept: Kept,
    /// The file's paragraphs, in the file's order, one for each `text:p` it had: not the paragraphs of the text's parts, which the writer matches them to by
    /// content.
    pub paragraphs: Vec<OdfParagraph>,
}
