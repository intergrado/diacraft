//! The tiles of connector, one-dimensional and group masters, looked at as pixels.
//!
//! The test windows draw nothing (`egui_kittest` has no renderer here), so this file fills the window's last frame in software: the shapes of
//! the frame are cut into triangles the way the graphics card is given them, and each triangle is painted over the pixels it covers (`Pixels`).
//! The theme and the scale are set by the test (the light theme, two pixels to a point), so a probe is the same on every machine. A probe asks
//! whether the pen's ink (the stencil ink token) is at a place of a tile or only the card's own fill is.

use std::path::PathBuf;

use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape, shape_with};
use diacraft_ui_egui::theme::Tokens;
use diacraft_ui_egui::tile_ends::UNIT;
use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};

use crate::editing::edit_window_with;
use crate::stencil_files::{open_from_the_flyout, window as stencil_window};
use crate::support::{Rest, TempDir, Window, elements_named, run_command};

/// The content type of a stencil.
const CT_STENCIL: &str = "application/vnd.ms-visio.stencil.main+xml"; // brand-ok: a content type the format defines

/// How much of a tile's side its figure takes, and how much of that the figure leaves free all round: the window's own numbers (`shapes_window.rs`).
const FIGURE_SHARE: f32 = 0.6;
const FIGURE_MARGIN: f32 = 0.12;

/// Pixels to a point.
const PPP: f32 = 2.0;

// ---- the window's pixels ------------------------------------------------------------------------------------------------------------------

/// What the window's last frame drew inside a rectangle of points. The card the figure is on is `fill`, and the pen is `ink`: a pixel's
/// *coverage* is how far its colour has gone from the first to the second.
struct Pixels {
    origin: Pos2,
    width: usize,
    height: usize,
    px: Vec<[f32; 4]>,
    fill: [f32; 3],
    ink: [f32; 3],
}

fn rgb(c: egui::Color32) -> [f32; 3] {
    [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0]
}

/// The frame inside `region`, in software.
fn pixels(w: &Window, region: Rect) -> Pixels {
    let output = w.output();
    let ppp = output.pixels_per_point;
    let tokens = Tokens::get(&w.ctx);
    let (fill, ink) = (rgb(tokens.panel_header_fill), rgb(tokens.stencil_ink));
    let (width, height) = ((region.width() * ppp).ceil() as usize, (region.height() * ppp).ceil() as usize);
    let mut out = Pixels { origin: region.min, width, height, px: vec![[fill[0], fill[1], fill[2], 1.0]; width * height], fill, ink };
    for ClippedPrimitive { clip_rect, primitive } in w.ctx.tessellate(output.shapes.clone(), ppp) {
        let Primitive::Mesh(mesh) = primitive else { continue };
        let clip = clip_rect.intersect(region);
        if !clip.is_positive() {
            continue;
        }
        for corners in mesh.indices.as_chunks::<3>().0 {
            let v: Vec<egui::epaint::Vertex> = corners.iter().filter_map(|i| mesh.vertices.get(*i as usize).copied()).collect();
            // Text and pictures take their colours from a texture (their corners differ in it): none of them is a probe.
            if v.len() == 3 && v[0].uv == v[1].uv && v[1].uv == v[2].uv {
                out.triangle(&v, clip, ppp);
            }
        }
    }
    out
}

