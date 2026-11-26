//! Theme tokens: every colour the interface draws with, named for what it colours.
//!
//! The rest of this crate never writes a colour. It asks [`Tokens`] for one, so a theme is a single
//! place to read and change, and a colour cannot be picked per widget by accident (a test scans the
//! sources for that). The values are DiaCraft's own: a light theme (the default) and a dark one.

use egui::{Color32, Theme};

/// The colours of one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    // ---- the window's frame: title bar, tab row, status bar
    /// The title bar's background (and the tab row's).
    pub top_bar_fill: Color32,
    /// The text and icons on the title bar.
    pub top_bar_text: Color32,
    /// What a control on the top bar is filled with while the pointer is not over it: nothing.
    pub control_idle_fill: Color32,
    /// What a control on the top bar is filled with while the pointer is over it.
    pub top_bar_hover: Color32,
    /// What the close button is filled with while the pointer is over it.
    pub close_hover_fill: Color32,
    /// The close button's icon while the pointer is over it.
    pub close_hover_icon: Color32,
    /// The status bar's background.
    pub status_bar_fill: Color32,
    /// The status text on it.
    pub status_bar_text: Color32,
    /// The text of a notice (a problem to tell the person about) on it.
    pub notice_text: Color32,
    /// The line between the bars and the canvas.
    pub separator: Color32,
    /// The shade laid over the whole window behind a question that must be answered; nothing under it takes the pointer.
    pub scrim: Color32,

    // ---- text and the accent
    /// Ordinary text on a panel.
    pub text: Color32,
    /// Quieter text: hints, secondary labels.
    pub text_dim: Color32,
    /// Text and glyphs of a control that cannot be used.
    pub text_disabled: Color32,
    /// The accent: the File tab, the active tab's mark, a selected control.
    pub accent: Color32,
    /// Text and icons drawn on the accent.
    pub accent_text: Color32,
    /// The accent's light tint: a selected or checked control's fill.
    pub accent_soft: Color32,
    /// The accent a little darker: the File tab while the pointer is over it.
    pub accent_hover: Color32,

    // ---- the ribbon
    /// The ribbon body, and the active tab that joins it.
    pub ribbon_fill: Color32,
    /// The line round the ribbon body.
    pub ribbon_border: Color32,
    /// The thin divider between two groups.
    pub group_divider: Color32,
    /// A button while the pointer is over it.
    pub hover: Color32,
    /// A button while it is pressed.
    pub pressed: Color32,
    /// An input box (search, font name, zoom percentage): its fill.
    pub input_fill: Color32,
    /// An input box's outline.
    pub input_border: Color32,

    // ---- icons: drawn in code with these
    /// The strokes of an icon.
    pub icon_ink: Color32,
    /// The colour accent of an icon: a highlighted part.
    pub icon_accent: Color32,
    /// The soft fill of an icon's body.
    pub icon_fill: Color32,
    /// A disabled icon's strokes.
    pub icon_ink_disabled: Color32,
    /// A disabled icon's fills.
    pub icon_fill_disabled: Color32,
    /// The graphite outline of a stencil preview, independent of ribbon icons.
    pub stencil_ink: Color32,
    /// The outline of a stencil preview that cannot be used.
    pub stencil_ink_disabled: Color32,
    /// Stencil previews have no body fill, so the panel or hover shows through.
    pub stencil_fill: Color32,
    /// What a picture of a master's icon (a bitmap from a stencil file, with colours of its own) is multiplied by: white leaves it as it is.
    pub image_tint: Color32,
    /// The same for a tile that cannot be used: the picture dimmed.
    pub image_tint_disabled: Color32,

    // ---- panels: the Shapes window, popups, tiles
    /// A side panel's fill (the Shapes window).
    pub panel_fill: Color32,
    /// A panel heading's fill.
    pub panel_header_fill: Color32,
    /// A tile's fill (a shape in a stencil).
    pub tile_fill: Color32,
    /// A tile's outline.
    pub tile_border: Color32,

    // ---- the drawing area
    /// The workspace round the page: where the drawing will be.
    pub canvas_fill: Color32,
    /// The page's paper.
    pub page_fill: Color32,
    /// The page's edge.
    pub page_border: Color32,
    /// The soft shadow under the page.
    pub page_shadow: Color32,
    /// What the picture of the page is multiplied by when it is painted: white, which changes nothing.
    pub picture_tint: Color32,
    /// A page break: the dashed line where the page passes the printer paper (View, Show, Page Breaks).
    pub page_break: Color32,
    /// A grid line.
    pub grid_line: Color32,
    /// A grid line at a whole unit.
    pub grid_major: Color32,
    /// A ruler's background.
    pub ruler_fill: Color32,
    /// A ruler's tick marks.
    pub ruler_tick: Color32,
    /// The lighter marks between numbered ruler ticks.
    pub ruler_tick_minor: Color32,
    /// A ruler's numbers.
    pub ruler_text: Color32,
    /// The lighter strip of a ruler that lies over the page.
    pub ruler_page_fill: Color32,

    // ---- editing: what is drawn over the page while shapes are selected, moved and joined
    /// The outline of the shapes being moved or resized, and of a connector's end while it is dragged.
    pub edit_line: Color32,
    /// The translucent fill of the preview of a shape in motion.
    pub edit_fill: Color32,
    /// A selection handle's fill (and the white of a connector end's halo).
    pub handle_fill: Color32,
    /// The veil of the selection net, drawn over the shapes it crosses: a translucent light grey.
    pub net_fill: Color32,
    /// The net's one-pixel outline.
    pub net_line: Color32,
    /// The frame drawn on each shape of a multiple selection, and the path of a connector in one: a dark blue.
    pub select_frame: Color32,
    /// The grey of the box round a selection, of the ring of its handles, and of the rotation handle's glyph and stem, and of a free
    /// connector end's squares.
    pub select_box: Color32,
    /// A connection point that is not under the pointer.
    pub glue_point: Color32,
    /// A connector end that is glued, and the connection point under the pointer: green.
    pub glue_on: Color32,
    /// The translucent halo round the connection point or shape a connector would be glued to.
    pub glue_halo: Color32,
    /// A connector end that is free.
    pub glue_off: Color32,
    /// The handle at the middle of a leg of the connector selected, and at the bend a leg being dragged comes to: a filled blue disc (
    /// the selection look's R30, "a bend or middle handle is a filled blue disc").
    pub bend_handle: Color32,
    /// The yellow handle at the end of a callout's leader, which a drag drops on a shape to tie the callout to it ("a yellow handle").
    pub callout_handle: Color32,
    /// A guide: the thin line across the drawing area that shapes are laid out along.
    pub guide_line: Color32,
    /// A guide that is selected, being dragged, or being dragged out of a ruler.
    pub guide_selected: Color32,
    /// The translucent band under a selected guide.
    pub guide_halo: Color32,
    /// What a drag snapped to: the line or the place the engine's snap pulled it to.
    pub snap_mark: Color32,
    /// An AutoConnect arrow: the small pale sky-blue triangle at a side of the shape the pointer is on (as corrected them: the
    /// desktop's five published pictures draw the arrows pale sky blue, and the gray triangles of the connectors page are the web app's; a later ruling replaced the earlier
    /// gray). As pale as the page allows: it reads on the white page and, at the page's edge, on the canvas (3:1 or more on both, in each theme: `tests/it/theme.rs`).
    pub auto_arrow: Color32,
    /// The AutoConnect arrow under the pointer: a deeper blue (the same pictures draw one on the arrow the pointer is near; no page gives either in words).
    pub auto_arrow_hot: Color32,

    // ---- the ShapeSheet window (SHEETWIN): what a cell's colour says of where it comes from
    /// The text of a cell the sheet holds itself (a formula, a value or `No Formula`): blue, with a dot at the cell's left edge so that the difference does not rest on colour (design 5.6).
    pub sheet_local: Color32,
    /// The text of a cell found nowhere, which reads its default.
    pub sheet_default: Color32,
    /// The text of an error value (`#DIV/0!`, `#REF!`), in the cell and in the bar.
    pub sheet_error: Color32,
    /// The background of a cell whose value the last commit changed, until the next commit (no timer).
    pub sheet_changed: Color32,

    // ---- the File view (backstage)
    /// The File view's narrow rail.
    pub rail_fill: Color32,
    /// The File view's rail text.
    pub rail_text: Color32,
    /// The text of a rail entry that does nothing yet.
    pub rail_text_disabled: Color32,
    /// A rail entry while the pointer is over it.
    pub rail_hover: Color32,
    /// The rail entry of the page that is open.
    pub rail_active: Color32,
    /// The File view's body.
    pub backstage_fill: Color32,
    /// A gallery tile in the File view.
    pub backstage_tile: Color32,

    // ---- the splash popup and About DiaCraft: text painted over the cover's own cream, not over a panel of the theme
    /// The text of the splash: the near-black of "DiaCraft" in the cover. **The same in both themes**, because the picture it lies on is: it is 4.5 to 1 against the cream
    /// at least (a test reads the picture).
    pub splash_ink: Color32,
    /// The licence links of the splash: the teal of "Diagramming software" in the cover, also the same in both themes, and underlined besides, so that a link is not told by
    /// its colour alone.
    pub splash_link: Color32,
    /// The paper of the splash: the cream of the cover, **the same in both themes**. A panel whose picture has not come yet (the web build fetches it) is this colour, so that the text
    /// painted over the picture's lower left corner is read on what it will be read on.
    pub splash_paper: Color32,
}

