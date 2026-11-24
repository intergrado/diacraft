//! Kani proof harnesses for the container's integer code of the threads of a read, a save and a frame (`spare::fit`, the one rule, and `spare::beside`, a
//! read's; `par::from_setting`, the count `DIACRAFT_THREADS` and the cores give), and for a value of the canonical form (`canon::Val`, its order, its equality and its hash);
//! and, a model of a lane's protocol on its own worker counts (`par::Workers`). Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved:** a signature's writers and the interner of `canon::names` (`Sig::put`, `Sig::num`, `Interner::intern`: a signature holds a `&mut` of a hash
//! map of names, and `HashMap::new()` alone reaches the futex `syscall`, which Kani 0.68 does not support: a probe failed at once), `weight` (a tree of `El`, which holds elements of
//! its own kind: the drop trap), `resolve_attr` (the splitting of a list of names); `put_in_order`, the order the definitions a name is resolved through are put in (a
//! walk on an explicit stack of the graph of the definitions, a loop in a loop in a loop; three definitions that name one each, and two that name two each, ran past 300 s, with the
//! law stated by the graph's closure).
//!
//! **Not proved here:** `spare::threads_fit` and `threads_beside` themselves (they read the process's limit and its address space from `/proc`, which Kani has no model of; what they add
//! to `fit` and `beside` is a clamp); a total order of three values (`Val::cmp` is the order of a pair, the kind and the text, then the bits, which the standard library's order of
//! tuples makes total; only what the canonical form adds to it is asked); `Builder::placing`'s choice and `with_attr` (a choice among the fixed words of a format's rules, and a tree of
//! the canonical form, whose drop calls itself); `holds_an_empty_root`'s guard (it looks at a format's predicate of a name, `is_rels_part` for VSDX, which searches the name for `/`
//! from its end: the memchr trap); the plan of an archive's reads (`plan_inflate`, `count_read`: the state of an archive is made only by reading one). The unit tests hold them.

use crate::canon::{Hash, Val};
use crate::par::{MAX_THREADS, from_setting};
use crate::spare::{READ_THREADS_MOST, THREAD_COST_BYTES, beside, fit};

/// The bytes what is left of a limit must hold for each thread beside the caller's: eight times the cost of one (a thread is given an eighth of what is left).
const EACH: u128 = 8 * THREAD_COST_BYTES as u128;

/// What the limit leaves beside what is in use and the room, as `fit` takes it: 0 when they pass it.
fn left_of(limit: u64, used: u64, room: u64) -> u128 {
    u128::from(limit).saturating_sub(u128::from(used)).saturating_sub(u128::from(room))
}

/// `spare::fit` (the rule a read, a save and a frame are held by) for **every** count wanted (from 1), limit, figure of the address space in use and room: all of them
/// with no limit; one (no thread made) with a limit and no figure for what is in use; and with both, **the most of the wanted that fit**: the threads beside the caller's, each at an
/// eighth of what is left, fit in what the limit leaves beside what is in use and the room, and one more would not, unless every one wanted is given. It never gives none, never more
/// than were wanted, and does not wrap at the ends of the numbers. Stated by products (`(n - 1) 8 c <= left < n 8 c`), so the harness has no divider of its own; it follows that the
/// count does not grow when what is in use or the room grows.
#[kani::proof]
fn a_piece_of_work_is_given_the_threads_that_fit_in_an_eighth_of_what_its_limit_leaves() {
    let wanted: usize = kani::any();
    kani::assume(wanted >= 1);
    let (limit, used, room): (Option<u64>, Option<u64>, u64) = (kani::any(), kani::any(), kani::any());
    let got = fit(wanted, limit, used, room);
    assert!(got >= 1 && got <= wanted);
    match (limit, used) {
        (None, _) => {
            assert!(got == wanted);
        }
        (Some(_), None) => {
            assert!(got == 1);
        }
        (Some(limit), Some(used)) => {
            let left = left_of(limit, used, room);
            let n = got as u128;
            assert!((n - 1) * EACH <= left);
            assert!(got == wanted || n * EACH > left);
            kani::cover!((got > 1) & (got < wanted), "some of the threads wanted fit");
            kani::cover!((got == wanted) & (wanted > 1) & (left < u128::from(u64::MAX)), "every thread wanted fits under a limit");
        }
    }
}

/// `spare::beside` (a read's threads) for **every** count wanted, limit, figure in use and room: the rule of `fit` for the count held to 1 to `READ_THREADS_MOST`, so never
/// more than eight, never none, never more than were wanted (none wanted is the caller's thread), and all of them with no limit.
#[kani::proof]
fn a_read_is_given_the_threads_that_fit_and_never_more_than_eight() {
    let wanted: usize = kani::any();
    let (limit, used, room): (Option<u64>, Option<u64>, u64) = (kani::any(), kani::any(), kani::any());
    let got = beside(wanted, limit, used, room);
    let held = wanted.clamp(1, READ_THREADS_MOST);
    assert!(got >= 1 && got <= held && got <= READ_THREADS_MOST);
    match (limit, used) {
        (None, _) => {
            assert!(got == held);
        }
        (Some(_), None) => {
            assert!(got == 1);
        }
        (Some(limit), Some(used)) => {
            let left = left_of(limit, used, room);
            let n = got as u128;
            assert!((n - 1) * EACH <= left && (got == held || n * EACH > left));
        }
    }
    kani::cover!((got == READ_THREADS_MOST) & (wanted > READ_THREADS_MOST), "a read asked for more than eight");
}

