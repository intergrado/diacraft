//! The Size & Position window ("fields for X, Y, width, height, angle and pin position on a 2-D shape, or begin and end for a
//! line; typing into a selection of several makes them the same size"; its title holds the selected shape's name).
//!
//! **What it shows** is the answer of the engine's `shape.placement` for the selection, read once for each revision of the drawing and each
//! selection: a shape's pin on the page, its size along its own axes, its angle, which of the nine places its pin is at, and for a line its ends with
//! their glue and its length. Lengths are shown **in the page's measurement unit at the drawing's scale** (the unit of the page's `DrawingScale`, the same
//! the rulers count in, and from the ruler's zero) and typed in the same way: a field takes units and arithmetic (`measure.rs`; a comma is refused with the
//! value unchanged). With **several shapes** selected a field shows nothing where they differ, and a value typed in X or Y puts every shape's
//! pin at it. The window copies nothing of the scene's outlines: what each shape can do (move, turn) is read as a few flags ([`Caps`]).
//!
//! **What a typed value sends**, one command and so one undo step (Auto Size's `fit_page` runs inside the command as it does for a drag), **to the shapes the
//! field showed when it took the focus** (`fields.rs`), whatever is selected when it lets go: X and Y send `shape.place` with the place typed, for any number
//! of shapes at any places (the answer's `unplaced`, the shapes it could not place, is told the way a refusal is); Angle sends `shape.rotate {angle}`
//! (a line takes it too, ROTATE-1D-1); Width and Height send `shape.size` with every shape selected, each keeping its pin; the nine places of
//! Pin Pos send `shape.pin`; a line's four end fields send `shape.set_end`, one coordinate of one end, and its length `shape.set_length`. **Every shape
//! selected is sent, up to the 4,096 the engine takes**; with more the fields are quiet and say so. A place or a size over the engine's bound
//! (`draw::MAX_COORD`, a million inches of drawing) is refused in words and nothing is sent. **A field the command would refuse is drawn quiet, with the
//! reason**: Width and Height with a line in the selection (a group is sized as one shape and its members follow), Pin Pos with a line, the two fields of an end that is glued to a shape (it follows
//! the shape), the length of a line with both ends glued. The pane's title is the `name` of the answer.

use std::sync::Arc;

use diacraft_engine::draw::MAX_COORD;
use egui::{Sense, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::canvas::page_drawing::{PageSettings, page_settings};
use crate::edit::inch;
use crate::edit::scene::Kind;
use crate::measure::{angle_text, length_text, parse_angle, parse_length};
use crate::panes::fields::{Field, Target, Typed};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};
use crate::work;

/// How many shapes one engine command takes (`cmd/params.rs`, `MAX_IDS`): a selection of more is not sent in part, its fields are quiet.
pub(crate) const MAX_SHAPES: usize = 4_096;

/// Two lengths this near, in paper inches, are the same: what separates "the shapes agree" from "they differ".
const SAME: f64 = 1.0e-6;

/// The nine places of the pin on the box, in the order the grid draws them, with the words `shape.pin` takes (`at`) and `shape.placement` answers (`pin_at`).
const PIN_PLACES: [(&str, f64, f64); 9] = [
    ("top-left", 0.0, 1.0),
    ("top-center", 0.5, 1.0),
    ("top-right", 1.0, 1.0),
    ("middle-left", 0.0, 0.5),
    ("center", 0.5, 0.5),
    ("middle-right", 1.0, 0.5),
    ("bottom-left", 0.0, 0.0),
    ("bottom-center", 0.5, 0.0),
    ("bottom-right", 1.0, 0.0),
];

/// One end of a line, as `shape.placement` gives it.
#[derive(Clone, Debug)]
struct End {
    x: f64,
    y: f64,
    /// The shape the end is glued to, if it is.
    glued_to: Option<u32>,
}

/// One shape of the answer of `shape.placement`: paper inches, degrees.
#[derive(Clone, Debug)]
struct Place {
    id: u32,
    /// The shape's name, it (`Name`, else `NameU`, else `Sheet.<id>`): the pane's title.
    name: String,
    one_d: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    angle: f64,
    pin_at: Option<String>,
    begin: Option<End>,
    end: Option<End>,
    length: Option<f64>,
}

