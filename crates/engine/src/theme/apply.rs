//! Applying a theme to pages: the part, the page's cells, the shapes that follow it, and the recalculation.
//!
//! **What applying a theme writes**:
//!
//! * **The part.** The theme's part joins the drawing's themes if no part carries its number (`ThemeIndex` is found by `schemeEnum`, never by
//!   a part's place). A part the drawing already holds under that number is used as it is, whoever made it. The writer adds the part to the
//!   package on the next save (`diacraft_vsdx::write`, `themes.rs`).
//! * **The page's cells.** `ThemeIndex`, `ColorSchemeIndex`, `EffectSchemeIndex`, `ConnectorSchemeIndex` and `FontSchemeIndex` of the page sheet
//!   take the theme's number, and `VariationColorIndex` and `VariationStyleIndex` the variant's place (0 to 3). Shapes read them through the
//!   `Theme` style's 65534. "No theme" writes 0 to all seven, which sends every themed cell back to the root style.
//! * **The shapes** ("a new theme rewrites it on the document, its pages and its shapes"). A shape (type `Shape`, to any depth of
//!   groups) with no style of a family, or the root style, follows the `Theme` style for that family, so that a shape DiaCraft drew takes the
//!   theme; a shape with a style of its own keeps it, and a cell a person set on a shape stays set ("standard colours stay put").
//!   A shape that holds its own scheme cell with a plain number (a shape pasted from a drawing with another theme)
//!   takes the new number, unless the matching `Lock...` cell of the Protection section is set (`LockThemeIndex` keeps a new theme from
//!   changing the theme number, and so for colours, effects, connectors and fonts).
//! * **A shape on a locked layer** (a shape on a locked layer is not edited, and the specifications say nothing of a theme and one):
//!   the theme is the drawing's, so the page's cells are written and the shape draws with what they select, but no link and no cell is written into it (each shape by its own
//!   list); the answer counts the shapes passed over (`Applied::locked`).
//! * **The recalculation** (`affected.rs` of `calc`: what `THEMEVAL` reads of the sheet's theme cells is not followed, and a caller whose edit
//!   changes them sets `Changes::whole_page`): the whole page is worked out again. A cell holding the marker `Themed` is not written by a
//!   recalculation (`commit.rs`): the renderer asks the theme each time it draws, so the new theme shows at once. A cell that holds a number
//!   under a theme formula, as files from other applications do, is written with what the theme gives now.
//!
//! **Bounded.** The walk of a page is one iterative pass over at most `MAX_SHAPES_PER_PAGE` shapes, no deeper than the model allows; a page
//! with more than [`MAX_THEMED_SHAPES`] shapes to change is refused whole, with the document as it was.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_calc::{Budget, Changes, Since};
use diacraft_doc::{
    CellAddr, CellEdit, CellWrite, Document, FormulaSlot, Links, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, RawAttrs, Resolver, Shape, ShapeList,
    ShapeRef, ShapeType, SheetRef, Value, WriteOrigin,
};
use diacraft_vsdx::{MAX_THEME_BYTES, MAX_THEMES};

use super::catalogue::CatalogueTheme;
use super::style::{Base, ensure_theme_style, theme_style_id};
use crate::calc_index::commit_changed;
use crate::draw::{Problem, require_page, shape_mut};

/// The most shapes of one page a theme is applied to in one command. A page near the model's cap of a million shapes takes a theme a part at a
/// time, which no command offers; the cost is measured in `tests/it/theme_cost.rs`.
pub const MAX_THEMED_SHAPES: usize = 200_000;

/// The most shapes [`follow_page_theme_refs`] makes follow the theme in one call: a colour of the theme put on a selection (`formatting::MAX_PICK_SHAPES`, the same number).
pub const MAX_FOLLOWED_SHAPES: usize = 10_000;

thread_local! {
    /// What the passes that make shapes follow the page's theme did on this thread, counted so that a test holds a cost without a clock ([`follow_work`]).
    static FOLLOW_WORK: std::cell::Cell<FollowWork> = const { std::cell::Cell::new(FollowWork { page_walks: 0, shapes_planned: 0 }) };
}

