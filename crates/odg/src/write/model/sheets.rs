//! The model's style sheets written as the common styles they were read from, into `styles.xml` of the package made (the native-format design's §3.4 row for the style sheets).
//!
//! An evidence run found the common styles of 703 of 719 visits written as **empty** common styles: the model writer splices every style sheet onto a package that holds none, so each was a
//! new style sheet of the splice, which writes an empty `style:style` and carries the whole of its sheet in DiaCraft's document file (`write/newstyles.rs`). DiaCraft read it back; **another
//! program saw a style with no property, and every shape that named it lost the look it gave** (a fill, a line, a font), and DiaCraft itself lost what the style held that no cell holds (the values
//! its cells only approximate, a bitmap fill among them, and the properties no cell reads). This step writes them before the splice, in the package the splice reads as the file a drawing was
//! opened from, so that the splice takes them for the file's own style sheets:
//!
//! * **Which.** The style sheets the `.odg` reader made (an `OdfStyle` of the `graphic` or `paragraph` family), from the first after the root up to the first that is not one, in the model's
//!   order: the reader numbers the common styles of those families in the order they stand, so each is read back with its `ID`. The rest stay the splice's new style sheets.
//! * **What.** `style:name` (the name the file held it by), `style:display-name`, `style:family`, `style:parent-style-name` (the style sheet the three links name), the other attributes the
//!   style had, and the properties elements: the attributes the sheet's own cells come to (the fill and line cells and the text block's, as a new shape's style writes them; the
//!   `Paragraph` and `Character` cells of row 0, as a paragraph's or a run's style writes them), over them **the held values whose stamps still hold** (as a shape's), and the properties no
//!   cell reads (`OdfStyle::unmapped`), with the children the style and its properties elements held. A value a cell writes is the cell's; a value no cell writes is the file's.
//! * **The forms of an earlier version.** A held spelling of the group `Style` may hold a weight, a slant or a variant that the schema of ODF 1.4 lists no value for (a file of 1.0 or 1.1 takes any
//!   text there, `fo:font-weight="250"`): it is written as the value 1.4 lists nearest and the value it had rides in an attribute of DiaCraft's own at the end of the same properties element
//!   (`diacraft:older-font-weight`; [`crate::ext::odf14::form_14`]), which a read gives back while the attribute beside it is still the one written.
//! * **What is carried.** What the markup does not say (a cell no attribute is written for, a formula, a face no declaration names) is the difference between the sheet the reader builds from
//!   this markup and the model's, which DiaCraft's document file carries for each style as it carries a style sheet of a file whose cells changed (`ext::docfile`'s `style` entries, read over the
//!   sheet the markup gives): the splice is told this markup is the model's own (`newstyles::plan` on the model path), so a sheet that differs in its cells alone is carried, never refused, and
//!   the file made is read back with every sheet the model holds (`newstyles::check`).
//!
//! **Prefixes.** The part's root holds the model's declarations with the standard ones of `style`, `draw`, `fo`, `svg` and `text` made sure of (`containers::dress`), so this markup uses those
//! prefixes; an attribute or child the file named with a prefix nothing binds in `office:styles` is left out and counted. **Two names of one attribute** (the file's prefix and the standard
//! one for the same namespace) are one: a held value is written over a cell's, and a cell's over a property no cell reads.
//!
//! **Cost.** Each style, attribute and child is a step, and each child is taken from the budget of what the part holds and charged before it is made ([`super::node_total`]).

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, OdfStyle, Passthrough, RawNode, Row, Sheet, Style, TextGroup, stamp_text_group};

use super::{WORDS_KEPT, node_total};
use crate::limits::Limits;
use crate::read::names::Ns;
use crate::read::units::LenUnit;
use crate::write::kept::{Bindings, is_qname, placeable};
use crate::write::lengths::{length_text, unit_of_word};
use crate::write::stylemap::{Op, Val};
use crate::write::{WriteError, WriteWork};

const STYLE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const DRAW_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const FO_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const SVG_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const TEXT_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";

/// The local name of a name as written.
fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, l)| l)
}

