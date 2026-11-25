//! The splash popup and About DiaCraft (owner 2026-11-22): the octopus cover as the whole of a popup in the middle of the window, with the version, the git
//! release, the year and the licence painted over its lower left corner.
//!
//! **One panel, two ways in.** At the start the desktop app and the web page give the window a splash ([`DiacraftApp::with_start_splash`]): it goes after
//! [`SHOWN_FOR`] seconds of the window's clock (`clock.rs`), **held while the pointer is over the panel** so that the links can be reached, and it runs on from where it
//! was when the pointer leaves. The window command `app.about` (File view, About) shows the same panel with **no timer**. Either goes on a click anywhere outside the links
//! (on the backdrop, or on the panel) and on Escape, **and the click or the key is used up**: the panel is a modal (`ask.rs`, [`DiacraftApp::is_modal_open`]), so the
//! drawing, the ribbon and the shortcuts do not see the press that dismisses it, and nothing under it takes the keys or the pointer while it is up. A link opens its page
//! through egui's `OpenUrl`, in a new tab on the web, and leaves the panel where it is: it is not dismissed before the page is open.
//!
//! **The timer asks for one frame, at its end** (`request_repaint_after` the time left), never one for each frame: kittest's `run` fails on a window that repaints for ever,
//! and a held timer asks for none. Nothing here reads the machine's clock.
//!
//! **Layout.** The picture is the popup, edge to edge, **836 × 470 points on every screen** (`panel_size`; the owner, 2026-11-24: "same size on every screen"), or scaled down
//! to fit a window with a margin, never up. **The picture it is drawn from** (`drawn_from_cover`): the splash copy (836 × 470 pixels, the cover halved) where the
//! panel is no more pixels wide than the copy, which is every screen of one pixel to the point; where it is more (at its full size, every density above one), the cover itself
//! (1672 × 941 pixels, `branding.rs`), **where the window has it**, so that the panel is not the copy blown up. The desktop carries the cover. The web build fetches it when the File
//! view first shows its Home page and not for the splash: until then its splash on a dense screen is the copy scaled up, and from then on the cover. Either is drawn whole
//! over the panel (the copy is the whole cover resized), so a point of the picture is at the same place in both, and the zones below hold in both (a test reads each picture).
//! There is no strip or margin of the theme round or under the text: it is painted over the picture, in [`TEXT_ZONE`], the corner
//! under the small flowchart and left of the lone diamond and the tentacles (a test reads the picture and holds the zone clear of art). **The rows are laid out from the
//! zone's bottom up**, and the zone ends as far above the picture's bottom edge as it starts from its left edge (the owner, 2026-11-23: the last row is not at the edge):
//! the block's height is the rows' own, read from the fonts in use, so the space under the last row is that margin whatever the fonts. Text sizes scale with the picture.
//! Its colours are the art's own (`Tokens::splash_ink` and `splash_link`: the cream is the picture's, not the theme's); only the scrim, the edge and the shadow follow the
//! theme. The links are underlined, so they are not told by their colour alone.
//!
//! **What is ours.** The words and their order (the version, the git release, the © line, the licence line with its two links) are the owner's brief; no reference is read. The
//! link under them, Third-party licences, is ours: it runs `app.licenses`, which hands the screen over to the window of `licenses.rs`. **About adds the
//! notice** ([`TRADEMARK`], owner 2026-11-23: the trademark line, that OpenDocument Format is an OASIS Standard, the marks of LibreOffice and OpenOffice, since 2026-11-25 those of
//! ArchiMate, UML and BPMN, and who DiaCraft is not affiliated with), whole, in a zone of its own ([`LEGAL_ZONE`]): the clear corner over the picture's title, apart from the rows
//! above, so it takes no room from them. Its text is laid out at `LEGAL_SIZE` and, if the face is so wide that it would not fit the corner, smaller (`fit_size`); it is never cut and
//! never leaves the corner. The start splash, which is the same panel on a timer, does not show it.

use std::time::Duration;

use egui::{
    Align, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Label, Layout, Margin, Modal, Modifiers, OpenUrl, Pos2, Rect,
    Response, RichText, Role, Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};
use serde_json::json;

use diacraft_engine::build_info::BuildInfo;

use crate::app::DiacraftApp;
use crate::branding::{Picture, SPLASH_HEIGHT, SPLASH_WIDTH};
use crate::theme::{self, Tokens};

/// How long the start splash stays, by the window's clock, while the pointer is not over it.
pub const SHOWN_FOR: Duration = Duration::from_secs(4);

/// The page of the MIT License, where its name links to.
pub const MIT_URL: &str = "https://opensource.org/license/mit";

/// The page of the Apache License 2.0, where its name links to.
pub const APACHE_URL: &str = "https://www.apache.org/licenses/LICENSE-2.0";

