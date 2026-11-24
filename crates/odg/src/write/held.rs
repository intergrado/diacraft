//! The held graphic values of a new shape, written into its automatic style while their stamps still match the cells (the native-format design's §3.3 and §3.4).
//!
//! A shape the `.odg` reader made keeps, beside the stand-in cells, what the file said that the cells can only approximate: a hatch, a gradient or a picture for its fill (and the
//! name of its opacity gradient), a dash or a join its line has, the markers and widths of its line ends that DiaCraft did not write, its shadow, and the alignment, padding and writing
//! mode of its text area. Each value is held **as the file wrote it, with the stamp of the cells it stands beside** (`read/held.rs`). **A value whose stamp still matches the shape's
//! cells is written as held** (its attributes go on the new automatic graphic style, over what the cells would write for the same attribute: the cells' own `draw:fill` of a
//! stand-in pattern, the marker DiaCraft draws for a stand-in end); **one whose cells changed is not**, and the style is written from the cells alone, as for a new shape. An edit
//! does not drop the value, so an undo that puts the cells back makes it good again (§3.3).
//!
//! The attributes written are the ones the reader holds for a value; a name this module has no place for (a `loext:` attribute, a prefix other than the ones ODF's own namespaces
//! are given) is left out, and the model keeps it.

use diacraft_doc::{Held, HeldGroup, Shape};

use super::stylemap::{GRAPHIC, Op, Val};
use crate::read::names::Ns;
use crate::read::{HeldEnd as End, HeldValue as Value};

/// The namespace and the local name an attribute of a held value is written under, by the name the file wrote it with: only the attributes the reader holds.
fn attribute(name: &str) -> Option<(Ns, &'static str)> {
    Some(match name {
        "draw:fill" => (Ns::Draw, "fill"),
        "draw:fill-gradient-name" => (Ns::Draw, "fill-gradient-name"),
        "draw:fill-hatch-name" => (Ns::Draw, "fill-hatch-name"),
        "draw:fill-hatch-solid" => (Ns::Draw, "fill-hatch-solid"),
        "draw:fill-image-name" => (Ns::Draw, "fill-image-name"),
        "draw:fill-image-width" => (Ns::Draw, "fill-image-width"),
        "draw:fill-image-height" => (Ns::Draw, "fill-image-height"),
        "draw:fill-image-ref-point" => (Ns::Draw, "fill-image-ref-point"),
        "draw:fill-image-ref-point-x" => (Ns::Draw, "fill-image-ref-point-x"),
        "draw:fill-image-ref-point-y" => (Ns::Draw, "fill-image-ref-point-y"),
        "draw:opacity-name" => (Ns::Draw, "opacity-name"),
        "draw:stroke" => (Ns::Draw, "stroke"),
        "draw:stroke-dash" => (Ns::Draw, "stroke-dash"),
        "draw:stroke-dash-names" => (Ns::Draw, "stroke-dash-names"),
        "draw:stroke-linejoin" => (Ns::Draw, "stroke-linejoin"),
        "draw:marker-start" => (Ns::Draw, "marker-start"),
        "draw:marker-start-width" => (Ns::Draw, "marker-start-width"),
        "draw:marker-start-center" => (Ns::Draw, "marker-start-center"),
        "draw:marker-end" => (Ns::Draw, "marker-end"),
        "draw:marker-end-width" => (Ns::Draw, "marker-end-width"),
        "draw:marker-end-center" => (Ns::Draw, "marker-end-center"),
        "draw:shadow" => (Ns::Draw, "shadow"),
        "draw:shadow-color" => (Ns::Draw, "shadow-color"),
        "draw:shadow-opacity" => (Ns::Draw, "shadow-opacity"),
        "draw:shadow-offset-x" => (Ns::Draw, "shadow-offset-x"),
        "draw:shadow-offset-y" => (Ns::Draw, "shadow-offset-y"),
        "draw:textarea-vertical-align" => (Ns::Draw, "textarea-vertical-align"),
        "fo:padding" => (Ns::Fo, "padding"),
        "style:writing-mode" => (Ns::Style, "writing-mode"),
        _ => return None,
    })
}

