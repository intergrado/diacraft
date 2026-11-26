//! The content check for OpenDocument drawings: what "the same content" means for two `.odg` packages, as the lists the container's check
//! ([`diacraft_container::canon`]) is given, and the comparison of a flat `.fodg` with a package (design section 4.3).
//!
//! The rules are the container's: the same set of entries by their exact names, folders included; XML entries compared with spacing, attribute
//! order, prefixes, namespace declarations and comments ignored; every other entry byte for byte. What this module adds is what only the format
//! can say, and each entry below has its source.
//!
//! # The lists
//!
//! **Siblings whose order has no meaning** ([`CANON_RULES`]`.keyed`):
//! * **Manifest rows (`manifest:file-entry`) by `manifest:full-path`.** Source: OpenDocument 1.4 Part 2, the manifest: each row describes one
//!   file of the package, named by its full path, and the package's own directory is a separate list; are the
//!   project's rules for the order a writer writes them in, which no reader depends on.
//! * **Styles (`style:style`) by `style:family` and `style:name`.** Source: (ODF 1.4 Part 3 §16.1): a style is found by
//!   family and name, and a container holds no two of one pair. This covers `office:styles` and `office:automatic-styles` alike, and no other
//!   element holds a `style:style`. What is *not* keyed, because no source says its order has no meaning: `style:default-style`, `text:list-style`,
//!   `style:page-layout` and `style:master-page` (a drawing's master pages are a list the person sees in order), the number styles, `draw:layer`
//!   and `draw:layer-set` (the layer set is the order of the layers, and `the_layer_set_reordered_fails` plants it).
//! * **Font faces (`style:font-face`) by `style:name`.** Source: a face is named, and a style names the face it uses.
//! * **The named definitions of `office:styles`**: `draw:marker`, `draw:gradient`, `draw:opacity`, `draw:hatch`, `draw:stroke-dash` and
//!   `draw:fill-image`, each by `draw:name`. Source: a style names its dash, gradient or marker, so the definitions
//!   are a set by name.
//!
//! **What says which thing an element is** ([`CANON_RULES`]`.identified_by`): `draw:name`, `draw:id`, `xml:id`, `style:name`, `style:family`, `table:name` and `config:name`.
//! The children of an element that are not keyed (the pages, the shapes of a page, the master pages, the layers, the settings) are set side by side by what they are, and not by the place
//! they stand in: an element the written file adds (`office:font-face-decls`, in 100 sample files) or drops (`office:scripts`, in 291 of 320 flat sample files) is **one difference**, and
//! every sibling after it is still compared, in depth, with its own (set by place, the root's children were each compared with the one beside them, and nothing inside was looked at: of 638
//! differences planted in `content.xml` of 51 files, none was seen). An element that has some of these attributes is told from its siblings by its name and their values, so a page, a
//! layer or a shape that changed, or was put in or taken out, is matched with the one it is; the same children in another order are **one** [`Found::Order`](diacraft_container::canon::Found::Order) (pages, shapes in z-order, layers
//! are in an order the person sees, so an order is a difference), and what is inside them is still compared. The attributes only choose what is set beside what: every pair is compared in
//! full. Source: ODF 1.4 Part 3 gives each to the element it names (`draw:name` to a page, a layer, a shape and a table, `draw:id` and `xml:id` to a shape and a glue point, `style:name` to a
//! master page, a page layout and a list style, `style:family` to a default style, `config:name` to a setting). See `diacraft_container::canon` for the three keys and what each matches.
//!
//! **Lengths** (NP8, [`CANON_RULES`]`.lengths`): exactly the length attributes the `.odg` writer respells, each with `lengths.rs` or the line that
//! writes it as its source. The writer gives a length as the shortest decimal that reads back, in the attribute's own unit (`2.50cm` is written
//! `2.5cm`), so those attributes pass when the number is the same `f64` and the unit the same text (`2.5cm` is not `25mm`), **read as ODF reads a
//! length** (a `-` and no `+`, no exponent, a unit of `cm`, `mm`, `in`, `pt`, `pc`, `px` or `%` directly after the number):
//! `+2.5cm`, `25e-1cm` and `2.5e0cm` are not lengths, so they are text and differ from `2.5cm`. They are in `lengths` and never in `numbers`, whose
//! reading is looser. Each is listed under the ODF 1.4 namespace and under the pre-standard W3C one that says the reader also takes
//! (`fo` and `svg`):
//! * `svg:x`, `svg:y`, `svg:width`, `svg:height` (a frame, `write/frame.rs` `put`, `write/shapes.rs` `place`), `svg:cx`, `svg:cy`, `svg:r`,
//!   `svg:rx`, `svg:ry` (a circle's or an ellipse's centre form, `write/frame.rs`), `svg:x1`, `svg:y1`, `svg:x2`, `svg:y2` (a line's or a connector's
//!   ends, `write/shapes.rs`);
//! * `svg:stroke-width` (the cell `LineWeight`, `write/stylemap.rs`) and `draw:marker-start-width`, `draw:marker-end-width` (a line end's width,
//!   `write/markers.rs` writes it as `{n}pt`);
//! * `fo:padding-top`, `fo:padding-bottom`, `fo:padding-left`, `fo:padding-right` (the four margins of a text block, `write/stylemap.rs`);
//! * `fo:margin-left`, `fo:margin-right`, `fo:margin-top`, `fo:margin-bottom`, `fo:text-indent`, `fo:line-height` (a length, `write/runs.rs`),
//!   `fo:letter-spacing`, `fo:font-size` (`write/runs.rs`, `write/text.rs`), and `fo:page-width`, `fo:page-height` with the page's margins
//!   (`write/stylemap.rs` `page_ops`);
//! * **the percentages** (ruling Call 22: a number's spelling may differ, so `75%` is `75.0%` and is not `76%`): `draw:opacity`, `draw:shadow-opacity`,
//!   `style:text-scale`, `style:text-position`, a gradient's `draw:start-intensity`, `draw:end-intensity`, `draw:border`, `draw:cx` and `draw:cy`, and a dash's
//!   `draw:dots1-length`, `draw:dots2-length` and `draw:distance`, which may be percentages of the stroke width (a percentage
//!   is a decimal number and `%`). Not under the W3C namespaces: they are the standard's own names. **`svg:stroke-opacity`**, "a number from 0 to 1, or a
//!   percentage", is the one attribute of [`CANON_RULES`]`.numbers`: `0.5` is `0.50` and `75%` is `75.0%`, the unit being the same.
//!
//! What is **not** listed, so that a respelling in it is a difference: every integer or index (`draw:z-index`, `draw:corners`, `text:c`,
//! `text:level`, a dash's `draw:dots1`), every angle, every name and every id (`007` is not `7`), `fo:padding` (the writer only takes it out),
//! `draw:corner-radius`, `svg:viewBox`, `svg:d`, `draw:points` and `draw:transform` (text with numbers in it, which the writer keeps as written or
//! writes whole).
//!
//! **Paragraphs** (`text:p`, `text:h`, `text:span`): white space is compared as ODF reads it (ODF 1.4 Part 3 §6.1.2): a run of
//! white space is one space, a paragraph's first and last are dropped, and `text:s` stands for its `text:c` spaces. **Mixed content**
//! ([`CANON_RULES`]`.exact_text`): the elements ODF lets stand inside paragraph content that hold text and children and are not paragraph
//! elements: `text:a`, `text:meta`, `text:meta-field`, `text:ruby-base` and `text:execute-macro` (ODF 1.4 Part 3 §6.1 to §6.4). In them blank text
//! is never ignored: a lone space between two spans of a link is a space and not indentation.
//!
//! # Empty containers
//!
//! **An empty container is the same as an absent one, for six elements and for no others** (`docs/diacraft-format.md` section 4.7): `office:meta`,
//! `office:scripts`, `office:font-face-decls`, `office:styles`, `office:automatic-styles` and `office:master-styles`, by their namespace and local name
//! (`EMPTY_CONTAINERS`, in [`CANON_RULES`]`.absent_when_empty`). An element of the six is *empty* when it has no attribute, no child element, no processing
//! instruction and no text but white space (a namespace declaration and a comment are no content, as everywhere in the check); one that holds anything is
//! compared as every element is. **Source:** a rule of that format that rests on ODF 1.3 Part 3, 3.2, 3.12, 3.14 and 3.15.2 to 3.15.4 and the schema, where each
//! of the six is a container of optional content that has no attribute, so an empty one holds what an absent one holds. ODF does not say in words that the two
//! are equivalent; the rule is made so that a writer that writes an empty container and one that leaves it out are not told apart (rows O9 and O9d of the format
//! document's section 8.5).
//!
//! **Not on the list**, so each is compared as it is: `office:settings` (ODF needs at least one item set in it: row O9e), `office:body`, `office:drawing`, a
//! `draw:page` (O9b), an empty paragraph (O9c), `draw:g`, `draw:frame` and every other element, and the name of one of the six in another namespace.
//!
//! **Where the rule is applied.** It is the container's rule (`Rules::absent_when_empty` of [`diacraft_container::canon`]), applied where a part's children are built,
//! so it holds in every form of the check: two packages (the roots of `styles.xml`, `content.xml` and `meta.xml`, each compared as XML), a package against a flat
//! file, and two flat files (the joined document). It leaves the joins alone: the container that two parts both hold is joined as before, and a joined container
//! that is still empty is then an absent one. What the container's reading adds, each with a test in `tests/it/canon_empty.rs`:
//! * a name written twice among the children of one element (a repeat, which the format never merges) is left out only when **every copy is empty**: an empty
//!   `office:styles` before a full one is not the full one alone, since a reader that takes the first of two reads the file differently (a false same is not
//!   allowed, and a false difference is);
//! * the difference between a container that holds something and an empty one is the one container, an element on one side only, as against an absent one: it is
//!   not located inside it.
//!
//! # What counts as nothing, and names that are free
//!
//! A program that writes a drawing again says things that are no fact about the drawing, and the check does not count them (`docs/diacraft-format.md` 4.5):
//! * **Attributes that count as nothing wherever they stand** ([`CANON_RULES`]`.ignored_attributes`): `office:version` and `manifest:version` (it stands on the manifest's root
//!   and on its rows, where an embedded object's row says the version of the object).
//! * **A child that counts as nothing** (`ignored_children`): `meta:generator` under `office:meta`. A `meta.xml` that holds only a generator, or only an empty `office:meta`, holds nothing and
//!   is the same as no `meta.xml` (the empty part, `EMPTY_PARTS`), and so is a `settings.xml` that holds nothing but an empty `office:settings`; under the root of a flat document an empty
//!   `office:settings` is none (`EMPTY_UNDER`).
//! * **The fields of `office:meta` in any order** (`unordered`, `META_FIELDS`): the order of a document's fields is no content, and the copies of one field are in the order written. A field
//!   added, taken out or changed is a difference (`meta:user-defined` added, `dc:creator` or `meta:initial-creator` taken out, `meta:creation-date` added, `meta:print-date` taken out).
//! * **The manifest's rows for what the entries say** (`derived_rows`, packages only): the rows are a set by their path (above), and a row that the other manifest has no row of the path of is
//!   no difference when it names an entry that only its package holds (the entry is the difference, and is counted as one) or a plain folder that files of its package stand under (no media
//!   type, or the row of `DiaCraft/` with the media type DiaCraft gives it). A folder with a media type that names a kind, a folder no file stands under, a row for no entry, a row for an
//!   entry both hold that the other manifest does not list, and a row that differs in a media type or in any attribute but its version are differences.
//! * **The names of automatic styles** (`free_names`, `FREE_NAMES`; the container's [`diacraft_container::canon::FreeNames`]): the children of the one `office:automatic-styles` of a
//!   part or of a flat document are definitions (`style:style` by `style:family` and `style:name`, `text:list-style` and `style:page-layout` by `style:name`), and a reference names one by
//!   its kind and family: two definitions of one content are the same under any names, and a shape that names one must name its match. **A common style's name is kept**, a definition that
//!   has no match keeps its name (so a style whose property changed is one difference in that style), and a name inside an automatic style, a name that two definitions of one kind and
//!   family hold, a document with two containers and a reference that asks for another family than the definition of its name has are compared as written.
//! * **DiaCraft's own carrier counts as nothing when only the written file has it** (`ignored_when_extra`, `EXTRA_MARKUP`, `without_extra_carrier`): the entries under
//!   `DiaCraft/` (ASCII case-insensitively, the folder included), their manifest rows and the attributes and elements of DiaCraft's namespace, that the second file has and the first has
//!   not. **The check is oriented by it**: what the first file (the source) held of the carrier is compared as written, so an entry of it that the second lacks is a missing part, and an
//!   object that rides only in a carrier entry of the second still leaves its own entry of the first missing.
//! * **An `xml:id` beside an equal `draw:id` counts as nothing when only the written file has it** (`twinned`, `TWINNED`): a file of ODF 1.0 or 1.1 names a shape by `draw:id` alone,
//!   and the file a visit writes says ODF 1.4, which takes `draw:id` only beside an `xml:id` (the schema's `common-draw-id-attlist`; both with equal values are one id).
//!   **Oriented** as the carrier is: an `xml:id` the source holds is compared as written, so one the written file lacks is a difference, and one whose value is not the `draw:id`'s on its element
//!   is a difference. It is the same element's twin and no other (an `xml:id` with no `draw:id` beside it is any attribute).
//! * **Copies of an automatic style count as one, and a reference from one automatic style to another is resolved**: see `FREE_NAMES`. `style:parent-style-name` never
//!   names an automatic style (§19.510).
//! * **Not the same, as before**: a layer set added where the source had none, DiaCraft's own markup that the source held and the written file lacks, every property and every name that is
//!   none of the above.
//!
//! * **A DOCTYPE in the manifest** ([`CANON_RULES`]`.doctype_parts`): `META-INF/manifest.xml` may begin with one DOCTYPE that holds only a name and an external id (and no
//!   internal subset), as an old OpenOffice.org writer writes it (32 sample files). It declares nothing and is read and left out, as the package layer reads a manifest, so such a manifest
//!   is read as one that has none: the DOCTYPE is no content (it was compared as bytes, and every such file differed). Any other DTD is refused as ever. This crate's own reading of a manifest
//!   ([`parse_manifest`], for the rows of an empty part and of an entry only one package holds) reads it so too.
//!
//! The first four are lists of the container's check; the manifest's rows are the one rule that this crate applies itself, to each side's manifest before the entries are compared.
//!
//! # Keys, prefixes in values, and what the container bounds
//!
//! Every key is folded **exactly** ([`KeyFold::Exact`]): OpenDocument entry names and `style:name`s are compared as written. The attributes that
//! hold prefixes (`Rules::qnames`) are none in a drawing's vocabulary that a writer of ours changes, so the list is empty. The container bounds what the
//! lists make reachable: a paragraph's text is one string whose bytes are counted, a part may hold [`MAX_COMPARE_ITEMS`] items on each side, and the parts
//! of both sides together [`MAX_COMPARE_BYTES`] (each package, half of that: [`compare_content`] holds the package reader to it).
//!
//! # A flat `.fodg` against a package (and against another flat file)
//!
//! A flat file has no entries, and its `office:automatic-styles` and `office:font-face-decls` hold what a package puts in two parts at once
//! So when either side is flat, **both sides are put into the one flat document the other would read as**, and the
//! documents are compared as one part, `office:document`:
//! * a flat file's root children are put in the order (the standard's), the order of the file being no content, and the
//!   ones that are repeated or foreign are kept as they are, after them, and **never merged**;
//! * a package is read into the document its four XML parts give: `office:meta` from `meta.xml`, `office:settings` from `settings.xml`,
//!   `office:scripts` and `office:body` from `content.xml`, `office:styles` and `office:master-styles` from `styles.xml`, and the two shared
//!   children joined, `styles.xml`'s first (the order of `write/convert.rs`, which writes a package as a flat file). The root says the media
//!   type the package says (`mimetype`, else the manifest's root row), and the `office:version` the parts state (several that differ read as
//!   one text, which differs from any flat file's);
//! * each child moved from a part root is given the declarations of the prefixes its names use (an element's, an attribute's, the default namespace's
//!   for an element with none) that its own tags do not bind, so a prefix means what it meant in its part: **the declarations of the whole root are not
//!   copied on every child**, since a root of a hundred thousand declarations with a hundred thousand children would be ten billion copies. Each copy is
//!   charged (its text and a fixed cost) against what one side may add, and one that does not fit is refused (`too_much_xml`). A shared child joined after
//!   another whose part bound other namespaces gives its own children the declarations of its part in the same way; two shared children whose plain
//!   attributes disagree are two siblings (which a flat file does not have, so it is a reported difference and never a false same).
//!   **Faces**: a `style:font-face` that stands in both parts and is *exactly* the same element in the same context of declarations is one (the writers of both
//!   formats declare each face in both parts, and `write/convert.rs` writes it twice into a flat file), and so is one written twice in a container, on either side;
//!   one that differs in an attribute or in what a prefix of it means is two. **A name both parts hold is a clash**: a child of the later
//!   part with the `style:name` (and `style:family`) of a child of the earlier one, an automatic style or a page layout of one name in both, or a face that is not
//!   that face. The package reader gives the content its own part's, a flat file's one container is read first-come, and `convert.rs` refuses to join them
//!   (`names_are_disjoint`), so **no flat file is the same as such a package**: the joined root carries `part-name-clash-N` (the container and the words), a name
//!   no flat file has, and the package differs from every flat file, as an attribute on one side only;
//! * **a picture is compared by its bytes**. In a flat file the base64 of `office:binary-data` is read (white space skipped) and written back without line breaks,
//!   and an `xlink:href` beside it is dropped (it is ignored). **In a package the href wins** (the reader follows it, whatever the element
//!   holds): a `draw:image` or `draw:fill-image` whose `xlink:href` names an entry of the package gets that entry's bytes as `office:binary-data`, loses its href, and
//!   loses the data beside it; one whose href names no entry (an address, a missing file) keeps the href as an attribute, so it differs from any flat file's picture.
//!   **The media type is content** (ruling Call 23): a `draw:image` that names an entry and states no `draw:mime-type` is given the one `write/convert.rs` writes
//!   for it, the manifest's media type for the entry or else the type its bytes' signature says, so that a flat file our converter wrote is the same; a flat file
//!   with no type, or another, differs. A `draw:fill-image` is given none. The pictures of one side, with the declarations copied (above), may not add up to
//!   more than a quarter of a part's cap (64 MiB by default): each copy is charged **the base64 it makes** (four thirds of the entry's bytes), a flat file's own data
//!   is charged what it decodes to (the white space inside it is the file's own and costs nothing), so that a package that names one large picture a million times
//!   is refused (`too_much_xml`, as [`OdgError::CompareCopies`], which says what was refused), not expanded. **One picture entry of about 48 MiB or more is refused
//!   the same way, however few times it is named**: its one copy is charged as base64, and 48 MiB of picture is 64 MiB of base64, the whole budget (three sixteenths
//!   of the cap on a part, less the media type's attribute and the declarations the parts' roots lend, which the copy shares the budget with). The same picture held
//!   inline in a flat file is charged what it decodes to, so a flat file's picture is compared up to 64 MiB;
//! * **the four parts of a package share one budget of items** (the smallest of the container's cap on one part's items, the reader's, and the comparison's
//!   [`MAX_COMPARE_ITEMS`]: 2,000,000 by default), as a flat file's one document has: each part is parsed with what the parts before it left, so that two
//!   parts each under the cap on one part and over it together are refused (`xml_too_complex`) before the second is built, and the trees of all four are
//!   never held at once;
//! * **what a flat file has no place for is not compared**: the entries of a package other than the four parts and the pictures its parts name
//!   (the manifest, `mimetype`, `Thumbnails/`, `Configurations2/`, the signatures, macros, embedded objects, DiaCraft's own entries),
//!   them. The check of a flat file against a package therefore says nothing of them;
//! * **an attribute of a part's root** other than its version and its namespace declarations has no place on the joined root: it is put there as
//!   `part-root-attribute-N` (with the part and the attribute as its value), a name no flat file has, so that it is reported, as an attribute on one
//!   side only, and never dropped unseen. **No flat file can spell a marker**: a flat file's own root attribute in no namespace whose name starts
//!   with `part-` (the start of both markers, `part-root-attribute-N` and `part-name-clash-N`) or with `flat-` is given a `flat-` before its name, so that
//!   `part-name-clash-0` written by hand is the attribute `flat-part-name-clash-0` of that file and never the package's marker. Naming `flat-` as well makes the
//!   renaming one-to-one (`part-x` and a hand-written `flat-part-x` stay two attributes, and so stay different), the file's own attributes are compared with their
//!   values like any other, and a name with a prefix is in a namespace and is left as it is. The name is five bytes longer, so one within five bytes of the XML
//!   limit on an attribute is refused as one past it (`xml_too_complex`).
//!
//! # What it does not see
//!
//! * A prefix used inside an attribute value, `xml:space`, the names a row names (the container's own list).
//! * Embedded objects of a flat file (an inline `office:document` or `math:math` in `draw:object`) against a package's `Object 1/` entries, and
//!   DiaCraft's extension (a child of a flat root against the entries of a package): both are reported as differences until a rule says how the
//!   two forms spell them.
//! * White space that stands alone against white space between empty containers: an element that holds nothing but white space (`<office:document-meta>`,
//!   a newline, `</office:document-meta>`) has it as content, and one whose only children are empty containers that stand between white space has none (the
//!   container is a child, so the white space is spacing, as everywhere). They differ: a false difference and never a false same.
//! * A part that one package has and the other has not (`meta.xml` against none) is an entry, not an element, so it is a difference between two packages, whatever
//!   the part holds. Against a flat file, which has no parts, a package's empty `office:meta` is none (see "Empty containers").
//!
//! # What it costs
//!
//! The container's: linear in the XML, `O(n log n)` in siblings of one key set (`Work::canon_steps`, `Work::canon_bytes`). **Free names add one pass over the container of automatic
//! styles of each side** (the content of each style written out once and looked up once: 1.5 to 2.1 times the steps and 1.8 to 2.8 times the bytes the check counted before, measured on
//! 100,000 styles by `tests/it/canon_free_names_scale.rs`, whatever the styles are: all alike, all different and matched, none matched), with a budget charged before anything is built (the
//! container's weight, 64 MiB). The flat path adds one
//! tree built for each side (parsed, joined, serialized and parsed again by the comparison: four times the XML of the side, which the part limits
//! and the XML limits bound before anything is built), a few passes over each tree (`canon_steps`: the ranking of the root's children, the names a
//! child uses, the join of two containers by maps, and the pictures), and the pictures' bytes and the declarations copied (`canon_bytes`). Measured
//! (`tests/it/canon_scale.rs`, `canon_hostile.rs`): 7 steps for each declaration and each child of a root of 20,000 declarations and 20,000 children;
//! 6.6 steps for each attribute of two containers of 20,000 attributes joined; 32 steps for each element of a flat file of 16,384 styles against its
//! package (1.4 times a package against a package of the same size).

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use diacraft_container::canon::{
    CanonError, ContentDiff, Definition, EmptyPart, EmptyUnder, Entry, FreeNames, IgnoredUnder, KeyFold, Keyed, MAX_COMPARE_BYTES, MAX_COMPARE_ITEMS,
    Name, ODF_TEXT_NAMESPACE, Reference, Rules, Takes, Twin, Unordered, compare_entries, part_holds_nothing,
};
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{self, El, Node, ParseOptions, XmlError, write_element};
use diacraft_container::{ContainerError, Work};

