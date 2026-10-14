"""subj.py shell <id>  -> shell assignments for run_cell.sh. subj.py list [filter]
A subject's dir is relative to $BENCH_WORK: repos/<owner>__<name> or synthetic/<lang>/n<N>."""
import os, json, sys, shlex
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
S = json.load(open(f"{ROOT}/corpus/subjects.json"))
if sys.argv[1] == "shell":
    m = [s for s in S if s["id"] == sys.argv[2]]
    if not m: sys.exit(1)
    s = m[0]
    langs = [l for l in s["languages"] if l != "c" or s["primary"] == "c"]
    print(f"PRISTINE={shlex.quote(os.path.join(os.environ['BENCH_WORK'], s['dir']))} LANGS={shlex.quote(' '.join(langs))} TIER={s.get('tier')} KIND={s['kind']}")
else:
    for s in S:
        if len(sys.argv) < 3 or sys.argv[2] in s["id"]:
            print(s["id"], s.get("tier"), s.get("rows"))
