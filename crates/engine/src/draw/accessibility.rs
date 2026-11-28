//! The first accessibility check: five rules a drawing's model can answer, run over the pages and the top-level shapes of a drawing.
//!
//! **The rules** (the Office checker lists one that names the reference app, the alt-text rule; the other four are practices its pages advise, so what each says here is ours):
//!
//! | id | severity | what it finds |
//! |---|---|---|
//! | `alt_text_missing_on_shape` | Error | a checked shape that has no alt text |
//! | `alt_text_missing_on_page` | Error | a foreground page that has no alt text |
//! | `alt_text_is_file_name` | Error | alt text on a checked shape or a foreground page that is a file name or a web address |
//! | `page_has_default_name` | Warning | a foreground page that still has a name made by default |
//! | `hyperlink_has_no_description` | Warning | a hyperlink of a checked shape whose description is missing or only says its address |
//!
//! **What is looked at.** Every foreground page in page order (a background page is left out, with its shapes: they show on the pages that use it), and on each the page's own
//! findings and then its **top-level shapes** in the page's drawing order. A top-level shape is skipped when it is deleted from its master, a guide, a connector (a one-dimensional shape whose
//! effective `ObjType` has the routable flag and not the one that outranks it, **or one DiaCraft's own connector makes, which an earlier build made with no `ObjType`
//! (gives a connector made now the routable flag): a one-dimensional shape with an end glued or a route DiaCraft wrote**, as the engine's routing takes a connector), or on layers that are all hidden. **A group answers for itself and
//! its members are not looked at**, so a group with no alt text is one finding whatever its members hold, and a group with alt text is none; a shape with text and no alt text is still a finding. The
//! rows of a master's page sheet and the masters themselves are not checked.
//!
//! **What a rule reads.** Alt text is [`diacraft_doc::alt_text::read`]: own or inherited, so an instance whose master has the text has it. The file-name rule splits each
//! text at white space, strips the trailing marks `.,;:!?)]}"'` from each word, and flags a word that, compared without case, ends in a dot and one of [`EXTENSIONS`] with a character before the
//! dot, or begins with a web scheme or `www.`; **one finding per sheet**, naming the box (title, description or both). "Duplicate text" is not checked (no source says what it compares).
//! The default-name rule takes `Page-` and digits, with more `-` and digits after it, for the page's `Name`, and, for a default of another language, a page whose universal
//! name is `Page-` and digits while its `Name` is one word with no space or digit, a hyphen and digits. The hyperlink rule reads the shape's `Hyperlink` rows, its own and its master's,
//! and finds a row with an address or a sub-address whose description is empty, white space, or equal to the address, both trimmed and compared without case: one finding for each row.
//!
//! **What it costs.** One resolver for the whole check, so the masters are mapped once; each shape costs a few cell reads. The check stops at [`MAX_FINDINGS`] findings or when it has
//! walked [`MAX_SHAPES_WALKED`] top-level shapes (the most a frame of an export draws) and says so ([`Cut`]); a text is read through borrowed slices, and a hyperlink's text is compared up to
//! [`MAX_LINK_TEXT`] characters. A shape with more than [`MAX_LINK_ROWS`] hyperlink rows has none of them read.

use std::collections::HashSet;

use diacraft_doc::alt_text::{self, AltText};
use diacraft_doc::{Document, Owner, Page, RowKey, SectionKey, Shape, ShapeRef, ShapeType, SheetRef};

use super::obstacle::{HiddenLayers, flags_of, obj_kind};
use super::read::Reader;
use super::{Problem, require_page};
use crate::error::shown;

/// The most findings a check lists.
pub const MAX_FINDINGS: usize = 10_000;
/// The most top-level shapes a check walks, in all its pages: the most shapes a frame of an export draws (`diacraft_render::limits::MAX_SHAPES_PER_FRAME`).
pub const MAX_SHAPES_WALKED: usize = 400_000;
/// The most rows of a shape's `Hyperlink` section the check reads.
pub const MAX_LINK_ROWS: usize = 4_096;
/// The most characters of a hyperlink's text the check compares.
pub const MAX_LINK_TEXT: usize = 4_096;

