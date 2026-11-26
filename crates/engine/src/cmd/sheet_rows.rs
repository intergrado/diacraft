//! The rows and sections of a ShapeSheet: `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section`, each one undo step and all or nothing, with
//! their rules in [`crate::draw::sheet_rows`] and the registry's locked-layer rule for a
//! shape of a page (`cmd/locked.rs`: a `sheet.*` call names a shape by its `id`). The sheet is named as every `sheet.*` command names it
//! and a row of a numbered section as a reference spells it (a Geometry row's `IX`, any other's one more).

use serde_json::{Value, json};

use super::{CommandSpec, has_document, optional_text, required_text};
use crate::Session;
use crate::draw::sheet_rows::{self as rows, AddRow, At, RowDone, RowRef, SectionDone};
use crate::error::{Result, bad};
use crate::sheetview::{Target, row_key_json};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "sheet.add_row",
            "Insert a ShapeSheet row",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet gets the row (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Adds one row to a section the sheet has, as one undo step: the sections that take rows are Geometry, Connection Points, Controls, Scratch, Shape Data, User-defined Cells, Hyperlinks, Actions, Action Tag and the stops of the two gradients \
             (the text makes the rows of Character, Paragraph, Tabs and Text Fields, layer.add those of Layers, the comments those of Reviewer and Annotation). \
             Answers {sheet, section, label, row, type, cells, renumbered, note}: section the spelling a reference uses, row the row's number (a Geometry row's IX, any other indexed row's IX plus one) or its name and label how a reference spells it (Prop.Row_2, 3), \
             cells the cells the new row holds as sheet.read gives them (every cell the catalogue lists for the section at its default, a Geometry row's X and Y those of the row before it and its other cells those its type uses, at 0), \
             and renumbered {rows, sections, formulas, connects, work} what an insert in the middle numbered again. \
             An indexed row keeps its number as its key: a number nobody holds is taken and nothing moves, a number a row holds is taken by that row and the run of rows after it, which move one up, and every formula of the page that names one of them is written again with its new number \
             (the evaluator's own reading of a reference finds them, the bytes of the number are all that change, a text that holds the name of the section in a string is not a reference), as are the Connect rows of a connection point that moved. \
             A master's own rows and a style's own rows are numbered by the instances and the shapes that read them, so an insert in the middle of those is refused and the row goes at the end. An instance (or a shape that reads a style) that inherits the section takes its own copy of the rows first and the insert then goes in, in the same undo step, and a later edit of the master's rows no longer reaches it. A Geometry section that holds an Ellipse or an Infinite Line row holds that row alone, and such a row is added only to an empty section. \
             An unnamed row of a named section is Row_1, Row_2 and on, the first not taken in any letter case. A refusal is in words and changes nothing; \
             page: whole number, the page, whose sheet is read when it is alone; \
             master: whole number, the master, whose page sheet is changed when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is changed; \
             document: true, the document's sheet is changed; \
             section: text, the section as sheet.read names it (Geometry1, Scratch, User, Prop, Connections, Controls, Hyperlink, Actions, SmartTags, FillGradientStops, LineGradientStops) or by its label (Shape Data, Connection Points) in any letter case; \
             after: whole number or text, optional, the row the new one goes after (a row's name in any letter case, or its number as a reference spells it, the end when it is left out); \
             before: whole number or text, optional, the row the new one goes before (not with after); \
             name: text, optional, the name of a row of a named section (letters, digits and underscores, not starting with a digit, at most 256 characters, not a row of the section in any letter case); \
             type: text, optional, a Geometry row's type (MoveTo, RelMoveTo, LineTo, RelLineTo, ArcTo, EllipticalArcTo, RelEllipticalArcTo, PolylineTo, NURBSTo, SplineStart, SplineKnot, RelCubBezTo, RelQuadBezTo, Ellipse, InfiniteLine, LineTo when it is left out)",
            has_document,
            add_row
        ),
        cmd!(
            "sheet.delete_row",
            "Delete a ShapeSheet row",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet loses the row (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Deletes one row of a section the sheet has, as one undo step. A row the sheet holds and no master or style gives is taken out, and where the section is the sheet's own the rows after it move one down and every formula of the page that names one \
             of them is written again with its new number, a formula that named the row itself reading #REF! (REF()), as the Connect rows of a connection point are (the one glued to a point that went is dropped). \
             A row a master or a style gives is dropped by a deleted row (Del) of the sheet's, with its key and no cell, and the numbers of the other rows stay. A master's own rows keep their numbers too. \
             The first row of a Geometry section cannot go while the rows after it do not begin a path. Answers {sheet, section, label, row, how, renumbered}: how is removed or marked, and renumbered as sheet.add_row gives it. A refusal is in words and changes nothing; \
             page: whole number, the page, whose sheet is changed when it is alone; \
             master: whole number, the master, whose page sheet is changed when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is changed; \
             document: true, the document's sheet is changed; \
             section: text, the section as sheet.read names it (Geometry1, Scratch, User, Prop, Connections, Controls, Hyperlink, Actions, SmartTags) or by its label (Shape Data) in any letter case; \
             row: whole number or text, the row (a row's name in any letter case, or its number as a reference spells it)",
            has_document,
            delete_row
        ),
        cmd!(
            "sheet.add_section",
            "Insert a ShapeSheet section",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet gets the section (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Adds a section as one undo step, the ones sheet.sections lists in can_add: Geometry (also as Ellipse or InfiniteLine), Connection Points and Controls on a shape only, Scratch, Shape Data, User-defined Cells, Hyperlinks, Actions and Action Tag on any sheet, \
             Text Transform and Layer Membership on a shape. A section other than Geometry is there once. A new section holds one row (a Geometry section a MoveTo and a LineTo, an Ellipse or an Infinite Line section one row, Text Transform the seven formulas \
             a shape reads when it has none and Layer Membership an empty LayerMember). Answers {sheet, section, label, ix, rows, cells, renumbered}: section the spelling a reference uses (Geometry2) and rows the keys of the rows it holds. A refusal is in words and changes nothing; \
             page: whole number, the page, whose sheet is changed when it is alone; \
             master: whole number, the master, whose page sheet is changed when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is changed; \
             document: true, the document's sheet is changed; \
             section: text, the section as sheet.sections names it in can_add (Geometry, Scratch, User, Prop, Connections, Controls, Hyperlink, Actions, SmartTags, Text Transform, Layer Membership) or by its label, in any letter case; \
             variant: text, optional, only for Geometry: Ellipse or InfiniteLine",
            has_document,
            add_section
        ),
        cmd!(
            "sheet.delete_section",
            "Delete a ShapeSheet section",
            ["Developer", "Shape Design"],
            None,
            "id: whole number, the shape whose sheet loses the section (with page: on that page, else on the first page that holds it, and with master: the shape of that master). \
             Give one sheet: id (with page), page, master (with id), style or document. \
             Deletes a section as one undo step, the ones sheet.sections lists in can_delete (Shape Transform never). A section the sheet holds and no master or style gives is taken out with its rows, and a formula of the page that names its rows reads #REF! \
             (REF()), and a Geometry section that goes numbers the later ones again and the formulas that name them follow. A section a master or a style gives is dropped by a deleted section (Del) of the sheet's. Text Transform and Layer Membership \
             are deleted by taking their cells out, and a shape then reads the defaults of the block again. Answers {sheet, section, label, ix, how, removed_cells, renumbered}. A refusal is in words and changes nothing; \
             page: whole number, the page, whose sheet is changed when it is alone; \
             master: whole number, the master, whose page sheet is changed when it is alone, and with id the master's shape; \
             style: whole number, the style, whose sheet is changed; \
             document: true, the document's sheet is changed; \
             section: text, the section as sheet.read names it (Geometry1, Scratch, User, Prop, Connections, Controls, Hyperlink, Actions, SmartTags, Text Transform, Layer Membership) or by its label, in any letter case",
            has_document,
            delete_section
        ),
    ]
}

/// A row a call gives for `key`: a whole number from 0 to 4,294,967,295 or text.
fn row_ref(p: &Value, key: &str, cmd: &str) -> Result<Option<RowRef>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(RowRef::Text(text.clone()))),
        Some(value) => match value.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Ok(Some(RowRef::Number(n))),
            None => Err(bad(cmd, format!("`{key}` must be a row's name or a whole number from 0 to {}", u32::MAX))),
        },
    }
}

