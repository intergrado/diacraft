//! The content check: what "the same content" means for two packages, as one comparison that a format's crate configures with its lists
//! (the native format's design, section 4.3).
//!
//! The question it answers is whether two packages say the same thing, not whether they are the same bytes. A program that writes a
//! package again may change what has no meaning in the format (the spacing of its XML, the order of its attributes, the prefixes it
//! gives its namespaces, how it compresses an entry) and must change nothing else. **The check never calls two different contents the
//! same**: every rule below that lets two spellings pass has a planted difference in `tests/it/canon.rs` that it still catches, and a
//! rule that cannot be shown to keep one is not here.
//!
//! # The rules
//!
//! **Package level** ([`compare_entries`]):
//! * The same set of part names, folded by ASCII case when the format says so (OPC part names), exactly when it does not (OpenDocument
//!   entry names). A name on one side only is [`Found::MissingPart`] or [`Found::ExtraPart`].
//! * A part the caller says is XML is compared by the XML rules when it parses on both sides; every other part, and one meant as XML
//!   that does not parse on either side, is compared byte for byte ([`Found::Bytes`]). A part too large or too deep for the XML limits is
//!   an error, not a verdict: the check cannot say anything about it.
//! * The order of the entries is ignored, and so is everything about how they are stored: that is not in the entries a caller gives.
//! * A part on one side only that is of a kind [`Rules::parts_absent_when_empty`] names (an [`EmptyPart`]: OPC's relationships parts, by their name), whose
//!   root is that kind's and holds nothing, is the same as no part. Any other part is a part whatever its root holds, and so is one that holds anything,
//!   does not parse or is larger than [`MAX_EMPTY_PART_BYTES`]: the missing or extra part it is.
//!
//! **XML parts**:
//! * Ignored: the XML declaration, anything outside the root element, comments anywhere (so that `a<!--x-->b` is `ab`), and every
//!   namespace declaration. Compared: processing instructions inside the root, because a program may act on them.
//! * Names are compared as (namespace URI, local name). An unprefixed element is in the default namespace and an unprefixed attribute
//!   in none. A prefix nothing binds is itself the name: `a:x` is not `b:x`, and is not the `x` of any namespace. **The nearest declaration
//!   decides, as XML Namespaces 1.0 says, wherever a rule reads a name** (an element, a child a rule lists, the root of an empty part and its
//!   children, the container of the free names): `xmlns=""` puts the element and its unprefixed descendants in no namespace until a
//!   declaration binds the default again (an element's own `xmlns=""` was read, where a rule read a child's name, as no declaration).
//! * **A part that breaks a constraint of XML Namespaces 1.0 is compared as bytes**, as a part that does not parse is (`breaks_namespaces`): a
//!   prefix declared empty, `xml` bound to another namespace or its namespace to another prefix or as the default, `xmlns` declared or its
//!   namespace bound, an element of the prefix `xmlns`, and a name that is no qualified name (`:x`, `x:`, `a:b:c`). The readers of the formats
//!   read each of these each its own way, so no reading of the check could be the same as all of them; such a part is the same content as
//!   another only when it has the same bytes. A prefix that nothing binds is not one of these: it is the name as written (above).
//! * Attributes are a set, by their names. Two that name one thing under different prefixes (which XML Namespaces 1.0 does not allow, and
//!   the parser does not see) are compared **in the order written**: a reader takes the first, so another order is another reading
//!   (they were compared as a list sorted by value, and two documents that swapped the values were the same).
//! * Attribute values are text, as the parser gives them. **The exception is a number**, and only in an attribute [`Rules::numbers`]
//!   lists: two values that each read whole as a decimal number and a unit are equal when the numbers are the same `f64` and the units the
//!   same ([`same_number`]). Anywhere else `007` and `7` differ.
//! * Text is compared after CDATA, references and entities are resolved, and adjacent runs (across a comment) are joined. Blank text
//!   is ignored only in an element that has a child (an element or an instruction) and no text that is not blank, and that
//!   [`Rules::exact_text`] does not name; `xml:space` is not read (every part of a package of one format may carry it on its root).
//! * In an element [`Rules::paragraphs`] names, white space follows ODF's paragraph rules (odg-styles R24): a run of white space is
//!   one space, the first and last of the paragraph are dropped, and `text:s` with `text:c` stands for that many spaces, counted and
//!   never written out. It is a stricter reading than ODF's: it does not collapse across a mark or an element, so it never joins two
//!   paragraphs that differ, and may keep apart two that ODF joins.
//! * Children are compared in order, except the siblings [`Rules::keyed`] names: those are compared as a set by their key, sorted
//!   (`O(n log n)`), the order among them and against the rest ignored. Equal keys are compared in the order written.
//! * **The children that are not keyed are set side by side by what they are, and not by the place they stand in**: see "What a difference is" below, and `align.rs`.
//! * **A part named in [`Rules::doctype_parts`] may begin with one DOCTYPE that holds only an external id** (and no internal subset), which is read and left out: the manifest of an
//!   OpenDocument package, as old OpenOffice.org writers wrote it. The DOCTYPE is no content, and a part that has one is read as one that has none. Any other DTD is refused as ever, and
//!   the part is then compared as bytes.
//! * A child that [`Rules::absent_when_empty`] names and that holds nothing (no attribute, no child element, no instruction, no text but white space)
//!   is left out of its parent: an empty container is the same as an absent one. Every copy of the name among the siblings must be empty for any to
//!   go, and the rule does not apply inside a paragraph element or to a keyed one. The blank text around it is read while it still stands, as spacing
//!   between children: `<a> <e/> </a>` is `<a/>`, and is not `<a> </a>` (blank text with no child beside it is content). A format that lists none is as before.
//!   [`Rules::absent_when_empty_under`] is the same rule for a child only under the parents its entries name ([`EmptyUnder`]): under any other element an empty
//!   one is the element it is.
//! * Under a parent [`Rules::unordered`] names, the children of its set are compared in the set's order, each name's copies in the order written, in the
//!   places they had among the parent's items: `<s><t/><x/></s>` is `<s><x/><t/></s>` when `t` and `x` are the set, `<s><x/><x/></s>` keeps its two `x` in
//!   their order, and a child outside the set keeps its place (`<s><t/><o/></s>` is not `<s><o/><t/></s>`).
//! * A child that [`Rules::placed`] names (a [`Placed`]) and that has no attribute for its place is read as having it, with the count of the children of its
//!   name before it: `<s><r/><r/></s>` is `<s><r i="0"/><r i="1"/></s>`. Which children are read so is the parent's kind's: all of them, none, or those with
//!   no name; a place that is written is compared as written (`01` is not `1`). A format that lists none is as before.
//!
//! * Attributes and elements of the namespaces [`Rules::ignored_when_extra`] names **count as nothing when only the second document has them**: what the first document
//!   holds of them must be in the second, with its value, and what only the second holds is no difference. **The comparison is oriented by it**: `count(a, b)` is not `count(b, a)` for a
//!   document that has such markup, and the first is the one the second became. They are left out of the hash that aligns children, and set apart from the keyed ones (an element of such a
//!   namespace is compared by the hash of its content, in order, apart from its siblings). A format that lists none is symmetric.
//! * An attribute that [`Rules::twinned`] names ([`Twin`]) **counts as nothing when only the second document has it, while its element holds the attribute it is the twin of with an equal
//!   value**: `xml:id="a"` beside `draw:id="a"` is ODF 1.2's way of saying what ODF 1.0 and 1.1 say by `draw:id` alone, so a program that writes the later version adds it.
//!   **Oriented as the above**: what the first document holds is in the second, with its value; what only the second holds, beside an equal twin, is no difference; the twin alone, or a twin
//!   of another value, is as any attribute. Such an attribute is left out of the hash that aligns children. A format that lists none is as before.
//! * [`Rules::free_names`] ([`FreeNames`]): the definitions of a container and the references to them are compared by what the definition says, and not by its name:
//!   a definition and the references to it that ask for its kind and family read as the content; **copies of one definition on one side count as one**, and **a reference from one
//!   definition to another is resolved to the content at the end of the chain**, a circle and a chain of more than [`MAX_CHAIN`] compared as written. A definition
//!   that has no match keeps its name and its references to what has none, and reads a reference to what has one as its content, so that a name that stands for a definition in one
//!   document and for none in the other is a difference.
//!
//! # What a difference is, and what is counted
//!
//! One difference is one of [`Found`], at one place. A child on one side only is one difference, whatever it holds. Children are
//! set side by side by what they are, so one added or taken-out child is one difference and not a shift of all the rest, and **every sibling after it is still compared, in
//! depth, with its own** (set side by side by place, the first child after an element put in was compared with the element, the second with the first, and so on,
//! and nothing inside them was looked at: a change planted deep in one of them was never seen). The common start and end of two lists of children, equal by a hash of each, are
//! matched at once. Of what lies between, each child is matched by **what it holds** (the same hash), then by **which thing it is** (its name and the attributes
//! [`Rules::identified_by`] lists), then by **what it is called** when its name is the only one of its kind in its list on each side (`align.rs`, which says why this is not an LCS: the count is
//! the same whichever document is first). Where the pairs are not in the same order in both lists the same children stand in another order: **one** [`Found::Order`], however many
//! cross. The children between two matched pairs that no key matched are set side by side by place (a replaced child is one difference), and what is left of one side is its own
//! (extra, missing). The hash only aligns: every pair it matches is compared in full, so it can never make two different contents the same. `count` is exact, and symmetric (`count(a, b) == count(b, a)`) under every rule but the one that orients the check
//! ([`Rules::ignored_when_extra`], above), which a format that lists nothing in it does not have; `shown` holds the first 200. The fuzz target `canon_compare` holds the
//! check to that law: symmetric without the rule, the same count with it where neither document declares its namespace, and oriented where it is planted.
//!
//! **A path** is element names and positions and nothing else: no attribute value, no key, no text, so no name a person chose can come
//! out of it (`/Pages[0]/Page[1]/Cell#3/@V`). `[i]` is the place among the element's items (its children that are not keyed, in order, and
//! its text and instructions), `#i` the place among its keyed children in the sorted order, `@name` an attribute. A path ends in `/...` past
//! 1,024 bytes, and a name in it is cut at 48 characters.
//!
//! # What it does not see
//!
//! Each is a reading of the design's section 4.3 that a person could want otherwise, kept and listed here so that none is a surprise:
//! * **A prefix used inside a value that a format does not list.** Namespace declarations are ignored, and a prefix that a value or a text
//!   names is the text it is, except in the attributes [`Rules::qnames`] lists: those are read through the namespaces in scope, so a
//!   prefix renamed passes, and one declared again as another namespace or no longer declared is a difference. A format lists the
//!   attributes whose values name prefixes (a schema `type`, the markup compatibility lists); one it has not listed is text.
//! * **`xml:space`** is an attribute like any other. It is not read as a rule, since every part of a VSDX package says `preserve` on its root
//!   and spacing between elements is not content there; the elements that do mix text with children are named in [`Rules::exact_text`].
//! * **Part names that a row names.** The set of parts folds case; a value that names a part (an `Override`'s `PartName`) is compared as
//!   written, so a part re-cased with a row for it is a difference there (never a false same).
//! * **Number spelling** is only for the attributes a format lists (`numbers`), and `-0` is not `0`.
//! * **A hash collision** cannot make two contents the same (see above); it can only name a difference otherwise (an `Order` for what is
//!   two changes), which the tests check by cutting the hash to no bits.
//! * **Blank text beside a child that counts as nothing** ([`Rules::ignored_children`], or of a namespace of [`Rules::ignored_when_extra`]) is
//!   spacing, as it is beside an empty container: the child stands in the tree as written, so `<a> <g/></a>` holds no text, and is not
//!   `<a> </a>` (blank text with no child beside it is content). A false difference, never a false same: reading such a child as absent would
//!   make a source written with indents (`<m>` and a line break around its only child that counts as nothing) differ from the same document
//!   written without them, which is the commoner case; only a rule that looks at both documents at once could have both.
//!
//! # What it costs, and the budget
//!
//! Linear in the parts' XML, `O(n log n)` in the siblings of one key set, in the children of one element that are set side by side (`align.rs`: two sorts of its children for each of
//! the keys it tries, the one the format lists and the others) and in the parts; counted in [`Work::canon_steps`] and [`Work::canon_bytes`]. **A list of more than [`MAX_ALIGNED`]
//! children between its matched start and end, on either side, is set side by side by place**, as every list was before, and counted in [`Work::canon_by_place`]: its count may then be
//! higher than the changes (an element put in shifts what follows it), and never lower. Each tree is built into a canonical form that borrows from the parsed tree, one part against its partner at a time;
//! the text of a paragraph is one string as long as the text. Recursion goes as deep as the parser's depth ceiling (512) and no deeper, on a
//! stack of 2 MiB.
//!
//! **The budget** ([`MAX_COMPARE_ITEMS`], [`MAX_COMPARE_BYTES`]) keeps a whole comparison under 2 GiB resident. Without it a pair of parts under
//! the parser's own limit (6 million items) held 3.4 to 3.9 GB, and a comparison of two packages held both inflated, up to 2 GiB, besides.
//! The parts of both sides together may hold 384 MiB (the largest real package is 173 MB), and a part may hold 2 million items on each side
//! (the largest real part has 1,481,648); a comparison past either is refused in words, before anything is read, with the part's name for
//! the items. A format's package reader is held to half of the bytes for each package (`diacraft-vsdx` does so). Measured at the budget, the
//! test profile at opt-level 1 (`tests/it/canon_scale.rs`, and `crates/vsdx/tests/it/canon.rs` for two whole packages): a part of
//! 1,999,999 items against its twin **1,023 MB** resident (the inputs are 28 MB), 14 steps for each element; 999,983 distinct keyed siblings
//! in two quasi-random orders **1,366 MB**, 2.6 steps for each `n log2 n`; two paragraphs of 190 MiB of text 1,333 MB with their strings;
//! two `.vsdx` packages of 192 MiB inflated each, with a page of 1,998,000 items, **1,435 MB** for the whole comparison, and with a page
//! of 180 MiB of text 771 MB. **The most measured is 1.44 GB**, so the figure to plan with is 1.5 GB, and 2 GiB holds at every limit.
//! 100,000 keyed siblings in a random order take 2.3 times the time of plain content of as many nodes, a million 3.5 times.
//!
//! **Qualified values** ([`Rules::qnames`]) are the one place where the canonical form can be longer than what it was made from: each prefix is
//! read as its namespace's URI. [`MAX_QUALIFIED_BYTES`] (16 MiB, on each side of a part) holds the expansion, charged token by token before it is
//! built; a prefix bound to a URI of 500,000 bytes and used 100,000 times (a value of 200 KB) would be 50 GB, and is refused in words
//! ([`CanonError::Values`]) at the 34th token, which `tests/it/canon_memory.rs` runs under `ulimit -v 3000000`.
//!
//! The trees and the canonical forms are built with ordinary allocation, as the parser's are, so on a machine with less than that an
//! allocation can still fail and abort. What the limits and the budget refuse is refused in words, before anything is built for it: a part
//! past a limit is refused before its partner is read, and `tests/it/canon_memory.rs` runs that under `ulimit -v 3000000`, with a
//! paragraph of 32 MiB beside it.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::limits::{XML_DEPTH_CEILING, XmlLimits};
use crate::work::Work;
use crate::xml::{self, El, Node, ParseOptions, XmlError};

