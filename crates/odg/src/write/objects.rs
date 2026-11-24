//! The package entries that go with a deleted frame (delete; the design's "Objects
//! DiaCraft can't draw"): the entries under its object folder (`draw:object`), the file of its `draw:object-ole`, the replacement image its frame
//! shows, each when no other element of the drawing names it, and the manifest rows of each, and nothing else.
//!
//! **A reference is an `href` attribute** the scan noted (`scan::HrefAt`), and two spell one name when they are the same after percent-decoding
//! and the leading `./` and trailing `/` are taken off. A deleted shape names what its own element and everything inside it name; the
//! replacement image is the `draw:image` of a frame that also holds an object. The image goes only if nothing outside the deleted shapes names
//! it (in `content.xml` or in `styles.xml`: a background picture is a reference); a picture of an ordinary frame is not an object's and is
//! never removed by a delete (the file is changed only as far as the delete says). **Nothing structural is ever removed**, whatever a hostile
//! `href` says: not `mimetype`, the four XML parts, `manifest.rdf`, `META-INF/`, `Thumbnails/`, `Configurations2/`, `Basic/` or `Scripts/`.

use std::collections::{HashMap, HashSet};

use super::scan::{Holder, HrefAt};
use crate::package::percent_decode;

/// What the deleted shapes of one part are: the references the part holds, and which shapes of each page are gone (by the page's index in the
/// part's scan, then the shape's ordinal; index 0 is the page).
pub(crate) struct PartRefs<'a> {
    pub hrefs: &'a [HrefAt],
    pub gone: &'a HashMap<u32, Vec<bool>>,
}

/// The entries a delete drops and the rows that name them.
#[derive(Debug, Default)]
pub(crate) struct Doomed {
    /// The names of the entries to leave out.
    pub entries: HashSet<String>,
    /// The manifest rows to take out, by their paths as the manifest may write them (a folder's with its slash and without).
    pub rows: HashSet<String>,
}

