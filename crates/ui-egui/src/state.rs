//! What the window itself remembers: the tab that is open, which panels are folded, the view of each page.
//!
//! None of it is the document. It is the window's, so it is changed by the window's commands (`commands.rs`), not
//! by the engine's, and nothing here is saved: the View settings it draws from (the grid, the rulers, the guides, the connection points) are the drawing's
//! And are here as the drawing showed them when a command last ended (`window_settings.rs`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui::{Rect, Theme};

use crate::canvas::picture::Pictures;
use crate::canvas::view::View;
use crate::edit::EditState;
use crate::file_open::FilePicker;
use crate::stencils::{AvailableStencil, Figure, Scratch, StencilInfo};

/// The ribbon tab open when the window starts.
pub const FIRST_TAB: &str = "Home";

/// The pages of the File view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackstagePage {
    Home,
    New,
    Print,
}

impl BackstagePage {
    /// The page's name, as the rail shows it and as `window.backstage` takes it.
    pub const fn name(self) -> &'static str {
        match self {
            BackstagePage::Home => "Home",
            BackstagePage::New => "New",
            BackstagePage::Print => "Print",
        }
    }

    /// The page named `name`, in any case.
    pub fn from_name(name: &str) -> Option<BackstagePage> {
        [BackstagePage::Home, BackstagePage::New, BackstagePage::Print].into_iter().find(|p| p.name().eq_ignore_ascii_case(name))
    }
}

