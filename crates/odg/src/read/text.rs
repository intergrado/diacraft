//! A shape's text.
//!
//! * **Paragraphs and runs**: each `text:p` (and `text:h`) is a paragraph. A paragraph's own properties are those
//!   its `text:style-name` style sets itself; a run's are those its `text:span`'s text style sets itself, with its paragraph's character
//!   properties. Each distinct set of what a paragraph's chain resolves is one `Paragraph` row, and each distinct set of what a run's chain
//!   resolves one `Character` row, numbered from 0 in order of first use. Row 0 holds the own properties' cells and leaves the rest to the
//!   shape's links; a row of `IX` 1 or more holds every cell its chain resolves, the size always, since in the model it inherits by
//!   `IX` through a style row that does not exist. A span's chain is flattened with the enclosing ones into one layer, so a span nested
//!   deep costs what one alone does. A `pp` marker opens each paragraph whose row differs from the one before, and a `cp` marker
//!   each run whose row differs; at the start, the row before is row 0 when row 0 holds no cells of its own (text with no marker uses row 0),
//!   and none when it holds some, so a first row with cells is opened by its marker. `cp` comes before `pp` where both open one place.
//! * **White space**: in a paragraph, runs of white space characters collapse to one space, and leading and trailing ones go; then
//!   `text:s` stands for `text:c` spaces (one when absent), `text:tab` for U+0009 and `text:line-break` for U+2028. Paragraphs are joined by
//!   U+000A, and a single empty paragraph is empty text (no text and no rows).
//! * **Lists**: a `text:list` whose list style's level is a bullet level makes its items' paragraphs bulleted (`Bullet`, `BulletStr`,
//!   `BulletFont`, `BulletFontSize`, `IndLeft`, `IndFirst` in the paragraph's own set); a numbered level gives no cell and is kept.
//! * **Fields**: `text:page-number`, `text:page-count`, `text:file-name`, and a `text:date` or `text:time` that is not fixed, are
//!   `Field` rows whose value is `PAGENUMBER()`, `PAGECOUNT()`, `FILENAME()` or `NOW()`, with the shown text as the cached value. Any other
//!   field reads as its shown text and is kept.
//!
//! Tab stops of a paragraph's own style are kept and not modelled: the shape's tab stops are its `Tabs` row 0 (`props.rs`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Cell, MAX_LIST_LEVELS, OdfListLevel, RawAttrs, RawElement, RawNode, Text, TextPart};
use diacraft_fonts::script::FontSlot;

use super::Reader;
use super::cells::SHORT_TEXT;
use super::names::{Names, Ns};
use super::props::{self, Out, Serves, View};
use super::shapes::Cx;
use super::sheets;
use super::styles::{Family, Kind, ListLevel, P, Resolved, list_level};
use super::units;
use crate::error::OdgError;

/// The bullet characters of `Bullet` 1 to 7.
const BULLETS: [(char, u8); 7] =
    [('\u{2022}', 1), ('\u{25C6}', 2), ('\u{25AA}', 3), ('\u{25A1}', 4), ('\u{2756}', 5), ('\u{27A2}', 6), ('\u{2713}', 7)];

