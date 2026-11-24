//! Kani proof harnesses for the byte code of DiaCraft's document file in an `.odg` package: the charge of a picture before its base64 text is decoded
//! (`body::picture_bytes`), and the age of a file's `version` (`docfile::Age::of`); the glue of the (`glue_box`, `past_frame_point`); and what the
//! model writer charges and declares of kept markup (`bounded`, `node_total` of a text, `head_total`, `Left::plus`, `need_of`, `declared_by`); and,
//! the law of the memory tests (`read::cost::held`, `slot`, `priced`), the charge of a text (`read::cost::text`, the codec's), what a read cost (`index::ReadCost`), a group's own entry of the document file
//! (`docfile::is_group_entry`), a field's text an element holds (`runs::model::field_text_ok`), and a held length's unit and a connector's type
//! as read (`lengths::unit_of_suffix`, `unit_of_word`, `shapes::asread::type_agrees`); and, the display a layer the save adds is written with and
//! the flags the reader takes back from it (`write::layers::NewLayer::display`, `read::layers::display_flags`), and the links of a shape put through the numbers of the
//! style sheets a document file restores (`ext::docfile::styleids::map_links`); and, the row of a layer that decides its flags and the flags
//! a layer of the layer set is written with (`write::layers::deciding`, `write::model::layer_set::chosen_flags`, taken out of `chosen`; `declared_flags` reads
//! `draw:display` by the reader's rule, `read::layers::display_flags`, now), the font size written (`write::runs::written_size`) and a negative length
//! (`ext::odf14::negative`), and the display forms of a fill (`write::fx::hatch_form`, `stipple_share`, `mixture`, `odf_angle_tenths`,
//! `GradientKey::lo_scale`, which is `lo_scale_of` the reader shares against `layout`), and a comment's text measured against its room and the places of a page's annotations after a save (
//! `read::comments::Sink`, `write::comments::counts_after`, taken out of `places_after`). Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved:** `write::zero::plain_field_text` (its search for two spaces, `str::contains` of a text, ran past 300 s for every text of three
//! bytes, of four bytes of four kinds, and with one symbolic byte of a character of two or three bytes); `zero::pair_refreshed` and `land` (a hash map of anchors);
//! `text::refreshed_field` (searches of a text); `odf14::zero_of` (`format!` and a search from the end), `refused_property` (`split_once`), `Namer::give` (a hash set);
//! `styleids::number`, `newstyles::places_of` and `markup_links` (hash maps and a document); the `shadow_blur_op` (it reads a sheet and the ops of a style),
//! `GradientKey::valid` (its conditions are the law; the round trip of a name it guards is a string's, `of_name`, which the exhaustive unit test holds) and the name codecs.
//! The numbering of the comments in `read::comments::finish` and `doc`'s `Comments::apply_threads` (hash maps and sets: the futex trap).
//!
//! **Not proved:** `asread::entry_name_ok` again (the string trap, no new idea); `write::layers::PageLayers::of` and `layers_text`'s total
//! (rows of names in hash maps, and escaped strings), `layers::reflagged` (names of sections); `canon::is_carrier` and the splitting of a list of names (strings, `split`); `styleids::number`,
//! `newstyles::places_of` and `markup_links` (hash maps and sets, and a document).
//!
//! **Not proved:** `text::model::drop_crossing` (three covers, and then two, ran CBMC out of 20 GB alone: the sort and the `retain` of a vector of
//! covers, which hold their tags as strings, are copies at places the solver does not know); the names of DiaCraft's entries and of the parts it carries (`items::entry_name_ok`,
//! `vsdx_part_name_ok`, `native_part_name_ok`, `asread::entry_name_ok`: `split('/')`, `contains`, `chars` and `to_ascii_lowercase` of a name, the string trap, which ran the VSDX
//! crate's `odf_entry_name_ok` out of 20 GB with only the case of each letter symbolic), `items::is_macro_part` (a `Part`, and the lower case of its name), `media_for_blob`
//! (`split_once`, the memchr trap) and the caps of the carried entries (comparisons inline in the walk of the parts); `field_element` (a text filtered by Unicode's blanks, collected and
//! put in upper case); `match_held` (hash maps); `ModelRows::of`'s charge, `cover_limit`, `list_limit` (a saturating expression each, in the writer's state); `Builder::chars` (`find`
//! of two characters in a builder of paragraphs); `Spaces::shown` (a saturating add); `Pfx::attr` (`format!`), `attr_ns` (`is_qname`), `walked` (an
//! `OdfShape`'s held values with kept nodes: the drop of a type that holds itself). The unit tests hold them.
//!
//! **Not proved here:** `node_total` of an element (a `RawElement` holding one text ran CBMC out of 20 GB); `is_qname` and `container_of` (`split_once`, the memchr trap,
//! and `is_ncname`'s Unicode classes); `declaration_ok` (namespaces of 36 characters and more); `compound_class` (its class is written with `format!`, which Kani does not finish;  the checks are the VSDX writer's `is_compound`, proved there);
//! `resolve`, `held::attribute` (names and paths matched as text); `Walk::object` (a walk of shapes and a set of
//! names); the knots of `route::quadratic_controls` (a closure inside the parse of a formula's text); the size sum of `kept_markup` (inline in a method of the editor). Their unit
//! tests hold them.

use diacraft_container::xml::El;

use crate::ext::body::{base64_decoded, picture_bytes};
use crate::ext::docfile::{Age, VERSION};

/// One character of each kind `base64_decoded` tells apart: a character of the alphabet, the padding, a blank, and one that is none of them.
const KINDS: &[u8; 4] = b"A= !";

/// `read_pictures` charges a blob to the budget of pictures **before** its text is decoded ("as the bytes it decodes to"): for **every** text of `N` characters, what
/// `base64_decoded` makes of it is never more than the charge for its length (`picture_bytes`). Two-sided: a text of `N` characters of the alphabet decodes to exactly the
/// charge, so a charge that is too high fails too. → how many bytes the text decoded to, for the harness's covers.
///
/// **Every text, by its kinds of character.** How many bytes the decoder makes, and whether it makes any, depends only on the kind of each character: one of the 64 of the
/// alphabet (which of them sets the bytes' values and not their number), the padding `=`, a blank (skipped), or anything else (no bytes at all, a character that is not ASCII
/// included, and it is at least one byte of the text). So a text of one character of each kind ([`KINDS`]) in each place stands for every text of that length. **One harness
/// for each length, one to four** (five took 124 s alone, 171 s in `cargo xtask kani` at load 28, and timed out at 300 s at load about 45, so it was dropped; six
/// and seven ran out of 20 GB beside the others; the unit test
/// `every_text_of_up_to_eight_characters_is_charged_no_less_than_it_decodes_to` takes every text of up to eight by its kinds): CBMC unrolls the loop that skips blanks inside
/// the decoder's own loop, each to the bound, so the program grows with the square of the bound; a symbolic length, or a bound of ten for one character, took 218 s and more.
fn charged_no_less_than_it_decodes_to<const N: usize>() -> Option<usize> {
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| KINDS[usize::from(k)]);
    let decoded = std::str::from_utf8(&raw).ok().and_then(base64_decoded).map(|bytes| bytes.len());
    if let Some(len) = decoded {
        assert!(len as u64 <= picture_bytes(N));
    }
    let full = std::str::from_utf8(&[b'A'; N]).ok().and_then(base64_decoded);
    assert!(full.map(|bytes| bytes.len() as u64) == Some(picture_bytes(N)));
    decoded
}

