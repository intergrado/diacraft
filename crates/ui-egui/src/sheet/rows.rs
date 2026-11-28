//! Rows and sections from the window (design 5.12 and 5.13): the Sections, Rows and Editing groups of the ShapeSheet
//! Design tab and the items of the sheet's right-click menu.
//!
//! **The window adds and deletes rows and sections only through the engine's `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section`**, one undo step each, and
//! shows a refusal in words (the footer and `sheet.last`), as it shows a refused formula. What the window can know before it asks it uses to grey a control, with the engine's own words
//! for the reason (`diacraft_engine::draw::sheet_rows::rows_refusal` and `section_refusal`): the rows the text makes, a section that cannot go, and (a Geometry section that
//! holds an Ellipse or an Infinite Line row holds that row alone). What it cannot know (an insert in the middle of a master's own rows) the engine refuses and the footer says.
//!
//! * **Insert Row** on the ribbon adds the row **after** the selected one, and on the right-click menu **Insert Row** adds it **before** it and, in Geometry, **Insert Row After** after it
//!   With a header, or a cell of a section's own, selected the row goes at the end. The new row is selected.
//! * **Delete Row** deletes the selected row (a cell of a row is in its row); the selection stays where the row was.
//! * **Insert Section** opens the card of `cards.rs`; **Delete Section** deletes the section of the selected cell, greyed where the section may not go (Shape Transform).
//! * **The Editing group**: **Shape Data** adds a Shape Data row to the sheet, **Link** a Hyperlink row, and **Shape Action** an Actions row to the selected Actions section
//!   (`window.sheetAddRow`): the section is made first, with its one row, when the sheet has none.
//!
//! What a control may do is a [`Facts`] of the selection: worked out once for each drawing generation, revision, arrangement and selection in `sheet_prepare`, so that a frame asks no
//! engine and reads no cell.

use diacraft_engine::Result;
use diacraft_engine::doc::{Document, RowKey, SheetKind, ix_to_row_number, row_number_to_ix};
use diacraft_engine::draw::sheet_rows::{rows_refusal, section_refusal};
use diacraft_engine::sheetview::SectionInfo;
use serde_json::{Map, Value, json};

use super::edit::{Then, refusal_words};
use super::view::{Sel, View};
use super::{Last, bad, place_params, word};
use crate::app::DiacraftApp;

/// What a control of the Sections, Rows and Editing groups does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    InsertRow,
    DeleteRow,
    InsertSection,
    DeleteSection,
    EditFormula,
    Add(Add),
}

/// The three controls of the Editing group that add a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Add {
    /// Shape Data: a row of the Shape Data section.
    Data,
    /// Link: a row of the Hyperlinks section.
    Link,
    /// Shape Action: a row of the selected Actions section.
    Action,
}

impl Add {
    /// The section as the engine's `sheet.add_row` and `sheet.add_section` name it.
    pub fn section(self) -> &'static str {
        match self {
            Add::Data => "Prop",
            Add::Link => "Hyperlink",
            Add::Action => "Actions",
        }
    }

    /// What the window titles the section.
    pub fn label(self) -> &'static str {
        match self {
            Add::Data => "Shape Data",
            Add::Link => "Hyperlinks",
            Add::Action => "Actions",
        }
    }

    /// The section a text names: its name or its label, in any letter case.
    pub fn from_word(text: &str) -> Option<Add> {
        [Add::Data, Add::Link, Add::Action]
            .into_iter()
            .find(|a| a.section().eq_ignore_ascii_case(text.trim()) || a.label().eq_ignore_ascii_case(text.trim()))
    }

    /// The section a control's args name (`{"section": "Prop"}`).
    fn from_args(args: &str) -> Option<Add> {
        let v: Value = serde_json::from_str(args).ok()?;
        Add::from_word(v.get("section")?.as_str()?)
    }
}

/// The section the selection is in, with what the controls need of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SectionFact {
    /// How the outline names it (`Scratch`, `Geometry2`, `Shape Transform`) and what the window titles it.
    pub name: String,
    pub label: String,
    /// A section of rows (a table), not a display group.
    pub rows: bool,
    pub geometry: bool,
    pub can_add_rows: bool,
    pub can_delete: bool,
    /// Why rows cannot be added or deleted, when they cannot (the engine's words).
    pub rows_words: String,
    /// Why the section cannot be deleted, when it cannot (the engine's words).
    pub section_words: String,
    /// The type of the one row that holds the Geometry section alone (`Ellipse`, `InfiniteLine`).
    pub alone: Option<String>,
}