/// The text of a shape and the rows it needs.
#[derive(Default)]
pub(crate) struct TextOut {
    pub text: Option<Text>,
    /// The cells of each `Paragraph` row, by `IX`.
    pub paragraphs: Vec<Vec<(&'static str, Cell)>>,
    /// The cells of each `Character` row, by `IX`.
    pub characters: Vec<Vec<(&'static str, Cell)>>,
    /// The `Field` rows, by `IX`: each value cell.
    pub fields: Vec<Cell>,
    /// Whether the text holds the frame of an object: as a child of the text box or in a paragraph. The model keeps nothing of such a frame, so the caller marks the
    /// shape.
    pub holds_object: bool,
    /// What each paragraph of the file named, in the file's order: its style's name, where each span begins and ends, and a stamp of its characters. The
    /// text's `OdfText` is made of it once the rows are in the shape's sheet.
    pub odf: Vec<ParaOdf>,
}

/// What a `text:span` named, or another element of the paragraph's text, and where it lay in its paragraph's characters.
pub(crate) struct SpanOdf {
    pub style: Option<usize>,
    pub name: Option<Arc<str>>,
    pub from: usize,
    pub to: usize,
    /// The `Character` row of the span's own text, when it has any.
    pub row: Option<u32>,
    /// Whether it is a `text:span` (its style is held with it), and not a hyperlink, a field or another element.
    pub span: bool,
    /// Its element, when it is not a `text:span` with no attribute but its style name.
    pub element: Option<Arc<RawElement>>,
    /// Where it was opened among the elements of its paragraph: of two runs over the same characters the one opened first is the outer.
    pub opened: usize,
}

/// What a paragraph named: see [`TextOut::odf`].
pub(crate) struct ParaOdf {
    pub style: Option<usize>,
    pub name: Option<Arc<str>>,
    /// The `Paragraph` row.
    pub row: u32,
    /// The `Character` row of the paragraph's own text, the characters no span holds (they read through the paragraph's style, text properties included), when it has any.
    pub own_row: Option<u32>,
    /// A stamp of the paragraph's characters and where its spans lie, as read.
    pub characters: diacraft_doc::Stamp,
    pub spans: Vec<SpanOdf>,
    /// Its element, when it is not a `text:p` with no attribute but its style name.
    pub element: Option<Arc<RawElement>>,
    /// The lists it is in, each held by the paragraph that opens it.
    pub levels: Vec<OdfListLevel>,
}

/// A list the reader is in: the elements of it that no paragraph has yet taken.
struct OpenLevel {
    list: Arc<RawElement>,
    item: Option<Arc<RawElement>>,
    list_fresh: bool,
    item_fresh: bool,
}

/// The element `el` as the model holds the element of a paragraph, a list or a run: its name and attributes as written but `skip`, the name as written of the style name a
/// `style` of the record holds, and none of its children. Charged before it is made.
fn shell(r: &mut Reader, names: &Names<'_>, el: &El, skip: Option<&str>) -> Result<Arc<RawElement>, OdgError> {
    let kept = || el.attrs.iter().filter(|(k, _)| Some(k.as_str()) != skip);
    let (mut items, mut bytes) = (3usize, el.name.len());
    for (k, v) in kept() {
        items = items.saturating_add(2);
        bytes = bytes.saturating_add(k.len()).saturating_add(v.len());
    }
    r.take_items(items.saturating_add(super::cost::text(bytes).div_ceil(super::ITEM_BYTES)))?;
    let mut made = RawElement {
        name: Arc::from(el.name.as_str()),
        attrs: RawAttrs::from_pairs(kept().map(|(k, v)| (k.as_str(), v.as_str()))),
        children: Vec::new(),
    };
    // The prefixes the element uses that an element above it declares on its own start tag are declared on the element.
    super::shapes::bind_locals(r, names, &mut made)?;
    Ok(Arc::new(made))
}

/// The name as written of the `text:style-name` attribute of `el`, if it has one.
fn style_attr<'t>(names: &Names<'t>, el: &'t El) -> Option<&'t str> {
    names.attrs(el).iter().find(|a| a.ns == Ns::Text && a.local == "style-name").map(|a| a.name)
}

/// Whether `el` has an attribute but the one named `skip`.
fn has_other_attrs(el: &El, skip: Option<&str>) -> bool {
    el.attrs.iter().any(|(k, _)| Some(k.as_str()) != skip)
}

/// A set of cells as a key, for numbering distinct sets.
///
/// A long value is named by where it is held, not copied: such values are a style's texts, one shared value for each (`StyleTexts::value`),
/// so a run that shows a family of a million characters makes a key of a few bytes, and two runs of one style make the same key. The
/// address names the text only while the text is alive and in its place, which a shared value is for the whole read (the reader holds it);
/// a debug build asserts that every long text a key names is one (`Reader::assert_shared_long`, which preferred a
/// type that only `StyleTexts` can make: that would change the cell type of every conversion in `props.rs`, `sheets.rs` and here, so the
/// assertion stands at the one place the address is used, which every cell of every text row passes through).
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn key_of(r: &Reader, cells: &[(&'static str, Cell)]) -> String {
    let value = |c: &Cell| match c.value.as_ref().map(diacraft_doc::Value::text) {
        Some(text) if text.len() > SHORT_TEXT => {
            #[cfg(debug_assertions)]
            r.assert_shared_long(text);
            format!("@{:x}:{}", text.as_ptr() as usize, text.len())
        }
        Some(text) => text.to_owned(),
        None => String::new(),
    };
    let mut parts: Vec<String> =
        cells.iter().map(|(n, c)| format!("{n}={}|{}|{}", value(c), c.formula.text().unwrap_or(""), c.unit.as_deref().unwrap_or(""))).collect();
    parts.sort();
    parts.join(";")
}

/// Rows numbered by first use of each distinct set of what a run's (or a paragraph's) chain resolves.
#[derive(Default)]
struct Numbering {
    by_key: HashMap<String, u32>,
    rows: Vec<Vec<(&'static str, Cell)>>,
}

impl Numbering {
    /// The row of a run (or a paragraph) whose chain resolves `full`, given `base`, what the shape's own chain resolves (plan Rulings
    ///). Row 0 holds every cell `full` has that `base` does not give (the cells of the run's styles and of their parents,
    /// wherever they differ from what the shape's links give) and leaves the rest to the shape's links; a row of `IX` 1 or more holds
    /// every cell, since in the model it inherits by `IX` through a style row that does not exist.
    fn row(&mut self, r: &mut Reader, base: &[(&'static str, Cell)], full: Vec<(&'static str, Cell)>) -> Result<u32, OdgError> {
        let key = key_of(r, &full);
        if let Some(ix) = self.by_key.get(&key) {
            return Ok(*ix);
        }
        let cells = if self.rows.is_empty() { differing(r, full, base) } else { full };
        r.take_cells(cells.len())?;
        r.take_items(super::cost::ROW)?;
        r.take_bytes(key.len())?;
        let ix = u32::try_from(self.rows.len()).unwrap_or(u32::MAX);
        self.by_key.insert(key, ix);
        self.rows.push(cells);
        Ok(ix)
    }
}

/// The cells of `full` that `base` does not give: those whose name `base` lacks, or whose value, formula or unit differ.
fn differing(r: &mut Reader, full: Vec<(&'static str, Cell)>, base: &[(&'static str, Cell)]) -> Vec<(&'static str, Cell)> {
    r.cells_scanned(full.len().saturating_mul(base.len()));
    let mut out = Vec::new();
    for (name, cell) in full {
        let same = base.iter().find(|(n, _)| *n == name).is_some_and(|(_, b)| props::same_cell(r, &cell, b) && cell.unit == b.unit);
        if !same {
            out.push((name, cell));
        }
    }
    out
}

/// What the shape's own text chain resolves, as paragraph and character cells: what the shape's links and local cells give a run that no
/// style of its own changes.
struct Base {
    paragraph: Vec<(&'static str, Cell)>,
    character: Vec<(&'static str, Cell)>,
}

/// One stretch of a paragraph's text on one `Character` row, or a field.
enum Piece {
    Chars {
        text: String,
        row: u32,
    },
    Field {
        ix: u32,
        shown: String,
        row: u32,
    },
    /// A zero-width node of the file that the text keeps whole: a bookmark, a reference mark, a soft page break (`TextPart::OdfRaw`).
    Raw(RawNode),
}

/// A paragraph as it is read.
struct Para {
    row: u32,
    pieces: Vec<Piece>,
    odf: ParaOdf,
}

/// What reading one shape's text keeps.
struct State<'v> {
    shape_layers: &'v [(Rc<Resolved>, Serves)],
    size: f64,
    paragraphs: Numbering,
    characters: Numbering,
    fields: Vec<Cell>,
    paras: Vec<Para>,
    /// What the shape's chain resolves, worked out when the first row is made (a shape with no text costs none).
    base: Option<Rc<Base>>,
    /// Whether the text holds the frame of an object ([`TextOut::holds_object`]).
    holds_object: bool,
    /// The lists the reader is in, outermost first.
    levels: Vec<OpenLevel>,
}

/// Reads the text children of `container` (a shape, or a frame's `draw:text-box`): its `text:p`, `text:h` and `text:list`. `skip` says
/// which other children belong to the shape and are not text. The shape's text chain is `shape_layers`, and `size` its text's size.
pub(crate) fn read<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    container: &'t El,
    shape_layers: &[(Rc<Resolved>, Serves)],
    size: f64,
) -> Result<TextOut, OdgError> {
    let mut st = State {
        shape_layers,
        size,
        paragraphs: Numbering::default(),
        characters: Numbering::default(),
        fields: Vec::new(),
        paras: Vec::new(),
        base: None,
        holds_object: false,
        levels: Vec::new(),
    };
    for child in container.elements() {
        let mark = names.enter(child);
        st.holds_object |= super::shapes::frame_holds_object(names, child);
        match names.element(child) {
            (Ns::Text, "p" | "h") => {
                r.saw_element();
                paragraph(r, names, cx, child, &mut st, None)?;
            }
            (Ns::Text, "list") => {
                r.saw_element();
                list(r, names, cx, child, &mut st, None, 1)?;
            }
            _ => {}
        }
        names.leave(mark);
    }
    finish(r, st)
}

/// Whether `el` is a text child of a shape (what [`read`] reads).
pub(crate) fn is_text<'t>(names: &Names<'t>, el: &'t El) -> bool {
    matches!(names.element(el), (Ns::Text, "p" | "h" | "list"))
}

/// A `text:list` at `level` (1 for the outermost). A list that names a list style looks it up, hashing the name once; one that names none
/// reads the style of the list around it (`inherited`), already found, so that nested lists inheriting a long name do not hash it again
/// each, and the levels are borrowed from the style set and not copied. The level the list's depth names is found by a binary search.
fn list<'t, 's>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'s>,
    el: &'t El,
    st: &mut State<'_>,
    inherited: Option<&'s [ListLevel]>,
    level: u32,
) -> Result<(), OdgError> {
    let attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let set = cx.set;
    let levels = match attrs.peek(Ns::Text, "style-name") {
        Some(name) => {
            r.name_bytes(name.len());
            set.list_style(name, cx.file)
        }
        None => inherited,
    };
    let this = levels.and_then(|ls| list_level(r, ls, level));
    // The list's own element is held by the first paragraph in it, and each item's by the item's first.
    if st.levels.len() >= MAX_LIST_LEVELS {
        return Err(OdgError::TooComplex { what: "levels of lists round a paragraph", max: MAX_LIST_LEVELS as u64 });
    }
    let at = st.levels.len();
    let shell_list = shell(r, names, el, None)?;
    st.levels.push(OpenLevel { list: shell_list, item: None, list_fresh: true, item_fresh: false });
    for item in el.elements() {
        r.saw_element();
        let mark = names.enter(item);
        let header = names.is(item, Ns::Text, "list-header");
        if names.is(item, Ns::Text, "list-item") || header {
            let shell_item = shell(r, names, item, None)?;
            if let Some(open) = st.levels.get_mut(at) {
                open.item = Some(shell_item);
                open.item_fresh = true;
            }
            for child in item.elements() {
                r.saw_element();
                let inner = names.enter(child);
                match names.element(child) {
                    (Ns::Text, "p" | "h") => paragraph(r, names, cx, child, st, if header { None } else { this })?,
                    (Ns::Text, "list") => list(r, names, cx, child, st, levels, level.saturating_add(1))?,
                    _ => r.keep("text_element"),
                }
                names.leave(inner);
            }
            if let Some(open) = st.levels.get_mut(at) {
                open.item = None;
                open.item_fresh = false;
            }
        } else {
            r.keep("text_element");
        }
        names.leave(mark);
    }
    st.levels.truncate(at);
    Ok(())
}

/// The cells a list level adds to its paragraphs' own set. The level's strings are the style set's, held for as long as the cache of
/// its texts lives (`texts.rs`), which is what lets each be one shared value.
fn bullet_cells<'s>(r: &mut Reader, cx: &mut Cx<'s>, level: &'s ListLevel) -> Result<Vec<(&'static str, Cell)>, OdgError> {
    let mut out = Vec::new();
    if level.numbered {
        r.keep("numbered_list");
        return Ok(out);
    }
    let Some(bullet) = level.bullet.as_deref() else {
        r.keep("list_level");
        return Ok(out);
    };
    let mut chars = bullet.chars();
    let first = chars.next();
    match first.and_then(|c| BULLETS.iter().find(|(b, _)| *b == c)).filter(|_| chars.next().is_none()) {
        Some((_, n)) => out.push(("Bullet", super::cells::plain(f64::from(*n)))),
        None => {
            out.push(("Bullet", super::cells::plain(1.0)));
            // The file's characters, and its font's family below: one shared value for every paragraph that shows them.
            out.push(("BulletStr", Cell::constant(cx.texts.value(r, bullet)?)));
        }
    }
    let declared = match level.font_name.as_deref() {
        Some(name) => cx.texts.face(r, name, cx.file)?,
        None => None,
    };
    // The family was cut of its white space and quotes when the list style was read.
    let face = declared.or(level.font_family.as_deref());
    if let Some(face) = face {
        out.push(("BulletFont", Cell::constant(cx.texts.value(r, face)?)));
    }
    // The size was read when the list style was read.
    match (level.font_size.is_some(), level.font_size_length) {
        (_, Some(l)) => out.push(("BulletFontSize", super::cells::length(l.inches, l.unit.word()))),
        (true, None) => r.keep("property_value"),
        (false, None) => {}
    }
    if let Some(l) = level.margin_left {
        out.push(("IndLeft", super::cells::length(l.inches, l.unit.word())));
    }
    if let Some(l) = level.text_indent {
        out.push(("IndFirst", super::cells::length(l.inches, l.unit.word())));
    }
    Ok(out)
}

/// What a paragraph style or a text style sets itself, as the cells it gives: the paragraph cells and the character cells.
fn own_cells(r: &mut Reader, cx: &mut Cx<'_>, style: Option<usize>, layers: &[(Rc<Resolved>, Serves)], size_base: f64) -> Result<Out, OdgError> {
    let Some(at) = style else { return Ok(Out::default()) };
    let resolved = cx.res.resolved(r, cx.set, at)?;
    let mut all = vec![(resolved, Serves::All)];
    all.extend(layers.iter().cloned());
    let view = View { set: cx.set, layers: all };
    let set = cx.set;
    let own = |kind: Kind, p: P| set.get(at).is_some_and(|s| s.own(kind, p).is_some());
    let size =
        if set.get(at).is_some_and(|s| s.own(Kind::Text, P::FontSize).is_some()) { Some(cx.res.font_size(r, set, at, size_base)) } else { None };
    let mut out = Out::default();
    props::paragraph(r, &view, &own, &mut out);
    let base = cx.sheets.base();
    props::character(r, &mut cx.texts, &view, &own, size, &base, FontSlot::Latin, &mut out)?;
    if set.get(at).is_some_and(|s| s.tabs.is_some()) {
        r.keep("paragraph_tabs");
    }
    // `DefaultTabStop` is the shape's (a text-block cell), not a paragraph's.
    if !out.direct.is_empty() {
        r.keep_n("paragraph_tabs", out.direct.len() as u64);
        out.direct.clear();
    }
    Ok(out)
}

/// Every paragraph and character cell the chain `layers` resolves, with the size always: what a row of `IX` 1 or more holds. What
/// these conversions keep is counted once, by the run's own conversion, so the counts are put back. `slot` is the script type of the run's letters, which its language is read through.
fn full_cells(r: &mut Reader, cx: &mut Cx<'_>, layers: &[(Rc<Resolved>, Serves)], size_base: f64, slot: FontSlot) -> Result<Out, OdgError> {
    let mut present: HashSet<(Kind, P)> = HashSet::new();
    for (resolved, serves) in layers {
        for (key, _) in resolved.iter() {
            r.style_step();
            if *serves != Serves::ParagraphDefault || key.0 != Kind::Graphic {
                present.insert(*key);
            }
        }
    }
    present.insert((Kind::Text, P::FontSize));
    let counted = ["property_value", "text_position"].map(|k| r.report.kept.get(k).copied());
    let view = View { set: cx.set, layers: layers.to_vec() };
    let size = sheets::view_size(r, cx.set, cx.res, &view, size_base).unwrap_or(size_base);
    let own = |k: Kind, p: P| present.contains(&(k, p));
    let mut out = Out::default();
    props::paragraph(r, &view, &own, &mut out);
    let base = cx.sheets.base();
    props::character(r, &mut cx.texts, &view, &own, Some(size), &base, slot, &mut out)?;
    // `DefaultTabStop` is the shape's, not a row's.
    out.direct.clear();
    for (k, before) in ["property_value", "text_position"].into_iter().zip(counted) {
        match before {
            Some(n) => {
                r.report.kept.insert(k, n);
            }
            None => {
                r.report.kept.remove(k);
            }
        }
    }
    Ok(out)
}

/// The layers a span's runs read through: its style's chain over the enclosing ones, as one layer over the default styles, so that a span
/// nested in many costs what one alone does (the security review's finding 3).
fn span_layers(
    r: &mut Reader,
    cx: &mut Cx<'_>,
    style: Option<usize>,
    outer: &[(Rc<Resolved>, Serves)],
) -> Result<Vec<(Rc<Resolved>, Serves)>, OdgError> {
    let mut chains: Vec<Rc<Resolved>> = Vec::with_capacity(outer.len() + 1);
    if let Some(at) = style {
        chains.push(cx.res.resolved(r, cx.set, at)?);
    }
    let mut defaults = Vec::new();
    for (resolved, serves) in outer {
        if *serves == Serves::All {
            chains.push(Rc::clone(resolved));
        } else {
            defaults.push((Rc::clone(resolved), *serves));
        }
    }
    let refs: Vec<&Resolved> = chains.iter().map(Rc::as_ref).collect();
    let flat = Resolved::flattened(&refs);
    for _ in 0..flat.len() {
        r.style_step();
    }
    let mut layers = Vec::with_capacity(defaults.len() + 1);
    layers.push((Rc::new(flat), Serves::All));
    layers.extend(defaults);
    Ok(layers)
}

/// The layers a paragraph's runs read through: its style's chain over the shape's.
fn paragraph_layers(
    r: &mut Reader,
    cx: &mut Cx<'_>,
    style: Option<usize>,
    shape: &[(Rc<Resolved>, Serves)],
) -> Result<Vec<(Rc<Resolved>, Serves)>, OdgError> {
    let mut layers = Vec::with_capacity(shape.len() + 1);
    if let Some(at) = style {
        layers.push((cx.res.resolved(r, cx.set, at)?, Serves::All));
    }
    layers.extend(shape.iter().cloned());
    Ok(layers)
}

/// A paragraph (`text:p` or `text:h`), bulleted by `bullet` when it is a list item's.
fn paragraph<'t, 's>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'s>,
    el: &'t El,
    st: &mut State<'_>,
    bullet: Option<&'s ListLevel>,
) -> Result<(), OdgError> {
    let attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let style = attrs.peek(Ns::Text, "style-name").and_then(|name| cx.set.find(r, Family::Paragraph, name, cx.file));
    let style_name = super::held::named(r, cx.shared, &attrs, Ns::Text, "style-name")?;
    // What the paragraph is besides its style name: a heading, or a paragraph that has an attribute of its own; and the lists round it that it opens.
    let skip = style_attr(names, el);
    let element = if !names.is(el, Ns::Text, "p") || has_other_attrs(el, skip) { Some(shell(r, names, el, skip)?) } else { None };
    let levels = take_levels(r, st)?;
    // The paragraph's own conversion counts what the styles keep without a cell, once (`full_cells` puts the counts back).
    own_cells(r, cx, style, st.shape_layers, st.size)?;
    let layers = paragraph_layers(r, cx, style, st.shape_layers)?;
    let mut full_para = full_cells(r, cx, &layers, st.size, FontSlot::Latin)?.paragraph;
    if let Some(level) = bullet {
        for (name, cell) in bullet_cells(r, cx, level)? {
            full_para.retain(|(n, _)| *n != name);
            full_para.push((name, cell));
        }
    }
    let base = shape_base(r, cx, st)?;
    let row = st.paragraphs.row(r, &base.paragraph, full_para)?;
    let mut run = Run { pieces: Vec::new(), space: Space::Start, trailing: None, count: 0, spans: Vec::new(), opened: 0 };
    let own_row = content(r, names, cx, el, st, &layers, &mut run, 0)?;
    // Trailing white space goes.
    if let Some((piece, at)) = run.trailing
        && let Some(Piece::Chars { text, .. }) = run.pieces.get_mut(piece)
    {
        run.count = run.count.saturating_sub(text.get(at..).map_or(0, |gone| gone.chars().count()));
        text.truncate(at);
        // **The spans end where the characters do**: a span, or a hyperlink, that closed after the white space that went ended one character past the paragraph's text. The
        // writer finds the file's paragraph for the model's by the stamp of its characters and where its runs lie, and passes by a paragraph whose runs end past the characters, so a
        // paragraph whose last run ended in a space was written with none of its hyperlinks (4 links in, 2 out).
        let end = run.count;
        for span in &mut run.spans {
            span.from = span.from.min(end);
            span.to = span.to.min(end);
        }
    }
    run.pieces.retain(|p| !matches!(p, Piece::Chars { text, .. } if text.is_empty()));
    // The spans in the order they begin, an outer span before an inner one that begins where it does.
    let mut spans = std::mem::take(&mut run.spans);
    spans.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)).then(a.opened.cmp(&b.opened)));
    let characters = characters_stamp(&run.pieces, &spans, run.count);
    // What the paragraph named is held until the text's `.odg` data is made, and is charged here, before it is: a text of a million paragraphs is a million of them.
    r.take_items(super::cost::TEXT_PARAGRAPH)?;
    st.paras.push(Para { row, pieces: run.pieces, odf: ParaOdf { style, name: style_name, row, own_row, characters, spans, element, levels } });
    Ok(())
}