use crate::error::{LimitKind, OdgError};
use crate::flat::Form;
use crate::limits::Limits;
use crate::ns::{MANIFEST, OFFICE, PRESENTATION, XML};
use crate::package::{Inflate, Package, percent_decode, read_package};

const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const FO_W3C: &str = "http://www.w3.org/1999/XSL/Format";
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const SVG_W3C: &str = "http://www.w3.org/2000/svg";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const CONFIG: &str = "urn:oasis:names:tc:opendocument:xmlns:config:1.0";

const fn name(ns: &'static str, local: &'static str) -> Name {
    Name { ns, local }
}

const FULL_PATH: &[Name] = &[name(MANIFEST, "full-path")];
const STYLE_KEY: &[Name] = &[name(STYLE, "family"), name(STYLE, "name")];
const STYLE_NAME: &[Name] = &[name(STYLE, "name")];
const DRAW_NAME: &[Name] = &[name(DRAW, "name")];

/// The length attributes the writer respells, each under every namespace the format spells it in: the ODF 1.4 `fo` and `svg`, and the pre-standard W3C ones
/// They are [`Rules::lengths`], read as ODF reads a length, and never [`Rules::numbers`].
const LENGTH_ATTRIBUTES: &[Name] = &[
    name(SVG, "x"),
    name(SVG_W3C, "x"),
    name(SVG, "y"),
    name(SVG_W3C, "y"),
    name(SVG, "width"),
    name(SVG_W3C, "width"),
    name(SVG, "height"),
    name(SVG_W3C, "height"),
    name(SVG, "x1"),
    name(SVG_W3C, "x1"),
    name(SVG, "y1"),
    name(SVG_W3C, "y1"),
    name(SVG, "x2"),
    name(SVG_W3C, "x2"),
    name(SVG, "y2"),
    name(SVG_W3C, "y2"),
    name(SVG, "cx"),
    name(SVG_W3C, "cx"),
    name(SVG, "cy"),
    name(SVG_W3C, "cy"),
    name(SVG, "r"),
    name(SVG_W3C, "r"),
    name(SVG, "rx"),
    name(SVG_W3C, "rx"),
    name(SVG, "ry"),
    name(SVG_W3C, "ry"),
    name(SVG, "stroke-width"),
    name(SVG_W3C, "stroke-width"),
    name(DRAW, "marker-start-width"),
    name(DRAW, "marker-end-width"),
    // The percentages the odg notes list and the dash's lengths, which may be percentages of the stroke width: a
    // percentage is a decimal number directly followed by `%`, so `75%` is `75.0%` and is not `76%` (ruling Call 22).
    name(DRAW, "opacity"),
    name(DRAW, "shadow-opacity"),
    name(STYLE, "text-scale"),
    name(STYLE, "text-position"),
    name(DRAW, "start-intensity"),
    name(DRAW, "end-intensity"),
    name(DRAW, "border"),
    name(DRAW, "cx"),
    name(DRAW, "cy"),
    name(DRAW, "dots1-length"),
    name(DRAW, "dots2-length"),
    name(DRAW, "distance"),
    name(FO, "padding-top"),
    name(FO_W3C, "padding-top"),
    name(FO, "padding-bottom"),
    name(FO_W3C, "padding-bottom"),
    name(FO, "padding-left"),
    name(FO_W3C, "padding-left"),
    name(FO, "padding-right"),
    name(FO_W3C, "padding-right"),
    name(FO, "margin-left"),
    name(FO_W3C, "margin-left"),
    name(FO, "margin-right"),
    name(FO_W3C, "margin-right"),
    name(FO, "margin-top"),
    name(FO_W3C, "margin-top"),
    name(FO, "margin-bottom"),
    name(FO_W3C, "margin-bottom"),
    name(FO, "text-indent"),
    name(FO_W3C, "text-indent"),
    name(FO, "line-height"),
    name(FO_W3C, "line-height"),
    name(FO, "letter-spacing"),
    name(FO_W3C, "letter-spacing"),
    name(FO, "font-size"),
    name(FO_W3C, "font-size"),
    name(FO, "page-width"),
    name(FO_W3C, "page-width"),
    name(FO, "page-height"),
    name(FO_W3C, "page-height"),
];

