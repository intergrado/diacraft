//! The root style and the style sheets.
//!
//! * **The root** (`ID` 0, `No Style`) starts from the root style DiaCraft writes for a new drawing (`Document::new_blank`, the engine's
//!   `file.new`), never from another producer's defaults, and each property the default `graphic` and `paragraph` styles set replaces its
//!   cell. Where both set one, the paragraph style's wins for text, as it is the family a paragraph asks first.
//! * **A style sheet** is made of each common style of family `graphic` or `paragraph`, in document order, with `ID`s from 1. Its
//!   `NameU` is the `style:name` exactly as written, encoded characters and all, and its `Name` the `style:display-name`, or the name.
//!   Its parent is the target of its three links, the root when it has none. It holds the cells its own properties set, nothing more:
//!   a paragraph style only text cells.

use std::rc::Rc;
use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Links, RowKey, Sheet, Style};

use super::Reader;
use super::held;
use super::props::{self, Base, Out, Serves, View};
use super::styles::{Family, Kind, P, Resolved, Resolver, StyleSet, Tab};
use super::texts::StyleTexts;
use crate::error::{LimitKind, OdgError};

/// The root and the style sheets, by `ID` (the root first), and what conversions need of them.
#[derive(Default)]
pub(crate) struct Sheets {
    styles: Vec<Style>,
    /// The root's values for what no chain says.
    pub base: Option<Base>,
    /// The default styles as the last layers of every view: the paragraph default, then the graphic default.
    pub defaults: Vec<(Rc<Resolved>, Serves)>,
}

impl Sheets {
    /// The styles, the root first, for the document.
    pub(crate) fn into_styles(self) -> Vec<Arc<Style>> {
        self.styles.into_iter().map(Arc::new).collect()
    }

    /// The most links up the style sheets a lookup follows: the parent chain the resolver follows is at most 64 deep, so a longer one is not
    /// walked for a shape.
    pub(crate) const MOST_LINKS: usize = 66;

    /// The number in the cell `name` of style sheet `id`, or of the nearest sheet up its links that holds one (the line link when the cell is a
    /// line cell, else the fill link: the walk follows the one family, up to the root); `None` when none does.
    pub(crate) fn number(&self, id: u32, name: &str) -> Option<f64> {
        let mut at = id;
        // Links go up and end at the root, whose links name itself or nothing: a bound keeps a malformed chain from looping.
        for _ in 0..Sheets::MOST_LINKS {
            let style = self.styles.get(at as usize).filter(|s| s.id == at)?;
            if let diacraft_doc::Lookup::Found(cell) = style.sheet.lookup(&CellAddr::direct(name)) {
                return cell.value.as_ref().and_then(diacraft_doc::Value::number);
            }
            let next = if name == "FillPattern" { style.sheet.links.fill } else { style.sheet.links.line }?;
            if next == at {
                return None;
            }
            at = next;
        }
        None
    }

    /// The root's values.
    pub(crate) fn base(&self) -> Base {
        self.base.unwrap_or(Base { size: 0.0, weight: 0.0, cap: 0, style_bits: 0 })
    }
}

/// The root's cell `name` (and of a row of `section`) as a number.
fn root_number(root: &Style, section: Option<&str>, name: &str) -> Option<f64> {
    let addr = match section {
        None => CellAddr::direct(name),
        Some(section) => CellAddr::in_row(section, None, RowKey::Ix(0), name),
    };
    match root.sheet.lookup(&addr) {
        diacraft_doc::Lookup::Found(cell) => cell.value.as_ref().and_then(diacraft_doc::Value::number),
        _ => None,
    }
}

