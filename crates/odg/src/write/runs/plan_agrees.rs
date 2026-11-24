//! The plan that makes a file's paragraphs the model's (`runs::plan`) finds the paragraphs it keeps in the middle by their characters. It did so with
//! a map of lists of places, one list (an allocation) for each paragraph of the file: at the 2,000,000 paragraphs the reader allows in a shape, an adversarial order took
//! seconds where a plain pass over the lists takes a fraction of one. The map is now one flat sorted list of (hash of the characters, place), and **the plan is the same**:
//! this module keeps the plan as the map made it ([`plan_with_map`]) and holds the two to the same steps and the same result over every pair of short lists and over random
//! lists, and times them side by side by hand.

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, DefaultHasher, Hasher};
use std::sync::Arc;

use super::{MOST_BUCKET_BITS, Props, Run, Step, View, bucket_bits, plan, plan_counted, plan_with};

/// The plan as it was before a change: the same walk, with a map of the file's paragraphs by their characters, each with the places that have them, in order.
fn plan_with_map(old: &[View], new: &[View], steps: &mut u64) -> Vec<Step> {
    *steps = steps.saturating_add((old.len() + new.len()) as u64);
    let mut out: Vec<Step> = Vec::with_capacity(old.len().max(new.len()));
    if old.len() == new.len() {
        for (k, (a, b)) in old.iter().zip(new).enumerate() {
            out.push(if a.same(b) { Step::Keep { old: k, new: k } } else { Step::Edit { old: k, new: k } });
        }
        return out;
    }
    let head = old.iter().zip(new).take_while(|(a, b)| a.same(b)).count();
    let tail = old[head..].iter().rev().zip(new[head..].iter().rev()).take_while(|(a, b)| a.same(b)).count();
    for k in 0..head {
        out.push(Step::Keep { old: k, new: k });
    }
    let (old_mid, new_mid) = (head..old.len() - tail, head..new.len() - tail);
    let mut by_chars: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for k in old_mid.clone() {
        by_chars.entry(old[k].chars.as_str()).or_default().push_back(k);
    }
    let mut matched: Vec<Option<usize>> = vec![None; new_mid.len()];
    let mut floor = old_mid.start;
    for (slot, j) in new_mid.clone().enumerate() {
        *steps = steps.saturating_add(1);
        let Some(places) = by_chars.get_mut(new[j].chars.as_str()) else { continue };
        while places.front().is_some_and(|k| *k < floor) {
            places.pop_front();
        }
        if let Some(&k) = places.front()
            && old[k].same(&new[j])
        {
            places.pop_front();
            matched[slot] = Some(k);
            floor = k;
        }
    }
    fn flush(out: &mut Vec<Step>, old_from: usize, until_old: usize, new_from: usize, new_to: usize) {
        let (mut o, mut n) = (old_from, new_from);
        while o < until_old && n < new_to {
            out.push(Step::Edit { old: o, new: n });
            o += 1;
            n += 1;
        }
        while o < until_old {
            out.push(Step::Remove { old: o });
            o += 1;
        }
        while n < new_to {
            out.push(Step::Insert { new: n });
            n += 1;
        }
    }
    let mut next_old = old_mid.start;
    let mut gap_from = new_mid.start;
    for (slot, j) in new_mid.clone().enumerate() {
        if let Some(k) = matched[slot] {
            flush(&mut out, next_old, k, gap_from, j);
            out.push(Step::Keep { old: k, new: j });
            next_old = k + 1;
            gap_from = j + 1;
        }
    }
    flush(&mut out, next_old, old_mid.end, gap_from, new_mid.end);
    for k in 0..tail {
        out.push(Step::Keep { old: old.len() - tail + k, new: new.len() - tail + k });
    }
    out
}

/// A paragraph of `chars` whose paragraph row is the style `pp` (`""` for none): two paragraphs of the same characters are the same only if their rows are.
fn view(chars: &str, pp: &str) -> View {
    let props = |key: &str| Arc::new(Props { key: key.to_owned(), ops: Vec::new(), faces: Vec::new(), bullet: false, list: None, parts: Vec::new() });
    View { chars: chars.to_owned(), pp: (!pp.is_empty()).then(|| props(pp)), runs: Vec::new(), empty: None, fields: Vec::new() }
}

/// A paragraph that has one run (so that two paragraphs of the same characters and row may still differ in the run).
fn with_run(mut v: View, key: &str) -> View {
    let props = Arc::new(Props { key: key.to_owned(), ops: Vec::new(), faces: Vec::new(), bullet: false, list: None, parts: Vec::new() });
    v.runs.push(Run { start: 0, end: v.chars.chars().count(), props: Some(props) });
    v
}

