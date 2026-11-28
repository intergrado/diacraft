# Localization parity

> **Last reviewed:** 2026-11-27

The target is the reference app's desktop edition, and M1 sets which release. The reference app was never taken apart: what it does comes from public documentation and public sample files, and the files DiaCraft writes are being tested by opening them in it, in LibreOffice Draw and in draw.io.

**Headline:** English only (measured). What the reference app ships in which languages is looked into later, from public sources only. Three designs and their plans were approved by the owner on 2026-11-26: translating the interface and the website, the language of text with spelling and proofing and in-app help, search and contextual help. Built so far: right-to-left text and its editing; the language of each run of text, a language tag read, kept and written in VSDX, `.odg` and `.diacraft` files; the settings file (`config.toml`, which the desktop programs write at their first start, holding the text's editing language taken from the system) and the language data tables; and the help crate with its topics and check, which no program uses yet: there is no in-app help. There is no interface-language setting in the window, no translation and no spelling check: the window, the command line and the site are in English until those tasks land.

| Language | UI strings translated | Status |
|---|---|---|
| English | Source language | Current |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-27 | minor | The headline says what of the three designs is built on `main` (right-to-left text and its editing, the language of text runs, the settings file written at the first start with the editing language from the system, the language data, the help crate, which no program uses yet), and that the window has no interface-language setting, where it said nothing was built |
| 2026-11-26 | minor | The headline says the translation, language and proofing, and in-app help designs were approved with their plans on 2026-11-26, and nothing of them is built |
| 2026-11-23 | minor | The headline says the languages are looked into later, from public sources only |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-10-31 | major | Created for M0 |
