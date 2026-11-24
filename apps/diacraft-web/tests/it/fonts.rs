//! The web build draws with the desktop's fonts. The shell makes its window through `page::new_app`, the same
//! `DiacraftApp` the desktop runs, and that window installs Hack and Ubuntu Light on its first pass. egui's own
//! `default_fonts` (which adds an icon font with other vendors' logos) is off, and the test
//! `the_lockfile_does_not_link_epaint_to_the_icon_font` in xtask reads Cargo.lock to keep it so on every member
//! and target, this one included.

use diacraft_browser::page::new_app;
use egui::{FontDefinitions, FontFamily};
use egui_kittest::Harness;

#[test]
fn the_window_the_shell_makes_installs_the_desktops_two_typefaces_and_no_icon_font() {
    let mut window = Harness::builder().with_size([800.0, 500.0]).build_eframe(|_| new_app());
    window.step();
    let definitions = window.ctx.fonts(|fonts| fonts.definitions().clone());
    let names: Vec<&String> = definitions.font_data.keys().collect();
    assert_eq!(names, ["Hack", "Ubuntu-Light"], "the web window installs another set of fonts than the desktop's");
    assert_eq!(definitions.families.get(&FontFamily::Proportional), Some(&vec!["Ubuntu-Light".to_owned()]));
    assert_eq!(definitions.families.get(&FontFamily::Monospace), Some(&vec!["Hack".to_owned(), "Ubuntu-Light".to_owned()]));
}

/// With `default_fonts` on, egui's own definitions hold its bundled fonts, icon font included.
#[test]
fn egui_default_fonts_are_off_in_this_crates_build() {
    assert_eq!(FontDefinitions::builtin_font_names(), &[] as &[&str]);
}
