//! The plan of `vsdx_carry`: a valid package whose DiaCraft solution part, carried items, blob relationships and blob parts come from the fuzzer.
//!
//! The package starts as the seed that carries the data of the other format (`seeds::carried_package`: the items, their stamps and the parts of DiaCraft's own are the model writer's, so a read
//! keeps them and the judge goes past the stamp), and the plan changes it, as a hostile or another program's edit would:
//!
//! * **the type of the file** (one of the six, or the seed's): a macro project that a package carries must stay out of a type that allows none;
//! * **parts planted** in the package, under `/diacraft/` in several spellings and outside it, the ones outside with bytes that begin with `OUT`; a relationship of DiaCraft's type
//!   leads to each (when the plan says), and a record of an item can name it: **nothing outside `/diacraft/` may be taken as a blob**;
//! * **edits of the relationships** of the solution part: a target changed to a planted part, to a part of the drawing, to a name that escapes, to another case; a type changed, an `Id` made
//!   a duplicate, a row made external, taken out, doubled, added;
//! * **edits of the items** of the `Data` element: an item taken out, doubled, two exchanged; an attribute of any element of an item set (to a number, a name from a table of hard ones such as
//!   `Basic/..` and `../x`, a role, a format, a media type, the `Id` of a planted relationship, the size or stamp of a planted part's bytes) or taken out; an element taken out or doubled;
//! * **the records made true** when the plan says: each `blob` record whose relationship leads to a part gets that part's size and stamp, so that the judge goes past the checks of what a
//!   record says it is and the only thing between a part and the model is the rule about where it is (without it nearly every record would be dropped at its size);
//! * **bytes spliced into the solution part**, as it is serialized;
//! * **the judge's budget** for the read: no steps, a few, or the default.
//!
//! Every count and length is bounded, and an edit that names an item or an element that is not there does nothing.

use std::collections::HashMap;

use arbitrary::Unstructured;
use std::sync::Arc;

use diacraft_doc::{Document, Format, PartRole, RawAttrs, RawElement, RawNode, Shape};
use diacraft_opc::name::resolve;
use diacraft_opc::xml::{El, Node, parse, to_bytes};
use diacraft_sheetxml::model::stamp_of_bytes;
use diacraft_vsdx::{Kind, MAX_CARRY_STEPS, Variant, WriteOptions};

use crate::seeds::{Items, carried_doc, pack, part, seeds};

/// The text every hard entry name of the table holds, so that a check can tell a warning that names an entry from one that does not.
pub const TOKEN: &str = "ZQX";

/// The bytes every part planted outside `/diacraft/` begins with.
pub const OUTSIDE: &[u8] = b"OUT";

/// The type of a relationship that leads to a part of DiaCraft's own.
const BLOB_REL_TYPE: &str = "urn:app.diacraft:extension:blob";

/// Entry names of the other format, the hard ones first. A macro project, a name that reaches outside a package, a name that differs only by case, the names a package makes itself.
pub const NAMES: &[&str] = &[
    "Basic/ZQX/m.xml",
    "BASIC/ZQX.xml",
    "scripts/ZQXpy",
    "Scripts/ZQX/",
    "META-INF/macrosignatures.xml",
    "meta-inf/MacroSignatures.xml",
    "../ZQXescape",
    "a/../ZQXb",
    "/ZQXabs",
    "a\\ZQXb",
    "C:ZQX",
    "diacraft/ZQX",
    "DIACRAFT/",
    "mimetype",
    "Content.XML",
    "META-INF/manifest.xml",
    "Object 1/ZQX.xml",
    "Object 1/",
    "Pictures/ZQX.png",
    "pictures/zqx.PNG",
    "Thumbnails/ZQX.png",
    "ObjectReplacements/ZQX",
    "Fonts/ZQX.ttf",
    "META-INF/documentsignatures.xml",
    "./ZQX",
    "ZQX//b",
    "ZQX/",
    "",
];

const ROLES: &[&str] =
    &["macro-project", "macro-signature", "macro-data", "signature", "thumbnail", "object-file", "embedding", "font", "other", "", "no-such-role"];
const FORMATS: &[&str] = &["odf", "vsdx", "native", "", "ODF"];
const MEDIA: &[&str] = &["text/xml", "application/octet-stream", "image/png", "a/b/c", "", "text/xml; charset=utf-8", "x"];
/// Numbers at the edges of the places they go (an `ID`, a size, an index, a count).
const NUMBERS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "9",
    "255",
    "65535",
    "4294967295",
    "4294967296",
    "18446744073709551615",
    "18446744073709551616",
    "-1",
    "1.5",
    "+1",
    " 1",
    "01",
];

