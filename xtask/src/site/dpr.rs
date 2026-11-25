//! `cargo xtask web-dpr [DIR]`: the web app in a headless Chrome at device scale factors of 1, 1.5, 2 and 3, and a phone-sized page at 3, checking that the canvas has the pixels its page needs.
//!
//! **Why this check exists.** eframe sizes the canvas from the `devicePixelContentBoxSize` its `ResizeObserver` gives, and a browser that reports that in CSS pixels where the ratio is more than 1
//! lays the window out for 1 / ratio of the page's width (`apps/diacraft-web/src/canvas_size.rs` has the whole story, and the guard that is its fix). A real Chrome with a real scale factor reports it right,
//! so only a browser told to *emulate* a ratio (`Emulation.setDeviceMetricsOverride`: DevTools' device mode, Lighthouse's mobile runs, the test tools' `deviceScaleFactor`) shows it, and only the DevTools protocol can ask for that. No
//! unit test can say that the page the browser really builds is the right size, so this runs the built app.
//!
//! **What it does.** It serves the app's folder (`dist/app`, from `cargo xtask site --web`, or DIR) on a loopback port with a small file server of its own (a plain one: the folder's `_headers` is not applied),
//! starts a headless Chrome or Chromium with a profile of its own (the program `CHROME` names, else the first of `google-chrome`, `chromium` and the like on the PATH; `CHROME_ARGS` adds
//! arguments, such as `--no-sandbox` for a container that runs as root), and drives it over the DevTools protocol ([`super::cdp`]). For each [`SCENARIOS`] entry it makes a page, emulates the size and the
//! ratio of each of its steps (a later step is a resize of the page that is running), loads the app, waits until it has drawn, and reads the canvas: its size in CSS pixels, its size in pixels and the
//! ratio. It **fails** when the canvas's pixels are not its CSS size times the ratio, within the browser's own rounding ([`TOLERANCE`]), which is what makes the window's size in points the page's. A
//! scenario's result is the last of a few seconds of looking: a canvas that is put right a frame late passes, one that stays wrong does not.
//!
//! It needs Chrome or Chromium, which `ci` does not assume, so it is not a step of `ci`: run it after a change to the web shell or the canvas's size, and before a release.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::cdp::{Cdp, parse_active_port};

pub(super) const USAGE: &str = "usage: cargo xtask web-dpr [DIR]   (DIR: the web app's folder, `dist/app` when none; needs Chrome or Chromium: the program on the PATH, or CHROME; extra arguments in CHROME_ARGS)";

/// The most the canvas's pixels may differ from its CSS size times the ratio on a side: the browser's rounding. The app's own guard lets the same go (`TOLERANCE` of
/// `apps/diacraft-web/src/canvas_size.rs`; a test holds the two equal).
pub(super) const TOLERANCE: u32 = 3;

/// How long the app has to start: a software-rendered Chrome compiles 18 MB of WebAssembly and starts the graphics.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a step is looked at: the canvas is put right on the frame after a resize, and a canvas that is not right by then stays wrong.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(8);

/// How long to wait for the browser to write its `DevToolsActivePort` file.
const BROWSER_TIMEOUT: Duration = Duration::from_secs(30);

/// The id of the page's canvas and of its loading text (`CANVAS_ID` and `LOADING_ID` of `apps/diacraft-web/src/page.rs`; a test holds them equal).
pub(super) const CANVAS_ID: &str = "diacraft_canvas";
pub(super) const LOADING_ID: &str = "diacraft_loading";

/// One size and ratio the page is emulated at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Step {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) ratio: f64,
}

/// A page and what is done to it: emulated at its first step, loaded, then emulated at each later step (a resize of the running app).
#[derive(Clone, Copy, Debug)]
pub(super) struct Scenario {
    pub(super) name: &'static str,
    /// The emulation is of a phone (`mobile`): the page has the viewport of its `<meta name="viewport">`.
    pub(super) mobile: bool,
    pub(super) steps: &'static [Step],
}

const fn step(width: u32, height: u32, ratio: f64) -> Step {
    Step { width, height, ratio }
}

