# Releasing DiaCraft

The first release is 0.2 (`docs/roadmap.md`). A pushed tag `vMAJOR.MINOR.PATCH` runs `.github/workflows/release.yml`, which builds DiaCraft in release for **Linux, Windows and macOS** and uploads three downloads and a `SHA256SUMS` file to a **draft** GitHub release of the tag. The owner reads the notes and the files, then presses Publish. Nothing here has run on GitHub yet: the workflows were written and checked on Linux (see "What was checked, and what was not"). The reference app was not inspected, and the downloads have not been opened in it by the project.

**The workflow is DiaCraft's own, and the one exception to the rule that workflows call the shared ones** (`AGENTS.md`, "CI workflows"): the shared `release.yml` needs org signing secrets and runners. It uses no action but GitHub's own `actions/checkout` and, in the `site` job, `actions/upload-artifact` (both pinned by commit); the GitHub CLI (`gh`) that every runner has uploads the files.

## The downloads

| Platform | File | Built on | Holds |
|---|---|---|---|
| Linux, x86_64 | `diacraft-<tag>-linux-x86_64.tar.gz` | `ubuntu-22.04`, target `x86_64-unknown-linux-gnu` | A folder with `diacraft`, `diacraft-cli`, `install.sh`, the launcher entry (`app.diacraft.desktop`), the icon, the five licence files (below) and `README.txt`. Needs glibc 2.35 or newer, which the oldest image GitHub still offers gives: Ubuntu 22.04 and Debian 12 run it. |
| Windows, x86_64 | `diacraft-<tag>-windows-x86_64.zip` | `windows-2025`, target `x86_64-pc-windows-msvc` | A folder with `diacraft.exe`, `diacraft-cli.exe`, the five licence files and `README.txt`. No installer. The C runtime is linked in, so no Visual C++ runtime is needed, and the main thread has an 8 MiB stack (`.cargo/config.toml`). |
| macOS, universal | `diacraft-<tag>-macos-universal.zip` | `macos-15` (Apple silicon), targets `aarch64-apple-darwin` and `x86_64-apple-darwin`, joined with `lipo` | A folder with `DiaCraft.app` (the universal program, `Contents/MacOS/DiaCraft`, named as the app because the app menu names its items after it, `Info.plist`, and `DiaCraft.icns`, made from the icon master at build time), `diacraft-cli` (universal), the five licence files (also inside the app, in `Contents/Resources`, so that they stay with it when the app is dragged to Applications alone) and `README.txt`. Built for macOS 11 or newer (`MACOSX_DEPLOYMENT_TARGET=11.0`). |
| All | `SHA256SUMS` | the last job | `sha256sum` lines for the three files above. |

`<tag>` is the tag, `v0.2.0`. The site's three asset patterns in `site/site.json` (`diacraft-{tag}-linux-x86_64.tar.gz` and the other two) are these names, and `cargo xtask site --release` refuses an empty one. `packaging/package.sh` makes all three archives; the release workflow and the Windows and macOS jobs of `ci.yml` run the same script, so the packaging a release would do is done on every push too.

**The licence files.** Every download holds these five files, and `packaging/package.sh` stops when one is missing or empty: `LICENSE-MIT` and `LICENSE-APACHE` (the two licences DiaCraft is offered under), `NOTICE` (the credits and the notices of the two fonts and of the mimalloc C library), `ATTRIBUTION.md` (where each picture comes from) and `THIRD-PARTY-LICENSES.txt`. The last is the notice that the licences of the Rust crates ask for: it lists every crate `diacraft` and `diacraft-cli` are built from on Linux, Windows and macOS (Apple silicon and Intel), with its licence and the licence DiaCraft uses it under (MIT where a crate offers a choice), and gives each licence and NOTICE text once, with the crates that carry it. The Linux `install.sh` puts the five under `share/doc/diacraft`. The window carries the same text built in: File, About, Third-party licences (`app.licenses`) shows it in a list that scrolls, so it is there wherever the program is.