/// The extensions of the file-name rule: the picture, vector, document and drawing formats a name may end in.
pub const EXTENSIONS: [&str; 28] = [
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "dib", "tif", "tiff", "svg", "svgz", "emf", "wmf", "webp", "ico", "heic", "pdf", "vsd",
    "vsdx", "vsdm", "vss", "vssx", "vst", "vstx", "odg", "fodg", "diacraft",
];
/// The schemes (and `www.`) that begin a web address for the rule.
const ADDRESS_STARTS: [&str; 5] = ["http://", "https://", "ftp://", "file:", "www."];

/// How serious a finding is: the groups of the pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// The word the answer and the pane use.
    pub const fn word(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// The five rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    MissingShapeAltText,
    MissingPageAltText,
    AltTextIsFileName,
    DefaultPageName,
    HyperlinkNoDescription,
}

impl Rule {
    /// Every rule, in the order the pane lists them.
    pub const ALL: [Rule; 5] =
        [Rule::MissingShapeAltText, Rule::MissingPageAltText, Rule::AltTextIsFileName, Rule::DefaultPageName, Rule::HyperlinkNoDescription];

    /// The stable id a program reads.
    pub const fn id(self) -> &'static str {
        match self {
            Rule::MissingShapeAltText => "alt_text_missing_on_shape",
            Rule::MissingPageAltText => "alt_text_missing_on_page",
            Rule::AltTextIsFileName => "alt_text_is_file_name",
            Rule::DefaultPageName => "page_has_default_name",
            Rule::HyperlinkNoDescription => "hyperlink_has_no_description",
        }
    }

    pub const fn severity(self) -> Severity {
        match self {
            Rule::MissingShapeAltText | Rule::MissingPageAltText | Rule::AltTextIsFileName => Severity::Error,
            Rule::DefaultPageName | Rule::HyperlinkNoDescription => Severity::Warning,
        }
    }

    /// What the rule finds, as the pane's group lists it.
    pub const fn title(self) -> &'static str {
        match self {
            Rule::MissingShapeAltText => "Missing alt text on a shape",
            Rule::MissingPageAltText => "Missing alt text on a page",
            Rule::AltTextIsFileName => "Alt text that is a file name or a web address",
            Rule::DefaultPageName => "Page with a default name",
            Rule::HyperlinkNoDescription => "Hyperlink with no description",
        }
    }

    /// Why it matters and how to put it right, in DiaCraft's own words (the pane's Additional Information).
    pub const fn advice(self) -> &'static str {
        match self {
            Rule::MissingShapeAltText => {
                "A screen reader says a shape's alt text in place of the shape. Give it a title, a description, or both: say what the shape shows or does."
            }
            Rule::MissingPageAltText => {
                "A screen reader says a page's alt text when it reaches the page. Give the page a title that says what it shows, in Page Setup."
            }
            Rule::AltTextIsFileName => {
                "A file name or a web address says nothing about what is shown. Replace it with a sentence that describes the picture."
            }
            Rule::DefaultPageName => "A name such as Page-1 does not say what the page holds. Rename the page.",
            Rule::HyperlinkNoDescription => {
                "A link that only shows its address, or nothing, tells a reader nothing about where it leads. Give it a description."
            }
        }
    }
}

/// The one action a finding offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Open the alt text of the shape or the page.
    AddAltText,
    /// Rename the page.
    RenamePage,
    /// Edit the shape's hyperlink.
    EditHyperlink,
}

impl Fix {
    /// The word the answer gives it.
    pub const fn word(self) -> &'static str {
        match self {
            Fix::AddAltText => "add_alt_text",
            Fix::RenamePage => "rename_page",
            Fix::EditHyperlink => "edit_hyperlink",
        }
    }
}

/// Which text of a sheet a finding is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Description,
    Both,
}

impl Field {
    pub const fn word(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Description => "description",
            Field::Both => "both",
        }
    }
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: Rule,
    pub page: u32,
    /// The shape, for a finding about a shape or one of its hyperlinks.
    pub shape: Option<u32>,
    /// The name of the hyperlink row, for the hyperlink rule.
    pub row: Option<String>,
    /// The text the rule found, for the file-name rule.
    pub field: Option<Field>,
    /// The name the pane lists the item by: the shape's name, or the page's.
    pub name: String,
    pub message: String,
    pub fix: Fix,
}

/// Why a list is not the whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// [`MAX_FINDINGS`] were found.
    Findings,
    /// [`MAX_SHAPES_WALKED`] shapes were walked.
    Shapes,
}

