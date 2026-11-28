//! The journal: the top-level commands that succeeded, with their params, oldest first.
//!
//! It is bounded in what it holds in memory, not only in what it shows: by a number of entries
//! ([`JOURNAL_LIMIT`]) and by an estimate of their memory ([`JOURNAL_MAX_BYTES`], with the leaves of the maps
//! under the objects counted: see [`entry_bytes`]). [`Journal::held_len`] and [`Journal::held_bytes`] report
//! what is held, so a test can see that the trims really run.

use serde_json::Value;

/// Most entries [`Journal::entries`] shows: the newest ones.
pub const JOURNAL_LIMIT: usize = 10_000;

/// How far past [`JOURNAL_LIMIT`] the journal grows before the oldest entries are let go, so that
/// trimming (a move of the entries that stay) is done once in a while instead of on every command.
pub const JOURNAL_SLACK: usize = 1_000;

/// The most memory the entries are allowed, by estimate: a legal batch of 256 `comment.author` calls with
/// 255-character names is about 275 KB of params, so the entry count alone would allow about 3 GB. The estimate counts the
/// leaves of the maps under the objects. In a release build the resident memory of the entries was 0.94 to 0.99 of it for
/// batches with long names, batches with one-letter names, batches of steps with no params and single commands (measured
///); before the leaves were counted it was 1.8 to 6.2 times the estimate (6.2 for 256 steps of `{"command": "page.add"}`).
pub const JOURNAL_MAX_BYTES: usize = 64 * 1024 * 1024;

/// What a [`serde_json::Value`] node takes before what it points to.
const NODE_BYTES: usize = std::mem::size_of::<Value>();

/// What an object entry takes besides its value and its key's text: the key's `String` and the share of
/// the map's node.
const OBJECT_ENTRY_BYTES: usize = std::mem::size_of::<String>() + 16;

/// How many entries one leaf of the `BTreeMap` under a [`serde_json::Map`] holds at the most.
const ENTRIES_PER_LEAF: usize = 11;

/// What the first entry of an object, and then every eleventh, costs in one go: a leaf of the `BTreeMap` under
/// the [`serde_json::Map`] (no `preserve_order` feature), which holds eleven keys and eleven values and a few words of its
/// own and is allocated whole however few entries are in it. 632 bytes on a 64-bit target, so a batch of 512 small objects
/// (256 steps, each with its params) is about 320 KB of leaves that the nodes and the text do not show. An empty object
/// allocates nothing.
const OBJECT_LEAF_BYTES: usize = ENTRIES_PER_LEAF * (std::mem::size_of::<String>() + std::mem::size_of::<Value>()) + 16;

/// What a journal entry takes besides its params: the id's `String`, the `Value` and its size record.
const ENTRY_BYTES: usize = std::mem::size_of::<(String, Value)>() + std::mem::size_of::<usize>();

