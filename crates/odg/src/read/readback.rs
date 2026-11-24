//! Whether the reader reads an attribute's text, asked by name: the question the save puts to each attribute it writes, so that a save the
//! reader would then refuse or misread is found before it is given. Each answer is the reader's own parser's, never a copy of it.

use super::geometry::{self, Budget};
use super::names::Ns;
pub(crate) use super::text::MOST_SPACES;
use super::{shapes, units};

/// Whether the reader reads `text` as the attribute `local` of `ns` on a shape element. An attribute the reader takes as any text is read.
pub(crate) fn reads(ns: Ns, local: &str, text: &str) -> bool {
    match (ns, local) {
        (Ns::Svg, "x" | "y" | "width" | "height" | "cx" | "cy" | "r" | "rx" | "ry" | "x1" | "y1" | "x2" | "y2") => units::length(text).is_some(),
        (Ns::Fo, "page-width" | "page-height" | "margin-left" | "margin-right" | "margin-top" | "margin-bottom") => units::length(text).is_some(),
        (Ns::Fo, "padding-top" | "padding-bottom" | "padding-left" | "padding-right") => units::length(text).is_some(),
        (Ns::Svg, "stroke-width") => units::length(text).is_some(),
        (Ns::Draw, "marker-start-width" | "marker-end-width") => units::length(text).is_some(),
        (Ns::Draw, "marker-start-center" | "marker-end-center") => units::boolean(text).is_some(),
        (Ns::Fo, "font-size") => units::length(text).is_some() || units::percent(text).is_some(),
        (Ns::Fo, "text-indent") => units::length(text).is_some(),
        (Ns::Style, "print-orientation") => matches!(text, "portrait" | "landscape"),
        (Ns::Draw, "points") => matches!(geometry::points(text, usize::MAX, &mut Budget::new(usize::MAX)), Ok(Some(_))),
        (Ns::Draw, "modifiers") => text.split_ascii_whitespace().all(|t| units::number_any(t).is_some()),
        // A custom shape's own glue points: pairs of numbers (writes no formula there), and their ways out, one angle each.
        (Ns::Draw, "glue-points") => {
            let mut count = 0usize;
            text.split_ascii_whitespace().all(|t| {
                count += 1;
                units::number_any(t).is_some()
            }) && count.is_multiple_of(2)
                && count > 0
        }
        (Ns::Draw, "glue-point-leaving-directions") => text.split_ascii_whitespace().all(|t| units::angle_degrees(t).is_some()),
        (Ns::Draw, "enhanced-path") => text
            .split_ascii_whitespace()
            .all(|t| units::number_any(t).is_some() || t.starts_with(['?', '$']) || (t.len() == 1 && "MLCQZNFSTUABVWGXY".contains(t))),
        (Ns::Draw, "transform") => shapes::transform_reads(text),
        (Ns::Draw, "z-index") => units::integer(text).is_some(),
        (Ns::Draw, "start-glue-point" | "end-glue-point") => units::integer(text).and_then(|n| u32::try_from(n).ok()).is_some(),
        (Ns::Svg, "viewBox") => matches!(geometry::view_box(text), Some(Ok(_))),
        (Ns::Svg, "d") => {
            // A path is read in a unit box and as many commands as it holds: the part's size has bounded them.
            let unit_box = geometry::ViewBox { left: 0.0, top: 0.0, width: 1.0, height: 1.0 };
            matches!(geometry::path(text, &unit_box, (1.0, 1.0), &mut Budget::new(usize::MAX)), Ok(Ok(_)))
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_attribute_the_save_writes_is_read_by_the_parser_the_reader_uses_for_it() {
        for (ns, local, good, bad) in [
            (Ns::Svg, "x", "1.5cm", "1.5"),
            (Ns::Svg, "width", "10mm", "wide"),
            (Ns::Svg, "cy", "-3pt", "1 cm"),
            (Ns::Draw, "transform", "rotate(0.5) translate(1cm 2cm)", "rotate(0.5"),
            (Ns::Draw, "z-index", "12", "1.5"),
            (Ns::Draw, "start-glue-point", "3", "-1"),
            (Ns::Svg, "viewBox", "0 0 100 100", "0 0 0 100"),
            (Ns::Svg, "d", "M 0 0 L 1 1", "M 0 0 Q"),
            (Ns::Draw, "marker-start-width", "6.54pt", "wide"),
            (Ns::Draw, "marker-end-width", "9.42pt", "6.54"),
            (Ns::Draw, "marker-start-center", "true", "maybe"),
            (Ns::Draw, "marker-end-center", "false", "1.5"),
            (Ns::Fo, "font-size", "12pt", "big"),
            (Ns::Fo, "font-size", "150%", "1.5.5"),
            (Ns::Fo, "text-indent", "-0.5cm", "indented"),
        ] {
            assert!(reads(ns, local, good), "{local}: {good}");
            assert!(!reads(ns, local, bad), "{local}: {bad}");
        }
        // An attribute the reader takes as any text, and one it does not read.
        assert!(reads(Ns::Draw, "start-shape", ""));
        assert!(reads(Ns::Draw, "name", "anything"));
    }
}