/// The window's own state.
pub struct UiState {
    /// The open ribbon tab, by its label.
    pub tab: String,
    /// The ribbon is folded to the tab row.
    pub ribbon_collapsed: bool,
    /// The Shapes window is folded to a strip.
    pub shapes_collapsed: bool,
    /// The grid shows (View, Show). **What the drawing shows** (`window_settings.rs`): on for a drawing that holds no windows part (a new one),
    /// its drawing window's `ShowGrid` for one that does, and off where that holds none. Turning the box writes it into the
    /// drawing, which a save keeps.
    pub show_grid: bool,
    /// The rulers show. As the grid: on with no windows part, the drawing window's `ShowRulers` with one, off where that holds none.
    pub show_rulers: bool,
    /// The guides show (View, Show, Guides). On with no windows part; the drawing window's `ShowGuides` with one, and on where that holds none.
    pub show_guides: bool,
    /// The connection points of the shapes in view are marked whatever the tool (View, Visual Aids, Connection Points). Off, they are marked only
    /// while the Connector tool is on or a connector's end is dragged, as before. Off with no windows part; the drawing window's `ShowConnectionPoints`
    /// with one, and off where that holds none.
    pub show_connection_points: bool,
    /// The program-wide "Enable AutoConnect" setting (File, Options, Advanced): **on at first**, as the specification says it is, and the window's
    /// own (not saved, like the Developer tab). Off, no drawing shows AutoConnect arrows and the View box is greyed out. The drawing's own box
    /// is the drawing's (`autoconnect.set`).
    pub enable_autoconnect: bool,
    /// The program-wide "Select shapes partially within area" setting (File, Options, Advanced): **off at first**, as the page states, and the window's own (not
    /// saved). On, a net or a loop takes the shapes it touches as well as those wholly inside it (`edit/region.rs`).
    pub select_partial: bool,
    /// The Select by Type dialog, while it is open (`select_by_type.rs`).
    pub select_by_type: Option<crate::select_by_type::SelectByTypeDialog>,
    /// What the Insert tab keeps: the picture chooser and the dialogs of Link, Field and Symbol (`insert_picture.rs`).
    pub insert: crate::insert_picture::InsertState,
    /// What the window keeps of comments (`comments_ui.rs`): the comment being typed, the one that is open, and the filter. Not saved.
    pub comments: crate::comments_ui::CommentsUi,
    /// What Change Shape says of the shapes it left as they were (`arrange_change_shape`), put in the status bar once the command that made it has ended.
    pub change_note: Option<String>,
    /// How much a Freeform stroke is smoothed before its curve is fitted, 0 to 1 (`window.freeformSmoothing`): the window's own setting, not saved. The specifications give no
    /// control for it (the Draw tab is the ink tab), so it is a window command, as the program-wide AutoConnect setting is.
    pub freeform_smoothing: f64,
    /// The windows part of the drawing when the window last showed its settings (`window_settings.rs`): a command that leaves another one (an undo of a box turned, a
    /// program's `window.settings.set`) has the window show the drawing's settings again. Compared by pointer.
    pub windows_seen: Option<Arc<diacraft_engine::doc::RawElement>>,
    /// The Developer tab is on (it is off until turned on).
    pub developer_tab: bool,
    /// The page the drawing area shows, by id.
    pub page: Option<u32>,
    /// The view of each page that has had one.
    pub views: HashMap<u32, View>,
    /// The File view, when it is open.
    pub backstage: Option<BackstagePage>,
    /// The theme the window follows.
    pub theme: Theme,
    /// How many drawings this window has made, for their names (`Drawing1`, `Drawing2`).
    pub new_count: u32,
    /// Which drawing the window shows: it moves whenever a drawing replaces the open one. **A session that is opened starts its revision again from 1**, so a
    /// cache that is kept for a revision of the drawing (the scene's neighbours: the format view, the panes' tree, picture and answers) is kept for a
    /// generation and a revision.
    pub generation: u64,
    /// The drawing area's rectangle on the last frame it was drawn, rulers excluded.
    pub canvas_rect: Option<Rect>,
    /// The page whose tab was last scrolled into view in the row of page tabs.
    pub tab_scrolled_to: Option<u32>,
    /// What the drawing area drew in the last frame.
    pub report: CanvasReport,
    /// The pictures of the pages: the render worker and the picture on the screen.
    pub pictures: Pictures,
    /// The tool, the selection and the drag in progress (`edit`).
    pub edit: EditState,
    /// The stencils the Shapes window lists and which one shows its masters.
    pub stencils: StencilState,
    /// What the formatting controls show of the selection, and the machine's fonts for the Font box (`format.rs`).
    pub format: crate::format::FormatState,
    /// The text a copy or a cut asked to put on the system clipboard, until the frame that has egui's context takes it (`clipboard.rs`).
    pub system_copy: Option<String>,
    /// Where the ribbon's body was drawn in the last frame: a press on it does not end the text box that is open (`edit/text.rs`).
    pub ribbon_body: Option<Rect>,
    /// The card of the folded ribbon group that is open: its id, and the number of the last pass that drew it (`ribbon/controls.rs`, `draw_fold`). It
    /// is open only while a pass draws it each time, so a card whose button is not drawn any more (another tab, a window with room for the group) is closed.
    pub fold_card: Option<(egui::Id, u64)>,
    /// The Print view's settings, its preview and the print under way (`print.rs`).
    pub print: crate::print::PrintState,
    /// What the page on view says of its setup, the Page Setup dialog and the Reorder Pages dialog (`page_setup.rs`).
    pub page_setup: crate::page_setup::PageSetupState,
    /// What the Design tab's galleries and Connectors menu show of the page on view (`ribbon/design.rs`).
    pub design: crate::ribbon::design::DesignState,
    /// What the Container Format tab shows of the selection (`container_ui.rs`).
    pub container: crate::container_ui::ContainerView,
    /// What the Cross-Functional Flowchart tab shows of the page and the selection (`swimlane_ui.rs`).
    pub swimlane: crate::swimlane_ui::SwimlaneView,
    /// What the Org Chart tab shows of the drawing and the selection (`org_ui.rs`).
    pub org: crate::org_ui::OrgView,
    /// The tiles of the Callout gallery, made the first time its menu opens (`ribbon/callouts.rs`).
    pub callouts: crate::ribbon::callouts::CalloutsState,
    /// What the Quick Styles gallery shows of the first shape selected (`format_fx.rs`).
    pub quick: crate::format_fx::QuickState,
    /// What the row of page tabs is doing with the pointer: a tab being dragged, a name being typed (`page_tabs.rs`).
    pub tabs: crate::page_tabs::TabsState,
    /// The docking panes at the right of the drawing area (`panes.rs`).
    pub panes: crate::panes::PanesState,
    /// The Text dialog, while it is open (`text_dialog.rs`).
    pub text_dialog: Option<crate::text_dialog::TextDialog>,
    /// The Layer Properties and Assign to Layer dialogs, while they are open (`layers_dialog.rs`).
    pub layers: crate::layers_dialog::LayersState,
    /// The Define Shape Data dialog and the question a drop asks, while they are open (`shape_data.rs`).
    pub shape_data: crate::shape_data::DataState,
    /// The ShapeSheet windows: the tabs of the sheet area, where it docks, the view toggles and the focus (`sheet/`).
    pub sheet: crate::sheet::SheetState,
    /// The Snap & Glue dialog, while it is open (`snap_dialog.rs`).
    pub snap_dialog: Option<crate::snap_dialog::SnapDialog>,
    /// The Zoom dialog, while it is open (`zoom_dialog.rs`).
    pub zoom_dialog: Option<crate::zoom_dialog::ZoomDialog>,
    /// The Configure Layout and Spacing Options dialogs, and the page breaks last read (`relayout_ui.rs`).
    pub relayout: crate::relayout_ui::RelayoutState,
    /// The Third-party licences window, while it is open (`licenses.rs`).
    pub licenses: Option<crate::licenses::LicensesWindow>,
    /// The Find or Replace dialog, while it is open, and the searches it keeps (`find_dialog.rs`).
    pub find: crate::find_dialog::FindState,
    /// The engine's answers to whether each shape operation can run on the shapes selected (`operations.rs`).
    pub shape_ops: crate::operations::OpsState,
    /// The Options dialog, while it is open (`options_dialog.rs`).
    pub options_dialog: Option<crate::options_dialog::OptionsDialog>,
    /// The heading of the File > New page on view, and the templates it lists (`new_page.rs`).
    pub new_page: crate::new_page::NewPageState,
    /// The splash popup or the About panel, while it is up (`splash.rs`).
    pub splash: Option<crate::splash::Splash>,
    /// The page's colour palette, read for a drawing, a revision and a page (`ribbon/palette.rs`).
    pub palette: Option<((u64, u64, u32), Arc<crate::ribbon::palette::PaletteView>)>,
    /// The Recent Colors of the palettes, newest first and ten at most: kept while the window is open.
    pub recent_colours: Vec<diacraft_engine::formatting::Rgb>,
    /// The Colors dialog (More Colors...), while it is open (`colors_dialog.rs`).
    pub colors_dialog: Option<crate::colors_dialog::ColorsDialog>,
    /// The theme or variant shown on the page while a tile is under the pointer (`theme_preview.rs`).
    pub theme_preview: crate::theme_preview::ThemePreview,
}