/// The ODF data of a style sheet the `.odg` reader made, of a family a style sheet is made of.
fn odf_of(style: &Style) -> Option<&OdfStyle> {
    style.ext.get().map(|d| &d.odf).filter(|o| matches!(o.family.as_deref(), Some("graphic" | "paragraph")) && o.name.is_some())
}

/// How many of the model's style sheets after the root this step writes: the run of those the `.odg` reader made, each with a name, and each numbered by its place (the `ID` the reader gives the
/// style it reads in that place). A name a container holds twice is the file's own and does not end the run.
pub(crate) fn count(doc: &Document) -> usize {
    let mut n = 0usize;
    for (at, style) in doc.styles.iter().enumerate().skip(1) {
        let Some(odf) = odf_of(style) else { break };
        let (Some(_), Some(name)) = (odf.family.as_deref(), odf.name.as_deref()) else { break };
        // A name a container holds twice is the file's own (the first is the one a reference finds): the style that repeats it is written under it again, as read.
        if u32::try_from(at).ok() != Some(style.id) || name.is_empty() {
            break;
        }
        n += 1;
    }
    n
}

/// The attributes the cells of `sheet` write for the group `Style` of a style's text properties (row 0 of `Character`): the namespace, the local name and the value of each, as the writer of a
/// style sheet says them. `None` for a sheet whose row says nothing the writer can write. The reader compares them with what the file spelled.
pub(crate) fn style_group_of_cells(sheet: &Sheet) -> Option<Vec<(Ns, &'static str, String)>> {
    let mut steps = 0u64;
    let no_palette = |_: i64| None;
    let mut rows = crate::write::runs::Rows::of(sheet, false, &no_palette, &mut steps);
    let props = rows.props(0).ok().flatten()?;
    let part = props.parts.iter().find(|p| p.id == "Style")?;
    Some(
        part.ops
            .iter()
            .filter_map(|op| match &op.val {
                Val::Text(text) => Some((op.ns, op.local, text.clone())),
                _ => None,
            })
            .collect(),
    )
}

/// The standard prefix this step writes a namespace under, for the vocabularies of a style's properties.
fn prefix_of(ns: Ns) -> Option<(&'static str, &'static str)> {
    Some(match ns {
        Ns::Style => ("style", STYLE_URI),
        Ns::Draw => ("draw", DRAW_URI),
        Ns::Fo => ("fo", FO_URI),
        Ns::Svg => ("svg", SVG_URI),
        Ns::Text => ("text", TEXT_URI),
        _ => return None,
    })
}

/// The attributes of one properties element, by the namespace and local name each stands for, in the order they are first given: **a later source does not replace an earlier one** unless it
/// is given with `over`.
#[derive(Default)]
struct Props {
    attrs: Vec<(String, String, String)>,
    /// The attributes of DiaCraft's own that carry the exact value an attribute of the standard was written without ([`Props::in_1_4_form`]): the local name of the carrier and the value.
    carried: Vec<(&'static str, String)>,
}

impl Props {
    /// Gives the attribute `name` (as it is written) of the namespace `uri` the value `value`: over one of the same namespace and local name when `over`, else only when there is none. A
    /// property no cell reads that a cell has written with the same value moves to its own place among them (the reader keeps them in the order the file wrote them).
    fn put(&mut self, uri: &str, name: String, value: String, over: bool) {
        let l = local(&name).to_owned();
        match self.attrs.iter().position(|(u, n, _)| u == uri && local(n) == l) {
            Some(at) if over => {
                if let Some(slot) = self.attrs.get_mut(at) {
                    *slot = (uri.to_owned(), name, value);
                }
            }
            Some(at) if self.attrs.get(at).is_some_and(|(_, _, v)| *v == value) => {
                let moved = self.attrs.remove(at);
                self.attrs.push(moved);
            }
            Some(_) => {}
            None => self.attrs.push((uri.to_owned(), name, value)),
        }
    }

    /// Takes out the attributes the group of cells `group` owns (`runs::owned`): what the cells wrote of it, before a held spelling of the group is put in their place.
    fn drop_owned(&mut self, group: &str) {
        for &(ns, name) in crate::write::runs::owned(group) {
            if let Some((_, uri)) = prefix_of(ns) {
                self.attrs.retain(|(u, n, _)| !(u == uri && local(n) == name));
            }
        }
    }

