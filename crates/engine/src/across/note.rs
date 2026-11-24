//! The save note (the native-format design's section 5.6): what a save across the formats tells a person about the data the file carries.
//!
//! **The result.** A save to a foreign format may need up to three parts of a note, each shown only when it applies. The save result has a `carried` field that holds the classes carried, whether
//! document-level data is among them, whether a macro project or objects are, and the number of digital signatures that no longer match. **Programs (the CLI and MCP) get it as a structured field,
//! never as a warning** (ODF decision 8); the window words it with [`words`], which is the design's text, exactly.
//!
//! **Class names are plain words.** The vocabulary is [`classes::VOCABULARY`](super::classes::VOCABULARY): "line ends", "gradients", "hatches", "glue directions", "layer settings", "style names",
//! "document properties", "settings", "macro project", "embedded objects", "window settings" and "unknown parts" (the design's), and a few more for what it does not list ("dash patterns",
//! "fill images", "shape data", "page data", "style data", "document data", "digital signatures", "package details", "page layouts").

use serde_json::{Value, json};

/// The class a shape's own carried data is in: it rides on the shape's own style. Every other class rides in DiaCraft's document data (`document_level`).
pub const SHAPE_DATA: &str = "shape data";

/// What a save carried (see the module documentation): the `carried` field of the save result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// The classes of data the file carries, in plain words, in the order of the vocabulary. Empty when nothing is carried.
    pub classes: Vec<String>,
    /// Some of it is in DiaCraft's document data (the document file and folder of an `.odg`, the solution part of a VSDX file) and not on shapes.
    pub document_level: bool,
    /// A macro project rides as bytes. It is never parsed and never run.
    pub macros: bool,
    /// Embedded objects ride as bytes.
    pub objects: bool,
    /// How many digital signatures in the file no longer match it (the parts a signature covers always differ after a save from the model).
    pub signatures_stale: u32,
}

impl Carried {
    /// Nothing carried: no note.
    pub fn none() -> Carried {
        Carried::default()
    }

    /// Whether there is nothing to say: no class, no macro project, no objects and no signature.
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty() && !self.macros && !self.objects && self.signatures_stale == 0
    }

    /// The structured field: `{classes: [..], document_level, macros, objects, signatures_stale}`.
    pub fn to_json(&self) -> Value {
        json!({
            "classes": self.classes,
            "document_level": self.document_level,
            "macros": self.macros,
            "objects": self.objects,
            "signatures_stale": self.signatures_stale,
        })
    }

    /// The field as a save result gives it, or `None` for a value that is not one. A frontend reads the field with this and words it with [`words`]; it never parses a warning.
    pub fn from_json(value: &Value) -> Option<Carried> {
        let classes = value.get("classes")?.as_array()?.iter().map(|c| c.as_str().map(str::to_owned)).collect::<Option<Vec<String>>>()?;
        Some(Carried {
            classes,
            document_level: value.get("document_level")?.as_bool()?,
            macros: value.get("macros")?.as_bool()?,
            objects: value.get("objects")?.as_bool()?,
            signatures_stale: u32::try_from(value.get("signatures_stale")?.as_u64()?).ok()?,
        })
    }
}

/// The format a file was written in: which words the note says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// One of the six VSDX types.
    Vsdx,
    /// An OpenDocument drawing (`.odg`) or template (`.otg`).
    Odg,
}

/// The note's words for a save, one string for each part that applies, in the design's order (the carried data, the macro project and the objects, the signatures): **the design's 5.6, exactly**.
/// Empty when `carried` has nothing to say: with nothing to say, there is no note.
pub fn words(target: Target, carried: &Carried) -> Vec<String> {
    let mut out = Vec::new();
    if !carried.classes.is_empty() {
        let classes = carried.classes.join(", ");
        out.push(match target {
            Target::Vsdx => format!(
                "This drawing has data a VSDX file has no place for ({classes}). DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."
            ),
            // **What another program does with the data is said as it was measured**: a LibreOffice save kept DiaCraft's own entries byte for byte in 129 of 145 null saves
            // Because the writer lists DiaCraft's folder in the manifest, so the note does not say it leaves them out; another program may keep them or not, and what it changes
            // in the drawing wins where DiaCraft can tell (an item stands only while its stamp holds).
            Target::Odg => format!(
                "This drawing has data an OpenDocument drawing has no place for ({classes}). DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell."
            ),
        });
    }
    let what = match (carried.macros, carried.objects) {
        (true, true) => Some("a macro project and embedded objects"),
        (true, false) => Some("a macro project"),
        (false, true) => Some("embedded objects"),
        (false, false) => None,
    };
    if let Some(what) = what {
        let programs = match target {
            Target::Vsdx => "VSDX",
            Target::Odg => "OpenDocument",
        };
        out.push(format!(
            "This file carries {what} that {programs} programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
        ));
    }
    if carried.signatures_stale > 0 {
        out.push(format!("{} digital signature(s) in this file no longer match it.", carried.signatures_stale));
    }
    out
}
