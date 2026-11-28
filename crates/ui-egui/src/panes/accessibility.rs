//! The Accessibility pane: what Review, Accessibility, **Check Accessibility** opens. It lists what
//! `review.check_accessibility` found under **Errors** and **Warnings**, each group by its rules with a count and each rule by its items (the shape's name or the page's); a click on an item goes
//! to its page and selects the shape (`window.showFinding`); the selected item shows why it matters and how to put it right ("Additional Information") and offers its fix: **Add Alt Text** opens
//! the Format Shape pane at the Alt Text section (Page Setup's Alt Text tab for a page), **Rename Page** Page Setup's Page Properties tab, **Edit Hyperlink** the Hyperlinks dialog.
//!
//! **When it checks**: when it is first drawn, **again after each edit of the drawing** (once for each revision, never for each frame), and when **Recheck** is pressed. A check
//! that was cut (past 10,000 findings or the walk of 400,000 shapes) is not made again by itself: the drawing is too big to check after every edit, so the pane says so and Recheck does it.
//! The pane lists at most [`MAX_ITEMS`] items of a rule (the command lists them all). Nothing here changes the drawing: the fixes open the places that do.
//!
//! The Tips group of the Office checker is not drawn: DiaCraft's five rules are errors and warnings.

use std::sync::Arc;

use egui::{Align2, CornerRadius, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::comments_ui::text_button;
use crate::panes::Pane;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};

/// The most items of one rule the pane draws: the check holds up to 10,000 findings, and a frame draws a list.
const MAX_ITEMS: usize = 200;

/// One finding of the check as the pane keeps it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FindingView {
    pub rule: String,
    pub severity: String,
    pub page: u32,
    pub page_name: String,
    pub shape: Option<u32>,
    pub row: Value,
    pub name: String,
    pub message: String,
    pub fix: String,
}

impl FindingView {
    /// What the item says: the shape's name and its page, or the page's name.
    fn label(&self) -> String {
        match self.shape {
            Some(_) => format!("{} ({})", diacraft_engine::shown(&self.name, 40), diacraft_engine::shown(&self.page_name, 24)),
            None => diacraft_engine::shown(&self.name, 40),
        }
    }

    /// Whether `key` names this finding.
    fn is(&self, key: &Key) -> bool {
        self.rule == key.rule && self.page == key.page && self.shape == key.shape && self.row == key.row
    }
}

/// One rule of the check: its words, and how many findings it has.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleView {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub advice: String,
    pub count: usize,
}

/// What names a finding from one check to the next: the rule, the page, the shape and the row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    rule: String,
    page: u32,
    shape: Option<u32>,
    row: Value,
}

/// One check, for the revision of the drawing it was made for.
#[derive(Clone, Debug, Default)]
pub(crate) struct Checked {
    generation: u64,
    revision: u64,
    pub findings: Vec<FindingView>,
    pub errors: usize,
    pub warnings: usize,
    pub pages: usize,
    pub shapes: usize,
    /// Why the list is cut, when it is.
    pub cut: Option<String>,
    pub notes: Vec<String>,
    pub rules: Vec<RuleView>,
    /// The engine's words when it would not check.
    pub error: Option<String>,
}

/// What the pane keeps between frames.
#[derive(Default)]
pub struct AccessState {
    checked: Option<Arc<Checked>>,
    /// The finding selected.
    selected: Option<Key>,
    /// How many times the check has run: the window state and the tests count them.
    checks: u64,
    /// The pane was drawn in this frame (for the window state).
    shown: bool,
    /// Recheck was pressed: the next frame checks whatever the drawing is.
    force: bool,
    /// A finding was just selected: the pane scrolls to its details and its fix, so that they are in view whatever the list's length.
    reveal: bool,
}

impl AccessState {
    /// A new frame: the pane has not been drawn in it yet.
    pub(crate) fn next_frame(&mut self) {
        self.shown = false;
    }
}

