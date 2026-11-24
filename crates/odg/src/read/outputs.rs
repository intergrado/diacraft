//! The fingerprints of the native outputs a shape's markup has, for tier 3 (the design's "Tier 3"): what the reader compares the
//! fingerprints the shape's own style carries with (`ext::tier3::judge`).
//!
//! They are made **once for a shape and only for what its fingerprints ask** (`ext::tier3::Needs`): the outline from the shape's element and its geometry
//! element, the text from the text the reader just read natively, the name from the `draw:name` it took, and each cell from the sheet the markup gave, all before
//! tier 2 is put over them. Every byte an outline's fingerprint reads is charged to the document's budget before it is read, and every element looked at is an
//! item.

use diacraft_container::xml::El;
use diacraft_doc::{Shape, Sheet};

use super::Reader;
use super::names::{Names, Ns};
use crate::error::OdgError;
use crate::ext::fingerprint::{
    Outline, OutlineFp, OutlineKind, cell_fp, format_fp, name_fp, other_marker_fp, outline_parts, picture_fp, plain_format_fp, plain_of, text_fp,
};
use crate::ext::tier3::{Natives, Needs};

/// The natives `needs` asks of the shape `el`, whose outline (if it has one) is of `kind`.
pub(super) fn natives<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    el: &'t El,
    kind: Option<OutlineKind>,
    (shape, sheet, other_ends): (&Shape, &Sheet, &[&'static str]),
    needs: &Needs,
) -> Result<Natives, OdgError> {
    let mut out = Natives::default();
    if needs.outline
        && let Some(kind) = kind
    {
        out.outline = outline_of(r, names, el, kind)?;
    }
    if needs.text {
        let plain = shape.text.as_deref().map(plain_of).unwrap_or_default();
        r.take_model_bytes(plain.len())?;
        out.text = Some(text_fp(&plain));
    }
    if needs.format {
        // The formatting of the text as the markup says it: the rows the markup gave, before tier 2 is put over them. A part walked is an item.
        let mut steps = 0u64;
        out.format = Some(match shape.text.as_deref() {
            Some(text) => format_fp(text, sheet, &mut steps),
            None => plain_format_fp(0),
        });
        r.take_items(usize::try_from(steps).unwrap_or(usize::MAX))?;
    }
    if needs.foreign {
        // The picture the markup shows: its length and its first eight bytes (the foreign data of the shape sits beside it).
        out.foreign = Some(picture_fp(shape.foreign.as_deref().and_then(|f| f.picture.as_deref()).map(|p| &p.bytes[..])));
    }
    if needs.name {
        out.name = Some(name_fp(shape.name_u.as_deref()));
    }
    for name in &needs.cells {
        r.take_items(1)?;
        // A line end whose marker is not DiaCraft's own holds the stand-in 4, as DiaCraft's own number 4 does: they are told apart by the marker.
        let fp = if other_ends.contains(&name.as_str()) { other_marker_fp(name) } else { cell_fp(name, sheet.cells.get(name.as_str())) };
        out.cells.insert(name.clone(), fp);
    }
    Ok(out)
}

/// The fingerprint of the outline of the element `el`: a custom shape's geometry element, a path's `svg:d`, a polygon's or polyline's `draw:points`. `None` when
/// a custom shape has no geometry element.
fn outline_of<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, kind: OutlineKind) -> Result<Option<OutlineFp>, OdgError> {
    match kind {
        OutlineKind::Path | OutlineKind::Points => {
            let attrs = names.attrs(el);
            let mut o = Outline::new(kind);
            o.path = if kind == OutlineKind::Path { attrs.peek(Ns::Svg, "d") } else { attrs.peek(Ns::Draw, "points") };
            o.view_box = attrs.peek(Ns::Svg, "viewBox");
            r.take_model_bytes(o.bytes())?;
            Ok(Some(outline_parts(&o)))
        }
        OutlineKind::Custom => {
            let mut found = None;
            for child in el.elements() {
                r.take_items(1)?;
                let mark = names.enter(child);
                let is = names.is(child, Ns::Draw, "enhanced-geometry");
                names.leave(mark);
                if is {
                    found = Some(child);
                    break;
                }
            }
            let Some(eg) = found else { return Ok(None) };
            let mark = names.enter(eg);
            let made = custom_outline(r, names, eg);
            names.leave(mark);
            made
        }
    }
}

