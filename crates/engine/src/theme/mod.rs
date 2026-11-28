//! Themes: the themes DiaCraft ships, and applying one to a page or to every page, with its variants.
//!
//! * [`catalogue`]: five themes of our own (**originals**, `docs/provenance.md`), each with four colour variants and four style variants, as
//!   definitions (`diacraft_vsdx::ThemeDef`) that `diacraft_vsdx::theme_part_bytes` makes into theme parts (`kit.rs` holds the lists they are
//!   made of).
//! * [`apply`]: a theme or "no theme" to pages, and a variant: the theme's part joins the drawing, the page's seven selector cells are written,
//!   the shapes follow the `Theme` style ([`style`]), and the page is worked out again (`apply.rs` says what, and why the whole page). **A shape that stands for a shape of a
//!   master follows the style through its master** (`masters.rs`): the master's shape is linked and the instance keeps no link of its own.
//! * The commands are in `cmd/theme.rs`: `theme.list`, `theme.apply` and `theme.variant`.
//!
//! **Quick Styles** (`formatting/quick.rs`): `shape.quick_style` picks an entry of the theme's lists and a colour for chosen shapes, and
//! `shape.quick_styles` lists the gallery.
//!
//! **What this does not do** (and what a later task could): swapping one component of a theme (the colours, the effects, the connectors or the fonts of
//! another), and removing the theme from chosen shapes. A shape put on a page that has a theme follows it in the same undo step ([`follow_page_theme`]).

mod apply;
mod catalogue;
mod kit;
mod masters;
mod new_colours;
mod palette;
mod preview;
mod style;

pub use apply::{
    Applied, Choice, FollowWork, MAX_FOLLOWED_SHAPES, MAX_THEMED_SHAPES, SELECTORS, Selection, apply, apply_within, follow_page_theme,
    follow_page_theme_refs, follow_work, reset_follow_work, selection_of, set_variants, set_variants_within,
};
pub use catalogue::{CatalogueTheme, VARIANT_NAMES, by_scheme, catalogue, find};
pub use palette::{
    COLUMNS, Column, MAX_TINT, PICK_PREFIX, Palette, STANDARD_COLOURS, Swatch, ThemePick, ThemeSlot, colour_name, colours, evaluate_picks,
    pick_colour, shade_label, shades,
};
pub use preview::{MAX_PREVIEW_SHAPES, PREVIEW_STEPS, Preview, PreviewChoice, Request as PreviewRequest, preview};
pub use style::{Base, DEFAULT_GRAPHIC_STYLE_NAME, THEME_STYLE_NAME, default_graphic_style_id, theme_style_id};
