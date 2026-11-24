//! Small TrueType fonts built in code, for tests (the `test-support` feature; never part of the app).
//!
//! Tests of layout need faces whose numbers are known exactly: a letter 600 units wide, a face with no bold, a face that lacks a
//! Han character, an `OS/2` table that says what a raised letter does, a `kern` table to prove nothing kerns. No shipped font
//! gives that, and no test may depend on what happens to be installed. A [`SynthFont`] builds the bytes of such a face: box
//! glyphs for outlines, a cmap, advances, names, `OS/2`, `post` and optionally `kern`.

/// One glyph: the character it is for, its advance in font units, and whether it has an outline (a space doesn't).
#[derive(Clone, Debug)]
pub struct GlyphSpec {
    pub ch: char,
    pub advance: u16,
    pub outline: bool,
}

/// A `ySubscript...` or `ySuperscript...` set of `OS/2` fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptFields {
    pub x_size: i16,
    pub y_size: i16,
    pub x_offset: i16,
    pub y_offset: i16,
}

/// The description of a test face. Start with [`SynthFont::new`] and chain.
#[derive(Clone, Debug)]
pub struct SynthFont {
    pub family: String,
    pub style: String,
    pub bold: bool,
    pub italic: bool,
    pub units_per_em: u16,
    pub notdef_advance: u16,
    pub ascent: i16,
    pub descent: i16,
    pub glyphs: Vec<GlyphSpec>,
    pub subscript: ScriptFields,
    pub superscript: ScriptFields,
    /// `post` `underlinePosition` and `underlineThickness`.
    pub underline: (i16, i16),
    /// `OS/2` `yStrikeoutSize` and `yStrikeoutPosition`.
    pub strikeout: (i16, i16),
    /// Pairs of characters and a kerning value, in a legacy `kern` table.
    pub kern: Vec<(char, char, i16)>,
    /// `OS/2` `usWeightClass`, when it should differ from the 400 or 700 the bold flag gives (a face whose name says Bold and whose
    /// table says 400).
    pub weight: Option<u16>,
    /// Whether the slant bits of `OS/2` and `head` are set, when that should differ from `italic`.
    pub slant_bits: Option<bool>,
    /// `OS/2` `ulUnicodeRange1` to `4`.
    pub unicode_ranges: [u32; 4],
    /// Leave out `glyf` and `loca`, as a font without TrueType outlines is.
    pub omit_outlines: bool,
    /// Dummy tables to add, which make the font directory long.
    pub extra_tables: usize,
    /// Zero bytes added to the end of the `name` table, which make it long (its strings can't be: they are addressed by 16 bits).
    pub name_padding: usize,
    /// Two-letter ligatures `liga` makes: the letters, and the advance of the ligature glyph. The ligature has no character of its own.
    pub ligatures: Vec<(char, char, u16)>,
    /// The feature the ligatures are made by: `liga` unless this says otherwise (`clig`, `calt`).
    pub ligature_feature: [u8; 4],
    /// A legacy family name (name ID 1) different from the family, which is then the typographic one (name ID 16), as a family of
    /// many weights has.
    pub legacy_family: Option<String>,
    /// `OS/2` `fsType`, the embedding licence bits: 0 (installable) unless a test says otherwise (2 is restricted licence).
    pub fs_type: u16,
}

impl SynthFont {
    /// A face named `family`, 1000 units to the em, with no glyphs but `.notdef` (600 wide).
    pub fn new(family: &str) -> SynthFont {
        SynthFont {
            family: family.to_owned(),
            style: "Regular".to_owned(),
            bold: false,
            italic: false,
            units_per_em: 1000,
            notdef_advance: 600,
            ascent: 800,
            descent: -200,
            glyphs: Vec::new(),
            subscript: ScriptFields { x_size: 650, y_size: 650, x_offset: 0, y_offset: 140 },
            superscript: ScriptFields { x_size: 650, y_size: 650, x_offset: 0, y_offset: 450 },
            underline: (-100, 50),
            strikeout: (50, 300),
            kern: Vec::new(),
            weight: None,
            slant_bits: None,
            unicode_ranges: [0; 4],
            omit_outlines: false,
            extra_tables: 0,
            name_padding: 0,
            ligatures: Vec::new(),
            ligature_feature: *b"liga",
            legacy_family: None,
            fs_type: 0,
        }
    }

