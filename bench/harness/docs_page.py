"""Fill report/performance.rst.tmpl from results/ and write docs/source/performance.rst, with its charts.
python3 -I docs_page.py   (run collect.py, tools_summary.py and charts.py first)"""
import json, math, collections, os, re, shutil

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(os.path.dirname(ROOT), "docs", "source", "performance.rst")
# runs.jsonl also carries the other tools' runs (scenario tool-*); those come from tools.jsonl here.
rows = [r for r in map(json.loads, open(f"{ROOT}/results/runs.jsonl")) if not r["scenario"].startswith("tool-")]
tools = [json.loads(l) for l in open(f"{ROOT}/results/tools.jsonl")]
subjects = {s["id"]: s for s in json.load(open(f"{ROOT}/corpus/subjects.json"))}

LANG = {"rust": "Rust", "node": "Node", "python": "Python", "go": "Go", "dotnet": ".NET", "ruby": "Ruby",
        "r": "R", "cpp": "C++", "c": "C", "java": "Java"}
ORDER = ["rust", "node", "python", "go", "dotnet", "ruby", "r", "cpp", "c", "java"]


def secs(x):
    if x is None:
        return ""
    return f"{x:.2f}" if x < 10 else f"{x:.1f}" if x < 100 else f"{x:.0f}"


def ok(r):
    return r and r.get("median_s") is not None and not r.get("dnf")


def table_rows(cells, indent=3):
    pad = " " * indent
    out = []
    for cell in cells:
        out.append(f"{pad}* - {cell[0]}".rstrip())
        out.extend(f"{pad}  - {c}".rstrip() for c in cell[1:])
    return "\n".join(out)


def by(subject_filter):
    d = collections.defaultdict(dict)
    for r in rows:
        if subject_filter(r) and not r.get("nouv"):
            d[r["subject"]][(r["scenario"], tuple(r["flags"]))] = r
    return d


def deps(r):
    return r.get("deps_unique") or r.get("deps_total")


# Synthetic ladder
syn = by(lambda r: r["subject"].startswith("synthetic/") and "polyglot" not in r["subject"])
rung = lambda k: int(k.split("/n")[-1])
lang_of = lambda k: k.split("/")[1]

ladder_rows, ladder_cells = [], []
ladder_fit, ladder_top = {}, {}
for lang in ORDER:
    ks = sorted((k for k in syn if lang_of(k) == lang), key=rung)
    if not ks:
        continue
    fit = {}
    for sc in ("cold", "warm"):
        pts = [(deps(syn[k][(sc, ())]), syn[k][(sc, ())]["median_s"]) for k in ks if ok(syn[k].get((sc, ())))]
        if not pts:
            continue
        top = max(pts)
        exp = None
        if len(pts) >= 2:
            xs = [math.log(p[0]) for p in pts]
            ys = [math.log(p[1]) for p in pts]
            mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
            den = sum((x - mx) ** 2 for x in xs)
            exp = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den if den else None
        fit[sc] = (top[1] / top[0], exp)
    ladder_fit[lang] = fit
    ladder_top[lang] = (rung(ks[-1]), ks[-1])
    big = syn[ks[-1]].get(("cold", ())) or syn[ks[-1]].get(("warm", ()))
    ladder_rows.append([
        LANG[lang],
        f"{fit['cold'][0]:.3f}" if "cold" in fit else "",
        f"{fit['warm'][0]:.3f}" if "warm" in fit else "",
        f"{fit['cold'][1]:.2f}" if fit.get("cold", (0, None))[1] is not None else "",
        f"{fit['warm'][1]:.2f}" if fit.get("warm", (0, None))[1] is not None else "",
        f"{rung(ks[-1])} direct, {deps(big)} packages",
    ])
    for k in ks:
        d = syn[k]
        cell = lambda sc: secs(d[(sc, ())]["median_s"]) if ok(d.get((sc, ()))) else ""
        failed = [sc for (sc, _), r in d.items() if r.get("install") == "FAILED"]
        anyr = d.get(("cold", ())) or next(iter(d.values()))
        ladder_cells.append([LANG[lang], str(rung(k)), str(deps(anyr) or ""), cell("cold"), cell("cold-installed"),
                             cell("warm-net"), cell("warm"), cell("offline"),
                             ("install failed: " + ", ".join(failed)) if failed else ""])

