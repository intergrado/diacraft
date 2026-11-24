//! A page as a list of drawing steps, to be drawn later and on other threads.
//!
//! The walk of a page ([`crate::frame`]) says what to draw to a [`Target`]. A [`Recorder`] is a target that only writes the steps down: a
//! [`Recording`] is the list, nested as the walk nested it (a clip or a blur holds the steps drawn inside it), each step with the box it can
//! reach in the target's pixels. Drawing a recording onto a target ([`replay`]) makes the very calls the walk would have made, in the same
//! order, so the pixels are the same ones; what it adds is that the steps can be read many times, and the ones that cannot reach a part of
//! the picture left out of that part's drawing ([`replay_within`], which `par.rs` uses to draw the bands of a frame on separate threads).

use std::sync::Arc;

use kurbo::{Affine, BezPath, Rect, Stroke};
use vello_cpu::Pixmap;

use crate::target::{Paint, Target};

/// Pixels past a shape's geometry that anti-aliasing can still colour. A step's box is widened by this much so that a step is never left
/// out of a part of the picture it touches.
const AA_SLOP_PX: f64 = 2.0;

/// The tile the rasteriser's filter layers are rounded out to, twice (the layer's box and its padding): see `Target::blur_layer_pixels`.
const BLUR_TILE_SLOP_PX: f64 = 8.0;

/// A step of the drawing: what a [`Target`] is told, with the same arguments.
pub(crate) enum Op {
    Fill {
        path: BezPath,
        paint: Paint,
        even_odd: bool,
    },
    FillWithPaintTransform {
        path: BezPath,
        paint: Paint,
        even_odd: bool,
        paint_transform: Affine,
    },
    Stroke {
        path: BezPath,
        stroke: Stroke,
        paint: Paint,
    },
    Image {
        image: Arc<Pixmap>,
        place: Affine,
    },
    /// What is drawn inside it is cut by `path`.
    Clip {
        path: BezPath,
        inner: Vec<Node>,
    },
    /// What is drawn inside it is blurred by `sigma` paper inches.
    Blur {
        sigma: f64,
        inner: Vec<Node>,
    },
}

/// A step and the box it can reach, in the target's pixels (with the slop of anti-aliasing and, for a blur, its reach).
pub(crate) struct Node {
    pub bbox: Rect,
    pub op: Op,
}

/// The steps of a frame, in drawing order, and the surface they were made for.
pub(crate) struct Recording {
    pub width: u16,
    pub height: u16,
    pub to_px: Affine,
    pub nodes: Vec<Node>,
}

impl Recording {
    /// The farthest any blur in the recording reaches past what it blurs, in pixels (0 when it has none): the margin a part of the picture
    /// needs around it to see the same blur as the whole does.
    pub(crate) fn blur_reach(&self) -> f64 {
        fn reach(nodes: &[Node], most: &mut f64) {
            for node in nodes {
                match &node.op {
                    Op::Clip { inner, .. } => reach(inner, most),
                    Op::Blur { inner, .. } => {
                        // The box is the inner union plus the reach on every side, so the reach is what the box has over the union.
                        let union = union_of(inner);
                        if let Some(u) = union {
                            let r = (u.x0 - node.bbox.x0).max(u.y0 - node.bbox.y0).max(node.bbox.x1 - u.x1).max(node.bbox.y1 - u.y1);
                            if r.is_finite() && r > *most {
                                *most = r;
                            }
                        }
                        reach(inner, most);
                    }
                    _ => {}
                }
            }
        }
        let mut most = 0.0;
        reach(&self.nodes, &mut most);
        most
    }

