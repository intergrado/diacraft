//! What T8 draws from and T6 reads: the sections, rows and members of a shape once its master and styles are put together, and the lookup
//! of the master shape a shape inherits from.

use diacraft_doc::{CellAddr, MasterStep, Member, Owner, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Source, ValueKind, Warnings};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn page_shape(path: &[u32]) -> ShapeRef {
    ShapeRef { owner: Owner::Page(1), path: path.to_vec() }
}

fn sheet(path: &[u32]) -> SheetRef {
    SheetRef::Shape(page_shape(path))
}

fn master_shape(master: u32, path: &[u32]) -> ShapeRef {
    ShapeRef { owner: Owner::Master(master), path: path.to_vec() }
}

fn ix_keys(keys: Option<Vec<SectionKey>>) -> Vec<Option<u32>> {
    keys.unwrap().into_iter().map(|k| k.ix).collect()
}

// ---- sections ---------------------------------------------------------------------------------------------------------------

/// A master shape with `Geometry` 0 and 1, each with a row, and an instance with `Geometry` 1 (a counterpart) and 3 (new).
fn geometry_pair() -> diacraft_doc::Document {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0").row_cell(
        "Geometry",
        Some(1),
        RowKey::Ix(1),
        "X",
        "10",
    ));
    let instance = ShapeBuilder::new(10).master(1).row_cell("Geometry", Some(1), RowKey::Ix(2), "X", "11").row_cell(
        "Geometry",
        Some(3),
        RowKey::Ix(1),
        "X",
        "30",
    );
    DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build()
}

#[test]
fn sections_merge_by_name_and_exact_geometry_ix() {
    // 169,441 inherited Geometry sections have a local counterpart and 152,121 have none. Geometry 0 is the master's whole; Geometry 1
    // is merged (rows 1 and 2); Geometry 3 is the page's own.
    let doc = geometry_pair();
    let resolver = Resolver::new(&doc);
    assert_eq!(ix_keys(resolver.effective_sections(&sheet(&[0]), "Geometry")), [Some(0), Some(1), Some(3)]);
    let merged = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", Some(1))).unwrap();
    assert_eq!(merged.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(1), RowKey::Ix(2)], "the master's row 1 and the page's row 2");
    let first = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", Some(0))).unwrap();
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0].cells[0].1.formula.source, Source::Master(_)), "a section with no local counterpart is inherited whole");
    let third = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", Some(3))).unwrap();
    assert_eq!((third.len(), &third[0].cells[0].1.formula.source), (1, &Source::Local));
}

#[test]
fn a_key_with_no_ix_is_not_the_first_geometry_section() {
    // The review found a key `Geometry` with no `IX` merged the local `IX` 3 with the master's `IX` 0. A key names a section by name and
    // exact `IX`: with none, it names no `Geometry` section at all.
    let doc = geometry_pair();
    let resolver = Resolver::new(&doc);
    assert!(resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", None)).unwrap().is_empty());
    let nowhere = resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Geometry", None, RowKey::Ix(1), "X")).unwrap();
    assert_eq!(nowhere.formula.source, Source::Default, "a cell of a section that is not there");
    // Geometry 3 holds the page's cell only, and Geometry 0 the master's only.
    let (three, zero) = (SectionKey::new("Geometry", Some(3)), SectionKey::new("Geometry", Some(0)));
    let rows_of = |key: &SectionKey| {
        resolver
            .effective_rows(&sheet(&[0]), key)
            .unwrap()
            .iter()
            .map(|r| r.cells[0].1.value.cell.and_then(|c| c.value.clone()).unwrap().text().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(rows_of(&three), ["30"]);
    assert_eq!(rows_of(&zero), ["0"]);
}

#[test]
fn a_section_deleted_locally_is_gone_from_the_effective_sections() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0").row_cell(
        "Geometry",
        Some(1),
        RowKey::Ix(1),
        "X",
        "1",
    ));
    // `Del` on the instance's `Geometry` 0: its master's section is gone. (`del_row` makes the section; a deleted section is built by hand.)
    let mut instance = ShapeBuilder::new(10).master(1).row_cell("Geometry", Some(0), RowKey::Ix(9), "X", "9");
    instance = instance.del_section("Geometry", Some(0));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    assert_eq!(ix_keys(resolver.effective_sections(&sheet(&[0]), "Geometry")), [Some(1)]);
    assert!(resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", Some(0))).unwrap().is_empty());
}

#[test]
fn other_sections_are_one_each_and_their_names_are_listed() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("User", None, RowKey::name("a"), "Value", "1").row_cell(
        "Scratch",
        None,
        RowKey::Ix(0),
        "X",
        "1",
    ));
    let instance = ShapeBuilder::new(10).master(1).row_cell("User", None, RowKey::name("b"), "Value", "2").row_cell(
        "Controls",
        None,
        RowKey::name("c"),
        "X",
        "1",
    );
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    assert_eq!(ix_keys(resolver.effective_sections(&sheet(&[0]), "User")), [None]);
    assert!(resolver.effective_sections(&sheet(&[0]), "Property").unwrap().is_empty());
    let names: Vec<String> = resolver.effective_section_names(&sheet(&[0])).unwrap().iter().map(|n| n.as_str().to_owned()).collect();
    for expected in ["User", "Scratch", "Controls"] {
        assert!(names.iter().any(|n| n == expected), "{names:?} lacks {expected}");
    }
    assert_eq!(names[0], "User", "the sheet's own come first");
    assert!(resolver.effective_sections(&sheet(&[9]), "User").is_none(), "no such shape");
}

