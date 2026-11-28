//! The catalogue of mutations: what a hostile file does to a seed, one class at a time, from a fixed stream of random numbers.
//!
//! A [`Class`] is a kind of change; [`mutate`] makes one mutant of a [`Seed`] by it, or says the seed has nothing for it. The classes fall in four groups:
//!
//! * **bytes**: flipped bits, overwritten runs, a cut-off end, bytes put in or taken out. Each is made on the file as it is (a package's checksums then refuse most of them, which is the
//!   container's own check) or inside one entry of a package that is then written again with true checksums, so that the change reaches the reader above the container;
//! * **entries**: an entry written twice, taken out, moved, given another name, or replaced by a long run of one byte;
//! * **lies**: a package whose headers say what its data does not (sizes, offsets, checksums, names, methods, flags, the count and the place of the central directory);
//! * **XML**: one part's text changed as [`super::xml`] says (nesting, attribute floods, copies, entity references, numbers past every range, text that is no number, formulas that
//!   read each other, loops of references, chains of linked items).

use super::rng::Rng;
use super::seeds::Seed;
use super::xml;
use super::zipfile::{Archive, Entry, Raw, Tail, raws_of, write_raw};

/// The most bytes a mutant may have; a larger one is not made (a sweep is meant to be quick, and a reader's own caps are tested elsewhere).
pub const MAX_MUTANT: usize = 24 << 20;

/// A kind of change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    BitFlips,
    Garble,
    Truncate,
    Splice,
    Duplicate,
    Remove,
    Reorder,
    Rename,
    Bomb,
    Lie,
    Nest,
    Flood,
    Wide,
    Entities,
    HugeNumber,
    NotANumber,
    Formula,
    Loop,
    Chain,
}

/// The four groups of classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Bytes,
    Entries,
    Lies,
    Xml,
}

impl Class {
    /// Every class, in the order a sweep takes them.
    pub const ALL: [Class; 19] = [
        Class::BitFlips,
        Class::Garble,
        Class::Truncate,
        Class::Splice,
        Class::Duplicate,
        Class::Remove,
        Class::Reorder,
        Class::Rename,
        Class::Bomb,
        Class::Lie,
        Class::Nest,
        Class::Flood,
        Class::Wide,
        Class::Entities,
        Class::HugeNumber,
        Class::NotANumber,
        Class::Formula,
        Class::Loop,
        Class::Chain,
    ];

    /// The class's name, as a report and a dump say it.
    pub fn name(self) -> &'static str {
        match self {
            Class::BitFlips => "bit-flips",
            Class::Garble => "garble",
            Class::Truncate => "truncate",
            Class::Splice => "splice",
            Class::Duplicate => "duplicate-entry",
            Class::Remove => "remove-entry",
            Class::Reorder => "reorder-entries",
            Class::Rename => "rename-entry",
            Class::Bomb => "bomb-entry",
            Class::Lie => "lying-zip",
            Class::Nest => "xml-nest",
            Class::Flood => "xml-flood",
            Class::Wide => "xml-wide",
            Class::Entities => "xml-entities",
            Class::HugeNumber => "xml-huge-number",
            Class::NotANumber => "xml-not-a-number",
            Class::Formula => "xml-formula",
            Class::Loop => "xml-loop",
            Class::Chain => "xml-chain",
        }
    }

    /// The group the class belongs to.
    pub fn group(self) -> Group {
        match self {
            Class::BitFlips | Class::Garble | Class::Truncate | Class::Splice => Group::Bytes,
            Class::Duplicate | Class::Remove | Class::Reorder | Class::Rename | Class::Bomb => Group::Entries,
            Class::Lie => Group::Lies,
            _ => Group::Xml,
        }
    }

    /// Whether a seed can have this class made of it at all: the package groups need a package, and a class that works through the profile needs the profile to name its parts.
    pub fn applies(self, seed: &Seed) -> bool {
        match self {
            Class::Duplicate | Class::Remove | Class::Reorder | Class::Rename | Class::Bomb | Class::Lie => seed.archive.is_some(),
            Class::Formula => seed.profile.cell.is_some(),
            Class::Loop => !seed.profile.refs.is_empty(),
            Class::Chain => seed.profile.chain.is_some(),
            _ => true,
        }
    }
}

