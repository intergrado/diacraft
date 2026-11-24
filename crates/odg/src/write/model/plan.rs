//! What the model asks of the package, and what it cannot say (the first step of the model writer; see the [module documentation](super)).
//!
//! **What an `.odg` cannot say rides in DiaCraft's own folder and document file** (`ext::items`): the parts of a VSDX file other than its thumbnail (a macro project, an embedded object,
//! custom XML, any other) and of a newer DiaCraft are entries of that folder, and a document property ODF has no field for (category, company, manager, the base of hyperlinks), the markup of a VSDX
//! document that no field reads and the package data of a VSDX file are items of the document file. **The refusals that remain are in words and none leaves anything out**: a part whose name no package of
//! its format can hold, one past the cap of an entry, and more than a file may carry. **The thumbnail of a VSDX file is no loss**: the `.odg` gets its own, made again from the drawing (design 4.4), and so
//! is the application version a VSDX file names, which is the producer's bookkeeping as the generator is.

use std::sync::Arc;

use diacraft_doc::{DocProps, Document, Format, MAX_PARTS, Part, PartRole};

use super::{repeated, reserved};
use crate::Variant;
use crate::base::{BaseLayout, NO_SIZE};
use crate::write::{WriteError, WriteOptions};

/// The words of a refusal of a drawing with no page that is not a background.
const NO_PAGE: &str = "a drawing with no page that is not a background (a package holds a page to put the drawing's pages by)";

/// The words of a refusal of parts named as the entries the writer makes, or twice.
pub(crate) const WORDS_PART_NAMES: &str = "a package part named as an entry the writer makes itself (mimetype, content.xml, styles.xml, the manifest or DiaCraft's document file), or two parts of one name";

/// What the model asks of the package.
pub(super) struct Plan<'a> {
    pub variant: Variant,
    pub layout: BaseLayout,
    /// The `ID` of the first page that is not a background: the page the package is made for is that page, so the index is given its `ID` and the splice finds the page it holds in the model.
    pub first_page_id: u32,
    /// The package entries the model holds, in order, as they are written: the model's parts of the `.odg` format but `meta.xml` and `settings.xml` (the bytes of those two stand in their places) and the thumbnails
    /// a thumbnail passed replaces.
    pub parts: Vec<&'a Arc<Part>>,
    /// `meta.xml` or `settings.xml` held as bytes, when the model could not hold them as fields.
    pub meta_part: Option<&'a Arc<Part>>,
    pub settings_part: Option<&'a Arc<Part>>,
    /// The package holds a signature: the save says it no longer matches.
    pub signed: bool,
    /// The properties a read of the package gives back.
    props: Arc<DocProps>,
}

impl Plan<'_> {
    /// The model the splice is given: the document, with the properties as the package says them.
    pub(super) fn model(&self, doc: &Document) -> Document {
        let mut model = doc.clone();
        if *model.props != *self.props {
            model.props = Arc::clone(&self.props);
        }
        model
    }
}

/// The first page that is not a background, which the package is made for.
fn first_page(doc: &Document) -> Result<&diacraft_doc::Page, WriteError> {
    doc.pages.iter().find(|p| !p.background).map(|p| &**p).ok_or(WriteError::Unsupported(NO_PAGE))
}

/// The layout of the page: its size in inches, and the way the paper is held when the page says it (`PrintPageOrientation`: 1 portrait, 2 landscape).
fn layout_of(page: &diacraft_doc::Page, variant: Variant) -> Result<BaseLayout, WriteError> {
    let number = |name: &str| page.sheet.cell(name).and_then(|c| c.value.as_ref()?.number());
    let (Some(width), Some(height)) = (number("PageWidth"), number("PageHeight")) else {
        return Err(WriteError::Unsupported(NO_SIZE));
    };
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(WriteError::Unsupported(NO_SIZE));
    }
    let (stated, landscape) = match number("PrintPageOrientation") {
        Some(1.0) => (true, false),
        Some(2.0) => (true, true),
        _ => (false, width > height),
    };
    Ok(BaseLayout { variant, width, height, landscape, stated })
}

