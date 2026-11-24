//! The shapes of the callout gallery (in `docs/editing-commands.md`).
//!
//! The specifications give the gallery's look in words (fourteen tiles: text with a line, brackets, speech boxes, a cloud, an oval and a leader line, each showing the word Text) and what a callout does
//! And no cell of one. So the shapes are **ours**, drawn in code from standard figures, and each is a **shape with no master**, as a text box is (`text_box.rs` says why):
//!
//! * *The frame.* `PinX`, `PinY`, `Width` and `Height` are the box the text sits in; `LocPinX` and `LocPinY` are `Width*0.5` and `Height*0.5`; `Angle`, `FlipX` and `FlipY` are 0, and the text block is
//!   the whole box. The shape is linked to the root style, so its fill, line and text come from the page's theme ("colour comes from the drawing's theme").
//! * *The leader's end.* `Scratch` row `IX` 0 holds it in the shape's own frame: `X` and `Y` are the tip, and `A`, `B` and `C` the working that finds it from a shape the callout is tied to. A callout
//!   that is **not tied** holds numbers in all five, and the tip stays where it was put as the callout moves. A callout that **is tied** holds formulas: `A` and `B` are the distance from the
//!   callout's pin to the target's (`CALLOUTTARGETREF()!PinX`), `C` the share of that distance that lies inside the target's box, so that the leader ends **at the edge of the target nearest the callout**,
//!   and `X` and `Y` carry the rest of the way into the callout's own frame, through its turn and flips. So a move of the target moves the tip, a move of the callout alone makes the leader longer or
//!   shorter, and a recalculation does both (`calc` reads `CALLOUTTARGETREF` through the shape's `Relationships` cell).
//! * *The figure.* One `Geometry` section for each path of the figure, each coordinate a formula of `Width`, `Height` and the tip, so that the figure follows the box and the tip. `Scratch` row `IX` 1
//!   holds what several of its cells share (the side a speech box's wedge leaves by, the share of a corner that is rounded).
//! * *Role.* `User.msvStructureType` is `Callout`, and `User.CalloutStyle` is the gallery tile's id.
//! * *Connection points.* The four middles of the sides, as a text box has them.

use std::sync::Arc;

use diacraft_doc::{Cell, Links, Row, Section, Shape, ShapeType, Sheet, Text, TextPart, Value};

use super::{flagged_section, length, length_formula, number_text, plain, put, row, text_box};

/// The width of a callout made with no size, in inches.
pub const CALLOUT_WIDTH: f64 = 1.5;
/// The height of a callout made with no size, in inches.
pub const CALLOUT_HEIGHT: f64 = 0.5;
/// The word a callout holds when it is made (the gallery's tiles show it).
pub const CALLOUT_TEXT: &str = "Text";

/// The tip's two cells, as the `Scratch` row `IX` 0 holds them (in the shape's own frame).
pub const TIP_X: &str = "Scratch.X1";
/// See [`TIP_X`].
pub const TIP_Y: &str = "Scratch.Y1";

/// The look of a tile: what the figure holds besides the leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    TextLine,
    Underline,
    BoxLine,
    RoundBoxLine,
    OvalLine,
    LeftBracket,
    RightBracket,
    Brackets,
    SpeechBox,
    SpeechRound,
    SpeechOval,
    Cloud,
    HeaderBox,
    DoubleBox,
}

/// A tile of the callout gallery.
#[derive(Clone, Copy, Debug)]
pub struct CalloutStyle {
    /// The id `callout.insert` takes.
    pub id: &'static str,
    /// The words under the tile and in its tip.
    pub name: &'static str,
    look: Look,
}

macro_rules! style {
    ($id:literal, $name:literal, $look:ident) => {
        CalloutStyle { id: $id, name: $name, look: Look::$look }
    };
}