`THIRD-PARTY-LICENSES.txt` is made by `cargo xtask licenses` and kept in the repository, so that a download, the window and the site's link (`site/index.html`, to the file on GitHub) all say the same. **When a dependency changes** (`Cargo.lock`, a version, a feature or a new crate), run `cargo xtask licenses` and commit the file: `cargo xtask licenses --check` is a step of `cargo xtask ci`, and the Linux job of the release workflow runs it before it builds, so a tag whose file is stale or whose dependency has no licence text stops there. It fails for a crate with no licence text, for a licence (or a licence file inside a crate) that `deny.toml` does not allow, for a crate that compiles C code that `xtask/src/licenses/policy.rs` does not name, and when the file is not what the lock says. Two things it cannot know: the C and other native code of a crate that has no build dependency on `cc` or `cmake`, and a licence text that a crate's sources do not carry. It needs the sources of every platform's crates; cargo fetches them (with a network) the first time. The number of crates each program is built from on each platform is in the first lines of the file. The programs link no TLS stack and no `ring` or `aws-lc` (the print library's Windows build script does, on the build machine, and that is in no program); the one C library they compile in is mimalloc, whose crate carries its licence text. **Open question for the owner (`docs/gaps.md`):** the Linux build includes `wayland-protocols-plasma`, an MIT crate whose protocol description files come from KDE and say LGPL-2.1-or-later; the file lists the LGPL text and says so, and `AGENTS.md` says no LGPL code is depended on.

**The version.** `--version` of both programs prints the version in `[workspace.package]` of `Cargo.toml`, so the workflow stops at once when the tag and that version differ (`v0.2.0` needs `version = "0.2.0"`). The build's own identity, the build line that the splash, the About panel and `app.version` show under the version, is the tag's version: the workflow sets `DIACRAFT_BUILD_GIT` to the tag and `DIACRAFT_BUILD_YEAR` to the year of the tagged commit (`apps/diacraft/build_info.rs`; the clock is never read), and a build of the tag `v0.2.0` says plain `0.2.0`. Before the tag exists a build says `0.2.0-dev+<commit>` (and `0.2.0+12.<commit>` twelve commits after it), whatever the last tag is called, so a build made for the release does not read as the one before. `packaging/smoke.sh` checks the versions and the line in the built programs and again in the unpacked download. The macOS app's `Info.plist` says the tag's version, and the workspace's for a name that holds none (a rehearsal's `dev-<commit>`; `packaging/package.sh` reads `[workspace.package]`).

## Cut a release (the owner's steps)

1. **Settle the site** (`docs/website.md`). Set `domain` in `site/site.json` to the public one (`repository` is `intergrado/diacraft` already), settle the release notes in `site/changelog.html`, and replace "0.2 is in development" in `site/index.html`. Run `cargo xtask site --write`, then `cargo xtask site --release`, and keep the generated changes in the commit.
2. **Set the version.** Run `cargo xtask version set 0.2.0`: it sets `version` in `[workspace.package]` of `Cargo.toml` to the release and refreshes `Cargo.lock` (the release builds with `--locked`); commit both. It is 0.2.0 already, and every place that says the version reads it from there.
3. **Pass the full gate** on that commit (`cargo xtask ci`, which includes `cargo xtask licenses --check`: run `cargo xtask licenses` first if a dependency changed) and push `main`. The `windows` and `macos` jobs of `ci.yml` build and smoke test both platforms on that push: do not tag a commit they have not passed.
4. **Tag and push the tag:** `git tag -a v0.2.0 -m "DiaCraft 0.2.0"`, then `git push origin v0.2.0`. A tag with a suffix (`v0.2.0-rc1`) makes a pre-release.
5. **Watch the Release workflow** (the Actions tab). `Plan` checks the tag against `Cargo.toml`; `Draft release` makes the draft; `Linux`, `Windows` and `macOS` build, smoke test, package, smoke test the unpacked download and upload it; `SHA256SUMS` checks that exactly the three files are there, writes the sums and uploads them. The run's summary lists the sums.
6. **Publish.** Open the draft release, write the notes (the draft holds a short text with the three file names), look at the files, and press Publish. The site's download buttons point at the files themselves once the release is the latest (`site/download.js` reads it), and at the release page till then.

