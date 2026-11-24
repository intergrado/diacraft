//! Which elements are the sheet's own: the vocabulary says which namespaces are read, and an element of any other is kept and never read as a
//! cell, a section or a row, whatever its local name.

use diacraft_sheetxml::{Scope, UNBOUND};

use super::{BIG, EXT, Ext, MAIN, Main, parse, read_in, read_with};

fn keys(sheet: &diacraft_doc::Sheet) -> Vec<&str> {
    sheet.cells.keys().map(|k| k.as_str()).collect()
}

#[test]
fn an_element_of_another_namespace_is_kept_and_never_read_as_a_cell_a_section_or_a_row() {
    let r = read_in::<Ext>(
        &format!(
            r#"<w xmlns="{EXT}" xmlns:o="urn:other"><PageSheet>
                <Cell N="A" V="1"/><o:Cell N="B"/><o:Section N="S"/>
                <Section N="User"><o:Row N="r"/><Row N="q"><o:Cell N="Z"/><Cell N="Y"/></Row></Section>
            </PageSheet></w>"#
        ),
        BIG,
    )
    .unwrap();
    assert_eq!(keys(&r.sheet), ["A"]);
    // The foreign cell and the foreign section are kept whole, in the sheet's own passthrough.
    assert_eq!(r.sheet.passthrough.len(), 2);
    let user = &r.sheet.sections[0];
    assert_eq!(user.rows.len(), 1);
    assert_eq!(user.passthrough.len(), 1);
    let row = &user.rows[0];
    assert_eq!(row.cells.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["Y"]);
    assert_eq!(row.passthrough.len(), 1);
}

#[test]
fn a_prefix_nothing_binds_is_no_namespace_anyone_reads_even_one_that_reads_no_namespace_at_all() {
    // `Main` reads an element with no namespace, and `q:Cell` has none that it is in: it is in the namespace of no one.
    let r = read_with::<Main>(r#"<PageSheet><Cell N="A"/><q:Cell N="B"/></PageSheet>"#, BIG).unwrap();
    assert_eq!(keys(&r.sheet), ["A"]);
    assert_eq!(r.sheet.passthrough.len(), 1);
    let el = parse(r#"<q:Cell/>"#);
    let seen = Scope::<Main>::empty().enter(&el).classify(&el, |uri| uri.map(str::to_owned));
    assert_eq!(seen.as_deref(), Some(UNBOUND));
}

#[test]
fn a_vocabulary_that_reads_one_namespace_does_not_read_no_namespace_and_one_that_reads_none_reads_a_default_declared_empty() {
    let ext = read_in::<Ext>(&format!(r#"<w xmlns="{EXT}"><PageSheet><Cell N="A"/><Cell xmlns="" N="B"/></PageSheet></w>"#), BIG).unwrap();
    assert_eq!(keys(&ext.sheet), ["A"]);
    assert_eq!(ext.sheet.passthrough.len(), 1);
    // The main vocabulary reads no namespace at all, and a default namespace declared empty is that.
    let main = read_with::<Main>(r#"<PageSheet xmlns=""><Cell N="A"/></PageSheet>"#, BIG).unwrap();
    assert_eq!(keys(&main.sheet), ["A"]);
    let other = read_with::<Main>(r#"<PageSheet><Cell xmlns="urn:other" N="A"/></PageSheet>"#, BIG).unwrap();
    assert!(other.sheet.cells.is_empty());
    assert_eq!(other.sheet.passthrough.len(), 1);
}

#[test]
fn a_prefix_may_be_declared_anywhere_above_and_rebound_below() {
    let r = read_in::<Main>(
        &format!(
            r#"<w xmlns:a="{MAIN}" xmlns:b="urn:other"><a:PageSheet>
                <a:Cell N="A"/><b:Cell N="B"/>
                <a:Section N="User"><a:Row N="r" xmlns:a="urn:rebound"><a:Cell N="Z"/></a:Row><a:Row N="s"/></a:Section>
            </a:PageSheet></w>"#
        ),
        BIG,
    )
    .unwrap();
    assert_eq!(keys(&r.sheet), ["A"]);
    let user = &r.sheet.sections[0];
    // The first row declares `a` as another namespace for itself and what is inside it: the row is not read, the second is.
    assert_eq!(user.rows.len(), 1);
    assert_eq!(user.rows[0].name.as_deref(), Some("s"));
    assert_eq!(user.passthrough.len(), 1);
}

#[test]
fn the_scope_names_the_namespace_of_an_element_and_of_an_attribute() {
    let root =
        parse(&format!(r#"<w xmlns="{MAIN}" xmlns:r="urn:rel" xmlns:e="{EXT}"><e:item r:id="x" id="y" e:id="z"><plain xmlns=""/></e:item></w>"#));
    let sc = Scope::<Main>::empty().enter(&root);
    assert_eq!(sc.uri_of_element(&root).as_deref(), Some(MAIN));
    assert!(sc.is(&root, "w") && !sc.is(&root, "item"));
    let item = root.elements().next().unwrap();
    let isc = sc.enter(item);
    assert_eq!(isc.uri_of_element(item).as_deref(), Some(EXT));
    assert!(!isc.is_ours(item));
    assert!(isc.is_attr_in("r:id", "id", "urn:rel"));
    assert!(isc.is_attr_in("e:id", "id", EXT));
    // No prefix is no namespace for an attribute: it is not found in any.
    assert!(!isc.is_attr_in("id", "id", MAIN));
    assert!(!isc.is_attr_in("r:id", "id", EXT));
    assert!(!isc.is_attr_in("z:id", "id", "urn:rel"));
    let plain = item.elements().next().unwrap();
    assert_eq!(isc.enter(plain).uri_of_element(plain), None);
    assert!(isc.enter(plain).is_ours(plain));
}

#[test]
fn a_root_that_declares_ten_thousand_prefixes_and_ten_thousand_children_that_each_use_one_is_read() {
    // The cost of a prefix is one lookup in the frames above and an answer kept in the frame asked: the scope never copies the prefixes in force.
    let mut root = String::from(&format!(r#"<PageSheet xmlns="{MAIN}""#));
    for i in 0..10_000 {
        root.push_str(&format!(r#" xmlns:p{i}="urn:p{i}""#));
    }
    root.push('>');
    for i in 0..10_000 {
        root.push_str(&format!(r#"<p{i}:Cell xmlns:q{i}="urn:q{i}" N="c{i}"/>"#));
    }
    root.push_str("</PageSheet>");
    let r = read_with::<Main>(&root, BIG).unwrap();
    // None of the prefixes is the vocabulary's: all of it is kept, and nothing is read.
    assert!(r.sheet.cells.is_empty());
    assert_eq!(r.sheet.passthrough.len(), 10_000);
}
