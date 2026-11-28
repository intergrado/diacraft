//! Comments in the window: the Comments pane, the markers on the page, New Comment, Show Markup, and Previous and Next.
//!
//! **What the specifications say**: Review, Comments, **New Comment** adds a comment to the selected shape, or to the page when nothing is selected;
//! a comment on a shape stays with it when it moves and shows as an icon beside the shape (a page's comment, in a corner of the page); clicking the icon opens the comment, and the X on it deletes it; the **Comments
//! pane** keeps the comments and replies beside the drawing, where they can be read, added, edited and answered, in cards that show the author, the age, a delete X, the text and a reply box, headed by the text of
//! the shape each belongs to, under a **Filter by** list and a **New Comment** button; **Show Markup** shows or hides the markers; Previous and Next step through the comments. **Comments are not printed**: the
//! markers are painted by this window over the page's picture and are no part of what print, export or the pictures draw.
//!
//! **Every change is a command** (`comment.add`, `comment.reply`, `comment.edit`, `comment.delete`, `comment.markers`, `comment.author`): this file reads `comment.list` and sends the others. What is only the window's
//! is not a command: the text being typed, which card is open, and the filter.
//!
//! **Where the specifications are silent, and what is chosen** (each is on the check list):
//! * **The marker** is a small speech bubble in the accent colour with the author's initial, at the top right corner of the shape (a page's comment: inside the page's top right corner); the one that is open has a ring.
//!   A thread is one marker. It is drawn only while Show Markup is on.
//! * **The age** is the date and time of the comment as it was written, in UTC (`2031-03-07 20:05`), and not "21 minutes ago": the window keeps no wall clock of its own.
//! * **The filter** has two values, All and This page (the specification names a Filter by list and its value "All" and no other).
//! * **Previous and Next** are in the menu of the Comments button and go to the comment's page and select its shape, as `comment.next` and `comment.previous` say.
//! * **The Comments button** at the right of the title bar opens and closes the pane.

use egui::{Align2, CornerRadius, Key, Modifiers, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::edit::Cx;
use crate::panes::Pane;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};

/// The longest text a comment's box takes, in bytes: the engine's cap, so that what is typed is what can be posted.
const TEXT_MAX: usize = diacraft_engine::doc::MAX_COMMENT_TEXT_BYTES;
/// The most comments the window reads at a time: the engine's default listing.
const LIST_MAX: usize = 1000;
/// The size of a marker's bubble, in points.
const MARKER: f32 = 20.0;

/// One comment as `comment.list` gave it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub id: u32,
    pub parent: Option<u32>,
    pub page: u32,
    pub shape: Option<u32>,
    pub author: String,
    pub initials: String,
    pub text: String,
    pub date: String,
    pub edited: bool,
    pub done: bool,
    /// Where its marker is, in paper inches.
    pub anchor: (f64, f64),
    /// The head of its card: the start of the shape's text.
    pub head: Option<String>,
    /// Its shape is not on the page.
    pub gone: bool,
}

/// The comments of the drawing as they were read for one revision of it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Listing {
    generation: u64,
    revision: u64,
    rows: Vec<Row>,
    markers: bool,
    more: bool,
}

/// A comment being written.
#[derive(Clone, Debug, Default)]
struct Compose {
    page: u32,
    shape: Option<u32>,
    text: String,
    /// The box takes the keyboard on its first frame.
    fresh: bool,
}

/// What the window keeps of comments: not in the drawing, and not saved.
#[derive(Default)]
pub struct CommentsUi {
    listing: Option<Listing>,
    composing: Option<Compose>,
    /// The comment that is open: the card is marked, and its marker has a ring.
    pub(crate) open: Option<u32>,
    /// A reply being written: the comment it answers, and the text.
    reply: Option<(u32, String)>,
    /// A text being changed: the comment and the new text.
    edit: Option<(u32, String)>,
    /// The filter: the comments of the page on view only.
    this_page: bool,
    /// The author's name and initials being changed.
    author: Option<(String, String)>,
    /// The card to bring into view on the next frame.
    scroll_to: Option<u32>,
    /// The reply or edit box that was just opened takes the keyboard.
    focus_next: bool,
    /// The pass in which a box of the pane held the keyboard: it had the focus, or an Escape took it (the drawing's keys leave Escape to the box, which cancels).
    keys_pass: Option<u64>,
}

/// The age text of a date written `2031-03-07T20:05:09Z`: `2031-03-07 20:05`.
fn age_of(date: &str) -> String {
    let (day, time) = date.split_once('T').unwrap_or((date, ""));
    let time: String = time.chars().take(5).collect();
    if time.is_empty() { day.to_owned() } else { format!("{day} {time}") }
}

