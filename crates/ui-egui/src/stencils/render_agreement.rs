//! The tile of a master and the page's picture of the shape it drops, held to each other for every master the engine has.
//!
//! A tile is the shape's outline as the pointer reads it (`edit/scene.rs`), and the page's picture is the renderer's. They read the same cells, so they must agree on **which geometry is
//! there**: a section that is hidden (`NoShow`), a shape that has no line and no fill (`LinePattern` 0 and `FillPattern` 0: the text boxes of a group's members), a path with no stroke
//! and no fill (`NoLine` with `NoFill`) draw nothing on the page, and a tile that draws them is a blot where the page has an open figure (the BPMN gateway and events, the task and the
//! data object). The test drops every master on a drawing, has the renderer draw the page onto a target that only records what it is told (in paper inches, the unit of the scene), and
//! holds the tile's paths to what it recorded in two ways:
//!
//! * **Nothing the page does not draw.** Every point of every path of the tile lies on a path the renderer stroked or filled.
//! * **Everything the page draws, except a line's ends.** Every point a path the renderer stroked or filled passes through (where its curves join, not the points between) lies on a path of
//!   the tile. A one-dimensional master is left out of this half: the arrowheads of its ends are marks the tile draws apart (`tile_ends.rs`).
//!
//! The letters of a shape are left out of both: a tile has none, and the renderer is held to no text.
#![allow(clippy::unwrap_used)]

use diacraft_render::{Limits, Paint, Renderer, SkipKind, Target};
use kurbo::{Affine, BezPath, PathEl, Point, Stroke};

use super::*;

/// How far, in paper inches, a point of the tile may be from the picture's path and the other way about: the picture's curves are flattened to a thousandth and the tile's arcs are
/// walked in pieces, so a point that is on one is a few thousandths from the other.
const NEAR: f64 = 0.01;

/// A target that keeps the geometry it is told to stroke and to fill: paper inches, as the renderer hands it over. Nothing is kept that is painted with a colour that is clear.
struct Recorder {
    size: (f64, f64),
    painted: Vec<BezPath>,
}

impl Target for Recorder {
    fn to_px(&self) -> Affine {
        Affine::new([100.0, 0.0, 0.0, -100.0, 0.0, self.size.1])
    }
    fn size(&self) -> (f64, f64) {
        self.size
    }
    fn fill(&mut self, path: &BezPath, paint: &Paint, _: bool) {
        if !matches!(paint, Paint::Solid(c) if c.a == 0) {
            self.painted.push(path.clone());
        }
    }
    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, _: Affine) {
        self.fill(path, paint, even_odd);
    }
    fn stroke(&mut self, path: &BezPath, _: &Stroke, paint: &Paint) {
        if !matches!(paint, Paint::Solid(c) if c.a == 0) {
            self.painted.push(path.clone());
        }
    }
    fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
        draw(self);
    }
    fn push_clip(&mut self, _: &BezPath) {}
    fn pop_clip(&mut self) {}
    fn image(&mut self, _: &Arc<diacraft_render::Pixmap>, _: Affine) {}
    fn mark(&self) -> usize {
        0
    }
    fn rewind(&mut self, _: usize) {}
}

/// Every master of every stencil the engine has, the ones a drawing has not opened too (`stencil.list`'s `available`): the Shapes window shows each of them on a drawing that opens it.
pub(crate) fn every_builtin_master() -> Vec<MasterInfo> {
    let mut session = Session::new();
    let list = session.execute("stencil.list", &json!({})).unwrap();
    for entry in list["available"].as_array().unwrap() {
        session.execute("stencil.open", &json!({"id": entry["id"]})).unwrap();
    }
    parse(&session.execute("stencil.list", &json!({})).unwrap()).into_iter().flat_map(|s| s.masters).collect()
}

