# DiaCraft branding artwork

Original artwork for DiaCraft, integrated in and corrected in to the owner's requested placement. The desktop uses the icon for its OS window decoration, and the shared desktop/web File Home displays the cover. The web page declares a local favicon, and the README displays the cover. Feature-parity scores are unchanged. The reference app was not inspected and our output has not been opened in it by the project.

## Deliverables

| File | Dimensions | Use |
|---|---|---|
| [Icon master](../assets/branding/diacraft-icon-master.png) | 1254 × 1254 px, RGB PNG | Opaque square source for platform icon exports |
| [Runtime icon](../assets/branding/diacraft-icon-256.png) | 256 × 256 px, RGB PNG | Native window icon and web favicon |
| [Cover](../assets/branding/diacraft-cover.png) | 1672 × 941 px, RGB PNG | Landscape cover, approximately 16:9 |
| [Splash copy](../assets/branding/diacraft-splash.png) | 836 × 470 px, 256-colour PNG, about 206 KB | The cover at half its size, the background of the splash popup and About DiaCraft (`crates/ui-egui/src/splash.rs`) |

The octopus coordinates a process rectangle and a decision diamond. Its branching arms connect the creature convention to the application's purpose. The cover pairs the same illustration with the name **DiaCraft** and the factual descriptor **Diagramming software**.

The art direction uses near-black ink `#0b0b0c`, warm paper `#efe9dc` and DiaCraft teal `#007a8c`. Teal comes from the current light-theme accent in `crates/ui-egui/src/theme.rs`. These are target palette values: the generated rasters include antialiasing and tonal variation, and are not exact three-colour indexed files.

The icon master is opaque and square. Its 256-pixel derivative preserves the approved composition. Platform-specific rounded clipping, macOS padding, small-size optical adjustments, ICO and ICNS packaging remain separate export work. The retained master avoids transparency defects seen in discarded generated drafts. No SVG master is claimed. Cover lettering is rasterized, generated artwork; no font file was used or added.

## App integration

`crates/ui-egui/src/branding.rs` decodes the bundled artwork under exact dimension and allocation limits, caches textures once per app, and reports load errors. **The web build does not bundle the cover or the splash copy**: `cargo xtask site --web` copies them into the site's `app/assets/` under names with their hashes, and the window fetches them when it first shows them (`asset_host.rs`); the decode and its limits are the same, and a picture that could not be had is a placeholder and a notice. The icon is the page's own `<link rel="icon">`. It does not change the hostile stencil-image decoder. The cover keeps its aspect ratio and uses at most 30% of the File Home body height, capped at 240 points. Home scrolls vertically, and keyboard focus scrolls the Blank drawing tile into view.

`apps/diacraft/src/main.rs` supplies the window icon to the native viewport. The in-app top bar has no application logo. The pinned window library ignores the runtime icon on Wayland, so Linux also needs the app-id-matched desktop entry in `packaging/linux/app.diacraft.desktop` and the icon installed in the user's or system's hicolor theme. macOS bundle icons and Windows executable resources remain packaging work.

For an installed `diacraft` executable on `PATH`, register the Linux identity with:

```sh
install -Dm644 assets/branding/diacraft-icon-256.png "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps/app.diacraft.png"
install -Dm644 packaging/linux/app.diacraft.desktop "${XDG_DATA_HOME:-$HOME/.local/share}/applications/app.diacraft.desktop"
```

On KDE, refresh the application cache with `kbuildsycoca6`. The desktop-entry filename, `Icon`, `StartupWMClass` and runtime app id all use `app.diacraft`, and so does the macOS bundle's `CFBundleIdentifier` (`packaging/macos/Info.plist`). The id comes from the domain `diacraft.app` (the owner, 2026-11-23), and DiaCraft's file namespace is `urn:app.diacraft:extension` ([the file format](diacraft-format.md), 3.1). Before that the id was `io.github.one_bit.diacraft`, which had been registered per-user on the development machine, with the desktop entry's `Exec` pointing to the verified `target/debug/diacraft` executable in this checkout; that registration, being another id, is another entry, and the old files are left where they are (remove them by hand when they are no use). The new id was checked on KDE Plasma under Wayland, with the release build: the two files above were put in the per-user folders and `kbuildsycoca6` was run, the window asked the compositor for the app id `app.diacraft` (`WAYLAND_DEBUG=1` shows the request `set_app_id`) and its title bar showed the octopus icon; with no entry registered the same build's window showed the compositor's generic icon, so the icon is the entry's; and the two files were taken out again. No system-wide files were changed. The portable repository entry keeps `Exec=diacraft %F`.