/// Where a planted part may be, with its slash: a part of DiaCraft's own in several spellings, and a part that is not.
const DIRS: &[&str] = &["/diacraft/", "/DiaCraft/", "/diacraft/sub/", "/diacraftx/", "/outside/", "/docProps/", "/", "/other/diacraft/"];
const EXTS: &[(&str, &str)] = &[("bin", "application/octet-stream"), ("xml", "text/xml"), ("png", "image/png"), ("dat", "application/octet-stream")];

/// Targets for a relationship of the solution part, relative to the part (`/<folder>/solutions/solution1.xml`): a part of the drawing, a name that escapes or spells the folder another way, a
/// name that is no part, a URL.
const TARGETS: &[&str] = &[
    "../pages/page1.xml",
    "../../diacraft/../docProps/core.xml",
    "/DIACRAFT/BLOB1.BIN",
    "../../diacraft/",
    "../../diacraft/_rels/blob1.bin.rels",
    "../../../x",
    "http://example.com/x",
    "",
    "#fragment",
    "../../diacraft/blob1.bin#x",
    "../../diacraft//blob1.bin",
    "../../diacraft/blob1.bin/",
    "..\\..\\diacraft\\blob1.bin",
    "../../diacraft/%62lob1.bin",
    "../../diacraft/blob2.xml",
    "solution1.xml",
    "../solutions/solution1.xml",
];

/// The six types of a drawing package, in a fixed order.
fn variant_of(n: u8) -> Option<Variant> {
    let kind = match n % 6 / 2 {
        0 => Kind::Drawing,
        1 => Kind::Stencil,
        _ => Kind::Template,
    };
    (n < 6).then_some(Variant { kind, macro_enabled: n % 2 == 1 })
}

// ------------------------------------------------------------------------------------------------------------------------------ the plan

/// What is planted, as the plan says it.
#[derive(Clone, Debug)]
struct Planted {
    dir: u8,
    name: u8,
    ext: u8,
    bytes: Vec<u8>,
}

/// A target for a relationship: a planted part, or a name of the table.
#[derive(Clone, Copy, Debug)]
enum To {
    Planted(u8),
    Table(u8),
}

#[derive(Clone, Debug)]
enum RelEdit {
    Retarget { row: u8, to: To },
    Type { row: u8, blob: bool },
    Id { row: u8, to: u8 },
    External(u8),
    Drop(u8),
    Dup(u8),
    Add(To),
}

/// A value for an attribute of an item.
#[derive(Clone, Debug)]
enum Val {
    Number(u8),
    Text(Vec<u8>),
    Name(u8),
    Role(u8),
    Format(u8),
    Media(u8),
    /// The `Id` of the relationship made for a planted part, or of a row of the seed's.
    Rid(u8),
    /// The stamp and the size of a planted part's bytes.
    Stamp(u8),
    Size(u8),
}

#[derive(Clone, Debug)]
enum ItemEdit {
    Drop(u8),
    Dup(u8),
    Swap(u8, u8),
    /// An attribute of an element of an item (of those that have one): one the element has, or (a high `attr`) one of the table, which it may not have.
    Set {
        item: u8,
        node: u16,
        attr: u8,
        val: Val,
    },
    Del {
        item: u8,
        node: u16,
        attr: u8,
    },
    DelNode {
        item: u8,
        node: u16,
    },
    DupNode {
        item: u8,
        node: u16,
    },
    /// A field of the `n`th `part` element of the data (`name`, `role`, `format`, `media-type`, `blob`), and of the `n`th `blob` record (`path`, `size`, `stamp`, `media-type`, `id`), and of
    /// the wrapper of an item (`kind`, `id`, `of`, `fp`, `name`): the places the carrier's rules judge.
    Field {
        of: Of,
        n: u8,
        field: u8,
        val: Val,
    },
}

#[derive(Clone, Copy, Debug)]
enum Of {
    Part,
    Blob,
    Item,
}

const PART_FIELDS: &[&str] = &["name", "role", "format", "media-type", "blob"];
const BLOB_FIELDS: &[&str] = &["path", "size", "stamp", "media-type", "id"];
const ITEM_FIELDS: &[&str] = &["kind", "id", "of", "fp", "name"];

/// What a target decides from its bytes, and builds a package from.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The type of the file: one of the six, or (6 and more) the seed's own, a macro-enabled drawing.
    variant: u8,
    /// The judge's budget for the read.
    pub steps: u64,
    fix_records: bool,
    link_planted: bool,
    planted: Vec<Planted>,
    rels: Vec<RelEdit>,
    items: Vec<ItemEdit>,
    /// Where bytes are spliced into the solution part, how many are cut, and what is put.
    raw: Option<(u16, u8, Vec<u8>)>,
}

/// A reader of the input that gives zero when it is spent, so that a short input is a plan with little in it.
struct Src<'a, 'b>(&'a mut Unstructured<'b>);

