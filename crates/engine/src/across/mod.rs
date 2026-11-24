//! Saving across the VSDX and OpenDocument formats (the native-format design's sections 5.1, 5.6 and 6; harvested from the shelved the `across/`).
//!
//! A drawing read from a VSDX file is saved as an `.odg` or an `.otg`, and a drawing read from an `.odg` as one of the six VSDX types, **by the model writer of the target**: it writes the
//! file from the model alone, and what the target cannot say rides in its carrier (VSDX's solution part and `/diacraft/` parts, the `.odg`'s DiaCraft document file and folder). **Nothing is
//! stripped**: the `strip.rs` emptied every `raw` and `passthrough` on the way across, and the model now holds each format's data tagged and routed instead (the design's 3.2).
//!
//! * [`hold`]: **what the drawing holds** of the two things a person may choose to leave out, **macro projects** and **embedded objects**, and the model without them ([`hold::without_objects`]).
//!   They are **options, never requirements** (`drop_macros`, `drop_objects`: the design's Owner 9): a macro project of either language goes into a VSDX file only of a macro-enabled type
//!   (Call 17), and a save to a macro-free type without `drop_macros` is refused in words that name the macro-enabled types as the other way out.
//! * [`onto`]: the model a model writer is given for a drawing of the other format: the objects left out when asked, and the cells a VSDX writer needs to draw a picture an `.odg` held.
//! * [`natively`]: **the objects a VSDX file holds once, as embeddings**: a save into the file is given the drawing, and the baseline, without the parts the embeddings were written from.
//! * [`classes`]: **what the file carries**, in plain words: the classes of data a VSDX file has no place for ("line ends", "gradients", ...), found by one counted walk of the model.
//! * [`note`]: **the save note** (the design's 5.6): the structured `carried` field of the save result, never a warning (ODF decision 8), and its words, exactly.
//!
//! **The model after a save across to VSDX.** The file written is the drawing's own from then on (Save As switches the working file, Call 15), and the splice writer compares the drawing with the
//! file as it reads. A drawing read from an `.odg` holds an object frame where the file holds an embedding, so the next save into the file would refuse "a change to the picture of a shape":
//! the save **adopts the model the file reads as** (`Session::adopt_saved`) when that holds everything the drawing did, which the read's own report says (every item came back) and the options
//! left nothing out. Otherwise the drawing is kept as it is, and a later save into the file says in words what it cannot write. **Which drawings** ([`adopts_by_working_file`]): one read
//! from an `.odg`, and a `.diacraft` drawing or one with no working file (a new one, or one made from a template) when the writer was given pictures an `.odg` held; never a drawing that holds none, or one read
//! from a VSDX file.

pub(crate) mod classes;
pub(crate) mod dropped;
pub(crate) mod hold;
pub(crate) mod natively;
pub mod note;
pub(crate) mod onto;

pub use note::{Carried, Target, words};

/// The most shapes, cells and rows one walk of a drawing in this module visits, and refuses past (counted in the units walked, as every walk of the model is).
pub(crate) const MAX_STEPS: u64 = 60_000_000;

/// The words of a walk that was refused for its size.
pub(crate) const TOO_BIG: &str = "the drawing is too big to save to the other format in one step";

/// What a save across may be asked (the options of `file.save_as`, the CLI's `convert` and the MCP tool): leave the macro projects and the embedded objects out of the file written. The drawing
/// (and a `.diacraft` file saved from it) keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Leave the macro project of the other format out of the file written, or of a type that cannot hold one.
    pub drop_macros: bool,
    /// Leave the embedded objects out of the file written. Each shape that showed one shows its picture.
    pub drop_objects: bool,
}

impl Options {
    /// What either of the two asks: a file that a Save As wrote without the objects and a later Save As that asks for the macros out is a file without both.
    #[must_use]
    pub fn union(self, other: Options) -> Options {
        Options { drop_macros: self.drop_macros || other.drop_macros, drop_objects: self.drop_objects || other.drop_objects }
    }

    /// Whether the options ask for nothing to be left out.
    pub fn is_empty(self) -> bool {
        self == Options::default()
    }
}