    /// **Writes the attributes of the group `Style` that ODF 1.4 lists no value for as the value it lists nearest** (`ext::odf14::form_14`): the spelling a file of an earlier version
    /// gave (`fo:font-weight="250"`, which the schemas of 1.0 and 1.1 take and 1.4's does not) is held by the style and written as it was, which the schema of the version written refuses. The
    /// value the file had rides in an attribute of DiaCraft's own on the same element ([`Props::element`]), and a read gives it back while the attribute beside it is the one written here. One step for
    /// each attribute; the save is marked as one that writes a form of an earlier version in 1.4's (the card says so in its values group).
    fn in_1_4_form(&mut self, work: &mut WriteWork) -> Result<(), WriteError> {
        work.charge_steps(self.attrs.len())?;
        for (uri, name, value) in self.attrs.iter_mut() {
            let Some(space) = crate::ext::odf14::space_of(uri) else { continue };
            let Some((kind, carrier)) = crate::ext::odf14::listed(space, local(name)) else { continue };
            let Some(form) = crate::ext::odf14::form_14(kind, value) else { continue };
            self.carried.push((carrier, std::mem::replace(value, form.to_owned())));
            work.forms_1_4 = true;
        }
        Ok(())
    }

    /// The element `name` holding the attributes and `children`, and the carriers beside them: each `diacraft:older-...` with the declaration of the prefix on the element, as the used
    /// automatic styles write theirs (`write/text/model.rs`). A carrier is written once and only where nothing of the element has its name or declares the prefix already, which a model of a
    /// hostile file may; one that cannot be written is added to `left`.
    fn element(self, name: &str, children: Vec<Node>, left: &mut usize) -> Option<El> {
        if self.attrs.is_empty() && children.is_empty() {
            return None;
        }
        let mut attrs: Vec<(String, String)> = self.attrs.into_iter().map(|(_, n, v)| (n, v)).collect();
        if !self.carried.is_empty() {
            let prefix = crate::ext::names::PREFIX;
            let declaration = format!("xmlns:{prefix}");
            if attrs.iter().any(|(n, _)| *n == declaration || n.split_once(':').is_some_and(|(p, _)| p == prefix)) {
                *left = left.saturating_add(self.carried.len());
            } else {
                attrs.push((declaration, diacraft_doc::DIACRAFT_URN.to_owned()));
                attrs.extend(self.carried.into_iter().map(|(carrier, value)| (format!("{prefix}:{carrier}"), value)));
            }
        }
        Some(El { name: name.to_owned(), attrs, children })
    }
}

/// The unit a length of the attribute `local` is written in: the unit of the cell it comes from, as the reader set it, else the one a file most often says it in.
fn unit_for(local: &str, sheet: &Sheet, row: Option<&Row>) -> LenUnit {
    let direct = |name: &str| sheet.cell(name).and_then(|c| c.unit.as_deref()).and_then(unit_of_word);
    let in_row = |name: &str| row.and_then(|r| r.cells.get(name)).and_then(|c| c.unit.as_deref()).and_then(unit_of_word);
    let found = match local {
        "stroke-width" => direct("LineWeight"),
        "padding-top" => direct("TopMargin"),
        "padding-bottom" => direct("BottomMargin"),
        "padding-left" => direct("LeftMargin"),
        "padding-right" => direct("RightMargin"),
        "font-size" | "font-size-asian" | "font-size-complex" => in_row("Size"),
        "margin-left" => in_row("IndLeft"),
        "margin-right" => in_row("IndRight"),
        "text-indent" => in_row("IndFirst"),
        "margin-top" => in_row("SpBefore"),
        "margin-bottom" => in_row("SpAfter"),
        "line-height" => in_row("SpLine"),
        "letter-spacing" => in_row("Letterspace"),
        _ => None,
    };
    found.unwrap_or(if local.starts_with("font-size") { LenUnit::Pt } else { LenUnit::Cm })
}

/// Puts the ops `ops` (a cell's attributes) on `props`, each under its standard prefix: a removal is nothing to write, and a namespace this step has no prefix for is left out.
fn put_ops(props: &mut Props, ops: &[Op], sheet: &Sheet, row: Option<&Row>, work: &mut WriteWork) -> Result<(), WriteError> {
    for op in ops {
        work.charge_steps(1)?;
        let Some((prefix, uri)) = prefix_of(op.ns) else { continue };
        let value = match &op.val {
            Val::Remove => continue,
            Val::Text(t) => t.clone(),
            Val::Length(inches) => {
                length_text(*inches, unit_for(op.local, sheet, row)).ok_or(WriteError::Unsupported("a length too large to write"))?
            }
        };
        props.put(uri, format!("{prefix}:{}", op.local), value, true);
    }
    Ok(())
}

/// The URI the prefix of the name `name` (as the file wrote it) is bound to where `bindings` are in force: `None` for a name with no prefix (in no namespace, which no property is in) and one
/// whose prefix nothing binds there.
fn uri_of<'a>(name: &str, bindings: &'a Bindings) -> Option<&'a str> {
    let (prefix, _) = name.split_once(':')?;
    bindings.get(prefix).map(String::as_str).filter(|u| !u.is_empty())
}

