//! The vocabulary: every element and attribute the codec writes, in one table, with the default the decoder assumes when one is absent.
//!
//! **Frozen from the switch**: after it the names and defaults here do not change; later work may only add names that are
//! optional. The public spec, `docs/diacraft-format.md`, copies [`ELEMENTS`], and tests that the two agree.
//!
//! **How to read it.** Every element is in `urn:app.diacraft:extension`; the names below are local names. An attribute with a
//! `default` is not written when the field holds it and is read as it when absent; one with none is optional, and absent means the field is
//! `None`; one that is **required** (`AttrDef::required`) has no default, is always written, and an element without it is refused (the table
//! said otherwise of twelve of them, and a test now holds the decoder to it for every attribute). These hold for every text attribute `x`:
//!
//! * `x-b64` holds the same text in base64 (standard alphabet, padded) when the text has a character XML 1.0 forbids;
//! * a child element `s` with `n="x"` holds the text as its own text (with `b64="1"` for base64) when it is longer than
//!   [`super::escape::ATTR_MAX_BYTES`] bytes.
//!
//! The same attributes name a field of an element in both places: a decoder reads `x`, `x-b64` or `s n="x"`, whichever is there. Within a part of
//! this table (the document part, a page part, a master part) the local names are unique, so that an element's meaning never depends on where it stands, but for
//! `page-ref` and `master-ref`, which only the document part holds. **Unique within each kind of part, not over every part a package has**: the package's own
//! manifest part, which this table does not hold, uses the names `manifest` and `part` for elements of its own.

/// What an attribute holds, as the table says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text: any string, escaped as the module says.
    Text,
    /// An unsigned 32-bit number in decimal.
    U32,
    /// An unsigned 64-bit number in decimal.
    U64,
    /// A flag: `1` for true, and absent for false (`0` and the words `true` and `false` are read, and never written).
    Flag,
    /// A blob's number.
    Blob,
    /// A stamp: 16 lowercase hex digits.
    Stamp,
}

/// An attribute of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrDef {
    pub name: &'static str,
    pub kind: Kind,
    /// The value the decoder assumes when the attribute is absent, and which the encoder then does not write. `None`: optional, and absent means
    /// the field is `None`.
    pub default: Option<&'static str>,
    /// Whether the decoder refuses an element that has none: the attribute is required, and has no default (`default` is `None`, and absent does not mean
    /// that the field is `None`). The encoder always writes it. A table that said otherwise would be the public spec's misstatement.
    pub required: bool,
    pub about: &'static str,
}

/// An element and what it may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElDef {
    pub name: &'static str,
    /// The elements it stands in, by local name (`part root` for the root of a part).
    pub inside: &'static [&'static str],
    pub attrs: &'static [AttrDef],
    /// What it holds besides its attributes: its children and text, in words.
    pub holds: &'static str,
}

/// The local names of the elements.
pub mod el {
    pub const DOCUMENT: &str = "document";
    pub const PAGE: &str = "page";
    pub const MASTER: &str = "master";
    pub const PAGE_REF: &str = "page-ref";
    pub const MASTER_REF: &str = "master-ref";
    pub const BLOB: &str = "blob";
    pub const PROPS: &str = "props";
    pub const COLOR: &str = "color";
    pub const FONT: &str = "font";
    pub const STYLE: &str = "style";
    pub const THEME: &str = "theme";
    pub const SOLUTION: &str = "solution";
    pub const SHEET: &str = "sheet";
    pub const SAME: &str = "same";
    pub const CELL: &str = "Cell";
    pub const SECTION: &str = "Section";
    pub const ROW: &str = "Row";
    pub const EXTRA: &str = "extra";
    pub const SHAPE: &str = "shape";
    pub const TEXT: &str = "text";
    pub const CHARS: &str = "t";
    pub const CP: &str = "cp";
    pub const PP: &str = "pp";
    pub const TP: &str = "tp";
    pub const FLD: &str = "fld";
    pub const FOREIGN: &str = "foreign";
    pub const PICTURE: &str = "picture";
    pub const CONNECT: &str = "connect";
    pub const ATTR: &str = "attr";
    pub const KEPT: &str = "kept";
    pub const KEPT_EL: &str = "el";
    pub const KEPT_TEXT: &str = "txt";
    pub const KEPT_COMMENT: &str = "comment";
    pub const KEPT_PI: &str = "pi";
    pub const LONG: &str = "s";
    pub const EXT: &str = "ext";
    pub const ODF: &str = "odf";
    pub const VSDX: &str = "vsdx";
    pub const REST: &str = "rest";
    pub const PART: &str = "part";
    pub const REL: &str = "rel";
    pub const MANIFEST: &str = "manifest";
    pub const VSDX_HELD: &str = "vsdx-held";
    pub const CONTENT_TYPE: &str = "content-type";
    pub const PROPS_REST: &str = "props-rest";
    pub const WINDOWS: &str = "windows";
    pub const CUSTOM: &str = "custom";
    pub const COMMENT_LIST: &str = "comment-list";
    pub const COMMENT_AUTHOR: &str = "comment-author";
    pub const COMMENT_ENTRY: &str = "comment-entry";
    pub const ODF_ROOT: &str = "root";
    pub const MANIFEST_ROW: &str = "manifest-row";
    pub const META: &str = "meta";
    pub const SETTINGS: &str = "settings";
    pub const DEFINITION: &str = "definition";
    pub const LAYER: &str = "layer";
    pub const KEPT_STYLE: &str = "kept-style";
    pub const MASTER_PAGE_ATTRS: &str = "master-page-attrs";
    pub const LAYOUT_PROPS: &str = "layout-props";
    pub const DRAWING_PAGE_STYLE: &str = "drawing-page-style";
    pub const UNITS: &str = "units";
    pub const UNMAPPED: &str = "unmapped";
    pub const HELD: &str = "held";
    pub const ODF_RAW: &str = "odf-raw";
    pub const PARAGRAPH: &str = "paragraph";
    pub const RUN: &str = "run";
    pub const TEXT_STYLE: &str = "text-style";
    pub const HELD_STYLE: &str = "held-style";
    pub const ELEMENT: &str = "element";
    pub const LEVEL: &str = "level";
    pub const LIST: &str = "list";
    pub const ITEM: &str = "item";
}