ladder_table = "   .. list-table::\n      :header-rows: 1\n\n" + table_rows(
    [["Language", "Direct", "Packages", "cold", "cold-installed", "warm-net", "warm", "offline", "Notes"]]
    + ladder_cells, indent=6)

# Scenarios at n100
scen_rows = []
for sc, label in (("cold", "``cold``"), ("cold-installed", "``cold-installed``"), ("warm-net", "``warm-net``"),
                  ("warm", "``warm``"), ("offline", "``offline``")):
    scen_rows.append([label] + [secs(syn[f"synthetic/{l}/n100"][(sc, ())]["median_s"])
                                if ok(syn[f"synthetic/{l}/n100"].get((sc, ()))) else ""
                                for l in ("rust", "node", "python")])

# Polyglot
poly_rows = []
single = lambda l, sc: (syn.get(f"synthetic/{l}/n100") or syn.get(f"synthetic/{l}/n10") or {}).get((sc, ()))
for sc in ("cold", "warm"):
    for p in ("p2", "p3", "p5", "pall"):
        s = f"synthetic/polyglot/{p}"
        r = next((x for x in rows if x["subject"] == s and x["scenario"] == sc and not x["flags"]), None)
        langs = subjects[s]["languages"]
        singles = [single(l, sc) for l in langs]
        times = [x["median_s"] for x in singles if ok(x)]
        name = "All ten" if p == "pall" else ", ".join(LANG[l] for l in langs)
        poly_rows.append([name, sc, secs(r["median_s"]) if ok(r) else "did not finish",
                          secs(sum(times)), secs(max(times))])

# Real repositories
real = by(lambda r: subjects.get(r["subject"], {}).get("kind") == "real")
real_rows, real_poly = [], []
for k in sorted(real, key=lambda k: (ORDER.index(subjects[k]["primary"]) if subjects[k].get("primary") in ORDER else 99, k)):
    s = subjects[k]
    c, w = real[k].get(("cold", ())), real[k].get(("warm", ()))
    if not c:
        continue
    name = f"`{k.split('/')[1]} <https://github.com/{s['repo']}>`__"
    # A Makefile used to count as C (the analyzer then reported the host's libraries); it reports nothing now.
    langs = " + ".join(LANG[l] for l in s["languages"] if l in LANG and (l != "c" or s["primary"] == "c"))
    notes = []
    if c.get("dnf"):
        cold = f"over {int(c['dnf_after_s']) // 60} min"
    else:
        cold = secs(c["median_s"])
    if not w:
        warm = ""
    elif w.get("dnf_stage") == "skipped_cold_dnf":
        warm = "skipped"
    elif w.get("dnf"):
        warm = f"over {int(w['dnf_after_s']) // 60} min"
    else:
        warm = secs(w["median_s"])
    if w and w.get("install") == "FAILED":
        notes.append("install failed")
    if k.startswith("java/"):
        notes.append("Maven Central hangs")
    ratio = f"{c['median_s'] / w['median_s']:.1f}x" if ok(c) and ok(w) else ""
    peak = c.get("peak_rss_mb")
    real_rows.append([f"{name} ({langs})", s["tier"], str(deps(c) or ""), cold, warm, ratio,
                      f"{peak:.0f}" if peak else "", ", ".join(notes)])
    if k.startswith("polyglot/") and k != "polyglot/grafana" and ok(c) and ok(w):
        real_poly.append(f"{k.split('/')[1]} ({langs}) {secs(c['median_s'])} s, then {secs(w['median_s'])} s")
real_poly_text = "; ".join(real_poly) + "; grafana (Go + Node) did not finish within an hour"

# Flags
flag_cells = []
for k, d in sorted(real.items()):
    base = d.get(("warm", ()))
    for (sc, fl), r in sorted(d.items()):
        if sc == "warm" and fl and ok(r) and ok(base):
            flag_cells.append([k.split("/")[1], f"``{' '.join(fl)}``", secs(r["median_s"]),
                               secs(base["median_s"]), f"{r['median_s'] / base['median_s']:.2f}"])