mod align;
mod names;

/// The namespace `xml:` is bound to without a declaration.
pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
/// The namespace of the declarations themselves (`xmlns`), which XML Namespaces 1.0 lets no declaration bind.
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
/// The OpenDocument text namespace, whose `text:s` and `text:c` the paragraph rule reads.
pub const ODF_TEXT_NAMESPACE: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
/// Most differences a [`ContentDiff`] lists; the count goes on past it.
pub const MAX_SHOWN: usize = 200;
/// Most children of one element, between the start and the end that match, that are set side by side by what they are (`align.rs`), on either side: 262,144. A list of more is set side by side by
/// place, as every list was before, and counted in [`Work::canon_by_place`]; the count is then not less than the changes, and may be more. The work of the alignment is `n log n`
/// and what it holds is a few dozen bytes for each child on each side, so the cap keeps it small beside the trees; the part limits ([`MAX_COMPARE_ITEMS`]) are what bound a list in the end.
pub const MAX_ALIGNED: usize = 1 << 18;
/// Most items one part of a comparison may hold, on each side: elements, attributes, text runs and instructions, as the parser counts them. The
/// parser itself allows 6 million, and a pair at that holds about 3.9 GB; this is the part of that which a whole comparison can afford
/// (see "What it costs"). The largest real part measured is 1,481,648 items. A part past it is refused in words, naming it, before its pair
/// is built; the limit `limits.max_nodes` is lowered to this when it is higher.
pub const MAX_COMPARE_ITEMS: usize = 2_000_000;
/// Most bytes the parts of a comparison may hold together, both sides: 384 MiB. The largest real package inflates to 173 MB, so two of
/// those fit. Over it the comparison is refused before anything is read.
pub const MAX_COMPARE_BYTES: u64 = 384 * 1024 * 1024;
/// Most bytes the values of the attributes [`Rules::qnames`] lists may expand to, in one part on one side: a value names prefixes, and each is read
/// as its namespace's URI, so a prefix bound to a URI of a megabyte and used many times in a value of a few kilobytes would be built that many
/// times over. 16 MiB; a real value (`Ignorable`, `Requires`, `xsi:type`) is a few dozen bytes. Charged token by token before each is built, and
/// past it the comparison is refused in words ([`CanonError::Values`]).
pub const MAX_QUALIFIED_BYTES: u64 = 16 * 1024 * 1024;
/// Longest path a difference shows, in bytes: a deeper place ends in `/...`.
const MAX_PATH_BYTES: usize = 1024;
/// Most characters of one element or attribute name a path shows.
const MAX_NAME_IN_PATH: usize = 48;
/// Most characters of a part name a difference shows.
const MAX_PART_IN_DIFFERENCE: usize = 128;

/// An element or attribute name: a namespace URI (empty for none) and a local name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name {
    pub ns: &'static str,
    pub local: &'static str,
}

/// An attribute that says again what another attribute of its element says ([`Rules::twinned`]): `attr`, whose `with` is the twin.
#[derive(Debug, Clone, Copy)]
pub struct Twin {
    pub attr: Name,
    pub with: Name,
}

/// Siblings compared as a set: the children named `child`, matched by the attributes `key`. A format's list gives each entry its source, in
/// a comment: the format's own statement that order has no meaning, or a measured fact.
#[derive(Debug, Clone, Copy)]
pub struct Keyed {
    pub child: Name,
    pub key: &'static [Name],
    /// How the text of a key is folded before two keys are compared: the way the format's own reader folds it, so that two siblings the
    /// reader takes for one thing are one key, and are compared in the order written (a reader that takes the first of them reads the other
    /// order differently).
    pub fold: KeyFold,
}

/// How a key is folded before it is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFold {
    /// As written.
    Exact,
    /// ASCII letters lowered: an extension in OPC's content types.
    AsciiCase,
    /// As OPC's reader folds the name of a part in a content types row: `%` escapes decoded when they decode to UTF-8 and hide no `/` or
    /// `\`, a slash put in front when there is none, ASCII letters lowered.
    PartName,
}

/// An attribute whose value names namespaces through prefixes, so that the prefix's namespace is part of what the value says.
#[derive(Debug, Clone, Copy)]
pub struct Qualified {
    pub attr: Name,
    /// The element the attribute is read on, or any element with the attribute (`Requires` is the attribute of one element only).
    pub on: Option<Name>,
    pub kind: QKind,
}

/// What the value of a [`Qualified`] attribute is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QKind {
    /// Names (`dcterms:W3CDTF`) separated by white space: each is its namespace and its local name, a name with no prefix in the default
    /// namespace in scope. A schema `type`; a markup compatibility `ProcessContent`.
    QNames,
    /// Prefixes separated by white space: each is its namespace. A markup compatibility `Ignorable`, `MustUnderstand` and a `Choice`'s
    /// `Requires`.
    Prefixes,
}

/// What a format tells the check (the lists of section 4.3): each is a list of names, and an empty list turns the rule off.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// Siblings whose order has no meaning.
    pub keyed: &'static [Keyed],
    /// Attributes the format types as a number or a length, which may be respelled (`1.50` for `1.5`). Only an attribute a writer is shown
    /// to respell goes here: one that holds text must not, or `007` and `7` pass as the same.
    pub numbers: &'static [Name],
    /// Attributes the format types as an OpenDocument length or percentage, which may be respelled the way ODF's own reading does
    /// ([`same_length`]: no `+`, no exponent, a unit of the length's list). An attribute in both this list and `numbers` is read as a length.
    pub lengths: &'static [Name],
    /// Attributes whose value holds prefixes (see [`Qualified`]): they are compared by the namespaces the prefixes name, so that a prefix
    /// that is declared again as another namespace, or no longer declared, is a difference, and a prefix renamed is not. Namespace
    /// declarations are otherwise ignored, so without this a declaration only a value uses could change unseen.
    pub qnames: &'static [Qualified],
    /// Elements of paragraph content, whose white space follows ODF's rules.
    pub paragraphs: &'static [Name],
    /// Elements that mix text with other children and whose white space is content, so that blank text in them is never ignored: without
    /// this a shape's text of one space beside a marker would be the same as no text. A format with no such element lists none.
    pub exact_text: &'static [Name],
    /// Elements that are the same as none when they hold nothing (an *empty container*): no attribute (a namespace declaration is none), no child
    /// element, no processing instruction and no text but white space (comments are no content here, as everywhere), read on the tree as it was written: an
    /// element that holds an empty element of the list is not empty. Such a child is left out of its parent, so a part that writes one and a part that
    /// leaves it out are the same. **Only when every copy of the name among the siblings is empty**:
    /// a name written twice where one copy holds something is kept as written, since a reader that takes the first copy reads the file differently from one
    /// that has only the full one. An element that is also [`Rules::keyed`] is never left out, and neither is one in a paragraph. A format lists an element
    /// here only on a source that says an empty one holds exactly what an absent one holds, wherever it stands; a source that says so of a child of one
    /// parent is listed in [`Rules::absent_when_empty_under`].
    pub absent_when_empty: &'static [Name],
    /// Children that are the same as none when they hold nothing **under one parent only** ([`EmptyUnder`]): read as [`Rules::absent_when_empty`] reads its
    /// list, for a child named in an entry's `children` whose parent is the entry's `parent`. The same name under any other element is the element it is,
    /// empty or not. A format lists a child here on a source that names the parent.
    pub absent_when_empty_under: &'static [EmptyUnder],
    /// Children of one parent whose order **among themselves** has no meaning ([`Unordered`]): under an element named `parent`, the children named in
    /// `children` are compared in the order of the list, each name's copies in the order written, and they take the places they had among the parent's
    /// items, so that every other child keeps its place. A format lists a set here only on a source that says its order is no content.
    pub unordered: &'static [Unordered],
    /// Parts that are the same as no part when they hold nothing ([`EmptyPart`]): a part on **one side only** whose name is of an entry's kind
    /// ([`EmptyPart::named`]), that is meant as XML, parses, has that entry's root and whose root holds nothing (as [`Rules::absent_when_empty`] reads it:
    /// no attribute, no child element, no instruction, no text but white space) is no difference. **A part of any other name is a part whatever its root
    /// holds**: a format keeps a part it does not know, so dropping one is a difference. A part of the kind that holds something, a part that does not parse,
    /// and one larger than [`MAX_EMPTY_PART_BYTES`] are compared as before (a missing or an extra part). A format lists a kind here only on a source that
    /// says such a part, empty, says what no part says.
    pub parts_absent_when_empty: &'static [EmptyPart],
    /// Attributes that **count as nothing wherever they stand**, as a namespace declaration counts as nothing: left out of what an element is compared by, and no
    /// content of an element that [`Rules::absent_when_empty`] or an [`EmptyPart`] asks to hold nothing. A format lists an attribute here only on a ruling that says
    /// its value is no fact about the document (ODF's `office:version`: a writer says the one version it writes). The name is read through the
    /// declarations in force where the attribute stands, its own element's among them.
    pub ignored_attributes: &'static [Name],
    /// Children that **count as nothing under one parent** ([`IgnoredUnder`]): under an element named `parent`, a child named in `children` is left out before anything else is
    /// read, and is no content of a parent that [`Rules::absent_when_empty`], [`Rules::absent_when_empty_under`] or an [`EmptyPart`] asks to hold nothing. A format lists a child
    /// here only on a ruling that says its text is no fact about the document (ODF's `meta:generator`: a writer says its own name). The same name under any other
    /// parent is the element it is.
    pub ignored_children: &'static [IgnoredUnder],
    /// Namespaces whose attributes and elements **count as nothing when they are only in the second document**, the one the first became: a program's own markup that it adds when it
    /// writes a document (ODF's `diacraft:` extension). **The comparison is oriented by it**: what the first document holds of such a namespace must be in the second (an
    /// attribute with its value, an element equal to one of it), and what only the second holds is no difference, so that the count of `compare(a, b)` is not that of `compare(b, a)` for
    /// a document that has such markup. A format that lists none is symmetric, as before. Such an attribute is left out of the hash that aligns children, so that a child that a program
    /// marked is still aligned with the child it was.
    pub ignored_when_extra: &'static [&'static str],
    /// Attributes that **count as nothing when only the second document has them, beside an equal twin** ([`Twin`]): an attribute that says again what another attribute of
    /// its element says, in the form a later version of the format takes (ODF's `xml:id` beside `draw:id`: a file of ODF 1.0 or 1.1 cannot say the first, and a writer that raises the
    /// version adds it). **The comparison is oriented by it**, as by [`Rules::ignored_when_extra`]: what the first document holds is compared as written, so an attribute of the first that the
    /// second lacks is a difference; and the attribute counts as nothing only while the element holds its twin with the same value. A format that lists none is symmetric, as before.
    pub twinned: &'static [Twin],
    /// Names that are free ([`FreeNames`]): the names of the definitions a document gives its own automatic things (ODF's automatic styles) and the references to
    /// them are compared by what the definition says, so that two documents that differ only in the names they chose are the same. At most one entry is read.
    pub free_names: &'static [FreeNames],
    /// Children that name their own place by an attribute and may say nothing in one ([`Placed`]): an absent attribute is read as the count of the children of
    /// the name before it, so that `<s><r/><r/></s>` is `<s><r i="0"/><r i="1"/></s>`. A format lists one only on a source that says an absent place means the count.
    pub placed: &'static [Placed],
    /// **Spellings of one name** (APPID): each pair `(old, new)` reads every `old` in an XML part (a namespace a declaration binds, an attribute value, a text, a name) as `new`, on both
    /// sides, before the part is read as XML. A format lists the namespace that its earlier writers wrote beside the one it writes, so that a document that one program wrote
    /// and another program wrote again in the new spelling are the same content and every other difference between them is still one. **Only a part meant as XML and valid UTF-8 is read
    /// so, and `new` must not be longer than `old`** (a pair that is longer is not applied: the copy of a part is never larger than the part). Applied one pair of parts at a time, so
    /// that what is held beside the parts is one pair's copy. It is a rule of the text and never of a name: the spelling is read wherever it stands, in a value or a text as in a
    /// namespace, and a format lists only a spelling that no document uses for anything else (a URN of its own).
    pub aliases: &'static [(&'static str, &'static str)],
    /// **What says which thing an element is**: the attributes whose values name an element among its siblings (a page's, a shape's or a style's name, an identifier). An
    /// element that has any of them is told from its siblings by its name and the values of those it has, in this list's order: **the second key of the alignment** of the children that are
    /// not keyed ([`MAX_ALIGNED`], `align.rs`), so that a page or a shape that changed, put in, taken out or written in another place is still matched with the page or the shape it is. A format
    /// lists an attribute only on a source that says it names a thing among its siblings. It is never what makes two children the same: every pair is compared in full. An empty list tries
    /// no such key.
    pub identified_by: &'static [Name],
    /// **The parts that may begin with a DOCTYPE** that holds only a name and an external id and no internal subset, by their exact names as the entry gives them: the
    /// OpenDocument manifest, which an old OpenOffice.org writer began so. It is read and left out of the tree, as the package layer reads a manifest, so that a part that has one is read as
    /// one that has none and the DOCTYPE is no content. Any other DTD, and a DOCTYPE in any other part, is refused as ever, and the part is compared as bytes.
    pub doctype_parts: &'static [&'static str],
}

/// A set of children whose order among themselves has no meaning under one parent ([`Rules::unordered`]).
#[derive(Debug, Clone, Copy)]
pub struct Unordered {
    pub parent: Name,
    pub children: &'static [Name],
}

/// Children that are the same as none when they hold nothing, under one parent ([`Rules::absent_when_empty_under`]).
#[derive(Debug, Clone, Copy)]
pub struct EmptyUnder {
    pub parent: Name,
    pub children: &'static [Name],
}

/// Children that count as nothing under one parent ([`Rules::ignored_children`]).
#[derive(Debug, Clone, Copy)]
pub struct IgnoredUnder {
    pub parent: Name,
    pub children: &'static [Name],
}

