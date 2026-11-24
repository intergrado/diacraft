//! The patch of the parts of pages and masters on more than one thread: what an edit that touched the shapes of several pages costs is the patch of each page's tree, which are
//! independent of one another, so they are made together and put in the plan in the order the pages have. See [`super::write_with_threads`].

use diacraft_doc::{Connects, ShapeList};
use diacraft_opc::xml::{El, Node};

use super::build::{self, connects_el, shapes_el};
use super::parts::Tree;
use super::{R, WriteError, patch};
use crate::ns::Scope;

/// The most bytes that the trees of the page and master parts being patched at once may take together: a tree of a part is many times its bytes (a read of the file holds the same), and a save
/// that patches the three pages of a drawing at once holds three. A part charged more than this is patched when no other is.
const REWRITE_WINDOW: usize = 640 << 20;

/// What a tree of a part is charged, in bytes of tree for each byte of the part: measured on pages of the reader's own diagram shapes (a part of 6.6 MB took 66 MB of tree and its bytes).
const TREE_PER_PART_BYTE: usize = 10;

/// A page or master part to patch: its name, what its root must be, and the shapes and connects it had and has. What [`Plan::rewrite_all`] runs: a function of these and of the bytes of the part,
/// so that it may run on any thread.
pub(super) struct Rewrite<'r> {
    pub(super) part: String,
    pub(super) root: &'static str,
    pub(super) shapes: (&'r ShapeList, &'r ShapeList),
    pub(super) connects: (&'r Connects, &'r Connects),
    /// The new shapes name relationships the save made in the part's relationships (pictures new to the page: `page_rows::Plan::dress_new_pictures`): their `Rel`s need the prefix `r`.
    pub(super) rels_made: bool,
}

impl Rewrite<'_> {
    /// The bytes of tree that patching a part of `bytes` bytes holds, for the window of the patches in flight.
    pub(super) fn charge(&self, bytes: usize) -> usize {
        bytes.saturating_mul(TREE_PER_PART_BYTE)
    }

    /// The part patched at the shapes and connects that differ: the tree of `bytes`, with the shapes and the connects changed as [`patch`] changes them, written again.
    pub(super) fn run(&self, bytes: &[u8]) -> R<Vec<u8>> {
        #[cfg(test)]
        #[allow(clippy::panic)] // the hook of a unit test of a worker that panics
        if self.root == "Panics" {
            panic!("a part of a test that panics");
        }
        let (part, root, shapes, connects) = (self.part.as_str(), self.root, self.shapes, self.connects);
        let held_bytes = bytes.len() as u64;
        let mut tree = Tree::parse(part, bytes)?;
        // The part holds what the file has of it, and what the shapes made new add is held to what is left of what a read takes of one part.
        let _room = build::PartRoom::enter(build::tally(&tree.root.children).0, held_bytes);
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, root) {
            return Err(WriteError::Mismatch("a page or master part has another root"));
        }
        // A `Rel` the save writes names its relationship by the prefix `r`: a root that binds none is given the binding, and one that binds it elsewhere has each `Rel` declare its own
        // (`build::ForeignR`), as a part the writer makes does.
        let foreign_r = tree.root.attr("xmlns:r").is_some_and(|uri| uri != crate::ns::NS_REL);
        if self.rels_made && tree.root.attr("xmlns:r").is_none() {
            tree.root.attrs.push(("xmlns:r".to_owned(), crate::ns::NS_REL.to_owned()));
        }
        let _r = build::ForeignR::enter_foreign(self.rels_made && foreign_r);
        let prefix = tree.root.prefix().to_owned();
        let find = |root: &El, local: &str| root.children.iter().position(|n| matches!(n, Node::El(e) if sc.enter(e).is(e, local)));
        match find(&tree.root, "Shapes") {
            Some(at) => {
                if let Some(Node::El(el)) = tree.root.children.get_mut(at) {
                    let esc = sc.enter(el);
                    patch::patch_shapes(el, &esc, shapes.0, shapes.1)?;
                }
            }
            None => {
                if !shapes.0.is_empty() {
                    return Err(WriteError::Mismatch("a part lacks the shapes the model was read from"));
                }
                if !shapes.1.is_empty() {
                    // Every shape here is new to the part: one that shows a picture no relationship names is refused, as `patch_shapes` refuses it (written here, it lost its picture).
                    for shape in shapes.1.iter() {
                        patch::refuse_a_new_picture(shape)?;
                    }
                    let mut made = shapes_el(shapes.1, 0)?;
                    build::with_prefix(&mut made, &prefix);
                    tree.root.children.insert(0, Node::El(made));
                }
            }
        }
        match find(&tree.root, "Connects") {
            Some(at) => {
                if let Some(Node::El(el)) = tree.root.children.get_mut(at) {
                    let esc = sc.enter(el);
                    patch::patch_connects(el, &esc, connects.0, connects.1)?;
                }
            }
            None => {
                if !connects.0.is_empty() {
                    return Err(WriteError::Mismatch("a part lacks the connects the model was read from"));
                }
                if !connects.1.is_empty() {
                    let mut made = connects_el(connects.1);
                    build::with_prefix(&mut made, &prefix);
                    tree.root.children.push(Node::El(made));
                }
            }
        }
        tree.bytes(part)
    }
}

