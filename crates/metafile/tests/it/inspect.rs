//! Reading a display list back: what each path item is, where it lies, and what clips it.

use diacraft_metafile::{Drawn, Item, Paint, Path, Picture, Seg};

/// A path item as the tests ask about it.
#[derive(Clone, Debug)]
pub struct Ink {
    pub bbox: [f64; 4],
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<[u8; 4]>,
    pub clip: u32,
    pub points: Vec<[f64; 2]>,
}

pub fn bbox_of(path: &Path) -> Option<[f64; 4]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut grow = |p: [f32; 2]| {
        for k in 0..2 {
            lo[k] = lo[k].min(f64::from(p[k]));
            hi[k] = hi[k].max(f64::from(p[k]));
        }
    };
    for s in &path.segs {
        match *s {
            Seg::Move(a) | Seg::Line(a) => grow(a),
            Seg::Cubic(a, b, c) => {
                grow(a);
                grow(b);
                grow(c);
            }
            Seg::Close => {}
        }
    }
    (lo[0].is_finite()).then_some([lo[0], lo[1], hi[0], hi[1]])
}

pub fn points_of(path: &Path) -> Vec<[f64; 2]> {
    path.segs
        .iter()
        .flat_map(|s| match *s {
            Seg::Move(a) | Seg::Line(a) => vec![[f64::from(a[0]), f64::from(a[1])]],
            Seg::Cubic(a, b, c) => [a, b, c].iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect(),
            Seg::Close => vec![],
        })
        .collect()
}

fn solid(p: &Paint) -> Option<[u8; 4]> {
    match p {
        Paint::Solid(c) => Some(*c),
        _ => None,
    }
}

pub fn inks(pic: &Picture) -> Vec<Ink> {
    pic.items
        .iter()
        .filter_map(|it: &Item| match &it.drawn {
            Drawn::Path(p) => Some(Ink {
                bbox: bbox_of(&p.path)?,
                fill: p.fill.as_ref().and_then(|f| solid(&f.paint)),
                stroke: p.stroke.as_ref().and_then(|s| solid(&s.paint)),
                clip: it.clip,
                points: points_of(&p.path),
            }),
            _ => None,
        })
        .collect()
}

/// The filled items only: `(bbox, colour)`.
pub fn fills(pic: &Picture) -> Vec<([f64; 4], [u8; 4])> {
    inks(pic).into_iter().filter_map(|i| i.fill.map(|c| (i.bbox, c))).collect()
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

pub fn bbox_is(got: [f64; 4], want: [f64; 4]) -> bool {
    got.iter().zip(want.iter()).all(|(a, b)| near(*a, *b))
}

pub const BLUE: [u8; 4] = [0, 0, 255, 255];
pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];

/// The bounding box of a clip's single part.
pub fn clip_bbox(pic: &Picture, clip: u32) -> Option<[f64; 4]> {
    let c = pic.clips.get(usize::try_from(clip).ok()?)?;
    match c.parts.as_slice() {
        [one] => bbox_of(&one.path),
        _ => None,
    }
}
