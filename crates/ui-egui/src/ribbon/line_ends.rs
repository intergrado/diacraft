//! The line ends the arrow menus offer: the 45 marks `format.shape` takes (`begin_arrow` and `end_arrow`, 0 to 45), each with a name and a
//! picture, both **ours**.
//!
//! **The names** say the kind of mark in DiaCraft's words (the specification names each by its kind, and no vendor's label is used). **The pictures** are drawn here from
//! plain pieces, a triangle, an open head, a disc, a bar, in the boxes give, **as the page's own renderer draws them** (`diacraft-render`'s `ends.rs`
//! holds the same table, which this one follows so that a menu's picture and the line on the page agree; the renderer keeps its table private, so a change to one
//! is a change to the other, and a test here holds this one to the numbers 1 to 45). A mark's tip is on the end of the line and its body lies back along it.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// The most a number is (`format.shape`'s own limit).
#[cfg(test)]
const MAX_NUMBER: u8 = 45;

/// The caption of each line end, by its number. Ours.
pub(crate) const NAMES: [&str; 46] = [
    "None",
    "Open head, short",
    "Triangle, short",
    "Open head",
    "Triangle",
    "Triangle, hollowed back",
    "Triangle, bulging back",
    "Open head, curved",
    "Slender head, hollowed back",
    "Slanted stroke through the end",
    "Disc",
    "Square",
    "Open head, long",
    "Long triangle",
    "Long triangle, hollow",
    "Triangle, short, hollow",
    "Triangle, hollow",
    "Triangle, hollowed back, hollow",
    "Triangle, bulging back, hollow",
    "Slender head, hollowed back, hollow",
    "Disc, hollow",
    "Square, hollow",
    "Long diamond, hollow",
    "Slanted stroke across",
    "One crossbar",
    "Two crossbars",
    "Three crossbars",
    "Fork",
    "Fork and crossbar",
    "Fork and disc",
    "Crossbar and disc",
    "Disc and crossbar, hollow",
    "Disc and two crossbars, hollow",
    "Disc and three crossbars, hollow",
    "Disc and long diamond, hollow",
    "Disc and crossbar",
    "Disc and two crossbars",
    "Disc and three crossbars",
    "Disc and long diamond",
    "Two triangles",
    "Two triangles, hollow",
    "Disc at the end, hollow",
    "Disc, second form",
    "Two open heads",
    "Open head and crossbar",
    "Two open heads and crossbar",
];

/// A point of a mark: along the line back from the tip, and across it, in units of the mark's size.
type At = (f32, f32);

/// A closed form.
enum Form {
    Poly(Vec<At>),
    /// Centre `x` back from the tip, and radius.
    Disc(f32, f32),
    /// Centre `x` back from the tip, and half side.
    Square(f32, f32),
}

/// One piece of a mark.
enum Piece {
    /// Filled in the line's colour.
    Solid(Form),
    /// An outline with nothing inside.
    Ring(Form),
    /// Strokes with no closed area.
    Open(Vec<At>),
}

/// A mark: its pieces, and how far from the tip the shaft stops (0 for an open mark, whose line runs on to the tip).
pub(crate) struct Art {
    pieces: Vec<Piece>,
    stop: f32,
}

fn triangle(len: f32) -> Form {
    Form::Poly(vec![(0.0, 0.0), (len, 1.0), (len, -1.0)])
}

fn head(half: f32, apex: f32) -> Form {
    Form::Poly(vec![(0.0, 0.0), (2.0, half), (apex, 0.0), (2.0, -half)])
}

fn diamond(from: f32) -> Form {
    Form::Poly(vec![(from, 0.0), (from + 2.25, 1.12), (from + 4.5, 0.0), (from + 2.25, -1.12)])
}

fn open_head(from: f32, len: f32) -> Piece {
    Piece::Open(vec![(from + len, 1.0), (from, 0.0), (from + len, -1.0)])
}

fn bar(x: f32) -> Piece {
    Piece::Open(vec![(x, 0.75), (x, -0.75)])
}

fn fork() -> Piece {
    Piece::Open(vec![(0.0, 0.75), (1.5, 0.0), (0.0, -0.75)])
}

fn disc(x: f32) -> Form {
    Form::Disc(x, 0.75)
}

