//! Five built-in stencils, fifty original masters. New figures follow.
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

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, DocError, Document, Links, Master, Row, Section, Shape, ShapeType, Sheet, Value};

mod arrows;
mod bring;
mod callout;
pub(crate) mod container;
mod decorative;
mod drawn;
mod flowchart;
mod gane_sarson;
mod icon;
mod instance;
mod keywords;
mod opened;
pub(crate) mod orgchart;
mod picture;
pub mod quick;
pub(crate) mod refs;
mod search;
pub(crate) mod swimlane;
mod text_box;
pub mod xfunc;

pub(crate) use bring::left_out_words;
pub use bring::{Brought, Identity, MAX_BRING_NODES, Refusal, bring_master, find_same_master};
pub use callout::{CALLOUT_HEIGHT, CALLOUT_TEXT, CALLOUT_WIDTH, CalloutStyle, STYLES as CALLOUT_STYLES, find_style as find_callout_style};
pub(crate) use callout::{CalloutFrame, callout_shape, free_tip_cells, tied_tip_cells};
pub(crate) use drawn::{DrawnFrame, drawn_shape};
pub use icon::{IconFormat, IconInfo, IconProblem, MAX_ICON_BYTES, decode as decode_icon, inspect as inspect_icon};
pub use instance::{MAX_LISTED_MEMBERS, OpenedDrop, drop_opened};
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

/// An outline or connection coordinate: shares of width, height and their minimum, written as a formula.
#[derive(Clone, Copy)]
struct Co {
    w: f64,
    h: f64,
    min: f64,
}

/// A share `w` of `Width`.
const fn fw(w: f64) -> Co {
    Co { w, h: 0.0, min: 0.0 }
}

/// A share `h` of `Height`.
const fn fh(h: f64) -> Co {
    Co { w: 0.0, h, min: 0.0 }
}

/// `Width*w + Height*h`.
const fn mix(w: f64, h: f64) -> Co {
    Co { w, h, min: 0.0 }
}

impl Co {
    /// The value at a box of `width` by `height`.
    fn value(self, width: f64, height: f64) -> f64 {
        self.w * width + self.h * height + self.min * width.min(height)
    }

    /// The formula, or `None` for 0, which is written as a plain constant. A share of 1 is the bare name: `Width`, `Width-Height*0.5`.
    fn formula(self) -> Option<String> {
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
        (!text.is_empty()).then_some(text)
    }

    /// The cell: a length formula with the value it gives at the default size, or the constant 0.
    fn cell(self, width: f64, height: f64) -> Cell {
        self.formula().map_or_else(|| length(0.0), |f| length_formula(&f, self.value(width, height)))
    }
}

/// A point of an outline.
type Xy = (Co, Co);

/// A connection point of a master: where it sits, and the way a connector leaves it.
struct Point {
    x: Co,
    y: Co,
    dx: f64,
    dy: f64,
}