/// The fourteen tiles, in the order the gallery shows them (rows of four, four, four and two).
pub const STYLES: [CalloutStyle; 14] = [
    style!("text_line", "Text with Line", TextLine),
    style!("underlined_text", "Underlined Text", Underline),
    style!("box_line", "Box with Line", BoxLine),
    style!("round_box_line", "Rounded Box with Line", RoundBoxLine),
    style!("oval_line", "Oval with Line", OvalLine),
    style!("left_bracket", "Left Bracket", LeftBracket),
    style!("right_bracket", "Right Bracket", RightBracket),
    style!("brackets", "Brackets", Brackets),
    style!("speech_box", "Speech Box", SpeechBox),
    style!("speech_round_box", "Rounded Speech Box", SpeechRound),
    style!("speech_oval", "Speech Oval", SpeechOval),
    style!("thought_cloud", "Thought Cloud", Cloud),
    style!("header_box", "Header Box", HeaderBox),
    style!("double_box", "Double Box", DoubleBox),
];

impl CalloutStyle {
    /// Where the end of the leader is in the callout's own frame when it is tied to nothing and nobody said: below the box and off its left side, or its right for the right bracket (whose leader leaves
    /// the right side), as a length of `k` drawing units to an inch.
    pub(crate) fn default_tip(&self, width: f64, k: f64) -> (f64, f64) {
        if self.faces_right() { (width + 0.3 * k, -0.45 * k) } else { (-0.3 * k, -0.45 * k) }
    }

    /// Whether the leader leaves by the box's right side, so that a callout for a shape goes to the left of it and not the right.
    pub(crate) fn faces_right(&self) -> bool {
        self.look == Look::RightBracket
    }
}

/// The tile with this id.
pub fn find_style(id: &str) -> Option<&'static CalloutStyle> {
    STYLES.iter().find(|s| s.id == id)
}

/// A row of a path of the figure, every coordinate a formula.
#[derive(Clone, Debug)]
enum G {
    Move(String, String),
    Line(String, String),
    /// An arc through three points: from where the path is, through `via`, to `to`.
    Arc {
        to: (String, String),
        via: (String, String),
    },
    /// A circle or an ellipse: the centre, a point on its right and a point on its top.
    Ellipse {
        centre: (String, String),
        right: (String, String),
        top: (String, String),
    },
}

/// A path of the figure: whether it takes the fill, and its rows.
struct Path {
    fill: bool,
    rows: Vec<G>,
}

fn s(text: &str) -> String {
    text.to_owned()
}

fn num(n: f64) -> String {
    number_text(n)
}

/// `Width*share`, with the bare words and the constant 0 for the shares that need no product.
fn wf(share: f64) -> String {
    match share {
        0.0 => s("0"),
        1.0 => s("Width"),
        x => format!("Width*{}", num(x)),
    }
}

/// `Height*share`.
fn hf(share: f64) -> String {
    match share {
        0.0 => s("0"),
        1.0 => s("Height"),
        x => format!("Height*{}", num(x)),
    }
}

fn mv(x: impl Into<String>, y: impl Into<String>) -> G {
    G::Move(x.into(), y.into())
}

fn ln(x: impl Into<String>, y: impl Into<String>) -> G {
    G::Line(x.into(), y.into())
}

/// An arc to (`x`, `y`) through (`ax`, `ay`).
fn arc(x: impl Into<String>, y: impl Into<String>, ax: impl Into<String>, ay: impl Into<String>) -> G {
    G::Arc { to: (x.into(), y.into()), via: (ax.into(), ay.into()) }
}

/// A circle of radius `r` (a formula) at (`x`, `y`).
fn circle(x: &str, y: &str, r: &str) -> G {
    G::Ellipse { centre: (s(x), s(y)), right: (format!("{x}+{r}"), s(y)), top: (s(x), format!("{y}+{r}")) }
}

/// The ellipse that fills the box.
fn oval() -> G {
    G::Ellipse { centre: (wf(0.5), hf(0.5)), right: (wf(1.0), hf(0.5)), top: (wf(0.5), hf(1.0)) }
}

/// The rectangle of the box, closed.
fn rectangle() -> Vec<G> {
    vec![mv("0", "0"), ln(wf(1.0), "0"), ln(wf(1.0), hf(1.0)), ln("0", hf(1.0)), ln("0", "0")]
}

/// The leader, from where it leaves the figure to the tip: a line, which takes no fill.
fn leader(from: (String, String)) -> Path {
    Path { fill: false, rows: vec![mv(from.0, from.1), ln(TIP_X, TIP_Y)] }
}

/// Where a leader leaves a figure that is the box: the point of the box nearest the tip (the tip itself, when it is inside).
fn clamp_start() -> (String, String) {
    (format!("MAX(0,MIN(Width,{TIP_X}))"), format!("MAX(0,MIN(Height,{TIP_Y}))"))
}

