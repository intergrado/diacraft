//! Kani proof harnesses for the number and byte code of the window: when a notice goes (`notice::ends_at`), how tall a submenu's list may be
//! (`ribbon::controls::submenu_height`), the colour of a template's tile (`new_page::parse_hex`), the art of a line end (`ribbon::line_ends::art`)
//! and the width of a strength's slider (`snap_dialog::slider_width`); and, where an arrow key takes the AutoConnect flyout
//! (`edit::autoconnect::tile_after`), the start splash's timer and size (`splash::Splash::tick`, `ended`, `panel_size`), and the macro-enabled twin of a
//! VSDX type (`save_kinds::macro_twin`); and, a gradient stop's opacity and a percent's hundredths (`format_fx::alpha_of`, `hundredths`);
//! and, the splash's size in points at every density and the picture it is drawn from on a dense screen (`panel_size`, `drawn_from_cover`). Run with
//! `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved:** that the splash panel lies inside the window with its margin (`panel_size`: a division and a product taken back to the window, 102 s and 192 s in
//! two forms, where its laws here take half a second; the unit and window tests of `splash.rs` hold it).
//!
//! **Not proved:** that two percents in order have hundredths in order (`hundredths` twice: two products and two roundings, the second circuit, past 300 s);
//! `comments_ui::age_of` (`split_once`, the memchr trap); the number boxes of the Define Shape Data dialog (`Draft::json`: took them out while the batch ran);
//! `font_list::tidy` (names sorted and deduplicated, strings); the `new_page::number_words` (`format!` of a number), `Legs::nudge_target` and `more_rows`
//! (strings and structs).
//!
//! **Not proved:** `Arrow::on_knob` and `grab_at` (a square root of a sum of two squares against a reach, which Kani's model of `sqrt` does not
//! settle, and the order of two tests; took the circle handle and the two functions out while the batch ran); `edit::draw::push_point` and `region::push_point` (the bound is one comparison of a
//! stroke's length, and a stroke of `MOST_POINTS` points is a vector of that many symbolic points); `edit::geom::box_inside` (comparisons, as the engine's `select::box_inside` proved
//! there); the cleaners of the build's release and year (`apps/diacraft/build_info.rs`, a module of the build scripts, and `trim`, `parse` and `jiff`).
//!
//! **Not proved here**: the grid of the template tiles (the columns a width holds, `new_page.rs`), which is worked inside the painting of the page and is no function of
//! its own; `ribbon::design::strip_tiles`, a `const fn` of a flag and an enum of many variants with three answers, which its unit test enumerates.

use std::time::Duration;

use crate::new_page::parse_hex;
use crate::notice::{LIFETIME, ends_at};
use crate::ribbon::controls::submenu_height;
use crate::ribbon::line_ends::art;
use crate::snap_dialog::slider_width;

/// `ends_at`: for a notice shown at **any** time, it goes [`LIFETIME`] (8 s) later, never earlier than it was shown, and at the end of time (`Duration::MAX`) rather than
/// wrapping when 8 s more is past it.
#[kani::proof]
fn a_notice_goes_eight_seconds_after_it_is_shown_or_at_the_end_of_time() {
    // Any time a `Duration` holds: whole seconds and the nanoseconds under one (`Duration::new` of more carries them, and panics past the last second).
    let nanos: u32 = kani::any();
    kani::assume(nanos < 1_000_000_000);
    let shown_at = Duration::new(kani::any(), nanos);
    let ends = ends_at(shown_at);
    assert!(ends >= shown_at);
    match shown_at.checked_add(LIFETIME) {
        Some(later) => {
            assert!(ends == later && ends - shown_at == Duration::from_secs(8));
        }
        None => {
            assert!(ends == Duration::MAX);
        }
    }
    kani::cover!(ends == Duration::MAX, "a notice shown at the end of time");
}

