//! What a label's attributes say: the `style` attribute's seven properties, the colours, the sizes of `font`, and a link's address (S-R25).
//!
//! **Only what the table of S-R25 names is read.** In a `style` attribute that is `font-family`, `font-size` (px, pt, and em of the label's size), `color`, `font-weight` (bold at
//! 600 and above, or `bold`), `font-style` (`italic`), `text-decoration` (`underline`, `line-through`) and `text-align` (on a block); every other property is skipped, and a value
//! that is not understood changes nothing. Nothing is evaluated and nothing is fetched: a `url(...)` or an `expression(...)` is a value no property here takes.
//!
//! **Linear.** A `style` is walked once, a piece at a time, and its pieces are read in the length they have: no piece is looked at twice. The label reader charges the bytes of a
//! style before it calls [`apply`].

use std::sync::Arc;

use crate::decode::keep_char;

use super::{Align, Link, Rgb, RunFmt};

/// The longest text read as a colour, in bytes: `rgba(100.000000%, 100.000000%, 100.000000%, 1.000000)` is 53.
const MAX_COLOUR_BYTES: usize = 64;

/// The longest a family name is kept, in characters: a font name is short, and a run's format is compared as often as its text changes.
pub(super) const MAX_FAMILY_CHARS: usize = 128;

/// The longest address of a link that is kept, in bytes: 32 KiB. **A choice of ours**: the address becomes a cell of the model, and a real link is a few hundred bytes.
pub(super) const MAX_LINK_BYTES: usize = 32 * 1024;

/// The sizes in px that `font size` 1 to 7 stand for: HTML maps them to the keywords `x-small` to `xxx-large`, and CSS scales those by 3/4, 8/9, 1, 6/5, 3/2, 2 and 3 of
/// `medium`, which a browser takes as 16 px (S-R25; 14.2 is 14.22 as the specification states it).
const HTML_SIZES: [f64; 7] = [12.0, 14.2, 16.0, 19.2, 24.0, 32.0, 48.0];

/// The size in px of `font size` `size`: 12, 14.2, 16, 19.2, 24, 32 and 48 for 1 to 7, and the nearest end for a size outside them (HTML clamps a legacy size to 1 to 7).
pub fn html_font_px(size: i32) -> f64 {
    let index = usize::try_from(size.clamp(1, 7).saturating_sub(1)).unwrap_or(0);
    HTML_SIZES.get(index).copied().unwrap_or(16.0)
}

/// The number a `font size` attribute gives, by HTML's rules for a legacy font size: white space at the start is skipped; a leading `+` or `-` makes the number relative to 3; the
/// digits that follow are the number and anything after them is ignored; no digit is no size (`None`). The result is not clamped (that is [`html_font_px`]'s).
pub fn legacy_font_size(value: &str) -> Option<i32> {
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0C', '\r']);
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'+') => (1_i64, text.get(1..)?),
        Some(b'-') => (-1_i64, text.get(1..)?),
        _ => (0_i64, text),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    let n = digits.get(..end).filter(|d| !d.is_empty())?;
    let number = n.bytes().fold(0_i64, |acc, b| acc.saturating_mul(10).saturating_add(i64::from(b - b'0')).min(1_000_000));
    let size = if sign == 0 { number } else { 3_i64.saturating_add(sign.saturating_mul(number)) };
    i32::try_from(size).ok()
}

/// The first family of a `font-family` list or of a `face`: the name up to the first comma outside quotes, quotes removed, at most [`MAX_FAMILY_CHARS`] characters; `None` when
/// there is none.
pub(super) fn first_family(value: &str) -> Option<Arc<str>> {
    let text = value.trim();
    let name = match text.chars().next() {
        Some(quote @ ('"' | '\'')) => {
            let inner = text.get(1..)?;
            inner.split(quote).next()?
        }
        _ => text.split(',').next()?,
    };
    // A name is kept with no character that XML cannot hold, so that what the model is given as a font is written in any file.
    let name: String = name.trim().chars().filter(|c| keep_char(*c)).take(MAX_FAMILY_CHARS).collect();
    if name.is_empty() {
        return None;
    }
    Some(Arc::from(name))
}

/// The value without a trailing `!important`.
fn plain_value(value: &str) -> &str {
    let v = value.trim();
    match v.rfind('!') {
        Some(at) if v.get(at.saturating_add(1)..).is_some_and(|rest| rest.trim().eq_ignore_ascii_case("important")) => {
            v.get(..at).unwrap_or(v).trim_end()
        }
        _ => v,
    }
}