/// The lists round the paragraph about to be made, outermost first: each list and item **it opens** is held, and the others are `None`. Each element is then taken: the next
/// paragraph in it continues it. Charged before it is made.
fn take_levels(r: &mut Reader, st: &mut State<'_>) -> Result<Vec<OdfListLevel>, OdgError> {
    if st.levels.is_empty() {
        return Ok(Vec::new());
    }
    r.take_items(st.levels.len().saturating_mul(super::cost::TEXT_LEVEL))?;
    let mut out = Vec::with_capacity(st.levels.len());
    for open in &mut st.levels {
        out.push(OdfListLevel {
            list: std::mem::take(&mut open.list_fresh).then(|| Arc::clone(&open.list)),
            item: if std::mem::take(&mut open.item_fresh) { open.item.clone() } else { None },
        });
    }
    Ok(out)
}

/// The stamp of a paragraph's characters and of where its spans lie, as the splice writer's paragraph plan tells an edited paragraph from one it did not touch.
fn characters_stamp(pieces: &[Piece], spans: &[SpanOdf], count: usize) -> diacraft_doc::Stamp {
    let mut digest = diacraft_doc::Digest::new("characters");
    digest.bytes(&u64::try_from(count).unwrap_or(u64::MAX).to_le_bytes());
    for piece in pieces {
        match piece {
            Piece::Chars { text, .. } => {
                digest.text(text);
            }
            Piece::Field { shown, .. } => {
                digest.bytes(&[1]).text(shown);
            }
            Piece::Raw(_) => {
                digest.bytes(&[2]);
            }
        }
    }
    for span in spans {
        digest.bytes(&u64::try_from(span.from).unwrap_or(u64::MAX).to_le_bytes()).bytes(&u64::try_from(span.to).unwrap_or(u64::MAX).to_le_bytes());
    }
    digest.finish()
}

