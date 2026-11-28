//! Theme commands: the Design tab's Themes and Variants galleries. The rules are in `theme/apply.rs` and `docs/editing-commands.md`,
//! "Themes".
//!
//! A page is named by `page`; left out, it is the first page that is not a background. `scope` is `page` (the one
//! page, the default) or `all` (every page of the drawing, backgrounds too: "a theme's menu chooses the current page or all pages, and pages in one
//! file can differ").

use diacraft_color::Rgba;
use serde_json::{Value, json};

use super::params::{optional_whole, page_for};
use super::{CommandSpec, always, has_document, optional_text, required_text};
use crate::Session;
use crate::error::{Result, bad, failed, shown};
use crate::formatting::{Rgb, look_of};
use crate::theme::{self, Applied, CatalogueTheme, Choice, PreviewChoice, Selection, VARIANT_NAMES};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "theme.list",
            "Themes",
            ["Design", "Themes"],
            None,
            "page: whole number, optional, the id of the page whose theme is told (default: the first page that is not a background)",
            always,
            list
        ),
        cmd!(
            "theme.apply",
            "Apply Theme",
            ["Design", "Themes"],
            None,
            "theme: text, the id of a theme in theme.list (tidepool, fernwood, ...), or none for no theme; \
             variant: whole number, optional, 0 to 3, the variant of the theme's colours and styles (default 0); \
             scope: text, optional, page (the default) or all; \
             page: whole number, optional, the id of the page (default: the first page that is not a background), and not given with scope all",
            has_document,
            apply
        ),
        cmd!(
            "theme.variant",
            "Theme Variant",
            ["Design", "Variants"],
            None,
            "variant: whole number, optional, 0 to 3, the variant of both the colours and the styles of the theme the page has; \
             colors: whole number, optional, 0 to 3, a variant of the colours alone; \
             styles: whole number, optional, 0 to 3, a variant of the styles alone (give variant, colors or styles, and variant not with the others); \
             scope: text, optional, page (the default) or all; \
             page: whole number, optional, the id of the page (default: the first page that is not a background), and not given with scope all",
            has_document,
            variant
        ),
        cmd!(
            query "theme.colors",
            "Theme Colors",
            ["Home", "Shape Styles"],
            None,
            "page: whole number, optional, the id of the page whose theme's colours are told (default: the first page that is not a background)",
            has_document,
            colors
        ),
        cmd!(
            query "theme.preview",
            "Preview Theme",
            ["Design", "Themes"],
            None,
            "theme: text, optional, the id of a theme in theme.list, or none for no theme, as theme.apply takes it; \
             variant: whole number, optional, 0 to 3, with theme the variant of the theme's colours and styles, and without it a variant of the theme the page has; \
             colors: whole number, optional, 0 to 3, a variant of the colours alone (without theme); \
             styles: whole number, optional, 0 to 3, a variant of the styles alone (without theme); \
             scope: text, optional, page (the default) or all; \
             page: whole number, optional, the id of the page (default: the first page that is not a background), and not given with scope all; \
             ids: array of whole numbers, optional, shapes of the page whose fill, line and text colour are told as the preview shows them",
            has_document,
            preview
        ),
    ]
}

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// The theme as a gallery shows it: its palette, its typefaces and its four variants.
fn theme_json(t: &CatalogueTheme, in_drawing: bool) -> Value {
    let c = t.colours();
    let e = &t.def.entries;
    let variants: Vec<Value> = e
        .colour_variants
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, v)| {
            let colours: Vec<String> = v.colours.iter().flatten().map(|c| hex(*c)).collect();
            json!({ "index": i, "name": VARIANT_NAMES.get(i).copied().unwrap_or(""), "monotone": v.monotone, "colours": colours })
        })
        .collect();
    json!({
        "id": t.id,
        "name": t.name,
        "scheme": t.scheme(),
        "in_drawing": in_drawing,
        "colours": {
            "dark1": hex(c[0]), "light1": hex(c[1]), "dark2": hex(c[2]), "light2": hex(c[3]),
            "accents": [hex(c[4]), hex(c[5]), hex(c[6]), hex(c[7]), hex(c[8]), hex(c[9])],
            "hyperlink": hex(c[10]), "followed_hyperlink": hex(c[11]),
            "background": e.background.map(hex),
        },
        "fonts": { "heading": t.def.major_font, "body": t.def.minor_font },
        "variants": variants,
    })
}

