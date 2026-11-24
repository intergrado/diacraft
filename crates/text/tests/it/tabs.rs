//! Tab stops.
//! Every letter is 0.1 inch wide and positions are measured from the text area's left edge.

use diacraft_text::{BlockFormat, CharFormat, Laid, ParaFormat, TabStop, lay_out};

use crate::{close, em_db, left, run};

/// The two default tab steps real files hold.
const REAL_DEFAULTS: [f64; 2] = [0.5, 0.590_551];

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, font: "Em".into(), ..CharFormat::default() }
}

fn tabs(stops: &[(f64, u8)]) -> ParaFormat {
    ParaFormat { tabs: stops.iter().map(|(position, alignment)| TabStop { position: *position, alignment: *alignment }).collect(), ..left() }
}

fn lay(text: &str, p: &ParaFormat, default_stop: f64, margin: f64) -> Laid {
    let block = BlockFormat { vertical_align: 0, height: Some(2.0), default_tab_stop: default_stop, left_margin: margin, ..BlockFormat::default() };
    lay_out(&[run(text, &tenth(), p)], &block, Some(10.0), &em_db())
}

/// The x of each glyph drawn, in drawing order.
fn xs(laid: &Laid) -> Vec<f64> {
    laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect()
}

#[test]
fn tab_a_custom_stop_then_the_next_default_step() {
    // `a` tab `b` tab `c`, Position0 0.5, default stop 1: `b` at 0.5 from the area's edge, `c` at the next default step past it.
    let x = xs(&lay("a\tb\tc", &tabs(&[(0.5, 0)]), 1.0, 0.0));
    close(x[0], 0.0);
    close(x[1], 0.5);
    close(x[2], 1.0);
    // With a margin, the stops are still measured from the area, not from the shape's edge.
    let x = xs(&lay("a\tb\tc", &tabs(&[(0.5, 0)]), 1.0, 0.1));
    close(x[0], 0.1);
    close(x[1], 0.6);
    close(x[2], 1.1);
}

#[test]
fn defaulttabstop_stops_count_from_the_margin_in_whole_multiples() {
    // LeftMargin 0.1, DefaultTabStop 0.5, a run 0.7 wide and a tab: the next run starts 1.1 from the block's edge, which is 1.0 from the
    // margin. Counting from the block's edge gives 1.0, and a stop 0.5 past the run's end gives 1.3.
    let x = xs(&lay("aaaaaaa\tb", &left(), 0.5, 0.1));
    close(x[7], 1.1);
}

#[test]
fn defaulttabstop_the_stop_spacing_is_the_blocks_not_a_constant() {
    let x = xs(&lay("a\tb", &left(), 0.590551, 0.0));
    close(x[1], 0.590551);
}

#[test]
fn a_default_stop_of_zero_or_less_adds_one_space_and_never_loops() {
    for stop in [0.0, -1.0, f64::NAN] {
        let x = xs(&lay("a\tb\tc", &left(), stop, 0.0));
        assert_eq!(x.len(), 3);
        assert!(x[1] > x[0] && x[2] > x[1], "{x:?}");
    }
}

#[test]
fn position_a_tab_goes_to_the_first_stop_past_the_pen_not_the_first_stop() {
    // Every file's root holds a default step of 0.5 or 0.590551, never a number larger than any stop.
    for default in REAL_DEFAULTS {
        // Two stops at 1 and 2, the pen at 1.5: the tab goes to 2.
        let x = xs(&lay("aaaaaaaaaaaaaaa\tb", &tabs(&[(1.0, 0), (2.0, 0)]), default, 0.0));
        close(x[15], 2.0);
    }
}

#[test]
fn tab_stops_may_come_in_any_order() {
    // Every file's root holds a default step of 0.5 or 0.590551, never a number larger than any stop.
    for default in REAL_DEFAULTS {
        let x = xs(&lay("a\tb\tc", &tabs(&[(1.0, 0), (0.5, 0)]), default, 0.0));
        close(x[1], 0.5);
        close(x[2], 1.0);
    }
}

#[test]
fn tab_a_decimal_stop_lines_the_point_up_and_text_with_no_point_ends_there() {
    // Every file's root holds a default step of 0.5 or 0.590551, never a number larger than any stop.
    for default in REAL_DEFAULTS {
        let p = tabs(&[(2.0, 3)]);
        // `12.5`: the point at 2 inches, so the text starts at 1.8 (two digits before it).
        let x = xs(&lay("\t12.5", &p, default, 0.0));
        close(x[2], 2.0);
        // `12` with no point ends at 2 inches.
        let x = xs(&lay("\t12", &p, default, 0.0));
        close(x[1] + 0.1, 2.0);
        // Alignment 4 and above read as decimal too.
        let x = xs(&lay("\t12.5", &tabs(&[(2.0, 4)]), default, 0.0));
        close(x[2], 2.0);
    }
}

#[test]
fn alignment_a_right_stop_ends_the_text_there_and_a_centre_stop_centres_on_it() {
    // Every file's root holds a default step of 0.5 or 0.590551, never a number larger than any stop.
    for default in REAL_DEFAULTS {
        let right = xs(&lay("\tabcd", &tabs(&[(2.0, 2)]), default, 0.0));
        close(right[3] + 0.1, 2.0);
        let centre = xs(&lay("\tabcd", &tabs(&[(2.0, 1)]), default, 0.0));
        close(centre[0], 2.0 - 0.2);
        // No alignment cell means left.
        let left_stop = xs(&lay("\tabcd", &tabs(&[(2.0, 0)]), default, 0.0));
        close(left_stop[0], 2.0);
    }
}

#[test]
fn text_that_would_start_before_the_pen_starts_at_the_pen() {
    // Every file's root holds a default step of 0.5 or 0.590551, never a number larger than any stop.
    for default in REAL_DEFAULTS {
        // A right stop too near for the text: the tab never moves backwards.
        let x = xs(&lay("aaaaaaaa\tbbbbbbbb", &tabs(&[(1.0, 2)]), default, 0.0));
        assert!(x[8] >= 0.8 - 1e-9, "{x:?}");
    }
}

#[test]
fn a_thousand_tabs_in_a_line_are_laid_out_in_linear_time() {
    // Counted, not timed: the line breaker looks at each of the 10,000 clusters once (and again where a line is cut short of where its scan
    // reached), and a tab looks along the text after it up to the next tab, which is one letter here: about two looks for each cluster.
    // A tab that scanned to the end of the line, or the paragraph, would be 5,000 times 10,000.
    let text = "a\t".repeat(5000);
    diacraft_text::work::reset();
    let laid = lay(&text, &tabs(&[(0.5, 1), (1.0, 3), (1.5, 2)]), 0.5, 0.0);
    assert!(!laid.lines.is_empty());
    let looks = diacraft_text::work::cluster_steps();
    assert!((10_000..=30_000).contains(&looks), "{looks} looks at 10,000 clusters");
}