/// One piece of a path. The path starts with a `Move`, and a filled one returns to where it started.
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
enum Outline {
    /// A real one-dimensional directed connector, constructed by the existing connector path.
    Connector,
    /// Vertices as shares of the box, in order; the path returns to the first.
    Polygon(&'static [(f64, f64)]),
    /// The ellipse that fills the box.
    Ellipse,
    /// One or more paths, one `Geometry` section each, and where the text goes.
    Paths { paths: &'static [PathSpec], text: TextBox },
}

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
}

/// A stencil: a named list of masters.
pub struct StencilSpec {
    /// A lowercase word: `flowchart`.
    pub id: &'static str,
    /// As the Shapes window shows it.
    pub name: &'static str,
    /// What the More Shapes menu files it under (ours): the words of the template categories (`General`, `Flowchart`) and `Software and Database`.
    pub category: &'static str,
    masters: &'static [MasterSpec],
}

impl StencilSpec {
    /// The masters, in the order the Shapes window lists them.
    pub fn masters(&self) -> &'static [MasterSpec] {
        self.masters
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

/// The four middles of the sides of a box, top first and then anticlockwise.
const SIDES: [Point; 4] = [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.5, 1.0, 0.0)];

/// Vertices of a regular pentagon fitted to its box: the golden ratio gives 0.809, 0.191 and 0.618.
const PENTAGON: [(f64, f64); 5] = [(0.5, 1.0), (0.0, 0.618), (0.191, 0.0), (0.809, 0.0), (1.0, 0.618)];
/// The corner cut of a regular octagon in a square: 1 / (2 + sqrt 2).
const CUT: f64 = 0.2929;

static MASTERS: [MasterSpec; 12] = [
    MasterSpec {
        identity: (1, 1),
        id: "rectangle",
        name: "Rectangle",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 2),
        id: "square",
        name: "Square",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        points: &SIDES,
    },
    MasterSpec { identity: (1, 3), id: "ellipse", name: "Ellipse", width: 1.5, height: 1.0, outline: Outline::Ellipse, points: &SIDES },
    MasterSpec { identity: (1, 4), id: "circle", name: "Circle", width: 1.0, height: 1.0, outline: Outline::Ellipse, points: &SIDES },
    MasterSpec {
        identity: (1, 5),
        id: "triangle",
        name: "Triangle",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)]),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.25, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.75, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 6),
        id: "right_triangle",
        name: "Right Triangle",
        width: 1.0,
        height: 1.0,
        outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        points: &[pt(0.0, 1.0, 0.0, 1.0), pt(0.0, 0.5, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(0.5, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 7),
        id: "diamond",
        name: "Diamond",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 8),
        id: "pentagon",
        name: "Pentagon",
        width: 1.2,
        height: 1.14,
        outline: Outline::Polygon(&PENTAGON),
        points: &[pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.618, -1.0, 0.0), pt(0.5, 0.0, 0.0, -1.0), pt(1.0, 0.618, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 9),
        id: "hexagon",
        name: "Hexagon",
        width: 1.4,
        height: 1.2,
        outline: Outline::Polygon(&[(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)]),
        points: &SIDES,
    },
    MasterSpec {
        identity: (1, 10),
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
        id: "parallelogram",
        name: "Parallelogram",
        width: 1.5,
        height: 1.0,
        outline: Outline::Polygon(&[(0.25, 0.0), (1.0, 0.0), (0.75, 1.0), (0.0, 1.0)]),
        points: &[pt(0.375, 1.0, 0.0, 1.0), pt(0.125, 0.5, -1.0, 0.0), pt(0.625, 0.0, 0.0, -1.0), pt(0.875, 0.5, 1.0, 0.0)],
    },
    MasterSpec {
        identity: (1, 12),
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

/// The built-in stencils, in the order the Shapes window lists them: the two original libraries followed by the three libraries, the Cross-Functional Flowchart's and the Organization Chart Shapes.
static STENCILS: [StencilSpec; 7] = [
    StencilSpec { id: STENCIL_ID, name: STENCIL_NAME, category: "General", masters: &MASTERS },
    StencilSpec { id: FLOWCHART_ID, name: FLOWCHART_NAME, category: "Flowchart", masters: &flowchart::FLOWCHART },
    StencilSpec { id: "decorative", name: "Decorative Shapes", category: "General", masters: &decorative::MASTERS },
    StencilSpec { id: ARROWS_ID, name: "Arrow Shapes", category: "General", masters: &arrows::MASTERS },
    StencilSpec { id: "gane-sarson", name: "Gane-Sarson", category: "Software and Database", masters: &gane_sarson::MASTERS },
    StencilSpec { id: xfunc::ID, name: "Cross-Functional Flowchart Shapes", category: "Flowchart", masters: &xfunc::MASTERS },
    StencilSpec { id: orgchart::ID, name: orgchart::NAME, category: "Business", masters: &orgchart::MASTERS },
];

/// The built-in stencils.
pub fn stencils() -> &'static [StencilSpec] {
    &STENCILS
}

