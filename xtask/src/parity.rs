//! `cargo xtask parity`: recompute the feature-parity summary in docs/parity-checklist.md from its row
//! table (each feature scored D / P / M, weighted by priority). `parity --check` fails when the file's
//! summary is out of date, when the file does not exist, and when anything under `## Rows` that holds a `|` is
//! neither a row it can read nor a header with its separator, and when it holds no rows at all: a row dropped
//! without a word would lower the feature count and move every percentage. Only a heading of the first or second
//! level ends the rows; `###` and below are inside them.

use std::path::Path;

use crate::ci::Outcome;

const START: &str = "<!-- SUMMARY -->";
const ROWS: &str = "\n## Rows";
const CHECKLIST: &str = "docs/parity-checklist.md";

/// (area, feature, priority 0–2, value 0 / 0.5 / 1) of a table row.
pub type Row = (String, String, u8, f64);

/// How many unreadable rows an error lists before it only counts the rest.
const MOST_LISTED: usize = 20;

/// The rows of `text`, which is the rows section of the checklist: `| Area | Feature | P0/P1/P2 | D/P/M |`. Anything in it that
/// holds a `|` and is neither a row that reads nor a header with its separator is an error that names it, and no rows are
/// returned: see [`rows_from`].
pub fn rows(text: &str) -> Result<Vec<Row>, String> {
    rows_from(text, 1)
}

/// What a table line's cells are, outside its outer pipes (which a row may leave out).
fn cells(line: &str) -> Vec<&str> {
    line.trim().trim_matches('|').split('|').map(str::trim).collect()
}

/// A table's separator line: `|---|:-:|`, with or without its outer pipes.
fn is_separator(line: &str) -> bool {
    let line = line.trim();
    line.contains('|') && line.contains('-') && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// A header: four cells, the first two `Area` and `Feature`, that do not make a row. The other two (`Priority` and `Status`, or
/// `P0/P1/P2` and `D/P/M`) may be worded as the page likes. A feature that is called `Feature` in an area called `Area` is a row,
/// and so is a bad row of any other area.
fn is_header(cells: &[&str]) -> bool {
    matches!(cells, [area, feature, _, _] if area.eq_ignore_ascii_case("area") && feature.eq_ignore_ascii_case("feature"))
        && read_row(cells).is_none()
}

/// The row a line's cells make, `None` when the priority or the status is not one of the letters.
fn read_row(cells: &[&str]) -> Option<Row> {
    let &[area, feature, priority, status] = cells else { return None };
    // A row names what it scores.
    if area.is_empty() || feature.is_empty() {
        return None;
    }
    let pri = match priority {
        "P0" => 0,
        "P1" => 1,
        "P2" => 2,
        _ => return None,
    };
    let v = match status {
        "D" => 1.0,
        "P" => 0.5,
        "M" => 0.0,
        _ => return None,
    };
    Some((area.to_string(), feature.to_string(), pri, v))
}

/// `line`, cut at 120 characters with `…` so one very long line cannot fill an error.
fn clip(line: &str) -> String {
    match line.char_indices().nth(120) {
        Some((i, _)) => format!("{}…", line.get(..i).unwrap_or_default()),
        None => line.to_owned(),
    }
}

/// A line that ends the rows section: a Markdown heading of the first or second level (one or two `#`, then a space or the end
/// of the line) with no `|` in it. A deeper heading (`###` and below) is a sub-heading inside the rows, and a line that holds a
/// `|` is a row or an error whatever it starts with: `# Document | Layers | P1 | M |` is a row.
fn is_heading(line: &str) -> bool {
    let line = line.trim_end();
    if line.contains('|') {
        return false;
    }
    let hashes = line.chars().take_while(|&c| c == '#').count();
    (1..=2).contains(&hashes) && line.get(hashes..).is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace))
}

