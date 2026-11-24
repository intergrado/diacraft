//! Build script: which git release and which year this build is, for the splash popup of the page. The work is in the desktop app's `build_info.rs`, which
//! this crate and the command line take as well, and which that app's tests run. It never fails the build.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[path = "../diacraft/build_info.rs"]
mod build_info;

fn main() {
    build_info::emit();
}