impl Pixels {
    /// One triangle, painted over what is there: the colour of each pixel's centre, from the colours of the corners.
    fn triangle(&mut self, v: &[egui::epaint::Vertex], clip: Rect, ppp: f32) {
        let (a, b, c) = (v[0].pos, v[1].pos, v[2].pos);
        let det = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if det.abs() < 1.0e-9 {
            return;
        }
        let low = pos2(a.x.min(b.x).min(c.x).max(clip.min.x), a.y.min(b.y).min(c.y).max(clip.min.y));
        let high = pos2(a.x.max(b.x).max(c.x).min(clip.max.x), a.y.max(b.y).max(c.y).min(clip.max.y));
        let column = |x: f32| ((x - self.origin.x) * ppp).floor().max(0.0) as usize;
        let row = |y: f32| ((y - self.origin.y) * ppp).floor().max(0.0) as usize;
        let colour = |k: usize| v[k].color.to_array().map(|n| f32::from(n) / 255.0);
        let (ca, cb, cc) = (colour(0), colour(1), colour(2));
        for j in row(low.y)..=row(high.y).min(self.height.saturating_sub(1)) {
            for i in column(low.x)..=column(high.x).min(self.width.saturating_sub(1)) {
                let p = self.origin + vec2((i as f32 + 0.5) / ppp, (j as f32 + 0.5) / ppp);
                if !clip.contains(p) {
                    continue;
                }
                let wa = ((b.x - p.x) * (c.y - p.y) - (b.y - p.y) * (c.x - p.x)) / det;
                let wb = ((c.x - p.x) * (a.y - p.y) - (c.y - p.y) * (a.x - p.x)) / det;
                let wc = 1.0 - wa - wb;
                if wa < -1.0e-4 || wb < -1.0e-4 || wc < -1.0e-4 {
                    continue;
                }
                let src: Vec<f32> = (0..4).map(|k| wa * ca[k] + wb * cb[k] + wc * cc[k]).collect();
                if let Some(dst) = self.px.get_mut(j * self.width + i) {
                    for k in 0..4 {
                        dst[k] = src[k] + dst[k] * (1.0 - src[3]);
                    }
                }
            }
        }
    }

    /// How far the pixel at `(i, j)` has gone from the card's colour to the ink's: 0 bare, 1 inked.
    fn coverage(&self, i: usize, j: usize) -> f32 {
        let Some(px) = self.px.get(j * self.width + i) else { return 0.0 };
        let (mut along, mut span) = (0.0, 0.0);
        for k in 0..3 {
            let axis = self.fill[k] - self.ink[k];
            along += (self.fill[k] - px[k]) * axis;
            span += axis * axis;
        }
        (along / span.max(1.0e-9)).clamp(0.0, 1.0)
    }

    /// The coverage of the pixel `p` is in and of every pixel whose centre is within `reach` points of `p`.
    fn near(&self, p: Pos2, reach: f32) -> Vec<f32> {
        let (cx, cy) = (((p.x - self.origin.x) * PPP).floor() as isize, ((p.y - self.origin.y) * PPP).floor() as isize);
        let r = (reach * PPP).ceil() as isize + 1;
        let mut found = Vec::new();
        for j in cy - r..=cy + r {
            for i in cx - r..=cx + r {
                let centre = self.origin + vec2((i as f32 + 0.5) / PPP, (j as f32 + 0.5) / PPP);
                if let (Ok(i), Ok(j)) = (usize::try_from(i), usize::try_from(j))
                    && i < self.width
                    && j < self.height
                    && (centre.distance(p) <= reach || (i as isize, j as isize) == (cx, cy))
                {
                    found.push(self.coverage(i, j));
                }
            }
        }
        found
    }

    /// Ink at `p`, or within a point of it (a thin stroke is found a little to one side).
    fn inked(&self, p: Pos2) -> bool {
        self.near(p, 1.0).into_iter().fold(0.0, f32::max) >= 0.55
    }

    /// Nothing but the card within a point of `p`.
    fn bare(&self, p: Pos2) -> bool {
        self.near(p, 1.0).into_iter().fold(0.0, f32::max) <= 0.2
    }

    /// Nothing but the card in the one pixel `p` is in: for the inside of a mark a few points across, where a point's reach would find its sides.
    fn bare_pixel(&self, p: Pos2) -> bool {
        self.near(p, 0.0).into_iter().fold(0.0, f32::max) <= 0.2
    }

    /// Filled with ink: every pixel within 0.6 of a point of `p`.
    fn solid(&self, p: Pos2) -> bool {
        self.near(p, 0.6).into_iter().fold(1.0, f32::min) >= 0.8
    }

    /// The cards side by side in rows of `columns`.
    fn sheet(cards: &[Pixels], columns: usize) -> Pixels {
        let (cw, ch) = cards.first().map_or((1, 1), |c| (c.width, c.height));
        let rows = cards.len().div_ceil(columns);
        let mut sheet = Pixels {
            origin: pos2(0.0, 0.0),
            width: cw * columns,
            height: ch * rows,
            px: vec![[0.5, 0.5, 0.5, 1.0]; cw * columns * ch * rows],
            fill: [0.0; 3],
            ink: [1.0; 3],
        };
        for (n, card) in cards.iter().enumerate() {
            let (x0, y0) = ((n % columns) * cw, (n / columns) * ch);
            for j in 0..ch.min(card.height) {
                for i in 0..cw.min(card.width) {
                    sheet.px[(y0 + j) * sheet.width + x0 + i] = card.px[j * card.width + i];
                }
            }
        }
        sheet
    }