macro_rules! charged_no_less_than_it_decodes_to {
    ($($name:ident: $n:literal, $unwind:literal),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind($unwind)] // two more than the characters: the decoder's loop holds the loop that skips blanks, and each is unrolled inside the other
            fn $name() {
                let decoded = charged_no_less_than_it_decodes_to::<$n>();
                kani::cover!(decoded.is_some_and(|len| len as u64 == picture_bytes($n)), "a text that decodes to the whole charge");
                kani::cover!(decoded.is_none(), "a text that is no base64");
            }
        )*
    };
}

charged_no_less_than_it_decodes_to! {
    a_blob_of_one_character_is_charged_no_less_than_it_decodes_to: 1, 3,
    a_blob_of_two_characters_is_charged_no_less_than_it_decodes_to: 2, 4,
    a_blob_of_three_characters_is_charged_no_less_than_it_decodes_to: 3, 5,
    a_blob_of_four_characters_is_charged_no_less_than_it_decodes_to: 4, 6,
}

/// The longest `version` a harness reads.
const WORD: usize = 3;

/// `Age::of`: the `version` of a document file is the current one **exactly** when it is `2` ([`VERSION`]), an old one exactly when it is a canonical decimal
/// number below it (`0` or `1`, and not `01`, `+1` or ` 1`: an old file is kept and left unread, so a current file taken for an old one would lose what it holds), and
/// another for every other word of up to [`WORD`] bytes and for a root with no `version`.
#[kani::proof]
#[kani::unwind(10)] // the name `version` is compared byte by byte, seven of them
fn a_document_files_version_is_old_exactly_when_it_is_a_number_below_the_current_one() {
    let raw: [u8; WORD] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= WORD);
    let Ok(word) = std::str::from_utf8(&raw[..len]) else { return };
    let root = El { name: "diacraft:document".to_owned(), attrs: vec![("version".to_owned(), word.to_owned())], children: Vec::new() };
    match Age::of(&root) {
        Age::Current => {
            assert!(word == VERSION);
        }
        Age::Old(n) => {
            assert!((word == "0" && n == 0) || (word == "1" && n == 1));
            kani::cover!(n == 1, "version 1");
        }
        Age::Other => {
            assert!(word != VERSION && word != "0" && word != "1");
            kani::cover!(word == "01", "a number that is not canonical");
            kani::cover!(word == "3", "a newer version");
        }
    }
    let bare = El { name: "diacraft:document".to_owned(), attrs: Vec::new(), children: Vec::new() };
    assert!(Age::of(&bare) == Age::Other);
}

// --- Glue points ---

/// `glue_row_beside`, for **every** glue id and either flip: it is its own inverse (the writer uses it both ways, as `glue_id`, and `glue_id` of the row
/// it reads is the id again); it takes the four standard ids onto the four standard rows and every other id to itself, so no two ids name one row; beside no flip it is
/// `glue_row`, and beside one it keeps the side middles 1 and 3 where they are. It does not panic.
#[kani::proof]
fn a_glue_id_and_its_row_beside_a_flip_are_one_inverse_of_the_other() {
    use crate::read::{glue_row, glue_row_beside};
    use crate::write::glue_id;
    let (id, other, flipped): (u32, u32, bool) = (kani::any(), kani::any(), kani::any());
    let row = glue_row_beside(id, flipped);
    assert!(glue_row_beside(row, flipped) == id);
    assert!(glue_id(row, flipped) == id && glue_id(id, flipped) == row);
    assert!((id <= 3) == (row <= 3));
    if id > 3 {
        assert!(row == id);
    }
    if other != id {
        assert!(glue_row_beside(other, flipped) != row);
    }
    if !flipped {
        assert!(row == glue_row(id));
    } else if id == 1 || id == 3 {
        assert!(row == id);
    }
    kani::cover!(!flipped & (id == 1) & (row == 3), "the right side's middle is row 3 with no flip");
    kani::cover!(flipped & (id == 2) & (row == 2), "the bottom beside a flip");
}

/// `own_glue_row`: for **every** glue id, a shape whose rows are its own glue points has row `id - 4` for an id of 4 or more, and no row for a standard point
/// (0 to 3); the writer's `row + 4` is its inverse for every row it can name. It does not panic.
#[kani::proof]
fn an_own_glue_point_is_its_row_four_on_and_a_standard_point_is_no_row() {
    use crate::read::own_glue_row;
    let (id, row): (u32, u32) = (kani::any(), kani::any());
    match own_glue_row(id) {
        Some(r) => {
            assert!(id >= 4 && r == id - 4);
        }
        None => {
            assert!(id < 4);
        }
    }
    if let Some(named) = row.checked_add(4) {
        assert!(own_glue_row(named) == Some(row));
    }
    kani::cover!(own_glue_row(u32::MAX) == Some(u32::MAX - 4), "the last id");
}

/// `text_turn_beside_flips` with no vertical flip, for **every** turn the file gives the text (NaN and the infinities included): the model's text turn is the
/// file's number itself, to the last bit, with no flip at all, and minus it beside a horizontal flip. **With a vertical flip it is not proved**: the half turn more is folded
/// into (-180, 180] through `rem_euclid`, and Kani's model of the float remainder is not bounded by the divisor (`docs/development.md`, Proofs); the unit tests hold it.
#[kani::proof]
fn a_texts_turn_beside_no_vertical_flip_is_the_files_or_its_mirror() {
    use crate::read::text_turn_beside_flips;
    let (file, flip_x): (f64, bool) = (kani::any(), kani::any());
    let turn = text_turn_beside_flips(file, flip_x, false);
    let expected = if flip_x { (-file).to_bits() } else { file.to_bits() };
    assert!(turn.to_bits() == expected);
}