#[test]
fn a_section_of_a_family_comes_from_the_style_chain_for_a_shape_and_for_a_style() {
    // `effective_rows` on a style used to skip the style's own chain, and on a shape the root for an unlisted section, where `resolve`
    // reads both.
    let doc = DocBuilder::new()
        .root(|r| r.row_cell("User", None, RowKey::name("x"), "Value", "root"))
        .style(3, StyleBuilder::new().links(None, None, Some(4)))
        .style(4, StyleBuilder::new().row_cell("Character", None, RowKey::Ix(0), "Size", "0.2"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(None, None, Some(3))))
        .build();
    let resolver = Resolver::new(&doc);
    // On a style: the chain.
    let rows = resolver.effective_rows(&SheetRef::Style(3), &SectionKey::new("Character", None)).unwrap();
    assert_eq!(rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(0)]);
    assert_eq!(rows[0].cells[0].1.formula.source, Source::Style { id: 4, family: Some(diacraft_doc::Family::Text) });
    // On a shape: the same, and the root for an unlisted section.
    let shape_rows = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Character", None)).unwrap();
    assert_eq!(shape_rows.len(), 1);
    let user = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("User", None)).unwrap();
    assert_eq!(user.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::name("x")]);
    // Each cell the rows list is what `resolve` says of it.
    for (section, rows) in [("Character", &shape_rows), ("User", &user)] {
        for row in rows.iter() {
            for (cell, resolved) in &row.cells {
                let addr = CellAddr { section: Some(SectionKey::new(section, None)), row: Some(row.key.clone()), cell: cell.clone() };
                assert_eq!(&resolver.resolve(&sheet(&[0]), &addr).unwrap(), resolved);
            }
        }
    }
    // The root's own sections are listed on the page sheets too, and the root asked is not asked twice.
    assert_eq!(resolver.effective_rows(&SheetRef::Page(1), &SectionKey::new("User", None)).unwrap().len(), 1);
    assert_eq!(resolver.effective_rows(&SheetRef::Style(0), &SectionKey::new("User", None)).unwrap().len(), 1);
}

#[test]
fn a_row_deleted_on_a_master_shape_reads_the_same_on_its_instance() {
    // `Del` stops the row's values being inherited. The master shape's own row read as nothing while its instance read the style's.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).links(None, None, Some(1)).del_row("Character", None, RowKey::Ix(0)));
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().row_cell("Character", None, RowKey::Ix(0), "Size", "0.2"))
        .master(1, master)
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let resolver = Resolver::new(&doc);
    let addr = CellAddr::in_row("Character", None, RowKey::Ix(0), "Size");
    assert!(resolver.resolve(&SheetRef::Shape(master_shape(1, &[0])), &addr).is_none(), "on the master shape");
    assert!(resolver.resolve(&sheet(&[0]), &addr).is_none(), "on its instance");
    assert!(resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Character", None)).unwrap().is_empty());
    // A row the master did not delete is read by both.
    let kept = CellAddr::in_row("Character", None, RowKey::Ix(1), "Size");
    assert!(resolver.resolve(&sheet(&[0]), &kept).is_some());
}

// ---- the master lookup ------------------------------------------------------------------------------------------------------

#[test]
fn the_master_shape_a_shape_inherits_from_is_a_public_lookup() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)));
    let many = MasterBuilder::new().shape(ShapeBuilder::new(5)).shape(ShapeBuilder::new(6));
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(10).master(1).child(ShapeBuilder::new(11).master_shape(6)))
        .shape(ShapeBuilder::new(12))
        .shape(ShapeBuilder::new(13).master(2))
        .shape(ShapeBuilder::new(14).master(99))
        .shape(ShapeBuilder::new(15).master_shape(6));
    let doc = DocBuilder::new().master(1, master).master(2, many).page(1, page).build();
    let resolver = Resolver::new(&doc);
    // An instance: its master's one top-level shape. A member: the shape it stands for, and the instance above it.
    assert_eq!(resolver.master_step(&page_shape(&[0])), Ok(Some(MasterStep { shape: master_shape(1, &[0]), instance: page_shape(&[0]), master: 1 })));
    assert_eq!(
        resolver.master_step(&page_shape(&[0, 0])),
        Ok(Some(MasterStep { shape: master_shape(1, &[0, 0]), instance: page_shape(&[0]), master: 1 }))
    );
    // Nothing to inherit from: a plain shape, an instance of a master of several shapes, a shape in a master part.
    assert_eq!(resolver.master_step(&page_shape(&[1])), Ok(None));
    assert_eq!(resolver.master_step(&page_shape(&[2])), Ok(None));
    assert_eq!(resolver.master_step(&master_shape(1, &[0, 0])), Ok(None));
    assert_eq!(resolver.master_step(&page_shape(&[9])), Ok(None), "no such shape");
    // Links that lead nowhere say so, and resolve as if unlinked.
    assert_eq!(resolver.master_step(&page_shape(&[3])), Err(Warnings::DANGLING_MASTER));
    assert_eq!(resolver.master_step(&page_shape(&[4])), Err(Warnings::DANGLING_MASTER_SHAPE));
}

