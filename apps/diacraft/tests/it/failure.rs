//! What DiaCraft tells the person when it cannot start (`failure`; the Windows start that did nothing: `diacraft.exe` has no console, so a start that failed
//! left no trace at all). The text of the message box, when a box is shown, and that every failure path goes through the one place that shows it.

use std::cell::RefCell;
use std::path::PathBuf;

use diacraft_desktop::failure::{Cause, Failure, LogPlace, NO_BOX_ENV, Surroundings, report};
use diacraft_desktop::graphics::System;

fn log_file(system: System) -> LogPlace {
    LogPlace::File(match system {
        System::Windows => PathBuf::from(r"C:\Users\ana\AppData\Roaming\diacraft\logs\diacraft.log"),
        System::MacOs => PathBuf::from("/Users/ana/Library/Application Support/diacraft/logs/diacraft.log"),
        System::Other => PathBuf::from("/home/ana/.config/diacraft/logs/diacraft.log"),
    })
}

fn failure(system: System, cause: Cause) -> Failure {
    Failure { cause, log: log_file(system), forced_backend: None, system, left_out: Vec::new() }
}

fn window(reasons: &[&str]) -> Cause {
    Cause::Window(reasons.iter().map(|r| (*r).to_owned()).collect())
}

// ---- the words ----

#[test]
fn the_box_says_that_diacraft_could_not_start_why_where_the_log_is_and_what_to_try() {
    let f = failure(System::Windows, window(&["the usual graphics: Adapter selection failed: no graphics adapter can show the window (0 listed)"]));
    let text = f.message();
    assert_eq!(f.title(), "DiaCraft could not start");
    assert!(text.starts_with("DiaCraft could not open its window."), "{text}");
    assert!(text.contains("no graphics adapter can show the window (0 listed)"), "the reason, as the error says it: {text}");
    assert!(text.contains(r"C:\Users\ana\AppData\Roaming\diacraft\logs\diacraft.log"), "the full path of the log: {text}");
    assert!(text.contains("Update the graphics driver"), "{text}");
    assert!(text.contains("WGPU_BACKEND") && text.contains("dx12") && text.contains("gl"), "{text}");
    assert!(text.contains("set WGPU_BACKEND=dx12"), "how to set it in a Command Prompt: {text}");
}

#[test]
fn every_try_that_failed_is_listed_with_its_reason() {
    let f = failure(System::Windows, window(&["the usual graphics: one", "DirectX 12 only: two", "a software adapter: three"]));
    let text = f.message();
    for reason in ["one", "two", "three"] {
        assert!(text.contains(reason), "{reason}: {text}");
    }
    let first = text.find("the usual graphics: one").unwrap();
    let last = text.find("a software adapter: three").unwrap();
    assert!(first < last, "in the order they were tried: {text}");
}

#[test]
fn a_long_list_of_reasons_and_a_long_reason_are_cut_so_the_box_stays_readable() {
    let many: Vec<String> = (0..20).map(|n| format!("try {n}: {}", "x".repeat(900))).collect();
    let f = failure(System::Windows, Cause::Window(many));
    let text = f.message();
    assert!(text.chars().count() < 2400, "{} characters", text.chars().count());
    assert!(text.contains("try 0:") && !text.contains("try 19:"), "the first reasons are kept: {text}");
    assert!(text.contains("more"), "and the box says that there were more: {text}");
    assert!(text.contains(r"diacraft.log"), "the log's place is never the part cut: {text}");
}