/// The cells of `Scratch` row `IX` 1, named by column (`A` to `D`, `X`, `Y`), for the look: the formulas that several rows of the figure share, and whether each is a length.
fn shared(look: Look) -> Vec<(&'static str, String, bool)> {
    let (tx, ty) = (TIP_X, TIP_Y);
    match look {
        Look::RoundBoxLine => vec![("D", s("MIN(Width,Height)*0.2"), true)],
        Look::LeftBracket | Look::RightBracket | Look::Brackets => vec![("D", s("MIN(Width,Height)*0.25"), true)],
        Look::DoubleBox => vec![("D", s("MIN(Width,Height)*0.08"), true)],
        Look::OvalLine => vec![
            ("A", format!("({tx}-Width*0.5)/(Width*0.5)"), false),
            ("B", format!("({ty}-Height*0.5)/(Height*0.5)"), false),
            ("C", s("MIN(1,1/MAX(SQRT(Scratch.A2*Scratch.A2+Scratch.B2*Scratch.B2),0.000001))"), false),
            ("X", format!("Width*0.5+Scratch.C2*({tx}-Width*0.5)"), true),
            ("Y", format!("Height*0.5+Scratch.C2*({ty}-Height*0.5)"), true),
        ],
        Look::SpeechBox | Look::SpeechRound => {
            let round = if look == Look::SpeechRound { "MIN(Width,Height)*0.2" } else { "0" };
            vec![
                // The side the wedge leaves by: 0 the bottom, 1 the right, 2 the top, 3 the left. The side the tip lies out from by most, in shares of the box.
                ("A", format!("IF(ABS({tx}-Width*0.5)*Height>ABS({ty}-Height*0.5)*Width,IF({tx}>Width*0.5,1,3),IF({ty}>Height*0.5,2,0))"), false),
                // Where the wedge's base is centred along a horizontal side, and along a vertical one: the tip's, kept off the corners.
                ("B", format!("MAX(Scratch.D2+Scratch.X2,MIN(Width-Scratch.D2-Scratch.X2,{tx}))"), true),
                ("C", format!("MAX(Scratch.D2+Scratch.X2,MIN(Height-Scratch.D2-Scratch.X2,{ty}))"), true),
                ("D", s(round), true),
                // Half the wedge's base.
                ("X", s("MIN(Width,Height)*0.1"), true),
            ]
        }
        Look::SpeechOval => vec![
            // The direction of the tip from the middle, as the angle on the ellipse that the wedge is centred on.
            ("A", format!("ATAN2(({ty}-Height*0.5)/(Height*0.5),({tx}-Width*0.5)/(Width*0.5))"), false),
        ],
        Look::Cloud => vec![("X", format!("MAX(0,MIN(Width,{tx}))"), true), ("Y", format!("MAX(0,MIN(Height,{ty}))"), true)],
        Look::TextLine | Look::Underline | Look::BoxLine | Look::HeaderBox => Vec::new(),
    }
}

