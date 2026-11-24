//! Icons, drawn in code.
//!
//! Every icon is DiaCraft's own: a handful of strokes and fills on a unit square, in the theme's icon colours. There is
//! no icon file, no icon font and no copy of another product's artwork (`AGENTS.md`, Assets). A control that has no
//! drawing of its own yet shows a base drawing with a small round badge (a plus, a check, an arrow...) that tells it from
//! the others: plain and consistent, and a place to put a specific drawing later without touching the callers.
//!
//! An [`Icon`] is a [`Base`] drawing and a [`Badge`]. [`paint`] draws it into a rectangle.

use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};

use crate::theme::{self, Tokens};

/// The corner a badge sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    None,
    Plus,
    Minus,
    Check,
    Cross,
    Arrow,
    Dot,
    Question,
}

/// The drawings an icon starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Page,
    Pages,
    Clipboard,
    Scissors,
    Brush,
    Pointer,
    Connector,
    TextA,
    TextBlock,
    Rect,
    Ellipse,
    Line,
    Freeform,
    Arc,
    Pencil,
    Dot,
    Swatches,
    Bucket,
    PenLine,
    Shadow,
    Align,
    Move,
    Front,
    Back,
    Group,
    Swap,
    Magnifier,
    Layers,
    Select,
    Picture,
    Star,
    Chart,
    Container,
    Callout,
    Link,
    Symbol,
    Palette,
    Resize,
    Gear,
    Window,
    Eye,
    Fit,
    Spelling,
    Book,
    Globe,
    Bubble,
    Ruler,
    Grid,
    Magnet,
    Database,
    Refresh,
    Table,
    Play,
    Help,
    Person,
    Bold,
    Italic,
    Underline,
    Strike,
    FontUp,
    FontDown,
    CaseAa,
    FontColor,
    Bullets,
    TextLeft,
    TextCenter,
    TextRight,
    TextJustify,
    TextTop,
    TextMiddle,
    TextBottom,
    IndentLess,
    IndentMore,
    TextTurn,
    ArrowLeft,
    ArrowRight,
    Undo,
    Redo,
    Save,
    FolderOpen,
    Collapse,
    Triangle,
    Diamond,
    Pentagon,
    Hexagon,
    Octagon,
    RightTriangle,
    Parallelogram,
    Trapezoid,
    Cross,
    Arrow,
    Cylinder,
    Square,
    Circle,
    Plus,
    Minus,
    Mark,
    /// A shape turned a quarter to the left or to the right (Rotate Left, Rotate Right).
    TurnLeft,
    TurnRight,
    /// A shape and its mirror image across a vertical or a horizontal line (Flip Horizontal, Flip Vertical).
    FlipH,
    FlipV,
    /// Two bars lined up on a line, on the centre, the right, the top, the middle or the bottom (the left is [`Base::Align`]).
    AlignCenter,
    AlignRight,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    /// A printer: a body with a tray of paper over it and a sheet coming out (Print).
    Printer,
    /// A sheet held upright and a sheet held on its side (Page Setup's Orientation).
    PagePortrait,
    PageLandscape,
}

/// A drawing: a base and a badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon {
    pub base: Base,
    pub badge: Badge,
}

impl Icon {
    /// The base drawing alone.
    pub const fn of(base: Base) -> Icon {
        Icon { base, badge: Badge::None }
    }

    /// The same drawing with `badge` in its corner.
    pub const fn with(self, badge: Badge) -> Icon {
        Icon { base: self.base, badge }
    }
}

/// A pen over a unit square: coordinates run 0 to 1 from the top left.
struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    ink: Color32,
    acc: Color32,
    fill: Color32,
    on_acc: Color32,
    w: f32,
}

type P = (f32, f32);

impl Pen<'_> {
    fn pt(&self, q: P) -> Pos2 {
        pos2(self.r.min.x + q.0 * self.r.width(), self.r.min.y + q.1 * self.r.height())
    }

    fn pts(&self, q: &[P]) -> Vec<Pos2> {
        q.iter().map(|q| self.pt(*q)).collect()
    }

    fn stroke(&self) -> Stroke {
        Stroke::new(self.w, self.ink)
    }

    fn acc_stroke(&self) -> Stroke {
        Stroke::new(self.w, self.acc)
    }

    fn line(&self, a: P, b: P) {
        self.p.line_segment([self.pt(a), self.pt(b)], self.stroke());
    }

    fn acc_line(&self, a: P, b: P) {
        self.p.line_segment([self.pt(a), self.pt(b)], self.acc_stroke());
    }

    fn path(&self, q: &[P]) {
        self.p.add(Shape::line(self.pts(q), self.stroke()));
    }

    fn acc_path(&self, q: &[P]) {
        self.p.add(Shape::line(self.pts(q), self.acc_stroke()));
    }

    fn poly(&self, q: &[P], fill: Color32) {
        self.p.add(Shape::convex_polygon(self.pts(q), fill, self.stroke()));
    }

    fn rect(&self, a: P, b: P, fill: Color32) {
        let rr = Rect::from_two_pos(self.pt(a), self.pt(b));
        self.p.rect(rr, CornerRadius::same(1), fill, self.stroke(), StrokeKind::Inside);
    }

    fn acc_rect(&self, a: P, b: P) {
        let rr = Rect::from_two_pos(self.pt(a), self.pt(b));
        self.p.rect(rr, CornerRadius::same(1), self.acc, Stroke::NONE, StrokeKind::Inside);
    }

    fn circle(&self, c: P, radius: f32, fill: Color32) {
        self.p.circle(self.pt(c), radius * self.r.width(), fill, self.stroke());
    }

    fn acc_circle(&self, c: P, radius: f32) {
        self.p.circle_filled(self.pt(c), radius * self.r.width(), self.acc);
    }

    fn text(&self, c: P, s: &str, size: f32, colour: Color32) {
        self.p.text(self.pt(c), Align2::CENTER_CENTER, s, theme::font(size * self.r.height()), colour);
    }

    /// Dashes along the outline of a rectangle.
    fn dashed(&self, a: P, b: P) {
        let rr = Rect::from_two_pos(self.pt(a), self.pt(b));
        let corners = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
        self.p.extend(Shape::dashed_line(&corners, self.stroke(), 2.4, 1.8));
    }

    /// A dashed straight line.
    fn dashed_line(&self, a: P, b: P) {
        self.p.extend(Shape::dashed_line(&[self.pt(a), self.pt(b)], self.stroke(), 2.4, 1.8));
    }

    fn ellipse_pts(&self, c: P, rx: f32, ry: f32, from: f32, to: f32) -> Vec<P> {
        let steps = 20;
        (0..=steps)
            .map(|i| {
                let a = from + (to - from) * i as f32 / steps as f32;
                (c.0 + rx * a.cos(), c.1 + ry * a.sin())
            })
            .collect()
    }
}

