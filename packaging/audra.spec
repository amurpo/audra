Name:           audra
Version:        %{ver}
Release:        1%{?dist}
Summary:        Reproductor de música nativo para Linux con scrobbling de Last.fm
License:        MIT

Requires:       gtk4
Requires:       libadwaita

%description
Audra es un reproductor de música nativo para Linux (GTK4/libadwaita)
con integración de Last.fm y scrobbling automático.

%install
install -Dm755 %{_builddir}/audra %{buildroot}%{_bindir}/audra

%files
%{_bindir}/audra

%changelog
* Thu Jan 01 2026 Daniel Avila <daigo.tnt@gmail.com> - %{ver}-1
- Versión inicial