/// A rounded rectangle through the sides' ends, with the wedge of a speech box when `wedge` says so, or none: the rows of one closed path whose corners are arcs of the radius in `Scratch.D2`
/// (or sharp). Each side runs between its two corners through three points that are its middle when the wedge is not on it, so the rows are the same whichever side the wedge leaves by.
fn speech_rows(round: bool) -> Vec<G> {
    let (r, half, side) = ("Scratch.D2", "Scratch.X2", "Scratch.A2");
    let (base_x, base_y, tx, ty) = ("Scratch.B2", "Scratch.C2", TIP_X, TIP_Y);
    let on = |k: u8, then: String, otherwise: String| format!("IF({side}={k},{then},{otherwise})");
    let edge_start = if round { r.to_owned() } else { s("0") };
    let mut rows = vec![mv(edge_start, "0")];
    // The bottom side, left to right.
    rows.push(ln(on(0, format!("{base_x}-{half}"), wf(0.5)), "0"));
    rows.push(ln(on(0, tx.to_owned(), wf(0.5)), on(0, ty.to_owned(), s("0"))));
    rows.push(ln(on(0, format!("{base_x}+{half}"), wf(0.5)), "0"));
    rows.push(ln(if round { format!("Width-{r}") } else { wf(1.0) }, "0"));
    if round {
        rows.push(arc(wf(1.0), r, format!("Width-{r}*0.2929"), format!("{r}*0.2929")));
    }
    // The right side, upward.
    rows.push(ln("Width", on(1, format!("{base_y}-{half}"), hf(0.5))));
    rows.push(ln(on(1, tx.to_owned(), s("Width")), on(1, ty.to_owned(), hf(0.5))));
    rows.push(ln("Width", on(1, format!("{base_y}+{half}"), hf(0.5))));
    rows.push(ln("Width", if round { format!("Height-{r}") } else { hf(1.0) }));
    if round {
        rows.push(arc(format!("Width-{r}"), "Height", format!("Width-{r}*0.2929"), format!("Height-{r}*0.2929")));
    }
    // The top side, right to left.
    rows.push(ln(on(2, format!("{base_x}+{half}"), wf(0.5)), "Height"));
    rows.push(ln(on(2, tx.to_owned(), wf(0.5)), on(2, ty.to_owned(), s("Height"))));
    rows.push(ln(on(2, format!("{base_x}-{half}"), wf(0.5)), "Height"));
    rows.push(ln(if round { r.to_owned() } else { s("0") }, "Height"));
    if round {
        rows.push(arc("0", format!("Height-{r}"), format!("{r}*0.2929"), format!("Height-{r}*0.2929")));
    }
    // The left side, downward.
    rows.push(ln("0", on(3, format!("{base_y}+{half}"), hf(0.5))));
    rows.push(ln(on(3, tx.to_owned(), s("0")), on(3, ty.to_owned(), hf(0.5))));
    rows.push(ln("0", on(3, format!("{base_y}-{half}"), hf(0.5))));
    rows.push(ln("0", if round { r.to_owned() } else { s("0") }));
    if round {
        rows.push(arc(r, "0", format!("{r}*0.2929"), format!("{r}*0.2929")));
    }
    rows
}

/// A rounded rectangle, closed, with no wedge: the corners are arcs of the radius in `Scratch.D2`.
fn rounded_rectangle() -> Vec<G> {
    let r = "Scratch.D2";
    vec![
        mv(r, "0"),
        ln(format!("Width-{r}"), "0"),
        arc("Width", r, format!("Width-{r}*0.2929"), format!("{r}*0.2929")),
        ln("Width", format!("Height-{r}")),
        arc(format!("Width-{r}"), "Height", format!("Width-{r}*0.2929"), format!("Height-{r}*0.2929")),
        ln(r, "Height"),
        arc("0", format!("Height-{r}"), format!("{r}*0.2929"), format!("Height-{r}*0.2929")),
        ln("0", r),
        arc(r, "0", format!("{r}*0.2929"), format!("{r}*0.2929")),
    ]
}

/// The outline of a cloud: bumps of one length along an ellipse (the box's aspect is the made size's), each an arc through a point pushed out from the middle, as shares of the box.
fn cloud_rows() -> Vec<G> {
    const BUMPS: usize = 14;
    const SAMPLES: usize = 720;
    let (cx, cy, rx, ry, push) = (0.5_f64, 0.5_f64, 0.43_f64, 0.36_f64, 1.12_f64);
    let aspect = CALLOUT_WIDTH / CALLOUT_HEIGHT;
    // The ellipse in the box's own proportions, walked by its parameter, with the length so far: the vertices are the places where that length is a whole number of bumps.
    let point = |t: f64| (rx * aspect * t.cos(), ry * t.sin());
    let mut length = vec![0.0_f64];
    let mut last = point(0.0);
    for i in 1..=SAMPLES {
        let at = point(std::f64::consts::TAU * i as f64 / SAMPLES as f64);
        let before = length.last().copied().unwrap_or(0.0);
        length.push(before + ((at.0 - last.0).powi(2) + (at.1 - last.1).powi(2)).sqrt());
        last = at;
    }
    let whole = length.last().copied().unwrap_or(1.0);
    // The parameter at `share` of the way round.
    let param_at = |share: f64| -> f64 {
        let want = whole * share;
        let i = length.partition_point(|l| *l < want).min(SAMPLES);
        std::f64::consts::TAU * i as f64 / SAMPLES as f64
    };
    let at = |share: f64, scale: f64| -> (String, String) {
        let t = param_at(share);
        (wf(cx + rx * scale * t.cos()), hf(cy + ry * scale * t.sin()))
    };
    let first = at(0.0, 1.0);
    let mut rows = vec![mv(first.0, first.1)];
    for k in 0..BUMPS {
        let (to_share, via_share) = ((k + 1) as f64 / BUMPS as f64, (k as f64 + 0.5) / BUMPS as f64);
        let (to, via) = (at(to_share, 1.0), at(via_share, push));
        rows.push(arc(to.0, to.1, via.0, via.1));
    }
    rows
}