/// Paints `icon` into `rect`. `enabled` false draws it quiet, from the disabled colours.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, t: &Tokens, enabled: bool) {
    let (ink, acc, fill) =
        if enabled { (t.icon_ink, t.icon_accent, t.icon_fill) } else { (t.icon_ink_disabled, t.icon_ink_disabled, t.icon_fill_disabled) };
    // The drawing sits in the middle of the box, a little in from its edge.
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), egui::vec2(side, side)).shrink(side * 0.06);
    let w = (side / 16.0).clamp(1.0, 1.5);
    let pen = Pen { p: painter, r: square, ink, acc, fill, on_acc: t.accent_text, w };
    draw_base(&pen, icon.base);
    draw_badge(&pen, icon.badge);
}

/// Stencil contours stay 1.35 logical points at every display scale.
pub(crate) const STENCIL_STROKE_WIDTH: f32 = 1.35;

/// Original stencil-only geometry, optically balanced within a 34-point tile figure.
/// The ribbon keeps its own geometry, colours, fills and stroke weight in [`paint`].
pub(crate) fn paint_stencil(painter: &Painter, rect: Rect, base: Base, t: &Tokens, enabled: bool) {
    let ink = if enabled { t.stencil_ink } else { t.stencil_ink_disabled };
    let stroke = Stroke::new(STENCIL_STROKE_WIDTH, ink);
    let scale = rect.width().min(rect.height()) / 34.0;
    let at = |(x, y): P| rect.center() + egui::vec2(x, y) * scale;
    let polygon = |points: &[P]| {
        painter.add(Shape::closed_line(points.iter().copied().map(at).collect(), stroke));
    };
    match base {
        Base::Rect | Base::Square => {
            let (w, h) = if base == Base::Square { (21.0, 21.0) } else { (26.0, 18.0) };
            painter.rect(Rect::from_center_size(rect.center(), egui::vec2(w, h) * scale), 0.5, t.stencil_fill, stroke, StrokeKind::Middle);
        }
        Base::Circle => {
            painter.circle(rect.center(), 11.5 * scale, t.stencil_fill, stroke);
        }
        Base::Ellipse => {
            painter.add(Shape::ellipse_stroke(rect.center(), egui::vec2(13.0, 9.0) * scale, stroke));
        }
        Base::Triangle => polygon(&[(0.0, -13.0), (13.0, 10.0), (-13.0, 10.0)]),
        Base::RightTriangle => polygon(&[(-11.0, -12.0), (-11.0, 11.0), (12.0, 11.0)]),
        Base::Diamond => polygon(&[(0.0, -14.0), (13.0, 0.0), (0.0, 14.0), (-13.0, 0.0)]),
        Base::Pentagon => polygon(&[(0.0, -13.0), (12.36, -4.02), (7.64, 10.52), (-7.64, 10.52), (-12.36, -4.02)]),
        Base::Hexagon => polygon(&[(-6.5, -11.25), (6.5, -11.25), (13.0, 0.0), (6.5, 11.25), (-6.5, 11.25), (-13.0, 0.0)]),
        Base::Octagon => {
            polygon(&[(-4.76, -11.5), (4.76, -11.5), (11.5, -4.76), (11.5, 4.76), (4.76, 11.5), (-4.76, 11.5), (-11.5, 4.76), (-11.5, -4.76)])
        }
        Base::Parallelogram => polygon(&[(-7.5, -9.0), (13.0, -9.0), (7.5, 9.0), (-13.0, 9.0)]),
        Base::Trapezoid => polygon(&[(-7.5, -9.0), (7.5, -9.0), (13.0, 9.0), (-13.0, 9.0)]),
        Base::Cylinder => {
            painter.add(Shape::ellipse_stroke(at((0.0, -8.0)), egui::vec2(11.5, 4.0) * scale, stroke));
            painter.line_segment([at((-11.5, -8.0)), at((-11.5, 8.0))], stroke);
            painter.line_segment([at((11.5, -8.0)), at((11.5, 8.0))], stroke);
            let bottom = (0..=32)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::PI / 32.0;
                    at((11.5 * angle.cos(), 8.0 + 4.0 * angle.sin()))
                })
                .collect();
            painter.add(Shape::line(bottom, stroke));
        }
        _ => {
            let side = rect.width().min(rect.height());
            let pen = Pen {
                p: painter,
                r: Rect::from_center_size(rect.center(), egui::vec2(side, side)).shrink(side * 0.06),
                ink,
                acc: ink,
                fill: t.stencil_fill,
                on_acc: ink,
                w: STENCIL_STROKE_WIDTH,
            };
            draw_shapes(&pen, base);
        }
    }
}

