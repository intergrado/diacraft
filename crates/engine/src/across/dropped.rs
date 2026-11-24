//! What a Save As left out stays out of every save into the file it wrote.
//!
//! A Save As that is asked to leave the embedded objects or the macro project out (`drop_objects`, `drop_macros`) writes a file that does not hold them, and **the drawing keeps them**: a `.diacraft`
//! saved from it afterwards holds all. Every save into that file leaves them out again, as the Save As did, and **compares the file with the drawing as it was given to the writer, never with the
//! drawing whole**: an unedited Save is then no edit, and a real edit after the Save As is written with what was left out still out.
//!
//! * [`given`]: the drawing as the writer of a VSDX file is given it for a save into a file that lacks something: the objects of an `.odg` as the pictures they show ([`super::onto::given`]) and,
//!   for a type that cannot hold a macro project, without the parts of one.
//! * [`restore`]: after a VSDX file was written from the model with something left out, **the drawing becomes what the file reads as, with what was left out put back on it**: the shapes that showed an
//!   object of an `.odg` (the file reads them as the pictures they were written as) and the parts that the file lacks (a macro project, the files of objects). The drawing then has what a read of the
//!   file gives (the identity of its pages and its shapes in the file's own parts), which the splice writer finds the file's elements by, and holds everything it held. The baseline the next save compares
//!   with is [`given`] of it, which the file's own reading is not: a shape [`given`] changed is a picture with no relationship in the one and the file's own in the other, and the splice writer refuses a
//!   change to a picture. A save into the file gives the writer [`given`] of the drawing, and the baseline is [`given`] of the drawing that save leaves, so what the two say of an unedited shape is the same
//!   by the same rule.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, Format, MAX_SHAPE_DEPTH, Master, Page, Part, Shape, ShapeList, SharedVec};

use super::hold::{Left, is_macro_part, is_object_part, is_odf_object, rewrite_list};
use super::{MAX_STEPS, Options, TOO_BIG, onto};

/// The drawing as the writer of a VSDX file is given it for a file that lacks what `asked` leaves out: the objects of an `.odg` as the pictures they show when `asked.drop_objects`, and the parts of a
/// macro project out of it when `macros_gone` (the type cannot hold one, and the Save As left it out). → the copy, and what was left out of the shapes; `doc` itself when it holds none of it.
pub(crate) fn given(doc: &Document, asked: Options, macros_gone: bool) -> Result<(Cow<'_, Document>, Left), &'static str> {
    let mut out = Cow::Borrowed(doc);
    let mut left = Left::default();
    if asked.drop_objects {
        let made = onto::given(doc, Format::Vsdx, asked)?;
        left = made.left;
        if let Some(changed) = made.doc {
            out = Cow::Owned(changed);
        }
    }
    if macros_gone && out.parts.iter().any(|part| is_macro_part(part)) {
        let mut copy = out.into_owned();
        copy.parts = SharedVec::from_vec(copy.parts.iter().filter(|part| !is_macro_part(part)).cloned().collect());
        out = Cow::Owned(copy);
    }
    Ok((out, left))
}

/// Whether a Save As to an `.odg`, asked to leave out `asked`, has something to leave out of a drawing that stands on one: the parts of a macro project of another format (a VBA project that rides in
/// the `.odg`), and the objects of another format (a VSDX embedding that rides, and the shapes that show one). What the `.odg` format holds itself is not left out of an `.odg`. **Such a Save As is
/// written from the model** (`save_odg_model`): a splice into the file cannot take out of it what it carries, and the same file written from the model leaves it out, as it does for a drawing of
/// another format. `Err` for a drawing too big to look through.
pub(crate) fn odg_leaves_out(doc: &Document, asked: Options) -> Result<bool, &'static str> {
    if asked.drop_macros && doc.parts.iter().any(|part| part.format != Format::Odf && is_macro_part(part)) {
        return Ok(true);
    }
    if asked.drop_objects {
        if doc.parts.iter().any(|part| part.format != Format::Odf && is_object_part(part)) {
            return Ok(true);
        }
        return Ok(onto::given(doc, Format::Odf, asked)?.doc.is_some());
    }
    Ok(false)
}

