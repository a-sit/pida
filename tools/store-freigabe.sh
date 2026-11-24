#!/usr/bin/env bash
# PIDA – eine Version als „im Store verfügbar“ markieren (ADR 0004 Nachtrag 31).
#
#   tools/store-freigabe.sh android 0.9.0        # im Play Store zu haben
#   tools/store-freigabe.sh ios 0.9.0            # in TestFlight bzw. im App Store zu haben
#   tools/store-freigabe.sh android 0.9.0 --zurueck   # Markierung wieder entfernen
#
# Die Apps auf iOS/Android fragen wie der Desktop die GitHub-Releases ab, melden eine neue Version aber erst,
# wenn das Release diese Markierungsdatei trägt (`PIDA_<Version>_store-android.txt` bzw. `…_store-ios.txt`,
# crates/core-api/src/update.rs). Erst markieren, wenn die Version im Store für die Tester wirklich zu haben ist
# (nach Googles bzw. Apples Prüfung) – sonst führt der Hinweis in einen Store ohne Update.
#
# Braucht `gh` (angemeldet, Schreibrecht auf a-sit/pida). Lädt nur diese eine Textdatei zum Release hoch.
set -euo pipefail

usage() {
  sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

[ $# -ge 2 ] || usage
store="$1"
version="${2#v}"
undo="${3:-}"
case "$store" in
  android) wo="im Google Play Store" ;;
  ios) wo="in TestFlight bzw. im App Store" ;;
  *) usage ;;
esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "store-freigabe: keine Version: $version" >&2; exit 2; }
[ -z "$undo" ] || [ "$undo" = "--zurueck" ] || usage

repo="a-sit/pida"
tag="v$version"
name="PIDA_${version}_store-${store}.txt"

if [ "$undo" = "--zurueck" ]; then
  gh release delete-asset "$tag" "$name" --repo "$repo" --yes
  echo "Markierung entfernt: $name ($tag)"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
printf 'PIDA %s ist %s verfügbar.\n' "$version" "$wo" > "$tmp/$name"
gh release upload "$tag" "$tmp/$name" --repo "$repo" --clobber
echo "Markiert: PIDA $version ist $wo verfügbar ($name am Release $tag)."
