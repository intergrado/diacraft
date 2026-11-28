//! The feature comparison page: `site/feature-comparison.html` compares DiaCraft with the reference app, draw.io and LibreOffice Draw, row by row.
//!
//! **The page is made from data.** The page file holds the words a person wrote (the title, the lead, how to read the table, the notices) and six regions that this module makes again from the
//! data each time `cargo xtask site --write` runs, between the marker lines `<!-- NAME:begin -->` and `<!-- NAME:end -->`: `facts` (the title block), `asof` (the line that says when the sources
//! were read), `editions` (the captions of the reference app's plans), `toc` (the list of areas), `areas` (one table for each area) and `only` (the features only DiaCraft has). `cargo xtask site`
//! makes them again in memory and says the page is stale when they differ, so a page that is not the data's is a failed check. The data is [`MATRIX`], read as `docs/parity-matrix.md` says.
//!
//! **The DiaCraft column is computed.** A row is Full only if every checklist row it names is done (D) in `docs/parity-checklist.md`, Not yet if every one is missing (M) and Partial otherwise; the
//! few rows that name no checklist row take the value and the source they give. Every checklist row must be in exactly one row of the data, so a row added to the checklist, or renamed, is an
//! error here until the data says where it belongs. The other columns are the research's: the value, what it rests on, the sources and the day they were read.
//!
//! **The name of the reference app** is not spelled in this file: a page of the site may name it (`xtask::brands::SITE_PAGE_NAMES`), the Rust sources and the docs may not, so the page's column and
//! sentences take it from that list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::{Table, Value as Toml};

use super::links::SOURCE_HOSTS;
use super::parts::read_bounded;
use super::settings::Settings;

/// The page this module makes parts of, a file of `site/`.
pub(super) const PAGE: &str = "feature-comparison.html";
/// The data, and the checklist it scores DiaCraft from, as paths from the repository's root.
const MATRIX: &str = "docs/parity-matrix.toml";
const CHECKLIST: &str = "docs/parity-checklist.md";
/// The most bytes of either file that are read: the data is some 300 thousand, the checklist some 200 thousand.
const MAX_FILE: u64 = 4 * 1024 * 1024;
/// Bounds on what the data may hold, so that a damaged file is refused and not rendered: rows, areas, items in a list, characters in a string.
const MAX_ROWS: usize = 1_000;
const MAX_AREAS: usize = 64;
const MAX_LIST: usize = 64;
const MAX_TEXT: usize = 4_000;

/// What a cell says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value {
    Full,
    Partial,
    NotYet,
    Unknown,
}

impl Value {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "Full" => Some(Self::Full),
            "Partial" => Some(Self::Partial),
            "Not yet" => Some(Self::NotYet),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// What a cell of LibreOffice or draw.io rests on (`docs/parity-matrix.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    /// A public page says so.
    Documented,
    /// Seen by running LibreOffice as a black box.
    Oracle,
    /// Not found in the documentation read: a Not yet that says "not found", and not "missing".
    Absent,
    /// No source settles it.
    Unknown,
}

/// Where the vendor's comparison of its plans puts a feature of the reference app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Edition {
    Desktop,
    Plan1,
    Plan2,
    Mixed,
    Web,
    None,
}

impl Edition {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "desktop" => Some(Self::Desktop),
            "plan1" => Some(Self::Plan1),
            "plan2" => Some(Self::Plan2),
            "mixed" => Some(Self::Mixed),
            "web" => Some(Self::Web),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// The caption under a cell of the reference app, and what it means. A feature with no plan mark (`desktop`) and a feature the app lacks (`none`) have none.
    fn caption(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Plan1 => Some(("Plans 1 and 2", "the vendor's plan comparison puts the feature in Plan 1 and in Plan 2")),
            Self::Plan2 => {
                Some(("Desktop plan", "the comparison puts the feature in the plan that includes the desktop app, and not in the two web plans"))
            }
            Self::Mixed => Some(("Varies by plan", "parts of the feature are in different plans, and the note says which")),
            Self::Web => Some(("Web app", "the feature is the web app's, or an online service's")),
            Self::Desktop | Self::None => None,
        }
    }
}

/// One source of a cell: a page to link, or a sentence that says what was read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Link(String),
    Text(String),
}

/// A cell of LibreOffice or draw.io.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Evidence {
    value: Value,
    basis: Basis,
    sources: Vec<Source>,
    /// The day the sources were read, `YYYY-MM-DD`.
    read: String,
    note: String,
}

/// A cell of the reference app.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reference {
    value: Value,
    edition: Edition,
    /// What the cell rests on: the sentences of the data's `[sources]`.
    sources: Vec<Source>,
    /// False for a Not yet that rests on `absent` (no page read lists the feature).
    found: bool,
    note: String,
}

/// How a checklist row is scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Score {
    Done,
    Part,
    Missing,
}

/// The DiaCraft cell of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Us {
    /// Computed from the checklist rows the row names, in the data's order.
    Checklist { value: Value, items: Vec<(String, Score)> },
    /// A row that names no checklist row: the value and the document it was read from.
    Manual { value: Value, source: String, note: String },
}