// ---- members ----------------------------------------------------------------------------------------------------------------

/// A master holding a group (ID 5) of 6, 7 and 8, and an instance that lists only some of them.
fn grouped(members: Vec<ShapeBuilder>) -> diacraft_doc::Document {
    let master =
        MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)).child(ShapeBuilder::new(7)).child(ShapeBuilder::new(8)));
    let mut instance = ShapeBuilder::new(40).master(1).group();
    for m in members {
        instance = instance.child(m);
    }
    DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build()
}

#[test]
fn an_instance_may_leave_members_out_and_they_are_listed_as_omitted() {
    // MS-VSDX lets an instance omit members, and R6 gives each the master's cells. The members the instance lists, the one it left out in
    // the master's place, and a shape the page added after.
    let doc = grouped(vec![ShapeBuilder::new(41).master_shape(6), ShapeBuilder::new(43).master_shape(8), ShapeBuilder::new(44)]);
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(
        members,
        [
            Member::Local(page_shape(&[0, 0])),
            Member::Omitted(master_shape(1, &[0, 1])),
            Member::Local(page_shape(&[0, 1])),
            Member::Local(page_shape(&[0, 2])),
        ]
    );
}

#[test]
fn a_member_deleted_locally_removes_the_master_shape_it_stands_for() {
    // `Del` on a member removes the master shape locally (masters R21): neither the member nor the shape is a member.
    let doc =
        grouped(vec![ShapeBuilder::new(41).master_shape(6), ShapeBuilder::new(42).master_shape(7).deleted(), ShapeBuilder::new(43).master_shape(8)]);
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, [Member::Local(page_shape(&[0, 0])), Member::Local(page_shape(&[0, 2]))]);
}

#[test]
fn an_instance_that_lists_every_member_has_no_omitted_ones_and_keeps_its_own_order() {
    // The census: every one of 21,355 instances of a grouped master lists all of its descendants. The page's order is the display order
    // Whatever order the master has: the file's [43, 99, 41, 42] comes back as it was written.
    let doc = grouped(vec![
        ShapeBuilder::new(43).master_shape(8),
        ShapeBuilder::new(99),
        ShapeBuilder::new(41).master_shape(6),
        ShapeBuilder::new(42).master_shape(7),
    ]);
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, (0..4).map(|i| Member::Local(page_shape(&[0, i]))).collect::<Vec<_>>());
}

#[test]
fn an_omitted_member_sits_after_the_member_that_stands_for_the_master_shape_before_it() {
    // Master 6, 7, 8, 9. The page lists 8, a shape of its own and 6, so 7 comes after 6 and 9 after 8: the page's order stands and each
    // omitted shape keeps its place among the master's siblings that are there.
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)).child(ShapeBuilder::new(7)).child(ShapeBuilder::new(8)).child(ShapeBuilder::new(9)),
    );
    let instance = ShapeBuilder::new(40)
        .master(1)
        .group()
        .child(ShapeBuilder::new(43).master_shape(8))
        .child(ShapeBuilder::new(99))
        .child(ShapeBuilder::new(41).master_shape(6));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(
        members,
        [
            Member::Local(page_shape(&[0, 0])),
            Member::Omitted(master_shape(1, &[0, 3])),
            Member::Local(page_shape(&[0, 1])),
            Member::Local(page_shape(&[0, 2])),
            Member::Omitted(master_shape(1, &[0, 1])),
        ]
    );
}

#[test]
fn omitted_members_with_no_member_before_them_come_first_in_the_masters_order() {
    // Nothing of the master precedes 6 and 7 on the page, so they lead, in the master's order, then the page's own list.
    let master =
        MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)).child(ShapeBuilder::new(7)).child(ShapeBuilder::new(8)));
    let instance = ShapeBuilder::new(40).master(1).group().child(ShapeBuilder::new(44)).child(ShapeBuilder::new(43).master_shape(8));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(
        members,
        [
            Member::Omitted(master_shape(1, &[0, 0])),
            Member::Omitted(master_shape(1, &[0, 1])),
            Member::Local(page_shape(&[0, 0])),
            Member::Local(page_shape(&[0, 1])),
        ]
    );
}

