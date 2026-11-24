//! Kani proof harnesses for the engine's integer code of the `.diacraft` open and save: the limits of a read (`nativefile::open::limits_with`, GATE.e142.F1), whether a
//! pass that fingerprints the model fits beside what the process holds (`nativefile::stamp_fits`) and the decision a model writer takes from two stamps
//! (`nativefile::content_unchanged`); and the snap strengths (the text of a strength, `Strength::put` and `get`, and the pull of a strength
//! on the screen as the snapping library is given it); and a resize of several shapes (the factor of a stretch and the carry of a coordinate); and, since
//! The path of a save (`cmd::file::save_path`), what a window shows and the change a setting writes (`view::shown_by`, `view::change_from`),
//! the zoom a snap takes and the reach of an alignment (`snap::takes_zoom`, `snap::alignment_reach`: the finding, real, and refused now), and the peak of a
//! VSDX save (`cmd::file::save_peak`); and, the window made for a drawing with no page (`view::every_flag` and `set`'s fallback), the
//! OpenDocument type of a name (`cmd::file::odg_variant_of_extension`), a connector's reroute setting (`draw::Reroute::from_code`), the axis a standard
//! glue point leaves by, its number, and the better of two routes (`odg_glue::axes_of`, `id_of`, `better`), the classes of a save across and the type of a picture it
//! holds (`across::classes::Classes`, `across::hold::picture_type`), the knots of a Freeform curve (`pen::freeform::knot`), a search's budget and a
//! field a match touches (`find::Work::charge`, `replace::touches_field`), a run's case, a paragraph's direction and a step of indent (`formatting::Case::from_cells`,
//! `Direction::from_cell`, `text::indent_to`), and a box inside a box (`select::box_inside`). Three laws failed on the old code and are fixed (gaps F37):
//! `axes_of`, `Classes::add` and `has`, and `indent_to`. And, a followed link's percent sequences (`draw::links::percent_byte`, `percent_decode_bytes`, the work
//! of `percent_decode`, and `hides`, what `hides_something` says), the readers of a picture's header (`draw::pictures::be16`, `be32`, `le16`, `le32`, `le_i32`,
//! `jpeg_size`, `tiff_size`, `Format::drawn`), the fields' budget of a call (`draw::fields::Tally::charge`), a glued end's row of a connection point (`draw::change::glue_index`),
//! a free callout's tip (`stencil::CalloutStyle::default_tip`), the shape a glued end landed on (`snap::Glue::shape`), a container's box,
//! settings and a call's code (`draw::container::Rect`, `Resize`, `Heading`, `draw::ties::whole_code`), and the save checker's classes, tally, meter and closeness
//! (`check::Class::bit`, `Tally`, `Meter`, `close`). One law failed on the old code and is fixed (gaps F38): `Rect::centre`. And, a field's
//! type, whole number, calendar and order (`draw::shapedata::Kind::from_code`, `type_number`, `whole`, `calendar_name`, `listed_order`), a
//! transparency's share, a gradient's angle and direction and a Quick Style (`formatting::share`, `radians_within_turn`, `GradientKind::of_dir`, `origin_of`, `dir_at`,
//! `QuickStyle::at`, `variant`), and a layout's style code and a move off a page break (`draw::relayout::place_code`, `style_of_code`, `off_breaks`),
//! a paragraph's alignment (`formatting::Align::from_cell`, `cell`), and the room of the browser's store of files (`memfs::admitted`, taken out of
//! `admit`, and `bytes_after_put`, taken out of `put`). Two laws failed on the old code and are fixed (gaps F39):
//! `radians_within_turn` and `QuickStyle::at` and `variant`.
//!
//! **Not proved:** `container::cycles::closing` (its walk taken out over marks kept in an array, the ties among three shapes ran past 300 s with
//! CaDiCaL and with Kissat, and CBMC out of 20 GB walked from every place: a loop of the stack inside a loop of the roots, the trap of `put_in_order`; the walk is as it was);
//! `relayout::spacing::spread` (two intervals ran past 300 s: the sort of the intervals beside four floats); `effects::rounded` (through `powi`, which Kani over-approximates,
//! and the NaN check would fail at `0 * inf` of a power it makes up); `shapedata::is_language_tag` (`split` at a hyphen, the memchr trap) and `is_unset_date` (a decimal read
//! from a text); `container::fit::Fitted::passed` (a hash set) and `connectors::Held` (hash maps), the futex trap; `fields::reads_the_page` (a resolver and a formula's text).
//!
//! **Not proved:** `percent_decode` and `hides_something` of a text (a text made by `str::from_utf8` has a length the solver does not know, and the decoder's
//! `Vec::with_capacity` of it ran CBMC out of 8 GB for three bytes: the decoder is proved on bytes and the test of hidden characters on a text, apart); `percent_encode` (its
//! `format!` of a byte in hexadecimal ran past 300 s for one byte); `jpeg_size` of any bytes, and of a segment of any code (every 11 bytes, and a symbolic code with the rest
//! fixed, ran past 300 s: the walk of the segments; a baseline frame header of any size is proved); `fields::with_part` (a `TextPart` holds a `RawNode`: the drop trap); the
//! `fields::Tally`'s walk of a shape's text (it takes a `Shape`); `callouts::outermost_paths` and `outermost_of` (the sort of a list of paths: two paths of one or two places ran
//! CBMC out of 8 GB); `ties::Ties` (the `retain_sheets`, `add`, `remove`, the `drop_emptied`: it holds an index and a set of hash sets, and
//! `HashMap::new()` alone reaches the futex `syscall`); `Ties::from_expr`'s cap (a formula's tree: the drop trap); `wf`, `hf` (`format!`); Change Shape's depth rule, its test of a
//! frame cell that differs and the shift of `recentre` (each one expression inline in code that reads a document; `number_of` gives them finite numbers), `put_cell` and
//! `merged_sheet` (maps of cells); `layers::glue_barred` and `standing_of` (a hash map of layers and a document); `check::cells::canonical`, `is_default` and `number_of`
//! (strings through `trim`, `to_uppercase` and Unicode's blanks), and `close`'s symmetry (`close(a, b) == close(b, a)`: the two subtractions are a second circuit, past 300 s; it
//! holds by IEEE's subtraction, `a - b == -(b - a)`); `cmd::file::save_path` again (a later change left it as an earlier harness proves it).
//!
//! **Not proved:** `literal_of(quoted(t))` (a text of one character ran CBMC out of 20 GB alone, and of three past 300 s, through `trim` and the pushes of a
//! `String`); `Classes::words` (21 pushes of a `String` beside symbolic bits, past 300 s); `classes::plain`, `Reroute::from_word` and the other words of the commands (strings);
//! `Target::of` (through `kind_of_extension`, below) and `OpenFile::extension` (an open file is made from a path and bytes); the copy room of `odgfile::save::save_model`, the split
//! of `settle::route_after_within`'s tries, the budgets of `Crossings`, `GluedPoints::of`, `jumps::route_read` and the other counters of (each a comparison and a
//! subtraction inline in code that reads a document or keeps a map; the cost tests count them); `hold::resolve`, `hold::is_object_part`, `signatures_of` (paths, a `Part`, a document);
//! `charge_bytes` and `select::charge` (their refusals are made with `format!`; the rule is `Work::charge`'s); `bound_of` and `Walker::to` (a place's text, and `char_indices` of a
//! symbolic text); `label::upright` (`rem_euclid`, Kani's float remainder); `pieces_needed`, `step_end`, `square_corner` and `pencil::bow_of` (six to eight floats through `hypot`,
//! a division and `sqrt`); `memberships` (it reads a document).
//!
//! **Not proved:** the charge `changed_by_edit` makes and the dispatch of `commit_changed_with` (calc: an `EvalCtx` is made with hash maps and a
//! `Clock`, and what the standard library does then reaches the foreign function `syscall`, which Kani 0.68 does not model; the dispatch needs a `Document`); `cmd::window::label`
//! (words) and `VsdxHeld::holds_windows_alone` (a holder of lists).
//!
//! **Not proved here: the kind a file's extension names** (`nativefile::kind_of_extension`). A text of symbolic characters made through `str::from_utf8` did not finish in 240 s,
//! and one pushed a character at a time into a `String` ran out of 20 GB for an extension of eight; the unit tests of `nativefile` hold it. **Nor the pages of **
//! (`draw::pages::next_id`, `background_place`, `set_page_type`): they read a `Document`, and one `diacraft_doc::Page::new` and its drop, alone, ran past 180 s (three pages
//! and a change of type past 300 s): the drop of a page's shape list, whose shapes hold shape lists, is unrolled at every level. Their unit tests and those of
//! `cmd/page` hold them.

use diacraft_doc::stamp::Stamp;

use crate::nativefile::open::{READ_BUDGET, limits_with};
use crate::nativefile::{content_unchanged, stamp_fits};

/// `limits_with` (GATE.e142.F1): for every reservation, the memory of the read is what was reserved, capped at [`READ_BUDGET`], so never more than the
/// process's budget (`diacraft_vsdx::MAX_MEMORY`) or the package's own (`diacraft_native::MAX_MEMORY`); every other limit is the package's default; and it
/// does not panic. Two-sided: a reservation under the budget is given exactly, so a `limits_with` that always gave the budget fails.
#[kani::proof]
fn a_read_is_held_to_what_it_reserved_and_never_more_than_either_budget() {
    let reserved: u64 = kani::any();
    let limits = limits_with(reserved);
    let defaults = diacraft_native::Limits::default();
    assert!(limits.max_memory <= diacraft_native::MAX_MEMORY && limits.max_memory <= diacraft_vsdx::MAX_MEMORY);
    assert!(limits.max_memory <= reserved);
    if reserved <= READ_BUDGET {
        assert!(limits.max_memory == reserved);
    } else {
        assert!(limits.max_memory == READ_BUDGET);
    }
    assert!(limits.max_file_bytes == defaults.max_file_bytes);
    assert!(limits.container == defaults.container);
    kani::cover!(reserved < READ_BUDGET, "a reservation under the budget is given whole");
    kani::cover!(reserved == READ_BUDGET, "a reservation of the whole budget");
    kani::cover!(reserved > READ_BUDGET, "a reservation past the budget is held to it");
}

/// `stamp_fits`: a pass that fingerprints the model fits beside what the process holds, or it does not, **by one figure**: there is no hold at which it fits
/// beside more and not beside less, it fits beside nothing, never beside all a `u64` can say (the sum saturates and does not wrap), and it does not panic.
#[kani::proof]
fn a_fingerprint_that_fits_beside_a_hold_fits_beside_any_smaller_one() {
    let (held, less): (u64, u64) = (kani::any(), kani::any());
    kani::assume(less <= held);
    if stamp_fits(held) {
        assert!(stamp_fits(less));
    }
    assert!(stamp_fits(0) && !stamp_fits(u64::MAX));
    kani::cover!(stamp_fits(held) & !stamp_fits(held.saturating_add(1)), "the last hold a pass fits beside");
}

/// `content_unchanged` (the decision a model writer takes, 4.4): `Some(true)` exactly when the drawing has the stamp it was read with and its content now has the
/// same, `Some(false)` exactly when both are there and differ, and `None` when either is missing. It reads the two stamps and nothing else.
#[kani::proof]
fn the_content_is_unchanged_exactly_when_both_stamps_are_there_and_equal() {
    let (read, now): (u64, u64) = (kani::any(), kani::any());
    let (has_read, has_now): (bool, bool) = (kani::any(), kani::any());
    let stored = has_read.then_some(Stamp(read));
    let current = has_now.then_some(Stamp(now));
    match content_unchanged(stored, current) {
        Some(same) => {
            assert!(has_read && has_now && same == (read == now));
            kani::cover!(!same, "an edited drawing");
        }
        None => {
            assert!(!has_read || !has_now);
            kani::cover!(has_now, "a drawing that never came from a foreign file");
        }
    }
}

// --- Snap strengths (`snap/settings.rs`, `snap/adapter.rs`) ---

/// The characters a strength's text is drawn from: digits (the two ends and one between), the characters next to them in ASCII (`/` and `:`), a sign, a blank and a letter.
/// What `strength_pixels` makes of a text depends only on which are digits and their values, so a text of these in each place stands for every text of that length but the
/// texts with a character that is not ASCII, which (more than one byte each, and no digit) are refused as the letter is.
const STRENGTH_KINDS: &[u8; 8] = b"059/:+ a";

/// `strength_pixels` of every text of `N` characters by its kinds: `Some(n)` exactly for one to three decimal digits, `n` their value (so at most 999); every
/// other text, the empty one and one of four characters included, is `None`.
fn strength_text_law<const N: usize>() -> Option<u16> {
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < STRENGTH_KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| STRENGTH_KINDS[usize::from(k)]);
    let text = std::str::from_utf8(&raw).unwrap_or("x");
    let digits = raw.iter().all(u8::is_ascii_digit);
    let value = raw.iter().fold(0u16, |n, b| n.wrapping_mul(10).wrapping_add(u16::from(b.wrapping_sub(b'0'))));
    let got = crate::snap::strength_pixels(text);
    if (1..=3).contains(&N) && digits {
        assert!(got == Some(value) && value <= crate::snap::STRENGTH_MAX);
    } else {
        assert!(got.is_none());
    }
    got
}

