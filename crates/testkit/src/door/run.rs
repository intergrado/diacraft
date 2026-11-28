//! The sweep: every case on a thread of its own with a wall-clock budget, the count of what got past the reader, and the dump of what failed.
//!
//! A [`Door`] is a format's seeds and the pipeline a mutant goes through. [`sweep`] makes `cases` mutants (a seed and a class for each, in a fixed order, from a fixed stream of random
//! numbers, so the same sweep is the same files on every run) and runs each in a thread of 2 MiB (the stack of a test thread, the one the engine's recursion limits were checked on)
//! under a budget. A case fails if it panics, if the pipeline says it is a bug ([`Outcome::Failed`]) or if it does not end in its budget; a sweep fails if its mutants never got past the
//! reader (so that a sweep that only exercises the parser shows up).
//!
//! **The budget** is for a hang and is no measure of speed: twenty seconds, and two hundred times what the unmutated seeds take on this machine now for each 64 KiB of the
//! mutant (so a loaded machine is given more, a quick one is not asked to be fast, and a file that is large is given the time a large file takes), never more than eighty seconds,
//! which is under the 120 seconds nextest allows a test. A case that passes it ends the sweep (its thread
//! cannot be stopped and would go on using the machine), and so does a run of an unmutated seed that does not end in the eighty seconds it is given: the report is failed at once. The work a reader does is held to its own counted budgets, as before: the sweeps run each reader with its limits as they ship.
//!
//! **Environment.** `DIACRAFT_DOOR_CASES=n` makes n cases for each format (the default is a few hundred); `DIACRAFT_DOOR_SEED=n` changes the stream, to look further than the
//! committed sweep does; `DIACRAFT_DOOR_BIG=1` adds the heavy sizes (nests of 100,000 levels, chains of 20,000 items, floods of 60,000 attributes, bombs of 16 MiB) to the ones a sweep picks from; `DIACRAFT_DOOR_CASE=i` runs case `i` alone (with `DIACRAFT_DOOR_DUMP` its file is kept whatever happens to it, and a step it was refused is given in full; to replay a failure that a sweep named); `DIACRAFT_DOOR_BUDGET=secs` gives each case that many seconds (to let a case that was called a hang run to its end); `DIACRAFT_DOOR_DUMP=name` writes each distinct failure's file and a
//! note to `$TMPDIR/diacraft-door/name` (with `1` for the folder itself; never under `/tmp`, which is memory on some machines).

// `assert_clean` fails the test with the failures' words, as an assertion does; the shipped-code ban on panicking (AGENTS.md, Never crash) is for code that runs for users.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::mutate::{Class, Mutant, mutate};
use super::pipeline::{Outcome, words_key};
use super::rng::Rng;
use super::seeds::Seed;

/// The stack a case runs on: a test thread's.
const CASE_STACK: usize = 2 << 20;
/// The least a case is given, whatever the machine.
const BUDGET_FLOOR: Duration = Duration::from_secs(20);
/// The most a case is given: under nextest's 120 seconds for a whole test.
const BUDGET_CEILING: Duration = Duration::from_secs(80);
/// How many times what the unmutated seeds take a case is given, on top of the floor, for each `BUDGET_UNIT` bytes of the mutant.
const BUDGET_FACTOR: u32 = 200;
/// The size of a mutant that the yardstick stands for: a seed is a few kilobytes, and the time of a case is mostly what it costs to open, draw and save at all.
const BUDGET_UNIT: usize = 64 << 10;
/// The stream a sweep uses unless `DIACRAFT_DOOR_SEED` says another.
const DEFAULT_SEED: u64 = 0x00D0_0DCA_FE26_1126;

/// What a mutant is put through: the file's bytes, the extension the file would have, and the number of the case.
pub type Pipeline = Arc<dyn Fn(&[u8], &str, usize) -> Outcome + Send + Sync>;

/// A format's seeds and the pipeline its mutants go through.
pub struct Door {
    pub name: &'static str,
    pub seeds: Vec<Seed>,
    pub pipeline: Pipeline,
}

