//! The kinds of file the Save dialog offers: which, and in what order.
//!
//! **Which is the engine's to say.** The window keeps no table of the saves across formats that are written: for each kind it might offer it asks
//! [`diacraft_engine::Session::save_as_allows`], the save-path table asked without saving, and offers what it allows (a saving of a `.diacraft` drawing as an `.odg` is
//! refused until that writer is wired, and then this list gains it with no change here). **The order is the window's**, the design's section 7:
//!
//! - the drawing's **own type first**, the one the dialog selects: what it was read as or last saved as, and for a new drawing, `.diacraft`;
//! - then for a stencil, `.diacraft-stencil` and the VSDX stencil types; and for any other drawing, `.diacraft` and `.diacraft-template`, then the OpenDocument
//!   drawing, then the VSDX types.
//!
//! A kind is a label for the dialog and an extension. `.fodg` is never a kind: a flat drawing is saved by Save, in its own form, and Save As is not available for it. **`.otg` is**,
//! the OpenDocument drawing template the engine writes beside the `.odg` (`odgfile::SAVE_TYPES`), listed after it.

use std::path::PathBuf;
use std::sync::Arc;

/// One kind of file the Save dialog offers: the label it shows, and the extension (without the dot, in lower case) a file of the kind is called by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveKind {
    pub label: &'static str,
    pub extension: &'static str,
}

/// What the Save dialog is asked: the name to start with, and the kinds of file to offer, the first of them the one the dialog selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveRequest {
    pub name: String,
    pub kinds: Vec<SaveKind>,
}

/// What chooses the file to save to: the desktop app's native Save dialog. It is given what to ask ([`SaveRequest`]), blocks until the person chooses or cancels, and answers the
/// path or `None`.
pub type SavePicker = Arc<dyn Fn(SaveRequest) -> Option<PathBuf> + Send + Sync>;

/// Every kind a Save dialog may offer, with its label. The order of the list is the order the kinds follow the drawing's own (see [`save_kinds`]). The six VSDX types are
/// named by their file-type labels, with the product's name (owner, 2026-11-23; `cargo xtask brands` lets these exact labels through, and the name
/// nowhere else in this file); DiaCraft's own types and the OpenDocument ones keep their names.
const KINDS: [SaveKind; 11] = [
    SaveKind { label: "DiaCraft drawing (.diacraft)", extension: "diacraft" },
    SaveKind { label: "DiaCraft stencil (.diacraft-stencil)", extension: "diacraft-stencil" },
    SaveKind { label: "DiaCraft template (.diacraft-template)", extension: "diacraft-template" },
    SaveKind { label: "OpenDocument drawing (.odg)", extension: "odg" },
    SaveKind { label: "OpenDocument drawing template (.otg)", extension: "otg" },
    SaveKind { label: "Visio Drawing (.vsdx)", extension: "vsdx" },
    SaveKind { label: "Visio Macro-Enabled Drawing (.vsdm)", extension: "vsdm" },
    SaveKind { label: "Visio Stencil (.vssx)", extension: "vssx" },
    SaveKind { label: "Visio Macro-Enabled Stencil (.vssm)", extension: "vssm" },
    SaveKind { label: "Visio Template (.vstx)", extension: "vstx" },
    SaveKind { label: "Visio Macro-Enabled Template (.vstm)", extension: "vstm" },
];

/// What follows a stencil's own type.
const STENCIL_KINDS: [&str; 3] = ["diacraft-stencil", "vssx", "vssm"];

/// What follows another drawing's own type: DiaCraft's drawing and template, the OpenDocument drawing and its template, and the VSDX types a drawing is saved as.
const DRAWING_KINDS: [&str; 8] = ["diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"];

/// The kind whose extension is `extension` (in any case), if it is one the dialog offers.
pub fn kind_of(extension: &str) -> Option<SaveKind> {
    KINDS.into_iter().find(|kind| kind.extension.eq_ignore_ascii_case(extension))
}

