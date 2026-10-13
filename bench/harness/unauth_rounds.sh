#!/bin/bash
# Unauthenticated first run: 3 rounds at least 61 minutes apart, one scan per subject per round (GitHub allows 60 requests/hour, a cold scan spends 14).
cd "$(dirname "$0")"
R=../results/raw
for round in 1 2 3; do
  start=$(date +%s)
  for s in rust/hyperfine node/express python/requests; do
    ONE_SCAN=1 SAMPLES=1 ./run_cell.sh $s unauth-cold
    [ -d $R/$s/unauth-cold ] && { rm -rf $R/$s/unauth-cold+r$round; mv $R/$s/unauth-cold $R/$s/unauth-cold+r$round; }
    python3 -I progress.py log "phase 6 round $round: $s done"
  done
  [ $round = 3 ] && break
  while [ $(( $(date +%s) - start )) -lt 3700 ]; do sleep 30; done
done
echo done > ../results/unauth_rounds.done
