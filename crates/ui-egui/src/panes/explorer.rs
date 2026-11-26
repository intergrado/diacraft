//! Drawing Explorer: a read-only tree of the document: the foreground pages, the background pages, the
//! styles, the masters and the patterns, in the order R20 lists them. A row for a page goes to that page (`page.select`); a branch folds and unfolds
//! (`window.explorerBranch`).
//!
//! **The work is bounded** (counted by tests, `work.rs`): the tree is worked out from the document once for each revision of the drawing, in one pass that only
//! counts; the names of a branch are built when the branch is first unfolded, and only the first [`ROWS_MAX`] of them.
//!
//! **A right click on the drawing (the first row), a style or a master opens its ShapeSheet** (the Drawing Explorer row): the document's, the style's, and
//! the master's first top-level shape (DiaCraft has no master window, so the explorer opens the shape of the master, where its cells are).
//!
//! Choices no specification settles (ours): the tree stops at the names of the pages, masters and styles (the shapes of a page are not listed, and F2 does not
//! rename an item: both are in the row of and not built); a branch lists at most [`ROWS_MAX`] rows and says how many it leaves out; a
//! master whose pattern flags name a fill pattern, a line pattern or a line end is listed under that name and not under Masters;
//! a page the sheet's `UIVisibility` hides from lists is listed all the same, as the page tabs list it.

use std::collections::HashMap;

use egui::{Align2, CornerRadius, Pos2, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};
use crate::work;

/// The most rows a branch lists.
pub(crate) const ROWS_MAX: usize = work::EXPLORER_ROWS_MAX;
/// The height of a row, and the indent of the rows under a branch, in points.
const ROW_H: f32 = 22.0;
const INDENT: f32 = 16.0;

/// A branch of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Branch {
    ForegroundPages,
    BackgroundPages,
    Masters,
    Styles,
    FillPatterns,
    LinePatterns,
    LineEnds,
}

impl Branch {
    /// The branches in the order R20 lists them: foreground pages, background pages, styles, masters, fill patterns, line patterns and line ends.
    pub const ALL: [Branch; 7] = [
        Branch::ForegroundPages,
        Branch::BackgroundPages,
        Branch::Styles,
        Branch::Masters,
        Branch::FillPatterns,
        Branch::LinePatterns,
        Branch::LineEnds,
    ];

    /// The word the window command and `ui.elements` call it.
    pub const fn word(self) -> &'static str {
        match self {
            Branch::ForegroundPages => "foreground_pages",
            Branch::BackgroundPages => "background_pages",
            Branch::Masters => "masters",
            Branch::Styles => "styles",
            Branch::FillPatterns => "fill_patterns",
            Branch::LinePatterns => "line_patterns",
            Branch::LineEnds => "line_ends",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Branch::ForegroundPages => "Foreground Pages",
            Branch::BackgroundPages => "Background Pages",
            Branch::Masters => "Masters",
            Branch::Styles => "Styles",
            Branch::FillPatterns => "Fill Patterns",
            Branch::LinePatterns => "Line Patterns",
            Branch::LineEnds => "Line Ends",
        }
    }

    pub fn from_name(name: &str) -> Option<Branch> {
        let name = name.trim();
        Branch::ALL.into_iter().find(|b| b.word().eq_ignore_ascii_case(name) || b.title().eq_ignore_ascii_case(name))
    }

    /// Whether the branch is unfolded before the person has chosen: the pages are, the rest is not.
    const fn unfolded_at_first(self) -> bool {
        matches!(self, Branch::ForegroundPages | Branch::BackgroundPages)
    }

    /// The branch a master with these pattern flags is listed under.
    fn of_master(flags: Option<u32>) -> Branch {
        match flags {
            Some(1 | 65) => Branch::LinePatterns,
            Some(2 | 1026) => Branch::LineEnds,
            Some(4100 | 20484) => Branch::FillPatterns,
            _ => Branch::Masters,
        }
    }
}

impl Branch {
    /// Where the branch is in [`Branch::ALL`].
    fn at(self) -> usize {
        Branch::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }
}

/// What the tree remembers: the branches the person has folded or unfolded, and the tree as it was made for the last revision of the drawing.
#[derive(Default)]
pub struct ExplorerState {
    chosen: HashMap<Branch, bool>,
    tree: Option<Tree>,
}