/// `submenu_height`, for a window of **every** height an `f32` holds, NaN and both infinities included: the list is a finite number of points, never shorter than four rows
/// (112), and as tall as the window less its margin (24) wherever that is a finite number of 112 or more.
#[kani::proof]
fn a_submenus_list_is_the_window_less_its_margin_and_never_under_four_rows() {
    let window_h: f32 = kani::any();
    // The window less its margin: an infinity less a number is the infinity, which is no height; Kani's NaN check stops only at a NaN made from numbers, and none is.
    let room = window_h - 24.0;
    let h = submenu_height(window_h);
    assert!(h.is_finite() && h >= 112.0);
    if room.is_finite() && room >= 112.0 {
        assert!(h == room);
    } else {
        assert!(h == 112.0);
    }
    kani::cover!(window_h.is_nan(), "a height that is no number");
    kani::cover!(h > 112.0, "a window taller than four rows and the margin");
}

/// The characters a tile's colour is drawn from: the hash, digits and letters at both ends of what is hexadecimal (`0`, `9`, `a`, `f`, `A`, `F`), the letter past it (`g`), a blank, and
/// the two bytes of `é` (a character of two bytes, which a pair of digits cannot be cut from). What `parse_hex` makes of a text depends only on these kinds and the values of the
/// digits, so a text of them in each place stands for every text of seven bytes.
const HEX_KINDS: &[u8; 12] = b"#09afAFg \xC3\xA9+";

/// The value of a hexadecimal digit, by the harness's own table.
fn digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `parse_hex`, for **every** text of seven bytes by its kinds: the colour is there exactly when the text is `#` and six hexadecimal digits, each byte of it the value of its
/// pair; every other text, one with a sign or a character of two bytes among the digits included, is none.
#[kani::proof]
#[kani::unwind(9)]
fn a_tiles_colour_is_read_exactly_from_a_hash_and_six_hexadecimal_digits() {
    let kinds: [u8; 7] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < HEX_KINDS.len()));
    let raw: [u8; 7] = kinds.map(|k| HEX_KINDS[usize::from(k)]);
    let Ok(text) = std::str::from_utf8(&raw) else { return };
    let digits: Option<Vec<u8>> = raw[1..].iter().map(|b| digit(*b)).collect();
    let well_formed = raw[0] == b'#' && digits.is_some();
    let got = parse_hex(text);
    assert!(got.is_some() == well_formed);
    if let (Some((r, g, b)), Some(d)) = (got, digits) {
        assert!(r == d[0] * 16 + d[1] && g == d[2] * 16 + d[3] && b == d[4] * 16 + d[5]);
    }
    kani::cover!(got.is_some(), "a colour");
    kani::cover!(got.is_none() & (raw[0] == b'#'), "a hash with something else than six digits");
}

/// `parse_hex` of a text of any other length than seven bytes is none, whatever it holds: a harness for six and one for eight.
#[kani::proof]
#[kani::unwind(10)]
fn a_tiles_colour_of_six_or_eight_bytes_is_none() {
    let six: [u8; 6] = kani::any();
    let eight: [u8; 8] = kani::any();
    kani::assume(six.iter().chain(eight.iter()).all(u8::is_ascii));
    if let (Ok(six), Ok(eight)) = (std::str::from_utf8(&six), std::str::from_utf8(&eight)) {
        assert!(parse_hex(six).is_none() && parse_hex(eight).is_none());
    }
}

/// `line_ends::art`, for **every** line end number: a picture exactly for the 45 line ends 1 to 45, none for 0 (no mark) and past 45, and the shaft of a mark stops between
/// its tip and six units back from it.
#[kani::proof]
#[kani::unwind(8)]
fn a_line_end_has_a_picture_exactly_from_one_to_forty_five() {
    let number: u8 = kani::any();
    match art(number) {
        Some(picture) => {
            assert!((1..=45).contains(&number));
            let stop = picture.stop();
            assert!(stop.is_finite() && (0.0..=6.0).contains(&stop));
            // Forgotten and not dropped: the drop of the pieces of 45 pictures, each a list of lists, is what ran the solver out of memory.
            std::mem::forget(picture);
        }
        None => {
            assert!(number == 0 || number > 45);
        }
    }
    kani::cover!(number == 45, "the last line end");
}