impl Us {
    fn value(&self) -> Value {
        match self {
            Self::Checklist { value, .. } | Self::Manual { value, .. } => *value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    area: usize,
    id: String,
    feature: String,
    reference: Reference,
    libreoffice: Evidence,
    drawio: Evidence,
    diacraft: Us,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Area {
    key: String,
    label: String,
}

/// Whether a tool lacks a feature that only DiaCraft is said to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lacks {
    Yes,
    No,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnlyCell {
    lacks: Lacks,
    basis: Basis,
    sources: Vec<Source>,
    read: String,
    note: String,
}

/// A feature that only DiaCraft has: listed only if it ships and every tool of the page lacks it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Only {
    id: String,
    feature: String,
    /// DiaCraft's own documents that describe it, as paths from the repository's root (a `#fragment` dropped).
    docs: Vec<String>,
    reference: OnlyCell,
    libreoffice: OnlyCell,
    drawio: OnlyCell,
}

/// The data of the page, read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Comparison {
    areas: Vec<Area>,
    rows: Vec<Row>,
    /// The features that qualify, in the data's order.
    only: Vec<Only>,
    /// The day the checklist's scores were last reviewed.
    reviewed: String,
    /// The days the vendor's pages were read, from the sentences the reference column rests on.
    reference_read: BTreeSet<String>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Reading the data

fn err(at: &str, message: &str) -> String {
    format!("{MATRIX}: {at}: {message}")
}

/// A string of the data is short and holds no control character: the page writes it between tags, and an address in a link.
fn clean(value: &str, key: &str, at: &str) -> Result<(), String> {
    if value.chars().count() > MAX_TEXT || value.chars().any(char::is_control) {
        return Err(err(at, &format!("`{key}` is more than {MAX_TEXT} characters or holds a control character")));
    }
    Ok(())
}

fn text_of<'a>(table: &'a Table, key: &str, at: &str) -> Result<&'a str, String> {
    let value = table.get(key).and_then(Toml::as_str).ok_or_else(|| err(at, &format!("expected a string `{key}`")))?;
    clean(value, key, at)?;
    Ok(value)
}

fn table_of<'a>(table: &'a Table, key: &str, at: &str) -> Result<&'a Table, String> {
    table.get(key).and_then(Toml::as_table).ok_or_else(|| err(at, &format!("expected a table `{key}`")))
}

fn strings_of(table: &Table, key: &str, at: &str) -> Result<Vec<String>, String> {
    let items = table.get(key).and_then(Toml::as_array).ok_or_else(|| err(at, &format!("expected a list of strings `{key}`")))?;
    if items.len() > MAX_LIST {
        return Err(err(at, &format!("`{key}` holds more than {MAX_LIST} items")));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let value = item.as_str().ok_or_else(|| err(at, &format!("`{key}` holds a value that is not a string")))?;
        clean(value, key, at)?;
        out.push(value.to_owned());
    }
    Ok(out)
}

/// The `[[name]]` tables of the data; none where the file has no such key and `optional` says it may not.
fn tables_of<'a>(root: &'a Table, name: &str, most: usize, optional: bool) -> Result<Vec<&'a Table>, String> {
    if optional && !root.contains_key(name) {
        return Ok(Vec::new());
    }
    let items = root.get(name).and_then(Toml::as_array).ok_or_else(|| err(name, &format!("expected `[[{name}]]` tables")))?;
    if items.len() > most {
        return Err(err(name, &format!("more than {most} `[[{name}]]` tables")));
    }
    items.iter().map(|item| item.as_table().ok_or_else(|| err(name, &format!("`{name}` holds a value that is not a table")))).collect()
}

fn value_of(table: &Table, key: &str, at: &str) -> Result<Value, String> {
    let text = text_of(table, key, at)?;
    Value::parse(text).ok_or_else(|| err(at, &format!("`{key}` is `{text}`: it is Full, Partial, Not yet or unknown")))
}

fn basis_of(table: &Table, at: &str) -> Result<Basis, String> {
    match text_of(table, "basis", at)? {
        "documented" => Ok(Basis::Documented),
        "oracle" => Ok(Basis::Oracle),
        "absent" => Ok(Basis::Absent),
        "unknown" => Ok(Basis::Unknown),
        other => Err(err(at, &format!("`basis` is `{other}`: it is documented, oracle, absent or unknown"))),
    }
}

/// A day as the data writes it, `YYYY-MM-DD`, that exists.
fn date_of(raw: &str, at: &str) -> Result<String, String> {
    match raw.parse::<jiff::civil::Date>() {
        Ok(date) if date.to_string() == raw => Ok(raw.to_owned()),
        _ => Err(err(at, &format!("`{raw}` is not a day written YYYY-MM-DD"))),
    }
}

/// An address the page may link: HTTPS, on GitHub or on a host of the documentation (`SOURCE_HOSTS`), with nothing in it that ends an attribute or a line.
fn link_ok(url: &str, at: &str) -> Result<(), String> {
    let host = url.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or("");
    if host != "github.com" && !SOURCE_HOSTS.contains(&host) {
        return Err(err(at, &format!("the source `{url}` is not an HTTPS address on github.com or on {}", SOURCE_HOSTS.join(", "))));
    }
    if url.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\')) {
        return Err(err(at, &format!("the source `{url}` holds a character that cannot be in a link")));
    }
    Ok(())
}

/// A source of the data: an address, `oracle: <what was read>` (LibreOffice run as a black box), or a key of `[sources]`.
fn source_of(raw: &str, keys: &BTreeMap<String, String>, at: &str) -> Result<Source, String> {
    if raw.starts_with("https://") {
        link_ok(raw, at)?;
        return Ok(Source::Link(raw.to_owned()));
    }
    if let Some(what) = raw.strip_prefix("oracle: ") {
        return Ok(Source::Text(what.to_owned()));
    }
    keys.get(raw)
        .map(|text| Source::Text(text.clone()))
        .ok_or_else(|| err(at, &format!("the source `{raw}` is no address, no `oracle: ...` and no key of [sources]")))
}