impl Tokens {
    /// The dark theme.
    pub const fn dark() -> Tokens {
        Tokens {
            top_bar_fill: Color32::from_gray(32),
            top_bar_text: Color32::from_gray(235),
            control_idle_fill: Color32::TRANSPARENT,
            top_bar_hover: Color32::from_gray(58),
            close_hover_fill: Color32::from_rgb(168, 32, 92),
            close_hover_icon: Color32::from_rgb(255, 255, 255),
            status_bar_fill: Color32::from_gray(36),
            status_bar_text: Color32::from_gray(205),
            notice_text: Color32::from_rgb(255, 196, 120),
            separator: Color32::from_gray(64),
            scrim: Color32::from_black_alpha(150),

            text: Color32::from_gray(235),
            text_dim: Color32::from_gray(180),
            text_disabled: Color32::from_gray(125),
            accent: Color32::from_rgb(38, 166, 182),
            accent_text: Color32::from_rgb(10, 24, 28),
            accent_soft: Color32::from_rgb(34, 70, 78),
            accent_hover: Color32::from_rgb(70, 190, 204),

            ribbon_fill: Color32::from_gray(43),
            ribbon_border: Color32::from_gray(68),
            group_divider: Color32::from_gray(61),
            hover: Color32::from_gray(61),
            pressed: Color32::from_gray(76),
            input_fill: Color32::from_gray(28),
            input_border: Color32::from_gray(88),

            icon_ink: Color32::from_gray(220),
            icon_accent: Color32::from_rgb(64, 190, 206),
            icon_fill: Color32::from_gray(76),
            icon_ink_disabled: Color32::from_gray(125),
            icon_fill_disabled: Color32::from_gray(56),
            stencil_ink: Color32::from_rgb(187, 199, 211),
            stencil_ink_disabled: Color32::from_rgb(102, 113, 125),
            stencil_fill: Color32::TRANSPARENT,
            image_tint: Color32::WHITE,
            image_tint_disabled: Color32::from_gray(110),

            panel_fill: Color32::from_gray(35),
            panel_header_fill: Color32::from_gray(43),
            tile_fill: Color32::from_gray(45),
            tile_border: Color32::from_gray(75),

            canvas_fill: Color32::from_gray(24),
            page_fill: Color32::from_gray(255),
            page_border: Color32::from_gray(140),
            page_shadow: Color32::from_rgba_premultiplied(0, 0, 0, 90),
            picture_tint: Color32::WHITE,
            page_break: Color32::from_rgb(214, 96, 96),
            grid_line: Color32::from_gray(238),
            grid_major: Color32::from_gray(220),
            ruler_fill: Color32::from_gray(38),
            ruler_tick: Color32::from_gray(145),
            ruler_tick_minor: Color32::from_gray(94),
            ruler_text: Color32::from_gray(190),
            ruler_page_fill: Color32::from_gray(51),

            edit_line: Color32::from_rgb(38, 166, 182),
            edit_fill: Color32::from_rgba_premultiplied(6, 27, 29, 41),
            handle_fill: Color32::from_rgb(255, 255, 255),
            net_fill: Color32::from_rgba_premultiplied(76, 76, 76, 102),
            net_line: Color32::from_rgb(173, 173, 173),
            select_frame: Color32::from_rgb(64, 140, 226),
            select_box: Color32::from_rgb(166, 166, 166),
            glue_point: Color32::from_rgb(88, 98, 116),
            glue_on: Color32::from_rgb(40, 180, 90),
            glue_halo: Color32::from_rgba_premultiplied(10, 45, 22, 64),
            glue_off: Color32::from_rgb(255, 255, 255),
            bend_handle: Color32::from_rgb(81, 154, 239),
            callout_handle: Color32::from_rgb(255, 214, 64),
            guide_line: Color32::from_rgb(104, 152, 236),
            guide_selected: Color32::from_rgb(64, 190, 206),
            guide_halo: Color32::from_rgba_premultiplied(14, 42, 46, 72),
            snap_mark: Color32::from_rgb(244, 140, 72),
            auto_arrow: Color32::from_rgb(64, 148, 232),
            auto_arrow_hot: Color32::from_rgb(28, 104, 206),

            sheet_local: Color32::from_rgb(130, 190, 255),
            sheet_default: Color32::from_gray(165),
            sheet_error: Color32::from_rgb(255, 150, 140),
            sheet_changed: Color32::from_rgb(70, 58, 20),

            rail_fill: Color32::from_rgb(20, 92, 104),
            rail_text: Color32::from_rgb(236, 247, 249),
            rail_text_disabled: Color32::from_rgb(120, 168, 178),
            rail_hover: Color32::from_rgb(30, 110, 122),
            rail_active: Color32::from_rgb(40, 128, 142),
            backstage_fill: Color32::from_gray(33),
            backstage_tile: Color32::from_gray(45),

            splash_ink: Color32::from_rgb(10, 10, 12),
            splash_link: Color32::from_rgb(0, 115, 131),
            splash_paper: Color32::from_rgb(244, 235, 220),
        }
    }