/// What `shape.placement` answered for a selection.
#[derive(Debug)]
pub(super) struct Placements {
    shapes: Vec<Place>,
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn end_of(v: Option<&Value>) -> Option<End> {
    let v = v.filter(|v| v.is_object())?;
    let glued_to =
        v.get("glued").filter(|g| g.is_object()).map(|g| g.get("id").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0));
    Some(End { x: number(v, "x")?, y: number(v, "y")?, glued_to })
}

impl Placements {
    /// Reads the answer. `None` where it does not have the shape.
    fn read(answer: &Value) -> Option<Placements> {
        let shapes = answer.get("shapes")?.as_array()?;
        let mut read = Vec::with_capacity(shapes.len());
        for s in shapes {
            read.push(Place {
                id: u32::try_from(s.get("id")?.as_u64()?).ok()?,
                name: s.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                one_d: s.get("one_d").and_then(Value::as_bool).unwrap_or(false),
                x: number(s, "x")?,
                y: number(s, "y")?,
                width: number(s, "width")?,
                height: number(s, "height")?,
                angle: number(s, "angle")?,
                pin_at: s.get("pin_at").and_then(Value::as_str).map(str::to_owned),
                begin: end_of(s.get("begin")),
                end: end_of(s.get("end")),
                length: number(s, "length"),
            });
        }
        Some(Placements { shapes: read })
    }
}

/// What the window keeps of the last `shape.placement` it asked: for which revision, page and shapes, and the answer.
#[derive(Default)]
pub struct PlacementCache {
    /// The drawing's generation and revision, the page and the shapes the answer is for.
    key: Option<(u64, u64, u32, Vec<u32>)>,
    read: Option<Arc<Placements>>,
}

/// What a shape can do, as the scene says: all the window needs of it besides what `shape.placement` answers. A few flags, so that nothing of the scene's
/// outlines is copied.
#[derive(Clone, Copy, Debug)]
struct Caps {
    /// It can be moved by itself (a connector glued at both ends cannot).
    moves: bool,
    /// It can be turned: a 2-D shape or a group that is not in a turned or flipped group, or a line.
    turns: bool,
}

/// What the window shows of the selection, read once for a frame.
struct Selected {
    page: u32,
    ids: Vec<u32>,
    /// What each shape can do, and what `shape.placement` says of it: the two lists are in the order of `ids`.
    caps: Vec<Caps>,
    read: Arc<Placements>,
    settings: PageSettings,
}

/// What the selection comes to for the window.
enum Pick {
    /// Nothing selected.
    Nothing,
    /// More shapes than one command takes.
    TooMany(usize),
    Some(Box<Selected>),
}

/// What the selected shapes have in common.
enum Agree<T> {
    One(T),
    Mixed,
}

impl<T> Agree<T> {
    fn shown(&self, text: impl Fn(&T) -> String) -> Option<String> {
        match self {
            Agree::One(v) => Some(text(v)),
            Agree::Mixed => None,
        }
    }
}

fn agree(values: impl Iterator<Item = f64>) -> Agree<f64> {
    let mut all = values;
    let Some(first) = all.next() else { return Agree::Mixed };
    if all.all(|v| (v - first).abs() <= SAME) { Agree::One(first) } else { Agree::Mixed }
}

impl DiacraftApp {
    /// The shapes `wanted` of `page`: what each can do, what `shape.placement` says of them, and the page's settings. `None` when the engine does not answer
    /// for them (a shape that has gone).
    fn selected_for(&mut self, page: u32, wanted: &[u32]) -> Option<Selected> {
        self.ensure_scene(page);
        let caps: Vec<(u32, Caps)> = {
            let scene = self.ui.edit.scene()?;
            wanted
                .iter()
                .filter_map(|id| {
                    let item = scene.get(*id)?;
                    Some((*id, Caps { moves: item.moves_by_itself(), turns: item.rotatable() || item.kind == Kind::Connector }))
                })
                .collect()
        };
        work::count(|w| w.caps_built += caps.len());
        if caps.is_empty() {
            return None;
        }
        let ids: Vec<u32> = caps.iter().map(|(id, _)| *id).collect();
        let read = self.placements(page, &ids)?;
        if read.shapes.len() != ids.len() {
            return None;
        }
        let settings = self.session.document().and_then(|doc| page_settings(doc, page))?;
        Some(Selected { page, ids, caps: caps.into_iter().map(|(_, c)| c).collect(), read, settings })
    }