/// What a page selects, as a command tells it: the theme by its id (`none`, or `custom` for a number the catalogue does not hold, as a drawing
/// from a file may), the variants, and the seven numbers.
fn selection_json(page: u32, sel: Selection) -> Value {
    let theme = if !sel.is_themed() {
        Value::String("none".to_owned())
    } else {
        match theme::by_scheme(sel.theme) {
            Some(t) if sel.numbers()[..5].iter().all(|n| *n == sel.theme) => Value::String(t.id.to_owned()),
            _ => Value::String("custom".to_owned()),
        }
    };
    let numbers: Vec<Value> = theme::SELECTORS.iter().zip(sel.numbers()).map(|(name, n)| json!({ "cell": name, "value": n })).collect();
    json!({
        "page": page,
        "theme": theme,
        "colors": sel.variation_colours,
        "styles": sel.variation_styles,
        "selection": numbers,
    })
}

/// The pages the command works on, from `scope` and `page`.
fn target_pages(s: &Session, p: &Value, cmd: &str) -> Result<(Vec<u32>, &'static str)> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let named = optional_whole(p, "page", cmd)?;
    let scope = optional_text(p, "scope", cmd)?;
    match scope.map(str::to_ascii_lowercase).as_deref() {
        None | Some("page") => {
            let id = match named {
                Some(id) if doc.page(id).is_some() => id,
                Some(id) => return Err(bad(cmd, format!("there is no page {id}"))),
                None => doc.first_foreground_page().map(|page| page.id).ok_or_else(|| bad(cmd, "the document has no pages"))?,
            };
            Ok((vec![id], "page"))
        }
        Some("all") => {
            if named.is_some() {
                return Err(bad(cmd, "give `page`, or scope all, not both"));
            }
            Ok((doc.pages.iter().map(|page| page.id).collect(), "all"))
        }
        Some(other) => Err(bad(cmd, format!("`scope` must be `page` or `all`, not `{}`", shown(other)))),
    }
}

/// The page the answer of a command that works on `pages` speaks of: the first of them that is not a background page, else the first of them (a file lists its background pages first, and a
/// drawing's theme is the theme of the page a person works on).
fn lead_page(doc: &diacraft_doc::Document, pages: &[u32]) -> Option<u32> {
    pages.iter().copied().find(|id| doc.page(*id).is_some_and(|page| !page.background)).or_else(|| pages.first().copied())
}

/// A variant place: 0 to 3.
fn variant_of(p: &Value, key: &str, cmd: &str) -> Result<Option<u8>> {
    match optional_whole(p, key, cmd)? {
        None => Ok(None),
        Some(n) if n <= 3 => Ok(Some(n as u8)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be 0 to 3, the places of the four variants"))),
    }
}

/// → `{themes, variants, page}`: the themes (`id`, `name`, `scheme`, `in_drawing`, `colours`, `fonts`, `variants`), the four variants' names, and the
/// theme the page has. `colours` holds the palette as `#rrggbb` (`dark1`, `light1`, `dark2`, `light2`, six `accents`, `hyperlink`,
/// `followed_hyperlink`, `background`), and each variant its seven `colours` and whether it is `monotone`. With no document open `page` is null.
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "theme.list";
    let doc = s.document();
    let themes: Vec<Value> = theme::catalogue().iter().map(|t| theme_json(t, doc.is_some_and(|d| d.theme_by_index(t.scheme()).is_some()))).collect();
    let page = match doc {
        None => Value::Null,
        Some(_) => {
            let (pages, _) = target_pages(s, p, ID)?;
            let id = pages.first().copied().ok_or_else(|| bad(ID, "the document has no pages"))?;
            let sel = s.document().and_then(|d| theme::selection_of(d, id)).ok_or_else(|| bad(ID, format!("there is no page {id}")))?;
            selection_json(id, sel)
        }
    };
    Ok(json!({ "themes": themes, "variants": VARIANT_NAMES, "page": page }))
}

fn applied_json(theme: &str, variant: u8, scope: &str, applied: &Applied) -> Value {
    json!({
        "theme": theme,
        "variant": variant,
        "scope": scope,
        "pages": applied.pages,
        "changed_pages": applied.changed_pages,
        "shapes_linked": applied.links_made,
        "cells_rewritten": applied.cells_rewritten,
        "theme_added": applied.part_added,
        "style_added": applied.style_added,
        "locked": applied.locked,
    })
}