    /// The boxes of the blurs in the recording, in pixels: each is the layer a blur makes (the box of what it blurs and its reach), before the
    /// layer is cut to a context.
    pub(crate) fn blur_boxes(&self) -> Vec<Rect> {
        fn collect(nodes: &[Node], out: &mut Vec<Rect>) {
            for node in nodes {
                match &node.op {
                    Op::Clip { inner, .. } => collect(inner, out),
                    Op::Blur { .. } => out.push(node.bbox),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        collect(&self.nodes, &mut out);
        out
    }
}

fn union_of(nodes: &[Node]) -> Option<Rect> {
    nodes.iter().map(|n| n.bbox).reduce(|a, b| a.union(b))
}

/// A scope the walk has opened and not yet closed.
struct Open {
    kind: Kind,
    nodes: Vec<Node>,
    union: Option<Rect>,
}

enum Kind {
    Clip(BezPath),
    Blur(f64),
}

/// A [`Target`] that writes the steps down. It answers the questions the walk asks (size, transform, the narrowest stroke, whether it
/// blurs) as a raster canvas of the same size does, so the walk makes the same choices.
pub(crate) struct Recorder {
    width: u16,
    height: u16,
    to_px: Affine,
    nodes: Vec<Node>,
    open: Vec<Open>,
    /// The largest scale from paper inches to pixels the view has, for the reach of strokes and blurs.
    scale: f64,
    /// How many steps have been recorded, nested ones counted: the shares of a parallel walk say it to the tally, which holds the recordings of all
    /// of them to a count (`Parallel::max_steps`), since a recording is memory the frame did not have to hold.
    count: usize,
}

impl Recorder {
    pub(crate) fn new(width: u16, height: u16, to_px: Affine) -> Recorder {
        let [a, b, c, d, ..] = to_px.as_coeffs();
        // The Frobenius norm is never less than the largest stretch the transform makes, so a reach counted with it is never short.
        let scale = (a * a + b * b + c * c + d * d).sqrt();
        Recorder { width, height, to_px, nodes: Vec::new(), open: Vec::new(), scale: if scale.is_finite() { scale } else { 0.0 }, count: 0 }
    }

    pub(crate) fn width(&self) -> u16 {
        self.width
    }

    pub(crate) fn height(&self) -> u16 {
        self.height
    }

    /// Puts steps written down elsewhere (by the share of a parallel walk) after what is recorded, inside the scope that is open.
    pub(crate) fn append(&mut self, nodes: Vec<Node>) {
        for node in nodes {
            self.put(node);
        }
    }

    /// The steps recorded so far (the open scopes are not in it): for a share, which opens none.
    pub(crate) fn into_nodes(mut self) -> Vec<Node> {
        self.rewind(0);
        self.nodes
    }

    /// Closes what is still open, and gives the recording.
    pub(crate) fn finish(mut self) -> Recording {
        self.rewind(0);
        Recording { width: self.width, height: self.height, to_px: self.to_px, nodes: self.nodes }
    }

    fn put(&mut self, node: Node) {
        self.count = self.count.saturating_add(1);
        match self.open.last_mut() {
            Some(open) => {
                open.union = Some(open.union.map_or(node.bbox, |u| u.union(node.bbox)));
                open.nodes.push(node);
            }
            None => self.nodes.push(node),
        }
    }

    /// The box of a path in paper inches, in pixels, with the slop of anti-aliasing and `extra` paper inches' worth of reach.
    fn bbox_of(&self, path: &BezPath, reach_paper: f64) -> Rect {
        let paper = path.control_box().inflate(reach_paper, reach_paper);
        self.to_px.transform_rect_bbox(paper).inflate(AA_SLOP_PX, AA_SLOP_PX)
    }

    /// Closes the innermost scope into its parent.
    fn close(&mut self) {
        let Some(open) = self.open.pop() else { return };
        let (bbox, op) = match open.kind {
            Kind::Clip(path) => {
                let clip_box = self.bbox_of(&path, 0.0);
                // Only what lies inside the clip shows, so the scope reaches no farther than the clip does.
                let bbox = open.union.map_or(Rect::ZERO, |u| u.intersect(clip_box));
                (bbox, Op::Clip { path, inner: open.nodes })
            }
            Kind::Blur(sigma) => {
                let reach = 3.0 * sigma.abs() * self.scale + BLUR_TILE_SLOP_PX;
                let bbox = open.union.map_or(Rect::ZERO, |u| u.inflate(reach, reach));
                (bbox, Op::Blur { sigma, inner: open.nodes })
            }
        };
        // An empty scope draws nothing: it has no box to meet.
        let bbox = if bbox.x0.is_nan() { Rect::ZERO } else { bbox };
        self.put(Node { bbox, op });
    }
}

impl Target for Recorder {
    fn to_px(&self) -> Affine {
        self.to_px
    }

    fn size(&self) -> (f64, f64) {
        (f64::from(self.width), f64::from(self.height))
    }

    fn min_stroke(&self) -> f64 {
        crate::canvas::MIN_STROKE_PX
    }

    fn mark(&self) -> usize {
        self.open.len()
    }

    fn recorded(&self) -> usize {
        self.count
    }

    fn rewind(&mut self, mark: usize) {
        while self.open.len() > mark {
            self.close();
        }
    }

    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool) {
        let bbox = self.bbox_of(path, 0.0);
        self.put(Node { bbox, op: Op::Fill { path: path.clone(), paint: paint.clone(), even_odd } });
    }

    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        let bbox = self.bbox_of(path, 0.0);
        self.put(Node { bbox, op: Op::FillWithPaintTransform { path: path.clone(), paint: paint.clone(), even_odd, paint_transform } });
    }

    fn stroke(&mut self, path: &BezPath, stroke: &Stroke, paint: &Paint) {
        // A join can reach `miter_limit` half-widths from the path, a square cap a little over one.
        let reach = stroke.width.abs() * 0.5 * stroke.miter_limit.max(1.5);
        let bbox = self.bbox_of(path, if reach.is_finite() { reach } else { 0.0 });
        self.put(Node { bbox, op: Op::Stroke { path: path.clone(), stroke: stroke.clone(), paint: paint.clone() } });
    }

    fn blurred<F: FnOnce(&mut Self)>(&mut self, sigma: f64, draw: F) {
        let depth = self.open.len();
        self.open.push(Open { kind: Kind::Blur(sigma), nodes: Vec::new(), union: None });
        draw(self);
        // The scope this call opened, if what was drawn inside it left the others as it found them.
        if self.open.len() > depth {
            self.rewind(depth + 1);
            self.close();
        }
    }

    fn push_clip(&mut self, path: &BezPath) {
        self.open.push(Open { kind: Kind::Clip(path.clone()), nodes: Vec::new(), union: None });
    }

    fn pop_clip(&mut self) {
        self.close();
    }

    fn image(&mut self, image: &Arc<Pixmap>, place: Affine) {
        let rect = Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height()));
        let paper = place.transform_rect_bbox(rect);
        let bbox = self.to_px.transform_rect_bbox(paper).inflate(AA_SLOP_PX, AA_SLOP_PX);
        self.put(Node { bbox, op: Op::Image { image: Arc::clone(image), place } });
    }
}

