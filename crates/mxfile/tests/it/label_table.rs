//! The reader of labels held to S-R25's table row by row, to S-R26 and S-R27, and to the work it counts. Each test says in its name
//! which row it holds the reader to, and builds its label by hand: no label of any file is read, and none is committed. The vectors are in `style_vectors.rs`, the
//! hostile labels in `label_hostile.rs`.

use std::sync::Arc;

use diacraft_mxfile::html::{
    Align, LINE_BREAK, Label, Link, Options, PARA_STEPS, Para, RUN_STEPS, Rgb, Run, RunFmt, Script, read_html, read_plain, warn,
};
use diacraft_mxfile::{Limits, Reading, Report};

/// A label read, and what the read said.
pub(crate) struct Got {
    pub(crate) label: Label,
    pub(crate) report: Report,
}

/// An HTML label read with `opts` under `limits`.
pub(crate) fn html_with(text: &str, opts: &Options<'_>, limits: &Limits) -> Got {
    let mut reading = Reading::new(limits);
    let label = read_html(text, opts, &mut reading).expect("a label within the work budget");
    Got { label, report: reading.report }
}

/// An HTML label read as a cell with the defaults reads it.
pub(crate) fn html(text: &str) -> Got {
    html_with(text, &Options::default(), &Limits::default())
}

/// A plain label read as a cell with the defaults reads it.
pub(crate) fn plain(text: &str) -> Got {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let label = read_plain(text, &Options::default(), &mut reading).expect("a label within the work budget");
    Got { label, report: reading.report }
}

/// The text of each paragraph.
pub(crate) fn lines(got: &Got) -> Vec<String> {
    got.label.paras.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect()).collect()
}

/// The runs of the only paragraph, as (text, format).
pub(crate) fn runs(got: &Got) -> Vec<(String, RunFmt)> {
    assert_eq!(got.label.paras.len(), 1, "one paragraph: {:?}", lines(got));
    got.label.paras[0].runs.iter().map(|r| (r.text.clone(), r.fmt.clone())).collect()
}

/// How many times a warning class was noted.
pub(crate) fn count(report: &Report, class: &str) -> u64 {
    report.warnings.get(class).map_or(0, |n| n.count)
}

/// The cases of a warning class.
pub(crate) fn cases(report: &Report, class: &str) -> Vec<String> {
    report.warnings.get(class).map(|n| n.cases.clone()).unwrap_or_default()
}

pub(crate) fn fmt() -> RunFmt {
    RunFmt::default()
}

pub(crate) fn bold() -> RunFmt {
    RunFmt { bold: Some(true), ..fmt() }
}

pub(crate) fn italic() -> RunFmt {
    RunFmt { italic: Some(true), ..fmt() }
}

pub(crate) fn sized(px: f64) -> RunFmt {
    RunFmt { size_px: Some(px), ..fmt() }
}

pub(crate) fn coloured(r: u8, g: u8, b: u8) -> RunFmt {
    RunFmt { color: Some(Rgb::new(r, g, b)), ..fmt() }
}

const BR: char = LINE_BREAK;

/// What one paragraph and one run that are built cost.
pub(crate) const BUILT: u64 = PARA_STEPS + RUN_STEPS;

// ---------------------------------------------------------------------------------------------------------------------------------------------------- text and references

#[test]
fn s_r25_text_and_the_six_named_references_and_numeric_references_are_characters() {
    let got = html("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; f&nbsp;g &#65;&#x42;&#X43;");
    assert_eq!(lines(&got), ["a & b <c> \"d\" 'e' f\u{A0}g ABC"]);
    assert_eq!(runs(&got), [("a & b <c> \"d\" 'e' f\u{A0}g ABC".to_owned(), fmt())], "one run: no format said anything");
}

#[test]
fn s_r25_another_named_reference_is_kept_as_its_literal_text() {
    let got = html("&copy; &hellip; &eacute; &unknown; &amp");
    assert_eq!(lines(&got), ["&copy; &hellip; &eacute; &unknown; &amp"]);
}

#[test]
fn s_r25_an_escaped_tag_is_text_and_not_a_tag() {
    let got = html("&lt;b&gt;not bold&lt;/b&gt; &lt;script&gt;x()&lt;/script&gt;");
    assert_eq!(lines(&got), ["<b>not bold</b> <script>x()</script>"]);
    assert_eq!(runs(&got), [("<b>not bold</b> <script>x()</script>".to_owned(), fmt())]);
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 0);
}