/// Where a paragraph's white space stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    /// Nothing written yet: white space here is leading, and goes.
    Start,
    /// A collapsed space was just written: more white space adds nothing.
    Collapsed,
    /// Something other than a collapsed space was written last.
    Normal,
}

/// The pieces of a paragraph as they are read, and its white space state.
struct Run {
    pieces: Vec<Piece>,
    space: Space,
    /// Where the last collapsed space was written (piece, byte), while it is the paragraph's last character, so that the end takes it off.
    trailing: Option<(usize, usize)>,
    /// The characters the paragraph holds so far.
    count: usize,
    /// The spans read so far, by where they lie in the characters.
    spans: Vec<SpanOdf>,
    /// How many elements of the paragraph's text have been opened, for [`SpanOdf::opened`].
    opened: usize,
}

impl Run {
    /// Appends `text` on `row`, and says where it starts (piece, byte).
    fn push(&mut self, row: u32, text: &str) -> (usize, usize) {
        self.count = self.count.saturating_add(text.chars().count());
        if let Some(Piece::Chars { text: last, row: last_row }) = self.pieces.last_mut()
            && *last_row == row
        {
            let at = last.len();
            last.push_str(text);
            return (self.pieces.len() - 1, at);
        }
        self.pieces.push(Piece::Chars { text: text.to_owned(), row });
        (self.pieces.len() - 1, 0)
    }