/// What the passes that make shapes follow the page's theme did since [`reset_follow_work`], on this thread: the walks of a whole page they made to find a shape by its `ID`, and the shapes
/// they planned an edit for. **A pass for a set of shapes the caller has the places of makes no walk of the page** (`follow_page_theme_refs`); a pass for one shape by its `ID`
/// (`follow_page_theme`, for a shape that a command has just put on the page) makes one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FollowWork {
    pub page_walks: u64,
    pub shapes_planned: u64,
}

/// What the theme-following passes did on this thread since the last [`reset_follow_work`].
pub fn follow_work() -> FollowWork {
    FOLLOW_WORK.with(std::cell::Cell::get)
}

/// Starts the count of [`follow_work`] again.
pub fn reset_follow_work() {
    FOLLOW_WORK.with(|c| c.set(FollowWork::default()));
}

fn note_follow(walks: u64, planned: usize) {
    FOLLOW_WORK.with(|c| {
        let was = c.get();
        c.set(FollowWork {
            page_walks: was.page_walks.saturating_add(walks),
            shapes_planned: was.shapes_planned.saturating_add(u64::try_from(planned).unwrap_or(u64::MAX)),
        });
    });
}

/// The cells of a page, a style or a shape that select a theme, in the order of [`Selection::numbers`]: the theme, the colour scheme, the effect
/// scheme, the connector scheme, the font scheme, the colour variant and the style variant.
pub const SELECTORS: [&str; 7] =
    ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex", "VariationColorIndex", "VariationStyleIndex"];

/// The `Lock...` cell of the Protection section that keeps a new theme from changing each of [`SELECTORS`]. The variants follow the colours and the
/// effects they pick among (our reading: no document says which lock a variant answers to).
const LOCKS: [&str; 7] =
    ["LockThemeIndex", "LockThemeColors", "LockThemeEffects", "LockThemeConnectors", "LockThemeFonts", "LockThemeColors", "LockThemeEffects"];

/// The attributes of a sheet's element that hold its style links: a link written must not leave the text of the old one behind (raw wins on a write).
const LINK_ATTRS: [&str; 3] = ["LineStyle", "FillStyle", "TextStyle"];

/// What a page's sheet selects: the seven cells of [`SELECTORS`], 0 where the page holds none or one that is not a whole number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub theme: u32,
    pub colours: u32,
    pub effects: u32,
    pub connectors: u32,
    pub fonts: u32,
    pub variation_colours: u32,
    pub variation_styles: u32,
}

impl Selection {
    /// The seven numbers, in the order of [`SELECTORS`].
    pub fn numbers(&self) -> [u32; 7] {
        [self.theme, self.colours, self.effects, self.connectors, self.fonts, self.variation_colours, self.variation_styles]
    }

    fn from_numbers(n: [u32; 7]) -> Selection {
        Selection { theme: n[0], colours: n[1], effects: n[2], connectors: n[3], fonts: n[4], variation_colours: n[5], variation_styles: n[6] }
    }

    /// Whether the page selects a theme: its theme number and its colour scheme are above 0.
    pub fn is_themed(&self) -> bool {
        self.theme > 0 && self.colours > 0
    }
}

/// A whole number a cell value reads as, for a selector.
fn whole(value: &Value) -> Option<u32> {
    let n = value.number()?;
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)).then_some(n as u32)
}

/// What page `page` selects; `None` for a page that is not there.
pub fn selection_of(doc: &Document, page: u32) -> Option<Selection> {
    let sheet = &doc.page(page)?.sheet;
    let read = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()).and_then(whole).unwrap_or(0);
    Some(Selection::from_numbers(SELECTORS.map(read)))
}

/// What a command applies.
#[derive(Clone, Copy, Debug)]
pub enum Choice {
    /// No theme: every selector 0.
    None,
    /// A theme of the catalogue and the variant (0 to 3) of both its colours and its styles.
    Theme(&'static CatalogueTheme, u8),
}

/// What an application did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The pages that were worked on, in the order given.
    pub pages: Vec<u32>,
    /// The pages whose cells or shapes changed.
    pub changed_pages: Vec<u32>,
    /// How many times a shape now follows the `Theme` style for a family it did not.
    pub links_made: usize,
    /// How many scheme cells local to a shape were given the page's numbers.
    pub cells_rewritten: usize,
    /// Whether the theme's part was added to the drawing.
    pub part_added: bool,
    /// Whether the `Theme` style was made, or filled in (a style of that name with no cell, which another program's save leaves).
    pub style_added: bool,
    /// How many shapes on a locked layer were passed over: a link or a cell would have been written into them.
    pub locked: usize,
}