    /// Name the style, such as `Bold`, setting the bold and italic bits from the words in it.
    pub fn styled(mut self, style: &str) -> SynthFont {
        self.style = style.to_owned();
        self.bold = style.contains("Bold");
        self.italic = style.contains("Italic");
        self
    }

    /// Add a glyph for each character of `chars` with the same advance. A space gets no outline.
    pub fn chars(mut self, chars: &str, advance: u16) -> SynthFont {
        for ch in chars.chars() {
            self.glyphs.retain(|g| g.ch != ch);
            self.glyphs.push(GlyphSpec { ch, advance, outline: !ch.is_whitespace() });
        }
        self
    }

    /// Add the printable ASCII characters with one advance.
    pub fn ascii(self, advance: u16) -> SynthFont {
        let all: String = (0x20u8..0x7F).map(char::from).collect();
        self.chars(&all, advance)
    }

    /// Set one character's advance (adding it if it isn't there).
    pub fn advance(mut self, ch: char, advance: u16) -> SynthFont {
        self.glyphs.retain(|g| g.ch != ch);
        self.glyphs.push(GlyphSpec { ch, advance, outline: !ch.is_whitespace() });
        self
    }

    /// The glyph id the face gives `ch`, or 0: ids follow the order of [`SynthFont::glyphs`] from 1, sorted by character.
    pub fn gid(&self, ch: char) -> u16 {
        let mut chars: Vec<char> = self.glyphs.iter().map(|g| g.ch).collect();
        chars.sort_unstable();
        chars.iter().position(|c| *c == ch).map_or(0, |i| i as u16 + 1)
    }

