//! The engine's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod across;
mod alt_text;
mod alt_text_files;
mod alt_text_odg;
mod arrange;
mod automation_roots;
mod batch;
mod callout_files;
mod callouts;
mod change_shape;
mod change_shape_files;
mod change_shape_limits;
mod change_shape_save;
mod clipboard;
mod clipboard_limits;
mod command_sweep;
mod commands;
mod comments;
mod comments_files;
mod compartment_scale;
mod conn_points;
mod conn_points_costs;
mod conn_points_edges;
mod conn_points_files;
mod conn_points_glue;
mod conn_points_marker;
mod conn_points_reopen;
mod conn_refs;
mod connect_auto;
mod connect_cost;
mod connect_files;
mod connect_hand;
mod connect_jumps;
mod connect_neighbour;
mod connect_recalc;
mod connect_reroute;
mod connect_routable;
mod connect_setting;
mod connect_style;
mod connect_walkglue;
mod connect_whole;
mod connector_default_fill;
mod container;
mod container_cycles;
mod container_files;
mod container_hostile;
mod container_more;
mod delete_connectors;
mod double_click;
mod draw_tools;
mod drop_resave;
mod drop_scale;
mod drop_scale_root;
mod edit;
mod edit_cost;
mod edit_file;
mod edit_foreign;
mod edit_ids;
mod edit_index;
mod edit_random;
mod edit_render;
mod edit_review;
mod edit_route;
mod edit_scope;
mod edit_text;
mod editor_paragraphs;
mod empty_text;
mod endfill_fixtures;
mod endfill_roundtrip;
mod export;
mod face_query;
mod fields;
mod fields_odg;
mod figure_scale;
mod find;
mod find_cost;
mod format;
mod format_cost;
mod format_file;
mod format_group_hidden;
mod format_para;
mod format_para_cmd;
mod format_para_files;
mod format_range;
mod format_scale;
mod format_text;
mod formatfx;
mod formatfx_cost;
mod formatfx_files;
mod formats;
mod formula_case;
mod group;
mod group_frame;
mod group_geometry;
mod group_size;
mod guard;
mod history;
mod history_bytes;
mod hyperlinks;
mod inherit_pass;
mod inherited_text;
mod insert_files;
mod journal;
mod jump_zero;
mod lang_files;
mod layers;
mod layers_files;
mod layers_locked_edits;
mod layers_locked_last;
mod layers_locked_more;
mod master_line_cells;
mod master_references;
mod master_unique_ids;
mod measure_fonts;
mod memfs_files;
mod model_guard;
mod move_delete_trip;
mod native_common;
mod native_cost;
mod native_default;
mod native_file;
mod native_memory;
mod native_odg;
mod native_reopen;
mod native_sniff;
mod native_speed;
mod noop_edits;
mod odg_build;
mod odg_carry_names;
mod odg_connect;
mod odg_cross_fontname;
mod odg_custom_glue_and_links;
mod odg_dash_chain;
mod odg_dashes;
mod odg_faults;
mod odg_fields_across;
mod odg_fields_back;
mod odg_file;
mod odg_findings_trip;
mod odg_flat;
mod odg_flat_saveas;
mod odg_flip_model;
mod odg_flipglue;
mod odg_flipturn;
mod odg_frameglue;
mod odg_fx;
mod odg_glue_edit;
mod odg_glue_writes;
mod odg_gradients;
mod odg_group_alt_text;
mod odg_group_data_flat;
mod odg_group_keys;
mod odg_group_refusal_words;
mod odg_group_reordered;
mod odg_group_save_again;
mod odg_groups;
mod odg_groupturn;
mod odg_hold;
mod odg_keptns;
mod odg_masters;
mod odg_memory;
mod odg_model_measure;
mod odg_new_shape_losses;
mod odg_newsave;
mod odg_newsave_more;
mod odg_note;
mod odg_objects_kept;
mod odg_odftag;
mod odg_package;
mod odg_pagekeep;
mod odg_pages;
mod odg_poly1d;
mod odg_pose_formula;
mod odg_random;
mod odg_readnote;
mod odg_refusal_ways;
mod odg_refused;
mod odg_resave;
mod odg_safe;
mod odg_save_note_trips;
mod odg_secname;
mod odg_section_name_handles;
mod odg_stencils;
mod odg_text;
mod odg_trip_lengths;
mod odg_untouched_routes;
mod odg_untouched_twins;
mod odg_visit_empty_paragraph_style;
mod odg_visit_run_style;
mod odg_visit_shape_ids;
mod odg_visit_shapes;
mod odg_visit_shared_paragraph_style;
mod odg_visit_styles;
mod odg_visit_text;
mod odg_vsdx_objects;
mod odg_wholeglue;
mod odgout_masters;
mod old_namespace;
mod open;
mod orgchart;
mod orgchart_files;
mod page_copy;
mod page_defaults;
mod page_links;
mod page_name;
mod page_names_save;
mod page_order;
mod page_readers;
mod page_setup;
mod page_setup_cost;
mod page_unique;
mod panels;
mod panels_cost;
mod par_open;
mod par_save;
mod picture_frame_images_and_extruded_glue;
mod pictures;
mod place;
mod places_guard;
mod print;
mod prop_empty_prompt;
mod reached_pages;
mod read_room;
mod rebuilt_places;
mod relayout;
mod relayout_files;
mod resave_sweep;
mod resize_locks;
mod resize_set;
mod reverse_ends;
mod root_style_files;
mod route_behavior;
mod route_clear;
mod route_fixed;
mod route_readers;
mod route_readers_cells;
mod route_readers_edits;
mod route_settings;
mod row_types;
mod save;
mod save_check;
mod save_check_before;
mod save_check_cost;
mod save_check_faces;
mod save_check_formulas;
mod save_check_json;
mod save_check_misses;
mod save_check_remaining;
mod save_disk;
mod scale_follow;
mod select;
mod setf_edit;
mod shape_places;
mod shapedata;
mod shapedata_cost;
mod shapedata_files;
mod shapedata_forms;
mod shapeops;
mod shapeops_cases;
mod shapeops_fit;
mod shapeops_hostile;
mod shapeops_join;
mod shapeops_join_trim_hostile;
mod shapeops_offset;
mod shapeops_offset_fit_cases;
mod shapeops_offset_fit_hostile;
mod shapeops_offset_memory;
mod shapeops_trim;
mod shapeops_trim_memory;
mod sheet_cross_pages;
mod sheet_files;
mod sheet_files_world;
mod sheet_hostile;
mod sheet_model;
mod sheet_places;
mod sheet_places_writers;
mod sheet_read;
mod sheet_rows;
mod sheet_unsaved;
mod sheet_write;
mod snap;
mod snap_adoption;
mod snap_commands;
mod snap_drop_points;
mod snap_glue;
mod snap_hand_mutants;
mod snap_kept_reads;
mod snap_library_settings;
mod snap_reads;
mod snap_repin;
mod snap_review_fixes;
mod snap_review_mutants;
mod snap_review_turned;
mod snap_review_words;
mod snap_strength;
mod snap_turned_sides;
mod solution_prolog;
mod stacked_rounding;
mod stencil_hostile;
mod stencil_instances;
mod stencil_kept_text;
mod stencil_line_pins;
mod stencil_open;
mod stencil_quick;
mod stencil_relationship_default;
mod stencil_search;
mod stencils2_bpmn;
mod stencils2_c4;
mod stencils2_chen;
mod stencils2_cloud;
mod stencils2_computers;
mod stencils2_crows_foot;
mod stencils2_network;
mod stencils2_support;
mod stencils2_uml_activity;
mod stencils2_uml_class;
mod stencils2_uml_component;
mod stencils2_uml_sequence;
mod stencils2_uml_state;
mod stencils2_uml_usecase;
mod stencils_in_drawings;
mod style_ids;
mod swimlane;
mod swimlane_cells;
mod swimlane_files;
mod swimlane_hostile;
mod swimlane_template;
mod template;
mod text;
mod text_block;
mod text_box;
mod text_direction_files;
mod text_ext;
mod text_rows;
mod theme;
mod theme_color;
mod theme_color_cost;
mod theme_color_files;
mod theme_cost;
mod theme_file;
mod theme_master_links;
mod theme_orphan;
mod unpaged_readers;
mod unsaved_cost;
mod unsaved_geometry;
mod unsaved_geometry_more;
mod unsaved_layers;
mod unsaved_readers;
mod unsaved_readers_cost;
mod unsaved_rule;
mod view_undo;
mod vsdx_embedded_peak;
mod vsdx_held_parts;
mod vsdx_language_cells;
mod vsdx_memory;
mod vsdx_save_peak;
mod vsdx_str_foreign;
mod vsdx_string_units;
mod vsdx_text_units;
mod wholeglue_group;
mod window_settings;