/// The renumbering an answer reports.
fn renumbered_json(r: &rows::Renumbered) -> Value {
    json!({
        "rows": r.rows,
        "sections": r.sections,
        "formulas": r.formulas,
        "connects": r.connects,
        "copied": {"rows": r.copied_rows, "cells": r.copied_cells},
        "work": {"visited": r.work.visited, "parsed": r.work.parsed, "located": r.work.located},
    })
}

fn row_json(done: &RowDone) -> Value {
    let (row, label) = done.section.key.as_ref().map_or((Value::Null, String::new()), |key| row_key_json(key, &done.row));
    let mut out = json!({
        "sheet": done.place.json(),
        "section": done.section.name,
        "label": label,
        "row": row,
        "type": done.row_type,
        "renumbered": renumbered_json(&done.renumbered),
    });
    if let Some(map) = out.as_object_mut() {
        if let Some(how) = done.how {
            map.insert("how".into(), json!(how.word()));
        } else {
            map.insert("cells".into(), Value::Array(done.cells.iter().map(|c| c.json()).collect()));
        }
        if let Some(note) = &done.note {
            map.insert("note".into(), json!(note));
        }
    }
    out
}

fn section_json(done: &SectionDone) -> Value {
    let rows: Vec<Value> = done.section.key.as_ref().map_or_else(Vec::new, |key| done.rows.iter().map(|row| row_key_json(key, row).0).collect());
    let mut out = json!({
        "sheet": done.place.json(),
        "section": done.section.name,
        "label": done.section.label,
        "ix": done.section.ix,
        "rows": rows,
        "renumbered": renumbered_json(&done.renumbered),
    });
    if let Some(map) = out.as_object_mut() {
        match done.how {
            Some(how) => {
                map.insert("how".into(), json!(how.word()));
                map.insert("removed_cells".into(), json!(done.removed_cells));
            }
            None => {
                map.insert("cells".into(), Value::Array(done.cells.iter().map(|c| c.json()).collect()));
            }
        }
    }
    out
}