flag_table = ".. list-table:: Median seconds, repeat run\n   :header-rows: 1\n   :widths: 25 30 15 15 15\n\n" + table_rows(
    [["Repository", "Flag", "With flag", "Default", "Ratio"]] + flag_cells)

# Tools
of_tool = lambda t: [x for x in tools if x["tool"] == t]
syft_sampled = sum(x.get("sampled", 0) for x in of_tool("syft"))
syft_with = syft_sampled - sum(x.get("tool_no_license", 0) for x in of_tool("syft"))
trivy_failed = sorted({x["subject"].split("/")[1] for x in of_tool("trivy") if str(x.get("exit")) not in ("0", "None")})
cdx_slow = max((x for x in of_tool("cdxgen") if x.get("median_s") is not None), key=lambda x: x["median_s"])
scan_peak = max(x.get("peak_rss_mb") or 0 for x in of_tool("scancode")) / 1024
go_version = re.search(r"go(\d+\.\d+)", json.load(open(f"{ROOT}/results/env.json")).get("go", ""))
WHAT = {
    "syft": f"Catalogues packages from lockfiles and installed files without the network; it does not look licenses up, so {syft_with} of {syft_sampled} sampled packages carried one.",
    "trivy": "Reports licenses found in files and installed metadata, not lockfile packages."
             + (f" Exited with an error on {' and '.join(trivy_failed)} when Maven Central rate limited this address." if trivy_failed else ""),
    "cdxgen": f"Run with ``--no-install-deps`` so it reads manifests like Feluda does. Fetches licenses from registries, one package at a time ({secs(cdx_slow['median_s'])} s on {cdx_slow['subject'].split('/')[1]}).",
    "scancode": f"Scans every file's content for license text, so it answers which licenses appear in the tree, not which ones dependencies declare. Up to {scan_peak:.1f} GB of memory.",
    "cargo-deny": "Reads installed Cargo metadata. Rust only, needs ``cargo fetch`` first.",
    "license-checker": "Reads installed ``node_modules``. Node only, needs an install first.",
    "pip-licenses": "Reads an installed Python environment. Python only, needs an install first.",
    "go-licenses": f"Did not run: it fails on Go {go_version.group(1) if go_version else 'this version'} (\"Package math does not have module info\").",
}
tool_rows = []
# Tools that read installed packages are compared with Feluda's repeat run, which has them installed too.
STATE = {"cargo-deny": "warm", "license-checker": "warm", "pip-licenses": "warm"}
feluda_at = {(r["subject"], r["scenario"]): r["median_s"] for r in rows if not r["flags"] and ok(r)}
med = lambda xs: sorted(xs)[len(xs) // 2] if xs else None
for t in ("syft", "trivy", "cdxgen", "scancode", "cargo-deny", "license-checker", "pip-licenses", "go-licenses"):
    st = STATE.get(t, "cold")
    ts = [x for x in tools if x["tool"] == t and x["state"] == st and x.get("median_s") is not None
          and not x.get("dnf") and str(x.get("exit")) == "0"]
    same = [feluda_at[(x["subject"], st)] for x in ts if (x["subject"], st) in feluda_at]
    tool_rows.append([t, str(len(ts)), secs(med([x["median_s"] for x in ts])),
                      f"{secs(med(same))} ({'repeat' if st == 'warm' else 'first'} run)" if same else "", WHAT[t]])

def label(subject):
    if subject.startswith("synthetic/"):
        return f"synthetic, {subject.rsplit('/n', 1)[1]} direct"
    return subject.split("/")[1]


# Memory, from the recorded probe run of every feluda cell
peaks = [r["peak_rss_mb"] for r in rows if r.get("peak_rss_mb")]
fam = lambda r: r["subject"].split("/")[1] if r["subject"].startswith("synthetic/") and "polyglot" not in r["subject"] else r["subject"].split("/")[0]
top = {}
for r in rows:
    if r.get("peak_rss_mb") and (fam(r) not in top or r["peak_rss_mb"] > top[fam(r)][0]):
        top[fam(r)] = (r["peak_rss_mb"], r["subject"])
memory = (f"Peak resident memory of the recorded run of each cell: median {med(peaks):.0f} MB, largest {max(peaks):.0f} MB. "
          "Largest per language: " + ", ".join(
              f"{LANG.get(l, 'several languages')} {top[l][0]:.0f} MB ({label(top[l][1])})"
              for l in ORDER + ["polyglot"] if l in top) + ".")

cells = sum(1 for r in rows if r.get("dnf_stage") != "skipped_cold_dnf")

# Every number in the template's prose, computed here so a rerun refreshes the whole page.
env = json.load(open(f"{ROOT}/results/env.json"))
WORDS = {2: "two", 3: "three", 4: "four", 5: "five", 6: "six", 7: "seven", 8: "eight", 9: "nine", 10: "ten"}
word = lambda n: WORDS.get(n, str(n))
p = lambda x: f"{x:.1f}" if x < 10 else f"{x:,.0f}"  # seconds in prose: 0.5, 8.6, 12, 271
times = lambda x: f"{x:.1f}" if x < 3 else f"{x:.0f}"
cap_text = lambda s: "an hour" if round(s / 60) == 60 else f"{round(s / 60)} minutes"
row = lambda s, sc, fl=(): (syn.get(s) or real.get(s) or {}).get((sc, fl))
nouv = lambda s, sc: next((r for r in rows if r["subject"] == s and r["scenario"] == sc and r.get("nouv") and not r["flags"]), None)
cpu = lambda r: (r.get("user_s") or 0) + (r.get("sys_s") or 0)
short = lambda k: k.split("/")[1]
joined = lambda xs: xs[0] if len(xs) == 1 else ", ".join(xs[:-1]) + " and " + xs[-1]

def ratio_n100(lang):
    c, w = row(f"synthetic/{lang}/n100", "cold"), row(f"synthetic/{lang}/n100", "warm")
    return c["median_s"] / w["median_s"] if ok(c) and ok(w) else None

F = {}
F["VERSION"] = re.search(r"\d+\.\d+\.\d+", env.get("feluda_version", "")).group(0)
F["REAL_COUNT"] = str(sum(1 for s in subjects.values() if s["kind"] == "real"))
rungs = sorted({rung(k) for k in syn})
F["RUNG_MIN"], F["RUNG_MAX"] = f"{rungs[0]:,}", f"{rungs[-1]:,}"
F["LANG_COUNT"] = word(len(ladder_fit))

# Rust: the largest rung's repeat run, and the largest real Rust repository
rust_top = row(ladder_top["rust"][1], "warm")
F["RUST_TOP_DEPS"], F["RUST_TOP_WARM"] = f"{deps(rust_top):,}", p(rust_top["median_s"])
rust_repo = max((k for k in real if subjects[k]["primary"] == "rust" and ok(row(k, "cold")) and ok(row(k, "warm"))),
                key=lambda k: deps(row(k, "cold")))
F["RUST_REPO"] = short(rust_repo)
F["RUST_REPO_COLD"], F["RUST_REPO_WARM"] = p(row(rust_repo, "cold")["median_s"]), p(row(rust_repo, "warm")["median_s"])

# Languages that look each dependency up: cost per dependency on a first run, at the largest rung
LOOKUP = ["node", "python", "go", "dotnet", "r", "cpp"]
per_dep = {l: ladder_fit[l]["cold"][0] for l in LOOKUP if "cold" in ladder_fit.get(l, {})}
F["PER_DEP_MIN"], F["PER_DEP_MAX"] = f"{min(per_dep.values()):.1f}", f"{max(per_dep.values()):.1f}"
slowest = max(per_dep, key=per_dep.get)
F["SLOWEST_LANG"], F["SLOWEST_PER_DEP"] = LANG[slowest], f"{per_dep[slowest]:.1f}"
F["GO_PER_DEP"] = f"{per_dep['go']:.1f}"
go_top = row(ladder_top["go"][1], "warm")
F["GO_TOP_RUNG"], F["GO_TOP_WARM_REQS"] = f"{ladder_top['go'][0]:,}", str(go_top["requests"])

# First run over repeat run, at 100 direct dependencies
for l in ("node", "dotnet", "python", "go", "r", "cpp"):
    F[f"RATIO_{l.upper()}"] = times(ratio_n100(l))
F["REPEAT_MAX"] = times(max(ratio_n100(l) for l in ("go", "python", "r", "cpp")))

# Memory: a runner with twice the largest peak, rounded up to a power of two
F["RUNNER_GB"] = str(2 ** math.ceil(math.log2(max(peaks) * 2 / 1024)))

# Scans that hit the cap
dnf = [k for k in real if (row(k, "cold") or {}).get("dnf")]
groups = collections.defaultdict(list)
for k in sorted(dnf):
    s = subjects[k]
    name = short(k) + (f" ({' and '.join(LANG[l] for l in s['languages'] if l in LANG and l != 'c')})" if s["primary"] == "polyglot" else "")
    groups[row(k, "cold")["dnf_after_s"]].append(name)
parts = [f"{joined(names)} {'did not finish within' if i == 0 else 'not within'} {cap_text(cap)}"
         for i, (cap, names) in enumerate(sorted(groups.items()))]
F["DNF_SENTENCE"] = ("Fresh scans of " + ", and ".join(parts) + ".") if parts else "Every repository finished within its cap."
go_dnf = [short(k) for k in sorted(dnf) if subjects[k]["primary"] == "go"]
F["GO_DNF"] = (joined(go_dnf) + " exceed " + cap_text(row(f"go/{go_dnf[0]}", "cold")["dnf_after_s"])) if go_dnf else "large Go repositories take minutes"

# Machine and method
F["MACHINE"] = f"{env['cpu']}, {env['cores']} cores, {env['mem_gb']} GB, {env['os'].rsplit(' ', 1)[0]}"
F["MAX_SAMPLES"] = str(max(r.get("samples") or 0 for r in rows))
java_top = row(ladder_top["java"][1], "cold") or row(ladder_top["java"][1], "warm")
F["JAVA_RUNG"], F["JAVA_DEPS"] = str(ladder_top["java"][0]), f"{deps(java_top):,}"

# The five states at 100 direct dependencies
for l in ("rust", "node", "python"):
    st = {sc: row(f"synthetic/{l}/n100", sc)["median_s"] for sc in ("cold-installed", "warm-net", "warm")}
    F[f"CACHE_SAVE_{l.upper()}"] = f"{st['cold-installed'] - st['warm']:.1f}"
    F[f"INSTALL_SAVE_{l.upper()}"] = f"{st['warm-net'] - st['warm']:.1f}"
F["OFFLINE_RUST"] = p(row("synthetic/rust/n100", "offline")["median_s"])
py_off = row("synthetic/python/n100", "offline")
F["OFFLINE_PY"], F["OFFLINE_PY_DEPS"] = p(py_off["median_s"]), str(deps(py_off))
F["OFFLINE_NODE"] = p(row("synthetic/node/n100", "offline")["median_s"])
F["UV_DEPS"] = str(row("synthetic/python/n100", "cold")["deps_total"])
nouv_cold = nouv("synthetic/python/n100", "cold")
F["NOUV_DEPS"], F["NOUV_REQS"] = str(nouv_cold["deps_total"]), str(nouv_cold["requests"])

# Flags: the spread of the opt out flags, and --no-vendor-scan on the largest tree
dev = [abs(r["median_s"] / real[k][("warm", ())]["median_s"] - 1) for k in real for (sc, fl), r in real[k].items()
       if sc == "warm" and fl and fl != ("--no-vendor-scan",) and ok(r) and ok(real[k].get(("warm", ())))]
F["FLAG_SPREAD"] = f"{round(max(dev) * 100):.0f}"
nvs = [k for k in real if ok(row(k, "warm", ("--no-vendor-scan",))) and ok(row(k, "warm"))]
big_tree = max(nvs, key=lambda k: deps(row(k, "warm")))
F["NVS_REPO"] = short(big_tree)
F["NVS_DEFAULT"], F["NVS_FLAG"] = p(row(big_tree, "warm")["median_s"]), p(row(big_tree, "warm", ("--no-vendor-scan",))["median_s"])

# CPU against wall time
go100, node_top = row("synthetic/go/n100", "warm"), row(ladder_top["node"][1], "warm")
F["GO_CPU"], F["GO_WALL"] = p(cpu(go100)), p(go100["median_s"])
F["NODE_CPU"], F["NODE_WALL"], F["NODE_RUNG"] = p(cpu(node_top)), p(node_top["median_s"]), f"{ladder_top['node'][0]:,}"

# Without a token
unauth = [r for r in rows if r["scenario"] == "unauth-cold"]
gh = [r["request_hosts"].get("api.github.com", {}) for r in unauth if r.get("request_hosts")]
gh_n = sorted(h.get("n", 0) for h in gh if h.get("n"))
refused = sum(n for h in gh for st, n in h.get("statuses", {}).items() if st in ("403", "429"))
F["UNAUTH_ROUNDS_CAP"] = word(max(collections.Counter(r["subject"] for r in unauth).values())).capitalize()
F["UNAUTH_SUBJECTS"] = joined([f"{short(s)} ({LANG[subjects[s]['primary']]})" for s in dict.fromkeys(r["subject"] for r in unauth)])
F["GH_MIN"], F["GH_MAX"] = str(gh_n[0]), str(gh_n[-1])
F["RUNS_PER_HOUR"] = word(60 // gh_n[0])
F["GH_REFUSED"] = "No request was refused." if not refused else f"{refused} requests were refused."

# Agreement with other tools, once both are written as SPDX ids
def agreement(t):
    a = sum(x.get("agree", 0) for x in tools if x["tool"] == t)
    d = sum(x.get("disagree", 0) for x in tools if x["tool"] == t)
    n = sum(x.get("tool_no_license", 0) for x in tools if x["tool"] == t)
    # "every package it named" when the tool left some packages without a license and agreed on the rest
    return f"{t} every package it named" if not d and n else f"{t} {a} of {a + d}"
F["AGREEMENT"] = ", ".join(agreement(t) for t in ("trivy", "cargo-deny", "license-checker", "pip-licenses", "cdxgen"))

# Express: what is reported without and with node_modules
F["EXPRESS_COLD_DEPS"] = str(row("node/express", "cold")["deps_total"])
F["EXPRESS_WARM_DEPS"] = str(row("node/express", "warm")["deps_total"])

# Node's first run cost per package on the larger rungs
node_big = [row(k, "cold") for k in syn if lang_of(k) == "node" and rung(k) >= 250 and ok(row(k, "cold"))]
node_cost = [r["median_s"] / deps(r) for r in node_big]
F["NODE_PER_PKG_MIN"], F["NODE_PER_PKG_MAX"] = f"{min(node_cost):.2g}", f"{max(node_cost):.2g}"

t = open(f"{ROOT}/report/performance.rst.tmpl").read()
for key, val in {
    "CELLS": str(cells),
    "MEMORY": memory,
    "MEM_MEDIAN": f"{med(peaks):.0f}",
    "MEM_MAX": f"{max(peaks):.0f}",
    "LADDER_ROWS": table_rows(ladder_rows),
    "LADDER_TABLE": ladder_table,
    "SCENARIO_ROWS": table_rows(scen_rows),
    "POLYGLOT_ROWS": table_rows(poly_rows),
    "REAL_POLYGLOT": real_poly_text,
    "REAL_ROWS": table_rows(real_rows),
    "FLAG_ROWS": flag_table,
    "TOOL_ROWS": table_rows(tool_rows),
    **F,
}.items():
    t = t.replace(f"@@{key}@@", val)
assert "@@" not in t, re.findall(r"@@\w+@@", t)
open(DOCS, "w").write(t)
print("wrote", DOCS, "cells", cells)

# The page's figures, from charts.py
STATIC = os.path.join(os.path.dirname(DOCS), "_static", "benchmarks")
os.makedirs(STATIC, exist_ok=True)
for name in ("time_vs_deps_cold", "time_vs_deps_warm", "cold_warm_ratio_n100", "scenarios_n100", "polyglot", "flag_deltas"):
    for variant in (name, f"{name}_dark"):  # light and dark page themes
        src = f"{ROOT}/report/charts/{variant}.svg"
        if os.path.exists(src):
            shutil.copyfile(src, f"{STATIC}/{variant}.svg")
        else:
            print("missing chart, run charts.py:", variant)
