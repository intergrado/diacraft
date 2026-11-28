//! The window's settings: the window holds a profile, saves what changed at the next frame and when it closes, and tells the person what the profile has to say.
//! The profile here is in a memory host: nothing reads or writes the real settings.

use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_engine::prefs::SCHEMA_VERSION;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::prefs_host::{LocaleSource, MemoryHost, Profile, WindowPrefs};
use eframe::App;

use crate::support::window;

fn profile(host: &Arc<MemoryHost>) -> Profile<WindowPrefs> {
    Profile::<WindowPrefs>::open_host(host.clone(), LocaleSource::Fixed(Some("fr_FR.UTF-8".to_owned()))).0
}

#[test]
fn the_window_saves_what_changed_at_the_next_frame() {
    let host = Arc::new(MemoryHost::default());
    let mut w = window(DiacraftApp::new(Session::new(), None).with_prefs(profile(&host)), [900.0, 600.0]);
    w.step();
    assert_eq!(host.config_text(), None, "a window that nobody changed writes nothing");
    let named = w.state_mut().prefs_mut().unwrap().update(|p| p.engine.proofing.grammar = true);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].describe(), "proofing.grammar: false -> true");
    w.step();
    let text = host.config_text().unwrap();
    assert!(text.contains("grammar = true") && text.contains("editing = [\"fr-FR\"]"), "{text}");
}

#[test]
fn the_window_saves_when_it_closes() {
    let host = Arc::new(MemoryHost::default());
    let mut w = window(DiacraftApp::new(Session::new(), None).with_prefs(profile(&host)), [900.0, 600.0]);
    w.step();
    w.state_mut().prefs_mut().unwrap().update(|p| p.engine.autocorrect.smart_quotes = false);
    // No frame in between: the window is closing.
    w.state_mut().on_exit();
    let text = host.config_text().unwrap();
    assert!(text.contains("smart_quotes = false"), "{text}");
}

#[test]
fn the_window_keeps_the_engines_sections_and_its_own_in_one_file() {
    let host = Arc::new(MemoryHost::default());
    let mut p = profile(&host);
    p.flush().unwrap();
    let text = host.config_text().unwrap();
    for section in ["[proofing]", "[autocorrect]", "[autocorrect.exceptions]", "[language]", "[interface]"] {
        assert!(text.contains(section), "{section} in\n{text}");
    }
    assert!(text.starts_with(&format!("schema_version = {SCHEMA_VERSION}")), "{text}");
}

#[test]
fn what_the_profile_has_to_say_is_the_status_bars_notice() {
    let host = Arc::new(MemoryHost::default());
    let mut w = window(DiacraftApp::new(Session::new(), None).with_prefs(profile(&host)), [900.0, 600.0]);
    w.step();
    assert_eq!(w.state().notice(), None);
    // A program set an editing language that is not a language tag: it was brought into range, and the person is told once.
    w.state_mut().prefs_mut().unwrap().update(|p| p.engine.language.editing = vec!["not a tag".to_owned(), "de-DE".to_owned()]);
    w.step();
    let notice = w.state().notice().unwrap_or_default().to_owned();
    assert!(notice.contains("editing languages dropped"), "{notice}");
    assert_eq!(w.state().prefs().unwrap().get().engine.language.editing, vec!["de-DE".to_owned()]);
}

#[test]
fn a_window_with_no_settings_is_as_it_was() {
    let mut w = window(DiacraftApp::new(Session::new(), None), [900.0, 600.0]);
    w.step();
    assert!(w.state().prefs().is_none());
    w.state_mut().on_exit();
}

#[test]
fn a_read_only_profile_does_not_stop_the_window_closing() {
    let host = Arc::new(MemoryHost::default());
    host.set_config("schema_version = 99\n");
    let mut w = window(DiacraftApp::new(Session::new(), None).with_prefs(profile(&host)), [900.0, 600.0]);
    w.step();
    w.state_mut().prefs_mut().unwrap().update(|p| p.engine.proofing.hide_errors = true);
    w.step();
    w.state_mut().on_exit();
    assert_eq!(host.config_text().as_deref(), Some("schema_version = 99\n"), "a newer file is never written over");
}