    /// The bytes of the font.
    pub fn build(&self) -> Vec<u8> {
        let mut glyphs = self.glyphs.clone();
        glyphs.sort_by_key(|g| g.ch);
        glyphs.dedup_by_key(|g| g.ch);
        let notdef = GlyphSpec { ch: '\0', advance: self.notdef_advance, outline: true };
        let ligature_glyphs: Vec<GlyphSpec> =
            self.ligatures.iter().map(|(_, _, advance)| GlyphSpec { ch: '\0', advance: *advance, outline: true }).collect();
        let all: Vec<&GlyphSpec> = std::iter::once(&notdef).chain(glyphs.iter()).chain(ligature_glyphs.iter()).collect();
        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"OS/2", self.os2(&glyphs)),
            (*b"cmap", cmap(&glyphs)),
            (*b"head", self.head()),
            (*b"hhea", self.hhea(&all)),
            (*b"hmtx", hmtx(&all)),
            (*b"maxp", maxp(all.len())),
            (*b"name", self.name()),
            (*b"post", self.post()),
        ];
        if !self.omit_outlines {
            let (glyf, loca) = self.glyf_and_loca(&all);
            tables.push((*b"glyf", glyf));
            tables.push((*b"loca", loca));
        }
        if !self.kern.is_empty() {
            tables.push((*b"kern", self.kern_table(&glyphs)));
        }
        if !self.ligatures.is_empty() {
            tables.push((*b"GSUB", self.gsub(&glyphs)));
        }
        for i in 0..self.extra_tables {
            tables.push((format!("X{i:03}").into_bytes().try_into().unwrap_or(*b"X000"), vec![0; 4]));
        }
        assemble(tables)
    }

    /// A `GSUB` table with one `liga` lookup: for each ligature, the two letters become its glyph.
    fn gsub(&self, glyphs: &[GlyphSpec]) -> Vec<u8> {
        let gid = |c: char| glyphs.iter().position(|g| g.ch == c).map_or(0, |i| i as u16 + 1);
        // Ligature sets keyed by the first letter's glyph, in glyph order (a coverage table is sorted).
        let mut sets: Vec<(u16, Vec<(u16, u16)>)> = Vec::new();
        for (k, (first, second, _)) in self.ligatures.iter().enumerate() {
            let (f, ligature) = (gid(*first), glyphs.len() as u16 + 1 + k as u16);
            match sets.iter_mut().find(|(g, _)| *g == f) {
                Some((_, list)) => list.push((gid(*second), ligature)),
                None => sets.push((f, vec![(gid(*second), ligature)])),
            }
        }
        sets.sort_by_key(|(g, _)| *g);
        let u16s = |v: &[u16]| -> Vec<u8> { v.iter().flat_map(|x| x.to_be_bytes()).collect() };
        // LigatureSubst format 1: coverage, then a set of ligatures for each covered glyph.
        let mut set_tables: Vec<Vec<u8>> = Vec::new();
        for (_, list) in &sets {
            let ligs: Vec<Vec<u8>> = list.iter().map(|(second, ligature)| u16s(&[*ligature, 2, *second])).collect();
            let mut t = u16s(&[list.len() as u16]);
            let mut offset = 2 + 2 * ligs.len();
            for l in &ligs {
                t.extend(u16s(&[offset as u16]));
                offset += l.len();
            }
            ligs.iter().for_each(|l| t.extend_from_slice(l));
            set_tables.push(t);
        }
        let coverage: Vec<u8> = u16s(&[1, sets.len() as u16].iter().copied().chain(sets.iter().map(|(g, _)| *g)).collect::<Vec<u16>>());
        let header = 6 + 2 * sets.len();
        let mut sub = u16s(&[1, (header) as u16, sets.len() as u16]);
        let mut offset = header + coverage.len();
        for t in &set_tables {
            sub.extend(u16s(&[offset as u16]));
            offset += t.len();
        }
        sub.extend(coverage);
        set_tables.iter().for_each(|t| sub.extend_from_slice(t));
        // Lookup: type 4, flags 0, one subtable right after the header.
        let lookup = [u16s(&[4, 0, 1, 8]), sub].concat();
        let lookup_list = [u16s(&[1, 4]), lookup].concat();
        let feature = u16s(&[0, 1, 0]);
        let feature_list = [u16s(&[1]), self.ligature_feature.to_vec(), u16s(&[8]), feature].concat();
        let lang_sys = u16s(&[0, 0xFFFF, 1, 0]);
        let script = [u16s(&[4, 0]), lang_sys].concat();
        let script_list = [u16s(&[1]), b"DFLT".to_vec(), u16s(&[8]), script].concat();
        let mut t = u16s(&[1, 0, 10, (10 + script_list.len()) as u16, (10 + script_list.len() + feature_list.len()) as u16]);
        t.extend(script_list);
        t.extend(feature_list);
        t.extend(lookup_list);
        t
    }

    fn glyf_and_loca(&self, all: &[&GlyphSpec]) -> (Vec<u8>, Vec<u8>) {
        let (mut glyf, mut loca) = (Vec::new(), Vec::new());
        for g in all {
            loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
            if g.outline {
                // A box of three quarters the advance wide and seven tenths of an em high.
                let w = (u32::from(g.advance) * 3 / 4) as i16;
                let h = (u32::from(self.units_per_em) * 7 / 10) as i16;
                glyf.extend(box_glyph(w, h));
            }
        }
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        (glyf, loca)
    }

    fn head(&self) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        t.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        t.extend_from_slice(&3u16.to_be_bytes());
        t.extend_from_slice(&self.units_per_em.to_be_bytes());
        t.extend_from_slice(&[0; 16]);
        for v in [0i16, -200, 1000, 800] {
            t.extend_from_slice(&v.to_be_bytes());
        }
        let mac_style = u16::from(self.bold) | (u16::from(self.slant_bits.unwrap_or(self.italic)) << 1);
        t.extend_from_slice(&mac_style.to_be_bytes());
        t.extend_from_slice(&8u16.to_be_bytes());
        t.extend_from_slice(&2i16.to_be_bytes());
        t.extend_from_slice(&1i16.to_be_bytes());
        t.extend_from_slice(&0i16.to_be_bytes());
        t
    }

    fn hhea(&self, all: &[&GlyphSpec]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        t.extend_from_slice(&self.ascent.to_be_bytes());
        t.extend_from_slice(&self.descent.to_be_bytes());
        t.extend_from_slice(&0i16.to_be_bytes());
        let widest = all.iter().map(|g| g.advance).max().unwrap_or(0);
        t.extend_from_slice(&widest.to_be_bytes());
        for v in [0i16, 0, widest as i16, 1, 0, 0] {
            t.extend_from_slice(&v.to_be_bytes());
        }
        t.extend_from_slice(&[0; 8]);
        t.extend_from_slice(&0i16.to_be_bytes());
        t.extend_from_slice(&(all.len() as u16).to_be_bytes());
        t
    }

    fn os2(&self, glyphs: &[GlyphSpec]) -> Vec<u8> {
        let mut t = Vec::new();
        let push16 = |t: &mut Vec<u8>, v: i16| t.extend_from_slice(&v.to_be_bytes());
        t.extend_from_slice(&4u16.to_be_bytes());
        push16(&mut t, 500);
        let weight: u16 = self.weight.unwrap_or(if self.bold { 700 } else { 400 });
        t.extend_from_slice(&weight.to_be_bytes());
        t.extend_from_slice(&5u16.to_be_bytes());
        t.extend_from_slice(&self.fs_type.to_be_bytes());
        for f in [self.subscript, self.superscript] {
            for v in [f.x_size, f.y_size, f.x_offset, f.y_offset] {
                push16(&mut t, v);
            }
        }
        push16(&mut t, self.strikeout.0);
        push16(&mut t, self.strikeout.1);
        push16(&mut t, 0);
        t.extend_from_slice(&[0; 10]);
        self.unicode_ranges.iter().for_each(|r| t.extend_from_slice(&r.to_be_bytes()));
        t.extend_from_slice(b"SYNT");
        let selection: u16 =
            (u16::from(self.slant_bits.unwrap_or(self.italic))) | (u16::from(self.bold) << 5) | (u16::from(!self.bold && !self.italic) << 6);
        t.extend_from_slice(&selection.to_be_bytes());
        let first = glyphs.first().map_or(0, |g| g.ch as u32).min(0xFFFF) as u16;
        let last = glyphs.last().map_or(0, |g| g.ch as u32).min(0xFFFF) as u16;
        t.extend_from_slice(&first.to_be_bytes());
        t.extend_from_slice(&last.to_be_bytes());
        push16(&mut t, self.ascent);
        push16(&mut t, self.descent);
        push16(&mut t, 0);
        t.extend_from_slice(&(self.ascent.max(0) as u16).to_be_bytes());
        t.extend_from_slice(&(self.descent.unsigned_abs()).to_be_bytes());
        t.extend_from_slice(&[0; 8]);
        push16(&mut t, 500);
        push16(&mut t, 700);
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&0x20u16.to_be_bytes());
        t.extend_from_slice(&1u16.to_be_bytes());
        t
    }

    fn post(&self) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&0x0003_0000u32.to_be_bytes());
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(&self.underline.0.to_be_bytes());
        t.extend_from_slice(&self.underline.1.to_be_bytes());
        t.extend_from_slice(&[0; 20]);
        t
    }

    fn name(&self) -> Vec<u8> {
        let full = format!("{} {}", self.family, self.style);
        let dashed = full.replace(' ', "-");
        // Name records go in order of their IDs. A face with a legacy family has it as ID 1 and the family as ID 16.
        let mut strings: Vec<(u16, &str)> =
            vec![(1, self.legacy_family.as_deref().unwrap_or(&self.family)), (2, &self.style), (4, &full), (6, &dashed)];
        if self.legacy_family.is_some() {
            strings.push((16, &self.family));
        }
        let encoded: Vec<Vec<u8>> = strings.iter().map(|(_, s)| s.encode_utf16().flat_map(u16::to_be_bytes).collect()).collect();
        let mut t = Vec::new();
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        t.extend_from_slice(&((6 + 12 * strings.len()) as u16).to_be_bytes());
        let mut offset = 0u16;
        for ((id, _), bytes) in strings.iter().zip(&encoded) {
            for v in [3u16, 1, 0x0409, *id, bytes.len() as u16, offset] {
                t.extend_from_slice(&v.to_be_bytes());
            }
            offset += bytes.len() as u16;
        }
        encoded.iter().for_each(|b| t.extend_from_slice(b));
        t.resize(t.len() + self.name_padding, 0);
        t
    }

    fn kern_table(&self, glyphs: &[GlyphSpec]) -> Vec<u8> {
        let gid = |c: char| glyphs.iter().position(|g| g.ch == c).map_or(0, |i| i as u16 + 1);
        let mut pairs: Vec<(u16, u16, i16)> = self.kern.iter().map(|(a, b, v)| (gid(*a), gid(*b), *v)).collect();
        pairs.sort_unstable();
        let mut sub = Vec::new();
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&((14 + 6 * pairs.len()) as u16).to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes());
        sub.extend_from_slice(&(pairs.len() as u16).to_be_bytes());
        sub.extend_from_slice(&[0; 6]);
        for (l, r, v) in pairs {
            for x in [l.to_be_bytes(), r.to_be_bytes(), v.to_be_bytes()] {
                sub.extend_from_slice(&x);
            }
        }
        let mut t = vec![0, 0, 0, 1];
        t.extend(sub);
        t
    }
}