fn row_of(v: &Value) -> Option<Row> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let num = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    let anchor = v.get("anchor");
    let at = |k: &str| anchor.and_then(|a| a.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
    Some(Row {
        id: num("id")?,
        parent: num("parent"),
        page: num("page")?,
        shape: num("shape"),
        author: text("author"),
        initials: text("initials"),
        text: text("text"),
        date: age_of(v.get("date").and_then(Value::as_str).unwrap_or("")),
        edited: v.get("edit_date").is_some_and(|d| !d.is_null()),
        done: v.get("done").and_then(Value::as_bool).unwrap_or(false),
        anchor: (at("x"), at("y")),
        head: v.get("shape_text").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned),
        gone: v.get("shape_gone").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// A button with text: the pane's own look, from the tokens.
pub(crate) fn text_button(ui: &mut Ui, t: &Tokens, label: &str, enabled: bool, name: &str) -> egui::Response {
    let width = widgets::text_width(ui, label, &theme::font(size::LABEL)) + 18.0;
    let (r, resp) = ui.allocate_exact_size(vec2(width, 24.0), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, label);
    paint_button_bg(ui, r, &resp, enabled, false, t, 4);
    ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, theme::font(size::LABEL), label_colour(t, enabled));
    let _ = name;
    resp
}

/// How many markers are drawn one under another where comments share an anchor: the rest of them are counted on the last (a badge).
const MAX_STACK: usize = 3;
/// The most markers a frame draws.
const MAX_MARKERS: usize = 500;

/// A marker: the thread it opens, where it is on the screen, and how many more threads are at its anchor that have none of their own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Place<'a> {
    pub row: &'a Row,
    pub at: Pos2,
    pub more: usize,
}

/// Where the markers of the thread roots of the page `page` go on the screen `area` (`screen_at` turns a place of the page, in paper inches, into a point of it).
///
/// **One pass, and every marker is where it is by arithmetic:** a root whose anchor or screen place is not a finite number, or is off the area, has no marker (a file's anchor can be anywhere, and a step
/// that cannot change a number never ends); the roots that share a cell of the marker's size are counted by one table, and the i-th of a cell is `i` markers' heights below the first, as far as
/// [`MAX_STACK`], the last of which carries the count of the others. No more than [`MAX_MARKERS`] markers are made. The work is counted (`work.rs`): a look at each row and at most two probes of the table each.
pub(crate) fn places<'a>(rows: &'a [Row], page: u32, area: Rect, screen_at: impl Fn((f64, f64)) -> Pos2) -> Vec<Place<'a>> {
    let visible = area.expand(MARKER);
    // The roots that have a marker, each with its cell and place; then how many there are in each cell.
    let mut found: Vec<(&Row, (i32, i32), Pos2)> = Vec::new();
    let mut totals: std::collections::HashMap<(i32, i32), usize> = std::collections::HashMap::new();
    for row in rows.iter().filter(|r| r.page == page && r.parent.is_none()) {
        crate::work::count(|w| w.comment_rows_seen += 1);
        let at = screen_at(row.anchor) + vec2(MARKER * 0.5, -MARKER * 0.5);
        if !(at.x.is_finite() && at.y.is_finite()) || !visible.contains(at) {
            continue;
        }
        // The cell is in the range of the area, so the casts cannot overflow.
        let cell = ((at.x / MARKER).floor() as i32, (at.y / MARKER).floor() as i32);
        crate::work::count(|w| w.comment_cells_probed += 1);
        *totals.entry(cell).or_insert(0) += 1;
        found.push((row, cell, at));
    }
    let mut next: std::collections::HashMap<(i32, i32), usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (row, cell, at) in found {
        crate::work::count(|w| w.comment_cells_probed += 1);
        let n = next.entry(cell).or_insert(0);
        let i = *n;
        *n += 1;
        if i >= MAX_STACK || out.len() >= MAX_MARKERS {
            continue;
        }
        let total = totals.get(&cell).copied().unwrap_or(1);
        let more = if i + 1 == MAX_STACK { total.saturating_sub(MAX_STACK) } else { 0 };
        out.push(Place { row, at: at + vec2(0.0, MARKER * i as f32), more });
    }
    out
}

