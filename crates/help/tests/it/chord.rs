//! The keys a help text names: read, canonical, and shown for each platform.

use diacraft_help::chord::{Chord, Platform};

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|| panic!("{text} should be a key"))
}

#[test]
fn a_key_is_read_with_its_modifiers_in_any_order_and_written_in_the_canonical_one() {
    let c = chord("Shift+Cmd+Z");
    assert_eq!((c.cmd, c.ctrl, c.alt, c.shift, c.key.as_str()), (true, false, false, true, "Z"));
    assert_eq!(c.canonical(), "Cmd+Shift+Z");
    assert_eq!(chord("Alt+Shift+Ctrl+F5").canonical(), "Ctrl+Alt+Shift+F5");
    assert_eq!(chord("F1").canonical(), "F1");
    assert_eq!(chord("5").canonical(), "5");
}

#[test]
fn every_kind_of_key_is_read() {
    for key in [
        "A",
        "Z",
        "0",
        "9",
        "F1",
        "F12",
        "F24",
        "Escape",
        "Tab",
        "Backspace",
        "Enter",
        "Space",
        "Insert",
        "Delete",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Up",
        "Down",
        "Left",
        "Right",
        "-",
        "=",
        "+",
        "[",
        "]",
        "\\",
        ";",
        "'",
        "`",
        ",",
        ".",
        "/",
    ] {
        let c = chord(&format!("Cmd+{key}"));
        assert_eq!(c.key, key);
        assert_eq!(Chord::parse(&c.canonical()), Some(c.clone()), "{key} reads back");
    }
    assert_eq!(chord("Cmd++").key, "+");
    assert_eq!(chord("+").key, "+");
}

#[test]
fn what_is_no_key_is_refused() {
    for bad in [
        "",
        "Hyper+Z",
        "Cmd+",
        "Cmd+Shift",
        "Cmd+Ctrl+Z",
        "Cmd+Cmd+Z",
        "cmd+z",
        "Cmd+z",
        "Cmd+F0",
        "Cmd+F36",
        "Cmd+Esc",
        "Cmd + Z",
        "Cmd+Z+Q",
        "Cmd+AA",
        "Cmd+É",
        "Cmd+\n",
        " Cmd+Z",
        "Cmd+Z ",
        "Shift",
        "Alt",
    ] {
        assert_eq!(Chord::parse(bad), None, "{bad:?}");
    }
    assert_eq!(Chord::parse(&format!("Cmd+{}", "A".repeat(100))), None);
}

#[test]
fn a_key_is_shown_as_each_platform_writes_it() {
    let undo = chord("Cmd+Shift+Z");
    assert_eq!(undo.display(Platform::Windows), "Ctrl+Shift+Z");
    assert_eq!(undo.display(Platform::Linux), "Ctrl+Shift+Z");
    assert_eq!(undo.display(Platform::Mac), "⇧⌘Z");
    assert_eq!(chord("Alt+Q").display(Platform::Windows), "Alt+Q");
    assert_eq!(chord("Alt+Q").display(Platform::Mac), "⌥Q");
    assert_eq!(chord("Cmd+Alt+F").display(Platform::Mac), "⌥⌘F");
    assert_eq!(chord("Alt+Shift+Cmd+F").display(Platform::Mac), "⌥⇧⌘F");
    assert_eq!(chord("Ctrl+Tab").display(Platform::Mac), "⌃⇥");
    assert_eq!(chord("Ctrl+Tab").display(Platform::Windows), "Ctrl+Tab");
    assert_eq!(chord("F1").display(Platform::Mac), "F1");
    assert_eq!(chord("F1").display(Platform::Windows), "F1");
    assert_eq!(chord("PageUp").display(Platform::Windows), "Page Up");
    assert_eq!(chord("PageUp").display(Platform::Mac), "⇞");
    assert_eq!(chord("Escape").display(Platform::Windows), "Esc");
    assert_eq!(chord("Escape").display(Platform::Mac), "⎋");
    assert_eq!(chord("Cmd+Left").display(Platform::Mac), "⌘←");
    assert_eq!(chord("Cmd+Left").display(Platform::Windows), "Ctrl+Left");
    assert_eq!(chord("Cmd+=").display(Platform::Windows), "Ctrl+=");
    assert_eq!(chord("Cmd++").display(Platform::Windows), "Ctrl++");
    assert_eq!(chord("Cmd+Space").display(Platform::Windows), "Ctrl+Space");
}
