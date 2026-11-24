//! The package data of a model, and how it follows the file.
//!
//! **What it is.** The `.odg` reader keeps in the model what a package and its parts hold beside the drawing (`read/keep.rs`): the title and the properties `meta.xml` gives, the rest of
//! `meta.xml`'s fields, `settings.xml`, the roots of the parts, `mimetype`, the manifest's rows and the entries no field reads. A model made from a file holds them **as the file had
//! them when it was read**.
//!
//! **Why it has to follow the file.** A splice save writes into the package things of its own: DiaCraft's `meta:generator`, the thumbnail, and the rows that name what it adds. The
//! session's model is not the file's reading of itself, and it cannot be made to follow at the save: every earlier version of the drawing that the history keeps holds the data as it
//! was, so an undo would bring the old data back, and a read of the whole file after every save would cost what the save does again. So **the data follows the file where a model
//! writer needs it**: [`read_package_data`] reads the file a drawing stands on, as it is now (the one it was opened from, or the one the last save wrote), and [`PackageData::put`]
//! puts that on the model that is to be written (the `.diacraft` save, and the VSDX save across the formats), whatever version of the drawing that model is. The drawing is the
//! model's; the package is the file's.
//!
//! **The document's named data goes with it**: the named definitions, the layer set, the style elements that are no style sheet, the font table and what the
//! document's containers hold that no field reads. A splice save writes into `styles.xml` and `content.xml` a `draw:marker` for each line end it writes, the page layouts and master
//! pages it makes, the automatic styles of the shapes and the text it writes and a `style:font-face` for a font a text names, and the layer set of a page it adds; the model holds all
//! of it as the file had it when it was read, and so would a `.diacraft` written after the save. The splice writer never writes the children of a container that no field reads
//! (`office:scripts` and the like), and a namespace it declares anew is on the roots (`roots` follows the file too): they follow all the same, as the file is what they are.
//!
//! **What a save writes of its own, and so what the file says that the drawing does not.** Of `meta.xml` only `meta:generator`: it says DiaCraft wrote the file (`DiaCraft/` and the
//! crate's version), and it is the save's own and no part of the drawing, so a `.diacraft` made after a save carries DiaCraft's and no longer the producer's. **A save writes no
//! other field of `meta.xml`**: the creation and modification dates, the editing cycles and duration, the statistics, the title and the user-defined fields keep their bytes
//! And so the modification date is **not** renewed by a save (a program that is to renew it does it in the properties of the model, which a save then
//! refuses to change until the writer writes them). The thumbnail is the save's own too (the picture of the first page as it is now). The rows the save adds are the file's: a row for
//! the thumbnail where the file had none, and for DiaCraft's own document file.
//!
//! **Cost.** [`read_package_data`] is a read of the file, whole: a drawing's pages cost as they do at an open, and the package's entries as many bytes as the file inflates to; what is
//! taken is a few lists of shared pointers, and the model of the read is let go of at once. [`PackageData::put`] keeps the allocation of an entry that is the same bytes (compared,
//! once, byte for byte), so that an entry the save copied is held once and only what the save wrote is new.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{Definition, DocProps, Document, FaceName, Kept, KeptStyle, Part, Passthrough, RawAttrs, RawElement, SharedVec};

use crate::error::OdgError;
use crate::limits::Limits;

/// What a model holds of its package: see the [module documentation](self). The pieces are the fields of the model that a read of the file sets from the package and its small
/// parts, and nothing of the drawing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackageData {
    /// `Document::title`, which `meta.xml`'s `dc:title` gives.
    title: Arc<str>,
    /// `Document::props`: the properties and the custom properties `meta.xml` gives.
    props: Arc<DocProps>,
    /// `OdfDoc::mimetype`.
    mimetype: Option<Arc<str>>,
    /// `OdfDoc::roots`.
    roots: Vec<(Arc<str>, RawAttrs)>,
    /// `OdfDoc::manifest_rows`.
    manifest_rows: Vec<(Arc<str>, RawAttrs)>,
    /// `OdfDoc::meta`: the fields of `meta.xml` that no property reads.
    meta: Passthrough,
    /// `OdfDoc::settings`.
    settings: Option<Arc<RawElement>>,
    /// `Document::parts`: the entries of the package that no field reads.
    parts: SharedVec<Arc<Part>>,
    /// `Document::fonts`: the font table, one face for each `style:font-face` the file declares. A save writes a declaration for a font its text names (`write/compare.rs`); the faces the
    /// session added since stay with the model ([`PackageData::put`]).
    fonts: Arc<Vec<FaceName>>,
    /// `OdfDoc::definitions`: the named markers, gradients, hatches, dashes and fill images of `office:styles`. A save adds a `draw:marker` for the line ends it writes
    /// (`write/markers.rs`).
    definitions: Vec<Definition>,
    /// `OdfDoc::layers`: the layer set of the master styles (`write/layers.rs`).
    layers: Vec<Arc<RawElement>>,
    /// `OdfDoc::styles`: the style elements that are no style sheet, whole or as what no cell reads of them: the page layouts a save writes for a page set up anew
    /// (`write/pages.rs`) and the automatic styles of the shapes and the text it writes (`write/newstyles.rs`).
    styles: Vec<KeptStyle>,
    /// `OdfDoc::kept`: the children of the document's containers that no field reads, which a save leaves as they are (the attributes of the containers are in `roots`).
    kept: Kept,
}