/// The rows section of `tail`, which starts at `## Rows`: up to the next heading of the first or second level, or the end of the
/// page. Headings below that (`###`) are inside it. What follows it (the revision history, any notes) is prose, and tables of any
/// shape are free there.
fn rows_section(tail: &str) -> &str {
    let mut end = 0;
    // `tail` begins with the newline that ends the line before `## Rows`, and then the heading itself: two lines to pass.
    for (n, line) in tail.split_inclusive('\n').enumerate() {
        if n >= 2 && is_heading(line) {
            return tail.get(..end).unwrap_or(tail);
        }
        end += line.len();
    }
    tail
}

/// [`rows`], with `first_line` the number of the first line of `text` in its file, for the error.
///
/// Every line of `text` that holds a `|` is one of three things, or an error. A **header** is the first line of a table (the line
/// before it holds no `|`), is followed by a separator, and has `Area` and `Feature` for its first two cells; it is skipped. The
/// **separator** directly under a header is skipped. Anything else is a **row**: four cells (the outer pipes may be left out)
/// with a priority of `P0`, `P1` or `P2` and a status of `D`, `P` or `M`. A line that is none of these (a row whose priority or
/// status is something else, a row of three or five cells in any area, a sentence with a pipe in it, a separator that is not
/// under a header) makes the whole reading fail with its line number: nothing is dropped, because a dropped row lowers the
/// count and moves every percentage with the gate still green. Lines with no `|` are free.
fn rows_from(text: &str, first_line: usize) -> Result<Vec<Row>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut bad = Vec::new();
    let mut header_at = None;
    for (i, line) in lines.iter().enumerate() {
        if !line.contains('|') {
            continue;
        }
        let number = first_line + i;
        if is_separator(line) {
            if i == 0 || header_at != Some(i - 1) {
                bad.push((number, line.trim()));
            }
            continue;
        }
        let cells = cells(line);
        let first_of_table = i == 0 || lines.get(i - 1).is_none_or(|before| !before.contains('|'));
        if first_of_table && lines.get(i + 1).is_some_and(|next| is_separator(next)) && is_header(&cells) {
            header_at = Some(i);
            continue;
        }
        match read_row(&cells) {
            Some(row) => out.push(row),
            None => bad.push((number, line.trim())),
        }
    }
    if bad.is_empty() {
        return Ok(out);
    }
    let listed: Vec<String> = bad.iter().take(MOST_LISTED).map(|(n, l)| format!("line {n}: {}", clip(l))).collect();
    let more = bad.len().saturating_sub(MOST_LISTED);
    Err(format!(
        "{CHECKLIST}: {} lines under `## Rows` are not rows (a row is `| Area | Feature | Priority | Status |`: four cells, an area and a feature, a priority of P0, P1 or P2 and a status of D, P or M; \
         the only other lines with a `|` are a header, whose first cells are Area and Feature, and the separator directly under it); \
         one that is dropped would change every percentage:\n  {}{}",
        bad.len(),
        listed.join("\n  "),
        if more > 0 { format!("\n  and {more} more") } else { String::new() }
    ))
}

fn weight(p: u8) -> f64 {
    match p {
        0 => 3.0,
        1 => 2.0,
        _ => 1.0,
    }
}

pub fn summary(rows: &[Row]) -> String {
    let pct = |rs: &[&Row]| {
        let tot: f64 = rs.iter().map(|r| weight(r.2)).sum();
        if tot == 0.0 { 0.0 } else { 100.0 * rs.iter().map(|r| weight(r.2) * r.3).sum::<f64>() / tot }
    };
    let all: Vec<_> = rows.iter().collect();
    let mut s = format!(
        "**Weighted breadth parity: {:.0}%** over {} features.\n\n| Priority | Features | Done | Partial | Missing | Parity |\n|---|---|---|---|---|---|\n",
        pct(&all),
        rows.len()
    );
    for p in 0..3u8 {
        let rr: Vec<_> = rows.iter().filter(|r| r.2 == p).collect();
        let n = |v: f64| rr.iter().filter(|r| r.3 == v).count();
        let unweighted = if rr.is_empty() { 0.0 } else { 100.0 * rr.iter().map(|r| r.3).sum::<f64>() / rr.len() as f64 };
        s += &format!("| P{p} | {} | {} | {} | {} | {unweighted:.0}% |\n", rr.len(), n(1.0), n(0.5), n(0.0));
    }
    s += "\n| Area | Parity |\n|---|---|\n";
    let mut areas: Vec<&str> = Vec::new();
    for r in rows {
        if !areas.contains(&r.0.as_str()) {
            areas.push(&r.0);
        }
    }
    for a in areas {
        let rr: Vec<_> = rows.iter().filter(|r| r.0 == a).collect();
        s += &format!("| {a} | {:.0}% |\n", pct(&rr));
    }
    let open: Vec<&str> = rows.iter().filter(|r| r.2 == 0 && r.3 < 1.0).map(|r| r.1.as_str()).collect();
    s += &format!("\nOpen P0 items: {}.\n", open.join(", "));
    s
}

