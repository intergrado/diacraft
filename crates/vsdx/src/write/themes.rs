//! Theme parts in a save.
//!
//! **What a save does with the model's themes.** A theme part that no edit touched is the original's, byte for byte: it is never read into
//! the save, and the content types and relationships that name it are carried (`mod.rs`). What the model's themes differ from the baseline's in
//! is one of three things, found by pairing the two lists:
//!
//! * **A theme in both** is paired by part name (compared as compares names), and then by bytes for what is left: a theme
//!   the session holds under a name the file did not give it is the same part when its bytes are the same, so a save does not take a part out
//!   and put it back under another name. A pair whose bytes differ is **rewritten** in the original's part, with its relationship and type as
//!   they were.
//! * **A theme only in the model** is **added**: a part named `theme<n>.xml` in the folder `theme` beside the document part, with the least
//!   `n` no part has, whatever name the model gave it (the model's name is the engine's and is not a package's to trust), a relationship from the
//!   document part of the type theme parts have, and an `Override` of their content type. The relationship takes the least
//!   `rId` the document part's relationships do not use.
//! * **A theme only in the baseline** is **removed**: the document part's relationships of that type to it go, and the part with its own
//!   relationships and `Override` goes if no relationship still reaches it.
//!
//! Selection is by `schemeEnum` and never by the place of a part, so none of this changes what a cell selects; the order of the model's themes
//! is the order a save adds them in.
//!
//! **Refused, as an error that writes nothing:** a theme that is not a well-formed XML part with the root `a:theme` (the reader would not take it
//! for a theme), more themes or more bytes of themes than the reader keeps (`MAX_THEMES`, `MAX_THEME_BYTES`: a file past them would read back with
//! themes missing).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::Theme;
use diacraft_opc::name::{fold, resolve};

use super::parts::{IdWalk, OD, RelRow, Tree, rel_type, relative_target, rels_name};
use super::{NameWalk, Plan, R, WriteError};
use crate::limits::{MAX_THEME_BYTES, MAX_THEMES};

const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// What the model's list of themes comes to against the baseline's.
#[derive(Debug, Default)]
struct Pairing {
    /// For each theme of the model, the baseline theme it is, if it is one.
    of: Vec<Option<usize>>,
    /// The baseline themes no theme of the model is.
    gone: Vec<usize>,
}

/// Pairs the themes of the model with the baseline's: by part name, then by bytes.
fn pair(base: &[&Arc<Theme>], now: &[&Arc<Theme>]) -> Pairing {
    let mut taken = vec![false; base.len()];
    let mut of: Vec<Option<usize>> = vec![None; now.len()];
    let by_name: HashMap<String, usize> = base.iter().enumerate().map(|(i, t)| (fold(&t.part), i)).collect();
    for (j, theme) in now.iter().enumerate() {
        if let Some(&i) = by_name.get(&fold(&theme.part))
            && let Some(flag) = taken.get_mut(i)
            && !*flag
        {
            *flag = true;
            if let Some(slot) = of.get_mut(j) {
                *slot = Some(i);
            }
        }
    }
    // The bytes of the baseline's themes that are still unpaired, hashed once: a model of 256 themes is not 65,536 comparisons.
    let mut by_bytes: HashMap<&[u8], Vec<usize>> = HashMap::new();
    for (i, theme) in base.iter().enumerate() {
        if !taken.get(i).copied().unwrap_or(true) {
            crate::work::theme_bytes(theme.bytes.len());
            by_bytes.entry(&theme.bytes[..]).or_default().push(i);
        }
    }
    for (j, theme) in now.iter().enumerate() {
        if of.get(j).copied().flatten().is_some() {
            continue;
        }
        crate::work::theme_bytes(theme.bytes.len());
        let Some(candidates) = by_bytes.get_mut(&theme.bytes[..]) else { continue };
        if let Some(i) = candidates.pop() {
            if let Some(flag) = taken.get_mut(i) {
                *flag = true;
            }
            if let Some(slot) = of.get_mut(j) {
                *slot = Some(i);
            }
        }
    }
    let gone = taken.iter().enumerate().filter(|(_, t)| !**t).map(|(i, _)| i).collect();
    Pairing { of, gone }
}

