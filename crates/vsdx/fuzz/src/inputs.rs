//! How the bytes of a target become a package the reader meets.
//!
//! Changing the archive's own bytes is stopped by the container's checksums nearly always, and a part of free bytes is not XML: so a target that only did that would explore the archive
//! reader (which `crates/container/fuzz` already fuzzes) and not the reader above it. **The input is a recipe instead**: one part of a seed package (the plain one or the one that carries
//! the data of the other format) is replaced by bytes, or has bytes spliced into it, and the package is packed again with right checksums, so the reader meets what a hostile file made by hand
//! would be. One input in eight is the bytes as they are.
//!
//! | byte | meaning |
//! |---|---|
//! | 0 | a value that is 7 modulo 8 says the rest is the package, as it is |
//! | 1 | bit 0: the seed (0 plain, 1 carried); bit 1: 0 replaces the part, 1 splices into it (2 bytes where, 2 bytes how many to cut, the rest is put there); the other six bits: the part, by place |
//! | 2 | the pool of a second read: 0 none, else this many 4 KiB beside what the read holds |
//! | the rest | the replacement or the splice: **a byte below 50 is one of [`SNIPPETS`]** (a tag, an entity, a namespace, a number at an edge, the carrier's words), any other byte is itself, so a short input is already XML that changes what the reader sees |

use crate::seeds::{pack, seeds};

/// Fragments that change what an XML reader sees: a tag, an entity, a namespace, a quote, a number at an edge, and the words of the carrier's items. The first 40 are those of the proptest
/// properties of `tests/it/fuzz.rs`.
pub const SNIPPETS: [&str; 50] = [
    "<",
    ">",
    "</",
    "/>",
    "&",
    "&amp;",
    "&#0;",
    "&#x110000;",
    "&bogus;",
    "\"",
    "'",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "<?",
    "?>",
    "<!DOCTYPE x>",
    "xmlns=\"urn:z\"",
    "xmlns:r=\"urn:z\"",
    " ID=\"4294967296\"",
    " ID=\"-1\"",
    " IX=\"",
    "<Cell N=\"X\"/>",
    "<Shape ID=\"1\">",
    "</Shape>",
    "<Shapes>",
    "</Shapes>",
    "<Section N=\"Geometry\"/>",
    "<Row/>",
    "\u{FFFE}",
    "\u{1F600}",
    "\r\n",
    "1.#INF",
    "Themed",
    "Inh",
    "No Formula",
    "N=\"\"",
    "urn:app.diacraft:extension",
    "dc:item",
    " kind=\"document\"",
    " blob=\"1\"",
    " role=\"macro-project\"",
    " Target=\"../../diacraft/blob1.bin\"",
    " Type=\"urn:app.diacraft:extension:blob\"",
    " name=\"Basic/x\"",
    " TargetMode=\"External\"",
    " fp=\"0000000000000000\"",
    // The namespace that builds made before the id moved to `diacraft.app` wrote, and the type of the relationship it made (APPID): read as DiaCraft's own.
    "urn:io.github.one_bit.diacraft:extension",
    " Type=\"urn:io.github.one_bit.diacraft:extension:blob\"",
];

/// The bytes of a payload with a byte below 50 put as the snippet of that number.
fn expand(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    for &b in payload {
        match SNIPPETS.get(usize::from(b)) {
            Some(snippet) => out.extend_from_slice(snippet.as_bytes()),
            None => out.push(b),
        }
    }
    out
}

/// A package for the reader, and how it is to be read.
pub struct Input {
    pub bytes: Vec<u8>,
    /// The bytes are the input as it came, and no recipe.
    pub raw: bool,
    /// A pool for a second read beside what the read holds, in bytes; `None` for none.
    pub pool: Option<u64>,
    /// The seed that was changed: carried, or plain.
    pub carried: bool,
}

/// Puts the rest of `payload` (as [`expand`] makes it) into `part` where its first four bytes say, taking out as many bytes as they say.
fn splice(part: &mut Vec<u8>, payload: &[u8]) {
    let (head, insert) = payload.split_at(payload.len().min(4));
    let insert = expand(insert);
    let n = |i: usize| head.get(i).copied().map_or(0usize, usize::from);
    let at = ((n(0) << 8) | n(1)) % part.len().saturating_add(1);
    let cut = (((n(2) << 8) | n(3)) % 256).min(part.len().saturating_sub(at));
    part.splice(at..at.saturating_add(cut), insert.iter().copied());
}

/// The package a target's bytes make, or `None` when they are too short to say.
pub fn input(data: &[u8]) -> Option<Input> {
    let (&first, rest) = data.split_first()?;
    if first % 8 == 7 {
        return Some(Input { bytes: rest.to_vec(), raw: true, pool: None, carried: false });
    }
    let (&how, rest) = rest.split_first()?;
    let (&pool, payload) = rest.split_first()?;
    let carried = how & 1 == 1;
    let seed = if carried { &seeds().carried } else { &seeds().plain };
    let mut items = seed.clone();
    let at = usize::from(how >> 2) % items.len().max(1);
    let part = &mut items.get_mut(at)?.1;
    if how & 2 == 0 {
        *part = expand(payload);
    } else {
        splice(part, payload);
    }
    Some(Input { bytes: pack(&items), raw: false, pool: (pool != 0).then(|| u64::from(pool) * 4096), carried })
}