/// What a sweep is asked to do.
#[derive(Clone, Debug)]
pub struct Config {
    pub cases: usize,
    pub seed: u64,
    pub only: Option<usize>,
    /// The heavy sizes as well (`DIACRAFT_DOOR_BIG=1`): nests, chains, floods, copies and bombs of the sizes that take seconds to read in a sound reader.
    pub big: bool,
    pub dump: Option<PathBuf>,
    /// A budget for a case in seconds in place of the one the machine's speed gives (`DIACRAFT_DOOR_BUDGET`): to let a case that was called a hang run to its end, with its times.
    pub budget_secs: Option<u64>,
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// The folder a dump goes to: `$TMPDIR/diacraft-door[/name]`, where `name` is plain letters, digits, dots, dashes and underscores. `None` when the machine's temporary files are `/tmp`.
fn dump_folder(asked: &str) -> Option<PathBuf> {
    let base = std::env::var_os("TMPDIR").map(PathBuf::from)?;
    if base.starts_with("/tmp") {
        eprintln!("DIACRAFT_DOOR_DUMP: TMPDIR is under /tmp, which is memory on some machines: set TMPDIR to a folder on a disk");
        return None;
    }
    let mut folder = base.join("diacraft-door");
    if asked != "1" {
        if asked.is_empty() || asked.starts_with('.') || !asked.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            eprintln!("DIACRAFT_DOOR_DUMP: `{asked}` is no folder name (letters, digits, dots, dashes and underscores)");
            return None;
        }
        folder = folder.join(asked);
    }
    Some(folder)
}

/// The folder a dump goes to when `DIACRAFT_DOOR_DUMP` asks for one.
pub fn dump_folder_from_env() -> Option<PathBuf> {
    std::env::var("DIACRAFT_DOOR_DUMP").ok().and_then(|asked| dump_folder(&asked))
}

impl Config {
    /// The sweep the environment asks for, with `default_cases` cases for each format when it says nothing.
    pub fn from_env(default_cases: usize) -> Config {
        Config {
            cases: env_number("DIACRAFT_DOOR_CASES").and_then(|n| usize::try_from(n).ok()).unwrap_or(default_cases),
            seed: env_number("DIACRAFT_DOOR_SEED").unwrap_or(DEFAULT_SEED),
            only: env_number("DIACRAFT_DOOR_CASE").and_then(|n| usize::try_from(n).ok()),
            big: env_number("DIACRAFT_DOOR_BIG").is_some_and(|n| n != 0),
            dump: dump_folder_from_env(),
            budget_secs: env_number("DIACRAFT_DOOR_BUDGET"),
        }
    }
}

/// How many mutants of a class there were and what became of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassCount {
    pub cases: usize,
    pub passed: usize,
    pub refused: usize,
}

/// A case that failed.
#[derive(Clone, Debug)]
pub struct Failure {
    pub case: usize,
    pub seed: &'static str,
    pub class: Class,
    pub detail: String,
    pub what: String,
    pub bytes: Vec<u8>,
}

impl Failure {
    /// What two failures have in common if they are one bug: the class of trouble, in the words with their numbers taken out.
    pub fn key(&self) -> String {
        words_key(&self.what)
    }
}

/// What a sweep did.
#[derive(Debug)]
pub struct Report {
    pub name: &'static str,
    pub config: Config,
    pub made: usize,
    pub skipped: usize,
    pub passed: usize,
    pub refused: usize,
    pub by_class: BTreeMap<&'static str, ClassCount>,
    pub refusals: BTreeMap<String, usize>,
    /// What a step of a case that got through was refused for: how many times, the first case and the words of that one whole.
    pub step_refusals: BTreeMap<String, (usize, usize, String)>,
    /// The time the steps of the cases that got through took, added up by the step's name.
    pub step_time: BTreeMap<String, Duration>,
    /// The slowest cases: how long, the case, the class and what was done.
    pub slowest: Vec<(Duration, usize, &'static str, String)>,
    pub failures: Vec<Failure>,
    pub yardstick: Duration,
    pub budget: Duration,
    pub elapsed: Duration,
    pub timed_out: bool,
}

impl Report {
    /// How many mutants got past the reader.
    pub fn door(&self) -> usize {
        self.passed
    }