/// Puts the cells of a conversion into `sheet`: direct cells under it, the `Paragraph`, `Character` and `Tabs` cells in row 0 of their
/// sections, and the gradient's stops as the rows of `FillGradient` (`GradientStopColor` and `GradientStopPosition`, the cells the renderer and a VSDX file
/// read), with the rows past them deleted up to `stops_through`. Each cell is taken from the document's total first.
pub(crate) fn apply(r: &mut Reader, sheet: &mut Sheet, out: Out) -> Result<(), OdgError> {
    r.take_cells(out.cells())?;
    let stop_rows = out.stops.as_ref().map_or(0, Vec::len);
    let deleted = if stop_rows == 0 { 0 } else { out.stops_through.saturating_sub(stop_rows) };
    // The rows these cells go in: a paragraph's, a run's, one for each stop and each deleted row past them, and one for the tab stops.
    let rows =
        usize::from(!out.paragraph.is_empty()) + usize::from(!out.character.is_empty()) + stop_rows + deleted + usize::from(out.tabs.is_some());
    r.take_items(rows.saturating_mul(super::cost::ROW))?;
    let model = |e: diacraft_doc::DocError| OdgError::Model(e);
    for (name, cell) in out.direct {
        sheet.set_cell(&CellAddr::direct(name), cell).map_err(model)?;
    }
    for (section, list) in [("Paragraph", out.paragraph), ("Character", out.character)] {
        for (name, cell) in list {
            sheet.set_cell(&CellAddr::in_row(section, None, RowKey::Ix(0), name), cell).map_err(model)?;
        }
    }
    if let Some(stops) = out.stops {
        for (i, (colour, position)) in stops.into_iter().enumerate() {
            let ix = RowKey::Ix(u32::try_from(i).unwrap_or(u32::MAX));
            sheet.set_cell(&CellAddr::in_row("FillGradient", None, ix.clone(), "GradientStopColor"), colour).map_err(model)?;
            sheet.set_cell(&CellAddr::in_row("FillGradient", None, ix, "GradientStopPosition"), position).map_err(model)?;
        }
        let key = diacraft_doc::SectionKey::new("FillGradient", None);
        for i in stop_rows..stop_rows.saturating_add(deleted) {
            sheet.row_mut(&key, &RowKey::Ix(u32::try_from(i).unwrap_or(u32::MAX))).map_err(model)?.del = true;
        }
    }
    if let Some(tabs) = out.tabs {
        for (name, cell) in props::tab_cells(&tabs) {
            sheet.set_cell(&CellAddr::in_row("Tabs", None, RowKey::Ix(0), &name), cell).map_err(model)?;
        }
    }
    Ok(())
}

/// The conversion of a style's properties: which cells, given what it owns and the view it reads through. `text_only` leaves out the
/// fill, line and shadow cells (a paragraph style's sheet). Tab stops are the caller's, which knows whose they are.
pub(crate) fn convert<'a>(
    r: &mut Reader,
    texts: &mut StyleTexts<'a>,
    view: &View<'a>,
    own: props::Own<'_>,
    size: Option<f64>,
    base: &Base,
    text_only: bool,
) -> Result<Out, OdgError> {
    let mut out = Out::default();
    if !text_only {
        props::line_fill(r, view, own, base, &mut out);
    }
    props::text_block(r, view, own, &mut out);
    props::paragraph(r, view, own, &mut out);
    props::character(r, texts, view, own, size, base, &mut out)?;
    Ok(out)
}

/// The font size a view gives text, in inches: the nearest style that sets `fo:font-size`, with a percentage of its parent's;
/// past the chain, `base`. `None` when nothing in the view sets it.
pub(crate) fn view_size(r: &mut Reader, set: &StyleSet, res: &mut Resolver, view: &View<'_>, base: f64) -> Option<f64> {
    let hit = view.hit(r, Kind::Text, P::FontSize)?;
    Some(res.font_size(r, set, hit.found.style, base))
}

