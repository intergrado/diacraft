//! The grammar of a style string (S-R1), in one pass.
//!
//! A style is split at every `;`. A piece holding `=` is a pair: the key is everything before the first `=`, the value everything after it (so a value may hold `=`, as the padding of
//! Base64 does). A piece with no `=` is the name of a named style. An empty piece is nothing, so a trailing `;` is optional, and a megabyte of `;` is a million empty pieces and no
//! more. **Nothing is trimmed and case counts**: ` fillColor` with a leading space is another key. The pieces are applied left to right, each over what came before.

/// One piece of a style string: its text and where it starts in the string, so that a value can be named by its place (a [`Span`](super) of the string) and never copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece<'a> {
    /// What lies between two `;`, as written.
    pub text: &'a str,
    /// The byte of the style string the piece starts at.
    pub offset: usize,
}

impl<'a> Piece<'a> {
    /// The key and the value of a piece that holds `=`: the key is what comes before the first one, the value all that comes after it. `None` for a piece with no `=`, which names a
    /// style.
    pub fn pair(&self) -> Option<(&'a str, &'a str)> {
        self.text.split_once('=')
    }
}

/// The pieces of `style`, in order, empty ones included (a style of `n` bytes has at most `n + 1`, and the walk is one pass).
pub fn pieces(style: &str) -> Pieces<'_> {
    Pieces { rest: Some(style), offset: 0 }
}

/// The pieces of a style string ([`pieces`]).
#[derive(Clone, Debug)]
pub struct Pieces<'a> {
    rest: Option<&'a str>,
    offset: usize,
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        let rest = self.rest?;
        let offset = self.offset;
        let (text, after) = match rest.split_once(';') {
            Some((text, after)) => (text, Some(after)),
            None => (rest, None),
        };
        self.rest = after;
        // The piece and its `;`: the next piece starts one byte after the text, and past the end of the string there is none.
        self.offset = offset.saturating_add(text.len()).saturating_add(1);
        Some(Piece { text, offset })
    }
}
