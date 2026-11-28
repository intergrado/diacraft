//! Alt text and the accessibility check: `shape.alt_text`, `page.alt_text`, `master.alt_text` and `review.check_accessibility`. The rules are named in `draw/alt_text.rs`
//! and `draw/accessibility.rs`
//! and in `docs/editing-commands.md`, "Alt text and the accessibility check".
//!
//! Each of the three commands reads, sets and clears the title and the description of one sheet, and says what the sheet has after: the texts, where each stands (its own row, or its master's), the
//! formula a cell holds, and whether a `GUARD` protects it. The window's Alt Text section, the Page Setup dialog's Alt Text tab and the Check Accessibility pane send these.

use serde_json::{Value, json};

use super::page_setup::target_page;
use super::params::{optional_whole, page_for};
use super::{CommandSpec, has_document, optional_text, required_u32};
use crate::Session;
use crate::draw::accessibility::{self, Cut, Finding, Fix, Report, Rule, Severity};
use crate::draw::alt_text::{self, Located, Part, Target};
use crate::error::{Result, bad, failed};
use diacraft_doc::alt_text::Where;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.alt_text",
            "Alt Text",
            ["Home", "Shape Styles", "Alt Text"],
            None,
            "id: whole number, the shape (a group, a member of a group and an instance too), whose alt text is read, set or cleared; \
             title: text, optional, the shape's title, the text a screen reader says first: at most 32768 characters, kept character for character, and the empty text clears it; \
             description: text, optional, the longer description, as title; \
             clear: true or false, optional, sets both texts to the empty text, and is not given with a title or a description; \
             page: whole number, optional, the id of the page (default: the first page with the shape). \
             With no title, description or clear the call only reads, and it reads a shape on a locked layer too (a call that writes is refused there). A text is written to the shape's own row, never to its master's, and clearing writes the empty text and keeps the row, so an instance \
             shows none of its master's. A text that is the shape's own already writes nothing, and a cell a GUARD protects is refused. Both texts are one undo step. \
             Answers {kind, page, id, title, description, title_from, description_from, title_master, description_master, title_length, description_length, title_formula, \
             description_formula, title_protected, description_protected, has_alt_text, changed}: title_from and description_from are own, master or none, \
             the master is the id of the master a text comes from, the formula is the text of a formula that is no string constant (the text above is its result), \
             and a text longer than 32768 characters is answered cut, with its length",
            has_document,
            shape
        ),
        cmd!(
            "page.alt_text",
            "Page Alt Text",
            ["Design", "Page Setup"],
            None,
            "page: whole number, optional, the id of the page (default: the first page that is not a background); \
             title: text, optional, the page's title: at most 32768 characters, kept character for character, and the empty text clears it; \
             description: text, optional, the longer description, as title; \
             clear: true or false, optional, sets both texts to the empty text, and is not given with a title or a description. \
             With no title, description or clear the call only reads. Answers {kind, page, title, description, title_from, description_from, title_master, description_master, title_length, \
             description_length, title_formula, description_formula, title_protected, description_protected, has_alt_text, changed}, as shape.alt_text",
            has_document,
            page
        ),
        cmd!(
            "master.alt_text",
            "Master Alt Text",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the id of the master, whose shape carries the alt text its instances inherit; \
             shape: whole number, optional, the id of a shape of the master (default: the master's one top-level shape, and a master with none or several is refused naming them); \
             title: text, optional, the title: at most 32768 characters, kept character for character, and the empty text clears it; \
             description: text, optional, the longer description, as title; \
             clear: true or false, optional, sets both texts to the empty text, and is not given with a title or a description. \
             With no title, description or clear the call only reads. The master's page sheet is not read or written. Answers {kind, id, shape, title, description, title_from, description_from, \
             title_master, description_master, title_length, description_length, title_formula, description_formula, title_protected, description_protected, has_alt_text, changed}, as shape.alt_text",
            has_document,
            master
        ),
        cmd!(
            query "review.check_accessibility",
            "Check Accessibility",
            ["Review", "Accessibility"],
            None,
            "page: whole number, optional, the id of a page to check (default: every page that is not a background). \
             Looks for alt text missing on a shape or a page and alt text that is a file name or a web address (errors), and for a page with a default name and a hyperlink with no description \
             (warnings), on the top-level shapes of each page but its connectors, guides and shapes on hidden layers. Answers {findings, errors, warnings, pages_checked, shapes_checked, \
             shapes_skipped, cut, cut_reason, notes, rules}: each finding is {rule, severity, page, shape, row, field, name, message, fix}, in the order of the pages and their drawing order, with \
             the fix {action, command, params} that puts it right (add_alt_text, rename_page or edit_hyperlink), at most 10000 findings, and cut says the list is cut, as does the check's walk of \
             at most 400000 shapes. rules lists the five rules with their severity, title, advice and the count of findings",
            has_document,
            check
        ),
    ]
}

/// What a call asks of the texts: what to set, if anything.
struct Given<'p> {
    title: Option<&'p str>,
    description: Option<&'p str>,
}

impl Given<'_> {
    fn writes(&self) -> bool {
        self.title.is_some() || self.description.is_some()
    }
}