/// The macro-enabled type of a VSDX type that holds no macro project (`vsdx` is `vsdm`, `vssx` `vssm`, `vstx` `vstm`, in any case), or `None` for any other. A save of a drawing that holds a
/// macro project to the type that cannot hold it is refused by the engine (`EngineError::MacrosNotAllowed`), and the window offers the twin as a way out (`ask.rs`): the names are those of
/// the VSDX file types the engine's own words name (".vsdm, .vssm or .vstm").
pub fn macro_twin(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "vsdx" => Some("vsdm"),
        "vssx" => Some("vssm"),
        "vstx" => Some("vstm"),
        _ => None,
    }
}

/// The kinds to offer for a drawing whose own type is `own` (an extension), and which is a stencil or not: `own` first, then the kinds that follow it, **each only if `allows`** says
/// the engine writes it for this drawing. No kind twice.
pub fn save_kinds(own: &str, stencil: bool, allows: impl Fn(&str) -> bool) -> Vec<SaveKind> {
    let rest: &[&str] = if stencil { &STENCIL_KINDS } else { &DRAWING_KINDS };
    let mut kinds: Vec<SaveKind> = Vec::new();
    for extension in std::iter::once(own).chain(rest.iter().copied()) {
        if let Some(kind) = kind_of(extension)
            && allows(kind.extension)
            && !kinds.contains(&kind)
        {
            kinds.push(kind);
        }
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extensions(kinds: &[SaveKind]) -> Vec<&'static str> {
        kinds.iter().map(|k| k.extension).collect()
    }

    /// The order the design gives, when the engine allows everything.
    #[test]
    fn the_own_type_comes_first_and_the_rest_follow_in_the_designs_order() {
        let all = |_: &str| true;
        assert_eq!(extensions(&save_kinds("vsdx", false, all)), ["vsdx", "diacraft", "diacraft-template", "odg", "otg", "vsdm", "vssx", "vstx"]);
        assert_eq!(extensions(&save_kinds("diacraft", false, all)), ["diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"]);
        assert_eq!(extensions(&save_kinds("odg", false, all)), ["odg", "diacraft", "diacraft-template", "otg", "vsdx", "vsdm", "vssx", "vstx"]);
        assert_eq!(extensions(&save_kinds("otg", false, all)), ["otg", "diacraft", "diacraft-template", "odg", "vsdx", "vsdm", "vssx", "vstx"]);
        assert_eq!(
            extensions(&save_kinds("diacraft-template", false, all)),
            ["diacraft-template", "diacraft", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"]
        );
        assert_eq!(
            extensions(&save_kinds("vstm", false, all)),
            ["vstm", "diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"],
            "an own type that is no usual kind"
        );
        assert_eq!(extensions(&save_kinds("vssx", true, all)), ["vssx", "diacraft-stencil", "vssm"]);
        assert_eq!(extensions(&save_kinds("diacraft-stencil", true, all)), ["diacraft-stencil", "vssx", "vssm"]);
        assert_eq!(extensions(&save_kinds("vssm", true, all)), ["vssm", "diacraft-stencil", "vssx"]);
    }

    /// The engine decides which of them are offered: a kind it refuses is left out, and nothing here names one.
    #[test]
    fn a_kind_the_engine_does_not_allow_is_left_out() {
        let no_odg = |e: &str| e != "odg";
        assert_eq!(extensions(&save_kinds("diacraft", false, no_odg)), ["diacraft", "diacraft-template", "otg", "vsdx", "vsdm", "vssx", "vstx"]);
        let no_otg = |e: &str| e != "otg";
        assert_eq!(extensions(&save_kinds("diacraft", false, no_otg)), ["diacraft", "diacraft-template", "odg", "vsdx", "vsdm", "vssx", "vstx"]);
        let only_odg_and_native = |e: &str| e.starts_with("diacraft") || e == "odg";
        assert_eq!(extensions(&save_kinds("odg", false, only_odg_and_native)), ["odg", "diacraft", "diacraft-template"]);
        assert_eq!(extensions(&save_kinds("diacraft", false, |_| false)), Vec::<&str>::new(), "a drawing nothing is allowed for is offered nothing");
    }

    /// `.fodg` is never a kind, whatever the drawing and whatever the engine says; nor is a type that is not one of ours.
    #[test]
    fn a_flat_drawing_and_an_unknown_type_are_never_kinds() {
        for own in ["fodg", "FODG", "txt", ""] {
            let kinds = save_kinds(own, false, |_| true);
            assert!(!kinds.iter().any(|k| k.extension == "fodg" || k.label.contains("fodg") || k.label.contains("flat")), "{own}");
            assert_eq!(
                extensions(&kinds),
                ["diacraft", "diacraft-template", "odg", "otg", "vsdx", "vsdm", "vssx", "vstx"],
                "{own}: the type after it only"
            );
        }
    }

    /// The own type in capitals is the same kind, and is not offered twice.
    #[test]
    fn the_case_of_the_own_type_does_not_matter() {
        assert_eq!(extensions(&save_kinds("VSDX", false, |_| true))[0], "vsdx");
        assert_eq!(save_kinds("VSDX", false, |_| true).iter().filter(|k| k.extension == "vsdx").count(), 1);
    }

    /// The macro-enabled twin of each macro-free VSDX type, in any case; nothing else has one, and the twin is a kind the dialog offers.
    #[test]
    fn each_macro_free_type_has_a_macro_enabled_twin() {
        assert_eq!(macro_twin("vsdx"), Some("vsdm"));
        assert_eq!(macro_twin("VSSX"), Some("vssm"));
        assert_eq!(macro_twin("Vstx"), Some("vstm"));
        for other in ["vsdm", "vssm", "vstm", "odg", "otg", "diacraft", "diacraft-template", "diacraft-stencil", "", "vsd", "vsdx "] {
            assert_eq!(macro_twin(other), None, "{other:?}");
        }
        for twin in ["vsdm", "vssm", "vstm"] {
            assert!(kind_of(twin).is_some(), "{twin} is a kind");
        }
    }

    /// The six VSDX types are named by their file-type labels (owner, 2026-11-23), and DiaCraft's own types and the OpenDocument ones keep their names.
    #[test]
    fn the_vsdx_types_are_named_by_their_file_type_labels() {
        let label = |extension: &str| kind_of(extension).map(|k| k.label);
        assert_eq!(label("vsdx"), Some("Visio Drawing (.vsdx)"));
        assert_eq!(label("vsdm"), Some("Visio Macro-Enabled Drawing (.vsdm)"));
        assert_eq!(label("vssx"), Some("Visio Stencil (.vssx)"));
        assert_eq!(label("vssm"), Some("Visio Macro-Enabled Stencil (.vssm)"));
        assert_eq!(label("vstx"), Some("Visio Template (.vstx)"));
        assert_eq!(label("vstm"), Some("Visio Macro-Enabled Template (.vstm)"));
        assert_eq!(label("diacraft"), Some("DiaCraft drawing (.diacraft)"));
        assert_eq!(label("diacraft-stencil"), Some("DiaCraft stencil (.diacraft-stencil)"));
        assert_eq!(label("diacraft-template"), Some("DiaCraft template (.diacraft-template)"));
        assert_eq!(label("odg"), Some("OpenDocument drawing (.odg)"));
        assert_eq!(label("otg"), Some("OpenDocument drawing template (.otg)"));
    }

    /// Each kind's label names its extension, so the person sees what a file will be called.
    #[test]
    fn every_label_names_its_extension() {
        for kind in KINDS {
            assert!(kind.label.ends_with(&format!("(.{})", kind.extension)), "{}", kind.label);
        }
        assert!(KINDS.iter().all(|k| k.extension == k.extension.to_ascii_lowercase()));
    }
}