/// The names of the attributes.
pub mod at {
    pub const VERSION: &str = "version";
    pub const TITLE: &str = "title";
    pub const ID: &str = "id";
    pub const NAME: &str = "name";
    pub const NAME_U: &str = "name-u";
    pub const PART: &str = "part";
    pub const BLOB: &str = "blob";
    pub const PATH: &str = "path";
    pub const MEDIA_TYPE: &str = "media-type";
    pub const SIZE: &str = "size";
    pub const STAMP: &str = "stamp";
    pub const IX: &str = "ix";
    pub const RGB: &str = "rgb";
    pub const BACKGROUND: &str = "background";
    pub const BACK_PAGE: &str = "back-page";
    pub const REL_ID: &str = "rel-id";
    pub const UNIQUE_ID: &str = "unique-id";
    pub const PATTERN_FLAGS: &str = "pattern-flags";
    pub const ICON: &str = "icon";
    pub const TYPE: &str = "type";
    pub const TYPE_OTHER: &str = "type-other";
    pub const MASTER: &str = "master";
    pub const MASTER_SHAPE: &str = "master-shape";
    pub const DEL: &str = "del";
    pub const DATA1: &str = "data1";
    pub const DATA2: &str = "data2";
    pub const DATA3: &str = "data3";
    pub const TEXT: &str = "text";
    pub const FROM_SHEET: &str = "from-sheet";
    pub const FROM_CELL: &str = "from-cell";
    pub const FROM_PART: &str = "from-part";
    pub const TO_SHEET: &str = "to-sheet";
    pub const TO_CELL: &str = "to-cell";
    pub const TO_PART: &str = "to-part";
    pub const DEF: &str = "def";
    pub const FP: &str = "fp";
    pub const OF: &str = "of";
    pub const LINE_STYLE: &str = "LineStyle";
    pub const FILL_STYLE: &str = "FillStyle";
    pub const TEXT_STYLE: &str = "TextStyle";
    pub const N: &str = "N";
    pub const V: &str = "V";
    pub const F: &str = "F";
    pub const FL: &str = "FL";
    pub const U: &str = "U";
    pub const E: &str = "E";
    pub const SECTION_IX: &str = "IX";
    pub const SECTION_DEL: &str = "Del";
    pub const ROW_T: &str = "T";
    pub const LOCAL_NAME: &str = "LocalName";
    pub const VALUE: &str = "value";
    pub const AFTER: &str = "after";
    pub const TARGET: &str = "target";
    pub const B64: &str = "b64";
    pub const N_LONG: &str = "n";
    pub const SUBJECT: &str = "subject";
    pub const CREATOR: &str = "creator";
    pub const LAST_MODIFIED_BY: &str = "last-modified-by";
    pub const KEYWORDS: &str = "keywords";
    pub const DESCRIPTION: &str = "description";
    pub const CATEGORY: &str = "category";
    pub const COMPANY: &str = "company";
    pub const MANAGER: &str = "manager";
    pub const HYPERLINK_BASE: &str = "hyperlink-base";
    pub const APP_VERSION: &str = "app-version";
    pub const CREATED: &str = "created";
    pub const MODIFIED: &str = "modified";
    pub const LAST_PRINTED: &str = "last-printed";
    pub const LANGUAGE: &str = "language";
    pub const KEY: &str = "key";
    pub const FORMAT: &str = "format";
    pub const ROLE: &str = "role";
    pub const SOURCE: &str = "source";
    pub const KIND: &str = "kind";
    pub const EXTERNAL: &str = "external";
    pub const EXTENSION: &str = "extension";
    pub const CONTENT_TYPE: &str = "content-type";
    pub const VALUE_TYPE: &str = "value-type";
    pub const FAMILY: &str = "family";
    pub const GROUP: &str = "group";
    pub const MIMETYPE: &str = "mimetype";
    pub const MASTER_PAGE: &str = "master-page";
    pub const LAYOUT: &str = "layout";
    pub const DISPLAY_NAME: &str = "display-name";
    pub const PARENT: &str = "parent";
    pub const STYLE_NAME: &str = "style-name";
    pub const PART_NAME: &str = "part-name";
    pub const CHARACTERS: &str = "characters";
    pub const FROM: &str = "from";
    pub const TO: &str = "to";
    pub const SHOW_TAGS: &str = "show-tags";
    pub const INITIALS: &str = "initials";
    pub const PAGE_ID: &str = "page";
    pub const SHAPE_ID: &str = "shape";
    pub const AUTHOR: &str = "author";
    pub const DATE: &str = "date";
    pub const EDIT_DATE: &str = "edit-date";
    pub const DONE: &str = "done";
    /// The suffix of a text attribute whose text is base64.
    pub const B64_SUFFIX: &str = "-b64";
}

