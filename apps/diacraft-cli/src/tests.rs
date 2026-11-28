//! The wiring of `mcp`'s three modes to the backends, from the command line to the bytes served, against a fake app on a
//! free port: the default address (127.0.0.1:7997) is a parameter here, so no test ever looks at, or drives, a real app.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diacraft_mcp::{Backend, ConnectError, DEFAULT_ADDR};
use serde_json::{Value, json};

use crate::args::{self, Action, Mode};
use crate::{mcp_on, open_backend};

/// A fake app on a free port that says it is `app` and records the methods it is asked, in order.
fn fake_app(app: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let addr = listener.local_addr().expect("its address").to_string();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let record = record.clone();
            std::thread::spawn(move || {
                let Ok(mut writer) = stream.try_clone() else { return };
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let Ok(request) = serde_json::from_str::<Value>(&line) else { return };
                    let method = request["method"].as_str().unwrap_or("").to_owned();
                    if let Ok(mut seen) = record.lock() {
                        seen.push(method);
                    }
                    let reply = json!({"id": request["id"], "ok": true, "result": {"app": app}});
                    if writeln!(writer, "{reply}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, seen)
}

fn seen(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().expect("the fake app's log").clone()
}

/// An address nothing listens at.
fn nothing_listens() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    listener.local_addr().expect("its address").to_string()
}

#[test]
fn the_default_mode_uses_the_app_when_diacraft_answers() {
    let (addr, log) = fake_app("diacraft");
    let (backend, why) = open_backend(Mode::Auto, &addr).expect("the default mode does not fail");
    assert!(matches!(backend, Backend::Remote(_)));
    assert_eq!(why, None);
    assert_eq!(seen(&log), ["ui.inspect"]);
}

#[test]
fn the_default_mode_runs_headless_and_says_why_when_another_app_answers() {
    let (addr, log) = fake_app("deckcraft");
    let (backend, why) = open_backend(Mode::Auto, &addr).expect("the default mode does not fail");
    assert!(matches!(backend, Backend::Headless(_)));
    assert_eq!(why, Some(ConnectError::NotDiacraft { app: "deckcraft".to_owned() }));
    assert_eq!(seen(&log), ["ui.inspect"], "the other app was asked who it is and nothing more");
}

#[test]
fn the_default_mode_runs_headless_when_nothing_answers() {
    let (backend, why) = open_backend(Mode::Auto, &nothing_listens()).expect("the default mode does not fail");
    assert!(matches!(backend, Backend::Headless(_)));
    assert!(matches!(why, Some(ConnectError::Refused(_))), "{why:?}");
}

#[test]
fn headless_mode_does_not_look_for_the_app() {
    let (addr, log) = fake_app("diacraft");
    // `Mode::Headless` carries no address, and the default one is a parameter: give it the fake's.
    let (backend, why) = open_backend(Mode::Headless, &addr).expect("headless does not fail");
    assert!(matches!(backend, Backend::Headless(_)));
    assert_eq!(why, None);
    // A connection would have asked who answers by now; wait a moment to be sure it did not.
    let end = Instant::now() + Duration::from_millis(300);
    while Instant::now() < end {
        assert!(seen(&log).is_empty(), "headless mode connected to an app");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn connect_mode_uses_the_address_given_and_refuses_another_app() {
    let (addr, _log) = fake_app("diacraft");
    let (backend, _) = open_backend(Mode::Connect(addr), &nothing_listens()).expect("DiaCraft answers at the address given");
    assert!(matches!(backend, Backend::Remote(_)), "the default address was used instead of the one given");

    let (other, log) = fake_app("deckcraft");
    let error = open_backend(Mode::Connect(other), &nothing_listens()).err().expect("another app is refused");
    assert!(error.contains("not DiaCraft") && error.contains("deckcraft"), "{error}");
    assert_eq!(seen(&log), ["ui.inspect"]);
    let error = open_backend(Mode::Connect(nothing_listens()), "127.0.0.1:1").err().expect("nothing there is refused");
    assert!(error.contains("cannot reach"), "{error}");
}

fn parse(args: &[&str]) -> Result<Action, String> {
    parse_fonts(args).map(|parsed| parsed.action)
}

fn parse_fonts(args: &[&str]) -> Result<args::Parsed, String> {
    args::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
}

#[test]
fn no_option_is_the_default_mode_and_each_option_picks_its_own() {
    // The default mode is what `diacraft-cli mcp` alone means: if this said headless, the app would never be looked for.
    assert_eq!(parse(&["mcp"]), Ok(Action::Mcp(Mode::Auto)));
    assert_eq!(parse(&["mcp", "--headless"]), Ok(Action::Mcp(Mode::Headless)));
    assert_eq!(parse(&["mcp", "--connect", "127.0.0.1:1234"]), Ok(Action::Mcp(Mode::Connect("127.0.0.1:1234".to_owned()))));
    assert_eq!(parse(&["mcp", "--connect=127.0.0.1:1234"]), Ok(Action::Mcp(Mode::Connect("127.0.0.1:1234".to_owned()))));
}

#[test]
fn the_default_address_is_the_documented_one() {
    // docs/mcp.md and docs/control-protocol.md: the app's control channel is on port 7997 of the loopback, and the other
    // craft apps' is on 7979, which is a different program.
    assert_eq!(DEFAULT_ADDR, "127.0.0.1:7997");
}

/// One `command_run file.new` as a client sends it.
const FILE_NEW: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"command_run\",\"arguments\":{\"id\":\"file.new\"}}}\n";

fn served(mode: Mode, default_addr: &str) -> (bool, String) {
    served_in(mode, default_addr, args::FontChoice::Bundled)
}

/// As `served`, with the faces the command line asked for. (The tests that do not look at the faces use the bundled ones, which reads nothing
/// from the machine.)
fn served_in(mode: Mode, default_addr: &str, fonts: args::FontChoice) -> (bool, String) {
    let mut output = Vec::new();
    let ended = mcp_on(mode, default_addr, fonts, FILE_NEW, &mut output).is_ok();
    (ended, String::from_utf8(output).expect("the server writes UTF-8"))
}

#[test]
fn the_default_path_serves_through_the_app_when_diacraft_answers_at_the_default_address() {
    let (addr, log) = fake_app("diacraft");
    let (ended, output) = served(Mode::Auto, &addr);
    assert!(ended);
    // The command went to the app: the fake's answer is what the client got.
    assert!(output.contains("{\\\"app\\\":\\\"diacraft\\\"}"), "{output}");
    assert_eq!(seen(&log), ["ui.inspect", "engine.execute"]);
}

#[test]
fn the_default_path_serves_headless_and_never_drives_another_app() {
    let (addr, log) = fake_app("deckcraft");
    let (ended, output) = served(Mode::Auto, &addr);
    assert!(ended);
    // The command ran in this process's own session: its answer is a new document, not the fake's.
    assert!(output.contains("Untitled") && !output.contains("deckcraft"), "{output}");
    assert_eq!(seen(&log), ["ui.inspect"], "only the question who it is reached the other app");
}

#[test]
fn headless_and_connect_paths_serve_their_own_backend() {
    let (addr, log) = fake_app("diacraft");
    let (ended, output) = served(Mode::Headless, &addr);
    assert!(ended && output.contains("Untitled"), "{output}");
    let (ended, output) = served(Mode::Connect(addr.clone()), &nothing_listens());
    assert!(ended && output.contains("diacraft"), "{output}");
    assert_eq!(seen(&log), ["ui.inspect", "engine.execute"], "headless mode never looked, --connect did, at the address it was given");
}

#[test]
fn oracle_takes_files_and_its_own_options_and_nothing_else() {
    use args::{DEFAULT_CAP, OracleArgs};
    let files = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        parse(&["oracle", "a.vsdx", "b.vsdx", "--json", "--source", "o/r"]),
        Ok(Action::Oracle(OracleArgs {
            files: files(&["a.vsdx", "b.vsdx"]),
            json: true,
            source: Some("o/r".into()),
            aggregate: false,
            cap: DEFAULT_CAP
        }))
    );
    assert_eq!(
        parse(&["oracle", "--aggregate", "r.jsonl", "--cap=3"]),
        Ok(Action::Oracle(OracleArgs { files: files(&["r.jsonl"]), json: false, source: None, aggregate: true, cap: 3 }))
    );
    // After `--` everything is a file, even a name that looks like an option.
    assert_eq!(
        parse(&["oracle", "--", "--json"]),
        Ok(Action::Oracle(OracleArgs { files: files(&["--json"]), json: false, source: None, aggregate: false, cap: DEFAULT_CAP }))
    );
    for bad in [
        &["oracle"][..],
        &["oracle", "--json"],
        &["oracle", "a", "--json", "--json"],
        &["oracle", "a", "--source", "x", "--source=y"],
        &["oracle", "a", "--source"],
        &["oracle", "a", "--cap", "3"],
        &["oracle", "--aggregate", "a", "--cap", "0"],
        &["oracle", "--aggregate", "a", "--cap", "-1"],
        &["oracle", "--aggregate", "a", "--cap", "+2"],
        &["oracle", "--aggregate", "a", "--json"],
        &["oracle", "--aggregate", "a", "--source", "x"],
        &["oracle", "a", "--unknown"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn export_takes_a_drawing_an_output_and_the_options_that_suit_the_format() {
    use args::{ExportArgs, Format};
    let make = |file: &str, page: Option<usize>, out: &str, format: Format| ExportArgs {
        file: file.to_owned(),
        page,
        out: out.to_owned(),
        format,
        dpi: None,
        outlines: false,
        transparent: false,
    };
    assert_eq!(parse(&["export", "d.vsdx", "--out", "p.svg"]), Ok(Action::Export(make("d.vsdx", None, "p.svg", Format::Svg))));
    assert_eq!(parse(&["export", "--page", "3", "d.vsdx", "--out=p.PDF"]), Ok(Action::Export(make("d.vsdx", Some(3), "p.PDF", Format::Pdf))));
    assert_eq!(
        parse(&["export", "d.vsdx", "--out", "p.png", "--dpi", "150.5", "--transparent"]),
        Ok(Action::Export(ExportArgs { dpi: Some(150.5), transparent: true, ..make("d.vsdx", None, "p.png", Format::Png) }))
    );
    assert_eq!(
        parse(&["export", "d.vsdx", "--out", "p.svg", "--text", "outlines"]),
        Ok(Action::Export(ExportArgs { outlines: true, ..make("d.vsdx", None, "p.svg", Format::Svg) }))
    );
    for bad in [
        &["export"][..],
        &["export", "d.vsdx"],
        &["export", "--out", "p.svg"],
        &["export", "d.vsdx", "e.vsdx", "--out", "p.svg"],
        &["export", "d.vsdx", "--out", "p.gif"],
        &["export", "d.vsdx", "--out", "p"],
        &["export", "d.vsdx", "--out", "p.svg", "--out", "q.svg"],
        &["export", "d.vsdx", "--out", "p.svg", "--page", "0"],
        &["export", "d.vsdx", "--out", "p.svg", "--page", "-1"],
        &["export", "d.vsdx", "--out", "p.svg", "--page", "+1"],
        &["export", "d.vsdx", "--out", "p.svg", "--page", "1", "--page", "2"],
        &["export", "d.vsdx", "--out", "p.png", "--dpi", "0"],
        &["export", "d.vsdx", "--out", "p.png", "--dpi", "NaN"],
        &["export", "d.vsdx", "--out", "p.png", "--dpi", "inf"],
        &["export", "d.vsdx", "--out", "p.png", "--dpi", "2401"],
        &["export", "d.vsdx", "--out", "p.svg", "--dpi", "96"],
        &["export", "d.vsdx", "--out", "p.png", "--text", "text"],
        &["export", "d.vsdx", "--out", "p.svg", "--text", "paths"],
        &["export", "d.vsdx", "--out", "p.svg", "--transparent", "--transparent"],
        &["export", "d.vsdx", "--out", "p.svg", "--unknown"],
        &["export", "d.vsdx", "--out"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
}

/// `--bundled-fonts` belongs to the commands that set text, wherever among their options it stands, once; the others do not have it.
#[test]
fn bundled_fonts_is_an_option_of_the_commands_that_set_text_and_of_no_other() {
    use args::FontChoice::{Bundled, System};

    for (line, action) in [
        (&["mcp", "--bundled-fonts", "--headless"][..], Action::Mcp(Mode::Headless)),
        (&["mcp", "--headless", "--bundled-fonts"][..], Action::Mcp(Mode::Headless)),
    ] {
        assert_eq!(parse_fonts(line), Ok(args::Parsed { action, fonts: Bundled, roots: Default::default() }), "{line:?}");
    }
    assert_eq!(parse_fonts(&["mcp", "--headless"]).map(|p| p.fonts), Ok(System), "the machine's faces are the default");
    let run = parse_fonts(&["run", "--cmd", "file.new", "--bundled-fonts"]).unwrap();
    assert_eq!((matches!(run.action, Action::Run(_)), run.fonts), (true, Bundled));
    let export = parse_fonts(&["export", "d.vsdx", "--bundled-fonts", "--out", "p.svg"]).unwrap();
    assert_eq!((matches!(export.action, Action::Export(_)), export.fonts), (true, Bundled));
    let roundtrip = parse_fonts(&["roundtrip", "d.vsdx", "--out", "e.vsdx", "--bundled-fonts"]).unwrap();
    assert_eq!((matches!(roundtrip.action, Action::Roundtrip(_)), roundtrip.fonts), (true, Bundled));
    // A value that spells the option is a value: it is `--out`'s, not the option.
    assert!(parse_fonts(&["export", "d.vsdx", "--out", "--bundled-fonts"]).is_err(), "--out takes the next word, which is no format");
    for bad in [
        &["mcp", "--bundled-fonts", "--bundled-fonts"][..],
        &["run", "--cmd", "file.new", "--bundled-fonts", "--bundled-fonts"],
        &["export", "d.vsdx", "--out", "p.svg", "--bundled-fonts", "--bundled-fonts"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--bundled-fonts", "--bundled-fonts"],
        &["commands", "--bundled-fonts"],
        &["oracle", "a.vsdx", "--bundled-fonts"],
        // A session in the running app has the app's faces: the option would do nothing, and nothing is ignored.
        &["mcp", "--connect", "127.0.0.1:1234", "--bundled-fonts"],
        &["mcp", "--bundled-fonts", "--connect=127.0.0.1:1234"],
    ] {
        assert!(parse_fonts(bad).is_err(), "{bad:?} should be refused");
    }
    // `--version` still wins over everything, a mistake included.
    assert_eq!(parse(&["--bundled-fonts", "--bundled-fonts", "--version"]), Ok(Action::Version));
}

/// The faces are read once, at the start and before any command, for the commands that set text, and the engine's commits and exports share
/// them: `--bundled-fonts` reads nothing from the machine, and `commands` and `oracle` set nothing.
#[test]
fn the_start_shares_the_faces_with_the_engine_for_the_commands_that_set_text() {
    use args::FontChoice::{Bundled, System};
    use diacraft_fonts::FontDb;

    crate::measure::set_measure_fonts(None);
    crate::start_fonts(&Action::Commands, System);
    assert!(crate::measure::measure_fonts().is_none(), "`commands` sets no text");
    crate::start_fonts(&Action::Run(Vec::new()), Bundled);
    let bundled = crate::measure::measure_fonts().expect("the engine has the faces");
    assert_eq!(bundled.face_count(), FontDb::bundled().face_count(), "nothing was read from the machine");
    assert!(Arc::ptr_eq(&crate::measure::renderer_fonts(), &bundled), "an export draws in the faces the commits measure in");
    crate::start_fonts(
        &Action::Export(args::ExportArgs {
            file: "d.vsdx".to_owned(),
            page: None,
            out: "p.svg".to_owned(),
            format: args::Format::Svg,
            dpi: None,
            outlines: false,
            transparent: false,
        }),
        System,
    );
    let system = crate::measure::measure_fonts().expect("the engine has the faces");
    assert!(system.face_count() >= bundled.face_count() && !Arc::ptr_eq(&system, &bundled));
    assert!(Arc::ptr_eq(&crate::measure::renderer_fonts(), &system));
    // `mcp` is not among them: it knows only once it has a backend whether the session is its own (see below).
    crate::measure::set_measure_fonts(None);
    crate::start_fonts(&Action::Mcp(Mode::Headless), System);
    assert!(crate::measure::measure_fonts().is_none(), "`mcp` reads the faces when it has a session of its own, and not before");
}

/// `mcp` reads the machine's faces only for a session of its own: not for `--connect`, where the session is the running app's and has the
/// app's faces, and in the default mode not before the headless session is chosen, so it does not scan for a run that goes through the app.
#[test]
fn mcp_reads_the_faces_only_for_a_session_of_its_own() {
    use args::FontChoice::{Bundled, System};

    // Through the app (the default address answers, and it is DiaCraft): nothing is read, whatever the faces asked for.
    crate::measure::set_measure_fonts(None);
    let (addr, _) = fake_app("diacraft");
    let (ended, _) = served_in(Mode::Auto, &addr, System);
    assert!(ended && crate::measure::measure_fonts().is_none(), "the app's session has the app's faces");
    let (addr, _) = fake_app("diacraft");
    let (ended, _) = served_in(Mode::Connect(addr), &nothing_listens(), System);
    assert!(ended && crate::measure::measure_fonts().is_none(), "`--connect` reads nothing");
    // A session of its own: a headless one, or the default mode that fell back to one.
    let (ended, _) = served_in(Mode::Headless, &nothing_listens(), Bundled);
    let bundled = crate::measure::measure_fonts().expect("the faces are read for a session of its own");
    assert!(ended && bundled.face_count() == diacraft_fonts::FontDb::bundled().face_count(), "`--bundled-fonts` reads nothing from the machine");
    crate::measure::set_measure_fonts(None);
    let (addr, _) = fake_app("deckcraft");
    let (ended, _) = served_in(Mode::Auto, &addr, System);
    assert!(ended && crate::measure::measure_fonts().is_some(), "the default mode that chose a headless session reads the machine's faces");
    crate::measure::set_measure_fonts(None);
}

/// `roundtrip --via diacraft` and `--edit KIND`: the one format a file can visit, the five salted edits of plan ruling NP13, and the mistakes that are refused.
#[test]
fn roundtrip_takes_via_and_the_salted_edits_and_nothing_else() {
    use args::{EditKind, RoundtripArgs, Via};
    let make = |via: Option<Via>, edit: Option<EditKind>, shift: Option<(f64, f64)>| RoundtripArgs {
        file: "d.vsdx".to_owned(),
        out: "e.vsdx".to_owned(),
        shift,
        via,
        edit,
    };
    assert_eq!(parse(&["roundtrip", "d.vsdx", "--out", "e.vsdx"]), Ok(Action::Roundtrip(make(None, None, None))), "the options are all optional");
    assert_eq!(parse(&["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft"]), Ok(Action::Roundtrip(make(Some(Via::Diacraft), None, None))));
    assert_eq!(
        parse(&["roundtrip", "--via=diacraft", "d.vsdx", "--out=e.vsdx"]),
        Ok(Action::Roundtrip(make(Some(Via::Diacraft), None, None))),
        "the option is also written with `=`, wherever it stands"
    );
    for (word, kind) in
        [("cell", EditKind::Cell), ("move", EditKind::Move), ("retext", EditKind::Retext), ("delete", EditKind::Delete), ("add", EditKind::Add)]
    {
        assert_eq!(
            parse(&["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit", word]),
            Ok(Action::Roundtrip(make(Some(Via::Diacraft), Some(kind), None))),
            "{word}"
        );
        assert_eq!(kind.word(), word, "the word the report says is the word the command line takes");
        assert_eq!(EditKind::of(word), Some(kind));
    }
    assert_eq!(EditKind::ALL.len(), 5);
    // `--move` is the move edit with a delta of the caller's own, and is taken with `--via` as it is without.
    assert_eq!(
        parse(&["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--move", "1,2"]),
        Ok(Action::Roundtrip(make(Some(Via::Diacraft), None, Some((1.0, 2.0)))))
    );
    for bad in [
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "vsdx"][..],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "Diacraft"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--via", "diacraft"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit", "rotate"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit", "Cell"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit"],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit", "cell", "--edit", "move"],
        // A salted edit is made before the trip: without one, `--edit` would be ignored, and nothing is ignored.
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--edit", "cell"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
}

/// `--edit` and `--move` are both edits of the first shape, and a line that gives both is refused in words that name the way out.
#[test]
fn edit_with_move_is_refused() {
    for line in [
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--edit", "move", "--move", "1,2"][..],
        &["roundtrip", "d.vsdx", "--out", "e.vsdx", "--via", "diacraft", "--move", "1,2", "--edit", "cell"],
    ] {
        let error = parse(line).expect_err("both edits were asked for");
        assert!(error.contains("--edit and --move") && error.contains("give one"), "{error}");
    }
}

/// `compare --content`: the content check on two files, given once; without it `compare` is what it was.
#[test]
fn compare_takes_content_once_and_two_files() {
    use args::CompareArgs;
    let make = |content: bool| CompareArgs { first: "a.vsdx".to_owned(), second: "b.vsdx".to_owned(), content };
    assert_eq!(parse(&["compare", "a.vsdx", "b.vsdx"]), Ok(Action::Compare(make(false))));
    assert_eq!(parse(&["compare", "--content", "a.vsdx", "b.vsdx"]), Ok(Action::Compare(make(true))));
    assert_eq!(parse(&["compare", "a.vsdx", "b.vsdx", "--content"]), Ok(Action::Compare(make(true))), "wherever it stands");
    for bad in [
        &["compare", "--content", "--content", "a.vsdx", "b.vsdx"][..],
        &["compare", "--content", "a.vsdx"],
        &["compare", "--content"],
        &["compare", "--content", "a.vsdx", "b.vsdx", "c.vsdx"],
        &["compare", "--contents", "a.vsdx", "b.vsdx"],
        &["compare", "--content=1", "a.vsdx", "b.vsdx"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
}

/// The help of `convert` lists every type it writes, the six VSDX types under their product's name (owner, 2026-11-23: the file-type labels and the help
/// where it names the types), and says each type once.
#[test]
fn the_convert_help_names_every_type_it_writes_and_the_vsdx_types_by_their_product() {
    let help: String = crate::help::plain().split_whitespace().collect::<Vec<_>>().join(" ");
    let list = help.split("The types written are ").nth(1).and_then(|rest| rest.split(". ").next()).unwrap_or_default();
    assert!(list.contains("the Visio types .vsdx, .vsdm, .vssx, .vssm, .vstx and .vstm"), "{list}");
    for extension in crate::convert::drawing_types().iter().chain(&["png", "svg", "pdf"]) {
        let dotted = format!(".{extension}");
        let named = list
            .match_indices(&dotted)
            .filter(|(at, _)| !list[at + dotted.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-'))
            .count();
        assert_eq!(named, 1, "{dotted} in: {list}");
    }
}

/// `convert` takes a drawing and an output whose extension says what to write: a drawing type (any case) or a picture type, and the options of a picture only for a picture.
#[test]
fn convert_takes_a_drawing_and_an_output_whose_extension_says_the_type_and_the_options_that_suit_it() {
    use args::{ConvertArgs, ConvertTo, ExportArgs, Format};
    let drawing = |out: &str, extension: &'static str| {
        Action::Convert(ConvertArgs {
            file: "d.vsdx".to_owned(),
            out: out.to_owned(),
            to: ConvertTo::Drawing(extension),
            drop_macros: false,
            drop_objects: false,
        })
    };
    assert_eq!(parse(&["convert", "d.vsdx", "--out", "a.diacraft"]), Ok(drawing("a.diacraft", "diacraft")));
    assert_eq!(parse(&["convert", "--out=B.VSDM", "d.vsdx"]), Ok(drawing("B.VSDM", "vsdm")));
    for extension in crate::convert::drawing_types() {
        let out = format!("x.{}", extension.to_ascii_uppercase());
        assert_eq!(parse(&["convert", "d.vsdx", "--out", &out]), Ok(drawing(&out, extension)), "{extension}");
    }
    let picture = |out: &str, format: Format| ExportArgs {
        file: "d.vsdx".to_owned(),
        page: None,
        out: out.to_owned(),
        format,
        dpi: None,
        outlines: false,
        transparent: false,
    };
    assert_eq!(
        parse(&["convert", "d.vsdx", "--out", "p.svg"]),
        Ok(Action::Convert(ConvertArgs {
            file: "d.vsdx".to_owned(),
            out: "p.svg".to_owned(),
            to: ConvertTo::Picture(picture("p.svg", Format::Svg)),
            drop_macros: false,
            drop_objects: false,
        }))
    );
    assert_eq!(
        parse(&["convert", "d.vsdx", "--out", "p.PNG", "--page", "3", "--dpi", "150.5", "--transparent"]),
        Ok(Action::Convert(ConvertArgs {
            file: "d.vsdx".to_owned(),
            out: "p.PNG".to_owned(),
            to: ConvertTo::Picture(ExportArgs { page: Some(3), dpi: Some(150.5), transparent: true, ..picture("p.PNG", Format::Png) }),
            drop_macros: false,
            drop_objects: false,
        }))
    );
    let fonts = parse_fonts(&["convert", "d.vsdx", "--out", "p.pdf", "--text", "outlines", "--bundled-fonts"]).unwrap();
    assert_eq!(fonts.fonts, args::FontChoice::Bundled);
    assert!(matches!(fonts.action, Action::Convert(ConvertArgs { to: ConvertTo::Picture(ExportArgs { outlines: true, .. }), .. })));
    // The options of a save across: for a drawing type only, each once.
    assert_eq!(
        parse(&["convert", "d.vsdx", "--out", "a.odg", "--drop-macros", "--drop-objects"]),
        Ok(Action::Convert(ConvertArgs {
            file: "d.vsdx".to_owned(),
            out: "a.odg".to_owned(),
            to: ConvertTo::Drawing("odg"),
            drop_macros: true,
            drop_objects: true
        }))
    );
    assert_eq!(
        parse(&["convert", "d.odg", "--drop-objects", "--out", "a.vsdm"]),
        Ok(Action::Convert(ConvertArgs {
            file: "d.odg".to_owned(),
            out: "a.vsdm".to_owned(),
            to: ConvertTo::Drawing("vsdm"),
            drop_macros: false,
            drop_objects: true
        }))
    );
    for bad in [
        &["convert", "d.vsdx", "--out", "a.odg", "--drop-macros", "--drop-macros"][..],
        &["convert", "d.vsdx", "--out", "a.odg", "--drop-objects", "--drop-objects"],
        &["convert", "d.vsdx", "--out", "p.png", "--drop-macros"],
        &["convert", "d.vsdx", "--out", "p.svg", "--drop-objects"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
    for bad in [
        &["convert"][..],
        &["convert", "d.vsdx"],
        &["convert", "--out", "a.vsdx"],
        &["convert", "d.vsdx", "e.vsdx", "--out", "a.vsdx"],
        &["convert", "d.vsdx", "--out"],
        &["convert", "d.vsdx", "--out", "a.vsdx", "--out", "b.vsdx"],
        &["convert", "d.vsdx", "--out", "a.gif"],
        &["convert", "d.vsdx", "--out", "a"],
        // Written by no one: not a type of the engine's (a flat drawing is opened, and saved by Save alone).
        &["convert", "d.vsdx", "--out", "a.fodg"],
        &["convert", "d.vsdx", "--out", "a.fotg"],
        &["convert", "d.vsdx", "--out", "a.vsd"],
        // The options of a picture are the picture's.
        &["convert", "d.vsdx", "--out", "a.vsdx", "--page", "1"],
        &["convert", "d.vsdx", "--out", "a.diacraft", "--dpi", "96"],
        &["convert", "d.vsdx", "--out", "a.odg", "--text", "outlines"],
        &["convert", "d.vsdx", "--out", "a.vssx", "--transparent"],
        &["convert", "d.vsdx", "--out", "a.vstx", "--bundled-fonts"],
        &["convert", "d.vsdx", "--out", "a.svg", "--dpi", "96"],
        &["convert", "d.vsdx", "--out", "a.png", "--text", "text"],
        &["convert", "d.vsdx", "--out", "a.png", "--page", "0"],
        &["convert", "d.vsdx", "--out", "a.png", "--dpi", "2401"],
        &["convert", "d.vsdx", "--out", "a.png", "--bundled-fonts", "--bundled-fonts"],
        &["convert", "d.vsdx", "--out", "a.vsdx", "--unknown"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
    // A drawing type reads no faces: the machine's are the default, and the option is refused above.
    assert_eq!(parse_fonts(&["convert", "d.vsdx", "--out", "a.vsdx"]).unwrap().fonts, args::FontChoice::System);
}

/// The types `convert` writes as drawings are the engine's: the types of the three kinds of `.diacraft` file, the six of the VSDX package, and the OpenDocument drawing and template (the engine's own
/// list, `diacraft_engine::odgfile::SAVE_TYPES`), in the order the help lists them.
#[test]
fn the_drawing_types_are_the_engines_own_names_in_the_order_the_help_lists_them() {
    assert_eq!(
        crate::convert::drawing_types(),
        ["diacraft", "diacraft-stencil", "diacraft-template", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg"]
    );
    // A type is found by its extension in any case, and an extension that is a prefix of one, or holds one, is none.
    for (path, found) in [
        ("a.VSDX", Some("vsdx")),
        ("dir.d/a.Diacraft-Stencil", Some("diacraft-stencil")),
        ("a.OTG", Some("otg")),
        ("a.png", None),
        ("a.vsdxx", None),
        (".vsdx", None),
        ("a.", None),
        ("a", None),
    ] {
        assert_eq!(crate::convert::drawing_type(path), found, "{path}");
    }
}

// ---- the automation roots (gaps A2 and A5) ------------------------------------------------------------------------------------------------------

fn roots_of(args: &[&str]) -> Result<(Vec<String>, Vec<String>), String> {
    let parsed = parse_fonts(args)?;
    let words = |list: &[std::path::PathBuf]| list.iter().map(|p| p.to_str().expect("text").to_owned()).collect::<Vec<_>>();
    Ok((words(parsed.roots.read()), words(parsed.roots.write())))
}

#[test]
fn mcp_takes_root_flags_in_either_spelling_and_repeated() {
    let (read, write) =
        roots_of(&["mcp", "--headless", "--automation-read-root", "/a", "--automation-read-root=/b", "--automation-write-root", "/w"])
            .expect("the flags are mcp's");
    assert_eq!((read, write), (vec!["/a".to_owned(), "/b".to_owned()], vec!["/w".to_owned()]));
    // The default mode takes them too (and stops at run time if the app answers, below); no flag is no root.
    assert!(parse(&["mcp", "--automation-write-root", "/w"]).is_ok());
    assert_eq!(roots_of(&["mcp", "--headless"]).expect("no flag"), (vec![], vec![]));
    for (args, words) in [(&["mcp", "--automation-read-root"][..], "needs a value"), (&["mcp", "--automation-write-root="][..], "empty")] {
        let error = parse(args).expect_err("a flag with no folder");
        assert!(error.contains(words), "{args:?}: {error}");
    }
}

#[test]
fn the_root_flags_are_mcps_alone_and_cannot_keep_the_running_app() {
    for command in ["run", "export", "convert", "roundtrip", "oracle", "compare", "commands"] {
        let error = parse(&[command, "--automation-read-root", "/a"]).expect_err("only mcp has roots");
        assert!(!error.is_empty(), "{command}: {error}");
    }
    // `--connect` names the app, which keeps to the roots it was started with: the flags would ask this process for what it cannot do.
    let error = parse(&["mcp", "--connect", "127.0.0.1:1", "--automation-write-root", "/w"]).expect_err("not with --connect");
    assert!(error.contains("--connect") && error.contains("--control") && error.contains("--headless"), "{error}");
}

/// The default mode with roots asked for, and the app answering: the server stops and says what to give, and has driven nothing in the app but the question who it is.
#[test]
fn the_default_mode_with_roots_does_not_go_on_in_an_app_it_cannot_keep() {
    let dir = std::env::temp_dir().join(format!("diacraft-cli-roots-unit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let roots = diacraft_engine::file_access::AutomationRoots::new(&[dir.as_path()], &[dir.as_path()]).expect("roots").expect("some");
    let (addr, log) = fake_app("diacraft");
    let mut output = Vec::new();
    let ended = crate::mcp_with_roots(Mode::Auto, &addr, args::FontChoice::Bundled, Some(roots.clone()), FILE_NEW, &mut output);
    let Err(crate::Failure::Message(words)) = ended else { panic!("the server went on in the app") };
    assert!(words.contains("--headless") && words.contains("--automation-read-root"), "{words}");
    assert!(output.is_empty(), "nothing was served");
    assert_eq!(seen(&log), ["ui.inspect"], "only the question who it is reached the app");
    // With no app answering, the same roots serve a session of this process, and a file outside is refused in words.
    let outside = std::env::temp_dir().join(format!("diacraft-cli-roots-outside-{}.diacraft", std::process::id()));
    let ask = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "command_run", "arguments": {"id": "file.new"}}})
    );
    let save = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "command_run", "arguments": {"id": "file.save_as", "params": {"path": outside.to_str().expect("text")}}}})
    );
    let mut output = Vec::new();
    let ended =
        crate::mcp_with_roots(Mode::Auto, &nothing_listens(), args::FontChoice::Bundled, Some(roots), format!("{ask}{save}").as_bytes(), &mut output);
    assert!(ended.is_ok());
    let replies = String::from_utf8(output).expect("UTF-8");
    assert!(replies.contains("automation path rejected"), "{replies}");
    assert!(!outside.exists(), "nothing was written outside the roots");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `completions` takes one shell, in lower case, and the program the script is for; `man` a page, or the list of them.
#[test]
fn completions_and_man_take_what_they_name_and_nothing_else() {
    use args::{CompletionsArgs, ManArgs, Program};
    use clap_complete::Shell;
    for (word, shell) in
        [("bash", Shell::Bash), ("zsh", Shell::Zsh), ("fish", Shell::Fish), ("powershell", Shell::PowerShell), ("elvish", Shell::Elvish)]
    {
        assert_eq!(parse(&["completions", word]), Ok(Action::Completions(CompletionsArgs { shell, program: Program::Cli })), "{word}");
    }
    assert_eq!(
        parse(&["completions", "--program", "diacraft", "bash"]),
        Ok(Action::Completions(CompletionsArgs { shell: Shell::Bash, program: Program::App })),
        "the option stands where it likes"
    );
    assert_eq!(
        parse(&["completions", "zsh", "--program=diacraft-cli"]),
        Ok(Action::Completions(CompletionsArgs { shell: Shell::Zsh, program: Program::Cli }))
    );
    for bad in [
        &["completions"][..],
        &["completions", "tcsh"],
        &["completions", "Bash"],
        &["completions", "bash", "zsh"],
        &["completions", "bash", "--program", "other"],
        &["completions", "bash", "--program"],
        &["completions", "bash", "--program", "diacraft", "--program", "diacraft"],
        &["completions", "bash", "--bogus"],
        &["completions", "bash", "--bundled-fonts"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
    assert!(parse(&["completions"]).unwrap_err().contains("bash, zsh, fish, powershell or elvish"));
    assert_eq!(parse(&["man"]), Ok(Action::Man(ManArgs::Page("diacraft-cli".to_owned()))));
    assert_eq!(parse(&["man", "diacraft-cli-run"]), Ok(Action::Man(ManArgs::Page("diacraft-cli-run".to_owned()))));
    assert_eq!(parse(&["man", "diacraft"]), Ok(Action::Man(ManArgs::Page("diacraft".to_owned()))));
    assert_eq!(parse(&["man", "--list"]), Ok(Action::Man(ManArgs::List)));
    for bad in [&["man", "run"][..], &["man", "a", "b"], &["man", "--list", "diacraft"], &["man", "--list", "--list"], &["man", "--bogus"]] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
}

/// `--color` is an option of every command, given once, before or after the command, and `--version` and `--help` still win over a mistake in it.
#[test]
fn color_is_taken_once_and_anywhere_and_never_stops_help_or_version() {
    assert_eq!(parse(&["--color", "always", "commands"]), Ok(Action::Commands));
    assert_eq!(parse(&["commands", "--color=never"]), Ok(Action::Commands));
    assert_eq!(
        parse(&["--color=auto", "export", "d.vsdx", "--color", "never", "--out", "p.svg"]).map(|a| matches!(a, Action::Export(_))),
        Err("--color was given twice".to_owned())
    );
    for bad in [
        &["--color", "red", "commands"][..],
        &["--color", "Always", "commands"],
        &["--color", "always", "--color", "never", "commands"],
        &["commands", "--color", "always", "--color=never"],
        &["--color", "always", "commands", "--color", "never"],
        &["--color"],
        &["commands", "--color"],
        &["--color", "always"],
        &["--color=always"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should be refused");
    }
    assert!(parse(&["--color", "red", "commands"]).unwrap_err().contains("--color must be `auto`, `always` or `never`"));
    assert_eq!(parse(&["--color", "red", "--help"]), Ok(Action::Help));
    assert_eq!(parse(&["--color", "--version"]), Ok(Action::Version));
    assert_eq!(parse(&["commands", "--color", "red", "-h"]), Ok(Action::Help));
}

/// What `--color` says is read from the line alone, loosely: the page and the messages are coloured as asked even when the rest of the line is a mistake.
#[test]
fn the_color_a_line_asks_for_is_read_loosely() {
    use crate::term::When;
    let scan = |line: &[&str]| args::scan_color(&line.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());
    assert_eq!(scan(&[]), When::Auto);
    assert_eq!(scan(&["commands"]), When::Auto);
    assert_eq!(scan(&["--color", "always"]), When::Always);
    assert_eq!(scan(&["--color=never", "--bogus"]), When::Never);
    assert_eq!(scan(&["export", "--bogus", "--color", "always", "--help"]), When::Always);
    assert_eq!(scan(&["--color", "red"]), When::Auto, "a word that is no choice asks for nothing");
    assert_eq!(scan(&["--color"]), When::Auto);
    assert_eq!(scan(&["--color", "always", "--color", "red"]), When::Always, "the last choice that is one");
    assert_eq!(scan(&["--colour", "always"]), When::Auto);
}

/// Clap reads the words as a person has always typed them, with the one difference that `--` ends the options for every command (a file that begins with a dash is named
/// after it), and that a word with a single dash is an option, not a file.
#[test]
fn two_dashes_end_the_options_and_one_dash_is_an_option() {
    use args::{ExportArgs, Format};
    let export = |file: &str| {
        Action::Export(ExportArgs {
            file: file.to_owned(),
            page: None,
            out: "p.svg".to_owned(),
            format: Format::Svg,
            dpi: None,
            outlines: false,
            transparent: false,
        })
    };
    assert_eq!(parse(&["export", "--out", "p.svg", "--", "-odd.vsdx"]), Ok(export("-odd.vsdx")));
    assert_eq!(parse(&["export", "--out", "p.svg", "--", "--out"]), Ok(export("--out")), "after the two dashes nothing is an option");
    assert!(parse(&["export", "-odd.vsdx", "--out", "p.svg"]).unwrap_err().contains("unknown option `-odd.vsdx` for `export`; see --help"));
    assert!(
        parse(&["export", "d.vsdx", "--out", "p.svg", "--", "e.vsdx"]).unwrap_err().contains("takes one drawing"),
        "a second file after the two dashes is still a second file"
    );
    // A lone dash is a word.
    assert_eq!(parse(&["export", "--out", "p.svg", "-"]), Ok(export("-")));
    // Help and version are looked for before the two dashes only: after them they are the names of files.
    for name in ["--help", "--version", "-h", "-V"] {
        assert_eq!(parse(&["export", "--out", "p.svg", "--", name]), Ok(export(name)), "`{name}` after the two dashes is a file");
    }
    assert_eq!(parse(&["export", "--help", "--out", "p.svg", "--", "--version"]), Ok(Action::Help), "before the two dashes help still wins");
    assert_eq!(parse(&["export", "--version", "--", "--help"]), Ok(Action::Version), "and so does version, over a help after them");
    assert!(
        parse(&["export", "--out", "p.svg", "--", "--help", "--version"]).unwrap_err().contains("takes one drawing"),
        "two such words after the two dashes are two files, and no help"
    );
}

/// What clap found wrong is said in the words the command line has always used, for each kind of mistake.
#[test]
fn the_words_of_a_mistake_are_the_ones_the_docs_give() {
    let said = |line: &[&str]| parse(line).unwrap_err();
    assert_eq!(said(&["export", "d.vsdx", "--bogus"]), "unknown option `--bogus` for `export`; see --help");
    assert_eq!(said(&["mcp", "extra"]), "unknown option `extra` for `mcp`; see --help");
    assert_eq!(said(&["export", "d.vsdx", "--out", "a", "--out", "b"]), "--out was given twice");
    assert_eq!(said(&["export", "d.vsdx", "--transparent", "--transparent"]), "--transparent was given twice");
    assert_eq!(said(&["mcp", "--bundled-fonts", "--bundled-fonts"]), "--bundled-fonts was given twice");
    assert_eq!(said(&["export", "d.vsdx", "--out"]), "--out needs a value");
    assert_eq!(said(&["run", "--cmd"]), "--cmd needs a value");
    assert_eq!(said(&["commands", "extra"]), "`commands` takes no arguments; unexpected `extra`");
    assert_eq!(said(&["compare", "--content=1", "a", "b"]), "unknown option `--content=1` for `compare`; see --help");
    assert_eq!(said(&["bogus"]), "unknown command `bogus`; see --help");
    assert_eq!(said(&["--bogus"]), "unknown command `--bogus`; see --help");
    assert!(said(&[]).starts_with("no command given\n\nUsage: diacraft-cli <command> [options]\n"));
    // An argument that holds a terminal escape is never echoed as one.
    for line in [&["export", "\u{1b}[2J", "--\u{1b}[31mx"][..], &["\u{1b}[2J"], &["commands", "\u{1b}]0;x\u{7}"]] {
        let error = said(line);
        assert!(!error.chars().any(char::is_control) || error.starts_with("no command"), "{error:?}");
    }
    // A very long argument is cut.
    let long = "x".repeat(10_000);
    assert!(said(&["export", "d.vsdx", &format!("--{long}")]).len() < 200);
}

/// The steps of `run` are put back in the order the line gave them, though clap reads each option as a list of its own.
#[test]
fn run_puts_cmd_params_and_commands_back_in_the_order_given() {
    use args::Source;
    let Ok(Action::Run(sources)) = parse(&[
        "run",
        "--commands",
        "a.jsonl",
        "--cmd",
        "file.new",
        "--cmd",
        "page.add",
        "--params",
        "{\"name\":\"Two\"}",
        "--commands",
        "-",
        "--cmd=x",
    ]) else {
        panic!("a line of steps");
    };
    let shape: Vec<String> = sources
        .iter()
        .map(|source| match source {
            Source::Cmd { id, params } => format!("cmd {id} {params}"),
            Source::Commands(from) => format!("commands {from}"),
        })
        .collect();
    assert_eq!(shape, ["commands a.jsonl", "cmd file.new {}", "cmd page.add {\"name\":\"Two\"}", "commands -", "cmd x {}"]);
    for (line, words) in [
        (&["run", "--params", "{}", "--cmd", "a"][..], "--params must follow the --cmd it is for"),
        (&["run", "--cmd", "a", "--params", "{}", "--params", "{}"], "--params was given twice for one --cmd"),
        (&["run", "--cmd", "a", "--commands", "f", "--params", "{}"], "--params must follow the --cmd it is for"),
        (&["run", "--cmd", "a", "--params", "[1]"], "--params must be a JSON object"),
        (&["run", "--commands", "-", "--commands", "-"], "--commands - was given twice: standard input is read once"),
        (&["run"], "nothing to run: give --cmd or --commands"),
    ] {
        assert!(parse(line).unwrap_err().contains(words), "{line:?}");
    }
}

/// **No line of words is a crash.** Lines made of the words of the grammar (commands, options, dashes, values) and of any text at all, in any order, are read without an internal error:
/// clap's own panics (a description it finds inconsistent, a word it cannot place) are caught by `read` and become a refusal that says so, which this holds to never happening; and a
/// line that is read is read the same way twice.
mod hostile_lines {
    use proptest::prelude::*;

    use crate::args;

    const WORDS: &[&str] = &[
        "mcp",
        "run",
        "commands",
        "oracle",
        "export",
        "convert",
        "roundtrip",
        "compare",
        "completions",
        "man",
        "--out",
        "--page",
        "--dpi",
        "--text",
        "--cmd",
        "--params",
        "--commands",
        "--color",
        "--color=always",
        "--connect",
        "--headless",
        "--bundled-fonts",
        "--via",
        "--edit",
        "--move",
        "--program",
        "--list",
        "--content",
        "--aggregate",
        "--cap",
        "--source",
        "--json",
        "--help",
        "--version",
        "-h",
        "-V",
        "-",
        "--",
        "---",
        "-o",
        "-odd",
        "=",
        "--out=",
        "--out=x",
        "always",
        "bash",
        "diacraft",
        "diacraft-cli-run",
        "1",
        "0",
        "-1",
        "1.5,2",
        "{}",
        "[]",
        "a.vsdx",
        "p.svg",
        "p.png",
        "",
    ];

    fn word() -> impl Strategy<Value = String> {
        prop_oneof![
            4 => proptest::sample::select(WORDS).prop_map(str::to_owned),
            1 => "\\PC{0,12}",
            1 => "[-=a-z0-9.,{}\\[\\]\"' ]{0,20}",
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(3000))]

        #[test]
        fn no_line_of_words_is_an_internal_error_and_each_is_read_the_same_twice(line in prop::collection::vec(word(), 0..12)) {
            let first = args::parse(&line);
            if let Err(e) = &first {
                prop_assert!(!e.contains("internal error"), "{line:?}: {e}");
                prop_assert!(!e.chars().any(|c| c.is_control() && c != '\n'), "{line:?}: {e:?}");
            }
            prop_assert_eq!(first, args::parse(&line));
        }
    }
}