/// The scenarios: the ratios the report named (1, 1.5, 2 and 3), the phone-sized page of it (390 by 844 at 3), and a running app that is resized and given another ratio.
pub(super) const SCENARIOS: &[Scenario] = &[
    Scenario { name: "desktop, ratio 1", mobile: false, steps: &[step(1200, 700, 1.0)] },
    Scenario { name: "desktop, ratio 1.5", mobile: false, steps: &[step(1200, 700, 1.5)] },
    Scenario { name: "desktop, ratio 2", mobile: false, steps: &[step(1200, 700, 2.0)] },
    Scenario { name: "desktop, ratio 3", mobile: false, steps: &[step(1200, 700, 3.0)] },
    Scenario { name: "phone, ratio 3", mobile: true, steps: &[step(390, 844, 3.0)] },
    Scenario { name: "resized while running", mobile: false, steps: &[step(1200, 700, 2.0), step(800, 600, 3.0), step(1000, 650, 1.5)] },
];

/// What the page says of its canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Probe {
    pub(super) ratio: f64,
    /// The canvas's size in CSS pixels.
    pub(super) css: (f64, f64),
    /// The canvas's size in its own pixels.
    pub(super) canvas: (f64, f64),
}

/// The script that reads a [`Probe`]: the page's own numbers, as JSON.
pub(super) fn probe_script() -> String {
    format!(
        "(() => {{ const c = document.getElementById('{CANVAS_ID}'); if (!c) return null; const r = c.getBoundingClientRect(); \
         return JSON.stringify({{ratio: window.devicePixelRatio, css: [r.width, r.height], canvas: [c.width, c.height]}}); }})()"
    )
}

/// The script that says whether the app has started: its loading text is gone (the program removes it when it has drawn its first frame), and what the loading text says while it is not.
pub(super) fn started_script() -> String {
    format!(
        "(() => {{ const l = document.getElementById('{LOADING_ID}'); const c = document.getElementById('{CANVAS_ID}'); \
         return JSON.stringify({{started: !!c && !l, text: l ? l.innerText : ''}}); }})()"
    )
}

/// Reads what [`probe_script`] gave.
pub(super) fn parse_probe(text: &str) -> Result<Probe, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("the page's answer is not JSON: {e}"))?;
    let pair = |key: &str| -> Option<(f64, f64)> {
        let array = value.get(key)?.as_array()?;
        Some((array.first()?.as_f64()?, array.get(1)?.as_f64()?))
    };
    let ratio = value.get("ratio").and_then(Value::as_f64).ok_or("the page's answer has no ratio")?;
    let css = pair("css").ok_or("the page's answer has no CSS size")?;
    let canvas = pair("canvas").ok_or("the page's answer has no canvas size")?;
    if ![ratio, css.0, css.1, canvas.0, canvas.1].iter().all(|n| n.is_finite()) {
        return Err("the page's answer has a number that is not finite".into());
    }
    Ok(Probe { ratio, css, canvas })
}

/// Whether the canvas of `probe` is right for `step`: the emulation took (the ratio and the size of the page are the step's), the canvas fills the page, and its pixels are its CSS size times the ratio. `Ok` is the
/// line to print, `Err` the words of the first thing that is wrong.
pub(super) fn judge(step: &Step, probe: &Probe) -> Result<String, String> {
    if (probe.ratio - step.ratio).abs() > 1e-3 {
        return Err(format!("the browser's device pixel ratio is {}, not the {} that was emulated", probe.ratio, step.ratio));
    }
    if (probe.css.0 - f64::from(step.width)).abs() > 1.0 || (probe.css.1 - f64::from(step.height)).abs() > 1.0 {
        return Err(format!(
            "the canvas is {}x{} CSS pixels in a page of {}x{}: it does not fill the page",
            probe.css.0, probe.css.1, step.width, step.height
        ));
    }
    let (want_w, want_h) = ((probe.css.0 * probe.ratio).round(), (probe.css.1 * probe.ratio).round());
    let tolerance = f64::from(TOLERANCE);
    let points = (probe.canvas.0 / probe.ratio, probe.canvas.1 / probe.ratio);
    if (probe.canvas.0 - want_w).abs() > tolerance || (probe.canvas.1 - want_h).abs() > tolerance {
        return Err(format!(
            "the canvas has {}x{} pixels where {}x{} CSS pixels at a ratio of {} need {want_w}x{want_h}: the window is laid out for {:.0}x{:.0} points, not the page's {}x{}",
            probe.canvas.0, probe.canvas.1, probe.css.0, probe.css.1, probe.ratio, points.0, points.1, step.width, step.height
        ));
    }
    Ok(format!(
        "{}x{} CSS pixels at a ratio of {}: a canvas of {}x{} pixels, a window of {:.0}x{:.0} points",
        step.width, step.height, step.ratio, probe.canvas.0, probe.canvas.1, points.0, points.1
    ))
}