/// Where the walk of [`entry_bytes`] is in an array or an object (and, in an object, how many entries it has taken).
enum Frame<'a> {
    Items(std::slice::Iter<'a, Value>),
    Entries(serde_json::map::Iter<'a>, usize),
}

/// An estimate of the memory the journal entry `id`, `params` holds. It is memory and not the length of the JSON: a
/// string of control characters is six times longer as JSON than in memory, while a number is a whole node and an object
/// with one entry is a node and a leaf of a map, however short its text. It walks the value once, counting each node,
/// each string's length, each object key and each leaf of an object's map (it does not serialise anything), and it stops
/// as soon as the total passes `cap`: the answer is then somewhere above `cap`, which is all a caller with a budget of
/// `cap` needs to know. The walk keeps its own stack, one frame per level, so a deeply nested value costs no more machine
/// stack than a flat one, and a huge array is not even queued before the walk stops.
pub fn entry_bytes(id: &str, params: &Value, cap: usize) -> usize {
    let mut total = ENTRY_BYTES.saturating_add(id.len());
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut next = Some(params);
    while total <= cap {
        let value = match next.take() {
            Some(value) => value,
            None => match stack.last_mut() {
                None => break,
                Some(Frame::Items(items)) => match items.next() {
                    Some(value) => value,
                    None => {
                        stack.pop();
                        continue;
                    }
                },
                Some(Frame::Entries(entries, taken)) => match entries.next() {
                    Some((key, value)) => {
                        if *taken % ENTRIES_PER_LEAF == 0 {
                            total = total.saturating_add(OBJECT_LEAF_BYTES);
                        }
                        *taken = taken.saturating_add(1);
                        total = total.saturating_add(OBJECT_ENTRY_BYTES).saturating_add(key.len());
                        value
                    }
                    None => {
                        stack.pop();
                        continue;
                    }
                },
            },
        };
        total = total.saturating_add(NODE_BYTES);
        match value {
            Value::String(text) => total = total.saturating_add(text.len()),
            Value::Array(items) => stack.push(Frame::Items(items.iter())),
            Value::Object(map) => stack.push(Frame::Entries(map.iter(), 0)),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    total
}

/// The journal of a session.
#[derive(Debug, Clone)]
pub struct Journal {
    /// Oldest first; holds up to `max_entries + slack` of them.
    entries: Vec<(String, Value)>,
    /// The estimated size of each entry, as [`entry_bytes`] gives it (cut at the budget).
    sizes: Vec<usize>,
    /// The sum of `sizes`.
    bytes: usize,
    max_entries: usize,
    slack: usize,
    max_bytes: usize,
}

impl Default for Journal {
    fn default() -> Self {
        Journal::new()
    }
}

impl Journal {
    /// An empty journal with the limits of a session: [`JOURNAL_LIMIT`], [`JOURNAL_SLACK`] and [`JOURNAL_MAX_BYTES`].
    pub fn new() -> Journal {
        Journal::with_limits(JOURNAL_LIMIT, JOURNAL_SLACK, JOURNAL_MAX_BYTES)
    }

    /// An empty journal that shows `max_entries`, holds up to `slack` more, and drops the oldest entries
    /// to stay within `max_bytes`.
    pub fn with_limits(max_entries: usize, slack: usize, max_bytes: usize) -> Journal {
        Journal { entries: Vec::new(), sizes: Vec::new(), bytes: 0, max_entries, slack, max_bytes }
    }

    /// Adds a command, then lets the oldest entries go: once the journal holds more than the limit
    /// plus the slack, until it holds the limit; and while it is over its byte budget. The entry just
    /// added is never let go, so a single entry larger than the budget is kept (and the next push
    /// drops it).
    pub fn push(&mut self, id: &str, params: Value) {
        // Cut at the budget: an entry that alone passes it is known to be over, and need not be walked on.
        let size = entry_bytes(id, &params, self.max_bytes);
        self.entries.push((id.to_owned(), params));
        self.sizes.push(size);
        self.bytes = self.bytes.saturating_add(size);
        self.trim();
    }

    fn trim(&mut self) {
        let mut drop = 0;
        if self.entries.len() > self.max_entries.saturating_add(self.slack) {
            drop = self.entries.len().saturating_sub(self.max_entries);
        }
        let mut bytes = self.bytes.saturating_sub(self.sizes.iter().take(drop).sum::<usize>());
        // Over the budget: the oldest go, up to the newest entry, which stays.
        while bytes > self.max_bytes && drop.saturating_add(1) < self.entries.len() {
            bytes = bytes.saturating_sub(self.sizes.get(drop).copied().unwrap_or(0));
            drop += 1;
        }
        if drop > 0 {
            self.entries.drain(..drop);
            self.sizes.drain(..drop);
            self.bytes = bytes;
        }
    }

    /// The newest entries, at most the limit, oldest first.
    pub fn entries(&self) -> &[(String, Value)] {
        self.entries.get(self.entries.len().saturating_sub(self.max_entries)..).unwrap_or_default()
    }

    /// How many entries are held in memory: [`Journal::entries`] shows at most the limit of them.
    pub fn held_len(&self) -> usize {
        self.entries.len()
    }

    /// The estimated size of the entries held in memory: at most the budget, unless the only entry held
    /// is larger than it.
    pub fn held_bytes(&self) -> usize {
        self.bytes
    }
}