/// The names the package keeps whatever a reference says.
fn structural(name: &str) -> bool {
    matches!(name, "mimetype" | "content.xml" | "styles.xml" | "meta.xml" | "settings.xml" | "manifest.rdf" | "")
        || ["META-INF", "Thumbnails", "Configurations2", "Basic", "Scripts"]
            .iter()
            .any(|dir| name == *dir || name.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/')))
}

/// An `href` as the name it spells: percent-decoded, with a leading `./` and a trailing `/` taken off.
pub(crate) fn normalised(href: &str) -> String {
    let decoded = percent_decode(href);
    let name = decoded.strip_prefix("./").unwrap_or(&decoded);
    name.trim_end_matches('/').to_owned()
}

/// Whether the reference lies in a deleted shape.
fn is_gone(refs: &PartRefs<'_>, h: &HrefAt) -> bool {
    h.shape != 0 && h.page.and_then(|p| refs.gone.get(&p)).and_then(|flags| flags.get(h.shape as usize)).copied().unwrap_or(false)
}

/// The entries of the package that the deleted shapes of `parts` take with them. `names` are the names of every entry, sorted.
pub(crate) fn doomed(parts: &[PartRefs<'_>], names: &[&str]) -> Doomed {
    // The object frames among the deleted shapes: those holding an object, by the part, page and shape.
    let mut frames: HashSet<(usize, u32, u32)> = HashSet::new();
    for (at, part) in parts.iter().enumerate() {
        for h in part.hrefs.iter().filter(|h| is_gone(part, h) && matches!(h.holder, Holder::Object | Holder::Ole)) {
            frames.insert((at, h.page.unwrap_or(0), h.shape));
        }
    }
    let mut candidates: HashSet<String> = HashSet::new();
    for (at, part) in parts.iter().enumerate() {
        for h in part.hrefs.iter().filter(|h| is_gone(part, h)) {
            let object = matches!(h.holder, Holder::Object | Holder::Ole);
            let replacement = h.holder == Holder::Image && frames.contains(&(at, h.page.unwrap_or(0), h.shape));
            if object || replacement {
                candidates.insert(normalised(&h.href));
            }
        }
    }
    // What the rest of the drawing still names, in either part.
    let kept: HashSet<String> = parts.iter().flat_map(|p| p.hrefs.iter().filter(|h| !is_gone(p, h)).map(|h| normalised(&h.href))).collect();
    let mut out = Doomed::default();
    for name in candidates.iter().filter(|n| !structural(n) && !kept.contains(*n)) {
        let folder = format!("{name}/");
        let start = names.partition_point(|n| *n < folder.as_str());
        // The entries under the folder (the folder's own entry, named with its slash, among them), and the file of that name.
        let mut found: Vec<&str> = names.get(start..).unwrap_or_default().iter().take_while(|n| n.starts_with(&folder)).copied().collect();
        let is_folder = !found.is_empty();
        if names.binary_search(&name.as_str()).is_ok() {
            found.push(name);
        }
        for entry in found {
            out.rows.insert(entry.to_owned());
            out.entries.insert(entry.to_owned());
        }
        // The folder's own row, written with its slash or without.
        if is_folder {
            out.rows.insert(folder);
            out.rows.insert(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn href(page: u32, shape: u32, holder: Holder, text: &str) -> HrefAt {
        HrefAt { page: Some(page), shape, holder, href: text.to_owned() }
    }

    fn gone(shapes: &[u32]) -> HashMap<u32, Vec<bool>> {
        let mut flags = vec![false; 8];
        for s in shapes {
            flags[*s as usize] = true;
        }
        HashMap::from([(0, flags)])
    }

    #[test]
    fn a_deleted_frames_object_folder_file_and_replacement_go_and_a_shared_one_stays() {
        let names = [
            "Object 1/content.xml",
            "Object 1/styles.xml",
            "Object 2",
            "ObjectReplacements/Object 1",
            "ObjectReplacements/Object 2",
            "Pictures/a.png",
            "Pictures/b.png",
            "content.xml",
        ];
        let hrefs = vec![
            href(0, 1, Holder::Object, "./Object 1"),
            href(0, 1, Holder::Image, "./ObjectReplacements/Object 1"),
            href(0, 2, Holder::Ole, "./Object%202"),
            href(0, 2, Holder::Image, "ObjectReplacements/Object 2"),
            href(0, 3, Holder::Image, "Pictures/a.png"),
            href(0, 3, Holder::Image, "Pictures/b.png"),
            href(0, 4, Holder::Image, "Pictures/a.png"),
            href(0, 5, Holder::Image, "ObjectReplacements/Object 2"),
        ];
        let flags = gone(&[1, 2, 3]);
        let found = doomed(&[PartRefs { hrefs: &hrefs, gone: &flags }], &names);
        let mut entries: Vec<&str> = found.entries.iter().map(String::as_str).collect();
        entries.sort_unstable();
        // Shape 5 still shows Object 2's replacement, so it stays; Object 2 itself goes with shape 2. The picture of the plain frame 3 is no object's.
        assert_eq!(entries, ["Object 1/content.xml", "Object 1/styles.xml", "Object 2", "ObjectReplacements/Object 1"]);
        assert!(found.rows.contains("Object 1/") && found.rows.contains("Object 1") && found.rows.contains("Object 2"));
        assert!(!found.rows.contains("ObjectReplacements/Object 2") && !found.rows.contains("Pictures/a.png"));
        // A picture only a deleted plain frame named stays too: it is no object's, and the package keeps what a delete is not known to own.
        assert!(!found.entries.contains("Pictures/b.png") && !found.rows.contains("Pictures/b.png"), "{found:?}");
    }

    #[test]
    fn nothing_structural_is_ever_doomed() {
        let names = ["META-INF/manifest.xml", "Thumbnails/thumbnail.png", "content.xml", "mimetype", "styles.xml"];
        let hrefs = vec![
            href(0, 1, Holder::Object, "content.xml"),
            href(0, 1, Holder::Ole, "./META-INF/manifest.xml"),
            href(0, 1, Holder::Object, "Thumbnails"),
            href(0, 1, Holder::Object, "/"),
            href(0, 1, Holder::Object, ""),
            href(0, 1, Holder::Ole, "mimetype"),
            href(0, 1, Holder::Object, "../styles.xml"),
        ];
        let flags = gone(&[1]);
        let found = doomed(&[PartRefs { hrefs: &hrefs, gone: &flags }], &names);
        assert!(found.entries.is_empty() && found.rows.is_empty(), "{found:?}");
    }
}
