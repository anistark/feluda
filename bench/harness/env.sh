# Sourced by every harness script. Settings, all from the environment:
#   BENCH_WORK  scratch directory for clones, per cell state and work copies (required; tens of GB)
#   BENCH_BIN   the feluda binary under test (default: $BENCH_WORK/feluda, a copy that stays put)
#   BENCH_PROXY optional HTTP proxy for every request, for example http://127.0.0.1:18888 from v4proxy.py
#   GITHUB_TOKEN taken from `gh auth token` when unset. It lives only in the environment, never in a file.
# The token is fetched once, before HOME is isolated, because gh keeps its login under the real HOME.
if [ -z "${GITHUB_TOKEN+x}" ]; then export GITHUB_TOKEN=$(gh auth token 2>/dev/null); fi
: "${BENCH_WORK:?set BENCH_WORK to a scratch directory outside the repository}"
export BENCH_WORK
export BENCH_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export BENCH_BIN=${BENCH_BIN:-$BENCH_WORK/feluda}
# rustup lives under the real HOME, which bench_env replaces
export RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
bench_env_dirs() { # prints the list of tool cache dirs for a state root
  local s=$1
  echo "$s/home $s/xdg $s/cargo $s/gomod $s/gopath $s/m2 $s/gem $s/npm $s/uv $s/nuget $s/gradle $s/pnpm $s/rlib"
}
bench_env() {
  local s=$BENCH_WORK/state/$1
  mkdir -p $(bench_env_dirs $s)
  export HOME=$s/home XDG_CACHE_HOME=$s/xdg CARGO_HOME=$s/cargo GOMODCACHE=$s/gomod GOPATH=$s/gopath
  export MAVEN_REPO_LOCAL=$s/m2 GEM_HOME=$s/gem GEM_PATH=$s/gem npm_config_cache=$s/npm UV_CACHE_DIR=$s/uv
  export NUGET_PACKAGES=$s/nuget GRADLE_USER_HOME=$s/gradle PNPM_HOME=$s/pnpm R_LIBS_USER=$s/rlib
  export FELUDA_CLEARLYDEFINED_ENABLED=${FELUDA_CLEARLYDEFINED_ENABLED:-true}
  export DOTNET_SYSTEM_NET_DISABLEIPV6=1 DOTNET_CLI_HOME=$s/home DOTNET_NOLOGO=1 DOTNET_CLI_TELEMETRY_OPTOUT=1
  bench_net
}
export DOTNET_SYSTEM_NET_DISABLEIPV6=1

# With BENCH_PROXY set, all traffic goes through it (see v4proxy.py); unset, requests go out directly.
bench_net() {
  [ -n "${BENCH_PROXY:-}" ] || return 0
  export HTTPS_PROXY=$BENCH_PROXY HTTP_PROXY=$BENCH_PROXY https_proxy=$BENCH_PROXY http_proxy=$BENCH_PROXY
  export NO_PROXY=localhost,127.0.0.1 no_proxy=localhost,127.0.0.1
  local hostport=${BENCH_PROXY#*://}; hostport=${hostport%%/*}
  export MAVEN_OPTS="-Dhttp.proxyHost=${hostport%:*} -Dhttp.proxyPort=${hostport##*:} -Dhttps.proxyHost=${hostport%:*} -Dhttps.proxyPort=${hostport##*:}"
  export GRADLE_OPTS="$MAVEN_OPTS"
}

# Homebrew's Go when present (macOS ships an old one), and never let go download a toolchain.
[ -d /opt/homebrew/opt/go/bin ] && export PATH=/opt/homebrew/opt/go/bin:$PATH
export GOTOOLCHAIN=local GOFLAGS=${GOFLAGS:-}

# ScanCode needs libmagic; on macOS it comes from Homebrew
if [ -d /opt/homebrew/opt/libmagic ]; then
  export TYPECODE_LIBMAGIC_PATH=/opt/homebrew/opt/libmagic/lib/libmagic.dylib TYPECODE_LIBMAGIC_DB_PATH=/opt/homebrew/opt/libmagic/share/misc/magic.mgc
fi