/// What the Shapes window remembers.
#[derive(Default)]
pub struct StencilState {
    /// The engine's `stencil.list`, read when the window first needs it and again when the stencils opened change.
    pub list: Option<Arc<Vec<StencilInfo>>>,
    /// The built-in stencils that were not open when `list` was read (`stencil.list`'s `available`): More Shapes offers them, and a choice opens one.
    pub available: Arc<Vec<AvailableStencil>>,
    /// The stencils that were open when `list` was read: the built-in ones that were (they change with a template), then the stencil files
    /// opened. A different set is a list to read again.
    pub listed_for: Vec<String>,
    /// The list was given (`with_stencil_list`) and is not read from the engine.
    pub fixed: bool,
    /// The id of the stencil whose masters show; the first when none is chosen.
    pub current: Option<String>,
    /// Local master-name filter, bounded to 128 Unicode characters by the shared UI/API setter.
    pub filter: String,
    /// The actual selected stencil when this filter was last synchronized (including fallback).
    pub filter_stencil: Option<String>,
    /// Focus from the last drawn filter, needed because egui drops it before an Escape frame.
    pub filter_focus: Option<egui::Id>,
    /// The filter owns this frame's keyboard, including the Escape that leaves it.
    pub filter_keys_held: bool,
    /// Reset the one gallery scroll state when the query or stencil changes.
    pub reset_scroll: bool,
    /// The figures read from masters' outlines, by [`crate::stencils::MasterInfo::key`] (`None`: the engine could not give one).
    pub figures: HashMap<String, Option<Arc<Figure>>>,
    /// The pictures of masters' icons, by the same key (`None`: the icon is not one the window reads).
    pub icons: HashMap<String, Option<egui::TextureHandle>>,
    /// The files the window opened stencils from, by the stencil's id: what a figure is drawn from again, and what is opened again in a drawing
    /// that is read from a file (the session is a new one then).
    pub files: Vec<(String, PathBuf)>,
    /// The sessions the figures of stencil files are drawn in, by the stencil's id (`None`: the file could not be opened again).
    pub scratch: HashMap<String, Option<Scratch>>,
    /// The stencils (by id) whose file was read again for their tiles while a read held the budget and was refused room: **not asked for again until no read holds the budget** (a
    /// stencil past the room it is given beside a read that runs was loaded and parsed on the window's thread for every tile in every frame).
    pub deferred: HashSet<String>,
    /// How many times a stencil file was read again to draw its tiles (`Scratch::open`): the work that a frame must not repeat while it is refused (for a test and `ui.inspect`).
    pub scratch_opens: usize,
    /// The chooser of the stencil file: the desktop app's native dialog (`with_stencil_picker`).
    pub picker: Option<FilePicker>,
    /// The dialog on its thread, until it answers.
    pub pick: Option<Receiver<Option<PathBuf>>>,
    /// A command asked for the chooser: the next frame shows it.
    pub want_dialog: bool,
    /// The results of the last search for shapes (`shapes.search`), kept as the stencil `search` until the box is cleared or another search is made.
    pub search: Option<crate::stencils::Found>,
    /// Whether a search takes any of the words (OR) and not all of them (AND, the default).
    pub search_any: bool,
    /// The stencil that was shown when the search began: where clearing the search goes back to.
    pub before_search: Option<String>,
    /// Where the Quick Shapes group and the rest of the masters stood in the last frame, for a master that is dragged across the divider.
    pub zones: Option<Zones>,
}