/// Which value a held entry is, from its group and the attributes it holds (`read/held.rs` makes each value of one group from its own attributes): `None` for a value that is no
/// graphic property of a style (the frame, the geometry, the layer and a glue point: they are the element's or the row's), and for the tab stops (an element of their own).
fn value_of(held: &Held) -> Option<Value> {
    let has = |name: &str| held.attrs.get(name).is_some();
    match held.group {
        HeldGroup::Fill => {
            if has("draw:opacity-name") {
                Some(Value::FillOpacity)
            } else if held.attrs.get("draw:fill") == Some("hatch") {
                Some(Value::HatchFill)
            } else {
                Some(Value::FillStyle)
            }
        }
        HeldGroup::Line => {
            if has("draw:stroke-dash") {
                Some(Value::Dash)
            } else if has("draw:stroke") {
                Some(Value::DashStroke)
            } else if has("draw:stroke-linejoin") {
                Some(Value::LineJoin)
            } else {
                None
            }
        }
        HeldGroup::LineEnds => {
            let begin = has("draw:marker-start") || has("draw:marker-start-width") || has("draw:marker-start-center");
            let end = has("draw:marker-end") || has("draw:marker-end-width") || has("draw:marker-end-center");
            let end_of = match (begin, end) {
                (true, false) => End::Begin,
                (false, true) => End::End,
                // A value is one end's: an entry that holds both is not one this reader made.
                _ => return None,
            };
            // A marker DiaCraft did not write is named; the width of a marker it wrote is held with no name.
            let foreign = has("draw:marker-start") || has("draw:marker-end");
            Some(if foreign { Value::ForeignMarker(end_of) } else { Value::OwnMarkerWidth(end_of) })
        }
        HeldGroup::Shadow => Some(Value::Shadow),
        HeldGroup::Text => {
            if has("draw:textarea-vertical-align") {
                Some(Value::TextJustify)
            } else if has("fo:padding") {
                Some(Value::TextPadding)
            } else if has("style:writing-mode") {
                Some(Value::TextWriting)
            } else {
                None
            }
        }
        HeldGroup::Frame | HeldGroup::Glue | HeldGroup::Geometry => None,
    }
}

/// The attributes a held value **says whole while its stamp holds**, so that the cells' stand-ins for any of them are not written beside it (found 207 trips that lost a
/// line-end property and 272 a gradient reference): **a fill** says its kind and the names of its gradient, hatch and picture as the style wrote them, and what it leaves unsaid the style's parent says
/// (a `draw:fill="solid"` written from the cells beside a name alone took the parent's gradient away); **a line end of another program's marker** says its marker, width and centre, and a width it
/// leaves unsaid is the parent's (a width worked out from the cells was added beside a marker named alone). The other values say only their own attributes.
fn says_whole(value: Value) -> &'static [(Ns, &'static str)] {
    const FILL: [(Ns, &str); 4] =
        [(Ns::Draw, "fill"), (Ns::Draw, "fill-gradient-name"), (Ns::Draw, "fill-hatch-name"), (Ns::Draw, "fill-image-name")];
    const BEGIN: [(Ns, &str); 3] = [(Ns::Draw, "marker-start"), (Ns::Draw, "marker-start-width"), (Ns::Draw, "marker-start-center")];
    const END: [(Ns, &str); 3] = [(Ns::Draw, "marker-end"), (Ns::Draw, "marker-end-width"), (Ns::Draw, "marker-end-center")];
    match value {
        Value::FillStyle | Value::HatchFill => &FILL,
        Value::ForeignMarker(End::Begin) => &BEGIN,
        Value::ForeignMarker(End::End) => &END,
        _ => &[],
    }
}