fn sources_of(table: &Table, keys: &BTreeMap<String, String>, at: &str) -> Result<Vec<Source>, String> {
    strings_of(table, "sources", at)?.iter().map(|raw| source_of(raw, keys, at)).collect()
}

fn evidence(table: &Table, keys: &BTreeMap<String, String>, at: &str) -> Result<Evidence, String> {
    let value = value_of(table, "value", at)?;
    let basis = basis_of(table, at)?;
    if (value == Value::Unknown) != (basis == Basis::Unknown) {
        return Err(err(at, "a cell is unknown if and only if its basis is unknown"));
    }
    if basis == Basis::Absent && value != Value::NotYet {
        return Err(err(at, "a cell whose basis is absent says Not yet: the feature was not found"));
    }
    let sources = sources_of(table, keys, at)?;
    if sources.is_empty() && basis != Basis::Unknown {
        return Err(err(at, "a cell that says something names its sources"));
    }
    Ok(Evidence { value, basis, sources, read: date_of(text_of(table, "read", at)?, at)?, note: text_of(table, "note", at)?.to_owned() })
}

fn reference_of(table: &Table, keys: &BTreeMap<String, String>, at: &str) -> Result<Reference, String> {
    let value = value_of(table, "value", at)?;
    if value == Value::Unknown {
        return Err(err(at, "a cell of the reference column is Full, Partial or Not yet"));
    }
    let edition = text_of(table, "edition", at)?;
    let edition =
        Edition::parse(edition).ok_or_else(|| err(at, &format!("`edition` is `{edition}`: it is desktop, plan1, plan2, mixed, web or none")))?;
    let names: Vec<&str> = text_of(table, "source", at)?.split(',').map(str::trim).collect();
    let mut sources = Vec::new();
    for name in &names {
        let text = keys.get(*name).ok_or_else(|| err(at, &format!("the source `{name}` is no key of [sources]")))?;
        sources.push(Source::Text(text.clone()));
    }
    let found = !names.contains(&"absent");
    if !found && value != Value::NotYet {
        return Err(err(at, "a cell that rests on `absent` says Not yet"));
    }
    Ok(Reference { value, edition, sources, found, note: text_of(table, "note", at)?.to_owned() })
}

/// The text of a checklist row's Feature cell as the data names it: a Markdown link counts as its text.
fn link_text(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut rest = cell;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        let Some(close) = after.find("](") else { break };
        let Some(end) = after.get(close + 2..).and_then(|tail| tail.find(')')) else { break };
        out.push_str(before);
        out.push_str(after.get(1..close).unwrap_or(""));
        rest = after.get(close + 2 + end + 1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// The score of each checklist row by the name of its feature. Two rows with one name cannot be told apart, and are an error.
fn scores_of(checklist: &str) -> Result<BTreeMap<String, Score>, String> {
    let mut scores = BTreeMap::new();
    for (_, feature, _, value) in xtask::parity::checklist_rows(checklist)? {
        let score = if value >= 1.0 {
            Score::Done
        } else if value > 0.0 {
            Score::Part
        } else {
            Score::Missing
        };
        if scores.insert(link_text(&feature), score).is_some() {
            return Err(format!("{CHECKLIST}: two rows are named `{}`, and the comparison cannot tell them apart", link_text(&feature)));
        }
    }
    Ok(scores)
}

/// The value the rule gives a row from the scores of its checklist rows: Full if all are done, Not yet if all are missing, Partial otherwise.
fn computed(items: &[(String, Score)]) -> Value {
    if items.iter().all(|(_, score)| *score == Score::Done) {
        Value::Full
    } else if items.iter().all(|(_, score)| *score == Score::Missing) {
        Value::NotYet
    } else {
        Value::Partial
    }
}

/// A path of the docs the data names (`docs/cli.md`, with a `#fragment` that the page drops), which must be a file of the repository.
fn doc_path(raw: &str, exists: &dyn Fn(&str) -> bool, at: &str) -> Result<String, String> {
    let path = raw.split('#').next().unwrap_or("");
    let plain = path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'));
    if !path.starts_with("docs/") || !path.ends_with(".md") || path.contains("..") || !plain || !exists(path) {
        return Err(err(at, &format!("`{raw}` is not a Markdown file of docs/ in the repository")));
    }
    Ok(path.to_owned())
}

/// The DiaCraft cell of a row, and the checklist rows it used (each may be in one row only).
fn us_of(row: &Table, scores: &BTreeMap<String, Score>, used: &mut BTreeSet<String>, exists: &dyn Fn(&str) -> bool, at: &str) -> Result<Us, String> {
    let rule = table_of(row, "diacraft", at)?;
    let names = strings_of(row, "checklist", at)?;
    match text_of(rule, "rule", at)? {
        "checklist" => {
            if names.is_empty() {
                return Err(err(at, "a row computed from the checklist names at least one checklist row"));
            }
            let mut items = Vec::with_capacity(names.len());
            for name in names {
                let score = scores.get(&name).ok_or_else(|| err(at, &format!("the checklist has no row named `{name}`")))?;
                if !used.insert(name.clone()) {
                    return Err(err(at, &format!("the checklist row `{name}` is in two rows")));
                }
                items.push((name, *score));
            }
            Ok(Us::Checklist { value: computed(&items), items })
        }
        "manual" => {
            if !names.is_empty() {
                return Err(err(at, "a row with a manual value names no checklist row"));
            }
            let value = value_of(rule, "value", at)?;
            if value == Value::Unknown {
                return Err(err(at, "a DiaCraft cell is Full, Partial or Not yet"));
            }
            let source = doc_path(text_of(rule, "source", at)?, exists, at)?;
            Ok(Us::Manual { value, source, note: text_of(rule, "note", at)?.to_owned() })
        }
        other => Err(err(at, &format!("`rule` is `{other}`: it is checklist or manual"))),
    }
}

/// A row id: the address the row has on the page, and the name a correction cites.
fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !id.starts_with('-')
}

struct Inputs<'a> {
    areas: &'a [Area],
    keys: &'a BTreeMap<String, String>,
    scores: &'a BTreeMap<String, Score>,
    exists: &'a dyn Fn(&str) -> bool,
}