// --- What a model writer may write of kept markup (`write/model`) ---

/// The texts the harness of `bounded` draws a node from: no characters, one, and four. Each costs its length and at least a byte.
const TEXTS: [&str; 3] = ["", "a", "abcd"];

/// `bounded` on two text nodes of any of [`TEXTS`] and **any** cap: the markup is within the cap exactly when the two nodes' costs (each its length, and at
/// least a byte) add up to no more than it, so a cap of 0 takes no list that has a node; and it does not panic or wrap at the largest cap.
#[kani::proof]
#[kani::unwind(4)]
fn kept_markup_is_within_the_cap_exactly_when_its_nodes_costs_are() {
    let (a, b): (usize, usize) = (kani::any(), kani::any());
    kani::assume(a < TEXTS.len() && b < TEXTS.len());
    let cap: usize = kani::any();
    let text = |i: usize| diacraft_doc::RawNode::Text(std::sync::Arc::from(TEXTS[i]));
    let nodes = [text(a), text(b)];
    let cost = |i: usize| TEXTS[i].len().max(1);
    let within = crate::write::bounded(nodes.iter(), cap);
    assert!(within == (cost(a) + cost(b) <= cap));
    assert!(!crate::write::bounded(nodes.iter().take(1), 0));
    kani::cover!(within & (cost(a) + cost(b) == cap), "markup that fills the cap");
    kani::cover!(!within & (cap > 0), "markup past the cap");
}

// ---: the model writer's kept markup and the glue of a frame ---

/// `node_total` (what a node of kept markup is charged before it is copied, from what is left) for a text node of any of [`TEXTS`] and **any** cap: its cost (its
/// length, and at least a byte) when that is within the cap, and none when it is past it; and the same answer as `bounded` gives of the node alone, so a caller that takes several
/// nodes from one budget and the check of a list agree.
#[kani::proof]
#[kani::unwind(4)]
fn a_text_of_kept_markup_is_charged_its_cost_exactly_when_the_cap_holds_it() {
    use crate::write::{bounded, node_total};
    let a: usize = kani::any();
    kani::assume(a < TEXTS.len());
    let cap: usize = kani::any();
    let alone = diacraft_doc::RawNode::Text(std::sync::Arc::from(TEXTS[a]));
    let cost = TEXTS[a].len().max(1);
    let got = node_total(&alone, cap);
    assert!(got == (cost <= cap).then_some(cost));
    assert!(got.is_some() == bounded(std::iter::once(&alone), cap));
    kani::cover!(got == Some(cap), "a text that fills the cap");
    kani::cover!(got.is_none() & (cap > 0), "a text past the cap");
    std::mem::forget(alone);
}

/// The text the names and values of `head_total`'s harness are cut from.
const LETTERS: &str = "abcdefgh";

/// `head_total` (the charge of a page's or a master page's start tag, made of pieces the model holds apart) for a name and two attributes of any lengths up to 8 and
/// **any** cap: the name twice and 5, and each attribute's name, value and 4, when that is within the cap, and none past it (the sum is past the cap as soon as a part of it is).
#[kani::proof]
#[kani::unwind(7)]
fn a_start_tag_is_charged_its_name_twice_and_its_attributes_exactly_when_the_cap_holds_them() {
    use crate::write::head_total;
    let lens: [usize; 5] = kani::any();
    kani::assume(lens.iter().all(|n| *n <= LETTERS.len()));
    let cut = |n: usize| LETTERS.get(..n).unwrap_or("");
    let cap: usize = kani::any();
    let attrs = [(cut(lens[1]), cut(lens[2])), (cut(lens[3]), cut(lens[4]))];
    let got = head_total(cut(lens[0]), attrs.iter().copied(), cap);
    let whole = 2 * lens[0] + 5 + lens[1] + lens[2] + 4 + lens[3] + lens[4] + 4;
    assert!(got == (whole <= cap).then_some(whole));
    kani::cover!(got == Some(cap), "a tag that fills the cap");
}

/// `Left::plus` (what a step of the model writer left out, by kind, added up) for **every** two counts: each kind the two added, the largest number where that
/// passes it; and the order of the two does not matter, and nothing added is the count itself.
#[kani::proof]
fn what_two_steps_left_out_adds_up_by_kind_and_never_wraps() {
    use crate::write::Left;
    let any_left = || Left { definitions: kani::any(), layers: kani::any(), styles: kani::any(), containers: kani::any() };
    let (a, b) = (any_left(), any_left());
    let sum = a.plus(b);
    let add = |x: usize, y: usize| (x as u128 + y as u128).min(usize::MAX as u128) as usize;
    assert!(sum.definitions == add(a.definitions, b.definitions) && sum.layers == add(a.layers, b.layers));
    assert!(sum.styles == add(a.styles, b.styles) && sum.containers == add(a.containers, b.containers));
    assert!(sum == b.plus(a) && a.plus(Left::default()) == a);
}

/// The characters the prefixes and the namespaces of `need_of`'s harness are drawn from.
const PREFIX_KINDS: &[u8; 2] = b"ab";

/// A text of up to two characters of [`PREFIX_KINDS`], or none.
fn any_word(raw: &mut [u8; 2]) -> Option<&str> {
    let kinds: [u8; 2] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < PREFIX_KINDS.len()));
    *raw = kinds.map(|k| PREFIX_KINDS[usize::from(k)]);
    let len: usize = kani::any();
    kani::assume(len <= 2);
    let there: bool = kani::any();
    there.then(|| std::str::from_utf8(raw.get(..len).unwrap_or(&[])).unwrap_or(""))
}

/// `need_of` (what a prefix of a new shape's kept markup needs where it is written) for **every** namespace it was read in, namespace the place binds it to (texts of
/// up to two characters, or none) and place on the writer's own start tag or not: written as it is exactly when the place binds it to the namespace it was read in; declared again,
/// to that namespace, when the place binds it otherwise or not at all and the markup is not on the writer's own tag; and left out when the model does not know its namespace, or when
/// it would need a declaration on the writer's own tag. **A prefix bound otherwise is never taken as the same.**
#[kani::proof]
#[kani::unwind(4)]
fn a_prefix_is_written_as_it_is_only_where_the_place_binds_it_to_the_namespace_it_was_read_in() {
    use crate::write::{Need, need_of};
    let (mut ra, mut rb) = ([0u8; 2], [0u8; 2]);
    let (read, here) = (any_word(&mut ra), any_word(&mut rb));
    let on_tag: bool = kani::any();
    let got = need_of(read, here, on_tag);
    let follows = match (read, &got) {
        (None, Need::Unknown) => true,
        (Some(r), Need::Bound(uri)) => here == Some(r) && uri == r,
        (Some(r), Need::Declare(uri)) => here != Some(r) && !on_tag && uri == r,
        (Some(r), Need::Unknown) => here != Some(r) && on_tag,
        _ => false,
    };
    assert!(follows);
    kani::cover!(matches!(got, Need::Declare(_)) & here.is_some(), "a prefix the place binds to another namespace, declared again");
}

