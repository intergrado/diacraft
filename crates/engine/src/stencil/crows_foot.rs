//! Crow's Foot Database Notation (`crows-foot`, GUID series 20): eight masters drawn in code (section 6.5 of the second stencil batch's design, "Stencils: UML, BPMN, C4, network
//! and database", and of the batch's notation spec, which win where they differ).
//!
//! **Original figures.** An entity is a group of compartments (`kit::compartments`), a header band over its attributes, so that each is as tall as its text; a note is the kit's dog-eared note;
//! a relationship is a connector whose two ends are the marks of the notation. The names are the generic ones of the notation and the inventory's sample list (Entity, Attribute, Relationship,
//! Primary Key Attribute, Primary Key Separator: C77), and nothing is traced, measured or copied from a standard, an application or a sample file.
//!
//! * **The order** is the Shapes window's, and the Quick Shapes are the first four that are no line: Entity, Entity with Keys and Note.
//! * **An entity** (`Entity`, `Entity with Keys`) is a header band, tinted with the theme's accent (`MSOTINT(THEMEVAL("AccentColor",RGB(58,127,192)),70)`: our default with no theme), over a compartment
//!   of attributes, one per line; the keys variant has a compartment of keys between them. Each compartment is as tall as its text, the group as tall as its compartments, and `LockHeight` makes a
//!   resize change the width only (an entity's width can be changed, its height follows its attributes). **The header is the member typing goes to** (it is last in the table, `IsTextEditTarget`
//!   0), and the group is selected first (`SelectMode` 1).
//! * **A relationship** is a right-angled connector whose **begin and end are formulas of two Shape Data fields**, `BeginCardinality` and `EndCardinality`: each a fixed list of the four words, and the
//!   number a word gives is [`end_number`] (exactly one 25, zero or one 30, one or many 28, zero or many 29; a text that is none of the words draws no mark). A new relationship starts
//!   with zero or many at the begin and exactly one at the end; the three shortcuts differ only in the words they start with. `Identifying` (a Boolean, true to start) makes the line
//!   solid, and false makes it dashed (a convention of data modelling tools, ours here).
//! * **Not built.** The reference app's Attribute, Primary Key Attribute and Primary Key Separator shapes (the compartments of the entity stand for them), a foreign key made by a relationship,
//!   attribute types shown or hidden, and a menu on the relationship that sets its ends (right-click variants wait for the Actions of M4: the Shape Data window sets the same fields).

use super::group_master::{MemberFrame, MemberSpec};
use super::kit::{self, GRID3, SIDES};
use super::{CharRow, ConnectorLook, Dress, FieldDef, FieldKind, Flag, LineEnd, MasterSpec, More, Outline, Route, TextBox, TextSpec};

#[cfg(test)]
mod tests;

/// The series of the stencil's masters in their GUIDs (`D1AC0014-...`).
const SERIES: u8 = 20;

/// A cardinality: the minimum and the maximum of one side of a relationship, the four words of the fixed list. The masters name two of the four as the words they start with; the tests and the proof
/// of the number table read the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "the masters start with two of the four words: the tests and the proof of the line-end numbers read the others")]
pub(super) enum Cardinality {
    ExactlyOne,
    ZeroOrOne,
    OneOrMany,
    ZeroOrMany,
}

#[allow(dead_code, reason = "`ALL` and `of_word` are read by the tests and by `end_number`, which the proof of the line-end numbers reads")]
impl Cardinality {
    /// The four, in the order of the fixed list.
    pub(super) const ALL: [Cardinality; 4] = [Cardinality::ExactlyOne, Cardinality::ZeroOrOne, Cardinality::OneOrMany, Cardinality::ZeroOrMany];