/// The attributes that hold a number or a percentage (`svg:stroke-opacity`: "a number from 0 to 1, or a percentage"), under both namespaces: the
/// looser reading of [`Rules::numbers`], in which `0.5` is `0.50` and `75%` is `75.0%`, and the unit must be the same.
const NUMBER_ATTRIBUTES: &[Name] = &[name(SVG, "stroke-opacity"), name(SVG_W3C, "stroke-opacity")];

/// The six containers of a drawing that are the same as none when they are empty (`docs/diacraft-format.md` 4.7), in the office namespace: see "Empty containers" in
/// the [module documentation](self). `office:settings` is not one of them (ODF wants at least one set of settings in it), nor is the body.
const EMPTY_CONTAINERS: &[Name] = &[
    name(OFFICE, "meta"),
    name(OFFICE, "scripts"),
    name(OFFICE, "font-face-decls"),
    name(OFFICE, "styles"),
    name(OFFICE, "automatic-styles"),
    name(OFFICE, "master-styles"),
];

/// The attributes that count as nothing wherever they stand (for the second): `office:version`, on the root of every part and of a flat
/// document, and `manifest:version`, on the manifest's root and on its rows (the root row says the version of the document, an embedded object's row the version of the object). A writer
/// of the model says the one version it writes, which is no fact about the drawing.
const IGNORED_ATTRIBUTES: &[Name] = &[name(OFFICE, "version"), name(MANIFEST, "version")];

/// The fields of `office:meta`, in the order of the schema (the metadata elements of ODF 1.4 Part 3; `office:meta` is an interleave there): the order a document writes them in is no content, and the copies of one field are
/// in the order written (the keywords are a list; the user-defined fields are compared as they stand). A child that is none of these keeps its place among the others. `meta:generator`
/// is among them for the form's sake and is no content ([`IGNORED_CHILDREN`]).
const META_FIELDS: &[Name] = &[
    name(META, "generator"),
    name(DC, "title"),
    name(DC, "description"),
    name(DC, "subject"),
    name(META, "keyword"),
    name(META, "initial-creator"),
    name(DC, "creator"),
    name(META, "printed-by"),
    name(META, "creation-date"),
    name(DC, "date"),
    name(META, "print-date"),
    name(META, "template"),
    name(META, "auto-reload"),
    name(META, "hyperlink-behaviour"),
    name(DC, "language"),
    name(META, "editing-cycles"),
    name(META, "editing-duration"),
    name(META, "document-statistic"),
    name(META, "user-defined"),
];

const META_UNORDERED: &[Unordered] = &[Unordered { parent: name(OFFICE, "meta"), children: META_FIELDS }];

/// The children that count as nothing under their parent: the generator of `office:meta`, which is the name of the program that wrote the file (`meta:generator`, one of
/// the metadata elements of ODF 1.4 Part 3). A writer of the model says its own, and a `meta.xml` that holds only a generator holds nothing else.
const IGNORED_CHILDREN: &[IgnoredUnder] = &[IgnoredUnder { parent: name(OFFICE, "meta"), children: &[name(META, "generator")] }];

const STYLE_FAMILY: Name = name(STYLE, "family");
const NUMBER: &str = "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0";

const fn style_of(family: &'static str) -> Takes {
    Takes { element: name(STYLE, "style"), family }
}

const GRAPHIC: &[Takes] = &[style_of("graphic")];
const PARAGRAPH: &[Takes] = &[style_of("paragraph")];
const TEXT_STYLE: &[Takes] = &[style_of("text")];
const DRAWING_PAGE: &[Takes] = &[style_of("drawing-page")];
const PRESENTATION_STYLE: &[Takes] = &[style_of("presentation")];
const SECTION_STYLE: &[Takes] = &[style_of("section")];
const RUBY_STYLE: &[Takes] = &[style_of("ruby")];
const LIST_STYLE: &[Takes] = &[Takes { element: name(ODF_TEXT_NAMESPACE, "list-style"), family: "" }];
const PAGE_LAYOUT: &[Takes] = &[Takes { element: name(STYLE, "page-layout"), family: "" }];
/// The seven data styles, which share one family of names: a name that two of them hold is not told.
const DATA_STYLES: &[Takes] = &[
    Takes { element: name(NUMBER, "number-style"), family: "" },
    Takes { element: name(NUMBER, "currency-style"), family: "" },
    Takes { element: name(NUMBER, "percentage-style"), family: "" },
    Takes { element: name(NUMBER, "date-style"), family: "" },
    Takes { element: name(NUMBER, "time-style"), family: "" },
    Takes { element: name(NUMBER, "boolean-style"), family: "" },
    Takes { element: name(NUMBER, "text-style"), family: "" },
];
const NUM_LIST_FORMAT: &[Takes] = &[Takes { element: name(NUMBER, "num-list-format"), family: "" }];

const fn asks(attr: Name, on: Option<Name>, takes: &'static [Takes]) -> Reference {
    Reference { attr, on, takes, list: false }
}

const fn asks_for_a_list(attr: Name, on: Option<Name>, takes: &'static [Takes]) -> Reference {
    Reference { attr, on, takes, list: true }
}

/// The names that are free ("producers may name automatic styles freely"): the automatic styles of a part or of a flat document, which are
/// the children of its one `office:automatic-styles`, and the references to them. A common style's name is kept: only `office:automatic-styles` is a container of free names.
///
/// - **Definitions**: `style:style` (named by `style:name` and told by `style:family`), `text:list-style`, `style:page-layout`, the seven data styles of `number:` and
///   `number:num-list-format` (each named by `style:name`, and a reference names it by that name and nothing else).
/// - **References from the document**, each by the attribute and the element it is read on, and the family of style it asks for (ODF 1.4 Part 3, the part the attribute is defined in, as
///   checked them against the schema): `draw:style-name` asks a drawing-page style of `draw:page`, `style:master-page`, `presentation:notes` and
///   `style:handout-master`, and a graphic style of any other element; `presentation:style-name` a presentation style; `draw:text-style-name` a paragraph style; `text:style-name` a
///   paragraph style of `text:p` and `text:h`, a text style of `text:span` and `text:a`, a list style of `text:list`, a section style of `text:section` and a ruby style of `text:ruby`;
///   `text:visited-style-name` a text style of `text:a`; `text:cond-style-name` a paragraph style of `text:p` and `text:h`; `style:page-layout-name` a page layout of `style:master-page`,
///   `presentation:notes` and `style:handout-master`; the lists of names `text:class-names` (paragraph styles of `text:p` and `text:h`, text styles of `text:span`), `draw:class-names`
///   (graphic styles) and `presentation:class-names` (presentation styles), each name read on its own. A reference that asks for a family no definition of the name has is not a reference
///   to it.
/// - **References from one definition to another**: `style:list-style-name` of a `style:style` asks a list style; `style:data-style-name` and
///   `style:percentage-data-style-name` a data style; `text:style-name` of a list level (`text:list-level-style-bullet`, `text:list-level-style-number`) and of
///   `text:outline-level-style` a text style; `style:num-list-format-name` of a `text:list-level-style-number` a `number:num-list-format`. A chain of them is resolved to the content at its
///   end; a circle, and a chain of more than 64, are compared as written. **`style:parent-style-name` is not one**: the parent of a style is a common style (§19.510), and
///   `style:next-style-name` names a common style as well.
/// - **Not read**: a reference of an element no entry lists, and anything inside an embedded `office:document` (its names are its own).
const FREE_NAMES: &[FreeNames] = &[FreeNames {
    container: name(OFFICE, "automatic-styles"),
    definitions: &[
        Definition { element: name(STYLE, "style"), family: Some(STYLE_FAMILY) },
        Definition { element: name(ODF_TEXT_NAMESPACE, "list-style"), family: None },
        Definition { element: name(STYLE, "page-layout"), family: None },
        Definition { element: name(NUMBER, "number-style"), family: None },
        Definition { element: name(NUMBER, "currency-style"), family: None },
        Definition { element: name(NUMBER, "percentage-style"), family: None },
        Definition { element: name(NUMBER, "date-style"), family: None },
        Definition { element: name(NUMBER, "time-style"), family: None },
        Definition { element: name(NUMBER, "boolean-style"), family: None },
        Definition { element: name(NUMBER, "text-style"), family: None },
        Definition { element: name(NUMBER, "num-list-format"), family: None },
    ],
    name: name(STYLE, "name"),
    references: &[
        asks(name(DRAW, "style-name"), Some(name(DRAW, "page")), DRAWING_PAGE),
        asks(name(DRAW, "style-name"), Some(name(STYLE, "master-page")), DRAWING_PAGE),
        asks(name(DRAW, "style-name"), Some(name(PRESENTATION, "notes")), DRAWING_PAGE),
        asks(name(DRAW, "style-name"), Some(name(STYLE, "handout-master")), DRAWING_PAGE),
        asks(name(DRAW, "style-name"), None, GRAPHIC),
        asks(name(PRESENTATION, "style-name"), None, PRESENTATION_STYLE),
        asks(name(DRAW, "text-style-name"), None, PARAGRAPH),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "p")), PARAGRAPH),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "h")), PARAGRAPH),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "span")), TEXT_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "a")), TEXT_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "list")), LIST_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "section")), SECTION_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "ruby")), RUBY_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "visited-style-name"), Some(name(ODF_TEXT_NAMESPACE, "a")), TEXT_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "cond-style-name"), Some(name(ODF_TEXT_NAMESPACE, "p")), PARAGRAPH),
        asks(name(ODF_TEXT_NAMESPACE, "cond-style-name"), Some(name(ODF_TEXT_NAMESPACE, "h")), PARAGRAPH),
        asks_for_a_list(name(ODF_TEXT_NAMESPACE, "class-names"), Some(name(ODF_TEXT_NAMESPACE, "p")), PARAGRAPH),
        asks_for_a_list(name(ODF_TEXT_NAMESPACE, "class-names"), Some(name(ODF_TEXT_NAMESPACE, "h")), PARAGRAPH),
        asks_for_a_list(name(ODF_TEXT_NAMESPACE, "class-names"), Some(name(ODF_TEXT_NAMESPACE, "span")), TEXT_STYLE),
        asks_for_a_list(name(DRAW, "class-names"), None, GRAPHIC),
        asks_for_a_list(name(PRESENTATION, "class-names"), None, PRESENTATION_STYLE),
        asks(name(STYLE, "page-layout-name"), Some(name(STYLE, "master-page")), PAGE_LAYOUT),
        asks(name(STYLE, "page-layout-name"), Some(name(PRESENTATION, "notes")), PAGE_LAYOUT),
        asks(name(STYLE, "page-layout-name"), Some(name(STYLE, "handout-master")), PAGE_LAYOUT),
    ],
    definition_references: &[
        asks(name(STYLE, "list-style-name"), Some(name(STYLE, "style")), LIST_STYLE),
        asks(name(STYLE, "data-style-name"), Some(name(STYLE, "style")), DATA_STYLES),
        asks(name(STYLE, "percentage-data-style-name"), Some(name(STYLE, "style")), DATA_STYLES),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "list-level-style-bullet")), TEXT_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "list-level-style-number")), TEXT_STYLE),
        asks(name(ODF_TEXT_NAMESPACE, "style-name"), Some(name(ODF_TEXT_NAMESPACE, "outline-level-style")), TEXT_STYLE),
        asks(name(STYLE, "num-list-format-name"), Some(name(ODF_TEXT_NAMESPACE, "list-level-style-number")), NUM_LIST_FORMAT),
    ],
    scopes: &[name(OFFICE, "document")],
}];

/// The namespaces whose attributes and elements count as nothing when only the written file has them: DiaCraft's own extension, which a writer of the model adds to what
/// it writes.
const EXTRA_MARKUP: &[&str] = &[crate::ext::names::URN];

/// The attributes that say again what another attribute of their element says, in the form ODF 1.2 and later take (`Rules::twinned`): an `xml:id` beside a `draw:id` of the
/// same value. Source: ODF 1.4 Part 3 (`draw:id` is deprecated in favour of `xml:id`, and the schema takes it only beside one), and ODF 1.0 and 1.1, which have no `xml:id` and
/// say an id by `draw:id` alone, so that a writer that raises the version to 1.4 adds it. It counts as nothing only in the written file (the check is oriented by it).
const TWINNED: &[Twin] = &[Twin { attr: name(XML, "id"), with: name(DRAW, "id") }];