/// Names that are free ([`Rules::free_names`], for the automatic styles of an OpenDocument drawing).
///
/// A document may hold, in a `container` that is a child of its root, **definitions**: children that are one of [`FreeNames::definitions`], each with a name (the attribute
/// [`FreeNames::name`]) and, for a kind that has one, a family (the attribute [`Definition::family`]). Elsewhere in the document, **references** (an attribute of
/// [`FreeNames::references`], on the elements its entry says) name a definition by its kind, its family and its name. The check compares a definition by what it says without its
/// name, and a reference by the definition it names, so that two definitions with the same content under two names are the same, and a reference to the one is the same as a
/// reference to the other.
///
/// * **What is renamed.** A definition whose kind, family and name no other definition of the container has, and whose content (every attribute but the name, and every child, read
///   as the whole check reads it) is the content of a definition of the other side. It is read as `\u{4}` and the number of its content, and so is every reference to it **that asks for
///   that kind and that family** (the first entry of [`FreeNames::references`] whose attribute and element match says which: a reference that asks for another family, as a
///   `draw:style-name` that asks for a graphic style does of a paragraph style of the same name, is not a reference to it and is compared as written).
/// * **What is not.** A definition with no match on the other side keeps its name, so that a style whose property changed is one difference in that style, as before; a name two
///   definitions of one kind and family hold, a definition with no name or no unique one, a document with two containers, and a name that no definition has are compared as
///   written; and a name inside the container, or inside an element of [`FreeNames::scopes`] (an embedded document, whose names are its own), is compared as written.
/// * **What it costs.** The container is weighed before anything is built for it ([`MAX_NAMED_BYTES`]; past it every name is compared as written, which reports more and never calls
///   different content the same), and then the content of each definition is written out once, in a form that is kept for the first document's alone, so that the work is linear in
///   the container and the memory is less than what it weighs.
#[derive(Debug, Clone, Copy)]
pub struct FreeNames {
    /// The container of definitions, a child of the root of the document.
    pub container: Name,
    /// The kinds of definition, as elements of the container.
    pub definitions: &'static [Definition],
    /// The attribute that holds a definition's name.
    pub name: Name,
    /// The references, in the order they are tried.
    pub references: &'static [Reference],
    /// The references **inside a definition** to another definition (an automatic style that names its parent, its next style or its list style), tried in the order given; `on` is the
    /// element of the definition they are read on (its own, or one below it). Resolved recursively as the shapes' are, to what the definition they name says: a chain
    /// that is circular, or more than [`MAX_CHAIN`] definitions long, is compared as written, and so is every definition that reaches one.
    pub definition_references: &'static [Reference],
    /// Elements, below the root, whose names are their own: neither a definition nor a reference inside one is read.
    pub scopes: &'static [Name],
}

/// A kind of definition: its element, and the attribute that holds its family (none for a kind that has no family).
#[derive(Debug, Clone, Copy)]
pub struct Definition {
    pub element: Name,
    pub family: Option<Name>,
}

/// An attribute that names a definition ([`FreeNames::references`]): on the element `on` (any element when none), what it names is a definition of one of the kinds `takes` (a name that
/// is the name of a definition of more than one of them is not told, and is compared as written). With `list`, the value is a list of names separated by white space, and each is
/// read: the ones that name a definition as its class, the others as written.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub attr: Name,
    pub on: Option<Name>,
    pub takes: &'static [Takes],
    pub list: bool,
}

/// What a reference asks for: a definition of the kind `element`, with the family `family` (`""` for a kind with none, or [`SAME_FAMILY`] in [`FreeNames::definition_references`]: the
/// family of the definition the reference is in).
#[derive(Debug, Clone, Copy)]
pub struct Takes {
    pub element: Name,
    pub family: &'static str,
}

/// The family a reference inside a definition asks for when it is the family of that definition (a style's parent is a style of its own family).
pub const SAME_FAMILY: &str = "\0";

/// Longest chain of definitions that name one another that is resolved: a definition whose references lead through more than this many others is compared as written.
pub const MAX_CHAIN: usize = 64;

/// Most a side's container of definitions may weigh (a bound on what its content is written out as, counted before anything is built for it): 64 MiB. A container past it has every
/// name of both documents compared as written, whichever document is first.
pub const MAX_NAMED_BYTES: u64 = 64 * 1024 * 1024;

/// A kind of part that is the same as no part when it holds nothing ([`Rules::parts_absent_when_empty`]): parts whose name `named` accepts, with the root `root`.
#[derive(Debug, Clone, Copy)]
pub struct EmptyPart {
    /// Whether a part's name, as the entry gives it, is of the kind: the format's own rule for its name (OPC names a relationships part `_rels/<source>.rels`,
    /// and a VSDX package gives `diacraft_opc::name::is_rels_part`). It must not panic on any name.
    pub named: fn(&str) -> bool,
    /// The root such a part has, by the namespace its own declarations bind.
    pub root: Name,
    /// Children of the root that are no content when they hold nothing themselves (as [`Rules::absent_when_empty`] reads it, [`Rules::ignored_attributes`] counting as
    /// nothing): ODF's `office:settings` of `settings.xml` and `office:meta` of `meta.xml` (an empty part counts as absent). Any other child, and one of these
    /// that holds something, is content. A kind with none lists none.
    pub children: &'static [Name],
}

/// A child whose place is an attribute that, when it is absent, is the count of the children of its name before it ([`Rules::placed`]; `IX` of a `Row`).
///
/// Under an element named `parent`, a child named `child` that has no attribute `place` is read as having it, with the number of children named `child` before it
/// in `parent` (counted whatever places those say) written in decimal; a place that is there is compared as written, so `01` and `+1` are not `1`. Which children are
/// read so depends on the **kind** of the parent, the value of its attribute `kind`, compared exactly: a kind in [`Placed::always`] reads every child so; one in
/// [`Placed::never`] none; any other kind (and a parent that says none) only a child with no attribute `named_by`, since a child that names itself is not found by its
/// place. A parent that no entry names and a child of another name or namespace are as written.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub parent: Name,
    /// The attribute of `parent` that says what kind of parent it is.
    pub kind: Name,
    pub child: Name,
    /// The attribute of `child` that names its place.
    pub place: Name,
    /// The attribute of `child` that names it, in the kinds that are neither of the two lists.
    pub named_by: Name,
    /// The kinds whose children are all found by their place.
    pub always: &'static [&'static str],
    /// The kinds whose children are never found by their place.
    pub never: &'static [&'static str],
}

/// The largest part [`Rules::parts_absent_when_empty`] reads to tell whether it holds nothing: 64 KiB, which a root with its declarations and a comment fits in
/// many times over. A larger part is a part (never the same as no part), so a part padded past it with white space is a difference, and nothing large is parsed
/// for the rule.
pub const MAX_EMPTY_PART_BYTES: usize = 64 * 1024;

impl Rules {
    /// No rule at all: everything in order, everything as written.
    pub const NONE: Rules = Rules {
        keyed: &[],
        numbers: &[],
        lengths: &[],
        qnames: &[],
        paragraphs: &[],
        exact_text: &[],
        absent_when_empty: &[],
        absent_when_empty_under: &[],
        unordered: &[],
        parts_absent_when_empty: &[],
        ignored_attributes: &[],
        ignored_children: &[],
        ignored_when_extra: &[],
        twinned: &[],
        free_names: &[],
        placed: &[],
        aliases: &[],
        identified_by: &[],
        doctype_parts: &[],
    };
}

/// What kind of difference was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Found {
    /// A part of the first package that the second does not have.
    MissingPart,
    /// A part of the second package that the first does not have.
    ExtraPart,
    /// Two parts that are not XML (or are XML that does not parse) with different bytes.
    Bytes,
    /// An element on one side only, or two elements or other items in one place that are not the same kind or name.
    Element,
    /// An attribute on one side only.
    Attribute,
    /// An attribute whose value differs.
    Value,
    /// Text that differs, or is on one side only.
    Text,
    /// A processing instruction that differs, or is on one side only.
    Instruction,
    /// The same children in another order (reported once, at the parent).
    Order,
}

impl Found {
    /// The word for the kind, for a report.
    pub fn word(self) -> &'static str {
        match self {
            Found::MissingPart => "missing_part",
            Found::ExtraPart => "extra_part",
            Found::Bytes => "bytes",
            Found::Element => "element",
            Found::Attribute => "attribute",
            Found::Value => "value",
            Found::Text => "text",
            Found::Instruction => "instruction",
            Found::Order => "order",
        }
    }
}

/// One difference: the part, the place in it, and what was found there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The part's name as the first package spells it (the second's for an extra part), cut to 128 characters.
    pub part: String,
    /// Element names and positions, from the root, with no value and no name a person chose. Empty for a part as a whole.
    pub path: String,
    pub found: Found,
}

/// What the comparison found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentDiff {
    /// The first [`MAX_SHOWN`] differences, in the order the parts (by name) and the documents give them.
    pub shown: Vec<Difference>,
    /// How many differences there are, exactly.
    pub count: u64,
}

impl ContentDiff {
    /// Whether the two have the same content.
    pub fn same(&self) -> bool {
        self.count == 0
    }
}

/// Why a comparison is refused: a part past the limits or the budget, or parts too large between them. A refusal is words and a class, never an
/// abort; the check cannot say anything of what it did not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonError {
    /// The XML of a part is past a limit (depth, items, an attribute): the part, as a difference would show its name, and the limit.
    Xml { part: String, error: XmlError },
    /// The parts of the two sides together hold more bytes than the budget ([`MAX_COMPARE_BYTES`]).
    TooLarge { bytes: u64, limit: u64 },
    /// The values of a part that name prefixes expand to more than [`MAX_QUALIFIED_BYTES`] on one side: the part, as a difference would show its
    /// name, and the limit.
    Values { part: String, limit: u64 },
}

impl CanonError {
    /// The class word: the XML error's (`xml_too_complex`), or `package_too_large`.
    pub fn class(&self) -> &'static str {
        match self {
            CanonError::Xml { error, .. } => error.class(),
            CanonError::TooLarge { .. } => "package_too_large",
            CanonError::Values { .. } => "xml_too_complex",
        }
    }
}

impl std::fmt::Display for CanonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonError::Xml { part, error } => write!(f, "\"{}\": {error}", part.escape_debug()),
            CanonError::TooLarge { bytes, limit } => {
                write!(f, "the parts to compare are {bytes} bytes together, larger than the {limit} bytes a comparison holds")
            }
            CanonError::Values { part, limit } => {
                write!(f, "\"{}\": the values that name namespace prefixes expand to more than {limit} bytes", part.escape_debug())
            }
        }
    }
}

impl std::error::Error for CanonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CanonError::Xml { error, .. } => Some(error),
            CanonError::TooLarge { .. } | CanonError::Values { .. } => None,
        }
    }
}

/// Why the canonical form of a part could not be built: the parser's limit, or the expansion of the qualified values past its own.
#[derive(Debug)]
enum Refusal {
    Xml(XmlError),
    Values,
}

impl From<XmlError> for Refusal {
    fn from(error: XmlError) -> Refusal {
        Refusal::Xml(error)
    }
}

/// A part of a package, as the check sees it.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub name: &'a str,
    /// Whether the part is meant as XML (the format's content types or its own list say so).
    pub xml: bool,
    pub bytes: &'a [u8],
}