fn draw_badge(pen: &Pen<'_>, badge: Badge) {
    if badge == Badge::None {
        return;
    }
    let c = (0.76, 0.76);
    pen.acc_circle(c, 0.2);
    let k = 0.09;
    let w = Stroke::new((pen.w * 0.9).max(1.0), pen.on_acc);
    let seg = |a: P, b: P| {
        pen.p.line_segment([pen.pt(a), pen.pt(b)], w);
    };
    match badge {
        Badge::Plus => {
            seg((c.0 - k, c.1), (c.0 + k, c.1));
            seg((c.0, c.1 - k), (c.0, c.1 + k));
        }
        Badge::Minus => seg((c.0 - k, c.1), (c.0 + k, c.1)),
        Badge::Check => {
            seg((c.0 - k, c.1), (c.0 - k * 0.3, c.1 + k * 0.8));
            seg((c.0 - k * 0.3, c.1 + k * 0.8), (c.0 + k, c.1 - k * 0.8));
        }
        Badge::Cross => {
            seg((c.0 - k * 0.8, c.1 - k * 0.8), (c.0 + k * 0.8, c.1 + k * 0.8));
            seg((c.0 - k * 0.8, c.1 + k * 0.8), (c.0 + k * 0.8, c.1 - k * 0.8));
        }
        Badge::Arrow => {
            seg((c.0 - k, c.1), (c.0 + k, c.1));
            seg((c.0 + k * 0.2, c.1 - k * 0.8), (c.0 + k, c.1));
            seg((c.0 + k * 0.2, c.1 + k * 0.8), (c.0 + k, c.1));
        }
        Badge::Dot => {
            pen.p.circle_filled(pen.pt(c), pen.r.width() * 0.05, pen.on_acc);
        }
        Badge::Question => pen.text(c, "?", 0.3, pen.on_acc),
        Badge::None => {}
    }
}

fn draw_base(pen: &Pen<'_>, base: Base) {
    match base {
        Base::Page | Base::Pages | Base::Clipboard | Base::Scissors | Base::Brush | Base::Pointer | Base::Connector => draw_basic(pen, base),
        Base::TextA
        | Base::TextBlock
        | Base::Bold
        | Base::Italic
        | Base::Underline
        | Base::Strike
        | Base::FontUp
        | Base::FontDown
        | Base::CaseAa
        | Base::FontColor
        | Base::Symbol
        | Base::Mark => draw_type(pen, base),
        Base::Bullets
        | Base::TextLeft
        | Base::TextCenter
        | Base::TextRight
        | Base::TextJustify
        | Base::TextTop
        | Base::TextMiddle
        | Base::TextBottom
        | Base::IndentLess
        | Base::IndentMore
        | Base::TextTurn
        | Base::ArrowLeft
        | Base::ArrowRight => draw_lines(pen, base),
        Base::Rect
        | Base::Ellipse
        | Base::Line
        | Base::Freeform
        | Base::Arc
        | Base::Pencil
        | Base::Dot
        | Base::Triangle
        | Base::Diamond
        | Base::Pentagon
        | Base::Hexagon
        | Base::Octagon
        | Base::RightTriangle
        | Base::Parallelogram
        | Base::Trapezoid
        | Base::Cross
        | Base::Arrow
        | Base::Cylinder
        | Base::Star
        | Base::Square
        | Base::Circle => draw_shapes(pen, base),
        Base::Undo
        | Base::Redo
        | Base::Save
        | Base::FolderOpen
        | Base::Collapse
        | Base::Refresh
        | Base::Play
        | Base::Help
        | Base::Person
        | Base::Plus
        | Base::Minus => draw_app(pen, base),
        _ => draw_misc(pen, base),
    }
}

fn draw_basic(pen: &Pen<'_>, base: Base) {
    match base {
        Base::Page => {
            pen.poly(&[(0.22, 0.08), (0.62, 0.08), (0.8, 0.26), (0.8, 0.92), (0.22, 0.92)], pen.fill);
            pen.path(&[(0.62, 0.08), (0.62, 0.26), (0.8, 0.26)]);
            pen.line((0.32, 0.5), (0.7, 0.5));
            pen.line((0.32, 0.66), (0.7, 0.66));
        }
        Base::Pages => {
            pen.rect((0.12, 0.22), (0.58, 0.92), pen.fill);
            pen.rect((0.38, 0.08), (0.88, 0.78), pen.fill);
            pen.line((0.5, 0.28), (0.78, 0.28));
            pen.line((0.5, 0.42), (0.78, 0.42));
        }
        Base::Clipboard => {
            pen.rect((0.2, 0.14), (0.8, 0.92), pen.fill);
            pen.acc_rect((0.36, 0.06), (0.64, 0.26));
            pen.line((0.32, 0.5), (0.68, 0.5));
            pen.line((0.32, 0.66), (0.68, 0.66));
        }
        Base::Scissors => {
            pen.line((0.3, 0.08), (0.64, 0.66));
            pen.line((0.7, 0.08), (0.36, 0.66));
            pen.circle((0.3, 0.8), 0.1, pen.fill);
            pen.circle((0.7, 0.8), 0.1, pen.fill);
        }
        Base::Brush => {
            pen.acc_rect((0.14, 0.1), (0.7, 0.34));
            pen.line((0.7, 0.22), (0.86, 0.22));
            pen.path(&[(0.86, 0.22), (0.86, 0.5), (0.5, 0.5), (0.5, 0.62)]);
            pen.rect((0.42, 0.62), (0.58, 0.92), pen.fill);
        }
        Base::Pointer => {
            pen.poly(&[(0.26, 0.08), (0.26, 0.82), (0.44, 0.65), (0.57, 0.92), (0.68, 0.86), (0.55, 0.6), (0.78, 0.6)], pen.fill);
        }
        _ => {
            pen.rect((0.08, 0.12), (0.38, 0.4), pen.fill);
            pen.rect((0.62, 0.6), (0.92, 0.88), pen.fill);
            pen.acc_path(&[(0.38, 0.26), (0.77, 0.26), (0.77, 0.6)]);
        }
    }
}

