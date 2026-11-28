//! A shape's text: the runs read from its cells, laid out by `diacraft_text`, and drawn as glyph outlines.
//!
//! **Where the text sits**: the layout works in the text block's own frame, paper inches
//! with the origin at the block's lower left. A point of that frame reaches the shape's: minus the block's local pin, flipped
//! like the shape, turned by `TxtAngle`, plus the block's pin. The shape's own transform then applies, and the page's scale at the end.
//! The flip appears in both transforms and cancels, so text is never mirrored, and nothing here flips a glyph.
//!
//! **Flips of the groups above** (no text is mirrored by any group above it). A group's flip reaches a member's text through the
//! shape's transform only, so there is nothing to cancel it. The walk carries the flips of the groups down ([`Flips`]) and the block's
//! transform takes them as well as the shape's own: the block is flipped about its local pin by the flips of the shape and of every group above
//! it, two of one axis cancelling. The block's pin goes where the groups' flips put it and the block keeps its shape about the pin, so the text
//! is never mirrored, reads as the specification says, and is turned as its rule says at every level (ours: the specification
//! gives the angle rule for a shape's own flip, and the same rule at each level is the least that is consistent with it).
//!
//! **Cost.** A layout of characters no face covers took 75 ms for 2,048 of them (the T7 review), and a face's outline is parsed again on
//! each call. So the renderer keeps the glyph outlines it has parsed and the layouts it has made across frames, counts the characters of
//! each frame against a budget before it lays any out, and stops laying out when the frame's text time is spent.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::lang;
use diacraft_doc::{CellAddr, RowKey, SectionKey, SheetRef, Text, TextPart};
use diacraft_fonts::Face;
use diacraft_text::{BlockFormat, CharFormat, GlyphRun, Laid, Note, ParaFormat, Run, TabStop, lay_out};
use kurbo::{Affine, BezPath, Rect, Shape as _};

use crate::face::{face_name, row_cell, run_font};
use crate::frame::{Flips, Frame};
use crate::layers::LayerColour;
use crate::limits::{MAX_GLYPHS_PER_BLOCK, MAX_LAYOUT_CACHE, MAX_OUTLINE_CACHE, WORK_PER_GLYPH};
use crate::read::{Cx, Val};
use crate::skips::SkipKind;
use crate::target::{Paint, Refused, Target, TextRun, TextSource};

/// The most characters of one block that are laid out (the layout takes more, up to 262,144; the largest real text in the census holds
/// 8,197). A block past it is cut and the frame says so.
pub(crate) const MAX_BLOCK_CHARS: usize = 16_384;

/// The fewest entries a share's cache keeps, however many shares there are.
const MIN_SHARE_CACHE: usize = 64;

/// What the renderer keeps between frames for text.
pub(crate) struct Caches {
    /// Outlines by face and glyph; `None` for a glyph the face draws as blank.
    outlines: HashMap<(u64, u32), Option<Arc<BezPath>>>,
    /// Laid-out blocks by a hash of everything the layout was given.
    layouts: HashMap<u64, Arc<Laid>>,
    /// How many blocks have been laid out since the renderer was made (not put back to 0 by [`Caches::clear`]).
    laid_out: u64,
    /// Pictures decoded.
    pub(crate) pictures: crate::picture::Pictures,
    /// How many outlines and layouts are kept before the table is emptied.
    max_outlines: usize,
    max_layouts: usize,
}

impl Default for Caches {
    fn default() -> Caches {
        Caches::with_caps(MAX_OUTLINE_CACHE, MAX_LAYOUT_CACHE)
    }
}

impl Caches {
    /// Caches that keep at most `max_outlines` outlines and `max_layouts` layouts.
    pub(crate) fn with_caps(max_outlines: usize, max_layouts: usize) -> Caches {
        Caches {
            outlines: HashMap::new(),
            layouts: HashMap::new(),
            laid_out: 0,
            pictures: crate::picture::Pictures::default(),
            max_outlines,
            max_layouts,
        }
    }

