#!/bin/bash
# Writes results/env.json (machine, toolchains, binary identity). Safe to rerun.
source "$(dirname "$0")/env.sh"
R=$(dirname "$BENCH_ROOT")
v() { "$@" 2>&1 | head -1 | tr -d '"\\'; }
cat > $BENCH_ROOT/results/env.json <<J
{
 "date": "$(date '+%Y-%m-%d %H:%M %z')",
 "feluda_binary": "$(basename "$BENCH_BIN")",
 "feluda_binary_sha256": "$(shasum -a 256 $BENCH_BIN | cut -d' ' -f1)",
 "feluda_version": "$($BENCH_BIN --version | grep -o "Feluda v[0-9.]*")",
 "git_head": "$(git -C $R rev-parse HEAD)",
 "git_head_short": "$(git -C $R rev-parse --short HEAD)",
 "git_diff_hash": "$(git -C $R diff HEAD | shasum | cut -d' ' -f1)",
 "git_branch": "$(git -C $R branch --show-current)",
 "os": "$(sw_vers -productName) $(sw_vers -productVersion) $(uname -m)",
 "cpu": "$(sysctl -n machdep.cpu.brand_string)",
 "cores": $(sysctl -n hw.ncpu),
 "mem_gb": $(( $(sysctl -n hw.memsize) / 1073741824 )),
 "rustc": "$(v rustc --version)", "cargo": "$(v cargo --version)",
 "go": "$(v go version)", "node": "$(v node --version)", "npm": "$(v npm --version)", "pnpm": "$(v pnpm --version)",
 "python": "$(v python3 --version)", "uv": "$(v uv --version)",
 "java": "$(v java -version)", "mvn": "$(v mvn --version)", "gradle": "$(gradle --version 2>&1 | grep -m1 '^Gradle')",
 "ruby": "$(v ruby --version)", "bundler": "$(v bundle --version)", "dotnet": "$(v dotnet --version)", "R": "$(v R --version)",
 "hyperfine": "$(v hyperfine --version)",
 "syft": "$(syft version 2>&1 | grep -m1 ^Version)", "trivy": "$(v trivy --version)",
 "network": "all traffic via harness/v4proxy.py on 127.0.0.1:18888 (IPv4 only, 1.5 s per address); IPv6 is blackholed on this network",
 "power": "$(pmset -g batt | head -1 | tr -d '\"')"
}
J
python3 -I -c "import json;json.load(open('$BENCH_ROOT/results/env.json'))" && echo env.json ok
