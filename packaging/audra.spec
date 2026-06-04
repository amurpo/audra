Name:           audra
Version:        %{ver}
Release:        1%{?dist}
Summary:        Native music player for Linux with Last.fm scrobbling
License:        GPL-3.0-or-later
URL:            https://github.com/amurpo/audra

BuildRequires:  gettext

Requires:       gtk4
Requires:       libadwaita
%description
Audra is a native music player for Linux (GTK4/libadwaita)
with Last.fm integration and automatic scrobbling.

%install
install -Dm755 %{_sourcedir}/audra          %{buildroot}%{_bindir}/audra
install -Dm644 %{_sourcedir}/io.github.amurpo.audra.desktop \
               %{buildroot}%{_datadir}/applications/io.github.amurpo.audra.desktop
install -Dm644 %{_sourcedir}/io.github.amurpo.audra.svg \
               %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.amurpo.audra.svg
install -Dm644 %{_sourcedir}/io.github.amurpo.audra.metainfo.xml \
               %{buildroot}%{_datadir}/metainfo/io.github.amurpo.audra.metainfo.xml
mkdir -p %{buildroot}%{_datadir}/locale/es/LC_MESSAGES
msgfmt %{_sourcedir}/es.po \
       -o %{buildroot}%{_datadir}/locale/es/LC_MESSAGES/audra.mo

%files
%{_bindir}/audra
%{_datadir}/applications/io.github.amurpo.audra.desktop
%{_datadir}/metainfo/io.github.amurpo.audra.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.amurpo.audra.svg
%{_datadir}/locale/es/LC_MESSAGES/audra.mo

%post
/usr/bin/gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || :
/usr/bin/update-desktop-database >/dev/null 2>&1 || :

%postun
/usr/bin/gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || :
/usr/bin/update-desktop-database >/dev/null 2>&1 || :

%transfiletriggerin -- /usr/share/icons/hicolor
/usr/bin/gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || :

%transfiletriggerpostun -- /usr/share/icons/hicolor
/usr/bin/gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || :

%transfiletriggerin -- /usr/share/applications
/usr/bin/update-desktop-database >/dev/null 2>&1 || :

%transfiletriggerpostun -- /usr/share/applications
/usr/bin/update-desktop-database >/dev/null 2>&1 || :

%changelog
* Thu Jan 01 2026 Daniel Avila - %{ver}-1
- Initial release