#[test]
fn a_deleted_member_keeps_its_place_for_the_omitted_shapes_after_it() {
    // Member 41 is `Del` and so gone, with the master shape 6 it stood for. 7 is omitted: it follows the (gone) 6 in the file, so it
    // leads the page's list here; the page's own 99 stays where it is.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)).child(ShapeBuilder::new(7)));
    let instance = ShapeBuilder::new(40).master(1).group().child(ShapeBuilder::new(41).master_shape(6).deleted()).child(ShapeBuilder::new(99));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, [Member::Omitted(master_shape(1, &[0, 1])), Member::Local(page_shape(&[0, 1]))]);
}

#[test]
fn the_members_of_an_instance_of_a_master_of_several_shapes_are_those_shapes() {
    // A master with several top-level shapes passes them to the instance as members.
    let many = MasterBuilder::new().shape(ShapeBuilder::new(5)).shape(ShapeBuilder::new(6)).shape(ShapeBuilder::new(7));
    let instance = ShapeBuilder::new(40).master(1).group().child(ShapeBuilder::new(41).master_shape(5)).child(ShapeBuilder::new(43).master_shape(7));
    let doc = DocBuilder::new().master(1, many).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, [Member::Local(page_shape(&[0, 0])), Member::Omitted(master_shape(1, &[1])), Member::Local(page_shape(&[0, 1]))]);
}

#[test]
fn a_member_with_members_of_its_own_is_merged_with_the_master_shape_it_stands_for() {
    // A member that stands for a group of the master has the master group's members, left out or listed, in turn.
    let master = MasterBuilder::new()
        .shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6).group().child(ShapeBuilder::new(9)).child(ShapeBuilder::new(10))));
    let instance =
        ShapeBuilder::new(40).master(1).group().child(ShapeBuilder::new(41).master_shape(6).group().child(ShapeBuilder::new(42).master_shape(10)));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    assert_eq!(resolver.effective_members(&page_shape(&[0])).unwrap(), [Member::Local(page_shape(&[0, 0]))]);
    assert_eq!(
        resolver.effective_members(&page_shape(&[0, 0])).unwrap(),
        [Member::Omitted(master_shape(1, &[0, 0, 0])), Member::Local(page_shape(&[0, 0, 0]))]
    );
}

#[test]
fn a_shape_with_no_master_has_its_own_members_in_file_order_and_a_missing_one_has_none() {
    let doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(10).group().child(ShapeBuilder::new(11)).child(ShapeBuilder::new(12).deleted()).child(ShapeBuilder::new(13)))
                .shape(ShapeBuilder::new(20).master(99).child(ShapeBuilder::new(21))),
        )
        .build();
    let resolver = Resolver::new(&doc);
    assert_eq!(resolver.effective_members(&page_shape(&[0])).unwrap(), [Member::Local(page_shape(&[0, 0])), Member::Local(page_shape(&[0, 2]))]);
    // A master that does not exist is a dangling link: the shape's own members.
    assert_eq!(resolver.effective_members(&page_shape(&[1])).unwrap(), [Member::Local(page_shape(&[1, 0]))]);
    assert_eq!(resolver.effective_members(&page_shape(&[5])), None);
    // A shape in a master part has no master step: its own members.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).master(1).child(ShapeBuilder::new(6)));
    let doc = DocBuilder::new().master(1, master).build();
    assert_eq!(Resolver::new(&doc).effective_members(&master_shape(1, &[0])).unwrap(), [Member::Local(master_shape(1, &[0, 0]))]);
}

// ---- listed -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_resolved_cell_says_whether_the_catalogue_lists_it() {
    // A cell no batch lists reads as a plain number from the root style: a text cell would read 0, not "". Callers that must not guess
    // can see it.
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Mystery", "1"))).build();
    let resolver = Resolver::new(&doc);
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::direct("PinX")).unwrap().listed);
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::direct("TxtWidth")).unwrap().listed);
    assert!(!resolver.resolve(&sheet(&[0]), &CellAddr::direct("Mystery")).unwrap().listed, "found locally and still not listed");
    // `LayerMember` is listed since the layer batch is catalogued, and the cells of `Property` rows since the data batch is.
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::direct("LayerMember")).unwrap().listed);
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Property", None, RowKey::name("p"), "Label")).unwrap().listed);
    assert!(!resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Property", None, RowKey::name("p"), "Ask")).unwrap().listed, "the cell is Verify");
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")).unwrap().listed);
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Layer", None, RowKey::Ix(0), "Visible")).unwrap().listed);
    assert!(resolver.resolve(&sheet(&[0]), &CellAddr::in_row("Actions", None, RowKey::name("Row_1"), "Menu")).unwrap().listed);
}