    /// The pixels as a PNG, for a person: the card's colours as drawn.
    fn write_png(&self, path: &std::path::Path) {
        let mut data = Vec::with_capacity(self.px.len() * 3);
        for px in &self.px {
            data.extend(px[..3].iter().map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&data).unwrap();
    }

    /// The lengths of the runs of ink along the row of pixels through `from`, to `to`: a stroke between two rows is the more covered of the two.
    fn runs(&self, from: Pos2, to: Pos2) -> Vec<usize> {
        let row = ((from.y - self.origin.y) * PPP) as usize;
        let (first, last) = (((from.x - self.origin.x) * PPP) as usize, ((to.x - self.origin.x) * PPP) as usize);
        let mut runs = Vec::new();
        let mut run = 0;
        for i in first..=last {
            let inked = (row.saturating_sub(1)..=row + 1).any(|j| self.coverage(i, j) >= 0.5);
            if inked {
                run += 1;
            } else if run > 0 {
                runs.push(run);
                run = 0;
            }
        }
        if run > 0 {
            runs.push(run);
        }
        runs
    }
}

// ---- the windows ---------------------------------------------------------------------------------------------------------------------------

/// The rectangle of the tile of the master called `name` in the window.
fn tile_rect(w: &Window, name: &str) -> Rect {
    let found = elements_named(w, &format!("shapes.tile.{name}"));
    found.first().map(|e| e.1).unwrap_or_else(|| panic!("no tile for {name}: {:?}", elements_named(w, "shapes.tile.")))
}

/// Where the point `p` of a figure whose box is `aspect` (its longer side 1, y up) lands on a tile: the way the Shapes window fits a figure to its
/// card.
fn on_tile(tile: Rect, aspect: (f64, f64), p: (f64, f64)) -> Pos2 {
    let side = tile.width() * FIGURE_SHARE;
    let square = Rect::from_center_size(tile.center(), vec2(side, side)).shrink(side * FIGURE_MARGIN);
    let scale = f64::from(square.width());
    let origin = pos2(square.center().x - (aspect.0 * scale / 2.0) as f32, square.center().y + (aspect.1 * scale / 2.0) as f32);
    pos2(origin.x + (p.0 * scale) as f32, origin.y - (p.1 * scale) as f32)
}

/// The window's own drawing at two pixels to a point, light, at rest.
fn at_two_pixels(mut w: Window) -> Window {
    w.set_pixels_per_point(PPP);
    w.rest();
    w
}

/// A window whose Shapes window lists one stencil with one connector master, **the engine's Data Flow line** with the ends and the pattern the list gives it.
fn flow_window(ends: Value) -> Window {
    let list = json!({"stencils": [{"id": "flows", "name": "Flows", "masters": [
        {"id": "gane-sarson.data-flow", "name": "Flow", "width": 1.5, "height": 0.0, "one_d": true, "ends": ends}
    ]}]});
    at_two_pixels(edit_window_with([1280.0, 900.0], |app| app.with_stencil_list(&list)))
}

/// The pixels of the card of `name`, and the card. With `STENCIL_TILES_DUMP` set to a folder, each card is also written there as a PNG (for a person to look at).
fn card(w: &Window, name: &str) -> (Pixels, Rect) {
    let tile = tile_rect(w, name);
    let pixels = pixels(w, tile);
    if let Some(dir) = std::env::var_os("STENCIL_TILES_DUMP") {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let file = std::path::Path::new(&dir).join(format!("{}-{n}-{name}.png", std::thread::current().name().unwrap_or("t").replace("::", "-")));
        pixels.write_png(&file);
    }
    (pixels, tile)
}

// ---- connectors ----------------------------------------------------------------------------------------------------------------------------

/// A line from the left to the right of the figure, with a hollow long diamond at its begin and a solid triangle at its end: each mark has the kind
/// gives its number, sits with its tip on its vertex, and the line stops short of the hollow one and runs into the solid one.
#[test]
fn a_connector_tile_draws_its_begin_and_end_marks() {
    let w = flow_window(json!({"begin": 22, "end": 4, "pattern": 1}));
    let (pixels, tile) = card(&w, "Flow");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 0.0), (x, y));
    // The line between the marks.
    assert!(pixels.inked(at(0.6, 0.0)), "the line runs from one mark to the other");
    // The begin: a long diamond, 4.5 units long and 2.24 across, its left vertex on the begin.
    assert!(pixels.inked(at(0.0, 0.0)), "the diamond's tip is on the begin");
    assert!(pixels.inked(at(2.25 * UNIT, 1.12 * UNIT)), "its upper vertex");
    assert!(pixels.inked(at(2.25 * UNIT, -1.12 * UNIT)), "and its lower one");
    assert!(pixels.bare(at(2.25 * UNIT, 0.0)), "a hollow mark has nothing inside, and the line stops at its back");
    assert!(pixels.bare(at(-0.12, 0.0)), "nothing is drawn past the begin");
    // The end: a solid triangle, 2 units long and 2 across, its tip on the end.
    assert!(pixels.inked(at(1.0, 0.0)), "the triangle's tip is on the end");
    assert!(pixels.solid(at(1.0 - UNIT, 0.0)), "a solid mark is filled");
    assert!(pixels.inked(at(1.0 - 2.0 * UNIT, UNIT)) && pixels.inked(at(1.0 - 2.0 * UNIT, -UNIT)), "its back corners");
    assert!(pixels.bare(at(1.12, 0.0)), "nothing is drawn past the end");
}