/// What a VSDX file written from the model carries, for the save result (`carried`): the classes of the data of the other format that rides in it (found in `doc`, the model the writer was given),
/// and whether a macro project or objects do, as the writer says. **Nothing carried is unnamed**: where the writer carried something that no finer class names, the class is "document data".
/// `signatures` is how many digital signatures the file holds that no longer match it.
pub(crate) fn carried_into_vsdx(doc: &diacraft_doc::Document, writer: diacraft_vsdx::Carried, signatures: u32) -> Result<Carried, &'static str> {
    let rides = writer.items > 0 || writer.parts > 0;
    let mut found = if rides { classes::into_vsdx(doc, writer.macros, writer.objects)? } else { classes::Classes::default() };
    if rides && found.is_empty() {
        found.catch_all();
    }
    let words = found.words();
    let document_level = words.iter().any(|w| !classes::is_shape_class(w));
    Ok(Carried { classes: words, document_level, macros: rides && writer.macros, objects: rides && writer.objects, signatures_stale: signatures })
}

/// What an `.odg` written from the model carries, for the save result (`carried`): what the `.odg` writer says it carried (`SaveNote::Carried`, its words made plain), the window settings the
/// drawing holds, and the signatures that no longer match.
pub(crate) fn carried_into_odg(doc: &diacraft_doc::Document, notes: &[diacraft_odg::SaveNote], signatures: u32) -> Carried {
    let mut found = classes::into_odf(doc);
    let (mut macros, mut objects) = (false, false);
    for note in notes {
        if let diacraft_odg::SaveNote::Carried { classes: said, macros: carried_macros, objects: carried_objects, .. } = note {
            classes::from_writer(said.iter().copied(), &mut found);
            macros = *carried_macros;
            objects = *carried_objects;
        }
    }
    let words = found.words();
    let document_level = words.iter().any(|w| !classes::is_shape_class(w));
    Carried { classes: words, document_level, macros, objects, signatures_stale: signatures }
}

/// **A save across to a VSDX type refuses what a save of the drawing as an `.odg` refuses**, for a drawing that holds the data of an `.odg` (it was read from one, or from a `.diacraft`
/// file made of one): **the `.odg` model writer's own rule** (`diacraft_odg::model_refusal`), which that writer calls too, so the two cannot drift. An evidence run found the crossing (`.odg` to `.diacraft`
/// to VSDX to `.diacraft` to `.odg`) finishing in 65 trips that the plain visit refuses, and losing frames, pictures, tables, objects and paragraphs with no word. A VSDX drawing (one that holds no
/// `.odg`'s data) is no crossing of this kind and is not judged here: its pictures are the VSDX writer's to write. → the words of the refusal, with the way out.
pub(crate) fn crossing_refusal(doc: &diacraft_doc::Document) -> Option<String> {
    let holds_odf = doc.ext.get().is_some_and(|data| data.odf != diacraft_doc::OdfDoc::default());
    if !holds_odf {
        return None;
    }
    diacraft_odg::model_refusal(doc).map(|why| {
        format!(
            "the drawing could not be saved: a save across from an OpenDocument drawing does not write what a save of it as an .odg does not: {why}. Nothing was written. A .diacraft file holds everything the drawing does: save it as .diacraft, or take out what an .odg cannot say."
        )
    })
}

/// Whether the drawing becomes the model a VSDX file written from it reads as (`Session::adopt_saved`): **only when that holds everything the drawing did**. The read's own report says it: every item
/// of carried data came back (none stale, none gone, none left out), and the caller's options left nothing out (`drop_macros` and `drop_objects` leave out what the drawing keeps).
pub(crate) fn adopts(written: &diacraft_vsdx::Written, given: &onto::Given) -> bool {
    given.left == hold::Left::default() && written.macros_dropped.is_empty() && came_back(written)
}