macro_rules! strength_text_law {
    ($($name:ident: $n:literal, $numbers:tt),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind(7)]
            fn $name() {
                let got = strength_text_law::<$n>();
                kani::cover!(got.is_none(), "a text that is no number");
                strength_text_law!(@number $numbers, got);
            }
        )*
    };
    // A length that can be a number covers one too; the empty text and one of four characters cannot.
    (@number true, $got:ident) => {
        kani::cover!($got.is_some(), "a number");
    };
    (@number false, $got:ident) => {};
}

strength_text_law! {
    a_strength_of_no_characters_is_refused: 0, false,
    a_strength_of_one_character_is_a_digit_or_refused: 1, true,
    a_strength_of_two_characters_is_two_digits_or_refused: 2, true,
    a_strength_of_three_characters_is_three_digits_or_refused: 3, true,
    a_strength_of_four_characters_is_refused: 4, false,
}

/// `Strength::put` and `get`: `put` takes a strength exactly when the index is one of the six and the pixels at most 999; then `get` of that index is the pixels
/// and the other five are as they were; a refusal changes nothing; and `get` of an index past the six is 0.
#[kani::proof]
fn a_strength_is_put_exactly_when_its_index_and_pixels_are_in_range() {
    use crate::snap::{STRENGTH_MAX, Strength};
    let before =
        Strength { ruler: kani::any(), grid: kani::any(), guides: kani::any(), points: kani::any(), geometry: kani::any(), extensions: kani::any() };
    let (index, pixels): (usize, u16) = (kani::any(), kani::any());
    let mut after = before;
    let taken = after.put(index, pixels);
    assert!(taken == (index < 6 && pixels <= STRENGTH_MAX));
    if taken {
        assert!(after.get(index) == pixels);
        let other: usize = kani::any();
        kani::assume(other < 6 && other != index);
        assert!(after.get(other) == before.get(other));
    } else {
        assert!(after == before);
    }
    let past: usize = kani::any();
    kani::assume(past >= 6);
    assert!(before.get(past) == 0);
    kani::cover!(taken & (pixels == STRENGTH_MAX), "the strongest pull");
    kani::cover!(!taken & (index < 6), "too strong a pull");
}

/// The pull of a strength on the screen, as the engine gives it to the snapping library (`Reach::Screen(f64::from(px))`, `craft_snap::Reach`): for every
/// strength, distance and zoom, a target never pulls for a distance or a zoom that is no number, a strength of 0 pulls a target on the line, and the reach in page units at a zoom
/// the engine takes (finite and above 0) is never below 0 and never a NaN, an infinity at a zoom so small that the pixels cover more than the largest number.
///
/// **Not proved: that a target pulls exactly when `d * zoom` is within the pixels, and that a stronger pull reaches whatever a weaker one does.** Either asks the solver to show two
/// products of the same two floats equal (the library's and the harness's, or two of the library's): a harness of each ran past 150 s (the second circuit,
/// `docs/development.md`, Proofs), where this one takes a second. The library's own tests hold the boundary.
#[kani::proof]
fn a_target_pulls_only_at_a_distance_and_zoom_that_are_numbers() {
    let px: u16 = kani::any();
    let (d, zoom): (f64, f64) = (kani::any(), kani::any());
    // An infinity times a zero is a NaN, which the library compares as no pull; Kani's NaN check stops at the product, so the two are left out.
    kani::assume(!(d.is_infinite() && zoom == 0.0) && !(d == 0.0 && zoom.is_infinite()));
    let reach = craft_snap::Reach::Screen(f64::from(px));
    let pulls = reach.within(d, zoom);
    if pulls {
        assert!(!d.is_nan() && !zoom.is_nan());
    }
    if zoom > 0.0 && zoom.is_finite() {
        let page = reach.document(zoom);
        assert!(page >= 0.0);
        kani::cover!(page == f64::INFINITY, "a zoom so small that the pull covers the page");
    }
    kani::cover!(pulls & (px == 0), "a strength of 0 pulls a shape on the target");
    kani::cover!(!pulls & (d > 0.0) & (zoom > 0.0), "a target out of reach");
}

// --- A resize of several shapes (`draw/scale.rs`) ---

/// `scale::factor`, for **every** two lengths: a factor exactly when both are numbers and the old one is no shorter than a billionth (in size), and then never a
/// NaN, zero for a new length of zero (and for one so small beside the old that the quotient rounds to 0: the first form of this law, zero **exactly** for a zero length, failed
/// there, on a quotient past the least number), and of the sign of the two lengths' signs together (the old box's sides may come in either order); it is an
/// infinity only for a stretch past the largest number, which the callers' `checked` refuses.
#[kani::proof]
fn a_stretch_has_a_factor_exactly_when_its_lengths_are_numbers_and_the_old_one_has_length() {
    let (old, new): (f64, f64) = (kani::any(), kani::any());
    let got = crate::draw::scale_factor(old, new);
    let stretches = old.is_finite() && new.is_finite() && old.abs() >= 1.0e-9;
    assert!(got.is_some() == stretches);
    if let Some(f) = got {
        assert!(!f.is_nan());
        if new == 0.0 {
            assert!(f == 0.0);
        }
        assert!(f.is_sign_negative() == (new.is_sign_negative() != old.is_sign_negative()));
        kani::cover!(f.is_infinite(), "a stretch past the largest number");
        kani::cover!(f.is_finite() & (f != 0.0), "an ordinary stretch");
        kani::cover!((f == 0.0) & (new != 0.0), "a new length so small beside the old one that the factor rounds to 0");
    }
    kani::cover!(!stretches & old.is_finite() & new.is_finite(), "an old length too short to stretch");
}

/// `scale::carry`, for **every** finite old low edge, new low edge and scale: the old low edge goes to the new one exactly, so the joint box's fixed side does not
/// move by a rounding; and a coordinate that is no number stays none (a NaN is carried as one, never made a number).
#[kani::proof]
fn the_old_low_edge_is_carried_to_the_new_one_exactly() {
    let (old_low, new_low, scale): (f64, f64, f64) = (kani::any(), kani::any(), kani::any());
    kani::assume(old_low.is_finite() && new_low.is_finite() && scale.is_finite());
    assert!(crate::draw::scale_carry(old_low, old_low, new_low, scale) == new_low);
    assert!(crate::draw::scale_carry(f64::NAN, old_low, new_low, scale).is_nan());
}

// ---: the path of a save, the window settings, the zoom of a snap (the finding) ---

/// A working file of a drawing from a number below 4: none (a new drawing), `.diacraft`, VSDX, `.odg`.
fn working_of(n: u8) -> Option<crate::file::FileFormat> {
    use crate::file::FileFormat;
    match n {
        0 => None,
        1 => Some(FileFormat::Diacraft),
        2 => Some(FileFormat::Vsdx),
        _ => Some(FileFormat::Odg),
    }
}

/// A kind of `.diacraft` file from a number below 3.
fn kind_of(n: u8) -> diacraft_native::Kind {
    match n {
        0 => diacraft_native::Kind::Drawing,
        1 => diacraft_native::Kind::Stencil,
        _ => diacraft_native::Kind::Template,
    }
}

/// One of the six VSDX types from a number below 3 and whether it allows macros.
fn variant_of(n: u8, macro_enabled: bool) -> diacraft_vsdx::Variant {
    let kind = match n {
        0 => diacraft_vsdx::Kind::Drawing,
        1 => diacraft_vsdx::Kind::Stencil,
        _ => diacraft_vsdx::Kind::Template,
    };
    diacraft_vsdx::Variant { kind, macro_enabled }
}

/// `save_path` (the design's 4.1, and the `.odg` model writer) over its **whole** product space: every working file (none, `.diacraft`, VSDX, `.odg`), stencil
/// or not, and every target (each of the three `.diacraft` kinds, each of the six VSDX types, `.odg` and `.otg`, a name of no type). The laws of the table of its documentation, each
/// two-sided: a stencil as an OpenDocument file is refused exactly for such a target and a stencil; a name of no type is refused with the working file it was asked for; `.diacraft` of a
/// kind is that kind whatever the drawing is; a VSDX type splices exactly for a VSDX drawing, into that type, is written from the model exactly for a `.diacraft` drawing and a new one,
/// and is written from the model for an `.odg` drawing too (a save across); an `.odg` or `.otg` is spliced exactly for an `.odg` drawing that is no stencil, written from the model exactly for a
/// `.diacraft` drawing, a new one and a VSDX drawing that are no stencil (a save across), each in the kind the name says; no OpenDocument target gives a VSDX path and no VSDX target an
/// OpenDocument one, and the only refusals are a stencil as an OpenDocument file and a name of no type. It never panics.
#[kani::proof]
fn a_save_takes_the_path_its_working_file_and_its_target_name() {
    use crate::cmd::{SavePath, Target, Why, save_path};
    use crate::file::FileFormat;
    let (w, t, k, v): (u8, u8, u8, u8) = (kani::any(), kani::any(), kani::any(), kani::any());
    kani::assume(w < 4 && t < 4 && k < 3 && v < 3);
    let (stencil, macro_enabled, template): (bool, bool, bool) = (kani::any(), kani::any(), kani::any());
    let working = working_of(w);
    let odg_kind = if template { diacraft_odg::Variant::Template } else { diacraft_odg::Variant::Drawing };
    let target = match t {
        0 => Target::Diacraft(kind_of(k)),
        1 => Target::Vsdx(variant_of(v, macro_enabled)),
        2 => Target::Odg(odg_kind),
        _ => Target::Other,
    };
    let path = save_path(working, stencil, target);
    let odg = matches!(target, Target::Odg(_));
    let vsdx_target = matches!(target, Target::Vsdx(_));
    // A VSDX target is written from the model for a drawing with no VSDX file: a new drawing, a `.diacraft` drawing and an `.odg` drawing (a save across). An OpenDocument
    // target is written from the model for a new drawing, a `.diacraft` drawing and a VSDX drawing.
    let vsdx_from_model = matches!(working, None | Some(FileFormat::Diacraft | FileFormat::Odg));
    let odg_from_model = matches!(working, None | Some(FileFormat::Diacraft | FileFormat::Vsdx));
    assert!((path == SavePath::Refused(Why::StencilToOdg)) == (odg && stencil));
    assert!(matches!(path, SavePath::Refused(Why::NoType(_))) == (target == Target::Other));
    if let SavePath::Refused(Why::NoType(asked)) = path {
        assert!(asked == working);
    }
    assert!(matches!(path, SavePath::Native(_)) == matches!(target, Target::Diacraft(_)));
    if let (SavePath::Native(kind), Target::Diacraft(named)) = (path, target) {
        assert!(kind == named);
    }
    assert!(matches!(path, SavePath::VsdxSplice(_)) == (vsdx_target && working == Some(FileFormat::Vsdx)));
    if let (SavePath::VsdxSplice(into), Target::Vsdx(named)) = (path, target) {
        assert!(into == named);
    }
    assert!((path == SavePath::VsdxModel) == (vsdx_target && vsdx_from_model));
    assert!((path == SavePath::OdgSplice(odg_kind)) == (odg && !stencil && working == Some(FileFormat::Odg)));
    assert!((path == SavePath::OdgModel(odg_kind)) == (odg && !stencil && odg_from_model));
    // No save is refused for the format the drawing came from but a stencil as an OpenDocument file and a name of no type.
    assert!(matches!(path, SavePath::Refused(_)) == ((odg && stencil) || target == Target::Other));
    if odg {
        assert!(!matches!(path, SavePath::VsdxSplice(_) | SavePath::VsdxModel));
    }
    if vsdx_target {
        assert!(!matches!(path, SavePath::OdgSplice(_) | SavePath::OdgModel(_)));
    }
    kani::cover!(matches!(path, SavePath::VsdxSplice(_)) & macro_enabled, "a macro-enabled VSDX type spliced");
    kani::cover!((path == SavePath::VsdxModel) & working.is_none(), "a new drawing written as a VSDX type from its model");
    kani::cover!(
        (path == SavePath::OdgModel(diacraft_odg::Variant::Template)) & working.is_none(),
        "a new drawing written as an .otg from its model"
    );
}

/// The four `Option<bool>` of a set of window flags, drawn whole.
fn any_flags() -> diacraft_doc::WindowFlags {
    diacraft_doc::WindowFlags {
        rulers: kani::any(),
        grid: kani::any(),
        page_breaks: kani::any(),
        guides: kani::any(),
        connection_points: kani::any(),
    }
}