/// The characters of `declared_by`'s harness: those of `xmlns`, the colon, and a letter of a prefix.
const DECLARATION_KINDS: &[u8; 7] = b"xmlns:p";

/// `declared_by` of every name of `N` characters of [`DECLARATION_KINDS`]: the default namespace (`""`) for `xmlns`; the prefix after `xmlns:` for a name that
/// begins with it, as the `.odg` reader reads a declaration (`read/names.rs`, `ns.rs`: **`xmlns:` with nothing after it is read as declaring the default namespace too**, which XML
/// Namespaces would not have, since a qualified name has a local part; the report of asks for a ruling, and the writer's callers refuse such a name as no qualified name
/// before they ask, but for the declarations of a base file's containers); and nothing for every other name.
fn declaration_law<const N: usize>() -> Option<usize> {
    use crate::write::declared_by;
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < DECLARATION_KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| DECLARATION_KINDS[usize::from(k)]);
    let name = std::str::from_utf8(&raw).unwrap_or("x");
    let got = declared_by(name);
    let want = if raw[..] == *b"xmlns" { Some(&b""[..]) } else { raw.strip_prefix(b"xmlns:") };
    assert!(got.map(str::as_bytes) == want);
    got.map(str::len)
}

macro_rules! declaration_law {
    ($($name:ident: $n:literal),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind(9)]
            fn $name() {
                let got = declaration_law::<$n>();
                kani::cover!(got.is_none(), "a name that declares nothing");
                kani::cover!(got.is_some(), "a declaration");
            }
        )*
    };
}

declaration_law! {
    a_name_of_five_characters_declares_the_default_namespace_exactly_when_it_is_xmlns: 5,
    a_name_of_six_characters_is_xmlns_and_its_colon_and_declares_the_default_namespace_or_is_no_declaration: 6,
    a_name_of_seven_characters_declares_a_prefix_of_one_character_or_nothing: 7,
}

/// `glue_box` (the box LibreOffice measures a shape's glue points in) for **every** kind of shape the reader tells apart and every `draw:kind` of up to four
/// characters of `ful` and another letter, or none: a frame's box for a custom shape, a rectangle and a frame (a text box, a picture) whatever the kind says; a whole ellipse's own box
/// for an ellipse and a circle exactly when the kind says none or `full`; and the shape's own frame for every other kind and every other ellipse.
#[kani::proof]
#[kani::unwind(6)]
fn a_shapes_glue_points_are_measured_in_the_box_of_its_kind() {
    use crate::read::{Boxed, Kindof, glue_box};
    const KIND_LETTERS: &[u8; 4] = b"fulx";
    let pick: u8 = kani::any();
    let kind = match pick {
        0 => Kindof::Rect,
        1 => Kindof::Ellipse,
        2 => Kindof::Circle,
        3 => Kindof::Line,
        4 => Kindof::Polyline,
        5 => Kindof::Polygon,
        6 => Kindof::Path,
        7 => Kindof::RegularPolygon,
        8 => Kindof::Frame,
        9 => Kindof::Custom,
        10 => Kindof::Connector,
        11 => Kindof::Group,
        12 => Kindof::Object("measure"),
        _ => Kindof::Kept("kept"),
    };
    let kinds: [u8; 4] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < KIND_LETTERS.len()));
    let raw: [u8; 4] = kinds.map(|k| KIND_LETTERS[usize::from(k)]);
    let len: usize = kani::any();
    kani::assume(len <= 4);
    let given: bool = kani::any();
    let word = raw.get(..len).unwrap_or(&[]);
    let ellipse_kind = given.then(|| std::str::from_utf8(word).unwrap_or("x"));
    let got = glue_box(kind, ellipse_kind);
    let whole = !given || word == b"full";
    let measured = match kind {
        Kindof::Custom | Kindof::Rect | Kindof::Frame => matches!(got, Boxed::Frame),
        Kindof::Ellipse | Kindof::Circle => matches!(got, Boxed::Ellipse) == whole && matches!(got, Boxed::Ellipse | Boxed::Own),
        _ => matches!(got, Boxed::Own),
    };
    assert!(measured);
    kani::cover!(matches!(kind, Kindof::Circle) & given & whole, "a circle whose kind says full");
    kani::cover!(matches!(kind, Kindof::Ellipse) & !whole, "an ellipse cut to a section, an arc or a cut");
}

/// `past_frame_point` (the rows `check_picture_glue` looks at) for **every** part a connection names, or none: a row of a frame's own glue point exactly for a part from
/// 104 up (the four below it, from 100, are the frame's four standard points; a part below 100 is no glue point), and then the part less 100. It never wraps.
#[kani::proof]
fn a_connection_names_a_frames_own_glue_point_exactly_from_part_one_hundred_and_four() {
    use crate::write::past_frame_point;
    let part: Option<u32> = kani::any();
    let got = past_frame_point(part);
    match part {
        Some(p) if p >= 104 => {
            assert!(got == Some(p - 100));
        }
        _ => {
            assert!(got.is_none());
        }
    }
    kani::cover!(got == Some(4), "the first glue point of the frame's own");
}

// ---: the law of the memory tests, what a read cost, a group's entry, the crossing of held text elements, the
// units and the route type of a shape as read ---