impl Src<'_, '_> {
    fn u8(&mut self) -> u8 {
        self.0.arbitrary::<u8>().unwrap_or(0)
    }

    fn u16(&mut self) -> u16 {
        u16::from(self.u8()) << 8 | u16::from(self.u8())
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::from(self.u8()) % n }
    }

    fn bytes(&mut self, most: usize) -> Vec<u8> {
        let n = self.below(most.saturating_add(1)).min(self.0.len());
        self.0.bytes(n).map(<[u8]>::to_vec).unwrap_or_default()
    }

    fn to(&mut self) -> To {
        let n = self.u8();
        if n.is_multiple_of(2) { To::Planted(n >> 1) } else { To::Table(n >> 1) }
    }
}

impl Plan {
    /// The plan the bytes of `data` say.
    pub fn of(data: &[u8]) -> Plan {
        let mut raw = Unstructured::new(data);
        let mut src = Src(&mut raw);
        let variant = src.u8() % 8;
        // Most plans give the judge the budget of a real read; one in eight none, one in eight a few steps, one in eight enough for a few of the items.
        let steps = match src.u8() % 8 {
            0 => 0,
            1 => 30,
            2 => 600,
            _ => MAX_CARRY_STEPS,
        };
        let flags = src.u8();
        let mut planted = Vec::new();
        for _ in 0..src.below(7) {
            planted.push(Planted { dir: src.u8(), name: src.u8(), ext: src.u8(), bytes: src.bytes(24) });
        }
        let mut rels = Vec::new();
        for _ in 0..src.below(13) {
            let row = src.u8();
            rels.push(match src.below(7) {
                0 => RelEdit::Retarget { row, to: src.to() },
                1 => RelEdit::Type { row, blob: src.u8() % 2 == 0 },
                2 => RelEdit::Id { row, to: src.u8() },
                3 => RelEdit::External(row),
                4 => RelEdit::Drop(row),
                5 => RelEdit::Dup(row),
                _ => RelEdit::Add(src.to()),
            });
        }
        let mut items = Vec::new();
        for _ in 0..src.below(17) {
            let item = src.u8();
            let value = |src: &mut Src<'_, '_>| match src.below(9) {
                0 => Val::Number(src.u8()),
                1 => Val::Text(src.bytes(12)),
                2 => Val::Name(src.u8()),
                3 => Val::Role(src.u8()),
                4 => Val::Format(src.u8()),
                5 => Val::Media(src.u8()),
                6 => Val::Rid(src.u8()),
                7 => Val::Stamp(src.u8()),
                _ => Val::Size(src.u8()),
            };
            items.push(match src.below(14) {
                0 => ItemEdit::Drop(item),
                1 => ItemEdit::Dup(item),
                2 => ItemEdit::Swap(item, src.u8()),
                3 => {
                    let (node, attr) = (src.u16(), src.u8());
                    ItemEdit::Set { item, node, attr, val: value(&mut src) }
                }
                4 => ItemEdit::Del { item, node: src.u16(), attr: src.u8() },
                5 => ItemEdit::DelNode { item, node: src.u16() },
                6 => ItemEdit::DupNode { item, node: src.u16() },
                _ => {
                    let of = match src.below(8) {
                        0..=3 => Of::Part,
                        4 | 5 => Of::Blob,
                        _ => Of::Item,
                    };
                    ItemEdit::Field { of, n: item, field: src.u8(), val: value(&mut src) }
                }
            });
        }
        let raw = (src.u8() % 8 == 0).then(|| (src.u16(), src.u8(), src.bytes(24)));
        Plan { variant, steps, fix_records: flags & 1 == 0 || flags & 2 == 0, link_planted: flags & 4 == 0, planted, rels, items, raw }
    }