/// Gives pages a theme, or none → `{theme, variant, scope, pages, changed_pages, shapes_linked, cells_rewritten, theme_added, style_added, locked}`:
/// the pages worked on and those that changed, how many times a shape began to follow the `Theme` style, how many scheme cells of shapes were
/// given the page's numbers, whether the theme's part and the style were added to the drawing, and how many shapes on a locked layer were passed over
/// (no link and no cell is written into them). A page that has the theme and variant, with every shape following it, is no undo step.
fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "theme.apply";
    let word = required_text(p, "theme", ID)?;
    let variant = variant_of(p, "variant", ID)?;
    let (pages, scope) = target_pages(s, p, ID)?;
    let (choice, name) = if word.eq_ignore_ascii_case("none") {
        if variant.is_some() {
            return Err(bad(ID, "`none` takes no variant"));
        }
        (Choice::None, "none".to_owned())
    } else {
        let found = theme::find(word).ok_or_else(|| bad(ID, format!("no theme `{}` (see theme.list)", shown(word))))?;
        (Choice::Theme(found, variant.unwrap_or(0)), found.id.to_owned())
    };
    // A drawing from an `.odg` bases the `Theme` style it makes on the file's default graphic style (`theme::Base`); any other drawing on the root.
    let base = if crate::odgfile::is_odg(s.source()) { theme::Base::FilesDefault } else { theme::Base::Root };
    s.edit_if(if matches!(choice, Choice::None) { "No theme" } else { "Apply theme" }, |doc| {
        let applied = theme::apply(doc, &pages, choice, base).map_err(|e| e.into_engine(ID))?;
        Ok((applied_json(&name, variant.unwrap_or(0), scope, &applied), applied.changed()))
    })
}

/// Picks a variant of the theme the pages have → as `theme.apply` answers, with the theme `custom` or its id as the pages hold it. A page with no
/// theme is an error, and with scope `all` the pages that have none are left out (and it is an error when no page has one).
fn variant(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "theme.variant";
    let (both, colours, styles) = (variant_of(p, "variant", ID)?, variant_of(p, "colors", ID)?, variant_of(p, "styles", ID)?);
    let (colours, styles) = match (both, colours, styles) {
        (Some(v), None, None) => (Some(v), Some(v)),
        (None, None, None) => return Err(bad(ID, "give `variant`, `colors` or `styles`")),
        (Some(_), _, _) => return Err(bad(ID, "`variant` goes alone: give it, or `colors` and `styles`")),
        (None, c, st) => (c, st),
    };
    let (mut pages, scope) = target_pages(s, p, ID)?;
    if scope == "all" {
        // Pages in one file can differ: every page that has a theme takes the variant, and one that has none is left as it is.
        let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
        pages.retain(|id| theme::selection_of(doc, *id).is_some_and(|sel| sel.is_themed()));
        if pages.is_empty() {
            return Err(failed(ID, "no page has a theme to give a variant"));
        }
    }
    s.edit_if("Theme variant", |doc| {
        let applied = theme::set_variants(doc, &pages, colours, styles).map_err(|e| e.into_engine(ID))?;
        let first = lead_page(doc, &pages).unwrap_or(0);
        let sel = theme::selection_of(doc, first).unwrap_or_default();
        let theme = selection_json(first, sel)["theme"].as_str().unwrap_or("custom").to_owned();
        let shown_variant = u8::try_from(sel.variation_colours).unwrap_or(0);
        Ok((applied_json(&theme, shown_variant, scope, &applied), applied.changed()))
    })
}

impl Session {
    /// A copy of the drawing with `request` applied to `page`, as `theme.apply` or `theme.variant` would apply it, and **nothing changed**: no command is run, so there is no undo step, no
    /// revision and no unsaved mark, and the redo stack stays. The window draws the copy while a tile is under the pointer (`theme/preview.rs`). A page past the preview's budget is
    /// refused in words.
    pub fn preview_theme(&self, page: u32, request: &theme::PreviewRequest) -> Result<theme::Preview> {
        const ID: &str = "theme.preview";
        let doc = self.document().ok_or_else(|| failed(ID, "no document open"))?;
        let choice = request.choice().map_err(|e| e.into_engine(ID))?;
        let base = if crate::odgfile::is_odg(self.source()) { theme::Base::FilesDefault } else { theme::Base::Root };
        theme::preview(doc, &[page], choice, base).map_err(|e| e.into_engine(ID))
    }
}

/// A colour as `#rrggbb` text.
fn hex_of(c: Rgb) -> String {
    c.hex()
}

/// One swatch of the palette as the answer tells it: the colour it works out to (`null` where the theme cannot answer), the words that name it, and what a command takes to pick it.
fn swatch_json(swatch: &theme::Swatch) -> Value {
    json!({
        "label": swatch.pick.label(),
        "tooltip": swatch.tooltip(),
        "hex": swatch.colour.map(hex_of),
        "pick": swatch.pick.word(),
        "tint": swatch.pick.tint,
        "formula": swatch.pick.formula(),
    })
}

