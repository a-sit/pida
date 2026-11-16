#!/usr/bin/env bash
# Coverage-geführtes Fuzzing (cargo-fuzz/libFuzzer) der Teile, die fremde Eingaben verarbeiten; Beschreibung:
# docs/entwicklung.md („Fuzzing“).
#
#   tools/fuzz.sh [SEKUNDEN] [ZIEL …]     Standard: 60 Sekunden je Ziel, alle Ziele
#
# Ziele: pdf_open, pdf_xmp (pdf-cos), pades_prepare (Signaturvorbereitung), sl12_response (Security-Layer-XML).
# Braucht eine Nightly-Toolchain und cargo-fuzz (PIDA_FUZZ_TOOLCHAIN, Standard „nightly“):
#   rustup toolchain install nightly --profile minimal && cargo install cargo-fuzz --locked
# Der Korpus wächst in fuzz/corpus/<Ziel> (nicht eingecheckt); Startwerte sind die Dateien aus testdata/.
# Funde liegen in fuzz/artifacts/<Ziel>.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FUZZ="$ROOT/fuzz"
TOOLCHAIN="${PIDA_FUZZ_TOOLCHAIN:-nightly}"
SECONDS_PER_TARGET="${1:-60}"
[ "$#" -gt 0 ] && shift
if [ "$#" -gt 0 ]; then TARGETS=("$@"); else TARGETS=(pdf_open pdf_xmp pades_prepare sl12_response); fi

command -v cargo-fuzz >/dev/null || { echo "cargo-fuzz fehlt: cargo install cargo-fuzz --locked" >&2; exit 2; }

cd "$FUZZ"
for target in "${TARGETS[@]}"; do
  seed_dirs=()
  dict=()
  case "$target" in
    pdf_open | pades_prepare)
      seed_dirs=(pdf pdf-cos pdf-incremental)
      dict=("-dict=$FUZZ/pdf.dict")
      ;;
    sl12_response)
      seed_dirs=(sl12/responses)
      dict=("-dict=$FUZZ/sl12.dict")
      ;;
    pdf_xmp) ;;
    *)
      echo "unbekanntes Ziel: $target" >&2
      exit 2
      ;;
  esac
  mkdir -p "corpus/$target"
  seeds=()
  for dir in ${seed_dirs[@]+"${seed_dirs[@]}"}; do
    [ -d "$ROOT/testdata/$dir" ] && seeds+=("$ROOT/testdata/$dir")
  done
  echo "== Fuzz-Ziel $target: $SECONDS_PER_TARGET s"
  # -timeout: ein einzelner Durchlauf über 20 s gilt als Hänger; -rss_limit_mb: Speicherbombe; -max_len deckt den
  # ganzen Startkorpus ab (größte Datei unter 1 MiB)
  rustup run "$TOOLCHAIN" cargo fuzz run "$target" "corpus/$target" ${seeds[@]+"${seeds[@]}"} -- \
    -max_total_time="$SECONDS_PER_TARGET" -timeout=20 -rss_limit_mb=2048 -max_len=1048576 \
    ${dict[@]+"${dict[@]}"} -print_final_stats=1
done
