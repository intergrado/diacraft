//! The help tree this crate is built with, held to the rules (the desktop programs embed the whole tree; the browser only the tables).

#![cfg(not(target_arch = "wasm32"))]

use diacraft_help::{Target, embedded, host};

#[test]
fn the_embedded_tree_has_the_seed_topics_and_reads_with_no_problem() {
    let bundle = embedded();
    for id in ["home", "use-help", "get-help"] {
        let topic = bundle.topic(id).unwrap_or_else(|| panic!("the embedded tree has no topic {id}"));
        assert_eq!(topic.header.id, id);
        assert!(!topic.doc.blocks.is_empty() && !topic.doc.cut_short, "{id}");
    }
    assert!(bundle.problems().is_empty(), "{:?}", bundle.problems());
}

#[test]
fn every_link_of_an_embedded_topic_goes_somewhere_that_is_there() {
    let bundle = embedded();
    let mut checked = 0usize;
    for id in bundle.ids() {
        let topic = bundle.topic(id).unwrap();
        for (_, target) in topic.links() {
            checked += 1;
            match target {
                Target::Topic { id: to, anchor } => {
                    if host::GENERATED_TOPICS.contains(&to.as_str()) {
                        assert!(anchor.is_none(), "{id}: a generated topic has no anchors");
                        continue;
                    }
                    let other = bundle.topic(to).unwrap_or_else(|| panic!("{id} links to the topic {to}, which is not there"));
                    if let Some(anchor) = anchor {
                        assert!(other.anchors().contains(&anchor.as_str()), "{id} links to {to}#{anchor}, which is not there");
                    }
                }
                Target::Anchor(anchor) => assert!(topic.anchors().contains(&anchor.as_str()), "{id} links to #{anchor}, which is not there"),
                _ => {}
            }
        }
    }
    assert!(checked > 0, "the seed topics link to each other: the walk found no link at all");
}

#[test]
fn the_three_tables_read_with_no_problem_and_the_sections_name_topics_that_are_there() {
    let bundle = embedded();
    let sections = bundle.sections().sections();
    assert!(!sections.is_empty(), "the help home lists sections");
    for section in sections {
        for id in &section.topics {
            assert!(
                bundle.topic(id).is_some() || host::GENERATED_TOPICS.contains(&id.as_str()),
                "section {} lists {id}, which is not there",
                section.title
            );
        }
    }
    // Every topic is listed by a section or is unlisted: the checker holds the real tree to this; here it holds the embedded one.
    for id in bundle.ids() {
        let listed = sections.iter().any(|s| s.topics.iter().any(|t| t == id));
        assert!(listed || bundle.topic(id).unwrap().header.unlisted, "{id} is in no section and is not unlisted");
    }
}

#[test]
fn a_topic_has_the_file_name_of_its_id_in_the_tree() {
    // `build.rs` embeds `help/topics/**/<id>.md`; the ids read from the headers are the file names.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(host::REPOSITORY_ROOT).join(host::HELP_TREE).join(host::TOPICS_DIR);
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                found.push(path.file_stem().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    let mut ids: Vec<String> = embedded().ids().into_iter().map(str::to_owned).collect();
    ids.sort();
    assert_eq!(found, ids);
}
