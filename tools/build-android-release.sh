#!/usr/bin/env bash
# PIDA – Android-Pakete einer Version bauen: APK (arm64, für GitHub) und App Bundle (alle ABIs, für Google Play).
#
#   tools/build-android-release.sh     # → dist/release-<Version>/assets/PIDA_<Version>_android_arm64.apk
#                                      #   dist/release-<Version>/PIDA_<Version>_android.aab
#
# Signiert mit dem Schlüssel aus ~/.pida-release/keystore.properties (bei Google Play nur der Upload-Schlüssel).
# Das App Bundle trägt die Symboltabelle des nativen Codes (Google Play: lesbare Absturzberichte); dafür wird der
# Kern hier ungestrippt mit Zeilentabellen gebaut; Gradle strippt die ausgelieferten Bibliotheken und legt die
# Symboltabelle daneben ins Bundle. Hochgeladen wird nichts (tools/play-upload.py).
# Ablauf und Hintergründe: docs/integration/mobile-android.md §3 und §8.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/apps/app/src-tauri/tauri.conf.json" | head -1)"
OUT="$ROOT/dist/release-$VERSION"
PROPS="${PIDA_ANDROID_KEYSTORE_PROPERTIES:-$HOME/.pida-release/keystore.properties}"
[ -f "$PROPS" ] || { echo "Fehlt: $PROPS (Signaturschlüssel)" >&2; exit 1; }

cd "$ROOT"
# shellcheck disable=SC1091
PIDA_RELEASE_REMAP=1 source tools/env.sh
[ -d tools/pdfium/android/jniLibs/arm64-v8a ] || tools/fetch-pdfium.sh --jnilibs
tools/third-party-notices.sh --pdfium android-arm64 --out target/notices-android

cd apps/app
export CARGO_TARGET_DIR="$ROOT/target-android"
export PIDA_ANDROID_KEYSTORE_PROPERTIES="$PROPS"
export PIDA_NOTICES_DIR=target/notices-android PIDA_REQUIRE_NOTICES=1 CI=true
# Ungestrippt mit Zeilentabellen, damit Gradle die Symboltabelle ziehen kann. Ausgeliefert wird die von Gradle
# gestrippte Fassung; Pfade sind über PIDA_RELEASE_REMAP ersetzt.
export CARGO_PROFILE_RELEASE_STRIP=none CARGO_PROFILE_RELEASE_DEBUG=line-tables-only

npm run tauri android build -- --apk --target aarch64
npm run tauri android build -- --aab

G=src-tauri/gen/android/app/build/outputs
mkdir -p "$OUT/assets"
cp "$G/apk/universal/release/app-universal-release.apk" "$OUT/assets/PIDA_${VERSION}_android_arm64.apk"
cp "$G/bundle/universalRelease/app-universal-release.aab" "$OUT/PIDA_${VERSION}_android.aab"

AAB="$OUT/PIDA_${VERSION}_android.aab"
echo "== Symboltabellen im Bundle"
unzip -l "$AAB" | grep -E 'debugsymbols/.*\.so\.sym$' || { echo "FEHLER: keine Symboltabellen im Bundle" >&2; exit 1; }
echo "== Bibliotheken im Bundle (gestrippt)"
unzip -l "$AAB" | grep -E 'base/lib/.*\.so$'
check="$(mktemp -d)"
trap 'rm -rf "$check"' EXIT
unzip -q -o "$AAB" 'base/lib/arm64-v8a/libpida_app_lib.so' 'BUNDLE-METADATA/com.android.tools.build.debugsymbols/*' -d "$check"
if [ "$(wc -c <"$check/base/lib/arm64-v8a/libpida_app_lib.so")" -ge "$(wc -c <"$CARGO_TARGET_DIR/aarch64-linux-android/release/libpida_app_lib.so")" ]; then
  echo "FEHLER: libpida_app_lib.so im Bundle ist nicht gestrippt" >&2
  exit 1
fi
if grep -rqa "$HOME" "$check"; then
  echo "FEHLER: Benutzerpfade im Bundle (Bibliothek oder Symboltabelle)" >&2
  exit 1
fi
echo "Fertig: $OUT"
