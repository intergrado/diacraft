//! Kani proof harnesses for the model's window settings (`window_flags.rs`): the flags over others and an empty set of flags, the `ID` a window made is given
//! (`next_free`, the number `next_id` writes), the place a flag put in a window goes among its children (`place_among`, `set_flag`'s), and a flag's value from its text
//! (`value_of`); and, the comments: a character a comment may not hold (`comments::bad_char`), the count of their text against its cap
//! (`text_total`, taken out of `add` and `edit`) and the walk that makes a reply's thread one level deep (`thread_root`, taken out of `straighten`). Run with `cargo xtask kani`
//! (`docs/development.md`, Proofs).
//!
//! **Not proved:** `comments::clean` (`trim`, a search for a character and `replace`, which copies a text of a length the solver does not know); `drop_where`
//! (a list of shared chunks of comments, each a struct of texts); `straighten` itself (it keeps a hash map: the futex trap). The unit tests of `comments.rs` hold them.
//!
//! **Not proved here:** `prefix_of` and `local` (a text searched for a character from its end, `rsplit_once`: the memchr trap of `docs/development.md`, Proofs), and the walks of a
//! windows part (`WindowFlags::of`, `written`, `set_flag` itself): they take a tree of `RawNode`s, whose drop calls itself (the drop trap). The unit tests of `window_flags.rs` hold them.

use crate::WindowFlags;
use crate::window_flags::{next_free, place_among, value_of};

/// Five flags of any values.
fn any_flags() -> WindowFlags {
    WindowFlags { rulers: kani::any(), grid: kani::any(), page_breaks: kani::any(), guides: kani::any(), connection_points: kani::any() }
}

/// `WindowFlags::over` and `is_empty` for **every** two and three sets of flags: each flag over another is the first one's where it holds one and the other's where it
/// does not; no flags over any are those, and any over none are themselves; `over` groups either way and takes the upper of two that are given; and `is_empty` is true exactly when
/// none of the five is given.
#[kani::proof]
fn flags_over_others_keep_their_own_and_take_the_rest_from_under() {
    let (a, b, c) = (any_flags(), any_flags(), any_flags());
    let one = |top: Option<bool>, under: Option<bool>, got: Option<bool>| match top {
        Some(v) => got == Some(v),
        None => got == under,
    };
    let ab = a.over(&b);
    assert!(one(a.rulers, b.rulers, ab.rulers) && one(a.grid, b.grid, ab.grid));
    assert!(one(a.guides, b.guides, ab.guides) && one(a.connection_points, b.connection_points, ab.connection_points));
    assert!(one(a.page_breaks, b.page_breaks, ab.page_breaks));
    let none = WindowFlags::default();
    assert!(a.over(&none) == a && none.over(&a) == a && a.over(&a) == a);
    assert!(ab.over(&c) == a.over(&b.over(&c)));
    let given = a.rulers.is_some() || a.grid.is_some() || a.page_breaks.is_some() || a.guides.is_some() || a.connection_points.is_some();
    assert!(a.is_empty() == !given);
    assert!(ab.is_empty() == (a.is_empty() && b.is_empty()));
    kani::cover!(!a.is_empty() & (ab != a), "flags that take some of their values from under");
}

/// `next_free` (the `ID` `next_id` gives a window made in a windows part) for no `ID` and for **every** three whole-number `ID`s whose highest is below `u32::MAX`: 0 for
/// none, and one more than the highest, so no window has it and nothing wraps.
#[kani::proof]
#[kani::unwind(5)]
fn a_window_made_is_given_one_more_than_the_highest_id() {
    assert!(next_free(Vec::new()) == 0);
    let ids: [u32; 3] = kani::any();
    let most = ids[0].max(ids[1]).max(ids[2]);
    kani::assume(most < u32::MAX);
    let got = next_free(ids.to_vec());
    assert!(got == most + 1 && !ids.contains(&got));
}