    /// The report as lines of text: the counts, the classes and the most common refusals.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "door sweep `{}`: {} cases ({} skipped), {} past the reader, {} refused, {} failed; yardstick {:?}, budget {:?}, {:?} in all\n",
            self.name,
            self.made,
            self.skipped,
            self.passed,
            self.refused,
            self.failures.len(),
            self.yardstick,
            self.budget,
            self.elapsed
        );
        for (class, c) in &self.by_class {
            out.push_str(&format!("  {class:<18} {:>4} cases  {:>4} past the reader  {:>4} refused\n", c.cases, c.passed, c.refused));
        }
        let mut top: Vec<(&String, &usize)> = self.refusals.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (words, n) in top.into_iter().take(8) {
            out.push_str(&format!("  refused {n:>4} x {words}\n"));
        }
        let mut times: Vec<(&String, &Duration)> = self.step_time.iter().collect();
        times.sort_by(|a, b| b.1.cmp(a.1));
        let spent: Vec<String> = times.into_iter().take(6).map(|(label, t)| format!("{label} {:.1}s", t.as_secs_f64())).collect();
        if !spent.is_empty() {
            out.push_str(&format!("  where the time of the cases that got through went: {}\n", spent.join(", ")));
        }
        for (took, case, class, detail) in &self.slowest {
            out.push_str(&format!("  slow: case {case} {:.2}s ({class}) {detail}\n", took.as_secs_f64()));
        }
        let mut steps: Vec<(&String, &(usize, usize, String))> = self.step_refusals.iter().collect();
        steps.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        for (words, (n, first, whole)) in steps.into_iter().take(8) {
            out.push_str(&format!("  a step of a case that got through was refused {n:>4} x (first: case {first}) {words}\n"));
            if self.config.only.is_some() {
                out.push_str(&format!("    in full: {whole}\n"));
            }
        }
        out
    }

    /// Fails unless no case failed and none timed out; each distinct failure is named with the case that replays it.
    #[track_caller]
    pub fn assert_clean(&self) {
        if self.failures.is_empty() {
            return;
        }
        let mut distinct: BTreeMap<String, Vec<&Failure>> = BTreeMap::new();
        for f in &self.failures {
            distinct.entry(f.key()).or_default().push(f);
        }
        let mut text = format!("door sweep `{}`: {} failure(s), {} distinct:\n", self.name, self.failures.len(), distinct.len());
        for (key, all) in &distinct {
            let Some(first) = all.first() else { continue };
            text.push_str(&format!(
                "- {} x {key}\n    first: case {} (seed `{}`, class {}): {}\n    what was done: {}\n    replay: DIACRAFT_DOOR_CASE={} DIACRAFT_DOOR_SEED={} (and DIACRAFT_DOOR_DUMP=1 to keep the file)\n",
                all.len(),
                first.case,
                first.seed,
                first.class.name(),
                first.what,
                first.detail,
                first.case,
                self.config.seed
            ));
        }
        text.push_str(&self.summary());
        panic!("{text}");
    }

    /// Fails unless every class that fits one of `seeds` made a mutant: a class that never ran is a place the sweep did not look.
    #[track_caller]
    pub fn assert_classes(&self, seeds: &[Seed]) {
        if self.config.only.is_some() || self.config.cases < Class::ALL.len() * seeds.len().max(1) {
            return;
        }
        let missing: Vec<&str> = Class::ALL
            .into_iter()
            .filter(|c| seeds.iter().any(|s| c.applies(s)))
            .map(Class::name)
            .filter(|n| self.by_class.get(n).is_none_or(|c| c.cases == 0))
            .collect();
        assert!(missing.is_empty(), "door sweep `{}`: no mutant of the class(es) {missing:?} was made:\n{}", self.name, self.summary());
    }

    /// Fails unless at least `min_percent` of the mutants got past the reader and at least `min_classes` classes got one past: a sweep whose mutants never pass the reader
    /// exercises the reader and nothing after it.
    #[track_caller]
    pub fn assert_door(&self, min_percent: usize, min_classes: usize) {
        if self.config.only.is_some() {
            return;
        }
        let classes = self.by_class.values().filter(|c| c.passed > 0).count();
        let share = self.passed * 100 / self.made.max(1);
        assert!(
            share >= min_percent && classes >= min_classes,
            "door sweep `{}`: {} of {} mutants ({share}%) got past the reader and {classes} class(es) had one past it; at least {min_percent}% and {min_classes} class(es) are asked, \
             or the sweep is exercising the reader and nothing after it:\n{}",
            self.name,
            self.passed,
            self.made,
            self.summary()
        );
    }
}

