//! The built-in stencils and their masters: seven stencils of fifty-eight original masters, and thirteen more registered and filled by family (STENCIL2). New figures follow.
//!
//! **Original figures.** Each outline is plain mathematics (a rectangle, an ellipse, a regular polygon fitted to its box, arcs through three
//! points), and its name, default size and connection points are our choice.
//! Nothing here is traced, measured or copied from another application's stencil. The flowchart names are the generic ones of the public
//! notation, read on 2026-11-01 from <https://en.wikipedia.org/wiki/Flowchart> (terminal, process, decision, input/output, predefined
//! process, on-page and off-page connectors, database, document, multiple documents, manual operation, manual input, preparation) and
//! from general descriptions of ISO 5807 flowcharts (delay, display, internal storage, summing junction, or). The drawings are ours.
//!
//! **Formulas of `Width` and `Height`**: a vertex is a share of the box, written as a formula
//! (`Width*0.809`) with the value it gives at the master's default size. An instance of another size inherits the formula and
//! evaluates it on itself, so a resize moves every vertex and every connection point with it. A share of 0
//! is a plain `0`, and a share of 1 is `Width` or `Height`. A flowchart master may also keep a slant or a rounded end the same size
//! while the box grows: `Width-Height*0.25` is a vertex a quarter of the height in from the right, at any width.
//!
//! **Fixed lengths follow the page's drop scale** (owner 2026-11-27; `docs/editing-commands.md`). A master dropped on a page whose
//! drawing scale is beyond the range of eight of its own keeps its size on paper, and a figure must look the same on paper as at 1:1, so **a length a master's formulas hold as a constant (the
//! head of a lifeline, a rounded box's corner, a package's tab, the markers of BPMN, the label bands of C4) is written times the factor the drop wrote**: `0.4*IF(DropOnPageScale>0,DropOnPageScale,1)`
//! on a shape of one piece, and `0.4*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)` in a member of a group, which no drop writes a factor on. A cell found nowhere reads
//! 0, so the formula falls back to 1 where there is none, and a drawing whose root style has no such cell keeps the lengths it had. The one place that makes the text is `Co::formula` for the
//! coordinates of an outline and of a connection point, `scaled!` for a table of member frames, and `dress` for the margin of a container (`group_master::drop_factor` is the factor). A share of
//! the width or the height already follows the size and is written as it was, and so is anything that is no length: a ratio, an angle, a count, a text size or a line weight.
//!
//! **Connection points**: rows of the `Connection` section, `IX` 0 up, each with `X`,
//! `Y`, `DirX`, `DirY` (the way a connector leaves) and `Type` 0. Every two-dimensional master has at least four, and each lies on its outline.
//!
//! **Text block**: the seven `Text Transform` cells with the formulas a public page prints as the ones written on creation
//! So a label sits in the middle of the shape and follows its size. A Basic Flowchart master may
//! narrow the block to the part of the shape that holds text (the inside of a diamond, the body of a document).
//!
//! **Several paths**: a master of more than one `Geometry` section draws the strokes
//! that are not part of its fill (the rims of a predefined process, the front edge of a database) as sections with `NoFill` 1.
//!
//! **Stencils from files**: `opened` holds the stencils a session has opened (`stencil.open`), `bring` is the one place that decides
//! whether a document already holds a master and copies one in with the styles, colours and faces it depends on, `instance` drops an opened
//! master on a page, and `icon` looks at a master's icon. The built-in stencils above are the same kind of thing to a window: a name and a list
//! of masters. **Which built-in stencils are open** is the session's, in `opened`: all by default, the ones a template names after `file.new {template}`,
//! and `stencil.open {id}` opens one more by hand (`stencil.close` refuses a built-in stencil); `shape.drop` searches every built-in stencil whether or not it is open.
//!
//! **A catalogue of registered and shipped stencils**. `ALL_STENCILS` registers every stencil the project plans, the second batch's thirteen with no masters yet. A stencil is
//! **shipped** when it has masters (`StencilSpec::shipped`): [`stencils`] gives the shipped ones in catalogue order and is the only list a command, a query or a window reads, so a stencil that is
//! not shipped is in no listing, search, `stencil.open` or `template.list` answer. A stencil is **open by default** (`StencilSpec::default_open`) in a new session: the seven of the first batches
//! are, the thirteen new ones are closed and a template opens its own.
//!
//! **A master is data** (`MasterSpec`): an outline (a polygon, an ellipse, paths, geometry made by a function, a one-dimensional figure, a group), its connection points, and optional extras
//! ([`dress::More`]): a [`dress::Dress`] (look, text, Shape Data, `User` rows, locks and role), a hidden flag and the masters it brings. A **connector master** holds its line ends, pattern and route
//! as a [`look::ConnectorLook`] on its own shape, and its instances inherit them. **A group master** (`Outline::Group`: [`group_master`]) is a `Group` with no geometry of its own and members that follow it
//! by guarded formulas (compartments as tall as their text among them); it is dropped through `instance.rs`, with every member listed, as a stencil file's group master is. **A container master** is a master whose dress has the
//! container role: a container like any other, whose outline no container command makes again.

use std::sync::{Arc, LazyLock};

use diacraft_doc::{Cell, CellName, DocError, Document, Links, Master, Row, Section, Shape, ShapeList, ShapeType, Sheet, Value};

/// A length of a member's frame that follows the page's drop scale, as the text of a formula: `scaled!("0.08")` is `0.08*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)`
/// (a drop beyond the range of eight writes the factor on the group, `Sheet.5`, and no member holds one). A table of frames is `&'static str`, so the
/// text is made at compile time; `group_master::drop_factor` holds the same text (a test says they are one). Only a length is written so: a share of the group's width or height is not.
macro_rules! scaled {
    ($length:literal) => {
        concat!($length, "*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)")
    };
}

mod arrows;
mod bpmn;
mod bring;
mod c4;
mod callout;
mod chen;
mod cloud;
mod computers;
pub(crate) mod container;
mod crows_foot;
mod decorative;
mod drawn;
pub(crate) mod dress;
mod flowchart;
mod gane_sarson;
pub(crate) mod group_master;
mod icon;
mod instance;
mod keywords;
pub(crate) mod kit;
mod line_ends;
pub(crate) mod look;
mod network;
mod opened;
pub(crate) mod orgchart;
mod picture;
pub mod quick;
pub(crate) mod refs;
mod search;
pub(crate) mod swimlane;
mod text_box;
mod uml_activity;
mod uml_class;
mod uml_component;
mod uml_sequence;
mod uml_state;
mod uml_usecase;
mod unique;
pub mod xfunc;

pub(crate) use bring::left_out_words;
pub use bring::{Brought, Identity, MAX_BRING_NODES, Refusal, bring_master, find_same_master};
pub use callout::{CALLOUT_HEIGHT, CALLOUT_TEXT, CALLOUT_WIDTH, CalloutStyle, STYLES as CALLOUT_STYLES, find_style as find_callout_style};
pub(crate) use callout::{CalloutFrame, callout_shape, free_tip_cells, tied_tip_cells};
pub(crate) use drawn::{DrawnFrame, drawn_shape};
pub(crate) use unique::repair_old_ids;
// The words a family's table is written in (STENCIL2): the family modules import them from here, and a stub names none yet.
#[allow(unused_imports)]
pub(crate) use dress::{CharRow, Dress, FieldDef, FieldKind, FieldText, More, PartKind, Role, TextSpec};
pub use icon::{IconFormat, IconInfo, IconProblem, MAX_ICON_BYTES, decode as decode_icon, inspect as inspect_icon};
pub(crate) use instance::drop_group_master;
pub use instance::{MAX_LISTED_MEMBERS, OpenedDrop, drop_opened, drop_opened_following};
#[allow(unused_imports)]
pub(crate) use look::{ConnectorLook, Flag, LineEnd, Route};
pub use opened::{
    AddError, IconError, IconState, MAX_LISTED_MASTERS, MAX_OPEN_ITEMS, MAX_OPEN_MEDIA_BYTES, MAX_OPEN_STENCILS, MAX_OPEN_XML_BYTES, MasterEntry,
    ModelCost, OpenedStencil, OpenedStencils,
};
pub(crate) use picture::picture_shape;
pub use search::{
    MAX_KEYWORD_BYTES, MAX_LISTED, MAX_QUERY_CHARS, MAX_QUERY_WORDS, MAX_SEARCHED_BYTES, MAX_SEARCHED_MASTERS, MAX_WORD_CHARS,
    Outcome as SearchOutcome, Query as SearchQuery, QueryProblem, Stop as SearchStop, answer as search_answer, search as search_stencils,
};
pub use text_box::{TEXT_BOX_HEIGHT, TEXT_BOX_WIDTH};
pub(crate) use text_box::{text_box_shape, text_transform_cells};