    /// One of `parts` caches that together keep what a whole one does (`share.rs`: each share of a parallel walk keeps its own).
    pub(crate) fn one_of(parts: usize) -> Caches {
        let parts = parts.max(1);
        Caches::with_caps((MAX_OUTLINE_CACHE / parts).max(MIN_SHARE_CACHE), (MAX_LAYOUT_CACHE / parts).max(MIN_SHARE_CACHE))
    }

    /// How many blocks have been laid out.
    pub(crate) fn laid_out(&self) -> u64 {
        self.laid_out
    }

    pub(crate) fn clear(&mut self) {
        self.outlines.clear();
        self.layouts.clear();
        self.pictures.clear();
    }

    pub(crate) fn outline(&mut self, face: &Face, gid: u32) -> Option<Arc<BezPath>> {
        let key = (face.id(), gid);
        if let Some(known) = self.outlines.get(&key) {
            return known.clone();
        }
        if self.outlines.len() >= self.max_outlines {
            self.outlines.clear();
        }
        let outline = face.outline(gid).filter(|p| !p.is_empty()).map(Arc::new);
        self.outlines.insert(key, outline.clone());
        outline
    }
}

/// The text a shape shows: its own `Text`, or when it has none its master shape's. An own `Text` with nothing in it is the shape's
/// text and shows nothing: the master's plays no part.
fn text_of(cx: &Cx<'_, '_>, at: &diacraft_doc::ShapeRef) -> Option<Arc<Text>> {
    let shape = cx.res.shape(at)?;
    if let Some(own) = &shape.text {
        return Some(Arc::clone(own));
    }
    match cx.res.master_step(at) {
        Ok(Some(step)) => cx.res.shape(&step.shape).and_then(|m| m.text.clone()),
        _ => None,
    }
}

fn char_format(cx: &Cx<'_, '_>, at: &SheetRef, ix: u32, layer: Option<&LayerColour>) -> CharFormat {
    let cell = |name: &str| row_cell("Character", ix, name);
    // The face is read by the function the readers of `FaceReader` use too (`face.rs`): what the window shows is what is drawn.
    let font = run_font(cx, at, ix);
    let defaults = CharFormat::default();
    let (color, transparency) = match layer {
        // A layer's colour is laid over the text of a shape on that one layer.
        Some(l) => (l.color, l.trans),
        None => (cx.colour_at(at, &cell("Color")).unwrap_or(Rgba::BLACK), cx.num_at(at, &cell("ColorTrans"), 0.0)),
    };
    CharFormat {
        font,
        asian_font: face_name(cx, at, &cell("AsianFont")),
        complex_font: face_name(cx, at, &cell("ComplexScriptFont")),
        size: cx.num_at(at, &cell("Size"), defaults.size),
        complex_size: cx.num_at(at, &cell("ComplexScriptSize"), defaults.complex_size),
        style: cx.num_at(at, &cell("Style"), 0.0).clamp(0.0, 255.0) as u8,
        case: cx.num_at(at, &cell("Case"), 0.0).clamp(0.0, 255.0) as u8,
        pos: cx.num_at(at, &cell("Pos"), 0.0).clamp(0.0, 255.0) as u8,
        scale: cx.num_at(at, &cell("FontScale"), 1.0),
        letterspace: cx.num_at(at, &cell("Letterspace"), 0.0),
        color,
        transparency,
        double_underline: cx.flag_at(at, &cell("DblUnderline"), false),
        overline: cx.flag_at(at, &cell("Overline"), false),
        strikethrough: cx.flag_at(at, &cell("Strikethru"), false),
        double_strikethrough: cx.flag_at(at, &cell("DoubleStrikethrough"), false),
        lang: run_language(cx, at, &cell("LangID")),
    }
}

