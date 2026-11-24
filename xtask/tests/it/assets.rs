//! `cargo xtask assets`: every asset file is attributed in ATTRIBUTION.md: every file under `assets/` and
//! `docs/images/`, and every file anywhere with an asset extension (AGENTS.md: every asset file has a row).

use xtask::assets::{attributed, is_asset, missing, run};

fn files(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_string()).collect()
}

const TABLE: &str =
    "# Attribution\n\n| Path | Author | Source | Licence | Use |\n|---|---|---|---|---|\n| `assets/icons/a.svg` | me | here | MIT | icon |\n";

#[test]
fn detects_unattributed_assets() {
    let all = files(&["assets/icons/a.svg", "assets/icons/b.svg", "crates/x/src/lib.rs", "docs/images/b.png", "assets/fonts/OFL.txt"]);
    assert_eq!(missing(&all, TABLE), ["assets/icons/b.svg", "docs/images/b.png", "assets/fonts/OFL.txt"]);
    assert!(!is_asset("crates/x/src/lib.rs"));
    assert!(is_asset("assets/fonts/OFL.txt"));
    assert!(is_asset("docs/images/shot.png"));
    // The folders are matched as folders, not as name prefixes.
    assert!(!is_asset("docs/images-old/notes.txt"));
    assert!(!is_asset("crates/x/assets/notes.txt"));
}

/// An asset is an asset wherever it sits, by its extension.
#[test]
fn assets_outside_the_asset_folders_are_found_by_extension() {
    let unattributed = ["crates/ui-egui/icons/x.svg", "packaging/linux/icon.png", "crates/fonts/Calibri.ttf"];
    assert_eq!(missing(&files(&unattributed), TABLE), unattributed);
    for path in [
        "a.png", "b.jpg", "b.jpeg", "c.gif", "d.webp", "e.bmp", "f.tif", "f.tiff", "g.ico", "h.icns", "i.svg", "j.pdf", "k.ai", "l.eps", "m.psd",
        "n.icc", "o.icm", "p.ttf", "q.otf", "r.woff", "s.woff2", "t.ase", "u.aco", "v.abr", "w.mp4", "x.wav", "y.mp3",
        // The VSDX family's files: sample documents, stencils and templates are assets (AGENTS.md).
        "z.vsd", "z.vsdx", "z.vsdm", "z.vss", "z.vssx", "z.vssm", "z.vst", "z.vstx", "z.vstm", "z.vdx",
        // The rest of the VSDX family (the XML stencil and template, the add-on library, the web drawing), other fonts, and the
        // CAD and metafile formats DiaCraft imports: each one a document or font that someone else made.
        "z.vsx", "z.vtx", "z.vsl", "z.vdw", "z.ttc", "z.eot", "z.dwg", "z.dxf", "z.emf", "z.wmf",
    ] {
        assert!(is_asset(&format!("crates/x/tests/fixtures/{path}")), "{path}");
    }
    // The extension is matched whatever its case.
    assert!(is_asset("packaging/windows/ICON.PNG") && is_asset("crates/fonts/Segoe.TtF") && is_asset("tests/Plan.DWG") && is_asset("a/Page.VDW"));
    // Through `missing`, which the gate uses: unattributed, they are reported; attributed, they are not.
    let unlisted = ["crates/x/tests/fixtures/a.vsx", "crates/x/tests/fixtures/b.dxf", "crates/x/tests/fixtures/c.ttc", "d/e.vdw"];
    assert_eq!(missing(&files(&unlisted), TABLE), unlisted);
    let table = format!("{TABLE}| d/e.vdw | me | here | MIT | sample |\n");
    assert_eq!(missing(&files(&unlisted), &table), &unlisted[..3]);
    // Code, text and data are not assets, and an asset-looking directory name is not a file extension.
    for path in [
        "crates/x/src/lib.rs",
        "crates/x/tests/fixtures/data.json",
        "notes/formulas.xml",
        "xtask/Cargo.toml",
        "NOTICE",
        "docs/png",
        "crates/x/svg/mod.rs",
    ] {
        assert!(!is_asset(path), "{path}");
    }
    // Attributing them clears them.
    let table = format!("{TABLE}| crates/ui-egui/icons/x.svg | me | here | MIT | icon |\n| `packaging/linux/icon.png` | me | here | MIT | icon |\n");
    assert_eq!(missing(&files(&unattributed), &table), ["crates/fonts/Calibri.ttf"]);
}

/// Through real git: unattributed assets anywhere fail the gate, and what a test generates at run time
/// (untracked and ignored, as `target/` is) does not.
#[test]
fn the_gate_sees_assets_anywhere_but_not_fixtures_generated_at_run_time() {
    let dir = crate::git_repo("assets-anywhere");
    dir.write("ATTRIBUTION.md", TABLE);
    dir.write(".gitignore", "/target\n/generated\n");
    dir.write("crates/x/src/lib.rs", "pub fn f() {}\n");
    dir.write("crates/x/tests/fixtures/data.json", "{}\n");
    dir.write("assets/icons/a.svg", "x");
    // Written by a test at run time, in ignored folders.
    dir.write("target/debug/fixture.png", "x");
    dir.write("generated/shape.svg", "x");
    run(dir.path()).unwrap();

    dir.write("crates/ui-egui/icons/x.svg", "x");
    dir.write("packaging/linux/icon.png", "x");
    dir.write("crates/fonts/Calibri.ttf", "x");
    let err = run(dir.path()).unwrap_err();
    for path in ["crates/ui-egui/icons/x.svg", "packaging/linux/icon.png", "crates/fonts/Calibri.ttf"] {
        assert!(err.contains(path), "{err}");
    }
    assert!(err.contains("3 asset file(s)") && !err.contains("fixture.png") && !err.contains("shape.svg") && !err.contains("data.json"), "{err}");
}