    /// What is selected on the page on view, for the window to show: **nothing when the selection is of another page** (`selection_on_view`; a field drawn for
    /// the page on view's shape of the same `ID` would show its values and take a value typed for it).
    fn size_position_selection(&mut self) -> Pick {
        let Some((page, wanted)) = self.selection_here() else { return Pick::Nothing };
        let count = wanted.len();
        if count > MAX_SHAPES {
            return Pick::TooMany(count);
        }
        match self.selected_for(page, &wanted) {
            Some(sel) => Pick::Some(Box::new(sel)),
            None => Pick::Nothing,
        }
    }

    /// `shape.placement` for `ids`, asked again only when the drawing, the page or the shapes are not those of the last answer.
    fn placements(&mut self, page: u32, ids: &[u32]) -> Option<Arc<Placements>> {
        let key = (self.ui.generation, self.session.revision(), page, ids.to_vec());
        if self.ui.panes.size.key.as_ref() != Some(&key) {
            work::count(|w| w.placement_queries += 1);
            let answer = self.session.execute("shape.placement", &json!({"ids": ids, "page": page})).ok();
            self.ui.panes.size.read = answer.as_ref().and_then(Placements::read).map(Arc::new);
            self.ui.panes.size.key = Some(key);
        }
        self.ui.panes.size.read.clone()
    }

    /// The title: "Size & Position" and, with one shape selected, its name as `shape.placement` gives it.
    pub(super) fn size_position_title(&mut self) -> String {
        let here = self.selection_here();
        let name = match here.as_ref().map(|(page, ids)| (*page, ids.as_slice())) {
            Some((page, [only])) => {
                self.placements(page, &[*only]).and_then(|read| read.shapes.first().map(|p| p.name.clone())).filter(|n| !n.is_empty())
            }
            _ => None,
        };
        match name {
            Some(name) => format!("Size & Position - {}", diacraft_engine::shown(&name, 40)),
            None => "Size & Position".to_owned(),
        }
    }

