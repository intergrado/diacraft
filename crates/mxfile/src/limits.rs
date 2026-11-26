//! Every cap the reader applies, as a named constant with the rule that sets it, and the [`Limits`] that carry them (design §7 of the draw.io import's design).
//!
//! The numbers are the specification's, and measures them on crafted files and may lower them, and only it. Each cap refuses in words with its class or
//! warns with a count: nothing is cut silently. A limit is checked **before** the thing it bounds is allocated or walked, and what a file claims about itself is never
//! trusted. Every constant is also a field of [`Limits`], so that a test can hold a cap exactly (a cap and one byte more) with a small one instead of a hundred megabytes.
//! Those no task before the one that reads the thing has a use for are listed here too, so that the table of the design is in one place.

use diacraft_doc::{MAX_PAGES, MAX_ROWS_PER_SECTION, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE};

/// A file read: 256 MiB. A file of more is refused as `drawio_too_large`, before a byte of it is looked at.
pub const MAX_FILE_BYTES: u64 = 256 << 20;

/// The prolog the sniff looks at before the name of the root: a byte order mark, an XML declaration, comments and white space, 64 KiB (design §1.9). A file whose root
/// comes later is not claimed. A constant and no field of [`Limits`]: the sniff takes no limits.
pub const MAX_PROLOG_BYTES: usize = 64 * 1024;

/// The XML of one page, plain (the span of its `diagram` element) or inflated and decoded: 64 MiB (R14 for the inflated page). Past it, `drawio_too_large`.
pub const MAX_PAGE_TEXT: u64 = 64 << 20;

/// The XML of all the pages of a file, after decoding: 256 MiB. Past it, `drawio_too_large`: the sum is capped, and not only each page, since a page under the
/// floor of the ratio can be a thousand to one.
pub const MAX_FILE_TEXT: u64 = 256 << 20;

/// The floor of the ratio of an inflated page to its compressed size: 1 MiB. Output under it is never refused for its ratio.
pub const RATIO_FLOOR: u64 = 1 << 20;

/// The ratio of an inflated page to its compressed size past the floor: 200 to 1 (real pages stay under 40, DEFLATE cannot pass about 1,032). Past it,
/// `drawio_ratio_exceeded`.
pub const MAX_RATIO: u64 = 200;

/// The depth of the XML of a file or a page, the root's level included: 64 elements. Past it, `drawio_too_deep`. The model's depth cap, 200, is lower than draw.io's
/// own 256 and is the one that counts for cells (`MAX_SHAPE_DEPTH`); this one is for the XML.
pub const MAX_XML_DEPTH: usize = 64;

/// The elements of one page's model: 2,000,000. Past it, `drawio_too_complex`. The reader of the cells counts them before it reads any (`file`).
pub const MAX_PAGE_ELEMENTS: usize = 2_000_000;

/// The items of the XML tree of one page, counted as the container counts them (each element, attribute, text, comment and instruction): 6,000,000, three for each of the
/// elements of [`MAX_PAGE_ELEMENTS`] (a labelled choice: a cell and its geometry are about twelve items for two elements, and a real page is far under). Past it,
/// `drawio_too_complex`. It bounds the memory of the tree.
pub const MAX_PAGE_ITEMS: usize = 6_000_000;

/// One attribute value: 32 MiB (a label, a style or a data URI). Past it, `drawio_too_large`.
pub const MAX_ATTR_BYTES: usize = 32 << 20;

/// The waypoints of a connector: 10,000. Past it they are dropped, with `drawio_too_many_points`.
pub const MAX_POINTS: usize = 10_000;

/// A coordinate or a size, in px: 10,000,000. Past it it is clamped, with `drawio_bad_number`.
pub const MAX_COORD_PX: f64 = 10_000_000.0;

/// The levels of cells that hang from one another below a layer: 200, the model's `MAX_SHAPE_DEPTH`. A cell deeper than that is the orphan, moved to the first layer, with
/// `drawio_too_deep_tree`.
pub const MAX_CELL_DEPTH: usize = MAX_SHAPE_DEPTH;

/// The smallest page size in px: 1. A size below it is clamped to it, with `drawio_bad_number`.
pub const MIN_PAGE_PX: f64 = 1.0;

/// The largest page size in px: 1,000,000. A size above it is clamped to it, with `drawio_bad_number`.
pub const MAX_PAGE_PX: f64 = 1_000_000.0;

/// The smallest page scale: 0.001. **A choice of ours** that R16 does not make: `pageScale` is a factor of the page's tile, and a scale of nothing, a negative one or one that makes
/// a tile infinite would leave the page with no size. A scale outside `MIN_PAGE_SCALE` to [`MAX_PAGE_SCALE`] is the nearest end of it, with `drawio_bad_number`, and one that is no
/// number is 1.
pub const MIN_PAGE_SCALE: f64 = 0.001;

