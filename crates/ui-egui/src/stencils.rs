//! The stencils the Shapes window lists, read from the engine's `stencil.list`, and what each master's tile shows.
//!
//! **The window knows no stencil by name.** It asks the engine which stencils there are and which masters each holds (`{stencils: [{id,
//! name, builtin, masters: [{id, name, width, height, hidden, icon, ..}]}]}`), shows a row for each stencil and the masters of the one chosen,
//! and drops a master by its id with `shape.drop` (with `stencil` for a stencil opened from a file). A stencil the engine gains appears with
//! no change here. **A hidden master is left out** (`hidden: true`: a window may), and at most
//! [`MAX_MASTERS`] of a stencil are shown (the engine lists 2,048).
//!
//! **A tile's figure.** The masters of Basic Shapes have figures drawn in code (`icons`), found by their names. **A master of a stencil
//! file that has an icon is drawn from it** (`icon_image.rs`: ICO, PNG and BMP; the engine hands the bytes over, `OpenedStencil::icon_bytes`).
//! Every other master is drawn from its own outline: the window drops it on a scratch drawing, reads the outline back the way the pointer does
//! (`edit/scene.rs`: lines, ellipses and the arcs of ellipses, walked), and draws that as an unfilled outline fitted to the tile; for a master of a
//! stencil file the scratch drawing is a session of its own that has the file open (`Scratch`), so the person's drawing is not touched. A
//! master the engine cannot drop (a stencil this window was told of and the engine was not) is drawn as a plain box of its default size.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit::geom::{self, P};
use crate::edit::scene::Scene;
use crate::icons::Base;

/// The most stencils, masters of one stencil, and characters of a name the window takes from a list.
const MAX_STENCILS: usize = 64;
pub const MAX_MASTERS: usize = 2_048;
const NAME_CHARS: usize = 64;

/// One master of a stencil.
#[derive(Clone, Debug, PartialEq)]
pub struct MasterInfo {
    /// What `shape.drop` takes as `master`.
    pub id: String,
    pub name: String,
    /// The default size in inches.
    pub width: f64,
    pub height: f64,
    /// A one-dimensional connector; zero height is intentional.
    pub one_d: bool,
    /// Whether the AutoConnect strip may offer it: `stencil.list`'s `autoconnect`, false for a part of a structure and a master of a stencil that does not feed the strip
    /// True when the list says nothing.
    pub autoconnect: bool,
    /// Whether its drop is put in a structure (`stencil.list`'s `part`: a Swimlane, a Separator, a stencil file's master typed `Container` or `List`): let go on an AutoConnect
    /// arrow, it is dropped as any drop of it is, and joined to nothing.
    pub part: bool,
    /// The id of the stencil file it is in, which `shape.drop` takes as `stencil`; `None` for a built-in stencil.
    pub stencil: Option<String>,
    /// Whether the engine says it has an icon (`stencil.list`'s `icon` is not null).
    pub icon: bool,
    /// The stencil the master is in, for a master listed in a stencil that gathers the masters of several (the Quick Shapes stencil and the search results): the heading it stands
    /// under and the stencil an Add to Quick Shapes names. `None` for a master listed in its own stencil.
    pub home: Option<Home>,
}

/// The stencil a master of a gathered list is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub id: String,
    pub name: String,
}

impl MasterInfo {
    /// The original directed preview belongs only to this built-in master. Imported
    /// one-dimensional masters keep their own outline and may have any direction.
    pub fn directed_preview(&self) -> bool {
        self.stencil.is_none() && self.id == "gane-sarson.data-flow"
    }

    /// What the figures and icons of masters are kept by: the master's id, and for a stencil file the stencil's as well.
    pub fn key(&self) -> String {
        match &self.stencil {
            Some(stencil) => format!("{stencil}\u{1}{}", self.id),
            None => self.id.clone(),
        }
    }
}

/// One stencil.
#[derive(Clone, Debug, PartialEq)]
pub struct StencilInfo {
    pub id: String,
    pub name: String,
    /// A stencil of the engine's own, which stays; `false` for one opened from a file, which can be closed.
    pub builtin: bool,
    /// What the More Shapes menu files it under (`stencil.list`'s `category`); `None` for a stencil file, and for a list that names none.
    pub category: Option<String>,
    pub masters: Vec<MasterInfo>,
    /// What the stencil is: one of the engine's (or a file's), or one the window makes of the masters of several.
    pub kind: Kind,
    /// How many of `masters`, from the first, are the stencil's Quick Shapes (above its divider): `stencil.quick`'s group, which the window puts first.
    pub quick: usize,
    /// Whether the drawing set the stencil's Quick Shapes (an empty group is then shown with its hint, and Reset Stencil has something to undo).
    pub customized: bool,
}