/// Builds the root and the style sheets (step 3 of the read).
pub(crate) fn build(r: &mut Reader, set: &mut StyleSet, res: &mut Resolver) -> Result<Sheets, OdgError> {
    res.prepare(r, set);
    let blank = Document::new_blank();
    let mut root: Style = blank.styles.get(0).map(|s| Style::clone(s)).unwrap_or_default();
    let mut base = Base {
        size: root_number(&root, Some("Character"), "Size").unwrap_or(0.0),
        weight: root_number(&root, None, "LineWeight").unwrap_or(0.0),
        cap: root_number(&root, None, "LineCap").map_or(0, |c| c.clamp(0.0, 2.0) as u8),
        style_bits: root_number(&root, Some("Character"), "Style").map_or(0, |s| s.clamp(0.0, 255.0) as u32),
    };
    let mut defaults = Vec::new();
    if let Some(at) = set.default_of(Family::Paragraph) {
        defaults.push((res.resolved(r, set, at)?, Serves::ParagraphDefault));
    }
    if let Some(at) = set.default_of(Family::Graphic) {
        defaults.push((res.resolved(r, set, at)?, Serves::GraphicDefault));
    }
    // The root: every property the default styles set.
    {
        let set: &StyleSet = set;
        let mut texts = StyleTexts::of(set);
        let view = View { set, layers: defaults.clone() };
        let own = |kind: Kind, p: P| defaults.iter().any(|(d, _)| d.get(kind, p).is_some());
        let size = view_size(r, set, res, &view, base.size);
        let mut out = convert(r, &mut texts, &view, &own, size, &base, false)?;
        // Nothing is under the root to delete rows of.
        out.stops_through = 0;
        if view.tabs().is_some() {
            out.tabs = view.tabs().cloned();
        }
        let mut sheet = Sheet::clone(&root.sheet);
        apply(r, &mut sheet, out)?;
        root.sheet = Arc::new(sheet);
        if let Some(size) = size {
            base.size = size;
        }
    }
    let mut sheets = Sheets { styles: vec![root], base: Some(base), defaults };
    // The style sheets: `ID`s first, in document order, so that a parent later in the file is linked.
    let order = set.sheet_styles();
    if order.len() >= r.limits.max_styles.min(diacraft_doc::MAX_STYLES) {
        return Err(OdgError::Limit { kind: LimitKind::Styles, max: r.limits.max_styles.min(diacraft_doc::MAX_STYLES) as u64 });
    }
    for (i, &at) in order.iter().enumerate() {
        set.set_sheet(at, u32::try_from(i + 1).unwrap_or(u32::MAX));
    }
    // The ids are given, so the set is only read from here on: its texts are cached for as long as it is borrowed.
    let set: &StyleSet = set;
    let mut texts = StyleTexts::of(set);
    for &at in &order {
        let Some(style) = set.get(at) else { continue };
        let (id, family) = (style.sheet.unwrap_or(0), style.family);
        r.take_bytes(style.name.len() + style.display.as_ref().map_or(0, String::len))?;
        let name_u: Arc<str> = Arc::from(style.name.as_str());
        let name: Arc<str> = style.display.as_deref().map_or_else(|| Arc::clone(&name_u), Arc::from);
        let parent = res.parent_of(at).and_then(|p| res.link(r, set, p)).and_then(|p| set.get(p)).and_then(|p| p.sheet).unwrap_or(0);
        let resolved = res.resolved(r, set, at)?;
        let mut layers = vec![(Rc::clone(&resolved), Serves::All)];
        layers.extend(sheets.defaults.iter().cloned());
        let view = View { set, layers };
        let own = |kind: Kind, p: P| set.get(at).is_some_and(|s| s.own(kind, p).is_some());
        let size =
            if set.get(at).is_some_and(|s| s.own(Kind::Text, P::FontSize).is_some()) { Some(res.font_size(r, set, at, base.size)) } else { None };
        let mut out = convert(r, &mut texts, &view, &own, size, &base, family == Family::Paragraph)?;
        out.tabs = set.get(at).and_then(|s| s.tabs.clone());
        let mut sheet = Sheet { links: Links { line: Some(parent), fill: Some(parent), text: Some(parent) }, ..Sheet::default() };
        apply(r, &mut sheet, out)?;
        r.take_items(1)?;
        let mut style_sheet = Style { id, name: Some(name), name_u: Some(name_u), sheet: Arc::new(sheet), ..Style::default() };
        // What the style holds that no cell reads, charged where the style was read; the lists behind it are shared. And the values it gives that its cells only
        // approximate, which carry the stamps of the cells just made.
        if let Some(odf) = set.get(at).and_then(|s| s.odf.as_deref()) {
            let mut data = odf.clone();
            held::for_sheet(r, set, at, &style_sheet.sheet, &mut data)?;
            hold_spelling(r, style, &style_sheet.sheet, &mut data)?;
            style_sheet.ext.make_mut().odf = data;
        }
        sheets.styles.push(style_sheet);
    }
    Ok(sheets)
}

/// **Holds the spelling of the `Style` group of a style sheet when the cells would write another**: the cell `Style` is one number whose bits say
/// the weight, the style, the small caps and the complex script's two, and a writer that says them from the cell writes all five attributes, where a file stated the weight alone, or none
/// (a style that only underlines). The attributes the file spelled, as written, are a held value of the group `Style` of the sheet's own slot, stamped by the `Style` cell as read (bit 4, the
/// underline's, left out): the writer says the held spelling while the stamp holds, and the cells' five when an edit changed the group. Nothing is held when the cells write what the file
/// spelled, which is what a style that spells all five in the forms the writer uses is. Charged before it is made.
fn hold_spelling(r: &mut Reader, style: &super::styles::OdfStyle, sheet: &Sheet, odf: &mut diacraft_doc::OdfStyle) -> Result<(), OdgError> {
    let Some(row) = sheet.section(&diacraft_doc::SectionKey::new("Character", None)).and_then(|s| s.row(&RowKey::Ix(0))) else { return Ok(()) };
    if !row.cells.contains_key("Style") {
        return Ok(());
    }
    // What the cells write of the group, as the writer of a style sheet says it.
    r.style_step();
    let Some(written) = crate::write::style_group_of_cells(sheet) else { return Ok(()) };
    let same = written.len() == style.spelled.len()
        && style.spelled.iter().all(|s| written.iter().any(|(ns, local, value)| *ns == s.ns && *local == s.local && *value == s.value));
    if same {
        return Ok(());
    }
    let bytes = style.spelled.iter().map(|s| s.name.len().saturating_add(s.value.len())).fold(0usize, usize::saturating_add);
    r.take_items(super::cost::HELD.saturating_add(style.spelled.len().saturating_mul(2)))?;
    r.take_bytes(bytes)?;
    odf.text_held.push(diacraft_doc::HeldStyleValue {
        group: diacraft_doc::TextGroup::Style,
        stamp: Some(diacraft_doc::stamp_text_group(row, diacraft_doc::TextGroup::Style)),
        attrs: diacraft_doc::RawAttrs::from_pairs(style.spelled.iter().map(|s| (s.name.as_str(), s.value.as_str()))),
        kept: diacraft_doc::Passthrough::new(),
    });
    Ok(())
}

