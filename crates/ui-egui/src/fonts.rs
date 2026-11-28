//! Interface fonts: optional open sans faces, with bundled Hack and Ubuntu Light as the fallback.
//!
//! egui's `default_fonts` feature would also register `egui-icons`, a subset of emoji-icon-font whose glyphs
//! are the Windows, Apple, Android and GitHub logos and the word `git`, and it compiles those bytes into the
//! binary whether or not the app installs them. The window draws none of them, and a logo of another vendor's
//! platform is not something DiaCraft shows, so that feature is off and the two typefaces are taken from
//! `epaint_default_fonts` by name. The constant for the icon font is never referenced, so its bytes are not
//! in the binary. A test in xtask reads Cargo.lock and fails if `epaint` depends on `epaint_default_fonts`,
//! which is what the feature adds on any member and target; `NOTICE` lists what is in the binary.
//!
//! **A character no face has is a box, never a panic.** egui's `MissingGlyphPolicy` is `Tofu` unless something sets `Panic`, and nothing in the window, the desktop app or the
//! web shell does: only `egui_kittest` does, so that a test window fails on a word of ours that holds such a character. A test that gives the window a person's text lays it out as
//! shipped (`tests/it/glyphs.rs`, `tests/it/support.rs::as_shipped`). Every proportional chain ends with Hack, so the characters Hack has and Ubuntu Light lacks (the arrows of a
//! status-bar notice) are drawn by the bundled faces alone, which are the only faces the web build loads.

mod script_faces;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use diacraft_fonts::{Face, FontDb, FontStyle, MatchKind, Outline};
use egui::{FontData, FontDefinitions, FontFamily};

/// The monospace typeface.
const HACK: &str = "Hack";

/// The proportional typeface.
const UBUNTU_LIGHT: &str = "Ubuntu-Light";

/// A real bold face when available, otherwise the same fallback chain as ordinary text.
pub fn strong_family() -> FontFamily {
    FontFamily::Name("Diacraft-Strong".into())
}

/// Optional interface families, in preference order. No document-name substitutions are used.
const UI_FAMILIES: [&str; 4] = ["Inter", "Noto Sans", "Liberation Sans", "DejaVu Sans"];
/// At most two faces are copied into egui, each capped before allocation.
const MAX_UI_FACE_BYTES: usize = 8 << 20;
/// The platform logos excluded from the bundled interface font set must stay excluded after an upgrade.
const LOGOS: [char; 5] = ['\u{E618}', '\u{E61F}', '\u{E624}', '\u{E625}', '\u{F8FF}'];

/// The always-available definitions: Ubuntu Light for text with Hack behind it, and Hack first for monospace.
pub fn font_definitions() -> FontDefinitions {
    let mut definitions = FontDefinitions::empty();
    definitions.font_data.insert(HACK.to_owned(), Arc::new(FontData::from_static(epaint_default_fonts::HACK_REGULAR)));
    definitions.font_data.insert(UBUNTU_LIGHT.to_owned(), Arc::new(FontData::from_static(epaint_default_fonts::UBUNTU_LIGHT)));
    // Hack behind Ubuntu Light, for the characters Ubuntu Light lacks and Hack has: the arrows, the box-drawing and block characters, more of the geometric shapes. It is in the
    // binary already, so the module is no larger; a character neither has is a box (egui's `Tofu`, which eframe keeps), and no letter of the proportional text moves.
    definitions.families.insert(FontFamily::Proportional, vec![UBUNTU_LIGHT.to_owned(), HACK.to_owned()]);
    // Ubuntu Light after Hack, for the characters Hack lacks (a square root, say), as egui's own default does.
    definitions.families.insert(FontFamily::Monospace, vec![HACK.to_owned(), UBUNTU_LIGHT.to_owned()]);
    definitions.families.insert(strong_family(), vec![UBUNTU_LIGHT.to_owned(), HACK.to_owned()]);
    definitions
}

/// Prefer regular interface text from an already discovered database. This never scans font folders,
/// but may lazily load catalogued files: filesystem-backed databases must be prepared on a worker.
/// The bundled text faces remain available for missing glyphs, and the monospace chain is unchanged.
pub fn font_definitions_with(db: &FontDb) -> FontDefinitions {
    let mut definitions = font_definitions();
    for family in UI_FAMILIES {
        let Some(regular) = interface_face(db, family, false) else { continue };
        let regular_name = format!("{family}-UI-Regular");
        insert_face(&mut definitions, &regular_name, &regular);
        let regular_chain = vec![regular_name, UBUNTU_LIGHT.to_owned(), HACK.to_owned()];
        let mut strong_chain = regular_chain.clone();
        if let Some(bold) = interface_face(db, family, true) {
            let bold_name = format!("{family}-UI-Bold");
            insert_face(&mut definitions, &bold_name, &bold);
            strong_chain.insert(0, bold_name);
        }
        definitions.families.insert(FontFamily::Proportional, regular_chain);
        definitions.families.insert(strong_family(), strong_chain);
        break;
    }
    // A face for Arabic and one for Hebrew, behind the others, for the text box (`script_faces.rs`).
    script_faces::add_script_faces(db, &mut definitions);
    definitions
}