/// The art of line end `number`; `None` for 0 (no mark) and for a number past 45.
pub(crate) fn art(number: u8) -> Option<Art> {
    let make = |pieces: Vec<Piece>, stop: f32| Some(Art { pieces, stop });
    // A disc at the tip, hollow (31 to 33) or solid (35 to 37), with these crossbars behind it.
    let disc_and_bars = |number: u8, bars: &[f32]| {
        let first = if number < 35 { Piece::Ring(disc(0.75)) } else { Piece::Solid(disc(0.75)) };
        make(std::iter::once(first).chain(bars.iter().map(|x| bar(*x))).collect(), 1.5)
    };
    match number {
        1 => make(vec![open_head(0.0, 1.0)], 0.0),
        2 => make(vec![Piece::Solid(triangle(1.0))], 1.0),
        3 => make(vec![open_head(0.0, 2.0)], 0.0),
        4 => make(vec![Piece::Solid(triangle(2.0))], 2.0),
        5 => make(vec![Piece::Solid(head(0.995, 1.75))], 1.75),
        6 => make(vec![Piece::Solid(head(1.01, 2.25))], 2.25),
        7 => make(vec![Piece::Open(vec![(1.84, 0.96), (0.5, 0.48), (0.08, 0.0), (0.5, -0.48), (1.84, -0.96)])], 0.0),
        8 => make(vec![Piece::Solid(head(1.0, 1.5))], 1.5),
        9 => make(vec![Piece::Open(vec![(-1.0, 1.0), (1.0, -1.0)])], 0.0),
        10 | 42 => make(vec![Piece::Solid(disc(0.0))], 0.75),
        11 => make(vec![Piece::Solid(Form::Square(0.0, 0.75))], 0.75),
        12 => make(vec![open_head(0.0, 3.0)], 0.0),
        13 => make(vec![Piece::Solid(triangle(3.0))], 3.0),
        14 => make(vec![Piece::Ring(triangle(3.0))], 3.0),
        15 => make(vec![Piece::Ring(triangle(1.0))], 1.0),
        16 => make(vec![Piece::Ring(triangle(2.0))], 2.0),
        17 => make(vec![Piece::Ring(head(1.0, 1.75))], 1.75),
        18 => make(vec![Piece::Ring(head(1.01, 2.25))], 2.25),
        19 => make(vec![Piece::Ring(head(1.0, 1.5))], 1.5),
        20 => make(vec![Piece::Ring(disc(0.0))], 0.75),
        21 => make(vec![Piece::Ring(Form::Square(0.0, 0.75))], 0.75),
        22 => make(vec![Piece::Ring(diamond(0.0))], 4.5),
        23 => make(vec![Piece::Open(vec![(1.5, -1.0), (2.5, 1.0)])], 0.0),
        24 => make(vec![bar(1.5)], 0.0),
        25 => make(vec![bar(1.5), bar(2.25)], 0.0),
        26 => make(vec![bar(1.5), bar(2.25), bar(3.0)], 0.0),
        27 => make(vec![fork()], 0.0),
        28 => make(vec![fork(), bar(2.0)], 0.0),
        29 => make(vec![fork(), Piece::Ring(disc(2.25))], 3.0),
        30 => make(vec![bar(0.75), Piece::Open(vec![(0.0, 0.0), (1.5, 0.0)]), Piece::Ring(disc(2.25))], 3.0),
        31 | 35 => disc_and_bars(number, &[2.25]),
        32 | 36 => disc_and_bars(number, &[2.25, 3.0]),
        33 | 37 => disc_and_bars(number, &[2.25, 3.0, 3.75]),
        34 => make(vec![Piece::Ring(disc(0.75)), Piece::Ring(diamond(1.5))], 6.0),
        38 => make(vec![Piece::Solid(disc(0.75)), Piece::Ring(diamond(1.5))], 6.0),
        39 | 40 => {
            let (first, second) = (triangle(2.0), Form::Poly(vec![(2.0, 0.0), (4.0, 1.0), (4.0, -1.0)]));
            let paint = |form: Form| if number == 40 { Piece::Ring(form) } else { Piece::Solid(form) };
            make(vec![paint(first), paint(second)], 4.0)
        }
        41 => make(vec![Piece::Ring(disc(0.75))], 1.5),
        43 => make(vec![open_head(0.0, 2.0), open_head(2.0, 2.0)], 0.0),
        44 => make(vec![open_head(0.0, 2.0), bar(2.75)], 0.0),
        45 => make(vec![open_head(0.0, 2.0), open_head(2.0, 2.0), bar(4.75)], 0.0),
        _ => None,
    }
}