impl DiacraftApp {
    /// The comments as the drawing has them now: made again when the drawing is another revision of another generation, and **by one `comment.list`** (at most a thousand). A drawing with none
    /// costs no command.
    pub(crate) fn comments_listing(&mut self) -> &Listing {
        let (generation, revision) = (self.ui.generation, self.session.revision());
        let fresh = self.ui.comments.listing.as_ref().is_some_and(|l| l.generation == generation && l.revision == revision);
        if !fresh {
            let mut listing = Listing { generation, revision, markers: true, ..Listing::default() };
            if let Some(doc) = self.session.document() {
                listing.markers = doc.comments.show_tags.unwrap_or(true);
                if !doc.comments.is_empty() {
                    let params = json!({ "details": true, "limit": LIST_MAX });
                    if let Ok(answer) = self.session.execute("comment.list", &params) {
                        if let Some(Value::Array(items)) = answer.get("comments") {
                            listing.rows = items.iter().filter_map(row_of).collect();
                        }
                        listing.more = answer.get("more").and_then(Value::as_bool).unwrap_or(false);
                    }
                }
            }
            self.ui.comments.listing = Some(listing);
        }
        self.ui.comments.listing.get_or_insert_with(Listing::default)
    }

    /// Whether the markers of the comments show (Show Markup).
    pub(crate) fn comment_markers_on(&self) -> bool {
        self.session.document().is_none_or(|d| d.comments.show_tags.unwrap_or(true))
    }

    /// How many comments the drawing has (replies included), for the title bar's button.
    pub(crate) fn comment_count(&self) -> usize {
        self.session.document().map_or(0, |d| d.comments.len())
    }

    // ---------------------------------------------------------------------------------------------------------- window commands

    /// `window.newComment`: opens the pane with a box for a comment on the shape selected, or on the page when none or several are.
    pub(crate) fn new_comment_command(&mut self, shape: Option<u32>, page: Option<u32>) -> diacraft_engine::Result<()> {
        let Some(on_view) = self.current_page() else {
            return Err(diacraft_engine::EngineError::Disabled { cmd: "window.newComment".to_owned(), reason: "no document open".to_owned() });
        };
        let page = page.unwrap_or(on_view);
        let shape = shape.or_else(|| match self.selection() {
            [one] if self.ui.edit.selection_page() == Some(page) => Some(*one),
            _ => None,
        });
        self.ui.comments.composing = Some(Compose { page, shape, text: String::new(), fresh: true });
        self.ui.comments.reply = None;
        self.ui.comments.edit = None;
        self.ui.panes.open(Pane::Comments);
        Ok(())
    }

    /// `window.showComment`: shows the comment: its page, its shape selected, its card open and in view, and the pane open.
    pub(crate) fn show_comment_command(&mut self, id: u32) -> diacraft_engine::Result<()> {
        let row = self.comments_listing().rows.iter().find(|r| r.id == id).cloned();
        let Some(row) = row else {
            return Err(diacraft_engine::EngineError::BadParams { cmd: "window.showComment".to_owned(), msg: format!("no comment has id {id}") });
        };
        let root = row.parent.unwrap_or(row.id);
        if self.current_page() != Some(row.page) && self.session.document().is_some_and(|d| d.page(row.page).is_some()) {
            self.execute("page.select", &json!({ "id": row.page }))?;
        }
        if let Some(shape) = row.shape.filter(|_| !row.gone) {
            self.select(&[shape]);
        }
        self.ui.comments.open = Some(root);
        self.ui.comments.scroll_to = Some(root);
        self.ui.panes.open(Pane::Comments);
        Ok(())
    }

    /// `window.stepComment`: goes to the next or the previous comment, from the one that is open.
    pub(crate) fn step_comment_command(&mut self, forward: bool) -> diacraft_engine::Result<()> {
        let id = if forward { "comment.next" } else { "comment.previous" };
        let params = match self.ui.comments.open {
            Some(from) => json!({ "from": from }),
            None => json!({}),
        };
        let answer = self.session.execute(id, &params)?;
        match answer.get("comment").and_then(|c| c.get("id")).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
            Some(found) => self.show_comment_command(found),
            None => {
                self.set_notice("This drawing has no comments.");
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------- the markers

    /// Paints the markers of the comments of the page `cx.page` over the picture. Nothing when Show Markup is off.
    pub(crate) fn paint_comment_markers(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let Some(listing) = self.ui.comments.listing.as_ref().filter(|l| l.markers && l.revision == self.session.revision()) else { return };
        for place in self.marker_places(listing, cx) {
            self.paint_marker(painter, t, place.row, place.at);
            // The threads that did not get a marker of their own, counted on the last of the stack.
            if place.more > 0 {
                let badge = place.at + vec2(MARKER * 0.5, -MARKER * 0.5);
                painter.circle_filled(badge, 7.0, t.text);
                painter.text(badge, Align2::CENTER_CENTER, format!("+{}", place.more.min(99)), theme::font(8.0), t.panel_fill);
            }
        }
    }

    /// The thread roots on the page, each with where its marker is on the screen: [`places`].
    fn marker_places<'a>(&self, listing: &'a Listing, cx: &Cx) -> Vec<Place<'a>> {
        places(&listing.rows, cx.page, cx.area, |p| cx.screen_at(p))
    }

