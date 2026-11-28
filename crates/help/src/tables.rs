//! The three tables of a help tree: tips, the help map and the sections.
//!
//! All three are tab-separated, one row a line, with `#` lines and blank lines skipped and `\n`, `\t` and `\\` the only escapes. A row that breaks a rule is skipped and
//! reported with its line (a checker fails on any problem, the program skips the row and goes on), the first of two rows with one key is kept, and no table holds more rows
//! than its cap.
//!
//! * **tips** (`tips.tsv`): `key`, `description`, `keywords`. The key is the id of a ribbon control or of a command; the description is at most 400 characters; the keywords are
//!   words separated by spaces, at most 32.
//! * **map** (`map.tsv`): `place`, `target`, `note`. See [`Place`]. The target is a topic id, a topic id and `#anchor`, or `none` with a note that says why.
//! * **sections** (`sections.tsv`): `section`, `topic`. The sections of the help home in the order they first appear, each with its topics in the order of their rows.

use std::collections::HashSet;

use crate::inline::valid_id;
use crate::{Code, MAX_DESCRIPTION_CHARS, MAX_KEYWORDS, MAX_MAP_ROWS, MAX_ROW_BYTES, MAX_TIPS_ROWS, Problem, report};

/// The columns of a row, split at tabs.
pub fn split_tsv_row(line: &str) -> Vec<&str> {
    line.split('\t').collect()
}

/// A column with `\n`, `\t` and `\\` read; `None` for a backslash before anything else, or at the end.
pub fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            _ => return None,
        }
    }
    Some(out)
}

/// The rows of a table: line number and columns. A row over the byte cap or with a control character is reported and left out, and so is every row past `cap`.
fn rows<'a>(text: &'a str, cap: usize, problems: &mut Vec<Problem>) -> Vec<(usize, Vec<&'a str>)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut count = 0usize;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_ROW_BYTES {
            report(problems, n, Code::RowTooLong, "");
            continue;
        }
        if line.chars().any(|c| c.is_control() && c != '\t') {
            report(problems, n, Code::RowControl, "");
            continue;
        }
        if count >= cap {
            report(problems, n, Code::TooManyRows, "");
            break;
        }
        count += 1;
        out.push((n, split_tsv_row(line)));
    }
    out
}

// ------------------------------------------------------------------------------------------------ tips

/// A tip's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tip {
    /// The control or command id.
    pub key: String,
    /// The description.
    pub description: String,
    /// The search keywords.
    pub keywords: Vec<String>,
    /// The line of the row.
    pub line: usize,
}

/// The tips table.
#[derive(Clone, Debug, Default)]
pub struct Tips {
    rows: Vec<Tip>,
}

impl Tips {
    /// Reads `tips.tsv`, skipping the rows that break a rule.
    pub fn parse(text: &str) -> (Tips, Vec<Problem>) {
        let mut problems = Vec::new();
        let mut tips = Tips::default();
        let mut seen = HashSet::new();
        for (n, columns) in rows(text, MAX_TIPS_ROWS, &mut problems) {
            match tip(n, &columns) {
                Err((code, detail)) => report(&mut problems, n, code, &detail),
                Ok(row) if !seen.insert(row.key.clone()) => report(&mut problems, n, Code::RowDuplicate, &row.key),
                Ok(row) => tips.rows.push(row),
            }
        }
        problems.sort_by_key(|p| p.line);
        (tips, problems)
    }

    /// The row of `key`.
    pub fn get(&self, key: &str) -> Option<&Tip> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// The rows, in order.
    pub fn rows(&self) -> &[Tip] {
        &self.rows
    }
}

fn tip(line: usize, columns: &[&str]) -> Result<Tip, (Code, String)> {
    let [key, description, keywords] = columns else { return Err((Code::RowColumns, format!("{} columns", columns.len()))) };
    if key.is_empty() || key.len() > 96 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.') {
        return Err((Code::RowKey, (*key).to_owned()));
    }
    let (Some(description), Some(keywords)) = (unescape(description), unescape(keywords)) else { return Err((Code::RowEscape, String::new())) };
    let chars = description.chars().count();
    if chars == 0 || chars > MAX_DESCRIPTION_CHARS {
        return Err((Code::DescriptionLength, format!("{chars} characters")));
    }
    let words: Vec<String> = keywords.split_whitespace().map(str::to_owned).collect();
    if words.len() > MAX_KEYWORDS {
        return Err((Code::TooManyKeywords, format!("{} keywords", words.len())));
    }
    Ok(Tip { key: (*key).to_owned(), description, keywords: words, line })
}

// ------------------------------------------------------------------------------------------------ map

/// A place of the window.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// `ribbon:<ControlId>`: a ribbon control, a menu item or a launcher.
    Ribbon(String),
    /// `group:<GroupId>`: a group of the ribbon.
    Group(String),
    /// `tab:<TabId>`: a tab of the ribbon.
    Tab(String),
    /// `dialog:<id>` or `dialog:<id>/<tab>`: a dialog, and a tab or page of it.
    Dialog {
        /// The dialog.
        id: String,
        /// The tab.
        tab: Option<String>,
    },
    /// `pane:<word>`: a pane.
    Pane(String),
    /// `backstage:<page>`: a page of the File view.
    Backstage(String),
    /// `window:<area>`: an area of the window.
    Window(String),
    /// `cmd:<command id>`: a command no control reaches.
    Command(String),
}

