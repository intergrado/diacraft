# Progress-doc templates

How the progress docs are kept. `cargo xtask docs` checks part of this.

## Status line

Only the scored progress docs have one: `ROADMAP.md`, `docs/gaps.md`, `docs/roadmap.md`, `docs/parity-checklist.md`, `docs/parity-evidence.md`, `docs/target-app-parity.md`, `docs/ui-parity.md`, `docs/file-format-parity.md`, `docs/hardware-parity.md` and `docs/localization-parity.md`. It is one line under the H1, and holds the date their scores were last checked against evidence and nothing else:

```
**Last reviewed:** 2026-10-31
```

In a doc it is a blockquote, starting `> `. The date moves when the scores are checked again, not on every edit. The line holds no date of the last edit, no list of changes and no history. What a scored doc measures against, its target, is said in its opening paragraph.

The other docs are reference docs, and have no status line: git records when each of them changed and what changed.

`cargo xtask docs` refuses a second status line, a status line in a doc that has no scores, and one that holds anything but the date.

## Revision history

The last section of every doc, newest first, with the columns Date, Change and Summary. The Change column is a level: `trivial`, `minor` or `major`. This page's own section below is the pattern.

## Numbers

Every headline number has a Kind: `measured (source)` or `estimated (method)`. Scores err low. Grade by behaviour, not by whether a menu item exists. Checklist rows are `| Area | Feature | P0/P1/P2 | D/P/M |`, with D 1.0, P 0.5 and M 0, and priorities weighted 3, 2 and 1.

## Honesty

Progress docs state that the reference app was never taken apart (what it does comes from public documentation and public sample files) and that the files DiaCraft writes are being tested by opening them in it, in LibreOffice Draw and in draw.io. Claims about how the real reference app behaves stay marked unverified.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-24 | minor | Only the scored docs have a status line, and it holds their review date alone; the reference docs have none, as git records their history |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-10-31 | major | Created from local notes |