/// A line with no ends has only its line, and a line end that the list does not know (a number past 254, a kind of mark of none) draws nothing.
#[test]
fn a_connector_with_no_ends_is_its_line_and_a_number_that_is_no_mark_draws_none() {
    for ends in
        [json!({"begin": 0, "end": 0, "pattern": 1}), json!({"begin": 200, "end": 99, "pattern": 1}), json!({"begin": "x", "end": -3, "pattern": 1})]
    {
        let w = flow_window(ends.clone());
        let (pixels, tile) = card(&w, "Flow");
        let at = |x: f64, y: f64| on_tile(tile, (1.0, 0.0), (x, y));
        assert!(pixels.inked(at(0.02, 0.0)) && pixels.inked(at(0.5, 0.0)) && pixels.inked(at(0.98, 0.0)), "{ends}: the line is whole");
        for x in [0.1, 0.9] {
            assert!(pixels.bare(at(x, 0.7 * UNIT)) && pixels.bare(at(x, -0.7 * UNIT)), "{ends}: nothing beside the line at {x}");
        }
    }
}

/// The line of a dashed connector is made of dashes with gaps, and a dotted one of dots: the pattern table of is
/// what the tile follows, in marks and gaps. A solid line is one run.
#[test]
fn a_dashed_connector_tile_is_dashed_and_a_dotted_one_dotted() {
    let runs = |pattern: u32| {
        let w = flow_window(json!({"begin": 0, "end": 0, "pattern": pattern}));
        let (pixels, tile) = card(&w, "Flow");
        pixels.runs(on_tile(tile, (1.0, 0.0), (0.0, 0.0)), on_tile(tile, (1.0, 0.0), (1.0, 0.0)))
    };
    let solid = runs(1);
    assert_eq!(solid.len(), 1, "a solid line is one run: {solid:?}");
    let dashed = runs(2);
    assert!(dashed.len() >= 2 && dashed.iter().all(|r| *r >= 8), "a dashed line is a few long dashes (in pixels): {dashed:?}");
    let dotted = runs(3);
    assert!(dotted.len() >= dashed.len() + 2 && dotted.iter().all(|r| *r <= 5), "a dotted line is many short dots: {dotted:?}");
    let dash_dot = runs(4);
    assert!(dash_dot.iter().any(|r| *r >= 8) && dash_dot.iter().any(|r| *r <= 5), "a dash and dot line has both: {dash_dot:?}");
}