/// The `ID` of the one shape in each master's part. Shapes in master parts have an `ID` of at least 4 in
/// every sample, so ours is 5.
pub const MASTER_SHAPE_ID: u32 = 5;

/// The id and the name of the stencil.
pub const STENCIL_ID: &str = "basic";
/// The stencil's name, as the Shapes window shows it.
pub const STENCIL_NAME: &str = "Basic Shapes";
/// The id of the flowchart stencil.
pub const FLOWCHART_ID: &str = "flowchart";
/// The flowchart stencil's name, as the Shapes window shows it.
pub const FLOWCHART_NAME: &str = "Basic Flowchart";

/// An outline or connection coordinate, written as a formula: shares of width, height and their minimum, a fixed length in inches, and a corner radius that gives way when the box is small.
///
/// The value is `w * Width + h * Height + min * MIN(Width,Height) + k + rs * MIN(rad, MIN(Width,Height) * 0.25)`. The last term is **the corner radius**: a rounded box keeps a corner of
/// `rad` inches until the box is smaller than four of them, and then the corner follows the box (`kit::rounded_box`). A coordinate holds at most one radius.
///
/// **The two lengths follow the page's drop scale**: the value is the one at the master's own scale, and the formula writes `k` and `rad` as
/// `k*<factor>` and `MIN(rad*<factor>,...)`, the factor being the `DropOnPageScale` that a drop beyond the range of eight writes (`group_master::drop_factor`), so that a fixed length is as long
/// on paper at every scale as it is on the master's. A share of the box already follows the box, and is written as it was.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Co {
    w: f64,
    h: f64,
    min: f64,
    /// A fixed length, in inches.
    k: f64,
    /// The radius of the corner term, in inches, and its coefficient: 0 for no such term.
    rad: f64,
    rs: f64,
}

/// A share `w` of `Width`.
const fn fw(w: f64) -> Co {
    Co { w, h: 0.0, min: 0.0, k: 0.0, rad: 0.0, rs: 0.0 }
}

/// A share `h` of `Height`.
const fn fh(h: f64) -> Co {
    Co { w: 0.0, h, min: 0.0, k: 0.0, rad: 0.0, rs: 0.0 }
}

/// `Width*w + Height*h`.
const fn mix(w: f64, h: f64) -> Co {
    Co { w, h, min: 0.0, k: 0.0, rad: 0.0, rs: 0.0 }
}

/// A fixed length of `k` inches.
#[allow(dead_code, reason = "the kit and the families use it as they land")]
const fn fk(k: f64) -> Co {
    Co { w: 0.0, h: 0.0, min: 0.0, k, rad: 0.0, rs: 0.0 }
}

#[allow(dead_code, reason = "the kit and the families use it as they land")]
/// `coeff` times the corner radius `rad` inches (`MIN(rad, MIN(Width,Height)*0.25)`).
const fn fr(rad: f64, coeff: f64) -> Co {
    Co { w: 0.0, h: 0.0, min: 0.0, k: 0.0, rad, rs: coeff }
}

impl Co {
    #[allow(dead_code, reason = "the kit and the families use it as they land")]
    /// The sum of two coordinates. Both may not hold a corner radius: the first one's is kept.
    const fn plus(self, o: Co) -> Co {
        let (rad, rs) = if self.rs != 0.0 { (self.rad, self.rs + if o.rad == self.rad { o.rs } else { 0.0 }) } else { (o.rad, o.rs) };
        Co { w: self.w + o.w, h: self.h + o.h, min: self.min + o.min, k: self.k + o.k, rad, rs }
    }

    /// The value at a box of `width` by `height`.
    fn value(self, width: f64, height: f64) -> f64 {
        let least = width.min(height);
        let corner = if self.rs == 0.0 { 0.0 } else { self.rs * self.rad.min(least * 0.25) };
        self.w * width + self.h * height + self.min * least + self.k + corner
    }

    /// The formula, or `None` when there is no length at all: the plain constant 0. A share of 1 is the bare name: `Width`, `Width-Height*0.5`. **A fixed length or a corner radius is written
    /// times the factor of the drop scale** (`of`): `Width-0.2*IF(DropOnPageScale>0,DropOnPageScale,1)`, `0.4*IF(...)` for a length alone.
    fn formula(self, of: Factor) -> Option<String> {
        let factor = drop_factor(of);
        let mut text = String::new();
        for (k, name) in [(self.w, "Width"), (self.h, "Height"), (self.min, "MIN(Width,Height)")] {
            if k == 0.0 {
                continue;
            }
            if text.is_empty() {
                if k < 0.0 {
                    text.push('-');
                }
            } else {
                text.push(if k < 0.0 { '-' } else { '+' });
            }
            text.push_str(name);
            if k.abs() != 1.0 {
                text.push('*');
                text.push_str(&number_text(k.abs()));
            }
        }
        if self.rs != 0.0 {
            if text.is_empty() {
                if self.rs < 0.0 {
                    text.push('-');
                }
            } else {
                text.push(if self.rs < 0.0 { '-' } else { '+' });
            }
            if self.rs.abs() != 1.0 {
                text.push_str(&number_text(self.rs.abs()));
                text.push('*');
            }
            text.push_str(&format!("MIN({}*{factor},MIN(Width,Height)*0.25)", number_text(self.rad)));
        }
        if self.k != 0.0 {
            if text.is_empty() {
                text.push_str(&number_text(self.k));
            } else {
                text.push(if self.k < 0.0 { '-' } else { '+' });
                text.push_str(&number_text(self.k.abs()));
            }
            text.push('*');
            text.push_str(factor);
        }
        (!text.is_empty()).then_some(text)
    }

    /// Whether the coordinate is a fixed length and nothing else: no share of the box and no corner radius.
    fn is_fixed(self) -> bool {
        self.w == 0.0 && self.h == 0.0 && self.min == 0.0 && self.rs == 0.0
    }

    /// The cell: a length formula with the value it gives at the default size, or the constant 0.
    fn cell(self, width: f64, height: f64, of: Factor) -> Cell {
        self.formula(of).map_or_else(|| length(self.k), |f| length_formula(&f, self.value(width, height)))
    }
}

/// A point of an outline.
type Xy = (Co, Co);

/// A connection point of a master: where it sits, and the way a connector leaves it.
#[derive(Clone, Copy, Debug)]
struct Point {
    x: Co,
    y: Co,
    dx: f64,
    dy: f64,
}

/// One piece of a path. The path starts with a `Move`, and a filled one returns to where it started.
#[derive(Clone, Copy, Debug)]
enum Seg {
    /// `MoveTo`: lifts the pen.
    Move(Co, Co),
    /// `LineTo`.
    Line(Co, Co),
    /// An arc of a circle from the pen to `to` through `via` (an `EllipticalArcTo` of ratio 1, which three points fix and which needs no sign
    /// convention for a bow).
    Arc { to: Xy, via: Xy },
    /// An arc of an ellipse whose axes are along the box's, with semi-axes `rx` and `ry`, from the pen to `to` through `via` (an
    /// `EllipticalArcTo` of ratio `rx / ry`).
    Half { to: Xy, via: Xy, rx: Co, ry: Co },
    /// The ellipse that fills the box; it is the whole path.
    Ellipse,
}

/// A `Geometry` section of a master: one path.
struct PathSpec {
    /// Whether it takes the fill (`NoFill` 0). A path that only draws a stroke inside another's fill has `NoFill` 1.
    fill: bool,
    segs: &'static [Seg],
    /// `NoShow`: the path is not drawn (a marker a variant of Shape Data hides). A formula of the Shape Data, or a constant.
    no_show: Flag,
    /// `NoLine`: the path takes the fill and draws no line.
    no_line: Flag,
}