/// A box contour from the origin to (`w`, `h`) as a simple glyph: four on-curve points with 16-bit deltas.
fn box_glyph(w: i16, h: i16) -> Vec<u8> {
    let mut g = Vec::new();
    g.extend_from_slice(&1i16.to_be_bytes());
    for v in [0, 0, w, h] {
        g.extend_from_slice(&v.to_be_bytes());
    }
    g.extend_from_slice(&3u16.to_be_bytes());
    g.extend_from_slice(&0u16.to_be_bytes());
    g.extend_from_slice(&[1, 1, 1, 1]);
    for dx in [0i16, 0, w, 0] {
        g.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, h, 0, -h] {
        g.extend_from_slice(&dy.to_be_bytes());
    }
    g.resize(g.len().div_ceil(4) * 4, 0);
    g
}

fn hmtx(all: &[&GlyphSpec]) -> Vec<u8> {
    all.iter().flat_map(|g| g.advance.to_be_bytes().into_iter().chain([0, 0])).collect()
}

fn maxp(glyph_count: usize) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    t.extend_from_slice(&(glyph_count as u16).to_be_bytes());
    for v in [4u16, 1, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        t.extend_from_slice(&v.to_be_bytes());
    }
    t
}

/// A format 12 cmap, platform 3 encoding 10, one group a character.
fn cmap(glyphs: &[GlyphSpec]) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(&0u16.to_be_bytes());
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&3u16.to_be_bytes());
    t.extend_from_slice(&10u16.to_be_bytes());
    t.extend_from_slice(&12u32.to_be_bytes());
    t.extend_from_slice(&12u16.to_be_bytes());
    t.extend_from_slice(&0u16.to_be_bytes());
    t.extend_from_slice(&((16 + 12 * glyphs.len()) as u32).to_be_bytes());
    t.extend_from_slice(&0u32.to_be_bytes());
    t.extend_from_slice(&(glyphs.len() as u32).to_be_bytes());
    for (i, g) in glyphs.iter().enumerate() {
        for v in [g.ch as u32, g.ch as u32, i as u32 + 1] {
            t.extend_from_slice(&v.to_be_bytes());
        }
    }
    t
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut word = [0u8; 4];
        word[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// The font file for these tables, sorted by tag, each padded to four bytes.
fn assemble(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let n = tables.len() as u16;
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= u32::from(n) {
        entry_selector += 1;
    }
    let search_range = 16 * (1u16 << entry_selector);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for v in [n, search_range, entry_selector, n * 16 - search_range] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}
