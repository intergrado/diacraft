//! The names a drawing's pages may have. A name a command gives a page is not "page" and digits (page1, in any case), and no other page has it (compared without
//! regard to case); a page added without a name is called by a name no page has, in `Name` and in `NameU`.

use std::sync::Arc;

use diacraft_doc::{DocError, Document, Page, check_given_page_name, is_page_and_digits, page_name_key};

#[test]
fn page_and_digits_is_the_form_in_any_case_and_nothing_else() {
    for yes in ["page1", "Page1", "PAGE12", "pAgE007", "page0", "page123456789012345678901234567890"] {
        assert!(is_page_and_digits(yes), "{yes}");
    }
    // The default form has a hyphen, and a name with anything more is another name.
    for no in [
        "",
        "page",
        "Page-1",
        "Page 1",
        "Page_1",
        "page1a",
        "apage1",
        "pages1",
        "page1 ",
        " page1",
        "Page-1-2",
        "VBackground-1",
        "page\u{661}",
        "pagé1",
    ] {
        assert!(!is_page_and_digits(no), "{no:?}");
    }
}

#[test]
fn names_are_compared_without_regard_to_case() {
    assert_eq!(page_name_key("Overview"), page_name_key("OVERVIEW"));
    assert_eq!(page_name_key("Überblick"), page_name_key("ÜBERBLICK"));
    assert_ne!(page_name_key("Overview"), page_name_key("Overview "));
}

#[test]
fn a_name_a_command_gives_is_not_page_and_digits() {
    for name in ["page1", "Page2", "PAGE10"] {
        let e = check_given_page_name(name).expect_err(name);
        assert_eq!(e, DocError::PageNameReserved, "{name}");
        assert!(e.to_string().contains("\"page\" and digits"), "{e}");
    }
    for name in ["Page-1", "Page 1", "Overview", "page1a"] {
        assert_eq!(check_given_page_name(name), Ok(()), "{name}");
    }
}

#[test]
fn a_free_name_is_one_no_other_page_has_whatever_its_case() {
    let mut doc = Document::new_blank();
    let second = doc.add_page(Some("Overview")).unwrap();
    assert_eq!(doc.check_free_page_name("Notes", None), Ok(()));
    for taken in ["Overview", "overview", "OVERVIEW", "Page-1", "page-1"] {
        let e = doc.check_free_page_name(taken, None).expect_err(taken);
        assert_eq!(e, DocError::PageNameTaken, "{taken}");
        assert!(e.to_string().contains("another page has this name"), "{e}");
    }
    // The page itself may be given its own name again, or the same name in another case.
    assert_eq!(doc.check_free_page_name("Overview", Some(second)), Ok(()));
    assert_eq!(doc.check_free_page_name("OVERVIEW", Some(second)), Ok(()));
    assert_eq!(doc.check_free_page_name("Page-1", Some(second)), Err(DocError::PageNameTaken));
    // The other rules of a given name come first.
    assert_eq!(doc.check_free_page_name("page1", None), Err(DocError::PageNameReserved));
    assert_eq!(doc.check_free_page_name(&"x".repeat(32), None), Err(DocError::PageNameTooLong));
    assert_eq!(doc.check_free_page_name("", None), Err(DocError::EmptyName));
}

#[test]
fn page_named_finds_the_page_that_has_a_name_in_any_case() {
    let mut doc = Document::new_blank();
    let second = doc.add_page(Some("Overview")).unwrap();
    assert_eq!(doc.page_named("OVERVIEW", None).map(|p| p.id), Some(second));
    assert_eq!(doc.page_named("OVERVIEW", Some(second)).map(|p| p.id), None);
    assert_eq!(doc.page_named("Nothing", None).map(|p| p.id), None);
}

#[test]
fn a_page_added_with_no_name_is_called_by_a_name_no_page_has_in_either_name() {
    let mut doc = Document::new_blank();
    let mut other = Page::new(1, "Overview");
    other.name_u = Some(Arc::from("Page-2"));
    doc.pages = vec![Arc::new(other)].into_iter().collect();
    // The next id is 2, and page 1's universal name is Page-2, so the new page does not take it for its own: it is called Page-3, in both names.
    let id = doc.add_page(None).unwrap();
    assert_eq!(id, 2);
    let page = doc.page(id).unwrap();
    assert_eq!((&*page.name, page.name_u.as_deref()), ("Page-3", Some("Page-3")));
    // A name of another case is taken too.
    doc.pages.push(Arc::new(Page::new(9, "page-10")));
    let id = doc.add_page(None).unwrap();
    assert_eq!(id, 10);
    assert_eq!(&*doc.page(id).unwrap().name, "Page-11");
}

#[test]
fn a_page_added_with_a_name_is_named_as_asked_even_when_another_has_it() {
    // The model takes the names of a file as they are (an importer names pages as its file does): the commands hold a name to the rules, the model does not.
    let mut doc = Document::new_blank();
    let id = doc.add_page(Some("Page-1")).unwrap();
    assert_eq!(&*doc.page(id).unwrap().name, "Page-1");
    let id = doc.add_page(Some("page7")).unwrap();
    assert_eq!(&*doc.page(id).unwrap().name, "page7");
}
