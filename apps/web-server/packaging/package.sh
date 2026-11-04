#!/usr/bin/env bash
# PIDA Web – Linux-Pakete bauen: .deb (Debian/Ubuntu), .rpm (RHEL/Alma/Rocky/Fedora/openSUSE) und .tar.gz (alle
# übrigen, mit install.sh/uninstall.sh). Alle drei enthalten dasselbe statisch gebaute Programm (musl).
#
#   apps/web-server/packaging/package.sh \
#       --binary target/x86_64-unknown-linux-musl/release/pida-web-server \
#       --dist apps/web/dist --notices target/notices-web --out target/web-packages [--formats deb,rpm,tar]
#
# Version aus Cargo.toml (Workspace; PIDA_WEB_PKG_VERSION überschreibt). Benötigt: dpkg-deb (deb), rpmbuild (rpm), tar.
#
# Ablage im System (deb/rpm):
#   /usr/bin/pida-web-server                     Programm
#   /usr/share/pida-web/                         Oberfläche (nur diese Dateien liefert der Server aus)
#   /lib/systemd/system/pida-web.service (deb)   bzw. /usr/lib/systemd/system/pida-web.service (rpm)
#   /etc/pida-web/pida-web.env                   Adresse/Port (Conffile: bleibt bei Updates erhalten)
#   /etc/apache2/sites-available/pida-web.conf   nur deb, nicht aktiviert (a2ensite pida-web)
#   /usr/share/doc/pida-web/                     LICENSE, Drittlizenzen, Apache-Vorlage
# Archiv: /opt/pida-web (install.sh), Dienst unter /etc/systemd/system.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BINARY="" DIST="" NOTICES="" OUT="" FORMATS="deb,rpm,tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) BINARY="$2"; shift ;;
    --dist) DIST="$2"; shift ;;
    --notices) NOTICES="$2"; shift ;;
    --out) OUT="$2"; shift ;;
    --formats) FORMATS="$2"; shift ;;
    -h | --help) sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "package.sh: unbekanntes Argument $1" >&2; exit 2 ;;
  esac
  shift
done
die() { echo "package.sh: $*" >&2; exit 1; }
[ -x "$BINARY" ] || die "--binary: kein ausführbares Programm ($BINARY)"
[ -f "$DIST/index.html" ] && [ -d "$DIST/pkg" ] && [ -d "$DIST/pdfium" ] || die "--dist: kein vollständiger Build (apps/web/build.sh)"
[ -f "$NOTICES/THIRD-PARTY-NOTICES.md" ] && [ -f "$NOTICES/RUST-STD-COPYRIGHT.html" ] || die "--notices: THIRD-PARTY-NOTICES.md/RUST-STD-COPYRIGHT.html fehlen"
[ -n "$OUT" ] || die "--out fehlt"
[ ! -e "$DIST/samples" ] || die "--dist enthält samples/ (Testdateien) – nicht ausliefern"

# PIDA_WEB_PKG_VERSION überschreibt die Version (nur für Update-Tests, z. B. 0.2.5+test1)
VERSION="${PIDA_WEB_PKG_VERSION:-$(awk -F'"' '/^\[workspace.package\]/{w=1} w && /^version *=/{print $2; exit}' "$ROOT/Cargo.toml")}"
[ -n "$VERSION" ] || die "Version nicht in Cargo.toml gefunden"
ARCH=x86_64 DEB_ARCH=amd64
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Gemeinsamer Stand: Oberfläche, Dokumentation
copy_dist() { mkdir -p "$1" && cp -R "$DIST/." "$1/"; }
copy_docs() {
  mkdir -p "$1"
  cp "$ROOT/LICENSE" "$1/LICENSE"
  cp "$NOTICES/THIRD-PARTY-NOTICES.md" "$NOTICES/RUST-STD-COPYRIGHT.html" "$1/"
  cp "$HERE/apache-pida-web.conf" "$1/apache-pida-web.conf"
}
# Datei mit Rechten ablegen, Zielordner anlegen (GNU `install -D` gibt es unter macOS nicht)
put() { mkdir -p "$(dirname "$3")" && cp "$2" "$3" && chmod "$1" "$3"; }
unit() { sed -e "s|@BINDIR@|$1|g" -e "s|@DATADIR@|$2|g" "$HERE/pida-web.service.in"; }