/// The language a run is drawn in: what its `LangID` means in this drawing (`lang::effective`, design 5.1), as the tag the shaper and `Case` read, and empty for no
/// language. A run with no cell anywhere on its chain is in the document's language, and a value that says none (`x-none`, `zxx`, empty, 0) is in none. The text measure
/// (`diacraft_calc`, `TEXTWIDTH` and `TEXTHEIGHT`) reads a run's language by the same two functions, so that a text measured is the text drawn.
fn run_language(cx: &Cx<'_, '_>, at: &SheetRef, addr: &CellAddr) -> String {
    lang::shaping_tag(lang::saved_value(&cx.res, at, addr), cx.document_language())
}

fn para_format(cx: &Cx<'_, '_>, at: &SheetRef, ix: u32, tabs: Vec<TabStop>) -> ParaFormat {
    let cell = |name: &str| row_cell("Paragraph", ix, name);
    let d = ParaFormat::default();
    let byte = |name: &str, default: u8| cx.num_at(at, &cell(name), f64::from(default)).clamp(0.0, 255.0) as u8;
    ParaFormat {
        bullet: byte("Bullet", d.bullet),
        bullet_font: face_name(cx, at, &cell("BulletFont")),
        bullet_size: cx.num_at(at, &cell("BulletFontSize"), d.bullet_size),
        bullet_string: cx.text_at(at, &cell("BulletStr")).map(|t| t.to_string()).unwrap_or_default(),
        flags: byte("Flags", d.flags),
        halign: byte("HorzAlign", d.halign),
        ind_first: cx.num_at(at, &cell("IndFirst"), 0.0),
        ind_left: cx.num_at(at, &cell("IndLeft"), 0.0),
        ind_right: cx.num_at(at, &cell("IndRight"), 0.0),
        sp_after: cx.num_at(at, &cell("SpAfter"), 0.0),
        sp_before: cx.num_at(at, &cell("SpBefore"), 0.0),
        sp_line: cx.num_at(at, &cell("SpLine"), d.sp_line),
        text_pos_after_bullet: cx.num_at(at, &cell("TextPosAfterBullet"), 0.0),
        tabs,
    }
}

/// The stops of the `Tabs` row `ix`: cells `Position<j>` and `Alignment<j>` for each stop `j` (TAB-R*).
fn tab_stops(cx: &Cx<'_, '_>, at: &SheetRef, ix: u32) -> Vec<TabStop> {
    let key = SectionKey::new("Tabs", None);
    let Some(rows) = cx.res.effective_rows(at, &key) else {
        return Vec::new();
    };
    let Some(row) = rows.iter().find(|r| r.key == RowKey::Ix(ix)) else {
        return Vec::new();
    };
    let mut stops = Vec::new();
    for (name, _) in &row.cells {
        let Some(j) = name.as_str().strip_prefix("Position") else {
            continue;
        };
        if stops.len() >= 64 || j.is_empty() || !j.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let position = cx.num_at(at, &row_cell("Tabs", ix, name.as_str()), 0.0);
        let alignment = cx.num_at(at, &row_cell("Tabs", ix, &format!("Alignment{j}")), 0.0).clamp(0.0, 255.0) as u8;
        stops.push(TabStop { position, alignment });
    }
    stops
}

fn block_format(cx: &Cx<'_, '_>, at: &SheetRef, height: Option<f64>) -> BlockFormat {
    let d = BlockFormat::default();
    let background = match cx.val(at, &CellAddr::direct("TextBkgnd")) {
        // A colour as `#RRGGBB` is itself: the "plus one" of Learn's formula is not in the value.
        Val::Color(c) => Some((c, cx.num(at, "TextBkgndTrans"))),
        // 0 and 255 say none. Any other number is a colour of the table, **one less than the number** (our rule):
        // Learn's "plus one" makes 1 the table's first colour, so 2, the number files hold, is white and not the red of `vColor`'s index 2.
        // **The one number to change, if a render disagrees:** read `n` for `n - 1.0`.
        Val::Num(n) if n != 0.0 && n != 255.0 => Some((cx.indexed_colour(n - 1.0), cx.num(at, "TextBkgndTrans"))),
        _ => None,
    };
    BlockFormat {
        left_margin: cx.num(at, "LeftMargin"),
        right_margin: cx.num(at, "RightMargin"),
        top_margin: cx.num(at, "TopMargin"),
        bottom_margin: cx.num(at, "BottomMargin"),
        vertical_align: cx.num_or(at, "VerticalAlign", f64::from(d.vertical_align)).clamp(0.0, 255.0) as u8,
        text_direction: cx.num(at, "TextDirection").clamp(0.0, 255.0) as u8,
        default_tab_stop: cx.num_or(at, "DefaultTabStop", d.default_tab_stop),
        background,
        height,
    }
}

