#!/bin/bash
# usage: scan_once.sh <owner/name> [feluda args]  -- one fresh scan of a clone, to size it for make_subjects.py
# (json report, request stats and stderr under results/raw/_phase1/)
source "$(dirname "$0")/env.sh"
r=$1; shift; id=${r//\//__}
d=$BENCH_WORK/repos/$id
out=$BENCH_ROOT/results/raw/_phase1/$id; mkdir -p $out
bench_env p1_$id
s=$(date +%s)
FELUDA_REQUEST_STATS=$out/requests.json timeout ${SCAN_TIMEOUT:-2400} /usr/bin/time -l "$BENCH_BIN" --path "$d" --json "$@" >$out/report.json 2>$out/stderr.raw
rc=$?
e=$(date +%s)
sed "s/$GITHUB_TOKEN/<TOKEN>/g" $out/stderr.raw > $out/stderr.log; rm $out/stderr.raw
echo "$r rc=$rc secs=$((e-s)) rows=$(jq 'length' $out/report.json 2>/dev/null) reqs=$(jq .total_requests $out/requests.json 2>/dev/null)"