/// The paths of the figure, in the order they are painted.
fn paths(look: Look) -> Vec<Path> {
    let (tx, ty) = (TIP_X, TIP_Y);
    match look {
        Look::TextLine => vec![leader(clamp_start())],
        Look::Underline => vec![Path {
            fill: false,
            rows: vec![mv(format!("IF({tx}>Width*0.5,0,Width)"), "0"), ln(format!("IF({tx}>Width*0.5,Width,0)"), "0"), ln(tx, ty)],
        }],
        Look::BoxLine => vec![Path { fill: true, rows: rectangle() }, leader(clamp_start())],
        Look::RoundBoxLine => vec![Path { fill: true, rows: rounded_rectangle() }, leader(clamp_start())],
        Look::OvalLine => vec![Path { fill: true, rows: vec![oval()] }, leader((s("Scratch.X2"), s("Scratch.Y2")))],
        Look::LeftBracket => vec![
            Path { fill: false, rows: vec![mv("Scratch.D2", "Height"), ln("0", "Height"), ln("0", "0"), ln("Scratch.D2", "0")] },
            leader((s("0"), hf(0.5))),
        ],
        Look::RightBracket => vec![
            Path { fill: false, rows: vec![mv("Width-Scratch.D2", "Height"), ln("Width", "Height"), ln("Width", "0"), ln("Width-Scratch.D2", "0")] },
            leader((s("Width"), hf(0.5))),
        ],
        Look::Brackets => vec![
            Path { fill: false, rows: vec![mv("Scratch.D2", "Height"), ln("0", "Height"), ln("0", "0"), ln("Scratch.D2", "0")] },
            Path { fill: false, rows: vec![mv("Width-Scratch.D2", "Height"), ln("Width", "Height"), ln("Width", "0"), ln("Width-Scratch.D2", "0")] },
            leader((format!("IF({tx}>Width*0.5,Width,0)"), hf(0.5))),
        ],
        Look::SpeechBox => vec![Path { fill: true, rows: speech_rows(false) }],
        Look::SpeechRound => vec![Path { fill: true, rows: speech_rows(true) }],
        Look::SpeechOval => {
            let base = |sign: &str| -> (String, String) {
                (format!("Width*0.5*(1+COS(Scratch.A2{sign}0.3))"), format!("Height*0.5*(1+SIN(Scratch.A2{sign}0.3))"))
            };
            let (b1, b2) = (base("-"), base("+"));
            vec![
                // The wedge is painted first, so that the oval covers its base.
                Path { fill: true, rows: vec![mv(b1.0.clone(), b1.1.clone()), ln(tx, ty), ln(b2.0, b2.1), ln(b1.0, b1.1)] },
                Path { fill: true, rows: vec![oval()] },
            ]
        }
        Look::Cloud => {
            let (x, y) = ("Scratch.X2", "Scratch.Y2");
            // Two small circles on the way from the figure to the tip.
            let near = (format!("{x}+({tx}-{x})*0.35"), format!("{y}+({ty}-{y})*0.35"));
            let far = (format!("{x}+({tx}-{x})*0.75"), format!("{y}+({ty}-{y})*0.75"));
            vec![
                Path { fill: true, rows: cloud_rows() },
                Path { fill: true, rows: vec![circle(&near.0, &near.1, "MIN(Width,Height)*0.08")] },
                Path { fill: true, rows: vec![circle(&far.0, &far.1, "MIN(Width,Height)*0.05")] },
            ]
        }
        Look::HeaderBox => vec![
            Path { fill: true, rows: rectangle() },
            Path { fill: false, rows: vec![mv("0", hf(0.75)), ln("Width", hf(0.75))] },
            leader(clamp_start()),
        ],
        Look::DoubleBox => vec![
            Path { fill: true, rows: rectangle() },
            Path {
                fill: false,
                rows: vec![
                    mv("Scratch.D2", "Scratch.D2"),
                    ln("Width-Scratch.D2", "Scratch.D2"),
                    ln("Width-Scratch.D2", "Height-Scratch.D2"),
                    ln("Scratch.D2", "Height-Scratch.D2"),
                    ln("Scratch.D2", "Scratch.D2"),
                ],
            },
            leader(clamp_start()),
        ],
    }
}

