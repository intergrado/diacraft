//! Telling which licence a text is, from what it says, and the standard texts for a licence a crate ships no text of.
//!
//! A file's name is not trusted (`LICENSE` can hold anything, and one file can hold two licences): [`classes`] reads the words. It returns SPDX ids for
//! the licences DiaCraft allows, and a `-family` name for the ones it does not (`GPL-family`, `LGPL-family`, `AGPL-family`, `CC-BY-family`,
//! `BSD-4-Clause`, `Unicode-DFS-2016`), so that a copyleft text hidden in a crate that calls itself MIT is seen. A text it knows none of has no class.

/// The words of `text` in lower case with one space between them, and comment marks (`//`, `#`, `*`) taken off the start of each line.
pub fn squash(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.trim_start().trim_start_matches(['/', '#', '*', ';']).trim();
        for word in line.split_whitespace() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&word.to_lowercase());
        }
    }
    out
}

/// Every licence `text` is (or holds), by SPDX id or family name, in a fixed order.
pub fn classes(text: &str) -> Vec<&'static str> {
    let t = squash(text);
    let has = |s: &str| t.contains(s);
    let mut out: Vec<&'static str> = Vec::new();
    if has("permission is hereby granted, free of charge, to any person obtaining a copy of this software") {
        out.push("MIT");
    }
    if has("apache license") && has("version 2.0") && has("terms and conditions for use, reproduction, and distribution") {
        out.push("Apache-2.0");
    }
    if has("redistribution and use in source and binary forms, with or without modification, are permitted") {
        if has("all advertising materials mentioning features") {
            out.push("BSD-4-Clause");
        } else if has("endorse or promote products derived") {
            out.push("BSD-3-Clause");
        } else {
            out.push("BSD-2-Clause");
        }
    }
    if has("permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted") {
        out.push(if has("provided that the above copyright notice and this permission notice appear in all copies") { "ISC" } else { "0BSD" });
    }
    if has("altered source versions must be plainly marked") {
        out.push("Zlib");
    }
    if has("boost software license") && has("version 1.0") {
        out.push("BSL-1.0");
    }
    if has("unicode license v3") {
        out.push("Unicode-3.0");
    }
    if has("unicode, inc. license agreement") {
        out.push("Unicode-DFS-2016");
    }
    if has("this is free and unencumbered software released into the public domain") {
        out.push("Unlicense");
    }
    if has("mozilla public license") && has("version 2.0") {
        out.push("MPL-2.0");
    }
    if has("sil open font license") && has("version 1.1") {
        out.push("OFL-1.1");
    }
    if has("ubuntu font licence") && has("version 1.0") {
        out.push("Ubuntu-font-1.0");
    }
    if has("cc0 1.0 universal") {
        out.push("CC0-1.0");
    } else if has("creative commons") && has("attribution") {
        out.push("CC-BY-family");
    }
    // The GNU texts name each other (the GPL ends by pointing to the Lesser one), so the title that comes first, near the start, says which one a text is.
    let head: String = t.chars().take(300).collect();
    let first = [
        ("gnu affero general public license", "AGPL-family"),
        ("gnu lesser general public license", "LGPL-family"),
        ("gnu library general public license", "LGPL-family"),
        ("gnu general public license", "GPL-family"),
    ]
    .iter()
    .filter_map(|(title, class)| head.find(title).map(|at| (at, *class)))
    .min();
    if let Some((_, class)) = first {
        out.push(class);
    }
    out
}

/// The class a licence id of a crate's expression is read as: `GPL-2.0-only` is `GPL-family`, an exception is dropped.
pub fn class_of_id(id: &str) -> String {
    let base = super::spdx::base_id(id);
    if base.starts_with("AGPL-") {
        "AGPL-family".to_owned()
    } else if base.starts_with("LGPL-") {
        "LGPL-family".to_owned()
    } else if base.starts_with("GPL-") {
        "GPL-family".to_owned()
    } else {
        base.to_owned()
    }
}

/// The standard text of a licence, with `holders` as its copyright holders, for a crate that ships none of its own. Only the short permissive licences
/// have one: for any other licence a crate must ship the text.
pub fn standard_text(class: &str, holders: &str) -> Option<String> {
    let template = match class {
        "MIT" => include_str!("texts/MIT.txt"),
        "BSD-2-Clause" => include_str!("texts/BSD-2-Clause.txt"),
        "BSD-3-Clause" => include_str!("texts/BSD-3-Clause.txt"),
        "ISC" => include_str!("texts/ISC.txt"),
        "Zlib" => include_str!("texts/Zlib.txt"),
        "BSL-1.0" => include_str!("texts/BSL-1.0.txt"),
        // The Apache text has no copyright line: the project's own copy is the standard one.
        "Apache-2.0" => return Some(include_str!("../../../LICENSE-APACHE").trim().to_owned()),
        _ => return None,
    };
    Some(template.replace("@HOLDERS@", holders).trim().to_owned())
}

/// The name a text is listed under.
pub fn title(class: &str) -> &'static str {
    match class {
        "MIT" => "MIT License",
        "Apache-2.0" => "Apache License 2.0",
        "BSD-2-Clause" => "BSD 2-Clause License",
        "BSD-3-Clause" => "BSD 3-Clause License",
        "ISC" => "ISC License",
        "0BSD" => "Zero-Clause BSD License",
        "Zlib" => "zlib License",
        "BSL-1.0" => "Boost Software License 1.0",
        "Unicode-3.0" => "Unicode License v3",
        "Unlicense" => "The Unlicense",
        "MPL-2.0" => "Mozilla Public License 2.0",
        "OFL-1.1" => "SIL Open Font License 1.1",
        "Ubuntu-font-1.0" => "Ubuntu Font Licence 1.0",
        "CC0-1.0" => "CC0 1.0 Universal",
        "LGPL-family" => "GNU Lesser General Public License",
        "GPL-family" => "GNU General Public License",
        "AGPL-family" => "GNU Affero General Public License",
        _ => "Licence text",
    }
}