/// Compares two sets of parts by the rules above. `fold_case` folds ASCII case in the part names. `limits` bound the XML of each part (and
/// its items are held to [`MAX_COMPARE_ITEMS`]). An error is a part the XML limits refuse on either side, named, or parts past the byte budget
/// ([`MAX_COMPARE_BYTES`]); a part that does not parse is compared as bytes.
pub fn compare_entries(
    a: &[Entry<'_>],
    b: &[Entry<'_>],
    fold_case: bool,
    rules: &Rules,
    limits: &XmlLimits,
    work: &mut Work,
) -> Result<ContentDiff, CanonError> {
    let bytes: u64 = a.iter().chain(b).fold(0u64, |sum, e| sum.saturating_add(e.bytes.len() as u64));
    if bytes > MAX_COMPARE_BYTES {
        return Err(CanonError::TooLarge { bytes, limit: MAX_COMPARE_BYTES });
    }
    let limits = &XmlLimits { max_nodes: limits.max_nodes.min(MAX_COMPARE_ITEMS), ..limits.clone() };
    let mut sink = Sink::default();
    let ka = sorted_names(a, fold_case, work);
    let kb = sorted_names(b, fold_case, work);
    let (mut i, mut j) = (0, 0);
    loop {
        match (ka.get(i), kb.get(j)) {
            (None, None) => break,
            (Some((_, x)), None) => {
                one_side(a.get(*x), Found::MissingPart, rules, limits, work, &mut sink);
                i += 1;
            }
            (None, Some((_, y))) => {
                one_side(b.get(*y), Found::ExtraPart, rules, limits, work, &mut sink);
                j += 1;
            }
            (Some((nx, x)), Some((ny, y))) => {
                work.canon_steps = work.canon_steps.saturating_add(1);
                match cmp_counted(nx, ny, work) {
                    Ordering::Less => {
                        one_side(a.get(*x), Found::MissingPart, rules, limits, work, &mut sink);
                        i += 1;
                    }
                    Ordering::Greater => {
                        one_side(b.get(*y), Found::ExtraPart, rules, limits, work, &mut sink);
                        j += 1;
                    }
                    Ordering::Equal => {
                        if let (Some(ea), Some(eb)) = (a.get(*x), b.get(*y)) {
                            compare_pair(ea, eb, rules, limits, work, &mut sink).map_err(|refusal| match refusal {
                                Refusal::Xml(error) => CanonError::Xml { part: sink.part.clone(), error },
                                Refusal::Values => CanonError::Values { part: sink.part.clone(), limit: MAX_QUALIFIED_BYTES },
                            })?;
                        }
                        i += 1;
                        j += 1;
                    }
                }
            }
        }
    }
    Ok(ContentDiff { shown: sink.shown, count: sink.count })
}

/// Whether `a` and `b` are the same number with the same unit: each reads whole as `[+-]digits[.digits][e[+-]digits]` and then nothing, `%` or
/// ASCII letters; the numbers are the same finite `f64`, and the units the same text.
///
/// Decided, each with a test: `1.50` is `1.5` and `15E-1` is `1.5`; `2cm` is not `20mm`; `.5` and `5.` are not numbers, so they are text; a
/// number that reads as infinity, or as zero though it has a digit that is not zero, is not read (it is text); `-0` is not `0`, since a
/// sign is content until a writer is shown to respell it; and `007` is `7` here, so the caller must list only attributes that hold numbers.
pub fn same_number(a: &str, b: &str) -> bool {
    match (read_number(a, false), read_number(b, false)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// The units a length or a percentage has in OpenDocument (odg-styles R17).
const LENGTH_UNITS: [&str; 7] = ["cm", "mm", "in", "pt", "pc", "px", "%"];

/// Whether `a` and `b` are the same length as OpenDocument reads one (odg-styles): a signed decimal number, **a `-` and no `+`**,
/// digits and optionally a point and digits, **no exponent**, **directly followed by a unit** of `cm`, `mm`, `in`, `pt`, `pc`, `px` or `%`,
/// in that case. What does not read so is text, and text is only the same as itself, so `+0.1cm` is not `0.1cm`, `1e-1cm` is not `0.1cm` and
/// `2.5e0cm` is not `2.5cm`: the standard defines none of them, and a reader sends them to its default. The numbers are compared as
/// [`same_number`] compares them (the same finite `f64`, `-0` not `0`) and the units must be the same.
pub fn same_length(a: &str, b: &str) -> bool {
    match (read_number(a, true), read_number(b, true)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

// ------------------------------------------------------------------------------------------------------------------------- numbers

/// The number and the unit `s` reads as, or none: see [`same_number`], and [`same_length`] when `strict`. The number is its `f64` bits, so
/// that `-0` and `0` differ.
fn read_number(s: &str, strict: bool) -> Option<(u64, &str)> {
    let b = s.as_bytes();
    let signs: &[u8] = if strict { b"-" } else { b"+-" };
    let mut i = usize::from(b.first().is_some_and(|c| signs.contains(c)));
    let digits = |from: usize| b.get(from..).map_or(0, |rest| rest.iter().take_while(|c| c.is_ascii_digit()).count());
    let whole = digits(i);
    if whole == 0 {
        return None;
    }
    let mut nonzero = b.get(i..i + whole)?.iter().any(|&c| c != b'0');
    i += whole;
    if b.get(i) == Some(&b'.') {
        let fraction = digits(i + 1);
        if fraction > 0 {
            nonzero |= b.get(i + 1..i + 1 + fraction)?.iter().any(|&c| c != b'0');
            i += 1 + fraction;
        }
    }
    if !strict && matches!(b.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
        let exponent = digits(i + 1 + sign);
        if exponent > 0 {
            i += 1 + sign + exponent;
        }
    }
    let unit = s.get(i..)?;
    let unit_ok = if strict { LENGTH_UNITS.contains(&unit) } else { unit.is_empty() || unit == "%" || unit.bytes().all(|c| c.is_ascii_alphabetic()) };
    if !unit_ok {
        return None;
    }
    let value: f64 = s.get(..i)?.parse().ok()?;
    // Infinity is no number, and a nonzero decimal that underflows to zero would be the same as zero: both stay text.
    if !value.is_finite() || (value == 0.0 && nonzero) {
        return None;
    }
    Some((value.to_bits(), unit))
}

// ------------------------------------------------------------------------------------------------------------------------ the hash

/// A 64-bit hash that only aligns children: FNV-1a over bytes and lengths, mixed once at the end. Never evidence that two things are the same.
#[derive(Clone, Copy)]
pub(crate) struct Hash(u64);

impl Hash {
    pub(crate) fn new(tag: u8) -> Hash {
        let mut h = Hash(0xcbf2_9ce4_8422_2325);
        h.byte(tag);
        h
    }

    fn byte(&mut self, b: u8) {
        self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        for &b in s.as_bytes() {
            self.byte(b);
        }
    }

    pub(crate) fn finish(self) -> u64 {
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) & test_support::mask()
    }
}

/// What tests use to prove that the hash only aligns: it can be cut to a few bits, so that unrelated children collide.
#[cfg(feature = "test-support")]
pub mod test_support {
    use std::cell::Cell;

    thread_local! {
        static MASK: Cell<u64> = const { Cell::new(u64::MAX) };
    }

    /// Restores the mask when it goes out of scope, a panic included.
    struct Restore(u64);

    impl Drop for Restore {
        fn drop(&mut self) {
            MASK.with(|m| m.set(self.0));
        }
    }

    /// Runs `f` with the hash of this thread cut to the bits of `mask`: with `0`, every two children hash alike, so every pair of children is
    /// "matched" by it and must be told apart by the full comparison alone. Only the features of the hash change, never what the check says.
    pub fn with_hash_mask<R>(mask: u64, f: impl FnOnce() -> R) -> R {
        let _restore = Restore(MASK.with(|m| m.replace(mask)));
        f()
    }

    pub(super) fn mask() -> u64 {
        MASK.with(Cell::get)
    }
}

#[cfg(not(feature = "test-support"))]
mod test_support {
    pub(super) fn mask() -> u64 {
        u64::MAX
    }
}

// ----------------------------------------------------------------------------------------------------------- the canonical form

/// An expanded name: borrowed from the parsed tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct QName<'a> {
    ns: &'a str,
    local: &'a str,
}

impl QName<'_> {
    fn is(&self, n: &Name) -> bool {
        self.ns == n.ns && self.local == n.local
    }
}

/// An attribute's value: text, or a number read under [`Rules::numbers`] (its bits and its unit).
///
/// **A `Text` and an `Owned` of one string are one value** (equal, ordered alike, hashed alike): the owned one is text a rule made (a folded key, a value read through
/// the namespaces in scope, the place a [`Placed`] child leaves out), and it stands for what the other side writes. The two never meet in the other rules: they are
/// made by the name of the attribute, which both sides share.
#[derive(Debug, Clone)]
pub(crate) enum Val<'a> {
    Text(&'a str),
    Num(u64, &'a str),
    /// A value made from another: a folded key, or a value read through the namespaces in scope (an owned text).
    Owned(String),
}

impl Val<'_> {
    /// The text of a text value, and the kind (text before numbers).
    fn rank(&self) -> (u8, &str) {
        match self {
            Val::Text(s) => (0, s),
            Val::Owned(s) => (0, s.as_str()),
            Val::Num(_, unit) => (1, unit),
        }
    }
}

impl PartialEq for Val<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Val<'_> {}

impl PartialOrd for Val<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Val<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        let bits = |v: &Val<'_>| if let Val::Num(bits, _) = v { *bits } else { 0 };
        self.rank().cmp(&other.rank()).then_with(|| bits(self).cmp(&bits(other)))
    }
}

impl Val<'_> {
    fn len(&self) -> usize {
        match self {
            Val::Text(s) => s.len(),
            Val::Num(_, unit) => 8 + unit.len(),
            Val::Owned(s) => s.len(),
        }
    }

    pub(crate) fn feed(&self, h: &mut Hash) {
        match self {
            Val::Text(s) => {
                h.byte(0);
                h.str(s);
            }
            Val::Num(bits, unit) => {
                h.byte(1);
                h.u64(*bits);
                h.str(unit);
            }
            // Alike to a text of the same string: they are one value (see [`Val`]).
            Val::Owned(s) => {
                h.byte(0);
                h.str(s);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CAttr<'a> {
    name: QName<'a>,
    val: Val<'a>,
    /// Counts as nothing when only the second document has it ([`Rules::ignored_when_extra`]): not in the hash that aligns the children.
    quiet: bool,
}

/// Opens a counted run of spaces in paragraph text, and `RUN_CLOSE` ends it: `RUN_OPEN`, the count in decimal, `RUN_CLOSE`. Neither can be in
/// any text a parser gives (XML 1.0 allows neither character, and a reference to one is refused), so no text spells a run.
const RUN_OPEN: char = '\u{1}';
const RUN_CLOSE: char = '\u{2}';
/// Opens a name or a prefix in a value that nothing in scope binds.
const UNBOUND: char = '\u{3}';

/// Text as it compares: a plain element's text as it is, and a paragraph's as **one string** as long as the text and no longer: one space for
/// a run of one, and a counted run (`text:s` with a count past what is worth writing out) as `RUN_OPEN`, the count and `RUN_CLOSE`.
#[derive(Debug, PartialEq, Eq)]
enum Txt<'a> {
    Plain(Cow<'a, str>),
    Para(String),
}

impl Txt<'_> {
    fn hash(&self) -> u64 {
        let mut h = Hash::new(b'T');
        match self {
            Txt::Plain(s) => {
                h.byte(0);
                h.str(s);
            }
            Txt::Para(s) => {
                h.byte(1);
                h.str(s);
            }
        }
        h.finish()
    }

    fn len(&self) -> usize {
        match self {
            Txt::Plain(s) => s.len(),
            Txt::Para(s) => s.len(),
        }
    }
}

/// A child that is not a keyed element, in the order written: comments are gone and adjacent text is one run.
#[derive(Debug)]
enum Item<'a> {
    El(Box<Canon<'a>>),
    Text(Txt<'a>, u64),
    Pi(&'a str, &'a str, u64),
}

impl Item<'_> {
    fn hash(&self) -> u64 {
        match self {
            Item::El(c) => c.hash,
            Item::Text(_, h) | Item::Pi(_, _, h) => *h,
        }
    }
}

/// The class of the elements that count as nothing when only the second document has them ([`Rules::ignored_when_extra`]): kept with the keyed elements, after all of them, and not keyed.
const QUIET: usize = usize::MAX;

/// An element of a keyed set, with its key (the value of each key attribute, or none).
#[derive(Debug)]
struct KeyedEl<'a> {
    class: usize,
    key: Vec<Option<Val<'a>>>,
    key_bytes: usize,
    el: Canon<'a>,
}

/// An element in its canonical form: names expanded, attributes sorted, text joined and normalised, keyed children set apart and sorted.
#[derive(Debug)]
struct Canon<'a> {
    name: QName<'a>,
    attrs: Vec<CAttr<'a>>,
    items: Vec<Item<'a>>,
    keyed: Vec<KeyedEl<'a>>,
    hash: u64,
}

/// What a child of an element is before it is built.
enum Seg<'a> {
    Text(Cow<'a, str>),
    El(&'a El),
    Pi(&'a str, &'a str),
}

/// XML's white space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn is_blank(s: &str) -> bool {
    s.chars().all(is_space)
}

// ----------------------------------------------------------------------------------------------------- the constraints of namespaces

/// The prefix the attribute `key` declares (`""` for the default namespace, `xmlns`), or none for an attribute that is no declaration. `xmlns:` with nothing after the colon
/// declares `""` too, as every reader of ours reads it; it is no name XML Namespaces 1.0 allows ([`attribute_breaks`]).
fn declared_prefix(key: &str) -> Option<&str> {
    if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") }
}

/// Whether `name`, an element's or an attribute's that is no declaration, is no qualified name of XML Namespaces 1.0 (section 4): an empty prefix or local part, or a second
/// colon. The readers split such a name each its own way (`:x` is the default namespace's `x` to one, and `a:b:c` is `b:c` of `a`'s namespace).
fn not_a_qname(name: &str) -> bool {
    match name.split_once(':') {
        None => name.is_empty(),
        Some((prefix, local)) => prefix.is_empty() || local.is_empty() || local.contains(':'),
    }
}

/// Whether the attribute `key = value` breaks a constraint of XML Namespaces 1.0 (section 3): a declaration of a prefix to the empty URI (No Prefix Undeclaring), of `xml`
/// to another namespace than its own or of its namespace to another prefix or as the default, of `xmlns` or of its namespace (Reserved Prefixes and Namespace Names), a
/// declaration of an empty prefix or of one with a colon, and a name that is no qualified name ([`not_a_qname`]). `xml` declared as its own namespace is allowed.
///
/// Each of these is read otherwise by one reader of ours than by another (the drawing reader takes `xml` for its own namespace whatever is declared, and the sheet reader
/// binds it as declared; a prefix declared empty is no namespace to one and a foreign one to the other), so no reading of the check could be the same as both.
fn attribute_breaks(key: &str, value: &str) -> bool {
    if key == "xmlns" {
        return value == XML_NAMESPACE || value == XMLNS_NAMESPACE;
    }
    match key.strip_prefix("xmlns:") {
        Some(prefix) => {
            prefix.is_empty()
                || prefix.contains(':')
                || prefix == "xmlns"
                || value.is_empty()
                || value == XMLNS_NAMESPACE
                || (prefix == "xml") != (value == XML_NAMESPACE)
        }
        None => not_a_qname(key),
    }
}

/// Whether the name of an element breaks a constraint of XML Namespaces 1.0: it is no qualified name ([`not_a_qname`]), or its prefix is `xmlns` (section 3).
fn element_name_breaks(name: &str) -> bool {
    not_a_qname(name) || name.split_once(':').is_some_and(|(prefix, _)| prefix == "xmlns")
}

/// Whether the element `el` itself (its name and its attributes, not its children) breaks a constraint of XML Namespaces 1.0 ([`element_name_breaks`],
/// [`attribute_breaks`]).
fn element_breaks(el: &El) -> bool {
    element_name_breaks(&el.name) || el.attrs.iter().any(|(k, v)| attribute_breaks(k, v))
}

/// Whether an element of the tree of `root`, `root` included, breaks a constraint of XML Namespaces 1.0 ([`element_breaks`]). **A part that does is compared as bytes**, as a
/// part that does not parse is: its names are not ones the rules can read alike to every reader of ours. A walk on an explicit stack, counted: a step for each element
/// and each attribute. The walk that builds a part reads the same as it binds each element ([`Builder::bind`]); this one is for what that walk does not build (a child that counts
/// as nothing, and a part on one side only that may be no part).
fn breaks_namespaces(root: &El, work: &mut Work) -> bool {
    let mut stack = vec![root];
    let mut looked = 0usize;
    let mut broken = false;
    while let Some(el) = stack.pop() {
        looked = looked.saturating_add(1).saturating_add(el.attrs.len());
        if element_breaks(el) {
            broken = true;
            break;
        }
        stack.extend(el.children.iter().filter_map(|node| if let Node::El(child) = node { Some(child) } else { None }));
    }
    Work::add(&mut work.canon_steps, looked);
    broken
}

fn cmp_counted(a: &str, b: &str, work: &mut Work) -> Ordering {
    work.canon_bytes = work.canon_bytes.saturating_add(a.len().min(b.len()) as u64);
    a.cmp(b)
}

/// A paragraph's text as it is built (the white-space rules of ODF in the module documentation): one string, with the spaces of a run kept as a
/// count until a character or the end comes, so that the text costs what it is long, and not a piece for each word and each run.
struct Para {
    out: String,
    /// Spaces so far in the run that has not been written (a literal run is one; `text:s` adds its count).
    run: u128,
    /// The last thing was a white-space character that a run already stands for (or the paragraph's start, which drops its leading ones).
    in_run: bool,
    /// The last thing was a space a run of literal white space made, which the paragraph's end drops.
    trailing: bool,
}

impl Para {
    fn new(outermost: bool) -> Para {
        Para { out: String::new(), run: 0, in_run: outermost, trailing: false }
    }

    fn space(&mut self, n: u128) {
        // Saturating: a sum of u64 counts of at most six million elements cannot reach u128's end, and no wrap could unmake a difference.
        self.run = self.run.saturating_add(n);
    }

    /// The run, written: nothing for none, a space for one, a counted run for more.
    fn write_run(&mut self) {
        match self.run {
            0 => {}
            1 => self.out.push(' '),
            n => {
                self.out.push(RUN_OPEN);
                self.out.push_str(&n.to_string());
                self.out.push(RUN_CLOSE);
            }
        }
        self.run = 0;
    }

