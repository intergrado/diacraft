//! The picture of a tile of the callout gallery (ours).
//!
//! The gallery's tiles show the shapes themselves, as the specifications say ("14 tiles ... each shows the word Text"). A tile is **the callout the command makes, drawn by the renderer**:
//! the callout of the style is added to a drawing of its own (a blank one, a page of 2.4 by 1.4 inches, nothing tied, the end of the leader at its default place), the page is recalculated, and the page is drawn
//! on white paper. So a tile can never show a figure the command does not make, and a tile that fails to draw is a bug in a tile's geometry that a test finds. The drawing is thrown away; the session's
//! document is not touched.

use diacraft_doc::Document;
use diacraft_render::export::unpremultiply;
use diacraft_render::{Renderer, View};

use super::callouts::add_callouts;
use super::page_setup::{SizeChoice, set_size};
use super::{Problem, settle};
use crate::stencil::CalloutStyle;

/// The size of the page a tile is drawn from, in inches.
pub const PREVIEW_PAGE: (f64, f64) = (2.4, 1.4);

/// The pixels to a paper inch a picture may be drawn at (a whole number of pixels to each side of the page when it is a multiple of 5: 50 gives 120 by 70).
pub const PREVIEW_PPI: std::ops::RangeInclusive<f64> = 8.0..=240.0;

/// A tile's picture: `width` by `height` pixels of straight (not premultiplied) RGBA, row by row from the top, on white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn problem(p: Problem) -> String {
    p.message
}

/// The picture of the tile `style` at `ppi` pixels to an inch. An error in words when the tile cannot be drawn.
pub fn preview(style: &CalloutStyle, ppi: f64) -> Result<Preview, String> {
    if !(ppi.is_finite() && PREVIEW_PPI.contains(&ppi)) {
        return Err(format!("a tile is drawn at {} to {} pixels to the inch", PREVIEW_PPI.start(), PREVIEW_PPI.end()));
    }
    let mut doc = Document::new_blank();
    let page = doc.pages.iter().next().map(|p| p.id).ok_or("the blank drawing has no page")?;
    set_size(&mut doc, page, SizeChoice::Custom { width: PREVIEW_PAGE.0, height: PREVIEW_PAGE.1 }).map_err(problem)?;
    let before = doc.clone();
    // The middle of the box is placed so that the end of the leader, which is below and to the left of it, is on the page.
    let k = super::units_per_inch(&doc, page);
    add_callouts(&mut doc, page, style, &[], Some((1.3 * k, 0.95 * k)), crate::stencil::CALLOUT_TEXT, 0).map_err(problem)?;
    settle(&mut doc, &before, page, &[1], &[]).map_err(problem)?;
    let view = View::fit_page(&doc, page, ppi).ok_or("the page cannot be drawn at that size")?;
    let mut renderer = Renderer::with_shared_fonts(crate::measure::renderer_fonts());
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let rendered = renderer.render(&doc, page, &view);
    if rendered.rgba_premul.is_empty() {
        return Err("the tile could not be drawn".to_owned());
    }
    Ok(Preview { width: rendered.width, height: rendered.height, rgba: unpremultiply(&rendered.rgba_premul) })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::stencil::CALLOUT_STYLES;

    /// How many pixels are not white.
    fn inked(p: &Preview) -> usize {
        p.rgba.as_chunks::<4>().0.iter().filter(|px| px[..3].iter().any(|c| *c < 200)).count()
    }

    #[test]
    fn every_tile_draws_ink_on_white_and_no_two_tiles_are_the_same_picture() {
        let pictures: Vec<Preview> = CALLOUT_STYLES.iter().map(|s| preview(s, 50.0).unwrap_or_else(|e| panic!("{}: {e}", s.id))).collect();
        for (style, picture) in CALLOUT_STYLES.iter().zip(&pictures) {
            assert_eq!((picture.width, picture.height), (120, 70), "{}", style.id);
            assert_eq!(picture.rgba.len(), 120 * 70 * 4);
            assert!(inked(picture) > 30, "{}: the tile shows something: {} pixels are ink", style.id, inked(picture));
            assert!(picture.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 255), "{}: the paper is opaque", style.id);
            // The corners are paper.
            assert_eq!(&picture.rgba[..4], &[255, 255, 255, 255], "{}", style.id);
        }
        for (i, a) in pictures.iter().enumerate() {
            for (j, b) in pictures.iter().enumerate().skip(i + 1) {
                assert_ne!(a.rgba, b.rgba, "{} and {} are drawn alike", CALLOUT_STYLES[i].id, CALLOUT_STYLES[j].id);
            }
        }
    }

    /// To look at the tiles: `CALLOUT_SHEET=/absolute/sheet.png cargo test -p diacraft-engine --lib -- --ignored a_contact_sheet`. Writes the fourteen tiles in rows of four, two pixels apart.
    #[test]
    #[ignore = "writes a picture to the path in CALLOUT_SHEET"]
    fn a_contact_sheet_of_the_fourteen_tiles_is_written_where_the_environment_says() {
        let Some(path) = std::env::var_os("CALLOUT_SHEET") else { return };
        let (w, h, gap, per_row) = (120_usize, 70_usize, 2_usize, 4_usize);
        let rows = CALLOUT_STYLES.len().div_ceil(per_row);
        let (sheet_w, sheet_h) = (per_row * (w + gap), rows * (h + gap));
        let mut rgba = vec![160_u8; sheet_w * sheet_h * 4];
        for (n, style) in CALLOUT_STYLES.iter().enumerate() {
            let tile = preview(style, 50.0).unwrap();
            let (x0, y0) = ((n % per_row) * (w + gap), (n / per_row) * (h + gap));
            for y in 0..h {
                let from = y * w * 4;
                let to = ((y0 + y) * sheet_w + x0) * 4;
                rgba[to..to + w * 4].copy_from_slice(&tile.rgba[from..from + w * 4]);
            }
        }
        let png = diacraft_render::export::encode_png(sheet_w as u32, sheet_h as u32, &rgba, None).unwrap();
        std::fs::write(path, png).unwrap();
    }

    #[test]
    fn a_size_that_is_not_a_number_or_is_out_of_range_is_refused_in_words() {
        for bad in [f64::NAN, f64::INFINITY, 0.0, -3.0, 7.9, 240.1, 1.0e9] {
            let e = preview(&CALLOUT_STYLES[0], bad).unwrap_err();
            assert!(e.contains("pixels to the inch"), "{bad}: {e}");
        }
    }
}