impl ExplorerState {
    /// A drawing replaced the open one: the tree of the old one is not shown for the new. The branches the person folded or unfolded stay as they are.
    pub(crate) fn forget_tree(&mut self) {
        self.tree = None;
    }
}

/// The tree for one revision of the drawing: how many rows each branch has (worked out from the document in one pass that builds no name), and the rows of
/// the branches that have been unfolded (built once, the first [`ROWS_MAX`] of each).
struct Tree {
    /// The drawing's generation and revision it was made for: a file that is opened starts its revision at 1 again, so the revision alone is not the drawing.
    generation: u64,
    revision: u64,
    counts: [usize; 7],
    rows: HashMap<Branch, Vec<Row>>,
}

/// The name a row shows: the document's name cut for the screen, or `fallback` where the document has none.
fn row_name(name: Option<&str>, name_u: Option<&str>, fallback: impl FnOnce() -> String) -> String {
    name.or(name_u).filter(|n| !n.is_empty()).map_or_else(fallback, |n| diacraft_engine::shown(n, 48))
}

impl Tree {
    /// Counts the rows of each branch: one pass over the pages, masters and styles that builds no name.
    fn counted(doc: &diacraft_engine::doc::Document, generation: u64, revision: u64) -> Tree {
        let mut counts = [0usize; 7];
        for page in doc.pages.iter() {
            counts[if page.background { Branch::BackgroundPages } else { Branch::ForegroundPages }.at()] += 1;
        }
        for master in doc.masters.iter() {
            counts[Branch::of_master(master.pattern_flags).at()] += 1;
        }
        counts[Branch::Styles.at()] = doc.styles.len();
        work::count(|w| w.explorer_scans += 1);
        Tree { generation, revision, counts, rows: HashMap::new() }
    }

    /// The first [`ROWS_MAX`] rows of `branch`: only their names are built.
    fn rows_of(doc: &diacraft_engine::doc::Document, branch: Branch) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        match branch {
            Branch::ForegroundPages | Branch::BackgroundPages => {
                let background = branch == Branch::BackgroundPages;
                for page in doc.pages.iter().filter(|p| p.background == background).take(ROWS_MAX) {
                    rows.push(Row { page: Some(page.id), name: diacraft_engine::shown(&page.name, 48), sheet: None });
                }
            }
            Branch::Styles => {
                for style in doc.styles.iter().take(ROWS_MAX) {
                    rows.push(Row {
                        page: None,
                        name: row_name(style.name.as_deref(), style.name_u.as_deref(), || format!("Style.{}", style.id)),
                        sheet: Some(Sheet::Style(style.id)),
                    });
                }
            }
            kind => {
                for master in doc.masters.iter().filter(|m| Branch::of_master(m.pattern_flags) == kind).take(ROWS_MAX) {
                    rows.push(Row {
                        page: None,
                        name: row_name(master.name.as_deref(), master.name_u.as_deref(), || format!("Master.{}", master.id)),
                        sheet: Some(Sheet::Master(master.id)),
                    });
                }
            }
        }
        work::count(|w| w.explorer_names += rows.len());
        rows
    }
}

impl ExplorerState {
    pub fn is_open(&self, branch: Branch) -> bool {
        self.chosen.get(&branch).copied().unwrap_or(branch.unfolded_at_first())
    }
}

/// What a row stands for besides a page: the object whose ShapeSheet its menu opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sheet {
    Style(u32),
    Master(u32),
}

/// One row of a branch.
struct Row {
    /// The page it goes to, for a page.
    page: Option<u32>,
    name: String,
    /// The style or the master it is, whose sheet a right click opens.
    sheet: Option<Sheet>,
}

impl DiacraftApp {
    /// Whether a branch of Drawing Explorer is unfolded. For tests and the window state.
    pub fn explorer_branch_open(&self, branch: Branch) -> bool {
        self.ui.panes.explorer.is_open(branch)
    }

