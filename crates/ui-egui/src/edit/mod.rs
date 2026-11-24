//! Drawing with the mouse and keyboard: the Pointer Tool and the Connector tool on the drawing area, the selection and its handles, the
//! Shapes window's tiles dropped on the page, text edited in place.
//!
//! **Every change is a command.** The window never edits the document itself: a drop is `shape.drop`, the end of a drag is
//! `shape.move`, `shape.resize`, `connector.add` or `connector.reconnect`, Delete is `shape.delete`, an edit of text is
//! `shape.set_text` (`docs/editing-commands.md`). Each is one undo step, so the toolbar's Undo and Ctrl+Z cover all of it.
//!
//! **A drag is drawn at once and committed on release.** While the pointer is down the window paints a light preview of the moving
//! shapes' outlines over the last picture and sends nothing; the one command goes on release. The render worker then draws a new
//! picture, and until it arrives the preview stays where the shapes now are, so nothing jumps back for a moment.
//!
//! **What the pointer meets comes from the model** (`scene.rs`): the shapes' frames and geometry, not the picture. Selection lives
//! here and not in the document (`docs/editing-commands.md`: "selection stays in the window").
//!
//! Choices no specification settles are marked `Assumed` in the code and listed in the task's report.

pub mod autoconnect;
mod block;
mod callout;
mod draw;
pub mod geom;
mod gesture;
mod guides;
mod keys;
mod legs;
mod paint;
mod region;
pub mod scene;
mod select_look;
pub mod snapping;
mod text;
pub(crate) mod tiles;

use egui::{Pos2, Rect, Ui};
use serde_json::{Value, json};

use self::geom::P;
use self::scene::{Item, Scene};
use crate::app::DiacraftApp;
use crate::canvas::page_drawing::PageSettings;
use crate::canvas::view::View;

pub use self::block::ShownBlock;
pub(crate) use self::draw::tool_of_command;
pub use self::paint::MAX_POINTS_MARKED;
pub use self::text::{plain_text, shape_text};
pub use self::tiles::TileDrag;

/// How far, in points, the pointer must move with the button down before a press becomes a drag.
pub const DRAG_START: f32 = 4.0;
/// How near, in points, the pointer must be to a line to hit it.
pub const HIT_REACH: f32 = 5.0;
/// How near, in points, to a handle or the end of a connector to take hold of it.
pub const HANDLE_REACH: f32 = 8.0;
/// How near, in points, to a connection point to be glued to it.
pub const POINT_REACH: f32 = 10.0;
/// The least a side of a shape can be made by dragging a handle, in inches.
pub const LEAST_SIZE: f64 = 0.1;
/// How long a preview of a finished edit is kept waiting for the picture that shows it. After this the picture is trusted to be late and
/// the preview goes.
const PREVIEW_PATIENCE: std::time::Duration = std::time::Duration::from_millis(1500);

/// The tool the pointer works with (Home, Tools).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Select, move and resize.
    #[default]
    Pointer,
    /// Drag from one shape to another to join them. **Assumed (from the spec's text): it stays on until the Pointer Tool or Esc.**
    Connector,
    /// A click or a drag on bare page makes a box of text; a click on a shape types into that shape ("Text tool and text
    /// boxes"). **Assumed (no specification says): the tool is used up by the box it opens, and the Pointer Tool is back while the person types.**
    Text,
    /// Moves, resizes and turns the text block of the one shape selected, apart from the shape. Stays on
    /// until the Pointer Tool or Esc, as the Connector tool does.
    TextBlock,
    /// One of the six drawing tools (Home, Tools, Drawing Tools; `draw.rs`): a drag draws a shape. Stays on until the Pointer Tool or Esc, as the Connector tool does.
    Draw(diacraft_engine::draw::pen::Tool),
    /// Home, Editing, Select, Area Select (`region.rs`): the next drag draws a net, even from a press on a shape. **Assumed (the page says "the next drag"): one drag uses it up and the
    /// Pointer Tool is back**; a click with no drag leaves it on.
    AreaSelect,
    /// Home, Editing, Select, Lasso Select (and the Draw tab's): the next drag draws a freeform loop, and the shapes inside it are selected; used up by one drag as Area Select is.
    Lasso,
}

impl Tool {
    /// The tool's name in the window's state and commands.
    pub const fn name(self) -> &'static str {
        match self {
            Tool::Pointer => "pointer",
            Tool::Connector => "connector",
            Tool::Text => "text",
            Tool::TextBlock => "text_block",
            Tool::Draw(tool) => tool.name(),
            Tool::AreaSelect => "area_select",
            Tool::Lasso => "lasso_select",
        }
    }
}

