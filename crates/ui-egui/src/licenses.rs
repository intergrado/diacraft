//! Third-party licences: the window that shows `THIRD-PARTY-LICENSES.txt`, the libraries DiaCraft is built from and the licences they are under.
//!
//! **Why it is here.** The licences of the libraries ask that their notices travel with the programs. Every download carries the file beside the programs (and the
//! install puts it under `share/doc/diacraft`), and the program carries it too: [`TEXT`] is the file, built into the window, so that About can show it whatever
//! the program is next to, and whether the person moved the program out of the folder it came in. The web build has no folder to carry a file in: it shows the
//! list of its own crates, `THIRD-PARTY-LICENSES-WEB.txt`, and the website links the same file. **The web build does not carry that file inside its module** (
//! `asset_host.rs`): the page fetches it, from a hashed file of the site beside the app, when the window first opens, and the window says that it is loading, or why it could
//! not be had, in the place of the list. The files are made by `cargo xtask licenses` and kept current by `cargo xtask licenses --check`; this module reads them as they are.
//!
//! **A command, not a button's own code.** `app.licenses` opens the window (and `app.licenses {close: true}` closes it), and the About panel's link, like a program on
//! the control channel, runs that command. The window is a modal card over the shade (`ask.rs`): while it is up nothing under it takes the keys or the pointer. Escape,
//! the Close button and the command close it.
//!
//! **The list.** The file is some 8,000 lines, and some of its lines are long paragraphs of a licence text. They are broken at [`COLUMNS`] columns, at a space where
//! there is one, once ([`rows`]), and the window lists only the rows that are in view (`ScrollArea::show_rows`), so that a frame does not lay out the whole file. Page
//! Up, Page Down, Home, End and the arrow keys scroll it, as the wheel and the bar do.

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;

use egui::{Context, FontId, Id, Key, Label, Modifiers, RichText, ScrollArea, TextWrapMode, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::asset_host::{Asset, Fetch, Source};
use crate::theme::{self, Tokens, size};

/// The notices of the desktop programs, as `cargo xtask licenses` made them: the file of the repository, and of every download. **Not in the web build**, which fetches its own list (the
/// crates of `diacraft-web` alone; the website links the same file beside the app).
#[cfg(not(target_arch = "wasm32"))]
pub const TEXT: &str = include_str!("../../../THIRD-PARTY-LICENSES.txt");

/// What the window says over the list. The desktop programs' file comes in every download; the web build's is the file of the repository that the website
/// links beside the app (the web build is no download).
pub const INTRO: &str = if cfg!(target_arch = "wasm32") {
    "The libraries the web version of DiaCraft is built from and the licences they are under. The same text is THIRD-PARTY-LICENSES-WEB.txt, which the website links beside the app."
} else {
    "The libraries DiaCraft is built from and the licences they are under. The same text is THIRD-PARTY-LICENSES.txt, which comes in the download."
};

/// The columns a row of the list is broken at.
pub const COLUMNS: usize = 100;

/// The most rows [`wrap_rows`] makes: a text that would make more is cut, with a row that says so. The file is far below it; the limit is for the function, which
/// the tests give any text.
const MOST_ROWS: usize = 200_000;

/// The size of the list's text, in points.
const FONT_SIZE: f32 = 11.0;

/// The rows of [`TEXT`], made once.
#[cfg(not(target_arch = "wasm32"))]
pub fn rows() -> &'static [String] {
    static ROWS: OnceLock<Vec<String>> = OnceLock::new();
    ROWS.get_or_init(|| wrap_rows(TEXT, COLUMNS))
}

/// What the window says in the place of the list while the list is on its way, and when it could not be had.
pub const WAIT: &str = "Loading the list of licences\u{2026}";

/// What the window says in the place of the list when it could not be had: the reason (which names the list), a way to try again, and where else the text is.
pub fn failed_words(reason: &str) -> String {
    let mut chars = reason.trim().chars();
    let sentence: String = chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect());
    let sentence = if sentence.is_empty() { "The list of third-party licences could not be had".to_owned() } else { sentence };
    format!("{sentence}. Reload the page to try again; the same text is the file the website links beside the app.")
}

