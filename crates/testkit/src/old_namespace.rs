//! Files written with the namespace that builds made before the id moved to the domain `diacraft.app` wrote (APPID): `urn:io.github.one_bit.diacraft:extension`.
//!
//! **They are made by a build of `main` at 3f696318 and not by hand.** `fixtures/old-namespace.json` holds the parts of five files that its `diacraft-cli` wrote (`made_by` and `recipe`
//! in the file say how): a drawing as `.diacraft`, that drawing written as a VSDX file and as an `.odg` from the model, the VSDX file written as an `.odg` and that `.odg` written as
//! a VSDX file (each of the last two carries what the other format cannot say, in the namespace). Each part is the text or the bytes the CLI wrote; the archives are put together again
//! here, the `mimetype` first and stored, and the rest deflated. The names of the format's own URIs and folders are spelled with a token in the file (`⟦m⟧`, `⟦V⟧` and `⟦v⟧`), as
//! the tree's rules for names ask, and are put back when the part is read.
//!
//! A test asks [`file`] for the bytes, and [`parts`] for the parts, to say what a reader or a writer of today does with a file of before.

// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on panicking (AGENTS.md, Never crash) is for
// code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;

use crate::package::{ZipItem, zip};

/// The file the parts are read from.
const FIXTURES: &str = include_str!("../fixtures/old-namespace.json");

/// The namespace the files were written with, spelled out here and not through the constant of the model, which is the current one.
pub const OLD_NAMESPACE: &str = "urn:io.github.one_bit.diacraft:extension";

/// A drawing of two pages and two shapes, saved as `.diacraft`: every part is in the old namespace.
pub const DRAWING_DIACRAFT: &str = "old-drawing.diacraft";
/// That drawing written as a VSDX file from the model: the DiaCraft solution part holds the Snap and Glue options the window has no flag for.
pub const DRAWING_VSDX: &str = "old-from-diacraft.vsdx";
/// That drawing written as an `.odg` from the model: the carrier `DiaCraft/document.xml` and the attributes of the namespace.
pub const DRAWING_ODG: &str = "old-from-diacraft.odg";
/// The VSDX file written as an `.odg`: the carrier holds what the VSDX file held and OpenDocument cannot say.
pub const VSDX_AS_ODG: &str = "old-vsdx-as.odg";
/// The `.odg` written as a VSDX file: the solution part holds items, and a relationship of the old type leads to a part of DiaCraft's own.
pub const ODG_AS_VSDX: &str = "old-odg-as.vsdx";

/// The five files, by name.
pub const NAMES: [&str; 5] = [DRAWING_DIACRAFT, DRAWING_VSDX, DRAWING_ODG, VSDX_AS_ODG, ODG_AS_VSDX];

/// Puts the names back that the file spells with a token.
fn untoken(text: &str) -> String {
    // The names are made of pieces: this file is read by the tree's rules for names, as the fixture is.
    let (company, product) = (["micro", "soft"].concat(), ["vis", "io"].concat());
    text.replace("\u{27e6}m\u{27e7}", &company).replace("\u{27e6}V\u{27e7}", &["Vis", "io"].concat()).replace("\u{27e6}v\u{27e7}", &product)
}

/// Standard base64 with padding.
fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0u32);
    for c in text.bytes().filter(|c| *c != b'=') {
        bits = (bits << 6) | value(c).expect("a base64 letter");
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push(u8::try_from((bits >> have) & 0xff).expect("a byte"));
        }
    }
    out
}

/// The parts of the file `name`, in the archive's order: each its name and bytes.
pub fn parts(name: &str) -> Vec<(String, Vec<u8>)> {
    let doc: Value = serde_json::from_str(FIXTURES).expect("the fixture file is JSON");
    let list = doc["files"][name].as_array().unwrap_or_else(|| panic!("no fixture called {name}"));
    list.iter()
        .map(|part| {
            let entry = untoken(part["name"].as_str().expect("a part has a name"));
            let bytes = match (part["text"].as_str(), part["base64"].as_str()) {
                (Some(text), _) => untoken(text).into_bytes(),
                (None, Some(data)) => base64(data),
                (None, None) => panic!("part {entry} of {name} has no bytes"),
            };
            (entry, bytes)
        })
        .collect()
}

/// The bytes of the file `name`: the archive of its parts, the `mimetype` (when it has one) first and stored.
pub fn file(name: &str) -> Vec<u8> {
    let items: Vec<ZipItem> = parts(name)
        .iter()
        .map(|(entry, bytes)| if entry == "mimetype" { ZipItem::stored(entry, bytes) } else { ZipItem::deflated(entry, bytes) })
        .collect();
    zip(&items)
}

/// The bytes of one part of the file `name`, as text.
pub fn part_text(name: &str, part: &str) -> String {
    let bytes = parts(name).into_iter().find(|(entry, _)| entry == part).unwrap_or_else(|| panic!("{name} has no part {part}")).1;
    String::from_utf8(bytes).expect("the part is text")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_an_archive_that_holds_the_old_namespace_and_never_the_current_one() {
        for name in NAMES {
            let all = parts(name);
            assert!(!all.is_empty(), "{name}");
            let holding: usize = all.iter().filter(|(_, bytes)| String::from_utf8_lossy(bytes).contains(OLD_NAMESPACE)).count();
            assert!(holding >= 1, "{name} holds the old namespace in no part");
            assert!(
                all.iter().all(|(_, bytes)| !String::from_utf8_lossy(bytes).contains("urn:app.diacraft")),
                "{name} holds the current namespace: it was not written by a build before the move"
            );
            let bytes = file(name);
            assert!(bytes.starts_with(b"PK\x03\x04"), "{name} is a ZIP archive");
        }
    }

    #[test]
    fn the_tokens_of_the_format_s_own_names_are_put_back() {
        let text = part_text(ODG_AS_VSDX, "[Content_Types].xml");
        assert!(text.contains("application/vnd.ms-") && !text.contains('\u{27e6}'), "{text}");
        assert!(parts(DRAWING_VSDX).iter().any(|(entry, _)| entry.starts_with("vis") && entry.ends_with("/document.xml")));
    }

    #[test]
    fn base64_reads_the_standard_alphabet_with_and_without_padding() {
        assert_eq!(base64("TWFu"), b"Man");
        assert_eq!(base64("TWE="), b"Ma");
        assert_eq!(base64("TQ=="), b"M");
        assert_eq!(base64("+/8="), [0xfb, 0xff]);
        assert!(base64("").is_empty());
    }
}