/// A CSS length as px: `12px`, `9pt` (4/3 px each) or `1.5em` (of `base_px`); a unit this reader does not take, a number that is not one and a size that is not above 0 give
/// `None`. The result is at most `max_px`.
pub(super) fn length_px(value: &str, base_px: f64, max_px: f64) -> Option<f64> {
    let v = value.trim();
    let split = v.bytes().position(|b| !(b.is_ascii_digit() || b == b'.' || b == b'+')).unwrap_or(v.len());
    let number: f64 = v.get(..split)?.parse().ok()?;
    let factor = match v.get(split..)?.to_ascii_lowercase().as_str() {
        "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "em" => base_px,
        _ => return None,
    };
    let px = number * factor;
    (px.is_finite() && px > 0.0).then(|| px.min(max_px))
}

/// Whether a `font-weight` is bold: `bold` and `bolder`, or a number of 600 and above; `normal`, `lighter` and a number below 600 are not; anything else is no answer.
fn weight_is_bold(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        other => other.parse::<f64>().ok().filter(|w| w.is_finite()).map(|w| w >= 600.0),
    }
}

/// What a `text-align` says, for a block.
fn align_of(value: &str) -> Option<Align> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "start" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" | "end" => Some(Align::Right),
        _ => None,
    }
}

/// What a label's `style` attribute is read against.
pub(super) struct Ctx {
    /// The label's own font size, for `em`.
    pub(super) base_px: f64,
    /// The largest size a run is given.
    pub(super) max_px: f64,
    /// Whether the element is a block, whose `text-align` is the paragraph's.
    pub(super) block: bool,
}

/// Applies the declarations of `style` to `fmt` and, for a block, `align`: the seven properties of S-R25, in order (a later declaration of one wins), and nothing else.
pub(super) fn apply(style: &str, ctx: &Ctx, fmt: &mut RunFmt, align: &mut Option<Align>) {
    for piece in style.split(';') {
        let Some((name, value)) = piece.split_once(':') else { continue };
        let value = plain_value(value);
        if value.is_empty() {
            continue;
        }
        declare(name.trim(), value, ctx, fmt, align);
    }
}

/// One declaration.
fn declare(name: &str, value: &str, ctx: &Ctx, fmt: &mut RunFmt, align: &mut Option<Align>) {
    let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
    if is("font-family") {
        if let Some(family) = first_family(value) {
            fmt.family = Some(family);
        }
    } else if is("font-size") {
        fmt.size_px = length_px(value, ctx.base_px, ctx.max_px).or(fmt.size_px);
    } else if is("color") {
        fmt.color = colour(value).or(fmt.color);
    } else if is("font-weight") {
        fmt.bold = weight_is_bold(value).or(fmt.bold);
    } else if is("font-style") {
        fmt.italic = italic_of(value).or(fmt.italic);
    } else if is("text-decoration") || is("text-decoration-line") {
        decorate(value, fmt);
    } else if is("text-align") && ctx.block {
        *align = align_of(value).or(*align);
    }
}

