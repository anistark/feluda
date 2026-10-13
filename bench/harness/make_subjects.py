"""Build corpus/subjects.json and corpus/repos.toml from the clones, the Phase 1 reports and the synthetic tree.
python3 -I make_subjects.py"""
import json, os, glob, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from detect import detect
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORK = os.environ["BENCH_WORK"]
REPOS = os.path.join(WORK, "repos")
rel = lambda d: os.path.relpath(d, WORK)  # subjects.json keeps paths relative to $BENCH_WORK
P1 = f"{ROOT}/results/raw/_phase1"
PRIMARY = {  # repo -> primary language (or "polyglot")
 "sharkdp/hyperfine":"rust","BurntSushi/ripgrep":"rust","astral-sh/uv":"rust","zed-industries/zed":"rust",
 "expressjs/express":"node","excalidraw/excalidraw":"node","facebook/docusaurus":"node","microsoft/vscode":"node",
 "spf13/cobra":"go","gohugoio/hugo":"go","prometheus/prometheus":"go","kubernetes/kubernetes":"go",
 "psf/requests":"python","fastapi/fastapi":"python","apache/superset":"python","home-assistant/core":"python",
 "spring-projects/spring-petclinic":"java","mybatis/mybatis-3":"java","netty/netty":"java","jenkinsci/jenkins":"java",
 "jekyll/jekyll":"ruby","rubocop/rubocop":"ruby","rails/rails":"ruby","discourse/discourse":"polyglot",
 "dotnet/eShop":"dotnet","dotnet/BenchmarkDotNet":"dotnet","Humanizr/Humanizer":"dotnet","restsharp/RestSharp":"dotnet",
 "tidyverse/ggplot2":"r","rstudio/r-manuals":"r",
 "curl/curl":"c","mapnik/mapnik":"cpp","KhronosGroup/OpenCL-SDK":"cpp",
 "mastodon/mastodon":"polyglot","tauri-apps/tauri":"polyglot","grafana/grafana":"polyglot","getsentry/sentry":"polyglot","rails/rails#":"x",
}
LANGMAP = {"cargo":"rust","npm":"node","golang":"go","pypi":"python","maven":"java","gem":"ruby","nuget":"dotnet","cran":"r"}
def tier(n):
    return "S" if n < 50 else "M" if n < 300 else "L" if n < 1500 else "XL"
subjects = []
# Without a scan_once.sh report, a repository keeps the size it already has in subjects.json
previous = {s["id"]: s for s in json.load(open(f"{ROOT}/corpus/subjects.json"))} if os.path.exists(f"{ROOT}/corpus/subjects.json") else {}
toml = []
for repo, prim in PRIMARY.items():
    if repo.endswith("#"): continue
    rid = repo.replace("/", "__")
    d = f"{REPOS}/{rid}"
    if not os.path.isdir(d): continue
    rep = f"{P1}/{rid}/report.json"
    rows = None; eco = {}
    if os.path.exists(rep):
        try:
            data = json.load(open(rep)); rows = len(data)
            for r in data: eco[r.get("ecosystem")] = eco.get(r.get("ecosystem"), 0) + 1
        except Exception: pass
    sha = subprocess.run(["git", "-C", d, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    langs = detect(d)
    name = {"home-assistant/core": "home-assistant"}.get(repo, repo.split("/")[1])
    sid = f"{prim}/{name}"
    if rows is None and sid in previous:
        rows, eco = previous[sid].get("rows"), previous[sid].get("ecosystems", {})
    subjects.append({"id": sid, "kind": "real", "repo": repo, "sha": sha, "dir": rel(d), "primary": prim, "languages": langs,
                     "rows": rows, "ecosystems": eco, "tier": tier(rows) if rows is not None else None})
    toml.append(f'[[repo]]\nname = "{repo}"\nurl = "https://github.com/{repo}"\nsha = "{sha}"\nlanguages = {json.dumps(langs)}\nsubject = "{sid}"\nrows = {rows if rows is not None else -1}\ntier = "{tier(rows) if rows is not None else "?"}"\n')
for d in sorted(glob.glob(f"{WORK}/synthetic/polyglot/*")):
    name = os.path.basename(d)
    subjects.append({"id": f"synthetic/polyglot/{name}", "kind": "synthetic-polyglot", "dir": rel(d), "primary": "polyglot", "languages": detect(d), "rung": 100, "rows": None, "tier": "M"})
for d in sorted(glob.glob(f"{WORK}/synthetic/*/n*")):
    lang = d.split("/")[-2]; n = int(d.split("/n")[-1])
    if not os.listdir(d): continue
    subjects.append({"id": f"synthetic/{lang}/n{n}", "kind": "synthetic", "dir": rel(d), "primary": lang, "languages": detect(d), "rung": n, "rows": None, "tier": f"n{n}"})
ov = json.load(open(f"{ROOT}/corpus/tier_overrides.json"))
for s in subjects:
    if s["id"] in ov: s["tier"] = ov[s["id"]]; s["tier_note"] = "set from go mod graph size; the Phase 1 scan was too slow to finish"
json.dump(subjects, open(f"{ROOT}/corpus/subjects.json", "w"), indent=1)
open(f"{ROOT}/corpus/repos.toml", "w").write("\n".join(toml))
print(len(subjects), "subjects")