    /// The parts planted: each one's name (with its slash), its bytes and whether it lies outside `/diacraft/` in the sense of the carrier.
    pub fn planted_parts(&self) -> Vec<(String, Vec<u8>, bool)> {
        let mut out: Vec<(String, Vec<u8>, bool)> = Vec::new();
        for (j, p) in self.planted.iter().enumerate() {
            let (ext, _) = EXTS.get(usize::from(p.ext) % EXTS.len()).copied().unwrap_or(("bin", ""));
            let dir = DIRS.get(usize::from(p.dir) % DIRS.len()).copied().unwrap_or("/outside/");
            let name = format!("{dir}{}{j}.{ext}", ["a", "b", "m", "n"][usize::from(p.name) % 4]);
            let folded = name.to_ascii_lowercase();
            let inside = folded.starts_with("/diacraft/");
            let mut bytes = p.bytes.clone();
            if inside
                && bytes.starts_with(OUTSIDE)
                && let Some(first) = bytes.first_mut()
            {
                *first = b'o';
            }
            if !inside {
                bytes.splice(0..0, OUTSIDE.iter().copied());
            }
            if !out.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
                out.push((name, bytes, !inside));
            }
        }
        out
    }

    /// The package the plan makes, or `None` when a part of it cannot be written (a control character in a name, a tree XML cannot hold).
    pub fn package(&self) -> Option<Vec<u8>> {
        let mut items: Items = seeds().carried.clone();
        let solution_at = items.iter().position(|(n, _)| n.ends_with("solution1.xml"))?;
        let rels_at = items.iter().position(|(n, _)| n.ends_with("solution1.xml.rels"))?;
        let solution_name = format!("/{}", items.get(solution_at)?.0);
        let mut tree = parse(&items.get(solution_at)?.1).ok()?;
        let mut rels = parse(&items.get(rels_at)?.1).ok()?;

        let planted = self.planted_parts();
        // The relationships: one for each planted part, then the plan's edits of them.
        if self.link_planted {
            for (j, (name, _, _)) in planted.iter().enumerate() {
                rels.children.push(Node::El(relationship(&format!("p{j}"), BLOB_REL_TYPE, name)));
            }
        }
        for edit in &self.rels {
            self.edit_rels(&mut rels, edit, &planted);
        }
        // The items.
        for edit in &self.items {
            self.edit_items(&mut tree, edit, &planted);
        }
        // The parts and the types the package gains.
        let mut types = String::from_utf8(items.first()?.1.clone()).ok()?;
        for (name, bytes, _) in &planted {
            let (_, ty) = EXTS.iter().find(|(e, _)| name.ends_with(&format!(".{e}"))).copied().unwrap_or(("bin", "application/octet-stream"));
            let row = format!("<Override PartName=\"{name}\" ContentType=\"{ty}\"/>");
            types = types.replacen("</Types>", &format!("{row}</Types>"), 1);
            items.push((name.trim_start_matches('/').to_owned(), bytes.clone()));
        }
        // The type of the file: the content type of the document part, as the writer spelled it for a macro-enabled drawing.
        if let Some(variant) = variant_of(self.variant) {
            let seed = Variant { kind: Kind::Drawing, macro_enabled: true };
            types = types.replacen(seed.content_type(), variant.content_type(), 1);
        }
        if self.fix_records {
            fix_records(&mut tree, &rels, &solution_name, &items);
        }
        items.get_mut(0)?.1 = types.into_bytes();
        items.get_mut(rels_at)?.1 = to_bytes(&rels, &[], &[], true).ok()?;
        let mut solution = to_bytes(&tree, &[], &[], true).ok()?;
        if let Some((at, cut, insert)) = &self.raw {
            let at = usize::from(*at) % solution.len().saturating_add(1);
            let cut = usize::from(*cut).min(solution.len().saturating_sub(at));
            solution.splice(at..at.saturating_add(cut), insert.iter().copied());
        }
        items.get_mut(solution_at)?.1 = solution;
        Some(pack(&items))
    }

    fn target(&self, to: To, planted: &[(String, Vec<u8>, bool)]) -> String {
        match to {
            To::Planted(j) if !planted.is_empty() => planted.get(usize::from(j) % planted.len()).map(|p| p.0.clone()).unwrap_or_default(),
            To::Planted(j) | To::Table(j) => TARGETS.get(usize::from(j) % TARGETS.len()).copied().unwrap_or("").to_owned(),
        }
    }

    fn edit_rels(&self, rels: &mut El, edit: &RelEdit, planted: &[(String, Vec<u8>, bool)]) {
        let rows: Vec<usize> =
            rels.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::El(e) if e.is("Relationship"))).map(|(at, _)| at).collect();
        let pick = |row: u8| (!rows.is_empty()).then(|| rows.get(usize::from(row) % rows.len()).copied()).flatten();
        match edit {
            RelEdit::Retarget { row, to } => {
                if let Some(Node::El(e)) = pick(*row).and_then(|at| rels.children.get_mut(at)) {
                    set(e, "Target", &self.target(*to, planted));
                }
            }
            RelEdit::Type { row, blob } => {
                if let Some(Node::El(e)) = pick(*row).and_then(|at| rels.children.get_mut(at)) {
                    set(e, "Type", if *blob { BLOB_REL_TYPE } else { "urn:example:other" });
                }
            }
            RelEdit::Id { row, to } => {
                if let Some(Node::El(e)) = pick(*row).and_then(|at| rels.children.get_mut(at)) {
                    set(e, "Id", &format!("rId{}", to % 8));
                }
            }
            RelEdit::External(row) => {
                if let Some(Node::El(e)) = pick(*row).and_then(|at| rels.children.get_mut(at)) {
                    set(e, "TargetMode", "External");
                }
            }
            RelEdit::Drop(row) => {
                if let Some(at) = pick(*row) {
                    rels.children.remove(at);
                }
            }
            RelEdit::Dup(row) => {
                if let Some(at) = pick(*row)
                    && let Some(copy) = rels.children.get(at).cloned()
                {
                    rels.children.insert(at, copy);
                }
            }
            RelEdit::Add(to) => {
                let id = format!("rId{}", rels.children.len() + 10);
                rels.children.push(Node::El(relationship(&id, BLOB_REL_TYPE, &self.target(*to, planted))));
            }
        }
    }

    fn edit_items(&self, root: &mut El, edit: &ItemEdit, planted: &[(String, Vec<u8>, bool)]) {
        let Some(data) = root.children.iter_mut().find_map(|n| match n {
            Node::El(e) if e.is("Data") => Some(e),
            _ => None,
        }) else {
            return;
        };
        let places: Vec<usize> =
            data.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::El(e) if e.is("item"))).map(|(at, _)| at).collect();
        if places.is_empty() {
            return;
        }
        let place = |item: u8| places.get(usize::from(item) % places.len()).copied().unwrap_or(0);
        match edit {
            ItemEdit::Drop(item) => {
                data.children.remove(place(*item));
            }
            ItemEdit::Dup(item) => {
                if let Some(copy) = data.children.get(place(*item)).cloned() {
                    data.children.insert(place(*item), copy);
                }
            }
            ItemEdit::Swap(a, b) => data.children.swap(place(*a), place(*b)),
            ItemEdit::Set { item, node, attr, val } => {
                let value = self.value(val, planted);
                if let Some(Node::El(wrapper)) = data.children.get_mut(place(*item)) {
                    let mut n = usize::from(*node) % count(wrapper);
                    if let Some(el) = nth_mut(wrapper, &mut n) {
                        let name = attr_of(el, *attr);
                        set(el, &name, &value);
                    }
                }
            }
            ItemEdit::Del { item, node, attr } => {
                if let Some(Node::El(wrapper)) = data.children.get_mut(place(*item)) {
                    let mut n = usize::from(*node) % count(wrapper);
                    if let Some(el) = nth_mut(wrapper, &mut n) {
                        let name = attr_of(el, *attr);
                        el.attrs.retain(|(k, _)| *k != name);
                    }
                }
            }
            ItemEdit::Field { of, n, field, val } => {
                let value = self.value(val, planted);
                let (local, fields) = match of {
                    Of::Part => ("part", PART_FIELDS),
                    Of::Blob => ("blob", BLOB_FIELDS),
                    Of::Item => ("item", ITEM_FIELDS),
                };
                let field = fields.get(usize::from(*field) % fields.len()).copied().unwrap_or("name");
                let total = count_named(data, local);
                if total > 0 {
                    let mut at = usize::from(*n) % total;
                    if let Some(el) = nth_named_mut(data, local, &mut at, true) {
                        set(el, field, &value);
                    }
                }
            }
            ItemEdit::DelNode { item, node } | ItemEdit::DupNode { item, node } => {
                if let Some(Node::El(wrapper)) = data.children.get_mut(place(*item)) {
                    let n = usize::from(*node) % count(wrapper);
                    if n > 0 {
                        restructure(wrapper, n - 1, matches!(edit, ItemEdit::DupNode { .. }));
                    }
                }
            }
        }
    }

    fn value(&self, val: &Val, planted: &[(String, Vec<u8>, bool)]) -> String {
        let table = |t: &[&str], n: u8| t.get(usize::from(n) % t.len().max(1)).copied().unwrap_or("").to_owned();
        let part = |n: u8| (!planted.is_empty()).then(|| planted.get(usize::from(n) % planted.len())).flatten();
        match val {
            Val::Number(n) => table(NUMBERS, *n),
            Val::Text(bytes) => String::from_utf8_lossy(bytes).chars().map(|c| if c.is_control() { '_' } else { c }).collect(),
            // The longest name an entry may have is 4096 bytes: one place in the table is a name past it.
            Val::Name(n) if usize::from(*n) % (NAMES.len() + 1) == NAMES.len() => format!("Pictures/{}.png", TOKEN.repeat(1_400)),
            Val::Name(n) => table(NAMES, *n),
            Val::Role(n) => table(ROLES, *n),
            Val::Format(n) => table(FORMATS, *n),
            Val::Media(n) => table(MEDIA, *n),
            Val::Rid(n) => {
                if self.link_planted && !planted.is_empty() {
                    format!("p{}", usize::from(*n) % planted.len())
                } else {
                    format!("rId{}", n % 8)
                }
            }
            Val::Stamp(n) => part(*n).map_or_else(|| "0000000000000000".to_owned(), |p| stamp_of_bytes(&p.1).text()),
            Val::Size(n) => part(*n).map_or_else(|| "0".to_owned(), |p| p.1.len().to_string()),
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the trees

/// The text of every attribute name an element of an item has in the seed, in order of first use (the names the plan's edits choose from).
fn attr_names() -> &'static Vec<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let mut out: Vec<String> = Vec::new();
        let tree = seeds().carried.iter().find(|(n, _)| n.ends_with("solution1.xml")).and_then(|(_, bytes)| parse(bytes).ok());
        let mut stack: Vec<&El> = tree.iter().collect();
        while let Some(el) = stack.pop() {
            for (k, _) in &el.attrs {
                if !k.starts_with("xmlns") && !out.contains(k) {
                    out.push(k.clone());
                }
            }
            stack.extend(el.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }));
        }
        out.sort();
        if out.is_empty() {
            out.push("fp".to_owned());
        }
        out
    })
}

