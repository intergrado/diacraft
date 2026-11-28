//! What the ribbon holds: tabs, their groups in order, and each group's controls, as tables.
//!
//! A control's `id` is the
//! name the map gives it. Its caption is a label the map confirms ([`Ctl::l`]) or, where the map has none, a caption of
//! DiaCraft's own that stands in until a source confirms one ([`Ctl::g`]; the map says not to derive a label from an id,
//! so none is derived: each is written out by hand and [`unconfirmed_captions`] lists them). A control works only when it
//! names a [`Ctl::command`] that exists; every other control is drawn disabled.
//!
//! Sizes are the layout's own choice (the map gives order and names, not sizes): a [`Slot::Big`] is a
//! large button, a [`Slot::Stack`] a column of up to three small ones with their labels, a [`Slot::Rows`] rows of small
//! icon buttons (Font, Paragraph), a [`Slot::Gallery`] a wide strip of tiles.

use super::line_ends::NAMES;
use crate::icons::{Badge, Base, Icon};

/// What kind of control it is, which decides how it draws and what a press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Runs its command.
    Button,
    /// Opens a menu of [`Ctl::items`].
    Menu,
    /// Runs its command, with a separate arrow that opens the menu.
    Split,
    /// A check box: its command toggles.
    Check,
    /// A box with a drop-down arrow (a font, a size).
    Combo,
    /// A strip of tiles with a drop-down arrow at its end.
    Gallery,
}

/// What a control that stands for a style or an alignment shows of the selection (`format.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    None,
    Bold,
    Italic,
    Underline,
    Strike,
    Left,
    Center,
    Right,
    Justify,
    /// The lines of the paragraphs fill the margins, the last too (`HorzAlign` 4). No ribbon button has it (`ParagraphDistributed` is off the ribbon): the Text dialog's
    /// Alignment choices do.
    Distributed,
    Top,
    Middle,
    Bottom,
    /// The paragraphs have a bullet.
    Bullets,
    /// The text runs down the block (`TextDirection` 1).
    Vertical,
    /// The paragraphs read left to right, or right to left (`Flags`).
    Ltr,
    Rtl,
    /// The letters show as capitals, or as small capitals (Change Case).
    AllCaps,
    SmallCaps,
    /// Auto Size of the page on view (`page.auto_size`): pressed when the page grows by itself.
    AutoSize,
}

/// What a menu holds besides its items: a list the window makes from the document or from the engine's tables, which a table of `&'static` controls
/// cannot hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    None,
    /// The named page sizes, in the groups of `page.presets`, with the size of the page on view marked (`page.size`).
    PageSizes,
    /// The tiles of the Themes gallery, from `theme.list` (`ribbon/design.rs`).
    Themes,
    /// The tiles of the Variants gallery: the four variants of the page's theme.
    Variants,
    /// The four colour variants of the page's theme, as the rows of the Colors menu.
    VariantColours,
    /// The tiles of the container gallery (`container_ui.rs`).
    ContainerStyles,
    /// The tiles of the Callout gallery: the engine's fourteen, drawn as pictures (`ribbon/callouts.rs`).
    Callouts,
    /// The tiles of the Re-Layout Page gallery: the layouts of the engine, each a picture of what it makes (`relayout_ui.rs`).
    Layouts,
    /// The tiles of the Change Shape gallery: the masters of the stencils that are open, for the shapes selected (`ribbon/change_shape.rs`).
    Masters,
    /// The Variant Styles and the Theme Styles of the Quick Styles gallery, for the first shape selected (`ribbon/quick_styles.rs`).
    QuickStyles,
    /// The check boxes of the ShapeSheet window's Show Sections card, one for each section the sheet on top shows (`sheet/cards.rs`).
    SheetSections,
}