/// The distance from `p` to the segment `a` to `b`.
fn to_segment(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    geom::distance(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Whether `p` is within [`NEAR`] of one of the `lines`, which are polylines.
fn on_a_line(p: P, lines: &[Vec<P>]) -> bool {
    lines.iter().any(|line| match line.as_slice() {
        [only] => geom::distance(p, *only) <= NEAR,
        _ => line.windows(2).any(|pair| matches!(pair, [a, b] if to_segment(p, *a, *b) <= NEAR)),
    })
}

/// The polylines a recorded path is flattened to (a closed one comes back to where it began), and the points its curves join at.
fn flattened(path: &BezPath) -> (Vec<Vec<P>>, Vec<P>) {
    let mut lines: Vec<Vec<P>> = Vec::new();
    let mut joins: Vec<P> = Vec::new();
    let mut start: P = (0.0, 0.0);
    let to = |p: Point| (p.x, p.y);
    kurbo::flatten(path.elements().iter().copied(), 0.001, |el| match el {
        PathEl::MoveTo(p) => {
            start = to(p);
            lines.push(vec![start]);
        }
        PathEl::LineTo(p) => {
            if let Some(line) = lines.last_mut() {
                line.push(to(p));
            }
        }
        PathEl::ClosePath => {
            if let Some(line) = lines.last_mut() {
                line.push(start);
            }
        }
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => joins.push(to(p)),
            PathEl::ClosePath => {}
        }
    }
    (lines, joins)
}

/// What the renderer strokes and fills of `master` dropped on a drawing, and what its tile is drawn from, held to each other; the words of the first disagreement.
fn disagreement(master: &MasterInfo) -> Option<String> {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let page = session.document().unwrap().pages.iter().next().unwrap().id;
    let dropped = session.execute("shape.drop", &json!({"master": master.id, "page": page, "x": 5.0, "y": 4.0})).unwrap();
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    let doc = session.document().unwrap();
    let scene = Scene::build_all(doc, page, session.revision()).unwrap();
    let (_, tile) = drawn(&scene, scene.get(id).unwrap());

    let (w, h) = diacraft_render::page_paper_size(doc, page).unwrap();
    let mut recorder = Recorder { size: (w * 100.0, h * 100.0), painted: Vec::new() };
    // The letters are no geometry: a frame that may lay out no text leaves them out, and says so.
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { max_text_chars: 0, ..Limits::default() });
    renderer.set_paper(None);
    let skipped: Vec<_> = renderer.draw_onto(doc, page, &mut recorder).into_iter().filter(|s| s.kind != SkipKind::TextCharBudget).collect();
    if !skipped.is_empty() {
        return Some(format!("the page is drawn with a skip: {skipped:?}"));
    }
    let (mut lines, mut joins) = (Vec::new(), Vec::new());
    for path in &recorder.painted {
        let (l, j) = flattened(path);
        lines.extend(l);
        joins.extend(j);
    }
    // A master that paints nothing (a text label) is drawn by neither: the window gives its tile a plain box.
    if lines.is_empty() {
        return (!tile.is_empty()).then(|| "the tile draws paths where the page draws nothing".to_owned());
    }
    if figure_of(master).is_none() {
        return Some("the page draws it and its tile has no figure".to_owned());
    }
    // The marks at the ends of a one-dimensional master are centred on the vertices of its line, and the picture's line stops under them: a mark is the tile's own (`tile_ends.rs`), and
    // the end of the line it is drawn on is not asked to be on the picture's.
    let ends_apart = master.directed_preview();
    for path in &tile {
        let last = path.len().saturating_sub(1);
        if let Some((_, p)) = path.iter().enumerate().find(|(at, p)| !(ends_apart && (*at == 0 || *at == last)) && !on_a_line(**p, &lines)) {
            return Some(format!("the tile draws a path through ({:.3}, {:.3}) where the page draws nothing", p.0, p.1));
        }
    }
    if !ends_apart && let Some(p) = joins.iter().find(|p| !on_a_line((p.0, p.1), &tile)) {
        return Some(format!("the page draws through ({:.3}, {:.3}) where the tile draws nothing", p.0, p.1));
    }
    None
}

#[test]
fn the_tile_of_every_master_the_engine_has_draws_what_the_page_draws_of_the_shape_and_nothing_else() {
    let masters = every_builtin_master();
    assert!(masters.len() > 150, "the engine's stencils hold {} masters", masters.len());
    let failed: Vec<String> = masters.iter().filter_map(|m| disagreement(m).map(|why| format!("{}: {why}", m.id))).collect();
    assert!(failed.is_empty(), "{} of {} masters disagree:\n{}", failed.len(), masters.len(), failed.join("\n"));
}

#[test]
fn the_tiles_of_the_bpmn_and_the_other_notation_shapes_are_their_default_figures() {
    let masters = every_builtin_master();
    let figure = |id: &str| {
        figure_of(masters.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no master {id}"))).unwrap_or_else(|| panic!("{id} has no figure"))
    };
    // A gateway is its diamond, with none of the markers it hides at its default Shape Data, nor the box its name is in.
    let gateway = figure("bpmn.gateway");
    assert_eq!((gateway.areas.len(), gateway.lines.len()), (1, 0), "{gateway:?}");
    assert_eq!(gateway.areas[0].0.len(), 5, "four corners and the first again");
    // A task is its rounded rectangle, and a data object its page with the folded corner.
    let task = figure("bpmn.task");
    assert_eq!((task.areas.len(), task.lines.len()), (1, 0), "{task:?}");
    let object = figure("bpmn.data-object");
    assert_eq!((object.areas.len(), object.lines.len()), (1, 1), "the page, and the fold: {object:?}");
    // The events are rings that close (one, one and two), in a square: the name under an event is not part of its figure.
    for (id, rings) in [("bpmn.start-event", 1), ("bpmn.end-event", 1), ("bpmn.intermediate-event", 2)] {
        let event = figure(id);
        assert_eq!((event.areas.len(), event.lines.len()), (rings, 0), "{id}: {event:?}");
        assert!((event.aspect.0 - 1.0).abs() < 1.0e-6 && (event.aspect.1 - 1.0).abs() < 1.0e-6, "{id}: {:?}", event.aspect);
    }
    // A label that has no line and no fill paints nothing, and has no figure: the window draws it as a plain box.
    assert!(figure_of(masters.iter().find(|m| m.id == "c4.title").unwrap()).is_none());
}