/// A hash that puts every text under one value: every text collides with every other, and the plan must tell them apart by their characters.
#[derive(Default)]
struct Constant;

impl Hasher for Constant {
    fn finish(&self) -> u64 {
        0x5A5A_5A5A_5A5A_5A5A
    }

    fn write(&mut self, _: &[u8]) {}
}

/// A hash that is the first byte of the text, in the top bits: the texts that start alike collide, and the others fall in a few buckets.
#[derive(Default)]
struct FirstByte {
    top: u64,
    seen: bool,
}

impl Hasher for FirstByte {
    fn finish(&self) -> u64 {
        self.top << 56
    }

    fn write(&mut self, bytes: &[u8]) {
        if !self.seen {
            self.seen = true;
            self.top = bytes.first().copied().map_or(0, u64::from);
        }
    }
}

/// The plan both ways, with the steps each counted: they must be the same, and so must the plan that the hashes above make (a hash that collides for every text, and one that
/// collides for the texts that start alike) and the one a fixed-key hash makes.
fn both(old: &[View], new: &[View]) -> (Vec<Step>, u64) {
    let (mut a, mut b) = (0u64, 0u64);
    let (made, was) = (plan(old, new, &mut a), plan_with_map(old, new, &mut b));
    assert_eq!(made, was, "the plan differs from the map's");
    assert_eq!(a, b, "the steps counted differ");
    let mut steps = 0u64;
    assert_eq!(plan_with(&BuildHasherDefault::<Constant>::default(), old, new, &mut steps), was, "a hash that collides for every text");
    assert_eq!(steps, b);
    assert_eq!(plan_with(&BuildHasherDefault::<FirstByte>::default(), old, new, &mut steps), was, "a hash that collides for texts that start alike");
    assert_eq!(plan_with(&BuildHasherDefault::<DefaultHasher>::default(), old, new, &mut steps), was, "a hash of fixed keys");
    (made, a)
}

/// The bits the buckets of a list of places are made from: the number of bits that write the count, from one to [`MOST_BUCKET_BITS`].
#[test]
fn the_buckets_of_the_list_are_about_a_bucket_for_each_place_and_never_more_than_the_most() {
    for (places, bits) in [
        (0usize, 1u32),
        (1, 1),
        (2, 2),
        (3, 2),
        (4, 3),
        (1_000, 10),
        (1_023, 10),
        (1_024, 11),
        (2_000_000, 21),
        (4_194_303, 22),
        (4_194_304, 22),
        (usize::MAX, 22),
    ] {
        assert_eq!(bucket_bits(places), bits, "{places} places");
    }
    assert_eq!(MOST_BUCKET_BITS, 22);
}

/// Every pair of lists of up to three paragraphs of six kinds (the text `a` plain, with a paragraph row, and with a run; `b` plain and with a paragraph row; and an
/// empty one): 259 lists, and so 67,081 pairs.
#[test]
fn the_flat_plan_is_the_map_s_plan_for_every_pair_of_short_lists() {
    let kinds: Vec<View> = vec![view("a", ""), view("a", "x"), with_run(view("a", ""), "r"), view("b", ""), view("b", "x"), view("", "")];
    let mut lists: Vec<Vec<usize>> = vec![Vec::new()];
    let mut from = 0;
    for _ in 0..3 {
        let to = lists.len();
        for at in from..to {
            for kind in 0..kinds.len() {
                let mut longer = lists[at].clone();
                longer.push(kind);
                lists.push(longer);
            }
        }
        from = to;
    }
    assert_eq!(lists.len(), 1 + 6 + 36 + 216);
    let made = |list: &[usize]| list.iter().map(|k| kinds[*k].clone()).collect::<Vec<View>>();
    let (mut pairs, mut kept) = (0usize, 0usize);
    for old in &lists {
        let old = made(old);
        for new in &lists {
            let (plan, _) = both(&old, &made(new));
            pairs += 1;
            kept += plan.iter().filter(|s| matches!(s, Step::Keep { .. })).count();
        }
    }
    assert_eq!(pairs, 67_081);
    assert!(kept > 20_000, "the check met many kept paragraphs: {kept}");
}

/// A deterministic stream of numbers.
struct Stream(u64);

impl Stream {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % below.max(1) as u64).unwrap_or(0)
    }
}

