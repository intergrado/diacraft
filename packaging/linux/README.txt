DiaCraft @NAME@ for Linux (@ARCH@: 64-bit @CPU@)

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

Shell completions
  diacraft-cli writes the completion script of your shell (bash, zsh, fish, powershell or elvish) to standard output:
    ./diacraft-cli completions bash > ~/.local/share/bash-completion/completions/diacraft-cli
    ./diacraft-cli completions zsh > ~/.zfunc/_diacraft-cli        (put ~/.zfunc on your fpath, before compinit, in ~/.zshrc)
    ./diacraft-cli completions fish > ~/.config/fish/completions/diacraft-cli.fish
  Add --program diacraft for the completions of the window program. Manual pages: ./diacraft-cli man | man -l -
  The .deb and the .rpm on the release page install both for every user; the AppImage writes them too (./file.AppImage --cli completions bash).

Other forms of this download, on the same release page: a .deb for Debian and Ubuntu (sudo apt install ./the-file.deb), an .rpm for Fedora (sudo dnf install ./the-file.rpm) and an AppImage (make it executable and run it).

Install it
  ./install.sh            copies the programs to ~/.local/bin, the entry and the icon to ~/.local/share
  ./install.sh --uninstall

What it needs
  A 64-bit @CPU@ Linux with glibc @GLIBC@ or newer (@DISTROS@ are new enough), and a desktop session
  (Wayland or X11) with a Vulkan or OpenGL graphics driver. The window is drawn on the GPU through wgpu. The
  program loads the window system's libraries (libxkbcommon, libwayland-client or libX11) when it starts, so
  nothing is linked that a desktop does not already have; diacraft-cli needs no more than the C library.

If the window does not open, run ./diacraft in a terminal to see why. DiaCraft tries the graphics your computer has in turn (its usual choice, the software
renderer, OpenGL alone), and when none works it says why and where the log is: in the terminal, or, when it was started from a launcher, in a message box (zenity shows it).
To pick the graphics by hand, start it with WGPU_BACKEND=vulkan or WGPU_BACKEND=gl (the only kind it then tries). Attach the log file to a bug report:
  ~/.config/diacraft/logs/diacraft.log

Check your download against SHA256SUMS.txt on the release page:
  sha256sum --check --ignore-missing SHA256SUMS.txt