/// Every master of every built-in stencil.
pub fn all_masters() -> impl Iterator<Item = &'static MasterSpec> {
    STENCILS.iter().flat_map(|s| s.masters.iter())
}

/// What `stencil.list` answers about the built-in stencils: `{stencils: [{id, name, builtin: true, masters: [{id, name, width, height,
/// connection_points}]}]}`, the sizes in inches. The stencils a session has opened come after them: [`list_with`].
pub fn list() -> serde_json::Value {
    list_with(&OpenedStencils::new())
}

/// What `stencil.list` answers: the built-in stencils that are open (by default every built-in; a template names the ones it
/// opens) and then the stencils the session has opened from files, each as [`OpenedStencil::to_json`] has it (`builtin: false`), and `available`,
/// the built-in stencils that are not open (`{id, name, builtin: true, category, masters_total}`), which `stencil.open {id}` opens. Each built-in stencil names its `category`.
pub fn list_with(opened: &OpenedStencils) -> serde_json::Value {
    let open = opened.open_builtins();
    let mut stencils: Vec<serde_json::Value> = open
        .iter()
        .filter_map(|id| builtin_stencil(id))
        .map(|s| {
            let masters: Vec<serde_json::Value> = s.masters().iter().map(master_json).collect();
            serde_json::json!({ "id": s.id, "name": s.name, "builtin": true, "category": s.category, "masters": masters })
        })
        .collect();
    stencils.extend(opened.iter().map(|s| s.to_json()));
    let available: Vec<serde_json::Value> = stencils_not_in(open)
        .map(|s| serde_json::json!({ "id": s.id, "name": s.name, "builtin": true, "category": s.category, "masters_total": s.masters().len() }))
        .collect();
    serde_json::json!({ "stencils": stencils, "available": available })
}

