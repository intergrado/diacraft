//! The comments part in a save (`crate::comments` reads and builds it).
//!
//! **A save into a file** compares the model's comments with the baseline's. Equal, the part is the original's, byte for byte (and a part the model could not read stays a part held as
//! bytes, which is not this one). Different, the part the document's relationship of the type `comments` names is **rewritten** from the model (its relationship and content type as
//! they were), or, when the file has none, a part is **added**: `comments.xml` beside the document part (or the next free name), with the document's relationship to it and its
//! content type. A model whose comments were all taken away writes the part with empty lists, as a file with no comment in it holds one. The model writer's (`held.rs`) and a new
//! drawing's (the skeleton, patched like a file) come to the same part.

use diacraft_opc::name::{fold, resolve};

use super::held::CT_COMMENTS;
use super::parts::{folder_of, relative_target};
use super::{Plan, R, Tree, WriteError};
use crate::comments::{comments_type, root};

impl Plan<'_> {
    /// The model's comments against the baseline's: see the module documentation.
    pub(super) fn comments(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        if base.comments == doc.comments {
            return Ok(());
        }
        let ty = comments_type();
        let document = self.document_part.clone();
        // **Only the part the read took as the comments part is rewritten** (`Comments::part`): a relationship of the type leads wherever the file says (to a page, to the document), and a save
        // that wrote the comments over what it leads to would destroy it. With no such part, the comments are a part of their own with a relationship of their own.
        let known = base.comments.part.as_deref().map(fold);
        let found = known.and_then(|known| {
            self.document_rels
                .iter()
                .filter(|r| r.ty == ty && !r.external)
                .filter_map(|r| resolve(&document, &r.target))
                .find(|target| fold(target) == known)
        });
        match found {
            Some(name) => {
                let bytes = Tree::new(root(&doc.comments)).bytes(&name)?;
                self.set_part(&name, bytes, None);
            }
            None if doc.comments.is_empty() => return Ok(()),
            None => {
                let folder = folder_of(&document);
                let preferred = format!("{folder}comments.xml");
                let name = if !self.present.contains_key(&fold(&preferred)) && !self.reserved.contains(&fold(&preferred)) {
                    preferred
                } else {
                    self.fresh_names(folder, "comments", ".xml", 1).pop().ok_or(WriteError::Limit("no part name was left for the comments part"))?
                };
                self.reserved.insert(fold(&name));
                let bytes = Tree::new(root(&doc.comments)).bytes(&name)?;
                self.set_part(&name, bytes, Some(CT_COMMENTS));
                let target = relative_target(&document, &name);
                let rels = self.rels_mut(&document)?;
                let id = rels.next_id();
                rels.add(&id, &ty, &target);
            }
        }
        self.edited = true;
        Ok(())
    }
}