/// What a row of a menu stands for in the state of the page on view, so that the row of the one the page has is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None,
    /// The page is held this way, `portrait` or `landscape`.
    Orientation(&'static str),
    /// The page's default connector style is this one, as `connector.style` spells it (`right_angle`, `straight`, `curved`), and no connector is selected.
    ConnectorStyle(&'static str),
    /// The page has no theme.
    NoTheme,
    /// The letters of the selection show this case, as `text.format` spells it (`normal`, `all_caps`, `initial_caps`, `small_caps`).
    Case(&'static str),
    /// The paragraphs of the selection have this bullet (0 for none).
    Bullet(u8),
    /// The containers selected resize this way, as `container.auto_resize` spells it (`none`, `expand`, `fit`).
    ContainerResize(&'static str),
    /// The containers selected have their heading on this side (`top`, `right`, `bottom`, `left`).
    ContainerHeading(&'static str),
    /// The containers selected have this margin, a word of `container.margin` (`none`, `narrow`, `medium`, `wide`).
    ContainerMargin(&'static str),
    /// The lanes of the page's structure lie this way, as `swimlane.orientation` spells it (`horizontal`, `vertical`).
    SwimlaneOrientation(&'static str),
    /// The page's org chart is laid out this way, a word of `orgchart.layout`.
    OrgLayout(&'static str),
    /// The position selected arranges its reports this way, a word of `orgchart.arrange`.
    OrgArrangement(&'static str),
}

/// A caption of a part of a menu that the map does not list as a control (a row of a palette): its words, and whether the map confirms them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caption {
    pub text: &'static str,
    /// The map confirms [`Caption::text`]. None of the palette's does: the map names the galleries and not what is in them.
    pub confirmed: bool,
}

/// A palette of colours at the top of a menu: a press on a colour runs the menu control's command with the colour as `#rrggbb` (or `theme:accent2:40`, `default`, `none`) under `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub key: &'static str,
    /// The row that sends `none` under `key` (No Fill, No Line), if the colour can be taken away.
    pub none: Option<Caption>,
    /// The row that opens the Colors dialog (More Colors...).
    pub more: Caption,
    /// The row that hands the colour to the theme (Use Default), sent as `default`.
    pub default: Caption,
    /// Whether the command takes a colour of the page's theme and `default` under `key`: the palette then has Use Default, Theme Colors and Variant Colors above the fixed colours.
    pub themed: bool,
}

/// What a menu item draws at its left, in place of an icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    Icon,
    /// A line of this weight, in points.
    Weight(f32),
    /// A line of this dash pattern (`LinePattern`, 1 to 23).
    Dash(u8),
    /// The end of a line with this line end number, at its begin or its end.
    Arrow {
        number: u8,
        begin: bool,
    },
}

/// One control.
#[derive(Clone, Copy, Debug)]
pub struct Ctl {
    /// The control's id in the map (`Cut`, `FillColorGallery`).
    pub id: &'static str,
    /// The caption.
    pub label: &'static str,
    /// The map confirms [`Ctl::label`].
    pub confirmed: bool,
    pub icon: Icon,
    pub kind: Kind,
    /// The command it runs: an engine command or one of the window's ([`crate::commands`]). `None`: it does nothing yet.
    pub command: Option<&'static str>,
    /// The parameters it runs the command with, as the text of a JSON object (`""`: none).
    pub args: &'static str,
    /// What it shows of the selection, if it stands for a style or an alignment.
    pub press: Press,
    /// The colours at the top of its menu, if it is a colour menu: it runs `command` with the colour.
    pub palette: Option<Palette>,
    /// How its row in a menu draws its sample.
    pub sample: Sample,
    /// What its row in a menu is marked for, in the state of the page.
    pub mark: Mark,
    /// What its menu holds before its items.
    pub body: Body,
    /// The controls inside a menu, split button or gallery.
    pub items: &'static [Ctl],
}

const fn i(base: Base) -> Icon {
    Icon::of(base)
}

/// A button with this id and icon, and the id as its caption until [`Ctl::l`] or [`Ctl::g`] says otherwise.
const fn c(id: &'static str, icon: Icon) -> Ctl {
    Ctl {
        id,
        label: id,
        confirmed: false,
        icon,
        kind: Kind::Button,
        command: None,
        args: "",
        press: Press::None,
        palette: None,
        sample: Sample::Icon,
        mark: Mark::None,
        body: Body::None,
        items: &[],
    }
}

impl Ctl {
    /// The caption, which the map confirms.
    const fn l(mut self, label: &'static str) -> Ctl {
        self.label = label;
        self.confirmed = true;
        self
    }

    /// The caption, which the map does not give: ours until a source confirms one.
    const fn g(mut self, label: &'static str) -> Ctl {
        self.label = label;
        self.confirmed = false;
        self
    }

    const fn menu(mut self, items: &'static [Ctl]) -> Ctl {
        self.kind = Kind::Menu;
        self.items = items;
        self
    }

    const fn split(mut self, items: &'static [Ctl]) -> Ctl {
        self.kind = Kind::Split;
        self.items = items;
        self
    }

    const fn gallery(mut self, items: &'static [Ctl]) -> Ctl {
        self.kind = Kind::Gallery;
        self.items = items;
        self
    }

    const fn check(mut self) -> Ctl {
        self.kind = Kind::Check;
        self
    }

    const fn combo(mut self) -> Ctl {
        self.kind = Kind::Combo;
        self
    }

    const fn cmd(mut self, command: &'static str) -> Ctl {
        self.command = Some(command);
        self
    }

    /// The command it runs, with these parameters (the text of a JSON object).
    const fn cmd_with(mut self, command: &'static str, args: &'static str) -> Ctl {
        self.command = Some(command);
        self.args = args;
        self
    }

    const fn press(mut self, press: Press) -> Ctl {
        self.press = press;
        self
    }

    /// A menu that opens with a palette of colours; a press on one runs `command` with it under `key`.
    const fn palette(mut self, key: &'static str, none: Option<&'static str>) -> Ctl {
        // The captions are ours, so they are marked unconfirmed (`unconfirmed_palette_captions`).
        let none = match none {
            Some(text) => Some(Caption { text, confirmed: false }),
            None => None,
        };
        self.palette = Some(Palette {
            key,
            none,
            more: Caption { text: "More Colors...", confirmed: false },
            default: Caption { text: "Use Default", confirmed: false },
            themed: true,
        });
        self
    }

    const fn sample(mut self, sample: Sample) -> Ctl {
        self.sample = sample;
        self
    }

    /// A row that is marked while the page is in the state `mark` names.
    const fn mark(mut self, mark: Mark) -> Ctl {
        self.mark = mark;
        self
    }

    /// A menu that opens with a list the window makes (`Body`).
    const fn body(mut self, body: Body) -> Ctl {
        self.body = body;
        self
    }
}

/// How a group lays a control out.
#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Big(Ctl),
    Stack(&'static [Ctl]),
    Rows(&'static [&'static [Ctl]]),
    Gallery(Ctl),
}

/// A group: its controls in order, its label under them, and whether it has a launcher at its lower right.
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
    /// The map or an image confirms [`Group::label`].
    pub confirmed: bool,
    pub slots: &'static [Slot],
    /// The dialog launcher at the group's lower right, with the control it stands for.
    pub launcher: Option<Ctl>,
    /// The icon of the single button the group folds into when the window is narrow.
    pub icon: Icon,
}

const fn g(id: &'static str, label: &'static str, confirmed: bool, icon: Icon, slots: &'static [Slot]) -> Group {
    Group { id, label, confirmed, slots, launcher: None, icon }
}

impl Group {
    const fn launch(mut self, c: Ctl) -> Group {
        self.launcher = Some(c);
        self
    }
}

/// A tab.
#[derive(Clone, Copy, Debug)]
pub struct Tab {
    pub id: &'static str,
    pub label: &'static str,
    pub groups: &'static [Group],
}

const PLUS: Badge = Badge::Plus;
const NONE: &[Ctl] = &[];

// ---------------------------------------------------------------------------------------------------- Home

const PASTE_ITEMS: &[Ctl] = &[
    c("Paste", i(Base::Clipboard)).g("Paste").cmd("clipboard.paste"),
    c("PasteSpecialDialog", i(Base::Clipboard)).l("Paste Special"),
    c("PasteAsHyperlink", i(Base::Link)).g("Paste as Hyperlink"),
    c("PasteDuplicate", i(Base::Pages)).g("Paste Duplicate").cmd("clipboard.duplicate"),
];
/// The items of Change Case: the dialog's Case box, one at a time. **Their parameters are `text.format`'s.**
const CASE_ITEMS: &[Ctl] = &[
    c("TextNormalCase", i(Base::CaseAa)).g("Normal Case").cmd_with("format.text", r#"{"case":"normal"}"#).mark(Mark::Case("normal")),
    c("TextAllCaps", i(Base::CaseAa)).g("All Caps").cmd_with("format.text", r#"{"case":"all_caps"}"#).mark(Mark::Case("all_caps")),
    c("TextInitialCaps", i(Base::CaseAa)).g("Initial Caps").cmd_with("format.text", r#"{"case":"initial_caps"}"#).mark(Mark::Case("initial_caps")),
    c("TextSmallCaps", i(Base::CaseAa)).g("Small Caps").cmd_with("format.text", r#"{"case":"small_caps"}"#).mark(Mark::Case("small_caps")),
];
/// The items of the Bullets menu: None and the seven marks of `Bullet`'s table. **The ids and the captions are ours**: the map names the button
/// and no style (the glyphs of the styles are not described).
const BULLET_ITEMS: &[Ctl] = &[
    c("BulletNone", i(Base::Bullets)).g("None").cmd_with("format.text", r#"{"bullet":0}"#).mark(Mark::Bullet(0)),
    c("BulletRound", i(Base::Bullets)).g("Round Bullet").cmd_with("format.text", r#"{"bullet":1}"#).mark(Mark::Bullet(1)),
    c("BulletCircle", i(Base::Bullets)).g("Circle").cmd_with("format.text", r#"{"bullet":2}"#).mark(Mark::Bullet(2)),
    c("BulletSquare", i(Base::Bullets)).g("Square").cmd_with("format.text", r#"{"bullet":3}"#).mark(Mark::Bullet(3)),
    c("BulletHollowSquare", i(Base::Bullets)).g("Hollow Square").cmd_with("format.text", r#"{"bullet":4}"#).mark(Mark::Bullet(4)),
    c("BulletDiamond", i(Base::Bullets)).g("Diamond").cmd_with("format.text", r#"{"bullet":5}"#).mark(Mark::Bullet(5)),
    c("BulletArrow", i(Base::Bullets)).g("Arrow").cmd_with("format.text", r#"{"bullet":6}"#).mark(Mark::Bullet(6)),
    c("BulletCheck", i(Base::Bullets)).g("Check Mark").cmd_with("format.text", r#"{"bullet":7}"#).mark(Mark::Bullet(7)),
];
const DRAWING_TOOLS: &[Ctl] = &[
    c("RectangleTool", i(Base::Rect)).l("Rectangle").cmd("tool.rectangle"),
    c("EllipseTool", i(Base::Ellipse)).l("Ellipse").cmd("tool.ellipse"),
    c("LineTool", i(Base::Line)).l("Line").cmd("tool.line"),
    c("FreeformTool", i(Base::Freeform)).l("Freeform").cmd("tool.freeform"),
    c("ArcTool", i(Base::Arc)).l("Arc").cmd("tool.arc"),
    c("PencilTool", i(Base::Pencil)).l("Pencil").cmd("tool.pencil"),
];
const STYLES_ITEMS: &[Ctl] =
    &[c("AllowThemes", i(Base::Palette)).l("Allow Themes"), c("RemoveTheme", i(Base::Palette).with(Badge::Cross)).l("Remove Theme")];
/// A transparency item of the Fill or the Line menu: `format.shape` with the key and the percent. **Ids of items under a map id are ours**, as are their captions (the map lists the
/// galleries and not what is in them).
macro_rules! transparency {
    ($id:literal, $label:literal, $key:literal, $percent:literal) => {
        c($id, i(Base::Bucket)).g($label).cmd_with("format.shape", concat!("{\"", $key, "\":", $percent, "}"))
    };
}
const FILL_TRANSPARENCY_ITEMS: &[Ctl] = &[
    transparency!("FillTransparency0", "0% (opaque)", "fill_transparency", 0),
    transparency!("FillTransparency25", "25%", "fill_transparency", 25),
    transparency!("FillTransparency50", "50%", "fill_transparency", 50),
    transparency!("FillTransparency75", "75%", "fill_transparency", 75),
    transparency!("FillTransparency100", "100% (clear)", "fill_transparency", 100),
    c("FillTransparencyMore", i(Base::Bucket)).g("More Transparency Options...").cmd("format.options"),
];
const LINE_TRANSPARENCY_ITEMS: &[Ctl] = &[
    transparency!("LineTransparency0", "0% (opaque)", "line_transparency", 0),
    transparency!("LineTransparency25", "25%", "line_transparency", 25),
    transparency!("LineTransparency50", "50%", "line_transparency", 50),
    transparency!("LineTransparency75", "75%", "line_transparency", 75),
    transparency!("LineTransparency100", "100% (clear)", "line_transparency", 100),
    c("LineTransparencyMore", i(Base::PenLine)).g("More Transparency Options...").cmd("format.options"),
];
/// A gradient preset of the Fill menu: `format.gradient` for the shapes selected, with the colour they have and a lighter one (the engine's start). The pane's Gradient fill holds the stops.
const GRADIENT_ITEMS: &[Ctl] = &[
    c("FillGradientDown", i(Base::Bucket)).g("Linear, Top to Bottom").cmd_with("format.gradient", r#"{"kind":"linear","angle":90}"#),
    c("FillGradientUp", i(Base::Bucket)).g("Linear, Bottom to Top").cmd_with("format.gradient", r#"{"kind":"linear","angle":270}"#),
    c("FillGradientRight", i(Base::Bucket)).g("Linear, Left to Right").cmd_with("format.gradient", r#"{"kind":"linear","angle":0}"#),
    c("FillGradientLeft", i(Base::Bucket)).g("Linear, Right to Left").cmd_with("format.gradient", r#"{"kind":"linear","angle":180}"#),
    c("FillGradientRadial", i(Base::Bucket)).g("Radial, from the Center").cmd_with("format.gradient", r#"{"kind":"radial","origin":"center"}"#),
    c("FillGradientRectangular", i(Base::Bucket))
        .g("Rectangular, from the Center")
        .cmd_with("format.gradient", r#"{"kind":"rectangular","origin":"center"}"#),
    c("FillGradientMore", i(Base::Bucket)).g("More Gradients...").cmd("format.options"),
];
/// A pattern item of the Fill menu: `format.shape` with the `FillPattern` number. The ground is what the shape has.
macro_rules! pattern {
    ($id:literal, $label:literal, $n:literal) => {
        c($id, i(Base::Bucket)).g($label).cmd_with("format.shape", concat!("{\"fill_pattern\":", $n, "}"))
    };
}
const PATTERN_ITEMS: &[Ctl] = &[
    pattern!("FillPatternSolid", "Solid", 1),
    pattern!("FillPattern2", "Diagonal lines, rising", 2),
    pattern!("FillPattern3", "Grid", 3),
    pattern!("FillPattern4", "Diagonal lattice", 4),
    pattern!("FillPattern5", "Diagonal lines, falling", 5),
    pattern!("FillPattern6", "Horizontal lines", 6),
    pattern!("FillPattern7", "Vertical lines", 7),
    pattern!("FillPattern8", "Dense speckle", 8),
    pattern!("FillPattern9", "Speckle", 9),
    pattern!("FillPattern10", "Medium speckle", 10),
    pattern!("FillPattern11", "Light speckle", 11),
    pattern!("FillPattern12", "Sparse dots", 12),
    pattern!("FillPattern13", "Heavy horizontal bars", 13),
    pattern!("FillPattern14", "Heavy vertical bars", 14),
    pattern!("FillPattern15", "Heavy diagonals, falling", 15),
    pattern!("FillPattern16", "Heavy diagonals, rising", 16),
    pattern!("FillPattern17", "Small checks", 17),
    pattern!("FillPattern18", "Dense lattice", 18),
    pattern!("FillPattern19", "Thin horizontal lines", 19),
    pattern!("FillPattern20", "Thin vertical lines", 20),
    pattern!("FillPattern21", "Thin diagonals, falling", 21),
    pattern!("FillPattern22", "Thin diagonals, rising", 22),
    pattern!("FillPattern23", "Fine grid", 23),
    pattern!("FillPattern24", "Fine lattice", 24),
];
const FILL_ITEMS: &[Ctl] = &[
    c("FillGradientMenu", i(Base::Bucket)).g("Gradient").menu(GRADIENT_ITEMS),
    c("FillPatternMenu", i(Base::Bucket)).g("Pattern").menu(PATTERN_ITEMS),
    c("FillTransparencyMenu", i(Base::Bucket)).g("Transparency").menu(FILL_TRANSPARENCY_ITEMS),
    c("FillDialog", i(Base::Bucket)).g("Fill Options...").cmd("format.options"),
];
/// A shadow preset of the Effects menu: `format.shadow` with the preset's word (`shape.shadow`). The gallery's contents are `named` only, so the words are ours.
macro_rules! shadow {
    ($id:literal, $label:literal, $word:literal) => {
        c($id, i(Base::Shadow)).g($label).cmd_with("format.shadow", concat!("{\"preset\":\"", $word, "\"}"))
    };
}
const SHADOW_ITEMS: &[Ctl] = &[
    shadow!("ShadowNone", "No Shadow", "none"),
    shadow!("ShadowBottomRight", "Offset Bottom Right", "bottom_right"),
    shadow!("ShadowBottom", "Offset Bottom", "bottom"),
    shadow!("ShadowBottomLeft", "Offset Bottom Left", "bottom_left"),
    shadow!("ShadowRight", "Offset Right", "right"),
    shadow!("ShadowCenter", "Centered", "center"),
    shadow!("ShadowLeft", "Offset Left", "left"),
    shadow!("ShadowTopRight", "Offset Top Right", "top_right"),
    shadow!("ShadowTop", "Offset Top", "top"),
    shadow!("ShadowTopLeft", "Offset Top Left", "top_left"),
    c("ShadowOptionsDialog", i(Base::Shadow)).g("Shadow Options...").cmd_with("format.options", r#"{"tab":"Effects"}"#),
];
/// A weight item: `format.shape` with the weight in points. **Ids of items under a map id are ours**, as are their captions (the map lists the
/// galleries and not what is in them).
macro_rules! weight {
    ($id:literal, $label:literal, $points:literal) => {
        c($id, i(Base::PenLine)).g($label).cmd_with("format.shape", concat!("{\"weight\":", $points, "}")).sample(Sample::Weight($points))
    };
}

/// A dash item: `format.shape` with the line pattern number (lists the 22 patterns after the solid line).
macro_rules! dash {
    ($id:literal, $label:literal, $n:literal) => {
        c($id, i(Base::PenLine)).g($label).cmd_with("format.shape", concat!("{\"pattern\":", $n, "}")).sample(Sample::Dash($n))
    };
}

/// A line end item, for the begin or the end of a line: the number, with the caption and the picture `line_ends.rs` holds.
macro_rules! line_end {
    ($n:literal, begin) => {
        c(concat!("BeginArrow", $n), i(Base::PenLine))
            .g(NAMES[$n])
            .cmd_with("format.shape", concat!("{\"begin_arrow\":", $n, "}"))
            .sample(Sample::Arrow { number: $n, begin: true })
    };
    ($n:literal, end) => {
        c(concat!("EndArrow", $n), i(Base::PenLine))
            .g(NAMES[$n])
            .cmd_with("format.shape", concat!("{\"end_arrow\":", $n, "}"))
            .sample(Sample::Arrow { number: $n, begin: false })
    };
}

const WEIGHT_ITEMS: &[Ctl] = &[
    weight!("LineWeight025", "1/4 pt", 0.25),
    weight!("LineWeight050", "1/2 pt", 0.5),
    weight!("LineWeight075", "3/4 pt", 0.75),
    weight!("LineWeight100", "1 pt", 1.0),
    weight!("LineWeight150", "1 1/2 pt", 1.5),
    weight!("LineWeight225", "2 1/4 pt", 2.25),
    weight!("LineWeight300", "3 pt", 3.0),
    weight!("LineWeight450", "4 1/2 pt", 4.5),
    weight!("LineWeight600", "6 pt", 6.0),
];
pub(crate) const DASH_ITEMS: &[Ctl] = &[
    dash!("LinePattern1", "Solid", 1),
    dash!("LinePattern2", "Dash", 2),
    dash!("LinePattern3", "Dot", 3),
    dash!("LinePattern4", "Dash, dot", 4),
    dash!("LinePattern5", "Dash, dot, dot", 5),
    dash!("LinePattern6", "Dash, dash, dot", 6),
    dash!("LinePattern7", "Long dash, dash", 7),
    dash!("LinePattern8", "Long dash, dash, dash", 8),
    dash!("LinePattern9", "Close dash", 9),
    dash!("LinePattern10", "Close dot", 10),
    dash!("LinePattern11", "Close dash, dot", 11),
    dash!("LinePattern12", "Close dash, dot, dot", 12),
    dash!("LinePattern13", "Close dash, dash, dot", 13),
    dash!("LinePattern14", "Close long dash, dash", 14),
    dash!("LinePattern15", "Close long dash, dash, dash", 15),
    dash!("LinePattern16", "Wide dash", 16),
    dash!("LinePattern17", "Wide dot", 17),
    dash!("LinePattern18", "Wide dash, dot", 18),
    dash!("LinePattern19", "Wide dash, dot, dot", 19),
    dash!("LinePattern20", "Wide dash, dash, dot", 20),
    dash!("LinePattern21", "Wide long dash, dash", 21),
    dash!("LinePattern22", "Wide long dash, dash, dash", 22),
    dash!("LinePattern23", "Very close dash", 23),
];
/// All 46 line ends `format.shape` takes (0 to 45).
pub(crate) const BEGIN_ARROW_ITEMS: &[Ctl] = &[
    line_end!(0, begin),
    line_end!(1, begin),
    line_end!(2, begin),
    line_end!(3, begin),
    line_end!(4, begin),
    line_end!(5, begin),
    line_end!(6, begin),
    line_end!(7, begin),
    line_end!(8, begin),
    line_end!(9, begin),
    line_end!(10, begin),
    line_end!(11, begin),
    line_end!(12, begin),
    line_end!(13, begin),
    line_end!(14, begin),
    line_end!(15, begin),
    line_end!(16, begin),
    line_end!(17, begin),
    line_end!(18, begin),
    line_end!(19, begin),
    line_end!(20, begin),
    line_end!(21, begin),
    line_end!(22, begin),
    line_end!(23, begin),
    line_end!(24, begin),
    line_end!(25, begin),
    line_end!(26, begin),
    line_end!(27, begin),
    line_end!(28, begin),
    line_end!(29, begin),
    line_end!(30, begin),
    line_end!(31, begin),
    line_end!(32, begin),
    line_end!(33, begin),
    line_end!(34, begin),
    line_end!(35, begin),
    line_end!(36, begin),
    line_end!(37, begin),
    line_end!(38, begin),
    line_end!(39, begin),
    line_end!(40, begin),
    line_end!(41, begin),
    line_end!(42, begin),
    line_end!(43, begin),
    line_end!(44, begin),
    line_end!(45, begin),
];
/// All 46 line ends `format.shape` takes (0 to 45).
pub(crate) const END_ARROW_ITEMS: &[Ctl] = &[
    line_end!(0, end),
    line_end!(1, end),
    line_end!(2, end),
    line_end!(3, end),
    line_end!(4, end),
    line_end!(5, end),
    line_end!(6, end),
    line_end!(7, end),
    line_end!(8, end),
    line_end!(9, end),
    line_end!(10, end),
    line_end!(11, end),
    line_end!(12, end),
    line_end!(13, end),
    line_end!(14, end),
    line_end!(15, end),
    line_end!(16, end),
    line_end!(17, end),
    line_end!(18, end),
    line_end!(19, end),
    line_end!(20, end),
    line_end!(21, end),
    line_end!(22, end),
    line_end!(23, end),
    line_end!(24, end),
    line_end!(25, end),
    line_end!(26, end),
    line_end!(27, end),
    line_end!(28, end),
    line_end!(29, end),
    line_end!(30, end),
    line_end!(31, end),
    line_end!(32, end),
    line_end!(33, end),
    line_end!(34, end),
    line_end!(35, end),
    line_end!(36, end),
    line_end!(37, end),
    line_end!(38, end),
    line_end!(39, end),
    line_end!(40, end),
    line_end!(41, end),
    line_end!(42, end),
    line_end!(43, end),
    line_end!(44, end),
    line_end!(45, end),
];
const ARROW_ITEMS: &[Ctl] = &[
    c("BeginArrowMenu", i(Base::PenLine).with(Badge::Arrow)).g("Begin Arrow").menu(BEGIN_ARROW_ITEMS),
    c("EndArrowMenu", i(Base::PenLine).with(Badge::Arrow)).g("End Arrow").menu(END_ARROW_ITEMS),
    // The Arrows gallery ends in "more arrows" (`ArrowsMore`): the Format Shape pane's Line section holds every end, with their sizes' neighbours.
    c("ArrowsMore", i(Base::PenLine).with(Badge::Arrow)).g("More Arrows...").cmd("format.options"),
];
const LINE_ITEMS: &[Ctl] = &[
    c("LineWeightGallery", i(Base::PenLine)).g("Weight").menu(WEIGHT_ITEMS),
    c("LineTransparencyMenu", i(Base::PenLine)).g("Transparency").menu(LINE_TRANSPARENCY_ITEMS),
    c("LinePatternGallery", i(Base::PenLine)).g("Dashes").menu(DASH_ITEMS),
    c("LineArrowsGallery", i(Base::PenLine).with(Badge::Arrow)).g("Arrows").menu(ARROW_ITEMS),
    c("LineDialog", i(Base::PenLine)).g("Line Options...").cmd("format.options"),
];
const EFFECT_ITEMS: &[Ctl] = &[
    c("ObjectEffectShadowGallery", i(Base::Shadow)).g("Shadow").menu(SHADOW_ITEMS),
    c("ReflectionGallery", i(Base::Shadow)).g("Reflection"),
    c("ObjectEffectGlowGallery", i(Base::Shadow)).g("Glow"),
    c("ObjectEffectSoftEdgesGallery", i(Base::Shadow)).g("Soft Edges"),
    c("BevelShapeGallery", i(Base::Shadow)).g("Bevel"),
    c("_3DRotationGallery", i(Base::Shadow)).g("3-D Rotation"),
];
/// The items of Rotate Shapes. **Their ids and captions are ours**: the map names the gallery and says nothing of what is in it. **Reverse Ends** is the last: the
/// reference application keeps the command off its ribbon, and older versions of it kept it in this menu, which the project follows (
/// "Reverse Ends"). It acts on the connectors and lines selected and is greyed for any other shape.
const ROTATE_ITEMS: &[Ctl] = &[
    c("ShapesRotateRight", i(Base::TurnRight)).g("Rotate Right 90\u{b0}").cmd_with("arrange.rotate", r#"{"by":-90}"#),
    c("ShapesRotateLeft", i(Base::TurnLeft)).g("Rotate Left 90\u{b0}").cmd_with("arrange.rotate", r#"{"by":90}"#),
    c("ShapesFlipVertical", i(Base::FlipV)).g("Flip Vertical").cmd_with("arrange.flip", r#"{"axis":"vertical"}"#),
    c("ShapesFlipHorizontal", i(Base::FlipH)).g("Flip Horizontal").cmd_with("arrange.flip", r#"{"axis":"horizontal"}"#),
    c("ShapesReverseEnds", i(Base::Swap)).g("Reverse Ends").cmd("arrange.reverseEnds"),
];
/// The items of Position: the space commands first, as the menu's pages list them, then the page breaks, then the rotations. The ids and captions of
/// the first four are ours, as the map lists none for them.
const POSITION_ITEMS: &[Ctl] = &[
    c("AutoSpace", i(Base::Align)).g("Auto Space").cmd("window.autoSpace"),
    c("SpacingOptionsDialog", i(Base::Gear)).g("Spacing Options...").cmd("window.spacingOptions"),
    c("ShapesMoveOffPageBreaks", i(Base::Page)).g("Move off Page Breaks").cmd("arrange.moveOffPageBreaks"),
    c("AvoidPageBreaks", i(Base::Page)).g("Avoid Page Breaks").check().cmd("arrange.avoidPageBreaks"),
    c("ShapesRotateGallery", i(Base::Resize)).l("Rotate Shapes").menu(ROTATE_ITEMS),
    c("RotateDiagram", i(Base::Resize)).l("Rotate Diagram"),
];
/// The items of Align, six moves to the shape selected first. Ids and captions ours.
const ALIGN_ITEMS: &[Ctl] = &[
    c("AlignShapesLeft", i(Base::Align)).g("Align Left").cmd_with("arrange.align", r#"{"how":"left"}"#),
    c("AlignShapesCenter", i(Base::AlignCenter)).g("Align Center").cmd_with("arrange.align", r#"{"how":"center"}"#),
    c("AlignShapesRight", i(Base::AlignRight)).g("Align Right").cmd_with("arrange.align", r#"{"how":"right"}"#),
    c("AlignShapesTop", i(Base::AlignTop)).g("Align Top").cmd_with("arrange.align", r#"{"how":"top"}"#),
    c("AlignShapesMiddle", i(Base::AlignMiddle)).g("Align Middle").cmd_with("arrange.align", r#"{"how":"middle"}"#),
    c("AlignShapesBottom", i(Base::AlignBottom)).g("Align Bottom").cmd_with("arrange.align", r#"{"how":"bottom"}"#),
];
const FRONT_ITEMS: &[Ctl] = &[
    c("ObjectBringToFront", i(Base::Front)).l("Bring to Front").cmd_with("arrange.order", r#"{"to":"front"}"#),
    c("ObjectBringForward", i(Base::Front)).g("Bring Forward").cmd_with("arrange.order", r#"{"to":"forward"}"#),
];
const BACK_ITEMS: &[Ctl] = &[
    c("ObjectSendToBack", i(Base::Back)).l("Send to Back").cmd_with("arrange.order", r#"{"to":"back"}"#),
    c("ObjectSendBackward", i(Base::Back)).g("Send Backward").cmd_with("arrange.order", r#"{"to":"backward"}"#),
];
const GROUP_ITEMS: &[Ctl] = &[
    c("ObjectsGroup", i(Base::Group)).l("Group").cmd("arrange.group"),
    c("ObjectsUngroup", i(Base::Group).with(Badge::Cross)).l("Ungroup").cmd("arrange.ungroup"),
    c("ObjectsAddToGroup", i(Base::Group).with(PLUS)).l("Add to Group"),
    c("ObjectsRemoveFromGroup", i(Base::Group).with(Badge::Minus)).l("Remove from Group"),
    c("ObjectsConvertToGroup", i(Base::Group)).g("Convert to Group"),
];
const FIND_ITEMS: &[Ctl] = &[
    c("FindDialog", i(Base::Magnifier)).g("Find").cmd_with("window.find", r#"{"mode":"find"}"#),
    c("ReplaceDialog", i(Base::Magnifier)).l("Replace").cmd_with("window.find", r#"{"mode":"replace"}"#),
];
const LAYER_ITEMS: &[Ctl] = &[
    c("LayerDialog", i(Base::Layers)).l("Assign to Layer").cmd("window.assignLayer"),
    c("LayerPropertiesDialog", i(Base::Layers)).l("Layer Properties").cmd("window.layerProperties"),
];
const SELECT_ITEMS: &[Ctl] = &[
    c("SelectAll", i(Base::Select)).l("Select All").cmd("edit.selectAll"),
    c("SelectByTypeDialog", i(Base::Select)).l("Select by Type").cmd("window.selectByType"),
    c("AreaSelect", i(Base::Select)).l("Area Select").cmd("tool.area_select"),
    c("LassoSelect", i(Base::Select)).l("Lasso Select").cmd("tool.lasso_select"),
];

/// The rows of the Container submenu of the right-click menu: Add to Underlying Container is the one the specifications name.
const CONTAINER_MENU_ITEMS: &[Ctl] = &[c("ContainerAddToUnderlying", i(Base::Container).with(PLUS))
    .l("Add to Underlying Container")
    .cmd_with("window.containerDo", r#"{"op":"add_member"}"#)];

/// The rows of the Swimlane button's menu on the Cross-Functional Flowchart tab (Insert a swimlane: "a new lane can be added before or after an existing one").
/// **Their ids are ours.**
const SWIMLANE_INSERT_ITEMS: &[Ctl] = &[
    c("SwimlaneInsertBefore", i(Base::Table).with(PLUS))
        .g("Insert Swimlane Before")
        .cmd_with("window.swimlaneDo", r#"{"op":"insert","value":"before"}"#),
    c("SwimlaneInsertAfter", i(Base::Table).with(PLUS))
        .g("Insert Swimlane After")
        .cmd_with("window.swimlaneDo", r#"{"op":"insert","value":"after"}"#),
];

/// The rows of the Swimlane submenu of the right-click menu of a lane or a phase. **Their ids are ours.**
const SWIMLANE_MENU_ITEMS: &[Ctl] = &[
    c("CanvasSwimlaneBefore", i(Base::Table).with(PLUS))
        .g("Insert Swimlane Before")
        .cmd_with("window.swimlaneDo", r#"{"op":"insert","value":"before"}"#),
    c("CanvasSwimlaneAfter", i(Base::Table).with(PLUS))
        .g("Insert Swimlane After")
        .cmd_with("window.swimlaneDo", r#"{"op":"insert","value":"after"}"#),
    c("CanvasSwimlaneSeparator", i(Base::Line).with(PLUS)).g("Insert Separator").cmd_with("window.swimlaneDo", r#"{"op":"separator"}"#),
    c("CanvasSwimlaneRotate", i(Base::TextTurn)).l("Rotate Lane Label").cmd_with("window.swimlaneDo", r#"{"op":"rotate_label"}"#),
];

/// The rows of the Orientation menu of the Cross-Functional Flowchart tab: the lanes lie one under the next, or side by side. The captions are ours (the map names the tab's Insert and Design groups
/// and not what the page of the tab does for the orientation).
const SWIMLANE_ORIENTATION_ITEMS: &[Ctl] = &[
    c("SwimlaneHorizontal", i(Base::Swap))
        .g("Horizontal Lanes")
        .cmd_with("window.swimlaneDo", r#"{"op":"orientation","value":"horizontal"}"#)
        .mark(Mark::SwimlaneOrientation("horizontal")),
    c("SwimlaneVertical", i(Base::Swap))
        .g("Vertical Lanes")
        .cmd_with("window.swimlaneDo", r#"{"op":"orientation","value":"vertical"}"#)
        .mark(Mark::SwimlaneOrientation("vertical")),
];

/// The rows of the menu of Home, Tools, Connection Point. **Their ids are ours**, and so are the words: a DiaCraft extra beyond the reference app. They act
/// on every edge of the shapes selected, and are greyed when that would change nothing (`edit/conn_edges.rs`). The control has no command of its own, since the Connection Point tool is not built;
/// when it is, the control becomes a split button, the tool on its face and these rows on its arrow.
const CONNECTION_POINT_ITEMS: &[Ctl] = &[
    c("ConnectionPointsDouble", i(Base::Dot).with(PLUS))
        .g("Double Connection Points")
        .cmd_with("window.connectionPointsDo", r#"{"action":"double","target":"all"}"#),
    c("ConnectionPointsHalve", i(Base::Dot).with(Badge::Minus))
        .g("Halve Connection Points")
        .cmd_with("window.connectionPointsDo", r#"{"action":"halve","target":"all"}"#),
];

/// The rows of the Connection Points submenu of the shape's right-click menu. **Their ids and words are ours.** They act on
/// the edge at the click that opened the menu, and on every edge of the shapes selected when the click is on none, and are greyed when that would change nothing.
const CANVAS_CONNECTION_POINT_ITEMS: &[Ctl] = &[
    c("CanvasConnectionPointsDouble", i(Base::Dot).with(PLUS))
        .g("Double Connection Points")
        .cmd_with("window.connectionPointsDo", r#"{"action":"double","target":"menu"}"#),
    c("CanvasConnectionPointsHalve", i(Base::Dot).with(Badge::Minus))
        .g("Halve Connection Points")
        .cmd_with("window.connectionPointsDo", r#"{"action":"halve","target":"menu"}"#),
];

/// The rows of the menu a right click on the drawing area opens. **Their ids are ours**; the words are the map's (and its note that
/// "Cut" and "Copy" are the words of a right-click menu), except Duplicate, which is ours, **Connection Points**, after Container and before Format Shape, and **Edit Alt Text...**, whose place after Format Shape is ours too: the shape's
/// menu of has no such row, and the specification's route to the section is Format Shape, then the Alt Text section.
pub const CANVAS_MENU: &[Ctl] = &[
    c("CanvasCut", i(Base::Scissors)).l("Cut").cmd("clipboard.cut"),
    c("CanvasCopy", i(Base::Pages)).l("Copy").cmd("clipboard.copy"),
    c("CanvasPaste", i(Base::Clipboard)).l("Paste").cmd("clipboard.paste"),
    c("CanvasDuplicate", i(Base::Pages)).g("Duplicate").cmd("clipboard.duplicate"),
    c("CanvasGroup", i(Base::Group)).l("Group").cmd("arrange.group"),
    c("CanvasUngroup", i(Base::Group).with(Badge::Cross)).l("Ungroup").cmd("arrange.ungroup"),
    c("CanvasContainer", i(Base::Container)).l("Container").cmd("window.containerDo").menu(CONTAINER_MENU_ITEMS),
    c("CanvasConnectionPoints", i(Base::Dot)).g("Connection Points").menu(CANVAS_CONNECTION_POINT_ITEMS),
    c("CanvasFormatShape", i(Base::Swatches)).g("Format Shape").cmd_with("window.pane", r#"{"pane":"format_shape","shown":true}"#),
    c("CanvasAltText", i(Base::Eye)).g("Edit Alt Text...").cmd("window.altText"),
    c("CanvasData", i(Base::Table)).l("Data").menu(CANVAS_DATA_ITEMS),
];

/// The menu a right click on a lane or a phase opens: the rows of [`CANVAS_MENU`] and the Swimlane submenu.
pub const LANE_MENU: &[Ctl] = &[
    c("CanvasCut", i(Base::Scissors)).l("Cut").cmd("clipboard.cut"),
    c("CanvasCopy", i(Base::Pages)).l("Copy").cmd("clipboard.copy"),
    c("CanvasPaste", i(Base::Clipboard)).l("Paste").cmd("clipboard.paste"),
    c("CanvasDuplicate", i(Base::Pages)).g("Duplicate").cmd("clipboard.duplicate"),
    c("CanvasGroup", i(Base::Group)).l("Group").cmd("arrange.group"),
    c("CanvasUngroup", i(Base::Group).with(Badge::Cross)).l("Ungroup").cmd("arrange.ungroup"),
    c("CanvasSwimlane", i(Base::Table)).g("Swimlane").cmd("window.swimlaneDo").menu(SWIMLANE_MENU_ITEMS),
    c("CanvasContainer", i(Base::Container)).l("Container").cmd("window.containerDo").menu(CONTAINER_MENU_ITEMS),
    c("CanvasFormatShape", i(Base::Swatches)).g("Format Shape").cmd_with("window.pane", r#"{"pane":"format_shape","shown":true}"#),
    c("CanvasAltText", i(Base::Eye)).g("Edit Alt Text...").cmd("window.altText"),
    c("CanvasData", i(Base::Table)).l("Data").menu(CANVAS_DATA_ITEMS),
];

/// The rows of the Data submenu of a shape's right-click menu (row 9: Shape Data... opens the Shape Data pane, Define Shape Data... the dialog; the rows for the data graphics
/// are not built). **Their ids are ours.**
const CANVAS_DATA_ITEMS: &[Ctl] = &[
    c("CanvasShapeData", i(Base::Window)).g("Shape Data...").cmd_with("window.pane", r#"{"pane":"shape_data","shown":true}"#),
    c("CanvasDefineShapeData", i(Base::Table)).g("Define Shape Data...").cmd("window.defineShapeData"),
];

/// The rows of the Reroute menu of a connector's right-click menu: when it may take a new path, the one the connector has marked. **Their ids are ours**; the
/// words are the Connector tab's Reroute list (Freely, As Needed, Never, On Crossover), and the order is the order of the codes 0, 1, 2 and 3.
const REROUTE_ITEMS: &[Ctl] = &[
    c("ConnectorRerouteFreely", i(Base::Connector)).g("Reroute Freely").check().cmd_with("connector.reroute", r#"{"mode":"freely"}"#),
    c("ConnectorRerouteAsNeeded", i(Base::Connector)).g("Reroute As Needed").check().cmd_with("connector.reroute", r#"{"mode":"as_needed"}"#),
    c("ConnectorRerouteNever", i(Base::Connector)).g("Never Reroute").check().cmd_with("connector.reroute", r#"{"mode":"never"}"#),
    c("ConnectorRerouteOnCrossover", i(Base::Connector))
        .g("Reroute on Crossover")
        .check()
        .cmd_with("connector.reroute", r#"{"mode":"on_crossover"}"#),
];

/// The rows of the menu a right click on a connector opens (Cut, Copy, Paste, Delete, Bring to Front and Send to Back are the rows the one-dimensional
/// shape's menu pictures; Edit Text is not here, for want of a command that opens the box), and the rows of a connector's own: **Right-Angle Connector**, **Straight Connector** and
/// **Curved Connector** (each sends `connector.style` with the connectors selected, which routes them again; their place above Reset Connector and their words are ours,
/// the design call), **Reset Connector**, which "is on a connector's right-click menu", and the Reroute choices, which the
/// specifications give no menu of their own (the report's Q2: they are put here, where Reset Connector is). **Their ids are ours.**
pub const CONNECTOR_MENU: &[Ctl] = &[
    c("ConnectorCut", i(Base::Scissors)).l("Cut").cmd("clipboard.cut"),
    c("ConnectorCopy", i(Base::Pages)).l("Copy").cmd("clipboard.copy"),
    c("ConnectorPaste", i(Base::Clipboard)).l("Paste").cmd("clipboard.paste"),
    c("ConnectorDelete", i(Base::Page).with(Badge::Cross)).l("Delete").cmd("shape.delete"),
    c("ConnectorBringToFront", i(Base::Front)).l("Bring to Front").menu(FRONT_ITEMS),
    c("ConnectorSendToBack", i(Base::Back)).l("Send to Back").menu(BACK_ITEMS),
    c("ConnectorRightAngleConnector", i(Base::Connector)).g("Right-Angle Connector").cmd_with("connector.style", r#"{"style":"right_angle"}"#),
    c("ConnectorStraightConnector", i(Base::Line)).g("Straight Connector").cmd_with("connector.style", r#"{"style":"straight"}"#),
    c("ConnectorCurvedConnector", i(Base::Arc)).g("Curved Connector").cmd_with("connector.style", r#"{"style":"curved"}"#),
    c("ConnectorReverseEnds", i(Base::Swap)).g("Reverse Ends").cmd("arrange.reverseEnds"),
    c("ConnectorResetConnector", i(Base::Refresh)).l("Reset Connector").cmd("connector.reset"),
    c("ConnectorReroute", i(Base::Connector)).g("Reroute").menu(REROUTE_ITEMS),
];

/// The rows of the menu at the right of the Connector button of Home, Tools: the three routing styles for the connectors selected, as the connector's right-click menu
/// has them (`connector.style`), and **Reverse Ends**. **Their ids and captions are ours**: the map gives the button no menu, and the
/// button itself still picks the Connector tool.
const CONNECTOR_TOOL_ITEMS: &[Ctl] = &[
    c("ConnectorToolRightAngle", i(Base::Connector)).g("Right-Angle Connector").cmd_with("connector.style", r#"{"style":"right_angle"}"#),
    c("ConnectorToolStraight", i(Base::Line)).g("Straight Connector").cmd_with("connector.style", r#"{"style":"straight"}"#),
    c("ConnectorToolCurved", i(Base::Arc)).g("Curved Connector").cmd_with("connector.style", r#"{"style":"curved"}"#),
    c("ConnectorToolReverseEnds", i(Base::Swap)).g("Reverse Ends").cmd("arrange.reverseEnds"),
];

static HOME: Tab = Tab {
    id: "TabHome",
    label: "Home",
    groups: &[
        g(
            "GroupClipboard",
            "Clipboard",
            true,
            i(Base::Clipboard),
            &[
                Slot::Big(c("PasteMenu", i(Base::Clipboard)).l("Paste").split(PASTE_ITEMS).cmd("clipboard.paste")),
                Slot::Stack(&[
                    c("Cut", i(Base::Scissors)).l("Cut").cmd("clipboard.cut"),
                    c("Copy", i(Base::Pages)).l("Copy").cmd("clipboard.copy"),
                    c("FormatPainter", i(Base::Brush)).l("Format Painter").cmd("format.painter"),
                ]),
            ],
        ),
        g(
            "GroupFont",
            "Font",
            true,
            i(Base::TextA),
            &[Slot::Rows(&[
                &[
                    c("Font", i(Base::TextA)).g("Font").combo().cmd("format.text"),
                    c("FontSize", i(Base::TextA)).g("Size").combo().cmd("format.text"),
                    c("FontSizeIncrease", i(Base::FontUp)).g("Increase Font Size").cmd_with("format.size", r#"{"step":"up"}"#),
                    c("FontSizeDecrease", i(Base::FontDown)).g("Decrease Font Size").cmd_with("format.size", r#"{"step":"down"}"#),
                ],
                &[
                    c("Bold", i(Base::Bold)).g("Bold").cmd_with("format.toggle", r#"{"what":"bold"}"#).press(Press::Bold),
                    c("Italic", i(Base::Italic)).g("Italic").cmd_with("format.toggle", r#"{"what":"italic"}"#).press(Press::Italic),
                    c("Underline", i(Base::Underline)).g("Underline").cmd_with("format.toggle", r#"{"what":"underline"}"#).press(Press::Underline),
                    c("Strikethrough", i(Base::Strike)).g("Strikethrough").cmd_with("format.toggle", r#"{"what":"strike"}"#).press(Press::Strike),
                    c("ChangeCaseDialogClassic", i(Base::CaseAa)).g("Change Case").menu(CASE_ITEMS),
                    c("FontColorGallery", i(Base::FontColor)).l("Font Color").menu(NONE).cmd("format.text").palette("color", None),
                ],
            ])],
        )
        .launch(c("FontDialog", i(Base::TextA)).g("Font").cmd_with("window.textDialog", r#"{"tab":"Font"}"#)),
        g(
            "GroupParagraph",
            "Paragraph",
            true,
            i(Base::TextLeft),
            &[Slot::Rows(&[
                &[
                    c("TextAlignTop", i(Base::TextTop)).g("Align Top").cmd_with("format.text", r#"{"valign":"top"}"#).press(Press::Top),
                    c("TextAlignMiddle", i(Base::TextMiddle))
                        .g("Align Middle")
                        .cmd_with("format.text", r#"{"valign":"middle"}"#)
                        .press(Press::Middle),
                    c("TextAlignBottom", i(Base::TextBottom))
                        .g("Align Bottom")
                        .cmd_with("format.text", r#"{"valign":"bottom"}"#)
                        .press(Press::Bottom),
                    c("Bullets", i(Base::Bullets))
                        .l("Bullets")
                        .split(BULLET_ITEMS)
                        .cmd_with("format.toggle", r#"{"what":"bullets"}"#)
                        .press(Press::Bullets),
                    c("TextRotate90Degrees", i(Base::TextTurn)).g("Rotate Text").cmd_with("format.rotateText", r#"{"by":90}"#),
                    c("TextDirectionLeftToRight", i(Base::ArrowRight))
                        .g("Left-to-Right")
                        .cmd_with("format.text", r#"{"direction":"ltr"}"#)
                        .press(Press::Ltr),
                    c("TextDirectionRightToLeft", i(Base::ArrowLeft))
                        .g("Right-to-Left")
                        .cmd_with("format.text", r#"{"direction":"rtl"}"#)
                        .press(Press::Rtl),
                ],
                &[
                    c("AlignLeft", i(Base::TextLeft)).g("Align Left").cmd_with("format.text", r#"{"align":"left"}"#).press(Press::Left),
                    c("AlignCenter", i(Base::TextCenter)).g("Center").cmd_with("format.text", r#"{"align":"center"}"#).press(Press::Center),
                    c("AlignRight", i(Base::TextRight)).g("Align Right").cmd_with("format.text", r#"{"align":"right"}"#).press(Press::Right),
                    c("AlignJustify", i(Base::TextJustify)).g("Justify").cmd_with("format.text", r#"{"align":"justify"}"#).press(Press::Justify),
                    c("TextDirectionVertical", i(Base::TextTurn))
                        .g("Vertical Text")
                        .cmd_with("format.toggle", r#"{"what":"vertical"}"#)
                        .press(Press::Vertical),
                    c("IndentDecreaseWord", i(Base::IndentLess)).g("Decrease Indent").cmd_with("format.text", r#"{"indent":"decrease"}"#),
                    c("IndentIncreaseWord", i(Base::IndentMore)).g("Increase Indent").cmd_with("format.text", r#"{"indent":"increase"}"#),
                ],
            ])],
        )
        .launch(c("ParagraphDialog", i(Base::TextLeft)).g("Paragraph").cmd_with("window.textDialog", r#"{"tab":"Paragraph"}"#)),
        g(
            "GroupTools",
            "Tools",
            true,
            i(Base::Pointer),
            &[
                Slot::Stack(&[
                    c("PointerTool", i(Base::Pointer)).l("Pointer Tool").cmd("tool.pointer"),
                    c("ConnectorTool", i(Base::Connector)).l("Connector").split(CONNECTOR_TOOL_ITEMS).cmd("tool.connector"),
                    c("TextTool", i(Base::TextA)).l("Text").cmd("tool.text"),
                ]),
                Slot::Stack(&[
                    c("DrawingToolsMenu", i(Base::Rect)).l("Drawing Tools").menu(DRAWING_TOOLS),
                    c("ConnectionPointTool", i(Base::Dot)).l("Connection Point").menu(CONNECTION_POINT_ITEMS),
                    c("TextBlockTool", i(Base::TextBlock)).l("Text Block").cmd("tool.text_block"),
                ]),
            ],
        ),
        g(
            "GroupShapeStyles",
            "Shape Styles",
            true,
            i(Base::Swatches),
            &[
                Slot::Big(
                    c("ShapeStylesGallery", i(Base::Swatches)).l("Quick Styles").cmd("format.quickStyle").menu(STYLES_ITEMS).body(Body::QuickStyles),
                ),
                Slot::Stack(&[
                    c("FillColorGallery", i(Base::Bucket)).l("Fill").menu(FILL_ITEMS).cmd("format.shape").palette("fill", Some("No Fill")),
                    c("LineColorGallery", i(Base::PenLine)).l("Line").menu(LINE_ITEMS).cmd("format.shape").palette("line", Some("No Line")),
                    c("ShapeEffectsMenu", i(Base::Shadow)).l("Effects").menu(EFFECT_ITEMS),
                ]),
            ],
        )
        .launch(c("ObjectFormatDialog", i(Base::Swatches)).g("Format Shape").cmd_with("window.pane", r#"{"pane":"format_shape","shown":true}"#)),
        g(
            "GroupArrange",
            "Arrange",
            true,
            i(Base::Align),
            &[
                Slot::Big(c("AlignGallery", i(Base::Align)).l("Align").menu(ALIGN_ITEMS)),
                Slot::Big(c("PositionMenu", i(Base::Move)).l("Position").menu(POSITION_ITEMS)),
                Slot::Stack(&[
                    c("ObjectBringToFrontMenu", i(Base::Front)).g("Bring to Front").menu(FRONT_ITEMS),
                    c("ObjectSendToBackMenu", i(Base::Back)).g("Send to Back").menu(BACK_ITEMS),
                    c("ObjectsGroupMenu", i(Base::Group)).l("Group").menu(GROUP_ITEMS),
                ]),
            ],
        ),
        g(
            "GroupEditing",
            "Editing",
            true,
            i(Base::Magnifier),
            &[
                Slot::Big(c("ReplaceShape", i(Base::Swap)).l("Change Shape").cmd("arrange.changeShape").menu(NONE).body(Body::Masters)),
                Slot::Stack(&[
                    c("FindMenu", i(Base::Magnifier)).l("Find").menu(FIND_ITEMS),
                    c("LayersMenu", i(Base::Layers)).l("Layers").menu(LAYER_ITEMS),
                    c("SelectMenu", i(Base::Select)).l("Select").menu(SELECT_ITEMS),
                ]),
            ],
        ),
    ],
};

// -------------------------------------------------------------------------------------------------- Insert

const NEW_PAGE_ITEMS: &[Ctl] = &[
    c("BlankPageInsert", i(Base::Page).with(PLUS)).l("Blank Page").cmd("page.add"),
    c("BackgroundPageInsert", i(Base::Page)).g("Background Page").cmd_with("page.add", r#"{"background":true}"#),
    c("DuplicateThisPage", i(Base::Pages)).l("Duplicate This Page").cmd("page.duplicate"),
];
const PICTURE_ITEMS: &[Ctl] = &[
    c("PictureInsertFromFile", i(Base::Picture)).l("Pictures").cmd("window.insertPicture"),
    c("InsertM365Picture", i(Base::Picture)).l("Stock Images"),
    c("ClipArtInsertDialog", i(Base::Picture)).l("Online Pictures"),
];
/// The rows of the Symbol menu: the symbols a drawing's text most often wants (`insert_symbol.rs`, ours: the map names the gallery and not what is in it), each sending `window.symbol`, and More Symbols.
const SYMBOL_ITEMS: &[Ctl] = &[
    c("SymbolCopyright", i(Base::Symbol)).g("\u{a9} Copyright").cmd_with("window.symbol", r#"{"text":"\u00a9"}"#),
    c("SymbolRegistered", i(Base::Symbol)).g("\u{ae} Registered").cmd_with("window.symbol", r#"{"text":"\u00ae"}"#),
    c("SymbolTrademark", i(Base::Symbol)).g("\u{2122} Trademark").cmd_with("window.symbol", r#"{"text":"\u2122"}"#),
    c("SymbolDegree", i(Base::Symbol)).g("\u{b0} Degree").cmd_with("window.symbol", r#"{"text":"\u00b0"}"#),
    c("SymbolPlusMinus", i(Base::Symbol)).g("\u{b1} Plus-Minus").cmd_with("window.symbol", r#"{"text":"\u00b1"}"#),
    c("SymbolMultiply", i(Base::Symbol)).g("\u{d7} Multiplication").cmd_with("window.symbol", r#"{"text":"\u00d7"}"#),
    c("SymbolDivide", i(Base::Symbol)).g("\u{f7} Division").cmd_with("window.symbol", r#"{"text":"\u00f7"}"#),
    c("SymbolEuro", i(Base::Symbol)).g("\u{20ac} Euro").cmd_with("window.symbol", r#"{"text":"\u20ac"}"#),
    c("SymbolPound", i(Base::Symbol)).g("\u{a3} Pound").cmd_with("window.symbol", r#"{"text":"\u00a3"}"#),
    c("SymbolYen", i(Base::Symbol)).g("\u{a5} Yen").cmd_with("window.symbol", r#"{"text":"\u00a5"}"#),
    c("SymbolSection", i(Base::Symbol)).g("\u{a7} Section").cmd_with("window.symbol", r#"{"text":"\u00a7"}"#),
    c("SymbolBullet", i(Base::Symbol)).g("\u{2022} Bullet").cmd_with("window.symbol", r#"{"text":"\u2022"}"#),
    c("SymbolEllipsis", i(Base::Symbol)).g("\u{2026} Ellipsis").cmd_with("window.symbol", r#"{"text":"\u2026"}"#),
    c("SymbolOmega", i(Base::Symbol)).g("\u{3a9} Omega").cmd_with("window.symbol", r#"{"text":"\u03a9"}"#),
    c("SymbolAlmostEqual", i(Base::Symbol)).g("\u{2248} Almost Equal").cmd_with("window.symbol", r#"{"text":"\u2248"}"#),
    c("SymbolsDialog", i(Base::Symbol)).g("More Symbols").cmd("window.symbols"),
];
const TEXT_BOX_ITEMS: &[Ctl] = &[
    c("TextBoxInsertHorizontal", i(Base::TextBlock)).l("Horizontal Text Box").cmd("tool.text"),
    c("TextBoxInsertVertical", i(Base::TextBlock)).l("Vertical Text Box"),
];

static INSERT: Tab = Tab {
    id: "TabInsert",
    label: "Insert",
    groups: &[
        g(
            "GroupInsertPages",
            "Pages",
            true,
            i(Base::Page),
            &[Slot::Big(c("BlankPageInsertMenu", i(Base::Page).with(PLUS)).l("New Page").menu(NEW_PAGE_ITEMS))],
        ),
        g(
            "GroupInsertIllustrations",
            "Illustrations",
            true,
            i(Base::Picture),
            &[
                Slot::Big(c("FlyoutAnchorInsertPictures", i(Base::Picture)).l("Pictures").menu(PICTURE_ITEMS)),
                Slot::Big(c("IconInsertFromFile", i(Base::Star)).l("Icons")),
                Slot::Big(c("ChartInsert", i(Base::Chart)).l("Chart")),
                Slot::Big(c("CadDrawing", i(Base::Ruler)).l("CAD Drawing")),
            ],
        ),
        g(
            "GroupDiagramParts",
            "Diagram Parts",
            true,
            i(Base::Container),
            &[
                Slot::Big(c("ContainerGallery", i(Base::Container)).l("Container").cmd("window.containerAdd").menu(NONE).body(Body::ContainerStyles)),
                Slot::Big(c("CalloutGallery", i(Base::Callout)).l("Callout").cmd("window.callout").menu(NONE).body(Body::Callouts)),
                Slot::Big(c("QuickConnectorTool", i(Base::Connector)).g("Quick Connector")),
            ],
        ),
        g("GroupInsertLinks", "Links", true, i(Base::Link), &[Slot::Big(c("HyperlinkInsert", i(Base::Link)).l("Link").cmd("window.hyperlinks"))]),
        g(
            "GroupTextBoxText",
            "Text",
            true,
            i(Base::TextBlock),
            &[
                Slot::Big(c("TextBoxInsertMenu", i(Base::TextBlock)).l("Text Box").menu(TEXT_BOX_ITEMS)),
                Slot::Stack(&[
                    c("ScreenTip", i(Base::Callout)).l("ScreenTip"),
                    c("OleObjectctInsert", i(Base::Window)).g("Object"),
                    c("FieldInsert", i(Base::Mark)).l("Field").cmd("window.field"),
                ]),
                Slot::Big(c("SymbolInsertGallery", i(Base::Symbol)).g("Symbol").menu(SYMBOL_ITEMS)),
            ],
        ),
    ],
};

// ---------------------------------------------------------------------------------------------------- Draw

static DRAW: Tab = Tab {
    id: "TabDrawInk",
    label: "Draw",
    groups: &[
        g(
            "GroupWrite",
            "Write",
            false,
            i(Base::Pencil),
            &[
                Slot::Big(c("DrawSelectTool", i(Base::Select)).g("Select")),
                Slot::Stack(&[
                    c("FingerPaintingMode", i(Base::Pointer)).g("Touch Drawing"),
                    c("InkEraser", i(Base::Pencil).with(Badge::Cross)).g("Eraser"),
                    c("LassoSelect", i(Base::Select)).l("Lasso Select").cmd("tool.lasso_select"),
                ]),
            ],
        ),
        g(
            "GroupPens",
            "Pens",
            false,
            i(Base::PenLine),
            &[
                Slot::Big(c("PensGallery", i(Base::Pencil)).g("Pens").menu(&[c("PenAddFavorite", i(Base::Star)).g("Add Favorite Pen")])),
                Slot::Stack(&[
                    c("InkColorPicker", i(Base::Palette)).g("Color").menu(NONE),
                    c("LineThickness", i(Base::PenLine)).g("Thickness").menu(&[c("PenPressure", i(Base::PenLine)).g("Pressure")]),
                ]),
            ],
        ),
        g(
            "GroupDrawCreate",
            "Create",
            false,
            i(Base::Rect),
            &[Slot::Stack(&[
                c("InkShapeClose", i(Base::Freeform)).g("Close Shape").cmd("draw.closeShape"),
                c("InkAutoCreateShapes", i(Base::Rect)).g("Auto-Create Shapes"),
                c("DrawConvertInkToGeometry", i(Base::Freeform).with(Badge::Arrow)).g("Ink to Shape"),
            ])],
        ),
        g(
            "GroupDrawConvert",
            "Convert",
            false,
            i(Base::TextA),
            &[Slot::Big(c("DrawConvertInkToText", i(Base::TextA).with(Badge::Arrow)).g("Ink to Text"))],
        ),
        // **A DiaCraft extra beyond the map** (owner, 2026-11-25): the shape operations are hard to find under Developer, Shape Design, so the Draw tab has a
        // menu of the same items ([`DRAW_OPERATIONS`]: the Developer menu's and Fit Curve), in a group of its own after Convert: it is the last group, so the row gives it up first when the
        // window is narrow. The control has an id of its own (`DrawShapeOperations`); Developer keeps `OperationsMenu`, as the map has it.
        g(
            "GroupDrawShapeOperations",
            "Shape Operations",
            false,
            i(Base::Rect),
            &[Slot::Big(c("DrawShapeOperations", i(Base::Rect)).g("Operations").menu(DRAW_OPERATIONS))],
        ),
    ],
};

// -------------------------------------------------------------------------------------------------- Design

/// The items of Orientation. **Their ids and captions are ours**: the map names the control and says nothing of what is in it. The row of the way the page
/// is held is marked (`page.setup`'s `orientation`).
const ORIENTATION_ITEMS: &[Ctl] = &[
    c("PageOrientationPortrait", i(Base::PagePortrait))
        .g("Portrait")
        .cmd_with("page.orientation", r#"{"orientation":"portrait"}"#)
        .mark(Mark::Orientation("portrait")),
    c("PageOrientationLandscape", i(Base::PageLandscape))
        .g("Landscape")
        .cmd_with("page.orientation", r#"{"orientation":"landscape"}"#)
        .mark(Mark::Orientation("landscape")),
];
/// The rows of Size under its list of named sizes. Fit to Drawing has no engine command yet (P1); More Page Sizes opens Page Setup on its Page Size tab.
const FIT_ITEMS: &[Ctl] = &[
    c("PageFitToDrawing", i(Base::Fit)).l("Fit to Drawing"),
    c("MorePageSizes", i(Base::Page)).g("More Page Sizes").cmd_with("window.pageSetup", r#"{"tab":"Page Size"}"#),
];
/// The rows under the Themes gallery's tiles. No Theme is ours (names it and the map has no id for it): it sends `theme.apply` with `none`.
/// Apply Theme to New Shapes has no engine setting behind it yet: new shapes always take the page's theme.
const THEME_TILES: &[Ctl] = &[
    c("ThemeNone", i(Base::Square)).g("No Theme").cmd_with("theme.apply", r#"{"theme":"none"}"#).mark(Mark::NoTheme),
    c("ThemeApplyToNewShapes", i(Base::Palette)).l("Apply Theme to New Shapes"),
];
/// The row under the Colors menu's four colour variants. Custom colour sets have no engine command.
const COLOUR_ITEMS: &[Ctl] = &[c("ThemeColorsCreateNew", i(Base::Plus)).g("Create New Theme Colors...")];
const VARIANT_ITEMS: &[Ctl] = &[
    c("ThemeColorsGallery", i(Base::Palette)).l("Colors").cmd("theme.variant").menu(COLOUR_ITEMS).body(Body::VariantColours),
    c("ThemeEffectsGallery", i(Base::Shadow)).g("Effects"),
    c("ThemeConnectorsGallery", i(Base::Connector)).g("Connectors"),
    c("EmbellishmentGallery", i(Base::Star)).g("Embellishments"),
];
/// The rows of Connectors. The three styles style the connectors selected, and the page's default for new connectors when none is (`connector.style`;
/// the window adds `ids` and `page`, `ribbon/design.rs`); the row of the page's default is marked. Show Line Jumps is the page's check box
/// (`connector.line_jumps`, which the window sends with the state the page is not in).
const CONNECTOR_ITEMS: &[Ctl] = &[
    c("ConnectorRightAngle", i(Base::Connector))
        .l("Right-Angle")
        .cmd_with("connector.style", r#"{"style":"right_angle"}"#)
        .mark(Mark::ConnectorStyle("right_angle")),
    c("ConnectorStraightLines", i(Base::Line))
        .l("Straight Lines")
        .cmd_with("connector.style", r#"{"style":"straight"}"#)
        .mark(Mark::ConnectorStyle("straight")),
    c("ConnectorCurvedLines", i(Base::Arc))
        .l("Curved Lines")
        .cmd_with("connector.style", r#"{"style":"curved"}"#)
        .mark(Mark::ConnectorStyle("curved")),
    c("ConnectorShowLineJumps", i(Base::Freeform)).l("Show Line Jumps").check().cmd_with("connector.line_jumps", r#"{"show":true}"#),
];

static DESIGN: Tab = Tab {
    id: "TabDesign",
    label: "Design",
    groups: &[
        g(
            "GroupPrintPreviewPageSetup",
            "Page Setup",
            true,
            i(Base::Page),
            &[
                Slot::Big(c("PageOrientation", i(Base::PagePortrait)).l("Orientation").menu(ORIENTATION_ITEMS)),
                Slot::Big(c("PageSize", i(Base::Resize)).l("Size").menu(FIT_ITEMS).body(Body::PageSizes)),
                Slot::Big(c("AutoSizePage", i(Base::Fit)).l("Auto Size").cmd("page.auto_size").press(Press::AutoSize)),
            ],
        )
        .launch(c("PageSetupDialog", i(Base::Page)).l("Page Setup").cmd_with("window.pageSetup", r#"{"tab":"Page Size"}"#)),
        g(
            "GroupThemes",
            "Themes",
            true,
            i(Base::Palette),
            &[Slot::Gallery(c("ThemesGallery", i(Base::Palette)).g("Themes").cmd("theme.apply").body(Body::Themes).gallery(THEME_TILES))],
        ),
        g(
            "GroupThemeVariants",
            "Variants",
            true,
            i(Base::Palette),
            &[Slot::Gallery(
                c("ThemeVariantsGallery", i(Base::Swatches)).g("Variants").cmd("theme.variant").body(Body::Variants).gallery(VARIANT_ITEMS),
            )],
        ),
        g(
            "GroupBackgrounds",
            "Backgrounds",
            true,
            i(Base::Picture),
            &[
                Slot::Big(
                    c("BackgroundsGallery", i(Base::Picture)).l("Backgrounds").menu(&[c("BackgroundColor", i(Base::Bucket)).g("Background Color")]),
                ),
                Slot::Big(c("BordersAndTitlesGallery", i(Base::Table)).l("Borders & Titles").menu(NONE)),
            ],
        )
        .launch(c("PageSetupPageDialog", i(Base::Page)).g("Page Setup").cmd_with("window.pageSetup", r#"{"tab":"Page Properties"}"#)),
        g(
            "GroupLayoutRouting",
            "Layout",
            true,
            i(Base::Connector),
            &[
                Slot::Big(
                    c("PageRelayoutGallery", i(Base::Swap))
                        .l("Re-Layout Page")
                        .cmd("window.relayout")
                        .menu(&[c("LayoutMoreOptions", i(Base::Gear)).g("More Layout Options...").cmd("window.relayoutOptions")])
                        .body(Body::Layouts),
                ),
                Slot::Big(c("ConnectorsGallery", i(Base::Connector)).l("Connectors").menu(CONNECTOR_ITEMS)),
            ],
        )
        .launch(
            c("PageSetupLayoutAndRoutingDialog", i(Base::Connector))
                .g("Layout and Routing")
                .cmd_with("window.pageSetup", r#"{"tab":"Layout and Routing"}"#),
        ),
    ],
};

// ---------------------------------------------------------------------------------------------------- Data

const REFRESH_ITEMS: &[Ctl] =
    &[c("DataRefreshAllDialog", i(Base::Refresh)).g("Refresh All"), c("DataRefreshDialog", i(Base::Refresh)).g("Refresh Data")];
const ADV_GRAPHICS: &[Ctl] = &[
    c("DataGraphicCreateNew", i(Base::Chart).with(PLUS)).g("New Data Graphic"),
    c("DataGraphicEdit", i(Base::Chart)).g("Edit Data Graphic"),
    c("DataGraphicApplyAfterLinkingData", i(Base::Chart)).g("Apply After Linking Data"),
];

static DATA: Tab = Tab {
    id: "TabData",
    label: "Data",
    groups: &[
        g(
            "GroupCreateFromData",
            "Create from Data",
            true,
            i(Base::Database),
            &[Slot::Big(c("AttachDataSource", i(Base::Database).with(PLUS)).l("Create"))],
        ),
        g(
            "GroupExternalData",
            "External Data",
            true,
            i(Base::Database),
            &[
                Slot::Big(c("QuickDataLinkDataToShapes", i(Base::Table).with(Badge::Arrow)).l("Quick Import")),
                Slot::Big(c("DataLinkDataToShapes", i(Base::Table).with(Badge::Arrow)).l("Custom Import")),
                Slot::Big(c("RefreshMenu", i(Base::Refresh)).l("Refresh All").menu(REFRESH_ITEMS)),
            ],
        ),
        g(
            "GroupDataGraphics",
            "Data Graphics",
            true,
            i(Base::Chart),
            &[
                Slot::Big(
                    c("DataGraphicCalloutsGallery", i(Base::Chart))
                        .l("Data Graphics")
                        .menu(&[c("MoreDataGraphics", i(Base::Chart)).g("More Data Graphics")]),
                ),
                Slot::Big(c("DataGraphicPosition", i(Base::Move)).l("Position").menu(&[
                    c("DataGraphicPositionHorizontal", i(Base::Move)).g("Horizontal"),
                    c("DataGraphicPositionVertical", i(Base::Move)).g("Vertical"),
                ])),
                Slot::Big(c("DataGraphicsConfigure", i(Base::Gear)).l("Configure")),
                Slot::Big(c("SmartSpace", i(Base::Align)).l("Auto Space")),
            ],
        ),
        g(
            "GroupDisplayData",
            "Display Data",
            true,
            i(Base::Table),
            &[Slot::Big(c("LegendInsertGallery", i(Base::Table)).l("Insert Legend").menu(NONE))],
        ),
        g(
            "GroupDataShowHide",
            "Show/Hide",
            true,
            i(Base::Eye),
            &[Slot::Stack(&[
                c("ShapeDataWindow", i(Base::Window)).l("Shape Data Window").check().cmd_with("window.pane", r#"{"pane":"shape_data"}"#),
                c("ExternalDataWindow", i(Base::Window)).l("External Data Window").check(),
                c("DataOptionsPane", i(Base::Window)).l("Data Graphic Fields").check(),
            ])],
        ),
        g(
            "GroupAdvancedDataLinking",
            "Advanced Data Linking",
            true,
            i(Base::Link),
            &[
                Slot::Big(c("DataAutomaticallyLink", i(Base::Link)).l("Link Data")),
                Slot::Big(c("DataGraphicsGallery", i(Base::Chart)).l("Advanced Data Graphics").menu(ADV_GRAPHICS)),
            ],
        ),
    ],
};

// -------------------------------------------------------------------------------------------------- Process

const LINK_EXISTING: &[Ctl] =
    &[c("SubprocessBrowseToOtherDocument", i(Base::FolderOpen)).g("Browse to Other Document"), c("SubprocessEditLink", i(Base::Link)).g("Edit Link")];
const CHECK_ITEMS: &[Ctl] = &[
    c("DiagramCheck", i(Base::Spelling)).l("Check Diagram"),
    c("RulesToCheck", i(Base::Gear)).l("Rules to Check"),
    c("RulesImportFrom", i(Base::FolderOpen)).l("Import Rules From"),
];
const IGNORE_ITEMS: &[Ctl] = &[
    c("IgnoreThisIssue", i(Base::Eye)).l("Ignore This Issue"),
    c("IgnoreRule", i(Base::Eye)).l("Ignore Rule"),
    c("StopIgnoringThisIssue", i(Base::Eye)).g("Stop Ignoring This Issue"),
    c("StopIgnoringRule", i(Base::Eye)).g("Stop Ignoring Rule"),
    c("ShowIgnoredIssues", i(Base::Eye)).g("Show Ignored Issues"),
];

static PROCESS: Tab = Tab {
    id: "TabProcess",
    label: "Process",
    groups: &[
        g(
            "GroupSubProcess",
            "Subprocess",
            true,
            i(Base::Pages),
            &[
                Slot::Big(c("SubprocessCreateNew", i(Base::Page).with(PLUS)).l("Create New")),
                Slot::Big(c("SubprocessLinkToExisting", i(Base::Link)).l("Link to Existing").menu(LINK_EXISTING)),
                Slot::Big(c("SubprocessCreateFromSelection", i(Base::Select)).l("Create from Selection")),
            ],
        ),
        g(
            "GroupDiagramValidation",
            "Diagram Validation",
            true,
            i(Base::Spelling),
            &[
                Slot::Big(c("DiagramCheckMenu", i(Base::Spelling)).l("Check Diagram").menu(CHECK_ITEMS)),
                Slot::Big(c("IgnoreThisIssueMenu", i(Base::Eye)).g("Ignore This Issue").menu(IGNORE_ITEMS)),
                Slot::Big(c("IssuesWindow", i(Base::Window)).l("Issues Window")),
            ],
        ),
        g(
            "GroupSharepointWorkflow",
            "SharePoint Workflow",
            true,
            i(Base::Globe),
            &[
                Slot::Stack(&[
                    c("SharePointWorkflowImport", i(Base::FolderOpen)).l("Import"),
                    c("SharePointWorkflowExport", i(Base::Save)).l("Export"),
                ]),
                Slot::Stack(&[
                    c("CreateStageViewDiagram", i(Base::Page)).g("Stage View"),
                    c("CreateWorkflowDiagram", i(Base::Page)).g("Workflow Diagram"),
                ]),
            ],
        ),
        g("GroupExport", "Export", true, i(Base::Save), &[Slot::Big(c("ExportToWord", i(Base::Page).with(Badge::Arrow)).l("Word"))]),
    ],
};

// -------------------------------------------------------------------------------------------------- Review

const LANGUAGE_ITEMS: &[Ctl] =
    &[c("SetLanguage", i(Base::Globe)).g("Set Proofing Language"), c("LanguagePreferences", i(Base::Gear)).g("Language Preferences")];
const COMMENT_ITEMS: &[Ctl] = &[
    c("CommentsPaneSplitButton", i(Base::Bubble)).l("Comments Pane").check().cmd_with("window.pane", r#"{"pane":"comments"}"#),
    c("CommentingPane", i(Base::Bubble)).g("Commenting Pane"),
    c("ReviewPreviousComment", i(Base::Bubble)).g("Previous Comment").cmd_with("window.stepComment", r#"{"forward":false}"#),
    c("ReviewNextComment", i(Base::Bubble)).g("Next Comment").cmd_with("window.stepComment", r#"{"forward":true}"#),
    c("ShowTags", i(Base::Bubble)).g("Show Markup").check().cmd("comment.markers"),
];

static REVIEW: Tab = Tab {
    id: "TabReviewWord",
    label: "Review",
    groups: &[
        g(
            "GroupProofing",
            "Proofing",
            true,
            i(Base::Spelling),
            &[
                Slot::Big(c("Spelling", i(Base::Spelling)).l("Spelling")),
                Slot::Stack(&[c("ResearchPane", i(Base::Book)).g("Research"), c("Thesaurus", i(Base::Book)).g("Thesaurus")]),
            ],
        ),
        g(
            "GroupAccessibility",
            "Accessibility",
            true,
            i(Base::Eye),
            &[Slot::Big(
                c("AccessibilityChecker", i(Base::Eye).with(Badge::Check))
                    .g("Check Accessibility")
                    .cmd_with("window.pane", r#"{"pane":"accessibility","shown":true}"#),
            )],
        ),
        g(
            "GroupLanguage",
            "Language",
            true,
            i(Base::Globe),
            &[
                Slot::Big(c("Translate", i(Base::Globe)).g("Translate")),
                Slot::Big(c("LanguageCommands", i(Base::Globe)).g("Language").menu(LANGUAGE_ITEMS)),
            ],
        ),
        g(
            "GroupComments",
            "Comments",
            true,
            i(Base::Bubble),
            &[
                Slot::Big(c("ReviewNewComment", i(Base::Bubble).with(PLUS)).l("New Comment").cmd("window.newComment")),
                Slot::Big(c("ShowCommentsMenu", i(Base::Bubble)).g("Show Comments").menu(COMMENT_ITEMS)),
                Slot::Big(c("InkTool", i(Base::Pencil)).g("Ink")),
            ],
        ),
        g("GroupReports", "Reports", true, i(Base::Table), &[Slot::Big(c("ShapeReports", i(Base::Table)).l("Shape Reports"))]),
    ],
};

// ---------------------------------------------------------------------------------------------------- View

const TASK_PANES: &[Ctl] = &[
    c("ShapesWindow", i(Base::Window)).l("Shapes").check().cmd("window.shapes"),
    c("ShapeDataWindowShowHide", i(Base::Window)).g("Shape Data").check().cmd_with("window.pane", r#"{"pane":"shape_data"}"#),
    c("PanAndZoomWindow", i(Base::Window)).l("Pan & Zoom").check().cmd_with("window.pane", r#"{"pane":"pan_zoom"}"#),
    c("SizeAndPositionWindow", i(Base::Window)).l("Size & Position").check().cmd_with("window.pane", r#"{"pane":"size_position"}"#),
    c("DiagramNavigationPane", i(Base::Window)).g("Navigation").check(),
];
const SWITCH_WINDOWS: &[Ctl] = &[c("WindowMoreWindowsDialog", i(Base::Window)).g("More Windows")];

static VIEW: Tab = Tab {
    id: "TabView",
    label: "View",
    groups: &[
        // The map gives this group the one control, PowerPoint. Picture is ours (no id of the map, a caption of ours): it opens the same
        // dialog as Export in the File view and writes the page on view as a PNG, an SVG or a PDF.
        g(
            "GroupExport",
            "Export",
            true,
            i(Base::Save),
            &[
                Slot::Big(c("ExportPicture", i(Base::Picture).with(Badge::Arrow)).g("Picture").cmd("file.export")),
                Slot::Big(c("ExportToPowerPoint", i(Base::Page).with(Badge::Arrow)).g("PowerPoint")),
            ],
        ),
        // Full Screen is ours: the browser version's own full screen, which asks the browser for the keys it keeps (`page_host.rs`). The map has no such control; Presentation
        // Mode, the reference application's full-screen view (F5), is another thing and stays as it was. On the desktop the control cannot run, and says why.
        g(
            "GroupDocumentViews",
            "Views",
            true,
            i(Base::Play),
            &[
                Slot::Big(c("ViewFullScreenView", i(Base::Play)).l("Presentation Mode")),
                Slot::Stack(&[c("BrowserFullScreen", i(Base::Resize)).g("Full Screen").check().cmd("window.fullScreen")]),
            ],
        ),
        g(
            "GroupViewShowHide",
            "Show",
            true,
            i(Base::Eye),
            &[
                Slot::Stack(&[
                    c("ShowRuler", i(Base::Ruler)).l("Ruler").check().cmd("view.rulers"),
                    c("ShowGrid", i(Base::Grid)).l("Grid").check().cmd("view.grid"),
                    c("ShowPageBreaks", i(Base::Page)).l("Page Breaks").check().cmd("view.pageBreaks"),
                ]),
                Slot::Stack(&[
                    c("ShowGuides", i(Base::Line)).l("Guides").check().cmd("view.guides"),
                    c("TaskPanesMenu", i(Base::Window)).l("Task Panes").menu(TASK_PANES),
                ]),
            ],
        )
        .launch(c("RulerAndGridDialog", i(Base::Ruler)).g("Ruler & Grid")),
        g(
            "GroupZoom",
            "Zoom",
            true,
            i(Base::Magnifier),
            &[
                // Opens the Zoom dialog (`zoom_dialog.rs`).
                Slot::Big(c("ZoomDialog", i(Base::Magnifier)).l("Zoom").cmd("window.zoomDialog")),
                Slot::Big(c("ZoomFitToWindow", i(Base::Fit)).g("Fit to Window").cmd("view.fitPage")),
                Slot::Big(c("ZoomPageWidth", i(Base::Resize)).g("Page Width").cmd("view.fitWidth")),
            ],
        ),
        // Dynamic Grid is the document's `dynamic_grid` (`snap.set`; a press sends the option the other way from how the document has it, so
        // the `true` here only names the option); AutoConnect is the drawing's box (`autoconnect.set`: the arrows on a shape the pointer rests on); Connection Points is
        // the window's own (`view.connectionPoints`, read from the file's drawing window); the launcher opens the Snap & Glue dialog.
        // The second column is **DiaCraft's own addition, beside the dialog** (the desktop reference application has no Snap box on its View tab, only its
        // web application does): Snap is the dialog's Snap switch and Snap to Grid its Snap to > Grid box, each the document's own option
        // (`snap`, `grid`) read and sent the same way Dynamic Grid is. The ids are the map's names of the commands that are off its ribbon (`SnapToggle`,
        // `SnapToGrid`); the captions are ours.
        g(
            "GroupVisualAids",
            "Visual Aids",
            true,
            i(Base::Magnet),
            &[
                Slot::Stack(&[
                    c("DynamicGrid", i(Base::Grid)).l("Dynamic Grid").check().cmd_with("snap.set", r#"{"dynamic_grid":true}"#),
                    c("AutoConnect", i(Base::Connector)).l("AutoConnect").check().cmd("view.autoConnect"),
                    c("ConnectionPoints", i(Base::Dot)).l("Connection Points").check().cmd("view.connectionPoints"),
                ]),
                Slot::Stack(&[
                    c("SnapToggle", i(Base::Magnet)).g("Snap").check().cmd_with("snap.set", r#"{"snap":true}"#),
                    c("SnapToGrid", i(Base::Grid)).g("Snap to Grid").check().cmd_with("snap.set", r#"{"grid":true}"#),
                ]),
            ],
        )
        .launch(c("SnapAndGlueDialog", i(Base::Magnet)).g("Snap & Glue").cmd_with("window.snapGlue", r#"{"tab":"General"}"#)),
        g(
            "GroupWindow",
            "Window",
            true,
            i(Base::Window),
            &[
                Slot::Big(c("WindowNew", i(Base::Window).with(PLUS)).g("New Window")),
                Slot::Big(c("WindowsArrangeAll", i(Base::Window)).g("Arrange All")),
                Slot::Big(c("WindowsCascade", i(Base::Pages)).g("Cascade")),
                Slot::Big(c("WindowSwitchWindowsMenuWord", i(Base::Window)).g("Switch Windows").menu(SWITCH_WINDOWS)),
            ],
        ),
        g(
            "GroupMacros",
            "Macros",
            true,
            i(Base::Play),
            &[Slot::Big(c("MacroPlay", i(Base::Play)).g("Macros")), Slot::Big(c("AddOnsMenu", i(Base::Gear)).l("Add-Ons").menu(NONE))],
        ),
    ],
};

// ------------------------------------------------------------------------------------------------ Developer

/// The three items of Show ShapeSheet's menu: the sheet of the shape selected, of the page on view and of the document. Shape needs a shape selected (`sheet_open_verdict`).
const SHEET_ITEMS: &[Ctl] = &[
    c("ShowShapeSheetShape", i(Base::Table)).l("Shape").cmd_with("window.shapesheet", r#"{"of":"shape"}"#),
    c("ShowShapeSheetPage", i(Base::Table)).l("Page").cmd_with("window.shapesheet", r#"{"of":"page"}"#),
    c("ShowShapeSheetDocument", i(Base::Table)).l("Document").cmd_with("window.shapesheet", r#"{"of":"document"}"#),
];

/// The first row of the right-click menus of a shape and of the page while the Developer tab is on: the sheet of the shape under the
/// pointer, or the page's when the click was on no shape. **The id is ours.**
pub const CANVAS_SHOW_SHEET: Ctl = c("CanvasShowShapeSheet", i(Base::Table)).l("Show ShapeSheet").cmd_with("window.shapesheet", "{}");
/// The Operations menu: Union, Combine, Fragment, Intersect, Subtract, Join, Trim and Offset run the window's `operations.*` commands on the shapes selected
/// (Offset's row opens its dialog, `operations.offsetDialog`) and are enabled when the engine says the operation can run on them (`operations.rs`); Update Alignment Box has no
/// command, so it is drawn quiet, with the tooltip "(not available yet)". **The Developer tab's menu has the nine items the map lists** (`OperationsMenu`): Fit Curve is not among
/// them (R200 lists `FitCurve` and `FitCurveDialog` as commands that are not on the ribbon), so the tab that is ours, Draw, is where it is ([`DRAW_OPERATIONS`]).
const OPERATIONS: &[Ctl] = &[
    c("ShapesObjectUnion", i(Base::Rect)).g("Union").cmd("operations.union"),
    c("ShapesObjectCombine", i(Base::Rect)).g("Combine").cmd("operations.combine"),
    c("ShapesObjectFragment", i(Base::Rect)).g("Fragment").cmd("operations.fragment"),
    c("ShapesObjectIntersect", i(Base::Rect)).g("Intersect").cmd("operations.intersect"),
    c("ShapesObjectSubtract", i(Base::Rect)).g("Subtract").cmd("operations.subtract"),
    c("ShapesObjectJoin", i(Base::Rect)).g("Join").cmd("operations.join"),
    c("ShapesObjectTrim", i(Base::Rect)).l("Trim").cmd("operations.trim"),
    c("ShapesObjectOffset", i(Base::Rect)).g("Offset").cmd("operations.offsetDialog"),
    c("ShapesObjectUpdateAlignmentBox", i(Base::Rect)).g("Update Alignment Box"),
];

/// The Draw tab's Shape Operations menu (ours): the items of [`OPERATIONS`] and **Fit Curve** (`FitCurveDialog`, the command's id, which opens
/// the dialog, `operations.fitCurveDialog`), a DiaCraft extra beyond the reference app's ribbon, which lists it among the commands that are not on it. Both menus run the same
/// commands and are enabled alike.
const DRAW_OPERATIONS: &[Ctl] = &[
    c("ShapesObjectUnion", i(Base::Rect)).g("Union").cmd("operations.union"),
    c("ShapesObjectCombine", i(Base::Rect)).g("Combine").cmd("operations.combine"),
    c("ShapesObjectFragment", i(Base::Rect)).g("Fragment").cmd("operations.fragment"),
    c("ShapesObjectIntersect", i(Base::Rect)).g("Intersect").cmd("operations.intersect"),
    c("ShapesObjectSubtract", i(Base::Rect)).g("Subtract").cmd("operations.subtract"),
    c("ShapesObjectJoin", i(Base::Rect)).g("Join").cmd("operations.join"),
    c("ShapesObjectTrim", i(Base::Rect)).l("Trim").cmd("operations.trim"),
    c("ShapesObjectOffset", i(Base::Rect)).g("Offset").cmd("operations.offsetDialog"),
    c("FitCurveDialog", i(Base::Rect)).g("Fit Curve").cmd("operations.fitCurveDialog"),
    c("ShapesObjectUpdateAlignmentBox", i(Base::Rect)).g("Update Alignment Box"),
];

static DEVELOPER: Tab = Tab {
    id: "TabDeveloper",
    label: "Developer",
    groups: &[
        g(
            "GroupCode",
            "Code",
            true,
            i(Base::Gear),
            &[
                Slot::Big(c("VisualBasic", i(Base::Gear)).g("Visual Basic")),
                Slot::Big(c("MacroPlay", i(Base::Play)).g("Macros")),
                Slot::Stack(&[
                    c("MacroRecord", i(Base::Play)).l("Record Macro"),
                    c("MacroRecorderPause", i(Base::Play)).g("Pause Recording"),
                    c("MacroSecurity", i(Base::Gear)).g("Macro Security"),
                ]),
                Slot::Stack(&[c("RecorderStop", i(Base::Play)).g("Stop Recording"), c("RecorderResume", i(Base::Play)).g("Resume Recording")]),
            ],
        ),
        g("GroupAddins", "Add-ins", true, i(Base::Gear), &[Slot::Big(c("ComAddInsDialog", i(Base::Gear)).g("COM Add-ins"))]),
        g(
            "GroupControls",
            "Controls",
            true,
            i(Base::Window),
            &[
                Slot::Big(c("ControlsGallery", i(Base::Window)).g("Insert").menu(NONE)),
                Slot::Stack(&[
                    c("DesignMode", i(Base::Pencil)).g("Design Mode"),
                    c("ControlProperties", i(Base::Table)).g("Properties"),
                    c("ViewVisualBasicCode", i(Base::Page)).g("View Code"),
                ]),
            ],
        ),
        g(
            "GroupShapeDesign",
            "Shape Design",
            true,
            i(Base::Table),
            &[
                Slot::Big(c("ShowShapeSheet", i(Base::Table)).l("Show ShapeSheet").cmd_with("window.shapesheet", "{}").split(SHEET_ITEMS)),
                Slot::Stack(&[
                    c("DeveloperDrawingTools", i(Base::Rect)).g("Drawing Tools").menu(DRAWING_TOOLS),
                    c("OperationsMenu", i(Base::Rect)).l("Operations").menu(OPERATIONS),
                    c("ObjectsGroupMenu", i(Base::Group)).l("Group").menu(GROUP_ITEMS),
                ]),
                Slot::Stack(&[
                    c("ShapeName", i(Base::TextA)).l("Shape Name"),
                    c("ShapeBehavior", i(Base::Gear)).l("Behavior"),
                    c("ShapeProtection", i(Base::Gear)).l("Protection"),
                ]),
            ],
        ),
        g(
            "GroupNewStencil",
            "Stencil",
            true,
            i(Base::Pages),
            &[Slot::Big(
                c("NewBlankStencil", i(Base::Pages).with(PLUS))
                    .g("New Stencil")
                    .menu(&[c("NewBlankStencilMetric", i(Base::Pages)).g("Metric"), c("NewBlankStencilUS", i(Base::Pages)).g("US Units")]),
            )],
        ),
        g(
            "GroupShowHideDeveloper",
            "Show/Hide",
            true,
            i(Base::Eye),
            &[Slot::Stack(&[
                c("DocumentStencil", i(Base::Pages)).l("Document Stencil").check(),
                c("DrawingExplorer", i(Base::Window)).l("Drawing Explorer").check().cmd_with("window.pane", r#"{"pane":"drawing_explorer"}"#),
                c("MasterExplorer", i(Base::Window)).g("Master Explorer").check(),
            ])],
        ),
        g(
            "GroupSdkTools",
            "Tools",
            true,
            i(Base::Gear),
            &[Slot::Stack(&[
                c("EventMonitor", i(Base::Eye)).g("Event Monitor"),
                c("PersistentEvents", i(Base::Gear)).g("Persistent Events"),
                c("PrintShapeSheet", i(Base::Table)).g("Print ShapeSheet"),
            ])],
        ),
    ],
};

// ------------------------------------------------------------------------------------------------ Org Chart

/// The Layout gallery of the Org Chart tab: the six layouts of a whole chart (`orgchart.layout`, `draw/relayout/org.rs`), a menu of rows.
const ORG_LAYOUT_ITEMS: &[Ctl] = &[
    c("OrgChartLayoutTopToBottom", i(Base::TextBottom))
        .g("Top to Bottom")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"top_to_bottom"}"#)
        .mark(Mark::OrgLayout("top_to_bottom")),
    c("OrgChartLayoutBottomToTop", i(Base::TextTop))
        .g("Bottom to Top")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"bottom_to_top"}"#)
        .mark(Mark::OrgLayout("bottom_to_top")),
    c("OrgChartLayoutLeftToRight", i(Base::ArrowRight))
        .g("Left to Right")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"left_to_right"}"#)
        .mark(Mark::OrgLayout("left_to_right")),
    c("OrgChartLayoutRightToLeft", i(Base::ArrowLeft))
        .g("Right to Left")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"right_to_left"}"#)
        .mark(Mark::OrgLayout("right_to_left")),
    c("OrgChartLayoutHangingRight", i(Base::Align))
        .g("Hanging, to the Right")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"hanging_right"}"#)
        .mark(Mark::OrgLayout("hanging_right")),
    c("OrgChartLayoutHangingLeft", i(Base::Align))
        .g("Hanging, to the Left")
        .cmd_with("window.orgChartDo", r#"{"op":"layout","value":"hanging_left"}"#)
        .mark(Mark::OrgLayout("hanging_left")),
];

/// The menu of Arrange Subordinates: how the reports of the position selected sit (`orgchart.arrange`).
const ORG_ARRANGE_ITEMS: &[Ctl] = &[
    c("OrgChartArrangeHorizontal", i(Base::TextBottom))
        .g("Horizontal")
        .cmd_with("window.orgChartDo", r#"{"op":"arrange","value":"horizontal"}"#)
        .mark(Mark::OrgArrangement("horizontal")),
    c("OrgChartArrangeVerticalRight", i(Base::Align))
        .g("Vertical, to the Right")
        .cmd_with("window.orgChartDo", r#"{"op":"arrange","value":"vertical_right"}"#)
        .mark(Mark::OrgArrangement("vertical_right")),
    c("OrgChartArrangeVerticalLeft", i(Base::Align))
        .g("Vertical, to the Left")
        .cmd_with("window.orgChartDo", r#"{"op":"arrange","value":"vertical_left"}"#)
        .mark(Mark::OrgArrangement("vertical_left")),
    c("OrgChartArrangeInherit", i(Base::Swap))
        .g("Same as the Chart")
        .cmd_with("window.orgChartDo", r#"{"op":"arrange","value":"inherit"}"#)
        .mark(Mark::OrgArrangement("inherit")),
];

/// Insert Picture's menu: one picture for the position selected (several from a folder are not built).
const ORG_PICTURE_ITEMS: &[Ctl] = &[
    c("OrgChartPictureInsertOne", i(Base::Picture)).g("Picture...").cmd_with("window.orgChartDo", r#"{"op":"picture"}"#),
    c("OrgChartPictureInsertMultiple", i(Base::Picture)).g("Multiple Pictures..."),
];

static ORG_CHART: Tab = Tab {
    id: "TabOrgChart",
    label: "Org Chart",
    groups: &[
        g(
            "GroupOrgChartLayout",
            "Layout",
            true,
            i(Base::Connector),
            &[
                Slot::Big(c("OrgChartLayoutGallery", i(Base::Connector)).g("Layout").menu(ORG_LAYOUT_ITEMS)),
                Slot::Stack(&[
                    c("OrgChartRelayout", i(Base::Swap)).g("Re-Layout").cmd_with("window.orgChartDo", r#"{"op":"relayout"}"#),
                    c("OrgChartFitToPage", i(Base::Fit)).g("Fit to Page"),
                    c("OrgChartSynchronize", i(Base::Refresh)).g("Synchronize").menu(NONE),
                ]),
                Slot::Big(c("OrgChartArrangeSubordinates", i(Base::Align)).g("Arrange Subordinates").menu(ORG_ARRANGE_ITEMS)),
            ],
        ),
        g(
            "GroupOrgChartArrange",
            "Arrange",
            true,
            i(Base::Move),
            &[
                Slot::Stack(&[
                    c("OrgChartReportMoveFurther", i(Base::Move)).g("Move Further"),
                    c("OrgChartReportMoveCloser", i(Base::Move)).g("Move Closer"),
                    c("Move", i(Base::Move)).g("Move"),
                ]),
                Slot::Stack(&[
                    c("OrgChartReportMoveLeft", i(Base::ArrowLeft)).g("Move Left"),
                    c("OrgChartReportMoveRight", i(Base::ArrowRight)).g("Move Right"),
                    c("OrgChartSubordinatesShowHide", i(Base::Eye)).g("Show/Hide Subordinates"),
                ]),
            ],
        ),
        g(
            "GroupOrgChartShapes",
            "Shapes",
            true,
            i(Base::Rect),
            &[
                Slot::Big(c("OrgChartChangeStyle", i(Base::Rect)).g("Change Style").menu(NONE)),
                Slot::Stack(&[
                    c("OrgChartShapeHeight", i(Base::Resize)).g("Height"),
                    c("OrgChartShapeWidth", i(Base::Resize)).g("Width"),
                    c("OrgChartPositionTypeChange", i(Base::Move)).g("Position Type"),
                ]),
            ],
        ),
        g(
            "GroupOrgChartPicture",
            "Picture",
            true,
            i(Base::Picture),
            &[
                Slot::Big(c("OrgChartPictureInsert", i(Base::Picture).with(PLUS)).g("Insert Picture").menu(ORG_PICTURE_ITEMS)),
                Slot::Stack(&[
                    c("OrgChartPictureChange", i(Base::Picture)).g("Change Picture").cmd_with("window.orgChartDo", r#"{"op":"picture"}"#),
                    c("OrgChartPictureDelete", i(Base::Picture).with(Badge::Cross))
                        .g("Delete Picture")
                        .cmd_with("window.orgChartDo", r#"{"op":"picture_delete"}"#),
                    c("OrgChartPictureHide", i(Base::Eye)).g("Hide Picture").cmd_with("window.orgChartDo", r#"{"op":"picture_hide"}"#).check(),
                ]),
            ],
        ),
        g(
            "GroupOrgChartOrganizationData",
            "Organization Data",
            true,
            i(Base::Database),
            &[Slot::Stack(&[
                c("OrgChartDataImport", i(Base::FolderOpen)).g("Import"),
                c("OrgChartDataExport", i(Base::Save)).g("Export"),
                c("OrgChartDataCompare", i(Base::Swap)).g("Compare"),
            ])],
        ),
    ],
};

// ---------------------------------------------------------------------------------------------------- Help

static HELP: Tab = Tab {
    id: "HelpTab",
    label: "Help",
    groups: &[g(
        "GroupHelpAndSupport",
        "Help",
        false,
        i(Base::Help),
        &[
            Slot::Big(c("Help", i(Base::Help)).g("Help")),
            Slot::Stack(&[
                c("ContactUs", i(Base::Bubble)).g("Contact Support"),
                c("OfficeFeedbackHelpTab", i(Base::Bubble)).g("Feedback"),
                c("ShowTraining", i(Base::Play)).g("Show Training"),
            ]),
            Slot::Stack(&[c("OfficeFeedbackIcon", i(Base::Bubble)).g("Feedback Icon"), c("WhatsNewRecentUpdates", i(Base::Star)).g("What's New")]),
        ],
    )],
};

// ------------------------------------------------------------------------------------------------ Container Format

const CONTAINER_MARGIN_ITEMS: &[Ctl] = &[
    c("ContainerMarginNone", i(Base::Resize))
        .g("None")
        .cmd_with("window.containerDo", r#"{"op":"margin","value":"none"}"#)
        .mark(Mark::ContainerMargin("none")),
    c("ContainerMarginNarrow", i(Base::Resize))
        .g("Narrow")
        .cmd_with("window.containerDo", r#"{"op":"margin","value":"narrow"}"#)
        .mark(Mark::ContainerMargin("narrow")),
    c("ContainerMarginMedium", i(Base::Resize))
        .g("Medium")
        .cmd_with("window.containerDo", r#"{"op":"margin","value":"medium"}"#)
        .mark(Mark::ContainerMargin("medium")),
    c("ContainerMarginWide", i(Base::Resize))
        .g("Wide")
        .cmd_with("window.containerDo", r#"{"op":"margin","value":"wide"}"#)
        .mark(Mark::ContainerMargin("wide")),
];
const CONTAINER_RESIZE_ITEMS: &[Ctl] = &[
    c("AutomaticResizeNone", i(Base::Resize))
        .l("None")
        .cmd_with("window.containerDo", r#"{"op":"auto_resize","value":"none"}"#)
        .mark(Mark::ContainerResize("none")),
    c("AutomaticResizeExpandAsNeeded", i(Base::Resize))
        .l("Expand as Needed")
        .cmd_with("window.containerDo", r#"{"op":"auto_resize","value":"expand"}"#)
        .mark(Mark::ContainerResize("expand")),
    c("AutomaticResizeAlwaysFitToContents", i(Base::Fit))
        .l("Always Fit to Contents")
        .cmd_with("window.containerDo", r#"{"op":"auto_resize","value":"fit"}"#)
        .mark(Mark::ContainerResize("fit")),
];
const CONTAINER_HEADING_ITEMS: &[Ctl] = &[
    c("ContainerHeadingTop", i(Base::TextTop))
        .g("Top")
        .cmd_with("window.containerDo", r#"{"op":"heading","value":"top"}"#)
        .mark(Mark::ContainerHeading("top")),
    c("ContainerHeadingRight", i(Base::TextRight))
        .g("Right")
        .cmd_with("window.containerDo", r#"{"op":"heading","value":"right"}"#)
        .mark(Mark::ContainerHeading("right")),
    c("ContainerHeadingBottom", i(Base::TextBottom))
        .g("Bottom")
        .cmd_with("window.containerDo", r#"{"op":"heading","value":"bottom"}"#)
        .mark(Mark::ContainerHeading("bottom")),
    c("ContainerHeadingLeft", i(Base::TextLeft))
        .g("Left")
        .cmd_with("window.containerDo", r#"{"op":"heading","value":"left"}"#)
        .mark(Mark::ContainerHeading("left")),
    c("ContainerRotateHeading", i(Base::TurnRight)).g("Rotate Heading").cmd_with("window.containerDo", r#"{"op":"heading","value":"rotate"}"#),
];

/// The contextual tab of a selected container: Size, Container Styles and Membership. The ids are the map's.
static CONTAINER_FORMAT: Tab = Tab {
    id: "TabFormat",
    label: "Container Format",
    groups: &[
        g(
            "GroupContainerSize",
            "Size",
            true,
            i(Base::Resize),
            &[
                Slot::Big(c("ContainerMargins", i(Base::Resize)).l("Margins").cmd("window.containerDo").menu(CONTAINER_MARGIN_ITEMS)),
                Slot::Big(c("FitToContents", i(Base::Fit)).l("Fit to Contents").cmd_with("window.containerDo", r#"{"op":"fit"}"#)),
                Slot::Big(c("AutomaticResize", i(Base::Resize)).l("Automatic Resize").cmd("window.containerDo").menu(CONTAINER_RESIZE_ITEMS)),
            ],
        ),
        g(
            "GroupContainerStyles",
            "Container Styles",
            true,
            i(Base::Container),
            &[
                Slot::Big(
                    c("ContainerStyles", i(Base::Container))
                        .l("Container Styles")
                        .cmd_with("window.containerDo", r#"{"op":"style"}"#)
                        .menu(NONE)
                        .body(Body::ContainerStyles),
                ),
                Slot::Big(c("ContainerHeadingStyles", i(Base::TextTop)).l("Heading Style").cmd("window.containerDo").menu(CONTAINER_HEADING_ITEMS)),
            ],
        ),
        g(
            "GroupMembership",
            "Membership",
            true,
            i(Base::Group),
            &[
                Slot::Big(c("ContainerLock", i(Base::Magnet)).l("Lock Container").cmd_with("window.containerDo", r#"{"op":"lock"}"#).check()),
                Slot::Big(c("SelectContents", i(Base::Select)).l("Select Contents").cmd_with("window.containerDo", r#"{"op":"select_contents"}"#)),
                Slot::Big(
                    c("ContainerDisband", i(Base::Group).with(Badge::Cross))
                        .l("Disband Container")
                        .cmd_with("window.containerDo", r#"{"op":"disband"}"#),
                ),
            ],
        ),
    ],
};

/// The label of the Cross-Functional Flowchart tab.
pub const CROSS_FUNCTIONAL_TAB: &str = "Cross-Functional Flowchart";

/// The contextual tab of a page that has swimlanes ("Cross-functional Flowchart" tab, groups Insert ("Swimlane") and Design ("Rotate Lane Label"); lists
/// a third group between them, Anordnen in the German screenshot it was read from, whose English name and controls no source gives: ours is Arrange, holding the orientation, which
/// puts on the template's tab, and the two steps that move a lane along its list). The ids are ours.
static CROSS_FUNCTIONAL_FLOWCHART: Tab = Tab {
    id: "TabCrossFunctionalFlowchart",
    label: CROSS_FUNCTIONAL_TAB,
    groups: &[
        g(
            "GroupSwimlaneInsert",
            "Insert",
            true,
            i(Base::Table),
            &[
                Slot::Big(
                    c("InsertSwimlane", i(Base::Table).with(PLUS))
                        .l("Swimlane")
                        .cmd_with("window.swimlaneDo", r#"{"op":"insert"}"#)
                        .split(SWIMLANE_INSERT_ITEMS),
                ),
                Slot::Big(c("InsertSeparator", i(Base::Line).with(PLUS)).g("Separator").cmd_with("window.swimlaneDo", r#"{"op":"separator"}"#)),
            ],
        ),
        g(
            "GroupSwimlaneArrange",
            "Arrange",
            false,
            i(Base::Swap),
            &[
                Slot::Big(c("SwimlaneOrientation", i(Base::Swap)).g("Orientation").cmd("window.swimlaneDo").menu(SWIMLANE_ORIENTATION_ITEMS)),
                Slot::Stack(&[
                    c("SwimlaneEarlier", i(Base::ArrowLeft)).g("Move Earlier").cmd_with("window.swimlaneDo", r#"{"op":"earlier"}"#),
                    c("SwimlaneLater", i(Base::ArrowRight)).g("Move Later").cmd_with("window.swimlaneDo", r#"{"op":"later"}"#),
                ]),
            ],
        ),
        g(
            "GroupSwimlaneDesign",
            "Design",
            true,
            i(Base::TextTurn),
            &[Slot::Big(c("RotateLaneLabel", i(Base::TextTurn)).l("Rotate Lane Label").cmd_with("window.swimlaneDo", r#"{"op":"rotate_label"}"#))],
        ),
    ],
};

// ------------------------------------------------------------------------------------------------ ShapeSheet Design

/// The label of the ShapeSheet Design tab.
pub const SHEET_DESIGN_TAB: &str = "ShapeSheet Design";

/// The contextual tab of the ShapeSheet window: View, Sections, Rows, Editing and
/// Formula Tracing. It shows while the sheet has the focus. **The ids and the labels are the map's and the spec's** (the April 2025 image names Link and Style Explorer). In this
/// feature View's Formulas, Values and Sections work, and so do the Sections, Rows and Editing groups (Insert and Delete Section and Row, Edit Formula, and Shape
/// Data, Link and Shape Action, which add a row to the sheet); Style Explorer and Formula Tracing are later work (design 2): they are drawn quiet, with the tooltip "(not available yet)".
static SHEET_DESIGN: Tab = Tab {
    id: "TabShapeSheetDesign",
    label: SHEET_DESIGN_TAB,
    groups: &[
        g(
            "GroupShapeSheetView",
            "View",
            true,
            i(Base::Table),
            &[
                Slot::Stack(&[
                    c("ShapeSheetShowFormulas", i(Base::TextA)).l("Formulas").check().cmd_with("window.sheetView", r#"{"formulas":true}"#),
                    c("ShapeSheetShowValues", i(Base::Table)).l("Values").check().cmd_with("window.sheetView", r#"{"values":true}"#),
                    c("ShapeSheetShowSections", i(Base::Layers)).l("Sections").cmd("window.sheetSections").menu(NONE).body(Body::SheetSections),
                ]),
                Slot::Big(c("StyleExplorer", i(Base::Palette)).l("Style Explorer").check()),
            ],
        ),
        g(
            "GroupShapeSheetSections",
            "Sections",
            true,
            i(Base::Layers),
            &[Slot::Stack(&[
                c("ShapeSheetInsertSection", i(Base::Layers).with(PLUS)).l("Insert").cmd("window.sheetInsertSection"),
                c("ShapeSheetDeleteSection", i(Base::Layers).with(Badge::Cross)).l("Delete").cmd("window.sheetDeleteSection"),
            ])],
        ),
        g(
            "GroupShapeSheetRows",
            "Rows",
            true,
            i(Base::Table),
            &[Slot::Stack(&[
                c("ShapeSheetInsertRow", i(Base::Table).with(PLUS)).l("Insert").cmd_with("window.sheetInsertRow", r#"{"at":"after"}"#),
                c("ShapeSheetDeleteRow", i(Base::Table).with(Badge::Cross)).l("Delete").cmd("window.sheetDeleteRow"),
            ])],
        ),
        g(
            "GroupShapeSheetEditing",
            "Editing",
            true,
            i(Base::TextA),
            &[
                Slot::Big(c("ShapeSheetEditFormula", i(Base::TextA)).l("Edit Formula").cmd("window.sheetEditFormula")),
                Slot::Stack(&[
                    c("ShapeData", i(Base::Database)).l("Shape Data").cmd_with("window.sheetAddRow", r#"{"section":"Prop"}"#),
                    c("HyperlinkInsert", i(Base::Link)).l("Link").cmd_with("window.sheetAddRow", r#"{"section":"Hyperlink"}"#),
                    c("ShapeSheetShapeAction", i(Base::Gear)).l("Shape Action").cmd_with("window.sheetAddRow", r#"{"section":"Actions"}"#),
                ]),
            ],
        ),
        g(
            "GroupShapeSheetFormulaTracing",
            "Formula Tracing",
            true,
            i(Base::Window),
            &[
                Slot::Big(c("ShowFormulaTracingWindow", i(Base::Window)).l("Show Window")),
                Slot::Stack(&[
                    c("TracePrecedentCells", i(Base::TurnLeft)).l("Trace Precedents"),
                    c("TraceDependentCells", i(Base::TurnRight)).l("Trace Dependents"),
                ]),
                Slot::Big(c("TraceBack", i(Base::Undo)).l("Back")),
                Slot::Big(c("TraceForward", i(Base::Redo)).l("Forward")),
            ],
        ),
    ],
};

/// What decides which tabs are on the row beyond the default ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabContext {
    /// The Developer tab is on (it is off until turned on).
    pub developer: bool,
    /// The document holds an org chart, which brings its own tab.
    pub org_chart: bool,
    /// A container is selected, which brings the Container Format tab at the right end of the row, after Help.
    pub container: bool,
    /// The page on view has swimlanes, which brings the Cross-Functional Flowchart tab after Help, before the Container Format tab.
    pub swimlane: bool,
    /// A ShapeSheet window is open and has the focus, which brings the ShapeSheet Design tab at the right end of the row.
    pub sheet: bool,
}

/// The name of the File tab, which comes first and opens the File view, not a ribbon body.
pub const FILE_TAB: &str = "File";

/// The tabs with a ribbon body that show for `cx`, in the order of the row (Home, Insert, Draw, Design,
/// Data, Process, Review, View, then Developer if on, Org Chart for an org chart, Help).
pub fn visible_tabs(cx: TabContext) -> Vec<&'static Tab> {
    let mut tabs = vec![&HOME, &INSERT, &DRAW, &DESIGN, &DATA, &PROCESS, &REVIEW, &VIEW];
    if cx.developer {
        tabs.push(&DEVELOPER);
    }
    if cx.org_chart {
        tabs.push(&ORG_CHART);
    }
    tabs.push(&HELP);
    // A contextual tab is added at the right end of the row, and goes when its object is not selected: a template's tab first, then Container Format.
    if cx.swimlane {
        tabs.push(&CROSS_FUNCTIONAL_FLOWCHART);
    }
    if cx.container {
        tabs.push(&CONTAINER_FORMAT);
    }
    if cx.sheet {
        tabs.push(&SHEET_DESIGN);
    }
    tabs
}

/// The labels of the whole row for `cx`, File first.
pub fn tab_names(cx: TabContext) -> Vec<&'static str> {
    std::iter::once(FILE_TAB).chain(visible_tabs(cx).into_iter().map(|t| t.label)).collect()
}

/// The tab with this label, whether or not it shows now.
pub fn tab_by_label(label: &str) -> Option<&'static Tab> {
    visible_tabs(TabContext { developer: true, org_chart: true, container: true, swimlane: true, sheet: true }).into_iter().find(|t| t.label == label)
}

impl Ctl {
    /// This control and every control inside it, depth first.
    pub fn walk(&self, f: &mut impl FnMut(&Ctl)) {
        f(self);
        for item in self.items {
            item.walk(f);
        }
    }
}

impl Group {
    /// Every control of the group (its launcher too), menus opened out.
    pub fn walk(&self, f: &mut impl FnMut(&Ctl)) {
        for slot in self.slots {
            match slot {
                Slot::Big(c) | Slot::Gallery(c) => c.walk(f),
                Slot::Stack(cs) => cs.iter().for_each(|c| c.walk(f)),
                Slot::Rows(rows) => rows.iter().flat_map(|r| r.iter()).for_each(|c| c.walk(f)),
            }
        }
        if let Some(l) = &self.launcher {
            l.walk(f);
        }
    }

    /// The group's top-level controls in order, not the ones inside menus.
    pub fn top_level(&self) -> Vec<&Ctl> {
        let mut out = Vec::new();
        for slot in self.slots {
            match slot {
                Slot::Big(c) | Slot::Gallery(c) => out.push(c),
                Slot::Stack(cs) => out.extend(cs.iter()),
                Slot::Rows(rows) => out.extend(rows.iter().flat_map(|r| r.iter())),
            }
        }
        out
    }
}

/// The control of the Home tab with this id, wherever it sits: what a pane that does what a Home control does asks for (Format Shape's colours).
pub(crate) fn home_control(id: &str) -> Option<&'static Ctl> {
    let mut found = None;
    for group in HOME.groups {
        group.walk(&mut |c| {
            if c.id == id && found.is_none() {
                found = Some(*c);
            }
        });
    }
    // `walk` hands out copies; the table's own entry is the one with that id in a slot.
    found.and_then(|c| HOME.groups.iter().flat_map(|g| g.top_level()).find(|top| top.id == c.id))
}

/// Every tab that can show, for tests and the parity count.
pub fn all_tabs() -> Vec<&'static Tab> {
    visible_tabs(TabContext { developer: true, org_chart: true, container: true, swimlane: true, sheet: true })
}

/// The captions of the rows of the colour palettes that are ours and not confirmed by the map: the tab, the group, the control the palette
/// belongs to and the words.
pub fn unconfirmed_palette_captions() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for tab in all_tabs() {
        for group in tab.groups {
            group.walk(&mut |c| {
                if let Some(palette) = &c.palette {
                    for caption in palette.none.iter().chain([&palette.more, &palette.default]).filter(|caption| !caption.confirmed) {
                        out.push((tab.label, group.id, c.id, caption.text));
                    }
                }
            });
        }
    }
    out
}

/// The ids of controls whose caption is ours and not confirmed by the map, with the group each is in.
pub fn unconfirmed_captions() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for tab in all_tabs() {
        for group in tab.groups {
            group.walk(&mut |c| {
                if !c.confirmed {
                    out.push((tab.label, group.id, c.id));
                }
            });
        }
    }
    out
}