/// `read::cost::held`, `slot` and `priced` (what the law of the memory tests holds a kind to, and the items that pay for it a quarter above) for **every** size: an allocation
/// is its bytes, an eighth of them and 24; an entry of a list that doubles is twice its size and an eighth of that; and the items of 64 bytes that pay for a need are the least whose
/// bytes are at least the need and a quarter of it. Each exactly, the largest number where the sum passes it (they saturate and never wrap), and each monotone (a second size, beside
/// it). The quotients are by eight, four and sixty-four, which are shifts.
#[kani::proof]
fn the_law_holds_a_kind_to_its_bytes_an_eighth_and_twenty_four_and_the_items_that_pay_for_it_are_a_quarter_above() {
    use crate::read::ITEM_BYTES;
    use crate::read::cost::{held, priced, slot};
    let (n, m): (usize, usize) = (kani::any(), kani::any());
    let most = usize::MAX as u128;
    let wide = n as u128;
    assert!(held(n) as u128 == (wide + wide / 8 + 24).min(most));
    assert!(slot(n) as u128 == (2 * wide + 2 * wide / 8).min(most));
    let need = (wide + wide / 4).min(most);
    let paid = priced(n) as u128 * ITEM_BYTES as u128;
    assert!(ITEM_BYTES == 64 && paid >= need && paid < need + 64);
    if n <= m {
        assert!(held(n) <= held(m) && slot(n) <= slot(m) && priced(n) <= priced(m));
    }
    kani::cover!((paid == need) & (n > 0), "a need a quarter above which is whole items");
    kani::cover!((held(n) == usize::MAX) & (slot(n) == usize::MAX), "a size past what the law can say");
}

/// `index::ReadCost` (what the read that made a model cost, in the units of a save's estimate) for **every** count: the model is its items at [`crate::index::ITEM_BYTES`]
/// (64) and its pictures; what a read holds beside it is its tree at [`crate::index::TREE_NODE_BYTES`] an item and the parts it inflated; and a save's peak is the model twice and
/// the read beside it. Each exactly, the largest number where it passes it: nothing wraps to a small estimate.
#[kani::proof]
fn a_reads_cost_is_its_items_at_sixty_four_bytes_and_its_pictures_and_a_save_holds_it_twice_beside_the_read() {
    use crate::index::{ITEM_BYTES, ReadCost, TREE_NODE_BYTES};
    let cost = ReadCost { items: kani::any(), media_bytes: kani::any(), tree_nodes: kani::any(), inflated_bytes: kani::any(), ..ReadCost::default() };
    let most = u128::from(u64::MAX);
    let model = (u128::from(cost.items) * u128::from(ITEM_BYTES) + u128::from(cost.media_bytes)).min(most);
    let transient = (u128::from(cost.tree_nodes) * u128::from(TREE_NODE_BYTES) + u128::from(cost.inflated_bytes)).min(most);
    assert!(ITEM_BYTES == 64);
    assert!(u128::from(cost.model_bytes()) == model && u128::from(cost.transient_bytes()) == transient);
    assert!(u128::from(cost.save_bytes()) == (2 * model + transient).min(most));
    kani::cover!((model < most) & (2 * model + transient > most), "a save whose peak passes the largest number though its model does not");
}

/// `docfile::is_group_entry` (a group's own entry of the document file, and not the item of its VSDX data made beside it) for **every** name of seven characters of
/// `:groupx`: exactly the names that end in `:group`, whatever the prefix. (A name shorter than six is no group's, by its length.)
#[kani::proof]
#[kani::unwind(9)]
fn a_groups_entry_is_told_by_a_name_that_ends_in_colon_group() {
    use crate::ext::docfile::is_group_entry;
    const LETTERS: &[u8; 7] = b":groupx";
    let kinds: [u8; 7] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < LETTERS.len()));
    let raw: [u8; 7] = kinds.map(|k| LETTERS[usize::from(k)]);
    // `String::from_utf8` of the bytes, and not `from_utf8(&raw).unwrap_or("x").to_owned()`: a text that is one of two copied into a `String` is not the text under Kani 0.68
    // (a probe: the copy of the seven bytes differed from them where `from_utf8` was proved to succeed).
    let entry = El { name: String::from_utf8(raw.to_vec()).unwrap_or_default(), attrs: Vec::new(), children: Vec::new() };
    let got = is_group_entry(&entry);
    assert!(got == (raw[1..] == *b":group"));
    kani::cover!(got & (raw[0] == b'x'), "a prefix's group");
    kani::cover!(!got & (raw[2..] == *b"group"), "a name that ends in group with no colon before it");
    std::mem::forget(entry);
}

/// The characters of `field_text_ok`'s harness: a letter, a space, and a tab (which a line feed, a carriage return and U+2028 are alike to: white and no space).
const FIELD_KINDS: &[u8; 3] = b"a \t";

/// `runs::model::field_text_ok` (a field's shown text that the field's element can hold and the reader reads back as it is) for **every** text of `N` characters of a
/// letter, a space and a tab: exactly a text that is not empty, starts and ends with no white character, holds no white character but the space, and no two spaces in a row.
fn field_text_law<const N: usize>() -> bool {
    use crate::write::field_text_ok;
    let kinds: [u8; N] = kani::any();
    kani::assume(kinds.iter().all(|k| usize::from(*k) < FIELD_KINDS.len()));
    let raw: [u8; N] = kinds.map(|k| FIELD_KINDS[usize::from(k)]);
    let got = field_text_ok(std::str::from_utf8(&raw).unwrap_or("\t"));
    let white = |b: u8| b != b'a';
    let want = N > 0
        && !white(raw[0])
        && !white(raw[N - 1])
        && raw.iter().all(|b| *b != b'\t')
        && raw.windows(2).all(|pair| !(white(pair[0]) && white(pair[1])));
    assert!(got == want);
    got
}

#[kani::proof]
#[kani::unwind(7)]
fn a_fields_text_of_five_characters_is_held_by_its_element_exactly_when_its_white_is_single_spaces_inside_it() {
    let got = field_text_law::<5>();
    kani::cover!(got, "a text of words between single spaces");
    kani::cover!(!got, "a text the element would not read back");
}

/// `write::lengths::unit_of_suffix` and `unit_of_word` (the unit a held length was written in, and the one a cell's `U` names) for **every** text of one and of two
/// bytes: a suffix names a unit exactly when it is one of the six (`cm`, `mm`, `in`, `pt`, `pc`, `px`), which is then the unit's own suffix; a cell's word exactly when it is one of the
/// five (`CM`, `MM`, `IN`, `PT`, `P`), which is then the unit's own word (`px` has none). And each unit is read back from its own suffix and word.
#[kani::proof]
#[kani::unwind(8)]
fn a_length_unit_is_named_exactly_by_its_own_suffix_and_its_own_cell_word() {
    use crate::read::units::LenUnit;
    use crate::write::lengths::{unit_of_suffix, unit_of_word, unit_suffix};
    const ALL: [LenUnit; 6] = [LenUnit::Cm, LenUnit::Mm, LenUnit::In, LenUnit::Pt, LenUnit::Pc, LenUnit::Px];
    for unit in ALL {
        assert!(unit_of_suffix(unit_suffix(unit)) == Some(unit));
        assert!(unit.word().is_none_or(|word| unit_of_word(word) == Some(unit)));
    }
    let (one, two): ([u8; 1], [u8; 2]) = (kani::any(), kani::any());
    if let Ok(text) = std::str::from_utf8(&two) {
        let suffix = unit_of_suffix(text);
        assert!(suffix.is_some() == ["cm", "mm", "in", "pt", "pc", "px"].contains(&text));
        assert!(suffix.is_none_or(|unit| unit_suffix(unit) == text));
        let word = unit_of_word(text);
        assert!(word.is_some() == ["CM", "MM", "IN", "PT"].contains(&text));
        assert!(word.is_none_or(|unit| unit.word() == Some(text)));
        kani::cover!(suffix == Some(LenUnit::Px), "px");
    }
    if let Ok(text) = std::str::from_utf8(&one) {
        assert!(unit_of_suffix(text).is_none());
        assert!((unit_of_word(text) == Some(LenUnit::Pc)) == (text == "P") && (unit_of_word(text).is_some() == (text == "P")));
    }
}

