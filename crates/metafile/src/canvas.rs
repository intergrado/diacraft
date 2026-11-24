//! Where drawing goes: the list of items, the clips they name, the report, and the budget that every item is charged to.

use std::collections::HashMap;

use crate::clip::Clip;
use crate::limits::{Budget, MAX_CLIPS};
use crate::model::{self, ClipPart, Detail, Drawn, Item, Report};

/// The output of a picture being read, and the budget it spends. A nested metafile gets its own `Canvas` over the same budget.
pub(crate) struct Canvas<'b> {
    pub items: Vec<Item>,
    pub clips: Vec<model::Clip>,
    pub report: Report,
    pub budget: &'b mut Budget,
    /// The clips already named, by the address of their `Rc`; the `Rc` is kept so that the address stays theirs.
    named: HashMap<usize, (Clip, u32)>,
}

impl<'b> Canvas<'b> {
    pub fn new(budget: &'b mut Budget) -> Canvas<'b> {
        Canvas { items: Vec::new(), clips: vec![model::Clip::default()], report: Report::default(), budget, named: HashMap::new() }
    }

    pub fn note(&mut self, detail: Detail, first: impl FnOnce() -> String) {
        self.report.note(detail, first);
    }

    /// The index of `clip` in the list, adding it the first time. `None` when the list is full or the points are past the budget.
    pub fn clip_index(&mut self, clip: &Clip) -> Option<u32> {
        if clip.is_unbounded() {
            return Some(0);
        }
        let key = std::rc::Rc::as_ptr(clip) as usize;
        if let Some((_, index)) = self.named.get(&key) {
            return Some(*index);
        }
        if self.clips.len() >= MAX_CLIPS {
            self.budget.note_cap("clips");
            return None;
        }
        if !self.budget.points(clip.points()) {
            return None;
        }
        let index = u32::try_from(self.clips.len()).ok()?;
        self.clips
            .push(model::Clip { parts: clip.parts.iter().map(|p| ClipPart { path: p.part.path.clone(), even_odd: p.part.even_odd }).collect() });
        self.named.insert(key, (clip.clone(), index));
        Some(index)
    }

    /// Adds an item cut by `clip`, charging the item and the `points` it holds. False when a cap refused it, or when the clip lets
    /// nothing through (the item would not show).
    pub fn emit(&mut self, clip: &Clip, drawn: Drawn, points: u64) -> bool {
        if clip.is_nothing() {
            return false;
        }
        // The dashes a stroke keeps are as much a copy as the points of its path: a pen of thousands of them used by every item would
        // otherwise hold them all for each.
        let dashes = match &drawn {
            Drawn::Path(p) => p.stroke.as_ref().map_or(0, |s| s.dashes.len() as u64),
            _ => 0,
        };
        if !self.budget.item() || !self.budget.points(points.saturating_add(dashes)) {
            return false;
        }
        let Some(clip) = self.clip_index(clip) else {
            return false;
        };
        self.items.push(Item { clip, drawn });
        true
    }

    /// Starts again, keeping the budget as it is: for the EMF half of a file when the EMF+ half could not be played. What the first
    /// attempt spent stays spent.
    pub fn reset(&mut self) {
        self.items.clear();
        self.clips.truncate(1);
        self.named.clear();
        self.report = Report::default();
    }

    /// The finished list: `(items, clips, report)`.
    pub fn finish(self) -> (Vec<Item>, Vec<model::Clip>, Report) {
        (self.items, self.clips, self.report)
    }
}
