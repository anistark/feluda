#!/bin/bash
source "$(dirname "$0")/env.sh"; bench_env gen
for l in "$@"; do echo "### $l"; python3 -I "$(dirname "$0")/gen_synthetic.py" $l 2>&1 | tail -30; done