/// `slider_width`, for a dialog of **any** finite width and gap: the slider is never narrower than 60 points and never a NaN (no width and gap make the
/// row's sum one: Kani's NaN check would stop at it), and it is 60 in a dialog with no room past its label (76), its number box (46) and its gaps.
///
/// **Not proved: that a wider row's slider is what the row leaves to the last bit.** The harness would work the row's sum again beside the code's, the second circuit of
/// `docs/development.md` (Proofs): it took 121 s, where this one takes under a second.
#[kani::proof]
fn a_strength_slider_is_never_under_sixty_points_or_a_nan() {
    let (width, gap): (f32, f32) = (kani::any(), kani::any());
    kani::assume(width.is_finite() && gap.is_finite());
    let slider = slider_width(width, gap);
    assert!(!slider.is_nan() && slider >= 60.0);
    if width <= 182.0 && gap >= 0.0 {
        assert!(slider == 60.0);
    }
    kani::cover!(slider > 60.0, "a dialog wide enough for a long slider");
    kani::cover!(slider == f32::INFINITY, "a gap so far below 0 that the slider is past every number");
}

// ---: the keys of the AutoConnect flyout and the start splash's timer and size ---

/// `edit::autoconnect::tile_after` (where an arrow key takes the keyboard in the flyout's strip of tiles) for **every** tile, count and key of the five it is given
/// (the four arrows and another): right and down go on one, left and up go back one, and any other key stays; the ends stay, so **a tile in the strip stays in it**, and no press moves
/// more than one tile. It never wraps at the largest place.
#[kani::proof]
fn an_arrow_key_takes_the_flyout_one_tile_on_or_back_and_never_out_of_the_strip() {
    use crate::edit::autoconnect::tile_after;
    use egui::Key;
    let (i, count, pick): (usize, usize, u8) = (kani::any(), kani::any(), kani::any());
    let key = match pick {
        0 => Key::ArrowRight,
        1 => Key::ArrowDown,
        2 => Key::ArrowLeft,
        3 => Key::ArrowUp,
        _ => Key::Enter,
    };
    let got = tile_after(i, count, key);
    let want = match pick {
        0 | 1 if i < usize::MAX && i + 1 < count => i + 1,
        2 | 3 if i > 0 && i - 1 < count => i - 1,
        _ => i,
    };
    assert!(got == want);
    if i < count {
        assert!(got < count);
    }
    assert!(got.abs_diff(i) <= 1);
    kani::cover!((pick == 0) & (i.checked_add(1) == Some(count)) & (got == i), "the last tile stays on a right arrow");
    kani::cover!((pick == 3) & (i.checked_sub(1) == Some(got)), "an up arrow goes back one");
}