impl PackageData {
    /// The package data `doc` holds: shared with it, nothing copied but the lists of pointers.
    pub fn of(doc: &Document) -> PackageData {
        let odf = doc.ext.get().map(|data| &data.odf);
        PackageData {
            title: Arc::clone(&doc.title),
            props: Arc::clone(&doc.props),
            mimetype: odf.and_then(|o| o.mimetype.clone()),
            roots: odf.map(|o| o.roots.clone()).unwrap_or_default(),
            manifest_rows: odf.map(|o| o.manifest_rows.clone()).unwrap_or_default(),
            meta: odf.map(|o| o.meta.clone()).unwrap_or_default(),
            settings: odf.and_then(|o| o.settings.clone()),
            parts: doc.parts.clone(),
            fonts: Arc::clone(&doc.fonts),
            definitions: odf.map(|o| o.definitions.clone()).unwrap_or_default(),
            layers: odf.map(|o| o.layers.clone()).unwrap_or_default(),
            styles: odf.map(|o| o.styles.clone()).unwrap_or_default(),
            kept: odf.map(|o| o.kept.clone()).unwrap_or_default(),
        }
    }

    /// Puts this package data on `doc`, in place of the package data it held, and leaves the rest of it (its pages, styles, fonts and every other field of its ODF data) as it was. An
    /// entry of `doc` that is the same, bytes and all, as one of this keeps its allocation, so that what the save copied is held once.
    pub fn put(&self, doc: &mut Document) {
        doc.title = Arc::clone(&self.title);
        doc.props = Arc::clone(&self.props);
        let held: HashMap<Arc<str>, Arc<Part>> = doc.parts.iter().map(|part| (Arc::clone(&part.name), Arc::clone(part))).collect();
        let parts: Vec<Arc<Part>> = self
            .parts
            .iter()
            .map(|part| match held.get(&part.name) {
                Some(old) if **old == **part => Arc::clone(old),
                _ => Arc::clone(part),
            })
            .collect();
        doc.parts = SharedVec::from_vec(parts);
        let odf = &mut doc.ext.make_mut().odf;
        odf.mimetype.clone_from(&self.mimetype);
        odf.roots.clone_from(&self.roots);
        odf.manifest_rows.clone_from(&self.manifest_rows);
        odf.meta = self.meta.clone();
        odf.settings.clone_from(&self.settings);
        odf.definitions.clone_from(&self.definitions);
        odf.layers.clone_from(&self.layers);
        odf.styles.clone_from(&self.styles);
        odf.kept.clone_from(&self.kept);
        // The font table is the file's, and **the faces the session added since are kept**: a text formatted in a font the file does not declare adds a face to the table
        // (an edit of the drawing, which a splice save writes as a declaration beside the style that names it), and the model of a `.diacraft` must hold it though no save has
        // written it yet. The file's faces come first, as the file declares them.
        let held: std::collections::HashSet<&str> = self.fonts.iter().map(|face| &*face.name_u).collect();
        let added: Vec<FaceName> = doc.fonts.iter().filter(|face| !held.contains(&*face.name_u)).cloned().collect();
        if added.is_empty() {
            doc.fonts = Arc::clone(&self.fonts);
        } else {
            let mut table: Vec<FaceName> = self.fonts.to_vec();
            table.extend(added);
            doc.fonts = Arc::new(table);
        }
        // A model with no ODF data at all, given none, holds no slot.
        doc.ext.tidy();
    }
}

/// The package data of the file in `bytes`, as it is now: a read of the whole file within `limits` (as [`crate::read`] reads it), of which the package data is taken and the rest let
/// go. See the [module documentation](self).
pub fn read_package_data(bytes: &[u8], limits: &Limits) -> Result<PackageData, OdgError> {
    let (doc, _) = crate::read::read(bytes, limits)?;
    Ok(PackageData::of(&doc))
}