/// The marks keep the line's pattern out of them: a dashed line has a whole triangle at its end.
#[test]
fn the_marks_of_a_dashed_connector_are_whole() {
    let w = flow_window(json!({"begin": 0, "end": 16, "pattern": 2}));
    let (pixels, tile) = card(&w, "Flow");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 0.0), (x, y));
    // A hollow triangle: its two long sides and its back are strokes, whole whatever the line's pattern.
    for t in [0.2, 0.5, 0.8] {
        let x = 1.0 - 2.0 * UNIT * t;
        assert!(pixels.inked(at(x, UNIT * t)), "the upper side at {t}");
        assert!(pixels.inked(at(x, -UNIT * t)), "the lower side at {t}");
    }
    assert!(pixels.inked(at(1.0 - 2.0 * UNIT, 0.5 * UNIT)) && pixels.inked(at(1.0 - 2.0 * UNIT, -0.5 * UNIT)), "the back");
    assert!(pixels.bare_pixel(at(1.0 - UNIT, 0.0)), "and nothing inside it, where the dashes would run on");
}

/// The master `id` as the engine's own `stencil.list` gives it.
fn engine_master(id: &str) -> Value {
    let list = diacraft_engine::Session::new().execute("stencil.list", &json!({})).unwrap();
    list["stencils"].as_array().unwrap().iter().flat_map(|s| s["masters"].as_array().unwrap().iter()).find(|m| m["id"] == id).cloned().unwrap()
}

/// The Data Flow keeps the tile it had: a line with a head at its end and nothing at its begin, in the same place and of the size the head had. The master is the
/// engine's own, as its list gives it (its `ends` are 0, 4 and 1), and the ink is the one of both themes.
#[test]
fn the_data_flow_tile_is_as_before() {
    let flow = engine_master("gane-sarson.data-flow");
    assert_eq!(flow["ends"], json!({"begin": 0, "end": 4, "pattern": 1}), "what the engine says of the Data Flow");
    for dark in [false, true] {
        let list = json!({"stencils": [{"id": "gane-sarson", "name": "Gane-Sarson", "masters": [flow.clone()]}]});
        let mut w = at_two_pixels(edit_window_with([1280.0, 900.0], |app| app.with_stencil_list(&list)));
        if dark {
            run_command(&mut w, "window.theme", json!({"theme": "dark"}));
        }
        let (pixels, tile) = card(&w, "Data Flow");
        let at = |x: f64, y: f64| on_tile(tile, (1.0, 0.0), (x, y));
        let theme = if dark { "dark" } else { "light" };
        assert!(pixels.inked(at(0.03, 0.0)) && pixels.inked(at(0.3, 0.0)) && pixels.inked(at(0.7, 0.0)), "{theme}: the line");
        assert!(pixels.inked(at(1.0, 0.0)), "{theme}: the head's tip is on the end");
        // The head the tile always had: 0.18 of the line long, and about as wide.
        assert!(pixels.inked(at(0.82, 0.09)) && pixels.inked(at(0.82, -0.09)), "{theme}: its back corners, 0.18 from the tip");
        assert!(pixels.bare(at(0.8, 0.16)) && pixels.bare(at(0.8, -0.16)), "{theme}: and no wider");
        assert!(pixels.bare(at(1.12, 0.0)), "{theme}: nothing past the tip");
        // Nothing at the begin.
        assert!(pixels.bare(at(0.05, 0.08)) && pixels.bare(at(0.05, -0.08)), "{theme}: no mark at the begin");
        assert!(pixels.bare(at(-0.12, 0.0)), "{theme}: nothing before the begin");
    }
}

// ---- stencil files: one-dimensional and group masters -------------------------------------------------------------------------------------

/// The cells of a one-dimensional master `w` long, and the rows of a path through `points` in the line's own frame (x along it, y across it).
fn line_cells(w: f64, points: &[(f64, f64)]) -> String {
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(n, (x, y))| {
            format!(r#"<Row T="{}" IX="{}"><Cell N="X" V="{x}"/><Cell N="Y" V="{y}"/></Row>"#, if n == 0 { "MoveTo" } else { "LineTo" }, n + 1)
        })
        .collect();
    format!(
        r#"<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="{w}"/><Cell N="EndY" V="0"/>
<Cell N="PinX" V="{half}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="0" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{w}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{half}" F="Width*0.5"/><Cell N="LocPinY" V="0"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>{rows}</Section>"#,
        half = w / 2.0
    )
}