/// The two parts of a stencil's gallery, in points on the screen, and where each tile of the Quick Shapes group stood.
#[derive(Clone, Debug, PartialEq)]
pub struct Zones {
    /// The id of the stencil the gallery shows.
    pub stencil: String,
    /// Above the divider: the Quick Shapes group.
    pub quick: egui::Rect,
    /// Below it: the other masters.
    pub rest: egui::Rect,
    /// Each tile of the group, in order.
    pub tiles: Vec<egui::Rect>,
}

impl StencilState {
    /// Forgets the list that was read, so that the next frame reads it again: a new drawing has the stencils its template opened.
    pub fn forget_list(&mut self) {
        if !self.fixed {
            self.list = None;
            self.listed_for.clear();
        }
    }

    /// Forgets what was read of the stencils opened, so that the next frame reads it again.
    pub fn forget_opened(&mut self) {
        if !self.fixed {
            self.list = None;
            self.listed_for.clear();
        }
        self.figures.retain(|key, _| !key.contains('\u{1}'));
        self.icons.clear();
        self.scratch.clear();
        self.deferred.clear();
    }
}

/// What the drawing area drew in a frame: for tests and for `ui.inspect`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasReport {
    /// The page's rectangle on the screen, in points.
    pub page: Option<Rect>,
    /// How many grid lines were drawn, across and down.
    pub grid_lines: (usize, usize),
    /// Whether the rulers were drawn.
    pub rulers: bool,
    /// How many page break lines were drawn, across the page and down it.
    pub page_breaks: (usize, usize),
    /// The picture of the page that was laid over the grid, if there was one.
    pub picture: Option<PictureReport>,
    /// The picture is the preview of a theme or a variant under the pointer, drawn from a copy of the drawing (`theme_preview.rs`), and not the page's own.
    pub preview: bool,
}

/// The picture of a page, as the last frame laid it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PictureReport {
    /// Its size in pixels.
    pub size: [usize; 2],
    /// How many of its pixels are not clear.
    pub lit: usize,
    /// It was drawn within the short limits of a view that was moving, and is to be drawn again at rest.
    pub rough: bool,
}

/// The View tab's toggles a new drawing starts with (what a drawing with no windows part shows): the grid and the rulers on
/// (on for a new drawing), the guides on, the connection points off. The engine's
/// ([`diacraft_engine::view::NEW_DRAWING`]); a drawing that holds a windows part shows its own (`window_settings.rs`).
pub const NEW_DRAWING_VIEW: crate::window_settings::ViewSettings = crate::window_settings::NEW_DRAWING;

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tab: FIRST_TAB.to_owned(),
            ribbon_collapsed: false,
            shapes_collapsed: false,
            show_grid: NEW_DRAWING_VIEW.grid,
            show_rulers: NEW_DRAWING_VIEW.rulers,
            show_guides: NEW_DRAWING_VIEW.guides,
            show_connection_points: NEW_DRAWING_VIEW.connection_points,
            enable_autoconnect: true,
            select_partial: false,
            select_by_type: None,
            insert: crate::insert_picture::InsertState::default(),
            comments: crate::comments_ui::CommentsUi::default(),
            change_note: None,
            freeform_smoothing: diacraft_engine::draw::pen::DEFAULT_SMOOTHING,
            windows_seen: None,
            developer_tab: false,
            page: None,
            views: HashMap::new(),
            backstage: None,
            theme: Theme::Light,
            new_count: 0,
            generation: 0,
            canvas_rect: None,
            tab_scrolled_to: None,
            report: CanvasReport::default(),
            pictures: Pictures::new(),
            edit: EditState::default(),
            stencils: StencilState::default(),
            format: crate::format::FormatState::default(),
            system_copy: None,
            ribbon_body: None,
            fold_card: None,
            print: crate::print::PrintState::default(),
            page_setup: crate::page_setup::PageSetupState::default(),
            design: crate::ribbon::design::DesignState::default(),
            container: crate::container_ui::ContainerView::default(),
            swimlane: crate::swimlane_ui::SwimlaneView::default(),
            org: crate::org_ui::OrgView::default(),
            callouts: crate::ribbon::callouts::CalloutsState::default(),
            quick: crate::format_fx::QuickState::default(),
            tabs: crate::page_tabs::TabsState::default(),
            panes: crate::panes::PanesState::default(),
            text_dialog: None,
            layers: Default::default(),
            shape_data: Default::default(),
            sheet: Default::default(),
            snap_dialog: None,
            zoom_dialog: None,
            relayout: crate::relayout_ui::RelayoutState::default(),
            licenses: None,
            find: Default::default(),
            shape_ops: Default::default(),
            options_dialog: None,
            new_page: Default::default(),
            splash: None,
            palette: None,
            recent_colours: Vec::new(),
            colors_dialog: None,
            theme_preview: crate::theme_preview::ThemePreview::new(),
        }
    }
}