/// Draws every step of `nodes` onto `target`, in order: the calls the walk made.
pub(crate) fn replay<T: Target>(nodes: &[Node], target: &mut T) {
    replay_within(nodes, None, target);
}

/// Draws the steps of `nodes` that can reach `within` (a box in the target's pixels; `None` is everywhere), in order. A step that cannot
/// reach it is left out with everything inside it; a clip is kept only if something inside reaches `within`; **a blur that reaches it is
/// drawn whole**, so that the layer is the one the whole frame makes.
pub(crate) fn replay_within<T: Target>(nodes: &[Node], within: Option<Rect>, target: &mut T) {
    for node in nodes {
        if let Some(w) = within
            && !touches(&node.bbox, &w)
        {
            continue;
        }
        match &node.op {
            Op::Fill { path, paint, even_odd } => target.fill(path, paint, *even_odd),
            Op::FillWithPaintTransform { path, paint, even_odd, paint_transform } => {
                target.fill_with_paint_transform(path, paint, *even_odd, *paint_transform);
            }
            Op::Stroke { path, stroke, paint } => target.stroke(path, stroke, paint),
            Op::Image { image, place } => target.image(image, *place),
            Op::Clip { path, inner } => {
                if within.is_some_and(|w| !inner.iter().any(|n| touches(&n.bbox, &w))) {
                    continue;
                }
                target.push_clip(path);
                replay_within(inner, within, target);
                target.pop_clip();
            }
            Op::Blur { sigma, inner } => {
                target.blurred(*sigma, |t| replay_within(inner, None, t));
            }
        }
    }
}

