//! The package the splice is given (the second step of the model writer; see the [module documentation](super)): the parts of [`crate::base`], `meta.xml`, `settings.xml` and the manifest written
//! from the model, and the model's own package entries put in as they are.
//!
//! **The order of the entries** is: `mimetype` (first, stored, with no extra field), `content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, the model's entries in the order it holds them,
//! and the manifest last (the splice adds the pictures, the thumbnail and DiaCraft's document file before it). A folder is a folder entry with no data; an entry the model holds as bytes of
//! `meta.xml` or `settings.xml` stands in the place of the part written from fields.
//!
//! **Compression.** An entry is deflated, or stored when deflate does not make it smaller ([`diacraft_container::zip::ArchiveWriter::add_smallest`]). An entry of [`TRIAL`] bytes or more is first deflated on
//! its own, and **stored instead when deflate would pass the container's ratio** (a package whose entry inflates a thousand times over is refused by the reader, and the model holds such bytes
//! when the file it came from stored them): the bytes of a package this writer makes are always bytes the reader reads.

use diacraft_container::limits::MAX_RATIO;
use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Document, Part};

use super::plan::Plan;
use super::{containers, manifest, meta, settings};
use crate::limits::Limits;
use crate::package::{MANIFEST_PATH, MIMETYPE_PATH};
use crate::write::{WriteError, WriteOptions, WriteWork};

/// The size from which an entry is deflated on its own first, to find out whether its ratio is one the reader reads: deflate cannot pass a ratio of 1,032, and an entry under this size cannot
/// come near it (a stream of zeros of this size deflates to more than a thousandth of itself).
const TRIAL: usize = 128 * 1024;

/// The entry `name` written into `writer`: deflated, or stored where deflate does not make it smaller or would make a ratio the reader refuses.
fn add(writer: &mut ArchiveWriter, name: &str, data: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    work.charge_copy(data.len(), limits)?;
    if data.len() >= TRIAL {
        // The entry deflated alone, in an archive of its own: the bytes it comes to say the ratio.
        let mut trial = ArchiveWriter::new();
        let method = trial.add_smallest(name, data).map_err(WriteError::Package)?;
        let compressed = trial.written_bytes().saturating_sub(30).saturating_sub(name.len());
        let past = method == Method::Deflated && (data.len() as u64) > MAX_RATIO.saturating_mul(compressed as u64);
        drop(trial);
        let method = if past { Method::Stored } else { method };
        return writer.add(name, data, method).map_err(WriteError::Package);
    }
    writer.add_smallest(name, data).map(|_| ()).map_err(WriteError::Package)
}

/// One of the model's entries into `writer`.
fn put_part(writer: &mut ArchiveWriter, part: &Part, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if part.name.ends_with('/') {
        return writer.add_directory(&part.name).map_err(WriteError::Package);
    }
    add(writer, &part.name, &part.bytes, limits, work)
}

/// The package made, and what the model holds of the document that it was not given, by kind (pieces of the containers it had no place for, and pieces whose prefixes nothing binds where
/// they would go).
pub(super) struct Built {
    pub bytes: Vec<u8>,
    pub left: super::document::Left,
}

/// The package of the plan: see the module documentation.
pub(super) fn build(doc: &Document, plan: &Plan<'_>, options: &WriteOptions, work: &mut WriteWork) -> Result<Built, WriteError> {
    let limits = &options.limits;
    // Each entry of the model is a step of the save: its row, its entry and its name.
    work.charge_steps(plan.parts.len())?;
    let mut base = crate::base::parts(&plan.layout)?;
    // A drawing with a language and no default style of its own says its language on the default graphic style: the package it is written onto starts with it.
    if let Some(style) = crate::lang::default_style(doc) {
        base.styles = base.styles.replacen("<office:styles></office:styles>", &format!("<office:styles>{style}</office:styles>"), 1);
    }
    // `meta.xml` and `settings.xml`: the bytes the model holds for them when it holds them as bytes, else written from its fields.
    let meta_own = if plan.meta_part.is_none() { Some(meta::write(doc, options.content_unchanged, limits, work)?) } else { None };
    let settings_own = if plan.settings_part.is_none() {
        Some(settings::write(doc, limits, work)?.unwrap_or_else(|| base.settings.clone().into_bytes()))
    } else {
        None
    };
    let meta_bytes: &[u8] = plan.meta_part.map_or_else(|| meta_own.as_deref().unwrap_or_default(), |part| &part.bytes);
    let settings_bytes: &[u8] = plan.settings_part.map_or_else(|| settings_own.as_deref().unwrap_or_default(), |part| &part.bytes);
    // The parts first: the pictures of the fill images that `styles.xml` names are entries of the package, each with its row.
    let mut fills = super::document::FillNames::of(doc);
    let content = containers::dress("content.xml", &base.content, doc, limits, work, &mut fills)?;
    // The layer set the drawing's layers call for, put into the part made.
    let styles = super::layer_set::declare(containers::dress("styles.xml", &base.styles, doc, limits, work, &mut fills)?, doc, limits, work)?;
    let fill_rows =
        fills.made.iter().map(|p| manifest::Row { path: p.name.clone(), attrs: vec![("manifest:media-type".to_owned(), p.media.clone())] });
    let rows = manifest::rows(doc, plan.variant.media_type(), plan.parts.iter().filter_map(|p| manifest::part_row(p)).chain(fill_rows));
    let manifest_bytes = manifest::write(doc, &rows, limits, work)?;
    let mut writer = ArchiveWriter::new();
    // The media type first, stored, with no extra field.
    writer.add(MIMETYPE_PATH, plan.variant.media_type().as_bytes(), Method::Stored).map_err(WriteError::Package)?;
    add(&mut writer, "content.xml", &content.bytes, limits, work)?;
    add(&mut writer, "styles.xml", &styles.bytes, limits, work)?;
    add(&mut writer, meta::PART, meta_bytes, limits, work)?;
    add(&mut writer, settings::PART, settings_bytes, limits, work)?;
    for part in &plan.parts {
        put_part(&mut writer, part, limits, work)?;
    }
    for picture in &fills.made {
        add(&mut writer, &picture.name, &picture.bytes, limits, work)?;
    }
    add(&mut writer, MANIFEST_PATH, &manifest_bytes, limits, work)?;
    let bytes = writer.finish().map_err(WriteError::Package)?;
    Ok(Built { bytes, left: content.left.plus(styles.left) })
}