/// What the selection is, for the controls: the section it is in, the row it is or is a cell of, and whether it is a cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub section: Option<SectionFact>,
    pub row: Option<RowKey>,
    pub cell: bool,
}

fn section_fact(view: &View, doc: &Document, kind: SheetKind, info: &SectionInfo) -> SectionFact {
    let geometry = info.key.as_ref().is_some_and(|k| k.name.as_str() == "Geometry");
    let rows_words = if info.can_add_rows {
        String::new()
    } else {
        info.display.map_or_else(|| format!("{} rows cannot be added or deleted here", info.label), rows_refusal)
    };
    let section_words = if info.can_delete {
        String::new()
    } else {
        info.display.and_then(|d| section_refusal(d, kind)).unwrap_or_else(|| format!("{} has nothing to delete here", info.label))
    };
    // A section that holds an Ellipse or an Infinite Line row holds it alone: the one row's type says it.
    let alone = (geometry && info.rows_total == 1)
        .then(|| view.row_type_of(doc, info, 0))
        .flatten()
        .filter(|t| matches!(t.as_str(), "Ellipse" | "InfiniteLine"));
    SectionFact {
        name: info.name.clone(),
        label: info.label.clone(),
        rows: info.key.is_some(),
        geometry,
        can_add_rows: info.can_add_rows,
        can_delete: info.can_delete,
        rows_words,
        section_words,
        alone,
    }
}

/// What the selection `sel` of `view` is, for the controls.
pub(crate) fn facts_of(view: &View, doc: &Document, kind: SheetKind, sel: Option<&Sel>) -> Facts {
    let Some(sel) = sel else { return Facts::default() };
    let row = match sel {
        Sel::Row { row, .. } | Sel::Cell { row: Some(row), .. } => Some(row.clone()),
        _ => None,
    };
    let section = view.base().sections.iter().find(|s| s.name == sel.section()).map(|info| section_fact(view, doc, kind, info));
    Facts { section, row, cell: matches!(sel, Sel::Cell { .. }) }
}

impl Facts {
    /// The section of rows the selection is in, or the reason there is none.
    fn table(&self, none: &str) -> std::result::Result<&SectionFact, String> {
        let section = self.section.as_ref().ok_or_else(|| none.to_owned())?;
        if section.rows {
            Ok(section)
        } else {
            Err(format!("{} is a group of cells, not a section of rows: select a row of a section of rows", section.label))
        }
    }

    /// Whether `op` can be done on the selection, or why not in words.
    pub fn verdict(&self, op: Op) -> std::result::Result<(), String> {
        match op {
            Op::InsertSection => Ok(()),
            Op::EditFormula => {
                if self.cell {
                    Ok(())
                } else {
                    Err("select a cell to edit its formula".to_owned())
                }
            }
            Op::InsertRow => {
                let section = self.table("select a cell or a row of a section of rows first")?;
                if !section.can_add_rows {
                    return Err(section.rows_words.clone());
                }
                match &section.alone {
                    Some(kind) => Err(format!(
                        "{} holds an {kind} row, and a Geometry section that holds an Ellipse or an Infinite Line row holds that row alone: insert another Geometry section instead",
                        section.label
                    )),
                    None => Ok(()),
                }
            }
            Op::DeleteRow => {
                let section = self.table("select a row first: a press on its number selects it")?;
                if self.row.is_none() {
                    return Err("select a row first: a press on its number selects it".to_owned());
                }
                if section.can_add_rows { Ok(()) } else { Err(section.rows_words.clone()) }
            }
            Op::DeleteSection => {
                let section = self.section.as_ref().ok_or_else(|| "select a cell of the section to delete first".to_owned())?;
                if section.can_delete { Ok(()) } else { Err(section.section_words.clone()) }
            }
            Op::Add(Add::Action) => match &self.section {
                Some(section) if section.name == Add::Action.section() => Ok(()),
                _ => Err("select a cell or a row of the Actions section first: Shape Action adds a row to it".to_owned()),
            },
            Op::Add(_) => Ok(()),
        }
    }
}