/// One mutant: the class, a line that says what was done, and the bytes.
#[derive(Clone, Debug)]
pub struct Mutant {
    pub class: Class,
    pub detail: String,
    pub bytes: Vec<u8>,
}

/// The mutant of `seed` by `class`, or `None` when the seed has nothing the class can change (or the mutant would pass [`MAX_MUTANT`]).
pub fn mutate(seed: &Seed, class: Class, rng: &mut Rng) -> Option<Mutant> {
    let (bytes, detail) = match class.group() {
        Group::Bytes => bytes_class(seed, class, rng)?,
        Group::Entries => entries_class(seed, class, rng)?,
        Group::Lies => lie(seed, rng)?,
        Group::Xml => xml_class(seed, class, rng)?,
    };
    (bytes.len() <= MAX_MUTANT).then_some(Mutant { class, detail, bytes })
}

// ---------------------------------------------------------------------------------------------------------------- bytes

fn flip_bits(data: &mut [u8], rng: &mut Rng) -> String {
    let n = rng.between(1, 8);
    for _ in 0..n {
        let at = rng.below(data.len());
        if let Some(b) = data.get_mut(at) {
            *b ^= 1 << rng.below(8);
        }
    }
    format!("{n} bit(s) flipped")
}

fn garble(data: &mut [u8], rng: &mut Rng) -> String {
    let len = rng.between(1, 64).min(data.len());
    let at = rng.below(data.len().saturating_sub(len) + 1);
    let fill = rng.below(3);
    for i in 0..len {
        let value = match fill {
            0 => 0,
            1 => 0xFF,
            _ => rng.byte(),
        };
        if let Some(b) = data.get_mut(at + i) {
            *b = value;
        }
    }
    format!("{len} byte(s) overwritten from {at} with {}", ["zeros", "0xFF", "noise"].get(fill).copied().unwrap_or(""))
}

fn truncate(data: &mut Vec<u8>, rng: &mut Rng) -> String {
    let keep = match rng.below(4) {
        0 => 0,
        1 => data.len().saturating_sub(rng.between(1, 22)),
        _ => rng.below(data.len()),
    };
    data.truncate(keep);
    format!("cut to {keep} byte(s)")
}

fn splice(data: &mut Vec<u8>, rng: &mut Rng) -> String {
    let at = rng.below(data.len() + 1);
    match rng.below(3) {
        0 => {
            let n = rng.between(1, 200);
            let noise: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
            drop(data.splice(at..at, noise));
            format!("{n} byte(s) of noise put in at {at}")
        }
        1 => {
            let end = (at + rng.between(1, 300)).min(data.len());
            drop(data.drain(at..end));
            format!("bytes {at}..{end} taken out")
        }
        _ => {
            let end = (at + rng.between(1, 300)).min(data.len());
            let copy: Vec<u8> = data.get(at..end).unwrap_or_default().to_vec();
            let to = rng.below(data.len() + 1);
            drop(data.splice(to..to, copy));
            format!("bytes {at}..{end} copied to {to}")
        }
    }
}

/// Applies a byte class to `data`.
fn alter(class: Class, data: &mut Vec<u8>, rng: &mut Rng) -> String {
    match class {
        Class::BitFlips => flip_bits(data, rng),
        Class::Garble => garble(data, rng),
        Class::Truncate => truncate(data, rng),
        _ => splice(data, rng),
    }
}

