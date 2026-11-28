//! The command line's settings: the person's profile, opened for the commands that run a session of this process, and saved when the command ends.
//!
//! `run` and `mcp` (a session of its own: not `--connect`, which drives the window's session and its settings) open the same `config.toml` the window opens, through
//! the engine's `Profile<EnginePrefs>`: the command line holds the engine's sections and keeps the window's own keys as they are. **`DIACRAFT_NO_PREFS`** (set to anything) and
//! an empty **`DIACRAFT_CONFIG_DIR`** open it in memory, so that nothing is read or written, which is what a script or an agent that must not touch the person's settings sets;
//! `DIACRAFT_CONFIG_DIR` set to a folder moves it there. What is wrong with the profile (a file broken, too big or from a newer build, a folder that cannot be written) is said once
//! on standard error and the command goes on with the defaults or the file's values. The other commands (`commands`, `oracle`, `export`, `convert`, `roundtrip`, `compare`) open none.

use diacraft_engine::prefs::{EnginePrefs, Profile};

use crate::args::{Action, Mode};

/// Whether `action` runs a session of this process, which reads the person's settings.
pub fn wants_profile(action: &Action) -> bool {
    matches!(action, Action::Run(_) | Action::Mcp(Mode::Headless | Mode::Auto))
}

/// The profile of this process's environment, and what is wrong with it, one sentence each, for standard error.
pub fn open() -> (Profile<EnginePrefs>, Vec<String>) {
    let (profile, report) = Profile::<EnginePrefs>::open_from_env();
    (profile, report.notes)
}

/// Saves the profile as the command ends: what could not be saved is said, a profile that was read-only is not told of again (it was told of when it opened).
pub fn close(profile: &mut Profile<EnginePrefs>) -> Option<String> {
    match profile.flush() {
        Ok(()) | Err(diacraft_engine::prefs::PrefsError::ReadOnly(_)) => None,
        Err(e) => Some(format!("the settings were not saved: {e}")),
    }
}