fn draw_type(pen: &Pen<'_>, base: Base) {
    let c = (0.5, 0.5);
    match base {
        Base::TextA => pen.text(c, "A", 0.86, pen.ink),
        Base::TextBlock => {
            pen.dashed((0.08, 0.22), (0.92, 0.78));
            pen.text(c, "A", 0.5, pen.ink);
        }
        Base::Bold => {
            pen.text((0.5, 0.5), "B", 0.9, pen.ink);
            pen.text((0.54, 0.5), "B", 0.9, pen.ink);
        }
        Base::Italic => {
            pen.line((0.4, 0.88), (0.64, 0.12));
            pen.line((0.32, 0.88), (0.48, 0.88));
            pen.line((0.56, 0.12), (0.72, 0.12));
        }
        Base::Underline => {
            pen.text((0.5, 0.42), "U", 0.78, pen.ink);
            pen.line((0.2, 0.88), (0.8, 0.88));
        }
        Base::Strike => {
            pen.text((0.5, 0.5), "S", 0.8, pen.ink);
            pen.line((0.16, 0.52), (0.84, 0.52));
        }
        Base::FontUp => {
            pen.text((0.4, 0.58), "A", 0.78, pen.ink);
            pen.acc_path(&[(0.72, 0.4), (0.84, 0.24), (0.96, 0.4)]);
        }
        Base::FontDown => {
            pen.text((0.4, 0.5), "A", 0.62, pen.ink);
            pen.acc_path(&[(0.72, 0.64), (0.84, 0.8), (0.96, 0.64)]);
        }
        Base::CaseAa => {
            pen.text((0.34, 0.5), "A", 0.78, pen.ink);
            pen.text((0.74, 0.58), "a", 0.58, pen.ink);
        }
        Base::FontColor => {
            pen.text((0.5, 0.42), "A", 0.78, pen.ink);
            pen.acc_rect((0.16, 0.8), (0.84, 0.94));
        }
        Base::Symbol => pen.text(c, "\u{03a9}", 0.86, pen.ink),
        _ => {
            pen.text(c, "#", 0.86, pen.ink);
        }
    }
}

fn draw_lines(pen: &Pen<'_>, base: Base) {
    let bars = |xs: [(f32, f32); 4]| {
        for (i, (a, b)) in xs.iter().enumerate() {
            let y = 0.22 + 0.19 * i as f32;
            pen.line((*a, y), (*b, y));
        }
    };
    match base {
        Base::TextLeft => bars([(0.14, 0.86), (0.14, 0.62), (0.14, 0.86), (0.14, 0.5)]),
        Base::TextCenter => bars([(0.14, 0.86), (0.26, 0.74), (0.14, 0.86), (0.3, 0.7)]),
        Base::TextRight => bars([(0.14, 0.86), (0.38, 0.86), (0.14, 0.86), (0.5, 0.86)]),
        Base::TextJustify => bars([(0.14, 0.86), (0.14, 0.86), (0.14, 0.86), (0.14, 0.86)]),
        Base::TextTop | Base::TextMiddle | Base::TextBottom => {
            let y = match base {
                Base::TextTop => 0.18,
                Base::TextMiddle => 0.5,
                _ => 0.82,
            };
            pen.rect((0.12, 0.08), (0.88, 0.92), pen.fill);
            pen.acc_line((0.26, y), (0.74, y));
            pen.acc_line((0.3, y + if y < 0.5 { 0.14 } else { -0.14 }), (0.7, y + if y < 0.5 { 0.14 } else { -0.14 }));
        }
        Base::Bullets => {
            for i in 0..3 {
                let y = 0.22 + 0.28 * i as f32;
                pen.acc_circle((0.18, y), 0.06);
                pen.line((0.36, y), (0.88, y));
            }
        }
        Base::IndentLess | Base::IndentMore => {
            let a = if base == Base::IndentLess { 0.32 } else { 0.12 };
            pen.line((0.5, 0.18), (0.9, 0.18));
            pen.line((0.5, 0.5), (0.9, 0.5));
            pen.line((0.5, 0.82), (0.9, 0.82));
            pen.acc_line((a, 0.5), (a + 0.2, 0.5));
            pen.acc_path(&[
                (a + if base == Base::IndentLess { 0.06 } else { 0.14 }, 0.4),
                (if base == Base::IndentLess { a } else { a + 0.2 }, 0.5),
                (a + if base == Base::IndentLess { 0.06 } else { 0.14 }, 0.6),
            ]);
        }
        Base::TextTurn => {
            pen.text((0.4, 0.5), "A", 0.7, pen.ink);
            pen.acc_path(&[(0.78, 0.2), (0.9, 0.5), (0.78, 0.8)]);
        }
        Base::ArrowLeft | Base::ArrowRight => {
            let (a, b) = if base == Base::ArrowLeft { (0.84, 0.16) } else { (0.16, 0.84) };
            pen.line((a, 0.5), (b, 0.5));
            let d = if base == Base::ArrowLeft { 0.16 } else { -0.16 };
            pen.path(&[(b + d, 0.32), (b, 0.5), (b + d, 0.68)]);
        }
        _ => {}
    }
}