fn row_of(table: &Table, inputs: &Inputs<'_>, used: &mut BTreeSet<String>, ids: &mut BTreeSet<String>) -> Result<Row, String> {
    let id = text_of(table, "id", "a row")?;
    let at = format!("row `{id}`");
    if !id_ok(id) || !ids.insert(id.to_owned()) {
        return Err(err(&at, "an id is lower-case letters, digits and hyphens, and one row has it"));
    }
    let area = text_of(table, "area", &at)?;
    let area = inputs.areas.iter().position(|a| a.key == area).ok_or_else(|| err(&at, &format!("the area `{area}` is not one of the [[area]]")))?;
    let feature = text_of(table, "feature", &at)?;
    if feature.is_empty() {
        return Err(err(&at, "a row has a feature name"));
    }
    Ok(Row {
        area,
        id: id.to_owned(),
        feature: feature.to_owned(),
        reference: reference_of(table_of(table, "reference", &at)?, inputs.keys, &at)?,
        libreoffice: evidence(table_of(table, "libreoffice", &at)?, inputs.keys, &format!("{at}, LibreOffice"))?,
        drawio: evidence(table_of(table, "drawio", &at)?, inputs.keys, &format!("{at}, draw.io"))?,
        diacraft: us_of(table, inputs.scores, used, inputs.exists, &at)?,
    })
}

fn lacks_of(table: &Table, at: &str) -> Result<Lacks, String> {
    match text_of(table, "lacks", at)? {
        "yes" => Ok(Lacks::Yes),
        "no" => Ok(Lacks::No),
        "unknown" => Ok(Lacks::Unknown),
        other => Err(err(at, &format!("`lacks` is `{other}`: it is yes, no or unknown"))),
    }
}

fn only_cell(table: &Table, keys: &BTreeMap<String, String>, at: &str) -> Result<OnlyCell, String> {
    let basis = basis_of(table, at)?;
    Ok(OnlyCell {
        lacks: lacks_of(table, at)?,
        basis,
        sources: sources_of(table, keys, at)?,
        read: date_of(text_of(table, "read", at)?, at)?,
        note: text_of(table, "note", at)?.to_owned(),
    })
}

fn only_of(table: &Table, keys: &BTreeMap<String, String>, exists: &dyn Fn(&str) -> bool) -> Result<(bool, Only), String> {
    let id = text_of(table, "id", "an item that only DiaCraft has")?;
    let at = format!("item `{id}`");
    let ships = table.get("ships").and_then(Toml::as_bool).ok_or_else(|| err(&at, "expected `ships = true` or `false`"))?;
    let docs = strings_of(table, "diacraft_sources", &at)?.iter().map(|raw| doc_path(raw, exists, &at)).collect::<Result<Vec<_>, _>>()?;
    let only = Only {
        id: id.to_owned(),
        feature: text_of(table, "feature", &at)?.to_owned(),
        docs,
        reference: only_cell(table_of(table, "reference", &at)?, keys, &format!("{at}, reference"))?,
        libreoffice: only_cell(table_of(table, "libreoffice", &at)?, keys, &format!("{at}, LibreOffice"))?,
        drawio: only_cell(table_of(table, "drawio", &at)?, keys, &format!("{at}, draw.io"))?,
    };
    Ok((ships, only))
}

fn areas_of(root: &Table) -> Result<Vec<Area>, String> {
    let mut areas: Vec<Area> = Vec::new();
    for table in tables_of(root, "area", MAX_AREAS, false)? {
        let key = text_of(table, "key", "an area")?;
        let label = text_of(table, "label", &format!("area `{key}`"))?;
        if !id_ok(key) || label.is_empty() || areas.iter().any(|a| a.key == key) {
            return Err(err(&format!("area `{key}`"), "an area has a key of lower-case letters, digits and hyphens that no other has, and a label"));
        }
        areas.push(Area { key: key.to_owned(), label: label.to_owned() });
    }
    Ok(areas)
}

/// The sentences of `[sources]`, by key.
fn keys_of(root: &Table) -> Result<BTreeMap<String, String>, String> {
    let table = table_of(root, "sources", "the file")?;
    let mut keys = BTreeMap::new();
    for key in table.keys() {
        keys.insert(key.clone(), text_of(table, key, "[sources]")?.to_owned());
    }
    Ok(keys)
}

/// The days named by `read YYYY-MM-DD` in `text`.
fn days_read(text: &str) -> Vec<String> {
    text.match_indices("read ")
        .filter_map(|(at, word)| text.get(at + word.len()..at + word.len() + 10))
        .filter(|day| day.parse::<jiff::civil::Date>().is_ok_and(|date| date.to_string() == *day))
        .map(str::to_owned)
        .collect()
}

