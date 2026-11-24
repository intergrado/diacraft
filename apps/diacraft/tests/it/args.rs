//! The command line: `--control PORT`, `DIACRAFT_CONTROL_PORT`, `--version`, `--help`, and a file to open.

use std::ffi::OsString;

use diacraft_desktop::args::{Action, DEFAULT_CONTROL_PORT, Options, USAGE, parse, splash_enabled_with};

fn run(args: &[&str], env: Option<&str>) -> Result<Action, String> {
    parse(args.iter().map(|a| (*a).to_owned()), env.map(OsString::from))
}

fn port(args: &[&str], env: Option<&str>) -> Option<u16> {
    match run(args, env).unwrap() {
        Action::Run(Options { control_port, .. }) => control_port,
        other => panic!("expected to run, got {other:?}"),
    }
}

#[test]
fn no_arguments_start_the_app_without_a_control_channel() {
    assert_eq!(port(&[], None), None);
    assert_eq!(port(&[], Some("")), None, "an empty variable is no variable");
}

#[test]
fn control_takes_a_port_in_either_spelling() {
    assert_eq!(port(&["--control", "7997"], None), Some(7997));
    assert_eq!(port(&["--control=7997"], None), Some(7997));
    assert_eq!(port(&["--control", "1"], None), Some(1));
    assert_eq!(port(&["--control", "65535"], None), Some(65535));
    assert_eq!(DEFAULT_CONTROL_PORT, 7997);
}

#[test]
fn a_port_must_be_a_number_from_1_to_65535() {
    for bad in ["0", "65536", "-1", "+7997", "7997 ", " 7997", "0x1f", "7_997", "7997.0", "", "abc", "99999999999999999999", "\u{663}\u{663}"] {
        let error = run(&["--control", bad], None).unwrap_err();
        assert!(error.contains("--control") && error.contains("1 to 65535"), "{bad:?}: {error}");
        let error = run(&[&format!("--control={bad}")], None).unwrap_err();
        assert!(error.contains("--control"), "{bad:?}: {error}");
    }
}

/// Only what is dangerous is escaped: an argument people typed (an apostrophe, an accent) is echoed as typed.
#[test]
fn an_error_echoes_an_argument_as_typed_except_what_is_dangerous() {
    let error = run(&["--it's-here"], None).unwrap_err();
    assert!(error.contains("`--it's-here`"), "{error}");
    let error = run(&["--cafe\u{301}\u{1b}"], None).unwrap_err();
    assert!(error.contains("--cafe\u{301}\\u{1b}"), "{error}");
}

/// The error does not echo a hostile argument raw.
#[test]
fn an_error_never_echoes_control_characters() {
    for args in [vec!["--control", "\u{1b}[2J"], vec!["--\u{1b}x"], vec!["a.vsdx", "\u{1b}[2J"]] {
        let error = run(&args, None).unwrap_err();
        assert!(!error.chars().any(char::is_control), "{error:?}");
    }
}

#[test]
fn control_needs_a_value_and_is_given_once() {
    assert!(run(&["--control"], None).unwrap_err().contains("needs a port"));
    assert!(run(&["--control", "7997", "--control", "7998"], None).unwrap_err().contains("twice"));
    assert!(run(&["--control=7997", "--control", "7998"], None).unwrap_err().contains("twice"));
}

#[test]
fn the_environment_names_a_port_when_the_flag_does_not() {
    assert_eq!(port(&[], Some("7001")), Some(7001));
    assert_eq!(port(&["--control", "7002"], Some("7001")), Some(7002), "the flag wins");
    let error = run(&[], Some("seven")).unwrap_err();
    assert!(error.contains("DIACRAFT_CONTROL_PORT") && error.contains("1 to 65535"), "{error}");
}

/// A bad environment value does not stop `--version`, `--help`, or a flag that overrides it.
#[test]
fn a_bad_environment_does_not_stop_version_help_or_the_flag() {
    assert!(matches!(run(&["--version"], Some("seven")), Ok(Action::Version)));
    assert!(matches!(run(&["--help"], Some("seven")), Ok(Action::Help)));
    assert_eq!(port(&["--control", "7002"], Some("seven")), Some(7002));
}