/// What one frame of the drawing area knows about where things are.
#[derive(Clone, Copy)]
pub struct Cx {
    pub area: Rect,
    pub view: View,
    pub page: u32,
    pub settings: PageSettings,
}

impl Cx {
    /// The point of the page, in inches, under a screen point.
    pub fn page_at(&self, at: Pos2) -> P {
        self.view.to_page(self.area, at)
    }

    /// The screen point of a point of the page.
    pub fn screen_at(&self, p: P) -> Pos2 {
        self.view.to_screen(self.area, p)
    }

    /// `points` screen points as inches of the page at this zoom.
    pub fn reach(&self, points: f32) -> f64 {
        f64::from(points) / self.view.scale()
    }
}

/// The primary button and the pointer in one frame.
struct Pointer {
    /// Where the pointer last was, anywhere in the window.
    latest: Option<Pos2>,
    /// Where it is when it is over the drawing area and nothing is over that.
    over: Option<Pos2>,
    pressed: bool,
    down: bool,
    released: bool,
    mods: egui::Modifiers,
}

/// A finished edit whose result the picture on the screen does not show yet: the outlines are painted where the shapes now are.
struct After {
    page: u32,
    revision: u64,
    paths: Vec<Vec<P>>,
    /// When the edit finished, by the window's clock.
    since: std::time::Duration,
}

/// What the window remembers of drawing: the tool, the selection, the gesture in progress, the text being edited.
#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    /// The Format Painter, while it is on: the look it picked up and whether it stays on (`painter.rs`). A mode of the drawing area beside the tools: choosing a tool puts it down.
    pub(crate) painter: Option<crate::painter::Painter>,
    /// The selected shapes of [`EditState::selection_page`], **the one chosen first leading** (what Align and the shape operations take as the first shape,
    /// `operations.rs`): a click or a press selects one shape alone, a Shift-click or a Ctrl-click adds a shape at the end, a rubber band lists the shapes it
    /// takes in the page's order (back to front) and a Shift-rubber-band adds them after what was selected.
    selection: Vec<u32>,
    selection_page: Option<u32>,
    scene: Option<Scene>,
    gesture: Option<gesture::Gesture>,
    tile: Option<TileDrag>,
    text: Option<text::TextEditing>,
    after: Option<After>,
    /// Where the pointer was over the page (inches) in the last frame, for the highlight of the connection point under it.
    hover: Option<P>,
    /// Where the last right click on the page was, in inches: what Paste in the context menu pastes at.
    context: Option<P>,
    told_unread: bool,
    /// Where the text block of the shape selected is, for the Text Block tool: read again when the drawing, the page or the shape changes.
    block: Option<block::BlockView>,
    /// The guide selected, by `ID`. Never together with selected shapes: selecting one drops the other.
    guide: Option<u32>,
    /// A guide being dragged out of a ruler, and where the pointer is on the page while it is over the drawing area.
    ruler: Option<guides::RulerDrag>,
    ruler_pointer: Option<P>,
    /// AutoConnect: the shape the pointer rests on and the flyout open (`autoconnect.rs`).
    pub(crate) auto: autoconnect::AutoState,
    /// The places the pointer went through in this frame, as egui gave them (a stroke of the Pencil or the Freeform tool is made of them, `draw.rs`).
    trail: Vec<Pos2>,
    /// The drawing tool last used: the Drawing Tools button shows it (Home, Tools).
    pub(crate) last_pen: Option<diacraft_engine::draw::pen::Tool>,
    /// The route of the one connector selected, for the handles on its legs (`legs.rs`).
    legs: Option<legs::Legs>,
    /// The end of the leader of the one callout selected, for its yellow handle (`callout.rs`).
    tip: Option<callout::Tip>,
    /// The drawing, page and shape the end of a leader was last asked for (`callout.rs`), found or not, so that a shape that is no callout is not asked about again until something changes.
    tip_asked: Option<(u64, u64, u32, u32)>,
    /// The page as the engine's snap reads it (its guides, its grid, its settings), kept while the document and the page stay as they are.
    snap_page: Option<snapping::SnapCache>,
    /// What makes the index of a page's shapes and the targets of a drag ready off the window's thread, and how many answers it has given: a landing worked out
    /// before an answer is worked out again after it.
    reader: crate::snap_reader::SnapReader,
    /// Whether the reader was given by the caller ([`DiacraftApp::with_snap_reader`]) and not the window's own.
    reader_given: bool,
    reads_done: u64,
    /// Where the tile being dragged would land, and what it was worked out for, so that a frame with the pointer still asks nothing.
    tile_landing: Option<tiles::TileLanding>,
    /// The `snap` of the answer of the last command a drag sent with it: what it snapped to and the shapes it held to guides (`held`).
    last_snap: Option<Value>,
    /// The document's Snap & Glue settings, with the drawing's generation and the revision they were read at: what the Dynamic Grid box shows
    /// and the window's own glue reads every frame, read once for each revision of a drawing (`snapping.rs`, `snap_settings_now`).
    settings: std::cell::Cell<Option<((u64, u64), diacraft_engine::snap::SnapSettings)>>,
}

