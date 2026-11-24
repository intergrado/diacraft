//! The real window, started as a person starts the app and looked at through its own `ui.screenshot`.
//!
//! On a dual-GPU Mac the window was black and the screenshot read back every pixel as (0, 0, 0, 0): the GPU it drew on ran
//! none of its commands. This test starts the app, takes a screenshot through the control channel and checks that it holds
//! the ribbon's colours: the top bar, the ribbon body and the File tab's accent, of the light theme or the dark one.
//!
//! It opens a window on this machine's screen for a few seconds, so it is not part of the suite: run it on a machine with a
//! display, as the ignore message says.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use diacraft_ui_egui::theme::Tokens;
use serde_json::{Value, json};

/// The app, ended when the test ends however it ends.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A port nothing listens on now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The control channel on `port`, once the app listens on it.
fn connect(port: u16, app: &mut App) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            return stream;
        }
        assert!(app.0.try_wait().unwrap().is_none(), "the app ended before its control channel opened");
        assert!(Instant::now() < deadline, "the control channel did not open in a minute");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Sends one request and returns its `result`, failing the test on an error answer.
fn call(stream: &mut TcpStream, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"id": id, "method": method, "params": params}).to_string() + "\n";
    stream.write_all(line.as_bytes()).unwrap();
    let mut answer = String::new();
    BufReader::new(stream.try_clone().unwrap()).read_line(&mut answer).unwrap();
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["ok"], json!(true), "{method}: {answer}");
    answer["result"].clone()
}

/// The 8-bit RGBA pixels of the PNG at `path`, with its width and height.
fn read_png(path: &Path) -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    pixels.truncate(info.buffer_size());
    (pixels, info.width, info.height)
}

/// How many pixels of `pixels` are exactly `colour`.
fn count(pixels: &[u8], colour: [u8; 4]) -> usize {
    pixels.as_chunks::<4>().0.iter().filter(|p| **p == colour).count()
}

#[test]
#[ignore = "opens the app's window on this machine's screen for a few seconds: cargo nextest run -p diacraft --run-ignored only -E 'test(the_window_draws_the_ribbon)'"]
fn the_window_draws_the_ribbon() {
    let dir: PathBuf = std::env::temp_dir().join(format!("diacraft-window-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shot = dir.join("window.png");
    let port = free_port();
    let mut app = App(Command::new(env!("CARGO_BIN_EXE_diacraft"))
        .args(["--control", &port.to_string()])
        .env("DIACRAFT_NO_SPLASH", "1")
        .env("DIACRAFT_NO_PREFS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap());
    let mut stream = connect(port, &mut app);
    // A few frames first: the window's first frames lay the ribbon out.
    let deadline = Instant::now() + Duration::from_secs(30);
    while call(&mut stream, 1, "ui.inspect", json!({}))["frames"].as_u64().unwrap_or(0) < 3 {
        assert!(Instant::now() < deadline, "the window drew fewer than 3 frames in 30 s");
        std::thread::sleep(Duration::from_millis(100));
    }
    let taken = call(&mut stream, 2, "ui.screenshot", json!({"path": shot.to_string_lossy()}));
    call(&mut stream, 3, "app.quit", json!({}));
    let quit_by = Instant::now() + Duration::from_secs(15);
    while app.0.try_wait().unwrap().is_none() && Instant::now() < quit_by {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(app.0.try_wait().unwrap().is_some(), "the app did not quit in 15 s after app.quit");

    let (pixels, width, height) = read_png(&shot);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((taken["width"].as_u64(), taken["height"].as_u64()), (Some(u64::from(width)), Some(u64::from(height))));
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| *p != [0, 0, 0, 0]), "every pixel is (0, 0, 0, 0): the GPU drew nothing");
    let found = |t: &Tokens| {
        [("top bar", t.top_bar_fill), ("ribbon", t.ribbon_fill), ("File tab", t.accent)].map(|(part, c)| (part, count(&pixels, c.to_array())))
    };
    let (light, dark) = (found(&Tokens::light()), found(&Tokens::dark()));
    // The File tab is the smallest of the three: about 38 by 20 points, so 760 pixels at one pixel a point.
    let holds = |parts: &[(&str, usize); 3]| parts.iter().all(|(_, n)| *n >= 300);
    assert!(holds(&light) || holds(&dark), "a {width} by {height} screenshot without the ribbon's colours: light {light:?}, dark {dark:?}");
}
