#!/bin/sh
# Run a Ghidra script against an analysed program (read-only).
# usage: [GP=client] ghidra.sh <Script.java> [script args...]
#   GP=server (default): research/ghidra, enshrouded_server.exe
#   GP=client:           research/ghidra_client, the memory dump of the DRM-unpacked enshrouded.exe
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT=$1; shift
if [ "$GP" = client ]; then
    DIR=ghidra_client; PROJECT=enshrouded_client; PROGRAM=enshrouded_dump.exe
else
    DIR=ghidra; PROJECT=enshrouded_server; PROGRAM=enshrouded_server.exe
fi
GHIDRA_HEADLESS_MAXMEM=16G "$ROOT/tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat" \
    "$(cygpath -w "$ROOT/research/$DIR")" $PROJECT \
    -process $PROGRAM -noanalysis -readOnly \
    -scriptPath "$(cygpath -w "$ROOT/research/scripts/ghidra")" \
    -postScript "$SCRIPT" "$@" 2>&1 | grep -vE '^INFO|^WARN|^\s*$' | tail -20
