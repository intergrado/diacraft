//! The law of the fuzz target `mxfile_label` (`fuzz/src/label.rs`, included below), held to a seed corpus and to random bytes by the reader's own tests, so that every input the target
//! crashed on is replayed here through the very same code (add it to `CRASHES`, with the commit that fixed it), and so that the law is checked on every run of the suite and not only by
//! the fuzzer.

use proptest::prelude::*;

#[path = "../../fuzz/src/label.rs"]
mod law;

/// Inputs the target crashed on, each with what it was.
const CRASHES: &[(&str, &[u8])] = &[];

/// A seed of `mode` made of the text `text`.
fn with(mode: u8, text: &str) -> Vec<u8> {
    let mut v = vec![mode];
    v.extend(text.bytes());
    v
}

/// The seeds: the vectors of the specification, one row of the table each, and what a hostile label is made of.
fn seeds() -> Vec<Vec<u8>> {
    let labels = [
        "<b>Title</b><br>Line &amp; <i>two</i><script>x()</script><img src=\"data:image/png,AAAA\">",
        "<div>A</div><div>B</div>",
        "<ul><li>x</li><li>y</li></ul>",
        "<font color=\"#ff0000\" size=\"5\">big</font>",
        "<span style=\"font-size:20px;color:#00f;position:fixed\">t</span>",
        "<a href=\"javascript:alert(1)\">go</a>",
        "a<b>c",
        "one\ntwo",
        // The rows of S-R25's table.
        "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; f&nbsp;g &#65;&#x42; &copy; &#1;",
        "<div>A</div>B<p>C<h1>D</h1><li>E<tr>F<blockquote>G<pre>H  I\n J</pre>",
        "<b>b</b><strong>s</strong><i>i</i><em>e</em><u>u</u><ins>n</ins><s>s</s><strike>k</strike><del>d</del><sub>b</sub><sup>p</sup>",
        "<font face=\"Arial\" size=\"7\" color=\"red\">f</font><font size=\"0\">z</font><font size=\"8\">e</font>",
        "<span style=\"font-family: 'A B', serif; font-size: 2em; color: rgb(1,2,3); font-weight: 600; font-style: italic; text-decoration: underline line-through; text-align: center\">t</span>",
        "<ol><li>a<ul><li>b</li></ul></li><li>c</li></ol><table><tr><td>a</td><td>b</td></tr><tr><th>c<th>d</table>",
        "a<hr>b<a href=\"https://x.y\">l</a><a href=\"mailto:a@b.c\">m</a><a href=\"data:page/id,p2\">p</a>",
        "<img><input><embed><svg><circle/></svg><math>x</math><iframe>y</iframe><object>z</object><video>v</video><audio></audio><canvas></canvas><form><button>b</button></form><select></select><textarea>t</textarea>",
        "<script>a</script><style>b</style><template>c</template><noscript>d</noscript><!--e--><?f?><!DOCTYPE g>",
        "<b onclick=\"alert(1)\" class=\"x\">t</b>",
        "x $$a+b$$ y `c`",
        // Hostile.
        "<script",
        "<b a=\"",
        "<!--",
        "</",
        "<<<<>>>>",
        "&&&&;;;;&#;&#x;&#xZ;&#99999999999999999999;",
        "<br><br><br>",
        "",
    ];
    let mut seeds = Vec::new();
    for mode in 0..6u8 {
        for label in labels {
            seeds.push(with(mode, label));
        }
    }
    // The pieces a recipe chooses, one of each, then floods of a few of them.
    seeds.push((0..=255u8).fold(vec![2u8], |mut v, b| {
        v.push(b);
        v
    }));
    for flood in [0xFF, 0xF8, 0xF4] {
        for piece in 0..110u8 {
            seeds.push(vec![2, piece, flood]);
        }
    }
    // Small caps: a cap of 0 and of 10, a depth of 0 and 1, a work of 0 and 40.
    for (cap, depth, work) in [(0u16, 0u16, 0u16), (10, 1, 40), (599, 7, 65_535), (0, 3, 0), (20, 0, 9)] {
        for size in 0..7u16 {
            let mut v = vec![3];
            for n in [cap, depth, work, size] {
                v.extend(n.to_be_bytes());
            }
            v.extend("<b><i>a</i>&amp;<div>b<br></div>".bytes());
            seeds.push(v);
        }
    }
    seeds.push(vec![]);
    seeds.push(vec![255; 64]);
    seeds
}

#[test]
fn the_seeds_hold_the_law() {
    for seed in seeds() {
        law::check(&seed);
    }
}

#[test]
fn every_input_that_crashed_the_target_holds_the_law() {
    for (what, input) in CRASHES {
        eprintln!("{what}");
        law::check(input);
    }
}

#[test]
fn every_byte_of_every_mode_and_a_sweep_of_short_inputs_hold_the_law() {
    for mode in 0..=255u8 {
        law::check(&[mode]);
    }
    for a in (0..=255u8).step_by(5) {
        for b in (0..=255u8).step_by(7) {
            for mode in 0..6u8 {
                law::check(&[mode, a, b, b'<', a, b'&', b';', b]);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Any bytes, in any mode.
    #[test]
    fn any_bytes_hold_the_law(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        law::check(&data);
    }

    /// Text that looks like what the reader meets: tags, references, quotes and styles, in every mode.
    #[test]
    fn text_of_the_kind_the_reader_meets_holds_the_law(mode in 0u8..6, text in "[A-Za-z0-9<>/=&#;:\"' \n\t!?$`%\u{e9}\u{a0}-]{0,600}") {
        law::check(&with(mode, &text));
    }

    /// Pieces chosen by bytes, floods now and then, with the caps made small or not.
    #[test]
    fn pieces_of_a_label_hold_the_law(mode in prop::sample::select(vec![2u8, 3, 5]), data in proptest::collection::vec(any::<u8>(), 0..200)) {
        let mut v = vec![mode];
        v.extend(data);
        law::check(&v);
    }
}