/// `splash::Splash::tick`, `left` and `ended` (the start splash goes after `SHOWN_FOR` of the window's clock while the pointer is not over it) for **every** three frames at
/// any whole second of the clock's first 18 hours, in any order, with the pointer over the panel or not at each: the time left is `SHOWN_FOR` less the time from each frame to the next
/// **after a frame at which the pointer was not over it** (a time that went back counts none), and never less than none, so it never grows; the splash has ended exactly when none
/// is left; and the About panel, which has no timer, never ends. (Times to the nanosecond ran past 300 s, at any time and in the first 18 hours alike: each time of the law is a
/// product of 128 bits, `as_nanos`.)
#[kani::proof]
#[kani::unwind(4)]
fn the_start_splash_goes_when_its_time_is_used_up_by_frames_with_the_pointer_away_from_it() {
    use crate::splash::{SHOWN_FOR, Splash};
    let mut start = Splash::start();
    let mut about = Splash::about();
    let mut left = SHOWN_FOR.as_secs();
    let mut seen: Option<u64> = None;
    let mut held = false;
    for _ in 0..3 {
        let (secs, over): (u16, bool) = (kani::any(), kani::any());
        let now = u64::from(secs);
        let before = start.left();
        if let Some(then) = seen
            && !held
        {
            left = left.saturating_sub(now.saturating_sub(then));
        }
        seen = Some(now);
        held = over;
        start.tick(Duration::from_secs(now), over);
        about.tick(Duration::from_secs(now), over);
        assert!(start.left() == Duration::from_secs(left) && start.left() <= before);
        assert!(start.ended() == (left == 0));
        assert!(!about.ended());
    }
    kani::cover!(start.ended(), "a splash whose time is up");
    kani::cover!(!start.ended() & (left < SHOWN_FOR.as_secs()), "a splash with time used and some left");
}

/// The splash's size in points at every density, and the splash copy's width in pixels.
const ART: (f32, f32) = (836.0, 470.0);

/// `splash::panel_size` (the panel's size in a window, the same at every pixel density) for a window of **every** size, a number or not: never a size that is no
/// number, always some size, never wider or taller than the picture's size in points (never scaled up), the picture's size exactly in a window with room for it and its margin (16 points
/// each side), and smaller than the picture in a window without that room. **Not proved here: that the panel lies inside the window with its margin** (the unit and window tests of
/// `splash.rs` hold it): that is a division and a product taken back to the window, which Kani settles slowly (measured on 0.68.0, 2026-11-24: 102 s stated against the room worked again,
/// 192 s as one sum against the window, where this harness takes half a second).
#[kani::proof]
fn the_splash_panel_is_its_pictures_size_in_points_or_smaller_to_fit_in_every_window_and_never_no_number() {
    use crate::splash::panel_size;
    let (w, h): (f32, f32) = (kani::any(), kani::any());
    let got = panel_size(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h)));
    assert!(got.x.is_finite() && got.y.is_finite() && got.x > 0.0 && got.y > 0.0);
    assert!(got.x <= ART.0 && got.y <= ART.1);
    if w >= ART.0 + 32.0 && h >= ART.1 + 32.0 {
        assert!(got.x == ART.0 && got.y == ART.1);
    }
    if w < ART.0 + 32.0 || h < ART.1 + 32.0 {
        assert!(got.x < ART.0 && got.y < ART.1);
    }
    kani::cover!(got.x < ART.0 / 2.0, "a window that holds half the picture");
    kani::cover!((got.x == ART.0) & (got.y == ART.1), "a window with room for it");
}

/// `splash::drawn_from_cover` (the panel is drawn from the cover where it is more pixels wide than the splash copy) for the panel `panel_size` gives in a window of **every**
/// size and at **every** density, a number or not: at one pixel to the point or less, at none, and at a density that is no number, the copy (whose pixels the panel never exceeds there); at
/// its full size above one pixel to the point, the cover.
#[kani::proof]
fn the_splash_is_drawn_from_the_cover_exactly_above_one_pixel_to_the_point_at_its_full_size_and_never_at_one_or_less() {
    use crate::splash::{drawn_from_cover, panel_size};
    let (w, h, ppp): (f32, f32, f32) = (kani::any(), kani::any(), kani::any());
    let size = panel_size(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h)));
    let cover = drawn_from_cover(size.x, ppp);
    if !(ppp > 1.0) {
        assert!(!cover);
    }
    if size.x == ART.0 {
        assert!(cover == (ppp > 1.0));
    }
    kani::cover!(cover & (size.x < ART.0), "a panel shrunk to fit on a dense screen, still more pixels than the copy");
    kani::cover!(!cover & (size.x < ART.0) & (ppp > 1.0), "a panel shrunk to fewer pixels than the copy on a dense screen");
    kani::cover!(cover & (size.x == ART.0), "the panel at its full size on a dense screen");
}

