//! DiaCraft fonts: the font database, the stand-ins for names the machine lacks, what a layout reads from a face (advances,
//! raised and lowered letters, the lines it recommends, outlines) and OpenType shaping.
//!
//! **Where faces come from, in the order a name is searched:** faces a caller adds at run time ([`FontDb::add_font_data`]), the
//! craft-fonts faces built into the binary when `CRAFT_FONTS_DIR` named a checkout at build time, the faces installed on the
//! machine, and last the bundled Hack and Ubuntu Light, which are always there. A name nothing has gets a stand-in ([`standin`]),
//! so [`FontDb::pick`] always returns a face. No Microsoft face is bundled, and the crate builds, tests and runs with none of the
//! optional sources.
//!
//! **The web build:** reading the machine's font folders is behind one seam, `system`, which does nothing on `wasm32`; a browser
//! shell hands in font bytes through [`FontDb::add_font_data`]. Nothing else here touches the file system.
//!
//! **Hostile input:** font files and names come from the machine and from drawings. Every table read is bounds-checked by `skrifa`
//! and `harfrust`, a face that doesn't parse is skipped once, names are capped in characters, a glyph is capped in path elements and in
//! the size of a point, and the sizes and counts a font file or a font folder can make us read are limited in `catalog` and `system`.
//! A damaged face never fails a lookup.
//!
//! **The font reader:** a face holds one `skrifa::Font`, which `harfrust` shapes and everything else here reads, and draws its outlines
//! through a `skrifa::ScalerFont`. No type of either crate is in this crate's API.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod catalog;
mod db;
mod face;
pub mod script;
pub mod standin;
mod system;
pub mod work;

#[cfg(feature = "test-support")]
pub mod synth;

pub use catalog::Origin;
pub use db::{AddReport, Cover, FontDb, FontStyle, MatchKind, Resolved, is_ignorable};
pub use face::{Face, Outline, ScriptBox, ShapeParams, ShapedGlyph, Stroke};
pub use kurbo::BezPath;
pub use script::{FontSlot, ScriptTag};
pub use standin::GenericClass;
pub use system::{font_dirs, xdg_font_dirs};

/// A face from the optional craft-fonts build input (empty unless `CRAFT_FONTS_DIR` was set when building: see `build.rs`).
pub(crate) struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    #[allow(dead_code)] // Kept for a later script-aware fallback; the list comes from the manifest.
    pub scripts: &'static [&'static str],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// How many faces the build embedded from craft-fonts: 0 unless `CRAFT_FONTS_DIR` was set when this crate was built.
pub fn craft_font_count() -> usize {
    CRAFT_FONTS.len()
}
