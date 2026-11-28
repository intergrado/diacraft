//! The header of a topic: `key: value` lines between two lines of three dashes.
//!
//! The keys are `id`, `title`, `summary` and `keywords`, `unlisted`, and the fields only the website reads: `site`, `path`, `description`, `seo_title` and `picture`. A key
//! the format does not have is a problem and is ignored; a topic with no usable `id` or `title` is refused, since nothing can address or name it.

use std::collections::HashSet;

use crate::inline::valid_id;
use crate::{Code, MAX_HEADER_BYTES, MAX_KEYWORDS, Problem, report};

/// The fields of a topic's header that only the website reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteFields {
    /// `site: yes|no`: whether the topic is a page of the website (default yes).
    pub publish: bool,
    /// `path`: where the page is, `help/<id>` when absent.
    pub path: Option<String>,
    /// `description`: the page's description, 140 to 160 characters, required when published.
    pub description: Option<String>,
    /// `seo_title`: the page's title for search, at most 60 characters.
    pub seo_title: Option<String>,
    /// `picture`: the name of the page's social picture.
    pub picture: Option<String>,
}

impl SiteFields {
    /// Where the page is: `path`, or `help/<id>`.
    pub fn path_or_default(&self, id: &str) -> String {
        self.path.clone().unwrap_or_else(|| format!("help/{id}"))
    }
}

/// The line of each key of a header, 0 for a key that is not there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyLines {
    /// `id`.
    pub id: usize,
    /// `title`.
    pub title: usize,
    /// `summary`.
    pub summary: usize,
    /// `keywords`.
    pub keywords: usize,
    /// `description`.
    pub description: usize,
    /// `seo_title`.
    pub seo_title: usize,
}

/// A topic's header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// `id`: lowercase words with dashes.
    pub id: String,
    /// `title`: at most 80 characters.
    pub title: String,
    /// `summary`: at most 160 characters, the search snippet.
    pub summary: String,
    /// `keywords`: comma-separated.
    pub keywords: Vec<String>,
    /// `unlisted: yes`: a topic the help home does not list.
    pub unlisted: bool,
    /// The fields only the website reads.
    pub site: SiteFields,
    /// The lines the keys are on.
    pub lines: KeyLines,
}

const MAX_TITLE: usize = 80;
const MAX_SUMMARY: usize = 160;
const MAX_SEO_TITLE: usize = 60;
const DESCRIPTION: std::ops::RangeInclusive<usize> = 140..=160;

/// Reads the header of `src`: the header, the rest of the text, and the number of the line that rest starts at. A file with no header, one that is never closed, one over
/// the cap, and one with no usable `id` or `title` are refused.
pub fn read<'a>(src: &'a str, problems: &mut Vec<Problem>) -> Result<(Header, &'a str, usize), Problem> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut pieces = src.split_inclusive('\n');
    if pieces.next().map(|l| l.trim_end()) != Some("---") {
        return Err(Problem::new(1, Code::NoHeader, ""));
    }
    let mut offset = src.split_inclusive('\n').next().map_or(0, str::len);
    let mut pairs: Vec<(usize, &str)> = Vec::new();
    let mut closed_at = None;
    for (n, piece) in pieces.enumerate() {
        offset += piece.len();
        if offset > MAX_HEADER_BYTES {
            return Err(Problem::new(0, Code::HeaderTooLarge, ""));
        }
        if piece.trim_end() == "---" {
            closed_at = Some(n + 2);
            break;
        }
        pairs.push((n + 2, piece.trim_end()));
    }
    let Some(closed_line) = closed_at else { return Err(Problem::new(1, Code::HeaderNotClosed, "")) };
    let header = fields(&pairs, problems)?;
    Ok((header, src.get(offset..).unwrap_or_default(), closed_line + 1))
}

/// The header from its `key: value` lines (with their line numbers).
fn fields(pairs: &[(usize, &str)], problems: &mut Vec<Problem>) -> Result<Header, Problem> {
    let mut header = Header { site: SiteFields { publish: true, ..SiteFields::default() }, ..Header::default() };
    let mut seen: HashSet<&str> = HashSet::new();
    for &(n, line) in pairs {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            report(problems, n, Code::HeaderLine, line);
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !seen.insert(key) {
            report(problems, n, Code::HeaderDuplicateKey, key);
            continue;
        }
        if value.is_empty() {
            report(problems, n, Code::HeaderValue, key);
            continue;
        }
        set_field(&mut header, key, value, n, problems)?;
    }
    require(&header, problems)?;
    Ok(header)
}

