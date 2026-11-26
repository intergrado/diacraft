//! The input and the properties of the fuzz target `mxfile_label`: bytes become a label, plain or HTML, and the reader of labels is held to its contract on them. It is a
//! file that the target (`fuzz_targets/mxfile_label.rs`) and the reader's own tests (`crates/mxfile/tests/it/label_fuzz.rs`) both include by path, so that the tests hold a seed corpus
//! and random inputs to the same law the fuzzer holds the bytes to, and replay on it the inputs the target crashed on. It uses nothing of the crate's library but the reader.
//!
//! **The input.** The first byte picks what the rest is: 0, an HTML label, the bytes read as text (as a lossy decoding makes of them); 1, a plain label; 2, an HTML label made of
//! pieces (tags, references, styles, white space and what a hostile label is made of) that the bytes choose, so that the fuzzer reaches the walk and not only the tokenizer; 3 and 4, the
//! same two with the caps of the read made small by the first bytes (the label's, the depth's, the work's) and a font size of any bit pattern; 5, the same pieces with `math` on and a cell named.
//!
//! **The properties.** The reader never panics and never hangs; the steps it counts never pass the budget it was given, whether it reads or refuses, and are at most 40 for each byte of
//! label it read (the look ahead of a stray `&` is the dearest, 35); a refusal is `drawio_too_complex`; what it gives is the same the second time, with the same report; no run
//! is empty and no two runs of a paragraph are alike; no character a run holds is one the clean takes out; a size is finite and above 0 and a paragraph count is at most the label's length
//! (and its text at most three times it: a break is three bytes for one, or for the four of `<br>`); and a label is cut to the cap with the cut counted.

use diacraft_mxfile::decode::keep_char;
use diacraft_mxfile::html::{Label, Link, Options, read_html, read_plain, warn};
use diacraft_mxfile::{DrawioError, Limits, Reading, Report};

/// The pieces a recipe is made of: what real labels are made of, and what hostile ones are.
const PIECES: &[&str] = &[
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<u>",
    "<s>",
    "<strong>",
    "<em>",
    "<sub>",
    "<sup>",
    "</sup>",
    "<font size=\"5\" color=\"#f00\" face=\"Arial, Helvetica\">",
    "<font size=\"0\" color=\"rgb(1,2,3)\">",
    "</font>",
    "<span style=\"color: red; font-size: 2em; font-weight: bold; text-align: center; position: fixed\">",
    "<span style=\"font-size:20px;font-family:'A B'\">",
    "</span>",
    "<div>",
    "<div style=\"text-align: right\">",
    "</div>",
    "<p>",
    "</p>",
    "<h1>",
    "</h3>",
    "<blockquote>",
    "<br>",
    "<br/>",
    "<hr>",
    "<ul>",
    "<ol>",
    "<li>",
    "</li>",
    "</ul>",
    "</ol>",
    "<table>",
    "<tr>",
    "<td>",
    "<th>",
    "</td>",
    "</tr>",
    "</table>",
    "<pre>",
    "</pre>",
    "<a href=\"https://x.y/?a=1&amp;b=2\">",
    "<a href=\"javascript:alert(1)\">",
    "<a href=\"data:page/id,p2\">",
    "</a>",
    "<img src=\"data:image/png,AAAA\">",
    "<input>",
    "<script>",
    "</script>",
    "<style>",
    "</style>",
    "<svg>",
    "</svg>",
    "<math>",
    "<iframe>",
    "</iframe>",
    "<form>",
    "<textarea>",
    "</textarea>",
    "<template>",
    "<!--",
    "-->",
    "<!DOCTYPE x>",
    "<?x?>",
    "<",
    ">",
    "</",
    "/>",
    "<b",
    "<a href=\"",
    "\"",
    "'",
    "=",
    "&amp;",
    "&lt;",
    "&nbsp;",
    "&copy;",
    "&#65;",
    "&#x1F600;",
    "&#1;",
    "&#0;",
    "&#xD800;",
    "&#x110000;",
    "&#99999999999999999999;",
    "&#150;",
    "&",
    "&;",
    "$$",
    "`",
    " ",
    "  ",
    "\t",
    "\n",
    "\r\n",
    "\u{a0}",
    "\u{2028}",
    "\u{e9}",
    "\u{4e2d}",
    "\u{1F600}",
    "\u{1}",
    "\u{FFFE}",
    "text",
    "a",
];

/// The text a byte stream is read as: UTF-8 with what is not made into replacement characters.
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The most bytes a recipe makes: a flood past it is the hostile tests' (`tests/it/label_hostile.rs`), and a fuzz run is the faster for the cap.
const RECIPE_MAX: usize = 256 << 10;

/// A label made of the pieces `bytes` choose, one piece for each byte, and a byte of 0xF0 and above repeats the piece before it a number of times, so that a flood is reachable.
fn recipe(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut last = "";
    for &b in bytes.iter().take(4096) {
        if b >= 0xF0 && !last.is_empty() {
            let times = 1usize << (b & 0x0F).min(12);
            out.push_str(&last.repeat(times));
        } else if let Some(piece) = PIECES.get(usize::from(b) % PIECES.len()) {
            out.push_str(piece);
            last = piece;
        }
        if out.len() > RECIPE_MAX {
            break;
        }
    }
    out
}