/// The runs of a text, with the formats in force for each stretch. Characters past `MAX_BLOCK_CHARS` are cut; the number
/// cut is returned.
fn build_runs(cx: &Cx<'_, '_>, at: &SheetRef, text: &Text, layer: Option<&LayerColour>) -> (Vec<Run>, usize, usize) {
    let (mut cp, mut pp, mut tp) = (0u32, 0u32, 0u32);
    let mut chars: HashMap<u32, Arc<CharFormat>> = HashMap::new();
    let mut paras: HashMap<(u32, u32), Arc<ParaFormat>> = HashMap::new();
    let mut runs = Vec::new();
    let (mut total, mut cut) = (0usize, 0usize);
    for part in &text.parts {
        let body: &str = match part {
            TextPart::Cp(ix) => {
                cp = *ix;
                continue;
            }
            TextPart::Pp(ix) => {
                pp = *ix;
                continue;
            }
            TextPart::Tp(ix) => {
                tp = *ix;
                continue;
            }
            TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => continue,
            TextPart::Chars(s) => s,
            TextPart::Fld { text, .. } => text,
        };
        // A stretch of the file's own indentation between markers draws nothing.
        if body.contains('\n') && body.contains(' ') && body.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
            continue;
        }
        let room = MAX_BLOCK_CHARS.saturating_sub(total);
        let n = body.chars().count();
        let piece: String = if n > room { body.chars().take(room).collect() } else { body.to_owned() };
        cut += n - piece.chars().count();
        total += piece.chars().count();
        let char_fmt = Arc::clone(chars.entry(cp).or_insert_with(|| Arc::new(char_format(cx, at, cp, layer))));
        let para_fmt = Arc::clone(paras.entry((pp, tp)).or_insert_with(|| Arc::new(para_format(cx, at, pp, tab_stops(cx, at, tp)))));
        runs.push(Run::new(piece, char_fmt, para_fmt));
    }
    (runs, total, cut)
}

fn hash_of(parts: &[&dyn std::fmt::Debug]) -> u64 {
    let mut h = DefaultHasher::new();
    for part in parts {
        format!("{part:?}").hash(&mut h);
    }
    h.finish()
}