/// A name in a place: letters, digits, underscores, dots and dashes.
fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

impl Place {
    /// Reads `kind:name`; `None` when it is not one of the kinds or the name is not a name.
    pub fn parse(text: &str) -> Option<Place> {
        let (kind, name) = text.split_once(':')?;
        if kind == "dialog" {
            let (id, tab) = match name.split_once('/') {
                Some((id, tab)) => (id, Some(tab)),
                None => (name, None),
            };
            return (name_ok(id) && tab.is_none_or(name_ok)).then(|| Place::Dialog { id: id.to_owned(), tab: tab.map(str::to_owned) });
        }
        if !name_ok(name) {
            return None;
        }
        let name = name.to_owned();
        Some(match kind {
            "ribbon" => Place::Ribbon(name),
            "group" => Place::Group(name),
            "tab" => Place::Tab(name),
            "pane" => Place::Pane(name),
            "backstage" => Place::Backstage(name),
            "window" => Place::Window(name),
            "cmd" => Place::Command(name),
            _ => return None,
        })
    }
}

/// Where a place's help is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapTarget {
    /// A topic, and a place in it.
    Topic {
        /// The topic.
        id: String,
        /// The anchor.
        anchor: Option<String>,
    },
    /// No topic, with why.
    None {
        /// Why.
        note: String,
    },
}

/// A row of the help map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRow {
    /// The place.
    pub place: Place,
    /// Where its help is.
    pub target: MapTarget,
    /// The line of the row.
    pub line: usize,
}

/// The help map.
#[derive(Clone, Debug, Default)]
pub struct Map {
    rows: Vec<MapRow>,
}

impl Map {
    /// Reads `map.tsv`, skipping the rows that break a rule.
    pub fn parse(text: &str) -> (Map, Vec<Problem>) {
        let mut problems = Vec::new();
        let mut map = Map::default();
        let mut seen = HashSet::new();
        for (n, columns) in rows(text, MAX_MAP_ROWS, &mut problems) {
            match map_row(n, &columns) {
                Err((code, detail)) => report(&mut problems, n, code, &detail),
                Ok(row) if !seen.insert(row.place.clone()) => {
                    report(&mut problems, n, Code::RowDuplicate, columns.first().copied().unwrap_or_default())
                }
                Ok(row) => map.rows.push(row),
            }
        }
        problems.sort_by_key(|p| p.line);
        (map, problems)
    }

    /// The target of `place`.
    pub fn get(&self, place: &Place) -> Option<&MapTarget> {
        self.rows.iter().find(|r| &r.place == place).map(|r| &r.target)
    }

    /// The rows, in order.
    pub fn rows(&self) -> &[MapRow] {
        &self.rows
    }
}

fn map_row(line: usize, columns: &[&str]) -> Result<MapRow, (Code, String)> {
    let [place, target, note] = columns else { return Err((Code::RowColumns, format!("{} columns", columns.len()))) };
    let parsed = Place::parse(place).ok_or_else(|| (Code::PlaceSyntax, (*place).to_owned()))?;
    let note = unescape(note).ok_or((Code::RowEscape, String::new()))?;
    let target = if *target == "none" {
        if note.trim().is_empty() {
            return Err((Code::MapTarget, "none needs a note".to_owned()));
        }
        MapTarget::None { note: note.trim().to_owned() }
    } else {
        let (id, anchor) = match target.split_once('#') {
            Some((id, anchor)) => (id, Some(anchor)),
            None => (*target, None),
        };
        if !valid_id(id) || !anchor.is_none_or(valid_id) {
            return Err((Code::MapTarget, (*target).to_owned()));
        }
        MapTarget::Topic { id: id.to_owned(), anchor: anchor.map(str::to_owned) }
    };
    Ok(MapRow { place: parsed, target, line })
}

// ------------------------------------------------------------------------------------------------ sections

/// A section of the help home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Its title.
    pub title: String,
    /// Its topics, in order.
    pub topics: Vec<String>,
    /// The line of each topic's row.
    pub lines: Vec<usize>,
}

/// The sections.
#[derive(Clone, Debug, Default)]
pub struct Sections {
    sections: Vec<Section>,
}

impl Sections {
    /// Reads `sections.tsv`, skipping the rows that break a rule.
    pub fn parse(text: &str) -> (Sections, Vec<Problem>) {
        let mut problems = Vec::new();
        let mut out = Sections::default();
        let mut seen = HashSet::new();
        for (n, columns) in rows(text, MAX_TIPS_ROWS, &mut problems) {
            let [title, topic] = columns.as_slice() else {
                report(&mut problems, n, Code::RowColumns, &format!("{} columns", columns.len()));
                continue;
            };
            let Some(title) = unescape(title).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) else {
                report(&mut problems, n, Code::RowKey, "");
                continue;
            };
            if !valid_id(topic) {
                report(&mut problems, n, Code::TargetId, topic);
                continue;
            }
            if !seen.insert((title.clone(), (*topic).to_owned())) {
                report(&mut problems, n, Code::RowDuplicate, topic);
                continue;
            }
            match out.sections.iter_mut().find(|s| s.title == title) {
                Some(section) => {
                    section.topics.push((*topic).to_owned());
                    section.lines.push(n);
                }
                None => out.sections.push(Section { title, topics: vec![(*topic).to_owned()], lines: vec![n] }),
            }
        }
        problems.sort_by_key(|p| p.line);
        (out, problems)
    }

    /// The sections, in the order they first appear.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
}