fn whole(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// What `review.check_accessibility` answered, as the pane keeps it. `page_name` says what a page is called.
fn checked_of(answer: &Value, generation: u64, revision: u64, page_name: impl Fn(u32) -> String) -> Checked {
    let count = |key: &str| answer.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    let findings = answer
        .get("findings")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| {
                    let page = f.get("page").and_then(whole)?;
                    Some(FindingView {
                        rule: text(f, "rule"),
                        severity: text(f, "severity"),
                        page,
                        page_name: page_name(page),
                        shape: f.get("shape").and_then(whole),
                        row: f.get("row").cloned().unwrap_or(Value::Null),
                        name: text(f, "name"),
                        message: text(f, "message"),
                        fix: f.get("fix").map(|fix| text(fix, "action")).unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let rules = answer
        .get("rules")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|r| RuleView {
                    id: text(r, "id"),
                    severity: text(r, "severity"),
                    title: text(r, "title"),
                    advice: text(r, "advice"),
                    count: r.get("count").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    Checked {
        generation,
        revision,
        findings,
        errors: count("errors"),
        warnings: count("warnings"),
        pages: count("pages_checked"),
        shapes: count("shapes_checked"),
        cut: answer
            .get("cut")
            .and_then(Value::as_bool)
            .filter(|cut| *cut)
            .map(|_| answer.get("cut_reason").and_then(Value::as_str).unwrap_or("the check stopped early").to_owned()),
        notes: answer
            .get("notes")
            .and_then(Value::as_array)
            .map(|n| n.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        rules,
        error: None,
    }
}

/// "1 error", "5 errors".
fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// The button's words for the fix the engine names.
fn fix_words(action: &str) -> &'static str {
    match action {
        "add_alt_text" => "Add Alt Text",
        "rename_page" => "Rename Page",
        "edit_hyperlink" => "Edit Hyperlink",
        _ => "Fix",
    }
}

impl DiacraftApp {
    /// The check as it stands for the drawing now, **made when there is none for this revision** and `again` allows it (a check that was cut is made again only on request).
    fn access_check(&mut self, again: bool) -> Option<Arc<Checked>> {
        self.session.document()?;
        let (generation, revision) = (self.ui.generation, self.session.revision());
        let current = self.ui.panes.access.checked.clone();
        let is_current = |c: &Checked| (c.generation, c.revision) == (generation, revision);
        let cut = current.as_ref().is_some_and(|c| c.cut.is_some() && c.generation == generation);
        let wanted = self.ui.panes.access.force || current.as_ref().is_none_or(|c| !is_current(c) && (again || !cut));
        if !wanted {
            return current;
        }
        self.ui.panes.access.force = false;
        self.ui.panes.access.checks += 1;
        let answer = self.session.execute("review.check_accessibility", &json!({}));
        let names = |page: u32| self.session.document().and_then(|d| d.page(page)).map(|p| p.name.to_string()).unwrap_or_default();
        let checked = match answer {
            Ok(answer) => checked_of(&answer, generation, revision, names),
            Err(e) => Checked { generation, revision, error: Some(crate::panes::alt_text::alt_words(&e)), ..Checked::default() },
        };
        let checked = Arc::new(checked);
        // The finding selected stays selected while the new check still has it.
        if let Some(key) = &self.ui.panes.access.selected
            && !checked.findings.iter().any(|f| f.is(key))
        {
            self.ui.panes.access.selected = None;
        }
        self.ui.panes.access.checked = Some(Arc::clone(&checked));
        Some(checked)
    }

    /// Forgets the check: another drawing replaced the open one.
    pub(super) fn forget_access_pane(&mut self) {
        self.ui.panes.access = AccessState::default();
    }

    /// `window.showFinding`: goes to the finding's page and selects its shape (a page's finding shows the page), marks the finding in the pane and opens the pane. A finding the last check
    /// does not have is refused in words.
    pub(crate) fn show_finding_command(&mut self, rule: &str, page: u32, shape: Option<u32>, row: Option<Value>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.showFinding";
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: ID.to_owned(), msg };
        let checked = self.access_check(true).ok_or_else(|| bad("no document open".to_owned()))?;
        let found =
            checked.findings.iter().find(|f| f.rule == rule && f.page == page && f.shape == shape && row.as_ref().is_none_or(|r| *r == f.row));
        let Some(found) = found else {
            return Err(bad(format!(
                "no finding of the rule `{}` on page {page}{}",
                diacraft_engine::shown(rule, 48),
                shape.map_or(String::new(), |s| format!(" for shape {s}"))
            )));
        };
        let key = Key { rule: found.rule.clone(), page: found.page, shape: found.shape, row: found.row.clone() };
        if self.current_page() != Some(page) {
            self.execute("page.select", &json!({"id": page}))?;
        }
        if let Some(shape) = shape {
            self.select(&[shape]);
        }
        self.ui.panes.access.selected = Some(key);
        self.ui.panes.access.reveal = true;
        self.ui.panes.open(Pane::Accessibility);
        Ok(())
    }

    /// The pane's reading for the window state: `null` unless the pane was drawn in the last frame.
    pub(crate) fn accessibility_state(&self) -> Value {
        let state = &self.ui.panes.access;
        let Some(checked) = state.checked.as_ref().filter(|_| state.shown) else { return Value::Null };
        let selected = state.selected.as_ref().and_then(|key| checked.findings.iter().position(|f| f.is(key)));
        let findings: Vec<Value> = checked
            .findings
            .iter()
            .take(MAX_ITEMS * 2)
            .map(|f| json!({"rule": f.rule, "severity": f.severity, "page": f.page, "shape": f.shape, "name": f.name, "message": f.message, "fix": f.fix}))
            .collect();
        json!({
            "errors": checked.errors,
            "warnings": checked.warnings,
            "findings": findings,
            "cut": checked.cut,
            "checks": state.checks,
            "selected": selected,
            "error": checked.error,
        })
    }

    /// The pane's body: Recheck and what the check found, the findings by severity and rule, and the one selected with its fix.
    pub(super) fn accessibility_body(&mut self, ui: &mut Ui, t: &Tokens) {
        self.ui.panes.access.shown = true;
        let words = |ui: &mut Ui, text: &str, colour: egui::Color32| {
            ui.add(egui::Label::new(RichText::new(text).font(theme::font(size::LABEL)).color(colour)).wrap());
        };
        let Some(checked) = self.access_check(false) else {
            words(ui, "No drawing open.", t.text_dim);
            return;
        };
        let stale = (checked.generation, checked.revision) != (self.ui.generation, self.session.revision());
        ui.horizontal(|ui| {
            let recheck = text_button(ui, t, "Recheck", true, "accessibility.recheck");
            self.note("accessibility.recheck", recheck.rect, true);
            if recheck.clicked() {
                self.ui.panes.access.force = true;
            }
        });
        ui.add_space(4.0);
        if let Some(error) = &checked.error {
            words(ui, error, t.notice_text);
            return;
        }
        let summary = match (checked.errors, checked.warnings) {
            (0, 0) => "No accessibility problems found.".to_owned(),
            (e, 0) => counted(e, "error", "errors"),
            (0, w) => counted(w, "warning", "warnings"),
            (e, w) => format!("{}, {}", counted(e, "error", "errors"), counted(w, "warning", "warnings")),
        };
        words(ui, &summary, t.text);
        words(ui, &format!("Checked {} and {}.", counted(checked.pages, "page", "pages"), counted(checked.shapes, "shape", "shapes")), t.text_dim);
        if let Some(cut) = &checked.cut {
            words(ui, &format!("The list is cut: {cut}."), t.notice_text);
        }
        if stale {
            words(ui, "The drawing changed since this check. Recheck checks it again.", t.notice_text);
        }
        for note in &checked.notes {
            words(ui, note, t.text_dim);
        }
        ui.add_space(6.0);
        for (severity, heading, name) in [("error", "Errors", "accessibility.group.errors"), ("warning", "Warnings", "accessibility.group.warnings")]
        {
            let in_group = checked.findings.iter().filter(|f| f.severity == severity).count();
            if in_group == 0 {
                continue;
            }
            let group = egui::CollapsingHeader::new(RichText::new(format!("{heading} ({in_group})")).font(theme::font(size::TAB)).color(t.text))
                .id_salt(name)
                .default_open(true)
                .show(ui, |ui| {
                    for rule in checked.rules.iter().filter(|r| r.severity == severity && r.count > 0) {
                        self.rule_items(ui, t, &checked, rule);
                    }
                });
            self.note(name, group.header_response.rect, true);
        }
        self.finding_details(ui, t, &checked);
    }

    /// One rule: its title and count, and its items.
    fn rule_items(&mut self, ui: &mut Ui, t: &Tokens, checked: &Checked, rule: &RuleView) {
        let heading =
            ui.add(egui::Label::new(RichText::new(format!("{} ({})", rule.title, rule.count)).font(theme::font(size::LABEL)).color(t.text)).wrap());
        self.note(format!("accessibility.rule.{}", rule.id), heading.rect, true);
        let mut chosen: Option<usize> = None;
        let items = checked.findings.iter().enumerate().filter(|(_, f)| f.rule == rule.id);
        for (index, finding) in items.clone().take(MAX_ITEMS) {
            let selected = self.ui.panes.access.selected.as_ref().is_some_and(|key| finding.is(key));
            let label = finding.label();
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            widgets::name_toggle(&resp, true, selected, &label);
            if selected {
                ui.painter().rect_filled(rect, CornerRadius::same(3), t.accent_soft);
                ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, t.accent), StrokeKind::Inside);
            } else {
                paint_button_bg(ui, rect, &resp, true, false, t, 3);
            }
            let clip = ui.painter().with_clip_rect(rect.shrink2(vec2(6.0, 0.0)));
            clip.text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, &label, theme::font(size::LABEL), label_colour(t, true));
            self.note(format!("accessibility.item.{index}"), rect, true);
            if resp.clicked() {
                chosen = Some(index);
            }
        }
        let more = items.count().saturating_sub(MAX_ITEMS);
        if more > 0 {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("and {more} more: the command lists them all.")).font(theme::font(size::LABEL)).color(t.text_dim),
                )
                .wrap(),
            );
        }
        ui.add_space(4.0);
        if let Some(finding) = chosen.and_then(|index| checked.findings.get(index)) {
            let params = json!({"rule": finding.rule, "page": finding.page, "shape": finding.shape, "row": finding.row});
            if let Err(e) = self.execute("window.showFinding", &params) {
                self.set_notice(&crate::panes::alt_text::alt_words(&e));
            }
        }
    }

    /// The finding selected: why it matters, how to put it right, and the button that opens the place that does.
    fn finding_details(&mut self, ui: &mut Ui, t: &Tokens, checked: &Checked) {
        let Some(finding) = self.ui.panes.access.selected.as_ref().and_then(|key| checked.findings.iter().find(|f| f.is(key))) else { return };
        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Additional Information").font(theme::font(size::TAB)).color(t.text));
        ui.add(egui::Label::new(RichText::new(&finding.message).font(theme::font(size::LABEL)).color(t.text)).wrap());
        if let Some(rule) = checked.rules.iter().find(|r| r.id == finding.rule) {
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(&rule.advice).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
        }
        ui.add_space(6.0);
        let words = fix_words(&finding.fix);
        let fix = text_button(ui, t, words, true, "accessibility.fix");
        self.note("accessibility.fix", fix.rect, true);
        if std::mem::take(&mut self.ui.panes.access.reveal) {
            fix.scroll_to_me(None);
        }
        if fix.clicked() {
            let finding = finding.clone();
            self.run_fix(&finding);
        }
    }

    /// Opens the place that puts a finding right.
    fn run_fix(&mut self, finding: &FindingView) {
        let result = match (finding.fix.as_str(), finding.shape) {
            ("add_alt_text", Some(shape)) => self.execute("window.altText", &json!({"of": "shape", "id": shape, "page": finding.page})),
            ("add_alt_text", None) => self.execute("window.altText", &json!({"of": "page", "page": finding.page})),
            ("rename_page", _) => self.execute("window.pageSetup", &json!({"tab": "Page Properties", "page": finding.page})),
            ("edit_hyperlink", Some(shape)) => self
                .execute("window.showFinding", &json!({"rule": finding.rule, "page": finding.page, "shape": shape, "row": finding.row}))
                .and_then(|_| self.execute("window.hyperlinks", &json!({}))),
            _ => return,
        };
        if let Err(e) = result {
            self.set_notice(&crate::panes::alt_text::alt_words(&e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counts_are_said_in_the_singular_where_there_is_one() {
        assert_eq!(counted(1, "error", "errors"), "1 error");
        assert_eq!(counted(0, "warning", "warnings"), "0 warnings");
        assert_eq!(counted(5, "error", "errors"), "5 errors");
    }

    #[test]
    fn a_fix_has_the_words_of_its_button() {
        assert_eq!(fix_words("add_alt_text"), "Add Alt Text");
        assert_eq!(fix_words("rename_page"), "Rename Page");
        assert_eq!(fix_words("edit_hyperlink"), "Edit Hyperlink");
        assert_eq!(fix_words("something else"), "Fix");
    }

    #[test]
    fn an_answer_is_read_as_the_pane_keeps_it_and_a_malformed_one_is_read_as_empty() {
        let answer = json!({
            "findings": [
                {"rule": "alt_text_missing_on_shape", "severity": "error", "page": 3, "shape": 7, "row": null, "name": "Box", "message": "m", "fix": {"action": "add_alt_text"}},
                {"rule": "page_has_default_name", "severity": "warning", "page": 3, "shape": null, "row": null, "name": "Page-1", "message": "n", "fix": {"action": "rename_page"}},
                {"rule": "broken", "page": "not a number"},
            ],
            "errors": 1, "warnings": 1, "pages_checked": 1, "shapes_checked": 4, "cut": true, "cut_reason": "10000 findings",
            "rules": [{"id": "alt_text_missing_on_shape", "severity": "error", "title": "Missing alt text", "advice": "Add some", "count": 1}],
        });
        let checked = checked_of(&answer, 1, 2, |p| format!("page {p}"));
        assert_eq!((checked.errors, checked.warnings, checked.pages, checked.shapes), (1, 1, 1, 4));
        assert_eq!(checked.findings.len(), 2, "the finding with no page number is left out");
        assert_eq!(
            (checked.findings[0].shape, checked.findings[0].page_name.as_str(), checked.findings[0].fix.as_str()),
            (Some(7), "page 3", "add_alt_text")
        );
        assert_eq!(checked.findings[0].label(), "Box (page 3)");
        assert_eq!(checked.findings[1].label(), "Page-1");
        assert_eq!(checked.cut.as_deref(), Some("10000 findings"));
        assert_eq!(checked.rules[0].count, 1);
        let empty = checked_of(&json!({}), 0, 0, |_| String::new());
        assert!(empty.findings.is_empty() && empty.cut.is_none() && empty.rules.is_empty());
        let not_cut = checked_of(&json!({"cut": false, "cut_reason": null}), 0, 0, |_| String::new());
        assert!(not_cut.cut.is_none());
    }
}