/// The rows the window lists: waiting for the text, made from it, or why not.
#[derive(Clone)]
pub(crate) enum Rows {
    Waiting,
    Ready(Arc<[String]>),
    Failed(String),
}

/// The list of licences the window has, kept once it is made: the text is asked for when the window first opens, broken into rows once, and the host lets go of the bytes.
#[derive(Default)]
pub(crate) struct Listing {
    made: Option<Result<Arc<[String]>, String>>,
}

impl Listing {
    /// The rows: made when the text is there, and kept, so that a list that is waiting costs a look at the host and one that failed is not asked for again every frame.
    pub(crate) fn get(&mut self, ctx: &Context, source: &Source) -> Rows {
        if let Some(done) = &self.made {
            return Self::shown(done);
        }
        let made = match source.get(ctx, Asset::Licenses) {
            Fetch::Waiting => return Rows::Waiting,
            Fetch::Failed(error) => Err(error),
            Fetch::Ready(bytes) => match std::str::from_utf8(bytes.as_slice()) {
                Ok(text) => Ok(Arc::from(wrap_rows(text, COLUMNS))),
                Err(_) => Err(format!("{} is not text", Asset::Licenses.words())),
            },
        };
        source.taken(Asset::Licenses);
        let shown = Self::shown(&made);
        self.made = Some(made);
        shown
    }

    /// The rows as they are, with no asking.
    pub(crate) fn peek(&self) -> Rows {
        self.made.as_ref().map_or(Rows::Waiting, Self::shown)
    }

    fn shown(done: &Result<Arc<[String]>, String>) -> Rows {
        match done {
            Ok(rows) => Rows::Ready(rows.clone()),
            Err(error) => Rows::Failed(error.clone()),
        }
    }
}

/// Breaks `text` into rows of at most `columns` characters (between 8 and 1,000): each line of the text is a row, or, when it is longer, a row for each piece of it,
/// broken at the last space that fits (or at the width where there is none), the pieces after the first indented as the line is, up to a quarter of the width. Tabs
/// become four spaces and other control characters are dropped. A text of more than [`MOST_ROWS`] rows is cut.
pub fn wrap_rows(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.clamp(8, 1000);
    let mut rows = Vec::new();
    for line in text.lines() {
        if rows.len() >= MOST_ROWS {
            rows.push("(the rest of the text is not shown)".to_owned());
            break;
        }
        let clean: String =
            line.chars().flat_map(|c| if c == '\t' { "    ".chars().collect::<Vec<_>>() } else { vec![c] }).filter(|c| !c.is_control()).collect();
        wrap_line(clean.trim_end(), columns, &mut rows);
    }
    rows
}

fn wrap_line(line: &str, columns: usize, rows: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= columns {
        rows.push(line.to_owned());
        return;
    }
    let indent = chars.iter().take_while(|c| **c == ' ').count().min(columns / 4);
    let pad = " ".repeat(indent);
    let mut start = 0;
    let mut first = true;
    while start < chars.len() {
        let width = if first { columns } else { columns.saturating_sub(indent).max(1) };
        let end = start.saturating_add(width).min(chars.len());
        let piece = |from: usize, to: usize| {
            let text: String = chars.get(from..to).unwrap_or_default().iter().collect();
            if first { text } else { format!("{pad}{text}") }
        };
        if end == chars.len() {
            rows.push(piece(start, end));
            break;
        }
        // The last space in the window, if that leaves a piece that is not empty; else the window is cut where it ends.
        let space = chars.get(start..end).and_then(|window| window.iter().rposition(|c| *c == ' ')).filter(|at| *at > 0);
        let (to, next) = match space {
            Some(at) => (start + at, start + at + 1),
            None => (end, end),
        };
        rows.push(piece(start, to));
        start = next;
        while chars.get(start) == Some(&' ') {
            start += 1;
        }
        first = false;
    }
}

/// The window while it is open.
#[derive(Debug, Default)]
pub struct LicensesWindow {
    /// Where the list is scrolled to, in points from the top, as the last frame left it.
    offset: f32,
    /// Where a key asks the list to go at the next frame.
    scroll_to: Option<f32>,
}

impl DiacraftApp {
    /// Whether the Third-party licences window is open.
    pub fn licenses_open(&self) -> bool {
        self.ui.licenses.is_some()
    }