fn bytes_class(seed: &Seed, class: Class, rng: &mut Rng) -> Option<(Vec<u8>, String)> {
    // Half of a package's mutants are made inside one entry and packed again with true checksums, so that the change reaches the reader above the container.
    if let (Some(archive), true) = (&seed.archive, rng.one_in(2)) {
        let mut archive = archive.clone();
        let index = rng.below(archive.entries.len());
        let entry = archive.entries.get_mut(index)?;
        let name = entry.name_text();
        let what = alter(class, &mut entry.data, rng);
        return Some((archive.honest(), format!("{what} inside the entry `{name}`, checksums made true")));
    }
    let mut bytes = seed.bytes.clone();
    let what = alter(class, &mut bytes, rng);
    Some((bytes, format!("{what} in the file")))
}

// ---------------------------------------------------------------------------------------------------------------- entries

fn rename(name: &[u8], rng: &mut Rng) -> (Vec<u8>, &'static str) {
    let text = String::from_utf8_lossy(name).into_owned();
    match rng.below(10) {
        0 => (text.to_uppercase().into_bytes(), "upper case"),
        1 => (format!("../{text}").into_bytes(), "a name that climbs out"),
        2 => (format!("/{text}").into_bytes(), "a name with a leading slash"),
        3 => (text.replace('/', "\\").into_bytes(), "backslashes"),
        4 => (format!("{text}\0x").into_bytes(), "a NUL in the name"),
        5 => (vec![b'a'; 65_535], "a name of 65,535 bytes"),
        6 => (format!("./{text}").into_bytes(), "a leading dot"),
        7 => (format!("{text}/").into_bytes(), "a name that ends in a slash"),
        8 => (format!("\u{202e}{text}\u{e9}").into_bytes(), "a bidirectional override and a letter with an accent"),
        _ => (Vec::new(), "an empty name"),
    }
}

fn bomb(archive: &mut Archive, rng: &mut Rng) -> Option<String> {
    let index = rng.below(archive.entries.len());
    let size = rng.size(&[1 << 20, 4 << 20], &[16 << 20]);
    let (fill, what): (Vec<u8>, &str) = match rng.below(3) {
        0 => (vec![0], "zeros"),
        1 => (b" ".to_vec(), "spaces"),
        _ => (b"<a/>".to_vec(), "empty elements"),
    };
    let entry = archive.entries.get_mut(index)?;
    let name = entry.name_text();
    entry.data = fill.iter().copied().cycle().take(size).collect();
    entry.deflate = true;
    Some(format!("the entry `{name}` replaced by {size} bytes of {what}, deflated"))
}

fn entries_class(seed: &Seed, class: Class, rng: &mut Rng) -> Option<(Vec<u8>, String)> {
    let mut archive = seed.archive.clone()?;
    let n = archive.entries.len();
    if n == 0 {
        return None;
    }
    let detail = match class {
        Class::Duplicate => {
            let index = rng.below(n);
            let mut copy = archive.entries.get(index)?.clone();
            let name = copy.name_text();
            let changed = rng.one_in(2);
            if changed {
                alter(Class::Garble, &mut copy.data, rng);
            }
            let at = *rng.pick(&[0, index + 1, n])?;
            archive.entries.insert(at.min(n), copy);
            format!("the entry `{name}` written twice{}, the second at {at}", if changed { " with its data garbled" } else { "" })
        }
        Class::Remove => {
            let how_many = if rng.one_in(4) { rng.between(2, n.max(2)).min(n) } else { 1 };
            let mut names = Vec::new();
            for _ in 0..how_many {
                if archive.entries.is_empty() {
                    break;
                }
                let index = rng.below(archive.entries.len());
                names.push(archive.entries.remove(index).name_text());
            }
            format!("taken out: {}", names.join(", "))
        }
        Class::Reorder => {
            match rng.below(3) {
                0 => archive.entries.reverse(),
                1 => {
                    let first = archive.entries.remove(0);
                    archive.entries.push(first);
                }
                _ => {
                    let (a, b) = (rng.below(n), rng.below(n));
                    archive.entries.swap(a, b);
                }
            }
            "entries moved".to_owned()
        }
        Class::Rename => {
            let index = rng.below(n);
            let entry = archive.entries.get_mut(index)?;
            let old = entry.name_text();
            let (name, what) = rename(&entry.name, rng);
            entry.name = name;
            format!("the entry `{old}` renamed: {what}")
        }
        _ => bomb(&mut archive, rng)?,
    };
    Some((archive.honest(), detail))
}