impl Cut {
    pub fn words(self) -> String {
        match self {
            Cut::Findings => format!("the list stops at {MAX_FINDINGS} findings; put these right and check again"),
            Cut::Shapes => format!("the check looked at the first {MAX_SHAPES_WALKED} shapes only"),
        }
    }
}

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub pages_checked: usize,
    /// Top-level shapes looked at by the rules.
    pub shapes_checked: usize,
    /// Top-level shapes the scope leaves out (connectors, guides, shapes on hidden layers, shapes deleted from a master).
    pub shapes_skipped: usize,
    pub cut: Option<Cut>,
    /// What the check left out and why, for a page it was asked for and does not check.
    pub notes: Vec<String>,
}

impl Report {
    /// How many findings of a severity the list holds.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings.iter().filter(|f| f.rule.severity() == severity).count()
    }

    /// How many findings of a rule the list holds.
    pub fn count_rule(&self, rule: Rule) -> usize {
        self.findings.iter().filter(|f| f.rule == rule).count()
    }
}

/// Whether a word, with its trailing marks taken off, is a file name or a web address.
fn word_is_file_or_address(word: &str) -> bool {
    let word = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'']);
    let lower = word.to_lowercase();
    if ADDRESS_STARTS.iter().any(|start| lower.starts_with(start)) {
        return true;
    }
    match lower.rsplit_once('.') {
        Some((stem, extension)) => !stem.is_empty() && EXTENSIONS.contains(&extension),
        None => false,
    }
}

/// Whether a text holds a word that is a file name or a web address.
pub fn is_file_or_address(text: &str) -> bool {
    text.split_whitespace().any(word_is_file_or_address)
}

/// A page name that is `Page-` and digits, with more hyphens and digits after.
fn is_page_digits(name: &str, more: bool) -> bool {
    let Some(rest) = name.strip_prefix("Page-") else { return false };
    let mut groups = rest.split('-');
    let digits = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit());
    match groups.next() {
        Some(first) if digits(first) => {}
        _ => return false,
    }
    let others: Vec<&str> = groups.collect();
    (more || others.is_empty()) && others.iter().all(|g| digits(g))
}

/// One word with no space and no digit, a hyphen and digits: the name a default takes in another language.
fn is_word_digits(name: &str) -> bool {
    let Some((word, digits)) = name.rsplit_once('-') else { return false };
    !word.is_empty()
        && !word.contains('-')
        && !word.chars().any(|c| c.is_whitespace() || c.is_ascii_digit())
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Whether a page's names say it still has the name it was given by default.
pub fn has_default_name(name: &str, name_u: Option<&str>) -> bool {
    is_page_digits(name, true) || (name_u.is_some_and(|u| is_page_digits(u, false)) && is_word_digits(name))
}

/// `text` cut at [`MAX_LINK_TEXT`] characters, trimmed.
fn link_text(text: &str) -> &str {
    let end = text.char_indices().nth(MAX_LINK_TEXT).map_or(text.len(), |(at, _)| at);
    text.get(..end).unwrap_or_default().trim()
}

/// How a hyperlink row fails the rule, if it does: its description is missing, or it only says the address.
fn link_fault(address: &str, sub_address: &str, description: &str) -> bool {
    let (address, sub_address, description) = (link_text(address), link_text(sub_address), link_text(description));
    if address.is_empty() && sub_address.is_empty() {
        return false;
    }
    description.is_empty() || (!address.is_empty() && description.to_lowercase() == address.to_lowercase())
}

/// What a shape is called in the pane and in words: its name, else its universal name, else `Sheet.<id>`, cut.
fn shape_name(shape: &Shape) -> String {
    let named = shape.name.as_deref().filter(|n| !n.is_empty()).or_else(|| shape.name_u.as_deref().filter(|n| !n.is_empty()));
    named.map_or_else(|| format!("Sheet.{}", shape.id), |name| name.chars().take(256).collect())
}

/// Which of the two texts is a file name or an address.
fn file_field(alt: &AltText<'_>) -> Option<Field> {
    match (is_file_or_address(alt.title.text), is_file_or_address(alt.description.text)) {
        (true, true) => Some(Field::Both),
        (true, false) => Some(Field::Title),
        (false, true) => Some(Field::Description),
        (false, false) => None,
    }
}

/// The cells that are the ends of a connector: a `Connect` row from one of them glues an end.
const END_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// The shapes of the page that have an end glued to another shape.
fn glued_ends(page: &Page) -> HashSet<u32> {
    page.connects.iter().filter(|c| c.from_cell.as_deref().is_some_and(|cell| END_CELLS.contains(&cell))).map(|c| c.from_sheet).collect()
}

/// The state of one check.
struct Check<'d> {
    reader: Reader<'d>,
    report: Report,
}

