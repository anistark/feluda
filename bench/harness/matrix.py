"""matrix.py <phase> -> JSON list of cells [{subject, scenario, flags, env}] in run order.
phases: ladder (synthetic ladder), real (real repos), flags (opt out flags on M tier repos), polyglot.
The study's numbers 3, 4, 4flags and 5 still work. Runs without a token are unauth_rounds.sh."""
import json, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
S = {s["id"]: s for s in json.load(open(f"{ROOT}/corpus/subjects.json"))}
# Trimmed on 2026-10-11 after the run time estimate (see PLAN.md "Trim"): fast languages keep the full ladder,
# slow sequential ones stop at 250 (go, python, ruby, r, cpp, dotnet), java/gradle stay at 10, c at 10 and 100.
LADDER = {"rust": [10,50,100,250,500,1000], "node": [10,50,100,250,500,1000],
          "python": [10,100,250], "go": [10,100,250], "dotnet": [10,100,250], "ruby": [10,100], "r": [10,100], "cpp": [10,100],
          "java": [10], "c": [10,100]}
FULL_LANGS = {"rust", "node", "python"}  # also get cold-installed, warm-net and offline, at n100
def phase3():
    cells = []
    rungs = sorted({n for v in LADDER.values() for n in v})
    for n in rungs:
        for lang, ns in LADDER.items():
            sid = f"synthetic/{lang}/n{n}"
            if n not in ns or sid not in S: continue
            sc = ["cold", "warm"]
            if n == 100 and lang in FULL_LANGS: sc = ["cold", "cold-installed", "warm-net", "warm", "offline"]
            for s in sc:
                cells.append({"subject": sid, "scenario": s, "flags": []})
    cells += [{"subject": "synthetic/python/n100", "scenario": sc, "flags": ["+nouv"]} for sc in ("cold", "warm")]
    return cells
def phase4():
    cells = []
    for sid, s in S.items():
        if s["kind"] != "real" or s["primary"] == "polyglot" or s.get("tier") is None: continue
        if s["primary"] == "java" and sid != "java/mybatis-3": continue  # search.maven.org hangs make each scan 15+ min; Phase 1 single scans are the Java real repo data
        sc = ["cold", "warm"]
        if s["tier"] == "M" and s["primary"] in ("rust", "node", "python"): sc = ["cold", "cold-installed", "warm-net", "warm"]
        for x in sc: cells.append({"subject": sid, "scenario": x, "flags": []})
    return cells
def phase4flags():
    cells = []
    for sid, s in S.items():
        if s["kind"] == "real" and s.get("tier") == "M" and s["primary"] in ("rust", "node", "python"):
            for sc in ("warm", "cold"):
                for fl in (["--no-local"], ["--no-clearlydefined"], ["--no-vendor-scan"]):
                    if sc == "cold": continue
                    cells.append({"subject": sid, "scenario": sc, "flags": fl})
            cells.append({"subject": sid, "scenario": "warm", "flags": ["--github-token-off"]})
    return [c for c in cells if c["flags"] != ["--github-token-off"]]
def phase5():
    cells = []
    for name in ("p2", "p3", "p5", "pall"):
        for x in ("cold", "warm"): cells.append({"subject": f"synthetic/polyglot/{name}", "scenario": x, "flags": []})
    for sid, s in S.items():
        if s["primary"] == "polyglot" and s["kind"] == "real" and s.get("tier") is not None:
            for x in ("cold", "warm"): cells.append({"subject": sid, "scenario": x, "flags": []})
    return cells
if __name__ == "__main__":
    PHASES = {"ladder": phase3, "real": phase4, "flags": phase4flags, "polyglot": phase5,
              "3": phase3, "4": phase4, "4flags": phase4flags, "5": phase5}
    print(json.dumps(PHASES[sys.argv[1]](), indent=0))