/// A `Geometry` section made by a function when the master is built ([`Outline::Code`]): the same as a [`PathSpec`], with its segments held.
struct OwnedPath {
    fill: bool,
    segs: Vec<Seg>,
    no_show: Flag,
    no_line: Flag,
}

/// The text block as shares of the box: left, bottom, width, height.
#[derive(Clone, Copy)]
struct TextBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl TextBox {
    /// The whole box.
    const FULL: TextBox = TextBox { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
}

/// The outline of a master.
#[allow(dead_code, reason = "Fixed1D, Code and Group are built by the families as they land (STENCIL2)")]
enum Outline {
    /// A real one-dimensional directed connector, constructed by the existing connector path, with its look: the Data Flow's is [`ConnectorLook::DATA_FLOW`].
    Connector(ConnectorLook),
    /// A one-dimensional shape that draws its own geometry and is **not routed**: its ends glue as a connector's do, and a move of a shape it is glued to stretches it and does not route it.
    /// The geometry is in the line's own frame: x along it, y across it.
    Fixed1D { look: ConnectorLook, paths: &'static [PathSpec] },
    /// Vertices as shares of the box, in order; the path returns to the first.
    Polygon(&'static [(f64, f64)]),
    /// The ellipse that fills the box.
    Ellipse,
    /// One or more paths, one `Geometry` section each, and where the text goes.
    Paths { paths: &'static [PathSpec], text: TextBox },
    /// Paths made by a function when the master is built, for figures with repeats. **Capped**: [`MAX_SECTIONS`] sections of [`MAX_ROWS`] rows in all ([`caps_problem`]).
    Code { build: fn() -> Vec<OwnedPath>, text: TextBox },
    /// A group of members (builds it: until then the master has its top-level shape and no members).
    Group { members: &'static [MemberSpec], text: TextBox },
}

/// The most `Geometry` sections, rows and connection points one master may hold (the catalogue test holds every master to them, and a build never goes past them).
pub(crate) const MAX_SECTIONS: usize = 64;
pub(crate) const MAX_ROWS: usize = 1_024;
pub(crate) const MAX_POINTS: usize = 128;

/// One master of the stencil.
pub struct MasterSpec {
    /// A stable lowercase id, optionally namespaced (`arrows.right`). Accepted by `shape.drop`, as is the name.
    pub id: &'static str,
    pub name: &'static str,
    /// The default size in inches.
    pub width: f64,
    pub height: f64,
    /// Stable namespace and ordinal, independent of display names and catalogue order.
    identity: (u8, u16),
    outline: Outline,
    points: &'static [Point],
    /// What the master wears besides its outline: `More::NONE` for the masters of the first seven stencils.
    more: &'static More,
}

/// A stencil: a named list of masters.
#[derive(Clone, Copy)]
pub struct StencilSpec {
    /// A lowercase word: `flowchart`.
    pub id: &'static str,
    /// As the Shapes window shows it.
    pub name: &'static str,
    /// What the More Shapes menu files it under (ours): the words of the template categories (`General`, `Flowchart`) and `Software and Database`.
    pub category: &'static str,
    masters: &'static [MasterSpec],
    /// Open in a new session: true for the seven stencils of the first batches, false for the new ones (a template opens its own).
    pub default_open: bool,
}

impl StencilSpec {
    /// The masters, in the order the Shapes window lists them.
    pub fn masters(&self) -> &'static [MasterSpec] {
        self.masters
    }

    /// Whether the stencil has masters. One that has none is registered and not shipped: no command, query or listing reaches it.
    pub fn shipped(&self) -> bool {
        !self.masters.is_empty()
    }

    /// Whether the AutoConnect strip takes masters from this stencil: every built-in one but Arrow Shapes ("Not all stencils feed it (the Arrow Shapes
    /// stencil does not)"). The rule the strip follows is `stencil/quick.rs`.
    pub fn feeds_strip(&self) -> bool {
        self.id != ARROWS_ID
    }
}

/// The id of the Arrow Shapes stencil, which does not feed the AutoConnect strip.
const ARROWS_ID: &str = "arrows";

const fn pt(fx: f64, fy: f64, dx: f64, dy: f64) -> Point {
    Point { x: fw(fx), y: fh(fy), dx, dy }
}

/// A connection point at (`x`, `y`) in the general form.
const fn pc(x: Co, y: Co, dx: f64, dy: f64) -> Point {
    Point { x, y, dx, dy }
}

use group_master::{Factor, MemberSpec, drop_factor};
use kit::SIDES;

/// Vertices of a regular pentagon fitted to its box: the golden ratio gives 0.809, 0.191 and 0.618.
const PENTAGON: [(f64, f64); 5] = [(0.5, 1.0), (0.0, 0.618), (0.191, 0.0), (0.809, 0.0), (1.0, 0.618)];
/// The corner cut of a regular octagon in a square: 1 / (2 + sqrt 2).
const CUT: f64 = 0.2929;

static MASTERS: [MasterSpec; 12] = [
    MasterSpec {
        identity: (1, 1),
        more: &More::PLACEABLE,
        id: "rectangle",
        name: "Rectangle",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 2),
        more: &More::PLACEABLE,
        id: "square",
        name: "Square",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 3),
        more: &More::PLACEABLE,
        id: "ellipse",
        name: "Ellipse",
        width: 1.5,
        height: 1.0,
        outline: Outline::Ellipse,
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 4),
        more: &More::PLACEABLE,
        id: "circle",
        name: "Circle",
        width: 1.0,
        height: 1.0,
        outline: Outline::Ellipse,
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 5),
        more: &More::PLACEABLE,
        id: "triangle",
        name: "Triangle",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)]),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.25, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.75, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 6),
        more: &More::PLACEABLE,
        id: "right_triangle",
        name: "Right Triangle",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        points: &[pt(0.0, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.5, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 7),
        more: &More::PLACEABLE,
        id: "diamond",
        name: "Diamond",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 8),
        more: &More::PLACEABLE,
        id: "pentagon",
        name: "Pentagon",
        width: 1.2,
        height: 1.14,
        outline: Outline::Polygon(&PENTAGON),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.618, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.618, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 9),
        more: &More::PLACEABLE,
        id: "hexagon",
        name: "Hexagon",
        width: 1.4,
        height: 1.2,
        outline: Outline::Polygon(&[(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 10),
        more: &More::PLACEABLE,
        id: "octagon",
        name: "Octagon",
        width: 1.2,
        height: 1.2,
        outline: Outline::Polygon(&[
            (CUT, 0.0),
            (1.0 - CUT, 0.0),
            (1.0, CUT),
            (1.0, 1.0 - CUT),
            (1.0 - CUT, 1.0),
            (CUT, 1.0),
            (0.0, 1.0 - CUT),
            (0.0, CUT),
        ]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 11),
        more: &More::PLACEABLE,
        id: "parallelogram",
        name: "Parallelogram",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.25, 0.0), (1.0, 0.0), (0.75, 1.0), (0.0, 1.0)]),
        points: &[pt(0.375, 1.0, 0.0, 1.0), pt(0.125, 0.5, -1.0, 0.0), pt(0.625, 0.0, 0.0, -1.0), pt(0.875, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 12),
        more: &More::PLACEABLE,
        id: "trapezoid",
        name: "Trapezoid",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (0.8, 1.0), (0.2, 1.0)]),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.1, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.9, 0.5, 1.0, 0.0)],
    },
];

/// The masters of Basic Shapes, in the order the Shapes window lists them. The other stencils are in [`stencils`].
pub fn masters() -> &'static [MasterSpec] {
    &MASTERS
}

/// The category words of the second batch's stencils.
const SOFTWARE: &str = "Software and Database";