/// The plan of the package of `doc`: see the module documentation.
pub(super) fn of<'a>(
    doc: &'a Document,
    options: &WriteOptions,
    carried: &std::collections::HashSet<(String, String)>,
) -> Result<Plan<'a>, WriteError> {
    let limits = &options.limits;
    if doc.pages.len() > crate::write::structure::most_pages(limits) {
        return Err(crate::write::structure::too_many(limits));
    }
    // More style sheets than the reader reads are no file: refused before a package is made of them.
    let most_styles = limits.max_styles.min(diacraft_doc::MAX_STYLES);
    if doc.styles.len() > most_styles {
        return Err(WriteError::Limit { what: "style sheets in the drawing", max: most_styles as u64 });
    }
    if doc.parts.len() > MAX_PARTS {
        return Err(WriteError::Limit { what: "package entries", max: MAX_PARTS as u64 });
    }
    let variant = options
        .variant
        .or_else(|| doc.ext.get().and_then(|d| d.odf.mimetype.as_deref()).and_then(Variant::from_media_type))
        .unwrap_or(Variant::Drawing);
    let first = first_page(doc)?;
    let layout = layout_of(first, variant)?;
    // The properties ODF has no field for (category, company, manager and the base of hyperlinks) ride in DiaCraft's document file, and so does everything the VSDX file held that no ODF markup says
    // The application version is the producer's, as the generator is.
    let _ = carried;
    let mut props = super::meta::odf_props(&doc.props);
    props.app_version = None;
    // The fields ODF has none for are carried by the document file, and a read of the package does not give them.
    (props.category, props.company, props.manager, props.hyperlink_base) = (None, None, None, None);
    // A field of `meta.xml` with no text is no field to the reader of it: an empty one rides in the document file (`ext::shell`), and a read of the package does not give it.
    for slot in crate::ext::shell::typed_texts_mut(&mut props) {
        if slot.as_deref() == Some("") {
            *slot = None;
        }
    }
    // The parts. Thumbnails: the one passed replaces any the model holds, unless the content is as it was read.
    let replaced = options.thumbnail.is_some() && !options.content_unchanged;
    let (mut parts, mut meta_part, mut settings_part, mut signed) = (Vec::new(), None, None, false);
    let mut bytes = 0u64;
    let mut largest = 0u64;
    for part in doc.parts.iter() {
        match part.format {
            Format::Odf => {}
            // The parts of a VSDX package and of a newer `.diacraft` ride in the entries of DiaCraft's own folder, named by the items of its document file (`ext::items`); the thumbnail of a VSDX file is
            // no loss, the `.odg` gets its own, made again from the drawing.
            Format::Vsdx | Format::Native => {
                let len = part.bytes.len() as u64;
                if part.role != PartRole::Thumbnail {
                    bytes = bytes.saturating_add(len);
                    largest = largest.max(len);
                }
                continue;
            }
        }
        if part.role == PartRole::Thumbnail && replaced {
            continue;
        }
        signed |= matches!(part.role, PartRole::Signature | PartRole::MacroSignature);
        let len = part.bytes.len() as u64;
        bytes = bytes.saturating_add(len);
        largest = largest.max(len);
        match &*part.name {
            "meta.xml" => meta_part = Some(part),
            "settings.xml" => settings_part = Some(part),
            name if reserved(name) => return Err(WriteError::Unsupported(WORDS_PART_NAMES)),
            _ => parts.push(part),
        }
    }
    if repeated(doc.parts.iter().filter(|p| p.format == Format::Odf).map(|p| &*p.name)) {
        return Err(WriteError::Unsupported(WORDS_PART_NAMES));
    }
    // The pictures of the fill images are entries of the package too, each once however many definitions share its bytes.
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for picture in doc.ext.get().into_iter().flat_map(|d| d.odf.definitions.iter()).filter_map(|d| d.bytes.as_ref()) {
        if !seen.insert(picture.as_ptr() as usize) {
            continue;
        }
        let len = picture.len() as u64;
        bytes = bytes.saturating_add(len);
        largest = largest.max(len);
    }
    // The memory the parts alone take, from the lengths they state, at the five copies a save holds at its peak (measured, at 3 GB of address space: the model's own, the package made, the read of it, and in the splice the read again and the package it makes): refused here, before a byte is made, so that the splice's own gate, which counts the same, is never the first to say.
    if bytes.saturating_mul(5).saturating_add(largest) > limits.max_write_memory {
        return Err(crate::write::too_much_memory(limits));
    }
    Ok(Plan { variant, layout, first_page_id: first.id, parts, meta_part, settings_part, signed, props: Arc::new(props) })
}