    fn text(&mut self, t: &str) {
        for c in t.chars() {
            if is_space(c) {
                if !self.in_run {
                    self.space(1);
                    self.in_run = true;
                    self.trailing = true;
                }
            } else {
                self.write_run();
                self.out.push(c);
                self.in_run = false;
                self.trailing = false;
            }
        }
    }

    /// `text:s`: spaces that no run absorbs and the end does not drop.
    fn explicit(&mut self, n: u128) {
        self.space(n);
        self.in_run = false;
        self.trailing = false;
    }

    /// An element or an instruction ends the run, and the text before it.
    fn barrier(&mut self) -> Option<String> {
        self.write_run();
        self.in_run = false;
        self.trailing = false;
        if self.out.is_empty() { None } else { Some(std::mem::take(&mut self.out)) }
    }

    /// The end of an outermost paragraph drops the last space a run made.
    fn finish(&mut self, outermost: bool) -> Option<String> {
        if outermost && self.trailing {
            self.run = self.run.saturating_sub(1);
        }
        self.barrier()
    }
}

/// How the children of one parent are placed ([`Rules::placed`]): the entry, and which children it reads.
#[derive(Clone, Copy)]
struct Placing<'r> {
    rule: &'r Placed,
    mode: Mode,
}

/// Which children of a parent have the place they leave out read ([`Placed`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Every child of the name (a kind in [`Placed::always`]).
    Always,
    /// Only a child that has no name ([`Placed::named_by`]): any other kind.
    Unnamed,
}

/// An element that is about to have its children built: its name, and how they are placed.
#[derive(Clone, Copy)]
struct Parent<'r, 'a> {
    name: QName<'a>,
    placing: Option<Placing<'r>>,
}

/// What the builder knows while it goes down the tree.
#[derive(Clone, Copy)]
struct Ctx {
    in_para: bool,
}

struct Builder<'r, 'a, 'w> {
    rules: &'r Rules,
    /// Prefix (empty for the default namespace) to the URIs it is bound to, innermost last.
    scopes: HashMap<&'a str, Vec<&'a str>>,
    /// Bytes the qualified values have expanded to so far, on this side of this part ([`MAX_QUALIFIED_BYTES`]).
    qualified: u64,
    work: &'w mut Work,
    /// What [`Rules::free_names`] found for this side: the children of the container, built, and the definitions a reference may name ([`names`]). None: every name as written.
    naming: Option<names::Naming<'a>>,
    /// How many elements of [`FreeNames::scopes`] the walk is inside: a reference there is not read.
    frozen: usize,
    /// Whether an element the walk read breaks a constraint of XML Namespaces 1.0 ([`element_breaks`]): the part is then not one the rules can read, and is compared as bytes.
    breaks: bool,
}

