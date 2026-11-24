//! A master's icon: base64 text in the masters index, looked at when a listing is made and decoded when it is
//! asked for.
//!
//! Icons are the file's, so they are hostile like the rest: the text is looked at only if it is short enough to be an icon (a text past
//! [`MAX_ICON_TEXT`] characters is refused without a pass over it, so a file whose every master holds megabytes of it costs the listing nothing),
//! the decoded size is worked out from the letters and never allocated before it is known to be within [`MAX_ICON_BYTES`], and the window is
//! told the kind of picture from its first bytes. The file's `IconSize` attribute says nothing here and is never read.

/// The most bytes a decoded icon may hold and still be listed or answered. Real icons are a few kilobytes; this is ours.
pub const MAX_ICON_BYTES: usize = 256 * 1024;

/// The most characters of base64 text (white space included) looked at: a little over what [`MAX_ICON_BYTES`] takes to write.
pub const MAX_ICON_TEXT: usize = 400_000;

/// What kind of picture the first bytes of an icon say it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconFormat {
    Ico,
    Png,
    Bmp,
    Gif,
    Jpeg,
    Emf,
    Wmf,
    /// Bytes that none of the above begin with: the window decides whether it can draw them.
    Unknown,
}

impl IconFormat {
    /// A fixed word, as it is written in an answer: `ico`, `png`, `bmp`, `gif`, `jpeg`, `emf`, `wmf` or `unknown`.
    pub fn word(self) -> &'static str {
        match self {
            IconFormat::Ico => "ico",
            IconFormat::Png => "png",
            IconFormat::Bmp => "bmp",
            IconFormat::Gif => "gif",
            IconFormat::Jpeg => "jpeg",
            IconFormat::Emf => "emf",
            IconFormat::Wmf => "wmf",
            IconFormat::Unknown => "unknown",
        }
    }

    fn of(head: &[u8]) -> IconFormat {
        if head.starts_with(&[0, 0, 1, 0]) {
            IconFormat::Ico
        } else if head.starts_with(&[0x89, b'P', b'N', b'G']) {
            IconFormat::Png
        } else if head.starts_with(b"BM") {
            IconFormat::Bmp
        } else if head.starts_with(b"GIF8") {
            IconFormat::Gif
        } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            IconFormat::Jpeg
        } else if head.starts_with(&[1, 0, 0, 0]) && head.get(40..44) == Some(b" EMF") {
            IconFormat::Emf
        } else if head.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
            IconFormat::Wmf
        } else {
            IconFormat::Unknown
        }
    }
}

/// Why an icon is not listed or answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconProblem {
    /// More text or more bytes than an icon may hold.
    TooLarge,
    /// Text that is not base64.
    NotBase64,
    /// Not looked at: the stencil's icons together are more text than a listing reads ([`MAX_ICON_SCAN`]).
    Skipped,
}

impl IconProblem {
    /// The words an answer gives for it: `too large` or `not base64`.
    pub fn words(self) -> &'static str {
        match self {
            IconProblem::TooLarge => "too large",
            IconProblem::NotBase64 => "not base64",
            IconProblem::Skipped => "not read, since the stencil holds more icon text than is read",
        }
    }
}

/// What an icon is, found without decoding it whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconInfo {
    pub format: IconFormat,
    /// The size of the picture once decoded.
    pub bytes: usize,
}

/// One base64 letter's value; `None` for any other character.
fn sextet(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Reads `text` as standard base64 with its padding, white space (space, tab, line breaks) anywhere between letters. Calls `put` with each
/// decoded byte, in order, until it says stop (`false`). → the number of bytes the whole text decodes to, or why it is not an icon. The pass is
/// one over the text and holds nothing.
fn scan(text: &str, mut put: impl FnMut(u8) -> bool) -> Result<usize, IconProblem> {
    if text.len() > MAX_ICON_TEXT {
        return Err(IconProblem::TooLarge);
    }
    let (mut acc, mut bits) = (0u32, 0u32);
    let (mut letters, mut pads) = (0usize, 0usize);
    let mut stopped = false;
    for &c in text.as_bytes() {
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
            continue;
        }
        if c == b'=' {
            pads += 1;
            continue;
        }
        let Some(v) = sextet(c) else { return Err(IconProblem::NotBase64) };
        // A letter after padding, or more than two paddings, is not base64.
        if pads > 0 {
            return Err(IconProblem::NotBase64);
        }
        letters += 1;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if !stopped && !put(((acc >> bits) & 0xFF) as u8) {
                stopped = true;
            }
            acc &= (1 << bits) - 1;
        }
    }
    if pads > 2 || (letters + pads) % 4 != 0 || letters == 0 {
        return Err(IconProblem::NotBase64);
    }
    let bytes = letters.saturating_mul(6) / 8;
    if bytes > MAX_ICON_BYTES {
        return Err(IconProblem::TooLarge);
    }
    Ok(bytes)
}