/// Where the summary starts (the marker) and where the rows start (`## Rows`) in `text`, and the rows.
fn read(text: &str) -> Result<(usize, usize, Vec<Row>), String> {
    let marker_error = || format!("{CHECKLIST} needs a `{START}` marker before `## Rows`");
    let a = text.find(START).ok_or_else(marker_error)?;
    let b = text.find(ROWS).filter(|&b| a < b).ok_or_else(marker_error)?;
    let tail = text.get(b..).ok_or_else(marker_error)?;
    // The first line of `tail` is what follows the last newline before `## Rows`, nothing: it is on the line that newline ends.
    let first_line = text.get(..b).unwrap_or_default().matches('\n').count() + 1;
    let rows = rows_from(rows_section(tail), first_line)?;
    // A checklist of no features would score 0% of nothing, and pass: a deleted table, an emptied section and a heading that hides
    // every row from the reading look the same, and none of them is a checklist.
    if rows.is_empty() {
        return Err(format!(
            "{CHECKLIST} has no rows under `## Rows`: a row is `| Area | Feature | Priority | Status |` with a priority of P0, P1 or P2 and a status of D, P or M"
        ));
    }
    Ok((a, b, rows))
}

/// The rows of a whole checklist file (`docs/parity-checklist.md`), read as `check` reads them: the file needs its `<!-- SUMMARY -->` marker before `## Rows`, a line under `## Rows` that looks
/// like a row and does not read is an error, and no rows is an error. The feature comparison page (`cargo xtask site`) scores its DiaCraft column from them.
pub fn checklist_rows(text: &str) -> Result<Vec<Row>, String> {
    read(text).map(|(_, _, rows)| rows)
}

/// `text` with the summary between `<!-- SUMMARY -->` and `## Rows` recomputed from the rows below. Fails when a line that
/// looks like a row cannot be read ([`rows`]).
pub fn render(text: &str) -> Result<String, String> {
    let (a, b, rows) = read(text)?;
    let marker_error = || format!("{CHECKLIST} needs a `{START}` marker before `## Rows`");
    let head = text.get(..a).ok_or_else(marker_error)?;
    let tail = text.get(b..).ok_or_else(marker_error)?;
    Ok(format!("{head}{START}\n{}{tail}", summary(&rows)))
}

/// Rewrites the summary in `docs/parity-checklist.md`.
pub fn run(root: &Path) -> Result<(), String> {
    let path = root.join(CHECKLIST);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = render(&text)?;
    std::fs::write(&path, &new).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("parity: updated the summary in {CHECKLIST} ({} rows)", read(&new)?.2.len());
    Ok(())
}

/// Fails when the file's summary differs from the recomputed one, when it holds a row that cannot be read, and when
/// there is no file: the checklist has been committed so a missing one is a deletion, and a gate that passes on
/// nothing would pass on it. Never writes.
pub fn check(root: &Path) -> Result<Outcome, String> {
    let path = root.join(CHECKLIST);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{CHECKLIST} does not exist: `parity --check` needs the checklist, which is committed; restore it"));
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if render(&text)? == text {
        Ok(Outcome::Ok)
    } else {
        Err(format!("{CHECKLIST} is out of date: its summary does not match its rows; run `cargo xtask parity` and commit the result"))
    }
}