/// `view::shown_by`, the resolution of `view_settings`, and `view::change_from`, the change `view::set` writes, for **every** four flags held, four wanted and the part
/// held or not: a drawing with no windows part shows a new drawing's settings; one with a part shows each flag it holds and `NO_VALUE`'s where it holds none; the change holds a flag
/// exactly where the wanted one is given and differs from what is shown, so it is empty (and `set` writes nothing) exactly when every flag given is shown already; and the change
/// put over the part's flags shows what was wanted where it was given and what was shown elsewhere, which is what `set` promises of the drawing after it.
#[kani::proof]
fn a_window_shows_what_its_part_holds_and_a_change_writes_only_what_differs() {
    use crate::view::{NEW_DRAWING, NO_VALUE, change_from, shown_by};
    let (held, wanted) = (any_flags(), any_flags());
    let has_part: bool = kani::any();
    let shown = shown_by(has_part.then_some(held));
    if has_part {
        let each = |f: Option<bool>, none: bool| match f {
            Some(v) => v,
            None => none,
        };
        assert!(shown.rulers == each(held.rulers, NO_VALUE.rulers) && shown.grid == each(held.grid, NO_VALUE.grid));
        assert!(
            shown.guides == each(held.guides, NO_VALUE.guides) && shown.connection_points == each(held.connection_points, NO_VALUE.connection_points)
        );
    } else {
        assert!(shown == NEW_DRAWING);
    }
    let change = change_from(&wanted, shown);
    let one = |c: Option<bool>, w: Option<bool>, now: bool| match w {
        Some(v) if v != now => c == Some(v),
        _ => c.is_none(),
    };
    assert!(one(change.rulers, wanted.rulers, shown.rulers) && one(change.grid, wanted.grid, shown.grid));
    assert!(one(change.guides, wanted.guides, shown.guides) && one(change.connection_points, wanted.connection_points, shown.connection_points));
    let agrees = |w: Option<bool>, now: bool| w.is_none() || w == Some(now);
    let all_shown = agrees(wanted.rulers, shown.rulers)
        && agrees(wanted.grid, shown.grid)
        && agrees(wanted.guides, shown.guides)
        && agrees(wanted.connection_points, shown.connection_points);
    assert!(change.is_empty() == all_shown);
    if has_part {
        let after = shown_by(Some(change.over(&held)));
        let want = |w: Option<bool>, now: bool| w.unwrap_or(now);
        assert!(after.rulers == want(wanted.rulers, shown.rulers) && after.grid == want(wanted.grid, shown.grid));
        assert!(
            after.guides == want(wanted.guides, shown.guides) && after.connection_points == want(wanted.connection_points, shown.connection_points)
        );
    }
    kani::cover!(!change.is_empty() & !has_part, "a new drawing given a setting that is not a new drawing's");
    kani::cover!(change.is_empty() & !wanted.is_empty(), "settings wanted that are shown already");
}

/// **The snap at a tiny zoom** (the finding; `snap::takes_zoom`, `snap::alignment_reach`): for **every** zoom above 0, a snap takes it exactly when the reach of an
/// alignment at the strongest pulls (999 points for the points, the outlines and the guides) of a subject whose shapes lie in its box is a distance on the page, and a target at
/// `ALIGN_REACH` is then within it. So the commands refuse no zoom at which an alignment works, and take none at which it does not. **The finding was real**: a zoom above 0 at which
/// the reach is an infinity, which `craft_snap::Reach::Document` reads as no reach, so nothing lined up, is reached (the cover); the commands took it until a later change. A weaker pull
/// is no farther (a quotient by the same zoom of a smaller number): not asked here, since it is a second divider (`docs/development.md`, Proofs, the second circuit).
#[kani::proof]
fn a_snap_takes_a_zoom_exactly_when_the_strongest_alignment_reaches_a_distance() {
    use crate::snap::{ALIGN_REACH, STRENGTH_MAX, Strength, alignment_reach, takes_zoom};
    let zoom: f64 = kani::any();
    kani::assume(zoom.is_finite() && zoom > 0.0);
    let strongest = Strength { points: STRENGTH_MAX, geometry: STRENGTH_MAX, guides: STRENGTH_MAX, ..Strength::default() };
    let reach = alignment_reach(&strongest, zoom, 0.0);
    let taken = takes_zoom(zoom);
    assert!(taken == reach.is_finite());
    if taken {
        assert!(craft_snap::Reach::Document(reach).within(ALIGN_REACH, zoom));
    }
    kani::cover!(!taken, "a zoom above 0 at which the reach is no number: the finding");
    kani::cover!(taken & (reach > 1.0e300), "a zoom a snap takes at which the reach is past 1E300 inches");
}

/// `snap::alignment_reach` for **every** zoom above 0, every strength of the points, the outlines and the guides (0 to 999 points each) and every overhang of the moving shapes beyond
/// the subject's box: never a NaN, and never short of `ALIGN_REACH`. **An overhang near the largest number makes the reach an infinity at any zoom** (the cover): a subject whose shapes
/// lie past about 1E308 inches of its box, which only a hostile file has; it is a concern of the report, not a law here.
#[kani::proof]
fn an_alignment_reaches_at_least_its_own_reach_and_is_never_a_nan() {
    use crate::snap::{ALIGN_REACH, STRENGTH_MAX, Strength, alignment_reach, takes_zoom};
    let zoom: f64 = kani::any();
    kani::assume(zoom.is_finite() && zoom > 0.0);
    let (points, geometry, guides): (u16, u16, u16) = (kani::any(), kani::any(), kani::any());
    kani::assume(points <= STRENGTH_MAX && geometry <= STRENGTH_MAX && guides <= STRENGTH_MAX);
    let overhang: f64 = kani::any();
    kani::assume(overhang.is_finite() && overhang >= 0.0);
    let reach = alignment_reach(&Strength { points, geometry, guides, ..Strength::default() }, zoom, overhang);
    assert!(!reach.is_nan() && reach >= ALIGN_REACH);
    kani::cover!((zoom == crate::snap::DEFAULT_ZOOM) & (reach == ALIGN_REACH), "the alignment's own reach at 100%");
    kani::cover!(reach.is_infinite() & (overhang < f64::MAX) & takes_zoom(zoom), "an overhang near the largest number at a zoom a snap takes");
}

/// `save_peak`, the arithmetic of `check_save_peak`, for **every** hold of the session and copy: the peak is eight sevenths of the two together
/// (rounded down to a multiple of eight sevenths' steps) and 350 MiB, or the largest number where that passes it, so it is never less than the two; a save is given what the peak
/// leaves beside the hold, never more than the peak, exactly when the peak is within the address space less its sixty-fourth, and is refused with the figures the words give
/// otherwise. The quotient by 7 is stated by products (`7 (p - 350 MiB) <= 8 n < 7 (p - 350 MiB) + 56`), so the harness has no divider of its own.
#[kani::proof]
fn a_save_is_given_what_its_peak_leaves_beside_the_hold_or_is_refused_past_the_address_space() {
    use crate::cmd::save_peak;
    let (held, copy): (u64, u64) = (kani::any(), kani::any());
    let need = (u128::from(held) + u128::from(copy)).min(u128::from(u64::MAX));
    let space = u128::from(diacraft_sheetxml::model::ADDRESS_SPACE_BYTES);
    let most = space - space / 64;
    let fixed: u128 = 350 << 20;
    let peak_law = |p: u128| p == u128::from(u64::MAX) || (p >= fixed && 7 * (p - fixed) <= 8 * need && 8 * need < 7 * (p - fixed) + 56);
    match save_peak(held, copy) {
        Ok(beside) => {
            let peak = u128::from(beside) + u128::from(held);
            assert!(peak <= most && peak_law(peak) && peak >= need);
            kani::cover!(peak == most, "a save at the edge of what a save may hold");
        }
        Err((n, p, m)) => {
            assert!(u128::from(n) == need && u128::from(m) == most && u128::from(p) > most && peak_law(u128::from(p)) && u128::from(p) >= need);
            kani::cover!(p == u64::MAX, "a peak past the largest number");
        }
    }
}

// ---: the window of a drawing with no page, the OpenDocument type of a name, the reroute of a connector, the
// glue of a standard connector's ends, the classes a save across names and the type of a picture it holds, the knots of a Freeform,
// a search's work, a field a match touches and a quoted string, the case, the direction and the indent of a paragraph, and a box inside a box ---

/// `view::every_flag` and the fallback of `view::set` for a drawing with no page (`change.over(&every_flag(shown))`), for **every** four settings shown and four flags
/// wanted: every flag of what is shown is given, the window shows what is shown, and the window made holds every flag, each the wanted one where it was given and differs and what is
/// shown elsewhere, so the drawing shows what was wanted where it was given and what it showed before everywhere else.
#[kani::proof]
fn a_window_made_for_a_drawing_with_no_page_holds_every_flag_as_shown_and_the_wanted_ones_over_them() {
    use crate::view::{ViewSettings, change_from, every_flag, shown_by};
    let shown = ViewSettings { rulers: kani::any(), grid: kani::any(), guides: kani::any(), connection_points: kani::any() };
    let all = every_flag(shown);
    assert!(all.rulers == Some(shown.rulers) && all.grid == Some(shown.grid));
    assert!(all.guides == Some(shown.guides) && all.connection_points == Some(shown.connection_points));
    assert!(shown_by(Some(all)) == shown);
    let wanted = any_flags();
    let made = change_from(&wanted, shown).over(&all);
    let each = |m: Option<bool>, w: Option<bool>, now: bool| m == Some(w.unwrap_or(now));
    assert!(each(made.rulers, wanted.rulers, shown.rulers) && each(made.grid, wanted.grid, shown.grid));
    assert!(each(made.guides, wanted.guides, shown.guides) && each(made.connection_points, wanted.connection_points, shown.connection_points));
    let after = shown_by(Some(made));
    assert!(after.rulers == wanted.rulers.unwrap_or(shown.rulers) && after.grid == wanted.grid.unwrap_or(shown.grid));
    assert!(
        after.guides == wanted.guides.unwrap_or(shown.guides)
            && after.connection_points == wanted.connection_points.unwrap_or(shown.connection_points)
    );
    kani::cover!(after != shown, "a setting that changes what the drawing shows");
}

/// `cmd::file::odg_variant_of_extension` for **every** extension of `N` bytes that is text: a drawing exactly for `odg` and a template exactly for `otg`, each in any
/// ASCII case, and nothing for every other text (`odg ` and `.odg` among those of four bytes). → what it gave.
fn odg_kind_law<const N: usize>() -> Option<diacraft_odg::Variant> {
    use crate::cmd::odg_variant_of_extension;
    let raw: [u8; N] = kani::any();
    let Ok(text) = std::str::from_utf8(&raw) else { return None };
    let got = odg_variant_of_extension(text);
    let either = |b: u8, c: u8| b == c || b == c.to_ascii_uppercase();
    let named = |word: &[u8; 3]| N == 3 && either(raw[0], word[0]) && either(raw[1], word[1]) && either(raw[2], word[2]);
    assert!((got == Some(diacraft_odg::Variant::Drawing)) == named(b"odg"));
    assert!((got == Some(diacraft_odg::Variant::Template)) == named(b"otg"));
    got
}

#[kani::proof]
#[kani::unwind(5)]
fn an_extension_of_three_bytes_names_an_opendocument_drawing_or_template_exactly_for_odg_and_otg_in_any_case() {
    let got = odg_kind_law::<3>();
    kani::cover!(got == Some(diacraft_odg::Variant::Template), "a template");
    kani::cover!(got.is_none(), "another extension");
}

#[kani::proof]
#[kani::unwind(6)]
fn an_extension_of_four_bytes_names_no_opendocument_type() {
    assert!(odg_kind_law::<4>().is_none());
}

/// `draw::Reroute::from_code` for **every** `ConFixedCode`, a number or not: 1, 2 and 3 are their settings for a number nearer to them than a half
/// (from just above 0.5 to just below 1.5, and so on), and every other number (0, the 4, 5 and 6 files hold for whole-shape glue, the halves themselves, an infinity, a number that is
/// not one) reads as 0, Freely. Each setting is read back from its own code.
#[kani::proof]
fn a_connectors_reroute_setting_is_its_code_to_the_nearest_whole_number_and_freely_for_any_other() {
    use crate::draw::Reroute;
    let code: f64 = kani::any();
    let got = Reroute::from_code(code);
    let near = |k: f64| code > k - 0.5 && code < k + 0.5;
    let want = if near(1.0) {
        Reroute::AsNeeded
    } else if near(2.0) {
        Reroute::Never
    } else if near(3.0) {
        Reroute::OnCrossover
    } else {
        Reroute::Freely
    };
    assert!(got == want);
    for r in Reroute::ALL {
        assert!(Reroute::from_code(f64::from(r.code())) == r);
    }
    kani::cover!((got == Reroute::Never) & (code != 2.0), "a code near 2");
    kani::cover!((got == Reroute::Freely) & (code == 1.5), "a half reads as freely");
}

/// The tolerance of `odg_glue` (`EPS`).
const GLUE_EPS: f64 = 1.0e-9;