/// `type_agrees` (whether a connector's `draw:type` as read is written back) for **every** two route cells, each a number or none, beside each type the reader tells
/// apart (none, `standard`, `line`, `lines`, `curve`, and one the standard does not define): it agrees exactly when the cells are the reader's for that type (`read::connectors::route_cells`:
/// 1 and 1 for none, `standard` and an unknown type, 2 and 1 for `line` and `lines`, 1 and 2 for `curve`), or when the file wrote no type and the cells say nothing. A cell that is no
/// number never agrees.
#[kani::proof]
#[kani::unwind(10)]
fn a_connectors_type_is_written_back_exactly_when_its_cells_are_what_the_reader_reads_from_it() {
    use crate::write::type_agrees;
    let (style, ext): (Option<f64>, Option<f64>) = (kani::any(), kani::any());
    let reads = |want: (f64, f64)| style == Some(want.0) && ext == Some(want.1);
    assert!(type_agrees(None, (style, ext)) == (reads((1.0, 1.0)) || (style.is_none() && ext.is_none())));
    assert!(type_agrees(Some("standard"), (style, ext)) == reads((1.0, 1.0)));
    assert!(type_agrees(Some("line"), (style, ext)) == reads((2.0, 1.0)));
    assert!(type_agrees(Some("lines"), (style, ext)) == reads((2.0, 1.0)));
    assert!(type_agrees(Some("curve"), (style, ext)) == reads((1.0, 2.0)));
    assert!(type_agrees(Some("polyline"), (style, ext)) == reads((1.0, 1.0)));
    if style.is_some_and(f64::is_nan) {
        assert!(!type_agrees(Some("curve"), (style, ext)) && !type_agrees(None, (style, ext)));
    }
    kani::cover!(type_agrees(Some("curve"), (style, ext)), "a curve");
}

/// `read::cost::text` (what the `.odg` reader charges a text of `n` bytes) for **every** length: **the codec's own charge** (`diacraft_sheetxml::model::cost::text_bytes`),
/// so the two readers of a drawing's text agree, nine eighths of the length rounded up, never under it, and monotone (a second length, beside it).
#[kani::proof]
fn the_reader_charges_a_text_as_the_codec_does_nine_eighths_of_its_length() {
    use crate::read::cost::text;
    let (n, more): (usize, usize) = (kani::any(), kani::any());
    assert!(text(n) as u64 == diacraft_sheetxml::model::cost::text_bytes(n));
    assert!(text(n) >= n);
    if n <= more {
        assert!(text(n) <= text(more));
    }
    kani::cover!((text(n) == usize::MAX) & (n < usize::MAX), "a length whose eighth passes the largest number");
}

/// `write::layers::NewLayer::display` (the `draw:display` a layer the save adds is written with) for **every** two flags of the layer, `Visible` and `Print`: no word (the
/// default, `always`) exactly when it is seen and prints, and a word otherwise; and **the reader takes what is written back to the same two flags** (`read::layers::display_flags`,
/// the default read as `always`), a round trip. The splice's new flags of a declared layer (`reflag_layers`) test the word a file has by the same rule.
#[kani::proof]
#[kani::unwind(9)]
fn a_layers_display_is_written_as_the_word_the_reader_takes_back_to_its_flags() {
    use crate::read::layers::display_flags;
    use crate::write::NewLayer;
    let (visible, print, protected): (bool, bool, bool) = (kani::any(), kani::any(), kani::any());
    let layer = NewLayer { name: String::new(), visible, print, protected };
    let word = layer.display();
    assert!(word.is_none() == (visible & print));
    assert!(display_flags(word.unwrap_or("always")) == (visible, print));
    assert!(display_flags("none") == (false, false));
    kani::cover!(word == Some("printer"), "a layer that prints and is not seen");
}

/// `ext::docfile::styleids::map_links` (a shape's links put through the `ID`s a document file restores to the style sheets, each a place of the file) for **every** three
/// links and `ID`s of any length up to four: a link that is a place of the file is the `ID` of the style at that place, any other number stays as it is, and no link stays no link.
#[kani::proof]
#[kani::unwind(5)]
fn a_shapes_link_to_a_place_of_the_file_is_the_id_of_the_style_there_and_any_other_stays() {
    use crate::ext::docfile::map_links;
    let all: [u32; 4] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 4);
    let ids = &all[..len];
    let links = diacraft_doc::Links { line: kani::any(), fill: kani::any(), text: kani::any() };
    let got = map_links(links, ids);
    let want = |link: Option<u32>| link.map(|p| if (p as usize) < len { all[p as usize] } else { p });
    assert!(got.line == want(links.line) && got.fill == want(links.fill) && got.text == want(links.text));
    kani::cover!((got.line != links.line) & (got.fill == links.fill) & links.fill.is_some(), "one link put through and one past the places");
}

/// A layer row's flags, `Visible`, `Print` and `Lock`.
type Flags3 = (bool, bool, bool);

/// `write::layers::deciding` (the row of a layer, among its first row on each page, that decides the flags it is written with) for **every** list of
/// up to four rows of any flags now and as read: the last row whose flags changed, and none exactly when no row changed.
#[kani::proof]
#[kani::unwind(6)]
fn the_row_of_a_layer_that_changed_last_decides_its_flags_and_none_when_none_changed() {
    use crate::write::deciding;
    let all: [(Flags3, Flags3); 4] = kani::any();
    let n: usize = kani::any();
    kani::assume(n <= 4);
    let rows = &all[..n];
    let got = deciding(rows);
    match got {
        Some(i) => {
            assert!(i < n && rows[i].0 != rows[i].1);
            for later in &rows[i + 1..] {
                assert!(later.0 == later.1);
            }
        }
        None => {
            for row in rows {
                assert!(row.0 == row.1);
            }
        }
    }
    kani::cover!(got.is_some_and(|i| i + 1 < n), "a changed row before an unchanged one");
    kani::cover!(got.is_none() & (n == 4), "four rows, none changed");
}

