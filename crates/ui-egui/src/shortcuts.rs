//! Keyboard shortcuts: the default shortcut each command carries, mapped to this platform's keys.
//!
//! A command's shortcut is written as in the registry (`Cmd+Shift+Z`): `Cmd` is the platform's command key (Ctrl on
//! Windows, Linux and FreeBSD, Command on the Mac), the last word is the key. A shortcut runs its command only if the
//! command can run now, and a key the window used is consumed so that nothing else sees it.

use diacraft_engine::command_specs;
use egui::os::OperatingSystem;
use egui::{Event, Key, KeyboardShortcut, Modifiers};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::commands::UI_COMMANDS;

/// The shortcut a registry string names, if it names one this window knows how to press.
pub fn parse(spec: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    let mut words = spec.split('+').peekable();
    while let Some(word) = words.next() {
        // A key that is `+` itself ends the string with an empty last word; it is not one this window uses.
        let last = words.peek().is_none();
        match word {
            "Cmd" if !last => modifiers |= Modifiers::COMMAND,
            "Ctrl" if !last => modifiers |= Modifiers::CTRL,
            "Shift" if !last => modifiers |= Modifiers::SHIFT,
            "Alt" if !last => modifiers |= Modifiers::ALT,
            "=" if last => key = Some(Key::Equals),
            "-" if last => key = Some(Key::Minus),
            name if last => key = Key::from_name(name),
            _ => return None,
        }
    }
    key.map(|k| KeyboardShortcut::new(modifiers, k))
}

/// Second shortcuts for commands whose registry entry carries one: the keys a person may reach for first. **Assumed (no spec gives it):**
/// Ctrl+Y redoes, as Ctrl+Shift+Z does (the registry's `edit.redo` has the latter alone). **The zoom keys are the inventory's** ("Zoom
/// controls and keys"): Alt+F6 zooms in, Alt+Shift+F6 zooms out and Ctrl+Shift+W fits the page to the
/// window; the window's own Ctrl+= and Ctrl+- stay beside them. On a Mac, Fit to Window has a key of its own ([`MAC_KEYS`]).
const ALSO: &[(&str, &str)] = &[("edit.redo", "Cmd+Y"), ("view.zoomIn", "Alt+F6"), ("view.zoomOut", "Alt+Shift+F6"), ("view.fitPage", "Cmd+Shift+W")];

/// Second keys on a Mac only. **Ours**: no spec gives the reference app's keys on a Mac.
/// - **Backspace deletes the selection**, beside Delete: the key a Mac keyboard marks delete sends Backspace, and Delete is Fn+Delete there (Forward
///   Delete). It runs as Delete does, only while no text is being edited: with a shape's text open, or any other text field, the key is the text's.
///   Windows, Linux and FreeBSD keep Delete alone, the key the shortcut list names.
/// - **Command+Plus zooms in**, beside Command+=. With Command held the window takes a key's character from the layout's Command table, which ignores
///   Option and, on several layouts, Shift too; so the `=` that a German, Portuguese or Nordic layout types with Shift arrives as `0` (Fit to Window),
///   and there `+` is a key of its own. Before, Command+Plus was egui's own key and scaled the whole window. Every layout measured (the list in the
///   tests, from a Mac's own layout tables) reaches one of the two; the Swiss ones only by the key at the US position of `=`.
pub const MAC_ALSO: &[(&str, &str)] = &[("shape.delete", "Backspace"), ("view.zoomIn", "Cmd+Plus")];

/// The Mac's own key for a command whose key there is one the system, the app menu or the Mac's standard keys give to something else: the command, the
/// key it has elsewhere, and its key on a Mac (on macOS no shortcut is one the system or the
/// app menu reserves, and such a command gets another key). **Ours**: no spec gives the reference app's keys on a Mac. The sources are Apple's Human
/// Interface Guidelines ("Keyboards", the table of standard keyboard shortcuts, which asks an app not to repurpose one; read 2026-11-22 and 2026-11-23),
/// Apple's "Mac keyboard shortcuts" (support.apple.com/102650, read 2026-11-23), and winit's app menu (Command+H hides the app, Option+Command+H the
/// others, Command+Q quits: a menu's key is taken before the window sees it). Each Mac key is in no row of those lists, or is the row's own action:
/// - Fit to Window, **Command+0** (Command+Shift+W closes a file and its windows): Command alone, beside Command+= and Command+-, the key the sibling
///   craft apps give their own Fit in Window.
/// - Flip Horizontal, **Shift+Command+H** (Command+H hides DiaCraft), and Flip Vertical, **Option+Command+J** (Command+J scrolls to the selection;
///   Shift+Command+J is Justify): the letters of the other systems' keys, with the modifier that is free.
/// - The Text Block Tool, **Option+Command+4** (Shift+Command+4 takes a screenshot of a part of the screen): beside the other tools' Command+1 to 9.
/// - The Text dialog on its Font tab, **Command+T** (F11 shows the desktop): the Mac's own key to show the fonts, which the sibling craft apps give
///   their own Font dialog.
/// - Center, **Shift+Command+E** (Shift+Command+C shows the Colors window): beside Shift+Command+L, R and J, the other alignments, and a letter every
///   layout has. The Mac's own Command+| (the pick) cannot be pressed in this window on any common Mac layout but Latin American: with
///   Command held the window takes the character from the layout's Command table, which ignores Option, and on several layouts Shift too. So on the
///   US, British, Portuguese, Brazilian and Dutch layouts, which type `|` with Shift, Shift+Command+\ arrives as `\`; where `|` needs Option (German,
///   Spanish, Italian, the Nordic layouts; Shift and Option on the French ones) it arrives as the digit or letter of its key.
/// - Increase and Decrease Font Size, **Option+Command+.** and **Option+Command+,**: Shift+Command+, cannot be pressed on the French,
///   Belgian and Italian layouts, whose comma has no Shift character of its own (Shift with it types `?`, and the key that types `.` with Shift is
///   taken by Increase); with Option, which the Command table ignores, both keys arrive as the full stop and the comma on every common layout. The two
///   keep the same modifiers.
/// - All Caps, **Option+Command+A** (Shift+Command+A deselects all).
/// - Align Text Bottom, **Option+Command+B** (Shift+Command+V pastes as; Option and Control with V are taken too).
/// - The Format Painter, **Option+Command+C** (Shift+Command+P is Page Setup): the Mac's own key to copy the style of the selection.
/// - Group, **Option+Command+G** (Shift+Command+G finds the previous match): the key a sibling craft app gives its Group.
///
/// Every other key stays: a Mac reserves none of them. Command+Shift+W, Command+H, Command+J, Command+Shift+4, F11, Command+Shift+C, Command+Shift+A,
/// Command+Shift+V, Command+Shift+P and Command+Shift+G are then left to the system there.
pub const MAC_KEYS: &[(&str, &str, &str)] = &[
    ("view.fitPage", "Cmd+Shift+W", "Cmd+0"),
    ("arrange.flip", "Cmd+H", "Cmd+Shift+H"),
    ("arrange.flip", "Cmd+J", "Cmd+Alt+J"),
    ("tool.text_block", "Cmd+Shift+4", "Cmd+Alt+4"),
    ("window.textDialog", "F11", "Cmd+T"),
    ("format.text", "Cmd+Shift+C", "Cmd+Shift+E"),
    ("format.size", "Cmd+Shift+.", "Cmd+Alt+."),
    ("format.size", "Cmd+Shift+,", "Cmd+Alt+,"),
    ("format.toggle", "Cmd+Shift+A", "Cmd+Alt+A"),
    ("format.text", "Cmd+Shift+V", "Cmd+Alt+B"),
    ("format.painter", "Cmd+Shift+P", "Cmd+Alt+C"),
    ("shape.group", "Cmd+Shift+G", "Cmd+Alt+G"),
];

