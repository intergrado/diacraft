//! How the window's text box shows a shape's paragraphs while its text is edited.
//!
//! The box edits the characters of a shape and sends them when the edit ends (`draw::text_edit`), so the paragraphs it must lay out are those of the text **as it
//! will be once the edit is sent**: [`editor_paragraphs`] takes the edit the person has made so far (the range of the shape's characters replaced, and what replaced
//! it), makes the parts the commit would make (`draw::replaced`: new characters join the run of the first character they replace, or of the one before them), and
//! reads one paragraph for each line feed and one more, as the box splits its characters. **A paragraph has the format of the run that holds its last character**
//! (a line feed ends a paragraph and belongs to it), the empty one after a final line feed the format in force at the end of the text.
//!
//! The cells are read as the renderer reads them (`look.rs`): by inheritance, through the theme, a missing `HorzAlign` centred and every other missing
//! cell 0. Indents are in paper inches. Nothing here changes the document.

use std::collections::HashMap;

use diacraft_doc::{Document, SheetRef, TextPart};

use super::look::{Reader, eval_ctx, row_addr};
use super::text::single;
use super::{Align, Direction};
use crate::draw::{Problem, replaced, shown_parts};

/// The most paragraphs [`editor_paragraphs`] reads: a text of a hundred thousand line feeds is not read paragraph by paragraph for the box. The paragraphs
/// past it are left out of the answer, and the box takes the direction of the ones it has.
pub const MAX_EDITOR_PARAGRAPHS: usize = 4_096;

/// What the text box needs of one paragraph of the text it edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorParagraph {
    /// The reading direction, the cell `Flags` (1 is right to left, and any other number is left to right).
    pub direction: Direction,
    /// The cell `HorzAlign`.
    pub align: Align,
    /// The cells `IndLeft` and `IndRight`, in paper inches: `IndLeft` is the indent of the start of the lines and `IndRight` of their end, whichever way the
    /// paragraph reads.
    pub ind_left: f64,
    pub ind_right: f64,
}

/// The `Paragraph` row each paragraph of `parts` takes: for each line feed the row in force where it stands, and for the last paragraph the row in force at the end.
/// At most [`MAX_EDITOR_PARAGRAPHS`].
fn paragraph_rows(parts: &[TextPart]) -> Vec<u32> {
    let mut rows = Vec::new();
    let mut in_force = 0_u32;
    for part in parts {
        match part {
            TextPart::Pp(ix) => in_force = *ix,
            TextPart::Chars(chars) => {
                for _ in chars.chars().filter(|c| *c == '\n') {
                    if rows.len() < MAX_EDITOR_PARAGRAPHS {
                        rows.push(in_force);
                    }
                }
            }
            _ => {}
        }
    }
    if rows.len() < MAX_EDITOR_PARAGRAPHS {
        rows.push(in_force);
    }
    rows
}

/// The paragraphs of the text of shape `id` on a page as the box lays them out when `edit` has been made to it: the characters `start..end` (counted in
/// characters, fields not counted, as `shape.set_text` counts them) replaced by `text`; `None` for the text as it is. One for each line feed and one more, at most
/// [`MAX_EDITOR_PARAGRAPHS`]. A shape that is not there, a group, and a range the text does not have are refused in words.
pub fn editor_paragraphs(doc: &Document, page: u32, id: u32, edit: Option<(usize, usize, &str)>) -> Result<Vec<EditorParagraph>, Problem> {
    let at = single(doc, page, &[id])?;
    // The text the shape shows, which is the text the box opened on: its own, or its master's when it holds none.
    let own: &[TextPart] = shown_parts(doc, &at);
    let changed;
    let parts: &[TextPart] = match edit {
        Some((start, end, text)) => match replaced(own, start, end, text)? {
            Some(made) => {
                changed = made;
                &changed
            }
            None => own,
        },
        None => own,
    };
    let mut ctx = eval_ctx();
    let reader = Reader::new(doc, &mut ctx);
    let sheet = SheetRef::Shape(at);
    let mut read: HashMap<u32, EditorParagraph> = HashMap::new();
    let paragraphs = paragraph_rows(parts)
        .into_iter()
        .map(|row| {
            *read.entry(row).or_insert_with(|| {
                let cell = |name: &str, default: f64| reader.num(&sheet, &row_addr("Paragraph", row, name), default);
                EditorParagraph {
                    direction: Direction::from_cell(cell("Flags", 0.0)),
                    align: Align::from_cell(cell("HorzAlign", 1.0)),
                    ind_left: cell("IndLeft", 0.0),
                    ind_right: cell("IndRight", 0.0),
                }
            })
        })
        .collect();
    Ok(paragraphs)
}