/// **Every stencil the project registers**, in the order the Shapes window lists them: the seven of the first batches (open in a new session), and the thirteen of the second batch
/// (STENCIL2: UML, BPMN, C4, network, cloud, crow's foot and Chen), registered with no masters and closed by default, which a family task fills. A stencil with no masters is not shipped
/// ([`StencilSpec::shipped`]): nothing reads it but [`registered`], which the catalogue's own tests use.
static ALL_STENCILS: [StencilSpec; 20] = [
    StencilSpec { id: STENCIL_ID, name: STENCIL_NAME, category: "General", masters: &MASTERS, default_open: true },
    StencilSpec { id: FLOWCHART_ID, name: FLOWCHART_NAME, category: "Flowchart", masters: &flowchart::FLOWCHART, default_open: true },
    StencilSpec { id: "decorative", name: "Decorative Shapes", category: "General", masters: &decorative::MASTERS, default_open: true },
    StencilSpec { id: ARROWS_ID, name: "Arrow Shapes", category: "General", masters: &arrows::MASTERS, default_open: true },
    StencilSpec { id: "gane-sarson", name: "Gane-Sarson", category: SOFTWARE, masters: &gane_sarson::MASTERS, default_open: true },
    StencilSpec { id: xfunc::ID, name: "Cross-Functional Flowchart Shapes", category: "Flowchart", masters: &xfunc::MASTERS, default_open: true },
    StencilSpec { id: orgchart::ID, name: orgchart::NAME, category: "Business", masters: &orgchart::MASTERS, default_open: true },
    StencilSpec { id: "uml-class", name: "UML Class", category: SOFTWARE, masters: &uml_class::MASTERS, default_open: false },
    StencilSpec { id: "uml-usecase", name: "UML Use Case", category: SOFTWARE, masters: &uml_usecase::MASTERS, default_open: false },
    StencilSpec { id: "uml-sequence", name: "UML Sequence", category: SOFTWARE, masters: &uml_sequence::MASTERS, default_open: false },
    StencilSpec { id: "uml-activity", name: "UML Activity", category: SOFTWARE, masters: &uml_activity::MASTERS, default_open: false },
    StencilSpec { id: "uml-state", name: "UML State Machine", category: SOFTWARE, masters: &uml_state::MASTERS, default_open: false },
    StencilSpec {
        id: "uml-component",
        name: "UML Component and Deployment",
        category: SOFTWARE,
        masters: &uml_component::MASTERS,
        default_open: false,
    },
    StencilSpec { id: "bpmn", name: "BPMN Shapes", category: "Flowchart", masters: &bpmn::MASTERS, default_open: false },
    StencilSpec { id: "c4", name: "C4 Model", category: SOFTWARE, masters: &c4::MASTERS, default_open: false },
    StencilSpec { id: "network", name: "Network Devices", category: "Network", masters: &network::MASTERS, default_open: false },
    StencilSpec { id: "computers", name: "Computers and Devices", category: "Network", masters: &computers::MASTERS, default_open: false },
    StencilSpec { id: "cloud", name: "Cloud Services", category: "Network", masters: &cloud::MASTERS, default_open: false },
    StencilSpec { id: "crows-foot", name: "Crow's Foot Database Notation", category: SOFTWARE, masters: &crows_foot::MASTERS, default_open: false },
    StencilSpec { id: "chen", name: "Chen's Database Notation", category: SOFTWARE, masters: &chen::MASTERS, default_open: false },
];

/// The stencils among `all` that are shipped, in the order given.
fn shipped_among(all: &[StencilSpec]) -> Vec<StencilSpec> {
    all.iter().filter(|s| s.shipped()).copied().collect()
}

/// The ids among `all` of the stencils a new session has open: the shipped ones that are open by default, in the order given.
fn default_open_among(all: &[StencilSpec]) -> Vec<&'static str> {
    all.iter().filter(|s| s.shipped() && s.default_open).map(|s| s.id).collect()
}

/// Every registered stencil, shipped or not, in catalogue order: for the catalogue's own tests, which hold the registration to its rules before a family fills it.
#[cfg(test)]
pub(crate) fn registered() -> &'static [StencilSpec] {
    &ALL_STENCILS
}

/// The built-in stencils that are shipped, in the order the Shapes window lists them. **The one list a command, a query or a window reads**: the filter is made once.
pub fn stencils() -> &'static [StencilSpec] {
    static SHIPPED: LazyLock<Vec<StencilSpec>> = LazyLock::new(|| shipped_among(&ALL_STENCILS));
    SHIPPED.as_slice()
}

/// The ids of the stencils a new session has open, in catalogue order: the shipped ones that are open by default.
pub(crate) fn default_open_ids() -> Vec<&'static str> {
    default_open_among(&ALL_STENCILS)
}

/// Every master of every shipped stencil. A hidden master (the custom line ends) is in no stencil and so is not here ([`find_any`] finds it).
pub fn all_masters() -> impl Iterator<Item = &'static MasterSpec> {
    stencils().iter().flat_map(|s| s.masters.iter())
}

/// What the table of the built-in masters' `BaseID`s holds of each: the shipped stencil that holds the master (none for a hidden master, a line end), and the master.
type GuidEntry = (Option<&'static str>, &'static MasterSpec);

/// The built-in masters' `BaseID`s (`MasterSpec::guid`, in capitals), each with what [`GuidEntry`] says. One table, made once.
fn guid_table() -> &'static std::collections::HashMap<String, GuidEntry> {
    static TABLE: LazyLock<std::collections::HashMap<String, GuidEntry>> = LazyLock::new(|| {
        let mut table = std::collections::HashMap::new();
        for stencil in stencils() {
            for master in stencil.masters() {
                // The first stencil that holds a master names it, which is the stencil it is in when it is in one only.
                table.entry(master.guid().to_ascii_uppercase()).or_insert((Some(stencil.id), master));
            }
        }
        for master in line_ends::MASTERS.iter() {
            table.entry(master.guid().to_ascii_uppercase()).or_insert((None, master));
        }
        table
    });
    &TABLE
}

/// The id of the shipped built-in stencil that holds the master whose `BaseID` (or `UniqueID`, as 0.2.3 and the first builds of 0.2.4 wrote it: the same text) is `guid` (`MasterSpec::guid`,
/// compared whole, in any case, with white space round it), if one does: what a drawing's masters say about the stencils they came from. A master that is in no stencil
/// (the hidden line ends) is none's.
pub(crate) fn stencil_of_guid(guid: &str) -> Option<&'static str> {
    // No built-in guid is longer than 38 characters (`{D1AC0001-0000-4000-8000-000000000001}`): a longer text is none of them and is not copied.
    if guid.len() > 64 {
        return None;
    }
    guid_table().get(&guid.trim().to_ascii_uppercase()).and_then(|(stencil, _)| *stencil)
}

/// The built-in master whose `BaseID` is exactly `guid`, shipped or hidden: **the text as DiaCraft wrote it** (capitals, braces, no white space), and no other spelling of it. What the repair
/// of a master of the old form asks (`unique::repair_old_ids`), which acts on the text 0.2.3 wrote and on nothing that only looks like it.
pub(crate) fn builtin_of_guid(guid: &str) -> Option<&'static MasterSpec> {
    // `{D1AC0001-0000-4000-8000-000000000001}`: 38 characters, and the first of the form a built-in master has (a cheap look before the table).
    if guid.len() != 38 || !guid.starts_with("{D1AC") {
        return None;
    }
    let (_, spec) = guid_table().get(guid)?;
    (spec.guid() == guid).then_some(*spec)
}

/// Whether `guid` is the `BaseID` of a built-in master, shipped or hidden (compared as [`stencil_of_guid`] does): a master that holds one is a built-in master of some build, whatever `UniqueID`
/// that build gave it, and a copy of it in another drawing is the same master by it alone.
pub(crate) fn is_builtin_guid(guid: &str) -> bool {
    guid.len() <= 64 && guid_table().contains_key(&guid.trim().to_ascii_uppercase())
}

/// A built-in master by its exact id, shipped or hidden: what [`More::brings`] names.
fn find_any(id: &str) -> Option<&'static MasterSpec> {
    all_masters().chain(line_ends::MASTERS.iter()).find(|m| m.id == id)
}

/// What `stencil.list` answers about the built-in stencils: `{stencils: [{id, name, builtin: true, masters: [{id, name, width, height,
/// connection_points}]}]}`, the sizes in inches. The stencils a session has opened come after them: [`list_with`].
pub fn list() -> serde_json::Value {
    list_with(&OpenedStencils::new())
}