    fn paint_marker(&self, painter: &Painter, t: &Tokens, row: &Row, at: Pos2) {
        let r = Rect::from_center_size(at, vec2(MARKER, MARKER * 0.8));
        let open = self.ui.comments.open == Some(row.id);
        let fill = if row.done { t.icon_fill_disabled } else { t.accent };
        painter.rect_filled(r, CornerRadius::same(5), fill);
        // The tail, at the lower left.
        let base = r.left_bottom() + vec2(4.0, -1.0);
        painter.add(Shape::convex_polygon(vec![base, base + vec2(5.0, 0.0), base + vec2(1.0, 4.5)], fill, Stroke::NONE));
        let initial: String = row.initials.chars().next().map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
        painter.text(r.center(), Align2::CENTER_CENTER, initial, theme::font(size::GROUP), t.accent_text);
        if open {
            painter.rect_stroke(r.expand(2.0), CornerRadius::same(6), Stroke::new(2.0, t.text), StrokeKind::Outside);
        }
    }

    /// The thread whose marker is under the screen point `at`, if one is.
    pub(crate) fn comment_marker_at(&self, cx: &Cx, at: Pos2) -> Option<u32> {
        let listing = self.ui.comments.listing.as_ref().filter(|l| l.markers && l.revision == self.session.revision())?;
        self.marker_places(listing, cx)
            .into_iter()
            .rev()
            .find(|p| Rect::from_center_size(p.at, vec2(MARKER + 4.0, MARKER)).contains(at))
            .map(|p| p.row.id)
    }

    // ---------------------------------------------------------------------------------------------------------- the pane