/// What a stencil of the Shapes window is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// A stencil of the engine's or one opened from a file.
    #[default]
    Normal,
    /// The Quick Shapes stencil: the Quick Shapes of every open stencil, each stencil's under its name.
    Quick,
    /// The results of a search for shapes, each stencil's matches under its name.
    Search,
}

/// The id of the Quick Shapes stencil, and of the search results, in the window's list.
pub const QUICK_ID: &str = "quick";
pub const SEARCH_ID: &str = "search";

impl StencilInfo {
    /// Whether the stencil is a window's own gathering (its masters are not its own).
    pub fn is_gathered(&self) -> bool {
        self.kind != Kind::Normal
    }

    /// The masters the AutoConnect strip offers from this stencil (`stencil.quick`'s `offer`): at most [`OFFERED`] of the Quick Shapes it may offer
    /// (no connector, no part of a structure, nothing of a stencil that does not feed it), and **the stencil's first ones it may when it has none**. A gathering offers the first it may.
    /// None for a stencil that feeds the strip nothing: the flyout then takes the next stencil's (`DiacraftApp::autoconnect_offer`).
    pub fn offer(&self) -> Vec<&MasterInfo> {
        let fed = |m: &&MasterInfo| !m.one_d && m.autoconnect;
        let first: Vec<&MasterInfo> = self.masters.iter().take(self.quick).filter(fed).take(OFFERED).collect();
        if first.is_empty() { self.masters.iter().filter(fed).take(OFFERED).collect() } else { first }
    }
}

/// The most masters the AutoConnect strip shows.
pub const OFFERED: usize = 4;

/// A built-in stencil that is not open: `stencil.list`'s `available`, which More Shapes opens (`stencil.open {id}`).
#[derive(Clone, Debug, PartialEq)]
pub struct AvailableStencil {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    /// How many masters it holds.
    pub masters: usize,
}

fn text(v: &Value, key: &str) -> Option<String> {
    let s = v.get(key)?.as_str()?;
    let s: String = s.chars().filter(|c| !c.is_control()).take(NAME_CHARS).collect();
    (!s.trim().is_empty()).then_some(s)
}

fn size(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0 && *n < 1.0e4).unwrap_or(1.0)
}

/// One master of an answer of `stencil.list`, `stencil.quick` or `stencil.search`; `file` is the id of the stencil file it is in (a built-in master drops by its id alone), and `home`
/// the stencil a gathered list says it is in. `None` for a master with no id or name, or a hidden one.
fn master_of(m: &Value, file: Option<&str>, home: Option<&Home>) -> Option<MasterInfo> {
    if m.get("hidden").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let one_d = m.get("one_d").and_then(Value::as_bool).unwrap_or(false);
    Some(MasterInfo {
        id: text(m, "id")?,
        name: text(m, "name")?,
        width: size(m, "width"),
        height: if one_d && m.get("height").and_then(Value::as_f64) == Some(0.0) { 0.0 } else { size(m, "height") },
        one_d,
        autoconnect: m.get("autoconnect").and_then(Value::as_bool) != Some(false),
        part: m.get("part").and_then(Value::as_bool) == Some(true),
        stencil: file.map(str::to_owned),
        icon: m.get("icon").is_some_and(Value::is_object),
        home: home.cloned(),
    })
}

/// The masters of an array of them, at most [`MAX_MASTERS`].
fn masters_of(list: Option<&Value>, file: Option<&str>, home: Option<&Home>) -> Vec<MasterInfo> {
    masters_of_at_most(list, file, home, MAX_MASTERS)
}

/// [`masters_of`] with a cap of its own: what is left of the cap of a gathering, so that many groups are read to that cap in all and not to each.
fn masters_of_at_most(list: Option<&Value>, file: Option<&str>, home: Option<&Home>, most: usize) -> Vec<MasterInfo> {
    list.and_then(Value::as_array).map(|ms| ms.iter().filter_map(|m| master_of(m, file, home)).take(most).collect()).unwrap_or_default()
}

