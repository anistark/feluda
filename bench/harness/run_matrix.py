"""run_matrix.py <phase> [--only substring] [--limit N]  -- run a phase's cells in order, resumable.
Skips cells whose meta.json exists. Updates PROGRESS.md after every cell. Stop conditions:
403/429 in a cell -> that cell's registry hosts are paused for an hour (cells needing them are deferred)."""
import json, os, subprocess, sys, time, datetime
H = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(H)
sys.path.insert(0, H)
HOSTS = {"rust": [], "node": ["registry.npmjs.org"], "go": ["pkg.go.dev"], "python": ["pypi.org"], "java": ["repo1.maven.org", "search.maven.org"],
         "ruby": ["rubygems.org"], "dotnet": ["api.nuget.org"], "r": ["r-universe.dev"], "cpp": ["raw.githubusercontent.com"], "c": []}

def tag(flags):
    f = [x for x in flags if x != "+nouv"]
    t = "_".join(x.replace("--", "").replace(" ", "_") for x in f)
    if "+nouv" in flags: t = (t + "_" if t else "") + "nouv"
    return t

def raw_dir(c):
    t = tag(c["flags"])
    return f"{ROOT}/results/raw/{c['subject']}/{c['scenario']}" + (f"+{t}" if t else "")

def langs(subject):
    s = json.load(open(f"{ROOT}/corpus/subjects.json"))
    s = {x["id"]: x for x in s}[subject]
    return s["languages"]

def progress(phase, cell, idx, total, last, problems, nxt):
    subprocess.run(["python3", "-I", f"{H}/progress.py", "header", f"{phase}", f"{cell} ({idx} of {total} in this phase)", last, problems, nxt])

def main():
    phase = sys.argv[1]
    only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
    limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else 10**9
    cells = json.loads(subprocess.run(["python3", "-I", f"{H}/matrix.py", phase], capture_output=True, text=True).stdout)
    if only: cells = [c for c in cells if only in c["subject"] or only in c["scenario"]]
    todo = [c for c in cells if not os.path.exists(raw_dir(c) + "/meta.json")]
    total = len(cells); paused = {}; problems = "none"; done = 0
    names = {"3": "synthetic ladder", "ladder": "synthetic ladder", "4": "real repos", "real": "real repos",
             "4flags": "flag variants", "flags": "flag variants", "5": "polyglot", "polyglot": "polyglot"}
    queue = list(todo)
    while queue and done < limit:
        now = time.time()
        pick = None
        for c in queue:
            hs = [h for l in langs(c["subject"]) for h in HOSTS.get(l, [])]
            if not any(paused.get(h, 0) > now for h in hs):
                pick = c; break
        if pick is None:
            time.sleep(60); continue
        queue.remove(pick)
        label = f"{pick['subject']} {pick['scenario']}{(' ' + ' '.join(pick['flags'])) if pick['flags'] else ''}"
        idx = total - len(queue)
        progress(names.get(phase, phase), label, idx, total, "see last line of the log", problems, queue[0]["subject"] + " " + queue[0]["scenario"] if queue else "end of phase")
        t0 = time.time()
        env = dict(os.environ)
        r = subprocess.run([f"{H}/run_cell.sh", pick["subject"], pick["scenario"]] + pick["flags"], env=env, capture_output=True, text=True)
        dur = time.time() - t0
        rd = raw_dir(pick)
        res = "no result"
        try:
            meta = json.load(open(rd + "/meta.json")) if os.path.exists(rd + "/meta.json") else {}
            if meta.get("dnf"):  # DNF or skipped cells have no request stats and are not failures
                res = f"{label}: DNF ({meta.get('dnf_stage') or 'timed out'})"
                raise StopIteration
            hf = json.load(open(rd + "/hyperfine.json"))["results"][0]
            ts = sorted(m["time_wall_clock"]["value"] for m in hf["measurements"])
            med = ts[len(ts) // 2]
            res = f"{label}: {med:.2f}s median over {len(ts)} (min {ts[0]:.2f}, max {ts[-1]:.2f}), cell took {dur/60:.1f} min"
            rq = json.load(open(rd + "/requests.json"))
            bad = {h: {s: c for s, c in v["statuses"].items() if s in ("403", "429")} for h, v in rq["hosts"].items()}
            bad = {h: v for h, v in bad.items() if v}
            if bad:
                problems = f"rate limit signals {bad} in {label}"
                subprocess.run(["python3", "-I", f"{H}/progress.py", "log", f"STOP CONDITION 403/429 {bad} in {label}; pausing those hosts for an hour"])
                for h in bad: paused[h] = time.time() + 3600
                for l in langs(pick["subject"]):
                    for h in HOSTS.get(l, []): paused[h] = time.time() + 3600
        except StopIteration:
            pass
        except Exception as e:
            problems = f"cell failed: {label} ({e})"
            subprocess.run(["python3", "-I", f"{H}/progress.py", "log", f"cell failed: {label}: {e}; run_cell stderr tail: {r.stderr[-300:]}"])
        done += 1
        progress(names.get(phase, phase), label + " finished", idx, total, res, problems, queue[0]["subject"] + " " + queue[0]["scenario"] if queue else "end of phase")
        subprocess.run(["python3", "-I", f"{H}/collect.py"], capture_output=True)
        st = os.statvfs(os.environ["BENCH_WORK"]); free = st.f_bavail * st.f_frsize / 1e9
        if free < 20:
            subprocess.run(["python3", "-I", f"{H}/progress.py", "log", f"STOP CONDITION free disk {free:.0f} GB under 20 GB; stopping"]); break

main()
