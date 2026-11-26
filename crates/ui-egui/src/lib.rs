//! The DiaCraft egui interface: the window and the control channel's side of it.
//!
//! [`DiacraftApp`] is an `eframe::App`. It owns a [`diacraft_engine::Session`] and reads from it; the
//! window changes the document only by running commands (`DiacraftApp::execute`). Colours come from
//! [`theme::Tokens`] and nowhere else.
//!
//! The window is the layout: a title bar (`title_bar`), a tab row and ribbon
//! ([`ribbon`]), the Shapes window (`shapes_window`), the drawing area ([`canvas`]) with its page tabs
//! (`page_tabs`), a status bar (`status_bar`), and the File view (`backstage`) over them. Icons are drawn
//! in code ([`icons`]). What the window itself remembers is in [`state`], and the commands that change it in
//! [`commands`].
//!
//! At its start the window shows a splash popup (the octopus cover, the version, the git release, the year and the licence), and `app.about` shows it again ([`splash`]).
//!
//! The control channel is how tests, agents and the CLI drive the running window. The transport (a
//! loopback TCP server) belongs to the desktop app; this crate takes a [`ControlInbox`], a channel of
//! [`ControlRequest`]s, and answers each on the frame thread. See [`control`] for the methods and
//! `docs/control-protocol.md` for the wire protocol.
//!
//! Nothing here depends on a windowing backend, so the crate builds for the web as well; only the
//! control channel and screenshots are never used there.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod app;
mod arrange;
mod ask;
mod asset_host;
mod backstage;
pub mod branding;
pub mod canvas;
pub mod clipboard;
pub mod clock;
mod colors_dialog;
pub mod commands;
mod comments_ui;
mod container_ui;
pub mod control;
pub mod edit;
mod export;
mod file_host;
mod file_open;
mod find_dialog;
mod findings;
pub mod font_list;
pub mod fonts;
pub mod format;
mod format_fx;
pub mod icon_image;
pub mod icons;
mod insert_callout;
mod insert_field;
mod insert_links;
mod insert_picture;
mod insert_symbol;
#[cfg(kani)]
mod kani_proofs;
pub mod layers_dialog;
pub mod licenses;
mod measure;
pub mod new_page;
pub mod notice;
pub mod operations;
mod options_dialog;
mod org_ui;
mod page_host;
mod page_setup;
mod page_tabs;
mod painter;
mod panes;
pub mod print;
mod print_preview;
mod print_view;
mod relayout_ui;
pub mod render_worker;
pub mod ribbon;
mod save;
mod save_kinds;
mod save_note;
mod select_by_type;
mod shape_data;
mod shapes_window;
mod sheet;
pub mod shortcuts;
pub mod snap_dialog;
pub mod snap_reader;
pub mod splash;
pub mod state;
mod status_bar;
pub mod stencil_files;
pub mod stencils;
mod swimlane_ui;
mod text_dialog;
pub mod theme;
mod theme_preview;
pub mod tile_ends;
mod title_bar;
mod widgets;
pub mod window_settings;
pub mod work;
mod zoom_dialog;

pub use app::{DiacraftApp, Element, Probe, Stage, WindowButton, WindowButtons};
pub use asset_host::{Asset, AssetHost, Bytes, Fetch};
pub use control::{ControlInbox, ControlReply, ControlRequest, Ticket};
pub use export::{ExportPicker, export_failure_text};
pub use file_host::{Delivered, FileHost, Want, place, safe_name};
pub use file_open::{FilePicker, refusal_text};
pub use page_host::{Browser, Offer, PageHost, browser_of, offer_for};
pub use page_setup::{DialogTab, PageSetupState};
pub use panes::{Branch, Pane, PanesState, Space};
pub use print::{Destination, Duplex, PageRange, PrintCancel, PrintChoices, PrintJob, PrintService, PrinterInfo};
pub use save_kinds::{SaveKind, SavePicker, SaveRequest};
pub use state::{CanvasReport, PictureReport};