    /// Text as the file holds it, white space collapsed: a run of white space characters is one space, none at the start.
    fn collapsible(&mut self, row: u32, text: &str) {
        let mut word = String::new();
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                if self.space == Space::Normal {
                    if !word.is_empty() {
                        self.push(row, &word);
                        word.clear();
                    }
                    self.trailing = Some(self.push(row, " "));
                    self.space = Space::Collapsed;
                }
            } else {
                word.push(c);
                self.space = Space::Normal;
                self.trailing = None;
            }
        }
        if !word.is_empty() {
            self.push(row, &word);
        }
    }

    /// Characters that are not collapsed: `text:s`, `text:tab`, `text:line-break`, a field's text.
    fn literal(&mut self, row: u32, text: &str) {
        self.push(row, text);
        self.space = Space::Normal;
        self.trailing = None;
    }
}

/// The row the own text of a content element (a paragraph or a span) is on, once it has some, and the element, whose own text says the script type the run's language is read through.
struct RowOf<'t> {
    ix: Option<u32>,
    el: &'t El,
}

impl<'t> RowOf<'t> {
    fn of(el: &'t El) -> RowOf<'t> {
        RowOf { ix: None, el }
    }
}

/// The text nodes an element holds itself, in order: the characters of its own run. What a span or a field inside it holds is the run of its own.
fn own_text(el: &El) -> impl Iterator<Item = &str> {
    el.children.iter().filter_map(|node| if let Node::Text(t) | Node::CData(t) = node { Some(t.as_str()) } else { None })
}

/// Whether the chain `layers` states a language in any of the three triplets: only then is the script type of a run's letters worked out.
fn states_language(layers: &[(Rc<Resolved>, Serves)]) -> bool {
    layers.iter().any(|(resolved, _)| resolved.has_language())
}

/// The row of a run's set of resolved character cells, numbered when the run first writes something (in order of first use). **A run's language is read from the triplet of the script
/// type most of its letters belong to**, so the letters of the element's own text are counted when the chain states a language.
fn run_row(r: &mut Reader, cx: &mut Cx<'_>, st: &mut State<'_>, layers: &[(Rc<Resolved>, Serves)], row: &mut RowOf<'_>) -> Result<u32, OdgError> {
    if let Some(ix) = row.ix {
        return Ok(ix);
    }
    let slot = if states_language(layers) { props::script_type_of(r, own_text(row.el)) } else { FontSlot::Latin };
    let full = full_cells(r, cx, layers, st.size, slot)?.character;
    let base = shape_base(r, cx, st)?;
    let made = st.characters.row(r, &base.character, full)?;
    row.ix = Some(made);
    Ok(made)
}

/// What the shape's chain resolves, worked out once for the shape's text: the cells row 0 is held against.
fn shape_base(r: &mut Reader, cx: &mut Cx<'_>, st: &mut State<'_>) -> Result<Rc<Base>, OdgError> {
    if let Some(base) = &st.base {
        return Ok(Rc::clone(base));
    }
    let out = full_cells(r, cx, st.shape_layers, st.size, FontSlot::Latin)?;
    let base = Rc::new(Base { paragraph: out.paragraph, character: out.character });
    st.base = Some(Rc::clone(&base));
    Ok(base)
}

/// The content of a paragraph or a span: text, spans, white space elements and fields. `layers` is the chain a span's properties read
/// through.
#[allow(clippy::too_many_arguments)]
fn content<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    el: &'t El,
    st: &mut State<'_>,
    layers: &[(Rc<Resolved>, Serves)],
    run: &mut Run,
    depth: usize,
) -> Result<Option<u32>, OdgError> {
    let mut row = RowOf::of(el);
    for node in &el.children {
        match node {
            Node::Text(t) | Node::CData(t) => {
                r.text_units(1);
                r.take_text(t.len())?;
                let ix = run_row(r, cx, st, layers, &mut row)?;
                run.collapsible(ix, t);
            }
            Node::El(child) => {
                r.saw_element();
                r.text_units(1);
                let mark = names.enter(child);
                element(r, names, cx, child, st, layers, run, &mut row, depth)?;
                names.leave(mark);
            }
            Node::Comment(_) | Node::Pi { .. } => {}
        }
    }
    Ok(row.ix)
}