impl Applied {
    /// Whether anything changed.
    pub fn changed(&self) -> bool {
        !self.changed_pages.is_empty() || self.part_added || self.style_added
    }
}

/// What to do to one shape.
#[derive(Debug, Default)]
struct ShapeEdit {
    path: Vec<u32>,
    /// The families (line, fill, text) that follow the `Theme` style from now on.
    link: [bool; 3],
    /// The scheme cells to give a number, as (place in [`SELECTORS`], the number).
    selectors: Vec<(usize, u32)>,
}

/// The part of the drawing a theme of the catalogue is: its part joins the drawing's themes when none carries its number. `true` if it was added.
fn ensure_part(doc: &mut Document, theme: &CatalogueTheme) -> Result<bool, Problem> {
    if doc.theme_by_index(theme.scheme()).is_some() {
        return Ok(false);
    }
    if doc.themes.len() >= MAX_THEMES {
        return Err(Problem::failed(format!("a drawing holds at most {MAX_THEMES} themes")));
    }
    let held = doc.themes.iter().fold(0_u64, |sum, t| sum.saturating_add(t.bytes.len() as u64));
    if held.saturating_add(theme.bytes.len() as u64) > MAX_THEME_BYTES {
        return Err(Problem::failed(format!("the drawing's themes would pass {MAX_THEME_BYTES} bytes")));
    }
    // The least number no theme part is named with. **The name is neutral**, `theme<n>.xml`, and is no package's path: a VSDX save names a part it adds itself (`vsdx::write::themes`:
    // "whatever name the model gave it"), so its package is the same whatever this name is, and the document file of an `.odg` writes the name as it is, where the folder of a VSDX
    // package would name the product in a file that is not its format. A theme read from a file keeps the name the file gave it.
    let taken: Vec<String> = doc.themes.iter().map(|t| t.part.to_ascii_lowercase()).collect();
    let part = (1..)
        .map(|n: u32| format!("theme{n}.xml"))
        .find(|name| !taken.contains(name))
        .ok_or_else(|| Problem::failed("no theme part name is left"))?;
    doc.themes.push(Arc::new(theme.theme(&part)));
    Ok(true)
}

/// Whether a `Lock...` cell of a shape is set: the cell, found along the shape's masters and styles, holds a number other than 0.
fn locked(resolver: &Resolver<'_>, at: &ShapeRef, lock: &str) -> bool {
    resolver
        .resolve(&SheetRef::Shape(at.clone()), &CellAddr::direct(lock))
        .and_then(|found| found.value.cell?.value.as_ref()?.number())
        .is_some_and(|n| n != 0.0)
}

/// Whether a family follows the style from now on: the shape has no style of its own for it, or the root style, and the master shape it stands for has
/// none either (a master that links a style of its own, or the file's own `Theme` style, is a style the shape already has: the shape's own link would
/// outrank the master's).
fn wants(link: Option<u32>, inherited: Option<u32>, style: Option<u32>) -> bool {
    style.is_some_and(|id| id != 0) && matches!(link, None | Some(0)) && matches!(inherited, None | Some(0))
}

/// The style links of the shapes of one master, found by `ID`: what an instance, or a member of one, inherits.
#[derive(Default)]
struct MasterLinks {
    /// The links of the first top-level shape that is not deleted: what an instance of the master inherits.
    first: Links,
    by_id: HashMap<u32, Links>,
}