impl Art {
    /// How far back from the tip the shaft stops, in units of the mark's size.
    pub(crate) fn stop(&self) -> f32 {
        self.stop
    }
}

/// How many units of a mark's size the farthest of its points lies behind the tip: what a picture of it must leave room for.
#[cfg(test)]
fn reach(art: &Art) -> f32 {
    let far = |points: &[At]| points.iter().map(|p| p.0).fold(f32::MIN, f32::max);
    art.pieces
        .iter()
        .map(|piece| match piece {
            Piece::Open(points) => far(points),
            Piece::Solid(form) | Piece::Ring(form) => match form {
                Form::Poly(points) => far(points),
                Form::Disc(x, r) | Form::Square(x, r) => x + r,
            },
        })
        .fold(0.0, f32::max)
}

/// Paints `art` with its tip at `tip`, its body lying back along the line in direction `back` (1 to the right, -1 to the left), one unit being `unit` points.
pub(crate) fn paint(painter: &Painter, tip: Pos2, back: f32, unit: f32, art: &Art, ink: Color32) {
    let at = |(along, across): At| pos2(tip.x + back * along * unit, tip.y + across * unit);
    let stroke = Stroke::new(1.3, ink);
    for piece in &art.pieces {
        match piece {
            Piece::Open(points) => {
                painter.add(Shape::line(points.iter().map(|p| at(*p)).collect(), stroke));
            }
            Piece::Solid(form) => paint_form(painter, form, &at, unit, Some(ink), Stroke::NONE),
            Piece::Ring(form) => paint_form(painter, form, &at, unit, None, stroke),
        }
    }
}

fn paint_form(painter: &Painter, form: &Form, at: &impl Fn(At) -> Pos2, unit: f32, fill: Option<Color32>, stroke: Stroke) {
    match form {
        Form::Poly(points) => {
            let points: Vec<Pos2> = points.iter().map(|p| at(*p)).collect();
            if let Some(fill) = fill {
                // Every polygon here is star-shaped from its first point (a head with a hollowed back is not convex), so a fan from it covers exactly the form.
                if let Some((first, rest)) = points.split_first() {
                    for pair in rest.windows(2) {
                        if let [a, b] = pair {
                            painter.add(Shape::convex_polygon(vec![*first, *a, *b], fill, Stroke::NONE));
                        }
                    }
                }
            } else {
                painter.add(Shape::closed_line(points, stroke));
            }
        }
        Form::Disc(x, r) => match fill {
            Some(fill) => {
                painter.circle_filled(at((*x, 0.0)), r * unit, fill);
            }
            None => {
                painter.circle_stroke(at((*x, 0.0)), r * unit, stroke);
            }
        },
        Form::Square(x, half) => {
            let rect = Rect::from_two_pos(at((x - half, -half)), at((x + half, *half)));
            match fill {
                Some(fill) => {
                    painter.rect_filled(rect, 0.0, fill);
                }
                None => {
                    painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every number from 1 to 45 has art, 0 and anything past 45 has none, and each name is its own: a menu of the 46 rows tells them apart.
    #[test]
    fn every_line_end_has_a_name_and_a_picture_and_the_names_differ() {
        assert!(art(0).is_none() && art(46).is_none() && art(254).is_none());
        for n in 1..=MAX_NUMBER {
            assert!(art(n).is_some(), "no art for {n}");
        }
        assert_eq!(NAMES.len(), usize::from(MAX_NUMBER) + 1);
        let mut names: Vec<&str> = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), NAMES.len(), "two numbers share a name");
        assert_eq!(NAMES[0], "None");
    }

    /// A picture leaves room for itself: no mark reaches more than 6 units back (the disc and a long diamond are the longest), and the shaft stops within the mark.
    #[test]
    fn no_picture_reaches_further_back_than_the_room_a_menu_row_gives() {
        for n in 1..=MAX_NUMBER {
            let Some(a) = art(n) else { continue };
            let r = reach(&a);
            assert!(r > 0.0 && r <= 6.0 + 1e-6, "{n} reaches {r}");
            assert!(a.stop() <= r + 1e-6, "{n}: the shaft stops at {} and the mark ends at {r}", a.stop());
        }
    }
}