#[test]
fn a_missing_layer_list_reads_as_a_string_and_not_as_a_plain_number() {
    // `LayerMember` is a text list: found nowhere, it is the empty string,
    // where an unlisted cell would read 0. A list of the shape's own, or one a master holds, is found as for any cell.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("LayerMember", "3;4"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(10))
                .shape(ShapeBuilder::new(11).master(1))
                .shape(ShapeBuilder::new(12).master(1).cell("LayerMember", "0;2")),
        )
        .build();
    let resolver = Resolver::new(&doc);
    let list = CellAddr::direct("LayerMember");
    let text_of = |sheet: &SheetRef| {
        let found = resolver.resolve(sheet, &list).unwrap();
        (found.value.source.clone(), found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
    };
    let bare = resolver.resolve(&sheet(&[0]), &list).unwrap();
    assert_eq!((bare.formula.source.clone(), bare.default.kind, bare.listed), (Source::Default, ValueKind::Text, true));
    assert_eq!(text_of(&sheet(&[0])), (Source::Default, None));
    let (source, text) = text_of(&sheet(&[1]));
    assert!(matches!(source, Source::Master(_)), "{source:?}");
    assert_eq!(text.as_deref(), Some("3;4"));
    assert_eq!(text_of(&sheet(&[2])), (Source::Local, Some("0;2".to_owned())));
}

// ---- Sheet.N and `Del`, and a row deleted on a style ------------------------------------------------------------------------------

#[test]
fn a_member_removed_with_del_is_not_what_a_sheet_n_names() {
    // The doc said `None` for a member removed with `Del`; the review's probe got the path to the deleted member, and to one under it.
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(12)
            .group()
            .child(ShapeBuilder::new(14))
            .child(ShapeBuilder::new(15).formula("PinX", "Sheet.12!Width", "1"))
            .child(ShapeBuilder::new(16).group().child(ShapeBuilder::new(17))),
    );
    let instance = ShapeBuilder::new(40)
        .master(1)
        .child(ShapeBuilder::new(41).master_shape(14).deleted())
        .child(ShapeBuilder::new(42).master_shape(15))
        .child(ShapeBuilder::new(43).master_shape(16).deleted().child(ShapeBuilder::new(44).master_shape(17)));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    let rebind = resolver.resolve(&sheet(&[0, 1]), &CellAddr::direct("PinX")).unwrap().rebind.unwrap();
    assert_eq!(resolver.rebind(&rebind, 15), Some(page_shape(&[0, 1])), "the member that is there");
    assert_eq!(resolver.rebind(&rebind, 14), None, "the member that was deleted");
    assert_eq!(resolver.rebind(&rebind, 16), None, "a deleted group");
    assert_eq!(resolver.rebind(&rebind, 17), None, "and a member under it, which went with it");
    assert_eq!(resolver.rebind(&rebind, 12), Some(page_shape(&[0])), "the instance itself is not a member and is not deleted");
}

#[test]
fn a_row_deleted_on_a_style_stops_the_styles_below_it() {
    // R28 at a style: its `Del` row is gone, and what the styles it is based on hold for the row is not read through it. (Where the shape
    // is linked to the base style directly, the row is there.)
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().del_row("Character", None, RowKey::Ix(0)).links(None, None, Some(2)))
        .style(2, StyleBuilder::new().row_cell("Character", None, RowKey::Ix(0), "Size", "0.2"))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(None, None, Some(1))).shape(ShapeBuilder::new(11).links(None, None, Some(2))))
        .build();
    let resolver = Resolver::new(&doc);
    let addr = CellAddr::in_row("Character", None, RowKey::Ix(0), "Size");
    assert!(resolver.resolve(&sheet(&[0]), &addr).is_none(), "through the deleting style");
    assert!(resolver.resolve(&SheetRef::Style(1), &addr).is_none(), "on the deleting style itself");
    assert!(resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Character", None)).unwrap().is_empty());
    let direct = resolver.resolve(&sheet(&[1]), &addr).unwrap();
    assert_eq!(direct.formula.source, Source::Style { id: 2, family: Some(diacraft_doc::Family::Text) });
}

#[test]
fn geometry_sections_are_listed_in_ix_order_whatever_step_each_came_from() {
    // The master's come first as they are met (1 and 2), and the page's own (0) after; the list is by `IX`, as the numbering is.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Geometry", Some(1), RowKey::Ix(1), "X", "1").row_cell(
        "Geometry",
        Some(2),
        RowKey::Ix(1),
        "X",
        "2",
    ));
    let instance = ShapeBuilder::new(10).master(1).row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0");
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    assert_eq!(ix_keys(Resolver::new(&doc).effective_sections(&sheet(&[0]), "Geometry")), [Some(0), Some(1), Some(2)]);
}

// ---- a local cell that holds a value, under a row the master or a style deleted ---------------------------------------------------------