/// The chain a run's properties read through.
type RunCells<'a> = &'a [(Rc<Resolved>, Serves)];

/// One element inside a paragraph. `run_cells` is the chain of the run it is in.
#[allow(clippy::too_many_arguments)]
fn element<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    child: &'t El,
    st: &mut State<'_>,
    run_cells: RunCells<'_>,
    run: &mut Run,
    row: &mut RowOf<'t>,
    depth: usize,
) -> Result<(), OdgError> {
    let layers = run_cells;
    st.holds_object |= super::shapes::frame_holds_object(names, child);
    let attrs = names.attrs(child);
    r.saw_attrs(attrs.len());
    match names.element(child) {
        (Ns::Text, "s") => {
            let count = attrs.peek(Ns::Text, "c").and_then(units::integer).unwrap_or(1).clamp(1, MOST_SPACES);
            let count = usize::try_from(count).unwrap_or(1);
            r.text_units(count);
            r.take_text(count)?;
            let ix = run_row(r, cx, st, layers, row)?;
            run.literal(ix, &" ".repeat(count));
        }
        (Ns::Text, "tab") => {
            r.take_text(1)?;
            let ix = run_row(r, cx, st, layers, row)?;
            run.literal(ix, "\t");
        }
        (Ns::Text, "line-break") => {
            r.take_text('\u{2028}'.len_utf8())?;
            let ix = run_row(r, cx, st, layers, row)?;
            run.literal(ix, "\u{2028}");
        }
        (Ns::Text, "span") => {
            if depth >= MOST_SPAN_DEPTH {
                r.keep("text_element");
                return Ok(());
            }
            let style = attrs.peek(Ns::Text, "style-name").and_then(|name| cx.set.find(r, Family::Text, name, cx.file));
            let name = super::held::named(r, cx.shared, &attrs, Ns::Text, "style-name")?;
            // The span's own conversion counts what its style keeps without a cell, once.
            own_cells(r, cx, style, layers, st.size)?;
            let inner = span_layers(r, cx, style, layers)?;
            let from = run.count;
            let opened = run.opened;
            run.opened = run.opened.saturating_add(1);
            let span_row = content(r, names, cx, child, st, &inner, run, depth + 1)?;
            r.take_items(super::cost::TEXT_RUN)?;
            // A span that has an attribute besides its style name holds its element too.
            let skip = style_attr(names, child);
            let element = if has_other_attrs(child, skip) { Some(shell(r, names, child, skip)?) } else { None };
            run.spans.push(SpanOdf { style, name, from, to: run.count, row: span_row, span: true, element, opened });
        }
        (Ns::Text, kind @ ("page-number" | "page-count" | "file-name" | "date" | "time")) => {
            let fixed = attrs.peek(Ns::Text, "fixed").and_then(units::boolean).unwrap_or(false);
            // The shown text is held twice, in the text and as the field's cached value: charged from its nodes' lengths before it is made.
            let length: usize = child.children.iter().map(|n| if let Node::Text(t) | Node::CData(t) = n { t.len() } else { 0 }).sum();
            r.take_text(length.saturating_mul(2))?;
            let shown = child.text();
            let formula = match kind {
                "page-number" => Some("PAGENUMBER()"),
                "page-count" => Some("PAGECOUNT()"),
                "file-name" => Some("FILENAME()"),
                _ if !fixed => Some("NOW()"),
                _ => None,
            };
            let ix = run_row(r, cx, st, layers, row)?;
            let from = run.count;
            let opened = run.opened;
            run.opened = run.opened.saturating_add(1);
            match formula {
                Some(formula) => {
                    r.take_cells(1)?;
                    r.take_items(super::cost::ROW)?;
                    if attrs.peek(Ns::Style, "data-style-name").is_some() {
                        r.keep("field_format");
                    }
                    let field = u32::try_from(st.fields.len()).unwrap_or(u32::MAX);
                    st.fields.push(r.forms.text_cell(formula, &shown));
                    run.count = run.count.saturating_add(shown.chars().count());
                    run.pieces.push(Piece::Field { ix: field, shown, row: ix });
                    run.space = Space::Normal;
                    run.trailing = None;
                }
                None => {
                    r.keep("field");
                    run.literal(ix, &shown);
                }
            }
            // The field's element, over the characters it shows.
            r.take_items(super::cost::TEXT_RUN)?;
            let element = Some(shell(r, names, child, None)?);
            run.spans.push(SpanOdf { style: None, name: None, from, to: run.count, row: None, span: false, element, opened });
        }
        (
            Ns::Text,
            "soft-page-break" | "bookmark" | "bookmark-start" | "bookmark-end" | "reference-mark" | "reference-mark-start" | "reference-mark-end",
        ) => {
            r.keep("text_mark");
            // The mark is held whole where it stands, a zero-width part of the text (`TextPart::OdfRaw`), charged before it is made.
            r.take_items(super::shapes::element_cost(child).saturating_add(1))?;
            run.pieces.push(Piece::Raw(RawNode::Element(super::shapes::raw_element_in(r, names, child)?)));
        }
        (Ns::Draw | Ns::Dr3d, _) => {
            // **A drawing object in a paragraph**: a frame anchored as a character (a picture, a text box, an object), a shape, a group or a hyperlink round one. It is an
            // object the text holds at a place, and none of its characters (its own paragraphs, a picture's text) are the paragraph's: it is held whole where it stands, a zero-width part of the text
            // as a mark is (`TextPart::OdfRaw`), charged before it is made. Read as a run of the characters it holds, it came apart on a save from the model: the frame was written with nothing in
            // it and its picture and its paragraph after it, inside the paragraph.
            r.keep("text_object");
            r.take_items(super::shapes::element_cost(child).saturating_add(1))?;
            run.pieces.push(Piece::Raw(RawNode::Element(super::shapes::raw_element_in(r, names, child)?)));
            // A frame of an object anywhere in it (a text box in the frame, holding one) marks the text, as the walk into it did.
            st.holds_object |= holds_object_inside(names, child, depth);
        }
        _ => {
            // A hyperlink (`text:a`), another field or an element of another vocabulary: its text is read, the element is kept.
            r.keep("text_element");
            let from = run.count;
            let opened = run.opened;
            run.opened = run.opened.saturating_add(1);
            if depth < MOST_SPAN_DEPTH {
                content(r, names, cx, child, st, layers, run, depth + 1)?;
            }
            // Its element, over the characters it shows, with the elements inside it as runs of their own.
            r.take_items(super::cost::TEXT_RUN)?;
            let element = Some(shell(r, names, child, None)?);
            run.spans.push(SpanOdf { style: None, name: None, from, to: run.count, row: None, span: false, element, opened });
        }
    }
    Ok(())
}