    /// `window.explorerBranch`: folds or unfolds a branch; without `open` it is toggled.
    pub(crate) fn explorer_branch_command(&mut self, name: &str, open: Option<bool>) -> diacraft_engine::Result<()> {
        let branch = Branch::from_name(name).ok_or_else(|| {
            let names: Vec<&str> = Branch::ALL.iter().map(|b| b.word()).collect();
            diacraft_engine::EngineError::BadParams {
                cmd: "window.explorerBranch".to_owned(),
                msg: format!("no branch `{}`; the branches are: {}", diacraft_engine::shown(name, 32), names.join(", ")),
            }
        })?;
        let open = open.unwrap_or(!self.ui.panes.explorer.is_open(branch));
        self.ui.panes.explorer.chosen.insert(branch, open);
        Ok(())
    }

    /// The window state's word for the branches that are unfolded.
    pub(crate) fn explorer_state(&self) -> Value {
        json!(Branch::ALL.iter().filter(|b| self.ui.panes.explorer.is_open(**b)).map(|b| b.word()).collect::<Vec<_>>())
    }

    pub(super) fn explorer_body(&mut self, ui: &mut Ui, t: &Tokens) {
        let (generation, revision) = (self.ui.generation, self.session.revision());
        // The tree is made again only when the drawing is another revision of another generation; the rows of a branch are made when it is first shown.
        let counted = self.ui.panes.explorer.tree.take().filter(|tree| tree.generation == generation && tree.revision == revision);
        let tree = match counted {
            Some(tree) => tree,
            None => match self.session.document() {
                Some(doc) => Tree::counted(doc, generation, revision),
                None => {
                    ui.add(egui::Label::new(egui::RichText::new("No drawing open.").font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
                    return;
                }
            },
        };
        let mut tree = tree;
        let current = self.current_page();
        self.drawing_row(ui, t);
        for branch in Branch::ALL {
            let count = tree.counts[branch.at()];
            // A branch with nothing in it is not listed, except the foreground pages, which a drawing always has.
            if count == 0 && branch != Branch::ForegroundPages {
                continue;
            }
            let open = self.ui.panes.explorer.is_open(branch);
            self.branch_row(ui, t, branch, count, open);
            if !open {
                continue;
            }
            if !tree.rows.contains_key(&branch)
                && let Some(doc) = self.session.document()
            {
                tree.rows.insert(branch, Tree::rows_of(doc, branch));
            }
            let rows: &[Row] = tree.rows.get(&branch).map_or(&[], Vec::as_slice);
            for row in rows {
                self.tree_row(ui, t, row, row.page.is_some() && row.page == current);
            }
            if count > rows.len() {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                let words = format!("{} more not listed", count - rows.len());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &words));
                self.note(format!("explorer.more.{}", branch.word()), r, false);
                ui.painter().text(pos2(r.left() + INDENT + 12.0, r.center().y), Align2::LEFT_CENTER, words, theme::font(size::LABEL), t.text_dim);
            }
        }
        self.ui.panes.explorer.tree = Some(tree);
    }