/// The links of a master's shapes, one pass over them with a stack (a master past the cap of shapes is indexed as far as the cap).
fn index_master(doc: &Document, id: u32) -> MasterLinks {
    let mut index = MasterLinks::default();
    let Some(master) = doc.master(id) else { return index };
    index.first = master.shapes.iter().find(|s| !s.del).map(|s| s.sheet.links).unwrap_or_default();
    let mut seen = 0_usize;
    let mut stack: Vec<&ShapeList> = vec![&master.shapes];
    while let Some(list) = stack.pop() {
        for shape in list.iter() {
            seen = seen.saturating_add(1);
            if seen > MAX_SHAPES_PER_PAGE {
                return index;
            }
            index.by_id.entry(shape.id).or_insert(shape.sheet.links);
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
    index
}

/// What planning the edits of a page, or of one shape and its members, holds.
struct Planner<'a> {
    doc: &'a Document,
    page: u32,
    /// What the page selects after the command.
    target: [u32; 7],
    /// The `Theme` style to follow, if a shape is to follow one.
    style: Option<u32>,
    /// Made when a shape has a number of its own to change, once.
    resolver: Option<Resolver<'a>>,
    /// The links of the masters the shapes name, each indexed when first needed.
    masters: HashMap<u32, MasterLinks>,
    plan: Vec<ShapeEdit>,
    /// The shapes walked.
    seen: usize,
    /// The page's locked layers and what reading the shapes' lists cost, and how many shapes on them were passed over.
    locks: crate::draw::layers::Locks,
    locked: usize,
}

impl<'a> Planner<'a> {
    fn new(doc: &'a Document, page: u32, target: [u32; 7], style: Option<u32>) -> Planner<'a> {
        Planner {
            doc,
            page,
            target,
            style,
            resolver: None,
            masters: HashMap::new(),
            plan: Vec::new(),
            seen: 0,
            locks: crate::draw::layers::Locks::default(),
            locked: 0,
        }
    }

    /// The links a shape inherits from the master shape it stands for. `master` is the master of the nearest ancestor that has one.
    fn inherited(&mut self, shape: &Shape, master: Option<u32>) -> Links {
        let (m, id) = match (shape.master, shape.master_shape, master) {
            (Some(m), _, _) => (m, None),
            (None, Some(id), Some(m)) => (m, Some(id)),
            _ => return Links::default(),
        };
        let doc = self.doc;
        let index = self.masters.entry(m).or_insert_with(|| index_master(doc, m));
        match id {
            None => index.first,
            Some(id) => index.by_id.get(&id).copied().unwrap_or_default(),
        }
    }

    /// The edits one shape needs, added to the plan.
    fn shape(&mut self, path: &[u32], shape: &Shape, master: Option<u32>) -> Result<(), Problem> {
        // A shape draws: not a group (its members do), a guide or a picture. A shape with no `Type` that was not deleted locally is read as a shape
        // (the missing type is seen with `Del`, and a hand-written part may leave it out).
        let draws = match shape.kind {
            ShapeType::Shape => true,
            ShapeType::Unspecified => !shape.del,
            _ => false,
        };
        if !draws {
            return Ok(());
        }
        let (links, inherited) = (shape.sheet.links, self.inherited(shape, master));
        let style = self.style;
        let mut edit = ShapeEdit {
            path: path.to_vec(),
            link: [wants(links.line, inherited.line, style), wants(links.fill, inherited.fill, style), wants(links.text, inherited.text, style)],
            ..ShapeEdit::default()
        };
        for (i, name) in SELECTORS.iter().enumerate() {
            let Some(cell) = shape.sheet.cell(name) else { continue };
            let Some(now) = cell.value.as_ref().and_then(whole) else { continue };
            // A number of its own: not the page's (65534), not a custom scheme (65535), not a formula's, and not what the page selects already.
            if now == 65_534 || now == 65_535 || cell.formula != FormulaSlot::Absent {
                continue;
            }
            if let Some(&number) = self.target.get(i)
                && now != number
            {
                edit.selectors.push((i, number));
            }
        }
        if !edit.selectors.is_empty() {
            // Locks are looked up only for a shape that has a number of its own to change: most have none.
            let doc = self.doc;
            let resolver = self.resolver.get_or_insert_with(|| {
                #[cfg(test)]
                tests::RESOLVERS_MADE.with(|c| c.set(c.get() + 1));
                Resolver::new(doc)
            });
            let at = ShapeRef { owner: Owner::Page(self.page), path: path.to_vec() };
            edit.selectors.retain(|(i, _)| LOCKS.get(*i).is_none_or(|lock| !locked(resolver, &at, lock)));
        }
        if edit.link.iter().any(|l| *l) || !edit.selectors.is_empty() {
            // A shape on a locked layer is not written into: passed over, and counted. Its list is read only when it has something to change.
            if self.locks.of(self.doc, self.page, shape)?.is_some() {
                self.locked = self.locked.saturating_add(1);
                return Ok(());
            }
            if self.plan.len() >= MAX_THEMED_SHAPES {
                return Err(Problem::failed(format!("a theme is applied to at most {MAX_THEMED_SHAPES} shapes of a page at a time")));
            }
            self.plan.push(edit);
        }
        Ok(())
    }

    /// Plans every shape of `list` and everything in them, in document order. `prefix` is the path of the list (the page's is empty) and `master` the
    /// master of the nearest ancestor that has one. One iterative pass, no deeper than the model allows and at most [`MAX_SHAPES_PER_PAGE`] shapes.
    fn walk(&mut self, list: &'a ShapeList, prefix: &[u32], master: Option<u32>) -> Result<(), Problem> {
        // The list being walked, the place of the next shape in it and the master in force there: one frame for each level.
        let mut stack: Vec<(&'a ShapeList, usize, Option<u32>)> = vec![(list, 0, master)];
        let mut path: Vec<u32> = prefix.to_vec();
        while let Some(frame) = stack.last_mut() {
            let (list, at, master) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            self.seen = self.seen.saturating_add(1);
            if self.seen > MAX_SHAPES_PER_PAGE {
                return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
            }
            let depth = stack.len();
            path.truncate(prefix.len().saturating_add(depth).saturating_sub(1));
            path.push(u32::try_from(at).unwrap_or(u32::MAX));
            self.shape(&path, shape, master)?;
            if prefix.len().saturating_add(depth) < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, 0, shape.master.or(master)));
            }
        }
        Ok(())
    }
}

