//! The class of a warning a save gives, and the words told with it (issue 10 of the demo video: a convert to `.odg` answered eight warnings that all said `other`).
//!
//! **A warning is told by a class, a fixed word that a program can read** (`field_as_text`, `macro_project_dropped`): the first row of [`ROWS`] whose phrases are all in the warning decides, and
//! a warning that no row knows is [`OTHER`]. The phrases are chosen so that no part's name can spell them: a part's name is told as a `part_label` and holds no space, and every phrase has one.
//! Every producer of a warning is in the table: the VSDX writer's (`crates/vsdx/src/write`), the `.odg` writer's notes (`diacraft_odg::SaveNote`, every variant, which a test holds to an
//! exhaustive `match` so that a variant added is noticed here), what the engine adds to a save (the flattening of a shape, the objects left out, the style links, the new fields, the comments,
//! the picture of the first page, the stencil list) and what the `.diacraft` writer says. A test looks for every phrase in the source of the producer, so that a warning reworded there is noticed
//! here, and the tests of `convert` hold that none of the fixtures' saves is told `other`.
//!
//! **The words** are what a person reads beside the class: for a warning whose sentence holds only words and numbers (the `.odg` writer's notes, which "never hold a name from the file"; the
//! engine's own sentences, which count shapes and name them by their numbers) they are **the engine's own sentence**, which says what changed and where (`2 fields of shape 3 on page 1 ...`); for
//! a warning that may name a part, or that holds the words of an error, they are **a sentence of the class** that holds no word of the warning. `roundtrip` tells only the class: its report may
//! cross to a person who must not see the file.

use diacraft_engine::shown;

/// The class of a warning no row knows. Its words say so.
pub(crate) const OTHER: &str = "other";

/// The words of [`OTHER`]: they hold nothing of the warning.
const OTHER_WORDS: &str = "a warning this command line has no class for; the window's save note tells it in its own words";

/// The most characters of the engine's own sentence the line holds: the longest of them is about 600.
const MOST_WORDS: usize = 800;

/// How a class is told in words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Words {
    /// The engine's own sentence, which holds words and numbers and no name from the file.
    Own,
    /// A sentence of the class, with nothing of the warning in it.
    Fixed(&'static str),
}

/// A warning as `convert` tells it: its class, and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub class: &'static str,
    pub words: String,
}

/// One row of the table: the phrases that must all be in a warning, the class, and how it is told.
type Row = (&'static [&'static str], &'static str, Words);