/// Why a case did not give an outcome.
enum Trouble {
    Panic(String),
    Timeout,
}

/// What a case of `len` bytes is given, with `yardstick` the time the unmutated seeds take: the floor, and `BUDGET_FACTOR` yardsticks for each `BUDGET_UNIT` bytes, never past the
/// ceiling.
fn budget_for(yardstick: Duration, len: usize) -> Duration {
    let units = u32::try_from(len / BUDGET_UNIT + 1).unwrap_or(u32::MAX);
    BUDGET_FLOOR.saturating_add(yardstick.saturating_mul(BUDGET_FACTOR).saturating_mul(units)).min(BUDGET_CEILING)
}

fn run_one(pipeline: &Pipeline, extension: &str, bytes: Vec<u8>, case: usize, budget: Duration) -> Result<Outcome, Trouble> {
    let (tx, rx) = mpsc::channel();
    let pipeline = Arc::clone(pipeline);
    let extension = extension.to_owned();
    let spawned = std::thread::Builder::new().name(format!("door-case-{case}")).stack_size(CASE_STACK).spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(|| pipeline(&bytes, &extension, case)));
        // The receiver is gone only after a timeout, which has ended the sweep already.
        let _ = tx.send(result.map_err(|payload| diacraft_engine::guard::panic_message(payload.as_ref())));
    });
    if let Err(e) = spawned {
        return Err(Trouble::Panic(format!("the sweep could not start a thread: {e}")));
    }
    match rx.recv_timeout(budget) {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(message)) => Err(Trouble::Panic(message)),
        Err(_) => Err(Trouble::Timeout),
    }
}

/// The seed and class a case is made from, and the mutant, or `None` when no class of the seed's has anything to change. The order is fixed: the seeds in turn, and for each round
/// the next class of those that fit the seed.
fn make_case(door: &Door, config: &Config, case: usize) -> Option<(usize, Mutant)> {
    let count = door.seeds.len();
    let seed_index = case % count.max(1);
    let seed = door.seeds.get(seed_index)?;
    let fitting: Vec<Class> = Class::ALL.into_iter().filter(|c| c.applies(seed)).collect();
    let start = (case / count.max(1) + seed_index) % fitting.len().max(1);
    let mut rng = Rng::for_case(config.seed, door.name, case).big(config.big);
    (0..fitting.len()).find_map(|k| mutate(seed, *fitting.get((start + k) % fitting.len())?, &mut rng)).map(|m| (seed_index, m))
}

/// What is wrong with a run of an unmutated seed, in words, or `None` when it did all there is to do.
fn baseline_fault(result: &Result<Outcome, Trouble>) -> Option<String> {
    match result {
        Ok(Outcome::Passed(p)) if p.drawn && p.saved > 0 && p.saved == p.reopened => None,
        Ok(Outcome::Passed(p)) => Some(format!("the unmutated seed got through but did not do it all: {p:?}")),
        Ok(Outcome::Refused(words)) => Some(format!("the unmutated seed was refused: {words}")),
        Ok(Outcome::Failed(what)) => Some(format!("the unmutated seed failed: {what}")),
        Err(Trouble::Panic(message)) => Some(format!("the unmutated seed panicked: {message}")),
        Err(Trouble::Timeout) => Some("the unmutated seed did not end".to_owned()),
    }
}