#[test]
fn a_local_inh_cell_keeps_its_value_when_a_later_step_deleted_the_row_or_the_section() {
    // A local cell that holds a `V` has the last word, `Inh` or not. The review: a `Del` at a later step returned `None` and hid it,
    // so a cached result written into a row the master deleted was reported written and read as gone (the lost edit).
    let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X");
    let by_master = |master_shape: ShapeBuilder| {
        let instance = ShapeBuilder::new(10).master(1).row_inh("Geometry", Some(0), RowKey::Ix(1), "X", Some("7"));
        DocBuilder::new().master(1, MasterBuilder::new().shape(master_shape)).page(1, PageBuilder::new().shape(instance)).build()
    };
    for (what, doc) in [
        ("a row deleted by the master", by_master(ShapeBuilder::new(5).del_row("Geometry", Some(0), RowKey::Ix(1)))),
        ("a section deleted by the master", by_master(ShapeBuilder::new(5).del_section("Geometry", Some(0)))),
    ] {
        let resolver = Resolver::new(&doc);
        let r = resolver.resolve(&sheet(&[0]), &addr).unwrap_or_else(|| panic!("{what}: the cell reads as gone"));
        assert_eq!(
            (r.value.source.clone(), r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())),
            (Source::Local, Some("7".to_owned())),
            "{what}"
        );
        assert_eq!(r.formula.source, Source::Default, "{what}: nothing is inherited through the deletion");
        assert!(!resolver.inherits(&sheet(&[0]), &addr), "{what}");
        let rows = resolver.effective_rows(&sheet(&[0]), &SectionKey::new("Geometry", Some(0))).unwrap();
        assert_eq!(rows.len(), 1, "{what}: the row the page holds is still a row");
        assert_eq!(rows[0].cells.len(), 1, "{what}: and so is its cell");
    }
    // By a style on the way (a text cell, through the text link).
    let doc = DocBuilder::new()
        .style(1, StyleBuilder::new().del_row("Character", None, RowKey::Ix(0)))
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(10).links(None, None, Some(1)).row_inh("Character", None, RowKey::Ix(0), "Size", Some("0.3"))),
        )
        .build();
    let r = Resolver::new(&doc).resolve(&sheet(&[0]), &CellAddr::in_row("Character", None, RowKey::Ix(0), "Size")).unwrap();
    assert_eq!((r.value.source, r.formula.source), (Source::Local, Source::Default));
    // And with nothing local at all there is still nothing: the cell is not there.
    let doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).del_row("Geometry", Some(0), RowKey::Ix(1))))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    assert!(Resolver::new(&doc).resolve(&sheet(&[0]), &addr).is_none());
}

#[test]
fn a_cached_result_written_into_a_row_the_master_deleted_is_read_back() {
    // The same through the write: `Ok(Written)` and then the value is there.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).del_row("Geometry", Some(0), RowKey::Ix(1)));
    let mut doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1))).build();
    let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X");
    let edit = diacraft_doc::CellEdit::Cached { value: diacraft_doc::Value::from_text("4"), error: None, unit: None };
    let out = doc.write_cell(&sheet(&[0]), &addr, edit, diacraft_doc::WriteOrigin::Evaluation);
    assert_eq!(out, Ok(diacraft_doc::WriteOutcome::Written));
    let r = Resolver::new(&doc).resolve(&sheet(&[0]), &addr).expect("the value written is not lost");
    assert_eq!(r.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()), Some("4".to_owned()));
}

