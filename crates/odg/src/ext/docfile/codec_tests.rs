//! The document file's version, the depth it is parsed at, the namespace a master declares, the memory a save holds for its trees, and the depth of the file
//! it makes.

use super::*;

fn root(version: Option<&str>) -> El {
    El {
        name: "diacraft:document".to_owned(),
        attrs: version.map(|v| ("version".to_owned(), v.to_owned())).into_iter().collect(),
        children: Vec::new(),
    }
}

/// The age of a document file's version: the one this build writes is current, a canonical number below it is old, and anything else (a higher number, a word, a number that
/// is not written canonically, none at all) is another, which is read for the names this build knows.
#[test]
fn a_files_version_is_current_old_or_another() {
    assert_eq!(VERSION, "2", "the version this build writes");
    assert_eq!(Age::of(&root(Some("2"))), Age::Current);
    assert_eq!(Age::of(&root(Some("1"))), Age::Old(1));
    assert_eq!(Age::of(&root(Some("0"))), Age::Old(0));
    for other in [Some("3"), Some("9"), Some("02"), Some("+1"), Some("1.0"), Some(""), Some("x"), Some("-1"), Some("4294967296"), None] {
        assert_eq!(Age::of(&root(other)), Age::Other, "{other:?}");
    }
}

/// The document file is parsed at the container's ceiling whatever the caller's default depth says, and a caller's own higher depth stays; no other limit moves.
#[test]
fn the_document_file_is_parsed_at_the_depth_the_codec_needs() {
    let ceiling = diacraft_container::limits::XML_DEPTH_CEILING;
    let default = crate::Limits::default();
    assert!(default.container.xml.max_depth < ceiling, "the container's default depth is below what the codec's masters need");
    let at = parse_limits(&default);
    assert_eq!(at.max_depth, ceiling);
    assert_eq!((at.max_nodes, at.max_attr_bytes), (default.container.xml.max_nodes, default.container.xml.max_attr_bytes), "and nothing else moves");
    let mut deeper = crate::Limits::default();
    deeper.container.xml.max_depth = ceiling + 100;
    assert_eq!(parse_limits(&deeper).max_depth, ceiling + 100, "a caller's higher depth stays");
}

fn element(name: &str, attrs: &[(&str, &str)]) -> Node {
    Node::El(El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children: Vec::new() })
}

/// A master that does not declare DiaCraft's namespace is told so, with the attribute that declares it under the prefix it is written with (or the default one): not a master
/// that declares it under its own prefix, whatever it is bound to. **The declaration is put on the master being read and on no other** (the reader's `read_master`): a file of
/// many masters is not walked once to give each one before any is charged.
#[test]
fn a_master_with_no_declaration_is_told_which_one_it_lacks_and_one_that_has_it_is_left_alone() {
    let urn = names::URN;
    let lacks = |name: &str, attrs: &[(&str, &str)]| match element(name, attrs) {
        Node::El(e) => body::missing_declaration(&e),
        other => panic!("{other:?}"),
    };
    assert_eq!(lacks("d:master", &[("id", "1")]).as_deref(), Some("xmlns:d"));
    assert_eq!(lacks("master", &[]).as_deref(), Some("xmlns"));
    assert_eq!(lacks("d:master", &[("xmlns:d", urn), ("id", "2")]), None);
    assert_eq!(
        lacks("d:master", &[("xmlns:d", "urn:elsewhere"), ("id", "4")]),
        None,
        "it declares the prefix, whatever it is bound to: the codec judges the namespace"
    );
    assert_eq!(lacks("master", &[("xmlns", urn)]), None);
}

/// The memory a save holds for the trees: each take is refused when the room is spent and the figure refused is the cap, what is taken is given back together, a save holds
/// the most it held at once, and the gate of a comparison holds nothing and refuses nothing.
#[test]
fn the_gate_takes_the_room_refuses_what_it_does_not_have_and_gives_it_all_back() {
    let mut work = WriteWork { memory_cap: 1_000, memory_room: 100, ..WriteWork::default() };
    {
        let mut gate = Gate::new(&mut work);
        gate.take(60).unwrap();
        gate.take(30).unwrap();
        match gate.take(11) {
            Err(WriteError::Limit { what, max }) => assert_eq!((what, max), (WORDS_MEMORY, 1_000)),
            other => panic!("{other:?}"),
        }
        gate.take(10).unwrap();
        gate.made();
        gate.made();
        gate.release();
    }
    assert_eq!(work.memory_room, 100, "all of it given back, and what was refused never taken");
    assert_eq!((work.docfile_held, work.docfile_trees), (100, 2));
    let mut none = Gate::none();
    none.take(u64::MAX).unwrap();
    none.made();
    none.release();
}

fn nested(levels: usize) -> El {
    let mut e = El { name: "diacraft:x".to_owned(), attrs: Vec::new(), children: Vec::new() };
    for _ in 1..levels {
        e = El { name: "diacraft:x".to_owned(), attrs: Vec::new(), children: vec![Node::El(e)] };
    }
    e
}

/// The file made is no deeper than the reader parses it: as deep as the container's ceiling is written, one level more is refused in words, and the depth of a tree is
/// counted with the root as the first level.
#[test]
fn a_file_as_deep_as_the_reader_parses_is_written_and_one_level_more_is_refused() {
    let ceiling = diacraft_container::limits::XML_DEPTH_CEILING;
    let limits = crate::Limits::default();
    assert_eq!(measure(&nested(1)).2, 1);
    assert_eq!(measure(&nested(7)).2, 7);
    let doc = Document::empty();
    let after = Native::default().after(&doc, &Structure::identity(doc.pages.len()));
    let check = ReadCheck { doc: &doc, after: &after, room: u64::MAX, walk: 0 };
    let (bytes, nodes) = written(nested(ceiling), &limits, &check).unwrap_or_else(|e| panic!("{ceiling} levels are written: {e:?}"));
    assert_eq!(nodes as usize, ceiling);
    assert!(!bytes.is_empty());
    match written(nested(ceiling + 1), &limits, &check) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("nested deeper"), "{words}"),
        other => panic!("{:?}", other.map(|(b, _)| b.len())),
    }
}