/// **Which drawings a save across to a VSDX type may make the model its file reads as**, the part of the gate of `file.save_as` that is about the drawing's own working file (the rest is
/// [`adopts`] and the check after the save).
///
/// * A drawing read from an **`.odg`**: always: every frame of it is written as something a VSDX file says in another way.
/// * A drawing of a **`.diacraft`** file, or **with no working file at all** (a new one, or one made from a template), **when the writer was given pictures an `.odg` held, in the box a
///   picture has** ([`onto::Given::boxed`]): the file reads back with the cells of that box and the drawing does not hold them, so a Save into the file was refused "a change to the picture of a
///   shape". A drawing that holds no such picture is written as the drawing holds it, and its file is what the drawing is: **a blank new drawing, whatever type it is saved as, is never made another
///   model** (it holds nothing of an `.odg`, so `boxed` is 0). This is the narrowest condition that closes the case: it asks for nothing but the pictures themselves. A drawing made from an `.otg`
///   template holds the frames of an `.odg`, and has no working file.
/// * A drawing of a **VSDX** file is never made another model: its file is its own, spliced into.
pub(crate) fn adopts_by_working_file(working: Option<crate::file::FileFormat>, given: &onto::Given) -> bool {
    use crate::file::FileFormat;
    match working {
        Some(FileFormat::Odg) => true,
        Some(FileFormat::Diacraft) | None => given.boxed > 0,
        Some(FileFormat::Vsdx) => false,
    }
}

/// Whether every item of carried data came back in the read of the file written (none stale, none gone, none left out of it): what is left of the gate of [`adopts`] when the options of the save left
/// something out, which the drawing then keeps beside what the file reads as ([`dropped`]).
pub(crate) fn came_back(written: &diacraft_vsdx::Written) -> bool {
    let carry = &written.read_back.carry;
    carry.stale == 0 && carry.gone == 0 && carry.left_out == 0 && carry.kept == carry.items
}

/// **The links of the shapes of a VSDX file to its style sheets are not carried by an `.odg`**, and the save says so ("no flatten loss is silent"). The engine hands the `.odg` writer each
/// shape of a VSDX drawing as a new shape of an `.odg` is (`odgfile::flatten`): what a style sheet gives it is its own value, so that another program draws it as the VSDX file does, and the link to the
/// style sheet is the root's. The drawing keeps the links (a `.diacraft` file saved from it holds them); the `.odg` holds the values. The words are for a drawing that came from a VSDX file (it holds the
/// package data of one) and has shapes that follow a style sheet other than the root; `None` otherwise.
pub(crate) fn style_link_words(doc: &diacraft_doc::Document) -> Result<Option<String>, &'static str> {
    if !doc.vsdx.as_ref().is_some_and(|held| !held.rels.is_empty()) {
        return Ok(None);
    }
    let mut following = 0u64;
    hold::each_shape(doc, false, |shape| {
        let links = shape.sheet.links;
        following = following.saturating_add(u64::from([links.line, links.fill, links.text].iter().any(|link| link.is_some_and(|id| id != 0))));
    })?;
    Ok((following > 0).then(|| {
        format!(
            "{following} shape{} of the drawing follow{} the style sheets of the drawing: the .odg holds {} own values and not the links, so {} follow{} no style sheet when the file is opened again (the drawing keeps them; save it as .diacraft to keep them in a file)",
            if following == 1 { "" } else { "s" },
            if following == 1 { "s" } else { "" },
            if following == 1 { "its" } else { "their" },
            if following == 1 { "it" } else { "they" },
            if following == 1 { "s" } else { "" },
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::FileFormat;

    fn given(boxed: u64) -> onto::Given {
        onto::Given { doc: None, left: hold::Left::default(), boxed }
    }

    /// **A drawing with no pictures of an `.odg` is never made another model by a save** (a new drawing is saved as it is), and a `.diacraft` drawing, or one with no working
    /// file (made from a template), is when the writer was given pictures an `.odg` held (`boxed`). Over every working file and a drawing with none, some and many such pictures.
    #[test]
    fn only_an_odg_drawing_and_a_drawing_of_odg_pictures_become_the_model_their_vsdx_file_reads_as() {
        for boxed in [0, 1, 7, u64::MAX] {
            let given = given(boxed);
            assert!(adopts_by_working_file(Some(FileFormat::Odg), &given), "an .odg drawing, {boxed} pictures");
            assert_eq!(adopts_by_working_file(Some(FileFormat::Diacraft), &given), boxed > 0, "a .diacraft drawing, {boxed} pictures");
            assert_eq!(adopts_by_working_file(None, &given), boxed > 0, "a drawing with no file (new, or made from a template), {boxed} pictures");
            assert!(!adopts_by_working_file(Some(FileFormat::Vsdx), &given), "a VSDX drawing's file is its own, {boxed} pictures");
        }
    }
}
