#!/bin/bash
# run_cell.sh <subject> <scenario> [feluda flags...]
#   scenario: cold | cold-installed | warm-net | warm | offline | unauth-cold
#   pseudo flags (not passed to feluda): +nouv hides uv from PATH
# env: SAMPLES, WARMUP override the defaults; CELL_TIMEOUT per run in seconds (default 3600)
H=$(dirname "$0"); source "$H/env.sh"
SAMPLES_FIXED=${SAMPLES:+1}
subject=$1; scenario=$2; shift 2
flags=(); nouv=0
for f in "$@"; do if [ "$f" = "+nouv" ]; then nouv=1; else flags+=("$f"); fi; done
eval "$(python3 -I $H/subj.py shell "$subject")" || { echo "unknown subject $subject"; exit 2; }   # PRISTINE LANGS TIER KIND
ftag=$(printf '%s' "${flags[*]}" | sed 's/--//g; s/ /_/g'); [ $nouv = 1 ] && ftag="${ftag:+${ftag}_}nouv"
cell="$(echo "$subject" | tr '/' '_')__${scenario}${ftag:+__$ftag}"
raw=$BENCH_ROOT/results/raw/$subject/$scenario${ftag:++$ftag}
mkdir -p "$raw" $BENCH_WORK/cells
case $scenario in
  cold)           SAMPLES=${SAMPLES:-5};  WARMUP=0; PREP=AB; INSTALL=0; PRIME=0 ;;
  cold-installed) SAMPLES=${SAMPLES:-5};  WARMUP=0; PREP=A;  INSTALL=1; PRIME=0 ;;
  warm-net)       SAMPLES=${SAMPLES:-5};  WARMUP=0; PREP=B;  INSTALL=0; PRIME=1 ;;
  warm)           SAMPLES=${SAMPLES:-10}; WARMUP=${WARMUP:-2}; PREP=;  INSTALL=1; PRIME=1 ;;
  offline)        SAMPLES=${SAMPLES:-10}; WARMUP=${WARMUP:-2}; PREP=;  INSTALL=1; PRIME=1 ;;
  unauth-cold)    SAMPLES=${SAMPLES:-3};  WARMUP=0; PREP=AB; INSTALL=0; PRIME=0 ;;
  *) echo "bad scenario"; exit 2 ;;
esac
# L and XL real repos: a 20 minute cap per scan says "too slow" as clearly as an hour does (cut 2026-10-12)
case "$KIND:$TIER" in real:L|real:XL) : ${CELL_TIMEOUT:=1200} ;; esac; : ${CELL_TIMEOUT:=3600}; export CELL_TIMEOUT

# Any other scenario of a subject whose cold scan did not finish is skipped: with no per package cache a
# primed run asks the registries the same questions again (grafana warm showed it), so it would not finish either.
cold_meta=$BENCH_ROOT/results/raw/$subject/cold/meta.json
if [ "$scenario" != cold ] && [ -f "$cold_meta" ] && python3 -I -c "import json,sys; sys.exit(0 if json.load(open('$cold_meta')).get('dnf') else 1)"; then
  mkdir -p "$raw"; echo "$(date '+%F %T') skipped: cold scan of $subject did not finish" > $raw/cell.log; echo 124 > $raw/exit.txt
  python3 -I - <<PY
import json
cold=json.load(open("$cold_meta")); t=float(cold.get("dnf_after_s") or $CELL_TIMEOUT)
m={"time_wall_clock":{"value":t,"unit":"second"},"time_user":{"value":0.0},"time_system":{"value":0.0},"exit_code":124}
json.dump({"results":[{"command":"skipped (cold DNF)","measurements":[m],"summary":{"time_wall_clock":{"mean":t,"median":t},"time_user":{"mean":0.0},"time_system":{"mean":0.0}}}]},open("$raw/hyperfine.json","w"))
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"$scenario","flags":"$*".split(),"nouv":0,"samples":0,"warmup":0,"install":"n/a","cell":"skipped","dnf":True,"dnf_after_s":t,"dnf_stage":"skipped_cold_dnf","date":"$(date +%F)"},open("$raw/meta.json","w"))
PY
  exit 0
