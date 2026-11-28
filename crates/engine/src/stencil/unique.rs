//! The `UniqueID` of a master DiaCraft makes (`docs/editing-commands.md`).
//!
//! **Why it is not the `BaseID`'s text.** The VSDX application keeps only the first three groups of a master's `UniqueID` (it puts a value of its own in the last two when it loads the
//! file) and keeps `BaseID` as written (a tester's observation, 2026-11-28). A `UniqueID` that differs from another only in its last two groups is then the same master
//! there: every master of one built-in stencil, and the masters at one place in two DiaCraft drawings, read as one, so a drop from a stencil file of DiaCraft's gave the first master
//! dropped and a paste between two DiaCraft drawings gave the destination's master. A `UniqueID` told apart by its first group fixed it, a `BaseID` told apart did not.
//!
//! **The rule.** A built-in master's `BaseID` is its fixed identity, `{D1AC<series>-0000-4000-8000-<ordinal>}` ([`MasterSpec::guid`]): it names the master across drawings, and every
//! build that ever wrote the master wrote that text, so a drawing that holds it is found again by it (`find_same_master`). Its `UniqueID` is a GUID of the version 4 form whose bits are a hash
//! of the master's identity (series and ordinal) and of what the master holds (the stamp of its part as the sheet codec writes it, with no `UniqueID` and the `ID` 1): **the same built-in
//! master has the same `UniqueID` in every drawing and file, a master that another build made another way has another, and the first three groups differ from every other master's**
//! (60 bits of a mixed 64-bit hash for each master of a catalogue of a few hundred; a test holds every master DiaCraft can make to it).
//!
//! Drawings saved by 0.2.3 and the first builds of 0.2.4 hold masters whose `UniqueID` is their `BaseID`'s text. They are read as they are, and found by their `BaseID`, never by the
//! form of the `UniqueID`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Digest, Document, Master};
use diacraft_sheetxml::model::{MemBlobs, encode_master, stamp_of_bytes};

use super::MasterSpec;

impl MasterSpec {
    /// The `UniqueID` a drawing gives this master (see the module documentation): the same text in every drawing.
    pub fn unique_id(&self) -> String {
        super::build_master(self, 1).unique_id.as_deref().map(str::to_owned).unwrap_or_default()
    }
}

/// The 64-bit finalizer of a hash (MurmurHash3's `fmix64`): every bit of the input reaches every bit of the output, so the 60 bits a `UniqueID` keeps from a stamp are as good as any 60.
pub(crate) fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// The stamp of what `master` holds, as the sheet codec writes its part, taken at the `ID` 1 and with no `UniqueID` (neither says what the master holds): `None` if the codec refuses it,
/// which it does not for a master this catalogue builds (the identity alone then makes the `UniqueID`).
fn content_stamp(master: &Master) -> Option<u64> {
    let mut probe = master.clone();
    probe.id = 1;
    probe.unique_id = None;
    let tree = encode_master(&probe, &mut MemBlobs::new()).ok()?;
    let mut bytes = Vec::new();
    diacraft_container::xml::write_element(&mut bytes, &tree).ok()?;
    Some(stamp_of_bytes(&bytes).0)
}

/// The `UniqueID` of `master`, which is what `spec` builds (see the module documentation).
pub(crate) fn unique_id_of(spec: &MasterSpec, master: &Master) -> String {
    let (series, at) = spec.identity;
    let content = content_stamp(master).unwrap_or(0);
    let hashed = |domain: &str| Digest::new(domain).bytes(&[series]).bytes(&at.to_le_bytes()).bytes(&content.to_le_bytes()).finish().0;
    guid_text(mix64(hashed("master-unique-id-1")), mix64(hashed("master-unique-id-2")))
}

/// A `UniqueID` for a copy of a master whose own the drawing cannot hold (`bring::unique_id_for`): the `attempt`th one, a hash of the text it replaces.
pub(crate) fn renewed(was: &str, attempt: u64) -> String {
    let hashed = |domain: &str| Digest::new(domain).text(was.trim()).bytes(&attempt.to_le_bytes()).finish().0;
    guid_text(mix64(hashed("renewed-unique-id-1")), mix64(hashed("renewed-unique-id-2")))
}

/// The first three groups of a GUID text, trimmed and in capitals: all the VSDX application keeps of a `UniqueID`, so what tells two masters apart there. A text of fewer than four groups is
/// its own key, whole.
pub(crate) fn prefix_key(text: &str) -> String {
    let text = text.trim();
    let mut groups = text.splitn(4, '-');
    match (groups.next(), groups.next(), groups.next(), groups.next()) {
        (Some(a), Some(b), Some(c), Some(_)) => format!("{a}-{b}-{c}").to_ascii_uppercase(),
        _ => text.to_ascii_uppercase(),
    }
}