impl<T: Target> Frame<'_, '_, '_, T> {
    /// Draws the text of the shape at `at`. `affine` takes the shape's frame to the page's drawing units, `groups` are the flips of the groups
    /// above the shape (not its own), and `scale` takes drawing units to paper inches.
    pub(crate) fn draw_text(
        &mut self,
        at: &diacraft_doc::ShapeRef,
        affine: Affine,
        groups: Flips,
        scale: f64,
        layer: Option<&LayerColour>,
        who: &dyn Fn() -> String,
    ) {
        let cx = self.cx;
        let Some(text) = text_of(cx, at) else {
            return;
        };
        if text.parts.is_empty() {
            return;
        }
        let sheet = crate::read::sheet_of(at);
        let (runs, chars, cut) = build_runs(cx, &sheet, &text, layer);
        if runs.is_empty() || chars == 0 {
            return;
        }
        if cut > 0 {
            cx.note_n(SkipKind::TextNote, cut, || format!("{}: {cut} characters past {MAX_BLOCK_CHARS} are not laid out", who()));
        }
        // The block: its size is in drawing units, its text in paper inches.
        let (tw, th) = (cx.num(&sheet, "TxtWidth"), cx.num(&sheet, "TxtHeight"));
        let width = tw * scale;
        let height = th * scale;
        let block = block_format(cx, &sheet, height.is_finite().then_some(height));
        let key = hash_of(&[&runs, &block, &width.to_bits()]);
        let laid = match self.caches.layouts.get(&key) {
            Some(known) => Arc::clone(known),
            None => {
                if self.text_chars.saturating_add(chars) > self.limits.max_text_chars {
                    cx.note(SkipKind::TextCharBudget, who);
                    return;
                }
                if self.limits.text_time.is_some_and(|limit| self.text_time > limit) {
                    cx.note(SkipKind::TextBudget, who);
                    return;
                }
                self.text_chars += chars;
                let started = self.clock.now();
                let laid = Arc::new(lay_out(&runs, &block, width.is_finite().then_some(width), self.fonts));
                self.caches.laid_out = self.caches.laid_out.saturating_add(1);
                self.text_time += self.clock.since(started);
                if self.caches.layouts.len() >= self.caches.max_layouts {
                    self.caches.layouts.clear();
                }
                self.caches.layouts.insert(key, Arc::clone(&laid));
                laid
            }
        };
        self.note_text(&laid, who);
        // A target that writes letters as text is given the characters the glyphs stand for.
        let owned = self.canvas.wants_text().then(|| OwnedSource::of(&runs));
        // With the shape's own flips; the block's units are paper inches, and the shape's drawing units. The flips of the groups above
        // are cancelled in the same place, about the same point, the block's local pin, and in the same way: the shape's
        // flip is in `affine` once and in the block's transform once, a group's flip is in `affine` only, so the block's transform takes it too.
        // Two flips of one axis, at any two levels, are none.
        let finite = |n: f64| if n.is_finite() { n } else { 0.0 };
        let flipped = Flips::of(cx, &sheet).then(groups);
        let flip_x = if flipped.x { -1.0 } else { 1.0 };
        let flip_y = if flipped.y { -1.0 } else { 1.0 };
        let inverse_scale = if scale > 0.0 { 1.0 / scale } else { 1.0 };
        let to_shape = Affine::translate((finite(cx.num(&sheet, "TxtPinX")), finite(cx.num(&sheet, "TxtPinY"))))
            * Affine::rotate(finite(cx.num(&sheet, "TxtAngle")))
            * Affine::scale_non_uniform(flip_x, flip_y)
            * Affine::translate((-finite(cx.num(&sheet, "TxtLocPinX")), -finite(cx.num(&sheet, "TxtLocPinY"))));
        let to_paper = Affine::scale(scale) * affine * to_shape * Affine::scale(inverse_scale);
        self.paint_laid(&laid, to_paper, owned.as_ref().map(OwnedSource::borrow), who);
    }

    /// Lays out the runs of a label the renderer makes itself (`placeholder.rs`), **within budgets of its own** (`Limits::max_label_chars`, `Limits::label_time`: a
    /// flood of placeholders must not spend the characters and the time the file's own text has) and through the cache the file's text uses: a label at one size is
    /// laid out once however many frames hold it. `None` when the labels have no characters or time left, and the label is left off; nothing is noted, since the
    /// label is no text of the file's. (A flood of labels at sizes of their own still fills the shared cache, which costs the file's text the layouts it had kept:
    /// they are made again, within the file's own budget.)
    pub(crate) fn lay_out_label(&mut self, runs: &[Run], block: &BlockFormat, width: f64, chars: usize) -> Option<Arc<Laid>> {
        let key = hash_of(&[&runs, &block, &width.to_bits()]);
        if let Some(known) = self.caches.layouts.get(&key) {
            return Some(Arc::clone(known));
        }
        if self.label_chars.saturating_add(chars) > self.limits.max_label_chars || self.limits.label_time.is_some_and(|limit| self.label_time > limit)
        {
            return None;
        }
        self.label_chars += chars;
        let started = self.clock.now();
        let laid = Arc::new(lay_out(runs, block, Some(width).filter(|w| w.is_finite()), self.fonts));
        self.caches.laid_out = self.caches.laid_out.saturating_add(1);
        self.label_time += self.clock.since(started);
        if self.caches.layouts.len() >= self.caches.max_layouts {
            self.caches.layouts.clear();
        }
        self.caches.layouts.insert(key, Arc::clone(&laid));
        Some(laid)
    }

    fn note_text(&self, laid: &Laid, who: &dyn Fn() -> String) {
        for note in &laid.notes {
            match note {
                Note::MissingGlyphs { clusters } => {
                    self.cx.note_n(SkipKind::MissingGlyphs, *clusters, || format!("{}: letters no face draws", who()))
                }
                // A face the file names that the machine lacks: the faces are named in the list the window shows (`SkipKind::FaceStandIn`).
                Note::StandIn { requested, used } => self.cx.note_face(requested, used),
                Note::StandInsOmitted { .. } => self.cx.note_faces_unnamed(),
                other => self.cx.note(SkipKind::TextNote, || format!("{}: {other:?}", who())),
            }
        }
    }

    /// Paints a laid-out block through `to_paper`, which takes the block's frame to paper inches. With a `source` the runs are first offered
    /// to the target as text ([`Target::text_run`]); a run it declines is drawn as outlines.
    pub(crate) fn paint_laid(&mut self, laid: &Laid, to_paper: Affine, source: Option<TextSource<'_>>, who: &dyn Fn() -> String) {
        if let Some(bg) = &laid.background {
            let rect: Rect = bg.rect;
            let path = to_paper * rect.to_path(0.1);
            self.fill_checked(&path, Rgba { a: alpha_of(bg.color.a, bg.opacity), ..bg.color }, who);
        }
        let mut glyphs = 0usize;
        for line in &laid.lines {
            for (n, run) in line.runs.iter().chain(line.bullet.iter()).enumerate() {
                if run.opacity <= 0.0 {
                    continue;
                }
                // A bullet's glyphs stand for no characters of the text: only the runs are offered.
                if let Some(source) = source
                    && n < line.runs.len()
                {
                    if self.offer_text(run, source, to_paper, &mut glyphs) {
                        continue;
                    }
                    self.cx.note(SkipKind::TextAsOutlines, || format!("{}: letters written as outlines", who()));
                }
                let scale = run.size / run.face.units_per_em().max(1.0);
                // The glyphs are work, counted before they are painted: the same block in a thousand shapes is laid out once and painted a
                // thousand times.
                let painted = run.glyphs.len().min(MAX_GLYPHS_PER_BLOCK.saturating_sub(glyphs));
                if !self.charge((painted as u64).saturating_mul(WORK_PER_GLYPH), who) {
                    return;
                }
                let lying = if run.turned { Affine::rotate(-std::f64::consts::FRAC_PI_2) } else { Affine::IDENTITY };
                let mut combined = BezPath::new();
                for g in &run.glyphs {
                    if glyphs >= MAX_GLYPHS_PER_BLOCK {
                        self.cx.note(SkipKind::GlyphsCut, who);
                        break;
                    }
                    let Some(outline) = self.caches.outline(&run.face, g.gid) else {
                        continue;
                    };
                    glyphs += 1;
                    // A glyph is its outline scaled by size over units per em (x also by `x_scale`), sheared for a made-up slant, turned a quarter
                    // clockwise about its origin when it lies on its side in vertical text, and moved to its point; its y is already up
                    // (`laid.rs`).
                    let shear = Affine::new([1.0, 0.0, run.slant, 1.0, 0.0, 0.0]);
                    let place = Affine::translate((g.x, g.y)) * lying * shear * Affine::scale_non_uniform(scale * run.x_scale, scale);
                    combined.extend((place * (*outline).clone()).elements().iter().copied());
                }
                if combined.is_empty() {
                    continue;
                }
                let paper = to_paper * combined;
                let color = Rgba { a: alpha_of(run.color.a, run.opacity), ..run.color };
                if self.fill_checked(&paper, color, who) && run.embolden > 0.0 {
                    let stroke = kurbo::Stroke::new(run.embolden).with_join(kurbo::Join::Round).with_caps(kurbo::Cap::Round);
                    self.canvas.stroke(&paper, &stroke, &Paint::Solid(color));
                }
            }
            for d in &line.decorations {
                if d.opacity <= 0.0 {
                    continue;
                }
                let path = to_paper * d.rect().to_path(0.1);
                self.fill_checked(&path, Rgba { a: alpha_of(d.color.a, d.opacity), ..d.color }, who);
            }
        }
    }

    /// Offers a run to the target as text. True when nothing is left to draw for it: the target wrote it, or it lies wholly outside the
    /// target. A run the target cannot write as text, one with a made-up weight or slant (which the target has no way to say), one with
    /// no face, and one that would pass the glyph budget are not offered or are declined, and the caller draws them as outlines.
    fn offer_text(&mut self, run: &GlyphRun, source: TextSource<'_>, to_paper: Affine, glyphs: &mut usize) -> bool {
        // A letter on its side (vertical text) is no `<text>`: a target sets that upright.
        let plain = !run.turned
            && run.embolden <= 0.0
            && run.slant == 0.0
            && run.x_scale.is_finite()
            && run.x_scale > 0.0
            && run.size.is_finite()
            && run.size > 0.0;
        if !plain || run.face.is_null() || run.glyphs.is_empty() || glyphs.saturating_add(run.glyphs.len()) > MAX_GLYPHS_PER_BLOCK {
            return false;
        }
        // Where the run can reach: its glyphs' points, an em to every side (a glyph's origin is on the baseline, and it may be wide).
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for g in &run.glyphs {
            x0 = x0.min(g.x);
            y0 = y0.min(g.y);
            x1 = x1.max(g.x + g.advance.abs());
            y1 = y1.max(g.y);
        }
        let reach = Rect::new(x0 - run.size, y0 - run.size, x1 + run.size * 2.0, y1 + run.size * 2.0);
        match self.canvas.check(&(to_paper * reach.to_path(0.1)), 0.0) {
            Ok(true) => {}
            Ok(false) => {
                *glyphs += run.glyphs.len();
                return true;
            }
            Err(Refused::OutOfRange) => return false,
        }
        let color = Rgba { a: alpha_of(run.color.a, run.opacity), ..run.color };
        let text = TextRun { face: &run.face, size: run.size, x_scale: run.x_scale, color, glyphs: &run.glyphs, source, to_paper };
        if self.canvas.text_run(&text) {
            *glyphs += run.glyphs.len();
            return true;
        }
        false
    }

    /// Fills `path` if it is a drawable one that reaches the view; true when it was filled.
    fn fill_checked(&mut self, path: &BezPath, color: Rgba, who: &dyn Fn() -> String) -> bool {
        match self.canvas.check(path, 0.0) {
            Ok(true) => {
                self.canvas.fill(path, &Paint::Solid(color), false);
                true
            }
            Ok(false) => false,
            Err(Refused::OutOfRange) => {
                self.cx.note(SkipKind::OutOfRange, || format!("{}: text", who()));
                false
            }
        }
    }
}

/// The characters of a block's runs in order, with where each starts, which is what a glyph's `chars` count.
struct OwnedSource {
    text: String,
    byte_at: Vec<usize>,
}

impl OwnedSource {
    fn of(runs: &[Run]) -> OwnedSource {
        let mut text = String::new();
        let mut byte_at = Vec::new();
        for run in runs {
            for c in run.text.chars() {
                byte_at.push(text.len());
                text.push(c);
            }
        }
        byte_at.push(text.len());
        OwnedSource { text, byte_at }
    }

    fn borrow(&self) -> TextSource<'_> {
        TextSource { text: &self.text, byte_at: &self.byte_at }
    }
}

fn alpha_of(alpha: u8, opacity: f64) -> u8 {
    (f64::from(alpha) * opacity.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
}