/// The parts of `work` (a job and the bytes of its part) patched on up to `threads` threads, in the order of `work`: the first that failed is the error, and a worker that panicked is
/// [`WriteError::Worker`] in its place. The trees in flight are held to [`REWRITE_WINDOW`].
pub(super) fn patch_together(threads: usize, work: &[(&Rewrite<'_>, &[u8])]) -> R<Vec<Vec<u8>>> {
    let mut tasks: Vec<diacraft_container::par::Ordered<'_, R<Vec<u8>>>> = Vec::new();
    tasks.try_reserve_exact(work.len()).map_err(|_| WriteError::Limit("the parts to patch do not fit in memory"))?;
    for &(job, bytes) in work {
        tasks.push(diacraft_container::par::Ordered { charge: job.charge(bytes.len()), task: Box::new(move || job.run(bytes)) });
    }
    let mut outs: Vec<Vec<u8>> = Vec::new();
    outs.try_reserve_exact(work.len()).map_err(|_| WriteError::Limit("the parts to patch do not fit in memory"))?;
    for result in diacraft_container::par::run_ordered(threads, REWRITE_WINDOW, tasks) {
        outs.push(result.map_err(|_| WriteError::Worker("patched a page or master part"))??);
    }
    Ok(outs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> (Vec<u8>, Vec<u8>) {
        let ns = crate::ns::NS_2012;
        (format!("<PageContents xmlns=\"{ns}\"/>").into_bytes(), format!("<PageContents xmlns=\"{ns}\"><!-- two --></PageContents>").into_bytes())
    }

    fn job<'r>(root: &'static str, shapes: &'r ShapeList, connects: &'r Connects) -> Rewrite<'r> {
        Rewrite { part: "/visio/pages/page1.xml".to_owned(), root, shapes: (shapes, shapes), connects: (connects, connects), rels_made: false }
    }

    #[test]
    fn a_worker_that_panics_is_an_error_of_the_save_in_words_and_the_parts_before_it_do_not_matter() {
        let (shapes, connects) = (ShapeList::default(), Connects::default());
        let ok = job("PageContents", &shapes, &connects);
        let bad = job("Panics", &shapes, &connects);
        let (empty, _) = parts();
        for threads in [2, 7, 31] {
            let work = [(&ok, empty.as_slice()), (&bad, empty.as_slice()), (&ok, empty.as_slice())];
            let error = patch_together(threads, &work).err();
            assert!(matches!(error, Some(WriteError::Worker(what)) if what.contains("patched a page")), "{threads} threads: {error:?}");
            let words = error.map(|e| e.to_string()).unwrap_or_default();
            assert!(words.contains("a worker thread of the save stopped"), "{words}");
        }
    }

    #[test]
    fn parts_patched_together_come_back_in_the_order_they_were_given() {
        let (shapes, connects) = (ShapeList::default(), Connects::default());
        let ok = job("PageContents", &shapes, &connects);
        let (empty, noted) = parts();
        let (a, b) = (empty.as_slice(), noted.as_slice());
        let work = [(&ok, a), (&ok, b), (&ok, a), (&ok, b), (&ok, a)];
        let one = patch_together(1, &work).expect("patched");
        let many = patch_together(31, &work).expect("patched");
        assert_eq!(one, many);
        assert_eq!(one.len(), 5);
        assert_ne!(one.first(), one.get(1), "the two parts are not the same bytes");
    }
}