/// Puts each unmutated seed through the pipeline, twice (the second time is the yardstick), each run given `budget`. Every way a run of a seed fails is a failure (the same words on the
/// second run as on the first are not another). Gives the time the slowest second run took, and whether a run did not end: that ends the control at once, as the thread of a run that does not
/// end cannot be stopped and each run started after it could hang for the whole budget again.
fn control(door: &Door, failures: &mut Vec<Failure>, budget: Duration) -> (Duration, bool) {
    let mut yardstick = Duration::ZERO;
    for round in 0..2 {
        for seed in &door.seeds {
            let started = Instant::now();
            let result = run_one(&door.pipeline, seed.extension, seed.bytes.clone(), usize::MAX - round, budget);
            if round == 1 {
                yardstick = yardstick.max(started.elapsed());
            }
            let Some(bad) = baseline_fault(&result) else { continue };
            if !failures.iter().any(|f| f.case == usize::MAX && f.seed == seed.name && f.what == bad) {
                failures.push(Failure {
                    case: usize::MAX,
                    seed: seed.name,
                    class: Class::BitFlips,
                    detail: "no mutation".to_owned(),
                    what: bad,
                    bytes: seed.bytes.clone(),
                });
            }
            if matches!(result, Err(Trouble::Timeout)) {
                return (yardstick, true);
            }
        }
    }
    (yardstick, false)
}

fn dump(config: &Config, name: &str, failure: &Failure, dumped: &mut std::collections::BTreeSet<String>) {
    let Some(folder) = &config.dump else { return };
    if !dumped.insert(failure.key()) {
        return;
    }
    let stem = format!("{name}-case{}-{}", failure.case, failure.class.name());
    let note = format!(
        "format: {name}\ncase: {}\nseed: {}\nstream: {}\nclass: {}\nwhat was done: {}\nwhat happened: {}\nreplay: DIACRAFT_DOOR_CASE={} DIACRAFT_DOOR_SEED={}\n",
        failure.case,
        failure.seed,
        config.seed,
        failure.class.name(),
        failure.detail,
        failure.what,
        failure.case,
        config.seed
    );
    let written = std::fs::create_dir_all(folder)
        .and_then(|()| std::fs::write(folder.join(format!("{stem}.bin")), &failure.bytes))
        .and_then(|()| std::fs::write(folder.join(format!("{stem}.txt")), note));
    match written {
        Ok(()) => eprintln!("DIACRAFT_DOOR_DUMP: wrote {}", folder.join(&stem).display()),
        Err(e) => eprintln!("DIACRAFT_DOOR_DUMP: could not write {}: {e}", folder.display()),
    }
}

/// Writes the seeds to the dump folder, once (a file that is there stays), so that a mutant can be set beside what it was made from.
fn dump_seeds(config: &Config, door: &Door) {
    let Some(folder) = &config.dump else { return };
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    for seed in &door.seeds {
        let file = folder.join(format!("seed-{}.{}", seed.name, seed.extension));
        if !file.exists() {
            // A seed that cannot be written is no failure of the sweep: the dump is a convenience.
            let _ = std::fs::write(&file, &seed.bytes);
        }
    }
}

impl Report {
    fn new(door: &Door, config: &Config) -> Report {
        Report {
            name: door.name,
            config: config.clone(),
            made: 0,
            skipped: 0,
            passed: 0,
            refused: 0,
            by_class: BTreeMap::new(),
            refusals: BTreeMap::new(),
            step_refusals: BTreeMap::new(),
            step_time: BTreeMap::new(),
            slowest: Vec::new(),
            failures: Vec::new(),
            yardstick: Duration::ZERO,
            budget: BUDGET_FLOOR,
            elapsed: Duration::ZERO,
            timed_out: false,
        }
    }

    /// Notes how long a case took, keeping the five slowest.
    fn note_time(&mut self, took: Duration, case: usize, class: Class, detail: &str) {
        self.slowest.push((took, case, class.name(), detail.chars().take(100).collect()));
        self.slowest.sort_by_key(|slow| std::cmp::Reverse(slow.0));
        self.slowest.truncate(5);
    }

    /// Counts what became of a case. `Some(words)` when it failed.
    fn record(&mut self, class: Class, case: usize, ran: Result<Outcome, Trouble>, budget: Duration) -> Option<String> {
        let counts = self.by_class.entry(class.name()).or_default();
        match ran {
            Ok(Outcome::Passed(p)) => {
                counts.passed += 1;
                self.passed += 1;
                for (key, whole) in p.refused {
                    self.step_refusals.entry(key).or_insert((0, case, whole)).0 += 1;
                }
                for (label, time) in p.times {
                    *self.step_time.entry(label).or_default() += time;
                }
                None
            }
            Ok(Outcome::Refused(words)) => {
                counts.refused += 1;
                self.refused += 1;
                *self.refusals.entry(words_key(&words)).or_default() += 1;
                None
            }
            Ok(Outcome::Failed(what)) => Some(what),
            Err(Trouble::Panic(message)) => Some(format!("the case panicked: {message}")),
            Err(Trouble::Timeout) => {
                self.timed_out = true;
                Some(format!("the case did not end in {budget:?}"))
            }
        }
    }
}