/// A drawing that stands on an `.odg` has the package data of the file put on it (`PackageData::put`), whose parts are the file's: **the parts that the Save As that wrote the file was asked to leave out
/// are not the file's, and the drawing keeps them.** The parts of `before` (the drawing's own) that are a macro project when `asked.drop_macros`, or the files of objects when `asked.drop_objects`, and
/// that the file does not have by name, are put back after the file's.
pub(crate) fn keep_parts(doc: &mut Document, before: &SharedVec<Arc<Part>>, asked: Options) {
    let held: HashSet<&str> = doc.parts.iter().map(|part| &*part.name).collect();
    let missing: Vec<Arc<Part>> = before
        .iter()
        .filter(|part| ((asked.drop_macros && is_macro_part(part)) || (asked.drop_objects && is_object_part(part))) && !held.contains(&*part.name))
        .cloned()
        .collect();
    if missing.is_empty() {
        return;
    }
    let mut all: Vec<Arc<Part>> = doc.parts.iter().cloned().collect();
    all.extend(missing);
    doc.parts = SharedVec::from_vec(all);
}

/// The shapes of `list`, members of groups too, that show an object of an `.odg` (`is_odf_object`), by `ID`. `Err` for a drawing too deep or too big, and when two of them have one `ID` (the
/// drawing is then not told apart by it).
fn frames_of(list: &ShapeList, steps: &mut u64) -> Result<HashMap<u32, Arc<Shape>>, &'static str> {
    let mut found: HashMap<u32, Arc<Shape>> = HashMap::new();
    let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 0)];
    while let Some(top) = stack.last_mut() {
        let level: &ShapeList = top.0;
        let Some(shape) = level.get(top.1) else {
            stack.pop();
            continue;
        };
        top.1 = top.1.saturating_add(1);
        *steps = steps.saturating_add(1);
        if *steps > MAX_STEPS {
            return Err(TOO_BIG);
        }
        if is_odf_object(shape) && found.insert(shape.id, Arc::clone(shape)).is_some() {
            return Err("two shapes of a page have one ID");
        }
        if !shape.children.is_empty() {
            if stack.len() >= MAX_SHAPE_DEPTH {
                return Err(TOO_BIG);
            }
            stack.push((&shape.children, 0));
        }
    }
    Ok(found)
}

/// `list` with each shape that `frames` has an original of (by `ID`) given back as that original, or `None` when it changes none. `restored` counts the shapes given back.
fn put_back(list: &ShapeList, frames: &HashMap<u32, Arc<Shape>>, restored: &mut u64, steps: &mut u64) -> Result<Option<ShapeList>, &'static str> {
    rewrite_list(
        list,
        &mut |shape| {
            frames.get(&shape.id).map(|original| {
                *restored = restored.saturating_add(1);
                Shape::clone(original)
            })
        },
        steps,
        0,
    )
}