/// `write::model::layer_set::chosen_flags` (the flags a layer of the layer set is written with, `chosen`'s) for **every** three rows (the first row of the layer on
/// each of three pages) and every declaration, or none: a row of the three; with no declaration the first; with one, its own flags exactly when no row differs from them, else
/// the flags of the last row that differs, the page that changed them; and a visible, printed, unlocked layer for no row. (A symbolic count of rows up to three took 206 s:
/// `chosen` collects the rows into a list of that length.)
#[kani::proof]
#[kani::unwind(5)]
fn a_layer_is_declared_as_the_last_page_that_changed_it_says_and_as_the_first_page_with_no_declaration() {
    use crate::write::chosen_flags;
    let rows: [Flags3; 3] = kani::any();
    let was: Option<Flags3> = kani::any();
    assert!(chosen_flags(&[], was) == (true, true, false));
    let got = chosen_flags(&rows, was);
    let n = rows.len();
    assert!(rows.contains(&got));
    match was {
        None => {
            assert!(got == rows[0]);
        }
        Some(w) => {
            let mut last_differing = None;
            for (i, row) in rows.iter().enumerate() {
                if *row != w {
                    last_differing = Some(i);
                }
            }
            match last_differing {
                None => {
                    assert!(got == w);
                }
                Some(i) => {
                    assert!(got == rows[i] && got != w);
                }
            }
        }
    }
    let differs = was.is_some_and(|w| got != w);
    kani::cover!(differs & (n == 3), "a page of three changed the layer");
    kani::cover!(was.is_some_and(|w| got == w) & (n == 3), "no page changed it");
}

/// `write::runs::written_size` (the font size the writer writes for a `Size` of inches) for **every** number: positive, never a NaN, and the size itself
/// exactly when it is positive (0, a size below 0 and no number are written as the least size, a tenth of a point).
#[kani::proof]
fn a_font_size_is_written_positive_and_as_itself_exactly_when_it_is_positive() {
    use crate::write::runs::{LEAST_FONT_SIZE, written_size};
    let inches: f64 = kani::any();
    let w = written_size(inches);
    assert!(w > 0.0);
    assert!((w == inches) == (inches > 0.0));
    assert!(inches > 0.0 || w == LEAST_FONT_SIZE);
    kani::cover!(inches == 0.0, "a size of 0");
    kani::cover!(inches.is_nan(), "no number");
}

/// A byte of a length by its kind: a space, a tab, a minus, a digit, a point or a letter.
fn length_byte(kind: u8, digit: u8) -> u8 {
    match kind % 6 {
        0 => b' ',
        1 => b'\t',
        2 => b'-',
        3 => b'0' + digit % 10,
        4 => b'.',
        _ => b'c',
    }
}

/// The law of `ext::odf14::negative` for every text of `N` bytes by their kinds: a length is negative exactly when, blanks aside at both ends, it starts with a minus and then
/// a digit or a point. → what it said, whether a blank came first, and whether the text, blanks aside, starts with a minus.
fn negative_law<const N: usize>() -> (bool, bool, bool) {
    let (kinds, digits): ([u8; N], [u8; N]) = (kani::any(), kani::any());
    let mut bytes = [0u8; N];
    for i in 0..N {
        bytes[i] = length_byte(kinds[i], digits[i]);
    }
    let text = std::str::from_utf8(&bytes).unwrap_or("");
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let start = bytes.iter().position(|b| !blank(b)).unwrap_or(N);
    let end = bytes.iter().rposition(|b| !blank(b)).map_or(start, |e| e + 1);
    let s = &bytes[start..end];
    let want = s.len() >= 2 && s[0] == b'-' && (s[1].is_ascii_digit() || s[1] == b'.');
    let got = crate::ext::odf14::negative(text);
    assert!(got == want);
    (got, start > 0, s.first() == Some(&b'-'))
}

/// `ext::odf14::negative` (a label width below 0, which ODF 1.4 refuses and the writer brings to 0) for every text of two bytes by their kinds.
#[kani::proof]
#[kani::unwind(4)]
fn a_length_of_two_bytes_is_negative_exactly_when_it_is_a_minus_and_a_digit_or_a_point() {
    let (got, _, minus) = negative_law::<2>();
    kani::cover!(got, "a negative length");
    kani::cover!(!got & minus, "a minus with no digit after it");
}

/// The same for every text of three bytes by their kinds.
#[kani::proof]
#[kani::unwind(5)]
fn a_length_of_three_bytes_is_negative_exactly_when_it_starts_with_a_minus_and_a_digit_or_a_point_blanks_aside() {
    let (got, blank_first, minus) = negative_law::<3>();
    kani::cover!(got & blank_first, "a negative length after a blank");
    kani::cover!(!got & minus, "a minus with no digit after it");
}

/// The same for every text of four bytes by their kinds.
#[kani::proof]
#[kani::unwind(6)]
fn a_length_of_four_bytes_is_negative_exactly_when_it_starts_with_a_minus_and_a_digit_or_a_point_blanks_aside() {
    let (got, blank_first, minus) = negative_law::<4>();
    kani::cover!(got & blank_first, "a negative length after a blank");
    kani::cover!(!got & minus, "a minus with no digit after it");
}

/// `write::fx::hatch_form` and `stipple_share` (a fill pattern written as a hatch's lines, or as the share of the marks in a stipple's one colour) for **every** byte: at
/// most one of them says the pattern, one exactly for each of 2 to 24 and neither for 0, 1 and past 24; a hatch's lines are single or double and run at 0, 45, 90 or 135 degrees,
/// and a stipple's share is more than none and less than all.
#[kani::proof]
#[kani::unwind(8)]
fn a_fill_pattern_is_a_hatch_or_a_stipple_never_both_and_one_of_them_from_two_to_twenty_four() {
    use crate::write::fx::{hatch_form, stipple_share};
    let p: u8 = kani::any();
    let (hatch, stipple) = (hatch_form(p), stipple_share(p));
    assert!(!(hatch.is_some() && stipple.is_some()));
    assert!((hatch.is_some() || stipple.is_some()) == (2..=24).contains(&p));
    if let Some((style, rotation, _)) = hatch {
        assert!(matches!(rotation, 0 | 45 | 90 | 135));
        assert!(style == "single" || style == "double");
    }
    if let Some(share) = stipple {
        assert!(share > 0.0 && share < 1.0);
    }
    kani::cover!(hatch.is_some() & (p == 23), "the last hatch");
    kani::cover!(stipple.is_some() & (p == 24), "the last stipple");
}