/// The largest page scale: 1,000 (see [`MIN_PAGE_SCALE`]).
pub const MAX_PAGE_SCALE: f64 = 1_000.0;

/// The longest text that is read as a number, in bytes: 64. **A choice of ours** (R33 says a value that is not a finite number is 0): the numbers real files write are under 40
/// bytes, and a value of 32 MiB of digits would be parsed for nothing, so a longer value is no number.
pub const MAX_NUMBER_BYTES: usize = 64;

/// The levels of arrays and objects that the reader of a JSON attribute (`vars`, `backgroundImage`) follows: 64. A text nested deeper is not read.
pub const MAX_JSON_DEPTH: usize = 64;

/// The file's `vars` that are read: the model's `MAX_ROWS_PER_SECTION`, 320,000 (makes them properties). The rest are dropped, with `drawio_too_many_properties`.
pub const MAX_VARS: usize = MAX_ROWS_PER_SECTION;

/// The attribute names that a case of `drawio_unknown_attribute` lists: 8. The count goes on past it.
pub const MAX_UNKNOWN_NAMES: usize = 8;

/// An HTML label read: 1 MiB (S-R25). Past it the label is cut, with `drawio_label_too_long`.
pub const MAX_LABEL_BYTES: usize = 1 << 20;

/// The levels of elements of an HTML label that are followed: 64 (S-R25). Deeper markup is read as text.
pub const MAX_LABEL_DEPTH: usize = 64;

/// The XML of an inline stencil: 4 MiB (S-R16). Past it, the stand-in, with `drawio_stencil_too_large`.
pub const MAX_STENCIL_BYTES: usize = 4 << 20;

/// The elements of an inline stencil: 100,000 (S-R16).
pub const MAX_STENCIL_ELEMENTS: usize = 100_000;

/// A picture, decoded: 32 MiB (design §7). Past it, a placeholder, with `drawio_picture_too_large`.
pub const MAX_PICTURE_BYTES: usize = 32 << 20;

/// All the pictures of a file, decoded: 256 MiB (design §7).
pub const MAX_MEDIA_BYTES: u64 = 256 << 20;

/// The placeholders replaced in one label: 1,000 (design §7). The rest are left as written, with `drawio_placeholder_frozen`.
pub const MAX_PLACEHOLDERS: usize = 1_000;

/// The labels of a page that follow their connector by formula: 2,000 (design §1.6). Past it a label is a loose text shape, with `drawio_label_not_tied`.
pub const MAX_TIED_LABELS: usize = 2_000;

/// The named cases a warning keeps: 200. The count goes on past it.
pub const MAX_CASES: usize = 200;

/// The chunks of a PNG that are walked: 10,000. Past it, there is no drawing.
pub const MAX_PNG_CHUNKS: usize = 10_000;

/// The `content` of an SVG: 64 MiB. Past it, `drawio_too_large`.
pub const MAX_SVG_CONTENT: usize = 64 << 20;

/// The steps of work a read may do, to start: a billion (design §7). Past it, `drawio_too_complex`. Every loop of the reader charges the units it walks before it runs
/// (`ReadWork`), and the sum of them is held to this. **Sets the number** from the measures on crafted files: with the units as they are counted (a byte
/// walked by a pass is one step), a file at the cap of its decoded text costs about 4 steps a byte, a little over a billion, so that the default refuses a file a few
/// pages short of the cap of its text.
pub const MAX_READ_WORK: u64 = 1_000_000_000;