/// A built-in master as `stencil.list` and `stencil.search` list it: `{id, name, width, height, connection_points}` and `one_d` for a connector.
/// A master as `stencil.list` lists it: `one_d` for a connector, `part` for one whose drop is put in a structure (a Swimlane, a Separator), and `autoconnect: false` for one that is no
/// connector and that the AutoConnect strip never offers (a part, or a master of a stencil that does not feed the strip).
fn master_json(m: &MasterSpec) -> serde_json::Value {
    let mut value = serde_json::json!({ "id": m.id, "name": m.name, "width": m.width, "height": m.height, "connection_points": m.point_count() });
    if let Some(map) = value.as_object_mut() {
        if m.is_connector() {
            map.insert("one_d".into(), serde_json::json!(true));
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

/// The built-in stencils whose id is not in `open`, in the order of the catalogue.
fn stencils_not_in<'a>(open: &'a [&'static str]) -> impl Iterator<Item = &'static StencilSpec> + 'a {
    STENCILS.iter().filter(move |s| !open.contains(&s.id))
}

impl MasterSpec {
    /// The connection points this master has.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// The `UniqueID` and `BaseID` of this master: fixed, so a drawing that already holds the master is found again (a copy keeps its `BaseID`).
    /// Ours: no spec gives a form. Basic Shapes are `D1AC0001`, Basic Flowchart `D1AC0002`, each numbered from 1.
    pub(crate) fn guid(&self) -> String {
        let (series, at) = self.identity;
        format!("{{D1AC000{series}-0000-4000-8000-{at:012X}}}")
    }

    /// Where this master's connection points are, from the lower left corner of its box at its default size: `(x, y)` in inches, in the order of the `Connection` section
    /// (`IX` 0 up). A shape dropped from the master has them there, whatever its place on the page (`shape.drop` gives it the master's size). What the window's drop
    /// preview passes the snap as the new shape's own places ([`crate::snap::dropped_points`]).
    pub fn connection_offsets(&self) -> Vec<(f64, f64)> {
        self.points.iter().map(|p| (p.x.value(self.width, self.height), p.y.value(self.width, self.height))).collect()
    }

    /// Whether placement uses the existing one-dimensional connector machinery.
    pub fn is_connector(&self) -> bool {
        matches!(self.outline, Outline::Connector)
    }

    /// Whether a drop of this master is put in a structure and is no shape of its outline: a Swimlane is inserted in a list of lanes, a Separator splits a phase (`xfunc.rs`).
    pub fn is_part(&self) -> bool {
        xfunc::is_part(self.id)
    }

    /// Whether the AutoConnect strip may offer this master: it is no connector, no part of a structure, and in a stencil that feeds the strip.
    pub fn on_strip(&self) -> bool {
        !self.is_connector() && !self.is_part() && STENCILS.iter().filter(|s| !s.feeds_strip()).all(|s| s.masters.iter().all(|m| m.id != self.id))
    }

    /// Where the text goes.
    fn text_box(&self) -> TextBox {
        match &self.outline {
            Outline::Paths { text, .. } => *text,
            Outline::Polygon(_) | Outline::Ellipse | Outline::Connector => TextBox::FULL,
        }
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

/// The built-in stencil with this exact id.
pub fn builtin_stencil(id: &str) -> Option<&'static StencilSpec> {
    STENCILS.iter().find(|s| s.id == id)
}

/// The built-in stencil a person or a program means by `key`: its id, in any case, with white space round it. A text longer than any id is not looked at.
pub fn find_stencil(key: &str) -> Option<&'static StencilSpec> {
    if key.len() > 64 {
        return None;
    }
    let wanted = key.trim().to_ascii_lowercase();
    STENCILS.iter().find(|s| s.id == wanted)
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

/// A number as the text of a value: rounded to nine places, so that arithmetic noise (`0.30000000000000004`) is not kept.
pub fn number_text(n: f64) -> String {
    let rounded = (n * 1.0e9).round() / 1.0e9;
    Value::from_number(if rounded.is_finite() { rounded } else { 0.0 }).map_or_else(|| "0".to_owned(), |v| v.text().to_owned())
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
fn ratio_cell(rx: Co, ry: Co, w: f64, h: f64) -> Cell {
    let (a, b) = (rx.value(w, h), ry.value(w, h));
    let value = if b == 0.0 { 1.0 } else { a / b };
    match (rx.formula(), ry.formula()) {
        (Some(top), Some(bottom)) => Cell::with_formula(&format!("({top})/({bottom})"), Some(Value::from_text(&number_text(value)))),
        _ => plain(value),
    }
}

/// The row of a segment. The axis angle `C` of an arc is 0 (the box's x axis); its ratio `D` is 1 for a circle.
fn seg_row(ix: u32, seg: &Seg, w: f64, h: f64) -> Arc<Row> {
    let at = |x: &Co, y: &Co| [("X", x.cell(w, h)), ("Y", y.cell(w, h))];
    let arc = |kind: &str, to: &Xy, via: &Xy, ratio: Cell| {
        let [x, y] = at(&to.0, &to.1);
        row(ix, Some(kind), vec![x, y, ("A", via.0.cell(w, h)), ("B", via.1.cell(w, h)), ("C", plain(0.0)), ("D", ratio)])
    };
    match seg {
        Seg::Move(x, y) => row(ix, Some("MoveTo"), at(x, y).into()),
        Seg::Line(x, y) => row(ix, Some("LineTo"), at(x, y).into()),
        Seg::Arc { to, via } => arc("EllipticalArcTo", to, via, plain(1.0)),
        Seg::Half { to, via, rx, ry } => arc("EllipticalArcTo", to, via, ratio_cell(*rx, *ry, w, h)),
        Seg::Ellipse => ellipse_row(ix, w, h),
    }
}

/// The `Geometry` sections of a master: `IX` 0 up, each one path, `NoFill` 0 for a path that takes the fill.
fn geometry(spec: &MasterSpec) -> Vec<Section> {
    let (w, h) = (spec.width, spec.height);
    match &spec.outline {
        Outline::Connector => {
            let mut section = flagged_section(0, false);
            section.rows.push(seg_row(1, &Seg::Move(fw(0.0), fh(0.0)), w, h));
            section.rows.push(seg_row(2, &Seg::Line(fw(1.0), fh(0.0)), w, h));
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
        Outline::Paths { paths, .. } => paths
            .iter()
            .enumerate()
            .map(|(n, path)| {
                let mut section = flagged_section(u32::try_from(n).unwrap_or(u32::MAX), path.fill);
                for (i, seg) in path.segs.iter().enumerate() {
                    section.rows.push(seg_row(u32::try_from(i + 1).unwrap_or(u32::MAX), seg, w, h));
                }
                section
            })
            .collect(),
    }
}

/// The `Connection` section: a row for each point, `IX` 0 up.
fn connections(spec: &MasterSpec) -> Section {
    let mut section = Section::new("Connection", None);
    for (i, p) in spec.points.iter().enumerate() {
        let ix = u32::try_from(i).unwrap_or(u32::MAX);
        section.rows.push(row(
            ix,
            None,
            vec![
                ("X", p.x.cell(spec.width, spec.height)),
                ("Y", p.y.cell(spec.width, spec.height)),
                ("DirX", plain(p.dx)),
                ("DirY", plain(p.dy)),
                ("Type", plain(0.0)),
            ],
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
    if spec.is_connector() {
        for (name, value) in [("BeginX", 0.0), ("BeginY", 0.0), ("EndX", w), ("EndY", 0.0)] {
            put(cells, name, length(value));
        }
        for (name, value) in [("BeginArrow", 0.0), ("EndArrow", 4.0), ("FillPattern", 0.0)] {
            put(cells, name, plain(value));
        }
    }
    sheet.sections.extend(geometry(spec).into_iter().map(Arc::new));
    sheet.sections.push(Arc::new(connections(spec)));
    // A position of the Organization Chart Shapes also holds its look, its Shape Data, its marker and the text that shows two of the fields.
    let text = orgchart::is_position(spec).then(|| Arc::new(orgchart::decorate(spec, &mut sheet)));
    Shape {
        id: MASTER_SHAPE_ID,
        name: Some(Arc::from(spec.name)),
        name_u: Some(Arc::from(spec.name)),
        kind: ShapeType::Shape,
        sheet: Arc::new(sheet),
        text,
        ..Shape::new(MASTER_SHAPE_ID)
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
    let guid = spec.guid();
    let mut raw = diacraft_doc::RawAttrs::new();
    raw.push("BaseID", &guid);
    let mut master = Master {
        id,
        name: Some(Arc::from(spec.name)),
        name_u: Some(Arc::from(spec.name)),
        unique_id: Some(Arc::from(guid.as_str())),
        pattern_flags: Some(0),
        sheet: Arc::new(page_sheet),
        raw,
        ..Master::default()
    };
    master.shapes.push(Arc::new(master_shape(spec)));
    master
}

/// The `ID` of the master `spec` is in `doc`, copying the master in when the document has none of it. Whether it has is decided by the one
/// matcher ([`find_same_master`]): a built-in master's `UniqueID` and `BaseID` are the same fixed GUID, and a master of the
/// document that has that `UniqueID` and another `BaseID` is another master, so the copy gets a `UniqueID` of its own then.
pub fn ensure_master(doc: &mut Document, spec: &MasterSpec) -> Result<u32, DocError> {
    let guid = spec.guid();
    let mut probe = Master { unique_id: Some(Arc::from(guid.as_str())), ..Master::default() };
    probe.raw.push("BaseID", &guid);
    if let Some(found) = find_same_master(doc, &probe, Identity { unique_is_unique: true, base_is_unique: true }) {
        return Ok(found);
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
mod flowchart_tests;
#[cfg(test)]
mod tests;