// ---------------------------------------------------------------------------------------------------------------- lies

fn lie_in_a_record(raws: &mut [Raw], rng: &mut Rng) -> String {
    let n = raws.len();
    let i = rng.below(n);
    let other = rng.below(n);
    let false_size = *rng.pick(&[0u32, 1, 7, 0xFFFF_FFFF, 0x7FFF_FFFF, 1 << 31]).unwrap_or(&0);
    let Some(r) = raws.get_mut(i) else { return "nothing".to_owned() };
    let name = String::from_utf8_lossy(&r.name).into_owned();
    match rng.below(20) {
        0 => {
            r.central_size = false_size;
            format!("`{name}`: the central record says the size is {false_size}")
        }
        1 => {
            r.size = false_size;
            format!("`{name}`: the local header says the size is {false_size}")
        }
        2 => {
            r.central_compressed = false_size;
            format!("`{name}`: the central record says the compressed size is {false_size}")
        }
        3 => {
            r.compressed = false_size;
            format!("`{name}`: the local header says the compressed size is {false_size}")
        }
        4 => {
            r.central_crc ^= 0xDEAD_BEEF;
            format!("`{name}`: the central record's checksum is wrong")
        }
        5 => {
            r.crc = 0;
            format!("`{name}`: the local header's checksum is 0")
        }
        6 => {
            r.central_name = format!("{name}.other").into_bytes();
            format!("`{name}`: the central record names `{name}.other`")
        }
        7 => {
            r.name = format!("x{name}").into_bytes();
            format!("`{name}`: the local header names `x{name}`")
        }
        8 => {
            let method = *rng.pick(&[0u16, 1, 9, 12, 14, 93, 99, 0xFFFF]).unwrap_or(&99);
            r.central_method = method;
            format!("`{name}`: the central record says method {method}")
        }
        9 => {
            let method = *rng.pick(&[0u16, 8, 99]).unwrap_or(&99);
            r.method = method;
            format!("`{name}`: the local header says method {method}, the data are as they were")
        }
        10 => {
            r.flags = *rng.pick(&[1u16, 8, 0x0800, 0x0049, 0xFFFF]).unwrap_or(&8);
            r.central_flags = *rng.pick(&[0u16, 1, 8, 0xFFFF]).unwrap_or(&0);
            format!("`{name}`: flags {:#x} (local) and {:#x} (central)", r.flags, r.central_flags)
        }
        11 => {
            let other_offset = *rng.pick(&[0u32, 1, 30, 0xFFFF_FFFF, 0x7FFF_FFFF]).unwrap_or(&0);
            r.offset = Some(other_offset);
            format!("`{name}`: the central record points at {other_offset}")
        }
        12 => {
            let target = raws.get(other).map(|o| u32::try_from(o.payload.len()).unwrap_or(0).saturating_add(30));
            if let Some(r) = raws.get_mut(i) {
                r.offset = target;
            }
            format!("`{name}`: the central record points at another entry's place ({other})")
        }
        13 => {
            r.extra = vec![0x01, 0x00, 0xFF, 0xFF, 1, 2, 3];
            r.central_extra =
                vec![0x01, 0x00, 0x10, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
            format!("`{name}`: extra fields with a Zip64 record that lies")
        }
        14 => {
            r.central_size = 0xFFFF_FFFF;
            r.central_compressed = 0xFFFF_FFFF;
            r.offset = Some(0xFFFF_FFFF);
            format!("`{name}`: Zip64 markers in the central record and no Zip64 record")
        }
        15 => {
            r.listed = false;
            format!("`{name}` is in no central record")
        }
        16 => {
            r.payload.truncate(r.payload.len() / 2);
            format!("`{name}`: the data are cut to half, the headers as they were")
        }
        17 => {
            let junk: Vec<u8> = (0..r.payload.len().min(64)).map(|_| rng.byte()).collect();
            r.payload = junk;
            format!("`{name}`: the data are noise, the headers as they were")
        }
        18 => {
            r.extra = vec![0xAB; 70];
            r.central_extra = vec![0xCD; 3];
            format!("`{name}`: extra fields that are noise")
        }
        _ => {
            r.flags |= 8;
            r.crc = 0;
            r.compressed = 0;
            r.size = 0;
            format!("`{name}`: a data descriptor is claimed and the local sizes are 0")
        }
    }
}

fn lie_in_the_tail(raws: &[Raw], tail: &mut Tail, rng: &mut Rng) -> String {
    let n = raws.len();
    match rng.below(10) {
        0 => {
            let count = *rng.pick(&[0u16, 1, 9999, 0xFFFF]).unwrap_or(&0);
            tail.count = Some(count);
            format!("the end record counts {count} entries")
        }
        1 => {
            let size = *rng.pick(&[0u32, 1, 0xFFFF_FFFF]).unwrap_or(&0);
            tail.directory_size = Some(size);
            format!("the end record says the central directory is {size} bytes")
        }
        2 => {
            let at = *rng.pick(&[0u32, 1, 0xFFFF_FFFF, 0x7FFF_FFFF]).unwrap_or(&0);
            tail.directory_offset = Some(at);
            format!("the end record puts the central directory at {at}")
        }
        3 => {
            tail.comment = b"PK\x05\x06 a comment that holds an end record".to_vec();
            tail.comment_length = Some(*rng.pick(&[0u16, 500, 0xFFFF]).unwrap_or(&0));
            "an end record's comment whose length is a lie".to_owned()
        }
        4 => {
            tail.prefix = vec![b'M', b'Z', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            "junk in front of the first header, the offsets as they would be without it".to_owned()
        }
        5 => {
            tail.suffix = (0..rng.between(1, 4096)).map(|_| rng.byte()).collect();
            format!("{} bytes of junk after the end record", tail.suffix.len())
        }
        6 => {
            tail.count = Some(u16::try_from(n.saturating_sub(1)).unwrap_or(0));
            "the end record counts one entry fewer than the directory holds".to_owned()
        }
        7 => {
            tail.count = Some(u16::try_from(n + 1).unwrap_or(u16::MAX));
            "the end record counts one entry more than the directory holds".to_owned()
        }
        8 => {
            tail.comment = b"\x50\x4b\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
            "a second end record in the comment of the first".to_owned()
        }
        _ => {
            tail.directory_size = Some(0);
            tail.count = Some(0);
            "an end record of an empty archive over a directory that is not".to_owned()
        }
    }
}

fn lie(seed: &Seed, rng: &mut Rng) -> Option<(Vec<u8>, String)> {
    let mut raws = raws_of(seed.archive.as_ref()?);
    if raws.is_empty() {
        return None;
    }
    let mut tail = Tail::default();
    let what = if rng.one_in(4) { lie_in_the_tail(&raws, &mut tail, rng) } else { lie_in_a_record(&mut raws, rng) };
    Some((write_raw(&raws, &tail), what))
}

// ---------------------------------------------------------------------------------------------------------------- XML

/// The index of the entry an XML mutator works on: one that holds XML, the largest half the time.
fn pick_xml_entry(archive: &Archive, rng: &mut Rng, skip: &[usize]) -> Option<usize> {
    let xml: Vec<usize> = (0..archive.entries.len()).filter(|i| !skip.contains(i) && archive.entries.get(*i).is_some_and(Entry::is_xml)).collect();
    if rng.one_in(2) { xml.iter().copied().max_by_key(|&i| archive.entries.get(i).map_or(0, |e| e.data.len())) } else { rng.pick(&xml).copied() }
}

fn run_xml(class: Class, text: &str, seed: &Seed, rng: &mut Rng) -> Option<(String, String)> {
    match class {
        Class::Nest => xml::nest(text, &seed.profile, rng),
        Class::Flood => xml::flood(text, rng),
        Class::Wide => xml::wide(text, rng),
        Class::Entities => xml::entities(text, rng),
        Class::HugeNumber => xml::huge_number(text, &seed.profile, rng),
        Class::NotANumber => xml::not_a_number(text, &seed.profile, rng),
        Class::Formula => xml::formula(text, &seed.profile, rng),
        Class::Loop => xml::reference_loop(text, &seed.profile, rng),
        _ => xml::chain(text, &seed.profile, rng),
    }
}

/// The number in the name of a master's part (`masters/master3.xml`, or the name of the part in a folder, with the prefix `masters/master` is `3`), and where the entry is.
fn master_parts(archive: &Archive, prefix: &str) -> Vec<(usize, String)> {
    archive
        .entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.name_text();
            let digits = name.get(name.find(prefix)? + prefix.len()..)?.strip_suffix(".xml")?.to_owned();
            digits.parse::<u32>().ok().map(|_| (i, digits))
        })
        .collect()
}

