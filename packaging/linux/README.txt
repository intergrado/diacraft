DiaCraft @NAME@ for Linux (64-bit)

DiaCraft is a free, open-source diagram editor. It opens and saves .diacraft, .vsdx and .odg drawings.
This is an early release: expect rough edges, and keep copies of drawings that matter to you.
Project page, documentation and bug reports: @REPO_URL@

What is in this folder
  diacraft                      the drawing application
  diacraft-cli                  the command line: convert and export drawings, run commands, serve MCP
  install.sh                    installs both for your user (see below)
  app.diacraft.desktop the launcher entry that install.sh puts in your application menu
  app.diacraft.png     the icon
  LICENSE-MIT, LICENSE-APACHE   the two licences DiaCraft is offered under; you may choose either
  NOTICE, ATTRIBUTION.md        credits, and where the pictures and fonts come from
  THIRD-PARTY-LICENSES.txt      the licence and notice texts of the libraries DiaCraft is built from (File, About,
                                Third-party licences shows the same text in the window)

Run it without installing
  ./diacraft              opens the window
  ./diacraft some.vsdx    opens a drawing
  ./diacraft-cli --help   lists what the command line does

Install it
  ./install.sh            copies the programs to ~/.local/bin, the entry and the icon to ~/.local/share
  ./install.sh --uninstall

What it needs
  A 64-bit Linux with glibc 2.35 or newer (Ubuntu 22.04 and Debian 12 are new enough), and a desktop session
  (Wayland or X11) with a Vulkan or OpenGL graphics driver. The window is drawn on the GPU through wgpu. The
  program loads the window system's libraries (libxkbcommon, libwayland-client or libX11) when it starts, so
  nothing is linked that a desktop does not already have; diacraft-cli needs no more than the C library.

If the window does not open, run ./diacraft in a terminal to see why, and attach the log file to a bug report:
  ~/.config/diacraft/logs/diacraft.log

Check your download against SHA256SUMS on the release page:
  sha256sum --check --ignore-missing SHA256SUMS