/// The outline of a `draw:enhanced-geometry` element, `names` being in its scope.
fn custom_outline<'t>(r: &mut Reader, names: &mut Names<'t>, eg: &'t El) -> Result<Option<OutlineFp>, OdgError> {
    let attrs = names.attrs(eg);
    let mut o = Outline::new(OutlineKind::Custom);
    o.path = attrs.peek(Ns::Draw, "enhanced-path");
    o.view_box = attrs.peek(Ns::Svg, "viewBox");
    o.shape_type = attrs.peek(Ns::Draw, "type");
    o.modifiers = attrs.peek(Ns::Draw, "modifiers");
    for child in eg.elements() {
        r.take_items(1)?;
        let mark = names.enter(child);
        match names.element(child) {
            (Ns::Draw, "equation") => {
                let a = names.attrs(child);
                o.equations.push((a.peek(Ns::Draw, "name").unwrap_or_default(), a.peek(Ns::Draw, "formula").unwrap_or_default()));
            }
            (Ns::Draw, "handle") => {
                let a = names.attrs(child);
                r.take_items(a.len())?;
                o.handles.push(a.iter().map(|x| (x.local, x.value)).collect());
            }
            _ => {}
        }
        names.leave(mark);
    }
    r.take_model_bytes(o.bytes())?;
    Ok(Some(outline_parts(&o)))
}

#[cfg(test)]
mod tests {
    use diacraft_container::Work;

    use super::*;
    use crate::ext::fingerprint::Fp;
    use crate::limits::Limits;

    const HEAD: &str =
        r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#;

    /// The fingerprint of the outline of the root element of `xml` as `kind` reads it.
    fn fp(xml: &str, kind: OutlineKind) -> Option<Fp> {
        let limits = Limits::default();
        let tree = diacraft_container::xml::parse_counted(xml.as_bytes(), &limits.container.xml, &mut Work::default()).unwrap();
        let mut names = Names::default();
        let mark = names.enter(&tree);
        let mut r = Reader::new(&limits);
        let made = outline_of(&mut r, &mut names, &tree, kind).unwrap();
        names.leave(mark);
        made.map(|parts| parts.whole)
    }

    /// A custom shape whose geometry element holds `children`.
    fn custom(children: &str) -> Option<Fp> {
        fp(
            &format!(
                r#"<draw:custom-shape {HEAD}><draw:enhanced-geometry svg:viewBox="0 0 10 10" draw:enhanced-path="M 0 0 Z N">{children}</draw:enhanced-geometry></draw:custom-shape>"#
            ),
            OutlineKind::Custom,
        )
    }