/// `draw::odg_glue::axes_of` and `id_of` (the page axis a standard glue point leaves its shape by, and LibreOffice's number of it) for **every** direction: none for a
/// direction that is not finite or of no length; else first a page axis, of one unit along `x` or along `y`, **along which the direction goes** (its part there has the axis's sign
/// and is not 0), along the larger part when they differ, and the other axis too only when the two parts are the same within the tolerance (at most three billionths of the larger, or
/// of an inch for a short one); and the number of each axis is its side: 0 up, 1 right, 2 down, 3 left, one number for each axis. Every direction has a number of the four.
/// **The law failed on the old code**: a direction whose two parts add up past the largest number (1.56E308 and 2.39E307) had an infinite tolerance and left by both.
#[kani::proof]
fn a_glue_point_leaves_by_the_page_axis_its_direction_goes_along_and_both_only_on_a_diagonal() {
    use crate::draw::{axes_of, id_of};
    let v: (f64, f64) = (kani::any(), kani::any());
    let got = axes_of(v);
    let (ax, ay) = (v.0.abs(), v.1.abs());
    let unit = |a: (f64, f64)| (a.0.abs() == 1.0 && a.1 == 0.0) || (a.0 == 0.0 && a.1.abs() == 1.0);
    let goes = |a: (f64, f64)| a.0 * v.0 > 0.0 || a.1 * v.1 > 0.0;
    let side = |a: (f64, f64)| {
        if a.1 == 1.0 {
            0
        } else if a.0 == 1.0 {
            1
        } else if a.1 == -1.0 {
            2
        } else {
            3
        }
    };
    match got {
        [None, None] => {
            assert!(!(v.0.is_finite() && v.1.is_finite()) || ax + ay <= GLUE_EPS);
        }
        [Some(first), second] => {
            assert!(v.0.is_finite() && v.1.is_finite() && unit(first) && goes(first) && id_of(first) == side(first));
            match second {
                None => {
                    assert!(if first.0 != 0.0 { ax > ay } else { ay > ax });
                }
                Some(other) => {
                    let (hi, lo) = (ax.max(ay), ax.min(ay));
                    assert!(first.1 == 0.0 && other.0 == 0.0 && unit(other) && goes(other) && id_of(other) == side(other));
                    assert!(hi - lo <= 3.0 * GLUE_EPS * hi.max(1.0));
                }
            }
        }
        [None, Some(_)] => {
            assert!(false, "a second axis with no first");
        }
    }
    assert!(id_of(v) <= 3);
    // The shapes of the answer as booleans first: a `matches!` inside a cover is a cover for each way through the match, and the way that does not match never satisfies it.
    let (both, one) = (matches!(got, [Some(_), Some(_)]), matches!(got, [Some(_), None]));
    kani::cover!(both & (ax != ay), "a diagonal within the tolerance");
    kani::cover!(one & (ax > 1.0e300), "a long direction");
}

/// `draw::odg_glue::better` (which of two routes of a standard connector LibreOffice takes) for **every** two routes, each across too narrow a gap or not, near a shape or
/// not, and of any finite cost: one that is not across a narrow gap first, then one that keeps away from the shapes, then the cheaper by more than the tolerance; **never each better than the
/// other**, a route is never better or worse than itself, and when the order of the two is told both ways it is told the same way. (It is not told both ways always: the
/// tolerance is the first route's cost's, so in a window of about the tolerance squared one route is cheaper one way and as good the other, the second cover.)
#[kani::proof]
fn of_two_routes_the_one_across_no_narrow_gap_then_away_from_shapes_then_cheaper_is_better_and_never_both() {
    use crate::draw::better_of;
    let (x, y): ((bool, bool, f64), (bool, bool, f64)) = (kani::any(), kani::any());
    // A route's cost is its length and its penalties, of finite points: an infinite cost would be a NaN under the code's difference, which Kani fails at.
    kani::assume(x.2.is_finite() && y.2.is_finite());
    let xy = better_of(x, y);
    let yx = better_of(y, x);
    if x.0 != y.0 {
        assert!(xy == Some(!x.0));
    } else if x.1 != y.1 {
        assert!(xy == Some(!x.1));
    } else if let Some(b) = xy {
        assert!(b == (x.2 < y.2) && x.2 != y.2);
    }
    assert!(!(xy == Some(true) && yx == Some(true)) && !(xy == Some(false) && yx == Some(false)));
    assert!(better_of(x, x).is_none());
    if let (Some(a), Some(b)) = (xy, yx) {
        assert!(a == !b);
    }
    kani::cover!((xy == Some(true)) & (x.0 == y.0) & (x.1 == y.1), "the cheaper of two alike");
    // The tolerance is the first route's (`EPS * (1 + |x.cost|)`), so two routes can be told apart one way and be as good the other, in a window of about EPS squared: harmless
    // (no route is ever better than one that is better than it), and the law above holds.
    kani::cover!(xy.is_some() & yx.is_none(), "two routes told apart one way and as good the other");
}

/// `across::classes::Classes` (the classes of what a save across leaves or carries, a bit for each word of the vocabulary) for **every** two classes put into an empty
/// set and every class asked: a class is in exactly when it was put in and is a bit of the set (below 32); a class put in takes no other with it, and nothing past the bits is ever
/// put in or found; and the set is empty exactly when neither was such a class. **The law failed on the old code**: a class past `u32::MAX` was taken for the bit 31.
/// (Its `words` are not asked: 21 pushes of a `String`, each beside a symbolic bit, ran past 300 s; the
/// unit test holds them.)
#[kani::proof]
fn a_class_is_in_a_set_exactly_when_it_was_put_in_and_is_a_bit_of_it() {
    use crate::across::classes::Classes;
    let (i, j, k): (usize, usize, usize) = (kani::any(), kani::any(), kani::any());
    let mut set = Classes::default();
    set.add(i);
    set.add(j);
    assert!(set.has(k) == ((k == i || k == j) && k < 32));
    assert!(set.is_empty() == (i >= 32 && j >= 32));
    kani::cover!((i == j) & (i < 21), "one class put in twice");
    kani::cover!((i >= 32) & (j < 32) & set.has(j), "a class past the bits beside one in them");
}

/// `across::hold::picture_type` (the `ForeignType` of a picture a save across holds) for **every** bytes of up to 44: `EnhMetaFile` exactly when bytes 40 to 43 are
/// ` EMF` (an EMF's first record), else `Bitmap` exactly for the first bytes of PNG, JPEG, GIF, BMP and either TIFF, and nothing for any other bytes, the empty ones included.
#[kani::proof]
#[kani::unwind(13)] // the words of the type compared, `EnhMetaFile` of 11 bytes
fn the_type_of_a_picture_is_an_emf_by_its_first_record_and_a_bitmap_by_its_signature() {
    use crate::across::hold::picture_type;
    let buf: [u8; 44] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 44);
    let bytes = buf.get(..len).unwrap_or(&[]);
    let starts = |sig: &[u8]| len >= sig.len() && buf[..sig.len()] == *sig;
    let want = if len == 44 && buf[40..44] == *b" EMF" {
        Some("EnhMetaFile")
    } else if starts(b"\x89PNG\r\n\x1a\n") || starts(b"\xFF\xD8\xFF") || starts(b"GIF8") || starts(b"BM") || starts(b"II*\0") || starts(b"MM\0*") {
        Some("Bitmap")
    } else {
        None
    };
    let got = picture_type(bytes);
    assert!(got == want);
    kani::cover!(got == Some("EnhMetaFile"), "an EMF");
    kani::cover!((got == Some("Bitmap")) & (len == 2), "a BMP's two bytes");
}

/// `pen::freeform::knot` (the knots of the `NURBSTo` row of a composite of `pieces` cubics) for **every** place and count of up to the most cubics a stroke is
/// made of (`MAX_CUBICS`): 0 at the first four places, `pieces` at place `3 pieces + 1`, and between them each whole number `k` at the three places `3k + 1` to `3k + 3`, so that the
/// knots never go down and each from 1 to `pieces - 1` comes three times. The quotient by 3 is stated by products.
#[kani::proof]
fn a_freeform_curves_knots_are_zero_four_times_then_each_whole_number_three_times_then_the_count() {
    use crate::draw::pen::{MAX_CUBICS, knot};
    let (i, pieces): (usize, usize) = (kani::any(), kani::any());
    kani::assume(pieces >= 1 && pieces <= MAX_CUBICS && i <= 3 * pieces + 1);
    let got = knot(i, pieces);
    if i < 4 {
        assert!(got == 0.0);
    } else if i == 3 * pieces + 1 {
        assert!(got == pieces as f64);
    } else {
        let k: usize = kani::any();
        kani::assume(k <= MAX_CUBICS && 3 * k < i && i <= 3 * k + 3);
        assert!(got == k as f64 && k >= 1 && k < pieces);
    }
    if i < 3 * pieces + 1 {
        assert!(knot(i, pieces) <= knot(i + 1, pieces));
    }
    kani::cover!((got == 1.0) & (i == 6), "the last place of the first whole number");
}

/// `find::Work::charge` (the budget of steps of one search, charged before the work) for **every** limit and three charges in a row: each is refused exactly when the steps
/// charged so far, as far as the largest number goes, pass the limit, so **a search refused stays refused**, and what it has used never goes down.
#[kani::proof]
fn a_search_is_refused_exactly_when_what_it_charged_passes_its_limit_and_stays_refused() {
    use crate::draw::find::Work;
    let limit: u64 = kani::any();
    let charges: [u64; 3] = kani::any();
    let mut work = Work::new(limit);
    let mut sum: u128 = 0;
    let mut refused = false;
    for c in charges {
        let before = work.used();
        sum = (sum + u128::from(c)).min(u128::from(u64::MAX));
        let got = work.charge(c);
        assert!(got.is_err() == (sum > u128::from(limit)));
        assert!(u128::from(work.used()) == sum && work.used() >= before);
        assert!(!refused || got.is_err());
        refused = got.is_err();
    }
    kani::cover!(refused & (limit == u64::MAX - 1), "a search refused at the largest limit");
}

/// `find::replace::touches_field` (a match that touches what a field shows is left) for **every** three fields in order of their starts (each from no further than its
/// end) and three matches in order of their starts, each asked in turn with the walk of the fields going on from where it was: a match touches a field exactly when they share a
/// character (the field starts before the match ends and ends after it starts), whatever the walk skipped for the matches before.
#[kani::proof]
#[kani::unwind(5)]
fn a_match_touches_a_field_exactly_when_they_share_a_character() {
    use crate::draw::find::touches_field;
    let raw: [(u8, u8); 3] = kani::any();
    let fields: [(usize, usize); 3] = raw.map(|(a, b)| (usize::from(a), usize::from(b)));
    let matches: [(u8, u8); 3] = kani::any();
    kani::assume(fields.iter().all(|&(s, e)| s <= e && e <= 8) && fields[0].0 <= fields[1].0 && fields[1].0 <= fields[2].0);
    kani::assume(matches.iter().all(|&(s, e)| s < e && e <= 8) && matches[0].0 <= matches[1].0 && matches[1].0 <= matches[2].0);
    let mut fi = 0usize;
    let mut touched_any = false;
    for (s, e) in matches.map(|(s, e)| (usize::from(s), usize::from(e))) {
        let got = touches_field(&fields, &mut fi, s, e);
        assert!(got == fields.iter().any(|&(fs, fe)| fs < e && fe > s));
        assert!(fi <= fields.len());
        touched_any |= got;
    }
    kani::cover!(touched_any, "a match that touches a field");
}

/// `formatting::Case::from_cells` and `Direction::from_cell` for **every** `Case` and `Flags` cell and every `Style`: all capitals exactly for a
/// case that rounds to 1 (from 0.5 up to but not including 1.5), initial capitals for one that rounds to 2, and else small capitals exactly when the style has their bit; right to left
/// exactly for a `Flags` that rounds to 1. A number that is not one reads as 0. Each is read back from its own cells.
#[kani::proof]
fn a_runs_case_and_a_paragraphs_direction_are_their_cells_to_the_nearest_whole_number() {
    use crate::formatting::{Case, Direction, SMALL_CAPS_BIT};
    let (case, style, flags): (f64, u32, f64) = (kani::any(), kani::any(), kani::any());
    let got = Case::from_cells(case, style);
    let want = if case >= 0.5 && case < 1.5 {
        Case::AllCaps
    } else if case >= 1.5 && case < 2.5 {
        Case::InitialCaps
    } else if style & SMALL_CAPS_BIT != 0 {
        Case::SmallCaps
    } else {
        Case::Normal
    };
    assert!(got == want);
    let rtl = Direction::from_cell(flags) == Direction::RightToLeft;
    assert!(rtl == (flags >= 0.5 && flags < 1.5));
    for c in [Case::Normal, Case::AllCaps, Case::InitialCaps, Case::SmallCaps] {
        let bits = if c == Case::SmallCaps { SMALL_CAPS_BIT } else { 0 };
        assert!(Case::from_cells(f64::from(c.cell()), bits) == c);
    }
    for d in [Direction::LeftToRight, Direction::RightToLeft] {
        assert!(Direction::from_cell(f64::from(d.cell())) == d);
    }
    kani::cover!((got == Case::SmallCaps) & case.is_nan(), "small capitals under a case that is no number");
    kani::cover!(rtl & (flags != 1.0), "a direction near 1");
}