The web entry point uses Trunk's local icon asset pipeline. No JavaScript, fonts, dependencies or remote requests were added. The shared UI displays the same artwork on native and WASM builds.

The splash copy is the cover halved with the same filter and written as a palette picture with no dithering, so that the cream the text lies on keeps its exact colour (a dithered palette moved it): it is a mechanical export of the cover and shares its terms. The popup shows it at its size over the window's pixels to the point, with the version, the git release, the year and the licence painted over its lower left corner, in the two inks of the art (`splash_ink`, the near-black of the title, and `splash_link`, the teal of the subtitle, in `crates/ui-egui/src/theme.rs`); a test reads the picture and holds that corner clear of art.

```sh
magick assets/branding/diacraft-cover.png -filter Lanczos -resize 836x470! -strip -dither None -colors 256 PNG8:splash.png
zopflipng -y --iterations=15 splash.png assets/branding/diacraft-splash.png
```

The 256-pixel export was produced with ImageMagick 7.1.2-35 Q16-HDRI x86_64:

```sh
magick assets/branding/diacraft-icon-master.png -filter Lanczos -resize 256x256 -strip PNG24:assets/branding/diacraft-icon-256.png
```

## Reference evidence

Read on 2026-11-11 from local read-only clones of public sibling repositories:

- [GridCraft icon notes](https://github.com/storytold/gridcraft/blob/1f4c67ef0a0200e48d832c7cbbb5570b743b7837/assets/app-icon/README.md): the ledger icon is explicitly a placeholder; the stated finished convention is an engraved creature portrait, with ink, paper and a product colour.
- [WordCraft attribution](https://github.com/storytold/wordcraft/blob/aeae018cfd1c1f0f5bfa09ab45eefc1b4e9ac584/ATTRIBUTION.md) and [DeckCraft attribution](https://github.com/storytold/deckcraft/blob/4c1d77ddce19d75d4cea08da4b61260df250ae31/ATTRIBUTION.md): their current functional icons are also labelled placeholders.
- The icon README files and rendered icons in the public [VectorCraft](https://github.com/storytold/vectorcraft/tree/main/assets/app-icon), [DesignCraft](https://github.com/storytold/designcraft/tree/main/assets/app-icon) and [FigCraft](https://github.com/storytold/figcraft/tree/main/assets/app-icon) repositories illustrate the finished convention: engraved creatures, full-bleed product colour, ink and paper, with the body extending off the tile.

The shared standards repository was not accessible, so this is alignment with the conventions restated by the sibling repositories, not a claim of full compliance with unseen guidelines. Sibling images were viewed for context but not supplied to the generator, traced, copied or included. The new octopus artwork was generated from a text prompt; the cover used only DiaCraft's newly generated icon as its image reference.

## Generation record

Built-in `image_gen` was used, not the fallback CLI. The tool did not expose a model/version identifier. Prompt dimensions were requests; the table above records the measured output dimensions. Discarded drafts are not included.

### Initial icon prompt

```text
Use case: logo-brand.
Create a beautiful, polished professional application icon for DiaCraft, a diagram creation tool. Original artwork.
Subject: an intelligent octopus, depicted as a refined engraved natural-history portrait, coordinating a tiny flowchart. Tight heroic crop: large elegant rounded mantle and alert natural eyes in the upper-middle; sculptural curling arms fill the lower half and run off the bottom tile edge. Two outer arms extend upward and hold a simple cream rounded rectangular process node on the left and a cream diamond decision node on the right; a clean thin right-angle connector links the two nodes behind the octopus. Make the diagram relationship immediately legible; only these two generous nodes, no tiny symbols, no letters.
Style: exquisite hand-engraved black linework, confident silhouette, selective crosshatching and carved contours, creamy highlights. Sophisticated editorial engraving with the clarity and presence of a premium desktop app icon. Noble and calm, not a cartoon, not sinister; no human clothing.
Palette: only near-black ink #0b0b0c, warm paper #efe9dc, and DiaCraft teal #007a8c (antialiasing permitted). Solid teal background. No gradients, 3D lighting, glow, bevel, texture in the background, or drop shadow.
Composition: square 1024x1024 image, a full-bleed rounded square tile, corner radius about 22% of the side, no outer border, no inner frame or circular medallion, no inset margin. Transparent corners outside the rounded square. Portrait fills the tile confidently but eye/head and the two diagram shapes stay within the safe area. Crisp bold contours and well-separated light areas survive small sizes. The body is deliberately cropped by the tile bottom. No text, no watermark, no copied logos.
```

### Final opaque master prompt

The final pass used the previous original DiaCraft draft as its reference.

```text
Create an OPAQUE SQUARE MASTER variant of the supplied original DiaCraft illustration. Keep exactly the same octopus, pose, shapes, engraved detailing and composition. The only change is the background: solid flat teal #007a8c covering EVERY background pixel out to ALL FOUR SQUARE corners, with no corner rounding. This is a full rectangular print, not a cutout. Remove the black smoky area above the octopus and all vignettes or discoloration from the teal field; that area must be flat teal. No transparency anywhere. Only three inks: black #0b0b0c lines, cream #efe9dc figure and shapes, teal #007a8c background. No gradient, texture, lighting, shadows or holes. Preserve original engraved illustration. Square canvas, 1024x1024.
```

### Cover prompt

Reference: the retained opaque DiaCraft icon master.

```text
Use case: ads-marketing. Create a polished professional software cover graphic for DiaCraft in a wide 16:9 landscape composition, ideally 2560x1440 or the highest available resolution.
Reference image role: this is DiaCraft's new original app illustration. Reuse the SAME engraved octopus identity, its alert eyes, head proportions, elegant arm curves, and the process rectangle plus decision diamond it holds. Extend and recompose the illustration for a cover, not a screenshot or product mockup.
Art direction: premium editorial software identity, warm ivory #efe9dc paper background, near-black #0b0b0c engraved drawing and typography, rich teal #007a8c accents. Flat three-ink aesthetic, crisp modern layout paired with exquisite antique engraving, calm and confident, lots of intentional negative space.
Layout: left 47% is a generous clean text column with left-aligned typography. Right 53% is a dramatic oversized engraved octopus portrait with elegant arms, cropped confidently by the right and bottom edges, behind and around it a few clean teal orthogonal connector lines and empty rounded rectangles and decision diamonds. Keep the diagram vocabulary deliberate, sparse and structurally coherent. The right background remains warm ivory; use teal fills inside selected diagram nodes and generous teal negative space between forms, no framed square icon or circular badge.
Exact text, no other text: main wordmark "DiaCraft" in large refined bold modern sans-serif, black, correct upper/lower case, one line in the left column. Beneath it a smaller refined sans-serif label "Diagramming software" in teal. Make the main title imposing and optically balanced, roughly 15% of image height. Under the subtitle leave breathing space and add a small elegant three-node schematic in teal (rounded rectangle to diamond to rounded rectangle) as a quiet baseline motif.
No UI screenshots, no platform/vendor logos, no website address, no fake text, no badges, no gradients, no 3D effects, no paper distress, no decorative borders. The final image should feel like carefully art-directed professional open-source software branding.
```

## Verification

Both selected images were visually reviewed for composition, matching creature identity, diagram shapes and cover spelling. PNG dimensions and colour mode were read from their file headers. The File Home cover in light and dark themes, and Home scrolling plus creating a new drawing at the 480 × 320 minimum, were verified. The icon work was built and run on KDE/Wayland: a screenshot including the OS decoration confirms the octopus in the OS title bar and no logo in the in-app top bar. Its local evidence is `plan/brand3/os-window.png`; earlier Home captures are in `plan/brand2/`. Both folders are ignored. Other desktop operating systems remain unverified.

The bundled-image test checks decoding, truncated input and incorrect dimensions. Home tests preserve the cover's aspect ratio and usable drawing action at narrow widths. The in-app top bar paints no application image; the cover has its own cached texture. The web entry-point tests account for Trunk's additional local favicon asset.

`cargo xtask ci` passed all 11 gates again after a change on 2026-11-11: formatting, native Clippy, 6,248 passing tests (43 skipped), doctests, asset attribution, brand names, documentation, dependency licences, parity summary, layers and WASM Clippy for 19 crates. `desktop-file-validate` passed for the portable and installed desktop entries. The native screenshots were visually inspected; browser rendering and other desktop operating systems were not exercised.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-24 | minor | APPID: the application id is `app.diacraft` (the desktop entry, the window's app id and the bundle id), and the page says that the registration of the earlier id is a different entry that is left alone |
| 2026-11-23 | trivial | Sentences reworded so that each reads whole on its own; nothing they say changes |
| 2026-11-23 | minor | The reference product is named only in the format lists ("the reference app" or VSDX elsewhere), and the page keeps to the words of the public tree |
| 2026-11-22 | minor | `diacraft-splash.png`, the cover at half its size in 256 colours, the background of the splash popup and About DiaCraft, and the command that makes it |
| 2026-11-11 | minor | OS application icon with Linux desktop registration; no duplicate in-app mark |
| 2026-11-11 | minor | Approved artwork integrated into desktop and web identity, File Home and README |
| 2026-11-11 | minor | Original engraved octopus icon and matching cover artwork |