/// `save_kinds::macro_twin` (the macro-enabled type the Save As dialog offers beside a VSDX type) for **every** extension of four bytes that is text: `vsdm`, `vssm`
/// and `vstm` exactly for `vsdx`, `vssx` and `vstx` in any ASCII case, nothing for every other text, and a twin has no twin.
#[kani::proof]
#[kani::unwind(6)]
fn a_vsdx_type_has_its_macro_enabled_twin_in_any_case_and_a_twin_has_none() {
    use crate::save_kinds::macro_twin;
    let raw: [u8; 4] = kani::any();
    let Ok(text) = std::str::from_utf8(&raw) else { return };
    let got = macro_twin(text);
    let named = |word: &[u8; 4]| raw.iter().zip(word).all(|(b, w)| b.to_ascii_lowercase() == *w);
    let want = if named(b"vsdx") {
        Some("vsdm")
    } else if named(b"vssx") {
        Some("vssm")
    } else if named(b"vstx") {
        Some("vstm")
    } else {
        None
    };
    assert!(got == want);
    assert!(got.is_none_or(|twin| macro_twin(twin).is_none()));
    kani::cover!(got == Some("vstm"), "a template's twin");
}

/// `format_fx::alpha_of` (the opacity a gradient stop's transparency is drawn with) for **every** number: fully opaque for no number, an infinity and every
/// transparency of 0 or below, clear from 100 up, and in between half way at 50 (127.5 rounded up).
#[kani::proof]
fn a_stops_opacity_is_full_for_no_transparency_and_none_from_a_hundred_percent() {
    use crate::format_fx::alpha_of;
    let t: f64 = kani::any();
    let a = alpha_of(t);
    if !t.is_finite() || t <= 0.0 {
        assert!(a == 255);
    }
    if t.is_finite() && t >= 100.0 {
        assert!(a == 0);
    }
    if t > 0.0 && t < 100.0 {
        assert!(a <= 255);
    }
    assert!(alpha_of(50.0) == 128);
    kani::cover!((a > 0) & (a < 255), "a stop seen through");
}

/// The same `alpha_of` for **every** two transparencies: the more see-through a stop, the less opaque it is drawn, never more.
#[kani::proof]
fn a_more_transparent_stop_is_never_drawn_more_opaque() {
    use crate::format_fx::alpha_of;
    let (t, u): (f64, f64) = (kani::any(), kani::any());
    kani::assume(t.is_finite() && u.is_finite() && t <= u);
    assert!(alpha_of(u) <= alpha_of(t));
    kani::cover!(alpha_of(u) < alpha_of(t), "less opaque");
}

/// `format_fx::hundredths` (a percent as whole hundredths, to compare a pane's field with the cell) for **every** number: 0 for no number and the infinities, never a
/// panic (the cast is held to the ends of an `i64`), of the percent's sign, 0 for a percent under four thousandths either way, and the largest from 1E17 up. (That two percents
/// in order have hundredths in order ran past 300 s: two products and two roundings, the second circuit.)
#[kani::proof]
fn hundredths_of_a_percent_have_its_sign_and_none_for_no_number() {
    use crate::format_fx::hundredths;
    let p: f64 = kani::any();
    let h = hundredths(p);
    if !p.is_finite() {
        assert!(h == 0);
    }
    if p >= 0.0 {
        assert!(h >= 0);
    }
    if p <= 0.0 {
        assert!(h <= 0);
    }
    if p.abs() < 0.004 {
        assert!(h == 0);
    }
    if p >= 1.0e17 && p.is_finite() {
        assert!(h == i64::MAX);
    }
    assert!(hundredths(35.0) == 3500);
    kani::cover!((h > 0) & (h < 100), "under one percent");
    kani::cover!(h == i64::MIN, "the least");
}