/// What `stencil.list` answers: the built-in stencils that are open (a new session has the seven of the first batches, a template names the ones it opens) and then the stencils the session
/// has opened from files, each as [`OpenedStencil::to_json`] has it (`builtin: false`), and `available`, the shipped built-in stencils that are not open (`{id, name, builtin: true, category,
/// masters_total}`), which `stencil.open {id}` opens. A stencil that is not shipped is in neither. Each built-in stencil names its `category`.
pub fn list_with(opened: &OpenedStencils) -> serde_json::Value {
    let open = opened.open_builtins();
    let mut stencils: Vec<serde_json::Value> = open
        .iter()
        .filter_map(|id| builtin_stencil(id))
        .map(|s| {
            let masters: Vec<serde_json::Value> = s.masters().iter().filter(|m| !m.is_hidden()).map(master_json).collect();
            serde_json::json!({ "id": s.id, "name": s.name, "builtin": true, "category": s.category, "masters": masters })
        })
        .collect();
    stencils.extend(opened.iter().map(|s| s.to_json()));
    let available: Vec<serde_json::Value> = stencils_not_in(open)
        .map(|s| serde_json::json!({ "id": s.id, "name": s.name, "builtin": true, "category": s.category, "masters_total": s.masters().len() }))
        .collect();
    serde_json::json!({ "stencils": stencils, "available": available })
}

/// A built-in master as `stencil.list` and `stencil.search` list it: `{id, name, width, height, connection_points}`; `one_d` for a connector or a one-dimensional figure, with `ends:
/// {begin, end, pattern}` (the line-end numbers and the pattern its cells resolve to: the window draws a tile from them); `part` for one whose drop is put in a structure (a Swimlane, a
/// Separator), and `autoconnect: false` for one that is no connector and that the AutoConnect strip never offers (a part, or a master of a stencil that does not feed the strip).
fn master_json(m: &MasterSpec) -> serde_json::Value {
    let mut value = serde_json::json!({ "id": m.id, "name": m.name, "width": m.width, "height": m.height, "connection_points": m.point_count() });
    if let Some(map) = value.as_object_mut() {
        if m.is_connector() {
            map.insert("one_d".into(), serde_json::json!(true));
        }
        if let Some(look) = m.look() {
            let (begin, end, pattern) = look.numbers();
            map.insert("ends".into(), serde_json::json!({ "begin": begin, "end": end, "pattern": pattern }));
        }
        if m.is_part() {
            map.insert("part".into(), serde_json::json!(true));
        }
        if !m.is_connector() && !m.on_strip() {
            map.insert("autoconnect".into(), serde_json::json!(false));
        }
    }
    value
}

/// The shipped built-in stencils whose id is not in `open`, in the order of the catalogue.
fn stencils_not_in<'a>(open: &'a [&'static str]) -> impl Iterator<Item = &'static StencilSpec> + 'a {
    stencils().iter().filter(move |s| !open.contains(&s.id))
}

impl MasterSpec {
    /// The connection points this master has.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// The `BaseID` of this master, which is also the `UniqueID` that 0.2.3 and the first builds of 0.2.4 wrote for it ([`MasterSpec::unique_id`] is the one written now): fixed, so a
    /// drawing that already holds the master is found again (a copy keeps its `BaseID`). Ours: no spec gives a form.
    /// `{D1AC<series as four hex digits>-0000-4000-8000-<ordinal as twelve>}`: Basic Shapes are `D1AC0001`, Basic
    /// Flowchart `D1AC0002`, each numbered from 1, and the second batch's stencils series 9 to 21 and the hidden line ends 22. Series 1 to 9 spell the text they had when the form was
    /// `D1AC000<series>`, so no saved drawing loses its masters (a series past 9 has more than one digit, which that form could not hold as a GUID).
    pub(crate) fn guid(&self) -> String {
        let (series, at) = self.identity;
        format!("{{D1AC{series:04X}-0000-4000-8000-{at:012X}}}")
    }

    /// Where this master's connection points are, from the lower left corner of its box at its default size: `(x, y)` in inches, in the order of the `Connection` section
    /// (`IX` 0 up). A shape dropped from the master has them there, whatever its place on the page (`shape.drop` gives it the master's size). What the window's drop
    /// preview passes the snap as the new shape's own places ([`crate::snap::dropped_points`]).
    pub fn connection_offsets(&self) -> Vec<(f64, f64)> {
        self.connection_offsets_at(self.width, self.height)
    }

    /// [`MasterSpec::connection_offsets`] for a box of `width` by `height`: the places that follow the size (shares of it) move with it, a fixed distance does not. A drop beyond the
    /// range of eight of the page's drawing scale has the master's size times the ratio.
    pub fn connection_offsets_at(&self, width: f64, height: f64) -> Vec<(f64, f64)> {
        self.points.iter().map(|p| (p.x.value(width, height), p.y.value(width, height))).collect()
    }

    /// Whether the master is one-dimensional: a connector, or a figure that is drawn along a line (`Fixed1D`). Its ends glue and it has a width and no height; the AutoConnect strip never
    /// offers it, and it needs no connection point of its own.
    pub fn is_connector(&self) -> bool {
        matches!(self.outline, Outline::Connector(_) | Outline::Fixed1D { .. })
    }

    /// Whether the master is a one-dimensional figure that is **not routed** (`Fixed1D`): its geometry is its own and a move of a shape it is glued to stretches it.
    pub fn is_fixed_1d(&self) -> bool {
        matches!(self.outline, Outline::Fixed1D { .. })
    }

    /// The line ends, pattern and route of a one-dimensional master.
    fn look(&self) -> Option<&ConnectorLook> {
        match &self.outline {
            Outline::Connector(look) | Outline::Fixed1D { look, .. } => Some(look),
            _ => None,
        }
    }

    /// Whether the window and the listing leave the master out (a hidden line end).
    pub fn is_hidden(&self) -> bool {
        self.more.hidden
    }

    /// The dress of the master, when it has one.
    fn dress(&self) -> Option<&'static Dress> {
        self.more.dress
    }

    /// Whether a drop of this master is put in a structure and is no shape of its outline: a Swimlane is inserted in a list of lanes, a Separator splits a phase (`xfunc.rs`), and BPMN's Pool
    /// and Lane are parts too (their dress says so).
    pub fn is_part(&self) -> bool {
        xfunc::is_part(self.id) || self.dress().is_some_and(|d| matches!(d.role, Role::Part(_)))
    }

    /// Whether the master is a container (the container role of its dress: a package, a subject): a shape dropped inside it joins it, and the AutoConnect strip passes over it.
    pub fn is_container(&self) -> bool {
        self.dress().is_some_and(|d| matches!(d.role, Role::Container { .. }))
    }

    /// Whether a drop of this master makes a structure of several shapes of its own (BPMN's Pool: a frame, a list of lanes and a lane) and not one shape of the master: the checks that drop a
    /// master and look at the one shape it makes pass it over, and its own tests look at the structure.
    pub fn makes_structure(&self) -> bool {
        self.part_kind() == Some(PartKind::Pool)
    }

    /// What part of a structure the master is, when its drop is put in one: BPMN's Pool and Lane (their dress says so). The Swimlane and the Separator of the Cross-Functional Flowchart stencil are
    /// parts by their ids (`xfunc::is_part`) and say none here.
    pub(crate) fn part_kind(&self) -> Option<PartKind> {
        match self.dress().map(|d| d.role) {
            Some(Role::Part(kind)) => Some(kind),
            _ => None,
        }
    }

    /// Whether the AutoConnect strip may offer this master: it is no connector, no part of a structure, no container, **no shape that no connector joins** (a title or a key: the strip adds
    /// a shape and joins it to the one it came from), and in a stencil that feeds the strip.
    pub fn on_strip(&self) -> bool {
        !self.is_connector()
            && !self.is_part()
            && !self.is_container()
            && !self.dress().is_some_and(|d| matches!(d.role, Role::Pointless))
            && stencils().iter().filter(|s| !s.feeds_strip()).all(|s| s.masters.iter().all(|m| m.id != self.id))
    }

    /// Where the text goes.
    fn text_box(&self) -> TextBox {
        self.outline.text_box()
    }

    /// Whether the master is a group of members (a class's compartments, a lifeline's head and stem): dropped as a group with its members listed (`group_master`).
    pub fn is_group(&self) -> bool {
        matches!(self.outline, Outline::Group { .. })
    }
}

