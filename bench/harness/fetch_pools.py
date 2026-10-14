"""Fetch ranked package pools for the synthetic ladder into corpus/pools/*.json. Run: python3 -I fetch_pools.py"""
import json, os, time, urllib.request, urllib.parse, sys, re, glob

POOLS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "corpus", "pools")
REPOS = os.path.join(os.environ["BENCH_WORK"], "repos")
UA = {"User-Agent": "feluda-bench (anirudhastark@gmail.com)"}

def get(url, tries=4):
    for i in range(tries):
        try:
            req = urllib.request.Request(url, headers=UA)
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read().decode()
        except Exception as e:
            err = e
            time.sleep(2 + i * 3)
    raise err

def save(name, items, meta=""):
    json.dump({"source": meta, "fetched": time.strftime("%Y-%m-%d"), "items": items}, open(f"{POOLS}/{name}.json", "w"), indent=0)
    print(name, len(items))

def ecosystems(reg, n):
    out = []
    page = 1
    while len(out) < n:
        d = json.loads(get(f"https://packages.ecosyste.ms/api/v1/registries/{reg}/packages?sort=downloads&order=desc&per_page=100&page={page}"))
        if not d:
            break
        out += [p["name"] for p in d]
        page += 1
        time.sleep(0.5)
    return out[:n]

os.makedirs(POOLS, exist_ok=True)
want = sys.argv[1:] or ["npm", "pypi", "crates", "nuget", "cran", "gem", "go", "maven"]
if "npm" in want:
    save("npm", ecosystems("npmjs.org", 1300), "packages.ecosyste.ms by downloads")
if "pypi" in want:
    d = json.loads(get("https://hugovk.github.io/top-pypi-packages/top-pypi-packages-30-days.min.json"))
    save("pypi", [r["project"] for r in d["rows"][:1500]], "hugovk top-pypi-packages 30 days")
if "crates" in want:
    out = []
    for page in range(1, 14):
        d = json.loads(get(f"https://crates.io/api/v1/crates?page={page}&per_page=100&sort=downloads"))
        out += [c["name"] for c in d["crates"]]
        time.sleep(1.1)
    save("crates", out, "crates.io by downloads")
if "nuget" in want:
    out = []
    for skip in range(0, 1300, 100):
        d = json.loads(get(f"https://azuresearch-usnc.nuget.org/query?take=100&skip={skip}&sortBy=totalDownloads&prerelease=false"))
        out += [p["id"] for p in d["data"]]
    save("nuget", out, "NuGet search by totalDownloads")
if "cran" in want:
    save("cran", ecosystems("cran.r-project.org", 600), "packages.ecosyste.ms by downloads")
if "gem" in want:
    out = ecosystems("rubygems.org", 800)
    save("gem", out, "packages.ecosyste.ms by downloads")
if "go" in want:
    freq = {}
    for gm in glob.glob(f"{REPOS}/*/go.mod"):
        txt = open(gm).read()
        for m in re.finditer(r"^\s*(?:require\s+)?([\w.\-]+\.[\w.\-]+/[^\s]+)\s+(v[^\s]+)", txt, re.M):
            mod, ver = m.group(1), m.group(2)
            if "// indirect" in txt[m.start():txt.find("\n", m.end())] and False:
                continue
            cur = freq.setdefault(mod, [0, ver])
            cur[0] += 1
            cur[1] = max(cur[1], ver, key=lambda v: [int(x) if x.isdigit() else 0 for x in re.split(r"[.\-+]", v[1:])])
    ranked = sorted(freq.items(), key=lambda kv: (-kv[1][0], kv[0]))
    save("go", [[k, v[1], v[0]] for k, v in ranked], "require lines of the cloned Go repos, by how many repos use the module")
