//! The window's settings: its type of `config.toml`, and how the window holds, saves and tells of the profile.
//!
//! **One store, behind the engine's [`Profile`].** The window holds a `Profile<WindowPrefs>`: on the desktop craft-settings keeps `config.toml` in the platform's config
//! folder, in the browser the same text is a `localStorage` key behind a [`PrefsHost`] (`apps/diacraft-web/src/web_prefs.rs`), and in a test it is in memory (`Profile::in_memory`);
//! the rest of the window is the same code on all three. The window saves what changed after a quiet period (the engine's `tick`, once a frame, which also asks for the frame
//! that saves an idle window) and when it closes. A program that drives the window changes the same settings through the engine's commands and the profile's `update`,
//! each change named.
//!
//! **Room for the other designs' keys.** [`WindowPrefs`] holds the engine's sections (`[proofing]`, `[autocorrect]`, `[language]`: the command line and the MCP server read the
//! same ones) and the window's own, which are fields of [`InterfacePrefs`] (`[interface]`) and of sections beside it. The display language and region
//! tip and search settings (`screentip_style`, `shapes_window_tips`, `other_tips`, `tip_keys`, `collapse_search`, `recent_search`, `last_seen_version`) and CPDOUBLE's minimum distance
//! each add their fields there, with `#[serde(default)]` and a default, and change nothing in the store. A key a build does not know stays in the file.

pub use diacraft_engine::prefs::{
    Change, Edit, FileHost, FileKind, FileName, LocaleSource, MemoryHost, PrefsError, PrefsHost, Profile, ProfileChange, Report, Tick, changes_json,
};
use diacraft_engine::prefs::{EnginePrefs, Prefs, SCHEMA_VERSION, Settings};
use serde::{Deserialize, Serialize};

use crate::app::DiacraftApp;

/// What the window keeps in `config.toml`: the engine's sections and its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPrefs {
    /// `[proofing]`, `[autocorrect]` and `[language]`, as the command line and the MCP server read them.
    #[serde(flatten)]
    pub engine: EnginePrefs,
    /// `[interface]`: the window's own settings.
    pub interface: InterfacePrefs,
}

/// `[interface]`: the settings of the window that are not the engine's. Fields are added here by the designs that need them (see the module).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfacePrefs {}

impl Settings for WindowPrefs {
    const APP_ID: &'static str = EnginePrefs::APP_ID;
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;
}

impl Prefs for WindowPrefs {
    fn engine(&self) -> &EnginePrefs {
        &self.engine
    }

    fn engine_mut(&mut self) -> &mut EnginePrefs {
        &mut self.engine
    }

    fn sanitize(&mut self) -> Vec<String> {
        // The engine's sections are brought into range here; a field of the window's own that needs a bound adds its check below.
        self.engine.bring_into_range()
    }
}

impl DiacraftApp {
    /// Gives the window its settings: it saves what changes, and when it closes. The program that opens the profile tells the person what its [`Report`] says
    /// (`with_notice`), once.
    #[must_use]
    pub fn with_prefs(mut self, profile: Profile<WindowPrefs>) -> DiacraftApp {
        self.prefs = Some(profile);
        self
    }

    /// The window's settings, when it has them.
    pub fn prefs(&self) -> Option<&Profile<WindowPrefs>> {
        self.prefs.as_ref()
    }

    /// The window's settings, to change. A change is named by what [`Profile::update`] returns.
    pub fn prefs_mut(&mut self) -> Option<&mut Profile<WindowPrefs>> {
        self.prefs.as_mut()
    }

    /// Saves what has waited out its quiet period, asks for the frame that saves what is still waiting, and tells the person what the profile has to say: a file that was
    /// broken while the window ran and moved aside, a save that failed (once).
    pub(crate) fn advance_prefs(&mut self, ctx: &egui::Context) {
        let Some(profile) = self.prefs.as_mut() else { return };
        let tick = profile.tick();
        if let Some(wait) = tick.next_in {
            ctx.request_repaint_after(wait);
        }
        let mut notes = profile.take_notes();
        if let Some(failed) = tick.failed {
            notes.push(format!("DiaCraft could not save its settings: {failed}"));
        }
        if !notes.is_empty() {
            self.set_notice(&notes.join("; "));
        }
    }

    /// Saves the settings now, as the window closes. A profile that is read-only was told of when it opened and is not told of again.
    pub(crate) fn flush_prefs(&mut self) {
        if let Some(profile) = self.prefs.as_mut()
            && let Err(e) = profile.flush()
        {
            log::warn!("the settings were not saved when the window closed: {e}");
        }
    }
}