/// The spellings DiaCraft's own namespace has had (`Rules::aliases`, APPID): the namespace that builds made before the id moved to `diacraft.app` wrote reads as the one written now,
/// wherever it stands in a part (a declaration, the carrier's fragments inside an attribute's value), so that **a drawing one build wrote and the other wrote again from the model
/// are the same content**, and the carrier of the first (`EXTRA_MARKUP`, oriented) is found in the second.
const ALIASES: &[(&str, &str)] = &[(diacraft_doc::DIACRAFT_URN_LEGACY, diacraft_doc::DIACRAFT_URN)];

/// Whether a package entry is `settings.xml`.
fn is_settings_part(entry: &str) -> bool {
    entry == "settings.xml"
}

/// Whether a package entry is `meta.xml`.
fn is_meta_part(entry: &str) -> bool {
    entry == "meta.xml"
}

/// The parts that are the same as no part when they hold nothing: a `settings.xml` whose root holds nothing but an `office:settings` that
/// holds nothing, and a `meta.xml` whose root holds nothing but an `office:meta` that holds nothing (`office:version` counting as nothing in both). A part of either name that holds a
/// setting or a property is the part it is.
const EMPTY_PARTS: &[EmptyPart] = &[
    EmptyPart { named: is_settings_part, root: name(OFFICE, "document-settings"), children: &[name(OFFICE, "settings")] },
    EmptyPart { named: is_meta_part, root: name(OFFICE, "document-meta"), children: &[name(OFFICE, "meta")] },
];

/// An `office:settings` that holds nothing, under the root of a flat document (and of the document a package is read into to be compared with one): the same as none, as an empty
/// `settings.xml` is no `settings.xml`. Under any other parent it is the element it is.
const EMPTY_UNDER: &[EmptyUnder] = &[EmptyUnder { parent: name(OFFICE, "document"), children: &[name(OFFICE, "settings")] }];

/// The elements whose white space is ODF's paragraph rule.
const PARAGRAPH_ELEMENTS: &[Name] = &[name(ODF_TEXT_NAMESPACE, "p"), name(ODF_TEXT_NAMESPACE, "h"), name(ODF_TEXT_NAMESPACE, "span")];

/// Mixed content that is not a paragraph element: blank text is never ignored in it.
const EXACT_INLINE: &[Name] = &[
    name(ODF_TEXT_NAMESPACE, "a"),
    name(ODF_TEXT_NAMESPACE, "meta"),
    name(ODF_TEXT_NAMESPACE, "meta-field"),
    name(ODF_TEXT_NAMESPACE, "ruby-base"),
    name(ODF_TEXT_NAMESPACE, "execute-macro"),
];

/// The attributes that say which thing an element is, among its siblings (`Rules::identified_by`): the name of a page, a layer, a shape or a table (`draw:name`,
/// `table:name`), the identifier of a shape or a glue point (`draw:id`, `xml:id`), the name of a master page, a page layout, a list style or a setting (`style:name`,
/// `config:name`) and the family of a default style (`style:family`). Source: ODF 1.4 Part 3, where each names the one thing among its siblings that an element is (a drawing's
/// `draw:name`, `draw:id`, `xml:id`; the style families; `style:name`; a setting's `config:name`), and the rules of that a style, a face and a named
/// definition are found by their names. They are what the check sets the children of one parent side by side by (a child put in or taken out is then one difference), and
/// never what two children are the same by: every pair is compared in full.
const IDENTIFIED_BY: &[Name] =
    &[name(DRAW, "name"), name(DRAW, "id"), name(XML, "id"), name(STYLE, "name"), name(STYLE, "family"), name(TABLE, "name"), name(CONFIG, "name")];

/// The lists of the content check for OpenDocument: see the [module documentation](self) for each entry and its source. Every key is folded exactly: ODF
/// names are compared as written.
pub const CANON_RULES: Rules = Rules {
    keyed: &[
        Keyed { child: name(MANIFEST, "file-entry"), key: FULL_PATH, fold: KeyFold::Exact },
        Keyed { child: name(STYLE, "style"), key: STYLE_KEY, fold: KeyFold::Exact },
        Keyed { child: name(STYLE, "font-face"), key: STYLE_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "marker"), key: DRAW_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "gradient"), key: DRAW_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "opacity"), key: DRAW_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "hatch"), key: DRAW_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "stroke-dash"), key: DRAW_NAME, fold: KeyFold::Exact },
        Keyed { child: name(DRAW, "fill-image"), key: DRAW_NAME, fold: KeyFold::Exact },
    ],
    lengths: LENGTH_ATTRIBUTES,
    numbers: NUMBER_ATTRIBUTES,
    paragraphs: PARAGRAPH_ELEMENTS,
    exact_text: EXACT_INLINE,
    absent_when_empty: EMPTY_CONTAINERS,
    absent_when_empty_under: EMPTY_UNDER,
    unordered: META_UNORDERED,
    parts_absent_when_empty: EMPTY_PARTS,
    ignored_attributes: IGNORED_ATTRIBUTES,
    ignored_children: IGNORED_CHILDREN,
    ignored_when_extra: EXTRA_MARKUP,
    twinned: TWINNED,
    free_names: FREE_NAMES,
    aliases: ALIASES,
    identified_by: IDENTIFIED_BY,
    doctype_parts: &[crate::package::MANIFEST_PATH],
    ..Rules::NONE
};

/// What a package's joined root names an attribute of a part's root, and a name both parts hold, in a name no flat file has (`package_document`): each is followed by
/// a number. Both start with [`MARKER_START`].
const ROOT_ATTRIBUTE_MARKER: &str = "part-root-attribute-";
const NAME_CLASH_MARKER: &str = "part-name-clash-";
/// What the markers start with: a flat file's own root attribute in no namespace that starts so is renamed ([`starts_like_a_marker`]).
const MARKER_START: &str = "part-";
/// What is put before the name of a flat file's own root attribute that starts like a marker, or like what such a name is given, so that no flat file can spell a marker
/// and two names of a flat file never become one.
const OWN_ATTRIBUTE_PREFIX: &str = "flat-";

/// Whether the name of an attribute of a flat file's root is one that is given [`OWN_ATTRIBUTE_PREFIX`]: in no namespace (no prefix: a prefix is bound to a namespace,
/// whatever it is called) and starting [`MARKER_START`], or [`OWN_ATTRIBUTE_PREFIX`] itself. The second is what makes the renaming one-to-one: the names that are renamed
/// are exactly those that start `part-` or `flat-`, and what they become starts `flat-`, so no renamed name is another attribute's own.
fn starts_like_a_marker(key: &str) -> bool {
    !key.contains(':') && (key.starts_with(MARKER_START) || key.starts_with(OWN_ATTRIBUTE_PREFIX))
}

/// The label a refusal that no part is named for is given (a pass of the check's own that is past a limit).
const COMPARED: &str = "the parts compared";
/// The name the one part of a flat comparison is shown under.
const DOCUMENT: &str = "office:document";

/// Whether the drawings in `a` and `b` have the same content, or what differs: each is a package or a flat `.fodg` (by its first bytes), read within
/// `limits` (so one that cannot be read is an error, with its class), then compared by [`CANON_RULES`]. `shown` of the answer lists the first 200
/// differences, with the part and a path of element names and positions, and no name a person chose. See the [module documentation](self) for what
/// a flat file is compared as.
pub fn compare_content(a: &[u8], b: &[u8], limits: &Limits) -> Result<ContentDiff, OdgError> {
    compare_content_counted(a, b, limits, &mut Work::default())
}

/// [`compare_content`], adding what the comparison cost to `work`: `canon_steps` and `canon_bytes`, and `xml_events` for the XML read, for the
/// tests that hold a cost to a count.
pub fn compare_content_counted(a: &[u8], b: &[u8], limits: &Limits, work: &mut Work) -> Result<ContentDiff, OdgError> {
    match (Form::of(a), Form::of(b)) {
        (Form::Package, Form::Package) => compare_packages(a, b, limits, work),
        (form_a, form_b) => compare_flat(a, form_a, b, form_b, limits, work),
    }
}

fn limit_error(error: XmlError) -> OdgError {
    OdgError::Xml { part: COMPARED.to_owned(), error }
}

/// What the container refused, as this crate says it: a part past an XML limit or the budget of items, with its name, or the parts of both sides past the
/// budget of bytes (the limit on the XML bytes of a read, the same class).
fn canon_error(error: CanonError) -> OdgError {
    match error {
        CanonError::Xml { part, error } => OdgError::Xml { part, error },
        CanonError::TooLarge { limit, .. } => OdgError::Limit { kind: LimitKind::XmlBytes, max: limit },
        // The values of a part that name prefixes, read through the namespaces in scope, are longer than the limit: the XML error of its kind, naming the part.
        CanonError::Values { part, limit } => {
            OdgError::Xml { part, error: XmlError::AttributeTooLarge { limit: usize::try_from(limit).unwrap_or(usize::MAX) } }
        }
    }
}

/// The limits a package is read with: each package may inflate to half of what a comparison holds ([`MAX_COMPARE_BYTES`]), which the archive refuses from the
/// sizes it states, before anything is inflated.
fn halved(limits: &Limits) -> Limits {
    let mut limits = limits.clone();
    limits.container.max_package_bytes = limits.container.max_package_bytes.min(MAX_COMPARE_BYTES / 2);
    limits
}

/// The items one tree of the flat path may hold: the container's cap on one part's items, the reader's, and the comparison's budget, which the joined
/// document would be refused at in any case.
fn most_items(limits: &Limits) -> usize {
    limits.container.xml.max_nodes.min(limits.max_read_xml_nodes).min(MAX_COMPARE_ITEMS)
}

// ------------------------------------------------------------------------------------------------------------ two packages

/// The names the format's own list says are XML whatever the manifest says: the four parts and the manifest.
const XML_PARTS: [&str; 5] = ["content.xml", "styles.xml", "meta.xml", "settings.xml", "META-INF/manifest.xml"];

/// Whether a media type is XML's: it ends in `xml` (which covers `+xml`), parameters left off, and is not a picture's (`image/svg+xml` is a
/// picture, and a picture is the same bytes).
fn media_is_xml(media_type: &str) -> bool {
    let kind = media_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    kind.ends_with("xml") && !kind.starts_with("image/")
}

/// Whether the file `name` of `package` is meant as XML: the format's own list (the four parts, the manifest and the signatures of `META-INF/`),
/// or its manifest row's media type.
fn is_xml(package: &Package, name: &str) -> bool {
    XML_PARTS.contains(&name)
        || (name.starts_with("META-INF/") && name.ends_with(".xml"))
        || package.manifest().entry(name).and_then(|row| row.media_type.as_deref()).is_some_and(media_is_xml)
}

/// The entries of a package as the check sees them: every file by its exact name with its bytes as inflated, and every folder as an entry with none.
fn entries_of(package: &Package) -> Vec<Entry<'_>> {
    package
        .entries()
        .map(|e| {
            let name = e.name.as_str();
            if e.folder {
                Entry { name, xml: false, bytes: &[] }
            } else {
                Entry { name, xml: is_xml(package, name), bytes: package.part(name).unwrap_or_default() }
            }
        })
        .collect()
}

fn compare_packages(a: &[u8], b: &[u8], limits: &Limits, work: &mut Work) -> Result<ContentDiff, OdgError> {
    // Every file is inflated: a comparison compares every part's bytes, and the trees are not kept (the check parses each part it compares). Each package
    // may inflate to half of what a comparison holds.
    let limits = &halved(limits);
    let pa = read_package(a, limits, work, false, Inflate::All)?;
    let pb = read_package(b, limits, work, false, Inflate::All)?;
    let (ea, eb) = (entries_of(&pa), entries_of(&pb));
    // An empty `settings.xml` or `meta.xml` is no part, and the manifest's row for it names nothing, on either side.
    let (gone_a, manifest_a) = empty_parts(&ea, limits, work);
    let (gone_b, manifest_b) = empty_parts(&eb, limits, work);
    let ea = without(ea, &gone_a, manifest_a.as_deref());
    let eb = without(eb, &gone_b, manifest_b.as_deref());
    // The entries of DiaCraft's own that only the written file (the second) has are no difference; those the source (the first) has are compared as written.
    let eb = without_extra_carrier(&ea, eb);
    // A row that says only what an entry the other side lacks, or the folder of the files under it, already says names nothing, on either side.
    let (rows_a, rows_b) = derived_rows(&ea, &eb, limits, work);
    let ea = without(ea, &[], rows_a.as_deref());
    let eb = without(eb, &[], rows_b.as_deref());
    for side in [&ea, &eb] {
        let xml_bytes = side.iter().filter(|e| e.xml).fold(0u64, |sum, e| sum.saturating_add(e.bytes.len() as u64));
        if xml_bytes > limits.max_xml_bytes {
            return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
        }
    }
    compare_entries(&ea, &eb, false, &CANON_RULES, &limits.container.xml, work).map_err(canon_error)
}

