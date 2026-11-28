//! The names a drawing's pages may have.
//!
//! **A drawing's page names are all different, and none is "page" and digits.** The program that opens a VSDX file renames a page called "page" and digits (`page1`, the name an
//! `.odg` gives its pages) and a page whose name another page has, and it gives the renamed page's `Name` and `NameU` different numbers. So a name a command gives a page is held to
//! both rules ([`crate::check_given_page_name`] and [`crate::Document::check_free_page_name`]), a page added without a name is called by a name no page has
//! ([`crate::Document::add_page`]), and the VSDX writers give a page that has such a name the default form (`Page-N`) when they write it. A name a file holds stays as it is.
//!
//! **Names are compared without regard to case** ([`page_name_key`]). No specification says how the program compares them; the rule is ours, and the safe one: a name that is a repeat
//! in another case is a repeat, and a drawing that never writes one cannot be renamed for it.

/// Whether `name` is "page" in any case followed by digits, and nothing more: `page1`, `Page12`, `PAGE007`. The default form of a page's name has a hyphen (`Page-1`) and is no
/// such name, nor is `Page 1`, `page1a` or `pages1`. Only the digits `0` to `9` count.
pub fn is_page_and_digits(name: &str) -> bool {
    match name.as_bytes().split_at_checked(4) {
        Some((head, digits)) => head.eq_ignore_ascii_case(b"page") && !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        None => false,
    }
}

/// The key two page names are compared by: the name in lower case, so that `Overview` and `OVERVIEW` are one name.
pub fn page_name_key(name: &str) -> String {
    name.to_lowercase()
}