/// `formatting::text::indent_to` for **every** `IndLeft`: a step goes to a whole number of quarter inches, **always a number**, Increase up and never past
/// `MAX_INDENT`, Decrease down and never below 0; an indent that is no number stays, and so does one of `MAX_INDENT` or more that is increased and one of 0 or less that is decreased;
/// and **within a billion inches each step is taken and moves by a quarter inch at most** (beyond that a quarter inch is under a step of the number's own, and the indent stays).
/// **The law failed on the old code**: from about minus half the largest number up, or half of it down, the count of quarter inches was an infinity, and so was the step.
#[kani::proof]
fn a_step_of_indent_goes_to_the_next_quarter_inch_and_stays_from_zero_to_a_hundred_inches() {
    use crate::formatting::{INDENT_STEP, Indent, MAX_INDENT, indent_to};
    assert!(INDENT_STEP == 0.25);
    let now: f64 = kani::any();
    let (up, down) = (indent_to(now, Indent::Increase), indent_to(now, Indent::Decrease));
    let on_grid = |r: f64| (r * 4.0).fract() == 0.0; // a whole number of quarter inches: `INDENT_STEP` is 0.25
    let near = now.abs() <= 1.0e9;
    if let Some(r) = up {
        assert!(now.is_finite() && now < MAX_INDENT && r.is_finite() && r <= MAX_INDENT && r > now && on_grid(r));
        if near {
            assert!(r <= now + INDENT_STEP + 1.0e-9);
        }
    }
    if let Some(r) = down {
        assert!(now.is_finite() && now > 1.0e-9 && r.is_finite() && r >= 0.0 && r < now && on_grid(r));
        if near {
            assert!(r == 0.0 || r >= now - INDENT_STEP - 1.0e-9);
        }
    }
    if near {
        assert!(up.is_some() == (now < MAX_INDENT) && down.is_some() == (now > 1.0e-9));
    }
    kani::cover!((up == Some(0.25)) & (now == 0.0), "0 up to a quarter inch");
    kani::cover!((down == Some(0.0)) & (now == 0.25), "a quarter inch down to 0");
    kani::cover!(up.is_none() & (now < -1.0e300), "an indent a quarter inch does not move, which stays");
}

/// `draw::select::box_inside` (a shape's box wholly inside the net of an area select) for **every** three boxes: a box is inside another exactly when each of its edges is
/// within that box's, an edge in common allowed; a box is inside itself exactly when none of its edges is no number; two boxes each inside the other are the same box; and a box inside
/// one inside a third is inside the third. Comparisons only, so twelve floats are within reach.
#[kani::proof]
fn a_box_inside_a_box_inside_a_third_is_inside_the_third() {
    use crate::draw::box_inside;
    use crate::snap::Rect;
    let any_rect = || Rect { x0: kani::any(), y0: kani::any(), x1: kani::any(), y1: kani::any() };
    let (a, b, c) = (any_rect(), any_rect(), any_rect());
    let ab = box_inside(&a, &b);
    assert!(ab == (a.x0 >= b.x0 && a.y0 >= b.y0 && a.x1 <= b.x1 && a.y1 <= b.y1));
    assert!(box_inside(&a, &a) == !(a.x0.is_nan() || a.y0.is_nan() || a.x1.is_nan() || a.y1.is_nan()));
    if ab && box_inside(&b, &a) {
        assert!(a.x0 == b.x0 && a.y0 == b.y0 && a.x1 == b.x1 && a.y1 == b.y1);
    }
    if ab && box_inside(&b, &c) {
        assert!(box_inside(&a, &c));
    }
    kani::cover!(ab & (a.x0 == b.x0) & (a.x1 < b.x1), "a box on another's left edge, inside it");
}

/// The value of a hexadecimal digit, by its ASCII place (the law's own, not the code's `to_digit`).
fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `draw::links::percent_byte` (a followed link decoded strictly) for **every** three bytes after a percent sign: the first two are the byte they stand for exactly when
/// both are hexadecimal digits, in either case, whatever follows; fewer than two bytes stand for none; and **every byte** is given back by its two upper-case digits.
#[kani::proof]
fn two_hexadecimal_digits_after_a_percent_sign_are_the_byte_they_stand_for() {
    use crate::draw::links::percent_byte;
    let (hi, lo, after): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    let got = percent_byte(&[hi, lo, after]);
    match (hex_value(hi), hex_value(lo)) {
        (Some(h), Some(l)) => assert!(got == Some(h * 16 + l)),
        _ => assert!(got.is_none()),
    }
    assert!(percent_byte(&[hi, lo]) == got);
    assert!(percent_byte(&[hi]).is_none() && percent_byte(&[]).is_none());
    let b: u8 = kani::any();
    let digit = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
    assert!(percent_byte(&[digit(b >> 4), digit(b & 15)]) == Some(b));
    kani::cover!(got == Some(0xFF), "the largest byte");
    kani::cover!(got.is_none() & hex_value(hi).is_some(), "a first digit and no second");
}

/// The bytes a text of a link is drawn from, by kind: a percent sign, hexadecimal digits at the ends of their runs and between (`0`, `2`, `7`, `9`, `A`, `C`, `F`, `a`, `f`), a letter
/// past them (`g`), a blank and a control character (`\x01`). What `percent_decode_bytes` makes of bytes depends only on where the percent signs are and the values of the digits
/// after them, so bytes of these in each place stand for every bytes of ASCII of that length whose sequences decode to these values.
const LINK_KINDS: &[u8; 13] = b"%0279ACFafg \x01";

/// `draw::links::percent_decode_bytes`, the work of `percent_decode` (a followed link is read **strictly**, and 2), of every four bytes by kinds: they decode
/// only when every percent sign starts two hexadecimal digits (the digits are never a percent sign, so the sequences do not overlap), and then each sequence is one byte, two bytes
/// fewer; and bytes with no percent sign are themselves. It never panics nor reads past them.
#[kani::proof]
#[kani::unwind(6)]
fn bytes_of_a_link_decode_only_when_every_percent_sign_starts_two_hexadecimal_digits() {
    use crate::draw::links::percent_decode_bytes;
    let kinds: [u8; 4] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < LINK_KINDS.len()));
    let raw = kinds.map(|k| LINK_KINDS[usize::from(k)]);
    let (mut whole, mut signs) = (true, 0usize);
    for (i, b) in raw.iter().enumerate() {
        if *b == b'%' {
            signs += 1;
            whole &= raw.get(i + 1).copied().and_then(hex_value).is_some() && raw.get(i + 2).copied().and_then(hex_value).is_some();
        }
    }
    let got = percent_decode_bytes(&raw);
    if let Some(decoded) = &got {
        assert!(whole && decoded.len() + 2 * signs == raw.len());
    }
    if !whole {
        assert!(got.is_none());
    }
    if signs == 0 {
        assert!(got.as_deref().map(str::as_bytes) == Some(raw.as_slice()));
    }
    kani::cover!(got.is_some() & (signs == 1), "a sequence decoded");
    kani::cover!(got.is_none() & whole, "sequences of bytes that are no text");
    kani::cover!(!whole & (signs == 1), "a lone percent sign");
}

/// `percent_decode_bytes` of **every** percent sign and two bytes: they decode exactly when the two are hexadecimal digits, in either case, whose byte is text on its own (below 0x80),
/// to that byte; a lone percent sign and a byte that is no text are refused.
#[kani::proof]
#[kani::unwind(6)]
fn a_percent_sequence_decodes_strictly_to_its_byte() {
    use crate::draw::links::percent_decode_bytes;
    let (x, y): (u8, u8) = (kani::any(), kani::any());
    let got = percent_decode_bytes(&[b'%', x, y]);
    match hex_value(x).zip(hex_value(y)).map(|(h, l)| h * 16 + l) {
        Some(v) if v < 0x80 => assert!(got.as_deref().map(str::as_bytes) == Some([v].as_slice())),
        _ => assert!(got.is_none()),
    }
    kani::cover!(got.as_deref() == Some("\u{7f}"), "the last control of ASCII");
}

/// `percent_decode_bytes` of **every** two percent sequences of upper-case digits: they decode exactly when their two bytes are text, two characters of ASCII or one of two bytes
/// (0xC2 to 0xDF, then 0x80 to 0xBF), to those bytes.
#[kani::proof]
#[kani::unwind(8)]
fn two_percent_sequences_decode_strictly_to_their_bytes_when_they_are_text() {
    use crate::draw::links::percent_decode_bytes;
    let digits: [u8; 4] = kani::any();
    let [a, b, c, d] = digits.map(|n| b"0123456789ABCDEF"[usize::from(n & 15)]);
    let (v, w) = ((digits[0] & 15) * 16 + (digits[1] & 15), (digits[2] & 15) * 16 + (digits[3] & 15));
    let ascii = v < 0x80 && w < 0x80;
    let pair = (0xC2..=0xDF).contains(&v) && (0x80..=0xBF).contains(&w);
    let got = percent_decode_bytes(&[b'%', a, b, b'%', c, d]);
    if ascii | pair {
        assert!(got.as_deref().map(str::as_bytes) == Some([v, w].as_slice()));
    } else {
        assert!(got.is_none());
    }
    kani::cover!(pair & (v == 0xC2) & (w == 0x85), "a control of two bytes, U+0085");
    kani::cover!(!ascii & !pair, "bytes that are no text");
}

/// Whether the text of `raw` holds a control character (U+0000 to U+001F, U+007F to U+009F) or an invisible or direction mark (U+200B to U+200F, U+202A to U+202E, U+2066 to
/// U+2069, U+FEFF), read from its bytes (the law's own reading of three bytes of UTF-8).
fn holds_hidden(raw: &[u8; 3]) -> bool {
    let c1 = |lead: u8, next: u8| lead == 0xC2 && (0x80..=0x9F).contains(&next);
    let marks = (raw[0] == 0xE2 && raw[1] == 0x80 && matches!(raw[2], 0x8B..=0x8F | 0xAA..=0xAE))
        || (raw[0] == 0xE2 && raw[1] == 0x81 && matches!(raw[2], 0xA6..=0xA9))
        || *raw == [0xEF, 0xBB, 0xBF];
    raw.iter().any(|b| *b < 0x20 || *b == 0x7F) || c1(raw[0], raw[1]) || c1(raw[1], raw[2]) || marks
}

/// `draw::links::hides`, what `hides_something` says of a link and what it decodes to, for **every** text of three bytes, as the text and as what it
/// decodes to: something hides exactly when it holds a control character or an invisible or direction mark, of one, two or three bytes.
#[kani::proof]
#[kani::unwind(5)]
fn a_link_hides_something_exactly_when_it_holds_a_control_character_or_an_invisible_mark() {
    use crate::draw::links::hides;
    let raw: [u8; 3] = kani::any();
    let Ok(text) = std::str::from_utf8(&raw) else { return };
    let hidden = holds_hidden(&raw);
    assert!(hides(text, "") == hidden && hides("", text) == hidden);
    kani::cover!(hidden & (raw[0] == 0xE2), "an invisible mark");
    kani::cover!(hidden & (raw[0] == 0xC2), "a control of two bytes");
    kani::cover!(!hidden & (raw[0] >= 0x80), "a character of more than a byte that hides nothing");
}

/// `draw::pictures::jpeg_size` of the shortest baseline JPEG (a frame header 0xC0), for **every** width and height: the width and height the header
/// says, the bytes read in their order.
#[kani::proof]
#[kani::unwind(12)]
fn a_baseline_jpegs_frame_header_gives_its_width_and_height() {
    use crate::draw::pictures::jpeg_size;
    let (w, h): (u16, u16) = (kani::any(), kani::any());
    let [w0, w1] = w.to_be_bytes();
    let [h0, h1] = h.to_be_bytes();
    let got = jpeg_size(&[0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, h0, h1, w0, w1]);
    assert!(got == Some((u32::from(w), u32::from(h))));
    kani::cover!((w > 0xFF) & (h > 0xFF), "a size of two bytes each way");
}

/// The number of `k` bytes at `at` of `b`, big-endian when `big`: `None` when they are not all within it (the law's own reading).
fn number_at(b: &[u8], at: usize, k: usize, big: bool) -> Option<u32> {
    let end = at.checked_add(k)?;
    if end > b.len() {
        return None;
    }
    let mut n = 0u32;
    for i in 0..k {
        let byte = u32::from(b[if big { at + i } else { end - 1 - i }]);
        n = (n << 8) | byte;
    }
    Some(n)
}