/// Keys of the browser version only: the command and its key, written as the registry writes it. **Ctrl+W (Cmd+W on a Mac) closes the window**, as it does in a browser's tab and in an
/// installed web app's window, asking about unsaved changes first (`app.closeWindow`). **Ours**: no specification gives a key for closing in the reference application (Alt+F4 closes the active
/// window). The desktop window keeps no Ctrl+W: its frame has the close button and Alt+F4 is the system's.
pub const WEB_ALSO: &[(&str, &str)] = &[("app.closeWindow", "Cmd+W")];

/// The keys a browser keeps for itself in a tab and never passes to the page: the key as the registry writes it (`Cmd` is the platform's command key, Ctrl on Windows and Linux and Command on a Mac), and
/// what the browser does with it. **Measured** on Chrome 158 and Firefox 160 on Linux, with real key presses (`docs/website.md`, "Measured, "); the keys Chromium's own list
/// (`IsReservedCommandOrKey` of `chrome/browser/ui/browser_command_controller.cc`: new tab, new window, new incognito window, close tab, close window, restore tab, select next and previous tab, exit)
/// and Firefox's add are taken from them and from the browsers' own help, and **not measured on a Mac or on Windows**. In a window of its own, and in full screen with the keyboard locked, a
/// browser passes these keys to the page first.
pub const BROWSER_KEPT: &[(&str, &str)] = &[
    ("Cmd+N", "opens a new browser window"),
    ("Cmd+Shift+N", "opens a private browser window"),
    ("Cmd+T", "opens a new tab"),
    ("Cmd+Shift+T", "opens the tab that was closed last"),
    ("Cmd+W", "closes the tab"),
    ("Cmd+Shift+W", "closes the browser window"),
    ("Ctrl+Tab", "switches to the next tab"),
    ("Ctrl+Shift+Tab", "switches to the previous tab"),
    ("Ctrl+PageDown", "switches to the next tab"),
    ("Ctrl+PageUp", "switches to the previous tab"),
    ("Cmd+Q", "quits the browser"),
    ("Ctrl+Shift+Q", "quits the browser"),
];

/// Whether a browser keeps `shortcut` for itself in a tab ([`BROWSER_KEPT`]): it never reaches the page there, and a command on it needs a window of its own, or full screen with the keyboard locked.
pub fn kept_by_browser(os: OperatingSystem, shortcut: &KeyboardShortcut) -> bool {
    BROWSER_KEPT.iter().any(|(spec, _)| parse(spec).is_some_and(|kept| same_key(os, &kept, shortcut)))
}

/// What a browser does with `shortcut` in a tab, if it keeps it ([`BROWSER_KEPT`]).
pub fn browser_does(os: OperatingSystem, shortcut: &KeyboardShortcut) -> Option<&'static str> {
    BROWSER_KEPT.iter().find(|(spec, _)| parse(spec).is_some_and(|kept| same_key(os, &kept, shortcut))).map(|(_, does)| *does)
}

/// The modifiers as the platform reads them, as (Command, Control, Shift, Alt): on a Mac `Cmd` is the Command key and `Ctrl` the Control key; elsewhere `Cmd` and `Ctrl` are the one key.
fn modifier_word(os: OperatingSystem, m: Modifiers) -> (bool, bool, bool, bool) {
    let (command, control) = if os == OperatingSystem::Mac { (m.command || m.mac_cmd, m.ctrl) } else { (false, m.ctrl || m.command) };
    (command, control, m.shift, m.alt)
}

/// Whether two shortcuts are the one key press: the key, and the modifiers as the platform reads them (`Cmd` is Ctrl off a Mac).
fn same_key(os: OperatingSystem, a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    a.logical_key == b.logical_key && modifier_word(os, a.modifiers) == modifier_word(os, b.modifiers)
}

/// Whether the window would run the command of `shortcut` for the key press `press`: **as `pressed` decides it**, which is egui's own logical match. The command key must be the one the shortcut
/// has; Shift and Alt the shortcut has must be held, and **an extra Shift or Alt does not stop it** (egui lets Shift be extra so that `Ctrl+=` is also the Shift that a keyboard needs for `=`:
/// on a Portuguese layout the key that types `=` is Shift and 0, and a press of it arrives as Ctrl+Shift with the key `=`), **except that a letter's shortcut with no Shift in it is not run
/// with Shift held** (so Ctrl+Shift+N is no Ctrl+N). The page takes a press that the window would run, so that the browser does not also act on it.
fn window_would_run(os: OperatingSystem, shortcut: &KeyboardShortcut, press: &KeyboardShortcut) -> bool {
    if shortcut.logical_key != press.logical_key {
        return false;
    }
    let (sc, sk, ss, sa) = modifier_word(os, shortcut.modifiers);
    let (pc, pk, ps, pa) = modifier_word(os, press.modifiers);
    let name = shortcut.logical_key.name();
    let letter = name.len() == 1 && name.bytes().all(|b| b.is_ascii_alphabetic());
    (sc, sk) == (pc, pk) && (!ss || ps) && !(letter && !ss && ps) && (!sa || pa)
}

/// The key press of a browser's `keydown` event as a shortcut on `os`: `key` and `code` are the event's `key` and `code`, then its `ctrlKey`, `metaKey`, `shiftKey` and `altKey`. `None` for a
/// key that has no name in egui, and for a press with the Windows or Super key held off a Mac (it is not ours).
pub fn press_of(os: OperatingSystem, key: &str, code: &str, ctrl: bool, meta: bool, shift: bool, alt: bool) -> Option<KeyboardShortcut> {
    let mac = os == OperatingSystem::Mac;
    if meta && !mac {
        return None;
    }
    let key = Key::from_name(key).or_else(|| Key::from_name(code))?;
    Some(KeyboardShortcut::new(Modifiers { alt, ctrl, shift, mac_cmd: meta, command: if mac { meta } else { ctrl } }, key))
}

/// Whether `press` is one of `taken` ([`keys_to_take`]), or a press that the window would run the command of one of them for (an extra Shift on a key that needs it: [`window_would_run`]).
pub fn is_taken(os: OperatingSystem, taken: &[KeyboardShortcut], press: &KeyboardShortcut) -> bool {
    taken.iter().any(|t| window_would_run(os, t, press))
}

/// The key presses the page of the browser version takes from the browser: for the system a browser's user agent names, the keys of [`keys_to_take`], and which `keydown` events are
/// one of them. The page asks it of each `keydown` in the capture phase and calls `preventDefault()` on the ones it takes, so that the browser does not also act on a key it passed to the page
/// (a window of its own, or full screen with the keyboard locked); the window runs the command from the same event as for any other key.
pub struct KeyTaker {
    os: OperatingSystem,
    taken: Vec<KeyboardShortcut>,
}

impl KeyTaker {
    /// The keys for the system `user_agent` names: the one egui's own web runner takes the window's system from.
    pub fn for_user_agent(user_agent: &str) -> KeyTaker {
        KeyTaker::for_os(OperatingSystem::from_user_agent(user_agent))
    }

    fn for_os(os: OperatingSystem) -> KeyTaker {
        KeyTaker { os, taken: keys_to_take(os) }
    }