/// Whether `bytes` are a well-formed XML part whose root is `a:theme`.
fn is_theme_part(name: &str, bytes: &[u8]) -> bool {
    Tree::parse(name, bytes).is_ok_and(|tree| tree.root.is("theme"))
}

impl Plan<'_> {
    /// The themes of the model against the baseline's: see the module documentation.
    pub(super) fn themes(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        if base.themes == doc.themes {
            return Ok(());
        }
        if doc.themes.len() > MAX_THEMES {
            return Err(WriteError::Limit("theme parts"));
        }
        let total = doc.themes.iter().fold(0_u64, |sum, t| sum.saturating_add(t.bytes.len() as u64));
        if total > MAX_THEME_BYTES {
            return Err(WriteError::Limit("bytes of theme parts"));
        }
        let b: Vec<&Arc<Theme>> = base.themes.iter().collect();
        let n: Vec<&Arc<Theme>> = doc.themes.iter().collect();
        let pairing = pair(&b, &n);
        // Every theme that is new or rewritten is checked before anything is written.
        for (theme, paired) in n.iter().zip(&pairing.of) {
            let changed = paired.and_then(|i| b.get(i)).is_none_or(|old| old.bytes != theme.bytes);
            if changed && !is_theme_part(&theme.part, &theme.bytes) {
                return Err(WriteError::Unsupported("a theme part that is not a well-formed XML part with the root a:theme"));
            }
        }
        let document = self.document_part.clone();
        // The names and the ids of the new themes come from one walk up the numbers each (the ids' collected at the first new theme), and not from a
        // pass over the parts and the rows for each.
        let mut names = NameWalk::new(&format!("{}theme/", super::parts::folder_of(&document)), "theme", ".xml");
        let mut ids: Option<IdWalk> = None;
        for (theme, paired) in n.iter().zip(&pairing.of) {
            match paired.and_then(|i| b.get(i)) {
                Some(old) if old.bytes == theme.bytes => {}
                Some(old) => {
                    self.edited = true;
                    self.set_part(&old.part, theme.bytes.to_vec(), None);
                }
                None => {
                    self.edited = true;
                    let Some(part) = self.next_name(&mut names) else { return Err(WriteError::Limit("no part name was left for a theme")) };
                    self.reserved.insert(fold(&part));
                    self.set_part(&part, theme.bytes.to_vec(), Some(CT_THEME));
                    let rels = self.rels_mut(&document)?;
                    let walk = ids.get_or_insert_with(|| rels.id_walk());
                    let Some(id) = walk.next() else { return Err(WriteError::Limit("no relationship id was left for a theme")) };
                    let target = relative_target(&document, &part);
                    rels.add(&id, &rel_type(OD, "theme"), &target);
                    self.document_rels.push(RelRow { id, ty: rel_type(OD, "theme"), target, external: false });
                }
            }
        }
        for &i in &pairing.gone {
            if let Some(old) = b.get(i) {
                self.edited = true;
                self.drop_theme(&document, &old.part)?;
            }
        }
        Ok(())
    }

    /// Takes a theme part out: the document part's relationships of the theme type that reach it, and the part, its relationships and its
    /// `Override` if no relationship of any part still reaches it.
    fn drop_theme(&mut self, document: &str, part: &str) -> R<()> {
        let key = fold(part);
        let ty = rel_type(OD, "theme");
        if self.alive(&rels_name(document)) {
            let ids: HashSet<String> = self
                .rels_mut(document)?
                .rows()
                .into_iter()
                .filter(|r| r.ty == ty && !r.external && resolve(document, &r.target).is_some_and(|t| fold(&t) == key))
                .map(|r| r.id)
                .collect();
            if !ids.is_empty() {
                let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
                self.rels_mut(document)?.remove_ids(&ids);
            }
        }
        self.document_rels.retain(|r| !(r.ty == ty && !r.external && resolve(document, &r.target).is_some_and(|t| fold(&t) == key)));
        let reached = self.all_targets()?;
        if !reached.contains(&key) {
            self.remove_part(part);
        }
        Ok(())
    }
}
