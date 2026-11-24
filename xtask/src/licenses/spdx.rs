//! SPDX licence expressions, as crates write them in `Cargo.toml` (`MIT OR Apache-2.0`, `(MIT OR Apache-2.0) AND Unicode-3.0`, the old
//! `MIT/Apache-2.0`): parsing them, and choosing which licence DiaCraft uses a crate under.
//!
//! **The choice.** An `OR` offers alternatives and DiaCraft takes one of the licences its own policy allows (`deny.toml`). It takes
//! the best-ranked: MIT where it is offered (DiaCraft's own licence), then Apache-2.0, then the other permissive ones ([`rank`]). An `AND` asks
//! for all its parts. A licence outside the allowed list is never chosen: an expression that offers no allowed way is an error.

use std::collections::BTreeSet;

/// Longest expression read, and deepest parenthesis: a crate's manifest is not trusted to be small.
const MOST_BYTES: usize = 4096;
const MOST_DEPTH: usize = 16;

/// A parsed expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// One licence, or one with an exception (`Apache-2.0 WITH LLVM-exception`, kept as that text).
    Id(String),
    /// Every part applies.
    And(Vec<Expr>),
    /// One of the parts applies, at the user's choice.
    Or(Vec<Expr>),
}

/// Parses `text`. The old `/` between licences means `OR`.
pub fn parse(text: &str) -> Result<Expr, String> {
    if text.len() > MOST_BYTES {
        return Err(format!("the licence expression is {} bytes long, more than {MOST_BYTES}", text.len()));
    }
    let tokens = tokenize(text);
    let mut at = 0;
    let expr = parse_or(&tokens, &mut at, 0)?;
    match tokens.get(at) {
        None => Ok(expr),
        Some(extra) => Err(format!("unexpected `{extra}` in the licence expression `{text}`")),
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    for c in text.chars() {
        match c {
            '(' | ')' => {
                flush(&mut word, &mut tokens);
                tokens.push(c.to_string());
            }
            // The old way of writing OR.
            '/' => {
                flush(&mut word, &mut tokens);
                tokens.push("OR".to_owned());
            }
            c if c.is_whitespace() => flush(&mut word, &mut tokens),
            c => word.push(c),
        }
    }
    flush(&mut word, &mut tokens);
    tokens
}

fn is_keyword(token: &str, word: &str) -> bool {
    token.eq_ignore_ascii_case(word)
}

fn parse_or(tokens: &[String], at: &mut usize, depth: usize) -> Result<Expr, String> {
    let mut parts = vec![parse_and(tokens, at, depth)?];
    while tokens.get(*at).is_some_and(|t| is_keyword(t, "OR")) {
        *at += 1;
        parts.push(parse_and(tokens, at, depth)?);
    }
    Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::Or(parts) })
}

fn parse_and(tokens: &[String], at: &mut usize, depth: usize) -> Result<Expr, String> {
    let mut parts = vec![parse_atom(tokens, at, depth)?];
    while tokens.get(*at).is_some_and(|t| is_keyword(t, "AND")) {
        *at += 1;
        parts.push(parse_atom(tokens, at, depth)?);
    }
    Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::And(parts) })
}

fn parse_atom(tokens: &[String], at: &mut usize, depth: usize) -> Result<Expr, String> {
    let token = tokens.get(*at).ok_or("the licence expression ends where a licence was expected")?;
    *at += 1;
    if token == "(" {
        if depth >= MOST_DEPTH {
            return Err(format!("the licence expression nests parentheses deeper than {MOST_DEPTH}"));
        }
        let inner = parse_or(tokens, at, depth + 1)?;
        return match tokens.get(*at) {
            Some(close) if close == ")" => {
                *at += 1;
                Ok(inner)
            }
            _ => Err("a parenthesis in the licence expression is not closed".to_owned()),
        };
    }
    if token == ")" || is_keyword(token, "AND") || is_keyword(token, "OR") || is_keyword(token, "WITH") {
        return Err(format!("`{token}` where a licence was expected"));
    }
    if tokens.get(*at).is_some_and(|t| is_keyword(t, "WITH")) {
        let exception = tokens.get(*at + 1).ok_or_else(|| format!("`{token} WITH` has no exception after it"))?;
        *at += 2;
        return Ok(Expr::Id(format!("{token} WITH {exception}")));
    }
    Ok(Expr::Id(token.clone()))
}

/// How much DiaCraft prefers a licence when a crate offers several: lower is better. MIT is DiaCraft's own, then Apache-2.0.
pub fn rank(id: &str) -> u32 {
    match base_id(id) {
        "MIT" => 0,
        "Apache-2.0" => 1,
        "BSD-2-Clause" => 2,
        "BSD-3-Clause" => 3,
        "ISC" => 4,
        "Zlib" => 5,
        "Unicode-3.0" => 6,
        "BSL-1.0" => 7,
        "0BSD" => 8,
        "CC0-1.0" => 9,
        "Unlicense" => 10,
        "MPL-2.0" => 20,
        _ => 50,
    }
}

/// The licence without its exception: `Apache-2.0 WITH LLVM-exception` is `Apache-2.0`.
pub fn base_id(id: &str) -> &str {
    id.split(" WITH ").next().unwrap_or(id)
}

/// Every licence id the expression names, offered or not.
pub fn ids(expr: &Expr) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect(expr, &mut out);
    out
}

fn collect(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Id(id) => {
            out.insert(id.clone());
        }
        Expr::And(parts) | Expr::Or(parts) => parts.iter().for_each(|p| collect(p, out)),
    }
}

/// The licences DiaCraft uses the expression's crate under, in the order the expression names them, or the licences that stand in the way
/// (the ones `allowed` refuses, in every alternative that was tried).
pub fn choose(expr: &Expr, allowed: &dyn Fn(&str) -> bool) -> Result<Vec<String>, Vec<String>> {
    match expr {
        Expr::Id(id) => {
            if allowed(id) {
                Ok(vec![id.clone()])
            } else {
                Err(vec![id.clone()])
            }
        }
        Expr::And(parts) => {
            let mut chosen: Vec<String> = Vec::new();
            let mut refused: Vec<String> = Vec::new();
            for part in parts {
                match choose(part, allowed) {
                    Ok(ids) => {
                        for id in ids {
                            if !chosen.contains(&id) {
                                chosen.push(id);
                            }
                        }
                    }
                    Err(ids) => refused.extend(ids),
                }
            }
            if refused.is_empty() { Ok(chosen) } else { Err(refused) }
        }
        Expr::Or(parts) => {
            let mut best: Option<(u32, Vec<String>)> = None;
            let mut refused: Vec<String> = Vec::new();
            for part in parts {
                match choose(part, allowed) {
                    Ok(ids) => {
                        let score = ids.iter().map(|id| rank(id)).sum::<u32>();
                        // The first of equal scores wins: the order the crate wrote them in.
                        if best.as_ref().is_none_or(|(s, _)| score < *s) {
                            best = Some((score, ids));
                        }
                    }
                    Err(ids) => refused.extend(ids),
                }
            }
            best.map(|(_, ids)| ids).ok_or(refused)
        }
    }
}