**A rehearsal costs nothing.** Run the workflow by hand on a branch (Actions, Release, Run workflow): it builds, smoke tests and packages all three, uploads nothing, and names them `dev-<commit>`. Or push a pre-release tag and delete the draft and the tag afterwards (the tag must match `Cargo.toml`'s version, so a pre-release tag needs `version = "0.2.0-rc1"`).

**A failed run.** `Re-run failed jobs` resumes where it stopped: an upload uses `--clobber`, and `Draft release` finds the draft it made. To start over, delete the draft and the tag and tag again.

## What the owner fills in

| What | Where | When |
|---|---|---|
| The site's domain (the public repository's name, `intergrado/diacraft`, is set) | `site/site.json`, then `cargo xtask site --write` | before the first release |
| The release notes | the draft release, and `site/changelog.html` | before Publish |
| Which workflows the public repository keeps | `.github/workflows/` | settled: both trees have the same two files. `ci.yml` runs the gate itself with no third-party action but checkout and the cargo cache, and keeps the `windows` and `macos` jobs; `release.yml` stands alone. When the public repository goes public, the gate job may call the shared gate workflow of the craft apps again (`AGENTS.md`, "CI workflows") |
| The Cloudflare Pages deploy: the project's name and two secrets, `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` | the comment at the end of the `site` job of `release.yml`, and `docs/website.md`, "Deploy to Cloudflare Pages" | when the site goes live: the job already builds the pages and the app into an artifact, and deploys nothing until the step is switched on |
| Code-signing secrets | repository settings, Secrets and variables, Actions | when there are certificates: see "Signing". Not needed for 0.2 |
| Who may push a `v*` tag | repository settings, Rules | optional: a tag ruleset limited to the owner |

Repository settings need no change for the workflow: it asks for `contents: write` in the jobs that upload and `contents: read` elsewhere.

## Opening an unsigned download

0.2 is **unsigned**: the programs carry no certificate of a publisher. Each download's `README.txt` says what follows, and the site's download cards say a confirmation may be asked. Checking the file against `SHA256SUMS` is how to know it is the one that was released (`sha256sum --check --ignore-missing SHA256SUMS` on Linux, `shasum -a 256 FILE` on macOS, `Get-FileHash FILE` in PowerShell).

- **Linux.** Nothing to confirm. Unpack and run `./diacraft`, or run `./install.sh` to add it to the application menu for the user (it needs no administrator rights, and `./install.sh --uninstall` removes it).
- **Windows.** SmartScreen may say "Windows protected your PC" for a program it does not know. Choose "More info", then "Run anyway". Unblocking the zip first (right-click, Properties, Unblock) avoids the question for everything in it.
- **macOS.** The first opening is refused with a message that Apple cannot check the app. Either open System Settings, Privacy & Security, scroll to the line about DiaCraft and choose "Open Anyway" (macOS 15 and later; on macOS 14 and earlier, Control-click the app, choose Open, then Open); or remove the quarantine mark in Terminal: `xattr -dr com.apple.quarantine /Applications/DiaCraft.app`. The same goes for `diacraft-cli`. Both programs carry an ad hoc signature, which an Apple silicon Mac needs to run a program at all; it says nothing about who made them.

## Signing

The hooks are in place and switched off. Each is a commented block in `release.yml` that names the secrets it reads.

- **macOS** (Developer ID, Apple Developer Program). Secrets: `MACOS_CERTIFICATE` (the "Developer ID Application" certificate and key as a `.p12`, in base64), `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY` (`Developer ID Application: Name (TEAMID)`), `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD` (an app-specific password). Uncomment the step "Import the signing certificate" and the step "Notarise the download" of the `macos` job, and add `MACOS_SIGN_IDENTITY` to the environment of "Make the download": `packaging/package.sh` then signs the CLI, the app's program and the app with the hardened runtime and a timestamp, where without it they are signed ad hoc. Stapling the ticket into the app (so it opens offline) needs the app on its own, so it is a second archive made after notarisation, and the hook leaves it out: the Mac checks the ticket online.
- **Windows** (Authenticode). Secrets: `WINDOWS_CERTIFICATE` (a `.pfx`, in base64) and `WINDOWS_CERTIFICATE_PASSWORD`. Uncomment the step "Sign the programs" of the `windows` job, which runs `signtool` on both programs before they are smoke tested and packaged. SmartScreen's reputation comes at once with an EV certificate or a cloud signing service, and over time with an ordinary one.
- **Linux.** Nothing is needed. A detached GPG signature of `SHA256SUMS` can be added to the `SHA256SUMS` job.

## Not done, and what it would take

