//! The five per-stencil checks of the second stencil batch, for UML Component and Deployment (`uml-component`; made this file and makes the stencil).
//!
//! Each test loops over the masters of `builtin_stencil("uml-component")` (`stencils2_support.rs`). **While the stencil has no masters each passes on nothing; the day it ships, each runs on every master and
//! fails, naming the master, where a check does not hold.** Nothing here changes when the stencil is filled.

use crate::stencils2_support as support;

#[test]
fn every_master_of_uml_component_is_dropped_labelled_resized_turned_copied_undone_and_redone() {
    support::drop_edit_undo("uml-component");
}

#[test]
fn every_master_of_uml_component_draws_with_nothing_skipped_and_its_ink_inside_its_box() {
    support::render_inside_its_box("uml-component");
}

#[test]
fn every_master_of_uml_component_round_trips_through_vsdx_odg_and_diacraft_and_saves_again_as_no_edit() {
    // The masters an `.odg` shows other programs as a rectangle are the family's to report (writes most outlines as they are drawn).
    let approximated = support::round_trips("uml-component");
    eprintln!("uml-component: written as a rectangle for other programs in an .odg: {approximated:?}");
}

#[test]
fn a_connector_master_of_uml_component_is_glued_between_two_nodes_and_follows_the_second_when_it_moves() {
    support::glue("uml-component");
}

#[test]
fn the_accent_masters_of_uml_component_follow_three_themes_and_have_their_default_with_none() {
    support::theme("uml-component");
}