fn relationship(id: &str, ty: &str, target: &str) -> El {
    El {
        name: "Relationship".to_owned(),
        attrs: vec![("Id".to_owned(), id.to_owned()), ("Type".to_owned(), ty.to_owned()), ("Target".to_owned(), target.to_owned())],
        children: Vec::new(),
    }
}

/// Sets an attribute, in place when the element has it.
fn set(el: &mut El, name: &str, value: &str) {
    match el.attrs.iter_mut().find(|(k, _)| k == name) {
        Some(slot) => slot.1 = value.to_owned(),
        None => el.attrs.push((name.to_owned(), value.to_owned())),
    }
}

/// The name of an attribute an edit means for `el`: one that the element has (`attr` picks among them), or, when `attr` is 128 or more, one of the table, which the element may not have.
fn attr_of(el: &El, attr: u8) -> String {
    let own: Vec<&String> = el.attrs.iter().map(|(k, _)| k).filter(|k| !k.starts_with("xmlns")).collect();
    if attr < 128 && !own.is_empty() {
        return own.get(usize::from(attr) % own.len()).map(|k| (*k).clone()).unwrap_or_default();
    }
    let names = attr_names();
    names.get(usize::from(attr) % names.len().max(1)).cloned().unwrap_or_else(|| "fp".to_owned())
}

