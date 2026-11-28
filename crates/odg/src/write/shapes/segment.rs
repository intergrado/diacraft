//! A new one-dimensional shape of a geometry of its own that is glued at no end, written as the polyline or the path it draws ([`crate::write::poly`] says which shapes, and how the
//! points are placed). **A glued one is a connector** (a polyline has no glue another program keeps; [`super::Editor::connector_text`]).
//!
//! The element is a `draw:polyline` of `draw:points` (a `draw:path` of `svg:d` when a row is a curve) in a view box of the box of its outline, with the style of the segment as a `draw:line`
//! has it (its stroke, its dash, its line ends: [`super::Editor::line_text`] writes the same style), its layer, its name, its text and its alt text. **What the element cannot say is tier 2**:
//! the reader gives it back as a 2-D shape of its box, and DiaCraft's data carries the segment's frame (its ends, the pose they work out) and its geometry over that shape
//! ([`native::Frame::Segment`]), with the fingerprint of the outline written, so that a polyline another program edits is told and the markup then stands (`ext::tier3`).

use super::{Editor, Element, NewShape, NewWrote, TierCtx, WriteError, native, refuse, standard_connection};
use crate::ext::fingerprint::{Outline, OutlineKind, outline_fp};
use crate::read::names::Ns;
use crate::write::compare::PageDiff;
use crate::write::emit::Pfx;
use crate::write::poly::{Placed, inches};

impl Editor<'_, '_> {
    /// A new segment written as its outline: the element `placed` says, in the page's coordinates. See the module documentation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn poly_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &Pfx,
        id: Option<&str>,
        z: Option<i64>,
        tc: &TierCtx<'_>,
        placed: &Placed,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        if n.shape.name_u.as_deref().is_some_and(crate::write::splice::has_bad_char)
            || n.shape.name.as_deref().is_some_and(crate::write::splice::has_bad_char)
        {
            return refuse(crate::write::splice::WORDS_XML_CHAR);
        }
        let parent = self.parent_of(sheet)?;
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_before = std::mem::take(&mut self.kept.before);
        let alt = std::mem::take(&mut self.kept.alt);
        let asread = self.kept.read.take();
        // Its text, as a line's is: the characters of `text:p`, and the rest of what a text holds tier 2.
        let (paragraphs, holds_text, read, plain, text_native) = self.paragraphs_text(n, pfx)?;
        let (paragraphs, read) =
            if !holds_text && asread.as_ref().is_some_and(|r| !r.bare_text) { (String::new(), Vec::new()) } else { (paragraphs, read) };
        let (_, mut ops, mut style_names) = super::native_style_as(sheet, &[], asread.is_some(), true);
        crate::write::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = if asread.is_some() { self.in_cell_units(ops, sheet) } else { ops };
        let display = crate::write::fx::display_of(sheet, &ops);
        let layer = diff.layers.of_shape(sheet);
        let (layer_text, layer_row) = match &asread {
            Some(r) => (r.layer.clone(), r.layer.as_deref().and_then(|name| diff.layers.row_named(name))),
            None => (Some(layer.name.clone()), layer.row),
        };
        // The outline the reader will fingerprint of the markup written: the points (or the path's data) and the view box.
        let mut outline = Outline::new(if placed.path { OutlineKind::Path } else { OutlineKind::Points });
        outline.path = Some(&placed.data);
        outline.view_box = Some(&placed.view_box);
        let native = native::Native {
            alt: Some((alt.0.as_str(), alt.1.as_str())),
            frame: native::Frame::Segment,
            style: &style_names,
            display: Some(&display),
            geometry: false,
            outline_sections: 1,
            picture: false,
            // (The reader gives a polyline the four standard connection rows, as it gives every shape: a segment that holds none has them said gone.)
            connection: !sheet.sections.iter().any(|s| s.name.as_str() == "Connection") || standard_connection(sheet),
            connector: false,
            route: None,
            glued: [false, false],
            text: &plain,
            text_native,
            format: self.format_written(diff, n, &plain, text_native),
            picture_bytes: None,
            expected: diacraft_doc::ShapeType::Shape,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: Some(outline_fp(&outline)),
        };
        // (It is glued at no end: the caller wrote a glued segment as a connector.)
        let (tier2, dia) = self.extension_of(n, &native, tc)?;
        let (graphic, paragraph) = match &asread {
            Some(_) => (self.graphic_style(asread.as_ref(), ops, &dia, parent.as_deref())?, None),
            None => {
                let (graphic, paragraph) = self.new_styles_on(ops, false, &dia, parent.as_deref())?;
                (Some(graphic), Some(paragraph))
            }
        };
        let element = if placed.path { "path" } else { "polyline" };
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let mut tag = format!("<{0}:{element}{1}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), paragraph.as_deref(), layer_text.as_deref()), z, id, &mut wrote));
        // The box of the outline on the page, as lengths in the document's unit; the view box and the data are whole numbers of hundredths of a millimetre from its corner.
        for (local, hundredths) in [("x", placed.left), ("y", placed.top), ("width", placed.width), ("height", placed.height)] {
            tag.push_str(&pfx.attr(Ns::Svg, local, &self.length(inches(hundredths))?));
            wrote.push((Ns::Svg, super::static_name(local)));
        }
        tag.push_str(&pfx.attr(Ns::Svg, "viewBox", &placed.view_box));
        wrote.push((Ns::Svg, "viewBox"));
        self.work.charge_copy(placed.data.len(), self.limits)?;
        if placed.path {
            tag.push_str(&pfx.attr(Ns::Svg, "d", &placed.data));
            wrote.push((Ns::Svg, "d"));
        } else {
            tag.push_str(&pfx.attr(Ns::Draw, "points", &placed.data));
            wrote.push((Ns::Draw, "points"));
        }
        self.work.charge_attribute(self.limits)?;
        let tier2 = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        Ok((
            format!("{tag}>{kept_before}{paragraphs}{kept_body}</{}:{element}>", pfx.draw),
            NewWrote { attrs: wrote, paras: read, tier2, ..NewWrote::default() },
        ))
    }
}