/// The most characters of icon text one listing reads, all the icons of a stencil together: the passes over them are charged to this, so a
/// stencil of thousands of icons each just under [`MAX_ICON_TEXT`] costs a few tens of milliseconds and not more.
pub const MAX_ICON_SCAN: usize = 32 * 1024 * 1024;

/// What the icon text is: its kind and its decoded size, from one pass over the text and no allocation but the first bytes.
pub fn inspect(text: &str) -> Result<IconInfo, IconProblem> {
    let mut head: Vec<u8> = Vec::with_capacity(48);
    let bytes = scan(text, |b| {
        head.push(b);
        head.len() < 48
    })?;
    Ok(IconInfo { format: IconFormat::of(&head), bytes })
}

/// The icon's bytes, for a window that draws it.
pub fn decode(text: &str) -> Result<(IconFormat, Vec<u8>), IconProblem> {
    // Checked first, so that the buffer is reserved only for an icon that is within the cap.
    let info = inspect(text)?;
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(info.bytes).map_err(|_| IconProblem::TooLarge)?;
    scan(text, |b| {
        out.push(b);
        true
    })?;
    Ok((info.format, out))
}

/// The text without its white space, as it is answered.
pub fn compact(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n')).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn base64_is_read_with_its_padding_and_white_space_and_nothing_else() {
        // "Man" "Ma" "M"
        assert_eq!(decode("TWFu").unwrap().1, b"Man");
        assert_eq!(decode("TWE=").unwrap().1, b"Ma");
        assert_eq!(decode("TQ==").unwrap().1, b"M");
        assert_eq!(decode(" TW\r\nFu\t").unwrap().1, b"Man");
        for bad in ["", "====", "TWF", "TW=u", "TWFu=", "TQ===", "TW Fu!", "TWF\u{e9}", "T\0Fu"] {
            assert_eq!(inspect(bad), Err(IconProblem::NotBase64), "{bad:?}");
        }
    }

    #[test]
    fn the_kind_is_the_first_bytes() {
        let png = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        let enc = |bytes: &[u8]| {
            const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in bytes.chunks(3) {
                let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
                for k in 0..4 {
                    s.push(if k <= c.len() { A[((n >> (18 - 6 * k)) & 63) as usize] as char } else { '=' });
                }
            }
            s
        };
        assert_eq!(inspect(&enc(&png)).unwrap(), IconInfo { format: IconFormat::Png, bytes: 8 });
        assert_eq!(inspect(&enc(&[0, 0, 1, 0, 1, 0])).unwrap().format, IconFormat::Ico);
        assert_eq!(inspect(&enc(b"BM12")).unwrap().format, IconFormat::Bmp);
        assert_eq!(inspect(&enc(b"plain")).unwrap().format, IconFormat::Unknown);
        let mut emf = vec![1, 0, 0, 0];
        emf.resize(40, 0);
        emf.extend_from_slice(b" EMF");
        assert_eq!(inspect(&enc(&emf)).unwrap().format, IconFormat::Emf);
    }

    #[test]
    fn an_icon_past_the_caps_is_refused_before_it_is_read_or_allocated() {
        let long = "A".repeat(MAX_ICON_TEXT + 4);
        assert_eq!(inspect(&long), Err(IconProblem::TooLarge));
        // Under the text cap and over the byte cap: 349,524 letters decode to 262,143 bytes; 349,528 to 262,146.
        let over = "A".repeat(4 * (MAX_ICON_BYTES / 3 + 1));
        assert_eq!(inspect(&over), Err(IconProblem::TooLarge));
        assert_eq!(decode(&over), Err(IconProblem::TooLarge));
        let fits = "A".repeat(4 * (MAX_ICON_BYTES / 3));
        assert_eq!(inspect(&fits).unwrap().bytes, MAX_ICON_BYTES / 3 * 3);
    }
}