#[test]
fn a_local_inh_cell_with_no_value_is_still_a_cell_the_deletion_ended_the_search_for() {
    // `F="Inh"` and no `V` is a cell a file holds. Under a row the master deleted nothing is inherited, and the cell is found: it is the
    // cell, reading what a cell with nothing reads, not a cell that is not there.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).del_row("Geometry", Some(0), RowKey::Ix(1)));
    let instance = ShapeBuilder::new(10).master(1).row_inh("Geometry", Some(0), RowKey::Ix(1), "X", None);
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let r = Resolver::new(&doc).resolve(&sheet(&[0]), &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")).expect("the local cell is there");
    assert_eq!((r.formula.source, r.value.source), (Source::Default, Source::Default));
}

// ---- a sheet with many sections, and a master that repeats an ID -------------------------------------------------------------------

#[test]
fn a_sheet_with_many_sections_is_read_as_a_scan_would_read_it() {
    // Past 16 sections a sheet's sections are found by an index: the first section with a name and `IX` wins, as in a scan, a key with the
    // wrong `IX` finds nothing, and the lists of sections and of names are the scan's.
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Lookup, Row, Section, Value};
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    let mut sections: Vec<Arc<Section>> = Vec::new();
    let put = |section: &mut Section, value: &str| {
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(value)));
        section.rows.push(Arc::new(row));
    };
    for i in 0..30 {
        let mut s = Section::new(&format!("S{i}"), None);
        put(&mut s, &format!("s{i}"));
        sections.push(Arc::new(s));
    }
    for ix in 0..10_u32 {
        let mut s = Section::new("Geometry", Some(ix));
        put(&mut s, &format!("g{ix}"));
        sections.push(Arc::new(s));
    }
    let mut twin = Section::new("Geometry", Some(3));
    put(&mut twin, "the second Geometry 3");
    sections.push(Arc::new(twin));
    // A section whose one row holds no cell: nothing to resolve in it, so only the listing of rows looks for the section.
    let mut bare = Section::new("Bare", None);
    bare.rows.push(Arc::new(Row { ix: Some(1), ..Row::default() }));
    sections.push(Arc::new(bare));
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).sections = sections;

    let resolver = Resolver::new(&doc);
    let local = resolver.sheet(&sheet(&[0])).unwrap();
    assert!(local.sections.len() > 16);
    let mut keys: Vec<SectionKey> = (0..30).map(|i| SectionKey::new(&format!("S{i}"), None)).collect();
    keys.extend((0..12).map(|ix| SectionKey::new("Geometry", Some(ix))));
    keys.extend([SectionKey::new("Geometry", None), SectionKey::new("S3", Some(0)), SectionKey::new("Nope", None)]);
    for key in keys {
        let addr = CellAddr { section: Some(key.clone()), row: Some(RowKey::Ix(1)), cell: CellName::new("X") };
        let scanned = match local.lookup(&addr) {
            Lookup::Found(cell) => cell.value.as_ref().map(|v| v.text().to_owned()),
            _ => None,
        };
        let indexed = resolver
            .resolve(&sheet(&[0]), &addr)
            .filter(|r| r.formula.source == Source::Local)
            .and_then(|r| r.formula.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()));
        assert_eq!(indexed, scanned, "{key:?}");
    }
    assert!(resolver.stats().section_indexes >= 1, "the sheet was indexed: {:?}", resolver.stats());
    assert_eq!(ix_keys(resolver.effective_sections(&sheet(&[0]), "Geometry")), (0..10).map(Some).collect::<Vec<_>>());
    let names = resolver.effective_section_names(&sheet(&[0])).unwrap();
    let want: Vec<String> = (0..30).map(|i| format!("S{i}")).chain(["Geometry".to_owned(), "Bare".to_owned()]).collect();
    assert_eq!(names.iter().map(|n| n.as_str().to_owned()).collect::<Vec<_>>(), want, "the sheet's own, in the order met, each once");
    // The rows of a section of a sheet with many are found through the index too, not by a scan of the sections.
    let listing = Resolver::new(&doc);
    let rows = listing.effective_rows(&sheet(&[0]), &SectionKey::new("Bare", None)).unwrap();
    assert_eq!((rows.len(), rows[0].cells.len()), (1, 0));
    assert!(listing.stats().section_indexes >= 1, "{:?}", listing.stats());
}

#[test]
fn a_master_that_repeats_an_id_gives_sheet_n_the_first_in_document_order() {
    // An ID need not be unique in a part. The first one wins in the master and in the instance.
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .group()
            .child(ShapeBuilder::new(6).formula("PinX", "Sheet.6!Width", "1"))
            .child(ShapeBuilder::new(6))
            .child(ShapeBuilder::new(7)),
    );
    let instance = ShapeBuilder::new(40)
        .master(1)
        .child(ShapeBuilder::new(41).master_shape(6))
        .child(ShapeBuilder::new(42).master_shape(6))
        .child(ShapeBuilder::new(43).master_shape(7));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&doc);
    let rebind = resolver.resolve(&sheet(&[0, 0]), &CellAddr::direct("PinX")).unwrap().rebind.unwrap();
    assert_eq!(resolver.rebind(&rebind, 6), Some(page_shape(&[0, 0])), "the first member that stands for 6");
    assert_eq!(resolver.rebind(&rebind, 7), Some(page_shape(&[0, 2])));
}

#[test]
fn a_second_member_standing_for_the_same_master_shape_is_a_shape_the_page_added() {
    // Two members name master shape 6: the first is the one that stands for it (and the omitted 7 follows it), the second stands for
    // nothing and is listed where the page has it.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6)).child(ShapeBuilder::new(7)));
    let instance = ShapeBuilder::new(40).master(1).group().child(ShapeBuilder::new(41).master_shape(6)).child(ShapeBuilder::new(42).master_shape(6));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, [Member::Local(page_shape(&[0, 0])), Member::Omitted(master_shape(1, &[0, 1])), Member::Local(page_shape(&[0, 1]))]);
}

#[test]
fn a_master_shape_with_del_is_not_an_omitted_member() {
    // The master itself deleted shape 6 (a stencil whose group lost a part): the instance has nothing to omit for it.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(ShapeBuilder::new(6).deleted()).child(ShapeBuilder::new(7)));
    let instance = ShapeBuilder::new(40).master(1).group();
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let members = Resolver::new(&doc).effective_members(&page_shape(&[0])).unwrap();
    assert_eq!(members, [Member::Omitted(master_shape(1, &[0, 1]))]);
}

// ---- the rows of a section with no cell resolved ----------------------------------------------------------------