/// `italic` and `oblique` are italic and `normal` is not; anything else is no answer.
fn italic_of(value: &str) -> Option<bool> {
    let lower = value.to_ascii_lowercase();
    match lower.split_whitespace().next()? {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

/// `underline` and `line-through` turn their decoration on; `none` turns nothing off (a decoration an ancestor draws is drawn through its children).
fn decorate(value: &str, fmt: &mut RunFmt) {
    for word in value.split_whitespace() {
        if word.eq_ignore_ascii_case("underline") {
            fmt.underline = Some(true);
        } else if word.eq_ignore_ascii_case("line-through") {
            fmt.strike = Some(true);
        }
    }
}

/// The colours CSS has by name that this reader knows, as `(name, red, green, blue)`: the sixteen of HTML 4 and `orange`, in order, for a binary search.
/// The style reader's table (S-R17) holds the whole list of CSS's names.
const NAMES: [(&str, u8, u8, u8); 18] = [
    ("aqua", 0, 255, 255),
    ("black", 0, 0, 0),
    ("blue", 0, 0, 255),
    ("fuchsia", 255, 0, 255),
    ("gray", 128, 128, 128),
    ("green", 0, 128, 0),
    ("grey", 128, 128, 128),
    ("lime", 0, 255, 0),
    ("maroon", 128, 0, 0),
    ("navy", 0, 0, 128),
    ("olive", 128, 128, 0),
    ("orange", 255, 165, 0),
    ("purple", 128, 0, 128),
    ("red", 255, 0, 0),
    ("silver", 192, 192, 192),
    ("teal", 0, 128, 128),
    ("white", 255, 255, 255),
    ("yellow", 255, 255, 0),
];

/// The colour a CSS value says: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (the alpha is not read), `rgb()` and `rgba()` (numbers or percentages, separated by commas, spaces or a
/// slash, the alpha not read), and the names of [`NAMES`]. `transparent`, `inherit`, `currentColor` and anything else are no colour.
pub(super) fn colour(value: &str) -> Option<Rgb> {
    let v = value.trim();
    // No colour is written in more than a few dozen characters: a longer value is not looked at.
    if v.len() > MAX_COLOUR_BYTES {
        return None;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return hex_colour(hex);
    }
    let lower = v.to_ascii_lowercase();
    if let Some(args) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")) {
        return function_colour(args.strip_suffix(')')?);
    }
    NAMES.binary_search_by(|(name, ..)| name.cmp(&lower.as_str())).ok().and_then(|i| NAMES.get(i)).map(|(_, r, g, b)| Rgb::new(*r, *g, *b))
}

/// The value of one hexadecimal digit.
fn nibble(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// `rgb`, `rgba`, `rrggbb` or `rrggbbaa` after the `#`.
fn hex_colour(hex: &str) -> Option<Rgb> {
    if hex.len() > 8 {
        return None;
    }
    let digits: Vec<u8> = hex.bytes().map(nibble).collect::<Option<_>>()?;
    let pair = |i: usize| -> Option<u8> { digits.get(i)?.checked_mul(16)?.checked_add(*digits.get(i.checked_add(1)?)?) };
    let one = |i: usize| -> Option<u8> { digits.get(i).map(|d| d.saturating_mul(17)) };
    match digits.len() {
        3 | 4 => Some(Rgb::new(one(0)?, one(1)?, one(2)?)),
        6 | 8 => Some(Rgb::new(pair(0)?, pair(2)?, pair(4)?)),
        _ => None,
    }
}

/// The arguments of `rgb(` or `rgba(`: three numbers, each a count of 0 to 255 or a percentage, and an alpha that is not read.
fn function_colour(args: &str) -> Option<Rgb> {
    let mut parts = args.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty());
    let mut channel = || -> Option<u8> {
        let part = parts.next()?;
        let (number, percent) = match part.strip_suffix('%') {
            Some(number) => (number, true),
            None => (part, false),
        };
        let value: f64 = number.parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        let counted = if percent { value * 255.0 / 100.0 } else { value };
        // Finite times finite is finite or infinite and never NaN, and the clamp puts it in 0 to 255, so the cast loses nothing.
        Some(counted.round().clamp(0.0, 255.0) as u8)
    };
    Some(Rgb::new(channel()?, channel()?, channel()?))
}

/// The address of a link, as S-R25 keeps it: with the white space browsers ignore taken out (the ends, and tabs and line feeds inside, so that `java<tab>script:` is what it is),
/// an `http:`, `https:` or `mailto:` address, or `data:page/id,` and a page's id. `None` for any other scheme, for no scheme, and for an address past [`MAX_LINK_BYTES`].
pub(super) fn link_of(href: &str) -> Option<Link> {
    if href.len() > MAX_LINK_BYTES.saturating_mul(2) {
        return None;
    }
    let cleaned: String = href.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r') && keep_char(*c)).collect();
    if cleaned.len() > MAX_LINK_BYTES {
        return None;
    }
    let scheme_end = cleaned.find(':')?;
    let scheme = cleaned.get(..scheme_end)?.to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" | "mailto" => Some(Link::Url(cleaned)),
        "data" => {
            let rest = cleaned.get(scheme_end.saturating_add(1)..)?;
            let id = rest.get(..8).filter(|p| p.eq_ignore_ascii_case("page/id,")).and_then(|_| rest.get(8..))?;
            (!id.is_empty()).then(|| Link::Page(id.to_owned()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::Script;
    use super::*;

    fn read(style: &str, block: bool) -> (RunFmt, Option<Align>) {
        let mut fmt = RunFmt::default();
        let mut align = None;
        apply(style, &Ctx { base_px: 12.0, max_px: 10_000_000.0, block }, &mut fmt, &mut align);
        (fmt, align)
    }

    #[test]
    fn the_sizes_of_font_1_to_7_are_the_specifications_and_the_rest_are_clamped() {
        let want = [12.0, 14.2, 16.0, 19.2, 24.0, 32.0, 48.0];
        for (n, px) in (1..=7).zip(want) {
            assert_eq!(html_font_px(n), px, "size {n}");
        }
        for n in [i32::MIN, -5, 0] {
            assert_eq!(html_font_px(n), 12.0);
        }
        for n in [8, 9, 100, i32::MAX] {
            assert_eq!(html_font_px(n), 48.0);
        }
    }

    #[test]
    fn a_legacy_size_is_read_by_the_html_rules() {
        let cases: [(&str, Option<i32>); 14] = [
            ("5", Some(5)),
            (" 5", Some(5)),
            ("\n\t 7", Some(7)),
            ("5px", Some(5)),
            ("+1", Some(4)),
            ("-1", Some(2)),
            ("+0", Some(3)),
            ("-9", Some(-6)),
            ("0", Some(0)),
            ("", None),
            ("x", None),
            ("+", None),
            ("-x", None),
            ("99999999999999999999", Some(1_000_000)),
        ];
        for (text, want) in cases {
            assert_eq!(legacy_font_size(text), want, "{text:?}");
        }
    }

    #[test]
    fn a_family_is_the_first_name_with_its_quotes_removed() {
        for (text, want) in [
            ("Arial", Some("Arial")),
            ("Arial, Helvetica", Some("Arial")),
            ("\"Times New Roman\", serif", Some("Times New Roman")),
            ("'Comic Sans MS'", Some("Comic Sans MS")),
            ("  Verdana  ", Some("Verdana")),
            ("\"\"", None),
            ("", None),
            (",Arial", None),
            ("Ar\u{1}ial\u{FFFE}", Some("Arial")),
            ("\u{1}\u{B}", None),
        ] {
            assert_eq!(first_family(text).as_deref(), want, "{text:?}");
        }
        let long = "x".repeat(1000);
        assert_eq!(first_family(&long).map(|f| f.chars().count()), Some(MAX_FAMILY_CHARS));
    }

    #[test]
    fn lengths_are_px_pt_and_em_of_the_base() {
        let px = |v: &str| length_px(v, 12.0, 1000.0);
        assert_eq!(px("20px"), Some(20.0));
        assert_eq!(px("20PX"), Some(20.0));
        assert_eq!(px("9pt"), Some(12.0));
        assert_eq!(px("1.5em"), Some(18.0));
        assert_eq!(px(".5em"), Some(6.0));
        assert_eq!(px("+10px"), Some(10.0));
        for bad in ["", "px", "20", "20%", "20rem", "20vw", "-5px", "0px", "1e3px", "large", "abc", "1..2px", "20 px", "20px20", "infinitypx"] {
            assert_eq!(px(bad), None, "{bad:?}");
        }
        assert_eq!(px("99999px"), Some(1000.0), "past the cap: the cap");
        assert_eq!(length_px("1em", f64::NAN, 1000.0), None, "no size from a base that is no number");
    }

    #[test]
    fn the_seven_properties_are_read_and_the_others_are_not() {
        let (fmt, align) = read(
            "font-family: 'Open Sans', sans-serif; font-size: 20px; color: #00f; font-weight: bold; font-style: italic; text-decoration: underline line-through; text-align: center; \
             position: fixed; background: url(x); margin: 3px; width: 10px; z-index: 5",
            true,
        );
        assert_eq!(
            fmt,
            RunFmt {
                bold: Some(true),
                italic: Some(true),
                underline: Some(true),
                strike: Some(true),
                script: Script::Normal,
                family: Some(Arc::from("Open Sans")),
                size_px: Some(20.0),
                color: Some(Rgb::new(0, 0, 255)),
            }
        );
        assert_eq!(align, Some(Align::Center));
    }

    #[test]
    fn a_text_align_on_what_is_no_block_is_not_read() {
        assert_eq!(read("text-align: right", false).1, None);
        assert_eq!(read("text-align: right", true).1, Some(Align::Right));
        for (v, want) in
            [("left", Some(Align::Left)), ("START", Some(Align::Left)), ("end", Some(Align::Right)), ("justify", None), ("", None), ("middle", None)]
        {
            assert_eq!(read(&format!("text-align:{v}"), true).1, want, "{v}");
        }
    }

    #[test]
    fn weights_are_bold_from_600_and_normal_unbolds() {
        for (v, want) in [
            ("bold", Some(true)),
            ("BOLD", Some(true)),
            ("bolder", Some(true)),
            ("600", Some(true)),
            ("700", Some(true)),
            ("1000", Some(true)),
            ("599", Some(false)),
            ("400", Some(false)),
            ("normal", Some(false)),
            ("lighter", Some(false)),
            ("heavy", None),
            ("NaN", None),
            ("inf", None),
            ("1e3", Some(true)),
        ] {
            assert_eq!(read(&format!("font-weight:{v}"), false).0.bold, want, "{v}");
        }
    }

    #[test]
    fn a_value_that_is_not_understood_changes_nothing_and_a_later_one_wins() {
        let (fmt, _) = read(
            "color: red; color: not-a-colour; font-size: 10px; font-size: 3 px; font-weight: bold; font-weight: ; font-style: italic; font-style: sideways",
            false,
        );
        assert_eq!(fmt.color, Some(Rgb::new(255, 0, 0)));
        assert_eq!(fmt.size_px, Some(10.0));
        assert_eq!(fmt.bold, Some(true));
        assert_eq!(fmt.italic, Some(true));
        let (fmt, _) = read("color: red; color: blue; font-style: italic; font-style: normal; text-decoration: none", false);
        assert_eq!((fmt.color, fmt.italic, fmt.underline), (Some(Rgb::new(0, 0, 255)), Some(false), None), "none turns nothing off");
    }

    #[test]
    fn important_and_odd_spacing_and_case_are_read() {
        let (fmt, _) = read("  COLOR :  RED !important ;FONT-SIZE:2em!IMPORTANT;;;:;x;font-weight", false);
        assert_eq!((fmt.color, fmt.size_px), (Some(Rgb::new(255, 0, 0)), Some(24.0)));
    }

    #[test]
    fn colours_are_hex_rgb_and_the_names_known() {
        for (text, want) in [
            ("#f00", Some((255, 0, 0))),
            ("#F00F", Some((255, 0, 0))),
            ("#ff8000", Some((255, 128, 0))),
            ("#ff800080", Some((255, 128, 0))),
            ("rgb(255, 0, 0)", Some((255, 0, 0))),
            ("RGB(1,2,3)", Some((1, 2, 3))),
            ("rgba(10, 20, 30, 0.5)", Some((10, 20, 30))),
            ("rgb(100% 0% 50%)", Some((255, 0, 128))),
            ("rgb(0 0 0 / 50%)", Some((0, 0, 0))),
            ("rgb(300, -4, 7.6)", Some((255, 0, 8))),
            ("Orange", Some((255, 165, 0))),
            ("navy", Some((0, 0, 128))),
            ("grey", Some((128, 128, 128))),
            ("#ff", None),
            ("#ggg", None),
            ("#fffff", None),
            ("#fffffffff", None),
            ("#", None),
            ("rgb(1,2)", None),
            ("rgb(1,2,x)", None),
            ("rgb(1,2,3", None),
            ("transparent", None),
            ("currentColor", None),
            ("inherit", None),
            ("", None),
            ("rgb(NaN, 1, 2)", None),
        ] {
            assert_eq!(colour(text), want.map(|(r, g, b)| Rgb::new(r, g, b)), "{text:?}");
        }
    }

    #[test]
    fn the_names_are_sorted_for_the_search() {
        assert!(NAMES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn a_link_is_kept_for_four_forms_and_for_nothing_else() {
        let url = |s: &str| Some(Link::Url(s.to_owned()));
        assert_eq!(link_of("https://example.com/a?b=c#d"), url("https://example.com/a?b=c#d"));
        assert_eq!(link_of("HTTP://example.com"), url("HTTP://example.com"));
        assert_eq!(link_of("mailto:a@b.c"), url("mailto:a@b.c"));
        assert_eq!(link_of("data:page/id,p2"), Some(Link::Page("p2".to_owned())));
        assert_eq!(link_of("DATA:PAGE/ID,x y"), Some(Link::Page("x y".to_owned())));
        assert_eq!(link_of("  https://a.b  "), url("https://a.b"), "white space at the ends goes");
        assert_eq!(link_of("ht\ttps://a\n.b"), url("https://a.b"), "tabs and line feeds inside go, as a browser takes them out");
        assert_eq!(link_of("https://a.b/\u{1}x\u{FFFE}"), url("https://a.b/x"), "and a character XML cannot hold");
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "\u{1}javascript:alert(1)",
            "vbscript:x",
            "data:text/html,<script>",
            "data:action/json,{}",
            "data:page/id,",
            "data:page/id",
            "file:///etc/passwd",
            "ftp://x",
            "//example.com",
            "/relative",
            "example.com",
            "",
            ":",
        ] {
            assert_eq!(link_of(bad), None, "{bad:?}");
        }
        assert_eq!(link_of(&format!("https://x/{}", "a".repeat(MAX_LINK_BYTES))), None, "past the cap");
        assert_eq!(link_of(&format!("https://x/{}", "a".repeat(MAX_LINK_BYTES * 2))), None);
    }
}