/// The characters a setting of `DIACRAFT_THREADS` is drawn from: digits (the two ends and one between), a sign of each kind, two blanks and a letter.
const SETTING_KINDS: &[u8; 8] = b"079+- \tx";

/// `par::from_setting` of every text of `N` characters of [`SETTING_KINDS`] and every count of cores: the setting when it is a whole number (blanks around it aside, a
/// plus before it allowed, as `usize`'s parse takes it), held to 1 to 64; else the cores less one, held to 1 to 64. So the count is always 1 to 64.
fn setting_law<const N: usize>() -> usize {
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < SETTING_KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| SETTING_KINDS[usize::from(k)]);
    let text = std::str::from_utf8(&raw).unwrap_or("x");
    let cores: usize = kani::any();
    let got = from_setting(Some(text), cores);
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let first = raw.iter().position(|b| !blank(b)).unwrap_or(N);
    let end = raw.iter().rposition(|b| !blank(b)).map_or(first, |i| i + 1);
    let word = raw.get(first..end).unwrap_or(&[]);
    let digits = word.strip_prefix(b"+").unwrap_or(word);
    let number = !digits.is_empty() && digits.iter().all(u8::is_ascii_digit);
    let value = digits.iter().fold(0usize, |n, b| n * 10 + usize::from(b.wrapping_sub(b'0')));
    let want = if number { value.clamp(1, MAX_THREADS) } else { cores.saturating_sub(1).clamp(1, MAX_THREADS) };
    assert!(got == want && (1..=MAX_THREADS).contains(&got));
    if number { got } else { 0 }
}

macro_rules! setting_law {
    ($($name:ident: $n:literal),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind(7)]
            fn $name() {
                let got = setting_law::<$n>();
                kani::cover!(got == 0, "a setting that is no number, and the cores' count");
                kani::cover!(got > 0, "a setting that is a number");
            }
        )*
    };
}

setting_law! {
    a_setting_of_one_character_is_a_digit_or_the_cores_count: 1,
    a_setting_of_three_characters_is_a_number_between_blanks_or_the_cores_count: 3,
    a_setting_of_four_characters_is_a_number_or_the_cores_count: 4,
}

/// The count with no setting, or one that is no text of `DIACRAFT_THREADS`, for **every** count of cores: the cores less one (a core is left to the window), held to 1 to 64.
#[kani::proof]
fn with_no_setting_the_threads_are_the_cores_less_one_held_to_one_to_sixty_four() {
    let cores: usize = kani::any();
    let got = from_setting(None, cores);
    assert!((1..=MAX_THREADS).contains(&got));
    assert!(
        got == if cores <= 2 {
            1
        } else if cores > MAX_THREADS {
            MAX_THREADS
        } else {
            cores - 1
        }
    );
    kani::cover!(got == MAX_THREADS, "a machine of more cores than a count is held to");
}

/// The characters the texts of a value are drawn from.
const TEXT_KINDS: &[u8; 2] = b"ab";

/// A text of up to two characters of [`TEXT_KINDS`], of a symbolic length.
fn any_text(raw: &mut [u8; 2]) -> &str {
    let kinds: [u8; 2] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < TEXT_KINDS.len()));
    *raw = kinds.map(|k| TEXT_KINDS[usize::from(k)]);
    let len: usize = kani::any();
    kani::assume(len <= 2);
    std::str::from_utf8(raw.get(..len).unwrap_or(&[])).unwrap_or("")
}

/// A value of the canonical form of a kind of the three (`0` text, `1` a number of the unit `text`, `2` an owned text), its text `text` and its bits `bits`.
fn value_of<'a>(kind: u8, text: &'a str, bits: u64) -> Val<'a> {
    match kind {
        0 => Val::Text(text),
        1 => Val::Num(bits, text),
        _ => Val::Owned(text.to_owned()),
    }
}

/// The hash of a value as the canonical form feeds it.
fn hash_of(v: &Val<'_>) -> u64 {
    let mut h = Hash::new(7);
    v.feed(&mut h);
    h.finish()
}

