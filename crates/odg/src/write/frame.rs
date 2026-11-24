//! What the writer does to one shape element for each change of the family *Transform*:
//! the attributes of its frame, written from the cells.
//!
//! * **A translation** (the frame moved, the same size and turn): an element with a `draw:transform` has the last translation of the list moved
//!   and every other term kept; a circle or an ellipse given by its centre has `svg:cx` and `svg:cy` moved; any other has `svg:x` and
//!   `svg:y` moved. An attribute the move does not change keeps its bytes.
//! * **A reshape** (its size or turn changed): the frame written whole in the canonical form. Unturned: `svg:x`, `svg:y`, `svg:width` and
//!   `svg:height`, with no `draw:transform`. Turned: `svg:width`, `svg:height` and `draw:transform="rotate (<a>) translate (<x> <y>)"`, with no
//!   `svg:x` or `svg:y`. A circle or an ellipse given by its centre is written in that form again (a turn is refused). A transform that held a
//!   shear, a scale or a matrix is dropped with it, and the save says so.
//! * **The ends of a segment** (`draw:line`, `draw:connector`, `draw:measure`): `svg:x1` to `svg:y2` written from the ends on the page, and
//!   `draw:transform` taken out, the ends being written already transformed.
//!
//! Every length keeps the unit its attribute was written in; one the element lacks takes the page's unit. Every write is counted
//! ([`WriteWork::attributes_written`]) and charged before it is made.