    pub(super) fn size_position_body(&mut self, ui: &mut Ui, t: &Tokens) {
        let words = |ui: &mut Ui, text: &str| {
            ui.add(egui::Label::new(egui::RichText::new(text).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            ui.add_space(6.0);
        };
        match self.size_position_selection() {
            Pick::Nothing => {
                words(ui, "Select a shape to see its size and position.");
                self.quiet_fields(ui, t, "Nothing is selected");
            }
            Pick::TooMany(count) => {
                let why =
                    format!("{count} shapes are selected: a command takes at most {MAX_SHAPES}, so the fields are not used. Select fewer shapes.");
                words(ui, &why);
                self.quiet_fields(ui, t, &why);
            }
            Pick::Some(sel) => {
                let one_d = matches!(sel.read.shapes.as_slice(), [only] if only.one_d);
                if one_d { self.line_fields(ui, t, &sel) } else { self.box_fields(ui, t, &sel) }
            }
        }
    }

    /// The fields drawn with nothing to show: blank and quiet.
    fn quiet_fields(&mut self, ui: &mut Ui, t: &Tokens, why: &str) {
        for (label, name) in
            [("X", "pane.size.x"), ("Y", "pane.size.y"), ("Width", "pane.size.width"), ("Height", "pane.size.height"), ("Angle", "pane.size.angle")]
        {
            self.pane_field(ui, t, Field { label, name, shown: None, enabled: false, why_not: Some(why), target: None });
        }
    }

    fn can(&self, command: &str) -> bool {
        self.command_enabled(command).is_ok()
    }

    /// X, Y, Width, Height, Angle and Pin Pos of a 2-D shape, or of several.
    fn box_fields(&mut self, ui: &mut Ui, t: &Tokens, sel: &Selected) {
        let s = &sel.settings;
        let unit = s.ruler_unit;
        let places = &sel.read.shapes;
        let len = |paper: f64| length_text(paper / s.drawing_scale, unit);
        let rel = |paper: f64, origin: f64| length_text((paper - origin) / s.drawing_scale, unit);
        let xs = agree(places.iter().map(|p| p.x));
        let ys = agree(places.iter().map(|p| p.y));
        let ws = agree(places.iter().map(|p| p.width));
        let hs = agree(places.iter().map(|p| p.height));
        let angles = agree(places.iter().map(|p| p.angle));
        let moves = sel.caps.iter().any(|c| c.moves) && self.can("shape.place");
        // The engine refuses the whole of a size that names a line; a group is sized as one shape.
        let sizes = places.iter().all(|p| !p.one_d) && self.can("shape.size");
        let turns = sel.caps.iter().any(|c| c.turns) && self.can("shape.rotate");
        let no_size = "A line is sized by its ends: a size typed here is for 2-D shapes and groups";
        let target = Some(Target { page: sel.page, ids: &sel.ids });
        let no_move = "These shapes cannot be moved by themselves";
        self.pane_field(
            ui,
            t,
            Field { label: "X", name: "pane.size.x", shown: xs.shown(|v| rel(*v, s.ruler_origin.0)), enabled: moves, why_not: Some(no_move), target },
        );
        self.pane_field(
            ui,
            t,
            Field { label: "Y", name: "pane.size.y", shown: ys.shown(|v| rel(*v, s.ruler_origin.1)), enabled: moves, why_not: Some(no_move), target },
        );
        self.pane_field(
            ui,
            t,
            Field { label: "Width", name: "pane.size.width", shown: ws.shown(|v| len(*v)), enabled: sizes, why_not: Some(no_size), target },
        );
        self.pane_field(
            ui,
            t,
            Field { label: "Height", name: "pane.size.height", shown: hs.shown(|v| len(*v)), enabled: sizes, why_not: Some(no_size), target },
        );
        self.pane_field(
            ui,
            t,
            Field {
                label: "Angle",
                name: "pane.size.angle",
                shown: angles.shown(|v| angle_text(*v)),
                enabled: turns,
                why_not: Some("These shapes cannot be turned"),
                target,
            },
        );
        ui.add_space(8.0);
        self.pin_grid(ui, t, sel);
    }

    /// Begin X, Begin Y, End X, End Y, Length and Angle of a line or a connector.
    fn line_fields(&mut self, ui: &mut Ui, t: &Tokens, sel: &Selected) {
        let Some(place) = sel.read.shapes.first() else { return };
        let s = &sel.settings;
        let unit = s.ruler_unit;
        let rel = |paper: f64, origin: f64| length_text((paper - origin) / s.drawing_scale, unit);
        let set_end = self.can("shape.set_end");
        let glued = |e: &Option<End>| e.as_ref().is_some_and(|e| e.glued_to.is_some());
        let both_glued = glued(&place.begin) && glued(&place.end);
        let set_length = self.can("shape.set_length") && !both_glued;
        let turns = self.can("shape.rotate") && !both_glued;
        let target = Some(Target { page: sel.page, ids: &sel.ids });
        let glued_words = |e: &Option<End>, which: &str| match e.as_ref().and_then(|e| e.glued_to) {
            Some(id) => format!("The {which} is glued to shape {id} and follows it: move that shape, or take the glue off first"),
            None => "This line's end cannot be set now".to_owned(),
        };
        let rows: [(&str, &str, Option<String>, bool, String); 4] = [
            (
                "Begin X",
                "pane.size.begin_x",
                place.begin.as_ref().map(|e| rel(e.x, s.ruler_origin.0)),
                set_end && !glued(&place.begin),
                glued_words(&place.begin, "begin"),
            ),
            (
                "Begin Y",
                "pane.size.begin_y",
                place.begin.as_ref().map(|e| rel(e.y, s.ruler_origin.1)),
                set_end && !glued(&place.begin),
                glued_words(&place.begin, "begin"),
            ),
            (
                "End X",
                "pane.size.end_x",
                place.end.as_ref().map(|e| rel(e.x, s.ruler_origin.0)),
                set_end && !glued(&place.end),
                glued_words(&place.end, "end"),
            ),
            (
                "End Y",
                "pane.size.end_y",
                place.end.as_ref().map(|e| rel(e.y, s.ruler_origin.1)),
                set_end && !glued(&place.end),
                glued_words(&place.end, "end"),
            ),
        ];
        for (label, name, shown, enabled, why) in rows {
            self.pane_field(ui, t, Field { label, name, shown, enabled, why_not: Some(&why), target });
        }
        self.pane_field(
            ui,
            t,
            Field {
                label: "Length",
                name: "pane.size.length",
                shown: place.length.map(|l| length_text(l / s.drawing_scale, unit)),
                enabled: set_length,
                why_not: Some("Both ends are glued to shapes, which set the length"),
                target,
            },
        );
        self.pane_field(
            ui,
            t,
            Field {
                label: "Angle",
                name: "pane.size.angle",
                shown: Some(angle_text(place.angle)),
                enabled: turns,
                why_not: Some("A line with both ends glued has nothing to turn"),
                target,
            },
        );
    }

    /// Sends a value typed in a field of this pane to the shapes it showed (`fields.rs`).
    pub(super) fn send_size_typed(&mut self, typed: Typed) {
        let Typed { name, text, page, ids } = typed;
        let Some(sel) = self.selected_for(page, &ids) else {
            self.field_refused(&name, "The shapes are not there any more: nothing was sent.".to_owned(), page, &ids);
            return;
        };
        let result = match name.as_str() {
            "pane.size.x" => self.commit_position(&sel, false, &text),
            "pane.size.y" => self.commit_position(&sel, true, &text),
            "pane.size.width" => self.commit_size(&sel, false, &text).map(|()| None),
            "pane.size.height" => self.commit_size(&sel, true, &text).map(|()| None),
            "pane.size.angle" => self.commit_angle(&sel, &text).map(|()| None),
            "pane.size.length" => self.commit_length(&sel, &text).map(|()| None),
            _ => self.commit_end(&sel, &name, &text).map(|()| None),
        };
        self.finish_commit(&sel, &name, result);
    }

    /// What follows a typed value: a refusal is told under its field, a success keeps the outlines painted until the picture shows the change. A success that
    /// left some shapes as they were (`Ok(Some(words))`: `shape.place` says how many it could not place) tells its words the way a refusal is told.
    fn finish_commit(&mut self, sel: &Selected, name: &str, result: Result<Option<String>, String>) {
        match result {
            Ok(note) => {
                self.keep_preview(sel.page, &sel.ids);
                match note {
                    Some(words) => self.field_refused(name, words, sel.page, &sel.ids),
                    None => self.field_accepted(),
                }
            }
            Err(words) => self.field_refused(name, words, sel.page, &sel.ids),
        }
    }

    /// The words for a typed number that is past what the engine takes.
    fn too_big(sel: &Selected) -> String {
        format!("That is too large: a place or a size is at most {} on this page.", length_text(MAX_COORD, sel.settings.ruler_unit))
    }

    /// X or Y: `shape.place` puts the pin of every shape selected at the place typed, shapes at different places and any number of them (up to the 4,096
    /// one command takes) in the one command and the one undo step. A shape with nothing to move (a connector glued at both ends) is skipped by the engine, and one
    /// it could not place (its pin through its groups is not a number, or a write would go past the page's reach) is counted in the answer: the words say how many.
    fn commit_position(&mut self, sel: &Selected, y: bool, text: &str) -> Result<Option<String>, String> {
        let s = &sel.settings;
        let value = parse_length(text, s.ruler_unit)?;
        let origin = if y { s.ruler_origin.1 } else { s.ruler_origin.0 };
        let target = value * s.drawing_scale + origin;
        if (target / s.drawing_scale).abs() > MAX_COORD {
            return Err(Self::too_big(sel));
        }
        let mut params = json!({"ids": sel.ids, "page": sel.page});
        if let Some(object) = params.as_object_mut() {
            object.insert(if y { "y" } else { "x" }.to_owned(), inch(target));
        }
        let answer = self.execute("shape.place", &params).map_err(|e| e.to_string())?;
        let unplaced = answer.get("unplaced").and_then(Value::as_u64).unwrap_or(0);
        Ok(match unplaced {
            0 => None,
            1 => Some("1 shape could not be placed: its place is past what the page holds, so it was left where it is.".to_owned()),
            n => Some(format!("{n} shapes could not be placed: their places are past what the page holds, so they were left where they are.")),
        })
    }

    /// Width or Height: `shape.size` for every shape selected, each keeping its pin.
    fn commit_size(&mut self, sel: &Selected, height: bool, text: &str) -> Result<(), String> {
        let s = &sel.settings;
        let value = parse_length(text, s.ruler_unit)?;
        if value > MAX_COORD {
            return Err(Self::too_big(sel));
        }
        let paper = value * s.drawing_scale;
        if paper <= 0.0 {
            return Err("A size must be more than nothing.".to_owned());
        }
        let key = if height { "height" } else { "width" };
        let mut params = json!({"ids": sel.ids, "page": sel.page});
        if let Some(object) = params.as_object_mut() {
            object.insert(key.to_owned(), inch(paper));
        }
        self.send_steps(&[("shape.size", params)], "Size")
    }

    /// Angle: the shapes that can be turned are turned to it.
    fn commit_angle(&mut self, sel: &Selected, text: &str) -> Result<(), String> {
        let degrees = parse_angle(text)?;
        let ids: Vec<u32> = sel.ids.iter().zip(&sel.caps).filter(|(_, c)| c.turns).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return Err("These shapes cannot be turned.".to_owned());
        }
        let params = json!({"ids": ids, "angle": crate::edit::geom::normal_degrees(degrees), "page": sel.page});
        self.send_steps(&[("shape.rotate", params)], "Angle")
    }

    /// An end of a line, by `shape.set_end`: the coordinate typed, the other one kept.
    fn commit_end(&mut self, sel: &Selected, name: &str, text: &str) -> Result<(), String> {
        let s = &sel.settings;
        let Some(place) = sel.read.shapes.first() else { return Ok(()) };
        let value = parse_length(text, s.ruler_unit)?;
        let (end, is_y) = match name {
            "pane.size.begin_x" => ("begin", false),
            "pane.size.begin_y" => ("begin", true),
            "pane.size.end_x" => ("end", false),
            _ => ("end", true),
        };
        let origin = if is_y { s.ruler_origin.1 } else { s.ruler_origin.0 };
        let paper = value * s.drawing_scale + origin;
        if (paper / s.drawing_scale).abs() > MAX_COORD {
            return Err(Self::too_big(sel));
        }
        let key = if is_y { "y" } else { "x" };
        let mut params = json!({"id": place.id, "end": end, "page": sel.page});
        if let Some(object) = params.as_object_mut() {
            object.insert(key.to_owned(), inch(paper));
        }
        self.send_steps(&[("shape.set_end", params)], "Line end")
    }

    /// The length of a line, by `shape.set_length`.
    fn commit_length(&mut self, sel: &Selected, text: &str) -> Result<(), String> {
        let s = &sel.settings;
        let Some(place) = sel.read.shapes.first() else { return Ok(()) };
        let value = parse_length(text, s.ruler_unit)?;
        if value > MAX_COORD {
            return Err(Self::too_big(sel));
        }
        let paper = value * s.drawing_scale;
        if paper <= 0.0 {
            return Err("A length must be more than nothing.".to_owned());
        }
        let params = json!({"id": place.id, "length": inch(paper), "page": sel.page});
        self.send_steps(&[("shape.set_length", params)], "Line length")
    }

    /// Sends one command, or several as one batch (one undo step).
    fn send_steps(&mut self, steps: &[(&str, Value)], label: &str) -> Result<(), String> {
        match steps {
            [] => Ok(()),
            [(id, params)] => self.execute(id, params).map(|_| ()).map_err(|e| e.to_string()),
            many => {
                let commands: Vec<Value> = many.iter().map(|(id, params)| json!({"command": id, "params": params})).collect();
                self.execute("command.batch", &json!({"commands": commands, "label": label})).map(|_| ()).map_err(|e| e.to_string())
            }
        }
    }

    /// The nine places of the pin on the box. The one the pin is at (the answer's `pin_at`) is marked, none when the pin is at a point of its own or the
    /// shapes differ; a press sends `shape.pin` for every shape selected. A line has no pin of its own, so the grid is quiet for it.
    fn pin_grid(&mut self, ui: &mut Ui, t: &Tokens, sel: &Selected) {
        let enabled = self.can("shape.pin") && sel.read.shapes.iter().all(|p| !p.one_d);
        let here = sel.read.shapes.iter().map(pin_place).reduce(|a, b| if a == b { a } else { None }).flatten();
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(super::fields::LABEL_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_size(vec2(super::fields::LABEL_W, 22.0));
                ui.label(egui::RichText::new("Pin Pos").font(theme::font(size::LABEL)).color(label_colour(t, enabled)));
            });
            let (rect, _) = ui.allocate_exact_size(vec2(66.0, 66.0), Sense::hover());
            ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
            self.note("pane.size.pin", rect, enabled);
            let mut pressed: Option<&'static str> = None;
            for (n, (word, fx, fy)) in PIN_PLACES.iter().enumerate() {
                let c = pos2(rect.left() + 11.0 + *fx as f32 * 44.0, rect.top() + 11.0 + (1.0 - *fy as f32) * 44.0);
                let r = egui::Rect::from_center_size(c, vec2(18.0, 18.0));
                let resp = ui.interact(r, ui.scope_id().with(("pin_place", n)), if enabled { Sense::click() } else { Sense::hover() });
                widgets::name_toggle(&resp, enabled, here == Some(n), &format!("Pin at {word}"));
                paint_button_bg(ui, r, &resp, enabled, here == Some(n), t, 9);
                let ink = if enabled { t.text } else { t.text_disabled };
                ui.painter().circle_stroke(c, 4.0, egui::Stroke::new(1.2, ink));
                if here == Some(n) {
                    ui.painter().circle_filled(c, 2.4, ink);
                }
                self.note(format!("pane.size.pin.{word}"), r, enabled);
                let resp = if enabled { resp } else { resp.on_hover_text("A line has no pin of its own: its ends are where it is") };
                if resp.clicked() && enabled {
                    pressed = Some(*word);
                }
            }
            if let Some(word) = pressed
                && here != PIN_PLACES.iter().position(|(w, _, _)| *w == word)
            {
                let params = json!({"ids": sel.ids, "at": word, "page": sel.page});
                let done = self.execute("shape.pin", &params).map(|_| None).map_err(|e| e.to_string());
                self.finish_commit(sel, "pane.size.pin", done);
            }
        });
    }
}