/// How many elements below `el` (`el` itself not counted) have the local name `local`.
fn count_named(el: &El, local: &str) -> usize {
    el.children
        .iter()
        .map(|n| match n {
            Node::El(e) => usize::from(e.is(local)) + count_named(e, local),
            _ => 0,
        })
        .sum()
}

/// The `n`th element below `el` in document order with the local name `local`.
fn nth_named_mut<'e>(el: &'e mut El, local: &str, n: &mut usize, below: bool) -> Option<&'e mut El> {
    if !below && el.is(local) {
        if *n == 0 {
            return Some(el);
        }
        *n -= 1;
    }
    for child in el.children.iter_mut() {
        if let Node::El(c) = child
            && let Some(found) = nth_named_mut(c, local, n, false)
        {
            return Some(found);
        }
    }
    None
}

/// The elements of a tree, the root included.
fn count(el: &El) -> usize {
    1 + el.children.iter().map(|n| if let Node::El(e) = n { count(e) } else { 0 }).sum::<usize>()
}

/// The `n`th element of a tree in document order, the root being the 0th.
fn nth_mut<'e>(el: &'e mut El, n: &mut usize) -> Option<&'e mut El> {
    if *n == 0 {
        return Some(el);
    }
    *n -= 1;
    for child in el.children.iter_mut() {
        if let Node::El(c) = child
            && let Some(found) = nth_mut(c, n)
        {
            return Some(found);
        }
    }
    None
}

/// Takes out (or doubles) the element that is the `n`th below `el` in document order, the first child being the 0th.
fn restructure(el: &mut El, mut n: usize, double: bool) -> bool {
    let mut at = 0;
    while at < el.children.len() {
        let size = match el.children.get(at) {
            Some(Node::El(c)) => count(c),
            _ => {
                at += 1;
                continue;
            }
        };
        if n < size {
            if n == 0 {
                if double {
                    if let Some(copy) = el.children.get(at).cloned() {
                        el.children.insert(at, copy);
                    }
                } else {
                    el.children.remove(at);
                }
                return true;
            }
            return match el.children.get_mut(at) {
                Some(Node::El(c)) => restructure(c, n - 1, double),
                _ => false,
            };
        }
        n -= size;
        at += 1;
    }
    false
}