    /// The word, as the Shape Data field holds it.
    pub(super) const fn word(self) -> &'static str {
        match self {
            Cardinality::ExactlyOne => "Exactly one",
            Cardinality::ZeroOrOne => "Zero or one",
            Cardinality::OneOrMany => "One or many",
            Cardinality::ZeroOrMany => "Zero or many",
        }
    }

    /// The line-end number of the cardinality (the table): exactly one 25 (two crossbars), zero or one 30 (a crossbar and a disc), one or many 28 (a fork and a crossbar), zero or many 29
    /// (a fork and a disc). The mark nearest the entity is the maximum and the farther one the minimum.
    pub(super) const fn number(self) -> u8 {
        match self {
            Cardinality::ExactlyOne => 25,
            Cardinality::ZeroOrOne => 30,
            Cardinality::OneOrMany => 28,
            Cardinality::ZeroOrMany => 29,
        }
    }

    /// The cardinality a word names, if it is one of the four, spelled as the list spells it.
    pub(super) fn of_word(word: &str) -> Option<Cardinality> {
        Cardinality::ALL.into_iter().find(|c| c.word() == word)
    }
}

/// The line-end number of a cardinality word (the table, [`Cardinality::number`]). **A text that is none of the four words is 0, no mark**: a fixed list set to text no item matches shows no
/// variant, whatever its length. The formulas of the masters ([`BEGIN_FORMULA`], [`END_FORMULA`]) say the same, and a test holds the two together.
#[allow(
    dead_code,
    reason = "the formulas of the masters say the same numbers: the tests hold the two together and the proof of the line-end numbers reads this one"
)]
pub(super) fn end_number(word: &str) -> u8 {
    Cardinality::of_word(word).map_or(0, Cardinality::number)
}

/// The formula of an end that reads the Shape Data row `$row`: a number for each of the four words, as [`end_number`] has them, and 0 for any other text.
macro_rules! end_formula {
    ($row:literal) => {
        concat!(
            "IF(STRSAME(Prop.",
            $row,
            ",\"Exactly one\"),25,IF(STRSAME(Prop.",
            $row,
            ",\"Zero or one\"),30,IF(STRSAME(Prop.",
            $row,
            ",\"One or many\"),28,IF(STRSAME(Prop.",
            $row,
            ",\"Zero or many\"),29,0))))"
        )
    };
}

/// `BeginArrow` of a relationship: the number of `BeginCardinality`'s word.
const BEGIN_FORMULA: &str = end_formula!("BeginCardinality");
/// `EndArrow` of a relationship: the number of `EndCardinality`'s word.
const END_FORMULA: &str = end_formula!("EndCardinality");
/// `LinePattern` of a relationship: solid (1) when it is identifying, dashed (2) when it is not.
const PATTERN_FORMULA: &str = "IF(Prop.Identifying,1,2)";

/// The four words, the items of a cardinality's fixed list (apart at semicolons).
const CARDINALITIES: &str = "Exactly one;Zero or one;One or many;Zero or many";

/// The Shape Data of a relationship: the words at its two ends and whether it is identifying (true to start).
const fn fields(begin: &'static str, end: &'static str) -> [FieldDef; 3] {
    [
        FieldDef {
            row: "BeginCardinality",
            label: "Begin Cardinality",
            kind: FieldKind::FixedList(CARDINALITIES),
            value: begin,
            prompt: "The minimum and maximum at the begin: Exactly one, Zero or one, One or many or Zero or many",
        },
        FieldDef {
            row: "EndCardinality",
            label: "End Cardinality",
            kind: FieldKind::FixedList(CARDINALITIES),
            value: end,
            prompt: "The minimum and maximum at the end: Exactly one, Zero or one, One or many or Zero or many",
        },
        FieldDef {
            row: "Identifying",
            label: "Identifying",
            kind: FieldKind::Boolean,
            value: "1",
            prompt: "A solid line when identifying, a dashed one when not",
        },
    ]
}