#[test]
fn version_and_help_come_before_anything_else_is_judged() {
    assert!(matches!(run(&["--version"], None), Ok(Action::Version)));
    assert!(matches!(run(&["-h"], None), Ok(Action::Help)));
    assert!(matches!(run(&["--help"], None), Ok(Action::Help)));
    // Even after a bad port or an unknown option: asking for help always works.
    assert!(matches!(run(&["--control", "0", "--version"], None), Ok(Action::Version)));
    assert!(matches!(run(&["--bogus", "--help"], None), Ok(Action::Help)));
}

#[test]
fn anything_else_is_an_error_not_ignored() {
    let error = run(&["--controll", "7997"], None).unwrap_err();
    assert!(error.contains("--controll") && error.contains("--help"), "{error}");
    let error = run(&["-x"], None).unwrap_err();
    assert!(error.contains("-x"), "{error}");
}

fn file(args: &[&str]) -> Option<String> {
    match run(args, None).unwrap() {
        Action::Run(Options { path, .. }) => path,
        other => panic!("expected to run, got {other:?}"),
    }
}

/// `diacraft FILE`: the one name that is not an option is the drawing to open, wherever it stands among the options.
#[test]
fn a_file_name_is_the_drawing_to_open() {
    assert_eq!(file(&[]), None);
    assert_eq!(file(&["drawing.vsdx"]), Some("drawing.vsdx".to_owned()));
    assert_eq!(file(&["--control", "7997", "drawing.vsdx"]), Some("drawing.vsdx".to_owned()));
    assert_eq!(file(&["drawing.vsdx", "--control=7997"]), Some("drawing.vsdx".to_owned()));
    assert_eq!(file(&["/home/someone/My Plans/floor plan.vsdx"]), Some("/home/someone/My Plans/floor plan.vsdx".to_owned()));
    assert_eq!(port(&["drawing.vsdx"], None), None);
}

/// A name with a dash first is a file after `--`, and a dash on its own stands for itself.
#[test]
fn a_file_whose_name_begins_with_a_dash_follows_two_dashes() {
    assert_eq!(file(&["--", "-odd.vsdx"]), Some("-odd.vsdx".to_owned()));
    assert_eq!(file(&["--", "--control"]), Some("--control".to_owned()), "after `--` nothing is an option");
    assert_eq!(file(&["--control", "7997", "--", "-odd.vsdx"]), Some("-odd.vsdx".to_owned()));
    assert!(run(&["-odd.vsdx"], None).is_err(), "without `--` it is an unknown option");
}

#[test]
fn a_second_file_is_an_error_and_says_so() {
    let error = run(&["a.vsdx", "b.vsdx"], None).unwrap_err();
    assert!(error.contains("only one file") && error.contains("b.vsdx"), "{error}");
    assert!(run(&["--", "a.vsdx", "b.vsdx"], None).is_err());
}

/// `--version` and `--help` still win over a file.
#[test]
fn version_and_help_win_over_a_file() {
    assert!(matches!(run(&["a.vsdx", "--version"], None), Ok(Action::Version)));
    assert!(matches!(run(&["a.vsdx", "b.vsdx", "--help"], None), Ok(Action::Help)));
}

#[test]
fn the_usage_text_names_every_option_and_the_variables() {
    for needle in ["FILE", "--control", "--version", "--help", "DIACRAFT_CONTROL_PORT", "DIACRAFT_NO_PREFS", "DIACRAFT_NO_SPLASH", "7997"] {
        assert!(USAGE.contains(needle), "{needle} missing from the usage text");
    }
}

/// The window shows its splash when it starts, unless `DIACRAFT_NO_SPLASH` is set, whatever it holds (as `DIACRAFT_NO_PREFS` is): a person drives a window that
/// has a splash over it with clicks that it would use up. The control channel does not turn it off by itself: it is how the splash is looked at.
#[test]
fn the_splash_shows_unless_no_splash_is_set() {
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |key| pairs.iter().find(|pair| pair.0 == key).map(|pair| OsString::from(pair.1))
    }
    assert!(splash_enabled_with(&env(&[])), "nothing set: the splash shows");
    assert!(!splash_enabled_with(&env(&[("DIACRAFT_NO_SPLASH", "1")])));
    assert!(!splash_enabled_with(&env(&[("DIACRAFT_NO_SPLASH", "")])), "set at all, whatever it holds");
    assert!(splash_enabled_with(&env(&[("DIACRAFT_NO_PREFS", "1"), ("DIACRAFT_CONTROL_PORT", "7997")])), "no other variable turns it off");
}