/// A package's manifest as a tree: **an old writer's DOCTYPE is read, and is no content** (the container's [`ParseOptions::allow_external_doctype`], which every
/// other reading of a manifest here asks for): a manifest that has one is read as one that has none, and a manifest that does not parse is compared as it is.
fn parse_manifest(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<El, XmlError> {
    xml::parse_with_options(bytes, &limits.container.xml, ParseOptions { allow_external_doctype: true, ..ParseOptions::default() }, work)
}

/// The parts of one side that counts as absent ([`EMPTY_PARTS`]: an empty `settings.xml` or `meta.xml`), and the side's manifest written again without the rows
/// that name them (`None` when there is none to leave out, or the manifest does not parse: it is then compared as it is, and the row is a difference, never a false same).
fn empty_parts<'p>(entries: &[Entry<'p>], limits: &Limits, work: &mut Work) -> (Vec<&'p str>, Option<Vec<u8>>) {
    let gone: Vec<&'p str> = entries.iter().filter(|e| part_holds_nothing(e, &CANON_RULES, &limits.container.xml, work)).map(|e| e.name).collect();
    if gone.is_empty() {
        return (gone, None);
    }
    let Some(manifest) = entries.iter().find(|e| e.name == crate::package::MANIFEST_PATH) else { return (gone, None) };
    let Ok(mut root) = parse_manifest(manifest.bytes, limits, work) else { return (gone, None) };
    let mut scopes = Scopes::default();
    scopes.enter_all(&root);
    let rows = root.children.len();
    root.children.retain(|node| {
        let Node::El(row) = node else { return true };
        if scopes.element(&row.name) != (MANIFEST, "file-entry") {
            return true;
        }
        let path = row.attrs.iter().find(|(k, _)| scopes.attribute(k) == (MANIFEST, "full-path")).map(|(_, v)| v.as_str());
        !path.is_some_and(|p| gone.contains(&p))
    });
    add(&mut work.canon_steps, rows);
    let mut out = Vec::new();
    match write_element(&mut out, &root) {
        Ok(()) => (gone, Some(out)),
        Err(_) => (gone, None),
    }
}

/// The manifest rows of each side that counts as nothing, and each side's manifest written again without them (`None` for a side with none to leave out, or a manifest that
/// does not parse: it is then compared as it is, and every row is a difference, never a false same). A row is left out of a side's manifest when **the other side's manifest has no row
/// of its path** and either:
/// - it names an **entry that only this side holds**, a file or a folder: the row says what the entry says, and the entry is the difference (the thumbnail the writer wrote, the file of
///   DiaCraft's own data); or
/// - it names a **folder that files of this side stand under** and is **a plain one**: it states no media type (the row of a folder says none), or it is the row of DiaCraft's own folder
///   with the media type the writer gives it (`DiaCraft/`: it has none of its own to say). A folder row with a media type that names a kind (an embedded object's folder), a folder no
///   file stands under, a row for a path that is no entry, and a row for an entry both sides hold stay: each says more than the files do.
///
/// A row both manifests have, however it differs, is compared as every row is.
fn derived_rows(a: &[Entry<'_>], b: &[Entry<'_>], limits: &Limits, work: &mut Work) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let (Some(ma), Some(mb)) =
        (a.iter().find(|e| e.name == crate::package::MANIFEST_PATH), b.iter().find(|e| e.name == crate::package::MANIFEST_PATH))
    else {
        return (None, None);
    };
    let (Ok(mut ra), Ok(mut rb)) = (parse_manifest(ma.bytes, limits, work), parse_manifest(mb.bytes, limits, work)) else {
        return (None, None);
    };
    let (side_a, side_b) = (Side::of(&ra, a), Side::of(&rb, b));
    add(&mut work.canon_steps, side_a.rows.len().saturating_add(side_b.rows.len()).saturating_add(a.len()).saturating_add(b.len()));
    // The rows of DiaCraft's own entries and folder that only the written file (the second) lists are no difference either.
    let mut gone_b = side_b.derived(&side_a);
    gone_b.extend(side_b.rows.keys().filter(|path| is_carrier(path) && !side_a.rows.contains_key(path.as_str())).cloned());
    let gone_a = side_a.derived(&side_b);
    (rewritten(&mut ra, &gone_a, work), rewritten(&mut rb, &gone_b, work))
}

/// One side's manifest rows (each full path, and the media type it states, `None` for none) and the names of its entries and the folders they stand under.
struct Side<'e> {
    rows: HashMap<String, Option<String>>,
    names: HashSet<&'e str>,
    folders: HashSet<&'e str>,
}

impl<'e> Side<'e> {
    fn of(root: &El, entries: &[Entry<'e>]) -> Side<'e> {
        Side { rows: rows_of(root), names: entries.iter().map(|e| e.name).collect(), folders: folders_of(entries) }
    }

    /// The paths of the rows of this side that name nothing ([`derived_rows`]), given the other side.
    fn derived(&self, other: &Side<'_>) -> HashSet<String> {
        let plain = |path: &str, media: &Option<String>| {
            path.ends_with('/')
                && self.folders.contains(path)
                && match media {
                    None => true,
                    Some(media) => media.is_empty(),
                }
        };
        self.rows
            .iter()
            .filter(|(path, _)| !other.rows.contains_key(path.as_str()))
            .filter(|(path, media)| (self.names.contains(path.as_str()) && !other.names.contains(path.as_str())) || plain(path, media))
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// The rows of a manifest: each full path and the media type it states (`None` when it states none), in the manifest's namespace, whatever prefix binds it. A path two rows hold is held
/// once, and the first row's type is the one: the package layer reads the first.
fn rows_of(root: &El) -> HashMap<String, Option<String>> {
    let mut scopes = Scopes::default();
    scopes.enter_all(root);
    let mut rows = HashMap::new();
    for node in &root.children {
        let Node::El(row) = node else { continue };
        if scopes.element(&row.name) != (MANIFEST, "file-entry") {
            continue;
        }
        let attr = |local: &str| row.attrs.iter().find(|(k, _)| scopes.attribute(k) == (MANIFEST, local)).map(|(_, v)| v.clone());
        if let Some(path) = attr("full-path") {
            rows.entry(path).or_insert_with(|| attr("media-type"));
        }
    }
    rows
}

/// The folders that an entry of `entries` stands under, each with its trailing slash (`Pictures/` for `Pictures/a.png`).
fn folders_of<'p>(entries: &[Entry<'p>]) -> HashSet<&'p str> {
    let mut folders = HashSet::new();
    for entry in entries {
        for (at, _) in entry.name.match_indices('/') {
            // A folder is one that the entry stands under: the entry itself, a folder's own entry, is not.
            if at + 1 < entry.name.len()
                && let Some(folder) = entry.name.get(..=at)
            {
                folders.insert(folder);
            }
        }
    }
    folders
}

/// The manifest `root` written again without the rows of the paths `gone`, or `None` when there are none to leave out or it cannot be written.
fn rewritten(root: &mut El, gone: &HashSet<String>, work: &mut Work) -> Option<Vec<u8>> {
    if gone.is_empty() {
        return None;
    }
    let mut scopes = Scopes::default();
    scopes.enter_all(root);
    let rows = root.children.len();
    root.children.retain(|node| {
        let Node::El(row) = node else { return true };
        if scopes.element(&row.name) != (MANIFEST, "file-entry") {
            return true;
        }
        let path = row.attrs.iter().find(|(k, _)| scopes.attribute(k) == (MANIFEST, "full-path")).map(|(_, v)| v.as_str());
        !path.is_some_and(|p| gone.contains(p))
    });
    add(&mut work.canon_steps, rows);
    let mut out = Vec::new();
    write_element(&mut out, root).ok().map(|()| out)
}

/// Whether an entry or a row is DiaCraft's own: under the folder `DiaCraft/` (the folder itself included), compared ASCII case-insensitively as the reader compares it.
fn is_carrier(path: &str) -> bool {
    path.get(..crate::ext::items::FOLDER.len()).is_some_and(|head| head.eq_ignore_ascii_case(crate::ext::items::FOLDER))
}

/// The entries `second` without those of DiaCraft's own (under `DiaCraft/`) that `first` has no entry of the name of (what a writer adds of its own counts as nothing, and
/// what the source held is compared as written, so that an entry the source had and the written file lacks is a missing part).
fn without_extra_carrier<'p>(first: &[Entry<'p>], second: Vec<Entry<'p>>) -> Vec<Entry<'p>> {
    let held: HashSet<&str> = first.iter().filter(|e| is_carrier(e.name)).map(|e| e.name).collect();
    second.into_iter().filter(|e| !is_carrier(e.name) || held.contains(e.name)).collect()
}

/// `entries` without the parts `gone`, its manifest's bytes `manifest` when there are such (see [`empty_parts`]).
fn without<'p>(entries: Vec<Entry<'p>>, gone: &[&str], manifest: Option<&'p [u8]>) -> Vec<Entry<'p>> {
    entries
        .into_iter()
        .filter(|e| !gone.contains(&e.name))
        .map(|e| match manifest {
            Some(bytes) if e.name == crate::package::MANIFEST_PATH => Entry { bytes, ..e },
            _ => e,
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------------- the flat document

/// The children of `office:document`, in the order of the standard, then the rank of everything else.
const R30: [&str; 8] = ["meta", "settings", "scripts", "font-face-decls", "styles", "automatic-styles", "master-styles", "body"];
const FONTS: usize = 3;
const AUTOMATIC: usize = 5;
/// The rank of a child that is no child of R30: kept, in the order it stands, after the eight.
const REST: usize = 8;

/// The part of a package each of the children comes from: the parts that may hold it, by their name.
fn rank_in_part(part: &str, local: &str) -> Option<usize> {
    let rank = R30.iter().position(|n| *n == local)?;
    let holds = matches!(
        (part, rank),
        ("meta.xml", 0) | ("settings.xml", 1) | ("content.xml", 2 | 7) | ("styles.xml", 4 | 6) | ("content.xml" | "styles.xml", FONTS | AUTOMATIC)
    );
    holds.then_some(rank)
}

/// The namespace bindings in force as a walk goes down a tree: each prefix (`""` for the default) to the URIs bound to it, the innermost last.
#[derive(Default)]
struct Scopes {
    bound: HashMap<String, Vec<String>>,
}

/// The namespaces the check tells names by (the office, style, drawing and link ones): a binding of a prefix to any other is of no use to it, and is not kept unless it
/// shadows a binding that is, so that a root of 100,000 declarations costs nothing to enter (every declaration was copied into owned strings, twice).
/// A name in another namespace reads as one in none, which no comparison here takes for any of these.
fn is_told_by(uri: &str) -> bool {
    matches!(uri, OFFICE | STYLE | DRAW | XLINK)
}

impl Scopes {
    /// Declares what `el` binds that matters (see [`is_told_by`]), and gives the prefixes it pushed, for [`Scopes::leave`].
    fn enter(&mut self, el: &El) -> Vec<String> {
        let mut pushed = Vec::new();
        for (key, value) in &el.attrs {
            let prefix = if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") };
            if let Some(prefix) = prefix
                && (is_told_by(value) || self.bound.contains_key(prefix))
            {
                self.bound.entry(prefix.to_owned()).or_default().push(value.clone());
                pushed.push(prefix.to_owned());
            }
        }
        pushed
    }

    /// Declares every binding of `el`, whatever the namespace (the manifest's walk, which tells a row by the manifest's namespace).
    fn enter_all(&mut self, el: &El) {
        for (key, value) in &el.attrs {
            let prefix = if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") };
            if let Some(prefix) = prefix {
                self.bound.entry(prefix.to_owned()).or_default().push(value.clone());
            }
        }
    }

    fn leave(&mut self, pushed: Vec<String>) {
        for prefix in pushed {
            if let Some(stack) = self.bound.get_mut(&prefix) {
                stack.pop();
            }
        }
    }

    fn uri(&self, prefix: &str) -> &str {
        self.bound.get(prefix).and_then(|stack| stack.last()).map_or("", String::as_str)
    }

    /// The namespace URI (`""` for none) and local name of an element written `name`.
    fn element<'n>(&self, name: &'n str) -> (&str, &'n str) {
        match name.split_once(':') {
            Some((prefix, local)) => (self.uri(prefix), local),
            None => (self.uri(""), name),
        }
    }

    /// The same for an attribute: one with no prefix is in no namespace.
    fn attribute<'n>(&self, name: &'n str) -> (&str, &'n str) {
        match name.split_once(':') {
            Some((prefix, local)) => (self.uri(prefix), local),
            None => ("", name),
        }
    }
}

fn is_declaration(key: &str) -> bool {
    key == "xmlns" || key.starts_with("xmlns:")
}

fn add(counter: &mut u64, n: usize) {
    *counter = counter.saturating_add(n as u64);
}

/// The rank of the root child `el`: the order of R30, when it is an office child of that name, else [`REST`].
fn rank_of(el: &El, scopes: &mut Scopes) -> usize {
    let pushed = scopes.enter(el);
    let (ns, local) = scopes.element(&el.name);
    let rank = if ns == OFFICE { R30.iter().position(|n| *n == local).unwrap_or(REST) } else { REST };
    scopes.leave(pushed);
    rank
}

/// The nodes that fall in each rank, in order (nine lists, the last [`REST`]).
fn buckets() -> Vec<Vec<Node>> {
    (0..=REST).map(|_| Vec::new()).collect()
}

fn put(buckets: &mut [Vec<Node>], rank: usize, node: Node) {
    if let Some(bucket) = buckets.get_mut(rank) {
        bucket.push(node);
    }
}

/// What the check may add to one side beyond what the side holds, in bytes: a picture a part names, and a declaration put on a child that moves out of its
/// part's root. Charged before anything is made, so that a file that names one large picture a million times, or whose root binds a prefix to a long address
/// that a hundred thousand children use, is refused in words (`too_much_xml`) and not expanded. A quarter of the container's cap on one part (64 MiB by default).
struct Added {
    left: u64,
    max: u64,
}

impl Added {
    fn of(limits: &Limits) -> Added {
        let max = limits.container.max_part_bytes / 4;
        Added { left: max, max }
    }

    /// Charges `n` bytes.
    fn charge(&mut self, n: usize) -> Result<(), OdgError> {
        let n = n as u64;
        if n > self.left {
            return Err(OdgError::CompareCopies { max: self.max });
        }
        self.left = self.left.saturating_sub(n);
        Ok(())
    }
}

/// The package whose entries an `xlink:href` names (none for a flat file), and what the pictures may add.
struct Pictures<'p, 'a> {
    package: Option<&'p Package>,
    added: &'a mut Added,
}

impl Pictures<'_, '_> {
    fn charge(&mut self, n: usize) -> Result<(), OdgError> {
        self.added.charge(n)
    }
}

/// The entry an `href` names, with its bytes: percent-decoded with a leading `./` dropped, else as written (as the reader).
fn entry_named<'p>(package: &'p Package, href: &str) -> Option<(String, &'p [u8])> {
    let decoded = percent_decode(href);
    let stripped = decoded.strip_prefix("./").unwrap_or(&decoded);
    if let Some(bytes) = package.part(stripped) {
        return Some((stripped.to_owned(), bytes));
    }
    let plain = href.strip_prefix("./").unwrap_or(href);
    package.part(plain).map(|bytes| (plain.to_owned(), bytes))
}

