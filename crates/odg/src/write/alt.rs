//! A shape's accessible title and description in the writer: the elements `svg:title` and `svg:desc`, made from the
//! rows `visAltText` and `visAltDescription` of the shape's `User` section, in the place R48 gives them.
//!
//! **A new shape** is written with the elements its texts need ([`element`], and `shapes.rs`'s `put_alt_text`): the shape's **effective** texts, so one that takes them from its master is written
//! with them (`.odg` has no masters). **A shape of the file** keeps an element as it was while its row is unchanged (the save is no edit); when the row's text changes, the element's
//! text is replaced; when a row has text and the shape has no element, one is put in the place; when a row becomes empty, the element is taken out ([`Editor::alt_text`]).
//!
//! **Where R48 puts them.** On a shape they are its first children: before its event listeners, its glue points and its text (a custom shape's geometry too), title first. On a frame they stand
//! after the frame's content, its event listeners and its glue points, at the end of the element. With one of the two in the file already, the other goes beside it in the schema's order: a
//! description after the title, a title before the description. **The element is written as it is made**: the text escaped (`&`, `<`, `>`, and a carriage return as its reference, which a
//! reader gives back), every space and line break kept, and refused in words when it holds a character XML 1.0 cannot hold.

use diacraft_doc::alt_text::{DESCRIPTION_ROW, TITLE_ROW};

use super::splice::{WORDS_XML_CHAR, has_bad_char};
use super::{WriteError, WriteWork};

/// The text as element content: the characters XML reserves are escaped, and a carriage return is written as its reference (a literal one reads as a line feed).
pub(crate) fn content(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

/// `<prefix:local>text</prefix:local>`, the text as [`content`] writes it. Refused for a text with a character XML 1.0 cannot hold.
pub(crate) fn element(prefix: &str, local: &str, text: &str) -> Result<String, WriteError> {
    if has_bad_char(text) {
        return Err(WriteError::Unsupported(WORDS_XML_CHAR));
    }
    Ok(format!("<{prefix}:{local}>{}</{prefix}:{local}>", content(text)))
}

/// The name of the element a row is written as: `title` for `visAltText`, `desc` for `visAltDescription`.
pub(crate) fn local_of_row(row: &str) -> Option<&'static str> {
    match row {
        TITLE_ROW => Some("title"),
        DESCRIPTION_ROW => Some("desc"),
        _ => None,
    }
}

/// What a save does to the title and the description of a shape of the file: for each, `None` leaves its element as it is, `Some(text)` writes it (an element is replaced or
/// made) and `Some("")` takes the element out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AltEdit {
    pub title: Option<String>,
    pub description: Option<String>,
}

impl AltEdit {
    /// Whether the edit changes nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none()
    }

    /// The most bytes of text the edit writes, for the charge made before it is.
    pub(crate) fn bytes(&self) -> usize {
        self.title.as_deref().map_or(0, str::len).saturating_add(self.description.as_deref().map_or(0, str::len))
    }

    /// Charges the work of writing the edit: a step, and the bytes by the writer's own charge for copied text.
    pub(crate) fn charge(&self, work: &mut WriteWork, limits: &crate::limits::Limits) -> Result<(), WriteError> {
        work.charge_steps(1)?;
        work.charge_copy(self.bytes(), limits)
    }
}

/// The text of the sheet's own row `name` in any form (the value of its `Value` cell; an error is no text): `None` for a sheet with no such row.
fn own<'a>(sheet: &'a diacraft_doc::Sheet, name: &str) -> Option<&'a str> {
    let section = sheet.section(&diacraft_doc::SectionKey::new(diacraft_doc::alt_text::SECTION, None))?;
    let row = section.row(&diacraft_doc::RowKey::name(name)).filter(|row| !row.del)?;
    let cell = row.cells.get(diacraft_doc::alt_text::VALUE_CELL)?;
    Some(if cell.error.is_some() { "" } else { cell.value.as_ref().map_or("", diacraft_doc::Value::text) })
}

/// What a save writes of the title and the description of a shape of the file whose sheet changed from `old` to `new`: the text of each own row that is another, `""` for a row that is
/// gone. A text that is the same writes nothing, and an instance's master text it has no row for is not its to write (the element it was written with stands).
pub(crate) fn edit_of(old: &diacraft_doc::Sheet, new: &diacraft_doc::Sheet) -> Option<AltEdit> {
    let change = |name: &str| {
        let (was, now) = (own(old, name), own(new, name));
        (was != now).then(|| now.unwrap_or("").to_owned())
    };
    let edit = AltEdit { title: change(TITLE_ROW), description: change(DESCRIPTION_ROW) };
    (!edit.is_empty()).then_some(edit)
}