impl EditState {
    /// Forgets everything about the old drawing: another one replaced it. The reader of pages is kept: what it is working on is of the old drawing, and its answer finds no
    /// page it is for.
    pub fn reset(&mut self) {
        let (tool, reader, given, pen) = (self.tool, std::mem::take(&mut self.reader), self.reader_given, self.last_pen);
        *self = EditState::default();
        (self.tool, self.reader, self.reader_given, self.last_pen) = (tool, reader, given, pen);
    }

    /// Gives the drag's snap another reader of pages (a test's).
    pub(crate) fn set_reader(&mut self, reader: crate::snap_reader::SnapReader) {
        self.reader = reader;
        self.reader_given = true;
    }

    /// Whether the caller gave the drag's snap its reader of pages.
    pub(crate) fn reader_given(&self) -> bool {
        self.reader_given
    }

    /// The selected shapes' `ID`s.
    pub fn selection(&self) -> &[u32] {
        &self.selection
    }

    /// The page the selected shapes are on (what the shape operations are asked about). `None` before anything was selected.
    pub(crate) fn selection_page(&self) -> Option<u32> {
        self.selection_page
    }

    /// The scene last made of a page, if one has been.
    pub(crate) fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }

    /// Where the right click that opened the context menu was, in inches of the page.
    pub(crate) fn context_at(&self) -> Option<(f64, f64)> {
        self.context
    }

    /// Forgets the selection and the drag: the shapes were cut.
    pub(crate) fn clear_selection_after_cut(&mut self) {
        self.selection.clear();
        self.gesture = None;
        self.after = None;
    }

    /// Whether a text box is open over a shape.
    pub fn editing_text(&self) -> bool {
        self.text.is_some()
    }

    /// Whether a drag is in progress.
    pub fn dragging(&self) -> bool {
        self.gesture.as_ref().is_some_and(|g| g.active) || self.tile.is_some() || self.ruler.is_some()
    }

    /// The guide selected, if one is.
    pub fn guide(&self) -> Option<u32> {
        self.guide
    }

    /// Drops a guide being dragged out of a ruler: the rulers are hidden.
    pub(crate) fn forget_ruler_drag(&mut self) {
        self.ruler = None;
        self.ruler_pointer = None;
    }

    /// The `snap` the last drag's command answered with (`null` when it was sent without), if a drag has sent one.
    pub fn last_snap(&self) -> Option<&Value> {
        self.last_snap.as_ref()
    }
}

/// Where the drag in progress would land, for `ui.inspect` and the tests: what the preview shows and the release sends.
#[derive(Clone, Debug, PartialEq)]
pub struct DragPreview {
    /// A move: how far the shapes go, in paper inches.
    pub delta: Option<(f64, f64)>,
    /// A resize: where the shape's box goes (lower left, then width and height, paper inches).
    pub sized: Option<diacraft_engine::snap::BoxAt>,
    /// A turn by the rotation handle: the angle the shape comes to, in degrees from just over -180 up to 180.
    pub turned: Option<f64>,
    /// The Text tool's drag: the corner the press landed at and the corner the pointer's lands at, paper inches: the box `text.box` is sent when the edit ends.
    pub corners: Option<(P, P)>,
    /// A guide's drag, or one out of a ruler: where the guide goes, along its axis.
    pub guide_at: Option<f64>,
    /// A guide's drag: the shapes held to it, which go with it.
    pub held: Vec<u32>,
    /// A join or a reshape: where the end dragged goes, and the shape and connection point it is glued to there, if one is.
    pub end: Option<(f64, f64)>,
    pub end_glued: Option<(u32, u32)>,
    /// A leg dragged by hand (`legs.rs`): the line it lands on, in paper inches (its height for a level leg, its place along x for an upright one).
    pub leg_to: Option<f64>,
    /// What the snap pulled the drag to, and what lies where it lands: the engine's marks, which the preview draws.
    pub marks: Vec<snapping::Mark>,
    /// Whether the release sends `snap`.
    pub snap: bool,
}