/// What the link to the third-party licences says when the pointer rests on it.
pub const LICENSES_HINT: &str = if cfg!(target_arch = "wasm32") {
    "THIRD-PARTY-LICENSES-WEB.txt: the libraries the web version of DiaCraft is built from and the licences they are under"
} else {
    "THIRD-PARTY-LICENSES.txt: the libraries DiaCraft is built from and the licences they are under"
};

/// The picture's size in its own pixels.
pub const ART_SIZE: Vec2 = Vec2::new(SPLASH_WIDTH as f32, SPLASH_HEIGHT as f32);

/// Where the text goes on the picture, in the picture's pixels (left, top, right, bottom): below the small flowchart (whose boxes end at y 328 and whose diamond's tip at
/// y 333), left of the lone diamond and the line down to it (x 289 and 313), and clear of the tentacle at the bottom (which comes to x 239 at the last row). Its bottom is
/// as far above the picture's bottom edge (470) as its left is from the left edge, so that the last row does not sit on the edge. A test holds the picture clear of art here.
pub const TEXT_ZONE: [f32; 4] = [24.0, 336.0, 232.0, 446.0];

/// The notice About DiaCraft says (owner, 2026-11-23: in NOTICE, the About box and the site's footer, the same words in each): the trademark line, that OpenDocument Format is an
/// OASIS Standard, the marks of LibreOffice and OpenOffice, those of ArchiMate, UML and BPMN (owner, 2026-11-25: the site names them), and that DiaCraft is not affiliated with or
/// endorsed by their owners. Its first sentence is the one the notice began with.
pub const TRADEMARK: &str = "Visio is a trademark of the Microsoft group of companies. OpenDocument Format (ODF) is an OASIS Standard. LibreOffice is a registered trademark of The Document Foundation. OpenOffice and Apache OpenOffice are registered trademarks of The Apache Software Foundation. ArchiMate is a registered trademark of The Open Group. UML and BPMN are trademarks of the Object Management Group. DiaCraft is not affiliated with or endorsed by Microsoft, OASIS, The Document Foundation, The Apache Software Foundation, The Open Group or the Object Management Group.";

/// Where the notice goes on the picture, in the picture's pixels (left, top, right, bottom): the clear corner over the title, as far from the picture's top and left
/// edges as the text zone is from its left edge, and left of the flowchart's line (x 376) and box. A test holds the picture clear of art here.
pub const LEGAL_ZONE: [f32; 4] = [24.0, 24.0, 352.0, 136.0];

/// The room kept round the panel when the window is too small for it, in points.
const MARGIN: f32 = 16.0;

/// Text sizes in the picture's pixels: the version line, the other lines and the notice. They scale with the picture.
const TITLE_SIZE: f32 = 16.0;
const LINE_SIZE: f32 = 12.0;
const LEGAL_SIZE: f32 = 11.0;

/// The least size the notice shrinks to, in the picture's pixels, when the interface font is so wide that it would not fit its corner at [`LEGAL_SIZE`].
const LEGAL_LEAST_SIZE: f32 = 7.0;

/// The most steps [`fit_size`] takes, each a twentieth smaller than the one before.
const FIT_STEPS: usize = 40;

/// The space between two rows, the space between the two rows of the licence, and the more space before the © line, before the licence and before the link, in the
/// picture's pixels: what lets the block fit the zone with the desktop's own fonts (108 of its 110 pixels).
const ROW_GAP: f32 = 2.0;
const LICENCE_ROW_GAP: f32 = 1.0;
const GROUP_GAPS: [f32; 3] = [2.0, 2.0, 1.0];

/// The gaps of the text block in points, for a picture shown at `k` points to its pixel: between its six rows (the version, the git release, the © line, the licence's two
/// rows and the link), and the more space between their groups. The block is its rows' heights and this.
pub(crate) fn block_gaps(k: f32) -> f32 {
    (4.0 * ROW_GAP + LICENCE_ROW_GAP + GROUP_GAPS[0] + GROUP_GAPS[1] + GROUP_GAPS[2]) * k
}

/// The size for a text to be laid out at, from `start` down to `least` in steps of a twentieth: the first at which `height_at(size)` is at most `room`, or `least` when none of
/// them is (or `start` when that is smaller than `least`). It never returns more than `start` or less than the smaller of the two, and a height that is not a number fits nothing.
/// The notice is laid out at [`LEGAL_SIZE`] when its corner holds it. Since it names the marks of ArchiMate, UML and BPMN too (2026-11-25) it is one step smaller with the
/// fonts of the tests (10.45 of the picture's pixels: 130 pixels tall at 11, in a corner of 112, measured at the six sizes of the tests); a wider face makes it smaller still
/// rather than longer than its corner, which would put it over the picture's title.
pub(crate) fn fit_size(start: f32, least: f32, room: f32, mut height_at: impl FnMut(f32) -> f32) -> f32 {
    let least = least.min(start);
    let mut size = start;
    for _ in 0..FIT_STEPS {
        if size <= least {
            return least;
        }
        if height_at(size) <= room {
            return size;
        }
        size *= 0.95;
    }
    least
}