/// The master a person or a program means by `key`, in any built-in stencil: its id or its name, in any case, with spaces, hyphens and
/// underscores alike. IDs are unique; repeated display names select the first (legacy) master.
pub fn find_master(key: &str) -> Option<&'static MasterSpec> {
    if key.len() > 64 {
        return None;
    }
    let simple = |text: &str| text.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let wanted = simple(key);
    all_masters().find(|m| simple(m.id) == wanted || simple(m.name) == wanted)
}

/// The shipped built-in stencil with this exact id (a stencil that is registered and has no masters is no stencil to any caller).
pub fn builtin_stencil(id: &str) -> Option<&'static StencilSpec> {
    stencils().iter().find(|s| s.id == id)
}

/// The shipped built-in stencil a person or a program means by `key`: its id, in any case, with white space round it. A text longer than any id is not looked at.
pub fn find_stencil(key: &str) -> Option<&'static StencilSpec> {
    if key.len() > 64 {
        return None;
    }
    let wanted = key.trim().to_ascii_lowercase();
    stencils().iter().find(|s| s.id == wanted)
}

/// The master a person or a program means by `key` in one built-in stencil: as [`find_master`], but only that stencil is searched.
pub fn find_master_in(stencil: &StencilSpec, key: &str) -> Option<&'static MasterSpec> {
    if key.len() > 64 {
        return None;
    }
    let simple = |text: &str| text.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let wanted = simple(key);
    stencil.masters.iter().find(|m| simple(m.id) == wanted || simple(m.name) == wanted)
}

/// A number as the text of a value: rounded to nine places, so that arithmetic noise (`0.30000000000000004`) is not kept. **A zero is `0`, whichever way it came**: a negative zero
/// (`-0.0`, or a number a hair below 0 that the rounding takes to it, which is what a point of a route carried into a connector's own frame and out again is) is no `-0` in a cell,
/// where it would be noise in a file, and the difference between two writes of one route that is none.
pub fn number_text(n: f64) -> String {
    let rounded = (n * 1.0e9).round() / 1.0e9;
    let rounded = if rounded.is_finite() && rounded != 0.0 { rounded } else { 0.0 };
    Value::from_number(rounded).map_or_else(|| "0".to_owned(), |v| v.text().to_owned())
}

/// A length constant, in inches.
fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
}

/// A plain number constant.
fn plain(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n)))
}

/// A length formula with the value it gives.
fn length_formula(formula: &str, n: f64) -> Cell {
    Cell::with_formula(formula, Some(Value::from_text(&number_text(n)))).with_unit("IN")
}

/// A share of `of` (`Width` or `Height`), as a cell: `Width*0.5` with its value at `size`, `Width` for 1, and the constant 0 for 0.
fn share(of: &str, f: f64, size: f64) -> Cell {
    if f == 0.0 {
        length(0.0)
    } else if f == 1.0 {
        length_formula(of, size)
    } else {
        length_formula(&format!("{of}*{}", number_text(f)), f * size)
    }
}

fn put(cells: &mut std::collections::BTreeMap<CellName, Cell>, name: &str, cell: Cell) {
    cells.insert(CellName::new(name), cell);
}

/// A row of a section keyed by `IX`.
fn row(ix: u32, row_type: Option<&str>, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: row_type.map(Arc::from), ..Row::default() };
    for (name, cell) in cells {
        put(&mut row.cells, name, cell);
    }
    Arc::new(row)
}

/// A row of a text section (`Character`, `Paragraph`) keyed by `IX`, holding `cells`; **a row past the first is whole**: it holds every cell
/// `diacraft_doc::root_text::row_cells` names for its section, those `cells` leaves out with the value the renderer assumes. A style holds row 0 alone, so a reader of the file takes nothing for a
/// cell such a row lacks from anywhere else, and reads it as 0: a `FontScale` of 0 piles the letters of a run on one another. Row 0 leans on the
/// styles, as the rows of files do.
fn text_row(section: &str, ix: u32, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut made = row(ix, None, cells);
    if ix >= 1 {
        diacraft_doc::root_text::complete_row_cells(section, &mut Arc::make_mut(&mut made).cells);
    }
    made
}

/// A `Geometry` section with its five flags: `NoFill` is 1 for a path that only strokes, and the others are 0.
fn flagged_section(ix: u32, fill: bool) -> Section {
    let mut section = Section::new("Geometry", Some(ix));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        put(&mut section.cells, flag, plain(if flag == "NoFill" && !fill { 1.0 } else { 0.0 }));
    }
    section
}

/// The row of an ellipse that fills the box: the centre, a point on the right and a point on top.
fn ellipse_row(ix: u32, w: f64, h: f64) -> Arc<Row> {
    row(
        ix,
        Some("Ellipse"),
        vec![
            ("X", share("Width", 0.5, w)),
            ("Y", share("Height", 0.5, h)),
            ("A", share("Width", 1.0, w)),
            ("B", share("Height", 0.5, h)),
            ("C", share("Width", 0.5, w)),
            ("D", share("Height", 1.0, h)),
        ],
    )
}

/// The ratio `D` of an elliptical arc whose semi-axes are `rx` along the box's x and `ry` along its y (`D` is the length of the
/// axis the angle `C` points along over the other's). A formula of the two, so that the arc stays a half-ellipse as the box is resized.
fn ratio_cell(rx: Co, ry: Co, w: f64, h: f64, of: Factor) -> Cell {
    let (a, b) = (rx.value(w, h), ry.value(w, h));
    let value = if b == 0.0 { 1.0 } else { a / b };
    // Two fixed lengths are in a ratio that the factor of the drop scale leaves as it is: a plain number.
    if rx.is_fixed() && ry.is_fixed() {
        return plain(value);
    }
    match (rx.formula(of), ry.formula(of)) {
        (Some(top), Some(bottom)) => Cell::with_formula(&format!("({top})/({bottom})"), Some(Value::from_text(&number_text(value)))),
        _ => plain(value),
    }
}

/// The row of a segment. The axis angle `C` of an arc is 0 (the box's x axis); its ratio `D` is 1 for a circle. A fixed length in it follows the drop scale through the factor `of` reads.
fn seg_row(ix: u32, seg: &Seg, w: f64, h: f64, of: Factor) -> Arc<Row> {
    let at = |x: &Co, y: &Co| [("X", x.cell(w, h, of)), ("Y", y.cell(w, h, of))];
    let arc = |kind: &str, to: &Xy, via: &Xy, ratio: Cell| {
        let [x, y] = at(&to.0, &to.1);
        row(ix, Some(kind), vec![x, y, ("A", via.0.cell(w, h, of)), ("B", via.1.cell(w, h, of)), ("C", plain(0.0)), ("D", ratio)])
    };
    match seg {
        Seg::Move(x, y) => row(ix, Some("MoveTo"), at(x, y).into()),
        Seg::Line(x, y) => row(ix, Some("LineTo"), at(x, y).into()),
        Seg::Arc { to, via } => arc("EllipticalArcTo", to, via, plain(1.0)),
        Seg::Half { to, via, rx, ry } => arc("EllipticalArcTo", to, via, ratio_cell(*rx, *ry, w, h, of)),
        Seg::Ellipse => ellipse_row(ix, w, h),
    }
}

/// One `Geometry` section from a path: `IX` `ix`, the flags it says, and its rows (at most `rows_left`, the rest of the master's [`MAX_ROWS`]). → the section.
fn path_section(ix: u32, path: (bool, &[Seg], &Flag, &Flag), (w, h): (f64, f64), of: Factor, rows_left: &mut usize) -> Section {
    let (fill, segs, no_show, no_line) = path;
    let mut section = flagged_section(ix, fill);
    if !matches!(no_show, Flag::Off) {
        put(&mut section.cells, "NoShow", no_show.cell());
    }
    if !matches!(no_line, Flag::Off) {
        put(&mut section.cells, "NoLine", no_line.cell());
    }
    for (i, seg) in segs.iter().enumerate() {
        if *rows_left == 0 {
            break;
        }
        *rows_left -= 1;
        section.rows.push(seg_row(u32::try_from(i + 1).unwrap_or(u32::MAX), seg, w, h, of));
    }
    section
}