// ------------------------------------------------------------------------------------------------------------------------------ the file server

/// The type a browser is told a file is. The app's `_headers` is not applied here; a `.wasm` file must be `application/wasm` for the streaming compile of the loader.
pub(super) fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

/// The path of the request line `GET /x?y HTTP/1.1` (or `HEAD`), without its query. `None` for anything else.
pub(super) fn request_target(head: &str) -> Option<(bool, String)> {
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    let get = match method {
        "GET" => true,
        "HEAD" => false,
        _ => return None,
    };
    let path = target.split(['?', '#']).next()?;
    path.starts_with('/').then(|| (get, path.to_owned()))
}

/// The file of `root` a request path names: `/` and a path that ends in a slash are the `index.html` there, and a name with `..`, a backslash, a colon, a percent sign or a control character in it is no
/// file. A path that is not a regular file (a folder, a link) is none either.
pub(super) fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let mut file = root.to_path_buf();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment == "." || segment == ".." || segment.chars().any(|c| matches!(c, '\\' | ':' | '%') || c.is_control()) {
            return None;
        }
        file.push(segment);
    }
    if path.ends_with('/') {
        file.push("index.html");
    }
    let meta = fs::symlink_metadata(&file).ok()?;
    (meta.is_file() && meta.len() <= 64 * 1024 * 1024).then_some(file)
}

/// Answers one request of `stream` from `root`. Every failure is the connection's: it is dropped.
fn serve_connection(mut stream: TcpStream, root: &Path) {
    // Failures below are a browser that went away: there is nobody to tell, and nothing to do.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 8 * 1024 {
        match stream.read(&mut byte) {
            Ok(1) => head.extend_from_slice(&byte),
            _ => return,
        }
    }
    let text = String::from_utf8_lossy(&head);
    let (status, kind, body, send_body) = match request_target(&text) {
        Some((send_body, path)) => match resolve(root, &path).and_then(|file| Some((content_type(&file), fs::read(&file).ok()?))) {
            Some((kind, body)) => ("200 OK", kind, body, send_body),
            None => ("404 Not Found", "text/plain; charset=utf-8", b"not found".to_vec(), send_body),
        },
        None => ("405 Method Not Allowed", "text/plain; charset=utf-8", b"GET and HEAD only".to_vec(), true),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(response.as_bytes()).is_ok() && send_body {
        // A browser that stopped reading is not an error of the check.
        let _ = stream.write_all(&body);
    }
}

/// Starts serving `root` on a loopback port of the system's choosing, on threads that live as long as the program. Returns the port.
pub(super) fn serve(root: PathBuf) -> Result<u16, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("could not listen on the loopback address: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("could not read the port of the file server: {e}"))?.port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            thread::spawn(move || serve_connection(stream, &root));
        }
    });
    Ok(port)
}

// ---------------------------------------------------------------------------------------------------------------------------------- the browser

/// The programs tried for a Chrome, in order, when `CHROME` names none.
pub(super) const BROWSER_CANDIDATES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
];