    /// The light theme: the default.
    pub const fn light() -> Tokens {
        Tokens {
            top_bar_fill: Color32::from_gray(239),
            top_bar_text: Color32::from_gray(40),
            control_idle_fill: Color32::TRANSPARENT,
            top_bar_hover: Color32::from_gray(222),
            close_hover_fill: Color32::from_rgb(168, 32, 92),
            close_hover_icon: Color32::from_rgb(255, 255, 255),
            status_bar_fill: Color32::from_gray(239),
            status_bar_text: Color32::from_gray(64),
            notice_text: Color32::from_rgb(138, 62, 0),
            separator: Color32::from_gray(210),
            scrim: Color32::from_black_alpha(90),

            text: Color32::from_gray(40),
            text_dim: Color32::from_gray(96),
            text_disabled: Color32::from_gray(137),
            accent: Color32::from_rgb(0, 122, 140),
            accent_text: Color32::from_rgb(255, 255, 255),
            accent_soft: Color32::from_rgb(214, 237, 241),
            accent_hover: Color32::from_rgb(0, 100, 116),

            ribbon_fill: Color32::from_gray(249),
            ribbon_border: Color32::from_gray(210),
            group_divider: Color32::from_gray(224),
            hover: Color32::from_gray(232),
            pressed: Color32::from_gray(217),
            input_fill: Color32::from_gray(255),
            input_border: Color32::from_gray(190),

            icon_ink: Color32::from_gray(66),
            icon_accent: Color32::from_rgb(0, 128, 148),
            icon_fill: Color32::from_gray(232),
            icon_ink_disabled: Color32::from_gray(156),
            icon_fill_disabled: Color32::from_gray(240),
            stencil_ink: Color32::from_rgb(83, 97, 113),
            stencil_ink_disabled: Color32::from_rgb(170, 179, 189),
            stencil_fill: Color32::TRANSPARENT,
            image_tint: Color32::WHITE,
            image_tint_disabled: Color32::from_gray(170),

            panel_fill: Color32::from_gray(255),
            panel_header_fill: Color32::from_gray(247),
            tile_fill: Color32::from_gray(255),
            tile_border: Color32::from_gray(216),

            canvas_fill: Color32::from_gray(230),
            page_fill: Color32::from_gray(255),
            page_border: Color32::from_gray(196),
            page_shadow: Color32::from_rgba_premultiplied(0, 0, 0, 22),
            picture_tint: Color32::WHITE,
            page_break: Color32::from_rgb(190, 60, 60),
            grid_line: Color32::from_gray(238),
            grid_major: Color32::from_gray(220),
            ruler_fill: Color32::from_gray(247),
            ruler_tick: Color32::from_gray(134),
            ruler_tick_minor: Color32::from_gray(192),
            ruler_text: Color32::from_gray(86),
            ruler_page_fill: Color32::from_gray(255),

            edit_line: Color32::from_rgb(0, 122, 140),
            edit_fill: Color32::from_rgba_premultiplied(0, 17, 20, 36),
            handle_fill: Color32::from_rgb(255, 255, 255),
            net_fill: Color32::from_rgba_premultiplied(76, 76, 76, 102),
            net_line: Color32::from_rgb(173, 173, 173),
            select_frame: Color32::from_rgb(0, 84, 166),
            select_box: Color32::from_rgb(166, 166, 166),
            glue_point: Color32::from_rgb(88, 98, 116),
            glue_on: Color32::from_rgb(26, 148, 62),
            glue_halo: Color32::from_rgba_premultiplied(7, 37, 16, 64),
            glue_off: Color32::from_rgb(255, 255, 255),
            bend_handle: Color32::from_rgb(36, 118, 222),
            callout_handle: Color32::from_rgb(245, 190, 0),
            guide_line: Color32::from_rgb(52, 110, 214),
            guide_selected: Color32::from_rgb(0, 122, 140),
            guide_halo: Color32::from_rgba_premultiplied(0, 27, 31, 56),
            snap_mark: Color32::from_rgb(214, 88, 30),
            auto_arrow: Color32::from_rgb(46, 130, 220),
            auto_arrow_hot: Color32::from_rgb(0, 84, 176),

            sheet_local: Color32::from_rgb(0, 84, 166),
            sheet_default: Color32::from_gray(100),
            sheet_error: Color32::from_rgb(178, 34, 34),
            sheet_changed: Color32::from_rgb(255, 242, 190),

            rail_fill: Color32::from_rgb(0, 106, 122),
            rail_text: Color32::from_rgb(240, 249, 250),
            rail_text_disabled: Color32::from_rgb(126, 184, 194),
            rail_hover: Color32::from_rgb(0, 120, 136),
            rail_active: Color32::from_rgb(0, 136, 154),
            backstage_fill: Color32::from_gray(248),
            backstage_tile: Color32::from_gray(255),

            splash_ink: Color32::from_rgb(10, 10, 12),
            splash_link: Color32::from_rgb(0, 115, 131),
            splash_paper: Color32::from_rgb(244, 235, 220),
        }
    }

