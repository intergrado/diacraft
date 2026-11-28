//! Which web addresses a help link may open, and the address of a feedback page.

use diacraft_help::url::{allowed, allowed_by, encode_body, feedback_url, feedback_url_for};
use diacraft_help::{MAX_FEEDBACK_BODY_BYTES, host};

#[test]
fn the_three_sites_are_allowed_whole_or_followed_by_a_path_a_query_or_a_fragment() {
    for ok in [
        "https://diacraft.dev",
        "https://diacraft.dev/",
        "https://diacraft.dev/help/page-setup",
        "https://diacraft.dev/help/page-setup#margins",
        "https://diacraft.dev?x=1",
        "https://diacraft.dev#top",
        "https://diacraft.app/",
        "https://diacraft.app/?open=1",
        "https://github.com/intergrado/diacraft",
        "https://github.com/intergrado/diacraft/",
        "https://github.com/intergrado/diacraft/issues/new?body=a%20b",
    ] {
        assert!(allowed(ok), "{ok}");
    }
}

#[test]
fn a_look_alike_host_a_user_name_a_port_another_scheme_or_another_repository_is_refused() {
    for bad in [
        "https://diacraft.dev.evil/",
        "https://diacraft.dev.evil",
        "https://x@diacraft.dev",
        "https://x@diacraft.dev/",
        "https://diacraft.dev@evil.com/",
        "https://diacraft.dev:8080/",
        "https://diacraft.devx/",
        "https://evil-diacraft.dev/",
        "https://sub.diacraft.dev/",
        "http://diacraft.dev/",
        "HTTPS://diacraft.dev/",
        "https://DIACRAFT.DEV/",
        "//diacraft.dev/",
        "https:diacraft.dev",
        "https://github.com/intergrado/diacraftx",
        "https://github.com/intergrado/other",
        "https://github.com/intergrado",
        "https://github.com/",
        "https://diacraft.dev/a@b",
        "https://diacraft.dev/?next=https://evil.com@x",
        "",
        "diacraft.dev",
        "javascript:alert(1)",
    ] {
        assert!(!allowed(bad), "{bad}");
    }
}

#[test]
fn white_space_control_characters_backslashes_and_non_ascii_are_refused_anywhere() {
    for bad in [
        " https://diacraft.dev/",
        "https://diacraft.dev/ ",
        "https://diacraft.dev/a b",
        "https://diacraft.dev/\n",
        "https://diacraft.dev/\t",
        "https://diacraft.dev/\u{0}",
        "https://diacraft.dev/\u{7f}",
        "https://diacraft.dev/é",
        "https://diacraft.dev\\evil",
        "https://diacraft.dev/\\evil.com",
        "https://diacraft.dev/\u{202e}",
    ] {
        assert!(!allowed(bad), "{bad:?}");
    }
}

/// A browser takes `.` and `..` out of a path before it asks for the address, so `https://diacraft.dev/guides/../../evil` is read as `https://diacraft.dev/evil` and one
/// with enough of them leaves the allowed folder of a host that has one. No path segment may be a dot segment, however it is spelled.
#[test]
fn a_dot_segment_in_the_path_is_refused_in_every_spelling() {
    for bad in [
        "https://diacraft.dev/guides/../../evil",
        "https://diacraft.dev/..",
        "https://diacraft.dev/.",
        "https://diacraft.dev/a/./b",
        "https://diacraft.dev/a/..",
        "https://diacraft.dev/a/../",
        "https://diacraft.dev/guides/%2e%2e/evil",
        "https://diacraft.dev/guides/%2E%2E/evil",
        "https://diacraft.dev/guides/.%2e/evil",
        "https://diacraft.dev/guides/%2E./evil",
        "https://diacraft.dev/guides/%2e/evil",
        "https://diacraft.dev/guides/%252e%252e/evil",
        "https://diacraft.dev/guides/%25252e%25252e/evil",
        "https://diacraft.dev/a/%2525252e%2525252e/b",
        "https://diacraft.dev/guides/..;/evil",
        "https://diacraft.dev/guides/..%3Bx/evil",
        "https://diacraft.dev/guides/%2e%2e%2fevil",
        "https://github.com/intergrado/diacraft/../other",
        "https://github.com/intergrado/diacraft/%2e%2e/other",
        "https://github.com/intergrado/diacraft/issues/../../../other",
        "https://diacraft.app/../",
        "https://diacraft.app/a/%2e%2E/",
    ] {
        assert!(!allowed(bad), "{bad}");
    }
}

#[test]
fn an_encoded_separator_a_control_character_or_a_broken_escape_in_the_path_is_refused() {
    for bad in [
        "https://diacraft.dev/a%2fb",
        "https://diacraft.dev/a%2Fb",
        "https://diacraft.dev/a%5cb",
        "https://diacraft.dev/a%5Cb",
        "https://diacraft.dev/a%00b",
        "https://diacraft.dev/a%0ab",
        "https://diacraft.dev/a%7fb",
        "https://diacraft.dev/a%",
        "https://diacraft.dev/a%4",
        "https://diacraft.dev/a%zz",
        "https://diacraft.dev/a%2",
        "https://diacraft.dev/?body=a%",
        "https://diacraft.dev/?body=%zz",
        "https://diacraft.dev/#a%4",
        "https://diacraft.dev?x=%",
    ] {
        assert!(!allowed(bad), "{bad}");
    }
}

#[test]
fn a_port_is_refused_even_the_default_one() {
    for bad in ["https://diacraft.dev:443/", "https://diacraft.dev:/", "https://diacraft.dev:0/", "https://github.com:443/intergrado/diacraft"] {
        assert!(!allowed(bad), "{bad}");
    }
}