/// Random lists: the model's paragraphs are the file's, shuffled in part, with some taken out, some added, some retyped and some with another row, over a few texts so that
/// many repeat (the places of one text then number many, which is what the lists of places held).
#[test]
fn the_flat_plan_is_the_map_s_plan_for_random_lists() {
    let mut stream = Stream(0x9E37_79B9_7F4A_7C15);
    let mut kept = 0usize;
    for round in 0..30_000 {
        let alphabet = 1 + stream.next(8);
        let long = 1 + stream.next(if round % 50 == 0 { 400 } else { 30 });
        let paragraph = |stream: &mut Stream| {
            let text = format!("t{}", stream.next(alphabet));
            let v = view(&text, if stream.next(4) == 0 { "x" } else { "" });
            if stream.next(5) == 0 { with_run(v, "r") } else { v }
        };
        let old: Vec<View> = (0..stream.next(long + 1)).map(|_| paragraph(&mut stream)).collect();
        let mut new: Vec<View> = old.clone();
        // A few moves, takings-out, insertions and retypings.
        for _ in 0..stream.next(6) {
            match stream.next(4) {
                0 if !new.is_empty() => {
                    let (a, b) = (stream.next(new.len()), stream.next(new.len()));
                    new.swap(a, b);
                }
                1 if !new.is_empty() => {
                    let at = stream.next(new.len());
                    new.remove(at);
                }
                2 => {
                    let at = stream.next(new.len() + 1);
                    let made = paragraph(&mut stream);
                    new.insert(at, made);
                }
                _ if !new.is_empty() => {
                    let at = stream.next(new.len());
                    new[at] = paragraph(&mut stream);
                }
                _ => {}
            }
        }
        // Some rounds are a list shuffled whole, or an unrelated one.
        match round % 7 {
            0 => (0..new.len()).rev().for_each(|i| new.swap(i, stream.next(i + 1))),
            1 => new = (0..stream.next(long + 1)).map(|_| paragraph(&mut stream)).collect(),
            _ => {}
        }
        let (plan, _) = both(&old, &new);
        kept += plan.iter().filter(|s| matches!(s, Step::Keep { .. })).count();
    }
    assert!(kept > 50_000, "the check met many kept paragraphs: {kept}");
}

/// A texts of 17 characters, distinct for each `k`.
fn text_of(k: usize) -> String {
    format!("paragraph {k:07}")
}

/// The entries of the sorted list that the looks of a plan went over, for the lists `old` and `new`, by the hash `hasher` (a hash of fixed keys unless it is the point), and
/// the looks the plan made (a step for each paragraph of the middle of `new`, which is all of `new` when no paragraph is alike at the head or the tail).
fn gone_over<S: std::hash::BuildHasher>(hasher: &S, old: &[View], new: &[View]) -> (u64, u64) {
    let (mut steps, mut looked) = (0u64, 0u64);
    plan_counted(hasher, old, new, &mut steps, &mut looked);
    (looked, steps - (old.len() + new.len()) as u64)
}

/// The review, LOW: the plan's looks go over a few entries of the sorted list each and not every later one. A look stops at the first entry of
/// another hash (`if held != hash { break None }`); without that stop it went on to the end of the list, so that a list of the file's paragraphs 40,000 long took 2.9 s in a
/// debug build, with the same plan and the same steps, a quadratic that only a count of the entries can see. At 20,000 paragraphs, for the three shapes of input that make
/// the looks miss, hit and pass a taken place: a look goes over **two entries at the most** (the entry it stops at, and the place already taken), and the looks together
/// over about one for each (not fewer than nine in ten: a count that does not count is no count either). The mutant counted about 200,000,000.
#[test]
fn the_looks_in_the_list_of_places_go_over_two_entries_at_the_most() {
    let n = 20_000usize;
    let fixed = BuildHasherDefault::<DefaultHasher>::default();
    let plain = |k: usize| view(&format!("old {k}"), "");
    // (a) Every look misses: no text of the model is in the file.
    let old: Vec<View> = (0..n).map(plain).collect();
    let new: Vec<View> = (0..=n).map(|k| view(&format!("new {k}"), "")).collect();
    // (b) The model's paragraphs are the file's in the reverse order, and one more: each look finds a place before the floor it must keep to.
    let reversed: Vec<View> = (0..=n).map(|k| if k == n { view("one more", "") } else { plain(n - 1 - k) }).collect();
    // (c) One text again and again, from the second place of the file: each look passes the place the look before took (and the file's first and last are others).
    let mut repeating: Vec<View> = vec![view("first", "")];
    repeating.extend((0..n).map(|_| view("x", "")));
    repeating.push(view("last", ""));
    let mut repeated: Vec<View> = vec![view("other first", "")];
    repeated.extend((0..n).map(|_| view("x", "")));
    repeated.extend([view("other last", ""), view("one more", "")]);
    for (what, old, new) in [("misses", &old, &new), ("reversed", &old, &reversed), ("one text repeated", &repeating, &repeated)] {
        let (looked, looks) = gone_over(&fixed, old, new);
        eprintln!("PLAN-LOOKED {what}: {looked} entries gone over by {looks} looks");
        assert!(looks >= n as u64, "{what}: the plan made {looks} looks");
        assert!(
            looked <= 2 * looks,
            "{what}: {looked} entries gone over by {looks} looks: more than two for a look, a look does not stop where its hash ends"
        );
        assert!(looked * 10 >= looks * 9, "{what}: {looked} entries gone over by {looks} looks: fewer than one in a look, the count does not count");
    }
}