/// What a read accepts. [`Limits::default`] holds the constants above, so `Limits { max_pages: 3, ..Limits::default() }` changes one and keeps the rest. No field is ever
/// raised by the reader.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// Most bytes of a file: [`MAX_FILE_BYTES`].
    pub max_file_bytes: u64,
    /// Most bytes of the XML of one page: [`MAX_PAGE_TEXT`].
    pub max_page_text: u64,
    /// Most bytes of the XML of all the pages of a file: [`MAX_FILE_TEXT`].
    pub max_file_text: u64,
    /// The floor of the ratio: [`RATIO_FLOOR`].
    pub ratio_floor: u64,
    /// The ratio of an inflated page to its compressed size: [`MAX_RATIO`].
    pub max_ratio: u64,
    /// Deepest XML, the root's level included: [`MAX_XML_DEPTH`].
    pub max_xml_depth: usize,
    /// Most elements of a page: [`MAX_PAGE_ELEMENTS`].
    pub max_page_elements: usize,
    /// Most items of the XML tree of a page: [`MAX_PAGE_ITEMS`].
    pub max_page_items: usize,
    /// Longest attribute value: [`MAX_ATTR_BYTES`].
    pub max_attr_bytes: usize,
    /// Most pages: the model's [`MAX_PAGES`].
    pub max_pages: usize,
    /// Most cells of a page: the model's [`MAX_SHAPES_PER_PAGE`].
    pub max_shapes_per_page: usize,
    /// Most waypoints of a connector: [`MAX_POINTS`].
    pub max_points: usize,
    /// Largest coordinate or size in px: [`MAX_COORD_PX`].
    pub max_coord_px: f64,
    /// Most bytes of an HTML label: [`MAX_LABEL_BYTES`].
    pub max_label_bytes: usize,
    /// Most levels of an HTML label: [`MAX_LABEL_DEPTH`].
    pub max_label_depth: usize,
    /// Most bytes of an inline stencil: [`MAX_STENCIL_BYTES`].
    pub max_stencil_bytes: usize,
    /// Most elements of an inline stencil: [`MAX_STENCIL_ELEMENTS`].
    pub max_stencil_elements: usize,
    /// Most bytes of a picture: [`MAX_PICTURE_BYTES`].
    pub max_picture_bytes: usize,
    /// Most bytes of all the pictures: [`MAX_MEDIA_BYTES`].
    pub max_media_bytes: u64,
    /// Most placeholders replaced in a label: [`MAX_PLACEHOLDERS`].
    pub max_placeholders: usize,
    /// Most labels of a page tied to their connector: [`MAX_TIED_LABELS`].
    pub max_tied_labels: usize,
    /// Most named cases a warning keeps: [`MAX_CASES`].
    pub max_cases: usize,
    /// Most chunks of a PNG walked: [`MAX_PNG_CHUNKS`].
    pub max_png_chunks: usize,
    /// Most bytes of an SVG's `content`: [`MAX_SVG_CONTENT`].
    pub max_svg_content: usize,
    /// Most steps of work: [`MAX_READ_WORK`].
    pub max_read_work: u64,
    /// Most levels of cells below a layer: [`MAX_CELL_DEPTH`].
    pub max_cell_depth: usize,
    /// Largest page size in px: [`MAX_PAGE_PX`].
    pub max_page_px: f64,
    /// Longest text read as a number: [`MAX_NUMBER_BYTES`].
    pub max_number_bytes: usize,
    /// Most levels of a JSON attribute followed: [`MAX_JSON_DEPTH`].
    pub max_json_depth: usize,
    /// Most of the file's `vars` read: [`MAX_VARS`].
    pub max_vars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_file_bytes: MAX_FILE_BYTES,
            max_page_text: MAX_PAGE_TEXT,
            max_file_text: MAX_FILE_TEXT,
            ratio_floor: RATIO_FLOOR,
            max_ratio: MAX_RATIO,
            max_xml_depth: MAX_XML_DEPTH,
            max_page_elements: MAX_PAGE_ELEMENTS,
            max_page_items: MAX_PAGE_ITEMS,
            max_attr_bytes: MAX_ATTR_BYTES,
            max_pages: MAX_PAGES,
            max_shapes_per_page: MAX_SHAPES_PER_PAGE,
            max_points: MAX_POINTS,
            max_coord_px: MAX_COORD_PX,
            max_label_bytes: MAX_LABEL_BYTES,
            max_label_depth: MAX_LABEL_DEPTH,
            max_stencil_bytes: MAX_STENCIL_BYTES,
            max_stencil_elements: MAX_STENCIL_ELEMENTS,
            max_picture_bytes: MAX_PICTURE_BYTES,
            max_media_bytes: MAX_MEDIA_BYTES,
            max_placeholders: MAX_PLACEHOLDERS,
            max_tied_labels: MAX_TIED_LABELS,
            max_cases: MAX_CASES,
            max_png_chunks: MAX_PNG_CHUNKS,
            max_svg_content: MAX_SVG_CONTENT,
            max_read_work: MAX_READ_WORK,
            max_cell_depth: MAX_CELL_DEPTH,
            max_page_px: MAX_PAGE_PX,
            max_number_bytes: MAX_NUMBER_BYTES,
            max_json_depth: MAX_JSON_DEPTH,
            max_vars: MAX_VARS,
        }
    }
}

impl Limits {
    /// The limits of the XML trees the reader builds: the depth, the items of a page and the size of an attribute, as the container's parser takes them.
    pub fn xml(&self) -> diacraft_container::XmlLimits {
        diacraft_container::XmlLimits { max_depth: self.max_xml_depth, max_nodes: self.max_page_items, max_attr_bytes: self.max_attr_bytes }
    }
}