/// The readers of a picture's header (`draw::pictures`: `be16`, `be32`, `le16`, `le32`, `le_i32`) for **every** six bytes, every length of them and every place,
/// the largest included: the number of two or four bytes at the place, in its order, exactly when they are all within the bytes, and none otherwise. None reads past the end.
#[kani::proof]
#[kani::unwind(7)]
fn a_number_of_a_pictures_header_is_its_bytes_in_order_and_none_past_the_end() {
    use crate::draw::pictures::{be16, be32, le_i32, le16, le32};
    let raw: [u8; 6] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 6);
    let b = &raw[..len];
    let at: usize = kani::any();
    assert!(be16(b, at) == number_at(b, at, 2, true));
    assert!(le16(b, at) == number_at(b, at, 2, false));
    assert!(be32(b, at) == number_at(b, at, 4, true));
    assert!(le32(b, at) == number_at(b, at, 4, false));
    assert!(le_i32(b, at).map(i32::cast_unsigned) == number_at(b, at, 4, false));
    kani::cover!(le32(b, at).is_some() & (at == 2), "four bytes that end at the end");
    kani::cover!(be16(b, at).is_none() & (at == usize::MAX), "the largest place");
}

/// The bytes of a TIFF of two entries, the width and the height, each a short (3) or a long (4) as `long_w` and `long_h` say, in the byte order `little` says, with the first
/// directory at 8.
fn tiff_of(little: bool, (w, long_w): (u32, bool), (h, long_h): (u32, bool)) -> [u8; 34] {
    let mut b = [0u8; 34];
    let put16 = |b: &mut [u8; 34], at: usize, v: u16| b[at..at + 2].copy_from_slice(&if little { v.to_le_bytes() } else { v.to_be_bytes() });
    let put32 = |b: &mut [u8; 34], at: usize, v: u32| b[at..at + 4].copy_from_slice(&if little { v.to_le_bytes() } else { v.to_be_bytes() });
    b[..4].copy_from_slice(if little { &[0x49, 0x49, 0x2A, 0x00] } else { &[0x4D, 0x4D, 0x00, 0x2A] });
    put32(&mut b, 4, 8);
    put16(&mut b, 8, 2);
    for (n, (tag, v, long)) in [(256u16, w, long_w), (257u16, h, long_h)].into_iter().enumerate() {
        let entry = 10 + 12 * n;
        put16(&mut b, entry, tag);
        put16(&mut b, entry + 2, if long { 4 } else { 3 });
        put32(&mut b, entry + 4, 1);
        if long {
            put32(&mut b, entry + 8, v);
        } else {
            put16(&mut b, entry + 8, v as u16);
        }
    }
    b
}

/// `draw::pictures::tiff_size` of a TIFF of two entries, in either byte order, each a short or a long, for **every** width and height: the size the entries say.
#[kani::proof]
#[kani::unwind(5)]
fn a_tiffs_size_is_what_its_width_and_height_entries_say_in_either_byte_order() {
    use crate::draw::pictures::tiff_size;
    let (little, long_w, long_h): (bool, bool, bool) = (kani::any(), kani::any(), kani::any());
    let (w, h): (u32, u32) = (kani::any(), kani::any());
    let short = |v: u32, long: bool| if long { v } else { u32::from(v as u16) };
    let got = tiff_size(&tiff_of(little, (w, long_w), (h, long_h)));
    assert!(got == Some((short(w, long_w), short(h, long_h))));
    kani::cover!(little & !long_w & long_h & (h > 0xFFFF), "a long height in little-endian order");
}

/// `tiff_size` of **every** 34 bytes: it never panics nor reads past them, and a first directory that says more entries than `MAX_TIFF_ENTRIES` gives no size.
#[kani::proof]
#[kani::unwind(5)]
fn a_tiffs_size_from_any_bytes_never_reads_past_them_nor_takes_more_entries_than_the_most() {
    use crate::draw::pictures::{MAX_TIFF_ENTRIES, tiff_size};
    let b: [u8; 34] = kani::any();
    let got = tiff_size(&b);
    let little = b[0] == 0x49 && b[1] == 0x49;
    let ifd = number_at(&b, 4, 4, !little).and_then(|n| usize::try_from(n).ok());
    let count = ifd.and_then(|at| number_at(&b, at, 2, !little));
    if count.is_some_and(|n| n as usize > MAX_TIFF_ENTRIES) {
        assert!(got.is_none());
    }
    kani::cover!(got.is_some(), "a size");
    kani::cover!(count.is_some_and(|n| n as usize > MAX_TIFF_ENTRIES), "a directory of more entries than the most");
}

/// `draw::pictures::Format::drawn`: every format is drawn but a GIF and a TIFF, which are held and drawn as a placeholder; and a metafile is typed as one.
#[kani::proof]
fn every_picture_format_is_drawn_but_a_gif_and_a_tiff() {
    use crate::draw::pictures::Format;
    let n: u8 = kani::any();
    let format = match n % 7 {
        0 => Format::Png,
        1 => Format::Jpeg,
        2 => Format::Gif,
        3 => Format::Bmp,
        4 => Format::Tiff,
        5 => Format::Emf,
        _ => Format::Wmf,
    };
    assert!(format.drawn() == !matches!(format, Format::Gif | Format::Tiff));
    kani::cover!(!format.drawn(), "a format held and not drawn");
}

/// `draw::fields::Tally::charge` (the fields of a call counted against one budget before any is worked out, MAX_FIELDS_PER_CALL) for **three** shapes in a row, each of any
/// count of fields, against any budget short of the largest number: a shape is kept exactly when it has fields, holds no more than `MAX_FIELDS`, the fields counted so far and its own (those of the shapes
/// skipped too) are within the budget, and no shape before passed it; **the fields of the shapes kept never pass the budget**; once a shape passes it no shape is kept after; the
/// fields of the shapes of more than `MAX_FIELDS` are counted as skipped, and never wrap.
#[kani::proof]
fn the_fields_of_a_call_are_counted_against_one_budget_and_none_kept_past_it() {
    use crate::draw::fields::{MAX_FIELDS, Tally};
    let limit: usize = kani::any();
    // The calls' budgets are 100,000 and 10,000 fields; the largest number would be none.
    kani::assume(limit < usize::MAX);
    let mut tally = Tally::new(limit);
    let (mut counted, mut kept, mut skipped) = (0u128, 0u128, 0u128);
    let mut passed = false;
    let (mut skipped_one, mut kept_after_skip, mut stopped) = (false, false, false);
    for _ in 0..3 {
        let count: usize = kani::any();
        let took = tally.charge(count);
        let before = passed;
        if !passed && count > 0 {
            counted += count as u128;
            passed = counted > limit as u128;
        }
        let want = !passed && count > 0 && count <= MAX_FIELDS;
        assert!(took == want);
        if took {
            kept += count as u128;
            kept_after_skip |= skipped_one;
        } else if !passed && count > MAX_FIELDS {
            skipped += count as u128;
            skipped_one = true;
        }
        stopped |= before & (count > 0) & (count <= MAX_FIELDS);
        let (seen_skipped, over) = tally.skipped_and_over();
        assert!(over == passed);
        assert!(seen_skipped as u128 == skipped.min(usize::MAX as u128));
        assert!(kept <= limit as u128);
    }
    kani::cover!(kept_after_skip, "a shape kept after a shape of too many fields was skipped");
    kani::cover!(stopped, "a shape refused after the budget was passed");
}

/// `draw::change::glue_index` (an end glued to a connection point of the shape that changed) for **every** `ToPart` or none: the row of the point exactly
/// from part 100, and then 100 less than the part.
#[kani::proof]
fn a_glued_end_names_a_connection_point_row_exactly_from_part_one_hundred() {
    let c = diacraft_doc::Connect { to_part: kani::any(), ..Default::default() };
    let got = crate::draw::glue_index(&c);
    match c.to_part {
        Some(part) if part >= 100 => assert!(got == Some(part - 100)),
        _ => assert!(got.is_none()),
    }
    kani::cover!(got == Some(0), "the first row");
    kani::cover!(c.to_part.is_some() & got.is_none(), "a part of the shape that is no connection point");
    std::mem::forget(c);
}

/// `stencil::CalloutStyle::default_tip` (where a free callout's leader ends) for **every** one of the fourteen styles, every width of the callout from 0 to half the
/// largest number and every count of drawing units to an inch up to it: a point below the box (never above its bottom), off its right side for the right bracket and its left for
/// every other, never no number. One style only has its leader leave by the right (the right bracket).
#[kani::proof]
#[kani::unwind(15)]
fn a_free_callouts_leader_ends_below_its_box_off_the_side_it_leaves_by() {
    use crate::stencil::CALLOUT_STYLES;
    let (width, k): (f64, f64) = (kani::any(), kani::any());
    kani::assume((0.0..=f64::MAX / 2.0).contains(&width) && (0.0..=f64::MAX / 2.0).contains(&k));
    let i: usize = kani::any();
    kani::assume(i < CALLOUT_STYLES.len());
    let style = &CALLOUT_STYLES[i];
    let (x, y) = style.default_tip(width, k);
    assert!(x.is_finite() && y.is_finite() && y <= 0.0);
    if style.faces_right() {
        assert!(x >= width);
    } else {
        assert!(x <= 0.0);
    }
    assert!(CALLOUT_STYLES.iter().filter(|s| s.faces_right()).count() == 1);
    kani::cover!(style.faces_right() & (x > width), "a tip off the right side");
    kani::cover!(!style.faces_right() & (y < 0.0) & (x < 0.0), "a tip below and to the left");
}

/// `snap::Glue::shape` (the shape a glued end landed on, which a layer that allows no glue bars) for **every** kind of glue and every shape and point: the shape it
/// landed on, whatever the kind.
#[kani::proof]
fn a_glued_end_names_the_shape_it_landed_on_whatever_it_landed_on() {
    use crate::snap::Glue;
    let (shape, point, kind): (u32, u32, u8) = (kani::any(), kani::any(), kani::any());
    let glue = match kind % 4 {
        0 => Glue::ConnectionPoint { shape, point },
        1 => Glue::Vertex { shape },
        2 => Glue::Handle { shape },
        _ => Glue::Outline { shape },
    };
    assert!(glue.shape() == shape);
    kani::cover!(matches!(glue, Glue::Outline { .. }), "an end on an outline");
}

/// A box of finite sides in order (a box a shape's corners make), drawn whole.
fn any_box() -> crate::draw::container::Rect {
    let r = crate::draw::container::Rect { x0: kani::any(), y0: kani::any(), x1: kani::any(), y1: kani::any() };
    kani::assume(r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite() && r.x0 <= r.x1 && r.y0 <= r.y1);
    r
}

/// `draw::container::Rect::centre` (the middle of a shape's box, which says the container it is in, and the place a member on an edge keeps) for **every** box of
/// finite sides in order: the middle is in the box, and is no infinity; its width and height are not negative. **Failed on the old code** (sides past half the largest number summed
/// to an infinity; gaps F38).
#[kani::proof]
fn a_boxs_middle_is_in_it_whatever_its_sides() {
    let a = any_box();
    let (cx, cy) = a.centre();
    assert!(cx.is_finite() && cy.is_finite() && a.contains((cx, cy)));
    assert!(a.width() >= 0.0 && a.height() >= 0.0);
    kani::cover!((a.x0 > f64::MAX / 2.0) & (a.x0 < a.x1), "a box past half the largest number");
}

/// `Rect::union` and `holds` (the box round a container's members, and whether a box holds another) for **every** two boxes of finite sides in order: a box holds
/// itself, and the box round two holds each of them whichever is first.
#[kani::proof]
fn the_box_round_two_boxes_holds_each() {
    let (a, b) = (any_box(), any_box());
    assert!(a.holds(&a));
    let ab = a.union(b);
    assert!(ab.holds(&a) && ab.holds(&b) && ab == b.union(a));
    kani::cover!(ab != a && ab != b, "a box round two that is neither");
}

/// `draw::container::Resize` and `Heading` (how a container resizes and where its heading sits, held as numbers in `User` rows): each is read
/// back from its own number, and from its own word; **every** number reads as one, the nearest within a half, a heading of no number at the top and a resize of none as needed; and
/// four steps of Rotate Heading come back to the side they began from, a turn through each side once.
#[kani::proof]
#[kani::unwind(8)]
fn a_containers_settings_read_back_from_their_numbers_and_words_and_four_turns_of_its_heading_come_round() {
    use crate::draw::container::{Heading, Resize};
    let n: f64 = kani::any();
    let resize = Resize::from_number(n);
    let near = |k: f64| (n - k).abs() < 0.5;
    assert!(
        resize
            == if near(0.0) {
                Resize::None
            } else if near(2.0) {
                Resize::Fit
            } else {
                Resize::Expand
            }
    );
    let side = Heading::from_number(n);
    let want = if near(1.0) {
        Heading::Right
    } else if near(2.0) {
        Heading::Bottom
    } else if near(3.0) {
        Heading::Left
    } else {
        Heading::Top
    };
    assert!(side == want);
    let pick: u8 = kani::any();
    let r = [Resize::None, Resize::Expand, Resize::Fit][usize::from(pick % 3)];
    assert!(Resize::from_number(f64::from(r.number())) == r && Resize::from_word(r.word()) == Some(r));
    let s = Heading::AROUND[usize::from(pick % 4)];
    assert!(Heading::from_number(f64::from(s.number())) == s && Heading::from_word(s.word()) == Some(s));
    assert!(s.next() != s && s.next().next().next().next() == s && s.next().is_vertical() != s.is_vertical());
    kani::cover!(n.is_nan() & (resize == Resize::Expand) & (side == Heading::Top), "no number");
    kani::cover!((side == Heading::Left) & (n > 3.0), "a number just above the left's");
}