/// `write::fx::mixture` (the one colour a stipple is written in, the marks' share of each channel blended straight) for **every** two colours and every share from none to
/// all: each channel between the ground's and the marks', and never past the 24 bits of a colour for any finite share (a stipple's is a constant of the table; an infinite one
/// would make `0 * inf` of a channel the two colours share, which Kani's NaN check stops at).
#[kani::proof]
#[kani::unwind(5)]
fn a_stipples_colour_has_each_channel_between_the_grounds_and_the_marks() {
    use crate::write::fx::mixture;
    let (ground, marks): (u32, u32) = (kani::any(), kani::any());
    let share: f64 = kani::any();
    kani::assume(share.is_finite());
    let got = mixture(ground, marks, share);
    assert!(got <= 0x00ff_ffff);
    if (0.0..=1.0).contains(&share) {
        for shift in [16u32, 8, 0] {
            let (a, b, c) = ((ground >> shift) & 255, (marks >> shift) & 255, (got >> shift) & 255);
            assert!(a.min(b) <= c && c <= a.max(b));
        }
    }
    kani::cover!(((got >> 8) & 255) != ((ground >> 8) & 255), "a channel moved toward the marks");
}

/// The same `mixture` at the two ends: a share of none is the ground and a share of all the marks, for **every** two colours.
#[kani::proof]
fn a_stipple_of_no_marks_is_its_ground_and_of_all_marks_its_marks() {
    use crate::write::fx::mixture;
    let (ground, marks): (u32, u32) = (kani::any(), kani::any());
    assert!(mixture(ground, marks, 0.0) == ground & 0x00ff_ffff);
    assert!(mixture(ground, marks, 1.0) == marks & 0x00ff_ffff);
}

/// `write::fx::odf_angle_tenths` (a `FillGradientAngle` in radians as `draw:angle` says it, tenths of a degree) for **every** number: below a whole turn, 3600 tenths, and 0
/// for no number and an infinity. (Through `rem_euclid`, the float remainder Kani over-approximates, so no exact angle is asked of it here; its unit tests hold the angles.)
#[kani::proof]
fn a_gradients_angle_in_the_file_is_below_a_turn_and_none_for_no_number() {
    use crate::write::fx::odf_angle_tenths;
    let r: f64 = kani::any();
    let t = odf_angle_tenths(r);
    assert!(t < 3600);
    if !r.is_finite() {
        assert!(t == 0);
    }
    kani::cover!(t > 1800, "an angle past half a turn");
}

/// `GradientKey::lo_scale` against `GradientKey::layout` (how much sooner the file's gradient reaches its last colour) for **every** `FillGradientDir` byte: twice
/// for a layout whose centre is a corner, the root of 5/4 over the root of 1/2 for one whose centre is the middle of an edge, and once for one from the centre, a linear one and
/// a number ODF has no layout for.
#[kani::proof]
fn a_gradients_stops_are_spread_by_where_its_centre_is() {
    use crate::write::fx::{GradientKey, MOST_STOPS, Stop};
    let dir: u8 = kani::any();
    let key = GradientKey { dir, angle: 0, n: 2, stops: [Stop::default(); MOST_STOPS] };
    let scale = key.lo_scale();
    match key.layout() {
        None => {
            assert!(dir > 13 && scale == 1.0);
        }
        Some((_, None)) => {
            assert!(dir == 0 && scale == 1.0);
        }
        Some((_, Some((cx, cy)))) => {
            let corner = (cx == 0 || cx == 100) && (cy == 0 || cy == 100);
            let edge = cx == 50 && (cy == 0 || cy == 100);
            if corner {
                assert!(scale == 2.0);
            } else if edge {
                assert!(scale == 1.581_138_83);
            } else {
                assert!(cx == 50 && cy == 50 && scale == 1.0);
            }
        }
    }
    kani::cover!(scale == 1.581_138_83, "the middle of an edge");
}

/// `read::comments::Sink` (the text of a comment measured against the room it may take, `measuring`'s) for **three** steps in a row, each a character of any size or
/// any number of spaces, in any room: `room` is exactly whether the bytes so far and the step's fit, with no overflow; a step is taken exactly then and adds its bytes; a refused one
/// makes nothing; so the length never passes the room.
#[kani::proof]
fn a_comments_text_is_measured_within_its_room_and_a_step_past_it_makes_nothing() {
    use crate::read::comments::Sink;
    let left: usize = kani::any();
    let mut sink = Sink::measuring(left);
    let mut taken = 0u32;
    for _ in 0..3 {
        let before = sink.len;
        let (spaces, n, c): (bool, usize, char) = (kani::any(), kani::any(), kani::any());
        let size = if spaces { n } else { c.len_utf8() };
        let fits = before as u128 + size as u128 <= left as u128;
        assert!(sink.room(size) == fits);
        let got = if spaces { sink.spaces(n) } else { sink.char(c) };
        assert!(got.is_some() == fits);
        assert!(sink.len == if fits { before + size } else { before });
        assert!(sink.len <= left);
        taken += u32::from(fits);
    }
    kani::cover!(taken == 3, "three steps taken");
    kani::cover!((taken == 2) & (sink.len == left), "a step that fills the room, and one refused");
}

/// `write::comments::counts_after` (`places_after`'s arithmetic, the places of a page's annotations after a save) for **every** count of annotations, taken out and
/// written: the first place of those written is the count kept, all of them less those taken out (none when more are taken out than there are), and the count after the save is
/// it and those written, held at `u32::MAX`; so the places run in order from the kept to the count, each below it.
#[kani::proof]
fn the_annotations_after_a_save_are_those_kept_and_then_those_written() {
    use crate::write::counts_after;
    let (total, removed, wrote): (u32, usize, usize) = (kani::any(), kani::any(), kani::any());
    let (kept, after) = counts_after(total, removed, wrote);
    assert!(kept <= total && kept <= after);
    if removed <= total as usize {
        assert!(kept as usize == total as usize - removed);
    } else {
        assert!(kept == 0);
    }
    if kept as u128 + wrote as u128 <= u128::from(u32::MAX) {
        assert!(after as u128 == kept as u128 + wrote as u128);
    } else {
        assert!(after == u32::MAX);
    }
    kani::cover!((kept > 0) & (after > kept), "kept and written");
    kani::cover!(after == u32::MAX, "held at the largest");
}