/// The stencils in an answer of `stencil.list`. A stencil or master with no id or name is left out; counts are capped.
pub fn parse(list: &Value) -> Vec<StencilInfo> {
    let Some(stencils) = list.get("stencils").and_then(Value::as_array) else { return Vec::new() };
    stencils
        .iter()
        .take(MAX_STENCILS)
        .filter_map(|s| {
            let id = text(s, "id")?;
            let builtin = s.get("builtin").and_then(Value::as_bool).unwrap_or(true);
            let masters = masters_of(s.get("masters"), (!builtin).then_some(id.as_str()), None);
            Some(StencilInfo {
                name: text(s, "name")?,
                builtin,
                category: text(s, "category"),
                masters,
                id,
                kind: Kind::Normal,
                quick: 0,
                customized: false,
            })
        })
        .collect()
}

/// What an answer of `stencil.quick` says of the Quick Shapes of each stencil: for each of the stencils `list` holds, the master ids of its group in order, and whether the
/// drawing set it; and the Quick Shapes stencil made of them all. Nothing in the answer is trusted: a group for a stencil the list lacks is left out.
pub fn apply_quick(list: &mut [StencilInfo], answer: &Value) -> Option<StencilInfo> {
    let groups = answer.get("stencils").and_then(Value::as_array)?;
    let mut gathered: Vec<MasterInfo> = Vec::new();
    for group in groups.iter().take(MAX_STENCILS) {
        let (Some(id), Some(name)) = (text(group, "stencil"), text(group, "name")) else { continue };
        let builtin = group.get("builtin").and_then(Value::as_bool).unwrap_or(true);
        let home = Home { id: id.clone(), name };
        let masters =
            masters_of_at_most(group.get("masters"), (!builtin).then_some(id.as_str()), Some(&home), MAX_MASTERS.saturating_sub(gathered.len()));
        if let Some(stencil) = list.iter_mut().find(|s| s.id == id && !s.is_gathered()) {
            stencil.customized = group.get("customized").and_then(Value::as_bool).unwrap_or(false);
            stencil.quick = put_first(&mut stencil.masters, masters.iter().map(|m| m.id.as_str()));
        }
        gathered.extend(masters);
    }
    Some(StencilInfo {
        id: QUICK_ID.to_owned(),
        name: "Quick Shapes".to_owned(),
        builtin: true,
        category: None,
        masters: gathered,
        kind: Kind::Quick,
        quick: 0,
        customized: false,
    })
}

/// Puts the masters called `ids` (those that are in `masters`) first, in the order of `ids`: → how many were. One pass for the places, and one more to put them: no
/// lookup by scanning.
fn put_first<'a>(masters: &mut Vec<MasterInfo>, ids: impl Iterator<Item = &'a str>) -> usize {
    let places: HashMap<&str, usize> = masters.iter().enumerate().map(|(at, m)| (m.id.as_str(), at)).collect();
    let mut order: Vec<usize> = Vec::new();
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for id in ids.take(MAX_MASTERS) {
        if let Some(at) = places.get(id).copied()
            && seen.insert(at)
        {
            order.push(at);
        }
    }
    let moved = order.len();
    let mut taken: Vec<Option<MasterInfo>> = std::mem::take(masters).into_iter().map(Some).collect();
    let mut out: Vec<MasterInfo> = Vec::with_capacity(taken.len());
    out.extend(order.iter().filter_map(|at| taken.get_mut(*at).and_then(Option::take)));
    out.extend(taken.into_iter().flatten());
    *masters = out;
    moved
}

/// What an answer of `stencil.search` says: the results as a stencil of their own, and what the search did.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The text that was searched for.
    pub text: String,
    /// The results, each stencil's matches together under its name.
    pub stencil: StencilInfo,
    /// How many masters matched, listed or not.
    pub matches: u64,
    /// Whether the search reached the end of the stencils (it stops at its limits).
    pub complete: bool,
    /// What the engine says of the search in words (a stop, more matches than were listed).
    pub notes: Vec<String>,
}

/// The results in an answer of `stencil.search`. The masters are capped as a listing's are.
pub fn parse_search(answer: &Value) -> Option<Found> {
    let groups = answer.get("results").and_then(Value::as_array)?;
    let mut masters: Vec<MasterInfo> = Vec::new();
    for group in groups.iter().take(MAX_STENCILS) {
        let (Some(id), Some(name)) = (text(group, "id"), text(group, "name")) else { continue };
        let builtin = group.get("builtin").and_then(Value::as_bool).unwrap_or(true);
        let home = Home { id: id.clone(), name };
        masters.extend(masters_of_at_most(
            group.get("masters"),
            (!builtin).then_some(id.as_str()),
            Some(&home),
            MAX_MASTERS.saturating_sub(masters.len()),
        ));
    }
    let notes = answer
        .get("notes")
        .and_then(Value::as_array)
        .map(|n| n.iter().filter_map(Value::as_str).take(4).map(|t| t.chars().filter(|c| !c.is_control()).take(400).collect()).collect())
        .unwrap_or_default();
    let stencil = StencilInfo {
        id: SEARCH_ID.to_owned(),
        name: "Search Results".to_owned(),
        builtin: true,
        category: None,
        masters,
        kind: Kind::Search,
        quick: 0,
        customized: false,
    };
    Some(Found {
        text: answer.get("text").and_then(Value::as_str).map(|t| t.chars().filter(|c| !c.is_control()).take(256).collect()).unwrap_or_default(),
        stencil,
        matches: answer.get("matches").and_then(Value::as_u64).unwrap_or(0),
        complete: answer.get("complete").and_then(Value::as_bool).unwrap_or(true),
        notes,
    })
}