fn draw_shapes(pen: &Pen<'_>, base: Base) {
    match base {
        Base::Rect => pen.rect((0.1, 0.2), (0.9, 0.8), pen.fill),
        Base::Square => pen.rect((0.2, 0.2), (0.8, 0.8), pen.fill),
        Base::Circle => {
            let pts = pen.ellipse_pts((0.5, 0.5), 0.36, 0.36, 0.0, std::f32::consts::TAU);
            pen.poly(&pts, pen.fill);
        }
        Base::Ellipse => {
            let pts = pen.ellipse_pts((0.5, 0.5), 0.4, 0.3, 0.0, std::f32::consts::TAU);
            pen.poly(&pts, pen.fill);
        }
        Base::Line => pen.line((0.12, 0.88), (0.88, 0.12)),
        Base::Freeform => pen.path(&[(0.1, 0.7), (0.26, 0.3), (0.42, 0.7), (0.58, 0.3), (0.74, 0.7), (0.9, 0.3)]),
        Base::Arc => {
            let pts = pen.ellipse_pts((0.12, 0.88), 0.76, 0.76, -std::f32::consts::FRAC_PI_2, 0.0);
            pen.path(&pts);
        }
        Base::Pencil => {
            pen.poly(&[(0.7, 0.08), (0.92, 0.3), (0.34, 0.88), (0.1, 0.9), (0.12, 0.66)], pen.fill);
            pen.acc_line((0.58, 0.2), (0.8, 0.42));
        }
        Base::Dot => {
            pen.line((0.5, 0.1), (0.5, 0.9));
            pen.line((0.1, 0.5), (0.9, 0.5));
            pen.acc_circle((0.5, 0.5), 0.1);
        }
        Base::Triangle => pen.poly(&[(0.5, 0.1), (0.92, 0.86), (0.08, 0.86)], pen.fill),
        Base::RightTriangle => pen.poly(&[(0.14, 0.12), (0.14, 0.86), (0.88, 0.86)], pen.fill),
        Base::Diamond => pen.poly(&[(0.5, 0.06), (0.92, 0.5), (0.5, 0.94), (0.08, 0.5)], pen.fill),
        Base::Pentagon => pen.poly(&[(0.5, 0.08), (0.92, 0.4), (0.76, 0.88), (0.24, 0.88), (0.08, 0.4)], pen.fill),
        Base::Hexagon => pen.poly(&[(0.26, 0.14), (0.74, 0.14), (0.94, 0.5), (0.74, 0.86), (0.26, 0.86), (0.06, 0.5)], pen.fill),
        Base::Octagon => {
            pen.poly(&[(0.32, 0.08), (0.68, 0.08), (0.92, 0.32), (0.92, 0.68), (0.68, 0.92), (0.32, 0.92), (0.08, 0.68), (0.08, 0.32)], pen.fill)
        }
        Base::Parallelogram => pen.poly(&[(0.3, 0.2), (0.94, 0.2), (0.7, 0.8), (0.06, 0.8)], pen.fill),
        Base::Trapezoid => pen.poly(&[(0.28, 0.2), (0.72, 0.2), (0.94, 0.8), (0.06, 0.8)], pen.fill),
        Base::Cross => pen.poly(
            &[
                (0.34, 0.08),
                (0.66, 0.08),
                (0.66, 0.34),
                (0.92, 0.34),
                (0.92, 0.66),
                (0.66, 0.66),
                (0.66, 0.92),
                (0.34, 0.92),
                (0.34, 0.66),
                (0.08, 0.66),
                (0.08, 0.34),
                (0.34, 0.34),
            ],
            pen.fill,
        ),
        Base::Arrow => pen.poly(&[(0.08, 0.36), (0.56, 0.36), (0.56, 0.14), (0.94, 0.5), (0.56, 0.86), (0.56, 0.64), (0.08, 0.64)], pen.fill),
        Base::Cylinder => {
            let top = pen.ellipse_pts((0.5, 0.22), 0.38, 0.13, 0.0, std::f32::consts::TAU);
            pen.poly(&top, pen.fill);
            pen.line((0.12, 0.22), (0.12, 0.78));
            pen.line((0.88, 0.22), (0.88, 0.78));
            let bottom = pen.ellipse_pts((0.5, 0.78), 0.38, 0.13, 0.0, std::f32::consts::PI);
            pen.path(&bottom);
        }
        Base::Star => {
            let mut v = Vec::new();
            for i in 0..10 {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let r = if i % 2 == 0 { 0.42 } else { 0.18 };
                v.push((0.5 + r * a.cos(), 0.52 + r * a.sin()));
            }
            pen.poly(&v, pen.fill);
        }
        _ => {}
    }
}

fn draw_app(pen: &Pen<'_>, base: Base) {
    match base {
        Base::Undo | Base::Redo => {
            let flip = base == Base::Redo;
            let x = |v: f32| if flip { 1.0 - v } else { v };
            let arc: Vec<P> = pen
                .ellipse_pts((0.5, 0.62), 0.32, 0.3, std::f32::consts::PI, std::f32::consts::TAU * 0.95)
                .into_iter()
                .map(|(a, b)| (x(a), b))
                .collect();
            pen.path(&arc);
            pen.path(&[(x(0.18), 0.4), (x(0.18), 0.64), (x(0.42), 0.64)]);
        }
        Base::Save => {
            pen.poly(&[(0.12, 0.12), (0.76, 0.12), (0.88, 0.24), (0.88, 0.88), (0.12, 0.88)], pen.fill);
            pen.rect((0.3, 0.12), (0.64, 0.38), pen.fill);
            pen.acc_rect((0.28, 0.58), (0.72, 0.88));
        }
        Base::FolderOpen => {
            pen.poly(&[(0.08, 0.2), (0.38, 0.2), (0.46, 0.3), (0.86, 0.3), (0.86, 0.84), (0.08, 0.84)], pen.fill);
            pen.line((0.08, 0.5), (0.86, 0.5));
        }
        Base::Collapse => pen.path(&[(0.2, 0.62), (0.5, 0.34), (0.8, 0.62)]),
        Base::Plus | Base::Minus => {
            pen.circle((0.5, 0.5), 0.4, pen.fill);
            pen.line((0.28, 0.5), (0.72, 0.5));
            if base == Base::Plus {
                pen.line((0.5, 0.28), (0.5, 0.72));
            }
        }
        Base::Refresh => {
            let arc = pen.ellipse_pts((0.5, 0.5), 0.34, 0.34, 0.3, std::f32::consts::TAU - 0.5);
            pen.path(&arc);
            pen.acc_path(&[(0.74, 0.1), (0.8, 0.34), (0.56, 0.34)]);
        }
        Base::Play => pen.poly(&[(0.28, 0.14), (0.28, 0.86), (0.84, 0.5)], pen.fill),
        Base::Help => {
            pen.circle((0.5, 0.5), 0.4, pen.fill);
            pen.text((0.5, 0.52), "?", 0.7, pen.ink);
        }
        _ => {
            pen.circle((0.5, 0.32), 0.18, pen.fill);
            let body = pen.ellipse_pts((0.5, 0.98), 0.36, 0.4, std::f32::consts::PI, std::f32::consts::TAU);
            pen.path(&body);
        }
    }
}