# Paketbaum für deb/rpm ($1 = Ziel, $2 = Verzeichnis der systemd-Units)
stage_tree() {
  local s="$1" unitdir="$2"
  put 755 "$BINARY" "$s/usr/bin/pida-web-server"
  copy_dist "$s/usr/share/pida-web"
  mkdir -p "$s$unitdir"
  unit /usr/bin /usr/share/pida-web >"$s$unitdir/pida-web.service"
  put 644 "$HERE/pida-web.env" "$s/etc/pida-web/pida-web.env"
  copy_docs "$s/usr/share/doc/pida-web"
  find "$s" -type d -exec chmod 755 {} +
  find "$s/usr/share" "$s/etc" "$s$unitdir" -type f -exec chmod 644 {} +
}

built=()
case ",$FORMATS," in *,deb,*)
  s="$WORK/deb"
  stage_tree "$s" /lib/systemd/system
  put 644 "$HERE/apache-pida-web.conf" "$s/etc/apache2/sites-available/pida-web.conf"
  mkdir -p "$s/DEBIAN"
  for f in postinst prerm postrm; do put 755 "$HERE/debian/$f" "$s/DEBIAN/$f"; done
  printf '%s\n' /etc/pida-web/pida-web.env /etc/apache2/sites-available/pida-web.conf >"$s/DEBIAN/conffiles"
  size="$(du -sk --exclude=DEBIAN "$s" 2>/dev/null | awk '{print $1}' || du -sk "$s" | awk '{print $1}')"
  cat >"$s/DEBIAN/control" <<EOF
Package: pida-web
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: A-SIT
Installed-Size: $size
Section: web
Priority: optional
Homepage: https://github.com/a-sit/pida
Description: PIDA Web – PDF-Dokumente im Browser signieren (ID Austria)
 Server der Browser-Variante von PIDA: liefert die Oberfläche aus (PIDA-Kern
 und PDFium als WebAssembly) und stellt den Rückkanal (DataURL) für das
 Signieren mit ID Austria über A-Trust bereit. Speichert nichts. Lauscht nur
 lokal; von außen über einen Webserver mit HTTPS davor (Vorlage für Apache in
 /etc/apache2/sites-available/pida-web.conf, nicht aktiviert).
EOF
  deb="$OUT/pida-web_${VERSION}_${DEB_ARCH}.deb"
  dpkg-deb --root-owner-group --build "$s" "$deb" >/dev/null
  built+=("$deb")
;; esac

case ",$FORMATS," in *,rpm,*)
  s="$WORK/rpm-stage"
  stage_tree "$s" /usr/lib/systemd/system
  top="$WORK/rpmbuild"
  mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
  rpmbuild -bb --quiet --define "_topdir $top" --define "stage $s" --define "pkgversion $VERSION" \
    --target "$ARCH" "$HERE/pida-web.spec" >/dev/null
  rpm="$OUT/pida-web-${VERSION}-1.${ARCH}.rpm"
  cp "$top/RPMS/$ARCH/pida-web-${VERSION}-1.${ARCH}.rpm" "$rpm"
  built+=("$rpm")
;; esac

case ",$FORMATS," in *,tar,*)
  name="pida-web-${VERSION}-linux-${ARCH}"
  t="$WORK/$name"
  put 755 "$BINARY" "$t/bin/pida-web-server"
  copy_dist "$t/share/pida-web"
  unit /opt/pida-web/bin /opt/pida-web/share/pida-web >"$t/pida-web.service"
  cp "$HERE/pida-web.env" "$HERE/apache-pida-web.conf" "$HERE/tarball/LIESMICH.txt" "$t/"
  for f in install.sh uninstall.sh; do put 755 "$HERE/tarball/$f" "$t/$f"; done
  cp "$ROOT/LICENSE" "$NOTICES/THIRD-PARTY-NOTICES.md" "$NOTICES/RUST-STD-COPYRIGHT.html" "$t/"
  echo "$VERSION" >"$t/VERSION"
  tgz="$OUT/$name.tar.gz"
  tar -C "$WORK" --owner=0 --group=0 -czf "$tgz" "$name" 2>/dev/null || tar -C "$WORK" -czf "$tgz" "$name"
  built+=("$tgz")
;; esac

(cd "$OUT" && for f in "${built[@]}"; do b="$(basename "$f")"; sha256sum "$b" 2>/dev/null || shasum -a 256 "$b"; done) >"$OUT/SHA256SUMS-web"
ls -l "${built[@]}" "$OUT/SHA256SUMS-web"
