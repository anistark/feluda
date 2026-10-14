"""Other tools on the same corpus, one run at a time, resumable. python3 -I run_tools.py"""
import json, os, subprocess, sys
H = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(H)
S = {s["id"]: s for s in json.load(open(f"{ROOT}/corpus/subjects.json"))}
real = [s for s in S.values() if s["kind"] == "real" and s.get("tier")]
cells = []
for s in sorted(real, key=lambda s: ({"S": 0, "M": 1, "L": 2, "XL": 3}[s["tier"]], s["id"])):
    for t in ("syft", "trivy"):
        cells.append((t, s["id"], "cold"))
    if s["tier"] == "S" or s["id"] in ("rust/hyperfine", "node/express", "python/fastapi", "c/curl"):
        cells.append(("cdxgen", s["id"], "cold")); cells.append(("scancode", s["id"], "cold")) if s["tier"] == "S" else None
for sid in ("rust/hyperfine", "rust/ripgrep", "rust/uv"):
    for st in ("cold", "warm"): cells.append(("cargo-deny", sid, st))
for sid in ("go/cobra", "go/hugo"):
    for st in ("cold", "warm"): cells.append(("go-licenses", sid, st))
for sid in ("synthetic/python/n10", "synthetic/python/n100"):
    for st in ("cold", "warm"): cells.append(("pip-licenses", sid, st))
for sid in ("node/express", "node/vscode", "synthetic/node/n100"):
    for st in ("cold", "warm"): cells.append(("license-checker", sid, st))
def done(c):
    return os.path.exists(f"{ROOT}/results/raw/{c[1]}/tool-{c[0]}-{c[2]}/meta.json")
for i, c in enumerate(cells):
    if done(c): continue
    subprocess.run(["python3", "-I", f"{H}/progress.py", "header", "7 (other tools)", f"{c[0]} on {c[1]} {c[2]} ({i+1} of {len(cells)})", "see log", "none", "next in list"])
    subprocess.run([f"{H}/run_tool.sh", *c], capture_output=True)
    d = f"{ROOT}/results/raw/{c[1]}/tool-{c[0]}-{c[2]}"
    try:
        ex = open(d + "/exit.txt").read().strip(); hf = json.load(open(d + "/hyperfine.json"))["results"][0]["measurements"]
        r = f"{c[0]} {c[1]} {c[2]}: exit {ex}, median {sorted(m['time_wall_clock']['value'] for m in hf)[len(hf)//2]:.1f}s over {len(hf)}"
    except Exception as e:
        r = f"{c[0]} {c[1]} {c[2]}: no result ({e})"
    subprocess.run(["python3", "-I", f"{H}/progress.py", "log", "phase 7 " + r])
open(f"{ROOT}/results/run_tools.done", "w").write("done")