    /// The equations of the geometry are part of its outline: the name and the formula of each, in order; a removed one, a changed one and a swapped pair are other outlines.
    #[test]
    fn an_equation_changed_removed_or_moved_is_another_outline() {
        let two = r#"<draw:equation draw:name="f0" draw:formula="$0 +1"/><draw:equation draw:name="f1" draw:formula="?f0 *2"/>"#;
        let same = custom(two);
        assert!(same.is_some() && same == custom(two));
        assert_ne!(same, custom(r#"<draw:equation draw:name="f0" draw:formula="$0 +1"/>"#), "one removed");
        assert_ne!(same, custom(""), "all removed");
        assert_ne!(same, custom(&two.replace("?f0 *2", "?f0 *3")), "a formula");
        assert_ne!(same, custom(&two.replace(r#"draw:name="f1""#, r#"draw:name="f2""#)), "a name");
        let swapped = r#"<draw:equation draw:name="f1" draw:formula="?f0 *2"/><draw:equation draw:name="f0" draw:formula="$0 +1"/>"#;
        assert_ne!(same, custom(swapped), "the order");
        // An element that is no equation, and another attribute of an equation, are not part of it.
        assert_eq!(same, custom(&format!(r#"<draw:other draw:name="x"/>{two}"#)));
        assert_eq!(same, custom(&two.replace(r#"draw:name="f0""#, r#"draw:name="f0" draw:note="n""#)));
    }

    /// The handles of the geometry are part of its outline: each one's attributes (in any order of writing), its removal, and a change of one value.
    #[test]
    fn a_handle_changed_or_removed_is_another_outline() {
        let one = r#"<draw:handle draw:handle-position="$0 top" draw:handle-range-x-minimum="0"/>"#;
        let same = custom(one);
        assert!(same.is_some() && same == custom(one));
        assert_ne!(same, custom(""), "removed");
        assert_ne!(same, custom(&one.replace("$0 top", "$0 bottom")), "a value");
        assert_ne!(same, custom(&format!("{one}{one}")), "another handle");
        assert_ne!(same, custom(r#"<draw:handle draw:handle-position="$0 top"/>"#), "an attribute removed");
        assert_eq!(
            same,
            custom(r#"<draw:handle draw:handle-range-x-minimum="0" draw:handle-position="$0 top"/>"#),
            "the attributes are told by name, not by the order they are written in"
        );
        // A handle is not an equation: the same attributes on an equation make another outline.
        assert_ne!(custom(one), custom(&one.replace("draw:handle", "draw:equation")));
    }

    /// A path is read by its `svg:d` and a polygon by its `draw:points`, and each by its view box; the other's attribute is no part of the outline.
    #[test]
    fn a_path_is_read_by_its_d_and_a_polygon_by_its_points() {
        let shape =
            |d: &str, points: &str, view_box: &str| format!(r#"<draw:path {HEAD} svg:d="{d}" draw:points="{points}" svg:viewBox="{view_box}"/>"#);
        let path = |d: &str, points: &str, view_box: &str| fp(&shape(d, points, view_box), OutlineKind::Path);
        let points = |d: &str, points: &str, view_box: &str| fp(&shape(d, points, view_box), OutlineKind::Points);
        let base = ("M 0 0", "0,0 1,1", "0 0 9 9");
        assert!(path(base.0, base.1, base.2).is_some());
        assert_eq!(path(base.0, base.1, base.2), path(base.0, "5,5 6,6", base.2), "a path is no polygon: its points are not read");
        assert_ne!(path(base.0, base.1, base.2), path("M 1 1", base.1, base.2), "its d is");
        assert_ne!(path(base.0, base.1, base.2), path(base.0, base.1, "0 0 8 8"), "its view box is");
        assert_eq!(points(base.0, base.1, base.2), points("M 9 9", base.1, base.2), "a polygon is no path: its d is not read");
        assert_ne!(points(base.0, base.1, base.2), points(base.0, "5,5 6,6", base.2), "its points are");
        assert_ne!(points(base.0, base.1, base.2), points(base.0, base.1, "0 0 8 8"), "its view box is");
        assert_ne!(path(base.0, base.1, base.2), points(base.0, base.1, base.2), "the kinds are not each other");
    }

    /// A custom shape with no geometry element has no outline; the work is charged for each element looked at and for each attribute of a handle.
    #[test]
    fn a_custom_shape_with_no_geometry_has_no_outline_and_the_elements_looked_at_are_charged() {
        assert_eq!(fp(&format!(r#"<draw:custom-shape {HEAD}><draw:other/></draw:custom-shape>"#), OutlineKind::Custom), None);
        // The items the work counts for a shape made of `before` (elements ahead of the geometry) and `inside` (its children).
        let cost = |before: &str, inside: &str| {
            let limits = Limits::default();
            let xml = format!(
                r#"<draw:custom-shape {HEAD}>{before}<draw:enhanced-geometry svg:viewBox="0 0 1 1">{inside}</draw:enhanced-geometry></draw:custom-shape>"#
            );
            let tree = diacraft_container::xml::parse_counted(xml.as_bytes(), &limits.container.xml, &mut Work::default()).unwrap();
            let mut names = Names::default();
            let mark = names.enter(&tree);
            let mut r = Reader::new(&limits);
            assert!(outline_of(&mut r, &mut names, &tree, OutlineKind::Custom).unwrap().is_some());
            names.leave(mark);
            r.report.work.items
        };
        let base = cost("", "");
        assert_eq!(cost("<draw:other/>", "") - base, 1, "an element looked at before the geometry is an item");
        assert_eq!(cost("<draw:other/><draw:other/>", "") - base, 2);
        assert_eq!(
            cost("", r#"<draw:equation draw:name="a" draw:formula="1"/>"#) - base,
            1,
            "a child of the geometry is an item (its attributes are not)"
        );
        assert_eq!(cost("", r#"<draw:handle draw:a="1"/>"#) - cost("", "<draw:other/>"), 1, "a handle's one attribute is an item");
        assert_eq!(cost("", r#"<draw:handle draw:a="1" draw:b="2" draw:c="3"/>"#) - cost("", "<draw:other/>"), 3, "and each of three is");
    }

    /// **Of the review.** The bytes a custom outline hashes are charged before they are: the path, the view box, the type, the modifiers, each equation's text and
    /// each attribute of a handle, at one item for [`ITEM_BYTES`] bytes.
    #[test]
    fn the_bytes_a_custom_outline_hashes_are_charged_before_they_are() {
        use crate::read::ITEM_BYTES;
        let cost = |attrs: &str, children: &str| {
            let limits = Limits::default();
            let xml = format!(
                r#"<draw:custom-shape {HEAD}><draw:enhanced-geometry svg:viewBox="0 0 1 1" {attrs}>{children}</draw:enhanced-geometry></draw:custom-shape>"#
            );
            let tree = diacraft_container::xml::parse_counted(xml.as_bytes(), &limits.container.xml, &mut Work::default()).unwrap();
            let mut names = Names::default();
            let mark = names.enter(&tree);
            let mut r = Reader::new(&limits);
            assert!(outline_of(&mut r, &mut names, &tree, OutlineKind::Custom).unwrap().is_some());
            names.leave(mark);
            r.report.work.items
        };
        let base = cost("", "");
        let block = "x".repeat(ITEM_BYTES * 64);
        for (what, attrs, children) in [
            ("the path", format!(r#"draw:enhanced-path="{block}""#), String::new()),
            ("the type", format!(r#"draw:type="{block}""#), String::new()),
            ("the modifiers", format!(r#"draw:modifiers="{block}""#), String::new()),
            ("an equation's formula", String::new(), format!(r#"<draw:equation draw:name="a" draw:formula="{block}"/>"#)),
            ("a handle's attribute", String::new(), format!(r#"<draw:handle draw:v="{block}"/>"#)),
        ] {
            let grown = cost(&attrs, &children) - cost("", &children.replace(&block, ""));
            assert_eq!(grown, 64, "{what}: the bytes are 64 items' worth");
        }
        assert!(base > 0);
    }
}