use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

pub use diacraft_testkit::fixtures::{blank_document, blank_session};

/// Everything a command may change, taken before and after to compare: a command that fails must
/// leave all of it as it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub document: Option<Document>,
    pub undo_steps: usize,
    pub redo_steps: usize,
    pub next_undo: Option<String>,
    pub next_redo: Option<String>,
    pub revision: u64,
    pub journal: Vec<(String, Value)>,
    /// The file the document was opened from: its path and its bytes.
    pub source: Option<(std::path::PathBuf, Vec<u8>)>,
}

pub fn snapshot(s: &Session) -> Snapshot {
    let history = s.history();
    Snapshot {
        document: s.document().cloned(),
        undo_steps: history.map_or(0, |h| h.undo_len()),
        redo_steps: history.map_or(0, |h| h.redo_len()),
        next_undo: history.and_then(|h| h.next_undo_label()).map(str::to_owned),
        next_redo: history.and_then(|h| h.next_redo_label()).map(str::to_owned),
        revision: s.revision(),
        journal: s.journal().to_vec(),
        source: s.source().map(|f| (f.path().to_path_buf(), f.bytes().to_vec())),
    }
}

/// The page names of the session's document, in order.
pub fn page_names(s: &Session) -> Vec<String> {
    s.document().map(|d| d.pages.iter().map(|p| p.name.to_string()).collect()).unwrap_or_default()
}

