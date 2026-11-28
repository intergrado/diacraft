//! The mirrors and the turn of the text of a held `draw:enhanced-geometry`, made the session's.
//!
//! A custom shape the `.odg` reader made keeps the geometry element its file had, and the model writer writes it back while its stamp (the `Geometry` rows and the equations) matches. The
//! element says three things the stamp does not cover, and the session can change each without a row changing: its mirrors (`draw:mirror-horizontal` and `draw:mirror-vertical`, which the
//! reader gives back as `FlipX` and `FlipY`), and the turn of its text (`draw:text-rotate-angle`, which the reader gives back as `TxtAngle` beside the flips,
//! [`crate::read::text_turn_beside_flips`]). Written as the file had them, a flip made in the session was in no file: the shape came back unflipped, with nothing said.
//!
//! [`mirrored`] makes the element say what the shape has now, and leaves it as it was when it says that already.

use diacraft_doc::{RawAttrs, RawElement};

use crate::read::text_turn_beside_flips;
use crate::read::units::{angle_degrees, boolean};

/// The words of the refusal of a geometry element that has no prefix to name its mirrors by: `draw:mirror-horizontal` has to be written, and the element is in the drawing namespace by
/// a default declaration, so the prefix of the writer's own is not known to it.
pub(crate) const WORDS_NO_PREFIX: &str = "a flip of a shape whose geometry element is written with no prefix (its mirrors cannot be written there without a prefix for the drawing namespace)";

/// The value of the attribute `local` of the drawing namespace on the element, found by the prefix the element's own name has (the attribute is in the namespace the element is in): `None`
/// for an element whose name has no prefix, since an attribute with none is in no namespace.
fn attr_of<'a>(el: &'a RawElement, local: &str) -> Option<&'a str> {
    let (prefix, _) = el.name.split_once(':')?;
    el.attrs.get(&format!("{prefix}:{local}"))
}

/// The words of the refusal of a flip of a shape whose geometry is an object the reader does not draw (a 3-D extrusion, a fontwork): it keeps no mirror of such a geometry
/// So a mirror written would be read back as none and the flip would be lost.
pub(crate) const WORDS_OBJECT_FLIP: &str =
    "a flip of a shape whose geometry is a 3-D extrusion or a fontwork (the file's readers keep no mirror for it, so the flip would be lost)";

/// The words of the refusal of a text on a shape whose geometry is such an object: the reader reads no text from it (the shape is a foreign object on its frame), so
/// a text written would be read back as none. The save into a file refuses the same text on an object frame (`own::WORDS_FRAME_TEXT`).
pub(crate) const WORDS_OBJECT_TEXT: &str = "a text on a shape whose geometry is a 3-D extrusion or a fontwork (the file's readers read no text from such a shape, so the text would be lost)";

/// Whether the element is an object the reader does not draw: a 3-D extrusion or a fontwork (`draw:extrusion` or `draw:text-path` true).
pub(crate) fn is_object(el: &RawElement) -> bool {
    ["extrusion", "text-path"].iter().any(|local| attr_of(el, local).and_then(boolean).unwrap_or(false))
}

/// The two mirrors an element says as the reader reads them: `false` for an attribute that is not `true`.
pub(crate) fn mirrors_of(el: &RawElement) -> (bool, bool) {
    let flag = |local: &str| attr_of(el, local).and_then(boolean).unwrap_or(false);
    (flag("mirror-horizontal"), flag("mirror-vertical"))
}

/// The turn of the text the element says, in degrees as the file writes it (`draw:text-rotate-angle`), 0 when it says none or none that reads.
fn text_turn_of(el: &RawElement) -> f64 {
    attr_of(el, "text-rotate-angle").and_then(angle_degrees).unwrap_or(0.0)
}

/// The turn of a custom shape's text the file says (`draw:text-rotate-angle`, degrees) for the reader to give `txt` degrees (`TxtAngle`) beside the flips: the inverse of
/// [`text_turn_beside_flips`], in (-180, 180].
pub(crate) fn file_text_turn(txt: f64, flip_x: bool, flip_y: bool) -> f64 {
    let turn = if flip_y { txt - 180.0 } else { txt };
    let file = if flip_x != flip_y { -turn } else { turn };
    let r = file.rem_euclid(360.0);
    // What a computation leaves past nine places is noise.
    let r = (r * 1.0e9).round() / 1.0e9;
    if r > 180.0 { r - 360.0 } else { r }
}