/// `next_free` for **every** three whole-number `ID`s one of which is `u32::MAX`, in any place: the least number none of them has (each number below it is one of them, so it is at
/// most 3), never one of them, and no wrap or panic in the walk up the sorted `ID`s.
#[kani::proof]
#[kani::unwind(5)]
fn a_window_made_beside_the_highest_id_is_given_the_least_free_one() {
    let ids: [u32; 3] = kani::any();
    kani::assume(ids.contains(&u32::MAX));
    let got = next_free(ids.to_vec());
    assert!(!ids.contains(&got) && got <= 3);
    assert!((0..3).all(|n| n >= got || ids.contains(&n)));
    kani::cover!(got > 0, "the highest is taken and the least free is past 0");
    kani::cover!(got == 0, "the highest is taken and 0 is free");
}

/// `place_among` (where `set_flag` puts a flag a window lacks) for **every** four children, each of a rank of the order (0 to 12) or of none, and every rank: the place
/// is one of the window's places (at most its length); no child of a lower rank comes at it or after it; when there is a child of a lower rank, the one just before the place is one
/// (the flag goes right after the last); and when there is none, no child of a higher rank comes before the place, which is the first such child's or the end.
#[kani::proof]
#[kani::unwind(6)]
fn a_flag_put_in_goes_after_the_last_child_before_it_in_the_order() {
    let ranks: [Option<usize>; 4] = kani::any();
    kani::assume(ranks.iter().all(|r| r.is_none_or(|r| r < 13)));
    let rank: usize = kani::any();
    kani::assume(rank < 13);
    let at = place_among(ranks.iter().copied(), rank);
    let lower = |r: &Option<usize>| r.is_some_and(|r| r < rank);
    let higher = |r: &Option<usize>| r.is_some_and(|r| r > rank);
    assert!(at <= ranks.len());
    let (before, after) = ranks.split_at(at);
    assert!(!after.iter().any(lower));
    if ranks.iter().any(lower) {
        assert!(before.last().is_some_and(lower));
    } else {
        assert!(!before.iter().any(higher));
        assert!(after.first().is_none_or(higher));
    }
    kani::cover!((at > 0) & (at < ranks.len()), "a flag put in among the children");
    kani::cover!((at == 0) & ranks.iter().any(higher), "a flag put in before a child that comes after it");
}

/// The characters a flag's text is drawn from: the letters of `true` and `false`, the two digits, a digit that is neither, two blanks and a letter that is in no word.
const VALUE_KINDS: &[u8; 14] = b"10truefals \t9x";

/// `value_of` of every text of `N` characters of [`VALUE_KINDS`]: on exactly for `1` and `true`, off exactly for `0` and `false`, blanks (a space, a tab) around them
/// aside, and no value for every other text. (The blanks beyond ASCII that `str::trim` also takes are the standard library's, and are not drawn here.)
fn value_law<const N: usize>() -> Option<bool> {
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < VALUE_KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| VALUE_KINDS[usize::from(k)]);
    let text = std::str::from_utf8(&raw).unwrap_or("x");
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let first = raw.iter().position(|b| !blank(b)).unwrap_or(N);
    let end = raw.iter().rposition(|b| !blank(b)).map_or(first, |i| i + 1);
    let word = raw.get(first..end).unwrap_or(&[]);
    let got = value_of(text);
    let want = match word {
        b"1" | b"true" => Some(true),
        b"0" | b"false" => Some(false),
        _ => None,
    };
    assert!(got == want);
    got
}

macro_rules! value_law {
    ($($name:ident: $n:literal),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind(9)]
            fn $name() {
                let got = value_law::<$n>();
                kani::cover!(got.is_none(), "a text that is no value");
                kani::cover!(got == Some(true), "on");
                kani::cover!(got == Some(false), "off");
            }
        )*
    };
}

value_law! {
    a_flag_of_one_character_is_a_digit_of_the_two_or_none: 1,
    a_flag_of_five_characters_is_false_or_true_beside_a_blank_or_none: 5,
    a_flag_of_seven_characters_is_a_word_between_blanks_or_none: 7,
}

// ----: the comments of a document ----