/// The texts a call gives (`title` and `description`), or the empty text for both when it says `clear`.
fn given<'p>(p: &'p Value, cmd: &str) -> Result<Given<'p>> {
    let (title, description) = (optional_text(p, "title", cmd)?, optional_text(p, "description", cmd)?);
    let clear = match p.get("clear") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err(bad(cmd, "`clear` must be true or false")),
    };
    if clear && (title.is_some() || description.is_some()) {
        return Err(bad(cmd, "`clear` empties both texts: give it alone, or give `title` and `description`"));
    }
    Ok(if clear { Given { title: Some(""), description: Some("") } } else { Given { title, description } })
}

fn from_word(from: Where) -> &'static str {
    match from {
        Where::Nowhere => "none",
        Where::Own => "own",
        Where::Master(_) => "master",
    }
}

fn master_of(from: Where) -> Value {
    match from {
        Where::Master(id) => json!(id),
        Where::Nowhere | Where::Own => Value::Null,
    }
}

/// One text's part of an answer, under the names `<which>_...`.
fn put(out: &mut serde_json::Map<String, Value>, which: &str, part: &Part) {
    out.insert(which.to_owned(), json!(part.text));
    out.insert(format!("{which}_from"), json!(from_word(part.from)));
    out.insert(format!("{which}_master"), master_of(part.from));
    out.insert(format!("{which}_length"), json!(part.length));
    out.insert(format!("{which}_formula"), json!(part.formula));
    out.insert(format!("{which}_protected"), json!(part.protected));
}

/// Reads, sets and clears the texts of the sheet `target` names, and answers what it has after.
fn on(s: &mut Session, target: &Target, given: &Given<'_>, cmd: &str) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let at: Located = alt_text::locate(doc, target).map_err(|e| e.into_engine(cmd))?;
    let changed = if given.writes() {
        s.edit_if("Set alt text", |doc| {
            let changed = alt_text::apply(doc, &at, given.title, given.description).map_err(|e| e.into_engine(cmd))?;
            Ok((changed, changed))
        })?
    } else {
        false
    };
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let read = alt_text::read(doc, &at);
    let mut out = serde_json::Map::new();
    match target {
        Target::Shape { page, id } => {
            out.insert("kind".to_owned(), json!("shape"));
            out.insert("page".to_owned(), json!(page));
            out.insert("id".to_owned(), json!(id));
        }
        Target::Page(page) => {
            out.insert("kind".to_owned(), json!("page"));
            out.insert("page".to_owned(), json!(page));
        }
        Target::Master { id, .. } => {
            out.insert("kind".to_owned(), json!("master"));
            out.insert("id".to_owned(), json!(id));
            out.insert("shape".to_owned(), json!(at.shape));
        }
    }
    put(&mut out, "title", &read.title);
    put(&mut out, "description", &read.description);
    out.insert("has_alt_text".to_owned(), json!(read.has_alt_text));
    out.insert("changed".to_owned(), json!(changed));
    Ok(Value::Object(out))
}

fn shape(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.alt_text";
    let id = required_u32(p, "id", ID)?;
    let named = optional_whole(p, "page", ID)?;
    let given = given(p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = page_for(doc, named, &[id], ID)?;
    on(s, &Target::Shape { page, id }, &given, ID)
}

fn page(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.alt_text";
    let given = given(p, ID)?;
    let page = target_page(s, p, ID)?;
    on(s, &Target::Page(page), &given, ID)
}

fn master(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "master.alt_text";
    let id = required_u32(p, "id", ID)?;
    let shape = optional_whole(p, "shape", ID)?;
    let given = given(p, ID)?;
    on(s, &Target::Master { id, shape }, &given, ID)
}

/// The fix a finding offers, as the answer gives it: the action and the command that does it, with the params that name the sheet.
fn fix_json(f: &Finding) -> Value {
    let (command, params) = match (f.fix, f.shape) {
        (Fix::AddAltText, Some(id)) => ("shape.alt_text", json!({ "id": id, "page": f.page })),
        (Fix::AddAltText, None) => ("page.alt_text", json!({ "page": f.page })),
        (Fix::RenamePage, _) => ("page.rename", json!({ "id": f.page })),
        (Fix::EditHyperlink, id) => ("hyperlink.set", json!({ "id": id, "page": f.page })),
    };
    json!({ "action": f.fix.word(), "command": command, "params": params })
}

fn finding_json(f: &Finding) -> Value {
    json!({
        "rule": f.rule.id(),
        "severity": f.rule.severity().word(),
        "page": f.page,
        "shape": f.shape,
        "row": f.row,
        "field": f.field.map(|field| field.word()),
        "name": f.name,
        "message": f.message,
        "fix": fix_json(f),
    })
}

fn report_json(r: &Report) -> Value {
    let rules: Vec<Value> = Rule::ALL
        .iter()
        .map(|rule| json!({ "id": rule.id(), "severity": rule.severity().word(), "title": rule.title(), "advice": rule.advice(), "count": r.count_rule(*rule) }))
        .collect();
    json!({
        "findings": r.findings.iter().map(finding_json).collect::<Vec<_>>(),
        "errors": r.count(Severity::Error),
        "warnings": r.count(Severity::Warning),
        "pages_checked": r.pages_checked,
        "shapes_checked": r.shapes_checked,
        "shapes_skipped": r.shapes_skipped,
        "cut": r.cut.is_some(),
        "cut_reason": r.cut.map(Cut::words),
        "notes": r.notes,
        "rules": rules,
    })
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "review.check_accessibility";
    let page = optional_whole(p, "page", ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let report = accessibility::check(doc, page).map_err(|e| e.into_engine(ID))?;
    Ok(report_json(&report))
}
