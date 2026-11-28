//! The glue points of a shape of the file, written when the model changed its connection points (CPDOUBLE.ODG).
//!
//! A shape DiaCraft writes new writes every connection point it has as its own glue point ([`super::geometry::glue_points_text`]). A shape **of the file** keeps the element it has, and
//! the splice used to write none of the points an edit added: the rows rode in DiaCraft's data alone, so a program that does not read it (LibreOffice) had no point to attach a connector
//! to, though the connector's end named the point's number. A doubling or a halving of the points of a shape of the file changes the rows; this writes them as the file says
//! a glue point:
//!
//! * **A custom shape whose style says its rows are its own** (`diacraft:glue="own"`: a shape DiaCraft wrote with points beyond the four standard ones) lists every row in the
//!   `draw:glue-points` of its geometry, from row 0, with `draw:glue-point-leaving-directions` when every row has a way out; glue id *r* + 4 is row *r*. The attributes are written again
//!   from the rows, and a `draw:glue-point` element the shape has is taken out (the reader would make a row of it).
//! * **Any other shape of a kind whose standard points LibreOffice numbers on its frame** (a custom shape, a rectangle, an ellipse, a circle) has the four standard points, the points its
//!   geometry lists (*k* of them, rows 4 on) and, for each row past those, a `draw:glue-point` child with the row's number as its id, as lengths in centimetres from the frame's centre in
//!   LibreOffice's own form with an escape direction. The elements the file has are kept while their row is as it was, and the others are taken out; a row the file has no element for
//!   gets one. **A shape turned or flipped in the model has its elements in the page's axes**: LibreOffice places such an element there, from the box the shape takes up on
//!   the page, so the element is written at the place the reader's own mapping takes back to the row ([`super::geometry::GlueFrame`], [`crate::read::Place::page_share`]).
//!
//! **The model writer writes the same elements** ([`ModelPoints`]): a shape the `.odg` reader made, written again from the model with no file to splice into, keeps the elements and the
//! geometry its file held, and a row the session added had none.
//!
//! The marker of the style (`diacraft:glue`) is the file's and stays as it is. Nothing is written for a section the elements cannot say (rows not numbered from 0 with none missing, a place
//! that is no number, more rows than [`super::geometry::MOST_GLUE_POINTS`]): the rows ride in DiaCraft's data as before.

use super::WriteError;
use super::attrs::TagEdit;
use super::compare::PointsEdit;
use super::frame::Editor;
use super::geometry::{GlueFrame, GluePoint, glue_point_attributes, glue_points_listed, glue_points_of};
use super::scan::{Kid, Kind, ScanPage, ScanShape};
use crate::read::names::Ns;
use crate::read::{Kindof, glue_box};
use diacraft_doc::{Row, Sheet};

/// Whether two glue points are the same place and way out: places within a millionth of the frame.
fn same_point(a: &GluePoint, b: &GluePoint) -> bool {
    let near = |p: f64, q: f64| (p - q).abs() <= 1e-9;
    near(a.x, b.x)
        && near(a.y, b.y)
        && match (a.way, b.way) {
            (None, None) => true,
            (Some(p), Some(q)) => near(p.0, q.0) && near(p.1, q.1),
            _ => false,
        }
}

/// A `draw:glue-point` element the shape has: where it is in the part, and the id it names.
struct Placed {
    lead: usize,
    end: usize,
    id: Option<usize>,
}