/// `comments::bad_char` (a character a comment may not hold) for **every** character: exactly the control characters (U+0000 to U+001F and U+007F to U+009F)
/// other than a tab, a line feed and a carriage return, and the two characters XML cannot hold, U+FFFE and U+FFFF.
#[kani::proof]
fn a_comment_may_hold_every_character_but_the_controls_other_than_line_breaks_and_tabs_and_two_xml_refuses() {
    let c: char = kani::any();
    let n = u32::from(c);
    let control = n <= 0x1F || (0x7F..=0x9F).contains(&n);
    let want = (control && !(n == 0x09 || n == 0x0A || n == 0x0D)) || n == 0xFFFE || n == 0xFFFF;
    assert!(crate::comments::bad_char(c) == want);
    kani::cover!(want & (n == 0x85), "a control of the second range");
    kani::cover!(!want & (n == 0x0D), "a carriage return");
}

/// `comments::text_total` (the bytes of text of all the comments after `add` and `edit`, against the 32 MiB cap) for **every** three counts: a total exactly when
/// it is within the cap, and so never past it; from a total within the cap, the exact sum of what is held less the old text and the new one; and an addition is the old text 0.
#[kani::proof]
fn the_text_of_the_comments_is_counted_exactly_and_never_passes_its_cap() {
    use crate::comments::{MAX_COMMENTS_TEXT_BYTES, text_total};
    let (held, old, new): (u64, u64, u64) = (kani::any(), kani::any(), kani::any());
    let cap = MAX_COMMENTS_TEXT_BYTES as u64;
    let got = text_total(held, old, new);
    if let Some(t) = got {
        assert!(t <= cap);
    }
    if held <= cap && old <= held {
        let exact = u128::from(held - old) + u128::from(new);
        assert!(got.is_some() == (exact <= u128::from(cap)));
        assert!(got.is_none_or(|t| u128::from(t) == exact));
    }
    kani::cover!(got == Some(cap), "a total of the whole cap");
    kani::cover!(got.is_none() & (held <= cap) & (old <= held) & (new <= cap), "a new text that does not fit");
}

/// The comments of a list of three, ids 1 to 3, for the walk of `thread_root`: the place of a comment by its id.
fn place_of(id: u32) -> Option<usize> {
    (1..=3).contains(&id).then(|| (id - 1) as usize)
}

/// `comments::thread_root` (`straighten`'s walk, which makes a file's replies one level deep) for **every** list of three comments with ids 1 to 3 and any parents
/// (a parent of 0 or 4 is in no list): for each comment that has a parent, the root is the comment at the end of the chain of its parents, the first that has none, and none
/// exactly when the chain leaves the list or runs round (a comment its own parent, two that answer each other, three in a ring); so a root is in the list, has no parent and is
/// not the comment itself. (Four comments took 50 s at load 25, three 31 s; the walk is `MAX_THREAD_STEPS` steps long round a ring.)
#[kani::proof]
#[kani::unwind(66)]
fn a_replys_thread_is_the_comment_at_the_end_of_its_chain_of_parents_and_none_where_the_chain_runs_round() {
    use crate::comments::thread_root;
    let parents: [Option<u32>; 3] = kani::any();
    kani::assume(parents.iter().all(|p| p.is_none_or(|p| p <= 4)));
    let at: usize = kani::any();
    kani::assume(at < 3);
    let id = (at + 1) as u32;
    let Some(first) = parents[at] else { return };
    let got = thread_root(id, first, &parents, place_of);
    // The chain of parents walked plainly: three steps reach the end of any chain of three comments that does not run round.
    let mut cursor = first;
    let mut end = None;
    for _ in 0..3 {
        match place_of(cursor) {
            None => break,
            Some(i) => match parents[i] {
                None => {
                    end = Some(cursor);
                    break;
                }
                Some(next) => cursor = next,
            },
        }
    }
    assert!(got == end);
    if let Some(r) = got {
        assert!(r != id && place_of(r).is_some_and(|i| parents[i].is_none()));
    }
    kani::cover!(got.is_some_and(|r| r != first), "a reply to a reply moved to its thread");
    kani::cover!(got.is_none() & place_of(first).is_some(), "a chain that runs round");
    kani::cover!(got.is_none() & place_of(first).is_none(), "a parent the list does not have");
}