#[test]
fn what_only_looks_like_a_dot_segment_is_a_path() {
    for ok in [
        "https://diacraft.dev/a..b",
        "https://diacraft.dev/...",
        "https://diacraft.dev/%2e%2e%2e",
        "https://diacraft.dev/.well-known/x",
        "https://diacraft.dev/a/.hidden",
        "https://diacraft.dev/a/b.",
        "https://diacraft.dev/a%20b",
        "https://diacraft.dev/100%25",
        "https://diacraft.dev/caf%C3%A9",
        "https://diacraft.dev//double",
        "https://diacraft.dev/a/",
        // a dot segment in the query or the fragment is no part of the path
        "https://diacraft.dev/?next=/../..",
        "https://diacraft.dev/#/../x",
        "https://diacraft.dev?x=%2e%2e",
        "https://github.com/intergrado/diacraft/issues/new?body=a%0Ab%20c",
    ] {
        assert!(allowed(ok), "{ok}");
    }
}

#[test]
fn a_host_with_a_folder_keeps_its_links_inside_it() {
    let prefixes = ["https://example.org/docs"];
    assert!(allowed_by(&prefixes, "https://example.org/docs/a/b"));
    assert!(!allowed_by(&prefixes, "https://example.org/docs/../secret"));
    assert!(!allowed_by(&prefixes, "https://example.org/docs/%2e%2e/secret"));
    assert!(!allowed_by(&prefixes, "https://example.org/docs/a/../../secret"));
}

#[test]
fn a_host_may_set_its_own_prefixes() {
    let prefixes = ["https://help.example.org", "https://example.org/docs"];
    assert!(allowed_by(&prefixes, "https://help.example.org/a"));
    assert!(allowed_by(&prefixes, "https://example.org/docs"));
    assert!(allowed_by(&prefixes, "https://example.org/docs/x"));
    assert!(!allowed_by(&prefixes, "https://example.org/docsx"));
    assert!(!allowed_by(&prefixes, "https://example.org/"));
    assert!(!allowed_by(&prefixes, "https://diacraft.dev/"));
    assert!(!allowed_by(&[], "https://diacraft.dev/"));
}

#[test]
fn the_hosts_values_agree_with_each_other() {
    assert!(host::WEB_PREFIXES.iter().all(|p| p.starts_with("https://") && !p.ends_with('/')));
    let repository_prefix = format!("https://github.com/{}", host::FEEDBACK_REPOSITORY);
    assert_eq!(host::WEB_PREFIXES[2], repository_prefix);
    assert!(feedback_url_for(host::FEEDBACK_REPOSITORY, "x").is_some(), "the host's repository makes an address");
}

#[test]
fn a_body_is_percent_encoded_by_bytes_and_keeps_only_the_unreserved_characters() {
    assert_eq!(encode_body("abc-._~XYZ019", 100), "abc-._~XYZ019");
    assert_eq!(encode_body("a b", 100), "a%20b");
    assert_eq!(encode_body("a\nb", 100), "a%0Ab");
    assert_eq!(encode_body("é", 100), "%C3%A9");
    assert_eq!(encode_body("DiaCraft 0.2.3, Linux (en)", 100), "DiaCraft%200.2.3%2C%20Linux%20%28en%29");
    assert_eq!(encode_body("&=?#/%", 100), "%26%3D%3F%23%2F%25");
    assert_eq!(encode_body("🙂", 100), "%F0%9F%99%82");
    assert_eq!(encode_body("", 100), "");
}

#[test]
fn a_body_is_cut_at_a_character_and_never_past_the_most() {
    assert_eq!(encode_body("aé", 7), "a%C3%A9");
    assert_eq!(encode_body("aé", 6), "a", "the character is kept whole or not at all");
    assert_eq!(encode_body("é", 5), "");
    assert_eq!(encode_body("abc", 2), "ab");
    assert_eq!(encode_body("abc", 0), "");
    assert_eq!(encode_body("a🙂", 11), "a");
    assert_eq!(encode_body("a🙂", 13), "a%F0%9F%99%82");
    for max in 0..40 {
        let out = encode_body("a é 🙂 b\nc&d", max);
        assert!(out.len() <= max, "{max}: {out}");
        assert!(out.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)));
    }
}

#[test]
fn the_feedback_address_names_the_new_issue_page_with_the_body_filled_in() {
    let url = feedback_url("DiaCraft 0.2.3 (abc123), Linux, en\n\n## What happened\n");
    assert!(url.starts_with("https://github.com/intergrado/diacraft/issues/new?body="), "{url}");
    assert!(url.ends_with("%23%23%20What%20happened%0A"), "{url}");
    assert!(allowed(&url), "{url}");
    assert!(!url.contains(' ') && !url.contains('\n'));
}

#[test]
fn the_feedback_body_is_cut_at_1500_bytes() {
    let long = "x".repeat(5_000);
    let url = feedback_url(&long);
    let body = url.split_once("?body=").unwrap().1;
    assert_eq!(body.len(), MAX_FEEDBACK_BODY_BYTES);
    let accents = feedback_url(&"é".repeat(5_000));
    let body = accents.split_once("?body=").unwrap().1;
    assert!(body.len() <= MAX_FEEDBACK_BODY_BYTES && body.len().is_multiple_of(6), "{}", body.len());
}

#[test]
fn a_repository_that_is_not_owner_and_name_makes_no_address() {
    assert!(feedback_url_for("owner/name", "x").is_some());
    assert!(feedback_url_for("my-org/my.repo_1", "x").is_some());
    for bad in ["", "owner", "owner/", "/name", "a b/c", "../x", "a/b/c", "a/../b", "a/b?x=1", "a/b#x", "é/x", "a@b/c", "a/b\n"] {
        assert_eq!(feedback_url_for(bad, "x"), None, "{bad:?}");
    }
}