/// The media type of a picture the way `write/convert.rs` writes it into a flat file: the manifest's row for the entry when it has a type, else the type the
/// bytes' own signature says (never the name's), else `application/octet-stream`.
fn media_of<'p>(package: &'p Package, name: &str, bytes: &[u8]) -> &'p str {
    let row = package.manifest().entry(name).and_then(|row| row.media_type.as_deref()).filter(|media| !media.is_empty());
    row.unwrap_or_else(|| crate::picturetype::picture_kind(bytes).map_or("application/octet-stream", |kind| kind.media))
}

/// `office:binary-data` as the picture's bytes: base64 read with white space skipped and written without line breaks. Text that is not base64 is left as
/// written (and then differs from any other text). What is charged is what the data decodes to: the white space in it is the file's own and costs nothing.
fn canonical_data(el: &mut El, pictures: &mut Pictures<'_, '_>, work: &mut Work) -> Result<(), OdgError> {
    let mut text = String::new();
    let mut read = 0usize;
    for node in &el.children {
        match node {
            Node::Text(t) | Node::CData(t) => {
                read = read.saturating_add(t.len());
                text.extend(t.chars().filter(|c| !c.is_ascii_whitespace()));
            }
            Node::Comment(_) => {}
            // Anything else in it is not a picture's data: left as it is.
            _ => return Ok(()),
        }
    }
    add(&mut work.canon_bytes, read);
    pictures.charge(text.len() / 4 * 3 + 3)?;
    let Some(bytes) = crate::ext::body::base64_decoded(&text) else { return Ok(()) };
    el.children = if bytes.is_empty() { Vec::new() } else { vec![Node::Text(crate::ext::body::base64_text(&bytes))] };
    Ok(())
}

/// A `draw:image` or `draw:fill-image` is its picture's bytes.
/// * In **a flat file** an `xlink:href` beside inline data is ignored: it is dropped.
/// * In **a package** the reader follows the href whatever the element holds (`read/shapes.rs`), so the href wins. One that names an entry of the package is replaced by
///   that entry as inline data, charged by the base64 it makes, and the data beside it (which the reader never looks at) is removed; and a `draw:image` that states no
///   `draw:mime-type` is given the one `write/convert.rs` writes for it ([`media_of`]), so that a flat file written by a producer of ours is the same (ruling Call 23: the
///   media type is content). One that names no entry (an address, a missing file) is kept as an attribute, with the data beside it, so that it differs from any flat
///   file's picture.
///
/// Gives the place of the data it made, which the walk does not go into again.
fn picture_element(
    el: &mut El,
    fill: bool,
    scopes: &mut Scopes,
    pictures: &mut Pictures<'_, '_>,
    work: &mut Work,
) -> Result<Option<usize>, OdgError> {
    let mut data_at: Vec<usize> = Vec::new();
    for (i, node) in el.children.iter().enumerate() {
        if let Node::El(child) = node {
            let pushed = scopes.enter(child);
            let (ns, local) = scopes.element(&child.name);
            if ns == OFFICE && local == "binary-data" {
                data_at.push(i);
            }
            scopes.leave(pushed);
        }
    }
    let at = el.attrs.iter().position(|(key, _)| !is_declaration(key) && scopes.attribute(key) == (XLINK, "href"));
    let Some(at) = at else { return Ok(None) };
    let Some(package) = pictures.package else {
        if !data_at.is_empty() {
            el.attrs.remove(at);
        }
        return Ok(None);
    };
    let Some((name, bytes)) = el.attrs.get(at).and_then(|(_, href)| entry_named(package, href)) else { return Ok(None) };
    pictures.charge(bytes.len().div_ceil(3).saturating_mul(4))?;
    add(&mut work.canon_bytes, bytes.len());
    let states_media = el.attrs.iter().any(|(key, _)| !is_declaration(key) && key.split_once(':').is_some_and(|(_, local)| local == "mime-type"));
    let media = (!fill && !states_media).then(|| media_of(package, &name, bytes));
    // The element's own prefix says the attribute's namespace (it is the drawing's); an element in the default namespace declares one for the purpose.
    let own_prefix = el.name.split_once(':').map(|(prefix, _)| prefix.to_owned());
    if let Some(media) = media {
        let key = own_prefix.as_ref().map_or_else(|| "canon-draw:mime-type".to_owned(), |prefix| format!("{prefix}:mime-type"));
        let declares = if own_prefix.is_none() { "xmlns:canon-draw".len() + DRAW.len() } else { 0 };
        pictures.charge(key.len().saturating_add(media.len()).saturating_add(declares).saturating_add(COPY_COST))?;
        if own_prefix.is_none() {
            el.attrs.push(("xmlns:canon-draw".to_owned(), DRAW.to_owned()));
        }
        el.attrs.push((key, media.to_owned()));
    }
    let data = El {
        name: "office:binary-data".to_owned(),
        attrs: vec![("xmlns:office".to_owned(), OFFICE.to_owned())],
        children: if bytes.is_empty() { Vec::new() } else { vec![Node::Text(crate::ext::body::base64_text(bytes))] },
    };
    // The attribute was found before the ones above were added, and nothing before it moves.
    el.attrs.remove(at);
    let mut index = 0usize;
    el.children.retain(|_| {
        let keep = data_at.binary_search(&index).is_err();
        index += 1;
        keep
    });
    el.children.insert(0, Node::El(data));
    Ok(Some(0))
}

/// The walk that puts every picture in its canonical form, one pass, recursing no deeper than the parser's ceiling.
fn pictures_of(el: &mut El, depth: usize, scopes: &mut Scopes, pictures: &mut Pictures<'_, '_>, work: &mut Work) -> Result<(), OdgError> {
    if depth > XML_DEPTH_CEILING {
        return Err(limit_error(XmlError::TooDeep { limit: XML_DEPTH_CEILING }));
    }
    add(&mut work.canon_steps, 1 + el.attrs.len());
    let pushed = scopes.enter(el);
    let result = pictures_below(el, depth, scopes, pictures, work);
    scopes.leave(pushed);
    result
}

fn pictures_below(el: &mut El, depth: usize, scopes: &mut Scopes, pictures: &mut Pictures<'_, '_>, work: &mut Work) -> Result<(), OdgError> {
    let (ns, local) = scopes.element(&el.name);
    let (is_data, is_image, fill) =
        (ns == OFFICE && local == "binary-data", ns == DRAW && (local == "image" || local == "fill-image"), local == "fill-image");
    if is_data {
        return canonical_data(el, pictures, work);
    }
    // An image first: what it holds that its href replaces is gone before the walk goes into it.
    let made = if is_image { picture_element(el, fill, scopes, pictures, work)? } else { None };
    for (i, node) in el.children.iter_mut().enumerate() {
        if made == Some(i) {
            continue;
        }
        if let Node::El(child) = node {
            pictures_of(child, depth + 1, scopes, pictures, work)?;
        }
    }
    Ok(())
}