/// The shapes of page `page` that need an edit, in document order, and how many shapes on a locked layer it passed over.
fn plan_page(doc: &Document, page: u32, target: [u32; 7], style: Option<u32>) -> Result<(Vec<ShapeEdit>, usize), Problem> {
    let top = &doc.page(page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.shapes;
    let mut planner = Planner::new(doc, page, target, style);
    planner.walk(top, &[], None)?;
    Ok((planner.plan, planner.locked))
}

/// Makes the shape `id` that a command has just put on `page`, and the shapes inside it, follow the page's theme ("Apply theme to new
/// shapes": on by default): a shape with no style of its own, or the root style, for a family follows the `Theme` style for it,
/// as [`apply`] makes the shapes of a page follow it. Nothing is done when the page selects no theme or the drawing has no `Theme` style (a page of
/// a file that selects a theme and holds no such style is left as the file has it), or when the shape is not there. The dropped, drawn and
/// connecting commands call it inside their own edit, so the links are part of their one undo step.
pub fn follow_page_theme(doc: &mut Document, page: u32, id: u32) -> Result<(), Problem> {
    let Some(selection) = selection_of(doc, page).filter(Selection::is_themed) else { return Ok(()) };
    let Some(style) = theme_style_id(doc).filter(|s| *s != 0) else { return Ok(()) };
    let plan = {
        // One walk of the page, to find the shape by its `ID`.
        note_follow(1, 0);
        let Some(at) = doc.find_shape(Owner::Page(page), id) else { return Ok(()) };
        let Some(shape) = doc.shape(&at) else { return Ok(()) };
        let mut planner = Planner::new(doc, page, selection.numbers(), Some(style));
        // A shape just put on the page is at its top level: no ancestor has a master.
        planner.shape(&at.path, shape, None)?;
        planner.walk(&shape.children, &at.path, shape.master)?;
        note_follow(0, planner.seen.saturating_add(1));
        planner.plan
    };
    carry_out(doc, page, &plan, Some(style))?;
    Ok(())
}

/// The master of the nearest ancestor of the shape at `path` that has one, found by going down the path once (`None` for a shape at the top of the page): what [`Planner::shape`] needs to find the
/// links a member of a master's group inherits. It costs the depth of the path, which the model bounds.
fn ancestor_master(doc: &Document, page: u32, path: &[u32]) -> Option<u32> {
    let mut list = &doc.page(page)?.shapes;
    let mut master = None;
    for step in path.split_last().map_or(&[][..], |(_, ancestors)| ancestors) {
        let shape = list.get(usize::try_from(*step).ok()?)?;
        master = shape.master.or(master);
        list = &shape.children;
    }
    master
}

/// Makes the shapes at `refs` (places on `page` the caller already has) follow the page's theme, as [`follow_page_theme`] makes one shape: **one pass, one planner and one resolver for all of
/// them, and no walk of the page to find a shape by its `ID`** (the shapes are where `refs` say). A theme colour or Use Default asks for it (`formatting`): the formula of such a cell is
/// answered by the page's theme only for a shape that reads it.
///
/// **Bounded:** at most [`MAX_FOLLOWED_SHAPES`] shapes, refused in words before the first is looked at, and the plan is held to [`MAX_THEMED_SHAPES`] as a theme's is. Each shape costs the depth of its
/// path (its ancestors' masters) and one look at its own links, and a master is indexed once however many shapes are its instances (`Planner::inherited`). The shapes planned are counted
/// ([`follow_work`]). Nothing is done where the page selects no theme or the drawing has no `Theme` style, and a place that is no shape is passed over.
pub fn follow_page_theme_refs(doc: &mut Document, page: u32, refs: &[ShapeRef]) -> Result<(), Problem> {
    let Some(selection) = selection_of(doc, page).filter(Selection::is_themed) else { return Ok(()) };
    let Some(style) = theme_style_id(doc).filter(|s| *s != 0) else { return Ok(()) };
    if refs.len() > MAX_FOLLOWED_SHAPES {
        return Err(Problem::failed(format!(
            "a colour of the theme is put on at most {MAX_FOLLOWED_SHAPES} shapes at a time; {} were named: pick it for fewer, or apply the theme to the page (a theme gives its shapes its own colours without a pick)",
            refs.len()
        )));
    }
    let plan = {
        let mut planner = Planner::new(doc, page, selection.numbers(), Some(style));
        for at in refs {
            let Some(shape) = doc.shape(at) else { continue };
            planner.seen = planner.seen.saturating_add(1);
            planner.shape(&at.path, shape, ancestor_master(doc, page, &at.path))?;
        }
        note_follow(0, planner.seen);
        planner.plan
    };
    carry_out(doc, page, &plan, Some(style))?;
    Ok(())
}

/// Carries out the edits: the links and the scheme cells of each shape. Returns how many links were made and how many cells were rewritten.
fn carry_out(doc: &mut Document, page: u32, plan: &[ShapeEdit], style: Option<u32>) -> Result<(usize, usize), Problem> {
    let (mut links, mut cells) = (0_usize, 0_usize);
    for edit in plan {
        let shape = shape_mut(doc, page, &edit.path).ok_or_else(|| Problem::failed("a shape of the page is not there"))?;
        let sheet = Arc::make_mut(&mut shape.sheet);
        if let Some(id) = style {
            for (family, follows) in edit.link.iter().enumerate() {
                if !*follows {
                    continue;
                }
                let link = match family {
                    0 => &mut sheet.links.line,
                    1 => &mut sheet.links.fill,
                    _ => &mut sheet.links.text,
                };
                *link = Some(id);
                links = links.saturating_add(1);
            }
            if edit.link.iter().any(|l| *l) && sheet.raw.iter().any(|(name, _)| LINK_ATTRS.contains(&name)) {
                sheet.raw = RawAttrs::from_pairs(sheet.raw.iter().filter(|(name, _)| !LINK_ATTRS.contains(name)));
            }
        }
        for (place, number) in &edit.selectors {
            let Some(name) = SELECTORS.get(*place) else { continue };
            let Some(mut cell) = sheet.cell(name).cloned() else { continue };
            cell.value = Some(Value::from_text(&number.to_string()));
            sheet.set_cell(&CellAddr::direct(name), cell).map_err(|e| Problem::failed(e.to_string()))?;
            cells = cells.saturating_add(1);
        }
    }
    Ok((links, cells))
}

/// Works the whole page out again, which is what a changed theme asks of `calc` (the module documentation).
///
/// Applying a theme writes the page's selector cells, the style links of shapes and the theme part, and no size and no cell of a geometry row: the
/// arguments of an elliptical arc's `C` and `D` are what they were, so the pass leaves those pairs as the file holds them (`arcs_untouched`). The pass is
/// measured from `before`, the document as the command found it (`Changes::since`): an instance is given a cell it inherits only where the theme changed
/// what the cell works out to on it, and not one for every cell this evaluator works out otherwise than the file.
fn recalculate(doc: &mut Document, before: &Since, page: u32, budget: &Budget) -> Result<(), Problem> {
    let changes = Changes { whole_page: true, arcs_untouched: true, since: before.clone(), ..Changes::default() };
    let report = commit_changed(doc, budget, crate::measure::eval_options(), page, &changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(())
}

/// Writes the page's seven selector cells where they differ from `target`. Returns whether any was written.
fn write_page_cells(doc: &mut Document, page: u32, target: [u32; 7]) -> Result<bool, Problem> {
    let now = selection_of(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.numbers();
    let sheet = doc.page(page).map(|p| Arc::clone(&p.sheet));
    let writes: Vec<CellWrite> = SELECTORS
        .iter()
        .zip(target)
        .zip(now)
        .filter(|((name, wanted), held)| {
            // A page that holds no such cell and wants 0 is as it should be; a cell that holds the number already is left as it is.
            let present = sheet.as_ref().is_some_and(|s| s.cell(name).is_some());
            wanted != held || (!present && *wanted != 0)
        })
        .map(|((name, wanted), _)| CellWrite {
            addr: CellAddr::direct(name),
            edit: CellEdit::Value(Value::from_text(&wanted.to_string())),
            origin: WriteOrigin::Drawing,
        })
        .collect();
    if writes.is_empty() {
        return Ok(false);
    }
    doc.write_cells(&SheetRef::Page(page), writes)?;
    Ok(true)
}

/// What a page selects once the command is done.
fn target_of(choice: &Choice) -> [u32; 7] {
    match choice {
        Choice::None => [0; 7],
        Choice::Theme(theme, variant) => {
            let n = theme.scheme();
            let v = u32::from(*variant);
            [n, n, n, n, n, v, v]
        }
    }
}

/// Applies a choice to these pages (see the module documentation). All or nothing: the caller edits a copy of the document, so an error leaves
/// the drawing as it was. `base` is what a `Theme` style the drawing lacks is based on ([`Base`]); a style the drawing has is used as it is.
pub fn apply(doc: &mut Document, pages: &[u32], choice: Choice, base: Base) -> Result<Applied, Problem> {
    apply_within(doc, pages, choice, base, &Budget::default())
}

/// [`apply`] with the recalculation held to `budget`: a preview makes its copy of the drawing under a smaller one (`preview.rs`). A page too large to be worked out within it is an
/// error and the document is as it was found, as it is for the default budget.
pub fn apply_within(doc: &mut Document, pages: &[u32], choice: Choice, base: Base, budget: &Budget) -> Result<Applied, Problem> {
    for page in pages {
        require_page(doc, *page)?;
    }
    let before = Since::of(doc);
    let mut applied = Applied { pages: pages.to_vec(), ..Applied::default() };
    let style = match choice {
        Choice::Theme(theme, _) => {
            applied.part_added = ensure_part(doc, theme)?;
            let (id, made) = ensure_theme_style(doc, base)?;
            applied.style_added = made;
            Some(id)
        }
        // Shapes that follow the style already stay as they are: with every selector at 0 they draw as the root style does.
        Choice::None => theme_style_id(doc),
    };
    for &page in pages {
        let target = target_of(&choice);
        let follow = if matches!(choice, Choice::None) { None } else { style };
        let wrote = write_page_cells(doc, page, target)?;
        let (plan, locked) = plan_page(doc, page, target, follow)?;
        let (links, cells) = carry_out(doc, page, &plan, follow)?;
        applied.links_made = applied.links_made.saturating_add(links);
        applied.cells_rewritten = applied.cells_rewritten.saturating_add(cells);
        applied.locked = applied.locked.saturating_add(locked);
        if wrote || !plan.is_empty() {
            recalculate(doc, &before, page, budget)?;
            applied.changed_pages.push(page);
        }
    }
    Ok(applied)
}

/// Picks a variant of the theme the pages select: `colours` and `styles` are places 0 to 3 in the lists of variants. A page that selects no theme
/// is an error. All or nothing, as [`apply`].
pub fn set_variants(doc: &mut Document, pages: &[u32], colours: Option<u8>, styles: Option<u8>) -> Result<Applied, Problem> {
    set_variants_within(doc, pages, colours, styles, &Budget::default())
}

/// [`set_variants`] with the recalculation held to `budget` (see [`apply_within`]).
pub fn set_variants_within(doc: &mut Document, pages: &[u32], colours: Option<u8>, styles: Option<u8>, budget: &Budget) -> Result<Applied, Problem> {
    for page in pages {
        require_page(doc, *page)?;
        if !selection_of(doc, *page).is_some_and(|s| s.is_themed()) {
            return Err(Problem::failed(format!("page {page} has no theme to give a variant")));
        }
    }
    let before = Since::of(doc);
    let mut applied = Applied { pages: pages.to_vec(), ..Applied::default() };
    for &page in pages {
        let now = selection_of(doc, page).unwrap_or_default();
        let mut target = now.numbers();
        if let Some(c) = colours {
            target[5] = u32::from(c);
        }
        if let Some(s) = styles {
            target[6] = u32::from(s);
        }
        let wrote = write_page_cells(doc, page, target)?;
        let (plan, locked) = plan_page(doc, page, target, None)?;
        let (_, cells) = carry_out(doc, page, &plan, None)?;
        applied.cells_rewritten = applied.cells_rewritten.saturating_add(cells);
        applied.locked = applied.locked.saturating_add(locked);
        if wrote || !plan.is_empty() {
            recalculate(doc, &before, page, budget)?;
            applied.changed_pages.push(page);
        }
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::cell::Cell;

    use diacraft_doc::{Cell as DocCell, CellName, Page};

    use super::*;

    thread_local! {
        /// The resolvers `Planner::shape` made: counted, so that a test holds the locks' lookup to one resolver for the page.
        pub(super) static RESOLVERS_MADE: Cell<usize> = const { Cell::new(0) };
    }

    /// A shape that holds a scheme cell of its own, a number the page does not select, and a lock.
    fn shape_with_own_cells(id: u32) -> Shape {
        let mut shape = Shape::new(id);
        shape.kind = ShapeType::Shape;
        let mut sheet = diacraft_doc::Sheet::default();
        for (name, value) in [("ColorSchemeIndex", "40"), ("LockThemeFonts", "0")] {
            sheet.cells.insert(CellName::new(name), DocCell::constant(Value::from_text(value)));
        }
        shape.sheet = Arc::new(sheet);
        shape
    }

    #[test]
    fn the_locks_of_a_page_are_found_through_one_resolver_whatever_the_shapes() {
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec((1..=2_000).map(|id| Arc::new(shape_with_own_cells(id))).collect());
        doc.pages.push(Arc::new(page));
        RESOLVERS_MADE.with(|c| c.set(0));
        let (plan, _) = plan_page(&doc, 1, [1001, 1001, 1001, 1001, 1001, 0, 0], None).unwrap();
        assert_eq!(plan.len(), 2_000, "every shape has a number of its own to change");
        assert_eq!(RESOLVERS_MADE.with(Cell::get), 1, "one resolver for the page, and not one for each shape");
    }

    #[test]
    fn a_page_with_no_shape_that_has_a_number_of_its_own_makes_no_resolver() {
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec((1..=100).map(|id| Arc::new(Shape::new(id))).collect());
        doc.pages.push(Arc::new(page));
        RESOLVERS_MADE.with(|c| c.set(0));
        plan_page(&doc, 1, [0; 7], Some(1)).unwrap();
        assert_eq!(RESOLVERS_MADE.with(Cell::get), 0);
    }
}