/// A box of a group: `w` by `h`, its lower left corner at `(x, y)` of the group's frame.
fn member_cells(x: f64, y: f64, w: f64, h: f64) -> String {
    let (right, top) = (x + w, y + h);
    format!(
        r#"<Cell N="PinX" V="{px}"/><Cell N="PinY" V="{py}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/>
<Cell N="LocPinX" V="{lx}" F="Width*0.5"/><Cell N="LocPinY" V="{ly}" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
        px = (x + right) / 2.0,
        py = (y + top) / 2.0,
        lx = w / 2.0,
        ly = h / 2.0
    )
}

/// A group `w` by `h` whose members are `members`, and which has no geometry of its own; `extra` is more cells of the group.
fn group_xml(id: u32, w: f64, h: f64, extra: &str, members: &str) -> String {
    let frame = format!(
        r#"<Cell N="PinX" V="{px}"/><Cell N="PinY" V="{py}"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="{px}"/><Cell N="LocPinY" V="{py}"/>{extra}"#,
        px = w / 2.0,
        py = h / 2.0
    );
    shape_with(id, r#" Type="Group""#, &format!("{frame}<Shapes>{members}</Shapes>"))
}

/// A stencil file of the masters `masters`, written under `dir`.
fn stencil_file(dir: &TempDir, masters: Vec<MasterSpec>) -> PathBuf {
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for m in masters {
        builder = builder.master(m);
    }
    let path = dir.file("Tiles.vssx");
    std::fs::write(&path, builder.build()).unwrap();
    path
}

/// The window with the stencil file open, at two pixels to a point.
fn window_on_file(masters: Vec<MasterSpec>) -> (Window, TempDir) {
    let dir = TempDir::new("stencil-tiles");
    let path = stencil_file(&dir, masters);
    let mut w = stencil_window(path);
    open_from_the_flyout(&mut w);
    (at_two_pixels(w), dir)
}

/// A one-dimensional master draws its own geometry along its line, however far the geometry goes across it: a zigzag, and a loop that goes out farther
/// than the line is long. Nothing is cut off, and the tile has the box of the geometry, not of the line.
#[test]
fn a_fixed_1d_tile_draws_its_geometry_along_a_line() {
    let zigzag = [(0.0, 0.0), (0.3, 0.2), (0.6, -0.2), (0.9, 0.2), (1.2, -0.2), (1.5, 0.0)];
    let hook = [(0.0, 0.0), (0.0, 0.45), (0.3, 0.45), (0.3, 0.0)];
    let masters = vec![
        MasterSpec::new(1, "Zigzag").shapes(&shape(5, &line_cells(1.5, &zigzag))),
        MasterSpec::new(2, "Hook").shapes(&shape(5, &line_cells(0.3, &hook))),
    ];
    let (w, _dir) = window_on_file(masters);
    assert_eq!(w.state().tile_art(Some("file-1"), "1"), Some("outline"));
    // The zigzag's box is its line's length by its amplitude: the longer side 1.5.
    let (pixels, tile) = card(&w, "Zigzag");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 0.4 / 1.5), (x / 1.5, y / 1.5 + 0.2 / 1.5));
    for (x, y) in zigzag {
        assert!(pixels.inked(at(x, y)), "the zigzag goes through ({x}, {y})");
    }
    assert!(pixels.inked(at(0.15, 0.1)) && pixels.inked(at(0.45, 0.0)), "and along its legs");
    assert!(pixels.bare(at(0.3, 0.0)) && pixels.bare(at(0.9, 0.0)), "not along the straight line between its ends");
    // The hook goes out to 0.45 over a line of 0.3: its box is 0.3 by 0.45, the longer side 0.45.
    let (pixels, tile) = card(&w, "Hook");
    let at = |x: f64, y: f64| on_tile(tile, (0.3 / 0.45, 1.0), (x / 0.45, y / 0.45));
    for (x, y) in [(0.0, 0.0), (0.0, 0.3), (0.15, 0.45), (0.3, 0.3), (0.3, 0.0)] {
        assert!(pixels.inked(at(x, y)), "the hook goes through ({x}, {y})");
    }
    assert!(pixels.bare(at(0.15, 0.2)), "and is open below");
    let inside = Rect::from_center_size(tile.center(), tile.size() - vec2(8.0, 8.0));
    assert!(inside.contains(at(0.15, 0.45)) && inside.contains(at(0.0, 0.0)), "the hook lies inside its card");
}

