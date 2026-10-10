#!/bin/sh
set -e

HERE=$(cd -- "$(dirname -- "$0")" && pwd)
SIM_DIR="$HERE/simulator"
SIM_BUILD="$SIM_DIR/build"
HARNESS="$HERE/esp-devkit/tools/harness/harness.py"

sim_build() {
    [ -d "$SIM_BUILD" ] || cmake --fresh -S "$SIM_DIR" -B "$SIM_BUILD" -G Ninja
    cmake --build "$SIM_BUILD"
}

usage() {
    cat >&2 <<'USAGE'
Usage:
  run.sh [simulator]                                host simulator (SDL window)
  run.sh simulator --verify <script> [args...]      harness script against the simulator
  run.sh esp32p4 [idf args...]                      idf.py on the device (default: flash monitor)
  run.sh esp32p4 --verify <port> <script> [args...] harness script against a flashed device
USAGE
    exit 1
}

TARGET=${1:-simulator}
[ $# -gt 0 ] && shift

case "$TARGET" in
  simulator)
    if [ "$1" = "--verify" ]; then
        shift
        [ $# -ge 1 ] || usage
        SCRIPT=$1; shift
        sim_build
        "$HARNESS" --sim "$SIM_BUILD/simulator" --out "$HERE/captures" "$SCRIPT" "$@"
    else
        sim_build
        "$SIM_BUILD/simulator"
    fi
    ;;
  esp32p4)
    if [ "$1" = "--verify" ]; then
        shift
        [ $# -ge 2 ] || usage
        PORT=$1; SCRIPT=$2; shift 2
        "$HARNESS" --port "$PORT" --out "$HERE/captures" "$SCRIPT" "$@"
    else
        [ $# -gt 0 ] || set -- flash monitor
        idf.py -C "$HERE/$TARGET" "$@"
    fi
    ;;
  *)
    usage
    ;;
esac