/// The sections without their rows of alt text in the form the files hold: what is compared to tell whether DiaCraft's data has to carry the `User` section (the title and the
/// description are the file's own markup). A section that has nothing else is none: its attributes and the nodes it keeps are something else, and a change of them is a change.
pub(crate) fn without_standard_rows(sections: &[std::sync::Arc<diacraft_doc::Section>]) -> Vec<std::sync::Arc<diacraft_doc::Section>> {
    let alt_row = |r: &std::sync::Arc<diacraft_doc::Row>| {
        r.name.as_deref().is_some_and(|n| local_of_row(n).is_some()) && diacraft_doc::alt_text::standard_text(r).is_some()
    };
    sections
        .iter()
        .filter_map(|s| {
            if !s.rows.iter().any(alt_row) {
                return Some(std::sync::Arc::clone(s));
            }
            let mut kept = diacraft_doc::Section::clone(s);
            kept.rows = s.rows.iter().filter(|r| !alt_row(r)).cloned().collect();
            let nothing =
                kept.rows.is_empty() && kept.cells.is_empty() && !kept.del && kept.ix.is_none() && kept.raw.is_empty() && kept.passthrough.is_empty();
            (!nothing).then(|| std::sync::Arc::new(kept))
        })
        .collect()
}

/// ` xmlns:svg1="..."`, the declaration of the writer's `svg` prefix among those it makes for an element put where the shape's scope is in force: empty when the prefix is bound already.
fn svg_declaration(pfx: &super::emit::Pfx) -> String {
    let start = format!(" xmlns:{}=\"", pfx.svg);
    let Some(at) = pfx.declare.find(&start) else { return String::new() };
    let rest = pfx.declare.get(at..).unwrap_or_default();
    let end = rest.get(start.len()..).and_then(|r| r.find('"')).map_or(rest.len(), |k| start.len() + k + 1);
    rest.get(..end).unwrap_or_default().to_owned()
}

/// Where an element of the title or the description stands in the part: where its unit starts, where it ends, and the bytes between its tags.
type Place = (usize, usize, (usize, usize));

/// The elements of a shape the reader makes the title and the description of: the first plain one of each kind.
struct Found {
    title: Option<Place>,
    description: Option<Place>,
}

impl Found {
    fn of(kids: &[super::scan::Kid]) -> Found {
        use super::scan::Kid;
        let find = |desc: bool| {
            kids.iter().find_map(|k| match k {
                Kid::Alt { desc: d, plain: true, lead, end, content } if *d == desc => Some((*lead, *end, *content)),
                _ => None,
            })
        };
        Found { title: find(false), description: find(true) }
    }

    fn of_kind(&self, desc: bool) -> Option<Place> {
        if desc { self.description } else { self.title }
    }
}

impl super::frame::Editor<'_, '_> {
    /// Writes the title and the description of a shape of the file (see the module documentation): an element of a text that changed has its content replaced, a text that is now empty
    /// has its element taken out, and a text with no element has one put where the schema puts it. A shape written empty is opened for them.
    pub(crate) fn alt_text(&mut self, shape: &super::scan::ScanShape, edit: &AltEdit) -> Result<(), WriteError> {
        edit.charge(self.work, self.limits)?;
        if [&edit.title, &edit.description].into_iter().flatten().any(|text| has_bad_char(text)) {
            return Err(WriteError::Unsupported(WORDS_XML_CHAR));
        }
        let kids: Vec<super::scan::Kid> = (shape.kids.0..shape.kids.1).filter_map(|i| self.kids.get(usize::try_from(i).ok()?).cloned()).collect();
        let found = Found::of(&kids);
        let pfx = super::emit::Pfx::of(&mut self.scopes, shape.scope, self.work);
        let declare = svg_declaration(&pfx);
        // The elements to make: a text that is not empty and has no element yet.
        let made: Vec<(bool, String)> = [(false, &edit.title), (true, &edit.description)]
            .into_iter()
            .filter_map(|(desc, text)| {
                let text = text.as_deref().filter(|t| !t.is_empty())?;
                found.of_kind(desc).is_none().then(|| {
                    let local = if desc { "desc" } else { "title" };
                    (desc, format!("<{p}:{local}{declare}>{}</{p}:{local}>", content(text), p = pfx.svg))
                })
            })
            .collect();
        // A shape written empty has no place to put an element in: its tag is opened, with the elements it needs and its end tag.
        if shape.end == shape.open.1 && shape.close_start == shape.open.1 {
            self.open_with(shape, &made);
            return Ok(());
        }
        for (desc, text) in [(false, &edit.title), (true, &edit.description)] {
            let Some(text) = text else { continue };
            match (found.of_kind(desc), text.is_empty()) {
                (Some((lead, end, _)), true) => self.splicer.remove(lead, end),
                (Some((_, _, between)), false) => self.splicer.replace(between.0, between.1, content(text)),
                (None, true) => {}
                (None, false) => {
                    if let Some((_, new)) = made.iter().find(|(d, _)| *d == desc) {
                        self.put_element(shape, &found, desc, new.clone());
                    }
                }
            }
        }
        Ok(())
    }