/// How the panel came: at the start of the window, with a timer, or from About, with none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplashKind {
    Start,
    About,
}

impl SplashKind {
    /// The name `ui.inspect` and the window state give it.
    pub const fn name(self) -> &'static str {
        match self {
            SplashKind::Start => "start",
            SplashKind::About => "about",
        }
    }
}

/// The panel while it is up.
#[derive(Debug)]
pub struct Splash {
    kind: SplashKind,
    /// The time left, for a start splash.
    left: Duration,
    /// The window's clock at the last frame that counted the time.
    seen: Option<Duration>,
    /// Whether the pointer was over the panel at that frame: the time since then is counted by what it was **then**, because a pointer that rested on the panel for a minute and
    /// then left has held the timer for that minute.
    held: bool,
    /// Where the panel was at the last frame, for the next frame to ask whether the pointer is over it.
    panel: Option<Rect>,
}

impl Splash {
    /// The splash of a start: it has [`SHOWN_FOR`].
    pub fn start() -> Splash {
        Splash { kind: SplashKind::Start, left: SHOWN_FOR, seen: None, held: false, panel: None }
    }

    /// The About panel: no timer.
    pub fn about() -> Splash {
        Splash { kind: SplashKind::About, left: Duration::MAX, seen: None, held: false, panel: None }
    }

    /// How it came.
    pub fn kind(&self) -> SplashKind {
        self.kind
    }

    /// Whether it has a timer.
    pub fn timed(&self) -> bool {
        self.kind == SplashKind::Start
    }

    /// The time left, for one that is timed.
    pub fn left(&self) -> Duration {
        self.left
    }

    /// A frame at `now` of the window's clock, with the pointer `over` the panel or not: the time since the last frame is counted if the pointer was not over the panel then.
    pub fn tick(&mut self, now: Duration, over: bool) {
        if let Some(seen) = self.seen
            && self.timed()
            && !self.held
        {
            self.left = self.left.saturating_sub(now.saturating_sub(seen));
        }
        self.seen = Some(now);
        self.held = over;
    }

    /// Whether its time is up.
    pub fn ended(&self) -> bool {
        self.timed() && self.left.is_zero()
    }

    fn over(&self, pointer: Option<Pos2>) -> bool {
        self.panel.zip(pointer).is_some_and(|(panel, at)| panel.contains(at))
    }
}

/// The panel's size in points in a window of `screen`: the picture's size in points, [`ART_SIZE`], **the same at every pixel density** (the owner, 2026-11-24: "same size on every screen"),
/// scaled down (never up) to fit with a margin. A window of no size, or of one that is no number, has a panel of a point or so, never one of no size or of NaN.
pub(crate) fn panel_size(screen: Rect) -> Vec2 {
    let room = (screen.size() - Vec2::splat(2.0 * MARGIN)).max(Vec2::splat(1.0));
    let fit = (room.x / ART_SIZE.x).min(room.y / ART_SIZE.y).min(1.0);
    ART_SIZE * fit
}

/// Whether a panel `width` points wide at `pixels_per_point` is drawn from the cover (twice the splash copy's pixels each way) rather than from the copy: **when it is more pixels wide than
/// the copy**, which would be blown up to fill it. At the panel's full size that is every density above one pixel to the point; a panel shrunk to fit a small window on such a screen that is
/// still no more pixels than the copy has it, which is not blown up there (and the cover would be shrunk past half, which its texture's filter does not smooth). A width or a density that is
/// no number, nothing or less is the copy, and no product of them is made that could be no number.
pub(crate) fn drawn_from_cover(width: f32, pixels_per_point: f32) -> bool {
    width > 0.0 && pixels_per_point > 0.0 && width * pixels_per_point > ART_SIZE.x
}

impl DiacraftApp {
    /// The window starts with the splash: it shows over the first frame and goes after [`SHOWN_FOR`], on a click outside its links or on Escape. The desktop app and the web
    /// page ask for it; a window made with `new` has none, so a test keeps its clicks.
    pub fn with_start_splash(mut self) -> DiacraftApp {
        self.ui.splash = Some(Splash::start());
        self
    }

    /// Gives the window the build it says in the splash and About panel in place of the engine's (`diacraft_engine::build_info`): for a test, which then does not depend on the
    /// commit it was built at.
    pub fn with_build_info(mut self, build: BuildInfo) -> DiacraftApp {
        self.build = build;
        self
    }