/// Whether two conversions give the same tab stops: the same list. Styles that hold equal stops hold one list (`Reader::shared_tabs`, made
/// when a style is read), so equal lists are the same list and the answer is an identity: a shape-style pair reads no stop, however many
/// stops the styles hold (the audit: a list of 2,048 stops was walked for each pair of two styles that hold equal ones).
fn same_tabs(a: Option<&Rc<[Tab]>>, b: Option<&Rc<[Tab]>>) -> bool {
    // Two conversions with no stops give none, whether or not they are called the same.
    matches!((a, b), (Some(a), Some(b)) if Rc::ptr_eq(a, b))
}

/// The cells of `list` that `other` does not hold with the same name and the same value (see [`props::same_cell`]).
///
/// The search visits at most as many entries of `other` as it holds, for each cell of `list`, and both lists are the cells one conversion
/// gave: at most one for each place in `props.rs` that pushes one (about 30 in a list), whatever the style states, so a crafted style cannot
/// make them long. The entries visited are counted ([`crate::ReadWork::cells_scanned`]).
fn keep(r: &mut Reader, list: Vec<(&'static str, Cell)>, other: &[(&'static str, Cell)]) -> Vec<(&'static str, Cell)> {
    list.into_iter()
        .filter(|(name, cell)| {
            let mut visited = 0;
            let same = other.iter().any(|(n, c)| {
                visited += 1;
                n == name && props::same_cell(r, cell, c)
            });
            r.cells_scanned(visited);
            !same
        })
        .collect()
}

/// The cells of `out` that differ from those of `against` (by name and place), for a shape's local cells: a cell equal to what its links
/// give is not written.
pub(crate) fn differing(r: &mut Reader, out: Out, against: &Out) -> Out {
    let direct = keep(r, out.direct, &against.direct);
    let paragraph = keep(r, out.paragraph, &against.paragraph);
    let character = keep(r, out.character, &against.character);
    // The gradient's stops: ten at most (`gradient.rs`), a colour and a place each.
    let stops = match (&out.stops, &against.stops) {
        (Some(a), Some(b))
            if a.len() == b.len() && a.iter().zip(b).all(|(x, y)| props::same_cell(r, &x.0, &y.0) && props::same_cell(r, &x.1, &y.1)) =>
        {
            None
        }
        _ => out.stops,
    };
    let tabs = if same_tabs(out.tabs.as_ref(), against.tabs.as_ref()) { None } else { out.tabs };
    Out { direct, paragraph, character, stops, stops_through: out.stops_through, tabs, other_ends: Vec::new() }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::Tab;
    use super::same_tabs;
    use crate::read::units::{LenUnit, Length};

    /// A list of two stops, of its own allocation.
    fn list() -> Rc<[Tab]> {
        Rc::from(vec![
            Tab { position: Length { inches: 1.0, unit: LenUnit::In }, alignment: 0 },
            Tab { position: Length { inches: 2.0, unit: LenUnit::In }, alignment: 2 },
        ])
    }

    #[test]
    fn tab_lists_are_the_same_only_when_they_are_the_same_list() {
        let (a, b) = (list(), list());
        assert!(same_tabs(Some(&a), Some(&Rc::clone(&a))));
        // Equal stops in two lists are two lists. The styles that hold them hold one (`Reader::shared_tabs`), so a pair never meets this; a
        // walk of the stops to tell would be the cost of 2,048 of them for each pair, and is not made.
        assert!(!same_tabs(Some(&a), Some(&b)));
        assert!(!same_tabs(Some(&a), None));
        assert!(!same_tabs(None, Some(&a)));
        // No stops on either side give no stops, whether or not they are called the same.
        assert!(!same_tabs(None, None));
    }
}