/// Where a producer's sentences are cut by a name (`{n} parts ...`), the phrases are the fixed text between.
const ROWS: [Row; 45] = [
    // ---- the VSDX writer (`crates/vsdx/src/write`): its sentences may name a part, so each class is told in words of its own
    (
        &["saved as a stencil"],
        "stencil_keeps_pages",
        Words::Fixed(
            "a drawing saved as a stencil keeps its pages with shapes in the file, but a stencil offers its masters to other drawings, not its pages",
        ),
    ),
    (
        &["the stencil has no masters"],
        "stencil_without_masters",
        Words::Fixed("the stencil has no masters, so it offers no shapes to other drawings"),
    ),
    (
        &["the macro project was left out"],
        "macro_project_dropped",
        Words::Fixed("the macro project was left out of the new file, because the type written cannot hold one"),
    ),
    (
        &["a page or master was removed"],
        "page_or_master_removed",
        Words::Fixed("a page or master was removed: comments, published-page lists and data recordsets that name it are not edited"),
    ),
    (
        &["the file was digitally signed"],
        "signature_no_longer_holds",
        Words::Fixed("the file was digitally signed, and the signature no longer holds"),
    ),
    (
        &["digital signature(s) in this file no longer match it"],
        "signature_no_longer_holds",
        Words::Fixed("the file was digitally signed, and the signature no longer holds"),
    ),
    (
        &["the list of page names in docProps/app.xml was not found"],
        "page_name_list_not_found",
        Words::Fixed("the list of page names in the file's properties was not found, and is left as it was"),
    ),
    (
        &["was kept as it was"],
        "property_part_kept",
        Words::Fixed("a property part of the package was kept as it was, because it could not be changed"),
    ),
    (&["parts had no content type"], "parts_without_content_type", Words::Fixed("parts of the package had no content type and were given one")),
    (
        &["were not saved: the file's windows part is not XML"],
        "window_settings_not_saved",
        Words::Fixed("the window's settings were not saved: the file's windows part is not XML"),
    ),
    (
        &["would have been larger than a package allows"],
        "content_types_compacted",
        Words::Fixed(
            "the package's content types were written as their rows alone: the file's own bytes would have been larger than a package allows",
        ),
    ),
    (
        &["DiaCraft's own settings (the Snap and Glue options with no flag in the file) were not saved"],
        "diacraft_settings_not_saved",
        Words::Fixed("DiaCraft's own settings (the Snap and Glue options with no flag in the file) were not saved"),
    ),
    (
        &["DiaCraft's Snap and Glue options were not saved"],
        "snap_glue_options_not_saved",
        Words::Fixed(
            "DiaCraft's Snap and Glue options were not saved: the file's settings part holds an element that the writer could not tell from them",
        ),
    ),
    (
        &["data of the other format that this drawing carries was not saved"],
        "carried_data_not_saved",
        Words::Fixed("data of the other format that the drawing carries was not saved"),
    ),
    (&["a held part was left out"], "held_part_left_out", Words::Fixed("a part of the file that was held was left out: another part has its name")),
    (&["could not be given an id"], "relationship_without_id", Words::Fixed("a relationship of the package could not be given an id")),
    // ---- the `.odg` writer's notes (`diacraft_odg::SaveNote`, one row for each variant): words and numbers, never a name from the file
    (&["was sheared or scaled by its transform"], "shear_dropped", Words::Own),
    (&["could not be written, and the file's own is kept"], "route_not_written", Words::Own),
    (&["has no place for a connector's path"], "route_dropped_for_version", Words::Own),
    (&["meta part could not be changed"], "meta_not_updated", Words::Own),
    (&["DiaCraft's document file in the package was of an older version"], "document_file_replaced", Words::Own),
    (&["the document holds", "that this save does not write"], "document_data_unwritten", Words::Own),
    (&["new shape", " held ", "that this save does not write"], "new_shape_unwritten", Words::Own),
    (&["has an outline that is written as a rectangle"], "outline_approximated", Words::Own),
    (&["a DiaCraft id that an earlier shape holds"], "duplicate_ids", Words::Own),
    (&["so the save wrote them as plain text, and the field's element is not in the file"], "field_written", Words::Own),
    (&["is not kept: it is not the box of the group's members"], "group_frame_not_kept", Words::Own),
    (&["no element in an .odg that is read back as a field"], "field_as_text", Words::Own),
    (&["round runs of the text of shape"], "text_elements_not_written", Words::Own),
    (&["a namespace prefix that nothing binds where"], "text_markup_unbound", Words::Own),
    (&["of the formatting of the text styles of the drawing could not be written"], "style_attributes_not_written", Words::Own),
    (&["the macro project of the other format was left out of the file at your choice"], "macro_project_left_out", Words::Own),
    (&["has data an OpenDocument drawing has no place for"], "carried", Words::Own),
    (&["the drawing is signed and was changed"], "signature_stale", Words::Own),
    // ---- what the engine adds to a save (`crates/engine/src/odgfile`, `across`, `draw`, `view`, `nativefile`, `cmd/file.rs`)
    (
        &["text formatting (the character and paragraph settings", "not written to the file, which will be missing when it is opened again"],
        "text_format_not_written",
        Words::Own,
    ),
    (
        &["the link to the Theme style", "not written to the file, which will be missing when it is opened again"],
        "theme_link_not_written",
        Words::Own,
    ),
    (&["left out of the file, as asked"], "objects_left_out", Words::Own),
    (&["own values and not the links"], "style_links_not_written", Words::Own),
    (&["an OpenDocument drawing saved in place holds a field only where the file had one"], "new_fields_as_text", Words::Own),
    (&["an .odg file holds a comment's author"], "comment_data_left_out", Words::Own),
    (&["are not kept in a flat drawing (.fodg)"], "window_settings_not_in_flat_file", Words::Own),
    (&["were not recorded in the"], "stencils_not_recorded", Words::Own),
    (
        &["could not be made, so no thumbnail was written"],
        "thumbnail_not_made",
        Words::Fixed("the picture of the first page could not be made, so no thumbnail was written"),
    ),
    (
        &["picture of the first page in the file was not renewed"],
        "thumbnail_not_renewed",
        Words::Fixed("the picture of the first page in the file was not renewed"),
    ),
    (
        &["could not be made, so no preview was written"],
        "preview_not_made",
        Words::Fixed("the picture of the first page could not be made, so no preview was written"),
    ),
];

