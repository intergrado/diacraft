//! The words of a refusal by a cap: what a person reads and what a harness compares. The six caps the sheets raise are written in
//! `diacraft-sheetxml` and converted to this crate's, so each is pinned here with the cap it becomes.

use diacraft_opc::ErrorKind;
use diacraft_sheetxml::{LimitError, LimitKind as Codec};
use diacraft_vsdx::{LimitKind, VsdxError};

#[test]
fn every_cap_has_its_class_word_and_its_words_and_the_message_holds_the_number() {
    let all = [
        (LimitKind::Pages, "too_many_pages", "pages"),
        (LimitKind::Styles, "too_many_styles", "styles"),
        (LimitKind::Masters, "too_many_masters", "masters"),
        (LimitKind::ShapesInAPart, "too_many_shapes_in_a_part", "shapes in one page or master part"),
        (LimitKind::Shapes, "too_many_shapes", "shapes"),
        (LimitKind::ShapeDepth, "shapes_too_deep", "levels of nested shapes"),
        (LimitKind::Cells, "too_many_cells", "cells"),
        (LimitKind::CellsInOnePlace, "too_many_cells_in_one_place", "cells in one sheet, section or row"),
        (LimitKind::SectionsInASheet, "too_many_sections_in_a_sheet", "sections in one sheet"),
        (LimitKind::RowsInASection, "too_many_rows_in_a_section", "rows in one section"),
        (LimitKind::NameBytes, "name_too_long", "bytes in a cell, section or row name"),
        (LimitKind::Items, "too_many_items", "XML items kept in the document"),
        (LimitKind::XmlBytes, "too_much_xml", "bytes of XML in the parts of the document"),
        (LimitKind::Colors, "too_many_colors", "entries in the colour table"),
    ];
    for (kind, class, what) in all {
        assert_eq!((kind.class(), kind.what()), (class, what));
        let e = VsdxError::Limit { kind, max: 7 };
        assert_eq!((e.class(), e.kind()), (class, ErrorKind::TooBig));
        assert_eq!(e.to_string(), format!("the file holds more than 7 {what}"));
    }
}

#[test]
fn the_memory_a_read_may_hold_is_a_cap_with_its_class_word_and_words_of_its_own() {
    // It is not a count of what the file holds, so it does not say "the file holds more than": it says what reading it would take.
    let e = VsdxError::Limit { kind: LimitKind::Memory, max: 2_600_000_000 };
    assert_eq!((LimitKind::Memory.class(), LimitKind::Memory.what()), ("too_much_memory", "bytes of memory to read the file"));
    assert_eq!((e.class(), e.kind()), ("too_much_memory", ErrorKind::TooBig));
    assert_eq!(e.to_string(), "reading the file would take more than 2600000000 bytes of memory: it is too large to read here");
}

#[test]
fn a_cap_of_the_sheet_codec_is_the_same_cap_here_with_the_same_words() {
    let converted = [
        (Codec::Cells, LimitKind::Cells),
        (Codec::CellsInOnePlace, LimitKind::CellsInOnePlace),
        (Codec::SectionsInASheet, LimitKind::SectionsInASheet),
        (Codec::RowsInASection, LimitKind::RowsInASection),
        (Codec::NameBytes, LimitKind::NameBytes),
        (Codec::Items, LimitKind::Items),
        (Codec::Bytes, LimitKind::Memory),
    ];
    for (codec, kind) in converted {
        assert_eq!(VsdxError::from(LimitError { kind: codec, max: 9 }), VsdxError::Limit { kind, max: 9 });
        assert_eq!((codec.class(), codec.what()), (kind.class(), kind.what()));
    }
}