/// Puts the attributes of the shape's held graphic values that are still good into `ops`, the attributes of the new automatic graphic style: **a held attribute replaces the op that
/// writes the same attribute** (the cells' stand-in for it), and is put after the others. Nothing changes for a shape that holds no value, or whose values are stale.
///
/// **A held value whose stamp holds says the cells it was stamped by**, as the reader made them from it, so those cells are no tier 2 (a dashed line's `LinePattern`, a shadow's
/// six cells): they are put in `said`. Only a shape an `.odg` reader made holds values.
pub(crate) fn graphic_ops_said(shape: &Shape, ops: &mut Vec<Op>, said: &mut Vec<&'static str>) {
    let Some(slot) = shape.ext.get() else { return };
    graphic_ops_of(&shape.sheet, &slot.odf.held, ops, said);
}

/// [`graphic_ops_said`] for the held values `values` of a sheet's owner, a shape's or a style sheet's (a style sheet holds the same values, held in the same way, for a common
/// style, `diacraft_doc::OdfStyle::held`).
pub(crate) fn graphic_ops_of(sheet: &diacraft_doc::Sheet, values: &[Held], ops: &mut Vec<Op>, said: &mut Vec<&'static str>) {
    let mut held: Vec<Op> = Vec::new();
    // The attributes a held value says whole: the cells' ops of any of them go, whether the value holds that attribute or not.
    let mut whole: Vec<(Ns, &'static str)> = Vec::new();
    for entry in values {
        let Some(value) = value_of(entry) else { continue };
        // The stamp is the cells' now against the cells' when the reader held the value: equal while nothing it stands beside was edited. A value with no stamp (a foreign dash, a join
        // held beside no cell) is written as held on every save.
        if entry.stamp != value.stamp(sheet) {
            continue;
        }
        if matches!(value, Value::DashStroke | Value::Shadow) {
            said.extend(value.cells());
        }
        whole.extend(says_whole(value));
        for (name, text) in entry.attrs.iter() {
            if let Some((ns, local)) = attribute(name) {
                held.push(Op { prop: GRAPHIC, ns, local, val: Val::Text(text.to_owned()) });
            }
        }
    }
    if held.is_empty() {
        return;
    }
    ops.retain(|o| !(o.prop == GRAPHIC && (held.iter().any(|h| h.ns == o.ns && h.local == o.local) || whole.contains(&(o.ns, o.local)))));
    // **A held bitmap fill is in force, so the attributes that go with a picture are its own**: the stand-in pattern's `draw:fill` takes them out (`stylemap`'s fill ops),
    // and that taking out would drop the ones the style kept beside the held value, its repeat mode (`style:repeat`) among them, which no cell and no held value writes.
    let bitmap = held.iter().any(|h| h.ns == Ns::Draw && h.local == "fill" && h.val == Val::Text("bitmap".to_owned()));
    if bitmap {
        ops.retain(|o| !(o.prop == GRAPHIC && o.val == Val::Remove && super::stylemap::goes_with_a_picture(o.ns, o.local)));
    }
    ops.extend(held);
}

/// The `draw:enhanced-geometry` element a custom shape kept, **when it is still the shape's outline**: the value of the geometry group that holds nothing but a stamp
/// ([`HeldValue::EnhancedGeometry`], whose element stays in the shape's kept markup), with the stamp of the `Geometry` sections and the equations' rows the reader set it with
/// still the cells' now. `None` for a shape that holds none, and for one whose outline was edited (a save then writes the outline again from the rows, as for a new shape). **The
/// element is the first kept one that `is_geometry` says is in ODF's drawing namespace** (the caller knows the namespaces where the shape is written), never one
/// that has the local name alone.
pub(crate) fn enhanced_geometry(shape: &Shape, is_geometry: impl Fn(&diacraft_doc::RawElement) -> bool) -> Option<&diacraft_doc::RawElement> {
    let slot = shape.ext.get()?;
    let entry = slot.odf.held.iter().find(|h| h.group == HeldGroup::Geometry && h.attrs.is_empty() && h.kept.is_empty())?;
    if entry.stamp != Value::EnhancedGeometry.stamp(&shape.sheet) {
        return None;
    }
    slot.odf.kept.kept.iter().find_map(|p| match &p.node {
        diacraft_doc::RawNode::Element(e) if is_geometry(e) => Some(e),
        _ => None,
    })
}