/// Row 0 of the sheet's section `section`: the row a common style's paragraph or text properties are read into.
fn row_zero<'a>(sheet: &'a Sheet, section: &str) -> Option<&'a Row> {
    sheet.sections.iter().filter(|s| s.name.as_str() == section).flat_map(|s| s.rows.iter()).find(|r| r.ix == Some(0)).map(|r| &**r)
}

/// The `style:font-face` a family is declared by among the faces the `.odg` reader kept (its `style:name`), for a face the text cells name.
fn face_named(doc: &Document, family: &str) -> Option<String> {
    let unquoted = |s: &str| s.trim().trim_matches(|c| c == '\'' || c == '"').to_owned();
    doc.fonts.iter().filter_map(|face| face.ext.get().map(|d| &d.odf)).find_map(|kept| {
        let name = kept.raw.iter().find(|(k, _)| local(k) == "name").map(|(_, v)| v)?;
        let declared = kept.raw.iter().find(|(k, _)| local(k) == "font-family").map(|(_, v)| unquoted(v));
        (name == family || declared.as_deref() == Some(family)).then(|| name.to_owned())
    })
}

/// What this step wrote and left out.
#[derive(Default)]
pub(crate) struct Written {
    /// The style sheets written natively.
    pub styles: usize,
    /// The pieces of them left out, whose prefix nothing binds in `office:styles` (an attribute or a child of the style or of its properties).
    pub left: usize,
    /// The dashes of DiaCraft's own name that the styles written name, once each: a style sheet's line pattern is a dash its cells write, and the definition is put in
    /// `office:styles` when the model's own definitions (as they were read) hold none of that name.
    pub dashes: Vec<crate::write::dash::DashKey>,
}

/// The common styles of the model's style sheets this step writes (see the [module documentation](self)), to be put at the end of `office:styles`, where `bindings` are in force. `budget` is
/// what is left of the part's cap, which each child kept whole is taken from.
pub(crate) fn common_styles(
    doc: &Document,
    bindings: &Bindings,
    limits: &Limits,
    work: &mut WriteWork,
    budget: &mut usize,
) -> Result<(Vec<El>, Written), WriteError> {
    let n = count(doc);
    let mut out = Vec::with_capacity(n);
    let mut written = Written::default();
    for (at, style) in doc.styles.iter().enumerate().skip(1).take(n) {
        work.charge_steps(1)?;
        let Some(odf) = odf_of(style) else { break };
        // In ODF 1.4's form: the form the save worked out for the style before it wrote anything (`ext::odf14::prepare`).
        let form = crate::ext::odf14::cached(work, at, odf);
        let kept = form.as_ref().and_then(|f| f.kept.as_ref()).unwrap_or(&odf.kept.kept);
        let made = one(doc, style, odf, kept, bindings, limits, work, budget, &mut written.left)?;
        note_dashes(&made, &mut written.dashes, work)?;
        out.push(made);
        written.styles += 1;
    }
    Ok((out, written))
}