/// What a read of the file written from `original` says (`back`), with what was left out put back on it: see the module documentation. `left` is what [`given`] left out of the shapes of `original`.
/// `natively` names the parts of `original` the save wrote as embeddings (`diacraft_vsdx::Written::natively`): the file holds each as the embedding made from it, which `back` holds, so it is not
/// put back (put back, the drawing held the object twice, and every later file written from it held it twice).
/// `None` when the drawing cannot be told apart from the file by its pages' and shapes' `ID`s (a shape that was left out is not found in the file, or two have one `ID`): the drawing is then kept as it is.
pub(crate) fn restore(mut back: Document, original: &Document, left: Left, natively: &[Arc<str>]) -> Option<Document> {
    let (mut steps, mut restored) = (0u64, 0u64);
    if left.shapes > 0 {
        for at in 0..back.pages.len() {
            let page = Arc::clone(back.pages.get(at)?);
            let Some(was) = original.pages.iter().find(|p| p.id == page.id) else { continue };
            let frames = frames_of(&was.shapes, &mut steps).ok()?;
            if frames.is_empty() {
                continue;
            }
            if let Some(shapes) = put_back(&page.shapes, &frames, &mut restored, &mut steps).ok()? {
                let mut copy = Page::clone(&page);
                copy.shapes = shapes;
                back.pages.set(at, Arc::new(copy)).ok()?;
            }
        }
        for at in 0..back.masters.len() {
            let master = Arc::clone(back.masters.get(at)?);
            let Some(was) = original.masters.iter().find(|m| m.id == master.id) else { continue };
            let frames = frames_of(&was.shapes, &mut steps).ok()?;
            if frames.is_empty() {
                continue;
            }
            if let Some(shapes) = put_back(&master.shapes, &frames, &mut restored, &mut steps).ok()? {
                let mut copy = Master::clone(&master);
                copy.shapes = shapes;
                back.masters.set(at, Arc::new(copy)).ok()?;
            }
        }
        if restored != left.shapes {
            return None;
        }
    }
    // The parts of a macro project and of objects that the file lacks.
    let held: HashSet<&str> = back.parts.iter().map(|part| &*part.name).collect();
    let went: HashSet<&str> = natively.iter().map(|name| &**name).collect();
    let missing: Vec<Arc<Part>> = original
        .parts
        .iter()
        .filter(|part| {
            (is_macro_part(part) || is_object_part(part))
                && !held.contains(&*part.name)
                && !(part.format != Format::Vsdx && went.contains(&*part.name))
        })
        .cloned()
        .collect();
    if !missing.is_empty() {
        let mut all: Vec<Arc<Part>> = back.parts.iter().cloned().collect();
        all.extend(missing);
        back.parts = SharedVec::from_vec(all);
    }
    Some(back)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{PartRole, RawAttrs};

    use super::*;

    fn part(format: Format, name: &str, role: PartRole, bytes: &[u8]) -> Arc<Part> {
        Arc::new(Part {
            format,
            name: Arc::from(name),
            media_type: Arc::from("application/octet-stream"),
            role,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(bytes),
        })
    }

    fn holding(parts: Vec<Arc<Part>>) -> Document {
        let mut doc = Document::empty();
        doc.parts = SharedVec::from_vec(parts);
        doc
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.parts.iter().map(|p| p.name.to_string()).collect()
    }

    const BOTH: Options = Options { drop_macros: true, drop_objects: true };

    #[test]
    fn the_union_of_two_asks_leaves_out_what_either_asks_and_nothing_is_asked_by_default() {
        let macros = Options { drop_macros: true, drop_objects: false };
        let objects = Options { drop_macros: false, drop_objects: true };
        assert_eq!(macros.union(objects), BOTH);
        assert_eq!(macros.union(macros), macros);
        assert_eq!(Options::default().union(Options::default()), Options::default());
        assert!(Options::default().is_empty() && !macros.is_empty() && !objects.is_empty() && !BOTH.is_empty());
    }

    #[test]
    fn given_takes_the_macro_parts_out_only_for_a_type_that_cannot_hold_them_and_returns_the_drawing_as_it_is_when_it_has_none() {
        let doc = holding(vec![
            part(Format::Odf, "Basic/Standard/Module1.xml", PartRole::MacroProject, b"m"),
            part(Format::Vsdx, "/visio/vbaProject.bin", PartRole::MacroProject, b"v"),
            part(Format::Odf, "Pictures/a.png", PartRole::Other, b"p"),
        ]);
        let (kept, _) = given(&doc, BOTH, false).unwrap();
        assert_eq!(names(&kept).len(), 3, "a type that holds macros keeps them");
        assert!(matches!(kept, Cow::Borrowed(_)), "and the drawing is given as it is");
        let (gone, left) = given(&doc, BOTH, true).unwrap();
        assert_eq!(names(&gone), ["Pictures/a.png"], "a macro-free type is given none of them");
        assert_eq!(left, Left::default(), "no shape showed an object");
        let none = holding(vec![part(Format::Odf, "Pictures/a.png", PartRole::Other, b"p")]);
        assert!(matches!(given(&none, BOTH, true).unwrap().0, Cow::Borrowed(_)), "a drawing with no macro part is given as it is");
    }

    #[test]
    fn restore_puts_back_the_macro_and_object_parts_the_file_lacks_and_no_other_part() {
        let original = holding(vec![
            part(Format::Odf, "Basic/Standard/Module1.xml", PartRole::MacroProject, b"m"),
            part(Format::Odf, "Object 1", PartRole::Other, b"o"),
            part(Format::Vsdx, "/visio/media/image1.png", PartRole::Other, b"i"),
        ]);
        let back = holding(vec![part(Format::Vsdx, "/visio/solutions/solutions.xml", PartRole::Other, b"s")]);
        let kept = restore(back, &original, Left::default(), &[]).unwrap();
        assert_eq!(
            names(&kept),
            ["/visio/solutions/solutions.xml", "Basic/Standard/Module1.xml", "Object 1"],
            "what the file lacks and was left out comes back after the file's"
        );
    }

    #[test]
    fn restore_keeps_the_drawing_as_it_is_when_a_shape_that_was_left_out_is_not_in_the_file() {
        let original = holding(Vec::new());
        let back = holding(Vec::new());
        let left = Left { shapes: 1, parts: 0 };
        assert!(restore(back, &original, left, &[]).is_none(), "one shape was left out and none was found to put back");
    }

    #[test]
    fn keep_parts_puts_back_only_what_the_asked_options_left_out_and_the_file_lacks() {
        let before = SharedVec::from_vec(vec![
            part(Format::Vsdx, "/visio/vbaProject.bin", PartRole::MacroProject, b"v"),
            part(Format::Vsdx, "/visio/embeddings/oleObject1.bin", PartRole::Embedding, b"o"),
            part(Format::Vsdx, "/visio/media/image1.png", PartRole::Other, b"i"),
        ]);
        let file = || holding(vec![part(Format::Odf, "Thumbnails/thumbnail.png", PartRole::Thumbnail, b"t")]);
        let mut macros = file();
        keep_parts(&mut macros, &before, Options { drop_macros: true, drop_objects: false });
        assert_eq!(names(&macros), ["Thumbnails/thumbnail.png", "/visio/vbaProject.bin"]);
        let mut objects = file();
        keep_parts(&mut objects, &before, Options { drop_macros: false, drop_objects: true });
        assert_eq!(names(&objects), ["Thumbnails/thumbnail.png", "/visio/embeddings/oleObject1.bin"]);
        let mut nothing = file();
        keep_parts(&mut nothing, &before, Options::default());
        assert_eq!(names(&nothing), ["Thumbnails/thumbnail.png"], "what was not asked for is the file's to say");
        let mut held = holding(vec![part(Format::Vsdx, "/visio/vbaProject.bin", PartRole::MacroProject, b"v")]);
        keep_parts(&mut held, &before, BOTH);
        assert_eq!(names(&held), ["/visio/vbaProject.bin", "/visio/embeddings/oleObject1.bin"], "a part the file has is not put back twice");
    }

    #[test]
    fn an_odg_save_as_has_something_to_leave_out_only_of_another_formats_macro_project_or_objects() {
        let odf = holding(vec![
            part(Format::Odf, "Basic/Standard/Module1.xml", PartRole::MacroProject, b"m"),
            part(Format::Odf, "Object 1", PartRole::Other, b"o"),
        ]);
        assert!(!odg_leaves_out(&odf, BOTH).unwrap(), "an .odg's own macro project and objects stay in an .odg");
        let riding = holding(vec![part(Format::Vsdx, "/visio/vbaProject.bin", PartRole::MacroProject, b"v")]);
        assert!(odg_leaves_out(&riding, Options { drop_macros: true, drop_objects: false }).unwrap());
        assert!(!odg_leaves_out(&riding, Options { drop_macros: false, drop_objects: true }).unwrap(), "no object asked out, and none held");
        let embedding = holding(vec![part(Format::Vsdx, "/visio/embeddings/oleObject1.bin", PartRole::Embedding, b"o")]);
        assert!(odg_leaves_out(&embedding, Options { drop_macros: false, drop_objects: true }).unwrap());
        assert!(!odg_leaves_out(&embedding, Options::default()).unwrap(), "nothing asked, nothing to leave out");
    }
}
