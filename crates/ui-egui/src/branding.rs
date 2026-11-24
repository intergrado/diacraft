//! Approved application artwork, separate from the hostile stencil-image decoder.
//! The icon is a mechanical 256-pixel export; the cover retains its original pixels; the splash is the cover at half its size, in at most 256 colours.
//!
//! **The web build carries none of the three** (`asset_host.rs`): the `include_bytes!` lines are for the desktop programs (`cfg(not(target_arch = "wasm32"))`), and the
//! window of the browser asks its [`AssetHost`](crate::asset_host::AssetHost) for the splash picture and the cover, and shows a placeholder until they come (or the reason they did
//! not). The icon is the page's own (`<link rel="icon">`): the browser has no window icon to set.

use std::io::Cursor;

use egui::{ColorImage, Context, Rect, TextureHandle, TextureOptions, Ui, pos2};

use crate::app::DiacraftApp;
use crate::asset_host::{Asset, Fetch, Source};

#[cfg(not(target_arch = "wasm32"))]
const ICON: &[u8] = include_bytes!("../../../assets/branding/diacraft-icon-256.png");
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const COVER: &[u8] = include_bytes!("../../../assets/branding/diacraft-cover.png");
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SPLASH: &[u8] = include_bytes!("../../../assets/branding/diacraft-splash.png");
const COVER_WIDTH: usize = 1672;
const COVER_HEIGHT: usize = 941;
/// The splash copy's size in pixels: the cover's, halved (`docs/branding.md` says how it is made).
pub const SPLASH_WIDTH: usize = 836;
pub const SPLASH_HEIGHT: usize = 470;

/// The application's window icon, in the straight RGBA format the native window system takes. The desktop programs only: the browser's icon is the page's.
#[cfg(not(target_arch = "wasm32"))]
pub fn window_icon() -> Result<egui::IconData, String> {
    let rgba = decode(ICON, 256, 256)?;
    Ok(egui::IconData { rgba, width: 256, height: 256 })
}

/// Read only the dimensions and colour encodings of our bundled artwork. This path is never used for
/// imported files, and cannot increase the stencil decoder's allocation limits.
fn decode(bytes: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: 16 * 1024 * 1024 });
    // The splash copy is a palette picture (256 colours): the palette is turned into RGB as it is read. Nothing changes for a picture that has none.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| format!("could not read artwork: {e}"))?;
    let info = reader.info();
    if usize::try_from(info.width) != Ok(width) || usize::try_from(info.height) != Ok(height) || info.bit_depth != png::BitDepth::Eight {
        return Err("bundled artwork has unexpected dimensions or bit depth".into());
    }
    let rgba_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= COVER_WIDTH * COVER_HEIGHT * 4)
        .ok_or_else(|| "bundled artwork exceeds its pixel budget".to_owned())?;
    let len = reader.output_buffer_size().filter(|n| *n <= rgba_len).ok_or_else(|| "bundled artwork exceeds its decode budget".to_owned())?;
    let mut buffer = vec![0; len];
    let frame = reader.next_frame(&mut buffer).map_err(|e| format!("could not decode artwork: {e}"))?;
    let data = buffer.get(..frame.buffer_size()).ok_or_else(|| "bundled artwork has incomplete pixels".to_owned())?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|pixel| pixel.iter().copied().chain(std::iter::once(255))).collect(),
        _ => return Err("bundled artwork has an unsupported colour encoding".into()),
    };
    if rgba.len() != rgba_len {
        return Err("bundled artwork has incomplete pixels".into());
    }
    Ok(rgba)
}

/// What the window has of a picture.
#[derive(Clone)]
pub(crate) enum Picture {
    /// Not here yet (the host is fetching it): the window shows a placeholder, and the host wakes it when the picture comes.
    Waiting,
    /// Decoded into a texture.
    Ready(TextureHandle),
    /// It could not be had or could not be decoded: the reason.
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Artwork {
    cover: Option<Result<TextureHandle, String>>,
    splash: Option<Result<TextureHandle, String>>,
    /// Whether the person was told that the cover could not be had (once: the File view is drawn every frame it is open).
    cover_said: bool,
}

fn picture_texture(ctx: &Context, name: &str, bytes: &[u8], width: usize, height: usize) -> Result<TextureHandle, String> {
    let rgba = decode(bytes, width, height)?;
    let pixels = crate::icon_image::Pixels { width, height, rgba };
    Ok(ctx.load_texture(name, ColorImage::from_rgba_premultiplied([width, height], &pixels.premultiplied()), TextureOptions::LINEAR))
}

/// The picture in `slot`: made when it is first asked for and its bytes are there, and kept, so that a picture that is waiting costs a look at the host and one that failed (the fetch or the
/// decode) says so once and is not tried again every frame. A picture decoded lets the host go of its bytes (the texture holds the pixels).
fn picture(slot: &mut Option<Result<TextureHandle, String>>, ctx: &Context, source: &Source, asset: Asset, size: (usize, usize)) -> Picture {
    if let Some(done) = slot {
        return match done {
            Ok(texture) => Picture::Ready(texture.clone()),
            Err(error) => Picture::Failed(error.clone()),
        };
    }
    let made = match source.get(ctx, asset) {
        Fetch::Waiting => return Picture::Waiting,
        Fetch::Failed(error) => Err(error),
        Fetch::Ready(bytes) => picture_texture(ctx, &format!("diacraft-{}", asset.key()), bytes.as_slice(), size.0, size.1),
    };
    source.taken(asset);
    let shown = match &made {
        Ok(texture) => Picture::Ready(texture.clone()),
        Err(error) => Picture::Failed(error.clone()),
    };
    *slot = Some(made);
    shown
}

impl Artwork {
    /// The splash picture, made when it is first asked for.
    pub(crate) fn splash_picture(&mut self, ctx: &Context, source: &Source) -> Picture {
        picture(&mut self.splash, ctx, source, Asset::Splash, (SPLASH_WIDTH, SPLASH_HEIGHT))
    }