/// The reference spelling of a row, as the engine's `after`, `before` and `row` take it: a name, or the number a reference gives it.
fn row_ref(geometry: bool, row: &RowKey) -> Value {
    match row {
        RowKey::Name(name) => json!(name.as_ref()),
        RowKey::Ix(ix) => ix_to_row_number(geometry, *ix).map_or(Value::Null, |n| json!(n)),
    }
}

/// The key of the row an answer names: a name, or the number a reference gives it.
fn row_key_of(value: &Value, geometry: bool) -> Option<RowKey> {
    match value {
        Value::String(name) => Some(RowKey::name(name)),
        Value::Number(n) => Some(RowKey::Ix(row_number_to_ix(geometry, u32::try_from(n.as_u64()?).ok()?)?)),
        _ => None,
    }
}

/// The row a `sheet.add_row` or `sheet.add_section` answer made, as a selection: the row, else the first row of the new section.
fn new_row_of(answer: &Value) -> Option<Sel> {
    let section = answer.get("section")?.as_str()?;
    let geometry = section.starts_with("Geometry");
    let row = answer.get("row").or_else(|| answer.get("rows").and_then(Value::as_array).and_then(|rows| rows.first()))?;
    Some(Sel::Row { section: section.to_owned(), row: row_key_of(row, geometry)? })
}