/// A geometry cell: a formula, its value worked out when the shape is recalculated.
fn formula(text: &str) -> Cell {
    length_formula(text, 0.0)
}

fn geometry_row(ix: u32, g: &G) -> Arc<Row> {
    let at = |x: &str, y: &str| [("X", formula(x)), ("Y", formula(y))];
    match g {
        G::Move(x, y) => row(ix, Some("MoveTo"), at(x, y).into()),
        G::Line(x, y) => row(ix, Some("LineTo"), at(x, y).into()),
        G::Arc { to, via } => {
            let [x, y] = at(&to.0, &to.1);
            row(ix, Some("EllipticalArcTo"), vec![x, y, ("A", formula(&via.0)), ("B", formula(&via.1)), ("C", plain(0.0)), ("D", plain(1.0))])
        }
        G::Ellipse { centre, right, top } => row(
            ix,
            Some("Ellipse"),
            vec![
                ("X", formula(&centre.0)),
                ("Y", formula(&centre.1)),
                ("A", formula(&right.0)),
                ("B", formula(&right.1)),
                ("C", formula(&top.0)),
                ("D", formula(&top.1)),
            ],
        ),
    }
}

/// The `Geometry` sections of the figure.
fn geometry(look: Look) -> Vec<Section> {
    paths(look)
        .into_iter()
        .enumerate()
        .map(|(n, path)| {
            let mut section = flagged_section(u32::try_from(n).unwrap_or(u32::MAX), path.fill);
            for (i, g) in path.rows.iter().enumerate() {
                section.rows.push(geometry_row(u32::try_from(i + 1).unwrap_or(u32::MAX), g));
            }
            section
        })
        .collect()
}

