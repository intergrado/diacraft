# The `.diacraft` file format

This document specifies `.diacraft`, DiaCraft's own file format, version 1.1, as DiaCraft 0.2 writes it. Version 1.1 adds one thing to 1.0, in 3.19, and raises one limit, in 6.1 (the XML items of a part); a file that uses neither is a file of 1.0. It is written so that another program can read and write the format from this text alone. It says how a file is laid out (section 2), what every element and attribute of the model means (section 3), what "the same content" means when a drawing goes through the format and back to a VSDX or OpenDocument file (section 4), how versions work (section 5), which limits a reader may apply (section 6), where DiaCraft puts its data inside VSDX and OpenDocument files (section 7), and gives small files with the models they stand for (section 8).

Parts of this document are marked **informative**: they explain, and no program is bound by them. Everything else is normative. The words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are used as RFC 2119 and RFC 8174 use them, when they are in capitals.

## 1. Scope and conformance

### 1.1 What a `.diacraft` file is

A `.diacraft` file is a ZIP package. It holds one of three things, its *kind* (1.2): a drawing, a set of stencil shapes, or a template. The content is a **model** written as XML:

- pages, each holding shapes in drawing order, and masters, which are reusable shapes;
- for every page, master and shape, a **sheet**: named **cells** (with a formula, a cached value and a unit each), **sections** and **rows**, as the VSDX format has them. The cells and the formula language mean what the VSDX format says they mean; this document does not repeat that;
- styles, themes, a colour table, a font table, document properties, and pictures and other byte strings, each stored once.

The model is a superset of what a VSDX file and an OpenDocument drawing hold. What either format says that the sheet cannot say exactly is kept in the file as **held data**, tagged with the format it belongs to, beside the cells it stands for (3.13 to 3.15). So a drawing that was read from a VSDX file or an OpenDocument drawing can be written back to that format with the same content (section 4). A `.diacraft` never contains the file it came from.

A `.diacraft` does not hold the undo history, the selection, or the view of a window (zoom, scroll, panels), with one exception: the window settings of a VSDX file, which are content and are held (3.14). There is no flat (single-XML) form of the format.

### 1.2 Kinds, extensions and media types

| Kind | Extension | Media type |
|---|---|---|
| `drawing` | `.diacraft` | `application/vnd.diacraft.drawing` |
| `stencil` | `.diacraft-stencil` | `application/vnd.diacraft.stencil` |
| `template` | `.diacraft-template` | `application/vnd.diacraft.template` |

The three media types are DiaCraft's own, in the `vnd` tree. **They are not registered with IANA.** The kinds share one structure. A stencil holds masters and needs no pages. A template is a drawing meant to be copied: a program that opens one is expected to start a new, unnamed drawing from it. The extension is a hint; the bytes decide (2.3).

### 1.3 Conformance

A **conforming file** is a package that meets every requirement of this document that is addressed to files. A **conforming reader** is a program that reads conforming files of this version of the format, and meets every requirement addressed to readers. A **conforming writer** is a program that writes only conforming files. A program that reads a file, changes it and writes it again is a *round-tripping program*, and meets the requirements of both.

The requirements that matter most, which later sections state in full:

- A reader MUST accept every conforming file of major version 1, whatever its minor version (section 5).
- A reader MUST refuse, in words, a file of a major version it does not know, and a file that is damaged (2.10). It MUST NOT guess.
- A reader MUST treat every byte of a file as hostile: it applies limits before it allocates, and it refuses what is too large in words (section 6). It never aborts.
- A round-tripping program MUST keep what it does not understand: unknown parts, and unknown attributes and elements inside the model (section 5).
- No program MAY run, open or parse as a package the bytes of a macro project or of an embedded object that a file carries. They are byte strings, and are copied as such (3.14).

### 1.4 Terms

| Term | Meaning |
|---|---|
| Package | The ZIP file. |
| Entry | One file inside the package. |
| Part | An entry that holds XML of the model: the document part, a page part, or a master part (3.2). The word *package part* in 3.14 is another use: a part of a foreign package that the model holds as bytes. |
| Blob | A byte string the model holds (a picture, a theme, an embedded object, a macro project, a font, ...), stored once as an entry (3.16). |
| Kept data | Attributes and nodes the model has no field for, written in a data form (3.12). |
| Held data | Data of the VSDX format or of the OpenDocument format that sits beside the cells it stands for (3.13 to 3.15). |
| Stamp | A 64-bit fingerprint written as 16 hexadecimal digits (3.17). |
| Content stamp | The stamp of a whole model, taken when a drawing is read from a VSDX or OpenDocument file (2.7). |
| Carrier | The place in a VSDX file or an OpenDocument drawing where DiaCraft puts its own data (section 7). |
| Foreign format, foreign file | The VSDX formats and OpenDocument Graphics (`.odg`, `.otg`, `.fodg`), and a file in one of them. |

### 1.5 References

Normative:

- RFC 2119 and RFC 8174, key words for requirements: https://www.rfc-editor.org/rfc/rfc2119 and https://www.rfc-editor.org/rfc/rfc8174.
- ZIP: the PKWARE .ZIP File Format Specification, APPNOTE 6.3.9: https://pkware.cachefly.net/webdocs/APPNOTE/APPNOTE-6.3.9.TXT.
- XML 1.0, Fifth Edition (W3C): https://www.w3.org/TR/xml/. Namespaces in XML 1.0 (W3C): https://www.w3.org/TR/xml-names/.
- RFC 4648, base64: https://www.rfc-editor.org/rfc/rfc4648.
- ECMA-376 Part 2, Open Packaging Conventions (5th edition, December 2021): https://ecma-international.org/publications-and-standards/standards/ecma-376/. Used for part names, content types and relationships of VSDX packages.
- ECMA-376 Part 3, Markup Compatibility and Extensibility (5th edition, December 2015): https://ecma-international.org/publications-and-standards/standards/ecma-376/. Used for the attributes of 4.4 that name prefixes.
- OpenDocument Format 1.3 (OASIS): Part 2, Packages: https://docs.oasis-open.org/office/OpenDocument/v1.3/os/part2-packages/OpenDocument-v1.3-os-part2-packages.html; Part 3, Schema: https://docs.oasis-open.org/office/OpenDocument/v1.3/os/part3-schema/OpenDocument-v1.3-os-part3-schema.html; and the schema, https://docs.oasis-open.org/office/OpenDocument/v1.3/os/schemas/OpenDocument-v1.3-schema.rng.
- The published specification of the VSDX file format, for the meaning of cells, sections, rows, shape types and formulas. This document says how they are stored and does not define them.

Informative: the FNV hash (Fowler, Noll and Vo), http://www.isthe.com/chongo/tech/comp/fnv/, which 3.17 uses.

## 2. The package

### 2.1 The ZIP container

The package is a ZIP file as the PKWARE specification defines it.

- Every entry MUST be stored (method 0) or deflated (method 8). Encryption MUST NOT be used. ZIP64 and multi-disk archives MUST NOT be used.
- Entry names are UTF-8. They are relative names with `/` separators. A name MUST NOT be empty, MUST NOT start with `/`, MUST NOT contain `\`, an empty segment or a segment `.` or `..`, and MUST NOT be longer than 1,024 bytes. No two entries have the same name (names are compared byte for byte).
- A writer writes no entry for a folder, no data descriptor, no extra field and no entry or archive comment. It gives every entry the date 1980-01-01 and the time 00:00:00, the earliest a ZIP file can state, and sets the UTF-8 flag (bit 11) on an entry whose name is not ASCII.
- A reader ignores an entry for a folder (a name that ends in `/` and has no data). It refuses a file that breaks the other rules above. It never trusts a size, a count or a CRC the archive states: it checks them against the data it reads.

### 2.2 The entries

| Entry | Role | What it holds | How it is stored |
|---|---|---|---|
| `mimetype` | none | The media type of the file, in ASCII, with no line end (2.3) | First entry, stored, no extra field |
| `manifest.xml` | none | The format version, the kind, and every other entry with its role (2.4) | Deflated |
| `document.xml` | `document` | The document part: version, title, properties, colours, fonts, the document sheet, styles, themes, held data, package parts, and the lists naming the page and master parts and the blobs (3.3) | Deflated |
| `pages/page<ID>.xml` | `page` | One page: its fields, sheet, shapes and connections (3.4) | Deflated |
| `masters/master<ID>.xml` | `master` | One master, with its shapes | Deflated |
| `blobs/<n>.<ext>` | `blob` | One byte string, stored once (2.6) | Stored for pictures and packages that are already compressed, and for a blob that deflate would make a thousand times smaller and more (6.1); deflated otherwise |
| `state.xml` | `state` | The identifier ledger and the content stamp (2.7) | Deflated |
| `preview.png` | `preview` | A small picture of the first page, for file managers (2.8) | Stored |
| any other name | any other word | A part a later minor version wrote (section 5) | As it was |

`mimetype`, `manifest.xml` and `document.xml` are required. A drawing has a page part for each page its document part names, and a master part for each master it names. A writer writes `state.xml`; a reader that finds none uses an empty state: no ledger, so every mark is 0, and no content stamp, so the stamp is unknown (*a rule of this format*). `preview.png` is optional.

### 2.3 The `mimetype` entry, and recognising a file

The first entry of the package is named `mimetype`. It is stored, it has no extra field, and its data is the media type of the file's kind (1.2) in ASCII with no line end: `application/vnd.diacraft.drawing`, `application/vnd.diacraft.stencil` or `application/vnd.diacraft.template`.

So the type is at fixed places in every file (the same arrangement that OpenDocument Part 2, 3.3, describes for its own `mimetype`): the bytes `PK` at position 0, the string `mimetype` at position 30, and the media type from position 38. A file manager's magic rule can match it there.

- A reader recognises a `.diacraft` file when its first entry is named `mimetype`, is stored, has no extra field, and its data starts with `application/vnd.diacraft.`. A file whose `mimetype` is compressed, has an extra field, is not the first entry, or names another type is not a `.diacraft` file, whatever its extension, and a program asked to open it as one refuses it in words. The extension only gives a warning: a file that has these bytes is opened whatever it is called, and a program may say that its extension is not the kind's.
- A data descriptor on the `mimetype` entry (flag bit 3: the checksum and the two sizes follow its data, and the local header holds 0 for them) does not stop a program from recognising the file. The length of the media type is then not in the local header, and a reader takes as the media type of the file the type of a kind that stands at position 38 and is followed by a descriptor that states its length for both sizes, with or without the descriptor's signature. *A rule of this format.*
- The kind named by `mimetype` MUST equal the `kind` of the manifest. A file in which they differ is damaged.
- A `.diacraft` is neither an Open Packaging Conventions package (it has no `[Content_Types].xml`) nor an OpenDocument package (it has no `META-INF/manifest.xml`, and its media type is not an OpenDocument type). Programs that read those formats do not claim it.

### 2.4 The manifest

`manifest.xml` is an XML 1.0 document in UTF-8. Its root is `manifest`, in the namespace of 3.1. It lists every entry of the package other than `mimetype` and `manifest.xml`, once each, in the order of the entries.

| Element | Attribute | Required | Default | Meaning |
|---|---|---|---|---|
| `manifest` | `format` | yes | none | The format version as `major.minor`, two decimal numbers separated by a dot: `1.1` for this version (a writer that uses nothing of 3.19 and writes no part of more than 6,000,000 XML items MAY write `1.0`). |
| `manifest` | `kind` | yes | none | `drawing`, `stencil` or `template`. |
| `manifest` | `written-by` | no | absent | Free text naming the program that wrote the file, such as `DiaCraft 0.2.0`. For people; no program may depend on it. |
| `part` | `path` | yes | none | The entry's name. |
| `part` | `role` | yes | none | What the entry is for: `document`, `page`, `master`, `blob`, `state`, `preview`, or any other word (an extra part, section 5). |
| `part` | `required` | no | `0` | A flag (3.1). `1` means that a reader that does not know the role MUST refuse the file. A writer of this version writes `required="1"` on the entries of role `document`, `page` and `master`, and on no other. |
| `part` | `media-type` | no | absent | The media type of the entry's bytes. Written for blobs and extra parts. |

Rules:

- Every entry other than `mimetype` and `manifest.xml` is listed exactly once, and every listed entry exists. An entry that the manifest does not list, a listed entry that is missing and an entry listed twice make the file damaged. The one exception is a listed entry of role `blob` that is missing: that is a lost blob, and not damage (2.10).
- The manifest's `part` and `manifest` elements are the manifest's own. They are not the `part` and `manifest` of the model vocabulary (3.14), which stand in `document.xml`.
- `format` is parsed as two non-negative whole numbers of at most nineteen decimal digits each. A value that is not two numbers with a dot between them makes the manifest damaged. Section 5 says what a reader does with the number.
- A role word is compared byte for byte.
- The row of role `document` names `document.xml`, the row of role `state` names `state.xml` and the row of role `preview` names `preview.png`, and each of the three stands once: a file in which another entry has one of these roles, in which one of them stands twice, or in which an entry named `document.xml`, `state.xml` or `preview.png` has another role, is damaged. A page or master entry that no list of the document part names is damaged too, and so is a part that the lists name more than once, in one list or in both (the lists are exact, 3.3). *These are rules of this format.*

### 2.5 Names of entries

- `pages/page<ID>.xml` holds the page whose `id` is `<ID>`, in decimal. If the same `id` is used by more than one page (a hostile or damaged drawing can), the k-th later page takes `pages/page<ID>-<k>.xml`, for k = 1, 2, and so on. Masters are named in the same way, `masters/master<ID>.xml`.
- A reader MUST take the name of each page and master part from the lists of the document part (3.3), and never work it out from an `id`.
- A blob is named `blobs/<n>.<ext>`, where `<n>` is its number (3.16) in decimal and `<ext>` follows from its media type: `png` for `image/png`; `jpg` for `image/jpeg` and `image/jpg`; `gif`; `bmp`; `tif` for `image/tiff`; `webp`; `svg` for `image/svg+xml`; `emf` for `image/x-emf` and `image/emf`; `wmf` for `image/x-wmf` and `image/wmf`; `pdf` for `application/pdf`; `zip` for `application/zip`; `xml` for `application/xml`, `text/xml` and any type that ends in `+xml` (other than `image/svg+xml`); `bin` for every other type. The media type is read in ASCII lower case with any parameter after a `;` dropped. The extension is a convenience: a reader finds a blob by the name its record gives (3.16), not by the extension.

### 2.6 Blobs in the package

Every byte string the model holds is stored once, as an entry of role `blob`. The model names it by number (3.16). A byte string that many shapes show, such as a picture, is stored once.

- A blob is **stored** (method 0) when its first bytes are a PNG signature, a JPEG signature, a GIF signature or `PK\x03\x04`, which are formats that are compressed already. Any other blob is **deflated**, except that a writer **stores** (method 0) a blob that deflate would make a thousand times smaller and more, since a reader refuses an entry whose inflated size is more than 1,000 times its stored size (6.1); a reader reads a blob either way.
- A reader MUST NOT parse a blob, open one as a package, or run it. A blob that holds an embedded package, an embedded object or a macro project is copied as bytes and nothing else.
- A blob entry that no record in the document part names is ignored for the model. When the minor version of the file is higher than the reader's, the reader holds the entry as an extra part (5.2), and a writer writes it back at its own path with its manifest row. Otherwise the reader drops it and tells the person in words. A writer of this version writes no such entry. An entry held as an extra part in this way whose checksum fails is a lost blob (2.10), not damage: it is left out, and the person is told. *Rules of this format.*

### 2.7 `state.xml`

`state.xml` is an XML 1.0 document in UTF-8. Its root is `state`, in the namespace of 3.1. It holds bookkeeping of the program that wrote the file, not part of the drawing's content.

| Element | Attribute | Required | Default | Meaning |
|---|---|---|---|---|
| `state` | `content-stamp` | no | absent | A stamp (3.17). The content stamp of the model, taken when the drawing was read from a VSDX or OpenDocument file. |
| `ledger` | `page` | yes | none | A page's `id`. |
| `ledger` | `mark` | yes | none | The highest shape `id` ever given to a shape of that page, a whole number (`u32`). |

- **The ledger.** A program that gives new shapes their `id` gives each one an `id` above the page's mark, so that the `id` of a deleted shape is not given again: formulas, connections, held data, and data carried in other files name shapes by `id`, and a reused `id` would quietly point them at a new shape. A writer writes one `ledger` element for each page that has a mark above 0. A reader takes a mark that is lower than the highest shape `id` on the page as that `id`: a ledger can raise a mark and never lower one. When the ledger has more than one row for one page, the reader takes the highest mark of them, so that an `id` is never given again. It ignores a row for a page the drawing does not have. A writer writes neither a second row for a page nor a row for a page the drawing lacks. *These rules are rules of this format.*
- **The content stamp.** It exists so that a program that writes the drawing to a foreign format can tell whether the drawing was edited since it was read from one (4.1). Only the program that made it can use it. A program that changes a drawing and does not compute stamps SHOULD leave out `content-stamp`; a stale one only makes the next DiaCraft treat the drawing as edited. A drawing that never came from a foreign file has none.
- A reader ignores elements and attributes of `state.xml` that it does not know.

### 2.8 `preview.png`

`preview.png` is a PNG picture of the first page, at most 256 pixels on its longer side. It is stored and its role is `preview`. It is for file managers and is never read back into the drawing: a reader MAY ignore it, and a writer MAY leave it out. A writer whose drawing is unchanged since it read the file copies the preview as it was, or, where it does not copy the entries of that file as they are, writes the same bytes again. A file in the earlier namespace (3.1) is unchanged in this sense when its document part is the part the writer makes once the earlier namespace in it is read as the current one: it keeps its preview too.

### 2.9 Order and determinism (informative)

The same model gives the same bytes, except that `preview.png` depends on the fonts of the machine. The reference implementation draws the preview within a budget of work (shapes, glyphs, dashes, pictures decoded), and to no time, so the load of the machine does not change it. A writer that wants this:

- writes the entries in this order: `mimetype`, `manifest.xml`, `document.xml`, the page parts in page order, the master parts in master order, the blobs by number, `state.xml`, `preview.png`, and the extra parts in the order they were read;
- gives every entry the fixed date of 2.1, and deflates at one fixed level;
- writes XML in the form of 3.1: the same declaration, no layout white space, attributes in the order of the tables of 3.18, and the known children in the order those tables give;
- numbers blobs in the order they are first used while it writes the document part, then the page parts in order, then the master parts in order.

This makes an unedited save give the same file, and a difference between two files show only real changes.

### 2.10 What is damage

A reader refuses a file as **damaged**, in words, and does not guess, when:

- the ZIP structure breaks 2.1, or an entry (a blob apart, see below) fails its CRC or does not inflate to the size its header states;
- `mimetype` is not as 2.3 says, or its kind differs from the manifest's;
- the manifest is not well-formed XML, its root is not `manifest`, `format` or `kind` is missing or not what 2.4 says, or a `part` has no `path` or `role`;
- an entry is unlisted, listed twice or missing (a missing blob apart), a row of the document, state or preview role names another entry, an entry named like one of them has another role, a page or master entry is named by no list of the document part, or a page or master part is named more than once by the lists (2.4, 3.3: *rules of this format*);
- `document.xml` is missing or not as section 3 says, or a page or master part that its lists name is missing, or its root `id` is not the `id` the list gives;
- a model part breaks section 3: a required attribute is missing, a number or flag or stamp is not one, a word of a closed list (3.1) is not on the list, base64 is not base64, two cells of one name stand in one place, or a part passes the limits of section 6.

**Damage confined to blobs does not refuse the file.** A blob entry that is missing, or whose CRC fails, or whose size or stamp differs from what its record in the document part states, is a **lost blob**, and so is a blob that an element names and the document part has no record of. The file opens. Each picture whose bytes are lost is shown as a picture that could not be loaded; a theme whose bytes are lost is left out; a package part whose bytes are lost is left out; and the program tells the person in words how many it lost, without naming anything from the file. Otherwise one bad picture would lose a whole drawing.

## 3. The model

### 3.1 Conventions of the vocabulary

**The namespace.** Every element of every model part, and of `manifest.xml` and `state.xml`, is in the namespace `urn:app.diacraft:extension` (the domain `diacraft.app`, which the application's id `app.diacraft` is also based on). Writers use the prefix `diacraft` and declare it on the root of the part. A reader MUST match the namespace and never the prefix: a part may use another prefix, or make the namespace the default. **The namespace does not change again in any version of the format.** **The earlier namespace is read too:** builds made before the id moved to the domain (APPID, 2026-11-23, before the first public release) wrote `urn:io.github.one_bit.diacraft:extension`, and **a reader MUST read an element or attribute in it as DiaCraft's own, in a part of this format and in the other formats' carriers alike, and a writer MUST write only the current one.** A part in the earlier namespace opens as it did, and is written again in the current one whenever a writer writes it (the declarations of the earlier namespace that the model keeps whole, in an unknown element or the prefix of an unknown attribute, are written as declarations of the current one); a part that a save copies as it was, and a file that a save splices into, keep what they held, so that **a package may hold both**, and it reads as one document, each entry once, with no entry added beside one that is there. A save that changes nothing of such a file keeps its preview (2.8). The format's version stays 1.1: the first public release is the first to write the current namespace. Element and attribute names are case-sensitive. Attributes are in no namespace.

**The vocabulary** is 65 elements with 150 attributes, in the tables of 3.18. Most names are the model's field names. The names of the sheet (`Cell`, `Section`, `Row`, and the attributes `N`, `V`, `F`, `U`, `E`, `IX`, `T`, `Del`, `LocalName`, `LineStyle`, `FillStyle`, `TextStyle`) are the VSDX format's, so that a program that knows that format reads them at once. Each local name has one meaning wherever it stands in a part of the model. (`manifest.xml` and `state.xml` use `manifest`, `part` and `state` for themselves; they are different documents, so no part is confused.)

**Attribute kinds.**

| Kind | What it holds |
|---|---|
| `text` | Any string. See the escapes below. |
| `u32` | A whole number from 0 to 4,294,967,295 in decimal digits. A writer writes plain digits: no sign and no leading zeros. A reader MUST refuse a value that is not a whole number in this range (a reader MAY accept a leading plus sign or leading zeros). |
| `u64` | The same up to 18,446,744,073,709,551,615. Only `size` of a `blob` is one. |
| `flag` | `1` for true. Absent means false. A reader also accepts `0`, `true` and `false`; a writer MUST NOT write them. Any other text is damage. |
| `blob` | A blob's number (3.16): a `u32` that names a `blob` record of the document part. |
| `stamp` | Exactly 16 lowercase hexadecimal digits (3.17). Anything else is damage. |

**Defaults.** The tables of 3.18 give each attribute one of three states:

- *with a default*: when the attribute is absent the field has that value. A writer SHOULD leave out an attribute whose value is its default, and a reader MUST read one that is present.
- *optional, with no default* (the Default column shows a dash and Required is no): when the attribute is absent the field has no value. This is not the same as an empty text: `name=""` is the empty name, and an absent `name` is no name.
- *required* (Required is yes): there is no default, a writer always writes it, and an element without it is damaged.

**Text, escapes and long values.** A `text` attribute `x` can be written in three forms, and a reader MUST read all three:

1. as the attribute `x`;
2. as the attribute `x-b64`, holding the base64 (RFC 4648, standard alphabet, with padding, no line breaks, nothing else in it) of the UTF-8 bytes of the text. A writer MUST use this form when the text holds a character XML 1.0 forbids: a control character other than tab, line feed and carriage return, U+FFFE or U+FFFF. Such a character is never refused and never dropped: it comes back exactly;
3. as a child element `s` with `n="x"` whose text is the value, and with `b64="1"` when that text is base64. A writer MUST use this form for a value longer than 16,384 bytes, because an XML reader caps an attribute by the bytes as written.

In an attribute value a writer escapes `&`, `<` and `"`, and writes a line feed, a carriage return and a tab as `&#10;`, `&#13;` and `&#9;`, so that no parser turns them into a space. In text it escapes `&`, `<` and `>`, and writes a carriage return as `&#13;`. The text of an element keeps every blank: nothing is trimmed, and a writer adds no layout white space inside an element that holds text. A writer writes no CDATA section and no comment inside a model part; a reader reads CDATA as text and ignores comments and processing instructions outside kept data (3.12).

**Numbers are text.** The model holds no floating-point number. A cell's value and formula are text, exactly as the source file wrote them: `007` stays `007`, and `1.50` stays `1.50`. Nothing in the format is respelled.