/// `canon::Val` for **every** two values of the three kinds, texts of up to two characters and any bits: **a text and an owned text of one string are one value**
/// (equal both ways); a number is never a text; two values are equal exactly when they compare `Equal`, which is when they are texts of one string or numbers of one unit and the
/// same bits; the order is antisymmetric (`b.cmp(a)` is `a.cmp(b)` reversed) and puts every text before every number. (Its totality is the order of a tuple's.)
#[kani::proof]
#[kani::unwind(4)]
fn a_text_and_an_owned_text_of_one_string_are_one_value_and_a_number_is_never_a_text() {
    use std::cmp::Ordering;
    let (ka, kb): (u8, u8) = (kani::any(), kani::any());
    kani::assume(ka < 3 && kb < 3);
    let (mut ra, mut rb) = ([0u8; 2], [0u8; 2]);
    let (ta, tb) = (any_text(&mut ra), any_text(&mut rb));
    let (ba, bb): (u64, u64) = (kani::any(), kani::any());
    let (a, b) = (value_of(ka, ta, ba), value_of(kb, tb, bb));
    let order = a.cmp(&b);
    assert!(b.cmp(&a) == order.reverse());
    assert!((a == b) == (order == Ordering::Equal));
    let (num_a, num_b) = (ka == 1, kb == 1);
    let same = num_a == num_b && ta == tb && (!num_a || ba == bb);
    assert!((a == b) == same);
    if num_a != num_b {
        assert!(order == if num_a { Ordering::Greater } else { Ordering::Less });
    }
    kani::cover!((ka == 0) & (kb == 2) & (a == b) & !ta.is_empty(), "a text and an owned text of one string");
    kani::cover!((ka == 1) & (kb == 1) & (a != b) & (ta == tb), "two numbers of one unit and other bits");
}

/// `Val::feed`: a text and an owned text of **every** string of up to two characters are hashed alike, as they are one value (the hash of the canonical form must agree
/// with its equality, or a child would not be matched by it).
#[kani::proof]
#[kani::unwind(10)]
fn a_text_and_an_owned_text_of_one_string_are_hashed_alike() {
    let mut raw = [0u8; 2];
    let text = any_text(&mut raw);
    assert!(hash_of(&Val::Text(text)) == hash_of(&Val::Owned(text.to_owned())));
}

// ---: the workers of a lane ---

/// The steps of the lane's model in `a_lane_never_starts_more_workers_than_tasks_in_flight_whatever_order_its_steps_run_in`.
const LANE_STEPS: usize = 8;

/// `par::Workers` (the counts of a lane's workers, which `Lane::submit` and the loop of a worker change, each in one hold of the lock) in **every order of up to eight
/// holds of the lock**, at most one to four workers allowed: a task given (and started a worker, or found that the system makes none, or run on the caller's thread when there is no
/// worker), a free worker taking a task, a busy one making its result, a free one going when the lane is closed and no task waits, the caller taking a result, and the lane closing (the
/// tasks that wait are dropped). **This is a model of the lane's protocol** (which step may come when, as `submit`, `work` and `Closer` take them), on the code's own counts and its own
/// rule of when a worker is started (`Workers::wants_one`), not the threads themselves, which Kani does not run. Through every order: the free workers, the busy ones and the ones gone
/// are the workers started; no more are started than allowed; and **a worker is started only when the tasks in flight (waiting or running) are more than the workers**, so the
/// workers are never more than the most tasks in flight at once (the flake: a worker counted busy after it had made its result, and a caller that took one task at a time had three
/// to eight).
#[kani::proof]
#[kani::unwind(9)]
fn a_lane_never_starts_more_workers_than_tasks_in_flight_whatever_order_its_steps_run_in() {
    use crate::par::Workers;
    let most: usize = kani::any();
    kani::assume((1..=4).contains(&most));
    let mut workers = Workers::default();
    let (mut queued, mut busy, mut gone, mut done, mut peak) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut closing, mut no_more_threads) = (false, false);
    for _ in 0..LANE_STEPS {
        let step: u8 = kani::any();
        match step {
            // `submit`: the task waits, a worker is started if the rule says so (or the system makes none), and with no worker at all it runs here, now.
            0 if !closing => {
                queued += 1;
                if workers.wants_one(queued, most) && !no_more_threads {
                    if kani::any() {
                        workers.started_one();
                        assert!(workers.started <= queued + busy);
                    } else {
                        no_more_threads = true;
                    }
                }
                if workers.started == 0 {
                    queued -= 1;
                    done += 1;
                }
            }
            // A free worker takes the first task that waits.
            1 if queued > 0 && workers.free > 0 => {
                queued -= 1;
                busy += 1;
                workers.took();
            }
            // A busy worker makes its result, and is free again in the same hold.
            2 if busy > 0 => {
                busy -= 1;
                done += 1;
                workers.made();
            }
            // A free worker goes: the lane is closing and no task waits.
            3 if closing && queued == 0 && workers.free > 0 => {
                gone += 1;
                workers.went();
            }
            // The caller takes a result.
            4 if done > 0 => {
                done -= 1;
            }
            // The lane closes: the tasks that wait are dropped, and nothing more is given.
            5 => {
                closing = true;
                queued = 0;
            }
            // A worker that finds nothing waits, and the caller waits for a result: no count changes.
            _ => {}
        }
        peak = peak.max(queued + busy);
        assert!(workers.free + busy + gone == workers.started);
        assert!(workers.started <= most && workers.started <= peak);
    }
    kani::cover!((workers.started == most) & (most == 4), "every worker allowed started");
    kani::cover!((gone == workers.started) & (gone > 0), "a lane closed whose workers all went");
}