    fn branch_row(&mut self, ui: &mut Ui, t: &Tokens, branch: Branch, count: usize, open: bool) {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let words = format!("{} ({count})", branch.title());
        widgets::name_toggle(&resp, true, open, &words);
        paint_button_bg(ui, r, &resp, true, false, t, 3);
        paint_arrow(ui, pos2(r.left() + 8.0, r.center().y), open, t.text);
        ui.painter().text(pos2(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, &words, theme::font(size::LABEL), t.text);
        self.note(format!("explorer.branch.{}", branch.word()), r, true);
        if resp.clicked() {
            let _ = self.execute("window.explorerBranch", &json!({ "branch": branch.word(), "open": !open }));
        }
    }

    /// The first row, the drawing: a right click opens its ShapeSheet.
    fn drawing_row(&mut self, ui: &mut Ui, t: &Tokens) {
        let name = self.display_name();
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &name));
        paint_button_bg(ui, r, &resp, true, false, t, 3);
        let clip = ui.painter().with_clip_rect(r);
        clip.text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, &name, theme::font_strong(size::LABEL), t.text);
        self.note("explorer.drawing", r, true);
        self.sheet_row_menu(&resp, None);
    }

    /// The right-click menu of a row that has a sheet: Show ShapeSheet (the drawing's when `of` is none).
    fn sheet_row_menu(&mut self, resp: &egui::Response, of: Option<Sheet>) {
        let t = Tokens::get(&resp.ctx);
        let mut chosen = false;
        resp.context_menu(|ui| {
            ui.set_width(170.0);
            let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
            widgets::name_button(&row, true, "Show ShapeSheet");
            paint_button_bg(ui, rect, &row, true, false, &t, 3);
            ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, "Show ShapeSheet", theme::font(size::LABEL), t.text);
            self.note("explorer.menu.sheet", rect, true);
            if row.clicked() {
                chosen = true;
                ui.close();
            }
        });
        if !chosen {
            return;
        }
        let params = match of {
            None => json!({"of": "document"}),
            Some(Sheet::Style(id)) => json!({"of": "style", "style": id}),
            Some(Sheet::Master(id)) => {
                // The master's first top-level shape that is not deleted, where its cells are; a master with none has its page sheet.
                let shape = self.session().document().and_then(|d| d.master(id)).and_then(|m| m.shapes.iter().find(|s| !s.del).map(|s| s.id));
                match shape {
                    Some(shape) => json!({"of": "master", "master": id, "id": shape}),
                    None => json!({"of": "master", "master": id}),
                }
            }
        };
        if let Err(e) = self.execute("window.shapesheet", &params) {
            self.set_notice(&e.to_string());
        }
    }

    fn tree_row(&mut self, ui: &mut Ui, t: &Tokens, row: &Row, current: bool) {
        let go = row.page.is_some();
        let sheet = row.sheet;
        let (r, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), if go || sheet.is_some() { Sense::click() } else { Sense::hover() });
        if go {
            widgets::name_toggle(&resp, true, current, &row.name);
            if current {
                ui.painter().rect_filled(r, CornerRadius::same(3), t.accent_soft);
            } else {
                paint_button_bg(ui, r, &resp, true, false, t, 3);
            }
        } else {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, sheet.is_some(), &row.name));
        }
        let clip = ui.painter().with_clip_rect(r);
        clip.text(
            pos2(r.left() + INDENT + 12.0, r.center().y),
            Align2::LEFT_CENTER,
            &row.name,
            theme::font(size::LABEL),
            if go { label_colour(t, true) } else { t.text_dim },
        );
        if let Some(id) = row.page {
            self.note(format!("explorer.page.{id}"), r, true);
            if resp.clicked() {
                let _ = self.execute("page.select", &json!({ "id": id }));
            }
        }
        match sheet {
            Some(Sheet::Style(id)) => self.note(format!("explorer.style.{id}"), r, true),
            Some(Sheet::Master(id)) => self.note(format!("explorer.master.{id}"), r, true),
            None => {}
        }
        if sheet.is_some() {
            self.sheet_row_menu(&resp, sheet);
        }
    }
}

/// A small triangle: pointing right when the branch is folded, down when it is unfolded.
fn paint_arrow(ui: &Ui, c: Pos2, open: bool, colour: egui::Color32) {
    let pts = if open {
        vec![c + vec2(-4.0, -2.5), c + vec2(4.0, -2.5), c + vec2(0.0, 3.0)]
    } else {
        vec![c + vec2(-2.5, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.5, 4.0)]
    };
    ui.painter().add(Shape::convex_polygon(pts, colour, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_master_is_listed_under_what_its_pattern_flags_say() {
        assert_eq!(Branch::of_master(None), Branch::Masters);
        assert_eq!(Branch::of_master(Some(0)), Branch::Masters);
        assert_eq!(Branch::of_master(Some(1)), Branch::LinePatterns);
        assert_eq!(Branch::of_master(Some(65)), Branch::LinePatterns);
        assert_eq!(Branch::of_master(Some(2)), Branch::LineEnds);
        assert_eq!(Branch::of_master(Some(1026)), Branch::LineEnds);
        assert_eq!(Branch::of_master(Some(4100)), Branch::FillPatterns);
        assert_eq!(Branch::of_master(Some(20484)), Branch::FillPatterns);
    }

    #[test]
    fn branches_are_found_by_word_or_title_and_the_pages_start_unfolded() {
        assert_eq!(Branch::from_name("masters"), Some(Branch::Masters));
        assert_eq!(Branch::from_name(" Line Ends "), Some(Branch::LineEnds));
        assert_eq!(Branch::from_name("nothing"), None);
        let state = ExplorerState::default();
        assert!(state.is_open(Branch::ForegroundPages) && !state.is_open(Branch::Styles));
        let mut words: Vec<&str> = Branch::ALL.iter().map(|b| b.word()).collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), Branch::ALL.len());
    }
}