/// A GUID of the version 4 form from 124 bits: `hi` makes the first two groups and the third (after its version digit 4, which takes the place of `hi`'s lowest four bits), `lo` the rest (the fourth
/// group's variant, 8 to B, takes the place of `lo`'s two highest bits). Upper-case digits, braces, as the other GUIDs DiaCraft writes.
pub(crate) fn guid_text(hi: u64, lo: u64) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        0x4000 | ((hi & 0xFFFF) >> 4),
        0x8000 | ((lo >> 48) & 0x3FFF),
        lo & 0xFFFF_FFFF_FFFF
    )
}

/// **The repair of a master of the old form** (owner ruling 2026-11-28): the drawing as a VSDX file is written from it. A master whose `UniqueID` and `BaseID` are both exactly the text
/// of a built-in master's `BaseID` (`{D1AC<series>-0000-4000-8000-<ordinal>}`, as 0.2.3 and the first builds of 0.2.4 wrote them: DiaCraft's own, and known to be taken for one master with every
/// other of its series by the application that opens the file) gets the `UniqueID` that master has today ([`MasterSpec::unique_id`]), and keeps its `BaseID`. **Every other master is left as it
/// is**: one that has the text as its `UniqueID` and another `BaseID`, one in another spelling, one that went through the application that opens the file, and every master of another program's file.
///
/// A repaired `UniqueID` is told from every other master's of the drawing in its first three groups: one that the drawing holds already (a master of another kind that holds the one of today, or a
/// second master of the old form of one built-in master, which a file may hold) is replaced by a hashed one ([`renewed`]). → the drawing repaired, or `None` when no master is of the old form, which
/// is nearly always: the drawing is then not copied. One pass over the masters, and a look at the text of each that is no more than its length and its start; the drawing it is given is not changed.
pub(crate) fn repair_old_ids(doc: &Document) -> Option<Document> {
    let mut old: Vec<(usize, &'static MasterSpec)> = Vec::new();
    for (at, master) in doc.masters.iter().enumerate() {
        let Some(unique) = master.unique_id.as_deref() else { continue };
        if master.raw.get("BaseID") != Some(unique) {
            continue;
        }
        if let Some(spec) = super::builtin_of_guid(unique) {
            old.push((at, spec));
        }
    }
    if old.is_empty() {
        return None;
    }
    // What the masters that stay hold in their first three groups, and then what each repaired one takes.
    let repaired: HashSet<usize> = old.iter().map(|(at, _)| *at).collect();
    let mut taken: HashSet<String> =
        doc.masters.iter().enumerate().filter(|(at, _)| !repaired.contains(at)).filter_map(|(_, m)| m.unique_id.as_deref()).map(prefix_key).collect();
    // For each built-in master met: the `UniqueID` it has today, and the number of the next try. A try is the one of today (0) or a hashed one ([`renewed`], 1 and up). **The count goes on from one
    // master of the old form of a built-in master to the next**, so that the tries of all of them together are at most the masters of the drawing and a few more: a drawing that holds one master many
    // times, and masters that hold the ids the tries would take, cannot make the work the product of the two.
    let mut today: HashMap<&'static str, (String, u64)> = HashMap::new();
    let limit = u64::try_from(taken.len().saturating_add(old.len())).unwrap_or(u64::MAX).saturating_add(64);
    let mut copy = Document::clone(doc);
    for (at, spec) in old {
        let (wanted, next) = today.entry(spec.id).or_insert_with(|| (spec.unique_id(), 0));
        let mut free = None;
        while *next <= limit && free.is_none() {
            let candidate = if *next == 0 { wanted.clone() } else { renewed(wanted, *next) };
            *next += 1;
            #[cfg(test)]
            TRIES.with(|n| n.set(n.get() + 1));
            if !taken.contains(&prefix_key(&candidate)) {
                free = Some(candidate);
            }
        }
        let (Some(new), Some(slot)) = (free, copy.masters.get_mut(at)) else { continue };
        taken.insert(prefix_key(&new));
        Arc::make_mut(slot).unique_id = Some(Arc::from(new.as_str()));
    }
    Some(copy)
}

// What the tests count of the work a repair does, for this thread: the `UniqueID`s tried.
#[cfg(test)]
thread_local! {
    static TRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