    /// The git release and the year the window says.
    pub fn build_info(&self) -> &BuildInfo {
        &self.build
    }

    /// How the splash or About panel came, while it is up.
    pub fn splash_kind(&self) -> Option<SplashKind> {
        self.ui.splash.as_ref().map(Splash::kind)
    }

    /// `app.about`: the panel, with no timer. One that is up is taken over: from then on it has none.
    pub(crate) fn show_about(&mut self) {
        self.ui.splash = Some(Splash::about());
    }

    /// `app.about {close: true}`: the panel goes. Closing what is not up is not an error.
    pub(crate) fn close_splash(&mut self) {
        self.ui.splash = None;
    }

    /// Escape, before anything else takes it: it dismisses the panel and is used up, so the drawing under it does not also clear its selection. Run at the start of a
    /// frame, before the shortcuts and the drawing's keys.
    pub(crate) fn splash_keys(&mut self, ctx: &Context) {
        if self.ui.splash.is_some() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.ui.splash = None;
        }
    }

    /// Draws the panel over the window, counts its time, and dismisses it on a click outside the links.
    pub(crate) fn splash(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut splash) = self.ui.splash.take() else { return };
        let size = panel_size(ctx.content_rect());
        // The picture first. **On a dense screen it is the cover where the window has it** (the desktop carries it, the web build has it once the File view has fetched it),
        // so that the panel is not the splash copy blown up; the web build does not fetch it for the splash, and draws the copy until then. Else the copy: one that cannot be had or decoded
        // is no splash, and says so once (the status bar, the log). **One that has not come yet is a panel with the words and no picture** (the web build fetches it, and the host wakes
        // the window when it is here): the layout does not move when it comes, and its four seconds begin then.
        let source = self.asset_source();
        let cover = if drawn_from_cover(size.x, ctx.pixels_per_point()) { self.artwork.cover_if_here(ctx, &source) } else { None };
        let texture = match cover {
            Some(cover) => Some(cover),
            None => match self.artwork.splash_picture(ctx, &source) {
                Picture::Ready(texture) => Some(texture),
                Picture::Waiting => None,
                Picture::Failed(error) => {
                    let message = format!("DiaCraft artwork could not load: {error}");
                    log::error!("{message}");
                    self.set_notice(&message);
                    return;
                }
            },
        };
        let over = splash.over(ctx.pointer_hover_pos());
        let about = splash.kind() == SplashKind::About;
        if texture.is_some() {
            splash.tick(self.clock.now(), over);
            if splash.ended() {
                return;
            }
        }
        let k = size.x / ART_SIZE.x;
        let mut shown = Shown::default();
        let frame = Frame::new()
            .fill(t.panel_fill)
            .stroke(Stroke::new(1.0, t.ribbon_border))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::ZERO)
            .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: t.page_shadow });
        let modal = Modal::new(Id::unique("splash")).backdrop_color(t.scrim).frame(frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let (panel, response) = ui.allocate_exact_size(size, Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Image, true, "DiaCraft, Diagramming software"));
            match &texture {
                Some(texture) => {
                    ui.painter().image(texture.id(), panel, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), t.image_tint);
                }
                None => {
                    // The picture's own paper, and a short wait in the free half of it (the right of the text zone, clear of the lines).
                    ui.painter().rect_filled(panel, 0.0, t.splash_paper);
                    let at = panel.min + vec2((TEXT_ZONE[2] + (ART_SIZE.x - TEXT_ZONE[2]) / 2.0) * k, ART_SIZE.y / 2.0 * k);
                    ui.painter().text(
                        at,
                        egui::Align2::CENTER_CENTER,
                        PICTURE_WAIT,
                        FontId::proportional(((LINE_SIZE + 2.0) * k).max(1.0)),
                        t.splash_ink,
                    );
                    shown.waiting = true;
                }
            }
            let zone = Rect::from_min_max(panel.min + vec2(TEXT_ZONE[0], TEXT_ZONE[1]) * k, panel.min + vec2(TEXT_ZONE[2], TEXT_ZONE[3]) * k);
            shown.panel = Some(panel);
            shown.zone = Some(zone);
            shown.on_panel = response.clicked() || response.secondary_clicked();
            self.paint_text(ui, &t, (panel, zone), about, &mut shown);
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        splash.panel = shown.panel;
        let mut notes = std::mem::take(&mut shown.rects);
        if shown.waiting {
            notes.extend(shown.panel.map(|rect| ("splash.waiting", rect)));
        }
        notes.extend(shown.panel.map(|rect| ("splash.panel", rect)));
        notes.extend(shown.zone.map(|rect| ("splash.zone", rect)));
        for (name, rect) in notes {
            self.note(name, rect, true);
        }
        if let Some(url) = shown.opened {
            ctx.open_url(OpenUrl::new_tab(url));
        }
        if shown.licenses {
            // The window of the third-party licences takes over from the panel: the link runs the command, as a program on the control channel does.
            ctx.request_repaint();
            if let Err(e) = self.execute("app.licenses", &json!({})) {
                self.set_notice(&e.to_string());
            }
            return;
        }
        let outside = modal.backdrop_response.clicked() || modal.backdrop_response.secondary_clicked();
        if outside || shown.on_panel {
            // Gone: the frame that follows is drawn without it.
            ctx.request_repaint();
            return;
        }
        // The one frame the timer asks for is at its end; a held timer has none to wait for, and the frame the pointer leaves with comes by itself. A panel that waits for its picture has no
        // timer yet, and the host's wake-up is its next frame.
        if splash.timed() && !over && texture.is_some() {
            ctx.request_repaint_after(splash.left());
        }
        self.ui.splash = Some(splash);
    }

    /// The text over the picture, in the zone: the version, the git release, the © line, the licence line with its two links and the link to the third-party licences, each
    /// line one row that is cut short with `…` if it does not fit the zone's width, the licence two rows that break where its sentence does. The block is laid out from the
    /// zone's bottom up: it starts its own height above the bottom (never above the zone's top). About adds the notice in its own zone.
    fn paint_text(&self, ui: &mut Ui, t: &Tokens, (panel, zone): (Rect, Rect), about: bool, shown: &mut Shown) {
        let k = panel.width() / ART_SIZE.x;
        let font = |size: f32| FontId::proportional((size * k).max(1.0));
        let title_font = theme::font_strong((TITLE_SIZE * k).max(1.0));
        let build = &self.build;
        let (version, copyright) = (format!("DiaCraft {}", env!("CARGO_PKG_VERSION")), format!("© {} the DiaCraft contributors", build.year));
        // The rows' heights are their galleys', laid out as the labels lay them out (a row is a whole number of pixels, and as tall as its tallest glyph's line).
        let rows = [
            (version.as_str(), &title_font),
            (build.git.as_str(), &font(LINE_SIZE)),
            (copyright.as_str(), &font(LINE_SIZE)),
            ("Licensed under the MIT License", &font(LINE_SIZE)),
            ("or the Apache License 2.0.", &font(LINE_SIZE)),
            ("Third-party licences", &font(LINE_SIZE)),
        ];
        let height: f32 =
            ui.fonts_mut(|f| rows.iter().map(|(text, font)| f.layout_no_wrap((*text).to_owned(), (*font).clone(), t.splash_ink).size().y).sum());
        let top = (zone.max.y - height - block_gaps(k)).max(zone.min.y);
        let block = Rect::from_min_max(pos2(zone.min.x, top), zone.max);
        let rects = &mut shown.rects;
        let opened = &mut shown.opened;
        let licenses = &mut shown.licenses;
        ui.scope_builder(UiBuilder::new().max_rect(block).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, ROW_GAP * k);
            let version = ui.add(line(version, title_font.clone(), t.splash_ink));
            rects.push(("splash.version", version.rect));
            let git = ui.add(line(build.git.clone(), font(LINE_SIZE), t.splash_ink));
            rects.push(("splash.git", git.rect));
            ui.add_space(GROUP_GAPS[0] * k);
            let copyright = ui.add(line(copyright, font(LINE_SIZE), t.splash_ink));
            rects.push(("splash.copyright", copyright.rect));
            ui.add_space(GROUP_GAPS[1] * k);
            // The line is as tall as its text, not as tall as a button.
            ui.spacing_mut().interact_size.y = 0.0;
            // "Licensed under the MIT License or the Apache License 2.0." in two rows that break where the sentence does (the owner, 2026-11-23: no "at your option").
            let licence = ui.scope(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, LICENCE_ROW_GAP * k);
                let mit = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.add(line("Licensed under the ".to_owned(), font(LINE_SIZE), t.splash_ink));
                        link(ui, "MIT License", MIT_URL, font(LINE_SIZE), t.splash_link)
                    })
                    .inner;
                let apache = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.add(line("or the ".to_owned(), font(LINE_SIZE), t.splash_ink));
                        let apache = link(ui, "Apache License 2.0", APACHE_URL, font(LINE_SIZE), t.splash_link);
                        ui.add(line(".".to_owned(), font(LINE_SIZE), t.splash_ink));
                        apache
                    })
                    .inner;
                (mit, apache)
            });
            let (mit, apache) = licence.inner;
            rects.push(("splash.link.mit", mit.0));
            rects.push(("splash.link.apache", apache.0));
            if mit.1 {
                *opened = Some(MIT_URL);
            } else if apache.1 {
                *opened = Some(APACHE_URL);
            }
            rects.push(("splash.licence", licence.response.rect));
            // The third-party licences: a link under the licence line that runs `app.licenses` (`licenses.rs`).
            ui.add_space(GROUP_GAPS[2] * k);
            let third = link(ui, "Third-party licences", LICENSES_HINT, font(LINE_SIZE), t.splash_link);
            rects.push(("splash.link.licenses", third.0));
            *licenses = third.1;
        });
        if about {
            // The notice, About's alone: wrapped in its own zone, the clear corner over the picture's title, at the size that holds it whole in that zone.
            let legal = Rect::from_min_max(panel.min + vec2(LEGAL_ZONE[0], LEGAL_ZONE[1]) * k, panel.min + vec2(LEGAL_ZONE[2], LEGAL_ZONE[3]) * k);
            let (start, least) = ((LEGAL_SIZE * k).max(1.0), (LEGAL_LEAST_SIZE * k).max(1.0));
            let size = ui.fonts_mut(|f| {
                fit_size(start, least, legal.height(), |size| {
                    f.layout(TRADEMARK.to_owned(), FontId::proportional(size), t.splash_ink, legal.width()).size().y
                })
            });
            ui.scope_builder(UiBuilder::new().max_rect(legal).layout(Layout::top_down(Align::Min)), |ui| {
                let label = Label::new(RichText::new(TRADEMARK).font(FontId::proportional(size)).color(t.splash_ink)).wrap().selectable(false);
                let row = ui.add(label);
                shown.rects.push(("splash.trademark", row.rect));
            });
        }
    }
}