/// What the header must have: an `id` and a `title` (without them nothing can address or name the topic), a summary and keywords, and a description when it is published.
fn require(header: &Header, problems: &mut Vec<Problem>) -> Result<(), Problem> {
    if header.lines.id == 0 {
        return Err(Problem::new(0, Code::HeaderMissing, "id"));
    }
    if header.lines.title == 0 {
        return Err(Problem::new(0, Code::HeaderMissing, "title"));
    }
    if header.summary.is_empty() {
        report(problems, 0, Code::HeaderMissing, "summary");
    }
    if header.keywords.is_empty() {
        report(problems, 0, Code::HeaderMissing, "keywords");
    }
    if header.site.publish && header.site.description.is_none() {
        report(problems, 0, Code::HeaderMissing, "description");
    }
    Ok(())
}

/// One `key: value` of the header, on line `n`: stored, or reported (an `id` that is no id refuses the topic).
fn set_field(header: &mut Header, key: &str, value: &str, n: usize, problems: &mut Vec<Problem>) -> Result<(), Problem> {
    match key {
        "id" => {
            if !valid_id(value) {
                return Err(Problem::new(n, Code::HeaderValue, "id"));
            }
            (header.id, header.lines.id) = (value.to_owned(), n);
        }
        "title" => {
            (header.title, header.lines.title) = (value.to_owned(), n);
            length(problems, n, "title", value, 1..=MAX_TITLE);
        }
        "summary" => {
            (header.summary, header.lines.summary) = (value.to_owned(), n);
            length(problems, n, "summary", value, 1..=MAX_SUMMARY);
        }
        "keywords" => {
            header.lines.keywords = n;
            header.keywords = keywords(problems, n, value);
        }
        "unlisted" => header.unlisted = yes_no(problems, n, key, value).unwrap_or(false),
        // A value that is neither is a problem on its own: the topic is read as not published, so that no second problem follows from it.
        "site" => header.site.publish = yes_no(problems, n, key, value).unwrap_or(false),
        "path" => header.site.path = path(problems, n, value),
        "description" => {
            header.lines.description = n;
            length(problems, n, "description", value, DESCRIPTION);
            header.site.description = Some(value.to_owned());
        }
        "seo_title" => {
            header.lines.seo_title = n;
            length(problems, n, "seo_title", value, 1..=MAX_SEO_TITLE);
            header.site.seo_title = Some(value.to_owned());
        }
        "picture" => {
            if valid_id(value) {
                header.site.picture = Some(value.to_owned());
            } else {
                report(problems, n, Code::HeaderValue, key);
            }
        }
        other => report(problems, n, Code::HeaderUnknownKey, other),
    }
    Ok(())
}

fn length(problems: &mut Vec<Problem>, line: usize, key: &str, value: &str, allowed: std::ops::RangeInclusive<usize>) {
    let chars = value.chars().count();
    if !allowed.contains(&chars) {
        report(problems, line, Code::HeaderLength, &format!("{key}: {chars} characters, {} to {} wanted", allowed.start(), allowed.end()));
    }
}

fn yes_no(problems: &mut Vec<Problem>, line: usize, key: &str, value: &str) -> Option<bool> {
    match value {
        "yes" => Some(true),
        "no" => Some(false),
        _ => {
            report(problems, line, Code::HeaderValue, key);
            None
        }
    }
}

/// The words of a `keywords` line: separated by commas, white space trimmed, none empty, at most the cap.
fn keywords(problems: &mut Vec<Problem>, line: usize, value: &str) -> Vec<String> {
    let mut words: Vec<String> = value.split(',').map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned).collect();
    if words.len() > MAX_KEYWORDS {
        report(problems, line, Code::TooManyKeywords, "");
        words.truncate(MAX_KEYWORDS);
    }
    words
}

/// A page's path: folders and names of lowercase words with dashes, joined by `/`, at most 64 bytes.
fn path(problems: &mut Vec<Problem>, line: usize, value: &str) -> Option<String> {
    if value.len() <= 64 && value.split('/').all(valid_id) {
        return Some(value.to_owned());
    }
    report(problems, line, Code::HeaderValue, "path");
    None
}
