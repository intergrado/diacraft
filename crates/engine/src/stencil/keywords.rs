//! The search words of the built-in masters (ours).
//!
//! A master of a stencil file carries its own search words in its `ShapeKeywords` cell, and a shape search
//! reads them beside the name. The built-in masters are made in code and hold no such cell, so the words are kept here, **by the master's id**: plain English words for what
//! the figure is called in other places or is used for (a diamond is also a rhombus and the shape of a decision). They are our own choice, written from the figures and from
//! general knowledge of flowcharts (ISO 5807) and not copied from any application's stencil. A master with no entry has no words beyond its name.
//!
//! The words are not written into a drawing: a master that is dropped brings the cells it is made of, and the search reads the stencils, not the drawing.

/// The search words of the built-in master with this id, space separated, or `""`.
pub(super) fn of(master_id: &str) -> &'static str {
    match master_id {
        // Basic Shapes.
        "rectangle" => "box rectangular block",
        "square" => "box block four sided",
        "ellipse" => "oval egg",
        "circle" => "round disc dot",
        "triangle" => "three sided delta",
        "right_triangle" => "wedge ramp slope",
        "diamond" => "rhombus lozenge decision choice",
        "pentagon" => "five sided",
        "hexagon" => "six sided",
        "octagon" => "eight sided stop sign",
        "parallelogram" => "slanted rhomboid skew",
        "trapezoid" => "trapezium",
        // Basic Flowchart.
        "process" => "step action task operation box",
        "decision" => "branch choice question yes no diamond",
        "terminator" => "start end stop begin finish oval stadium",
        "data" => "input output io parallelogram",
        "document" => "paper report page",
        "multiple_documents" => "papers reports pages stack",
        "predefined_process" => "subroutine subprocess call",
        "manual_input" => "keyboard typed entry",
        "manual_operation" => "hand by",
        "preparation" => "setup initialise initialize hexagon",
        "on_page_reference" => "connector join link circle",
        "off_page_reference" => "connector join link page",
        "database" => "storage disk cylinder data store",
        "internal_storage" => "memory buffer",
        "delay" => "wait pause",
        "display" => "screen monitor show output",
        "summing_junction" => "sum add circle cross",
        "or" => "either alternative circle plus",
        // Decorative Shapes.
        "decorative.star-5" => "five point star favourite favorite rating",
        "decorative.star-6" => "six point star hexagram",
        "decorative.star-8" => "eight point star",
        "decorative.burst" => "explosion splash flash callout",
        "decorative.heart" => "love like favourite favorite",
        "decorative.lightning-bolt" => "flash electric power energy",
        "decorative.cross" => "plus add medical",
        "decorative.ribbon" => "banner award scroll",
        // Arrow Shapes.
        "arrows.right" => "next forward direction pointer",
        "arrows.left" => "back previous direction pointer",
        "arrows.up" => "rise increase direction pointer",
        "arrows.down" => "fall decrease direction pointer",
        "arrows.left-right" => "both two way double horizontal",
        "arrows.up-down" => "both two way double vertical",
        "arrows.four-way" => "move all directions compass",
        "arrows.bent" => "turn corner elbow",
        // Gane-Sarson.
        "gane-sarson.interface" => "external entity source sink terminator actor",
        "gane-sarson.process" => "transform function bubble",
        "gane-sarson.data-store" => "file storage repository",
        "gane-sarson.data-flow" => "arrow connector line",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every master of the five stencils this table was written for has words of its own (a stencil another task adds has its own table or none), and the table holds no
    /// entry for a master that does not exist.
    #[test]
    fn the_masters_of_the_five_first_stencils_have_search_words_and_no_entry_is_dead() {
        for id in ["basic", "flowchart", "decorative", "arrows", "gane-sarson"] {
            let stencil = crate::stencil::builtin_stencil(id).expect("a built-in stencil");
            for master in stencil.masters() {
                assert!(!of(master.id).is_empty(), "{} has no search words", master.id);
            }
        }
        let table = include_str!("keywords.rs");
        let in_table = table.lines().filter_map(|l| l.trim().strip_prefix('"')?.split_once("\" =>")).map(|(id, _)| id.to_owned());
        for id in in_table {
            if id.contains(' ') || id == "no-such-master" {
                continue;
            }
            assert!(crate::stencil::all_masters().any(|m| m.id == id), "the table has words for `{id}`, which is no built-in master");
        }
    }

    #[test]
    fn a_master_the_table_does_not_know_has_none() {
        assert_eq!(of("no-such-master"), "");
    }
}