impl Editor<'_, '_> {
    /// Writes the glue points of the shape of the file whose element is `shape` (ordinal `ord`) for the edit `edit` of its connection rows (see the module documentation). Gives the
    /// attributes written on the shape's geometry element, for the check of the bytes made; none when nothing was written there.
    pub(crate) fn glue_edit(
        &mut self,
        scan: &ScanPage,
        shape: &ScanShape,
        ord: u32,
        edit: &PointsEdit,
    ) -> Result<Vec<(Ns, &'static str)>, WriteError> {
        if !matches!(shape.kind, Kind::Custom | Kind::Rect | Kind::Ellipse | Kind::Circle) {
            return Ok(Vec::new());
        }
        self.work.charge_steps(edit.now.len().saturating_add(edit.was.len()))?;
        let kids: Vec<Kid> = self.kids.get(shape.kids.0 as usize..shape.kids.1 as usize).unwrap_or_default().to_vec();
        let geometry = kids.iter().find_map(|k| if let Kid::Geometry { open, scope, .. } = k { Some((*open, *scope)) } else { None });
        let mut placed: Vec<Placed> = Vec::new();
        for kid in &kids {
            if let Kid::Glue { open, lead, end, scope } = kid {
                let tag = TagEdit::untracked(self.bytes, *open, *scope, self.work)?;
                let id = tag.find(&mut self.scopes, Ns::Draw, "id", self.work).and_then(|at| tag.text(at).trim().parse::<usize>().ok());
                placed.push(Placed { lead: *lead, end: *end, id });
            }
        }
        let own = shape.kind == Kind::Custom && self.file_glue_own(scan, ord)?;
        if own {
            return self.glue_listed(geometry, &placed, edit);
        }
        // Where LibreOffice measures the points of this kind of shape (a whole ellipse's own extent, a frame's box, an arc's own frame), at the shape's turn and mirrors in the model.
        let Some(frame) = self.glue_frame(shape, edit)? else { return Ok(Vec::new()) };
        // The points the geometry lists are rows 4 on: only a custom shape lists any.
        let listed = match (shape.kind, geometry) {
            (Kind::Custom, Some(geometry)) => match self.listed_points(geometry)? {
                Some(pairs) => pairs,
                None => return unwritten(edit).map(|()| Vec::new()),
            },
            _ => 0,
        };
        self.glue_elements(shape, &kids, placed, listed.saturating_add(4), edit, &frame)?;
        Ok(Vec::new())
    }

    /// The frame the glue points of the shape of the file are written in: the edit's size, turn and mirrors, with the box LibreOffice measures this kind of shape's points in
    /// ([`glue_box`]; a whole ellipse's is its own extent, which the shape's `draw:kind` says: an arc, a cut or a section is measured in its own frame). `None` for a shape with no size.
    fn glue_frame(&mut self, shape: &ScanShape, edit: &PointsEdit) -> Result<Option<GlueFrame>, WriteError> {
        let Some(mut frame) = edit.frame else { return Ok(None) };
        let (kind, ellipse) = match shape.kind {
            Kind::Custom => (Kindof::Custom, false),
            Kind::Rect => (Kindof::Rect, false),
            Kind::Ellipse => (Kindof::Ellipse, true),
            _ => (Kindof::Circle, true),
        };
        let draw_kind = if ellipse {
            let tag = TagEdit::untracked(self.bytes, shape.open, shape.scope, self.work)?;
            tag.find(&mut self.scopes, Ns::Draw, "kind", self.work).map(|at| tag.text(at))
        } else {
            None
        };
        frame.place.boxed = glue_box(kind, draw_kind.as_deref());
        Ok(Some(frame))
    }

    /// How many glue points the shape's geometry element lists that the reader makes rows of: `None` for a geometry the reader stops at before them (an extrusion, a fontwork, a view box with
    /// no width or height), whose rows the writer cannot place.
    fn listed_points(&mut self, (open, scope): ((usize, usize), u32)) -> Result<Option<usize>, WriteError> {
        let tag = TagEdit::untracked(self.bytes, open, scope, self.work)?;
        let mut get = |ns: Ns, local: &str| tag.find(&mut self.scopes, ns, local, self.work).map(|at| tag.text(at));
        let (extrusion, text_path, view_box) = (get(Ns::Draw, "extrusion"), get(Ns::Draw, "text-path"), get(Ns::Svg, "viewBox"));
        if crate::read::reads_no_glue_points(extrusion.as_deref(), text_path.as_deref(), view_box.as_deref()) {
            return Ok(None);
        }
        Ok(Some(get(Ns::Draw, "glue-points").map_or(0, |value| value.split_ascii_whitespace().count() / 2)))
    }