/// Makes the statics of a relationship inside a module of its own: `module::LOOK` is its look (the two ends as formulas, with the numbers [`end_number`] gives the words it starts with, a
/// right-angled route) and `module::MORE` its extras (the Shape Data and the pattern).
macro_rules! relationship {
    ($module:ident, $begin:ident, $end:ident) => {
        mod $module {
            use super::{
                BEGIN_FORMULA, Cardinality, ConnectorLook, Dress, END_FORMULA, FieldDef, Flag, LineEnd, More, PATTERN_FORMULA, Route, fields,
            };

            pub(super) const LOOK: ConnectorLook = ConnectorLook {
                route: Route::RightAngle,
                ..ConnectorLook::plain(
                    LineEnd::Formula { formula: BEGIN_FORMULA, value: Cardinality::$begin.number() },
                    LineEnd::Formula { formula: END_FORMULA, value: Cardinality::$end.number() },
                )
            };
            static DATA: [FieldDef; 3] = fields(Cardinality::$begin.word(), Cardinality::$end.word());
            static DRESS: Dress = Dress { line_pattern: Some(Flag::Formula(PATTERN_FORMULA)), data: &DATA, ..Dress::NONE };
            pub(super) static MORE: More = More { dress: Some(&DRESS), hidden: false, brings: &[] };
        }
    };
}

relationship!(relationship, ZeroOrMany, ExactlyOne);
relationship!(one_to_one, ExactlyOne, ExactlyOne);
relationship!(one_to_many, ExactlyOne, ZeroOrMany);
relationship!(many_to_many, ZeroOrMany, ZeroOrMany);

/// A note link: a straight dashed line with no ends.
const NOTE_LINK: ConnectorLook = ConnectorLook { pattern: 2, route: Route::Straight, ..ConnectorLook::plain(LineEnd::None, LineEnd::None) };

// ---- the entities ---------------------------------------------------------------------------------------------------------------------

/// The tint of an entity's header band: the theme's accent, lightened to 70 percent, or our blue (the design's section 4.7) when there is no theme.
const HEADER_FILL: &str = "MSOTINT(THEMEVAL(\"AccentColor\",RGB(58,127,192)),70)";

const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

/// The header: the entity's name in bold, on the tint. It is the member typing on the selected group goes to.
static HEADER_DRESS: Dress =
    Dress { fill: Some(HEADER_FILL), text: TextSpec::Plain("Entity"), chars: &[CharRow { size_pt: 12.0, style: 1, color: None }], ..Dress::NONE };

/// The lists of an entity: 10 points, plain, flowing down from the top of their compartment (`VerticalAlign` 0, written with the dress's cells), so that a line more never moves the lines above.
static LIST_CHARS: [CharRow; 1] = [CharRow { size_pt: 10.0, style: 0, color: None }];
static TOP_ALIGNED: [(&str, f64); 1] = [("VerticalAlign", 0.0)];
static ATTRIBUTES_DRESS: Dress = Dress { text: TextSpec::Plain("attribute : type"), chars: &LIST_CHARS, locks: &TOP_ALIGNED, ..Dress::NONE };
static KEYS_DRESS: Dress = Dress { text: TextSpec::Plain("PK  id"), chars: &LIST_CHARS, locks: &TOP_ALIGNED, ..Dress::NONE };

/// An entity is selected as a group first, its members hold the text (the header is typed on), and people join it (`ObjType` 1).
static ENTITY_DRESS: Dress = Dress { select_mode: Some(1), text_edit_target: Some(false), obj_type: Some(1), ..Dress::NONE };
static ENTITY_MORE: More = More { dress: Some(&ENTITY_DRESS), hidden: false, brings: &[] };

/// The header (at least 0.4 inch tall) over the attributes (0.8): 1.2 inches in all.
const ENTITY_FRAMES: [MemberFrame; 2] = kit::compartments([0.4, 0.8]);
static ENTITY_MEMBERS: [MemberSpec; 2] = [
    MemberSpec { name: "Attributes", frame: ENTITY_FRAMES[1], outline: Outline::Polygon(RECT), points: &[], dress: Some(&ATTRIBUTES_DRESS) },
    MemberSpec { name: "Name", frame: ENTITY_FRAMES[0], outline: Outline::Polygon(RECT), points: &[], dress: Some(&HEADER_DRESS) },
];