/// A master with a `Scratch` section of rows 0 to 2 and an instance that adds a cell to row 1, a row 5 and deletes row 2.
fn scratch_rows() -> diacraft_doc::Document {
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .row_cell("Scratch", None, RowKey::Ix(0), "X", "0")
            .row_cell("Scratch", None, RowKey::Ix(1), "X", "1")
            .row_cell("Scratch", None, RowKey::Ix(2), "X", "2")
            .row_type("Scratch", None, RowKey::Ix(1), "Special"),
    );
    let instance = ShapeBuilder::new(10)
        .master(1)
        .row_cell("Scratch", None, RowKey::Ix(1), "Y", "11")
        .row_cell("Scratch", None, RowKey::Ix(5), "A", "50")
        .del_row("Scratch", None, RowKey::Ix(2));
    DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build()
}

#[test]
fn the_rows_of_a_section_are_listed_with_their_types_and_cell_names_and_agree_with_the_merged_rows() {
    let doc = scratch_rows();
    let resolver = Resolver::new(&doc);
    let key = SectionKey::new("Scratch", None);
    let listed = resolver.effective_row_keys(&sheet(&[0]), &key).unwrap();
    let merged = resolver.effective_rows(&sheet(&[0]), &key).unwrap();
    assert_eq!(listed.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(0), RowKey::Ix(1), RowKey::Ix(5)], "row 2 is deleted here");
    assert_eq!(listed.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), merged.iter().map(|r| r.key.clone()).collect::<Vec<_>>());
    for (row, full) in listed.iter().zip(&merged) {
        assert_eq!(row.row_type, full.row_type, "the type of {:?}", row.key);
        let names: Vec<_> = full.cells.iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(row.cells, names, "the cells of {:?}", row.key);
        assert_eq!(resolver.row_cell_names(&sheet(&[0]), &key, &row.key).unwrap(), names, "one row by itself");
    }
    // Row 1 holds the master's X and the instance's Y; its type is the master's.
    assert_eq!(listed[1].cells.iter().map(|c| c.as_str()).collect::<Vec<_>>(), ["X", "Y"]);
    assert_eq!(listed[1].row_type.as_deref(), Some("Special"));
}

#[test]
fn a_row_that_is_deleted_or_not_there_and_a_sheet_that_is_not_there_have_no_cells_to_list() {
    let doc = scratch_rows();
    let resolver = Resolver::new(&doc);
    let key = SectionKey::new("Scratch", None);
    assert!(resolver.row_cell_names(&sheet(&[0]), &key, &RowKey::Ix(2)).unwrap().is_empty(), "deleted here: nothing of the master's is inherited");
    assert!(resolver.row_cell_names(&sheet(&[0]), &key, &RowKey::Ix(99)).unwrap().is_empty(), "not there");
    assert_eq!(resolver.row_cell_names(&sheet(&[9]), &key, &RowKey::Ix(0)), None, "no such shape");
    assert_eq!(resolver.effective_row_keys(&sheet(&[9]), &key), None);
    // A section deleted here has no rows at all, whatever the master holds.
    let mut instance = ShapeBuilder::new(10).master(1);
    instance = instance.del_section("Scratch", None);
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).row_cell("Scratch", None, RowKey::Ix(0), "X", "0"));
    let gone = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let resolver = Resolver::new(&gone);
    assert!(resolver.effective_row_keys(&sheet(&[0]), &key).unwrap().is_empty());
    assert!(resolver.row_cell_names(&sheet(&[0]), &key, &RowKey::Ix(0)).unwrap().is_empty());
}

#[test]
fn the_sheets_a_cell_is_looked_for_on_are_listed_nearest_first_by_the_class_of_the_cell() {
    use diacraft_doc::CellClass;
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).links(None, Some(2), None));
    let doc = DocBuilder::new()
        .style(2, StyleBuilder::new().links(None, Some(3), None))
        .style(3, StyleBuilder::new())
        .master(1, master)
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let resolver = Resolver::new(&doc);
    let sources = |class: CellClass| -> Vec<Source> { resolver.lookup_layers(&sheet(&[0]), class).unwrap().into_iter().map(|(s, _)| s).collect() };
    let fill = diacraft_doc::Family::Fill;
    // A fill cell: the shape, its master shape, then the chain of the fill style the master shape names, and no root.
    assert_eq!(
        sources(CellClass::FillFamily),
        [
            Source::Local,
            Source::Master(master_shape(1, &[0])),
            Source::Style { id: 2, family: Some(fill) },
            Source::Style { id: 3, family: Some(fill) }
        ]
    );
    // A cell of any style: the shape, the master shape and the root style.
    assert_eq!(sources(CellClass::StyleAny), [Source::Local, Source::Master(master_shape(1, &[0])), Source::Style { id: 0, family: None }]);
    // A cell of the shape only: no style at all. A page sheet takes no master step.
    assert_eq!(sources(CellClass::ShapeOnly), [Source::Local, Source::Master(master_shape(1, &[0]))]);
    assert_eq!(resolver.lookup_layers(&SheetRef::Page(1), CellClass::PageSheet).unwrap().len(), 2, "the page sheet and the root");
    assert!(resolver.lookup_layers(&sheet(&[9]), CellClass::StyleAny).is_none());
}