    /// A custom shape whose rows are its own glue points: every row is listed in its geometry again, and the `draw:glue-point` elements it has are taken out.
    fn glue_listed(
        &mut self,
        geometry: Option<((usize, usize), u32)>,
        placed: &[Placed],
        edit: &PointsEdit,
    ) -> Result<Vec<(Ns, &'static str)>, WriteError> {
        let Some((open, scope)) = geometry else { return Ok(Vec::new()) };
        if self.listed_points((open, scope))?.is_none() {
            return Ok(Vec::new());
        }
        let Some((points, ways)) = glue_points_listed(&edit.now) else { return Ok(Vec::new()) };
        let mut tag = TagEdit::untracked(self.bytes, open, scope, self.work)?;
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        self.write(&mut tag, Ns::Draw, "glue-points", &points)?;
        wrote.push((Ns::Draw, "glue-points"));
        match ways {
            Some(ways) => {
                self.write(&mut tag, Ns::Draw, "glue-point-leaving-directions", &ways)?;
                wrote.push((Ns::Draw, "glue-point-leaving-directions"));
            }
            // A way out some row lacks is no list of directions: the attribute goes, or the file would say the wrong way out for the rows after it.
            None => {
                if let Some(at) = tag.find(&mut self.scopes, Ns::Draw, "glue-point-leaving-directions", self.work) {
                    self.work.charge_attribute(self.limits)?;
                    tag.remove(&mut self.splicer, at)?;
                }
            }
        }
        for element in placed {
            self.splicer.remove(element.lead, element.end);
        }
        Ok(wrote)
    }

    /// The `draw:glue-point` elements of a shape whose rows 0 to `first` - 1 are the four standard points and the geometry's own (see the module documentation): the ones the file has whose
    /// row is as it was stay, the others go, and each row from `first` on that has none gets one.
    fn glue_elements(
        &mut self,
        shape: &ScanShape,
        kids: &[Kid],
        placed: Vec<Placed>,
        first: usize,
        edit: &PointsEdit,
        frame: &GlueFrame,
    ) -> Result<(), WriteError> {
        if edit.now.len() < first {
            return unwritten(edit);
        }
        let mut kept: Vec<usize> = Vec::new();
        let (mut keep_end, mut taken): (Option<usize>, Vec<&Placed>) = (None, Vec::new());
        for element in &placed {
            let stays = !edit.moved
                && element.id.is_some_and(|id| {
                    id >= first && !kept.contains(&id) && matches!((edit.now.get(id), edit.was.get(id)), (Some(a), Some(b)) if same_point(a, b))
                });
            match (stays, element.id) {
                (true, Some(id)) => {
                    kept.push(id);
                    keep_end = Some(element.end);
                }
                _ => taken.push(element),
            }
        }
        let pfx = super::emit::Pfx::of(&mut self.scopes, shape.scope, self.work);
        let mut made = String::new();
        for (row, point) in edit.now.iter().enumerate().skip(first) {
            if kept.contains(&row) {
                continue;
            }
            let Some(element) = element_text(&pfx, true, row, point, frame) else { return unwritten(edit) };
            self.work.charge_attribute(self.limits)?;
            made.push_str(&element);
        }
        self.work.charge_copy(made.len(), self.limits)?;
        // The new elements go where the schema puts them: after the title and the description and the elements already there, before the text.
        let mut rest = taken.into_iter();
        if made.is_empty() {
            for element in rest {
                self.splicer.remove(element.lead, element.end);
            }
            return Ok(());
        }
        let after_alt = kids.iter().filter_map(|k| if let Kid::Alt { end, .. } = k { Some(*end) } else { None }).max();
        if shape.end == shape.open.1 && shape.close_start == shape.open.1 {
            self.open_with(shape, &[(false, made)]);
            return Ok(());
        }
        if let Some(end) = keep_end {
            self.splicer.insert(end, made);
        } else if let Some(first_taken) = rest.next() {
            // The first element taken out is where the new ones stand: one edit of that range, so that no insertion meets a removal.
            self.splicer.replace(first_taken.lead, first_taken.end, made);
        } else if let Some(end) = after_alt {
            self.splicer.insert(end, made);
        } else {
            self.splicer.insert_first(shape.open.1, made);
        }
        for element in rest {
            self.splicer.remove(element.lead, element.end);
        }
        Ok(())
    }
}