/// The program that is the browser: `named` (the value of `CHROME`) when there is one, else the first of [`BROWSER_CANDIDATES`] that answers `--version`.
pub(super) fn find_browser(named: Option<&str>) -> Result<String, String> {
    let works = |program: &str| {
        Command::new(program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    };
    if let Some(program) = named.filter(|p| !p.is_empty()) {
        return if works(program) { Ok(program.to_owned()) } else { Err(format!("CHROME names `{program}`, which does not run with --version")) };
    }
    BROWSER_CANDIDATES
        .iter()
        .find(|program| works(program))
        .map(|program| (*program).to_owned())
        .ok_or_else(|| format!("no Chrome or Chromium was found (tried {}): install one, or name it in CHROME", BROWSER_CANDIDATES.join(", ")))
}

/// The arguments the browser is started with: headless, with a profile of its own, DevTools on a port the system chooses (it writes it in the profile's `DevToolsActivePort`), and software graphics
/// (SwiftShader), so that a machine with no GPU runs the app's WebGL 2 as the browser version falls back to it. `extra` is the user's (`CHROME_ARGS`), split at spaces.
pub(super) fn browser_args(profile: &Path, extra: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "--headless=new",
        "--remote-debugging-port=0",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-dev-shm-usage",
        "--hide-scrollbars",
        "--enable-unsafe-swiftshader",
        "--use-angle=swiftshader",
        "--ignore-gpu-blocklist",
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    args.push(format!("--user-data-dir={}", profile.display()));
    args.extend(extra.split_whitespace().map(str::to_owned));
    args.push("about:blank".to_owned());
    args
}

/// The most names tried for a profile folder before giving up: a random name of 128 bits is taken only by a folder that someone made on purpose.
const PROFILE_TRIES: usize = 16;

/// A random name for a profile folder: 128 bits that a program or a user on this machine cannot guess (`RandomState` is seeded by the operating system, and each state is hashed with the process, the time and
/// a count). A name that could be guessed, such as one made from the process's number, is one that someone else could make before this does.
pub(super) fn random_name() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let salt = (std::process::id(), nanos, COUNT.fetch_add(1, Ordering::Relaxed));
    format!("diacraft-web-dpr-{:016x}{:016x}", RandomState::new().hash_one(salt), RandomState::new().hash_one(salt))
}