/// `sheet.add_row`.
fn add_row(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.add_row";
    let target = Target::from_params(p).map_err(|words| bad(ID, words))?;
    let section = required_text(p, "section", ID)?;
    let (after, before) = (row_ref(p, "after", ID)?, row_ref(p, "before", ID)?);
    let at = match (after, before) {
        (Some(_), Some(_)) => return Err(bad(ID, "give after or before, not both")),
        (Some(row), None) => At::After(row),
        (None, Some(row)) => At::Before(row),
        (None, None) => At::End,
    };
    let request = AddRow {
        section: section.to_owned(),
        at,
        name: optional_text(p, "name", ID)?.map(str::to_owned),
        row_type: optional_text(p, "type", ID)?.map(str::to_owned),
    };
    let done = s.edit_if("Insert row", |doc| {
        let done = rows::add_row(doc, &target, &request).map_err(|problem| problem.into_engine(ID))?;
        Ok((done, true))
    })?;
    Ok(row_json(&done))
}

/// `sheet.delete_row`.
fn delete_row(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.delete_row";
    let target = Target::from_params(p).map_err(|words| bad(ID, words))?;
    let section = required_text(p, "section", ID)?;
    let row = row_ref(p, "row", ID)?.ok_or_else(|| bad(ID, "missing `row`"))?;
    let done = s.edit_if("Delete row", |doc| {
        let done = rows::delete_row(doc, &target, section, &row).map_err(|problem| problem.into_engine(ID))?;
        Ok((done, true))
    })?;
    Ok(row_json(&done))
}

/// `sheet.add_section`.
fn add_section(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.add_section";
    let target = Target::from_params(p).map_err(|words| bad(ID, words))?;
    let section = required_text(p, "section", ID)?;
    let variant = optional_text(p, "variant", ID)?;
    let done = s.edit_if("Insert section", |doc| {
        let done = rows::add_section(doc, &target, section, variant).map_err(|problem| problem.into_engine(ID))?;
        Ok((done, true))
    })?;
    Ok(section_json(&done))
}

/// `sheet.delete_section`.
fn delete_section(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "sheet.delete_section";
    let target = Target::from_params(p).map_err(|words| bad(ID, words))?;
    let section = required_text(p, "section", ID)?;
    let done = s.edit_if("Delete section", |doc| {
        let done = rows::delete_section(doc, &target, section).map_err(|problem| problem.into_engine(ID))?;
        Ok((done, true))
    })?;
    Ok(section_json(&done))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Every key of a command's params doc is its own entry, the five sheet keys are on each, and each command writes (it is journaled).
    #[test]
    fn the_params_docs_list_the_five_sheet_keys_and_each_command_its_own() {
        let own: [(&str, &[&str]); 4] = [
            ("sheet.add_row", &["section", "after", "before", "name", "type"]),
            ("sheet.delete_row", &["section", "row"]),
            ("sheet.add_section", &["section", "variant"]),
            ("sheet.delete_section", &["section"]),
        ];
        for spec in specs() {
            let keys = super::super::param_keys(spec.params).unwrap();
            assert_eq!(&keys[..5], ["id", "page", "master", "style", "document"], "{}", spec.id);
            let want = own.iter().find(|(id, _)| *id == spec.id).map(|(_, keys)| *keys).unwrap();
            assert_eq!(&keys[5..], want, "{}", spec.id);
            assert!(spec.journal, "{}: a row or a section is a change", spec.id);
        }
    }

    #[test]
    fn a_row_is_a_name_or_a_whole_number_and_nothing_else() {
        let p = json!({"after": 3, "before": "Cost", "row": -1, "x": 1.5, "y": 4_294_967_296_u64, "z": null});
        assert_eq!(row_ref(&p, "after", "c").unwrap(), Some(RowRef::Number(3)));
        assert_eq!(row_ref(&p, "before", "c").unwrap(), Some(RowRef::Text("Cost".into())));
        assert_eq!(row_ref(&p, "z", "c").unwrap(), None);
        assert_eq!(row_ref(&p, "missing", "c").unwrap(), None);
        for key in ["row", "x", "y"] {
            assert!(row_ref(&p, key, "c").unwrap_err().to_string().contains("must be a row's name or a whole number"), "{key}");
        }
    }
}