fn interface_face(db: &FontDb, family: &str, bold: bool) -> Option<Face> {
    // Avoid resolving absent names: document fallbacks may be proprietary or deliberately different families.
    if !db.has_family(family) {
        return None;
    }
    let resolved = db.resolve(family, FontStyle { bold, italic: false });
    let face = resolved.face;
    if resolved.kind != MatchKind::Named
        || !face.family().eq_ignore_ascii_case(family)
        || face.is_null()
        || face.is_broken()
        || face.is_italic()
        || face.is_bold() != bold
        || (!bold && !(350..=500).contains(&face.weight()))
        || face.data().len() > MAX_UI_FACE_BYTES
        || LOGOS.iter().any(|&ch| face.glyph_for(ch) != 0)
    {
        return None;
    }
    safe_interface_outlines(&face).then_some(face)
}

/// egui rasterizes raw outline bytes without our document renderer's bounded path sink. Admit only
/// static TrueType faces whose every glyph passed that sink and these smaller UI-specific budgets.
/// A rejected optional face leaves the bundled fallback intact. All native work runs on Preparation.
fn safe_interface_outlines(face: &Face) -> bool {
    const MAX_GLYPHS: u32 = 16_384;
    const MAX_GLYPH_ELEMENTS: usize = 4096;
    const MAX_TOTAL_ELEMENTS: usize = 1_048_576;
    let Some(glyphs) = face.static_truetype_glyph_count(4.0).filter(|&n| n > 0 && n <= MAX_GLYPHS) else { return false };
    let em = face.units_per_em();
    let max_coordinate = em * 4.0;
    let mut total = 0usize;
    for glyph in 0..glyphs {
        let advance = face.advance(glyph);
        if !advance.is_finite() || !(0.0..=max_coordinate).contains(&advance) {
            return false;
        }
        match face.outline_glyph(glyph) {
            Outline::Broken => return false,
            Outline::Blank => {}
            Outline::Drawn(path) => {
                let count = path.elements().len();
                if count > MAX_GLYPH_ELEMENTS || count > MAX_TOTAL_ELEMENTS - total {
                    return false;
                }
                total += count;
                let bounds = path.control_box();
                if [bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().any(|p| !p.is_finite() || p.abs() > max_coordinate)
                    || bounds.width() > max_coordinate
                    || bounds.height() > max_coordinate
                {
                    return false;
                }
            }
        }
    }
    !face.is_broken()
}

fn insert_face(definitions: &mut FontDefinitions, name: &str, face: &Face) {
    let mut data = FontData::from_owned(face.data().to_vec());
    // A collection can hold several styles; egui must draw the face the database actually selected.
    data.index = face.index();
    // Admission checked unhinted geometry. Font instructions must not transform it afterwards.
    data.tweak.hinting = Some(false);
    definitions.font_data.insert(name.to_owned(), Arc::new(data));
}

/// One optional interface-font preparation. Loading a face is lazy even after the database's metadata
/// scan finishes, so both resolving the face and copying its bytes belong on this worker.
pub(crate) struct Preparation {
    ready: Receiver<Result<FontDefinitions, String>>,
    #[cfg(test)]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Preparation {
    pub(crate) fn start(make: impl FnOnce() -> FontDefinitions + Send + 'static) -> Option<Self> {
        // Browser builds have no filesystem faces or native threads. Their initial bundled definitions
        // already include the optional craft-fonts build input.
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let (send, ready) = channel();
        match std::thread::Builder::new().name("diacraft-ui-fonts".to_owned()).spawn(move || {
            let result = diacraft_engine::guard::catch_panic(make);
            // Closing the window while fonts are loading makes the result unnecessary.
            let _ = send.send(result);
        }) {
            Ok(worker) => {
                // Production drops the handle to detach; unit tests may join after releasing their gate.
                #[cfg(not(test))]
                drop(worker);
                Some(Self {
                    ready,
                    #[cfg(test)]
                    worker: Some(worker),
                })
            }
            Err(error) => {
                log::warn!("interface fonts could not start loading ({error}); keeping the bundled faces");
                None
            }
        }
    }

    /// Test synchronization only: wait off the UI loop until the answer has been sent, then exercise
    /// the ordinary nonblocking poll. Production never stores or joins a worker handle.
    #[cfg(test)]
    pub(crate) fn finish_for_test(&mut self) -> std::thread::Result<()> {
        self.worker.take().map_or(Ok(()), |worker| worker.join())
    }

    /// Never waits for the metadata scan, disk reads, or copying. A completed answer is consumed once.
    pub(crate) fn poll(&self) -> Option<Result<FontDefinitions, String>> {
        match self.ready.try_recv() {
            Ok(definitions) => Some(definitions),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("interface font worker stopped before returning its faces".to_owned())),
        }
    }
}