/// The root of a flat file, its children in the order.
fn flat_document(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<El, OdgError> {
    // The reader's own caps for one part of one XML document, before anything is parsed.
    let cap = limits.container.max_part_bytes;
    if bytes.len() as u64 > cap {
        return Err(OdgError::Container(ContainerError::PartTooLarge { name: DOCUMENT.to_owned(), limit: cap }));
    }
    if bytes.len() as u64 > limits.max_xml_bytes {
        return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
    }
    let mut xml_limits = limits.container.xml.clone();
    xml_limits.max_nodes = most_items(limits);
    let mut root = xml::parse_counted(bytes, &xml_limits, work).map_err(|error| OdgError::Xml { part: DOCUMENT.to_owned(), error })?;
    let mut scopes = Scopes::default();
    scopes.enter(&root);
    if scopes.element(&root.name) != (OFFICE, "document") {
        return Err(OdgError::UnexpectedFlatRoot { root: crate::report::shown(&root.name) });
    }
    // A flat file has no place for the markers of a package's joined root: its own root attribute that starts like one is renamed, so that it can never be one.
    for (key, _) in &mut root.attrs {
        if starts_like_a_marker(key) {
            key.insert_str(0, OWN_ATTRIBUTE_PREFIX);
        }
    }
    let mut ranked = buckets();
    let mut taken = [false; REST];
    for mut node in std::mem::take(&mut root.children) {
        let rank = match &node {
            Node::El(child) => rank_of(child, &mut scopes),
            _ => REST,
        };
        // The first of each of the children is that child; a repeat is kept after them, never merged, as a package's is. (`taken` has a place for each
        // of the eight and none for the rest.)
        let rank = match taken.get_mut(rank) {
            Some(seen) if !*seen => {
                *seen = true;
                rank
            }
            _ => REST,
        };
        // The one container of faces holds each face once, as the package side's joined one does.
        if rank == FONTS
            && let Node::El(container) = &mut node
        {
            drop_repeated_faces(container);
        }
        put(&mut ranked, rank, node);
    }
    add(&mut work.canon_steps, ranked.iter().map(Vec::len).sum());
    root.children = ranked.into_iter().flatten().collect();
    Ok(root)
}

/// The prefixes the names in `el`'s tree use (`""` for an element with no prefix), which is what decides what its declarations mean to it, and how many
/// elements and attributes were looked at (for the cost count). One pass over the tree, on an explicit stack.
fn prefixes_used(el: &El) -> (HashSet<&str>, usize) {
    let mut used = HashSet::new();
    let mut looked = 0usize;
    let mut stack = vec![el];
    while let Some(el) = stack.pop() {
        looked = looked.saturating_add(1).saturating_add(el.attrs.len());
        used.insert(match el.name.split_once(':') {
            Some((prefix, _)) => prefix,
            None => "",
        });
        for (key, _) in el.attrs.iter().filter(|(k, _)| !is_declaration(k)) {
            if let Some((prefix, _)) = key.split_once(':') {
                used.insert(prefix);
            }
        }
        for node in &el.children {
            if let Node::El(child) = node {
                stack.push(child);
            }
        }
    }
    (used, looked)
}

/// The prefix a declaration key binds (`""` for `xmlns`), or none for a key that is no declaration.
fn declared_prefix(key: &str) -> Option<&str> {
    if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") }
}

/// The declarations in `attrs`, by the prefix each binds (the last of a prefix bound twice cannot be: a parsed tree has none).
fn declarations(attrs: &[(String, String)]) -> HashMap<&str, (&str, &str)> {
    attrs.iter().filter_map(|(k, v)| declared_prefix(k).map(|p| (p, (k.as_str(), v.as_str())))).collect()
}

/// Puts on `child` the declarations in `decls` for the prefixes its tree uses and that it does not bind itself: what a child that leaves the root of its part
/// needs to mean what it meant there. Each copy is charged (its key, its address and a fixed cost for the attribute) before it is made, and the passes are
/// counted, so the cost is the names in the tree and no more: never the child's count times the root's.
fn carry(child: &mut El, decls: &HashMap<&str, (&str, &str)>, added: &mut Added, work: &mut Work) -> Result<(), OdgError> {
    if decls.is_empty() {
        return Ok(());
    }
    let mut copies: Vec<(&str, &str)> = Vec::new();
    {
        let (used, looked) = prefixes_used(child);
        add(&mut work.canon_steps, looked.saturating_add(used.len()));
        let own: HashSet<&str> = child.attrs.iter().filter_map(|(k, _)| declared_prefix(k)).collect();
        for prefix in used {
            if let Some(&(key, value)) = decls.get(prefix).filter(|_| !own.contains(prefix)) {
                added.charge(key.len().saturating_add(value.len()).saturating_add(COPY_COST))?;
                add(&mut work.canon_bytes, key.len().saturating_add(value.len()));
                copies.push((key, value));
            }
        }
    }
    for (key, value) in copies {
        child.attrs.push((key.to_owned(), value.to_owned()));
    }
    Ok(())
}

/// What one copied declaration costs besides its text: the attribute's own place in the tree.
const COPY_COST: usize = 64;

/// How many items a parsed tree holds, as the parser counts them against its limit: each element, attribute and child that is not an element.
fn items_of(root: &El) -> usize {
    let mut items = 0usize;
    let mut stack = vec![root];
    while let Some(el) = stack.pop() {
        items = items.saturating_add(1).saturating_add(el.attrs.len());
        for node in &el.children {
            match node {
                Node::El(child) => stack.push(child),
                _ => items = items.saturating_add(1),
            }
        }
    }
    items
}

/// The most bytes of faces compared for their sameness: past it the rest are kept as they are, which only reports more.
const MOST_FACE_BYTES: usize = 8 * 1024 * 1024;

/// The faces of one container seen so far, each as the text it is written as, up to [`MOST_FACE_BYTES`] of them.
#[derive(Default)]
struct Faces {
    known: HashSet<Vec<u8>>,
    spent: usize,
}

impl Faces {
    /// Whether `child` is written exactly as one seen before (the same face twice is one: a producer and our own converter declare a face in both parts), and
    /// remembers it when it is not.
    fn repeats(&mut self, child: &El) -> bool {
        let mut text = Vec::new();
        if write_element(&mut text, child).is_err() {
            return false;
        }
        if self.known.contains(&text) {
            return true;
        }
        if self.spent.saturating_add(text.len()) <= MOST_FACE_BYTES {
            self.spent += text.len();
            self.known.insert(text);
        }
        false
    }
}

/// A flat file's one container of faces holds each face once: a face written twice exactly is one.
fn drop_repeated_faces(container: &mut El) {
    let mut faces = Faces::default();
    container.children.retain(|node| match node {
        Node::El(child) => !faces.repeats(child),
        _ => true,
    });
}

/// What names a child of a container of styles or faces, as a number made of its element, its `style:family` (none for a face or a page layout) and its
/// `style:name`; `None` for a child with no name. The scopes are those of the container. Two children of one number are of one name, or (once in 2^64) look so: the
/// check reports a difference for such a pair, which is the safe side.
fn name_key(child: &El, scopes: &mut Scopes) -> Option<u64> {
    let pushed = scopes.enter(child);
    let (ns, local) = scopes.element(&child.name);
    let (mut name, mut family) = (None, "");
    for (key, value) in child.attrs.iter().filter(|(k, _)| !is_declaration(k)) {
        match scopes.attribute(key) {
            (STYLE, "name") => name = Some(value.as_str()),
            (STYLE, "family") => family = value.as_str(),
            _ => {}
        }
    }
    let key = name.map(|name| {
        let mut hasher = DefaultHasher::new();
        (ns, local, family, name).hash(&mut hasher);
        hasher.finish()
    });
    scopes.leave(pushed);
    key
}

/// What joining the pieces of one container made: the children of the joined container, and whether a later part holds a name that an earlier one holds.
struct Joined {
    nodes: Vec<Node>,
    clash: bool,
}

/// The children two parts hold under one name, joined into the one a flat file has: the first one's element, then the others' children in turn. A later one
/// keeps what its part's root bound: when it binds nothing the first does not bind the same, its declarations go on each of its children (so a prefix means
/// what it meant in its part), and when a plain attribute of it disagrees with the first's the two stay two elements, which is a difference from any flat file
/// and never a false same. With `faces`, a face that is written exactly as one before it, in the same context of declarations, is that one (a face declared in
/// both parts is one; so is a face written twice in one part).
///
/// **A name that both parts hold is a clash** (`clash` of the answer): a child of a later part with the `style:name` (and `style:family`) of a child of an earlier
/// part, a face that is not that face, an automatic style of the same name in both. The package reader gives the content its own part's, while a flat file's one
/// container is read first-come, and `write/convert.rs` refuses to join them (`names_are_disjoint`); no flat file is the same as such a package, and the caller
/// says so.
fn join(pieces: Vec<Node>, faces: bool, added: &mut Added, work: &mut Work) -> Result<Joined, OdgError> {
    let mut out: Vec<El> = Vec::new();
    let mut clash = false;
    let mut seen = Faces::default();
    // The names the first piece holds. (Two pieces reach here at the most, one for each of the two parts that may hold the container, so the names of a later piece
    // are compared with the first's alone.)
    let mut names: HashSet<u64> = HashSet::new();
    for piece in pieces {
        let Node::El(mut piece) = piece else { continue };
        add(&mut work.canon_steps, 1usize.saturating_add(piece.attrs.len()).saturating_add(piece.children.len()));
        let mut scopes = Scopes::default();
        scopes.enter(&piece);
        let Some(first) = out.first_mut() else {
            // The first piece: its names are the ones a later piece may repeat, and a face written twice in it is one.
            let mut kept = Vec::with_capacity(piece.children.len());
            for node in std::mem::take(&mut piece.children) {
                if let Node::El(child) = &node {
                    if faces && seen.repeats(child) {
                        continue;
                    }
                    add(&mut work.canon_steps, 1usize.saturating_add(child.attrs.len()));
                    names.extend(name_key(child, &mut scopes));
                }
                kept.push(node);
            }
            piece.children = kept;
            out.push(piece);
            continue;
        };
        // The attributes of the later container that are no declarations: those the first has must agree (looked up in a map, so a container of many attributes
        // costs its count), and those it lacks are added to it.
        let new_plain: Vec<(String, String)> = {
            let held: HashMap<&str, &str> = first.attrs.iter().filter(|(k, _)| !is_declaration(k)).map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let joinable = piece.attrs.iter().filter(|(k, _)| !is_declaration(k)).all(|(k, v)| held.get(k.as_str()).is_none_or(|have| have == v));
            if !joinable {
                out.push(piece);
                continue;
            }
            piece.attrs.iter().filter(|(k, _)| !is_declaration(k) && !held.contains_key(k.as_str())).cloned().collect()
        };
        first.attrs.extend(new_plain);
        let first_decls = declarations(&first.attrs);
        let later_decls = declarations(&piece.attrs);
        // The same context: every declaration the later part's root made the first's root made too.
        let same_context = later_decls.iter().all(|(prefix, (_, address))| first_decls.get(prefix).is_some_and(|(_, held)| held == address));
        for mut node in piece.children {
            if let Node::El(child) = &mut node {
                add(&mut work.canon_steps, 1usize.saturating_add(child.attrs.len()));
                let key = name_key(child, &mut scopes);
                // The same element in the same context of the prefixes it uses is the same face.
                let alike = same_context || {
                    let (used, looked) = prefixes_used(child);
                    add(&mut work.canon_steps, looked);
                    used.iter().all(|prefix| later_decls.get(prefix).map(|d| d.1) == first_decls.get(prefix).map(|d| d.1))
                };
                if faces && alike && seen.repeats(child) {
                    continue;
                }
                if let Some(key) = key {
                    clash |= names.contains(&key);
                }
                if !same_context {
                    carry(child, &later_decls, added, work)?;
                }
            }
            first.children.push(node);
        }
    }
    Ok(Joined { nodes: out.into_iter().map(Node::El).collect(), clash })
}

/// The document a package says: see the [module documentation](self).
fn package_document(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<El, OdgError> {
    /// The parts, `styles.xml` first (the order its shared children are joined in), each with the root it must have.
    const PARTS: [(&str, &str); 4] = [
        ("styles.xml", "document-styles"),
        ("content.xml", "document-content"),
        ("meta.xml", "document-meta"),
        ("settings.xml", "document-settings"),
    ];
    let limits = &halved(limits);
    let package = read_package(bytes, limits, work, false, Inflate::All)?;
    let mut ranked = buckets();
    let mut versions: Vec<String> = Vec::new();
    let mut leftovers: Vec<String> = Vec::new();
    let mut xml_bytes = 0u64;
    // One budget of items for the four parts together, as a flat file has for its one document: each part is parsed with what the parts before it left, so
    // that two parts each just under the cap on one part are refused (`xml_too_complex`) and the trees of the four are never all held.
    let mut nodes_left = most_items(limits);
    let mut added = Added::of(limits);
    for (part, root_local) in PARTS {
        let Some(data) = package.part(part) else { continue };
        xml_bytes = xml_bytes.saturating_add(data.len() as u64);
        if xml_bytes > limits.max_xml_bytes {
            return Err(OdgError::Limit { kind: LimitKind::XmlBytes, max: limits.max_xml_bytes });
        }
        let mut xml_limits = limits.container.xml.clone();
        xml_limits.max_nodes = nodes_left;
        let mut root = xml::parse_counted(data, &xml_limits, work).map_err(|error| OdgError::Xml { part: part.to_owned(), error })?;
        nodes_left = nodes_left.saturating_sub(items_of(&root));
        let mut scopes = Scopes::default();
        scopes.enter(&root);
        if scopes.element(&root.name) != (OFFICE, root_local) {
            return Err(OdgError::UnexpectedRoot { part });
        }
        let version = root.attrs.iter().find(|(k, _)| !is_declaration(k) && scopes.attribute(k) == (OFFICE, "version")).map(|(_, v)| v.clone());
        if let Some(version) = version.filter(|v| !versions.contains(v)) {
            versions.push(version);
        }
        // An attribute of the part's root other than its version and its declarations has no place on the joined root of a flat document: it goes there under a
        // name no flat file has, so that it is a difference and is not dropped unseen.
        for (key, value) in root.attrs.iter().filter(|(k, _)| !is_declaration(k) && scopes.attribute(k) != (OFFICE, "version")) {
            added.charge(part.len().saturating_add(key.len()).saturating_add(value.len()).saturating_add(COPY_COST))?;
            leftovers.push(format!("{part} {key}={value}"));
        }
        let decls = declarations(&root.attrs);
        let mut taken = [false; REST];
        for node in std::mem::take(&mut root.children) {
            let Node::El(mut child) = node else {
                // Text (blank between children, or not), CDATA and instructions of a part's root stay as they are; a comment is no content.
                if !matches!(node, Node::Comment(_)) {
                    put(&mut ranked, REST, node);
                }
                continue;
            };
            let pushed = scopes.enter(&child);
            let (ns, local) = scopes.element(&child.name);
            let place = if ns == OFFICE { rank_in_part(part, local) } else { None };
            scopes.leave(pushed);
            // The first of each name a part may hold is that child; a repeat is kept beside, never merged.
            let rank = match place.and_then(|r| taken.get_mut(r).map(|t| (r, t))) {
                Some((r, seen)) if !*seen => {
                    *seen = true;
                    r
                }
                _ => REST,
            };
            // What the child needs of its part's root to mean what it meant there: the declarations of the prefixes its tree uses.
            carry(&mut child, &decls, &mut added, work)?;
            put(&mut ranked, rank, Node::El(child));
        }
    }
    add(&mut work.canon_steps, ranked.iter().map(Vec::len).sum());
    let mut children = Vec::new();
    let mut clashes: Vec<&'static str> = Vec::new();
    for (rank, bucket) in ranked.into_iter().enumerate() {
        match rank {
            FONTS => {
                let joined = join(bucket, true, &mut added, work)?;
                if joined.clash {
                    clashes.push("font-face-decls");
                }
                children.extend(joined.nodes);
            }
            AUTOMATIC => {
                let joined = join(bucket, false, &mut added, work)?;
                if joined.clash {
                    clashes.push("automatic-styles");
                }
                children.extend(joined.nodes);
            }
            _ => children.extend(bucket),
        }
    }
    let mut attrs = vec![("xmlns:office".to_owned(), OFFICE.to_owned())];
    if !versions.is_empty() {
        attrs.push(("office:version".to_owned(), versions.join("|")));
    }
    if let Some(media) = package.media_type() {
        attrs.push(("office:mimetype".to_owned(), media.to_owned()));
    }
    for (i, text) in leftovers.into_iter().enumerate() {
        attrs.push((format!("{ROOT_ATTRIBUTE_MARKER}{i}"), text));
    }
    // A name both parts hold has no place in a flat file's one container: the root says so, in a name no flat file has, and the package differs from every flat file.
    for (i, local) in clashes.iter().enumerate() {
        attrs.push((format!("{NAME_CLASH_MARKER}{i}"), format!("{local}: a name of the later part is the first part's too")));
    }
    let mut root = El { name: DOCUMENT.to_owned(), attrs, children };
    let mut pictures = Pictures { package: Some(&package), added: &mut added };
    pictures_of(&mut root, 0, &mut Scopes::default(), &mut pictures, work)?;
    Ok(root)
}

/// The one flat document a side is, as bytes: a flat file as it is with its children in order, a package as its parts say.
fn flat_space(bytes: &[u8], form: Form, limits: &Limits, work: &mut Work) -> Result<Vec<u8>, OdgError> {
    let root = match form {
        Form::Flat => {
            let mut root = flat_document(bytes, limits, work)?;
            let mut added = Added::of(limits);
            let mut pictures = Pictures { package: None, added: &mut added };
            pictures_of(&mut root, 0, &mut Scopes::default(), &mut pictures, work)?;
            root
        }
        Form::Package => package_document(bytes, limits, work)?,
    };
    let out = xml::to_bytes(&root, &[], &[], false)
        .map_err(|e| OdgError::Xml { part: DOCUMENT.to_owned(), error: XmlError::Malformed { offset: 0, message: e.to_string() } })?;
    add(&mut work.canon_bytes, out.len());
    Ok(out)
}

fn compare_flat(a: &[u8], form_a: Form, b: &[u8], form_b: Form, limits: &Limits, work: &mut Work) -> Result<ContentDiff, OdgError> {
    let da = flat_space(a, form_a, limits, work)?;
    let db = flat_space(b, form_b, limits, work)?;
    let ea = [Entry { name: DOCUMENT, xml: true, bytes: &da }];
    let eb = [Entry { name: DOCUMENT, xml: true, bytes: &db }];
    compare_entries(&ea, &eb, false, &CANON_RULES, &limits.container.xml, work).map_err(canon_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_package_may_inflate_to_half_of_what_a_comparison_holds_and_no_more_than_its_own_limit() {
        let default = Limits::default();
        assert!(default.container.max_package_bytes > MAX_COMPARE_BYTES / 2, "the default package cap is above half of a comparison's bytes");
        assert_eq!(halved(&default).container.max_package_bytes, MAX_COMPARE_BYTES / 2);
        let mut small = Limits::default();
        small.container.max_package_bytes = 1_000;
        assert_eq!(halved(&small).container.max_package_bytes, 1_000, "a lower limit of the caller's stays");
        // Nothing else is changed.
        assert_eq!(halved(&default).container.max_part_bytes, default.container.max_part_bytes);
        assert_eq!(halved(&default).max_xml_bytes, default.max_xml_bytes);
    }

    #[test]
    fn a_tree_of_the_flat_path_holds_the_least_of_the_three_item_limits() {
        let mut limits = Limits::default();
        assert_eq!(most_items(&limits), MAX_COMPARE_ITEMS.min(limits.max_read_xml_nodes).min(limits.container.xml.max_nodes));
        limits.container.xml.max_nodes = 700;
        assert_eq!(most_items(&limits), 700);
        limits.container.xml.max_nodes = 9_000_000;
        limits.max_read_xml_nodes = 600;
        assert_eq!(most_items(&limits), 600);
        limits.max_read_xml_nodes = 9_000_000;
        assert_eq!(most_items(&limits), MAX_COMPARE_ITEMS);
    }

    #[test]
    fn what_the_container_refuses_is_said_as_this_crate_says_it() {
        let xml = XmlError::TooManyNodes { limit: 5 };
        match canon_error(CanonError::Xml { part: "content.xml".to_owned(), error: xml.clone() }) {
            OdgError::Xml { part, error } => assert_eq!((part.as_str(), error), ("content.xml", xml)),
            other => panic!("{other:?}"),
        }
        match canon_error(CanonError::TooLarge { bytes: 9, limit: 5 }) {
            OdgError::Limit { kind, max } => assert_eq!((kind, max), (LimitKind::XmlBytes, 5)),
            other => panic!("{other:?}"),
        }
        match canon_error(CanonError::Values { part: "styles.xml".to_owned(), limit: 7 }) {
            OdgError::Xml { part, error } => assert_eq!((part.as_str(), error), ("styles.xml", XmlError::AttributeTooLarge { limit: 7 })),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_picture_walk_goes_no_deeper_than_the_parsers_ceiling() {
        let chain = |levels: usize| {
            let mut el = El { name: "leaf".to_owned(), attrs: Vec::new(), children: Vec::new() };
            for _ in 1..levels {
                el = El { name: "x".to_owned(), attrs: Vec::new(), children: vec![Node::El(el)] };
            }
            el
        };
        let walk = |levels: usize| {
            let (mut added, mut work) = (Added { left: 1_000, max: 1_000 }, Work::default());
            let mut pictures = Pictures { package: None, added: &mut added };
            pictures_of(&mut chain(levels), 0, &mut Scopes::default(), &mut pictures, &mut work)
        };
        assert!(walk(XML_DEPTH_CEILING + 1).is_ok(), "the deepest element of 513 levels is at depth 512, which is the ceiling");
        assert!(matches!(walk(XML_DEPTH_CEILING + 2), Err(OdgError::Xml { error: XmlError::TooDeep { limit: XML_DEPTH_CEILING }, .. })));
    }

    #[test]
    fn what_a_media_type_says_is_xml() {
        for xml in [
            "text/xml",
            "application/xml",
            "application/rdf+xml",
            "TEXT/XML",
            " text/xml ; charset=utf-8",
            "application/vnd.oasis.opendocument.graphics+xml",
        ] {
            assert!(media_is_xml(xml), "{xml}");
        }
        for other in ["image/svg+xml", "IMAGE/SVG+XML", "image/png", "application/octet-stream", "", "text/plain", "application/xmlx"] {
            assert!(!media_is_xml(other), "{other}");
        }
    }

    #[test]
    fn a_prefix_means_what_the_nearest_binding_says() {
        let mut scopes = Scopes::default();
        let outer = El {
            name: "a".to_owned(),
            attrs: vec![("xmlns:p".to_owned(), OFFICE.to_owned()), ("xmlns".to_owned(), STYLE.to_owned())],
            children: Vec::new(),
        };
        let inner = El { name: "b".to_owned(), attrs: vec![("xmlns:p".to_owned(), DRAW.to_owned())], children: Vec::new() };
        let a = scopes.enter(&outer);
        assert_eq!(scopes.element("p:x"), (OFFICE, "x"));
        assert_eq!(scopes.element("x"), (STYLE, "x"));
        assert_eq!(scopes.attribute("x"), ("", "x"));
        let b = scopes.enter(&inner);
        assert_eq!(scopes.element("p:x"), (DRAW, "x"));
        scopes.leave(b);
        assert_eq!(scopes.element("p:x"), (OFFICE, "x"));
        scopes.leave(a);
        assert_eq!(scopes.element("p:x"), ("", "x"));
    }

    #[test]
    fn a_binding_to_a_namespace_nothing_here_tells_names_by_is_not_kept_unless_it_shadows_one_that_is() {
        let mut scopes = Scopes::default();
        let many: Vec<(String, String)> = (0..1_000).map(|i| (format!("xmlns:p{i}"), format!("urn:p{i}"))).collect();
        let root = El { name: "r".to_owned(), attrs: [many, vec![("xmlns:d".to_owned(), DRAW.to_owned())]].concat(), children: Vec::new() };
        let pushed = scopes.enter(&root);
        assert_eq!(pushed, ["d"], "only the one that matters is kept");
        assert_eq!(scopes.element("p7:x"), ("", "x"), "a name in another namespace reads as one in none");
        assert_eq!(scopes.element("d:x"), (DRAW, "x"));
        // `d` rebound to a namespace that is of no use still shadows the drawing's, and is left again.
        let shadow = El { name: "s".to_owned(), attrs: vec![("xmlns:d".to_owned(), "urn:other".to_owned())], children: Vec::new() };
        let inner = scopes.enter(&shadow);
        assert_eq!(scopes.element("d:x"), ("urn:other", "x"));
        scopes.leave(inner);
        assert_eq!(scopes.element("d:x"), (DRAW, "x"));
        scopes.leave(pushed);
        assert_eq!(scopes.element("d:x"), ("", "x"));
    }

    #[test]
    fn the_length_list_names_each_attribute_under_both_uris_and_no_count_or_name() {
        assert_eq!(
            LENGTH_ATTRIBUTES.len(),
            70,
            "29 attributes: 14 of `svg` and 14 of `fo` under two URIs each, and two of `draw` for the markers; 12 more of the percentages and the dash's lengths"
        );
        let mut seen = HashSet::new();
        for n in LENGTH_ATTRIBUTES {
            assert!(seen.insert((n.ns, n.local)), "{} {} twice", n.ns, n.local);
            let twin = match n.ns {
                FO => Some(FO_W3C),
                FO_W3C => Some(FO),
                SVG => Some(SVG_W3C),
                SVG_W3C => Some(SVG),
                DRAW | STYLE => None,
                other => panic!("{other} is no namespace of a length attribute"),
            };
            if let Some(twin) = twin {
                assert!(LENGTH_ATTRIBUTES.iter().any(|m| m.ns == twin && m.local == n.local), "{} has no twin under {twin}", n.local);
            }
        }
        // Names, ids, counts and indexes are never lengths: `007` is not `7`.
        for text in
            ["c", "name", "id", "z-index", "level", "corners", "dots1", "start-angle", "end-angle", "viewBox", "d", "points", "transform", "padding"]
        {
            assert!(LENGTH_ATTRIBUTES.iter().all(|n| n.local != text), "{text} is not a length");
        }
        // The percentages of the notes are on it (a percentage is a decimal number and `%`: `75%` is `75.0%`), but for a stroke's opacity, which is a number or a
        // percentage and is read as a number.
        for text in [
            "opacity",
            "shadow-opacity",
            "text-scale",
            "text-position",
            "start-intensity",
            "end-intensity",
            "border",
            "cx",
            "cy",
            "dots1-length",
            "distance",
        ] {
            assert!(LENGTH_ATTRIBUTES.iter().any(|n| n.local == text), "{text} is a percentage");
        }
        assert!(LENGTH_ATTRIBUTES.iter().all(|n| n.local != "stroke-opacity"));
        assert_eq!(NUMBER_ATTRIBUTES.iter().map(|n| n.local).collect::<Vec<_>>(), ["stroke-opacity", "stroke-opacity"]);
        assert!(NUMBER_ATTRIBUTES.iter().all(|n| !LENGTH_ATTRIBUTES.contains(n)), "an attribute is read one way");
    }

    #[test]
    fn the_paragraph_and_mixed_content_lists_are_disjoint_and_are_the_rules() {
        assert_eq!(PARAGRAPH_ELEMENTS.iter().map(|n| n.local).collect::<Vec<_>>(), ["p", "h", "span"]);
        assert!(PARAGRAPH_ELEMENTS.iter().all(|n| n.ns == ODF_TEXT_NAMESPACE));
        // An element is a paragraph or it is mixed content that is none: a paragraph's white space is ODF's rule, and the rest's is exact.
        assert!(EXACT_INLINE.iter().all(|n| n.ns == ODF_TEXT_NAMESPACE && !PARAGRAPH_ELEMENTS.iter().any(|p| p == n)));
        assert_eq!(CANON_RULES.paragraphs, PARAGRAPH_ELEMENTS);
        assert_eq!(CANON_RULES.exact_text, EXACT_INLINE);
        assert_eq!(CANON_RULES.lengths, LENGTH_ATTRIBUTES);
        // Lengths are read as ODF reads them; the looser numbers are the stroke's opacity alone.
        assert_eq!(CANON_RULES.numbers, NUMBER_ATTRIBUTES);
    }

    #[test]
    fn a_flat_roots_own_attribute_is_renamed_when_it_starts_like_a_marker_or_like_a_rename_and_in_no_other_case() {
        // The markers a package's joined root has, and the names a rename is given, start as the renaming looks for.
        assert!(ROOT_ATTRIBUTE_MARKER.starts_with(MARKER_START) && NAME_CLASH_MARKER.starts_with(MARKER_START));
        assert!(OWN_ATTRIBUTE_PREFIX.starts_with("flat-"));
        for key in [
            "part-root-attribute-0",
            "part-name-clash-0",
            "part-root-attribute-17",
            "part-x",
            "part-",
            "flat-x",
            "flat-part-x",
            "flat-",
            "flat-part-root-attribute-0",
        ] {
            assert!(starts_like_a_marker(key), "{key}");
        }
        for key in [
            "",
            "part",
            "flat",
            "x",
            "xmlns",
            "xmlns:part-x",
            "Part-x",
            "FLAT-x",
            "a-part-x",
            "office:version",
            "part-x:y",
            "flat-x:y",
            "part-ns:thing",
            ":part-x",
        ] {
            assert!(!starts_like_a_marker(key), "{key}");
        }
        // The renaming is one-to-one over any names: what is renamed starts `flat-`, so it is never a name that is not renamed or a marker, and two names that are
        // renamed stay two.
        let renamed = |key: &str| if starts_like_a_marker(key) { format!("{OWN_ATTRIBUTE_PREFIX}{key}") } else { key.to_owned() };
        let names = [
            "part-x",
            "flat-part-x",
            "flat-flat-part-x",
            "flat-x",
            "flat-flat-x",
            "x",
            "part-",
            "flat-",
            "part-root-attribute-0",
            "flat-part-root-attribute-0",
        ];
        let mut seen = HashSet::new();
        for key in names {
            let name = renamed(key);
            assert!(seen.insert(name.clone()), "{key} became {name}, which another name became too");
            assert!(!name.starts_with(MARKER_START), "{key} became {name}, which could be a marker");
        }
    }

    #[test]
    fn each_child_of_has_one_place_in_a_package() {
        assert_eq!(rank_in_part("meta.xml", "meta"), Some(0));
        assert_eq!(rank_in_part("settings.xml", "settings"), Some(1));
        assert_eq!(rank_in_part("content.xml", "scripts"), Some(2));
        assert_eq!(rank_in_part("content.xml", "body"), Some(7));
        assert_eq!(rank_in_part("styles.xml", "styles"), Some(4));
        assert_eq!(rank_in_part("styles.xml", "master-styles"), Some(6));
        assert_eq!(rank_in_part("styles.xml", "automatic-styles"), Some(AUTOMATIC));
        assert_eq!(rank_in_part("content.xml", "font-face-decls"), Some(FONTS));
        // A child in the part of another kind is no child of that part.
        assert_eq!(rank_in_part("content.xml", "styles"), None);
        assert_eq!(rank_in_part("styles.xml", "body"), None);
        assert_eq!(rank_in_part("meta.xml", "body"), None);
        assert_eq!(rank_in_part("content.xml", "other"), None);
    }
}