/// The most spaces one `text:s` stands for: `text:c` is a count the file gives, so it is bounded before a string is made of it.
pub(crate) const MOST_SPACES: i64 = 1 << 16;

/// Whether an element a paragraph holds whole has the frame of an object among its descendants: looked for down its tree, each element in its own namespace scope, no deeper than
/// [`MOST_SPAN_DEPTH`] below the paragraph's spans (the tree's own depth cap is the bound, and this keeps the recursion to it). The tree is the reader's, charged when it was held.
fn holds_object_inside<'t>(names: &mut Names<'t>, el: &'t El, depth: usize) -> bool {
    if depth >= MOST_SPAN_DEPTH {
        return false;
    }
    el.elements().any(|child| {
        let mark = names.enter(child);
        let found = super::shapes::frame_holds_object(names, child) || holds_object_inside(names, child, depth + 1);
        names.leave(mark);
        found
    })
}

/// The deepest nesting of spans read; the tree's own depth cap is the bound, and this keeps the recursion to it.
const MOST_SPAN_DEPTH: usize = 256;

/// The text and rows of everything read.
fn finish(r: &mut Reader, st: State<'_>) -> Result<TextOut, OdgError> {
    let State { paragraphs, characters, fields, paras, holds_object, .. } = st;
    let mut parts: Vec<TextPart> = Vec::new();
    // A paragraph that holds nothing but marks is as empty as one that holds nothing: the marks are not held when the text is empty. (A drawing object a paragraph holds alone is no
    // exception: the shape keeps the empty text's paragraph whole, the object in it, as its own markup, `shapes::keep_empty_text`.)
    let empty_text = paras.iter().all(|p| p.pieces.iter().all(|piece| matches!(piece, Piece::Raw(_)))) && paras.len() <= 1;
    if empty_text {
        return Ok(TextOut { holds_object, ..TextOut::default() });
    }
    let first_empty = |n: &Numbering| n.rows.first().is_none_or(Vec::is_empty);
    let mut prev_pp: Option<u32> = first_empty(&paragraphs).then_some(0);
    let mut prev_cp: Option<u32> = first_empty(&characters).then_some(0);
    let mut chars = String::new();
    // Each run of characters becomes one part, its buffer let go: the pieces are moved in and dropped as they go, so the text is held at
    // most twice while it is put together.
    let flush = |chars: &mut String, parts: &mut Vec<TextPart>| {
        if !chars.is_empty() {
            parts.push(TextPart::Chars(std::sync::Arc::from(std::mem::take(chars))));
        }
    };
    let mut odf: Vec<ParaOdf> = Vec::with_capacity(paras.len());
    for (i, para) in paras.into_iter().enumerate() {
        odf.push(para.odf);
        if i > 0 {
            chars.push('\n');
        }
        let first_row = para.pieces.iter().find_map(|p| match p {
            Piece::Chars { row, .. } | Piece::Field { row, .. } => Some(*row),
            Piece::Raw(_) => None,
        });
        let opens_cp = first_row.filter(|row| prev_cp != Some(*row));
        let opens_pp = (prev_pp != Some(para.row)).then_some(para.row);
        if opens_cp.is_some() || opens_pp.is_some() {
            flush(&mut chars, &mut parts);
        }
        if let Some(row) = opens_cp {
            parts.push(TextPart::Cp(row));
            prev_cp = Some(row);
        }
        if let Some(row) = opens_pp {
            parts.push(TextPart::Pp(row));
            prev_pp = Some(row);
        }
        for piece in para.pieces {
            // A zero-width mark: the run in force is not changed, and the characters before it are a part of their own.
            if let Piece::Raw(node) = piece {
                flush(&mut chars, &mut parts);
                parts.push(TextPart::OdfRaw(Box::new(node)));
                continue;
            }
            let row = match &piece {
                Piece::Chars { row, .. } | Piece::Field { row, .. } => *row,
                Piece::Raw(_) => continue,
            };
            if prev_cp != Some(row) {
                flush(&mut chars, &mut parts);
                parts.push(TextPart::Cp(row));
                prev_cp = Some(row);
            }
            match piece {
                Piece::Chars { text, .. } if chars.is_empty() => chars = text,
                Piece::Chars { text, .. } => chars.push_str(&text),
                Piece::Field { ix, shown, .. } => {
                    flush(&mut chars, &mut parts);
                    parts.push(TextPart::Fld { ix, text: std::sync::Arc::from(shown) });
                }
                Piece::Raw(_) => {}
            }
        }
    }
    flush(&mut chars, &mut parts);
    // Each part its place in the list and, for a run of characters, its string ([`super::cost::TEXT_PART`]); the characters were charged as they were read.
    r.take_items(parts.len().saturating_mul(super::cost::TEXT_PART))?;
    Ok(TextOut {
        text: Some(Text { parts, ..Default::default() }),
        paragraphs: paragraphs.rows,
        characters: characters.rows,
        fields,
        holds_object,
        odf,
    })
}