/// The first row whose phrases are all in `warning`.
fn row_of(warning: &str) -> Option<&'static Row> {
    ROWS.iter().find(|(phrases, _, _)| phrases.iter().all(|phrase| warning.contains(phrase)))
}

/// A fixed word for a warning: its class from [`ROWS`], and never a word of the warning. [`OTHER`] for a text no row knows.
pub(crate) fn warning_class(warning: &str) -> &'static str {
    row_of(warning).map_or(OTHER, |(_, class, _)| class)
}

/// A warning as `convert` tells it: its class, and the words of its row (`Words`): the engine's own sentence, cut at [`MOST_WORDS`] characters, or a sentence of the class.
pub(crate) fn told(warning: &str) -> Told {
    match row_of(warning) {
        Some((_, class, Words::Own)) => Told { class, words: shown(warning, MOST_WORDS) },
        Some((_, class, Words::Fixed(words))) => Told { class, words: (*words).to_owned() },
        None => Told { class: OTHER, words: OTHER_WORDS.to_owned() },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_odg::SaveNote;

    use super::*;

    /// The sources that give warnings: each phrase of the table is in one of them.
    const PRODUCERS: [&str; 16] = [
        include_str!("../../../crates/vsdx/src/write/mod.rs"),
        include_str!("../../../crates/vsdx/src/write/model.rs"),
        include_str!("../../../crates/vsdx/src/write/held.rs"),
        include_str!("../../../crates/vsdx/src/write/diacraft.rs"),
        include_str!("../../../crates/vsdx/src/write/settings.rs"),
        include_str!("../../../crates/vsdx/src/write/props.rs"),
        include_str!("../../../crates/odg/src/write/mod.rs"),
        include_str!("../../../crates/engine/src/odgfile/flatten.rs"),
        include_str!("../../../crates/engine/src/odgfile/save.rs"),
        include_str!("../../../crates/engine/src/across/onto.rs"),
        include_str!("../../../crates/engine/src/across/mod.rs"),
        include_str!("../../../crates/engine/src/draw/fields.rs"),
        include_str!("../../../crates/engine/src/draw/comments.rs"),
        include_str!("../../../crates/engine/src/view.rs"),
        include_str!("../../../crates/engine/src/nativefile/save.rs"),
        include_str!("../../../crates/engine/src/cmd/file.rs"),
    ];

    #[test]
    fn every_phrase_the_warnings_are_known_by_is_in_the_source_of_a_producer() {
        for (phrases, class, _) in ROWS {
            for phrase in phrases {
                assert!(
                    PRODUCERS.iter().any(|source| source.contains(phrase)),
                    "`{phrase}` ({class}) is in no warning of the producers: was one reworded?"
                );
            }
            assert!(
                phrases.iter().any(|phrase| phrase.contains(' ')),
                "{class}: a phrase with no space could be spelled by a part's label, which holds none"
            );
        }
    }

    #[test]
    fn a_class_is_one_word_that_is_not_other_and_has_the_same_words_wherever_it_is_given() {
        let mut seen: std::collections::BTreeMap<&str, Words> = std::collections::BTreeMap::new();
        for (_, class, words) in ROWS {
            assert_ne!(class, OTHER);
            assert!(class.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{class}");
            if let Some(earlier) = seen.insert(class, words) {
                assert_eq!(earlier, words, "{class} is told in two ways");
            }
            if let Words::Fixed(text) = words {
                assert!(!text.is_empty() && text.contains(' '), "{class}");
            }
        }
        // Two rows may share a class only because one class has two sentences (the signatures).
        assert!(seen.len() >= ROWS.len() - 1);
    }

    /// A note of every variant of the `.odg` writer's notes, as a value.
    fn every_note() -> Vec<SaveNote> {
        vec![
            SaveNote::SignatureStale,
            SaveNote::ShearDropped { page: 1, shape: 2 },
            SaveNote::RouteNotWritten { page: 1, shape: 2 },
            SaveNote::RouteDroppedForVersion { page: 1, shape: 2 },
            SaveNote::MetaNotUpdated,
            SaveNote::NewShapeUnwritten { page: 1, shape: 2, attributes: 3, elements: 4, data: true, foreign: true },
            SaveNote::OutlineApproximated { page: 1, shape: 2 },
            SaveNote::DuplicateIds { page: 1, shapes: vec![2, 3], count: 4 },
            SaveNote::FieldWritten { page: 1, shape: 2, element: "date" },
            SaveNote::GroupFrameNotKept { page: 1, shape: 2, glued: vec![3], glued_total: 2 },
            SaveNote::DocumentFileReplaced,
            SaveNote::DocumentDataUnwritten { definitions: 1, layers: 2, styles: 3, containers: 4 },
            SaveNote::FieldAsText { page: 1, shape: 2, fields: 3 },
            SaveNote::TextElementsNotWritten { page: 1, shape: 2, elements: 3 },
            SaveNote::TextMarkupUnbound { page: 1, shape: 2, elements: 3 },
            SaveNote::StyleAttributesNotWritten { attributes: 3 },
            SaveNote::Carried { classes: vec!["shape data"], document_level: true, macros: true, objects: true },
            SaveNote::MacroProjectLeftOut { parts: 3 },
        ]
    }

    #[test]
    fn every_note_of_the_odg_writer_has_a_class_of_its_own_and_is_told_in_its_own_words() {
        let mut classes = std::collections::BTreeSet::new();
        for note in every_note() {
            let text = note.to_string();
            let said = told(&text);
            assert_ne!(said.class, OTHER, "{text}");
            assert_eq!(said.class, note.class(), "{text}: the class is the writer's own word for the note");
            assert_eq!(said.words, text, "{text}: a note's sentence holds words and numbers only, and is told as it is");
            assert!(classes.insert(said.class), "{}: a class for two notes", said.class);
        }
    }

    #[test]
    fn every_class_word_the_odg_writer_gives_its_notes_is_a_class_here() {
        // The writer's own `class()` is an exhaustive `match`, so a variant added there has a word; this holds that each word of it is a row of the table, whether or not a test here made a sample.
        let source = include_str!("../../../crates/odg/src/write/mod.rs");
        let start = source.find("pub fn class(&self) -> &'static str").expect("the writer's `class`");
        let body = &source[start..source[start..].find("\n    }\n}").map_or(source.len(), |end| start + end)];
        let words: Vec<&str> = body.split("=> \"").skip(1).filter_map(|part| part.split('"').next()).collect();
        assert!(words.len() >= 18, "{words:?}");
        for word in words {
            assert!(
                ROWS.iter().any(|(_, class, _)| *class == word),
                "`{word}` is a note of the `.odg` writer and no row of the table gives it that class"
            );
        }
    }

    #[test]
    fn the_engine_s_own_sentences_are_classified_and_a_sentence_that_no_row_knows_is_other_and_tells_nothing_of_it() {
        for (text, class) in [
            (
                "text formatting (the character and paragraph settings a style sheet gives the text) of 13 new shapes were not written to the file, which will be missing when it is opened again (the first is shape 1 on page 1): the .odg save has no attribute for it yet",
                "text_format_not_written",
            ),
            (
                "the link to the Theme style (the file says one parent) of 2 new shapes were not written to the file, which will be missing when it is opened again (the first is shape 1 on page 1): the .odg save has no attribute for it yet",
                "theme_link_not_written",
            ),
            (
                "13 shapes of the drawing follow the style sheets of the drawing: the .odg holds their own values and not the links, so they follow no style sheet when the file is opened again",
                "style_links_not_written",
            ),
            ("2 embedded objects were left out of the file, as asked: each shape stays as the picture it showed", "objects_left_out"),
            (
                "the stencils open now were not recorded in the file: the file holds DiaCraft data of a version this build does not read, which it keeps as it is",
                "stencils_not_recorded",
            ),
        ] {
            assert_eq!(warning_class(text), class, "{text}");
            assert_eq!(told(text).words, text);
        }
        let secret = "Secret(story)~ was left out, and nobody knows why";
        let said = told(secret);
        assert_eq!((said.class, said.words.contains("Secret")), (OTHER, false), "{said:?}");
        assert_eq!(warning_class(""), OTHER);
        // A fixed sentence holds nothing of the warning, whatever the warning names.
        let named = told("/a~/b~ was kept as it was: it is not usable XML (syntax) Secret");
        assert_eq!((named.class, named.words.contains("Secret")), ("property_part_kept", false));
    }

    #[test]
    fn a_long_sentence_is_cut_at_the_most_words_and_never_panics() {
        let long = format!("2 fields of shape 1 on page 0 have no element in an .odg that is read back as a field {}", "é".repeat(5_000));
        let said = told(&long);
        assert_eq!(said.class, "field_as_text");
        assert!(said.words.chars().count() <= MOST_WORDS + 3, "{}", said.words.chars().count());
    }
    #[test]
    fn a_warning_of_the_vsdx_writer_is_told_by_its_class_whatever_part_it_names() {
        // The writer's warnings as it words them, with the labels it gives parts (`part_label`: structure by its name, anything else by its role).
        for (warning, class) in [
            (
                "saved as a stencil: the 2 pages with shapes stay in the file, but a stencil offers its masters to other drawings, not its pages",
                "stencil_keeps_pages",
            ),
            ("the stencil has no masters, so it offers no shapes to other drawings", "stencil_without_masters"),
            (
                "the macro project was left out of the new file, because a drawing cannot hold one: 2 parts removed (2 named by their owner)",
                "macro_project_dropped",
            ),
            (
                "the macro project was left out of the new file, because a drawing cannot hold one: 3 parts removed (/macros/project.bin and 2 more)",
                "macro_project_dropped",
            ),
            (
                "a page or master was removed; comments, published-page lists and data recordsets that name it are not edited",
                "page_or_master_removed",
            ),
            ("the file was digitally signed, and the signature no longer holds", "signature_no_longer_holds"),
            ("2 digital signature(s) in this file no longer match it.", "signature_no_longer_holds"),
            ("the list of page names in docProps/app.xml was not found, and is left as it was", "page_name_list_not_found"),
            ("a property part was kept as it was: it is not usable XML (syntax)", "property_part_kept"),
            ("/docProps/app.xml was kept as it was: its root is not the one a property part has", "property_part_kept"),
            ("3 parts had no content type and were given application/octet-stream", "parts_without_content_type"),
            (
                "the window's settings (grid, rulers, guides, connection points) were not saved: the file's windows part is not XML",
                "window_settings_not_saved",
            ),
            ("a relationship of the package could not be given an id", "relationship_without_id"),
            ("a held part was left out: another part has its name, which differs from it at most in case", "held_part_left_out"),
        ] {
            assert_eq!(warning_class(warning), class, "{warning}");
        }
        // A text that is none of them, or that is a name only, or that holds the words of the table cut by a name, is `other`, and tells nothing.
        for text in ["", "Secret(story)~", "/a~/b~cd was left out", "saved as a", "a stencil parts had", "someone's words: was kept"] {
            assert_eq!(warning_class(text), OTHER, "{text}");
        }
    }
}
