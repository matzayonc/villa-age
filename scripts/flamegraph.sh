#!/usr/bin/env bash
# Profiles a run with `perf` and renders a flamegraph SVG, via `cargo flamegraph`.
#
#   scripts/flamegraph.sh                        # 60 simulated seconds, headless, default map
#   scripts/flamegraph.sh --duration 300         # any villa-age flags are passed through
#   scripts/flamegraph.sh --map big.ron --seed 3
#   scripts/flamegraph.sh --windowed --duration 30    # profile with rendering (first arg only)
#   OUT=perf/run1.svg FREQ=999 scripts/flamegraph.sh  # output path, sampling rate in Hz
#
# Runs headless by default: that's the simulation being measured, and it steps as fast as it can,
# so a 60 s run is a few seconds of wall time.
#
# Needs `perf` (Manjaro/Arch: `sudo pacman -S perf`) and `cargo flamegraph`
# (`cargo install flamegraph`). The binary is built with the `profiling` cargo profile (release
# speed plus debug info, for symbol names) and with frame pointers, so perf can walk the stack
# cheaply and reliably. DWARF unwinding (`cargo flamegraph`'s default) makes huge recordings
# that lose samples under load, and `perf script` mangles its call chains on some perf builds.
# Only Rust code is rebuilt with frame pointers; stacks that pass through libc or the
# precompiled std can be cut short at that point.

set -euo pipefail

cd "$(dirname "$0")/.."

OUT="${OUT:-target/flamegraph/flamegraph.svg}"
FREQ="${FREQ:-499}"

mode=(--headless)
if [[ "${1:-}" == "--windowed" ]]; then
    mode=()
    shift
fi
args=("$@")
if [[ ${#args[@]} -eq 0 ]]; then
    args=(--duration 60)
fi

missing=()
command -v perf >/dev/null || missing+=("perf (sudo pacman -S perf)")
cargo flamegraph --version >/dev/null 2>&1 || missing+=("cargo-flamegraph (cargo install flamegraph)")
if [[ ${#missing[@]} -gt 0 ]]; then
    printf 'missing: %s\n' "${missing[@]}" >&2
    exit 1
fi

# perf_event_paranoid > 2 forbids profiling even your own processes without CAP_PERFMON.
paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)
if (( paranoid > 2 )); then
    echo "kernel.perf_event_paranoid is $paranoid; run: sudo sysctl kernel.perf_event_paranoid=2" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUT")"
echo "profiling: villa-age ${mode[*]} ${args[*]}"
# `-c` replaces cargo flamegraph's default `perf record` (DWARF) with frame-pointer walking.
RUSTFLAGS="${RUSTFLAGS:-} -C force-frame-pointers=yes" cargo flamegraph \
    --profile profiling \
    --bin villa-age \
    --cmd "record -F $FREQ -g" \
    --output "$OUT" \
    --title "villa-age ${mode[*]} ${args[*]}" \
    -- "${mode[@]}" "${args[@]}"

# cargo flamegraph leaves the recording in the working directory; keep it next to the graph for
# `perf report -i <data>`.
data="${OUT%.svg}.perf.data"
mv -f perf.data "$data"
rm -f perf.data.old

echo "wrote $OUT (samples in $data)"
