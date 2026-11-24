//! A theme shown on a page without being applied ("pointing at a tile previews it on the page", R19 for the layouts).
//!
//! **A preview is not an edit.** [`preview`] takes the drawing as it is and gives back **a copy** with the theme or the variant applied to it, by the same code [`apply`]
//! and [`set_variants`] run, so what it shows is what a press on the tile would make. The session's drawing is not touched: no command is run, so there is no undo step, no new
//! revision and no unsaved mark, and the redo stack is as it was (a theme applied for real is an edit, and an edit clears it). A drawing is a handful of shared lists, so the copy
//! costs a handful of pointers, and applying the theme copies only the shapes it links and the cells it writes (the cost test is `tests/it/theme_color_cost.rs`).
//!
//! **Bounded** (ours): the copy is made only for pages that hold at most [`MAX_PREVIEW_SHAPES`] shapes in all, counted (stopping at the cap) **before anything is
//! copied**, and its recalculation is held to [`PREVIEW_STEPS`] steps (`apply` has a hundred times that). A page past either is refused in words, and the window shows no preview for
//! it: a preview is made again each time the pointer enters a tile, and a page of a hundred thousand shapes would make each entry a long wait for a picture nobody asked for.

use diacraft_calc::Budget;
use diacraft_doc::Document;

use super::apply::{Applied, Choice, apply_within, set_variants_within};
use super::style::Base;
use crate::draw::Problem;

/// The most shapes, groups' members counted, of the pages one preview is made for.
pub const MAX_PREVIEW_SHAPES: usize = 5_000;

/// The steps `calc` may take to work the previewed pages out: a tenth of [`Budget::DEFAULT_STEPS`]. A rectangle takes about 180 steps and a page of [`MAX_PREVIEW_SHAPES`] of them 900,000, so a
/// page of the shapes that have more cells still comes within it; **the copy is made on the window's own thread** at each tile the pointer enters, so the budget is what a person may wait for
/// (about a fifth of a second for this many steps), and a page past it is refused in words ([`preview`]).
pub const PREVIEW_STEPS: u64 = Budget::DEFAULT_STEPS / 10;

/// What a preview shows.
#[derive(Clone, Copy, Debug)]
pub enum PreviewChoice {
    /// A theme and its variant, or no theme.
    Apply(Choice),
    /// A variant of the theme the pages have: the colours' place and the styles' (0 to 3), each optional.
    Variants { colours: Option<u8>, styles: Option<u8> },
}

/// What a window asks to be previewed: the tile under the pointer. It is a plain value, so that the window can tell whether the tile it shows is the one it asked for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Request {
    /// A theme of the catalogue by its id, and the variant (0 to 3) of its colours and styles.
    Theme { id: String, variant: u8 },
    /// No theme.
    NoTheme,
    /// A variant of the theme the page has: the colours' place and the styles' (0 to 3), each optional.
    Variants { colours: Option<u8>, styles: Option<u8> },
}

impl Request {
    /// What the request asks `preview` to apply, or why it cannot be: a theme the catalogue lacks, a variant past the fourth.
    pub fn choice(&self) -> Result<PreviewChoice, Problem> {
        let place = |n: u8| if n <= 3 { Ok(n) } else { Err(Problem::bad("a variant is 0 to 3, the places of the four variants")) };
        match self {
            Request::Theme { id, variant } => {
                let found =
                    super::catalogue::find(id).ok_or_else(|| Problem::bad(format!("no theme `{}` (see theme.list)", crate::error::shown(id))))?;
                Ok(PreviewChoice::Apply(Choice::Theme(found, place(*variant)?)))
            }
            Request::NoTheme => Ok(PreviewChoice::Apply(Choice::None)),
            Request::Variants { colours, styles } => {
                colours.map(place).transpose()?;
                styles.map(place).transpose()?;
                Ok(PreviewChoice::Variants { colours: *colours, styles: *styles })
            }
        }
    }
}

/// A drawing with a theme applied to it, and what applying it did.
pub struct Preview {
    pub document: Document,
    pub applied: Applied,
}

/// The shapes of page `page`, at most `limit + 1` of them counted (a page past the limit stops the walk at once). `None` for a page that is not there.
fn shapes_up_to(doc: &Document, page: u32, limit: usize) -> Option<usize> {
    let mut stack = vec![&doc.page(page)?.shapes];
    let mut counted = 0_usize;
    while let Some(list) = stack.pop() {
        for shape in list.iter() {
            counted = counted.saturating_add(1);
            if counted > limit {
                return Some(counted);
            }
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
    Some(counted)
}

/// A copy of `doc` with `choice` applied to `pages`, as `theme.apply` or `theme.variant` would apply it, or why not. `base` is what a `Theme` style the drawing lacks is based on
/// (`apply`'s).
pub fn preview(doc: &Document, pages: &[u32], choice: PreviewChoice, base: Base) -> Result<Preview, Problem> {
    let mut held = 0_usize;
    for page in pages {
        let room = MAX_PREVIEW_SHAPES.saturating_sub(held);
        let count = shapes_up_to(doc, *page, room).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
        held = held.saturating_add(count);
        if held > MAX_PREVIEW_SHAPES {
            return Err(Problem::failed(format!(
                "the pages hold more than {MAX_PREVIEW_SHAPES} shapes, so no preview is made: apply the theme to see it"
            )));
        }
    }
    let budget = Budget::with_steps(PREVIEW_STEPS);
    let mut document = doc.clone();
    let applied = match choice {
        PreviewChoice::Apply(choice) => apply_within(&mut document, pages, choice, base, &budget),
        PreviewChoice::Variants { colours, styles } => set_variants_within(&mut document, pages, colours, styles, &budget),
    }
    // The recalculation ran out of its budget: the same refusal, in the words of a preview.
    .map_err(|problem| {
        if problem.message.contains("too large to recalculate") {
            Problem::failed("the pages are too large to preview in the time a preview may take: apply the theme to see it")
        } else {
            problem
        }
    })?;
    Ok(Preview { document, applied })
}
