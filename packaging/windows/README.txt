DiaCraft @NAME@ for Windows (@ARCH@: 64-bit @CPU@)

DiaCraft is a free, open-source diagram editor. It opens and saves .diacraft, .vsdx and .odg drawings.
This is an early release: expect rough edges, and keep copies of drawings that matter to you.
Project page, documentation and bug reports: @REPO_URL@

What is in this folder
  diacraft.exe                  the drawing application
  diacraft-cli.exe              the command line: convert and export drawings, run commands, serve MCP
  LICENSE-MIT, LICENSE-APACHE   the two licences DiaCraft is offered under; you may choose either
  NOTICE, ATTRIBUTION.md        credits, and where the pictures and fonts come from
  THIRD-PARTY-LICENSES.txt      the licence and notice texts of the libraries DiaCraft is built from (File, About,
                                Third-party licences shows the same text in the window)

This zip needs no installer. Unpack the whole zip to a folder of your own (right-click the zip, Extract All) and
double-click diacraft.exe. To open a drawing, drag the file onto diacraft.exe, or choose File, Open in the window.

There is an installer too, diacraft-@NAME@-windows-@ARCH@.msi, on the release page. It installs DiaCraft for you alone
(no administrator rights) into %LocalAppData%\Programs\DiaCraft, with a Start menu shortcut and diacraft-cli on your
PATH, and lists DiaCraft in Open with for .diacraft, .vsdx and .odg files, without making it the program they open
with. A later installer replaces it, and Settings, Apps, Installed apps removes it.

This download is not signed yet. The first time you start it, Windows may say "Windows protected your PC"
(SmartScreen). Choose "More info", then "Run anyway". If you want to be sure the file is the one that was
released, check it against SHA256SUMS on the release page. In PowerShell:
  Get-FileHash .\diacraft-@NAME@-windows-@ARCH@.zip -Algorithm SHA256

The command line
  Open a terminal in the folder and run  .\diacraft-cli.exe --help
  diacraft.exe is a window program: it prints nothing in a terminal, so use diacraft-cli.exe --version to see the version.

What it needs
  64-bit Windows 10 or 11 on an @CPU@ processor, with a graphics driver that supports DirectX 12, Vulkan or OpenGL.

If the window does not open, attach the log file to a bug report:
  %APPDATA%\diacraft\logs\diacraft.log
