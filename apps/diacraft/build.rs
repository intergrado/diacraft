//! Build script: which git release and which year this build is, for the splash and About panel of the window. The work is in `build_info.rs`, which
//! the command line's and the web app's build scripts share, and which `tests/it/build_info.rs` runs. It never fails the build.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod build_info;

fn main() {
    build_info::emit();
}
