# Progress-doc templates

> **Last updated:** 2026-11-23 · **Change:** minor (the reference product is named only in the format lists, and the page keeps to the words of the public tree; before that, created from local notes)

How the progress docs are kept. `cargo xtask docs` checks part of this.

## Status line

Exactly one, directly under the H1, replaced on every edit:

```
**Last reviewed:** 2026-10-31 · **Last updated:** 2026-10-31 · **Change:** minor (one item; another item) · **Target:** the reference app's desktop edition
```

In a doc it is a blockquote, starting `> `.

`Change` is a level, `trivial`, `minor` or `major`, then one parenthesis of items separated by `; `, with no item twice. `Last reviewed` is when scores were last checked against evidence, and `Last updated` is the last edit. Docs with no scores leave out `Last reviewed` and `Target`.

## Revision history

The last section of every doc, newest first, with the columns Date, Change and Summary. The summary repeats the status line's `Change`. This page's own section below is the pattern.

## Numbers

Every headline number has a Kind: `measured (source)` or `estimated (method)`. Scores err low. Grade by behaviour, not by whether a menu item exists. Checklist rows are `| Area | Feature | P0/P1/P2 | D/P/M |`, with D 1.0, P 0.5 and M 0, and priorities weighted 3, 2 and 1.

## Honesty

Progress docs state that the reference app was not inspected and that our output has not been opened in it by the project. Claims about the real reference app stay marked unverified.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-10-31 | major | Created from local notes |