/// Makes each `blob` record whose relationship leads to a part of the package say that part's size and stamp.
fn fix_records(tree: &mut El, rels: &El, solution: &str, items: &Items) {
    let by_id: HashMap<&str, &str> = rels
        .children
        .iter()
        .filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
        .filter(|e| e.attr("TargetMode") != Some("External"))
        .filter_map(|e| Some((e.attr("Id")?, e.attr("Target")?)))
        .collect();
    let bytes: HashMap<String, &Vec<u8>> = items.iter().map(|(n, b)| (format!("/{n}").to_ascii_lowercase(), b)).collect();
    let mut stack: Vec<&mut El> = vec![tree];
    while let Some(el) = stack.pop() {
        if el.is("blob") {
            let found = el
                .attr("path")
                .and_then(|id| by_id.get(id))
                .and_then(|target| resolve(solution, target))
                .and_then(|name| bytes.get(&name.to_ascii_lowercase()).copied());
            if let Some(found) = found {
                set(el, "size", &found.len().to_string());
                set(el, "stamp", &stamp_of_bytes(found).text());
            }
        }
        stack.extend(el.children.iter_mut().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }));
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ a model

/// The roles of a part, in a fixed order.
const PART_ROLES: [PartRole; 11] = [
    PartRole::Thumbnail,
    PartRole::MacroProject,
    PartRole::MacroSignature,
    PartRole::MacroData,
    PartRole::Embedding,
    PartRole::ObjectFile,
    PartRole::ObjectReplacement,
    PartRole::Signature,
    PartRole::CustomXml,
    PartRole::Font,
    PartRole::Other,
];

/// What an entry of an `.odg` package is for, by its name: the rule of the `.odg` reader, as the carrier makes the role of a part again when it reads one.
fn odf_role(name: &str) -> PartRole {
    let object = name.strip_prefix("Object ").is_some_and(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest.get(digits..).is_some_and(|tail| tail.starts_with('/'))
    });
    let folded = name.to_ascii_lowercase();
    if name.starts_with("Thumbnails/") && !name.ends_with('/') {
        PartRole::Thumbnail
    } else if name == "META-INF/macrosignatures.xml" {
        PartRole::MacroSignature
    } else if folded.starts_with("basic/") || folded.starts_with("scripts/") || folded == "meta-inf/macrosignatures.xml" {
        PartRole::MacroProject
    } else if name == "META-INF/documentsignatures.xml" {
        PartRole::Signature
    } else if name.starts_with("ObjectReplacements/") {
        PartRole::ObjectReplacement
    } else if object {
        PartRole::ObjectFile
    } else if name.starts_with("Fonts/") {
        PartRole::Font
    } else {
        PartRole::Other
    }
}

/// Names a model may hold that no package could (a control character, a name past the cap by one byte, one at it) and names that a package may hold, beside the hard ones of [`NAMES`].
fn model_name(n: u8, text: &[u8]) -> String {
    let at = usize::from(n) % (NAMES.len() + 6);
    match NAMES.get(at) {
        Some(name) => (*name).to_owned(),
        None => match at - NAMES.len() {
            0 => "ZQX\u{1}control".to_owned(),
            1 => "x".repeat(4096),
            2 => "x".repeat(4097),
            3 => "Object 9/ZQX.xml".to_owned(),
            4 => "Extras/ZQX.bin".to_owned(),
            _ => String::from_utf8_lossy(text).into_owned(),
        },
    }
}

/// A plan for the writer of a model: the model that carries the data of the other format in every place it rides, with parts of the other format added that a program could put there (a name
/// that reaches outside a package, one that differs only by case, a macro project in a package that allows none, a role the name does not say), written as a package of a type with macros
/// or of one without, with the macro project left out or not.
#[derive(Clone, Debug)]
pub struct ModelPlan {
    macros: bool,
    drop_macros: bool,
    parts: Vec<(u8, u8, u8, u8, Vec<u8>)>,
    /// Object frames of an `.odg`: for each, which entry it names, what the entry holds, and what its replacement is.
    frames: Vec<(u8, u8, u8)>,
}

/// The first eight bytes of a compound file.
const COMPOUND_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The bytes of a compound file with `tag` in the class of its root entry: 1,536 bytes with a root entry at 1,024.
fn compound(tag: u8) -> Vec<u8> {
    let mut bytes = vec![0u8; 1536];
    bytes[..8].copy_from_slice(&COMPOUND_SIGNATURE);
    bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
    bytes[1024 + 0x42] = 5;
    bytes[1024 + 0x50] = tag;
    bytes
}

impl ModelPlan {
    /// The plan the bytes of `data` say.
    pub fn of(data: &[u8]) -> ModelPlan {
        let mut raw = Unstructured::new(data);
        let mut src = Src(&mut raw);
        let flags = src.u8();
        let mut parts = Vec::new();
        for _ in 0..src.below(7) {
            parts.push((src.u8(), src.u8(), src.u8(), src.u8(), src.bytes(16)));
        }
        let mut frames = Vec::new();
        for _ in 0..src.below(4) {
            frames.push((src.u8(), src.u8(), src.u8()));
        }
        ModelPlan { macros: flags & 1 == 1, drop_macros: flags & 2 == 2, parts, frames }
    }