use super::attrs::TagEdit;
use super::lengths::{length_text, same};
use super::own::Pose;
use super::route;
use super::scan::{Kind, ScanShape, Scopes};
use super::splice::Splicer;
use super::transform::{flips_the_frame, has_more_than_a_turn, parse_terms, translated, turned};
use super::{SaveNote, WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::names::Ns;
use crate::read::units::{LenUnit, length};

/// What the edits of one part are made with: the part's bytes and splicer, its namespaces, the work and the limits, and the page being edited.
pub(crate) struct Editor<'a, 'w> {
    pub bytes: &'a [u8],
    pub splicer: Splicer<'a>,
    pub scopes: Scopes,
    pub work: &'w mut WriteWork,
    pub limits: &'w Limits,
    pub notes: &'w mut Vec<SaveNote>,
    /// The page: its height in inches (the y flip of R2) and the unit of its `fo:page-width`, for a length with no attribute to take one from.
    pub height: f64,
    pub unit: LenUnit,
    /// The automatic styles of the part, when the save edits styles or takes shapes out (their styles may be left with no one to name them).
    pub book: Option<super::styles::StyleBook>,
    /// What the scan found inside the shapes it was asked to look inside: their paragraphs and enhanced geometry.
    pub kids: Vec<super::scan::Kid>,
    /// The attributes written on the geometry element of a custom shape, by the element's ordinal before the save: what the check of the bytes made reads
    /// back (`outline.rs`).
    pub kid_edits: Vec<(u32, Vec<(Ns, &'static str)>)>,
    /// The paragraph style written for each paragraph a retext put in, by the paragraph's number in the model's text, and the shape's element ordinal before the
    /// save: what the check of the bytes made looks for (`text.rs`).
    pub inserted: Vec<(u32, super::StyleNames)>,
    /// What a retext expects of the zero-width elements put in the paragraphs of a shape, by the shape's element ordinal before the save (`zero.rs`).
    pub zero: Vec<(u32, Vec<super::zero::Expect>)>,
    /// What DiaCraft's extension was written to carry of each shape an edit gave a tier 2, by the shape's element ordinal before the save: the check of
    /// the bytes made reads the file back and compares each, as it does the new shapes' (`check_tier2`).
    pub tier2_edited: Vec<(u32, std::sync::Arc<crate::ext::payload::Tier2>)>,
    /// The largest number in an id of the form `id` and digits that the part holds: a new id is one more.
    pub max_id: u64,
    /// The part declares ODF 1.0 or 1.1 (or none): what 1.2 added is not written in it (`scan::is_legacy`).
    pub legacy: bool,
    /// The markers (a number and a band class) of the line ends whose styles this save wrote: their `draw:marker` elements are the save's to add to
    /// `office:styles` of `styles.xml` (`markers.rs`).
    pub markers: std::collections::BTreeSet<super::markers::Def>,
    /// The faces the part declares and the text styles this save has made or found (`text.rs`).
    pub texts: super::text::TextStyles,
    /// `office:font-face-decls` of the part, when it has one and the scan was asked for the styles: where a declaration the save adds goes.
    pub fonts: Option<super::scan::FaceDecls>,
    /// A flat drawing with no `office:font-face-decls`: where the save makes the element, before the first of the children R30 puts after it. `None` for a package.
    pub fonts_at: Option<usize>,
    /// The part is a flat drawing's: a new picture is written inline.
    pub flat: bool,
    /// The prefix of the office namespace the root's children are written under, in a flat drawing: where a new picture's `office:binary-data` is written.
    pub office: Option<String>,
    /// The new pictures of the save's package, shared by the parts written (`picture.rs`): a new picture names an entry of it. `None` in a flat drawing, whose
    /// pictures are inline.
    pub pictures: Option<&'w mut super::picture::PictureBook>,
    /// The `style:name` of each style sheet by its ID, for the parent of an automatic style a shape's links change (`newstyles.rs`).
    pub style_names: &'w std::collections::HashMap<u32, String>,
    /// What this save adds to `office:styles` of `styles.xml`, in the order it is added (the markers, then the new style sheets), put in by [`Editor::flush_common_styles`] **in one
    /// edit**: an `office:styles` that is an empty element (`<office:styles/>`) is rewritten whole to hold them, and two edits of the same bytes are refused, so that a save
    /// that adds a marker and a style sheet at once (a connector with an end drawn after a theme) was refused (found by the test of a connector after a theme).
    pub common_text: String,
    /// The markup of the file a new shape of the model path keeps, for the element being written: the attributes for its start tag, the children for its end and a custom shape's own
    /// geometry ([`Editor::kept_markup`]). Set before each new shape is written and taken by the function that writes its element.
    pub kept: super::shapes::KeptMarkup,
    /// The document being saved: its colour table and its font table, which the rows of a shape's text are read through.
    pub doc: &'w diacraft_doc::Document,
    /// The names of the common styles of the package (the other part's): a paragraph or run of the model path that names one names it by reference, and no automatic style is made under it.
    pub commons: std::collections::HashSet<String>,
    /// The text of the shape being written, made by the model writer before its element and taken by the function that writes it ([`Editor::rich_text`]).
    pub rich: Option<super::text::model::Rich>,
    /// The entries the model holds (`Document::parts`) by name, as an `href` of the markup a shape kept finds them (`package::href_entry`); made at the first kept `draw:image` that asks
    /// ([`Editor::is_the_frames_picture`]) and taken from the save's memory room for good.
    pub part_names: Option<std::collections::HashMap<String, usize>>,
    /// The names of the entries the model's pictures were read from (`Media::part`), shared with the model; made at the first kept `draw:image` link that the package has no entry of, to tell a link
    /// to the picture another frame shows (an entry the save has yet to write) from a link that names no entry ([`Editor::link_out_of_the_package`]).
    pub picture_names: Option<std::collections::HashSet<std::sync::Arc<str>>>,
}

/// A length attribute of a tag: where it is, what it reads as and the unit it was written in.
struct Held {
    at: usize,
    inches: f64,
    unit: LenUnit,
}

impl<'a> Editor<'a, '_> {
    fn tag(&mut self, shape: &ScanShape) -> Result<TagEdit<'a>, WriteError> {
        let bytes = self.bytes;
        TagEdit::new(bytes, shape.open, shape.scope, self.work)
    }

    /// Charges one attribute written.
    fn charge(&mut self) -> Result<(), WriteError> {
        self.work.charge_attribute(self.limits)
    }

    /// The length attribute `local` of `ns` on the tag, when it is there and reads as a length.
    fn held(&mut self, tag: &TagEdit<'_>, ns: Ns, local: &str) -> Option<Held> {
        let at = tag.find(&mut self.scopes, ns, local, self.work)?;
        let l = length(&tag.text(at))?;
        Some(Held { at, inches: l.inches, unit: l.unit })
    }

    /// Writes the length attribute `local` as `inches`: in its place and its unit when it is there (and not at all when it already reads as
    /// that, unless `force`), added in `fallback` when it is not. **An attribute that is there and reads as no length** (`svg:y="8(cmm"`, which the
    /// reader reads as absent) is replaced in its place, in `fallback`: adding another would make the tag name it twice, which is not XML (found
    /// by the fuzz target, once the check of the bytes made was a scan of tags and no longer a read, which refuses a repeated attribute).
    fn put(&mut self, tag: &mut TagEdit<'_>, local: &str, inches: f64, fallback: LenUnit, force: bool) -> Result<(), WriteError> {
        let words = WriteError::Unsupported("a length too large to write");
        match self.held(tag, Ns::Svg, local) {
            Some(held) if !force && same(held.inches, inches) => Ok(()),
            Some(held) => {
                self.charge()?;
                tag.set(&mut self.splicer, held.at, &length_text(inches, held.unit).ok_or(words)?)
            }
            None => {
                self.charge()?;
                let text = length_text(inches, fallback).ok_or(words)?;
                match tag.find(&mut self.scopes, Ns::Svg, local, self.work) {
                    Some(at) => tag.set(&mut self.splicer, at, &text),
                    None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Svg, local, &text),
                }
            }
        }
    }

    /// Moves the length attribute `local` by `delta` inches: added, from 0, when it is not there.
    fn shift(&mut self, tag: &mut TagEdit<'_>, local: &str, delta: f64) -> Result<(), WriteError> {
        let now = self.held(tag, Ns::Svg, local).map_or(0.0, |h| h.inches);
        let fallback = self.unit;
        self.put(tag, local, now + delta, fallback, false)
    }

    /// The unit the frame of the tag is written in: that of the first of `svg:x`, `svg:y`, `svg:width` and `svg:height` it has, else the page's.
    fn frame_unit(&mut self, tag: &TagEdit<'_>) -> LenUnit {
        for local in ["x", "y", "width", "height"] {
            if let Some(held) = self.held(tag, Ns::Svg, local) {
                return held.unit;
            }
        }
        self.unit
    }

    /// Whether the element is a circle or an ellipse given by its centre: `svg:cx`, `svg:cy` and `svg:r`, or `svg:rx` and `svg:ry`.
    fn by_centre(&mut self, tag: &TagEdit<'_>, kind: Kind) -> bool {
        let has = |e: &mut Editor<'_, '_>, local: &str| e.held(tag, Ns::Svg, local).is_some();
        match kind {
            Kind::Circle => has(self, "cx") && has(self, "cy") && has(self, "r"),
            Kind::Ellipse => has(self, "cx") && has(self, "cy") && has(self, "rx") && has(self, "ry"),
            _ => false,
        }
    }

    /// Writes a translation of the frame by (`dx`, `dy`) inches, y down the page.
    pub(crate) fn translate(&mut self, shape: &ScanShape, dx: f64, dy: f64) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        if let Some(at) = tag.find(&mut self.scopes, Ns::Draw, "transform", self.work) {
            let text = tag.text(at);
            if parse_terms(&text).is_some() {
                let new = translated(&text, dx, dy, self.unit).ok_or(WriteError::Unsupported("a length too large to write"))?;
                if new != text {
                    self.charge()?;
                    tag.set(&mut self.splicer, at, &new)?;
                }
                return Ok(());
            }
        }
        let locals: &[&str] = if self.by_centre(&tag, shape.kind) { &["cx", "cy"] } else { &["x", "y"] };
        for (local, delta) in locals.iter().zip([dx, dy]) {
            self.shift(&mut tag, local, delta)?;
        }
        Ok(())
    }

    /// Writes the frame whole, `pose` being the pose now and `angle` its turn.
    pub(crate) fn reshape(&mut self, shape: &ScanShape, pose: &Pose, angle: f64, who: (usize, u32)) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        let (w, h) = pose.size;
        let centre = (pose.centre.0, self.height - pose.centre.1);
        let transform = tag.find(&mut self.scopes, Ns::Draw, "transform", self.work);
        if let Some(at) = transform {
            let text = tag.text(at);
            if let Some(terms) = parse_terms(&text) {
                // A flip the transform holds is the reader's `FlipY`, and the frame written whole would lose it (the flips the file holds as mirrors of the
                // geometry stay, and the turn written is the turn of the frame without them: `compare::frame_change`).
                if flips_the_frame(&text, &terms) {
                    return Err(WriteError::Unsupported("a flip, a scale or a shear of a shape's frame (the flip is in the shape's transform)"));
                }
                if has_more_than_a_turn(&terms) {
                    self.notes.push(SaveNote::ShearDropped { page: who.0, shape: who.1 });
                }
            }
        }
        if self.by_centre(&tag, shape.kind) {
            return self.reshape_by_centre(&mut tag, shape.kind, pose, angle);
        }
        let unit = self.frame_unit(&tag);
        let unturned = angle == 0.0;
        self.put(&mut tag, "width", w, unit, false)?;
        self.put(&mut tag, "height", h, unit, false)?;
        if unturned {
            self.put(&mut tag, "x", centre.0 - w / 2.0, unit, false)?;
            self.put(&mut tag, "y", centre.1 - h / 2.0, unit, false)?;
            if let Some(at) = transform {
                self.charge()?;
                tag.remove(&mut self.splicer, at)?;
            }
            return Ok(());
        }
        // The frame's top-left corner after the turn, as the reader's map takes (0, 0) of the frame: the centre less the turned half diagonal.
        let (s, c) = angle.sin_cos();
        let (hx, hy) = (w / 2.0 * c + h / 2.0 * s, -w / 2.0 * s + h / 2.0 * c);
        let text = turned(angle, centre.0 - hx, centre.1 - hy, unit).ok_or(WriteError::Unsupported("a length too large to write"))?;
        self.charge()?;
        match transform {
            Some(at) => tag.set(&mut self.splicer, at, &text)?,
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "transform", &text)?,
        }
        for local in ["x", "y"] {
            if let Some(held) = self.held(&tag, Ns::Svg, local) {
                self.charge()?;
                tag.remove(&mut self.splicer, held.at)?;
            }
        }
        Ok(())
    }

    /// A circle or an ellipse given by its centre, resized or moved: written in that form again (a turn has no such form).
    fn reshape_by_centre(&mut self, tag: &mut TagEdit<'_>, kind: Kind, pose: &Pose, angle: f64) -> Result<(), WriteError> {
        if angle != 0.0 {
            return Err(WriteError::Unsupported("a turn of a circle or an ellipse given by its centre"));
        }
        let (w, h) = pose.size;
        let unit = self.held(tag, Ns::Svg, "cx").map_or(self.unit, |h| h.unit);
        let (cx, cy) = (pose.centre.0, self.height - pose.centre.1);
        self.put(tag, "cx", cx, unit, false)?;
        self.put(tag, "cy", cy, unit, false)?;
        if kind == Kind::Circle {
            if !same(w, h) {
                return Err(WriteError::Unsupported("a circle given by its centre resized to another height than width"));
            }
            self.put(tag, "r", w / 2.0, unit, false)
        } else {
            self.put(tag, "rx", w / 2.0, unit, false)?;
            self.put(tag, "ry", h / 2.0, unit, false)
        }
    }

    /// Writes the ends of a segment, on the page (inches, y up), as `svg:x1` to `svg:y2`, and takes `draw:transform` out.
    pub(crate) fn set_ends(&mut self, shape: &ScanShape, ends: [(f64, f64); 2]) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        let transform = tag.find(&mut self.scopes, Ns::Draw, "transform", self.work).filter(|at| parse_terms(&tag.text(*at)).is_some());
        let unit = ["x1", "y1", "x2", "y2"].iter().find_map(|l| self.held(&tag, Ns::Svg, l)).map_or(self.unit, |h| h.unit);
        let force = transform.is_some();
        let values = [("x1", ends[0].0), ("y1", self.height - ends[0].1), ("x2", ends[1].0), ("y2", self.height - ends[1].1)];
        for (local, inches) in values {
            self.put(&mut tag, local, inches, unit, force)?;
        }
        if let Some(at) = transform {
            self.charge()?;
            tag.remove(&mut self.splicer, at)?;
        }
        Ok(())
    }

    /// Writes `draw:z-index` as `value`: in its place when the element has one, added when it has not.
    pub(crate) fn set_z(&mut self, shape: &ScanShape, value: i64) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        self.charge()?;
        match tag.find(&mut self.scopes, Ns::Draw, "z-index", self.work) {
            Some(at) => tag.set(&mut self.splicer, at, &value.to_string()),
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "z-index", &value.to_string()),
        }
    }

    /// Writes `draw:layer` as `name`, a layer the save declares: in its place when the element has one, added when it has not.
    pub(crate) fn set_layer(&mut self, shape: &ScanShape, name: &str) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        self.charge()?;
        match tag.find(&mut self.scopes, Ns::Draw, "layer", self.work) {
            Some(at) => tag.set(&mut self.splicer, at, name),
            None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Draw, "layer", name),
        }
    }

    /// Writes a connector's path and its view box: `svg:d` and `svg:viewBox`, in place, added where the element has none.
    ///
    /// **In a part of ODF 1.0 or 1.1** a connector has neither: the route is its ends, which the frame's write has made, and the schemas
    /// of those versions have no `svg:d` or `svg:viewBox` on a `draw:connector`. Neither is added; one the file has (a file that is not valid for its
    /// version) is taken out, since it describes the route the ends no longer say.
    ///
    /// **`draw:line-skew` goes with the route** (Connectors: "remove when DiaCraft routes the connector anew", kept until then): it describes the route the
    /// file had, and a consumer that computes its own from the type and the skew would draw this one differently. It is taken out
    /// whichever version the part is, since a route has been written (or, in ODF 1.0 and 1.1, the ends say it).
    ///
    /// **In a part of ODF 1.0 or 1.1 the save says so** ([`SaveNote::RouteDroppedForVersion`], naming the connector at `who`, its page and `ID`): the route the model has is
    /// not in the file, which draws the connector as its reader routes it from the ends (the save used to say nothing).
    pub(crate) fn set_route(&mut self, shape: &ScanShape, path: &str, view_box: &str, who: (usize, u32)) -> Result<(), WriteError> {
        let mut tag = self.tag(shape)?;
        if let Some(at) = tag.find(&mut self.scopes, Ns::Draw, "line-skew", self.work) {
            self.charge()?;
            tag.remove(&mut self.splicer, at)?;
        }
        if self.legacy {
            for local in ["d", "viewBox"] {
                if let Some(at) = tag.find(&mut self.scopes, Ns::Svg, local, self.work) {
                    self.charge()?;
                    tag.remove(&mut self.splicer, at)?;
                }
            }
            self.notes.push(SaveNote::RouteDroppedForVersion { page: who.0, shape: who.1 });
            return Ok(());
        }
        for (local, text) in [("d", path), ("viewBox", view_box)] {
            self.charge()?;
            match tag.find(&mut self.scopes, Ns::Svg, local, self.work) {
                Some(at) => tag.set(&mut self.splicer, at, text)?,
                None => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Svg, local, text)?,
            }
        }
        Ok(())
    }

    /// Moves the path of a connector the save keeps `by` hundredths of a millimetre down the page: the connector no edit touched, through a change of the page's height
    /// The page's height edit keeps `PinY`, so every end moves down the page by what the page grew, and the file's
    /// path, which is a fixed distance from the page's top, goes with them: **`svg:d` is shifted** ([`route::shifted`]: the first absolute `M` moves and the relative steps do not,
    /// in a path of absolute points every point moves), **`svg:viewBox` and `draw:line-skew` stay as they were**, and a connector with no `svg:d` has nothing to shift.
    ///
    /// Gives `false` when the file's path cannot be placed (it breaks SVG's syntax, does not begin with a move, or holds a number a path cannot): its bytes are left as
    /// they are and the caller says so. It is the same in a part of ODF 1.0 or 1.1, whose connector has no place for a path: one that holds a path is shifted as any other,
    /// and none is made.
    ///
    /// **The memory is charged before the text is made**: what the pass may hold, the path and up to twenty times as much text
    /// ([`route::shift_peak`]), is taken from the memory a save may hold first and given back once the text is known, and the text that stays until the part is written is
    /// charged as the splicer keeps it, escaped ([`TagEdit::written_len`]).
    pub(crate) fn shift_route(&mut self, shape: &ScanShape, by: f64) -> Result<bool, WriteError> {
        const WHAT: &str = "memory for the paths a save shifted";
        let tag = self.tag(shape)?;
        let Some(at) = tag.find(&mut self.scopes, Ns::Svg, "d", self.work) else { return Ok(true) };
        let path = tag.text(at);
        // **What the pass may hold is charged before it is made**: the text it makes is up to twenty times the path, and is reserved by the pass; the bound is given back once
        // the text is known, and what is kept for good (below) is charged in its place.
        let peak = route::shift_peak(path.len());
        self.work.hold(peak, WHAT)?;
        let moved = route::shifted(&path, by);
        match moved {
            Some(moved) if moved != path => {
                // The splicer keeps the value as it is written, **escaped** (a line feed is five bytes): that is what stays until the part is written, and is charged, with
                // the bound still held for the escaped text is made beside the text made.
                self.work.hold(tag.written_len(at, &moved), WHAT)?;
                self.work.release(peak);
                self.charge()?;
                tag.set(&mut self.splicer, at, &moved)?;
                Ok(true)
            }
            other => {
                self.work.release(peak);
                Ok(other.is_some())
            }
        }
    }

    /// Takes the glue of one end of a connector out (R9 delete): `draw:start-shape` or `draw:end-shape`, and its glue point.
    pub(crate) fn unglue(&mut self, shape: &ScanShape, start: bool) -> Result<(), WriteError> {
        let tag = self.tag(shape)?;
        let names = if start { ["start-shape", "start-glue-point"] } else { ["end-shape", "end-glue-point"] };
        for local in names {
            if let Some(at) = tag.find(&mut self.scopes, Ns::Draw, local, self.work) {
                self.charge()?;
                tag.remove(&mut self.splicer, at)?;
            }
        }
        Ok(())
    }
}