impl DiacraftApp {
    /// The selected shapes of the page on view.
    pub fn selection(&self) -> &[u32] {
        self.ui.edit.selection()
    }

    /// The guide selected on the page on view, if one is.
    pub fn selected_guide_id(&self) -> Option<u32> {
        self.ui.edit.guide()
    }

    /// The `snap` the last drag's command answered with: what the engine snapped it to, and in `held` the shapes it holds to guides.
    pub fn last_snap(&self) -> Option<&Value> {
        self.ui.edit.last_snap()
    }

    /// Whether the shapes selected are all in one list (the page's, or one group's members): what `shape.group` takes. Shapes the scene does not
    /// know are left to the command to refuse.
    pub(crate) fn selection_in_one_list(&self) -> bool {
        let Some(scene) = &self.ui.edit.scene else { return true };
        let mut parents = self.ui.edit.selection.iter().filter_map(|id| scene.get(*id)).map(|item| item.parent);
        let Some(first) = parents.next() else { return true };
        parents.all(|parent| parent == first)
    }

    /// Whether every shape selected is a connector, and one is: the shapes a connector's right-click menu is for (`ribbon/spec.rs`, `CONNECTOR_MENU`).
    pub(crate) fn selection_is_connectors(&self) -> bool {
        let Some(scene) = &self.ui.edit.scene else { return false };
        let selected = &self.ui.edit.selection;
        !selected.is_empty() && selected.iter().all(|id| scene.get(*id).is_some_and(|item| item.kind == scene::Kind::Connector))
    }

    /// What kinds of shape are selected: `(connectors, others)`, whether the selection holds a connector and whether it holds any other shape. The Change Shape gallery lists the masters
    /// that fit (a connector is changed only to a connector, and any other shape only to a master that is not one). With no scene to ask, both: the engine refuses what does not fit.
    pub(crate) fn selection_kinds(&self) -> (bool, bool) {
        let Some(scene) = &self.ui.edit.scene else { return (true, true) };
        let mut kinds = (false, false);
        for item in self.ui.edit.selection.iter().filter_map(|id| scene.get(*id)) {
            if item.kind == scene::Kind::Connector {
                kinds.0 = true;
            } else {
                kinds.1 = true;
            }
        }
        if kinds == (false, false) { (true, true) } else { kinds }
    }

    /// Whether any shape selected is a group (what `shape.ungroup` takes apart). With no scene to ask it says yes and leaves the command to refuse.
    pub(crate) fn selection_has_group(&self) -> bool {
        let Some(scene) = &self.ui.edit.scene else { return true };
        self.ui.edit.selection.iter().filter_map(|id| scene.get(*id)).any(|item| item.kind == scene::Kind::Group)
    }

    /// The corners, in inches on the page, of the shape `id` on the page the scene holds: for what shows a shape (Find Next brings the match into view).
    pub(crate) fn scene_corners(&self, id: u32) -> Option<[geom::P; 4]> {
        self.ui.edit.scene.as_ref()?.get(id).map(|item| item.corners)
    }

    /// Selects `ids` on the page on view; an `ID` the page does not have is ignored.
    pub fn select(&mut self, ids: &[u32]) {
        let Some(page) = self.current_page() else { return };
        self.ensure_scene(page);
        let known: Vec<u32> = match &self.ui.edit.scene {
            Some(scene) => ids.iter().copied().filter(|id| scene.get(*id).is_some()).collect(),
            None => Vec::new(),
        };
        self.set_selection(page, dedup(known));
    }

    fn set_selection(&mut self, page: u32, ids: Vec<u32>) {
        self.ui.edit.selection = ids;
        self.ui.edit.selection_page = Some(page);
        self.ui.edit.guide = None;
    }

