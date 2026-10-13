#!/bin/bash
# usage: clone.sh owner/name [sha]
# Clone into $BENCH_WORK/repos/owner__name with hooks off and no global git config; one commit deep.
# With a sha, that exact commit is fetched and checked out (the pinned corpus); without, the default branch.
source "$(dirname "$0")/env.sh"; bench_net
r=$1; sha=${2:-}; d=$BENCH_WORK/repos/${r//\//__}
g() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_TERMINAL_PROMPT=0 git -c core.hooksPath=/dev/null -c protocol.file.allow=never "$@"; }
if [ -d "$d/.git" ]; then echo "$r exists"
elif [ -n "$sha" ]; then
  mkdir -p "$d" && g -C "$d" init -q && g -C "$d" remote add origin "https://github.com/$r.git" \
    && g -C "$d" fetch -q --depth 1 --no-tags origin "$sha" && g -C "$d" -c advice.detachedHead=false checkout -q FETCH_HEAD 2>&1 | tail -2
else
  g clone --depth 1 --no-tags --single-branch -q "https://github.com/$r.git" "$d" 2>&1 | tail -2
fi
echo "$r $(git -C $d rev-parse HEAD 2>/dev/null) $(du -sm $d 2>/dev/null | cut -f1)MB"
