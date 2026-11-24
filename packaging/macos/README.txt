DiaCraft @NAME@ for macOS (Apple silicon and Intel)

DiaCraft is a free, open-source diagram editor. It opens and saves .diacraft, .vsdx and .odg drawings.
This is an early release: expect rough edges, and keep copies of drawings that matter to you.
Project page, documentation and bug reports: @REPO_URL@

What is in this folder
  DiaCraft.app                  the drawing application (one app for Apple silicon and Intel Macs)
  diacraft-cli                  the command line: convert and export drawings, run commands, serve MCP
  LICENSE-MIT, LICENSE-APACHE   the two licences DiaCraft is offered under; you may choose either
  NOTICE, ATTRIBUTION.md        credits, and where the pictures and fonts come from
  THIRD-PARTY-LICENSES.txt      the licence and notice texts of the libraries DiaCraft is built from (File, About,
                                Third-party licences shows the same text in the window)

Install
  From the disk image (the .dmg): open it and drag DiaCraft.app onto the Applications link beside it. From the zip: drag
  DiaCraft.app to your Applications folder. To use the command line from any terminal, copy diacraft-cli to
  /usr/local/bin (sudo cp diacraft-cli /usr/local/bin/).

Opening an app that is not signed
  This download is not signed or notarised by Apple yet, so macOS asks you to confirm the first time you open it.
  Either of these works. Do it once for the app, and once more for diacraft-cli if you use it.

  1. Open DiaCraft.app. macOS says it cannot verify it. Choose Done (or OK), then open System Settings, Privacy &
     Security, scroll down to the line about DiaCraft and choose "Open Anyway", then confirm with your password.
     On macOS 14 and older, instead Control-click DiaCraft.app, choose Open, then choose Open in the box.

  2. In Terminal, remove the quarantine mark that your browser put on the download:
       xattr -dr com.apple.quarantine /Applications/DiaCraft.app
       xattr -d com.apple.quarantine /usr/local/bin/diacraft-cli

  To check the file you downloaded is the one that was released, compare it with SHA256SUMS on the release page:
       shasum -a 256 diacraft-@NAME@-macos-@ARCH@.dmg     (or the .zip)

What it needs
  macOS 11 or newer, on a Mac with an Apple silicon chip or an Intel chip.

The command line
  diacraft-cli --help lists what it does. diacraft-cli --version prints the version.

If the window does not open, attach the log file to a bug report:
  ~/Library/Application Support/diacraft/logs/diacraft.log