/// A group master's tile draws the outlines of its members, and not the box of the group, which holds nothing of its own.
#[test]
fn a_group_master_tile_draws_its_members_and_not_its_empty_box() {
    // A group 3 by 2 holding two boxes, 0.8 square, far apart, with nothing between them and nothing of its own to draw.
    let members = format!("{}{}", shape(6, &member_cells(0.2, 0.6, 0.8, 0.8)), shape(7, &member_cells(2.0, 0.6, 0.8, 0.8)));
    let (w, _dir) = window_on_file(vec![MasterSpec::new(1, "Pair").shapes(&group_xml(5, 3.0, 2.0, "", &members))]);
    assert_eq!(w.state().tile_art(Some("file-1"), "1"), Some("outline"), "the members are drawn, not a box");
    let (pixels, tile) = card(&w, "Pair");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 2.0 / 3.0), (x / 3.0, y / 3.0));
    // The members: each box's four sides.
    for (x, y) in [(0.2, 1.0), (1.0, 1.0), (0.6, 0.6), (0.6, 1.4), (2.0, 1.0), (2.8, 1.0), (2.4, 0.6), (2.4, 1.4)] {
        assert!(pixels.inked(at(x, y)), "a member's side at ({x}, {y})");
    }
    // The group's box: its four corners, the middle of each side, and the space between the members are bare.
    for (x, y) in [(0.0, 0.0), (3.0, 0.0), (0.0, 2.0), (3.0, 2.0), (1.5, 0.0), (1.5, 2.0), (0.0, 1.0), (3.0, 1.0), (1.5, 1.0)] {
        assert!(pixels.bare(at(x, y)), "the group has nothing at ({x}, {y})");
    }
}

/// A group inside a group is drawn too, and members at every depth.
#[test]
fn a_group_inside_a_group_master_is_drawn_to_its_members() {
    let inner = group_xml(8, 1.0, 1.0, "", &shape(9, &member_cells(0.1, 0.1, 0.8, 0.8)));
    // The inner group is placed by its own frame cells in the outer group's frame: its pin at (2.5, 1.5).
    let inner = inner.replacen(r#"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/>"#, r#"<Cell N="PinX" V="2.5"/><Cell N="PinY" V="1.5"/>"#, 1);
    let members = format!("{}{inner}", shape(6, &member_cells(0.2, 0.6, 0.8, 0.8)));
    let (w, _dir) = window_on_file(vec![MasterSpec::new(1, "Nest").shapes(&group_xml(5, 3.0, 2.0, "", &members))]);
    let (pixels, tile) = card(&w, "Nest");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 2.0 / 3.0), (x / 3.0, y / 3.0));
    assert!(pixels.inked(at(0.2, 1.0)), "the outer member");
    // The inner group's own member: 0.8 square, centred on the inner group's pin (2.5, 1.5): its left side is at x 2.1.
    assert!(pixels.inked(at(2.1, 1.5)) && pixels.inked(at(2.9, 1.5)), "the member of the group inside the group");
    assert!(pixels.bare(at(1.5, 1.0)) && pixels.bare(at(2.5, 1.5)), "and the space is bare");
}