    /// Opens the tag of a shape written empty (`<draw:rect .../>`) round the elements `made`: the end of the tag is `>` and the end tag follows them. Nothing is done for none.
    pub(super) fn open_with(&mut self, shape: &super::scan::ScanShape, made: &[(bool, String)]) {
        if made.is_empty() {
            return;
        }
        let head = self.bytes.get(shape.open.0 + 1..shape.open.1).unwrap_or_default();
        let name_len = head.iter().position(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>')).unwrap_or(head.len());
        let qname = String::from_utf8_lossy(head.get(..name_len).unwrap_or_default()).into_owned();
        let all: String = made.iter().map(|(_, text)| text.as_str()).collect();
        self.splicer.replace(shape.open.1.saturating_sub(2), shape.open.1, format!(">{all}</{qname}>"));
    }

    /// Puts the new element `new` of the title (`desc` false) or the description of a shape that has none of that kind: a description after the title that stands, a title before the
    /// description that stands; with neither, a frame's go after its content and a shape's before everything else it holds (title first: the edits at one place keep the order they were made in).
    fn put_element(&mut self, shape: &super::scan::ScanShape, found: &Found, desc: bool, new: String) {
        match (desc, found.title, found.description) {
            (true, Some((_, end, _)), _) => self.splicer.insert(end, new),
            (false, _, Some((lead, _, _))) => self.splicer.insert(lead, new),
            _ if shape.kind == super::scan::Kind::Frame => self.splicer.insert(shape.close_start, new),
            _ => self.splicer.insert_first(shape.open.1, new),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::alt_text::standard_row;
    use diacraft_doc::{Row, Section, Sheet};

    use super::*;

    #[test]
    fn content_escapes_what_xml_reserves_and_a_carriage_return_and_nothing_else() {
        assert_eq!(content("a & <b> \"c\" 'd'\te\nf"), "a &amp; &lt;b&gt; \"c\" 'd'\te\nf");
        assert_eq!(content("x\r\ny\r"), "x&#13;\ny&#13;");
        assert_eq!(content("  spaced  "), "  spaced  ");
        assert_eq!(content(""), "");
    }

    #[test]
    fn an_element_is_made_under_the_prefix_and_refused_for_a_character_xml_cannot_hold() {
        assert_eq!(element("svg", "title", "A & B").unwrap(), "<svg:title>A &amp; B</svg:title>");
        assert_eq!(element("s1", "desc", "").unwrap(), "<s1:desc></s1:desc>");
        for bad in ["a\u{0}b", "a\u{1b}b", "a\u{fffe}b"] {
            assert!(matches!(element("svg", "title", bad), Err(WriteError::Unsupported(_))), "{bad:?}");
        }
        assert_eq!(local_of_row(TITLE_ROW), Some("title"));
        assert_eq!(local_of_row(DESCRIPTION_ROW), Some("desc"));
        assert_eq!(local_of_row("visDescription"), None);
    }

    fn sheet_with(rows: &[(&str, &str)]) -> Sheet {
        let mut section = Section::new("User", None);
        for (name, text) in rows {
            section.rows.push(Arc::new(standard_row(name, text)));
        }
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(section));
        sheet
    }

    #[test]
    fn an_edit_is_the_text_of_each_own_row_that_changed_and_empty_for_one_that_is_gone() {
        let was = sheet_with(&[(TITLE_ROW, "Pump"), (DESCRIPTION_ROW, "Feeds")]);
        assert_eq!(edit_of(&was, &was), None);
        let now = sheet_with(&[(TITLE_ROW, "Valve"), (DESCRIPTION_ROW, "Feeds")]);
        assert_eq!(edit_of(&was, &now), Some(AltEdit { title: Some("Valve".to_owned()), description: None }));
        let cleared = sheet_with(&[(TITLE_ROW, ""), (DESCRIPTION_ROW, "Feeds")]);
        assert_eq!(edit_of(&was, &cleared), Some(AltEdit { title: Some(String::new()), description: None }));
        let gone = sheet_with(&[(DESCRIPTION_ROW, "Feeds")]);
        assert_eq!(edit_of(&was, &gone), Some(AltEdit { title: Some(String::new()), description: None }), "a row that is gone is an empty text");
        let added = sheet_with(&[(DESCRIPTION_ROW, "Feeds"), (TITLE_ROW, "New")]);
        assert_eq!(edit_of(&gone, &added), Some(AltEdit { title: Some("New".to_owned()), description: None }));
        assert_eq!(edit_of(&Sheet::default(), &Sheet::default()), None);
        // The same texts in rows of another order or form are no edit of the elements.
        let reordered = sheet_with(&[(DESCRIPTION_ROW, "Feeds"), (TITLE_ROW, "Pump")]);
        assert_eq!(edit_of(&was, &reordered), None);
    }

    #[test]
    fn a_row_deleted_or_in_error_is_read_as_no_text_or_the_empty_text() {
        let changed = |change: &dyn Fn(&mut Row)| {
            let mut sheet = sheet_with(&[(TITLE_ROW, "Pump")]);
            let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
            change(Arc::make_mut(section.rows.get_mut(0).unwrap()));
            sheet
        };
        assert_eq!(own(&changed(&|_| {}), TITLE_ROW), Some("Pump"));
        assert_eq!(own(&changed(&|row| row.del = true), TITLE_ROW), None);
        let in_error = changed(&|row| row.cells.get_mut("Value").unwrap().error = Some(Arc::from("#REF!")));
        assert_eq!(own(&in_error, TITLE_ROW), Some(""));
    }

    #[test]
    fn the_rows_that_the_file_says_are_left_out_of_the_comparison_of_a_section() {
        let mut other = Row { name: Some(Arc::from("Other")), ..Row::default() };
        other.cells.insert("Value".into(), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        let mut mixed = Section::new("User", None);
        mixed.rows.push(Arc::new(standard_row(TITLE_ROW, "Pump")));
        mixed.rows.push(Arc::new(other.clone()));
        let mut only = Section::new("User", None);
        only.rows.push(Arc::new(standard_row(DESCRIPTION_ROW, "Feeds")));
        let mut formula = Section::new("User", None);
        let mut computed = standard_row(TITLE_ROW, "x");
        computed
            .cells
            .insert("Value".into(), diacraft_doc::Cell::with_formula("Prop.Name", Some(diacraft_doc::Value::from_text("x"))).with_unit("STR"));
        formula.rows.push(Arc::new(computed));
        let kept = without_standard_rows(&[Arc::new(mixed), Arc::new(only), Arc::new(formula)]);
        assert_eq!(kept.len(), 2, "a section that holds nothing else is none");
        assert_eq!(kept.first().map(|s| s.rows.len()), Some(1));
        assert_eq!(kept.first().and_then(|s| s.rows.first()).and_then(|r| r.name.as_deref()), Some("Other"));
        assert_eq!(kept.get(1).map(|s| s.rows.len()), Some(1), "a row that is a formula is no row of the file's form");
    }

    #[test]
    fn the_declaration_of_the_svg_prefix_is_found_among_those_made_and_empty_when_it_is_bound() {
        let pfx = |svg: &str, declare: &str| super::super::emit::Pfx {
            draw: "draw".to_owned(),
            svg: svg.to_owned(),
            fo: "fo".to_owned(),
            style: "style".to_owned(),
            text: "text".to_owned(),
            declare: declare.to_owned(),
        };
        let declare = " xmlns:draw1=\"urn:d\" xmlns:svg1=\"urn:svg\" xmlns:fo1=\"urn:fo\"";
        assert_eq!(svg_declaration(&pfx("svg1", declare)), " xmlns:svg1=\"urn:svg\"");
        assert_eq!(svg_declaration(&pfx("svg", declare)), "");
        assert_eq!(svg_declaration(&pfx("svg", "")), "");
    }
}
