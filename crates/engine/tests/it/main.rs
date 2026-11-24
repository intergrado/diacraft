//! The engine's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod across;
mod arrange;
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
mod connect_auto;
mod connect_cost;
mod connect_files;
mod connect_hand;
mod connect_jumps;
mod connect_neighbour;
mod connect_reroute;
mod connect_setting;
mod connect_style;
mod container;
mod container_cycles;
mod container_files;
mod container_hostile;
mod container_more;
mod draw_tools;
mod drop_resave;
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
mod export;
mod face_query;
mod fields;
mod fields_odg;
mod find;
mod find_cost;
mod format;
mod format_cost;
mod format_file;
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
mod group;
mod group_frame;
mod group_geometry;
mod group_size;
mod guard;
mod history;
mod history_bytes;
mod hyperlinks;
mod inherit_pass;
mod insert_files;
mod journal;
mod layers;
mod layers_files;
mod layers_locked_edits;
mod layers_locked_last;
mod layers_locked_more;
mod master_references;
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
mod odg_build;
mod odg_carry_names;
mod odg_connect;
mod odg_custom_glue_and_links;
mod odg_faults;
mod odg_file;
mod odg_findings_trip;
mod odg_flat;
mod odg_flat_saveas;
mod odg_flipglue;
mod odg_flipturn;
mod odg_frameglue;
mod odg_fx;
mod odg_glue_writes;
mod odg_gradients;
mod odg_group_keys;
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
mod odg_vsdx_objects;
mod odg_wholeglue;
mod odgout_masters;
mod old_namespace;
mod open;
mod orgchart;
mod orgchart_files;
mod page_copy;
mod page_setup;
mod page_setup_cost;
mod panels;
mod panels_cost;
mod par_open;
mod par_save;
mod picture_frame_images_and_extruded_glue;
mod pictures;
mod place;
mod print;
mod prop_empty_prompt;
mod read_room;
mod relayout;
mod relayout_files;
mod resave_sweep;
mod resize_set;
mod route_clear;
mod save;
mod save_check;
mod save_check_cost;
mod save_check_json;
mod save_check_misses;
mod save_check_remaining;
mod save_disk;
mod select;
mod shapedata;
mod shapedata_cost;
mod shapedata_files;
mod shapedata_forms;
mod shapeops;
mod shapeops_cases;
mod shapeops_hostile;
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
mod stencil_hostile;
mod stencil_instances;
mod stencil_kept_text;
mod stencil_line_pins;
mod stencil_open;
mod stencil_quick;
mod stencil_relationship_default;
mod stencil_search;
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
mod text_ext;
mod theme;
mod theme_color;
mod theme_color_cost;
mod theme_color_files;
mod theme_cost;
mod theme_file;
mod view_undo;
mod vsdx_embedded_peak;
mod vsdx_held_parts;
mod vsdx_memory;
mod vsdx_save_peak;
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
