//! A regression found by the 0.2.4 gate: a span of a text style that a second shape shares comes back from a visit through `.diacraft` with **every property its style stated**, and not
//! with a new automatic style that lost the Asian and complex sizes and the Asian weight and states the defaults (`font-style` normal, `font-variant` normal, `text-underline-style` none) it never had.
//!
//! The cause: the first span of a shape's text is on row 0 of its `Character` section, which holds the cells that differ from the shape's own, and a later one is on a row that holds every cell its chain
//! resolves. A drawing whose default style states the face of the complex script gives the later row a face cell, and with it a held value that says the span's style stated none of the face, which
//! the first row has no cell for. The writer took the two uses of one style for two contents, and wrote the second from the cells alone (a language on the span gave it a row of its own, which is
//! how a language made the bug show).
//!
//! The drawings are built in code from the ODF 1.4 Part 3 vocabulary (the default style §16.2, the text properties §16.29: the three scripts' sizes, weights and slants, the faces, the language and
//! country). Each is an ODF 1.2 package and a flat drawing. The text is invented.

use std::collections::{BTreeMap, HashMap};

use diacraft_container::xml::{self, El};
use proptest::prelude::*;

use crate::drawing::Drawing;
use crate::flat::Flat;
use crate::visit_styles::{part, visit};

/// The default graphic style: it states one property, the face of the complex script.
const DEFAULT_COMPLEX_FACE: &str =
    r#"<style:default-style style:family="graphic"><style:text-properties style:font-family-complex="Skiff Naskh"/></style:default-style>"#;

/// The text properties of the first span: a size in the three scripts, and bold in all three.
const S1: &str = concat!(
    r#"fo:font-size="14pt" style:font-size-asian="14pt" style:font-size-complex="14pt" "#,
    r#"fo:font-weight="bold" style:font-weight-asian="bold" style:font-weight-complex="bold""#
);

/// The text properties of the second span: the first's, and a language and a country.
const S2: &str = concat!(
    r#"fo:font-size="14pt" style:font-size-asian="14pt" style:font-size-complex="14pt" "#,
    r#"fo:font-weight="bold" style:font-weight-asian="bold" style:font-weight-complex="bold" "#,
    r#"fo:language="en" fo:country="US""#
);

fn text_style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties {attrs}/></style:style>"#)
}

fn span(style: &str, text: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{text}</text:span>"#)
}

