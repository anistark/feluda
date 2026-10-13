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
WHAT = {
    "syft": "Catalogues packages from lockfiles and installed files without the network; it does not look licenses up, so 47 of 759 sampled packages carried one.",
    "trivy": "Reports licenses found in files and installed metadata, not lockfile packages. Exited 1 on netty and spring-petclinic when Maven Central rate limited this address.",
    "cdxgen": "Run with ``--no-install-deps`` so it reads manifests like Feluda does. Fetches licenses from registries, one package at a time (200 s on hyperfine).",
    "scancode": "Scans every file's content for license text, so it answers which licenses appear in the tree, not which ones dependencies declare. Up to 1.4 GB of memory.",
    "cargo-deny": "Reads installed Cargo metadata. Rust only, needs ``cargo fetch`` first.",
    "license-checker": "Reads installed ``node_modules``. Node only, needs an install first.",
    "pip-licenses": "Reads an installed Python environment. Python only, needs an install first.",
    "go-licenses": "Did not run: it fails on Go 1.27 (\"Package math does not have module info\").",
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