const fn text(name: &'static str, default: Option<&'static str>, about: &'static str) -> AttrDef {
    AttrDef { name, kind: Kind::Text, default, required: false, about }
}
const fn u32_(name: &'static str, default: Option<&'static str>, about: &'static str) -> AttrDef {
    AttrDef { name, kind: Kind::U32, default, required: false, about }
}
const fn flag(name: &'static str, about: &'static str) -> AttrDef {
    AttrDef { name, kind: Kind::Flag, default: Some("0"), required: false, about }
}
/// A text attribute the decoder refuses an element without.
const fn required_text(name: &'static str, about: &'static str) -> AttrDef {
    AttrDef { name, kind: Kind::Text, default: None, required: true, about }
}
/// An attribute of another kind that the decoder refuses an element without.
const fn required(name: &'static str, kind: Kind, about: &'static str) -> AttrDef {
    AttrDef { name, kind, default: None, required: true, about }
}

const ROOT: &[&str] = &["part root"];

/// Every element of the codec, with its attributes and what it holds.
pub const ELEMENTS: &[ElDef] = &[
    ElDef {
        name: el::DOCUMENT,
        inside: ROOT,
        attrs: &[u32_(at::VERSION, Some("1"), "the model version"), text(at::TITLE, Some(""), "the document's title")],
        holds: "props, color, font, sheet, style, theme, solution, comment-list, kept, ext, part, vsdx-held, page-ref, master-ref and blob, in this order",
    },
    ElDef {
        name: el::PAGE_REF,
        inside: &[el::DOCUMENT],
        attrs: &[u32_(at::ID, Some("0"), "the page's ID"), text(at::PART, Some(""), "the name of the page's part")],
        holds: "nothing",
    },
    ElDef {
        name: el::MASTER_REF,
        inside: &[el::DOCUMENT],
        attrs: &[u32_(at::ID, Some("0"), "the master's ID"), text(at::PART, Some(""), "the name of the master's part")],
        holds: "nothing",
    },
    ElDef {
        name: el::BLOB,
        inside: &[el::DOCUMENT],
        attrs: &[
            AttrDef { name: at::ID, kind: Kind::Blob, default: Some("0"), required: false, about: "the blob's number" },
            text(at::PATH, Some(""), "the name of the blob's entry"),
            text(at::MEDIA_TYPE, Some(""), "the blob's media type"),
            AttrDef { name: at::SIZE, kind: Kind::U64, default: Some("0"), required: false, about: "its size in bytes" },
            required(at::STAMP, Kind::Stamp, "the stamp of its bytes"),
        ],
        holds: "nothing",
    },
    ElDef {
        name: el::PROPS,
        inside: &[el::DOCUMENT],
        attrs: &[
            text(at::SUBJECT, None, "subject"),
            text(at::CREATOR, None, "creator"),
            text(at::LAST_MODIFIED_BY, None, "last modified by"),
            text(at::KEYWORDS, None, "keywords"),
            text(at::DESCRIPTION, None, "description"),
            text(at::CATEGORY, None, "category"),
            text(at::COMPANY, None, "company"),
            text(at::MANAGER, None, "manager"),
            text(at::HYPERLINK_BASE, None, "hyperlink base"),
            text(at::APP_VERSION, None, "application version"),
            text(at::CREATED, None, "creation time, as text"),
            text(at::MODIFIED, None, "modification time, as text"),
            text(at::LAST_PRINTED, None, "last printed time, as text"),
            text(at::LANGUAGE, None, "language"),
        ],
        holds: "custom; absent means all fields are none and no custom property",
    },
    ElDef {
        name: el::COLOR,
        inside: &[el::DOCUMENT],
        attrs: &[u32_(at::IX, Some("0"), "the color index"), text(at::RGB, Some(""), "the color as #RRGGBB, verbatim")],
        holds: "nothing",
    },
    ElDef {
        name: el::FONT,
        inside: &[el::DOCUMENT],
        attrs: &[text(at::NAME_U, Some(""), "the face's universal name")],
        holds: "ext and attr, for the face's other attributes",
    },
    ElDef {
        name: el::STYLE,
        inside: &[el::DOCUMENT],
        attrs: &[u32_(at::ID, Some("0"), "the style's ID"), text(at::NAME, None, "its name"), text(at::NAME_U, None, "its universal name")],
        holds: "ext, sheet, attr and kept",
    },
    ElDef {
        name: el::THEME,
        inside: &[el::DOCUMENT],
        attrs: &[
            text(at::PART, Some(""), "the theme part's name in the package it came from"),
            required(at::BLOB, Kind::Blob, "the theme part's bytes"),
        ],
        holds: "nothing",
    },
    ElDef { name: el::SOLUTION, inside: &[el::DOCUMENT], attrs: &[], holds: "one el: the application's own data element, kept whole" },
    ElDef {
        name: el::PAGE,
        inside: ROOT,
        attrs: &[
            u32_(at::ID, Some("0"), "the page's ID"),
            text(at::NAME, Some(""), "its name"),
            text(at::NAME_U, None, "its universal name"),
            flag(at::BACKGROUND, "the page is a background page"),
            u32_(at::BACK_PAGE, None, "the ID of its background page"),
            text(at::REL_ID, None, "the relationship ID of the page's part"),
            text(at::PART_NAME, None, "the name of the page's part in the package it came from"),
        ],
        holds: "ext, sheet, shape, connect, attr and kept",
    },
    ElDef {
        name: el::MASTER,
        inside: ROOT,
        attrs: &[
            u32_(at::ID, Some("0"), "the master's ID"),
            text(at::NAME, None, "its name"),
            text(at::NAME_U, None, "its universal name"),
            text(at::UNIQUE_ID, None, "its GUID"),
            u32_(at::PATTERN_FLAGS, None, "its pattern flags"),
            text(at::REL_ID, None, "the relationship ID of the master's part"),
            text(at::ICON, None, "its icon, as base64 text"),
            text(at::PART_NAME, None, "the name of the master's part in the package it came from"),
        ],
        holds: "ext, sheet, shape, connect, attr and kept",
    },
    ElDef {
        name: el::SHAPE,
        inside: &[el::PAGE, el::MASTER, el::SHAPE],
        attrs: &[
            u32_(at::ID, Some("0"), "the shape's ID"),
            text(at::NAME, None, "its name"),
            text(at::NAME_U, None, "its universal name"),
            text(at::TYPE, None, "Shape, Group, Foreign or Guide; absent is a shape with no type"),
            text(at::TYPE_OTHER, None, "a type the model has no word for, as it was"),
            u32_(at::MASTER, None, "the ID of the master it is an instance of"),
            u32_(at::MASTER_SHAPE, None, "the ID of the master's shape it stands for"),
            flag(at::DEL, "deleted locally"),
            text(at::DATA1, None, "Data1"),
            text(at::DATA2, None, "Data2"),
            text(at::DATA3, None, "Data3"),
        ],
        holds: "ext, sheet, text, foreign, attr, kept, and the shapes of a group directly, in order",
    },
    ElDef {
        name: el::TEXT,
        inside: &[el::SHAPE],
        attrs: &[],
        holds: "the pieces of the text in order: bare text nodes, t, cp, pp, tp, fld and kept; and ext (the text's slot); a txt, comment or pi directly in it is a newer build's text, comment or instruction",
    },
    ElDef { name: el::CHARS, inside: &[el::TEXT], attrs: &[flag(at::B64, "the text is base64")], holds: "characters" },
    ElDef { name: el::CP, inside: &[el::TEXT], attrs: &[u32_(at::IX, Some("0"), "the Character row")], holds: "nothing" },
    ElDef { name: el::PP, inside: &[el::TEXT], attrs: &[u32_(at::IX, Some("0"), "the Paragraph row")], holds: "nothing" },
    ElDef { name: el::TP, inside: &[el::TEXT], attrs: &[u32_(at::IX, Some("0"), "the Tabs row")], holds: "nothing" },
    ElDef {
        name: el::FLD,
        inside: &[el::TEXT],
        attrs: &[u32_(at::IX, Some("0"), "the Field row"), flag(at::B64, "the text is base64")],
        holds: "the field's displayed text",
    },
    ElDef {
        name: el::FOREIGN,
        inside: &[el::SHAPE],
        attrs: &[text(at::REL_ID, None, "the relationship ID"), text(at::TEXT, None, "text inside the element")],
        holds: "ext, attr (the element's attributes), picture and kept",
    },
    ElDef {
        name: el::PICTURE,
        inside: &[el::FOREIGN],
        attrs: &[
            text(at::PART, Some(""), "the picture part's name"),
            text(at::TYPE, Some(""), "its content type"),
            required(at::BLOB, Kind::Blob, "its bytes"),
        ],
        holds: "nothing",
    },
    ElDef {
        name: el::CONNECT,
        inside: &[el::PAGE, el::MASTER],
        attrs: &[
            u32_(at::FROM_SHEET, Some("0"), "the shape the connection starts at"),
            text(at::FROM_CELL, None, "its cell"),
            u32_(at::FROM_PART, None, "its part"),
            u32_(at::TO_SHEET, Some("0"), "the shape it ends at"),
            text(at::TO_CELL, None, "its cell"),
            u32_(at::TO_PART, None, "its part"),
        ],
        holds: "ext and attr",
    },
    ElDef {
        name: el::SHEET,
        inside: &[el::DOCUMENT, el::STYLE, el::PAGE, el::MASTER, el::SHAPE],
        attrs: &[
            u32_(at::LINE_STYLE, None, "the line style link"),
            u32_(at::FILL_STYLE, None, "the fill style link"),
            u32_(at::TEXT_STYLE, None, "the text style link"),
            text(at::DEF, None, "the name by which later `same` elements of the part stand for this sheet"),
            AttrDef {
                name: at::FP,
                kind: Kind::Stamp,
                default: None,
                required: false,
                about: "the fingerprint of the sheet as 1.0 would say it, written with a `def`",
            },
        ],
        holds: "ext, Cell, Section, same (for a section), attr and kept; absent means an empty sheet",
    },
    ElDef {
        name: el::SAME,
        inside: &[el::DOCUMENT, el::STYLE, el::PAGE, el::MASTER, el::SHAPE, el::SHEET],
        attrs: &[required_text(at::OF, "the `def` of the sheet or section, earlier in the part, that this stands for")],
        holds: "nothing: a copy of the sheet (where a sheet may stand) or the section (in a sheet) that its `of` names; a reader that does not know it keeps it as unknown content",
    },
    ElDef {
        name: el::CELL,
        inside: &[el::SHEET, el::SECTION, el::ROW],
        attrs: &[
            required_text(at::N, "the cell's name"),
            text(at::V, None, "its value, as text"),
            text(at::F, None, "its formula; Inh and No Formula are the markers"),
            text(at::FL, None, "a formula whose text is a marker's"),
            text(at::U, None, "its unit"),
            text(at::E, None, "its error"),
        ],
        holds: "attr and extra",
    },
    ElDef {
        name: el::EXTRA,
        inside: &[el::CELL],
        attrs: &[text(at::TEXT, None, "text inside the cell")],
        holds: "ext and kept; present even when empty, since an empty extra is not an absent one",
    },
    ElDef {
        name: el::SECTION,
        inside: &[el::SHEET],
        attrs: &[
            required_text(at::N, "the section's name"),
            u32_(at::SECTION_IX, None, "its IX"),
            flag(at::SECTION_DEL, "deleted locally"),
            text(at::DEF, None, "the name by which later `same` elements of the part stand for this section"),
            AttrDef {
                name: at::FP,
                kind: Kind::Stamp,
                default: None,
                required: false,
                about: "the fingerprint of the section as 1.0 would say it, written with a `def`",
            },
        ],
        holds: "ext, Cell, Row, attr and kept",
    },
    ElDef {
        name: el::ROW,
        inside: &[el::SECTION],
        attrs: &[
            text(at::N, None, "the row's name"),
            u32_(at::SECTION_IX, None, "its IX"),
            text(at::LOCAL_NAME, None, "its local name"),
            text(at::ROW_T, None, "its type"),
            flag(at::SECTION_DEL, "deleted locally"),
        ],
        holds: "ext, Cell, attr and kept",
    },
    ElDef {
        name: el::ATTR,
        inside: &[
            el::FONT,
            el::STYLE,
            el::PAGE,
            el::MASTER,
            el::SHAPE,
            el::FOREIGN,
            el::CONNECT,
            el::SHEET,
            el::CELL,
            el::SECTION,
            el::ROW,
            el::KEPT_EL,
            el::ODF,
            el::REST,
            el::MANIFEST,
            el::CUSTOM,
            el::COMMENT_LIST,
            el::COMMENT_AUTHOR,
            el::COMMENT_ENTRY,
            el::ODF_ROOT,
            el::MANIFEST_ROW,
            el::MASTER_PAGE_ATTRS,
            el::LAYOUT_PROPS,
            el::UNITS,
            el::UNMAPPED,
            el::HELD,
            el::HELD_STYLE,
        ],
        attrs: &[text(at::NAME, Some(""), "the attribute's name, as written"), text(at::VALUE, Some(""), "its value")],
        holds: "nothing",
    },
    ElDef {
        name: el::KEPT,
        inside: &[
            el::DOCUMENT,
            el::STYLE,
            el::PAGE,
            el::MASTER,
            el::SHAPE,
            el::FOREIGN,
            el::SHEET,
            el::SECTION,
            el::ROW,
            el::EXTRA,
            el::TEXT,
            el::ODF,
            el::REST,
            el::META,
            el::HELD,
            el::HELD_STYLE,
        ],
        attrs: &[u32_(at::AFTER, Some("0"), "how many of the typed children of the parent came before it")],
        holds: "one of el, txt, comment and pi",
    },
    ElDef {
        name: el::KEPT_EL,
        inside: &[
            el::KEPT,
            el::SOLUTION,
            el::KEPT_EL,
            el::WINDOWS,
            el::SETTINGS,
            el::DEFINITION,
            el::LAYER,
            el::KEPT_STYLE,
            el::DRAWING_PAGE_STYLE,
            el::ODF_RAW,
            el::ELEMENT,
            el::LIST,
            el::ITEM,
        ],
        attrs: &[text(at::NAME, Some(""), "the element's name, prefix included")],
        holds: "attr for its attributes (xmlns declarations among them), then el, txt, comment and pi for its children",
    },
    ElDef {
        name: el::KEPT_TEXT,
        inside: &[el::KEPT, el::KEPT_EL, el::ODF_RAW, el::TEXT],
        attrs: &[flag(at::B64, "the text is base64")],
        holds: "characters",
    },
    ElDef {
        name: el::KEPT_COMMENT,
        inside: &[el::KEPT, el::KEPT_EL, el::ODF_RAW, el::TEXT],
        attrs: &[flag(at::B64, "the text is base64")],
        holds: "the comment's characters",
    },
    ElDef {
        name: el::KEPT_PI,
        inside: &[el::KEPT, el::KEPT_EL, el::ODF_RAW, el::TEXT],
        attrs: &[text(at::TARGET, Some(""), "the instruction's target"), flag(at::B64, "the data is base64")],
        holds: "the instruction's data",
    },
    ElDef {
        name: el::EXT,
        inside: &[
            el::DOCUMENT,
            el::PAGE,
            el::MASTER,
            el::STYLE,
            el::SHAPE,
            el::ROW,
            el::SHEET,
            el::SECTION,
            el::EXTRA,
            el::FOREIGN,
            el::CONNECT,
            el::FONT,
            el::TEXT,
        ],
        attrs: &[],
        holds: "odf and vsdx: what the object holds of each format; the unknown content of a newer build is not here but in the object's own element (a text's own attributes are on the text, and its unknown elements are in this one, since a text's own children are its parts)",
    },
    ElDef {
        name: el::ODF,
        inside: &[el::EXT],
        attrs: &[
            text(at::MIMETYPE, None, "a document's: the mimetype entry"),
            text(at::MASTER_PAGE, None, "a page's: its master page's name"),
            text(at::LAYOUT, None, "a page's: its layout's name"),
            AttrDef { name: at::STAMP, kind: Kind::Stamp, default: None, required: false, about: "a page's: the stamp of its cells" },
            text(at::NAME, None, "a style's: its name"),
            text(at::DISPLAY_NAME, None, "a style's: its display name"),
            text(at::PARENT, None, "a style's: its parent"),
            text(at::FAMILY, None, "a style's: its family"),
            text(at::STYLE_NAME, None, "a shape's or a row's: its automatic style name"),
        ],
        holds: "attr and kept (what the format kept); by owner: root, manifest-row, meta, settings, definition, layer, kept-style (a document), master-page-attrs, layout-props, drawing-page-style, units (a page), unmapped, units, held and held-style (a style), held and units (a shape), held (a row), paragraph (a text)",
    },
    ElDef {
        name: el::PARAGRAPH,
        inside: &[el::ODF],
        attrs: &[AttrDef {
            name: at::CHARACTERS,
            kind: Kind::Stamp,
            default: None,
            required: false,
            about: "the stamp of the paragraph's characters when read; absent for none",
        }],
        holds: "text-style (the style the paragraph names), element (its own element, when it is not a plain text:p), level (one for each list round it, outermost first) and run, in order",
    },
    ElDef {
        name: el::RUN,
        inside: &[el::PARAGRAPH],
        attrs: &[
            u32_(at::FROM, Some("0"), "where the run starts, in characters of its paragraph"),
            u32_(at::TO, Some("0"), "where it ends, in characters of its paragraph"),
        ],
        holds: "text-style (the style the run names) and element (the element it is made of, when it is not a plain text:span)",
    },
    ElDef {
        name: el::TEXT_STYLE,
        inside: &[el::PARAGRAPH, el::RUN],
        attrs: &[
            text(at::NAME, None, "the style name a paragraph or run names; absent: it names none"),
            AttrDef {
                name: at::STAMP,
                kind: Kind::Stamp,
                default: None,
                required: false,
                about: "the stamp of the row's cells when read; absent for none",
            },
        ],
        holds: "held-style, one for each held value; the element is absent for a style that names nothing, has no stamp and holds nothing",
    },
    ElDef {
        name: el::HELD_STYLE,
        inside: &[el::TEXT_STYLE, el::ODF],
        attrs: &[
            required_text(
                at::GROUP,
                "the group of the row's cells the value rests on: Size, Style, Underline, Strike, Overline, Case, Pos, Color, Letterspace, FontScale, Font, AsianFont, ComplexScriptFont, HorzAlign, Flags, IndLeft, IndRight, IndFirst, SpBefore, SpAfter or SpLine",
            ),
            AttrDef {
                name: at::STAMP,
                kind: Kind::Stamp,
                default: None,
                required: false,
                about: "the stamp of those cells when read; absent for a value that is not stamped",
            },
        ],
        holds: "attr and kept",
    },
    ElDef { name: el::VSDX, inside: &[el::EXT], attrs: &[], holds: "rest, one for each element the object had no slot for" },
    ElDef {
        name: el::REST,
        inside: &[el::VSDX, el::PROPS_REST],
        attrs: &[text(at::KEY, Some(""), "the element's local name, and /IX for a colour entry")],
        holds: "attr and kept",
    },
    ElDef {
        name: el::PART,
        inside: &[el::DOCUMENT],
        attrs: &[
            required_text(at::FORMAT, "vsdx, odf or native: whose rules the part belongs to"),
            text(at::NAME, Some(""), "the part's name in its package"),
            text(at::MEDIA_TYPE, Some(""), "its media type"),
            required_text(
                at::ROLE,
                "what it is for: thumbnail, macro-project, macro-signature, macro-data, embedding, object-file, object-replacement, signature, custom-xml, font or other",
            ),
            required(at::BLOB, Kind::Blob, "its bytes"),
        ],
        holds: "manifest and rel",
    },
    ElDef { name: el::MANIFEST, inside: &[el::PART], attrs: &[], holds: "attr: the other attributes of the manifest row that named the part" },
    ElDef {
        name: el::REL,
        inside: &[el::PART, el::VSDX_HELD],
        attrs: &[
            text(at::SOURCE, Some(""), "the part the relationship comes from"),
            text(at::ID, Some(""), "its ID"),
            text(at::KIND, Some(""), "its type"),
            text(at::TARGET, Some(""), "its target"),
            flag(at::EXTERNAL, "the target is outside the package"),
        ],
        holds: "nothing",
    },
    ElDef { name: el::VSDX_HELD, inside: &[el::DOCUMENT], attrs: &[], holds: "rel, content-type, props-rest and windows" },
    ElDef {
        name: el::CONTENT_TYPE,
        inside: &[el::VSDX_HELD],
        attrs: &[
            text(at::EXTENSION, None, "a default's extension"),
            text(at::PART, None, "an override's part"),
            text(at::CONTENT_TYPE, Some(""), "the content type"),
        ],
        holds: "nothing; exactly one of extension (a default) and part (an override) is given, and a row with neither or both is refused",
    },
    ElDef { name: el::PROPS_REST, inside: &[el::VSDX_HELD], attrs: &[], holds: "rest" },
    ElDef { name: el::WINDOWS, inside: &[el::VSDX_HELD], attrs: &[], holds: "one el" },
    ElDef {
        name: el::CUSTOM,
        inside: &[el::PROPS],
        attrs: &[
            required_text(at::FORMAT, "vsdx or odf: the format it came from"),
            text(at::NAME, Some(""), "its name"),
            text(at::VALUE_TYPE, Some(""), "its type"),
            text(at::VALUE, Some(""), "its value"),
        ],
        holds: "attr",
    },
    ElDef {
        name: el::COMMENT_LIST,
        inside: &[el::DOCUMENT],
        attrs: &[
            text(at::SHOW_TAGS, None, "1 or 0: whether the markers of the comments show on the page; absent, they show"),
            text(
                at::PART,
                None,
                "the name of the package part the comments were read from, as the source package gave it; the only part a save into that package rewrites",
            ),
        ],
        holds: "attr, then comment-author and comment-entry, in order; absent when there is no comment, no author and no setting",
    },
    ElDef {
        name: el::COMMENT_AUTHOR,
        inside: &[el::COMMENT_LIST],
        attrs: &[
            required(at::ID, Kind::U32, "the author's number, unique among the authors"),
            text(at::NAME, Some(""), "the author's name"),
            text(at::INITIALS, Some(""), "the author's initials"),
        ],
        holds: "attr",
    },
    ElDef {
        name: el::COMMENT_ENTRY,
        inside: &[el::COMMENT_LIST],
        attrs: &[
            required(at::ID, Kind::U32, "the comment's number, unique among the comments"),
            u32_(
                at::PARENT,
                None,
                "the number of the comment this one answers, which has no parent itself; absent for the first comment of a thread",
            ),
            u32_(at::PAGE_ID, Some("0"), "the ID of the page the comment is on"),
            u32_(at::SHAPE_ID, None, "the ID of the shape the comment is on; absent for a comment on the page"),
            u32_(at::AUTHOR, Some("0"), "the number of the comment's author"),
            text(at::TEXT, Some(""), "the comment's text, its lines parted by a line feed"),
            text(at::DATE, Some(""), "when it was written, as the source format wrote it"),
            text(at::EDIT_DATE, None, "when its text last changed, as text"),
            flag(at::DONE, "the comment is marked as done"),
        ],
        holds: "attr",
    },
    ElDef { name: el::ODF_ROOT, inside: &[el::ODF], attrs: &[text(at::NAME, Some(""), "the root element's name")], holds: "attr" },
    ElDef { name: el::MANIFEST_ROW, inside: &[el::ODF], attrs: &[text(at::NAME, Some(""), "the entry the row names")], holds: "attr" },
    ElDef { name: el::META, inside: &[el::ODF], attrs: &[], holds: "kept" },
    ElDef { name: el::SETTINGS, inside: &[el::ODF], attrs: &[], holds: "one el" },
    ElDef {
        name: el::DEFINITION,
        inside: &[el::ODF],
        attrs: &[
            required_text(at::FAMILY, "marker, gradient, opacity, hatch, stroke-dash or fill-image"),
            text(at::NAME, Some(""), "its name"),
            AttrDef { name: at::BLOB, kind: Kind::Blob, default: None, required: false, about: "a fill image's bytes" },
        ],
        holds: "one el",
    },
    ElDef { name: el::LAYER, inside: &[el::ODF], attrs: &[], holds: "one el" },
    ElDef {
        name: el::KEPT_STYLE,
        inside: &[el::ODF],
        attrs: &[text(at::FAMILY, Some(""), "its family"), text(at::NAME, Some(""), "its name")],
        holds: "one el",
    },
    ElDef { name: el::MASTER_PAGE_ATTRS, inside: &[el::ODF], attrs: &[], holds: "attr" },
    ElDef { name: el::LAYOUT_PROPS, inside: &[el::ODF], attrs: &[], holds: "attr" },
    ElDef { name: el::DRAWING_PAGE_STYLE, inside: &[el::ODF], attrs: &[], holds: "one el" },
    ElDef { name: el::UNITS, inside: &[el::ODF], attrs: &[], holds: "attr" },
    ElDef { name: el::UNMAPPED, inside: &[el::ODF], attrs: &[], holds: "attr" },
    ElDef {
        name: el::HELD,
        inside: &[el::ODF],
        attrs: &[
            required_text(at::GROUP, "fill, line, line-ends, shadow, text, frame, glue or geometry"),
            AttrDef {
                name: at::STAMP,
                kind: Kind::Stamp,
                default: None,
                required: false,
                about: "the stamp of the cells it stands beside; absent for a value that is not stamped",
            },
        ],
        holds: "attr and kept",
    },
    ElDef { name: el::ODF_RAW, inside: &[el::TEXT], attrs: &[], holds: "one of el, txt, comment and pi" },
    ElDef {
        name: el::ELEMENT,
        inside: &[el::PARAGRAPH, el::RUN],
        attrs: &[],
        holds: "one el: the element of the paragraph or the run, with its name and attributes as the file had them and none of its children",
    },
    ElDef {
        name: el::LEVEL,
        inside: &[el::PARAGRAPH],
        attrs: &[],
        holds: "list and item, each one el, and each there only for the paragraph that opens that list or item",
    },
    ElDef {
        name: el::LIST,
        inside: &[el::LEVEL],
        attrs: &[],
        holds: "one el: a text:list, with its name and attributes as the file had them and none of its children",
    },
    ElDef {
        name: el::ITEM,
        inside: &[el::LEVEL],
        attrs: &[],
        holds: "one el: a text:list-item or text:list-header, with its name and attributes as the file had them and none of its children",
    },
    ElDef {
        name: el::LONG,
        inside: &[],
        attrs: &[text(at::N_LONG, Some(""), "the name of the attribute this holds the text of"), flag(at::B64, "the text is base64")],
        holds: "the text of a field longer than the attribute cap; it stands in any element with a text attribute",
    },
];

/// The definition of the element called `local`.
pub fn element(local: &str) -> Option<&'static ElDef> {
    ELEMENTS.iter().find(|e| e.name == local)
}

/// The definition of the attribute `name` of the element `local`, matching a `-b64` suffix to its text attribute.
pub fn attribute(local: &str, name: &str) -> Option<&'static AttrDef> {
    let def = element(local)?;
    let base = name.strip_suffix(at::B64_SUFFIX).unwrap_or(name);
    def.attrs.iter().find(|a| a.name == name).or_else(|| def.attrs.iter().find(|a| a.name == base && a.kind == Kind::Text))
}