/// A shape `kind` (`rect` or `line`) at `y` cm, with the paragraph `inner`.
fn shape(kind: &str, y: u32, inner: &str) -> String {
    let geometry = if kind == "line" {
        format!(r#"svg:x1="2cm" svg:y1="{y}cm" svg:x2="8cm" svg:y2="{y}cm""#)
    } else {
        format!(r#"svg:x="2cm" svg:y="{y}cm" svg:width="6cm" svg:height="2cm""#)
    };
    format!(r#"<draw:{kind} draw:layer="layout" {geometry}><text:p>{inner}</text:p></draw:{kind}>"#)
}

/// The vector (ODF 1.2): the default style that states the complex face, the text styles `T1` (S1) and `T2` (S2), shape A with S1 and then S2 in its paragraph, shape B with S2 alone.
fn vector(kind: &str) -> Drawing {
    let a = shape(kind, 3, &format!("{}{}", span("T1", "Ferry "), span("T2", "dock")));
    let b = shape(kind, 7, &span("T2", "pier"));
    let mut d = Drawing::a4().common(DEFAULT_COMPLEX_FACE).auto(&text_style("T1", S1)).auto(&text_style("T2", S2)).page(&format!("{a}{b}"));
    d.version = "1.2".to_owned();
    d
}

/// The properties of the style `name` of the family `family` in a part's root (its text properties' attributes), with the parent's under them.
fn style_props(roots: &[&El], family: &str, name: &str, depth: usize) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    if depth > 16 {
        return props;
    }
    for root in roots {
        let mut stack: Vec<&El> = vec![root];
        while let Some(el) = stack.pop() {
            for child in el.elements() {
                if child.name == "style:style" && child.attr("style:name") == Some(name) && child.attr("style:family") == Some(family) {
                    if let Some(parent) = child.attr("style:parent-style-name") {
                        props = style_props(roots, family, parent, depth + 1);
                    }
                    for tp in child.children_named("text-properties") {
                        for (k, v) in &tp.attrs {
                            props.insert(k.clone(), v.clone());
                        }
                    }
                    return props;
                }
                stack.push(child);
            }
        }
    }
    props
}

/// Each span of `written` with its text, the name of the style it names and that style's properties, in the order of the file.
fn spans_of(written: &[u8]) -> Vec<(String, Option<String>, BTreeMap<String, String>)> {
    let content = part(written, "content.xml");
    let styles = part(written, "styles.xml");
    let content = xml::parse(content.as_bytes()).unwrap();
    let styles = xml::parse(styles.as_bytes()).unwrap();
    let roots = [&content, &styles];
    let mut stack: Vec<&El> = vec![&content];
    let mut found: Vec<&El> = Vec::new();
    while let Some(el) = stack.pop() {
        if el.name == "text:span" {
            found.push(el);
        }
        for child in el.elements() {
            stack.push(child);
        }
    }
    found.reverse();
    let mut out = Vec::new();
    for sp in found {
        let name = sp.attr("text:style-name").map(str::to_owned);
        let props = name.as_deref().map(|n| style_props(&roots, "text", n, 0)).unwrap_or_default();
        out.push((sp.text(), name, props));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// What is wrong with the way `written` writes the spans: each of the two spans of S2 ("dock" and "pier") names a style that states every property of S2, and **the same style**: the file's
/// `T2` was one style, and the visit adds none a drawing program would list.
fn problems(what: &str, written: &[u8]) -> Vec<String> {
    let spans = spans_of(written);
    let want: BTreeMap<String, String> =
        S2.split("\" ").map(|kv| kv.trim_end_matches('"')).filter_map(|kv| kv.split_once("=\"")).map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    let mut found = Vec::new();
    let mut names = Vec::new();
    for text in ["dock", "pier"] {
        let Some((_, name, props)) = spans.iter().find(|s| s.0 == text) else {
            found.push(format!("{what}: no span of {text}: {spans:?}"));
            continue;
        };
        names.push(name.clone());
        for (k, v) in &want {
            if props.get(k) != Some(v) {
                found.push(format!("{what}: the span of {text} (style {name:?}) has {k} = {:?}, not {v}: {props:?}", props.get(k)));
            }
        }
    }
    if names.len() == 2 && names[0] != names[1] {
        found.push(format!("{what}: the two spans of one style name two: {names:?}"));
    }
    found
}

/// **Red on 898db0ef**: the package, shape B's span names a new automatic style that lost the Asian and complex sizes and the Asian weight.
#[test]
fn a_package_visit_keeps_every_property_of_the_second_span_that_shares_a_style() {
    let mut found = Vec::new();
    for kind in ["line", "rect"] {
        let (written, _) = visit(&vector(kind).bytes()).unwrap();
        found.extend(problems(&format!("package, {kind}"), &written));
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// A flat drawing is read as the package that holds the same elements, so its visit is the same.
#[test]
fn a_flat_visit_keeps_every_property_of_the_second_span_that_shares_a_style() {
    let mut found = Vec::new();
    for kind in ["line", "rect"] {
        let (written, _) = visit(&Flat::of(&vector(kind)).bytes()).unwrap();
        found.extend(problems(&format!("flat, {kind}"), &written));
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// The shape with the span of S2 alone first, and shape A second: the style is made by the span on the first row, and the one on a later row is the second to ask.
#[test]
fn the_order_of_the_two_shapes_is_no_matter() {
    for kind in ["line", "rect"] {
        let b = shape(kind, 3, &span("T2", "pier"));
        let a = shape(kind, 7, &format!("{}{}", span("T1", "Ferry "), span("T2", "dock")));
        let mut d = Drawing::a4().common(DEFAULT_COMPLEX_FACE).auto(&text_style("T1", S1)).auto(&text_style("T2", S2)).page(&format!("{b}{a}"));
        d.version = "1.2".to_owned();
        let (written, _) = visit(&d.bytes()).unwrap();
        let found = problems(&format!("package, {kind}, B first"), &written);
        assert!(found.is_empty(), "{found:#?}");
        let (written, _) = visit(&Flat::of(&d).bytes()).unwrap();
        let found = problems(&format!("flat, {kind}, B first"), &written);
        assert!(found.is_empty(), "{found:#?}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ the property

/// The properties a character of a text is drawn with: the text properties of its chain, each attribute by name.
type Props = BTreeMap<String, String>;

/// The styles and the faces of a package's two parts, by family and name.
struct Index<'a> {
    styles: HashMap<(String, String), &'a El>,
    default_graphic: Option<&'a El>,
    /// The family each declared face names.
    faces: HashMap<String, String>,
}

impl<'a> Index<'a> {
    fn of(parts: [&'a El; 2]) -> Index<'a> {
        let mut index = Index { styles: HashMap::new(), default_graphic: None, faces: HashMap::new() };
        for root in parts {
            for container in root.elements() {
                match container.local() {
                    "font-face-decls" => {
                        for face in container.children_named("font-face") {
                            if let (Some(name), Some(family)) = (face.attr("style:name"), face.attr("svg:font-family")) {
                                index.faces.insert(name.to_owned(), family.trim_matches('\'').to_owned());
                            }
                        }
                    }
                    "styles" | "automatic-styles" => {
                        for style in container.elements() {
                            if style.is("default-style") && style.attr("style:family") == Some("graphic") {
                                index.default_graphic = Some(style);
                            } else if style.is("style")
                                && let (Some(family), Some(name)) = (style.attr("style:family"), style.attr("style:name"))
                            {
                                index.styles.insert((family.to_owned(), name.to_owned()), style);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        index
    }

    /// The text properties of the style `name` of `family`, the parent's under its own.
    fn chain(&self, family: &str, name: Option<&str>, into: &mut Props) {
        let mut chain: Vec<&El> = Vec::new();
        let mut at = name.map(str::to_owned);
        while let Some(current) = at.take() {
            let Some(style) = self.styles.get(&(family.to_owned(), current)) else { break };
            chain.push(style);
            at = style.attr("style:parent-style-name").map(str::to_owned);
            if chain.len() > 16 {
                break;
            }
        }
        for style in chain.into_iter().rev() {
            for tp in style.children_named("text-properties") {
                for (k, v) in &tp.attrs {
                    into.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// The values the standard gives a property no style states: a property said at one of them is no more than a property not said, once the chain is resolved.
const INITIALS: [(&str, &str); 10] = [
    ("fo:font-style", "normal"),
    ("style:font-style-asian", "normal"),
    ("style:font-style-complex", "normal"),
    ("fo:font-variant", "normal"),
    ("fo:font-weight", "normal"),
    ("style:font-weight-asian", "normal"),
    ("style:font-weight-complex", "normal"),
    ("style:text-underline-style", "none"),
    ("style:text-line-through-style", "none"),
    ("style:text-overline-style", "none"),
];

/// A chain's properties as the drawing shows them: a face named by a declaration is the family the declaration says, and a property at the value the standard gives it is no property.
fn shown(mut props: Props, index: &Index<'_>) -> Props {
    for (name, family) in [
        ("style:font-name", "fo:font-family"),
        ("style:font-name-asian", "style:font-family-asian"),
        ("style:font-name-complex", "style:font-family-complex"),
    ] {
        if let Some(face) = props.remove(name) {
            let family_name = index.faces.get(&face).cloned().unwrap_or(face);
            props.insert(family.to_owned(), family_name);
        }
    }
    props.retain(|k, v| !INITIALS.iter().any(|(name, initial)| k == name && v == initial));
    props
}

/// The attributes of the three triplets of a language (the language, the country, the script and the tag), by script: Latin, Asian, complex.
const LANGUAGE: [[&str; 4]; 3] = [
    ["fo:language", "fo:country", "fo:script", "style:rfc-language-tag"],
    ["style:language-asian", "style:country-asian", "style:script-asian", "style:rfc-language-tag-asian"],
    ["style:language-complex", "style:country-complex", "style:script-complex", "style:rfc-language-tag-complex"],
];

/// The script a letter is in: 0 Latin, 1 Asian (the CJK block), 2 complex (the Arabic block).
fn script_of(c: char) -> usize {
    match c {
        '\u{0600}'..='\u{06FF}' => 2,
        '\u{3000}'..='\u{9FFF}' => 1,
        _ => 0,
    }
}

/// The properties a letter of `script` is drawn with: **the language of a letter is the one its script's triplet says**, so the other two triplets are not properties of it. A language stated in a
/// triplet the letters of the span are not in (`fo:language` on a span of Asian letters) is drawn as no language, and the visit may leave it out (the variant 0.2.5 takes, not this task's).
fn of_script(props: &Props, script: usize) -> Props {
    let mut kept = props.clone();
    for (other, names) in LANGUAGE.iter().enumerate() {
        if other != script {
            for name in names {
                kept.remove(*name);
            }
        }
    }
    kept
}

/// Appends `text` drawn with `props` to `runs`, cut where the script of the letters changes, and joined to the run before it when it is drawn as it is.
fn push_runs(runs: &mut Vec<(String, Props)>, text: &str, props: &Props) {
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        let script = script_of(first);
        let mut run = String::from(first);
        while let Some(next) = chars.next_if(|c| script_of(*c) == script) {
            run.push(next);
        }
        let props = of_script(props, script);
        match runs.last_mut() {
            Some((last, same)) if *same == props => last.push_str(&run),
            _ => runs.push((run, props)),
        }
    }
}

/// The text of a shape as it is drawn: each paragraph as its runs, a run being the letters of one script that are drawn with the same properties.
fn drawn(bytes: &[u8]) -> Vec<Vec<(String, Props)>> {
    let content = part(bytes, "content.xml");
    let styles = part(bytes, "styles.xml");
    let content = xml::parse(content.as_bytes()).unwrap();
    let styles = xml::parse(styles.as_bytes()).unwrap();
    let index = Index::of([&styles, &content]);
    let mut shapes: Vec<&El> = Vec::new();
    let mut stack: Vec<&El> = vec![&content];
    while let Some(el) = stack.pop() {
        if el.children_named("p").next().is_some() {
            shapes.push(el);
        }
        stack.extend(el.elements());
    }
    // The stack gave them last first.
    shapes.reverse();
    let mut out = Vec::new();
    for shape in shapes {
        let mut under = Props::new();
        if let Some(default) = index.default_graphic {
            for tp in default.children_named("text-properties") {
                under.extend(tp.attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        index.chain("graphic", shape.attr("draw:style-name"), &mut under);
        for paragraph in shape.children_named("p") {
            let mut base = under.clone();
            index.chain("paragraph", paragraph.attr("text:style-name"), &mut base);
            let mut runs: Vec<(String, Props)> = Vec::new();
            for node in &paragraph.children {
                let (text, props) = match node {
                    xml::Node::Text(t) => (t.clone(), shown(base.clone(), &index)),
                    xml::Node::El(el) if el.is("span") => {
                        let mut props = base.clone();
                        index.chain("text", el.attr("text:style-name"), &mut props);
                        (el.text(), shown(props, &index))
                    }
                    _ => continue,
                };
                push_runs(&mut runs, &text, &props);
            }
            out.push(runs);
        }
    }
    out
}

/// The attributes a group of properties says, in one of its two values. Each group is a property of one script (or two attributes that say one language), so that a span that states some of them
/// and a default style or a shape that states others make the chains the property test visits.
const GROUPS: [&[&[(&str, &str)]]; 16] = [
    &[&[("fo:font-size", "14pt")], &[("fo:font-size", "18pt")]],
    &[&[("style:font-size-asian", "14pt")], &[("style:font-size-asian", "12pt")]],
    &[&[("style:font-size-complex", "14pt")], &[("style:font-size-complex", "12pt")]],
    &[&[("fo:font-weight", "bold")], &[("fo:font-weight", "normal")]],
    &[&[("style:font-weight-asian", "bold")], &[("style:font-weight-asian", "normal")]],
    &[&[("style:font-weight-complex", "bold")], &[("style:font-weight-complex", "normal")]],
    &[&[("fo:font-style", "italic")], &[("fo:font-style", "normal")]],
    &[&[("style:font-style-asian", "italic")], &[("style:font-style-asian", "normal")]],
    &[&[("style:font-style-complex", "italic")], &[("style:font-style-complex", "normal")]],
    &[&[("fo:language", "en"), ("fo:country", "US")], &[("fo:language", "de"), ("fo:country", "DE")]],
    &[&[("style:language-asian", "ja"), ("style:country-asian", "JP")], &[("style:language-asian", "zh"), ("style:country-asian", "CN")]],
    &[&[("style:language-complex", "ar"), ("style:country-complex", "SA")], &[("style:language-complex", "he"), ("style:country-complex", "IL")]],
    &[&[("fo:color", "#336699")], &[("fo:color", "#cc3300")]],
    &[&[("style:font-family-complex", "Skiff Naskh")], &[("style:font-family-complex", "Quay Kufi")]],
    &[&[("style:font-family-asian", "Moor Gothic")], &[("style:font-family-asian", "Pier Mincho")]],
    &[&[("style:text-underline-style", "solid"), ("style:text-underline-type", "single")], &[("style:text-underline-style", "none")]],
];

/// The groups a default graphic style may state: the properties of the scripts a run's row has no cell of its own for, and a size, a weight and a slant of the Latin script.
const DEFAULT_GROUPS: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14];

/// The attribute list a set of (group, value) choices says: a group chosen twice is said once, by the later choice.
fn attributes(choices: &[(usize, usize)], allowed: &[usize]) -> String {
    let mut said: BTreeMap<usize, usize> = BTreeMap::new();
    for (group, value) in choices {
        if let Some(group) = allowed.get(group % allowed.len()) {
            said.insert(*group, *value % 2);
        }
    }
    let mut text = String::new();
    for (group, value) in said {
        for (name, value) in GROUPS.get(group).and_then(|g| g.get(value)).copied().unwrap_or_default() {
            text.push_str(&format!(r#"{name}="{value}" "#));
        }
    }
    text.trim_end().to_owned()
}

/// One item of a paragraph: bare text, or a span of the text style `style`.
#[derive(Clone, Debug)]
struct Item {
    style: Option<usize>,
    word: usize,
    script: usize,
}

/// Invented words of the three scripts: Latin, Asian and complex.
const WORDS: [[&str; 3]; 3] = [["ferry", "dock", "pier"], ["港町", "桟橋", "船着場"], ["مرفأ", "رصيف", "عبّارة"]];

impl Item {
    fn markup(&self, styles: usize) -> String {
        let text = WORDS.get(self.script % 3).and_then(|w| w.get(self.word % 3)).copied().unwrap_or("pier");
        match self.style {
            Some(at) => span(&format!("T{}", at % styles + 1), text),
            None => text.to_owned(),
        }
    }
}

/// A drawing the property test visits.
#[derive(Clone, Debug)]
struct Case {
    default: Vec<(usize, usize)>,
    styles: Vec<Vec<(usize, usize)>>,
    /// Each shape: whether it is a line, and its paragraphs.
    shapes: Vec<(bool, Vec<Vec<Item>>)>,
}

fn choices() -> impl Strategy<Value = Vec<(usize, usize)>> {
    prop::collection::vec((0usize..16, 0usize..2), 0..6)
}

fn item() -> impl Strategy<Value = Item> {
    (prop::option::weighted(0.8, 0usize..4), 0usize..3, prop_oneof![8 => Just(0usize), 1 => Just(1usize), 1 => Just(2usize)])
        .prop_map(|(style, word, script)| Item { style, word, script })
}

fn case() -> impl Strategy<Value = Case> {
    (
        choices(),
        prop::collection::vec(choices(), 1..5),
        prop::collection::vec((any::<bool>(), prop::collection::vec(prop::collection::vec(item(), 1..5), 1..3)), 2..5),
    )
        .prop_map(|(default, styles, shapes)| Case { default, styles, shapes })
}

impl Case {
    fn drawing(&self) -> Drawing {
        let default = attributes(&self.default, &DEFAULT_GROUPS);
        let mut d = Drawing::a4();
        if !default.is_empty() {
            d = d.common(&format!(r#"<style:default-style style:family="graphic"><style:text-properties {default}/></style:default-style>"#));
        }
        for (at, choice) in self.styles.iter().enumerate() {
            let attrs = attributes(choice, &(0..16).collect::<Vec<_>>());
            if !attrs.is_empty() {
                d = d.auto(&text_style(&format!("T{}", at + 1), &attrs));
            } else {
                d = d.auto(&format!(r#"<style:style style:name="T{}" style:family="text"/>"#, at + 1));
            }
        }
        let mut shapes = String::new();
        for (n, (line, paragraphs)) in self.shapes.iter().enumerate() {
            let y = u32::try_from(n).unwrap_or(0) * 3 + 2;
            let kind = if *line { "line" } else { "rect" };
            let geometry = if *line {
                format!(r#"svg:x1="2cm" svg:y1="{y}cm" svg:x2="8cm" svg:y2="{y}cm""#)
            } else {
                format!(r#"svg:x="2cm" svg:y="{y}cm" svg:width="6cm" svg:height="2cm""#)
            };
            let paragraphs: String = paragraphs
                .iter()
                .map(|items| format!("<text:p>{}</text:p>", items.iter().map(|i| i.markup(self.styles.len())).collect::<String>()))
                .collect();
            shapes.push_str(&format!(r#"<draw:{kind} draw:layer="layout" {geometry}>{paragraphs}</draw:{kind}>"#));
        }
        let mut d = d.page(&shapes);
        d.version = "1.2".to_owned();
        d
    }
}

/// **An unedited visit keeps what each run is drawn with**: the properties of every character, through its span, its paragraph, its shape and the default style, are those the file gave it, for
/// spans that state a size, a weight, a slant, a face and a language of each of the three scripts, shared by two shapes or more, in a package and in a flat drawing.
fn the_visit_draws_every_run_as_the_file_did(case: &Case) -> Result<(), TestCaseError> {
    let d = case.drawing();
    let source = drawn(&d.bytes());
    for (route, bytes) in [("package", d.bytes()), ("flat", Flat::of(&d).bytes())] {
        let (written, _) = visit(&bytes).map_err(|e| TestCaseError::fail(format!("{route}: {e}")))?;
        let back = drawn(&written);
        prop_assert_eq!(&back, &source, "{} route\nsource: {}\nwritten: {}", route, part(&d.bytes(), "content.xml"), part(&written, "content.xml"));
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 192, ..ProptestConfig::default() })]

    #[test]
    fn an_unedited_visit_keeps_the_properties_of_every_run_of_shapes_that_share_styles(case in case()) {
        the_visit_draws_every_run_as_the_file_did(&case)?;
    }
}