/// What an edit of the points that cannot be written does: nothing, as before, **but for a turn the elements were to be written again for**: the shape's plan has gone on as if they would be
/// (it does not refuse the turn), so the save is refused in words.
fn unwritten(edit: &PointsEdit) -> Result<(), WriteError> {
    if edit.moved { Err(WriteError::Unsupported(super::compare::WORDS_TURNED_GLUE)) } else { Ok(()) }
}

/// The text of a `draw:glue-point` element for row `row` of a shape in `frame`, under the prefixes `pfx` ([`glue_point_attributes`]): the declarations the prefixes need go on it when
/// `declare` (an element put where the shape's own tag does not carry them). `None` when the place is out of range.
pub(crate) fn element_text(pfx: &super::emit::Pfx, declare: bool, row: usize, point: &GluePoint, frame: &GlueFrame) -> Option<String> {
    let attrs = glue_point_attributes(row, point, frame)?;
    let mut text = format!("<{}:glue-point{}", pfx.draw, if declare { pfx.declare.as_str() } else { "" });
    for (local, value) in &attrs {
        let ns = if matches!(*local, "x" | "y") { Ns::Svg } else { Ns::Draw };
        text.push_str(&pfx.attr(ns, local, value));
    }
    text.push_str("/>");
    Some(text)
}

/// The id a `draw:glue-point` element's attributes name (`draw:id`, whatever prefix the file binds to ODF's drawing namespace): `None` when it names none, or a number that is none.
fn element_id(attrs: &diacraft_doc::RawAttrs) -> Option<usize> {
    attrs
        .iter()
        .find(|(name, _)| *name != "xml:id" && name.rsplit_once(':').map_or(*name, |(_, local)| local) == "id")
        .and_then(|(_, value)| value.trim().parse().ok())
}

/// Whether the row `ix` of a shape is **as the file's `draw:glue-point` element gave it**: the element is held beside the row, with the row's own id, and the stamp of the row's cells is the
/// one the reader set it with ([`diacraft_doc::stamp_row`]).
fn row_as_read(row: &Row, ix: usize) -> bool {
    row.ext.get().and_then(|e| e.odf.held.as_ref()).is_some_and(|h| {
        h.group == diacraft_doc::HeldGroup::Glue && h.stamp == Some(diacraft_doc::stamp_row(row)) && element_id(&h.attrs) == Some(ix)
    })
}

/// **What the model writer writes of the `draw:glue-point` elements of a shape the `.odg` reader made**: the shape is written again from the model, with the elements its file
/// had among the children it keeps, and the model's rows may differ from the file's. As the splice writes them ([`Editor::glue_elements`]): the elements of the rows from `first` on that are as
/// they were read stay, the others are taken out, and each row from `first` on that has none gets one, in LibreOffice's own form. An element whose id is below `first` (a point the geometry
/// lists), or that has no usable id, makes no row and stays as the file had it.
pub(crate) struct ModelPoints {
    first: usize,
    keep: std::collections::HashSet<usize>,
    /// The elements made for the rows that have none of the file's, in row order, as text.
    pub made: String,
}

impl ModelPoints {
    /// The elements of a shape whose `Connection` rows are `sheet`'s, for the rows from `first` on: `None` when the rows are not a list the elements can say (not numbered from 0 with none
    /// missing, a place that is no number, past [`super::geometry::MOST_GLUE_POINTS`], fewer than `first`) or a place is out of range, and then the shape's elements are written as the
    /// file had them, its rows riding in DiaCraft's data as before.
    ///
    /// **`moved` is whether the shape is turned or flipped other than it was when the file was read** ([`super::shapes::asread::place_as_read`]): an element of the file is in the page's
    /// axes, so it is the row it made only in the turn and the flips it was read in, and a turn or a flip made in the session leaves it where it was, to be read back as another place
    /// than the row's. Then no element is kept, and every row from `first` on is written its own, for the frame the shape has now.
    pub(crate) fn of(sheet: &Sheet, first: usize, frame: &GlueFrame, moved: bool, pfx: &super::emit::Pfx) -> Option<ModelPoints> {
        let points = glue_points_of(sheet)?;
        if points.len() < first {
            return None;
        }
        let section = sheet.sections.iter().find(|s| s.name.as_str() == "Connection")?;
        let mut keep = std::collections::HashSet::new();
        let mut made = String::new();
        for (k, (row, point)) in section.rows.iter().zip(&points).enumerate().skip(first) {
            if !moved && row_as_read(row, k) {
                keep.insert(k);
            } else {
                made.push_str(&element_text(pfx, false, k, point, frame)?);
            }
        }
        Some(ModelPoints { first, keep, made })
    }