fn draw_misc(pen: &Pen<'_>, base: Base) {
    match base {
        Base::PagePortrait => {
            pen.rect((0.26, 0.08), (0.74, 0.92), pen.fill);
            pen.line((0.36, 0.34), (0.64, 0.34));
            pen.line((0.36, 0.5), (0.64, 0.5));
            pen.acc_rect((0.36, 0.66), (0.64, 0.8));
        }
        Base::PageLandscape => {
            pen.rect((0.08, 0.26), (0.92, 0.74), pen.fill);
            pen.line((0.2, 0.4), (0.8, 0.4));
            pen.line((0.2, 0.52), (0.64, 0.52));
            pen.acc_rect((0.2, 0.6), (0.4, 0.66));
        }
        Base::Printer => {
            pen.rect((0.28, 0.1), (0.72, 0.36), pen.fill);
            pen.rect((0.1, 0.34), (0.9, 0.7), pen.fill);
            pen.rect((0.28, 0.58), (0.72, 0.92), pen.fill);
            pen.line((0.36, 0.72), (0.64, 0.72));
            pen.line((0.36, 0.82), (0.56, 0.82));
            pen.acc_circle((0.8, 0.46), 0.04);
        }
        Base::Swatches => {
            pen.acc_rect((0.1, 0.1), (0.46, 0.46));
            pen.rect((0.54, 0.1), (0.9, 0.46), pen.fill);
            pen.rect((0.1, 0.54), (0.46, 0.9), pen.fill);
            pen.rect((0.54, 0.54), (0.9, 0.9), pen.fill);
        }
        Base::Bucket => {
            pen.poly(&[(0.18, 0.46), (0.5, 0.14), (0.82, 0.46), (0.5, 0.78)], pen.fill);
            pen.acc_circle((0.86, 0.72), 0.07);
            pen.acc_rect((0.1, 0.86), (0.9, 0.96));
        }
        Base::PenLine => {
            pen.line((0.1, 0.9), (0.6, 0.9));
            pen.poly(&[(0.66, 0.1), (0.9, 0.34), (0.36, 0.74), (0.2, 0.72), (0.22, 0.56)], pen.fill);
        }
        Base::Shadow => {
            pen.acc_rect((0.3, 0.3), (0.92, 0.92));
            pen.rect((0.1, 0.1), (0.7, 0.7), pen.fill);
        }
        Base::Align => {
            pen.line((0.14, 0.08), (0.14, 0.92));
            pen.acc_rect((0.22, 0.2), (0.84, 0.4));
            pen.acc_rect((0.22, 0.6), (0.58, 0.8));
        }
        Base::Move => {
            pen.line((0.5, 0.1), (0.5, 0.9));
            pen.line((0.1, 0.5), (0.9, 0.5));
            pen.path(&[(0.4, 0.22), (0.5, 0.1), (0.6, 0.22)]);
            pen.path(&[(0.4, 0.78), (0.5, 0.9), (0.6, 0.78)]);
            pen.path(&[(0.22, 0.4), (0.1, 0.5), (0.22, 0.6)]);
            pen.path(&[(0.78, 0.4), (0.9, 0.5), (0.78, 0.6)]);
        }
        Base::Front | Base::Back => {
            let (a, b) = if base == Base::Front { (pen.fill, pen.acc) } else { (pen.acc, pen.fill) };
            pen.rect((0.1, 0.1), (0.62, 0.62), a);
            pen.rect((0.38, 0.38), (0.9, 0.9), b);
        }
        Base::Group => {
            pen.dashed((0.06, 0.06), (0.94, 0.94));
            pen.rect((0.18, 0.2), (0.48, 0.5), pen.fill);
            pen.rect((0.52, 0.5), (0.82, 0.8), pen.fill);
        }
        Base::Swap => {
            pen.line((0.12, 0.34), (0.82, 0.34));
            pen.path(&[(0.68, 0.2), (0.84, 0.34), (0.68, 0.48)]);
            pen.line((0.88, 0.7), (0.18, 0.7));
            pen.path(&[(0.32, 0.56), (0.16, 0.7), (0.32, 0.84)]);
        }
        Base::Magnifier => {
            pen.circle((0.42, 0.42), 0.28, pen.fill);
            pen.line((0.62, 0.62), (0.9, 0.9));
        }
        Base::Layers => {
            pen.poly(&[(0.5, 0.08), (0.94, 0.3), (0.5, 0.52), (0.06, 0.3)], pen.fill);
            pen.path(&[(0.06, 0.5), (0.5, 0.72), (0.94, 0.5)]);
            pen.path(&[(0.06, 0.7), (0.5, 0.92), (0.94, 0.7)]);
        }
        Base::Select => {
            pen.dashed((0.08, 0.08), (0.7, 0.7));
            pen.poly(&[(0.42, 0.4), (0.42, 0.9), (0.55, 0.78), (0.64, 0.96), (0.72, 0.92), (0.63, 0.74), (0.8, 0.74)], pen.fill);
        }
        Base::Picture => {
            pen.rect((0.08, 0.16), (0.92, 0.84), pen.fill);
            pen.acc_circle((0.32, 0.38), 0.08);
            pen.path(&[(0.12, 0.78), (0.4, 0.5), (0.58, 0.68), (0.72, 0.54), (0.9, 0.76)]);
        }
        Base::Chart => {
            pen.path(&[(0.1, 0.1), (0.1, 0.9), (0.92, 0.9)]);
            pen.acc_rect((0.22, 0.52), (0.36, 0.86));
            pen.acc_rect((0.45, 0.3), (0.59, 0.86));
            pen.acc_rect((0.68, 0.42), (0.82, 0.86));
        }
        Base::Container => {
            pen.dashed((0.06, 0.12), (0.94, 0.88));
            pen.rect((0.18, 0.4), (0.42, 0.72), pen.fill);
            pen.rect((0.56, 0.4), (0.8, 0.72), pen.fill);
            pen.acc_rect((0.06, 0.12), (0.94, 0.26));
        }
        Base::Callout | Base::Bubble => {
            pen.poly(&[(0.08, 0.14), (0.92, 0.14), (0.92, 0.66), (0.5, 0.66), (0.26, 0.92), (0.3, 0.66), (0.08, 0.66)], pen.fill);
            pen.line((0.24, 0.32), (0.76, 0.32));
            pen.line((0.24, 0.48), (0.6, 0.48));
        }
        Base::Link => {
            pen.rect((0.06, 0.34), (0.54, 0.66), pen.fill);
            pen.rect((0.46, 0.34), (0.94, 0.66), pen.fill);
            pen.acc_line((0.36, 0.5), (0.64, 0.5));
        }
        Base::Palette => {
            let o = pen.ellipse_pts((0.5, 0.5), 0.42, 0.38, 0.0, std::f32::consts::TAU);
            pen.poly(&o, pen.fill);
            pen.acc_circle((0.32, 0.42), 0.07);
            pen.acc_circle((0.5, 0.28), 0.07);
            pen.acc_circle((0.68, 0.42), 0.07);
        }
        Base::Resize => {
            pen.rect((0.14, 0.14), (0.7, 0.7), pen.fill);
            pen.acc_line((0.48, 0.92), (0.92, 0.48));
            pen.path(&[(0.92, 0.7), (0.92, 0.48), (0.7, 0.48)]);
        }
        _ => draw_misc_more(pen, base),
    }
}