#[test]
fn design_4_a_numeric_reference_to_a_control_character_is_removed_and_a_number_no_character_has_is_the_replacement_character() {
    assert_eq!(lines(&html("a&#1;b&#8;c&#11;d&#12;e&#14;f&#31;g&#0;h")), ["abcdefgh"]);
    assert_eq!(lines(&html("a&#xFFFE;b&#xFFFF;c")), ["abc"]);
    assert_eq!(lines(&html("a&#xD800;b&#x110000;c&#99999999999999999999;d")), ["a\u{FFFD}b\u{FFFD}c\u{FFFD}d"]);
    assert_eq!(lines(&html("a\u{1}b\u{FFFE}c\u{B}d")), ["abcd"], "and the characters themselves");
    // Tab, line feed and carriage return are white space, and in a `pre` they stay.
    assert_eq!(lines(&html("a&#9;b&#10;c&#13;d")), ["a b c d"]);
    assert_eq!(lines(&html("<pre>a&#9;b</pre>")), ["a\tb"]);
    // A reference to 0x80..0x9F is Windows-1252's, not a control.
    assert_eq!(lines(&html("&#150;&#133;")), ["\u{2013}\u{2026}"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- br, blocks, hr

#[test]
fn s_r25_br_is_a_line_break_inside_the_paragraph() {
    let got = html("a<br>b<BR/>c<br />d<br\n>e");
    assert_eq!(lines(&got), [format!("a{BR}b{BR}c{BR}d{BR}e")]);
}

#[test]
fn s_r25_each_block_ends_the_paragraph_before_it_and_after_it() {
    for tag in ["div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "blockquote", "pre", "DIV", "P"] {
        let got = html(&format!("A<{tag}>B</{tag}>C"));
        assert_eq!(lines(&got), ["A", "B", "C"], "{tag}");
    }
}

#[test]
fn s_r25_a_block_that_follows_text_is_a_paragraph_break_and_a_block_first_is_not() {
    assert_eq!(lines(&html("<div>A</div>")), ["A"]);
    assert_eq!(lines(&html("<div>A</div><div>B</div>")), ["A", "B"]);
    assert_eq!(lines(&html("A<div>B</div>")), ["A", "B"]);
    assert_eq!(lines(&html("<div>A</div>B")), ["A", "B"]);
    assert_eq!(lines(&html("<div><div>A</div></div>")), ["A"], "a block in a block with nothing between them");
    assert_eq!(lines(&html("<div>A<div>B</div>C</div>")), ["A", "B", "C"]);
    assert_eq!(lines(&html("<p>A<p>B")), ["A", "B"], "an end tag that is missing");
}

#[test]
fn s_r25_blocks_with_nothing_in_them_make_no_paragraph_but_a_br_makes_an_empty_line() {
    assert_eq!(lines(&html("<div></div><div> </div><p></p>A")), ["A"]);
    assert_eq!(lines(&html("<div>A</div>\n<div>B</div>\n")), ["A", "B"], "the white space between blocks makes no paragraph");
    // How the editor writes a blank line.
    assert_eq!(lines(&html("<div>A</div><div><br></div><div>B</div>")), ["A", "", "B"]);
    assert_eq!(html("<div><br></div>").label.paras.len(), 1);
    assert!(html("<div><br></div>").label.is_empty(), "one empty line");
}

#[test]
fn design_a_br_at_the_end_of_a_block_makes_no_line_of_its_own() {
    assert_eq!(lines(&html("<div>A<br></div>")), ["A"]);
    assert_eq!(lines(&html("A<br>")), ["A"]);
    assert_eq!(lines(&html("<div>A<br><br></div>")), [format!("A{BR}")], "two make one empty line");
    assert_eq!(lines(&html("<div>A<br>B</div>")), [format!("A{BR}B")]);
    assert_eq!(lines(&html("<br><br>A")), [format!("{BR}{BR}A")], "and at the start they are lines");
}

#[test]
fn s_r25_h1_to_h6_are_also_bold() {
    for n in 1..=6 {
        let got = html(&format!("<h{n}>Title</h{n}>text"));
        assert_eq!(got.label.paras.len(), 2, "h{n}");
        assert_eq!(got.label.paras[0].runs, [Run { text: "Title".to_owned(), fmt: bold() }], "h{n}");
        assert_eq!(got.label.paras[1].runs, [Run { text: "text".to_owned(), fmt: fmt() }], "h{n}");
    }
    // Any heading's end tag closes the heading that is open.
    assert_eq!(lines(&html("<h1>a</h2>b")), ["a", "b"]);
}

#[test]
fn s_r25_hr_is_a_paragraph_break_and_makes_no_empty_paragraph() {
    assert_eq!(lines(&html("a<hr>b")), ["a", "b"]);
    assert_eq!(lines(&html("<hr>a<hr/>")), ["a"]);
    assert_eq!(lines(&html("a<hr><hr>b")), ["a", "b"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- formats

#[test]
fn s_r25_the_formatting_elements_give_their_run_a_format() {
    let cases: [(&[&str], RunFmt); 6] = [
        (&["b", "strong"], bold()),
        (&["i", "em"], italic()),
        (&["u", "ins"], RunFmt { underline: Some(true), ..fmt() }),
        (&["s", "strike", "del"], RunFmt { strike: Some(true), ..fmt() }),
        (&["sub"], RunFmt { script: Script::Sub, ..fmt() }),
        (&["sup"], RunFmt { script: Script::Super, ..fmt() }),
    ];
    for (tags, want) in cases {
        for tag in tags {
            let got = html(&format!("a<{tag}>b</{tag}>c"));
            assert_eq!(runs(&got), [("a".to_owned(), fmt()), ("b".to_owned(), want.clone()), ("c".to_owned(), fmt())], "{tag}");
        }
    }
}

#[test]
fn s_r25_runs_are_merged_when_their_formats_are_equal() {
    assert_eq!(runs(&html("<b>a</b><strong>b</strong>c")), [("ab".to_owned(), bold()), ("c".to_owned(), fmt())]);
    assert_eq!(runs(&html("<span>a</span>b<span>c</span>")), [("abc".to_owned(), fmt())]);
    assert_eq!(runs(&html("a<font>b</font>c")), [("abc".to_owned(), fmt())], "a font that says nothing");
    assert_eq!(runs(&html("<b>a<b>b</b>c</b>")), [("abc".to_owned(), bold())], "bold in bold is the same");
    assert_eq!(runs(&html("<i>a</i><b>b</b>")), [("a".to_owned(), italic()), ("b".to_owned(), bold())]);
}

#[test]
fn s_r25_formats_add_up_as_the_elements_nest() {
    let both = RunFmt { bold: Some(true), italic: Some(true), ..fmt() };
    assert_eq!(runs(&html("<b><i>x</i>y</b>z")), [("x".to_owned(), both), ("y".to_owned(), bold()), ("z".to_owned(), fmt())]);
}

#[test]
fn s_r25_font_size_1_to_7_is_the_html_size_and_0_and_8_are_the_nearest() {
    let px = [(0, 12.0), (1, 12.0), (2, 14.2), (3, 16.0), (4, 19.2), (5, 24.0), (6, 32.0), (7, 48.0), (8, 48.0), (99, 48.0)];
    for (n, want) in px {
        let got = html(&format!("<font size=\"{n}\">t</font>"));
        assert_eq!(runs(&got), [("t".to_owned(), sized(want))], "size {n}");
    }
    // The relative forms count from 3; a size that is no number says nothing.
    assert_eq!(runs(&html("<font size=\"+2\">t</font>")), [("t".to_owned(), sized(24.0))]);
    assert_eq!(runs(&html("<font size=\"-1\">t</font>")), [("t".to_owned(), sized(14.2))]);
    assert_eq!(runs(&html("<font size=\"big\">t</font>")), [("t".to_owned(), fmt())]);
    assert_eq!(runs(&html("<font size=5>t</font>")), [("t".to_owned(), sized(24.0))], "no quotes");
}

#[test]
fn s_r25_font_face_and_color_are_the_runs_family_and_colour() {
    let got = html("<font face=\"Arial, Helvetica\" color=\"#ff0000\">t</font>");
    assert_eq!(runs(&got), [("t".to_owned(), RunFmt { family: Some(Arc::from("Arial")), color: Some(Rgb::new(255, 0, 0)), ..fmt() })]);
    let got = html("<font face=\"'Courier New'\" color=\"rgb(0, 128, 255)\">t</font>");
    assert_eq!(runs(&got), [("t".to_owned(), RunFmt { family: Some(Arc::from("Courier New")), color: Some(Rgb::new(0, 128, 255)), ..fmt() })]);
    // A colour that is no colour says nothing.
    assert_eq!(runs(&html("<font color=\"not a colour\" face=\"\">t</font>")), [("t".to_owned(), fmt())]);
    // A family never holds a character XML cannot hold, however it was written, and a link neither.
    assert_eq!(runs(&html("<font face=\"Ar&#1;ial\u{1}\">t</font>")), [("t".to_owned(), RunFmt { family: Some(Arc::from("Arial")), ..fmt() })]);
    assert_eq!(runs(&html("<font face=\"&#1;\">t</font>")), [("t".to_owned(), fmt())]);
    assert_eq!(html("<a href=\"https://a.b/&#1;x\">t</a>").label.link, Some(Link::Url("https://a.b/x".to_owned())));
}

#[test]
fn s_r25_a_style_attribute_keeps_seven_properties_and_ignores_the_others() {
    let got = html(
        "<span style=\"font-family: Verdana, sans-serif; font-size: 20px; color: #00f; font-weight: 700; font-style: italic; \
         text-decoration: underline; position: fixed; background-color: red; margin: 4px; display: none\">t</span>",
    );
    let want = RunFmt {
        bold: Some(true),
        italic: Some(true),
        underline: Some(true),
        family: Some(Arc::from("Verdana")),
        size_px: Some(20.0),
        color: Some(Rgb::new(0, 0, 255)),
        ..fmt()
    };
    assert_eq!(runs(&got), [("t".to_owned(), want)]);
    let got = html("<span style=\"text-decoration: line-through\">t</span>");
    assert_eq!(runs(&got), [("t".to_owned(), RunFmt { strike: Some(true), ..fmt() })]);
}

#[test]
fn s_r25_a_style_on_any_kept_element_is_read_and_on_one_that_is_not_kept_it_is_not() {
    let red = coloured(255, 0, 0);
    for tag in ["span", "b", "i", "u", "s", "sub", "sup", "font", "a", "strong", "em", "div", "p", "li", "td", "blockquote"] {
        let got = html(&format!("<{tag} style=\"color: red\">t</{tag}>"));
        let merged = got.label.paras.iter().flat_map(|p| p.runs.iter()).all(|r| r.fmt.color == red.color);
        assert!(merged && !got.label.paras.is_empty(), "{tag}: {:?}", got.label.paras);
    }
    for tag in ["center", "code", "small", "label", "unknown"] {
        let got = html(&format!("<{tag} style=\"color: red\">t</{tag}>"));
        assert_eq!(runs(&got), [("t".to_owned(), fmt())], "{tag}: its tags are not followed, so its style is not read");
    }
}

#[test]
fn s_r25_a_style_font_size_is_px_pt_or_em_of_the_labels_size() {
    let opts = Options { font_px: 16.0, ..Options::default() };
    let size = |style: &str| runs(&html_with(&format!("<span style=\"{style}\">t</span>"), &opts, &Limits::default()))[0].1.size_px;
    assert_eq!(size("font-size: 20px"), Some(20.0));
    assert_eq!(size("font-size: 9pt"), Some(12.0));
    assert_eq!(size("font-size: 1.5em"), Some(24.0), "an em is the size the cell has, 16 px here");
    assert_eq!(size("font-size: 50%"), None, "a percentage is not one of the three");
    assert_eq!(size("font-size: large"), None);
    assert_eq!(size("font-size: -3px"), None);
    // No size is no size: the label's own size is the cell's, said by the text block and not by a run.
    assert_eq!(size("color: red"), None);
}

#[test]
fn s_r25_font_weight_is_bold_from_600_and_normal_unbolds_inside_a_bold_run() {
    let weight = |w: &str| runs(&html(&format!("<span style=\"font-weight:{w}\">t</span>")))[0].1.bold;
    assert_eq!((weight("bold"), weight("600"), weight("900"), weight("bolder")), (Some(true), Some(true), Some(true), Some(true)));
    assert_eq!((weight("normal"), weight("400"), weight("599"), weight("lighter")), (Some(false), Some(false), Some(false), Some(false)));
    assert_eq!(weight("heavy"), None);
    assert_eq!(
        runs(&html("<b>a<span style=\"font-weight: normal\">b</span></b>")),
        [("a".to_owned(), bold()), ("b".to_owned(), RunFmt { bold: Some(false), ..fmt() })]
    );
}

#[test]
fn s_r25_text_align_on_a_block_is_the_paragraphs_and_a_block_inside_it_has_it_too() {
    let got = html(
        "<div style=\"text-align: center\">A</div><div>B</div><div style=\"text-align:right\"><p>C</p><p style=\"text-align: left\">D</p></div>",
    );
    let aligns: Vec<Option<Align>> = got.label.paras.iter().map(|p| p.align).collect();
    assert_eq!(lines(&got), ["A", "B", "C", "D"]);
    assert_eq!(aligns, [Some(Align::Center), None, Some(Align::Right), Some(Align::Left)]);
    // On an element that is not a block it says nothing: a `span` is inline.
    let got = html("<span style=\"text-align: center\">A</span>");
    assert_eq!(got.label.paras[0].align, None);
    // A line break stays in the paragraph, which keeps its alignment.
    let got = html("<div style=\"text-align: center\">A<br>B</div>");
    assert_eq!((got.label.paras.len(), got.label.paras[0].align), (1, Some(Align::Center)));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- lists, tables

#[test]
fn s_r25_a_ul_item_starts_with_a_bullet_and_an_ol_item_with_its_number() {
    assert_eq!(lines(&html("<ul><li>x</li><li>y</li></ul>")), ["\u{2022} x", "\u{2022} y"]);
    assert_eq!(lines(&html("<ol><li>x</li><li>y</li><li>z</li></ol>")), ["1. x", "2. y", "3. z"]);
    assert_eq!(lines(&html("<ol start=\"5\"><li>x</li></ol>")), ["1. x"], "the start attribute is not read");
    assert_eq!(lines(&html("<li>alone</li>")), ["\u{2022} alone"]);
    assert_eq!(lines(&html("a<ul><li>b</li></ul>c")), ["a", "\u{2022} b", "c"]);
}

#[test]
fn s_r25_lists_nest_and_each_ol_counts_its_own_items() {
    let got = html("<ol><li>a<ol><li>b</li><li>c</li></ol></li><li>d<ul><li>e</li></ul></li></ol><ol><li>f</li></ol>");
    assert_eq!(lines(&got), ["1. a", "1. b", "2. c", "2. d", "\u{2022} e", "1. f"]);
}

#[test]
fn s_r25_a_list_item_keeps_its_runs_and_the_prefix_is_in_the_items_own_format() {
    let got = html("<ul><li><b>x</b> y</li></ul>");
    assert_eq!(got.label.paras.len(), 1);
    assert_eq!(
        got.label.paras[0].runs,
        [Run { text: "\u{2022} ".to_owned(), fmt: fmt() }, Run { text: "x".to_owned(), fmt: bold() }, Run { text: " y".to_owned(), fmt: fmt() }]
    );
}

#[test]
fn design_the_end_tags_html_lets_a_page_leave_out_are_implied_by_the_next_start_tag() {
    assert_eq!(lines(&html("<ul><li>a<li>b<li>c</ul>")), ["\u{2022} a", "\u{2022} b", "\u{2022} c"]);
    assert_eq!(
        lines(&html("<ul><li>a<ul><li>b</ul><li>c</ul>")),
        ["\u{2022} a", "\u{2022} b", "\u{2022} c"],
        "an item after a list inside an item ends that item"
    );
    assert_eq!(lines(&html("<ol><li>a<li>b</ol>")), ["1. a", "2. b"]);
    assert_eq!(lines(&html("<table><tr><td>a<td>b<tr><td>c<td>d</table>")), ["a\tb", "c\td"]);
    assert_eq!(lines(&html("<table><tr><th>a<th>b<tr><td>c</table>")), ["a\tb", "c"]);
    assert_eq!(lines(&html("<p>a<p>b<div>c</div><p>d<ul><li>e</ul>")), ["a", "b", "c", "d", "\u{2022} e"]);
    // Only the top of the stack is looked at: an item with a format open inside it is not ended, and its next item is one inside the format.
    assert_eq!(lines(&html("<li>a<b>x<li>y")), ["\u{2022} ax", "\u{2022} y"]);
}

#[test]
fn s_r25_a_table_row_is_a_paragraph_and_its_cells_are_set_off_by_tabs() {
    assert_eq!(lines(&html("<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td><td>e</td></tr></table>")), ["a\tb", "c\td\te"]);
    assert_eq!(
        lines(&html("<table><thead><tr><th>h1</th><th>h2</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>")),
        ["h1\th2", "1\t2"]
    );
    assert_eq!(lines(&html("<table> <tr> <td> a </td> <td> b </td> </tr> </table>")), ["a\tb"], "white space around cells goes");
    assert_eq!(lines(&html("<table><tr><td>only</td></tr></table>")), ["only"]);
    assert_eq!(lines(&html("x<table><tr><td>a</td><td></td><td>c</td></tr></table>y")), ["x", "a\t\tc", "y"], "an empty cell keeps its tab");
    // Rows of two tables: a cell after a table starts no row with a tab.
    assert_eq!(lines(&html("<table><tr><td>a</td></tr></table><table><tr><td>b</td></tr></table>")), ["a", "b"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- links

#[test]
fn s_r25_the_first_link_of_the_label_is_kept_for_http_https_mailto_and_a_page_link() {
    let link = |href: &str| html(&format!("<a href=\"{href}\">go</a>")).label.link;
    assert_eq!(link("http://example.com/a?b=c#d"), Some(Link::Url("http://example.com/a?b=c#d".to_owned())));
    assert_eq!(link("https://example.com"), Some(Link::Url("https://example.com".to_owned())));
    assert_eq!(link("mailto:ann@example.com"), Some(Link::Url("mailto:ann@example.com".to_owned())));
    assert_eq!(link("data:page/id,p2"), Some(Link::Page("p2".to_owned())));
    assert_eq!(link("HTTPS://EXAMPLE.COM"), Some(Link::Url("HTTPS://EXAMPLE.COM".to_owned())));
    // The text is the text of the anchor, whatever the link.
    let got = html("a <a href=\"https://x.y\">go <b>far</b></a> b");
    assert_eq!(lines(&got), ["a go far b"]);
    assert_eq!(runs(&got), [("a go ".to_owned(), fmt()), ("far".to_owned(), bold()), (" b".to_owned(), fmt())], "an anchor adds no format");
}

#[test]
fn s_r25_a_link_with_any_other_scheme_is_dropped_and_its_text_stays() {
    for href in [
        "javascript:alert(1)",
        "JAVASCRIPT:alert(1)",
        " javascript:alert(1)",
        "java&#9;script:alert(1)",
        "java&#10;script:alert(1)",
        "&#106;avascript:alert(1)",
        "vbscript:msgbox(1)",
        "data:text/html,<script>alert(1)</script>",
        "data:action/json,{\"actions\":[]}",
        "file:///etc/passwd",
        "ftp://example.com",
        "/relative/path",
        "#fragment",
        "example.com",
        "",
    ] {
        let got = html(&format!("<a href=\"{href}\">go</a>"));
        assert_eq!(lines(&got), ["go"], "{href}");
        assert_eq!(got.label.link, None, "{href}");
    }
}

#[test]
fn s_r25_only_the_first_link_is_read_and_an_anchor_with_no_href_is_no_link() {
    let first = |text: &str| html(text).label.link;
    assert_eq!(first("<a href=\"https://a.b\">x</a><a href=\"https://c.d\">y</a>"), Some(Link::Url("https://a.b".to_owned())));
    assert_eq!(first("<a name=\"top\">x</a><a href=\"https://c.d\">y</a>"), Some(Link::Url("https://c.d".to_owned())));
    assert_eq!(first("<a href=\"javascript:x\">x</a><a href=\"https://c.d\">y</a>"), None, "the first link is the one that counts, kept or not");
    assert_eq!(first("<a href=https://a.b>x</a>"), Some(Link::Url("https://a.b".to_owned())), "no quotes");
    assert_eq!(first("<a href=\"https://a.b?x=1&amp;y=2\">x</a>"), Some(Link::Url("https://a.b?x=1&y=2".to_owned())), "its references are decoded");
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- dropped

#[test]
fn s_r25_an_element_that_cannot_be_shown_is_dropped_with_what_is_inside_and_counted_once() {
    for tag in ["svg", "math", "iframe", "object", "video", "audio", "canvas", "form", "button", "select", "textarea", "SVG"] {
        let got = html(&format!("x<{tag}>inside <b>bold</b> &amp; <i>more</i></{tag}>y"));
        assert_eq!(lines(&got), ["xy"], "{tag}");
        assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 1, "{tag}");
    }
}

#[test]
fn s_r25_an_img_an_input_and_an_embed_are_dropped_alone_and_counted() {
    for tag in ["img", "input", "embed"] {
        let got = html(&format!("x<{tag} src=\"a.png\">inside<{tag}/>y"));
        assert_eq!(lines(&got), ["xinsidey"], "{tag}: nothing waits for an end tag");
        assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 2, "{tag}");
    }
}

#[test]
fn s_r25_what_is_inside_a_dropped_element_is_not_counted_and_each_dropped_element_is() {
    let got = html("a<svg><svg>x</svg><circle/><script>y</script></svg>b<form><input><button>z</button></form>c<img><img>d");
    assert_eq!(lines(&got), ["abcd"]);
    assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 4, "the svg, the form and two img");
    assert_eq!(cases(&got.report, warn::LABEL_CONTENT_DROPPED), ["<svg>", "<form>", "<img>", "<img>"]);
    let got = html_with("<img>", &Options { cell: "c7", ..Options::default() }, &Limits::default());
    assert_eq!(cases(&got.report, warn::LABEL_CONTENT_DROPPED), ["c7: <img>"], "the cell is named");
}

#[test]
fn s_r25_script_style_template_noscript_comments_and_instructions_are_dropped_and_not_counted() {
    let texts = [
        "a<script>alert(1)</script>b",
        "a<SCRIPT type=\"x\">if (a < b) { x = \"</b>\"; }</SCRIPT>b",
        "a<style>p { color: red }</style>b",
        "a<template><b>x</b><script>y</script></template>b",
        "a<noscript>x<noscript>y</noscript>z</noscript>b",
        "a<!-- <b>x</b> -->b",
        "a<?php echo 1 ?>b",
        "a<!DOCTYPE html>b",
        "a<!-->b",
    ];
    for text in texts {
        let got = html(text);
        assert_eq!(lines(&got), ["ab"], "{text}");
        assert_eq!(count(&got.report, warn::LABEL_CONTENT_DROPPED), 0, "{text}");
    }
}

#[test]
fn s_r25_a_script_that_is_never_closed_takes_the_rest_of_the_label() {
    assert_eq!(lines(&html("a<script>b<c>d")), ["a"]);
    assert_eq!(lines(&html("a<script b")), ["a"]);
    assert_eq!(lines(&html("a<style>b")), ["a"]);
    assert_eq!(lines(&html("a<!-- b")), ["a"]);
    assert_eq!(lines(&html("a<svg>b<c>d")), ["a"]);
    assert_eq!(count(&html("a<svg>b").report, warn::LABEL_CONTENT_DROPPED), 1);
}

#[test]
fn s_r25_an_element_that_is_not_named_keeps_only_its_text() {
    assert_eq!(lines(&html("<center>a</center><code>b</code><small>c</small><foo:bar x=\"1\">d</foo:bar>e")), ["abcde"]);
    assert_eq!(runs(&html("<marquee><b>a</b></marquee>")), [("a".to_owned(), bold())]);
}

#[test]
fn s_r25_an_event_handler_a_javascript_url_and_any_other_attribute_are_ignored() {
    let got = html("<b onclick=\"alert(1)\" onmouseover=alert(2) class=\"x\" id=\"y\" data-z title=\"t\">t</b>");
    assert_eq!(runs(&got), [("t".to_owned(), bold())]);
    let got = html("<span style=\"color:red\" onclick=\"x()\">a</span><img src=\"javascript:alert(1)\" onerror=\"alert(2)\">");
    assert_eq!(runs(&got), [("a".to_owned(), coloured(255, 0, 0))]);
    let got = html("<font face=\"Arial\" onload=\"x()\" style=\"background:url(javascript:alert(1))\">t</font>");
    assert_eq!(runs(&got), [("t".to_owned(), RunFmt { family: Some(Arc::from("Arial")), ..fmt() })]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- white space

#[test]
fn s_r25_runs_of_white_space_are_one_space_and_a_line_has_none_at_its_ends() {
    assert_eq!(lines(&html("a   b\n\t c\r\nd")), ["a b c d"]);
    assert_eq!(lines(&html("  a  ")), ["a"]);
    assert_eq!(lines(&html("\n<div>\n a \n</div>\n")), ["a"]);
    assert_eq!(lines(&html("a <br> b")), [format!("a{BR}b")]);
    assert_eq!(lines(&html("<div>A </div> <div> B</div>")), ["A", "B"]);
    assert_eq!(lines(&html("<b>a </b>b")), ["a b"]);
    assert_eq!(runs(&html("<b>a </b>b")), [("a ".to_owned(), bold()), ("b".to_owned(), fmt())]);
    assert_eq!(runs(&html("<b>a</b> <i>b</i>")), [("a".to_owned(), bold()), (" ".to_owned(), fmt()), ("b".to_owned(), italic())]);
    assert_eq!(
        runs(&html("<b>a </b> <i> b</i>")),
        [("a ".to_owned(), bold()), ("b".to_owned(), italic())],
        "a space after a space is not kept, across elements"
    );
}

#[test]
fn s_r25_a_no_break_space_is_not_white_space() {
    assert_eq!(lines(&html("a&nbsp;&nbsp;b")), ["a\u{A0}\u{A0}b"]);
    assert_eq!(lines(&html("&nbsp;a&nbsp;")), ["\u{A0}a\u{A0}"]);
    assert_eq!(lines(&html("a\u{A0}\u{A0}b")), ["a\u{A0}\u{A0}b"]);
}

#[test]
fn s_r25_a_pre_block_keeps_its_white_space_and_its_line_feeds_are_line_breaks() {
    assert_eq!(lines(&html("<pre>a  b\n c\t d</pre>")), [format!("a  b{BR} c\t d")]);
    assert_eq!(lines(&html("<pre>\nx</pre>")), ["x"], "the first line feed after the tag is not text");
    assert_eq!(lines(&html("<pre>\n\nx</pre>")), [format!("{BR}x")]);
    assert_eq!(lines(&html("<pre>a\r\nb\rc</pre>")), [format!("a{BR}b{BR}c")]);
    assert_eq!(lines(&html("a<pre> b </pre>c")), ["a", " b ", "c"]);
    assert_eq!(lines(&html("<pre><b>a  b</b></pre> c  d")), ["a  b", "c d"], "inside a format; and white space collapses again after it");
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- the depth

#[test]
fn s_r25_element_nesting_is_followed_to_64_levels_and_the_65th_is_read_as_its_text() {
    let nest = |levels: usize, style: &str| format!("{}<span style=\"{style}\">t{}", "<span>".repeat(levels - 1), "</span>".repeat(levels));
    let at_64 = html(&nest(64, "color: red"));
    assert_eq!(runs(&at_64), [("t".to_owned(), coloured(255, 0, 0))], "the 64th level is followed");
    let at_65 = html(&nest(65, "color: red"));
    assert_eq!(runs(&at_65), [("t".to_owned(), fmt())], "the 65th is not: its text is the text of the 64th");
    // A smaller cap is the cap.
    let limits = Limits { max_label_depth: 2, ..Limits::default() };
    let got = html_with("<b><i><u>x</u></i></b>", &Options::default(), &limits);
    assert_eq!(runs(&got), [("x".to_owned(), RunFmt { bold: Some(true), italic: Some(true), ..fmt() })]);
    // A block past the cap breaks nothing and a list item past it has no bullet.
    let got = html_with("<b><i><div>x</div><li>y</li></i></b>", &Options::default(), &limits);
    assert_eq!(lines(&got), ["xy"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- S-R26, S-R27

#[test]
fn s_r26_a_plain_label_is_its_text_and_tags_and_references_in_it_are_text() {
    let got = plain("a<b>c &amp; &lt; <script>x()</script> <br>");
    assert_eq!(lines(&got), ["a<b>c &amp; &lt; <script>x()</script> <br>"]);
    assert_eq!(runs(&got), [("a<b>c &amp; &lt; <script>x()</script> <br>".to_owned(), fmt())]);
    assert_eq!(got.report.warnings.len(), 0, "nothing in it is dropped");
    assert_eq!(lines(&plain("  two  spaces\tand a tab  ")), ["  two  spaces\tand a tab  "], "its white space is kept");
}

#[test]
fn s_r26_in_a_plain_label_a_line_feed_is_a_line_break_and_backslash_n_is_two_characters() {
    assert_eq!(lines(&plain("one\ntwo")), [format!("one{BR}two")]);
    assert_eq!(lines(&plain("one\r\ntwo\rthree\n\nfour")), [format!("one{BR}two{BR}three{BR}{BR}four")]);
    assert_eq!(lines(&plain("one\\ntwo")), ["one\\ntwo"]);
    assert_eq!(plain("one\ntwo").label.paras.len(), 1, "one paragraph: a break, not a new paragraph");
    assert_eq!(lines(&plain("one\n")), ["one"], "a break at the very end makes no line");
    assert_eq!(lines(&plain("\none")), [format!("{BR}one")]);
    assert_eq!(plain("\n").label.paras.len(), 1);
    assert!(plain("\n").label.is_empty());
    assert_eq!(plain("").label.paras.len(), 0, "no text, no paragraph");
    assert_eq!(lines(&plain("a\u{1}\u{FFFE}\u{FFFF}b\u{B}")), ["ab"], "a character XML cannot hold is left out");
}

#[test]
fn s_r27_a_label_with_text_between_two_dollar_pairs_is_shown_as_it_is_and_counted() {
    let on = Options { math: true, cell: "7", ..Options::default() };
    let limits = Limits::default();
    let got = html_with("x $$a+b$$ y", &on, &limits);
    assert_eq!(lines(&got), ["x $$a+b$$ y"], "the delimiters stay");
    assert_eq!(count(&got.report, warn::MATH_SHOWN_AS_TEXT), 1);
    assert_eq!(cases(&got.report, warn::MATH_SHOWN_AS_TEXT), ["7: $$a+b$$"]);
    let mut reading = Reading::new(&limits);
    let label = read_plain("$$x$$", &on, &mut reading).unwrap();
    assert_eq!(label.text(), "$$x$$");
    read_html("`1+2` and `3`", &on, &mut reading).unwrap();
    read_html("<div>$$a</div><div>b$$</div>", &on, &mut reading).unwrap();
    assert_eq!(count(&reading.report, warn::MATH_SHOWN_AS_TEXT), 3, "each label once, across paragraphs too");
    // Not counted: one pair mark only, an odd number of backquotes, and a drawing whose math is off.
    for text in ["$$ only one", "$ a $", "one ` only", "no math", "$$"] {
        assert_eq!(count(&html_with(text, &on, &limits).report, warn::MATH_SHOWN_AS_TEXT), 0, "{text}");
    }
    assert_eq!(count(&html("x $$a$$ y").report, warn::MATH_SHOWN_AS_TEXT), 0, "math is off");
    assert_eq!(count(&plain("x $$a$$ y").report, warn::MATH_SHOWN_AS_TEXT), 0);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- the work

/// The steps a read of an HTML label counted.
fn steps(text: &str) -> u64 {
    html(text).report.work.label_steps
}

#[test]
fn design_7_a_label_charges_its_bytes_each_token_and_what_each_piece_of_text_decodes() {
    assert_eq!(steps(""), 0, "no label, no work");
    assert_eq!((PARA_STEPS, RUN_STEPS), (16, 8));
    assert_eq!(steps("a"), 1 + 1 + 1 + BUILT, "its byte, its one token, the byte of its text, and the paragraph and the run it makes");
    assert_eq!(steps("<b>x</b>"), 8 + 3 + 1 + BUILT, "eight bytes, three tokens, one byte of text, a paragraph and a run");
    assert_eq!(steps("&"), 1 + 1 + (1 + 33) + BUILT, "a stray ampersand may look 33 bytes ahead, and is charged for it");
    assert_eq!(steps("&amp;"), 5 + 1 + (5 + 33) + BUILT);
    assert_eq!(steps("<br>"), 4 + 1 + BUILT, "the break is in a run that is taken out again, and the paragraph is an empty line");
    assert_eq!(steps("<!-- x -->"), 10 + 1, "a comment builds nothing");
    // An end tag that closes nothing costs its token and no walk of the stack.
    assert_eq!(steps(&"</i>".repeat(1000)), 4000 + 1000);
    // Three paragraphs and three runs; and runs that are merged cost nothing more.
    assert_eq!(steps("<div>a</div><div>b</div><div>c</div>"), 36 + 9 + 3 + 3 * BUILT);
    assert_eq!(steps("a<b>b</b>c"), 10 + 5 + 3 + 3 * RUN_STEPS + PARA_STEPS, "three runs, one paragraph");
    assert_eq!(steps("<b>a</b><b>b</b>"), 16 + 6 + 2 + RUN_STEPS + PARA_STEPS, "one run, as the two are alike");
    // An attribute is charged for the bytes it decodes, and a style for the bytes it parses.
    assert_eq!(steps("<font size=5>"), 13 + 1 + 1);
    assert_eq!(steps("<span style=\"color:red\">"), 24 + 1 + 9 + 9);
}

#[test]
fn design_7_a_work_budget_that_is_passed_refuses_the_label_as_too_complex_and_counts_nothing_past_it() {
    let limits = Limits { max_read_work: 10, ..Limits::default() };
    let mut reading = Reading::new(&limits);
    let err = read_html("a long label with <b>tags</b> and more text", &Options::default(), &mut reading).unwrap_err();
    assert_eq!(err.class, "drawio_too_complex");
    assert!(err.words.contains("more work"), "{}", err.words);
    assert!(reading.report.work.total() <= 10, "the work counted never passes the budget: {}", reading.report.work.total());
    let mut reading = Reading::new(&limits);
    let err = read_plain("a long plain label that costs eleven steps", &Options::default(), &mut reading).unwrap_err();
    assert_eq!(err.class, "drawio_too_complex");
    // A label that fits is read, up to the last step.
    let limits = Limits { max_read_work: 12 + BUILT, ..Limits::default() };
    let mut reading = Reading::new(&limits);
    assert!(read_html("<b>x</b>", &Options::default(), &mut reading).is_ok());
    assert_eq!(reading.report.work.label_steps, 12 + BUILT);
    let limits = Limits { max_read_work: 11 + BUILT, ..Limits::default() };
    let mut reading = Reading::new(&limits);
    assert!(read_html("<b>x</b>", &Options::default(), &mut reading).is_err());
}

#[test]
fn the_label_a_read_gives_has_no_empty_run_no_two_runs_alike_and_no_character_xml_cannot_hold() {
    let labels = [
        "<b>a</b><b></b><b>b</b> <i> </i> c<span></span>d",
        "a<br><br>b<div>c<br></div><p></p>d&#1;e&nbsp;<font size=3></font>",
        "<ul><li><b></b></li><li> </li></ul><table><tr><td></td><td> </td></tr></table>",
    ];
    for text in labels {
        let got = html(text);
        assert!(!got.label.paras.is_empty(), "{text}: something to hold to the rule");
        for Para { runs, .. } in &got.label.paras {
            assert!(runs.iter().all(|r| !r.text.is_empty()), "{text}: an empty run in {runs:?}");
            assert!(runs.windows(2).all(|w| w[0].fmt != w[1].fmt), "{text}: two runs alike in {runs:?}");
            assert!(runs.iter().flat_map(|r| r.text.chars()).all(diacraft_mxfile::decode::keep_char), "{text}");
        }
    }
}