/// What the panel says in the place of its picture while the picture is on its way (the web build fetches it).
pub const PICTURE_WAIT: &str = "Loading the picture\u{2026}";

/// What a frame of the panel drew and heard: for the notes of `ui.elements`, and for what the frame does after the modal is drawn.
#[derive(Default)]
struct Shown {
    /// The picture was not there: the panel is the paper and the words.
    waiting: bool,
    panel: Option<Rect>,
    zone: Option<Rect>,
    rects: Vec<(&'static str, Rect)>,
    /// A press on the picture or on the text that is no link.
    on_panel: bool,
    /// The page of the link that was pressed.
    opened: Option<&'static str>,
    /// The link to the third-party licences was pressed.
    licenses: bool,
}

/// A line of the text: one row, cut short with `…` if it is too wide, and not selectable (a selectable label would take the press that is meant to close the panel).
fn line(text: String, font: FontId, colour: Color32) -> Label {
    Label::new(RichText::new(text).font(font).color(colour)).truncate().selectable(false)
}

/// A link: the licence's name, underlined besides its colour, with the page (or what the link shows) in its tooltip. → where it is, and whether it was pressed.
fn link(ui: &mut Ui, text: &str, url: &str, font: FontId, colour: Color32) -> (Rect, bool) {
    let response: Response = ui.add(Label::new(RichText::new(text).font(font).color(colour).underline()).sense(Sense::click()).selectable(false));
    response.widget_info(|| WidgetInfo::labeled(Role::Link, true, text));
    let rect = response.rect;
    let response = response.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(url);
    (rect, response.clicked())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_is_counted_by_what_the_pointer_did_at_the_frame_before() {
        let secs = Duration::from_secs;
        let mut splash = Splash::start();
        splash.tick(secs(10), false);
        assert_eq!(splash.left(), SHOWN_FOR, "the first frame starts the time, whenever the clock was");
        splash.tick(secs(11), true);
        assert_eq!(splash.left(), secs(3), "a second with the pointer away");
        // The pointer rests on the panel for a minute, and leaves: the minute is held, and the time runs on from the frame it left at.
        splash.tick(secs(71), true);
        assert_eq!(splash.left(), secs(3), "a minute over the panel counts for nothing");
        splash.tick(secs(72), false);
        assert_eq!(splash.left(), secs(3), "the frame the pointer left at: the second before it was held");
        splash.tick(secs(74), false);
        assert_eq!(splash.left(), secs(1));
        assert!(!splash.ended());
        splash.tick(secs(100), false);
        assert!(splash.ended() && splash.left().is_zero(), "the time is up and does not go below nothing");
    }

    #[test]
    fn a_clock_that_stands_still_or_runs_back_counts_nothing() {
        let secs = Duration::from_secs;
        let mut splash = Splash::start();
        splash.tick(secs(5), false);
        splash.tick(secs(5), false);
        splash.tick(secs(2), false);
        assert_eq!(splash.left(), SHOWN_FOR);
    }

    #[test]
    fn about_has_no_timer_and_never_ends() {
        let mut about = Splash::about();
        assert!(!about.timed());
        about.tick(Duration::ZERO, false);
        about.tick(Duration::from_secs(1_000_000), false);
        assert!(!about.ended());
        assert_eq!(about.kind(), SplashKind::About);
        assert_eq!((SplashKind::Start.name(), SplashKind::About.name()), ("start", "about"));
    }

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(w, h))
    }

    /// The owner, 2026-11-24 ("same size on every screen"): in a window with room for it the panel is 836 × 470 points at 1, 1.5, 2 and 3 pixels to the point, never larger in a larger
    /// window; at one pixel to the point it is the copy's own pixels, and above one it is more pixels than the copy, so it is drawn from the cover.
    #[test]
    fn the_panel_is_836_by_470_points_at_every_density_and_drawn_from_the_cover_above_one_pixel_to_the_point() {
        assert_eq!(ART_SIZE, vec2(836.0, 470.0));
        for (ppp, cover) in [(1.0, false), (1.5, true), (2.0, true), (3.0, true)] {
            for window in [screen(1280.0, 800.0), screen(3000.0, 2000.0), screen(836.0 + 2.0 * MARGIN, 470.0 + 2.0 * MARGIN)] {
                let size = panel_size(window);
                assert_eq!(size, ART_SIZE, "{window:?} at {ppp}");
                assert_eq!(drawn_from_cover(size.x, ppp), cover, "{window:?} at {ppp}");
            }
        }
        assert!(drawn_from_cover(ART_SIZE.x, 1.0 + f32::EPSILON), "the least density above one");
        assert!(!drawn_from_cover(ART_SIZE.x, 1.0 - f32::EPSILON / 2.0));
    }

    /// A small window shrinks the panel to fit with its margin, keeping the picture's shape, the same at one and at two pixels to the point; the panel shrunk is drawn from the copy at one,
    /// and from the cover at two, where it is still more pixels than the copy (448 × 252 points in a window of 480 × 320, 896 pixels wide). Just above one it is the copy, which it does not blow up.
    #[test]
    fn in_a_small_window_the_panel_shrinks_to_fit_at_one_and_at_two_pixels_to_the_point() {
        for (w, h) in [(480.0, 320.0), (640.0, 400.0), (800.0, 300.0), (867.0, 1000.0), (1000.0, 501.0)] {
            let small = panel_size(screen(w, h));
            assert!(small.x <= w - 2.0 * MARGIN && small.y <= h - 2.0 * MARGIN, "{w} × {h}: {small:?}");
            assert!(small.x < ART_SIZE.x && small.y < ART_SIZE.y, "{w} × {h}: smaller than the picture, never larger: {small:?}");
            assert!(
                (small.x - (w - 2.0 * MARGIN)).abs() < 1e-3 || (small.y - (h - 2.0 * MARGIN)).abs() < 1e-3,
                "{w} × {h}: as large as fits: {small:?}"
            );
            assert!((small.x / small.y - ART_SIZE.x / ART_SIZE.y).abs() < 1e-4, "{w} × {h}: the picture keeps its shape");
        }
        let small = panel_size(screen(480.0, 320.0));
        assert!((small - vec2(448.0, 448.0 * ART_SIZE.y / ART_SIZE.x)).abs().max_elem() < 1e-3, "{small:?}");
        assert!(!drawn_from_cover(small.x, 1.0) && drawn_from_cover(small.x, 2.0));
        assert!(!drawn_from_cover(small.x, 1.25), "{} pixels wide, fewer than the copy's 836", small.x * 1.25);
    }

    /// A window of no size, of a negative one, or of one that is no number or infinite, is not a panel of no size, nor of NaN, nor larger than the picture; and a width or a density that is
    /// no number, nothing or negative is drawn from the copy.
    #[test]
    fn a_window_that_is_no_number_is_a_panel_of_some_size_and_drawn_from_the_copy() {
        let (nan, inf) = (f32::NAN, f32::INFINITY);
        for window in [
            screen(0.0, 0.0),
            screen(-100.0, 100.0),
            screen(nan, nan),
            screen(nan, 800.0),
            screen(inf, inf),
            screen(f32::MAX, 1.0),
            screen(-inf, inf),
            Rect::NOTHING,
            Rect::EVERYTHING,
            Rect::NAN,
        ] {
            let size = panel_size(window);
            assert!(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0, "{window:?}: {size:?}");
            assert!(size.x <= ART_SIZE.x && size.y <= ART_SIZE.y, "{window:?}: {size:?}");
        }
        assert_eq!(panel_size(screen(inf, inf)), ART_SIZE);
        for (width, ppp) in
            [(nan, 2.0), (ART_SIZE.x, nan), (inf, 0.0), (0.0, inf), (-ART_SIZE.x, -2.0), (ART_SIZE.x, -2.0), (ART_SIZE.x, 0.0), (nan, nan)]
        {
            assert!(!drawn_from_cover(width, ppp), "{width} at {ppp}");
        }
        assert!(drawn_from_cover(inf, 2.0) && drawn_from_cover(ART_SIZE.x, inf), "an infinity is more pixels");
    }

    /// The notice's size: the start when its height fits, smaller in steps of a twentieth when it does not, never below the least, and the least when nothing fits or the
    /// height is no number.
    #[test]
    fn the_notices_size_is_the_largest_step_that_fits_and_never_leaves_its_bounds() {
        let tall = |size: f32| size * 10.0;
        assert_eq!(fit_size(11.0, 7.0, 110.0, tall), 11.0, "it fits at the start");
        assert_eq!(fit_size(11.0, 7.0, 109.0, tall), 11.0 * 0.95, "one step down");
        let fitted = fit_size(11.0, 7.0, 90.0, tall);
        assert!(fitted <= 9.0 && fitted > 9.0 * 0.95, "{fitted}: the first of 11, 10.45, 9.93, 9.43, 8.96 whose height is at most 90 is 8.96");
        assert_eq!(fit_size(11.0, 7.0, 50.0, tall), 7.0, "nothing fits above the least");
        assert_eq!(fit_size(11.0, 7.0, -1.0, tall), 7.0, "a room of no height");
        assert_eq!(fit_size(11.0, 7.0, 100.0, |_| f32::NAN), 7.0, "a height that is no number fits nothing");
        assert_eq!(fit_size(5.0, 7.0, 0.0, tall), 5.0, "a start below the least is the start");
        assert_eq!(fit_size(11.0, 11.0, 0.0, tall), 11.0);
        // Never more calls than the steps, even for a height that always fails.
        let mut calls = 0;
        fit_size(11.0, 0.0, -1.0, |_| {
            calls += 1;
            1.0
        });
        assert_eq!(calls, FIT_STEPS);
    }

    #[test]
    fn the_zone_is_inside_the_picture_and_has_room() {
        const {
            assert!(TEXT_ZONE[0] >= 0.0 && TEXT_ZONE[1] >= 0.0 && TEXT_ZONE[2] <= ART_SIZE.x && TEXT_ZONE[3] <= ART_SIZE.y);
            assert!(TEXT_ZONE[2] - TEXT_ZONE[0] >= 200.0 && TEXT_ZONE[3] - TEXT_ZONE[1] >= 108.0);
            // The owner, 2026-11-23: the zone ends as far above the picture's bottom edge as it starts from its left edge.
            assert!(ART_SIZE.y - TEXT_ZONE[3] == TEXT_ZONE[0]);
            assert!(LEGAL_ZONE[0] == TEXT_ZONE[0] && LEGAL_ZONE[3] < TEXT_ZONE[1]);
        }
    }

    /// The block's gaps scale with the picture: 14 of its pixels. With the desktop's fonts (a version row of 19 pixels, the others of 15) the block is 108 of the picture's
    /// pixels, which the zone's 110 hold.
    #[test]
    fn the_blocks_gaps_scale_with_the_picture_and_the_block_fits_the_zone_with_the_desktops_fonts() {
        assert_eq!(block_gaps(1.0), 14.0);
        assert_eq!(block_gaps(0.5), 7.0);
        assert!(19.0 + 5.0 * 15.0 + block_gaps(1.0) <= TEXT_ZONE[3] - TEXT_ZONE[1]);
    }

    /// A picture that cannot be decoded is no splash: the panel is dropped, the person is told once in the status bar, and nothing is drawn or kept that would try again every
    /// frame. (The bundled picture decodes, so the failure is put in by hand.)
    #[test]
    fn a_splash_picture_that_cannot_be_decoded_is_no_splash_and_says_so() {
        let ctx = Context::default();
        let mut app = DiacraftApp::new(diacraft_engine::Session::new(), None).with_start_splash();
        app.artwork.break_splash_for_test("bundled artwork has unexpected dimensions or bit depth");
        for _ in 0..3 {
            ctx.run_pass(egui::RawInput::default(), |ctx| app.splash(ctx, Tokens::light())).drop_without_applying_deltas();
        }
        assert_eq!(app.splash_kind(), None, "no panel is kept");
        let notice = app.notice().expect("the person is told");
        assert!(notice.contains("artwork could not load") && notice.contains("unexpected dimensions"), "{notice}");
        // About over a broken picture is the same: told, and nothing shown.
        app.show_about();
        ctx.run_pass(egui::RawInput::default(), |ctx| app.splash(ctx, Tokens::light())).drop_without_applying_deltas();
        assert_eq!(app.splash_kind(), None);
    }
}