    /// Where the drag in progress would land if the button came up now: the move, the resize or the guide the preview shows (`None` when no such drag
    /// is on).
    pub fn drag_preview(&self) -> Option<DragPreview> {
        let edit = &self.ui.edit;
        let none = DragPreview {
            delta: None,
            sized: None,
            turned: None,
            corners: None,
            guide_at: None,
            held: Vec::new(),
            end: None,
            end_glued: None,
            leg_to: None,
            marks: Vec::new(),
            snap: false,
        };
        if let Some(guides::RulerDrag { at, .. }) = edit.ruler {
            return Some(DragPreview { guide_at: at, ..none });
        }
        let g = edit.gesture.as_ref().filter(|g| g.active && g.landing.worked_out())?;
        let l = &g.landing;
        let delta = matches!(g.mode, gesture::Mode::Move { .. }).then_some(l.delta);
        let held = match &g.mode {
            gesture::Mode::Guide { held, .. } => held.clone(),
            _ => Vec::new(),
        };
        let (end, end_glued) = match l.end {
            Some(gesture::Target::Free(p)) => (Some(p), None),
            Some(gesture::Target::Point { id, index, at }) => (Some(at), Some((id, index))),
            _ => (None, None),
        };
        Some(DragPreview {
            delta,
            sized: l.sized,
            turned: l.turn.map(|t| t.to),
            corners: l.corners,
            guide_at: l.guide_at,
            held,
            end,
            end_glued,
            leg_to: l.leg.map(|leg| leg.to),
            marks: l.marks.clone(),
            snap: l.snap,
        })
    }

    /// What the status bar says of the one selected shape: its width, height and angle, the size it is being
    /// dragged to while a handle is. `None` with nothing selected, or several shapes.
    pub fn selection_summary(&self) -> Option<String> {
        if let Some(words) = self.guide_summary() {
            return Some(words);
        }
        let item = self.single()?;
        let moving = self.ui.edit.gesture.as_ref().filter(|g| g.active);
        let (lo, hi) = match moving {
            Some(g) => match &g.mode {
                gesture::Mode::Resize { handle, basis, .. } => {
                    // The size along the shape's own axes, and the frame starts at (0, 0), so the size is the corner to corner distance.
                    let (lo, hi) = g.new_frame(*handle, *basis);
                    ((0.0, 0.0), (hi.0 - lo.0, hi.1 - lo.1))
                }
                _ => item.frame,
            },
            None => item.frame,
        };
        // While the rotation handle is dragged the status bar shows the angle the shape would be turned to.
        let angle = match moving.map(|g| &g.mode) {
            Some(gesture::Mode::Rotate { pin, grab, start, .. }) => {
                let cx = self.cx_of(self.ui.page?)?;
                moving.map_or(item.degrees, |g| g.turn_to(&cx, *pin, *grab, *start))
            }
            _ => item.degrees,
        };
        let degrees = if angle.abs() < 0.05 { 0.0 } else { angle.rem_euclid(360.0) };
        Some(format!("Width: {:.2} in   Height: {:.2} in   Angle: {degrees:.0}\u{b0}", hi.0 - lo.0, hi.1 - lo.1))
    }

    /// Whether a drag with the pointer is on (a move, a resize, a join, a rubber band, or a tile from the Shapes window).
    pub fn edit_dragging(&self) -> bool {
        self.ui.edit.dragging()
    }

    /// The screen point of the begin or the end of connector `id` as the last frame drew the page: where a handle is to be taken hold of.
    pub fn connector_end_point(&self, id: u32, end: bool) -> Option<Pos2> {
        let cx = self.cx_of(self.ui.page?)?;
        let (begin, last) = self.ui.edit.scene.as_ref()?.get(id)?.ends?;
        Some(cx.screen_at(if end { last } else { begin }))
    }

    /// The tool in use.
    pub fn tool(&self) -> Tool {
        self.ui.edit.tool
    }

    /// Changes the tool. A drag in progress is dropped.
    pub fn set_tool(&mut self, tool: Tool) {
        // A tool is a mode, and so is the Format Painter: choosing a tool puts the painter down (`painter.rs`).
        self.ui.edit.painter = None;
        if self.ui.edit.tool != tool {
            self.ui.edit.tool = tool;
            self.ui.edit.gesture = None;
        }
        if let Tool::Draw(pen) = tool {
            self.ui.edit.last_pen = Some(pen);
        }
    }

    /// Whether the control `command` is the tool in use (so its button is drawn pressed).
    pub(crate) fn is_current_tool(&self, command: Option<&str>) -> bool {
        match command {
            Some("tool.pointer") => self.ui.edit.tool == Tool::Pointer,
            Some("tool.connector") => self.ui.edit.tool == Tool::Connector,
            Some("tool.text") => self.ui.edit.tool == Tool::Text,
            Some("tool.text_block") => self.ui.edit.tool == Tool::TextBlock,
            Some("tool.area_select") => self.ui.edit.tool == Tool::AreaSelect,
            Some("tool.lasso_select") => self.ui.edit.tool == Tool::Lasso,
            Some("format.painter") => self.ui.edit.painter.is_some(),
            Some(id) => tool_of_command(id).is_some_and(|pen| self.ui.edit.tool == Tool::Draw(pen)),
            None => false,
        }
    }

