# The spec of DiaCraft's .rpm for Fedora. packaging/linux/make-rpm.sh builds the package from it, with `rpmbuild -bb --build-in-place`, and gives it the macros below
# (--define): `diacraft_version` (what rpm calls the version: 0.2.4, or 0.2.4~rc1 for a pre-release), `diacraft_url`, and `diacraft_extras` (1 when the shell completions and the
# manual pages were written into the tree, which only a build of the command line that has them can do). It appends the changelog entry after the last line of this file.
#
# The package installs the two programs that were built, as they are: nothing is compiled here, so there is no %%build, and no debuginfo package (the script strips the programs).
# The build directory it runs in holds the licence files, and `files/`, the tree that goes under `/`.

%if "%{?diacraft_version}" == ""
%{error:diacraft_version is not defined: run packaging/linux/make-rpm.sh}
%endif
%{!?diacraft_url:%global diacraft_url https://diacraft.dev}

%global debug_package %{nil}
# The package owns the files it lists and no more: no links under /usr/lib/.build-id.
%global _build_id_links none

Name:           diacraft
Version:        %{diacraft_version}
Release:        1
Summary:        Diagram editor
License:        MIT OR Apache-2.0
URL:            %{diacraft_url}
ExclusiveArch:  x86_64 aarch64
BuildRequires:  desktop-file-utils

# rpm reads what the programs are linked with and gives the package those Requires itself: the C library (with the version of each symbol the build needs: glibc 2.35 or newer),
# the math library and libgcc_s. The programs load the rest by name when they start, which a reader of the link line does not see, so those are named here, by the sonames
# the programs load. A soname with no version is the Requires rpm writes for a library, and Fedora's package that holds the library answers it, on every Fedora and for both
# CPUs ((64bit) is how rpm says a 64-bit library, on x86_64 and aarch64 alike). packaging/linux/make-rpm.sh scans the programs and stops when one of them names a library that no line
# below covers, so that a new dependency cannot leave the package behind.
#
# What the programs cannot start without: the keyboard library (both window systems use it), libxcb (`x11rb` loads it to talk to an X server), a window system (Wayland or X11)
# and a graphics API (Vulkan, or OpenGL through EGL).
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxcb.so.1()(64bit)
Requires:       (libwayland-client.so.0()(64bit) or libX11.so.6()(64bit))
Requires:       (libvulkan.so.1()(64bit) or libEGL.so.1()(64bit))
# The directory the icon goes in is the icon theme's, and the file trigger that refreshes the icon cache is too.
Requires:       hicolor-icon-theme
# What completes them: the rest of the window libraries, the graphics drivers, and the desktop's portal that the Open and Save dialogs use.
Recommends:     libxkbcommon-x11.so.0()(64bit)
Recommends:     libX11-xcb.so.1()(64bit)
Recommends:     libXcursor.so.1()(64bit)
Recommends:     libXi.so.6()(64bit)
Recommends:     libwayland-egl.so.1()(64bit)
Recommends:     libdbus-1.so.3()(64bit)
Recommends:     mesa-vulkan-drivers
Recommends:     mesa-dri-drivers
Recommends:     xdg-desktop-portal

%description
DiaCraft is a free, open-source diagram editor. It opens and saves drawings in
the .diacraft, .vsdx and .odg formats, and draws them as PNG, SVG and PDF.

The package holds the drawing application, diacraft, and the command line,
diacraft-cli, which converts and exports drawings, runs commands and serves
MCP.

%prep
# Nothing to unpack: the build directory is the one the script made.

%build
# Nothing to build: the programs were built before.

%install
cp -a files/. %{buildroot}/

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/app.diacraft.desktop

# Fedora's own tools refresh the desktop database and the icon cache when files arrive in the folders they watch (file triggers of desktop-file-utils and hicolor-icon-theme), so
# the package has no %%post or %%postun.

%files
%license LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt
%{_bindir}/diacraft
%{_bindir}/diacraft-cli
%{_datadir}/applications/app.diacraft.desktop
%{_datadir}/icons/hicolor/256x256/apps/app.diacraft.png
%if 0%{?diacraft_extras}
%{_datadir}/bash-completion/completions/diacraft
%{_datadir}/bash-completion/completions/diacraft-cli
%{_datadir}/zsh/site-functions/_diacraft
%{_datadir}/zsh/site-functions/_diacraft-cli
%{_datadir}/fish/vendor_completions.d/diacraft.fish
%{_datadir}/fish/vendor_completions.d/diacraft-cli.fish
%{_mandir}/man1/diacraft*.1*
%endif

%changelog