    pub(crate) fn comments_body(&mut self, ui: &mut Ui, t: &Tokens) {
        if self.session.document().is_none() {
            ui.add(egui::Label::new(RichText::new("No drawing open.").font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            return;
        }
        let on_view = self.current_page();
        self.comments_listing();
        let listing = self.ui.comments.listing.clone().unwrap_or_default();
        // The row of the buttons: New Comment, Previous, Next.
        ui.horizontal(|ui| {
            let new = text_button(ui, t, "New Comment", true, "comments.new");
            self.note("comments.new", new.rect, true);
            if new.clicked() {
                let _ = self.execute("window.newComment", &json!({}));
            }
            for (label, forward, key) in [("Previous", false, "comments.previous"), ("Next", true, "comments.next")] {
                let enabled = !listing.rows.is_empty();
                let b = text_button(ui, t, label, enabled, key);
                self.note(key, b.rect, enabled);
                if b.clicked() {
                    let _ = self.execute("window.stepComment", &json!({ "forward": forward }));
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter by").font(theme::font(size::LABEL)).color(t.text_dim));
            for (label, this) in [("All", false), ("This page", true)] {
                let on = self.ui.comments.this_page == this;
                let (r, resp) = ui.allocate_exact_size(vec2(widgets::text_width(ui, label, &theme::font(size::LABEL)) + 16.0, 22.0), Sense::click());
                widgets::name_toggle(&resp, true, on, label);
                if on {
                    ui.painter().rect_filled(r, CornerRadius::same(4), t.accent_soft);
                } else {
                    paint_button_bg(ui, r, &resp, true, false, t, 4);
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, label, theme::font(size::LABEL), label_colour(t, true));
                self.note(format!("comments.filter.{}", if this { "page" } else { "all" }), r, true);
                if resp.clicked() {
                    self.ui.comments.this_page = this;
                }
            }
        });
        ui.add_space(4.0);
        let mut markers = listing.markers;
        let box_resp = ui.checkbox(&mut markers, RichText::new("Show Markup").font(theme::font(size::LABEL)).color(t.text));
        self.note("comments.markup", box_resp.rect, true);
        if box_resp.changed() {
            let _ = self.execute("comment.markers", &json!({ "show": markers }));
        }
        ui.add_space(6.0);
        self.compose_box(ui, t);
        // The cards.
        let roots: Vec<&Row> =
            listing.rows.iter().filter(|r| r.parent.is_none() && (!self.ui.comments.this_page || Some(r.page) == on_view)).collect();
        if roots.is_empty() && self.ui.comments.composing.is_none() {
            let words = if listing.rows.is_empty() {
                "No comments yet. New Comment adds one to the shape selected, or to the page."
            } else {
                "No comments on this page."
            };
            ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
        }
        for root in roots {
            self.card(ui, t, root, &listing.rows);
        }
        if listing.more {
            ui.add(
                egui::Label::new(
                    RichText::new("More comments than the pane lists. The commands reach them all.").font(theme::font(size::LABEL)).color(t.text_dim),
                )
                .wrap(),
            );
        }
        ui.add_space(8.0);
        self.author_line(ui, t);
    }

    /// Whether a box of the Comments pane holds the keyboard now: in this pass or the one before, so that an Escape that took the focus from it as the pass began is the box's
    /// and not the drawing's (which would clear the selection). Found the Escape was consumed before the pane saw it.
    pub(crate) fn comment_keys_held(&self, ctx: &egui::Context) -> bool {
        self.ui.comments.keys_pass.is_some_and(|pass| ctx.cumulative_pass_nr().saturating_sub(pass) <= 1)
    }

    /// Notes that a box of the pane holds the keyboard, and says whether an Escape ended its edit in this pass (`escape` was read before the box was added: the box takes the key
    /// for itself while it has the focus, and egui takes the focus from it as the pass begins).
    fn box_keys(&mut self, ui: &Ui, edit: &egui::Response, escape: bool) -> bool {
        let escaped = escape && edit.lost_focus();
        if edit.has_focus() || escaped {
            self.ui.comments.keys_pass = Some(ui.ctx().cumulative_pass_nr());
        }
        escaped
    }

    /// The box a new comment is typed in, with Post and Cancel.
    fn compose_box(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(mut compose) = self.ui.comments.composing.take() else { return };
        let target = match compose.shape {
            Some(id) => format!("New comment on shape {id}"),
            None => "New comment on the page".to_owned(),
        };
        ui.label(RichText::new(target).font(theme::font(size::LABEL)).color(t.text_dim));
        let id = ui.scope_id().with("comments_compose");
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let edit = ui.add(
            TextEdit::multiline(&mut compose.text)
                .id(id)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(TEXT_MAX)
                .font(theme::font(size::LABEL))
                .hint_text("Write a comment"),
        );
        self.note("comments.compose", edit.rect, true);
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "New comment"));
        if compose.fresh {
            edit.request_focus();
            compose.fresh = false;
        }
        let escaped = self.box_keys(ui, &edit, escape);
        let post_keys = edit.has_focus() && ui.input_mut(|i| crate::shortcuts::consume_exact(i, Modifiers::COMMAND, Key::Enter));
        let can_post = !compose.text.trim().is_empty();
        let (mut post, mut cancel) = (post_keys && can_post, escaped);
        ui.horizontal(|ui| {
            let b = text_button(ui, t, "Post", can_post, "comments.post");
            self.note("comments.post", b.rect, can_post);
            post |= b.clicked();
            let c = text_button(ui, t, "Cancel", true, "comments.cancel");
            self.note("comments.cancel", c.rect, true);
            cancel |= c.clicked();
        });
        ui.add_space(6.0);
        if post {
            let mut params = json!({ "text": compose.text, "page": compose.page });
            if let (Some(shape), Some(map)) = (compose.shape, params.as_object_mut()) {
                map.insert("id".to_owned(), json!(shape));
            }
            match self.execute("comment.add", &params) {
                Ok(answer) => {
                    if let Some(id) = answer.get("comment").and_then(|c| c.get("id")).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
                        self.ui.comments.open = Some(id);
                        self.ui.comments.scroll_to = Some(id);
                    }
                }
                Err(e) => {
                    self.set_notice(&e.to_string());
                    self.ui.comments.composing = Some(compose);
                }
            }
        } else if !cancel {
            self.ui.comments.composing = Some(compose);
        }
    }

    /// One thread: the card of a comment and its replies.
    fn card(&mut self, ui: &mut Ui, t: &Tokens, root: &Row, all: &[Row]) {
        let open = self.ui.comments.open == Some(root.id);
        let frame = egui::Frame::new()
            .fill(if open { t.accent_soft } else { t.tile_fill })
            .stroke(Stroke::new(if open { 2.0 } else { 1.0 }, if open { t.accent } else { t.tile_border }))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(8));
        let mut opened = false;
        let out = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The head: the text of the shape the comment is on, or where it is.
            let head = match (root.shape, root.gone, &root.head) {
                (None, _, _) => "Page".to_owned(),
                (Some(id), true, _) => format!("Shape {id} is not on the page"),
                (Some(_), false, Some(text)) => text.clone(),
                (Some(id), false, None) => format!("Shape {id}"),
            };
            // The head opens the comment: its page, its shape selected, its marker ringed. (The whole card is not a click, or its buttons would not get theirs.)
            let head_resp =
                ui.add(egui::Label::new(RichText::new(head).font(theme::font(size::GROUP)).color(t.text_dim)).truncate().sense(Sense::click()));
            self.note(format!("comments.head.{}", root.id), head_resp.rect, true);
            if head_resp.clicked() {
                opened = true;
            }
            self.entry(ui, t, root, false);
            for reply in all.iter().filter(|r| r.parent == Some(root.id)) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| self.entry(ui, t, reply, true));
                });
            }
            // The reply box, or its button.
            if self.ui.comments.reply.as_ref().is_some_and(|(to, _)| *to == root.id) {
                self.reply_box(ui, t, root.id);
            } else {
                let b = text_button(ui, t, "Reply", true, "comments.reply");
                self.note(format!("comments.reply.{}", root.id), b.rect, true);
                if b.clicked() {
                    self.ui.comments.reply = Some((root.id, String::new()));
                    self.ui.comments.edit = None;
                    self.ui.comments.focus_next = true;
                }
            }
        });
        self.note(format!("comments.card.{}", root.id), out.response.rect, true);
        if self.ui.comments.scroll_to == Some(root.id) {
            ui.scroll_to_rect(out.response.rect, None);
            self.ui.comments.scroll_to = None;
        }
        if opened && !open {
            let _ = self.execute("window.showComment", &json!({ "comment": root.id }));
        }
        ui.add_space(6.0);
    }

    /// The author line of a comment or reply, its text (or the box it is changed in), and Edit and the delete X.
    fn entry(&mut self, ui: &mut Ui, t: &Tokens, row: &Row, is_reply: bool) {
        ui.horizontal(|ui| {
            let (chip, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            ui.painter().circle_filled(chip.center(), 10.0, t.accent);
            let initial: String = row.initials.chars().next().map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
            ui.painter().text(chip.center(), Align2::CENTER_CENTER, initial, theme::font(size::GROUP), t.accent_text);
            ui.vertical(|ui| {
                ui.add(egui::Label::new(RichText::new(&row.author).font(theme::font(size::LABEL)).color(t.text).strong()).truncate());
                let when = if row.edited { format!("{} (edited)", row.date) } else { row.date.clone() };
                ui.label(RichText::new(when).font(theme::font(size::GROUP)).color(t.text_dim));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                widgets::name_button(&resp, true, if is_reply { "Delete reply" } else { "Delete comment" });
                paint_button_bg(ui, r, &resp, true, false, t, 4);
                let c = r.center();
                let stroke = Stroke::new(1.5, t.text);
                ui.painter().line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], stroke);
                ui.painter().line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], stroke);
                self.note(format!("comments.delete.{}", row.id), r, true);
                if resp.clicked() {
                    if self.ui.comments.open == Some(row.id) {
                        self.ui.comments.open = None;
                    }
                    if let Err(e) = self.execute("comment.delete", &json!({ "comment": row.id })) {
                        self.set_notice(&e.to_string());
                    }
                }
            });
        });
        if self.ui.comments.edit.as_ref().is_some_and(|(id, _)| *id == row.id) {
            self.edit_box(ui, t, row.id);
            return;
        }
        ui.add(egui::Label::new(RichText::new(&row.text).font(theme::font(size::LABEL)).color(label_colour(t, !row.done))).wrap());
        let b = text_button(ui, t, "Edit", true, "comments.edit");
        self.note(format!("comments.edit.{}", row.id), b.rect, true);
        if b.clicked() {
            self.ui.comments.edit = Some((row.id, row.text.clone()));
            self.ui.comments.reply = None;
            self.ui.comments.focus_next = true;
        }
    }

    fn edit_box(&mut self, ui: &mut Ui, t: &Tokens, id: u32) {
        let Some((_, mut text)) = self.ui.comments.edit.take() else { return };
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let edit = ui.add(
            TextEdit::multiline(&mut text)
                .id(ui.scope_id().with(("comments_edit", id)))
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(TEXT_MAX)
                .font(theme::font(size::LABEL)),
        );
        let escaped = self.box_keys(ui, &edit, escape);
        self.note(format!("comments.editbox.{id}"), edit.rect, true);
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Comment text"));
        if std::mem::take(&mut self.ui.comments.focus_next) {
            edit.request_focus();
        }
        let can = !text.trim().is_empty();
        let (mut save, mut cancel) = (false, escaped);
        ui.horizontal(|ui| {
            let s = text_button(ui, t, "Save", can, "comments.save");
            self.note("comments.save", s.rect, can);
            save = s.clicked();
            let c = text_button(ui, t, "Cancel", true, "comments.cancel_edit");
            self.note("comments.cancel_edit", c.rect, true);
            cancel |= c.clicked();
        });
        if save {
            if let Err(e) = self.execute("comment.edit", &json!({ "comment": id, "text": text })) {
                self.set_notice(&e.to_string());
                self.ui.comments.edit = Some((id, text));
            }
        } else if !cancel {
            self.ui.comments.edit = Some((id, text));
        }
    }

    fn reply_box(&mut self, ui: &mut Ui, t: &Tokens, to: u32) {
        let Some((_, mut text)) = self.ui.comments.reply.take() else { return };
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let edit = ui.add(
            TextEdit::multiline(&mut text)
                .id(ui.scope_id().with(("comments_reply", to)))
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .char_limit(TEXT_MAX)
                .font(theme::font(size::LABEL))
                .hint_text("Reply"),
        );
        let escaped = self.box_keys(ui, &edit, escape);
        self.note(format!("comments.replybox.{to}"), edit.rect, true);
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Reply"));
        if std::mem::take(&mut self.ui.comments.focus_next) {
            edit.request_focus();
        }
        let can = !text.trim().is_empty();
        let (mut post, mut cancel) = (false, escaped);
        ui.horizontal(|ui| {
            let p = text_button(ui, t, "Post", can, "comments.reply_post");
            self.note("comments.reply_post", p.rect, can);
            post = p.clicked();
            let c = text_button(ui, t, "Cancel", true, "comments.reply_cancel");
            self.note("comments.reply_cancel", c.rect, true);
            cancel |= c.clicked();
        });
        if post {
            if let Err(e) = self.execute("comment.reply", &json!({ "comment": to, "text": text })) {
                self.set_notice(&e.to_string());
                self.ui.comments.reply = Some((to, text));
            }
        } else if !cancel {
            self.ui.comments.reply = Some((to, text));
        }
    }

    /// "Commenting as": the name and initials new comments are given, and the way to change them (`comment.author`).
    fn author_line(&mut self, ui: &mut Ui, t: &Tokens) {
        let current = self.session.comment_author();
        if let Some((mut name, mut initials)) = self.ui.comments.author.take() {
            ui.label(RichText::new("Commenting as").font(theme::font(size::LABEL)).color(t.text_dim));
            let name_box =
                ui.add(TextEdit::singleline(&mut name).desired_width(f32::INFINITY).char_limit(255).font(theme::font(size::LABEL)).hint_text("Name"));
            name_box.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Author name"));
            let initials_box =
                ui.add(TextEdit::singleline(&mut initials).desired_width(80.0).char_limit(8).font(theme::font(size::LABEL)).hint_text("Initials"));
            initials_box.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Author initials"));
            let (mut ok, mut cancel) = (false, false);
            ui.horizontal(|ui| {
                let s = text_button(ui, t, "OK", !name.trim().is_empty(), "comments.author_ok");
                ok = s.clicked();
                let c = text_button(ui, t, "Cancel", true, "comments.author_cancel");
                cancel = c.clicked();
            });
            if ok {
                let mut params = json!({ "name": name });
                if let (false, Some(map)) = (initials.trim().is_empty(), params.as_object_mut()) {
                    map.insert("initials".to_owned(), json!(initials));
                }
                if let Err(e) = self.execute("comment.author", &params) {
                    self.set_notice(&e.to_string());
                    self.ui.comments.author = Some((name, initials));
                }
            } else if !cancel {
                self.ui.comments.author = Some((name, initials));
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Commenting as {} ({})", current.0, current.1)).font(theme::font(size::LABEL)).color(t.text_dim),
                )
                .truncate(),
            );
            let b = text_button(ui, t, "Change", true, "comments.author");
            self.note("comments.author", b.rect, true);
            if b.clicked() {
                self.ui.comments.author = Some(current.clone());
            }
        });
    }

    /// The Comments button at the right of the title bar: opens and closes the pane, and says how many comments the drawing has.
    pub(crate) fn comments_button(&mut self, ui: &mut Ui, t: &Tokens, right: f32, bar: Rect) -> f32 {
        let r = Rect::from_center_size(pos2(right - 14.0, bar.center().y), vec2(26.0, 26.0));
        let enabled = self.session.document().is_some();
        let resp = ui.interact(r, ui.scope_id().with("comments_button"), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, "Comments");
        widgets::paint_button_bg(ui, r, &resp, enabled, self.pane_open(Pane::Comments), t, 4);
        crate::icons::paint(ui.painter(), r.shrink(5.0), crate::icons::Icon::of(crate::icons::Base::Bubble), t, enabled);
        let count = self.comment_count();
        if count > 0 {
            let badge = Rect::from_center_size(r.right_top() + vec2(-3.0, 4.0), vec2(12.0, 12.0));
            ui.painter().circle_filled(badge.center(), 6.0, t.accent);
            let words = if count > 99 { "9+".to_owned() } else { count.to_string() };
            ui.painter().text(badge.center(), Align2::CENTER_CENTER, words, theme::font(8.0), t.accent_text);
        }
        self.note("titlebar.comments", r, enabled);
        if resp.on_hover_text(widgets::tip("Comments", enabled)).clicked() {
            let _ = self.execute("window.pane", &json!({ "pane": "comments" }));
        }
        r.left() - 6.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn row(id: u32, page: u32, anchor: (f64, f64)) -> Row {
        Row {
            id,
            parent: None,
            page,
            shape: Some(1),
            author: "A".to_owned(),
            initials: "A".to_owned(),
            text: "t".to_owned(),
            date: String::new(),
            edited: false,
            done: false,
            anchor,
            head: None,
            gone: false,
        }
    }

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))
    }

    /// 96 points to the inch, from the page's lower left at the area's lower left.
    fn screen(p: (f64, f64)) -> Pos2 {
        pos2((p.0 * 96.0) as f32, (800.0 - p.1 * 96.0) as f32)
    }

    #[test]
    fn a_hundred_thousand_comments_on_one_anchor_are_one_stack_of_markers_found_by_a_pass() {
        let rows: Vec<Row> = (1..=100_000).map(|i| row(i, 1, (3.0, 4.0))).collect();
        crate::work::reset();
        let placed = places(&rows, 1, area(), screen);
        assert_eq!(placed.len(), MAX_STACK, "one stack of markers");
        assert_eq!(placed.last().map(|p| p.more), Some(100_000 - MAX_STACK), "the others are counted on the last");
        let work = crate::work::work();
        assert_eq!(work.comment_rows_seen, 100_000, "each row is looked at once");
        assert!(work.comment_cells_probed <= 2 * 100_000, "at most two probes of the table for each: {}", work.comment_cells_probed);
        // The same rows spread over the page cost the same looks: the count is the yardstick.
        let spread: Vec<Row> = (1..=100_000).map(|i| row(i, 1, (f64::from(i % 10), f64::from(i % 8)))).collect();
        crate::work::reset();
        let _ = places(&spread, 1, area(), screen);
        assert_eq!(crate::work::work().comment_rows_seen, 100_000);
    }

    #[test]
    fn markers_stack_one_under_another_and_each_is_at_most_the_cap() {
        let rows: Vec<Row> = (1..=2).map(|i| row(i, 1, (3.0, 4.0))).collect();
        let placed = places(&rows, 1, area(), screen);
        assert_eq!(placed.len(), 2);
        assert!((placed[1].at.y - placed[0].at.y - MARKER).abs() < 1e-3, "the second is a marker's height lower");
        assert_eq!(placed[0].more + placed[1].more, 0);
    }

    #[test]
    fn an_anchor_that_is_not_a_number_or_is_nowhere_near_the_page_has_no_marker_and_the_frame_ends() {
        let rows = vec![
            row(1, 1, (1e30, 1e30)),
            row(2, 1, (f64::NAN, 1.0)),
            row(3, 1, (f64::INFINITY, -f64::INFINITY)),
            row(4, 1, (-1e30, 2.0)),
            row(5, 1, (2.0, 2.0)),
            row(6, 2, (2.0, 2.0)),
        ];
        let placed = places(&rows, 1, area(), |p| pos2(p.0 as f32 * 96.0, 800.0 - p.1 as f32 * 96.0));
        assert_eq!(placed.iter().map(|p| p.row.id).collect::<Vec<_>>(), vec![5], "only the marker that is on the area, of this page");
    }

    #[test]
    fn at_most_the_cap_of_markers_a_frame() {
        let rows: Vec<Row> = (0..5_000).map(|i| row(i + 1, 1, (f64::from(i % 10) * 0.5, f64::from(i / 10 % 8) * 0.5))).collect();
        assert!(places(&rows, 1, area(), screen).len() <= MAX_MARKERS);
    }
}