/// Which of the nine places a shape's pin is at, as the answer's `pin_at` says: the index into `PIN_PLACES`. `None` for a point of its own and for a line.
fn pin_place(place: &Place) -> Option<usize> {
    let at = place.pin_at.as_deref()?;
    PIN_PLACES.iter().position(|(word, _, _)| *word == at)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_answer_of_shape_placement_is_read_for_a_box_and_for_a_line() {
        let answer = json!({"page": 1, "units_per_inch": 1.0, "shapes": [
            {"id": 2, "name": "Rectangle.2", "one_d": false, "group": false, "x": 5.0, "y": 7.0, "width": 1.5, "height": 1.0, "angle": 30.0,
             "pin": {"x": 0.75, "y": 0.5}, "pin_at": "center", "flip_x": false, "flip_y": false},
            {"id": 7, "name": "Dynamic connector.7", "one_d": true, "group": false, "x": 3.0, "y": 7.0, "width": 2.0, "height": 0.0, "angle": 0.0,
             "pin": {"x": 1.0, "y": 0.0}, "pin_at": null, "flip_x": false, "flip_y": false,
             "begin": {"x": 2.0, "y": 7.0, "glued": {"id": 1, "point": 3}}, "end": {"x": 4.0, "y": 7.0, "glued": null}, "length": 2.0}
        ]});
        let read = Placements::read(&answer).unwrap();
        assert_eq!(read.shapes.len(), 2);
        assert_eq!(read.shapes[0].name, "Rectangle.2");
        assert_eq!(pin_place(&read.shapes[0]), Some(4), "the centre is the fifth place");
        assert!(pin_place(&read.shapes[1]).is_none(), "a line has none");
        let line = &read.shapes[1];
        assert_eq!(line.begin.as_ref().unwrap().glued_to, Some(1));
        assert_eq!(line.end.as_ref().unwrap().glued_to, None);
        assert_eq!(line.length, Some(2.0));
        assert!(Placements::read(&json!({"shapes": [{"id": 1}]})).is_none(), "an entry without its numbers is not read");
        assert!(Placements::read(&json!({})).is_none());
    }

    #[test]
    fn a_custom_pin_is_at_none_of_the_nine_places() {
        let mut place =
            Placements::read(&json!({"shapes": [{"id": 1, "x": 1.0, "y": 1.0, "width": 1.0, "height": 1.0, "angle": 0.0, "pin_at": "custom"}]}))
                .unwrap()
                .shapes
                .remove(0);
        assert_eq!(pin_place(&place), None);
        place.pin_at = Some("bottom-right".to_owned());
        assert_eq!(pin_place(&place), Some(8));
    }

    #[test]
    fn the_window_takes_as_many_shapes_as_one_engine_command_does() {
        assert_eq!(MAX_SHAPES, 4_096);
    }
}