impl<'r, 'a, 'w> Builder<'r, 'a, 'w> {
    /// A builder with every name as written.
    fn new(rules: &'r Rules, work: &'w mut Work) -> Builder<'r, 'a, 'w> {
        Builder { rules, scopes: HashMap::new(), qualified: 0, work, naming: None, frozen: 0, breaks: false }
    }

    /// A builder that reads names as `naming` says ([`names::resolve`]), the qualified values it already expanded counted, and what the definitions it built broke known.
    fn with_naming(rules: &'r Rules, work: &'w mut Work, naming: Option<names::Naming<'a>>) -> Builder<'r, 'a, 'w> {
        let qualified = naming.as_ref().map_or(0, |n| n.qualified);
        let breaks = naming.as_ref().is_some_and(|n| n.breaks);
        Builder { rules, scopes: HashMap::new(), qualified, work, naming, frozen: 0, breaks }
    }

    /// Binds what `el` declares for the walk of its content, and gives the prefixes it bound, for [`Builder::unbind`]. Notes whether `el` breaks a constraint of XML
    /// Namespaces 1.0 ([`element_breaks`]), in the same pass over its attributes.
    fn bind(&mut self, el: &'a El) -> Vec<&'a str> {
        let mut bound = Vec::new();
        self.breaks |= element_name_breaks(&el.name);
        for (k, v) in &el.attrs {
            self.breaks |= attribute_breaks(k, v);
            if let Some(prefix) = declared_prefix(k) {
                self.scopes.entry(prefix).or_default().push(v);
                bound.push(prefix);
            }
        }
        bound
    }

    fn unbind(&mut self, bound: Vec<&'a str>) {
        for prefix in bound {
            if let Some(stack) = self.scopes.get_mut(prefix) {
                stack.pop();
            }
        }
    }

    fn steps(&mut self, n: usize) {
        Work::add(&mut self.work.canon_steps, n);
    }

    fn bytes(&mut self, n: usize) {
        Work::add(&mut self.work.canon_bytes, n);
    }

    fn lookup(&self, prefix: &str) -> Option<&'a str> {
        self.scopes.get(prefix).and_then(|stack| stack.last().copied()).filter(|uri| !uri.is_empty())
    }

    /// The expanded name of the element `el`, read through its own declarations first, then the ones in force where it stands.
    fn expand_with(&self, el: &'a El, name: &'a str) -> QName<'a> {
        self.expand_in(&[el], name)
    }

    /// The expanded name of the element written `name`, read through the declarations of each of `els` (the innermost first), then the ones in force, as [`Builder::expand`]
    /// reads it once they are bound ([`Builder::bound_in`]): `xmlns=""` puts it in no namespace (an own declaration of the empty URI was read as no declaration, and
    /// the default namespace in force taken for it).
    fn expand_in(&self, els: &[&'a El], name: &'a str) -> QName<'a> {
        let (prefix, local) = match name.split_once(':') {
            None => ("", name),
            Some(("xml", local)) => return QName { ns: XML_NAMESPACE, local },
            Some((prefix, local)) => (prefix, local),
        };
        match self.bound_in(els, prefix) {
            Some(uri) => QName { ns: uri, local },
            None if prefix.is_empty() => QName { ns: "", local },
            None => QName { ns: "", local: name },
        }
    }

    /// The URI `prefix` (`""` for the default namespace) is bound to where the elements `els` stand: their own declarations first (the innermost first), then the ones in force,
    /// as XML Namespaces 1.0 reads them. The nearest declaration decides, and one of the empty URI binds nothing: `xmlns=""` leaves an unprefixed name in no namespace, and
    /// `xmlns:p=""`, which 1.0 does not allow (the part is then bytes, [`attribute_breaks`]), leaves `p` unbound, as [`Builder::lookup`] reads both once they are bound.
    fn bound_in(&self, els: &[&'a El], prefix: &str) -> Option<&'a str> {
        let own = els.iter().find_map(|el| el.attrs.iter().rev().find(|(k, _)| declared_prefix(k) == Some(prefix)).map(|(_, v)| v.as_str()));
        match own {
            Some(uri) => Some(uri).filter(|uri| !uri.is_empty()),
            None => self.lookup(prefix),
        }
    }

    /// The expanded name of an element or an attribute written `name`.
    fn expand(&self, name: &'a str, attribute: bool) -> QName<'a> {
        match name.split_once(':') {
            None if attribute => QName { ns: "", local: name },
            None => QName { ns: self.lookup("").unwrap_or(""), local: name },
            Some(("xml", local)) => QName { ns: XML_NAMESPACE, local },
            Some((prefix, local)) => match self.lookup(prefix) {
                Some(uri) => QName { ns: uri, local },
                // Nothing binds the prefix: the name as written, which no bound name can be (a bound name's local part has no colon).
                None => QName { ns: "", local: name },
            },
        }
    }

    /// A value that names namespaces by prefix, as the namespaces the prefixes are bound to in scope: a bound name is `RUN_OPEN`, the URI and
    /// `RUN_CLOSE` and then the local name, and one nothing binds is the text as written after `UNBOUND`. None of the three characters can be in
    /// a value a parser gives, and every token begins with `RUN_OPEN` or `UNBOUND`, so the tokens need no separator: two values are the same
    /// string only when they are the same list of tokens (`x y` unbound is not `xy` unbound).
    ///
    /// Each token is charged before it is built ([`MAX_QUALIFIED_BYTES`]): the expansion is what the URIs make it, not what the value is long.
    fn through_scope(&mut self, value: &str, kind: QKind) -> Result<String, Refusal> {
        let mut out = String::new();
        for token in value.split(is_space).filter(|t| !t.is_empty()) {
            let (prefix, local) = match kind {
                QKind::Prefixes => (Some(token), ""),
                QKind::QNames => match token.split_once(':') {
                    Some((prefix, local)) => (Some(prefix), local),
                    None => (None, token),
                },
            };
            let uri = match prefix {
                Some("xml") => Some(XML_NAMESPACE),
                Some(prefix) => self.lookup(prefix),
                None => Some(self.lookup("").unwrap_or("")),
            };
            // The markers are one byte each.
            let len = match uri {
                Some(uri) => 2usize.saturating_add(uri.len()).saturating_add(local.len()),
                None => 1usize.saturating_add(token.len()),
            };
            self.qualified = self.qualified.saturating_add(u64::try_from(len).unwrap_or(u64::MAX));
            if self.qualified > MAX_QUALIFIED_BYTES {
                return Err(Refusal::Values);
            }
            self.bytes(len);
            match uri {
                Some(uri) => {
                    out.push(RUN_OPEN);
                    out.push_str(uri);
                    out.push(RUN_CLOSE);
                    out.push_str(local);
                }
                None => {
                    out.push(UNBOUND);
                    out.push_str(token);
                }
            }
        }
        Ok(out)
    }

    /// Whether the attribute `name`, written with `value` on `el`, is one [`Rules::twinned`] lists beside its twin of the same value on the same element (the names read through the
    /// declarations in force). The look at the element's other attributes is counted, as the walk of them is.
    fn is_twin(&mut self, el: &'a El, name: QName<'a>, value: &str) -> bool {
        let rules = self.rules;
        if !rules.twinned.iter().any(|t| name.is(&t.attr)) {
            return false;
        }
        self.steps(el.attrs.len());
        rules
            .twinned
            .iter()
            .any(|t| name.is(&t.attr) && el.attrs.iter().any(|(k, v)| v == value && !k.starts_with("xmlns") && self.expand(k, true).is(&t.with)))
    }

    /// The attributes of `el` as they compare: declarations left out, names expanded, numbers read where the rules say, sorted.
    fn attributes(&mut self, el: &'a El, owner: QName<'a>) -> Result<Vec<CAttr<'a>>, Refusal> {
        let mut attrs = Vec::with_capacity(el.attrs.len());
        for (k, v) in &el.attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                continue;
            }
            let name = self.expand(k, true);
            // An attribute the format says counts as nothing (the `office:version`) is no part of what the element is compared by.
            if self.rules.ignored_attributes.iter().any(|n| name.is(n)) {
                continue;
            }
            let length = self.rules.lengths.iter().any(|n| name.is(n));
            let numeric = length || self.rules.numbers.iter().any(|n| name.is(n));
            let qualified = self.rules.qnames.iter().find(|q| name.is(&q.attr) && q.on.is_none_or(|on| owner.is(&on)));
            let val = match numeric.then(|| read_number(v, length)).flatten() {
                Some((bits, unit)) => Val::Num(bits, unit),
                None => match qualified {
                    Some(q) => Val::Owned(self.through_scope(v, q.kind)?),
                    // A name that a reference gives to a definition that has a match on the other side is read as the definition's content.
                    None => match self.reference(name, owner, v) {
                        Some(read) => Val::Owned(read),
                        None => Val::Text(v),
                    },
                },
            };
            self.bytes(v.len());
            let quiet = self.rules.ignored_when_extra.contains(&name.ns) || self.is_twin(el, name, v);
            attrs.push(CAttr { name, val, quiet });
        }
        self.steps(attrs.len());
        // By name, and the copies of one name (one attribute under two prefixes of one namespace, which XML Namespaces 1.0 does not allow) in the order written: a reader takes
        // the first, so another order is another reading (they were sorted by value, and two documents that swapped the values were the same). The sort is stable.
        let mut compared = 0u64;
        attrs.sort_by(|x, y| {
            compared += 1;
            x.name.cmp(&y.name)
        });
        self.work.canon_steps = self.work.canon_steps.saturating_add(compared);
        Ok(attrs)
    }

    /// `el` in its canonical form. `depth` is the element's own depth, and the tree the parser gave is never past the ceiling.
    fn build(&mut self, el: &'a El, ctx: Ctx, depth: usize) -> Result<Canon<'a>, Refusal> {
        if depth > XML_DEPTH_CEILING {
            return Err(XmlError::TooDeep { limit: XML_DEPTH_CEILING }.into());
        }
        self.steps(1);
        let bound = self.bind(el);
        let name = self.expand(&el.name, false);
        let attrs = self.attributes(el, name)?;
        // An element whose names are its own (an embedded document): no reference below it names a definition of this one.
        let scope = depth > 0 && self.naming.is_some() && self.rules.free_names.iter().any(|f| f.scopes.iter().any(|n| name.is(n)));
        self.frozen += usize::from(scope);
        let built = self.children(el, name, ctx, depth);
        self.frozen -= usize::from(scope);
        self.unbind(bound);
        let (items, keyed) = built?;
        Ok(seal(name, attrs, items, keyed))
    }

    /// What the attribute `attr` of the element `owner` reads as with `value` when it names a definition that has a match on the other side ([`FreeNames`]): its class, or the names of a
    /// list with each class that has one.
    fn reference(&self, attr: QName<'a>, owner: QName<'a>, value: &'a str) -> Option<String> {
        let naming = self.naming.as_ref().filter(|_| self.frozen == 0)?;
        let rule = self.rules.free_names.first()?;
        naming.read(rule.references, owner, attr, value)
    }

    /// The children of `el`: comments gone, text joined across them, split into runs and elements.
    fn segments(&mut self, el: &'a El) -> Vec<Seg<'a>> {
        fn flush<'a>(out: &mut Vec<Seg<'a>>, pending: &mut Option<Cow<'a, str>>) {
            if let Some(text) = pending.take().filter(|t| !t.is_empty()) {
                out.push(Seg::Text(text));
            }
        }
        let mut out = Vec::new();
        let mut pending: Option<Cow<'a, str>> = None;
        for node in &el.children {
            match node {
                Node::Text(t) | Node::CData(t) => match &mut pending {
                    Some(text) => text.to_mut().push_str(t),
                    None => pending = Some(Cow::Borrowed(t.as_str())),
                },
                Node::Comment(_) => {}
                Node::El(e) => {
                    flush(&mut out, &mut pending);
                    out.push(Seg::El(e));
                }
                Node::Pi { target, data } => {
                    flush(&mut out, &mut pending);
                    out.push(Seg::Pi(target, data));
                }
            }
        }
        flush(&mut out, &mut pending);
        out
    }

    #[allow(clippy::type_complexity)] // the two lists a canonical element holds, named where they are made
    fn children(&mut self, el: &'a El, name: QName<'a>, ctx: Ctx, depth: usize) -> Result<(Vec<Item<'a>>, Vec<KeyedEl<'a>>), Refusal> {
        let para = self.rules.paragraphs.iter().any(|n| name.is(n));
        let inner = Ctx { in_para: ctx.in_para || para };
        let segs = self.segments(el);
        let mut items = Vec::new();
        let mut keyed = Vec::new();
        if para {
            self.paragraph(segs, !ctx.in_para, inner, depth, &mut items, &mut keyed)?;
        } else {
            let parent = Parent { name, placing: self.placing(el, name) };
            self.plain(segs, parent, inner, depth, &mut items, &mut keyed)?;
        }
        let mut work = (0u64, 0u64);
        keyed.sort_by(|x: &KeyedEl<'a>, y: &KeyedEl<'a>| {
            work.0 += 1;
            work.1 += x.key_bytes.min(y.key_bytes) as u64;
            x.class.cmp(&y.class).then_with(|| x.key.cmp(&y.key))
        });
        self.work.canon_steps = self.work.canon_steps.saturating_add(work.0);
        self.work.canon_bytes = self.work.canon_bytes.saturating_add(work.1);
        Ok((items, keyed))
    }

    /// Children of an element that is not a paragraph: blank text between children is ignored when the element holds no other text.
    fn plain(
        &mut self,
        segs: Vec<Seg<'a>>,
        parent: Parent<'r, 'a>,
        ctx: Ctx,
        depth: usize,
        items: &mut Vec<Item<'a>>,
        keyed: &mut Vec<KeyedEl<'a>>,
    ) -> Result<(), Refusal> {
        let Parent { name, placing } = parent;
        let has_child = segs.iter().any(|s| !matches!(s, Seg::Text(_)));
        let has_text = segs.iter().any(|s| matches!(s, Seg::Text(t) if !is_blank(t)));
        let exact = self.rules.exact_text.iter().any(|n| name.is(n));
        let drop_blank = has_child && !has_text && !exact;
        // The children [`Rules::absent_when_empty`] names: where each empty one stands among the items (`voids`, a place each, in order), and the names of those that
        // hold something (at most one for each name of the list).
        let (mut voids, mut solid): (Vec<usize>, Vec<QName<'a>>) = (Vec::new(), Vec::new());
        // The children of the name [`Rules::placed`] gives so far.
        let mut rows = 0usize;
        for seg in segs {
            match seg {
                Seg::Text(t) if drop_blank && is_blank(&t) => {}
                Seg::Text(t) => {
                    self.bytes(t.len());
                    let txt = Txt::Plain(t);
                    let h = txt.hash();
                    items.push(Item::Text(txt, h));
                }
                Seg::El(e) => {
                    // The container's children were built, with the names of the definitions read, before the walk began ([`names::resolve`]).
                    let prebuilt = if depth == 1 { self.naming.as_mut().and_then(|n| n.next_child(self.rules.free_names, name)) } else { None };
                    // A child that counts as nothing under this parent (the generator) is not read, but for whether it breaks a constraint of XML Namespaces 1.0: a
                    // part that does is bytes, wherever the break stands.
                    if self.is_ignored_child(name, e) {
                        self.steps(1);
                        self.breaks |= breaks_namespaces(e, self.work);
                        continue;
                    }
                    let mut c = match prebuilt {
                        Some(names::Slot::Built(built)) => built,
                        // A copy of a definition that is already there counts as that one.
                        Some(names::Slot::Dropped) => {
                            self.steps(1);
                            continue;
                        }
                        None => self.build(e, ctx, depth + 1)?,
                    };
                    if let Some(placing) = placing
                        && c.name.is(&placing.rule.child)
                    {
                        c = self.place_in_order(c, placing, rows);
                        rows += 1;
                    }
                    // Not inside a paragraph, however deep: its content is text and marks, and an element in it is content. Listed anywhere, or under this parent.
                    let listed = !ctx.in_para
                        && (self.rules.absent_when_empty.iter().any(|n| c.name.is(n))
                            || self.rules.absent_when_empty_under.iter().any(|u| name.is(&u.parent) && u.children.iter().any(|n| c.name.is(n))));
                    let (name, at) = (c.name, items.len());
                    let void = listed && self.holds_nothing(e, name);
                    self.place(c, items, keyed);
                    if listed {
                        if !void {
                            if !solid.contains(&name) {
                                solid.push(name);
                            }
                        } else if items.len() > at {
                            voids.push(at);
                        }
                    }
                }
                Seg::Pi(target, data) => items.push(pi(target, data)),
            }
        }
        // Left out once every copy of a name is known: an empty one beside a full one of its name stays.
        voids.retain(|at| matches!(items.get(*at), Some(Item::El(c)) if !solid.contains(&c.name)));
        if !voids.is_empty() {
            let mut gone = voids.iter().copied().peekable();
            let mut index = 0usize;
            items.retain(|_| {
                let leave = gone.peek() == Some(&index);
                if leave {
                    gone.next();
                }
                index += 1;
                !leave
            });
            self.steps(index);
        }
        if let Some(rule) = self.rules.unordered.iter().find(|u| name.is(&u.parent)) {
            self.reorder(items, rule.children);
        }
        Ok(())
    }

    /// How the children of `el` (named `name`) are placed, by the first entry of [`Rules::placed`] that names it, or none: the parent's kind, read as the entry says
    /// (a parent of a kind in `never` has no placing).
    fn placing(&self, el: &'a El, name: QName<'a>) -> Option<Placing<'r>> {
        let rule = self.rules.placed.iter().find(|p| name.is(&p.parent))?;
        let kind = el.attrs.iter().find(|(k, _)| declared_prefix(k).is_none() && self.expand(k, true).is(&rule.kind)).map(|(_, v)| v.as_str());
        let mode = match kind {
            Some(k) if rule.always.contains(&k) => Mode::Always,
            Some(k) if rule.never.contains(&k) => return None,
            _ => Mode::Unnamed,
        };
        Some(Placing { rule, mode })
    }

    /// `c`, a child of the name [`Placed::child`], with the place it leaves out read as `before`, the count of the children of its name before it: when its parent's
    /// kind reads it so (`placing`) and it has no place of its own (and, in a kind that finds only the children with no name, no name). Counted.
    fn place_in_order(&mut self, c: Canon<'a>, placing: Placing<'r>, before: usize) -> Canon<'a> {
        let rule = placing.rule;
        let has = |n: &Name| c.attrs.iter().any(|a| a.name.is(n));
        if has(&rule.place) || (placing.mode == Mode::Unnamed && has(&rule.named_by)) {
            return c;
        }
        let value = before.to_string();
        self.steps(1);
        self.bytes(value.len());
        let attr = CAttr { name: QName { ns: rule.place.ns, local: rule.place.local }, val: Val::Owned(value), quiet: false };
        with_attr(c, attr)
    }

    /// The children of an [`Unordered`] set among `items`, in the set's order and each name's copies in the order written, put in the places the set's
    /// children had: every other item stays where it is. Counted: a step for each item, and one for each comparison of the sort.
    fn reorder(&mut self, items: &mut Vec<Item<'a>>, children: &[Name]) {
        let rank = |item: &Item<'a>| match item {
            Item::El(c) => children.iter().position(|n| c.name.is(n)),
            Item::Text(..) | Item::Pi(..) => None,
        };
        if items.iter().filter(|item| rank(item).is_some()).take(2).count() < 2 {
            return;
        }
        let taken = std::mem::take(items);
        self.steps(taken.len());
        // Each item, or a hole where one of the set was; the set's children, with their rank and their place.
        let mut slots: Vec<Option<Item<'a>>> = Vec::with_capacity(taken.len());
        let mut listed: Vec<(usize, usize, Item<'a>)> = Vec::new();
        for (place, item) in taken.into_iter().enumerate() {
            match rank(&item) {
                Some(r) => {
                    listed.push((r, place, item));
                    slots.push(None);
                }
                None => slots.push(Some(item)),
            }
        }
        let mut compared = 0u64;
        listed.sort_by(|x, y| {
            compared += 1;
            (x.0, x.1).cmp(&(y.0, y.1))
        });
        self.work.canon_steps = self.work.canon_steps.saturating_add(compared);
        let mut next = listed.into_iter().map(|(_, _, item)| item);
        items.reserve(slots.len());
        for slot in slots {
            match slot {
                Some(item) => items.push(item),
                None => items.extend(next.next()),
            }
        }
    }

    /// Whether `e` holds nothing: no attribute (a namespace declaration is none), no child element, no processing instruction and no text but white space.
    /// A comment is no content. Read on the tree the parser gave, so a child that the rule itself leaves out still counts as a child: only a container
    /// that was written empty is one. Counted by what it looks at: it stops at the first thing that is content, so an element that holds a great deal costs
    /// a few steps, and one that holds nothing but a great deal of white space costs what that is.
    fn holds_nothing(&mut self, e: &'a El, name: QName<'a>) -> bool {
        let (mut empty, mut looked, mut text) = (true, 1usize, 0usize);
        for (key, _) in &e.attrs {
            looked = looked.saturating_add(1);
            if key != "xmlns" && !key.starts_with("xmlns:") && !self.is_ignored(e, key) {
                empty = false;
                break;
            }
        }
        if empty {
            for node in &e.children {
                looked = looked.saturating_add(1);
                match node {
                    Node::Comment(_) => {}
                    Node::Text(t) | Node::CData(t) => {
                        text = text.saturating_add(t.len());
                        if !is_blank(t) {
                            empty = false;
                            break;
                        }
                    }
                    // A child that counts as nothing under this parent holds nothing (the generator), read through the declarations of `e` and of the document.
                    Node::El(child) if self.is_ignored_in(name, child, e) => {}
                    Node::El(_) | Node::Pi { .. } => {
                        empty = false;
                        break;
                    }
                }
            }
        }
        self.steps(looked);
        self.bytes(text);
        empty
    }

    /// Whether `child`, a child of the element `parent` (named `name`), is one [`Rules::ignored_children`] lists under that name, its prefix read through its own declarations
    /// and then `parent`'s, whose declarations are not in force any more.
    fn is_ignored_in(&self, name: QName<'a>, child: &'a El, parent: &'a El) -> bool {
        if self.rules.ignored_children.is_empty() {
            return false;
        }
        let child_name = self.expand_in(&[child, parent], &child.name);
        self.rules.ignored_children.iter().any(|u| name.is(&u.parent) && u.children.iter().any(|n| child_name.is(n)))
    }

    /// Whether `child`, a child of the element being built (named `parent`, whose declarations are in force), is one [`Rules::ignored_children`] lists under that name.
    fn is_ignored_child(&self, parent: QName<'a>, child: &'a El) -> bool {
        if self.rules.ignored_children.is_empty() {
            return false;
        }
        let child_name = self.expand_with(child, &child.name);
        self.rules.ignored_children.iter().any(|u| parent.is(&u.parent) && u.children.iter().any(|n| child_name.is(n)))
    }

    /// Whether the attribute `key` of `e` is one [`Rules::ignored_attributes`] lists: in no namespace without a prefix, its prefix read through `e`'s own declarations first, then
    /// the ones in force where `e` stands, as an element's name is ([`Builder::expand_in`]; a prefix bound to nothing is the name as written, which no list holds). A rule that
    /// lists none reads nothing.
    fn is_ignored(&self, e: &'a El, key: &'a str) -> bool {
        if self.rules.ignored_attributes.is_empty() {
            return false;
        }
        let name = if key.contains(':') { self.expand_in(&[e], key) } else { QName { ns: "", local: key } };
        self.rules.ignored_attributes.iter().any(|n| name.is(n))
    }

    /// Children of a paragraph element.
    fn paragraph(
        &mut self,
        segs: Vec<Seg<'a>>,
        outermost: bool,
        ctx: Ctx,
        depth: usize,
        items: &mut Vec<Item<'a>>,
        keyed: &mut Vec<KeyedEl<'a>>,
    ) -> Result<(), Refusal> {
        let mut para = Para::new(outermost);
        for seg in segs {
            match seg {
                Seg::Text(t) => {
                    self.bytes(t.len());
                    para.text(&t);
                }
                Seg::El(e) => {
                    let c = self.build(e, ctx, depth + 1)?;
                    if let Some(n) = spaces(&c) {
                        para.explicit(n);
                    } else {
                        self.push_text(items, para.barrier());
                        self.place(c, items, keyed);
                    }
                }
                Seg::Pi(target, data) => {
                    self.push_text(items, para.barrier());
                    items.push(pi(target, data));
                }
            }
        }
        self.push_text(items, para.finish(outermost));
        Ok(())
    }

    /// A paragraph's text, once built, goes to the items; the bytes built are counted, as the bytes read are.
    fn push_text(&mut self, items: &mut Vec<Item<'a>>, text: Option<String>) {
        if let Some(text) = text {
            self.bytes(text.len());
            let txt = Txt::Para(text);
            let h = txt.hash();
            items.push(Item::Text(txt, h));
        }
    }

    /// A built child goes to its keyed set when a rule names it, else to the items in order.
    fn place(&mut self, c: Canon<'a>, items: &mut Vec<Item<'a>>, keyed: &mut Vec<KeyedEl<'a>>) {
        // An element of a namespace that counts as nothing when extra is set apart, after the keyed ones.
        if self.rules.ignored_when_extra.contains(&c.name.ns) {
            keyed.push(KeyedEl { class: QUIET, key: Vec::new(), key_bytes: 0, el: c });
            return;
        }
        let class = self.rules.keyed.iter().position(|k| c.name.is(&k.child));
        match class.and_then(|class| self.rules.keyed.get(class).map(|rule| (class, rule))) {
            Some((class, rule)) => {
                let key: Vec<Option<Val<'a>>> =
                    rule.key.iter().map(|n| c.attrs.iter().find(|a| a.name.is(n)).map(|a| fold_key(&a.val, rule.fold))).collect();
                let key_bytes = key.iter().flatten().map(Val::len).sum();
                keyed.push(KeyedEl { class, key, key_bytes, el: c });
            }
            None => items.push(Item::El(Box::new(c))),
        }
    }
}

/// A key as its format's reader folds it.
fn fold_key<'a>(val: &Val<'a>, fold: KeyFold) -> Val<'a> {
    match (fold, val) {
        (KeyFold::AsciiCase, Val::Text(t)) => Val::Owned(t.to_ascii_lowercase()),
        (KeyFold::PartName, Val::Text(t)) => {
            let mut name = lenient_decode(t);
            if !name.starts_with('/') {
                name.insert(0, '/');
            }
            name.make_ascii_lowercase();
            Val::Owned(name)
        }
        _ => val.clone(),
    }
}

