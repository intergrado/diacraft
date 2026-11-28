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

/// After `--` every word is a file, `--help`, `-h` and `--version` included: `diacraft -- --help` opens a drawing named `--help`.
#[test]
fn after_two_dashes_help_and_version_are_the_names_of_files() {
    for name in ["--help", "--version", "-h"] {
        assert_eq!(file(&["--", name]), Some(name.to_owned()), "`{name}` after the two dashes is a file");
        assert_eq!(file(&["--control", "7997", "--", name]), Some(name.to_owned()), "`{name}` after a port and the two dashes is a file");
    }
    // Before the two dashes they still win, over a help or a version after them as over everything else.
    assert!(matches!(run(&["--help", "--", "--version"], None), Ok(Action::Help)));
    assert!(matches!(run(&["--version", "--", "--help"], None), Ok(Action::Version)));
    assert!(matches!(run(&["--bogus", "-h", "--", "a.vsdx"], None), Ok(Action::Help)));
    // Two such words after the two dashes are two files, and no help.
    let error = run(&["--", "--help", "--version"], None).unwrap_err();
    assert!(error.contains("only one file") && error.contains("--version"), "{error}");
}

#[test]
fn the_usage_text_names_every_option_and_the_variables() {
    for needle in [
        "FILE",
        "--control",
        "--version",
        "--help",
        "DIACRAFT_CONTROL_PORT",
        "DIACRAFT_NO_PREFS",
        "DIACRAFT_CONFIG_DIR",
        "DIACRAFT_NO_SPLASH",
        "DIACRAFT_NO_ERROR_BOX",
        "WGPU_BACKEND",
        "7997",
    ] {
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

// ---- the automation roots (gaps A2 and A5) ------------------------------------------------------------------------------------------------------

use diacraft_desktop::args::{Environment, parse_with};

fn roots(args: &[&str], environment: Environment) -> Result<(Vec<String>, Vec<String>), String> {
    match parse_with(args.iter().map(|a| (*a).to_owned()), environment)? {
        Action::Run(Options { roots, .. }) => {
            let words = |list: &[std::path::PathBuf]| list.iter().map(|p| p.to_str().unwrap().to_owned()).collect::<Vec<_>>();
            Ok((words(roots.read()), words(roots.write())))
        }
        other => panic!("expected to run, got {other:?}"),
    }
}

fn variables(read: Option<&str>, write: Option<&str>) -> Environment {
    Environment { control_port: None, read_roots: read.map(OsString::from), write_roots: write.map(OsString::from) }
}

#[test]
fn the_root_flags_name_folders_in_either_spelling_and_may_be_repeated() {
    let (read, write) = roots(
        &[
            "--control",
            "7997",
            "--automation-read-root",
            "/a",
            "--automation-read-root=/b",
            "--automation-write-root",
            "/w",
            "--automation-write-root=/x",
        ],
        Environment::default(),
    )
    .unwrap();
    assert_eq!((read, write), (vec!["/a".to_owned(), "/b".to_owned()], vec!["/w".to_owned(), "/x".to_owned()]));
    // With no flag there is no root, and so nothing is kept to a folder: what the program did before.
    assert_eq!(roots(&["--control", "7997"], Environment::default()).unwrap(), (vec![], vec![]));
}

/// The roots keep the control channel to folders, so a program started with no control channel and with a root flag would be one that was asked for something it does
/// not do: nothing is ignored.
#[test]
fn a_root_flag_with_no_control_channel_is_an_error() {
    let error = roots(&["--automation-read-root", "/a"], Environment::default()).unwrap_err();
    assert!(error.contains("--automation-read-root") && error.contains("--control"), "{error}");
    let error = roots(&["--automation-write-root=/w"], Environment::default()).unwrap_err();
    assert!(error.contains("--automation-write-root") && error.contains("--control"), "{error}");
    // A port from the environment is a control channel too.
    assert!(roots(&["--automation-read-root", "/a"], Environment { control_port: Some("7001".into()), ..Environment::default() }).is_ok());
}

#[test]
fn a_root_flag_needs_a_folder() {
    for args in [&["--control", "7997", "--automation-read-root"][..], &["--control", "7997", "--automation-write-root="][..]] {
        let error = roots(args, Environment::default()).unwrap_err();
        assert!(error.contains("needs a folder"), "{args:?}: {error}");
    }
}

#[test]
fn the_environment_names_roots_when_the_flags_do_not() {
    let list = |names: &[&str]| std::env::join_paths(names).unwrap().into_string().unwrap();
    let (read_env, write_env) = (list(&["/e1", "/e2"]), list(&["/w1"]));
    let (read, write) = roots(&["--control", "7997"], variables(Some(&read_env), Some(&write_env))).unwrap();
    assert_eq!((read, write), (vec!["/e1".to_owned(), "/e2".to_owned()], vec!["/w1".to_owned()]));
    // A flag of a kind wins over the environment for that kind, and the other kind keeps the environment's.
    let (read, write) = roots(&["--control", "7997", "--automation-read-root", "/f"], variables(Some(&read_env), Some(&write_env))).unwrap();
    assert_eq!((read, write), (vec!["/f".to_owned()], vec!["/w1".to_owned()]));
    // The environment on its own, with no control channel, asks nothing of the program: a person who sets it for every start is not stopped for the starts that have none.
    assert!(roots(&[], variables(Some(&read_env), None)).is_ok());
}

#[test]
fn a_file_named_like_a_root_flag_after_two_dashes_is_a_file() {
    assert_eq!(file(&["--", "--automation-read-root"]), Some("--automation-read-root".to_owned()));
}

#[test]
fn the_help_says_what_the_roots_are_and_how_to_give_them() {
    for word in ["--automation-read-root", "--automation-write-root", "DIACRAFT_AUTOMATION_READ_ROOT", "DIACRAFT_AUTOMATION_WRITE_ROOT", "folder"] {
        assert!(USAGE.contains(word), "the help does not say {word}");
    }
}

#[test]
fn version_and_help_come_before_a_bad_root_flag() {
    assert!(matches!(parse_with(["--automation-read-root".to_owned(), "--version".to_owned()], Environment::default()), Ok(Action::Version)));
    assert!(matches!(parse_with(["--automation-write-root".to_owned(), "-h".to_owned()], Environment::default()), Ok(Action::Help)));
}

// ---- --reset-graphics ----

fn resets(args: &[&str]) -> Result<bool, String> {
    match run(args, None)? {
        Action::Run(Options { reset_graphics, .. }) => Ok(reset_graphics),
        other => panic!("expected to run, got {other:?}"),
    }
}

#[test]
fn reset_graphics_is_an_option_of_the_start_and_opens_the_window_as_usual() {
    assert_eq!(resets(&[]), Ok(false), "the adapters left out stay left out unless it is asked for");
    assert_eq!(resets(&["--reset-graphics"]), Ok(true));
    assert_eq!(resets(&["--reset-graphics", "--reset-graphics"]), Ok(true), "said twice is said once");
    // It leaves the file and the control channel as they were.
    match run(&["--reset-graphics", "--control", "7997", "drawing.vsdx"], None).unwrap() {
        Action::Run(Options { reset_graphics, control_port, path, .. }) => {
            assert!(reset_graphics);
            assert_eq!(control_port, Some(7997));
            assert_eq!(path.as_deref(), Some("drawing.vsdx"));
        }
        other => panic!("{other:?}"),
    }
    // After the two dashes it is a file's name.
    match run(&["--", "--reset-graphics"], None).unwrap() {
        Action::Run(Options { reset_graphics, path, .. }) => assert_eq!((reset_graphics, path.as_deref()), (false, Some("--reset-graphics"))),
        other => panic!("{other:?}"),
    }
    // Only the whole word is the option.
    assert!(run(&["--reset-graphic"], None).unwrap_err().contains("unknown argument"));
}

#[test]
fn the_usage_text_documents_reset_graphics_and_the_probe_fault_diagnostic() {
    for needle in ["--reset-graphics", "DIACRAFT_GPU_PROBE_FAIL", ":abort", ":hang", "DirectX 12 before Vulkan"] {
        assert!(USAGE.contains(needle), "{needle} missing from the usage text");
    }
    let seconds = diacraft_desktop::graphics::PROBE_GIVE_UP.as_secs();
    assert!(USAGE.contains(&format!("gives up after {seconds} s")), "the usage text names the time the start waits for a test ({seconds} s)");
    assert_eq!(seconds, 12);
}