    /// How many key presses are taken.
    pub fn count(&self) -> usize {
        self.taken.len()
    }

    /// Whether the page takes this `keydown`: the event's `key` and `code`, then its `ctrlKey`, `metaKey`, `shiftKey` and `altKey`.
    pub fn takes(&self, key: &str, code: &str, ctrl: bool, meta: bool, shift: bool, alt: bool) -> bool {
        press_of(self.os, key, code, ctrl, meta, shift, alt).is_some_and(|press| is_taken(self.os, &self.taken, &press))
    }
}

/// Every key press a command of the window has on `os` (`web` adds the keys of the browser version).
pub fn bound_keys(os: OperatingSystem, web: bool) -> Vec<KeyboardShortcut> {
    shortcuts_for(os, web).into_iter().map(|b| b.shortcut).collect()
}

/// The key press that runs command `id` on `os`, if it has one (the first binding of it; `web` adds the keys of the browser version). `id` is the engine's own id or the window's.
pub fn shortcut_of(os: OperatingSystem, id: &str, web: bool) -> Option<KeyboardShortcut> {
    shortcuts_for(os, web).into_iter().find(|b| b.id == window_id_of(id)).map(|b| b.shortcut)
}

fn window_id_of(id: &str) -> &str {
    ENGINE_KEYS_FOR_THE_WINDOW.iter().find(|(engine, ..)| *engine == id).map_or(id, |(_, window, _)| *window)
}

/// Whether the page takes `shortcut` from the browser, if a command of the window has it: **every chord of the command key** (Ctrl, or Command on a Mac) **and every function key**, **except the
/// clipboard's own** (Ctrl+C, Ctrl+X and Ctrl+V with nothing else held: they are the browser's copy, cut and paste events, which the web runner turns into egui's, and a `preventDefault()` on their
/// keys would stop them). **Why more than the keys a browser keeps in a tab ([`BROWSER_KEPT`]):** a key a tab passes to the page is acted on by the browser too, unless the page prevents it, and
/// eframe's web runner prevents only Ctrl+O, Ctrl+P and Ctrl+S, the arrows, Tab, Backspace and Space. Measured with real key presses in Chrome 158 and Firefox 160 (`docs/website.md`, "Measured, "):
/// before this, **Ctrl+R (Rotate Right) reloaded the page and lost the drawing**, Ctrl+U (Underline) opened the page's source in a tab, Ctrl+H (Flip) the history, Ctrl+J (Flip Vertical) the
/// downloads, Ctrl+F (Find) the browser's find bar, Ctrl+D (Duplicate) the bookmark box, and Ctrl+L, Ctrl+K and Ctrl+G moved the keyboard to the address bar or the browser's find; and Ctrl+- (Zoom
/// Out) zoomed the browser's page as well as the drawing.
fn page_takes(shortcut: &KeyboardShortcut) -> bool {
    let m = shortcut.modifiers;
    let chord = m.command || m.ctrl || m.mac_cmd;
    let function = matches!(
        shortcut.logical_key,
        Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12
    );
    let clipboard = matches!(shortcut.logical_key, Key::C | Key::X | Key::V) && chord && !m.shift && !m.alt;
    (chord || function) && !clipboard
}

/// The keys the page of the browser version must take, so that a press the browser would act on does nothing in the browser and runs the window's command: every key press that a command of the
/// window has on `os` ([`WEB_ALSO`] included) that [`page_takes`] says the page takes. The browser passes them to the page (in a window of its own, in full screen with the keyboard locked, and,
/// for all but the keys of [`BROWSER_KEPT`], in a tab), and the page calls `preventDefault()` on each (`apps/diacraft-web/src/web_page.rs`).
pub fn keys_to_take(os: OperatingSystem) -> Vec<KeyboardShortcut> {
    let mut taken: Vec<KeyboardShortcut> = shortcuts_for(os, true).into_iter().map(|b| b.shortcut).filter(page_takes).collect();
    taken.dedup();
    taken
}