- **An AppImage** for Linux. The tarball runs wherever the AppImage would. An AppImage needs an `AppDir` (the program, the desktop file, the icon, an `AppRun`) and `appimagetool`, a third-party binary the workflow would download and check by checksum. The program loads the window system, Vulkan and OpenGL libraries from the host, so nothing else is bundled; the AppImage runtime itself needs FUSE 2 on the host (or `--appimage-extract-and-run`). About a day; it goes in the `linux` job beside the tarball.
- **A Windows installer (MSI).** WiX Toolset (v4 or v5, `dotnet tool install --global wix`), a `.wxs` file (product, per-user install folder, Start menu shortcut, optional `.diacraft`, `.vsdx` and `.odg` associations), and signing for the installer too. About a day; it goes in the `windows` job.
- **A `.dmg` for macOS.** `hdiutil create` from the folder, with a link to `/Applications`, signed and notarised as well. Small once signing exists.
- **An icon on `diacraft.exe`.** The window's icon is set at run time, so the taskbar and the title bar show it, but the file in Explorer has the default icon. Embedding one needs a resource compiler in the build (a `build.rs` with `winresource` or `embed-resource`, a new dependency) and an `.ico` made from the icon master.
- **File associations.** The macOS app does not claim `.diacraft`, `.vsdx` or `.odg` (opening a file by double-click arrives as an Apple event, which the window layer does not handle yet), the Windows zip has no installer to register them, and the Linux entry names no MIME types. Opening a file from the command line (`diacraft FILE`) works everywhere, and the window's File, Open does too.
- **`diacraft.exe` in a terminal.** It is built for the GUI subsystem so that no console window opens behind it, which means it prints nothing in a terminal: `--version` and `--help` are read with `diacraft-cli.exe`. A program cannot attach to its parent's console without `unsafe` code, which the project forbids.
- **Other targets:** Windows and Linux on ARM, FreeBSD. None is built.
- **The web build is no download.** It is the second site of the website (`diacraft.app`, with the pages on `diacraft.dev`), made by the `site` job: the pages (`diacraft-site-<name>`) and the app at the root of its own site (`diacraft-app-<name>`) in two artifacts, each of which the owner deploys to a Pages project of its own (`docs/website.md`, "The web app" and "Deploy to Cloudflare Pages"). Its notices are `THIRD-PARTY-LICENSES-WEB.txt`, made by `cargo xtask licenses` beside the downloads' file (the policy's `OUTPUTS` say which platforms each file covers; `diacraft-web` is the program of the `wasm32-unknown-unknown` target) and checked by `licenses --check`, which the Linux job runs. The app's licences window shows that list, and the app's page links the repository's copy; the file is not in the artifact, since its licence texts hold copyright lines that the site's own name check refuses. The `site` job has not run on GitHub: it was run on Linux with the tools it installs, by hand.

## What was checked, and what was not

On Linux (2026-11-22): both programs build in release; `packaging/smoke.sh` passes on them (the versions, the tag in `app.version`, a drawing saved as `.vsdx`, converted to `.diacraft` and back with the same content, drawn as PNG, SVG and PDF, and the MCP server answering); `packaging/package.sh` makes the Linux archive, and its `install.sh` installs and removes cleanly; the Windows and macOS archives were staged with the platform tools stood in for. For `x86_64-pc-windows-msvc` and `x86_64-apple-darwin`, `cargo check` passes for both programs from Linux **with the C allocator replaced by a stand-in**: mimalloc's C build needs the target's SDK, which the Linux machine does not have. So the C build of mimalloc, the link, the signing, the icon tools and everything that runs on those two systems are first tried by the CI jobs. **On the first run of the `windows` and `macos` jobs and of the release workflow, look at:**

- **The link on Windows** with `crt-static` and the 8 MiB stack (`.cargo/config.toml`), and mimalloc's C build there (it is compiled as C++ for MSVC).
- **`diacraft.exe --version`** in `smoke.sh`: a program for the GUI subsystem may print nothing in a terminal, and the script then warns and checks only the exit status.
- **The macOS tools in `package.sh`**: `sips` and `iconutil` making the `.icns`, `lipo` joining the two architectures, `codesign` (ad hoc), `ditto` writing the zip; and the step "Check what is in the download", which verifies the signature and both architectures.
- **The Intel half on the macOS runner:** a cross build from Apple silicon, run only for `--version` and only if Rosetta is there.
- **`gh release upload` on a draft** found by its tag (the jobs rely on `gh` finding a draft by tag, which it does by looking through the release list), and the `SHA256SUMS` job's check that exactly the three files are there.
- **Linux on `ubuntu-22.04`:** the apt package names and the C compiler there are older than on the gate's image.
- **Build times:** a cold release build of both programs, with no cache; the macOS job builds twice.