impl Check<'_> {
    /// Adds a finding, or stops the check at the cap. → whether the check goes on.
    fn push(&mut self, finding: Finding) -> bool {
        if self.report.findings.len() >= MAX_FINDINGS {
            self.report.cut = Some(Cut::Findings);
            return false;
        }
        self.report.findings.push(finding);
        true
    }

    fn page_findings(&mut self, page: &Page) -> bool {
        let sheet = SheetRef::Page(page.id);
        let alt = alt_text::read(self.reader.resolver(), &sheet);
        let name = page.name.chars().take(256).collect::<String>();
        let found = |rule: Rule, field: Option<Field>, message: String, fix: Fix| Finding {
            rule,
            page: page.id,
            shape: None,
            row: None,
            field,
            name: name.clone(),
            message,
            fix,
        };
        if !alt.has_text() {
            let message = format!("The page \"{}\" has no alt text.", shown(&page.name));
            if !self.push(found(Rule::MissingPageAltText, None, message, Fix::AddAltText)) {
                return false;
            }
        } else if let Some(field) = file_field(&alt) {
            let message =
                format!("The alt text of the page \"{}\" is a file name or a web address; say what the page shows instead.", shown(&page.name));
            if !self.push(found(Rule::AltTextIsFileName, Some(field), message, Fix::AddAltText)) {
                return false;
            }
        }
        if has_default_name(&page.name, page.name_u.as_deref()) {
            let message = format!("The page is named \"{}\", a name made by default; give it a name that says what it shows.", shown(&page.name));
            if !self.push(found(Rule::DefaultPageName, None, message, Fix::RenamePage)) {
                return false;
            }
        }
        true
    }

    /// Whether the scope leaves a top-level shape out.
    fn skipped(&self, shape: &Shape, sheet: &SheetRef, layers: &HiddenLayers, glued: &HashSet<u32>) -> bool {
        if shape.del || shape.kind == ShapeType::Guide || layers.hides(shape) {
            return true;
        }
        self.reader.is_one_d(sheet)
            && (obj_kind(flags_of(self.reader.number(sheet, "ObjType"))).routable || glued.contains(&shape.id) || self.reader.is_routed(sheet))
    }

    fn shape_findings(&mut self, page: u32, shape: &Shape, at: ShapeRef) -> bool {
        let sheet = SheetRef::Shape(at);
        let alt = alt_text::read(self.reader.resolver(), &sheet);
        let name = shape_name(shape);
        let found = |rule: Rule, field: Option<Field>, row: Option<String>, message: String, fix: Fix| Finding {
            rule,
            page,
            shape: Some(shape.id),
            row,
            field,
            name: name.clone(),
            message,
            fix,
        };
        if !alt.has_text() {
            let message = format!("The shape \"{}\" has no alt text.", shown(&name));
            if !self.push(found(Rule::MissingShapeAltText, None, None, message, Fix::AddAltText)) {
                return false;
            }
        } else if let Some(field) = file_field(&alt) {
            let message =
                format!("The alt text of the shape \"{}\" is a file name or a web address; say what the shape shows instead.", shown(&name));
            if !self.push(found(Rule::AltTextIsFileName, Some(field), None, message, Fix::AddAltText)) {
                return false;
            }
        }
        let key = SectionKey::new("Hyperlink", None);
        let Some(Ok(rows)) = self.reader.resolver().effective_rows_within(&sheet, &key, MAX_LINK_ROWS) else { return true };
        for effective in &rows {
            let RowKey::Name(row_name) = &effective.key else { continue };
            // The cells of a link that hold a formula and no saved value are what the formula works out to, as the picture reads them.
            let row = self.reader.worked().row(&sheet, &key, effective);
            let text = |cell: &str| row.text(cell).unwrap_or_default();
            if link_fault(&text("Address"), &text("SubAddress"), &text("Description")) {
                let message = format!("A hyperlink of the shape \"{}\" has no description of its own; say where it leads.", shown(&name));
                if !self.push(found(Rule::HyperlinkNoDescription, None, Some(row_name.to_string()), message, Fix::EditHyperlink)) {
                    return false;
                }
            }
        }
        true
    }

    /// Checks one foreground page. → whether the check goes on.
    fn page(&mut self, page: &Page, walked: &mut usize) -> bool {
        self.report.pages_checked += 1;
        if !self.page_findings(page) {
            return false;
        }
        let layers = HiddenLayers::of(self.reader.doc(), Owner::Page(page.id));
        let glued = glued_ends(page);
        for (i, shape) in page.shapes.iter().enumerate() {
            if *walked >= MAX_SHAPES_WALKED {
                self.report.cut = Some(Cut::Shapes);
                return false;
            }
            *walked += 1;
            let Ok(index) = u32::try_from(i) else { return true };
            let at = ShapeRef::top(Owner::Page(page.id), index);
            let sheet = SheetRef::Shape(at.clone());
            if self.skipped(shape, &sheet, &layers, &glued) {
                self.report.shapes_skipped += 1;
                continue;
            }
            self.report.shapes_checked += 1;
            if !self.shape_findings(page.id, shape, at) {
                return false;
            }
        }
        true
    }
}