/// The day the checklist's scores were last reviewed: its status line, `> **Last reviewed:** YYYY-MM-DD`.
fn reviewed_of(checklist: &str) -> Result<String, String> {
    let day = checklist.lines().find_map(|line| line.strip_prefix("> **Last reviewed:** ")).map(str::trim).unwrap_or("");
    date_of(day, CHECKLIST).map_err(|_| format!("{CHECKLIST}: no status line `> **Last reviewed:** YYYY-MM-DD`"))
}

/// Every checklist row is in a row of the data, once.
fn covered(scores: &BTreeMap<String, Score>, used: &BTreeSet<String>) -> Result<(), String> {
    let missing: Vec<&str> = scores.keys().filter(|name| !used.contains(*name)).map(String::as_str).take(10).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!("{MATRIX}: the checklist has rows that no row of the comparison names (add each to the row it belongs to): {}", missing.join("; ")))
}

/// The data of the page, from the text of the data file and of the checklist. `exists` says whether a path of the repository is a file.
pub(super) fn build(matrix: &str, checklist: &str, exists: &dyn Fn(&str) -> bool) -> Result<Comparison, String> {
    let root: Table = matrix.parse().map_err(|e| format!("{MATRIX}: {e}"))?;
    if root.get("version").and_then(Toml::as_integer) != Some(1) {
        return Err(err("the file", "expected `version = 1`"));
    }
    if strings_of(&root, "values", "the file")? != ["Full", "Partial", "Not yet", "unknown"] {
        return Err(err("the file", "`values` is Full, Partial, Not yet and unknown, in that order"));
    }
    let keys = keys_of(&root)?;
    let areas = areas_of(&root)?;
    let scores = scores_of(checklist)?;
    let inputs = Inputs { areas: &areas, keys: &keys, scores: &scores, exists };
    let (mut used, mut ids, mut rows) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    for table in tables_of(&root, "row", MAX_ROWS, false)? {
        rows.push(row_of(table, &inputs, &mut used, &mut ids)?);
    }
    covered(&scores, &used)?;
    if let Some(empty) = areas.iter().enumerate().find(|(at, _)| !rows.iter().any(|row| row.area == *at)) {
        return Err(err(&format!("area `{}`", empty.1.key), "an area has at least one row"));
    }
    let mut only = Vec::new();
    for table in tables_of(&root, "only", MAX_ROWS, true)? {
        let (ships, item) = only_of(table, &keys, exists)?;
        let every = [item.reference.lacks, item.libreoffice.lacks, item.drawio.lacks].iter().all(|lacks| *lacks == Lacks::Yes);
        if ships && every {
            only.push(item);
        }
    }
    let mut reference_read = BTreeSet::new();
    for row in &rows {
        for source in &row.reference.sources {
            if let Source::Text(text) = source {
                reference_read.extend(days_read(text));
            }
        }
    }
    if reference_read.is_empty() {
        return Err(err("[sources]", "the sources of the reference column name no day (`read YYYY-MM-DD`)"));
    }
    Ok(Comparison { areas, rows, only, reviewed: reviewed_of(checklist)?, reference_read })
}

/// The data of the repository at `root`: the data file and the checklist, each read to a bound.
pub(super) fn load(root: &Path) -> Result<Comparison, String> {
    let read = |name: &str| -> Result<String, String> {
        let bytes = read_bounded(&root.join(name), MAX_FILE)?;
        String::from_utf8(bytes).map_err(|_| format!("{name}: expected UTF-8"))
    };
    build(&read(MATRIX)?, &read(CHECKLIST)?, &|path| root.join(path).is_file())
}

impl Comparison {
    /// How many rows each column has with each value: (Full, Partial, Not yet, unknown), for the reference app, draw.io, LibreOffice and DiaCraft.
    pub(super) fn counts(&self) -> [[usize; 4]; 4] {
        let mut counts = [[0; 4]; 4];
        let slot = |value: Value| match value {
            Value::Full => 0,
            Value::Partial => 1,
            Value::NotYet => 2,
            Value::Unknown => 3,
        };
        for row in &self.rows {
            for (column, value) in [row.reference.value, row.drawio.value, row.libreoffice.value, row.diacraft.value()].into_iter().enumerate() {
                if let Some(count) = counts.get_mut(column).and_then(|c| c.get_mut(slot(value))) {
                    *count += 1;
                }
            }
        }
        counts
    }