/// The five cells of the tip when it is tied to a shape: formulas, the values of which a recalculation fills.
pub(crate) fn tied_tip_cells() -> [(&'static str, Cell); 5] {
    let a = "IFERROR(CALLOUTTARGETREF()!PinX-PinX,0)";
    let b = "IFERROR(CALLOUTTARGETREF()!PinY-PinY,0)";
    // The share of the way from the target's pin to the callout's pin that lies inside the target's box: the leader ends where it leaves the box.
    let c = "MIN(1,IFERROR(CALLOUTTARGETREF()!Width*0.5,0)/MAX(ABS(Scratch.A1),0.000001),IFERROR(CALLOUTTARGETREF()!Height*0.5,0)/MAX(ABS(Scratch.B1),0.000001))";
    // The rest of the way, from the callout's pin, in the callout's own frame: back through its turn, then its flips, then to its local pin.
    let x = "LocPinX+IF(FlipX,-1,1)*(Scratch.A1*(1-Scratch.C1)*COS(Angle)+Scratch.B1*(1-Scratch.C1)*SIN(Angle))";
    let y = "LocPinY+IF(FlipY,-1,1)*(Scratch.B1*(1-Scratch.C1)*COS(Angle)-Scratch.A1*(1-Scratch.C1)*SIN(Angle))";
    [
        ("A", length_formula(a, 0.0)),
        ("B", length_formula(b, 0.0)),
        ("C", Cell::with_formula(c, Some(Value::from_text("0")))),
        ("X", length_formula(x, 0.0)),
        ("Y", length_formula(y, 0.0)),
    ]
}

/// The five cells of the tip when it is tied to nothing: numbers, the tip at (`x`, `y`) in the callout's own frame.
pub(crate) fn free_tip_cells((x, y): (f64, f64)) -> [(&'static str, Cell); 5] {
    [("A", length(0.0)), ("B", length(0.0)), ("C", plain(0.0)), ("X", length(x)), ("Y", length(y))]
}

/// A row of the `User` section: a text.
fn user_row(name: &str, text: &str) -> Arc<Row> {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    put(&mut row.cells, "Value", Cell::constant(Value::from_text(text)).with_unit("STR"));
    Arc::new(row)
}

/// What a callout is made at: its box on the page and where the leader ends in its own frame, all in the page's drawing units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CalloutFrame {
    pub pin: (f64, f64),
    pub width: f64,
    pub height: f64,
    pub tip: (f64, f64),
}

/// The shape `id`: a callout of the style `style`, holding `text`, tied to nothing.
pub(crate) fn callout_shape(id: u32, style: &CalloutStyle, frame: &CalloutFrame, text: &str) -> Shape {
    let (width, height) = (frame.width, frame.height);
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    let cells = &mut sheet.cells;
    put(cells, "PinX", length(frame.pin.0));
    put(cells, "PinY", length(frame.pin.1));
    put(cells, "Width", length(width));
    put(cells, "Height", length(height));
    put(cells, "LocPinX", length_formula("Width*0.5", width * 0.5));
    put(cells, "LocPinY", length_formula("Height*0.5", height * 0.5));
    for flat in ["Angle", "FlipX", "FlipY", "ResizeMode"] {
        put(cells, flat, plain(0.0));
    }
    for (name, cell) in text_box::text_transform_cells(width, height) {
        put(cells, name, cell);
    }
    // The figure takes the page's theme: it holds no fill or line of its own.
    let mut scratch = Section::new("Scratch", None);
    let mut tip = Vec::new();
    for (name, cell) in free_tip_cells(frame.tip) {
        tip.push((name, cell));
    }
    scratch.rows.push(row(0, None, tip));
    let helpers = shared(style.look);
    if !helpers.is_empty() {
        let cells = helpers
            .iter()
            .map(|(name, text, is_length)| (*name, if *is_length { formula(text) } else { Cell::with_formula(text, Some(Value::from_text("0"))) }))
            .collect();
        scratch.rows.push(row(1, None, cells));
    }
    sheet.sections.push(Arc::new(scratch));
    let mut user = Section::new("User", None);
    user.rows.push(user_row("msvStructureType", "Callout"));
    user.rows.push(user_row("CalloutStyle", style.id));
    sheet.sections.push(Arc::new(user));
    for section in geometry(style.look) {
        sheet.sections.push(Arc::new(section));
    }
    sheet.sections.push(Arc::new(text_box::sides(width, height)));
    let text = (!text.is_empty()).then(|| Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
    Shape { kind: ShapeType::Shape, sheet: Arc::new(sheet), text, ..Shape::new(id) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_gallery_has_fourteen_tiles_with_ids_and_names_that_differ() {
        assert_eq!(STYLES.len(), 14);
        let mut ids: Vec<&str> = STYLES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 14);
        let mut names: Vec<&str> = STYLES.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 14);
        for style in &STYLES {
            assert_eq!(find_style(style.id).map(|s| s.id), Some(style.id));
        }
        assert!(find_style("nothing").is_none());
    }

    #[test]
    fn every_look_draws_at_least_one_path_and_every_row_is_a_move_a_line_an_arc_or_an_ellipse() {
        for style in &STYLES {
            let figure = paths(style.look);
            assert!(!figure.is_empty(), "{}", style.id);
            for path in figure {
                assert!(
                    matches!(path.rows.first(), Some(G::Move(..) | G::Ellipse { .. })),
                    "{}: a path begins with a move or is an ellipse",
                    style.id
                );
            }
        }
    }

    #[test]
    fn a_callout_has_the_tip_in_row_one_its_figure_in_geometry_and_its_role_in_user() {
        for style in &STYLES {
            let frame = CalloutFrame { pin: (3.0, 4.0), width: 1.5, height: 0.5, tip: (-0.3, -0.4) };
            let shape = callout_shape(7, style, &frame, "Text");
            let names: Vec<&str> = shape.sheet.sections.iter().map(|s| s.name.as_str()).collect();
            assert!(
                names.contains(&"Scratch") && names.contains(&"User") && names.contains(&"Geometry") && names.contains(&"Connection"),
                "{names:?}"
            );
            assert_eq!(shape.text.as_ref().map(|t| t.parts.len()), Some(1));
        }
    }
}
