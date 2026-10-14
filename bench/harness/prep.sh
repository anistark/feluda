#!/bin/bash
# prep.sh <cellkey> <A|B|AB>  -- reset warmth layers before a timed run.
# A = feluda's own cache. B = ecosystem state (tool caches in the cell's state dir, node_modules, obj).
source "$(dirname "$0")/env.sh"
cell=$1; mode=$2; source $BENCH_WORK/cells/$cell.env
S=$BENCH_WORK/state/$cell
case "$S" in $BENCH_WORK/state/*) ;; *) echo "refusing $S" >&2; exit 9;; esac
case "$WORK" in $BENCH_WORK/work/*) ;; *) echo "refusing $WORK" >&2; exit 9;; esac
case $mode in *A*) rm -rf "$S/home/Library/Caches/feluda" "$S/xdg/feluda";; esac
case $mode in *B*)
  chmod -R u+w "$S/gomod" "$S/gopath" 2>/dev/null
  for d in cargo gomod gopath m2 gem npm uv nuget gradle pnpm rlib; do rm -rf "$S/$d"; mkdir -p "$S/$d"; done
  rm -rf "$S/home/.nuget" "$S/home/.local" "$S/home/.cache"
  rm -rf "$WORK/node_modules"
  find "$WORK" -maxdepth 4 -name obj -type d -not -path "*/node_modules/*" -prune -exec rm -rf {} + 2>/dev/null
  ;;
esac
exit 0