fi
S=$BENCH_WORK/state/$cell; W=$BENCH_WORK/work/$cell
case "$S" in $BENCH_WORK/state/*) ;; *) exit 9;; esac
chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"; mkdir -p "$BENCH_WORK/work"
cp -cR "$PRISTINE" "$W" || cp -R "$PRISTINE" "$W"
printf 'WORK=%s\nLANGS="%s"\n' "$W" "$LANGS" > $BENCH_WORK/cells/$cell.env
bench_env $cell
[ $nouv = 1 ] && export PATH=$(echo "$PATH" | tr ':' '\n' | grep -v "/.local/bin" | paste -sd: -)
BIN=$BENCH_BIN
fargs=("${flags[@]}")
case $scenario in
  offline) export HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 https_proxy=http://127.0.0.1:1 http_proxy=http://127.0.0.1:1; fargs+=(--no-clearlydefined) ;;
  unauth-cold) unset GITHUB_TOKEN ;;
esac
scrub() { sed "s/${GITHUB_TOKEN:-__no_token__}/<TOKEN>/g"; }
log() { echo "$(date '+%F %T') $*" >> $raw/cell.log; }
: > $raw/cell.log
log "cell=$cell scenario=$scenario flags=${flags[*]} nouv=$nouv samples=$SAMPLES warmup=$WARMUP prep=$PREP install=$INSTALL prime=$PRIME"

# --- setup (untimed)
online() { # temporarily online for setup of offline cell
  ( export HTTPS_PROXY=$BENCH_PROXY HTTP_PROXY=$BENCH_PROXY https_proxy=$BENCH_PROXY http_proxy=$BENCH_PROXY; "$@" ); }
inst_ok=n/a
if [ $INSTALL = 1 ]; then
  t0=$(date +%s); online "$H/install.sh" $cell > $raw/install.log 2>&1; irc=$?; inst_ok=$([ $irc = 0 ] && echo yes || echo FAILED)
  log "install rc=$irc $(( $(date +%s)-t0 ))s"
fi
if [ $PRIME = 1 ]; then
  t0=$(date +%s); online timeout ${CELL_TIMEOUT:-3600} "$BIN" --path "$W" "${flags[@]}" >/dev/null 2>$raw/prime.err; prc=$?
  log "prime rc=$prc $(( $(date +%s)-t0 ))s"
  if [ "$prc" = 124 ]; then  # the scan exceeds the cap even primed: DNF, no probe, no samples
    echo 124 > $raw/exit.txt
    python3 -I - <<PY
import json
t=float("$CELL_TIMEOUT")
m={"time_wall_clock":{"value":t,"unit":"second"},"time_user":{"value":0.0},"time_system":{"value":0.0},"exit_code":124}
json.dump({"results":[{"command":"prime (timed out)","measurements":[m],"summary":{"time_wall_clock":{"mean":t,"median":t},"time_user":{"mean":0.0},"time_system":{"mean":0.0}}}]},open("$raw/hyperfine.json","w"))
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"$scenario","flags":"${flags[*]}".split(),"nouv":$nouv,"samples":1,"warmup":0,"install":"$inst_ok","cell":"$cell","dnf":True,"dnf_after_s":t,"dnf_stage":"prime","feluda_sha256":"$(shasum -a 256 $BIN | cut -d' ' -f1)","date":"$(date +%F)"},open("$raw/meta.json","w"))
PY
    chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"; log "DNF: prime timed out"; exit 0
  fi
fi
[ -n "$PREP" ] && "$H/prep.sh" $cell $PREP

# --- recorded probe run: /usr/bin/time -l, --json report and request stats in one scan; its wall time sizes the sample count
reprep() { [ -n "$PREP" ] && "$H/prep.sh" $cell $PREP; }
reprep
FELUDA_REQUEST_STATS=$raw/requests.json /usr/bin/time -l timeout ${CELL_TIMEOUT:-3600} "$BIN" --path "$W" "${fargs[@]}" --json > $raw/report.json 2> $raw/time.raw; echo $? > $raw/exit.txt
grep -E "^ +[0-9.]+ +real|maximum resident" $raw/time.raw | scrub > $raw/time.txt
scrub < $raw/time.raw | grep -v "⠋\|⠙\|⠹\|⠸\|⠼\|⠴\|⠦\|⠧\|⠇\|⠏" > $raw/probe.err; rm -f $raw/time.raw
probe=$(awk '/ real/{print $1}' $raw/time.txt); probe=${probe:-60}
log "probe run exit=$(cat $raw/exit.txt) real=${probe}s rows=$(jq length $raw/report.json 2>/dev/null)"
if [ -z "$SAMPLES_FIXED" ]; then
  BUD=${CELL_BUDGET:-1200}
  CAP=$SAMPLES; if awk "BEGIN{exit !($probe > 60)}"; then [ -n "$PREP" ] && CAP=3 || CAP=5; fi
  SAMPLES=$(python3 -I -c "t=max($probe,0.05);n=int((${BUD}-2*t)//t);print(max(1,min($CAP,n)))")
  if awk "BEGIN{exit !($probe > ${SLOW_WARMUP_CUTOFF:-120})}"; then WARMUP=0; fi
fi
log "samples=$SAMPLES warmup=$WARMUP"

if [ "$(cat $raw/exit.txt)" = 124 ]; then  # probe hit the per scan timeout: record DNF, skip the samples and the debug run
  python3 -I - <<PY
import json
t=float("${CELL_TIMEOUT:-3600}")
m={"time_wall_clock":{"value":t,"unit":"second"},"time_user":{"value":0.0},"time_system":{"value":0.0},"exit_code":124}
json.dump({"results":[{"command":"probe (timed out)","measurements":[m],"summary":{"time_wall_clock":{"mean":t,"median":t},"time_user":{"mean":0.0},"time_system":{"mean":0.0}}}]},open("$raw/hyperfine.json","w"))
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"$scenario","flags":"${flags[*]}".split(),"nouv":$nouv,"samples":1,"warmup":0,"install":"$inst_ok","cell":"$cell","dnf":True,"dnf_after_s":t,"feluda_sha256":"$(shasum -a 256 $BIN | cut -d' ' -f1)","date":"$(date +%F)"},open("$raw/meta.json","w"))
PY
  chmod -R u+w "$S" "$W" 2>/dev/null; rm -rf "$S" "$W"; log "DNF: probe timed out"; exit 0
fi
if [ "${ONE_SCAN:-0}" = 1 ]; then  # unauthenticated cells: the probe is the only scan (GitHub allows 60 requests an hour)
  python3 -I - <<PY
import json
t=float("$probe")
m={"time_wall_clock":{"value":t,"unit":"second"},"time_user":{"value":0.0},"time_system":{"value":0.0},"exit_code":int(open("$raw/exit.txt").read())}
json.dump({"results":[{"command":"probe","measurements":[m],"summary":{"time_wall_clock":{"mean":t,"median":t},"time_user":{"mean":0.0},"time_system":{"mean":0.0}}}]},open("$raw/hyperfine.json","w"))
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"$scenario","flags":[],"nouv":0,"samples":1,"warmup":0,"install":"n/a","cell":"$cell","feluda_sha256":"$(shasum -a 256 $BIN | cut -d' ' -f1)","date":"$(date +%F)"},open("$raw/meta.json","w"))
PY
  rm -rf "$S" "$W"; log "one scan cell done"; exit 0
fi

# --- timed: hyperfine
hf=(hyperfine --runs $SAMPLES --warmup $WARMUP --ignore-failure --export-json $raw/hyperfine.json -N)
[ -n "$PREP" ] && hf+=(--prepare "$H/prep.sh $cell $PREP")
export FELUDA_REQUEST_STATS=$raw/requests_last_sample.json
log "hyperfine start"
"${hf[@]}" -- "timeout ${CELL_TIMEOUT:-3600} $BIN --path $W ${fargs[*]}" > $raw/hyperfine.out 2>&1
log "hyperfine rc=$?"

# --- one debug run for coarse phase timing (skipped for slow cells)
if awk "BEGIN{exit !($probe < ${DEBUG_MAX_SECS:-240})}"; then
  reprep
  FELUDA_REQUEST_STATS=/dev/null timeout ${CELL_TIMEOUT:-3600} "$BIN" --path "$W" "${fargs[@]}" --debug 2>&1 | scrub | grep -v '^\[TRACE\]' | head -c 30000000 | ts -s '%.s' > $raw/debug.log
  log "debug run done"
fi

# --- meta
python3 -I - <<PY
import json
json.dump({"subject":"$subject","kind":"$KIND","tier":"$TIER","languages":"$LANGS".split(),"scenario":"$scenario","flags":"${flags[*]}".split(),"nouv":$nouv,
 "samples":$SAMPLES,"warmup":$WARMUP,"install":"$inst_ok","cell":"$cell","feluda_sha256":"$(shasum -a 256 $BIN | cut -d' ' -f1)","date":"$(date +%F)"}, open("$raw/meta.json","w"), indent=1)
PY
chmod -R u+w "$S" "$W" 2>/dev/null
[ "${KEEP_STATE:-0}" = 1 ] || { rm -rf "$S" "$W"; }
log "cell done"
