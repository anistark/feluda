#!/bin/bash
# install.sh <cellkey>  -- warm layer B: fetch/install the project's dependencies into the cell's isolated state.
# No package scripts run (npm/pnpm/yarn --ignore-scripts, cargo fetch, go mod download, dotnet restore).
H=$(dirname "$0"); source "$H/env.sh"
cell=$1; source $BENCH_WORK/cells/$cell.env
bench_env $cell
cd "$WORK" || exit 1
rc=0
for l in $LANGS; do
  case $l in
    rust)   cargo fetch -q 2>&1 | tail -3; [ ${PIPESTATUS[0]} -eq 0 ] || rc=1 ;;
    node)
      export npm_config_manage_package_manager_versions=false COREPACK_ENABLE_STRICT=0 COREPACK_ENABLE_AUTO_PIN=0
      if   [ -f pnpm-lock.yaml ]; then pnpm install --frozen-lockfile --ignore-scripts --config.confirmModulesPurge=false 2>&1 | tail -3
      elif [ -f yarn.lock ];      then (command -v yarn >/dev/null && yarn install --frozen-lockfile --ignore-scripts --non-interactive 2>&1 | tail -3) || { echo "yarn install failed"; rc=1; }
      elif [ -f package-lock.json ]; then npm ci --ignore-scripts --no-audit --no-fund --legacy-peer-deps --force 2>&1 | tail -3
      else npm install --ignore-scripts --no-audit --no-fund --legacy-peer-deps 2>&1 | tail -3; fi
      [ ${PIPESTATUS[0]} -eq 0 ] || rc=1 ;;
    go)     GOFLAGS=-mod=mod go mod download 2>&1 | tail -3; [ ${PIPESTATUS[0]} -eq 0 ] || rc=1 ;;
    dotnet) for p in *.csproj *.fsproj *.vbproj *.slnx; do [ -f "$p" ] && { dotnet restore "$p" 2>&1 | tail -2; [ ${PIPESTATUS[0]} -eq 0 ] || rc=1; }; done ;;
    java|ruby)
      tmp=$BENCH_WORK/state/$cell/_install_report.json
      "$BENCH_BIN" --path "$WORK" --json --no-clearlydefined > "$tmp" 2>/dev/null
      python3 -I "$H/install_fetch.py" $l "$tmp" "$BENCH_WORK/state/$cell" ;;
    *) ;;
  esac
done
exit $rc
