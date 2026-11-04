# PIDA Web – RPM aus dem vorbereiteten Verzeichnis (package.sh: --define "stage <Verzeichnis>",
# "pkgversion <Version>"). Das Programm ist statisch gebaut (musl): keine Abhängigkeiten.
%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none

Name:           pida-web
Version:        %{pkgversion}
Release:        1
Summary:        PIDA Web – PDF-Dokumente im Browser signieren (ID Austria)
License:        EUPL-1.2
URL:            https://github.com/a-sit/pida
ExclusiveArch:  x86_64
AutoReqProv:    no

%description
Server der Browser-Variante von PIDA: liefert die Oberfläche aus (PIDA-Kern und PDFium als WebAssembly) und
stellt den Rückkanal (DataURL) für das Signieren mit ID Austria über A-Trust bereit. Speichert nichts. Lauscht nur
lokal; von außen über einen Webserver mit HTTPS davor (Vorlage für Apache unter /usr/share/doc/pida-web).

%install
cp -a %{stage}/. %{buildroot}/

%post
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
    # Neuinstallation: aktivieren und starten (Update: %%postun startet neu)
    if [ "$1" -eq 1 ]; then
        systemctl enable --now pida-web.service >/dev/null 2>&1 || :
    fi
fi

%preun
# endgültiges Entfernen (nicht beim Update): stoppen und deaktivieren
if [ "$1" -eq 0 ] && [ -d /run/systemd/system ]; then
    systemctl disable --now pida-web.service >/dev/null 2>&1 || :
fi

%postun
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
    # Update: laufenden Dienst mit der neuen Version neu starten
    if [ "$1" -ge 1 ]; then
        systemctl try-restart pida-web.service >/dev/null 2>&1 || :
    fi
fi

%files
/usr/bin/pida-web-server
/usr/share/pida-web
/usr/lib/systemd/system/pida-web.service
%dir /etc/pida-web
%config(noreplace) /etc/pida-web/pida-web.env
%doc /usr/share/doc/pida-web