/// Whether two turns in degrees are the same turn, a whole turn apart or not.
fn same_turn(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(360.0);
    d < 1.0e-7 || 360.0 - d < 1.0e-7
}

/// Whether the element says the shape's mirrors (`flip`: `FlipX`, `FlipY`) and a turn of its text that reads as the shape's `TxtAngle` (`txt`, degrees) beside them. A copy of it made by
/// [`mirrored`] is needed exactly when it does not.
pub(crate) fn differs(el: &RawElement, flip: (bool, bool), txt: f64) -> bool {
    mirrors_of(el) != flip || !same_turn(text_turn_beside_flips(text_turn_of(el), flip.0, flip.1), txt)
}

/// The element with its mirrors the shape's (`flip`: `FlipX`, `FlipY`) and the turn of its text the one that reads as the shape's `TxtAngle` (`txt`, degrees, counterclockwise), or `None` when it
/// says both already ([`differs`]), in which case it is written as it was. A mirror that is not wanted is written `false` where the file wrote one and is left out where it wrote none.
///
/// **What the file says is what the reader gives back**: after the call the reader's flips of the element are `flip`, and its turn of the text, beside them, is `txt`. `Err` for an element whose name
/// has no prefix: it holds no prefix to write the attributes with ([`WORDS_NO_PREFIX`]).
pub(crate) fn mirrored(el: &RawElement, flip: (bool, bool), txt: f64) -> Result<Option<RawElement>, &'static str> {
    if !differs(el, flip, txt) {
        return Ok(None);
    }
    let held = mirrors_of(el);
    let text_now = text_turn_of(el);
    // The text reads as `txt` beside the flips it will have: with the file's own number when that gives it, and else with the one that does.
    let text_same = same_turn(text_turn_beside_flips(text_now, flip.0, flip.1), txt);
    let Some((prefix, _)) = el.name.split_once(':') else { return Err(WORDS_NO_PREFIX) };
    let wanted_text = if text_same { None } else { Some(file_text_turn(txt, flip.0, flip.1)) };
    let mut pairs: Vec<(String, String)> = Vec::with_capacity(el.attrs.len().saturating_add(3));
    let (mut wrote_h, mut wrote_v, mut wrote_text) = (false, false, false);
    for (name, value) in el.attrs.iter() {
        let local = name.strip_prefix(prefix).and_then(|rest| rest.strip_prefix(':'));
        match local {
            Some("mirror-horizontal") => {
                wrote_h = true;
                pairs.push((name.to_owned(), value_of(flip.0, value, held.0)));
            }
            Some("mirror-vertical") => {
                wrote_v = true;
                pairs.push((name.to_owned(), value_of(flip.1, value, held.1)));
            }
            Some("text-rotate-angle") if wanted_text.is_some() => {
                wrote_text = true;
                pairs.push((name.to_owned(), degrees(wanted_text.unwrap_or(0.0))));
            }
            _ => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    for (done, on, local) in [(wrote_h, flip.0, "mirror-horizontal"), (wrote_v, flip.1, "mirror-vertical")] {
        if !done && on {
            pairs.push((format!("{prefix}:{local}"), "true".to_owned()));
        }
    }
    if let (false, Some(turn)) = (wrote_text, wanted_text) {
        pairs.push((format!("{prefix}:text-rotate-angle"), degrees(turn)));
    }
    Ok(Some(RawElement { name: el.name.clone(), attrs: RawAttrs::from_pairs(pairs), children: el.children.clone() }))
}

/// The value of a mirror attribute the element wrote: `true` or `false` to say `want`, the attribute's own text when it already says it.
fn value_of(want: bool, written: &str, says: bool) -> String {
    if want == says && boolean(written).is_some() { written.to_owned() } else { want.to_string() }
}

/// A turn as the file writes it: a plain number of degrees.
fn degrees(turn: f64) -> String {
    format!("{turn}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(attrs: &[(&str, &str)]) -> RawElement {
        RawElement { name: "draw:enhanced-geometry".into(), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() }
    }

    fn says(el: &RawElement) -> ((bool, bool), f64) {
        let text = el.attrs.get("draw:text-rotate-angle").and_then(angle_degrees).unwrap_or(0.0);
        (mirrors_of(el), text_turn_beside_flips(text, mirrors_of(el).0, mirrors_of(el).1))
    }

    /// An element that says the shape's mirrors and text turn already is written as it was.
    #[test]
    fn an_element_that_says_the_flips_already_is_left_alone() {
        let el = element(&[("svg:viewBox", "0 0 1 1"), ("draw:mirror-horizontal", "true")]);
        assert_eq!(mirrored(&el, (true, false), 0.0), Ok(None));
        assert_eq!(mirrored(&element(&[]), (false, false), 0.0), Ok(None));
    }

    /// Each flip is written on the element, and the reader's reading of the result is the flip and the text turn asked for, whatever the element said before.
    #[test]
    fn the_mirrors_and_the_text_turn_written_are_the_ones_the_reader_reads_back() {
        let starts = [
            element(&[]),
            element(&[("draw:mirror-horizontal", "true")]),
            element(&[("draw:mirror-vertical", "true"), ("draw:text-rotate-angle", "30")]),
            element(&[("draw:mirror-horizontal", "false"), ("draw:mirror-vertical", "true"), ("draw:text-rotate-angle", "-90")]),
            element(&[("draw:mirror-horizontal", "yes"), ("draw:text-rotate-angle", "180")]),
        ];
        for el in &starts {
            for fx in [false, true] {
                for fy in [false, true] {
                    for txt in [0.0, 30.0, -90.0, 180.0, 135.0] {
                        let made = mirrored(el, (fx, fy), txt).unwrap_or_else(|e| panic!("{e}"));
                        let made = made.as_ref().unwrap_or(el);
                        let (flips, text) = says(made);
                        assert_eq!(flips, (fx, fy), "{el:?} {fx} {fy} {txt}");
                        assert!(same_turn(text, txt), "{el:?} flips {fx} {fy}: the text reads {text}, wanted {txt}");
                    }
                }
            }
        }
    }

    /// The attributes the element holds besides its mirrors and text turn, and its children, are written as they were; a mirror it did not hold is added and one it held is changed in place.
    #[test]
    fn the_rest_of_the_element_is_kept_and_a_held_mirror_is_changed_in_place() {
        let el = element(&[("svg:viewBox", "0 0 21600 21600"), ("draw:mirror-horizontal", "true"), ("draw:type", "non-primitive")]);
        let made = mirrored(&el, (false, true), 0.0).unwrap().unwrap();
        let names: Vec<(&str, &str)> = made.attrs.iter().collect();
        assert_eq!(
            names,
            [
                ("svg:viewBox", "0 0 21600 21600"),
                ("draw:mirror-horizontal", "false"),
                ("draw:type", "non-primitive"),
                ("draw:mirror-vertical", "true"),
                ("draw:text-rotate-angle", "180"),
            ]
        );
    }

    /// An element with no prefix cannot be given mirrors, and says so; one that needs none is not touched.
    #[test]
    fn an_element_with_no_prefix_is_refused_only_when_it_needs_a_mirror() {
        let bare = RawElement { name: "enhanced-geometry".into(), attrs: RawAttrs::new(), children: Vec::new() };
        assert_eq!(mirrored(&bare, (false, false), 0.0), Ok(None));
        assert_eq!(mirrored(&bare, (true, false), 0.0), Err(WORDS_NO_PREFIX));
    }

    /// The text turn written is the inverse of the reader's.
    #[test]
    fn the_file_text_turn_is_the_inverse_of_the_readers() {
        for fx in [false, true] {
            for fy in [false, true] {
                for txt in [-179.0, -90.0, 0.0, 30.0, 90.0, 179.0, 180.0] {
                    let file = file_text_turn(txt, fx, fy);
                    assert!(same_turn(text_turn_beside_flips(file, fx, fy), txt), "{fx} {fy} {txt}: {file}");
                    assert!(file > -180.0 && file <= 180.0, "{file}");
                }
            }
        }
    }
}
