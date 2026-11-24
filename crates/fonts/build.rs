//! Optional craft-fonts build input: with `CRAFT_FONTS_DIR=<checkout of storytold/craft-fonts>` the faces its manifests list are
//! embedded as `CRAFT_FONTS`, and unset the list is empty. The work is in `build_input.rs`. Nothing here fails the build, since
//! everything must build, test and run without craft-fonts: problems are cargo warnings and the faces are skipped.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod build_input;

use std::path::PathBuf;

fn main() {
    let (dir, mut warnings) = build_input::checked_dir(std::env::var_os("CRAFT_FONTS_DIR").as_deref());
    for line in build_input::rerun_directives(dir.as_deref()) {
        println!("{line}");
    }
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let (source, more) = build_input::build(dir.as_deref(), wasm);
    warnings.extend(more);
    for w in warnings {
        // A newline would end the warning early and let the rest be read as another directive.
        println!("cargo::warning=craft-fonts: {}", w.replace(['\n', '\r'], " "));
    }
    // OUT_DIR is always set by cargo for a build script; without it nothing can be written, which is an error to show.
    let Some(out) = std::env::var_os("OUT_DIR") else {
        println!("cargo::error=OUT_DIR is not set");
        return;
    };
    let out = PathBuf::from(out).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, source) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}
