//! Strings, colours and the braced GUID of `USE`.
//!
//! There is no error literal: R55 says an error value has no literal and only arises from evaluation, so a `#` that
//! is not a colour is [`ParseErrorKind::BadHash`].

use std::sync::Arc;

use super::{Node, Parser, Res, is_name_char};
use crate::{Expr, ParseErrorKind};

/// What is between the braces of a GUID: eight, four, four, four and twelve hexadecimal digits with a hyphen between.
const GUID_BODY: usize = 36;
/// Where the hyphens of a GUID's body are.
const GUID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

impl Parser<'_> {
    /// A string: text between double quotes, `""` inside it being one quote. The position is at the opening quote.
    pub(super) fn string(&mut self) -> Res<Node> {
        let start = self.pos;
        self.pos += 1;
        let mut text = String::new();
        loop {
            let rest = self.rest();
            let Some(end) = rest.find('"') else { return self.fail_at(ParseErrorKind::UnterminatedString, start) };
            text.push_str(rest.get(..end).unwrap_or(""));
            self.pos += end + 1;
            if self.peek() == Some('"') {
                text.push('"');
                self.pos += 1;
            } else {
                return self.leaf(Expr::Str(Arc::from(text)));
            }
        }
    }

    /// A colour, `#` and six hex digits, whose digits do not go on. The position is at the `#`. Anything else after a `#` is
    /// an error, `#REF!` and the other spellings of an error value among it.
    pub(super) fn hash(&mut self) -> Res<Node> {
        let Some([r, g, b]) = colour(self.rest()) else { return self.fail(ParseErrorKind::BadHash) };
        self.pos += 7;
        self.leaf(Expr::Color { r, g, b })
    }

    /// A GUID in braces with no quotes, in the shape R62 gives, whole with its braces: it reads as the string it spells.
    /// The position is at the `{`. Where one may stand is for the caller; this is the token.
    pub(super) fn guid(&mut self) -> Res<Node> {
        let rest = self.rest();
        let text = rest.get(..GUID_BODY + 2).filter(|t| is_guid(t));
        let Some(text) = text else { return self.fail(ParseErrorKind::BadGuid) };
        self.pos += GUID_BODY + 2;
        self.leaf(Expr::Guid(Arc::from(text)))
    }
}

/// `{`, the 36 characters of a GUID, `}`.
fn is_guid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first() == Some(&b'{')
        && bytes.last() == Some(&b'}')
        && bytes.len() == GUID_BODY + 2
        && bytes.iter().skip(1).take(GUID_BODY).enumerate().all(|(i, b)| if GUID_HYPHENS.contains(&i) { *b == b'-' } else { b.is_ascii_hexdigit() })
}

/// The colour `#` and six hex digits at the start of `text`, when the digits do not go on.
fn colour(text: &str) -> Option<[u8; 3]> {
    let digits = text.get(1..7)?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) || text.get(7..).and_then(|t| t.chars().next()).is_some_and(is_name_char) {
        return None;
    }
    let part = |i: usize| digits.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([part(0)?, part(2)?, part(4)?])
}