fn draw_misc_more(pen: &Pen<'_>, base: Base) {
    match base {
        Base::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                pen.line((0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()), (0.5 + 0.44 * a.cos(), 0.5 + 0.44 * a.sin()));
            }
            pen.circle((0.5, 0.5), 0.3, pen.fill);
            pen.acc_circle((0.5, 0.5), 0.1);
        }
        Base::Window => {
            pen.rect((0.08, 0.14), (0.92, 0.86), pen.fill);
            pen.acc_rect((0.08, 0.14), (0.92, 0.3));
        }
        Base::Eye => {
            let top = pen.ellipse_pts((0.5, 0.78), 0.44, 0.5, std::f32::consts::PI * 1.2, std::f32::consts::PI * 1.8);
            pen.path(&top);
            let bottom = pen.ellipse_pts((0.5, 0.22), 0.44, 0.5, std::f32::consts::PI * 0.2, std::f32::consts::PI * 0.8);
            pen.path(&bottom);
            pen.acc_circle((0.5, 0.5), 0.12);
        }
        Base::Fit => {
            for (cx, cy) in [(0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9)] {
                let (dx, dy) = (if cx < 0.5 { 0.24 } else { -0.24 }, if cy < 0.5 { 0.24 } else { -0.24 });
                pen.path(&[(cx + dx, cy), (cx, cy), (cx, cy + dy)]);
            }
            pen.acc_rect((0.34, 0.34), (0.66, 0.66));
        }
        Base::Spelling => {
            pen.text((0.4, 0.34), "abc", 0.4, pen.ink);
            pen.acc_path(&[(0.14, 0.74), (0.36, 0.92), (0.86, 0.5)]);
        }
        Base::Book => {
            pen.poly(&[(0.5, 0.2), (0.5, 0.9), (0.08, 0.82), (0.08, 0.12)], pen.fill);
            pen.poly(&[(0.5, 0.2), (0.5, 0.9), (0.92, 0.82), (0.92, 0.12)], pen.fill);
        }
        Base::Globe => {
            let o = pen.ellipse_pts((0.5, 0.5), 0.4, 0.4, 0.0, std::f32::consts::TAU);
            pen.poly(&o, pen.fill);
            pen.line((0.1, 0.5), (0.9, 0.5));
            let m = pen.ellipse_pts((0.5, 0.5), 0.18, 0.4, 0.0, std::f32::consts::TAU);
            pen.acc_path(&m);
        }
        Base::Ruler => {
            pen.rect((0.06, 0.34), (0.94, 0.7), pen.fill);
            for i in 0..6 {
                let x = 0.16 + 0.14 * i as f32;
                pen.line((x, 0.34), (x, if i % 2 == 0 { 0.54 } else { 0.46 }));
            }
        }
        Base::Grid => {
            pen.rect((0.1, 0.1), (0.9, 0.9), pen.fill);
            pen.line((0.37, 0.1), (0.37, 0.9));
            pen.line((0.63, 0.1), (0.63, 0.9));
            pen.line((0.1, 0.37), (0.9, 0.37));
            pen.line((0.1, 0.63), (0.9, 0.63));
        }
        Base::Magnet => {
            let arc = pen.ellipse_pts((0.5, 0.5), 0.32, 0.34, std::f32::consts::PI, std::f32::consts::TAU);
            pen.path(&arc);
            pen.line((0.18, 0.5), (0.18, 0.82));
            pen.line((0.82, 0.5), (0.82, 0.82));
            pen.acc_rect((0.1, 0.78), (0.26, 0.92));
            pen.acc_rect((0.74, 0.78), (0.9, 0.92));
        }
        Base::Database => {
            let top = pen.ellipse_pts((0.5, 0.24), 0.36, 0.12, 0.0, std::f32::consts::TAU);
            pen.poly(&top, pen.fill);
            pen.line((0.14, 0.24), (0.14, 0.78));
            pen.line((0.86, 0.24), (0.86, 0.78));
            pen.path(&pen.ellipse_pts((0.5, 0.5), 0.36, 0.12, 0.0, std::f32::consts::PI));
            pen.path(&pen.ellipse_pts((0.5, 0.78), 0.36, 0.12, 0.0, std::f32::consts::PI));
        }
        Base::Table => {
            pen.rect((0.08, 0.14), (0.92, 0.86), pen.fill);
            pen.acc_rect((0.08, 0.14), (0.92, 0.32));
            pen.line((0.5, 0.14), (0.5, 0.86));
            pen.line((0.08, 0.59), (0.92, 0.59));
        }
        Base::TurnLeft | Base::TurnRight => {
            // A box and an arc over it that ends in the head of an arrow: right runs the arc clockwise, left is its mirror image.
            let mirror = |q: P| if base == Base::TurnLeft { (1.0 - q.0, q.1) } else { q };
            pen.rect((0.28, 0.54), (0.72, 0.92), pen.fill);
            let arc = pen.ellipse_pts((0.5, 0.5), 0.38, 0.34, -2.8, -0.35);
            let tip = arc.last().copied().unwrap_or((0.85, 0.38));
            let arc: Vec<P> = arc.into_iter().map(mirror).collect();
            pen.acc_path(&arc);
            pen.acc_path(&[mirror((tip.0 - 0.17, tip.1)), mirror(tip), mirror((tip.0, tip.1 - 0.17))]);
        }
        Base::AlignCenter => {
            pen.line((0.5, 0.08), (0.5, 0.92));
            pen.acc_rect((0.16, 0.2), (0.84, 0.4));
            pen.acc_rect((0.3, 0.6), (0.7, 0.8));
        }
        Base::AlignRight => {
            pen.line((0.86, 0.08), (0.86, 0.92));
            pen.acc_rect((0.16, 0.2), (0.78, 0.4));
            pen.acc_rect((0.42, 0.6), (0.78, 0.8));
        }
        Base::AlignTop => {
            pen.line((0.08, 0.14), (0.92, 0.14));
            pen.acc_rect((0.2, 0.22), (0.4, 0.84));
            pen.acc_rect((0.6, 0.22), (0.8, 0.58));
        }
        Base::AlignMiddle => {
            pen.line((0.08, 0.5), (0.92, 0.5));
            pen.acc_rect((0.2, 0.16), (0.4, 0.84));
            pen.acc_rect((0.6, 0.3), (0.8, 0.7));
        }
        Base::AlignBottom => {
            pen.line((0.08, 0.86), (0.92, 0.86));
            pen.acc_rect((0.2, 0.16), (0.4, 0.78));
            pen.acc_rect((0.6, 0.42), (0.8, 0.78));
        }
        Base::FlipH => {
            pen.dashed_line((0.5, 0.06), (0.5, 0.94));
            pen.poly(&[(0.4, 0.2), (0.4, 0.8), (0.08, 0.8)], pen.fill);
            pen.poly(&[(0.6, 0.2), (0.6, 0.8), (0.92, 0.8)], pen.acc);
        }
        Base::FlipV => {
            pen.dashed_line((0.06, 0.5), (0.94, 0.5));
            pen.poly(&[(0.2, 0.4), (0.8, 0.4), (0.2, 0.08)], pen.fill);
            pen.poly(&[(0.2, 0.6), (0.8, 0.6), (0.2, 0.92)], pen.acc);
        }
        _ => {
            // A plain rounded tile for a control with no drawing of its own.
            pen.rect((0.14, 0.14), (0.86, 0.86), pen.fill);
            pen.acc_circle((0.5, 0.5), 0.1);
        }
    }
}