    /// The tokens of the egui theme the window follows.
    pub const fn for_theme(theme: Theme) -> Tokens {
        match theme {
            Theme::Dark => Tokens::dark(),
            Theme::Light => Tokens::light(),
        }
    }

    /// The tokens of the theme `ctx` is showing now.
    pub fn get(ctx: &egui::Context) -> Tokens {
        Tokens::for_theme(ctx.theme())
    }

    /// An egui `Visuals` for the widgets egui draws itself (menus, text fields, sliders, tooltips), so that they
    /// match the rest of the window: built from the tokens and from nothing else.
    pub fn visuals(&self, theme: Theme) -> egui::Visuals {
        let mut v = match theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        };
        v.override_text_color = Some(self.text);
        v.panel_fill = self.ribbon_fill;
        v.window_fill = self.panel_fill;
        v.extreme_bg_color = self.input_fill;
        v.faint_bg_color = self.panel_header_fill;
        v.window_stroke = egui::Stroke::new(1.0, self.separator);
        v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: self.page_shadow };
        v.selection.bg_fill = self.accent_soft;
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.hyperlink_color = self.accent;
        let widgets = &mut v.widgets;
        widgets.noninteractive.bg_fill = self.panel_fill;
        widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.separator);
        widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.text);
        widgets.inactive.bg_fill = self.input_fill;
        widgets.inactive.weak_bg_fill = self.control_idle_fill;
        widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.input_border);
        widgets.inactive.fg_stroke = egui::Stroke::new(1.0, self.text);
        widgets.hovered.bg_fill = self.hover;
        widgets.hovered.weak_bg_fill = self.hover;
        widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.accent);
        widgets.hovered.fg_stroke = egui::Stroke::new(1.0, self.text);
        widgets.active.bg_fill = self.pressed;
        widgets.active.weak_bg_fill = self.pressed;
        widgets.active.bg_stroke = egui::Stroke::new(1.0, self.accent);
        widgets.active.fg_stroke = egui::Stroke::new(1.0, self.text);
        widgets.open.bg_fill = self.accent_soft;
        widgets.open.weak_bg_fill = self.accent_soft;
        widgets.open.fg_stroke = egui::Stroke::new(1.0, self.text);
        v
    }
}