/// The text with its `%XX` escapes decoded, when they decode to UTF-8 and none hides a slash or a backslash; as it is when they do not
/// (OPC's reader decodes the names in its rows this way, to look a part up).
fn lenient_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut smuggled = false;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        // Two hex digits after the `%` and nothing else (`u8::from_str_radix` would take a `+` before one digit, which OPC's reader does not).
        let digits = (bytes.get(i + 1).copied().and_then(hex_digit), bytes.get(i + 2).copied().and_then(hex_digit));
        match digits {
            (Some(high), Some(low)) if b == b'%' => {
                let value = high * 16 + low;
                smuggled |= value == b'/' || value == b'\\';
                out.push(value);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    if smuggled { s.to_owned() } else { String::from_utf8(out).unwrap_or_else(|_| s.to_owned()) }
}

/// The value of a hexadecimal digit (either case), or none.
fn hex_digit(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

fn pi<'a>(target: &'a str, data: &'a str) -> Item<'a> {
    let mut h = Hash::new(b'P');
    h.str(target);
    h.str(data);
    Item::Pi(target, data, h.finish())
}

/// The number of spaces `c` stands for when it is a `text:s` that holds nothing but its `text:c` (a count of digits, with an optional `+`,
/// one when absent); none for anything else, which is then an element like any other.
fn spaces(c: &Canon<'_>) -> Option<u128> {
    let ours = c.name.ns == ODF_TEXT_NAMESPACE && c.name.local == "s";
    if !ours || !c.items.is_empty() || !c.keyed.is_empty() {
        return None;
    }
    match c.attrs.as_slice() {
        [] => Some(1),
        [CAttr { name, val: Val::Text(v), .. }] if name.ns == ODF_TEXT_NAMESPACE && name.local == "c" => {
            let digits = v.strip_prefix('+').unwrap_or(v);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u64>().ok().map(u128::from)
        }
        _ => None,
    }
}

/// `c` with one more attribute (one it has not), put in its place among the sorted ones, and sealed again.
fn with_attr<'a>(c: Canon<'a>, attr: CAttr<'a>) -> Canon<'a> {
    let Canon { name, mut attrs, items, keyed, .. } = c;
    let at = attrs.partition_point(|a| a.name < attr.name);
    attrs.insert(at, attr);
    seal(name, attrs, items, keyed)
}

/// The element with its hash.
fn seal<'a>(name: QName<'a>, attrs: Vec<CAttr<'a>>, items: Vec<Item<'a>>, keyed: Vec<KeyedEl<'a>>) -> Canon<'a> {
    let mut h = Hash::new(b'E');
    h.str(name.ns);
    h.str(name.local);
    // What counts as nothing when extra (a quiet attribute, a quiet element) is no part of the hash: it only aligns the children, and a child that was marked is the child it was.
    h.u64(attrs.iter().filter(|a| !a.quiet).count() as u64);
    for a in attrs.iter().filter(|a| !a.quiet) {
        h.str(a.name.ns);
        h.str(a.name.local);
        a.val.feed(&mut h);
    }
    h.u64(items.len() as u64);
    for item in &items {
        h.u64(item.hash());
    }
    h.u64(keyed.iter().filter(|k| k.class != QUIET).count() as u64);
    for k in keyed.iter().filter(|k| k.class != QUIET) {
        h.u64(k.class as u64);
        h.u64(k.el.hash);
    }
    Canon { name, attrs, items, keyed, hash: h.finish() }
}

// ------------------------------------------------------------------------------------------------------------------- the comparison

#[derive(Default)]
struct Sink {
    shown: Vec<Difference>,
    count: u64,
    part: String,
}

impl Sink {
    fn add(&mut self, found: Found, path: impl FnOnce() -> String) {
        self.count = self.count.saturating_add(1);
        if self.shown.len() < MAX_SHOWN {
            self.shown.push(Difference { part: self.part.clone(), path: path(), found });
        }
    }

    fn part_only(&mut self, found: Found, name: Option<&str>) {
        self.part = label(name.unwrap_or_default());
        self.add(found, String::new);
    }
}

/// A part's name as a difference shows it: no control character, and not long.
fn label(name: &str) -> String {
    name.chars().take(MAX_PART_IN_DIFFERENCE).map(|c| if c.is_control() { '?' } else { c }).collect()
}

#[derive(Clone, Copy)]
enum Mark {
    /// `local[i]`: the `i`th item of its parent.
    Item,
    /// `local#i`: the `i`th keyed child of its parent.
    Keyed,
    /// `/@local`.
    Attr,
}

#[derive(Clone, Copy)]
struct PathSeg<'t> {
    local: &'t str,
    mark: Mark,
    index: usize,
}

fn render(path: &[PathSeg<'_>], tail: Option<PathSeg<'_>>) -> String {
    let mut out = String::new();
    for seg in path.iter().copied().chain(tail) {
        if out.len() >= MAX_PATH_BYTES {
            break;
        }
        out.push('/');
        if matches!(seg.mark, Mark::Attr) {
            out.push('@');
        }
        out.extend(seg.local.chars().take(MAX_NAME_IN_PATH));
        match seg.mark {
            Mark::Item => out.push_str(&format!("[{}]", seg.index)),
            Mark::Keyed => out.push_str(&format!("#{}", seg.index)),
            Mark::Attr => {}
        }
    }
    // A path that ran past the cap, or stopped at it, ends in `/...`, and is never longer than the cap.
    if out.len() >= MAX_PATH_BYTES {
        out.truncate(out.floor_char_boundary(MAX_PATH_BYTES - 4));
        out.push_str("/...");
    }
    out
}

/// The two lists of a middle and what [`align`] made of them.
struct Place<'p, 't> {
    a: &'p [Item<'t>],
    b: &'p [Item<'t>],
    /// The place of the middle among its parent's children.
    base: usize,
    /// For each child of the first list, the place of its match in the second.
    a_partner: &'p [Option<usize>],
    /// For each child of the second list, whether it is matched.
    b_matched: &'p [bool],
}

struct Cmp<'t, 'w> {
    sink: &'w mut Sink,
    path: Vec<PathSeg<'t>>,
    work: &'w mut Work,
    rules: &'w Rules,
}