/// A loop of masters: a shape in each of up to three masters is made an instance of the next master, the last of the first (or one master's shape an instance of its own master).
fn master_loop(seed: &Seed, rng: &mut Rng) -> Option<(Vec<u8>, String)> {
    let (prefix, element, attr) = seed.profile.masters?;
    let archive = seed.archive.as_ref()?;
    let masters = master_parts(archive, prefix);
    if masters.is_empty() {
        return None;
    }
    let first = rng.below(masters.len());
    let k = if rng.one_in(3) { 1 } else { rng.between(2, 3).min(masters.len()) };
    let ring: Vec<&(usize, String)> = (0..k).filter_map(|i| masters.get((first + i) % masters.len().max(1))).collect();
    let mut changed = archive.clone();
    for (i, (index, _)) in ring.iter().enumerate() {
        let next = &ring.get((i + 1) % ring.len())?.1;
        let entry = changed.entries.get_mut(*index)?;
        let text = String::from_utf8_lossy(&entry.data).into_owned();
        let tags = xml::scan(&text);
        let shapes: Vec<&xml::Tag> = tags.iter().filter(|t| t.kind != xml::Kind::Close && xml::name(&text, t) == element).collect();
        let tag = *rng.pick(&shapes)?;
        let edit = xml::set_attr(&text, tag, attr, next);
        entry.data = xml::apply(&text, vec![edit]).into_bytes();
    }
    let numbers: Vec<&str> = ring.iter().map(|(_, n)| n.as_str()).collect();
    Some((changed.honest(), format!("a loop of masters {}: a shape of each is an instance of the next", numbers.join(" > "))))
}