    /// Whether the shape's kept `draw:glue-point` element, whose attributes are `attrs`, is written: not an element that stands for a row that was taken away or edited.
    pub(crate) fn keeps(&self, attrs: &diacraft_doc::RawAttrs) -> bool {
        match element_id(attrs) {
            Some(id) if id >= self.first && id < diacraft_doc::MAX_ROWS_PER_SECTION => self.keep.contains(&id),
            _ => true,
        }
    }
}

/// The numbers of an attribute that lists them (`draw:glue-points`, `draw:glue-point-leaving-directions`): `None` when any is no finite number.
fn numbers_of(text: &str) -> Option<Vec<f64>> {
    text.split(|c: char| c.is_ascii_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().ok().filter(|n| n.is_finite()))
        .collect()
}

/// Whether two lists of numbers are the same within a millionth.
fn same_numbers(a: &str, b: &str) -> bool {
    match (numbers_of(a), numbers_of(b)) {
        (Some(p), Some(q)) => p.len() == q.len() && p.iter().zip(&q).all(|(x, y)| (x - y).abs() <= 1e-6),
        _ => false,
    }
}

/// **The geometry a shape whose rows are its own glue points writes**: the held `draw:enhanced-geometry` `el` with its `draw:glue-points` and
/// `draw:glue-point-leaving-directions` made again from `sheet`'s rows, every row listed from row 0 as a new shape lists them ([`glue_points_listed`]), and how many it lists. `None` when the
/// element already lists the rows (within a millionth: the element is written as it was held), or the rows are not a list a geometry can say.
pub(crate) fn relisted(el: &diacraft_doc::RawElement, sheet: &Sheet) -> Option<(diacraft_doc::RawElement, usize)> {
    let points = glue_points_of(sheet)?;
    let (text, ways) = glue_points_listed(&points)?;
    let local = |name: &str| name.rsplit_once(':').map_or(name.to_owned(), |(_, l)| l.to_owned());
    let named = |wanted: &str| el.attrs.iter().find(|(name, _)| local(name) == wanted).map(|(_, value)| value.to_owned());
    let (listed, directions) = (named("glue-points"), named("glue-point-leaving-directions"));
    let same_points = listed.as_deref().is_some_and(|value| same_numbers(value, &text));
    let same_ways = match (&directions, &ways) {
        (None, None) => true,
        (Some(value), Some(ways)) => same_numbers(value, ways),
        _ => false,
    };
    if same_points && same_ways {
        return None;
    }
    let prefix = el.name.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
    let mut pairs: Vec<(String, String)> = Vec::new();
    let (mut wrote_points, mut wrote_ways) = (false, false);
    for (name, value) in el.attrs.iter() {
        match local(name).as_str() {
            "glue-points" => {
                pairs.push((name.to_owned(), text.clone()));
                wrote_points = true;
            }
            "glue-point-leaving-directions" => {
                if let Some(ways) = &ways {
                    pairs.push((name.to_owned(), ways.clone()));
                }
                wrote_ways = true;
            }
            _ => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    if !wrote_points {
        pairs.push((format!("{prefix}glue-points"), text));
    }
    if let (false, Some(ways)) = (wrote_ways, &ways) {
        pairs.push((format!("{prefix}glue-point-leaving-directions"), ways.clone()));
    }
    let attrs = diacraft_doc::RawAttrs::from_pairs(pairs);
    Some((diacraft_doc::RawElement { name: el.name.clone(), attrs, children: el.children.clone() }, points.len()))
}