/// Runs a command that must succeed.
pub fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// A session with a document of two pages, "Baseline" and "Second", made by two edits, so undo has
/// a step beneath the one a test makes.
pub fn session_with_history() -> Session {
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Baseline"}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    s
}

/// The invariants of a session after any call, however hostile: a valid document, a history within
/// its limit and a journal within its bound.
pub fn check_session(s: &Session) -> Result<(), String> {
    if let Some(doc) = s.document() {
        doc.validate().map_err(|e| format!("the document is invalid: {e}"))?;
    }
    if let Some(h) = s.history() {
        if h.undo_len() > h.limit() || h.redo_len() > h.limit() {
            return Err(format!("history holds {} undo and {} redo steps over its limit {}", h.undo_len(), h.redo_len(), h.limit()));
        }
    } else if s.document().is_some() {
        return Err("a document without a history".into());
    }
    if s.journal().len() > diacraft_engine::JOURNAL_LIMIT {
        return Err(format!("the journal shows {} entries", s.journal().len()));
    }
    // What the journal actually holds in memory, not only what `journal()` shows of it.
    let log = s.journal_log();
    if log.held_len() > diacraft_engine::JOURNAL_LIMIT + diacraft_engine::JOURNAL_SLACK {
        return Err(format!("the journal holds {} entries in memory", log.held_len()));
    }
    // Over the byte budget only as a lone newest entry, which is always kept.
    if log.held_bytes() > diacraft_engine::JOURNAL_MAX_BYTES && log.held_len() > 1 {
        return Err(format!("the journal holds {} bytes in {} entries", log.held_bytes(), log.held_len()));
    }
    Ok(())
}

/// The message of a `BadParams` error; any other error fails the test.
pub fn bad_params_msg(e: &EngineError) -> &str {
    match e {
        EngineError::BadParams { msg, .. } => msg,
        other => panic!("expected BadParams, got {other:?}"),
    }
}
