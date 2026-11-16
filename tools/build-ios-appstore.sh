#!/usr/bin/env bash
# PIDA – iOS-Paket für TestFlight bauen: Archiv mit Tauri, Export mit lokalem Verteilungszertifikat.
#
#   tools/build-ios-appstore.sh            # → dist/release-<Version>/PIDA_<Version>_ios_appstore.ipa
#   tools/build-ios-appstore.sh --export   # nur exportieren (vorhandenes Archiv derselben Version)
#
# Signiert wird von Hand – das Archiv über tools/ios/appstore-signing.xcconfig, der Export über
# tools/ios/ExportOptions-appstore.plist: Identität „iPhone Distribution“ des A-SIT-Teams im Schlüsselbund
# und Profil „PIDA App Store“. Das braucht weder ein angemeldetes Konto in Xcode noch den
# App-Store-Connect-Schlüssel.
#
# Der Schlüssel aus ~/.pida-release/appstore.env darf den Build NICHT erreichen: Mit ihm in der Umgebung
# reicht Tauri ihn an xcodebuild weiter, und danach kennt die Kommandozeile das in Xcode angemeldete Konto
# nicht mehr („No Accounts“, docs/integration/mobile-ios.md §11). Er wird nur zum Hochladen gebraucht
# (tools/testflight-upload.py lädt ihn selbst). Hochgeladen wird hier nichts.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEAM="${APPLE_DEVELOPMENT_TEAM:-9CYHJNG644}"
PROFILE="PIDA App Store"
GEN="$ROOT/apps/app/src-tauri/gen/apple"
ARCHIVE="$GEN/build/pida-app_iOS.xcarchive"
VERSION="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/apps/app/src-tauri/tauri.conf.json" | head -1)"
OUT="$ROOT/dist/release-$VERSION"
LOG="$ROOT/target-ios/ios-appstore-$VERSION.log"
ONLY_EXPORT=0
[ "${1:-}" = "--export" ] && ONLY_EXPORT=1

# Kein App-Store-Connect-Schlüssel im Build, auch wenn die aufrufende Shell ihn gesetzt hat.
unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_API_KEY_ID APPLE_API_ISSUER_ID

# Voraussetzungen fürs Signieren zuerst prüfen – sonst fällt es erst nach dem langen Build auf.
security find-identity -v -p codesigning | grep -q "iPhone Distribution: .*($TEAM)" || {
  echo "Fehlt: Identität 'iPhone Distribution' des Teams $TEAM mit privatem Schlüssel im Schlüsselbund." >&2; exit 1; }
found=0
for p in "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"/*.mobileprovision \
         "$HOME/Library/MobileDevice/Provisioning Profiles"/*.mobileprovision; do
  [ -f "$p" ] || continue
  if security cms -D -i "$p" 2>/dev/null | grep -A1 "<key>Name</key>" | grep -q "<string>$PROFILE</string>"; then found=1; break; fi
done
[ "$found" = 1 ] || { echo "Fehlt: Profil '$PROFILE' (als .mobileprovision in Xcodes Profilordner)." >&2; exit 1; }

mkdir -p "$OUT" "$ROOT/target-ios"
if [ "$ONLY_EXPORT" = 0 ]; then
  stamp="$(mktemp)"
  shim="$(mktemp -d)"
  (
    cd "$ROOT"
    # shellcheck disable=SC1091
    PIDA_RELEASE_REMAP=1 source tools/env.sh >/dev/null 2>&1
    tools/fetch-pdfium.sh --xcframework >/dev/null
    cd apps/app
    export CARGO_TARGET_DIR="$ROOT/target-ios" LANG=en_US.UTF-8
    # Die automatische Signatur des Projekts für diesen einen Build überschreiben. Tauri ruft xcodebuild über
    # den PATH auf und entfernt XCODE_XCCONFIG_FILE aus dessen Umgebung; deshalb hängt ein vorgeschaltetes
    # xcodebuild die Datei als -xcconfig an den Build-Aufruf an (Export und Abfragen bleiben unverändert).
    cat >"$shim/xcodebuild" <<SHIM
#!/bin/bash
for a in "\$@"; do
  case "\$a" in build|archive) exec /usr/bin/xcodebuild -xcconfig "$ROOT/tools/ios/appstore-signing.xcconfig" "\$@";; esac
done
exec /usr/bin/xcodebuild "\$@"
SHIM
    chmod +x "$shim/xcodebuild"
    export PATH="$shim:$PATH"
    # Tauri exportiert nach dem Archiv selbst mit eigenen Exporteinstellungen; ob das gelingt, ist hier
    # unerheblich – maßgeblich ist das Archiv, exportiert wird unten von Hand.
    APPLE_DEVELOPMENT_TEAM="$TEAM" CI=true npx tauri ios build --target aarch64 --export-method app-store-connect \
      >"$LOG" 2>&1 || true
  )
  # Tauri schreibt beim Bauen in versionierte Xcode-Dateien; unverändert zurücklassen.
  git -C "$ROOT" checkout -- "$GEN/pida-app.xcodeproj/project.pbxproj" "$GEN/ExportOptions.plist" 2>/dev/null || true
  [ -d "$ARCHIVE" ] && [ "$ARCHIVE" -nt "$stamp" ] || { rm -rf "$stamp" "$shim"; tail -30 "$LOG" >&2; echo "Kein neues Archiv – siehe $LOG" >&2; exit 1; }
  rm -rf "$stamp" "$shim"
fi

[ -d "$ARCHIVE" ] || { echo "Kein Archiv unter $ARCHIVE" >&2; exit 1; }
got="$(/usr/libexec/PlistBuddy -c "Print ApplicationProperties:CFBundleShortVersionString" "$ARCHIVE/Info.plist")"
[ "$got" = "$VERSION" ] || { echo "Archiv hat Version $got, erwartet $VERSION" >&2; exit 1; }

rm -rf "$OUT/export-ios"
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportPath "$OUT/export-ios" \
  -exportOptionsPlist "$ROOT/tools/ios/ExportOptions-appstore.plist" 2>&1 | grep -E "^error|EXPORT (SUCCEEDED|FAILED)" || true
[ -f "$OUT/export-ios/PIDA.ipa" ] || { echo "Export fehlgeschlagen" >&2; exit 1; }
cp "$OUT/export-ios/PIDA.ipa" "$OUT/PIDA_${VERSION}_ios_appstore.ipa"
echo "Fertig: $OUT/PIDA_${VERSION}_ios_appstore.ipa"
echo "Hochladen: python3 tools/testflight-upload.py \"$OUT/PIDA_${VERSION}_ios_appstore.ipa\""