/// The key a command has on `os`: its own, or the Mac's ([`MAC_KEYS`]). `id` is the command's own id (the engine's for an engine command, before the
/// window's stands in for it).
pub fn key_on(os: OperatingSystem, id: &str, spec: &'static str) -> &'static str {
    if os != OperatingSystem::Mac {
        return spec;
    }
    MAC_KEYS.iter().find(|(command, elsewhere, _)| *command == id && *elsewhere == spec).map_or(spec, |(_, _, mac)| *mac)
}

/// A key that runs a window command on the selection, with the parameters the key stands for.
///
/// The engine has no shortcut for the format and arrange commands: one command with a parameter does the work of several keys, so the
/// window maps them. **Where each key comes from** is in `source`: a line of the feature inventories that names it, or "ours" for a key no
/// specification gives. Where a specification names the keys in the order of the commands (Rotate Left, Rotate Right, Flip Horizontal and Flip
/// Vertical, "also on Ctrl+L, Ctrl+R, Ctrl+H and Ctrl+J"), the letters are matched to the commands in that order: **assumed**, since the page
/// names no letter for a command.
pub struct WindowKey {
    pub shortcut: &'static str,
    pub command: &'static str,
    pub params: &'static str,
    /// Which line of the specifications names the key, or "ours".
    pub source: &'static str,
    /// The key also works while a shape's text box is open: it formats the characters selected in it.
    pub in_text_box: bool,
}

const fn key(shortcut: &'static str, command: &'static str, params: &'static str, source: &'static str, in_text_box: bool) -> WindowKey {
    WindowKey { shortcut, command, params, source, in_text_box }
}

/// The keys of the window. Ctrl+Shift+> and Ctrl+Shift+< are written with the keys they are on, the full stop and the comma, as egui names
/// them (the symbol itself has no key of its own; see [`pressed`]).
pub const WINDOW_KEYS: &[WindowKey] = &[
    key("Cmd+B", "format.toggle", r#"{"what":"bold"}"#, "inventory-shapes: Bold, italic and underline", true),
    key("Cmd+I", "format.toggle", r#"{"what":"italic"}"#, "inventory-shapes: Bold, italic and underline", true),
    key("Cmd+U", "format.toggle", r#"{"what":"underline"}"#, "inventory-shapes: Bold, italic and underline", true),
    key("Cmd+Shift+.", "format.size", r#"{"step":"up"}"#, "inventory-shapes: Font and size", true),
    key("Cmd+Shift+,", "format.size", r#"{"step":"down"}"#, "inventory-shapes: Font and size", true),
    key("Cmd+Shift+L", "format.text", r#"{"align":"left"}"#, "inventory-shapes: Horizontal paragraph alignment", true),
    key("Cmd+Shift+C", "format.text", r#"{"align":"center"}"#, "inventory-shapes: Horizontal paragraph alignment", true),
    key("Cmd+Shift+R", "format.text", r#"{"align":"right"}"#, "inventory-shapes: Horizontal paragraph alignment", true),
    key("Cmd+Shift+J", "format.text", r#"{"align":"justify"}"#, "inventory-shapes: Horizontal paragraph alignment", true),
    // The two cases that have a key: each is on or off, as the Case box has them one at a time.
    key("Cmd+Shift+A", "format.toggle", r#"{"what":"all_caps"}"#, "a-home-font-paragraph R10: Ctrl+Shift+A", true),
    key("Cmd+Shift+K", "format.toggle", r#"{"what":"small_caps"}"#, "a-home-font-paragraph R10: Ctrl+Shift+K", true),
    key("Cmd+Shift+T", "format.text", r#"{"valign":"top"}"#, "inventory-shapes: Vertical text alignment", true),
    key("Cmd+Shift+M", "format.text", r#"{"valign":"middle"}"#, "inventory-shapes: Vertical text alignment", true),
    key("Cmd+Shift+V", "format.text", r#"{"valign":"bottom"}"#, "inventory-shapes: Vertical text alignment", true),
    key("Cmd+L", "arrange.rotate", r#"{"by":90}"#, "inventory-shapes: Rotate 90 and flip", false),
    key("Cmd+R", "arrange.rotate", r#"{"by":-90}"#, "inventory-shapes: Rotate 90 and flip", false),
    key("Cmd+H", "arrange.flip", r#"{"axis":"horizontal"}"#, "inventory-shapes: Rotate 90 and flip", false),
    key("Cmd+J", "arrange.flip", r#"{"axis":"vertical"}"#, "inventory-shapes: Rotate 90 and flip", false),
    key("F3", "window.pane", r#"{"pane":"format_shape","shown":true}"#, "inventory-styling: Format Shape pane with live preview", false),
    key("F11", "window.textDialog", r#"{"tab":"Font"}"#, "inventory-shapes: Font and size", true),
    key("Alt+F9", "window.snapGlue", r#"{"tab":"General"}"#, "inventory-document: Snap on or off (Alt+F9 opens the dialog)", false),
    key("Cmd+Shift+F", "arrange.order", r#"{"to":"front"}"#, "inventory-shapes: Bring to front and send to back", false),
    key("Cmd+Shift+B", "arrange.order", r#"{"to":"back"}"#, "inventory-shapes: Bring to front and send to back", false),
];

/// The engine commands that have a key of their own (registered them) and that the window runs through its own commands, because they take the
/// shapes selected: the engine's id, the window's, and where the key comes from. **Ctrl+C, Ctrl+X, Ctrl+V** are the platform's keys, and ours
/// (names the commands and no key); **Ctrl+D** is named for the web only ("Duplicate command"), so its
/// desktop key is ours; **Ctrl+Shift+G and Ctrl+Shift+U** are the inventory's ("Group and ungroup").
pub const ENGINE_KEYS_FOR_THE_WINDOW: &[(&str, &str, &str)] = &[
    ("edit.copy", "clipboard.copy", "ours: the platform's key"),
    ("edit.cut", "clipboard.cut", "ours: the platform's key"),
    ("edit.paste", "clipboard.paste", "ours: the platform's key"),
    ("shape.duplicate", "clipboard.duplicate", "ours: named for the web only"),
    ("shape.group", "arrange.group", "inventory-shapes: Group and ungroup"),
    ("shape.ungroup", "arrange.ungroup", "inventory-shapes: Group and ungroup"),
];

/// The window command that stands for the engine command `id` when its key is pressed.
fn window_id(id: &'static str) -> &'static str {
    ENGINE_KEYS_FOR_THE_WINDOW.iter().find(|(engine, ..)| *engine == id).map_or(id, |(_, window, _)| *window)
}

/// A key to press: the command it runs, the parameters it runs it with, and the key.
struct Binding {
    id: &'static str,
    params: &'static str,
    shortcut: KeyboardShortcut,
    in_text_box: bool,
}

/// Every command that has a shortcut on `os`, the engine's and the window's, then the keys of [`WINDOW_KEYS`], with the ones with the most
/// modifiers first, so that `Cmd+Shift+Z` is taken before `Cmd+Z` can claim the same key.
#[cfg(test)]
fn shortcuts(os: OperatingSystem) -> Vec<Binding> {
    shortcuts_for(os, false)
}

/// [`shortcuts`], and where `web` the keys of the browser version too ([`WEB_ALSO`]).
fn shortcuts_for(os: OperatingSystem, web: bool) -> Vec<Binding> {
    let engine = command_specs().iter().filter_map(|c| c.shortcut.map(|s| (c.id, s)));
    let window = UI_COMMANDS.iter().filter_map(|c| c.shortcut.map(|s| (c.id, s)));
    let also = ALSO.iter().map(|(id, s)| (*id, *s));
    let mac_also = MAC_ALSO.iter().filter(|_| os == OperatingSystem::Mac).map(|(id, s)| (*id, *s));
    let web_also = WEB_ALSO.iter().filter(|_| web).map(|(id, s)| (*id, *s));
    let mut all: Vec<Binding> = engine
        .chain(window)
        .chain(also)
        .chain(mac_also)
        .chain(web_also)
        .filter_map(|(id, s)| parse(key_on(os, id, s)).map(|shortcut| Binding { id: window_id(id), params: "", shortcut, in_text_box: false }))
        .collect();
    all.extend(WINDOW_KEYS.iter().filter_map(|k| {
        parse(key_on(os, k.command, k.shortcut)).map(|shortcut| Binding { id: k.command, params: k.params, shortcut, in_text_box: k.in_text_box })
    }));
    let count = |m: &Modifiers| [m.alt, m.ctrl, m.shift, m.command, m.mac_cmd].into_iter().filter(|b| *b).count();
    all.sort_by_key(|b| std::cmp::Reverse(count(&b.shortcut.modifiers)));
    all
}

/// Whether `shortcut` was pressed in this frame, and takes the press. **A key the layout types with Shift held** (`>` on the full stop's key, `<`
/// on the comma's) comes from the desktop as the symbol, which egui has no key for, with the physical key beside it: the physical key stands in.
fn pressed(ctx: &egui::Context, shortcut: &KeyboardShortcut) -> bool {
    ctx.input_mut(|i| {
        // **A letter's shortcut with no Shift in it is not pressed with Shift**: egui's own match lets Shift be extra (so `Ctrl+=` is `Ctrl+Shift+=` on a
        // keyboard that needs Shift for it), and then Ctrl+Shift+U, which a text box leaves to itself, would be taken by the Ctrl+U that a text box allows.
        let letter = shortcut.logical_key.name().len() == 1 && shortcut.logical_key.name().bytes().all(|b| b.is_ascii_alphabetic());
        if letter && !shortcut.modifiers.shift {
            let at = i.events.iter().position(|e| {
                matches!(e, Event::Key { key, pressed: true, modifiers, .. }
                    if *key == shortcut.logical_key && !modifiers.shift && modifiers.matches_logically(shortcut.modifiers))
            });
            return match at {
                Some(n) => {
                    i.events.remove(n);
                    true
                }
                None => false,
            };
        }
        if i.consume_shortcut(shortcut) {
            return true;
        }
        if !matches!(shortcut.logical_key, Key::Period | Key::Comma) {
            return false;
        }
        let at = i.events.iter().position(|e| {
            matches!(e, Event::Key { physical_key: Some(k), pressed: true, modifiers, .. }
                if *k == shortcut.logical_key && modifiers.matches_logically(shortcut.modifiers))
        });
        match at {
            Some(n) => {
                i.events.remove(n);
                true
            }
            None => false,
        }
    })
}

impl DiacraftApp {
    /// Runs the command of each shortcut pressed in this frame.
    ///
    /// **Which keys are free** (L6 of the M3 review). With a question on the screen or a file being read, none: the keys are the card's.
    /// With the File view open or a text box open, only the shortcuts of the commands on files (New, Open, Save, Save As, and Close in the browser version): the rest would
    /// change a drawing the person is not looking at (Delete under the File view), or are the box's (Ctrl+Z in a text box undoes typing,
    /// not the drawing). A label being typed is sent when New, Open, Save or Save As run (M1).
    pub(crate) fn run_shortcuts(&mut self, ctx: &egui::Context) {
        if self.is_modal_open() {
            return;
        }
        let editing = self.ui.edit.editing_text();
        let backstage = self.ui.backstage.is_some();
        // While the ShapeSheet has the focus its keys are its own: Delete, Ctrl+A, Ctrl+D and the rest would change a drawing the person is not looking at. Files, the window's
        // own commands and views, and undo and redo (the drawing's, whatever made the step) stay.
        let sheet = self.sheet_keys_held();
        // A text field other than the shape's own box has the keyboard (the Font list's, the Size list's, the box for a colour), **with the shape's box
        // open or not**: its keys are its own.
        let other_box = self.shape_filter_keys_held() || (ctx.text_edit_focused() && ctx.memory(|m| m.focused()) != self.text_box_field());
        if !backstage && !other_box && !editing && !ctx.text_edit_focused() {
            self.run_clipboard_events(ctx);
        }
        for binding in shortcuts_for(ctx.os(), self.in_browser()) {
            let id = binding.id;
            // Close joins the commands on files (the browser version's Ctrl+W): it works in the File view and in a text box, where the label being typed is sent first.
            let files = id.starts_with("file.") || id == "app.closeWindow";
            let sheet_allows = !sheet || crate::sheet::shortcut_allowed(id);
            let allowed = files || (!backstage && !other_box && sheet_allows && (!editing || binding.in_text_box));
            if !allowed {
                continue;
            }
            // A command that cannot run now is quiet.
            if pressed(ctx, &binding.shortcut) && self.command_enabled(id).is_ok() {
                self.run_shortcut_command(ctx, id, binding.params);
                ctx.request_repaint();
            }
        }
    }

    /// Ctrl+C, Ctrl+X and Ctrl+V from a desktop. **egui-winit turns them into `Event::Copy`, `Event::Cut` and `Event::Paste` and sends no key**, so the
    /// table of keys never sees them: this takes the events (when no text field has the keyboard, which would want them for its text) and runs the
    /// commands. The text a paste event carries is the system clipboard's, and is not used: a paste is the session's clipboard (`clipboard.rs`).
    fn run_clipboard_events(&mut self, ctx: &egui::Context) {
        let (mut copy, mut cut, mut paste) = (false, false, false);
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                match e {
                    Event::Copy => copy = true,
                    Event::Cut => cut = true,
                    Event::Paste(_) => paste = true,
                    _ => return true,
                }
                false
            });
        });
        for (asked, id) in [(copy, "clipboard.copy"), (cut, "clipboard.cut"), (paste, "clipboard.paste")] {
            if asked && self.command_enabled(id).is_ok() {
                self.run_shortcut_command(ctx, id, "");
                ctx.request_repaint();
            }
        }
    }

    /// What a shortcut does for the command `id`. Some commands take what only the window knows: a path (the dialogs ask for it), the
    /// selection, a question about unsaved changes first.
    fn run_shortcut_command(&mut self, ctx: &egui::Context, id: &str, params: &str) {
        match id {
            "file.open" => self.pick_and_open(ctx),
            "file.new" => self.ask_then(ctx, crate::save::Then::New),
            "file.save" => self.save(ctx),
            "file.save_as" => self.save_as(ctx),
            // Ctrl+P shows the Print view (the engine's `file.print` writes the PDF and has no view to show).
            "file.print" => self.open_print_view(),
            "shape.delete" => self.delete_selection(),
            _ => {
                // The parameters of a window key are written in the table above; a table entry that is not JSON is a bug the test finds.
                let params: Value = if params.is_empty() { json!({}) } else { serde_json::from_str(params).unwrap_or_else(|_| json!({})) };
                if let Err(e) = self.execute(id, &params) {
                    self.set_notice(&e.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_strings_become_shortcuts() {
        assert_eq!(parse("Cmd+Z"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)));
        assert_eq!(parse("Cmd+Shift+Z"), Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)));
        assert_eq!(parse("Cmd+="), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals)));
        assert_eq!(parse("Cmd+-"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus)));
        assert_eq!(parse("Cmd+Nonsense"), None);
        assert_eq!(parse("Z+Cmd"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn a_second_shortcut_belongs_to_a_registered_command() {
        for (id, spec) in ALSO {
            assert!(diacraft_engine::find_command(id).is_some() || crate::commands::find_ui_command(id).is_some(), "{id}");
            assert!(parse(spec).is_some(), "{spec}");
        }
    }

    const SYSTEMS: [OperatingSystem; 3] = [OperatingSystem::Mac, OperatingSystem::Windows, OperatingSystem::Nix];

    #[test]
    fn every_registered_shortcut_can_be_pressed_and_none_is_taken_twice() {
        for os in SYSTEMS {
            let all = shortcuts(os);
            let with: usize = command_specs().iter().filter(|c| c.shortcut.is_some()).count()
                + UI_COMMANDS.iter().filter(|c| c.shortcut.is_some()).count()
                + ALSO.len()
                + if os == OperatingSystem::Mac { MAC_ALSO.len() } else { 0 }
                + WINDOW_KEYS.len();
            assert_eq!(all.len(), with, "{os:?}: a registered shortcut does not parse");
            for (i, b) in all.iter().enumerate() {
                assert!(
                    all.iter().skip(i + 1).all(|other| other.shortcut != b.shortcut),
                    "{os:?}: {} shares its shortcut with another command",
                    b.id
                );
            }
        }
    }

    /// A Mac key stands in for a key the command has elsewhere (so that it cannot go stale when that key moves), and only on a Mac. Fit to
    /// Window is Command+0 there, and no command takes Command+Shift+W, the system's Close a file and its associated windows.
    #[test]
    fn a_mac_key_replaces_a_key_the_command_has_and_only_on_a_mac() {
        let everywhere: Vec<(&str, &str)> = command_specs()
            .iter()
            .filter_map(|c| c.shortcut.map(|s| (c.id, s)))
            .chain(UI_COMMANDS.iter().filter_map(|c| c.shortcut.map(|s| (c.id, s))))
            .chain(ALSO.iter().copied())
            .chain(WINDOW_KEYS.iter().map(|k| (k.command, k.shortcut)))
            .collect();
        // One entry for each key it replaces: a second would never be read.
        for (i, (id, elsewhere, _)) in MAC_KEYS.iter().enumerate() {
            assert!(MAC_KEYS.iter().skip(i + 1).all(|(other, key, _)| (other, key) != (id, elsewhere)), "{id} {elsewhere} is replaced twice");
        }
        for (id, elsewhere, mac) in MAC_KEYS {
            assert!(everywhere.contains(&(*id, *elsewhere)), "{id} has no key {elsewhere} for the Mac's {mac} to replace");
            assert!(parse(mac).is_some(), "{mac}");
            assert_eq!(key_on(OperatingSystem::Mac, id, elsewhere), *mac);
            assert_eq!(key_on(OperatingSystem::Windows, id, elsewhere), *elsewhere);
            assert_eq!(key_on(OperatingSystem::Nix, id, elsewhere), *elsewhere);
        }
        let close_file = parse("Cmd+Shift+W");
        assert!(shortcuts(OperatingSystem::Mac).iter().all(|b| Some(b.shortcut) != close_file), "Command+Shift+W is the system's on a Mac");
        let fit = |os| shortcuts(os).into_iter().filter(|b| b.id == "view.fitPage").map(|b| b.shortcut).collect::<Vec<_>>();
        assert_eq!(fit(OperatingSystem::Mac), [KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0)]);
        assert_eq!(fit(OperatingSystem::Windows), [KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::W)]);
    }

    /// The browser version's own keys: Ctrl+W (Command+W on a Mac) closes the window, bound only where there is a page, and clashing with no other key on any system. On a Mac it
    /// is the key the Mac gives to Close Window, and the command does that very thing (allows it).
    #[test]
    fn the_web_keys_are_bound_on_the_web_only_and_clash_with_nothing() {
        for os in SYSTEMS {
            let desktop = shortcuts_for(os, false);
            let web = shortcuts_for(os, true);
            assert_eq!(web.len(), desktop.len() + WEB_ALSO.len(), "{os:?}");
            assert!(desktop.iter().all(|b| b.id != "app.closeWindow"), "{os:?}: the desktop window has no Ctrl+W");
            for (i, b) in web.iter().enumerate() {
                assert!(
                    web.iter().skip(i + 1).all(|other| other.shortcut != b.shortcut),
                    "{os:?}: {} shares its shortcut with another command",
                    b.id
                );
            }
            let close: Vec<_> = web.iter().filter(|b| b.id == "app.closeWindow").map(|b| b.shortcut).collect();
            assert_eq!(close, [KeyboardShortcut::new(Modifiers::COMMAND, Key::W)], "{os:?}");
        }
        for (id, spec) in WEB_ALSO {
            assert!(crate::commands::find_ui_command(id).is_some(), "{id}");
            assert!(parse(spec).is_some(), "{spec}");
        }
        let mac_close = MAC_RESERVED.iter().find(|(key, ..)| *key == "Cmd+W").expect("the Mac's own Close Window is listed");
        assert_eq!(mac_close.1, "close the window", "the key does on the web what the Mac does with it");
    }

    /// A key a Mac reserves: the key, what the Mac does with it, and the binding (command and parameters) that may have it.
    type Reserved = (&'static str, &'static str, Option<(&'static str, &'static str)>);

    /// The keys a Mac gives to something before an app or in every app: the key as the registry writes it, what the Mac does with it, and
    /// the window's binding (command and parameters) that may have it because it does that very thing, or `None`. From Apple's Human Interface Guidelines
    /// ("Keyboards", the table of standard keyboard shortcuts, read 2026-11-23), Apple's "Mac keyboard shortcuts" (support.apple.com/102650, read
    /// 2026-11-23: the system's own keys, not the Finder's, which are the Finder's only) and winit's app menu (`platform_impl/macos/menu.rs`: Hide,
    /// Hide Others and Quit). Command-Hyphen and Command-Equal ("decrease" and "increase the size of the selection") zoom the drawing out and in, as
    /// in every sibling craft app (kept them beside Fit to Window). The Eject key has no key in egui and is left out.
    const MAC_RESERVED: &[Reserved] = &[
        ("Cmd+Space", "Spotlight", None),
        ("Cmd+Shift+Space", "ask Siri about the window", None),
        ("Cmd+Alt+Space", "the Spotlight results window", None),
        ("Ctrl+Cmd+Space", "the Special Characters window", None),
        ("Shift+Tab", "the controls in reverse", None),
        ("Cmd+Tab", "the next app", None),
        ("Cmd+Shift+Tab", "the previous app", None),
        ("Ctrl+Tab", "the next group of controls", None),
        ("Ctrl+Shift+Tab", "the previous group of controls", None),
        ("Escape", "cancel", None),
        ("Cmd+Alt+Escape", "Force Quit", None),
        ("Ctrl+F1", "full keyboard access", None),
        ("Ctrl+F2", "the menu bar", None),
        ("Ctrl+F3", "the Dock", None),
        ("Ctrl+F4", "the active window", None),
        ("Ctrl+Shift+F4", "the window before", None),
        ("Ctrl+F5", "the toolbar", None),
        ("Cmd+F5", "VoiceOver", None),
        ("Cmd+Alt+F5", "the Accessibility Shortcuts panel", None),
        ("Ctrl+F6", "the next panel", None),
        ("Ctrl+Shift+F6", "the panel before", None),
        ("Ctrl+F7", "the keyboard access mode", None),
        ("Ctrl+F8", "the status menu", None),
        ("F11", "Show Desktop", None),
        ("F12", "Dashboard", None),
        ("Cmd+`", "the next window of the app", None),
        ("Cmd+Shift+`", "the window before", None),
        ("Cmd+Alt+`", "the window drawer", None),
        ("Cmd+-", "the selection smaller", Some(("view.zoomOut", ""))),
        ("Cmd+Shift+-", "the selection smaller", None),
        ("Cmd+Alt+-", "zoom the screen out", None),
        ("Cmd+=", "the selection larger", Some(("view.zoomIn", ""))),
        ("Cmd+Shift+=", "the selection larger", None),
        ("Cmd+Alt+=", "zoom the screen in", None),
        ("Cmd+{", "left-align", None),
        ("Cmd+}", "right-align", None),
        // The Mac's own center key; no common Mac layout but Latin American can press it in this window, so Center is Shift+Command+E.
        ("Cmd+|", "center", None),
        ("Cmd+:", "the Spelling window", None),
        ("Cmd+;", "find misspelled words", None),
        ("Cmd+,", "the app's settings", None),
        ("Ctrl+Alt+Cmd+,", "less screen contrast", None),
        ("Cmd+.", "cancel an operation", None),
        ("Ctrl+Alt+Cmd+.", "more screen contrast", None),
        ("Cmd+?", "the Help menu", None),
        ("Cmd+Alt+/", "font smoothing", None),
        ("Cmd+Shift+3", "a screenshot of the screen", None),
        ("Ctrl+Cmd+Shift+3", "a screenshot of the screen to the clipboard", None),
        ("Cmd+Shift+4", "a screenshot of a part of the screen", None),
        ("Ctrl+Cmd+Shift+4", "a screenshot of a part of the screen to the clipboard", None),
        ("Cmd+Shift+5", "the screenshot and recording tools", None),
        ("Cmd+Shift+6", "ask Siri about a part of the screen", None),
        ("Cmd+Alt+8", "screen zoom", None),
        ("Ctrl+Alt+Cmd+8", "inverted colours", None),
        ("Cmd+A", "select all", Some(("edit.selectAll", ""))),
        ("Cmd+Shift+A", "deselect all", None),
        ("Cmd+B", "bold", Some(("format.toggle", r#"{"what":"bold"}"#))),
        ("Cmd+C", "copy", Some(("clipboard.copy", ""))),
        ("Cmd+Shift+C", "the Colors window", None),
        ("Cmd+Alt+C", "copy the style of the selection", Some(("format.painter", ""))),
        ("Ctrl+Cmd+C", "copy the formatting of the selection", None),
        ("Cmd+Alt+D", "the Dock", None),
        ("Ctrl+Cmd+D", "the definition of a word", None),
        ("Cmd+E", "find the selection", None),
        ("Cmd+F", "find", Some(("window.find", ""))),
        ("Cmd+Alt+F", "the search field", None),
        ("Ctrl+Cmd+F", "full screen", None),
        ("Cmd+G", "find the next one", None),
        ("Cmd+Shift+G", "find the one before", None),
        ("Cmd+H", "hide the app (the app menu's Hide)", None),
        ("Cmd+Alt+H", "hide the other apps (the app menu's Hide Others)", None),
        ("Cmd+I", "italic", Some(("format.toggle", r#"{"what":"italic"}"#))),
        ("Cmd+Alt+I", "the inspector", None),
        ("Cmd+J", "scroll to the selection", None),
        ("Cmd+K", "add a link", Some(("window.hyperlinks", ""))),
        ("Cmd+M", "minimize the window", None),
        ("Cmd+Alt+M", "minimize the app's windows", None),
        ("Cmd+N", "a new document", Some(("file.new", ""))),
        ("Cmd+O", "open a document", Some(("file.open", ""))),
        ("Cmd+P", "print", Some(("file.print", ""))),
        ("Cmd+Shift+P", "Page Setup", None),
        ("Cmd+Q", "quit (the app menu's Quit)", None),
        ("Cmd+Shift+Q", "log out", None),
        ("Cmd+Alt+Shift+Q", "log out at once", None),
        ("Ctrl+Cmd+Q", "lock the screen", None),
        ("Cmd+S", "save", Some(("file.save", ""))),
        ("Cmd+Shift+S", "save as", Some(("file.save_as", ""))),
        ("Cmd+T", "the Fonts window", Some(("window.textDialog", r#"{"tab":"Font"}"#))),
        ("Cmd+Alt+T", "the toolbar", None),
        ("Cmd+U", "underline", Some(("format.toggle", r#"{"what":"underline"}"#))),
        ("Cmd+V", "paste", Some(("clipboard.paste", ""))),
        ("Cmd+Shift+V", "paste as", None),
        ("Cmd+Alt+V", "apply a style to the selection", None),
        ("Cmd+Alt+Shift+V", "paste and match the style", None),
        ("Ctrl+Cmd+V", "apply formatting to the selection", None),
        ("Cmd+W", "close the window", None),
        ("Cmd+Shift+W", "close the file and its windows", None),
        ("Cmd+Alt+W", "close the app's windows", None),
        ("Cmd+X", "cut", Some(("clipboard.cut", ""))),
        ("Cmd+Z", "undo", Some(("edit.undo", ""))),
        ("Cmd+Shift+Z", "redo", Some(("edit.redo", ""))),
        ("Cmd+ArrowLeft", "the keyboard layout of the system's script", None),
        ("Cmd+ArrowRight", "the keyboard layout of the Roman script", None),
        ("Ctrl+ArrowUp", "Mission Control", None),
        ("Ctrl+ArrowDown", "the app's windows", None),
        ("Ctrl+ArrowLeft", "the value or cell before", None),
        ("Ctrl+ArrowRight", "the next value or cell", None),
        ("Ctrl+Space", "the input source before", None),
        ("Ctrl+Alt+Space", "the next input source", None),
    ];

    /// No key of the Mac's table is one the Mac reserves, unless the key does there what the Mac does with it. Command+H (the app menu's
    /// Hide), Command+Shift+4 (a screenshot) and F11 (Show Desktop) were the window's, and six more by convention; each now has a Mac key of its own.
    #[test]
    fn no_mac_key_is_one_the_mac_or_the_app_menu_reserves() {
        let mut taken = Vec::new();
        for (key, what, allowed) in MAC_RESERVED {
            let reserved = parse(key).unwrap_or_else(|| panic!("{key} does not parse"));
            for b in shortcuts(OperatingSystem::Mac).iter().filter(|b| b.shortcut == reserved) {
                if *allowed != Some((b.id, b.params)) {
                    taken.push(format!("{key} ({what}): {} {}", b.id, b.params));
                }
            }
        }
        assert!(taken.is_empty(), "keys a Mac reserves:\n{}", taken.join("\n"));
        // Each binding that may have a key of the list has it on a Mac, so that the list cannot name a binding that is gone.
        let mac = shortcuts(OperatingSystem::Mac);
        for (key, _, allowed) in MAC_RESERVED {
            if let Some((id, params)) = allowed {
                let reserved = parse(key);
                assert!(mac.iter().any(|b| Some(b.shortcut) == reserved && b.id == *id && b.params == *params), "{key} is no longer {id} {params}");
            }
        }
        // Elsewhere the keys stay as they were.
        let flip = |os| shortcuts(os).into_iter().filter(|b| b.id == "arrange.flip").map(|b| (b.params, b.shortcut)).collect::<Vec<_>>();
        let (h, v) = (r#"{"axis":"horizontal"}"#, r#"{"axis":"vertical"}"#);
        let cmd = Modifiers::COMMAND;
        assert_eq!(flip(OperatingSystem::Windows), [(h, KeyboardShortcut::new(cmd, Key::H)), (v, KeyboardShortcut::new(cmd, Key::J))]);
        assert_eq!(
            flip(OperatingSystem::Mac),
            [(h, KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::H)), (v, KeyboardShortcut::new(cmd | Modifiers::ALT, Key::J))]
        );
    }

    /// The keys a Mac shortcut may be on, so that every common Mac layout can press it. **Measured** on a Mac (macOS 26.7) from the layouts'
    /// own tables (`UCKeyTranslate` of every key with each modifier) for US, ABC, British and British-PC, German, Austrian and Swiss German, French,
    /// French-PC, ABC-AZERTY, Belgian and Swiss French, Spanish, Spanish-ISO and Latin American, Portuguese, Brazilian (three), Italian and Italian-Pro,
    /// Swedish and Swedish-Pro, Norwegian, Danish, Finnish, Icelandic and Dutch, on an ISO and an ANSI keyboard, through the path a key takes to this
    /// window as read from the pinned winit and egui-winit: with Command held the character comes from the layout's Command table, which ignores
    /// Option and, on some layouts, Shift; a letter of that table wins; a character egui has no key for falls back to the key's US position.
    /// - With no Shift in the shortcut: the letters, the digits, `-`, `,` and `.`. Each reaches the window on every one of those layouts by typing the
    ///   character, with Shift where the layout needs it (the digits of the French layouts); `,` and `.` also by the key at the US position.
    /// - With Shift in it: the letters only. With Command and Shift the US and British layouts give a digit or a sign without its Shift, while French
    ///   gives the Shift character, so no sign is the same everywhere; the French comma has no Shift character at all.
    /// - Option may be added to either, since the Command table ignores it.
    const LAYOUT_SAFE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-,.";
    const LAYOUT_SAFE_WITH_SHIFT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    /// Two keys of one command that together reach every layout of the list, though each lacks some: `=` is Shift+0 on the Portuguese and Icelandic
    /// layouts (and German and the Nordic ones), where it arrives as `0`, and there `+` is a key of its own; the US and British layouts give `=` for
    /// Shift+Command+= too, so `+` never arrives there. The Swiss layouts type both with Shift on digits, which arrive as digits; there only the key at
    /// the US position of `=` (`^`, which egui has no key for) reaches Command+=.
    const LAYOUT_PAIRS: &[(&str, &str, &str)] = &[("view.zoomIn", "Cmd+=", "Cmd+Plus")];

    /// The keys that type no character, which a layout does not move.
    fn types_no_character(key: Key) -> bool {
        let name = key.name();
        let function = name.len() > 1 && name.starts_with('F') && name.bytes().skip(1).all(|b| b.is_ascii_digit());
        function
            || matches!(
                key,
                Key::ArrowDown
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::ArrowUp
                    | Key::Escape
                    | Key::Tab
                    | Key::Backspace
                    | Key::Enter
                    | Key::Insert
                    | Key::Delete
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
            )
    }

    /// The character of a key, as the lists above write it.
    fn character(key: Key) -> Option<char> {
        match key {
            Key::Minus => Some('-'),
            Key::Comma => Some(','),
            Key::Period => Some('.'),
            Key::Equals => Some('='),
            Key::Plus => Some('+'),
            Key::Pipe => Some('|'),
            other => {
                let mut chars = other.name().chars();
                chars.next().filter(|_| chars.next().is_none())
            }
        }
    }

    /// Every key of the Mac's table is one the common Mac layouts can press (the measured lists above): a key that types no character, or a
    /// character of `LAYOUT_SAFE` (`LAYOUT_SAFE_WITH_SHIFT` when the shortcut holds Shift) pressed with Command, or one of a pair that together reach
    /// every layout. The Command+| was none of these: no layout of the list but Latin American can press it here.
    #[test]
    fn every_mac_key_is_one_the_common_mac_layouts_can_press() {
        let mac = shortcuts(OperatingSystem::Mac);
        let on_mac = |id: &str, spec: &str| mac.iter().any(|b| b.id == id && Some(b.shortcut) == parse(spec));
        let mut refused = Vec::new();
        for b in &mac {
            let (key, modifiers) = (b.shortcut.logical_key, b.shortcut.modifiers);
            if types_no_character(key) {
                continue;
            }
            let safe = if modifiers.shift { LAYOUT_SAFE_WITH_SHIFT } else { LAYOUT_SAFE };
            let in_list = character(key).is_some_and(|c| safe.contains(c));
            // Option with a letter alone types a sign there, which comes to the window as the key's US position: a character needs Command.
            let with_command = modifiers.command;
            let paired = LAYOUT_PAIRS.iter().any(|(id, one, other)| {
                *id == b.id && [(one, other), (other, one)].iter().any(|(this, partner)| parse(this) == Some(b.shortcut) && on_mac(id, partner))
            });
            if !(with_command && in_list) && !paired {
                refused.push(format!("{} {}: {:?} with {:?}", b.id, b.params, key, modifiers));
            }
        }
        assert!(refused.is_empty(), "keys some common Mac layouts cannot press:\n{}", refused.join("\n"));
        // The lists hold what they say, so that the test cannot pass on a list widened by mistake.
        assert!(LAYOUT_SAFE.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "-,.".contains(c)));
        assert_eq!(LAYOUT_SAFE.len(), 26 + 10 + 3);
        assert!(LAYOUT_SAFE_WITH_SHIFT.chars().all(|c| c.is_ascii_uppercase()) && LAYOUT_SAFE_WITH_SHIFT.len() == 26);
        for (id, one, other) in LAYOUT_PAIRS {
            assert!(on_mac(id, one) && on_mac(id, other), "{id} has both {one} and {other} on a Mac");
        }
        // The keys moved: Center, the font sizes; and the Command+| is no key of the Mac's.
        let keys_of = |id: &str, params: &str| mac.iter().filter(|b| b.id == id && b.params == params).map(|b| b.shortcut).collect::<Vec<_>>();
        let cmd = Modifiers::COMMAND;
        assert_eq!(keys_of("format.text", r#"{"align":"center"}"#), [KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::E)]);
        assert_eq!(keys_of("format.size", r#"{"step":"up"}"#), [KeyboardShortcut::new(cmd | Modifiers::ALT, Key::Period)]);
        assert_eq!(keys_of("format.size", r#"{"step":"down"}"#), [KeyboardShortcut::new(cmd | Modifiers::ALT, Key::Comma)]);
        assert!(mac.iter().all(|b| b.shortcut.logical_key != Key::Pipe), "no Mac key is on |");
    }

    /// The Mac's second keys are the Mac's only. On a Mac Backspace deletes, beside Delete, and Command+Plus zooms in, beside Command+=; on
    /// Windows, Linux and FreeBSD Backspace is no shortcut, and the keys of the other systems stay as they were.
    #[test]
    fn the_macs_second_keys_are_on_a_mac_only() {
        let keys = |os, id: &str| shortcuts(os).into_iter().filter(|b| b.id == id).map(|b| b.shortcut).collect::<Vec<_>>();
        let delete = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
        let backspace = KeyboardShortcut::new(Modifiers::NONE, Key::Backspace);
        assert_eq!(keys(OperatingSystem::Mac, "shape.delete"), [delete, backspace]);
        let (zoom_in, plus) = (KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals), KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus));
        let alt_f6 = KeyboardShortcut::new(Modifiers::ALT, Key::F6);
        assert_eq!(keys(OperatingSystem::Mac, "view.zoomIn"), [zoom_in, alt_f6, plus]);
        for os in [OperatingSystem::Windows, OperatingSystem::Nix] {
            assert_eq!(keys(os, "shape.delete"), [delete], "{os:?}");
            assert_eq!(keys(os, "view.zoomIn"), [zoom_in, alt_f6], "{os:?}");
            assert!(shortcuts(os).iter().all(|b| b.shortcut != backspace && b.shortcut.logical_key != Key::Plus), "{os:?}");
            let keys_of = |params: &str| shortcuts(os).into_iter().filter(|b| b.params == params).map(|b| b.shortcut).collect::<Vec<_>>();
            let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
            assert_eq!(keys_of(r#"{"align":"center"}"#), [KeyboardShortcut::new(ctrl_shift, Key::C)], "{os:?}");
            assert_eq!(keys_of(r#"{"step":"up"}"#), [KeyboardShortcut::new(ctrl_shift, Key::Period)], "{os:?}");
            assert_eq!(keys_of(r#"{"step":"down"}"#), [KeyboardShortcut::new(ctrl_shift, Key::Comma)], "{os:?}");
        }
        for (id, spec) in MAC_ALSO {
            assert!(diacraft_engine::find_command(id).is_some() || crate::commands::find_ui_command(id).is_some(), "{id}");
            assert!(parse(spec).is_some(), "{spec}");
        }
    }

    /// Every engine command whose key the window runs through its own command names a window command that exists, and an engine command with a key.
    #[test]
    fn the_engine_keys_the_window_runs_itself_name_window_commands() {
        for (engine, window, source) in ENGINE_KEYS_FOR_THE_WINDOW {
            let spec = diacraft_engine::find_command(engine).unwrap_or_else(|| panic!("{engine} is not an engine command"));
            assert!(spec.shortcut.is_some(), "{engine} has no key of its own");
            assert!(crate::commands::find_ui_command(window).is_some(), "{window} is not a window command");
            assert!(!source.is_empty());
        }
    }

    /// A window key names a window command, with parameters that command takes and that are JSON.
    #[test]
    fn every_window_key_names_a_window_command_with_parameters_it_takes() {
        for k in WINDOW_KEYS {
            let command = crate::commands::find_ui_command(k.command)
                .unwrap_or_else(|| panic!("{} names `{}`, which is not a window command", k.shortcut, k.command));
            let params: Value = serde_json::from_str(k.params).unwrap_or_else(|e| panic!("{}: {e}", k.shortcut));
            let allowed = diacraft_engine::param_keys(command.params).unwrap();
            let object = params.as_object().unwrap_or_else(|| panic!("{}: the parameters are not an object", k.shortcut));
            assert!(
                object.keys().all(|key| allowed.contains(&key.as_str())) && (allowed.is_empty() || !object.is_empty()),
                "{}: {} does not take {:?}",
                k.shortcut,
                k.command,
                object.keys().collect::<Vec<_>>()
            );
            assert!(!k.source.is_empty());
        }
    }
}