/// The colours of a page's theme as a palette offers them → `{page, themed, theme_colors, variant_colors, standard_colors}`. `theme_colors` is the columns left to right
/// (`column`, 1 to 10, `slot`, `label`, and its six swatches: the colour, then its shades: ten columns, the third and fourth repeating the first two), `variant_colors` the seven
/// colours of the page's colour variant, and `standard_colors` the ten fixed
/// colours. A swatch holds `label`, `tooltip` (colour name, theme colour, shade), `hex` (what the formula works out to on this page), `pick` (what `shape.format` and
/// `text.format` take to put it on: `theme:accent2:40`), `tint` and `formula` (what the cell then holds). With no theme on the page `themed` is false and the
/// colours are the table `THEMEVAL` gives with no theme. Nothing is changed.
fn colors(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "theme.colors";
    let (pages, _) = target_pages(s, p, ID)?;
    let page = pages.first().copied().ok_or_else(|| bad(ID, "the document has no pages"))?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let palette = theme::colours(doc, page).map_err(|e| e.into_engine(ID))?;
    let columns: Vec<Value> = palette
        .columns
        .iter()
        .map(|c| {
            json!({ "column": c.column, "slot": c.slot.id(), "label": c.slot.label(), "swatches": c.swatches.iter().map(swatch_json).collect::<Vec<_>>() })
        })
        .collect();
    let standard: Vec<Value> = theme::STANDARD_COLOURS.iter().map(|(name, c)| json!({ "label": name, "hex": hex_of(*c) })).collect();
    Ok(json!({
        "page": palette.page,
        "themed": palette.themed,
        "theme_colors": columns,
        "variant_colors": palette.variants.iter().map(swatch_json).collect::<Vec<_>>(),
        "standard_colors": standard,
    }))
}

/// What a preview shows of the shapes `ids` of a page: their fill, line and text colour as `#rrggbb`, or `none`.
fn looks_json(doc: &diacraft_doc::Document, page: u32, ids: &[u32]) -> Result<Value> {
    const ID: &str = "theme.preview";
    let looks = look_of(doc, page, ids).map_err(|e| e.into_engine(ID))?;
    let paint = |c: Option<Rgb>| c.map_or_else(|| "none".to_owned(), hex_of);
    Ok(Value::Array(looks.iter().map(|l| json!({ "id": l.id, "fill": paint(l.fill), "line": paint(l.line), "color": hex_of(l.color) })).collect()))
}

/// Works out what a theme or a variant would make of the page, **on a copy, and changes nothing** → as `theme.apply` answers, with `preview` true and, for `ids`, `shapes`: their
/// colours in the copy. No undo step, no new revision, no unsaved mark: the window draws the copy while a tile is under the pointer (`theme/preview.rs`). A page
/// of more than `MAX_PREVIEW_SHAPES` shapes is refused in words.
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "theme.preview";
    let (both, colours, styles) = (variant_of(p, "variant", ID)?, variant_of(p, "colors", ID)?, variant_of(p, "styles", ID)?);
    let theme_word = optional_text(p, "theme", ID)?;
    let (pages, scope) = target_pages(s, p, ID)?;
    let (choice, name, shown_variant) = match theme_word {
        Some(word) if word.eq_ignore_ascii_case("none") => {
            if both.is_some() || colours.is_some() || styles.is_some() {
                return Err(bad(ID, "`none` takes no variant"));
            }
            (PreviewChoice::Apply(Choice::None), "none".to_owned(), 0)
        }
        Some(word) => {
            if colours.is_some() || styles.is_some() {
                return Err(bad(ID, "with `theme`, give the variant as `variant`"));
            }
            let found = theme::find(word).ok_or_else(|| bad(ID, format!("no theme `{}` (see theme.list)", shown(word))))?;
            (PreviewChoice::Apply(Choice::Theme(found, both.unwrap_or(0))), found.id.to_owned(), both.unwrap_or(0))
        }
        None => {
            let (c, st) = match (both, colours, styles) {
                (Some(v), None, None) => (Some(v), Some(v)),
                (None, None, None) => return Err(bad(ID, "give `theme`, `variant`, `colors` or `styles`")),
                (Some(_), _, _) => return Err(bad(ID, "`variant` goes alone: give it, or `colors` and `styles`")),
                (None, c, st) => (c, st),
            };
            (PreviewChoice::Variants { colours: c, styles: st }, "variants".to_owned(), c.or(st).unwrap_or(0))
        }
    };
    let ids: Vec<u32> = match p.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => super::params::required_ids(p, "ids", ID)?,
    };
    let base = if crate::odgfile::is_odg(s.source()) { theme::Base::FilesDefault } else { theme::Base::Root };
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let made = theme::preview(doc, &pages, choice, base).map_err(|e| e.into_engine(ID))?;
    let mut out = applied_json(&name, shown_variant, scope, &made.applied);
    if let Some(map) = out.as_object_mut() {
        map.insert("preview".to_owned(), Value::Bool(true));
        if !ids.is_empty() {
            let page = page_for(&made.document, lead_page(&made.document, &pages), &ids, ID)?;
            map.insert("shapes".to_owned(), looks_json(&made.document, page, &ids)?);
        }
    }
    Ok(out)
}