/// The sections of paths in a list, `IX` 0 up, within the caps of one master.
fn path_sections(paths: &[(bool, &[Seg], &Flag, &Flag)], size: (f64, f64), of: Factor) -> Vec<Section> {
    let mut rows_left = MAX_ROWS;
    paths
        .iter()
        .take(MAX_SECTIONS)
        .enumerate()
        .map(|(n, path)| path_section(u32::try_from(n).unwrap_or(u32::MAX), *path, size, of, &mut rows_left))
        .collect()
}

/// The paths a `Paths` or `Fixed1D` outline holds, in the form [`path_sections`] takes.
fn spec_paths(paths: &'static [PathSpec]) -> Vec<(bool, &'static [Seg], &'static Flag, &'static Flag)> {
    paths.iter().map(|p| (p.fill, p.segs, &p.no_show, &p.no_line)).collect()
}

/// The `Geometry` sections of a master: `IX` 0 up, each one path, `NoFill` 0 for a path that takes the fill. A master made by code is held to [`MAX_SECTIONS`] sections and [`MAX_ROWS`] rows
/// in all (the catalogue test, [`caps_problem`], fails a master that goes past them, and a build never does).
fn geometry(spec: &MasterSpec) -> Vec<Section> {
    outline_sections(&spec.outline, (spec.width, spec.height), Factor::Shape)
}

/// The `Geometry` sections an outline draws in a box of `w` by `h`: a master's own, or a member's of a group master. A fixed length in one is written times the factor of the drop scale that `of`
/// says where to read: the shape's own, or the group's for a member.
fn outline_sections(outline: &Outline, (w, h): (f64, f64), of: Factor) -> Vec<Section> {
    match outline {
        Outline::Connector(_) => {
            let mut section = flagged_section(0, false);
            section.rows.push(seg_row(1, &Seg::Move(fw(0.0), fh(0.0)), w, h, of));
            section.rows.push(seg_row(2, &Seg::Line(fw(1.0), fh(0.0)), w, h, of));
            vec![section]
        }
        Outline::Ellipse => {
            // `Ellipse` is the section's only row.
            let mut section = flagged_section(0, true);
            section.rows.push(ellipse_row(1, w, h));
            vec![section]
        }
        Outline::Polygon(vertices) => {
            let mut section = flagged_section(0, true);
            // The path comes back to its first vertex: no row closes a path, the end meets the start.
            let closed = vertices.iter().chain(vertices.first());
            for (i, (fx, fy)) in closed.enumerate() {
                let kind = if i == 0 { "MoveTo" } else { "LineTo" };
                let ix = u32::try_from(i + 1).unwrap_or(u32::MAX);
                section.rows.push(row(ix, Some(kind), vec![("X", share("Width", *fx, w)), ("Y", share("Height", *fy, h))]));
            }
            vec![section]
        }
        Outline::Paths { paths, .. } | Outline::Fixed1D { paths, .. } => path_sections(&spec_paths(paths), (w, h), of),
        Outline::Code { build, .. } => {
            let built = build();
            let paths: Vec<(bool, &[Seg], &Flag, &Flag)> = built.iter().map(|p| (p.fill, p.segs.as_slice(), &p.no_show, &p.no_line)).collect();
            path_sections(&paths, (w, h), of)
        }
        // A group has no geometry of its own: its members draw (`group_master`).
        Outline::Group { .. } => Vec::new(),
    }
}

/// How many sections and rows an outline would have, before the caps cut them.
fn outline_size(outline: &Outline) -> (usize, usize) {
    match outline {
        Outline::Connector(_) => (1, 2),
        Outline::Ellipse => (1, 1),
        Outline::Polygon(vertices) => (1, vertices.len().saturating_add(1)),
        Outline::Paths { paths, .. } | Outline::Fixed1D { paths, .. } => {
            (paths.len(), paths.iter().map(|p| p.segs.len()).fold(0, usize::saturating_add))
        }
        Outline::Code { build, .. } => {
            let built = build();
            (built.len(), built.iter().map(|p| p.segs.len()).fold(0, usize::saturating_add))
        }
        Outline::Group { .. } => (0, 0),
    }
}

/// How many sections, rows and connection points the master would have, before the caps cut them: `(sections, rows, points)`. **A group master's are its members' together** (and its own points).
#[allow(dead_code, reason = "read by the catalogue's tests")]
fn size_of(spec: &MasterSpec) -> (usize, usize, usize) {
    let (sections, rows) = outline_size(&spec.outline);
    match &spec.outline {
        Outline::Group { members, .. } => {
            let (member_sections, member_rows, member_points) = group_master::size_of_members(members, outline_size);
            (sections.saturating_add(member_sections), rows.saturating_add(member_rows), spec.points.len().saturating_add(member_points))
        }
        _ => (sections, rows, spec.points.len()),
    }
}

/// What is wrong with the master's size, in words: more than [`MAX_SECTIONS`] `Geometry` sections, [`MAX_ROWS`] rows or [`MAX_POINTS`] connection points. The catalogue's test calls it for
/// every master, so a master made by code that grows past what the engine reads is found by a test and not by a drawing.
#[allow(dead_code, reason = "read by the catalogue's tests")]
pub(crate) fn caps_problem(spec: &MasterSpec) -> Option<String> {
    let (sections, rows, points) = size_of(spec);
    let mut said = Vec::new();
    for (count, most, what) in [(sections, MAX_SECTIONS, "Geometry sections"), (rows, MAX_ROWS, "rows"), (points, MAX_POINTS, "connection points")] {
        if count > most {
            said.push(format!("{count} {what}, past {most}"));
        }
    }
    if let Outline::Group { members, .. } = &spec.outline
        && members.len() > group_master::MAX_MEMBERS
    {
        said.push(format!("{} members, past {}", members.len(), group_master::MAX_MEMBERS));
    }
    (!said.is_empty()).then(|| format!("{}: {}", spec.id, said.join(", ")))
}

/// The `Connection` section: a row for each point, `IX` 0 up, at most [`MAX_POINTS`].
fn connections(spec: &MasterSpec) -> Section {
    connection_section(spec.points, (spec.width, spec.height), Factor::Shape)
}

/// The `Connection` section of `points` in a box of `w` by `h`; a fixed distance in one follows the drop scale by the factor `of` reads.
fn connection_section(points: &[Point], (w, h): (f64, f64), of: Factor) -> Section {
    let mut section = Section::new("Connection", None);
    for (i, p) in points.iter().take(MAX_POINTS).enumerate() {
        let ix = u32::try_from(i).unwrap_or(u32::MAX);
        section.rows.push(row(
            ix,
            None,
            vec![("X", p.x.cell(w, h, of)), ("Y", p.y.cell(w, h, of)), ("DirX", plain(p.dx)), ("DirY", plain(p.dy)), ("Type", plain(0.0))],
        ));
    }
    section
}