/// A two-byte number from the front of `bytes` and what is left.
fn take_u16(bytes: &[u8]) -> (u16, &[u8]) {
    match bytes {
        [a, b, rest @ ..] => (u16::from(*a) << 8 | u16::from(*b), rest),
        [a] => (u16::from(*a), &[]),
        [] => (0, &[]),
    }
}

/// What one run of the reader gave: a label or a refusal, and what it said.
type Outcome = (Result<Label, DrawioError>, Report);

fn read(text: &str, html: bool, opts: &Options<'_>, limits: &Limits) -> Outcome {
    let mut reading = Reading::new(limits);
    let result = if html { read_html(text, opts, &mut reading) } else { read_plain(text, opts, &mut reading) };
    (result, reading.report)
}

/// The label past the cap, as the reader cuts it: how many bytes it reads.
fn kept_len(text: &str, limits: &Limits) -> usize {
    text.len().min(limits.max_label_bytes)
}

/// Reads `text` twice and holds the reader to its contract.
fn law(text: &str, html: bool, opts: &Options<'_>, limits: &Limits) {
    let (first, report) = read(text, html, opts, limits);
    let (second, again) = read(text, html, opts, limits);
    assert_eq!(report, again, "the same label, two reports");
    let work = report.work;
    assert!(work.total() <= limits.max_read_work, "the work {} is past the budget {}", work.total(), limits.max_read_work);
    let kept = kept_len(text, limits);
    let cut = report.warnings.get(warn::LABEL_TOO_LONG).map_or(0, |n| n.count);
    assert_eq!(cut, u64::from(text.len() > limits.max_label_bytes), "a label past the cap is cut once, and counted once");
    match (first, second) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b, "the same label, two readings");
            check_label(&a, kept);
            let bound = 40 * u64::try_from(kept).unwrap_or(u64::MAX) + 100 + if opts.math { 3 * kept as u64 } else { 0 };
            assert!(work.label_steps <= bound, "{} steps for {kept} bytes", work.label_steps);
        }
        (Err(a), Err(b)) => {
            assert_eq!((a.kind, a.class, &a.words), (b.kind, b.class, &b.words), "the same label, two refusals");
            assert_eq!(a.class, "drawio_too_complex", "the only refusal a label gives is of its work");
        }
        _ => panic!("the same label, a label and a refusal"),
    }
}

/// What every label that was read holds.
fn check_label(label: &Label, kept: usize) {
    assert!(label.paras.len() <= kept.saturating_add(1), "{} paragraphs for {kept} bytes", label.paras.len());
    let mut bytes = 0usize;
    for para in &label.paras {
        for run in &para.runs {
            assert!(!run.text.is_empty(), "an empty run");
            assert!(run.text.chars().all(keep_char), "a character the clean takes out: {:?}", run.text);
            if let Some(px) = run.fmt.size_px {
                assert!(px.is_finite() && px > 0.0, "a size of {px}");
            }
            if let Some(family) = &run.fmt.family {
                assert!(!family.is_empty() && family.chars().all(keep_char), "a family of {family:?}");
            }
            bytes = bytes.saturating_add(run.text.len());
        }
        assert!(para.runs.windows(2).all(|w| w[0].fmt != w[1].fmt), "two runs alike in {:?}", para.runs);
    }
    assert!(bytes <= kept.saturating_mul(3).saturating_add(64), "{bytes} bytes of text for {kept} bytes of label");
    if let Some(Link::Url(text) | Link::Page(text)) = &label.link {
        assert!(!text.is_empty() && text.chars().all(keep_char), "a link of {text:?}");
    }
}

/// The caps of a read made small, and a size that is any bit pattern, from the front of `bytes`: the options, the limits and the rest.
fn small<'a>(bytes: &'a [u8], cell: &'a str) -> (Options<'a>, Limits, &'a [u8]) {
    let (cap, rest) = take_u16(bytes);
    let (depth, rest) = take_u16(rest);
    let (work, rest) = take_u16(rest);
    let (size, rest) = take_u16(rest);
    let limits = Limits {
        max_label_bytes: usize::from(cap % 600),
        max_label_depth: usize::from(depth % 8),
        // From nothing to a few tens of thousands of steps: both ends are reached.
        max_read_work: u64::from(work) * if work % 3 == 0 { 8 } else { 1 },
        ..Limits::default()
    };
    let font_px = match size % 7 {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => -3.0,
        3 => 0.0,
        4 => 1e300,
        _ => f64::from(size),
    };
    (Options { font_px, math: size % 2 == 0, cell }, limits, rest)
}

/// The contract of the reader on `data`; panics when it breaks it.
pub fn check(data: &[u8]) {
    let (mode, body) = data.split_first().map_or((0, &[][..]), |(m, b)| (*m, b));
    let limits = Limits::default();
    match mode % 6 {
        0 => law(&text_of(body), true, &Options::default(), &limits),
        1 => law(&text_of(body), false, &Options::default(), &limits),
        2 => law(&recipe(body), true, &Options::default(), &limits),
        3 => {
            let (opts, small_limits, rest) = small(body, "");
            law(&text_of(rest), true, &opts, &small_limits);
            law(&recipe(rest), true, &opts, &small_limits);
        }
        4 => {
            let (opts, small_limits, rest) = small(body, "");
            law(&text_of(rest), false, &opts, &small_limits);
        }
        _ => {
            let opts = Options { font_px: 16.0, math: true, cell: "c1" };
            let text = recipe(body);
            law(&text, true, &opts, &limits);
            law(&text, false, &opts, &limits);
        }
    }
}
