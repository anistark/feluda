#!/bin/bash
# ecosystem_time.sh <subject> <cold|warm>  -- time the ecosystem tools feluda shells out to, in the same layer B state.
H=$(dirname "$0"); source "$H/env.sh"
subject=$1; state=$2
eval "$(python3 -I $H/subj.py shell "$subject")" || exit 2
cell="$(echo "$subject" | tr '/' '_')__ecotool_$state"
raw=$BENCH_ROOT/results/raw/$subject/ecotool-$state; mkdir -p "$raw" $BENCH_WORK/cells
S=$BENCH_WORK/state/$cell; W=$BENCH_WORK/work/$cell
chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"; cp -cR "$PRISTINE" "$W" || cp -R "$PRISTINE" "$W"
printf 'WORK=%s\nLANGS="%s"\n' "$W" "$LANGS" > $BENCH_WORK/cells/$cell.env
bench_env $cell
[ $state = warm ] && { "$H/install.sh" $cell > $raw/install.log 2>&1; PREP=; } || PREP=B
cmds=()
for l in $LANGS; do case $l in
  rust)   cmds+=("cargo metadata --format-version 1 --manifest-path $W/Cargo.toml") ;;
  node)   [ -f $W/pnpm-lock.yaml ] || cmds+=("npm --prefix $W ls --all --json --production --dev") ;;
  go)     cmds+=("sh -c 'cd $W && go mod graph'") ;;
  python) [ -f $W/requirements.txt ] && cmds+=("sh -c 'cd $W && uv pip compile --quiet requirements.txt'") ;;
  dotnet) cmds+=("sh -c 'cd $W && dotnet list package --include-transitive'") ;;
esac; done
[ ${#cmds[@]} -eq 0 ] && { echo "no ecosystem tool for $subject"; exit 0; }
hf=(hyperfine --runs ${SAMPLES:-5} --warmup 0 --ignore-failure --export-json $raw/hyperfine.json -N)
[ -n "$PREP" ] && hf+=(--prepare "$H/prep.sh $cell B")
"${hf[@]}" "${cmds[@]}" > $raw/hyperfine.out 2>&1
python3 -I -c "import json;json.dump({'subject':'$subject','scenario':'ecotool-$state','kind':'$KIND','tier':'$TIER','languages':'$LANGS'.split(),'flags':[],'nouv':0,'samples':${SAMPLES:-5},'warmup':0,'cell':'$cell','date':'$(date +%F)'},open('$raw/meta.json','w'))"
chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"