/// The count sees a walk: with a hash that puts every text under one value (a collision for all of them, which a file cannot make against a hash keyed at random) the looks pass
/// the entries before the text's own, and 300 texts that are none of the file's go over every entry of the 300 each. This is what the count above is **for**.
#[test]
fn the_count_of_entries_gone_over_counts_the_places_a_look_passes() {
    let n = 300usize;
    let old: Vec<View> = (0..n).map(|k| view(&format!("old {k}"), "")).collect();
    let new: Vec<View> = (0..=n).map(|k| view(&format!("new {k}"), "")).collect();
    let (looked, looks) = gone_over(&BuildHasherDefault::<Constant>::default(), &old, &new);
    assert_eq!(looks, n as u64 + 1);
    assert_eq!(looked, (n * (n + 1)) as u64, "each of the {looks} looks goes over all {n} entries, which are all of its hash");
}

/// The two plans timed beside a plain pass over the same lists, by hand: the file's paragraphs distinct and in order, the model's reversed (so that the map of the file's
/// paragraphs is made whole and almost no paragraph is kept) and one more; a shuffle; and one text repeated. At 100,000, 1,000,000 and 2,000,000 (the most a shape may hold).
/// `cargo nextest run -p diacraft-odg --run-ignored only -E 'test(plans_timed)' --no-capture`.
#[test]
#[ignore = "timing, by hand: 100,000 to 2,000,000 paragraphs, the plan with the map and the flat one beside a plain pass"]
fn plans_timed_beside_a_plain_pass() {
    for n in [100_000usize, 1_000_000, 2_000_000] {
        for shape in ["reversed", "shuffled", "repeated"] {
            let mut stream = Stream(0x2545_F491_4F6C_DD1D);
            let old: Vec<View> = (0..n).map(|k| view(&if shape == "repeated" { "x".to_owned() } else { text_of(k) }, "")).collect();
            let mut new: Vec<View> = match shape {
                "reversed" => (0..n).rev().map(|k| view(&text_of(k), "")).collect(),
                "shuffled" => {
                    let mut ids: Vec<usize> = (0..n).collect();
                    for i in (1..n).rev() {
                        ids.swap(i, stream.next(i + 1));
                    }
                    ids.iter().map(|k| view(&text_of(*k), "")).collect()
                }
                _ => (0..n).map(|_| view("x", "")).collect(),
            };
            new.push(view("one more", ""));
            let time = |what: &dyn Fn() -> usize| {
                let started = std::time::Instant::now();
                let kept = what();
                (started.elapsed().as_secs_f64(), kept)
            };
            let (plain, _) = time(&|| old.iter().zip(&new).filter(|(a, b)| a.same(b)).count());
            let (flat, kept_flat) = time(&|| plan(&old, &new, &mut 0).iter().filter(|s| matches!(s, Step::Keep { .. })).count());
            let (map, kept_map) = time(&|| plan_with_map(&old, &new, &mut 0).iter().filter(|s| matches!(s, Step::Keep { .. })).count());
            assert_eq!(kept_flat, kept_map);
            eprintln!(
                "PLAN-TIME n={n} {shape}: plain pass {plain:.3} s, the plan with the map {map:.3} s ({:.1}x), the flat plan {flat:.3} s ({:.1}x), {kept_flat} kept",
                map / plain.max(1e-9),
                flat / plain.max(1e-9)
            );
        }
    }
}