    /// The model: the seed's, and the parts of the plan after its own.
    pub fn model(&self) -> Document {
        let mut doc = carried_doc().clone();
        for (format, name, role, media, bytes) in &self.parts {
            let format = if format % 2 == 0 { Format::Odf } else { Format::Native };
            let name = model_name(*name, bytes);
            // The role of an entry of an `.odg` package is what its name says (the reader makes it again from the name, whatever a file says, and says so): a model made by the `.odg` reader holds
            // no other. A part of a newer `.diacraft` file keeps the role the file gave it, whatever its name.
            let role = match format {
                Format::Odf => odf_role(&name),
                _ => PART_ROLES.get(usize::from(*role) % PART_ROLES.len()).copied().unwrap_or(PartRole::Other),
            };
            let media = MEDIA.get(usize::from(*media) % MEDIA.len()).copied().unwrap_or("");
            doc.parts.push(part(format, &name, media, role, bytes));
        }
        self.add_frames(&mut doc);
        doc
    }

    /// The object frames of the plan, on the seed's first page: each names an entry (a part of its own that holds a compound file, bytes that are none or a folder's file, one of the plan's parts, a
    /// folder, or a name that reaches outside the package) and shows a picture (a PNG, bytes no reader draws, none, or a PNG that the manifest types as XML or as nothing in particular).
    fn add_frames(&self, doc: &mut Document) {
        use diacraft_doc::{ForeignData, Media, ShapeType};
        let png = |tag: u8| -> Vec<u8> { vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, tag] };
        for (at, (name, object, picture)) in self.frames.iter().enumerate() {
            let tag = u8::try_from(at).unwrap_or(0);
            let entry = format!("Object {}", 20 + at);
            // What the frame names.
            let named = match name % 4 {
                0 => entry.clone(),
                1 => self.parts.get(usize::from(*object) % self.parts.len().max(1)).map_or_else(|| entry.clone(), |p| model_name(p.1, &p.4)),
                2 => format!("{entry}/"),
                _ => "../ZQXoutside".to_owned(),
            };
            // What the entry holds: a compound file, bytes that are none, or nothing.
            if name % 4 == 0 {
                let bytes = match object % 3 {
                    0 => compound(tag),
                    1 => b"PK\x03\x04 not a compound file".to_vec(),
                    _ => Vec::new(),
                };
                doc.parts.push(part(Format::Odf, &entry, "application/vnd.sun.star.oleobject", odf_role(&entry), &bytes));
            }
            let (bytes, content_type) = match picture % 5 {
                0 => (Some(png(tag)), "image/png"),
                1 => (Some(b"NOTAPICT".to_vec()), "image/png"),
                2 => (None, ""),
                3 => (Some(png(tag)), "text/xml"),
                _ => (Some(png(tag)), "application/octet-stream"),
            };
            let media = bytes.map(|bytes| {
                Arc::new(Media {
                    part: Arc::from(format!("ObjectReplacements/{entry}").as_str()),
                    content_type: Arc::from(content_type),
                    bytes: Arc::from(bytes),
                })
            });
            let foreign = ForeignData { picture: media, attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]), ..ForeignData::default() };
            let mut shape = Shape { id: 9000 + u32::from(tag), kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() };
            let element = RawElement {
                name: Arc::from(if name % 7 == 6 { "draw:object" } else { "draw:object-ole" }),
                attrs: RawAttrs::from_pairs([("xlink:href", format!("./{named}").as_str())]),
                children: Vec::new(),
            };
            shape.ext.make_mut().odf.kept.kept.push(0, RawNode::Element(element));
            if let Some(page) = doc.pages.get_mut(0) {
                Arc::make_mut(page).shapes.push(Arc::new(shape));
            }
        }
    }

    /// Whether the model holds an object frame the writer may write as an embedding.
    pub fn has_frames(&self) -> bool {
        !self.frames.is_empty()
    }

    /// The options of the save: the type (a macro-enabled drawing, or a drawing), the time, the model said to be the one that was read, and the macro project left out or not.
    pub fn options(&self) -> WriteOptions {
        WriteOptions {
            now: Some(1_700_000_000),
            content_unchanged: true,
            variant: Some(Variant { kind: Kind::Drawing, macro_enabled: self.macros }),
            drop_macros: self.drop_macros,
            ..WriteOptions::default()
        }
    }

    /// Whether the package is of a type that allows macros.
    pub fn allows_macros(&self) -> bool {
        self.macros
    }

    /// Whether the macro project is left out when the type allows none.
    pub fn drops_macros(&self) -> bool {
        self.drop_macros
    }
}