On a real Mac (2026-11-22: an Intel MacBook Pro, macOS 26.7.3, Rust 1.96.1, `MACOSX_DEPLOYMENT_TARGET=11.0`): both programs build in release, mimalloc's C build and the link included; `smoke.sh` passes; `package.sh macos` makes the Intel zip from one folder and the universal zip from two (`aarch64-apple-darwin` was added with `rustup target add` and cross-built on the Intel Mac), and the checks of the step "Check what is in the download" pass on it: both programs hold `arm64` and `x86_64` (`lipo -archs`), `plutil -lint` passes on `Info.plist`, the icon and the licence files are there, `codesign --verify --strict` passes on the ad hoc signatures, and the Intel half runs. The Apple silicon half was not run (an Intel Mac cannot). The packaged app opens its window and draws (after the fix: see `docs/hardware-parity.md`), and is signed ad hoc, so a person opening it from Finder is asked to confirm. Not checked on a Mac: the CI jobs themselves, a cold build's time on GitHub's runners, notarisation. A second run (2026-11-22) built both halves again in 10 min 20 s on that Mac and made the universal zip with the program in the app named `DiaCraft` (`CFBundleExecutable`); `smoke.sh` and the step's checks, copied from `release.yml`, passed on it, and the app opened from it runs as the process `DiaCraft`.

## Building by hand

```
cargo build --release --locked -p diacraft -p diacraft-cli                                   # the machine's own target: the programs are in target/release
cargo build --release --locked --target x86_64-pc-windows-msvc -p diacraft -p diacraft-cli   # on Windows: --target applies the flags of .cargo/config.toml
bash packaging/smoke.sh target/release                                                       # runs both programs once
bash packaging/package.sh linux v0.2.0 dist target/release                                   # or windows, or macos (one folder for each architecture)
```

`DIACRAFT_BUILD_GIT` and `DIACRAFT_BUILD_YEAR` set the build's own identity for a tree that has no `.git`, such as the public export; without them git describes the commit.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-24 | minor | APPID: the launcher entry of the Linux download is `app.diacraft.desktop` |
| 2026-11-24 | minor | The public repository is `intergrado/diacraft`, at the organisation `intergrado`: the first step of a release, the owner's table and the publish section name it; the owner's own account still makes the commits and pushes (an admin of the organisation), to `git@github-intergrado:intergrado/diacraft.git`, and a clone whose remote is any other address, the old ones included, is refused |
| 2026-11-24 | minor | The web build is the second site of the website, kept as a second artifact (`diacraft-app-<name>`) beside the pages' (`diacraft-site-<name>`) |
| 2026-11-23 | trivial | The publish section says how a sentence is written so that the public tree reads it whole |
| 2026-11-23 | minor | The site's repository setting is the public repository's name, and the owner's table lists only the site's domain; the publish section says whose identity its commits carry and what a clone's remotes must be |
| 2026-11-23 | minor | The publish section says what the strip of the private process's citations takes and what is rewritten here instead, that `--check-only` is a step of `ci`, and that an allowlist entry is the line as the strip writes it |
| 2026-11-23 | minor | The version lines: the workspace version is 0.2.0, the step that sets it is `cargo xtask version set`, and the build line is the plain version for a build of the tag and `<version>-dev+<commit>` before it |
| 2026-11-23 | minor | The public repository keeps the same two workflows; the procedure that publishes a release to it (`cargo xtask export-public --check-only`, `--suggest-allowlist`, `--publish-to`, the public tree's own gate, the push and the tag) is a section that the export leaves out |
| 2026-11-22 | minor | The program in the app is named DiaCraft; the universal zip made and checked again on the Mac |
| 2026-11-22 | minor | What was built, packaged and checked on a real Intel Mac, the universal zip included |
| 2026-11-22 | minor | The `site` job and its artifact, the deploy hook and what the owner fills in for it, and the web build's notices as `THIRD-PARTY-LICENSES-WEB.txt` (they were not covered) |
| 2026-11-22 | minor | The five licence files in every download, `THIRD-PARTY-LICENSES.txt` and how it is made and kept current (`cargo xtask licenses`, `--check` in `ci` and in the release workflow), the About window that shows it, and the web build's notices as not done |
| 2026-11-22 | major | The release workflow, the three downloads for Linux, Windows and macOS, how to cut a release, how to open an unsigned download, the signing hooks, what is not done and what the first run of the jobs should be watched for |
| 2026-11-21 | minor | The page names 0.2 as the first release (it said the workflow comes in M9), says only the gate workflow exists, and names `cargo xtask export-public` |
| 2026-10-31 | major | Created for M0 |