/// `n` and its noun, in the plural unless it is one.
fn counted(n: u64, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// What the footer says about the rows an insert or a delete numbered again, and the rows an instance took as its own first.
fn renumber_words(answer: &Value) -> String {
    let n = |path: &[&str]| -> u64 {
        let mut at = answer.get("renumbered");
        for key in path {
            at = at.and_then(|v| v.get(*key));
        }
        at.and_then(Value::as_u64).unwrap_or(0)
    };
    let (rows, formulas, copied) = (n(&["rows"]), n(&["formulas"]), n(&["copied", "rows"]));
    let mut out = String::new();
    let moved = format!(" {} after it {} numbered again", counted(rows, "row"), if rows == 1 { "was" } else { "were" });
    if rows > 0 && formulas > 0 {
        let follow = if formulas == 1 { "formula follows" } else { "formulas follow" };
        out.push_str(&format!("{moved}, and {formulas} {follow} the rows that moved."));
    } else if rows > 0 {
        out.push_str(&format!("{moved}."));
    }
    if copied > 0 {
        out.push_str(&format!(" The sheet took its own copy of the {} it inherits first.", counted(copied, "row")));
    }
    if let Some(note) = answer.get("note").and_then(Value::as_str) {
        out.push(' ');
        out.push_str(note);
    }
    out
}

/// What the Ops are in the window state: `{ok, reason}` for each.
pub(crate) fn verdict_json(verdict: &std::result::Result<(), String>) -> Value {
    match verdict {
        Ok(()) => json!({"ok": true, "reason": Value::Null}),
        Err(reason) => json!({"ok": false, "reason": reason}),
    }
}

impl DiacraftApp {
    /// Whether `op` can be done now on the sheet on top, or why not: no sheet, a sheet that is gone, or what the selection says.
    pub(crate) fn sheet_op_verdict(&self, op: Op) -> std::result::Result<(), String> {
        let sheet = &self.ui.sheet;
        let Some(tab) = sheet.tabs.get(sheet.active) else { return Err("no ShapeSheet is open".to_owned()) };
        if tab.view.as_ref().is_none_or(View::is_gone) {
            return Err("the sheet is not in the drawing".to_owned());
        }
        match &tab.facts {
            Some((_, facts)) => facts.verdict(op),
            None => Facts::default().verdict(op),
        }
    }

    /// Whether the selection is in a Geometry section: only there does the menu offer Insert Row After.
    pub(crate) fn sheet_in_geometry(&self) -> bool {
        let tab = self.ui.sheet.tabs.get(self.ui.sheet.active);
        tab.and_then(|t| t.facts.as_ref()).and_then(|(_, f)| f.section.as_ref()).is_some_and(|s| s.geometry)
    }

    /// What a window command of the three groups can do now (the registry's `enabled`): the verdict of the control that runs it, and for Shape Data, Link and Shape Action, which share
    /// `window.sheetAddRow` and are told apart by their section, that a sheet is open.
    pub(crate) fn sheet_command_verdict(&self, id: &str) -> std::result::Result<(), String> {
        self.sheet_ctl_verdict(Some(id), "").unwrap_or_else(|| self.sheet_acts_verdict())
    }

    /// What a ribbon control that names one of the row and section commands can do now: `None` for any other control.
    pub(crate) fn sheet_ctl_verdict(&self, command: Option<&str>, args: &str) -> Option<std::result::Result<(), String>> {
        let op = match command? {
            "window.sheetInsertRow" => Op::InsertRow,
            "window.sheetDeleteRow" => Op::DeleteRow,
            "window.sheetInsertSection" => Op::InsertSection,
            "window.sheetDeleteSection" => Op::DeleteSection,
            "window.sheetEditFormula" => Op::EditFormula,
            "window.sheetAddRow" => Op::Add(Add::from_args(args)?),
            _ => return None,
        };
        Some(self.sheet_op_verdict(op))
    }

    /// The window state's account of what each control of the three groups can do.
    pub(crate) fn sheet_ops_json(&self) -> Value {
        let ops = [
            ("insertRow", Op::InsertRow),
            ("deleteRow", Op::DeleteRow),
            ("insertSection", Op::InsertSection),
            ("deleteSection", Op::DeleteSection),
            ("editFormula", Op::EditFormula),
            ("shapeData", Op::Add(Add::Data)),
            ("link", Op::Add(Add::Link)),
            ("shapeAction", Op::Add(Add::Action)),
        ];
        let mut out = Map::new();
        for (name, op) in ops {
            out.insert(name.to_owned(), verdict_json(&self.sheet_op_verdict(op)));
        }
        Value::Object(out)
    }

    /// Makes the sheet on top ready and checks `op` for it. → the sheet and what the selection is, or the refusal in words.
    pub(crate) fn sheet_ready(&mut self, id: &str, op: Op) -> Result<(super::Place, Facts)> {
        self.sheet_prepare();
        // A text being typed is committed first, as a press on another cell commits it; one the engine refuses stays in the editor and stops the command.
        if op != Op::EditFormula && self.sheet_editing() {
            if !self.sheet_commit_edit(Then::Stay) {
                return Err(bad(id, "the text being edited was refused: correct it, or cancel the edit, first"));
            }
            self.sheet_prepare();
        }
        self.sheet_op_verdict(op).map_err(|words| bad(id, words))?;
        let tab = self.ui.sheet.tabs.get(self.ui.sheet.active).ok_or_else(|| bad(id, "no ShapeSheet is open"))?;
        Ok((tab.place.clone(), tab.facts.as_ref().map(|(_, f)| f.clone()).unwrap_or_default()))
    }

    /// Runs one of the engine's row or section commands. A refusal is kept for the window state's `sheet.last` and said in the footer, and is handed back as it is.
    pub(crate) fn sheet_run(&mut self, command: &str, params: &Value) -> Result<Value> {
        match self.execute(command, params) {
            Ok(answer) => {
                self.ui.sheet.last = Some(Last::Done(answer.clone()));
                Ok(answer)
            }
            Err(error) => {
                let words = refusal_words(&error);
                self.ui.sheet.last = Some(Last::Refused(words.clone()));
                self.sheet_say(words);
                Err(error)
            }
        }
    }

    /// Selects `sel` in the tab on top (its section is shown and opened) and says `words` in the footer for as long as the selection stays there. `None` leaves nothing selected.
    pub(crate) fn sheet_show(&mut self, sel: Option<Sel>, words: String) {
        let Some(tab) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active) else { return };
        tab.selected = sel;
        if let Some(sel) = &tab.selected {
            // A section Show Sections hides or a fold closes is shown again: what was added or selected is to be seen.
            let (hidden, folded) = (tab.hidden.remove(sel.section()), tab.collapsed.remove(sel.section()));
            if hidden || folded {
                tab.arrange_version += 1;
            }
            tab.ensure_visible = true;
        }
        tab.edit = None;
        tab.note = Some((tab.selected.clone(), words));
    }

    /// The selection after a delete: the row now at `at` in the section `name` (the last when there are fewer), else the section's header, else nothing.
    fn sheet_row_near(&mut self, name: &str, at: usize) -> Option<Sel> {
        self.sheet_prepare();
        let tab = self.ui.sheet.tabs.get(self.ui.sheet.active)?;
        let info = tab.view.as_ref()?.base().sections.iter().find(|s| s.name == name)?;
        Some(match info.rows.get(at).or_else(|| info.rows.last()) {
            Some(row) => Sel::Row { section: info.name.clone(), row: row.clone() },
            None => Sel::Header { section: info.name.clone() },
        })
    }

    /// `window.sheetInsertRow`: a row after the selected one (`at: after`, the ribbon's), or before it (`at: before`, the menu's Insert Row).
    pub(crate) fn sheet_insert_row_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        let at = match word(id, p, "at")? {
            None | Some("after") => "after",
            Some("before") => "before",
            Some(_) => return Err(bad(id, "`at` must be after or before")),
        };
        let (place, facts) = self.sheet_ready(id, Op::InsertRow)?;
        let section = facts.section.ok_or_else(|| bad(id, "no section is selected"))?;
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("section".to_owned(), json!(section.name));
            if let Some(row) = &facts.row {
                map.insert(at.to_owned(), row_ref(section.geometry, row));
            }
        }
        let answer = self.sheet_run("sheet.add_row", &params)?;
        let label = answer.get("label").and_then(Value::as_str).unwrap_or("").to_owned();
        let words = format!("Inserted row {label} of {}.{}", section.label, renumber_words(&answer));
        self.sheet_show(new_row_of(&answer), words);
        Ok(())
    }

    /// `window.sheetDeleteRow`: the selected row goes.
    pub(crate) fn sheet_delete_row_command(&mut self, id: &str) -> Result<()> {
        let (place, facts) = self.sheet_ready(id, Op::DeleteRow)?;
        let (Some(section), Some(row)) = (facts.section, facts.row) else { return Err(bad(id, "no row is selected")) };
        let position = self
            .ui
            .sheet
            .tabs
            .get(self.ui.sheet.active)
            .and_then(|t| t.view.as_ref())
            .and_then(|v| v.base().sections.iter().find(|s| s.name == section.name))
            .and_then(|info| info.rows.iter().position(|r| *r == row))
            .unwrap_or(0);
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("section".to_owned(), json!(section.name));
            map.insert("row".to_owned(), row_ref(section.geometry, &row));
        }
        let answer = self.sheet_run("sheet.delete_row", &params)?;
        let label = answer.get("label").and_then(Value::as_str).unwrap_or("").to_owned();
        let mut words = format!("Deleted row {label} of {}.{}", section.label, renumber_words(&answer));
        if answer.get("how").and_then(Value::as_str) == Some("marked") {
            words.push_str(" A master or a style gives it, so the sheet now marks it deleted.");
        }
        let near = self.sheet_row_near(&section.name, position);
        self.sheet_show(near, words);
        Ok(())
    }

    /// `window.sheetDeleteSection`: the section of the selected cell goes.
    pub(crate) fn sheet_delete_section_command(&mut self, id: &str) -> Result<()> {
        let (place, facts) = self.sheet_ready(id, Op::DeleteSection)?;
        let section = facts.section.ok_or_else(|| bad(id, "no section is selected"))?;
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("section".to_owned(), json!(section.name));
        }
        self.sheet_run("sheet.delete_section", &params)?;
        self.sheet_show(None, format!("Deleted {}.", section.label));
        Ok(())
    }

    /// `window.sheetAddRow`: the Editing group's Shape Data, Link and Shape Action: a row at the end of the section, which is made first, with its one row, when the sheet has none.
    pub(crate) fn sheet_add_row_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        let text = word(id, p, "section")?.ok_or_else(|| bad(id, "missing `section`"))?;
        let add = Add::from_word(text).ok_or_else(|| bad(id, "`section` must be Prop, Hyperlink or Actions"))?;
        let (place, _) = self.sheet_ready(id, Op::Add(add))?;
        let has =
            self.ui.sheet.tabs.get(self.ui.sheet.active).and_then(|t| t.view.as_ref()).is_some_and(|v| v.base().section(add.section()).is_some());
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("section".to_owned(), json!(add.section()));
        }
        let (command, verb) = if has { ("sheet.add_row", "Inserted row") } else { ("sheet.add_section", "Inserted section with row") };
        let answer = self.sheet_run(command, &params)?;
        let first = answer
            .get("label")
            .or_else(|| answer.get("rows").and_then(Value::as_array).and_then(|r| r.first()))
            .map_or_else(String::new, |l| l.as_str().map_or_else(|| l.to_string(), str::to_owned));
        let words = format!("{verb} {first} of {}.{}", add.label(), renumber_words(&answer));
        self.sheet_show(new_row_of(&answer), words);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use diacraft_engine::doc::RowKey;

    use super::*;

    fn section(name: &str, rows: bool) -> SectionFact {
        SectionFact {
            name: name.to_owned(),
            label: name.to_owned(),
            rows,
            geometry: name.starts_with("Geometry"),
            can_add_rows: rows,
            can_delete: true,
            rows_words: String::new(),
            section_words: String::new(),
            alone: None,
        }
    }

    #[test]
    fn nothing_selected_greys_every_control_but_insert_section() {
        let none = Facts::default();
        assert_eq!(none.verdict(Op::InsertSection), Ok(()));
        for op in [Op::InsertRow, Op::DeleteRow, Op::DeleteSection, Op::EditFormula, Op::Add(Add::Action)] {
            assert!(none.verdict(op).is_err(), "{op:?}");
        }
        assert_eq!(none.verdict(Op::Add(Add::Data)), Ok(()), "Shape Data and Link need no selection");
        assert_eq!(none.verdict(Op::Add(Add::Link)), Ok(()));
    }

    #[test]
    fn a_group_of_cells_has_no_rows_and_a_row_is_needed_to_delete_one() {
        let group = Facts { section: Some(section("Protection", false)), row: None, cell: true };
        assert!(group.verdict(Op::InsertRow).unwrap_err().contains("group of cells"));
        assert!(group.verdict(Op::DeleteRow).unwrap_err().contains("select a row"));
        let header = Facts { section: Some(section("Scratch", true)), row: None, cell: false };
        assert_eq!(header.verdict(Op::InsertRow), Ok(()), "a header: the row goes at the end");
        assert!(header.verdict(Op::DeleteRow).unwrap_err().contains("select a row first"));
        let row = Facts { section: Some(section("Scratch", true)), row: Some(RowKey::Ix(1)), cell: false };
        assert_eq!((row.verdict(Op::InsertRow), row.verdict(Op::DeleteRow)), (Ok(()), Ok(())));
        assert!(row.verdict(Op::EditFormula).is_err(), "a row is no cell");
    }

    #[test]
    fn a_section_that_cannot_take_rows_or_go_says_why_in_the_engines_words() {
        let mut text = section("Character", true);
        text.can_add_rows = false;
        text.rows_words = "Character rows are made by the text".to_owned();
        text.can_delete = false;
        text.section_words = "Character cannot be inserted or deleted here".to_owned();
        let facts = Facts { section: Some(text), row: Some(RowKey::Ix(0)), cell: true };
        assert_eq!(facts.verdict(Op::InsertRow), Err("Character rows are made by the text".to_owned()));
        assert_eq!(facts.verdict(Op::DeleteRow), Err("Character rows are made by the text".to_owned()));
        assert_eq!(facts.verdict(Op::DeleteSection), Err("Character cannot be inserted or deleted here".to_owned()));
    }

    #[test]
    fn a_geometry_section_that_holds_an_ellipse_alone_takes_no_row_but_can_lose_it() {
        let mut ellipse = section("Geometry2", true);
        ellipse.alone = Some("Ellipse".to_owned());
        let facts = Facts { section: Some(ellipse), row: Some(RowKey::Ix(1)), cell: false };
        let words = facts.verdict(Op::InsertRow).unwrap_err();
        assert!(words.contains("Ellipse") && words.contains("alone"), "{words}");
        assert_eq!(facts.verdict(Op::DeleteRow), Ok(()));
    }

    #[test]
    fn shape_action_needs_the_actions_section_selected() {
        let outside = Facts { section: Some(section("Scratch", true)), row: None, cell: false };
        assert!(outside.verdict(Op::Add(Add::Action)).unwrap_err().contains("Actions"));
        let inside = Facts { section: Some(section("Actions", true)), row: None, cell: false };
        assert_eq!(inside.verdict(Op::Add(Add::Action)), Ok(()));
    }

    #[test]
    fn a_row_is_a_name_or_the_number_a_reference_gives_it_and_the_two_ways_agree() {
        assert_eq!(row_ref(false, &RowKey::Ix(1)), json!(2), "outside Geometry the number is the IX plus one");
        assert_eq!(row_ref(true, &RowKey::Ix(1)), json!(1), "in Geometry it is the IX");
        assert_eq!(row_ref(false, &RowKey::name("Cost")), json!("Cost"));
        assert_eq!(row_ref(false, &RowKey::Ix(u32::MAX)), Value::Null, "a number that does not fit is none");
        for geometry in [false, true] {
            for ix in [0_u32, 1, 7, 4_000_000] {
                let key = RowKey::Ix(ix);
                if let Some(back) = row_key_of(&row_ref(geometry, &key), geometry) {
                    assert_eq!(back, key, "{geometry} {ix}");
                }
            }
        }
        assert_eq!(row_key_of(&json!("Row_3"), false), Some(RowKey::name("Row_3")));
        assert_eq!(row_key_of(&json!(-1), false), None);
        assert_eq!(row_key_of(&json!(4_294_967_296_u64), false), None);
        assert_eq!(row_key_of(&Value::Null, false), None);
    }

    #[test]
    fn the_row_an_answer_made_is_selected_by_its_row_or_the_first_row_of_its_section() {
        let row = json!({"section": "Scratch", "row": 3});
        assert_eq!(new_row_of(&row), Some(Sel::Row { section: "Scratch".to_owned(), row: RowKey::Ix(2) }));
        let geometry = json!({"section": "Geometry2", "row": 2});
        assert_eq!(new_row_of(&geometry), Some(Sel::Row { section: "Geometry2".to_owned(), row: RowKey::Ix(2) }));
        let section = json!({"section": "Prop", "rows": ["Row_1"]});
        assert_eq!(new_row_of(&section), Some(Sel::Row { section: "Prop".to_owned(), row: RowKey::name("Row_1") }));
        assert_eq!(new_row_of(&json!({"section": "Text Transform", "rows": []})), None, "a group of cells has no row");
    }

    #[test]
    fn the_footer_says_what_a_renumbering_did() {
        assert_eq!(renumber_words(&json!({})), "");
        let moved = json!({"renumbered": {"rows": 2, "formulas": 3, "copied": {"rows": 0}}});
        assert_eq!(renumber_words(&moved), " 2 rows after it were numbered again, and 3 formulas follow the rows that moved.");
        let rows_only = json!({"renumbered": {"rows": 1, "formulas": 0}});
        assert_eq!(renumber_words(&rows_only), " 1 row after it was numbered again.");
        let one = json!({"renumbered": {"rows": 1, "formulas": 1}});
        assert_eq!(renumber_words(&one), " 1 row after it was numbered again, and 1 formula follows the rows that moved.");
        let copied = json!({"renumbered": {"rows": 0, "copied": {"rows": 4}}, "note": "rows past the 10th are ignored"});
        assert_eq!(renumber_words(&copied), " The sheet took its own copy of the 4 rows it inherits first. rows past the 10th are ignored");
    }

    #[test]
    fn the_editing_groups_controls_name_their_section() {
        assert_eq!(Add::from_args(r#"{"section":"Prop"}"#), Some(Add::Data));
        assert_eq!(Add::from_args(r#"{"section":"hyperlink"}"#), Some(Add::Link));
        assert_eq!(Add::from_args(r#"{"section":"Actions"}"#), Some(Add::Action));
        assert_eq!(Add::from_word("Shape Data"), Some(Add::Data));
        assert_eq!(Add::from_args(r#"{"section":"Geometry1"}"#), None);
        assert_eq!(Add::from_args("{}"), None);
        assert_eq!(Add::from_args("not json"), None);
    }
}