    /// The line `cargo xtask site` prints for the page.
    pub(super) fn line(&self) -> String {
        let [_, _, _, us] = self.counts();
        format!(
            "site: feature comparison: {} rows in {} areas, {} features only DiaCraft has; DiaCraft is Full in {}, Partial in {} and Not yet in {}",
            self.rows.len(),
            self.areas.len(),
            self.only.len(),
            us[0],
            us[1],
            us[2]
        )
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Making the page's regions

/// `text` as the text of an element or the value of an attribute in double quotes.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// `YYYY-MM-DD` as `26 November 2026` (`long`) or `26 Nov 2026`; the day itself where it cannot be written.
fn day(raw: &str, long: bool) -> String {
    let format = if long { "%-d %B %Y" } else { "%-d %b %Y" };
    raw.parse::<jiff::civil::Date>().ok().and_then(|date| jiff::fmt::strtime::format(format, date).ok()).unwrap_or_else(|| raw.to_owned())
}

/// `26 October 2026`, `26 October and 26 November 2026`, `1 October, 2 October and 3 November 2026`: the days in one phrase.
fn days_phrase(days: &BTreeSet<String>) -> String {
    let words: Vec<String> = days.iter().map(|d| day(d, true)).collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `text` with its first letter a capital, but for the name of draw.io, which is written in lower case wherever it stands.
fn capital(text: &str) -> String {
    if text.starts_with(DRAWIO) {
        return text.to_owned();
    }
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// What the page needs to write itself: the data, the repository the links go to, the release, and the names of the tools.
struct View<'a> {
    cmp: &'a Comparison,
    repository: &'a str,
    version: &'a str,
    /// The reference app, as a page of the site may name it.
    reference: String,
}

const DRAWIO: &str = "draw.io";
const LIBREOFFICE: &str = "LibreOffice Draw";
const OURS: &str = "DiaCraft";

/// The mark of a value: a shape that differs for each (a disc, a half disc, a ring, a dashed ring, a question mark) and the word, so that colour is never the only difference. A Not yet that rests
/// on a documentation that does not mention the feature says so under the word.
fn mark(value: Value, found: bool) -> String {
    match (value, found) {
        (Value::Full, _) => r#"<span class="cmp-mark cmp-full"><i aria-hidden="true"></i>Full</span>"#.to_owned(),
        (Value::Partial, _) => r#"<span class="cmp-mark cmp-part"><i aria-hidden="true"></i>Partial</span>"#.to_owned(),
        (Value::NotYet, true) => r#"<span class="cmp-mark cmp-no"><i aria-hidden="true"></i>Not yet</span>"#.to_owned(),
        (Value::NotYet, false) => {
            r#"<span class="cmp-mark cmp-no cmp-nf"><i aria-hidden="true"></i>Not yet</span><span class="cmp-sub">not found in docs</span>"#
                .to_owned()
        }
        (Value::Unknown, _) => {
            r#"<span class="cmp-mark cmp-unk"><i aria-hidden="true">?</i><span class="visually-hidden">Unknown</span></span>"#.to_owned()
        }
    }
}

/// An address as the text of its link: the host without `www.`, and the last part of the path after an ellipsis when the path is long.
fn link_label(url: &str) -> String {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.trim_end_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [] | [""] => host.to_owned(),
        [_, _] | [_] => format!("{host}/{path}"),
        [.., last] => format!("{host}/…/{last}"),
    }
}

/// A details element of a cell: the label, and what it opens.
fn why(label: &str, body: &str) -> String {
    format!("<details class=\"cmp-why\"><summary>{}</summary>{body}</details>", esc(label))
}

impl View<'_> {
    fn td(&self, column: &str, class: &str, inner: &str) -> String {
        format!("<td role=\"cell\" data-col=\"{}\" class=\"{class}\">{inner}</td>", esc(column))
    }

    /// The link to a file of the repository on GitHub, marked as `cargo xtask site --write` marks the links it keeps current.
    fn repo_link(&self, path: &str, text: &str) -> String {
        format!(
            "<a data-repo=\"/blob/HEAD/{}\" href=\"https://github.com/{}/blob/HEAD/{}\">{}</a>",
            esc(path),
            esc(self.repository),
            esc(path),
            esc(text)
        )
    }

    /// A sentence that says what was read, with the name of the reference app where the data says "the reference app".
    fn sentence(&self, text: &str) -> String {
        esc(&capital(&text.replace("the reference app", &self.reference)))
    }

    /// The list of sources of a cell: an address is a link, a sentence is text.
    fn source_list(&self, sources: &[Source]) -> String {
        if sources.is_empty() {
            return String::new();
        }
        let mut out = String::from("<ul class=\"cmp-src\">");
        for source in sources {
            match source {
                Source::Link(url) => out.push_str(&format!("<li><a href=\"{}\">{}</a></li>", esc(url), esc(&link_label(url)))),
                Source::Text(text) => out.push_str(&format!("<li>{}</li>", self.sentence(text))),
            }
        }
        out.push_str("</ul>");
        out
    }

    fn reference_cell(&self, cell: &Reference) -> String {
        let mut inner = mark(cell.value, cell.found);
        if let Some((caption, _)) = cell.edition.caption() {
            inner.push_str(&format!("<span class=\"cmp-sub\">{caption}</span>"));
        }
        if !cell.note.is_empty() {
            inner.push_str(&why("Note", &format!("<p>{}</p>{}", self.sentence(&cell.note), self.source_list(&cell.sources))));
        }
        self.td(&self.reference, "cmp-ref", &inner)
    }

    fn evidence_cell(&self, column: &str, class: &str, cell: &Evidence) -> String {
        let mut inner = mark(cell.value, cell.basis != Basis::Absent);
        let label = if cell.sources.iter().any(|s| matches!(s, Source::Link(_))) { "Sources" } else { "Note" };
        let note = if cell.note.is_empty() { String::new() } else { format!("<p>{}</p>", self.sentence(&cell.note)) };
        let body = format!("{note}{}<p class=\"cmp-read\">Read {}</p>", self.source_list(&cell.sources), day(&cell.read, false));
        inner.push_str(&why(label, &body));
        self.td(column, class, &inner)
    }

    /// The DiaCraft cell of the row `id`: the mark, what the checklist says in numbers, the link to it, and the row's id, which a correction can name.
    fn us_cell(&self, id: &str, us: &Us) -> String {
        let mut inner = mark(us.value(), true);
        let named = format!("<p class=\"cmp-read\">Row id {}</p>", esc(id));
        match us {
            Us::Checklist { items, .. } => {
                let count = |wanted: Score| items.iter().filter(|(_, score)| *score == wanted).count();
                let (done, part, missing) = (count(Score::Done), count(Score::Part), count(Score::Missing));
                let rows = if items.len() == 1 { "row" } else { "rows" };
                let body = format!(
                    "<p>{done} done, {part} in part and {missing} not yet, of the {} checklist {rows} this row covers.</p><p>{}</p>{named}",
                    items.len(),
                    self.repo_link(CHECKLIST, "The full checklist")
                );
                inner.push_str(&why(&format!("{done} of {} done", items.len()), &body));
            }
            Us::Manual { source, note, .. } => {
                let body = format!("<p>{}</p><p>{}</p>{named}", self.sentence(note), self.repo_link(source, "Where it is read from"));
                inner.push_str(&why("Source", &body));
            }
        }
        self.td(OURS, "cmp-us", &inner)
    }

    fn row(&self, row: &Row) -> String {
        format!(
            "<tr role=\"row\"><th scope=\"row\" role=\"rowheader\" id=\"{}\">{}</th>{}{}{}{}</tr>\n",
            esc(&row.id),
            esc(&row.feature),
            self.reference_cell(&row.reference),
            self.evidence_cell(DRAWIO, "cmp-io", &row.drawio),
            self.evidence_cell(LIBREOFFICE, "cmp-lo", &row.libreoffice),
            self.us_cell(&row.id, &row.diacraft)
        )
    }

    /// The head of every table: the tools, in the order of the columns. The name of each is bold, which sets it apart, and LibreOffice carries its mark.
    fn thead(&self) -> String {
        format!(
            "<thead role=\"rowgroup\"><tr role=\"row\"><th scope=\"col\" role=\"columnheader\">Feature</th><th scope=\"col\" role=\"columnheader\"><b>{}</b></th>\
             <th scope=\"col\" role=\"columnheader\"><b>{DRAWIO}</b></th><th scope=\"col\" role=\"columnheader\"><b>LibreOffice®</b> Draw</th>\
             <th scope=\"col\" role=\"columnheader\" class=\"cmp-us\"><b>{OURS}</b></th></tr></thead>",
            esc(&self.reference)
        )
    }

    fn area(&self, at: usize, area: &Area) -> String {
        let rows: Vec<&Row> = self.cmp.rows.iter().filter(|row| row.area == at).collect();
        let label = esc(&area.label);
        let mut out = format!(
            "<section class=\"cmp-area\" id=\"area-{key}\"><details open=\"open\"><summary><h2>{label}</h2><span class=\"cmp-count\">{n} rows</span></summary>\n\
             <div class=\"cmp-scroll\" role=\"region\" aria-label=\"{label}: the comparison\" tabindex=\"0\">\n\
             <table class=\"cmp-table\" role=\"table\"><caption class=\"visually-hidden\">{label}: each feature in {reference}, {DRAWIO}, {LIBREOFFICE} and {OURS}</caption>\n{head}\n<tbody role=\"rowgroup\">\n",
            key = esc(&area.key),
            n = rows.len(),
            reference = esc(&self.reference),
            head = self.thead()
        );
        for row in rows {
            out.push_str(&self.row(row));
        }
        out.push_str("</tbody></table>\n</div></details></section>\n");
        out
    }

    fn areas(&self) -> String {
        self.cmp.areas.iter().enumerate().map(|(at, area)| self.area(at, area)).collect()
    }

    fn toc(&self) -> String {
        let mut out = String::new();
        for area in &self.cmp.areas {
            out.push_str(&format!("<li><a href=\"#area-{}\"><span>{}</span></a></li>\n", esc(&area.key), esc(&area.label)));
        }
        out.push_str("<li><a href=\"#only\"><span>Only in DiaCraft</span></a></li>\n");
        out
    }

    fn facts(&self) -> String {
        let mut days: BTreeSet<String> = self.cmp.reference_read.clone();
        for row in &self.cmp.rows {
            days.insert(row.libreoffice.read.clone());
            days.insert(row.drawio.read.clone());
        }
        let read: Vec<String> = days.iter().map(|d| day(d, false)).collect();
        format!(
            "<div><dt>Rows</dt><dd><b>{}</b></dd></div>\n<div><dt>Areas</dt><dd><b>{}</b></dd></div>\n<div><dt>Sources read</dt><dd>{}</dd></div>\n<div><dt>DiaCraft</dt><dd>Release <b>{}</b></dd></div>\n",
            self.cmp.rows.len(),
            self.cmp.areas.len(),
            esc(&read.join(" · ")),
            esc(self.version)
        )
    }

    /// The line that says where the cells come from and when they were read, and where to report a correction. It names the copy of
    /// LibreOffice Draw run as a black box only when a cell of it, in a row or in the list of what only DiaCraft has, rests on one.
    fn asof(&self) -> String {
        let (mut io, mut lo) = (BTreeSet::new(), BTreeSet::new());
        for row in &self.cmp.rows {
            io.insert(row.drawio.read.clone());
            lo.insert(row.libreoffice.read.clone());
        }
        let others: BTreeSet<String> = io.union(&lo).cloned().collect();
        let all: BTreeSet<String> = others.union(&self.cmp.reference_read).cloned().collect();
        let black_box = self.cmp.rows.iter().any(|row| row.libreoffice.basis == Basis::Oracle)
            || self.cmp.only.iter().any(|item| item.libreoffice.basis == Basis::Oracle);
        let from = if black_box {
            format!("public documentation or, for some of {LIBREOFFICE}'s, from a copy of it run as a black box")
        } else {
            "public documentation".to_owned()
        };
        format!(
            "<p class=\"cmp-asof\"><strong>As of {all_phrase}.</strong> Every cell comes from {from}: the pages of {reference} were read on {reference_phrase}, \
             and those of {DRAWIO} and {LIBREOFFICE} on {others_phrase}. The {OURS} column is computed from its parity checklist, whose scores were last reviewed on {reviewed}, for release {version}. \
             <a data-repo=\"/issues/new\" href=\"https://github.com/{repository}/issues/new\">Report a correction</a> on GitHub with the row, the cell and a public source.</p>\n",
            all_phrase = days_phrase(&all),
            others_phrase = days_phrase(&others),
            reference = esc(&self.reference),
            reference_phrase = days_phrase(&self.cmp.reference_read),
            reviewed = day(&self.cmp.reviewed, true),
            version = esc(self.version),
            repository = esc(self.repository)
        )
    }

    /// What each caption under a cell of the reference app means, for the captions the table uses.
    fn editions(&self) -> String {
        let used: BTreeSet<Edition> = self.cmp.rows.iter().map(|row| row.reference.edition).collect();
        let mut out = String::new();
        for (caption, meaning) in used.into_iter().filter_map(Edition::caption) {
            out.push_str(&format!("<li><strong>{caption}:</strong> {}.</li>\n", esc(meaning)));
        }
        out
    }

    /// What the other tools say of one feature that only DiaCraft has.
    fn only_tool(&self, name: &str, cell: &OnlyCell) -> String {
        let note = if cell.note.is_empty() { String::new() } else { format!("{}. ", self.sentence(&cell.note)) };
        format!(
            "<div><dt>{}</dt><dd>{note}{}<span class=\"cmp-read\">Read {}</span></dd></div>",
            esc(name),
            self.source_list(&cell.sources),
            day(&cell.read, false)
        )
    }

    fn only_item(&self, item: &Only) -> String {
        let docs: Vec<String> = item.docs.iter().map(|path| self.repo_link(path, path.strip_prefix("docs/").unwrap_or(path))).collect();
        format!(
            "<li id=\"{}\"><p class=\"cmp-only-name\">{}</p><dl class=\"cmp-others\">{}{}{}</dl><p class=\"cmp-docs\">In DiaCraft: {}</p></li>\n",
            esc(&item.id),
            esc(&item.feature),
            self.only_tool(&self.reference, &item.reference),
            self.only_tool(DRAWIO, &item.drawio),
            self.only_tool(LIBREOFFICE, &item.libreoffice),
            docs.join(" · ")
        )
    }

    fn only(&self) -> String {
        self.cmp.only.iter().map(|item| self.only_item(item)).collect()
    }
}

/// The name of the reference app as a page of the site writes it: the one name of the deny list that a page may hold, with a capital.
fn reference_name() -> Result<String, String> {
    let name = xtask::brands::SITE_PAGE_NAMES.first().ok_or("the repository lists no name that a page of the site may hold")?;
    Ok(capital(name))
}

/// The regions of the page and what goes in each, as (name, lines).
fn regions(cmp: &Comparison, settings: &Settings) -> Result<Vec<(&'static str, String)>, String> {
    let view = View { cmp, repository: &settings.repository, version: &settings.version, reference: reference_name()? };
    Ok(vec![
        ("facts", view.facts()),
        ("asof", view.asof()),
        ("editions", view.editions()),
        ("toc", view.toc()),
        ("areas", view.areas()),
        ("only", view.only()),
    ])
}

/// `source` with what is between the marker lines `<!-- name:begin -->` and `<!-- name:end -->` replaced by `body`. The markers are on lines of their own, each once, the beginning first.
fn replace_region(source: &str, name: &str, body: &str) -> Result<String, String> {
    let (begin, end) = (format!("<!-- {name}:begin -->"), format!("<!-- {name}:end -->"));
    let line_of = |marker: &str| -> Result<usize, String> {
        let mut found = source.match_indices(marker).map(|(at, _)| at);
        let at = found.next().ok_or_else(|| format!("{PAGE}: the page has no `{marker}` line"))?;
        if found.next().is_some() {
            return Err(format!("{PAGE}: the page has `{marker}` twice"));
        }
        let before = source.get(..at).unwrap_or("");
        if !before.is_empty() && !before.trim_end_matches([' ', '\t']).ends_with('\n') {
            return Err(format!("{PAGE}: `{marker}` is not at the start of a line"));
        }
        Ok(at)
    };
    let (from, to) = (line_of(&begin)?, line_of(&end)?);
    if from >= to {
        return Err(format!("{PAGE}: `{end}` comes before `{begin}`"));
    }
    let after_begin =
        source.get(from..).and_then(|tail| tail.find('\n')).map(|at| from + at + 1).ok_or_else(|| format!("{PAGE}: `{begin}` ends the file"))?;
    let end_line = source.get(..to).map(|head| head.trim_end_matches([' ', '\t']).len()).unwrap_or(to);
    let head = source.get(..after_begin).ok_or_else(|| format!("{PAGE}: cannot cut at `{begin}`"))?;
    let tail = source.get(end_line..).ok_or_else(|| format!("{PAGE}: cannot cut at `{end}`"))?;
    Ok(format!("{head}{body}{tail}"))
}

/// The page `source` with its regions made from `comparison` and the settings: the same page when it is current.
pub(super) fn render(source: &str, comparison: &Comparison, settings: &Settings) -> Result<String, String> {
    let mut page = source.to_owned();
    for (name, body) in regions(comparison, settings)? {
        page = replace_region(&page, name, &body)?;
    }
    Ok(page)
}