impl<'t> Cmp<'t, '_> {
    fn step(&mut self) {
        self.work.canon_steps = self.work.canon_steps.saturating_add(1);
    }

    fn record(&mut self, found: Found, tail: Option<PathSeg<'t>>) {
        let path = &self.path;
        self.sink.add(found, || render(path, tail));
    }

    fn element(&mut self, a: &Canon<'t>, b: &Canon<'t>, seg: PathSeg<'t>) {
        if a.name != b.name {
            self.record(Found::Element, Some(seg));
            return;
        }
        self.path.push(seg);
        self.step();
        self.attributes(&a.attrs, &b.attrs);
        self.items(&a.items, &b.items);
        self.keyed(&a.keyed, &b.keyed);
        self.path.pop();
    }

    fn attributes(&mut self, a: &[CAttr<'t>], b: &[CAttr<'t>]) {
        let (mut i, mut j) = (0, 0);
        loop {
            self.step();
            let at = |seg: &CAttr<'t>| PathSeg { local: seg.name.local, mark: Mark::Attr, index: 0 };
            match (a.get(i), b.get(j)) {
                (None, None) => break,
                (Some(x), None) => {
                    self.record(Found::Attribute, Some(at(x)));
                    i += 1;
                }
                (None, Some(y)) => {
                    // What only the second document holds of a namespace that counts as nothing when extra is no difference.
                    if !y.quiet {
                        self.record(Found::Attribute, Some(at(y)));
                    }
                    j += 1;
                }
                (Some(x), Some(y)) => match x.name.cmp(&y.name) {
                    Ordering::Less => {
                        self.record(Found::Attribute, Some(at(x)));
                        i += 1;
                    }
                    Ordering::Greater => {
                        if !y.quiet {
                            self.record(Found::Attribute, Some(at(y)));
                        }
                        j += 1;
                    }
                    Ordering::Equal => {
                        Work::add(&mut self.work.canon_bytes, x.val.len().min(y.val.len()));
                        if x.val != y.val {
                            self.record(Found::Value, Some(at(x)));
                        }
                        i += 1;
                        j += 1;
                    }
                },
            }
        }
    }

    /// The ordered children: the start and the end that match (by hash) are paired, and what is between is paired by place, or is one
    /// [`Found::Order`] when it is the same children in another order. Every pair is compared in full.
    fn items(&mut self, a: &[Item<'t>], b: &[Item<'t>]) {
        let start = a.iter().zip(b).take_while(|(x, y)| x.hash() == y.hash()).count();
        let (a_start, a_rest) = a.split_at(start.min(a.len()));
        let (b_start, b_rest) = b.split_at(start.min(b.len()));
        let end = a_rest.iter().rev().zip(b_rest.iter().rev()).take_while(|(x, y)| x.hash() == y.hash()).count();
        let (a_mid, a_end) = a_rest.split_at(a_rest.len().saturating_sub(end));
        let (b_mid, b_end) = b_rest.split_at(b_rest.len().saturating_sub(end));
        self.pairs(a_start, b_start, 0);
        self.middle(a_mid, b_mid, start);
        self.pairs(a_end, b_end, start.saturating_add(a_mid.len()));
    }

    fn pairs(&mut self, a: &[Item<'t>], b: &[Item<'t>], base: usize) {
        for (k, (x, y)) in a.iter().zip(b).enumerate() {
            self.pair(x, y, base.saturating_add(k));
        }
    }

    /// What lies between the start and the end that match: the children of each side are set in order by what they are ([`align`]), each pair is compared in full, the same
    /// children in another order are one [`Found::Order`], and what is matched by nothing is, between two anchors, set side by side by place (a replaced child is one difference) or
    /// is put in or taken out. A list longer than [`MAX_ALIGNED`] is set side by side by place, and counted.
    fn middle(&mut self, a: &[Item<'t>], b: &[Item<'t>], base: usize) {
        // Nothing to match: one side holds nothing (what the other holds is put in or taken out) or one child each (a replaced child, or a changed one, compared in full).
        match (a, b) {
            ([x], [y]) => self.pair(x, y, base),
            ([], rest) | (rest, []) => {
                for (k, item) in rest.iter().enumerate() {
                    self.alone(item, base.saturating_add(k));
                }
            }
            _ if a.len().max(b.len()) > MAX_ALIGNED => {
                Work::add(&mut self.work.canon_by_place, a.len().saturating_add(b.len()));
                self.by_place(a, b, base);
            }
            _ => self.aligned(a, b, base),
        }
    }

    /// The middle of two lists that are set side by side by what they are ([`Cmp::middle`]).
    fn aligned(&mut self, a: &[Item<'t>], b: &[Item<'t>], base: usize) {
        let (fa, fb) = (self.facets(a), self.facets(b));
        let aligned = align::align(&fa, &fb, !self.rules.identified_by.is_empty(), self.work);
        if aligned.crossed {
            self.record(Found::Order, None);
        }
        let place = Place { a, b, base, a_partner: &aligned.a_partner, b_matched: &aligned.b_matched };
        let (mut from_a, mut from_b) = (0usize, 0usize);
        for &(end_a, end_b) in &aligned.anchors {
            self.gap(&place, from_a..end_a, from_b..end_b);
            if let (Some(x), Some(y)) = (a.get(end_a), b.get(end_b)) {
                self.pair(x, y, base.saturating_add(end_a));
            }
            (from_a, from_b) = (end_a.saturating_add(1), end_b.saturating_add(1));
        }
        self.gap(&place, from_a..a.len(), from_b..b.len());
    }

    /// The children between two anchors: the ones that are matched by a pair that crosses another are compared with their match; the others, none of which a key matched, are set
    /// side by side by place, first with first, and what is left of one side is put in or taken out.
    fn gap(&mut self, place: &Place<'_, 't>, in_a: std::ops::Range<usize>, in_b: std::ops::Range<usize>) {
        let mut free_b = in_b.filter(|&j| !place.b_matched.get(j).copied().unwrap_or(true));
        for i in in_a {
            let Some(x) = place.a.get(i) else { continue };
            let partner = match place.a_partner.get(i).copied().flatten() {
                Some(j) => Some(j),
                None => free_b.next(),
            };
            match partner.and_then(|j| place.b.get(j)) {
                Some(y) => self.pair(x, y, place.base.saturating_add(i)),
                None => self.alone(x, place.base.saturating_add(i)),
            }
        }
        for j in free_b {
            if let Some(y) = place.b.get(j) {
                self.alone(y, place.base.saturating_add(j));
            }
        }
    }

    /// What the alignment needs to know of each child of `items`: what it holds, what it is called and, when the format says so, which thing it is.
    fn facets(&mut self, items: &[Item<'t>]) -> Vec<align::Facet> {
        let by = self.rules.identified_by;
        Work::add(&mut self.work.canon_steps, items.len().saturating_mul(by.len().saturating_add(1)));
        items
            .iter()
            .map(|item| match item {
                Item::El(c) => {
                    let mut name = Hash::new(b'N');
                    name.str(c.name.ns);
                    name.str(c.name.local);
                    let name = name.finish();
                    // Its name, and each attribute of the list it has, in the list's order (the attributes are sorted by name, so one is found by a search).
                    let mut identity = Hash::new(b'D');
                    identity.u64(name);
                    let mut told = false;
                    for (k, n) in by.iter().enumerate() {
                        let at = c.attrs.partition_point(|a| a.name < QName { ns: n.ns, local: n.local });
                        if let Some(attr) = c.attrs.get(at).filter(|a| a.name.is(n) && !a.quiet) {
                            identity.u64(k as u64);
                            attr.val.feed(&mut identity);
                            told = true;
                        }
                    }
                    align::Facet { content: c.hash, name, identity: told.then(|| identity.finish()) }
                }
                Item::Text(_, h) => align::Facet { content: *h, name: Hash::new(b'T').finish(), identity: None },
                Item::Pi(target, _, h) => {
                    let mut name = Hash::new(b'I');
                    name.str(target);
                    align::Facet { content: *h, name: name.finish(), identity: None }
                }
            })
            .collect()
    }

    /// The old reading, for a list past [`MAX_ALIGNED`]: set side by side by place, or one [`Found::Order`] when it is the same children in another order. Every pair is compared
    /// in full.
    fn by_place(&mut self, a: &[Item<'t>], b: &[Item<'t>], base: usize) {
        if a.len() == b.len() && !a.is_empty() && self.same_children(a, b) {
            self.record(Found::Order, None);
            return;
        }
        let n = a.len().min(b.len());
        let (a_pair, a_rest) = a.split_at(n);
        let (b_pair, b_rest) = b.split_at(n);
        self.pairs(a_pair, b_pair, base);
        for (k, x) in a_rest.iter().chain(b_rest).enumerate() {
            self.alone(x, base.saturating_add(n).saturating_add(k));
        }
    }

    /// Whether the two lists hold the same children, whatever their order (by hash: used only to name the difference).
    fn same_children(&mut self, a: &[Item<'t>], b: &[Item<'t>]) -> bool {
        let mut ha: Vec<u64> = a.iter().map(Item::hash).collect();
        let mut hb: Vec<u64> = b.iter().map(Item::hash).collect();
        let n = ha.len();
        Work::add(&mut self.work.canon_steps, n.saturating_mul(2).saturating_mul((usize::BITS - n.leading_zeros()) as usize));
        ha.sort_unstable();
        hb.sort_unstable();
        ha == hb
    }

    fn seg_of(item: &Item<'t>, index: usize) -> PathSeg<'t> {
        match item {
            Item::El(c) => PathSeg { local: c.name.local, mark: Mark::Item, index },
            Item::Text(..) => PathSeg { local: "#text", mark: Mark::Item, index },
            Item::Pi(..) => PathSeg { local: "#instruction", mark: Mark::Item, index },
        }
    }

    fn alone(&mut self, item: &Item<'t>, index: usize) {
        self.step();
        let found = match item {
            Item::El(_) => Found::Element,
            Item::Text(..) => Found::Text,
            Item::Pi(..) => Found::Instruction,
        };
        self.record(found, Some(Self::seg_of(item, index)));
    }

    fn pair(&mut self, x: &Item<'t>, y: &Item<'t>, index: usize) {
        self.step();
        let seg = Self::seg_of(x, index);
        match (x, y) {
            (Item::El(p), Item::El(q)) => self.element(p, q, seg),
            (Item::Text(p, _), Item::Text(q, _)) => {
                Work::add(&mut self.work.canon_bytes, p.len().min(q.len()));
                if p != q {
                    self.record(Found::Text, Some(seg));
                }
            }
            (Item::Pi(t1, d1, _), Item::Pi(t2, d2, _)) => {
                Work::add(&mut self.work.canon_bytes, d1.len().min(d2.len()));
                if t1 != t2 || d1 != d2 {
                    self.record(Found::Instruction, Some(seg));
                }
            }
            _ => self.record(Found::Element, Some(seg)),
        }
    }

    /// The elements of the first document of a namespace that counts as nothing when extra ([`Rules::ignored_when_extra`]) against the second's: each of the first's must have an equal one in
    /// the second (one difference for each that has none); the second's that the first has no equal of are no difference. Both lists are sorted by their hash and the elements of one hash
    /// are paired in order and compared in full, so the work is linear and a collision of the hash only reports a difference where there is none (never the reverse).
    fn quiet(&mut self, a: &[KeyedEl<'t>], b: &[KeyedEl<'t>]) {
        let sorted = |list: &[KeyedEl<'t>]| {
            let mut order: Vec<usize> = (0..list.len()).collect();
            order.sort_by_key(|&k| list.get(k).map_or(0, |e| e.el.hash));
            order
        };
        let (oa, ob) = (sorted(a), sorted(b));
        Work::add(&mut self.work.canon_steps, a.len().saturating_add(b.len()).saturating_add(1));
        let hash_of = |list: &[KeyedEl<'t>], k: usize| list.get(k).map_or(0, |e| e.el.hash);
        let mut j = 0usize;
        let mut previous: Option<u64> = None;
        let mut paired_in_run = 0usize;
        for (place, &k) in oa.iter().enumerate() {
            let Some(x) = a.get(k) else { continue };
            let hash = x.el.hash;
            if previous != Some(hash) {
                previous = Some(hash);
                paired_in_run = 0;
                // The first of the second's elements that is not before this hash.
                while ob.get(j).is_some_and(|&m| hash_of(b, m) < hash) {
                    j += 1;
                }
            }
            let partner = ob.get(j + paired_in_run).and_then(|&m| b.get(m)).filter(|y| y.el.hash == hash);
            paired_in_run += 1;
            let equal = partner.is_some_and(|y| {
                let mut found = Sink::default();
                let mut inner = Cmp { sink: &mut found, path: Vec::new(), work: &mut *self.work, rules: self.rules };
                inner.element(&x.el, &y.el, PathSeg { local: x.el.name.local, mark: Mark::Item, index: 0 });
                found.count == 0
            });
            if !equal {
                self.record(Found::Element, Some(PathSeg { local: x.el.name.local, mark: Mark::Item, index: place }));
            }
        }
    }

    /// The keyed children, both sorted: merged, so that a key on one side only is one difference and a key on both is compared in full.
    fn keyed(&mut self, a: &[KeyedEl<'t>], b: &[KeyedEl<'t>]) {
        // The elements that count as nothing when extra sort last: they are compared apart.
        let (a, a_quiet) = a.split_at(a.partition_point(|k| k.class != QUIET));
        let (b, b_quiet) = b.split_at(b.partition_point(|k| k.class != QUIET));
        if !a_quiet.is_empty() {
            self.quiet(a_quiet, b_quiet);
        }
        let (mut i, mut j) = (0, 0);
        loop {
            self.step();
            let seg = |k: &KeyedEl<'t>, index: usize| PathSeg { local: k.el.name.local, mark: Mark::Keyed, index };
            match (a.get(i), b.get(j)) {
                (None, None) => break,
                (Some(x), None) => {
                    self.record(Found::Element, Some(seg(x, i)));
                    i += 1;
                }
                (None, Some(y)) => {
                    self.record(Found::Element, Some(seg(y, j)));
                    j += 1;
                }
                (Some(x), Some(y)) => {
                    Work::add(&mut self.work.canon_bytes, x.key_bytes.min(y.key_bytes));
                    match x.class.cmp(&y.class).then_with(|| x.key.cmp(&y.key)) {
                        Ordering::Less => {
                            self.record(Found::Element, Some(seg(x, i)));
                            i += 1;
                        }
                        Ordering::Greater => {
                            self.record(Found::Element, Some(seg(y, j)));
                            j += 1;
                        }
                        Ordering::Equal => {
                            self.element(&x.el, &y.el, seg(x, i));
                            i += 1;
                            j += 1;
                        }
                    }
                }
            }
        }
    }
}

/// The parts of one side by name (folded when asked), sorted, with each part's place in the list. Equal names stay in the order given.
fn sorted_names<'n>(entries: &[Entry<'n>], fold_case: bool, work: &mut Work) -> Vec<(Cow<'n, str>, usize)> {
    let mut names: Vec<(Cow<'n, str>, usize)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (if fold_case { Cow::Owned(e.name.to_ascii_lowercase()) } else { Cow::Borrowed(e.name) }, i))
        .collect();
    let mut counted = (0u64, 0u64);
    names.sort_by(|x, y| {
        counted.0 += 1;
        counted.1 += x.0.len().min(y.0.len()) as u64;
        x.0.cmp(&y.0).then(x.1.cmp(&y.1))
    });
    work.canon_steps = work.canon_steps.saturating_add(counted.0);
    work.canon_bytes = work.canon_bytes.saturating_add(counted.1);
    names
}

/// A part on one side only: a difference, unless it is a part [`Rules::parts_absent_when_empty`] lets go.
fn one_side(entry: Option<&Entry<'_>>, found: Found, rules: &Rules, limits: &XmlLimits, work: &mut Work, sink: &mut Sink) {
    if entry.is_some_and(|e| holds_an_empty_root(e, rules, limits, work)) {
        return;
    }
    sink.part_only(found, entry.map(|e| e.name));
}

/// Whether a part is one [`Rules::parts_absent_when_empty`] names and holds nothing: its name is of a listed kind ([`EmptyPart::named`]), it is meant as XML,
/// at most [`MAX_EMPTY_PART_BYTES`], parses within the limits, its root is that kind's root (by the namespace its own declarations bind, the only ones in
/// scope at the root) and the root holds nothing. Anything else, a part of another name whatever its root, a limit or damage included, is a part.
fn holds_an_empty_root(entry: &Entry<'_>, rules: &Rules, limits: &XmlLimits, work: &mut Work) -> bool {
    // The kinds this part's name is of: none, and it is the part it is before anything is read.
    let kinds = || rules.parts_absent_when_empty.iter().filter(|kind| (kind.named)(entry.name));
    if kinds().next().is_none() || !entry.xml || entry.bytes.len() > MAX_EMPTY_PART_BYTES {
        return false;
    }
    let Ok(root) = xml::parse_with_options(entry.bytes, limits, options_of(entry, rules), work) else { return false };
    let mut builder = Builder::new(rules, work);
    builder.bind(&root);
    let name = builder.expand(&root.name, false);
    let Some(kind) = kinds().find(|kind| name.is(&kind.root)) else { return false };
    // The root with the children the kind lets go, each holding nothing, left out (an `office:settings` that holds no setting is a part with nothing in it).
    let mut rest = El { name: root.name.clone(), attrs: root.attrs.clone(), children: Vec::new() };
    if !kind.children.is_empty() {
        rest.children.reserve(root.children.len());
        for node in &root.children {
            if let Node::El(child) = node {
                let child_name = builder.expand_with(child, &child.name);
                if kind.children.iter().any(|n| child_name.is(n)) && builder.holds_nothing(child, child_name) {
                    continue;
                }
            }
            rest.children.push(node.clone());
        }
    }
    let empty = builder.holds_nothing(if kind.children.is_empty() { &root } else { &rest }, name);
    // A part that breaks a constraint of XML Namespaces 1.0 is bytes (see [`breaks_namespaces`]), and so a part, as one that does not parse is: asked last, and only of a part
    // that holds nothing (which is small), since any other is a part already.
    empty && !breaks_namespaces(&root, builder.work)
}

/// Whether `entry` is a part [`Rules::parts_absent_when_empty`] lets go: of a listed kind, meant as XML, small enough to read, its root that kind's and holding nothing but the
/// children the kind lets go, each holding nothing. A format whose manifest lists every part (ODF's) asks this to leave the part's row out too.
pub fn part_holds_nothing(entry: &Entry<'_>, rules: &Rules, limits: &XmlLimits, work: &mut Work) -> bool {
    holds_an_empty_root(entry, rules, limits, work)
}

/// Whether both sides' parts of one name are parts [`Rules::parts_absent_when_empty`] lets go: then each is the same as no part, and the two are the same (an
/// empty `settings.xml` is no `settings.xml`, whichever empty form it takes).
fn both_empty(a: &Entry<'_>, b: &Entry<'_>, rules: &Rules, limits: &XmlLimits, work: &mut Work) -> bool {
    !rules.parts_absent_when_empty.is_empty() && holds_an_empty_root(a, rules, limits, work) && holds_an_empty_root(b, rules, limits, work)
}

/// How a part is parsed: a part [`Rules::doctype_parts`] names may begin with a DOCTYPE that holds only an external id, which is read and is no content; any other part, and
/// any other DOCTYPE, is refused as ever.
fn options_of(entry: &Entry<'_>, rules: &Rules) -> ParseOptions {
    ParseOptions { allow_external_doctype: rules.doctype_parts.contains(&entry.name), ..ParseOptions::default() }
}

/// Whether the XML error is a limit (the part is too large to say anything of) and not damage (the part is bytes).
fn is_limit(e: &XmlError) -> bool {
    matches!(e, XmlError::TooDeep { .. } | XmlError::TooManyNodes { .. } | XmlError::AttributeTooLarge { .. })
}

/// The bytes of a part with every spelling of [`Rules::aliases`] put as its other, when the part is XML meant as such, is valid UTF-8 and holds one; `None` otherwise. The text is
/// searched once for each pair, in time linear in its length. **The pass is not counted in [`Work`]**: it reads a part once and no part twice, and the parts of a comparison are held to
/// [`MAX_COMPARE_BYTES`] before it begins, so that what it costs is bounded by what the comparison already holds; the counts are those of the comparison itself, which the pinned costs of
/// the formats hold to a pass and to `n log n`.
fn aliased(entry: &Entry<'_>, rules: &Rules) -> Option<Vec<u8>> {
    if !entry.xml || rules.aliases.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(entry.bytes).ok()?;
    let mut out: Option<String> = None;
    for (old, new) in rules.aliases {
        if old.is_empty() || new.len() > old.len() {
            continue;
        }
        let current = out.as_deref().unwrap_or(text);
        if current.contains(old) {
            out = Some(current.replace(old, new));
        }
    }
    out.map(String::into_bytes)
}

fn compare_pair(a: &Entry<'_>, b: &Entry<'_>, rules: &Rules, limits: &XmlLimits, work: &mut Work, sink: &mut Sink) -> Result<(), Refusal> {
    sink.part = label(a.name);
    // The spellings the format lists as one (`Rules::aliases`): this pair of parts, read in the one spelling, for the rest of the comparison.
    let (owned_a, owned_b) = (aliased(a, rules), aliased(b, rules));
    let (as_a, as_b) = (owned_a.as_deref().map(|bytes| Entry { bytes, ..*a }), owned_b.as_deref().map(|bytes| Entry { bytes, ..*b }));
    let (a, b) = (as_a.as_ref().unwrap_or(a), as_b.as_ref().unwrap_or(b));
    // The same bytes are the same content, under any rule.
    if a.bytes == b.bytes {
        Work::add(&mut work.canon_bytes, a.bytes.len());
        return Ok(());
    }
    if both_empty(a, b, rules, limits, work) {
        return Ok(());
    }
    if a.xml && b.xml {
        // A part the limits refuse is refused before its partner is read: nothing is built for a pair that cannot be compared.
        let ta = xml::parse_with_options(a.bytes, limits, options_of(a, rules), work);
        if let Err(e) = &ta
            && is_limit(e)
        {
            return Err(e.clone().into());
        }
        let tb = xml::parse_with_options(b.bytes, limits, options_of(b, rules), work);
        match (ta, tb) {
            (Ok(ta), Ok(tb)) => return compare_trees(&ta, &tb, rules, work, sink),
            (_, Err(e)) if is_limit(&e) => return Err(e.into()),
            _ => {}
        }
    }
    sink.add(Found::Bytes, String::new);
    Ok(())
}

fn compare_trees(a: &El, b: &El, rules: &Rules, work: &mut Work, sink: &mut Sink) -> Result<(), Refusal> {
    let ctx = Ctx { in_para: false };
    // The names that are free: the definitions of the two sides are matched by their content, before either tree is built.
    let (na, nb) = match names::resolve(rules, a, b, work)? {
        Some((na, nb)) => (Some(na), Some(nb)),
        None => (None, None),
    };
    let (ca, a_breaks) = {
        let mut builder = Builder::with_naming(rules, work, na);
        let built = builder.build(a, ctx, 0)?;
        (built, builder.breaks)
    };
    let (cb, b_breaks) = {
        let mut builder = Builder::with_naming(rules, work, nb);
        let built = builder.build(b, ctx, 0)?;
        (built, builder.breaks)
    };
    // A part that breaks a constraint of XML Namespaces 1.0 is not one the rules can read: it is its bytes, as a part that does not parse is, and the two are not the same bytes
    // (the caller compared those first). Both sides are built first, so that a refusal is the same whichever side is first.
    if a_breaks || b_breaks {
        sink.add(Found::Bytes, String::new);
        return Ok(());
    }
    let mut cmp = Cmp { sink, path: Vec::new(), work, rules };
    let root = PathSeg { local: ca.name.local, mark: Mark::Item, index: 0 };
    cmp.element(&ca, &cb, root);
    Ok(())
}