fn xml_class(seed: &Seed, class: Class, rng: &mut Rng) -> Option<(Vec<u8>, String)> {
    if class == Class::Loop
        && seed.profile.masters.is_some()
        && rng.one_in(2)
        && let Some(made) = master_loop(seed, rng)
    {
        return Some(made);
    }
    let Some(archive) = &seed.archive else {
        let text = String::from_utf8_lossy(&seed.bytes).into_owned();
        let (out, what) = run_xml(class, &text, seed, rng)?;
        return Some((out.into_bytes(), what));
    };
    let mut tried = Vec::new();
    for _ in 0..6 {
        let index = pick_xml_entry(archive, rng, &tried)?;
        tried.push(index);
        let entry = archive.entries.get(index)?;
        let text = String::from_utf8_lossy(&entry.data).into_owned();
        if let Some((out, what)) = run_xml(class, &text, seed, rng) {
            let mut changed = archive.clone();
            changed.entries.get_mut(index)?.data = out.into_bytes();
            return Some((changed.honest(), format!("{what} in the entry `{}`", entry.name_text())));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::xml::{CellNames, Profile};

    fn seed() -> Seed {
        let page = r#"<Shapes><Shape ID="1" LineStyle="2" W="3cm"><Cell N="Width" V="2" F="Height"/><Cell N="Height" V="1"/></Shape><Shape ID="2" LineStyle="1"/></Shapes>"#;
        let archive = Archive {
            entries: vec![
                Entry::new("mimetype", b"application/test".to_vec(), false),
                Entry::new("page.xml", page.as_bytes().to_vec(), true),
                Entry::new("other.xml", b"<o a=\"1\"/>".to_vec(), true),
                Entry::new("pic.png", vec![1, 2, 3, 4], false),
            ],
        };
        Seed {
            name: "unit",
            extension: "zip",
            bytes: archive.honest(),
            archive: Some(archive),
            profile: Profile {
                cell: Some(CellNames { element: "Cell", name: "N", value: "V", formula: "F" }),
                refs: &[("LineStyle", "ID")],
                nest_unit: Some(("<Shape>", "</Shape>")),
                nest_into: &["Shapes"],
                chain: Some(xml::Chain { into: "Shapes", item: |k, _| format!("<Shape ID=\"{}\"/>", 100 + k), modest: &[10, 40], heavy: &[500] }),
                masters: None,
            },
        }
    }

    #[test]
    fn every_class_makes_a_different_file_of_a_seed_that_fits_it_and_says_what_it_did() {
        let seed = seed();
        for class in Class::ALL {
            assert!(class.applies(&seed), "{}", class.name());
            let mut made = 0;
            for case in 0..60 {
                let mut rng = Rng::for_case(11, class.name(), case);
                if let Some(m) = mutate(&seed, class, &mut rng) {
                    assert_eq!(m.class, class);
                    assert!(!m.detail.is_empty(), "{}", class.name());
                    if m.bytes != seed.bytes {
                        made += 1;
                    }
                }
            }
            assert!(made >= 30, "{}: only {made} of 60 cases changed the file", class.name());
        }
    }

    #[test]
    fn the_same_stream_makes_the_same_mutant_and_a_class_the_seed_cannot_have_is_not_applied() {
        let seed = seed();
        let one = |case| mutate(&seed, Class::Lie, &mut Rng::for_case(3, "lie", case)).map(|m| (m.bytes, m.detail));
        assert_eq!(one(4), one(4));
        assert_ne!(one(4), one(5));
        let flat = Seed { archive: None, profile: Profile::default(), ..seed.clone() };
        for class in
            [Class::Duplicate, Class::Remove, Class::Reorder, Class::Rename, Class::Bomb, Class::Lie, Class::Formula, Class::Loop, Class::Chain]
        {
            assert!(!class.applies(&flat), "{}", class.name());
        }
        assert!(mutate(&flat, Class::Lie, &mut Rng::new(1)).is_none());
    }

    #[test]
    fn a_lie_is_read_back_as_a_refusal_or_a_different_archive_and_never_as_the_honest_one() {
        let seed = seed();
        let honest = seed.bytes.clone();
        let mut different = 0;
        for case in 0..80 {
            let m = mutate(&seed, Class::Lie, &mut Rng::for_case(8, "lie", case)).unwrap();
            if m.bytes != honest {
                different += 1;
            }
        }
        assert_eq!(different, 80);
    }

    #[test]
    fn an_xml_mutant_of_a_package_is_a_package_with_true_checksums() {
        let seed = seed();
        for case in 0..30 {
            let m = mutate(&seed, Class::HugeNumber, &mut Rng::for_case(2, "x", case)).unwrap();
            let archive = Archive::parse(&m.bytes).expect("an honest archive of changed entries");
            assert_eq!(archive.entries.len(), 4);
            assert_eq!(archive.entries.first(), seed.archive.as_ref().and_then(|a| a.entries.first()), "the mimetype entry is as it was");
        }
    }
}