#[cfg(test)]
mod tests {
    use diacraft_doc::Cell;
    use diacraft_doc::Value;

    use super::super::Reader;
    use super::SHORT_TEXT;
    use super::key_of;
    use crate::limits::Limits;

    /// A cell holding `text` as a value of its own allocation.
    fn valued(text: &str) -> Cell {
        Cell::constant(Value::from_text(text))
    }

    /// A cell holding the shared value of `text`, as `StyleTexts::value` makes one.
    fn shared(r: &mut Reader, text: &str) -> Cell {
        Cell::constant(r.shared_value(text).unwrap())
    }

    #[test]
    fn a_value_of_the_short_limit_is_keyed_by_what_it_says_and_a_longer_one_by_where_it_is() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let of = |len: usize, c: char| c.to_string().repeat(len);
        // Two copies of a text of the limit make one key, and the key holds the text.
        let (a, b) = (valued(&of(SHORT_TEXT, 'a')), valued(&of(SHORT_TEXT, 'a')));
        assert_eq!(key_of(&r, &[("Font", a)]), key_of(&r, &[("Font", b)]));
        assert!(key_of(&r, &[("Font", valued(&of(SHORT_TEXT, 'a')))]).contains(&of(SHORT_TEXT, 'a')));
        // A text one byte longer is a shared value: equal texts are one value and one key, and a key holds an address and not the text.
        let (a, b) = (shared(&mut r, &of(SHORT_TEXT + 1, 'a')), shared(&mut r, &of(SHORT_TEXT + 1, 'a')));
        assert_eq!(key_of(&r, &[("Font", a)]), key_of(&r, &[("Font", b)]));
        // Two texts of one length that differ are two values and two keys.
        let (a, b) = (shared(&mut r, &of(SHORT_TEXT + 1, 'a')), shared(&mut r, &of(SHORT_TEXT + 1, 'b')));
        assert_ne!(key_of(&r, &[("Font", a)]), key_of(&r, &[("Font", b.clone())]));
        assert!(!key_of(&r, &[("Font", b)]).contains(&of(SHORT_TEXT, 'b')));
        // One value shown twice is one key, whatever its length.
        let long = shared(&mut r, &of(1_000, 'c'));
        assert_eq!(key_of(&r, &[("Font", long.clone())]), key_of(&r, &[("Font", long)]));
    }

    /// A long text that is not a shared value is refused where the key would name it by its address (debug builds): the address of a text
    /// that is freed with the cell that held it could be taken by another, and two runs would then share a row.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a shared value")]
    fn a_long_text_that_is_not_a_shared_value_is_refused_by_a_debug_build() {
        let limits = Limits::default();
        let r = Reader::new(&limits);
        key_of(&r, &[("Font", valued(&"a".repeat(SHORT_TEXT + 1)))]);
    }

    /// A text of the limit, or shorter, is keyed by its content and needs no registry.
    #[test]
    fn a_short_text_needs_no_shared_value() {
        let limits = Limits::default();
        let r = Reader::new(&limits);
        assert!(key_of(&r, &[("Font", valued(&"a".repeat(SHORT_TEXT)))]).contains("Font="));
    }
}