/// A group set to select the group only (`SelectMode` 0) has no members for a pointer, and its tile still draws them: it shows the group as it is.
#[test]
fn a_group_set_to_group_only_is_drawn_to_its_members_too() {
    let members = shape(6, &member_cells(0.2, 0.6, 0.8, 0.8));
    let group = group_xml(5, 3.0, 2.0, r#"<Cell N="SelectMode" V="0"/>"#, &members);
    let (w, _dir) = window_on_file(vec![MasterSpec::new(1, "Only").shapes(&group)]);
    assert_eq!(w.state().tile_art(Some("file-1"), "1"), Some("outline"));
    let (pixels, tile) = card(&w, "Only");
    let at = |x: f64, y: f64| on_tile(tile, (1.0, 2.0 / 3.0), (x / 3.0, y / 3.0));
    assert!(pixels.inked(at(0.2, 1.0)) && pixels.inked(at(1.0, 1.0)), "the member's sides");
    assert!(pixels.bare(at(3.0, 2.0)) && pixels.bare(at(1.5, 1.0)), "and not the group's box");
}

// ---- a look at the marks -----------------------------------------------------------------------------------------------------------------

/// The cards of every line end and every pattern, as two sheets in each theme, written to the folder `STENCIL_TILES_DUMP` names. Not a test of anything: a person looks at
/// the sheets. The sheet of ends has a card for each number 0 to 45 and 254, in rows of eight, each a line with that mark at both its ends; the sheet of patterns has a card
/// for each pattern 1 to 23, in rows of eight, each a line with a triangle at its end.
#[test]
#[ignore = "writes the sheets of marks and patterns to STENCIL_TILES_DUMP, for a person to look at"]
fn a_look_at_every_mark_and_every_pattern() {
    let dir = PathBuf::from(std::env::var_os("STENCIL_TILES_DUMP").expect("STENCIL_TILES_DUMP names the folder to write to"));
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let cards = |ends: &dyn Fn(u32) -> Value, numbers: Vec<u32>| -> Vec<Pixels> {
            numbers
                .into_iter()
                .map(|n| {
                    let mut w = flow_window(ends(n));
                    if dark {
                        run_command(&mut w, "window.theme", json!({"theme": "dark"}));
                    }
                    pixels(&w, tile_rect(&w, "Flow"))
                })
                .collect()
        };
        let marks = cards(&|n| json!({"begin": n, "end": n, "pattern": 1}), (0..=45).chain([254]).collect());
        Pixels::sheet(&marks, 8).write_png(&dir.join(format!("marks-{theme}.png")));
        let patterns = cards(&|n| json!({"begin": 0, "end": 4, "pattern": n}), (1..=23).collect());
        Pixels::sheet(&patterns, 8).write_png(&dir.join(format!("patterns-{theme}.png")));
    }
}

/// A stencil file of the masters this task draws tiles for, written to `Tiles.vssx` in the folder `STENCIL_TILES_DUMP` names: for the app to open, and a person to look at its
/// tiles. A zigzag and a hook (one-dimensional), a group of two boxes, a group in a group, a group set to select the group only, and a box for company.
#[test]
#[ignore = "writes Tiles.vssx to STENCIL_TILES_DUMP, for a person to look at"]
fn a_stencil_file_of_the_tiles_for_a_look_in_the_app() {
    let dir = PathBuf::from(std::env::var_os("STENCIL_TILES_DUMP").expect("STENCIL_TILES_DUMP names the folder to write to"));
    let zigzag = [(0.0, 0.0), (0.3, 0.2), (0.6, -0.2), (0.9, 0.2), (1.2, -0.2), (1.5, 0.0)];
    let hook = [(0.0, 0.0), (0.0, 0.45), (0.3, 0.45), (0.3, 0.0)];
    let pair = format!("{}{}", shape(6, &member_cells(0.2, 0.6, 0.8, 0.8)), shape(7, &member_cells(2.0, 0.6, 0.8, 0.8)));
    let inner = group_xml(8, 1.0, 1.0, "", &shape(9, &member_cells(0.1, 0.1, 0.8, 0.8)));
    let inner = inner.replacen(r#"<Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/>"#, r#"<Cell N="PinX" V="2.5"/><Cell N="PinY" V="1.5"/>"#, 1);
    let nest = format!("{}{inner}", shape(6, &member_cells(0.2, 0.6, 0.8, 0.8)));
    let only = group_xml(5, 3.0, 2.0, r#"<Cell N="SelectMode" V="0"/>"#, &shape(6, &member_cells(0.2, 0.6, 0.8, 0.8)));
    let file = VsdxBuilder::new()
        .document_type(CT_STENCIL)
        .master(MasterSpec::new(1, "Zigzag").shapes(&shape(5, &line_cells(1.5, &zigzag))))
        .master(MasterSpec::new(2, "Hook").shapes(&shape(5, &line_cells(0.3, &hook))))
        .master(MasterSpec::new(3, "Pair").shapes(&group_xml(5, 3.0, 2.0, "", &pair)))
        .master(MasterSpec::new(4, "Nest").shapes(&group_xml(5, 3.0, 2.0, "", &nest)))
        .master(MasterSpec::new(5, "Only").shapes(&only))
        .master(MasterSpec::new(6, "Box").shapes(&shape(5, &member_cells(0.0, 0.0, 2.0, 1.0))))
        .build();
    std::fs::write(dir.join("Tiles.vssx"), file).unwrap();
}