/// Paints a chevron pointing down, centred at `c`, `s` points across.
pub fn chevron(painter: &Painter, c: Pos2, s: f32, colour: Color32) {
    painter.line_segment([pos2(c.x - s, c.y - s * 0.5), pos2(c.x, c.y + s * 0.5)], Stroke::new(1.2, colour));
    painter.line_segment([pos2(c.x, c.y + s * 0.5), pos2(c.x + s, c.y - s * 0.5)], Stroke::new(1.2, colour));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every base draws something, at the two sizes the ribbon uses, in both states, without a panic.
    #[test]
    fn every_icon_paints_shapes_in_both_states() {
        let ctx = egui::Context::default();
        let t = Tokens::light();
        let bases = [
            Base::Page,
            Base::Pages,
            Base::Bold,
            Base::Gear,
            Base::Cylinder,
            Base::Star,
            Base::Mark,
            Base::Undo,
            Base::Redo,
            Base::Octagon,
            Base::Printer,
            Base::PagePortrait,
            Base::PageLandscape,
        ];
        for base in bases {
            for size in [18.0, 32.0] {
                for enabled in [true, false] {
                    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                        let r = Rect::from_min_size(pos2(4.0, 4.0), egui::vec2(size, size));
                        paint(ui.painter(), r, Icon::of(base).with(Badge::Plus), &t, enabled);
                    });
                    out.textures_delta.clear();
                    assert!(!out.shapes.is_empty(), "{base:?} at {size} painted nothing");
                }
            }
        }
    }
}