    /// Forgets the selection, the gesture and the text box: a new drawing, a file, or another page.
    pub(crate) fn edit_reset(&mut self) {
        self.ui.edit.reset();
    }

    /// Makes the scene of `page` at the session's revision if the one held is older, and drops from the selection what is no longer there.
    /// → whether the page has a scene.
    pub(crate) fn ensure_scene(&mut self, page: u32) -> bool {
        let revision = self.session.revision();
        let fresh = self.ui.edit.scene.as_ref().is_some_and(|s| s.page == page && s.revision == revision);
        if !fresh {
            self.ui.edit.scene = self.session.document().and_then(|doc| Scene::build(doc, page, revision));
            self.prune_selection(page);
        }
        self.ui.edit.scene.is_some()
    }

    fn prune_selection(&mut self, page: u32) {
        if self.ui.edit.selection_page != Some(page) {
            self.ui.edit.selection.clear();
            self.ui.edit.selection_page = Some(page);
            self.ui.edit.text = None;
        }
        if let Some(scene) = &self.ui.edit.scene {
            // A shape whose layer was locked or hidden after it was picked is no longer selected.
            self.ui.edit.selection.retain(|id| scene.get(*id).is_some() && scene.selectable(*id));
            if self.ui.edit.text.as_ref().is_some_and(|t| t.new.is_none() && scene.get(t.id).is_none()) {
                self.ui.edit.text = None;
            }
        }
    }

    /// The selected item when exactly one shape is selected.
    fn single(&self) -> Option<&Item> {
        match self.ui.edit.selection.as_slice() {
            [only] => self.ui.edit.scene.as_ref()?.get(*only),
            _ => None,
        }
    }

    /// Runs a command on behalf of the pointer or keyboard, and tells the person if it fails. → its answer.
    fn run_edit(&mut self, id: &str, params: &Value) -> Option<Value> {
        match self.execute(id, params) {
            Ok(value) => Some(value),
            Err(e) => {
                self.set_notice(&e.to_string());
                None
            }
        }
    }

    /// Remembers where `ids` now are, so their outlines stay painted until the picture of the new revision is on the screen.
    pub(crate) fn keep_preview(&mut self, page: u32, ids: &[u32]) {
        self.ensure_scene(page);
        let revision = self.session.revision();
        let paths: Vec<Vec<P>> = match &self.ui.edit.scene {
            Some(scene) => paint::preview_items(scene, ids).into_iter().flat_map(paint::preview_paths).collect(),
            None => Vec::new(),
        };
        self.ui.edit.after = (!paths.is_empty()).then(|| After { page, revision, paths, since: self.clock.now() });
    }