/// The built-in stencils in an answer of `stencil.list` that are not open (`available`), with the same bounds as [`parse`].
pub fn parse_available(list: &Value) -> Vec<AvailableStencil> {
    let Some(available) = list.get("available").and_then(Value::as_array) else { return Vec::new() };
    available
        .iter()
        .take(MAX_STENCILS)
        .filter_map(|s| {
            let masters = s.get("masters_total").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
            Some(AvailableStencil { id: text(s, "id")?, name: text(s, "name")?, category: text(s, "category"), masters })
        })
        .collect()
}

/// The stencils the engine has: its `stencil.list`, asked of a session of its own (the command needs no drawing).
pub fn builtin() -> Vec<StencilInfo> {
    Session::new().execute("stencil.list", &json!({})).map(|answer| parse(&answer)).unwrap_or_default()
}

/// A session that is only for drawing the tiles of one stencil file: it has the file open (read again, by the path the window opened it from) and a
/// drawing of its own that each master is dropped on. The person's drawing is never used for this.
pub struct Scratch {
    session: Session,
    /// What the scratch session calls the stencil.
    stencil: String,
}

impl Scratch {
    /// A session with the stencil file at `path` open, or `None` when it cannot be (the file is gone or changed).
    pub fn open(path: &std::path::Path) -> Option<Scratch> {
        let mut session = Session::new();
        let answer = session.execute("stencil.open", &json!({"path": path.to_str()?})).ok()?;
        let stencil = answer.get("id")?.as_str()?.to_owned();
        Some(Scratch { session, stencil })
    }

    /// The figure of `master` of the stencil.
    pub fn figure(&mut self, master: &MasterInfo) -> Option<Figure> {
        figure_in(&mut self.session, Some(&self.stencil), master)
    }
}

/// The tiles a stencil shows beyond its masters, drawn quiet: figures the window has and the stencil has no master for yet.
pub fn planned(stencil_id: &str) -> &'static [(Base, &'static str)] {
    match stencil_id {
        "basic" => &[(Base::Star, "Star"), (Base::Cross, "Cross"), (Base::Arrow, "Arrow"), (Base::Cylinder, "Cylinder")],
        _ => &[],
    }
}

/// The figures drawn in code for the masters of Basic Shapes, by name.
pub fn drawn_in_code(name: &str) -> Option<Base> {
    Some(match name {
        "Rectangle" => Base::Rect,
        "Square" => Base::Square,
        "Ellipse" => Base::Ellipse,
        "Circle" => Base::Circle,
        "Triangle" => Base::Triangle,
        "Right Triangle" => Base::RightTriangle,
        "Diamond" => Base::Diamond,
        "Pentagon" => Base::Pentagon,
        "Hexagon" => Base::Hexagon,
        "Octagon" => Base::Octagon,
        "Parallelogram" => Base::Parallelogram,
        "Trapezoid" => Base::Trapezoid,
        _ => return None,
    })
}

/// A master's outline for its tile: points in a box `aspect.0` by `aspect.1` with the longer side 1 (y up), the closed paths with the
/// triangles that fill them, and the open ones.
#[derive(Debug, PartialEq)]
pub struct Figure {
    pub aspect: (f64, f64),
    pub areas: Vec<(Vec<P>, Vec<[usize; 3]>)>,
    pub lines: Vec<Vec<P>>,
    /// Avoid acute miter spikes in the tiny Four-Way Arrow preview. The contour is unchanged.
    pub separate_strokes: bool,
}

/// What a tile shows.
#[derive(Clone, Debug)]
pub enum Art {
    /// A figure drawn in code.
    Icon(Base),
    /// The master's own outline.
    Outline(Arc<Figure>),
    /// A box of the master's default size, for a master whose outline could not be read.
    Box(f64, f64),
    /// The master's icon, a texture of this size in pixels.
    Image(egui::TextureId, (f32, f32)),
}