/// Runs the sweep. The seeds are put through the pipeline first, twice (they must pass both times: a sweep of mutants of a seed that does not work means nothing, so a seed that fails
/// a run, or does not end, fails the sweep and no mutant is made), then `config.cases` mutants.
pub fn sweep(door: &Door, config: &Config) -> Report {
    sweep_with(door, config, BUDGET_CEILING)
}

/// [`sweep`] with the time each run of an unmutated seed is given (a test of the control gives it a short one).
fn sweep_with(door: &Door, config: &Config, control_budget: Duration) -> Report {
    let started = Instant::now();
    let mut report = Report::new(door, config);
    let (yardstick, hung) = control(door, &mut report.failures, control_budget);
    report.yardstick = yardstick;
    report.timed_out = hung;
    dump_seeds(config, door);
    report.budget = budget_for(report.yardstick, 0);
    let mut dumped = std::collections::BTreeSet::new();
    // No mutant is made of a seed that fails its baseline (a mutant of a seed that does not work means nothing), and none when a run did not end (its thread is still using the machine).
    let cases: Vec<usize> = match (report.failures.is_empty(), config.only) {
        (false, _) => Vec::new(),
        (true, Some(only)) => vec![only],
        (true, None) => (0..config.cases).collect(),
    };
    for case in cases {
        let Some((seed_index, mutant)) = make_case(door, config, case) else {
            report.skipped += 1;
            continue;
        };
        let Mutant { class, detail, bytes } = mutant;
        if config.only.is_some() {
            dump(
                config,
                door.name,
                &Failure {
                    case,
                    seed: door.seeds.get(seed_index).map_or("", |s| s.name),
                    class,
                    detail: detail.clone(),
                    what: "run alone".to_owned(),
                    bytes: bytes.clone(),
                },
                &mut dumped,
            );
        }
        report.made += 1;
        report.by_class.entry(class.name()).or_default().cases += 1;
        let (extension, seed_name) = door.seeds.get(seed_index).map_or(("bin", ""), |s| (s.extension, s.name));
        let budget = config.budget_secs.map_or_else(|| budget_for(report.yardstick, bytes.len()), Duration::from_secs);
        let began = Instant::now();
        let ran = run_one(&door.pipeline, extension, bytes.clone(), case, budget);
        report.note_time(began.elapsed(), case, class, &detail);
        let Some(what) = report.record(class, case, ran, budget) else { continue };
        let failure = Failure { case, seed: seed_name, class, detail, what, bytes };
        dump(config, door.name, &failure, &mut dumped);
        report.failures.push(failure);
        if report.timed_out {
            break;
        }
    }
    for failure in report.failures.iter().filter(|f| f.case == usize::MAX) {
        dump(config, door.name, failure, &mut dumped);
    }
    report.elapsed = started.elapsed();
    eprintln!("{}", report.summary());
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::pipeline::Passed;
    use crate::door::xml::Profile;
    use crate::door::zipfile::{Archive, Entry};

    fn door_of(pipeline: impl Fn(&[u8], &str, usize) -> Outcome + Send + Sync + 'static) -> Door {
        let archive = Archive { entries: vec![Entry::new("a.xml", b"<a b=\"1\"/>".to_vec(), true), Entry::new("m", b"x".to_vec(), false)] };
        let seeds = vec![Seed { name: "s", extension: "zip", bytes: archive.honest(), archive: Some(archive), profile: Profile::default() }];
        Door { name: "unit", seeds, pipeline: Arc::new(pipeline) }
    }

    fn passes() -> Outcome {
        Outcome::Passed(Passed { drawn: true, saved: 1, reopened: 1, ..Passed::default() })
    }

    fn config(cases: usize) -> Config {
        Config { cases, seed: 1, only: None, big: false, dump: None, budget_secs: None }
    }

    #[test]
    fn a_sweep_counts_what_gets_past_the_reader_and_what_is_refused_and_does_the_same_twice() {
        let door = door_of(|bytes, _, case| {
            if bytes.len() % 2 == 0 || case >= usize::MAX - 1 {
                passes()
            } else {
                Outcome::Refused(format!("refused for {case} reasons of odd length"))
            }
        });
        let (a, b) = (sweep(&door, &config(60)), sweep(&door, &config(60)));
        assert_eq!(a.made + a.skipped, 60);
        assert_eq!(a.passed + a.refused, a.made);
        assert!(a.passed > 0 && a.refused > 0, "{}", a.summary());
        assert_eq!((a.made, a.passed, a.refused, &a.by_class), (b.made, b.passed, b.refused, &b.by_class), "the same files on every run");
        assert_eq!(a.refusals.len(), 1, "the words are counted with their numbers taken out");
        a.assert_clean();
    }

    #[test]
    #[should_panic(expected = "got past the reader")]
    fn a_sweep_whose_mutants_never_pass_the_reader_fails() {
        let door = door_of(|_, _, case| if case >= usize::MAX - 1 { passes() } else { Outcome::Refused("not a drawing at all".to_owned()) });
        sweep(&door, &config(40)).assert_door(5, 1);
    }

    #[test]
    #[should_panic(expected = "1 distinct")]
    fn a_failure_and_a_panic_fail_the_sweep_and_are_named_once_each_by_their_words() {
        let door = door_of(|_, _, case| match case {
            3 | 9 => Outcome::Failed(format!("a bug at step {case}")),
            usize::MAX | 0 => passes(),
            _ => passes(),
        });
        let report = sweep(&door, &config(12));
        assert_eq!(report.failures.len(), 2);
        report.assert_clean();
    }

    #[test]
    fn a_panic_in_a_case_is_a_failure_with_its_message_and_a_case_that_never_ends_stops_the_sweep() {
        let door = door_of(|_, _, case| if case == 2 { panic!("boom {case}") } else { passes() });
        let report = sweep(&door, &config(6));
        let failure = report.failures.first().unwrap();
        assert!(failure.what.contains("the case panicked: boom 2"), "{}", failure.what);
        assert_eq!(report.failures.len(), 1);
        let hung = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let release = hung.clone();
        let door = door_of(move |_, _, case| {
            if case == 1 {
                while !release.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            passes()
        });
        let started = Instant::now();
        let result = run_one(&door.pipeline, "zip", vec![0], 1, Duration::from_millis(100));
        assert!(matches!(result, Err(Trouble::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(5));
        hung.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn a_seed_that_does_not_pass_the_pipeline_is_a_failure_before_any_mutant_is_made() {
        let door = door_of(|_, _, _| Outcome::Refused("the seed is not even a drawing".to_owned()));
        let report = sweep(&door, &config(4));
        assert!(report.failures.iter().any(|f| f.case == usize::MAX && f.what.contains("the unmutated seed was refused")));
    }

    /// A door of two seeds whose pipeline notes the case of every call. A run of an unmutated seed is the case `usize::MAX` (the first) or one less (the second) and is answered by
    /// `control` with that case; a mutant, any other case, passes.
    fn recording(control: impl Fn(usize) -> Outcome + Send + Sync + 'static) -> (Door, Arc<std::sync::Mutex<Vec<usize>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let mut door = door_of(move |_, _, case| {
            log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(case);
            if case >= usize::MAX - 1 { control(case) } else { passes() }
        });
        let archive = Archive { entries: vec![Entry::new("b.xml", b"<b c=\"2\"/>".to_vec(), true)] };
        door.seeds.push(Seed { name: "t", extension: "zip", bytes: archive.honest(), archive: Some(archive), profile: Profile::default() });
        (door, seen)
    }

    fn cases_seen(seen: &Arc<std::sync::Mutex<Vec<usize>>>) -> Vec<usize> {
        seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// A seed that fails on either of its two runs fails the baseline (the second run's failure was thrown away, so that a sweep of a pipeline that passed once and
    /// failed after it came out clean), and no mutant is made of a baseline that failed.
    #[test]
    fn a_seed_that_fails_either_of_its_two_runs_fails_the_baseline_and_no_mutant_runs() {
        let not_all = || Outcome::Passed(Passed { drawn: true, saved: 1, reopened: 0, ..Passed::default() });
        type Control = Box<dyn Fn(usize) -> Outcome + Send + Sync>;
        let runs: Vec<(&str, Control)> = vec![
            ("the unmutated seed failed: second", Box::new(|case| if case == usize::MAX { passes() } else { Outcome::Failed("second".to_owned()) })),
            ("the unmutated seed panicked: second boom", Box::new(|case| if case == usize::MAX { passes() } else { panic!("second boom") })),
            ("the unmutated seed got through but did not do it all", Box::new(move |case| if case == usize::MAX { passes() } else { not_all() })),
            (
                "the unmutated seed was refused: first",
                Box::new(|case| if case == usize::MAX { Outcome::Refused("first".to_owned()) } else { passes() }),
            ),
            ("the unmutated seed was refused: always", Box::new(|_| Outcome::Refused("always".to_owned()))),
        ];
        for (words, control) in runs {
            let (door, seen) = recording(control);
            let report = sweep_with(&door, &config(12), Duration::from_secs(5));
            let baseline: Vec<&Failure> = report.failures.iter().filter(|f| f.case == usize::MAX).collect();
            // Both seeds fail the same way, once each (the same words on the second run are not a second failure).
            assert_eq!(baseline.len(), 2, "{words}: {:?}", report.failures);
            assert!(baseline.iter().all(|f| f.what.contains(words)), "{words}: {baseline:?}");
            assert_eq!(report.made, 0, "{words}: a mutant was made of a baseline that failed");
            let seen = cases_seen(&seen);
            assert!(seen.iter().all(|case| *case >= usize::MAX - 1), "{words}: a mutant ran: {seen:?}");
        }
    }

    /// A seed that does not end in the control's budget ends the control: the report is failed at once, and no other run of a seed and no mutant is started (the thread that
    /// does not end cannot be stopped, and each run that hangs after it takes the whole budget again).
    #[test]
    fn a_control_that_does_not_end_fails_the_report_at_once_and_starts_no_other_worker() {
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = Arc::clone(&release);
        let (door, seen) = recording(move |_| {
            while !hold.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            passes()
        });
        let started = Instant::now();
        let report = sweep_with(&door, &config(12), Duration::from_millis(200));
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(report.timed_out, "{}", report.summary());
        let baseline: Vec<&Failure> = report.failures.iter().filter(|f| f.case == usize::MAX).collect();
        assert_eq!(baseline.len(), 1, "{:?}", report.failures);
        assert!(baseline.iter().all(|f| f.what.contains("did not end")), "{baseline:?}");
        assert_eq!(report.made, 0, "a mutant was made after a control that did not end");
        assert_eq!(cases_seen(&seen), [usize::MAX], "only the first run of the first seed was started");
        assert!(started.elapsed() < Duration::from_secs(5), "the report was not at once: {:?}", started.elapsed());
    }

    #[test]
    fn the_budget_of_a_case_grows_with_its_size_and_the_machines_slowness_and_stops_at_the_ceiling() {
        let quick = Duration::from_millis(20);
        assert_eq!(budget_for(quick, 0), BUDGET_FLOOR + quick * BUDGET_FACTOR);
        assert!(budget_for(quick, 7 << 20) > budget_for(quick, 1 << 10));
        assert!(budget_for(Duration::from_millis(200), 1 << 10) > budget_for(quick, 1 << 10), "a slower machine is given more");
        assert_eq!(budget_for(Duration::from_secs(10), 7 << 20), BUDGET_CEILING);
        assert_eq!(budget_for(Duration::MAX, usize::MAX), BUDGET_CEILING, "no overflow");
        assert!(BUDGET_CEILING < Duration::from_secs(120), "under what nextest allows a test");
    }

    #[test]
    fn dump_folders_are_under_the_temporary_folder_and_named_with_plain_characters() {
        assert!(dump_folder("../x").is_none() && dump_folder(".hidden").is_none() && dump_folder("a/b").is_none() && dump_folder("").is_none());
        if let Some(folder) = dump_folder("name-1.2_x") {
            assert!(folder.ends_with("diacraft-door/name-1.2_x") && !folder.starts_with("/tmp"));
        }
    }
}
