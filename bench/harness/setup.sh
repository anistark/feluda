#!/bin/bash
# usage: setup.sh
# Prepare $BENCH_WORK: unpack the synthetic corpus and clone every real repository at its pinned commit.
# Nothing in a clone is ever run. Safe to rerun; existing clones are left alone.
H=$(dirname "$0"); source "$H/env.sh"
mkdir -p "$BENCH_WORK"
[ -d "$BENCH_WORK/synthetic" ] || tar -C "$BENCH_WORK" -xzf "$BENCH_ROOT/corpus/synthetic.tar.gz"
python3 -I - "$BENCH_ROOT/corpus/subjects.json" <<'PY' | while read -r repo sha; do "$H/clone.sh" "$repo" "$sha"; done
import json, sys
for s in json.load(open(sys.argv[1])):
    if s["kind"] == "real": print(s["repo"], s["sha"])
PY
[ -x "$BENCH_BIN" ] || echo "Copy the feluda build under test to $BENCH_BIN (or set BENCH_BIN) before running cells."