/// Notes the dashes of DiaCraft's own name that the style `made` names (`draw:stroke-dash` on a properties element of it) into `dashes`, once each: at most one for each pattern and cap form.
fn note_dashes(made: &El, dashes: &mut Vec<crate::write::dash::DashKey>, work: &mut WriteWork) -> Result<(), WriteError> {
    for child in &made.children {
        let Node::El(props) = child else { continue };
        work.charge_steps(props.attrs.len().saturating_add(1))?;
        for (name, value) in &props.attrs {
            if local(name) == "stroke-dash"
                && let Some(key) = crate::write::dash::DashKey::of_name(value)
                && !dashes.contains(&key)
            {
                dashes
                    .try_reserve(1)
                    .map_err(|_| WriteError::Limit { what: "memory for the dashes of the style sheets", max: dashes.len() as u64 })?;
                dashes.push(key);
            }
        }
    }
    Ok(())
}

/// The ODF name of the style sheet of `ID` `id`, as this step writes it (the root has none).
fn odf_name(doc: &Document, id: u32) -> Option<&str> {
    doc.styles.iter().find(|s| s.id == id).and_then(|s| odf_of(s)).and_then(|o| o.name.as_deref())
}

/// One style sheet's common style.
#[allow(clippy::too_many_arguments)] // the pieces of the part a style is made of, named where they come from
fn one(
    doc: &Document,
    style: &Style,
    odf: &OdfStyle,
    kept: &Passthrough,
    bindings: &Bindings,
    limits: &Limits,
    work: &mut WriteWork,
    budget: &mut usize,
    left: &mut usize,
) -> Result<El, WriteError> {
    let sheet = &*style.sheet;
    let family = odf.family.as_deref().unwrap_or("graphic");
    let mut attrs: Vec<(String, String)> = Vec::new();
    let name = odf.name.as_deref().unwrap_or_default();
    attrs.push(("style:name".to_owned(), name.to_owned()));
    // The name a person sees is the model's (the reader gives a style the display name, else its name): written when it is not the name.
    if let Some(shown) = style.name.as_deref().filter(|shown| *shown != name) {
        attrs.push(("style:display-name".to_owned(), shown.to_owned()));
    }
    attrs.push(("style:family".to_owned(), family.to_owned()));
    // The parent the three links name: one style sheet, or none for the root.
    let links = sheet.links;
    let link = links.line.unwrap_or(0);
    if link != 0
        && links.fill.unwrap_or(0) == link
        && links.text.unwrap_or(0) == link
        && let Some(parent) = odf_name(doc, link)
    {
        attrs.push(("style:parent-style-name".to_owned(), parent.to_owned()));
    }
    // The other attributes the style had, where their prefix is bound.
    for (k, v) in odf.kept.raw.iter() {
        work.charge_steps(1)?;
        if !is_qname(k) || uri_of(k, bindings).is_none() && k.contains(':') {
            *left = left.saturating_add(1);
            continue;
        }
        if attrs.iter().any(|(a, _)| a == k) {
            continue;
        }
        attrs.push((k.to_owned(), v.to_owned()));
    }
    let mut graphic = Props::default();
    let mut paragraph = Props::default();
    let mut text = Props::default();
    // What the cells come to: the graphic properties as a new shape's style writes them, the text's as a paragraph's and a run's.
    if family == "graphic" {
        let (_, mut ops, mut said) = crate::write::shapes::native_style_as(sheet, &[], true, false);
        crate::write::held::graphic_ops_of(sheet, &odf.held, &mut ops, &mut said);
        put_ops(&mut graphic, &ops, sheet, None, work)?;
    }
    let no_palette = |_: i64| None;
    let mut steps = 0u64;
    for (section, is_paragraph) in [("Paragraph", true), ("Character", false)] {
        let row = row_zero(sheet, section);
        let mut rows = crate::write::runs::Rows::of(sheet, is_paragraph, &no_palette, &mut steps);
        // A row the writer cannot say as attributes (a formula, a colour of the palette) is the document file's: nothing of it is written here.
        let Ok(Some(props)) = rows.props(0) else { continue };
        let target = if is_paragraph { &mut paragraph } else { &mut text };
        // A row that states no underline is not given one that says "none" (as a run's style is not): the row holds no `DblUnderline`, and the style would read back with it.
        let ops: Vec<Op> = props
            .ops
            .iter()
            .filter(|o| {
                !(o.local == "text-underline-style"
                    && matches!(&o.val, Val::Text(t) if t == "none")
                    && !row.is_some_and(|r| r.cells.contains_key("DblUnderline")))
            })
            .cloned()
            .collect();
        put_ops(target, &ops, sheet, row, work)?;
        for (attribute, family) in &props.faces {
            if let Some(face) = face_named(doc, family) {
                target.put(STYLE_URI, format!("style:{attribute}"), face, true);
            }
        }
        // The held values of the row whose stamps still hold, over the cells' (as a paragraph's or a run's style writes them).
        if let Some(row) = row {
            for value in odf.text_held.iter().filter(|v| v.group.is_paragraph() == is_paragraph) {
                work.charge_steps(1)?;
                if value.stamp != Some(stamp_text_group(row, value.group)) {
                    continue;
                }
                // The spelling of a group: the attributes the file stated of the group are all of it, so the ones the cells would write beside them are not written.
                if value.group == TextGroup::Style {
                    target.drop_owned("Style");
                }
                for (k, v) in value.attrs.iter() {
                    match uri_of(k, bindings) {
                        Some(uri) if is_qname(k) => target.put(uri, k.to_owned(), v.to_owned(), true),
                        _ => *left = left.saturating_add(1),
                    }
                }
            }
        }
    }
    work.charge_steps(usize::try_from(steps).unwrap_or(usize::MAX))?;
    // The properties no cell reads, each on the element it was read from, under every value a cell or a held value gives.
    for (key, value) in odf.unmapped.iter() {
        work.charge_steps(1)?;
        let Some((element, attribute)) = key.split_once('/') else { continue };
        // A property of an earlier version that ODF 1.4 has no place for rides in the document file (`ext::odf14`).
        if crate::ext::odf14::refused_property(key) {
            continue;
        }
        let target = match local(element) {
            "graphic-properties" => &mut graphic,
            "paragraph-properties" => &mut paragraph,
            "text-properties" => &mut text,
            _ => continue,
        };
        match uri_of(attribute, bindings) {
            Some(uri) if is_qname(attribute) => target.put(uri, attribute.to_owned(), value.to_owned(), false),
            _ => *left = left.saturating_add(1),
        }
    }
    // The children the style held: those of its properties elements go back in them, the others after them.
    let (mut in_graphic, mut in_paragraph, mut in_text, mut after) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // `kept` is in ODF 1.4's form: a list style named, a label width brought to 0; the source's form rides in the document file (`ext::odf14`).
    for placed in kept.iter() {
        work.charge_steps(1)?;
        let RawNode::Element(el) = &placed.node else { continue };
        let holder = match local(&el.name) {
            "graphic-properties" => Some(&mut in_graphic),
            "paragraph-properties" => Some(&mut in_paragraph),
            "text-properties" => Some(&mut in_text),
            _ => None,
        };
        let nodes: Vec<&RawNode> = if holder.is_some() { el.children.iter().collect() } else { vec![&placed.node] };
        let target = match holder {
            Some(list) => list,
            None => &mut after,
        };
        for node in nodes {
            let bytes = node_total(node, *budget).ok_or(WriteError::Unsupported(WORDS_KEPT))?;
            match placeable(node, bindings) {
                Some((true, steps)) => {
                    work.charge_steps(steps)?;
                    *budget = budget.saturating_sub(bytes);
                    work.charge_copy(bytes, limits)?;
                    target.push(diacraft_sheetxml::passthrough::node_to_tree(node));
                }
                Some((false, _)) => *left = left.saturating_add(1),
                None => return Err(WriteError::Unsupported(super::WORDS_NAMES)),
            }
        }
    }
    // The group `Style` the schema of the version written lists no value of is written in the value it lists nearest, the file's carried.
    text.in_1_4_form(work)?;
    let mut children: Vec<Node> = Vec::new();
    for (props, name, kids) in [
        (graphic, "style:graphic-properties", in_graphic),
        (paragraph, "style:paragraph-properties", in_paragraph),
        (text, "style:text-properties", in_text),
    ] {
        work.charge_steps(props.attrs.len())?;
        if let Some(el) = props.element(name, kids, left) {
            children.push(Node::El(el));
        }
    }
    children.extend(after);
    Ok(El { name: "style:style".to_owned(), attrs, children })
}