/// The one shape of a master.
fn master_shape(spec: &MasterSpec) -> Shape {
    let (w, h) = (spec.width, spec.height);
    // Linked to the root style, as the shapes of real files are, so that the look is read from it (a shape with no link reads none).
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    // The master's shape sits with its box at the origin of the master (our choice).
    put(cells, "PinX", length_formula("Width*0.5", w * 0.5));
    put(cells, "PinY", length_formula("Height*0.5", h * 0.5));
    put(cells, "Width", length(w));
    put(cells, "Height", length(h));
    // The formulas written on creation.
    put(cells, "LocPinX", length_formula("Width*0.5", w * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", h * 0.5));
    // A line end is drawn pointing right with its pin on its tip.
    line_ends::pin_at_tip(spec, cells, w, h);
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    // The text block is the shape, or the part of it the master names.
    let text = spec.text_box();
    let (centre_x, centre_y) = (text.x + text.w / 2.0, text.y + text.h / 2.0);
    put(cells, "TxtPinX", length_formula(&format!("Width*{}", number_text(centre_x)), centre_x * w));
    put(cells, "TxtPinY", length_formula(&format!("Height*{}", number_text(centre_y)), centre_y * h));
    put(cells, "TxtLocPinX", length_formula("TxtWidth*0.5", text.w * w * 0.5));
    put(cells, "TxtLocPinY", length_formula("TxtHeight*0.5", text.h * h * 0.5));
    put(cells, "TxtWidth", length_formula(&format!("Width*{}", number_text(text.w)), text.w * w));
    put(cells, "TxtHeight", length_formula(&format!("Height*{}", number_text(text.h)), text.h * h));
    put(cells, "TxtAngle", plain(0.0));
    if let Some(look) = spec.look() {
        for (name, value) in [("BeginX", 0.0), ("BeginY", 0.0), ("EndX", w), ("EndY", 0.0)] {
            put(cells, name, length(value));
        }
        // The line ends, the pattern and the route are the master's: an instance inherits them.
        for (name, cell) in look.cells() {
            put(cells, name, cell);
        }
        if spec.is_fixed_1d() {
            // Route style 16 (centre to centre) keeps its geometry: a glued end that moves is followed, and no route is laid over the figure.
            put(cells, "ShapeRouteStyle", plain(16.0));
        }
    }
    sheet.sections.extend(geometry(spec).into_iter().map(Arc::new));
    sheet.sections.push(Arc::new(connections(spec)));
    // A group of compartments is as tall as they are, and holds what that needs; a dress says otherwise below (`group_master`).
    if let Outline::Group { members, .. } = &spec.outline {
        group_master::group_cells(members, &mut sheet);
    }
    // A position of the Organization Chart Shapes also holds its look, its Shape Data, its marker and the text that shows two of the fields. Any other master's dress does the same by data.
    let text = if orgchart::is_position(spec) {
        Some(Arc::new(orgchart::decorate(spec, &mut sheet)))
    } else {
        spec.dress().and_then(|dress| dress::decorate(dress, &mut sheet, Factor::Shape)).map(Arc::new)
    };
    let (kind, children) = match &spec.outline {
        Outline::Group { members, .. } => (ShapeType::Group, ShapeList::from_vec(group_master::build_members(spec, members))),
        _ => (ShapeType::Shape, ShapeList::default()),
    };
    let mut shape = Shape {
        id: MASTER_SHAPE_ID,
        name: Some(Arc::from(spec.name)),
        name_u: Some(Arc::from(spec.name)),
        kind,
        sheet: Arc::new(sheet),
        text,
        children,
        ..Shape::new(MASTER_SHAPE_ID)
    };
    // A text a cell of the master holds says `STR` (the unit of a string): the master's cells are the ones an instance inherits, and a program that reads the `U`
    // takes a text with none for a number it cannot read. The tables say it where they make a cell, and this is what no table forgot.
    shape.fit_string_units();
    shape
}

/// Whether a sheet holds a formula that has no value yet: a cell a dress or a flag gave as a formula alone.
fn has_unsettled(sheet: &Sheet) -> bool {
    let bare = |cell: &Cell| matches!(cell.formula, diacraft_doc::FormulaSlot::Local(_)) && cell.value.is_none();
    sheet.cells.values().any(bare) || sheet.sections.iter().any(|s| s.cells.values().any(bare) || s.rows.iter().any(|r| r.cells.values().any(bare)))
}

/// Whether a shape or any member of it holds a formula that has no value yet (a group master's members do).
fn shape_has_unsettled(shape: &Shape) -> bool {
    has_unsettled(&shape.sheet) || shape.children.iter().any(|member| shape_has_unsettled(member))
}

/// A master whose cells that hold a formula and no value are worked out: the master is calculated alone, in a document of its own, so that **a cached value is what its formula gives at the
/// master's default Shape Data and size**, and a table never repeats a number a formula computes. A master that cannot be settled (the budget is spent) is returned as it was.
fn settle_values(master: Master) -> Master {
    let id = master.id;
    let mut scratch = Document::new_blank();
    if scratch.add_master(master.clone()).is_err() {
        return master;
    }
    let report = diacraft_calc::commit(&mut scratch, &diacraft_calc::Budget::default(), diacraft_calc::EvalOptions::default());
    if report.budget_exhausted {
        return master;
    }
    match scratch.master(id) {
        Some(settled) => (**settled).clone(),
        None => master,
    }
}

/// The master `spec` describes, as it is written into a document under `id`.
fn build_master(spec: &MasterSpec, id: u32) -> Master {
    let mut page_sheet = Sheet::default();
    for (name, value) in [("PageWidth", spec.width), ("PageHeight", spec.height)] {
        put(&mut page_sheet.cells, name, length(value));
    }
    for name in ["PageScale", "DrawingScale"] {
        put(&mut page_sheet.cells, name, length(1.0));
    }
    let mut raw = diacraft_doc::RawAttrs::new();
    raw.push("BaseID", &spec.guid());
    if spec.is_hidden() {
        raw.push("Hidden", "1");
    }
    // The `UniqueID` is made last, from what the master holds (`unique.rs`): the `BaseID` is the fixed identity and the `UniqueID` tells the master apart from every other in the first three groups.
    let mut master = Master {
        id,
        name: Some(Arc::from(spec.name)),
        name_u: Some(Arc::from(spec.name)),
        pattern_flags: Some(line_ends::pattern_flags(spec)),
        sheet: Arc::new(page_sheet),
        raw,
        ..Master::default()
    };
    master.shapes.push(Arc::new(master_shape(spec)));
    if master.shapes.iter().any(|shape| shape_has_unsettled(shape)) {
        master = settle_values(master);
    }
    master.unique_id = Some(Arc::from(unique::unique_id_of(spec, &master).as_str()));
    master
}

/// The text a shape dropped from `spec` holds when the master's text has fields (the shown form of a field lives in the text that holds it): `None` for any other master.
pub(crate) fn dropped_text(spec: &MasterSpec) -> Option<diacraft_doc::Text> {
    spec.dress().and_then(dress::dropped_text)
}

/// The most masters a master brings in a chain (a master that brings another that brings another): a chain past it is left as far as it went.
const MAX_BRING_DEPTH: usize = 4;

/// The `ID` of the master `spec` is in `doc`, copying the master in when the document has none of it. Whether it has is decided by the one
/// matcher ([`find_same_master`]): a built-in master is known by its fixed `BaseID`, and by a `UniqueID` of the same text, which is what 0.2.3 and the first builds of 0.2.4 wrote
/// for it (the `UniqueID` of a master this build makes is another, `unique.rs`: a drawing of either kind is found by the `BaseID`). A master of the document that has that `UniqueID` and another
/// `BaseID` is another master, and the copy keeps its own `UniqueID` unless the first three groups of it are another master's (`bring::unique_id_for`).
/// **The masters it brings** (`More::brings`: a connector's line end) are copied in first, each by the same rule.
pub fn ensure_master(doc: &mut Document, spec: &MasterSpec) -> Result<u32, DocError> {
    ensure_master_within(doc, spec, MAX_BRING_DEPTH)
}

fn ensure_master_within(doc: &mut Document, spec: &MasterSpec, depth: usize) -> Result<u32, DocError> {
    // The probe names the master by the text it had in drawings of earlier builds (a `UniqueID` that is the `BaseID`'s text) and by its `BaseID`: the one a build now makes is no key, which a
    // drawing of an earlier build does not hold and which costs the master built to make.
    let guid = spec.guid();
    let mut probe = Master { unique_id: Some(Arc::from(guid.as_str())), ..Master::default() };
    probe.raw.push("BaseID", &guid);
    if let Some(found) = find_same_master(doc, &probe, Identity { unique_is_unique: true, base_is_unique: true }) {
        return Ok(found);
    }
    if depth > 0 {
        for id in spec.more.brings {
            if let Some(brought) = find_any(id).filter(|m| !std::ptr::eq(*m, spec)) {
                ensure_master_within(doc, brought, depth - 1)?;
            }
        }
    }
    let id = doc.masters.iter().map(|m| m.id).max().map_or(Some(1), |highest| highest.checked_add(1)).ok_or(DocError::IdsExhausted)?;
    let mut master = build_master(spec, id);
    master.unique_id = bring::unique_id_for(doc, master.unique_id.as_deref());
    doc.add_master(master)?;
    Ok(id)
}

#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod catalogue_tests;
#[cfg(test)]
mod flowchart_tests;
#[cfg(test)]
mod group_master_tests;
#[cfg(test)]
mod tests;