    /// Drops the preview of an edit once the picture shows it (or has been late long enough).
    fn settle_preview(&mut self, ctx: &egui::Context, page: u32) {
        let Some(after) = &self.ui.edit.after else { return };
        let shown = self.ui.pictures.image(page).is_some_and(|image| image.revision >= after.revision);
        let stale = self.clock.now().saturating_sub(after.since) > PREVIEW_PATIENCE || after.page != page || after.revision > self.session.revision();
        if shown || stale {
            self.ui.edit.after = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }

    /// The drawing area's input and the text box, for one frame. Runs after the view is settled for the frame.
    pub(crate) fn edit_canvas(&mut self, ui: &Ui, response: &egui::Response, cx: &Cx) {
        let ctx = ui.ctx().clone();
        // The comments of the drawing, read once for each revision (`comments_ui.rs`): the markers are painted from them.
        self.comments_listing();
        // A question is on the screen, or a file is being read: the pointer is the card's, not the drawing's (L6 and L7 of the M3 review). A
        // drag that was on when the card came up is dropped, and not finished at the card's button.
        if self.is_modal_open() {
            self.ui.edit.gesture = None;
            self.ui.edit.tile = None;
            self.ui.edit.ruler = None;
            return;
        }
        // What the reader of the page has made ready since the last frame is taken before the drag is landed.
        self.poll_snap_reads(&ctx);
        let (pointer, pressed, down, released, mods) = ui.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                held_keys(i),
            )
        });
        let over = pointer.filter(|p| response.hovered() && cx.area.contains(*p));
        self.ui.edit.hover = over.map(|p| cx.page_at(p));
        let wants_scene = pressed && over.is_some()
            || self.ui.edit.gesture.is_some()
            || !self.ui.edit.selection.is_empty()
            || (matches!(self.ui.edit.tool, Tool::Connector | Tool::Text | Tool::TextBlock | Tool::Draw(_)) && over.is_some())
            || self.ui.edit.after.is_some()
            // View > Connection Points marks the points of the shapes in view, which the scene holds.
            || self.ui.show_connection_points
            // AutoConnect's arrows are on the shape the pointer rests on, which the scene finds.
            || (over.is_some() || self.ui.edit.tile.is_some()) && self.autoconnect_live();
        if wants_scene {
            self.ensure_scene(cx.page);
        }
        self.prune_guide(cx.page);
        self.settle_preview(&ctx, cx.page);
        self.refresh_block(cx.page);
        // The handles on the legs of the connector selected (`legs.rs`).
        self.legs_update(cx);
        // The yellow handle at the end of the leader of the callout selected (`callout.rs`).
        self.tip_update(cx);
        if let Some(unread) = self.ui.edit.scene.as_ref().map(|s| s.unread).filter(|n| *n > 0)
            && !self.ui.edit.told_unread
        {
            self.ui.edit.told_unread = true;
            self.set_notice(&format!(
                "This page has more shapes than the mouse can select yet; {unread} are left out. The commands still reach them."
            ));
        }
        // A right click on the page: the shape under it is selected if it is not, and the point is kept for Paste in the context menu.
        if response.secondary_clicked()
            && let Some(at) = ui.input(|i| i.pointer.latest_pos()).filter(|p| cx.area.contains(*p))
        {
            let p = cx.page_at(at);
            self.ui.edit.context = Some(p);
            if self.ui.edit.text.is_none() && self.ensure_scene(cx.page) {
                let hit = self.ui.edit.scene.as_ref().and_then(|s| s.hit(p, cx.reach(HIT_REACH))).map(|item| item.id);
                if let Some(id) = hit.filter(|id| !self.ui.edit.selection.contains(id)) {
                    self.set_selection(cx.page, vec![id]);
                }
            }
        }
        // The arrows of the shape the pointer rests on, and the flyout of masters (`autoconnect.rs`).
        self.auto_update(ui, cx, pointer, over);
        if self.ui.edit.text.is_some() {
            self.text_box_input(&ctx, cx, pressed, over);
        }
        // The places the pointer went through this frame, for a stroke (`draw.rs`).
        self.ui.edit.trail = if matches!(self.ui.edit.tool, Tool::Draw(_) | Tool::Lasso) {
            ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(at) = e { Some(*at) } else { None }).collect())
        } else {
            Vec::new()
        };
        if self.ui.edit.text.is_none() {
            self.canvas_pointer(&ctx, response, cx, &Pointer { latest: pointer, over, pressed, down, released, mods });
        }
        self.canvas_cursor(&ctx, cx, over);
    }

    fn canvas_pointer(&mut self, ctx: &egui::Context, response: &egui::Response, cx: &Cx, pointer: &Pointer) {
        // A press on a comment's marker opens the comment (`comments_ui.rs`): the shape under it is not picked, and no gesture begins.
        if pointer.pressed
            && self.ui.edit.tool == Tool::Pointer
            && let Some(at) = pointer.over
            && let Some(id) = self.comment_marker_at(cx, at)
        {
            let _ = self.execute("window.showComment", &serde_json::json!({ "comment": id }));
            return;
        }
        if pointer.pressed
            && let Some(at) = pointer.over
        {
            self.begin_gesture(cx, at, pointer.mods);
        }
        if let Some(at) = pointer.latest
            && self.ui.edit.gesture.is_some()
        {
            self.update_gesture(cx, at, pointer.mods);
            if pointer.released || !pointer.down {
                self.finish_gesture(cx, at, pointer.mods);
            } else {
                ctx.request_repaint();
            }
        }
        if response.double_clicked() && self.ui.edit.tool == Tool::Pointer {
            self.edit_text_at(cx, pointer.over);
        }
    }

    fn canvas_cursor(&self, ctx: &egui::Context, cx: &Cx, over: Option<Pos2>) {
        use egui::CursorIcon;
        let Some(at) = over else { return };
        if self.ui.edit.text.is_some() {
            return;
        }
        // The Format Painter has no cursor of its own in egui: the one that says "a copy goes here".
        if self.ui.edit.painter.is_some() {
            ctx.set_cursor_icon(CursorIcon::Copy);
            return;
        }
        if self.ui.edit.tool == Tool::Connector || matches!(self.ui.edit.tool, Tool::Draw(_) | Tool::AreaSelect | Tool::Lasso) {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
            return;
        }
        if self.ui.edit.tool == Tool::Text && self.ui.edit.gesture.is_none() {
            ctx.set_cursor_icon(CursorIcon::Text);
            return;
        }
        // The net's crosshair is the pointer from the press, not from the drag threshold.
        if let Some(gesture) = &self.ui.edit.gesture
            && (gesture.active || matches!(gesture.mode, gesture::Mode::Marquee { .. }))
        {
            ctx.set_cursor_icon(gesture.cursor());
            return;
        }
        let p = cx.page_at(at);
        if self.ui.edit.tool == Tool::TextBlock {
            ctx.set_cursor_icon(self.block_cursor(cx, p));
            return;
        }
        // An AutoConnect arrow that shows is pressed.
        if self.ui.edit.tool == Tool::Pointer
            && let Some(cursor) = self.auto_cursor(cx, at)
        {
            ctx.set_cursor_icon(cursor);
            return;
        }
        if let Some(cursor) = self.handle_cursor(cx, p) {
            ctx.set_cursor_icon(cursor);
        } else if self.ui.edit.scene.as_ref().is_some_and(|s| s.hit(p, cx.reach(HIT_REACH)).is_some()) {
            ctx.set_cursor_icon(CursorIcon::Move);
        } else if let Some(cursor) = self.guide_cursor(cx, p) {
            ctx.set_cursor_icon(cursor);
        }
    }

    /// The distance between two lines of the grid as the view draws it, on each axis, in paper inches: the step of an arrow key and of the places a
    /// click on a tile tries. The grid is the snap's (the drawing's units and scale); an axis with no line steps by an eighth of an inch.
    fn grid_pitch(&mut self, cx: &Cx) -> (f64, f64) {
        let [x, y] = self.grid_marks(cx.page, cx.view.scale());
        let pitch = |m: Option<diacraft_engine::snap::Marks>| m.map_or(0.125, |m| m.pitch);
        (pitch(x), pitch(y))
    }

    /// What the status bar says of the guide selected: which way it runs and where it is, as the ruler reads it.
    fn guide_summary(&self) -> Option<String> {
        let page = self.ui.page?;
        let id = self.ui.edit.guide?;
        let cx = self.cx_of(page)?;
        let key = (self.ui.generation, self.session.revision(), page);
        let line = self.ui.edit.snap_page.as_ref().filter(|c| c.key() == key)?.page.guides.iter().find(|g| g.id == id).copied()?;
        let s = &cx.settings;
        let (origin, which) =
            if line.axis == diacraft_engine::snap::Axis::X { (s.ruler_origin.0, "Vertical") } else { (s.ruler_origin.1, "Horizontal") };
        let value = (line.at / s.drawing_scale - origin) / s.ruler_unit.base_per_unit();
        value.is_finite().then(|| format!("{which} guide at {} {}", crate::canvas::grid::ruler_label(value), s.ruler_unit.code().to_lowercase()))
    }
}

/// The keys held: the ones egui was told of, and the ones the press itself carries (a press given by the control channel has them on the
/// event and sends no separate word of them).
fn held_keys(i: &egui::InputState) -> egui::Modifiers {
    let on_press = i.events.iter().rev().find_map(|e| match e {
        egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, modifiers, .. } => Some(*modifiers),
        _ => None,
    });
    on_press.map_or(i.modifiers, |m| m | i.modifiers)
}

/// The ids once each, in order.
fn dedup(ids: Vec<u32>) -> Vec<u32> {
    let mut seen = Vec::with_capacity(ids.len());
    for id in ids {
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

/// A JSON number for a length in inches, rounded to a millionth of an inch: arithmetic noise does not reach the document.
pub(crate) fn inch(v: f64) -> Value {
    json!((v * 1.0e6).round() / 1.0e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_sent_to_the_engine_are_rounded_to_a_millionth_of_an_inch() {
        assert_eq!(inch(0.1 + 0.2), json!(0.3));
        assert_eq!(inch(1.0 / 3.0), json!(0.333333));
    }

    #[test]
    fn dedup_keeps_the_first_of_each() {
        assert_eq!(dedup(vec![3, 1, 3, 2, 1]), vec![3, 1, 2]);
    }
}
