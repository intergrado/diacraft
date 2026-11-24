//! Every cap that bounds a layout, in one place, with the reason for each.
//!
//! These are project rules, not the reference app's: no source bounds a text (reads up to the part
//! limit with no special cap, and caps rows at 65,536). Real texts are far smaller (the largest in the census is 8,197
//! characters), so these never touch a real drawing; they exist so a hostile one costs a bounded amount.

/// The most characters laid out in one block. The rest is not laid out and a [`Note::Truncated`](crate::Note::Truncated) says so.
pub const MAX_CHARS: usize = 262_144;
/// The most runs read in one block (a run with no characters still counts: a `cp` marker costs something).
pub const MAX_RUNS: usize = 262_144;
/// The most lines in one block. A block of no width puts every cluster on a line of its own.
pub const MAX_LINES: usize = 100_000;
/// The most scalar values of one grapheme cluster laid out: a letter with a hundred combining marks draws its first 32.
pub const MAX_CLUSTER_CHARS: usize = 32;
/// The most calls to the shaper in one block. A text that switches face or direction at every letter would otherwise make a
/// call for each; past the budget the rest is measured from the cmap without shaping.
pub const MAX_SHAPE_ITEMS: usize = 4096;
/// The longest font name read from a run, in characters; a longer name is cut.
pub const MAX_NAME_CHARS: usize = 256;
/// A length or size beyond this many inches (about 158 miles), or one that isn't a number, is read as its default: past it the
/// arithmetic of positions would overflow. It is no clamp on `Size` as means it (no floor at one point): any size a
/// drawing can really hold is far inside it.
pub const MAX_LENGTH: f64 = 1.0e7;
/// The most times one layout searches the whole database for a face that has a cluster's glyphs. Each search may look at every face
/// the machine has, so a text of many different letters no face draws would otherwise cost the letters times the faces; past the
/// budget those clusters draw their first face's missing-glyph box.
pub const MAX_FACE_SEARCHES: usize = 2048;
/// The most characters of a bullet's string drawn; past it the string is cut and a note says so. `BulletStr` may hold any string
/// But a bullet that is a paragraph of text has no use.
pub const MAX_BULLET_CHARS: usize = 256;
/// The most [`Note::StandIn`](crate::Note::StandIn) notes one layout makes: a drawing naming this many missing faces is damaged, and
/// a note for each of tens of thousands would be a cost for nothing.
pub const MAX_NOTES: usize = 256;