/// Whether two boxes share a pixel row and column (a box with a side that is not a number touches nothing).
pub(crate) fn touches(a: &Rect, b: &Rect) -> bool {
    a.x1 >= b.x0 && a.x0 <= b.x1 && a.y1 >= b.y0 && a.y0 <= b.y1
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_color::Rgba;
    use kurbo::{Circle, Shape as _};

    use super::*;
    use crate::canvas::Canvas;

    fn solid(r: u8, g: u8, b: u8) -> Paint {
        Paint::Solid(Rgba::rgb(r, g, b))
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        Rect::new(x0, y0, x1, y1).to_path(0.1)
    }

    /// A drawing of every kind of step, 10 by 10 inches at 20 pixels to the inch (200 pixels square): a fill, a stroke, a gradient, a clip with
    /// a fill inside it, a blur across the middle of the frame inside the clip, and a picture.
    fn drawing<T: Target>(t: &mut T) {
        t.fill(&rect(1.0, 1.0, 6.0, 5.0), &solid(255, 0, 0), true);
        let mut line = BezPath::new();
        line.move_to((0.5, 9.5));
        line.curve_to((3.0, 2.0), (6.0, 12.0), (9.5, 0.5));
        t.stroke(&line, &Stroke::new(0.3).with_join(kurbo::Join::Miter), &solid(0, 0, 255));
        let gradient = Paint::Gradient(crate::target::Gradient {
            kind: crate::target::GradientKind::Linear { start: kurbo::Point::new(0.0, 0.0), end: kurbo::Point::new(10.0, 0.0) },
            stops: vec![
                crate::target::GradientStop { offset: 0.0, color: Rgba::rgb(255, 255, 0) },
                crate::target::GradientStop { offset: 1.0, color: Rgba::rgb(0, 128, 128) },
            ],
        });
        t.fill_with_paint_transform(&rect(6.0, 6.0, 9.5, 9.5), &gradient, false, Affine::translate((0.5, 0.0)));
        t.push_clip(&Circle::new((5.0, 5.0), 4.0).to_path(0.1));
        t.fill(&rect(0.0, 4.0, 10.0, 6.0), &solid(0, 160, 0), false);
        t.blurred(0.3, |t| t.fill(&rect(2.0, 3.0, 8.0, 7.5), &solid(40, 40, 40), true));
        t.pop_clip();
        let mut picture = Pixmap::new(2, 2);
        picture.data_as_u8_slice_mut().copy_from_slice(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);
        t.image(&Arc::new(picture), Affine::translate((0.0, 0.0)) * Affine::scale(0.02));
    }

    fn to_px() -> Affine {
        Affine::new([20.0, 0.0, 0.0, -20.0, 0.0, 200.0])
    }

    #[test]
    fn a_recording_replayed_makes_the_pixels_the_drawing_makes() {
        let mut direct = Canvas::new(200, 200, to_px());
        drawing(&mut direct);
        let mut recorder = Recorder::new(200, 200, to_px());
        drawing(&mut recorder);
        let recording = recorder.finish();
        let mut again = Canvas::new(200, 200, to_px());
        replay(&recording.nodes, &mut again);
        let (a, b) = (direct.finish(), again.finish());
        assert!(a.iter().any(|&v| v != 0) && a == b, "the replay is the drawing");
    }

    #[test]
    fn bands_of_a_recording_are_the_frame_at_every_thread_count_with_a_blur_across_every_edge() {
        let mut direct = Canvas::new(200, 200, to_px());
        drawing(&mut direct);
        let whole = direct.finish();
        let mut recorder = Recorder::new(200, 200, to_px());
        drawing(&mut recorder);
        let recording = recorder.finish();
        for threads in [2usize, 3, 7, 31] {
            let par = crate::Parallel::eager(threads);
            let (pixels, used) = crate::par::raster(&recording, par, threads).unwrap();
            assert!(used > 1, "{threads} threads: the frame is drawn in bands");
            assert!(pixels == whole, "{threads} threads: the bands are the frame");
        }
    }

    /// Counts what is drawn, and says nothing is blurred away.
    #[derive(Default)]
    struct Count {
        fills: usize,
        clips: usize,
        blurs: usize,
        inner_fills_in_blurs: usize,
        in_blur: usize,
    }

    impl Target for Count {
        fn to_px(&self) -> Affine {
            Affine::IDENTITY
        }
        fn size(&self) -> (f64, f64) {
            (1000.0, 1000.0)
        }
        fn fill(&mut self, _: &BezPath, _: &Paint, _: bool) {
            self.fills += 1;
            self.inner_fills_in_blurs += usize::from(self.in_blur > 0);
        }
        fn fill_with_paint_transform(&mut self, p: &BezPath, paint: &Paint, e: bool, _: Affine) {
            self.fill(p, paint, e);
        }
        fn stroke(&mut self, _: &BezPath, _: &Stroke, _: &Paint) {}
        fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
            self.blurs += 1;
            self.in_blur += 1;
            draw(self);
            self.in_blur -= 1;
        }
        fn push_clip(&mut self, _: &BezPath) {
            self.clips += 1;
        }
        fn pop_clip(&mut self) {}
        fn image(&mut self, _: &Arc<Pixmap>, _: Affine) {}
        fn mark(&self) -> usize {
            0
        }
        fn rewind(&mut self, _: usize) {}
    }

    #[test]
    fn a_band_leaves_out_the_steps_that_cannot_reach_it_and_draws_a_blur_that_does_whole() {
        let mut recorder = Recorder::new(1000, 2000, Affine::IDENTITY);
        recorder.fill(&rect(0.0, 0.0, 1000.0, 100.0), &solid(1, 1, 1), true); // rows 0 to 100
        recorder.fill(&rect(0.0, 500.0, 1000.0, 600.0), &solid(2, 2, 2), true); // rows 500 to 600
        recorder.push_clip(&rect(0.0, 700.0, 1000.0, 800.0));
        recorder.fill(&rect(0.0, 0.0, 1000.0, 2000.0), &solid(3, 3, 3), true); // as tall as the frame, cut to rows 700 to 800
        recorder.pop_clip();
        // Three deviations of 10 pixels reach 42 rows (the transform's norm is the square root of two) and the tile slack 8 past what is blurred.
        recorder.blurred(10.0, |t| {
            t.fill(&rect(0.0, 1100.0, 100.0, 1110.0), &solid(4, 4, 4), true);
            t.fill(&rect(0.0, 1500.0, 100.0, 1510.0), &solid(5, 5, 5), true);
        });
        let recording = recorder.finish();
        let band = |first: f64, end: f64| {
            let mut count = Count::default();
            replay_within(&recording.nodes, Some(Rect::new(f64::NEG_INFINITY, first, f64::INFINITY, end)), &mut count);
            (count.fills, count.clips, count.blurs, count.inner_fills_in_blurs)
        };
        assert_eq!(band(450.0, 650.0), (1, 0, 0, 0), "only the fill in the band");
        assert_eq!(band(690.0, 710.0), (1, 1, 0, 0), "the clip that the band meets, with the fill in it");
        assert_eq!(band(1000.0, 1040.0), (0, 0, 0, 0), "between: nothing reaches it");
        assert_eq!(
            band(1540.0, 1545.0),
            (2, 0, 1, 2),
            "in the blur's reach of its second fill: the whole blur, the first fill far from the band too"
        );
        let mut count = Count::default();
        replay(&recording.nodes, &mut count);
        assert_eq!((count.fills, count.clips, count.blurs), (5, 1, 1), "everywhere: all of it");
    }

    #[test]
    fn a_blurs_reach_is_what_its_box_has_over_what_it_blurs() {
        let mut recorder = Recorder::new(100, 100, Affine::scale(2.0));
        recorder.blurred(1.5, |t| t.fill(&rect(10.0, 10.0, 20.0, 20.0), &solid(0, 0, 0), true));
        let recording = recorder.finish();
        // Three deviations of 1.5 paper inches at a scale of 2 (the transform's norm is 2 * sqrt 2 over the two axes), and the tile slack.
        let reach = recording.blur_reach();
        assert!(reach >= 9.0 + 8.0 - 1.0e-9, "at least three deviations in pixels and the slack: {reach}");
        assert!(reach <= 3.0 * 1.5 * 2.0 * 2.0_f64.sqrt() + 8.0 + 1.0e-9, "and no more than the norm of the scale gives: {reach}");
        assert_eq!(recording.blur_boxes().len(), 1);
        assert_eq!(Recording { width: 1, height: 1, to_px: Affine::IDENTITY, nodes: Vec::new() }.blur_reach(), 0.0);
    }

    #[test]
    fn a_drawing_cut_short_inside_a_blur_leaves_what_follows_outside_it() {
        let mut recorder = Recorder::new(100, 100, Affine::IDENTITY);
        let mark = recorder.mark();
        recorder.push_clip(&rect(0.0, 0.0, 50.0, 50.0));
        let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recorder.blurred(1.0, |t| {
                t.fill(&rect(1.0, 1.0, 5.0, 5.0), &solid(9, 9, 9), true);
                panic!("cut short");
            });
        }));
        assert!(cut.is_err());
        assert_eq!(recorder.mark(), mark + 2, "the clip and the blur are open");
        recorder.rewind(mark);
        assert_eq!(recorder.mark(), mark);
        recorder.fill(&rect(60.0, 60.0, 70.0, 70.0), &solid(8, 8, 8), true);
        let recording = recorder.finish();
        // The clip holds the blur holding the first fill; the second fill is beside the clip at the top level.
        assert_eq!(recording.nodes.len(), 2);
        assert!(matches!(&recording.nodes[0].op, Op::Clip { inner, .. } if matches!(&inner[0].op, Op::Blur { inner, .. } if inner.len() == 1)));
        assert!(matches!(&recording.nodes[1].op, Op::Fill { .. }));
    }

    #[test]
    fn a_recorder_counts_the_steps_it_is_given_nested_ones_too() {
        let mut recorder = Recorder::new(100, 100, Affine::IDENTITY);
        recorder.fill(&rect(0.0, 0.0, 1.0, 1.0), &solid(0, 0, 0), true);
        recorder.push_clip(&rect(0.0, 0.0, 50.0, 50.0));
        recorder.fill(&rect(1.0, 1.0, 2.0, 2.0), &solid(0, 0, 0), true);
        recorder.pop_clip();
        // Two fills and the clip that closed over one of them.
        assert_eq!(recorder.recorded(), 3);
    }

    #[test]
    fn steps_written_down_elsewhere_are_added_inside_the_scope_that_is_open() {
        let mut elsewhere = Recorder::new(100, 100, Affine::IDENTITY);
        elsewhere.fill(&rect(0.0, 0.0, 10.0, 10.0), &solid(1, 1, 1), true);
        let nodes = elsewhere.into_nodes();
        let mut recorder = Recorder::new(100, 100, Affine::IDENTITY);
        recorder.push_clip(&rect(0.0, 0.0, 50.0, 50.0));
        recorder.append(nodes);
        let recording = recorder.finish();
        assert!(matches!(&recording.nodes[0].op, Op::Clip { inner, .. } if inner.len() == 1));
    }
}