/// Makes `ctx` follow `theme` (and not the desktop's), with the visuals and spacing this window uses.
pub fn apply(ctx: &egui::Context, theme: Theme) {
    ctx.set_visuals_of(Theme::Light, Tokens::light().visuals(Theme::Light));
    ctx.set_visuals_of(Theme::Dark, Tokens::dark().visuals(Theme::Dark));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.menu_margin = egui::Margin::same(4);
    });
    ctx.set_theme(egui::ThemePreference::from(theme));
}

/// Text sizes, in points.
pub mod size {
    /// A ribbon button's label.
    pub const LABEL: f32 = 12.5;
    /// A group's name under its controls.
    pub const GROUP: f32 = 11.5;
    /// A tab's name.
    pub const TAB: f32 = 13.0;
    /// Text in the title bar and the status bar.
    pub const BAR: f32 = 12.5;
}

/// Sizes of the marks a selection is drawn with, in screen points: they keep their size whatever the zoom. The figures are read from published
/// images and are approximate; each is kept here so that the look can be tuned in one place.
pub mod select {
    /// The width of the net's outline.
    pub const NET_LINE: f32 = 1.0;
    /// The width of the frame of a shape in a multiple selection, and of the primary one's: the primary's is twice as wide.
    pub const FRAME: f32 = 2.0;
    pub const FRAME_PRIMARY: f32 = 4.0;
    /// The width of the box line round a selection.
    pub const BOX_LINE: f32 = 1.0;
    /// The room between the combined box of several shapes and the boxes of the outermost (1.3 handle widths in the pictures that agree; the rule is
    /// open). A single shape's box has none.
    pub const BOX_MARGIN: f32 = 1.3 * HANDLE;
    /// A handle's outer width, ring included, and the ring's width (11 to 12 across with a one-pixel ring).
    pub const HANDLE: f32 = 12.0;
    pub const HANDLE_RING: f32 = 1.0;
    /// The rotation handle (in handle widths of 12: the glyph 1.6 to 2 across, the stem 1.5 to 1.9 long, the glyph's centre 2.9 to 3.2 above the box
    /// line): the glyph's outer width, the height of its centre above the box line, and the width of its ring and of the stem.
    pub const ROTATE_GLYPH: f32 = 21.0;
    pub const ROTATE_CENTRE: f32 = 36.0;
    pub const ROTATE_RING: f32 = 2.0;
    pub const ROTATE_STEM: f32 = 1.0;
    /// A glued connector end: a filled disc this wide with a white halo of the ring's width round it (365 look). A free end: a square this wide
    /// with a ring of that width (the 2016 look; no later one is pictured).
    pub const END_DISC: f32 = 9.0;
    pub const END_HALO: f32 = 1.0;
    pub const END_SQUARE: f32 = 6.0;
    /// A leg's handle: a filled blue disc this wide with a white halo of the ring's width round it (365 look: "a bend or middle handle is a filled blue disc about 9
    /// pixels"). A leg shorter than `BEND_LEG` on the screen has no handle, so that the handle does not touch the marks at the ends.
    pub const BEND: f32 = 9.0;
    pub const BEND_HALO: f32 = 1.0;
    pub const BEND_LEG: f32 = 24.0;
}

/// A colour of the drawing's own, as the person chose it or the document holds it: the swatches of a palette and the bar under a colour control.
/// It is not a colour of the window, so it is no token; it is made here, the one file that spells a colour.
pub fn document_colour(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A colour of the drawing with an opacity `a` (255 is opaque): a transparent fill, a stop of a gradient or a shadow in a swatch. Like [`document_colour`] it is the
/// drawing's colour and not the window's.
pub fn document_colour_alpha(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// A proportional font of `size` points.
pub fn font(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

/// A real stronger face when available, with the regular face as fallback.
pub fn font_strong(size: f32) -> egui::FontId {
    egui::FontId::new(size, crate::fonts::strong_family())
}
