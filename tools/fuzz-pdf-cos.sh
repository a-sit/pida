#!/usr/bin/env bash
# Coverage-geführtes Fuzzing von pida-pdf-cos (cargo-fuzz/libFuzzer); Beschreibung: docs/entwicklung.md („Fuzzing“).
#
#   tools/fuzz-pdf-cos.sh [SEKUNDEN] [ZIEL …]     Standard: 60 Sekunden je Ziel, alle Ziele (open, xmp)
#
# Braucht eine Nightly-Toolchain und cargo-fuzz (PIDA_FUZZ_TOOLCHAIN, Standard „nightly“):
#   rustup toolchain install nightly --profile minimal && cargo install cargo-fuzz --locked
# Der Korpus wächst in crates/pdf-cos/fuzz/corpus/<Ziel> (nicht eingecheckt); Startwerte für „open“ sind die PDFs
# aus testdata/. Funde liegen in crates/pdf-cos/fuzz/artifacts/<Ziel>; nachstellen mit
#   PIDA_FUZZ_FILE=<Fund> cargo test -p pida-pdf-cos --test robustness replay_single_file -- --nocapture
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FUZZ="$ROOT/crates/pdf-cos/fuzz"
TOOLCHAIN="${PIDA_FUZZ_TOOLCHAIN:-nightly}"
SECONDS_PER_TARGET="${1:-60}"
[ "$#" -gt 0 ] && shift
if [ "$#" -gt 0 ]; then TARGETS=("$@"); else TARGETS=(open xmp); fi

command -v cargo-fuzz >/dev/null || { echo "cargo-fuzz fehlt: cargo install cargo-fuzz --locked" >&2; exit 2; }

cd "$FUZZ"
for target in "${TARGETS[@]}"; do
  mkdir -p "corpus/$target"
  seeds=()
  if [ "$target" = open ]; then
    for dir in pdf pdf-cos pdf-incremental; do
      [ -d "$ROOT/testdata/$dir" ] && seeds+=("$ROOT/testdata/$dir")
    done
  fi
  echo "== Fuzz-Ziel $target: $SECONDS_PER_TARGET s"
  # -timeout: ein einzelner Durchlauf über 20 s gilt als Hänger; -rss_limit_mb: Speicherbombe; -max_len deckt den
  # ganzen Startkorpus ab (größte Datei unter 1 MiB)
  rustup run "$TOOLCHAIN" cargo fuzz run "$target" "corpus/$target" ${seeds[@]+"${seeds[@]}"} -- \
    -max_total_time="$SECONDS_PER_TARGET" -timeout=20 -rss_limit_mb=2048 -max_len=1048576 \
    -dict="$FUZZ/pdf.dict" -print_final_stats=1
done