#[test]
fn a_back_end_the_person_asked_for_is_named_and_the_box_says_it_was_not_swapped() {
    let mut f = failure(System::Windows, window(&["the usual graphics: no graphics adapter can show the window (0 listed)"]));
    f.forced_backend = Some("metal".to_owned());
    let text = f.message();
    assert!(text.contains(r#"WGPU_BACKEND is set to "metal""#), "{text}");
    assert!(text.contains("only that") && text.contains("Remove it"), "DiaCraft tried only what was asked, and the way out is to remove it: {text}");
    let plain = failure(System::Windows, window(&["x"])).message();
    assert!(!plain.contains("is set to"), "nothing is said about a variable that is not set: {plain}");
}

#[test]
fn a_run_with_no_log_file_says_so_instead_of_a_path() {
    let mut f = failure(System::Windows, window(&["x"]));
    f.log = LogPlace::Unkept;
    let text = f.message();
    assert!(text.contains("keeps no log file"), "{text}");
    assert!(!text.contains("diacraft.log"), "{text}");
}

#[test]
fn the_advice_fits_the_system() {
    let mac = failure(System::MacOs, window(&["x"])).message();
    assert!(!mac.contains("Command Prompt") && !mac.contains("dx12"), "{mac}");
    assert!(mac.contains("Update macOS"), "{mac}");
    let linux = failure(System::Other, window(&["x"])).message();
    assert!(!linux.contains("Command Prompt") && !linux.contains("dx12"), "{linux}");
    assert!(linux.contains("WGPU_BACKEND=gl") && linux.contains("WGPU_BACKEND=vulkan"), "{linux}");
    assert!(linux.contains("graphics driver"), "{linux}");
}

#[test]
fn a_panic_is_an_internal_error_to_report_with_the_log() {
    let f = failure(System::Windows, Cause::Panic("index out of range".to_owned()));
    assert_eq!(f.title(), "DiaCraft stopped");
    let text = f.message();
    assert!(text.starts_with("DiaCraft stopped on an internal error."), "{text}");
    assert!(text.contains("index out of range") && text.contains("report") && text.contains("diacraft.log"), "{text}");
    assert!(!text.contains("WGPU_BACKEND"), "a bug in DiaCraft is not a graphics driver: {text}");
}

#[test]
fn a_command_line_or_folder_error_is_said_as_it_is_without_graphics_advice() {
    let args = failure(System::Windows, Cause::Arguments("--control needs a port from 1 to 65535".to_owned()));
    let text = args.message();
    assert!(text.contains("--control needs a port from 1 to 65535"), "{text}");
    assert!(!text.contains("WGPU_BACKEND") && !text.contains("graphics driver"), "{text}");
    assert_eq!(args.exit_code(), 1);
    let folders = failure(System::Windows, Cause::Folders("no such folder: D:\\x".to_owned()));
    assert!(folders.message().contains("no such folder"), "{}", folders.message());
    assert_eq!(folders.exit_code(), 2, "the code a bad automation folder has always had");
    assert_eq!(failure(System::Windows, window(&["x"])).exit_code(), 1);
    assert_eq!(failure(System::Windows, Cause::Panic("x".into())).exit_code(), 1);
}

#[test]
fn the_reasons_of_a_try_are_one_line_each_in_standard_error_and_the_log() {
    let f = failure(System::Windows, window(&["the usual graphics: one\nsecond line", "DirectX 12 only: two"]));
    let line = f.one_line();
    assert!(!line.contains('\n'), "{line:?}");
    assert!(line.contains("one") && line.contains("two"), "{line}");
}

// ---- when the box is shown ----

fn surroundings(system: System) -> Surroundings {
    Surroundings { system, stderr_is_terminal: false, opted_out: false, scripted: false, has_display: true }
}

#[test]
fn a_start_with_nothing_to_read_standard_error_gets_the_box() {
    for system in [System::Windows, System::MacOs, System::Other] {
        assert!(surroundings(system).shows_box(&window(&["x"])), "{system:?}");
        assert!(surroundings(system).shows_box(&Cause::Panic("x".into())), "{system:?}");
    }
}

#[test]
fn a_terminal_a_script_an_opt_out_or_no_display_keep_the_box_away() {
    let base = surroundings(System::Windows);
    assert!(!Surroundings { stderr_is_terminal: true, ..base }.shows_box(&window(&["x"])), "the person reads standard error");
    assert!(
        !Surroundings { scripted: true, ..base }.shows_box(&window(&["x"])),
        "--control: a script reads the exit code and the log, and a box would hold it up"
    );
    assert!(!Surroundings { opted_out: true, ..base }.shows_box(&window(&["x"])), "{NO_BOX_ENV}");
    assert!(!Surroundings { has_display: false, ..base }.shows_box(&window(&["x"])), "no screen to show it on");
}

#[test]
fn a_command_line_error_gets_a_box_on_windows_only() {
    // On Windows nothing else would show it (no console). Elsewhere a person who mistyped a command has the terminal, and a script has the exit code.
    let args = Cause::Arguments("x".to_owned());
    assert!(surroundings(System::Windows).shows_box(&args));
    assert!(surroundings(System::Windows).shows_box(&Cause::Folders("x".to_owned())));
    assert!(!surroundings(System::MacOs).shows_box(&args));
    assert!(!surroundings(System::Other).shows_box(&args));
    assert!(!surroundings(System::Other).shows_box(&Cause::Folders("x".to_owned())));
}

#[test]
fn the_surroundings_of_this_process_read_the_variable_and_the_system() {
    let here = Surroundings::of_process(true);
    assert_eq!(here.system, System::this());
    assert!(here.scripted);
    assert!(!Surroundings::of_process(false).scripted);
}

// ---- every failure goes through the one place ----

/// Runs `report` for `failure` in `surroundings` with a hook that records what it was asked to show, and standard error held in a buffer.
fn reported(failure: &Failure, surroundings: &Surroundings) -> (Vec<(String, String)>, String) {
    let shown = RefCell::new(Vec::new());
    let mut stderr = Vec::new();
    report(failure, surroundings, &|title, body| shown.borrow_mut().push((title.to_owned(), body.to_owned())), &mut stderr);
    (shown.into_inner(), String::from_utf8(stderr).unwrap())
}

#[test]
fn a_failed_window_start_asks_the_hook_to_show_the_box_and_still_writes_standard_error() {
    let f = failure(System::Windows, window(&["the usual graphics: no adapter"]));
    let (shown, stderr) = reported(&f, &surroundings(System::Windows));
    assert_eq!(shown, vec![(f.title().to_owned(), f.message())], "one box, with the text and the title of the failure");
    assert!(stderr.starts_with("diacraft: the window could not run: ") && stderr.contains("no adapter"), "{stderr}");
    assert!(stderr.contains("diacraft.log"), "the line says where the log is: {stderr}");
    assert!(stderr.ends_with('\n'));
}

#[test]
fn a_panic_and_a_bad_command_line_go_through_the_same_hook() {
    let panic = failure(System::Windows, Cause::Panic("boom".to_owned()));
    let (shown, stderr) = reported(&panic, &surroundings(System::Windows));
    assert_eq!(shown.len(), 1);
    assert!(shown[0].1.contains("boom") && stderr.contains("internal error") && stderr.contains("boom"), "{shown:?} {stderr}");

    let args = failure(System::Windows, Cause::Arguments("bad option --nope".to_owned()));
    let (shown, stderr) = reported(&args, &surroundings(System::Windows));
    assert_eq!(shown.len(), 1);
    assert_eq!(stderr, "diacraft: bad option --nope\n", "standard error is what it always was for a command line");
}

#[test]
fn no_box_means_the_hook_is_not_asked_and_standard_error_still_has_the_text() {
    let f = failure(System::Other, window(&["x"]));
    for blocked in [
        Surroundings { stderr_is_terminal: true, ..surroundings(System::Other) },
        Surroundings { scripted: true, ..surroundings(System::Other) },
        Surroundings { opted_out: true, ..surroundings(System::Other) },
        Surroundings { has_display: false, ..surroundings(System::Other) },
    ] {
        let (shown, stderr) = reported(&f, &blocked);
        assert!(shown.is_empty(), "{blocked:?}");
        assert!(stderr.contains("the window could not run"), "{stderr}");
    }
}

#[test]
fn the_failure_is_logged_once_with_its_reasons() {
    crate::capture_logs();
    let f = failure(System::Windows, window(&["the usual graphics: logged-by-report-test"]));
    reported(&f, &surroundings(System::Windows));
    let ours: Vec<String> = crate::logged().into_iter().filter(|l| l.contains("logged-by-report-test")).collect();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert!(ours[0].starts_with("ERROR ") && ours[0].contains("could not start"), "{ours:?}");
}

// ---- the real program ----

/// A start with no display at all cannot make a window on Linux: the event loop is refused. That is the early failure of a start with no graphics to show on, and it must end
/// with a failure code, the words in standard error and no box (the box is turned off here, so that a test never opens one).
#[cfg(target_os = "linux")]
#[test]
fn a_start_with_no_display_ends_with_a_failure_code_and_the_words_in_standard_error() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(env!("CARGO_BIN_EXE_diacraft"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env("DIACRAFT_NO_PREFS", "1")
        .env("DIACRAFT_NO_SPLASH", "1")
        .env(NO_BOX_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the app did not end in 60 s with no display");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = reader.join().unwrap();
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("diacraft: the window could not run: "), "{text}");
}

#[test]
fn the_empty_list_wgpu_ends_a_surface_error_with_is_left_out_of_the_box_and_kept_in_the_log() {
    let raw = "the usual graphics: WGPU error: Failed to create surface for any enabled backend: {}";
    let f = failure(System::Windows, window(&[raw]));
    let text = f.message();
    assert!(text.contains("Failed to create surface for any enabled backend"), "{text}");
    assert!(!text.contains("{}"), "{text}");
    assert!(text.contains("enabled backend\n"), "the colon that led to the empty list goes with it: {text}");
    assert!(f.one_line().ends_with("{}"), "the log has what wgpu said, whole: {}", f.one_line());
}

#[test]
fn a_box_that_panics_does_not_end_the_report() {
    // The system's dialog is code we do not own: if it panics, the exit code and standard error are still what the start ends with.
    let f = failure(System::Windows, window(&["a reason"]));
    let mut stderr = Vec::new();
    report(&f, &surroundings(System::Windows), &|_, _| panic!("the system's box panicked"), &mut stderr);
    assert!(String::from_utf8(stderr).unwrap().contains("a reason"));
}

// ---- the adapters left out because their test did not finish ----

#[test]
fn a_start_that_left_adapters_out_names_them_and_says_how_to_test_them_again() {
    let one = failure(System::Windows, window(&["no graphics adapter can show the window (4 listed; 1 failed on an earlier try or start)"]))
        .with_left_out(vec!["Intel(R) UHD Graphics (vulkan)".to_owned()]);
    let text = one.message();
    assert!(
        text.contains(
            "DiaCraft left out Intel(R) UHD Graphics (vulkan) because its test did not finish: a start ended in it, or it did not answer in time"
        ),
        "{text}"
    );
    assert!(text.contains("To test it again, start DiaCraft with  diacraft.exe --reset-graphics ."), "{text}");
    let many = failure(System::Other, window(&["x"])).with_left_out(vec!["A (dx12)".to_owned(), "B (vulkan)".to_owned()]);
    let text = many.message();
    assert!(text.contains("left out A (dx12); B (vulkan) because their tests did not finish") && text.contains("To test them again"), "{text}");
    assert!(text.contains("diacraft --reset-graphics") && !text.contains("diacraft.exe"), "{text}");
    // Before the advice, after the log: the person reads why, then what to do.
    assert!(
        text.find("The log of this start").unwrap() < text.find("left out A").unwrap()
            && text.find("left out A").unwrap() < text.find("What to try").unwrap(),
        "{text}"
    );
    // Nothing is said when nothing was left out, and not for a failure that is no window's.
    assert!(!failure(System::Windows, window(&["x"])).message().contains("left out"));
    assert!(!failure(System::Windows, Cause::Arguments("bad".to_owned())).with_left_out(vec!["A (dx12)".to_owned()]).message().contains("left out"));
}