/// The header (0.4 inch), the keys (0.35) and the attributes (0.75): 1.5 inches in all.
const KEYS_FRAMES: [MemberFrame; 3] = kit::compartments([0.4, 0.35, 0.75]);
static KEYS_MEMBERS: [MemberSpec; 3] = [
    MemberSpec { name: "Attributes", frame: KEYS_FRAMES[2], outline: Outline::Polygon(RECT), points: &[], dress: Some(&ATTRIBUTES_DRESS) },
    MemberSpec { name: "Keys", frame: KEYS_FRAMES[1], outline: Outline::Polygon(RECT), points: &[], dress: Some(&KEYS_DRESS) },
    MemberSpec { name: "Name", frame: KEYS_FRAMES[0], outline: Outline::Polygon(RECT), points: &[], dress: Some(&HEADER_DRESS) },
];

pub(super) static MASTERS: [MasterSpec; 8] = [
    MasterSpec {
        identity: (SERIES, 1),
        id: "crows-foot.entity",
        name: "Entity",
        width: 2.0,
        height: 1.2,
        outline: Outline::Group { members: &ENTITY_MEMBERS, text: TextBox::FULL },
        points: &GRID3,
        more: &ENTITY_MORE,
    },
    MasterSpec {
        identity: (SERIES, 2),
        id: "crows-foot.entity-with-keys",
        name: "Entity with Keys",
        width: 2.0,
        height: 1.5,
        outline: Outline::Group { members: &KEYS_MEMBERS, text: TextBox::FULL },
        points: &GRID3,
        more: &ENTITY_MORE,
    },
    MasterSpec {
        identity: (SERIES, 3),
        id: "crows-foot.note",
        name: "Note",
        width: 1.4,
        height: 0.8,
        outline: Outline::Code { build: kit::note, text: TextBox::FULL },
        points: &SIDES,
        more: &More::PLACEABLE,
    },
    MasterSpec {
        identity: (SERIES, 4),
        id: "crows-foot.relationship",
        name: "Relationship",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(relationship::LOOK),
        points: &[],
        more: &relationship::MORE,
    },
    MasterSpec {
        identity: (SERIES, 5),
        id: "crows-foot.one-to-one",
        name: "One to One",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(one_to_one::LOOK),
        points: &[],
        more: &one_to_one::MORE,
    },
    MasterSpec {
        identity: (SERIES, 6),
        id: "crows-foot.one-to-many",
        name: "One to Many",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(one_to_many::LOOK),
        points: &[],
        more: &one_to_many::MORE,
    },
    MasterSpec {
        identity: (SERIES, 7),
        id: "crows-foot.many-to-many",
        name: "Many to Many",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(many_to_many::LOOK),
        points: &[],
        more: &many_to_many::MORE,
    },
    MasterSpec {
        identity: (SERIES, 8),
        id: "crows-foot.note-link",
        name: "Note Link",
        width: 1.5,
        height: 0.0,
        outline: Outline::Connector(NOTE_LINK),
        points: &[],
        more: &More::NONE,
    },
];

/// The search words of the master with this id, space separated, or `""`. Plain English: what the figure is called elsewhere and what it is used for, and the names of the
/// inventory's sample list for the notation (C77: Entity, Attribute, Relationship, Primary Key Attribute, Primary Key Separator) that its purpose serves.
pub(super) fn keywords(master_id: &str) -> &'static str {
    match master_id {
        "crows-foot.entity" => "table record row attribute attributes column entity relationship diagram database",
        "crows-foot.entity-with-keys" => "primary key attribute separator pk keys table record entity column",
        "crows-foot.note" => "comment annotation remark text label",
        "crows-foot.relationship" => "cardinality crow's crow foot crows connector line optionality identifying minimum maximum one many zero",
        "crows-foot.one-to-one" => "one one 1:1 cardinality relationship connector line",
        "crows-foot.one-to-many" => "one many 1:n 1:m cardinality relationship connector line parent child",
        "crows-foot.many-to-many" => "many many m:n cardinality relationship connector line junction",
        "crows-foot.note-link" => "comment annotation link connector dashed line attach",
        _ => "",
    }
}
