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

There is an installer too, diacraft-@NAME@-windows-@FILEARCH@.msi, on the release page. It installs DiaCraft for you alone
(no administrator rights) into %LocalAppData%\Programs\DiaCraft, with a Start menu shortcut and diacraft-cli on your
PATH, and lists DiaCraft in Open with for .diacraft, .vsdx and .odg files, without making it the program they open
with. A later installer replaces it, and Settings, Apps, Installed apps removes it.

This download is not signed yet. The first time you start it, Windows may say "Windows protected your PC"
(SmartScreen). Choose "More info", then "Run anyway". If you want to be sure the file is the one that was
released, check it against SHA256SUMS.txt on the release page. In PowerShell:
  Get-FileHash .\diacraft-@NAME@-windows-@FILEARCH@-portable.zip -Algorithm SHA256

The command line
  Open a terminal in the folder and run  .\diacraft-cli.exe --help
  diacraft.exe is a window program: it prints nothing in a terminal, so use diacraft-cli.exe --version to see the version.
  For tab completion in PowerShell, put this line in your profile (notepad $PROFILE):
    diacraft-cli completions powershell | Out-String | Invoke-Expression
  (the installer puts diacraft-cli on your PATH; from the unpacked zip, give the whole path to diacraft-cli.exe). It also writes the scripts of bash, zsh, fish and elvish.

What it needs
  64-bit Windows 10 or 11 on an @CPU@ processor, with a graphics driver that supports DirectX 12, Vulkan or OpenGL.

If the window does not open
  DiaCraft draws its window on your graphics card, and tries what your computer has in turn: its usual choice, then DirectX 12 alone, then the software mode (the
  processor draws, which is slow and works with no graphics driver, as in a virtual machine or a Remote Desktop session), then OpenGL alone. It opens on the first that
  works, and the status bar says when that was not the usual one. If none works, it shows a message box, "DiaCraft could not start", that says why, where the log is
  and what to try. If you see neither a window nor a box, the program was stopped before it ran (see SmartScreen above, or a security program).
  To try by hand, open a Command Prompt in DiaCraft's folder and run
      set WGPU_BACKEND=dx12
      diacraft.exe
  and, if that fails too, the same with gl in place of dx12 (vulkan is the third kind). With WGPU_BACKEND set DiaCraft tries only that kind of graphics, and the box says so
  if it cannot start; remove the setting (set WGPU_BACKEND=) to let DiaCraft choose again. Updating the graphics driver, from the maker of the graphics card or of the
  computer, mends most of it. A script that starts diacraft.exe sets DIACRAFT_NO_ERROR_BOX=1 to have no box: the exit code (1) and the log say it.
  If a graphics driver crashes or hangs while DiaCraft tests it, the next start leaves that graphics adapter out and says so in the log ("the last start ended while testing ...").
  To test it again, run diacraft.exe --reset-graphics in a Command Prompt.
  Attach the log file to a bug report:
  %APPDATA%\diacraft\logs\diacraft.log
