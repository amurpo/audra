Name:           audra
Version:        %{ver}
Release:        1%{?dist}
Summary:        Native music player for Linux with Last.fm scrobbling
License:        GPL-3.0-or-later

Requires:       gtk4
Requires:       libadwaita

%description
Audra is a native music player for Linux (GTK4/libadwaita)
with Last.fm integration and automatic scrobbling.

%install
install -Dm755 %{_sourcedir}/audra          %{buildroot}%{_bindir}/audra
install -Dm644 %{_sourcedir}/com.audra.player.desktop \
               %{buildroot}%{_datadir}/applications/com.audra.player.desktop
install -Dm644 %{_sourcedir}/com.audra.player.svg \
               %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.audra.player.svg

%files
%{_bindir}/audra
%{_datadir}/applications/com.audra.player.desktop
%{_datadir}/icons/hicolor/scalable/apps/com.audra.player.svg

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
* Thu Jan 01 2026 Daniel Avila <daigo.tnt@gmail.com> - %{ver}-1
- Initial release
