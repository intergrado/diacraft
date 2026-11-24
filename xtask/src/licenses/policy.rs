//! What the licence tool is told by hand: the platforms and programs it covers, and the few places where what a crate says about itself is not enough.
//!
//! Every list here is checked against what the tool finds: an entry that matches nothing is an error, so a list cannot go stale unseen.

/// The programs DiaCraft ships as downloads (`cargo build --release -p diacraft -p diacraft-cli`).
pub const PROGRAMS: &[&str] = &["diacraft", "diacraft-cli"];

/// The program of the web build (`trunk build --release` in `apps/diacraft-web`): the app the website runs in the browser, as WebAssembly.
pub const WEB_PROGRAMS: &[&str] = &["diacraft-web"];

/// The target of the web build.
pub const WEB_TRIPLE: &str = "wasm32-unknown-unknown";

/// The platforms the notices cover, as (target triple, name, programs built for it). The release workflow builds the first six as downloads
/// (`.github/workflows/release.yml`), and its site job builds the web build into the site artifact. The two ARM64 targets name the crates their builds
/// link: the same as x86_64's, but for Windows' import libraries (`windows_aarch64_msvc` for `windows_x86_64_msvc`).
pub const TARGETS: &[(&str, &str, &[&str])] = &[
    ("x86_64-unknown-linux-gnu", "Linux", PROGRAMS),
    ("aarch64-unknown-linux-gnu", "Linux (ARM64)", PROGRAMS),
    ("x86_64-pc-windows-msvc", "Windows", PROGRAMS),
    ("aarch64-pc-windows-msvc", "Windows (ARM64)", PROGRAMS),
    ("aarch64-apple-darwin", "macOS (Apple silicon)", PROGRAMS),
    ("x86_64-apple-darwin", "macOS (Intel)", PROGRAMS),
    (WEB_TRIPLE, "Web", WEB_PROGRAMS),
];

/// One file of notices: its name at the root of the repository, the platforms whose builds it covers, and whether the lists of this module are checked
/// against it. A list entry that matches nothing is stale only when every file has been looked at, so the lists belong to the first file: the one every
/// download carries.
pub struct Output {
    pub file: &'static str,
    pub triples: &'static [&'static str],
    pub checks_lists: bool,
}

/// The files the notices are written to, at the root of the repository. The first is the one every download carries. The second covers the web build
/// alone: it is what the web app's licences window shows, and what the website links beside the app.
pub const OUTPUTS: &[Output] = &[
    Output {
        file: "THIRD-PARTY-LICENSES.txt",
        triples: &[
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
        ],
        checks_lists: true,
    },
    Output { file: "THIRD-PARTY-LICENSES-WEB.txt", triples: &[WEB_TRIPLE], checks_lists: false },
];

/// Files that are licences although their names do not say so, by crate: `epaint_default_fonts` keeps the licences of its fonts under the font's name
/// (`Hack-Regular.txt`, `UFL.txt` for Ubuntu Light, `OFL.txt` for the Noto fonts of its optional emoji feature, which its own licence expression names).
pub const LISTED_FILES: &[(&str, &[&str])] = &[("epaint_default_fonts", &["fonts/Hack-Regular.txt", "fonts/UFL.txt", "fonts/OFL.txt"])];

/// Licence-named files that are left out, as (crate, file in the crate, why): they belong to material that is not compiled into a program.
pub const SKIPPED_FILES: &[(&str, &str, &str)] =
    &[("petgraph", "assets/images/LICENSE.md", "the licence of a picture in petgraph's documentation, which is not compiled into a program")];

/// Files of a licence outside `deny.toml`'s list that a crate carries inside its own, listed and reviewed, as (crate, file in the crate, what the notices say of it).
/// The text still goes in the notices, as the licence asks; the check does not fail on it. The sentence is read by the people who get the programs.
///
/// `wayland-protocols-plasma` (2026-11-22): the crate is MIT, but the protocol description files its macro turns into Rust code come from
/// KDE's plasma-wayland-protocols, whose files say LGPL-2.1-or-later. winit's KWin blur support on Linux uses them. AGENTS.md says no LGPL code is
/// depended on, so this is the owner's to decide (docs/gaps.md): the entry is the notice, not a decision.
pub const REVIEWED_FILES: &[(&str, &str, &str)] = &[(
    "wayland-protocols-plasma",
    "plasma-wayland-protocols/COPYING.LIB",
    "the text of the GNU Lesser General Public License. The crate's own licence is MIT, but the protocol description files that its macro turns into Rust code \
     come from KDE's plasma-wayland-protocols, whose files say LGPL-2.1-or-later",
)];

/// Crates that compile C or C++ code into a program (they have a build dependency on `cc` or `cmake`), as (crate, what it compiles and where its notice is).
/// A crate like that must be listed, so that nobody ships native code without looking at its notices.
pub const NATIVE_CODE: &[(&str, &str)] = &[
    (
        "libmimalloc-sys",
        "builds the mimalloc memory allocator, a C library, from the sources in the crate's c_src folder and links it into the program as the allocator \
         (the desktop app and the command line, not the web build); the library's own licence is listed below",
    ),
    (
        "wayland-backend",
        "builds a small C file of its own (log_shim.c, under the crate's licence) when its `log` feature is on; the Wayland client library itself is the \
         computer's, loaded at run time",
    ),
];

/// The copyright holders of the Rust standard library, for its standard MIT text.
pub const RUST_HOLDERS: &str = "The Rust Project Contributors";