    /// The window's part of the window state (`null` when it is closed): how many rows the list has, where it is scrolled to, and whether the text is `"ready"`, `"waiting"` (the web
    /// build is fetching it) or `"failed"` (and then `error` says why).
    pub(crate) fn licenses_state(&self) -> Value {
        let Some(window) = &self.ui.licenses else { return Value::Null };
        match self.licence_list.peek() {
            Rows::Ready(rows) => json!({"rows": rows.len(), "offset": window.offset, "text": "ready"}),
            Rows::Waiting => json!({"rows": 0, "offset": window.offset, "text": "waiting"}),
            Rows::Failed(error) => json!({"rows": 0, "offset": window.offset, "text": "failed", "error": error}),
        }
    }

    /// `app.licenses`: opens the window at the top of the list; one that is open stays as it is.
    pub(crate) fn open_licenses(&mut self) {
        if self.ui.licenses.is_none() {
            self.ui.licenses = Some(LicensesWindow::default());
        }
    }

    /// `app.licenses {close: true}`: the window goes. Closing what is not open is not an error.
    pub(crate) fn close_licenses(&mut self) {
        self.ui.licenses = None;
    }

    /// Draws the window over the drawing area, and closes it on Escape or Close.
    pub(crate) fn licenses_window(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut window) = self.ui.licenses.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 780.0);
        // A fixed height: a window that finds its size from its list takes frames to settle in the middle of the screen.
        let list_h = (screen.height() - 190.0).clamp(100.0, 560.0);
        let mono = FontId::monospace(FONT_SIZE);
        let source = self.asset_source();
        let listing = self.licence_list.get(ctx, &source);
        let rows: &[String] = match &listing {
            Rows::Ready(rows) => rows,
            Rows::Waiting | Rows::Failed(_) => &[],
        };
        let mut close = false;
        let mut seen: Vec<(&'static str, egui::Rect)> = Vec::new();
        let mut row_h = FONT_SIZE;
        let modal = egui::Modal::new(Id::unique("licenses_window")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            let title = ui.label(RichText::new("Third-party licences").font(theme::font(size::TAB + 1.0)).color(t.text));
            seen.push(("licenses.title", title.rect));
            ui.add_space(4.0);
            ui.add(Label::new(RichText::new(INTRO).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            ui.add_space(8.0);
            row_h = ui.fonts_mut(|f| f.row_height(&mono)).max(1.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let mut area = ScrollArea::both().id_salt("licenses_list").min_scrolled_height(list_h).max_height(list_h).auto_shrink([false, false]);
            if let Some(to) = window.scroll_to.take() {
                area = area.vertical_scroll_offset(to);
            }
            // The rows touch: `show_rows` takes the pitch of the rows from the spacing of the `Ui` it is given, so it is set before, and not inside the closure.
            let spacing = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Rows::Ready(_) = &listing {
                let out = area.show_rows(ui, row_h, rows.len(), |ui, range| {
                    for at in range {
                        let text = rows.get(at).map_or("", String::as_str);
                        // A row with nothing in it keeps the height of a row.
                        let text = if text.is_empty() { "\u{a0}" } else { text };
                        ui.add(Label::new(RichText::new(text).font(mono.clone()).color(t.text)).wrap_mode(TextWrapMode::Extend));
                    }
                });
                window.offset = out.state.clamped_offset().y;
                seen.push(("licenses.list", out.inner_rect));
            } else {
                // The list is on its way, or could not be had: the same place, with the words in it, and the card keeps its size (it finds none from its list).
                let (place, _) = ui.allocate_exact_size(vec2(width, list_h), egui::Sense::hover());
                ui.painter().rect_stroke(place, 0.0, egui::Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
                let words = match &listing {
                    Rows::Failed(error) => failed_words(error),
                    _ => WAIT.to_owned(),
                };
                let galley = ui.painter().layout(words, theme::font(size::LABEL), t.text_dim, (width - 24.0).max(40.0));
                let at = place.center() - galley.size() / 2.0;
                ui.painter().galley(at, galley, t.text_dim);
                seen.push(("licenses.notice", place));
            }
            ui.spacing_mut().item_spacing.y = spacing;
            ui.add_space(8.0);
            let button = ui.add(egui::Button::new(RichText::new("Close").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
            seen.push(("licenses.close", button.rect));
            close = button.clicked();
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect) in seen {
            self.note(name, rect, true);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            close = true;
        }
        // The keys that move a list: a page, a line, and the ends. The next frame puts the list there.
        let end = rows.len() as f32 * row_h;
        let at = window.offset;
        let to = ctx.input_mut(|i| {
            [
                (Key::PageDown, at + list_h),
                (Key::PageUp, at - list_h),
                (Key::ArrowDown, at + row_h),
                (Key::ArrowUp, at - row_h),
                (Key::Home, 0.0),
                (Key::End, end),
            ]
            .into_iter()
            .find(|(key, _)| i.consume_key(Modifiers::NONE, *key))
            .map(|(_, to)| to.clamp(0.0, end))
        });
        if let Some(to) = to {
            window.scroll_to = Some(to);
            ctx.request_repaint();
        }
        if close {
            // Gone: the frame that follows is drawn without it.
            ctx.request_repaint();
        } else {
            self.ui.licenses = Some(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_line_is_one_row_and_an_empty_line_is_an_empty_row() {
        assert_eq!(wrap_rows("one\n\ntwo", 100), ["one", "", "two"]);
        assert!(wrap_rows("", 100).is_empty());
    }

    #[test]
    fn a_long_line_is_broken_at_the_last_space_that_fits_and_the_pieces_keep_the_indent() {
        let rows = wrap_rows("    aaaa bbbb cccc dddd eeee", 16);
        assert_eq!(rows, ["    aaaa bbbb", "    cccc dddd", "    eeee"]);
        assert!(rows.iter().all(|r| r.chars().count() <= 16));
    }

    #[test]
    fn a_word_longer_than_the_width_is_cut_where_the_width_ends_and_nothing_is_lost() {
        let rows = wrap_rows(&"x".repeat(30), 10);
        assert_eq!(rows, ["xxxxxxxxxx"; 3]);
        // Characters outside ASCII are cut between characters, not between their bytes.
        let rows = wrap_rows(&"é".repeat(25), 10);
        assert_eq!(rows.iter().map(|r| r.chars().count()).collect::<Vec<_>>(), [10, 10, 5]);
    }

    #[test]
    fn tabs_become_spaces_and_other_control_characters_go() {
        assert_eq!(wrap_rows("a\tb\u{7}c\u{1b}[0m", 100), ["a    bc[0m"]);
    }

    #[test]
    fn the_width_is_kept_inside_what_the_function_can_use_and_a_huge_text_is_cut() {
        assert_eq!(wrap_rows("abcdefghijklmnopq", 0), ["abcdefgh", "ijklmnop", "q"], "a width under 8 is 8");
        let big = "line\n".repeat(MOST_ROWS + 10);
        let rows = wrap_rows(&big, 100);
        assert_eq!(rows.len(), MOST_ROWS + 1);
        assert_eq!(rows.last().map(String::as_str), Some("(the rest of the text is not shown)"));
    }

    #[test]
    fn the_words_for_a_list_that_could_not_be_had_start_with_a_capital_and_say_what_to_do() {
        let said = failed_words("the list of third-party licences could not be fetched (the server answered 404)");
        assert_eq!(
            said,
            "The list of third-party licences could not be fetched (the server answered 404). Reload the page to try again; the same text is the file the website links beside the app."
        );
        assert!(failed_words("\u{e9}lan").starts_with("\u{c9}lan"), "a capital that is not ASCII");
        assert!(
            failed_words("").starts_with("The list of third-party licences could not be had."),
            "a reason with nothing in it is still a sentence"
        );
        assert!(failed_words("  \n ").starts_with("The list of third-party licences"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_rows_of_the_file_say_its_title_and_its_end_and_no_row_is_wider_than_the_columns() {
        let rows = rows();
        assert_eq!(rows.first().map(String::as_str), Some("DiaCraft: third-party licences"));
        assert_eq!(rows.last().map(String::as_str), Some("End of the third-party licences."));
        assert!(rows.len() > TEXT.lines().count() / 2, "{} rows", rows.len());
        assert!(rows.iter().all(|r| r.chars().count() <= COLUMNS), "a row is wider than {COLUMNS}");
    }
}
