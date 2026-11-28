//! The help tree this crate was built with.
//!
//! `build.rs` reads the help tree of the repository (`host::HELP_TREE`, and the catalogs in `host::CATALOG_TREE`) and writes constants that hold its files: the desktop
//! programs embed everything, the browser build only the three tables (the topics are fetched there, one file for a language).

use std::sync::OnceLock;

use crate::{Bundle, Parts, from_parts};

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// The help tree embedded at build time, read once.
pub fn embedded() -> &'static Bundle {
    static BUNDLE: OnceLock<Bundle> = OnceLock::new();
    BUNDLE.get_or_init(|| from_parts(&Parts { topics: TOPICS, tips: TIPS, map: MAP, sections: SECTIONS, catalogs: CATALOGS }))
}