/// Checks the drawing's foreground pages, or the one page `page` names, by the five rules. A page that is not there is refused; a background page is left out, and the report says so.
pub fn check(doc: &Document, page: Option<u32>) -> Result<Report, Problem> {
    if let Some(id) = page {
        require_page(doc, id)?;
    }
    let mut check = Check { reader: Reader::new(doc), report: Report::default() };
    let mut walked = 0usize;
    for p in doc.pages.iter() {
        if page.is_some_and(|id| id != p.id) {
            continue;
        }
        if p.background {
            check.report.notes.push(format!(
                "the page \"{}\" is a background page, which the check leaves out: its shapes show on the pages that use it",
                shown(&p.name)
            ));
            continue;
        }
        if !check.page(p, &mut walked) {
            break;
        }
    }
    Ok(check.report)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_file_name_or_an_address_is_found_by_its_words() {
        for text in [
            "diagram1.png",
            "logo.SVG",
            "see www.example.org",
            "https://example.org/x)",
            "photo.jpeg.",
            "map.png too",
            "a.b.vsdx,",
            "FILE:/x",
            "x.diacraft",
        ] {
            assert!(is_file_or_address(text), "{text}");
        }
        for text in ["", "Release 2.0", "e.g. a pump", "Pump.", ".png", "Version 3.vsdx-tool", "png", "a png", "http", "pdf.", "wwwx.example"] {
            assert!(!is_file_or_address(text), "{text}");
        }
    }

    #[test]
    fn a_default_page_name_is_page_and_digits() {
        for (name, name_u) in
            [("Page-3", None), ("Page-1-2", None), ("Seite-4", Some("Page-4")), ("Page-12", Some("Overview")), ("Pagina-7", Some("Page-7"))]
        {
            assert!(has_default_name(name, name_u), "{name} {name_u:?}");
        }
        for (name, name_u) in [
            ("Overview", Some("Page-1")),
            ("Page 3", None),
            ("Page-3a", None),
            ("Page-", None),
            ("Page--1", None),
            ("Plan-A-4", Some("Page-4")),
            ("Seite-4", Some("Page-4-1")),
            ("Seite-4", None),
            ("Seite 5-4", Some("Page-4")),
            ("page-3", None),
        ] {
            assert!(!has_default_name(name, name_u), "{name} {name_u:?}");
        }
    }

    #[test]
    fn a_link_with_no_description_or_only_its_address_is_a_fault() {
        assert!(link_fault("https://example.org", "", ""));
        assert!(link_fault("https://example.org", "", " HTTPS://EXAMPLE.ORG "));
        assert!(link_fault("", "Page-2", "  "));
        assert!(!link_fault("https://example.org", "", "Product page"));
        assert!(!link_fault("", "", ""));
        assert!(!link_fault(" ", "\t", "x"));
        assert!(!link_fault("", "Page-2", "Page-2"), "the rule compares the description with the address only");
    }
}