**Order.** A reader MUST accept the known children of an element in any order, and attributes in any order. Repeated elements keep their order, and it means something: pages, shapes (their order is the drawing's order, a later shape in front of an earlier one), connections, sections, rows, `attr` pairs, `kept` nodes, the parts of a text, and the members of a group. A writer SHOULD write the known children in the order of the Holds column of 3.18. White space between elements is layout and is ignored.

**The form of a part.** Every part is an XML 1.0 document in UTF-8 with no document type declaration. A writer starts it with `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` and a carriage return and line feed, writes attributes in double quotes in the order of the tables, writes an element with no children as `<x/>`, and adds no white space between elements. A reader refuses a document type declaration and any entity other than the five predefined and numeric character references, and never reads anything outside the package.

### 3.2 Parts and their roots

There are three kinds of parts. Each is a whole XML document whose root declares the namespace.

- The **document part**, `document.xml`, has the root `document`.
- A **page part** has the root `page`: the root is the page, with its attributes.
- A **master part** has the root `master`.

A reader MUST refuse a part whose root is not the one its role says. A fragment (one shape, one sheet, one held value) is written, where a carrier needs one (section 7), as an element of this vocabulary that declares the namespace itself.

### 3.3 The document part

`document` carries the title and the model version, and these children (3.18 gives their order):

- `props`: the document properties, with user-defined properties as `custom` children (3.10);
- `color`: the colour table, one entry each;
- `font`: the font table, one face each;
- `sheet`: the document's own sheet (the one the VSDX format calls the document sheet);
- `style`: the style sheets (3.4);
- `theme`: the themes, each as a name and a blob (3.10);
- `solution`: DiaCraft's own application data (3.10);
- `kept`, `ext`: kept data and held data of the document (3.12, 3.13);
- `part`: the package parts of a foreign format that the model holds as bytes (3.14), and `vsdx-held`: the rest of a VSDX package that the model holds (3.14);
- `page-ref`: one for each page, in page order. It gives the page's `id` and the name of the entry that holds its part;
- `master-ref`: the same for each master;
- `blob`: one for each blob, in the order of the numbers (3.16).

The lists are exact. The number of `page-ref` elements equals the number of page parts, and the same for masters; the order is the order of the pages and masters. A page or master part whose root `id` differs from the `id` its list entry gives makes the file damaged, and so does a part that the lists name more than once (an entry is the part of one page or one master). *A rule of this format.*

### 3.4 Pages, masters and styles

A **page** has an `id`, a `name` (what its tab shows) and a universal name `name-u`. Its `sheet` is the page sheet, with cells such as `PageWidth` and `PageHeight`. A page is a **background page** when `background` is on; `back-page` is the `id` of the page that serves as this page's background. A page may be missing the `back-page` it names or form a cycle with other pages (hostile files can); a reader does not follow such a chain forever. The page's shapes follow as `shape` children, in drawing order, then its `connect` elements (3.8). A stencil has no pages.

A **master** is a reusable shape: it has an `id`, names, a GUID `unique-id`, `pattern-flags`, an `icon`, its own sheet, its shapes and its connections. A shape that is an instance of a master names it with `master` (3.5).

A **style** is a sheet of cells and sections with three links (`LineStyle`, `FillStyle`, `TextStyle`) to the styles it is based on. Style 0, named `No Style`, is the root every style leads to. A shape's `sheet` names the styles it takes its line, fill and text cells from, with the same three attributes.

`rel-id` and `part-name` on a page or master, and `rel-id` on a `foreign` shape, are the identifiers the page, master or picture had in the VSDX package it came from. They are kept so that a VSDX writer can give them back, and mean nothing to a program that does not write VSDX.

### 3.5 Shapes

A `shape` has these attributes: `id`, names, `type`, `master` and `master-shape`, `del`, and `data1` to `data3`. Its children are, in order, its `sheet`, its `text`, its `foreign` data, its kept attributes and nodes, its `ext` slot, and then **the members of a group as `shape` elements directly inside it**, in order, with no wrapper.

- `type` is `Shape`, `Group`, `Foreign` or `Guide` (a closed list). When none of those words fits, the source's own word is in `type-other` and `type` is absent. When neither is present the shape has no stated type.
- `master` is the `id` of the master the shape is an instance of; `master-shape` is the `id` of the shape inside that master that this shape stands for; `del` marks an inherited shape that was deleted in this instance. What an instance inherits and how is the VSDX format's rule.
- A shape's `id` is a whole number. It identifies the shape for formulas, connections, the ledger (2.7) and carried data. Shapes of one part do not have to have different `id` numbers, but a program that gives new ones SHOULD give each a number no shape of the page has had.
- **Depth.** A shape of a page or a master is at level 1, and each group adds a level. A conforming file has at most **200** levels. A reader MUST refuse a deeper one in words, and MUST check the depth before it builds the shape.
- At most 1,000,000 shapes stand in one part.

### 3.6 Text

`text` holds the pieces of a shape's text in order:

- **characters**: a bare text node, or a `t` element. A writer uses a bare text node, except that it uses `t` for a run that is empty, or holds a character XML cannot hold (then `t` has `b64="1"` and base64 text), or directly follows another bare text node (which a reader would join to it);
- **markers**: `cp`, `pp` and `tp`, each with `ix`, name the `Character`, `Paragraph` and `Tabs` row of the shape's sheet that applies from that place on;
- `fld`: a field, with the `Field` row it uses in `ix` and its displayed text as its text;
- `kept`: VSDX markup the model has no field for (3.12);
- `odf-raw`: a node an OpenDocument drawing had inside the text, never written into VSDX (3.15);
- `ext`, always last: the held data of the text (3.13);
- any other element, and a `txt`, `comment` or `pi` directly inside `text`: what a later minor version added (section 5).

Characters keep every blank, line break and tab. The markers and the rows they name stay in step, which is why the text is kept in pieces and not flattened.

### 3.7 Pictures and foreign data

A `foreign` element holds what a `Foreign` shape shows: its own attributes as `attr` children (`ForeignType`, `ObjectType`, `CompressionType` and the rest, kept verbatim and in order, as the VSDX format names them), a `rel-id`, optional `text`, and a `picture`. A `picture` gives the picture's `part` name and content `type` in the package it came from, and its bytes as a blob. Many shapes may name one blob, and then share it. A picture whose bytes are lost (2.10) is a picture that could not be loaded: the shape is a foreign shape that shows nothing.

### 3.8 Connections

A `connect` element records one connection between two shapes, as the VSDX format's `Connect` row does: `from-sheet` is the `id` of the shape the connection starts at, `from-cell` the name of its cell, `from-part` a number that says which part of that shape, and the same three for the end. A page or master lists its `connect` elements after its shapes, in order. A program that edits connectors keeps them in step with what the connector's cells say.

### 3.9 Sheets, cells, sections and rows

A `sheet` has up to three style links, then `Cell`, `Section`, `attr`, `kept` and `ext` children. A sheet with no links, cells, sections or kept data has no element.

- **Cells** stand directly in a sheet, in a section, or in a row. In one place no two cells have the same `N`; a writer writes them in the order of their names (sorted by the bytes of the name), and a reader accepts any order. At most 4,096 cells stand in one place.
- **Sections** stand in a sheet, in the order of the source, with a name `N`, an optional `IX`, and `Del`. At most 8,192 sections stand in one sheet.
- **Rows** stand in a section, in order, with an optional name `N`, `IX`, `LocalName`, a type `T` (such as `MoveTo` or `LineTo`) and `Del`. At most 320,000 rows stand in one section.
- A cell's, section's or row's name is 1 to 1,024 bytes. Names are case-sensitive: `PinX` and `pinx` are two cells.

A `Cell` has `N` (required), a value `V`, a formula `F`, a unit `U` and an error `E`, all text. Cached values and formulas are stored exactly as the source wrote them, and the format does not require a value to agree with its formula. **The two markers.** `F="Inh"` means the formula is inherited (the cell's value is the cached result of a formula it takes from its style or master), and `F="No Formula"` means the cell has no formula. A formula whose text is itself `Inh` or `No Formula` is written in `FL` instead of `F`, so that it is not read as a marker. The formula language and the meaning of cell names, sections and rows are the VSDX format's.

A `Cell` may have an `extra` child, for text inside the cell element. It is written even when empty: **an empty `extra` is not an absent one**.

A sheet or a section that the model holds in more than one place is written once and named, and the other places say `same` (3.19).

### 3.10 Properties, colours, fonts, themes and application data

- **`props`** holds the document properties, each as text exactly as the source format wrote it (dates too). The element is absent when no field is set and there is no user-defined property. A user-defined property is a `custom` child with its `format` (`vsdx` or `odf`, where it came from), its `name`, its `value-type` as the source spells it, and its `value`.
- **`color`**: one entry for each palette slot the table defines, with `ix` and `rgb` as `#RRGGBB`. At most 4,096.
- **`font`**: one face each, with its universal name `name-u`, its other attributes as `attr` children, and its slot.
- **`theme`**: a theme is a part of the foreign package that holds colours, fonts and effects. The model keeps its name and its bytes as a blob. What can be derived from the bytes (the colours, the fonts, the scheme numbers) is worked out by a reader and never stored; that is why a theme whose bytes are lost is left out (2.10).
- **`comment-list`** holds the comments of the drawing: the authors, then the comments in the order they were made, each naming its page and its shape by `ID`, so that a move of a shape parts them from nothing. A reply is a `comment-entry` with a `parent`, a thread is one level deep, and a reader makes a deeper chain flat in one pass. At most 100,000 comments, 10,000 authors, 65,536 bytes of text in a comment and 32 MiB in all. What a VSDX file holds of them is its comments part, whose entries have no parent (a reply is written there as a comment of the same page and shape); what an `.odg` holds is not written yet.
- **`solution`** holds one `el`: DiaCraft's own application data (for example the snap and glue options and the stencil list), kept whole. A program that does not understand it keeps it.

### 3.11 Identity

`id` is the identity of a page, a master, a style and a shape: a `u32`. In a well-formed drawing the `id` of every page, every master and every style is different from the others of its kind; a shape's `id` is not required to be. A reader still accepts duplicates, because hostile files exist (the part names of 2.5 carry the extra suffix for them). `name` is what a person sees, `name-u` is the universal name that a style or a formula refers to, and `unique-id` of a master is its GUID. Names are text and are never changed when a file is written.

### 3.12 Kept data

Where a source file has attributes or nodes that the model has no field for, the model keeps them as **kept data**, and a writer writes them back. Kept data is always written as data, never as literal XML. That way its prefixes cannot clash with the model's namespace, and a kept attribute cannot clash with a typed attribute of the same name (a shape with `id` 7 and a kept attribute `ID="007"` keeps both).

- **Kept attributes**: `attr` children, each with `name` (as written, with its prefix) and `value`, in order. The same name may stand twice; both are kept in order.
- **Kept nodes**: `kept` children. Each holds exactly one node and has `after`, a number set by whoever wrote the node: how many of the owner's typed children (cells, sections, rows, shapes, as the owner counts them in source order) came before the node in the source file. A writer of the source format puts the node back after that many. A program that does not write that format keeps the number as it is.
- **The node** inside `kept` is one of:
  - `el`, with `name` (prefix included), then `attr` children for its attributes (the `xmlns` declarations among them, as ordinary attributes), then its children as `el`, `txt`, `comment` and `pi`, in order;
  - `txt`, the characters of a text node;
  - `comment`, the characters of a comment;
  - `pi`, with `target` and the data as its text.
  `txt`, `comment` and `pi` take `b64="1"` and base64 text when their characters hold what XML forbids.
- A kept element is at most **200** levels deep. A reader MUST refuse a deeper one.

A kept text node is the element `txt` and not `text`: `text` is the element of a shape's text, and each local name of the vocabulary has one meaning.

Because kept data is data, it is exact, and it needs no namespace care: the names inside it are text.

### 3.13 Extension slots

Many objects have one **slot** for data that belongs to a format other than the model's own cells: the document, a page, a master, a style, a shape, a row, a text, a sheet, a section, a cell's `extra`, a `foreign` element, a `connect` and a `font`. A slot is one `ext` child, written only when it holds something. It has up to two children:

- `odf`: what the object holds of **OpenDocument** data (3.15);
- `vsdx`: the **VSDX** elements the object had no place for, one `rest` child each. `rest` has a `key` (the element's local name, and `/` and the `IX` for a colour entry) and holds that element's attributes and nodes as `attr` and `kept` children.

A slot holding nothing is the same as no slot: a reader that finds an empty `ext` and a writer that leaves it out agree.

**What the `odf` element holds, by owner.** For a document, a page, a style, a shape, a row and a text it holds typed children (3.15). For every other owner (a master, a sheet, a section, a cell's `extra`, a `foreign`, a `connect`, a `font`) it holds only `attr` and `kept` children: the attributes and nodes the OpenDocument file had there.

**Unknown content.** What a later minor version of this format writes on an object (an attribute or an element that this version does not know) is not in the slot. It stays where it was written, on the object's own element. Section 5 gives the rules.

### 3.14 Package parts and relationships held for VSDX

A VSDX package holds parts that no field of the model reads: macro projects, embedded objects, custom XML, the thumbnail, digital signatures, fonts, and anything else. The model holds each as a **package part**, and an OpenDocument package's entries that no field reads are held the same way.

A `part` element (in the document part) has `format` (`vsdx`, `odf` or `native`: whose rules the part belongs to; a closed list), its `name` in its package, its `media-type`, a `role` (a closed list: `thumbnail`, `macro-project`, `macro-signature`, `macro-data`, `embedding`, `object-file`, `object-replacement`, `signature`, `custom-xml`, `font`, `other`), and `blob`, its bytes. Its children are `manifest`, whose `attr` children are the other attributes of the manifest row that named the part in an OpenDocument package, and `rel`, one for each relationship from the part.

A `rel` is a relationship as the Open Packaging Conventions define one: `source` (the part it comes from, or `/` for the package), `id`, `kind` (its type, a URI, verbatim), `target` and `external` for a target outside the package.

`vsdx-held` holds what a VSDX package has at the document level that nothing else here holds: the `rel` rows that a writer would not make itself (the package's, the document's, each page's and master's), the content-type rows beyond what the parts need (`content-type` with `extension` for a default row and `part` for an override row, never both and never neither), `props-rest` (the elements of the VSDX property parts that no field reads, as `rest` children), and `windows`, the window settings, kept whole. A drawing that came from no VSDX file holds a `vsdx-held` with `windows` alone once its window settings are set (informative: the grid, the rulers, the guides and the connection points are the flags of its drawing `Window`, and a drawing with none opens as a new drawing shows; no rule of the file format changes).

**A package part of format `native`** is an extra part of a later minor version (5.2). A writer of this version never writes it as a `part` element with a blob: it writes it as an entry of its own, at its own path, with its manifest row. A reader that finds a `part` element of format `native` reads it, as it reads one of any format, as a held part whose bytes are its blob.

**A package part is bytes.** A macro project is kept as bytes and nothing else. No program parses it past its container, runs it, or activates it. The same holds for embedded objects. The bytes are copied from one file to another unchanged.

### 3.15 OpenDocument data

This is what the `odf` element of a slot holds. It records what an OpenDocument drawing says that the cells cannot say exactly, so that the drawing can be written back as it was.

| Owner | Attributes of `odf` | Children of `odf` |
|---|---|---|
| Document | `mimetype` | `root` (name, and the attributes of one of the file's XML roots, as `attr`), `manifest-row` (name, and the other attributes of one manifest row), `meta` (the metadata, as `kept`), `settings` (the settings tree, one `el`), `definition`, `layer`, `kept-style` |
| Page | `master-page`, `layout`, `stamp` | `master-page-attrs`, `layout-props`, `drawing-page-style` (one `el`), `units` |
| Style | `name`, `display-name`, `parent`, `family` | `unmapped` (the style properties no cell stands for, and the values no cell can show, as `attr`), `units`, `held` (any number, in order), `held-style` (any number, in order) |
| Shape | `style-name` | `held` (any number, in order), `units` |
| Row | `style-name` | `held` (at most one) |
| Text | none | `paragraph` (any number, in order) |

All of them also take `attr` and `kept` children for what the file kept beside.

- A **definition** is a named drawing resource of the document exactly as the file wrote it: a line-end marker, a gradient, an opacity gradient, a hatch, a dash pattern, or a fill image (`family` is `marker`, `gradient`, `opacity`, `hatch`, `stroke-dash` or `fill-image`; a closed list). It holds one `el`. A fill image's bytes are a blob named by `blob`. At most 65,536 definitions of one family stand in a document.
- A **layer** is one `el`: an element of the layer set, in order.
- A **kept style** is an OpenDocument style that the model's style sheets do not hold (paragraph, text, drawing-page, list, data and default styles, and the automatic styles of either part), kept whole as one `el` by its `family` and `name`; an automatic style of the graphic, paragraph or text family is kept as the part of it that no cell reads, and not at all when it holds none. DiaCraft's reader of an `.odg` writes the `family` as the container, `common` (the styles of `office:styles`), `auto` (the automatic styles of `styles.xml` and `content.xml`, in that order), `master` (a master page of `office:master-styles` that holds no shape, kept whole with its attributes and children as the file wrote them: `master:master-page`), `unused` or `unused-styles` (an automatic style of the graphic, paragraph or text family of `content.xml`, or of `styles.xml`, that **nothing names**, kept whole, as one `el`: one with no name, one that no shape, paragraph or run found, and the second of a name that its container holds twice, which is held under a name no style of the file holds), a colon, and the kind (`graphic`, `text`, `list-style`, `page-layout`, and so on); a default style has the name `""`. A writer of an `.odg` writes an `unused` style in the automatic styles of `content.xml` and an `unused-styles` style in those of `styles.xml`, under the name it is held by (none for a style with no name), and a kept master page among the master pages, its children in the master page of its name that a page is on and whole when no page is on it, and an automatic drawing-page style in the part of what names it: a master page's in `styles.xml` and a page's in `content.xml`. **A style written whole is written in the form ODF 1.4 takes on its `style:paragraph-properties` and `style:text-properties`**: `text:enable-numbering` of ODF 1.0, which no later version has, is written as `diacraft:older-enable-numbering` in the place it stood, any other attribute of the standard's namespaces other than `fo` and `style` that the schemas of 1.2 and 1.4 have no place for there is left out and said, and a weight, a slant or a variant that 1.4 lists no value for (`fo:font-weight="250"`) is written as the value 1.4 lists nearest, the value it had riding in `diacraft:older-font-weight` (or `older-font-style`, `older-font-variant`, `older-font-weight-complex`, `older-font-style-complex`) at the end of the same element. A read puts each back where it stood, a value only while the attribute beside it is the one the writer wrote for it.
- `units` records the unit each length attribute was written in, so that `2cm` comes back as `2cm` and not as the same length in inches. It is a list of `attr` pairs. DiaCraft's reader of an `.odg` records only a unit that no cell spells (`px`, or another unit than the cell that takes the value), since a cell holds the unit of the length it was read from; and it keys a style's `unmapped` pairs by the properties element and the attribute (`style:graphic-properties/draw:fill-hatch-name`).
- A **held value** (`held`) keeps a construct exactly, beside the **stand-in cells** that can only approximate it. An OpenDocument hatch is a fill pattern in the cells and the hatch's own definition here. `group` says what it is (`fill`, `line`, `line-ends`, `shadow`, `text`, `frame`, `glue` or `geometry`; a closed list). Its `attr` and `kept` children are the attributes and nodes exactly as the file had them. Its `stamp` is the stamp of the cells it stands beside, as they were when it was read.
- **A held value never overrides a cell.** A writer of the format uses a held value only while the stamp of the cells still equals the stamp stored with it. Otherwise it writes from the cells, as it would for a new shape. A program that edits the cells does not need to touch held values: a stale stamp is found by the next program that judges it, and an undo that puts the cells back makes the value good again. Which cells each kind of value stands beside is the business of the program that makes and judges the stamp. Another program keeps held values and their stamps exactly. Some values have no stamp (the frame of a shape, and a circle's centre form): the position of the shape on the page says whether they are good.
- A **style** holds the values its cells only approximate, as a shape does. A `held` child of a style's `odf` is a held value exactly as a shape's is (`group`, `stamp`, and the attributes and nodes as the file had them), and its `stamp` is the stamp of the cells of the style's own sheet that it stands beside, by the same rule (a hatch by the fill cells and the rows of `FillGradient`, a marker by that end's arrow cells, a shadow by the shadow cells, and none for a dash or a tab stop, which are written as held on every save). A `held-style` child of a style's `odf` is a value of its paragraph or text properties, as in a `text-style`, and its `stamp` is that of its `group` in row 0 of the style's `Paragraph` or `Character` section. Of a style sheet's group `Style` (the weight, the style and the small caps of the text, and the complex script's weight and style: one number, the cell `Style`) a `held-style` is the **spelling** the file gave the group when that is not what the cell alone says (a weight and nothing else, a weight written as a number, none of the five where the style only underlines): its `attr` children are the attributes the file stated of the group, exactly and in order, there are none when it stated none, and the writer says those while the stamp holds and the cell's five when an edit changed the cell. A value of a style that no cell can show (a length that does not read, a keyword the property does not take) is among its `unmapped` pairs, as written.
- **What a text holds of its structure**. A paragraph's `element` is one `el`: the paragraph's own element when it is not a `text:p` with no attribute but its style name (a `text:h` with its `text:outline-level`, or a `text:p` with `text:class-names` or `xml:id`), with its name and attributes as the file had them, without the style name (the `text-style` holds it) and without its children. A `level` of a paragraph is one list round it, outermost first, with a `list` and an `item` (each one `el`: a `text:list`, and a `text:list-item` or `text:list-header`, with its attributes and none of its children) **for the paragraph that opens that list or item, and none for the paragraphs that continue it**; an empty `level` is a paragraph in the list and the item an earlier paragraph opened. A paragraph is in at most 256 levels of lists. A list or an item that holds no paragraph is not held. A `run` is a stretch of the paragraph's characters that an element wraps: a `text:span` (its `text-style`), or any other element of the paragraph's text (a hyperlink, a field, a frame anchored in it) with its `element`; the runs are in the order they begin, an outer one before an inner one that begins where it does. A text with no characters is no `text` of the model: what its paragraphs say (a heading, a list, a bookmark, a paragraph with a style or an attribute) is a `kept` child of its shape's `odf`, and a bare `text:p` says nothing.
- **What the containers hold that no field reads**. The attributes of a container below the root of a part are a `root` named `<part>/<element>/<element>`, the elements as the file wrote them (`content.xml/office:body/office:drawing`, `styles.xml/office:master-styles`). A child of a container that no field reads (`office:scripts`, a child of the body, the drawing, the master styles or a layer set) is a `kept` child of the document's `odf`: **one `el` that is the root of its part and holds the next container, and so on down to the child kept, whole**; its `after` is the number of children of the innermost container that a reader read before it. A master page that holds no shape is a kept style (`master:master-page`); a file written before it may hold a master page's children here, in an `el` of the master page with its `style:name` among its attributes, which says which one, and a reader takes them as they are. A page's own layer set is a `kept` child of the page's `odf` when its `Layer` rows do not say all of it. A flat drawing holds none of this but its master pages and the attributes of its root, which are held as the `root` entries `content.xml` and `styles.xml` (7.4), and a reader says how much it left out (7.7).
- A **text** keeps what its paragraphs and runs named: one `paragraph` for each paragraph the file had, in the file's order, with `characters` (the stamp of its characters when read), a `text-style` (the style it names, with `name` and `stamp`, and a `held-style` for each held style value, each with its `group` from the list in 3.18 and its own stamp), and `run` children (each with `from` and `to`, in characters of its paragraph, and a `text-style`).
- **How the `.odg` reader fills the document's data** (informative). Each distinct `style:font-face` is a `font` of the document, named by the family it declares (by the declaration's own name where it declares none), and holds the declaration exactly in its `odf` element; an equal declaration in the other part of the package is the same `font`. `dc:title` is the document's `title`; the fields both formats say (the description, the subject, the first keyword, the initial creator as the creator, `dc:creator` as who saved it last, the creation, modification and print dates, the language) are `props`, and `dc:language` is also the document sheet's cell `DocLangID` when it is a language tag; each `meta:user-defined` is a `custom` of format `odf`; every other field of `meta.xml` is a `kept` child of `meta`, with `after`, the number of fields typed before it. `settings` is the whole `office:document-settings` tree. A `root` is named by the entry whose root it is (`content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, `META-INF/manifest.xml`). Each entry of the package that no field reads, a file or a folder (a folder has no bytes), is a `part` of format `odf`; the row of the manifest that names it is the part's `media-type` and `manifest`, and every other row, a second row for one path and a row for an entry the package lacks among them, is a `manifest-row` named by its path (by the empty name when the row has none). A `meta.xml` or a `settings.xml` that these fields cannot hold (more than 4 MiB, or a `meta.xml` whose `office:meta` has attributes of its own) is a `part` and nothing else.
- **How the `.odg` model writer writes this data** (informative). A drawing with no file is written as a package of its own. `meta.xml` is written from `title`, `props`, `custom` and `meta`: the typed fields in the order of the schema, each `kept` node after the number of typed fields its `after` names, and the generator the model holds only while the content is as it was read, else the writer's own. `settings.xml` is the `settings` tree, and for a model that holds none a root that holds nothing (the ODF 1.4 schema has no empty `office:settings`). The manifest is written from the `root` of `META-INF/manifest.xml`, the `manifest-row`s (the root row and the rows of the four parts, with the attributes held) and each `part` (its `media-type` and `manifest`); the entries are the `part`s, in their order. A container's `kept` children and the attributes a `root` holds for a container are put back in `content.xml` and `styles.xml` where they stood. **Every part states `office:version="1.4"` and the manifest and its root row `manifest:version="1.4"`, whatever the file the data came from said**: some programs that read OpenDocument refuse a package whose parts and manifest state two versions. A `part` of format `vsdx` or `native`, a property OpenDocument has no field for (`category`, `company`, `manager`, `hyperlink-base`) and the markup of a VSDX document that no field reads ride in `DiaCraft/`; until that is written, a save refuses in words and never leaves them out. **A shape is written as it was read** (informative): the writer reads a shape's `held` values and `odf` slot as a writer of the model does. A `held` value of the group `geometry` with no attribute and one kept element is the element the shape was read from (a bare `text:p` the element held is its child); one with a stamp stands beside the `Geometry` sections (a plain shape's, and a connector's route), one with none is the element alone. A segment holds its ends as written, and a `draw:stroke="dash"` is a held value of the group `line` stamped by `LinePattern` (the writer names no dash of DiaCraft's own beside it: the dash is the style's parent's, or none, as the file had it, until the pattern is set; a style that names a dash of DiaCraft's own that its cells say again, by the pattern and the cap form its name says, holds neither the stroke nor the dash, and its cells write both); the value of the group `shadow` is stamped by the six cells the reader sets from it. A shape whose `Geometry` sections DiaCraft's own data carried holds no value of the element's outline. The writer uses a value while its stamp holds, the style name and units the slot holds while the style's cells and the lengths' cells are as they were, and writes the rest from the cells; a shape's automatic graphic style is written under the name the shape held with the attributes of its graphic properties that the document's kept style of the family `auto:graphic` and that name holds and no cell reads. **Style sheets and fill images** (informative): a style sheet whose `odf` slot holds a family and a name is written as the common style it was (its name, the `name` it shows, its family, the parent its links name, the attributes of its cells, the `held` values whose stamps hold, its `unmapped` properties and its `kept` children), and the `style` entry carries only the difference between that markup's sheet and the model's; a `definition` of the family `fill-image` is written with its bytes as the `Pictures/` entry its `xlink:href` names (or `Pictures/fill<n>` where the package cannot give that name), with a manifest row. **A frame's images and a custom shape's glue points** (informative): a frame shows the first child it draws, and its other children are `kept` children of the shape's `odf` whose `after` is 0 for one that stood before the child it shows and 1 for one that stood after it (a reader that wrote them before the change gave 0 to all). The writer writes a picture frame's kept children with its own picture between them, in that order, so that a frame of two PNGs shows the same one on its next read. A kept `draw:image` whose link names an entry of the model that holds the frame's own picture (the replacement picture of an object whose entry the writer made) is that picture, and the writer makes no second; any other kept image is another image of the frame. A kept image's link that is a path in the package and names no entry of the package written (a flat file's relative link, `plan.svg`, is a file beside the document) is written as a link out of the package, `../plan.svg`; a link that names an entry, one with a scheme, one from the root or out of the package and a place of the document are written as they were. The glue points a custom shape's geometry lists are rows 4 and on of the shape unless the reader stops at the geometry before it reads them (`draw:extrusion` or `draw:text-path` true, or a view box with no width or height): then they are no rows, the shape has the four standard rows, and the writer says no `diacraft:glue="own"` for it. **A held geometry is written with the shape's flips** (informative): the stamp of a geometry value covers the `Geometry` rows and the equations, and not the flips or the turn of the text the session may give the shape afterwards, so the writer makes the element say `FlipX` and `FlipY` as `draw:mirror-horizontal` and `draw:mirror-vertical` and the shape's `TxtAngle` as the `draw:text-rotate-angle` that the reader reads back as it beside them, and leaves the element as the file had it when it says them already. The reader keeps no mirror and no text of a geometry that is an object (`draw:extrusion` or `draw:text-path` true), so the writer refuses a flip or a text of such a shape in words. A `draw:glue-point` element is in the page's axes and is the row it made only in the turn and flips the shape was read with: after a turn or a flip every row past the geometry's own points has its element written again for the shape's frame, and a rectangle or an ellipse written as a custom shape that lists its rows keeps none beside the list.

### 3.16 Blobs

Every byte string the model holds (a picture, a theme, a package part, a fill image) is a **blob**. Each distinct byte string is stored once.

- A blob has a **number**: the order of its first use, from 0, as the document part, then the page parts in order, then the master parts in order are written. Elements name a blob by `blob`, the number.
- The `blob` list of the document part has one record for each blob, in the order of the numbers: its number `id` (absent for 0), `path` (the entry), `media-type`, `size` and `stamp`. The records and the manifest agree about the entries.
- A writer finds equal byte strings by size, then by stamp, then by comparing the bytes, so that a stamp that happens to be equal never joins two different strings. The media type does not take part in this: a byte string that is used under two media types is one blob, and its record carries the media type of its first use.
- A reader gives all the shapes that name one blob one copy of its bytes.
- A record whose entry is missing, or whose `size` or `stamp` differs from the entry, is a lost blob (2.10), not a refusal.
- A reader checks a blob record before it fetches the bytes: it adds up the sizes the records state and refuses a drawing whose blobs are more than it can hold (section 6), without reading them.

### 3.17 Stamps

A **stamp** is the FNV-1a hash over 64 bits (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`), written as 16 lowercase hexadecimal digits of the 64-bit result, most significant digit first.

What is hashed is a sequence of **fields**. A field is its length in bytes as 8 bytes, least significant first, and then its bytes. The first field is always the name of what is hashed, the *domain*, as its UTF-8 bytes. A text field is the UTF-8 bytes of the text. A stamp says whether something changed. It is no integrity check and no secret.

- **The stamp of a byte string** (the `stamp` of a `blob` record): domain `bytes`, then one field holding the bytes. Test vectors are in 8.4.
- **The content stamp** (`content-stamp` in `state.xml`) is domain `content`, then, in order: each part as the writer of **version 1.0** would write it (the document part without its `blob` records, then each page part, then each master part), as a tree: **every `same` is the element it names, walked where the `same` stands, and no `def` is there** (3.19), so that a drawing has the stamp it had when each place held its own copy, and a stamp stored with a drawing is good for a file that says its units once. A tree is walked in order. An element gives the text field `<`, a text field of its name as written (with its prefix), an 8-byte field holding the number of its attributes (its `xmlns` declarations count), and for each attribute a text field of its name and one of its value as it is in the tree (after unescaping, and in the `-b64` form where a writer would use it). Its children follow in order and then the text field `/`. A text node gives the text field `t` and its text, a comment `c` and its text, and a processing instruction `p`, its target and its data. After the parts come the blob records, in the order of the numbers: an 8-byte field holding the number, a text field of the media type, an 8-byte field holding the size, and a text field of the stamp of the bytes as 16 digits. The stamp is defined over the content in this form, and not over how a later version writes a part: section 5.5 says that a later version keeps every stamp already stored. **The namespace is fed as version 1.0 wrote it** (APPID): a value that is `urn:app.diacraft:extension`, or that begins with it and a colon, is fed with `urn:io.github.one_bit.diacraft:extension` in its place, so that the stamp of a drawing is the same whichever of the two namespaces its parts say, and every stamp that a build before the namespace moved stored stays good.
- **Held stamps**: a stamp stored with a held value, a page, a paragraph or a style value is a stamp of cells, made by the program that wrote it (3.15). Another program reads it only as 16 digits, and never as something it can recompute.

### 3.18 Reference tables

These two tables are the whole vocabulary. Element names in the first table are the local names of the elements. In the second table the **Default** column shows the value an absent attribute has (a dash: none; `""`: the empty text), and **Required** says whether an element without the attribute is damaged. Every `text` attribute of every element may also be written as `<name>-b64` or as an `s` child (3.1). The `s` element stands in any element that has a text attribute.

| Element | Stands in | Holds |
|---|---|---|
| `document` | root of a part | In this order: `props`, `color`, `font`, `sheet`, `style`, `theme`, `solution`, `comment-list`, `kept`, `ext`, `part`, `vsdx-held`, `page-ref`, `master-ref`, `blob`. |
| `page-ref` | `document` | Nothing. One for each page, in page order. |
| `master-ref` | `document` | Nothing. One for each master, in master order. |
| `blob` | `document` | Nothing. One for each blob, in the order of the numbers. |
| `props` | `document` | `custom`, one for each user-defined property. The element is absent when every field is unset and there is no user-defined property. |
| `color` | `document` | Nothing. |
| `font` | `document` | `attr`, for the face's other attributes, then `ext`. |
| `style` | `document` | `sheet`, `attr`, `kept`, `ext`. |
| `theme` | `document` | Nothing. |
| `solution` | `document` | One `el`: DiaCraft's own application data, kept whole. |
| `page` | root of a part | `sheet`, `shape`, `connect`, `attr`, `kept`, `ext`. |
| `master` | root of a part | `sheet`, `shape`, `connect`, `attr`, `kept`, `ext`. |
| `shape` | `page`, `master`, `shape` | `sheet`, `text`, `foreign`, `attr`, `kept`, `ext`, then the members of a group as `shape` elements, in order. |
| `text` | `shape` | The pieces of the text in order: bare text, `t`, `cp`, `pp`, `tp`, `fld`, `kept`, `odf-raw`, and (last) `ext`. A `txt`, `comment` or `pi` directly inside it is a node a later version added. |
| `t` | `text` | Characters. |
| `cp` | `text` | Nothing. |
| `pp` | `text` | Nothing. |
| `tp` | `text` | Nothing. |
| `fld` | `text` | The field's displayed text. |
| `foreign` | `shape` | `attr` (the element's own attributes), `picture`, `kept`, `ext`. |
| `picture` | `foreign` | Nothing. |
| `connect` | `page`, `master` | `attr`, `ext`. |
| `sheet` | `document`, `style`, `page`, `master`, `shape` | `Cell`, `Section`, `same` (in place of a section), `attr`, `kept`, `ext`. A sheet with nothing in it has no element. |
| `same` | `document`, `style`, `page`, `master`, `shape`, `sheet` | Nothing. A copy of the sheet or the section that its `of` names (3.19). |
| `Cell` | `sheet`, `Section`, `Row` | `attr`, `extra`. |
| `extra` | `Cell` | `kept`, `ext`. Written even when empty: an empty `extra` is not an absent one. |
| `Section` | `sheet` | `Cell`, `Row`, `attr`, `kept`, `ext`. |
| `Row` | `Section` | `Cell`, `attr`, `kept`, `ext`. |
| `attr` | `font`, `style`, `page`, `master`, `shape`, `foreign`, `connect`, `sheet`, `Cell`, `Section`, `Row`, `el`, `odf`, `rest`, `manifest`, `custom`, `comment-list`, `comment-author`, `comment-entry`, `root`, `manifest-row`, `master-page-attrs`, `layout-props`, `units`, `unmapped`, `held`, `held-style` | Nothing. |
| `kept` | `document`, `style`, `page`, `master`, `shape`, `foreign`, `sheet`, `Section`, `Row`, `extra`, `text`, `odf`, `rest`, `meta`, `held`, `held-style` | Exactly one of `el`, `txt`, `comment`, `pi`. |
| `el` | `kept`, `solution`, `el`, `windows`, `settings`, `definition`, `layer`, `kept-style`, `drawing-page-style`, `odf-raw`, `element`, `list`, `item` | `attr` for its attributes (namespace declarations among them), then `el`, `txt`, `comment` and `pi` for its children, in order. |
| `txt` | `kept`, `el`, `odf-raw`, `text` | Characters. |
| `comment` | `kept`, `el`, `odf-raw`, `text` | The comment's characters. |
| `pi` | `kept`, `el`, `odf-raw`, `text` | The instruction's data. |
| `ext` | `document`, `page`, `master`, `style`, `shape`, `Row`, `sheet`, `Section`, `extra`, `foreign`, `connect`, `font`, `text` | `odf` and `vsdx`: what the object holds of each format. |
| `odf` | `ext` | `attr` and `kept` (what the format kept), and the children listed for its owner in section 3.15. |
| `paragraph` | `odf` | `text-style`, then `element`, `level` (one for each list round the paragraph, outermost first) and `run`, in order. |
| `run` | `paragraph` | `text-style`, `element`. |
| `text-style` | `paragraph`, `run` | `held-style`, one for each held value. Absent when it names nothing, has no stamp and holds nothing. |
| `held-style` | `text-style`, `odf` | `attr`, `kept`. |
| `element` | `paragraph`, `run` | One `el`. |
| `level` | `paragraph` | `list`, `item`. |
| `list` | `level` | One `el`. |
| `item` | `level` | One `el`. |
| `vsdx` | `ext` | `rest`, one for each element the object had no place for. |
| `rest` | `vsdx`, `props-rest` | `attr`, `kept`. |
| `part` | `document` | `manifest`, `rel`. |
| `manifest` | `part` | `attr`: the other attributes of the manifest row that named the part. |
| `rel` | `part`, `vsdx-held` | Nothing. |
| `vsdx-held` | `document` | `rel`, `content-type`, `props-rest`, `windows`. |
| `content-type` | `vsdx-held` | Nothing. Exactly one of `extension` and `part` is given; a row with neither or both is damaged. |
| `props-rest` | `vsdx-held` | `rest`. |
| `windows` | `vsdx-held` | One `el`: the window settings element, kept whole. |
| `custom` | `props` | `attr`. |
| `comment-list` | `document` | `attr`, then `comment-author` and `comment-entry`, in order. Absent when the drawing has no comment, no author and no setting. |
| `comment-author` | `comment-list` | `attr`. |
| `comment-entry` | `comment-list` | `attr`. |
| `root` | `odf` | `attr`. |
| `manifest-row` | `odf` | `attr`. |
| `meta` | `odf` | `kept`. |
| `settings` | `odf` | One `el`. |
| `definition` | `odf` | One `el`. |
| `layer` | `odf` | One `el`. |
| `kept-style` | `odf` | One `el`. |
| `master-page-attrs` | `odf` | `attr`. |
| `layout-props` | `odf` | `attr`. |
| `drawing-page-style` | `odf` | One `el`. |
| `units` | `odf` | `attr`. |
| `unmapped` | `odf` | `attr`. |
| `held` | `odf` | `attr`, `kept`. |
| `odf-raw` | `text` | Exactly one of `el`, `txt`, `comment`, `pi`. |
| `s` | any element with a text attribute | The text of a field that is too long for an attribute. |

| Element | Attribute | Kind | Default | Required | Meaning |
|---|---|---|---|---|---|
| `document` | `version` | u32 | `1` | no | The version of the document model the file was made with. 1 in every 1.x file. |
| `document` | `title` | text | `""` | no | The drawing's title. |
| `page-ref` | `id` | u32 | `0` | no | The page's `id`; it must equal the `id` on the root of the page part. |
| `page-ref` | `part` | text | `""` | no | The name of the entry that holds the page's part. |
| `master-ref` | `id` | u32 | `0` | no | The master's `id`; it must equal the `id` on the root of the master part. |
| `master-ref` | `part` | text | `""` | no | The name of the entry that holds the master's part. |
| `blob` | `id` | blob | `0` | no | The blob's number: its order of first use, from 0. |
| `blob` | `path` | text | `""` | no | The name of the entry that holds the bytes, `blobs/<n>.<ext>`. |
| `blob` | `media-type` | text | `""` | no | The media type of the bytes. |
| `blob` | `size` | u64 | `0` | no | The number of bytes. |
| `blob` | `stamp` | stamp | — | yes | The stamp of the bytes (section 3.17). A reader checks it against the entry. |
| `props` | `subject` | text | — | no | The subject. |
| `props` | `creator` | text | — | no | The author. |
| `props` | `last-modified-by` | text | — | no | Who last changed the drawing. |
| `props` | `keywords` | text | — | no | The keywords, as one text. |
| `props` | `description` | text | — | no | The description. |
| `props` | `category` | text | — | no | The category. |
| `props` | `company` | text | — | no | The company. |
| `props` | `manager` | text | — | no | The manager. |
| `props` | `hyperlink-base` | text | — | no | The base address for relative hyperlinks. |
| `props` | `app-version` | text | — | no | The version of the program that wrote the source file, as text. |
| `props` | `created` | text | — | no | When the drawing was created, as text exactly as the source format wrote it. |
| `props` | `modified` | text | — | no | When it was last changed, as text exactly as the source format wrote it. |
| `props` | `last-printed` | text | — | no | When it was last printed, as text exactly as the source format wrote it. |
| `props` | `language` | text | — | no | The language, as text. |
| `color` | `ix` | u32 | `0` | no | The colour index the entry defines. |
| `color` | `rgb` | text | `""` | no | The colour as `#RRGGBB`, verbatim. |
| `font` | `name-u` | text | `""` | no | The font face's universal name, the name a style refers to it by. |
| `style` | `id` | u32 | `0` | no | The style's `id`. Style 0 is the root every style leads to. |
| `style` | `name` | text | — | no | Its name. |
| `style` | `name-u` | text | — | no | Its universal name. |
| `theme` | `part` | text | `""` | no | The name of the theme part in the package it came from. |
| `theme` | `blob` | blob | — | yes | The bytes of the theme part, an XML part. Its colours and fonts are worked out from the bytes by a reader and never stored. |
| `page` | `id` | u32 | `0` | no | The page's `id`. |
| `page` | `name` | text | `""` | no | The page's name, as its tab shows it. |
| `page` | `name-u` | text | — | no | Its universal name. |
| `page` | `background` | flag | `0` | no | The page is a background page. |
| `page` | `back-page` | u32 | — | no | The `id` of this page's background page. |
| `page` | `rel-id` | text | — | no | The relationship id that named the page's part in the VSDX package the drawing came from, kept so that a VSDX writer can give it back. |
| `page` | `part-name` | text | — | no | The name of the page's part in the package it came from, kept for the same reason. |
| `master` | `id` | u32 | `0` | no | The master's `id`. |
| `master` | `name` | text | — | no | Its name. |
| `master` | `name-u` | text | — | no | Its universal name. |
| `master` | `unique-id` | text | — | no | The GUID that names the master in this drawing, as text. |
| `master` | `pattern-flags` | u32 | — | no | The master's pattern flags, a number. |
| `master` | `rel-id` | text | — | no | The relationship id that named the master's part in the VSDX package it came from. |
| `master` | `icon` | text | — | no | The master's icon, as base64 text, verbatim. |
| `master` | `part-name` | text | — | no | The name of the master's part in the package it came from. |
| `shape` | `id` | u32 | `0` | no | The shape's `id`. |
| `shape` | `name` | text | — | no | Its name. |
| `shape` | `name-u` | text | — | no | Its universal name. |
| `shape` | `type` | text | — | no | `Shape`, `Group`, `Foreign` or `Guide`. Absent: a shape with no stated type. No other word is read. |
| `shape` | `type-other` | text | — | no | A type that none of the four words names, as the source wrote it. Written instead of `type`. |
| `shape` | `master` | u32 | — | no | The `id` of the master this shape is an instance of. |
| `shape` | `master-shape` | u32 | — | no | The `id` of the shape inside that master that this shape stands for. |
| `shape` | `del` | flag | `0` | no | A shape inherited from a master that was deleted in this instance. |
| `shape` | `data1` | text | — | no | A free text property of the shape, as the source format keeps it. |
| `shape` | `data2` | text | — | no | A second free text property. |
| `shape` | `data3` | text | — | no | A third free text property. |
| `t` | `b64` | flag | `0` | no | The text is base64 of its UTF-8 bytes, because it holds a character XML cannot. |
| `cp` | `ix` | u32 | `0` | no | The `Character` row of the shape's sheet that applies from here on. |
| `pp` | `ix` | u32 | `0` | no | The `Paragraph` row that applies from here on. |
| `tp` | `ix` | u32 | `0` | no | The `Tabs` row that applies from here on. |
| `fld` | `ix` | u32 | `0` | no | The `Field` row that this field uses. |
| `fld` | `b64` | flag | `0` | no | The displayed text is base64, because it holds a character XML cannot. |
| `foreign` | `rel-id` | text | — | no | The relationship id of the picture or object in the VSDX package it came from. |
| `foreign` | `text` | text | — | no | Text inside the element, as the source had it. |
| `picture` | `part` | text | `""` | no | The picture's part name in the package it came from. |
| `picture` | `type` | text | `""` | no | The picture's content type. |
| `picture` | `blob` | blob | — | yes | The picture's bytes. |
| `connect` | `from-sheet` | u32 | `0` | no | The `id` of the shape the connection starts at. |
| `connect` | `from-cell` | text | — | no | The cell of that shape it starts at, by name. |
| `connect` | `from-part` | u32 | — | no | The part of that shape it starts at, a number. |
| `connect` | `to-sheet` | u32 | `0` | no | The `id` of the shape the connection ends at. |
| `connect` | `to-cell` | text | — | no | The cell of that shape it ends at, by name. |
| `connect` | `to-part` | u32 | — | no | The part of that shape it ends at, a number. |
| `sheet` | `LineStyle` | u32 | — | no | The `id` of the style the line cells of this sheet take their defaults from. |
| `sheet` | `FillStyle` | u32 | — | no | The `id` of the style the fill cells take their defaults from. |
| `sheet` | `TextStyle` | u32 | — | no | The `id` of the style the text cells take their defaults from. |
| `sheet` | `def` | text | — | no | The name by which later `same` elements of the part stand for this sheet (3.19). |
| `sheet` | `fp` | stamp | — | no | The fingerprint of the sheet as a reader of 1.0 reads it, with a `def` (3.19). |
| `same` | `of` | text | — | yes | The `def` of the sheet or the section, earlier in the part, that this stands for (3.19). |
| `Cell` | `N` | text | — | yes | The cell's name, as the VSDX format names cells (`PinX`, `Width`, ...). |
| `Cell` | `V` | text | — | no | The value, as text exactly as the source wrote it. |
| `Cell` | `F` | text | — | no | The formula, as text. `Inh` and `No Formula` are markers (section 3.9). |
| `Cell` | `FL` | text | — | no | A formula whose own text is `Inh` or `No Formula`, so that it is not read as a marker. |
| `Cell` | `U` | text | — | no | The unit of the value (`IN`, `PT`, ...). |
| `Cell` | `E` | text | — | no | The error, as text, when the cell holds one. |
| `extra` | `text` | text | — | no | Text inside the cell element, when the source had any. |
| `Section` | `N` | text | — | yes | The section's name (`Geometry`, `Character`, `User`, ...). |
| `Section` | `IX` | u32 | — | no | The section's index, when it has one. |
| `Section` | `Del` | flag | `0` | no | The section is deleted in this instance. |
| `Section` | `def` | text | — | no | The name by which later `same` elements of the part stand for this section (3.19). |
| `Section` | `fp` | stamp | — | no | The fingerprint of the section as a reader of 1.0 reads it, with a `def` (3.19). |
| `Row` | `N` | text | — | no | The row's name, for a named row. |
| `Row` | `IX` | u32 | — | no | The row's index, for an indexed row. |
| `Row` | `LocalName` | text | — | no | The row's local name. |
| `Row` | `T` | text | — | no | The row's type (`MoveTo`, `LineTo`, ...). |
| `Row` | `Del` | flag | `0` | no | The row is deleted in this instance. |
| `attr` | `name` | text | `""` | no | The attribute's name as written, prefix included. |
| `attr` | `value` | text | `""` | no | Its value. |
| `kept` | `after` | u32 | `0` | no | A number set by whoever wrote the node: how many of the owner's typed children came before the node in the source (section 3.12). |
| `el` | `name` | text | `""` | no | The element's name as written, prefix included. |
| `txt` | `b64` | flag | `0` | no | The text is base64, because it holds a character XML cannot. |
| `comment` | `b64` | flag | `0` | no | The comment is base64, because it holds a character XML cannot. |
| `pi` | `target` | text | `""` | no | The instruction's target. |
| `pi` | `b64` | flag | `0` | no | The data is base64, because it holds a character XML cannot. |
| `odf` | `mimetype` | text | — | no | A document's: the media type the OpenDocument file named. |
| `odf` | `master-page` | text | — | no | A page's: the name of its master page. |
| `odf` | `layout` | text | — | no | A page's: the name of its page layout. |
| `odf` | `stamp` | stamp | — | no | A page's: the stamp of its cells when read (section 3.17). |
| `odf` | `name` | text | — | no | A style's: the name the file gave it. |
| `odf` | `display-name` | text | — | no | A style's: its display name. |
| `odf` | `parent` | text | — | no | A style's: the name of its parent style. |
| `odf` | `family` | text | — | no | A style's: its style family. |
| `odf` | `style-name` | text | — | no | A shape's or a row's: the name of its automatic style. |
| `paragraph` | `characters` | stamp | — | no | The stamp of the paragraph's characters when read. |
| `run` | `from` | u32 | `0` | no | Where the run starts, in characters of its paragraph. |
| `run` | `to` | u32 | `0` | no | Where it ends, in characters of its paragraph. |
| `text-style` | `name` | text | — | no | The name of the style the paragraph or run names. Absent: it names none. |
| `text-style` | `stamp` | stamp | — | no | The stamp of the row's cells when read. |
| `held-style` | `group` | text | — | yes | Which part of the style the value rests on: `Size`, `Style`, `Underline`, `Strike`, `Overline`, `Case`, `Pos`, `Color`, `Letterspace`, `FontScale`, `Font`, `AsianFont`, `ComplexScriptFont`, `HorzAlign`, `Flags`, `IndLeft`, `IndRight`, `IndFirst`, `SpBefore`, `SpAfter` or `SpLine`. No other word is read. |
| `held-style` | `stamp` | stamp | — | no | The stamp of those cells when read. Absent for a value that has none. |
| `rest` | `key` | text | `""` | no | The local name of the element the object had no place for, with `/` and the `IX` for a colour entry. |
| `part` | `format` | text | — | yes | Whose rules the part belongs to: `vsdx`, `odf` or `native`. No other word is read. |
| `part` | `name` | text | `""` | no | The part's name in its package. |
| `part` | `media-type` | text | `""` | no | Its media type. |
| `part` | `role` | text | — | yes | What it is for: `thumbnail`, `macro-project`, `macro-signature`, `macro-data`, `embedding`, `object-file`, `object-replacement`, `signature`, `custom-xml`, `font` or `other`. No other word is read. |
| `part` | `blob` | blob | — | yes | Its bytes. |
| `rel` | `source` | text | `""` | no | The part the relationship comes from (`/` for the package). |
| `rel` | `id` | text | `""` | no | The relationship's id. |
| `rel` | `kind` | text | `""` | no | Its type, a URI, verbatim. |
| `rel` | `target` | text | `""` | no | Its target, verbatim. |
| `rel` | `external` | flag | `0` | no | The target is outside the package. |
| `content-type` | `extension` | text | — | no | For a default row: the extension, without its dot. |
| `content-type` | `part` | text | — | no | For an override row: the name of the part. |
| `content-type` | `content-type` | text | `""` | no | The content type. |
| `custom` | `format` | text | — | yes | The format the property came from: `vsdx` or `odf`. |
| `custom` | `name` | text | `""` | no | The property's name. |
| `custom` | `value-type` | text | `""` | no | Its type, as the source format spells it. |
| `custom` | `value` | text | `""` | no | Its value, as text. |
| `comment-list` | `show-tags` | text | — | no | `1` or `0`: whether the markers of the comments show on the page. Absent, they show. |
| `comment-list` | `part` | text | — | no | The name of the package part the comments were read from, as the source package gave it. A program that writes a package rewrites only that part. |
| `comment-author` | `id` | u32 | — | yes | The author's number, unique among the authors. |
| `comment-author` | `name` | text | `""` | no | The author's name. |
| `comment-author` | `initials` | text | `""` | no | The author's initials. |
| `comment-entry` | `id` | u32 | — | yes | The comment's number, unique among the comments. |
| `comment-entry` | `parent` | u32 | — | no | The number of the comment this one answers, which has no parent itself. Absent for the first comment of a thread. |
| `comment-entry` | `page` | u32 | `0` | no | The `ID` of the page the comment is on. |
| `comment-entry` | `shape` | u32 | — | no | The `ID` of the shape the comment is on. Absent for a comment on the page. |
| `comment-entry` | `author` | u32 | `0` | no | The number of the comment's author. |
| `comment-entry` | `text` | text | `""` | no | The comment's text, its lines parted by a line feed. |
| `comment-entry` | `date` | text | `""` | no | When it was written, as the source format wrote it. |
| `comment-entry` | `edit-date` | text | — | no | When its text last changed. |
| `comment-entry` | `done` | flag | `0` | no | The comment is marked as done. |
| `root` | `name` | text | `""` | no | The name of the root element of one of the file's XML parts. |
| `manifest-row` | `name` | text | `""` | no | The name of the entry the manifest row describes. |
| `definition` | `family` | text | — | yes | `marker`, `gradient`, `opacity`, `hatch`, `stroke-dash` or `fill-image`. No other word is read. |
| `definition` | `name` | text | `""` | no | The definition's name. |
| `definition` | `blob` | blob | — | no | A fill image's bytes. |
| `kept-style` | `family` | text | `""` | no | The style's family. |
| `kept-style` | `name` | text | `""` | no | The style's name. |
| `held` | `group` | text | — | yes | `fill`, `line`, `line-ends`, `shadow`, `text`, `frame`, `glue` or `geometry`. No other word is read. |
| `held` | `stamp` | stamp | — | no | The stamp of the cells the value stands beside, when read. Absent for a value that has none. |
| `s` | `n` | text | `""` | no | The name of the attribute whose text this holds. |
| `s` | `b64` | flag | `0` | no | The text is base64. |

### 3.19 Shared sheets and sections (added in 1.1)

A drawing often holds one sheet or one section in many places: the same path rows for every rectangle of one size. 1.1 lets a part say such a unit once and refer to it, so that the file is what the drawing holds and not what it would be if each place had its own copy.

- **Naming a unit.** A `sheet` or a `Section` may have the optional attribute `def` (text, no default). It gives the unit a **name**. A name is not empty (a writer writes a decimal number) and is the name of one unit in its part, a sheet or a section: the names of the document part, of a page part and of a master part are each their own, and **a part has one set of names for both kinds**. A unit that gives a name that an earlier unit of the part has given is **read as a unit of no name**: it is read in full, the shape or sheet it stands in keeps it, the name it did not get is counted (5.3), the name stays the first unit's, and a `same` that names it takes the first.
- **The fingerprint of a unit.** A `def` MAY be accompanied by the optional attribute `fp` (a stamp, 3.17: 16 lowercase hexadecimal digits). It is the stamp, of domain `unit`, **of the unit as a reader of 1.0 reads it**, so that an edit by a program of 1.0 shows (5.4). The fields, in order, are those of the element of the unit and of every element under it, in document order: the text field `<`, a text field of the element's local name (the prefix is not in it), then for each attribute a text field of its name as written and one of its value, **leaving out** the `xmlns` declarations and, on the unit's own element, `def` and `fp`; then the children in order, and the text field `/`. A text node that is not all white space gives the text field `t` and one of its text; a comment and a processing instruction give nothing. A `same` inside the unit gives, in the place of an element, the text field `same` and a text field of the fingerprint of the unit it names, as 16 digits (`?` if it names no unit). A writer of 1.1 SHOULD write `fp` on each unit it names; a reader of 1.1 that reads a named unit computes the fingerprint of the tree it read, and **a unit whose `fp` is there and is not that is read as the file has it, but its name is not given** (that is counted, 5.3): the `same` elements that name it name no unit (next point). A file without `fp` reads as it did. Vector 8.3 gives one.
- **Standing for a unit.** The element `same` has the required attribute `of` (text) and holds nothing. Where a `sheet` may stand (in a shape, a page, a master, a style or the document), a `same` stands for a copy of the sheet whose name is `of`; inside a sheet, where a `Section` may stand, it stands for a copy of the section whose name is `of`. The unit MUST be named by an element **earlier in the same part, in the order of the document**, and be of the kind the place holds: a reader MUST NOT look for it later in the part or in another part. A shape that holds a `sheet` and a `same` for its sheet has the first; the second is not read and is counted (5.3).
- **A `same` that names nothing is not damage.** A `same` that names no unit (no unit of that name is named earlier in the part, the unit of that name is of the other kind, or its name was not given because its fingerprint did not match) is **not read and is counted** (5.3), and the file opens: the place it stands in is read as it is read when it holds nothing, a shape with no sheet and a sheet without that section. A program of 1.0 makes such a file in the ordinary way: it changes the order of the shapes, deletes the shape that holds a `def`, or copies a shape that holds one (two units of one name: the copy is read as a unit of no name, and its shape keeps its sheet). A `same` with no `of` is damage, as is an element that lacks any attribute it must have (3.1).
- **What it means.** The unit is what the named element decodes to, as it stands where it was named: a named sheet that holds a `same` for a section is the sheet with that section. The drawing holds a copy where each `same` stands. A program MAY make the copies one allocation, and a change to one place MUST NOT change another.
- **What a writer does.** A writer SHOULD name a sheet or a section that its model holds in more than one place of a part, **whether the places hold one allocation or equal copies of it** (two units are equal when every field is, never because two hashes agree), write it in full at its first place, and write a `same` at each place after it; it MUST NOT name a unit that one place holds, and it MUST NOT name a unit inside kept data (3.12) or unknown content. The names of a part are the numbers `1`, `2`, ... in the order the units were written.
- **Stamps and the content check do not change.** A `same` is replaced by the unit it stands for before the content of the drawing is looked at, and a `def` and its `fp` are not content: the stamps of 3.17 and the check of section 4 are over the content and never over how a part says it, so a drawing has the stamp it had when each place held its own copy, and two files that differ only in `def`, `fp` and `same` are the same content.
- **What a reader of 1.0 does.** It reads `def`, `fp` and `same` as unknown (5.3): the drawing opens; a shape that said its sheet with a `same` has no sheet, a sheet that said a section with a `same` has no such section, and a note says that content was not read. It keeps what it did not read and writes it back where it was, so a file it opens and saves without changing the named units reads as before in a reader of 1.1. **A change that a program of 1.0 makes to a named unit, or to the order of the places, is found by a reader of 1.1**: a unit that was edited no longer matches its `fp`, so its name is not given and the places that stood for it are counted and read as none, and a `same` before its unit, or after the unit was deleted, is counted too; a unit that a second unit gives its name again to is read as a unit of no name and counted. Nothing is read as another unit than the one named, no unit that was written is lost, and nothing is damage.
- **Limits.** How many units a part names is bounded by the items of the part (6.1). A reader charges each name it keeps and MAY refuse a part that names more than it can hold, in words (6.4).

## 4. Same content

### 4.1 What it is for

A drawing can leave a VSDX file or an OpenDocument drawing, live in a `.diacraft`, and be written back. What the person is promised is not that the new file has the same bytes. It is that it has **the same content**: every part, element, attribute and value, and the same bytes for every picture, embedded object, macro project, font and thumbnail. This section defines "the same content" as one comparison of two files of one format. A program that reads and writes VSDX files or OpenDocument drawings through `.diacraft` uses it to check what it keeps.

The new file differs as bytes. XML spacing, the order of attributes, the prefixes given to namespaces, the spelling of some numbers, the order of entries, the compression, and the dates are not content, and a file written again may change them. A digital signature over parts no longer matches, because the parts' bytes changed, and a program that writes the signature parts back says so.

The check never calls two different contents the same. Every rule below that lets two spellings pass has a pair that differs and that the check still tells apart. Where this document is unsure, the check says "different" and not "same".

### 4.2 Parts

- **Which parts exist.** The two files have the same set of part names. For a VSDX package, names are compared the way the Open Packaging Conventions compare them: by ASCII case-insensitive matching (ECMA-376 Part 2, 6.2.2.3), so that `/X/Page1.xml` and `/x/page1.xml` are one part and non-ASCII letters do not fold. For an OpenDocument package, entry names are compared exactly, folders included. A part on one side only is a difference, with three exceptions: in a VSDX package, a relationships part on one side only that holds no row is not (4.4), and neither is one whose only rows are rows that a page's own parts imply (4.4); in an OpenDocument package, a `settings.xml` or a `meta.xml` on one side only that holds nothing is not (4.5); and in an OpenDocument package, an entry under `DiaCraft/` that only the second package has is not (4.5). A part of any other name is a difference whatever its root holds.
- **XML or bytes.** A part is compared as XML when it is XML and parses on both sides, and byte for byte otherwise (pictures, embedded objects, macro projects, fonts, thumbnails, unknown binary parts). A part meant as XML that does not parse on either side is compared byte for byte, and so is one that breaks a constraint of XML Namespaces 1.0 (4.3). **One exception, for the manifest of an OpenDocument package**: it may begin with one DOCTYPE that holds only a name and an external id and no internal subset (the file of an old OpenOffice.org writer has one), which declares nothing and is read and left out, so that such a manifest is read as one that has none and the DOCTYPE is no content. Every other DTD, in the manifest or in any other part, is not read.
  - In a VSDX package a part is XML when its content type, with any parameter after `;` dropped and lowercased, ends in `xml` (which covers `+xml`) and does not start with `image/`. A picture of type `image/svg+xml` is therefore bytes. A part with no content type is XML only if its name ends in `.rels`. The content-types stream, which is not a part, is always XML.
  - In an OpenDocument package a file is XML when it is `content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, `META-INF/manifest.xml`, any other `.xml` file under `META-INF/` (the signatures), or has a manifest row whose media type ends in `xml` and does not start with `image/`.
- **Not content.** The order of the entries, the compression method and level, the dates, extra fields, the archive comment and data descriptors.
- **Too large to say.** A part that is past the checker's limits (4.8) is refused with a word about it. The check does not answer about what it did not read.

### 4.3 XML parts

Both sides are parsed with bounded limits. Then:

- **Ignored:** the XML declaration; anything outside the root element; comments anywhere (`a<!--x-->b` is `ab`); every namespace declaration. **Compared:** processing instructions inside the root, because a program may act on them (a VSDX `NewValue` instruction is one).
- **Counting as nothing when extra (a format's list; the check is then oriented).** A format may list namespaces whose attributes and elements count as nothing when only the **second** document has them, the one the first became (an OpenDocument writer's own markup, 4.5). What the first document holds of such a namespace must be in the second (an attribute with its value, an element equal to one of it); what only the second holds is no difference. So **the count of `compare(a, b)` is not that of `compare(b, a)`** for a document that has such markup: the first file is the one the drawing came from. A format that lists none is symmetric, as 4.3 says everywhere else. Such an attribute is left out of what aligns the children, so a child that was marked is still the child it was.
- **Spellings of one namespace (a format's list; APPID).** A format may list pairs of spellings, the earlier one and the one written now, and then each part of both documents that is XML meant as such and valid UTF-8 is read with the earlier spelling put as the other **wherever it stands** (a declaration, an attribute's value, a text), before it is read as XML; a pair whose second spelling is longer than its first is not applied. The OpenDocument list and the VSDX list name DiaCraft's namespace `urn:io.github.one_bit.diacraft:extension` as the one `urn:app.diacraft:extension`, and with it the type of the relationship to a carried part (7.2), which holds the namespace: **a file written before the id moved and the same drawing written again are the same content**, in the carrier and in the VSDX solution part. Every other difference between them is still one, and a count is the same whichever document is first.
- **Counting as nothing (a format's list).** A format may list attributes that count as nothing wherever they stand (an OpenDocument `office:version`) and children that count as nothing under one parent (an OpenDocument `meta:generator` under `office:meta`). Such an attribute is left out of what its element is compared by, and such a child is left out of its parent, before anything else is read; neither is content of an element that a rule asks to hold nothing (4.7). A format lists one only on a ruling that says its value is no fact about the document (4.5).
- **Names** of elements and attributes are compared as a pair (namespace URI, local name), so prefixes do not matter. An unprefixed element is in the default namespace in scope, and an unprefixed attribute is in no namespace. A prefix that no declaration binds is itself the name: `a:x` is not `b:x` and is not the `x` of any namespace. The nearest declaration decides, as XML Namespaces 1.0 says, wherever the check reads a name, the names its rules list included (a child that counts as nothing, the root of an empty part and its children, the container of free names): `xmlns=""` puts an element and its unprefixed descendants in no namespace until a declaration binds the default again.
- **A part that breaks a namespace constraint is bytes.** A part that breaks a constraint of XML Namespaces 1.0 is compared byte for byte, as a part that does not parse is: a prefix declared as the empty URI, `xml` bound to another namespace or its namespace bound to another prefix or as the default, the prefix `xmlns` declared or its namespace bound, an element of the prefix `xmlns`, and a name that is not a qualified name (`:x`, `x:`, `a:b:c`). Readers read these each their own way, so such a part is the same content as another only when the bytes are the same. A prefix that no declaration binds is not one of these: it is compared as its name, as above.
- **Attributes** are a set: their order is not content. Two attributes that name one thing under different prefixes (which XML Namespaces 1.0 does not allow) are compared in the order written, since a reader takes the first: the two values swapped are a difference.
- **Attribute values** are text, after the XML parser's own normalization, and are compared as text. There are three exceptions, and each applies only to the attributes a format lists (4.4, 4.5). Anywhere else `007` and `7` differ.
  - **Numbers.** Two values are the same number when each reads whole as an optional sign, digits, an optional fraction (a point and at least one digit) and an optional exponent, followed by a unit that is empty, `%` or ASCII letters, and the numbers are the same finite double-precision value and the units are the same text. So `1.50`, `1.5` and `15E-1` are one number, and `2cm` and `20mm` are not (units are never converted). `.5` and `5.` are not numbers and are compared as text. A value that reads as infinity, or as zero though it has a digit that is not zero, is text. `-0` is not `0`: a sign is content.
  - **Lengths.** A length is read as follows, which is stricter than the pattern of the ODF schema (that pattern also accepts `.5cm` and `5.cm`; here they are not lengths, so they differ from `0.5cm` and `5cm`, which is the safe side): an optional `-`, digits, optionally a point and digits, and directly after them a unit, `cm`, `mm`, `in`, `pt`, `pc`, `px`, or `%` for a percentage, in lower case (ODF 1.3 Part 3, 18.3.18 and 18.3.23, and the schema). Two lengths are the same when the numbers are the same value and the units are the same text. There is no `+` and no exponent: `+2.5cm` and `25e-1cm` are not lengths, so they are text and differ from `2.5cm`. A value that does not read as a length is the same only as itself.
  - **Qualified values.** A few attributes hold prefixes or prefixed names (4.4). They are read through the namespaces in scope: a prefix is its namespace's URI. A prefix that is renamed passes. One that is declared again as another namespace, or no longer declared, is a difference.
- **Text** is compared after CDATA sections, character references and entities are resolved, and text separated only by a comment is joined.
  - **Blank text** is ignored only in an element that has a child element or instruction and no text that is not blank, and that is not in the *mixed* list of the format (4.4, 4.5). A child that counts as nothing, or that counts as nothing when extra, is a child here as it is written: the blank text beside it is spacing. In every other element the text is compared exactly. `xml:space` is not read: it is an attribute like any other.
  - In the **paragraph elements** of OpenDocument (4.5), white space is compared as ODF 1.3 Part 3, 6.1.2 reads it: tab, carriage return and line feed are spaces, the spaces at the start and the end of the paragraph are dropped, a run of spaces is one space, and a `text:s` element stands for `text:c` spaces (one when `text:c` is absent). The elements `text:tab` and `text:line-break` stand for the tab and the line break they name, and are not spaces, so a `text:tab` is not the same as a raw tab character. The check does not collapse white space across an element inside the paragraph, so it may keep apart two paragraphs that ODF reads alike, and never joins two that ODF reads differently.
  - Numbers in text are never respelled. The number rules are for attribute values only.
- **Children** are compared in order, with these exceptions: the siblings of the format's **keyed** list (4.4, 4.5) are a set, compared by their key, whatever order they stand in. Siblings of one key are compared in the order written. Children that are not keyed keep their order among themselves, and are set side by side by what they are and not by the place they stand in (the next item). Everything else keeps its order: pages, shapes (their order is the drawing's order), rows, paragraphs and runs, the layer set; a different order of them is one difference, the order.
- **Children that are not keyed are set side by side by what they are.** The children at the start of two lists that are equal, and those at the end, are matched at once. Of what lies between them, each child is matched by three keys in turn, each among the children the keys before it did not match: **what it holds** (the same content); **which thing it is** (its name and the attributes the format lists as naming a thing, 4.4 and 4.5: a page's or a shape's own name or identifier); and **what it is called**, when its name is the only one of its kind in the list on each side. The first two match only where both lists hold the same number of children of a key, the first with the first, the second with the second, and so on; where the numbers differ nothing says which child was put in or taken out, and none is matched by that key. **Each pair is compared in full**, so that what is inside a child is found whatever stands before it. Where the pairs are not in the same order in both lists, the same children are in another order, and that is **one** difference, the order, however many cross. The children between two matched pairs that no key matched are set side by side by place, the first of one side with the first of the other, which is a replaced child (one difference), and what is left of one side is put in or taken out. The result does not depend on which document is first. A list of more than 262,144 children between its matched start and end, on either side, is not set side by side by what they are but by place, as lists were before; the count then may be higher than the changes (a child put in shifts what follows it), never lower, and the checker counts the children it set so (4.8).

**What a difference is.** A difference is one of: a part missing, a part extra, bytes, an element, an attribute, a value, text, an instruction, or an order (the same children in another order, reported once at their parent). A child on one side only is one difference, however much it holds. One element added or taken out is one difference and not a shift of all that follows it: every sibling after it is still compared, in depth, with the sibling that is its own. A program shows the first differences (DiaCraft shows 200) and counts all of them exactly; the count is the same whichever file is first. A difference is located by element names and positions only: no value, key, text or name a person chose appears in it.

### 4.4 The lists for VSDX packages

- **Keyed siblings.** Each is a set because the order of the rows has no meaning, or because the writer's order is not the source's.
  - `Cell`, by `N`, in whichever namespace a VSDX drawing part uses for its elements, and in none. The model keeps a sheet's cells by name, so a writer writes them in the order of their names and not the file's, and a cell is found by its name and never by its place. Every other child of a shape, section or row stays in order: shapes are the drawing's order, and rows are rows. *Source: a rule of this format.*
  - Content-type `Default` rows, by `Extension` (compared as ASCII case-insensitive, as an OPC reader folds an extension), and `Override` rows, by `PartName` (folded as an OPC reader folds a part name: a leading `/` added when there is none, a `%` escape decoded when it decodes to UTF-8 and hides no `/` or `\`, and ASCII letters lowered). *Source: ECMA-376 Part 2, 7.2.3.2.1: the order of Default and Override elements is not significant.* Rows an OPC reader takes for one (`/a.xml` and `/A.XML`) are one key, and are compared in the order written, because a reader takes the first.
  - `Relationship` rows, by `Id`. *Source: ECMA-376 Part 2, 6.5.3.4 identifies a relationship by its `Id`, unique within the part, and nowhere makes the order of the rows meaningful.*
- **What says which thing an element is.** `ID`, `N`, `IX`, `NameU` and `Name`: an element that has one of them is told from its siblings by the name of the element and the values of the ones it has (4.3, "Children that are not keyed are set side by side by what they are"), so that a shape put in, taken out or written in another place is one difference and the shapes around it are compared with their own. *Source: a rule of this format, which chooses what sets the children side by side and never what makes two of them the same: every pair is compared in full.*
- **Numbers and lengths: none.** The model keeps every VSDX value as the text the file gave, so no writer respells one. `V`, `F`, `U`, `E`, `IX`, `ID` and `N` hold text and are never listed: if `V` were listed, `007` changed to `7` would pass as the same content.
- **Qualified values.** `xsi:type` (a prefixed name, such as `dcterms:W3CDTF`), the markup-compatibility attributes `Ignorable` and `MustUnderstand` (prefixes) and `ProcessContent` (prefixed names), and `Requires` on a markup-compatibility `Choice` element (prefixes). *Source: ECMA-376 Part 3, 7.2 (`Ignorable`), 7.3 (`ProcessContent`), 7.4 (`MustUnderstand`) and 7.6 (`Choice` and its `Requires`), and, for `xsi:type`, the schema instance namespace of W3C XML Schema Part 1. A prefix declared again changes what the part says.*
- **Mixed content: `Text`.** The `Text` element of a shape, in any VSDX namespace and in none, holds both text and marker children (`cp`, `pp`, `tp`, `fld`). A single space beside a marker is content and not spacing, so blank text is never ignored in it.
- **Paragraphs: none.**
- **Empty containers** (4.7). `Colors`, `FaceNames`, `StyleSheets` and `DocumentSheet` of the document part, and `Connects` and `Shapes` (a page's, a master's and a group's), in each VSDX namespace and in none, are the same as absent when they are empty: no attribute, no child element, no processing instruction and no text but white space. One that holds anything is compared as every element is. **Only under its own parent:** the first four as children of the document part's root element, `Connects` and `Shapes` as children of a page's root `PageContents` or a master's root `MasterContents`, and `Shapes` as a child of a `Shape` (a group), the parent in any of the three namespaces too. The same name, empty, under any other element (a `Shapes` inside an element no reader of the format reads, a `Colors` in a page) is an element of the part and is compared as it is. *Source: a rule of this format (2026-11-21). Each holds a collection and nothing else: no colour, face, style, cell or row, shape or connection is in an empty one, and none is in an absent one.*
- **An empty relationships part.** A relationships part on one side only whose root is `Relationships` in the relationships namespace and holds nothing (no row, no attribute, no instruction, no text but white space; a comment is no content) is not a difference. A relationships part is one by its name, as OPC names it (ECMA-376 Part 2, 6.5: `_rels/<last segment>.rels` beside its source, and `/_rels/.rels` for the package; read as `.rels` at the end of a name in a folder `_rels`, ASCII case folded as part names are). **A part of any other name is a part whatever its root holds**, an empty `Relationships` root included: a reader keeps a part it does not know, so a part dropped is a difference. One that holds a row, or anything else, is a missing or an extra part, and so is one that does not parse or that is larger than 64 KiB (DiaCraft's checker reads no larger one for the rule; a larger empty one is a difference, the safe side). *Source: a rule of this format, resting on ECMA-376 Part 2, 6.5.3: the relationships of a part are the rows of its relationships part, so a part whose relationships part holds none has the relationships of a part that has no relationships part.*
- **The order of a shape's sections, text and foreign data.** Under a `Shape`, its `Section`, `Text` and `ForeignData` children are compared in that order, wherever they stand among the shape's children, and **the sections keep their order among themselves**. Every other child of a shape (`Shapes`, `Data1` to `Data3`, an element of another namespace) keeps its place, and `Cell` is keyed as above. *Source: a rule of this format. Each of the three is read by its name, once, and its place among the others says nothing to a reader of the format; a writer of the model writes the sections, then the text, then the foreign data (the order of the schema), and files hold the text first, or the foreign data before the text.*
- **A relationship row that the page's parts imply.** In a relationships part of a page, a `master` row (the relationship type `.../relationships/master` of the format, exactly; internal) to the part of a master that a `Shape` of the page names by its `Master`, a shape inside a group included, is not a difference when the other package has no row of that type to that part and no row of that `Id`; nor is a `page` row (internal) to the part of the page that the `BackPage` of the page's row of the pages index names, on the same condition. A master's part is the one that its row of the masters index leads to, and the pages index and the masters index are those the document part's relationships `pages` and `masters` reach. A row to a master no shape uses, to a part that is not the background page's, a `page` row for a page that says no `BackPage`, an external row, a row of another type, another `Id` for a part the other package has a row for, and a relationships part larger than 1 MiB are compared as every row is; the relationships part that is left with no row is an empty one (the rule above). *Source: a rule of this format. A page is given these rows, with no `r:id` to them, from the `Master` values of its shapes and its `BackPage`, and a writer rebuilds them from those; the row says what the shapes already say, so none is added or lost. A `page` row is kept where the page says no `BackPage` and does not know why, so that row is not implied.*
- **A part's content type given where it had none, and the types of two kinds of part.** Each part both packages hold has the content type of its `Override`, else of the `Default` of its extension (ECMA-376 Part 2, 10.1.2.2). It is not a difference when a part has no type in one package and one in the other (the row that gives it the type, an `Override` or a `Default` that gives a type to no other part, is left out of the comparison; the type given is not judged); when a part that a package relationship of the types `extended-properties`, `custom-properties` or `core-properties` reaches has one of the six types of the drawing's document in one package and the type of its kind in the other, each by an `Override`; or when a relationships part has the relationships media type by an `Override` in one package and by the `Default` for `rels` in the other. The same part typed by an `Override` in one package and a `Default` in the other (any other part), two different types for a part that has one in both, a part with no relationship of its kind, a type of another kind, a row for no part and a `Default` that gives a type to no part both hold are compared as every row is. *Source: a rule of this format. An untyped part is not legal OPC and a writer that gives it a type adds a fact the file's package lacked, not content; a property part is the part its relationship says, and a file that types it as the drawing names two main parts; a relationships part needs no row of its own from the 4th edition of ECMA-376 on, and the `Default` for `rels` meets every edition.*
- **A `Row` with no `IX`.** Under a `Section` in each VSDX namespace and in none, a `Row` that has no `IX` is read as having it, with the number of `Row` children before it in that section. Which rows are read so depends on the section's `N`: every row of `Character`, `Field`, `FillGradient`, `Geometry`, `Layer`, `LineGradient`, `Paragraph`, `Reviewer`, `Scratch` and `Tabs`; none of `User`, `Property`, `Actions`, `Control`, `Hyperlink` and `ActionTag`; and, in any other section, a row that has no `N`. An `IX` that is written is compared as written (`01` is not `1`). *Source: a rule of this format, resting on MS-VSDX: `IX` rises through a section and, absent, is the count of the earlier rows; DiaCraft's reader keys the rows of those sections by it.*

### 4.5 The lists for OpenDocument packages

- **Keyed siblings**, each compared by exact text (names and paths are compared as written):
  - manifest rows `manifest:file-entry`, by `manifest:full-path` (ODF 1.3 Part 2, 3.2: one row for each file, named by its path);
  - styles `style:style`, by `style:family` and `style:name` (ODF 1.3 Part 3, 19.502: the family and the name identify a style, and `office:styles`, `office:automatic-styles` and `office:master-styles` each hold no two of one pair);
  - font faces `style:font-face`, by `style:name` (19.502.3);
  - the named definitions `draw:marker`, `draw:gradient`, `draw:opacity`, `draw:hatch`, `draw:stroke-dash` and `draw:fill-image`, each by `draw:name`. *Source: a rule of this format. ODF 1.3 Part 3, 16.42.1 to 16.42.9 and 19.197, gives each of these elements its `draw:name` and has `office:styles` hold them (3.15.2); it says nothing of their order.*
  - **Not keyed**, because no source says their order is meaningless, and a false difference is safe where a false same is not: the pages, the shapes, the layer set `draw:layer-set` (the order of its layers is content), the master pages and page layouts, `style:default-style`, list styles and data styles.
- **What says which thing an element is** (4.3, "Children that are not keyed are set side by side by what they are"): `draw:name`, `draw:id`, `xml:id`, `style:name`, `style:family`, `table:name` and `config:name`. An element that has some of them is told from its siblings by the name of the element and the values of the ones it has, so that a page, a layer, a shape, a master page or a setting put in or taken out is one difference and the siblings around it are compared with their own. *Source: a rule of this format, which chooses what sets the children side by side and never what makes two of them the same: ODF 1.3 Part 3 gives each of these attributes to the element it names, and every pair is compared in full.*
- **The manifest's DOCTYPE** (4.2): `META-INF/manifest.xml` may begin with one DOCTYPE that holds only an external id; it is no content.
- **Lengths** (strict, as 4.3): these attributes, each in the ODF 1.3 namespace and in the older W3C one that old files use (`svg` is `http://www.w3.org/2000/svg` there, and `fo` is `http://www.w3.org/1999/XSL/Format`), 70 names in all:
  - `svg:x`, `svg:y`, `svg:width`, `svg:height`, `svg:x1`, `svg:y1`, `svg:x2`, `svg:y2`, `svg:cx`, `svg:cy`, `svg:r`, `svg:rx`, `svg:ry`, `svg:stroke-width`;
  - `fo:padding-top`, `fo:padding-bottom`, `fo:padding-left`, `fo:padding-right`, `fo:margin-left`, `fo:margin-right`, `fo:margin-top`, `fo:margin-bottom`, `fo:text-indent`, `fo:line-height`, `fo:letter-spacing`, `fo:font-size`, `fo:page-width`, `fo:page-height`;
  - in the ODF 1.3 namespaces only: `draw:marker-start-width`, `draw:marker-end-width`; and the percentages `draw:opacity`, `draw:shadow-opacity`, `draw:start-intensity`, `draw:end-intensity`, `draw:border`, `draw:cx`, `draw:cy`, `draw:dots1-length`, `draw:dots2-length`, `draw:distance`, `style:text-scale` and `style:text-position`. A percentage compares as a number: `75%` and `75.0%` are the same, `75%` and `76%` are not.
  - These are the attributes an OpenDocument writer respells (the shortest decimal that reads back, in the attribute's own unit), which is why only they may be respelled. An attribute that holds an integer, an index, an angle, a name or an identifier is not listed: `007` is not `7`. Neither are `fo:padding`, `draw:corner-radius`, `svg:viewBox`, `svg:d`, `draw:points` and `draw:transform`, which hold text that has numbers in it.
- **Numbers** (the looser reading of 4.3): `svg:stroke-opacity`, in the two `svg` namespaces: a number from 0 to 1 or a percentage. `0.5` is `0.50` and `75%` is `75.0%`.
- **Qualified values:** none.
- **Paragraph elements:** `text:p`, `text:h` and `text:span`.
- **Mixed content** (blank text is never ignored): `text:a`, `text:meta`, `text:meta-field`, `text:ruby-base` and `text:execute-macro`. In them a lone space between two spans of a link is a space and not indentation.
- **Attributes that count as nothing wherever they stand** (4.3): `office:version`, on the root of every part and of a flat document, and `manifest:version`, on the manifest's root and on its rows (the root row says the version of the document, an embedded object's row the version of the object). *Source: a rule of this format: a program that writes a drawing says the one version it writes, which is no fact about the drawing.*
- **A child that counts as nothing:** `meta:generator`, under `office:meta`, the name of the program that wrote the file (an element of the metadata of ODF 1.4 Part 3). A generator under any other parent is the element it is. *Source: a rule of this format: a program says its own name when it writes the file.*
- **Fields in any order:** the children of `office:meta` are a set by their names: `dc:title`, `dc:description`, `dc:subject`, `meta:keyword`, `meta:initial-creator`, `dc:creator`, `meta:printed-by`, `meta:creation-date`, `dc:date`, `meta:print-date`, `meta:template`, `meta:auto-reload`, `meta:hyperlink-behaviour`, `dc:language`, `meta:editing-cycles`, `meta:editing-duration`, `meta:document-statistic` and `meta:user-defined` may stand in any order among themselves, **the copies of one name in the order written** (the keywords of a document are a list, and the user-defined fields are compared as they stand), and a child that is none of them keeps its place among the others. A field added, taken out or changed is a difference. *Source: a rule of this format on the schema, where `office:meta` is an interleave.*
- **Empty parts**: a `settings.xml` whose root holds nothing but an `office:settings` that holds nothing, and a `meta.xml` whose root holds nothing but an `office:meta` that holds nothing (the generator and the version counting as nothing), are the same as no such part, whatever empty form each takes, and the manifest's row for it goes with it. A part that holds a setting or a property, a part that does not parse, a part of more than 64 KiB and a part of any other name are parts. Under the root of a flat document (4.6), an `office:settings` that holds nothing is none.
- **Rows of the manifest for what the entries say**: the manifest's rows are compared as a set by their full path, so their order is no content (above). A row that the other manifest has no row of the path of is no difference when it names **an entry that only its package holds**, a file or a folder (the row says what the entry says, and the entry is the one difference: the thumbnail a writer adds, the file of DiaCraft's own data), or **a folder that files of its package stand under** and is a plain one: it states no media type. (The rows of DiaCraft's own entries and of its folder are the carrier's, below: a row of the first manifest is compared as written.) A folder row with a media type that names a kind (an embedded object's folder), a folder no file stands under, a row for a path that is no entry, a row for an entry that both packages hold and the other does not list, and a row that both manifests have and that differs in its media type or in any attribute but its version are differences. *Source: a rule of this format: a program that writes a package lists the folders its files stand in, and the files it adds, which the package it started from did not list.*
- **DiaCraft's own carrier counts as nothing when only the written file has it** (the check is oriented, 4.3). The carrier is **the entries under the folder `DiaCraft/`** (compared ASCII case-insensitively, as a reader compares them, the folder included), **their rows in the manifest** (the row of the folder with any media type), and **the attributes and elements of DiaCraft's namespace** (7.3: `urn:app.diacraft:extension`, and `urn:io.github.one_bit.diacraft:extension`, which the check reads as the same, 4.3). Such an entry, row, attribute or element that only the second package has is no difference. **Everything the first package held is compared as written**: an entry under `DiaCraft/` of the first that the second lacks is a missing part, one that both have is compared as any entry (a part that is XML as XML, a picture byte for byte), an attribute of the first that the second lacks is a difference, and so is an element of the first that no equal element of the second answers. So **an object that rides only in an entry under `DiaCraft/` still counts**: the entry it had in the first package is missing from the second. The markup of DiaCraft's namespace in a copy of an automatic style that is left out as one of its class (below) is not compared. *Source: a rule of this format: what a writer adds of its own is no fact about the drawing, and what the source held must come back.*
- **An `xml:id` beside an equal `draw:id` counts as nothing when only the written file has it** (the check is oriented, 4.3). A file of ODF 1.0 or 1.1 names a shape by `draw:id` alone, and the file a visit writes says ODF 1.4, which takes `draw:id` only beside an `xml:id` (the two with equal values are one id). An `xml:id` that only the second file has is no difference **while its element holds a `draw:id` of the same value** (names read through the declarations in force). **Everything the first file held is compared as written**: an `xml:id` of the first that the second lacks is a difference, so is one whose value is not its element's `draw:id`, and so is the `xml:id` of an element that holds no `draw:id`. The attribute is left out of the hash that aligns the children. *Source: a rule of this format, on ODF 1.4 Part 3 (`draw:id` is deprecated in favour of `xml:id`) and the schemas of ODF 1.0 and 1.1, which have no `xml:id`.*
- **Names that are free: automatic styles** (and, for the copies and the references between definitions; ODF 1.4 Part 3, 19.502.2: producers may name automatic styles freely). The children of the one `office:automatic-styles` of a part (or of a flat document) are **definitions**: `style:style` (told by `style:family` and named by `style:name`), `text:list-style`, `style:page-layout`, the data styles (`number:number-style`, `number:currency-style`, `number:percentage-style`, `number:date-style`, `number:time-style`, `number:boolean-style` and `number:text-style`) and `number:num-list-format` (each named by `style:name`). A **reference** names one by its kind, its family and its name, and the table is the one the ODF 1.4 schema and Part 3 give (checked it, §19.219, §19.227, §19.426, §19.508 and §19.880): `draw:style-name` asks a drawing-page style of `draw:page`, `style:master-page`, `presentation:notes` and `style:handout-master` (§19.219.25 and §19.219.26 for the last two), and a graphic style of any other element; `presentation:style-name` a presentation style; `draw:text-style-name` a paragraph style; `text:style-name` a paragraph style of `text:p` and `text:h`, a text style of `text:span` and `text:a`, a list style of `text:list`, a section style of `text:section` and a ruby style of `text:ruby`; `text:visited-style-name` a text style of `text:a`; `text:cond-style-name` a paragraph style of `text:p` and `text:h`; `style:page-layout-name` a page layout of `style:master-page`, `presentation:notes` and `style:handout-master`; and the **lists of names** `text:class-names` (paragraph styles of `text:p` and `text:h`, text styles of `text:span`), `draw:class-names` (graphic styles) and `presentation:class-names` (presentation styles), in which each name is read on its own. Two definitions are **the same when their content is the same** (every attribute but the name, and every child, as 4.3 reads them, **with every reference inside them resolved to the content of what it names**, below), and a reference is the same as one to a definition with the same content: a definition that has a match on the other side, and every reference to it that asks for its kind and family, is read as its content and not as its name. **Copies of one definition on one side count as one** (a writer's copy for each page, a style named under two names): the first stays and the others are left out, whether or not the other side has the style, and a reference to any of them is a reference to that content, so one shared style in the first document and a copy of it for each shape in the second are the same content. **A common style's name is kept** (a `style:style` of `office:styles` is compared by its family and name), so is the name of a master page and every name that is none of the above. A definition that has no match keeps its name, so a style whose property changed under its name is **one difference in that style**; a name that two definitions of one kind and family hold (the container is not valid), a document with two containers of automatic styles, and a name that no definition has, are compared as written; so is a name inside an embedded `office:document`. A reference that asks for a family the definition of its name has not is no reference to it: a shape's `draw:style-name` that names a paragraph style of an automatic style's name does not make the two documents the same. **References from one definition to another** are resolved the same way, to the content at the end of the chain: `style:list-style-name` of a `style:style` names a list style; `style:data-style-name` and `style:percentage-data-style-name` a data style; `text:style-name` of `text:list-level-style-bullet`, `text:list-level-style-number` and `text:outline-level-style` a text style; `style:num-list-format-name` of a `text:list-level-style-number` a `number:num-list-format`. **`style:parent-style-name` is not one** (§19.510: the parent of a style is a common style, whose name is kept), and neither is `style:next-style-name`. A definition that names another reads, in a signature of its content, the content of the one it names; **a chain of more than 64 definitions, a circle, and a definition that reaches one are compared as written** (a false difference, never a false same), and so is a name that a definition inside the chain names that no definition has. A reference in a definition that has no match to a definition that has one is still read as the content, so a name that stands for a style in one file and for none in the other is a difference. The count of differences stays exact. The references the schema allows that this does not read (`text:style-name` of the index and table-of-contents elements and their templates, of `text:ruby-text`; the references of data styles to one another, and of a list level's number format beyond the above) are compared as written. A container that weighs more than 64 MiB (a bound on what its content is written out as, charged before anything is built) has every name of both documents compared as written, whichever is first. *Source: a rule of this format.*
- **Not the same, as before** (keep them): a layer set added where the source had none (`draw:layer-set` is not an empty container, and the order of its layers is content), `meta:user-defined` fields added, `dc:creator` or `meta:initial-creator` taken out, `meta:creation-date` added, `meta:print-date` taken out, an automatic style the writer left out because nothing named it, DiaCraft's own markup that the source held and the written file lacks, and every other difference.

### 4.6 An OpenDocument package against a flat file

A `.fodg` file is the same document as an `.odg` package in one XML file. A package and a flat file are compared as **one joined document**. This is so because a package may hold `office:automatic-styles` and `office:font-face-decls` in both `content.xml` and `styles.xml`, and a flat file holds each once, so the content cannot be compared part by part. The check judges content, not where the content is placed.

Both sides are put in the form of one flat document, the root `office:document`, and compared as one part:

- A flat file's children of the root are put in the order the standard gives (`office:meta`, `office:settings`, `office:scripts`, `office:font-face-decls`, `office:styles`, `office:automatic-styles`, `office:master-styles`, `office:body`): their order in the file is not content. A child that is repeated, or is not one of those eight, is kept as it is after them and is never merged.
- A package is read into the document its parts give: `office:meta` from `meta.xml`, `office:settings` from `settings.xml`, `office:scripts` and `office:body` from `content.xml`, `office:styles` and `office:master-styles` from `styles.xml`, and the `office:font-face-decls` and `office:automatic-styles` of the two parts joined, those of `styles.xml` first. The root carries the media type the package states (its `mimetype`, else the root row of its manifest) and the `office:version` its parts state.
- A child that leaves its part carries the namespace declarations that its own names use, so a prefix keeps its meaning.
- An `office:settings` under the root that holds nothing is none, as an empty `settings.xml` is no part, and an `office:meta` that holds only a generator holds nothing (4.5). The names of automatic styles are free in the one joined container as in a part, and DiaCraft's attributes and elements that only the second document has count as nothing (4.5).
- A font face that stands in both parts and is exactly the same element is one face. One that differs in an attribute is two.
- **A name that both parts hold is a clash**: an automatic style or page layout of one name in both parts, or a face that is not the same face. A reader of a package takes the content's own, and a reader of a flat file takes the first, so the two cannot be told alike. A package with a clash is never the same as any flat file.
- **A picture is compared by its bytes.** In a flat file, by the decoded `office:binary-data` (an `xlink:href` beside the data is ignored). In a package, by the entry an `xlink:href` names: the reference wins over data beside it. A `draw:image` or `draw:fill-image` whose `xlink:href` names no entry (an address, or a missing file) keeps the reference as an attribute, so it differs from any flat file's picture.
- **The media type of a picture is content.** A `draw:image` that names an entry and states no `draw:mime-type` is given the media type the manifest states for that entry (else the type its bytes' signature says, else `application/octet-stream`), which is what the program that writes a package as a flat file writes. A `draw:fill-image` is given none. One that states its own type keeps it. A flat file that links a picture and states no type therefore differs from its package.
- **What a flat file has no place for is not compared:** the manifest, `mimetype`, thumbnails, configurations, signatures, macro projects, embedded objects, DiaCraft's own entries, and pictures that no part names.
- An attribute on a part's root, other than its version and its namespace declarations, has no place on the joined root. It is put there under a name no flat file has, so it is reported and not dropped.

### 4.7 Empty containers

**An empty container is the same as an absent one, for six elements, and for no others.** The six are `office:meta`, `office:scripts`, `office:font-face-decls`, `office:styles`, `office:automatic-styles` and `office:master-styles`. An element of the six is *empty* when it has no attribute, no child element, no processing instruction and no text but white space; one that holds anything is compared as every element is.

*Source and status. This is a rule of this format, resting on ODF 1.3.* Part 3 describes each of the six as a container that "contains" a collection (3.2, 3.12, 3.14, 3.15.2 to 3.15.4), gives it no attributes, and the schema makes each one optional with only optional content. So an empty one holds exactly what an absent one holds: no metadata, no scripts, no faces, no common styles, no automatic styles, no masters. ODF does not say in words that the two are equivalent. The rule is made so that a writer that writes an empty container, and one that leaves it out, are not told apart: an OpenDocument writer that starts a package from nothing writes some of them, and a file written by another program may lack them.

*Every other element is compared as it is.* In particular `office:settings` is not on the list, because ODF requires at least one settings set in it (it is none only as the whole content of a `settings.xml`, and under the root of a flat document, 4.5); `office:body`, a page `draw:page` and an empty `text:p`, `draw:g` or `draw:frame` are content, and a drawing with a page and one without differ. The VSDX elements treated this way are those 4.4 lists. *Three points of this rule, each a rule of this format chosen on the safe side (a difference reported rather than a sameness claimed):* a repeated container (two `office:styles`, say) is the same as an absent one only when every copy is empty; the rule does not apply inside a paragraph, at any depth; and emptiness is judged on the tree as written, so a listed element that holds an empty listed element is not empty. Between two packages, a `meta.xml` part that holds only an empty `office:meta`, or an `office:meta` that holds only a generator, is the same as a package with no `meta.xml` part, as an empty `settings.xml` is (4.2 and 4.5); against a flat file the same holds.

### 4.8 Limits of the check, and what it does not see

- **A checker's limits** are its own choice, not the format's. A checker may refuse a part that has more items or more bytes than it can compare, or two packages that together hold more than it can hold, and it MUST say so in words naming the part, and never answer "same" or "different" for what it did not read. DiaCraft's checker compares at most 2,000,000 XML items in one part on each side and 384 MiB of parts on both sides together, and reads each package within half of that.
- **Lists past the cap.** A checker sets the children of one element side by side by what they are only up to a number of them (DiaCraft's: 262,144 between the matched start and end, on either side, 4.3); a longer list is set side by side by place, and the checker counts those children; its count of differences is then not less than what the changes are, and may be more (a child put in shifts what follows it). It never calls a changed list the same.
- **Not seen.** A prefix named inside an attribute value that a format does not list as qualified is text. A reference to an automatic style that stands in a place 4.5 does not list is compared as written, so a style that is renamed and named there differs (a false difference and never a false same). `-0` is not `0`. A part re-cased in a VSDX package with its content-type row for it is reported as a difference in the row's `PartName`, which is a false difference and never a false same. Embedded objects held inline in a flat file and DiaCraft's own data inside a flat file are reported against a package until a rule says how the two forms spell them.
- **Proofs.** A checker is itself tested with planted differences, which must fail (a changed value, a missing attribute, an extra element, two ordered siblings swapped, a changed picture byte, a dropped part), planted reformatting, which must pass (spacing, attribute order, prefixes, number spelling, keyed siblings reordered, compression), and an echo control: a file against itself is the same, and a file against a copy with one salted value differs at the salt and nowhere else.

## 5. Versions

### 5.1 The numbers

The format version is two numbers, **major** and **minor**, written `major.minor` in the `format` attribute of the manifest. This document specifies **1.1**; its differences from 1.0 are 3.19, which a file of 1.0 does not use, and the limit of XML items in one part in 6.1, which is 12,000,000 here and was 6,000,000 in 1.0.

- A writer always writes its own version, whatever version it read.
- A reader that meets a **major** version it does not know MUST refuse the file in words that name the version: DiaCraft says "made by a newer DiaCraft (format 2.0)". Nothing else is guessed.
- A reader MUST open a file of a **higher minor** version of a major version it knows (it reads what it knows and keeps what it does not, 5.2 and 5.3), and a newer reader MUST read every older file of the same major version. **The one exception is a limit:** a minor version may raise a limit of 6.1 (5.5), and a reader of an earlier minor MAY refuse a file that passes the limit that reader has, in words that name that limit (6.1, 6.4), and MUST NOT call it damaged. It refuses nothing else of a higher minor.
- The `version` attribute of the document element is the version of the document model, not of the package, and is 1 in every 1.x file.

### 5.2 Unknown parts

A higher minor version may add entries. A part whose role the reader does not know is an **extra part**.

- If its manifest row has `required="1"`, the reader MUST refuse the file in words that name the role.
- Otherwise the reader keeps it: its path, its role word, its media type, the other attributes of its manifest row, and its bytes. It holds the part with the drawing as a package part of format `native` (3.14), so that the part survives every later save, including a save to another format. *A rule of this format.*
- A program that writes the drawing as a `.diacraft` writes each extra part again as an entry of its own, at the same path, with the same bytes and the same manifest row (its role word, media type and other attributes), after all the entries of this version, in the order it read them. The part is not stored again as a blob, and no `part` element of the document part names it. *A rule of this format.* A writer that cannot keep an extra part MUST NOT write the file as if it had no such part: it refuses, or tells the person in words.
- When a drawing that holds extra parts is written as a VSDX or OpenDocument file, the parts ride in that file as DiaCraft data (section 7), so that they come back.
- An entry of role `blob` that no record names is handled in the same way when the file's minor version is higher than the reader's (2.6). *A rule of this format.*
- The part is bytes: no program parses it.

### 5.3 Unknown content inside the model

A higher minor version may add attributes and child elements to the elements of this vocabulary, in this namespace or in another. A reader keeps what it does not know **on the object it stood in**, and a writer writes it back **where it was**:

- an unknown attribute stays on its element, after the attributes this version writes, in the order it had;
- an unknown child element or node stays a child of its element, placed after as many of that element's known children as stood before it;
- an unknown element is written with the namespace declarations its names use, so that the part is namespace-well-formed (an unknown element of this format's own namespace therefore carries a repeated declaration of it, which changes no meaning). A newer reader reads it as its own.

This holds for the document, a page, a master, a style, a shape, a sheet, a section, a row, a cell, a `foreign`, a `connect`, a `font`, and a text. It also holds for the **records** that have no slot of their own: a colour entry, a package part, a relationship, a content-type row, a user-defined property, a definition, a kept style, an OpenDocument root or manifest row, a held value, a paragraph, a run, a text style and a held style value. What a newer version wrote on a record stays on that record, and a program that rewrites a file attaches it to the same record by what the record **is**, never by where it stands:

| Record | Its key |
|---|---|
| Colour entry | its `ix` |
| Package part | its `name` |
| Relationship | its `source` and `id` |
| Content-type row | its `extension`, or its `part` |
| User-defined property | its `format` and `name` |
| Definition | its `family` and `name` |
| Kept style | its `family` and `name` |
| OpenDocument root, manifest row | its `name` |
| Held value | its `group` and its ordinal among the held values of that group that the same owner holds |
| Paragraph, run, text style, held style value | its ordinal in the owner's list (a run: the paragraph's ordinal and its own; a held style value: also its group and its ordinal within that group) |

The last two rows are the records that have no identity of their own, so their place in the list is their key: two held values of one group each keep what a newer version wrote on them. A record whose key is gone (it was deleted) loses what was kept for it. A program that deletes, inserts or reorders records of a list keyed by ordinal MAY misplace what a newer version wrote on the records after the change, and SHOULD then drop the kept content of that list whole.

A few elements have no place to keep what a newer version adds: the list elements `page-ref`, `master-ref` and `blob`, `same`, the elements that name a byte string (`theme`, `picture`), `props`, `extra`, `solution`, the text markers `t`, `cp`, `pp`, `tp` and `fld`, the wrappers of the slots (`ext`, `odf`, `vsdx`, `rest`) and of the held data of 3.14 and 3.15. A reader drops unknown content on them, counts what it dropped, and tells the person in words, never naming anything from the file. A later minor version SHOULD NOT put information on these elements that matters.

**The lists of words are closed in 1.x.** These attributes take only the words listed: `type` of a shape (`Shape`, `Group`, `Foreign`, `Guide`), `format` of a package part and of a user-defined property, `role` of a package part, `group` of a held value and of a held style value, and `family` of a definition. A word that is not on the list makes the part damaged. A later minor version that needs a new word adds a new element or attribute for it.

### 5.4 Stale unknowns (for writers of a later minor version)

Nothing in version 1.0 computes the stamp described here. It is stated so that later versions agree. **Version 1.1 computes one**, the fingerprint of a named unit (3.19): what a newer writer stores is the name that `same` elements take, the object is the unit, and what a reader drops when it does not match is the name.

A newer writer may store, on an object, data that an older reader will not know. An older program may then change the object while it keeps that data untouched, which would make the data lie. So a newer writer stamps the data it stores with a **fingerprint of the object**: the stamp (3.17) of the object's content that the older version of the format also holds, as it is when decoded, **never** of the unknown data itself, so that the stamp does not go round in a circle. An older program keeps the data and its stamp exactly as they are. When the newer program reads the file again it computes the fingerprint again, and **drops the data whose object no longer matches**, because an older program edited that object. So an older program never loses a newer program's data, and never makes it lie.

A 1.0 writer keeps what it does not understand exactly as it was read, stamps included, and does not compute or compare any such fingerprint.

### 5.5 Freeze and change

The element names, attribute names and stated defaults of 3.18 **do not change** in any 1.x version. A later minor version may add new elements and attributes that are optional, and may raise a limit of 6.1 (and never lowers one: a file that keeps to the limits of an earlier minor keeps to those of every later one). A change of any other kind (a name, a default, the meaning of a field, a field moved from one place to another) raises the **major** version if it makes an older reader read the drawing differently, or the minor version if it comes with a migration step in the newer readers, tested on a hand-written file of the old form.

1.1 did the second for the items of a part (6.1). A newer reader reads every 1.x file ever written. A newer writer may lay the same content out differently from an older one, which is why the promise of section 4 is about content and not bytes.

**A later change to the encoding keeps the content stamps of files already written.** The content stamp (3.17) is defined over content, not over the encoding: a later version that writes the parts differently still computes the stamp of the same content as 3.17 describes it, in the form 1.0 gives, so that the stamps already stored stay good. What a later minor version adds is not in the stamp of a drawing that holds none of it, and that version defines how its additions enter the stamp. A change that cannot keep the stored stamps (a change of what the stamp covers, say) needs a new major version. *A rule of this format.*

## 6. Limits

### 6.1 What a file must keep to

A conforming file keeps to these limits. A writer MUST NOT write a file that passes one, and a reader MAY refuse a file that does. **The table is the limits of the format. A minor version may raise a limit and never lowers one** (5.5), so a file of 1.0 that keeps to the limits of 1.0 is a conforming file of every 1.x. One limit differs: 1.0 allowed a part 6,000,000 XML items and 1.1 allows 12,000,000, so a part of more than 6,000,000 items is a part of 1.1, and a reader of 1.0 MAY refuse it, in words that name the limit it has (5.1). A reader may hold itself to lower limits of its own (6.4) and then MUST refuse a file that passes one of them in words that name the limit it is, and not as damaged: DiaCraft does (6.4).

| What | Limit |
|---|---|
| Entries in the package | 50,000 |
| Bytes of one entry, inflated | 256 MiB |
| Bytes of the package, inflated, all entries together | 1 GiB |
| Ratio of an entry's inflated size to its stored size | at most 1,000 to 1 |
| The name of an entry | 1,024 bytes |
| XML items in one part (each element, attribute, text, comment and instruction) | 12,000,000 (1.0: 6,000,000) |
| One attribute value, as written | 1 MiB |
| Depth of the XML of a part | 512 elements |
| Pages in a drawing | 1,000 |
| Masters, and styles | 65,536 each |
| Shapes in one part | 1,000,000 |
| Colour entries | 4,096 |
| Levels of nested shapes (3.5) | 200 |
| Levels of a kept element (3.12) | 200 |
| Cells in one place, sections in a sheet, rows in a section | 4,096; 8,192; 320,000 |
| The name of a cell, a section or a row | 1 to 1,024 bytes |
| Package parts held (3.14) | 50,000 |
| Definitions of one family (3.15) | 65,536 |

The depth of 512 covers the deepest path the vocabulary allows: the root, 200 levels of shapes, a few wrapping elements (`sheet`, `ext` and `kept`), and a kept element of 200 levels of its own, at most 407. A reader MUST parse the parts of a `.diacraft` at depth 512, and not at the lower depth that VSDX parts use.

### 6.2 What a reader does

- A reader treats every byte as hostile. It checks a limit **before** it allocates for the thing the limit bounds: the number of entries and the sizes the archive states before it inflates, the size of the file before it reads it, the depth of shapes before it builds one, the sizes the blob records state before it fetches a blob, and a count of the items of every part. It never trusts a count, a size or a stamp a file states.
- One budget covers the whole open: the model parts, the blobs and `state.xml` share it, and each element, attribute and byte of text is charged once.
- What it cannot afford it refuses **in words** that say what is large. It does not abort, and it does not open a part of a drawing in silence. The refusal is for the file, not for what it could not hold.
- It does not read a blob as a package, parse it, or follow what is inside it: bytes that hold a package or a bomb are copied, and nothing nested is opened.

### 6.3 What a writer does

A writer SHOULD check, before it writes anything, that its own reader would open the file it is about to write, and refuse in words that say what is large when it would not. A program that saves over a person's file replaces it only when the new file is whole.

### 6.4 A reader's own limits (informative)

The limits above are the format's. A reader also has limits of its own, that bound its memory, and these are its choice, not rules of the format. A file that one reader refuses for them another may read.

DiaCraft 0.2's reader holds an open to about 3 GB of address space. For that it reads one part at a time, and a part that holds XML is at most 128 MiB. **A page part or a master part is read as a stream of the children of its root** (its sheet, its shapes, its connects and what it keeps): the reader holds the root's start tag and one child at a time, so that a page has as many shapes as its bytes hold, and a child (a shape and everything under it: a group of tens of thousands of shapes, say) is at most 4,000,000 XML items, of which 2,000,000 are held in the pool of one tree and what it has beyond them is counted as model for as long as the child is held; the document part is read whole, at most 2,000,000 items, and what its tree has beyond 256,000 items is counted as model while the pages are read; **`state.xml` is read whole as well, at the same 2,000,000 items**, of which the root and its declaration are two and each ledger row is three (its element and its two attributes), so a ledger has 666,666 rows at the most. A row costs its bytes (one event of the parser and a few steps of the walk), and a mark is kept only for a page the drawing has (2.7), so a ledger of 200,001 rows (8.4 MB of text) is read in about a tenth of a second, a quarter of what plain pages of that size take, and the cost is linear in the size of the part. A reader that holds a part whole is held to 2,000,000 items for it. **A page or a master part that is read as a stream is still a part of 6.1: DiaCraft's writer does not write one of more than 12,000,000 items in all, and its reader refuses one** (in words that name the limit, "more than 12000000 XML items in one part"). At most 64,000,000 items are read over all the parts of a package (the items of the unit that a `same` stands for are counted again for each, so that a drawing is charged what it would be if each place said its unit in full); it holds the model and the blobs to 1,344 MiB between them, the blobs to at most 384 MiB of it, so that a drawing with no pictures may have a model of 1,344 MiB and one with 384 MiB of pictures a model of 960 MiB, the model counted as it is held (a sheet or a section that an earlier one equals, or that a `same` names, is one copy), and the package to 512 MiB inflated, and refuses a file larger than the package cap plus 64 MiB before it reads it. A drawing that is open may be edited to a model of 1.5 GiB, and the edit that would make it larger is refused in words; a file of more than 1,344 MiB of model is not one it saves as a `.diacraft`. These figures are the reader's choice. DiaCraft raises them as its reader improves, so nothing in a file may depend on them. **They are lower than the limits of 6.1** (the package is 512 MiB here against 1 GiB there, a part held whole 2,000,000 items and a child of a part 4,000,000 against 12,000,000 for the part), so a file that keeps to 6.1 may be refused: DiaCraft refuses it in words that name its own limit (for a child of more than 4,000,000 items, "the drawing holds more than 4000000 XML items in one element of a part"; the number is written without commas), and does not call it damaged.

## 7. DiaCraft's data in VSDX and OpenDocument files

This section is about the files DiaCraft writes in the two other formats, not about `.diacraft`. It says where DiaCraft puts its own data in them, so that a program that reads such a file knows what that data is and what it may do with it.

### 7.1 What rides, and what does not

A VSDX file or an OpenDocument drawing that DiaCraft writes **carries only what its format cannot say**, item by item. It never carries a second copy of the drawing.

- **Cells are written natively.** No cell is copied into DiaCraft's data. The file draws the same without that data, because DiaCraft draws held data (3.15) by its stand-in cells, which the file has.
- **Into a VSDX file** ride: the data of the model tagged OpenDocument (3.15), the held values, the unknown content of a newer minor version, the extra parts of 5.2, an OpenDocument macro project as bytes, and the files of OpenDocument objects that VSDX cannot hold as an embedding. An object goes natively wherever the target can hold it, and rides as data only where it cannot.
- **Into an OpenDocument drawing** ride: the VSDX data of the model that OpenDocument cannot say (kept attributes and nodes, `Data1` to `Data3`, deleted flags, foreign data beyond a picture, held VSDX data and package parts), the macro project of a VSDX file with its signature and data parts as bytes, the VSDX embeddings an OpenDocument drawing cannot hold as an object, the unknown content of a newer minor version, and the extra parts of 5.2.
- **A macro project is bytes and nothing else.** It is never parsed past its container, never run, by any version of DiaCraft. **A VSDX file of a type that promises no macros takes no macro project, of any language, even as DiaCraft data**: a program that writes one MUST NOT put a macro project in it (DiaCraft refuses the save in words, and offers the person a way to leave the macro project out), because the extension of such a file tells people and scanners that it has none.
- A file that carries macro projects or objects may be flagged by a file scanner, though the program that reads it never shows or runs them.
- **Every item is keyed to its object**, by a page `id`, a shape `id`, a master `id` or a style name, and is **stamped** (7.5). An item whose object is gone is dropped without a word.

### 7.2 In a VSDX file

- **The solution part.** DiaCraft's data is in one application-data part of the VSDX package, found through the package's own solutions index, the way the VSDX format finds any solution part. The index lists a row named `DiaCraft`. The part's root is `SolutionXML` with `Name="DiaCraft"`, in no namespace, and its one child is `Data` in DiaCraft's namespace (3.1), with the attribute `version` in that namespace, today `1`. The name `DiaCraft` and the namespace never change. **The part has no XML declaration**: it starts at the root, as the solution parts of the files the VSDX application writes do. A build before 0.2.4 wrote one, and a reader MUST read the part with or without it. DiaCraft cuts the declaration from the part of such a file on every save that writes the file again, an edit of anything or a Save As, though its data did not change, and leaves the part as it is on a save with no edit.
  - **Written today:** `Data` holds DiaCraft's own application data, the same element as `solution` in 3.10.
  - **Items.** In a file that carries data of the other format, `Data` also holds **items**: elements named `item` in DiaCraft's namespace, children of `Data`, written after DiaCraft's own application data. An item has `kind`, the key of its kind, and (but for the kind `parts`) `fp`, its stamp (7.5); its one fragment is an element of the vocabulary of section 3 that declares the namespace itself. The fragment holds **only what VSDX cannot say of its object**: the `odf` and `native` sides of the slots (3.13), the parts and the user-defined properties that are tagged for the other format, and the places in the sheets that hold such data. It holds no cell with a value or a formula. A reader that does not know a `kind` keeps the item with the rest of the `Data`.

    | `kind` | Key | Fragment |
    |---|---|---|
    | `document` | none | a `document` (3.3) with the slot of the document, the `font`s whose slot holds data, and the places of the document's sheet that hold data |
    | `parts` | none (no `fp`) | a `document` with the `part`s (3.14) and the user-defined properties of the other formats, and the `blob` list that names the bytes (3.16) |
    | `page`, `master` | `id` | the page's or master's part (3.4) with its slot, its sheet's places that hold data and its `connect`s whose slot holds data |
    | `style` | `name`, written as `name-b64` when XML cannot hold it; `id` for a style that has no name, whose name another style of the drawing has too, or whose name is longer than 4,096 bytes; a reader finds an `id` among all the styles, whatever their names | a `style` with its slot and its sheet's places that hold data |
    | `comments` | none | a `comments` element holding a `link` with `id` and `parent` for each reply: the number of the comment that is the reply and the number of the comment it answers (one thread level); stamped (3.17) with what the comments part says of each comment, in order (number, page, shape, author, dates, Done mark and text, and not the thread, which the part cannot say); a reader puts a link on its comments only when the stamp matches, both comments are there on one page and shape and neither is a reply already |
    | `shape`, `master-shape` | `of`, the `id` of the page or master, and `id` | a `shape` without its children, each of which has an item of its own; and an `at` child of the item, with `n` and `k`, for each part of the shape's text that rides: `n` is the number of markers (`cp`, `pp`, `tp`, `fld` and kept nodes) before it and `k` the bytes of characters since the last of them |

    A section, a row or a `connect` of a fragment that holds data is named by its position among its siblings: an `attr` of its own, named `at`. A `font` is named by its `name-u`. An item whose object has a key that another object of the file has too is not written (a style whose name and `ID` both another style has), and a writer says so only for an object that holds data. Two items that reach one style, one by its name and one by its `id`, are dropped with a word, as two items of one key are.
  - **Stamps.** The `fp` of an item is the stamp (3.17) of **what the file says of the object as VSDX says it** when the item was written: the object's own fields, its sheet (every cell with its formula, value, unit and error, every section and row), its text (the characters between two markers are one run), the unknown content of its slots, and a picture's length and first eight bytes (the name of a picture's part and the type it is given are the writer's choice, and a shape that shows a picture is written with the attributes its picture calls for, `ForeignType`, `CompressionType`, `ObjectType`, `ShowAsIcon`, `ObjectWidth` and `ObjectHeight`, so those are no part of the stamp). It is made of nothing the item carries, so that the program that reads the file can make it from what it reads, before the item is applied. It leaves out the view of a page (`ViewScale`, `ViewCenterX` and `ViewCenterY`), the names the object had in the package it came from, and a shape's children. A document's stamp is of its title and the descriptive fields of its properties (subject, creator, keywords, description, category, company, manager, hyperlink base and language) and no more.
- **DiaCraft parts.** Bytes do not go in the solution part. They go in package parts of DiaCraft's own, named `/diacraft/blob<n>.<ext>`, because an OpenDocument entry name may hold a space, which a part name of this package cannot. **Each distinct byte string is one part**, however many parts of the other format hold it. `<n>` is the least number, counted up from 1 after the last one tried, for which no part of the package has that name (compared ASCII case-insensitively as the Open Packaging Conventions compare names), and `<ext>` is `png`, `jpg`, `gif`, `bmp`, `tif`, `webp`, `svg`, `emf`, `wmf`, `pdf` or `zip` by the media type, `xml` for `application/xml`, `text/xml` and any type that ends in `+xml`, and `bin` for any other. Each takes the media type of its bytes as its content type (`application/octet-stream` when that is not a `type/subtype` of token characters). **Each is linked by a relationship from the solution part, of the type `urn:app.diacraft:extension:blob`** (a reader reads `urn:io.github.one_bit.diacraft:extension:blob`, which builds made before the id moved wrote, as the same type), and the `blob` records of the `document` and `parts` items (3.16) name it by the `Id` of that relationship, in `path`. A program finds DiaCraft's parts through the solution part's relationships, never by the shape of their names, and no DiaCraft part has a name that another part of the package has, compared ASCII case-insensitively, and no relationship points at another part than the one it names. **A relationship of that type is DiaCraft's only when it leads to a part under `/diacraft/` (not a folder, not a relationships part) that no other relationship of the package names.** The solution part is DiaCraft's own, and carries items, only when it holds nothing but the `Data` and relationships of that type, all of them DiaCraft's; any other solution part is patched as before and gets no items. A program that writes such a part MUST NOT take a part out of the package because a relationship of that type names it, unless the relationship is DiaCraft's by that rule.
- **Objects.** An object frame of an OpenDocument drawing (a `draw:frame` that holds `draw:object-ole`) goes natively, as an embedding, when its object is an entry of the package or inline base64 whose bytes are a compound file and the first `draw:image` of the frame shows a picture in BMP, EMF, GIF, JPEG, PNG or TIFF by its first bytes. **The object is a compound file** when it has at least 512 bytes, begins with `D0 CF 11 E0 A1 B1 1A E1`, is of version 3 with sector shift 9 or of version 4 with sector shift 12 (and has the byte order mark `FFFE`), and the entry that its first directory sector starts, at the sector number plus one times the sector size, lies inside the bytes and has the type 5; **nothing else of the object is read, and its class is not**. A writer then writes: the object's bytes, unchanged, as `embeddings/oleObject<n>.bin` under the folder of the document part (`<n>` above every number in use, compared ASCII case-insensitively), typed by a `Default` for `bin` of `application/vnd.openxmlformats-officedocument.oleObject` or, when the package's `Default` for `bin` says another type, by an `Override`; a relationship of the type `oleObject` from the page part to it; the picture as a picture part, named and typed as any picture, with the one `image` relationship that the object part's own relationships part holds (**the page has no relationship to the picture**); and a `Foreign` shape whose `ForeignData` has `ForeignType` `Object`, then the attributes that rode with the frame, or `ObjectType` 33280, `ShowAsIcon` 0, `ObjectWidth` and `ObjectHeight` as the shape's `Width` and `Height` are written. **The object rides nowhere else**: no part of DiaCraft's own holds its bytes, and the item of the frame's shape holds no `draw:object-ole` element. Two frames of one object that show one picture share one embedding; a frame that shows another picture of the object has an embedding of its own. An object past 256 MiB, and every other frame (a `draw:object`, an object or a picture the package does not hold, bytes that are no compound file, a replacement in another format or none, a frame on a master, an entry that is a macro project) ride as this section says above, and its shape shows its replacement as a picture.
- **Caps.** The solution part is read up to 64 MiB. A file carries at most 30,000 parts of DiaCraft's (a relationships part is at most 4 MiB, and a relationship to one of these parts takes about 130 bytes), each at most 256 MiB, and all of them keep to the limits of the package (50,000 parts, 1 GiB inflated). The items of a file are at most 64 MiB less a sixteenth for the settings, and at most 5,900,000 XML items, which is what a part of a VSDX package may hold. A macro project or object that is too large for them, or items that pass them, are refused at the save, in words that name the part where it is one, and the person is offered a way to leave the project or the objects out, or to save a `.diacraft` file instead. Nothing is cut.
- **Macro projects.** A VSDX type that promises no macros takes no macro project of the other format (7.1). **A reader reads a macro project of the other format, a part whose entry name is under `Basic/` or `Scripts/` or is `META-INF/macrosignatures.xml` (compared ASCII case-insensitively), or whose item says it is a macro project, only when the type of the package allows macros**: otherwise the part is left out before its bytes are copied or charged, with a note. A writer does the same by the entry name and by the role, whichever says it. **The role of a part of an OpenDocument package is the role its entry name says** (a macro project under `Basic/` and `Scripts/`, the macro signatures, the document signatures, the thumbnail under `Thumbnails/`, the replacement pictures under `ObjectReplacements/`, the objects under `Object <n>/`, the fonts under `Fonts/`, any other part none), whatever the item states, and a reader that finds another role in the item takes the one the name says.
- **Reading.** A reader reads a DiaCraft part only when its item is fresh (7.5), charges its bytes to its budget before it copies them, and never parses, runs or activates it. **A reader judges each item against the file it now reads**: an item whose object is not in the file, or whose key two objects of the file have, is dropped without a word; an item whose stamp does not match is dropped; **two items for one object name nothing, and all of them are dropped, with a note**, so that an object is judged once however many items name it; an item that names a part the package does not have, a part whose size or stamp is not the one the `blob` record states, and two records of one item that name one relationship, are dropped with a note. An entry name that an item gives is checked before it is put in the model and before it is written into an OpenDocument drawing, so that a crafted name cannot reach outside the package: **a name is refused when it is empty, longer than 1,024 bytes, begins with `/`, holds `\`, begins with a drive letter, holds a control character or an empty, `.` or `..` segment, is the name of an entry an OpenDocument package makes itself (`mimetype`, `content.xml`, `styles.xml`, `META-INF/manifest.xml`, and `meta.xml` and `settings.xml` spelt any other way than so) or is under `DiaCraft/`** (compared ASCII case-insensitively; `meta.xml` and `settings.xml` spelt so ride, as the `.odg` reader holds them as parts when no field can and the `.odg` model writer writes such a part in the place of its own), and a name that differs from an earlier one only in case is left out. A reader spends a bounded amount of work and memory on items, and leaves out, with a note, the items it has not judged when that is spent: reading never fails the drawing for what the solution part carries.
- Whether another program keeps this part and the DiaCraft parts when it saves a VSDX file is **not known**, and DiaCraft tells the person that a program that saves the file may leave them out.

### 7.3 In an OpenDocument drawing

- **Per shape: attributes of DiaCraft's namespace on the shape's own automatic style** (its `style:graphic-properties`). OpenDocument 1.3 lets an extended document carry elements and attributes of a foreign namespace, one that is not among its own (Part 3, 3.17; for a foreign element outside paragraph text it says a consumer should not interpret the content and may preserve it, and for an attribute it says nothing more). Its package rules forbid only other files under `META-INF/` (Part 2, 2.2.1, item E; an extended package, 2.2.2, is exempt from that item), so an entry outside that folder is allowed. DiaCraft's data is of that kind. Written today:

  | Local name | Value |
  |---|---|
  | `version` | `1`, written with any other attribute of the namespace |
  | `id` | the shape's `id` in decimal |
  | `name`, `name-u` | the shape's name and universal name, when they are not what `draw:name` gives |
  | `master`, `master-shape` | the shape's links to a master |
  | `type` | `Guide` for a guide |
  | `cells`, `s-<name>` or `s-<name>-<ix>`, `text` | a fragment of XML (the cells of the shape's own sheet; one whole section; the shape's whole text), written only where the reader of the drawing would not rebuild the cell, section or text from the shape's markup |
  | `connects` | the glue of the ends of a segment the file holds as a polyline or a path, which that element cannot say: at most one space-separated token for each end, `b` or `e`, the `id` of the glued shape, the part of the glued shape's cell and of the end's own cell, and the glued cell's name escaped as the names of `s-<name>` are, as `<end><id>:<part>:<part>:<cell>` (`_` for a part that is none). A reader makes of each a `Connect` row of the page when the shape it names is on the page, and leaves it out, with a note, when it is not. **No build writes it any more** (a segment glued at an end is a `draw:connector`, whose glue is the element's own); the reader still takes it from a file an earlier build wrote |
  | `gone` | tokens that name what the reader would rebuild and the model lacks |
  | `vsdx` | a `shape` element of the vocabulary of section 3 that declares the namespace itself, written for a shape that holds data of a VSDX file: **only what the markup cannot say**, the kept attributes and nodes, `Data1` to `Data3`, the deleted flag, the shape's type and the `vsdx` and `native` sides of its slots (3.13), and the places of its sheet that hold such data. It holds no cell with a value or a formula that the markup says |
  | `foreign` | a `shape` element holding the shape's foreign data (3.7) beside its picture: `ForeignType` and then every other attribute **in the order the file had them**, the relationship id, the text, the kept nodes and the slots. The picture is the markup's, and a reader keeps the type its bytes say |
  | `fp-text`, `fp-name`, `fp-master`, `fp-s-<name>[-<ix>]`, `fp-c-<name>` | the stamp (7.5) of the outputs the item of that key determined |
  | `fp-format` | the stamp of the **formatting** of the shape's text: its characters in stretches that have one formatting each (a run's row and a paragraph's, by what their cells say and not by their `IX`), the stretches that follow one another with one formatting made one, a marker whose row says nothing the same as none, so that a text with no formatting has the stamp of its characters alone |
  | `fp-vsdx` | the stamp of the shape's `draw:name` and of its outline (none for an element that has no outline: a line, a connector, a picture frame) |
  | `fp-foreign` | the stamp of the picture the item sits beside: its length and its first eight bytes, or none |

  **A carried text's stamp covers its formatting.** A text a writer writes from the model is plain characters in the markup, and its formatting is DiaCraft's data. The item `text` is stamped with `fp-text` (the characters) and with `fp-format`, so that **a change of the formatting alone made in another program, a bold turned on in the markup, drops the item and the file wins**, as a change of the characters does. A file with `fp-text` and no `fp-format` (one written before this rule) is judged by its characters alone.

  A shape with nothing to carry has no attribute of the namespace. A name of the namespace that a reader does not know is kept as written and ignored. The fragments are today of the sheet-fragment dialect, with no namespace declaration and no prefix on their elements; the fragments of the VSDX data are of the vocabulary of section 3 (the whole-model codec, as 7.2 uses it), and each declares the namespace itself.
- **Groups, object frames and copies of shapes cannot hold these attributes**, because OpenDocument gives a group no automatic style and some programs drop the attributes on object frames. What would ride on them goes in the document file, keyed by the group's name or by `id`; the VSDX data of a group rides there as a `vsdx` item of the kind `group` (below).
- **Per document: the document file.** DiaCraft's document-level data is the entry `DiaCraft/document.xml`, listed in the manifest with the media type `application/vnd.diacraft.document+xml`. It is an XML 1.0 document. Its root is `document` in DiaCraft's namespace with an unprefixed `version="2"`. Its children, each written only when it has something to hold, come in this order: `data` (DiaCraft's application data, kept whole), `words` (the snap and glue words as attributes), `docsheet` (what the document's sheet holds that the markup cannot say), `page` (one for each page that has something to say, found by its name), `group` (one for each group that has data of its own: a group that holds no connection points is one, its `gone` saying that it has none, since a reader gives every group the four standard ones; **the rows of a group's own `draw:glue-point` elements are no data of it** when the model writer writes the drawing, the four standard rows and then the rows whose element is held with the row's stamp, because the writer writes the elements with the group and a reader reads the same rows again from them), `style` (one for each style sheet that differs from its markup), `blob` (one for each distinct picture that a shape of a master shows: its number as `id`, and its bytes as base64 text), `master` (one for each master: the `master` element of section 3, which declares the namespace itself, with its shapes as children), `theme` (the theme's part name and its bytes as base64 text) and `colors`, and then the `vsdx` items (below). A child that a reader does not know, and text outside the children, is kept as it is.
  - **Reading** never fails the drawing: a document file that is not XML, or whose root is not `document`, is left unread with a warning, and the drawing is read from its markup, as it must be after another program saved it without the file. A `page` entry for a page the drawing does not have is a warning and nothing more, and so is a master that section 3 finds damaged, or that is past a cap of the model (shapes nested more than 200 deep, a kept element nested deeper than section 3 keeps one), which is left out while the rest of the file is read.
  - **Saving** rewrites the file when the model differs from what was read. A save with nothing to write and nothing kept takes the file and its manifest row out.
  - The file is at most 64 MiB. **Versions.** The file's own `version` is 2: the masters and the `sheet` elements inside `docsheet`, `page`, `group` and `style` are written in the vocabulary of section 3 (version 1, which no release wrote, had a vocabulary of its own). A reader keeps a file of a lower version as it is and does not read it, with one note, and a save that has something to write makes a new file of version 2 that holds what the model holds, with a note that it replaced a file of a lower version; a reader that meets a higher version reads the names it knows, with one note, and keeps the rest, and a save over such a file writes the root with the version it wrote, 2.
- **The VSDX data in the document file: items.** A drawing that holds the data of a VSDX file (7.1) carries what an OpenDocument drawing cannot say of it as `vsdx` elements, children of the root written after every other child. An item's `of` names its kind, and its one child is the fragment, an element of the vocabulary of section 3 that declares the namespace itself and holds **only** the VSDX side of its object (the kept attributes and nodes, `Data1` to `Data3`, the deleted flag, the `vsdx` and `native` sides of the slots, the places of the sheets that hold such data); no cell that the markup says is copied. A reader that does not know an `of` keeps the item as it is.

  | `of` | Key | Stamp `fp` | Fragment |
  |---|---|---|---|
  | `document` | none | the title and the descriptive fields of the properties (as 7.2; one that holds an empty text is the stamp of none, as the file says it) | a `document` (3.3) with the slot of the document, the `font`s whose slot holds data, the descriptive fields of the properties that hold an empty text (`meta.xml` has no way to say one; a reader puts it back only where the file has none), the places of the document's sheet that hold data and the relationships and content types of the VSDX package and the rest of its property parts (3.14) |
  | `parts` | none (no `fp`) | none: it sits beside nothing the file says | a `document` with the `part`s (3.14) and the user-defined properties of the VSDX file, and the `blob` records that name the entries (3.16) |
  | `page` | `name`, and `nth` (the number among the pages of that name, from 1) | none | the page's part (3.4) with its slot and the places of its sheet that hold data |
  | `style` | `name`, else `id` | none | a `style` with its slot and the places of its sheet that hold data |
  | `group` | `page` and `page-nth`, `name` and `nth` | the names of the group's members, in order | a `shape` with the VSDX side of the group, without its children |

  **A reader judges each item against the file it now reads** (7.5): an item whose object is not in the file is dropped without a word; an item whose stamp does not match is dropped; two items for one object name nothing and are dropped with a note; an item that names an entry the package does not have, a record whose entry has another size or stamp than the record states, two records that name one entry, and a part whose name no package of its format could hold are dropped with a note. **A `page` or a `style` item has no stamp**, and is judged by its key alone. **A `group` item of a group with no name whose stamp is the stamp of more than one group with no name of the page read names none of them, and is dropped with a note**: another program may have put one such group in the other's place. The entry of a record is **read only when its item is good**, after its bytes are charged to the reader's budget, and is never parsed, run or opened. A part of the VSDX format is named as that package names a part: **a slash, then segments joined by slashes**, none empty, none that ends in a dot, none that is `.` or `..`, no control character and no backslash, not longer than 1,024 bytes and not the content types stream, compared ASCII case-insensitively; a part of a `.diacraft` package is named as an entry of its ZIP (7.2 gives the rules). A name that differs from an earlier one only in case is left out. The work a reader spends on items is bounded, and it leaves out, with a note, the items it has not judged when that is spent.

- **Comments: a page's `office:annotation` elements**. A drawing's comments are written at the end of each page's `draw:page` (the ODF 1.4 schema's `draw-page`), one `office:annotation` for each, with `dc:creator`, `dc:date` and `meta:creator-initials` in the schema's order and a `text:p` for each line of the text (runs of spaces are `text:s`, a tab is `text:tab`), `svg:x` and `svg:y`, and these attributes of DiaCraft's namespace: `version` (`1`, written with the others and required to read them), `id` (the comment's number), `shape` (the number the shape it is on is read with), `parent` (the number of the comment it answers), `done` (`1`) and `edited` (the date of its last edit). LibreOffice writes and reads the element too, as `officeooo:annotation` of `http://openoffice.org/2009/office` when it saves, with the place, the author, the initials, the date and the lines and none of DiaCraft's attributes; a reader takes both elements. **A reader takes an annotation only when it holds what the model does** (those attributes and children, plain text, within the caps of a drawing: 100,000 comments, 10,000 authors, 64 KiB of text in a comment and 32 MiB between all of them); any other stays in the page's markup. A writer that changes a page's comments takes out the annotations it took and leaves the rest.
- **More than one file: the `DiaCraft/` folder.** Bytes and the extra parts ride as entries under the folder `DiaCraft/`. The manifest lists the folder as a row of its own, whose full path is `DiaCraft/` and whose media type is **not empty**, beside one row for each file under it. No ZIP entry is written for the folder. This is required (*a rule of this format*): some programs that edit OpenDocument drawings keep every listed file of `DiaCraft/` when they save only when that row exists with a media type, and drop every one of them when it is missing or empty, listed or not; a file that is not listed makes some programs offer to repair the package. A reader accepts a folder row whose path ends in `/`, needs no ZIP entry for the folder, and needs no order of the files and no compressed size, since a program may deflate again what it keeps. A file under `DiaCraft/` that has no row is never DiaCraft's data. **The entries of the VSDX data** are named `DiaCraft/blob<n>.<ext>`: each distinct byte string of the parts is one entry however many parts hold it, `<n>` is the number of its `blob` record counting from 0, `<ext>` is the extension 7.2 gives by the media type, the row of the manifest has the media type of its bytes (`application/octet-stream` when that is not a `type/subtype` of token characters), and an entry of 64 KiB or more is stored and a smaller one deflated. A `blob` record of the `parts` item names its entry in `path` and states its `size` and its `stamp` (3.16, 3.17). **A save into the file writes the entries its document file names** (informative): `<n>` counts the byte strings of the parts that ride now, and they change between saves (the part of an object whose frame was deleted rides from then on), so a save that writes the document file again writes each entry it names with the bytes its record states, writing anew an entry of that name that holds others, and takes out the entries `DiaCraft/blob<n>.<ext>` it no longer names; a save that takes the document file out takes them out too. Other files under `DiaCraft/` are kept as they are.
- **Caps.** All DiaCraft data under `DiaCraft/` is at most 512 MiB inflated in a drawing, one file at most 256 MiB, **at most 30,000 entries** (the manifest is read up to 4 MiB, and a row takes about 120 bytes), and the limits of the package of 6.1 hold besides. A save that would write more is refused in words that name the part where it is one, and writes nothing: nothing is cut. A reader that finds more leaves the rest out, with a note.
- **Macro projects and objects are bytes.** An OpenDocument drawing has no type that promises no macros, so a macro project of a VSDX file, with its signature and data parts, rides in its entry whatever its language, and **it is never parsed, run or opened**: a read counts none of its bytes as XML. A person may leave it out at the save, and the save says that it did. **What a Save As is asked to leave out stays out of every save into the file it wrote** (informative): the file lacks the macro project or the objects, the drawing the program holds keeps them (a `.diacraft` saved from it holds them), and each later save into that file leaves them out again, so a save with no edit is no edit. The embedded objects of a VSDX file that an OpenDocument drawing cannot hold as an object ride the same way. **A save into a file the program wrote is given the drawing as that file holds it** (informative): an object of a VSDX file that an `.odg` holds as an object frame is held once, as the frame's own entry, and the embedding does not ride again, and the sections of a sheet are compared by their names and never by their place, so a save with no edit is no edit. **The same holds for a VSDX file** (informative): an object that went into it as an embedding is held once by every save into the file, an edit too, because the object's file that the program still holds is not carried a second time in the program's own part, and a drawing made from a template that holds frames of an OpenDocument drawing is saved again as it was.
- **What a save by LibreOffice keeps** (measured with LibreOffice 26.8.2.2, on copies, headless): the attributes of DiaCraft's namespace on the shapes' styles, `DiaCraft/document.xml` and every entry of `DiaCraft/` come through a save of such a file, **when the manifest lists the folder with a media type**; with the folder's row taken out, every entry of the folder is dropped and the attributes stay; an entry that no row lists makes it refuse the file. A writer therefore lists every entry and the folder.
- **A program that saves the file may keep the document file or leave it out.** LibreOffice 26.8.2.2 kept DiaCraft's own entries byte for byte in 129 of 145 saves of such files with no edit, because the writer lists the folder in the manifest; a program that leaves them out loses the document-level data while the data on shapes stays. DiaCraft tells the person, in the note of the save, what rides, that another program may keep it or leave it out, and that what that program changes in the drawing wins where DiaCraft can tell. Programs may also copy `DiaCraft/` unchanged after they edit the drawing, so a reader never trusts an item because its file survived: it judges each item, document-level ones included, against the drawing it now sits in (7.5).

### 7.4 A flat OpenDocument file

A flat `.fodg` is read by DiaCraft, and a drawing that came from one is saved in place, by Save, as a flat file: an unedited save writes the file's own bytes, and an edited save splices into the one document and carries only DiaCraft's own element of the root. **DiaCraft's data and the carrier of this section write packages only**: the document file and the folder `DiaCraft/` have no place in a flat file, so no save writes a `.fodg` from the model, and a `.fodg` is never the target of a Save As. A drawing that came from a flat file is saved by Save As as a `.diacraft` file, as an `.odg` package written from the model, or as a VSDX type, and the flat file is left as it was. What the flat file holds in its own form comes with the drawing: its `office:meta` is the `meta.xml` of a package and its `office:settings` is the `settings.xml` of one, so the title, the properties, the generator and the settings are held as a package's are, and a picture it holds inline (`office:binary-data`) is a picture of the drawing that a package writes as an entry of `Pictures/` under a name of the writer's own, and a `.diacraft` file holds as a blob. The one root of a flat file is the root of both parts: its attributes but `office:mimetype` (the media type is the `mimetype` entry's), the namespace declarations among them, are held as the `root` entries `content.xml` and `styles.xml`, so that the package written from the file or from the model declares the prefixes the markup the model keeps uses (a hyperlink's `xlink:href`, an attribute of another vocabulary on a geometry element) and writes that markup again. **A declaration made below the roots is kept with the markup that uses it**: the roots the model holds are the declarations of the root and of the containers down to the one that holds the pages (a flat file: its root alone), so an element kept whole, a text element (a link, a field, a span or paragraph with an attribute of its own, a list, an item, a mark) and a properties element of an automatic style, whose names use a prefix that a page, a shape, a paragraph, a span or the properties element itself declared on its own start tag, carry that declaration as the first of their attributes (one that binds what the roots bind is not added), and a shape's kept attributes carry `xmlns:p` beside them; the writer writes what it kept where the declaration is in force, and says what it cannot (a prefix nothing binds). **An attribute of a namespace of the standard's own other than `fo` and `style`, on the properties of an automatic text or paragraph style, is written in a part of ODF 1.2 or later only where the schemas of 1.2 and 1.4 give it there** (`text:number-lines`, `text:line-number` and `text:relative-tab-stop-position` on a paragraph's properties, `text:display` and `text:condition` on a text's; GATE.b94e.F1): `text:enable-numbering` of ODF 1.0, which no later version has, rides in the attribute `diacraft:older-enable-numbering` of the same element, which a read gives back as the attribute it stood for, any other is left out and said (the model writer labels every part 1.4). **A flat file whose root binds the prefix of its extension to the earlier namespace** is read with the extension holding its own declaration of the current one. A flat file has no manifest, so a package written from it has the manifest of the model writer. The package is the same content as the flat file by the check of section 4.6 for a drawing whose parts the model writer writes as they were read. What the writer does not yet write (the named definitions and the style families of other kinds, and the names it gives the automatic styles of a page) is said in the note of the save and is a difference of that check.

### 7.5 Stamps and judging

An item carried in a file is written with a **stamp**: a fingerprint of the outputs that the item sits beside in that file, made by the stamp function of 3.17. When DiaCraft reads the file again it computes the stamp again from the cells and markup it now finds. **An item whose stamp no longer matches is dropped, so another program's edit wins.** An item whose object is gone is dropped silently. An item with no stamp is not judged. With no other program in between, the drawing that comes back equals the one that went out, except for the held data of the format the trip went through, of which new items may appear.

**A segment written as the outline it draws is judged by what it draws.** A one-dimensional shape of a geometry of its own is a `draw:polyline` or a `draw:path` in an OpenDocument drawing, which a reader makes a 2-D shape of its box; the shape's frame (every frame cell, its ends) and its geometry in its own axes ride in the cells and sections of its style (and, in a file an earlier build wrote, its glue in `connects`). A segment glued at an end is no polyline: it is a `draw:connector` of the type `line` whose geometry rides in the sections of its style. Their stamp is no fingerprint of the points' text, which another program that saves the file writes in its own form (a view box one unit wider, each point a unit of 0.01 mm off: measured on LibreOffice 26.8.2.2). The box and the end of each row of the outline the carried cells draw are worked out again, and compared with the polyline's to the grid of the file (0.0008 inch); **a polyline that is not what the cells draw (moved, resized, turned, flipped, a point moved or added) drops the frame, the geometry, the link to the master and the glue together**, and the polyline the markup says stands.

A re-imported file is a new drawing. DiaCraft never merges a `.diacraft` back into a drawing it just read from a file, because merging would undo another program's edit.

### 7.6 What another program should do with this data

- A program that writes a VSDX or OpenDocument file it did not create, **and does not change the drawing**, SHOULD keep DiaCraft's data in it exactly as it is.
- A program that changes the drawing MAY keep the data (DiaCraft drops what no longer matches) or MAY drop it **whole**: the solution part and the DiaCraft parts of a VSDX file, or the attributes of the namespace and the `DiaCraft/` folder with its manifest row together in an OpenDocument drawing. It MUST NOT edit part of it, copy items to other objects, or move a stamp to another item.
- No program MAY run, open as a package, or parse any macro project or object that the data carries. It is bytes.

### 7.7 The save note

A program that writes one of these files from a drawing that came from the other format tells the person what the file carries. **The note is a structured value** and never a warning: `{classes, document_level, macros, objects, signatures_stale}`, where `classes` are the kinds of data that ride, in plain words, `document_level` says that some of it is in DiaCraft's document data and not on shapes, `macros` and `objects` say that a macro project or embedded objects ride as bytes, and `signatures_stale` is how many digital signatures in the file no longer match it. With nothing to say there is no note. Each part is shown only when it applies, in this order, in these words:

- **Carried data, in a VSDX file:** "This drawing has data a VSDX file has no place for ({classes}). DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."
- **Carried data, in an OpenDocument drawing:** "This drawing has data an OpenDocument drawing has no place for ({classes}). DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell." The second sentence says what was measured: a LibreOffice save kept DiaCraft's own entries byte for byte in 129 of 145 saves with no edit, so it is the same whatever the classes are (it said that LibreOffice leaves out the document data until a later change).
- **A macro project or objects:** "This file carries {a macro project | embedded objects | a macro project and embedded objects} that {VSDX | OpenDocument} programs may not show. DiaCraft keeps them so that they come back when the file is opened in DiaCraft. DiaCraft never runs macros."
- **Signatures:** "{n} digital signature(s) in this file no longer match it."

The classes are "line ends", "gradients", "hatches", "glue directions", "layer settings", "style names", "document properties", "settings", "macro project", "embedded objects", "window settings" and "unknown parts", and, for what those do not name, "dash patterns", "fill images", "page layouts", "package details", "digital signatures", "shape data", "page data", "style data" and "document data". Only "shape data" rides on shapes; every other class rides in the document data. A save never carries anything without naming a class.

**The check after the save** (informative). A program that writes one of these files from the model, and not by splicing into the file the drawing came from, can read the file back and compare it with the drawing it was written from, class by class, and say what differs. DiaCraft does after its three model writers (the VSDX writer, the `.odg` writer and the `.diacraft` writer for a drawing that came from a file of another format, or from a template file of one), and gives a second structured value in the save result, **`checked`**: `{done, classes, as_values, reason}`. `classes` is a list of `{class, words, count}` for each class that differs, in the order `pages`, `shapes`, `shape-numbers`, `geometry`, `text`, `text-formatting`, `pictures`, `objects`, `macro-project`, `fonts`, `line-ends`, `gradients`, `style-content`, `style-sheets`, `masters`, `layers`, `connections`, `formulas`, `hyperlinks`, `fields`, `user-cells`, `shape-attributes`, `properties`, `comments` and `kept-data`, and `count` is the number of the items that differ in the unit the class counts (a shape, a paragraph, a picture, a formula, a row). `as_values` says that some content is written in the forms the file format holds, and `done` is false, with `reason` in words, for a file that was written and could not be checked (the reader refused it, or the drawing is past the steps the check may take). **The same content is no difference:** a length in another unit, a colour in another spelling, `Width*1` for `Width`, a value a style sheet gives where the drawing held it on the shape, a link to a style sheet written as the values it gives, and a form of an earlier version of the file format written in the form its current version takes are one flag, said once. **A formula that does not come back as a formula is a loss**, even when its value comes back, and is counted in `formulas`. What a file holds beyond the drawing is no loss where it changes nothing the drawing shows (see below). DiaCraft's own carrier (the `DiaCraft/` entries of an `.odg`, the solutions parts of a VSDX file) is not compared as content; what it carries is compared where it comes back, in the model. A save into the file the drawing was opened from is not checked in 0.2 (`checked` is `null`), and the Compatibility Checker before a save (Continue or Cancel) is a later item. The words are, in order: "DiaCraft read the file back and compared it with the drawing, and the file differs in {n} way(s). The drawing in DiaCraft is as it was.", a line "{class}: {count and what is true of it}" for each class, "Written as values the file format holds: units, the form of a formula (Width*1 for Width), values taken from style sheets, and links to style sheets. It is the same content.", and for a check that could not finish "The file was written; DiaCraft could not check it: {reason}". With nothing to say there is no card. **What the check judges and how far it goes.** The comparison goes both ways for the cells: a cell, row or section the file holds that the drawing's sheets (its own, a master's, a style sheet's) give nothing for is a difference in its class unless it holds what nothing gives a cell of its kind (the number 0, black, nothing); a font face, a part or a property more is no difference. **A font face that is its name and nothing more** (no attribute of a VSDX `FaceName`, no markup of an `.odg` declaration, nothing of DiaCraft's own) **is looked for in the file only where a cell names it**: a `Font`, `AsianFont` or `ComplexScriptFont` cell of a `Character` row or a `BulletFont` cell of a `Paragraph` row, of a style sheet or a shape, by the face's name in any case or by its number (its `ID`, else its place in the table); the theme marker and `0` name none. The one row a VSDX writer puts in the font table of a new drawing (the format asks for a row, and nothing names it), which a drawing made from a built-in template and every VSDX file written from a new drawing hold, is in no `.odg`, which declares the faces its texts use, and is no missing font. A face a cell names, and one that holds more than its name, is counted in `fonts` when the file lacks it. A glued end's `BeginX`, `BeginY`, `EndX` and `EndY` are the connection's, which the readers make from the glue, and are judged in `connections` and not in `formulas`; a connection is the same when it joins the shapes paired with its own, whatever number the file gave them. A connector's `ShapeRouteStyle` and `ConLineRouteExt` are judged by the route they give, each the connector's own or, where that is 0, the page's `RouteStyle` and `LineRouteExt`: the `.odg` writer says the route a connector is drawn in on the connector, so one that took its route from the page comes back with the same route said on it, which is the same content. A line end's size that no sheet gives is the medium size it is drawn with, 2; a cell of the text block that the file does not hold reads its default formula (`Width*0.5` for `TxtPinX`), so a drawing's cell that holds that formula comes back as it; and the label of a line or a connector with no text (its text block's place and the white background the engine writes when it routes one) is no difference when the file holds none of it, though a background of another colour is. A mark or a drawing object a text holds where it stands (a bookmark, a picture anchored as a character) is compared too, as a picture when it shows one and as kept data otherwise. A drawing made from a template file of another format is checked when it is saved as `.diacraft`. The steps the check may take are 60,000,000, or, for larger drawings, a figure in proportion to what the two drawings hold, so that a walk of every cell of a large drawing finishes and a scan of every item against every other still stops. **A face or a colour a cell names by a number** (a `Font` cell's `ID` or place in the table, a colour's index past the fixed palette) **is found in a table of the drawing's faces or colours made once and charged a step for each entry before it is made**, so that a drawing with many such cells and a large table is not a walk of the table for each cell. The rows of a section and the sections of a sheet are paired by key, the n-th of a key in the drawing with the n-th in the file, and a deleted row or section with none: a file that holds a key twice (two `Paragraph` rows of one `IX`, two `Paragraph` sections, a deleted `Geometry` row and a row with no `IX` that the rows before it give the same number) and is written back as it was names nothing, and a row the file holds as deleted is a row not in the file. The paragraphs a picture frame's `draw:image` holds, which DiaCraft's reader of an OpenDocument drawing keeps whole on the picture, are compared too, and counted in `text` when they do not come back. **The runs of a paragraph are told by the characters they cover, and not by where one ends and the next begins**: a writer joins neighbouring spans that give one formatting, and a file read back with fewer runs over the same characters is the same text and formatting. A run that is an element (a hyperlink, a field, a span that holds an attribute besides its style name) is looked for in the file at the same characters, in order, and a hyperlink the file lacks is counted in `hyperlinks`, any other element in `text-formatting`; a plain span is the values of its style that no cell says, and each character of the drawing's has the values of the spans over it in the file's too, whichever way those spans begin and end. A plain span that holds none says nothing of its characters here (the `Character` rows are compared with the sheet's).

**What a reader does not hold** (informative). The check after a save compares the drawing with the file written, so it cannot see what the reader of the source left out of the drawing before the drawing existed. DiaCraft's reader of an OpenDocument drawing counts that by class as it reads, and the open says it, first among its warnings: a line "What DiaCraft does not hold of this file: a save into the file keeps it, and a copy saved as another file (.diacraft or another format), or a new drawing made from it, is without it", then one line for each class with its count, however many there are: shapes of the kinds kept only as placeholders (a dimension line, a callout, a form control, a 3-D scene), which a copy writes as plain shapes; shapes in hyperlinks that a modelled shape follows, which a copy writes over the shapes that were over them (the model keeps a hyperlink's element unmodelled, and a copy writes it after the shapes it models); font declarations that repeat a name, which the one table of fonts holds once (an exact repeat within one part, or one name declared otherwise in the two parts); style properties that no copy writes (a default tab distance); and, for a flat drawing, the pieces of its markup that no part of the drawing holds. A save into the file the drawing was opened from keeps them, since it splices into the file's bytes. What the reader keeps is not named: a master page that holds no shape (one that holds shapes is a background page), a font declaration no run names, and a style's second spelling of a font family or the type of a line that is off, which are written back while their cells are as read.

**A style sheet in an OpenDocument drawing** (7.3): the document file's `style` entry of the root of the style sheets has `root="1"` and no `name`; any `style` entry may say the style's links where they are not the parent the markup gives (`line-style`, `fill-style` and `text-style`: a number, or `-` for none), the root's shown name (`display`), its `name-u`, and, **when its `ID` is not its place**, its `ID` (`id`: an OpenDocument file numbers its common styles by their places, and the reader gives the style at place *p* the `ID` *p* unless an entry says another). Every link of a model is an `ID`: the reader puts the links it made from the markup (a shape's parent, a style's parent) through the map the entries make before it reads them over the style sheets, and a link an entry gives is an `ID` already. An `id` that is no number, or one that another style sheet holds, is ignored (the first claim stands), and a style sheet that claims none keeps its place's number when no style holds it, and otherwise the next number past all the others. A style sheet whose three links differ, whose name is empty, too long or another style's is written as an empty common style under a name of its own, and these entries carry the truth. **A style sheet of an earlier ODF version**: a writer that labels the file 1.4 writes in 1.4's form what a style read from a file labelled 1.0 or 1.1 holds and the 1.4 schema refuses, and the entry's `older` children say the source's form, one each: `key` and `value` for a property left out (`style:paragraph-properties/text:enable-numbering`, keyed as the reader keys the style's unmapped properties), `named` for a list style given a name it had none of (`DiaCraft_20_list_20_<n>`, the least `n` from 1 that no list style of the style holds), and `list`, `level` and `width` for a label width (`text:min-label-width`) below 0 that was written as 0 in its unit. A reader puts each back where it still fits: the property when the style has none of that key, the name taken out of the list style that has it, the width where the level of that list style is at 0; what another program renamed or changed is left as the file has it. A reader keeps only a property 1.4 refuses, the first record of a key, and at most 4,096 records for a style (a writer writes no more, and refuses a style that would need more); a second entry for the same style says nothing of it. A font size of 0 is written as a tenth of a point, and the style's `Character` rows ride as any cells do.

## 8. Test vectors

### 8.1 How to read them

A package is shown as an **entry list**: the name, how it is stored (stored, or deflated at any level) and the content. Every ZIP entry is dated 1980-01-01 00:00:00. The XML texts are shown with line breaks and indentation, for reading. **The bytes of an entry are the declaration line `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`, a carriage return and a line feed, and then the element with every line break and the spaces after it removed.** That is what a writer writes, and a reader reads either form. In a few vectors only the lines that change from an earlier vector are shown.

The texts of the document and page parts of these vectors were checked against DiaCraft's own reader and writer of the vocabulary of section 3: each decoded to the model described, and was written back to exactly the bytes of the canonical form. `manifest.xml` and `state.xml` follow section 2 and were checked for well-formedness only.

### 8.2 Seven packages

**Vector 1: a blank drawing.** Kind `drawing`.

| Entry | Stored | Content |
|---|---|---|
| `mimetype` | stored | `application/vnd.diacraft.drawing` |
| `manifest.xml` | deflated | below |
| `document.xml` | deflated | below |
| `pages/page1.xml` | deflated | below |
| `state.xml` | deflated | below |

`manifest.xml`:

```xml
<diacraft:manifest xmlns:diacraft="urn:app.diacraft:extension" format="1.0" kind="drawing" written-by="DiaCraft 0.2.0">
  <diacraft:part path="document.xml" role="document" required="1"/>
  <diacraft:part path="pages/page1.xml" role="page" required="1"/>
  <diacraft:part path="state.xml" role="state"/>
</diacraft:manifest>
```

`document.xml`:

```xml
<diacraft:document xmlns:diacraft="urn:app.diacraft:extension" title="Untitled">
  <diacraft:style name="No Style" name-u="No Style">
    <diacraft:sheet>
      <diacraft:Cell N="BottomMargin" V="0.05555556"/>
      <diacraft:Cell N="DisplayMode" V="2"/>
      <diacraft:Cell N="EnableFillProps" V="1"/>
      <diacraft:Cell N="EnableLineProps" V="1"/>
      <diacraft:Cell N="EnableTextProps" V="1"/>
      <diacraft:Cell N="FillBkgnd" V="0"/>
      <diacraft:Cell N="FillForegnd" V="1"/>
      <diacraft:Cell N="FillPattern" V="1"/>
      <diacraft:Cell N="IsSnapTarget" V="1"/>
      <diacraft:Cell N="IsTextEditTarget" V="1"/>
      <diacraft:Cell N="LeftMargin" V="0.05555556"/>
      <diacraft:Cell N="LineColor" V="0"/>
      <diacraft:Cell N="LinePattern" V="1"/>
      <diacraft:Cell N="LineWeight" V="0.01"/>
      <diacraft:Cell N="RightMargin" V="0.05555556"/>
      <diacraft:Cell N="SelectMode" V="1"/>
      <diacraft:Cell N="TopMargin" V="0.05555556"/>
      <diacraft:Cell N="VerticalAlign" V="1"/>
      <diacraft:Section N="Character">
        <diacraft:Row IX="0">
          <diacraft:Cell N="Color" V="0"/>
          <diacraft:Cell N="Size" V="0.1666667"/>
        </diacraft:Row>
      </diacraft:Section>
    </diacraft:sheet>
  </diacraft:style>
  <diacraft:page-ref id="1" part="pages/page1.xml"/>
</diacraft:document>
```

`pages/page1.xml`:

```xml
<diacraft:page xmlns:diacraft="urn:app.diacraft:extension" id="1" name="Page-1" name-u="Page-1">
  <diacraft:sheet>
    <diacraft:Cell N="DrawingScale" V="1" U="IN"/>
    <diacraft:Cell N="PageBottomMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageHeight" V="8.5" U="IN"/>
    <diacraft:Cell N="PageLeftMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageRightMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageScale" V="1" U="IN"/>
    <diacraft:Cell N="PageTopMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageWidth" V="11" U="IN"/>
  </diacraft:sheet>
</diacraft:page>
```

`state.xml`:

```xml
<diacraft:state xmlns:diacraft="urn:app.diacraft:extension"/>
```

*It decodes to:* a drawing titled `Untitled`, with no masters, no themes, no colours, no fonts, no properties and no held data. It has one style, `id` 0, named `No Style` (name and universal name), whose sheet holds the 18 cells shown and one section `Character` with one row, `IX` 0, of the cells `Color` 0 and `Size` 0.1666667. It has one page, `id` 1, named `Page-1`, whose sheet holds eight cells in inches (`PageWidth` 11, `PageHeight` 8.5, margins 0, `DrawingScale` 1, `PageScale` 1), and no shapes and no connections. The state has no content stamp and no ledger entry. Its content stamp (3.17) is `00ebc460d282d097`. **Its root style holds no `DropOnPageScale`**, as the root of a new drawing held none before a change; a new drawing's root now holds the cell (`<diacraft:Cell N="DropOnPageScale" V="1"/>` among the style's cells, 19 in place of 18, as every file of the corpus holds it in its root), and the content stamp of a new blank drawing was `2a5bda518f5b4efb` from then. **Its root style states no text cell the renderer assumes** beyond its size and colour, as the root of a new drawing stated none before a change; a new drawing's root now holds them (the text block's `DefaultTabStop` 0.5 and `TextBkgnd`, `TextBkgndTrans` and `TextDirection` 0, which makes 23 cells in place of 19; a `Character` row `IX` 0 of 12 cells in place of 2; and a `Paragraph` section with one row, `IX` 0, of 12 cells, `HorzAlign` 1, `SpLine` -1.2 and the rest: what the renderer assumes where no cell says), and the content stamp of a new blank drawing is `c43fd9d9d0153d90`. All are drawings of this vocabulary: a root is the file's, a reader makes no cell of one that it lacks, and the vectors below stay as they are.

**Vector 2: one rectangle.** Entries as Vector 1, except `pages/page1.xml` and `state.xml`. The manifest is Vector 1's.

`pages/page1.xml`:

```xml
<diacraft:page xmlns:diacraft="urn:app.diacraft:extension" id="1" name="Page-1" name-u="Page-1">
  <diacraft:sheet>
    <diacraft:Cell N="DrawingScale" V="1" U="IN"/>
    <diacraft:Cell N="PageBottomMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageHeight" V="8.5" U="IN"/>
    <diacraft:Cell N="PageLeftMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageRightMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageScale" V="1" U="IN"/>
    <diacraft:Cell N="PageTopMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageWidth" V="11" U="IN"/>
  </diacraft:sheet>
  <diacraft:shape id="1" name="Rectangle" type="Shape">
    <diacraft:sheet>
      <diacraft:Cell N="Height" V="1" U="IN"/>
      <diacraft:Cell N="LocPinX" V="1" F="Width*0.5" U="IN"/>
      <diacraft:Cell N="LocPinY" V="0.5" F="Height*0.5" U="IN"/>
      <diacraft:Cell N="PinX" V="3" U="IN"/>
      <diacraft:Cell N="PinY" V="2" U="IN"/>
      <diacraft:Cell N="Width" V="2" U="IN"/>
      <diacraft:Section N="Geometry" IX="0">
        <diacraft:Row IX="1" T="MoveTo">
          <diacraft:Cell N="X" V="0"/>
          <diacraft:Cell N="Y" V="0"/>
        </diacraft:Row>
        <diacraft:Row IX="2" T="LineTo">
          <diacraft:Cell N="X" V="2" F="Width"/>
          <diacraft:Cell N="Y" V="0"/>
        </diacraft:Row>
        <diacraft:Row IX="3" T="LineTo">
          <diacraft:Cell N="X" V="2" F="Width"/>
          <diacraft:Cell N="Y" V="1" F="Height"/>
        </diacraft:Row>
        <diacraft:Row IX="4" T="LineTo">
          <diacraft:Cell N="X" V="0"/>
          <diacraft:Cell N="Y" V="1" F="Height"/>
        </diacraft:Row>
        <diacraft:Row IX="5" T="LineTo">
          <diacraft:Cell N="X" V="0"/>
          <diacraft:Cell N="Y" V="0"/>
        </diacraft:Row>
      </diacraft:Section>
    </diacraft:sheet>
    <diacraft:text>Hello</diacraft:text>
  </diacraft:shape>
</diacraft:page>
```

`state.xml`:

```xml
<diacraft:state xmlns:diacraft="urn:app.diacraft:extension">
  <diacraft:ledger page="1" mark="1"/>
</diacraft:state>
```

*It decodes to:* Vector 1's drawing, and on page 1 one shape: `id` 1, `type` `Shape`, named `Rectangle`. Its sheet has six cells (`Height` 1 IN; `LocPinX` with the value 1, the formula `Width*0.5` and the unit IN; `LocPinY` 0.5, `Height*0.5`, IN; `PinX` 3 IN; `PinY` 2 IN; `Width` 2 IN) and one section `Geometry`, `IX` 0, with five rows: `IX` 1 of type `MoveTo` and `IX` 2 to 5 of type `LineTo`, each with an `X` and a `Y` cell. Four of its ten geometry cells have a formula (`Width` or `Height`). Its text is one run, `Hello`. The ledger says page 1 has had shape `id` 1: a program that reads it takes 1 as the page's mark.

**Vector 3: a kept attribute that holds a tab.** Entries as Vector 2, except `pages/page1.xml`: Vector 2's, with this line added after `<diacraft:text>Hello</diacraft:text>`:

```xml
<diacraft:attr name="Tag" value="a&#9;b"/>
```

*It decodes to:* Vector 2's drawing, and the rectangle holds one kept attribute: the name `Tag`, the value `a`, a tab character, `b`. A writer writes the tab as `&#9;`. A raw tab in an attribute value would be read by an XML parser as a space, so a writer that wrote one would change the value.

**Vector 4: a forbidden character.** Entries as Vector 2, except `pages/page1.xml`: Vector 2's, with the start tag of the shape replaced by:

```xml
<diacraft:shape id="1" name-b64="QQFC" type="Shape">
```

*It decodes to:* Vector 2's drawing, and the rectangle's name is the three characters `A`, U+0001, `B`. `QQFC` is the base64 of the three bytes `41 01 42`. U+0001 is a control character XML 1.0 forbids, so a writer MUST write the name in the `-b64` form, and no raw U+0001 appears in any part. A reader that read `name-b64="!!"` (not base64) would refuse the part.

**Vector 5: a picture shown by two shapes, stored once.** Kind `drawing`.

| Entry | Stored | Content |
|---|---|---|
| `mimetype` | stored | as in Vector 1 |
| `manifest.xml` | deflated | below, with a row for the blob |
| `document.xml` | deflated | Vector 1's, with the line below added after the `page-ref` |
| `pages/page1.xml` | deflated | below, with two shapes |
| `blobs/0.png` | stored | the 12 bytes `89 50 4E 47 0D 0A 1A 0A 00 01 02 03` |
| `state.xml` | deflated | below, with the mark 2 |

The bytes are a PNG signature and four more: not an image, which no reader decodes. They are stored because they begin with a PNG signature (2.6).

`manifest.xml`:

```xml
<diacraft:manifest xmlns:diacraft="urn:app.diacraft:extension" format="1.0" kind="drawing" written-by="DiaCraft 0.2.0">
  <diacraft:part path="document.xml" role="document" required="1"/>
  <diacraft:part path="pages/page1.xml" role="page" required="1"/>
  <diacraft:part path="blobs/0.png" role="blob" media-type="image/png"/>
  <diacraft:part path="state.xml" role="state"/>
</diacraft:manifest>
```

The line added to `document.xml`, after `<diacraft:page-ref id="1" part="pages/page1.xml"/>`:

```xml
<diacraft:blob path="blobs/0.png" media-type="image/png" size="12" stamp="71df7ba063b3b4da"/>
```

`pages/page1.xml`:

```xml
<diacraft:page xmlns:diacraft="urn:app.diacraft:extension" id="1" name="Page-1" name-u="Page-1">
  <diacraft:sheet>
    <diacraft:Cell N="DrawingScale" V="1" U="IN"/>
    <diacraft:Cell N="PageBottomMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageHeight" V="8.5" U="IN"/>
    <diacraft:Cell N="PageLeftMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageRightMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageScale" V="1" U="IN"/>
    <diacraft:Cell N="PageTopMargin" V="0" U="IN"/>
    <diacraft:Cell N="PageWidth" V="11" U="IN"/>
  </diacraft:sheet>
  <diacraft:shape id="1" name="Picture 1" type="Foreign">
    <diacraft:sheet>
      <diacraft:Cell N="Height" V="1" U="IN"/>
      <diacraft:Cell N="PinX" V="2" U="IN"/>
      <diacraft:Cell N="PinY" V="2" U="IN"/>
      <diacraft:Cell N="Width" V="1" U="IN"/>
    </diacraft:sheet>
    <diacraft:foreign>
      <diacraft:attr name="ForeignType" value="Bitmap"/>
      <diacraft:picture part="media/image1.png" type="image/png" blob="0"/>
    </diacraft:foreign>
  </diacraft:shape>
  <diacraft:shape id="2" name="Picture 2" type="Foreign">
    <diacraft:sheet>
      <diacraft:Cell N="Height" V="1" U="IN"/>
      <diacraft:Cell N="PinX" V="5" U="IN"/>
      <diacraft:Cell N="PinY" V="2" U="IN"/>
      <diacraft:Cell N="Width" V="1" U="IN"/>
    </diacraft:sheet>
    <diacraft:foreign>
      <diacraft:attr name="ForeignType" value="Bitmap"/>
      <diacraft:picture part="media/image1.png" type="image/png" blob="0"/>
    </diacraft:foreign>
  </diacraft:shape>
</diacraft:page>
```

`state.xml`:

```xml
<diacraft:state xmlns:diacraft="urn:app.diacraft:extension">
  <diacraft:ledger page="1" mark="2"/>
</diacraft:state>
```

*It decodes to:* Vector 1's drawing, with one blob (number 0, `image/png`, 12 bytes, the stamp shown), and on page 1 two shapes of type `Foreign`, `Picture 1` (`PinX` 2) and `Picture 2` (`PinX` 5), each 1 by 1 inch. Each has the foreign attribute `ForeignType` = `Bitmap` and a picture of part name `media/image1.png`, type `image/png`, whose bytes are blob 0. **Both pictures hold the very same bytes**, and the package has one `blobs/` entry. The record's `id` is absent because it is 0.

**Vector 6: an unknown part from a newer minor version.** Entries as Vector 1, plus one entry, and the manifest changed:

| Entry | Stored | Content |
|---|---|---|
| `notes/review.txt` | stored or deflated | the 17 bytes `check the margins` |

`manifest.xml`:

```xml
<diacraft:manifest xmlns:diacraft="urn:app.diacraft:extension" format="1.2" kind="drawing" written-by="Example 1.2">
  <diacraft:part path="document.xml" role="document" required="1"/>
  <diacraft:part path="pages/page1.xml" role="page" required="1"/>
  <diacraft:part path="state.xml" role="state"/>
  <diacraft:part path="notes/review.txt" role="annotation" media-type="text/plain"/>
</diacraft:manifest>
```

*It decodes to:* Vector 1's drawing, with one extra part (5.2): the path `notes/review.txt`, the role word `annotation`, the media type `text/plain`, and the 17 bytes. Its role is not one of this version's, and it is not required, so the file opens. *Written again by a writer of this version:* the manifest says the writer's own version (`format="1.1"` for the version this document specifies), the entry `notes/review.txt` is present with the same bytes at the same path, its manifest row still says `role="annotation"` and `media-type="text/plain"`, and it comes after `state.xml`. Nothing is written for it in `document.xml`.

**Vector 7: unknown content inside a model part.** Entries as Vector 1, except `pages/page1.xml`: Vector 2's, with the start tag of the page replaced by the first line below, and the second line added between `</diacraft:sheet>` of the shape and `<diacraft:text>`:

```xml
<diacraft:page xmlns:diacraft="urn:app.diacraft:extension" id="1" name="Page-1" name-u="Page-1" layer-hint="3">
<diacraft:glow xmlns:diacraft="urn:app.diacraft:extension" radius="2"/>
```

*It decodes to:* Vector 2's drawing, in which the page holds one unknown attribute (`layer-hint`, value `3`) and the rectangle holds one unknown element, `glow` of this format's namespace with the attribute `radius` = `2`, placed after one known child, its `sheet`. A reader of this version does not know them and keeps them where they stood. *Written again:* the same attribute and element are in the same places, and the part's bytes are the ones above. (The repeated declaration of the namespace on `glow` is how a writer writes an unknown element; a reader that reads the part without it, and a writer that writes it, agree on the model, and the first write adds it.)

### 8.3 Refusals

Each row is Vector 2 or Vector 5 with one change. "Refused" means the reader refuses the file in words and does not open any part of it.

| Given | Expect |
|---|---|
| Vector 1 with the manifest `format="2.0"` | Refused; the words name the version (DiaCraft says "made by a newer DiaCraft (format 2.0)"). |
| Vector 1 with the manifest `format="1.7"` | Opens, as Vector 1. |
| Vector 1 with `kind="stencil"` in the manifest and `mimetype` unchanged | Refused as damaged: the kinds differ. |
| Vector 1 with an entry `extra.txt` that the manifest does not list | Refused as damaged. |
| Vector 1 with the manifest listing `pages/page2.xml`, which is not in the package | Refused as damaged. |
| Vector 6 with `required="1"` on the row of `notes/review.txt`, and the role `layers` | Refused; the words name the role `layers`. |
| Vector 1 with `mimetype` deflated, or not the first entry | Not a `.diacraft` file: not recognised, whatever its extension. |
| Vector 5 with `blobs/0.png` missing from the package | Opens; both pictures are lost, one note says one picture was lost, and the shapes show none. |
| Vector 5 with the last byte of `blobs/0.png` changed (12 bytes) | Opens, as the row above: the stamp does not match. |
| Vector 5 with 13 bytes in `blobs/0.png` | Opens, as the row above: the size does not match. |
| Vector 5 with `stamp="71DF7BA063B3B4DA"` in the blob record | Refused as damaged: a stamp is lowercase. |
| Vector 2 with a shape of `type="Hologram"` | Refused as damaged: the word is not on the list. |
| Vector 2 with a `Cell` that has no `N` | Refused as damaged. |
| Vector 2 with two cells `Height` in the shape's sheet | Refused as damaged. |
| Vector 2 with `id="4294967296"` on the shape | Refused as damaged. |
| Vector 2 with `name-b64="!!"` in place of `name` | Refused as damaged. |
| Vector 2 with `del="2"` on the shape | Refused as damaged: a flag is `1`, `0`, `true` or `false`. |
| Vector 2 with the root of the page part given `id="2"` | Refused as damaged: the list says 1. |
| Vector 1 with 200 shapes nested one in another on the page | Opens (the page part is 201 elements deep, which the depth of 512 allows). |
| Vector 1 with 201 shapes nested one in another on the page | Refused in words: more than 200 levels of nested shapes. |
| Vector 1 whose page holds two shapes, the first with a `sheet` that has `def="1"` and the second with `<diacraft:same of="1"/>` | Opens: the two shapes have the same sheet (3.19), and the drawing has the content and the stamp of the drawing whose second shape holds a copy of the first's sheet in full. |
| The same page with `<diacraft:same of="2"/>` in the second shape | Opens: no unit of the part is named `2`, so the `same` is not read and is counted (5.3), and the second shape has no sheet. |
| The same page with the `same` in the first shape and the `def` in the second | Opens: a unit is named by an element before it, so the first shape has no sheet (one `same` counted), and the second has its sheet. |
| The same page with `def="1"` on the sheets of both shapes | Opens: the first sheet has the name; the second, which gives it again, is read as a sheet of no name and counted, and the second shape keeps its sheet. |
| The same page with `def="1"` on a `Section` in the first shape's sheet, and in the second shape a `same of="1"` where the sheet stands | Opens: the name is a section's, and a sheet stands there, so the `same` is not read and is counted; the second shape has no sheet. |
| The same page with a `same` that has no `of` | Refused as damaged: a `same` must name what it stands for. |
| The page of two shapes whose first shape holds `<diacraft:sheet def="1" fp="5e36db3382a321fa"><diacraft:Cell N="Width" V="1"/></diacraft:sheet>` and whose second holds `<diacraft:same of="1"/>` | Opens: the fingerprint is the unit's (3.19: the domain `unit`, then the fields `<`, `sheet`, `<`, `Cell`, `N`, `Width`, `V`, `1`, `/`, `/`), and the two shapes have the sheet of a width of 1. |
| The same page with `V="2"` on the cell and `fp` as it is (a program of 1.0 edited the unit) | Opens: the first shape has the sheet it says, a width of 2; its name is not given (counted, as an attribute a reader does not read), so the `same` that names it is not read and is counted too, and the second shape has no sheet. |

### 8.4 Stamps

| Given | Expect |
|---|---|
| The domain `cells` and one text field, `LineWeight` | `7bce65b857cfdb09` |
| The stamp of the 12 bytes of Vector 5's picture (domain `bytes`) | `71df7ba063b3b4da` |
| The stamp of no bytes at all | `a09bd6c00ede502b` |
| The content stamp of Vector 1's model | `00ebc460d282d097` |
| The content stamp of Vector 2's model | `3ec10b64c29d5038` |
| The content stamp of Vector 3's model | `716eac1e9ba6c7e2` |
| The content stamp of Vector 4's model | `ff1285cc49617032` |
| The content stamp of Vector 5's model | `e10a91ed0d11dd5d` |
| The content stamp of Vector 7's model | `3828032e1f7dcf03` |

The first row is worked by hand like this: the domain field is the eight bytes `05 00 00 00 00 00 00 00` followed by the five bytes of `cells`, the text field is the eight bytes `0A 00 00 00 00 00 00 00` followed by the ten bytes of `LineWeight`, and the hash runs over those 31 bytes in that order from the offset basis. A program that reproduces the first three rows has the function; the others also test the walk of 3.17.

### 8.5 The content check

Each row gives two XML parts and the answer. The VSDX rows use no namespaces; the OpenDocument rows are all inside an `office:document` root that declares the `office`, `style`, `text`, `draw`, `svg` and `fo` prefixes of ODF 1.3. "Same" means no difference; "1 difference" means exactly one.

| Id | A | B | Expect |
|---|---|---|---|
| C1 | `<Shape ID="1"><Cell N="A" V="1"/><Cell N="B" V="2"/></Shape>` | the two cells in the other order | same (keyed `Cell`) |
| C2 | `<Shape ID="1"><Cell N="A" V="007"/></Shape>` | `V="7"` | 1 difference: `007` is text |
| C3 | `<Shape ID="1" P="1" Q="2"><Cell N="A" V="1"/></Shape>` | `<Shape  Q="2"  P="1"  ID="1"><!-- c --><Cell N="A" V="1"/></Shape>` | same: spacing, attribute order, a comment |
| C4 | `<Shapes><Shape ID="1"/><Shape ID="2"/></Shapes>` | the two shapes in the other order | 1 difference: shapes are ordered |
| C5 | `<Text><cp IX="0"/> </Text>` | `<Text><cp IX="0"/></Text>` | 1 difference: a space beside a marker is content |
| C6 | `<Rows> <Row IX="1"/> <Row IX="2"/> </Rows>` | `<Rows><Row IX="1"/><Row IX="2"/></Rows>` | same: blank text between children |
| C7 | `<Shape><Cell N="A" V="1"/></Shape>` | `<Shape><Cell N="A" V="1" U="IN"/></Shape>` | 1 difference: an attribute on one side only |
| C8 | `<Shape><?NewValue V?></Shape>` | `<?NewValue W?>` in its place | 1 difference: an instruction |
| C9 | `<a:r xmlns:a="urn:x"><a:t>x</a:t></a:r>` | the same with the prefix `b` | same: prefixes |
| C10 | `<a:r xmlns:a="urn:x"/>` | `<a:r xmlns:a="urn:y"/>` | different: another namespace |
| C11 | `<Shape><Cell N="A" V="1">a&amp;b</Cell></Shape>` | `<Cell N="A" V="1"><![CDATA[a&b]]></Cell>` | same: CDATA and references are resolved |
| C12 | `<r xmlns="urn:x"><a xmlns=""/></r>` | `<r xmlns="urn:x"><a/></r>` | 1 difference: `xmlns=""` puts `a` in no namespace |
| C13 | `<r><p:a xmlns:p=""/></r>` | the same with a space before `/>` | 1 difference (bytes): a prefix declared as the empty URI breaks a namespace constraint |
| C14 | `<r xmlns:p="urn:x" xmlns:q="urn:x" p:k="1" q:k="2"/>` | the same with `p:k="2" q:k="1"` | different: one attribute under two prefixes is compared in the order written |
| C14b | the first of C14 | the same with `q:k="1" p:k="2"` | same: the values in the same order, under prefixes that are free |
| O1 | `<draw:frame svg:width="2.50cm"/>` | `svg:width="2.5cm"` | same |
| O2 | `svg:width="2.5cm"` | `svg:width="25mm"` | 1 difference: units are not converted |
| O3 | `svg:width="2.5cm"` | `svg:width="+2.5cm"` | 1 difference: not a length |
| O3b | `svg:width="2.5cm"` | `svg:width="25e-1cm"` | 1 difference: not a length |
| O4 | `draw:opacity="75%"` | `draw:opacity="75.0%"` | same: a percentage is a number |
| O4b | `draw:opacity="75%"` | `draw:opacity="76%"` | 1 difference |
| O5 | `<text:p>a<text:s text:c="2"/>b</text:p>` | `<text:p>a <text:s/>b</text:p>` | same: both are `a`, two spaces, `b` |
| O5b | `<text:p>a<text:s/>b</text:p>` | `<text:p>a b</text:p>` | same |
| O5c | `<text:p>a<text:s text:c="3"/>b</text:p>` | `<text:p>a<text:s text:c="2"/>b</text:p>` | 1 difference |
| O5d | `<text:p>a<text:s text:c="2"/>b</text:p>` | `<text:p>a  b</text:p>` | 1 difference: a run of spaces in the text is one space |
| O6 | `<text:p>a  b</text:p>` | `<text:p>a b</text:p>` | same: a run of spaces collapses |
| O6b | `<draw:g draw:name="x  y"/>` | `draw:name="x y"` | 1 difference: not paragraph content |
| O6d | `<text:p> a</text:p>` | `<text:p>a</text:p>` | same: a leading space is dropped |
| O7 | `<office:styles>` with `style:style` named `a` and `b` of family `graphic`, in that order | the order `b`, `a` | same (keyed) |
| O8 | `<draw:layer-set>` with layers `a`, `b` | the order `b`, `a` | 1 difference: the layer set is ordered |
| O9 | `<office:styles/><office:body/>` | `<office:body/>` | same (4.7: an empty container is an absent one). |
| O9b | `<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body>` | `<office:body><office:drawing/></office:body>` | 1 difference: a page is content |
| O9c | `<text:p/>` | nothing | 1 difference: an empty paragraph is content |
| O9d | the five empty elements `office:automatic-styles`, `office:font-face-decls`, `office:master-styles`, `office:scripts` and `office:meta` | nothing | same. |
| O9e | `<office:settings/>` | nothing | same: an empty `office:settings` under the root of a flat document is none (4.5); inside any other element it is content |
| O10 | `svg:stroke-opacity="0.5"` | `"0.50"` | same |
| O11 | two `draw:page` named `a`, `b` | the order `b`, `a` | 1 difference: pages are ordered |
| O12 | two `draw:marker` named `m1`, `m2` | the order `m2`, `m1` | same (keyed) |
| O14 | `draw:z-index="007"` | `"7"` | 1 difference: an integer is not respelled |
| O15 | `office:version="1.2"` on the roots of the parts and `manifest:version="1.2"` on the manifest's root and root row | the same with `1.4` | same (4.5) |
| O16 | `<office:meta><meta:generator>A</meta:generator><dc:title>t</dc:title><meta:keyword>1</meta:keyword><meta:keyword>2</meta:keyword></office:meta>` | `<office:meta><meta:keyword>1</meta:keyword><meta:keyword>2</meta:keyword><dc:title>t</dc:title><meta:generator>B</meta:generator></office:meta>` | same: the generator and the order of the fields |
| O16b | the first of O16 | the keywords in the order `2`, `1` | 1 difference: the copies of a field are in the order written |
| O16c | a `meta.xml` that holds `<office:meta><meta:generator>A</meta:generator></office:meta>` | no `meta.xml` | same: it holds nothing |
| O17 | two graphic automatic styles `gr1` (fill `#ff0000`) and `gr2` (fill `#0000ff`), a shape that names `gr1` and one that names `gr2` | the same two styles named `x` and `y`, in the other order, and the shapes naming `y` and `x` | same: the names of automatic styles are free (4.5) |
| O17b | the first of O17 | the shapes naming the other of the two styles | 1 difference: a shape follows the style with its content |
| O17c | an automatic graphic style `gr1` and a shape that names it | a common graphic style `gr1` of the same content, and a shape that names it | different: a common style's name is kept |
| O18 | the manifest's rows with a row for the folder `Pictures/` that files of the package stand under | the same without it | same: a plain folder row says what the files say (4.5) |
| O18b | the manifest's rows with a row for the folder `Object 1/` typed as a chart, files standing under it | the same without it | 1 difference: a folder row with a media type that names a kind says more |
| O19 | a package | the same with a thumbnail entry and its manifest row | 1 difference: the entry (its row says what it says) |
| O21 | a package | the same with the entries `DiaCraft/document.xml` and `DiaCraft/blob0.bin`, their rows and the row of the folder `DiaCraft/` | same: DiaCraft's own carrier that only the second package has (4.5) |
| O21b | the second of O21 | the first of O21 | different: the entries the first held are missing from the second; the check is oriented |
| O21c | a package with the entry `Object 1/content.xml` | the same without it, and with `DiaCraft/blob0.bin` | 1 difference: the object's entry is missing; the carrier does not make it up |
| O21d | `<style:graphic-properties .../>` | the same with `diacraft:id="5"` | same; the other way round, 1 difference (the attribute of the source is missing) |
| O22 | one graphic style `gr1` and two pages whose shapes name it | the style and a copy `gr3` of the same content (with attributes of DiaCraft's namespace), the shapes naming `gr1` and `gr3` | same: copies count as one, and a shape follows the content |
| O23 | a paragraph style `P1` with `style:list-style-name="L1"` and the list style `L1` | the same named `P7`, `L5` | same: a reference from one automatic style to another is resolved |
| O23b | the first of O23 | `P1` naming `L1` and a list style named `L9` | 1 difference: `L1` names no definition of the second file |
| O23c | a text style `T1` with `style:list-style-name="L1"` and a list style `L1` whose level names `T1` | the same with the names `T9`, `L9` | different: a circle is compared as written |
| O20 | a master page with no `draw:layer-set` | the same with a `draw:layer-set` | 1 difference: a layer set added where the source had none |

The rows O9, O9d and O9e state the rule of 4.7.

## 9. Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-27 | minor | 8.2: Vector 1's decoded text says its root style states no text cell beyond a size and a colour, which a new drawing's root does now (a `Paragraph` row, the rest of the text block and the cells of the first run), and the stamp of the blank drawing it makes; no vector changed |
| 2026-11-27 | minor | 7.3, 7.5: the attribute `connects` is no longer written (a segment glued at an end is a `draw:connector` of the type `line` glued to its shapes, with its geometry in the sections of its style), and the reader still takes it from a file an earlier build wrote |
| 2026-11-27 | minor | 3.15: the paragraph on how the `.odg` model writer writes a shape says a held geometry is written with the shape's mirrors and the turn of its text, a glue point element in the page's axes is written again after a turn or a flip, and a flip or a text of an extrusion or a fontwork is refused in words |
| 2026-11-26 | minor | 7.3, 7.5: the attribute `connects` carries the glue of the ends of a segment the file holds as a polyline or a path, and a segment so written is judged by the box and the points it draws and not by a stamp of its points' text |
| 2026-11-26 | minor | 7.7: the check after a save finds the face or the colour a cell names by a number in a table made once and charged a step for each entry, and the formulas of a new drawing's shapes (a fixed colour's `RGB(r,g,b)`, a typed formula, a pin) come back through an `.odg` as the formulas they were, a drawing from a VSDX file's having done so; informative, no rule of the file format changes |
| 2026-11-26 | minor | 7.7: the check after a save looks for a font face that is its name and nothing more only where a cell names it, so the row a VSDX writer puts in a new drawing's font table is no missing font in an `.odg`; informative, no rule of the file format changes |
| 2026-11-26 | minor | 8.2: Vector 1's decoded text says its root style holds no `DropOnPageScale`, which a new drawing's root holds since the ruling on F40, and the stamp of the blank drawing it makes; no vector changed |
| 2026-11-26 | minor | 2.6: a writer stores a blob that deflate would make a thousand times smaller and more, which a reader refuses as an entry past the ratio of 1,000 to 1; no reader changes, and no file a build wrote before it is affected |
| 2026-11-26 | minor | 4.5: the content check of two `.odg` files counts an `xml:id` that only the written file has, beside an equal `draw:id` on its element, as nothing (oriented: one the source holds, or that says another value, is a difference), since a file of ODF 1.0 or 1.1 names a shape by `draw:id` alone and a writer that says ODF 1.4 writes the pair |
| 2026-11-26 | minor | 3.15: a style written whole is written in the form ODF 1.4 takes on its paragraph and text properties: `text:enable-numbering` rides in `diacraft:older-enable-numbering` in its place, other attributes the schemas have no place for are left out and said, and a weight, a slant or a variant 1.4 lists no value for is written as the value it lists nearest with the value it had in `diacraft:older-font-weight` and its kin, which a read gives back while the attribute beside it is the one written |
| 2026-11-25 | minor | 3.15: a kept style's family may be `unused` or `unused-styles` (an automatic style that nothing names, kept whole, as one `el`), and a style sheet's group `Style` may be held by the spelling the file gave it, as a `held-style` of the group `Style` (the vocabulary is the same: `family` and `group` are text, and `Style` was in the list) |
| 2026-11-25 | minor | 4.2, 4.3, 4.4, 4.5 and 4.8: the content check sets the children of an element that are not keyed side by side by what they are (the same content, the attributes a format lists as naming a thing, the one name of its kind) and not by place, so that an element put in or taken out is one difference and every sibling after it is compared in depth with its own, the same children in another order are one difference, and what is inside them is still found; lists past 262,144 children are set by place and counted; the manifest of an OpenDocument package may begin with a DOCTYPE that holds only an external id, which is no content; no rule of the file format changes |
| 2026-11-24 | minor | 2.8, 3.1 and 7.4 (GATE.b94e.F1): a file in the earlier namespace is unchanged when its document part is the part the writer makes with the current namespace in it, and keeps its preview; an attribute of the standard's namespaces that ODF 1.2 and 1.4 give no place on an automatic style's properties is not written into a part of 1.2 or later (`text:enable-numbering` rides in `diacraft:older-enable-numbering`); a flat file's extension under a root that binds its prefix to the earlier namespace is held in the current one |
| 2026-11-24 | minor | 3.1, 3.17, 4.3 and 7.2 (APPID): the namespace is `urn:app.diacraft:extension`; the earlier one is read as the same namespace and never written; a package that holds both reads as one document; a format's list of spellings of one name (4.3) lets the content check read the two as one; a stamp is fed the namespace as version 1.0 wrote it (3.17), so that every stored stamp stays good |
| 2026-11-23 | trivial | 4.4 and 8 (informative, no rule of the format changes): the sources of 4.4 say in words what gives a page its rows |
| 2026-11-23 | minor | 3.15: the paragraph on how the `.odg` model writer writes a shape says a frame's kept children carry `after` 0 or 1 by the side of the child the frame shows they stood on, a picture frame writes its own picture among them, a link that names no entry of the package is written out of it, and the glue points of a geometry the reader stops at (an extrusion, a fontwork, a view box with no width or height) are no rows |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-23 | minor | 7.3, informative: a save that writes the document file again writes each entry of `DiaCraft/` it names with the bytes its record states, and takes out the entries it no longer names (an entry of the name was taken for the one named, so a document file named an object's bytes in an entry that held a VBA project, which went into a second entry) |
| 2026-11-23 | minor | 3.15 and 7.4: an element an `.odg` drawing keeps whole, a text element it keeps and a properties element of an automatic style it keeps declare the prefixes they use that an element above them made below the roots, and a shape's kept attributes carry the declaration beside them (`xmlns:p` among them), so the writer writes them again bound to the namespace they were read in; what it cannot write it says |
| 2026-11-23 | minor | 3.15 and 7.4: the root of a flat file, which is the root of both parts, is held as the `root` entries `content.xml` and `styles.xml`, so that a package written from the file or from the model declares the prefixes of the markup the model keeps and writes a hyperlink of a flat drawing again |
| 2026-11-23 | minor | 7.3: the object a VSDX file holds as an embedding is held once by an edited save into it too, and a drawing made from a template that holds frames of an OpenDocument drawing is saved again as it was (informative). |
| 2026-11-23 | minor | 7.3 and 7.7: the rows of a group's own `draw:glue-point` elements, unchanged since they were read, are no data of its `group` entry when the model writer writes the drawing; the check after a save tells the runs of a paragraph by the characters they cover, counts a hyperlink the file lacks in `hyperlinks`; informative, no rule of the file format changes |
| 2026-11-23 | minor | 7.3: a save into a file the program wrote is given the drawing as that file holds it: an object held as an object frame does not ride again, and the sections of a sheet are compared by name (informative; no rule of the file format changes). |
| 2026-11-23 | minor | 7.3: what a Save As is asked to leave out of a file stays out of every save into that file, and the drawing keeps it (informative; no rule of the file format changes). |
| 2026-11-23 | minor | 7.7: the check after a save pairs the rows and sections of one key in order, a deleted one with none, and compares the paragraphs a picture held; informative |
| 2026-11-23 | minor | 7.3 and 7.7: a group that holds no connection points has a `group` entry whose `gone` says so (a reader gives every group the four standard ones), and the check after a save judges a connector's `ShapeRouteStyle` and `ConLineRouteExt` by the route they give, the connector's own or the page's, a line end's size by the size it is drawn with (2), a text block cell the file does not hold by its default formula, and the label of a line or a connector with no text as the engine's; informative, no rule of the file format changes |
| 2026-11-23 | minor | 7.3: the `document` item of an OpenDocument drawing also carries the descriptive fields of the properties that hold an empty text (a VSDX file's empty `dc:subject`, `cp:keywords` and the like), which `meta.xml` has no way to say; a reader puts one back only where the file has none, and the stamp of the item says an empty field as none, as the file does; and the entry of a group that is an instance of a master, or stands for a shape of one, carries its `id` whatever else it holds |
| 2026-11-23 | minor | 3.15 and 7.7: a master page that holds no shape is a kept style `master:master-page`, written back among the master pages (its children in the master page of its name, or whole), an automatic drawing-page style is written in the part of what names it, and the open of an OpenDocument drawing names what the drawing does not hold, a line for each class with its count (informative). |
| 2026-11-22 | minor | 7.3 and 7.7: a style's entry in an OpenDocument drawing may have `older` children (`key` and `value`, `named`, or `list`, `level` and `width`), which say the source's form of what a writer labelling the file 1.4 wrote in 1.4's form; a reader keeps a property 1.4 refuses, the first record of a key and at most 4,096 records a style, and puts each back where it still fits; the check's values group says them (an optional element; a reader that does not know it reads the style in 1.4's form) |
| 2026-11-22 | minor | 3.10 and 3.18: the document holds an optional `comment-list` (the authors and the comments of the drawing, replies as a `parent`), with `comment-author` and `comment-entry` and their attributes; the names are new and optional, so a 1.0 reader keeps them as unknown content |
| 2026-11-22 | minor | 7.3 and 7.7: what a LibreOffice save was measured to do with DiaCraft's data (it kept the entries byte for byte in 129 of 145 saves with no edit, the manifest listing the folder), and the note's `.odg` sentence: another program may keep the data or leave it out, and what it changes wins where DiaCraft can tell (informative; no rule of the file format changes) |
| 2026-11-22 | minor | 4.2, 4.3, 4.5, 4.6 and 8.5: for OpenDocument packages the content check is oriented by DiaCraft's own carrier: the entries under `DiaCraft/`, their manifest rows, and the attributes and elements of DiaCraft's namespace count as nothing when only the second (written) package has them, and what the first held is compared as written; copies of an automatic style count as one; a reference from one automatic style to another (`style:list-style-name`, `style:data-style-name`, `style:percentage-data-style-name`, the text style of a list level, `style:num-list-format-name`) is resolved to the content, bounded (64), a circle compared as written, `style:parent-style-name` never; the table of references follows the schema (`presentation:notes` and `style:handout-master` ask a drawing-page style; `text:visited-style-name`, `text:cond-style-name`, `text:section`, `text:ruby` and the lists of class names added; data styles and `number:num-list-format` are definitions); vectors O21 to O23c added; no rule of the file format changes |
| 2026-11-22 | minor | 7.7: the check after the save, informative: the structured `checked` value (`done`, `classes`, `as_values`, `reason`), its classes, the same content that is no difference, a formula that does not come back as a formula counted as lost, and its words. No rule of the file format changes. |
| 2026-11-22 | minor | 4.2, 4.3, 4.5, 4.6, 4.7, 4.8 and 8.5: for OpenDocument packages the content check counts as nothing `office:version` and `manifest:version`, the generator of `office:meta` and the order of its fields (the copies of one field in the order written), an empty `settings.xml` or `meta.xml` as no part and an empty `office:settings` under a flat root as none, the manifest's rows for an entry only its package holds or for a plain folder that files stand under, and the names of automatic styles, matched by content (a common style's name kept, a style whose property changed one difference); a layer set added where the source had none, `meta:user-defined` fields added, `dc:creator` or `meta:initial-creator` taken out, `meta:creation-date` added, `meta:print-date` taken out and every other difference still count; vectors O9e (changed) and O15 to O20 added; no rule of the file format changes |
| 2026-11-22 | minor | 2.9: the reference implementation draws the preview within a budget of work and to no time. |
| 2026-11-22 | minor | 7.4: a drawing that came from a flat `.fodg` is saved by Save As as a `.diacraft` file, an `.odg` package or a VSDX type, and never as a `.fodg`; its `office:meta`, `office:settings` and inline pictures come with it (informative; no rule of the file format changes). |
| 2026-11-22 | minor | 7.3: a `group` item of a group with no name whose stamp more than one such group of the page has is dropped with a note; 3 (informative): how the `.odg` model writer writes the style sheets, the fill images and a `settings.xml` for a model that holds none |
| 2026-11-22 | minor | 7.7: the save note, its classes and its words; `root`, `line-style`, `fill-style`, `text-style` and `display` on the `style` entries of the document file (additive, optional). |
| 2026-11-22 | minor | 7.3: the attributes `vsdx`, `foreign`, `fp-format`, `fp-vsdx` and `fp-foreign`, the `vsdx` items of the document file (`document`, `parts`, `page`, `style`, `group`) with their keys and stamps, the entries `DiaCraft/blob<n>.<ext>` and the records that name them, the names a VSDX part and an entry may have, the caps and the macro project as bytes; the stamp of a text covers its formatting, so a formatting edit in another program wins; LibreOffice 26.8.2.2 keeps the attributes and the folder with its row when it saves such a file (measured) |
| 2026-11-22 | minor | 7.2: a part of the other format named `meta.xml` or `settings.xml`, spelt so, is carried and read back; another spelling of them is refused as the other reserved names are |
| 2026-11-22 | minor | 3.15: the paragraph on how the `.odg` model writer writes this data says a shape is written as it was read (informative; no rule of the file format changes) |
| 2026-11-22 | minor | 7.2: a style's item is keyed by `id` where another style of the drawing has its name, or its name is longer than 4,096 bytes, and not only where it has none; a reader finds an `id` among all the styles whatever their names; two items that reach one style (by its name and by its `id`) are dropped with a word; a writer says an item was left out only for an object that holds data |
| 2026-11-22 | minor | The paragraph on `vsdx-held` says a drawing of no VSDX file holds `windows` alone once its window settings are set (informative; no rule of the file format changes) |
| 2026-11-22 | minor | 7.2: an object frame of an OpenDocument drawing that holds a compound file and a picture in a format a VSDX file draws is written as an embedding and rides nowhere else (the compound file check, the parts, the relationships, the types and the shape are stated); the stamp of a shape that shows a picture is of the picture's length and first eight bytes, and no longer of the name and type of its part or of the attributes a writer gives a picture or an object |
| 2026-11-21 | minor | 4.2 and 4.4: a part on one side only is a difference unless it is a relationships part by its name (`.rels` in a folder `_rels`) whose root is an empty `Relationships`, whatever the root of a part of another name holds; `Colors`, `FaceNames`, `StyleSheets` and `DocumentSheet` are the same as absent only as children of the document part's root element, `Connects` and `Shapes` only of `PageContents` or `MasterContents`, and `Shapes` of a `Shape`; no rule of the file format changes |
| 2026-11-21 | minor | 4.2 and 4.4: for VSDX packages the content check counts as the same a relationship row of a page that implies and the other package lacks, a part's content type given where it had none (a property part typed as the drawing and given its own, an `Override` for a relationships part and the `Default` for `rels`), and a `Row` with no `IX` and one whose `IX` is the count of the rows before it; no rule of the file format changes |
| 2026-11-21 | minor | 4.2, 4.4 and 4.7: for VSDX packages the content check counts an empty `Colors`, `FaceNames`, `StyleSheets`, `DocumentSheet`, `Connects` or `Shapes` as an absent one, a relationships part on one side only that holds no row as no part, and a shape's `Section`, `Text` and `ForeignData` children in any order among themselves as the same (the sections keeping their own order); no rule of the file format changes |
| 2026-11-21 | minor | 3.15: an informative paragraph says how the `.odg` model writer writes the title, the properties, `meta.xml`'s fields, `settings.xml`, the manifest, the entries and the containers' markup, and that every part states version 1.4 |
| 2026-11-21 | minor | 7.2: the encoding of the items that a VSDX file carries (their kinds, keys and stamps, the fragment of each, the position of a place in a sheet and of a text part that rides), the names, types and relationships of the parts of DiaCraft's own and which relationships are DiaCraft's, the caps (30,000 parts, items of at most 64 MiB), the rule for a macro project of the other format and for the role of a part, and what a reader does with an item are stated; the rows of 7.2 that said they were fixed and not written by any release are replaced by them; no rule of an earlier version changes |
| 2026-11-21 | minor | 3.15 and 3.18 (optional elements, no rule of the format changes): a style's `odf` may hold `held` (the values its cells only approximate, with the stamps of its own cells) and `held-style` (the values of its paragraph and text properties, with the stamp of their row); a paragraph's `element` (a heading, or its own attributes) and `level`s (the lists round it, each held by the paragraph that opens it); a run's `element` (a hyperlink, a field, a span's own attributes); the document's `kept` (the markup of its containers that no field reads) and the roots named below the root of a part (the containers' attributes); a text with no characters is a `kept` child of its shape's `odf`; a paragraph is in at most 256 levels of lists. A file made before has none of these and reads as it did |
| 2026-11-21 | minor | 6.4 (informative, no rule of the format changes): DiaCraft's reader reads `state.xml` as a tree of at most 2,000,000 items, so a ledger has at most 666,666 rows, and its cost is linear in the bytes of the part and bounded by that tree |
| 2026-11-21 | minor | 3.15: the family DiaCraft's reader of an `.odg` gives a kept style is the container (`common` or `auto`), a colon and the kind, a default style's name is empty, a style's unmapped pairs are keyed by the properties element and the attribute, and a unit is recorded only when no cell spells it; no word, element or attribute of the vocabulary changes |
| 2026-11-21 | minor | 3.15 says, informatively, how the `.odg` reader fills the document's data: a `font` for each distinct declaration, `title`, `props` and `custom` from `meta.xml` with the rest of its fields kept in place, the whole settings tree, the roots, and every entry no field reads as a `part` with the row that names it. |
| 2026-11-21 | minor | 7.3: a master past a cap of the model is left out like a damaged one and does not refuse the drawing; a save over a file of a higher version writes the version it wrote, and a save that replaces a file of a lower one says so (the vocabulary of 3 is unchanged) |
| 2026-11-21 | minor | 7.3: the `.odg` document file is version 2: its masters and the `sheet` elements of its entries are in the vocabulary of section 3, the pictures of the masters are `blob` children written once each, and a file of a lower version is kept as it is and not read, with a note (the vocabulary of 3 is unchanged) |
| 2026-11-21 | minor | 5.1, 5.5, 6.1 and 6.4 say one thing in one way: a minor version may add optional elements and attributes and raise a limit of 6.1 and never lowers one, a reader MUST open a file of a higher minor and MAY refuse it only in words that name a limit of its own (it was said as a MUST in 5.1 and a MAY in 6.1), and 6.4 quotes the words DiaCraft writes; 6.4 states that a child of a part (a group of tens of thousands of shapes, a path of hundreds of thousands of rows) may have up to 4,000,000 XML items, of which 2,000,000 are in the pool of one tree and the rest is counted as model while the child is held (a child was held to 2,000,000, which refused drawings the format allows); no element, attribute or default of the vocabulary changed |
| 2026-11-21 | minor | 6.1: the limit of XML items in one part is raised from 6,000,000 to 12,000,000 (version 1.1; the owner's ruling, so that every page the `.odg` reader opens is a part of the format: 76,000 rectangles that each have a sheet of their own are a page of 9.2 million items); a part of more than 6,000,000 items is a part of 1.1, and the manifest of a file that holds one does not say 1.0; a minor version may raise a limit of 6.1 and never lowers one (5.5); DiaCraft's reader refuses a streamed part of more than 12,000,000 items and its writer does not write one; no element, attribute or default of the vocabulary changed |
| 2026-11-21 | minor | 6.4: DiaCraft's own working limits change, which are an implementation choice and no rule of the format, and the writer holds a part made as a stream to the items of 6.1 (the limit is that of the row above): a child of a part and a part held whole are at most 2,000,000 items (they were 4,000,000), the document part's tree beyond 256,000 items is counted as model, and the model and the blobs have 1,344 MiB between them with the blobs at most 384 MiB of it (the model was held to 640 MiB); no element, attribute or default of the vocabulary changed |
| 2026-11-21 | minor | 6.4 and 3.19: a reader may read a page part or a master part as a stream of the children of its root (DiaCraft's holds a child to 4,000,000 items and the part to 128 MiB and 64,000,000 items), and a writer names a sheet or a section that its model holds in more than one place whether the places hold one allocation or equal copies; no element, attribute or default of the vocabulary changed, and a file of 1.1 is the same file |
| 2026-11-21 | minor | The hardening round of the package: a data descriptor on the `mimetype` entry does not stop a file from being recognised (2.3), and a writer that does not copy the entries of a file as they are writes its preview again when the drawing is unchanged (2.8) |
| 2026-11-21 | minor | The review of the package: a part that the lists name twice is damage, an entry named `document.xml`, `state.xml` or `preview.png` has the role of it, a version number is read to nineteen digits, a blob of a newer minor that no record names and whose checksum fails is a lost blob; these are marked as rules of this format; vector 6 is a file of the minor version 1.2 |
| 2026-11-21 | minor | The package is built: the document, state and preview rows name their own entries and a page or master entry that no list names is damaged (2.4, 2.10); vector 6 says the writer's own version; the two implementation notes that said the reference implementation wrote a part of an unknown role as a `part` element with a blob (3.14, 5.2) are gone, since it writes the entry at its own path |
| 2026-11-20 | minor | 3.19, version 1.1: a part says a shared sheet or section once (`def` on a `sheet` or a `Section`, and `same` with `of`); a `def` MAY carry `fp`, the fingerprint of its unit as 1.0 reads it, and a `same` that names no unit, a second `def` of a name (the unit is read as a unit of no name, and its shape keeps its sheet) and a unit whose `fp` no longer matches are counted (5.3) and never damage, so that what a program of 1.0 does to a file of 1.1 opens; 3.17 gives the stamp over the form of 1.0, so a stamp stored for a drawing stays good; **6.1 keeps the limits of 1.0 (it had been narrowed: the package 512 MiB and 4,000,000 items in a part), 6.4 says DiaCraft's own lower working limits, and a 1.0 file within 1.0's limits that DiaCraft refuses is refused in words that name its own limit** |
| 2026-11-20 | major | Created: the public specification of `.diacraft` 1.0, with the vocabulary of the model, the content check, versions, limits, the carriers and test vectors |
| 2026-11-22 | minor | 7.2, 7.3 and 7.7: the item `comments` (the thread of the replies in a VSDX file), the page's `office:annotation` elements of an OpenDocument drawing with DiaCraft's attributes, and the class `comments` of the check after a save |
| 2026-11-23 | minor | 7.7: what the check after a save judges and how far it goes: the cells both ways, a glued end as its connection, connections between the shapes paired, the marks and objects of a text, a drawing made from a template file, steps in proportion to the drawings. No rule of the file format changes. |
| 2026-11-23 | minor | 4.2 and 4.3: names are read as XML Namespaces 1.0 reads them wherever the check reads one (`xmlns=""` puts an element in no namespace, also where a rule lists the name); a part that breaks a namespace constraint is bytes; one attribute under two prefixes is compared in the order written, not sorted by value; blank text beside a child that counts as nothing is spacing |
| 2026-11-27 | minor | 7.2: the solution part is written with no XML declaration, as the solution parts of the files the VSDX application writes are; a reader reads it with or without, and a save that writes a file of the builds before 0.2.4 again cuts the declaration from its part |
| 2026-11-27 | minor | 3.15: a `draw:stroke="dash"` held beside a pattern that is a stand-in has no dash of DiaCraft's written beside it by the `.odg` model writer, and a style that names a dash of DiaCraft's own that its cells say again holds neither the stroke nor the dash |