/// `draw::ties::whole_code` (the code a `DEPENDSON` call opens with) for **every** number: a code exactly for a whole number from 0 to 999,999, the number itself
/// with nothing cut; none for a fraction, a number past the range, an infinity or no number.
#[kani::proof]
fn a_calls_code_is_a_whole_number_below_a_million_and_nothing_is_cut_from_it() {
    let code: f64 = kani::any();
    let got = crate::draw::ties::whole_code(code);
    match got {
        Some(n) => assert!(n < 1_000_000 && f64::from(n) == code),
        None => assert!(!(code >= 0.0 && code < 1.0e6) || code != f64::from(code as u32)),
    }
    kani::cover!(got == Some(999_999), "the last code");
    kani::cover!(got.is_none() & (code > 0.0) & (code < 1.0), "a fraction");
}

/// `check::Class::bit` and the checker's `Tally` (the classes of what a save would lose, and what it counts of each) for **every** class of `ALL` (25 since
/// added the comments; at most the 32 bits of a set), two adds of any counts and a set of flags: each class has a bit of its own, below 2 to the number of classes;
/// an add counts only its class, held at the largest number; and the flags add one to each class they hold. (wrote the bound as 2^24, for the 24 classes there were
/// then: the harness failed on main once added the 25th, a law stated with a count that changed, not a fault of the code; it by `ALL`.)
#[kani::proof]
#[kani::unwind(27)]
fn a_class_of_a_loss_has_a_bit_of_its_own_and_is_counted_alone() {
    use crate::check::{ALL, Tally};
    assert!(ALL.len() <= 32);
    let (i, j): (usize, usize) = (kani::any(), kani::any());
    kani::assume(i < ALL.len() && j < ALL.len());
    let (ci, cj) = (ALL[i], ALL[j]);
    assert!(u64::from(ci.bit()) < 1u64 << ALL.len() && ci.bit().count_ones() == 1);
    assert!((ci.bit() == cj.bit()) == (i == j));
    let (a, b): (u64, u64) = (kani::any(), kani::any());
    let mut tally = Tally::default();
    tally.add(ci, a);
    tally.add(cj, b);
    let both = if i == j { a.saturating_add(b) } else { a };
    assert!(tally.count(ci) == both);
    let k: usize = kani::any();
    kani::assume(k < ALL.len() && k != i && k != j);
    assert!(tally.count(ALL[k]) == 0);
    let flags: u32 = kani::any();
    let before = tally.count(ALL[k]);
    tally.flagged(flags);
    assert!(tally.count(ALL[k]) == before + u64::from(flags & ALL[k].bit() != 0));
    kani::cover!((i != j) & (tally.count(cj) == b.saturating_add(1)), "a class added to and flagged");
}

/// The checker's `Meter` (the steps a comparison takes, charged before the work) for **three** charges in a row of any steps within any budget: the steps never go down
/// and are held at the largest number; a charge is within the budget exactly when the steps so far are; and the meter is spent exactly when they are past it, and stays so.
#[kani::proof]
fn a_comparisons_steps_are_charged_before_the_work_and_once_past_the_budget_it_stays_past() {
    use crate::check::Meter;
    let max: u64 = kani::any();
    let meter = Meter::new(max);
    let mut steps = 0u128;
    let mut spent = false;
    for _ in 0..3 {
        let n: usize = kani::any();
        let within = meter.charge(n);
        steps += n as u128;
        let held = steps.min(u128::from(u64::MAX));
        assert!(within == (held <= u128::from(max)));
        assert!(meter.exhausted() == !within && meter.check().is_err() == !within);
        assert!(!(spent & within));
        spent |= !within;
    }
    kani::cover!(spent, "a comparison past its budget");
}

/// `check::cells::close` (whether two numbers of cells are one value) for **every** finite number, the only numbers a value reads as: a number is one value with
/// itself, and none with a number that is no number; and numbers a ten-millionth of their size apart are not one value, where a hundredth of that is.
#[kani::proof]
fn a_number_is_one_value_with_itself_and_none_with_no_number() {
    use crate::check::close;
    let a: f64 = kani::any();
    kani::assume(a.is_finite());
    assert!(close(a, a));
    assert!(!close(a, f64::NAN) && !close(f64::NAN, a));
    assert!(!close(1.0, 1.0 + 1.0e-7) && close(1.0, 1.0 + 1.0e-9) && !close(1.0e6, 1.0e6 + 0.1) && close(1.0e6, 1.0e6 + 1.0e-4));
    kani::cover!(a.abs() > 1.0e300, "a number near the largest");
}

/// `shapedata::Kind::from_code` and `code` (the `Type` cell's number) for **every** `i64`: a kind exactly from 0 to 7, whose number is the one given;
/// and every kind of the eight, in the order of `Kind::ALL`, is its own place there and reads back from its number.
#[kani::proof]
#[kani::unwind(10)]
fn a_type_cells_number_is_a_kind_exactly_from_zero_to_seven_and_each_kind_reads_back() {
    use crate::draw::shapedata::Kind;
    let n: i64 = kani::any();
    let kind = Kind::from_code(n);
    match kind {
        Some(k) => {
            assert!(i64::from(k.code()) == n);
        }
        None => {
            assert!(!(0..=7).contains(&n));
        }
    }
    assert!(kind.is_some() == (0..=7).contains(&n));
    let i: usize = kani::any();
    kani::assume(i < Kind::ALL.len());
    let k = Kind::ALL[i];
    assert!(usize::from(k.code()) == i);
    assert!(Kind::from_code(i64::from(k.code())) == Some(k));
    kani::cover!(kind == Some(Kind::Currency), "the last kind");
    kani::cover!(kind.is_none() & (n == 8), "one past the last");
    kani::cover!(kind.is_none() & (n < 0), "below the first");
}

/// `shapedata::whole` (a `LangID` or a `Calendar` cell's number as the field's whole number) for **every** number: a whole number exactly when the number is one
/// from 0 to `u32::MAX`, and then the number itself, nothing cut; none for a fraction, a number below 0 or past the range, an infinity or no number.
#[kani::proof]
fn a_fields_whole_number_is_the_cells_number_exactly_when_it_is_a_whole_number_of_a_u32() {
    let n: f64 = kani::any();
    let got = crate::draw::shapedata::whole(n);
    let whole_in_range = n >= 0.0 && n <= f64::from(u32::MAX) && f64::from(n as u32) == n;
    match got {
        Some(v) => {
            assert!(f64::from(v) == n);
        }
        None => {
            assert!(!whole_in_range);
        }
    }
    assert!(got.is_some() == whole_in_range);
    kani::cover!(got == Some(u32::MAX), "the largest");
    kani::cover!(got.is_none() & (n > 0.0) & (n < 1.0), "a fraction");
    kani::cover!(got.is_none() & (n == 4_294_967_296.0), "one past the largest");
}

/// A sort key of up to two bytes, of any ASCII values: the keys the order is asked of are lowered texts, and the order of texts is the order of their bytes.
fn any_key(bytes: &[u8; 2]) -> &str {
    let len: usize = kani::any();
    kani::assume(len <= 2);
    kani::assume(bytes.iter().all(|b| *b < 0x80));
    std::str::from_utf8(&bytes[..len]).unwrap_or("")
}

/// `shapedata::listed_order` (the order of the fields by their sort keys, the unkeyed last), the comparator of the list's stable sort, for **every**
/// three keys of up to two bytes: it is an order (the two ways round are each other's reverse, it is the same only for the same key, and it is transitive), and a field with a key
/// comes before every field with none.
#[kani::proof]
#[kani::unwind(4)]
fn the_fields_are_listed_by_their_keys_in_one_order_and_the_unkeyed_last() {
    use crate::draw::shapedata::listed_order;
    use std::cmp::Ordering;
    let (ra, rb, rc): ([u8; 2], [u8; 2], [u8; 2]) = (kani::any(), kani::any(), kani::any());
    let (a, b, c) = (any_key(&ra), any_key(&rb), any_key(&rc));
    let ab = listed_order(a, b);
    assert!(ab == listed_order(b, a).reverse());
    assert!((ab == Ordering::Equal) == (a.as_bytes() == b.as_bytes()));
    if listed_order(a, b) != Ordering::Greater && listed_order(b, c) != Ordering::Greater {
        assert!(listed_order(a, c) != Ordering::Greater);
    }
    if a.is_empty() && !b.is_empty() {
        assert!(ab == Ordering::Greater);
    }
    kani::cover!((ab == Ordering::Less) & !b.is_empty(), "two keys in order");
    kani::cover!((ab == Ordering::Greater) & a.is_empty(), "the unkeyed after a keyed one");
    kani::cover!((ab == Ordering::Less) & (a.len() == 2) & (b.len() == 2), "keys of two bytes");
}

/// `formatting::share` (a transparency's percent as the share the cell holds, on the half-percent steps) for **every** number: from 0 to 1 and never a
/// NaN; 0 exactly for an infinity, no number and a percent below a quarter; 1 exactly from 99.75 up; and between, at least the first step (0.005) and at most the last (0.995).
#[kani::proof]
fn a_transparency_is_a_share_from_nothing_to_all_on_half_percent_steps() {
    use crate::formatting::share;
    let p: f64 = kani::any();
    let s = share(p);
    assert!((0.0..=1.0).contains(&s));
    assert!((s == 0.0) == (!p.is_finite() || p < 0.25));
    assert!((s == 1.0) == (p.is_finite() && p >= 99.75));
    assert!(s == 0.0 || s >= 0.005);
    assert!(s == 1.0 || s <= 0.995);
    kani::cover!(s == 0.005, "the first step");
    kani::cover!((s > 0.0) & (s < 1.0) & (p > 50.0), "a share between");
}

/// `formatting::radians_within_turn` (`shape.gradient`'s angle as the radians the cell holds, of the angle within a turn that the remainder by 360 leaves) for
/// **every** finite angle within a turn either way: a number of at most a turn, of the angle's sign or 0, and **0 is never minus 0** (a cell's text of it is `0`); an angle of
/// less than half a billionth of a radian is 0 and one of more is not.
#[kani::proof]
fn an_angle_within_a_turn_is_written_as_radians_to_nine_places_and_never_as_minus_zero() {
    use crate::formatting::radians_within_turn;
    let t: f64 = kani::any();
    kani::assume(t.is_finite() && t.abs() < 360.0);
    let r = radians_within_turn(t);
    assert!(r.is_finite() && r.abs() <= 6.283_185_308);
    assert!(r != 0.0 || r.is_sign_positive(), "a zero is plus zero");
    assert!(r == 0.0 || ((r > 0.0) == (t > 0.0)));
    if t.abs() <= 2.86e-8 {
        assert!(r == 0.0);
    }
    if t.abs() >= 2.87e-8 {
        assert!(r != 0.0);
    }
    kani::cover!((r == 0.0) & (t < 0.0), "a small angle below 0");
    kani::cover!(r < -6.28, "nearly a turn below 0");
}

/// `formatting::GradientKind::of_dir` and `origin_of` (the table) for **every** `FillGradientDir` byte: one kind each, the linear exactly for 0 and the path
/// from 13 up; an origin exactly for the radial and the rectangular ones, and a rectangular one is never at the middle of an edge; and for every direction from 0 to 13 (each one
/// in turn), `dir_at` of its kind and its origin gives it back, and a direction past 13 is the path's own, 13.
#[kani::proof]
#[kani::unwind(16)]
fn a_gradients_direction_is_of_one_kind_and_its_kind_and_origin_give_it_back() {
    use crate::formatting::GradientKind;
    let d: u8 = kani::any();
    let kind = GradientKind::of_dir(d);
    let origin = GradientKind::origin_of(d);
    assert!((kind == GradientKind::Linear) == (d == 0));
    assert!((kind == GradientKind::Path) == (d >= 13));
    assert!((kind == GradientKind::Radial) == (1..=7).contains(&d));
    assert!(origin.is_empty() == (kind == GradientKind::Linear || kind == GradientKind::Path));
    if kind == GradientKind::Rectangular {
        assert!(origin != "top" && origin != "bottom");
    }
    for each in 0u8..=13 {
        let k = GradientKind::of_dir(each);
        assert!(k.dir_at(GradientKind::origin_of(each)) == Some(each));
    }
    assert!(GradientKind::of_dir(200).dir_at(GradientKind::origin_of(200)) == Some(13));
    kani::cover!(kind == GradientKind::Rectangular, "a rectangular one");
    kani::cover!((kind == GradientKind::Path) & (d > 13), "a number past the table");
}