/// The figure of a master, from its own geometry: dropped on a scratch drawing and read back through the scene the pointer uses. `None`
/// when the engine has no such master.
pub fn figure_of(master: &MasterInfo) -> Option<Figure> {
    figure_in(&mut Session::new(), None, master)
}

/// The figure of `master` dropped on a new drawing of `session`; `stencil` is the stencil file's id in that session, for a master of one.
fn figure_in(session: &mut Session, stencil: Option<&str>, master: &MasterInfo) -> Option<Figure> {
    session.execute("file.new", &json!({})).ok()?;
    let page = session.document()?.pages.iter().next()?.id;
    let mut params = json!({"master": master.id, "page": page, "x": 5.0, "y": 4.0});
    if let (Some(stencil), Some(map)) = (stencil, params.as_object_mut()) {
        map.insert("stencil".to_owned(), json!(stencil));
    }
    let dropped = session.execute("shape.drop", &params).ok()?;
    let id = u32::try_from(dropped.get("id")?.as_u64()?).ok()?;
    let scene = Scene::build(session.document()?, page, session.revision())?;
    let item = scene.get(id)?;
    let (lo, hi) = item.frame;
    let (w, h) = ((hi.0 - lo.0).abs(), (hi.1 - lo.1).abs());
    let longest = w.max(h);
    if !(longest.is_finite() && longest > 1.0e-9) {
        return None;
    }
    let fit = |p: &P| ((p.0 - lo.0) / longest, (p.1 - lo.1) / longest);
    let (mut areas, mut lines) = (Vec::new(), Vec::new());
    for path in &item.outline {
        let points: Vec<P> = path.iter().map(fit).collect();
        let closed = points.len() >= 4 && points.first().zip(points.last()).is_some_and(|(a, b)| geom::distance(*a, *b) < 1.0e-6);
        if closed {
            let triangles = geom::triangulate(&points);
            areas.push((points, triangles));
        } else if points.len() >= 2 {
            lines.push(points);
        }
    }
    if master.directed_preview() {
        // An original thumbnail cue. Page arrows remain the connector's EndArrow style,
        // which outline extraction deliberately does not include.
        if let Some(end) = lines.last().and_then(|line| line.last()).copied() {
            lines.push(vec![(end.0 - 0.18, end.1 + 0.11), end, (end.0 - 0.18, end.1 - 0.11)]);
        }
    }
    let separate_strokes = master.stencil.is_none() && master.id == "arrows.four-way";
    (!areas.is_empty() || !lines.is_empty()).then_some(Figure { aspect: (w / longest, h / longest), areas, lines, separate_strokes })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_engines_list_is_read_as_it_is_given_and_nothing_in_it_is_trusted() {
        let list = json!({"stencils": [
            {"id": "basic", "name": "Basic Shapes", "masters": [{"id": "rectangle", "name": "Rectangle", "width": 1.5, "height": 1.0}]},
            {"id": "two", "name": "Second", "masters": [{"id": "a", "name": "A\u{1b}[31m", "width": -3, "height": "tall"}, {"name": "no id"}]},
            {"name": "no id"},
            "not a stencil"
        ]});
        let stencils = parse(&list);
        assert_eq!(stencils.len(), 2);
        assert_eq!(
            stencils[0].masters,
            [MasterInfo {
                id: "rectangle".into(),
                name: "Rectangle".into(),
                width: 1.5,
                height: 1.0,
                one_d: false,
                autoconnect: true,
                part: false,
                stencil: None,
                icon: false,
                home: None
            }]
        );
        assert_eq!(stencils[1].masters.len(), 1, "a master with no id is left out");
        assert_eq!(stencils[1].masters[0].name, "A[31m", "control characters do not reach the screen");
        assert_eq!((stencils[1].masters[0].width, stencils[1].masters[0].height), (1.0, 1.0), "a size that is not a size is a unit");
        assert!(parse(&json!({})).is_empty() && parse(&json!(null)).is_empty());
        let many = json!({"stencils": (0..200).map(|i| json!({"id": format!("s{i}"), "name": "S", "masters": []})).collect::<Vec<_>>()});
        assert_eq!(parse(&many).len(), MAX_STENCILS);
    }

    #[test]
    fn a_stencil_file_is_listed_with_its_ids_for_a_drop_its_icons_and_without_its_hidden_masters() {
        let list = json!({"stencils": [
            {"id": "basic", "name": "Basic Shapes", "builtin": true, "masters": [{"id": "rectangle", "name": "Rectangle", "width": 1.0, "height": 1.0, "hidden": false, "icon": null}]},
            {"id": "file-1", "name": "Mine", "builtin": false, "masters": [
                {"id": "5", "name": "Box", "width": 2.0, "height": 1.0, "hidden": false, "icon": {"format": "ico", "bytes": 120}},
                {"id": "6", "name": "Secret", "width": 1.0, "height": 1.0, "hidden": true, "icon": null},
                {"id": "7", "name": "Line", "width": 1.0, "height": 0.0, "hidden": false, "icon": null}
            ]}
        ]});
        let stencils = parse(&list);
        assert!(stencils[0].builtin && !stencils[1].builtin);
        assert_eq!(stencils[0].masters[0].stencil, None, "a built-in master is dropped by its id alone");
        let names: Vec<&str> = stencils[1].masters.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Box", "Line"], "a hidden master is not a tile");
        assert!(stencils[1].masters.iter().all(|m| m.stencil.as_deref() == Some("file-1")));
        assert_eq!((stencils[1].masters[0].icon, stencils[1].masters[1].icon), (true, false));
        assert_ne!(stencils[0].masters[0].key(), stencils[1].masters[0].key());
        let key_of = |stencil: &str, id: &str| {
            MasterInfo {
                id: id.into(),
                name: "x".into(),
                width: 1.0,
                height: 1.0,
                one_d: false,
                autoconnect: true,
                part: false,
                stencil: Some(stencil.into()),
                icon: false,
                home: None,
            }
            .key()
        };
        assert_ne!(key_of("file-1", "5"), key_of("file-2", "5"), "two stencils may have a master of one id");
        let many = json!({"stencils": [{"id": "f", "name": "F", "builtin": false, "masters": (0..3_000).map(|i| json!({"id": i.to_string(), "name": "m"})).collect::<Vec<_>>()}]});
        assert_eq!(parse(&many)[0].masters.len(), MAX_MASTERS);
    }

    #[test]
    fn the_engines_own_stencils_are_listed_with_their_masters() {
        let stencils = builtin();
        let names: Vec<&str> = stencils.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names.first().copied(), Some("Basic Shapes"), "{names:?}");
        assert!(stencils.iter().all(|s| !s.masters.is_empty()), "{stencils:?}");
        let all: Vec<&str> = stencils.iter().flat_map(|s| s.masters.iter().map(|m| m.name.as_str())).collect();
        for name in ["Rectangle", "Diamond"] {
            assert!(all.contains(&name), "{name} in {all:?}");
        }
    }

    #[test]
    fn a_master_is_drawn_from_its_own_outline_and_a_diamond_from_its_four_corners() {
        let diamond = stencils_master("Diamond");
        let figure = figure_of(&diamond).expect("the engine has a diamond");
        assert_eq!(figure.areas.len(), 1);
        let (points, triangles) = &figure.areas[0];
        assert_eq!(points.len(), 5, "four corners, and the first again to close it");
        assert_eq!(triangles.len(), 2);
        assert!((figure.aspect.0 - 1.0).abs() < 1e-9 && (figure.aspect.1 - 1.0 / 1.5).abs() < 1e-9, "the longer side is 1: {:?}", figure.aspect);
        // A master that is not there has no outline.
        assert!(
            figure_of(&MasterInfo {
                id: "no_such_master".into(),
                name: "x".into(),
                width: 1.0,
                height: 1.0,
                one_d: false,
                autoconnect: true,
                part: false,
                stencil: None,
                icon: false,
                home: None
            })
            .is_none()
        );
    }

    #[test]
    fn batch_previews_keep_concavity_open_store_and_connector_direction() {
        let all = builtin();
        let master = |id: &str| all.iter().flat_map(|s| &s.masters).find(|m| m.id == id).unwrap();
        let flow = master("gane-sarson.data-flow");
        assert!(flow.one_d);
        let imported = MasterInfo { stencil: Some("file-1".into()), ..flow.clone() };
        assert!(!imported.directed_preview(), "an imported master never acquires the built-in arrow cue");
        assert_eq!(flow.height, 0.0);
        let figure = figure_of(flow).unwrap();
        assert!(figure.areas.is_empty());
        assert_eq!(figure.lines.len(), 2, "shaft and a visible directed head");
        assert_eq!(figure.lines[1].len(), 3);
        assert!(figure.lines[1][0].1 > figure.lines[1][1].1 && figure.lines[1][2].1 < figure.lines[1][1].1);
        let four_way = master("arrows.four-way");
        assert!(figure_of(four_way).unwrap().separate_strokes);
        let imported_four_way = MasterInfo { stencil: Some("file-1".into()), ..four_way.clone() };
        assert!(!figure_of(&imported_four_way).unwrap().separate_strokes, "an imported id never takes the built-in join treatment");
        let store = figure_of(master("gane-sarson.data-store")).unwrap();
        assert!(store.areas.is_empty());
        assert_eq!(store.lines.len(), 1);
        assert_eq!(store.lines[0].len(), 4);
        assert_ne!(store.lines[0].first(), store.lines[0].last(), "right side stays open");
        for id in ["decorative.star-5", "decorative.heart", "decorative.cross", "arrows.right", "arrows.four-way"] {
            let figure = figure_of(master(id)).unwrap();
            assert_eq!(figure.areas.len(), 1, "{id}");
            let (points, triangles) = &figure.areas[0];
            assert_eq!(triangles.len(), points.len() - 3, "{id}: a concave polygon is triangulated fully");
        }
    }

    fn stencils_master(name: &str) -> MasterInfo {
        builtin().into_iter().flat_map(|s| s.masters).find(|m| m.name == name).unwrap()
    }

    /// Every master of every stencil has an outline to draw, and the round ones are round: a terminator's ends are walked, not cut off.
    #[test]
    fn every_master_the_engine_has_can_be_drawn_and_a_round_end_is_round() {
        for stencil in builtin() {
            for master in &stencil.masters {
                let figure = figure_of(master).unwrap_or_else(|| panic!("{} has no outline", master.name));
                assert!(!figure.areas.is_empty() || !figure.lines.is_empty(), "{}", master.name);
                for (points, _) in &figure.areas {
                    assert!(points.iter().all(|p| p.0.is_finite() && p.1.is_finite() && p.0 > -0.01 && p.0 < 1.01), "{}: {points:?}", master.name);
                }
            }
        }
        let terminator = figure_of(&stencils_master("Terminator")).unwrap();
        assert!(terminator.areas[0].0.len() > 16, "a stadium's ends are many small pieces: {}", terminator.areas[0].0.len());
    }

    fn listed(id: &str, name: &str, masters: &[(&str, &str)], file: bool) -> serde_json::Value {
        json!({"id": id, "name": name, "builtin": !file, "masters": masters.iter().map(|(i, n)| json!({"id": i, "name": n, "width": 1.0, "height": 1.0})).collect::<Vec<_>>()})
    }

    #[test]
    fn the_quick_shapes_of_a_stencil_go_first_in_their_order_and_the_quick_stencil_gathers_them() {
        let mut list = parse(&json!({"stencils": [
            listed("basic", "Basic Shapes", &[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")], false),
            listed("file-1", "Mine", &[("1", "One"), ("2", "Two")], true),
        ]}));
        let answer = json!({"stencils": [
            {"stencil": "basic", "name": "Basic Shapes", "builtin": true, "customized": true, "masters": [{"id": "c", "name": "C"}, {"id": "a", "name": "A"}, {"id": "zzz", "name": "Gone"}, {"id": "c", "name": "C"}]},
            {"stencil": "file-1", "name": "Mine", "builtin": false, "customized": false, "masters": [{"id": "2", "name": "Two"}]},
            {"stencil": "unknown", "name": "No such", "builtin": true, "masters": [{"id": "q", "name": "Q"}]},
        ]});
        let quick = apply_quick(&mut list, &answer).unwrap();
        let names: Vec<&str> = list[0].masters.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["C", "A", "B", "D"], "the group first, in its order, the rest as they were");
        assert_eq!((list[0].quick, list[0].customized), (2, true), "a master the list does not have, and a repeat, are not counted");
        assert_eq!((list[1].quick, list[1].customized), (1, false));
        assert_eq!(list[1].masters[0].name, "Two");
        // The Quick Shapes stencil: every group's masters, each told which stencil it is in, and the ones of a stencil file dropped by that file.
        assert_eq!((quick.kind, quick.id.as_str(), quick.name.as_str()), (Kind::Quick, QUICK_ID, "Quick Shapes"));
        assert!(quick.is_gathered());
        let homes: Vec<(&str, &str)> = quick.masters.iter().map(|m| (m.home.as_ref().unwrap().id.as_str(), m.id.as_str())).collect();
        assert_eq!(homes, [("basic", "c"), ("basic", "a"), ("basic", "zzz"), ("basic", "c"), ("file-1", "2"), ("unknown", "q")]);
        assert_eq!(quick.masters[4].stencil.as_deref(), Some("file-1"));
        assert_eq!(quick.masters[0].stencil, None);
        // An answer with no stencils is no Quick Shapes stencil.
        assert!(apply_quick(&mut list, &json!({})).is_none());
    }

    #[test]
    fn what_a_stencil_offers_is_its_first_four_quick_shapes_that_are_not_connectors_or_its_first_four_masters() {
        let mut list = parse(&json!({"stencils": [json!({"id": "s", "name": "S", "builtin": true, "masters": [
            {"id": "1", "name": "One"}, {"id": "2", "name": "Two", "one_d": true, "height": 0.0}, {"id": "3", "name": "Three"}, {"id": "4", "name": "Four"},
            {"id": "5", "name": "Five"}, {"id": "6", "name": "Six"}, {"id": "7", "name": "Seven"}]})]}));
        let names = |l: &[StencilInfo]| l[0].offer().into_iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        // Nothing set: the first four that are not connectors.
        assert_eq!(names(&list), ["One", "Three", "Four", "Five"]);
        // A group of three: the Quick Shapes that are not connectors, which may be fewer than four (the strip is not filled from the rest).
        list[0].quick = 3;
        assert_eq!(names(&list), ["One", "Three"]);
        // A group that holds only a connector offers the stencil's own first four.
        list[0].masters.swap(0, 1);
        list[0].quick = 2;
        assert_eq!(names(&list), ["One"], "the connector in the group is not offered, and nothing is added from the rest");
        list[0].quick = 1;
        assert_eq!(names(&list), ["One", "Three", "Four", "Five"], "a group of a connector alone is no group");
    }

    #[test]
    fn a_hostile_group_costs_one_pass_and_is_capped() {
        let masters: Vec<(String, String)> = (0..3_000).map(|i| (i.to_string(), format!("M{i}"))).collect();
        let refs: Vec<(&str, &str)> = masters.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let mut list = parse(&json!({"stencils": [listed("big", "Big", &refs, true)]}));
        // A group naming the same master over and over, and masters that are not there.
        let ids: Vec<serde_json::Value> =
            (0..50_000).map(|i| json!({"id": if i % 2 == 0 { "7".to_owned() } else { format!("x{i}") }, "name": "N"})).collect();
        let answer = json!({"stencils": [{"stencil": "big", "name": "Big", "builtin": false, "customized": true, "masters": ids}]});
        let quick = apply_quick(&mut list, &answer).unwrap();
        assert_eq!(list[0].quick, 1);
        assert_eq!(list[0].masters.len(), MAX_MASTERS, "the list is as long as it was");
        assert!(quick.masters.len() <= MAX_MASTERS);
        assert_eq!(list[0].masters[0].id, "7");
    }

    #[test]
    fn the_search_results_are_read_with_nothing_in_them_trusted() {
        let answer = json!({"text": "ar\u{1b}row", "matches": 3, "complete": false, "notes": ["one\u{7}", "two"], "results": [
            {"id": "arrows", "name": "Arrow Shapes", "builtin": true, "masters": [{"id": "arrows.right", "name": "Right Arrow", "width": 1.5, "height": 0.8}]},
            {"id": "file-2", "name": "Mine", "builtin": false, "masters": [{"id": "9", "name": "Hidden arrow", "hidden": true}, {"id": "10", "name": "Arrow Two"}]},
            {"name": "no id"}, "not a group"
        ]});
        let found = parse_search(&answer).unwrap();
        assert_eq!((found.text.as_str(), found.matches, found.complete), ("arrow", 3, false));
        assert_eq!(found.notes, ["one", "two"]);
        assert_eq!((found.stencil.kind, found.stencil.id.as_str(), found.stencil.name.as_str()), (Kind::Search, SEARCH_ID, "Search Results"));
        let ids: Vec<&str> = found.stencil.masters.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["arrows.right", "10"], "a hidden master is left out");
        assert_eq!(found.stencil.masters[1].stencil.as_deref(), Some("file-2"));
        assert_eq!(found.stencil.masters[1].home.as_ref().map(|h| h.name.as_str()), Some("Mine"));
        assert!(parse_search(&json!({})).is_none());
        // Past the cap: a list of masters is cut as a stencil's is.
        let many = json!({"results": [{"id": "f", "name": "F", "builtin": false, "masters": (0..3_000).map(|i| json!({"id": i.to_string(), "name": "m"})).collect::<Vec<_>>()}]});
        assert_eq!(parse_search(&many).unwrap().stencil.masters.len(), MAX_MASTERS);
    }
}