    /// The cover, made when it is first asked for.
    pub(crate) fn cover_picture(&mut self, ctx: &Context, source: &Source) -> Picture {
        picture(&mut self.cover, ctx, source, Asset::Cover, (COVER_WIDTH, COVER_HEIGHT))
    }
}

#[cfg(test)]
impl Artwork {
    /// Makes the splash picture a decode that failed with `why`: for the test of what the window does with a picture it cannot read.
    pub(crate) fn break_splash_for_test(&mut self, why: &str) {
        self.splash = Some(Err(why.to_owned()));
    }
}

impl DiacraftApp {
    /// Cache even a failed decode, so a broken bundled image reports once without retrying every frame. **A cover that has not come yet** (the web build fetches it when the File view first
    /// shows it) **is a placeholder with a short wait in it**, and one that could not be had is the placeholder with the reason: the File view works without it.
    pub(crate) fn paint_cover(&mut self, ui: &Ui, rect: Rect, t: &crate::theme::Tokens) {
        let source = self.asset_source();
        match self.artwork.cover_picture(ui.ctx(), &source) {
            Picture::Ready(texture) => {
                ui.painter().image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), t.image_tint);
            }
            Picture::Waiting => paint_picture_placeholder(ui, rect, t, COVER_WAIT),
            Picture::Failed(error) => {
                if !self.artwork.cover_said {
                    self.artwork.cover_said = true;
                    let message = format!("DiaCraft artwork could not load: {error}");
                    log::error!("{message}");
                    self.set_notice(&message);
                }
                paint_picture_placeholder(ui, rect, t, COVER_FAILED);
            }
        }
    }
}

/// What the cover's place says while the cover is on its way, and when it could not be had.
pub const COVER_WAIT: &str = "Loading the cover\u{2026}";
pub const COVER_FAILED: &str = "The cover could not be loaded";

/// The place of a picture that is not there: the tile's fill, a hairline, and `words` in the middle.
fn paint_picture_placeholder(ui: &Ui, rect: Rect, t: &crate::theme::Tokens, words: &str) {
    ui.painter().rect_filled(rect, 0.0, t.backstage_tile);
    ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, words, crate::theme::font(crate::theme::size::LABEL), t.text_dim);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bundled_artwork_decodes_and_wrong_dimensions_or_truncation_are_refused() {
        let icon = window_icon().unwrap();
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert_eq!(decode(COVER, COVER_WIDTH, COVER_HEIGHT).unwrap().len(), COVER_WIDTH * COVER_HEIGHT * 4);
        assert!(decode(COVER, 256, 256).is_err());
        assert!(decode(&ICON[..ICON.len() / 2], 256, 256).is_err());
        assert!(decode(&[], 256, 256).is_err());
    }

    /// The splash copy is a 256-colour picture that decodes to the pixels of its size, and it is the cover's halved, not another picture.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_splash_copy_decodes_from_its_palette_and_only_at_its_own_size() {
        assert_eq!(decode(SPLASH, SPLASH_WIDTH, SPLASH_HEIGHT).unwrap().len(), SPLASH_WIDTH * SPLASH_HEIGHT * 4);
        assert!(decode(SPLASH, COVER_WIDTH, COVER_HEIGHT).is_err());
        assert!(decode(SPLASH, 256, 256).is_err());
        assert!(decode(&SPLASH[..SPLASH.len() / 2], SPLASH_WIDTH, SPLASH_HEIGHT).is_err());
        assert_eq!((SPLASH_WIDTH * 2, SPLASH_HEIGHT * 2), (COVER_WIDTH, COVER_HEIGHT - 1), "half the cover, the odd row dropped");
    }
}