/// `formatting::QuickStyle::at` and `variant` (the Quick Style gallery) for **every** two places of each: a Theme Style exactly for rows 1 to 6 and columns 1 to 7,
/// an entry of 1 to 6 and a colour of 100 to 106, two places one style exactly when they are one place; a Variant Style exactly for 0 to 3, an entry of 100 to 103 in the first
/// colour, one style each; and no Variant Style is a Theme Style.
#[kani::proof]
fn a_quick_style_is_one_place_of_the_gallery_or_one_variant_and_no_two_places_share_one() {
    use crate::formatting::QuickStyle;
    let (row, column, row2, column2): (u32, u32, u32, u32) = (kani::any(), kani::any(), kani::any(), kani::any());
    let (place, place2): (u32, u32) = (kani::any(), kani::any());
    let s = QuickStyle::at(row, column);
    assert!(s.is_some() == ((1..=6).contains(&row) && (1..=7).contains(&column)));
    if let Some(q) = s {
        assert!((1..=6).contains(&q.matrix) && (100..=106).contains(&q.colour));
    }
    if let (Some(a), Some(b)) = (s, QuickStyle::at(row2, column2)) {
        assert!((a == b) == (row == row2 && column == column2));
    }
    let v = QuickStyle::variant(place);
    assert!(v.is_some() == (place <= 3));
    if let Some(q) = v {
        assert!((100..=103).contains(&q.matrix) && q.colour == 100);
        assert!(s != Some(q));
    }
    if let (Some(a), Some(b)) = (v, QuickStyle::variant(place2)) {
        assert!((a == b) == (place == place2));
    }
    kani::cover!(s == Some(QuickStyle { matrix: 6, colour: 106 }), "the last of the gallery");
    kani::cover!(v == Some(QuickStyle { matrix: 103, colour: 100 }), "the fourth variant");
}

/// A side of the page by a number.
fn side_of(n: u8) -> diacraft_layout::Side {
    use diacraft_layout::Side;
    match n % 4 {
        0 => Side::Up,
        1 => Side::Down,
        2 => Side::Left,
        _ => Side::Right,
    }
}

/// Any style a layout is given, by numbers: a flowchart or a hierarchy of any direction (and alignment), the radial and circular ones, and a compact tree of any two sides.
fn any_style() -> diacraft_layout::Style {
    use diacraft_layout::{Align, Direction, Style};
    let (kind, a, b): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    let dir = match a % 4 {
        0 => Direction::TopToBottom,
        1 => Direction::BottomToTop,
        2 => Direction::LeftToRight,
        _ => Direction::RightToLeft,
    };
    match kind % 5 {
        0 => Style::Flowchart(dir),
        1 => Style::Hierarchy(
            dir,
            match b % 3 {
                0 => Align::Start,
                1 => Align::Center,
                _ => Align::End,
            },
        ),
        2 => Style::CompactTree { run: side_of(a), level: side_of(b) },
        3 => Style::Radial,
        _ => Style::Circular,
    }
}

/// Whether a style is a compact tree that runs along one axis and sets its levels off along the same: what the layout refuses (`Settings::check`), and no call makes.
fn compact_on_one_axis(style: diacraft_layout::Style) -> bool {
    match style {
        diacraft_layout::Style::CompactTree { run, level } => run.is_vertical() == level.is_vertical(),
        _ => false,
    }
}

/// `relayout::place_code` and `style_of_code` (the table of `PlaceStyle`) for **every** style a layout takes (a compact tree that runs along one axis and sets
/// its levels off along the other: the layout refuses the rest) and **every** byte: a style's code is one of 1 to 14 and 16 to 27 and reads back as the style; two such styles have
/// one code only when they are one style; a code of the table is the code of the style it reads as, and any other (0, 15, past 27) reads as a flowchart down the page; and a compact
/// tree along one axis, which no call makes, is given the first compact tree's code.
#[kani::proof]
#[kani::unwind(10)]
fn a_layouts_place_style_code_reads_back_as_its_style_and_every_code_is_one_style() {
    use crate::draw::relayout::{place_code, style_of_code};
    use diacraft_layout::{Direction, Style};
    let style = any_style();
    let code = place_code(style);
    let one_axis = compact_on_one_axis(style);
    if one_axis {
        assert!(code == 7);
    } else {
        assert!((1..=14).contains(&code) || (16..=27).contains(&code));
        assert!(style_of_code(code) == style);
        let other = any_style();
        if !compact_on_one_axis(other) {
            assert!((place_code(other) == code) == (other == style));
        }
    }
    let c: u8 = kani::any();
    let read = style_of_code(c);
    if (1..=14).contains(&c) || (16..=27).contains(&c) {
        assert!(place_code(read) == c);
    } else {
        assert!(read == Style::Flowchart(Direction::TopToBottom));
    }
    let compact = matches!(style, Style::CompactTree { .. });
    kani::cover!(compact & !one_axis & (code == 14), "the last compact tree");
    kani::cover!(code == 27, "the last hierarchy");
    kani::cover!(one_axis, "a compact tree along one axis");
}

/// `relayout::off_breaks` (how far a shape's extent along an axis moves off a page break) for **every** extent of finite ends in order, on a page of
/// three sheets (breaks at 10 and 20, the sheets from 0 to 30, half a unit of margin): no move exactly when no break runs across it; a move puts it across no break and on the sheets;
/// an extent a sheet holds beside the break with its margin is always moved, and one longer than a sheet never is.
#[kani::proof]
#[kani::unwind(4)]
fn a_shape_moved_off_a_page_break_lies_on_one_sheet_and_one_that_fits_on_a_sheet_is_moved() {
    use crate::draw::relayout::{BREAK_TOUCH as TOUCH, off_breaks};
    const BREAKS: [f64; 2] = [10.0, 20.0];
    let (lo, hi): (f64, f64) = (kani::any(), kani::any());
    kani::assume(lo.is_finite() && hi.is_finite() && lo <= hi);
    let across = |a: f64, b: f64| BREAKS.iter().any(|&c| a + TOUCH < c && c < b - TOUCH);
    let got = off_breaks(lo, hi, &BREAKS, (0.0, 30.0), 0.5);
    let was_across = across(lo, hi);
    assert!((got == Some(0.0)) == !was_across);
    if let Some(d) = got
        && d != 0.0
    {
        let (a, b) = (lo + d, hi + d);
        assert!(!across(a, b) && a >= -TOUCH && b <= 30.0 + TOUCH);
    }
    if hi - lo <= 9.4 {
        assert!(got.is_some());
    }
    if was_across && hi - lo > 10.0 + 1.0e-5 {
        assert!(got.is_none());
    }
    let (back, on) = (got.is_some_and(|d| d < 0.0), got.is_some_and(|d| d > 0.0));
    kani::cover!(back, "moved back");
    kani::cover!(on, "moved on");
    kani::cover!(got.is_none() & was_across, "too long for a sheet");
}

/// `shapedata::type_number` (the whole number a `Type` cell's number is) for **every** number: a whole number exactly when the number is finite, has no
/// fraction and is at most 2^53 in size, and then the number itself, nothing cut; and the kind it names (`Kind::from_code`) exactly for 0 to 7, whose code is the number, so never
/// for 2.5, -0.5 or 7.5.
#[kani::proof]
#[kani::unwind(10)]
fn a_type_cells_number_is_a_whole_number_exactly_when_a_double_holds_it_and_a_kind_exactly_from_zero_to_seven() {
    use crate::draw::shapedata::{Kind, type_number};
    let n: f64 = kani::any();
    let got = type_number(n);
    let exact = n.is_finite() && n.abs() <= 9_007_199_254_740_992.0 && (n as i64) as f64 == n;
    assert!(got.is_some() == exact);
    if let Some(k) = got {
        assert!(k as f64 == n);
    }
    let kind = got.and_then(Kind::from_code);
    assert!(kind.is_some() == (n >= 0.0 && n <= 7.0 && n.floor() == n));
    if let Some(k) = kind {
        assert!(f64::from(k.code()) == n);
    }
    kani::cover!(got == Some(9_007_199_254_740_992), "the largest");
    kani::cover!(got.is_none() & (n > 2.0) & (n < 3.0), "a fraction");
    kani::cover!(kind == Some(Kind::Date), "a date");
}

/// `shapedata::calendar_name` (the name of a `Calendar` cell's number) for **every** `u32`: a name exactly for the fourteen numbers of the table,
/// 0 to 13, and none past them.
#[kani::proof]
fn a_calendar_has_a_name_exactly_for_the_numbers_of_the_table() {
    use crate::draw::shapedata::{CALENDARS, calendar_name};
    let code: u32 = kani::any();
    let got = calendar_name(code);
    assert!(got.is_some() == (code <= 13));
    assert!(CALENDARS.len() == 14);
    kani::cover!(got.is_some() & (code == 13), "the last calendar");
    kani::cover!(got.is_none() & (code == 14), "one past it");
}

/// `formatting::Align::from_cell` and `cell` (a paragraph's `HorzAlign`) for **every** number: each of the five alignments reads back from its own number, and
/// every number reads as the alignment whose number it rounds to (a half away from 0), a number past the list, below it, no number and an infinity as left.
#[kani::proof]
#[kani::unwind(7)]
fn a_paragraphs_alignment_reads_back_from_its_number_and_every_number_reads_as_the_one_it_rounds_to() {
    use crate::formatting::Align;
    let n: f64 = kani::any();
    let got = Align::from_cell(n);
    let within = |k: f64| n >= k - 0.5 && n < k + 0.5;
    let want = if within(1.0) {
        Align::Center
    } else if within(2.0) {
        Align::Right
    } else if within(3.0) {
        Align::Justify
    } else if within(4.0) {
        Align::Distributed
    } else {
        Align::Left
    };
    assert!(got == want);
    let pick: u8 = kani::any();
    let a = [Align::Left, Align::Center, Align::Right, Align::Justify, Align::Distributed][usize::from(pick % 5)];
    assert!(Align::from_cell(f64::from(a.cell())) == a);
    kani::cover!((got == Align::Distributed) & (n > 4.0), "a number just above the last's");
    kani::cover!(n.is_nan() & (got == Align::Left), "no number");
}

/// `memfs::admitted` (`admit`'s rule, whether the browser may take a file of a size at all, asked before a byte of it is read) for **every** size, cap of a package file,
/// room of a read and bytes the store holds: taken exactly when the file is within the cap, smaller than the room and fits beside what the store holds within `MAX_BYTES`; refused
/// as too large exactly past the cap, as too big where the room does not hold it and it is `MAX_MEMORY` or more, and as short of memory otherwise.
#[kani::proof]
fn a_file_the_browser_is_asked_to_take_is_taken_exactly_when_the_read_and_the_store_have_room_for_it() {
    use crate::OpenProblem;
    use crate::memfs::{MAX_BYTES, admitted};
    let (size, cap, room, held): (u64, u64, u64, u64) = (kani::any(), kani::any(), kani::any(), kani::any());
    let got = admitted(size, cap, || room, || held);
    let fits_store = u128::from(size) + u128::from(held) <= u128::from(MAX_BYTES);
    assert!(got.is_ok() == (size <= cap && size < room && fits_store));
    match got {
        Ok(()) => {}
        Err(OpenProblem::FileTooLarge) => {
            assert!(size > cap);
        }
        Err(OpenProblem::TooBig) => {
            assert!(size <= cap && size >= room && size >= diacraft_vsdx::MAX_MEMORY);
        }
        Err(OpenProblem::OutOfMemory) => {
            assert!(size <= cap && ((size >= room && size < diacraft_vsdx::MAX_MEMORY) || (size < room && !fits_store)));
        }
        Err(_) => {
            assert!(false, "no other refusal");
        }
    }
    kani::cover!(got.is_ok() & (size > 0), "a file taken");
    kani::cover!((got == Err(OpenProblem::OutOfMemory)) & (size < room), "a store with no room for it");
}

/// `memfs::bytes_after_put` (`put`'s count of the bytes the store holds) for **every** three counts: a count exactly when it is within `MAX_BYTES`, so a store never holds
/// more; from a count within it and an entry it holds, the exact count after the entry gives way to the new bytes.
#[kani::proof]
fn the_bytes_of_the_files_in_memory_are_counted_exactly_and_never_pass_their_cap() {
    use crate::memfs::{MAX_BYTES, bytes_after_put};
    let (held, replaced, new): (u64, u64, u64) = (kani::any(), kani::any(), kani::any());
    let got = bytes_after_put(held, replaced, new);
    if let Some(t) = got {
        assert!(t <= MAX_BYTES);
    }
    if held <= MAX_BYTES && replaced <= held {
        let exact = u128::from(held - replaced) + u128::from(new);
        assert!(got.is_some() == (exact <= u128::from(MAX_BYTES)));
        assert!(got.is_none_or(|t| u128::from(t) == exact));
    }
    kani::cover!(got == Some(MAX_BYTES), "the store full to the byte");
    kani::cover!(got.is_none() & (held <= MAX_BYTES), "a file with no room");
}