#[test]
fn only_a_table_row_attributes_a_file() {
    let prose = "Not a row: `assets/icons/b.svg` is mentioned here.\n\n| Path | Author |\n|---|---|\n| `assets/icons/a.svg` | me |\n";
    assert_eq!(missing(&files(&["assets/icons/a.svg", "assets/icons/b.svg"]), prose), ["assets/icons/b.svg"]);
    // The path cell may be written with or without backticks, and a longer path is another path.
    let plain = "| Path |\n|---|\n| assets/icons/a.svg |\n| `assets/icons/a.svg.bak` |\n";
    let rows = attributed(plain);
    assert!(rows.iter().any(|r| r == "assets/icons/a.svg") && rows.iter().any(|r| r == "assets/icons/a.svg.bak"), "{rows:?}");
    assert_eq!(missing(&files(&["assets/icons/a.svg", "assets/icons/a.sv"]), plain), ["assets/icons/a.sv"]);
}

#[test]
fn no_asset_folders_means_nothing_to_attribute() {
    assert!(missing(&files(&["README.md", "crates/x/src/lib.rs"]), TABLE).is_empty());
    assert!(missing(&[], "").is_empty());
}

#[test]
fn the_tree_is_clean() {
    run(&xtask::root()).unwrap();
}

#[test]
fn an_unattributed_asset_with_a_non_ascii_or_spaced_name_fails_the_gate() {
    let dir = crate::git_repo("assets-names");
    dir.write("ATTRIBUTION.md", TABLE);
    dir.write("assets/icons/naïve icon.svg", "x");
    dir.write("docs/images/日本語.png", "x");
    dir.write("assets/icons/a.svg", "x");
    let err = run(dir.path()).unwrap_err();
    assert!(err.contains("2 asset file(s)") && err.contains("assets/icons/naïve icon.svg") && err.contains("docs/images/日本語.png"), "{err}");
    assert!(!err.contains("assets/icons/a.svg"), "{err}");
    // Attributing them passes.
    dir.write(
        "ATTRIBUTION.md",
        &format!("{TABLE}| `assets/icons/naïve icon.svg` | me | here | MIT | icon |\n| docs/images/日本語.png | me | here | MIT | shot |\n"),
    );
    run(dir.path()).unwrap();
}

/// A font a dependency compiles into the binary is no file of this repository, so `assets` has no file to ask a row for. It is
/// recorded by a row all the same (AGENTS.md: every asset the project ships has one), whose first cell is a label that no
/// repository path can equal. The gate needs no change for that: it asks files for rows and never rows for files, so such a row
/// passes it, and it does not attribute a file that someone commits under the font's name, which still needs a row of its own.
#[test]
fn a_row_for_a_font_a_dependency_bundles_passes_the_gate_and_attributes_no_file() {
    let label = "`[crate epaint_default_fonts] fonts/Hack-Regular.ttf`";
    let table = format!("{TABLE}| {label} | the authors | a source | MIT | the window |\n");
    // No file in the repository: nothing is missing, and the row is not asked for a file.
    assert!(missing(&files(&["crates/x/src/lib.rs", "assets/icons/a.svg"]), &table).is_empty());
    // A copy of the font in the repository is a file, and the label does not attribute it.
    let copies = ["fonts/Hack-Regular.ttf", "crates/ui-egui/fonts/Hack-Regular.ttf", "epaint_default_fonts/fonts/Hack-Regular.ttf"];
    assert_eq!(missing(&files(&copies), &table), copies);
    // Through the gate on a repository that holds only the row.
    let dir = crate::git_repo("assets-dependency-font");
    dir.write("ATTRIBUTION.md", &table);
    dir.write("assets/icons/a.svg", "x");
    run(dir.path()).unwrap();
    dir.write("fonts/Hack-Regular.ttf", "x");
    assert!(run(dir.path()).unwrap_err().contains("fonts/Hack-Regular.ttf"));
}

/// The typefaces the window installs are the ones the binaries embed, and each has a row that names both builds that carry it:
/// the desktop app and the web build, which runs the same window. A typeface added to `fonts.rs` without a row fails here.
#[test]
fn every_typeface_the_window_installs_has_a_row_naming_the_desktop_app_and_the_web_build() {
    let root = xtask::root();
    let installed = std::fs::read_to_string(root.join("crates/ui-egui/src/fonts.rs")).unwrap();
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap();
    let constants: Vec<&str> = installed
        .split("epaint_default_fonts::")
        .skip(1)
        .filter_map(|rest| rest.split(|c: char| !(c.is_ascii_uppercase() || c == '_')).next())
        .collect();
    assert!(constants.contains(&"HACK_REGULAR") && constants.contains(&"UBUNTU_LIGHT"), "fonts.rs no longer names the two typefaces: {constants:?}");
    let normal = |text: &str| text.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    let rows: Vec<&str> = attribution.lines().filter(|l| l.trim_start().starts_with('|')).collect();
    for constant in constants {
        let row = rows.iter().find(|row| row.split('|').nth(1).is_some_and(|first| normal(first).contains(&normal(constant))));
        let row = row.unwrap_or_else(|| panic!("fonts.rs installs {constant}, and ATTRIBUTION.md has no row for it"));
        assert!(row.contains("`apps/diacraft`") && row.contains("`apps/diacraft-web`"), "the row for {constant} does not name both builds: {row}");
    }
}