/// Makes the folder `path`, which must not exist (its parent must): a folder that is already there, a link, or a file of that name is an error, never reused. On Unix its mode is 0700, so that no other
/// user can read or write what a browser keeps in it.
pub(super) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    // Not recursive: that would make the parents, and accept a folder that is there already.
    builder.recursive(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// A new folder for a browser's profile, removed when it is dropped.
#[derive(Debug)]
pub(super) struct Profile(PathBuf);

impl Profile {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    /// Makes a new folder under `base` with the names `next` gives, in turn: one that exists is skipped (it is someone's, not ours), and after [`PROFILE_TRIES`] of them it is an error. The folder is
    /// empty when this returns, so nothing in it, a `DevToolsActivePort` file included, is anything but what the browser started afterwards put there.
    pub(super) fn create_with(base: &Path, mut next: impl FnMut() -> String) -> Result<Profile, String> {
        for _ in 0..PROFILE_TRIES {
            let path = base.join(next());
            match create_private_dir(&path) {
                Ok(()) => {
                    let profile = Profile(path);
                    let empty = fs::read_dir(profile.path()).is_ok_and(|mut entries| entries.next().is_none());
                    return if empty {
                        Ok(profile)
                    } else {
                        Err(format!("{} is not empty, and a browser's profile must start so", profile.path().display()))
                    };
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("could not make {}: {e}", path.display())),
            }
        }
        Err(format!("could not make a profile folder of a name that was not taken under {} in {PROFILE_TRIES} tries", base.display()))
    }
}

/// How many times, and how far apart, a profile folder is removed again when it is not removed the first time.
const REMOVE_TRIES: usize = 40;
const REMOVE_PAUSE: Duration = Duration::from_millis(250);

/// Removes `path` with `remove`, again after `pause` while it fails with anything but "not found", up to `tries` times, and gives the last failure. A browser that has just been stopped has processes of
/// its own (the renderers, the graphics process) that are still closing and still writing in its profile for a moment, and a removal that races with them fails or leaves the folder behind.
pub(super) fn remove_folder(path: &Path, tries: usize, pause: Duration, mut remove: impl FnMut(&Path) -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = Ok(());
    for attempt in 0..tries {
        match remove(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Err(e),
        }
        if attempt + 1 < tries {
            thread::sleep(pause);
        }
    }
    last
}

impl Drop for Profile {
    fn drop(&mut self) {
        // Best effort: a folder that cannot be removed in all those tries is left, and it is private to this user (mode 0700 on Unix).
        let _ = remove_folder(&self.0, REMOVE_TRIES, REMOVE_PAUSE, |path| fs::remove_dir_all(path));
    }
}

/// A browser this program started, and its profile folder. It is stopped, and the profile removed, when this is dropped.
pub(super) struct Browser {
    child: Child,
    // After `child` in the struct, so that the browser is stopped before its folder is removed.
    _profile: Profile,
}

impl Browser {
    /// Starts `program` with a new profile folder under the system's temporary folder (`TMPDIR` on Unix) and waits for it to say where its DevTools are: the port and the path of its WebSocket.
    pub(super) fn start(program: &str, extra: &str) -> Result<(Browser, u16, String), String> {
        Browser::start_in(&std::env::temp_dir(), program, extra)
    }

    /// [`Browser::start`] with the profile folder made under `base`. The folder is new, with a random name and private to this user, and what is read from it is only what the browser started here wrote: a
    /// `DevToolsActivePort` file that was in a folder before is never trusted, since no folder of ours was there before.
    pub(super) fn start_in(base: &Path, program: &str, extra: &str) -> Result<(Browser, u16, String), String> {
        let profile = Profile::create_with(base, random_name)?;
        let child = Command::new(program)
            .args(browser_args(profile.path(), extra))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {program}: {e}"))?;
        let port_file = profile.path().join("DevToolsActivePort");
        let mut browser = Browser { child, _profile: profile };
        let started = Instant::now();
        loop {
            if let Ok(text) = fs::read_to_string(&port_file)
                && let Ok((port, path)) = parse_active_port(&text)
            {
                return Ok((browser, port, path));
            }
            if let Ok(Some(status)) = browser.child.try_wait() {
                return Err(format!("{program} ended at once ({status}); in a container that runs as root, CHROME_ARGS=--no-sandbox may be needed"));
            }
            if started.elapsed() > BROWSER_TIMEOUT {
                return Err(format!("{program} did not say where its DevTools are in {} seconds", BROWSER_TIMEOUT.as_secs()));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Best effort: a browser that is already gone is what this wants. The profile folder is removed by its own drop, after this.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ the run

/// Runs `script` in the page of `session`, and the string it gives.
fn evaluate(cdp: &mut Cdp, session: &str, script: &str) -> Result<Option<String>, String> {
    let answer = cdp.call(Some(session), "Runtime.evaluate", json!({ "expression": script, "returnByValue": true }))?;
    if let Some(details) = answer.get("exceptionDetails") {
        return Err(format!("the page's script failed: {}", details.to_string().chars().take(300).collect::<String>()));
    }
    Ok(answer.get("result").and_then(|r| r.get("value")).and_then(Value::as_str).map(str::to_owned))
}

/// Waits until the app has drawn its first frame, and says what the loading text said if it has not by [`START_TIMEOUT`].
fn wait_started(cdp: &mut Cdp, session: &str) -> Result<(), String> {
    let started = Instant::now();
    let mut said = String::new();
    while started.elapsed() < START_TIMEOUT {
        // A page that is still navigating answers with an error or nothing: look again.
        if let Ok(Some(text)) = evaluate(cdp, session, &started_script())
            && let Ok(value) = serde_json::from_str::<Value>(&text)
        {
            if value.get("started").and_then(Value::as_bool) == Some(true) {
                return Ok(());
            }
            said = value.get("text").and_then(Value::as_str).unwrap_or_default().replace('\n', " ");
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "the app did not draw its first frame in {} seconds (the page says: `{}`)",
        START_TIMEOUT.as_secs(),
        said.chars().take(200).collect::<String>()
    ))
}

/// Looks at the canvas for [`SETTLE_TIMEOUT`] and gives the first right answer, or the last wrong one.
fn look(cdp: &mut Cdp, session: &str, step: &Step) -> Result<String, String> {
    let started = Instant::now();
    loop {
        let verdict = match evaluate(cdp, session, &probe_script()) {
            Ok(Some(text)) => parse_probe(&text).and_then(|probe| judge(step, &probe)),
            Ok(None) => Err("the page has no canvas".to_owned()),
            Err(e) => Err(e),
        };
        if verdict.is_ok() || started.elapsed() > SETTLE_TIMEOUT {
            return verdict;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Runs one scenario on a page of its own and returns the line of each step.
fn run_scenario(cdp: &mut Cdp, port: u16, scenario: &Scenario) -> Result<Vec<String>, String> {
    let target = cdp.call(None, "Target.createTarget", json!({ "url": "about:blank" }))?;
    let target_id = target.get("targetId").and_then(Value::as_str).ok_or("the browser made no page")?.to_owned();
    let attached = cdp.call(None, "Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }))?;
    let session = attached.get("sessionId").and_then(Value::as_str).ok_or("the browser gave no session for the page")?.to_owned();
    let outcome = run_steps(cdp, &session, port, scenario);
    // The page is closed whatever happened; a browser that cannot is about to be stopped anyway.
    let _ = cdp.call(None, "Target.closeTarget", json!({ "targetId": target_id }));
    outcome
}

fn run_steps(cdp: &mut Cdp, session: &str, port: u16, scenario: &Scenario) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for (index, step) in scenario.steps.iter().enumerate() {
        let metrics = json!({ "mobile": scenario.mobile, "width": step.width, "height": step.height, "deviceScaleFactor": step.ratio });
        cdp.call(Some(session), "Emulation.setDeviceMetricsOverride", metrics)?;
        if index == 0 {
            cdp.call(Some(session), "Page.navigate", json!({ "url": format!("http://127.0.0.1:{port}/") }))?;
            wait_started(cdp, session)?;
        }
        lines.push(look(cdp, session, step)?);
    }
    Ok(lines)
}

/// The folder the arguments name: `dist/app` of `root` when there are none, else the one given.
pub(super) fn parse_args(root: &Path, args: &[&str]) -> Result<PathBuf, String> {
    match args {
        [] => Ok(root.join("dist").join("app")),
        [dir] if !dir.starts_with('-') && !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Err(USAGE.to_owned()),
    }
}

/// The command: serves the folder, starts the browser, runs every scenario and prints each step. Fails, naming each, when a scenario has a canvas that is wrong.
pub(super) fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let dir = parse_args(root, args)?;
    for needed in ["index.html", "manifest.json"] {
        if !dir.join(needed).is_file() {
            return Err(format!("{} has no {needed}: make the app's folder with `cargo xtask site --web` first, or name it\n{USAGE}", dir.display()));
        }
    }
    let program = find_browser(std::env::var("CHROME").ok().as_deref())?;
    let port = serve(dir.clone())?;
    let (browser, debug_port, path) = Browser::start(&program, &std::env::var("CHROME_ARGS").unwrap_or_default())?;
    let mut cdp = Cdp::connect(debug_port, &path)?;
    println!("web-dpr: {} on {}, served from http://127.0.0.1:{port}/", program, dir.display());
    let mut failures = Vec::new();
    for scenario in SCENARIOS {
        match run_scenario(&mut cdp, port, scenario) {
            Ok(lines) => lines.iter().for_each(|line| println!("  ok    {}: {line}", scenario.name)),
            Err(why) => {
                println!("  FAIL  {}: {why}", scenario.name);
                failures.push(format!("{}: {why}", scenario.name));
            }
        }
    }
    drop(cdp);
    drop(browser);
    if failures.is_empty() {
        println!("web-dpr: {} scenarios, the canvas had the pixels its page needs in each", SCENARIOS.len());
        Ok(())
    } else {
        Err(format!("{} of {} scenarios failed:\n  {}", failures.len(), SCENARIOS.len(), failures.join("\n  ")))
    }
}
