#!/bin/bash
# run_tool.sh <tool> <subject> <cold|warm>
#   tools: syft trivy cdxgen scancode depscan cargo-deny go-licenses pip-licenses license-checker license_finder
# Same isolation as run_cell.sh. cold = fresh state (tool caches, installed deps removed); warm = deps installed and one priming run done.
H=$(dirname "$0"); source "$H/env.sh"
tool=$1; subject=$2; state=$3
eval "$(python3 -I $H/subj.py shell "$subject")" || exit 2
cell="$(echo "$subject" | tr '/' '_')__tool_${tool}_$state"
raw=$BENCH_ROOT/results/raw/$subject/tool-$tool-$state; mkdir -p "$raw" $BENCH_WORK/cells
S=$BENCH_WORK/state/$cell; W=$BENCH_WORK/work/$cell
chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"; cp -cR "$PRISTINE" "$W" || cp -R "$PRISTINE" "$W"
# never let a tool find a build wrapper of an untrusted clone
for w in mvnw mvnw.cmd .mvn gradlew gradlew.bat gradle/wrapper; do rm -rf "$W/$w"; done
printf 'WORK=%s\nLANGS="%s"\n' "$W" "$LANGS" > $BENCH_WORK/cells/$cell.env
bench_env $cell
OUT=$raw/out
need_install=0
case $tool in
  syft)          cmd="syft scan dir:$W -q -o syft-json=$OUT.json" ;;
  trivy)         cmd="trivy fs --quiet --scanners license --license-full --format json --output $OUT.json $W" ;;
  cdxgen)        # cdxgen sees only node and itself on PATH, so it cannot start mvn, gradle, pip or any build tool
                 RB=$BENCH_WORK/restricted-bin; mkdir -p $RB
                 ln -sf "$(command -v node)" $RB/node; ln -sf "$(command -v cdxgen)" $RB/cdxgen
                 export FETCH_LICENSE=true; cmd="env PATH=$RB:/usr/bin:/bin FETCH_LICENSE=true cdxgen --no-install-deps --no-validate -o $OUT.json $W" ;;
  scancode)      cmd="scancode --license --package --processes 4 --quiet --json-pp $OUT.json $W" ;;
  depscan)       cmd="depscan --src $W --reports-dir $raw/depscan-out --no-banner" ;;
  cargo-deny)    need_install=1; cmd="sh -c 'cd $W && cargo deny list --format json > $OUT.json'" ;;
  go-licenses)   need_install=1; cmd="sh -c 'cd $W && go-licenses report ./... > $OUT.csv'" ;;
  pip-licenses)  need_install=1; cmd="sh -c 'cd $W && .venv/bin/pip-licenses --format=json > $OUT.json'" ;;
  license-checker) need_install=1; cmd="license-checker-rseidelsohn --json --start $W --out $OUT.json" ;;
  license_finder) need_install=1; cmd="sh -c 'cd $W && license_finder report --format json --save $OUT.json'" ;;
  *) echo "unknown tool"; exit 2 ;;
esac
if [ $state = warm ] || [ $need_install = 1 ]; then
  "$H/install.sh" $cell > $raw/install.log 2>&1
  if [ $tool = pip-licenses ]; then
    ( cd $W && uv venv -q .venv && uv pip install -q --python .venv --only-binary :all: -r requirements.txt pip-licenses ) >> $raw/install.log 2>&1
  fi
fi
[ $state = warm ] && PREP= || PREP=AB
[ $need_install = 1 ] && [ $state = cold ] && PREP=A   # tools that need the project installed are timed on top of an installed project
[ $state = warm ] && timeout 1800 sh -c "$cmd" > /dev/null 2>&1   # prime
[ -n "$PREP" ] && "$H/prep.sh" $cell $PREP
FELUDA_REQUEST_STATS=/dev/null /usr/bin/time -l timeout 1800 sh -c "$cmd" > $raw/probe.out 2> $raw/time.raw; echo $? > $raw/exit.txt
grep -E "^ +[0-9.]+ +real|maximum resident" $raw/time.raw > $raw/time.txt; rm -f $raw/time.raw
probe=$(awk '/ real/{print $1}' $raw/time.txt); probe=${probe:-60}
SAMPLES=$(python3 -I -c "t=max($probe,0.05);n=int((${TOOL_BUDGET:-1200}-2*t)//t);print(max(1,min(${SAMPLES:-5},n)))")
hf=(hyperfine --runs $SAMPLES --warmup 0 --ignore-failure --export-json $raw/hyperfine.json -N)
[ -n "$PREP" ] && hf+=(--prepare "$H/prep.sh $cell $PREP")
"${hf[@]}" -- "timeout 1800 $cmd" > $raw/hyperfine.out 2>&1
python3 -I - <<PY
import json
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"tool-$tool-$state","flags":[],"nouv":0,"samples":$SAMPLES,"warmup":0,"cell":"$cell","tool":"$tool","date":"$(date +%F)"}, open("$raw/meta.json","w"), indent=1)
PY
chmod -R u+w "$S" "$W" 2>/dev/null; [ "${KEEP_STATE:-0}" = 1 ] || rm -rf "$S" "$W"
