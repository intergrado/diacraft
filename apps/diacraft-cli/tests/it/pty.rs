//! A pseudo-terminal to run the command line in, so that the tests of its colours and its width see what a person at a terminal sees (Unix only: `rustix`
//! makes the terminal in safe code).

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::process::{Command, Stdio};

use rustix::fs::{Mode, OFlags};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

use crate::BIN;

/// What the program printed on a terminal of `columns` columns, with its status. Standard input, output and error are all the terminal; the colour variables of the test's
/// own environment are removed first and `env` is set after (`TERM` is `xterm-256color` unless `env` says otherwise).
pub fn in_terminal(args: &[&str], env: &[(&str, &str)], columns: u16) -> (String, Option<i32>) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = rustix::fs::open(&name, OFlags::RDWR | OFlags::NOCTTY, Mode::empty()).unwrap();
    tcsetwinsize(&slave, Winsize { ws_row: 40, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0 }).unwrap();

    let mut command = Command::new(BIN);
    command.args(args).env_remove("RUST_LOG").env("TERM", "xterm-256color");
    // A test never reads or writes the person's settings: `run` and `mcp` open them unless this is set.
    command.env("DIACRAFT_NO_PREFS", "1");
    for variable in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "COLUMNS"] {
        command.env_remove(variable);
    }
    for (key, value) in env {
        command.env::<&OsStr, &OsStr>(key.as_ref(), value.as_ref());
    }
    command.stdin(Stdio::from(slave.try_clone().unwrap())).stdout(Stdio::from(slave.try_clone().unwrap())).stderr(Stdio::from(slave));
    let mut child = command.spawn().unwrap();
    // The parent's copies of the slave went into the command (which `spawn` keeps until it is dropped): drop it so that the master sees the end.
    drop(command);

    let mut printed = Vec::new();
    // Linux says EIO, not the end of the file, once every holder of the slave has gone: what was read is all there is.
    let _ = File::from(master).read_to_end(&mut printed);
    let status = child.wait().unwrap().code();
    // A terminal turns a line feed into a carriage return and a line feed.
    (String::from_utf8_lossy(&printed).replace("\r\n", "\n"), status)
}
